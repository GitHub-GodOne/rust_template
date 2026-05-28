#![allow(clippy::missing_errors_doc)]

use loco_rs::prelude::*;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, Set,
};
use serde::{Deserialize, Serialize};

use crate::{
    controllers::admin::authorize,
    errors::{ApiError, ApiResult},
    models::_entities::{rate_limit_events, rate_limit_rules},
    responses::{self, ApiResponse, EmptyData, PageResponse},
};

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct RateLimitQueryParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub enabled: Option<bool>,
}

impl RateLimitQueryParams {
    #[must_use]
    pub fn page(&self) -> u64 {
        self.page.unwrap_or(1).max(1)
    }

    #[must_use]
    pub fn page_size(&self) -> u64 {
        self.page_size.unwrap_or(10).clamp(1, 100)
    }
}

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct RateLimitEventQueryParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub rule_id: Option<i32>,
    pub ip: Option<String>,
}

impl RateLimitEventQueryParams {
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
pub struct RateLimitRuleRecord {
    pub id: i32,
    pub name: String,
    pub scope: String,
    pub path_pattern: String,
    pub method: Option<String>,
    pub limit_count: i32,
    pub window_seconds: i32,
    pub enabled: bool,
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct RateLimitEventRecord {
    pub id: i32,
    pub ip: String,
    pub method: String,
    pub path: String,
    pub rule_id: Option<i32>,
    pub user_id: Option<i32>,
    pub occurred_at: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveRateLimitRuleParams {
    pub name: String,
    pub scope: String,
    pub path_pattern: String,
    pub method: Option<String>,
    pub limit_count: i32,
    pub window_seconds: i32,
    pub enabled: Option<bool>,
    pub description: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/admin/rate-limits",
    tag = "admin-rate-limits",
    security(("bearer_auth" = [])),
    params(RateLimitQueryParams),
    responses((status = 200, body = ApiResponse<PageResponse<RateLimitRuleRecord>>))
)]
#[debug_handler]
pub async fn list(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<RateLimitQueryParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:rate_limit:list").await?;
    let page = params.page();
    let page_size = params.page_size();
    let mut query = rate_limit_rules::Entity::find().order_by_desc(rate_limit_rules::Column::Id);

    if let Some(enabled) = params.enabled {
        query = query.filter(rate_limit_rules::Column::Enabled.eq(enabled));
    }

    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(RateLimitRuleRecord::from)
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
    path = "/api/admin/rate-limits/{id}",
    tag = "admin-rate-limits",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<RateLimitRuleRecord>))
)]
#[debug_handler]
pub async fn get(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:rate_limit:list").await?;
    let rule = find_rule(&ctx, id).await?;
    Ok(responses::ok(RateLimitRuleRecord::from(rule)))
}

#[utoipa::path(
    post,
    path = "/api/admin/rate-limits",
    tag = "admin-rate-limits",
    security(("bearer_auth" = [])),
    request_body = SaveRateLimitRuleParams,
    responses((status = 200, body = ApiResponse<RateLimitRuleRecord>))
)]
#[debug_handler]
pub async fn create(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SaveRateLimitRuleParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:rate_limit:create").await?;
    validate_rule(&params)?;

    let rule = rate_limit_rules::ActiveModel {
        name: Set(params.name),
        scope: Set(params.scope),
        path_pattern: Set(params.path_pattern),
        method: Set(params.method),
        limit_count: Set(params.limit_count),
        window_seconds: Set(params.window_seconds),
        enabled: Set(params.enabled.unwrap_or(true)),
        description: Set(params.description),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;

    Ok(responses::ok(RateLimitRuleRecord::from(rule)))
}

#[utoipa::path(
    put,
    path = "/api/admin/rate-limits/{id}",
    tag = "admin-rate-limits",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = SaveRateLimitRuleParams,
    responses((status = 200, body = ApiResponse<RateLimitRuleRecord>))
)]
#[debug_handler]
pub async fn update(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<SaveRateLimitRuleParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:rate_limit:update").await?;
    validate_rule(&params)?;
    let rule = find_rule(&ctx, id).await?;

    let mut active = rule.into_active_model();
    active.name = Set(params.name);
    active.scope = Set(params.scope);
    active.path_pattern = Set(params.path_pattern);
    active.method = Set(params.method);
    active.limit_count = Set(params.limit_count);
    active.window_seconds = Set(params.window_seconds);
    active.enabled = Set(params.enabled.unwrap_or(true));
    active.description = Set(params.description);
    let rule = active.update(&ctx.db).await?;

    Ok(responses::ok(RateLimitRuleRecord::from(rule)))
}

