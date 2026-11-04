#![allow(clippy::missing_errors_doc)]

use std::{
    collections::HashMap,
    env, fs,
    io::{Read, Write},
    path::{Component, Path as FsPath, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread,
};

use axum::{
    extract::{ws::Message, ws::WebSocket, ws::WebSocketUpgrade, Path, Query},
    http::{header, HeaderMap, HeaderValue},
    response::IntoResponse,
};
use chrono::{DateTime, Duration, Utc};
use loco_rs::prelude::*;
use portable_pty::{native_pty_system, PtySize};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::{
    controllers::admin::{authorize, remote_sessions::RemoteSessionRecord},
    errors::{ApiError, ApiResult},
    models::{admin_logs, system_settings},
    responses::{self, ApiResponse},
    services::{
        command_runner::simplify_verbatim_path,
        ssh_session_host::{self, HostEvent, HostRequest, HostSpec, OutputHistory, ShellCommand},
    },
};

const TARGETS_SETTING_KEY: &str = "ssh_manager.targets";
const LOCAL_TARGET_KEY: &str = "local-shell";
const TICKET_TTL_SECONDS: i64 = 60;
/// Lines of output kept per session (`ssh_manager.scrollback_lines`).
static SCROLLBACK_LINES: AtomicUsize = AtomicUsize::new(ssh_session_host::MAX_SCROLLBACK_LINES);
const OUTPUT_BROADCAST_CAPACITY: usize = 512;
const MAX_FILE_PREVIEW_BYTES: u64 = 100 * 1024 * 1024;

static SSH_TICKETS: OnceLock<Mutex<HashMap<String, SshTicket>>> = OnceLock::new();
static SSH_SESSIONS: OnceLock<Mutex<HashMap<String, Arc<ManagedSshSession>>>> = OnceLock::new();

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
    /// Tab name; the target name when empty.
    pub title: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CreateSshSessionParams {
    pub target_key: String,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
    /// Tab name; the target name when empty.
    pub title: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct RenameSshSessionParams {
    /// New tab name (at most 40 characters); empty restores the default.
    pub title: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SshTicketRecord {
    pub ticket: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SshSessionRecord {
    pub id: String,
    pub target_key: String,
    pub target_name: String,
    pub target_type: String,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub status: String,
    pub cols: u16,
    pub rows: u16,
    pub current_directory: String,
    /// Tab name chosen by the user; clients show the target name otherwise.
    pub title: Option<String>,
    pub created_by: i32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SshFileRecord {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub extension: Option<String>,
    pub size_bytes: i64,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SshFileBrowserRecord {
    pub session_id: String,
    pub current_directory: String,
    pub path: String,
    /// `None` at a filesystem root (`/`, `C:\`).
    pub parent_path: Option<String>,
    pub directories: Vec<SshFileRecord>,
    pub files: Vec<SshFileRecord>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct SshFileBrowserParams {
    pub path: Option<String>,
}

#[derive(Debug, Clone)]
struct SshTicket {
    session_id: String,
    user_id: i32,
    operator: String,
    expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", content = "target", rename_all = "snake_case")]
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

#[derive(Debug, Clone)]
struct TerminalBroadcastMessage {
    message_type: String,
    data: String,
}

struct ManagedSshSession {
    id: String,
    target: SshTargetRecord,
    target_kind: SshTarget,
    user_id: i32,
    current_directory: Mutex<String>,
    status: Mutex<String>,
    cols: Mutex<u16>,
    rows: Mutex<u16>,
    created_at: DateTime<Utc>,
    updated_at: Mutex<DateTime<Utc>>,
    terminal: Terminal,
    output_buffer: Mutex<OutputHistory>,
    output_tx: broadcast::Sender<TerminalBroadcastMessage>,
    /// Browser terminals currently attached; idle only counts while 0.
    attached: AtomicUsize,
    title: Mutex<Option<String>>,
}

/// Where a session's PTY lives.
enum Terminal {
    /// In this process; the shell ends with the server.
    Local {
        master: Mutex<Box<dyn portable_pty::MasterPty + Send>>,
        writer: Mutex<Box<dyn Write + Send>>,
        child: Mutex<Option<Box<dyn portable_pty::Child + Send + Sync>>>,
    },
    /// In a session host process (`services::ssh_session_host`), which can
    /// outlive the server.
    Host(ssh_session_host::HostConnection),
}

/// What a session host keeps for restoring the session after a restart.
#[derive(Debug, Deserialize, Serialize)]
struct PersistedSession {
    target: SshTarget,
    user_id: i32,
    created_at_ms: i64,
    current_directory: String,
    #[serde(default)]
    title: Option<String>,
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
    get,
    path = "/api/admin/ssh/sessions",
    tag = "admin-ssh",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<Vec<SshSessionRecord>>))
)]
#[debug_handler]
pub async fn sessions(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:ssh:list").await?;
    let mut store = session_store()
        .lock()
        .map_err(|_| ApiError::internal("failed to lock ssh session store"))?;
    // Sessions whose shell has exited cannot be resumed; drop them so clients
    // stop offering them as reconnect targets.
    store.retain(|_, session| session.status() != "closed");
    // Each user only sees the shells they opened.
    let sessions = store
        .values()
        .filter(|session| session.user_id == user.id)
        .map(|session| session.record())
        .collect::<Vec<_>>();
    drop(store);
    Ok(responses::ok(sessions))
}

#[utoipa::path(
    post,
    path = "/api/admin/ssh/sessions",
    tag = "admin-ssh",
    security(("bearer_auth" = [])),
    request_body = CreateSshSessionParams,
    responses((status = 200, body = ApiResponse<SshSessionRecord>))
)]
#[debug_handler]
pub async fn create_session(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    session: crate::controllers::admin::SessionExt,
    Json(params): Json<CreateSshSessionParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:ssh:connect").await?;
    crate::controllers::admin::require_remote_reauth(&ctx, &session).await?;
    let title = normalize_title(params.title.as_deref())?;
    let target = find_target(&ctx, &params.target_key).await?;
    let operator = Some(user.name.clone())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| user.email.clone());
    let session = open_session(
        &ctx,
        &target,
        user.id,
        params.cols.unwrap_or(120).clamp(20, 300),
        params.rows.unwrap_or(32).clamp(8, 100),
    )
    .await?;
    if title.is_some() {
        session.set_title(title);
    }
    let target_name = session.target.name.clone();
    record_ssh_log(
        &ctx.db,
        user.id,
        Some(operator),
        "connect",
        format!("创建 SSH 共享会话：{target_name}"),
        Some(101),
        None,
    )
    .await;
    Ok(responses::ok(session.record()))
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
    session: crate::controllers::admin::SessionExt,
    Json(params): Json<CreateSshTicketParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:ssh:connect").await?;
    crate::controllers::admin::require_remote_reauth(&ctx, &session).await?;
    let title = normalize_title(params.title.as_deref())?;
    let target = find_target(&ctx, &params.target_key).await?;
    let operator = Some(user.name.clone())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| user.email.clone());
    let session = open_session(
        &ctx,
        &target,
        user.id,
        params.cols.unwrap_or(120).clamp(20, 300),
        params.rows.unwrap_or(32).clamp(8, 100),
    )
    .await?;
    if title.is_some() {
        session.set_title(title);
    }
    let record = create_attach_ticket(&session.id, user.id, operator.clone())?;
    record_ssh_log(
        &ctx.db,
        user.id,
        Some(operator),
        "create_ticket",
        format!("创建 SSH 终端连接票据：{}", session.target.name),
        Some(200),
        None,
    )
    .await;
    Ok(responses::ok(record))
}

#[utoipa::path(
    post,
    path = "/api/admin/ssh/sessions/{id}/tickets",
    tag = "admin-ssh",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<SshTicketRecord>))
)]
#[debug_handler]
pub async fn create_session_ticket(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:ssh:connect").await?;
    let session = find_live_session(&id, user.id)?;
    let operator = Some(user.name.clone())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| user.email.clone());
    let record = create_attach_ticket(&session.id, user.id, operator.clone())?;
    record_ssh_log(
        &ctx.db,
        user.id,
        Some(operator),
        "attach_ticket",
        format!("创建 SSH 会话附加票据：{}", session.target.name),
        Some(200),
        None,
    )
    .await;
    Ok(responses::ok(record))
}

