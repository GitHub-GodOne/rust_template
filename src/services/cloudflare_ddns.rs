//! Cloudflare IPv6 dynamic DNS updater.
//!
//! Keeps Cloudflare AAAA records pointed at this host's current public IPv6
//! address. Configuration lives in system settings (group `ddns`) and is
//! re-read on every check, so changes apply without a restart.

#![allow(clippy::missing_errors_doc)]

use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
    net::Ipv6Addr,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use chrono::{DateTime, Local};
use sea_orm::DatabaseConnection;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::{
    errors::{ApiError, ApiResult},
    models::{admin_logs, system_settings},
    services::http_client::{build_http_client_with_overrides, HttpClientRuntimeOverrides},
};

pub const KEY_ENABLED: &str = "ddns.enabled";
pub const KEY_API_TOKEN: &str = "ddns.cloudflare_api_token";
pub const KEY_ZONE_ID: &str = "ddns.cloudflare_zone_id";
pub const KEY_RECORD_NAMES: &str = "ddns.record_names";
pub const KEY_PROXIED: &str = "ddns.proxied";
pub const KEY_TTL: &str = "ddns.ttl";
pub const KEY_INTERVAL: &str = "ddns.check_interval_seconds";
pub const KEY_SOURCE: &str = "ddns.ipv6_source";
pub const KEY_INTERFACE: &str = "ddns.interface_name";
pub const KEY_LOOKUP_URL: &str = "ddns.ipv6_lookup_url";
pub const KEY_API_BASE: &str = "ddns.cloudflare_api_base";
pub const KEY_MANUAL_IPV6: &str = "ddns.manual_ipv6";

pub const DEFAULT_API_BASE: &str = "https://api.cloudflare.com/client/v4";
pub const DEFAULT_LOOKUP_URL: &str = "https://api6.ipify.org";
const DEFAULT_INTERVAL_SECONDS: i64 = 300;
const MIN_INTERVAL_SECONDS: i64 = 30;
const MAX_INTERVAL_SECONDS: i64 = 86_400;
const STARTUP_DELAY: Duration = Duration::from_secs(15);
/// Re-check Cloudflare even when the address is unchanged, in case a record
/// was edited by hand on the Cloudflare dashboard.
const VERIFY_INTERVAL: Duration = Duration::from_hours(1);
const REQUEST_TIMEOUT_SECONDS: u64 = 30;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ipv6Source {
    Auto,
    Interface,
    Url,
    Manual,
}

impl Ipv6Source {
    fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "" | "auto" => Some(Self::Auto),
            "interface" => Some(Self::Interface),
            "url" => Some(Self::Url),
            "manual" => Some(Self::Manual),
            _ => None,
        }
    }

    const fn code(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Interface => "interface",
            Self::Url => "url",
            Self::Manual => "manual",
        }
    }
}

#[derive(Clone, Debug)]
pub struct DdnsSettings {
    pub enabled: bool,
    pub api_base: String,
    pub api_token: String,
    pub zone_id: Option<String>,
    pub record_names: Vec<String>,
    pub proxied: bool,
    pub ttl: u32,
    pub interval_seconds: u64,
    pub source: Ipv6Source,
    pub interface_name: Option<String>,
    pub lookup_url: String,
    pub manual_ipv6: Option<String>,
}

impl DdnsSettings {
    fn fingerprint(&self, address: Ipv6Addr) -> u64 {
        let mut hasher = DefaultHasher::new();
        address.hash(&mut hasher);
        self.api_base.hash(&mut hasher);
        self.api_token.hash(&mut hasher);
        self.zone_id.hash(&mut hasher);
        self.record_names.hash(&mut hasher);
        self.proxied.hash(&mut hasher);
        self.ttl.hash(&mut hasher);
        hasher.finish()
    }

    fn ensure_ready(&self) -> Result<(), String> {
        if self.api_token.is_empty() {
            return Err(format!("未配置 Cloudflare API Token（{KEY_API_TOKEN}）"));
        }
        if self.record_names.is_empty() {
            return Err(format!("未配置需要更新的域名（{KEY_RECORD_NAMES}）"));
        }
        if self.source == Ipv6Source::Manual && self.manual_ipv6.is_none() {
            return Err(format!(
                "获取方式为手动指定，但未填写 IPv6 地址（{KEY_MANUAL_IPV6}）"
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, utoipa::ToSchema)]
pub struct DdnsRecordStatus {
    pub name: String,
    /// `created`, `updated`, `unchanged` or `failed`.
    pub action: String,
    pub content: Option<String>,
    pub message: Option<String>,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Default, Deserialize, Serialize, utoipa::ToSchema)]
pub struct DdnsStatus {
    pub enabled: bool,
    pub configured: bool,
    pub token_configured: bool,
    pub source: String,
    pub interface_name: Option<String>,
    pub record_names: Vec<String>,
    pub zone_id: Option<String>,
    pub proxied: bool,
    pub ttl: u32,
    pub interval_seconds: u64,
    pub manual_ipv6: Option<String>,
    pub current_ipv6: Option<String>,
    pub last_synced_ipv6: Option<String>,
    pub last_checked_at: Option<String>,
    pub last_synced_at: Option<String>,
    pub last_error: Option<String>,
    /// What started the last sync: `schedule`, `manual` or `manual_address`.
    pub last_sync_trigger: Option<String>,
    pub records: Vec<DdnsRecordStatus>,
}

/// What started a sync run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SyncTrigger {
    /// Periodic background check; skips Cloudflare when nothing changed.
    Schedule,
    /// "Sync now" button using the configured address source.
    Manual,
    /// One-off sync of an address typed in by the administrator.
    Address(Ipv6Addr),
}

impl SyncTrigger {
    const fn code(self) -> &'static str {
        match self {
            Self::Schedule => "schedule",
            Self::Manual => "manual",
            Self::Address(_) => "manual_address",
        }
    }
}

#[derive(Default)]
struct SyncMemory {
    fingerprint: Option<u64>,
    verified_at: Option<Instant>,
}

fn status_cell() -> &'static Mutex<DdnsStatus> {
    static STATUS: OnceLock<Mutex<DdnsStatus>> = OnceLock::new();
    STATUS.get_or_init(|| Mutex::new(DdnsStatus::default()))
}

fn sync_lock() -> &'static tokio::sync::Mutex<SyncMemory> {
    static LOCK: OnceLock<tokio::sync::Mutex<SyncMemory>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(SyncMemory::default()))
}

