#![allow(clippy::missing_errors_doc)]

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Component, Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant, SystemTime},
};

use base64::Engine as _;
use loco_rs::{app::AppContext, hash};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter, Set};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;

use crate::{
    errors::{ApiError, ApiResult},
    models::_entities::system_settings,
};

pub const SITES_SETTING_KEY: &str = "static_hosting.sites";
const CACHE_TTL: Duration = Duration::from_secs(5);
const RESERVED_PUBLIC_PREFIXES: [&str; 7] = [
    "/api",
    "/admin",
    "/login",
    "/public",
    "/api-docs",
    "/swagger-ui",
    "/static",
];
const DENIED_PATH_SEGMENTS: [&str; 5] = [".git", ".svn", ".hg", ".env", ".ssh"];
const MAX_ACCESS_RULES: usize = 50;
const MAX_ACCESS_USERNAME_CHARS: usize = 64;
pub const MIN_ACCESS_PASSWORD_CHARS: usize = 4;
/// 已验证的访问凭据缓存时长，避免同一页面的每个资源都重新计算 argon2。
const ACCESS_CACHE_TTL: Duration = Duration::from_mins(10);
const MAX_ACCESS_CACHE_ENTRIES: usize = 1024;

// Independent on/off switches persisted as JSON; an enum would break saved configs.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
pub struct StaticSiteConfig {
    pub key: String,
    pub name: String,
    pub url_path: String,
    pub local_root: String,
    #[serde(default = "default_index_file")]
    pub index_file: String,
    #[serde(default)]
    pub spa_fallback: bool,
    /// 目录中没有首页文件时，是否生成文件列表页面。
    #[serde(default)]
    pub directory_listing: bool,
    /// 开启目录列表后，即使目录中存在首页文件也优先显示文件列表。
    #[serde(default)]
    pub listing_overrides_index: bool,
    /// 需要用户名和密码才能访问的目录，按最具体的目录匹配。
    #[serde(default)]
    pub access_rules: Vec<StaticAccessRule>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_cache_seconds")]
    pub cache_seconds: u32,
}

/// 目录访问保护规则，公开访问时使用 HTTP Basic 认证。
#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
pub struct StaticAccessRule {
    /// 站点内的相对目录，空字符串表示整个站点。
    #[serde(default)]
    pub path: String,
    pub username: String,
    /// argon2 密码哈希；只保存在配置中，接口返回前会被清空。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub password_hash: String,
}

/// 公开请求解析出的目标：具体文件，或需要生成列表的目录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublicTarget {
    File(PathBuf),
    Directory { path: PathBuf, relative: String },
}

#[derive(Debug, Clone)]
pub struct DirectoryListingEntry {
    pub name: String,
    pub relative: String,
    pub is_dir: bool,
    pub size_bytes: u64,
    pub modified: Option<SystemTime>,
}

pub struct DirectoryListing {
    pub entries: Vec<DirectoryListingEntry>,
    pub truncated: bool,
}

const MAX_LISTING_ENTRIES: usize = 5000;

struct CachedSites {
    loaded_at: Instant,
    sites: Vec<StaticSiteConfig>,
}

static SITE_CACHE: OnceLock<RwLock<Option<CachedSites>>> = OnceLock::new();

fn default_index_file() -> String {
    "index.html".to_string()
}

const fn default_enabled() -> bool {
    true
}

const fn default_cache_seconds() -> u32 {
    300
}

pub async fn load_sites(ctx: &AppContext) -> ApiResult<Vec<StaticSiteConfig>> {
    let setting = system_settings::Entity::find()
        .filter(system_settings::Column::Key.eq(SITES_SETTING_KEY))
        .one(&ctx.db)
        .await?;
    let value = setting.map_or_else(|| "[]".to_string(), |setting| setting.value);
    let sites = serde_json::from_str::<Vec<StaticSiteConfig>>(&value)
        .map_err(|_| ApiError::bad_request("静态托管站点配置不是有效 JSON"))?;
    normalize_sites(sites)
}

pub async fn load_cached_sites(ctx: &AppContext) -> ApiResult<Vec<StaticSiteConfig>> {
    let cache = SITE_CACHE.get_or_init(|| RwLock::new(None));
    {
        let cached = cache.read().await;
        if let Some(cached) = cached.as_ref() {
            if cached.loaded_at.elapsed() < CACHE_TTL {
                return Ok(cached.sites.clone());
            }
        }
    }

    let sites = load_sites(ctx).await?;
    *cache.write().await = Some(CachedSites {
        loaded_at: Instant::now(),
        sites: sites.clone(),
    });
    Ok(sites)
}

