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
        _entities::{departments, tenants, user_departments, users},
        rbac::{self, EffectiveDataScope},
    },
    responses::{self, ApiResponse, EmptyData, PageParams, PageResponse},
};

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct DepartmentRecord {
    pub id: i32,
    pub tenant_id: i32,
    pub parent_id: Option<i32>,
    pub name: String,
    pub code: String,
    pub description: Option<String>,
    pub sort_order: i32,
    pub enabled: bool,
    pub is_system: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveDepartmentParams {
    pub tenant_id: Option<i32>,
    pub parent_id: Option<i32>,
    pub name: String,
    pub code: String,
    pub description: Option<String>,
    pub sort_order: Option<i32>,
    pub enabled: Option<bool>,
}

#[utoipa::path(
    get,
    path = "/api/admin/departments",
    tag = "admin-departments",
    security(("bearer_auth" = [])),
    params(PageParams),
    responses((status = 200, body = ApiResponse<PageResponse<DepartmentRecord>>))
)]
#[debug_handler]
pub async fn list(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<PageParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:department:list").await?;
    let scope = rbac::resolve_data_scope(&ctx.db, &actor).await?;
    ensure_department_feature_enabled(&ctx, &scope).await?;

    let page = params.page();
    let page_size = params.page_size();
    let mut query = apply_department_scope(departments::Entity::find(), &scope)
        .order_by_asc(departments::Column::TenantId)
        .order_by_asc(departments::Column::SortOrder)
        .order_by_asc(departments::Column::Id);

    if let Some(keyword) = params.keyword.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(
            Condition::any()
                .add(departments::Column::Name.contains(keyword))
                .add(departments::Column::Code.contains(keyword))
                .add(departments::Column::Description.contains(keyword)),
        );
    }

    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(DepartmentRecord::from)
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
    path = "/api/admin/departments/{id}",
    tag = "admin-departments",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<DepartmentRecord>))
)]
#[debug_handler]
pub async fn get(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:department:list").await?;
    let department = find_department(&ctx, id).await?;
    assert_department_visible(&ctx, &actor, &department).await?;

    Ok(responses::ok(DepartmentRecord::from(department)))
}

#[utoipa::path(
    post,
    path = "/api/admin/departments",
    tag = "admin-departments",
    security(("bearer_auth" = [])),
    request_body = SaveDepartmentParams,
    responses((status = 200, body = ApiResponse<DepartmentRecord>))
)]
#[debug_handler]
pub async fn create(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SaveDepartmentParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:department:create").await?;
    let tenant_id = resolve_department_tenant(&ctx, &actor, params.tenant_id).await?;
    validate_department(&ctx, tenant_id, None, params.parent_id, &params).await?;

    let department = departments::ActiveModel {
        tenant_id: Set(tenant_id),
        parent_id: Set(params.parent_id),
        name: Set(params.name),
        code: Set(params.code),
        description: Set(params.description),
        sort_order: Set(params.sort_order.unwrap_or(0)),
        enabled: Set(params.enabled.unwrap_or(true)),
        is_system: Set(false),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;

    Ok(responses::ok(DepartmentRecord::from(department)))
}

#[utoipa::path(
    put,
    path = "/api/admin/departments/{id}",
    tag = "admin-departments",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = SaveDepartmentParams,
    responses((status = 200, body = ApiResponse<DepartmentRecord>))
)]
#[debug_handler]
pub async fn update(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<SaveDepartmentParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:department:update").await?;
    let department = find_department(&ctx, id).await?;
    assert_department_visible(&ctx, &actor, &department).await?;
    let tenant_id = resolve_department_tenant(&ctx, &actor, params.tenant_id).await?;
    if department.tenant_id != tenant_id {
        return Err(ApiError::forbidden("cannot move department across tenants"));
    }
    if department.is_system && department.code != params.code {
        return Err(ApiError::bad_request(
            "system department code cannot be changed",
        ));
    }
    validate_department(&ctx, tenant_id, Some(id), params.parent_id, &params).await?;

    let mut active = department.into_active_model();
    active.parent_id = Set(params.parent_id);
    active.name = Set(params.name);
    active.code = Set(params.code);
    active.description = Set(params.description);
    active.sort_order = Set(params.sort_order.unwrap_or(0));
    active.enabled = Set(params.enabled.unwrap_or(true));
    let department = active.update(&ctx.db).await?;

    Ok(responses::ok(DepartmentRecord::from(department)))
}

#[utoipa::path(
    delete,
    path = "/api/admin/departments/{id}",
    tag = "admin-departments",
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
    let actor = authorize(&ctx, &auth, "system:department:delete").await?;
    let department = find_department(&ctx, id).await?;
    assert_department_visible(&ctx, &actor, &department).await?;
    if department.is_system {
        return Err(ApiError::bad_request("system department cannot be deleted"));
    }

    let child_count = departments::Entity::find()
        .filter(departments::Column::ParentId.eq(id))
        .count(&ctx.db)
        .await?;
    let user_count = user_departments::Entity::find()
        .filter(user_departments::Column::DepartmentId.eq(id))
        .count(&ctx.db)
        .await?;
    if child_count > 0 || user_count > 0 {
        return Err(ApiError::bad_request("department has children or users"));
    }

    departments::Entity::delete_by_id(id).exec(&ctx.db).await?;
    Ok(responses::empty())
}

