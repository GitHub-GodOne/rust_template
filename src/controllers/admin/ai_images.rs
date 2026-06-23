#![allow(clippy::missing_errors_doc)]

use std::{fs, path::PathBuf};

use axum::{
    extract::{multipart::Field, Multipart, Path as AxumPath},
    http::{header, HeaderMap, HeaderValue},
    response::IntoResponse,
};
use base64::{engine::general_purpose, Engine as _};
use chrono::Utc;
use loco_rs::prelude::*;
use reqwest::multipart;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, EntityTrait, IntoActiveModel, PaginatorTrait,
    QueryFilter, QueryOrder, Set,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    controllers::admin::{authorize, storage_profiles},
    errors::{ApiError, ApiResult},
    models::{
        _entities::{ai_image_generations, system_settings, upload_files},
        admin_logs, system_settings as system_settings_model,
    },
    responses::{self, ApiResponse, EmptyData, PageParams, PageResponse},
    services::{http_client, storage},
};

const CONFIG_SETTING_KEY: &str = "ai_image_manager.providers";
const CONFIG_SETTING_NAME: &str = "AI 图片生成配置";
const CONFIG_GROUP_KEY: &str = "ai_image_manager";
const GENERATED_MIME_TYPE: &str = "image/png";
const DEFAULT_MODEL: &str = "gpt-image-2";
const DEFAULT_SIZE: &str = "1024x1536";
const DEFAULT_QUALITY: &str = "high";
const DEFAULT_COUNT: u32 = 1;

#[derive(Debug, Clone, Deserialize, Serialize)]
struct AiImageConfig {
    key: String,
    name: String,
    enabled: bool,
    base_url: String,
    api_key: String,
    model: String,
    size: String,
    quality: String,
    n: u32,
    save_mode: String,
    local_output_dir: Option<String>,
    storage_bucket_id: Option<i32>,
    storage_prefix: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct AiImageConfigRecord {
    pub key: String,
    pub name: String,
    pub enabled: bool,
    pub base_url: String,
    pub api_key_configured: bool,
    pub model: String,
    pub size: String,
    pub quality: String,
    pub n: u32,
    pub save_mode: String,
    pub local_output_dir: Option<String>,
    pub storage_bucket_id: Option<i32>,
    pub storage_prefix: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveAiImageConfigParams {
    pub key: String,
    pub name: String,
    pub enabled: Option<bool>,
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub size: Option<String>,
    pub quality: Option<String>,
    pub n: Option<u32>,
    pub save_mode: String,
    pub local_output_dir: Option<String>,
    pub storage_bucket_id: Option<i32>,
    pub storage_prefix: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct AiImageGenerationRecord {
    pub id: i32,
    pub batch_id: String,
    pub config_key: String,
    pub config_name: String,
    pub prompt: String,
    pub model: String,
    pub size: String,
    pub quality: String,
    pub output_index: i32,
    pub save_mode: String,
    pub storage_profile_id: Option<i32>,
    pub storage_bucket_id: Option<i32>,
    pub output_upload_file_id: Option<i32>,
    pub local_output_path: Option<String>,
    pub original_name: String,
    pub mime_type: Option<String>,
    pub status: String,
    pub error_message: Option<String>,
    pub reference_summary: Option<String>,
    pub reference_count: i32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct AiImageGenerationBatchRecord {
    pub batch_id: String,
    pub items: Vec<AiImageGenerationRecord>,
}

#[derive(Debug)]
struct GenerationPayload {
    config_key: String,
    prompt: String,
    model: Option<String>,
    size: Option<String>,
    quality: Option<String>,
    n: Option<u32>,
    reference_upload_ids: Vec<i32>,
    uploaded_images: Vec<UploadedReferenceImage>,
}

#[derive(Debug, Clone)]
struct EffectiveGenerationParams {
    prompt: String,
    model: String,
    size: String,
    quality: String,
    n: u32,
}

#[derive(Debug)]
struct UploadedReferenceImage {
    original_name: String,
    mime_type: Option<String>,
    bytes: Vec<u8>,
}

#[derive(Debug)]
struct SavedGenerationAsset {
    storage_profile_id: Option<i32>,
    storage_bucket_id: Option<i32>,
    output_upload_file_id: Option<i32>,
    local_output_path: Option<String>,
    original_name: String,
    mime_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AiImageApiResponse {
    #[serde(default)]
    data: Vec<AiImageApiData>,
}

#[derive(Debug, Deserialize)]
struct AiImageApiData {
    b64_json: Option<String>,
}

#[derive(Copy, Clone)]
enum FileDisposition {
    Inline,
    Attachment,
}

impl FileDisposition {
    const fn as_header_value(self) -> &'static str {
        match self {
            Self::Inline => "inline",
            Self::Attachment => "attachment",
        }
    }
}

#[utoipa::path(
    get,
    path = "/api/admin/ai-images/configs",
    tag = "admin-ai-images",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<Vec<AiImageConfigRecord>>))
)]
#[debug_handler]
pub async fn list_configs(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:ai_image:list").await?;
    let configs = load_configs(&ctx)
        .await?
        .into_iter()
        .map(AiImageConfigRecord::from)
        .collect::<Vec<_>>();
    Ok(responses::ok(configs))
}