#[utoipa::path(
    delete,
    path = "/api/admin/ssh/sessions/{id}",
    tag = "admin-ssh",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<SshSessionRecord>))
)]
#[debug_handler]
pub async fn close_session(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:ssh:connect").await?;
    find_session(&id, user.id)?;
    let session = remove_session(&id)?;
    session.close();
    record_ssh_log(
        &ctx.db,
        user.id,
        Some(user.name.clone()),
        "close",
        format!("关闭 SSH 共享会话：{}", session.target.name),
        Some(200),
        None,
    )
    .await;
    Ok(responses::ok(session.record()))
}

#[utoipa::path(
    put,
    path = "/api/admin/ssh/sessions/{id}",
    tag = "admin-ssh",
    security(("bearer_auth" = [])),
    request_body = RenameSshSessionParams,
    responses((status = 200, body = ApiResponse<SshSessionRecord>))
)]
#[debug_handler]
pub async fn rename_session(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<String>,
    Json(params): Json<RenameSshSessionParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:ssh:connect").await?;
    let title = normalize_title(params.title.as_deref())?;
    let session = find_session(&id, user.id)?;
    tokio::task::spawn_blocking({
        let session = Arc::clone(&session);
        move || session.set_title(title)
    })
    .await
    .map_err(|_| ApiError::internal("failed to rename ssh session"))?;
    Ok(responses::ok(session.record()))
}

pub const MAX_TITLE_CHARS: usize = 40;

/// A tab name without control characters; `None` for an empty one.
fn normalize_title(value: Option<&str>) -> ApiResult<Option<String>> {
    let title: String = value
        .unwrap_or_default()
        .chars()
        .filter(|char| !char.is_control())
        .collect();
    let title = title.trim();
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err(ApiError::bad_request("标签名称最多 40 个字"));
    }
    Ok(Some(title.to_string()).filter(|title| !title.is_empty()))
}

#[utoipa::path(
    get,
    path = "/api/admin/ssh/sessions/{id}/files",
    tag = "admin-ssh",
    security(("bearer_auth" = [])),
    params(SshFileBrowserParams),
    responses((status = 200, body = ApiResponse<SshFileBrowserRecord>))
)]
#[debug_handler]
pub async fn session_files(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<String>,
    Query(params): Query<SshFileBrowserParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:ssh:list").await?;
    let session = find_session(&id, user.id)?;
    Ok(responses::ok(session.file_browser(params.path.as_deref())?))
}

#[utoipa::path(
    get,
    path = "/api/admin/ssh/sessions/{id}/files/preview",
    tag = "admin-ssh",
    security(("bearer_auth" = [])),
    params(("id" = String, Path), SshFileBrowserParams),
    responses((status = 200, description = "Preview a file from the SSH session"))
)]
#[debug_handler]
pub async fn preview_session_file(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<String>,
    Query(params): Query<SshFileBrowserParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:ssh:list").await?;
    let path = params
        .path
        .as_deref()
        .ok_or_else(|| ApiError::bad_request("file path is required"))?;
    let session = find_session(&id, user.id)?;
    let target = session.preview_file_path(path)?;
    let metadata = target
        .metadata()
        .map_err(|_| ApiError::bad_request("file metadata is unavailable"))?;
    if metadata.len() > MAX_FILE_PREVIEW_BYTES {
        return Err(ApiError::bad_request("file is too large to preview"));
    }
    let bytes = fs::read(&target).map_err(|_| ApiError::bad_request("file content not found"))?;
    let file_name = target
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("preview")
        .replace(['\r', '\n', '"'], "_");
    let content_type = mime_guess::from_path(&target).first_or_octet_stream();
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(content_type.as_ref())
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("inline; filename=\"{file_name}\""))
            .map_err(|_| ApiError::internal("failed to build preview response"))?,
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    Ok((headers, bytes).into_response())
}

