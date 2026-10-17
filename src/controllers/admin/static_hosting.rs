#![allow(clippy::missing_errors_doc)]

use std::{fs, path::Path as FsPath};

use axum::extract::{multipart::Field, Multipart};
use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    controllers::admin::authorize,
    errors::{ApiError, ApiResult},
    models::{admin_logs, system_settings},
    responses::{self, ApiResponse, EmptyData},
    services::static_hosting::{
        access_rule_label, hash_access_password, load_sites, normalize_relative_path,
        normalize_required_relative_path, normalize_site, prepare_site_root, public_file_url,
        redact_site, resolve_existing_path, resolve_write_path, save_sites, StaticAccessRule,
        StaticSiteConfig,
    },
};

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveStaticSiteParams {
    pub key: String,
    pub name: String,
    pub url_path: String,
    pub local_root: String,
    pub index_file: Option<String>,
    pub spa_fallback: Option<bool>,
    pub directory_listing: Option<bool>,
    /// 有首页文件时仍显示目录列表；未传入时保留原值。
    pub listing_overrides_index: Option<bool>,
    pub enabled: Option<bool>,
    pub cache_seconds: Option<u32>,
    /// 受密码保护的目录；未传入时保留原有规则，传入空数组表示全部移除。
    pub access_rules: Option<Vec<SaveStaticAccessRuleParams>>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveStaticAccessRuleParams {
    /// 站点内的相对目录，留空表示整个站点。
    pub path: Option<String>,
    pub username: String,
    /// 新密码；编辑已有目录时留空表示保持原密码。
    pub password: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct StaticBrowserParams {
    pub site_key: String,
    pub path: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct StaticPathParams {
    pub site_key: String,
    pub path: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CreateStaticFolderParams {
    pub site_key: String,
    pub path: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct StaticFileRecord {
    pub name: String,
    pub path: String,
    pub url: String,
    pub is_dir: bool,
    pub extension: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: i64,
    pub updated_at: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct StaticBrowserRecord {
    pub site: StaticSiteConfig,
    pub path: String,
    pub directories: Vec<StaticFileRecord>,
    pub files: Vec<StaticFileRecord>,
}

impl SaveStaticSiteParams {
    fn into_config(self, existing: Option<&StaticSiteConfig>) -> ApiResult<StaticSiteConfig> {
        let access_rules = match self.access_rules {
            Some(rules) => resolve_access_rules(rules, existing)?,
            None => existing
                .map(|site| site.access_rules.clone())
                .unwrap_or_default(),
        };
        Ok(StaticSiteConfig {
            key: self.key,
            name: self.name,
            url_path: self.url_path,
            local_root: self.local_root,
            index_file: self.index_file.unwrap_or_else(|| "index.html".to_string()),
            spa_fallback: self.spa_fallback.unwrap_or(false),
            directory_listing: self.directory_listing.unwrap_or(false),
            listing_overrides_index: self
                .listing_overrides_index
                .or_else(|| existing.map(|site| site.listing_overrides_index))
                .unwrap_or(false),
            enabled: self.enabled.unwrap_or(true),
            cache_seconds: self.cache_seconds.unwrap_or(300),
            access_rules,
        })
    }
}

/// 新密码会被哈希；未填写密码的目录沿用同一目录已保存的密码。
fn resolve_access_rules(
    rules: Vec<SaveStaticAccessRuleParams>,
    existing: Option<&StaticSiteConfig>,
) -> ApiResult<Vec<StaticAccessRule>> {
    rules
        .into_iter()
        .map(|rule| {
            let path = normalize_relative_path(rule.path.as_deref())?;
            let password_hash = match rule.password.filter(|password| !password.is_empty()) {
                Some(password) => hash_access_password(&password)?,
                None => existing
                    .and_then(|site| {
                        site.access_rules
                            .iter()
                            .find(|saved| saved.path.eq_ignore_ascii_case(&path))
                    })
                    .map(|saved| saved.password_hash.clone())
                    .ok_or_else(|| {
                        ApiError::bad_request(format!(
                            "请为受保护目录「{}」设置访问密码",
                            access_rule_label(&path)
                        ))
                    })?,
            };
            Ok(StaticAccessRule {
                path,
                username: rule.username,
                password_hash,
            })
        })
        .collect()
}

#[utoipa::path(
    get,
    path = "/api/admin/static-sites",
    tag = "admin-static-hosting",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<Vec<StaticSiteConfig>>))
)]
#[debug_handler]
pub async fn list(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:static_host:list").await?;
    let sites = load_sites(&ctx).await?;
    Ok(responses::ok(
        sites.into_iter().map(redact_site).collect::<Vec<_>>(),
    ))
}

#[utoipa::path(
    post,
    path = "/api/admin/static-sites",
    tag = "admin-static-hosting",
    security(("bearer_auth" = [])),
    request_body = SaveStaticSiteParams,
    responses((status = 200, body = ApiResponse<StaticSiteConfig>))
)]
#[debug_handler]
pub async fn create(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SaveStaticSiteParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:static_host:create").await?;
    let site = normalize_site(params.into_config(None)?)?;
    let mut sites = load_sites(&ctx).await?;
    if sites.iter().any(|existing| existing.key == site.key) {
        return Err(ApiError::bad_request("站点标识已存在"));
    }
    if sites
        .iter()
        .any(|existing| existing.url_path == site.url_path)
    {
        return Err(ApiError::bad_request("URL 路径已被其他站点使用"));
    }
    prepare_site_root(&site)?;
    sites.push(site.clone());
    save_sites(&ctx, sites, user.id).await?;
    record_static_log(
        &ctx,
        &user,
        "create_site",
        format!("新增静态托管站点：{} -> {}", site.url_path, site.local_root),
    )
    .await;
    Ok(responses::ok(redact_site(site)))
}