#[utoipa::path(
    post,
    path = "/api/admin/ai-images/configs",
    tag = "admin-ai-images",
    security(("bearer_auth" = [])),
    request_body = SaveAiImageConfigParams,
    responses((status = 200, body = ApiResponse<AiImageConfigRecord>))
)]
#[debug_handler]
pub async fn create_config(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SaveAiImageConfigParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:ai_image:config").await?;
    let mut configs = load_configs(&ctx).await?;
    let config = build_config(&params, None)?;
    if configs.iter().any(|item| item.key == config.key) {
        return Err(ApiError::bad_request("ai image config key already exists"));
    }
    configs.push(config.clone());
    save_configs(&ctx, user.id, &configs).await?;
    record_ai_image_log(
        &ctx,
        &user,
        "create_config",
        format!("创建 AI 图片配置：{}", config.name),
        Some(200),
        None,
    )
    .await;
    Ok(responses::ok(AiImageConfigRecord::from(config)))
}

#[utoipa::path(
    put,
    path = "/api/admin/ai-images/configs/{key}",
    tag = "admin-ai-images",
    security(("bearer_auth" = [])),
    params(("key" = String, Path)),
    request_body = SaveAiImageConfigParams,
    responses((status = 200, body = ApiResponse<AiImageConfigRecord>))
)]
#[debug_handler]
pub async fn update_config(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    AxumPath(key): AxumPath<String>,
    Json(params): Json<SaveAiImageConfigParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:ai_image:config").await?;
    let mut configs = load_configs(&ctx).await?;
    let position = configs
        .iter()
        .position(|item| item.key == key)
        .ok_or_else(|| ApiError::bad_request("ai image config not found"))?;
    let existing = configs[position].clone();
    let config = build_config(&params, Some(&existing))?;
    if configs
        .iter()
        .enumerate()
        .any(|(index, item)| index != position && item.key == config.key)
    {
        return Err(ApiError::bad_request("ai image config key already exists"));
    }
    configs[position] = config.clone();
    save_configs(&ctx, user.id, &configs).await?;
    record_ai_image_log(
        &ctx,
        &user,
        "update_config",
        format!("更新 AI 图片配置：{}", config.name),
        Some(200),
        None,
    )
    .await;
    Ok(responses::ok(AiImageConfigRecord::from(config)))
}

#[utoipa::path(
    delete,
    path = "/api/admin/ai-images/configs/{key}",
    tag = "admin-ai-images",
    security(("bearer_auth" = [])),
    params(("key" = String, Path)),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn delete_config(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    AxumPath(key): AxumPath<String>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:ai_image:config").await?;
    let mut configs = load_configs(&ctx).await?;
    let original_len = configs.len();
    configs.retain(|item| item.key != key);
    if configs.len() == original_len {
        return Err(ApiError::bad_request("ai image config not found"));
    }
    save_configs(&ctx, user.id, &configs).await?;
    record_ai_image_log(
        &ctx,
        &user,
        "delete_config",
        format!("删除 AI 图片配置：{key}"),
        Some(200),
        None,
    )
    .await;
    Ok(responses::empty())
}

#[utoipa::path(
    get,
    path = "/api/admin/ai-images/generations",
    tag = "admin-ai-images",
    security(("bearer_auth" = [])),
    params(PageParams),
    responses((status = 200, body = ApiResponse<PageResponse<AiImageGenerationRecord>>))
)]
#[debug_handler]
pub async fn list_generations(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<PageParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:ai_image:list").await?;
    let page = params.page();
    let page_size = params.page_size();
    let mut query = ai_image_generations::Entity::find()
        .filter(ai_image_generations::Column::CreatedBy.eq(user.id))
        .order_by_desc(ai_image_generations::Column::Id);
    if let Some(keyword) = params.keyword.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(
            Condition::any()
                .add(ai_image_generations::Column::Prompt.contains(keyword))
                .add(ai_image_generations::Column::ConfigName.contains(keyword))
                .add(ai_image_generations::Column::BatchId.contains(keyword)),
        );
    }
    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(AiImageGenerationRecord::from)
        .collect::<Vec<_>>();
    Ok(responses::ok(PageResponse {
        items,
        page,
        page_size,
        total,
    }))
}