pub async fn save_sites(
    ctx: &AppContext,
    sites: Vec<StaticSiteConfig>,
    user_id: i32,
) -> ApiResult<Vec<StaticSiteConfig>> {
    let sites = normalize_sites(sites)?;
    let value =
        serde_json::to_string(&sites).map_err(|_| ApiError::internal("序列化静态托管配置失败"))?;
    let existing = system_settings::Entity::find()
        .filter(system_settings::Column::Key.eq(SITES_SETTING_KEY))
        .one(&ctx.db)
        .await?;

    if let Some(existing) = existing {
        let mut active = existing.into_active_model();
        active.value = Set(value);
        active.updated_by = Set(Some(user_id));
        active.update(&ctx.db).await?;
    } else {
        system_settings::ActiveModel {
            key: Set(SITES_SETTING_KEY.to_string()),
            name: Set("静态托管站点".to_string()),
            group_key: Set("static_hosting".to_string()),
            value: Set(value),
            value_type: Set("json".to_string()),
            default_value: Set(Some("[]".to_string())),
            description: Set(Some("URL 路径与本机静态站点目录映射".to_string())),
            is_public: Set(false),
            is_builtin: Set(true),
            is_encrypted: Set(false),
            sort_order: Set(185),
            created_by: Set(Some(user_id)),
            updated_by: Set(Some(user_id)),
            ..Default::default()
        }
        .insert(&ctx.db)
        .await?;
    }

    invalidate_cache().await;
    Ok(sites)
}

pub async fn invalidate_cache() {
    if let Some(cache) = SITE_CACHE.get() {
        *cache.write().await = None;
    }
}

pub fn normalize_site(mut site: StaticSiteConfig) -> ApiResult<StaticSiteConfig> {
    site.key = site.key.trim().to_ascii_lowercase();
    site.name = site.name.trim().to_string();
    site.url_path = normalize_url_path(&site.url_path)?;
    site.local_root = site.local_root.trim().to_string();
    site.index_file = normalize_required_relative_path(&site.index_file)?;
    site.cache_seconds = site.cache_seconds.min(31_536_000);

    if site.key.is_empty()
        || !site
            .key
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '-' | '_'))
    {
        return Err(ApiError::bad_request(
            "站点标识只能包含小写字母、数字、短横线和下划线",
        ));
    }
    if site.name.is_empty() {
        return Err(ApiError::bad_request("站点名称不能为空"));
    }
    if site.local_root.is_empty() {
        return Err(ApiError::bad_request("服务器目录不能为空"));
    }
    if site.url_path != "/" && is_reserved_request_path(&site.url_path) {
        return Err(ApiError::bad_request("URL 路径与后台保留路径冲突"));
    }
    normalize_access_rules(&mut site.access_rules)?;
    Ok(site)
}

fn normalize_access_rules(rules: &mut [StaticAccessRule]) -> ApiResult<()> {
    if rules.len() > MAX_ACCESS_RULES {
        return Err(ApiError::bad_request(format!(
            "每个站点最多设置 {MAX_ACCESS_RULES} 个受保护目录"
        )));
    }
    let mut paths = HashSet::new();
    for rule in rules.iter_mut() {
        rule.path = normalize_relative_path(Some(&rule.path))?;
        rule.username = rule.username.trim().to_string();
        if rule.username.is_empty()
            || rule.username.contains(':')
            || rule.username.chars().any(char::is_control)
            || rule.username.chars().count() > MAX_ACCESS_USERNAME_CHARS
        {
            return Err(ApiError::bad_request(format!(
                "访问用户名不能为空、不能包含冒号，且不超过 {MAX_ACCESS_USERNAME_CHARS} 个字符"
            )));
        }
        if rule.password_hash.is_empty() {
            return Err(ApiError::bad_request(format!(
                "请为受保护目录「{}」设置访问密码",
                access_rule_label(&rule.path)
            )));
        }
        if !paths.insert(rule.path.to_lowercase()) {
            return Err(ApiError::bad_request(format!(
                "受保护目录「{}」重复",
                access_rule_label(&rule.path)
            )));
        }
    }
    rules.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(())
}

#[must_use]
pub const fn access_rule_label(path: &str) -> &str {
    if path.is_empty() {
        "整个站点"
    } else {
        path
    }
}

/// 生成访问密码哈希。
pub fn hash_access_password(password: &str) -> ApiResult<String> {
    if password.chars().count() < MIN_ACCESS_PASSWORD_CHARS {
        return Err(ApiError::bad_request(format!(
            "访问密码至少 {MIN_ACCESS_PASSWORD_CHARS} 位"
        )));
    }
    hash::hash_password(password).map_err(|_| ApiError::internal("生成访问密码失败"))
}

/// 去掉密码哈希，供管理接口返回。
#[must_use]
pub fn redact_site(mut site: StaticSiteConfig) -> StaticSiteConfig {
    for rule in &mut site.access_rules {
        rule.password_hash.clear();
    }
    site
}

pub fn normalize_sites(sites: Vec<StaticSiteConfig>) -> ApiResult<Vec<StaticSiteConfig>> {
    let mut normalized = sites
        .into_iter()
        .map(normalize_site)
        .collect::<ApiResult<Vec<_>>>()?;
    let mut keys = HashSet::new();
    let mut url_paths = HashSet::new();
    for site in &normalized {
        if !keys.insert(site.key.clone()) {
            return Err(ApiError::bad_request("静态托管站点标识不能重复"));
        }
        if !url_paths.insert(site.url_path.clone()) {
            return Err(ApiError::bad_request("静态托管 URL 路径不能重复"));
        }
    }
    normalized.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(normalized)
}

