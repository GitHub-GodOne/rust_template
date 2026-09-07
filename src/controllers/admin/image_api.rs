#![allow(clippy::missing_errors_doc)]

use std::{error::Error as _, fmt::Write as _};

use axum::{
    extract::{multipart::Field, Multipart},
    http::{header, HeaderMap},
};
use base64::{engine::general_purpose, Engine as _};
use chrono::Utc;
use loco_rs::prelude::*;
use reqwest::multipart;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
    controllers::admin::{
        ai_images::{self, ImageApiProviderConfig},
        authorize,
        uploads::{self, FileApiUploadRecord, StoreFileApiBytesParams},
    },
    errors::{ApiError, ApiResult},
    models::{admin_logs, system_settings},
    responses::{self, ApiResponse},
    services::http_client::{self, HttpClientRuntimeOverrides},
};

const DEFAULT_BASE_URL: &str = "https://api.pikju.top/v1";
const DEFAULT_BASE_MODEL: &str = "gpt-5.4-mini";
const DEFAULT_IMAGE_MODEL: &str = "gpt-image-2";
const DEFAULT_SIZE: &str = "1024x1536";
const DEFAULT_QUALITY: &str = "high";
const DEFAULT_OUTPUT_FORMAT: &str = "png";
const DEFAULT_PARTIAL_IMAGES: u8 = 2;
const DEFAULT_REASONING_EFFORT: &str = "medium";
const DEFAULT_TIMEOUT_SECONDS: u64 = 900;
const MAX_TIMEOUT_SECONDS: u64 = 3_600;
const DEFAULT_IMAGE_COUNT: u32 = 1;
const MAX_IMAGE_COUNT: u32 = 10;
const MAX_EDIT_IMAGES: usize = 16;

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct ImageApiGenerationParams {
    pub prompt: String,
    pub config_key: Option<String>,
    #[serde(alias = "base_url")]
    pub api_url: Option<String>,
    pub api_key: Option<String>,
    pub base_model: Option<String>,
    pub image_model: Option<String>,
    pub size: Option<String>,
    pub quality: Option<String>,
    pub output_format: Option<String>,
    pub partial_images: Option<u8>,
    pub reasoning_effort: Option<String>,
    pub timeout_seconds: Option<u64>,
    pub storage_bucket_id: Option<i32>,
    pub prefix: Option<String>,
    pub visibility: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct ImageApiGenerationRecord {
    pub provider_key: String,
    pub provider_name: String,
    pub base_model: String,
    pub image_model: String,
    pub size: String,
    pub quality: String,
    pub output_format: String,
    pub file: FileApiUploadRecord,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct DirectImageGenerationParams {
    pub prompt: String,
    pub config_key: Option<String>,
    #[serde(alias = "base_url")]
    pub api_url: Option<String>,
    pub api_key: Option<String>,
    #[serde(alias = "image_model")]
    pub model: Option<String>,
    pub size: Option<String>,
    pub quality: Option<String>,
    pub output_format: Option<String>,
    pub n: Option<u32>,
    pub timeout_seconds: Option<u64>,
    pub storage_bucket_id: Option<i32>,
    pub prefix: Option<String>,
    pub visibility: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct DirectImageEditForm {
    #[schema(value_type = String, format = Binary)]
    pub image: String,
    #[schema(value_type = Option<String>, format = Binary)]
    pub mask: Option<String>,
    pub prompt: String,
    pub config_key: Option<String>,
    pub api_url: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub size: Option<String>,
    pub quality: Option<String>,
    pub output_format: Option<String>,
    pub n: Option<u32>,
    pub timeout_seconds: Option<u64>,
    pub storage_bucket_id: Option<i32>,
    pub prefix: Option<String>,
    pub visibility: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct DirectImageGenerationRecord {
    pub provider_key: String,
    pub provider_name: String,
    pub operation: String,
    pub model: String,
    pub size: String,
    pub quality: String,
    pub output_format: String,
    pub files: Vec<FileApiUploadRecord>,
}

#[derive(Debug)]
struct EffectiveGenerationParams {
    prompt: String,
    base_model: String,
    image_model: String,
    size: String,
    quality: String,
    output_format: String,
    partial_images: u8,
    reasoning_effort: String,
    timeout_seconds: u64,
    storage_bucket_id: Option<i32>,
    prefix: Option<String>,
    visibility: String,
}

#[derive(Debug)]
struct EffectiveDirectImageParams {
    prompt: String,
    model: String,
    size: String,
    quality: String,
    output_format: String,
    n: u32,
    timeout_seconds: u64,
    storage_bucket_id: Option<i32>,
    prefix: Option<String>,
    visibility: String,
}

#[derive(Debug)]
struct DirectImageInput {
    original_name: String,
    mime_type: Option<String>,
    bytes: Vec<u8>,
}

struct ImageResponseBody {
    status: reqwest::StatusCode,
    text: String,
    read_error: Option<String>,
}

#[derive(Default)]
struct DirectImageEditFields {
    prompt: Option<String>,
    config_key: Option<String>,
    api_url: Option<String>,
    api_key: Option<String>,
    model: Option<String>,
    size: Option<String>,
    quality: Option<String>,
    output_format: Option<String>,
    n: Option<u32>,
    timeout_seconds: Option<u64>,
    storage_bucket_id: Option<i32>,
    prefix: Option<String>,
    visibility: Option<String>,
}

struct DirectImageEditPayload {
    request: DirectImageGenerationParams,
    images: Vec<DirectImageInput>,
    mask: Option<DirectImageInput>,
}

#[utoipa::path(
    post,
    path = "/api/image-api/generations",
    tag = "image-api",
    security(("bearer_auth" = [])),
    request_body = ImageApiGenerationParams,
    responses((status = 200, body = ApiResponse<ImageApiGenerationRecord>))
)]
#[debug_handler]
pub async fn generate(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    Json(params): Json<ImageApiGenerationParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:ai_image:generate").await?;
    let result = generate_and_store(&ctx, &user, &headers, params).await;
    match result {
        Ok(record) => {
            record_image_api_log(
                &ctx,
                &user,
                "generate",
                "/api/image-api/generations",
                format!("图片生成接口调用成功：{}", record.provider_name),
                Some(200),
                None,
            )
            .await;
            Ok(responses::ok(record))
        }
        Err(error) => {
            record_image_api_log(
                &ctx,
                &user,
                "generate_failed",
                "/api/image-api/generations",
                "图片生成接口调用失败",
                Some(i32::from(error.status_code())),
                Some(error.message().to_string()),
            )
            .await;
            Err(error)
        }
    }
}

#[utoipa::path(
    post,
    path = "/api/image-api/images/generations",
    tag = "image-api",
    security(("bearer_auth" = [])),
    request_body = DirectImageGenerationParams,
    responses((status = 200, body = ApiResponse<DirectImageGenerationRecord>))
)]
#[debug_handler]
pub async fn generate_direct(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    Json(params): Json<DirectImageGenerationParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:ai_image:generate").await?;
    let result = generate_direct_and_store(&ctx, &user, &headers, params).await;
    finish_direct_request(
        &ctx,
        &user,
        result,
        "direct_generate",
        "/api/image-api/images/generations",
        "纯生图接口",
    )
    .await
}

