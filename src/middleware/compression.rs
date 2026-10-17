//! gzip/brotli/zstd for the frontend bundle and API responses.
//!
//! Loco's built-in `compression` middleware compresses everything, which
//! breaks resumable downloads and video seeking. This one only compresses
//! bodies whose size is known up front, and never media, archives, raw
//! binary files or partial (206) responses.

use axum::body::HttpBody as Body;
use axum::{
    http::{Extensions, HeaderMap, Response, StatusCode, Version},
    Router as AXRouter,
};
use loco_rs::{app::AppContext, controller::middleware::MiddlewareLayer, Result};
use tower_http::compression::{
    predicate::{NotForContentType, Predicate},
    CompressionLayer, DefaultPredicate,
};

pub struct CompressionMiddleware;

fn not_partial(status: StatusCode, _: Version, _: &HeaderMap, _: &Extensions) -> bool {
    status != StatusCode::PARTIAL_CONTENT
}

/// Only bodies that are complete in memory. The compressor loses the end of
/// a streamed body whose chunks arrive with pauses (database pages, file
/// reads), and browsers then reject the truncated download.
#[derive(Clone, Copy)]
pub struct KnownSize;

impl Predicate for KnownSize {
    fn should_compress<B>(&self, response: &Response<B>) -> bool
    where
        B: Body,
    {
        response.body().size_hint().exact().is_some()
    }
}

/// Whether a response may be compressed.
#[must_use]
pub fn predicate() -> impl Predicate {
    DefaultPredicate::new()
        .and(KnownSize)
        .and(NotForContentType::const_new("video/"))
        .and(NotForContentType::const_new("audio/"))
        .and(NotForContentType::const_new("application/octet-stream"))
        .and(NotForContentType::const_new("application/zip"))
        .and(NotForContentType::const_new("application/gzip"))
        .and(NotForContentType::const_new("application/x-"))
        .and(NotForContentType::const_new("application/pdf"))
        .and(not_partial as fn(StatusCode, Version, &HeaderMap, &Extensions) -> bool)
}

impl MiddlewareLayer for CompressionMiddleware {
    fn name(&self) -> &'static str {
        "app_compression"
    }

    fn config(&self) -> serde_json::Result<serde_json::Value> {
        Ok(serde_json::json!({ "enabled": true }))
    }

    fn apply(&self, app: AXRouter<AppContext>) -> Result<AXRouter<AppContext>> {
        Ok(app.layer(CompressionLayer::new().compress_when(predicate())))
    }
}

#[cfg(test)]
mod tests {
    use axum::body::{Body as AxumBody, Bytes};

    use super::*;

    fn response(body: AxumBody, content_type: &str) -> Response<AxumBody> {
        Response::builder()
            .header("content-type", content_type)
            .body(body)
            .unwrap()
    }

    #[test]
    fn only_complete_text_bodies_are_compressed() {
        let text = "x".repeat(200);
        assert!(predicate().should_compress(&response(AxumBody::from(text.clone()), "text/csv")));

        let stream =
            futures_util::stream::iter([Ok::<_, std::io::Error>(Bytes::from(text.clone()))]);
        assert!(
            !predicate().should_compress(&response(AxumBody::from_stream(stream), "text/csv")),
            "streamed bodies stay uncompressed"
        );
        assert!(!predicate().should_compress(&response(AxumBody::from(text.clone()), "video/mp4")));
        let mut partial = response(AxumBody::from(text), "text/plain");
        *partial.status_mut() = StatusCode::PARTIAL_CONTENT;
        assert!(!predicate().should_compress(&partial));
    }
}
