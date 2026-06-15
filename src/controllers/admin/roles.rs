#![allow(clippy::missing_errors_doc, clippy::struct_excessive_bools)]

use std::collections::BTreeMap;

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
        _entities::{
            data_scopes, permissions as permissions_entity, role_data_scopes, role_menus,
            role_permissions, roles,
        },
        rbac::{self, EffectiveDataScope, SUPER_ADMIN_ROLE},
    },
    responses::{self, ApiResponse, EmptyData, PageParams, PageResponse},
};

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct RoleRecord {
    pub id: i32,
    pub name: String,
    pub code: String,
    pub description: Option<String>,
    pub is_system: bool,
    pub enabled: bool,
    pub tenant_id: Option<i32>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveRoleParams {
    pub name: String,
    pub code: String,
    pub description: Option<String>,
    pub enabled: Option<bool>,
    pub tenant_id: Option<i32>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct RolePermissionIds {
    pub permission_ids: Vec<i32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct RoleMenuGrant {
    pub menu_id: i32,
    pub can_create: bool,
    pub can_update: bool,
    pub can_delete: bool,
    pub can_import: bool,
    pub can_export: bool,
    pub can_print: bool,
    pub can_help: bool,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct RoleMenuGrants {
    pub grants: Vec<RoleMenuGrant>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct RoleDataScopeIds {
    pub data_scope_ids: Vec<i32>,
}

#[utoipa::path(
    get,
    path = "/api/admin/roles",
    tag = "admin-roles",
    security(("bearer_auth" = [])),
    params(PageParams),
    responses((status = 200, body = ApiResponse<PageResponse<RoleRecord>>))
)]
#[debug_handler]
pub async fn list(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<PageParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:role:list").await?;
    let scope = rbac::resolve_data_scope(&ctx.db, &actor).await?;

    let page = params.page();
    let page_size = params.page_size();
    let mut query = apply_role_scope(roles::Entity::find(), &scope).order_by_asc(roles::Column::Id);

    if let Some(keyword) = params
        .keyword
        .as_deref()
        .filter(|keyword| !keyword.is_empty())
    {
        query = query.filter(
            Condition::any()
                .add(roles::Column::Name.contains(keyword))
                .add(roles::Column::Code.contains(keyword)),
        );
    }

    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(RoleRecord::from)
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
    path = "/api/admin/roles/{id}",
    tag = "admin-roles",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<RoleRecord>))
)]
#[debug_handler]
pub async fn get(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:role:list").await?;
    let role = find_role(&ctx, id).await?;
    assert_role_visible(&ctx, &actor, &role).await?;
    Ok(responses::ok(RoleRecord::from(role)))
}

#[utoipa::path(
    post,
    path = "/api/admin/roles",
    tag = "admin-roles",
    security(("bearer_auth" = [])),
    request_body = SaveRoleParams,
    responses((status = 200, body = ApiResponse<RoleRecord>))
)]
#[debug_handler]
pub async fn create(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SaveRoleParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:role:create").await?;
    let tenant_id = resolve_role_tenant(&ctx, &actor, params.tenant_id).await?;
    if params.code == SUPER_ADMIN_ROLE && !rbac::is_super_admin(&ctx.db, actor.id).await? {
        return Err(ApiError::forbidden("cannot create super admin role"));
    }
    let role = roles::ActiveModel {
        name: Set(params.name),
        code: Set(params.code),
        description: Set(params.description),
        enabled: Set(params.enabled.unwrap_or(true)),
        is_system: Set(false),
        tenant_id: Set(tenant_id),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;

    Ok(responses::ok(RoleRecord::from(role)))
}

#[utoipa::path(
    put,
    path = "/api/admin/roles/{id}",
    tag = "admin-roles",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = SaveRoleParams,
    responses((status = 200, body = ApiResponse<RoleRecord>))
)]
#[debug_handler]
pub async fn update(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<SaveRoleParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:role:update").await?;
    let role = find_role(&ctx, id).await?;
    assert_role_mutable(&ctx, &actor, &role).await?;
    let tenant_id = resolve_role_tenant(&ctx, &actor, params.tenant_id).await?;
    if role.is_system && role.code != params.code {
        return Err(ApiError::bad_request("system role code cannot be changed"));
    }

    let mut active = role.into_active_model();
    active.name = Set(params.name);
    active.code = Set(params.code);
    active.description = Set(params.description);
    active.enabled = Set(params.enabled.unwrap_or(true));
    active.tenant_id = Set(tenant_id);
    let role = active.update(&ctx.db).await?;

    Ok(responses::ok(RoleRecord::from(role)))
}

#[utoipa::path(
    delete,
    path = "/api/admin/roles/{id}",
    tag = "admin-roles",
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
    let actor = authorize(&ctx, &auth, "system:role:delete").await?;
    let role = find_role(&ctx, id).await?;
    assert_role_mutable(&ctx, &actor, &role).await?;
    if role.is_system || role.code == SUPER_ADMIN_ROLE {
        return Err(ApiError::bad_request("system role cannot be deleted"));
    }

    roles::Entity::delete_by_id(id).exec(&ctx.db).await?;
    Ok(responses::empty())
}

