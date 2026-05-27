#![allow(clippy::missing_errors_doc)]

use loco_rs::prelude::*;
use sea_orm::{ColumnTrait, Condition, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder};
use serde::{Deserialize, Serialize};

use crate::{
    controllers::admin::authorize,
    errors::{ApiError, ApiResult},
    models::_entities::operation_logs,
    responses::{self, ApiResponse, EmptyData, PageResponse},
};

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct LogQueryParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub keyword: Option<String>,
    pub level: Option<String>,
    pub log_type: Option<String>,
    pub module: Option<String>,
    pub status: Option<i32>,
}

impl LogQueryParams {
    #[must_use]
    pub fn page(&self) -> u64 {
        self.page.unwrap_or(1).max(1)
    }

    #[must_use]
    pub fn page_size(&self) -> u64 {
        self.page_size.unwrap_or(10).clamp(1, 100)
    }
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct LogRecord {
    pub id: i32,
    pub trace_id: Option<String>,
    pub log_type: String,
    pub level: String,
    pub module: String,
    pub action: String,
    pub message: String,
    pub method: Option<String>,
    pub path: Option<String>,
    pub status: Option<i32>,
    pub duration_ms: Option<i32>,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub user_id: Option<i32>,
    pub operator: Option<String>,
    pub request_summary: Option<String>,
    pub response_summary: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[utoipa::path(
    get,
    path = "/api/admin/logs",
    tag = "admin-logs",
    security(("bearer_auth" = [])),
    params(LogQueryParams),
    responses((status = 200, body = ApiResponse<PageResponse<LogRecord>>))
)]
#[debug_handler]
pub async fn list(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<LogQueryParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:log:list").await?;

    let page = params.page();
    let page_size = params.page_size();
    let mut query = operation_logs::Entity::find().order_by_desc(operation_logs::Column::Id);

    if let Some(keyword) = params
        .keyword
        .as_deref()
        .filter(|keyword| !keyword.is_empty())
    {
        query = query.filter(
            Condition::any()
                .add(operation_logs::Column::Message.contains(keyword))
                .add(operation_logs::Column::Module.contains(keyword))
                .add(operation_logs::Column::Action.contains(keyword))
                .add(operation_logs::Column::Operator.contains(keyword))
                .add(operation_logs::Column::Path.contains(keyword)),
        );
    }
    if let Some(level) = params.level.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(operation_logs::Column::Level.eq(level));
    }
    if let Some(log_type) = params.log_type.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(operation_logs::Column::LogType.eq(log_type));
    }
    if let Some(module) = params.module.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(operation_logs::Column::Module.eq(module));
    }
    if let Some(status) = params.status {
        query = query.filter(operation_logs::Column::Status.eq(status));
    }

    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(LogRecord::from)
        .collect();

    Ok(responses::ok(PageResponse {
        items,
        page,
        page_size,
        total,
    }))
}

#[utoipa::path(
    get,
    path = "/api/admin/logs/{id}",
    tag = "admin-logs",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<LogRecord>))
)]
#[debug_handler]
pub async fn get(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:log:detail").await?;
    let log = find_log(&ctx, id).await?;
    Ok(responses::ok(LogRecord::from(log)))
}

#[utoipa::path(
    delete,
    path = "/api/admin/logs/{id}",
    tag = "admin-logs",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn delete(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:log:delete").await?;
    find_log(&ctx, id).await?;
    operation_logs::Entity::delete_by_id(id)
        .exec(&ctx.db)
        .await?;
    Ok(responses::empty())
}

async fn find_log(ctx: &AppContext, id: i32) -> ApiResult<operation_logs::Model> {
    operation_logs::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("log not found"))
}

impl From<operation_logs::Model> for LogRecord {
    fn from(log: operation_logs::Model) -> Self {
        Self {
            id: log.id,
            trace_id: log.trace_id,
            log_type: log.log_type,
            level: log.level,
            module: log.module,
            action: log.action,
            message: log.message,
            method: log.method,
            path: log.path,
            status: log.status,
            duration_ms: log.duration_ms,
            ip: log.ip,
            user_agent: log.user_agent,
            user_id: log.user_id,
            operator: log.operator,
            request_summary: log.request_summary,
            response_summary: log.response_summary,
            error_message: log.error_message,
            created_at: log.created_at.to_rfc3339(),
            updated_at: log.updated_at.to_rfc3339(),
        }
    }
}
