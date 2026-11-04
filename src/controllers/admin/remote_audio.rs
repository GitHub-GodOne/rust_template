//! 远程音频 (`/api/admin/remote-audio`): listening to what this machine
//! plays, without its screen.
//!
//! A ticket (`POST /tickets`) opens the WebSocket `/ws/{ticket}`, which
//! first sends a `format` message, then binary chunks of 16-bit PCM
//! (`services::audio_capture`) and a `stats` message every few seconds.
//! Every listening session is a `remote_audio_sessions` row that counts the
//! audio bytes sent from opening to closing; live sessions also show in
//! 在线会话 → 远程连接, where admins can end them.

#![allow(clippy::missing_errors_doc)]

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration as StdDuration,
};

use axum::extract::{
    ws::{Message, WebSocket, WebSocketUpgrade},
    Path, Query,
};
use chrono::{DateTime, Duration, Utc};
use loco_rs::prelude::*;
use sea_orm::{
    sea_query::Expr, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect, Select,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use uuid::Uuid;

use crate::{
    controllers::admin::{authorize, remote_sessions::RemoteSessionRecord, sessions},
    errors::{ApiError, ApiResult},
    models::_entities::{remote_audio_sessions, users},
    responses::{self, ApiResponse},
    services::{
        audio_capture::{self, Chunk, Quality, CHUNK_MS, QUALITIES},
        client_info::ClientInfo,
    },
};

pub const LIST_PERMISSION: &str = "system:remote_audio:list";
pub const LISTEN_PERMISSION: &str = "system:remote_audio:listen";
const TICKET_TTL_SECONDS: i64 = 60;
const STATS_INTERVAL: StdDuration = StdDuration::from_secs(2);
/// Stats ticks between saving the counters, so a crash loses little.
const FLUSH_EVERY_TICKS: u32 = 5;
const DEFAULT_QUALITY: &str = "standard";
const LOCAL_TARGET_KEY: &str = "local-audio";

static TICKETS: OnceLock<Mutex<HashMap<String, AudioTicket>>> = OnceLock::new();
static LIVE: OnceLock<Mutex<HashMap<i32, Arc<LiveAudio>>>> = OnceLock::new();

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct AudioQualityRecord {
    /// `high`, `standard` or `low`.
    pub key: String,
    pub sample_rate: u32,
    pub channels: u16,
    /// Bitrate while sound plays; silence costs nothing.
    pub bitrate_kbps: u32,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct AudioStatusRecord {
    pub supported: bool,
    /// `wasapi` (Windows) or `pulseaudio` (Linux).
    pub backend: Option<String>,
    /// Playback device that is captured.
    pub device: Option<String>,
    /// Why capture is unavailable.
    pub message: Option<String>,
    pub qualities: Vec<AudioQualityRecord>,
    pub default_quality: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CreateAudioTicketParams {
    /// One of the `qualities` keys; `standard` when empty.
    pub quality: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct AudioTicketRecord {
    pub ticket: String,
    pub expires_at: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct AudioSessionRecord {
    pub id: i32,
    pub user_id: Option<i32>,
    pub user_name: Option<String>,
    pub user_email: Option<String>,
    pub quality: String,
    pub sample_rate: i32,
    pub channels: i32,
    /// Audio bytes sent from opening to closing (live: so far).
    pub bytes_sent: i64,
    /// Audio actually sent; silence is skipped.
    pub audio_ms: i64,
    /// From opening to closing (live: so far).
    pub duration_ms: i64,
    /// `live`, `ended` or `failed`.
    pub status: String,
    /// `client`, `disconnected`, `admin`, `user_disabled`, `error` or
    /// `server_stopped`.
    pub close_reason: Option<String>,
    pub error_message: Option<String>,
    pub device: Option<String>,
    pub client_ip: Option<String>,
    pub user_agent: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct AudioSessionPage {
    pub items: Vec<AudioSessionRecord>,
    pub page: u64,
    pub page_size: u64,
    pub total: u64,
    /// Bytes of every session matching the filters.
    pub total_bytes: i64,
}

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct AudioSessionQueryParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    /// Matches the listener's name or email.
    pub keyword: Option<String>,
    pub user_id: Option<i32>,
    /// `live`, `ended` or `failed`.
    pub status: Option<String>,
}

#[derive(Debug, Clone)]
struct AudioTicket {
    user_id: i32,
    quality: Quality,
    client: ClientInfo,
    expires_at: DateTime<Utc>,
}

/// A listening session while its WebSocket is open.
struct LiveAudio {
    id: i32,
    user_id: i32,
    quality: Quality,
    device: String,
    started_at: DateTime<Utc>,
    bytes_sent: AtomicU64,
    audio_ms: AtomicU64,
    last_active: Mutex<DateTime<Utc>>,
    close_reason: Mutex<Option<&'static str>>,
    stop: Notify,
}

impl LiveAudio {
    fn stop(&self, reason: &'static str) {
        if let Ok(mut close_reason) = self.close_reason.lock() {
            close_reason.get_or_insert(reason);
        }
        self.stop.notify_one();
    }

    fn remote_record(&self) -> RemoteSessionRecord {
        let last_active = self
            .last_active
            .lock()
            .map_or(self.started_at, |value| *value);
        RemoteSessionRecord {
            kind: "audio".to_string(),
            id: self.id.to_string(),
            user_id: self.user_id,
            user_name: None,
            user_email: None,
            target_key: LOCAL_TARGET_KEY.to_string(),
            target_name: format!("本机音频（{}）", self.quality.key),
            address: Some(self.device.clone()).filter(|device| !device.is_empty()),
            status: "running".to_string(),
            viewers: 1,
            created_at: self.started_at.to_rfc3339(),
            last_active_at: last_active.to_rfc3339(),
        }
    }
}

fn ticket_store() -> &'static Mutex<HashMap<String, AudioTicket>> {
    TICKETS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn live_store() -> &'static Mutex<HashMap<i32, Arc<LiveAudio>>> {
    LIVE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn quality_record(quality: &Quality) -> AudioQualityRecord {
    AudioQualityRecord {
        key: quality.key.to_string(),
        sample_rate: quality.sample_rate,
        channels: quality.channels,
        bitrate_kbps: quality.bitrate_kbps(),
    }
}

#[utoipa::path(
    get,
    path = "/api/admin/remote-audio/status",
    tag = "admin-remote-audio",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<AudioStatusRecord>))
)]
#[debug_handler]
pub async fn status(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    authorize(&ctx, &auth, LIST_PERMISSION).await?;
    let support = tokio::task::spawn_blocking(audio_capture::support)
        .await
        .map_err(|_| ApiError::internal("failed to probe audio capture"))?;
    Ok(responses::ok(AudioStatusRecord {
        supported: support.supported,
        backend: support.backend.map(ToString::to_string),
        device: support.device,
        message: support.message,
        qualities: QUALITIES.iter().map(quality_record).collect(),
        default_quality: DEFAULT_QUALITY.to_string(),
    }))
}

#[utoipa::path(
    post,
    path = "/api/admin/remote-audio/tickets",
    tag = "admin-remote-audio",
    security(("bearer_auth" = [])),
    request_body = CreateAudioTicketParams,
    responses((status = 200, body = ApiResponse<AudioTicketRecord>))
)]
#[debug_handler]
pub async fn create_ticket(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    session: crate::controllers::admin::SessionExt,
    client: ClientInfo,
    Json(params): Json<CreateAudioTicketParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, LISTEN_PERMISSION).await?;
    crate::controllers::admin::require_remote_reauth(&ctx, &session).await?;
    let key = params
        .quality
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .unwrap_or(DEFAULT_QUALITY);
    let quality = audio_capture::quality(key)
        .ok_or_else(|| ApiError::bad_request(format!("不支持的音质：{key}")))?;
    let ticket = Uuid::new_v4().to_string();
    let expires_at = Utc::now() + Duration::seconds(TICKET_TTL_SECONDS);
    let mut store = ticket_store()
        .lock()
        .map_err(|_| ApiError::internal("failed to lock audio ticket store"))?;
    let now = Utc::now();
    store.retain(|_, ticket| ticket.expires_at > now);
    store.insert(
        ticket.clone(),
        AudioTicket {
            user_id: user.id,
            quality,
            client,
            expires_at,
        },
    );
    drop(store);
    Ok(responses::ok(AudioTicketRecord {
        ticket,
        expires_at: expires_at.to_rfc3339(),
    }))
}

