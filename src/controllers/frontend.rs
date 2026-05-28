use axum::{
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
};
use include_dir::{include_dir, Dir};
use loco_rs::prelude::*;

static FRONTEND_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/frontend/dist");

pub fn routes() -> Routes {
    Routes::new()
        .add("/", get(index))
        .add("/{*path}", get(asset))
}

async fn index() -> Result<Response> {
    Ok(frontend_response("index.html", false))
}

async fn asset(Path(path): Path<String>) -> Result<Response> {
    let path = path.trim_start_matches('/');
    if is_backend_path(path) {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }

    Ok(if FRONTEND_DIR.get_file(path).is_some() {
        frontend_response(path, true)
    } else {
        frontend_response("index.html", false)
    })
}

fn frontend_response(path: &str, immutable: bool) -> Response {
    let Some(file) = FRONTEND_DIR.get_file(path) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(content_type(path)),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(if immutable {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        }),
    );

    (headers, file.contents().to_vec()).into_response()
}

fn is_backend_path(path: &str) -> bool {
    path == "api"
        || path.starts_with("api/")
        || path == "api-docs"
        || path.starts_with("api-docs/")
        || path == "swagger-ui"
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, extension)| extension) {
        Some("css") => "text/css; charset=utf-8",
        Some("html") => "text/html; charset=utf-8",
        Some("ico") => "image/x-icon",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("js") => "text/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("txt") => "text/plain; charset=utf-8",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}
