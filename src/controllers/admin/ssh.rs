#![allow(clippy::missing_errors_doc)]

use std::{
    collections::{HashMap, VecDeque},
    env, fs,
    io::{Read, Write},
    path::{Component, Path as FsPath, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    thread,
};

use axum::{
    extract::{ws::Message, ws::WebSocket, ws::WebSocketUpgrade, Path, Query},
    response::IntoResponse,
};
use chrono::{DateTime, Duration, Utc};
use loco_rs::prelude::*;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
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
const OUTPUT_BUFFER_CHUNKS: usize = 500;
const OUTPUT_BROADCAST_CAPACITY: usize = 512;

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
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CreateSshSessionParams {
    pub target_key: String,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
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
    master: Mutex<Box<dyn portable_pty::MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    child: Mutex<Option<Box<dyn portable_pty::Child + Send + Sync>>>,
    output_buffer: Mutex<VecDeque<String>>,
    output_tx: broadcast::Sender<TerminalBroadcastMessage>,
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
    authorize(&ctx, &auth, "system:ssh:list").await?;
    let sessions = session_store()
        .lock()
        .map_err(|_| ApiError::internal("failed to lock ssh session store"))?
        .values()
        .map(|session| session.record())
        .collect::<Vec<_>>();
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
    Json(params): Json<CreateSshSessionParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:ssh:connect").await?;
    let target = find_target(&ctx, &params.target_key).await?;
    let operator = Some(user.name.clone())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| user.email.clone());
    let session = create_managed_session(
        &target,
        user.id,
        params.cols.unwrap_or(120).clamp(20, 300),
        params.rows.unwrap_or(32).clamp(8, 100),
    )?;
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
    Json(params): Json<CreateSshTicketParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:ssh:connect").await?;
    let target = find_target(&ctx, &params.target_key).await?;
    let operator = Some(user.name.clone())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| user.email.clone());
    let session = create_managed_session(
        &target,
        user.id,
        params.cols.unwrap_or(120).clamp(20, 300),
        params.rows.unwrap_or(32).clamp(8, 100),
    )?;
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
    let session = find_session(&id)?;
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
    authorize(&ctx, &auth, "system:ssh:list").await?;
    let session = find_session(&id)?;
    Ok(responses::ok(session.file_browser(params.path.as_deref())?))
}

#[debug_handler]
pub async fn terminal_ws(Path(ticket): Path<String>, ws: WebSocketUpgrade) -> ApiResult<Response> {
    let ticket = consume_ticket(&ticket)?;
    let session = find_session(&ticket.session_id)?;
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
    let session = find_session(&id)?;
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

    fn command(&self) -> CommandBuilder {
        match self {
            Self::Local(_) => {
                let shell = env::var("SHELL")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| "/bin/sh".to_string());
                let mut command = CommandBuilder::new(shell);
                command.arg("-i");
                command.env("TERM", "xterm-256color");
                command
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

    fn current_directory(&self) -> String {
        self.current_directory
            .lock()
            .map_or_else(|_| "~".to_string(), |directory| directory.clone())
    }

    fn locked_value(value: &Mutex<u16>, fallback: u16) -> u16 {
        value.lock().map_or(fallback, |value| *value)
    }

    fn history(&self) -> Vec<String> {
        self.output_buffer
            .lock()
            .map_or_else(|_| Vec::new(), |buffer| buffer.iter().cloned().collect())
    }

    fn append_output(&self, data: String) {
        if let Ok(mut buffer) = self.output_buffer.lock() {
            buffer.push_back(data.clone());
            while buffer.len() > OUTPUT_BUFFER_CHUNKS {
                buffer.pop_front();
            }
        }
        self.touch();
        let _ = self.output_tx.send(TerminalBroadcastMessage {
            message_type: "output".to_string(),
            data,
        });
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
        if let Ok(master) = self.master.lock() {
            let _ = master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
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
        if let Ok(mut writer) = self.writer.lock() {
            let _ = writer.write_all(data);
            let _ = writer.flush();
        }
        self.touch();
    }

    fn close(&self) {
        if let Ok(mut child) = self.child.lock() {
            if let Some(child) = child.as_mut() {
                let _ = child.kill();
            }
            *child = None;
        }
        self.mark_closed();
    }

    fn file_browser(&self, path: Option<&str>) -> ApiResult<SshFileBrowserRecord> {
        match &self.target_kind {
            SshTarget::Local(_) => self.local_file_browser(path),
            SshTarget::Remote(_) => Err(ApiError::bad_request(
                "remote ssh file browser is not supported yet",
            )),
        }
    }

    fn local_file_browser(&self, path: Option<&str>) -> ApiResult<SshFileBrowserRecord> {
        let directory = resolve_local_browser_path(&self.current_directory(), path)?;
        if !directory.is_dir() {
            return Err(ApiError::bad_request("path is not a directory"));
        }
        let normalized = directory
            .canonicalize()
            .map_err(|_| ApiError::bad_request("invalid directory"))?;
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
    normalize_path_components(&candidate)
}

fn normalize_path_components(path: &FsPath) -> ApiResult<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) => return Err(ApiError::bad_request("invalid path")),
            Component::RootDir => normalized.push(component.as_os_str()),
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

fn create_managed_session(
    target: &SshTarget,
    user_id: i32,
    cols: u16,
    rows: u16,
) -> ApiResult<Arc<ManagedSshSession>> {
    let target_record = target.record();
    let current_directory = match target {
        SshTarget::Local(_) => initial_local_directory(),
        SshTarget::Remote(_) => "~".to_string(),
    };
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
        .spawn_command(target.command())
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
        target: target_record,
        target_kind: target.clone(),
        user_id,
        current_directory: Mutex::new(current_directory),
        status: Mutex::new("running".to_string()),
        cols: Mutex::new(cols),
        rows: Mutex::new(rows),
        created_at: Utc::now(),
        updated_at: Mutex::new(Utc::now()),
        master: Mutex::new(pair.master),
        writer: Mutex::new(writer),
        child: Mutex::new(Some(child)),
        output_buffer: Mutex::new(VecDeque::new()),
        output_tx,
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

fn create_attach_ticket(
    session_id: &str,
    user_id: i32,
    operator: String,
) -> ApiResult<SshTicketRecord> {
    find_session(session_id)?;
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

fn find_session(id: &str) -> ApiResult<Arc<ManagedSshSession>> {
    session_store()
        .lock()
        .map_err(|_| ApiError::internal("failed to lock ssh session store"))?
        .get(id)
        .cloned()
        .ok_or_else(|| ApiError::bad_request("ssh session not found"))
}

fn remove_session(id: &str) -> ApiResult<Arc<ManagedSshSession>> {
    session_store()
        .lock()
        .map_err(|_| ApiError::internal("failed to lock ssh session store"))?
        .remove(id)
        .ok_or_else(|| ApiError::bad_request("ssh session not found"))
}

async fn handle_terminal_socket(
    mut socket: WebSocket,
    session: Arc<ManagedSshSession>,
    ticket: SshTicket,
) {
    let mut output_rx = session.output_tx.subscribe();
    let _ = send_status(&mut socket, "connected", "connected").await;
    for output in session.history() {
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