#[utoipa::path(
    get,
    path = "/api/admin/roles/{id}/permissions",
    tag = "admin-roles",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<RolePermissionIds>))
)]
#[debug_handler]
pub async fn permissions(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:role:assign_permissions").await?;
    let role = find_role(&ctx, id).await?;
    assert_role_mutable(&ctx, &actor, &role).await?;

    let permission_ids = role_permissions::Entity::find()
        .filter(role_permissions::Column::RoleId.eq(id))
        .order_by_asc(role_permissions::Column::PermissionId)
        .all(&ctx.db)
        .await?
        .into_iter()
        .map(|link| link.permission_id)
        .collect();

    Ok(responses::ok(RolePermissionIds { permission_ids }))
}

#[utoipa::path(
    put,
    path = "/api/admin/roles/{id}/permissions",
    tag = "admin-roles",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = RolePermissionIds,
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn save_permissions(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<RolePermissionIds>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:role:assign_permissions").await?;
    let role = find_role(&ctx, id).await?;
    assert_role_mutable(&ctx, &actor, &role).await?;

    let mut permission_ids = params.permission_ids;
    permission_ids.sort_unstable();
    permission_ids.dedup();
    let permission_count = permissions_entity::Entity::find()
        .filter(permissions_entity::Column::Id.is_in(permission_ids.clone()))
        .count(&ctx.db)
        .await?;
    if permission_count != permission_ids.len() as u64 {
        return Err(ApiError::bad_request("invalid permission ids"));
    }

    role_permissions::Entity::delete_many()
        .filter(role_permissions::Column::RoleId.eq(id))
        .exec(&ctx.db)
        .await?;

    if !permission_ids.is_empty() {
        let rows = permission_ids
            .into_iter()
            .map(|permission_id| role_permissions::ActiveModel {
                role_id: Set(id),
                permission_id: Set(permission_id),
                ..Default::default()
            });
        role_permissions::Entity::insert_many(rows)
            .exec(&ctx.db)
            .await?;
    }

    Ok(responses::empty())
}

#[utoipa::path(
    get,
    path = "/api/admin/roles/{id}/menus",
    tag = "admin-roles",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<RoleMenuGrants>))
)]
#[debug_handler]
pub async fn menus(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:role:assign_menus").await?;
    let role = find_role(&ctx, id).await?;
    assert_role_mutable(&ctx, &actor, &role).await?;

    let grants = role_menus::Entity::find()
        .filter(role_menus::Column::RoleId.eq(id))
        .order_by_asc(role_menus::Column::MenuId)
        .all(&ctx.db)
        .await?
        .into_iter()
        .map(RoleMenuGrant::from)
        .collect();

    Ok(responses::ok(RoleMenuGrants { grants }))
}

#[utoipa::path(
    put,
    path = "/api/admin/roles/{id}/menus",
    tag = "admin-roles",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = RoleMenuGrants,
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn save_menus(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<RoleMenuGrants>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:role:assign_menus").await?;
    let role = find_role(&ctx, id).await?;
    assert_role_mutable(&ctx, &actor, &role).await?;

    let mut grants_by_menu = BTreeMap::new();
    for grant in params.grants {
        grants_by_menu.insert(grant.menu_id, grant);
    }
    let menu_ids = grants_by_menu.keys().copied().collect::<Vec<_>>();
    let menu_count = crate::models::_entities::menus::Entity::find()
        .filter(crate::models::_entities::menus::Column::Id.is_in(menu_ids.clone()))
        .count(&ctx.db)
        .await?;
    if menu_count != menu_ids.len() as u64 {
        return Err(ApiError::bad_request("invalid menu ids"));
    }

    role_menus::Entity::delete_many()
        .filter(role_menus::Column::RoleId.eq(id))
        .exec(&ctx.db)
        .await?;

    if !grants_by_menu.is_empty() {
        let rows = grants_by_menu
            .into_values()
            .map(|grant| role_menus::ActiveModel {
                role_id: Set(id),
                menu_id: Set(grant.menu_id),
                can_create: Set(grant.can_create),
                can_update: Set(grant.can_update),
                can_delete: Set(grant.can_delete),
                can_import: Set(grant.can_import),
                can_export: Set(grant.can_export),
                can_print: Set(grant.can_print),
                can_help: Set(grant.can_help),
                ..Default::default()
            });
        role_menus::Entity::insert_many(rows).exec(&ctx.db).await?;
    }

    Ok(responses::empty())
}

#[utoipa::path(
    get,
    path = "/api/admin/roles/{id}/data-scopes",
    tag = "admin-roles",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<RoleDataScopeIds>))
)]
#[debug_handler]
pub async fn data_scopes(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:role:assign_data_scopes").await?;
    let role = find_role(&ctx, id).await?;
    assert_role_mutable(&ctx, &actor, &role).await?;

    let data_scope_ids = role_data_scopes::Entity::find()
        .filter(role_data_scopes::Column::RoleId.eq(id))
        .order_by_asc(role_data_scopes::Column::DataScopeId)
        .all(&ctx.db)
        .await?
        .into_iter()
        .map(|link| link.data_scope_id)
        .collect();

    Ok(responses::ok(RoleDataScopeIds { data_scope_ids }))
}

