#![allow(clippy::missing_errors_doc)]

use axum::{
    body::{Body, Bytes},
    http::header,
};
use chrono::{DateTime, FixedOffset, Local};
use futures_util::StreamExt;
use loco_rs::prelude::*;
use sea_orm::{
    ColumnTrait, Condition, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect, Select,
};
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
    /// Only logs at or after this time (RFC 3339, e.g. `2026-10-11T00:00:00+08:00`).
    pub start_time: Option<String>,
    /// Only logs at or before this time (RFC 3339).
    pub end_time: Option<String>,
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
    let query = filtered_query(&params)?.order_by_desc(operation_logs::Column::Id);

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

fn parse_time(value: Option<&str>, field: &str) -> ApiResult<Option<DateTime<FixedOffset>>> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            DateTime::parse_from_rfc3339(value)
                .map_err(|_| ApiError::bad_request(format!("{field} 不是有效的时间")))
        })
        .transpose()
}

/// Logs matching the list filters (shared by the list and the export).
fn filtered_query(params: &LogQueryParams) -> ApiResult<Select<operation_logs::Entity>> {
    let mut query = operation_logs::Entity::find();
    let start = parse_time(params.start_time.as_deref(), "开始时间")?;
    let end = parse_time(params.end_time.as_deref(), "结束时间")?;
    if let (Some(start), Some(end)) = (start, end) {
        if start > end {
            return Err(ApiError::bad_request("开始时间不能晚于结束时间"));
        }
    }
    if let Some(start) = start {
        query = query.filter(operation_logs::Column::CreatedAt.gte(start));
    }
    if let Some(end) = end {
        query = query.filter(operation_logs::Column::CreatedAt.lte(end));
    }

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
    Ok(query)
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

#[utoipa::path(
    get,
    path = "/api/admin/audit-rules",
    tag = "admin-logs",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<crate::services::audit::AuditSettings>))
)]
#[debug_handler]
pub async fn audit_rules(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:log:list").await?;
    Ok(responses::ok(
        crate::services::audit::settings(&ctx.db).await,
    ))
}

#[utoipa::path(
    put,
    path = "/api/admin/audit-rules",
    tag = "admin-logs",
    security(("bearer_auth" = [])),
    request_body = crate::services::audit::SaveAuditSettings,
    responses((status = 200, body = ApiResponse<crate::services::audit::AuditSettings>))
)]
#[debug_handler]
pub async fn save_audit_rules(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<crate::services::audit::SaveAuditSettings>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:log:config").await?;
    Ok(responses::ok(
        crate::services::audit::save_settings(&ctx.db, &params).await?,
    ))
}

/// Larger exports must be narrowed down with filters.
const MAX_EXPORT_ROWS: u64 = 200_000;
const EXPORT_BATCH: u64 = 1000;

const EXPORT_HEADERS: [&str; 18] = [
    "ID",
    "时间",
    "级别",
    "类型",
    "功能",
    "动作",
    "消息",
    "操作人",
    "用户ID",
    "IP",
    "请求方法",
    "请求路径",
    "状态码",
    "耗时(ms)",
    "请求内容",
    "错误信息",
    "User Agent",
    "追踪ID",
];

/// One CSV field. Values that a spreadsheet would run as a formula are
/// prefixed with `'`.
#[must_use]
pub fn csv_field(value: &str) -> String {
    let value = if value.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{value}")
    } else {
        value.to_string()
    };
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value
    }
}

fn type_label(log_type: &str) -> &str {
    match log_type {
        "request" => "接口请求",
        "login" => "登录",
        "operation" => "操作",
        "system" => "系统",
        "error" => "异常",
        other => other,
    }
}

#[must_use]
pub fn csv_row(log: &operation_logs::Model) -> String {
    let optional = |value: &Option<String>| value.clone().unwrap_or_default();
    let number = |value: Option<i32>| value.map(|value| value.to_string()).unwrap_or_default();
    let feature = crate::services::audit::feature_by_key(&log.module)
        .map_or(log.module.as_str(), |feature| feature.label);
    let fields = [
        log.id.to_string(),
        log.created_at
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
        log.level.clone(),
        type_label(&log.log_type).to_string(),
        feature.to_string(),
        log.action.clone(),
        log.message.clone(),
        optional(&log.operator),
        number(log.user_id),
        optional(&log.ip),
        optional(&log.method),
        optional(&log.path),
        number(log.status),
        number(log.duration_ms),
        optional(&log.request_summary),
        optional(&log.error_message),
        optional(&log.user_agent),
        optional(&log.trace_id),
    ];
    let mut row = fields
        .iter()
        .map(|field| csv_field(field))
        .collect::<Vec<_>>()
        .join(",");
    row.push_str("\r\n");
    row
}

