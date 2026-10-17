//! Deletes operation logs older than `security.log_retention_days`
//! (0 keeps them forever). Runs in the background and on demand.

#![allow(clippy::missing_errors_doc)]

use std::{
    sync::{Mutex, OnceLock},
    time::Duration,
};

use chrono::{DateTime, FixedOffset, Local};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect, Set,
};
use serde::{Deserialize, Serialize};

use crate::{
    errors::{ApiError, ApiResult},
    models::{
        _entities::{operation_logs, system_settings},
        system_settings as settings,
    },
};

pub const SETTING_KEY: &str = "security.log_retention_days";
pub const DEFAULT_RETENTION_DAYS: i64 = 90;
pub const MAX_RETENTION_DAYS: i64 = 3650;
// Keep compatibility with stable Rust versions before duration_constructors.
#[allow(clippy::duration_suboptimal_units)]
const FIRST_RUN_DELAY: Duration = Duration::from_secs(60);
#[allow(clippy::duration_suboptimal_units)]
const RUN_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// Rows deleted per statement, so the table is never locked for long.
const BATCH: u64 = 5000;

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct CleanupResult {
    #[schema(value_type = String, format = DateTime)]
    pub ran_at: DateTime<FixedOffset>,
    pub deleted: u64,
    /// `manual` or `schedule`.
    pub trigger: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct RetentionStatus {
    /// 0 keeps logs forever.
    pub retention_days: i64,
    /// Logs currently older than the retention period.
    pub expired_count: u64,
    pub total_count: u64,
    #[schema(value_type = Option<String>, format = DateTime)]
    pub oldest_at: Option<DateTime<FixedOffset>>,
    pub last_cleanup: Option<CleanupResult>,
    #[schema(value_type = Option<String>, format = DateTime)]
    pub next_run_at: Option<DateTime<FixedOffset>>,
    pub interval_hours: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveRetentionParams {
    pub retention_days: i64,
}

#[derive(Default)]
struct CleanerState {
    last: Option<CleanupResult>,
    next_run_at: Option<DateTime<FixedOffset>>,
}

fn state() -> &'static Mutex<CleanerState> {
    static STATE: OnceLock<Mutex<CleanerState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(CleanerState::default()))
}

pub async fn retention_days(db: &DatabaseConnection) -> ApiResult<i64> {
    let days = settings::number_i64(db, SETTING_KEY, DEFAULT_RETENTION_DAYS).await?;
    Ok(days.clamp(0, MAX_RETENTION_DAYS))
}

fn cutoff(days: i64) -> DateTime<FixedOffset> {
    (Local::now() - chrono::Duration::days(days)).fixed_offset()
}

/// Deletes logs older than `days` in batches; returns how many went.
pub async fn purge_older_than(db: &DatabaseConnection, days: i64) -> ApiResult<u64> {
    if days <= 0 {
        return Ok(0);
    }
    let before = cutoff(days);
    let mut deleted = 0;
    loop {
        let ids: Vec<i32> = operation_logs::Entity::find()
            .select_only()
            .column(operation_logs::Column::Id)
            .filter(operation_logs::Column::CreatedAt.lt(before))
            .order_by_asc(operation_logs::Column::Id)
            .limit(BATCH)
            .into_tuple()
            .all(db)
            .await?;
        if ids.is_empty() {
            break;
        }
        let count = ids.len();
        deleted += operation_logs::Entity::delete_many()
            .filter(operation_logs::Column::Id.is_in(ids))
            .exec(db)
            .await?
            .rows_affected;
        if (count as u64) < BATCH {
            break;
        }
        tokio::task::yield_now().await;
    }
    Ok(deleted)
}

