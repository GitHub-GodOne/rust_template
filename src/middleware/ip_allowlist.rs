//! `security.admin_ip_allowlist` and `security.admin_ip_blocklist` restrict
//! login and admin API access. A matching blocklist entry always wins.

use std::net::SocketAddr;

use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
    Router as AXRouter,
};
use loco_rs::{app::AppContext, controller::middleware::MiddlewareLayer, Result};

use crate::{
    responses,
    services::{client_info::trusted_client_ip, security_settings},
};

/// Paths outside the admin back office: public file links, the external
/// command API (authenticated by its own keys) and first-run setup.
const PUBLIC_PREFIXES: &[&str] = &["/api/command-api/", "/api/uploads/", "/api/setup"];

pub struct IpAllowlistMiddleware {
    ctx: AppContext,
}

impl IpAllowlistMiddleware {
    #[must_use]
    pub fn new(ctx: &AppContext) -> Self {
        Self { ctx: ctx.clone() }
    }
}

impl MiddlewareLayer for IpAllowlistMiddleware {
    fn name(&self) -> &'static str {
        "ip_allowlist"
    }

    fn config(&self) -> serde_json::Result<serde_json::Value> {
        Ok(serde_json::json!({ "enabled": true }))
    }

    fn apply(&self, app: AXRouter<AppContext>) -> Result<AXRouter<AppContext>> {
        Ok(app.layer(axum::middleware::from_fn_with_state(
            self.ctx.clone(),
            check_ip,
        )))
    }
}

fn is_protected(path: &str) -> bool {
    let back_office = path.starts_with("/api/")
        || path.starts_with("/api-docs")
        || path.starts_with("/swagger-ui");
    back_office
        && !PUBLIC_PREFIXES
            .iter()
            .any(|prefix| path.starts_with(prefix))
}

async fn check_ip(State(ctx): State<AppContext>, request: Request, next: Next) -> Response {
    if !is_protected(request.uri().path()) {
        return next.run(request).await;
    }
    let settings = match security_settings::load(&ctx.db).await {
        Ok(settings) => settings,
        Err(error) => {
            tracing::error!(
                error = error.to_string(),
                "failed to load security settings"
            );
            return next.run(request).await;
        }
    };
    if settings.admin_ip_allowlist.is_empty() && settings.admin_ip_blocklist.is_empty() {
        return next.run(request).await;
    }
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(address)| *address);
    let ip = trusted_client_ip(request.headers(), peer);
    if settings.ip_allowed(ip) {
        return next.run(request).await;
    }
    let reason = if settings.ip_blocked(ip) {
        "admin IP blocklist"
    } else {
        "admin IP allowlist"
    };
    tracing::warn!(ip = ?ip, path = request.uri().path(), reason, "blocked by admin IP policy");
    let message = if settings.ip_blocked(ip) {
        format!(
            "当前 IP {} 在后台访问黑名单中",
            ip.map_or_else(|| "未知".to_string(), |ip| ip.to_string())
        )
    } else {
        format!(
            "当前 IP {} 不在后台访问白名单中",
            ip.map_or_else(|| "未知".to_string(), |ip| ip.to_string())
        )
    };
    responses::error(StatusCode::FORBIDDEN, "IP_NOT_ALLOWED", &message)
}

#[cfg(test)]
mod tests {
    use super::is_protected;

    #[test]
    fn back_office_paths_are_protected_and_public_links_are_not() {
        assert!(is_protected("/api/auth/login"));
        assert!(is_protected("/api/admin/users"));
        assert!(is_protected("/swagger-ui"));
        assert!(!is_protected("/api/uploads/12"));
        assert!(!is_protected("/api/command-api/runs"));
        assert!(!is_protected("/admin/dashboard"));
        assert!(!is_protected("/public/files/abc"));
    }
}
