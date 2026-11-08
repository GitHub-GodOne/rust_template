//! Domain certificates and domain → local service mappings.
//!
//! A mapping publishes `upstream_host:upstream_port` under a domain, either
//! - `direct`: Let's Encrypt certificate (DNS-01 via Cloudflare) served by the
//!   built-in HTTPS proxy on 443 or any custom port, with A/AAAA records kept
//!   pointed at this host; or
//! - `tunnel`: a Cloudflare Tunnel run by a supervised `cloudflared`, with
//!   HTTPS terminated at Cloudflare's edge.

#![allow(clippy::missing_errors_doc)]

pub mod acme;
pub mod cloudflare;
pub mod proxy;
pub mod tunnel;

use std::{
    collections::{HashMap, HashSet},
    hash::{DefaultHasher, Hash, Hasher},
    net::{Ipv4Addr, Ipv6Addr},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use chrono::{DateTime, FixedOffset, Local};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, IntoActiveModel, QueryFilter,
    QueryOrder, Set,
};
use serde::{Deserialize, Serialize};

use self::cloudflare::{CloudflareApi, CloudflareCredentials, DesiredRecord};
use crate::{
    errors::{ApiError, ApiResult},
    models::_entities::{domain_mappings, system_settings, tls_certificates},
    services::{
        cloudflare_ddns,
        http_client::{build_http_client_with_overrides, HttpClientRuntimeOverrides},
        ip_rules::IpRule,
    },
};

pub const SETTINGS_KEY: &str = "certificates.config";
pub const STORAGE_DIR: &str = "storage/certificates";
pub const MODE_DIRECT: &str = "direct";
pub const MODE_TUNNEL: &str = "tunnel";
// Keep compatibility with stable Rust versions before duration_constructors.
#[allow(clippy::duration_suboptimal_units)]
const SYNC_INTERVAL: Duration = Duration::from_secs(5 * 60);
#[allow(clippy::duration_suboptimal_units)]
const VERIFY_INTERVAL: Duration = Duration::from_secs(60 * 60);
const RENEW_RETRY_INTERVAL: chrono::Duration = chrono::Duration::hours(6);

/// Settings stored as one JSON system setting.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct CertificateSettings {
    pub acme_email: String,
    /// `production`, `staging` or a custom ACME directory URL.
    pub acme_directory: String,
    /// Empty token → reuse the IPv6 DDNS Cloudflare credentials.
    pub cloudflare_auth_mode: String,
    pub cloudflare_api_token: String,
    pub cloudflare_api_email: String,
    pub cloudflare_account_id: String,
    pub cloudflare_api_base: String,
    pub renew_before_days: u32,
    pub propagation_timeout_seconds: u64,
    pub ipv4_lookup_url: String,
    pub cloudflared_path: String,
    pub tunnel_name: String,
    pub tunnel_id: String,
    pub tunnel_token: String,
}

impl Default for CertificateSettings {
    fn default() -> Self {
        Self {
            acme_email: String::new(),
            acme_directory: "production".to_string(),
            cloudflare_auth_mode: "api_token".to_string(),
            cloudflare_api_token: String::new(),
            cloudflare_api_email: String::new(),
            cloudflare_account_id: String::new(),
            cloudflare_api_base: String::new(),
            renew_before_days: 30,
            propagation_timeout_seconds: 120,
            ipv4_lookup_url: "https://api.ipify.org".to_string(),
            cloudflared_path: String::new(),
            tunnel_name: String::new(),
            tunnel_id: String::new(),
            tunnel_token: String::new(),
        }
    }
}

/// Settings as shown in the admin UI: secrets are reported, never returned.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct CertificateSettingsView {
    pub acme_email: String,
    pub acme_directory: String,
    pub cloudflare_auth_mode: String,
    pub cloudflare_api_token_set: bool,
    pub cloudflare_api_email: String,
    pub cloudflare_account_id: String,
    pub cloudflare_api_base: String,
    /// True when no token is set here and the DDNS credentials are used instead.
    pub uses_ddns_credentials: bool,
    pub renew_before_days: u32,
    pub propagation_timeout_seconds: u64,
    pub ipv4_lookup_url: String,
    pub cloudflared_path: String,
    pub tunnel_name: String,
    pub tunnel_id: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveCertificateSettingsParams {
    pub acme_email: Option<String>,
    pub acme_directory: Option<String>,
    pub cloudflare_auth_mode: Option<String>,
    /// Omit to keep the saved token; an empty string clears it.
    pub cloudflare_api_token: Option<String>,
    pub cloudflare_api_email: Option<String>,
    pub cloudflare_account_id: Option<String>,
    pub cloudflare_api_base: Option<String>,
    pub renew_before_days: Option<u32>,
    pub propagation_timeout_seconds: Option<u64>,
    pub ipv4_lookup_url: Option<String>,
    pub cloudflared_path: Option<String>,
    pub tunnel_name: Option<String>,
}

pub async fn load_settings(db: &DatabaseConnection) -> ApiResult<CertificateSettings> {
    let setting = system_settings::Entity::find()
        .filter(system_settings::Column::Key.eq(SETTINGS_KEY))
        .one(db)
        .await?;
    Ok(setting
        .and_then(|setting| serde_json::from_str(&setting.value).ok())
        .unwrap_or_default())
}

