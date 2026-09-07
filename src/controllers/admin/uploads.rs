#![allow(clippy::missing_errors_doc)]

use std::{collections::VecDeque, fs, path::PathBuf};

use axum::{
    extract::{multipart::Field, Multipart},
    http::{header, HeaderMap, HeaderName, HeaderValue},
};
use chrono::{Local, Utc};
use futures_util::TryStreamExt;
use loco_rs::prelude::*;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, EntityTrait, IntoActiveModel, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, Set,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    controllers::admin::{authorize, storage_profiles},
    errors::{ApiError, ApiResult},
    models::{
        _entities::{
            storage_buckets, storage_profiles as storage_profiles_entity, upload_files,
            upload_tasks,
        },
        admin_logs, system_settings,
    },
    responses::{self, ApiResponse, EmptyData, PageResponse},
    services::{
        file_stream,
        storage::{self, StorageObjectRecord, StoragePrefixRecord},
    },
};

const MAX_EDITABLE_UPLOAD_BYTES: usize = 1024 * 1024;
const TEXT_ENCODING_SAMPLE_BYTES: usize = 64 * 1024;
const EDITABLE_UPLOAD_EXTENSIONS: &[&str] = &[
    "bash",
    "c",
    "cc",
    "cfg",
    "cjs",
    "conf",
    "config",
    "cpp",
    "cs",
    "css",
    "csv",
    "cts",
    "cxx",
    "dockerignore",
    "editorconfig",
    "env",
    "eslintrc",
    "fish",
    "gitignore",
    "go",
    "h",
    "hpp",
    "htm",
    "html",
    "ini",
    "java",
    "js",
    "json",
    "jsx",
    "kt",
    "kts",
    "less",
    "lock",
    "log",
    "lua",
    "markdown",
    "md",
    "mjs",
    "mts",
    "npmrc",
    "php",
    "prettierrc",
    "properties",
    "py",
    "rb",
    "rs",
    "sass",
    "scss",
    "sh",
    "sql",
    "svelte",
    "svg",
    "toml",
    "ts",
    "tsx",
    "txt",
    "vue",
    "xml",
    "yaml",
    "yml",
    "zsh",
];
const EDITABLE_UPLOAD_FILENAMES: &[&str] =
    &["dockerfile", "license", "makefile", "procfile", "readme"];

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct UploadQueryParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub keyword: Option<String>,
    pub category: Option<String>,
    pub mime_type: Option<String>,
    pub status: Option<String>,
    pub storage_profile_id: Option<i32>,
    pub storage_bucket_id: Option<i32>,
    pub bucket: Option<String>,
    pub prefix: Option<String>,
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

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct UploadBrowserParams {
    pub storage_bucket_id: Option<i32>,
    pub prefix: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct UploadRecord {
    pub id: i32,
    pub storage: String,
    pub storage_profile_id: Option<i32>,
    pub storage_bucket_id: Option<i32>,
    pub bucket: Option<String>,
    pub prefix: Option<String>,
    pub etag: Option<String>,
    pub object_key: String,
    pub url: String,
    pub public_url: Option<String>,
    pub download_url: String,
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
pub struct UploadBrowserRecord {
    pub prefixes: Vec<StoragePrefixRecord>,
    pub objects: Vec<StorageObjectRecord>,
    pub managed_object_keys: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct UploadContentRecord {
    pub content: String,
    pub size_bytes: i64,
    pub updated_at: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct UpdateUploadContentParams {
    pub content: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct FileApiUploadForm {
    #[schema(value_type = String, format = Binary)]
    pub file: String,
    pub storage_profile_id: Option<i32>,
    pub storage_bucket_id: Option<i32>,
    pub prefix: Option<String>,
    pub category: Option<String>,
    pub tags: Option<String>,
    pub visibility: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct FileApiUploadRecord {
    pub id: i32,
    pub original_name: String,
    pub filename: String,
    pub mime_type: Option<String>,
    pub size_bytes: i64,
    pub sha256: String,
    pub storage_provider: String,
    pub storage_profile_id: i32,
    pub storage_bucket_id: i32,
    pub storage_bucket: String,
    pub prefix: Option<String>,
    pub object_key: String,
    pub network_url: String,
    pub download_url: String,
    pub public_url: Option<String>,
    pub local_path: Option<String>,
    pub created_at: String,
}

pub(crate) struct StoreFileApiBytesParams {
    pub original_name: String,
    pub mime_type: Option<String>,
    pub bytes: Vec<u8>,
    pub storage_bucket_id: Option<i32>,
    pub prefix: Option<String>,
    pub category: Option<String>,
    pub tags: Option<String>,
    pub visibility: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct UpdateUploadParams {
    pub category: Option<Option<String>>,
    pub tags: Option<Option<String>>,
    pub visibility: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct ImportUploadObjectParams {
    pub storage_bucket_id: i32,
    pub object_key: String,
    pub original_name: Option<String>,
    pub mime_type: Option<String>,
    pub category: Option<String>,
    pub tags: Option<String>,
    pub visibility: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct ImportUploadObjectsParams {
    pub storage_bucket_id: i32,
    pub prefix: Option<String>,
    pub visibility: Option<String>,
    pub category: Option<String>,
    pub tags: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct ImportUploadObjectsRecord {
    pub imported: usize,
    pub skipped: usize,
    pub items: Vec<UploadRecord>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CreateUploadFolderParams {
    pub storage_bucket_id: i32,
    pub prefix: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CreateUploadFileParams {
    pub storage_bucket_id: i32,
    pub prefix: Option<String>,
    pub name: String,
    #[serde(default)]
    pub content: String,
    pub visibility: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct RenameUploadParams {
    pub original_name: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CreateUploadTaskParams {
    pub storage_bucket_id: i32,
    pub original_name: String,
    pub mime_type: Option<String>,
    pub size_bytes: i64,
    pub chunk_size: i64,
    pub total_chunks: i32,
    pub prefix: Option<String>,
    pub category: Option<String>,
    pub tags: Option<String>,
    pub visibility: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct UploadTaskRecord {
    pub id: i32,
    pub storage: String,
    pub storage_profile_id: Option<i32>,
    pub storage_bucket_id: Option<i32>,
    pub bucket: Option<String>,
    pub prefix: Option<String>,
    pub object_key: String,
    pub original_name: String,
    pub filename: String,
    pub extension: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: i64,
    pub chunk_size: i64,
    pub total_chunks: i32,
    pub uploaded_chunks: Vec<i32>,
    pub uploaded_bytes: i64,
    pub category: Option<String>,
    pub tags: Option<String>,
    pub visibility: String,
    pub status: String,
    pub error_message: Option<String>,
    pub completed_at: Option<String>,
    pub upload_file_id: Option<i32>,
    pub uploader_id: Option<i32>,
    pub created_at: String,
    pub updated_at: String,
}

#[utoipa::path(
    get,
    path = "/api/admin/uploads/tasks",
    tag = "admin-uploads",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<Vec<UploadTaskRecord>>))
)]
#[debug_handler]
pub async fn list_tasks(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:upload:list").await?;
    let tasks = upload_tasks::Entity::find()
        .filter(upload_tasks::Column::UploaderId.eq(user.id))
        .order_by_desc(upload_tasks::Column::Id)
        .limit(50)
        .all(&ctx.db)
        .await?
        .into_iter()
        .map(UploadTaskRecord::from)
        .collect::<Vec<_>>();
    Ok(responses::ok(tasks))
}

#[utoipa::path(
    post,
    path = "/api/admin/uploads/tasks",
    tag = "admin-uploads",
    security(("bearer_auth" = [])),
    request_body = CreateUploadTaskParams,
    responses((status = 200, body = ApiResponse<UploadTaskRecord>))
)]
#[debug_handler]
pub async fn create_task(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<CreateUploadTaskParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:upload:create").await?;
    validate_visibility(params.visibility.as_deref())?;
    if params.size_bytes <= 0 || params.chunk_size <= 0 || params.total_chunks <= 0 {
        return Err(ApiError::bad_request("invalid upload task size"));
    }
    let max_bytes = upload_limit_bytes(&ctx).await?;
    if params.size_bytes > max_bytes {
        return Err(ApiError::bad_request(
            system_settings::upload_limit_error_message(max_bytes),
        ));
    }
    let extension = file_extension(&params.original_name);
    ensure_allowed_extension(&ctx, extension.as_deref()).await?;
    let safe_name = sanitize_filename(&params.original_name);
    let filename = format!("{}-{safe_name}", Uuid::new_v4());
    let prefix = if let Some(prefix) = params.prefix.as_deref() {
        storage::normalize_prefix(Some(prefix))?
    } else {
        Some(format!("{}/", Utc::now().format("%Y/%m")))
    };
    let original_name = params.original_name.clone();
    let (profile, bucket) =
        storage_profiles::resolve_bucket(&ctx, &user, Some(params.storage_bucket_id)).await?;
    let object_key = join_object_key(prefix.as_deref(), &filename);
    let task = upload_tasks::ActiveModel {
        storage: Set(profile.provider),
        storage_profile_id: Set(Some(profile.id)),
        storage_bucket_id: Set(Some(bucket.id)),
        bucket: Set(Some(bucket.bucket)),
        prefix: Set(prefix),
        object_key: Set(object_key),
        original_name: Set(params.original_name),
        filename: Set(filename),
        extension: Set(extension),
        mime_type: Set(params.mime_type),
        size_bytes: Set(params.size_bytes),
        chunk_size: Set(params.chunk_size),
        total_chunks: Set(params.total_chunks),
        uploaded_chunks: Set("[]".to_string()),
        uploaded_bytes: Set(0),
        sha256: Set(None),
        category: Set(params.category),
        tags: Set(params.tags),
        visibility: Set(params.visibility.unwrap_or_else(|| "private".to_string())),
        status: Set("pending".to_string()),
        error_message: Set(None),
        completed_at: Set(None),
        upload_file_id: Set(None),
        uploader_id: Set(Some(user.id)),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;
    prepare_task_dir(task.id)?;
    record_upload_log(
        &ctx,
        &user,
        "create_task",
        format!("创建素材分片上传任务：{original_name}"),
        Some(200),
        None,
    )
    .await;
    Ok(responses::ok(UploadTaskRecord::from(task)))
}

#[utoipa::path(
    post,
    path = "/api/admin/uploads/tasks/{id}/chunks/{chunk_index}",
    tag = "admin-uploads",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path), ("chunk_index" = i32, Path)),
    request_body(content = String, content_type = "multipart/form-data"),
    responses((status = 200, body = ApiResponse<UploadTaskRecord>))
)]
#[debug_handler]
pub async fn upload_task_chunk(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path((id, chunk_index)): Path<(i32, i32)>,
    mut multipart: Multipart,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:upload:create").await?;
    let task = find_task(&ctx, &user, id).await?;
    if task.status == "completed" {
        return Ok(responses::ok(UploadTaskRecord::from(task)));
    }
    if chunk_index < 0 || chunk_index >= task.total_chunks {
        return Err(ApiError::bad_request("invalid chunk index"));
    }
    let mut chunk = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| ApiError::bad_request("invalid multipart payload"))?
    {
        if field.name().unwrap_or_default() == "chunk" {
            chunk = Some(read_upload_field(field, task.chunk_size).await?);
        }
    }
    let chunk = chunk.ok_or_else(|| ApiError::bad_request("chunk field is required"))?;
    save_task_chunk(task.id, chunk_index, &chunk)?;
    let mut uploaded_chunks = parse_uploaded_chunks(&task.uploaded_chunks);
    if !uploaded_chunks.contains(&chunk_index) {
        uploaded_chunks.push(chunk_index);
        uploaded_chunks.sort_unstable();
    }
    let uploaded_bytes = uploaded_chunks
        .iter()
        .map(|index| task_chunk_size(task.id, *index))
        .sum::<ApiResult<i64>>()?;
    let mut active = task.into_active_model();
    active.uploaded_chunks = Set(serialize_uploaded_chunks(&uploaded_chunks));
    active.uploaded_bytes = Set(uploaded_bytes);
    active.status = Set("uploading".to_string());
    active.error_message = Set(None);
    let task = active.update(&ctx.db).await?;
    Ok(responses::ok(UploadTaskRecord::from(task)))
}

#[utoipa::path(
    post,
    path = "/api/admin/uploads/tasks/{id}/complete",
    tag = "admin-uploads",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<UploadTaskRecord>))
)]
#[debug_handler]
pub async fn complete_task(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:upload:create").await?;
    let task = find_task(&ctx, &user, id).await?;
    if task.status == "completed" {
        return Ok(responses::ok(UploadTaskRecord::from(task)));
    }
    let uploaded_chunks = parse_uploaded_chunks(&task.uploaded_chunks);
    if uploaded_chunks.len() != usize::try_from(task.total_chunks).unwrap_or_default() {
        return Err(ApiError::bad_request("upload task chunks are incomplete"));
    }

    let mut active = task.into_active_model();
    active.status = Set("importing".to_string());
    active.error_message = Set(None);
    let task = active.update(&ctx.db).await?;
    record_upload_log(
        &ctx,
        &user,
        "complete_task_queued",
        format!("提交分片上传入库任务：{}", task.original_name),
        Some(202),
        None,
    )
    .await;

    let worker_ctx = ctx.clone();
    let worker_user = user.clone();
    let worker_task = task.clone();
    tokio::spawn(async move {
        finish_upload_task_in_background(worker_ctx, worker_user, worker_task).await;
    });

    Ok(responses::ok(UploadTaskRecord::from(task)))
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
                .add(upload_files::Column::Category.contains(keyword))
                .add(upload_files::Column::Bucket.contains(keyword)),
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
    if let Some(storage_profile_id) = params.storage_profile_id {
        query = query.filter(upload_files::Column::StorageProfileId.eq(storage_profile_id));
    }
    if let Some(storage_bucket_id) = params.storage_bucket_id {
        query = query.filter(upload_files::Column::StorageBucketId.eq(storage_bucket_id));
    }
    if let Some(bucket) = params.bucket.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(upload_files::Column::Bucket.eq(bucket));
    }
    if let Some(prefix) = params.prefix.as_deref().filter(|value| !value.is_empty()) {
        query =
            query.filter(upload_files::Column::Prefix.eq(storage::normalize_prefix(Some(prefix))?));
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
    multipart: Multipart,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:upload:create").await?;
    let payload = read_upload_payload(&ctx, multipart).await?;
    let upload = store_upload(&ctx, &user, payload).await?;
    record_upload_log(
        &ctx,
        &user,
        "upload",
        format!("上传素材成功：{}", upload.file.original_name),
        Some(200),
        None,
    )
    .await;
    Ok(responses::ok(UploadRecord::from(upload.file)))
}

#[utoipa::path(
    post,
    path = "/api/file-api/uploads",
    tag = "file-api",
    security(("bearer_auth" = [])),
    request_body(content = inline(FileApiUploadForm), content_type = "multipart/form-data"),
    responses((status = 200, body = ApiResponse<FileApiUploadRecord>))
)]
#[debug_handler]
pub async fn file_api_create(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    multipart: Multipart,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:upload:create").await?;
    let payload = read_upload_payload(&ctx, multipart).await?;
    let upload = store_upload(&ctx, &user, payload).await?;
    let record = file_api_upload_record(&ctx, &headers, &upload)?;
    record_upload_log_at(
        &ctx,
        &user,
        "api_upload",
        format!("接口上传文件成功：{}", upload.file.original_name),
        Some(200),
        None,
        "/api/file-api/uploads",
    )
    .await;
    Ok(responses::ok(record))
}

