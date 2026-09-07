#![allow(clippy::missing_errors_doc)]

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, OnceLock},
};

use axum::{
    extract::{ws::Message, ws::WebSocket, ws::WebSocketUpgrade, Path},
    response::IntoResponse,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration, Utc};
use futures_util::{SinkExt, StreamExt};
use loco_rs::prelude::*;
use ring::{
    aead::{self, Aad, LessSafeKey, Nonce, UnboundKey},
    rand::{SecureRandom, SystemRandom},
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, IntoActiveModel, QueryFilter,
    Set,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use uuid::Uuid;

use crate::{
    controllers::admin::authorize,
    errors::{ApiError, ApiResult},
    models::{_entities::vnc_credentials, admin_logs, system_settings},
    responses::{self, ApiResponse},
};

const TARGETS_SETTING_KEY: &str = "vnc_manager.targets";
const LOCAL_TARGET_KEY: &str = "local-vnc";
const LOCAL_VNC_HOST: &str = "127.0.0.1";
const DEFAULT_VNC_PORT: u16 = 5900;
const TICKET_TTL_SECONDS: i64 = 60;
const CREDENTIAL_PAYLOAD_PREFIX: &str = "v1:";
const CREDENTIAL_NONCE_LENGTH: usize = 12;
const CREDENTIAL_KEY_CONTEXT: &[u8] = b"gpt-images:vnc-credentials:v1:";

static VNC_TICKETS: OnceLock<Mutex<HashMap<String, VncTicket>>> = OnceLock::new();
static VNC_SESSIONS: OnceLock<Mutex<HashMap<String, Arc<ManagedVncSession>>>> = OnceLock::new();

#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
pub struct VncTargetRecord {
    pub key: String,
    pub name: String,
    pub target_type: String,
    pub host: String,
    pub port: u16,
    pub enabled: bool,
    pub requires_password: bool,
    pub credentials_saved: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
pub struct VncTargetConfig {
    pub key: String,
    pub name: String,
    pub host: String,
    pub port: Option<u16>,
    pub password: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CreateVncSessionParams {
    pub target_key: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct VncTicketRecord {
    pub ticket: String,
    pub expires_at: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub target: Option<String>,
    pub credentials_saved: bool,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveVncCredentialsParams {
    pub target_key: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub target: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct VncCredentialQuery {
    pub target_key: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct VncCredentialStatus {
    pub target_key: String,
    pub saved: bool,
}

#[derive(Clone, Default, Deserialize, Serialize)]
struct SavedVncCredentials {
    username: Option<String>,
    password: Option<String>,
    target: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
pub struct VncSessionRecord {
    pub id: String,
    pub target_key: String,
    pub target_name: String,
    pub target_type: String,
    pub host: String,
    pub port: u16,
    pub status: String,
    pub requires_password: bool,
    pub created_by: i32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
struct VncTicket {
    session_id: String,
    user_id: i32,
    operator: String,
    has_credentials: bool,
    expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
enum VncTarget {
    Local(VncTargetRecord),
    Remote(VncTargetConfig),
}

struct ManagedVncSession {
    id: String,
    target: VncTargetRecord,
    target_kind: VncTarget,
    user_id: i32,
    status: Mutex<String>,
    created_at: DateTime<Utc>,
    updated_at: Mutex<DateTime<Utc>>,
}

#[utoipa::path(
    get,
    path = "/api/admin/vnc/targets",
    tag = "admin-vnc",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<Vec<VncTargetRecord>>))
)]
#[debug_handler]
pub async fn targets(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:vnc:list").await?;
    Ok(responses::ok(target_records(&ctx, user.id).await?))
}

#[utoipa::path(
    put,
    path = "/api/admin/vnc/credentials",
    tag = "admin-vnc",
    security(("bearer_auth" = [])),
    request_body = SaveVncCredentialsParams,
    responses((status = 200, body = ApiResponse<VncCredentialStatus>))
)]
#[debug_handler]
pub async fn save_credentials(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SaveVncCredentialsParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:vnc:connect").await?;
    let target = find_target(&ctx, &params.target_key).await?;
    let target_key = target.record().key;
    let credentials = normalize_credentials(params)?;
    let jwt_config = ctx.config.get_jwt_config()?;
    let encrypted_payload =
        encrypt_credentials(&jwt_config.secret, user.id, &target_key, &credentials)?;
    upsert_credentials(&ctx.db, user.id, &target_key, encrypted_payload).await?;
    record_vnc_log(
        &ctx.db,
        user.id,
        Some(user.name),
        "save_credentials",
        format!("保存 VNC 登录凭据：{}", target.record().name),
        Some(200),
        None,
    )
    .await;
    Ok(responses::ok(VncCredentialStatus {
        target_key,
        saved: true,
    }))
}

#[utoipa::path(
    delete,
    path = "/api/admin/vnc/credentials",
    tag = "admin-vnc",
    security(("bearer_auth" = [])),
    params(VncCredentialQuery),
    responses((status = 200, body = ApiResponse<VncCredentialStatus>))
)]
#[debug_handler]
pub async fn delete_credentials(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<VncCredentialQuery>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:vnc:connect").await?;
    let target = find_target(&ctx, &params.target_key).await?;
    let target_record = target.record();
    vnc_credentials::Entity::delete_many()
        .filter(vnc_credentials::Column::UserId.eq(user.id))
        .filter(vnc_credentials::Column::TargetKey.eq(&target_record.key))
        .exec(&ctx.db)
        .await?;
    record_vnc_log(
        &ctx.db,
        user.id,
        Some(user.name),
        "delete_credentials",
        format!("清除 VNC 登录凭据：{}", target_record.name),
        Some(200),
        None,
    )
    .await;
    Ok(responses::ok(VncCredentialStatus {
        target_key: target_record.key,
        saved: false,
    }))
}

#[utoipa::path(
    get,
    path = "/api/admin/vnc/sessions",
    tag = "admin-vnc",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<Vec<VncSessionRecord>>))
)]
#[debug_handler]
pub async fn sessions(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:vnc:list").await?;
    let sessions = session_store()
        .lock()
        .map_err(|_| ApiError::internal("failed to lock vnc session store"))?
        .values()
        .map(|session| session.record())
        .collect::<Vec<_>>();
    Ok(responses::ok(sessions))
}