#[utoipa::path(
    put,
    path = "/api/admin/static-sites/{key}",
    tag = "admin-static-hosting",
    security(("bearer_auth" = [])),
    params(("key" = String, Path)),
    request_body = SaveStaticSiteParams,
    responses((status = 200, body = ApiResponse<StaticSiteConfig>))
)]
#[debug_handler]
pub async fn update(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(key): Path<String>,
    Json(params): Json<SaveStaticSiteParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:static_host:update").await?;
    let key = key.trim().to_ascii_lowercase();
    let mut sites = load_sites(&ctx).await?;
    let index = sites
        .iter()
        .position(|existing| existing.key == key)
        .ok_or_else(|| ApiError::bad_request("静态托管站点不存在"))?;
    let mut site = params.into_config(Some(&sites[index]))?;
    site.key = key;
    let site = normalize_site(site)?;
    if sites
        .iter()
        .enumerate()
        .any(|(other_index, existing)| other_index != index && existing.url_path == site.url_path)
    {
        return Err(ApiError::bad_request("URL 路径已被其他站点使用"));
    }
    prepare_site_root(&site)?;
    sites[index] = site.clone();
    save_sites(&ctx, sites, user.id).await?;
    record_static_log(
        &ctx,
        &user,
        "update_site",
        format!(
            "更新静态托管站点：{} -> {}（密码保护目录 {} 个）",
            site.url_path,
            site.local_root,
            site.access_rules.len()
        ),
    )
    .await;
    Ok(responses::ok(redact_site(site)))
}

#[utoipa::path(
    delete,
    path = "/api/admin/static-sites/{key}",
    tag = "admin-static-hosting",
    security(("bearer_auth" = [])),
    params(("key" = String, Path)),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn delete_site(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(key): Path<String>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:static_host:delete").await?;
    let mut sites = load_sites(&ctx).await?;
    let original_len = sites.len();
    sites.retain(|site| site.key != key);
    if sites.len() == original_len {
        return Err(ApiError::bad_request("静态托管站点不存在"));
    }
    save_sites(&ctx, sites, user.id).await?;
    record_static_log(
        &ctx,
        &user,
        "delete_site",
        format!("删除静态托管映射：{key}（服务器文件保留）"),
    )
    .await;
    Ok(responses::empty())
}

