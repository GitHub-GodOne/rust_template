#![allow(clippy::missing_errors_doc)]

use std::{
    fmt::Write as _,
    fs,
    path::{Component, Path, PathBuf},
};

use axum::{
    extract::{multipart::Field, Multipart},
    http::{header, HeaderMap, HeaderName, HeaderValue},
    response::IntoResponse,
};
use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    controllers::admin::authorize,
    errors::{ApiError, ApiResult},
    models::{admin_logs, system_settings},
    responses::{self, ApiResponse, EmptyData},
};

const ROOTS_SETTING_KEY: &str = "file_manager.roots";

#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
pub struct FileRootRecord {
    pub key: String,
    pub name: String,
    pub url_path: String,
    pub local_root: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
pub struct ManagedFileRecord {
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
pub struct FileBrowserRecord {
    pub root: FileRootRecord,
    pub path: String,
    pub directories: Vec<ManagedFileRecord>,
    pub files: Vec<ManagedFileRecord>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct FileBrowserParams {
    pub root_key: String,
    pub path: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct FilePathParams {
    pub root_key: String,
    pub path: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CreateFileFolderParams {
    pub root_key: String,
    pub path: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct RenameFileParams {
    pub root_key: String,
    pub path: String,
    pub name: String,
}

#[utoipa::path(
    get,
    path = "/api/admin/files/roots",
    tag = "admin-files",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<Vec<FileRootRecord>>))
)]
#[debug_handler]
pub async fn roots(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:file:list").await?;
    Ok(responses::ok(file_roots(&ctx).await?))
}

#[utoipa::path(
    get,
    path = "/api/admin/files/browser",
    tag = "admin-files",
    security(("bearer_auth" = [])),
    params(FileBrowserParams),
    responses((status = 200, body = ApiResponse<FileBrowserRecord>))
)]
#[debug_handler]
pub async fn browser(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<FileBrowserParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:file:list").await?;
    let root = find_root(&ctx, &params.root_key).await?;
    let relative = normalize_relative_path(params.path.as_deref())?;
    let directory = resolve_existing_path(&root, &relative)?;
    if !directory.is_dir() {
        return Err(ApiError::bad_request("path is not a directory"));
    }

    let mut directories = Vec::new();
    let mut files = Vec::new();
    for entry in fs::read_dir(directory).map_err(|_| ApiError::internal("failed to list files"))? {
        let entry = entry.map_err(|_| ApiError::internal("failed to list files"))?;
        let metadata = entry
            .metadata()
            .map_err(|_| ApiError::internal("failed to read file metadata"))?;
        let name = entry.file_name().to_string_lossy().to_string();
        let item_path = join_relative_path(&relative, &name);
        let record = file_record(&root, &item_path, &name, &metadata);
        if metadata.is_dir() {
            directories.push(record);
        } else if metadata.is_file() {
            files.push(record);
        }
    }
    directories.sort_by(|left, right| left.name.cmp(&right.name));
    files.sort_by(|left, right| left.name.cmp(&right.name));

    Ok(responses::ok(FileBrowserRecord {
        root,
        path: relative,
        directories,
        files,
    }))
}

#[utoipa::path(
    post,
    path = "/api/admin/files/folders",
    tag = "admin-files",
    security(("bearer_auth" = [])),
    request_body = CreateFileFolderParams,
    responses((status = 200, body = ApiResponse<ManagedFileRecord>))
)]
#[debug_handler]
pub async fn create_folder(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<CreateFileFolderParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:file:create").await?;
    let root = find_root(&ctx, &params.root_key).await?;
    let relative = normalize_required_path(&params.path)?;
    let target = resolve_new_path(&root, &relative)?;
    if target.exists() {
        return Err(ApiError::bad_request("target path already exists"));
    }
    fs::create_dir(&target).map_err(|_| ApiError::internal("failed to create folder"))?;
    let metadata =
        fs::metadata(&target).map_err(|_| ApiError::internal("failed to read folder"))?;
    let name = file_name(&relative)?;
    record_file_log(
        &ctx,
        &user,
        "create_folder",
        format!("创建文件目录：{relative}"),
        Some(200),
        None,
    )
    .await;
    Ok(responses::ok(file_record(
        &root, &relative, &name, &metadata,
    )))
}

