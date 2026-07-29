#![allow(clippy::missing_errors_doc)]

use std::{
    fs,
    path::{Component, Path as FsPath},
};

use chrono::Local;
use loco_rs::prelude::*;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, EntityTrait, IntoActiveModel, PaginatorTrait,
    QueryFilter, QueryOrder, Set,
};
use serde::{Deserialize, Serialize};

use crate::{
    controllers::admin::{authorize, file_manager::FileRootRecord},
    errors::{ApiError, ApiResult},
    models::{
        _entities::{chat_message_attachments, chat_messages, upload_files},
        system_settings,
    },
    responses::{self, ApiResponse, EmptyData, PageResponse},
};

const FILE_ROOTS_SETTING_KEY: &str = "file_manager.roots";
const SOURCE_MATERIAL: &str = "material";
const SOURCE_LOCAL_FILE: &str = "local_file";

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct ChatMessageQueryParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub keyword: Option<String>,
    pub status: Option<String>,
}

impl ChatMessageQueryParams {
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
pub struct ChatAttachmentParams {
    pub source_type: String,
    pub upload_file_id: Option<i32>,
    pub file_root_key: Option<String>,
    pub file_path: Option<String>,
    pub display_name: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveChatMessageParams {
    pub title: String,
    pub content: String,
    pub status: Option<String>,
    pub sent_at: Option<String>,
    pub remark: Option<String>,
    pub attachments: Option<Vec<ChatAttachmentParams>>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct ChatMessageRecord {
    pub id: i32,
    pub title: String,
    pub content: String,
    pub status: String,
    pub sent_at: Option<String>,
    pub copy_count: i32,
    pub last_copied_at: Option<String>,
    pub remark: Option<String>,
    pub attachment_count: u64,
    pub created_by: Option<i32>,
    pub updated_by: Option<i32>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct ChatAttachmentRecord {
    pub id: i32,
    pub source_type: String,
    pub upload_file_id: Option<i32>,
    pub file_root_key: Option<String>,
    pub file_path: Option<String>,
    pub display_name: String,
    pub mime_type: Option<String>,
    pub size_bytes: i64,
    pub sort_order: i32,
    pub material_url: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct ChatMessageDetailRecord {
    #[serde(flatten)]
    pub message: ChatMessageRecord,
    pub attachments: Vec<ChatAttachmentRecord>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CopyChatMessageRecord {
    pub id: i32,
    pub content: String,
    pub copy_count: i32,
    pub last_copied_at: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/admin/chat-messages",
    tag = "admin-chat-messages",
    security(("bearer_auth" = [])),
    params(ChatMessageQueryParams),
    responses((status = 200, body = ApiResponse<PageResponse<ChatMessageRecord>>))
)]
#[debug_handler]
pub async fn list(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<ChatMessageQueryParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:chat:list").await?;
    let page = params.page();
    let page_size = params.page_size();
    let mut query = chat_messages::Entity::find().order_by_desc(chat_messages::Column::UpdatedAt);

    if let Some(keyword) = params
        .keyword
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        query = query.filter(
            Condition::any()
                .add(chat_messages::Column::Title.contains(keyword))
                .add(chat_messages::Column::Content.contains(keyword))
                .add(chat_messages::Column::Remark.contains(keyword)),
        );
    }
    if let Some(status) = params
        .status
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        query = query.filter(chat_messages::Column::Status.eq(status));
    }

    let total = query.clone().count(&ctx.db).await?;
    let messages = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?;
    let mut items = Vec::with_capacity(messages.len());
    for message in messages {
        items.push(record_with_attachment_count(&ctx, message).await?);
    }

    Ok(responses::ok(PageResponse {
        items,
        page,
        page_size,
        total,
    }))
}

#[utoipa::path(
    get,
    path = "/api/admin/chat-messages/{id}",
    tag = "admin-chat-messages",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<ChatMessageDetailRecord>))
)]
#[debug_handler]
pub async fn get(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:chat:detail").await?;
    let message = find_message(&ctx, id).await?;
    Ok(responses::ok(detail_record(&ctx, message).await?))
}

