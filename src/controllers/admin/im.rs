//! 在线聊天: direct and group conversations between back-office users.
//!
//! Members send text, images and files; every change is pushed to the
//! members' event streams as an `im` event. Group events (created, members
//! invited or removed, renamed) are stored as `system` messages whose JSON
//! content the client renders in the viewer's language.

#![allow(clippy::missing_errors_doc)]

use std::{
    collections::{HashMap, HashSet},
    path::{Path as FsPath, PathBuf},
};

use axum::{
    extract::Multipart,
    http::{header, HeaderMap, HeaderName, HeaderValue},
};
use loco_rs::prelude::*;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, EntityTrait, IntoActiveModel, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, Set, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    controllers::admin::{
        authorize,
        file_manager::{
            content_disposition, mime_type, read_file_field, sanitize_filename, FileDisposition,
        },
    },
    errors::{ApiError, ApiResult},
    models::{
        _entities::{im_conversations, im_members, im_messages, users::Column as UserColumn},
        rbac::{self, EffectiveDataScope},
        system_settings,
        user_sessions::USER_ACTIVE,
        users,
    },
    responses::{self, ApiResponse, EmptyData},
    services::{file_stream, realtime, recycle_bin, storage},
};

const PERMISSION: &str = "system:im:chat";
pub const STORAGE_DIR: &str = "storage/im";
const KIND_DIRECT: &str = "direct";
const KIND_GROUP: &str = "group";
const ROLE_OWNER: &str = "owner";
const ROLE_MEMBER: &str = "member";
const MESSAGE_TEXT: &str = "text";
const MESSAGE_IMAGE: &str = "image";
const MESSAGE_FILE: &str = "file";
const MESSAGE_SYSTEM: &str = "system";
const MAX_TEXT_CHARS: usize = 5000;
const MAX_NAME_CHARS: usize = 60;
const MAX_GROUP_MEMBERS: usize = 500;
const MAX_CONTACTS: u64 = 500;
const NOT_FOUND_CODE: &str = "IM_CONVERSATION_NOT_FOUND";

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct ContactParams {
    pub keyword: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct ContactRecord {
    pub id: i32,
    pub name: String,
    pub email: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct MemberRecord {
    pub user_id: i32,
    pub name: String,
    pub email: String,
    pub role: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
pub struct MessageRecord {
    pub id: i32,
    pub conversation_id: i32,
    pub sender_id: Option<i32>,
    pub sender_name: Option<String>,
    /// `text`, `image`, `file` or `system` (JSON describing a group event).
    pub kind: String,
    pub content: String,
    pub file_name: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: Option<i64>,
    pub created_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct ConversationRecord {
    pub id: i32,
    /// `direct` or `group`.
    pub kind: String,
    /// The group name, or the other person's name in a direct chat.
    pub name: String,
    pub owner_id: Option<i32>,
    /// The other person in a direct chat.
    pub peer_id: Option<i32>,
    /// The signed-in user, so the client can tell its own messages apart.
    pub viewer_id: i32,
    pub member_count: u64,
    pub unread_count: u64,
    pub last_message: Option<MessageRecord>,
    pub last_message_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct ConversationDetailRecord {
    #[serde(flatten)]
    pub conversation: ConversationRecord,
    pub members: Vec<MemberRecord>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct DirectParams {
    pub user_id: i32,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct GroupParams {
    pub name: Option<String>,
    pub member_ids: Vec<i32>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct RenameParams {
    pub name: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct AddMembersParams {
    pub user_ids: Vec<i32>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct MessageListParams {
    /// Older messages than this id (scrolling back).
    pub before_id: Option<i32>,
    /// Newer messages than this id (catching up).
    pub after_id: Option<i32>,
    pub limit: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SendTextParams {
    pub content: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct ReadParams {
    /// Read up to this message; the latest one when omitted.
    pub message_id: Option<i32>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct FileParams {
    pub download: Option<bool>,
}

fn display_name(user: &users::Model) -> String {
    if user.name.trim().is_empty() {
        user.email.clone()
    } else {
        user.name.clone()
    }
}

/// Users someone may chat with: other active accounts, limited to their
/// tenant unless their data scope covers everything.
async fn contact_scope(ctx: &AppContext, me: &users::Model) -> ApiResult<Condition> {
    let mut condition = Condition::all()
        .add(UserColumn::Status.eq(USER_ACTIVE))
        .add(UserColumn::Id.ne(me.id));
    let tenant_id = match rbac::resolve_data_scope(&ctx.db, me).await? {
        EffectiveDataScope::All => None,
        EffectiveDataScope::Tenant { tenant_id }
        | EffectiveDataScope::Department { tenant_id, .. } => Some(tenant_id),
        EffectiveDataScope::SelfOnly { .. } | EffectiveDataScope::None => me.tenant_id,
    };
    if let Some(tenant_id) = tenant_id {
        condition = condition.add(UserColumn::TenantId.eq(tenant_id));
    }
    Ok(condition)
}

async fn assert_contacts(
    ctx: &AppContext,
    me: &users::Model,
    ids: &[i32],
) -> ApiResult<Vec<users::Model>> {
    let wanted: HashSet<i32> = ids.iter().copied().filter(|id| *id != me.id).collect();
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let found = users::Entity::find()
        .filter(contact_scope(ctx, me).await?)
        .filter(UserColumn::Id.is_in(wanted.iter().copied()))
        .all(&ctx.db)
        .await?;
    if found.len() != wanted.len() {
        return Err(ApiError::bad_request("部分用户不存在、已停用或不能聊天"));
    }
    Ok(found)
}

fn not_found() -> ApiError {
    ApiError::not_found("会话不存在或你已不在会话中").with_code(NOT_FOUND_CODE)
}

/// The conversation and the caller's membership; strangers get not found.
async fn membership(
    ctx: &AppContext,
    id: i32,
    user_id: i32,
) -> ApiResult<(im_conversations::Model, im_members::Model)> {
    let member = im_members::Entity::find()
        .filter(im_members::Column::ConversationId.eq(id))
        .filter(im_members::Column::UserId.eq(user_id))
        .one(&ctx.db)
        .await?
        .ok_or_else(not_found)?;
    let conversation = im_conversations::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(not_found)?;
    Ok((conversation, member))
}

async fn group_membership(
    ctx: &AppContext,
    id: i32,
    user_id: i32,
) -> ApiResult<(im_conversations::Model, im_members::Model)> {
    let found = membership(ctx, id, user_id).await?;
    if found.0.kind != KIND_GROUP {
        return Err(ApiError::bad_request("单聊不能管理成员"));
    }
    Ok(found)
}

async fn member_rows(ctx: &AppContext, id: i32) -> ApiResult<Vec<im_members::Model>> {
    Ok(im_members::Entity::find()
        .filter(im_members::Column::ConversationId.eq(id))
        .order_by_asc(im_members::Column::Id)
        .all(&ctx.db)
        .await?)
}

async fn user_names(
    ctx: &AppContext,
    ids: impl IntoIterator<Item = i32>,
) -> ApiResult<HashMap<i32, users::Model>> {
    let ids: HashSet<i32> = ids.into_iter().collect();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    Ok(users::Entity::find()
        .filter(UserColumn::Id.is_in(ids))
        .all(&ctx.db)
        .await?
        .into_iter()
        .map(|user| (user.id, user))
        .collect())
}

fn message_record(
    message: im_messages::Model,
    people: &HashMap<i32, users::Model>,
) -> MessageRecord {
    MessageRecord {
        id: message.id,
        conversation_id: message.conversation_id,
        sender_id: message.sender_id,
        sender_name: message
            .sender_id
            .and_then(|id| people.get(&id))
            .map(display_name),
        kind: message.kind,
        content: message.content,
        file_name: message.file_name,
        mime_type: message.mime_type,
        size_bytes: message.size_bytes,
        created_at: message.created_at.to_rfc3339(),
    }
}

async fn conversation_record(
    ctx: &AppContext,
    conversation: &im_conversations::Model,
    me: &im_members::Model,
    members: &[im_members::Model],
    people: &HashMap<i32, users::Model>,
) -> ApiResult<ConversationRecord> {
    let peer_id = (conversation.kind == KIND_DIRECT)
        .then(|| {
            members
                .iter()
                .map(|member| member.user_id)
                .find(|id| *id != me.user_id)
        })
        .flatten();
    let name = if conversation.kind == KIND_DIRECT {
        peer_id
            .and_then(|id| people.get(&id))
            .map(display_name)
            .unwrap_or_default()
    } else {
        conversation.name.clone().unwrap_or_default()
    };
    let last_message = im_messages::Entity::find()
        .filter(im_messages::Column::ConversationId.eq(conversation.id))
        .order_by_desc(im_messages::Column::Id)
        .one(&ctx.db)
        .await?;
    let mut unread = im_messages::Entity::find()
        .filter(im_messages::Column::ConversationId.eq(conversation.id))
        .filter(
            Condition::any()
                .add(im_messages::Column::SenderId.is_null())
                .add(im_messages::Column::SenderId.ne(me.user_id)),
        );
    if let Some(read) = me.last_read_message_id {
        unread = unread.filter(im_messages::Column::Id.gt(read));
    }
    let last_people = match last_message.as_ref().and_then(|message| message.sender_id) {
        Some(id) if !people.contains_key(&id) => user_names(ctx, [id]).await?,
        _ => HashMap::new(),
    };
    Ok(ConversationRecord {
        id: conversation.id,
        kind: conversation.kind.clone(),
        name,
        owner_id: conversation.owner_id,
        peer_id,
        viewer_id: me.user_id,
        member_count: u64::try_from(members.len()).unwrap_or(u64::MAX),
        unread_count: unread.count(&ctx.db).await?,
        last_message: last_message.map(|message| {
            if last_people.is_empty() {
                message_record(message, people)
            } else {
                message_record(message, &last_people)
            }
        }),
        last_message_at: conversation.last_message_at.map(|at| at.to_rfc3339()),
        created_at: conversation.created_at.to_rfc3339(),
    })
}

async fn detail(
    ctx: &AppContext,
    conversation: &im_conversations::Model,
    me: &im_members::Model,
) -> ApiResult<ConversationDetailRecord> {
    let members = member_rows(ctx, conversation.id).await?;
    let people = user_names(ctx, members.iter().map(|member| member.user_id)).await?;
    let record = conversation_record(ctx, conversation, me, &members, &people).await?;
    let members = members
        .iter()
        .filter_map(|member| {
            people.get(&member.user_id).map(|user| MemberRecord {
                user_id: user.id,
                name: display_name(user),
                email: user.email.clone(),
                role: member.role.clone(),
            })
        })
        .collect();
    Ok(ConversationDetailRecord {
        conversation: record,
        members,
    })
}

/// Tells each user to reload a conversation (membership, name or read state changed).
fn notify_changed(user_ids: impl IntoIterator<Item = i32>, conversation_id: i32) {
    let data = serde_json::json!({ "type": "conversation", "conversation_id": conversation_id });
    let mut seen = HashSet::new();
    for user_id in user_ids {
        if seen.insert(user_id) {
            realtime::publish("im", realtime::Audience::User(user_id), data.clone());
        }
    }
}

struct NewMessage {
    kind: &'static str,
    content: String,
    file_name: Option<String>,
    file_path: Option<String>,
    mime_type: Option<String>,
    size_bytes: Option<i64>,
}

impl NewMessage {
    const fn text(kind: &'static str, content: String) -> Self {
        Self {
            kind,
            content,
            file_name: None,
            file_path: None,
            mime_type: None,
            size_bytes: None,
        }
    }
}

/// Stores a message, marks it read for the sender and pushes it to members.
async fn post_message(
    ctx: &AppContext,
    conversation: &im_conversations::Model,
    sender: &users::Model,
    new: NewMessage,
) -> ApiResult<MessageRecord> {
    let txn = ctx.db.begin().await?;
    let message = im_messages::ActiveModel {
        conversation_id: Set(conversation.id),
        sender_id: Set(Some(sender.id)),
        kind: Set(new.kind.to_string()),
        content: Set(new.content),
        file_name: Set(new.file_name),
        file_path: Set(new.file_path),
        mime_type: Set(new.mime_type),
        size_bytes: Set(new.size_bytes),
        ..Default::default()
    }
    .insert(&txn)
    .await?;
    let mut active = conversation.clone().into_active_model();
    active.last_message_at = Set(Some(message.created_at));
    active.update(&txn).await?;
    im_members::Entity::update_many()
        .col_expr(
            im_members::Column::LastReadMessageId,
            sea_orm::sea_query::Expr::value(message.id),
        )
        .filter(im_members::Column::ConversationId.eq(conversation.id))
        .filter(im_members::Column::UserId.eq(sender.id))
        .exec(&txn)
        .await?;
    txn.commit().await?;
    let members = member_rows(ctx, conversation.id).await?;
    let people = HashMap::from([(sender.id, sender.clone())]);
    let record = message_record(message, &people);
    for member in &members {
        realtime::publish(
            "im",
            realtime::Audience::User(member.user_id),
            serde_json::json!({
                "type": "message",
                "conversation_id": conversation.id,
                "message": record,
                "mine": member.user_id == sender.id,
            }),
        );
    }
    Ok(record)
}

/// Records a group event such as `created`, `invited`, `removed`, `left` or
/// `renamed` as a system message.
async fn post_event(
    ctx: &AppContext,
    conversation: &im_conversations::Model,
    actor: &users::Model,
    action: &str,
    targets: &[users::Model],
) -> ApiResult<MessageRecord> {
    let content = serde_json::json!({
        "action": action,
        "actor": display_name(actor),
        "targets": targets.iter().map(display_name).collect::<Vec<_>>(),
        "name": conversation.name,
    });
    post_message(
        ctx,
        conversation,
        actor,
        NewMessage::text(MESSAGE_SYSTEM, content.to_string()),
    )
    .await
}

async fn add_members(
    conn: &impl sea_orm::ConnectionTrait,
    conversation_id: i32,
    user_ids: Vec<(i32, &'static str)>,
) -> ApiResult<()> {
    for (user_id, role) in user_ids {
        im_members::ActiveModel {
            conversation_id: Set(conversation_id),
            user_id: Set(user_id),
            role: Set(role.to_string()),
            ..Default::default()
        }
        .insert(conn)
        .await?;
    }
    Ok(())
}

fn clean_name(name: &str) -> ApiResult<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("群名称不能为空"));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(ApiError::bad_request("群名称不能超过 60 个字符"));
    }
    Ok(name.to_string())
}

fn default_group_name(me: &users::Model, others: &[users::Model]) -> String {
    let names: Vec<String> = std::iter::once(me)
        .chain(others.iter())
        .take(4)
        .map(display_name)
        .collect();
    names.join("、").chars().take(MAX_NAME_CHARS).collect()
}

#[utoipa::path(
    get,
    path = "/api/admin/im/contacts",
    tag = "admin-im",
    security(("bearer_auth" = [])),
    params(ContactParams),
    responses((status = 200, body = ApiResponse<Vec<ContactRecord>>))
)]
#[debug_handler]
pub async fn contacts(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<ContactParams>,
) -> ApiResult<Response> {
    let me = authorize(&ctx, &auth, PERMISSION).await?;
    let mut query = users::Entity::find()
        .filter(contact_scope(&ctx, &me).await?)
        .order_by_asc(UserColumn::Name)
        .limit(MAX_CONTACTS);
    if let Some(keyword) = params
        .keyword
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        query = query.filter(
            Condition::any()
                .add(UserColumn::Name.contains(keyword))
                .add(UserColumn::Email.contains(keyword)),
        );
    }
    let items: Vec<ContactRecord> = query
        .all(&ctx.db)
        .await?
        .into_iter()
        .map(|user| ContactRecord {
            id: user.id,
            name: display_name(&user),
            email: user.email,
        })
        .collect();
    Ok(responses::ok(items))
}

#[utoipa::path(
    get,
    path = "/api/admin/im/conversations",
    tag = "admin-im",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<Vec<ConversationRecord>>))
)]
#[debug_handler]
pub async fn list(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    let me = authorize(&ctx, &auth, PERMISSION).await?;
    let mine = im_members::Entity::find()
        .filter(im_members::Column::UserId.eq(me.id))
        .all(&ctx.db)
        .await?;
    let ids: Vec<i32> = mine.iter().map(|member| member.conversation_id).collect();
    let conversations = im_conversations::Entity::find()
        .filter(im_conversations::Column::Id.is_in(ids.clone()))
        .all(&ctx.db)
        .await?;
    let members = im_members::Entity::find()
        .filter(im_members::Column::ConversationId.is_in(ids))
        .all(&ctx.db)
        .await?;
    let people = user_names(&ctx, members.iter().map(|member| member.user_id)).await?;
    let mut by_conversation: HashMap<i32, Vec<im_members::Model>> = HashMap::new();
    for member in members {
        by_conversation
            .entry(member.conversation_id)
            .or_default()
            .push(member);
    }
    let mut items = Vec::with_capacity(conversations.len());
    for conversation in &conversations {
        let Some(me_member) = mine
            .iter()
            .find(|member| member.conversation_id == conversation.id)
        else {
            continue;
        };
        let members = by_conversation
            .get(&conversation.id)
            .map_or(&[][..], Vec::as_slice);
        items.push(conversation_record(&ctx, conversation, me_member, members, &people).await?);
    }
    // Most recent activity first, like a phone chat list.
    items.sort_by(|a, b| {
        b.last_message_at
            .as_ref()
            .unwrap_or(&b.created_at)
            .cmp(a.last_message_at.as_ref().unwrap_or(&a.created_at))
    });
    Ok(responses::ok(items))
}

#[utoipa::path(
    get,
    path = "/api/admin/im/conversations/{id}",
    tag = "admin-im",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<ConversationDetailRecord>))
)]
#[debug_handler]
pub async fn get(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let me = authorize(&ctx, &auth, PERMISSION).await?;
    let (conversation, member) = membership(&ctx, id, me.id).await?;
    Ok(responses::ok(detail(&ctx, &conversation, &member).await?))
}