#[utoipa::path(
    post,
    path = "/api/admin/files/upload",
    tag = "admin-files",
    security(("bearer_auth" = [])),
    request_body(content = String, content_type = "multipart/form-data"),
    responses((status = 200, body = ApiResponse<ManagedFileRecord>))
)]
#[debug_handler]
pub async fn upload(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    mut multipart: Multipart,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:file:create").await?;
    let max_bytes =
        system_settings::number_i64(&ctx.db, "upload.max_size_mb", 20).await? * 1024 * 1024;
    let mut root_key = None;
    let mut directory = String::new();
    let mut uploaded = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| ApiError::bad_request("invalid multipart payload"))?
    {
        let name = field.name().unwrap_or_default().to_string();
        match name.as_str() {
            "file" => {
                let original_name = field.file_name().unwrap_or("upload.bin").to_string();
                let bytes = read_file_field(field, max_bytes).await?;
                uploaded = Some((sanitize_filename(&original_name), bytes));
            }
            "root_key" => {
                root_key = Some(
                    field
                        .text()
                        .await
                        .map_err(|_| ApiError::bad_request("failed to read root_key"))?,
                );
            }
            "path" => {
                directory = field
                    .text()
                    .await
                    .map_err(|_| ApiError::bad_request("failed to read path"))?;
            }
            _ => {}
        }
    }

    let root_key = root_key.ok_or_else(|| ApiError::bad_request("root_key is required"))?;
    let (filename, bytes) = uploaded.ok_or_else(|| ApiError::bad_request("file is required"))?;
    let root = find_root(&ctx, &root_key).await?;
    let directory = normalize_relative_path(Some(&directory))?;
    let relative = join_relative_path(&directory, &filename);
    let target = resolve_new_path(&root, &relative)?;
    if target.exists() {
        return Err(ApiError::bad_request("target file already exists"));
    }
    fs::write(&target, bytes).map_err(|_| ApiError::internal("failed to save file"))?;
    let metadata = fs::metadata(&target).map_err(|_| ApiError::internal("failed to read file"))?;
    record_file_log(
        &ctx,
        &user,
        "upload",
        format!("上传文件：{relative}"),
        Some(200),
        None,
    )
    .await;
    Ok(responses::ok(file_record(
        &root, &relative, &filename, &metadata,
    )))
}

#[utoipa::path(
    put,
    path = "/api/admin/files/rename",
    tag = "admin-files",
    security(("bearer_auth" = [])),
    request_body = RenameFileParams,
    responses((status = 200, body = ApiResponse<ManagedFileRecord>))
)]
#[debug_handler]
pub async fn rename(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<RenameFileParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:file:update").await?;
    let root = find_root(&ctx, &params.root_key).await?;
    let relative = normalize_required_path(&params.path)?;
    let source = resolve_existing_path(&root, &relative)?;
    let name = sanitize_filename(&params.name);
    let parent = parent_relative_path(&relative);
    let target_relative = join_relative_path(parent.as_deref().unwrap_or_default(), &name);
    let target = resolve_new_path(&root, &target_relative)?;
    if target.exists() {
        return Err(ApiError::bad_request("target path already exists"));
    }
    fs::rename(&source, &target).map_err(|_| ApiError::internal("failed to rename file"))?;
    let metadata = fs::metadata(&target).map_err(|_| ApiError::internal("failed to read file"))?;
    record_file_log(
        &ctx,
        &user,
        "rename",
        format!("重命名文件：{relative} -> {target_relative}"),
        Some(200),
        None,
    )
    .await;
    Ok(responses::ok(file_record(
        &root,
        &target_relative,
        &name,
        &metadata,
    )))
}