/// Streams matching logs newest first, a batch at a time (keyset on id so
/// new rows arriving mid-export do not shift the pages).
fn export_stream(
    db: DatabaseConnection,
    query: Select<operation_logs::Entity>,
) -> impl futures_util::Stream<Item = Result<Bytes, std::io::Error>> {
    let header = format!("\u{feff}{}\r\n", EXPORT_HEADERS.join(","));
    let first =
        futures_util::stream::once(async move { Ok::<_, std::io::Error>(Bytes::from(header)) });
    let rows = futures_util::stream::unfold(Some(i32::MAX), move |cursor| {
        let db = db.clone();
        let query = query.clone();
        async move {
            let before = cursor?;
            let batch = query
                .filter(operation_logs::Column::Id.lt(before))
                .order_by_desc(operation_logs::Column::Id)
                .limit(EXPORT_BATCH)
                .all(&db)
                .await;
            match batch {
                Ok(rows) if rows.is_empty() => None,
                Ok(rows) => {
                    let next = rows.last().map(|row| row.id);
                    let chunk: String = rows.iter().map(csv_row).collect();
                    Some((Ok(Bytes::from(chunk)), next))
                }
                Err(error) => Some((Err(std::io::Error::other(error.to_string())), None)),
            }
        }
    });
    first.chain(rows)
}

#[utoipa::path(
    get,
    path = "/api/admin/logs/export",
    tag = "admin-logs",
    security(("bearer_auth" = [])),
    params(LogQueryParams),
    responses((status = 200, description = "CSV file (UTF-8 with BOM)", content_type = "text/csv"))
)]
#[debug_handler]
pub async fn export(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<LogQueryParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:log:export").await?;
    let query = filtered_query(&params)?;
    let total = query.clone().count(&ctx.db).await?;
    if total > MAX_EXPORT_ROWS {
        return Err(ApiError::bad_request(format!(
            "符合条件的日志有 {total} 条，超过单次导出上限 {MAX_EXPORT_ROWS} 条，请缩小时间范围或增加筛选条件"
        )));
    }
    let range = match (params.start_time.as_deref(), params.end_time.as_deref()) {
        (None, None) => "全部时间".to_string(),
        (start, end) => format!("{} 至 {}", start.unwrap_or("最早"), end.unwrap_or("现在")),
    };
    crate::models::admin_logs::record(
        &ctx.db,
        crate::models::admin_logs::LogInput {
            log_type: "operation",
            level: "info",
            module: "logs",
            action: "export",
            message: &format!("导出日志 {total} 条（{range}）"),
            user_id: Some(user.id),
            operator: Some(user.email.clone()),
            method: Some("GET"),
            path: Some("/api/admin/logs/export"),
            status: Some(200),
            error_message: None,
        },
    )
    .await;

    let filename = format!(
        "operation-logs-{}.csv",
        Local::now().format("%Y%m%d-%H%M%S")
    );
    Response::builder()
        .header(header::CONTENT_TYPE, "text/csv; charset=utf-8")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        )
        .header("x-total-count", total.to_string())
        .body(Body::from_stream(export_stream(ctx.db.clone(), query)))
        .map_err(|error| ApiError::internal(error.to_string()))
}

#[utoipa::path(
    get,
    path = "/api/admin/logs/retention",
    tag = "admin-logs",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<crate::services::log_retention::RetentionStatus>))
)]
#[debug_handler]
pub async fn retention(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:log:list").await?;
    Ok(responses::ok(
        crate::services::log_retention::status(&ctx.db).await?,
    ))
}

#[utoipa::path(
    put,
    path = "/api/admin/logs/retention",
    tag = "admin-logs",
    security(("bearer_auth" = [])),
    request_body = crate::services::log_retention::SaveRetentionParams,
    responses((status = 200, body = ApiResponse<crate::services::log_retention::RetentionStatus>))
)]
#[debug_handler]
pub async fn save_retention(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<crate::services::log_retention::SaveRetentionParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:log:config").await?;
    Ok(responses::ok(
        crate::services::log_retention::save(&ctx.db, user.id, params.retention_days).await?,
    ))
}

#[utoipa::path(
    post,
    path = "/api/admin/logs/retention/run",
    tag = "admin-logs",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<crate::services::log_retention::CleanupResult>))
)]
#[debug_handler]
pub async fn run_retention(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:log:delete").await?;
    let result = crate::services::log_retention::run_cleanup(&ctx.db, "manual").await;
    if let Some(error) = &result.error {
        return Err(ApiError::internal(format!("清理失败：{error}")));
    }
    Ok(responses::ok(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_fields_are_quoted_and_formula_safe() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_field("line\nbreak"), "\"line\nbreak\"");
        assert_eq!(csv_field("=SUM(A1)"), "'=SUM(A1)");
        assert_eq!(csv_field("-1+1"), "'-1+1");
        assert_eq!(csv_field("中文"), "中文");
    }
}