#[utoipa::path(
    post,
    path = "/api/image-api/images/edits",
    tag = "image-api",
    security(("bearer_auth" = [])),
    request_body(content = DirectImageEditForm, content_type = "multipart/form-data"),
    responses((status = 200, body = ApiResponse<DirectImageGenerationRecord>))
)]
#[debug_handler]
pub async fn edit_direct(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:ai_image:generate").await?;
    let result = match parse_direct_edit_payload(&ctx, &mut multipart).await {
        Ok(payload) => edit_direct_and_store(&ctx, &user, &headers, payload).await,
        Err(error) => Err(error),
    };
    finish_direct_request(
        &ctx,
        &user,
        result,
        "direct_edit",
        "/api/image-api/images/edits",
        "图片编辑接口",
    )
    .await
}

async fn finish_direct_request(
    ctx: &AppContext,
    user: &crate::models::users::Model,
    result: ApiResult<DirectImageGenerationRecord>,
    action: &str,
    path: &'static str,
    label: &str,
) -> ApiResult<Response> {
    match result {
        Ok(record) => {
            record_image_api_log(
                ctx,
                user,
                action,
                path,
                format!("{label}调用成功：{}", record.provider_name),
                Some(200),
                None,
            )
            .await;
            Ok(responses::ok(record))
        }
        Err(error) => {
            record_image_api_log(
                ctx,
                user,
                &format!("{action}_failed"),
                path,
                format!("{label}调用失败"),
                Some(i32::from(error.status_code())),
                Some(error.message().to_string()),
            )
            .await;
            Err(error)
        }
    }
}

async fn generate_and_store(
    ctx: &AppContext,
    user: &crate::models::users::Model,
    headers: &HeaderMap,
    request: ImageApiGenerationParams,
) -> ApiResult<ImageApiGenerationRecord> {
    let provider = resolve_provider(
        ctx,
        request.config_key.as_deref(),
        request.api_url.as_deref(),
        request.api_key.as_deref(),
    )
    .await?;
    let params = EffectiveGenerationParams::new(request, &provider)?;
    let image_bytes = request_image(ctx, &provider, &params).await?;
    let filename = generated_filename(&params.output_format);
    let file = uploads::store_file_api_bytes(
        ctx,
        user,
        headers,
        StoreFileApiBytesParams {
            original_name: filename,
            mime_type: Some(output_mime_type(&params.output_format).to_string()),
            bytes: image_bytes,
            storage_bucket_id: params.storage_bucket_id,
            prefix: params.prefix.clone(),
            category: Some("ai-generated".to_string()),
            tags: Some(format!(
                "ai-generated,{},{}",
                params.base_model, params.image_model
            )),
            visibility: Some(params.visibility.clone()),
        },
    )
    .await?;

    Ok(ImageApiGenerationRecord {
        provider_key: provider.key,
        provider_name: provider.name,
        base_model: params.base_model,
        image_model: params.image_model,
        size: params.size,
        quality: params.quality,
        output_format: params.output_format,
        file,
    })
}

async fn generate_direct_and_store(
    ctx: &AppContext,
    user: &crate::models::users::Model,
    headers: &HeaderMap,
    request: DirectImageGenerationParams,
) -> ApiResult<DirectImageGenerationRecord> {
    let provider = resolve_provider(
        ctx,
        request.config_key.as_deref(),
        request.api_url.as_deref(),
        request.api_key.as_deref(),
    )
    .await?;
    let params = EffectiveDirectImageParams::new(request, &provider)?;
    let images = request_direct_generation(ctx, &provider, &params).await?;
    store_direct_images(ctx, user, headers, &provider, &params, "generation", images).await
}

async fn edit_direct_and_store(
    ctx: &AppContext,
    user: &crate::models::users::Model,
    headers: &HeaderMap,
    payload: DirectImageEditPayload,
) -> ApiResult<DirectImageGenerationRecord> {
    let provider = resolve_provider(
        ctx,
        payload.request.config_key.as_deref(),
        payload.request.api_url.as_deref(),
        payload.request.api_key.as_deref(),
    )
    .await?;
    let params = EffectiveDirectImageParams::new(payload.request, &provider)?;
    let images = request_direct_edit(ctx, &provider, &params, payload.images, payload.mask).await?;
    store_direct_images(ctx, user, headers, &provider, &params, "edit", images).await
}

async fn store_direct_images(
    ctx: &AppContext,
    user: &crate::models::users::Model,
    headers: &HeaderMap,
    provider: &ImageApiProviderConfig,
    params: &EffectiveDirectImageParams,
    operation: &str,
    images: Vec<Vec<u8>>,
) -> ApiResult<DirectImageGenerationRecord> {
    let category = if operation == "edit" {
        "ai-edited"
    } else {
        "ai-generated"
    };
    let mut files = Vec::with_capacity(images.len());
    for (index, bytes) in images.into_iter().enumerate() {
        let file = uploads::store_file_api_bytes(
            ctx,
            user,
            headers,
            StoreFileApiBytesParams {
                original_name: direct_image_filename(operation, &params.output_format, index + 1),
                mime_type: Some(output_mime_type(&params.output_format).to_string()),
                bytes,
                storage_bucket_id: params.storage_bucket_id,
                prefix: params.prefix.clone(),
                category: Some(category.to_string()),
                tags: Some(format!("{category},direct-api,{}", params.model)),
                visibility: Some(params.visibility.clone()),
            },
        )
        .await?;
        files.push(file);
    }

    Ok(DirectImageGenerationRecord {
        provider_key: provider.key.clone(),
        provider_name: provider.name.clone(),
        operation: operation.to_string(),
        model: params.model.clone(),
        size: params.size.clone(),
        quality: params.quality.clone(),
        output_format: params.output_format.clone(),
        files,
    })
}