pub(crate) async fn store_file_api_bytes(
    ctx: &AppContext,
    user: &crate::models::users::Model,
    headers: &HeaderMap,
    params: StoreFileApiBytesParams,
) -> ApiResult<FileApiUploadRecord> {
    validate_visibility(params.visibility.as_deref())?;
    let upload = store_upload(
        ctx,
        user,
        UploadPayload {
            storage_profile_id: None,
            storage_bucket_id: params.storage_bucket_id,
            prefix: storage::normalize_prefix(params.prefix.as_deref())?,
            category: params.category,
            tags: params.tags,
            visibility: params.visibility,
            file: Some(UploadedPart {
                original_name: params.original_name,
                mime_type: params.mime_type,
                bytes: params.bytes,
            }),
        },
    )
    .await?;
    file_api_upload_record(ctx, headers, &upload)
}

fn file_api_upload_record(
    ctx: &AppContext,
    headers: &HeaderMap,
    upload: &StoredUpload,
) -> ApiResult<FileApiUploadRecord> {
    let download_url = absolute_network_url(
        ctx,
        headers,
        &format!("/api/admin/uploads/{}/download", upload.file.id),
    );
    let public_url = public_upload_url(&upload.file)
        .as_deref()
        .map(|url| absolute_network_url(ctx, headers, url));
    let network_url = public_url.clone().unwrap_or_else(|| download_url.clone());
    let local_path =
        storage::object_local_path(&upload.profile, &upload.bucket, &upload.file.object_key)?
            .map(|path| path.to_string_lossy().into_owned());
    Ok(FileApiUploadRecord {
        id: upload.file.id,
        original_name: upload.file.original_name.clone(),
        filename: upload.file.filename.clone(),
        mime_type: upload.file.mime_type.clone(),
        size_bytes: upload.file.size_bytes,
        sha256: upload.file.sha256.clone(),
        storage_provider: upload.profile.provider.clone(),
        storage_profile_id: upload.profile.id,
        storage_bucket_id: upload.bucket.id,
        storage_bucket: upload.bucket.bucket.clone(),
        prefix: upload.file.prefix.clone(),
        object_key: upload.file.object_key.clone(),
        network_url,
        download_url,
        public_url,
        local_path,
        created_at: upload.file.created_at.to_rfc3339(),
    })
}

