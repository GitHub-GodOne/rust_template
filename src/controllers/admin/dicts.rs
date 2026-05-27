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
    models::_entities::{dict_items, dict_types},
    responses::{self, ApiResponse, EmptyData, PageParams, PageResponse},
};

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct DictTypeRecord {
    pub id: i32,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub is_builtin: bool,
    pub sort_order: i32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveDictTypeParams {
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub enabled: Option<bool>,
    pub is_builtin: Option<bool>,
    pub sort_order: Option<i32>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct DictItemRecord {
    pub id: i32,
    pub dict_type_id: i32,
    pub label: String,
    pub value: String,
    pub color: Option<String>,
    pub extra: Option<String>,
    pub enabled: bool,
    pub is_default: bool,
    pub sort_order: i32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveDictItemParams {
    pub dict_type_id: i32,
    pub label: String,
    pub value: String,
    pub color: Option<String>,
    pub extra: Option<String>,
    pub enabled: Option<bool>,
    pub is_default: Option<bool>,
    pub sort_order: Option<i32>,
}

#[utoipa::path(
    get,
    path = "/api/admin/dict-types",
    tag = "admin-dicts",
    security(("bearer_auth" = [])),
    params(PageParams),
    responses((status = 200, body = ApiResponse<PageResponse<DictTypeRecord>>))
)]
#[debug_handler]
pub async fn list_types(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<PageParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:dict:list").await?;

    let page = params.page();
    let page_size = params.page_size();
    let mut query = dict_types::Entity::find()
        .order_by_asc(dict_types::Column::SortOrder)
        .order_by_asc(dict_types::Column::Id);

    if let Some(keyword) = params.keyword.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(
            Condition::any()
                .add(dict_types::Column::Code.contains(keyword))
                .add(dict_types::Column::Name.contains(keyword))
                .add(dict_types::Column::Description.contains(keyword)),
        );
    }

    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(DictTypeRecord::from)
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
    path = "/api/admin/dict-types/{id}",
    tag = "admin-dicts",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<DictTypeRecord>))
)]
#[debug_handler]
pub async fn get_type(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:dict:detail").await?;
    let dict_type = find_type(&ctx, id).await?;
    Ok(responses::ok(DictTypeRecord::from(dict_type)))
}

#[utoipa::path(
    post,
    path = "/api/admin/dict-types",
    tag = "admin-dicts",
    security(("bearer_auth" = [])),
    request_body = SaveDictTypeParams,
    responses((status = 200, body = ApiResponse<DictTypeRecord>))
)]
#[debug_handler]
pub async fn create_type(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SaveDictTypeParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:dict:create").await?;
    let dict_type = dict_types::ActiveModel {
        code: Set(params.code),
        name: Set(params.name),
        description: Set(params.description),
        enabled: Set(params.enabled.unwrap_or(true)),
        is_builtin: Set(params.is_builtin.unwrap_or(false)),
        sort_order: Set(params.sort_order.unwrap_or_default()),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;

    Ok(responses::ok(DictTypeRecord::from(dict_type)))
}

#[utoipa::path(
    put,
    path = "/api/admin/dict-types/{id}",
    tag = "admin-dicts",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = SaveDictTypeParams,
    responses((status = 200, body = ApiResponse<DictTypeRecord>))
)]
#[debug_handler]
pub async fn update_type(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<SaveDictTypeParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:dict:update").await?;
    let dict_type = find_type(&ctx, id).await?;
    let mut active = dict_type.into_active_model();
    active.code = Set(params.code);
    active.name = Set(params.name);
    active.description = Set(params.description);
    active.enabled = Set(params.enabled.unwrap_or(true));
    active.is_builtin = Set(params.is_builtin.unwrap_or(false));
    active.sort_order = Set(params.sort_order.unwrap_or_default());
    let dict_type = active.update(&ctx.db).await?;

    Ok(responses::ok(DictTypeRecord::from(dict_type)))
}

#[utoipa::path(
    delete,
    path = "/api/admin/dict-types/{id}",
    tag = "admin-dicts",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn delete_type(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:dict:delete").await?;
    let dict_type = find_type(&ctx, id).await?;
    if dict_type.is_builtin {
        return Err(ApiError::bad_request("builtin dict type cannot be deleted"));
    }

    let item_count = dict_items::Entity::find()
        .filter(dict_items::Column::DictTypeId.eq(id))
        .count(&ctx.db)
        .await?;
    if item_count > 0 {
        return Err(ApiError::bad_request("dict type has items"));
    }

    dict_types::Entity::delete_by_id(id).exec(&ctx.db).await?;
    Ok(responses::empty())
}

