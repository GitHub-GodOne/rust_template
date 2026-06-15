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
        _entities::{departments, roles, user_departments, user_roles, users},
        rbac::{self, EffectiveDataScope, SUPER_ADMIN_ROLE},
        users::RegisterParams,
    },
    responses::{self, ApiResponse, EmptyData, PageParams, PageResponse},
};

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct UserRecord {
    pub id: i32,
    pub pid: String,
    pub name: String,
    pub email: String,
    pub tenant_id: Option<i32>,
    pub current_department_id: Option<i32>,
    pub is_verified: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CreateUserParams {
    pub name: String,
    pub email: String,
    pub password: String,
    pub tenant_id: Option<i32>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct UpdateUserParams {
    pub name: String,
    pub email: String,
    pub tenant_id: Option<i32>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct AssignedRoleRecord {
    pub id: i32,
    pub name: String,
    pub code: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct AssignedDepartmentRecord {
    pub id: i32,
    pub tenant_id: i32,
    pub name: String,
    pub code: String,
    pub is_primary: bool,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveUserRolesParams {
    pub role_ids: Vec<i32>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveUserDepartmentsParams {
    pub department_ids: Vec<i32>,
    pub current_department_id: Option<i32>,
}

#[utoipa::path(
    get,
    path = "/api/admin/users",
    tag = "admin-users",
    security(("bearer_auth" = [])),
    params(PageParams),
    responses((status = 200, body = ApiResponse<PageResponse<UserRecord>>))
)]
#[debug_handler]
pub async fn list(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<PageParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:user:list").await?;
    let scope = rbac::resolve_data_scope(&ctx.db, &actor).await?;

    let page = params.page();
    let page_size = params.page_size();
    let mut query =
        apply_user_scope(users::Entity::find(), &scope).order_by_desc(users::Column::Id);

    if let Some(keyword) = params
        .keyword
        .as_deref()
        .filter(|keyword| !keyword.is_empty())
    {
        query = query.filter(
            Condition::any()
                .add(users::Column::Name.contains(keyword))
                .add(users::Column::Email.contains(keyword)),
        );
    }

    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(UserRecord::from)
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
    path = "/api/admin/users/{id}",
    tag = "admin-users",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<UserRecord>))
)]
#[debug_handler]
pub async fn get(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:user:list").await?;
    let user = find_user(&ctx, id).await?;
    assert_user_visible(&ctx, &actor, &user).await?;

    Ok(responses::ok(UserRecord::from(user)))
}

#[utoipa::path(
    post,
    path = "/api/admin/users",
    tag = "admin-users",
    security(("bearer_auth" = [])),
    request_body = CreateUserParams,
    responses((status = 200, body = ApiResponse<UserRecord>))
)]
#[debug_handler]
pub async fn create(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<CreateUserParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:user:create").await?;
    let tenant_id = resolve_user_tenant(&ctx, &actor, params.tenant_id).await?;
    let mut user = crate::models::users::Model::create_with_password(
        &ctx.db,
        &RegisterParams {
            name: params.name,
            email: params.email,
            password: params.password,
        },
    )
    .await?;
    let mut active = user.into_active_model();
    active.tenant_id = Set(tenant_id);
    user = active.update(&ctx.db).await?;

    Ok(responses::ok(UserRecord::from(user)))
}

#[utoipa::path(
    put,
    path = "/api/admin/users/{id}",
    tag = "admin-users",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = UpdateUserParams,
    responses((status = 200, body = ApiResponse<UserRecord>))
)]
#[debug_handler]
pub async fn update(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<UpdateUserParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:user:update").await?;
    let user = find_user(&ctx, id).await?;
    assert_user_visible(&ctx, &actor, &user).await?;
    let tenant_id = resolve_user_tenant(&ctx, &actor, params.tenant_id).await?;

    let mut active = user.into_active_model();
    active.name = Set(params.name);
    active.email = Set(params.email);
    active.tenant_id = Set(tenant_id);
    let user = active.update(&ctx.db).await?;

    Ok(responses::ok(UserRecord::from(user)))
}