#[utoipa::path(
    post,
    path = "/api/admin/uploads/import-object",
    tag = "admin-uploads",
    security(("bearer_auth" = [])),
    request_body = ImportUploadObjectParams,
    responses((status = 200, body = ApiResponse<UploadRecord>))
)]
#[debug_handler]
pub async fn import_object(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<ImportUploadObjectParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:upload:create").await?;
    validate_visibility(params.visibility.as_deref())?;
    let object_key = storage::normalize_object_key(&params.object_key)?;
    if upload_files::Entity::find()
        .filter(upload_files::Column::StorageBucketId.eq(params.storage_bucket_id))
        .filter(upload_files::Column::ObjectKey.eq(&object_key))
        .one(&ctx.db)
        .await?
        .is_some()
    {
        return Err(ApiError::bad_request("object already imported"));
    }

    let (profile, bucket) =
        storage_profiles::resolve_bucket(&ctx, &user, Some(params.storage_bucket_id)).await?;
    let file = import_external_object(
        &ctx,
        &user,
        &profile,
        &bucket,
        ImportExternalObjectInput {
            object_key,
            original_name: params.original_name,
            mime_type: params.mime_type,
            category: params.category,
            tags: params.tags,
            visibility: params.visibility,
        },
    )
    .await?;
    let file = ensure_access_url(&ctx, file).await?;
    record_upload_log(
        &ctx,
        &user,
        "import_object",
        format!("外部对象入库成功：{}", file.original_name),
        Some(200),
        None,
    )
    .await;
    Ok(responses::ok(UploadRecord::from(file)))
}

#[utoipa::path(
    post,
    path = "/api/admin/uploads/import-objects",
    tag = "admin-uploads",
    security(("bearer_auth" = [])),
    request_body = ImportUploadObjectsParams,
    responses((status = 200, body = ApiResponse<ImportUploadObjectsRecord>))
)]
#[debug_handler]
pub async fn import_objects(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<ImportUploadObjectsParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:upload:create").await?;
    validate_visibility(params.visibility.as_deref())?;
    let (profile, bucket) =
        storage_profiles::resolve_bucket(&ctx, &user, Some(params.storage_bucket_id)).await?;
    let mut pending_prefixes = VecDeque::from([params.prefix.clone()]);
    let mut imported = 0;
    let mut skipped = 0;
    let mut items = Vec::new();
    while let Some(current_prefix) = pending_prefixes.pop_front() {
        let browser = storage::list_objects(&profile, &bucket, current_prefix.as_deref()).await?;
        pending_prefixes.extend(
            browser
                .prefixes
                .into_iter()
                .map(|prefix| Some(prefix.prefix)),
        );
        for object in browser.objects {
            if is_folder_marker_object(&object.key) {
                skipped += 1;
                continue;
            }
            if upload_files::Entity::find()
                .filter(upload_files::Column::StorageBucketId.eq(bucket.id))
                .filter(upload_files::Column::ObjectKey.eq(&object.key))
                .one(&ctx.db)
                .await?
                .is_some()
            {
                skipped += 1;
                continue;
            }
            match import_external_object(
                &ctx,
                &user,
                &profile,
                &bucket,
                ImportExternalObjectInput {
                    object_key: object.key,
                    original_name: Some(object.name),
                    mime_type: None,
                    category: params.category.clone(),
                    tags: params.tags.clone(),
                    visibility: params.visibility.clone(),
                },
            )
            .await
            {
                Ok(file) => {
                    imported += 1;
                    items.push(UploadRecord::from(ensure_access_url(&ctx, file).await?));
                }
                Err(_) => skipped += 1,
            }
        }
    }
    record_upload_log(
        &ctx,
        &user,
        "import_objects",
        format!("批量入库外部对象：成功 {imported} 个，跳过 {skipped} 个"),
        Some(200),
        None,
    )
    .await;
    Ok(responses::ok(ImportUploadObjectsRecord {
        imported,
        skipped,
        items,
    }))
}

#[utoipa::path(
    post,
    path = "/api/admin/uploads/folders",
    tag = "admin-uploads",
    security(("bearer_auth" = [])),
    request_body = CreateUploadFolderParams,
    responses((status = 200, body = ApiResponse<StoragePrefixRecord>))
)]
#[debug_handler]
pub async fn create_folder(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<CreateUploadFolderParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:upload:create").await?;
    let (profile, bucket) =
        storage_profiles::resolve_bucket(&ctx, &user, Some(params.storage_bucket_id)).await?;
    let folder = storage::create_folder(&profile, &bucket, &params.prefix).await?;
    record_upload_log(
        &ctx,
        &user,
        "create_folder",
        format!("创建素材目录：{}", folder.prefix),
        Some(200),
        None,
    )
    .await;
    Ok(responses::ok(folder))
}

#[utoipa::path(
    post,
    path = "/api/admin/uploads/files",
    tag = "admin-uploads",
    security(("bearer_auth" = [])),
    request_body = CreateUploadFileParams,
    responses((status = 200, body = ApiResponse<UploadRecord>))
)]
#[debug_handler]
pub async fn create_file(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<CreateUploadFileParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:upload:create").await?;
    let name = params.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("file name is required"));
    }
    if matches!(name, "." | "..") || name.contains('/') || name.contains('\\') {
        return Err(ApiError::bad_request("invalid file name"));
    }
    validate_visibility(params.visibility.as_deref())?;
    let normalized_prefix = storage::normalize_prefix(params.prefix.as_deref())?;
    let mime_type = mime_guess::from_path(name)
        .first_raw()
        .map_or_else(|| "text/plain".to_string(), str::to_string);
    let upload = store_upload(
        &ctx,
        &user,
        UploadPayload {
            storage_profile_id: None,
            storage_bucket_id: Some(params.storage_bucket_id),
            // Some("") keeps root-level files at the root instead of the dated upload prefix.
            prefix: Some(normalized_prefix.unwrap_or_default()),
            category: None,
            tags: None,
            visibility: params.visibility,
            file: Some(UploadedPart {
                original_name: name.to_string(),
                mime_type: Some(mime_type),
                bytes: params.content.into_bytes(),
            }),
        },
    )
    .await?;
    record_upload_log(
        &ctx,
        &user,
        "create_file",
        format!("新建素材文件：{}", upload.file.original_name),
        Some(200),
        None,
    )
    .await;
    Ok(responses::ok(UploadRecord::from(upload.file)))
}