#[debug_handler]
pub async fn terminal_ws(Path(ticket): Path<String>, ws: WebSocketUpgrade) -> ApiResult<Response> {
    let ticket = consume_ticket(&ticket)?;
    let session = find_live_session(&ticket.session_id, ticket.user_id)?;
    Ok(ws
        .on_upgrade(move |socket| handle_terminal_socket(socket, session, ticket))
        .into_response())
}

#[debug_handler]
pub async fn session_ws(
    Path((id, ticket)): Path<(String, String)>,
    ws: WebSocketUpgrade,
) -> ApiResult<Response> {
    let ticket = consume_ticket(&ticket)?;
    if ticket.session_id != id {
        return Err(ApiError::unauthorized("invalid ssh session ticket"));
    }
    let session = find_live_session(&id, ticket.user_id)?;
    Ok(ws
        .on_upgrade(move |socket| handle_terminal_socket(socket, session, ticket))
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

    fn command(&self) -> ShellCommand {
        match self {
            Self::Local(_) => {
                #[cfg(windows)]
                let mut command = {
                    let mut command = ShellCommand::new("powershell.exe");
                    command.arg("-NoLogo").arg("-NoProfile").arg("-NoExit");
                    command.env(
                        "PATH",
                        crate::services::windows_env::refreshed_path().to_string_lossy(),
                    );
                    command
                };
                #[cfg(not(windows))]
                let mut command = {
                    let shell = env::var("SHELL")
                        .ok()
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or_else(|| "/bin/sh".to_string());
                    // A login shell loads the profile (and PATH) a service lacks.
                    let mut command = ShellCommand::new(shell);
                    command.arg("-l").arg("-i");
                    command
                };
                command.env("TERM", "xterm-256color");
                command
            }
            Self::Remote(target) => {
                let mut command = ShellCommand::new("ssh");
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

impl ManagedSshSession {
    fn record(&self) -> SshSessionRecord {
        SshSessionRecord {
            id: self.id.clone(),
            target_key: self.target.key.clone(),
            target_name: self.target.name.clone(),
            target_type: self.target.target_type.clone(),
            host: self.target.host.clone(),
            port: self.target.port,
            username: self.target.username.clone(),
            status: self.status(),
            cols: Self::locked_value(&self.cols, 120),
            rows: Self::locked_value(&self.rows, 32),
            current_directory: self.current_directory(),
            title: self.title(),
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

    fn remote_record(&self) -> RemoteSessionRecord {
        let record = self.record();
        RemoteSessionRecord {
            kind: "ssh".to_string(),
            id: record.id,
            user_id: self.user_id,
            user_name: None,
            user_email: None,
            target_key: record.target_key,
            target_name: record.target_name,
            address: record.host.map(|host| match record.port {
                Some(port) => format!("{host}:{port}"),
                None => host,
            }),
            status: record.status,
            viewers: self.attached.load(Ordering::SeqCst),
            created_at: record.created_at,
            last_active_at: record.updated_at,
        }
    }

    fn current_directory(&self) -> String {
        self.current_directory
            .lock()
            .map_or_else(|_| "~".to_string(), |directory| directory.clone())
    }

    fn locked_value(value: &Mutex<u16>, fallback: u16) -> u16 {
        value.lock().map_or(fallback, |value| *value)
    }

    /// New output from now on plus everything kept so far. Taken under the
    /// buffer lock, which `append_output` also broadcasts under, so no output
    /// is replayed twice or missed.
    fn subscribe_with_history(
        &self,
    ) -> (broadcast::Receiver<TerminalBroadcastMessage>, Vec<String>) {
        self.output_buffer.lock().map_or_else(
            |_| (self.output_tx.subscribe(), Vec::new()),
            |buffer| (self.output_tx.subscribe(), buffer.snapshot()),
        )
    }

    fn append_output(&self, data: String) {
        if let Ok(mut buffer) = self.output_buffer.lock() {
            buffer.push(data.clone(), scrollback_limit());
            let _ = self.output_tx.send(TerminalBroadcastMessage {
                message_type: "output".to_string(),
                data,
            });
        }
        self.touch();
    }

    fn mark_closed(&self) {
        if let Ok(mut status) = self.status.lock() {
            *status = "closed".to_string();
        }
        self.touch();
        let _ = self.output_tx.send(TerminalBroadcastMessage {
            message_type: "closed".to_string(),
            data: "closed".to_string(),
        });
    }

    fn touch(&self) {
        if let Ok(mut updated_at) = self.updated_at.lock() {
            *updated_at = Utc::now();
        }
    }

    fn resize(&self, cols: u16, rows: u16) {
        let cols = cols.clamp(20, 300);
        let rows = rows.clamp(8, 100);
        match &self.terminal {
            Terminal::Local { master, .. } => {
                if let Ok(master) = master.lock() {
                    let _ = master.resize(PtySize {
                        rows,
                        cols,
                        pixel_width: 0,
                        pixel_height: 0,
                    });
                }
            }
            Terminal::Host(host) => {
                let _ = host.send(&HostRequest::Resize { cols, rows });
            }
        }
        if let Ok(mut current_cols) = self.cols.lock() {
            *current_cols = cols;
        }
        if let Ok(mut current_rows) = self.rows.lock() {
            *current_rows = rows;
        }
        self.touch();
    }

    fn write_input(&self, data: &[u8]) {
        match &self.terminal {
            Terminal::Local { writer, .. } => {
                if let Ok(mut writer) = writer.lock() {
                    let _ = writer.write_all(data);
                    let _ = writer.flush();
                }
            }
            Terminal::Host(host) => {
                let _ = host.send(&HostRequest::Input {
                    data: data.to_vec(),
                });
            }
        }
        self.touch();
    }

    fn close(&self) {
        match &self.terminal {
            Terminal::Local { child, .. } => {
                if let Ok(mut child) = child.lock() {
                    if let Some(child) = child.as_mut() {
                        let _ = child.kill();
                    }
                    *child = None;
                }
            }
            Terminal::Host(host) => host.close(),
        }
        self.mark_closed();
    }

    fn title(&self) -> Option<String> {
        self.title.lock().ok().and_then(|title| title.clone())
    }

    /// Renames the tab; a host-backed session keeps the name across server
    /// restarts.
    fn set_title(&self, title: Option<String>) {
        if let Ok(mut current) = self.title.lock() {
            current.clone_from(&title);
        }
        if !matches!(self.terminal, Terminal::Host(_)) {
            return;
        }
        let metadata = serde_json::to_value(PersistedSession {
            target: self.target_kind.clone(),
            user_id: self.user_id,
            created_at_ms: self.created_at.timestamp_millis(),
            current_directory: self.current_directory(),
            title,
        });
        if let Err(error) = metadata
            .map_err(|error| error.to_string())
            .and_then(|metadata| ssh_session_host::save_session_metadata(&self.id, metadata))
        {
            tracing::warn!(
                error,
                session_id = self.id,
                "failed to save ssh session title"
            );
        }
    }

    fn set_persist(&self, persist: bool) {
        if let Terminal::Host(host) = &self.terminal {
            let _ = host.send(&HostRequest::Persist { value: persist });
        }
    }

    fn file_browser(&self, path: Option<&str>) -> ApiResult<SshFileBrowserRecord> {
        match &self.target_kind {
            SshTarget::Local(_) => self.local_file_browser(path),
            SshTarget::Remote(_) => Err(ApiError::bad_request(
                "remote ssh file browser is not supported yet",
            )),
        }
    }

    fn preview_file_path(&self, path: &str) -> ApiResult<PathBuf> {
        match &self.target_kind {
            SshTarget::Local(_) => {
                let target = resolve_local_browser_path(&self.current_directory(), Some(path))?
                    .canonicalize()
                    .map_err(|_| ApiError::bad_request("invalid file path"))?;
                if !target.is_file() {
                    return Err(ApiError::bad_request("path is not a file"));
                }
                Ok(target)
            }
            SshTarget::Remote(_) => Err(ApiError::bad_request(
                "remote ssh file preview is not supported yet",
            )),
        }
    }

    fn local_file_browser(&self, path: Option<&str>) -> ApiResult<SshFileBrowserRecord> {
        let directory = resolve_local_browser_path(&self.current_directory(), path)?;
        if !directory.is_dir() {
            return Err(ApiError::bad_request("path is not a directory"));
        }
        let normalized = simplify_verbatim_path(
            &directory
                .canonicalize()
                .map_err(|_| ApiError::bad_request("invalid directory"))?,
        );
        if let Ok(mut current_directory) = self.current_directory.lock() {
            *current_directory = normalized.to_string_lossy().to_string();
        }
        let mut directories = Vec::new();
        let mut files = Vec::new();
        for entry in
            fs::read_dir(&normalized).map_err(|_| ApiError::internal("failed to list files"))?
        {
            let entry = entry.map_err(|_| ApiError::internal("failed to list files"))?;
            let metadata = entry
                .metadata()
                .map_err(|_| ApiError::internal("failed to read file metadata"))?;
            let name = entry.file_name().to_string_lossy().to_string();
            let path = entry.path().to_string_lossy().to_string();
            let record = ssh_file_record(&name, path, &metadata);
            if metadata.is_dir() {
                directories.push(record);
            } else if metadata.is_file() {
                files.push(record);
            }
        }
        directories.sort_by(|left, right| left.name.cmp(&right.name));
        files.sort_by(|left, right| left.name.cmp(&right.name));
        let path = normalized.to_string_lossy().to_string();
        Ok(SshFileBrowserRecord {
            session_id: self.id.clone(),
            current_directory: path.clone(),
            path,
            parent_path: normalized
                .parent()
                .map(|parent| parent.to_string_lossy().to_string()),
            directories,
            files,
        })
    }
}

fn ticket_store() -> &'static Mutex<HashMap<String, SshTicket>> {
    SSH_TICKETS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn session_store() -> &'static Mutex<HashMap<String, Arc<ManagedSshSession>>> {
    SSH_SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn resolve_local_browser_path(current_directory: &str, path: Option<&str>) -> ApiResult<PathBuf> {
    let value = path.unwrap_or(current_directory).trim();
    if value.chars().any(char::is_control) {
        return Err(ApiError::bad_request("invalid path"));
    }
    let candidate = if value.is_empty() {
        PathBuf::from(current_directory)
    } else if FsPath::new(value).is_absolute() {
        PathBuf::from(value)
    } else {
        PathBuf::from(current_directory).join(value)
    };
    normalize_path_components(&simplify_verbatim_path(&candidate))
}

fn normalize_path_components(path: &FsPath) -> ApiResult<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            // Windows drive (`C:`) or UNC share; `..` never climbs above it
            // because `PathBuf::pop` stops at the root.
            Component::Prefix(_) | Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(value) => normalized.push(value),
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err(ApiError::bad_request("invalid path"));
    }
    Ok(normalized)
}

fn ssh_file_record(name: &str, path: String, metadata: &fs::Metadata) -> SshFileRecord {
    let extension = FsPath::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .map(ToString::to_string);
    let updated_at = metadata
        .modified()
        .ok()
        .map(DateTime::<Utc>::from)
        .map(|value| value.to_rfc3339());
    SshFileRecord {
        name: name.to_string(),
        path,
        is_dir: metadata.is_dir(),
        extension,
        size_bytes: i64::try_from(metadata.len()).unwrap_or(i64::MAX),
        updated_at,
    }
}

fn initial_local_directory() -> String {
    env::current_dir()
        .ok()
        .or_else(|| env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/"))
        .to_string_lossy()
        .to_string()
}

/// How a new session runs its PTY.
#[derive(Clone, Copy)]
enum Backend {
    /// In this process, e.g. in tests where the binary cannot be a host.
    Local,
    /// In a session host; `persist` keeps it across server restarts.
    Host { persist: bool },
}

impl Backend {
    async fn for_context(ctx: &AppContext) -> Self {
        if matches!(ctx.environment, loco_rs::environment::Environment::Test) {
            Self::Local
        } else {
            Self::Host {
                persist: keep_sessions_on_restart(&ctx.db).await,
            }
        }
    }
}

/// Opens the session off the async runtime: starting a host waits for it.
async fn open_session(
    ctx: &AppContext,
    target: &SshTarget,
    user_id: i32,
    cols: u16,
    rows: u16,
) -> ApiResult<Arc<ManagedSshSession>> {
    let backend = Backend::for_context(ctx).await;
    let target = target.clone();
    tokio::task::spawn_blocking(move || {
        create_managed_session(&target, user_id, cols, rows, backend)
    })
    .await
    .map_err(|_| ApiError::internal("failed to open ssh session"))?
}

fn create_managed_session(
    target: &SshTarget,
    user_id: i32,
    cols: u16,
    rows: u16,
    backend: Backend,
) -> ApiResult<Arc<ManagedSshSession>> {
    let current_directory = match target {
        SshTarget::Local(_) => initial_local_directory(),
        SshTarget::Remote(_) => "~".to_string(),
    };
    if let Backend::Host { persist } = backend {
        let id = Uuid::new_v4().to_string();
        let created_at = Utc::now();
        let spec = HostSpec {
            token: Uuid::new_v4().to_string(),
            command: target.command(),
            cols,
            rows,
            persist,
            history_lines: scrollback_limit(),
            session: serde_json::to_value(PersistedSession {
                target: target.clone(),
                user_id,
                created_at_ms: created_at.timestamp_millis(),
                current_directory: current_directory.clone(),
                title: None,
            })
            .map_err(|_| ApiError::internal("failed to serialize ssh session"))?,
        };
        match ssh_session_host::spawn(&id, &spec) {
            Ok(attached) => {
                return register_host_session(ManagedSshSession::from_host(
                    id,
                    target,
                    user_id,
                    current_directory,
                    created_at,
                    attached,
                ));
            }
            // Still usable, just not across restarts.
            Err(error) => {
                tracing::warn!(error, "ssh session host unavailable, using a local PTY");
            }
        }
    }
    create_local_session(target, user_id, cols, rows, current_directory)
}

fn create_local_session(
    target: &SshTarget,
    user_id: i32,
    cols: u16,
    rows: u16,
    current_directory: String,
) -> ApiResult<Arc<ManagedSshSession>> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|err| ApiError::internal(format!("打开 PTY 失败：{err}")))?;
    let mut child = pair
        .slave
        .spawn_command(target.command().builder())
        .map_err(|err| ApiError::internal(format!("启动终端失败：{err}")))?;
    drop(pair.slave);
    let mut reader = match pair.master.try_clone_reader() {
        Ok(reader) => reader,
        Err(err) => {
            let _ = child.kill();
            return Err(ApiError::internal(format!("读取终端失败：{err}")));
        }
    };
    let writer = match pair.master.take_writer() {
        Ok(writer) => writer,
        Err(err) => {
            let _ = child.kill();
            return Err(ApiError::internal(format!("写入终端失败：{err}")));
        }
    };
    let (output_tx, _) = broadcast::channel::<TerminalBroadcastMessage>(OUTPUT_BROADCAST_CAPACITY);
    let session = Arc::new(ManagedSshSession {
        id: Uuid::new_v4().to_string(),
        target: target.record(),
        target_kind: target.clone(),
        user_id,
        current_directory: Mutex::new(current_directory),
        status: Mutex::new("running".to_string()),
        cols: Mutex::new(cols),
        rows: Mutex::new(rows),
        created_at: Utc::now(),
        updated_at: Mutex::new(Utc::now()),
        terminal: Terminal::Local {
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            child: Mutex::new(Some(child)),
        },
        output_buffer: Mutex::new(OutputHistory::default()),
        output_tx,
        attached: AtomicUsize::new(0),
        title: Mutex::new(None),
    });
    session_store()
        .lock()
        .map_err(|_| ApiError::internal("failed to lock ssh session store"))?
        .insert(session.id.clone(), Arc::clone(&session));
    spawn_output_reader(Arc::clone(&session), &mut reader);
    Ok(session)
}

fn spawn_output_reader(session: Arc<ManagedSshSession>, reader: &mut Box<dyn Read + Send>) {
    let mut reader = std::mem::replace(reader, Box::new(std::io::empty()));
    thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(size) => {
                    session.append_output(String::from_utf8_lossy(&buffer[..size]).into_owned());
                }
            }
        }
        session.mark_closed();
        if let Ok(mut store) = session_store().lock() {
            store.remove(&session.id);
        }
    });
}

