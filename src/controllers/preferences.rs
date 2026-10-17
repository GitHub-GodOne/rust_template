//! Self-service per-user UI preferences (no admin permission needed: every
//! signed-in user reads and writes only their own rows).

#![allow(clippy::missing_errors_doc)]

use loco_rs::prelude::*;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter, Set};
use serde::{Deserialize, Serialize};

use crate::{
    errors::{ApiError, ApiResult},
    models::{_entities::user_preferences, users},
    responses::{self, ApiResponse, EmptyData},
};

/// Keys clients may store; extend when a new preference is introduced.
const ALLOWED_KEYS: &[&str] = &["dock"];
const MAX_VALUE_BYTES: usize = 16 * 1024;

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct PreferenceRecord {
    pub key: String,
    /// `null` when the user has not saved this preference yet.
    #[schema(value_type = Object)]
    pub value: Option<serde_json::Value>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SavePreferenceParams {
    #[schema(value_type = Object)]
    pub value: serde_json::Value,
}

fn ensure_key(key: &str) -> ApiResult<()> {
    if ALLOWED_KEYS.contains(&key) {
        Ok(())
    } else {
        Err(ApiError::bad_request("unsupported preference key"))
    }
}

async fn current_user(ctx: &AppContext, auth: &auth::JWT) -> ApiResult<users::Model> {
    Ok(users::Model::find_by_pid(&ctx.db, &auth.claims.pid).await?)
}

async fn find(
    ctx: &AppContext,
    user_id: i32,
    key: &str,
) -> ApiResult<Option<user_preferences::Model>> {
    Ok(user_preferences::Entity::find()
        .filter(user_preferences::Column::UserId.eq(user_id))
        .filter(user_preferences::Column::PrefKey.eq(key))
        .one(&ctx.db)
        .await?)
}

fn record(key: &str, model: Option<user_preferences::Model>) -> PreferenceRecord {
    PreferenceRecord {
        key: key.to_string(),
        value: model
            .as_ref()
            .and_then(|model| serde_json::from_str(&model.value).ok()),
        updated_at: model.map(|model| model.updated_at.to_rfc3339()),
    }
}

#[utoipa::path(
    get,
    path = "/api/user/preferences/{key}",
    tag = "user-preferences",
    security(("bearer_auth" = [])),
    params(("key" = String, Path)),
    responses((status = 200, body = ApiResponse<PreferenceRecord>))
)]
#[debug_handler]
pub async fn get(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(key): Path<String>,
) -> ApiResult<Response> {
    ensure_key(&key)?;
    let user = current_user(&ctx, &auth).await?;
    let model = find(&ctx, user.id, &key).await?;
    Ok(responses::ok(record(&key, model)))
}

#[utoipa::path(
    put,
    path = "/api/user/preferences/{key}",
    tag = "user-preferences",
    security(("bearer_auth" = [])),
    params(("key" = String, Path)),
    request_body = SavePreferenceParams,
    responses((status = 200, body = ApiResponse<PreferenceRecord>))
)]
#[debug_handler]
pub async fn save(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(key): Path<String>,
    Json(params): Json<SavePreferenceParams>,
) -> ApiResult<Response> {
    ensure_key(&key)?;
    if !params.value.is_object() {
        return Err(ApiError::bad_request(
            "preference value must be a JSON object",
        ));
    }
    let value = serde_json::to_string(&params.value)
        .map_err(|_| ApiError::bad_request("invalid preference value"))?;
    if value.len() > MAX_VALUE_BYTES {
        return Err(ApiError::bad_request("preference value is too large"));
    }
    let user = current_user(&ctx, &auth).await?;
    let saved = match find(&ctx, user.id, &key).await? {
        Some(existing) => {
            let mut active = existing.into_active_model();
            active.value = Set(value);
            active.update(&ctx.db).await?
        }
        None => {
            user_preferences::ActiveModel {
                user_id: Set(user.id),
                pref_key: Set(key.clone()),
                value: Set(value),
                ..Default::default()
            }
            .insert(&ctx.db)
            .await?
        }
    };
    Ok(responses::ok(record(&key, Some(saved))))
}

#[utoipa::path(
    delete,
    path = "/api/user/preferences/{key}",
    tag = "user-preferences",
    security(("bearer_auth" = [])),
    params(("key" = String, Path)),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn reset(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(key): Path<String>,
) -> ApiResult<Response> {
    ensure_key(&key)?;
    let user = current_user(&ctx, &auth).await?;
    user_preferences::Entity::delete_many()
        .filter(user_preferences::Column::UserId.eq(user.id))
        .filter(user_preferences::Column::PrefKey.eq(key))
        .exec(&ctx.db)
        .await?;
    Ok(responses::empty())
}

#[must_use]
pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/user/preferences")
        .add("/{key}", axum::routing::get(get).put(save).delete(reset))
}
