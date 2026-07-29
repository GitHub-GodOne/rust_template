#![allow(clippy::missing_errors_doc)]

use std::{fs, path::Path as FsPath, time::Instant};

use axum::{
    http::{header, HeaderMap, HeaderName, HeaderValue},
    response::IntoResponse,
};
use loco_rs::prelude::*;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, PaginatorTrait, QueryFilter,
    QueryOrder, Set,
};
use serde::{Deserialize, Serialize};

pub use crate::services::http_client::HttpClientRuntimeConfig;
use crate::{
    controllers::admin::authorize,
    errors::{ApiError, ApiResult},
    models::_entities::{http_request_presets, http_request_runs, system_settings},
    responses::{self, ApiResponse, PageResponse},
    services::{
        http_client::{self as http_client_service, HTTP_CLIENT_RUNTIME_KEY},
        http_request_manager::{self, HttpRequestDraft, HttpRequestExecuteParams},
    },
};

const SETTING_NAME: &str = "HTTP 客户端配置";
const SETTING_GROUP: &str = "http_client";

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct HttpClientTestParams {
    pub url: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct HttpClientTestRecord {
    pub ok: bool,
    pub status_code: Option<u16>,
    pub duration_ms: u64,
    pub message: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct HttpRequestQueryParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub keyword: Option<String>,
    pub enabled: Option<bool>,
}

impl HttpRequestQueryParams {
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
pub struct HttpRequestRunQueryParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub preset_id: Option<i32>,
    pub status: Option<String>,
    pub keyword: Option<String>,
}

impl HttpRequestRunQueryParams {
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
pub struct SaveHttpRequestPresetParams {
    pub name: String,
    pub code: String,
    pub description: Option<String>,
    pub method: String,
    pub url: String,
    pub headers_json: Option<String>,
    pub query_json: Option<String>,
    pub body_type: Option<String>,
    pub body_text: Option<String>,
    pub content_type: Option<String>,
    pub source_format: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct ParseHttpRequestImportParams {
    pub format: String,
    pub content: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct HttpRequestPresetRecord {
    pub id: i32,
    pub name: String,
    pub code: String,
    pub description: Option<String>,
    pub method: String,
    pub url: String,
    pub headers_json: Option<String>,
    pub query_json: Option<String>,
    pub body_type: String,
    pub body_text: Option<String>,
    pub content_type: Option<String>,
    pub source_format: String,
    pub enabled: bool,
    pub last_used_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct HttpRequestRunRecord {
    pub id: i32,
    pub preset_id: Option<i32>,
    pub name: String,
    pub method: String,
    pub url: String,
    pub headers_json: Option<String>,
    pub query_json: Option<String>,
    pub request_body_type: String,
    pub request_body_text: Option<String>,
    pub request_body_size_bytes: i64,
    pub status: String,
    pub response_status_code: Option<i32>,
    pub response_headers_json: Option<String>,
    pub response_content_type: Option<String>,
    pub response_size_bytes: i64,
    pub duration_ms: i32,
    pub error_message: Option<String>,
    pub response_preview_text: Option<String>,
    pub has_response_body: bool,
    pub created_at: String,
}

#[utoipa::path(
    get,
    path = "/api/admin/http-client/config",
    tag = "admin-http-client",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<HttpClientRuntimeConfig>))
)]
#[debug_handler]
pub async fn get_config(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:http_client:config").await?;
    let config = http_client_service::load_http_client_config(&ctx.db).await?;
    Ok(responses::ok(config))
}

#[utoipa::path(
    put,
    path = "/api/admin/http-client/config",
    tag = "admin-http-client",
    security(("bearer_auth" = [])),
    request_body = HttpClientRuntimeConfig,
    responses((status = 200, body = ApiResponse<HttpClientRuntimeConfig>))
)]
#[debug_handler]
pub async fn update_config(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<HttpClientRuntimeConfig>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:http_client:config").await?;
    params.validate()?;
    save_config(&ctx, user.id, &params).await?;
    Ok(responses::ok(params))
}