impl ManagedSshSession {
    fn from_host(
        id: String,
        target: &SshTarget,
        user_id: i32,
        current_directory: String,
        created_at: DateTime<Utc>,
        attached: ssh_session_host::Attached,
    ) -> (Self, ssh_session_host::HostEvents) {
        let (output_tx, _) =
            broadcast::channel::<TerminalBroadcastMessage>(OUTPUT_BROADCAST_CAPACITY);
        let last_activity =
            DateTime::from_timestamp_millis(attached.last_activity_ms).unwrap_or_else(Utc::now);
        let history = OutputHistory::from_chunks(attached.history, scrollback_limit());
        let session = Self {
            id,
            target: target.record(),
            target_kind: target.clone(),
            user_id,
            current_directory: Mutex::new(current_directory),
            status: Mutex::new("running".to_string()),
            cols: Mutex::new(attached.cols),
            rows: Mutex::new(attached.rows),
            created_at,
            updated_at: Mutex::new(last_activity),
            terminal: Terminal::Host(attached.connection),
            output_buffer: Mutex::new(history),
            output_tx,
            attached: AtomicUsize::new(0),
            title: Mutex::new(None),
        };
        (session, attached.events)
    }
}

/// Stores a host-backed session and forwards the host's output to it.
fn register_host_session(
    (session, events): (ManagedSshSession, ssh_session_host::HostEvents),
) -> ApiResult<Arc<ManagedSshSession>> {
    let session = Arc::new(session);
    let stored = session_store()
        .lock()
        .map(|mut store| store.insert(session.id.clone(), Arc::clone(&session)))
        .is_ok();
    if !stored {
        session.close();
        return Err(ApiError::internal("failed to lock ssh session store"));
    }
    let reader = Arc::clone(&session);
    thread::spawn(move || {
        let mut events = events;
        loop {
            let mut exited = false;
            for event in events.by_ref() {
                match event {
                    HostEvent::Output { data } => reader.append_output(data),
                    HostEvent::Exit => {
                        exited = true;
                        break;
                    }
                    HostEvent::Hello { .. } | HostEvent::Busy => {}
                }
            }
            if exited || reader.status() == "closed" {
                break;
            }
            // The connection dropped but the shell still runs in its host:
            // attach again instead of ending the session.
            let Terminal::Host(host) = &reader.terminal else {
                break;
            };
            match host.reconnect() {
                Some(next) => {
                    tracing::info!(session_id = reader.id, "reattached ssh session host");
                    events = next;
                }
                None => break,
            }
        }
        reader.mark_closed();
        if let Ok(mut store) = session_store().lock() {
            store.remove(&reader.id);
        }
    });
    Ok(session)
}