#[utoipa::path(
    post,
    path = "/api/admin/ai-images/generations",
    tag = "admin-ai-images",
    security(("bearer_auth" = [])),
    request_body(content = String, content_type = "multipart/form-data"),
    responses((status = 200, body = ApiResponse<AiImageGenerationBatchRecord>))
)]
#[debug_handler]
#[allow(clippy::too_many_lines)]
pub async fn create_generation(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    mut multipart: Multipart,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:ai_image:generate").await?;
    let payload = parse_generation_payload(&ctx, &mut multipart).await?;
    let configs = load_configs(&ctx).await?;
    let config = configs
        .into_iter()
        .find(|item| item.key == payload.config_key)
        .ok_or_else(|| ApiError::bad_request("ai image config not found"))?;
    if !config.enabled {
        return Err(ApiError::bad_request("ai image config is disabled"));
    }
    let params = EffectiveGenerationParams::new(
        &config,
        &payload.prompt,
        payload.model,
        payload.size,
        payload.quality,
        payload.n,
    )?;
    let mut images = payload.uploaded_images;
    let uploaded_count = images.len();
    let library_count = payload.reference_upload_ids.len();
    let reference_summary = build_reference_summary(uploaded_count, library_count);
    let library_images =
        load_library_reference_images(&ctx, &user, &payload.reference_upload_ids).await?;
    images.extend(library_images);

    let batch_id = Uuid::new_v4().to_string();
    let generated = match request_ai_images(&ctx, &config, &params, &images).await {
        Ok(items) => items,
        Err(error) => {
            let _ = insert_failed_generation(
                &ctx,
                &user,
                &batch_id,
                &config,
                &params,
                reference_summary.clone(),
                i32::try_from(images.len()).unwrap_or(i32::MAX),
                &error,
            )
            .await;
            return Err(error);
        }
    };

    let mut records = Vec::with_capacity(generated.len());
    for (index, bytes) in generated.into_iter().enumerate() {
        let output_index = i32::try_from(index + 1).unwrap_or(i32::MAX);
        let asset = match save_generated_asset(&ctx, &user, &config, output_index, bytes).await {
            Ok(asset) => asset,
            Err(error) => {
                let _ = insert_failed_generation(
                    &ctx,
                    &user,
                    &batch_id,
                    &config,
                    &params,
                    reference_summary.clone(),
                    i32::try_from(images.len()).unwrap_or(i32::MAX),
                    &error,
                )
                .await;
                return Err(error);
            }
        };
        let record = ai_image_generations::ActiveModel {
            batch_id: Set(batch_id.clone()),
            config_key: Set(config.key.clone()),
            config_name: Set(config.name.clone()),
            prompt: Set(params.prompt.clone()),
            model: Set(params.model.clone()),
            size: Set(params.size.clone()),
            quality: Set(params.quality.clone()),
            output_index: Set(output_index),
            save_mode: Set(config.save_mode.clone()),
            storage_profile_id: Set(asset.storage_profile_id),
            storage_bucket_id: Set(asset.storage_bucket_id),
            output_upload_file_id: Set(asset.output_upload_file_id),
            local_output_path: Set(asset.local_output_path),
            original_name: Set(asset.original_name),
            mime_type: Set(asset.mime_type),
            status: Set("success".to_string()),
            error_message: Set(None),
            reference_summary: Set(reference_summary.clone()),
            reference_count: Set(i32::try_from(images.len()).unwrap_or(i32::MAX)),
            created_by: Set(Some(user.id)),
            ..Default::default()
        }
        .insert(&ctx.db)
        .await?;
        records.push(AiImageGenerationRecord::from(record));
    }

    record_ai_image_log(
        &ctx,
        &user,
        "generate",
        format!("生成 AI 图片：{}", config.name),
        Some(200),
        None,
    )
    .await;

    Ok(responses::ok(AiImageGenerationBatchRecord {
        batch_id,
        items: records,
    }))
}

#[utoipa::path(
    get,
    path = "/api/admin/ai-images/generations/{id}/preview",
    tag = "admin-ai-images",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = String, content_type = "application/octet-stream"))
)]
#[debug_handler]
pub async fn preview_generation(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    AxumPath(id): AxumPath<i32>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:ai_image:list").await?;
    let generation = find_generation(&ctx, &user, id).await?;
    generation_content_response(&ctx, &user, &generation, FileDisposition::Inline).await
}

#[utoipa::path(
    get,
    path = "/api/admin/ai-images/generations/{id}/download",
    tag = "admin-ai-images",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = String, content_type = "application/octet-stream"))
)]
#[debug_handler]
pub async fn download_generation(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    AxumPath(id): AxumPath<i32>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:ai_image:list").await?;
    let generation = find_generation(&ctx, &user, id).await?;
    generation_content_response(&ctx, &user, &generation, FileDisposition::Attachment).await
}

