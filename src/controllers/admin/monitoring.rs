#![allow(clippy::missing_errors_doc)]

use loco_rs::prelude::*;
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};
use serde::{Deserialize, Serialize};

use crate::{
    controllers::admin::authorize,
    errors::ApiResult,
    models::_entities::{database_backups, operation_logs, rate_limit_events, scheduled_task_runs},
    responses::{self, ApiResponse},
};

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct MonitoringOverview {
    pub db_ok: bool,
    pub task_success_count: u64,
    pub task_failed_count: u64,
    pub backup_success_count: u64,
    pub backup_failed_count: u64,
    pub rate_limit_event_count: u64,
    pub error_log_count: u64,
    pub health_links: Vec<String>,
}

#[utoipa::path(
    get,
    path = "/api/admin/monitoring/overview",
    tag = "admin-monitoring",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<MonitoringOverview>))
)]
#[debug_handler]
pub async fn overview(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:monitor:view").await?;

    let db_ok = ctx.db.ping().await.is_ok();
    let task_success_count = scheduled_task_runs::Entity::find()
        .filter(scheduled_task_runs::Column::Status.eq("success"))
        .count(&ctx.db)
        .await?;
    let task_failed_count = scheduled_task_runs::Entity::find()
        .filter(scheduled_task_runs::Column::Status.eq("failed"))
        .count(&ctx.db)
        .await?;
    let backup_success_count = database_backups::Entity::find()
        .filter(database_backups::Column::Status.eq("success"))
        .count(&ctx.db)
        .await?;
    let backup_failed_count = database_backups::Entity::find()
        .filter(database_backups::Column::Status.eq("failed"))
        .count(&ctx.db)
        .await?;
    let rate_limit_event_count = rate_limit_events::Entity::find().count(&ctx.db).await?;
    let error_log_count = operation_logs::Entity::find()
        .filter(operation_logs::Column::Level.eq("error"))
        .count(&ctx.db)
        .await?;

    Ok(responses::ok(MonitoringOverview {
        db_ok,
        task_success_count,
        task_failed_count,
        backup_success_count,
        backup_failed_count,
        rate_limit_event_count,
        error_log_count,
        health_links: vec![
            "/_health".to_string(),
            "/_readiness".to_string(),
            "/_ping".to_string(),
        ],
    }))
}
