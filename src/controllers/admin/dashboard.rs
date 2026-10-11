#![allow(clippy::missing_errors_doc)]

use std::collections::HashSet;

use chrono::{DateTime, Duration, FixedOffset, Local, NaiveTime, Utc};
use loco_rs::prelude::*;
use sea_orm::{
    ColumnTrait, Condition, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect,
};
use serde::{Deserialize, Serialize};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

use crate::{
    controllers::admin::{authorize, notifications, payments, users_admin, work_orders},
    errors::ApiResult,
    models::{
        _entities::{
            operation_logs, payment_orders, payment_refunds, scheduled_task_runs,
            system_notifications, users, work_orders as work_order_entity,
        },
        rbac,
    },
    responses::{self, ApiResponse},
};

const ACTIVE_WORK_ORDER_STATUSES: [&str; 4] = ["open", "assigned", "in_progress", "pending"];
const PENDING_PAYMENT_STATUSES: [&str; 2] = ["pending", "paying"];
const TREND_DAYS: i64 = 7;
const RECENT_LIMIT: u64 = 5;
const RECENT_LOG_LIMIT: u64 = 6;

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct DashboardOverview {
    pub generated_at: String,
    pub system: DashboardSystemStatus,
    pub stats: DashboardStats,
    /// Pending items the current administrator is allowed to act on.
    pub todos: Vec<DashboardTodo>,
    pub my_work_orders: Option<Vec<DashboardWorkOrder>>,
    pub recent_notifications: Option<Vec<DashboardNotification>>,
    pub recent_logs: Option<Vec<DashboardLog>>,
    pub activity_trend: Option<Vec<DashboardTrendPoint>>,
    pub recent_logins: Vec<DashboardLogin>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct DashboardSystemStatus {
    pub db_ok: bool,
    pub version: String,
    pub environment: String,
    pub uptime_seconds: u64,
}

/// Each section is `None` when the current administrator lacks the matching
/// list permission, so the page never leaks numbers the user cannot browse.
#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct DashboardStats {
    pub users: Option<DashboardUserStats>,
    pub work_orders: Option<DashboardWorkOrderStats>,
    pub payments: Option<DashboardPaymentStats>,
    pub notifications: Option<DashboardNotificationStats>,
    pub activity: Option<DashboardActivityStats>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct DashboardUserStats {
    pub total: u64,
    pub new_today: u64,
    pub locked: u64,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct DashboardWorkOrderStats {
    pub active: u64,
    pub unassigned: u64,
    pub assigned_to_me: u64,
    pub overdue: u64,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct DashboardPaymentStats {
    pub today_orders: u64,
    pub today_paid: u64,
    pub pending: u64,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct DashboardNotificationStats {
    pub total: u64,
    pub unread: u64,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct DashboardActivityStats {
    pub today_operations: u64,
    pub today_logins: u64,
    pub today_errors: u64,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct DashboardTodo {
    pub key: String,
    pub title: String,
    pub description: String,
    pub count: u64,
    pub path: String,
    /// `info`, `warning` or `error`.
    pub level: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct DashboardWorkOrder {
    pub id: i32,
    pub order_no: String,
    pub title: String,
    pub priority: String,
    pub status: String,
    pub due_at: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct DashboardNotification {
    pub id: i32,
    pub title: String,
    pub level: String,
    pub category: String,
    pub read: bool,
    pub created_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct DashboardLog {
    pub id: i32,
    pub log_type: String,
    pub level: String,
    pub module: String,
    pub action: String,
    pub message: String,
    pub operator: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct DashboardTrendPoint {
    /// Local calendar date, `YYYY-MM-DD`.
    pub date: String,
    pub total: u64,
    pub errors: u64,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct DashboardLogin {
    pub success: bool,
    pub message: String,
    pub ip: Option<String>,
    pub created_at: String,
}

struct Viewer {
    user: users::Model,
    super_admin: bool,
    permissions: HashSet<String>,
}

impl Viewer {
    async fn load(ctx: &AppContext, user: users::Model) -> ApiResult<Self> {
        let super_admin = rbac::is_super_admin(&ctx.db, user.id).await?;
        let permissions = if super_admin {
            HashSet::new()
        } else {
            rbac::load_user_permissions(&ctx.db, user.id)
                .await?
                .into_iter()
                .map(|permission| permission.code)
                .collect()
        };
        Ok(Self {
            user,
            super_admin,
            permissions,
        })
    }

    fn can(&self, code: &str) -> bool {
        self.super_admin || self.permissions.contains(code)
    }
}

#[utoipa::path(
    get,
    path = "/api/admin/dashboard/overview",
    tag = "admin-dashboard",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<DashboardOverview>))
)]
#[debug_handler]
pub async fn overview(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "dashboard:view").await?;
    let viewer = Viewer::load(&ctx, user).await?;
    let today = local_day_start(0);
    let mut todos = Vec::new();

    let users = user_section(&ctx, &viewer, today, &mut todos).await?;
    let (work_orders, my_work_orders) = work_order_section(&ctx, &viewer, &mut todos).await?;
    let payments = payment_section(&ctx, &viewer, today, &mut todos).await?;
    let (notifications, recent_notifications) =
        notification_section(&ctx, &viewer, &mut todos).await?;
    scheduled_task_section(&ctx, &viewer, &mut todos).await?;
    let (activity, recent_logs, activity_trend) = activity_section(&ctx, &viewer, today).await?;

    Ok(responses::ok(DashboardOverview {
        generated_at: Local::now().to_rfc3339(),
        system: system_status(&ctx).await,
        stats: DashboardStats {
            users,
            work_orders,
            payments,
            notifications,
            activity,
        },
        todos,
        my_work_orders,
        recent_notifications,
        recent_logs,
        activity_trend,
        recent_logins: load_recent_logins(&ctx, viewer.user.id).await?,
    }))
}

async fn user_section(
    ctx: &AppContext,
    viewer: &Viewer,
    today: DateTime<FixedOffset>,
    todos: &mut Vec<DashboardTodo>,
) -> ApiResult<Option<DashboardUserStats>> {
    if !viewer.can("system:user:list") {
        return Ok(None);
    }
    let stats = user_stats(ctx, viewer, today).await?;
    todos.push(todo(
        "locked_users",
        "锁定账号",
        "登录失败次数过多被锁定的账号",
        stats.locked,
        "/admin/system/users",
        "warning",
    ));
    Ok(Some(stats))
}

async fn work_order_section(
    ctx: &AppContext,
    viewer: &Viewer,
    todos: &mut Vec<DashboardTodo>,
) -> ApiResult<(
    Option<DashboardWorkOrderStats>,
    Option<Vec<DashboardWorkOrder>>,
)> {
    if !viewer.can("system:work_order:list") {
        return Ok((None, None));
    }
    let stats = work_order_stats(ctx, viewer).await?;
    for (key, title, description, count, level) in [
        (
            "unassigned_work_orders",
            "待分派工单",
            "新提交且尚未指派处理人的工单",
            stats.unassigned,
            "warning",
        ),
        (
            "my_work_orders",
            "我负责的工单",
            "指派给我且尚未解决的工单",
            stats.assigned_to_me,
            "info",
        ),
        (
            "overdue_work_orders",
            "已逾期工单",
            "超过截止时间仍未解决的工单",
            stats.overdue,
            "error",
        ),
    ] {
        todos.push(todo(
            key,
            title,
            description,
            count,
            "/admin/system/work-orders",
            level,
        ));
    }
    let mine = load_my_work_orders(ctx, viewer).await?;
    Ok((Some(stats), Some(mine)))
}

async fn payment_section(
    ctx: &AppContext,
    viewer: &Viewer,
    today: DateTime<FixedOffset>,
    todos: &mut Vec<DashboardTodo>,
) -> ApiResult<Option<DashboardPaymentStats>> {
    if viewer.can("system:payment_refund:list") {
        let mut query =
            payment_refunds::Entity::find().filter(payment_refunds::Column::Status.eq("pending"));
        if !viewer.super_admin {
            query = query.filter(payments::refund_visible_condition(&viewer.user));
        }
        todos.push(todo(
            "pending_refunds",
            "待审核退款",
            "等待审核的退款申请",
            query.count(&ctx.db).await?,
            "/admin/system/payments",
            "warning",
        ));
    }
    if !viewer.can("system:payment_order:list") {
        return Ok(None);
    }
    Ok(Some(payment_stats(ctx, viewer, today).await?))
}

async fn notification_section(
    ctx: &AppContext,
    viewer: &Viewer,
    todos: &mut Vec<DashboardTodo>,
) -> ApiResult<(
    Option<DashboardNotificationStats>,
    Option<Vec<DashboardNotification>>,
)> {
    if !viewer.can("system:notification:list") {
        return Ok((None, None));
    }
    let (stats, recent) = notification_overview(ctx, viewer).await?;
    todos.push(todo(
        "unread_notifications",
        "未读通知",
        "尚未阅读的系统通知",
        stats.unread,
        "/admin/system/notifications",
        "info",
    ));
    Ok((Some(stats), Some(recent)))
}

async fn scheduled_task_section(
    ctx: &AppContext,
    viewer: &Viewer,
    todos: &mut Vec<DashboardTodo>,
) -> ApiResult<()> {
    if !viewer.can("system:scheduled_task:list") {
        return Ok(());
    }
    let failed_runs = scheduled_task_runs::Entity::find()
        .filter(scheduled_task_runs::Column::Status.eq("failed"))
        .filter(scheduled_task_runs::Column::CreatedAt.gte(Utc::now() - Duration::hours(24)))
        .count(&ctx.db)
        .await?;
    todos.push(todo(
        "failed_task_runs",
        "失败的定时任务",
        "最近 24 小时内执行失败的任务",
        failed_runs,
        "/admin/system/scheduled-tasks",
        "error",
    ));
    Ok(())
}

async fn activity_section(
    ctx: &AppContext,
    viewer: &Viewer,
    today: DateTime<FixedOffset>,
) -> ApiResult<(
    Option<DashboardActivityStats>,
    Option<Vec<DashboardLog>>,
    Option<Vec<DashboardTrendPoint>>,
)> {
    if !viewer.can("system:log:list") {
        return Ok((None, None, None));
    }
    Ok((
        Some(activity_stats(ctx, today).await?),
        Some(load_recent_logs(ctx).await?),
        Some(load_activity_trend(ctx).await?),
    ))
}

async fn user_stats(
    ctx: &AppContext,
    viewer: &Viewer,
    today: DateTime<FixedOffset>,
) -> ApiResult<DashboardUserStats> {
    let scope = rbac::resolve_data_scope(&ctx.db, &viewer.user).await?;
    let scoped = || users_admin::apply_user_scope(users::Entity::find(), &scope);
    Ok(DashboardUserStats {
        total: scoped().count(&ctx.db).await?,
        new_today: scoped()
            .filter(users::Column::CreatedAt.gte(today))
            .count(&ctx.db)
            .await?,
        locked: scoped()
            .filter(users::Column::LockedUntil.gt(Utc::now()))
            .count(&ctx.db)
            .await?,
    })
}

async fn work_order_stats(ctx: &AppContext, viewer: &Viewer) -> ApiResult<DashboardWorkOrderStats> {
    let active = || {
        let query = work_order_entity::Entity::find()
            .filter(work_order_entity::Column::Status.is_in(ACTIVE_WORK_ORDER_STATUSES));
        if viewer.super_admin {
            query
        } else {
            query.filter(work_orders::visible_condition(&viewer.user))
        }
    };
    Ok(DashboardWorkOrderStats {
        active: active().count(&ctx.db).await?,
        unassigned: active()
            .filter(work_order_entity::Column::Status.eq("open"))
            .filter(work_order_entity::Column::AssigneeId.is_null())
            .count(&ctx.db)
            .await?,
        assigned_to_me: active()
            .filter(work_order_entity::Column::AssigneeId.eq(viewer.user.id))
            .count(&ctx.db)
            .await?,
        overdue: active()
            .filter(work_order_entity::Column::DueAt.lt(Utc::now()))
            .count(&ctx.db)
            .await?,
    })
}

async fn load_my_work_orders(
    ctx: &AppContext,
    viewer: &Viewer,
) -> ApiResult<Vec<DashboardWorkOrder>> {
    let items = work_order_entity::Entity::find()
        .filter(work_order_entity::Column::Status.is_in(ACTIVE_WORK_ORDER_STATUSES))
        .filter(
            Condition::any()
                .add(work_order_entity::Column::AssigneeId.eq(viewer.user.id))
                .add(work_order_entity::Column::CreatorId.eq(viewer.user.id)),
        )
        .order_by_desc(work_order_entity::Column::UpdatedAt)
        .limit(RECENT_LIMIT)
        .all(&ctx.db)
        .await?;
    Ok(items
        .into_iter()
        .map(|item| DashboardWorkOrder {
            id: item.id,
            order_no: item.order_no,
            title: item.title,
            priority: item.priority,
            status: item.status,
            due_at: item.due_at.map(|value| value.to_rfc3339()),
            updated_at: item.updated_at.to_rfc3339(),
        })
        .collect())
}

async fn payment_stats(
    ctx: &AppContext,
    viewer: &Viewer,
    today: DateTime<FixedOffset>,
) -> ApiResult<DashboardPaymentStats> {
    let visible = || {
        let query = payment_orders::Entity::find();
        if viewer.super_admin {
            query
        } else {
            query.filter(payments::order_visible_condition(&viewer.user))
        }
    };
    Ok(DashboardPaymentStats {
        today_orders: visible()
            .filter(payment_orders::Column::CreatedAt.gte(today))
            .count(&ctx.db)
            .await?,
        today_paid: visible()
            .filter(payment_orders::Column::PaidAt.gte(today))
            .count(&ctx.db)
            .await?,
        pending: visible()
            .filter(payment_orders::Column::Status.is_in(PENDING_PAYMENT_STATUSES))
            .count(&ctx.db)
            .await?,
    })
}

async fn notification_overview(
    ctx: &AppContext,
    viewer: &Viewer,
) -> ApiResult<(DashboardNotificationStats, Vec<DashboardNotification>)> {
    let visible = || {
        let query = system_notifications::Entity::find();
        if viewer.super_admin {
            query
        } else {
            query.filter(notifications::visible_condition(&viewer.user))
        }
    };
    let stats = DashboardNotificationStats {
        total: visible().count(&ctx.db).await?,
        unread: visible()
            .filter(system_notifications::Column::ReadAt.is_null())
            .count(&ctx.db)
            .await?,
    };
    let recent = visible()
        .order_by_desc(system_notifications::Column::Id)
        .limit(RECENT_LIMIT)
        .all(&ctx.db)
        .await?
        .into_iter()
        .map(|item| DashboardNotification {
            id: item.id,
            title: item.title,
            level: item.level,
            category: item.category,
            read: item.read_at.is_some(),
            created_at: item.created_at.to_rfc3339(),
        })
        .collect();
    Ok((stats, recent))
}

async fn activity_stats(
    ctx: &AppContext,
    today: DateTime<FixedOffset>,
) -> ApiResult<DashboardActivityStats> {
    let since_today =
        || operation_logs::Entity::find().filter(operation_logs::Column::CreatedAt.gte(today));
    Ok(DashboardActivityStats {
        today_operations: since_today()
            .filter(operation_logs::Column::LogType.ne("login"))
            .count(&ctx.db)
            .await?,
        today_logins: since_today()
            .filter(operation_logs::Column::Action.eq("login_success"))
            .count(&ctx.db)
            .await?,
        today_errors: since_today()
            .filter(operation_logs::Column::Level.eq("error"))
            .count(&ctx.db)
            .await?,
    })
}

async fn load_recent_logs(ctx: &AppContext) -> ApiResult<Vec<DashboardLog>> {
    let items = operation_logs::Entity::find()
        .order_by_desc(operation_logs::Column::Id)
        .limit(RECENT_LOG_LIMIT)
        .all(&ctx.db)
        .await?;
    Ok(items
        .into_iter()
        .map(|item| DashboardLog {
            id: item.id,
            log_type: item.log_type,
            level: item.level,
            module: item.module,
            action: item.action,
            message: item.message,
            operator: item.operator,
            created_at: item.created_at.to_rfc3339(),
        })
        .collect())
}

async fn load_activity_trend(ctx: &AppContext) -> ApiResult<Vec<DashboardTrendPoint>> {
    let mut points = Vec::new();
    for offset in (0..TREND_DAYS).rev() {
        let start = local_day_start(offset);
        let end = start + Duration::days(1);
        let in_day = || {
            operation_logs::Entity::find()
                .filter(operation_logs::Column::CreatedAt.gte(start))
                .filter(operation_logs::Column::CreatedAt.lt(end))
        };
        points.push(DashboardTrendPoint {
            date: start.format("%Y-%m-%d").to_string(),
            total: in_day().count(&ctx.db).await?,
            errors: in_day()
                .filter(operation_logs::Column::Level.eq("error"))
                .count(&ctx.db)
                .await?,
        });
    }
    Ok(points)
}

async fn load_recent_logins(ctx: &AppContext, user_id: i32) -> ApiResult<Vec<DashboardLogin>> {
    let items = operation_logs::Entity::find()
        .filter(operation_logs::Column::UserId.eq(user_id))
        .filter(operation_logs::Column::LogType.eq("login"))
        .order_by_desc(operation_logs::Column::Id)
        .limit(RECENT_LIMIT)
        .all(&ctx.db)
        .await?;
    Ok(items
        .into_iter()
        .map(|item| DashboardLogin {
            success: item.action == "login_success",
            message: item.message,
            ip: item.ip,
            created_at: item.created_at.to_rfc3339(),
        })
        .collect())
}

async fn system_status(ctx: &AppContext) -> DashboardSystemStatus {
    DashboardSystemStatus {
        db_ok: ctx.db.ping().await.is_ok(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        environment: ctx.environment.to_string(),
        uptime_seconds: process_uptime_seconds(),
    }
}

fn process_uptime_seconds() -> u64 {
    let Ok(pid) = sysinfo::get_current_pid() else {
        return 0;
    };
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        false,
        ProcessRefreshKind::nothing(),
    );
    system.process(pid).map_or(0, sysinfo::Process::run_time)
}

/// Start of the server-local calendar day `days_ago` days before today.
fn local_day_start(days_ago: i64) -> DateTime<FixedOffset> {
    let date = Local::now().date_naive() - Duration::days(days_ago);
    let naive = date.and_time(NaiveTime::MIN);
    naive.and_local_timezone(Local).earliest().map_or_else(
        || naive.and_utc().fixed_offset(),
        |value| value.fixed_offset(),
    )
}

fn todo(
    key: &str,
    title: &str,
    description: &str,
    count: u64,
    path: &str,
    level: &str,
) -> DashboardTodo {
    DashboardTodo {
        key: key.to_string(),
        title: title.to_string(),
        description: description.to_string(),
        count,
        path: path.to_string(),
        level: level.to_string(),
    }
}
