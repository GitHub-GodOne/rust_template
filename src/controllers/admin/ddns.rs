#![allow(clippy::missing_errors_doc)]

use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    controllers::admin::authorize,
    errors::ApiResult,
    responses::{self, ApiResponse},
    services::cloudflare_ddns::{self, DdnsConfig, DdnsStatus, SaveDdnsConfigParams},
};

#[derive(Debug, Default, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SyncDdnsParams {
    /// Publish this IPv6 once instead of the configured address source.
    pub ipv6: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/admin/ddns/status",
    tag = "admin-ddns",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<DdnsStatus>))
)]
#[debug_handler]
pub async fn status(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:ddns:view").await?;
    Ok(responses::ok(cloudflare_ddns::status(&ctx.db).await?))
}

#[utoipa::path(
    get,
    path = "/api/admin/ddns/config",
    tag = "admin-ddns",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<DdnsConfig>))
)]
#[debug_handler]
pub async fn get_config(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:ddns:view").await?;
    Ok(responses::ok(cloudflare_ddns::load_config(&ctx.db).await?))
}

#[utoipa::path(
    put,
    path = "/api/admin/ddns/config",
    tag = "admin-ddns",
    security(("bearer_auth" = [])),
    request_body = SaveDdnsConfigParams,
    responses((status = 200, body = ApiResponse<DdnsConfig>))
)]
#[debug_handler]
pub async fn update_config(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SaveDdnsConfigParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:ddns:update").await?;
    Ok(responses::ok(
        cloudflare_ddns::save_config(&ctx.db, user.id, params).await?,
    ))
}

#[utoipa::path(
    post,
    path = "/api/admin/ddns/sync",
    tag = "admin-ddns",
    security(("bearer_auth" = [])),
    request_body = SyncDdnsParams,
    responses((status = 200, body = ApiResponse<DdnsStatus>))
)]
#[debug_handler]
pub async fn sync(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    body: axum::body::Bytes,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:ddns:update").await?;
    // The body is optional: an empty POST syncs the configured address source.
    let params: SyncDdnsParams = if body.iter().all(u8::is_ascii_whitespace) {
        SyncDdnsParams::default()
    } else {
        serde_json::from_slice(&body)
            .map_err(|_| crate::errors::ApiError::bad_request("invalid sync request body"))?
    };
    Ok(responses::ok(
        cloudflare_ddns::sync_now(&ctx.db, params.ipv6.as_deref()).await?,
    ))
}