#[utoipa::path(
    post,
    path = "/api/admin/im/conversations/direct",
    tag = "admin-im",
    security(("bearer_auth" = [])),
    request_body = DirectParams,
    responses((status = 200, body = ApiResponse<ConversationDetailRecord>))
)]
#[debug_handler]
pub async fn open_direct(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<DirectParams>,
) -> ApiResult<Response> {
    let me = authorize(&ctx, &auth, PERMISSION).await?;
    if params.user_id == me.id {
        return Err(ApiError::bad_request("不能和自己单聊"));
    }
    assert_contacts(&ctx, &me, &[params.user_id]).await?;
    let key = format!(
        "{}:{}",
        me.id.min(params.user_id),
        me.id.max(params.user_id)
    );
    let existing = im_conversations::Entity::find()
        .filter(im_conversations::Column::DirectKey.eq(key.as_str()))
        .one(&ctx.db)
        .await?;
    let conversation = if let Some(conversation) = existing {
        // Someone who left (or was deleted and restored) rejoins quietly.
        let members = member_rows(&ctx, conversation.id).await?;
        let missing = [me.id, params.user_id]
            .into_iter()
            .filter(|id| !members.iter().any(|member| member.user_id == *id))
            .map(|id| (id, ROLE_MEMBER));
        add_members(&ctx.db, conversation.id, missing.collect()).await?;
        conversation
    } else {
        let txn = ctx.db.begin().await?;
        let conversation = im_conversations::ActiveModel {
            kind: Set(KIND_DIRECT.to_string()),
            direct_key: Set(Some(key)),
            ..Default::default()
        }
        .insert(&txn)
        .await?;
        add_members(
            &txn,
            conversation.id,
            vec![(me.id, ROLE_MEMBER), (params.user_id, ROLE_MEMBER)],
        )
        .await?;
        txn.commit().await?;
        conversation
    };
    let (conversation, member) = membership(&ctx, conversation.id, me.id).await?;
    Ok(responses::ok(detail(&ctx, &conversation, &member).await?))
}