async fn parse_direct_edit_payload(
    ctx: &AppContext,
    multipart: &mut Multipart,
) -> ApiResult<DirectImageEditPayload> {
    let max_bytes = upload_limit_bytes(ctx).await?;
    let mut fields = DirectImageEditFields::default();
    let mut images = Vec::new();
    let mut mask = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| ApiError::bad_request("invalid multipart payload"))?
    {
        let name = field.name().unwrap_or_default().to_string();
        match name.as_str() {
            "image" | "image[]" | "images" | "images[]" => {
                if images.len() >= MAX_EDIT_IMAGES {
                    return Err(ApiError::bad_request(format!(
                        "at most {MAX_EDIT_IMAGES} edit images are supported"
                    )));
                }
                images.push(read_direct_image(field, max_bytes, "image").await?);
            }
            "mask" => {
                if mask.is_some() {
                    return Err(ApiError::bad_request("only one mask is supported"));
                }
                mask = Some(read_direct_image(field, max_bytes, "mask").await?);
            }
            "prompt" => fields.prompt = Some(read_multipart_text(field, "prompt").await?),
            "config_key" => {
                fields.config_key = Some(read_multipart_text(field, "config_key").await?);
            }
            "api_url" | "base_url" => {
                fields.api_url = Some(read_multipart_text(field, "api_url").await?);
            }
            "api_key" => fields.api_key = Some(read_multipart_text(field, "api_key").await?),
            "model" | "image_model" => {
                fields.model = Some(read_multipart_text(field, "model").await?);
            }
            "size" => fields.size = Some(read_multipart_text(field, "size").await?),
            "quality" => fields.quality = Some(read_multipart_text(field, "quality").await?),
            "output_format" => {
                fields.output_format = Some(read_multipart_text(field, "output_format").await?);
            }
            "n" => {
                let value = read_multipart_text(field, "n").await?;
                fields.n = Some(parse_u32_field(&value, "n")?);
            }
            "timeout_seconds" => {
                let value = read_multipart_text(field, "timeout_seconds").await?;
                fields.timeout_seconds = Some(parse_u64_field(&value, "timeout_seconds")?);
            }
            "storage_bucket_id" => {
                let value = read_multipart_text(field, "storage_bucket_id").await?;
                fields.storage_bucket_id = Some(parse_i32_field(&value, "storage_bucket_id")?);
            }
            "prefix" => fields.prefix = Some(read_multipart_text(field, "prefix").await?),
            "visibility" => {
                fields.visibility = Some(read_multipart_text(field, "visibility").await?);
            }
            _ => {}
        }
    }

    if images.is_empty() {
        return Err(ApiError::bad_request("at least one image is required"));
    }
    Ok(DirectImageEditPayload {
        request: DirectImageGenerationParams {
            prompt: fields
                .prompt
                .ok_or_else(|| ApiError::bad_request("prompt is required"))?,
            config_key: fields.config_key,
            api_url: fields.api_url,
            api_key: fields.api_key,
            model: fields.model,
            size: fields.size,
            quality: fields.quality,
            output_format: fields.output_format,
            n: fields.n,
            timeout_seconds: fields.timeout_seconds,
            storage_bucket_id: fields.storage_bucket_id,
            prefix: fields.prefix,
            visibility: fields.visibility,
        },
        images,
        mask,
    })
}

async fn read_multipart_text(mut field: Field<'_>, name: &str) -> ApiResult<String> {
    let max_bytes = match name {
        "prompt" => 400_000,
        "api_key" => 65_536,
        _ => 8_192,
    };
    let mut bytes = Vec::new();
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|_| ApiError::bad_request(format!("failed to read {name}")))?
    {
        if bytes.len().saturating_add(chunk.len()) > max_bytes {
            return Err(ApiError::bad_request(format!("{name} is too long")));
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes).map_err(|_| ApiError::bad_request(format!("invalid {name}")))
}

