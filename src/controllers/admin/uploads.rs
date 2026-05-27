#![allow(clippy::missing_errors_doc)]

use std::{fs, path::PathBuf};

use axum::{
    extract::Multipart,
    http::{header, HeaderMap, HeaderValue},
    response::IntoResponse,
};
use chrono::Utc;
use loco_rs::prelude::*;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, EntityTrait, IntoActiveModel, PaginatorTrait,
    QueryFilter, QueryOrder, Set,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    controllers::admin::authorize,
    errors::{ApiError, ApiResult},
    models::_entities::{system_settings, upload_files},
    responses::{self, ApiResponse, EmptyData, PageResponse},
};

const STORAGE_ROOT: &str = "storage/uploads";
const DEFAULT_MAX_UPLOAD_MB: i64 = 20;
const DEFAULT_ALLOWED_EXTENSIONS: &str = "jpg,jpeg,png,gif,webp,pdf,txt,csv,xlsx,zip";

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct UploadQueryParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub keyword: Option<String>,
    pub category: Option<String>,
    pub mime_type: Option<String>,
    pub status: Option<String>,
}

impl UploadQueryParams {
    #[must_use]
    pub fn page(&self) -> u64 {
        self.page.unwrap_or(1).max(1)
    }

    #[must_use]
    pub fn page_size(&self) -> u64 {
        self.page_size.unwrap_or(10).clamp(1, 100)
    }
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct UploadRecord {
    pub id: i32,
    pub storage: String,
    pub object_key: String,
    pub url: String,
    pub original_name: String,
    pub filename: String,
    pub extension: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: i64,
    pub sha256: String,
    pub category: Option<String>,
    pub tags: Option<String>,
    pub visibility: String,
    pub status: String,
    pub uploader_id: Option<i32>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct UpdateUploadParams {
    pub category: Option<String>,
    pub tags: Option<String>,
    pub visibility: Option<String>,
    pub status: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/admin/uploads",
    tag = "admin-uploads",
    security(("bearer_auth" = [])),
    params(UploadQueryParams),
    responses((status = 200, body = ApiResponse<PageResponse<UploadRecord>>))
)]
#[debug_handler]
pub async fn list(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<UploadQueryParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:upload:list").await?;

    let page = params.page();
    let page_size = params.page_size();
    let mut query = upload_files::Entity::find().order_by_desc(upload_files::Column::Id);

    if let Some(keyword) = params.keyword.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(
            Condition::any()
                .add(upload_files::Column::OriginalName.contains(keyword))
                .add(upload_files::Column::Filename.contains(keyword))
                .add(upload_files::Column::ObjectKey.contains(keyword))
                .add(upload_files::Column::Category.contains(keyword)),
        );
    }
    if let Some(category) = params.category.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(upload_files::Column::Category.eq(category));
    }
    if let Some(mime_type) = params
        .mime_type
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        query = query.filter(upload_files::Column::MimeType.contains(mime_type));
    }
    if let Some(status) = params.status.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(upload_files::Column::Status.eq(status));
    }

    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(UploadRecord::from)
        .collect();

    Ok(responses::ok(PageResponse {
        items,
        page,
        page_size,
        total,
    }))
}

#[utoipa::path(
    get,
    path = "/api/admin/uploads/{id}",
    tag = "admin-uploads",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<UploadRecord>))
)]
#[debug_handler]
pub async fn get(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:upload:detail").await?;
    let file = find_file(&ctx, id).await?;
    Ok(responses::ok(UploadRecord::from(file)))
}

#[utoipa::path(
    post,
    path = "/api/admin/uploads",
    tag = "admin-uploads",
    security(("bearer_auth" = [])),
    request_body(content = String, content_type = "multipart/form-data"),
    responses((status = 200, body = ApiResponse<UploadRecord>))
)]
#[debug_handler]
pub async fn create(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    mut multipart: Multipart,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:upload:create").await?;
    let mut uploaded = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| ApiError::bad_request("invalid multipart payload"))?
    {
        if field.name() != Some("file") {
            continue;
        }

        let original_name = field.file_name().unwrap_or("upload.bin").to_string();
        let mime_type = field.content_type().map(str::to_string);
        let bytes = field
            .bytes()
            .await
            .map_err(|_| ApiError::bad_request("failed to read uploaded file"))?;
        let max_bytes = upload_limit_bytes(&ctx).await?;
        if i64::try_from(bytes.len()).unwrap_or(i64::MAX) > max_bytes {
            return Err(ApiError::bad_request("uploaded file is too large"));
        }

        let extension = file_extension(&original_name);
        ensure_allowed_extension(&ctx, extension.as_deref()).await?;
        let safe_name = sanitize_filename(&original_name);
        let date_path = Utc::now().format("%Y/%m").to_string();
        let filename = format!("{}-{safe_name}", Uuid::new_v4());
        let object_key = format!("{date_path}/{filename}");
        let physical_path = PathBuf::from(STORAGE_ROOT).join(&object_key);
        if let Some(parent) = physical_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|_| ApiError::internal("failed to prepare upload storage"))?;
        }
        fs::write(&physical_path, &bytes)
            .map_err(|_| ApiError::internal("failed to save uploaded file"))?;

        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let sha256 = hex::encode(hasher.finalize());
        let file = upload_files::ActiveModel {
            storage: Set("local".to_string()),
            object_key: Set(object_key),
            url: Set(String::new()),
            original_name: Set(original_name),
            filename: Set(filename),
            extension: Set(extension),
            mime_type: Set(mime_type),
            size_bytes: Set(i64::try_from(bytes.len()).unwrap_or(i64::MAX)),
            sha256: Set(sha256),
            visibility: Set("private".to_string()),
            status: Set("active".to_string()),
            uploader_id: Set(Some(user.id)),
            ..Default::default()
        }
        .insert(&ctx.db)
        .await?;

        let mut active = file.clone().into_active_model();
        active.url = Set(format!("/api/admin/uploads/{}/download", file.id));
        let file = active.update(&ctx.db).await?;
        uploaded = Some(file);
        break;
    }

    let file = uploaded.ok_or_else(|| ApiError::bad_request("file field is required"))?;
    Ok(responses::ok(UploadRecord::from(file)))
}