fn consume_ticket(ticket: &str) -> ApiResult<AudioTicket> {
    let now = Utc::now();
    let ticket = ticket_store()
        .lock()
        .map_err(|_| ApiError::internal("failed to lock audio ticket store"))?
        .remove(ticket)
        .filter(|ticket| ticket.expires_at > now)
        .ok_or_else(|| ApiError::unauthorized("invalid or expired audio ticket"))?;
    Ok(ticket)
}

#[debug_handler]
pub async fn listen(
    State(ctx): State<AppContext>,
    Path(ticket): Path<String>,
    ws: WebSocketUpgrade,
) -> ApiResult<Response> {
    let ticket = consume_ticket(&ticket)?;
    Ok(ws
        .on_upgrade(move |socket| handle_socket(ctx.db, socket, ticket))
        .into_response())
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMessage<'a> {
    Format {
        session_id: i32,
        sample_rate: u32,
        channels: u16,
        encoding: &'a str,
        chunk_ms: u32,
        device: &'a str,
    },
    Stats {
        bytes_sent: u64,
        audio_ms: u64,
        elapsed_ms: i64,
    },
    Error {
        message: &'a str,
    },
    Closed {
        reason: &'a str,
    },
}

async fn send_message(socket: &mut WebSocket, message: &ServerMessage<'_>) -> bool {
    let Ok(text) = serde_json::to_string(message) else {
        return false;
    };
    socket.send(Message::Text(text.into())).await.is_ok()
}