#[utoipa::path(
    post,
    path = "/api/admin/im/conversations/group",
    tag = "admin-im",
    security(("bearer_auth" = [])),
    request_body = GroupParams,
    responses((status = 200, body = ApiResponse<ConversationDetailRecord>))
)]
#[debug_handler]
pub async fn create_group(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<GroupParams>,
) -> ApiResult<Response> {
    let me = authorize(&ctx, &auth, PERMISSION).await?;
    let others = assert_contacts(&ctx, &me, &params.member_ids).await?;
    if others.is_empty() {
        return Err(ApiError::bad_request("请至少选择一位群成员"));
    }
    if others.len() + 1 > MAX_GROUP_MEMBERS {
        return Err(ApiError::bad_request("群成员不能超过 500 人"));
    }
    let name = match params
        .name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
    {
        Some(name) => clean_name(name)?,
        None => default_group_name(&me, &others),
    };
    let txn = ctx.db.begin().await?;
    let conversation = im_conversations::ActiveModel {
        kind: Set(KIND_GROUP.to_string()),
        name: Set(Some(name)),
        owner_id: Set(Some(me.id)),
        ..Default::default()
    }
    .insert(&txn)
    .await?;
    add_members(
        &txn,
        conversation.id,
        std::iter::once((me.id, ROLE_OWNER))
            .chain(others.iter().map(|user| (user.id, ROLE_MEMBER)))
            .collect(),
    )
    .await?;
    txn.commit().await?;
    post_event(&ctx, &conversation, &me, "created", &others).await?;
    let (conversation, member) = membership(&ctx, conversation.id, me.id).await?;
    Ok(responses::ok(detail(&ctx, &conversation, &member).await?))
}

