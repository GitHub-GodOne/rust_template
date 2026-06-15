#![allow(clippy::missing_errors_doc)]

use std::{
    collections::HashMap,
    env,
    io::{Read, Write},
    sync::{Mutex, OnceLock},
    thread,
};

use axum::{
    extract::{ws::Message, ws::WebSocket, ws::WebSocketUpgrade, Path},
    response::IntoResponse,
};
use chrono::{DateTime, Duration, Utc};
use loco_rs::prelude::*;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{
    controllers::admin::authorize,
    errors::{ApiError, ApiResult},
    models::{admin_logs, system_settings},
    responses::{self, ApiResponse},
};

const TARGETS_SETTING_KEY: &str = "ssh_manager.targets";
const LOCAL_TARGET_KEY: &str = "local-shell";
const TICKET_TTL_SECONDS: i64 = 60;

static SSH_TICKETS: OnceLock<Mutex<HashMap<String, SshTicket>>> = OnceLock::new();

#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SshTargetRecord {
    pub key: String,
    pub name: String,
    pub target_type: String,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SshTargetConfig {
    pub key: String,
    pub name: String,
    pub host: String,
    pub port: Option<u16>,
    pub username: String,
    pub key_path: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CreateSshTicketParams {
    pub target_key: String,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SshTicketRecord {
    pub ticket: String,
    pub expires_at: String,
}

#[derive(Debug, Clone)]
struct SshTicket {
    target: SshTarget,
    user_id: i32,
    operator: String,
    cols: u16,
    rows: u16,
    expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
enum SshTarget {
    Local(SshTargetRecord),
    Remote(SshTargetConfig),
}

#[derive(Debug, Deserialize)]
struct TerminalClientMessage {
    #[serde(rename = "type")]
    message_type: String,
    data: Option<String>,
    cols: Option<u16>,
    rows: Option<u16>,
}

#[derive(Debug, Serialize)]
struct TerminalServerMessage<'a> {
    #[serde(rename = "type")]
    message_type: &'a str,
    data: &'a str,
}

#[utoipa::path(
    get,
    path = "/api/admin/ssh/targets",
    tag = "admin-ssh",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<Vec<SshTargetRecord>>))
)]
#[debug_handler]
pub async fn targets(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:ssh:list").await?;
    Ok(responses::ok(target_records(&ctx).await?))
}

#[utoipa::path(
    post,
    path = "/api/admin/ssh/tickets",
    tag = "admin-ssh",
    security(("bearer_auth" = [])),
    request_body = CreateSshTicketParams,
    responses((status = 200, body = ApiResponse<SshTicketRecord>))
)]
#[debug_handler]
pub async fn create_ticket(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<CreateSshTicketParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:ssh:connect").await?;
    let target = find_target(&ctx, &params.target_key).await?;
    let ticket = Uuid::new_v4().to_string();
    let expires_at = Utc::now() + Duration::seconds(TICKET_TTL_SECONDS);
    let target_name = target.record().name.clone();
    let ssh_ticket = SshTicket {
        target,
        user_id: user.id,
        operator: Some(user.name.clone())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| user.email.clone()),
        cols: params.cols.unwrap_or(120).clamp(20, 300),
        rows: params.rows.unwrap_or(32).clamp(8, 100),
        expires_at,
    };
    ticket_store()
        .lock()
        .map_err(|_| ApiError::internal("failed to lock ssh ticket store"))?
        .insert(ticket.clone(), ssh_ticket);
    record_ssh_log(
        &ctx.db,
        user.id,
        Some(user.name.clone()),
        "create_ticket",
        format!("创建 SSH 终端连接票据：{target_name}"),
        Some(200),
        None,
    )
    .await;

    Ok(responses::ok(SshTicketRecord {
        ticket,
        expires_at: expires_at.to_rfc3339(),
    }))
}

#[debug_handler]
pub async fn terminal_ws(
    State(ctx): State<AppContext>,
    Path(ticket): Path<String>,
    ws: WebSocketUpgrade,
) -> ApiResult<Response> {
    let ticket = consume_ticket(&ticket)?;
    Ok(ws
        .on_upgrade(move |socket| handle_terminal_socket(socket, ticket, ctx.db))
        .into_response())
}

async fn target_records(ctx: &AppContext) -> ApiResult<Vec<SshTargetRecord>> {
    Ok(all_targets(ctx)
        .await?
        .into_iter()
        .map(|target| target.record())
        .collect())
}