#[utoipa::path(
    post,
    path = "/api/admin/vnc/sessions",
    tag = "admin-vnc",
    security(("bearer_auth" = [])),
    request_body = CreateVncSessionParams,
    responses((status = 200, body = ApiResponse<VncSessionRecord>))
)]
#[debug_handler]
pub async fn create_session(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<CreateVncSessionParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:vnc:connect").await?;
    let target = find_target(&ctx, &params.target_key).await?;
    let operator = Some(user.name.clone())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| user.email.clone());
    let session = create_managed_session(&target, user.id)?;
    record_vnc_log(
        &ctx.db,
        user.id,
        Some(operator),
        "connect",
        format!("创建 VNC 共享会话：{}", session.target.name),
        Some(101),
        None,
    )
    .await;
    Ok(responses::ok(session.record()))
}

#[utoipa::path(
    post,
    path = "/api/admin/vnc/sessions/{id}/tickets",
    tag = "admin-vnc",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<VncTicketRecord>))
)]
#[debug_handler]
pub async fn create_session_ticket(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:vnc:connect").await?;
    let session = find_session(&id)?;
    let operator = Some(user.name.clone())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| user.email.clone());
    let jwt_config = ctx.config.get_jwt_config()?;
    let saved_credentials =
        load_credentials(&ctx.db, &jwt_config.secret, user.id, &session.target.key).await?;
    let credentials_saved = saved_credentials.is_some();
    let mut credentials = saved_credentials.unwrap_or_default();
    if credentials.password.is_none() {
        credentials.password = session.target_kind.password();
    }
    let record = create_attach_ticket(
        &session,
        user.id,
        operator.clone(),
        credentials,
        credentials_saved,
    )?;
    record_vnc_log(
        &ctx.db,
        user.id,
        Some(operator),
        "attach_ticket",
        format!("创建 VNC 会话附加票据：{}", session.target.name),
        Some(200),
        None,
    )
    .await;
    Ok(responses::ok(record))
}

