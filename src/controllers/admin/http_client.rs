#![allow(clippy::missing_errors_doc)]

use std::time::Instant;

use loco_rs::prelude::*;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter, Set};
use serde::{Deserialize, Serialize};

pub use crate::services::http_client::HttpClientRuntimeConfig;
use crate::{
    controllers::admin::authorize,
    errors::{ApiError, ApiResult},
    models::_entities::system_settings,
    responses::{self, ApiResponse},
    services::http_client::{self as http_client_service, HTTP_CLIENT_RUNTIME_KEY},
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

fn validate_test_url(value: &str) -> ApiResult<reqwest::Url> {
    let url = reqwest::Url::parse(value.trim())
        .map_err(|_| ApiError::bad_request("test url is invalid"))?;
    if matches!(url.scheme(), "http" | "https") {
        Ok(url)
    } else {
        Err(ApiError::bad_request("test url must use http or https"))
    }
}

fn trim_message(message: &str) -> String {
    message.chars().take(500).collect()
}