#[utoipa::path(
    delete,
    path = "/api/admin/users/{id}",
    tag = "admin-users",
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
    let actor = authorize(&ctx, &auth, "system:user:delete").await?;
    let user = find_user(&ctx, id).await?;
    assert_user_visible(&ctx, &actor, &user).await?;
    if actor.id == id {
        return Err(ApiError::bad_request("cannot delete current user"));
    }

    users::Entity::delete_by_id(id).exec(&ctx.db).await?;
    Ok(responses::empty())
}

#[utoipa::path(
    get,
    path = "/api/admin/users/{id}/roles",
    tag = "admin-users",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<Vec<AssignedRoleRecord>>))
)]
#[debug_handler]
pub async fn roles(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:user:assign_roles").await?;
    let user = find_user(&ctx, id).await?;
    assert_user_visible(&ctx, &actor, &user).await?;

    let links = user_roles::Entity::find()
        .filter(user_roles::Column::UserId.eq(id))
        .all(&ctx.db)
        .await?;
    let role_ids = links
        .into_iter()
        .map(|link| link.role_id)
        .collect::<Vec<_>>();
    if role_ids.is_empty() {
        return Ok(responses::ok(Vec::<AssignedRoleRecord>::new()));
    }

    let roles = roles::Entity::find()
        .filter(roles::Column::Id.is_in(role_ids))
        .order_by_asc(roles::Column::Id)
        .all(&ctx.db)
        .await?
        .into_iter()
        .map(AssignedRoleRecord::from)
        .collect::<Vec<_>>();

    Ok(responses::ok(roles))
}

#[utoipa::path(
    put,
    path = "/api/admin/users/{id}/roles",
    tag = "admin-users",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = SaveUserRolesParams,
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn save_roles(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<SaveUserRolesParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:user:assign_roles").await?;
    let user = find_user(&ctx, id).await?;
    assert_user_visible(&ctx, &actor, &user).await?;

    let mut role_ids = params.role_ids;
    role_ids.sort_unstable();
    role_ids.dedup();

    assert_roles_assignable(&ctx, &actor, user.tenant_id, &role_ids).await?;

    user_roles::Entity::delete_many()
        .filter(user_roles::Column::UserId.eq(id))
        .exec(&ctx.db)
        .await?;

    if !role_ids.is_empty() {
        let rows = role_ids.into_iter().map(|role_id| user_roles::ActiveModel {
            user_id: Set(id),
            role_id: Set(role_id),
            ..Default::default()
        });
        user_roles::Entity::insert_many(rows).exec(&ctx.db).await?;
    }

    Ok(responses::empty())
}

#[utoipa::path(
    get,
    path = "/api/admin/users/{id}/departments",
    tag = "admin-users",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<Vec<AssignedDepartmentRecord>>))
)]
#[debug_handler]
pub async fn departments(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:user:list").await?;
    let user = find_user(&ctx, id).await?;
    assert_user_visible(&ctx, &actor, &user).await?;

    Ok(responses::ok(load_assigned_departments(&ctx, id).await?))
}

#[utoipa::path(
    put,
    path = "/api/admin/users/{id}/departments",
    tag = "admin-users",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = SaveUserDepartmentsParams,
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn save_departments(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<SaveUserDepartmentsParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:user:assign_departments").await?;
    let user = find_user(&ctx, id).await?;
    assert_user_visible(&ctx, &actor, &user).await?;

    let mut department_ids = params.department_ids;
    department_ids.sort_unstable();
    department_ids.dedup();
    assert_departments_assignable(
        &ctx,
        &actor,
        &user,
        &department_ids,
        params.current_department_id,
    )
    .await?;

    user_departments::Entity::delete_many()
        .filter(user_departments::Column::UserId.eq(id))
        .exec(&ctx.db)
        .await?;

    if !department_ids.is_empty() {
        let rows = department_ids
            .iter()
            .map(|department_id| user_departments::ActiveModel {
                user_id: Set(id),
                department_id: Set(*department_id),
                is_primary: Set(Some(*department_id) == params.current_department_id),
                ..Default::default()
            });
        user_departments::Entity::insert_many(rows)
            .exec(&ctx.db)
            .await?;
    }

    let mut active = user.into_active_model();
    active.current_department_id = Set(params.current_department_id);
    active.update(&ctx.db).await?;

    Ok(responses::empty())
}