#[utoipa::path(
    post,
    path = "/api/admin/chat-messages",
    tag = "admin-chat-messages",
    security(("bearer_auth" = [])),
    request_body = SaveChatMessageParams,
    responses((status = 200, body = ApiResponse<ChatMessageDetailRecord>))
)]
#[debug_handler]
pub async fn create(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SaveChatMessageParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:chat:create").await?;
    let draft = normalize_message(&params)?;
    let message = chat_messages::ActiveModel {
        title: Set(draft.title),
        content: Set(draft.content),
        status: Set(draft.status),
        sent_at: Set(draft.sent_at),
        copy_count: Set(0),
        last_copied_at: Set(None),
        remark: Set(draft.remark),
        created_by: Set(Some(actor.id)),
        updated_by: Set(Some(actor.id)),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;
    sync_attachments(
        &ctx,
        message.id,
        actor.id,
        params.attachments.unwrap_or_default(),
    )
    .await?;
    Ok(responses::ok(detail_record(&ctx, message).await?))
}

#[utoipa::path(
    put,
    path = "/api/admin/chat-messages/{id}",
    tag = "admin-chat-messages",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = SaveChatMessageParams,
    responses((status = 200, body = ApiResponse<ChatMessageDetailRecord>))
)]
#[debug_handler]
pub async fn update(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<SaveChatMessageParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:chat:update").await?;
    let draft = normalize_message(&params)?;
    let message = find_message(&ctx, id).await?;
    let mut active = message.into_active_model();
    active.title = Set(draft.title);
    active.content = Set(draft.content);
    active.status = Set(draft.status);
    active.sent_at = Set(draft.sent_at);
    active.remark = Set(draft.remark);
    active.updated_by = Set(Some(actor.id));
    let message = active.update(&ctx.db).await?;
    sync_attachments(
        &ctx,
        message.id,
        actor.id,
        params.attachments.unwrap_or_default(),
    )
    .await?;
    Ok(responses::ok(detail_record(&ctx, message).await?))
}

#[utoipa::path(
    delete,
    path = "/api/admin/chat-messages/{id}",
    tag = "admin-chat-messages",
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
    authorize(&ctx, &auth, "system:chat:delete").await?;
    find_message(&ctx, id).await?;
    chat_message_attachments::Entity::delete_many()
        .filter(chat_message_attachments::Column::ChatMessageId.eq(id))
        .exec(&ctx.db)
        .await?;
    chat_messages::Entity::delete_by_id(id)
        .exec(&ctx.db)
        .await?;
    Ok(responses::empty())
}

#[utoipa::path(
    post,
    path = "/api/admin/chat-messages/{id}/mark-sent",
    tag = "admin-chat-messages",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<ChatMessageRecord>))
)]
#[debug_handler]
pub async fn mark_sent(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:chat:update").await?;
    let message = find_message(&ctx, id).await?;
    let mut active = message.into_active_model();
    active.status = Set("sent".to_string());
    active.sent_at = Set(Some(Local::now().into()));
    active.updated_by = Set(Some(actor.id));
    let message = active.update(&ctx.db).await?;
    Ok(responses::ok(
        record_with_attachment_count(&ctx, message).await?,
    ))
}

#[utoipa::path(
    post,
    path = "/api/admin/chat-messages/{id}/copy",
    tag = "admin-chat-messages",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<CopyChatMessageRecord>))
)]
#[debug_handler]
pub async fn copy(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:chat:copy").await?;
    let message = find_message(&ctx, id).await?;
    let mut active = message.into_active_model();
    let copy_count = active.copy_count.as_ref().saturating_add(1);
    active.copy_count = Set(copy_count);
    active.last_copied_at = Set(Some(Local::now().into()));
    let message = active.update(&ctx.db).await?;
    Ok(responses::ok(CopyChatMessageRecord {
        id: message.id,
        content: message.content,
        copy_count: message.copy_count,
        last_copied_at: message.last_copied_at.map(|time| time.to_rfc3339()),
    }))
}

struct NormalizedMessage {
    title: String,
    content: String,
    status: String,
    sent_at: Option<chrono::DateTime<chrono::FixedOffset>>,
    remark: Option<String>,
}

struct NormalizedAttachment {
    source_type: String,
    upload_file_id: Option<i32>,
    file_root_key: Option<String>,
    file_path: Option<String>,
    display_name: String,
    mime_type: Option<String>,
    size_bytes: i64,
}

fn normalize_message(params: &SaveChatMessageParams) -> ApiResult<NormalizedMessage> {
    let title = params.title.trim().to_string();
    if title.is_empty() {
        return Err(ApiError::bad_request("title is required"));
    }
    let content = params.content.trim().to_string();
    if content.is_empty() {
        return Err(ApiError::bad_request("content is required"));
    }
    let status = params
        .status
        .as_deref()
        .unwrap_or("draft")
        .trim()
        .to_string();
    if !matches!(status.as_str(), "draft" | "sent" | "archived") {
        return Err(ApiError::bad_request("invalid chat status"));
    }
    Ok(NormalizedMessage {
        title,
        content,
        status,
        sent_at: parse_optional_datetime(params.sent_at.as_deref())?,
        remark: params.remark.as_deref().and_then(trim_optional),
    })
}