async fn handle_socket(db: DatabaseConnection, mut socket: WebSocket, ticket: AudioTicket) {
    let started_at = Utc::now();
    let capture = audio_capture::start(ticket.quality).await;
    let device = capture
        .as_ref()
        .map(|capture| capture.device.clone())
        .unwrap_or_default();
    let row = match insert_session(&db, &ticket, started_at, &capture, &device).await {
        Ok(row) => row,
        Err(error) => {
            tracing::warn!(%error, "failed to record remote audio session");
            let message = ServerMessage::Error {
                message: "failed to record the listening session",
            };
            let _ = send_message(&mut socket, &message).await;
            return;
        }
    };
    let mut capture = match capture {
        Ok(capture) => capture,
        Err(error) => {
            let _ = send_message(&mut socket, &ServerMessage::Error { message: &error }).await;
            let _ = socket.send(Message::Close(None)).await;
            return;
        }
    };

    let live = Arc::new(LiveAudio {
        id: row.id,
        user_id: ticket.user_id,
        quality: ticket.quality,
        device: device.clone(),
        started_at,
        bytes_sent: AtomicU64::new(0),
        audio_ms: AtomicU64::new(0),
        last_active: Mutex::new(started_at),
        close_reason: Mutex::new(None),
        stop: Notify::new(),
    });
    if let Ok(mut store) = live_store().lock() {
        store.insert(row.id, Arc::clone(&live));
    }
    let (reason, error) = stream(&db, &mut socket, &mut capture, &live, &device).await;
    drop(capture);
    if let Ok(mut store) = live_store().lock() {
        store.remove(&row.id);
    }
    // An admin or a disabled account ended it; the listener is told why.
    let reason = live
        .close_reason
        .lock()
        .ok()
        .and_then(|value| *value)
        .unwrap_or(reason);
    if matches!(reason, "admin" | "user_disabled") {
        let _ = send_message(&mut socket, &ServerMessage::Closed { reason }).await;
        let _ = socket.send(Message::Close(None)).await;
    }
    finish_session(&db, &live, reason, error).await;
}