pub fn prepare_site_root(site: &StaticSiteConfig) -> ApiResult<PathBuf> {
    fs::create_dir_all(&site.local_root).map_err(|_| ApiError::internal("创建静态托管目录失败"))?;
    canonical_site_root(site)
}

pub fn canonical_site_root(site: &StaticSiteConfig) -> ApiResult<PathBuf> {
    let root = fs::canonicalize(&site.local_root)
        .map_err(|_| ApiError::bad_request("静态托管目录不存在"))?;
    if root.parent().is_none() {
        return Err(ApiError::bad_request("不能将文件系统根目录设为托管目录"));
    }
    if !root.is_dir() {
        return Err(ApiError::bad_request("静态托管路径不是目录"));
    }
    Ok(root)
}

pub fn normalize_relative_path(path: Option<&str>) -> ApiResult<String> {
    let Some(path) = path.map(str::trim).filter(|path| !path.is_empty()) else {
        return Ok(String::new());
    };
    let path = path.trim_matches('/');
    if path.is_empty() {
        return Ok(String::new());
    }
    validate_relative_path(path)?;
    Ok(path.to_string())
}

pub fn normalize_required_relative_path(path: &str) -> ApiResult<String> {
    let path = normalize_relative_path(Some(path))?;
    if path.is_empty() {
        return Err(ApiError::bad_request("文件路径不能为空"));
    }
    Ok(path)
}

pub fn resolve_existing_path(site: &StaticSiteConfig, relative: &str) -> ApiResult<PathBuf> {
    let root = canonical_site_root(site)?;
    let target = if relative.is_empty() {
        root.clone()
    } else {
        root.join(relative)
    };
    let target = fs::canonicalize(target).map_err(|_| ApiError::bad_request("文件路径不存在"))?;
    ensure_under_root(&root, &target)?;
    Ok(target)
}

pub fn resolve_write_path(site: &StaticSiteConfig, relative: &str) -> ApiResult<PathBuf> {
    let relative = normalize_required_relative_path(relative)?;
    let root = prepare_site_root(site)?;
    let relative_path = Path::new(&relative);
    let file_name = relative_path
        .file_name()
        .ok_or_else(|| ApiError::bad_request("文件路径无效"))?;
    let parent = relative_path.parent().unwrap_or_else(|| Path::new(""));
    let mut current = root.clone();

    for component in parent.components() {
        let Component::Normal(segment) = component else {
            return Err(ApiError::bad_request("文件路径无效"));
        };
        let next = current.join(segment);
        if next.exists() {
            let canonical =
                fs::canonicalize(&next).map_err(|_| ApiError::bad_request("文件目录不存在"))?;
            ensure_under_root(&root, &canonical)?;
            if !canonical.is_dir() {
                return Err(ApiError::bad_request("文件路径中的父级不是目录"));
            }
            current = canonical;
        } else {
            fs::create_dir(&next).map_err(|_| ApiError::internal("创建上传目录失败"))?;
            current =
                fs::canonicalize(&next).map_err(|_| ApiError::internal("读取上传目录失败"))?;
            ensure_under_root(&root, &current)?;
        }
    }

    Ok(current.join(file_name))
}

#[must_use]
pub fn match_site<'a>(
    sites: &'a [StaticSiteConfig],
    request_path: &str,
) -> Option<(&'a StaticSiteConfig, String)> {
    sites
        .iter()
        .filter(|site| site.enabled)
        .filter_map(|site| {
            matched_relative_path(&site.url_path, request_path).map(|relative| (site, relative))
        })
        .max_by_key(|(site, _)| site.url_path.len())
}

/// 解析公开访问路径：优先返回文件，其次目录首页；
/// 目录没有首页（或设置了列表优先）且开启了目录列表时返回目录目标，最后才走 SPA 回退。
pub fn resolve_public_target(
    site: &StaticSiteConfig,
    relative: &str,
    allow_spa_fallback: bool,
) -> ApiResult<Option<PublicTarget>> {
    let root = canonical_site_root(site)?;
    let relative = normalize_relative_path(Some(relative))?;
    let requested = if relative.is_empty() {
        root.clone()
    } else {
        root.join(&relative)
    };

    if let Some(file) = canonical_file_under_root(&root, &requested)? {
        return Ok(Some(PublicTarget::File(file)));
    }
    if let Some(directory) = canonical_dir_under_root(&root, &requested)? {
        let listing_first = site.directory_listing && site.listing_overrides_index;
        if !listing_first {
            if let Some(file) = canonical_file_under_root(&root, &directory.join(&site.index_file))?
            {
                return Ok(Some(PublicTarget::File(file)));
            }
        }
        if site.directory_listing {
            return Ok(Some(PublicTarget::Directory {
                path: directory,
                relative,
            }));
        }
    }
    if site.spa_fallback && allow_spa_fallback {
        return Ok(
            canonical_file_under_root(&root, &root.join(&site.index_file))?.map(PublicTarget::File),
        );
    }
    Ok(None)
}