async fn find_target(ctx: &AppContext, key: &str) -> ApiResult<SshTarget> {
    let normalized = key.trim();
    if normalized.is_empty() {
        return Err(ApiError::bad_request("ssh target is required"));
    }
    all_targets(ctx)
        .await?
        .into_iter()
        .find(|target| target.record().key == normalized)
        .ok_or_else(|| ApiError::bad_request("ssh target not found"))
}

async fn all_targets(ctx: &AppContext) -> ApiResult<Vec<SshTarget>> {
    let mut targets = vec![SshTarget::Local(local_target())];
    let value = system_settings::string_value(&ctx.db, TARGETS_SETTING_KEY, "[]").await?;
    let remote_targets = serde_json::from_str::<Vec<SshTargetConfig>>(&value)
        .map_err(|_| ApiError::bad_request("invalid ssh manager targets setting"))?;
    for target in remote_targets.into_iter().filter(|target| target.enabled) {
        targets.push(SshTarget::Remote(normalize_remote_target(target)?));
    }
    Ok(targets)
}

fn local_target() -> SshTargetRecord {
    SshTargetRecord {
        key: LOCAL_TARGET_KEY.to_string(),
        name: "本机 Shell".to_string(),
        target_type: "local".to_string(),
        host: None,
        port: None,
        username: None,
        enabled: true,
    }
}

fn normalize_remote_target(mut target: SshTargetConfig) -> ApiResult<SshTargetConfig> {
    target.key = target.key.trim().to_string();
    target.name = target.name.trim().to_string();
    target.host = target.host.trim().to_string();
    target.username = target.username.trim().to_string();
    target.key_path = target
        .key_path
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if target.key.is_empty()
        || target.name.is_empty()
        || target.host.is_empty()
        || target.username.is_empty()
        || contains_control_or_whitespace(&target.key)
        || contains_control_or_whitespace(&target.host)
        || contains_control_or_whitespace(&target.username)
        || target.key.starts_with('-')
        || target.host.starts_with('-')
        || target.username.starts_with('-')
    {
        return Err(ApiError::bad_request("invalid ssh target configuration"));
    }
    if target.port.is_some_and(|port| port == 0) {
        return Err(ApiError::bad_request("invalid ssh target port"));
    }
    if target
        .key_path
        .as_deref()
        .is_some_and(contains_control_char)
    {
        return Err(ApiError::bad_request("invalid ssh key path"));
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

impl SshTarget {
    fn record(&self) -> SshTargetRecord {
        match self {
            Self::Local(record) => record.clone(),
            Self::Remote(target) => SshTargetRecord {
                key: target.key.clone(),
                name: target.name.clone(),
                target_type: "ssh".to_string(),
                host: Some(target.host.clone()),
                port: target.port.or(Some(22)),
                username: Some(target.username.clone()),
                enabled: target.enabled,
            },
        }
    }

    fn command(&self) -> CommandBuilder {
        match self {
            Self::Local(_) => {
                let shell = env::var("SHELL")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| "/bin/sh".to_string());
                CommandBuilder::new(shell)
            }
            Self::Remote(target) => {
                let mut command = CommandBuilder::new("ssh");
                command.arg("-tt");
                command.arg("-o");
                command.arg("StrictHostKeyChecking=accept-new");
                command.arg("-o");
                command.arg("ServerAliveInterval=30");
                command.arg("-p");
                command.arg(target.port.unwrap_or(22).to_string());
                if let Some(key_path) = target.key_path.as_deref() {
                    command.arg("-i");
                    command.arg(key_path);
                }
                command.arg(format!("{}@{}", target.username, target.host));
                command
            }
        }
    }
}

fn ticket_store() -> &'static Mutex<HashMap<String, SshTicket>> {
    SSH_TICKETS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn consume_ticket(ticket: &str) -> ApiResult<SshTicket> {
    let now = Utc::now();
    let mut store = ticket_store()
        .lock()
        .map_err(|_| ApiError::internal("failed to lock ssh ticket store"))?;
    store.retain(|_, ticket| ticket.expires_at > now);
    let ticket = store
        .remove(ticket)
        .ok_or_else(|| ApiError::unauthorized("invalid or expired ssh ticket"))?;
    drop(store);
    if ticket.expires_at <= now {
        return Err(ApiError::unauthorized("invalid or expired ssh ticket"));
    }
    Ok(ticket)
}

async fn handle_terminal_socket(mut socket: WebSocket, ticket: SshTicket, db: DatabaseConnection) {
    let target = ticket.target.record();
    let target_name = target.name.clone();
    let pty_system = native_pty_system();
    let pair = match pty_system.openpty(PtySize {
        rows: ticket.rows,
        cols: ticket.cols,
        pixel_width: 0,
        pixel_height: 0,
    }) {
        Ok(pair) => pair,
        Err(err) => {
            let _ = send_status(&mut socket, "error", &format!("打开 PTY 失败：{err}")).await;
            return;
        }
    };
    let mut child = match pair.slave.spawn_command(ticket.target.command()) {
        Ok(child) => child,
        Err(err) => {
            let _ = send_status(&mut socket, "error", &format!("启动终端失败：{err}")).await;
            return;
        }
    };
    drop(pair.slave);
    let mut reader = match pair.master.try_clone_reader() {
        Ok(reader) => reader,
        Err(err) => {
            let _ = send_status(&mut socket, "error", &format!("读取终端失败：{err}")).await;
            let _ = child.kill();
            return;
        }
    };
    let mut writer = match pair.master.take_writer() {
        Ok(writer) => writer,
        Err(err) => {
            let _ = send_status(&mut socket, "error", &format!("写入终端失败：{err}")).await;
            let _ = child.kill();
            return;
        }
    };
    let (output_tx, output_rx) = mpsc::unbounded_channel::<String>();
    thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(size) => {
                    let chunk = String::from_utf8_lossy(&buffer[..size]).into_owned();
                    if output_tx.send(chunk).is_err() {
                        break;
                    }
                }
            }
        }
    });

    record_ssh_log(
        &db,
        ticket.user_id,
        Some(ticket.operator.clone()),
        "connect",
        format!("连接 SSH 终端：{target_name}"),
        Some(101),
        None,
    )
    .await;
    let _ = send_status(&mut socket, "connected", "connected").await;

    bridge_terminal_messages(&mut socket, output_rx, pair.master, &mut writer).await;

    let _ = child.kill();
    record_ssh_log(
        &db,
        ticket.user_id,
        Some(ticket.operator),
        "disconnect",
        format!("断开 SSH 终端：{target_name}"),
        Some(200),
        None,
    )
    .await;
    let _ = send_status(&mut socket, "closed", "closed").await;
}

