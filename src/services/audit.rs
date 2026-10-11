//! Request audit logging with per-feature switches.
//!
//! Every `/api` request is mapped to a feature (`FEATURES`) by its path. The
//! audit middleware records write requests (and, when switched on, reads)
//! into `operation_logs`; each feature can be turned off, include reads, or
//! drop request bodies via `audit_log_rules`.
//!
//! When adding API routes, put their path prefix under a feature here;
//! `tests/requests/audit.rs` fails for any `/api` route that maps nowhere.

#![allow(clippy::missing_errors_doc)]

use std::{
    cell::Cell,
    collections::HashMap,
    sync::{OnceLock, RwLock},
    time::{Duration, Instant},
};

use chrono::Local;
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set, TransactionTrait};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    errors::{ApiError, ApiResult},
    models::_entities::audit_log_rules,
};

/// Rule key of the global switch.
pub const GLOBAL_KEY: &str = "*";
/// Feature used for `/api` paths that no feature claims.
pub const OTHER_FEATURE: &str = "other";
const CACHE_TTL: Duration = Duration::from_secs(15);
const MASK: &str = "******";
const SUMMARY_LIMIT: usize = 2000;

#[derive(Debug, Clone, Copy)]
pub struct AuditFeature {
    pub key: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    /// Path prefixes; the longest matching prefix wins.
    pub prefixes: &'static [&'static str],
    /// Record GET requests by default (polled endpoints should stay off).
    pub default_log_reads: bool,
}

const fn feature(
    key: &'static str,
    label: &'static str,
    group: &'static str,
    prefixes: &'static [&'static str],
) -> AuditFeature {
    AuditFeature {
        key,
        label,
        group,
        prefixes,
        default_log_reads: false,
    }
}

pub const FEATURES: &[AuditFeature] = &[
    feature("auth", "登录认证", "账号", &["/api/auth"]),
    feature("user_preferences", "个人偏好", "账号", &["/api/user/"]),
    feature("setup", "安装向导", "账号", &["/api/setup"]),
    feature("users", "用户管理", "权限管理", &["/api/admin/users"]),
    feature("roles", "角色管理", "权限管理", &["/api/admin/roles"]),
    feature("menus", "菜单管理", "权限管理", &["/api/admin/menus"]),
    feature(
        "permissions",
        "权限管理",
        "权限管理",
        &["/api/admin/permissions"],
    ),
    feature(
        "departments",
        "部门管理",
        "权限管理",
        &["/api/admin/departments"],
    ),
    feature("tenants", "租户管理", "权限管理", &["/api/admin/tenants"]),
    feature(
        "data_scopes",
        "数据权限",
        "权限管理",
        &["/api/admin/data-scopes"],
    ),
    feature("dashboard", "首页", "系统管理", &["/api/admin/dashboard"]),
    feature("settings", "系统设置", "系统管理", &["/api/admin/settings"]),
    feature(
        "dicts",
        "字典管理",
        "系统管理",
        &["/api/admin/dict-types", "/api/admin/dict-items"],
    ),
    feature(
        "logs",
        "日志审计",
        "系统管理",
        &["/api/admin/logs", "/api/admin/audit-rules"],
    ),
    feature(
        "notifications",
        "通知中心",
        "系统管理",
        &["/api/admin/notifications"],
    ),
    feature(
        "email_templates",
        "邮件模板",
        "系统管理",
        &["/api/admin/email-templates"],
    ),
    feature(
        "scheduled_tasks",
        "定时任务",
        "系统管理",
        &[
            "/api/admin/scheduled-tasks",
            "/api/admin/scheduled-task-runs",
        ],
    ),
    feature("backups", "数据库备份", "系统管理", &["/api/admin/backups"]),
    feature(
        "monitoring",
        "系统监控",
        "系统管理",
        &["/api/admin/monitoring"],
    ),
    feature(
        "rate_limits",
        "限流规则",
        "系统管理",
        &["/api/admin/rate-limits", "/api/admin/rate-limit-events"],
    ),
    feature("ddns", "IPv6 动态解析", "系统管理", &["/api/admin/ddns"]),
    feature(
        "project_cli",
        "项目命令",
        "系统管理",
        &["/api/admin/project-cli"],
    ),
    feature(
        "uploads",
        "素材库",
        "文件与存储",
        &["/api/admin/uploads", "/api/uploads"],
    ),
    feature(
        "files",
        "文件管理",
        "文件与存储",
        &["/api/admin/files", "/api/file-api"],
    ),
    feature(
        "storage",
        "存储配置",
        "文件与存储",
        &["/api/admin/storage-profiles", "/api/admin/storage-buckets"],
    ),
    feature(
        "static_sites",
        "静态托管",
        "文件与存储",
        &["/api/admin/static-sites"],
    ),
    feature(
        "commands",
        "命令管理",
        "运维工具",
        &[
            "/api/admin/commands",
            "/api/admin/command-runs",
            "/api/admin/command-run-logs",
            "/api/admin/command-workflows",
            "/api/admin/command-workflow-runs",
            "/api/command-api",
        ],
    ),
    feature(
        "api_keys",
        "API 授权",
        "运维工具",
        &["/api/admin/api-keys", "/api/admin/command-api-keys"],
    ),
    feature("ssh", "SSH 管理", "运维工具", &["/api/admin/ssh"]),
    feature("vnc", "VNC 管理", "运维工具", &["/api/admin/vnc"]),
    feature("rdp", "RDP 管理", "运维工具", &["/api/admin/rdp"]),
    feature(
        "http_client",
        "HTTP 请求",
        "运维工具",
        &["/api/admin/http-client"],
    ),
    feature(
        "work_orders",
        "工单管理",
        "业务",
        &["/api/admin/work-orders"],
    ),
    feature(
        "payments",
        "支付管理",
        "业务",
        &[
            "/api/admin/payment-channels",
            "/api/admin/payment-orders",
            "/api/admin/payment-refunds",
            "/api/admin/payment-callbacks",
        ],
    ),
    feature(
        "content",
        "内容管理",
        "业务",
        &[
            "/api/admin/content-articles",
            "/api/admin/content-categories",
        ],
    ),
    feature(
        "chat_messages",
        "聊天消息",
        "业务",
        &["/api/admin/chat-messages"],
    ),
    feature(
        "ai_images",
        "AI 生图",
        "业务",
        &["/api/admin/ai-images", "/api/image-api"],
    ),
];

