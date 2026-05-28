use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use axum::{
    extract::{Request, State},
    http::{header::HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
    Router as AXRouter,
};
use chrono::offset::Local;
use loco_rs::{app::AppContext, controller::middleware::MiddlewareLayer, Result};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::{
    models::_entities::{rate_limit_events, rate_limit_rules},
    responses,
};

static RATE_LIMIT_BUCKETS: OnceLock<Mutex<HashMap<String, Vec<Instant>>>> = OnceLock::new();

pub struct RateLimitMiddleware {
    ctx: AppContext,
}

impl RateLimitMiddleware {
    #[must_use]
    pub fn new(ctx: &AppContext) -> Self {
        Self { ctx: ctx.clone() }
    }
}

impl MiddlewareLayer for RateLimitMiddleware {
    fn name(&self) -> &'static str {
        "rate_limit"
    }

    fn config(&self) -> serde_json::Result<serde_json::Value> {
        Ok(serde_json::json!({ "enabled": true }))
    }

    fn apply(&self, app: AXRouter<AppContext>) -> Result<AXRouter<AppContext>> {
        Ok(app.layer(axum::middleware::from_fn_with_state(
            self.ctx.clone(),
            handle_rate_limit,
        )))
    }
}

async fn handle_rate_limit(
    State(ctx): State<AppContext>,
    request: Request,
    next: Next,
) -> Response {
    if !request.uri().path().starts_with("/api/") {
        return next.run(request).await;
    }

    let method = request.method().as_str().to_string();
    let path = request.uri().path().to_string();
    let ip = client_ip(request.headers());

    match limited_rule(&ctx, &ip, &method, &path).await {
        Ok(Some(rule_id)) => {
            record_rate_limit_event(&ctx, &ip, &method, &path, rule_id).await;
            responses::error(
                StatusCode::TOO_MANY_REQUESTS,
                "RATE_LIMITED",
                "too many requests",
            )
        }
        Ok(None) => next.run(request).await,
        Err(error) => {
            tracing::error!(error = error.to_string(), "rate limit middleware failed");
            next.run(request).await
        }
    }
}

async fn limited_rule(
    ctx: &AppContext,
    ip: &str,
    method: &str,
    path: &str,
) -> std::result::Result<Option<i32>, sea_orm::DbErr> {
    let rules = rate_limit_rules::Entity::find()
        .filter(rate_limit_rules::Column::Enabled.eq(true))
        .all(&ctx.db)
        .await?;

    for rule in rules {
        if !rule_matches(&rule, method, path) {
            continue;
        }
        let key = format!("{}:{}:{}", rule.id, method, ip);
        if increment_and_check(&key, rule.limit_count, rule.window_seconds) {
            return Ok(Some(rule.id));
        }
    }

    Ok(None)
}

fn increment_and_check(key: &str, limit_count: i32, window_seconds: i32) -> bool {
    let buckets = RATE_LIMIT_BUCKETS.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut buckets) = buckets.lock() else {
        return false;
    };
    let now = Instant::now();
    let window = Duration::from_secs(u64::try_from(window_seconds).unwrap_or(60));
    let entries = buckets.entry(key.to_string()).or_default();
    entries.retain(|instant| now.duration_since(*instant) <= window);
    if entries.len() >= usize::try_from(limit_count).unwrap_or(usize::MAX) {
        return true;
    }
    entries.push(now);
    false
}

fn rule_matches(rule: &rate_limit_rules::Model, method: &str, path: &str) -> bool {
    let method_matches = rule
        .method
        .as_deref()
        .is_none_or(|rule_method| rule_method.eq_ignore_ascii_case(method));
    if !method_matches {
        return false;
    }
    rule.path_pattern.strip_suffix('*').map_or_else(
        || rule.path_pattern == path,
        |prefix| path.starts_with(prefix),
    )
}

async fn record_rate_limit_event(
    ctx: &AppContext,
    ip: &str,
    method: &str,
    path: &str,
    rule_id: i32,
) {
    let result = rate_limit_events::ActiveModel {
        ip: Set(ip.to_string()),
        method: Set(method.to_string()),
        path: Set(path.to_string()),
        rule_id: Set(Some(rule_id)),
        user_id: Set(None),
        occurred_at: Set(Local::now().into()),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await;

    if let Err(error) = result {
        tracing::error!(
            error = error.to_string(),
            "failed to record rate limit event"
        );
    }
}

fn client_ip(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|value| value.to_str().ok())
        })
        .unwrap_or("unknown")
        .to_string()
}
