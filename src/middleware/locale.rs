//! Translates the `message` of JSON error responses for English clients.
//!
//! Sits outside the audit layer (logs keep the original text) and inside
//! compression (bodies are still plain JSON here).

use axum::{
    body::{to_bytes, Body},
    extract::Request,
    http::{header, HeaderValue},
    middleware::Next,
    response::Response,
    Router as AXRouter,
};
use loco_rs::{app::AppContext, controller::middleware::MiddlewareLayer, Result};

use crate::i18n;

/// Error bodies larger than this are passed through untouched.
const MAX_ERROR_BODY: usize = 64 * 1024;

pub struct LocaleMiddleware;

impl MiddlewareLayer for LocaleMiddleware {
    fn name(&self) -> &'static str {
        "locale"
    }

    fn config(&self) -> serde_json::Result<serde_json::Value> {
        Ok(serde_json::json!({ "enabled": true }))
    }

    fn apply(&self, app: AXRouter<AppContext>) -> Result<AXRouter<AppContext>> {
        Ok(app.layer(axum::middleware::from_fn(translate_errors)))
    }
}

fn is_json(response: &Response) -> bool {
    response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("json"))
}

fn small_enough(response: &Response) -> bool {
    response
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .is_none_or(|length| length <= MAX_ERROR_BODY)
}

async fn translate_errors(request: Request, next: Next) -> Response {
    let english = i18n::prefers_english(
        request
            .headers()
            .get(header::ACCEPT_LANGUAGE)
            .and_then(|value| value.to_str().ok()),
    );
    let response = next.run(request).await;
    if !english
        || response.status().as_u16() < 400
        || !is_json(&response)
        || !small_enough(&response)
    {
        return response;
    }
    let (mut parts, body) = response.into_parts();
    let Ok(bytes) = to_bytes(body, MAX_ERROR_BODY).await else {
        // Streaming bodies over the limit cannot be restored; report plainly.
        parts.headers.remove(header::CONTENT_LENGTH);
        return Response::from_parts(parts, Body::from("{\"success\":false,\"code\":\"INTERNAL_ERROR\",\"message\":\"request failed\",\"data\":null}"));
    };
    let translated = serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .and_then(|mut value| {
            let message = value.get("message")?.as_str()?.to_string();
            let english = i18n::translate(&message);
            if english == message {
                return None;
            }
            value["message"] = serde_json::Value::String(english);
            serde_json::to_vec(&value).ok()
        });
    match translated {
        Some(body) => {
            if let Ok(length) = HeaderValue::from_str(&body.len().to_string()) {
                parts.headers.insert(header::CONTENT_LENGTH, length);
            }
            Response::from_parts(parts, Body::from(body))
        }
        None => Response::from_parts(parts, Body::from(bytes)),
    }
}