#[utoipa::path(
    delete,
    path = "/api/admin/vnc/sessions/{id}",
    tag = "admin-vnc",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<VncSessionRecord>))
)]
#[debug_handler]
pub async fn close_session(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:vnc:connect").await?;
    let session = remove_session(&id)?;
    session.mark_closed();
    record_vnc_log(
        &ctx.db,
        user.id,
        Some(user.name.clone()),
        "close",
        format!("关闭 VNC 共享会话：{}", session.target.name),
        Some(200),
        None,
    )
    .await;
    Ok(responses::ok(session.record()))
}

#[debug_handler]
pub async fn session_ws(
    Path((id, ticket)): Path<(String, String)>,
    ws: WebSocketUpgrade,
) -> ApiResult<Response> {
    let ticket = consume_ticket(&ticket)?;
    if ticket.session_id != id {
        return Err(ApiError::unauthorized("invalid vnc session ticket"));
    }
    let session = find_session(&id)?;
    Ok(ws
        .on_upgrade(move |socket| handle_vnc_socket(socket, session, ticket))
        .into_response())
}

async fn target_records(ctx: &AppContext, user_id: i32) -> ApiResult<Vec<VncTargetRecord>> {
    let saved_target_keys = vnc_credentials::Entity::find()
        .filter(vnc_credentials::Column::UserId.eq(user_id))
        .all(&ctx.db)
        .await?
        .into_iter()
        .map(|credential| credential.target_key)
        .collect::<HashSet<_>>();
    Ok(all_targets(ctx)
        .await?
        .into_iter()
        .map(|target| {
            let mut record = target.record();
            record.credentials_saved = saved_target_keys.contains(&record.key);
            record
        })
        .collect())
}

async fn find_target(ctx: &AppContext, key: &str) -> ApiResult<VncTarget> {
    let normalized = key.trim();
    if normalized.is_empty() {
        return Err(ApiError::bad_request("vnc target is required"));
    }
    all_targets(ctx)
        .await?
        .into_iter()
        .find(|target| target.record().key == normalized)
        .ok_or_else(|| ApiError::bad_request("vnc target not found"))
}

async fn all_targets(ctx: &AppContext) -> ApiResult<Vec<VncTarget>> {
    let mut targets = vec![VncTarget::Local(local_target())];
    let value = system_settings::string_value(&ctx.db, TARGETS_SETTING_KEY, "[]").await?;
    let remote_targets = serde_json::from_str::<Vec<VncTargetConfig>>(&value)
        .map_err(|_| ApiError::bad_request("invalid vnc manager targets setting"))?;
    for target in remote_targets.into_iter().filter(|target| target.enabled) {
        targets.push(VncTarget::Remote(normalize_remote_target(target)?));
    }
    Ok(targets)
}

fn local_target() -> VncTargetRecord {
    VncTargetRecord {
        key: LOCAL_TARGET_KEY.to_string(),
        name: "本机 VNC".to_string(),
        target_type: "local".to_string(),
        host: LOCAL_VNC_HOST.to_string(),
        port: DEFAULT_VNC_PORT,
        enabled: true,
        requires_password: false,
        credentials_saved: false,
    }
}