#[utoipa::path(
    delete,
    path = "/api/admin/files",
    tag = "admin-files",
    security(("bearer_auth" = [])),
    params(FilePathParams),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn delete(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<FilePathParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:file:delete").await?;
    let root = find_root(&ctx, &params.root_key).await?;
    let relative = normalize_required_path(&params.path)?;
    let target = resolve_existing_path(&root, &relative)?;
    if target.is_dir() {
        fs::remove_dir(&target).map_err(|_| ApiError::bad_request("folder is not empty"))?;
    } else {
        fs::remove_file(&target).map_err(|_| ApiError::internal("failed to delete file"))?;
    }
    record_file_log(
        &ctx,
        &user,
        "delete",
        format!("删除文件：{relative}"),
        Some(200),
        None,
    )
    .await;
    Ok(responses::empty())
}

#[utoipa::path(
    get,
    path = "/api/admin/files/preview",
    tag = "admin-files",
    security(("bearer_auth" = [])),
    params(FilePathParams),
    responses((status = 200, description = "Preview managed file inline"))
)]
#[debug_handler]
pub async fn preview(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<FilePathParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:file:download").await?;
    let response = file_response(&ctx, &user, &params, FileDisposition::Inline).await?;
    Ok(response)
}

#[utoipa::path(
    get,
    path = "/api/admin/files/download",
    tag = "admin-files",
    security(("bearer_auth" = [])),
    params(FilePathParams),
    responses((status = 200, description = "Download managed file"))
)]
#[debug_handler]
pub async fn download(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<FilePathParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:file:download").await?;
    let response = file_response(&ctx, &user, &params, FileDisposition::Attachment).await?;
    Ok(response)
}

async fn file_response(
    ctx: &AppContext,
    user: &crate::models::users::Model,
    params: &FilePathParams,
    disposition: FileDisposition,
) -> ApiResult<Response> {
    let root = find_root(ctx, &params.root_key).await?;
    let relative = normalize_required_path(&params.path)?;
    let target = resolve_existing_path(&root, &relative)?;
    if !target.is_file() {
        return Err(ApiError::bad_request("path is not a file"));
    }
    let bytes = fs::read(&target).map_err(|_| ApiError::bad_request("file content not found"))?;
    let name = file_name(&relative)?;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        mime_type(&relative)
            .unwrap_or("application/octet-stream")
            .parse()
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "{}; filename=\"{}\"",
            disposition.as_header_value(),
            sanitize_filename(&name)
        ))
        .map_err(|_| ApiError::internal("failed to build file response"))?,
    );
    if matches!(disposition, FileDisposition::Inline) {
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("private, no-store"),
        );
        headers.insert(
            HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        );
    }
    record_file_log(
        ctx,
        user,
        disposition.log_action(),
        format!("{}文件：{relative}", disposition.log_name()),
        Some(200),
        None,
    )
    .await;
    Ok((headers, bytes).into_response())
}

#[derive(Clone, Copy)]
enum FileDisposition {
    Attachment,
    Inline,
}

impl FileDisposition {
    const fn as_header_value(self) -> &'static str {
        match self {
            Self::Attachment => "attachment",
            Self::Inline => "inline",
        }
    }

    const fn log_action(self) -> &'static str {
        match self {
            Self::Attachment => "download",
            Self::Inline => "preview",
        }
    }

    const fn log_name(self) -> &'static str {
        match self {
            Self::Attachment => "下载",
            Self::Inline => "预览",
        }
    }
}