async fn store_settings(
    db: &DatabaseConnection,
    settings: &CertificateSettings,
    user_id: Option<i32>,
) -> ApiResult<()> {
    let value =
        serde_json::to_string(settings).map_err(|_| ApiError::internal("序列化证书设置失败"))?;
    let existing = system_settings::Entity::find()
        .filter(system_settings::Column::Key.eq(SETTINGS_KEY))
        .one(db)
        .await?;
    if let Some(existing) = existing {
        let mut active = existing.into_active_model();
        active.value = Set(value);
        if user_id.is_some() {
            active.updated_by = Set(user_id);
        }
        active.update(db).await?;
    } else {
        system_settings::ActiveModel {
            key: Set(SETTINGS_KEY.to_string()),
            name: Set("证书与域名映射设置".to_string()),
            group_key: Set("certificates".to_string()),
            value: Set(value),
            value_type: Set("json".to_string()),
            default_value: Set(Some("{}".to_string())),
            description: Set(Some(
                "ACME 证书申请、Cloudflare 凭据与 Cloudflare Tunnel 配置".to_string(),
            )),
            is_public: Set(false),
            is_builtin: Set(true),
            is_encrypted: Set(true),
            sort_order: Set(260),
            created_by: Set(user_id),
            updated_by: Set(user_id),
            ..Default::default()
        }
        .insert(db)
        .await?;
    }
    Ok(())
}

#[must_use]
pub fn settings_view(settings: &CertificateSettings) -> CertificateSettingsView {
    CertificateSettingsView {
        acme_email: settings.acme_email.clone(),
        acme_directory: settings.acme_directory.clone(),
        cloudflare_auth_mode: settings.cloudflare_auth_mode.clone(),
        cloudflare_api_token_set: !settings.cloudflare_api_token.is_empty(),
        cloudflare_api_email: settings.cloudflare_api_email.clone(),
        cloudflare_account_id: settings.cloudflare_account_id.clone(),
        cloudflare_api_base: settings.cloudflare_api_base.clone(),
        uses_ddns_credentials: settings.cloudflare_api_token.is_empty(),
        renew_before_days: settings.renew_before_days,
        propagation_timeout_seconds: settings.propagation_timeout_seconds,
        ipv4_lookup_url: settings.ipv4_lookup_url.clone(),
        cloudflared_path: settings.cloudflared_path.clone(),
        tunnel_name: settings.tunnel_name.clone(),
        tunnel_id: settings.tunnel_id.clone(),
    }
}

pub async fn save_settings(
    db: &DatabaseConnection,
    params: SaveCertificateSettingsParams,
    user_id: i32,
) -> ApiResult<CertificateSettings> {
    let mut settings = load_settings(db).await?;
    let trimmed = |value: Option<String>| value.map(|value| value.trim().to_string());
    if let Some(email) = trimmed(params.acme_email) {
        if !email.is_empty() && !email.contains('@') {
            return Err(ApiError::bad_request("ACME 联系邮箱格式不正确"));
        }
        settings.acme_email = email;
    }
    if let Some(directory) = trimmed(params.acme_directory) {
        if !matches!(directory.as_str(), "" | "production" | "staging")
            && !directory.starts_with("https://")
        {
            return Err(ApiError::bad_request(
                "ACME 目录只能是 production、staging 或 https:// 开头的地址",
            ));
        }
        settings.acme_directory = if directory.is_empty() {
            "production".to_string()
        } else {
            directory
        };
    }
    if let Some(mode) = trimmed(params.cloudflare_auth_mode) {
        if !matches!(mode.as_str(), "api_token" | "global_api_key") {
            return Err(ApiError::bad_request(
                "Cloudflare 认证方式必须是 api_token 或 global_api_key",
            ));
        }
        settings.cloudflare_auth_mode = mode;
    }
    if let Some(token) = trimmed(params.cloudflare_api_token) {
        settings.cloudflare_api_token = token;
    }
    if let Some(email) = trimmed(params.cloudflare_api_email) {
        settings.cloudflare_api_email = email;
    }
    if let Some(account_id) = trimmed(params.cloudflare_account_id) {
        if !account_id
            .chars()
            .all(|ch| ch.is_ascii_hexdigit() || ch == '-')
        {
            return Err(ApiError::bad_request("Cloudflare Account ID 格式不正确"));
        }
        if account_id != settings.cloudflare_account_id {
            // A tunnel belongs to one account; start over on the new one.
            settings.tunnel_id.clear();
            settings.tunnel_token.clear();
        }
        settings.cloudflare_account_id = account_id;
    }
    if let Some(base) = trimmed(params.cloudflare_api_base) {
        if !base.is_empty() && !base.starts_with("http://") && !base.starts_with("https://") {
            return Err(ApiError::bad_request(
                "Cloudflare API 地址必须以 http(s):// 开头",
            ));
        }
        settings.cloudflare_api_base = base;
    }
    if let Some(days) = params.renew_before_days {
        if !(1..=60).contains(&days) {
            return Err(ApiError::bad_request("提前续签天数需在 1 到 60 之间"));
        }
        settings.renew_before_days = days;
    }
    if let Some(seconds) = params.propagation_timeout_seconds {
        settings.propagation_timeout_seconds = seconds.min(600);
    }
    if let Some(url) = trimmed(params.ipv4_lookup_url) {
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(ApiError::bad_request("IPv4 查询地址必须以 http(s):// 开头"));
        }
        settings.ipv4_lookup_url = url;
    }
    if let Some(path) = trimmed(params.cloudflared_path) {
        settings.cloudflared_path = path;
    }
    if let Some(name) = trimmed(params.tunnel_name) {
        if !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        {
            return Err(ApiError::bad_request(
                "隧道名称只能包含字母、数字、短横线和下划线",
            ));
        }
        if name != settings.tunnel_name {
            settings.tunnel_id.clear();
            settings.tunnel_token.clear();
        }
        settings.tunnel_name = name;
    }
    store_settings(db, &settings, Some(user_id)).await?;
    forget_sync_state();
    Ok(settings)
}

