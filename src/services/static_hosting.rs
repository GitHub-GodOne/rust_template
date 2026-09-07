#![allow(clippy::missing_errors_doc)]

use std::{
    collections::HashSet,
    fs,
    path::{Component, Path, PathBuf},
    sync::OnceLock,
    time::{Duration, Instant},
};

use loco_rs::app::AppContext;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter, Set};
use serde::{Deserialize, Serialize};
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
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_cache_seconds")]
    pub cache_seconds: u32,
}

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
    Ok(site)
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

pub fn resolve_public_file(
    site: &StaticSiteConfig,
    relative: &str,
    allow_spa_fallback: bool,
) -> ApiResult<Option<PathBuf>> {
    let root = canonical_site_root(site)?;
    let relative = normalize_relative_path(Some(relative))?;
    let requested = if relative.is_empty() {
        root.join(&site.index_file)
    } else {
        root.join(&relative)
    };

    if let Some(file) = canonical_file_under_root(&root, &requested)? {
        return Ok(Some(file));
    }
    if requested.is_dir() {
        if let Some(file) = canonical_file_under_root(&root, &requested.join(&site.index_file))? {
            return Ok(Some(file));
        }
    }
    if site.spa_fallback && allow_spa_fallback {
        return canonical_file_under_root(&root, &root.join(&site.index_file));
    }
    Ok(None)
}

#[must_use]
pub fn is_reserved_request_path(path: &str) -> bool {
    RESERVED_PUBLIC_PREFIXES
        .iter()
        .any(|prefix| path == *prefix || path.starts_with(&format!("{prefix}/")))
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
            enabled: true,
            cache_seconds: 300,
        }
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
            resolve_public_file(&site, "", true).unwrap().unwrap(),
            fs::canonicalize(root.join("index.html")).unwrap()
        );
        assert_eq!(
            resolve_public_file(&site, "dashboard/report", true)
                .unwrap()
                .unwrap(),
            fs::canonicalize(root.join("index.html")).unwrap()
        );
        assert!(resolve_public_file(&site, "missing.png", false)
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
}