#[utoipa::path(
    put,
    path = "/api/admin/im/conversations/{id}",
    tag = "admin-im",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = RenameParams,
    responses((status = 200, body = ApiResponse<ConversationDetailRecord>))
)]
#[debug_handler]
pub async fn rename(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<RenameParams>,
) -> ApiResult<Response> {
    let me = authorize(&ctx, &auth, PERMISSION).await?;
    let (conversation, member) = group_membership(&ctx, id, me.id).await?;
    let name = clean_name(&params.name)?;
    let mut active = conversation.into_active_model();
    active.name = Set(Some(name));
    let conversation = active.update(&ctx.db).await?;
    post_event(&ctx, &conversation, &me, "renamed", &[]).await?;
    Ok(responses::ok(detail(&ctx, &conversation, &member).await?))
}

#[utoipa::path(
    post,
    path = "/api/admin/im/conversations/{id}/members",
    tag = "admin-im",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = AddMembersParams,
    responses((status = 200, body = ApiResponse<ConversationDetailRecord>))
)]
#[debug_handler]
pub async fn invite(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<AddMembersParams>,
) -> ApiResult<Response> {
    let me = authorize(&ctx, &auth, PERMISSION).await?;
    let (conversation, member) = group_membership(&ctx, id, me.id).await?;
    let members = member_rows(&ctx, id).await?;
    let ids: Vec<i32> = params
        .user_ids
        .into_iter()
        .filter(|id| !members.iter().any(|member| member.user_id == *id))
        .collect();
    let added = assert_contacts(&ctx, &me, &ids).await?;
    if added.is_empty() {
        return Err(ApiError::bad_request("所选用户都已在群里"));
    }
    if members.len() + added.len() > MAX_GROUP_MEMBERS {
        return Err(ApiError::bad_request("群成员不能超过 500 人"));
    }
    let rows = added.iter().map(|user| (user.id, ROLE_MEMBER)).collect();
    add_members(&ctx.db, id, rows).await?;
    post_event(&ctx, &conversation, &me, "invited", &added).await?;
    Ok(responses::ok(detail(&ctx, &conversation, &member).await?))
}