fn normalize_remote_target(mut target: VncTargetConfig) -> ApiResult<VncTargetConfig> {
    target.key = target.key.trim().to_string();
    target.name = target.name.trim().to_string();
    target.host = target.host.trim().to_string();
    target.password = target
        .password
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if target.key.is_empty()
        || target.name.is_empty()
        || target.host.is_empty()
        || contains_control_or_whitespace(&target.key)
        || contains_control_or_whitespace(&target.host)
        || target.key.starts_with('-')
        || target.host.starts_with('-')
    {
        return Err(ApiError::bad_request("invalid vnc target configuration"));
    }
    if target.port.is_some_and(|port| port == 0) {
        return Err(ApiError::bad_request("invalid vnc target port"));
    }
    if target
        .password
        .as_deref()
        .is_some_and(contains_control_char)
    {
        return Err(ApiError::bad_request("invalid vnc password"));
    }
    Ok(target)
}

fn contains_control_or_whitespace(value: &str) -> bool {
    value
        .chars()
        .any(|char| char.is_control() || char.is_whitespace())
}

fn contains_control_char(value: &str) -> bool {
    value.chars().any(char::is_control)
}

impl VncTarget {
    fn record(&self) -> VncTargetRecord {
        match self {
            Self::Local(record) => record.clone(),
            Self::Remote(target) => VncTargetRecord {
                key: target.key.clone(),
                name: target.name.clone(),
                target_type: "vnc".to_string(),
                host: target.host.clone(),
                port: target.port.unwrap_or(DEFAULT_VNC_PORT),
                enabled: target.enabled,
                requires_password: target.password.is_some(),
                credentials_saved: false,
            },
        }
    }

    fn password(&self) -> Option<String> {
        match self {
            Self::Local(_) => None,
            Self::Remote(target) => target.password.clone(),
        }
    }
}

impl ManagedVncSession {
    fn record(&self) -> VncSessionRecord {
        VncSessionRecord {
            id: self.id.clone(),
            target_key: self.target.key.clone(),
            target_name: self.target.name.clone(),
            target_type: self.target.target_type.clone(),
            host: self.target.host.clone(),
            port: self.target.port,
            status: self.status(),
            requires_password: self.target.requires_password,
            created_by: self.user_id,
            created_at: self.created_at.to_rfc3339(),
            updated_at: self
                .updated_at
                .lock()
                .map_or_else(|_| self.created_at, |updated_at| *updated_at)
                .to_rfc3339(),
        }
    }

    fn status(&self) -> String {
        self.status
            .lock()
            .map_or_else(|_| "error".to_string(), |status| status.clone())
    }

    fn address(&self) -> String {
        format!("{}:{}", self.target.host, self.target.port)
    }

    fn mark_status(&self, next_status: &str) {
        if let Ok(mut status) = self.status.lock() {
            *status = next_status.to_string();
        }
        self.touch();
    }

    fn mark_closed(&self) {
        self.mark_status("closed");
    }

    fn touch(&self) {
        if let Ok(mut updated_at) = self.updated_at.lock() {
            *updated_at = Utc::now();
        }
    }
}

fn ticket_store() -> &'static Mutex<HashMap<String, VncTicket>> {
    VNC_TICKETS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn session_store() -> &'static Mutex<HashMap<String, Arc<ManagedVncSession>>> {
    VNC_SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn create_managed_session(target: &VncTarget, user_id: i32) -> ApiResult<Arc<ManagedVncSession>> {
    let session = Arc::new(ManagedVncSession {
        id: Uuid::new_v4().to_string(),
        target: target.record(),
        target_kind: target.clone(),
        user_id,
        status: Mutex::new("ready".to_string()),
        created_at: Utc::now(),
        updated_at: Mutex::new(Utc::now()),
    });
    session_store()
        .lock()
        .map_err(|_| ApiError::internal("failed to lock vnc session store"))?
        .insert(session.id.clone(), Arc::clone(&session));
    Ok(session)
}

