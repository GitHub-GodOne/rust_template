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
    models::_entities::{permissions, role_permissions},
    responses::{self, ApiResponse, EmptyData, PageParams, PageResponse},
};

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct PermissionRecord {
    pub id: i32,
    pub name: String,
    pub code: String,
    pub group_name: String,
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SavePermissionParams {
    pub name: String,
    pub code: String,
    pub group_name: String,
    pub description: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/admin/permissions",
    tag = "admin-permissions",
    security(("bearer_auth" = [])),
    params(PageParams),
    responses((status = 200, body = ApiResponse<PageResponse<PermissionRecord>>))
)]
#[debug_handler]
pub async fn list(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<PageParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:permission:list").await?;

    let page = params.page();
    let page_size = params.page_size();
    let mut query = permissions::Entity::find()
        .order_by_asc(permissions::Column::GroupName)
        .order_by_asc(permissions::Column::Id);

    if let Some(keyword) = params
        .keyword
        .as_deref()
        .filter(|keyword| !keyword.is_empty())
    {
        query = query.filter(
            Condition::any()
                .add(permissions::Column::Name.contains(keyword))
                .add(permissions::Column::Code.contains(keyword))
                .add(permissions::Column::GroupName.contains(keyword)),
        );
    }

    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(PermissionRecord::from)
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
    path = "/api/admin/permissions/{id}",
    tag = "admin-permissions",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<PermissionRecord>))
)]
#[debug_handler]
pub async fn get(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:permission:list").await?;
    let permission = find_permission(&ctx, id).await?;
    Ok(responses::ok(PermissionRecord::from(permission)))
}

#[utoipa::path(
    post,
    path = "/api/admin/permissions",
    tag = "admin-permissions",
    security(("bearer_auth" = [])),
    request_body = SavePermissionParams,
    responses((status = 200, body = ApiResponse<PermissionRecord>))
)]
#[debug_handler]
pub async fn create(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SavePermissionParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:permission:create").await?;
    let permission = permissions::ActiveModel {
        name: Set(params.name),
        code: Set(params.code),
        group_name: Set(params.group_name),
        description: Set(params.description),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;

    Ok(responses::ok(PermissionRecord::from(permission)))
}

#[utoipa::path(
    put,
    path = "/api/admin/permissions/{id}",
    tag = "admin-permissions",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = SavePermissionParams,
    responses((status = 200, body = ApiResponse<PermissionRecord>))
)]
#[debug_handler]
pub async fn update(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<SavePermissionParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:permission:update").await?;
    let permission = find_permission(&ctx, id).await?;
    let mut active = permission.into_active_model();
    active.name = Set(params.name);
    active.code = Set(params.code);
    active.group_name = Set(params.group_name);
    active.description = Set(params.description);
    let permission = active.update(&ctx.db).await?;

    Ok(responses::ok(PermissionRecord::from(permission)))
}

#[utoipa::path(
    delete,
    path = "/api/admin/permissions/{id}",
    tag = "admin-permissions",
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
    authorize(&ctx, &auth, "system:permission:delete").await?;
    find_permission(&ctx, id).await?;

    let grant_count = role_permissions::Entity::find()
        .filter(role_permissions::Column::PermissionId.eq(id))
        .count(&ctx.db)
        .await?;
    if grant_count > 0 {
        return Err(ApiError::bad_request("permission is assigned to roles"));
    }

    permissions::Entity::delete_by_id(id).exec(&ctx.db).await?;
    Ok(responses::empty())
}

async fn find_permission(ctx: &AppContext, id: i32) -> ApiResult<permissions::Model> {
    permissions::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("permission not found"))
}

impl From<permissions::Model> for PermissionRecord {
    fn from(permission: permissions::Model) -> Self {
        Self {
            id: permission.id,
            name: permission.name,
            code: permission.code,
            group_name: permission.group_name,
            description: permission.description,
            created_at: permission.created_at.to_rfc3339(),
            updated_at: permission.updated_at.to_rfc3339(),
        }
    }
}