#[utoipa::path(
    delete,
    path = "/api/admin/im/conversations/{id}/members/{user_id}",
    tag = "admin-im",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path), ("user_id" = i32, Path)),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn remove_member(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path((id, user_id)): Path<(i32, i32)>,
) -> ApiResult<Response> {
    let me = authorize(&ctx, &auth, PERMISSION).await?;
    let (conversation, mine) = group_membership(&ctx, id, me.id).await?;
    let leaving = user_id == me.id;
    if !leaving && mine.role != ROLE_OWNER {
        return Err(ApiError::forbidden("只有群主可以移除成员"));
    }
    let members = member_rows(&ctx, id).await?;
    let target = members
        .iter()
        .find(|member| member.user_id == user_id)
        .ok_or_else(|| ApiError::not_found("该用户不在群里"))?;
    let target_user = users::Entity::find_by_id(user_id).one(&ctx.db).await?;
    im_members::Entity::delete_by_id(target.id)
        .exec(&ctx.db)
        .await?;
    let remaining: Vec<&im_members::Model> = members
        .iter()
        .filter(|member| member.user_id != user_id)
        .collect();
    let mut conversation = conversation;
    // The earliest remaining member takes over a group its owner left.
    if target.role == ROLE_OWNER {
        if let Some(next) = remaining.first() {
            let mut promoted = (*next).clone().into_active_model();
            promoted.role = Set(ROLE_OWNER.to_string());
            promoted.update(&ctx.db).await?;
            let mut active = conversation.into_active_model();
            active.owner_id = Set(Some(next.user_id));
            conversation = active.update(&ctx.db).await?;
        }
    }
    if remaining.is_empty() {
        im_conversations::Entity::delete_by_id(id)
            .exec(&ctx.db)
            .await?;
        remove_files(id);
    } else {
        let targets: Vec<users::Model> = target_user.into_iter().collect();
        let action = if leaving { "left" } else { "removed" };
        post_event(&ctx, &conversation, &me, action, &targets).await?;
    }
    notify_changed([user_id], id);
    Ok(responses::empty())
}