fn snapshot() -> DdnsStatus {
    status_cell()
        .lock()
        .map(|status| status.clone())
        .unwrap_or_default()
}

fn update_status(change: impl FnOnce(&mut DdnsStatus)) -> DdnsStatus {
    status_cell().lock().map_or_else(
        |_| DdnsStatus::default(),
        |mut status| {
            change(&mut status);
            status.clone()
        },
    )
}

fn apply_settings(status: &mut DdnsStatus, settings: &DdnsSettings) {
    status.enabled = settings.enabled;
    status.configured = settings.ensure_ready().is_ok();
    status.token_configured = !settings.api_token.is_empty();
    status.source = settings.source.code().to_string();
    status.interface_name.clone_from(&settings.interface_name);
    status.record_names.clone_from(&settings.record_names);
    status.zone_id.clone_from(&settings.zone_id);
    status.proxied = settings.proxied;
    status.ttl = settings.ttl;
    status.interval_seconds = settings.interval_seconds;
    status.manual_ipv6.clone_from(&settings.manual_ipv6);
}

/// Current status merged with the latest settings.
pub async fn status(db: &DatabaseConnection) -> ApiResult<DdnsStatus> {
    let settings = load_settings(db).await?;
    Ok(update_status(|status| apply_settings(status, &settings)))
}

pub async fn load_settings(db: &DatabaseConnection) -> ApiResult<DdnsSettings> {
    let text =
        |key: &'static str, default: &'static str| system_settings::string_value(db, key, default);
    let enabled = text(KEY_ENABLED, "false").await? == "true";
    let proxied = text(KEY_PROXIED, "false").await? == "true";
    let source = Ipv6Source::parse(&text(KEY_SOURCE, "auto").await?).unwrap_or(Ipv6Source::Auto);
    let ttl = system_settings::number_i64(db, KEY_TTL, 1).await?;
    let interval = system_settings::number_i64(db, KEY_INTERVAL, DEFAULT_INTERVAL_SECONDS).await?;

    Ok(DdnsSettings {
        enabled,
        api_base: text(KEY_API_BASE, DEFAULT_API_BASE)
            .await?
            .trim()
            .trim_end_matches('/')
            .to_string(),
        api_token: text(KEY_API_TOKEN, "").await?.trim().to_string(),
        zone_id: non_empty(&text(KEY_ZONE_ID, "").await?),
        record_names: parse_record_names(&text(KEY_RECORD_NAMES, "").await?),
        proxied,
        ttl: normalize_ttl(ttl),
        interval_seconds: u64::try_from(interval.clamp(MIN_INTERVAL_SECONDS, MAX_INTERVAL_SECONDS))
            .unwrap_or(300),
        source,
        interface_name: non_empty(&text(KEY_INTERFACE, "").await?),
        lookup_url: text(KEY_LOOKUP_URL, DEFAULT_LOOKUP_URL)
            .await?
            .trim()
            .to_string(),
        manual_ipv6: non_empty(&text(KEY_MANUAL_IPV6, "").await?),
    })
}

/// Validates a DDNS setting value before it is saved.
pub fn validate_setting(key: &str, value: &str) -> ApiResult<()> {
    let value = value.trim();
    match key {
        KEY_SOURCE if Ipv6Source::parse(value).is_none() => Err(ApiError::bad_request(
            "ddns.ipv6_source 只能是 auto、interface、url 或 manual",
        )),
        KEY_INTERVAL => match value.parse::<i64>() {
            Ok(seconds) if (MIN_INTERVAL_SECONDS..=MAX_INTERVAL_SECONDS).contains(&seconds) => {
                Ok(())
            }
            _ => Err(ApiError::bad_request(
                "ddns.check_interval_seconds 必须是 30 到 86400 之间的整数",
            )),
        },
        KEY_TTL => match value.parse::<i64>() {
            Ok(ttl) if normalize_ttl(ttl) == u32::try_from(ttl).unwrap_or(0) => Ok(()),
            _ => Err(ApiError::bad_request(
                "ddns.ttl 只能是 1（自动）或 60 到 86400 之间的整数",
            )),
        },
        KEY_RECORD_NAMES => parse_record_names(value)
            .iter()
            .all(|name| is_valid_hostname(name))
            .then_some(())
            .ok_or_else(|| ApiError::bad_request("ddns.record_names 包含无效的域名")),
        KEY_MANUAL_IPV6 if !value.is_empty() => parse_manual_ipv6(value)
            .map(|_| ())
            .map_err(ApiError::bad_request),
        KEY_LOOKUP_URL | KEY_API_BASE if !value.is_empty() => url::Url::parse(value)
            .ok()
            .filter(|url| matches!(url.scheme(), "http" | "https"))
            .map(|_| ())
            .ok_or_else(|| ApiError::bad_request(format!("{key} 必须是 http(s) 地址"))),
        _ => Ok(()),
    }
}

struct SettingDef {
    key: &'static str,
    name: &'static str,
    value_type: &'static str,
    default_value: &'static str,
    description: &'static str,
    sort_order: i32,
}