#[utoipa::path(
    put,
    path = "/api/admin/uploads/{id}/rename",
    tag = "admin-uploads",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = RenameUploadParams,
    responses((status = 200, body = ApiResponse<UploadRecord>))
)]
#[debug_handler]
pub async fn rename(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<RenameUploadParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:upload:update").await?;
    let original_name = params.original_name.trim();
    if original_name.is_empty() {
        return Err(ApiError::bad_request("file name is required"));
    }
    let file = find_file(&ctx, id).await?;
    if file.status != "active" {
        return Err(ApiError::bad_request("file is not active"));
    }
    let extension = file_extension(original_name);
    ensure_allowed_extension(&ctx, extension.as_deref()).await?;
    let filename = sanitize_filename(original_name);
    let target_key = join_object_key(file.prefix.as_deref(), &filename);
    if target_key != file.object_key {
        if upload_files::Entity::find()
            .filter(upload_files::Column::StorageBucketId.eq(file.storage_bucket_id))
            .filter(upload_files::Column::ObjectKey.eq(&target_key))
            .filter(upload_files::Column::Id.ne(file.id))
            .one(&ctx.db)
            .await?
            .is_some()
        {
            return Err(ApiError::bad_request("target object already imported"));
        }
        let (profile, bucket) = file_storage_location(&ctx, &user, &file).await?;
        if storage::object_metadata(&profile, &bucket, &target_key)
            .await
            .is_ok()
        {
            return Err(ApiError::bad_request("target object already exists"));
        }
        let stored =
            storage::rename_object(&profile, &bucket, &file.object_key, &target_key).await?;
        let mut active = file.into_active_model();
        active.object_key = Set(stored.object_key);
        active.url = Set(stored.url);
        active.bucket = Set(Some(stored.bucket));
        active.prefix = Set(stored.prefix);
        active.etag = Set(stored.etag);
        active.original_name = Set(original_name.to_string());
        active.filename = Set(filename);
        active.extension = Set(extension);
        let file = active.update(&ctx.db).await?;
        let file = ensure_access_url(&ctx, file).await?;
        record_upload_log(
            &ctx,
            &user,
            "rename",
            format!("重命名素材：{}", file.original_name),
            Some(200),
            None,
        )
        .await;
        return Ok(responses::ok(UploadRecord::from(file)));
    }
    let mut active = file.into_active_model();
    active.original_name = Set(original_name.to_string());
    active.filename = Set(filename);
    active.extension = Set(extension);
    let file = active.update(&ctx.db).await?;
    let file = ensure_access_url(&ctx, file).await?;
    record_upload_log(
        &ctx,
        &user,
        "rename",
        format!("重命名素材：{}", file.original_name),
        Some(200),
        None,
    )
    .await;
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
    let user = authorize(&ctx, &auth, "system:upload:update").await?;
    validate_visibility(params.visibility.as_deref())?;
    validate_status(params.status.as_deref())?;
    let file = find_file(&ctx, id).await?;

    let mut active = file.into_active_model();
    if let Some(category) = params.category {
        active.category = Set(normalize_optional_text(category));
    }
    if let Some(tags) = params.tags {
        active.tags = Set(normalize_optional_text(tags));
    }
    if let Some(visibility) = params.visibility {
        active.visibility = Set(visibility);
    }
    if let Some(status) = params.status {
        active.status = Set(status);
    }
    let file = active.update(&ctx.db).await?;
    let file = ensure_access_url(&ctx, file).await?;
    record_upload_log(
        &ctx,
        &user,
        "update",
        format!("更新素材信息：{}", file.original_name),
        Some(200),
        None,
    )
    .await;

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
    let user = authorize(&ctx, &auth, "system:upload:delete").await?;
    let file = find_file(&ctx, id).await?;
    let original_name = file.original_name.clone();
    let mut active = file.into_active_model();
    active.status = Set("deleted".to_string());
    active.update(&ctx.db).await?;
    record_upload_log(
        &ctx,
        &user,
        "delete",
        format!("删除素材：{original_name}"),
        Some(200),
        None,
    )
    .await;
    Ok(responses::empty())
}

#[utoipa::path(
    get,
    path = "/api/admin/uploads/browser",
    tag = "admin-uploads",
    security(("bearer_auth" = [])),
    params(UploadBrowserParams),
    responses((status = 200, body = ApiResponse<UploadBrowserRecord>))
)]
#[debug_handler]
pub async fn browser(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<UploadBrowserParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:upload:list").await?;
    let (profile, bucket) =
        storage_profiles::resolve_bucket(&ctx, &user, params.storage_bucket_id).await?;
    let browser = storage::list_objects(&profile, &bucket, params.prefix.as_deref()).await?;
    let object_keys = browser
        .objects
        .iter()
        .map(|object| object.key.clone())
        .collect::<Vec<_>>();
    let managed_object_keys = if object_keys.is_empty() {
        Vec::new()
    } else {
        upload_files::Entity::find()
            .filter(upload_files::Column::ObjectKey.is_in(object_keys))
            .all(&ctx.db)
            .await?
            .into_iter()
            .map(|file| file.object_key)
            .collect()
    };
    Ok(responses::ok(UploadBrowserRecord {
        prefixes: browser.prefixes,
        objects: browser.objects,
        managed_object_keys,
    }))
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
    headers: HeaderMap,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:upload:download").await?;
    let file = find_file(&ctx, id).await?;
    if file.status != "active" {
        return Err(ApiError::bad_request("file is not active"));
    }
    let (profile, bucket) = file_storage_location(&ctx, &actor, &file).await?;
    let response = file_content_response(
        &profile,
        &bucket,
        &headers,
        &file,
        FileDisposition::Attachment,
        FileResponseScope::Private,
    )
    .await?;
    record_upload_log(
        &ctx,
        &actor,
        "download",
        format!("下载素材：{}", file.original_name),
        Some(200),
        None,
    )
    .await;

    Ok(response)
}

#[utoipa::path(
    get,
    path = "/api/admin/uploads/{id}/preview",
    tag = "admin-uploads",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, description = "Preview uploaded file inline"))
)]
#[debug_handler]
pub async fn preview(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:upload:download").await?;
    let file = find_file(&ctx, id).await?;
    if file.status != "active" {
        return Err(ApiError::bad_request("file is not active"));
    }
    let (profile, bucket) = file_storage_location(&ctx, &actor, &file).await?;
    let response = file_content_response(
        &profile,
        &bucket,
        &headers,
        &file,
        FileDisposition::Inline,
        FileResponseScope::Private,
    )
    .await?;
    record_upload_log(
        &ctx,
        &actor,
        "preview",
        format!("预览素材：{}", file.original_name),
        Some(200),
        None,
    )
    .await;

    Ok(response)
}

#[utoipa::path(
    get,
    path = "/api/uploads/{id}",
    tag = "uploads",
    params(("id" = i32, Path)),
    responses(
        (status = 200, description = "Preview a public uploaded file inline"),
        (status = 206, description = "Preview a byte range of a public uploaded file"),
        (status = 403, description = "File is not public")
    )
)]
#[debug_handler]
pub async fn public_file(
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let file = find_file(&ctx, id).await?;
    if file.status != "active" || file.visibility != "public" {
        return Err(ApiError::forbidden("file is not publicly available"));
    }
    let (profile, bucket) = public_file_storage_location(&ctx, &file).await?;
    file_content_response(
        &profile,
        &bucket,
        &headers,
        &file,
        FileDisposition::Inline,
        FileResponseScope::Public,
    )
    .await
}

#[utoipa::path(
    get,
    path = "/api/admin/uploads/{id}/content",
    tag = "admin-uploads",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<UploadContentRecord>))
)]
#[debug_handler]
pub async fn read_content(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:upload:update").await?;
    let file = find_file(&ctx, id).await?;
    ensure_editable_upload(&file)?;
    let (profile, bucket) = file_storage_location(&ctx, &actor, &file).await?;
    let (content, size_bytes, updated_at) =
        read_editable_upload_text(&profile, &bucket, &file).await?;
    Ok(responses::ok(UploadContentRecord {
        content,
        size_bytes,
        updated_at: updated_at.or_else(|| Some(file.updated_at.to_rfc3339())),
    }))
}

