//! Rejects access tokens whose login session was revoked or expired, or
//! whose user was disabled, and hands the session to handlers as a request
//! extension (`user_sessions::CurrentSession`).

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
    Router as AXRouter,
};
use loco_rs::{app::AppContext, auth::jwt::JWT, controller::middleware::MiddlewareLayer, Result};

use super::api_key_auth::{bearer_token, looks_like_jwt};
use crate::{models::user_sessions, responses};

/// Auth endpoints that must work with a stale or missing token.
const OPEN_AUTH_PATHS: &[&str] = &[
    "/api/auth/login",
    "/api/auth/refresh",
    "/api/auth/logout",
    "/api/auth/register",
    "/api/auth/verify",
    "/api/auth/forgot",
    "/api/auth/reset",
    "/api/auth/magic-link",
    "/api/auth/resend-verification-mail",
    "/api/auth/captcha",
];

pub struct SessionGuardMiddleware {
    ctx: AppContext,
}

impl SessionGuardMiddleware {
    #[must_use]
    pub fn new(ctx: &AppContext) -> Self {
        Self { ctx: ctx.clone() }
    }
}

impl MiddlewareLayer for SessionGuardMiddleware {
    fn name(&self) -> &'static str {
        "session_guard"
    }

    fn config(&self) -> serde_json::Result<serde_json::Value> {
        Ok(serde_json::json!({ "enabled": true }))
    }

    fn apply(&self, app: AXRouter<AppContext>) -> Result<AXRouter<AppContext>> {
        Ok(app.layer(axum::middleware::from_fn_with_state(
            self.ctx.clone(),
            guard_session,
        )))
    }
}

fn is_open_path(path: &str) -> bool {
    OPEN_AUTH_PATHS
        .iter()
        .any(|open| path == *open || path.starts_with(&format!("{open}/")))
}

async fn guard_session(
    State(ctx): State<AppContext>,
    mut request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();
    if !path.starts_with("/api/") || is_open_path(path) {
        return next.run(request).await;
    }
    let Some(token) = bearer_token(request.headers()).filter(|token| looks_like_jwt(token)) else {
        return next.run(request).await;
    };
    let Ok(jwt_config) = ctx.config.get_jwt_config() else {
        return next.run(request).await;
    };
    // Malformed or expired tokens are left to the handler's own extractor.
    let Ok(token) = JWT::new(&jwt_config.secret).validate(token) else {
        return next.run(request).await;
    };
    let claims = token.claims;
    if claims.claims.contains_key(user_sessions::API_KEY_CLAIM) {
        // Minted per request by the API-key middleware, which checks the key.
        return next.run(request).await;
    }
    let Some(sid) = claims
        .claims
        .get(user_sessions::SID_CLAIM)
        .and_then(serde_json::Value::as_str)
    else {
        return session_expired();
    };
    match user_sessions::validate(&ctx.db, sid, &claims.pid).await {
        Ok(Some(session)) => {
            request.extensions_mut().insert(session);
            next.run(request).await
        }
        Ok(None) => session_expired(),
        Err(error) => {
            tracing::error!(error = error.to_string(), "session lookup failed");
            responses::error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "SESSION_LOOKUP_FAILED",
                "session lookup failed",
            )
        }
    }
}

fn session_expired() -> Response {
    responses::error(
        StatusCode::UNAUTHORIZED,
        "SESSION_EXPIRED",
        "登录已失效，请重新登录",
    )
}

#[cfg(test)]
mod tests {
    use super::is_open_path;

    #[test]
    fn only_token_free_auth_endpoints_skip_the_guard() {
        assert!(is_open_path("/api/auth/refresh"));
        assert!(is_open_path("/api/auth/verify/abc"));
        assert!(!is_open_path("/api/auth/current"));
        assert!(!is_open_path("/api/auth/loginx"));
        assert!(!is_open_path("/api/admin/users"));
    }
}