const OTHER: AuditFeature = feature(OTHER_FEATURE, "其他接口", "其他", &[]);

/// The feature a request path belongs to (`other` when none claims it).
#[must_use]
pub fn feature_for_path(path: &str) -> &'static AuditFeature {
    FEATURES
        .iter()
        .flat_map(|feature| {
            feature
                .prefixes
                .iter()
                .map(move |prefix| (feature, *prefix))
        })
        .filter(|(_, prefix)| {
            path == prefix.trim_end_matches('/')
                || path.starts_with(prefix)
                    && (prefix.ends_with('/') || path[prefix.len()..].starts_with('/'))
        })
        .max_by_key(|(_, prefix)| prefix.len())
        .map_or(&OTHER, |(feature, _)| feature)
}

#[must_use]
pub fn feature_by_key(key: &str) -> Option<&'static AuditFeature> {
    FEATURES
        .iter()
        .chain(std::iter::once(&OTHER))
        .find(|feature| feature.key == key)
}

// ---------------------------------------------------------------------------
// Rules
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct AuditRule {
    pub enabled: bool,
    pub log_reads: bool,
    pub log_body: bool,
}

impl AuditRule {
    #[must_use]
    pub const fn default_for(feature: &AuditFeature) -> Self {
        Self {
            enabled: true,
            log_reads: feature.default_log_reads,
            log_body: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuditRules {
    pub enabled: bool,
    overrides: HashMap<String, AuditRule>,
}

impl AuditRules {
    #[must_use]
    pub fn rule(&self, feature: &AuditFeature) -> AuditRule {
        self.overrides
            .get(feature.key)
            .copied()
            .unwrap_or_else(|| AuditRule::default_for(feature))
    }

    /// Whether a request with this method should be recorded.
    #[must_use]
    pub fn should_record(&self, feature: &AuditFeature, is_read: bool) -> bool {
        let rule = self.rule(feature);
        self.enabled && rule.enabled && (!is_read || rule.log_reads)
    }
}

struct CachedRules {
    loaded_at: Instant,
    rules: AuditRules,
}

fn cache() -> &'static RwLock<Option<CachedRules>> {
    static CACHE: OnceLock<RwLock<Option<CachedRules>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

/// Drops the cached rules so the next request reloads them.
pub fn invalidate_rules() {
    if let Ok(mut cached) = cache().write() {
        *cached = None;
    }
}

/// Current rules; read from the database at most every `CACHE_TTL`.
pub async fn load_rules(db: &DatabaseConnection) -> AuditRules {
    if let Ok(cached) = cache().read() {
        if let Some(cached) = cached.as_ref() {
            if cached.loaded_at.elapsed() < CACHE_TTL {
                return cached.rules.clone();
            }
        }
    }
    let rows = match audit_log_rules::Entity::find().all(db).await {
        Ok(rows) => rows,
        Err(error) => {
            tracing::warn!(error = %error, "failed to load audit rules; using defaults");
            Vec::new()
        }
    };
    let mut rules = AuditRules {
        enabled: true,
        overrides: HashMap::new(),
    };
    for row in rows {
        if row.feature_key == GLOBAL_KEY {
            rules.enabled = row.enabled;
        } else {
            rules.overrides.insert(
                row.feature_key,
                AuditRule {
                    enabled: row.enabled,
                    log_reads: row.log_reads,
                    log_body: row.log_body,
                },
            );
        }
    }
    if let Ok(mut cached) = cache().write() {
        *cached = Some(CachedRules {
            loaded_at: Instant::now(),
            rules: rules.clone(),
        });
    }
    rules
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct AuditFeatureRecord {
    pub key: String,
    pub label: String,
    pub group: String,
    pub prefixes: Vec<String>,
    #[serde(flatten)]
    pub rule: AuditRule,
    pub defaults: AuditRule,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct AuditSettings {
    pub enabled: bool,
    pub features: Vec<AuditFeatureRecord>,
}

#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveAuditFeature {
    pub key: String,
    #[serde(flatten)]
    pub rule: AuditRule,
}

#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveAuditSettings {
    pub enabled: bool,
    pub features: Vec<SaveAuditFeature>,
}

pub async fn settings(db: &DatabaseConnection) -> AuditSettings {
    invalidate_rules();
    let rules = load_rules(db).await;
    AuditSettings {
        enabled: rules.enabled,
        features: FEATURES
            .iter()
            .chain(std::iter::once(&OTHER))
            .map(|feature| AuditFeatureRecord {
                key: feature.key.to_string(),
                label: feature.label.to_string(),
                group: feature.group.to_string(),
                prefixes: feature.prefixes.iter().map(ToString::to_string).collect(),
                rule: rules.rule(feature),
                defaults: AuditRule::default_for(feature),
            })
            .collect(),
    }
}

async fn upsert(
    txn: &impl sea_orm::ConnectionTrait,
    key: &str,
    rule: AuditRule,
) -> Result<(), sea_orm::DbErr> {
    use sea_orm::{ColumnTrait, QueryFilter};
    let now = Local::now().fixed_offset();
    let existing = audit_log_rules::Entity::find()
        .filter(audit_log_rules::Column::FeatureKey.eq(key))
        .one(txn)
        .await?;
    if let Some(existing) = existing {
        let mut active: audit_log_rules::ActiveModel = existing.into();
        active.enabled = Set(rule.enabled);
        active.log_reads = Set(rule.log_reads);
        active.log_body = Set(rule.log_body);
        active.updated_at = Set(now);
        active.update(txn).await?;
    } else {
        audit_log_rules::ActiveModel {
            feature_key: Set(key.to_string()),
            enabled: Set(rule.enabled),
            log_reads: Set(rule.log_reads),
            log_body: Set(rule.log_body),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(txn)
        .await?;
    }
    Ok(())
}

pub async fn save_settings(
    db: &DatabaseConnection,
    params: &SaveAuditSettings,
) -> ApiResult<AuditSettings> {
    for item in &params.features {
        if feature_by_key(&item.key).is_none() {
            return Err(ApiError::bad_request(format!("未知功能 {}", item.key)));
        }
    }
    let txn = db.begin().await?;
    upsert(
        &txn,
        GLOBAL_KEY,
        AuditRule {
            enabled: params.enabled,
            log_reads: false,
            log_body: false,
        },
    )
    .await?;
    for item in &params.features {
        upsert(&txn, &item.key, item.rule).await?;
    }
    txn.commit().await?;
    Ok(settings(db).await)
}

// ---------------------------------------------------------------------------
// Request scope: lets handler-written logs reuse request details and stops
// the middleware from logging the same request twice.
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct AuditScope {
    pub feature: &'static AuditFeature,
    /// Feature switched on (handler logs are dropped otherwise).
    pub feature_enabled: bool,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub started: Instant,
    /// A handler already wrote a detailed log for this request.
    pub handled: Cell<bool>,
}

tokio::task_local! {
    pub static AUDIT_SCOPE: AuditScope;
}

/// Request details for a handler-written log, if inside a request.
#[must_use]
pub fn current_request() -> Option<(bool, Option<String>, Option<String>, i32)> {
    AUDIT_SCOPE
        .try_with(|scope| {
            scope.handled.set(true);
            (
                scope.feature_enabled,
                scope.ip.clone(),
                scope.user_agent.clone(),
                i32::try_from(scope.started.elapsed().as_millis()).unwrap_or(i32::MAX),
            )
        })
        .ok()
}

// ---------------------------------------------------------------------------
// Summaries
// ---------------------------------------------------------------------------

fn is_sensitive(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "password",
        "passwd",
        "token",
        "secret",
        "api_key",
        "apikey",
        "authorization",
        "private_key",
        "credential",
        "passphrase",
    ]
    .iter()
    .any(|word| key.contains(word))
}

/// Replaces values of password/token-like keys at any depth.
#[must_use]
pub fn mask_sensitive(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| {
                    let value = if is_sensitive(&key) && !value.is_null() {
                        Value::String(MASK.to_string())
                    } else {
                        mask_sensitive(value)
                    };
                    (key, value)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(mask_sensitive).collect()),
        other => other,
    }
}

/// Masked, length-limited JSON of a request body (`None` if not JSON).
#[must_use]
pub fn summarize_body(body: &[u8]) -> Option<String> {
    if body.is_empty() {
        return None;
    }
    let value: Value = serde_json::from_slice(body).ok()?;
    let text = serde_json::to_string(&mask_sensitive(value)).ok()?;
    Some(truncate(text, SUMMARY_LIMIT))
}

/// Error text from an API error response body.
#[must_use]
pub fn error_from_body(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    ["description", "message", "error"]
        .iter()
        .find_map(|key| value.get(key)?.as_str().map(ToString::to_string))
        .map(|text| truncate(text, SUMMARY_LIMIT))
}

fn truncate(mut text: String, limit: usize) -> String {
    if text.len() > limit {
        let mut end = limit;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push('…');
    }
    text
}

/// Chinese verb for an HTTP method.
#[must_use]
pub fn action_label(method: &str, path: &str) -> &'static str {
    match method {
        "GET" => "查询",
        "DELETE" => "删除",
        "PUT" | "PATCH" => "修改",
        "POST" if path.ends_with("/login") => "登录",
        "POST" if path.ends_with("/logout") => "退出",
        "POST" => "新增/执行",
        _ => "请求",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_map_to_the_longest_prefix() {
        assert_eq!(feature_for_path("/api/admin/users").key, "users");
        assert_eq!(feature_for_path("/api/admin/users/12").key, "users");
        assert_eq!(
            feature_for_path("/api/admin/command-runs/3/logs").key,
            "commands"
        );
        assert_eq!(
            feature_for_path("/api/user/preferences/dock").key,
            "user_preferences"
        );
        assert_eq!(feature_for_path("/api/admin/usersx").key, OTHER_FEATURE);
        assert_eq!(feature_for_path("/api/nothing").key, OTHER_FEATURE);
    }

    #[test]
    fn feature_keys_and_prefixes_are_unique() {
        let mut keys = std::collections::HashSet::new();
        let mut prefixes = std::collections::HashSet::new();
        for feature in FEATURES {
            assert!(keys.insert(feature.key), "duplicate key {}", feature.key);
            for prefix in feature.prefixes {
                assert!(prefix.starts_with("/api/"), "{prefix}");
                assert!(prefixes.insert(*prefix), "duplicate prefix {prefix}");
            }
        }
    }

    #[test]
    fn rules_fall_back_to_defaults_and_respect_the_global_switch() {
        let users = feature_by_key("users").unwrap();
        let mut rules = AuditRules {
            enabled: true,
            overrides: HashMap::new(),
        };
        assert!(rules.should_record(users, false));
        assert!(
            !rules.should_record(users, true),
            "reads are off by default"
        );
        rules.overrides.insert(
            "users".into(),
            AuditRule {
                enabled: true,
                log_reads: true,
                log_body: false,
            },
        );
        assert!(rules.should_record(users, true));
        rules.enabled = false;
        assert!(!rules.should_record(users, false));
    }

    #[test]
    fn bodies_are_masked_and_truncated() {
        let body = br#"{"email":"a@b.c","password":"p","nested":{"apiToken":"t","list":[{"secret_key":"s"}]},"empty_password":null}"#;
        let summary = summarize_body(body).unwrap();
        assert!(summary.contains(r#""email":"a@b.c""#));
        assert!(!summary.contains(r#""p""#) && !summary.contains(r#""t""#));
        assert_eq!(summary.matches(MASK).count(), 3);
        assert!(summary.contains(r#""empty_password":null"#));
        assert!(summarize_body(b"not json").is_none());
        let long = format!(r#"{{"text":"{}"}}"#, "汉".repeat(2000));
        assert!(summarize_body(long.as_bytes()).unwrap().ends_with('…'));
        assert_eq!(
            error_from_body(r#"{"error":"bad_request","description":"缺少参数"}"#.as_bytes())
                .as_deref(),
            Some("缺少参数")
        );
    }
}