/// Metadata used when a DDNS setting row is missing and has to be created.
/// Kept in sync with `src/fixtures/system_settings.yaml`.
const SETTING_DEFS: &[SettingDef] = &[
    SettingDef { key: KEY_ENABLED, name: "启用 IPv6 动态解析", value_type: "boolean", default_value: "false", description: "开启后定时检测本机公网 IPv6，并自动更新到 Cloudflare 的 AAAA 记录", sort_order: 210 },
    SettingDef { key: KEY_API_TOKEN, name: "Cloudflare API Token", value_type: "secret", default_value: "", description: "Cloudflare API 令牌，需要 Zone.DNS 编辑权限（建议只授权目标域名）", sort_order: 215 },
    SettingDef { key: KEY_ZONE_ID, name: "Cloudflare Zone ID", value_type: "string", default_value: "", description: "域名所在 Zone 的 ID；留空时根据域名自动查找", sort_order: 220 },
    SettingDef { key: KEY_RECORD_NAMES, name: "解析域名", value_type: "string", default_value: "", description: "需要更新的完整域名，多个用逗号分隔，例如 home.example.com,nas.example.com", sort_order: 225 },
    SettingDef { key: KEY_PROXIED, name: "Cloudflare 代理", value_type: "boolean", default_value: "false", description: "是否开启 Cloudflare 代理（橙色云朵）；直连 IPv6 时请保持 false", sort_order: 230 },
    SettingDef { key: KEY_TTL, name: "解析 TTL", value_type: "number", default_value: "1", description: "记录 TTL 秒数，1 表示自动，也可填 60 到 86400", sort_order: 235 },
    SettingDef { key: KEY_INTERVAL, name: "检测间隔（秒）", value_type: "number", default_value: "300", description: "检测本机 IPv6 是否变化的间隔，30 到 86400 秒", sort_order: 240 },
    SettingDef { key: KEY_SOURCE, name: "IPv6 获取方式", value_type: "string", default_value: "auto", description: "auto：优先读取网卡，失败再查外部地址；interface：只读网卡；url：只通过外部地址查询；manual：使用手动指定的地址", sort_order: 245 },
    SettingDef { key: KEY_INTERFACE, name: "指定网卡", value_type: "string", default_value: "", description: "只使用该网卡上的 IPv6（如 eth0、en0、以太网），留空自动选择稳定的公网地址", sort_order: 250 },
    SettingDef { key: KEY_LOOKUP_URL, name: "IPv6 查询地址", value_type: "string", default_value: DEFAULT_LOOKUP_URL, description: "返回纯文本 IPv6 的查询地址，url/auto 模式使用", sort_order: 255 },
    SettingDef { key: KEY_API_BASE, name: "Cloudflare API 地址", value_type: "string", default_value: DEFAULT_API_BASE, description: "Cloudflare API 根地址，一般无需修改", sort_order: 260 },
    SettingDef { key: KEY_MANUAL_IPV6, name: "手动指定 IPv6", value_type: "string", default_value: "", description: "获取方式为 manual 时同步到域名的固定 IPv6 地址", sort_order: 265 },
];

/// Editable DDNS configuration as shown on the admin page. The API token is
/// never returned, only whether one is stored.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct DdnsConfig {
    pub enabled: bool,
    pub token_configured: bool,
    pub zone_id: String,
    pub record_names: String,
    pub proxied: bool,
    pub ttl: i64,
    pub check_interval_seconds: i64,
    pub ipv6_source: String,
    pub interface_name: String,
    pub manual_ipv6: String,
    pub ipv6_lookup_url: String,
    pub api_base: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveDdnsConfigParams {
    pub enabled: bool,
    /// Leave empty to keep the stored token.
    pub api_token: Option<String>,
    pub zone_id: Option<String>,
    pub record_names: String,
    pub proxied: bool,
    pub ttl: i64,
    pub check_interval_seconds: i64,
    pub ipv6_source: String,
    pub interface_name: Option<String>,
    pub manual_ipv6: Option<String>,
    pub ipv6_lookup_url: Option<String>,
    pub api_base: Option<String>,
}

pub async fn load_config(db: &DatabaseConnection) -> ApiResult<DdnsConfig> {
    let settings = load_settings(db).await?;
    Ok(DdnsConfig {
        enabled: settings.enabled,
        token_configured: !settings.api_token.is_empty(),
        zone_id: settings.zone_id.unwrap_or_default(),
        record_names: settings.record_names.join(", "),
        proxied: settings.proxied,
        ttl: i64::from(settings.ttl),
        check_interval_seconds: i64::try_from(settings.interval_seconds)
            .unwrap_or(DEFAULT_INTERVAL_SECONDS),
        ipv6_source: settings.source.code().to_string(),
        interface_name: settings.interface_name.unwrap_or_default(),
        manual_ipv6: settings.manual_ipv6.unwrap_or_default(),
        ipv6_lookup_url: settings.lookup_url,
        api_base: settings.api_base,
    })
}

pub async fn save_config(
    db: &DatabaseConnection,
    user_id: i32,
    params: SaveDdnsConfigParams,
) -> ApiResult<DdnsConfig> {
    let text = |value: Option<String>| {
        value
            .map(|value| value.trim().to_string())
            .unwrap_or_default()
    };
    let record_names = parse_record_names(&params.record_names).join(",");
    let source = params.ipv6_source.trim().to_string();
    let manual = text(params.manual_ipv6);
    let manual = if manual.is_empty() {
        manual
    } else {
        parse_manual_ipv6(&manual)
            .map_err(ApiError::bad_request)?
            .to_string()
    };
    let lookup_url = text(params.ipv6_lookup_url);
    let api_base = text(params.api_base);
    let token = text(params.api_token);

    let mut values: Vec<(&str, String)> = vec![
        (KEY_ENABLED, params.enabled.to_string()),
        (KEY_ZONE_ID, text(params.zone_id)),
        (KEY_RECORD_NAMES, record_names.clone()),
        (KEY_PROXIED, params.proxied.to_string()),
        (KEY_TTL, params.ttl.to_string()),
        (KEY_INTERVAL, params.check_interval_seconds.to_string()),
        (KEY_SOURCE, source.clone()),
        (KEY_INTERFACE, text(params.interface_name)),
        (KEY_MANUAL_IPV6, manual.clone()),
        (
            KEY_LOOKUP_URL,
            if lookup_url.is_empty() {
                DEFAULT_LOOKUP_URL.to_string()
            } else {
                lookup_url
            },
        ),
        (
            KEY_API_BASE,
            if api_base.is_empty() {
                DEFAULT_API_BASE.to_string()
            } else {
                api_base.trim_end_matches('/').to_string()
            },
        ),
    ];
    if !token.is_empty() {
        values.push((KEY_API_TOKEN, token.clone()));
    }
    for (key, value) in &values {
        validate_setting(key, value)?;
    }
    if source == Ipv6Source::Manual.code() && manual.is_empty() {
        return Err(ApiError::bad_request(
            "获取方式为手动指定时，请填写 IPv6 地址",
        ));
    }
    if params.enabled {
        let has_token = !token.is_empty() || !load_settings(db).await?.api_token.is_empty();
        if !has_token || record_names.is_empty() {
            return Err(ApiError::bad_request(
                "启用前请填写 Cloudflare API Token 和至少一个解析域名",
            ));
        }
    }

    for (key, value) in values {
        upsert_setting(db, user_id, key, value).await?;
    }
    load_config(db).await
}