/// Cloudflare credentials from these settings, or the DDNS ones when unset.
pub async fn cloudflare_credentials(
    db: &DatabaseConnection,
    settings: &CertificateSettings,
) -> ApiResult<CloudflareCredentials> {
    if !settings.cloudflare_api_token.is_empty() {
        return Ok(CloudflareCredentials {
            api_base: non_empty_or(
                &settings.cloudflare_api_base,
                cloudflare_ddns::DEFAULT_API_BASE,
            ),
            auth_mode: non_empty_or(&settings.cloudflare_auth_mode, "api_token"),
            api_token: settings.cloudflare_api_token.clone(),
            api_email: settings.cloudflare_api_email.clone(),
        });
    }
    let ddns = cloudflare_ddns::load_settings(db).await?;
    Ok(CloudflareCredentials {
        api_base: non_empty_or(&settings.cloudflare_api_base, &ddns.api_base),
        auth_mode: ddns.auth_mode,
        api_token: ddns.api_token,
        api_email: ddns.api_email,
    })
}

fn non_empty_or(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.trim().to_string()
    }
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

/// Lower-case ASCII (punycode) domain; `*.` prefix only when allowed.
pub fn normalize_domain(value: &str, allow_wildcard: bool) -> Result<String, String> {
    let value = value.trim().trim_end_matches('.').to_lowercase();
    let (wildcard, rest) = value
        .strip_prefix("*.")
        .map_or((false, value.as_str()), |rest| (true, rest));
    if wildcard && !allow_wildcard {
        return Err("域名映射不能使用通配符域名".to_string());
    }
    let ascii = url::Url::parse(&format!("https://{rest}/"))
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .filter(|host| host.parse::<std::net::IpAddr>().is_err() && !host.starts_with('['))
        .ok_or_else(|| format!("{value} 不是有效的域名"))?;
    let labels: Vec<&str> = ascii.split('.').collect();
    let valid = ascii.len() <= 253
        && labels.len() >= 2
        && labels.iter().all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
        });
    if !valid {
        return Err(format!("{value} 不是有效的域名"));
    }
    Ok(if wildcard {
        format!("*.{ascii}")
    } else {
        ascii
    })
}

pub fn validate_upstream_host(value: &str) -> Result<String, String> {
    let value = value.trim().trim_start_matches('[').trim_end_matches(']');
    if value.is_empty() {
        return Err("本地服务地址不能为空".to_string());
    }
    if value.parse::<std::net::IpAddr>().is_ok() || value.eq_ignore_ascii_case("localhost") {
        return Ok(value.to_ascii_lowercase());
    }
    if value.len() <= 253
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-'))
    {
        return Ok(value.to_ascii_lowercase());
    }
    Err(format!("{value} 不是有效的本地服务地址"))
}

pub fn validate_port(value: i32, field: &str) -> Result<u16, String> {
    u16::try_from(value)
        .ok()
        .filter(|port| *port > 0)
        .ok_or_else(|| format!("{field}需在 1 到 65535 之间"))
}

pub fn normalize_ip_mode(
    mode: &str,
    address: Option<&str>,
    v6: bool,
) -> Result<(String, Option<String>), String> {
    let label = if v6 { "IPv6" } else { "IPv4" };
    match mode.trim() {
        "" | "off" => Ok(("off".to_string(), None)),
        "auto" => Ok(("auto".to_string(), None)),
        "manual" => {
            let address = address.map(str::trim).unwrap_or_default();
            if v6 {
                cloudflare_ddns::parse_manual_ipv6(address)
                    .map(|address| ("manual".to_string(), Some(address.to_string())))
            } else {
                address
                    .parse::<Ipv4Addr>()
                    .ok()
                    .filter(|address| !address.is_unspecified() && !address.is_broadcast())
                    .map(|address| ("manual".to_string(), Some(address.to_string())))
                    .ok_or_else(|| format!("{address} 不是有效的 IPv4 地址"))
            }
        }
        other => Err(format!(
            "{label} 映射方式 {other} 无效，可选 off、auto、manual"
        )),
    }
}

/// Public URL of a mapping, e.g. `https://app.example.com:8443`.
#[must_use]
pub fn public_url(domain: &str, mode: &str, public_port: i32) -> String {
    if mode == MODE_TUNNEL || public_port == 443 {
        format!("https://{domain}")
    } else {
        format!("https://{domain}:{public_port}")
    }
}

/// Whether a certificate for `names` is valid for `domain`.
#[must_use]
pub fn certificate_covers(names: &[String], domain: &str) -> bool {
    let domain = domain.to_ascii_lowercase();
    names.iter().any(|name| {
        let name = name.to_ascii_lowercase();
        name == domain
            || name.strip_prefix("*.").is_some_and(|parent| {
                domain
                    .split_once('.')
                    .is_some_and(|(_, rest)| rest == parent)
            })
    })
}

#[must_use]
pub fn certificate_names(certificate: &tls_certificates::Model) -> Vec<String> {
    let mut names = vec![certificate.domain.clone()];
    names.extend(
        serde_json::from_str::<Vec<String>>(&certificate.alt_names)
            .unwrap_or_default()
            .into_iter()
            .filter(|name| !name.is_empty() && *name != certificate.domain),
    );
    names
}