/// One cleanup pass using the configured retention; remembers the result
/// and notes it in the log when anything was removed.
pub async fn run_cleanup(db: &DatabaseConnection, trigger: &str) -> CleanupResult {
    let outcome = match retention_days(db).await {
        Ok(days) => purge_older_than(db, days)
            .await
            .map(|deleted| (days, deleted)),
        Err(error) => Err(error),
    };
    let result = CleanupResult {
        ran_at: Local::now().fixed_offset(),
        deleted: outcome.as_ref().map_or(0, |(_, deleted)| *deleted),
        trigger: trigger.to_string(),
        error: outcome
            .as_ref()
            .err()
            .map(|error| error.message().to_string()),
    };
    match &outcome {
        Ok((days, deleted)) if *deleted > 0 => {
            let label = if trigger == "manual" {
                "手动清理"
            } else {
                "自动清理"
            };
            let now = Local::now().fixed_offset();
            let _ = operation_logs::ActiveModel {
                log_type: Set("system".to_string()),
                level: Set("info".to_string()),
                module: Set("logs".to_string()),
                action: Set("cleanup".to_string()),
                message: Set(format!("{label} {deleted} 条超过 {days} 天的日志")),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            }
            .insert(db)
            .await;
            tracing::info!(deleted, days, trigger, "old operation logs removed");
        }
        Err(error) => {
            tracing::warn!(error = error.message(), "log cleanup failed");
        }
        _ => {}
    }
    if let Ok(mut state) = state().lock() {
        state.last = Some(result.clone());
    }
    result
}

/// Background cleaner: shortly after start, then every `RUN_INTERVAL`.
pub fn start_cleaner(db: DatabaseConnection) {
    tokio::spawn(async move {
        let mut delay = FIRST_RUN_DELAY;
        loop {
            if let Ok(mut state) = state().lock() {
                state.next_run_at = Some(
                    (Local::now() + chrono::Duration::from_std(delay).unwrap_or_default())
                        .fixed_offset(),
                );
            }
            tokio::time::sleep(delay).await;
            run_cleanup(&db, "schedule").await;
            delay = RUN_INTERVAL;
        }
    });
}

pub async fn status(db: &DatabaseConnection) -> ApiResult<RetentionStatus> {
    let days = retention_days(db).await?;
    let expired_count = if days > 0 {
        operation_logs::Entity::find()
            .filter(operation_logs::Column::CreatedAt.lt(cutoff(days)))
            .count(db)
            .await?
    } else {
        0
    };
    let total_count = operation_logs::Entity::find().count(db).await?;
    let oldest_at = operation_logs::Entity::find()
        .order_by_asc(operation_logs::Column::CreatedAt)
        .one(db)
        .await?
        .map(|log| log.created_at);
    let (last_cleanup, next_run_at) = state()
        .lock()
        .map(|state| (state.last.clone(), state.next_run_at))
        .unwrap_or_default();
    Ok(RetentionStatus {
        retention_days: days,
        expired_count,
        total_count,
        oldest_at,
        last_cleanup,
        next_run_at,
        interval_hours: RUN_INTERVAL.as_secs() / 3600,
    })
}

/// Stores the retention in system settings (creating the row if missing).
pub async fn save(db: &DatabaseConnection, user_id: i32, days: i64) -> ApiResult<RetentionStatus> {
    if !(0..=MAX_RETENTION_DAYS).contains(&days) {
        return Err(ApiError::bad_request(format!(
            "保留天数需在 0 到 {MAX_RETENTION_DAYS} 之间（0 表示永久保留）"
        )));
    }
    let now = Local::now().fixed_offset();
    let existing = system_settings::Entity::find()
        .filter(system_settings::Column::Key.eq(SETTING_KEY))
        .one(db)
        .await?;
    if let Some(existing) = existing {
        let mut active: system_settings::ActiveModel = existing.into();
        active.value = Set(days.to_string());
        active.updated_by = Set(Some(user_id));
        active.updated_at = Set(now);
        active.update(db).await?;
    } else {
        system_settings::ActiveModel {
            key: Set(SETTING_KEY.to_string()),
            name: Set("日志保留天数".to_string()),
            group_key: Set("security".to_string()),
            value: Set(days.to_string()),
            value_type: Set("number".to_string()),
            default_value: Set(Some(DEFAULT_RETENTION_DAYS.to_string())),
            description: Set(Some(RETENTION_DESCRIPTION.to_string())),
            is_public: Set(false),
            is_builtin: Set(true),
            is_encrypted: Set(false),
            sort_order: Set(40),
            updated_by: Set(Some(user_id)),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await?;
    }
    status(db).await
}

pub const RETENTION_DESCRIPTION: &str =
    "操作日志保留天数，超过的日志每 6 小时自动清理一次；0 表示永久保留";