async fn file_roots(ctx: &AppContext) -> ApiResult<Vec<FileRootRecord>> {
    let value = system_settings::string_value(&ctx.db, ROOTS_SETTING_KEY, "[]").await?;
    let roots = serde_json::from_str::<Vec<FileRootRecord>>(&value)
        .map_err(|_| ApiError::bad_request("invalid file manager roots setting"))?;
    roots
        .into_iter()
        .filter(|root| root.enabled)
        .map(normalize_root)
        .collect::<ApiResult<Vec<_>>>()
}

async fn find_root(ctx: &AppContext, key: &str) -> ApiResult<FileRootRecord> {
    file_roots(ctx)
        .await?
        .into_iter()
        .find(|root| root.key == key)
        .ok_or_else(|| ApiError::bad_request("file root not found"))
}

fn normalize_root(mut root: FileRootRecord) -> ApiResult<FileRootRecord> {
    root.key = root.key.trim().to_string();
    root.name = root.name.trim().to_string();
    root.url_path = root.url_path.trim().trim_end_matches('/').to_string();
    root.local_root = root.local_root.trim().to_string();
    if root.key.is_empty()
        || root.name.is_empty()
        || root.url_path.is_empty()
        || root.local_root.is_empty()
    {
        return Err(ApiError::bad_request("invalid file root configuration"));
    }
    if !root.url_path.starts_with('/') {
        return Err(ApiError::bad_request(
            "file root url_path must start with /",
        ));
    }
    Ok(root)
}

fn ensure_root_path(root: &FileRootRecord) -> ApiResult<PathBuf> {
    fs::create_dir_all(&root.local_root)
        .map_err(|_| ApiError::internal("failed to prepare file root"))?;
    fs::canonicalize(&root.local_root).map_err(|_| ApiError::bad_request("file root not found"))
}

fn resolve_existing_path(root: &FileRootRecord, relative: &str) -> ApiResult<PathBuf> {
    let root_path = ensure_root_path(root)?;
    let target = if relative.is_empty() {
        root_path.clone()
    } else {
        root_path.join(relative)
    };
    let target =
        fs::canonicalize(target).map_err(|_| ApiError::bad_request("file path not found"))?;
    ensure_under_root(&root_path, &target)?;
    Ok(target)
}

fn resolve_new_path(root: &FileRootRecord, relative: &str) -> ApiResult<PathBuf> {
    let root_path = ensure_root_path(root)?;
    let target = root_path.join(relative);
    let parent = target
        .parent()
        .ok_or_else(|| ApiError::bad_request("invalid file path"))?;
    let parent =
        fs::canonicalize(parent).map_err(|_| ApiError::bad_request("parent path not found"))?;
    ensure_under_root(&root_path, &parent)?;
    Ok(target)
}

fn ensure_under_root(root: &Path, target: &Path) -> ApiResult<()> {
    if target.starts_with(root) {
        Ok(())
    } else {
        Err(ApiError::forbidden("file path is outside configured root"))
    }
}

fn normalize_required_path(path: &str) -> ApiResult<String> {
    let path = normalize_relative_path(Some(path))?;
    if path.is_empty() {
        return Err(ApiError::bad_request("file path is required"));
    }
    Ok(path)
}

fn normalize_relative_path(path: Option<&str>) -> ApiResult<String> {
    let Some(path) = path.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(String::new());
    };
    let path = path.trim_start_matches('/').trim_end_matches('/');
    if path.is_empty() {
        return Ok(String::new());
    }
    if path
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(ApiError::bad_request("invalid file path"));
    }
    if Path::new(path).components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(ApiError::bad_request("invalid file path"));
    }
    Ok(path.to_string())
}