fn create_attach_ticket(
    session: &ManagedVncSession,
    user_id: i32,
    operator: String,
    credentials: SavedVncCredentials,
    credentials_saved: bool,
) -> ApiResult<VncTicketRecord> {
    let ticket = Uuid::new_v4().to_string();
    let expires_at = Utc::now() + Duration::seconds(TICKET_TTL_SECONDS);
    let has_credentials = credentials.username.is_some()
        || credentials.password.is_some()
        || credentials.target.is_some();
    let vnc_ticket = VncTicket {
        session_id: session.id.clone(),
        user_id,
        operator,
        has_credentials,
        expires_at,
    };
    ticket_store()
        .lock()
        .map_err(|_| ApiError::internal("failed to lock vnc ticket store"))?
        .insert(ticket.clone(), vnc_ticket);
    Ok(VncTicketRecord {
        ticket,
        expires_at: expires_at.to_rfc3339(),
        username: credentials.username,
        password: credentials.password,
        target: credentials.target,
        credentials_saved,
    })
}

fn consume_ticket(ticket: &str) -> ApiResult<VncTicket> {
    let now = Utc::now();
    let mut store = ticket_store()
        .lock()
        .map_err(|_| ApiError::internal("failed to lock vnc ticket store"))?;
    store.retain(|_, ticket| ticket.expires_at > now);
    let ticket = store
        .remove(ticket)
        .ok_or_else(|| ApiError::unauthorized("invalid or expired vnc ticket"))?;
    drop(store);
    if ticket.expires_at <= now {
        return Err(ApiError::unauthorized("invalid or expired vnc ticket"));
    }
    Ok(ticket)
}

fn find_session(id: &str) -> ApiResult<Arc<ManagedVncSession>> {
    session_store()
        .lock()
        .map_err(|_| ApiError::internal("failed to lock vnc session store"))?
        .get(id)
        .cloned()
        .ok_or_else(|| ApiError::bad_request("vnc session not found"))
}

fn remove_session(id: &str) -> ApiResult<Arc<ManagedVncSession>> {
    session_store()
        .lock()
        .map_err(|_| ApiError::internal("failed to lock vnc session store"))?
        .remove(id)
        .ok_or_else(|| ApiError::bad_request("vnc session not found"))
}

async fn handle_vnc_socket(socket: WebSocket, session: Arc<ManagedVncSession>, ticket: VncTicket) {
    match TcpStream::connect(session.address()).await {
        Ok(stream) => Box::pin(proxy_vnc_socket(socket, stream, &session)).await,
        Err(error) => {
            session.mark_status("error");
            tracing::warn!(
                session_id = session.id,
                error = error.to_string(),
                "failed to connect vnc target"
            );
        }
    }

    tracing::debug!(
        session_id = session.id,
        user_id = ticket.user_id,
        operator = ticket.operator,
        has_credentials = ticket.has_credentials,
        "vnc websocket detached"
    );
}