async fn find_user(ctx: &AppContext, id: i32) -> ApiResult<users::Model> {
    users::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("user not found"))
}

fn apply_user_scope(
    query: sea_orm::Select<users::Entity>,
    scope: &EffectiveDataScope,
) -> sea_orm::Select<users::Entity> {
    match scope {
        EffectiveDataScope::All => query,
        EffectiveDataScope::Tenant { tenant_id }
        | EffectiveDataScope::Department { tenant_id, .. } => {
            query.filter(users::Column::TenantId.eq(*tenant_id))
        }
        EffectiveDataScope::SelfOnly { user_id, .. } => {
            query.filter(users::Column::Id.eq(*user_id))
        }
        EffectiveDataScope::None => query.filter(users::Column::Id.eq(-1)),
    }
}

async fn assert_user_visible(
    ctx: &AppContext,
    actor: &users::Model,
    user: &users::Model,
) -> ApiResult<()> {
    match rbac::resolve_data_scope(&ctx.db, actor).await? {
        EffectiveDataScope::All => Ok(()),
        EffectiveDataScope::Tenant { tenant_id }
        | EffectiveDataScope::Department { tenant_id, .. }
            if user.tenant_id == Some(tenant_id) =>
        {
            Ok(())
        }
        EffectiveDataScope::SelfOnly { user_id, .. } if user.id == user_id => Ok(()),
        _ => Err(ApiError::forbidden("data scope denied")),
    }
}

async fn resolve_user_tenant(
    ctx: &AppContext,
    actor: &users::Model,
    requested_tenant_id: Option<i32>,
) -> ApiResult<Option<i32>> {
    match rbac::resolve_data_scope(&ctx.db, actor).await? {
        EffectiveDataScope::All => Ok(requested_tenant_id),
        EffectiveDataScope::Tenant { tenant_id }
        | EffectiveDataScope::Department { tenant_id, .. } => {
            if requested_tenant_id.is_some_and(|id| id != tenant_id) {
                return Err(ApiError::forbidden("cannot assign user to another tenant"));
            }
            Ok(Some(tenant_id))
        }
        _ => Err(ApiError::forbidden("data scope denied")),
    }
}

async fn assert_roles_assignable(
    ctx: &AppContext,
    actor: &users::Model,
    target_tenant_id: Option<i32>,
    role_ids: &[i32],
) -> ApiResult<()> {
    let roles = roles::Entity::find()
        .filter(roles::Column::Id.is_in(role_ids.to_vec()))
        .all(&ctx.db)
        .await?;
    if roles.len() != role_ids.len() {
        return Err(ApiError::bad_request("invalid role ids"));
    }

    let actor_is_super_admin = rbac::is_super_admin(&ctx.db, actor.id).await?;
    match rbac::resolve_data_scope(&ctx.db, actor).await? {
        EffectiveDataScope::All => {
            if !actor_is_super_admin && roles.iter().any(|role| role.code == SUPER_ADMIN_ROLE) {
                return Err(ApiError::forbidden("cannot assign super admin role"));
            }
            Ok(())
        }
        EffectiveDataScope::Tenant { tenant_id }
        | EffectiveDataScope::Department { tenant_id, .. } => {
            if target_tenant_id != Some(tenant_id) {
                return Err(ApiError::forbidden("cannot assign roles across tenants"));
            }
            if roles
                .iter()
                .any(|role| role.tenant_id != Some(tenant_id) || role.code == SUPER_ADMIN_ROLE)
            {
                return Err(ApiError::forbidden(
                    "cannot assign role outside current tenant",
                ));
            }
            Ok(())
        }
        _ => Err(ApiError::forbidden("data scope denied")),
    }
}