async fn upsert_setting(
    db: &DatabaseConnection,
    user_id: i32,
    key: &str,
    value: String,
) -> ApiResult<()> {
    use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter, Set};

    use crate::models::_entities::system_settings as settings_entity;

    let existing = settings_entity::Entity::find()
        .filter(settings_entity::Column::Key.eq(key))
        .one(db)
        .await?;
    if let Some(existing) = existing {
        if existing.value == value {
            return Ok(());
        }
        let mut active = existing.into_active_model();
        active.value = Set(value);
        active.updated_by = Set(Some(user_id));
        active.update(db).await?;
        return Ok(());
    }
    let def = SETTING_DEFS
        .iter()
        .find(|def| def.key == key)
        .ok_or_else(|| ApiError::internal(format!("unknown ddns setting {key}")))?;
    settings_entity::ActiveModel {
        key: Set(def.key.to_string()),
        name: Set(def.name.to_string()),
        group_key: Set("ddns".to_string()),
        value: Set(value),
        value_type: Set(def.value_type.to_string()),
        default_value: Set(Some(def.default_value.to_string())),
        description: Set(Some(def.description.to_string())),
        is_public: Set(false),
        is_builtin: Set(true),
        is_encrypted: Set(def.key == KEY_API_TOKEN),
        sort_order: Set(def.sort_order),
        created_by: Set(Some(user_id)),
        updated_by: Set(Some(user_id)),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(())
}

/// Runs the periodic updater for the lifetime of the process.
pub fn start_updater(db: DatabaseConnection) {
    tokio::spawn(async move {
        tokio::time::sleep(STARTUP_DELAY).await;
        loop {
            let interval = match load_settings(&db).await {
                Ok(settings) => {
                    let interval = settings.interval_seconds;
                    if settings.enabled {
                        sync_with_settings(&db, settings, SyncTrigger::Schedule).await;
                    } else {
                        update_status(|status| apply_settings(status, &settings));
                    }
                    interval
                }
                Err(error) => {
                    tracing::warn!(error = error.message(), "failed to load ddns settings");
                    u64::try_from(DEFAULT_INTERVAL_SECONDS).unwrap_or(300)
                }
            };
            tokio::time::sleep(Duration::from_secs(interval)).await;
        }
    });
}

/// Manual sync from the admin UI; always talks to Cloudflare.
///
/// With `address` the typed-in IPv6 is published instead of the configured
/// source; unless the source is `manual`, the next scheduled check may
/// replace it again.
pub async fn sync_now(db: &DatabaseConnection, address: Option<&str>) -> ApiResult<DdnsStatus> {
    let settings = load_settings(db).await?;
    if !settings.enabled {
        return Err(ApiError::bad_request(
            "IPv6 动态解析未启用，请先在配置中打开「启用」并保存",
        ));
    }
    if settings.api_token.is_empty() || settings.record_names.is_empty() {
        return Err(ApiError::bad_request(
            "请先配置 Cloudflare API Token 和需要更新的域名",
        ));
    }
    let trigger = if let Some(value) = address.map(str::trim).filter(|value| !value.is_empty()) {
        SyncTrigger::Address(parse_manual_ipv6(value).map_err(ApiError::bad_request)?)
    } else {
        settings.ensure_ready().map_err(ApiError::bad_request)?;
        SyncTrigger::Manual
    };
    Ok(sync_with_settings(db, settings, trigger).await)
}

// The lock is held for the whole run on purpose: background and manual syncs
// must not talk to Cloudflare at the same time.
#[allow(clippy::significant_drop_tightening)]
async fn sync_with_settings(
    db: &DatabaseConnection,
    settings: DdnsSettings,
    trigger: SyncTrigger,
) -> DdnsStatus {
    let mut memory = sync_lock().lock().await;
    let checked_at = now_text();
    update_status(|status| {
        apply_settings(status, &settings);
        status.last_checked_at = Some(checked_at.clone());
    });

    let address = if let SyncTrigger::Address(address) = trigger {
        address
    } else {
        if let Err(message) = settings.ensure_ready() {
            return record_failure(db, message).await;
        }
        match detect_ipv6(db, &settings).await {
            Ok(address) => address,
            Err(message) => return record_failure(db, message).await,
        }
    };
    if trigger != SyncTrigger::Address(address) {
        update_status(|status| status.current_ipv6 = Some(address.to_string()));
    }

    let fingerprint = settings.fingerprint(address);
    let fresh = memory.fingerprint == Some(fingerprint)
        && memory
            .verified_at
            .is_some_and(|time| time.elapsed() < VERIFY_INTERVAL);
    if fresh && trigger == SyncTrigger::Schedule {
        return snapshot();
    }

    let results = match CloudflareClient::new(db, &settings).await {
        Ok(client) => client.sync_records(&settings, address).await,
        Err(message) => return record_failure(db, message).await,
    };
    let failures: Vec<String> = results
        .iter()
        .filter(|record| record.action == "failed")
        .map(|record| {
            format!(
                "{}：{}",
                record.name,
                record.message.as_deref().unwrap_or("未知错误")
            )
        })
        .collect();
    let changed: Vec<&str> = results
        .iter()
        .filter(|record| matches!(record.action.as_str(), "created" | "updated"))
        .map(|record| record.name.as_str())
        .collect();

    if !changed.is_empty() {
        let message = format!(
            "IPv6 动态解析已更新 {} → {address}{}",
            changed.join(", "),
            if matches!(trigger, SyncTrigger::Address(_)) {
                "（手动指定）"
            } else {
                ""
            }
        );
        admin_logs::record(
            db,
            admin_logs::LogInput {
                log_type: "operation",
                level: "info",
                module: "ddns",
                action: "ipv6_record_updated",
                message: &message,
                user_id: None,
                operator: Some("system".to_string()),
                method: None,
                path: None,
                status: None,
                error_message: None,
            },
        )
        .await;
    }

    if failures.is_empty() {
        memory.fingerprint = Some(fingerprint);
        memory.verified_at = Some(Instant::now());
        let synced_at = now_text();
        update_status(|status| {
            status.records = results;
            status.last_error = None;
            status.last_synced_ipv6 = Some(address.to_string());
            status.last_synced_at = Some(synced_at);
            status.last_sync_trigger = Some(trigger.code().to_string());
        })
    } else {
        memory.fingerprint = None;
        update_status(|status| status.records = results);
        record_failure(db, failures.join("；")).await
    }
}

/// Stores the error and writes an operation log only when the error changes,
/// so a persistent misconfiguration does not flood the log every interval.
async fn record_failure(db: &DatabaseConnection, message: String) -> DdnsStatus {
    let previous = snapshot().last_error;
    let status = update_status(|status| status.last_error = Some(message.clone()));
    if previous.as_deref() != Some(message.as_str()) {
        tracing::warn!(error = %message, "ddns sync failed");
        admin_logs::record(
            db,
            admin_logs::LogInput {
                log_type: "operation",
                level: "error",
                module: "ddns",
                action: "ipv6_sync_failed",
                message: "IPv6 动态解析同步失败",
                user_id: None,
                operator: Some("system".to_string()),
                method: None,
                path: None,
                status: None,
                error_message: Some(message),
            },
        )
        .await;
    }
    status
}