/// Whether a certificate expiring at `not_after` should be renewed now.
#[must_use]
pub fn renewal_due(
    not_after: Option<DateTime<FixedOffset>>,
    now: DateTime<FixedOffset>,
    renew_before_days: u32,
) -> bool {
    not_after.is_none_or(|not_after| {
        not_after - now <= chrono::Duration::days(i64::from(renew_before_days))
    })
}

/// Cloudflare Tunnel ingress rules for the enabled tunnel mappings.
#[must_use]
pub fn tunnel_ingress(mappings: &[&domain_mappings::Model]) -> serde_json::Value {
    let mut sorted: Vec<&&domain_mappings::Model> = mappings.iter().collect();
    sorted.sort_by(|left, right| left.domain.cmp(&right.domain));
    let mut rules: Vec<serde_json::Value> = sorted
        .iter()
        .map(|mapping| {
            let host = if mapping.upstream_host.contains(':') {
                format!("[{}]", mapping.upstream_host)
            } else {
                mapping.upstream_host.clone()
            };
            serde_json::json!({
                "hostname": mapping.domain,
                "service": format!("http://{host}:{}", mapping.upstream_port),
            })
        })
        .collect();
    rules.push(serde_json::json!({ "service": "http_status:404" }));
    serde_json::json!({ "ingress": rules })
}

#[must_use]
pub fn default_tunnel_name() -> String {
    let host = sysinfo::System::host_name().unwrap_or_else(|| "server".to_string());
    let host: String = host
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    format!("gpt-images-{}", host.trim_matches('-'))
}

// ---------------------------------------------------------------------------
// Runtime state
// ---------------------------------------------------------------------------

#[derive(Default)]
struct SyncState {
    dns: HashMap<i32, DnsSync>,
    tunnel: Option<(u64, Instant)>,
}

struct DnsSync {
    fingerprint: u64,
    at: Instant,
    error: Option<String>,
}

fn sync_state() -> &'static Mutex<SyncState> {
    static STATE: OnceLock<Mutex<SyncState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(SyncState::default()))
}

fn forget_sync_state() {
    if let Ok(mut state) = sync_state().lock() {
        *state = SyncState::default();
    }
}

fn issuing() -> &'static Mutex<HashSet<i32>> {
    static ISSUING: OnceLock<Mutex<HashSet<i32>>> = OnceLock::new();
    ISSUING.get_or_init(|| Mutex::new(HashSet::new()))
}

#[must_use]
pub fn is_issuing(certificate_id: i32) -> bool {
    issuing()
        .lock()
        .is_ok_and(|set| set.contains(&certificate_id))
}

fn reconcile_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn fingerprint(value: &impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn now() -> DateTime<FixedOffset> {
    Local::now().fixed_offset()
}

// ---------------------------------------------------------------------------
// Certificate issuing
// ---------------------------------------------------------------------------

/// Starts issuing (or renewing) a certificate in the background.
pub async fn start_issue(db: &DatabaseConnection, certificate_id: i32) -> ApiResult<()> {
    let settings = load_settings(db).await?;
    let credentials = cloudflare_credentials(db, &settings).await?;
    if credentials.api_token.is_empty() {
        return Err(ApiError::bad_request(
            "申请证书需要 Cloudflare API Token（DNS 编辑权限）：请先在「设置」中填写，或配置 IPv6 动态解析的 Cloudflare 凭据",
        ));
    }
    let certificate = tls_certificates::Entity::find_by_id(certificate_id)
        .one(db)
        .await?
        .ok_or_else(|| ApiError::not_found("证书不存在"))?;
    {
        let mut set = issuing()
            .lock()
            .map_err(|_| ApiError::internal("证书任务状态异常"))?;
        if !set.insert(certificate_id) {
            return Err(ApiError::bad_request("该证书正在签发中，请稍候"));
        }
    }
    // A renewal keeps serving the current certificate until the new one arrives.
    let renewing = certificate.cert_path.is_some();
    let mut active = certificate.into_active_model();
    if !renewing {
        active.status = Set("issuing".to_string());
    }
    active.last_attempt_at = Set(Some(now()));
    active.last_error = Set(None);
    active.update(db).await?;

    let db = db.clone();
    tokio::spawn(async move {
        let result = issue_now(&db, certificate_id, &settings, &credentials).await;
        if let Ok(mut set) = issuing().lock() {
            set.remove(&certificate_id);
        }
        if let Err(error) = result {
            tracing::warn!(certificate_id, error, "certificate issuing failed");
        }
        if let Err(error) = sync_all(&db, true).await {
            tracing::warn!(error = ?error, "certificate reconcile after issuing failed");
        }
    });
    Ok(())
}

async fn issue_now(
    db: &DatabaseConnection,
    certificate_id: i32,
    settings: &CertificateSettings,
    credentials: &CloudflareCredentials,
) -> Result<(), String> {
    let certificate = tls_certificates::Entity::find_by_id(certificate_id)
        .one(db)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "证书不存在".to_string())?;
    let names = certificate_names(&certificate);
    let outcome = async {
        let cloudflare = CloudflareApi::new(db, credentials).await?;
        acme::issue(
            &cloudflare,
            &acme::IssueRequest {
                names: &names,
                directory: &settings.acme_directory,
                contact_email: &settings.acme_email,
                storage_dir: Path::new(STORAGE_DIR),
                propagation_timeout: Duration::from_secs(settings.propagation_timeout_seconds),
            },
        )
        .await
    }
    .await;

    let had_certificate = certificate.cert_path.is_some();
    let mut active = certificate.into_active_model();
    match &outcome {
        Ok(issued) => {
            active.status = Set("active".to_string());
            active.cert_path = Set(Some(issued.cert_path.display().to_string()));
            active.key_path = Set(Some(issued.key_path.display().to_string()));
            active.not_before = Set(Some(issued.not_before));
            active.not_after = Set(Some(issued.not_after));
            active.issuer = Set(Some(issued.issuer.clone()));
            active.renewed_at = Set(Some(now()));
            active.last_error = Set(None);
        }
        Err(error) => {
            // A failed renewal keeps serving the still-valid certificate.
            if !had_certificate {
                active.status = Set("failed".to_string());
            }
            active.last_error = Set(Some(error.clone()));
        }
    }
    active.update(db).await.map_err(|error| error.to_string())?;
    outcome.map(|_| ())
}