/// 读取目录内容用于公开列表：跳过隐藏文件和符号链接，目录在前、按名称排序。
pub fn list_public_directory(directory: &Path, relative: &str) -> ApiResult<DirectoryListing> {
    let reader = fs::read_dir(directory).map_err(|_| ApiError::internal("读取托管目录失败"))?;
    let mut entries = Vec::new();
    for entry in reader {
        let Ok(entry) = entry else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || validate_relative_path(&name).is_err() {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_dir() && !metadata.is_file() {
            continue;
        }
        let relative = if relative.is_empty() {
            name.clone()
        } else {
            format!("{relative}/{name}")
        };
        entries.push(DirectoryListingEntry {
            name,
            relative,
            is_dir: metadata.is_dir(),
            size_bytes: if metadata.is_file() {
                metadata.len()
            } else {
                0
            },
            modified: metadata.modified().ok(),
        });
    }
    entries.sort_by(|left, right| {
        right
            .is_dir
            .cmp(&left.is_dir)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    let truncated = entries.len() > MAX_LISTING_ENTRIES;
    entries.truncate(MAX_LISTING_ENTRIES);
    Ok(DirectoryListing { entries, truncated })
}

/// 生成目录列表 HTML 页面，所有链接均为站点绝对路径。
#[must_use]
pub fn render_directory_listing(
    site: &StaticSiteConfig,
    relative: &str,
    listing: &DirectoryListing,
) -> String {
    use std::fmt::Write as _;

    let current_url = public_dir_url(site, relative);
    let title = escape_html(&percent_decode_path(&current_url).unwrap_or(current_url));
    let rows = render_listing_rows(site, relative, listing);
    let summary = {
        let directories = listing.entries.iter().filter(|entry| entry.is_dir).count();
        let files = listing.entries.len() - directories;
        let mut summary = format!("{directories} 个目录 · {files} 个文件");
        if listing.truncated {
            let _ = write!(summary, " · 仅显示前 {MAX_LISTING_ENTRIES} 项");
        }
        summary
    };
    let index_notice = listing
        .entries
        .iter()
        .find(|entry| !entry.is_dir && entry.name == site.index_file)
        .map_or_else(String::new, |entry| {
            format!(
                r#"<p class="notice">此目录包含首页文件，<a href="{}">打开 {}</a></p>"#,
                escape_html(&public_file_url(site, &entry.relative)),
                escape_html(&entry.name)
            )
        });

    format!(
        r#"<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>目录 {title}</title>
<style>
:root {{ color-scheme: light dark; --bg:#f6f4f0; --card:#fff; --text:#2b2723; --muted:#857c72; --line:#e8e2da; --hover:#faf6f1; --accent:#b45f3c; }}
@media (prefers-color-scheme: dark) {{ :root {{ --bg:#1e1c1a; --card:#262320; --text:#ece6de; --muted:#a39a90; --line:#38332e; --hover:#2e2a26; --accent:#e08a63; }} }}
* {{ box-sizing: border-box; }}
body {{ margin:0; padding:32px 16px; background:var(--bg); color:var(--text); font:14px/1.5 -apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC", "Noto Sans SC", sans-serif; }}
main {{ max-width:960px; margin:0 auto; }}
h1 {{ margin:0 0 4px; font-size:18px; font-weight:600; word-break:break-all; }}
p {{ margin:0 0 16px; color:var(--muted); font-size:12px; }}
.card {{ overflow:hidden; background:var(--card); border:1px solid var(--line); border-radius:10px; }}
table {{ width:100%; border-collapse:collapse; }}
th, td {{ padding:9px 14px; text-align:left; border-bottom:1px solid var(--line); }}
th {{ color:var(--muted); font-size:12px; font-weight:500; }}
tr:last-child td {{ border-bottom:0; }}
tbody tr:hover {{ background:var(--hover); }}
td a {{ display:flex; align-items:center; gap:8px; color:inherit; text-decoration:none; word-break:break-all; }}
td a:hover {{ color:var(--accent); }}
.icon {{ width:18px; flex:0 0 18px; text-align:center; }}
.num {{ width:1%; white-space:nowrap; color:var(--muted); font-variant-numeric:tabular-nums; }}
.up a {{ color:var(--muted); }}
.empty {{ padding:32px 14px; text-align:center; color:var(--muted); }}
.lock {{ margin-left:2px; font-size:12px; }}
.notice {{ margin:-8px 0 16px; }}
.notice a {{ color:var(--accent); }}
@media (max-width:600px) {{ body {{ padding:20px 12px; }} th:nth-child(3), td:nth-child(3) {{ display:none; }} }}
</style>
</head>
<body>
<main>
<h1>{title}</h1>
<p>{summary}</p>
{index_notice}
<div class="card">
<table>
<thead><tr><th>名称</th><th class="num">大小</th><th class="num">修改时间</th></tr></thead>
<tbody>{rows}</tbody>
</table>
</div>
</main>
</body>
</html>
"#
    )
}

fn render_listing_rows(
    site: &StaticSiteConfig,
    relative: &str,
    listing: &DirectoryListing,
) -> String {
    use std::fmt::Write as _;

    let current_rule = protecting_rule(site, relative, None).map(|rule| rule.path.as_str());
    let mut rows = String::new();
    if !relative.is_empty() {
        let parent = relative.rsplit_once('/').map_or("", |(parent, _)| parent);
        let _ = write!(
            rows,
            r#"<tr class="up"><td><a href="{}"><span class="icon">↩</span>返回上级</a></td><td></td><td></td></tr>"#,
            escape_html(&public_dir_url(site, parent))
        );
    }
    for entry in &listing.entries {
        let (href, icon, size_label) = if entry.is_dir {
            (public_dir_url(site, &entry.relative), "📁", "—".to_string())
        } else {
            (
                public_file_url(site, &entry.relative),
                "📄",
                format_size(entry.size_bytes),
            )
        };
        let modified = entry.modified.map_or_else(
            || "—".to_string(),
            |time| {
                chrono::DateTime::<chrono::Local>::from(time)
                    .format("%Y-%m-%d %H:%M")
                    .to_string()
            },
        );
        // 只在进入新的受保护目录时标记，当前目录本身已受保护时不重复显示。
        let entry_rule =
            protecting_rule(site, &entry.relative, None).map(|rule| rule.path.as_str());
        let lock = if entry_rule.is_some() && entry_rule != current_rule {
            r#"<span class="lock" title="需要密码访问">🔒</span>"#
        } else {
            ""
        };
        let _ = write!(
            rows,
            r#"<tr><td><a href="{href}"><span class="icon">{icon}</span>{name}{slash}{lock}</a></td><td class="num">{size_label}</td><td class="num">{modified}</td></tr>"#,
            href = escape_html(&href),
            name = escape_html(&entry.name),
            slash = if entry.is_dir { "/" } else { "" },
        );
    }
    if listing.entries.is_empty() {
        rows.push_str(r#"<tr><td colspan="3" class="empty">此目录为空</td></tr>"#);
    }
    rows
}

fn public_dir_url(site: &StaticSiteConfig, relative: &str) -> String {
    let url = public_file_url(site, relative);
    if url.ends_with('/') {
        url
    } else {
        format!("{url}/")
    }
}

fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

#[allow(clippy::cast_precision_loss)]
fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

#[must_use]
pub fn is_reserved_request_path(path: &str) -> bool {
    RESERVED_PUBLIC_PREFIXES
        .iter()
        .any(|prefix| path == *prefix || path.starts_with(&format!("{prefix}/")))
}

/// 找出保护该请求的访问规则，最具体的目录优先。
///
/// 同时按请求路径（忽略大小写）和解析后的真实路径（文件身份）匹配，
/// 避免大小写不敏感的文件系统或符号链接绕过保护。
#[must_use]
pub fn protecting_rule<'a>(
    site: &'a StaticSiteConfig,
    relative: &str,
    resolved: Option<&Path>,
) -> Option<&'a StaticAccessRule> {
    if site.access_rules.is_empty() {
        return None;
    }
    let root = canonical_site_root(site).ok();
    site.access_rules
        .iter()
        .filter(|rule| {
            lexically_protects(&rule.path, relative)
                || resolved
                    .zip(root.as_deref())
                    .is_some_and(|(resolved, root)| physically_protects(root, &rule.path, resolved))
        })
        .max_by_key(|rule| rule.path.len())
}

fn lexically_protects(rule_path: &str, relative: &str) -> bool {
    if rule_path.is_empty() {
        return true;
    }
    let rule_path = rule_path.to_lowercase();
    let relative = relative.trim_matches('/').to_lowercase();
    relative == rule_path
        || relative
            .strip_prefix(&rule_path)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn physically_protects(root: &Path, rule_path: &str, resolved: &Path) -> bool {
    let Ok(rule_dir) = fs::canonicalize(root.join(rule_path)) else {
        return false;
    };
    resolved
        .ancestors()
        .take_while(|ancestor| ancestor.starts_with(root))
        .any(|ancestor| {
            ancestor == rule_dir || same_file::is_same_file(ancestor, &rule_dir).unwrap_or(false)
        })
}

/// 校验 `Authorization: Basic ...` 是否满足访问规则。
pub async fn access_granted(rule: &StaticAccessRule, authorization: Option<&str>) -> bool {
    let Some(header) = authorization else {
        return false;
    };
    let cache_key = access_cache_key(rule, header);
    if access_cache_hit(&cache_key) {
        return true;
    }
    let Some((username, password)) = parse_basic_credentials(header) else {
        return false;
    };
    if username != rule.username {
        return false;
    }
    let hash = rule.password_hash.clone();
    let verified = tokio::task::spawn_blocking(move || hash::verify_password(&password, &hash))
        .await
        .unwrap_or(false);
    if verified {
        remember_access(cache_key);
    }
    verified
}

fn parse_basic_credentials(header: &str) -> Option<(String, String)> {
    let (scheme, encoded) = header.trim().split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("basic") {
        return None;
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .ok()?;
    let decoded = String::from_utf8(decoded).ok()?;
    let (username, password) = decoded.split_once(':')?;
    Some((username.to_string(), password.to_string()))
}

fn access_cache() -> &'static Mutex<HashMap<[u8; 32], Instant>> {
    static ACCESS_CACHE: OnceLock<Mutex<HashMap<[u8; 32], Instant>>> = OnceLock::new();
    ACCESS_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn access_cache_key(rule: &StaticAccessRule, header: &str) -> [u8; 32] {
    // 包含密码哈希：修改密码后旧凭据的缓存自然失效。
    let mut hasher = Sha256::new();
    hasher.update(rule.username.as_bytes());
    hasher.update([0]);
    hasher.update(rule.password_hash.as_bytes());
    hasher.update([0]);
    hasher.update(header.trim().as_bytes());
    hasher.finalize().into()
}

fn access_cache_hit(key: &[u8; 32]) -> bool {
    let Ok(cache) = access_cache().lock() else {
        return false;
    };
    cache
        .get(key)
        .is_some_and(|verified_at| verified_at.elapsed() < ACCESS_CACHE_TTL)
}

fn remember_access(key: [u8; 32]) {
    let Ok(mut cache) = access_cache().lock() else {
        return;
    };
    if cache.len() >= MAX_ACCESS_CACHE_ENTRIES {
        cache.retain(|_, verified_at| verified_at.elapsed() < ACCESS_CACHE_TTL);
        if cache.len() >= MAX_ACCESS_CACHE_ENTRIES {
            cache.clear();
        }
    }
    cache.insert(key, Instant::now());
}

/// `WWW-Authenticate` 使用的 realm：每条规则独立，浏览器不会混用不同目录的凭据。
#[must_use]
pub fn access_realm(site: &StaticSiteConfig, rule: &StaticAccessRule) -> String {
    let path = rule
        .path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(percent_encode_segment)
        .collect::<Vec<_>>()
        .join("/");
    format!("{}/{path}", site.key)
}

pub fn percent_decode_path(path: &str) -> ApiResult<String> {
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(ApiError::bad_request("URL 路径编码无效"));
            }
            let high = decode_hex(bytes[index + 1])?;
            let low = decode_hex(bytes[index + 2])?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).map_err(|_| ApiError::bad_request("URL 路径不是 UTF-8"))
}

pub fn public_file_url(site: &StaticSiteConfig, relative: &str) -> String {
    if relative.is_empty() {
        return site.url_path.clone();
    }
    let encoded = relative
        .split('/')
        .map(percent_encode_segment)
        .collect::<Vec<_>>()
        .join("/");
    if site.url_path == "/" {
        format!("/{encoded}")
    } else {
        format!("{}/{encoded}", site.url_path)
    }
}

fn normalize_url_path(path: &str) -> ApiResult<String> {
    let path = path.trim();
    if !path.starts_with('/') || path.contains(['?', '#', '\\', '\0']) {
        return Err(ApiError::bad_request(
            "URL 路径必须以 / 开头且不能包含查询参数或反斜线",
        ));
    }
    if path == "/" {
        return Ok(path.to_string());
    }
    let path = path.trim_end_matches('/');
    validate_relative_path(path.trim_start_matches('/'))?;
    Ok(path.to_string())
}

fn validate_relative_path(path: &str) -> ApiResult<()> {
    if path.contains(['\\', '\0'])
        || path.split('/').any(|segment| {
            segment.is_empty()
                || segment == "."
                || segment == ".."
                || DENIED_PATH_SEGMENTS.contains(&segment)
        })
        || Path::new(path).components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(ApiError::bad_request("文件路径无效或包含受保护目录"));
    }
    Ok(())
}

fn ensure_under_root(root: &Path, target: &Path) -> ApiResult<()> {
    if target.starts_with(root) {
        Ok(())
    } else {
        Err(ApiError::forbidden("文件路径超出静态托管目录"))
    }
}

fn matched_relative_path(prefix: &str, request_path: &str) -> Option<String> {
    if prefix == "/" {
        return Some(request_path.trim_start_matches('/').to_string());
    }
    if request_path == prefix {
        return Some(String::new());
    }
    request_path
        .strip_prefix(prefix)
        .and_then(|remaining| remaining.strip_prefix('/'))
        .map(str::to_string)
}

fn canonical_file_under_root(root: &Path, target: &Path) -> ApiResult<Option<PathBuf>> {
    if !target.exists() {
        return Ok(None);
    }
    let target = fs::canonicalize(target).map_err(|_| ApiError::bad_request("静态文件不存在"))?;
    ensure_under_root(root, &target)?;
    if target.is_file() {
        Ok(Some(target))
    } else {
        Ok(None)
    }
}

fn canonical_dir_under_root(root: &Path, target: &Path) -> ApiResult<Option<PathBuf>> {
    if !target.is_dir() {
        return Ok(None);
    }
    let target = fs::canonicalize(target).map_err(|_| ApiError::bad_request("静态目录不存在"))?;
    ensure_under_root(root, &target)?;
    Ok(target.is_dir().then_some(target))
}

fn decode_hex(value: u8) -> ApiResult<u8> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(ApiError::bad_request("URL 路径编码无效")),
    }
}

fn percent_encode_segment(segment: &str) -> String {
    let mut encoded = String::new();
    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn site(key: &str, url_path: &str) -> StaticSiteConfig {
        StaticSiteConfig {
            key: key.to_string(),
            name: key.to_string(),
            url_path: url_path.to_string(),
            local_root: "storage/static-hosting/test".to_string(),
            index_file: "index.html".to_string(),
            spa_fallback: false,
            directory_listing: false,
            listing_overrides_index: false,
            enabled: true,
            cache_seconds: 300,
            access_rules: Vec::new(),
        }
    }

    fn rule(path: &str, username: &str, password_hash: &str) -> StaticAccessRule {
        StaticAccessRule {
            path: path.to_string(),
            username: username.to_string(),
            password_hash: password_hash.to_string(),
        }
    }

    fn basic(username: &str, password: &str) -> String {
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"))
        )
    }

    fn file_target(path: &Path) -> Option<PublicTarget> {
        fs::canonicalize(path).ok().map(PublicTarget::File)
    }

    #[test]
    fn longest_url_prefix_wins() {
        let sites = vec![site("docs", "/docs"), site("v2", "/docs/v2")];
        let (matched, relative) = match_site(&sites, "/docs/v2/index.html").unwrap();
        assert_eq!(matched.key, "v2");
        assert_eq!(relative, "index.html");
    }

    #[test]
    fn protected_and_traversal_paths_are_rejected() {
        assert!(normalize_relative_path(Some("../config/development.yaml")).is_err());
        assert!(normalize_relative_path(Some("site/.git/config")).is_err());
        assert!(normalize_site(site("admin", "/admin/site")).is_err());
        assert!(normalize_site(site("assets", "/static/site")).is_err());
    }

    #[test]
    fn encoded_paths_are_decoded_without_losing_utf8() {
        assert_eq!(
            percent_decode_path("/%E9%A6%96%E9%A1%B5.html").unwrap(),
            "/首页.html"
        );
        assert!(percent_decode_path("/%ZZ").is_err());
    }

    #[test]
    fn hosted_index_spa_fallback_and_nested_write_paths_resolve() {
        let root = std::env::temp_dir().join(format!("gpt-images-static-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("index.html"), b"static index").unwrap();
        let mut site = site("preview", "/preview");
        site.local_root = root.to_string_lossy().to_string();
        site.spa_fallback = true;

        assert_eq!(
            resolve_public_target(&site, "", true).unwrap(),
            file_target(&root.join("index.html"))
        );
        assert_eq!(
            resolve_public_target(&site, "dashboard/report", true).unwrap(),
            file_target(&root.join("index.html"))
        );
        assert!(resolve_public_target(&site, "missing.png", false)
            .unwrap()
            .is_none());

        let nested = resolve_write_path(&site, "assets/js/app.js").unwrap();
        assert_eq!(
            nested,
            fs::canonicalize(&root).unwrap().join("assets/js/app.js")
        );
        assert!(root.join("assets/js").is_dir());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn directory_without_index_is_listed_only_when_enabled() {
        let root = std::env::temp_dir().join(format!("gpt-images-listing-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("docs/guide")).unwrap();
        fs::write(root.join("docs/b <script>.txt"), b"hello").unwrap();
        fs::write(root.join("docs/.secret"), b"hidden").unwrap();
        fs::write(root.join("docs/guide/index.html"), b"guide").unwrap();
        let mut site = site("files", "/files");
        site.local_root = root.to_string_lossy().to_string();

        assert!(resolve_public_target(&site, "docs", false)
            .unwrap()
            .is_none());

        site.directory_listing = true;
        let Some(PublicTarget::Directory { path, relative }) =
            resolve_public_target(&site, "docs", false).unwrap()
        else {
            panic!("expected directory listing target");
        };
        assert_eq!(relative, "docs");
        // 有首页的子目录仍然返回首页文件
        assert_eq!(
            resolve_public_target(&site, "docs/guide", false).unwrap(),
            file_target(&root.join("docs/guide/index.html"))
        );

        let listing = list_public_directory(&path, &relative).unwrap();
        let names = listing
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["guide", "b <script>.txt"]);

        let html = render_directory_listing(&site, &relative, &listing);
        assert!(html.contains(r#"href="/files/docs/guide/""#));
        assert!(html.contains(r#"href="/files/docs/b%20%3Cscript%3E.txt""#));
        assert!(html.contains("b &lt;script&gt;.txt"));
        assert!(html.contains(r#"href="/files/""#));
        assert!(!html.contains(".secret"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn listing_can_take_priority_over_index_file() {
        let root = std::env::temp_dir().join(format!("gpt-images-override-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("index.html"), b"home").unwrap();
        fs::write(root.join("notes.txt"), b"notes").unwrap();
        let mut site = site("override", "/override");
        site.local_root = root.to_string_lossy().to_string();
        site.directory_listing = true;

        assert_eq!(
            resolve_public_target(&site, "", true).unwrap(),
            file_target(&root.join("index.html"))
        );

        site.listing_overrides_index = true;
        let Some(PublicTarget::Directory { path, relative }) =
            resolve_public_target(&site, "", true).unwrap()
        else {
            panic!("expected directory listing target");
        };
        // 首页文件仍然可以直接访问
        assert_eq!(
            resolve_public_target(&site, "index.html", true).unwrap(),
            file_target(&root.join("index.html"))
        );
        let listing = list_public_directory(&path, &relative).unwrap();
        let html = render_directory_listing(&site, &relative, &listing);
        assert!(html.contains(r#"class="notice""#));
        assert!(html.contains(r#"href="/override/index.html""#));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn access_rules_validate_and_redact() {
        let mut protected = site("locked", "/locked");
        protected.access_rules = vec![rule("/Private/", "alice", "hash")];
        let normalized = normalize_site(protected.clone()).unwrap();
        assert_eq!(normalized.access_rules[0].path, "Private");

        protected.access_rules = vec![rule("a", "bad:name", "hash")];
        assert!(normalize_site(protected.clone()).is_err());
        protected.access_rules = vec![rule("a", "alice", "")];
        assert!(normalize_site(protected.clone()).is_err());
        protected.access_rules = vec![rule("a", "alice", "x"), rule("A", "bob", "y")];
        assert!(normalize_site(protected.clone()).is_err());
        protected.access_rules = vec![rule("../etc", "alice", "x")];
        assert!(normalize_site(protected.clone()).is_err());
        assert!(hash_access_password("abc").is_err());

        let redacted = redact_site(normalized);
        assert!(redacted.access_rules[0].password_hash.is_empty());
        let json = serde_json::to_string(&redacted).unwrap();
        assert!(!json.contains("password_hash"));
    }

    #[test]
    fn most_specific_rule_protects_paths_case_insensitively() {
        let mut site = site("rules", "/rules");
        site.access_rules = vec![rule("", "site", "h1"), rule("docs/private", "owner", "h2")];
        fn matched<'a>(site: &'a StaticSiteConfig, relative: &str) -> Option<&'a str> {
            protecting_rule(site, relative, None).map(|rule| rule.username.as_str())
        }
        assert_eq!(matched(&site, "index.html"), Some("site"));
        assert_eq!(matched(&site, "docs/private"), Some("owner"));
        assert_eq!(matched(&site, "Docs/PRIVATE/report.pdf"), Some("owner"));
        assert_eq!(matched(&site, "docs/private-notes.txt"), Some("site"));

        site.access_rules = vec![rule("docs/private", "owner", "h2")];
        assert_eq!(matched(&site, "docs/readme.md"), None);
        assert_eq!(matched(&site, ""), None);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_into_protected_directories_stay_protected() {
        let root = std::env::temp_dir().join(format!("gpt-images-symlink-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("private")).unwrap();
        fs::write(root.join("private/secret.txt"), b"secret").unwrap();
        std::os::unix::fs::symlink(root.join("private"), root.join("public-alias")).unwrap();
        let mut site = site("links", "/links");
        site.local_root = root.to_string_lossy().to_string();
        site.access_rules = vec![rule("private", "owner", "hash")];

        let Some(PublicTarget::File(resolved)) =
            resolve_public_target(&site, "public-alias/secret.txt", false).unwrap()
        else {
            panic!("expected the symlinked file to resolve");
        };
        assert!(protecting_rule(&site, "public-alias/secret.txt", Some(&resolved)).is_some());
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn basic_credentials_are_verified_against_the_rule() {
        let hash = hash_access_password("s3cret-pass").unwrap();
        let rule = rule("", "alice", &hash);

        assert!(access_granted(&rule, Some(&basic("alice", "s3cret-pass"))).await);
        // 第二次命中缓存，结果一致
        assert!(access_granted(&rule, Some(&basic("alice", "s3cret-pass"))).await);
        assert!(!access_granted(&rule, Some(&basic("alice", "wrong"))).await);
        assert!(!access_granted(&rule, Some(&basic("bob", "s3cret-pass"))).await);
        assert!(!access_granted(&rule, Some("Bearer token")).await);
        assert!(!access_granted(&rule, Some("Basic not-base64!")).await);
        assert!(!access_granted(&rule, None).await);
        assert_eq!(
            parse_basic_credentials(&basic("alice", "a:b")),
            Some(("alice".to_string(), "a:b".to_string()))
        );
    }
}