async fn load_assigned_departments(
    ctx: &AppContext,
    user_id: i32,
) -> ApiResult<Vec<AssignedDepartmentRecord>> {
    let links = user_departments::Entity::find()
        .filter(user_departments::Column::UserId.eq(user_id))
        .all(&ctx.db)
        .await?;
    let department_ids = links
        .iter()
        .map(|link| link.department_id)
        .collect::<Vec<_>>();
    if department_ids.is_empty() {
        return Ok(Vec::new());
    }

    let departments = departments::Entity::find()
        .filter(departments::Column::Id.is_in(department_ids))
        .order_by_asc(departments::Column::SortOrder)
        .order_by_asc(departments::Column::Id)
        .all(&ctx.db)
        .await?;

    Ok(departments
        .into_iter()
        .map(|department| {
            let is_primary = links
                .iter()
                .any(|link| link.department_id == department.id && link.is_primary);
            AssignedDepartmentRecord {
                id: department.id,
                tenant_id: department.tenant_id,
                name: department.name,
                code: department.code,
                is_primary,
            }
        })
        .collect())
}

async fn assert_departments_assignable(
    ctx: &AppContext,
    actor: &users::Model,
    target_user: &users::Model,
    department_ids: &[i32],
    current_department_id: Option<i32>,
) -> ApiResult<()> {
    if let Some(current_department_id) = current_department_id {
        if !department_ids.contains(&current_department_id) {
            return Err(ApiError::bad_request(
                "current department must be assigned to user",
            ));
        }
    }

    let departments = departments::Entity::find()
        .filter(departments::Column::Id.is_in(department_ids.to_vec()))
        .all(&ctx.db)
        .await?;
    if departments.len() != department_ids.len() {
        return Err(ApiError::bad_request("invalid department ids"));
    }
    if departments.iter().any(|department| !department.enabled) {
        return Err(ApiError::bad_request("department is disabled"));
    }

    match rbac::resolve_data_scope(&ctx.db, actor).await? {
        EffectiveDataScope::All => {
            if let Some(tenant_id) = target_user.tenant_id {
                if departments
                    .iter()
                    .any(|department| department.tenant_id != tenant_id)
                {
                    return Err(ApiError::bad_request(
                        "department must belong to user's tenant",
                    ));
                }
            }
            Ok(())
        }
        EffectiveDataScope::Tenant { tenant_id }
        | EffectiveDataScope::Department { tenant_id, .. } => {
            if target_user.tenant_id != Some(tenant_id) {
                return Err(ApiError::forbidden(
                    "cannot assign departments across tenants",
                ));
            }
            if departments
                .iter()
                .any(|department| department.tenant_id != tenant_id)
            {
                return Err(ApiError::forbidden(
                    "cannot assign department outside current tenant",
                ));
            }
            Ok(())
        }
        _ => Err(ApiError::forbidden("data scope denied")),
    }
}

impl From<users::Model> for UserRecord {
    fn from(user: users::Model) -> Self {
        Self {
            id: user.id,
            pid: user.pid.to_string(),
            name: user.name,
            email: user.email,
            tenant_id: user.tenant_id,
            current_department_id: user.current_department_id,
            is_verified: user.email_verified_at.is_some(),
            created_at: user.created_at.to_rfc3339(),
            updated_at: user.updated_at.to_rfc3339(),
        }
    }
}

impl From<roles::Model> for AssignedRoleRecord {
    fn from(role: roles::Model) -> Self {
        Self {
            id: role.id,
            name: role.name,
            code: role.code,
        }
    }
}