/// The session row, `live` or (when capture did not start) `failed`.
async fn insert_session(
    db: &DatabaseConnection,
    ticket: &AudioTicket,
    started_at: DateTime<Utc>,
    capture: &Result<audio_capture::Capture, String>,
    device: &str,
) -> Result<remote_audio_sessions::Model, sea_orm::DbErr> {
    let failed = capture.as_ref().err();
    remote_audio_sessions::ActiveModel {
        quality: Set(ticket.quality.key.to_string()),
        sample_rate: Set(i32::try_from(ticket.quality.sample_rate).unwrap_or(i32::MAX)),
        channels: Set(i32::from(ticket.quality.channels)),
        bytes_sent: Set(0),
        audio_ms: Set(0),
        status: Set(if failed.is_some() { "failed" } else { "live" }.to_string()),
        close_reason: Set(failed.map(|_| "error".to_string())),
        error_message: Set(failed.cloned()),
        device: Set(Some(device.to_string()).filter(|device| !device.is_empty())),
        client_ip: Set(ticket.client.ip.clone()),
        user_agent: Set(ticket.client.user_agent.clone()),
        started_at: Set(started_at.into()),
        ended_at: Set(failed.map(|_| started_at.into())),
        user_id: Set(Some(ticket.user_id)),
        ..Default::default()
    }
    .insert(db)
    .await
}

async fn finish_session(
    db: &DatabaseConnection,
    live: &LiveAudio,
    reason: &str,
    error: Option<String>,
) {
    let ended_at = Utc::now();
    let update = remote_audio_sessions::ActiveModel {
        id: Set(live.id),
        bytes_sent: Set(counter(&live.bytes_sent)),
        audio_ms: Set(counter(&live.audio_ms)),
        status: Set("ended".to_string()),
        close_reason: Set(Some(reason.to_string())),
        error_message: Set(error),
        ended_at: Set(Some(ended_at.into())),
        updated_at: Set(ended_at.into()),
        ..Default::default()
    }
    .update(db)
    .await;
    if let Err(error) = update {
        tracing::warn!(%error, session_id = live.id, "failed to finish remote audio session");
    }
}

fn counter(value: &AtomicU64) -> i64 {
    i64::try_from(value.load(Ordering::SeqCst)).unwrap_or(i64::MAX)
}

/// Forwards audio until either side ends; returns the close reason and an
/// error, if any.
async fn stream(
    db: &DatabaseConnection,
    socket: &mut WebSocket,
    capture: &mut audio_capture::Capture,
    live: &LiveAudio,
    device: &str,
) -> (&'static str, Option<String>) {
    let format = ServerMessage::Format {
        session_id: live.id,
        sample_rate: live.quality.sample_rate,
        channels: live.quality.channels,
        encoding: "pcm_s16le",
        chunk_ms: CHUNK_MS,
        device,
    };
    if !send_message(socket, &format).await {
        return ("disconnected", None);
    }
    let mut stats = tokio::time::interval(STATS_INTERVAL);
    stats.tick().await;
    let mut ticks = 0_u32;
    loop {
        tokio::select! {
            chunk = capture.chunks.recv() => match chunk {
                Some(Chunk::Pcm(samples)) => {
                    let bytes: Vec<u8> = samples.iter().flat_map(|sample| sample.to_le_bytes()).collect();
                    let size = bytes.len() as u64;
                    if socket.send(Message::Binary(bytes.into())).await.is_err() {
                        return ("disconnected", None);
                    }
                    live.bytes_sent.fetch_add(size, Ordering::SeqCst);
                    live.audio_ms.fetch_add(u64::from(CHUNK_MS), Ordering::SeqCst);
                    if let Ok(mut last_active) = live.last_active.lock() {
                        *last_active = Utc::now();
                    }
                }
                Some(Chunk::Error(error)) => {
                    let _ = send_message(socket, &ServerMessage::Error { message: &error }).await;
                    return ("error", Some(error));
                }
                None => return ("error", Some("audio capture stopped".to_string())),
            },
            message = socket.recv() => match message {
                Some(Ok(Message::Close(_)) | Err(_)) | None => return ("client", None),
                Some(Ok(_)) => {}
            },
            _ = stats.tick() => {
                let message = ServerMessage::Stats {
                    bytes_sent: live.bytes_sent.load(Ordering::SeqCst),
                    audio_ms: live.audio_ms.load(Ordering::SeqCst),
                    elapsed_ms: (Utc::now() - live.started_at).num_milliseconds(),
                };
                if !send_message(socket, &message).await {
                    return ("disconnected", None);
                }
                ticks += 1;
                if ticks % FLUSH_EVERY_TICKS == 0 {
                    flush(db, live).await;
                }
            }
            () = live.stop.notified() => return ("admin", None),
        }
    }
}