#[must_use]
pub fn apply_department_scope(
    query: sea_orm::Select<departments::Entity>,
    scope: &EffectiveDataScope,
) -> sea_orm::Select<departments::Entity> {
    match scope {
        EffectiveDataScope::All => query,
        EffectiveDataScope::Tenant { tenant_id }
        | EffectiveDataScope::Department { tenant_id, .. } => {
            query.filter(departments::Column::TenantId.eq(*tenant_id))
        }
        EffectiveDataScope::SelfOnly { tenant_id, .. } => match tenant_id {
            Some(tenant_id) => query.filter(departments::Column::TenantId.eq(*tenant_id)),
            None => query.filter(departments::Column::Id.eq(-1)),
        },
        EffectiveDataScope::None => query.filter(departments::Column::Id.eq(-1)),
    }
}

pub async fn assert_department_visible(
    ctx: &AppContext,
    actor: &users::Model,
    department: &departments::Model,
) -> ApiResult<()> {
    match rbac::resolve_data_scope(&ctx.db, actor).await? {
        EffectiveDataScope::All => Ok(()),
        EffectiveDataScope::Tenant { tenant_id }
        | EffectiveDataScope::Department { tenant_id, .. }
            if department.tenant_id == tenant_id =>
        {
            ensure_tenant_departments_enabled(ctx, tenant_id).await
        }
        _ => Err(ApiError::forbidden("data scope denied")),
    }
}

pub async fn load_user_departments(
    ctx: &AppContext,
    user_id: i32,
) -> ApiResult<Vec<departments::Model>> {
    let links = user_departments::Entity::find()
        .filter(user_departments::Column::UserId.eq(user_id))
        .all(&ctx.db)
        .await?;
    let ids = links
        .into_iter()
        .map(|link| link.department_id)
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    Ok(departments::Entity::find()
        .filter(departments::Column::Id.is_in(ids))
        .filter(departments::Column::Enabled.eq(true))
        .order_by_asc(departments::Column::SortOrder)
        .order_by_asc(departments::Column::Id)
        .all(&ctx.db)
        .await?)
}

async fn resolve_department_tenant(
    ctx: &AppContext,
    actor: &users::Model,
    requested_tenant_id: Option<i32>,
) -> ApiResult<i32> {
    match rbac::resolve_data_scope(&ctx.db, actor).await? {
        EffectiveDataScope::All => requested_tenant_id
            .or(actor.tenant_id)
            .ok_or_else(|| ApiError::bad_request("tenant_id is required")),
        EffectiveDataScope::Tenant { tenant_id }
        | EffectiveDataScope::Department { tenant_id, .. } => {
            if requested_tenant_id.is_some_and(|id| id != tenant_id) {
                return Err(ApiError::forbidden(
                    "cannot manage another tenant department",
                ));
            }
            ensure_tenant_departments_enabled(ctx, tenant_id).await?;
            Ok(tenant_id)
        }
        _ => Err(ApiError::forbidden("data scope denied")),
    }
}

async fn validate_department(
    ctx: &AppContext,
    tenant_id: i32,
    department_id: Option<i32>,
    parent_id: Option<i32>,
    params: &SaveDepartmentParams,
) -> ApiResult<()> {
    if params.name.trim().is_empty() || params.code.trim().is_empty() {
        return Err(ApiError::bad_request(
            "department name and code are required",
        ));
    }
    let tenant = tenants::Entity::find_by_id(tenant_id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("tenant not found"))?;
    if !tenant.enabled {
        return Err(ApiError::bad_request("tenant is disabled"));
    }
    if department_id.is_some_and(|id| parent_id == Some(id)) {
        return Err(ApiError::bad_request("department cannot be its own parent"));
    }
    if let Some(parent_id) = parent_id {
        let parent = find_department(ctx, parent_id).await?;
        if parent.tenant_id != tenant_id {
            return Err(ApiError::bad_request(
                "parent department must belong to same tenant",
            ));
        }
    }
    let duplicate = departments::Entity::find()
        .filter(departments::Column::TenantId.eq(tenant_id))
        .filter(departments::Column::Code.eq(params.code.clone()))
        .one(&ctx.db)
        .await?;
    if duplicate.is_some_and(|department| Some(department.id) != department_id) {
        return Err(ApiError::bad_request("department code already exists"));
    }
    Ok(())
}

async fn ensure_department_feature_enabled(
    ctx: &AppContext,
    scope: &EffectiveDataScope,
) -> ApiResult<()> {
    match scope {
        EffectiveDataScope::All => Ok(()),
        EffectiveDataScope::Tenant { tenant_id }
        | EffectiveDataScope::Department { tenant_id, .. } => {
            ensure_tenant_departments_enabled(ctx, *tenant_id).await
        }
        _ => Err(ApiError::forbidden("data scope denied")),
    }
}

async fn ensure_tenant_departments_enabled(ctx: &AppContext, tenant_id: i32) -> ApiResult<()> {
    let tenant = tenants::Entity::find_by_id(tenant_id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("tenant not found"))?;
    if tenant.departments_enabled {
        Ok(())
    } else {
        Err(ApiError::forbidden("department management is disabled"))
    }
}

async fn find_department(ctx: &AppContext, id: i32) -> ApiResult<departments::Model> {
    departments::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("department not found"))
}

impl From<departments::Model> for DepartmentRecord {
    fn from(department: departments::Model) -> Self {
        Self {
            id: department.id,
            tenant_id: department.tenant_id,
            parent_id: department.parent_id,
            name: department.name,
            code: department.code,
            description: department.description,
            sort_order: department.sort_order,
            enabled: department.enabled,
            is_system: department.is_system,
            created_at: department.created_at.to_rfc3339(),
            updated_at: department.updated_at.to_rfc3339(),
        }
    }
}
