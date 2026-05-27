#![allow(clippy::missing_errors_doc)]

use std::collections::BTreeMap;

use loco_rs::prelude::*;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, Set,
};
use serde::{Deserialize, Serialize};

use crate::{
    controllers::admin::authorize,
    errors::{ApiError, ApiResult},
    models::_entities::{menus, role_menus},
    responses::{self, ApiResponse, EmptyData},
};

#[derive(Clone, Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct MenuRecord {
    pub id: i32,
    pub parent_id: Option<i32>,
    pub title: String,
    pub path: Option<String>,
    pub icon: Option<String>,
    pub permission_code: Option<String>,
    pub sort_order: i32,
    pub visible: bool,
    pub enabled: bool,
    pub children: Vec<MenuRecord>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveMenuParams {
    pub parent_id: Option<i32>,
    pub title: String,
    pub path: Option<String>,
    pub icon: Option<String>,
    pub permission_code: Option<String>,
    pub sort_order: Option<i32>,
    pub visible: Option<bool>,
    pub enabled: Option<bool>,
}

#[utoipa::path(
    get,
    path = "/api/admin/menus",
    tag = "admin-menus",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<Vec<MenuRecord>>))
)]
#[debug_handler]
pub async fn list(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:menu:list").await?;
    let menus = menus::Entity::find()
        .order_by_asc(menus::Column::SortOrder)
        .order_by_asc(menus::Column::Id)
        .all(&ctx.db)
        .await?;

    Ok(responses::ok(build_tree(menus)))
}

#[utoipa::path(
    get,
    path = "/api/admin/menus/{id}",
    tag = "admin-menus",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<MenuRecord>))
)]
#[debug_handler]
pub async fn get(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:menu:list").await?;
    let menu = find_menu(&ctx, id).await?;
    Ok(responses::ok(MenuRecord::from(menu)))
}

#[utoipa::path(
    post,
    path = "/api/admin/menus",
    tag = "admin-menus",
    security(("bearer_auth" = [])),
    request_body = SaveMenuParams,
    responses((status = 200, body = ApiResponse<MenuRecord>))
)]
#[debug_handler]
pub async fn create(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SaveMenuParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:menu:create").await?;
    ensure_parent_exists(&ctx, params.parent_id).await?;

    let menu = menus::ActiveModel {
        parent_id: Set(params.parent_id),
        title: Set(params.title),
        path: Set(params.path),
        icon: Set(params.icon),
        permission_code: Set(params.permission_code),
        sort_order: Set(params.sort_order.unwrap_or_default()),
        visible: Set(params.visible.unwrap_or(true)),
        enabled: Set(params.enabled.unwrap_or(true)),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;

    Ok(responses::ok(MenuRecord::from(menu)))
}

#[utoipa::path(
    put,
    path = "/api/admin/menus/{id}",
    tag = "admin-menus",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = SaveMenuParams,
    responses((status = 200, body = ApiResponse<MenuRecord>))
)]
#[debug_handler]
pub async fn update(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<SaveMenuParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:menu:update").await?;
    if params.parent_id == Some(id) {
        return Err(ApiError::bad_request("menu cannot be its own parent"));
    }
    ensure_parent_exists(&ctx, params.parent_id).await?;

    let menu = find_menu(&ctx, id).await?;
    let mut active = menu.into_active_model();
    active.parent_id = Set(params.parent_id);
    active.title = Set(params.title);
    active.path = Set(params.path);
    active.icon = Set(params.icon);
    active.permission_code = Set(params.permission_code);
    active.sort_order = Set(params.sort_order.unwrap_or_default());
    active.visible = Set(params.visible.unwrap_or(true));
    active.enabled = Set(params.enabled.unwrap_or(true));
    let menu = active.update(&ctx.db).await?;

    Ok(responses::ok(MenuRecord::from(menu)))
}

#[utoipa::path(
    delete,
    path = "/api/admin/menus/{id}",
    tag = "admin-menus",
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
    authorize(&ctx, &auth, "system:menu:delete").await?;
    find_menu(&ctx, id).await?;

    let child_count = menus::Entity::find()
        .filter(menus::Column::ParentId.eq(id))
        .count(&ctx.db)
        .await?;
    if child_count > 0 {
        return Err(ApiError::bad_request("menu has children"));
    }

    let grant_count = role_menus::Entity::find()
        .filter(role_menus::Column::MenuId.eq(id))
        .count(&ctx.db)
        .await?;
    if grant_count > 0 {
        return Err(ApiError::bad_request("menu is assigned to roles"));
    }

    menus::Entity::delete_by_id(id).exec(&ctx.db).await?;
    Ok(responses::empty())
}

async fn find_menu(ctx: &AppContext, id: i32) -> ApiResult<menus::Model> {
    menus::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("menu not found"))
}

async fn ensure_parent_exists(ctx: &AppContext, parent_id: Option<i32>) -> ApiResult<()> {
    if let Some(parent_id) = parent_id {
        find_menu(ctx, parent_id).await?;
    }
    Ok(())
}

fn build_tree(menus: Vec<menus::Model>) -> Vec<MenuRecord> {
    let mut children_by_parent = BTreeMap::<Option<i32>, Vec<menus::Model>>::new();
    for menu in menus {
        children_by_parent
            .entry(menu.parent_id)
            .or_default()
            .push(menu);
    }
    build_children(None, &children_by_parent)
}

fn build_children(
    parent_id: Option<i32>,
    children_by_parent: &BTreeMap<Option<i32>, Vec<menus::Model>>,
) -> Vec<MenuRecord> {
    children_by_parent
        .get(&parent_id)
        .map(|children| {
            children
                .iter()
                .map(|menu| {
                    let mut record = MenuRecord::from(menu.clone());
                    record.children = build_children(Some(menu.id), children_by_parent);
                    record
                })
                .collect()
        })
        .unwrap_or_default()
}

impl From<menus::Model> for MenuRecord {
    fn from(menu: menus::Model) -> Self {
        Self {
            id: menu.id,
            parent_id: menu.parent_id,
            title: menu.title,
            path: menu.path,
            icon: menu.icon,
            permission_code: menu.permission_code,
            sort_order: menu.sort_order,
            visible: menu.visible,
            enabled: menu.enabled,
            children: Vec::new(),
        }
    }
}