async fn read_direct_image(
    mut field: Field<'_>,
    max_bytes: i64,
    fallback_name: &str,
) -> ApiResult<DirectImageInput> {
    let limit_message = system_settings::upload_limit_error_message(max_bytes);
    let original_name = field
        .file_name()
        .map(sanitize_input_filename)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("{fallback_name}.png"));
    let mime_type = field.content_type().map(str::to_string);
    if mime_type
        .as_deref()
        .is_some_and(|value| !value.starts_with("image/"))
    {
        return Err(ApiError::bad_request(format!(
            "{fallback_name} must be an image"
        )));
    }
    let max_bytes = usize::try_from(max_bytes).unwrap_or(usize::MAX);
    let mut bytes = Vec::new();
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|_| ApiError::bad_request(format!("failed to read {fallback_name}")))?
    {
        if bytes.len().saturating_add(chunk.len()) > max_bytes {
            return Err(ApiError::bad_request(limit_message));
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.is_empty() {
        return Err(ApiError::bad_request(format!("{fallback_name} is empty")));
    }
    Ok(DirectImageInput {
        original_name,
        mime_type,
        bytes,
    })
}

async fn resolve_provider(
    ctx: &AppContext,
    config_key: Option<&str>,
    api_url: Option<&str>,
    api_key: Option<&str>,
) -> ApiResult<ImageApiProviderConfig> {
    let api_url = api_url
        .map(|value| required_text(value, "api_url", 2_048))
        .transpose()?;
    let api_key = api_key
        .map(|value| required_text(value, "api_key", 16_384))
        .transpose()?;
    let has_config_key = config_key
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    let has_request_credentials = api_url.is_some() || api_key.is_some();
    let direct_request = !has_config_key && api_url.is_some() && api_key.is_some();
    let managed_provider = if direct_request {
        None
    } else {
        ai_images::resolve_image_api_provider(ctx, config_key).await?
    };
    let has_managed_provider = managed_provider.is_some();
    let mut provider = managed_provider.unwrap_or_else(|| ImageApiProviderConfig {
        key: "cpamc-environment".to_string(),
        name: "CPAMC environment".to_string(),
        base_url: environment_text("CPAMC_BASE_URL")
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
        api_key: environment_text("CPAMC_API_KEY").unwrap_or_default(),
        image_model: DEFAULT_IMAGE_MODEL.to_string(),
        size: DEFAULT_SIZE.to_string(),
        quality: DEFAULT_QUALITY.to_string(),
        storage_bucket_id: None,
        storage_prefix: None,
    });

    if let Some(api_url) = api_url {
        provider.base_url = api_url;
    }
    if let Some(api_key) = api_key {
        provider.api_key = api_key;
    }
    if provider.api_key.trim().is_empty() {
        return Err(ApiError::bad_request(
            "api_key is required; pass it in the request, configure an enabled provider, or set CPAMC_API_KEY",
        ));
    }
    provider.api_key = normalize_upstream_api_key(&provider.api_key)?;
    if !has_managed_provider && has_request_credentials {
        provider.key = "request".to_string();
        provider.name = "Request credentials".to_string();
    }
    Ok(provider)
}

impl EffectiveGenerationParams {
    fn new(
        request: ImageApiGenerationParams,
        provider: &ImageApiProviderConfig,
    ) -> ApiResult<Self> {
        let prompt = required_text(&request.prompt, "prompt", 100_000)?;
        let base_model = optional_text(request.base_model, "base_model", 128)?
            .or_else(|| environment_text("CPAMC_BASE_MODEL"))
            .unwrap_or_else(|| DEFAULT_BASE_MODEL.to_string());
        let image_model = optional_text(request.image_model, "image_model", 128)?
            .unwrap_or_else(|| provider.image_model.clone());
        let size =
            optional_text(request.size, "size", 64)?.unwrap_or_else(|| provider.size.clone());
        let quality = optional_text(request.quality, "quality", 16)?
            .unwrap_or_else(|| provider.quality.clone())
            .to_ascii_lowercase();
        ensure_choice("quality", &quality, &["low", "medium", "high", "auto"])?;
        let output_format = optional_text(request.output_format, "output_format", 16)?
            .unwrap_or_else(|| DEFAULT_OUTPUT_FORMAT.to_string())
            .to_ascii_lowercase();
        ensure_choice("output_format", &output_format, &["png", "jpeg", "webp"])?;
        let reasoning_effort = optional_text(request.reasoning_effort, "reasoning_effort", 16)?
            .unwrap_or_else(|| DEFAULT_REASONING_EFFORT.to_string())
            .to_ascii_lowercase();
        ensure_choice(
            "reasoning_effort",
            &reasoning_effort,
            &["low", "medium", "high"],
        )?;
        let partial_images = request.partial_images.unwrap_or(DEFAULT_PARTIAL_IMAGES);
        if partial_images > 3 {
            return Err(ApiError::bad_request(
                "partial_images must be between 0 and 3",
            ));
        }
        let timeout_seconds = request.timeout_seconds.unwrap_or(DEFAULT_TIMEOUT_SECONDS);
        if !(1..=MAX_TIMEOUT_SECONDS).contains(&timeout_seconds) {
            return Err(ApiError::bad_request(format!(
                "timeout_seconds must be between 1 and {MAX_TIMEOUT_SECONDS}"
            )));
        }
        let visibility = optional_text(request.visibility, "visibility", 16)?
            .unwrap_or_else(|| "private".to_string())
            .to_ascii_lowercase();
        ensure_choice("visibility", &visibility, &["private", "public"])?;

        Ok(Self {
            prompt,
            base_model,
            image_model,
            size,
            quality,
            output_format,
            partial_images,
            reasoning_effort,
            timeout_seconds,
            storage_bucket_id: request.storage_bucket_id.or(provider.storage_bucket_id),
            prefix: request.prefix.or_else(|| provider.storage_prefix.clone()),
            visibility,
        })
    }
}

impl EffectiveDirectImageParams {
    fn new(
        request: DirectImageGenerationParams,
        provider: &ImageApiProviderConfig,
    ) -> ApiResult<Self> {
        let prompt = required_text(&request.prompt, "prompt", 100_000)?;
        let model = optional_text(request.model, "model", 128)?
            .unwrap_or_else(|| provider.image_model.clone());
        let size =
            optional_text(request.size, "size", 64)?.unwrap_or_else(|| provider.size.clone());
        let quality = optional_text(request.quality, "quality", 16)?
            .unwrap_or_else(|| provider.quality.clone())
            .to_ascii_lowercase();
        ensure_choice("quality", &quality, &["low", "medium", "high", "auto"])?;
        let output_format = optional_text(request.output_format, "output_format", 16)?
            .unwrap_or_else(|| DEFAULT_OUTPUT_FORMAT.to_string())
            .to_ascii_lowercase();
        ensure_choice("output_format", &output_format, &["png", "jpeg", "webp"])?;
        let n = request.n.unwrap_or(DEFAULT_IMAGE_COUNT);
        if !(1..=MAX_IMAGE_COUNT).contains(&n) {
            return Err(ApiError::bad_request(format!(
                "n must be between 1 and {MAX_IMAGE_COUNT}"
            )));
        }
        let timeout_seconds = request.timeout_seconds.unwrap_or(DEFAULT_TIMEOUT_SECONDS);
        if !(1..=MAX_TIMEOUT_SECONDS).contains(&timeout_seconds) {
            return Err(ApiError::bad_request(format!(
                "timeout_seconds must be between 1 and {MAX_TIMEOUT_SECONDS}"
            )));
        }
        let visibility = optional_text(request.visibility, "visibility", 16)?
            .unwrap_or_else(|| "private".to_string())
            .to_ascii_lowercase();
        ensure_choice("visibility", &visibility, &["private", "public"])?;

        Ok(Self {
            prompt,
            model,
            size,
            quality,
            output_format,
            n,
            timeout_seconds,
            storage_bucket_id: request.storage_bucket_id.or(provider.storage_bucket_id),
            prefix: request.prefix.or_else(|| provider.storage_prefix.clone()),
            visibility,
        })
    }
}

async fn request_direct_generation(
    ctx: &AppContext,
    provider: &ImageApiProviderConfig,
    params: &EffectiveDirectImageParams,
) -> ApiResult<Vec<Vec<u8>>> {
    let url = build_images_url(&provider.base_url, "generations")?;
    let client = direct_image_client(ctx, params.timeout_seconds).await?;
    let payload = json!({
        "model": &params.model,
        "prompt": &params.prompt,
        "size": &params.size,
        "quality": &params.quality,
        "output_format": &params.output_format,
        "n": params.n,
    });
    let response = post_json_with_connect_retry(
        &client,
        &url,
        &provider.api_key,
        "application/json",
        &payload,
        "image request failed",
    )
    .await?;
    parse_direct_image_response(
        &client,
        &provider.base_url,
        &provider.api_key,
        response,
        &params.output_format,
    )
    .await
}

async fn request_direct_edit(
    ctx: &AppContext,
    provider: &ImageApiProviderConfig,
    params: &EffectiveDirectImageParams,
    images: Vec<DirectImageInput>,
    mask: Option<DirectImageInput>,
) -> ApiResult<Vec<Vec<u8>>> {
    let url = build_images_url(&provider.base_url, "edits")?;
    let client = direct_image_client(ctx, params.timeout_seconds).await?;
    let multiple_images = images.len() > 1;
    let mut form = multipart::Form::new()
        .text("model", params.model.clone())
        .text("prompt", params.prompt.clone())
        .text("size", params.size.clone())
        .text("quality", params.quality.clone())
        .text("output_format", params.output_format.clone())
        .text("n", params.n.to_string());
    for image in images {
        let part = multipart::Part::bytes(image.bytes)
            .file_name(image.original_name)
            .mime_str(image.mime_type.as_deref().unwrap_or("image/png"))
            .map_err(|_| ApiError::bad_request("invalid image mime type"))?;
        form = form.part(if multiple_images { "image[]" } else { "image" }, part);
    }
    if let Some(mask) = mask {
        let part = multipart::Part::bytes(mask.bytes)
            .file_name(mask.original_name)
            .mime_str(mask.mime_type.as_deref().unwrap_or("image/png"))
            .map_err(|_| ApiError::bad_request("invalid mask mime type"))?;
        form = form.part("mask", part);
    }

    let response = client
        .post(&url)
        .bearer_auth(&provider.api_key)
        .header(header::ACCEPT, "application/json")
        .header(header::ACCEPT_ENCODING, "identity")
        .multipart(form)
        .send()
        .await
        .map_err(|error| {
            ApiError::bad_request(request_error_message("image edit request failed", &error))
        })?;
    parse_direct_image_response(
        &client,
        &provider.base_url,
        &provider.api_key,
        response,
        &params.output_format,
    )
    .await
}

async fn direct_image_client(ctx: &AppContext, timeout_seconds: u64) -> ApiResult<reqwest::Client> {
    http_client::build_http_client_with_overrides(
        &ctx.db,
        HttpClientRuntimeOverrides {
            request_timeout_seconds: Some(timeout_seconds),
            disable_idle_pool: true,
            enable_http2_keep_alive: true,
        },
    )
    .await
}

async fn post_json_with_connect_retry(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
    accept: &str,
    payload: &Value,
    error_context: &str,
) -> ApiResult<reqwest::Response> {
    const MAX_ATTEMPTS: u8 = 2;
    const MAX_RETRYABLE_ELAPSED_SECONDS: u64 = 30;
    let idempotency_key = Uuid::new_v4().to_string();
    for attempt in 1..=MAX_ATTEMPTS {
        let started_at = std::time::Instant::now();
        match client
            .post(url)
            .bearer_auth(api_key)
            .header(header::ACCEPT, accept)
            .header(header::ACCEPT_ENCODING, "identity")
            .header("Idempotency-Key", &idempotency_key)
            .json(payload)
            .send()
            .await
        {
            Ok(response) => return Ok(response),
            Err(error)
                if is_retryable_image_connection_error(&error)
                    && started_at.elapsed().as_secs() <= MAX_RETRYABLE_ELAPSED_SECONDS
                    && attempt < MAX_ATTEMPTS =>
            {
                tracing::warn!(
                    attempt,
                    max_attempts = MAX_ATTEMPTS,
                    elapsed_seconds = started_at.elapsed().as_secs_f32(),
                    url,
                    error = %error,
                    "retrying image request after connection failure"
                );
                tokio::time::sleep(std::time::Duration::from_millis(500 * u64::from(attempt)))
                    .await;
            }
            Err(error) => {
                return Err(ApiError::bad_request(image_request_error_message(
                    error_context,
                    &error,
                    started_at.elapsed(),
                )));
            }
        }
    }
    Err(ApiError::bad_request(error_context))
}

fn is_retryable_image_connection_error(error: &reqwest::Error) -> bool {
    if error.is_connect() {
        return true;
    }
    let details = request_error_message("", error).to_ascii_lowercase();
    details.contains("connection error")
        || details.contains("unexpected eof")
        || details.contains("tls close_notify")
}

fn image_request_error_message(
    context: &str,
    error: &reqwest::Error,
    elapsed: std::time::Duration,
) -> String {
    let mut message = request_error_message(context, error);
    let lower = message.to_ascii_lowercase();
    if elapsed.as_secs() >= 90
        && (lower.contains("unexpected eof") || lower.contains("tls close_notify"))
    {
        message.push_str(
            "; an intermediary closed this long-running request; check the reverse proxy/CDN and local proxy/VPN/TUN idle timeouts, or use the streaming Responses image endpoint",
        );
    }
    message
}

async fn read_image_response_body(mut response: reqwest::Response) -> ApiResult<ImageResponseBody> {
    let status = response.status();
    let mut bytes = Vec::new();
    let mut read_error = None;
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => bytes.extend_from_slice(&chunk),
            Ok(None) => break,
            Err(error) => {
                read_error = Some(request_error_message(
                    "failed to read image response",
                    &error,
                ));
                break;
            }
        }
    }
    let text = String::from_utf8(bytes).map_err(|error| {
        let mut message = format!("upstream image response was not valid UTF-8: {error}");
        if let Some(read_error) = read_error.as_deref() {
            message.push_str("; ");
            message.push_str(read_error);
        }
        ApiError::bad_request(message)
    })?;
    Ok(ImageResponseBody {
        status,
        text,
        read_error,
    })
}