async fn parse_generation_payload(
    ctx: &AppContext,
    multipart: &mut Multipart,
) -> ApiResult<GenerationPayload> {
    let max_bytes = upload_limit_bytes(ctx).await?;
    let mut config_key = None;
    let mut prompt = None;
    let mut model = None;
    let mut size = None;
    let mut quality = None;
    let mut n = None;
    let mut reference_upload_ids = Vec::new();
    let mut uploaded_images = Vec::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| ApiError::bad_request("invalid multipart payload"))?
    {
        let name = field.name().unwrap_or_default().to_string();
        match name.as_str() {
            "config_key" => {
                config_key = Some(
                    field
                        .text()
                        .await
                        .map_err(|_| ApiError::bad_request("failed to read config_key"))?,
                );
            }
            "prompt" => {
                prompt = Some(
                    field
                        .text()
                        .await
                        .map_err(|_| ApiError::bad_request("failed to read prompt"))?,
                );
            }
            "model" => {
                model = Some(
                    field
                        .text()
                        .await
                        .map_err(|_| ApiError::bad_request("failed to read model"))?,
                );
            }
            "size" => {
                size = Some(
                    field
                        .text()
                        .await
                        .map_err(|_| ApiError::bad_request("failed to read size"))?,
                );
            }
            "quality" => {
                quality = Some(
                    field
                        .text()
                        .await
                        .map_err(|_| ApiError::bad_request("failed to read quality"))?,
                );
            }
            "n" => {
                let value = field
                    .text()
                    .await
                    .map_err(|_| ApiError::bad_request("failed to read n"))?;
                n = Some(parse_u32(&value, "n")?);
            }
            "reference_upload_ids" | "reference_upload_ids[]" => {
                let value = field
                    .text()
                    .await
                    .map_err(|_| ApiError::bad_request("failed to read reference_upload_ids"))?;
                reference_upload_ids.push(parse_i32(&value, "reference_upload_ids")?);
            }
            "image" => {
                uploaded_images.push(read_reference_image(field, max_bytes).await?);
            }
            _ => {}
        }
    }

    Ok(GenerationPayload {
        config_key: normalize_config_key(
            &config_key.ok_or_else(|| ApiError::bad_request("config_key is required"))?,
        )?,
        prompt: normalize_required_text(
            &prompt.ok_or_else(|| ApiError::bad_request("prompt is required"))?,
            "prompt",
        )?,
        model: normalize_optional_text(model),
        size: normalize_optional_text(size),
        quality: normalize_optional_text(quality),
        n,
        reference_upload_ids,
        uploaded_images,
    })
}

async fn read_reference_image(
    field: Field<'_>,
    max_bytes: i64,
) -> ApiResult<UploadedReferenceImage> {
    let original_name = field
        .file_name()
        .map(sanitize_filename)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "reference.png".to_string());
    let mime_type = field.content_type().map(str::to_string);
    let bytes = read_file_field(field, max_bytes).await?;
    Ok(UploadedReferenceImage {
        original_name,
        mime_type,
        bytes,
    })
}

async fn read_file_field(mut field: Field<'_>, max_bytes: i64) -> ApiResult<Vec<u8>> {
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

async fn load_library_reference_images(
    ctx: &AppContext,
    actor: &crate::models::users::Model,
    ids: &[i32],
) -> ApiResult<Vec<UploadedReferenceImage>> {
    let mut images = Vec::with_capacity(ids.len());
    for id in ids {
        let file = upload_files::Entity::find_by_id(*id)
            .one(&ctx.db)
            .await?
            .ok_or_else(|| ApiError::bad_request("reference upload file not found"))?;
        let bucket_id = file
            .storage_bucket_id
            .ok_or_else(|| ApiError::bad_request("reference upload file has no storage bucket"))?;
        let (profile, bucket) =
            storage_profiles::resolve_bucket(ctx, actor, Some(bucket_id)).await?;
        if file
            .storage_profile_id
            .is_some_and(|profile_id| profile_id != profile.id)
        {
            return Err(ApiError::bad_request(
                "reference upload file storage mismatch",
            ));
        }
        let bytes = storage::get_object(&profile, &bucket, &file.object_key).await?;
        images.push(UploadedReferenceImage {
            original_name: sanitize_filename(&file.original_name),
            mime_type: file.mime_type,
            bytes,
        });
    }
    Ok(images)
}

async fn request_ai_images(
    ctx: &AppContext,
    config: &AiImageConfig,
    params: &EffectiveGenerationParams,
    images: &[UploadedReferenceImage],
) -> ApiResult<Vec<Vec<u8>>> {
    let url = build_generation_url(&config.base_url, !images.is_empty());
    let client = http_client::build_http_client(&ctx.db).await?;
    let response = if images.is_empty() {
        client
            .post(&url)
            .bearer_auth(&config.api_key)
            .json(&json!({
                "model": params.model,
                "prompt": params.prompt,
                "size": params.size,
                "quality": params.quality,
                "n": params.n,
            }))
            .send()
            .await
            .map_err(|error| ApiError::bad_request(format!("ai image request failed: {error}")))?
    } else {
        let mut form = multipart::Form::new()
            .text("model", params.model.clone())
            .text("prompt", params.prompt.clone())
            .text("size", params.size.clone())
            .text("quality", params.quality.clone())
            .text("n", params.n.to_string())
            .text("response_format", "b64_json");

        for image in images {
            let part = multipart::Part::bytes(image.bytes.clone())
                .file_name(image.original_name.clone())
                .mime_str(image.mime_type.as_deref().unwrap_or(GENERATED_MIME_TYPE))
                .map_err(|_| ApiError::bad_request("invalid reference image mime type"))?;
            form = form.part("image", part);
        }

        client
            .post(&url)
            .bearer_auth(&config.api_key)
            .multipart(form)
            .send()
            .await
            .map_err(|error| ApiError::bad_request(format!("ai image request failed: {error}")))?
    };
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(ai_image_response_error(
            "upstream request failed",
            &url,
            status,
            &body,
        ));
    }
    let payload: AiImageApiResponse = serde_json::from_str(&body)
        .map_err(|_| ai_image_response_error("invalid ai image response", &url, status, &body))?;
    let mut generated = Vec::with_capacity(payload.data.len());
    for item in payload.data {
        let encoded = item
            .b64_json
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                ai_image_response_error(
                    "ai image response is missing b64_json",
                    &url,
                    status,
                    &body,
                )
            })?;
        let bytes = general_purpose::STANDARD.decode(encoded).map_err(|_| {
            ai_image_response_error("failed to decode ai image response", &url, status, &body)
        })?;
        generated.push(bytes);
    }
    if generated.is_empty() {
        return Err(ai_image_response_error(
            "ai image response returned no images",
            &url,
            status,
            &body,
        ));
    }
    Ok(generated)
}

