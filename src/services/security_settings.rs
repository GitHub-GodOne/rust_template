#![allow(clippy::missing_errors_doc)]
//! `security.*` settings read on hot paths (every API request), cached for a
//! few seconds and dropped whenever a setting is saved.

use std::{
    net::IpAddr,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use sea_orm::{DatabaseConnection, DbErr, EntityTrait};

use super::{
    client_info::is_local,
    ip_rules::{parse_rules, IpRule},
};
use crate::{
    errors::{ApiError, ApiResult},
    models::_entities::system_settings,
};

pub const REAUTH_ENABLED: &str = "security.reauth_enabled";
pub const REMOTE_REAUTH_ENABLED: &str = "security.remote_reauth_enabled";
pub const REAUTH_MINUTES: &str = "security.reauth_minutes";
pub const ADMIN_IP_ALLOWLIST: &str = "security.admin_ip_allowlist";
pub const ADMIN_IP_BLOCKLIST: &str = "security.admin_ip_blocklist";
pub const API_DOCS_ACCESS: &str = "security.api_docs_access";
pub const LOGIN_CAPTCHA: &str = "security.login_captcha";

const CACHE_TTL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocsAccess {
    Public,
    Login,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptchaMode {
    Off,
    AfterFailures,
    Always,
}

#[derive(Debug, Clone)]
pub struct SecuritySettings {
    pub reauth_enabled: bool,
    /// SSH/VNC/RDP connections also ask for the password (off by default).
    pub remote_reauth_enabled: bool,
    pub reauth_minutes: i64,
    pub admin_ip_allowlist: Vec<IpRule>,
    pub admin_ip_blocklist: Vec<IpRule>,
    pub docs_access: DocsAccess,
    pub login_captcha: CaptchaMode,
}

impl Default for SecuritySettings {
    fn default() -> Self {
        Self {
            reauth_enabled: true,
            remote_reauth_enabled: false,
            reauth_minutes: 10,
            admin_ip_allowlist: Vec::new(),
            admin_ip_blocklist: Vec::new(),
            docs_access: DocsAccess::Login,
            login_captcha: CaptchaMode::AfterFailures,
        }
    }
}

impl SecuritySettings {
    /// True when the list is empty, the caller is this machine, or matches.
    #[must_use]
    pub fn ip_allowed(&self, ip: Option<IpAddr>) -> bool {
        if self.ip_blocked(ip) {
            return false;
        }
        if self.admin_ip_allowlist.is_empty() {
            return true;
        }
        ip.is_some_and(|ip| {
            is_local(ip) || self.admin_ip_allowlist.iter().any(|rule| rule.contains(ip))
        })
    }

    #[must_use]
    pub fn ip_blocked(&self, ip: Option<IpAddr>) -> bool {
        ip.is_some_and(|ip| self.admin_ip_blocklist.iter().any(|rule| rule.contains(ip)))
    }
}

static CACHE: OnceLock<Mutex<Option<(Instant, SecuritySettings)>>> = OnceLock::new();

fn cache() -> &'static Mutex<Option<(Instant, SecuritySettings)>> {
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Forgets the cached values; call after any setting changes.
pub fn invalidate() {
    if let Ok(mut cached) = cache().lock() {
        *cached = None;
    }
}

pub async fn load(db: &DatabaseConnection) -> Result<SecuritySettings, DbErr> {
    if let Ok(cached) = cache().lock() {
        if let Some((loaded_at, settings)) = cached.as_ref() {
            if loaded_at.elapsed() < CACHE_TTL {
                return Ok(settings.clone());
            }
        }
    }
    let rows = system_settings::Entity::find().all(db).await?;
    let value = |key: &str| {
        rows.iter()
            .find(|row| row.key == key)
            .map(|row| row.value.trim().to_string())
    };
    let defaults = SecuritySettings::default();
    let settings = SecuritySettings {
        reauth_enabled: value(REAUTH_ENABLED).map_or(defaults.reauth_enabled, |v| v != "false"),
        remote_reauth_enabled: value(REMOTE_REAUTH_ENABLED)
            .map_or(defaults.remote_reauth_enabled, |v| v == "true"),
        reauth_minutes: value(REAUTH_MINUTES)
            .and_then(|v| v.parse::<i64>().ok())
            .map_or(defaults.reauth_minutes, |minutes| minutes.clamp(1, 24 * 60)),
        admin_ip_allowlist: value(ADMIN_IP_ALLOWLIST)
            .map(|v| parse_rules(&v).0)
            .unwrap_or_default(),
        admin_ip_blocklist: value(ADMIN_IP_BLOCKLIST)
            .map(|v| parse_rules(&v).0)
            .unwrap_or_default(),
        docs_access: value(API_DOCS_ACCESS)
            .as_deref()
            .and_then(parse_docs_access)
            .unwrap_or(defaults.docs_access),
        login_captcha: value(LOGIN_CAPTCHA)
            .as_deref()
            .and_then(parse_captcha_mode)
            .unwrap_or(defaults.login_captcha),
    };
    if let Ok(mut cached) = cache().lock() {
        *cached = Some((Instant::now(), settings.clone()));
    }
    Ok(settings)
}

fn parse_docs_access(value: &str) -> Option<DocsAccess> {
    match value {
        "public" => Some(DocsAccess::Public),
        "login" => Some(DocsAccess::Login),
        "disabled" => Some(DocsAccess::Disabled),
        _ => None,
    }
}

fn parse_captcha_mode(value: &str) -> Option<CaptchaMode> {
    match value {
        "off" => Some(CaptchaMode::Off),
        "after_failures" => Some(CaptchaMode::AfterFailures),
        "always" => Some(CaptchaMode::Always),
        _ => None,
    }
}

/// Rejects values the security features cannot use, and an IP allowlist that
/// would lock out the administrator saving it.
pub fn validate_setting(key: &str, value: &str, editor_ip: Option<IpAddr>) -> ApiResult<()> {
    let value = value.trim();
    match key {
        ADMIN_IP_ALLOWLIST => {
            let (rules, invalid) = parse_rules(value);
            if !invalid.is_empty() {
                return Err(ApiError::bad_request(format!(
                    "IP 白名单中有无法识别的地址：{}",
                    invalid.join(", ")
                )));
            }
            let candidate = SecuritySettings {
                admin_ip_allowlist: rules,
                ..SecuritySettings::default()
            };
            if !candidate.ip_allowed(editor_ip) {
                return Err(ApiError::bad_request(format!(
                    "IP 白名单没有包含你当前的地址 {}，保存后你会被挡在后台之外",
                    editor_ip.map_or_else(|| "未知".to_string(), |ip| ip.to_string())
                )));
            }
            Ok(())
        }
        ADMIN_IP_BLOCKLIST => {
            let (rules, invalid) = parse_rules(value);
            if !invalid.is_empty() {
                return Err(ApiError::bad_request(format!(
                    "IP 黑名单中有无法识别的地址：{}",
                    invalid.join(", ")
                )));
            }
            if rules
                .iter()
                .any(|rule| editor_ip.is_some_and(|ip| rule.contains(ip)))
            {
                return Err(ApiError::bad_request(format!(
                    "当前 IP {} 在黑名单中，保存后你会被挡在后台之外",
                    editor_ip.map_or_else(|| "未知".to_string(), |ip| ip.to_string())
                )));
            }
            Ok(())
        }
        API_DOCS_ACCESS if parse_docs_access(value).is_none() => Err(ApiError::bad_request(
            "接口文档访问只能是 public、login 或 disabled",
        )),
        LOGIN_CAPTCHA if parse_captcha_mode(value).is_none() => Err(ApiError::bad_request(
            "登录验证码只能是 off、after_failures 或 always",
        )),
        "ssh_manager.idle_timeout_minutes" | "ssh_manager.max_session_hours"
            if !value.parse::<i64>().is_ok_and(|number| number >= 0) =>
        {
            Err(ApiError::bad_request("请填写不小于 0 的整数，0 表示不限制"))
        }
        "ssh_manager.scrollback_lines"
            if !value
                .parse::<i64>()
                .is_ok_and(|number| (0..=100_000).contains(&number)) =>
        {
            Err(ApiError::bad_request(
                "请填写 0 到 100000 之间的整数，0 表示不限制",
            ))
        }
        REAUTH_MINUTES
            if !value
                .parse::<i64>()
                .is_ok_and(|minutes| (1..=24 * 60).contains(&minutes)) =>
        {
            Err(ApiError::bad_request(
                "二次确认有效分钟数必须在 1 到 1440 之间",
            ))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        validate_setting, ADMIN_IP_ALLOWLIST, ADMIN_IP_BLOCKLIST, API_DOCS_ACCESS, REAUTH_MINUTES,
    };

    #[test]
    fn ssh_scrollback_accepts_zero_up_to_the_browser_ceiling() {
        let key = "ssh_manager.scrollback_lines";
        for ok in ["0", "1", "5000", "100000"] {
            assert!(validate_setting(key, ok, None).is_ok(), "{ok}");
        }
        for bad in ["-1", "100001", "abc", "1.5"] {
            assert!(validate_setting(key, bad, None).is_err(), "{bad}");
        }
    }

    #[test]
    fn allowlist_must_keep_the_editor_inside() {
        let editor = Some("203.0.113.8".parse().unwrap());
        assert!(validate_setting(ADMIN_IP_ALLOWLIST, "203.0.113.0/24", editor).is_ok());
        assert!(validate_setting(ADMIN_IP_ALLOWLIST, "198.51.100.1", editor).is_err());
        assert!(validate_setting(ADMIN_IP_ALLOWLIST, "", editor).is_ok());
        assert!(validate_setting(ADMIN_IP_ALLOWLIST, "nope", editor).is_err());
        let local = Some("127.0.0.1".parse().unwrap());
        assert!(validate_setting(ADMIN_IP_ALLOWLIST, "198.51.100.1", local).is_ok());
    }

    #[test]
    fn enumerated_settings_are_checked() {
        assert!(validate_setting(API_DOCS_ACCESS, "login", None).is_ok());
        assert!(validate_setting(API_DOCS_ACCESS, "everyone", None).is_err());
        assert!(validate_setting(REAUTH_MINUTES, "0", None).is_err());
        assert!(validate_setting(REAUTH_MINUTES, "15", None).is_ok());
    }

    #[test]
    fn blocklist_rejects_the_editor_ip() {
        let editor = Some("203.0.113.8".parse().unwrap());
        assert!(validate_setting(ADMIN_IP_BLOCKLIST, "198.51.100.1", editor).is_ok());
        assert!(validate_setting(ADMIN_IP_BLOCKLIST, "203.0.113.8", editor).is_err());
        assert!(validate_setting(ADMIN_IP_BLOCKLIST, "nope", editor).is_err());
    }
}
