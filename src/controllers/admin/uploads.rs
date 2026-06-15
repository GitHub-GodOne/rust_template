#![allow(clippy::missing_errors_doc)]

use std::{collections::VecDeque, fs, path::PathBuf};

use axum::{
    extract::{multipart::Field, Multipart},
    http::{header, HeaderMap, HeaderName, HeaderValue},
    response::IntoResponse,
};
use chrono::{Local, Utc};
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
    services::storage::{self, StorageBrowserRecord, StoragePrefixRecord},
};

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
        return Err(ApiError::bad_request("uploaded file is too large"));
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
    mut multipart: Multipart,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:upload:create").await?;
    let max_bytes = upload_limit_bytes(&ctx).await?;
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

    let uploaded = payload
        .file
        .ok_or_else(|| ApiError::bad_request("file field is required"))?;

    let extension = file_extension(&uploaded.original_name);
    ensure_allowed_extension(&ctx, extension.as_deref()).await?;
    let safe_name = sanitize_filename(&uploaded.original_name);
    let date_path = Utc::now().format("%Y/%m").to_string();
    let filename = format!("{}-{safe_name}", Uuid::new_v4());
    let prefix = payload.prefix.or(Some(date_path));
    let (profile, bucket) =
        storage_profiles::resolve_bucket(&ctx, &user, payload.storage_bucket_id).await?;
    if payload
        .storage_profile_id
        .is_some_and(|id| id != profile.id)
    {
        return Err(ApiError::bad_request(
            "storage profile does not match bucket",
        ));
    }
    let stored = storage::put_object(
        &profile,
        &bucket,
        prefix.as_deref(),
        &filename,
        uploaded.bytes.clone(),
    )
    .await?;

    let mut hasher = Sha256::new();
    hasher.update(&uploaded.bytes);
    let sha256 = hex::encode(hasher.finalize());
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
        size_bytes: Set(i64::try_from(uploaded.bytes.len()).unwrap_or(i64::MAX)),
        sha256: Set(sha256),
        category: Set(payload.category),
        tags: Set(payload.tags),
        visibility: Set(payload.visibility.unwrap_or_else(|| "private".to_string())),
        status: Set("active".to_string()),
        uploader_id: Set(Some(user.id)),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;

    let file = ensure_download_url(&ctx, file).await?;
    record_upload_log(
        &ctx,
        &user,
        "upload",
        format!("上传素材成功：{}", file.original_name),
        Some(200),
        None,
    )
    .await;
    Ok(responses::ok(UploadRecord::from(file)))
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
    let file = ensure_download_url(&ctx, file).await?;
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
                    items.push(UploadRecord::from(ensure_download_url(&ctx, file).await?));
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
    active.category = Set(params.category);
    active.tags = Set(params.tags);
    active.visibility = Set(params.visibility.unwrap_or_else(|| "private".to_string()));
    active.status = Set(params.status.unwrap_or_else(|| "active".to_string()));
    let file = active.update(&ctx.db).await?;
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
    responses((status = 200, body = ApiResponse<StorageBrowserRecord>))
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
    Ok(responses::ok(browser))
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
    let actor = authorize(&ctx, &auth, "system:upload:download").await?;
    let file = find_file(&ctx, id).await?;
    if file.status != "active" {
        return Err(ApiError::bad_request("file is not active"));
    }
    let response = file_content_response(&ctx, &actor, &file, FileDisposition::Attachment).await?;
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
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:upload:download").await?;
    let file = find_file(&ctx, id).await?;
    if file.status != "active" {
        return Err(ApiError::bad_request("file is not active"));
    }
    let response = file_content_response(&ctx, &actor, &file, FileDisposition::Inline).await?;
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
}

async fn file_content_response(
    ctx: &AppContext,
    actor: &crate::models::users::Model,
    file: &upload_files::Model,
    disposition: FileDisposition,
) -> ApiResult<Response> {
    let (profile, bucket) = file_storage_location(ctx, actor, file).await?;
    let bytes = storage::get_object(&profile, &bucket, &file.object_key).await?;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        file.mime_type
            .as_deref()
            .unwrap_or("application/octet-stream")
            .parse()
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
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
            HeaderValue::from_static("private, no-store"),
        );
        headers.insert(
            HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        );
    }

    Ok((headers, bytes).into_response())
}

async fn file_storage_location(
    ctx: &AppContext,
    actor: &crate::models::users::Model,
    file: &upload_files::Model,
) -> ApiResult<(storage_profiles_entity::Model, storage_buckets::Model)> {
    file_storage_location_from_ids(ctx, actor, file.storage_bucket_id, file.storage_profile_id)
        .await
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

async fn ensure_download_url(
    ctx: &AppContext,
    file: upload_files::Model,
) -> ApiResult<upload_files::Model> {
    if !file.url.is_empty() {
        return Ok(file);
    }
    let mut active = file.clone().into_active_model();
    active.url = Set(format!("/api/admin/uploads/{}/download", file.id));
    active.update(&ctx.db).await.map_err(ApiError::from)
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
    let file = ensure_download_url(ctx, file).await?;
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
            path: Some("/api/admin/uploads"),
            status,
            error_message,
        },
    )
    .await;
}

async fn upload_limit_bytes(ctx: &AppContext) -> ApiResult<i64> {
    let max_mb = system_settings::number_i64(&ctx.db, "upload.max_size_mb", 20).await?;
    Ok(max_mb.max(1) * 1024 * 1024)
}

async fn read_upload_field(mut field: Field<'_>, max_bytes: i64) -> ApiResult<Vec<u8>> {
    let max_bytes = usize::try_from(max_bytes).unwrap_or(usize::MAX);
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
        Self {
            id: file.id,
            storage: file.storage,
            storage_profile_id: file.storage_profile_id,
            storage_bucket_id: file.storage_bucket_id,
            bucket: file.bucket,
            prefix: file.prefix,
            etag: file.etag,
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