async fn renew_due_certificates(db: &DatabaseConnection) -> ApiResult<()> {
    let settings = load_settings(db).await?;
    let now = now();
    let certificates = tls_certificates::Entity::find()
        .filter(tls_certificates::Column::AutoRenew.eq(true))
        .all(db)
        .await?;
    for certificate in certificates {
        if !renewal_due(certificate.not_after, now, settings.renew_before_days)
            || is_issuing(certificate.id)
        {
            continue;
        }
        let recently_tried = certificate
            .last_attempt_at
            .is_some_and(|at| now - at < RENEW_RETRY_INTERVAL);
        if recently_tried {
            continue;
        }
        if let Err(error) = start_issue(db, certificate.id).await {
            tracing::warn!(certificate = certificate.domain, error = ?error, "automatic renewal not started");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Reconciliation: proxy listeners, DNS records and the tunnel
// ---------------------------------------------------------------------------

async fn public_ipv4(db: &DatabaseConnection, url: &str) -> Result<Ipv4Addr, String> {
    let client = build_http_client_with_overrides(
        db,
        HttpClientRuntimeOverrides {
            request_timeout_seconds: Some(15),
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
        .map_err(|error| format!("通过 {url} 查询公网 IPv4 失败：{error}"))?
        .text()
        .await
        .map_err(|error| format!("读取 {url} 响应失败：{error}"))?;
    body.trim()
        .parse::<Ipv4Addr>()
        .map_err(|_| format!("{url} 返回的不是 IPv4 地址"))
}

async fn public_ipv6(db: &DatabaseConnection) -> Result<Ipv6Addr, String> {
    let ddns = cloudflare_ddns::load_settings(db)
        .await
        .map_err(|error| error.message().to_string())?;
    cloudflare_ddns::detect_ipv6(db, &ddns).await
}

/// Detected public addresses, looked up once per sync.
#[derive(Default)]
struct Addresses {
    ipv4: Option<Result<Ipv4Addr, String>>,
    ipv6: Option<Result<Ipv6Addr, String>>,
}

/// Records a mapping should have. An address that cannot be detected only
/// skips its own record type (reported in the second value), so e.g. a host
/// without public IPv4 still keeps its AAAA record current.
async fn desired_records(
    db: &DatabaseConnection,
    settings: &CertificateSettings,
    mapping: &domain_mappings::Model,
    addresses: &mut Addresses,
) -> (Vec<DesiredRecord>, Vec<String>) {
    let mut records = Vec::new();
    let mut errors = Vec::new();
    match mapping.ipv4_mode.as_str() {
        "manual" => records.push(DesiredRecord {
            record_type: "A",
            name: mapping.domain.clone(),
            content: mapping.ipv4_address.clone().unwrap_or_default(),
            proxied: false,
        }),
        "auto" => {
            if addresses.ipv4.is_none() {
                addresses.ipv4 = Some(public_ipv4(db, &settings.ipv4_lookup_url).await);
            }
            match addresses.ipv4.clone().unwrap_or_else(|| Err(String::new())) {
                Ok(address) => records.push(DesiredRecord {
                    record_type: "A",
                    name: mapping.domain.clone(),
                    content: address.to_string(),
                    proxied: false,
                }),
                Err(error) => {
                    errors.push(format!("{error}（没有公网 IPv4 时请把 IPv4 映射设为关闭）"));
                }
            }
        }
        _ => {}
    }
    match mapping.ipv6_mode.as_str() {
        "manual" => records.push(DesiredRecord {
            record_type: "AAAA",
            name: mapping.domain.clone(),
            content: mapping.ipv6_address.clone().unwrap_or_default(),
            proxied: false,
        }),
        "auto" => {
            if addresses.ipv6.is_none() {
                addresses.ipv6 = Some(public_ipv6(db).await);
            }
            match addresses.ipv6.clone().unwrap_or_else(|| Err(String::new())) {
                Ok(address) => records.push(DesiredRecord {
                    record_type: "AAAA",
                    name: mapping.domain.clone(),
                    content: address.to_string(),
                    proxied: false,
                }),
                Err(error) => errors.push(error),
            }
        }
        _ => {}
    }
    (records, errors)
}

async fn sync_direct_dns(
    db: &DatabaseConnection,
    settings: &CertificateSettings,
    cloudflare: &Result<CloudflareApi, String>,
    mappings: &[&domain_mappings::Model],
    force: bool,
) -> HashMap<i32, Option<String>> {
    let mut results = HashMap::new();
    let mut addresses = Addresses::default();
    for mapping in mappings {
        if mapping.ipv4_mode == "off" && mapping.ipv6_mode == "off" {
            continue;
        }
        let (records, mut failures) = desired_records(db, settings, mapping, &mut addresses).await;
        let print = fingerprint(&(
            records
                .iter()
                .map(|record| (record.record_type, &record.name, &record.content))
                .collect::<Vec<_>>(),
            &failures,
        ));
        let fresh = sync_state().lock().ok().and_then(|state| {
            state.dns.get(&mapping.id).and_then(|sync| {
                (sync.fingerprint == print && sync.at.elapsed() < VERIFY_INTERVAL)
                    .then(|| sync.error.clone())
            })
        });
        if let (false, Some(previous)) = (force, fresh) {
            results.insert(mapping.id, previous);
            continue;
        }
        let error = match cloudflare {
            Ok(cloudflare) => {
                for record in &records {
                    if let Err(error) = cloudflare.upsert_record(record).await {
                        failures.push(format!("{} 记录：{error}", record.record_type));
                    }
                }
                (!failures.is_empty()).then(|| format!("DNS：{}", failures.join("；")))
            }
            Err(error) => Some(format!("DNS：{error}")),
        };
        if let Ok(mut state) = sync_state().lock() {
            state.dns.insert(
                mapping.id,
                DnsSync {
                    fingerprint: print,
                    at: Instant::now(),
                    error: error.clone(),
                },
            );
        }
        if error.is_none() {
            let mut active = (*mapping).clone().into_active_model();
            active.synced_at = Set(Some(now()));
            let _ = active.update(db).await;
        }
        results.insert(mapping.id, error);
    }
    results
}

/// Ensures the tunnel exists, pushes ingress + CNAME records and runs cloudflared.
/// Returns an error per mapping id (or one shared error).
async fn sync_tunnel(
    db: &DatabaseConnection,
    settings: &mut CertificateSettings,
    cloudflare: &Result<CloudflareApi, String>,
    mappings: &[&domain_mappings::Model],
    force: bool,
) -> Result<HashMap<i32, Option<String>>, String> {
    if mappings.is_empty() {
        tunnel::apply(None).await;
        return Ok(HashMap::new());
    }
    if settings.cloudflare_account_id.is_empty() {
        tunnel::apply(None).await;
        return Err("使用 Cloudflare Tunnel 需要在设置中填写 Cloudflare Account ID".to_string());
    }
    let cloudflare = cloudflare.as_ref().map_err(Clone::clone)?;
    let ingress = tunnel_ingress(mappings);
    let print = fingerprint(&(
        &settings.tunnel_id,
        ingress.to_string(),
        &settings.cloudflare_account_id,
    ));
    let fresh = !force
        && !settings.tunnel_token.is_empty()
        && sync_state().lock().ok().is_some_and(|state| {
            state
                .tunnel
                .is_some_and(|(previous, at)| previous == print && at.elapsed() < VERIFY_INTERVAL)
        });

    let mut errors = HashMap::new();
    if !fresh {
        let account = settings.cloudflare_account_id.clone();
        if settings.tunnel_name.is_empty() {
            settings.tunnel_name = default_tunnel_name();
        }
        let exists = !settings.tunnel_id.is_empty()
            && cloudflare
                .tunnel_exists(&account, &settings.tunnel_id)
                .await?;
        if !exists {
            let tunnel = match cloudflare
                .find_tunnel(&account, &settings.tunnel_name)
                .await?
            {
                Some(tunnel) => tunnel,
                None => {
                    cloudflare
                        .create_tunnel(&account, &settings.tunnel_name)
                        .await?
                }
            };
            settings.tunnel_id = tunnel.id;
            settings.tunnel_token.clear();
        }
        if settings.tunnel_token.is_empty() {
            settings.tunnel_token = cloudflare
                .tunnel_token(&account, &settings.tunnel_id)
                .await?;
        }
        store_settings(db, settings, None)
            .await
            .map_err(|error| error.message().to_string())?;
        cloudflare
            .put_tunnel_config(&account, &settings.tunnel_id, &ingress)
            .await?;
        let target = format!("{}.cfargotunnel.com", settings.tunnel_id);
        for mapping in mappings {
            let error = cloudflare
                .upsert_record(&DesiredRecord {
                    record_type: "CNAME",
                    name: mapping.domain.clone(),
                    content: target.clone(),
                    proxied: true,
                })
                .await
                .err()
                .map(|error| format!("DNS：{error}"));
            if error.is_none() {
                let mut active = (*mapping).clone().into_active_model();
                active.synced_at = Set(Some(now()));
                let _ = active.update(db).await;
            }
            errors.insert(mapping.id, error);
        }
        let print = fingerprint(&(
            &settings.tunnel_id,
            ingress.to_string(),
            &settings.cloudflare_account_id,
        ));
        if let Ok(mut state) = sync_state().lock() {
            state.tunnel = Some((print, Instant::now()));
        }
    }

    if let Some(binary) = tunnel::find_binary(&settings.cloudflared_path) {
        tunnel::apply(Some((binary, settings.tunnel_token.clone()))).await;
        Ok(errors)
    } else {
        tunnel::apply(None).await;
        Err(
            "隧道配置已同步，但本机未找到 cloudflared：请安装 cloudflared 或在设置中填写其路径"
                .to_string(),
        )
    }
}

/// Loads the active certificates into the HTTPS proxy and routes the direct
/// mappings. Returns the names each ready certificate covers.
async fn apply_proxy(
    direct: &[&domain_mappings::Model],
    certificates: &[tls_certificates::Model],
) -> (Vec<Vec<String>>, Vec<proxy::ListenerStatus>) {
    let ready: Vec<proxy::ProxyCertificate> = certificates
        .iter()
        .filter(|certificate| certificate.status == "active")
        .filter_map(|certificate| {
            Some(proxy::ProxyCertificate {
                names: certificate_names(certificate),
                cert_path: PathBuf::from(certificate.cert_path.as_ref()?),
                key_path: PathBuf::from(certificate.key_path.as_ref()?),
            })
        })
        .collect();
    let routes: Vec<proxy::ProxyRoute> = direct
        .iter()
        .filter_map(|mapping| {
            Some(proxy::ProxyRoute {
                port: u16::try_from(mapping.public_port).ok()?,
                host: mapping.domain.clone(),
                upstream_host: mapping.upstream_host.clone(),
                upstream_port: u16::try_from(mapping.upstream_port).ok()?,
                allowed_ips: serde_json::from_str::<Vec<String>>(&mapping.allowed_ips)
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|value| IpRule::parse(&value))
                    .collect(),
            })
        })
        .collect();
    proxy::apply(routes, &ready).await;
    let names = ready
        .into_iter()
        .map(|certificate| certificate.names)
        .collect();
    (names, proxy::status().await)
}

/// Inputs for working out each mapping's status after a sync.
struct SyncOutcome<'a> {
    certificate_names: &'a [Vec<String>],
    listeners: &'a [proxy::ListenerStatus],
    dns_errors: &'a HashMap<i32, Option<String>>,
    tunnel: &'a Result<HashMap<i32, Option<String>>, String>,
    tunnel_running: bool,
}

fn mapping_status(
    mapping: &domain_mappings::Model,
    outcome: &SyncOutcome<'_>,
) -> (&'static str, Option<String>) {
    let mut problems: Vec<String> = Vec::new();
    let status = if !mapping.enabled {
        "disabled"
    } else if mapping.mode == MODE_TUNNEL {
        match outcome.tunnel {
            Ok(errors) => {
                if let Some(Some(error)) = errors.get(&mapping.id) {
                    problems.push(error.clone());
                }
                if !outcome.tunnel_running {
                    problems.push("cloudflared 正在启动或已退出，请查看运行日志".to_string());
                    "pending"
                } else if problems.is_empty() {
                    "active"
                } else {
                    "error"
                }
            }
            Err(error) => {
                problems.push(error.clone());
                "error"
            }
        }
    } else {
        if let Some(Some(error)) = outcome.dns_errors.get(&mapping.id) {
            problems.push(error.clone());
        }
        let covered = outcome
            .certificate_names
            .iter()
            .any(|names| certificate_covers(names, &mapping.domain));
        let listener_error = outcome
            .listeners
            .iter()
            .find(|listener| i32::from(listener.port) == mapping.public_port)
            .and_then(|listener| listener.error.clone());
        if !covered {
            problems.push("证书尚未签发，签发成功后自动开始 HTTPS 监听".to_string());
            "pending"
        } else if let Some(error) = listener_error {
            problems.push(error);
            "error"
        } else if problems.is_empty() {
            "active"
        } else {
            "error"
        }
    };
    (status, (!problems.is_empty()).then(|| problems.join("；")))
}

/// Brings listeners, DNS records, the tunnel and mapping statuses in line
/// with the database. `force` bypasses the "nothing changed" shortcuts.
pub async fn sync_all(db: &DatabaseConnection, force: bool) -> ApiResult<()> {
    let _guard = reconcile_lock().lock().await;
    let mut settings = load_settings(db).await?;
    let mappings = domain_mappings::Entity::find()
        .order_by_asc(domain_mappings::Column::Id)
        .all(db)
        .await?;
    let certificates = tls_certificates::Entity::find().all(db).await?;

    let enabled: Vec<&domain_mappings::Model> =
        mappings.iter().filter(|mapping| mapping.enabled).collect();
    let direct: Vec<&domain_mappings::Model> = enabled
        .iter()
        .copied()
        .filter(|mapping| mapping.mode == MODE_DIRECT)
        .collect();
    let tunneled: Vec<&domain_mappings::Model> = enabled
        .iter()
        .copied()
        .filter(|mapping| mapping.mode == MODE_TUNNEL)
        .collect();

    let needs_cloudflare = !tunneled.is_empty()
        || direct
            .iter()
            .any(|mapping| mapping.ipv4_mode != "off" || mapping.ipv6_mode != "off");
    let cloudflare = if needs_cloudflare {
        match cloudflare_credentials(db, &settings).await {
            Ok(credentials) => CloudflareApi::new(db, &credentials).await,
            Err(error) => Err(error.message().to_string()),
        }
    } else {
        Err("未配置 Cloudflare".to_string())
    };

    let (certificate_names, listeners) = apply_proxy(&direct, &certificates).await;

    let dns_errors = sync_direct_dns(db, &settings, &cloudflare, &direct, force).await;
    let tunnel_result = sync_tunnel(db, &mut settings, &cloudflare, &tunneled, force).await;
    let tunnel_running = tunnel::status(&settings.cloudflared_path).await.running;

    let outcome = SyncOutcome {
        certificate_names: &certificate_names,
        listeners: &listeners,
        dns_errors: &dns_errors,
        tunnel: &tunnel_result,
        tunnel_running,
    };
    for mapping in &mappings {
        let (status, last_error) = mapping_status(mapping, &outcome);
        if mapping.status != status || mapping.last_error != last_error {
            let mut active = mapping.clone().into_active_model();
            active.status = Set(status.to_string());
            active.last_error = Set(last_error);
            active.update(db).await?;
        }
    }
    Ok(())
}

/// Background loop: initial sync, periodic DNS/tunnel checks and renewals.
pub fn start(db: DatabaseConnection) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(10)).await;
        loop {
            if let Err(error) = sync_all(&db, false).await {
                tracing::warn!(error = ?error, "certificate/domain mapping sync failed");
            }
            if let Err(error) = renew_due_certificates(&db).await {
                tracing::warn!(error = ?error, "certificate renewal check failed");
            }
            tokio::time::sleep(SYNC_INTERVAL).await;
        }
    });
}