// ---------------------------------------------------------------------------
// IPv6 detection
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddressCandidate {
    pub interface: String,
    pub address: Ipv6Addr,
    /// Privacy-extension or otherwise unstable address (`None` = unknown).
    pub temporary: Option<bool>,
}

async fn detect_ipv6(db: &DatabaseConnection, settings: &DdnsSettings) -> Result<Ipv6Addr, String> {
    let from_interface = || async {
        let interface = settings.interface_name.clone();
        let candidates = tokio::task::spawn_blocking(list_interface_addresses)
            .await
            .map_err(|error| format!("读取网卡地址失败：{error}"))?;
        pick_address(&candidates, interface.as_deref()).ok_or_else(|| {
            interface.map_or_else(
                || "本机网卡上没有可用的公网 IPv6 地址".to_string(),
                |name| format!("网卡 {name} 上没有可用的公网 IPv6 地址"),
            )
        })
    };

    match settings.source {
        Ipv6Source::Interface => from_interface().await,
        Ipv6Source::Url => lookup_ipv6(db, &settings.lookup_url).await,
        Ipv6Source::Manual => settings.manual_ipv6.as_deref().map_or_else(
            || Err("未填写手动指定的 IPv6 地址".to_string()),
            parse_manual_ipv6,
        ),
        Ipv6Source::Auto => match from_interface().await {
            Ok(address) => Ok(address),
            Err(interface_error) => lookup_ipv6(db, &settings.lookup_url)
                .await
                .map_err(|url_error| format!("{interface_error}；{url_error}")),
        },
    }
}

async fn lookup_ipv6(db: &DatabaseConnection, url: &str) -> Result<Ipv6Addr, String> {
    let client = build_http_client_with_overrides(
        db,
        HttpClientRuntimeOverrides {
            request_timeout_seconds: Some(REQUEST_TIMEOUT_SECONDS),
            ..Default::default()
        },
    )
    .await
    .map_err(|error| format!("创建 HTTP 客户端失败：{}", error.message()))?;
    let body = client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|error| format!("通过 {url} 查询 IPv6 失败：{error}"))?
        .text()
        .await
        .map_err(|error| format!("读取 {url} 响应失败：{error}"))?;
    parse_public_ipv6(&body).ok_or_else(|| format!("{url} 返回的不是公网 IPv6 地址"))
}

/// Parses a plain-text lookup response into a public IPv6 address.
#[must_use]
pub fn parse_public_ipv6(body: &str) -> Option<Ipv6Addr> {
    body.trim()
        .parse::<Ipv6Addr>()
        .ok()
        .filter(|address| is_global_unicast(*address))
}

/// Validates an administrator-supplied address. Unlike auto-detection this
/// also accepts unique-local (`fc00::/7`) addresses for internal DNS.
pub fn parse_manual_ipv6(value: &str) -> Result<Ipv6Addr, String> {
    let address = value
        .trim()
        .parse::<Ipv6Addr>()
        .map_err(|_| format!("{} 不是有效的 IPv6 地址", value.trim()))?;
    let first = address.segments()[0];
    let invalid = address.is_unspecified()
        || address.is_loopback()
        || address.is_multicast()
        || (first & 0xffc0) == 0xfe80
        || address.to_ipv4_mapped().is_some();
    if invalid {
        return Err(format!(
            "{address} 不能用于域名解析（不能是回环、链路本地、组播或 IPv4 映射地址）"
        ));
    }
    Ok(address)
}

/// Global unicast (`2000::/3`), excluding the documentation prefix `2001:db8::/32`.
#[must_use]
pub const fn is_global_unicast(address: Ipv6Addr) -> bool {
    let segments = address.segments();
    (segments[0] & 0xe000) == 0x2000 && !(segments[0] == 0x2001 && segments[1] == 0x0db8)
}

/// Picks the address to publish: stable addresses win over temporary ones,
/// then EUI-64 addresses, then a deterministic order so the choice does not
/// flap between checks.
#[must_use]
pub fn pick_address(candidates: &[AddressCandidate], interface: Option<&str>) -> Option<Ipv6Addr> {
    let mut usable: Vec<&AddressCandidate> = candidates
        .iter()
        .filter(|candidate| is_global_unicast(candidate.address))
        .filter(|candidate| interface.is_none_or(|name| candidate.interface == name))
        .collect();
    usable.sort_by_key(|candidate| {
        (
            candidate.temporary == Some(true),
            !is_eui64(candidate.address),
            candidate.interface.clone(),
            candidate.address,
        )
    });
    usable.first().map(|candidate| candidate.address)
}

const fn is_eui64(address: Ipv6Addr) -> bool {
    let octets = address.octets();
    octets[11] == 0xff && octets[12] == 0xfe
}