#[utoipa::path(
    post,
    path = "/api/admin/http-client/test",
    tag = "admin-http-client",
    security(("bearer_auth" = [])),
    request_body = HttpClientTestParams,
    responses((status = 200, body = ApiResponse<HttpClientTestRecord>))
)]
#[debug_handler]
pub async fn test_request(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<HttpClientTestParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:http_client:test").await?;
    let url = validate_test_url(&params.url)?;
    let client = http_client_service::build_http_client(&ctx.db).await?;
    let started = Instant::now();
    let result = client.get(url.clone()).send().await;
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);

    let record = match result {
        Ok(response) => {
            let status = response.status();
            HttpClientTestRecord {
                ok: status.is_success(),
                status_code: Some(status.as_u16()),
                duration_ms,
                message: if status.is_success() {
                    "request completed".to_string()
                } else {
                    format!("request completed with http {status}")
                },
            }
        }
        Err(error) => HttpClientTestRecord {
            ok: false,
            status_code: None,
            duration_ms,
            message: trim_message(&error.to_string()),
        },
    };

    Ok(responses::ok(record))
}

#[debug_handler]
pub async fn list_requests(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<HttpRequestQueryParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:http_client:request:list").await?;
    let page = params.page();
    let page_size = params.page_size();
    let mut query =
        http_request_presets::Entity::find().order_by_desc(http_request_presets::Column::UpdatedAt);
    if let Some(keyword) = params
        .keyword
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        query = query.filter(
            http_request_presets::Column::Name
                .contains(keyword)
                .or(http_request_presets::Column::Code.contains(keyword))
                .or(http_request_presets::Column::Url.contains(keyword)),
        );
    }
    if let Some(enabled) = params.enabled {
        query = query.filter(http_request_presets::Column::Enabled.eq(enabled));
    }
    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(HttpRequestPresetRecord::from)
        .collect();
    Ok(responses::ok(PageResponse {
        items,
        page,
        page_size,
        total,
    }))
}

#[debug_handler]
pub async fn create_request(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SaveHttpRequestPresetParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:http_client:request:create").await?;
    ensure_preset_code_unique(&ctx, &params.code, None).await?;
    let draft = normalize_preset(&params)?;
    let preset = http_request_presets::ActiveModel {
        name: Set(params.name.trim().to_string()),
        code: Set(params.code.trim().to_string()),
        description: Set(normalize_optional(params.description)),
        method: Set(draft.method),
        url: Set(draft.url),
        headers_json: Set(draft.headers_json),
        query_json: Set(draft.query_json),
        body_type: Set(draft.body_type),
        body_text: Set(draft.body_text),
        content_type: Set(draft.content_type),
        source_format: Set(params.source_format.unwrap_or_else(|| "manual".to_string())),
        enabled: Set(params.enabled.unwrap_or(true)),
        created_by: Set(Some(actor.id)),
        updated_by: Set(Some(actor.id)),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;
    Ok(responses::ok(HttpRequestPresetRecord::from(preset)))
}

#[debug_handler]
pub async fn get_request(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:http_client:request:list").await?;
    Ok(responses::ok(HttpRequestPresetRecord::from(
        find_preset(&ctx, id).await?,
    )))
}

#[debug_handler]
pub async fn update_request(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<SaveHttpRequestPresetParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:http_client:request:update").await?;
    ensure_preset_code_unique(&ctx, &params.code, Some(id)).await?;
    let draft = normalize_preset(&params)?;
    let preset = find_preset(&ctx, id).await?;
    let mut active = preset.into_active_model();
    active.name = Set(params.name.trim().to_string());
    active.code = Set(params.code.trim().to_string());
    active.description = Set(normalize_optional(params.description));
    active.method = Set(draft.method);
    active.url = Set(draft.url);
    active.headers_json = Set(draft.headers_json);
    active.query_json = Set(draft.query_json);
    active.body_type = Set(draft.body_type);
    active.body_text = Set(draft.body_text);
    active.content_type = Set(draft.content_type);
    active.source_format = Set(params.source_format.unwrap_or_else(|| "manual".to_string()));
    active.enabled = Set(params.enabled.unwrap_or(true));
    active.updated_by = Set(Some(actor.id));
    Ok(responses::ok(HttpRequestPresetRecord::from(
        active.update(&ctx.db).await?,
    )))
}

#[debug_handler]
pub async fn delete_request(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:http_client:request:delete").await?;
    find_preset(&ctx, id).await?;
    http_request_presets::Entity::delete_by_id(id)
        .exec(&ctx.db)
        .await?;
    Ok(responses::empty())
}

#[debug_handler]
pub async fn send_request(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:http_client:request:send").await?;
    let preset = find_preset(&ctx, id).await?;
    if !preset.enabled {
        return Err(ApiError::bad_request("request preset is disabled"));
    }
    let run = http_request_manager::execute_request(
        &ctx,
        actor.id,
        Some(id),
        preset_execute_params(&preset),
    )
    .await?;
    let mut active = preset.into_active_model();
    active.last_used_at = Set(Some(chrono::Local::now().into()));
    active.update(&ctx.db).await?;
    Ok(responses::ok(HttpRequestRunRecord::from(run)))
}

#[debug_handler]
pub async fn send_ad_hoc_request(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<HttpRequestExecuteParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:http_client:request:send").await?;
    let run = http_request_manager::execute_request(&ctx, actor.id, None, params).await?;
    Ok(responses::ok(HttpRequestRunRecord::from(run)))
}

#[debug_handler]
pub async fn list_runs(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<HttpRequestRunQueryParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:http_client:run:list").await?;
    let page = params.page();
    let page_size = params.page_size();
    let mut query =
        http_request_runs::Entity::find().order_by_desc(http_request_runs::Column::CreatedAt);
    if let Some(preset_id) = params.preset_id {
        query = query.filter(http_request_runs::Column::PresetId.eq(preset_id));
    }
    if let Some(status) = params
        .status
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        query = query.filter(http_request_runs::Column::Status.eq(status));
    }
    if let Some(keyword) = params
        .keyword
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        query = query.filter(
            http_request_runs::Column::Name
                .contains(keyword)
                .or(http_request_runs::Column::Url.contains(keyword)),
        );
    }
    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(HttpRequestRunRecord::from)
        .collect();
    Ok(responses::ok(PageResponse {
        items,
        page,
        page_size,
        total,
    }))
}