fn response_parse_error_message(
    message: impl Into<String>,
    response_body: &ImageResponseBody,
) -> String {
    let mut message = message.into();
    append_response_read_error(&mut message, response_body);
    message
}

fn append_response_read_error(message: &mut String, response_body: &ImageResponseBody) {
    if let Some(read_error) = response_body.read_error.as_deref() {
        message.push_str("; ");
        message.push_str(read_error);
        let _ = write!(
            message,
            "; received {} response bytes before the connection ended",
            response_body.text.len()
        );
    }
}

fn warn_on_trailing_response_error(response_body: &ImageResponseBody, url: &str) {
    if let Some(read_error) = response_body.read_error.as_deref() {
        tracing::warn!(
            url,
            received_bytes = response_body.text.len(),
            error = read_error,
            "accepted complete image payload after response connection ended with an error"
        );
    }
}

async fn parse_direct_image_response(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    response: reqwest::Response,
    output_format: &str,
) -> ApiResult<Vec<Vec<u8>>> {
    let response_body = read_image_response_body(response).await?;
    if !response_body.status.is_success() {
        let mut message = format!(
            "upstream image request failed: status={}, body={}",
            response_body.status.as_u16(),
            summarize_remote_body(&response_body.text)
        );
        append_response_read_error(&mut message, &response_body);
        return Err(ApiError::bad_request(message));
    }
    let payload: Value = serde_json::from_str(&response_body.text).map_err(|error| {
        ApiError::bad_request(response_parse_error_message(
            format!("upstream returned invalid image JSON: {error}"),
            &response_body,
        ))
    })?;
    warn_on_trailing_response_error(&response_body, base_url);
    if let Some(error) = direct_image_error(&payload) {
        return Err(ApiError::bad_request(format!(
            "upstream image request failed: {error}"
        )));
    }
    let items = payload
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::bad_request("upstream image response is missing data"))?;
    let mut images = Vec::with_capacity(items.len());
    for item in items {
        let bytes = if let Some(encoded) = item
            .get("b64_json")
            .or_else(|| item.get("base64"))
            .or_else(|| item.get("b64"))
            .and_then(Value::as_str)
        {
            decode_image(encoded)?
        } else if let Some(url) = item.get("url").and_then(Value::as_str) {
            download_direct_image(client, base_url, api_key, url).await?
        } else {
            return Err(ApiError::bad_request(
                "upstream image item has neither b64_json nor url",
            ));
        };
        validate_image(&bytes, output_format)?;
        images.push(bytes);
    }
    if images.is_empty() {
        return Err(ApiError::bad_request(
            "upstream image response returned no images",
        ));
    }
    Ok(images)
}