fn create_attach_ticket(
    session_id: &str,
    user_id: i32,
    operator: String,
) -> ApiResult<SshTicketRecord> {
    find_session(session_id, user_id)?;
    let ticket = Uuid::new_v4().to_string();
    let expires_at = Utc::now() + Duration::seconds(TICKET_TTL_SECONDS);
    let ssh_ticket = SshTicket {
        session_id: session_id.to_string(),
        user_id,
        operator,
        expires_at,
    };
    ticket_store()
        .lock()
        .map_err(|_| ApiError::internal("failed to lock ssh ticket store"))?
        .insert(ticket.clone(), ssh_ticket);
    Ok(SshTicketRecord {
        ticket,
        expires_at: expires_at.to_rfc3339(),
    })
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

/// Error code clients use to recreate a session that no longer exists, e.g.
/// after a server restart or once its shell has exited.
pub const SESSION_NOT_FOUND_CODE: &str = "SSH_SESSION_NOT_FOUND";

fn session_not_found(message: &str) -> ApiError {
    ApiError::not_found(message).with_code(SESSION_NOT_FOUND_CODE)
}

fn session_by_id(id: &str) -> ApiResult<Arc<ManagedSshSession>> {
    session_store()
        .lock()
        .map_err(|_| ApiError::internal("failed to lock ssh session store"))?
        .get(id)
        .cloned()
        .ok_or_else(|| session_not_found("ssh session not found"))
}

/// A session of `user_id`. Other users' sessions answer "not found" so their
/// existence is not revealed.
fn find_session(id: &str, user_id: i32) -> ApiResult<Arc<ManagedSshSession>> {
    let session = session_by_id(id)?;
    if session.user_id == user_id {
        Ok(session)
    } else {
        Err(session_not_found("ssh session not found"))
    }
}

/// Like [`find_session`], but treats a session whose shell has exited as
/// gone and drops it, so attaching never lands on a dead terminal.
fn find_live_session(id: &str, user_id: i32) -> ApiResult<Arc<ManagedSshSession>> {
    let session = find_session(id, user_id)?;
    if session.status() == "closed" {
        let _ = remove_session(id);
        return Err(session_not_found("ssh session has ended"));
    }
    Ok(session)
}

fn remove_session(id: &str) -> ApiResult<Arc<ManagedSshSession>> {
    session_store()
        .lock()
        .map_err(|_| ApiError::internal("failed to lock ssh session store"))?
        .remove(id)
        .ok_or_else(|| session_not_found("ssh session not found"))
}

async fn handle_terminal_socket(
    mut socket: WebSocket,
    session: Arc<ManagedSshSession>,
    ticket: SshTicket,
) {
    let (mut output_rx, history) = session.subscribe_with_history();
    let _attached = AttachGuard::new(&session);
    // The client clears its terminal on `connected` and rebuilds it from the
    // replayed output, so reconnecting shows the same screen, not a copy.
    let _ = send_status(&mut socket, "connected", "connected").await;
    let _ = send_terminal_message(&mut socket, "scrollback", &scrollback_limit().to_string()).await;
    for output in history {
        if send_output(&mut socket, &output).await.is_err() {
            return;
        }
    }

    loop {
        tokio::select! {
            output = output_rx.recv() => {
                match output {
                    Ok(output) => {
                        if send_terminal_message(&mut socket, &output.message_type, &output.data).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            message = socket.recv() => {
                match message {
                    Some(Ok(Message::Text(text))) => handle_client_message(&text, &session),
                    Some(Ok(Message::Binary(bytes))) => session.write_input(&bytes),
                    Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                }
            }
        }
    }

    tracing::debug!(
        session_id = session.id,
        user_id = ticket.user_id,
        operator = ticket.operator,
        "ssh websocket detached"
    );
}

fn handle_client_message(text: &str, session: &ManagedSshSession) {
    let Ok(message) = serde_json::from_str::<TerminalClientMessage>(text) else {
        return;
    };
    match message.message_type.as_str() {
        "input" => {
            if let Some(data) = message.data {
                session.write_input(data.as_bytes());
            }
        }
        "resize" => session.resize(message.cols.unwrap_or(120), message.rows.unwrap_or(32)),
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

/// Counts an attached browser terminal for as long as it lives.
struct AttachGuard {
    session: Arc<ManagedSshSession>,
}

impl AttachGuard {
    fn new(session: &Arc<ManagedSshSession>) -> Self {
        session.attached.fetch_add(1, Ordering::SeqCst);
        session.touch();
        Self {
            session: Arc::clone(session),
        }
    }
}

impl Drop for AttachGuard {
    fn drop(&mut self) {
        self.session.attached.fetch_sub(1, Ordering::SeqCst);
        // Idle time starts when the last terminal detaches.
        self.session.touch();
    }
}

pub const IDLE_TIMEOUT_SETTING: &str = "ssh_manager.idle_timeout_minutes";
pub const MAX_LIFETIME_SETTING: &str = "ssh_manager.max_session_hours";
pub const KEEP_ON_RESTART_SETTING: &str = "ssh_manager.keep_sessions_on_restart";
pub const SCROLLBACK_SETTING: &str = "ssh_manager.scrollback_lines";

fn scrollback_limit() -> usize {
    SCROLLBACK_LINES.load(Ordering::SeqCst)
}

/// Reads `ssh_manager.scrollback_lines` and trims every session (and its
/// host) to it; called on startup and whenever the setting is saved.
pub async fn apply_scrollback_setting(db: &DatabaseConnection) {
    let setting = system_settings::number_i64(db, SCROLLBACK_SETTING, 0)
        .await
        .unwrap_or(0);
    let lines = ssh_session_host::scrollback_lines(setting);
    SCROLLBACK_LINES.store(lines, Ordering::SeqCst);
    let sessions: Vec<_> = session_store()
        .lock()
        .map(|store| store.values().cloned().collect())
        .unwrap_or_default();
    for session in sessions {
        if let Ok(mut buffer) = session.output_buffer.lock() {
            buffer.trim(lines);
        }
        if let Terminal::Host(host) = &session.terminal {
            let _ = host.send(&HostRequest::HistoryLimit { lines });
        }
    }
}

/// `ssh_manager.keep_sessions_on_restart`, on unless set to `false`.
async fn keep_sessions_on_restart(db: &DatabaseConnection) -> bool {
    !system_settings::string_value(db, KEEP_ON_RESTART_SETTING, "true")
        .await
        .is_ok_and(|value| value.trim() == "false")
}

/// Tells every session host whether to outlive the server, after
/// `ssh_manager.keep_sessions_on_restart` changed.
pub async fn apply_keep_setting(db: &DatabaseConnection) {
    let persist = keep_sessions_on_restart(db).await;
    let sessions: Vec<_> = session_store()
        .lock()
        .map(|store| store.values().cloned().collect())
        .unwrap_or_default();
    for session in sessions {
        session.set_persist(persist);
    }
}

/// Attaches to the session hosts an earlier server run left.
///
/// Their terminals continue where they were. With
/// `ssh_manager.keep_sessions_on_restart` off they are ended instead (only a
/// crash leaves them then). Runs before the server listens, so clients
/// reconnecting right away find their sessions.
pub async fn restore_sessions(db: &DatabaseConnection) {
    apply_scrollback_setting(db).await;
    let persist = keep_sessions_on_restart(db).await;
    let restored = tokio::task::spawn_blocking(move || {
        ssh_session_host::remove_stale_executables();
        let mut restored = 0_usize;
        for leftover in ssh_session_host::leftovers() {
            let Ok(session) =
                serde_json::from_value::<PersistedSession>(leftover.spec.session.clone())
            else {
                leftover.discard();
                continue;
            };
            if !persist {
                leftover.discard();
                continue;
            }
            let Ok(attached) = leftover.attach() else {
                continue;
            };
            let created_at =
                DateTime::from_timestamp_millis(session.created_at_ms).unwrap_or_else(Utc::now);
            let managed = ManagedSshSession::from_host(
                leftover.id.clone(),
                &session.target,
                session.user_id,
                session.current_directory,
                created_at,
                attached,
            );
            if let Ok(mut title) = managed.0.title.lock() {
                title.clone_from(&session.title);
            }
            if register_host_session(managed).is_ok() {
                restored += 1;
            }
        }
        restored
    })
    .await
    .unwrap_or(0);
    if restored > 0 {
        tracing::info!(restored, "restored ssh sessions from the previous run");
    }
}
#[allow(clippy::duration_suboptimal_units)]
const REAPER_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

/// Why a session should be closed now, if at all. Limits of 0 mean
/// "keep until the shell exits or the service stops".
fn expiry_reason(
    created_at: DateTime<Utc>,
    last_activity: DateTime<Utc>,
    attached: usize,
    now: DateTime<Utc>,
    idle_minutes: i64,
    max_hours: i64,
) -> Option<&'static str> {
    if max_hours > 0 && now - created_at >= Duration::hours(max_hours) {
        return Some("max_lifetime");
    }
    if idle_minutes > 0 && attached == 0 && now - last_activity >= Duration::minutes(idle_minutes) {
        return Some("idle");
    }
    None
}

fn close_sessions_where(
    predicate: impl Fn(&ManagedSshSession) -> bool,
) -> Vec<Arc<ManagedSshSession>> {
    let Ok(mut store) = session_store().lock() else {
        return Vec::new();
    };
    let ids: Vec<String> = store
        .values()
        .filter(|session| predicate(session))
        .map(|session| session.id.clone())
        .collect();
    let closed: Vec<_> = ids.iter().filter_map(|id| store.remove(id)).collect();
    drop(store);
    for session in &closed {
        session.close();
    }
    closed
}

/// Ends every SSH session of a user (disabled or deleted account).
#[must_use = "returns how many sessions were closed"]
pub fn close_user_sessions(user_id: i32) -> usize {
    close_sessions_where(|session| session.user_id == user_id).len()
}

/// Every user's open shells, for the 在线会话 admin page.
#[must_use]
pub fn remote_sessions() -> Vec<RemoteSessionRecord> {
    session_store().lock().map_or_else(
        |_| Vec::new(),
        |store| {
            store
                .values()
                .filter(|session| session.status() != "closed")
                .map(|session| session.remote_record())
                .collect()
        },
    )
}

/// Closes any user's shell and detaches its terminals.
#[must_use = "returns the closed session, if it existed"]
pub fn close_remote_session(id: &str) -> Option<RemoteSessionRecord> {
    close_sessions_where(|session| session.id == id)
        .first()
        .map(|session| session.remote_record())
}

/// Closes sessions past `ssh_manager.idle_timeout_minutes` or
/// `ssh_manager.max_session_hours`; both default to 0 (never).
pub async fn reap_expired_sessions(db: &DatabaseConnection) -> usize {
    let idle_minutes = crate::models::system_settings::number_i64(db, IDLE_TIMEOUT_SETTING, 0)
        .await
        .unwrap_or(0)
        .max(0);
    let max_hours = crate::models::system_settings::number_i64(db, MAX_LIFETIME_SETTING, 0)
        .await
        .unwrap_or(0)
        .max(0);
    if idle_minutes == 0 && max_hours == 0 {
        return 0;
    }
    let now = Utc::now();
    let expired = close_sessions_where(|session| {
        let last_activity = session
            .updated_at
            .lock()
            .map_or(session.created_at, |updated_at| *updated_at);
        expiry_reason(
            session.created_at,
            last_activity,
            session.attached.load(Ordering::SeqCst),
            now,
            idle_minutes,
            max_hours,
        )
        .is_some()
    });
    for session in &expired {
        record_ssh_log(
            db,
            session.user_id,
            None,
            "auto_close",
            format!("SSH 会话超时，已自动关闭：{}", session.target.name),
            None,
            None,
        )
        .await;
    }
    expired.len()
}

/// Background check for expired sessions, once a minute.
pub fn start_session_reaper(db: DatabaseConnection) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(REAPER_INTERVAL).await;
            let closed = reap_expired_sessions(&db).await;
            if closed > 0 {
                tracing::info!(closed, "closed expired ssh sessions");
            }
        }
    });
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

#[cfg(test)]
mod tests {
    use super::{
        expiry_reason, resolve_local_browser_path, simplify_verbatim_path, Duration, FsPath,
        PathBuf, Utc,
    };

    #[test]
    fn sessions_are_kept_unless_a_limit_is_configured() {
        let now = Utc::now();
        let created = now - Duration::hours(30);
        let quiet = now - Duration::hours(2);
        // Defaults: kept until the shell exits or the service stops.
        assert_eq!(expiry_reason(created, quiet, 0, now, 0, 0), None);
        // Idle only counts while no terminal is attached.
        assert_eq!(expiry_reason(created, quiet, 0, now, 60, 0), Some("idle"));
        assert_eq!(expiry_reason(created, quiet, 1, now, 60, 0), None);
        assert_eq!(expiry_reason(created, now, 0, now, 60, 0), None);
        // The lifetime cap applies even while attached.
        assert_eq!(
            expiry_reason(created, now, 1, now, 0, 24),
            Some("max_lifetime")
        );
        assert_eq!(expiry_reason(created, now, 1, now, 0, 48), None);
    }

    #[test]
    fn browser_paths_resolve_relative_and_parent_segments() {
        let current = std::env::temp_dir();
        let resolved =
            resolve_local_browser_path(&current.to_string_lossy(), Some("child/../other/./leaf"))
                .unwrap();
        assert_eq!(resolved, current.join("other").join("leaf"));
        assert_eq!(
            resolve_local_browser_path(&current.to_string_lossy(), None).unwrap(),
            current
        );
        assert!(resolve_local_browser_path("/tmp", Some("bad\npath")).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn browser_paths_keep_windows_drive_prefixes() {
        assert_eq!(
            resolve_local_browser_path(r"C:\Users", Some(r"D:\data\..\logs")).unwrap(),
            PathBuf::from(r"D:\logs")
        );
        assert_eq!(
            resolve_local_browser_path(r"C:\Users", Some(r"..\..\..")).unwrap(),
            PathBuf::from(r"C:\")
        );
        assert_eq!(
            resolve_local_browser_path(r"\\?\C:\Users", Some("pikju")).unwrap(),
            PathBuf::from(r"C:\Users\pikju")
        );
    }

    #[test]
    fn verbatim_windows_paths_are_simplified() {
        assert_eq!(
            simplify_verbatim_path(FsPath::new(r"\\?\C:\Program Files")),
            PathBuf::from(r"C:\Program Files")
        );
        assert_eq!(
            simplify_verbatim_path(FsPath::new(r"\\?\UNC\server\share")),
            PathBuf::from(r"\\server\share")
        );
        assert_eq!(
            simplify_verbatim_path(FsPath::new("/home/app")),
            PathBuf::from("/home/app")
        );
    }
}
