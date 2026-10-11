//! Records `/api` requests into `operation_logs` according to the per-feature
//! switches in `services::audit`.

use std::{
    cell::Cell,
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use axum::{
    body::{to_bytes, Body},
    extract::{Request, State},
    http::{header, HeaderMap, Method},
    middleware::Next,
    response::Response,
    Router as AXRouter,
};
use chrono::Local;
use loco_rs::{app::AppContext, auth::jwt::JWT, controller::middleware::MiddlewareLayer, Result};
use sea_orm::{ActiveModelTrait, Set};

use crate::{
    middleware::access_log::client_ip,
    models::_entities::{operation_logs, users},
    services::audit::{
        self, action_label, error_from_body, summarize_body, AuditScope, AUDIT_SCOPE,
    },
};

/// Request bodies larger than this are not captured.
const MAX_CAPTURED_BODY: usize = 64 * 1024;
/// Error responses larger than this are not inspected for a message.
const MAX_ERROR_BODY: usize = 16 * 1024;
// Keep compatibility with stable Rust versions before duration_constructors.
#[allow(clippy::duration_suboptimal_units)]
const OPERATOR_CACHE_TTL: Duration = Duration::from_secs(5 * 60);

pub struct AuditMiddleware {
    ctx: AppContext,
}

impl AuditMiddleware {
    #[must_use]
    pub fn new(ctx: &AppContext) -> Self {
        Self { ctx: ctx.clone() }
    }
}

impl MiddlewareLayer for AuditMiddleware {
    fn name(&self) -> &'static str {
        "audit"
    }

    fn config(&self) -> serde_json::Result<serde_json::Value> {
        Ok(serde_json::json!({ "enabled": true }))
    }

    fn apply(&self, app: AXRouter<AppContext>) -> Result<AXRouter<AppContext>> {
        Ok(app.layer(axum::middleware::from_fn_with_state(
            self.ctx.clone(),
            audit_request,
        )))
    }
}

fn header_text(headers: &HeaderMap, name: header::HeaderName) -> Option<String> {
    headers.get(name)?.to_str().ok().map(ToString::to_string)
}

fn content_length(headers: &HeaderMap) -> Option<usize> {
    header_text(headers, header::CONTENT_LENGTH)?.parse().ok()
}

fn is_json(headers: &HeaderMap) -> bool {
    header_text(headers, header::CONTENT_TYPE)
        .is_some_and(|value| value.to_ascii_lowercase().contains("json"))
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    header_text(headers, header::AUTHORIZATION)?
        .strip_prefix("Bearer ")
        .map(|token| token.trim().to_string())
}

/// Buffers a small JSON body so it can be summarized, then puts it back.
async fn capture_request_body(request: Request) -> (Request, Option<String>) {
    let small = content_length(request.headers()).is_some_and(|len| len <= MAX_CAPTURED_BODY);
    if !small || !is_json(request.headers()) {
        return (request, None);
    }
    let (parts, body) = request.into_parts();
    match to_bytes(body, MAX_CAPTURED_BODY).await {
        Ok(bytes) => {
            let summary = summarize_body(&bytes);
            (Request::from_parts(parts, Body::from(bytes)), summary)
        }
        Err(_) => (Request::from_parts(parts, Body::empty()), None),
    }
}

/// Reads the error message out of a small JSON error response.
async fn capture_error(response: Response) -> (Response, Option<String>) {
    let small = content_length(response.headers()).is_some_and(|len| len <= MAX_ERROR_BODY);
    if !small || !is_json(response.headers()) {
        return (response, None);
    }
    let (parts, body) = response.into_parts();
    match to_bytes(body, MAX_ERROR_BODY).await {
        Ok(bytes) => {
            let error = error_from_body(&bytes);
            (Response::from_parts(parts, Body::from(bytes)), error)
        }
        Err(_) => (Response::from_parts(parts, Body::empty()), None),
    }
}

type OperatorCache = Mutex<HashMap<String, (Instant, i32, String)>>;

fn operator_cache() -> &'static OperatorCache {
    static CACHE: OnceLock<OperatorCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// User id and display name behind a bearer token.
async fn resolve_operator(ctx: &AppContext, token: &str) -> Option<(i32, String)> {
    let secret = ctx.config.get_jwt_config().ok()?.secret.clone();
    let pid = JWT::new(&secret).validate(token).ok()?.claims.pid;
    if let Ok(cache) = operator_cache().lock() {
        if let Some((at, id, name)) = cache.get(&pid) {
            if at.elapsed() < OPERATOR_CACHE_TTL {
                return Some((*id, name.clone()));
            }
        }
    }
    let user = users::Model::find_by_pid(&ctx.db, &pid).await.ok()?;
    let name = if user.name.trim().is_empty() {
        user.email.clone()
    } else {
        format!("{} <{}>", user.name, user.email)
    };
    if let Ok(mut cache) = operator_cache().lock() {
        cache.insert(pid, (Instant::now(), user.id, name.clone()));
    }
    Some((user.id, name))
}