async fn download_direct_image(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    image_url: &str,
) -> ApiResult<Vec<u8>> {
    if image_url.trim_start().starts_with("data:") {
        return decode_image(image_url);
    }
    let url = reqwest::Url::parse(image_url)
        .or_else(|_| reqwest::Url::parse(base_url).and_then(|base| base.join(image_url.trim())))
        .map_err(|_| ApiError::bad_request("upstream returned an invalid image URL"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(ApiError::bad_request(
            "upstream returned an unsupported image URL",
        ));
    }
    let same_origin = reqwest::Url::parse(base_url).is_ok_and(|base| {
        base.scheme() == url.scheme()
            && base.host_str() == url.host_str()
            && base.port_or_known_default() == url.port_or_known_default()
    });
    let mut request = client.get(url);
    if same_origin {
        request = request.bearer_auth(api_key);
    }
    let response = request.send().await.map_err(|error| {
        ApiError::bad_request(request_error_message("image download failed", &error))
    })?;
    let status = response.status();
    if !status.is_success() {
        return Err(ApiError::bad_request(format!(
            "image download failed with status {}",
            status.as_u16()
        )));
    }
    response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|error| ApiError::bad_request(format!("failed to read image bytes: {error}")))
}

fn direct_image_error(payload: &Value) -> Option<String> {
    let error = payload.get("error")?;
    Some(
        error
            .get("message")
            .and_then(Value::as_str)
            .map_or_else(|| error.to_string(), ToString::to_string),
    )
}

fn normalize_upstream_api_key(value: &str) -> ApiResult<String> {
    let mut value = required_text(value, "api_key", 16_384)?;
    if value
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("bearer "))
    {
        value = required_text(&value[7..], "api_key", 16_384)?;
    }
    let authorization = format!("Bearer {value}");
    header::HeaderValue::from_str(&authorization).map_err(|_| {
        ApiError::bad_request(
            "api_key contains invalid characters; remove line breaks and other invisible characters",
        )
    })?;
    Ok(value)
}

fn request_error_message(context: &str, error: &reqwest::Error) -> String {
    let mut message = format!("{context}: {error}");
    let mut source = error.source();
    while let Some(cause) = source {
        let detail = cause.to_string();
        if !detail.is_empty() && !message.contains(&detail) {
            message.push_str("; caused by: ");
            message.push_str(&detail);
        }
        source = cause.source();
    }
    message
}

async fn request_image(
    ctx: &AppContext,
    provider: &ImageApiProviderConfig,
    params: &EffectiveGenerationParams,
) -> ApiResult<Vec<u8>> {
    let url = build_responses_url(&provider.base_url)?;
    let client = http_client::build_http_client_with_overrides(
        &ctx.db,
        HttpClientRuntimeOverrides {
            request_timeout_seconds: Some(params.timeout_seconds),
            disable_idle_pool: true,
            enable_http2_keep_alive: true,
        },
    )
    .await?;
    let mut tool = json!({
        "type": "image_generation",
        "action": "generate",
        "model": &params.image_model,
        "size": &params.size,
        "quality": &params.quality,
        "output_format": &params.output_format,
    });
    if params.partial_images > 0 {
        tool["partial_images"] = json!(params.partial_images);
    }
    let payload = json!({
        "model": &params.base_model,
        "instructions": "",
        "stream": true,
        "store": false,
        "reasoning": {
            "effort": &params.reasoning_effort,
            "summary": "auto"
        },
        "parallel_tool_calls": true,
        "include": ["reasoning.encrypted_content"],
        "tool_choice": {"type": "image_generation"},
        "input": [{
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": &params.prompt}]
        }],
        "tools": [tool]
    });

    let response = post_json_with_connect_retry(
        &client,
        &url,
        &provider.api_key,
        "text/event-stream, application/json",
        &payload,
        "image request failed",
    )
    .await?;
    let response_body = read_image_response_body(response).await?;
    if !response_body.status.is_success() {
        let mut message = format!(
            "upstream image request failed: status={}, body={}",
            response_body.status.as_u16(),
            summarize_remote_body(&response_body.text)
        );
        append_response_read_error(&mut message, &response_body);
        return Err(ApiError::bad_request(message));
    }

    let encoded = extract_generated_image(&response_body.text).map_err(|error| {
        ApiError::bad_request(response_parse_error_message(
            error.message().to_string(),
            &response_body,
        ))
    })?;
    warn_on_trailing_response_error(&response_body, &url);
    let image = decode_image(&encoded)?;
    validate_image(&image, &params.output_format)?;
    Ok(image)
}