fn conversation_dir(id: i32) -> PathBuf {
    FsPath::new(STORAGE_DIR).join(id.to_string())
}

fn remove_files(id: i32) {
    let _ = std::fs::remove_dir_all(conversation_dir(id));
}

#[utoipa::path(
    delete,
    path = "/api/admin/im/conversations/{id}",
    tag = "admin-im",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn dissolve(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let me = authorize(&ctx, &auth, PERMISSION).await?;
    let (conversation, member) = group_membership(&ctx, id, me.id).await?;
    if member.role != ROLE_OWNER {
        return Err(ApiError::forbidden("只有群主可以解散群聊"));
    }
    let members = member_rows(&ctx, id).await?;
    // Members and messages cascade and are kept with the group; so are files.
    recycle_bin::delete_row(
        &ctx.db,
        recycle_bin::Deletion::new(
            "im_conversations",
            id,
            conversation.name.unwrap_or_default(),
            &me,
        ),
        "im_conversations",
        i64::from(id),
        &[conversation_dir(id)],
    )
    .await?;
    notify_changed(members.iter().map(|member| member.user_id), id);
    Ok(responses::empty())
}

#[utoipa::path(
    get,
    path = "/api/admin/im/conversations/{id}/messages",
    tag = "admin-im",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path), MessageListParams),
    responses((status = 200, body = ApiResponse<Vec<MessageRecord>>))
)]
#[debug_handler]
pub async fn messages(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Query(params): Query<MessageListParams>,
) -> ApiResult<Response> {
    let me = authorize(&ctx, &auth, PERMISSION).await?;
    membership(&ctx, id, me.id).await?;
    let limit = params.limit.unwrap_or(50).clamp(1, 200);
    let mut query = im_messages::Entity::find()
        .filter(im_messages::Column::ConversationId.eq(id))
        .limit(limit);
    if let Some(after) = params.after_id {
        query = query
            .filter(im_messages::Column::Id.gt(after))
            .order_by_asc(im_messages::Column::Id);
    } else {
        if let Some(before) = params.before_id {
            query = query.filter(im_messages::Column::Id.lt(before));
        }
        query = query.order_by_desc(im_messages::Column::Id);
    }
    let mut rows = query.all(&ctx.db).await?;
    rows.sort_by_key(|message| message.id);
    let people = user_names(&ctx, rows.iter().filter_map(|message| message.sender_id)).await?;
    let items: Vec<MessageRecord> = rows
        .into_iter()
        .map(|message| message_record(message, &people))
        .collect();
    Ok(responses::ok(items))
}