async fn save_generated_asset(
    ctx: &AppContext,
    user: &crate::models::users::Model,
    config: &AiImageConfig,
    output_index: i32,
    bytes: Vec<u8>,
) -> ApiResult<SavedGenerationAsset> {
    match config.save_mode.as_str() {
        "local" => save_generated_local_asset(config, output_index, bytes),
        "storage" => save_generated_storage_asset(ctx, user, config, output_index, bytes).await,
        _ => Err(ApiError::bad_request("invalid ai image save mode")),
    }
}

fn save_generated_local_asset(
    config: &AiImageConfig,
    output_index: i32,
    bytes: Vec<u8>,
) -> ApiResult<SavedGenerationAsset> {
    let directory = config
        .local_output_dir
        .as_deref()
        .ok_or_else(|| ApiError::bad_request("local output directory is required"))?;
    let directory = ensure_local_output_dir(directory)?;
    let filename = generated_filename(output_index);
    let target = directory.join(&filename);
    fs::write(&target, bytes).map_err(|_| ApiError::internal("failed to save generated image"))?;
    Ok(SavedGenerationAsset {
        storage_profile_id: None,
        storage_bucket_id: None,
        output_upload_file_id: None,
        local_output_path: Some(target.display().to_string()),
        original_name: filename,
        mime_type: Some(GENERATED_MIME_TYPE.to_string()),
    })
}