#[utoipa::path(
    get,
    path = "/api/admin/static-sites/browser",
    tag = "admin-static-hosting",
    security(("bearer_auth" = [])),
    params(StaticBrowserParams),
    responses((status = 200, body = ApiResponse<StaticBrowserRecord>))
)]
#[debug_handler]
pub async fn browser(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<StaticBrowserParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:static_host:list").await?;
    let site = find_site(&ctx, &params.site_key).await?;
    let relative = normalize_relative_path(params.path.as_deref())?;
    let directory = resolve_existing_path(&site, &relative)?;
    if !directory.is_dir() {
        return Err(ApiError::bad_request("当前路径不是目录"));
    }

    let mut directories = Vec::new();
    let mut files = Vec::new();
    for entry in fs::read_dir(directory).map_err(|_| ApiError::internal("读取托管目录失败"))?
    {
        let entry = entry.map_err(|_| ApiError::internal("读取托管目录失败"))?;
        let file_type = entry
            .file_type()
            .map_err(|_| ApiError::internal("读取文件类型失败"))?;
        if file_type.is_symlink() {
            continue;
        }
        let metadata = entry
            .metadata()
            .map_err(|_| ApiError::internal("读取文件信息失败"))?;
        let name = entry.file_name().to_string_lossy().to_string();
        let path = join_relative_path(&relative, &name);
        let record = static_file_record(&site, &path, &name, &metadata);
        if metadata.is_dir() {
            directories.push(record);
        } else if metadata.is_file() {
            files.push(record);
        }
    }
    directories.sort_by(|left, right| left.name.cmp(&right.name));
    files.sort_by(|left, right| left.name.cmp(&right.name));

    Ok(responses::ok(StaticBrowserRecord {
        site: redact_site(site),
        path: relative,
        directories,
        files,
    }))
}

#[utoipa::path(
    post,
    path = "/api/admin/static-sites/folders",
    tag = "admin-static-hosting",
    security(("bearer_auth" = [])),
    request_body = CreateStaticFolderParams,
    responses((status = 200, body = ApiResponse<StaticFileRecord>))
)]
#[debug_handler]
pub async fn create_folder(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<CreateStaticFolderParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:static_host:upload").await?;
    let site = find_site(&ctx, &params.site_key).await?;
    let relative = normalize_required_relative_path(&params.path)?;
    let target = resolve_write_path(&site, &relative)?;
    if target.exists() {
        return Err(ApiError::bad_request("目标目录已存在"));
    }
    fs::create_dir(&target).map_err(|_| ApiError::internal("创建目录失败"))?;
    let metadata = fs::metadata(&target).map_err(|_| ApiError::internal("读取目录信息失败"))?;
    let name = file_name(&relative)?;
    record_static_log(
        &ctx,
        &user,
        "create_folder",
        format!("创建静态托管目录：{}/{}", site.key, relative),
    )
    .await;
    Ok(responses::ok(static_file_record(
        &site, &relative, &name, &metadata,
    )))
}

#[utoipa::path(
    post,
    path = "/api/admin/static-sites/upload",
    tag = "admin-static-hosting",
    security(("bearer_auth" = [])),
    request_body(content = String, content_type = "multipart/form-data"),
    responses((status = 200, body = ApiResponse<StaticFileRecord>))
)]
#[debug_handler]
pub async fn upload(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    mut multipart: Multipart,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:static_host:upload").await?;
    let max_bytes = system_settings::number_i64(&ctx.db, "static_hosting.max_file_size_mb", 50)
        .await?
        .clamp(1, 1024)
        * 1024
        * 1024;
    let mut site_key = None;
    let mut relative = None;
    let mut overwrite = false;
    let mut uploaded = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| ApiError::bad_request("上传请求格式无效"))?
    {
        let name = field.name().unwrap_or_default().to_string();
        match name.as_str() {
            "site_key" => {
                site_key = Some(
                    field
                        .text()
                        .await
                        .map_err(|_| ApiError::bad_request("读取站点标识失败"))?,
                );
            }
            "path" => {
                relative = Some(
                    field
                        .text()
                        .await
                        .map_err(|_| ApiError::bad_request("读取上传路径失败"))?,
                );
            }
            "overwrite" => {
                overwrite = field
                    .text()
                    .await
                    .map_err(|_| ApiError::bad_request("读取覆盖选项失败"))?
                    == "true";
            }
            "file" => {
                uploaded = Some(read_file_field(field, max_bytes).await?);
            }
            _ => {}
        }
    }

    let site_key = site_key.ok_or_else(|| ApiError::bad_request("站点标识不能为空"))?;
    let relative = normalize_required_relative_path(
        relative
            .as_deref()
            .ok_or_else(|| ApiError::bad_request("上传路径不能为空"))?,
    )?;
    let bytes = uploaded.ok_or_else(|| ApiError::bad_request("上传文件不能为空"))?;
    let site = find_site(&ctx, &site_key).await?;
    let target = resolve_write_path(&site, &relative)?;
    if target.exists() {
        let existing = resolve_existing_path(&site, &relative)?;
        if existing.is_dir() {
            return Err(ApiError::bad_request("上传路径与现有目录冲突"));
        }
        if !overwrite {
            return Err(ApiError::bad_request("目标文件已存在，请启用覆盖上传"));
        }
    }
    fs::write(&target, bytes).map_err(|_| ApiError::internal("保存托管文件失败"))?;
    let metadata = fs::metadata(&target).map_err(|_| ApiError::internal("读取文件信息失败"))?;
    let name = file_name(&relative)?;
    record_static_log(
        &ctx,
        &user,
        "upload",
        format!("上传静态托管文件：{}/{}", site.key, relative),
    )
    .await;
    Ok(responses::ok(static_file_record(
        &site, &relative, &name, &metadata,
    )))
}