#[utoipa::path(
    post,
    path = "/api/admin/im/conversations/{id}/messages",
    tag = "admin-im",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = SendTextParams,
    responses((status = 200, body = ApiResponse<MessageRecord>))
)]
#[debug_handler]
pub async fn send_text(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<SendTextParams>,
) -> ApiResult<Response> {
    let me = authorize(&ctx, &auth, PERMISSION).await?;
    let (conversation, _) = membership(&ctx, id, me.id).await?;
    let content = params.content.trim_end();
    if content.trim().is_empty() {
        return Err(ApiError::bad_request("消息内容不能为空"));
    }
    if content.chars().count() > MAX_TEXT_CHARS {
        return Err(ApiError::bad_request("消息不能超过 5000 个字符"));
    }
    let record = post_message(
        &ctx,
        &conversation,
        &me,
        NewMessage::text(MESSAGE_TEXT, content.to_string()),
    )
    .await?;
    Ok(responses::ok(record))
}

#[utoipa::path(
    post,
    path = "/api/admin/im/conversations/{id}/files",
    tag = "admin-im",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body(content = String, content_type = "multipart/form-data"),
    responses((status = 200, body = ApiResponse<MessageRecord>))
)]
#[debug_handler]
pub async fn send_file(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    mut multipart: Multipart,
) -> ApiResult<Response> {
    let me = authorize(&ctx, &auth, PERMISSION).await?;
    let (conversation, _) = membership(&ctx, id, me.id).await?;
    let max_bytes = system_settings::upload_limit_bytes(&ctx.db).await?;
    let mut uploaded = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| ApiError::bad_request("invalid multipart payload"))?
    {
        if field.name() == Some("file") {
            let name = sanitize_filename(field.file_name().unwrap_or("file.bin"));
            let declared = field.content_type().map(str::to_string);
            let bytes = read_file_field(field, max_bytes).await?;
            uploaded = Some((name, declared, bytes));
        }
    }
    let (name, declared, bytes) =
        uploaded.ok_or_else(|| ApiError::bad_request("file is required"))?;
    // The extension decides how the file is served; the browser's claim
    // only fills in types the server does not know.
    let mime = mime_type(&name)
        .map(str::to_string)
        .or(declared)
        .unwrap_or_else(|| "application/octet-stream".to_string());
    let kind = if mime.starts_with("image/") && !mime.starts_with("image/svg") {
        MESSAGE_IMAGE
    } else {
        MESSAGE_FILE
    };
    let dir = conversation_dir(id);
    std::fs::create_dir_all(&dir).map_err(|_| ApiError::internal("failed to save file"))?;
    let stored = dir.join(format!("{}-{name}", Uuid::new_v4().simple()));
    std::fs::write(&stored, &bytes).map_err(|_| ApiError::internal("failed to save file"))?;
    let record = post_message(
        &ctx,
        &conversation,
        &me,
        NewMessage {
            kind,
            content: String::new(),
            file_name: Some(name),
            file_path: Some(stored.to_string_lossy().replace('\\', "/")),
            mime_type: Some(mime),
            size_bytes: Some(i64::try_from(bytes.len()).unwrap_or(i64::MAX)),
        },
    )
    .await?;
    Ok(responses::ok(record))
}