#[debug_handler]
pub async fn get_run(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:http_client:run:list").await?;
    Ok(responses::ok(HttpRequestRunRecord::from(
        find_run(&ctx, id).await?,
    )))
}

#[debug_handler]
pub async fn download_run_result(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:http_client:run:download").await?;
    let run = find_run(&ctx, id).await?;
    let storage_path = run
        .response_storage_path
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::bad_request("request run has no saved response"))?;
    let path = FsPath::new(storage_path);
    let bytes = fs::read(path).map_err(|_| ApiError::bad_request("response file not found"))?;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        run.response_content_type
            .as_deref()
            .unwrap_or("application/octet-stream")
            .parse()
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"http-response-{id}.bin\""))
            .map_err(|_| ApiError::internal("failed to build download response"))?,
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    Ok((headers, bytes).into_response())
}

#[debug_handler]
pub async fn parse_import(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<ParseHttpRequestImportParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:http_client:request:create").await?;
    let _ = ctx;
    Ok(responses::ok(http_request_manager::parse_import(
        &params.format,
        &params.content,
    )?))
}

async fn save_config(
    ctx: &AppContext,
    user_id: i32,
    config: &HttpClientRuntimeConfig,
) -> ApiResult<()> {
    let value = serde_json::to_string(config)
        .map_err(|_| ApiError::internal("failed to serialize http client config"))?;
    let default_value = serde_json::to_string(&HttpClientRuntimeConfig::default())
        .map_err(|_| ApiError::internal("failed to serialize http client config"))?;
    let setting = system_settings::Entity::find()
        .filter(system_settings::Column::Key.eq(HTTP_CLIENT_RUNTIME_KEY))
        .one(&ctx.db)
        .await?;

    if let Some(setting) = setting {
        let mut active = setting.into_active_model();
        active.value = Set(value);
        active.value_type = Set("json".to_string());
        active.default_value = Set(Some(default_value));
        active.updated_by = Set(Some(user_id));
        active.update(&ctx.db).await?;
    } else {
        system_settings::ActiveModel {
            key: Set(HTTP_CLIENT_RUNTIME_KEY.to_string()),
            name: Set(SETTING_NAME.to_string()),
            group_key: Set(SETTING_GROUP.to_string()),
            value: Set(value),
            value_type: Set("json".to_string()),
            default_value: Set(Some(default_value)),
            description: Set(Some("项目外部 HTTP 请求的 reqwest 运行时参数".to_string())),
            is_public: Set(false),
            is_builtin: Set(true),
            is_encrypted: Set(false),
            sort_order: Set(205),
            created_by: Set(Some(user_id)),
            updated_by: Set(Some(user_id)),
            ..Default::default()
        }
        .insert(&ctx.db)
        .await?;
    }
    Ok(())
}