#[must_use]
pub fn storage_dir() -> &'static Path {
    Path::new(STORAGE_DIR)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapping(id: i32, domain: &str, host: &str, port: i32) -> domain_mappings::Model {
        domain_mappings::Model {
            created_at: now(),
            updated_at: now(),
            id,
            name: domain.to_string(),
            domain: domain.to_string(),
            mode: MODE_TUNNEL.to_string(),
            upstream_host: host.to_string(),
            upstream_port: port,
            public_port: 443,
            ipv4_mode: "off".to_string(),
            ipv4_address: None,
            ipv6_mode: "off".to_string(),
            ipv6_address: None,
            allowed_ips: "[]".to_string(),
            enabled: true,
            status: "pending".to_string(),
            last_error: None,
            synced_at: None,
            created_by: None,
            certificate_id: None,
        }
    }

    #[test]
    fn domains_are_validated_and_normalized() {
        assert_eq!(
            normalize_domain(" App.Example.COM. ", false).unwrap(),
            "app.example.com"
        );
        assert_eq!(
            normalize_domain("*.example.com", true).unwrap(),
            "*.example.com"
        );
        assert!(normalize_domain("*.example.com", false).is_err());
        assert_eq!(
            normalize_domain("中文.example.com", false).unwrap(),
            "xn--fiq228c.example.com"
        );
        for invalid in [
            "localhost",
            "example",
            "-bad.example.com",
            "a_b.example.com",
            "1.2.3.4",
            "exa mple.com",
            "",
        ] {
            assert!(normalize_domain(invalid, false).is_err(), "{invalid}");
        }
    }

    #[test]
    fn upstreams_ports_and_addresses_are_validated() {
        assert_eq!(validate_upstream_host("127.0.0.1").unwrap(), "127.0.0.1");
        assert_eq!(validate_upstream_host("[::1]").unwrap(), "::1");
        assert_eq!(validate_upstream_host("LocalHost").unwrap(), "localhost");
        assert!(validate_upstream_host("bad host").is_err());
        assert_eq!(validate_port(443, "端口").unwrap(), 443);
        assert!(validate_port(0, "端口").is_err());
        assert!(validate_port(70_000, "端口").is_err());
        assert_eq!(
            normalize_ip_mode("manual", Some("203.0.113.5"), false).unwrap(),
            ("manual".to_string(), Some("203.0.113.5".to_string()))
        );
        assert!(normalize_ip_mode("manual", Some("::1"), false).is_err());
        assert!(normalize_ip_mode("manual", Some("2001:db8::1"), true).is_ok());
        assert_eq!(normalize_ip_mode("", None, true).unwrap().0, "off");
        assert!(normalize_ip_mode("sometimes", None, true).is_err());
    }

    #[test]
    fn urls_coverage_and_renewal_windows() {
        assert_eq!(
            public_url("a.example.com", MODE_DIRECT, 443),
            "https://a.example.com"
        );
        assert_eq!(
            public_url("a.example.com", MODE_DIRECT, 8443),
            "https://a.example.com:8443"
        );
        assert_eq!(
            public_url("a.example.com", MODE_TUNNEL, 8443),
            "https://a.example.com"
        );

        let names = vec!["example.com".to_string(), "*.example.com".to_string()];
        assert!(certificate_covers(&names, "example.com"));
        assert!(certificate_covers(&names, "App.example.com"));
        assert!(!certificate_covers(&names, "a.b.example.com"));
        assert!(!certificate_covers(&names, "example.org"));

        let now = now();
        assert!(renewal_due(None, now, 30));
        assert!(renewal_due(Some(now + chrono::Duration::days(29)), now, 30));
        assert!(!renewal_due(
            Some(now + chrono::Duration::days(31)),
            now,
            30
        ));
    }

    #[test]
    fn tunnel_ingress_ends_with_a_catch_all() {
        let first = mapping(1, "b.example.com", "127.0.0.1", 3000);
        let second = mapping(2, "a.example.com", "::1", 8080);
        let ingress = tunnel_ingress(&[&first, &second]);
        assert_eq!(
            ingress,
            serde_json::json!({ "ingress": [
                { "hostname": "a.example.com", "service": "http://[::1]:8080" },
                { "hostname": "b.example.com", "service": "http://127.0.0.1:3000" },
                { "service": "http_status:404" },
            ]})
        );
    }

    #[test]
    fn settings_view_hides_secrets() {
        let settings = CertificateSettings {
            cloudflare_api_token: "secret".to_string(),
            tunnel_token: "tunnel-secret".to_string(),
            ..Default::default()
        };
        let json = serde_json::to_string(&settings_view(&settings)).unwrap();
        assert!(!json.contains("secret"));
        assert!(json.contains(r#""cloudflare_api_token_set":true"#));
        assert!(default_tunnel_name().starts_with("gpt-images-"));
    }
}