async fn proxy_vnc_socket(socket: WebSocket, stream: TcpStream, session: &ManagedVncSession) {
    session.mark_status("connected");
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (mut tcp_reader, mut tcp_writer) = stream.into_split();
    let mut buffer = [0_u8; 16_384];

    loop {
        tokio::select! {
            message = ws_receiver.next() => {
                match message {
                    Some(Ok(Message::Binary(bytes))) => {
                        if tcp_writer.write_all(&bytes).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Text(text))) => {
                        if tcp_writer.write_all(text.as_bytes()).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                }
            }
            read_result = tcp_reader.read(&mut buffer) => {
                match read_result {
                    Ok(0) | Err(_) => break,
                    Ok(size) => {
                        if ws_sender
                            .send(Message::Binary(Vec::from(&buffer[..size]).into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        }
    }

    session.mark_status("ready");
}

fn normalize_credentials(params: SaveVncCredentialsParams) -> ApiResult<SavedVncCredentials> {
    let credentials = SavedVncCredentials {
        username: normalize_credential_text(params.username, "username", 512, true)?,
        password: normalize_credential_text(params.password, "password", 4096, false)?,
        target: normalize_credential_text(params.target, "target", 512, true)?,
    };
    if credentials.username.is_none()
        && credentials.password.is_none()
        && credentials.target.is_none()
    {
        return Err(ApiError::bad_request(
            "at least one vnc credential field is required",
        ));
    }
    Ok(credentials)
}

fn normalize_credential_text(
    value: Option<String>,
    field: &str,
    max_length: usize,
    trim: bool,
) -> ApiResult<Option<String>> {
    let Some(mut value) = value else {
        return Ok(None);
    };
    if trim {
        value = value.trim().to_string();
    }
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() > max_length || contains_control_char(&value) {
        return Err(ApiError::bad_request(format!(
            "invalid vnc credential field: {field}"
        )));
    }
    Ok(Some(value))
}

async fn upsert_credentials(
    db: &DatabaseConnection,
    user_id: i32,
    target_key: &str,
    encrypted_payload: String,
) -> ApiResult<()> {
    let existing = vnc_credentials::Entity::find()
        .filter(vnc_credentials::Column::UserId.eq(user_id))
        .filter(vnc_credentials::Column::TargetKey.eq(target_key))
        .one(db)
        .await?;
    if let Some(existing) = existing {
        let mut active = existing.into_active_model();
        active.encrypted_payload = Set(encrypted_payload);
        active.updated_at = Set(Utc::now().into());
        active.update(db).await?;
    } else {
        vnc_credentials::ActiveModel {
            user_id: Set(user_id),
            target_key: Set(target_key.to_string()),
            encrypted_payload: Set(encrypted_payload),
            ..Default::default()
        }
        .insert(db)
        .await?;
    }
    Ok(())
}

async fn load_credentials(
    db: &DatabaseConnection,
    secret: &str,
    user_id: i32,
    target_key: &str,
) -> ApiResult<Option<SavedVncCredentials>> {
    let Some(record) = vnc_credentials::Entity::find()
        .filter(vnc_credentials::Column::UserId.eq(user_id))
        .filter(vnc_credentials::Column::TargetKey.eq(target_key))
        .one(db)
        .await?
    else {
        return Ok(None);
    };

    match decrypt_credentials(secret, user_id, target_key, &record.encrypted_payload) {
        Ok(credentials) => Ok(Some(credentials)),
        Err(error) => {
            tracing::warn!(
                user_id,
                target_key,
                error = error.message(),
                "discarding unreadable vnc credentials"
            );
            vnc_credentials::Entity::delete_by_id(record.id)
                .exec(db)
                .await?;
            Ok(None)
        }
    }
}

fn encrypt_credentials(
    secret: &str,
    user_id: i32,
    target_key: &str,
    credentials: &SavedVncCredentials,
) -> ApiResult<String> {
    let key = credential_key(secret)?;
    let mut nonce_bytes = [0_u8; CREDENTIAL_NONCE_LENGTH];
    SystemRandom::new()
        .fill(&mut nonce_bytes)
        .map_err(|_| ApiError::internal("failed to create vnc credential nonce"))?;
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);
    let mut payload = serde_json::to_vec(credentials)
        .map_err(|_| ApiError::internal("failed to serialize vnc credentials"))?;
    key.seal_in_place_append_tag(
        nonce,
        Aad::from(credential_aad(user_id, target_key).as_bytes()),
        &mut payload,
    )
    .map_err(|_| ApiError::internal("failed to encrypt vnc credentials"))?;

    let mut encoded = Vec::with_capacity(CREDENTIAL_NONCE_LENGTH + payload.len());
    encoded.extend_from_slice(&nonce_bytes);
    encoded.extend_from_slice(&payload);
    Ok(format!(
        "{CREDENTIAL_PAYLOAD_PREFIX}{}",
        URL_SAFE_NO_PAD.encode(encoded)
    ))
}

fn decrypt_credentials(
    secret: &str,
    user_id: i32,
    target_key: &str,
    encrypted_payload: &str,
) -> ApiResult<SavedVncCredentials> {
    let encoded = encrypted_payload
        .strip_prefix(CREDENTIAL_PAYLOAD_PREFIX)
        .ok_or_else(|| ApiError::internal("unsupported vnc credential payload"))?;
    let mut payload = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| ApiError::internal("invalid vnc credential payload"))?;
    if payload.len() <= CREDENTIAL_NONCE_LENGTH {
        return Err(ApiError::internal("invalid vnc credential payload"));
    }
    let mut nonce_bytes = [0_u8; CREDENTIAL_NONCE_LENGTH];
    nonce_bytes.copy_from_slice(&payload[..CREDENTIAL_NONCE_LENGTH]);
    payload.drain(..CREDENTIAL_NONCE_LENGTH);

    let key = credential_key(secret)?;
    let decrypted = key
        .open_in_place(
            Nonce::assume_unique_for_key(nonce_bytes),
            Aad::from(credential_aad(user_id, target_key).as_bytes()),
            &mut payload,
        )
        .map_err(|_| ApiError::internal("failed to decrypt vnc credentials"))?;
    serde_json::from_slice(decrypted)
        .map_err(|_| ApiError::internal("invalid decrypted vnc credentials"))
}

fn credential_key(secret: &str) -> ApiResult<LessSafeKey> {
    let mut hasher = Sha256::new();
    hasher.update(CREDENTIAL_KEY_CONTEXT);
    hasher.update(secret.as_bytes());
    let digest = hasher.finalize();
    let key = UnboundKey::new(&aead::AES_256_GCM, &digest)
        .map_err(|_| ApiError::internal("failed to initialize vnc credential encryption"))?;
    Ok(LessSafeKey::new(key))
}

fn credential_aad(user_id: i32, target_key: &str) -> String {
    format!("{user_id}:{target_key}")
}

async fn record_vnc_log(
    db: &DatabaseConnection,
    user_id: i32,
    operator: Option<String>,
    action: &'static str,
    message: String,
    status: Option<i32>,
    error_message: Option<String>,
) {
    admin_logs::record(
        db,
        admin_logs::LogInput {
            log_type: "operation",
            level: if error_message.is_some() {
                "error"
            } else {
                "info"
            },
            module: "vnc",
            action,
            message: &message,
            user_id: Some(user_id),
            operator,
            method: None,
            path: None,
            status,
            error_message,
        },
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::{decrypt_credentials, encrypt_credentials, SavedVncCredentials};

    #[test]
    fn vnc_credentials_round_trip_without_plaintext_storage() {
        let credentials = SavedVncCredentials {
            username: Some("pikju".to_string()),
            password: Some("vnc-secret-123".to_string()),
            target: Some("local-vnc".to_string()),
        };
        let encrypted = encrypt_credentials("jwt-secret", 7, "local-vnc", &credentials).unwrap();
        assert!(!encrypted.contains("pikju"));
        assert!(!encrypted.contains("vnc-secret-123"));

        let decrypted = decrypt_credentials("jwt-secret", 7, "local-vnc", &encrypted).unwrap();
        assert_eq!(decrypted.username.as_deref(), Some("pikju"));
        assert_eq!(decrypted.password.as_deref(), Some("vnc-secret-123"));
        assert_eq!(decrypted.target.as_deref(), Some("local-vnc"));
    }

    #[test]
    fn vnc_credentials_are_bound_to_user_and_target() {
        let credentials = SavedVncCredentials {
            password: Some("vnc-secret-123".to_string()),
            ..Default::default()
        };
        let encrypted = encrypt_credentials("jwt-secret", 7, "local-vnc", &credentials).unwrap();
        assert!(decrypt_credentials("jwt-secret", 8, "local-vnc", &encrypted).is_err());
        assert!(decrypt_credentials("jwt-secret", 7, "remote-vnc", &encrypted).is_err());
        assert!(decrypt_credentials("different-secret", 7, "local-vnc", &encrypted).is_err());
    }
}