fn build_responses_url(base_url: &str) -> ApiResult<String> {
    let mut url = reqwest::Url::parse(base_url.trim())
        .map_err(|_| ApiError::bad_request("invalid ai image base_url"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(ApiError::bad_request("invalid ai image base_url"));
    }
    url.set_query(None);
    url.set_fragment(None);

    let mut path = normalized_image_api_base_path(url.path());
    path.push_str("/responses");
    url.set_path(&path);
    Ok(url.to_string())
}

fn build_images_url(base_url: &str, operation: &str) -> ApiResult<String> {
    ensure_choice("image operation", operation, &["generations", "edits"])?;
    let mut url = reqwest::Url::parse(base_url.trim())
        .map_err(|_| ApiError::bad_request("invalid ai image base_url"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(ApiError::bad_request("invalid ai image base_url"));
    }
    url.set_query(None);
    url.set_fragment(None);
    let mut path = normalized_image_api_base_path(url.path());
    path.push_str("/images/");
    path.push_str(operation);
    url.set_path(&path);
    Ok(url.to_string())
}

fn normalized_image_api_base_path(path: &str) -> String {
    let mut path = path.trim_end_matches('/').to_string();
    for suffix in ["/images/generations", "/images/edits", "/responses"] {
        if let Some(base_path) = path.strip_suffix(suffix) {
            path = base_path.to_string();
            break;
        }
    }
    if path.is_empty() {
        "/v1".to_string()
    } else {
        path
    }
}

fn extract_generated_image(body: &str) -> ApiResult<String> {
    let body = body.trim();
    if body.is_empty() {
        return Err(ApiError::bad_request("upstream image response is empty"));
    }
    if let Ok(payload) = serde_json::from_str::<Value>(body) {
        if let Some(error) = extract_error_message(&payload, "") {
            return Err(ApiError::bad_request(format!(
                "upstream image generation failed: {error}"
            )));
        }
        if let Some(image) = extract_final_image(&payload) {
            return Ok(image);
        }
        if extract_partial_image(&payload).is_some() {
            return Err(ApiError::bad_request(
                "upstream returned only a partial image",
            ));
        }
    }

    let mut event_name = String::new();
    let mut data_lines = Vec::new();
    let mut final_image = None;
    let mut saw_partial = false;
    for raw_line in body.lines().chain(std::iter::once("")) {
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() {
            if !data_lines.is_empty() {
                let raw_data = data_lines.join("\n");
                data_lines.clear();
                if raw_data.trim() != "[DONE]" {
                    if let Ok(payload) = serde_json::from_str::<Value>(&raw_data) {
                        if let Some(error) = extract_error_message(&payload, &event_name) {
                            return Err(ApiError::bad_request(format!(
                                "upstream image generation failed: {error}"
                            )));
                        }
                        if let Some(image) = extract_final_image(&payload) {
                            final_image = Some(image);
                        }
                        saw_partial |= extract_partial_image(&payload).is_some();
                    }
                }
            }
            event_name.clear();
        } else if let Some(value) = line.strip_prefix("event:") {
            event_name = value.trim().to_string();
        } else if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.trim_start().to_string());
        }
    }

    final_image.ok_or_else(|| {
        if saw_partial {
            ApiError::bad_request("upstream returned only partial images without a final image")
        } else {
            ApiError::bad_request("upstream image response did not contain a final image")
        }
    })
}

fn extract_final_image(payload: &Value) -> Option<String> {
    if let Some(image) = payload
        .get("data")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|item| item.get("b64_json"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        return Some(image.to_string());
    }

    match payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "response.output_item.done" => payload
            .get("item")
            .filter(|item| {
                item.get("type").and_then(Value::as_str) == Some("image_generation_call")
            })
            .and_then(|item| item.get("result"))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(ToString::to_string),
        "image_generation.completed" | "response.image_generation_call.completed" => payload
            .get("b64_json")
            .or_else(|| payload.get("result"))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(ToString::to_string),
        "response.completed" => payload
            .get("response")
            .and_then(|response| response.get("output"))
            .and_then(Value::as_array)
            .and_then(|items| {
                items.iter().find_map(|item| {
                    (item.get("type").and_then(Value::as_str) == Some("image_generation_call"))
                        .then(|| item.get("result").and_then(Value::as_str))
                        .flatten()
                        .filter(|value| !value.trim().is_empty())
                        .map(ToString::to_string)
                })
            }),
        _ => None,
    }
}

fn extract_partial_image(payload: &Value) -> Option<&str> {
    let event_type = payload.get("type").and_then(Value::as_str)?;
    if !event_type.ends_with("partial_image") {
        return None;
    }
    payload
        .get("partial_image_b64")
        .or_else(|| payload.get("b64_json"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn extract_error_message(payload: &Value, event_name: &str) -> Option<String> {
    let event_type = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if event_name != "error" && !matches!(event_type, "error" | "response.failed") {
        return None;
    }
    if let Some(error) = payload.get("error") {
        return Some(
            error
                .get("message")
                .and_then(Value::as_str)
                .map_or_else(|| error.to_string(), ToString::to_string),
        );
    }
    if let Some(error) = payload
        .get("response")
        .and_then(|response| response.get("error"))
    {
        return Some(error.to_string());
    }
    Some(summarize_remote_body(&payload.to_string()))
}

fn decode_image(value: &str) -> ApiResult<Vec<u8>> {
    let encoded = value
        .trim()
        .strip_prefix("data:")
        .and_then(|value| value.split_once(',').map(|(_, encoded)| encoded))
        .unwrap_or_else(|| value.trim());
    let encoded = encoded
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    [
        general_purpose::STANDARD,
        general_purpose::STANDARD_NO_PAD,
        general_purpose::URL_SAFE,
        general_purpose::URL_SAFE_NO_PAD,
    ]
    .iter()
    .find_map(|engine| engine.decode(&encoded).ok())
    .ok_or_else(|| ApiError::bad_request("upstream returned invalid base64 image data"))
}

fn validate_image(bytes: &[u8], output_format: &str) -> ApiResult<()> {
    let valid = match output_format {
        "png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "jpeg" => bytes.starts_with(b"\xff\xd8\xff"),
        "webp" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!(
            "upstream response is not a valid {output_format} image"
        )))
    }
}

fn generated_filename(output_format: &str) -> String {
    let extension = if output_format == "jpeg" {
        "jpg"
    } else {
        output_format
    };
    format!(
        "cpamc-image-{}-{}.{}",
        Utc::now().format("%Y%m%d%H%M%S"),
        Uuid::new_v4().simple(),
        extension
    )
}

fn direct_image_filename(operation: &str, output_format: &str, index: usize) -> String {
    let extension = if output_format == "jpeg" {
        "jpg"
    } else {
        output_format
    };
    format!(
        "ai-image-{operation}-{}-{}-{index}.{extension}",
        Utc::now().format("%Y%m%d%H%M%S"),
        Uuid::new_v4().simple(),
    )
}

fn output_mime_type(output_format: &str) -> &'static str {
    match output_format {
        "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "image/png",
    }
}