#[utoipa::path(
    put,
    path = "/api/admin/uploads/{id}/content",
    tag = "admin-uploads",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = UpdateUploadContentParams,
    responses((status = 200, body = ApiResponse<UploadRecord>))
)]
#[debug_handler]
pub async fn update_content(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<UpdateUploadContentParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:upload:update").await?;
    let file = find_file(&ctx, id).await?;
    ensure_editable_upload(&file)?;
    let (profile, bucket) = file_storage_location(&ctx, &actor, &file).await?;

    // Read the current object first so a mislabeled binary file cannot be overwritten as text.
    read_editable_upload_text(&profile, &bucket, &file).await?;
    validate_editable_upload_content(&params.content)?;

    let bytes = params.content.into_bytes();
    let size_bytes = i64::try_from(bytes.len()).unwrap_or(i64::MAX);
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let sha256 = hex::encode(hasher.finalize());
    let (object_prefix, object_name) = split_object_key(&file.object_key)?;
    let stored = storage::put_object(
        &profile,
        &bucket,
        object_prefix.as_deref(),
        &object_name,
        bytes,
    )
    .await?;
    if stored.object_key != file.object_key {
        return Err(ApiError::internal(
            "updated object key changed unexpectedly",
        ));
    }

    let current_url = file.url.clone();
    let mut active = file.into_active_model();
    active.bucket = Set(Some(stored.bucket));
    active.prefix = Set(stored.prefix);
    active.etag = Set(stored.etag);
    active.object_key = Set(stored.object_key);
    active.url = Set(if stored.url.is_empty() {
        current_url
    } else {
        stored.url
    });
    active.size_bytes = Set(size_bytes);
    active.sha256 = Set(sha256);
    active.updated_at = Set(Utc::now().into());
    let file = active.update(&ctx.db).await?;
    let file = ensure_access_url(&ctx, file).await?;
    record_upload_log(
        &ctx,
        &actor,
        "edit_content",
        format!("编辑素材文本：{}", file.original_name),
        Some(200),
        None,
    )
    .await;
    Ok(responses::ok(UploadRecord::from(file)))
}

#[derive(Clone, Copy)]
enum FileDisposition {
    Attachment,
    Inline,
}

#[derive(Clone, Copy)]
enum FileResponseScope {
    Private,
    Public,
}

impl FileDisposition {
    const fn as_header_value(self) -> &'static str {
        match self {
            Self::Attachment => "attachment",
            Self::Inline => "inline",
        }
    }
}

async fn file_content_response(
    profile: &storage_profiles_entity::Model,
    bucket: &storage_buckets::Model,
    request_headers: &HeaderMap,
    file: &upload_files::Model,
    disposition: FileDisposition,
    scope: FileResponseScope,
) -> ApiResult<Response> {
    let metadata = storage::object_metadata(profile, bucket, &file.object_key).await?;
    let total_size = usize::try_from(metadata.size_bytes)
        .map_err(|_| ApiError::bad_request("file is too large to stream"))?;
    let requested_range = file_stream::requested_range(request_headers, total_size)?;
    let content =
        storage::stream_object(profile, bucket, &file.object_key, requested_range.clone()).await?;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        file_content_type(profile, bucket, file, total_size, scope).await?,
    );
    let disposition_header = format!(
        "{}; filename=\"{}\"",
        disposition.as_header_value(),
        sanitize_filename(&file.original_name)
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&disposition_header)
            .map_err(|_| ApiError::internal("failed to build file response"))?,
    );
    if matches!(disposition, FileDisposition::Inline) {
        headers.insert(
            header::CACHE_CONTROL,
            match scope {
                FileResponseScope::Private => HeaderValue::from_static("private, no-store"),
                FileResponseScope::Public => HeaderValue::from_static("public, max-age=300"),
            },
        );
        headers.insert(
            HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        );
    }

    file_stream::streaming_response(content, headers, requested_range.is_some())
}

async fn file_content_type(
    profile: &storage_profiles_entity::Model,
    bucket: &storage_buckets::Model,
    file: &upload_files::Model,
    total_size: usize,
    scope: FileResponseScope,
) -> ApiResult<HeaderValue> {
    let configured = file
        .mime_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("application/octet-stream");
    let is_text = is_editable_upload_text(
        file.mime_type.as_deref(),
        file.extension.as_deref(),
        &file.original_name,
    );
    if !matches!(scope, FileResponseScope::Public)
        || !is_text
        || configured.to_ascii_lowercase().contains("charset=")
    {
        return configured
            .parse()
            .map_err(|_| ApiError::internal("failed to build file response"));
    }

    let mime_type = if configured == "application/octet-stream" {
        "text/plain"
    } else {
        configured
    };
    let charset = detect_stored_text_charset(profile, bucket, file, total_size).await?;
    HeaderValue::from_str(&format!("{mime_type}; charset={charset}"))
        .map_err(|_| ApiError::internal("failed to build file response"))
}

async fn detect_stored_text_charset(
    profile: &storage_profiles_entity::Model,
    bucket: &storage_buckets::Model,
    file: &upload_files::Model,
    total_size: usize,
) -> ApiResult<&'static str> {
    if total_size == 0 {
        return Ok("utf-8");
    }
    let sample_end = total_size.min(TEXT_ENCODING_SAMPLE_BYTES);
    let content =
        storage::stream_object(profile, bucket, &file.object_key, Some(0..sample_end)).await?;
    let sample = content
        .stream
        .try_fold(
            Vec::with_capacity(sample_end),
            |mut sample, chunk| async move {
                sample.extend_from_slice(&chunk);
                Ok(sample)
            },
        )
        .await
        .map_err(|_| ApiError::internal("failed to detect text encoding"))?;
    Ok(detect_text_charset(&sample, total_size))
}

fn detect_text_charset(sample: &[u8], total_size: usize) -> &'static str {
    if sample.starts_with(&[0xff, 0xfe]) {
        return "utf-16le";
    }
    if sample.starts_with(&[0xfe, 0xff]) {
        return "utf-16be";
    }
    match std::str::from_utf8(sample) {
        Ok(_) => "utf-8",
        Err(error) if error.error_len().is_none() && sample.len() < total_size => "utf-8",
        Err(_) => "gb18030",
    }
}

fn ensure_editable_upload(file: &upload_files::Model) -> ApiResult<()> {
    if file.status != "active" {
        return Err(ApiError::bad_request("file is not active"));
    }
    if is_editable_upload_text(
        file.mime_type.as_deref(),
        file.extension.as_deref(),
        &file.original_name,
    ) {
        Ok(())
    } else {
        Err(ApiError::bad_request("该素材不是可编辑的文本文件"))
    }
}

fn is_editable_upload_text(mime_type: Option<&str>, extension: Option<&str>, name: &str) -> bool {
    let mime_type = mime_type.unwrap_or_default().to_ascii_lowercase();
    let extension = extension.unwrap_or_default().to_ascii_lowercase();
    let name = name.to_ascii_lowercase();
    mime_type.starts_with("text/")
        || matches!(
            mime_type.as_str(),
            "application/json"
                | "application/javascript"
                | "application/toml"
                | "application/xml"
                | "application/x-yaml"
                | "application/yaml"
        )
        || EDITABLE_UPLOAD_EXTENSIONS.contains(&extension.as_str())
        || EDITABLE_UPLOAD_FILENAMES.contains(&name.as_str())
}

async fn read_editable_upload_text(
    profile: &storage_profiles_entity::Model,
    bucket: &storage_buckets::Model,
    file: &upload_files::Model,
) -> ApiResult<(String, i64, Option<String>)> {
    let metadata = storage::object_metadata(profile, bucket, &file.object_key).await?;
    let size = usize::try_from(metadata.size_bytes)
        .map_err(|_| ApiError::bad_request("文件大小无效，无法在线编辑"))?;
    if size > MAX_EDITABLE_UPLOAD_BYTES {
        return Err(ApiError::bad_request("文件超过 1 MiB，暂不支持在线编辑"));
    }
    let bytes = storage::get_object(profile, bucket, &file.object_key).await?;
    if bytes.len() > MAX_EDITABLE_UPLOAD_BYTES {
        return Err(ApiError::bad_request("文件超过 1 MiB，暂不支持在线编辑"));
    }
    let content = decode_editable_upload_text(bytes)?;
    Ok((content, metadata.size_bytes, metadata.updated_at))
}

fn decode_editable_upload_text(bytes: Vec<u8>) -> ApiResult<String> {
    if bytes.contains(&0) {
        return Err(ApiError::bad_request("文件包含二进制数据，无法在线编辑"));
    }
    String::from_utf8(bytes).map_err(|_| ApiError::bad_request("文件不是 UTF-8 文本，无法在线编辑"))
}

fn validate_editable_upload_content(content: &str) -> ApiResult<()> {
    if content.len() > MAX_EDITABLE_UPLOAD_BYTES {
        return Err(ApiError::bad_request("文件超过 1 MiB，暂不支持在线编辑"));
    }
    if content.contains('\0') {
        return Err(ApiError::bad_request("文件包含二进制数据，无法在线编辑"));
    }
    Ok(())
}

fn split_object_key(object_key: &str) -> ApiResult<(Option<String>, String)> {
    let (prefix, name) = object_key
        .rsplit_once('/')
        .map_or((None, object_key), |(prefix, name)| {
            (Some(prefix.to_string()), name)
        });
    if name.is_empty() {
        return Err(ApiError::bad_request("invalid object key"));
    }
    Ok((prefix, name.to_string()))
}