async fn flush(db: &DatabaseConnection, live: &LiveAudio) {
    let _ = remote_audio_sessions::ActiveModel {
        id: Set(live.id),
        bytes_sent: Set(counter(&live.bytes_sent)),
        audio_ms: Set(counter(&live.audio_ms)),
        updated_at: Set(Utc::now().into()),
        ..Default::default()
    }
    .update(db)
    .await;
}

#[utoipa::path(
    get,
    path = "/api/admin/remote-audio/sessions",
    tag = "admin-remote-audio",
    security(("bearer_auth" = [])),
    params(AudioSessionQueryParams),
    responses((status = 200, body = ApiResponse<AudioSessionPage>))
)]
#[debug_handler]
pub async fn list_sessions(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<AudioSessionQueryParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, LIST_PERMISSION).await?;
    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(10).clamp(1, 100);
    let query = filtered_query(&ctx, &actor, &params).await?;

    let total = query.clone().count(&ctx.db).await?;
    let total_bytes = query
        .clone()
        .select_only()
        .expr(Expr::cust("COALESCE(SUM(bytes_sent), 0)::bigint"))
        .into_tuple::<i64>()
        .one(&ctx.db)
        .await?
        .unwrap_or(0);
    let rows = query
        .order_by_desc(remote_audio_sessions::Column::StartedAt)
        .order_by_desc(remote_audio_sessions::Column::Id)
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?;

    let user_ids: Vec<i32> = rows.iter().filter_map(|row| row.user_id).collect();
    let users: HashMap<i32, users::Model> = users::Entity::find()
        .filter(users::Column::Id.is_in(user_ids))
        .all(&ctx.db)
        .await?
        .into_iter()
        .map(|user| (user.id, user))
        .collect();
    let live: HashMap<i32, Arc<LiveAudio>> = live_store()
        .lock()
        .map(|store| store.clone())
        .unwrap_or_default();
    let now = Utc::now();
    let items = rows
        .into_iter()
        .map(|row| {
            let user = row.user_id.and_then(|id| users.get(&id));
            let current = live.get(&row.id);
            let started_at = row.started_at.with_timezone(&Utc);
            let ended_at = row.ended_at.map(|value| value.with_timezone(&Utc));
            AudioSessionRecord {
                id: row.id,
                user_id: row.user_id,
                user_name: user.map(|user| user.name.clone()),
                user_email: user.map(|user| user.email.clone()),
                quality: row.quality,
                sample_rate: row.sample_rate,
                channels: row.channels,
                bytes_sent: current.map_or(row.bytes_sent, |live| counter(&live.bytes_sent)),
                audio_ms: current.map_or(row.audio_ms, |live| counter(&live.audio_ms)),
                duration_ms: (ended_at.unwrap_or(now) - started_at).num_milliseconds(),
                status: row.status,
                close_reason: row.close_reason,
                error_message: row.error_message,
                device: row.device,
                client_ip: row.client_ip,
                user_agent: row.user_agent,
                started_at: started_at.to_rfc3339(),
                ended_at: ended_at.map(|value| value.to_rfc3339()),
            }
        })
        .collect();
    Ok(responses::ok(AudioSessionPage {
        items,
        page,
        page_size,
        total,
        total_bytes,
    }))
}