/// Everything known about a finished request, written in the background.
struct PendingLog {
    feature_key: &'static str,
    message: String,
    action: &'static str,
    level: &'static str,
    method: String,
    path: String,
    status: i32,
    duration_ms: i32,
    ip: Option<String>,
    user_agent: Option<String>,
    token: Option<String>,
    api_key: Option<String>,
    trace_id: Option<String>,
    request_summary: Option<String>,
    error_message: Option<String>,
}

async fn write_log(ctx: AppContext, log: PendingLog) {
    let operator = match &log.token {
        Some(token) => resolve_operator(&ctx, token).await,
        None => None,
    };
    let operator_name = operator.as_ref().map(|(_, name)| {
        log.api_key
            .as_ref()
            .map_or_else(|| name.clone(), |key| format!("{name}（API Key #{key}）"))
    });
    let now = Local::now().fixed_offset();
    let row = operation_logs::ActiveModel {
        trace_id: Set(log.trace_id),
        log_type: Set("request".to_string()),
        level: Set(log.level.to_string()),
        module: Set(log.feature_key.to_string()),
        action: Set(log.action.to_string()),
        message: Set(log.message),
        method: Set(Some(log.method)),
        path: Set(Some(log.path)),
        status: Set(Some(log.status)),
        duration_ms: Set(Some(log.duration_ms)),
        ip: Set(log.ip),
        user_agent: Set(log.user_agent),
        operator: Set(operator_name),
        user_id: Set(operator.map(|(id, _)| id)),
        request_summary: Set(log.request_summary),
        error_message: Set(log.error_message),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    if let Err(error) = row.insert(&ctx.db).await {
        tracing::warn!(error = %error, "failed to record audit log");
    }
}

async fn audit_request(State(ctx): State<AppContext>, request: Request, next: Next) -> Response {
    let path = request.uri().path().to_string();
    let method = request.method().clone();
    if !path.starts_with("/api/") || method == Method::OPTIONS || method == Method::HEAD {
        return next.run(request).await;
    }
    let feature = audit::feature_for_path(&path);
    let rules = audit::load_rules(&ctx.db).await;
    let rule = rules.rule(feature);
    let is_read = method == Method::GET;
    let record = rules.should_record(feature, is_read);

    let started = Instant::now();
    let ip = client_ip(&request);
    let user_agent = header_text(request.headers(), header::USER_AGENT);
    let token = bearer_token(request.headers());
    let api_key = header_text(
        request.headers(),
        header::HeaderName::from_static("x-auth-api-key-id"),
    );
    let full_path = request
        .uri()
        .path_and_query()
        .map_or_else(|| path.clone(), |value| value.as_str().to_string());
    let (request, request_summary) = if record && rule.log_body && !is_read {
        capture_request_body(request).await
    } else {
        (request, None)
    };

    let scope = AuditScope {
        feature,
        feature_enabled: rules.enabled && rule.enabled,
        ip: ip.clone(),
        user_agent: user_agent.clone(),
        started,
        handled: Cell::new(false),
    };
    let (response, handled) = AUDIT_SCOPE
        .scope(scope, async move {
            let response = next.run(request).await;
            let handled = AUDIT_SCOPE.with(|scope| scope.handled.get());
            (response, handled)
        })
        .await;
    // A handler that wrote its own, more specific log already covered it.
    if !record || handled {
        return response;
    }

    let status = response.status();
    let duration_ms = i32::try_from(started.elapsed().as_millis()).unwrap_or(i32::MAX);
    let (response, error_message) = if status.is_client_error() || status.is_server_error() {
        capture_error(response).await
    } else {
        (response, None)
    };
    let trace_id = header_text(
        response.headers(),
        header::HeaderName::from_static("x-request-id"),
    );
    let level = if status.is_server_error() {
        "error"
    } else if status.is_client_error() {
        "warn"
    } else {
        "info"
    };
    let action = action_label(method.as_str(), &path);

    tokio::spawn(write_log(
        ctx,
        PendingLog {
            feature_key: feature.key,
            message: format!("{} · {action} {path}", feature.label),
            action,
            level,
            method: method.to_string(),
            path: full_path,
            status: i32::from(status.as_u16()),
            duration_ms,
            ip,
            user_agent,
            token,
            api_key,
            trace_id,
            request_summary,
            error_message,
        },
    ));
    response
}
