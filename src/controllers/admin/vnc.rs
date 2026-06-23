#![allow(clippy::missing_errors_doc)]

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};

use axum::{
    extract::{ws::Message, ws::WebSocket, ws::WebSocketUpgrade, Path},
    response::IntoResponse,
};
use chrono::{DateTime, Duration, Utc};
use futures_util::{SinkExt, StreamExt};
use loco_rs::prelude::*;
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use uuid::Uuid;

use crate::{
    controllers::admin::authorize,
    errors::{ApiError, ApiResult},
    models::{admin_logs, system_settings},
    responses::{self, ApiResponse},
};

const TARGETS_SETTING_KEY: &str = "vnc_manager.targets";
const LOCAL_TARGET_KEY: &str = "local-vnc";
const LOCAL_VNC_HOST: &str = "127.0.0.1";
const DEFAULT_VNC_PORT: u16 = 5900;
const TICKET_TTL_SECONDS: i64 = 60;

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
    pub password: Option<String>,
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
    password: Option<String>,
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
    authorize(&ctx, &auth, "system:vnc:list").await?;
    Ok(responses::ok(target_records(&ctx).await?))
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
    let record = create_attach_ticket(&session, user.id, operator.clone())?;
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

async fn target_records(ctx: &AppContext) -> ApiResult<Vec<VncTargetRecord>> {
    Ok(all_targets(ctx)
        .await?
        .into_iter()
        .map(|target| target.record())
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
) -> ApiResult<VncTicketRecord> {
    let ticket = Uuid::new_v4().to_string();
    let expires_at = Utc::now() + Duration::seconds(TICKET_TTL_SECONDS);
    let password = session.target_kind.password();
    let vnc_ticket = VncTicket {
        session_id: session.id.clone(),
        user_id,
        operator,
        password: password.clone(),
        expires_at,
    };
    ticket_store()
        .lock()
        .map_err(|_| ApiError::internal("failed to lock vnc ticket store"))?
        .insert(ticket.clone(), vnc_ticket);
    Ok(VncTicketRecord {
        ticket,
        expires_at: expires_at.to_rfc3339(),
        password,
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
        has_password = ticket.password.is_some(),
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