async fn filtered_query(
    ctx: &AppContext,
    actor: &users::Model,
    params: &AudioSessionQueryParams,
) -> ApiResult<Select<remote_audio_sessions::Entity>> {
    let mut query = remote_audio_sessions::Entity::find();
    if let Some(ids) = sessions::visible_user_ids(ctx, actor).await? {
        query = query.filter(remote_audio_sessions::Column::UserId.is_in(ids));
    }
    if let Some(user_id) = params.user_id {
        query = query.filter(remote_audio_sessions::Column::UserId.eq(user_id));
    }
    if let Some(status) = params
        .status
        .as_deref()
        .map(str::trim)
        .filter(|status| !status.is_empty())
    {
        query = query.filter(remote_audio_sessions::Column::Status.eq(status));
    }
    if let Some(keyword) = params
        .keyword
        .as_deref()
        .map(str::trim)
        .filter(|keyword| !keyword.is_empty())
    {
        let pattern = format!("%{keyword}%");
        let ids: Vec<i32> = users::Entity::find()
            .filter(
                users::Column::Name
                    .like(pattern.clone())
                    .or(users::Column::Email.like(pattern)),
            )
            .select_only()
            .column(users::Column::Id)
            .into_tuple()
            .all(&ctx.db)
            .await?;
        query = query.filter(remote_audio_sessions::Column::UserId.is_in(ids));
    }
    Ok(query)
}

/// Marks sessions a stopped server left open as ended (at their last save).
pub async fn finish_stale_sessions(db: &DatabaseConnection) {
    let result = remote_audio_sessions::Entity::update_many()
        .col_expr(remote_audio_sessions::Column::Status, Expr::value("ended"))
        .col_expr(
            remote_audio_sessions::Column::CloseReason,
            Expr::value("server_stopped"),
        )
        .col_expr(
            remote_audio_sessions::Column::EndedAt,
            Expr::col(remote_audio_sessions::Column::UpdatedAt).into(),
        )
        .filter(remote_audio_sessions::Column::Status.eq("live"))
        .exec(db)
        .await;
    if let Err(error) = result {
        tracing::warn!(%error, "failed to finish stale remote audio sessions");
    }
}

fn stop_where(predicate: impl Fn(&LiveAudio) -> bool, reason: &'static str) -> Vec<Arc<LiveAudio>> {
    let stopped: Vec<Arc<LiveAudio>> = live_store()
        .lock()
        .map(|store| {
            store
                .values()
                .filter(|live| predicate(live))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    for live in &stopped {
        live.stop(reason);
    }
    stopped
}

/// Ends every listening session of a user (disabled or deleted account).
#[must_use = "returns how many sessions were closed"]
pub fn close_user_sessions(user_id: i32) -> usize {
    stop_where(|live| live.user_id == user_id, "user_disabled").len()
}

/// Every user's live listening sessions, for the 在线会话 admin page.
#[must_use]
pub fn remote_sessions() -> Vec<RemoteSessionRecord> {
    live_store().lock().map_or_else(
        |_| Vec::new(),
        |store| store.values().map(|live| live.remote_record()).collect(),
    )
}

/// Ends any user's listening session.
#[must_use = "returns the closed session, if it existed"]
pub fn close_remote_session(id: &str) -> Option<RemoteSessionRecord> {
    let id: i32 = id.parse().ok()?;
    stop_where(|live| live.id == id, "admin")
        .first()
        .map(|live| live.remote_record())
}