#[utoipa::path(
    get,
    path = "/api/admin/im/messages/{id}/file",
    tag = "admin-im",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path), FileParams),
    responses((status = 200, description = "File content"))
)]
#[debug_handler]
pub async fn file(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Query(params): Query<FileParams>,
    request_headers: HeaderMap,
) -> ApiResult<Response> {
    let me = authorize(&ctx, &auth, PERMISSION).await?;
    let message = im_messages::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::not_found("文件不存在"))?;
    membership(&ctx, message.conversation_id, me.id).await?;
    let (Some(path), Some(name)) = (message.file_path, message.file_name) else {
        return Err(ApiError::not_found("文件不存在"));
    };
    let path = PathBuf::from(path);
    let total_size = std::fs::metadata(&path)
        .ok()
        .filter(std::fs::Metadata::is_file)
        .and_then(|metadata| usize::try_from(metadata.len()).ok())
        .ok_or_else(|| ApiError::not_found("文件不存在"))?;
    let range = file_stream::requested_range(&request_headers, total_size)?;
    let content = storage::stream_local_path(path, range.clone()).await?;
    let disposition = if params.download.unwrap_or(false) || message.kind != MESSAGE_IMAGE {
        FileDisposition::Attachment
    } else {
        FileDisposition::Inline
    };
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        message
            .mime_type
            .as_deref()
            .and_then(|mime| mime.parse().ok())
            .unwrap_or_else(|| HeaderValue::from_static("application/octet-stream")),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&content_disposition(disposition, &name))
            .map_err(|_| ApiError::internal("failed to build file response"))?,
    );
    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=86400"),
    );
    file_stream::streaming_response(content, headers, range.is_some())
}

#[utoipa::path(
    post,
    path = "/api/admin/im/conversations/{id}/read",
    tag = "admin-im",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = ReadParams,
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn mark_read(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<ReadParams>,
) -> ApiResult<Response> {
    let me = authorize(&ctx, &auth, PERMISSION).await?;
    let (_, member) = membership(&ctx, id, me.id).await?;
    let latest = im_messages::Entity::find()
        .filter(im_messages::Column::ConversationId.eq(id))
        .order_by_desc(im_messages::Column::Id)
        .one(&ctx.db)
        .await?
        .map(|message| message.id);
    let target = match (params.message_id, latest) {
        (Some(wanted), Some(latest)) => Some(wanted.min(latest)),
        (_, latest) => latest,
    };
    if target > member.last_read_message_id {
        let mut active = member.into_active_model();
        active.last_read_message_id = Set(target);
        active.update(&ctx.db).await?;
        // Other open tabs of the same user clear their badge too.
        notify_changed([me.id], id);
    }
    Ok(responses::empty())
}