async fn save_generated_storage_asset(
    ctx: &AppContext,
    user: &crate::models::users::Model,
    config: &AiImageConfig,
    output_index: i32,
    bytes: Vec<u8>,
) -> ApiResult<SavedGenerationAsset> {
    let bucket_id = config
        .storage_bucket_id
        .ok_or_else(|| ApiError::bad_request("storage bucket is required"))?;
    let (profile, bucket) = storage_profiles::resolve_bucket(ctx, user, Some(bucket_id)).await?;
    let filename = generated_filename(output_index);
    let stored = storage::put_object(
        &profile,
        &bucket,
        config.storage_prefix.as_deref(),
        &filename,
        bytes.clone(),
    )
    .await?;
    let sha256 = hex::encode(Sha256::digest(&bytes));
    let file = upload_files::ActiveModel {
        storage: Set(profile.provider.clone()),
        storage_profile_id: Set(Some(profile.id)),
        storage_bucket_id: Set(Some(bucket.id)),
        bucket: Set(Some(bucket.bucket.clone())),
        prefix: Set(stored.prefix.clone()),
        etag: Set(stored.etag),
        object_key: Set(stored.object_key.clone()),
        url: Set(stored.url),
        original_name: Set(filename.clone()),
        filename: Set(filename.clone()),
        extension: Set(Some("png".to_string())),
        mime_type: Set(Some(GENERATED_MIME_TYPE.to_string())),
        size_bytes: Set(i64::try_from(bytes.len()).unwrap_or(i64::MAX)),
        sha256: Set(sha256),
        category: Set(Some("ai-generated".to_string())),
        tags: Set(Some("ai-generated".to_string())),
        visibility: Set("private".to_string()),
        status: Set("active".to_string()),
        uploader_id: Set(Some(user.id)),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;
    let file = ensure_upload_download_url(ctx, file).await?;
    Ok(SavedGenerationAsset {
        storage_profile_id: Some(profile.id),
        storage_bucket_id: Some(bucket.id),
        output_upload_file_id: Some(file.id),
        local_output_path: None,
        original_name: file.original_name,
        mime_type: file.mime_type,
    })
}

async fn ensure_upload_download_url(
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

async fn generation_content_response(
    ctx: &AppContext,
    actor: &crate::models::users::Model,
    generation: &ai_image_generations::Model,
    disposition: FileDisposition,
) -> ApiResult<Response> {
    let (bytes, mime_type) = if let Some(upload_file_id) = generation.output_upload_file_id {
        let file = upload_files::Entity::find_by_id(upload_file_id)
            .one(&ctx.db)
            .await?
            .ok_or_else(|| ApiError::bad_request("generated upload file not found"))?;
        let bucket_id = file
            .storage_bucket_id
            .ok_or_else(|| ApiError::bad_request("generated upload file has no storage bucket"))?;
        let (profile, bucket) =
            storage_profiles::resolve_bucket(ctx, actor, Some(bucket_id)).await?;
        if file
            .storage_profile_id
            .is_some_and(|profile_id| profile_id != profile.id)
        {
            return Err(ApiError::bad_request(
                "generated upload file storage mismatch",
            ));
        }
        (
            storage::get_object(&profile, &bucket, &file.object_key).await?,
            file.mime_type,
        )
    } else {
        let path = generation
            .local_output_path
            .as_deref()
            .ok_or_else(|| ApiError::bad_request("generated image output path is missing"))?;
        (
            fs::read(path).map_err(|_| ApiError::bad_request("generated image file not found"))?,
            generation.mime_type.clone(),
        )
    };

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        mime_type
            .as_deref()
            .unwrap_or("application/octet-stream")
            .parse()
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "{}; filename=\"{}\"",
            disposition.as_header_value(),
            sanitize_filename(&generation.original_name)
        ))
        .map_err(|_| ApiError::internal("failed to build generated image response"))?,
    );
    if matches!(disposition, FileDisposition::Inline) {
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("private, no-store"),
        );
    }
    Ok((headers, bytes).into_response())
}

#[allow(clippy::too_many_arguments)]
async fn insert_failed_generation(
    ctx: &AppContext,
    user: &crate::models::users::Model,
    batch_id: &str,
    config: &AiImageConfig,
    params: &EffectiveGenerationParams,
    reference_summary: Option<String>,
    reference_count: i32,
    error: &ApiError,
) -> ApiResult<()> {
    ai_image_generations::ActiveModel {
        batch_id: Set(batch_id.to_string()),
        config_key: Set(config.key.clone()),
        config_name: Set(config.name.clone()),
        prompt: Set(params.prompt.clone()),
        model: Set(params.model.clone()),
        size: Set(params.size.clone()),
        quality: Set(params.quality.clone()),
        output_index: Set(0),
        save_mode: Set(config.save_mode.clone()),
        storage_profile_id: Set(None),
        storage_bucket_id: Set(None),
        output_upload_file_id: Set(None),
        local_output_path: Set(None),
        original_name: Set(generated_filename(0)),
        mime_type: Set(Some(GENERATED_MIME_TYPE.to_string())),
        status: Set("failed".to_string()),
        error_message: Set(Some(error_message(error))),
        reference_summary: Set(reference_summary),
        reference_count: Set(reference_count),
        created_by: Set(Some(user.id)),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;
    record_ai_image_log(
        ctx,
        user,
        "generate_failed",
        format!("生成 AI 图片失败：{}", config.name),
        Some(500),
        Some(error_message(error)),
    )
    .await;
    Ok(())
}

async fn find_generation(
    ctx: &AppContext,
    user: &crate::models::users::Model,
    id: i32,
) -> ApiResult<ai_image_generations::Model> {
    ai_image_generations::Entity::find_by_id(id)
        .filter(ai_image_generations::Column::CreatedBy.eq(user.id))
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("ai image generation not found"))
}

async fn load_configs(ctx: &AppContext) -> ApiResult<Vec<AiImageConfig>> {
    let value = system_settings_model::string_value(&ctx.db, CONFIG_SETTING_KEY, "[]").await?;
    serde_json::from_str::<Vec<AiImageConfig>>(&value)
        .map_err(|_| ApiError::bad_request("invalid ai image config setting"))
}