#[utoipa::path(
    put,
    path = "/api/admin/roles/{id}/data-scopes",
    tag = "admin-roles",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = RoleDataScopeIds,
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn save_data_scopes(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<RoleDataScopeIds>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:role:assign_data_scopes").await?;
    let role = find_role(&ctx, id).await?;
    assert_role_mutable(&ctx, &actor, &role).await?;

    let mut data_scope_ids = params.data_scope_ids;
    data_scope_ids.sort_unstable();
    data_scope_ids.dedup();
    let scopes = data_scopes::Entity::find()
        .filter(data_scopes::Column::Id.is_in(data_scope_ids.clone()))
        .all(&ctx.db)
        .await?;
    if scopes.len() != data_scope_ids.len() {
        return Err(ApiError::bad_request("invalid data scope ids"));
    }
    if !rbac::is_super_admin(&ctx.db, actor.id).await?
        && scopes.iter().any(|scope| scope.code == "all")
    {
        return Err(ApiError::forbidden("cannot assign all data scope"));
    }

    role_data_scopes::Entity::delete_many()
        .filter(role_data_scopes::Column::RoleId.eq(id))
        .exec(&ctx.db)
        .await?;

    if !data_scope_ids.is_empty() {
        let rows = data_scope_ids
            .into_iter()
            .map(|data_scope_id| role_data_scopes::ActiveModel {
                role_id: Set(id),
                data_scope_id: Set(data_scope_id),
                ..Default::default()
            });
        role_data_scopes::Entity::insert_many(rows)
            .exec(&ctx.db)
            .await?;
    }

    Ok(responses::empty())
}

async fn find_role(ctx: &AppContext, id: i32) -> ApiResult<roles::Model> {
    roles::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("role not found"))
}

fn apply_role_scope(
    query: sea_orm::Select<roles::Entity>,
    scope: &EffectiveDataScope,
) -> sea_orm::Select<roles::Entity> {
    match scope {
        EffectiveDataScope::All => query,
        EffectiveDataScope::Tenant { tenant_id }
        | EffectiveDataScope::Department { tenant_id, .. } => {
            query.filter(roles::Column::TenantId.eq(*tenant_id))
        }
        EffectiveDataScope::SelfOnly { .. } | EffectiveDataScope::None => {
            query.filter(roles::Column::Id.eq(-1))
        }
    }
}

async fn assert_role_visible(
    ctx: &AppContext,
    actor: &crate::models::_entities::users::Model,
    role: &roles::Model,
) -> ApiResult<()> {
    match rbac::resolve_data_scope(&ctx.db, actor).await? {
        EffectiveDataScope::All => Ok(()),
        EffectiveDataScope::Tenant { tenant_id }
        | EffectiveDataScope::Department { tenant_id, .. }
            if role.tenant_id == Some(tenant_id) =>
        {
            Ok(())
        }
        _ => Err(ApiError::forbidden("data scope denied")),
    }
}

async fn assert_role_mutable(
    ctx: &AppContext,
    actor: &crate::models::_entities::users::Model,
    role: &roles::Model,
) -> ApiResult<()> {
    assert_role_visible(ctx, actor, role).await?;
    if role.code == SUPER_ADMIN_ROLE && !rbac::is_super_admin(&ctx.db, actor.id).await? {
        return Err(ApiError::forbidden("cannot mutate super admin role"));
    }
    Ok(())
}

async fn resolve_role_tenant(
    ctx: &AppContext,
    actor: &crate::models::_entities::users::Model,
    requested_tenant_id: Option<i32>,
) -> ApiResult<Option<i32>> {
    match rbac::resolve_data_scope(&ctx.db, actor).await? {
        EffectiveDataScope::All => Ok(requested_tenant_id),
        EffectiveDataScope::Tenant { tenant_id } => {
            if requested_tenant_id.is_some_and(|id| id != tenant_id) {
                return Err(ApiError::forbidden("cannot assign role to another tenant"));
            }
            Ok(Some(tenant_id))
        }
        _ => Err(ApiError::forbidden("data scope denied")),
    }
}

impl From<roles::Model> for RoleRecord {
    fn from(role: roles::Model) -> Self {
        Self {
            id: role.id,
            name: role.name,
            code: role.code,
            description: role.description,
            is_system: role.is_system,
            enabled: role.enabled,
            tenant_id: role.tenant_id,
            created_at: role.created_at.to_rfc3339(),
            updated_at: role.updated_at.to_rfc3339(),
        }
    }
}

impl From<role_menus::Model> for RoleMenuGrant {
    fn from(grant: role_menus::Model) -> Self {
        Self {
            menu_id: grant.menu_id,
            can_create: grant.can_create,
            can_update: grant.can_update,
            can_delete: grant.can_delete,
            can_import: grant.can_import,
            can_export: grant.can_export,
            can_print: grant.can_print,
            can_help: grant.can_help,
        }
    }
}