#[utoipa::path(
    get,
    path = "/api/admin/dict-types/{id}/items",
    tag = "admin-dicts",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<Vec<DictItemRecord>>))
)]
#[debug_handler]
pub async fn list_items(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:dict:list").await?;
    find_type(&ctx, id).await?;
    let items: Vec<DictItemRecord> = dict_items::Entity::find()
        .filter(dict_items::Column::DictTypeId.eq(id))
        .order_by_asc(dict_items::Column::SortOrder)
        .order_by_asc(dict_items::Column::Id)
        .all(&ctx.db)
        .await?
        .into_iter()
        .map(DictItemRecord::from)
        .collect();

    Ok(responses::ok(items))
}

#[utoipa::path(
    post,
    path = "/api/admin/dict-items",
    tag = "admin-dicts",
    security(("bearer_auth" = [])),
    request_body = SaveDictItemParams,
    responses((status = 200, body = ApiResponse<DictItemRecord>))
)]
#[debug_handler]
pub async fn create_item(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SaveDictItemParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:dict:create").await?;
    find_type(&ctx, params.dict_type_id).await?;
    validate_extra(params.extra.as_deref())?;

    let item = dict_items::ActiveModel {
        dict_type_id: Set(params.dict_type_id),
        label: Set(params.label),
        value: Set(params.value),
        color: Set(params.color),
        extra: Set(params.extra),
        enabled: Set(params.enabled.unwrap_or(true)),
        is_default: Set(params.is_default.unwrap_or(false)),
        sort_order: Set(params.sort_order.unwrap_or_default()),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;

    Ok(responses::ok(DictItemRecord::from(item)))
}

#[utoipa::path(
    put,
    path = "/api/admin/dict-items/{id}",
    tag = "admin-dicts",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = SaveDictItemParams,
    responses((status = 200, body = ApiResponse<DictItemRecord>))
)]
#[debug_handler]
pub async fn update_item(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<SaveDictItemParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:dict:update").await?;
    find_type(&ctx, params.dict_type_id).await?;
    validate_extra(params.extra.as_deref())?;
    let item = find_item(&ctx, id).await?;

    let mut active = item.into_active_model();
    active.dict_type_id = Set(params.dict_type_id);
    active.label = Set(params.label);
    active.value = Set(params.value);
    active.color = Set(params.color);
    active.extra = Set(params.extra);
    active.enabled = Set(params.enabled.unwrap_or(true));
    active.is_default = Set(params.is_default.unwrap_or(false));
    active.sort_order = Set(params.sort_order.unwrap_or_default());
    let item = active.update(&ctx.db).await?;

    Ok(responses::ok(DictItemRecord::from(item)))
}

#[utoipa::path(
    delete,
    path = "/api/admin/dict-items/{id}",
    tag = "admin-dicts",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn delete_item(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:dict:delete").await?;
    let item = find_item(&ctx, id).await?;
    if item.is_default {
        return Err(ApiError::bad_request("default dict item cannot be deleted"));
    }

    dict_items::Entity::delete_by_id(id).exec(&ctx.db).await?;
    Ok(responses::empty())
}

async fn find_type(ctx: &AppContext, id: i32) -> ApiResult<dict_types::Model> {
    dict_types::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("dict type not found"))
}

async fn find_item(ctx: &AppContext, id: i32) -> ApiResult<dict_items::Model> {
    dict_items::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("dict item not found"))
}

fn validate_extra(extra: Option<&str>) -> ApiResult<()> {
    if let Some(extra) = extra.filter(|value| !value.is_empty()) {
        serde_json::from_str::<serde_json::Value>(extra)
            .map_err(|_| ApiError::bad_request("dict item extra must be valid json"))?;
    }
    Ok(())
}

impl From<dict_types::Model> for DictTypeRecord {
    fn from(dict_type: dict_types::Model) -> Self {
        Self {
            id: dict_type.id,
            code: dict_type.code,
            name: dict_type.name,
            description: dict_type.description,
            enabled: dict_type.enabled,
            is_builtin: dict_type.is_builtin,
            sort_order: dict_type.sort_order,
            created_at: dict_type.created_at.to_rfc3339(),
            updated_at: dict_type.updated_at.to_rfc3339(),
        }
    }
}

impl From<dict_items::Model> for DictItemRecord {
    fn from(item: dict_items::Model) -> Self {
        Self {
            id: item.id,
            dict_type_id: item.dict_type_id,
            label: item.label,
            value: item.value,
            color: item.color,
            extra: item.extra,
            enabled: item.enabled,
            is_default: item.is_default,
            sort_order: item.sort_order,
            created_at: item.created_at.to_rfc3339(),
            updated_at: item.updated_at.to_rfc3339(),
        }
    }
}