async fn file_storage_location(
    ctx: &AppContext,
    actor: &crate::models::users::Model,
    file: &upload_files::Model,
) -> ApiResult<(storage_profiles_entity::Model, storage_buckets::Model)> {
    file_storage_location_from_ids(ctx, actor, file.storage_bucket_id, file.storage_profile_id)
        .await
}

async fn public_file_storage_location(
    ctx: &AppContext,
    file: &upload_files::Model,
) -> ApiResult<(storage_profiles_entity::Model, storage_buckets::Model)> {
    let bucket_id = file
        .storage_bucket_id
        .ok_or_else(|| ApiError::bad_request("file storage bucket is unavailable"))?;
    let bucket = storage_profiles::find_bucket(ctx, bucket_id).await?;
    if !bucket.enabled {
        return Err(ApiError::forbidden("file is not publicly available"));
    }
    if file
        .storage_profile_id
        .is_some_and(|profile_id| profile_id != bucket.storage_profile_id)
    {
        return Err(ApiError::bad_request(
            "file storage profile does not match bucket",
        ));
    }
    let profile = storage_profiles::find_profile(ctx, bucket.storage_profile_id).await?;
    if !profile.enabled || profile.tenant_id != bucket.tenant_id {
        return Err(ApiError::forbidden("file is not publicly available"));
    }
    Ok((profile, bucket))
}

async fn file_storage_location_from_ids(
    ctx: &AppContext,
    actor: &crate::models::users::Model,
    bucket_id: Option<i32>,
    profile_id: Option<i32>,
) -> ApiResult<(storage_profiles_entity::Model, storage_buckets::Model)> {
    let (profile, bucket) = if let Some(bucket_id) = bucket_id {
        storage_profiles::resolve_bucket(ctx, actor, Some(bucket_id)).await?
    } else {
        storage_profiles::resolve_default_bucket(ctx, actor).await?
    };
    if profile_id.is_some_and(|id| id != profile.id) {
        return Err(ApiError::bad_request(
            "storage profile does not match bucket",
        ));
    }
    Ok((profile, bucket))
}

async fn ensure_access_url(
    ctx: &AppContext,
    file: upload_files::Model,
) -> ApiResult<upload_files::Model> {
    let access_url = default_upload_access_url(&file);
    if !file.url.trim().is_empty() && !is_internal_upload_url(&file.url, file.id) {
        return Ok(file);
    }
    if file.url == access_url {
        return Ok(file);
    }
    let mut active = file.clone().into_active_model();
    active.url = Set(access_url);
    active.update(&ctx.db).await.map_err(ApiError::from)
}

fn public_upload_path(id: i32) -> String {
    format!("/api/uploads/{id}")
}

fn download_upload_path(id: i32) -> String {
    format!("/api/admin/uploads/{id}/download")
}

fn default_upload_access_url(file: &upload_files::Model) -> String {
    if file.status == "active" && file.visibility == "public" {
        public_upload_path(file.id)
    } else {
        download_upload_path(file.id)
    }
}

fn is_internal_upload_url(value: &str, id: i32) -> bool {
    let value = value.trim();
    let path = url::Url::parse(value).map_or_else(
        |_| value.split(['?', '#']).next().unwrap_or(value).to_string(),
        |url| url.path().to_string(),
    );
    let path = path.trim_end_matches('/');
    path == public_upload_path(id) || path == download_upload_path(id)
}

fn public_upload_url(file: &upload_files::Model) -> Option<String> {
    if file.status != "active" || file.visibility != "public" {
        return None;
    }
    let value = file.url.trim();
    if value.is_empty() || is_internal_upload_url(value, file.id) {
        Some(public_upload_path(file.id))
    } else {
        Some(value.to_string())
    }
}

async fn find_file(ctx: &AppContext, id: i32) -> ApiResult<upload_files::Model> {
    upload_files::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("file not found"))
}

struct ImportExternalObjectInput {
    object_key: String,
    original_name: Option<String>,
    mime_type: Option<String>,
    category: Option<String>,
    tags: Option<String>,
    visibility: Option<String>,
}

fn is_folder_marker_object(object_key: &str) -> bool {
    object_key.ends_with('/') || object_key.ends_with("/.keep")
}

async fn import_external_object(
    ctx: &AppContext,
    user: &crate::models::users::Model,
    profile: &storage_profiles_entity::Model,
    bucket: &storage_buckets::Model,
    input: ImportExternalObjectInput,
) -> ApiResult<upload_files::Model> {
    let metadata = storage::object_metadata(profile, bucket, &input.object_key).await?;
    let original_name = input
        .original_name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            input
                .object_key
                .rsplit('/')
                .next()
                .unwrap_or(&input.object_key)
                .to_string()
        });
    let extension = file_extension(&original_name);
    ensure_allowed_extension(ctx, extension.as_deref()).await?;
    let filename = input
        .object_key
        .rsplit('/')
        .next()
        .unwrap_or(&input.object_key)
        .to_string();
    let prefix = input
        .object_key
        .rsplit_once('/')
        .map(|(prefix, _)| format!("{prefix}/"));
    upload_files::ActiveModel {
        storage: Set(profile.provider.clone()),
        storage_profile_id: Set(Some(profile.id)),
        storage_bucket_id: Set(Some(bucket.id)),
        bucket: Set(Some(bucket.bucket.clone())),
        prefix: Set(prefix),
        etag: Set(metadata.etag),
        object_key: Set(input.object_key),
        url: Set(metadata.url),
        original_name: Set(original_name),
        filename: Set(filename),
        extension: Set(extension),
        mime_type: Set(input.mime_type),
        size_bytes: Set(metadata.size_bytes),
        sha256: Set(String::new()),
        category: Set(input.category),
        tags: Set(input.tags),
        visibility: Set(input.visibility.unwrap_or_else(|| "private".to_string())),
        status: Set("active".to_string()),
        uploader_id: Set(Some(user.id)),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await
    .map_err(ApiError::from)
}

async fn find_task(
    ctx: &AppContext,
    user: &crate::models::users::Model,
    id: i32,
) -> ApiResult<upload_tasks::Model> {
    upload_tasks::Entity::find_by_id(id)
        .filter(upload_tasks::Column::UploaderId.eq(user.id))
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("upload task not found"))
}

async fn finish_upload_task_in_background(
    ctx: AppContext,
    user: crate::models::users::Model,
    task: upload_tasks::Model,
) {
    let uploaded_chunks = parse_uploaded_chunks(&task.uploaded_chunks);
    let result = complete_upload_task(&ctx, &user, &task, &uploaded_chunks).await;
    match result {
        Ok(file) => {
            record_upload_log(
                &ctx,
                &user,
                "complete_task",
                format!("分片上传完成并入库：{}", file.original_name),
                Some(200),
                None,
            )
            .await;
        }
        Err(err) => {
            let message = format!("{err:?}");
            let mut active = task.into_active_model();
            active.status = Set("failed".to_string());
            active.error_message = Set(Some(message.clone()));
            if let Err(update_err) = active.update(&ctx.db).await {
                tracing::error!(
                    error = update_err.to_string(),
                    "failed to mark upload task failed"
                );
            }
            record_upload_log(
                &ctx,
                &user,
                "complete_task_failed",
                "分片上传入库失败",
                Some(500),
                Some(message),
            )
            .await;
        }
    }
}

