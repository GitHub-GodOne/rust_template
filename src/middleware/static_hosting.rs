use std::path::Path;

use axum::{
    body::Body,
    extract::{Request, State},
    http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Router as AXRouter,
};
use loco_rs::{app::AppContext, controller::middleware::MiddlewareLayer, Result};

use crate::services::static_hosting::{
    access_granted, access_realm, is_reserved_request_path, list_public_directory,
    load_cached_sites, match_site, percent_decode_path, protecting_rule, render_directory_listing,
    resolve_public_target, PublicTarget, StaticSiteConfig,
};

pub struct StaticHostingMiddleware {
    ctx: AppContext,
}

impl StaticHostingMiddleware {
    #[must_use]
    pub fn new(ctx: &AppContext) -> Self {
        Self { ctx: ctx.clone() }
    }
}

impl MiddlewareLayer for StaticHostingMiddleware {
    fn name(&self) -> &'static str {
        "static_hosting"
    }

    fn config(&self) -> serde_json::Result<serde_json::Value> {
        Ok(serde_json::json!({ "enabled": true }))
    }

    fn apply(&self, app: AXRouter<AppContext>) -> Result<AXRouter<AppContext>> {
        Ok(app.layer(axum::middleware::from_fn_with_state(
            self.ctx.clone(),
            serve_static_site,
        )))
    }
}

async fn serve_static_site(
    State(ctx): State<AppContext>,
    request: Request,
    next: Next,
) -> Response {
    if !matches!(*request.method(), Method::GET | Method::HEAD) {
        return next.run(request).await;
    }

    let Ok(decoded_path) = percent_decode_path(request.uri().path()) else {
        return next.run(request).await;
    };
    if is_reserved_request_path(&decoded_path) {
        return next.run(request).await;
    }

    let sites = match load_cached_sites(&ctx).await {
        Ok(sites) => sites,
        Err(error) => {
            tracing::error!(error = ?error, "failed to load static hosting sites");
            return next.run(request).await;
        }
    };
    let Some((site, relative)) = match_site(&sites, &decoded_path) else {
        return next.run(request).await;
    };

    let accepts_html = request
        .headers()
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("text/html"));
    let resolved = resolve_public_target(site, &relative, accepts_html);
    // 先校验访问密码，再区分文件是否存在，避免未授权访问者探测受保护目录的内容。
    let resolved_path = match &resolved {
        Ok(Some(PublicTarget::File(path) | PublicTarget::Directory { path, .. })) => {
            Some(path.as_path())
        }
        _ => None,
    };
    let protected = match check_access(site, &relative, resolved_path, request.headers()).await {
        Ok(protected) => protected,
        Err(response) => return response,
    };

    let target = match resolved {
        Ok(Some(PublicTarget::File(target))) => target,
        Ok(Some(PublicTarget::Directory { path, relative })) => {
            return match list_public_directory(&path, &relative) {
                Ok(listing) => html_response(
                    request.method() == Method::HEAD,
                    render_directory_listing(site, &relative, &listing),
                    protected,
                ),
                Err(error) => {
                    tracing::warn!(
                        site = site.key,
                        path = decoded_path,
                        error = ?error,
                        "failed to list hosted static directory"
                    );
                    not_found_response()
                }
            };
        }
        Ok(None) => return not_found_response(),
        Err(error) => {
            tracing::warn!(
                site = site.key,
                path = decoded_path,
                error = ?error,
                "static hosting request rejected"
            );
            return not_found_response();
        }
    };

    let bytes = match tokio::fs::read(&target).await {
        Ok(bytes) => bytes,
        Err(error) => {
            tracing::warn!(
                site = site.key,
                path = target.display().to_string(),
                error = error.to_string(),
                "failed to read hosted static file"
            );
            return not_found_response();
        }
    };
    file_response(
        site,
        &target,
        bytes,
        request.method() == Method::HEAD,
        protected,
    )
}

fn file_response(
    site: &StaticSiteConfig,
    target: &Path,
    bytes: Vec<u8>,
    head_only: bool,
    protected: bool,
) -> Response {
    let mime = mime_guess::from_path(target).first_or_octet_stream();
    let mut headers = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(mime.as_ref()) {
        headers.insert(header::CONTENT_TYPE, value);
    }
    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    // 受保护内容只允许浏览器私有缓存，避免 CDN 等共享缓存把它发给未授权访问者。
    let visibility = if protected { "private" } else { "public" };
    let cache_control =
        if mime.type_() == mime_guess::mime::TEXT && mime.subtype() == mime_guess::mime::HTML {
            HeaderValue::from_static(if protected {
                "private, no-cache"
            } else {
                "no-cache"
            })
        } else {
            HeaderValue::from_str(&format!("{visibility}, max-age={}", site.cache_seconds))
                .unwrap_or_else(|_| HeaderValue::from_static("private, max-age=300"))
        };
    headers.insert(header::CACHE_CONTROL, cache_control);

    if head_only {
        return (StatusCode::OK, headers, Body::empty()).into_response();
    }
    (StatusCode::OK, headers, bytes).into_response()
}

/// `Ok(true)` when a password rule protects the request and the visitor passed it.
async fn check_access(
    site: &StaticSiteConfig,
    relative: &str,
    resolved: Option<&Path>,
    headers: &HeaderMap,
) -> Result<bool, Response> {
    let Some(rule) = protecting_rule(site, relative, resolved) else {
        return Ok(false);
    };
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    if access_granted(rule, authorization).await {
        Ok(true)
    } else {
        Err(unauthorized_response(&access_realm(site, rule)))
    }
}

fn html_response(head_only: bool, html: String, protected: bool) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(if protected {
            "private, no-cache"
        } else {
            "no-cache"
        }),
    );
    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    if head_only {
        return (StatusCode::OK, headers, Body::empty()).into_response();
    }
    (StatusCode::OK, headers, html).into_response()
}

fn unauthorized_response(realm: &str) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    let challenge = format!(r#"Basic realm="{realm}", charset="UTF-8""#);
    if let Ok(value) = HeaderValue::from_str(&challenge) {
        headers.insert(header::WWW_AUTHENTICATE, value);
    }
    (
        StatusCode::UNAUTHORIZED,
        headers,
        "此目录需要用户名和密码才能访问",
    )
        .into_response()
}

fn not_found_response() -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    (StatusCode::NOT_FOUND, headers, "Static file not found").into_response()
}