async fn sync_attachments(
    ctx: &AppContext,
    message_id: i32,
    actor_id: i32,
    attachments: Vec<ChatAttachmentParams>,
) -> ApiResult<()> {
    let mut normalized = Vec::with_capacity(attachments.len());
    for params in attachments {
        normalized.push(normalize_attachment(ctx, params).await?);
    }
    chat_message_attachments::Entity::delete_many()
        .filter(chat_message_attachments::Column::ChatMessageId.eq(message_id))
        .exec(&ctx.db)
        .await?;
    for (index, attachment) in normalized.into_iter().enumerate() {
        let sort_order = i32::try_from(index).unwrap_or(i32::MAX);
        chat_message_attachments::ActiveModel {
            chat_message_id: Set(message_id),
            source_type: Set(attachment.source_type),
            upload_file_id: Set(attachment.upload_file_id),
            file_root_key: Set(attachment.file_root_key),
            file_path: Set(attachment.file_path),
            display_name: Set(attachment.display_name),
            mime_type: Set(attachment.mime_type),
            size_bytes: Set(attachment.size_bytes),
            sort_order: Set(sort_order),
            created_by: Set(Some(actor_id)),
            ..Default::default()
        }
        .insert(&ctx.db)
        .await?;
    }
    Ok(())
}

async fn normalize_attachment(
    ctx: &AppContext,
    params: ChatAttachmentParams,
) -> ApiResult<NormalizedAttachment> {
    match params.source_type.trim() {
        SOURCE_MATERIAL => normalize_material_attachment(ctx, params).await,
        SOURCE_LOCAL_FILE => normalize_local_attachment(ctx, params).await,
        _ => Err(ApiError::bad_request("invalid attachment source type")),
    }
}

async fn normalize_material_attachment(
    ctx: &AppContext,
    params: ChatAttachmentParams,
) -> ApiResult<NormalizedAttachment> {
    let upload_file_id = params
        .upload_file_id
        .ok_or_else(|| ApiError::bad_request("upload_file_id is required"))?;
    let file = upload_files::Entity::find_by_id(upload_file_id)
        .one(&ctx.db)
        .await?
        .filter(|file| file.status == "active")
        .ok_or_else(|| ApiError::bad_request("upload file not found"))?;
    Ok(NormalizedAttachment {
        source_type: SOURCE_MATERIAL.to_string(),
        upload_file_id: Some(file.id),
        file_root_key: None,
        file_path: None,
        display_name: params
            .display_name
            .as_deref()
            .and_then(trim_optional)
            .unwrap_or(file.original_name),
        mime_type: params
            .mime_type
            .as_deref()
            .and_then(trim_optional)
            .or(file.mime_type),
        size_bytes: params.size_bytes.unwrap_or(file.size_bytes),
    })
}

async fn normalize_local_attachment(
    ctx: &AppContext,
    params: ChatAttachmentParams,
) -> ApiResult<NormalizedAttachment> {
    let root_key = params
        .file_root_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::bad_request("file_root_key is required"))?;
    let file_path = normalize_required_path(params.file_path.as_deref())?;
    let root = find_file_root(ctx, root_key).await?;
    let metadata = local_file_metadata(&root, &file_path)?;
    if !metadata.is_file() {
        return Err(ApiError::bad_request("file path is not a file"));
    }
    Ok(NormalizedAttachment {
        source_type: SOURCE_LOCAL_FILE.to_string(),
        upload_file_id: None,
        file_root_key: Some(root.key),
        file_path: Some(file_path.clone()),
        display_name: params
            .display_name
            .as_deref()
            .and_then(trim_optional)
            .unwrap_or_else(|| {
                file_path
                    .rsplit('/')
                    .next()
                    .unwrap_or(&file_path)
                    .to_string()
            }),
        mime_type: params
            .mime_type
            .as_deref()
            .and_then(trim_optional)
            .or_else(|| mime_type(&file_path)),
        size_bytes: params
            .size_bytes
            .unwrap_or_else(|| i64::try_from(metadata.len()).unwrap_or(i64::MAX)),
    })
}

async fn detail_record(
    ctx: &AppContext,
    message: chat_messages::Model,
) -> ApiResult<ChatMessageDetailRecord> {
    let attachments = chat_message_attachments::Entity::find()
        .filter(chat_message_attachments::Column::ChatMessageId.eq(message.id))
        .order_by_asc(chat_message_attachments::Column::SortOrder)
        .all(&ctx.db)
        .await?;
    let mut records = Vec::with_capacity(attachments.len());
    for attachment in attachments {
        records.push(attachment_record(ctx, attachment).await?);
    }
    Ok(ChatMessageDetailRecord {
        message: ChatMessageRecord::from_model(message, u64::try_from(records.len()).unwrap_or(0)),
        attachments: records,
    })
}

