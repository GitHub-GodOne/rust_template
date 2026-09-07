use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode},
    middleware::Next,
    response::Response,
    Router as AXRouter,
};
use chrono::{Duration, Utc};
use loco_rs::{app::AppContext, controller::middleware::MiddlewareLayer, Result};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter, Set};

use crate::{
    models::{_entities::command_api_keys, users},
    responses,
};

const LAST_USED_WRITE_INTERVAL_SECONDS: i64 = 30;

pub struct ApiKeyAuthMiddleware {
    ctx: AppContext,
}

impl ApiKeyAuthMiddleware {
    #[must_use]
    pub fn new(ctx: &AppContext) -> Self {
        Self { ctx: ctx.clone() }
    }
}

impl MiddlewareLayer for ApiKeyAuthMiddleware {
    fn name(&self) -> &'static str {
        "api_key_auth"
    }

    fn config(&self) -> serde_json::Result<serde_json::Value> {
        Ok(serde_json::json!({ "enabled": true }))
    }

    fn apply(&self, app: AXRouter<AppContext>) -> Result<AXRouter<AppContext>> {
        Ok(app.layer(axum::middleware::from_fn_with_state(
            self.ctx.clone(),
            authenticate_api_key,
        )))
    }
}

async fn authenticate_api_key(
    State(ctx): State<AppContext>,
    mut request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();
    if !path.starts_with("/api/") || path.starts_with("/api/command-api/") {
        return next.run(request).await;
    }

    let Some(token) = bearer_token(request.headers()) else {
        return next.run(request).await;
    };
    if looks_like_jwt(token) {
        return next.run(request).await;
    }

    let key = match command_api_keys::Entity::find()
        .filter(command_api_keys::Column::Token.eq(token))
        .one(&ctx.db)
        .await
    {
        Ok(Some(key)) => key,
        Ok(None) => return next.run(request).await,
        Err(error) => {
            tracing::error!(error = error.to_string(), "API key lookup failed");
            return responses::error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "API_KEY_LOOKUP_FAILED",
                "API key authentication failed",
            );
        }
    };

    if !key.enabled {
        return unauthorized("API key is disabled");
    }
    let Some(owner_id) = key.created_by else {
        return unauthorized("API key is not bound to a user; edit or recreate it");
    };
    let owner = match users::Entity::find_by_id(owner_id).one(&ctx.db).await {
        Ok(Some(owner)) => owner,
        Ok(None) => return unauthorized("API key owner no longer exists"),
        Err(error) => {
            tracing::error!(error = error.to_string(), "API key owner lookup failed");
            return responses::error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "API_KEY_OWNER_LOOKUP_FAILED",
                "API key authentication failed",
            );
        }
    };

    let jwt_config = match ctx.config.get_jwt_config() {
        Ok(config) => config,
        Err(error) => {
            tracing::error!(error = error.to_string(), "JWT configuration unavailable");
            return responses::error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "JWT_CONFIG_UNAVAILABLE",
                "API key authentication failed",
            );
        }
    };
    let jwt = match owner.generate_jwt(&jwt_config.secret, jwt_config.expiration) {
        Ok(jwt) => jwt,
        Err(error) => {
            tracing::error!(error = error.to_string(), "API key JWT generation failed");
            return responses::error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "API_KEY_JWT_FAILED",
                "API key authentication failed",
            );
        }
    };
    let Ok(authorization) = HeaderValue::from_str(&format!("Bearer {jwt}")) else {
        return responses::error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "API_KEY_JWT_HEADER_FAILED",
            "API key authentication failed",
        );
    };
    request
        .headers_mut()
        .insert(header::AUTHORIZATION, authorization);
    if let Ok(value) = HeaderValue::from_str(&key.id.to_string()) {
        request
            .headers_mut()
            .insert(HeaderName::from_static("x-auth-api-key-id"), value);
    }

    update_last_used(&ctx, key).await;
    next.run(request).await
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?.trim();
    value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn looks_like_jwt(token: &str) -> bool {
    let mut segments = token.split('.');
    matches!(
        (segments.next(), segments.next(), segments.next(), segments.next()),
        (Some(header), Some(payload), Some(signature), None)
            if !header.is_empty() && !payload.is_empty() && !signature.is_empty()
    )
}

async fn update_last_used(ctx: &AppContext, key: command_api_keys::Model) {
    let now = Utc::now().fixed_offset();
    let should_update = key.last_used_at.is_none_or(|last_used| {
        now.signed_duration_since(last_used) >= Duration::seconds(LAST_USED_WRITE_INTERVAL_SECONDS)
    });
    if !should_update {
        return;
    }

    let mut active = key.into_active_model();
    active.last_used_at = Set(Some(now));
    if let Err(error) = active.update(&ctx.db).await {
        tracing::warn!(
            error = error.to_string(),
            "failed to update API key usage time"
        );
    }
}

fn unauthorized(message: &str) -> Response {
    responses::error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", message)
}

#[cfg(test)]
mod tests {
    use super::looks_like_jwt;

    #[test]
    fn distinguishes_jwt_from_permanent_api_key() {
        assert!(looks_like_jwt("header.payload.signature"));
        assert!(!looks_like_jwt("eyJ0_1234567890"));
        assert!(!looks_like_jwt("custom-token"));
    }
}