async fn complete_upload_task(
    ctx: &AppContext,
    user: &crate::models::users::Model,
    task: &upload_tasks::Model,
    uploaded_chunks: &[i32],
) -> ApiResult<upload_files::Model> {
    if let Some(upload_file_id) = task.upload_file_id {
        return find_file(ctx, upload_file_id).await;
    }
    let (profile, bucket) =
        file_storage_location_from_ids(ctx, user, task.storage_bucket_id, task.storage_profile_id)
            .await?;
    let mut active = task.clone().into_active_model();
    active.status = Set("importing".to_string());
    active.error_message = Set(None);
    active.update(&ctx.db).await?;

    let mut hasher = Sha256::new();
    let mut part_paths = Vec::with_capacity(uploaded_chunks.len());
    for chunk_index in uploaded_chunks {
        let path = task_chunk_path(task.id, *chunk_index);
        let bytes =
            fs::read(&path).map_err(|_| ApiError::bad_request("upload task chunk is missing"))?;
        hasher.update(&bytes);
        part_paths.push(path);
    }
    let stored =
        storage::put_object_from_files(&profile, &bucket, &task.object_key, &part_paths).await?;
    let file = upload_files::ActiveModel {
        storage: Set(task.storage.clone()),
        storage_profile_id: Set(task.storage_profile_id),
        storage_bucket_id: Set(task.storage_bucket_id),
        bucket: Set(Some(stored.bucket)),
        prefix: Set(stored.prefix),
        etag: Set(stored.etag),
        object_key: Set(stored.object_key),
        url: Set(stored.url),
        original_name: Set(task.original_name.clone()),
        filename: Set(task.filename.clone()),
        extension: Set(task.extension.clone()),
        mime_type: Set(task.mime_type.clone()),
        size_bytes: Set(task.size_bytes),
        sha256: Set(hex::encode(hasher.finalize())),
        category: Set(task.category.clone()),
        tags: Set(task.tags.clone()),
        visibility: Set(task.visibility.clone()),
        status: Set("active".to_string()),
        uploader_id: Set(Some(user.id)),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;
    let file = ensure_access_url(ctx, file).await?;
    let mut active = task.clone().into_active_model();
    active.status = Set("completed".to_string());
    active.error_message = Set(None);
    active.completed_at = Set(Some(Local::now().into()));
    active.upload_file_id = Set(Some(file.id));
    active.sha256 = Set(Some(file.sha256.clone()));
    active.update(&ctx.db).await?;
    remove_task_dir(task.id);
    Ok(file)
}

async fn record_upload_log(
    ctx: &AppContext,
    user: &crate::models::users::Model,
    action: &'static str,
    message: impl Into<String>,
    status: Option<i32>,
    error_message: Option<String>,
) {
    record_upload_log_at(
        ctx,
        user,
        action,
        message,
        status,
        error_message,
        "/api/admin/uploads",
    )
    .await;
}

async fn record_upload_log_at(
    ctx: &AppContext,
    user: &crate::models::users::Model,
    action: &'static str,
    message: impl Into<String>,
    status: Option<i32>,
    error_message: Option<String>,
    path: &'static str,
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
            module: "uploads",
            action,
            message: &message,
            user_id: Some(user.id),
            operator: Some(user.email.clone()),
            method: None,
            path: Some(path),
            status,
            error_message,
        },
    )
    .await;
}

async fn read_upload_payload(
    ctx: &AppContext,
    mut multipart: Multipart,
) -> ApiResult<UploadPayload> {
    let max_bytes = upload_limit_bytes(ctx).await?;
    let mut payload = UploadPayload::default();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| ApiError::bad_request("invalid multipart payload"))?
    {
        let name = field.name().unwrap_or_default().to_string();
        if name == "file" {
            let original_name = field.file_name().unwrap_or("upload.bin").to_string();
            let mime_type = field.content_type().map(str::to_string);
            let bytes = read_upload_field(field, max_bytes).await?;
            payload.file = Some(UploadedPart {
                original_name,
                mime_type,
                bytes,
            });
        } else {
            let value = field
                .text()
                .await
                .map_err(|_| ApiError::bad_request("failed to read multipart field"))?;
            payload.set_field(&name, &value)?;
        }
    }
    Ok(payload)
}

async fn store_upload(
    ctx: &AppContext,
    user: &crate::models::users::Model,
    payload: UploadPayload,
) -> ApiResult<StoredUpload> {
    let UploadPayload {
        storage_profile_id,
        storage_bucket_id,
        prefix,
        category,
        tags,
        visibility,
        file,
    } = payload;
    let uploaded = file.ok_or_else(|| ApiError::bad_request("file field is required"))?;
    let size_bytes = i64::try_from(uploaded.bytes.len()).unwrap_or(i64::MAX);
    let max_bytes = upload_limit_bytes(ctx).await?;
    if size_bytes > max_bytes {
        return Err(ApiError::bad_request(
            system_settings::upload_limit_error_message(max_bytes),
        ));
    }
    let extension = file_extension(&uploaded.original_name);
    ensure_allowed_extension(ctx, extension.as_deref()).await?;
    let safe_name = sanitize_filename(&uploaded.original_name);
    let filename = format!("{}-{safe_name}", Uuid::new_v4());
    let prefix = prefix.or_else(|| Some(Utc::now().format("%Y/%m").to_string()));
    let (profile, bucket) = storage_profiles::resolve_bucket(ctx, user, storage_bucket_id).await?;
    if storage_profile_id.is_some_and(|id| id != profile.id) {
        return Err(ApiError::bad_request(
            "storage profile does not match bucket",
        ));
    }

    let mut hasher = Sha256::new();
    hasher.update(&uploaded.bytes);
    let sha256 = hex::encode(hasher.finalize());
    let stored = storage::put_object(
        &profile,
        &bucket,
        prefix.as_deref(),
        &filename,
        uploaded.bytes,
    )
    .await?;
    let file = upload_files::ActiveModel {
        storage: Set(profile.provider.clone()),
        storage_profile_id: Set(Some(profile.id)),
        storage_bucket_id: Set(Some(bucket.id)),
        bucket: Set(Some(stored.bucket)),
        prefix: Set(stored.prefix),
        etag: Set(stored.etag),
        object_key: Set(stored.object_key),
        url: Set(stored.url),
        original_name: Set(uploaded.original_name),
        filename: Set(filename),
        extension: Set(extension),
        mime_type: Set(uploaded.mime_type),
        size_bytes: Set(size_bytes),
        sha256: Set(sha256),
        category: Set(category),
        tags: Set(tags),
        visibility: Set(visibility.unwrap_or_else(|| "private".to_string())),
        status: Set("active".to_string()),
        uploader_id: Set(Some(user.id)),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;
    let file = ensure_access_url(ctx, file).await?;
    Ok(StoredUpload {
        file,
        profile,
        bucket,
    })
}

fn absolute_network_url(ctx: &AppContext, headers: &HeaderMap, value: &str) -> String {
    let value = value.trim();
    if value.starts_with("http://") || value.starts_with("https://") {
        return value.to_string();
    }
    let origin = request_origin(ctx, headers);
    if value.starts_with("//") {
        let scheme = origin
            .split_once("://")
            .map_or("http", |(scheme, _)| scheme);
        return format!("{scheme}:{value}");
    }
    format!(
        "{}/{}",
        origin.trim_end_matches('/'),
        value.trim_start_matches('/')
    )
}

fn request_origin(ctx: &AppContext, headers: &HeaderMap) -> String {
    let forwarded_proto = first_header_value(headers, "x-forwarded-proto")
        .or_else(|| forwarded_parameter(headers, "proto"));
    let forwarded_host = first_header_value(headers, "x-forwarded-host")
        .or_else(|| forwarded_parameter(headers, "host"));
    let host = forwarded_host.or_else(|| first_header_value(headers, "host"));
    if let Some(host) = host {
        if host.starts_with("http://") || host.starts_with("https://") {
            return host.trim_end_matches('/').to_string();
        }
        let scheme = forwarded_proto.unwrap_or_else(|| {
            if ctx.config.server.host.starts_with("https://") {
                "https".to_string()
            } else {
                "http".to_string()
            }
        });
        return format!("{scheme}://{}", host.trim_end_matches('/'));
    }
    ctx.config
        .server
        .full_url()
        .trim_end_matches('/')
        .to_string()
}

fn first_header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)?
        .to_str()
        .ok()?
        .split(',')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn forwarded_parameter(headers: &HeaderMap, name: &str) -> Option<String> {
    let forwarded = first_header_value(headers, "forwarded")?;
    forwarded.split(';').find_map(|part| {
        let (key, value) = part.split_once('=')?;
        if key.trim().eq_ignore_ascii_case(name) {
            Some(value.trim().trim_matches('"').to_string())
        } else {
            None
        }
    })
}

async fn upload_limit_bytes(ctx: &AppContext) -> ApiResult<i64> {
    Ok(system_settings::upload_limit_bytes(&ctx.db).await?)
}