async fn attachment_record(
    ctx: &AppContext,
    attachment: chat_message_attachments::Model,
) -> ApiResult<ChatAttachmentRecord> {
    let material_url = if let Some(upload_file_id) = attachment.upload_file_id {
        upload_files::Entity::find_by_id(upload_file_id)
            .one(&ctx.db)
            .await?
            .map(|file| file.url)
    } else {
        None
    };
    Ok(ChatAttachmentRecord {
        id: attachment.id,
        source_type: attachment.source_type,
        upload_file_id: attachment.upload_file_id,
        file_root_key: attachment.file_root_key,
        file_path: attachment.file_path,
        display_name: attachment.display_name,
        mime_type: attachment.mime_type,
        size_bytes: attachment.size_bytes,
        sort_order: attachment.sort_order,
        material_url,
        created_at: attachment.created_at.to_rfc3339(),
    })
}

async fn record_with_attachment_count(
    ctx: &AppContext,
    message: chat_messages::Model,
) -> ApiResult<ChatMessageRecord> {
    let count = chat_message_attachments::Entity::find()
        .filter(chat_message_attachments::Column::ChatMessageId.eq(message.id))
        .count(&ctx.db)
        .await?;
    Ok(ChatMessageRecord::from_model(message, count))
}

async fn find_message(ctx: &AppContext, id: i32) -> ApiResult<chat_messages::Model> {
    chat_messages::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("chat message not found"))
}

async fn find_file_root(ctx: &AppContext, key: &str) -> ApiResult<FileRootRecord> {
    let value = system_settings::string_value(&ctx.db, FILE_ROOTS_SETTING_KEY, "[]").await?;
    let roots = serde_json::from_str::<Vec<FileRootRecord>>(&value)
        .map_err(|_| ApiError::bad_request("invalid file manager roots setting"))?;
    roots
        .into_iter()
        .map(normalize_root)
        .collect::<ApiResult<Vec<_>>>()?
        .into_iter()
        .find(|root| root.enabled && root.key == key)
        .ok_or_else(|| ApiError::bad_request("file root not found"))
}

fn normalize_root(mut root: FileRootRecord) -> ApiResult<FileRootRecord> {
    root.key = root.key.trim().to_string();
    root.name = root.name.trim().to_string();
    root.url_path = root.url_path.trim().trim_end_matches('/').to_string();
    root.local_root = root.local_root.trim().to_string();
    if root.key.is_empty() || root.name.is_empty() || root.local_root.is_empty() {
        return Err(ApiError::bad_request("invalid file root configuration"));
    }
    Ok(root)
}

fn local_file_metadata(root: &FileRootRecord, relative: &str) -> ApiResult<fs::Metadata> {
    let root_path = fs::canonicalize(&root.local_root)
        .map_err(|_| ApiError::bad_request("file root not found"))?;
    let target = fs::canonicalize(root_path.join(relative))
        .map_err(|_| ApiError::bad_request("file path not found"))?;
    if !target.starts_with(&root_path) {
        return Err(ApiError::forbidden("file path is outside configured root"));
    }
    target
        .metadata()
        .map_err(|_| ApiError::bad_request("file path not found"))
}

fn normalize_required_path(path: Option<&str>) -> ApiResult<String> {
    let path = normalize_relative_path(path)?;
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
    if FsPath::new(path).components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(ApiError::bad_request("invalid file path"));
    }
    Ok(path.to_string())
}

fn parse_optional_datetime(
    value: Option<&str>,
) -> ApiResult<Option<chrono::DateTime<chrono::FixedOffset>>> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    chrono::DateTime::parse_from_rfc3339(value)
        .map(Some)
        .map_err(|_| ApiError::bad_request("invalid datetime"))
}

fn trim_optional(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn mime_type(path: &str) -> Option<String> {
    mime_guess::from_path(path).first_raw().map(str::to_string)
}

impl ChatMessageRecord {
    fn from_model(message: chat_messages::Model, attachment_count: u64) -> Self {
        Self {
            id: message.id,
            title: message.title,
            content: message.content,
            status: message.status,
            sent_at: message.sent_at.map(|time| time.to_rfc3339()),
            copy_count: message.copy_count,
            last_copied_at: message.last_copied_at.map(|time| time.to_rfc3339()),
            remark: message.remark,
            attachment_count,
            created_by: message.created_by,
            updated_by: message.updated_by,
            created_at: message.created_at.to_rfc3339(),
            updated_at: message.updated_at.to_rfc3339(),
        }
    }
}