fn file_record(
    root: &FileRootRecord,
    relative: &str,
    name: &str,
    metadata: &fs::Metadata,
) -> ManagedFileRecord {
    ManagedFileRecord {
        name: name.to_string(),
        path: relative.to_string(),
        url: public_url(root, relative),
        is_dir: metadata.is_dir(),
        extension: if metadata.is_file() {
            file_extension(relative)
        } else {
            None
        },
        mime_type: if metadata.is_file() {
            mime_type(relative).map(str::to_string)
        } else {
            None
        },
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

fn public_url(root: &FileRootRecord, relative: &str) -> String {
    if relative.is_empty() {
        return root.url_path.clone();
    }
    format!(
        "{}/{}",
        root.url_path.trim_end_matches('/'),
        relative
            .split('/')
            .map(percent_encode_segment)
            .collect::<Vec<_>>()
            .join("/")
    )
}

fn percent_encode_segment(segment: &str) -> String {
    let mut encoded = String::new();
    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

async fn read_file_field(mut field: Field<'_>, max_bytes: i64) -> ApiResult<Vec<u8>> {
    let max_bytes = usize::try_from(max_bytes.max(1)).unwrap_or(usize::MAX);
    let mut bytes = Vec::new();
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|_| ApiError::bad_request("failed to read uploaded file"))?
    {
        if bytes.len().saturating_add(chunk.len()) > max_bytes {
            return Err(ApiError::bad_request("uploaded file is too large"));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

async fn record_file_log(
    ctx: &AppContext,
    user: &crate::models::users::Model,
    action: &'static str,
    message: impl Into<String>,
    status: Option<i32>,
    error_message: Option<String>,
) {
    let message = message.into();
    admin_logs::record(
        &ctx.db,
        admin_logs::LogInput {
            log_type: "operation",
            level: if error_message.is_some() {
                "error"
            } else {
                "info"
            },
            module: "files",
            action,
            message: &message,
            user_id: Some(user.id),
            operator: Some(user.email.clone()),
            method: None,
            path: Some("/api/admin/files"),
            status,
            error_message,
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

fn parent_relative_path(path: &str) -> Option<String> {
    path.rsplit_once('/').map(|(parent, _)| parent.to_string())
}

fn file_name(path: &str) -> ApiResult<String> {
    path.rsplit('/')
        .next()
        .map(str::to_string)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::bad_request("invalid file path"))
}

fn file_extension(file_name: &str) -> Option<String> {
    file_name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .filter(|extension| !extension.is_empty())
}

fn mime_type(file_name: &str) -> Option<&'static str> {
    match file_extension(file_name).as_deref() {
        Some("txt" | "log") => Some("text/plain; charset=utf-8"),
        Some("md" | "markdown") => Some("text/markdown; charset=utf-8"),
        Some("json") => Some("application/json; charset=utf-8"),
        Some("csv") => Some("text/csv; charset=utf-8"),
        Some("yaml" | "yml") => Some("application/yaml; charset=utf-8"),
        Some("html" | "htm") => Some("text/html; charset=utf-8"),
        Some("css") => Some("text/css; charset=utf-8"),
        Some("js" | "mjs") => Some("text/javascript; charset=utf-8"),
        Some("jpg" | "jpeg") => Some("image/jpeg"),
        Some("png") => Some("image/png"),
        Some("gif") => Some("image/gif"),
        Some("webp") => Some("image/webp"),
        Some("svg") => Some("image/svg+xml"),
        Some("bmp") => Some("image/bmp"),
        Some("pdf") => Some("application/pdf"),
        Some("mp4" | "m4v") => Some("video/mp4"),
        Some("webm") => Some("video/webm"),
        Some("ogv") => Some("video/ogg"),
        Some("mov") => Some("video/quicktime"),
        Some("mp3") => Some("audio/mpeg"),
        Some("wav") => Some("audio/wav"),
        Some("ogg") => Some("audio/ogg"),
        Some("m4a") => Some("audio/mp4"),
        Some("aac") => Some("audio/aac"),
        Some("flac") => Some("audio/flac"),
        _ => None,
    }
}

fn sanitize_filename(file_name: &str) -> String {
    let sanitized = file_name
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() || sanitized == "." || sanitized == ".." {
        "file.bin".to_string()
    } else {
        sanitized
    }
}