async fn save_configs(ctx: &AppContext, user_id: i32, configs: &[AiImageConfig]) -> ApiResult<()> {
    let value = serde_json::to_string(configs)
        .map_err(|_| ApiError::internal("failed to serialize ai image configs"))?;
    let setting = system_settings::Entity::find()
        .filter(system_settings::Column::Key.eq(CONFIG_SETTING_KEY))
        .one(&ctx.db)
        .await?;
    if let Some(setting) = setting {
        let mut active = setting.into_active_model();
        active.value = Set(value);
        active.updated_by = Set(Some(user_id));
        active.update(&ctx.db).await?;
    } else {
        system_settings::ActiveModel {
            key: Set(CONFIG_SETTING_KEY.to_string()),
            name: Set(CONFIG_SETTING_NAME.to_string()),
            group_key: Set(CONFIG_GROUP_KEY.to_string()),
            value: Set(value),
            value_type: Set("secret".to_string()),
            default_value: Set(Some("[]".to_string())),
            description: Set(Some("后台 AI 图片生成可复用配置".to_string())),
            is_public: Set(false),
            is_builtin: Set(true),
            is_encrypted: Set(true),
            sort_order: Set(200),
            created_by: Set(Some(user_id)),
            updated_by: Set(Some(user_id)),
            ..Default::default()
        }
        .insert(&ctx.db)
        .await?;
    }
    Ok(())
}

fn build_config(
    params: &SaveAiImageConfigParams,
    existing: Option<&AiImageConfig>,
) -> ApiResult<AiImageConfig> {
    let key = normalize_config_key(&params.key)?;
    let name = normalize_required_text(&params.name, "name")?;
    let base_url = normalize_required_text(&params.base_url, "base_url")?;
    let api_key = match normalize_optional_text(params.api_key.clone()) {
        Some(value) => value,
        None => existing
            .map(|config| config.api_key.clone())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| ApiError::bad_request("api_key is required"))?,
    };
    let save_mode = normalize_required_text(&params.save_mode, "save_mode")?;
    if !matches!(save_mode.as_str(), "local" | "storage") {
        return Err(ApiError::bad_request("invalid ai image save mode"));
    }
    let local_output_dir = normalize_optional_text(params.local_output_dir.clone());
    let storage_prefix = normalize_optional_text(params.storage_prefix.clone())
        .map(|value| format!("{}/", value.trim_matches('/')))
        .filter(|value| value != "/");
    let storage_bucket_id = params
        .storage_bucket_id
        .or_else(|| existing.and_then(|config| config.storage_bucket_id));
    if save_mode == "local" && local_output_dir.is_none() {
        return Err(ApiError::bad_request(
            "local_output_dir is required for local save mode",
        ));
    }
    if save_mode == "storage" && storage_bucket_id.is_none() {
        return Err(ApiError::bad_request(
            "storage_bucket_id is required for storage save mode",
        ));
    }
    let n = params
        .n
        .unwrap_or_else(|| existing.map_or(DEFAULT_COUNT, |config| config.n))
        .clamp(1, 10);
    Ok(AiImageConfig {
        key,
        name,
        enabled: params
            .enabled
            .unwrap_or_else(|| existing.is_none_or(|config| config.enabled)),
        base_url,
        api_key,
        model: normalize_optional_text(params.model.clone())
            .or_else(|| existing.map(|config| config.model.clone()))
            .unwrap_or_else(|| DEFAULT_MODEL.to_string()),
        size: normalize_optional_text(params.size.clone())
            .or_else(|| existing.map(|config| config.size.clone()))
            .unwrap_or_else(|| DEFAULT_SIZE.to_string()),
        quality: normalize_optional_text(params.quality.clone())
            .or_else(|| existing.map(|config| config.quality.clone()))
            .unwrap_or_else(|| DEFAULT_QUALITY.to_string()),
        n,
        save_mode,
        local_output_dir,
        storage_bucket_id,
        storage_prefix,
        description: normalize_optional_text(params.description.clone()),
    })
}

impl EffectiveGenerationParams {
    fn new(
        config: &AiImageConfig,
        prompt: &str,
        model: Option<String>,
        size: Option<String>,
        quality: Option<String>,
        n: Option<u32>,
    ) -> ApiResult<Self> {
        Ok(Self {
            prompt: normalize_required_text(prompt, "prompt")?,
            model: model.unwrap_or_else(|| config.model.clone()),
            size: size.unwrap_or_else(|| config.size.clone()),
            quality: quality.unwrap_or_else(|| config.quality.clone()),
            n: n.unwrap_or(config.n).clamp(1, 10),
        })
    }
}

impl From<AiImageConfig> for AiImageConfigRecord {
    fn from(config: AiImageConfig) -> Self {
        Self {
            key: config.key,
            name: config.name,
            enabled: config.enabled,
            base_url: config.base_url,
            api_key_configured: !config.api_key.trim().is_empty(),
            model: config.model,
            size: config.size,
            quality: config.quality,
            n: config.n,
            save_mode: config.save_mode,
            local_output_dir: config.local_output_dir,
            storage_bucket_id: config.storage_bucket_id,
            storage_prefix: config.storage_prefix,
            description: config.description,
        }
    }
}