async fn read_upload_field(mut field: Field<'_>, max_bytes: i64) -> ApiResult<Vec<u8>> {
    let limit_message = system_settings::upload_limit_error_message(max_bytes);
    let max_bytes = usize::try_from(max_bytes).unwrap_or(usize::MAX);
    let mut bytes = Vec::new();
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|_| ApiError::bad_request("failed to read uploaded file"))?
    {
        if bytes.len().saturating_add(chunk.len()) > max_bytes {
            return Err(ApiError::bad_request(limit_message));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

async fn ensure_allowed_extension(ctx: &AppContext, extension: Option<&str>) -> ApiResult<()> {
    let Some(extension) = extension else {
        return Err(ApiError::bad_request("uploaded file extension is required"));
    };
    let allowed = system_settings::string_value(
        &ctx.db,
        "upload.allowed_extensions",
        "jpg,jpeg,png,gif,webp,svg,bmp,pdf,txt,csv,xls,xlsx,doc,docx,ppt,pptx,zip,rar,7z,tar,gz,mp4,m4v,webm,ogg,mov,avi,mkv,mp3,wav,m4a,aac,flac",
    )
    .await?;
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

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn upload_task_root() -> PathBuf {
    PathBuf::from("storage/upload_tasks")
}

fn prepare_task_dir(task_id: i32) -> ApiResult<()> {
    fs::create_dir_all(upload_task_root().join(task_id.to_string()))
        .map_err(|_| ApiError::internal("failed to prepare upload task storage"))
}

fn remove_task_dir(task_id: i32) {
    let _ = fs::remove_dir_all(upload_task_root().join(task_id.to_string()));
}

fn task_chunk_path(task_id: i32, chunk_index: i32) -> PathBuf {
    upload_task_root()
        .join(task_id.to_string())
        .join(format!("{chunk_index}.part"))
}

fn save_task_chunk(task_id: i32, chunk_index: i32, bytes: &[u8]) -> ApiResult<()> {
    prepare_task_dir(task_id)?;
    fs::write(task_chunk_path(task_id, chunk_index), bytes)
        .map_err(|_| ApiError::internal("failed to save upload chunk"))
}

fn task_chunk_size(task_id: i32, chunk_index: i32) -> ApiResult<i64> {
    fs::metadata(task_chunk_path(task_id, chunk_index))
        .map(|metadata| i64::try_from(metadata.len()).unwrap_or(i64::MAX))
        .map_err(|_| ApiError::bad_request("upload task chunk is missing"))
}

fn parse_uploaded_chunks(value: &str) -> Vec<i32> {
    serde_json::from_str(value).unwrap_or_default()
}

fn serialize_uploaded_chunks(chunks: &[i32]) -> String {
    serde_json::to_string(chunks).unwrap_or_else(|_| "[]".to_string())
}

fn join_object_key(prefix: Option<&str>, filename: &str) -> String {
    prefix
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map_or_else(
            || filename.to_string(),
            |prefix| format!("{}/{filename}", prefix.trim_end_matches('/')),
        )
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

#[derive(Default)]
struct UploadPayload {
    storage_profile_id: Option<i32>,
    storage_bucket_id: Option<i32>,
    prefix: Option<String>,
    category: Option<String>,
    tags: Option<String>,
    visibility: Option<String>,
    file: Option<UploadedPart>,
}

impl UploadPayload {
    fn set_field(&mut self, name: &str, value: &str) -> ApiResult<()> {
        let value = value.trim().to_string();
        if value.is_empty() {
            return Ok(());
        }
        match name {
            "storage_profile_id" => self.storage_profile_id = Some(parse_i32(&value, name)?),
            "storage_bucket_id" => self.storage_bucket_id = Some(parse_i32(&value, name)?),
            "prefix" => self.prefix = storage::normalize_prefix(Some(&value))?,
            "category" => self.category = Some(value),
            "tags" => self.tags = Some(value),
            "visibility" => {
                validate_visibility(Some(&value))?;
                self.visibility = Some(value);
            }
            _ => {}
        }
        Ok(())
    }
}

struct UploadedPart {
    original_name: String,
    mime_type: Option<String>,
    bytes: Vec<u8>,
}

struct StoredUpload {
    file: upload_files::Model,
    profile: storage_profiles_entity::Model,
    bucket: storage_buckets::Model,
}

fn parse_i32(value: &str, field: &str) -> ApiResult<i32> {
    value
        .parse::<i32>()
        .map_err(|_| ApiError::bad_request(format!("invalid {field}")))
}

impl From<upload_tasks::Model> for UploadTaskRecord {
    fn from(task: upload_tasks::Model) -> Self {
        Self {
            id: task.id,
            storage: task.storage,
            storage_profile_id: task.storage_profile_id,
            storage_bucket_id: task.storage_bucket_id,
            bucket: task.bucket,
            prefix: task.prefix,
            object_key: task.object_key,
            original_name: task.original_name,
            filename: task.filename,
            extension: task.extension,
            mime_type: task.mime_type,
            size_bytes: task.size_bytes,
            chunk_size: task.chunk_size,
            total_chunks: task.total_chunks,
            uploaded_chunks: parse_uploaded_chunks(&task.uploaded_chunks),
            uploaded_bytes: task.uploaded_bytes,
            category: task.category,
            tags: task.tags,
            visibility: task.visibility,
            status: task.status,
            error_message: task.error_message,
            completed_at: task.completed_at.map(|time| time.to_rfc3339()),
            upload_file_id: task.upload_file_id,
            uploader_id: task.uploader_id,
            created_at: task.created_at.to_rfc3339(),
            updated_at: task.updated_at.to_rfc3339(),
        }
    }
}

impl From<upload_files::Model> for UploadRecord {
    fn from(file: upload_files::Model) -> Self {
        let public_url = public_upload_url(&file);
        let download_url = download_upload_path(file.id);
        let url = public_url.clone().unwrap_or_else(|| download_url.clone());
        Self {
            id: file.id,
            storage: file.storage,
            storage_profile_id: file.storage_profile_id,
            storage_bucket_id: file.storage_bucket_id,
            bucket: file.bucket,
            prefix: file.prefix,
            etag: file.etag,
            object_key: file.object_key,
            url,
            public_url,
            download_url,
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

#[cfg(test)]
mod tests {
    use super::{
        decode_editable_upload_text, detect_text_charset, is_editable_upload_text,
        validate_editable_upload_content, MAX_EDITABLE_UPLOAD_BYTES,
    };

    #[test]
    fn editable_upload_detection_accepts_text_formats() {
        assert!(is_editable_upload_text(
            Some("text/plain"),
            None,
            "notes.data"
        ));
        assert!(is_editable_upload_text(
            Some("application/json"),
            None,
            "payload.data"
        ));
        assert!(is_editable_upload_text(None, Some("rs"), "main.rs"));
        assert!(is_editable_upload_text(None, None, "Dockerfile"));
        assert!(!is_editable_upload_text(
            Some("image/png"),
            Some("png"),
            "cover.png"
        ));
    }

    #[test]
    fn editable_upload_decoder_accepts_utf8() {
        let decoded = decode_editable_upload_text("hello, 文本\n".as_bytes().to_vec()).unwrap();
        assert_eq!(decoded, "hello, 文本\n");
    }

    #[test]
    fn public_text_charset_detection_supports_common_chinese_encodings() {
        assert_eq!(detect_text_charset("中文".as_bytes(), 6), "utf-8");
        assert_eq!(detect_text_charset(&[0xd6, 0xd0, 0xce, 0xc4], 4), "gb18030");
        assert_eq!(
            detect_text_charset(&[0xff, 0xfe, 0x2d, 0x4e], 4),
            "utf-16le"
        );
        assert_eq!(
            detect_text_charset(&[0xfe, 0xff, 0x4e, 0x2d], 4),
            "utf-16be"
        );
    }

    #[test]
    fn editable_upload_decoder_rejects_binary_content() {
        let nul_error = decode_editable_upload_text(b"hello\0world".to_vec()).unwrap_err();
        assert!(nul_error.message().contains("二进制"));

        let utf8_error = decode_editable_upload_text(vec![0xff, 0xfe]).unwrap_err();
        assert!(utf8_error.message().contains("UTF-8"));
    }

    #[test]
    fn editable_upload_validator_enforces_content_limit() {
        assert!(validate_editable_upload_content(&"a".repeat(MAX_EDITABLE_UPLOAD_BYTES)).is_ok());

        let oversized = "a".repeat(MAX_EDITABLE_UPLOAD_BYTES + 1);
        let size_error = validate_editable_upload_content(&oversized).unwrap_err();
        assert!(size_error.message().contains("1 MiB"));

        let nul_error = validate_editable_upload_content("hello\0world").unwrap_err();
        assert!(nul_error.message().contains("二进制"));
    }
}