#[utoipa::path(
    delete,
    path = "/api/admin/static-sites/files",
    tag = "admin-static-hosting",
    security(("bearer_auth" = [])),
    params(StaticPathParams),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn delete_file(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<StaticPathParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:static_host:delete").await?;
    let site = find_site(&ctx, &params.site_key).await?;
    let relative = normalize_required_relative_path(&params.path)?;
    let target = resolve_existing_path(&site, &relative)?;
    if target.is_dir() {
        fs::remove_dir_all(&target).map_err(|_| ApiError::internal("删除目录失败"))?;
    } else {
        fs::remove_file(&target).map_err(|_| ApiError::internal("删除文件失败"))?;
    }
    record_static_log(
        &ctx,
        &user,
        "delete_file",
        format!("删除静态托管内容：{}/{}", site.key, relative),
    )
    .await;
    Ok(responses::empty())
}

async fn find_site(ctx: &AppContext, key: &str) -> ApiResult<StaticSiteConfig> {
    load_sites(ctx)
        .await?
        .into_iter()
        .find(|site| site.key == key)
        .ok_or_else(|| ApiError::bad_request("静态托管站点不存在"))
}

fn static_file_record(
    site: &StaticSiteConfig,
    relative: &str,
    name: &str,
    metadata: &fs::Metadata,
) -> StaticFileRecord {
    StaticFileRecord {
        name: name.to_string(),
        path: relative.to_string(),
        url: public_file_url(site, relative),
        is_dir: metadata.is_dir(),
        extension: metadata
            .is_file()
            .then(|| file_extension(relative))
            .flatten(),
        mime_type: metadata
            .is_file()
            .then(|| {
                mime_guess::from_path(relative)
                    .first()
                    .map(|mime| mime.to_string())
            })
            .flatten(),
        size_bytes: if metadata.is_file() {
            i64::try_from(metadata.len()).unwrap_or(i64::MAX)
        } else {
            0
        },
        updated_at: metadata
            .modified()
            .ok()
            .map(chrono::DateTime::<chrono::Utc>::from)
            .map(|time| time.to_rfc3339()),
    }
}

async fn read_file_field(mut field: Field<'_>, max_bytes: i64) -> ApiResult<Vec<u8>> {
    let max_bytes = usize::try_from(max_bytes.max(1)).unwrap_or(usize::MAX);
    let mut bytes = Vec::new();
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|_| ApiError::bad_request("读取上传文件失败"))?
    {
        if bytes.len().saturating_add(chunk.len()) > max_bytes {
            return Err(ApiError::bad_request("单个托管文件超过上传大小限制"));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

async fn record_static_log(
    ctx: &AppContext,
    user: &crate::models::users::Model,
    action: &'static str,
    message: String,
) {
    admin_logs::record(
        &ctx.db,
        admin_logs::LogInput {
            log_type: "operation",
            level: "info",
            module: "static_hosting",
            action,
            message: &message,
            user_id: Some(user.id),
            operator: Some(user.email.clone()),
            method: None,
            path: Some("/api/admin/static-sites"),
            status: Some(200),
            error_message: None,
        },
    )
    .await;
}

fn join_relative_path(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{}/{name}", prefix.trim_end_matches('/'))
    }
}

fn file_name(path: &str) -> ApiResult<String> {
    FsPath::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| ApiError::bad_request("文件路径无效"))
}

fn file_extension(path: &str) -> Option<String> {
    FsPath::new(path)
        .extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
        .filter(|extension| !extension.is_empty())
}