#[utoipa::path(
    delete,
    path = "/api/admin/rate-limits/{id}",
    tag = "admin-rate-limits",
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
    authorize(&ctx, &auth, "system:rate_limit:delete").await?;
    find_rule(&ctx, id).await?;
    rate_limit_rules::Entity::delete_by_id(id)
        .exec(&ctx.db)
        .await?;
    Ok(responses::empty())
}

#[utoipa::path(
    get,
    path = "/api/admin/rate-limit-events",
    tag = "admin-rate-limits",
    security(("bearer_auth" = [])),
    params(RateLimitEventQueryParams),
    responses((status = 200, body = ApiResponse<PageResponse<RateLimitEventRecord>>))
)]
#[debug_handler]
pub async fn list_events(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<RateLimitEventQueryParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:monitor:view").await?;
    let page = params.page();
    let page_size = params.page_size();
    let mut query = rate_limit_events::Entity::find().order_by_desc(rate_limit_events::Column::Id);

    if let Some(rule_id) = params.rule_id {
        query = query.filter(rate_limit_events::Column::RuleId.eq(rule_id));
    }
    if let Some(ip) = params.ip.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(rate_limit_events::Column::Ip.eq(ip));
    }

    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(RateLimitEventRecord::from)
        .collect();

    Ok(responses::ok(PageResponse {
        items,
        page,
        page_size,
        total,
    }))
}

async fn find_rule(ctx: &AppContext, id: i32) -> ApiResult<rate_limit_rules::Model> {
    rate_limit_rules::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("rate limit rule not found"))
}

fn validate_rule(params: &SaveRateLimitRuleParams) -> ApiResult<()> {
    if params.name.trim().is_empty() || params.path_pattern.trim().is_empty() {
        return Err(ApiError::bad_request(
            "rate limit name and path are required",
        ));
    }
    if params.limit_count <= 0 || params.window_seconds <= 0 {
        return Err(ApiError::bad_request(
            "rate limit count and window must be positive",
        ));
    }
    match params.scope.as_str() {
        "ip" | "user" | "global" => Ok(()),
        _ => Err(ApiError::bad_request("unsupported rate limit scope")),
    }
}

impl From<rate_limit_rules::Model> for RateLimitRuleRecord {
    fn from(rule: rate_limit_rules::Model) -> Self {
        Self {
            id: rule.id,
            name: rule.name,
            scope: rule.scope,
            path_pattern: rule.path_pattern,
            method: rule.method,
            limit_count: rule.limit_count,
            window_seconds: rule.window_seconds,
            enabled: rule.enabled,
            description: rule.description,
            created_at: rule.created_at.to_rfc3339(),
            updated_at: rule.updated_at.to_rfc3339(),
        }
    }
}

impl From<rate_limit_events::Model> for RateLimitEventRecord {
    fn from(event: rate_limit_events::Model) -> Self {
        Self {
            id: event.id,
            ip: event.ip,
            method: event.method,
            path: event.path,
            rule_id: event.rule_id,
            user_id: event.user_id,
            occurred_at: event.occurred_at.to_rfc3339(),
            created_at: event.created_at.to_rfc3339(),
            updated_at: event.updated_at.to_rfc3339(),
        }
    }
}