impl From<ai_image_generations::Model> for AiImageGenerationRecord {
    fn from(generation: ai_image_generations::Model) -> Self {
        Self {
            id: generation.id,
            batch_id: generation.batch_id,
            config_key: generation.config_key,
            config_name: generation.config_name,
            prompt: generation.prompt,
            model: generation.model,
            size: generation.size,
            quality: generation.quality,
            output_index: generation.output_index,
            save_mode: generation.save_mode,
            storage_profile_id: generation.storage_profile_id,
            storage_bucket_id: generation.storage_bucket_id,
            output_upload_file_id: generation.output_upload_file_id,
            local_output_path: generation.local_output_path,
            original_name: generation.original_name,
            mime_type: generation.mime_type,
            status: generation.status,
            error_message: generation.error_message,
            reference_summary: generation.reference_summary,
            reference_count: generation.reference_count,
            created_at: generation.created_at.to_rfc3339(),
            updated_at: generation.updated_at.to_rfc3339(),
        }
    }
}

fn normalize_required_text(value: &str, field: &str) -> ApiResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(ApiError::bad_request(format!("{field} is required")));
    }
    Ok(value.to_string())
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
}

fn normalize_config_key(value: &str) -> ApiResult<String> {
    let value = normalize_required_text(value, "key")?;
    if value.starts_with('-')
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(ApiError::bad_request("invalid ai image config key"));
    }
    Ok(value)
}

fn build_generation_url(base_url: &str, with_reference_images: bool) -> String {
    let base_url = base_url.trim_end_matches('/');
    let suffix = if with_reference_images {
        "/images/edits"
    } else {
        "/images/generations"
    };
    if base_url.ends_with(suffix) {
        base_url.to_string()
    } else {
        format!("{base_url}{suffix}")
    }
}

fn generated_filename(output_index: i32) -> String {
    let timestamp = Utc::now().format("%Y%m%d%H%M%S");
    let suffix = Uuid::new_v4().simple();
    if output_index > 0 {
        format!("ai-image-{timestamp}-{output_index}-{suffix}.png")
    } else {
        format!("ai-image-{timestamp}-{suffix}.png")
    }
}

fn ensure_local_output_dir(directory: &str) -> ApiResult<PathBuf> {
    let path = PathBuf::from(directory);
    if path.as_os_str().is_empty() {
        return Err(ApiError::bad_request("local output directory is required"));
    }
    fs::create_dir_all(&path)
        .map_err(|_| ApiError::internal("failed to prepare local output directory"))?;
    let path = fs::canonicalize(path)
        .map_err(|_| ApiError::bad_request("local output directory not found"))?;
    ensure_directory(&path)?;
    Ok(path)
}

fn ensure_directory(path: &std::path::Path) -> ApiResult<()> {
    if path.is_dir() {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "local output path is not a directory",
        ))
    }
}

fn build_reference_summary(uploaded_count: usize, library_count: usize) -> Option<String> {
    let parts = [
        (uploaded_count > 0).then(|| format!("uploaded:{uploaded_count}")),
        (library_count > 0).then(|| format!("library:{library_count}")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

fn ai_image_response_error(
    reason: &str,
    url: &str,
    status: reqwest::StatusCode,
    body: &str,
) -> ApiError {
    ApiError::bad_request(format!(
        "{reason}: url={url}, status={}, body={}",
        status.as_u16(),
        summarize_remote_error(body)
    ))
}

fn summarize_remote_error(body: &str) -> String {
    let body = body.trim();
    if body.is_empty() {
        return "empty response body".to_string();
    }
    body.chars().take(2_000).collect::<String>()
}

fn sanitize_filename(file_name: &str) -> String {
    let sanitized = file_name
        .trim()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
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

async fn upload_limit_bytes(ctx: &AppContext) -> ApiResult<i64> {
    let max_mb =
        crate::models::system_settings::number_i64(&ctx.db, "upload.max_size_mb", 20).await?;
    Ok(max_mb.max(1) * 1024 * 1024)
}

fn parse_i32(value: &str, field: &str) -> ApiResult<i32> {
    value
        .parse::<i32>()
        .map_err(|_| ApiError::bad_request(format!("invalid {field}")))
}

fn parse_u32(value: &str, field: &str) -> ApiResult<u32> {
    value
        .parse::<u32>()
        .map_err(|_| ApiError::bad_request(format!("invalid {field}")))
}

fn error_message(error: &ApiError) -> String {
    error.message().to_string()
}

async fn record_ai_image_log(
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
            module: "ai_images",
            action,
            message: &message,
            user_id: Some(user.id),
            operator: Some(user.email.clone()),
            method: None,
            path: Some("/api/admin/ai-images"),
            status,
            error_message,
        },
    )
    .await;
}
