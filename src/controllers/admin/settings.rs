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
    models::_entities::system_settings,
    responses::{self, ApiResponse, EmptyData, PageResponse},
};

const SECRET_MASK: &str = "******";

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct SettingQueryParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub keyword: Option<String>,
    pub group_key: Option<String>,
}

impl SettingQueryParams {
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
pub struct SettingRecord {
    pub id: i32,
    pub key: String,
    pub name: String,
    pub group_key: String,
    pub value: String,
    pub value_type: String,
    pub default_value: Option<String>,
    pub description: Option<String>,
    pub is_public: bool,
    pub is_builtin: bool,
    pub is_encrypted: bool,
    pub sort_order: i32,
    pub created_by: Option<i32>,
    pub updated_by: Option<i32>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveSettingParams {
    pub key: String,
    pub name: String,
    pub group_key: String,
    pub value: String,
    pub value_type: String,
    pub default_value: Option<String>,
    pub description: Option<String>,
    pub is_public: Option<bool>,
    pub is_builtin: Option<bool>,
    pub is_encrypted: Option<bool>,
    pub sort_order: Option<i32>,
}

#[utoipa::path(
    get,
    path = "/api/admin/settings",
    tag = "admin-settings",
    security(("bearer_auth" = [])),
    params(SettingQueryParams),
    responses((status = 200, body = ApiResponse<PageResponse<SettingRecord>>))
)]
#[debug_handler]
pub async fn list(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<SettingQueryParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:setting:list").await?;

    let page = params.page();
    let page_size = params.page_size();
    let mut query = system_settings::Entity::find()
        .order_by_asc(system_settings::Column::GroupKey)
        .order_by_asc(system_settings::Column::SortOrder)
        .order_by_asc(system_settings::Column::Id);

    if let Some(keyword) = params.keyword.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(
            Condition::any()
                .add(system_settings::Column::Key.contains(keyword))
                .add(system_settings::Column::Name.contains(keyword))
                .add(system_settings::Column::Description.contains(keyword)),
        );
    }
    if let Some(group_key) = params
        .group_key
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        query = query.filter(system_settings::Column::GroupKey.eq(group_key));
    }

    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(SettingRecord::from)
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
    path = "/api/admin/settings/{id}",
    tag = "admin-settings",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<SettingRecord>))
)]
#[debug_handler]
pub async fn get(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:setting:detail").await?;
    let setting = find_setting(&ctx, id).await?;
    Ok(responses::ok(SettingRecord::from(setting)))
}

#[utoipa::path(
    post,
    path = "/api/admin/settings",
    tag = "admin-settings",
    security(("bearer_auth" = [])),
    request_body = SaveSettingParams,
    responses((status = 200, body = ApiResponse<SettingRecord>))
)]
#[debug_handler]
pub async fn create(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SaveSettingParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:setting:create").await?;
    validate_setting(&params.value_type, &params.value)?;

    let setting = system_settings::ActiveModel {
        key: Set(params.key),
        name: Set(params.name),
        group_key: Set(params.group_key),
        value: Set(params.value),
        value_type: Set(params.value_type),
        default_value: Set(params.default_value),
        description: Set(params.description),
        is_public: Set(params.is_public.unwrap_or(false)),
        is_builtin: Set(params.is_builtin.unwrap_or(false)),
        is_encrypted: Set(params.is_encrypted.unwrap_or(false)),
        sort_order: Set(params.sort_order.unwrap_or_default()),
        created_by: Set(Some(user.id)),
        updated_by: Set(Some(user.id)),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;

    Ok(responses::ok(SettingRecord::from(setting)))
}

#[utoipa::path(
    put,
    path = "/api/admin/settings/{id}",
    tag = "admin-settings",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = SaveSettingParams,
    responses((status = 200, body = ApiResponse<SettingRecord>))
)]
#[debug_handler]
pub async fn update(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<SaveSettingParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:setting:update").await?;
    let setting = find_setting(&ctx, id).await?;
    let value = if setting.value_type == "secret" && params.value == SECRET_MASK {
        setting.value.clone()
    } else {
        validate_setting(&params.value_type, &params.value)?;
        params.value
    };

    let mut active = setting.into_active_model();
    active.key = Set(params.key);
    active.name = Set(params.name);
    active.group_key = Set(params.group_key);
    active.value = Set(value);
    active.value_type = Set(params.value_type);
    active.default_value = Set(params.default_value);
    active.description = Set(params.description);
    active.is_public = Set(params.is_public.unwrap_or(false));
    active.is_builtin = Set(params.is_builtin.unwrap_or(false));
    active.is_encrypted = Set(params.is_encrypted.unwrap_or(false));
    active.sort_order = Set(params.sort_order.unwrap_or_default());
    active.updated_by = Set(Some(user.id));
    let setting = active.update(&ctx.db).await?;

    Ok(responses::ok(SettingRecord::from(setting)))
}

#[utoipa::path(
    delete,
    path = "/api/admin/settings/{id}",
    tag = "admin-settings",
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
    authorize(&ctx, &auth, "system:setting:delete").await?;
    let setting = find_setting(&ctx, id).await?;
    if setting.is_builtin {
        return Err(ApiError::bad_request("builtin setting cannot be deleted"));
    }

    system_settings::Entity::delete_by_id(id)
        .exec(&ctx.db)
        .await?;
    Ok(responses::empty())
}

async fn find_setting(ctx: &AppContext, id: i32) -> ApiResult<system_settings::Model> {
    system_settings::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("setting not found"))
}

fn validate_setting(value_type: &str, value: &str) -> ApiResult<()> {
    match value_type {
        "string" | "secret" => Ok(()),
        "number" => value
            .parse::<f64>()
            .map(|_| ())
            .map_err(|_| ApiError::bad_request("setting value must be a number")),
        "boolean" => match value {
            "true" | "false" => Ok(()),
            _ => Err(ApiError::bad_request("setting value must be true or false")),
        },
        "json" => serde_json::from_str::<serde_json::Value>(value)
            .map(|_| ())
            .map_err(|_| ApiError::bad_request("setting value must be valid json")),
        _ => Err(ApiError::bad_request("unsupported setting value type")),
    }
}

impl From<system_settings::Model> for SettingRecord {
    fn from(setting: system_settings::Model) -> Self {
        let value = if setting.value_type == "secret" {
            SECRET_MASK.to_string()
        } else {
            setting.value
        };

        Self {
            id: setting.id,
            key: setting.key,
            name: setting.name,
            group_key: setting.group_key,
            value,
            value_type: setting.value_type,
            default_value: setting.default_value,
            description: setting.description,
            is_public: setting.is_public,
            is_builtin: setting.is_builtin,
            is_encrypted: setting.is_encrypted,
            sort_order: setting.sort_order,
            created_by: setting.created_by,
            updated_by: setting.updated_by,
            created_at: setting.created_at.to_rfc3339(),
            updated_at: setting.updated_at.to_rfc3339(),
        }
    }
}
