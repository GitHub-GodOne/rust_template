#![allow(clippy::missing_errors_doc)]
use crate::{
    controllers::admin::authorize,
    errors::{ApiError, ApiResult},
    models::_entities::rathole_profiles,
    responses,
};
use axum::extract::Path;
use chrono::Utc;
use loco_rs::prelude::*;
use sea_orm::{ActiveModelTrait, EntityTrait, ModelTrait, Set};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveProfile {
    pub name: String,
    pub mode: String,
    pub config_toml: String,
    pub enabled: Option<bool>,
}
#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct ValidateProfile {
    pub mode: String,
    pub config_toml: String,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct GenerateKeyRequest {
    pub curve: Option<String>,
}
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ProfileRecord {
    pub id: i32,
    pub name: String,
    pub mode: String,
    pub config_toml: String,
    pub enabled: bool,
    pub status: String,
    pub last_error: Option<String>,
    pub running: bool,
    pub created_at: String,
    pub updated_at: String,
}
fn record(item: rathole_profiles::Model) -> ProfileRecord {
    ProfileRecord {
        id: item.id,
        name: item.name,
        mode: item.mode,
        config_toml: item.config_toml,
        enabled: item.enabled,
        status: item.status,
        last_error: item.last_error,
        running: crate::services::rathole_manager::is_running(item.id),
        created_at: item.created_at.to_rfc3339(),
        updated_at: item.updated_at.to_rfc3339(),
    }
}
fn mode_ok(mode: &str) -> bool {
    matches!(mode, "auto" | "server" | "client")
}
async fn validate_save(p: &SaveProfile) -> ApiResult<()> {
    if p.name.trim().is_empty() {
        return Err(ApiError::bad_request("配置名称不能为空"));
    }
    if !mode_ok(&p.mode) {
        return Err(ApiError::bad_request("mode 必须是 auto、server 或 client"));
    }
    crate::services::rathole_manager::validate(&p.config_toml, &p.mode)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
}

#[debug_handler]
pub async fn list(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:rathole:list").await?;
    let items = rathole_profiles::Entity::find()
        .all(&ctx.db)
        .await?
        .into_iter()
        .map(record)
        .collect::<Vec<_>>();
    Ok(responses::ok(items))
}
#[debug_handler]
pub async fn get(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:rathole:list").await?;
    let item = rathole_profiles::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("rathole 配置不存在"))?;
    Ok(responses::ok(record(item)))
}
#[debug_handler]
pub async fn validate(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<ValidateProfile>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:rathole:manage").await?;
    if !mode_ok(&params.mode) {
        return Err(ApiError::bad_request("mode 必须是 auto、server 或 client"));
    }
    crate::services::rathole_manager::validate(&params.config_toml, &params.mode)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(responses::ok(serde_json::json!({"valid": true})))
}
#[debug_handler]
pub async fn genkey(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<GenerateKeyRequest>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:rathole:manage").await?;
    let curve = match params.curve.as_deref() {
        Some("x448") => Some(rathole::KeypairType::X448),
        _ => Some(rathole::KeypairType::X25519),
    };
    let (private_key, public_key) =
        rathole::generate_keypair(curve).map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(responses::ok(serde_json::json!({
        "private_key": private_key,
        "public_key": public_key,
    })))
}
#[debug_handler]
pub async fn create(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SaveProfile>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:rathole:manage").await?;
    validate_save(&params).await?;
    let now = Utc::now().fixed_offset();
    let item = rathole_profiles::ActiveModel {
        name: Set(params.name.trim().to_string()),
        mode: Set(params.mode),
        config_toml: Set(params.config_toml),
        enabled: Set(params.enabled.unwrap_or(false)),
        status: Set("stopped".into()),
        last_error: Set(None),
        last_started_at: Set(None),
        last_stopped_at: Set(None),
        created_by: Set(Some(actor.id)),
        updated_by: Set(Some(actor.id)),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;
    if item.enabled {
        crate::services::rathole_manager::start(item.id, &item.mode, &item.config_toml)
            .await
            .map_err(|e| ApiError::bad_request(e.to_string()))?;
    }
    Ok(responses::ok(record(item)))
}
#[debug_handler]
pub async fn update(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<SaveProfile>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:rathole:manage").await?;
    validate_save(&params).await?;
    let old = rathole_profiles::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("rathole 配置不存在"))?;
    let mut active = old.into_active_model();
    active.name = Set(params.name.trim().to_string());
    active.mode = Set(params.mode);
    active.config_toml = Set(params.config_toml);
    active.enabled = Set(params.enabled.unwrap_or(false));
    active.updated_by = Set(Some(actor.id));
    active.updated_at = Set(Utc::now().fixed_offset());
    let item = active.update(&ctx.db).await?;
    if item.enabled {
        crate::services::rathole_manager::start(item.id, &item.mode, &item.config_toml)
            .await
            .map_err(|e| ApiError::bad_request(e.to_string()))?;
    } else {
        crate::services::rathole_manager::stop(item.id)
            .await
            .map_err(|e| ApiError::bad_request(e.to_string()))?;
    }
    Ok(responses::ok(record(item)))
}
#[debug_handler]
pub async fn delete(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:rathole:manage").await?;
    let item = rathole_profiles::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("rathole 配置不存在"))?;
    crate::services::rathole_manager::stop(id)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    item.delete(&ctx.db).await?;
    Ok(responses::empty())
}
#[debug_handler]
pub async fn start(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:rathole:run").await?;
    let item = rathole_profiles::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("rathole 配置不存在"))?;
    crate::services::rathole_manager::start(id, &item.mode, &item.config_toml)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(responses::ok(serde_json::json!({"running": true})))
}
#[debug_handler]
pub async fn stop(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:rathole:run").await?;
    crate::services::rathole_manager::stop(id)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(responses::ok(serde_json::json!({"running": false})))
}
