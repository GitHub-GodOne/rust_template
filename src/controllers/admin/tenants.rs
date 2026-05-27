#![allow(clippy::missing_errors_doc)]

use loco_rs::prelude::*;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    Set,
};
use serde::{Deserialize, Serialize};

use crate::{
    controllers::admin::authorize,
    errors::{ApiError, ApiResult},
    models::{
        _entities::{roles, tenants, users},
        rbac,
    },
    responses::{self, ApiResponse, EmptyData, PageParams, PageResponse},
};

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct TenantRecord {
    pub id: i32,
    pub name: String,
    pub code: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub is_system: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveTenantParams {
    pub name: String,
    pub code: String,
    pub description: Option<String>,
    pub enabled: Option<bool>,
}

#[utoipa::path(
    get,
    path = "/api/admin/tenants",
    tag = "admin-tenants",
    security(("bearer_auth" = [])),
    params(PageParams),
    responses((status = 200, body = ApiResponse<PageResponse<TenantRecord>>))
)]
#[debug_handler]
pub async fn list(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<PageParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:tenant:list").await?;
    require_all_scope(&ctx, &actor).await?;

    let page = params.page();
    let page_size = params.page_size();
    let mut query = tenants::Entity::find().order_by_asc(tenants::Column::Id);

    if let Some(keyword) = params.keyword.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(
            Condition::any()
                .add(tenants::Column::Name.contains(keyword))
                .add(tenants::Column::Code.contains(keyword))
                .add(tenants::Column::Description.contains(keyword)),
        );
    }

    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(TenantRecord::from)
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
    path = "/api/admin/tenants/{id}",
    tag = "admin-tenants",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<TenantRecord>))
)]
#[debug_handler]
pub async fn get(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:tenant:list").await?;
    require_all_scope(&ctx, &actor).await?;
    let tenant = find_tenant(&ctx, id).await?;
    Ok(responses::ok(TenantRecord::from(tenant)))
}

#[utoipa::path(
    post,
    path = "/api/admin/tenants",
    tag = "admin-tenants",
    security(("bearer_auth" = [])),
    request_body = SaveTenantParams,
    responses((status = 200, body = ApiResponse<TenantRecord>))
)]
#[debug_handler]
pub async fn create(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SaveTenantParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:tenant:create").await?;
    require_all_scope(&ctx, &actor).await?;
    validate_tenant(&params)?;

    let tenant = tenants::ActiveModel {
        name: Set(params.name),
        code: Set(params.code),
        description: Set(params.description),
        enabled: Set(params.enabled.unwrap_or(true)),
        is_system: Set(false),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;

    Ok(responses::ok(TenantRecord::from(tenant)))
}

#[utoipa::path(
    put,
    path = "/api/admin/tenants/{id}",
    tag = "admin-tenants",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = SaveTenantParams,
    responses((status = 200, body = ApiResponse<TenantRecord>))
)]
#[debug_handler]
pub async fn update(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<SaveTenantParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:tenant:update").await?;
    require_all_scope(&ctx, &actor).await?;
    validate_tenant(&params)?;
    let tenant = find_tenant(&ctx, id).await?;
    if tenant.is_system && tenant.code != params.code {
        return Err(ApiError::bad_request(
            "system tenant code cannot be changed",
        ));
    }

    let mut active = tenant.into_active_model();
    active.name = Set(params.name);
    active.code = Set(params.code);
    active.description = Set(params.description);
    active.enabled = Set(params.enabled.unwrap_or(true));
    let tenant = active.update(&ctx.db).await?;

    Ok(responses::ok(TenantRecord::from(tenant)))
}

#[utoipa::path(
    delete,
    path = "/api/admin/tenants/{id}",
    tag = "admin-tenants",
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
    let actor = authorize(&ctx, &auth, "system:tenant:delete").await?;
    require_all_scope(&ctx, &actor).await?;
    let tenant = find_tenant(&ctx, id).await?;
    if tenant.is_system {
        return Err(ApiError::bad_request("system tenant cannot be deleted"));
    }

    let user_count = users::Entity::find()
        .filter(users::Column::TenantId.eq(id))
        .count(&ctx.db)
        .await?;
    let role_count = roles::Entity::find()
        .filter(roles::Column::TenantId.eq(id))
        .count(&ctx.db)
        .await?;
    if user_count > 0 || role_count > 0 {
        return Err(ApiError::bad_request("tenant has users or roles"));
    }

    tenants::Entity::delete_by_id(id).exec(&ctx.db).await?;
    Ok(responses::empty())
}

async fn require_all_scope(ctx: &AppContext, actor: &users::Model) -> ApiResult<()> {
    if rbac::resolve_data_scope(&ctx.db, actor).await?.is_all() {
        Ok(())
    } else {
        Err(ApiError::forbidden("data scope denied"))
    }
}

async fn find_tenant(ctx: &AppContext, id: i32) -> ApiResult<tenants::Model> {
    tenants::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("tenant not found"))
}

fn validate_tenant(params: &SaveTenantParams) -> ApiResult<()> {
    if params.name.trim().is_empty() || params.code.trim().is_empty() {
        return Err(ApiError::bad_request("tenant name and code are required"));
    }
    Ok(())
}

impl From<tenants::Model> for TenantRecord {
    fn from(tenant: tenants::Model) -> Self {
        Self {
            id: tenant.id,
            name: tenant.name,
            code: tenant.code,
            description: tenant.description,
            enabled: tenant.enabled,
            is_system: tenant.is_system,
            created_at: tenant.created_at.to_rfc3339(),
            updated_at: tenant.updated_at.to_rfc3339(),
        }
    }
}