fn sanitize_input_filename(value: &str) -> String {
    value
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(character, ':' | '*' | '?' | '"' | '<' | '>' | '|')
            {
                '_'
            } else {
                character
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

fn parse_u32_field(value: &str, field: &str) -> ApiResult<u32> {
    value
        .trim()
        .parse::<u32>()
        .map_err(|_| ApiError::bad_request(format!("invalid {field}")))
}

fn parse_u64_field(value: &str, field: &str) -> ApiResult<u64> {
    value
        .trim()
        .parse::<u64>()
        .map_err(|_| ApiError::bad_request(format!("invalid {field}")))
}

fn parse_i32_field(value: &str, field: &str) -> ApiResult<i32> {
    value
        .trim()
        .parse::<i32>()
        .map_err(|_| ApiError::bad_request(format!("invalid {field}")))
}

async fn upload_limit_bytes(ctx: &AppContext) -> ApiResult<i64> {
    Ok(system_settings::upload_limit_bytes(&ctx.db).await?)
}

fn required_text(value: &str, field: &str, max_chars: usize) -> ApiResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(ApiError::bad_request(format!("{field} is required")));
    }
    if value.chars().count() > max_chars {
        return Err(ApiError::bad_request(format!("{field} is too long")));
    }
    Ok(value.to_string())
}

fn optional_text(
    value: Option<String>,
    field: &str,
    max_chars: usize,
) -> ApiResult<Option<String>> {
    value
        .map(|value| required_text(&value, field, max_chars))
        .transpose()
}

fn environment_text(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn ensure_choice(field: &str, value: &str, choices: &[&str]) -> ApiResult<()> {
    if choices.contains(&value) {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!("invalid {field}")))
    }
}

fn summarize_remote_body(body: &str) -> String {
    let body = body.trim();
    if body.is_empty() {
        "empty response body".to_string()
    } else {
        body.chars().take(1_000).collect()
    }
}

async fn record_image_api_log(
    ctx: &AppContext,
    user: &crate::models::users::Model,
    action: &str,
    path: &'static str,
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
            module: "image_api",
            action,
            message: &message,
            user_id: Some(user.id),
            operator: Some(user.email.clone()),
            method: Some("POST"),
            path: Some(path),
            status,
            error_message,
        },
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    const IMAGE_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Wl2nWQAAAAASUVORK5CYII=";

    #[test]
    fn responses_url_is_normalized() {
        assert_eq!(
            build_responses_url("https://api.example.test/v1").unwrap(),
            "https://api.example.test/v1/responses"
        );
        assert_eq!(
            build_responses_url("https://api.example.test").unwrap(),
            "https://api.example.test/v1/responses"
        );
        assert_eq!(
            build_responses_url("https://api.example.test/v1/images/generations").unwrap(),
            "https://api.example.test/v1/responses"
        );
        assert_eq!(
            build_responses_url("https://api.example.test/v1/responses").unwrap(),
            "https://api.example.test/v1/responses"
        );
    }

    #[test]
    fn direct_image_urls_are_normalized() {
        assert_eq!(
            build_images_url("https://api.example.test/v1", "generations").unwrap(),
            "https://api.example.test/v1/images/generations"
        );
        assert_eq!(
            build_images_url("https://api.example.test/v1/responses", "edits").unwrap(),
            "https://api.example.test/v1/images/edits"
        );
        assert_eq!(
            build_images_url("https://api.example.test/v1/images/generations", "edits").unwrap(),
            "https://api.example.test/v1/images/edits"
        );
    }

    #[test]
    fn extracts_final_image_from_sse() {
        let body = format!(
            "event: response.output_item.done\ndata: {{\"type\":\"response.output_item.done\",\"item\":{{\"type\":\"image_generation_call\",\"result\":\"{IMAGE_B64}\"}}}}\n\ndata: [DONE]\n\n"
        );
        assert_eq!(extract_generated_image(&body).unwrap(), IMAGE_B64);
    }

    #[test]
    fn extracts_final_image_when_stream_ends_without_done_marker() {
        let body = format!(
            "event: response.output_item.done\ndata: {{\"type\":\"response.output_item.done\",\"item\":{{\"type\":\"image_generation_call\",\"result\":\"{IMAGE_B64}\"}}}}\n\n"
        );
        assert_eq!(extract_generated_image(&body).unwrap(), IMAGE_B64);
    }

    #[test]
    fn extracts_image_from_json_response() {
        let body = json!({"data": [{"b64_json": IMAGE_B64}]}).to_string();
        assert_eq!(extract_generated_image(&body).unwrap(), IMAGE_B64);
    }

    #[test]
    fn rejects_partial_only_stream() {
        let body = format!(
            "data: {{\"type\":\"response.image_generation_call.partial_image\",\"partial_image_b64\":\"{IMAGE_B64}\"}}\n\ndata: [DONE]\n\n"
        );
        let error = extract_generated_image(&body).unwrap_err();
        assert!(error.message().contains("partial"));
    }

    #[test]
    fn validates_supported_image_signatures() {
        assert!(validate_image(b"\x89PNG\r\n\x1a\nrest", "png").is_ok());
        assert!(validate_image(b"\xff\xd8\xffrest", "jpeg").is_ok());
        assert!(validate_image(b"RIFFxxxxWEBPrest", "webp").is_ok());
        assert!(validate_image(b"not-an-image", "png").is_err());
    }

    #[test]
    fn normalizes_and_validates_upstream_api_keys() {
        assert_eq!(
            normalize_upstream_api_key("Bearer upstream-key").unwrap(),
            "upstream-key"
        );
        assert_eq!(
            normalize_upstream_api_key(" upstream-key \n").unwrap(),
            "upstream-key"
        );
        assert!(normalize_upstream_api_key("upstream\nkey").is_err());
    }
}