/// Lists this host's IPv6 addresses with the best per-OS source:
/// Linux `/proc/net/if_inet6`, BSD/macOS `ifconfig`, Windows PowerShell
/// `Get-NetIPAddress`; anything else falls back to the generic interface list
/// (which cannot tell temporary addresses apart).
fn list_interface_addresses() -> Vec<AddressCandidate> {
    let native = native_interface_addresses();
    if !native.is_empty() {
        return native;
    }
    let networks = sysinfo::Networks::new_with_refreshed_list();
    networks
        .iter()
        .flat_map(|(name, data)| {
            data.ip_networks()
                .iter()
                .filter_map(move |network| match network.addr {
                    std::net::IpAddr::V6(address) => Some(AddressCandidate {
                        interface: name.clone(),
                        address,
                        temporary: None,
                    }),
                    std::net::IpAddr::V4(_) => None,
                })
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn native_interface_addresses() -> Vec<AddressCandidate> {
    std::fs::read_to_string("/proc/net/if_inet6")
        .map(|content| parse_proc_if_inet6(&content))
        .unwrap_or_default()
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
fn native_interface_addresses() -> Vec<AddressCandidate> {
    std::process::Command::new("ifconfig")
        .output()
        .map(|output| parse_ifconfig(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or_default()
}

#[cfg(windows)]
fn native_interface_addresses() -> Vec<AddressCandidate> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // Enums are converted to strings because Windows PowerShell 5.1 would
    // otherwise serialize them as numbers.
    const SCRIPT: &str = "[Console]::OutputEncoding=[Text.Encoding]::UTF8; \
        @(Get-NetIPAddress -AddressFamily IPv6 -ErrorAction SilentlyContinue | \
        Select-Object InterfaceAlias, IPAddress, \
        @{n='PrefixOrigin';e={\"$($_.PrefixOrigin)\"}}, \
        @{n='SuffixOrigin';e={\"$($_.SuffixOrigin)\"}}, \
        @{n='AddressState';e={\"$($_.AddressState)\"}}) | ConvertTo-Json -Compress";
    std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|output| parse_windows_addresses(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or_default()
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly",
    windows
)))]
const fn native_interface_addresses() -> Vec<AddressCandidate> {
    Vec::new()
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WindowsAddress {
    interface_alias: Option<String>,
    #[serde(rename = "IPAddress")]
    ip_address: String,
    #[serde(default)]
    prefix_origin: String,
    #[serde(default)]
    suffix_origin: String,
    #[serde(default)]
    address_state: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum OneOrMany<T> {
    Many(Vec<T>),
    One(T),
}

/// Parses `Get-NetIPAddress | ConvertTo-Json` output. Temporary (privacy)
/// addresses are router-advertised with a random suffix; deprecated,
/// tentative, duplicate and invalid addresses are skipped.
#[must_use]
pub fn parse_windows_addresses(json: &str) -> Vec<AddressCandidate> {
    let json = json.trim().trim_start_matches('\u{feff}');
    if json.is_empty() {
        return Vec::new();
    }
    let rows = match serde_json::from_str::<OneOrMany<WindowsAddress>>(json) {
        Ok(OneOrMany::Many(rows)) => rows,
        Ok(OneOrMany::One(row)) => vec![row],
        Err(_) => return Vec::new(),
    };
    rows.into_iter()
        .filter(|row| {
            row.address_state.is_empty() || row.address_state.eq_ignore_ascii_case("Preferred")
        })
        .filter_map(|row| {
            let address = row.ip_address.split('%').next()?.parse::<Ipv6Addr>().ok()?;
            Some(AddressCandidate {
                interface: row.interface_alias.unwrap_or_default(),
                address,
                temporary: Some(
                    row.prefix_origin
                        .eq_ignore_ascii_case("RouterAdvertisement")
                        && row.suffix_origin.eq_ignore_ascii_case("Random"),
                ),
            })
        })
        .collect()
}

/// Parses Linux `/proc/net/if_inet6`, skipping temporary, deprecated,
/// tentative and DAD-failed addresses.
#[must_use]
pub fn parse_proc_if_inet6(content: &str) -> Vec<AddressCandidate> {
    const TEMPORARY: u32 = 0x01;
    const UNUSABLE: u32 = 0x08 | 0x20 | 0x40;
    content
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let [hex, _, _, _, flags, interface] = fields.as_slice() else {
                return None;
            };
            let flags = u32::from_str_radix(flags, 16).ok()?;
            if flags & UNUSABLE != 0 || hex.len() != 32 {
                return None;
            }
            let value = u128::from_str_radix(hex, 16).ok()?;
            Some(AddressCandidate {
                interface: (*interface).to_string(),
                address: Ipv6Addr::from(value),
                temporary: Some(flags & TEMPORARY != 0),
            })
        })
        .collect()
}

/// Parses BSD/macOS `ifconfig` output, skipping deprecated and detached addresses.
#[must_use]
pub fn parse_ifconfig(output: &str) -> Vec<AddressCandidate> {
    let mut interface = String::new();
    let mut candidates = Vec::new();
    for line in output.lines() {
        if !line.starts_with(char::is_whitespace) {
            if let Some((name, _)) = line.split_once(':') {
                interface = name.trim().to_string();
            }
            continue;
        }
        let mut words = line.split_whitespace();
        if words.next() != Some("inet6") {
            continue;
        }
        let Some(address) = words
            .next()
            .and_then(|value| value.split('%').next())
            .and_then(|value| value.parse::<Ipv6Addr>().ok())
        else {
            continue;
        };
        let flags: Vec<&str> = words.collect();
        if flags.iter().any(|flag| {
            matches!(
                *flag,
                "deprecated" | "detached" | "tentative" | "duplicated"
            )
        }) {
            continue;
        }
        candidates.push(AddressCandidate {
            interface: interface.clone(),
            address,
            temporary: Some(flags.contains(&"temporary")),
        });
    }
    candidates
}

// ---------------------------------------------------------------------------
// Cloudflare API
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CloudflareEnvelope<T> {
    success: bool,
    #[serde(default)]
    errors: Vec<CloudflareMessage>,
    result: Option<T>,
}

#[derive(Deserialize)]
struct CloudflareMessage {
    code: Option<i64>,
    message: String,
}

#[derive(Deserialize)]
struct CloudflareZone {
    id: String,
}

#[derive(Deserialize)]
struct CloudflareRecord {
    id: String,
    content: String,
    ttl: u32,
    #[serde(default)]
    proxied: bool,
}

struct CloudflareClient {
    http: reqwest::Client,
    api_base: String,
    token: String,
}

impl CloudflareClient {
    async fn new(db: &DatabaseConnection, settings: &DdnsSettings) -> Result<Self, String> {
        let http = build_http_client_with_overrides(
            db,
            HttpClientRuntimeOverrides {
                request_timeout_seconds: Some(REQUEST_TIMEOUT_SECONDS),
                ..Default::default()
            },
        )
        .await
        .map_err(|error| format!("创建 HTTP 客户端失败：{}", error.message()))?;
        Ok(Self {
            http,
            api_base: settings.api_base.clone(),
            token: settings.api_token.clone(),
        })
    }

    async fn sync_records(
        &self,
        settings: &DdnsSettings,
        address: Ipv6Addr,
    ) -> Vec<DdnsRecordStatus> {
        let mut zones: HashMap<String, String> = HashMap::new();
        let mut results = Vec::with_capacity(settings.record_names.len());
        for name in &settings.record_names {
            let outcome = async {
                let zone_id = match &settings.zone_id {
                    Some(zone_id) => zone_id.clone(),
                    None => self.resolve_zone(name, &mut zones).await?,
                };
                self.upsert_record(&zone_id, name, address, settings).await
            }
            .await;
            results.push(match outcome {
                Ok(action) => DdnsRecordStatus {
                    name: name.clone(),
                    action: action.to_string(),
                    content: Some(address.to_string()),
                    message: None,
                },
                Err(message) => DdnsRecordStatus {
                    name: name.clone(),
                    action: "failed".to_string(),
                    content: None,
                    message: Some(message),
                },
            });
        }
        results
    }

    async fn resolve_zone(
        &self,
        name: &str,
        cache: &mut HashMap<String, String>,
    ) -> Result<String, String> {
        for candidate in zone_candidates(name) {
            if let Some(zone_id) = cache.get(&candidate) {
                return Ok(zone_id.clone());
            }
            let zones: Vec<CloudflareZone> = self
                .request(
                    self.http
                        .get(format!("{}/zones", self.api_base))
                        .query(&[("name", candidate.as_str())]),
                )
                .await?;
            if let Some(zone) = zones.into_iter().next() {
                cache.insert(candidate, zone.id.clone());
                return Ok(zone.id);
            }
        }
        Err(format!(
            "当前 Token 下找不到 {name} 所属的 Zone，请检查域名或填写 {KEY_ZONE_ID}"
        ))
    }

    async fn upsert_record(
        &self,
        zone_id: &str,
        name: &str,
        address: Ipv6Addr,
        settings: &DdnsSettings,
    ) -> Result<&'static str, String> {
        let records_url = format!("{}/zones/{zone_id}/dns_records", self.api_base);
        let existing: Vec<CloudflareRecord> = self
            .request(
                self.http
                    .get(&records_url)
                    .query(&[("type", "AAAA"), ("name", name)]),
            )
            .await?;
        let body = serde_json::json!({
            "type": "AAAA",
            "name": name,
            "content": address.to_string(),
            "ttl": settings.ttl,
            "proxied": settings.proxied,
        });

        let Some(record) = existing.into_iter().next() else {
            let _: serde_json::Value = self
                .request(self.http.post(&records_url).json(&body))
                .await?;
            return Ok("created");
        };
        let same_address = record.content.parse::<Ipv6Addr>().ok() == Some(address);
        if same_address && record.ttl == settings.ttl && record.proxied == settings.proxied {
            return Ok("unchanged");
        }
        let _: serde_json::Value = self
            .request(
                self.http
                    .patch(format!("{records_url}/{}", record.id))
                    .json(&body),
            )
            .await?;
        Ok("updated")
    }

    async fn request<T: DeserializeOwned>(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> Result<T, String> {
        let response = builder
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|error| format!("请求 Cloudflare 失败：{error}"))?;
        let status = response.status();
        let envelope: CloudflareEnvelope<T> = response.json().await.map_err(|error| {
            format!("Cloudflare 返回了无法解析的响应（HTTP {status}）：{error}")
        })?;
        if envelope.success {
            return envelope
                .result
                .ok_or_else(|| "Cloudflare 响应缺少 result".to_string());
        }
        let detail = envelope
            .errors
            .iter()
            .map(|error| {
                error.code.map_or_else(
                    || error.message.clone(),
                    |code| format!("[{code}] {}", error.message),
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        Err(format!("Cloudflare 返回错误（HTTP {status}）：{detail}"))
    }
}

/// Candidate zone names for a record, longest first, e.g.
/// `a.b.example.com` → `a.b.example.com`, `b.example.com`, `example.com`.
#[must_use]
pub fn zone_candidates(name: &str) -> Vec<String> {
    let labels: Vec<&str> = name.trim_end_matches('.').split('.').collect();
    (0..labels.len().saturating_sub(1))
        .map(|start| labels[start..].join("."))
        .collect()
}

#[must_use]
pub fn parse_record_names(value: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for name in value
        .split([',', '，', '\n', ' '])
        .map(|name| name.trim().trim_end_matches('.').to_ascii_lowercase())
        .filter(|name| !name.is_empty())
    {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

fn is_valid_hostname(name: &str) -> bool {
    let labels: Vec<&str> = name.split('.').collect();
    name.len() <= 253
        && labels.len() >= 2
        && labels.iter().enumerate().all(|(index, label)| {
            (index == 0 && *label == "*")
                || (!label.is_empty()
                    && label.len() <= 63
                    && !label.starts_with('-')
                    && !label.ends_with('-')
                    && label
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-'))
        })
}

/// Cloudflare accepts 1 (automatic) or 60–86400 seconds.
fn normalize_ttl(value: i64) -> u32 {
    if (60..=86_400).contains(&value) {
        u32::try_from(value).unwrap_or(1)
    } else {
        1
    }
}

fn non_empty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn now_text() -> String {
    let now: DateTime<Local> = Local::now();
    now.to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(interface: &str, address: &str, temporary: Option<bool>) -> AddressCandidate {
        AddressCandidate {
            interface: interface.to_string(),
            address: address.parse().unwrap(),
            temporary,
        }
    }

    #[test]
    fn global_unicast_filter_rejects_local_and_documentation_ranges() {
        assert!(is_global_unicast("2409:8a1e:1234::1".parse().unwrap()));
        assert!(!is_global_unicast("fe80::1".parse().unwrap()));
        assert!(!is_global_unicast("fd00::1".parse().unwrap()));
        assert!(!is_global_unicast("::1".parse().unwrap()));
        assert!(!is_global_unicast("2001:db8::1".parse().unwrap()));
    }

    #[test]
    fn lookup_response_must_be_public_ipv6() {
        assert_eq!(
            parse_public_ipv6(" 2409:8a1e:1234::abcd\n"),
            Some("2409:8a1e:1234::abcd".parse().unwrap())
        );
        assert_eq!(parse_public_ipv6("1.2.3.4"), None);
        assert_eq!(parse_public_ipv6("fe80::1"), None);
        assert_eq!(parse_public_ipv6("<html>"), None);
    }

    #[test]
    fn stable_addresses_win_over_temporary_ones() {
        let candidates = [
            candidate("en0", "2409:895c:3177:592:e9e7:1d16:8014:7475", Some(true)),
            candidate("en0", "2409:895c:3177:592:1c7b:2a42:3d7c:e2a9", Some(false)),
            candidate("en0", "fe80::1", Some(false)),
        ];
        assert_eq!(
            pick_address(&candidates, None),
            Some("2409:895c:3177:592:1c7b:2a42:3d7c:e2a9".parse().unwrap())
        );
    }

    #[test]
    fn interface_filter_and_eui64_preference() {
        let candidates = [
            candidate("eth0", "2409:1::1", None),
            candidate("eth1", "2409:2::211:22ff:fe33:4455", None),
            candidate("eth1", "2409:2::9", None),
        ];
        assert_eq!(
            pick_address(&candidates, Some("eth1")),
            Some("2409:2::211:22ff:fe33:4455".parse().unwrap())
        );
        assert_eq!(pick_address(&candidates, Some("wlan0")), None);
    }

    #[test]
    fn linux_proc_entries_skip_temporary_flags_and_unusable_states() {
        let content = "\
240988a1e123400000000000000000a1 02 40 00 00 eth0
240988a1e123400000000000000000b2 02 40 00 01 eth0
240988a1e123400000000000000000c3 02 40 00 20 eth0
fe800000000000000000000000000001 02 40 20 80 eth0
";
        let parsed = parse_proc_if_inet6(content);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].temporary, Some(false));
        assert_eq!(parsed[1].temporary, Some(true));
        assert_eq!(
            pick_address(&parsed, None),
            Some("2409:88a1:e123:4000::a1".parse().unwrap())
        );
    }

    #[test]
    fn macos_ifconfig_marks_temporary_addresses() {
        let output = "\
lo0: flags=8049<UP,LOOPBACK,RUNNING,MULTICAST> mtu 16384
\tinet6 ::1 prefixlen 128
en0: flags=8863<UP,BROADCAST,SMART,RUNNING,SIMPLEX,MULTICAST> mtu 1500
\tinet6 fe80::2f:9c85:1c10:fbc7%en0 prefixlen 64 secured scopeid 0xe
\tinet6 2409:895c:3177:592:1c7b:2a42:3d7c:e2a9 prefixlen 64 autoconf secured
\tinet6 2409:895c:3177:592:e9e7:1d16:8014:7475 prefixlen 64 autoconf temporary
\tinet6 2409:895c:3177:592::dead prefixlen 64 autoconf deprecated
";
        let parsed = parse_ifconfig(output);
        assert_eq!(parsed.len(), 4);
        assert!(parsed
            .iter()
            .all(|item| item.address.to_string() != "2409:895c:3177:592::dead"));
        assert_eq!(
            pick_address(&parsed, Some("en0")),
            Some("2409:895c:3177:592:1c7b:2a42:3d7c:e2a9".parse().unwrap())
        );
    }

    #[test]
    fn windows_powershell_output_prefers_public_addresses() {
        let json = r#"[
            {"InterfaceAlias":"以太网","IPAddress":"fe80::1%12","PrefixOrigin":"WellKnown","SuffixOrigin":"Link","AddressState":"Preferred"},
            {"InterfaceAlias":"以太网","IPAddress":"2409:8a1e:1::aaaa","PrefixOrigin":"RouterAdvertisement","SuffixOrigin":"Random","AddressState":"Preferred"},
            {"InterfaceAlias":"以太网","IPAddress":"2409:8a1e:1::bbbb","PrefixOrigin":"RouterAdvertisement","SuffixOrigin":"Link","AddressState":"Preferred"},
            {"InterfaceAlias":"以太网","IPAddress":"2409:8a1e:1::cccc","PrefixOrigin":"RouterAdvertisement","SuffixOrigin":"Link","AddressState":"Deprecated"}
        ]"#;
        let parsed = parse_windows_addresses(json);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[1].temporary, Some(true));
        assert_eq!(
            pick_address(&parsed, Some("以太网")),
            Some("2409:8a1e:1::bbbb".parse().unwrap())
        );

        let single = r#"{"InterfaceAlias":"Wi-Fi","IPAddress":"2409:8a1e:2::1","PrefixOrigin":"Dhcp","SuffixOrigin":"Dhcp","AddressState":"Preferred"}"#;
        assert_eq!(parse_windows_addresses(single).len(), 1);
        assert!(parse_windows_addresses("").is_empty());
        assert!(parse_windows_addresses("not json").is_empty());
    }

    #[test]
    fn manual_addresses_reject_unroutable_ranges() {
        assert!(parse_manual_ipv6("2409:8a1e:1::1").is_ok());
        assert!(parse_manual_ipv6(" fd12:3456::1 ").is_ok());
        assert!(parse_manual_ipv6("::1").is_err());
        assert!(parse_manual_ipv6("fe80::1").is_err());
        assert!(parse_manual_ipv6("ff02::1").is_err());
        assert!(parse_manual_ipv6("::ffff:1.2.3.4").is_err());
        assert!(parse_manual_ipv6("1.2.3.4").is_err());
    }

    #[test]
    fn zone_candidates_walk_up_to_the_registered_domain() {
        assert_eq!(
            zone_candidates("a.b.example.com"),
            vec!["a.b.example.com", "b.example.com", "example.com"]
        );
        assert_eq!(zone_candidates("example.com."), vec!["example.com"]);
    }

    #[test]
    fn record_names_are_normalized_and_deduplicated() {
        assert_eq!(
            parse_record_names(" Home.Example.com, nas.example.com.，home.example.com\n"),
            vec!["home.example.com", "nas.example.com"]
        );
    }

    #[test]
    fn setting_validation() {
        assert!(validate_setting(KEY_SOURCE, "interface").is_ok());
        assert!(validate_setting(KEY_SOURCE, "manual").is_ok());
        assert!(validate_setting(KEY_MANUAL_IPV6, "2409:8a1e::1").is_ok());
        assert!(validate_setting(KEY_MANUAL_IPV6, "fe80::1").is_err());
        assert!(validate_setting(KEY_MANUAL_IPV6, "").is_ok());
        assert!(validate_setting(KEY_SOURCE, "dhcp").is_err());
        assert!(validate_setting(KEY_INTERVAL, "300").is_ok());
        assert!(validate_setting(KEY_INTERVAL, "5").is_err());
        assert!(validate_setting(KEY_TTL, "1").is_ok());
        assert!(validate_setting(KEY_TTL, "120").is_ok());
        assert!(validate_setting(KEY_TTL, "30").is_err());
        assert!(validate_setting(KEY_RECORD_NAMES, "home.example.com,*.lab.example.com").is_ok());
        assert!(validate_setting(KEY_RECORD_NAMES, "localhost").is_err());
        assert!(validate_setting(KEY_RECORD_NAMES, "").is_ok());
        assert!(validate_setting(KEY_LOOKUP_URL, "ftp://x").is_err());
        assert!(validate_setting(KEY_LOOKUP_URL, "https://api6.ipify.org").is_ok());
    }
}