async fn bridge_terminal_messages(
    socket: &mut WebSocket,
    mut output_rx: mpsc::UnboundedReceiver<String>,
    master: Box<dyn portable_pty::MasterPty + Send>,
    writer: &mut Box<dyn Write + Send>,
) {
    loop {
        tokio::select! {
            Some(output) = output_rx.recv() => {
                if send_output(socket, &output).await.is_err() {
                    break;
                }
            }
            message = socket.recv() => {
                match message {
                    Some(Ok(Message::Text(text))) => handle_client_message(&text, master.as_ref(), writer),
                    Some(Ok(Message::Binary(bytes))) => {
                        if writer.write_all(&bytes).is_err() || writer.flush().is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                }
            }
        }
    }
}

fn handle_client_message(
    text: &str,
    master: &(dyn portable_pty::MasterPty + Send),
    writer: &mut Box<dyn Write + Send>,
) {
    let Ok(message) = serde_json::from_str::<TerminalClientMessage>(text) else {
        return;
    };
    match message.message_type.as_str() {
        "input" => {
            if let Some(data) = message.data {
                let _ = writer.write_all(data.as_bytes());
                let _ = writer.flush();
            }
        }
        "resize" => {
            let _ = master.resize(PtySize {
                rows: message.rows.unwrap_or(32).clamp(8, 100),
                cols: message.cols.unwrap_or(120).clamp(20, 300),
                pixel_width: 0,
                pixel_height: 0,
            });
        }
        _ => {}
    }
}

async fn send_output(socket: &mut WebSocket, data: &str) -> Result<(), axum::Error> {
    send_terminal_message(socket, "output", data).await
}

async fn send_status(socket: &mut WebSocket, status: &str, data: &str) -> Result<(), axum::Error> {
    send_terminal_message(socket, status, data).await
}

async fn send_terminal_message(
    socket: &mut WebSocket,
    message_type: &str,
    data: &str,
) -> Result<(), axum::Error> {
    let payload = serde_json::to_string(&TerminalServerMessage { message_type, data })
        .unwrap_or_else(|_| "{\"type\":\"error\",\"data\":\"serialization failed\"}".to_string());
    socket.send(Message::Text(payload.into())).await
}

async fn record_ssh_log(
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
            module: "ssh",
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