#[utoipa::path(
    put,
    path = "/api/admin/uploads/{id}",
    tag = "admin-uploads",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = UpdateUploadParams,
    responses((status = 200, body = ApiResponse<UploadRecord>))
)]
#[debug_handler]
pub async fn update(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<UpdateUploadParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:upload:update").await?;
    validate_visibility(params.visibility.as_deref())?;
    validate_status(params.status.as_deref())?;
    let file = find_file(&ctx, id).await?;

    let mut active = file.into_active_model();
    active.category = Set(params.category);
    active.tags = Set(params.tags);
    active.visibility = Set(params.visibility.unwrap_or_else(|| "private".to_string()));
    active.status = Set(params.status.unwrap_or_else(|| "active".to_string()));
    let file = active.update(&ctx.db).await?;

    Ok(responses::ok(UploadRecord::from(file)))
}

#[utoipa::path(
    delete,
    path = "/api/admin/uploads/{id}",
    tag = "admin-uploads",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn delete(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:upload:delete").await?;
    let file = find_file(&ctx, id).await?;
    let mut active = file.into_active_model();
    active.status = Set("deleted".to_string());
    active.update(&ctx.db).await?;
    Ok(responses::empty())
}

#[utoipa::path(
    get,
    path = "/api/admin/uploads/{id}/download",
    tag = "admin-uploads",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, description = "Download uploaded file"))
)]
#[debug_handler]
pub async fn download(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:upload:download").await?;
    let file = find_file(&ctx, id).await?;
    if file.status != "active" {
        return Err(ApiError::bad_request("file is not active"));
    }

    let physical_path = PathBuf::from(STORAGE_ROOT).join(&file.object_key);
    let bytes =
        fs::read(physical_path).map_err(|_| ApiError::bad_request("file content not found"))?;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        file.mime_type
            .as_deref()
            .unwrap_or("application/octet-stream")
            .parse()
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    let disposition = format!(
        "attachment; filename=\"{}\"",
        sanitize_filename(&file.original_name)
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&disposition)
            .map_err(|_| ApiError::internal("failed to build download response"))?,
    );

    Ok((headers, bytes).into_response())
}

async fn find_file(ctx: &AppContext, id: i32) -> ApiResult<upload_files::Model> {
    upload_files::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("file not found"))
}

async fn upload_limit_bytes(ctx: &AppContext) -> ApiResult<i64> {
    let max_mb = system_settings::Entity::find()
        .filter(system_settings::Column::Key.eq("upload.max_size_mb"))
        .one(&ctx.db)
        .await?
        .and_then(|setting| setting.value.parse::<i64>().ok())
        .unwrap_or(DEFAULT_MAX_UPLOAD_MB);
    Ok(max_mb.max(1) * 1024 * 1024)
}

async fn ensure_allowed_extension(ctx: &AppContext, extension: Option<&str>) -> ApiResult<()> {
    let Some(extension) = extension else {
        return Err(ApiError::bad_request("uploaded file extension is required"));
    };
    let allowed = system_settings::Entity::find()
        .filter(system_settings::Column::Key.eq("upload.allowed_extensions"))
        .one(&ctx.db)
        .await?
        .map_or_else(
            || DEFAULT_ALLOWED_EXTENSIONS.to_string(),
            |setting| setting.value,
        );
    let allowed = allowed
        .split(',')
        .map(|value| value.trim().to_ascii_lowercase())
        .collect::<Vec<_>>();
    if allowed.iter().any(|value| value == extension) {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "uploaded file extension is not allowed",
        ))
    }
}

fn validate_visibility(visibility: Option<&str>) -> ApiResult<()> {
    match visibility.unwrap_or("private") {
        "private" | "public" => Ok(()),
        _ => Err(ApiError::bad_request("invalid file visibility")),
    }
}

fn validate_status(status: Option<&str>) -> ApiResult<()> {
    match status.unwrap_or("active") {
        "active" | "deleted" => Ok(()),
        _ => Err(ApiError::bad_request("invalid file status")),
    }
}

fn file_extension(file_name: &str) -> Option<String> {
    file_name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .filter(|extension| !extension.is_empty())
}

fn sanitize_filename(file_name: &str) -> String {
    let sanitized = file_name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "upload.bin".to_string()
    } else {
        sanitized
    }
}

impl From<upload_files::Model> for UploadRecord {
    fn from(file: upload_files::Model) -> Self {
        Self {
            id: file.id,
            storage: file.storage,
            object_key: file.object_key,
            url: file.url,
            original_name: file.original_name,
            filename: file.filename,
            extension: file.extension,
            mime_type: file.mime_type,
            size_bytes: file.size_bytes,
            sha256: file.sha256,
            category: file.category,
            tags: file.tags,
            visibility: file.visibility,
            status: file.status,
            uploader_id: file.uploader_id,
            created_at: file.created_at.to_rfc3339(),
            updated_at: file.updated_at.to_rfc3339(),
        }
    }
}