fn normalize_preset(params: &SaveHttpRequestPresetParams) -> ApiResult<HttpRequestDraft> {
    if params.name.trim().is_empty() || params.code.trim().is_empty() {
        return Err(ApiError::bad_request("name and code are required"));
    }
    http_request_manager::normalize_request(HttpRequestExecuteParams {
        name: Some(params.name.clone()),
        method: params.method.clone(),
        url: params.url.clone(),
        headers_json: params.headers_json.clone(),
        query_json: params.query_json.clone(),
        body_type: params.body_type.clone(),
        body_text: params.body_text.clone(),
        content_type: params.content_type.clone(),
    })
}

async fn ensure_preset_code_unique(
    ctx: &AppContext,
    code: &str,
    ignore_id: Option<i32>,
) -> ApiResult<()> {
    let existing = http_request_presets::Entity::find()
        .filter(http_request_presets::Column::Code.eq(code.trim()))
        .one(&ctx.db)
        .await?;
    if existing.is_some_and(|preset| Some(preset.id) != ignore_id) {
        return Err(ApiError::bad_request("请求编码已存在"));
    }
    Ok(())
}

async fn find_preset(ctx: &AppContext, id: i32) -> ApiResult<http_request_presets::Model> {
    http_request_presets::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("request preset not found"))
}

async fn find_run(ctx: &AppContext, id: i32) -> ApiResult<http_request_runs::Model> {
    http_request_runs::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("request run not found"))
}

fn preset_execute_params(preset: &http_request_presets::Model) -> HttpRequestExecuteParams {
    HttpRequestExecuteParams {
        name: Some(preset.name.clone()),
        method: preset.method.clone(),
        url: preset.url.clone(),
        headers_json: preset.headers_json.clone(),
        query_json: preset.query_json.clone(),
        body_type: Some(preset.body_type.clone()),
        body_text: preset.body_text.clone(),
        content_type: preset.content_type.clone(),
    }
}

fn validate_test_url(value: &str) -> ApiResult<reqwest::Url> {
    let url = reqwest::Url::parse(value.trim())
        .map_err(|_| ApiError::bad_request("test url is invalid"))?;
    if matches!(url.scheme(), "http" | "https") {
        Ok(url)
    } else {
        Err(ApiError::bad_request("test url must use http or https"))
    }
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim().to_string();
        (!trimmed.is_empty()).then_some(trimmed)
    })
}

fn trim_message(message: &str) -> String {
    message.chars().take(500).collect()
}

impl From<http_request_presets::Model> for HttpRequestPresetRecord {
    fn from(preset: http_request_presets::Model) -> Self {
        Self {
            id: preset.id,
            name: preset.name,
            code: preset.code,
            description: preset.description,
            method: preset.method,
            url: preset.url,
            headers_json: preset.headers_json,
            query_json: preset.query_json,
            body_type: preset.body_type,
            body_text: preset.body_text,
            content_type: preset.content_type,
            source_format: preset.source_format,
            enabled: preset.enabled,
            last_used_at: preset.last_used_at.map(|value| value.to_rfc3339()),
            created_at: preset.created_at.to_rfc3339(),
            updated_at: preset.updated_at.to_rfc3339(),
        }
    }
}

impl From<http_request_runs::Model> for HttpRequestRunRecord {
    fn from(run: http_request_runs::Model) -> Self {
        let has_response_body = run.response_storage_path.is_some();
        Self {
            id: run.id,
            preset_id: run.preset_id,
            name: run.name,
            method: run.method,
            url: run.url,
            headers_json: run.headers_json,
            query_json: run.query_json,
            request_body_type: run.request_body_type,
            request_body_text: run.request_body_text,
            request_body_size_bytes: run.request_body_size_bytes,
            status: run.status,
            response_status_code: run.response_status_code,
            response_headers_json: run.response_headers_json,
            response_content_type: run.response_content_type,
            response_size_bytes: run.response_size_bytes,
            duration_ms: run.duration_ms,
            error_message: run.error_message,
            response_preview_text: run.response_preview_text,
            has_response_body,
            created_at: run.created_at.to_rfc3339(),
        }
    }
}
