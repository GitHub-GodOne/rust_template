//! Browser RDP connections through a private guacd gateway.
#![allow(clippy::missing_errors_doc)]

use crate::{
    controllers::admin::authorize,
    errors::{ApiError, ApiResult},
    models::{
        _entities::system_settings as settings, admin_logs, rdp_credentials, system_settings,
    },
    responses,
};
use axum::{
    extract::{
        ws::{Message, WebSocket},
        Path, WebSocketUpgrade,
    },
    response::IntoResponse,
};
use chrono::{DateTime, Utc};
use futures_util::SinkExt;
use loco_rs::prelude::*;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter, Set};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, OnceLock},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    time::timeout,
};
use uuid::Uuid;

const TARGETS_KEY: &str = "rdp_manager.targets";
const IO_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_INSTRUCTION: usize = 4 * 1024 * 1024;
static TICKETS: OnceLock<Mutex<HashMap<String, Ticket>>> = OnceLock::new();

#[derive(Clone, Deserialize, Serialize, utoipa::ToSchema)]
pub struct RdpTarget {
    pub key: String,
    pub name: String,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub ignore_certificate: bool,
}

const fn default_port() -> u16 {
    3389
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct ConnectParams {
    pub target_key: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub domain: String,
    pub width: Option<u16>,
    pub height: Option<u16>,
    #[serde(default)]
    pub remember_credentials: bool,
    #[serde(default)]
    pub use_saved_credentials: bool,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct CredentialStatus {
    pub saved: bool,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct GatewayStatus {
    pub reachable: bool,
    pub message: String,
}

fn gateway_address() -> String {
    std::env::var("RDP_GUACD_ADDRESS").unwrap_or_else(|_| "127.0.0.1:4822".to_string())
}

fn endpoint(target: &RdpTarget) -> String {
    format!("{}:{}", target.host, target.port)
}

#[utoipa::path(get, path = "/api/admin/rdp/gateway", tag = "admin-rdp", security(("bearer_auth" = [])), responses((status = 200, body = crate::responses::ApiResponse<GatewayStatus>)))]
pub async fn gateway_status(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
) -> ApiResult<impl IntoResponse> {
    authorize(&ctx, &auth, "system:rdp:list").await?;
    let reachable = matches!(
        timeout(
            Duration::from_secs(3),
            TcpStream::connect(gateway_address())
        )
        .await,
        Ok(Ok(_))
    );
    Ok(responses::ok(GatewayStatus { reachable, message: if reachable { "RDP 网关端口可达" } else { "RDP 网关未就绪：请在后端所在机器启动 guacd，或配置 RDP_GUACD_ADDRESS。此问题与 Windows 密码无关。" }.to_string() }))
}

#[utoipa::path(get, path = "/api/admin/rdp/credentials/{key}", tag = "admin-rdp", security(("bearer_auth" = [])), params(("key" = String, Path)), responses((status = 200, body = crate::responses::ApiResponse<CredentialStatus>)))]
pub async fn credential_status(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(key): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let user = authorize(&ctx, &auth, "system:rdp:connect").await?;
    let target = configured_targets(&ctx)
        .await?
        .into_iter()
        .find(|t| t.key == key)
        .ok_or_else(|| ApiError::bad_request("RDP 目标不存在"))?;
    let secret = ctx.config.get_jwt_config()?.secret.clone();
    let saved = rdp_credentials::load(&ctx.db, &secret, user.id, &key, &endpoint(&target))
        .await?
        .is_some();
    Ok(responses::ok(CredentialStatus { saved }))
}

#[utoipa::path(delete, path = "/api/admin/rdp/credentials/{key}", tag = "admin-rdp", security(("bearer_auth" = [])), params(("key" = String, Path)), responses((status = 200, body = crate::responses::ApiResponse<CredentialStatus>)))]
pub async fn delete_credentials(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(key): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let user = authorize(&ctx, &auth, "system:rdp:connect").await?;
    rdp_credentials::delete(&ctx.db, user.id, &key).await?;
    log(&ctx, user.id, "clear_credentials", "清除 RDP 登录凭据").await;
    Ok(responses::ok(CredentialStatus { saved: false }))
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct TicketRecord {
    pub ticket: String,
    pub expires_at: String,
}

struct Ticket {
    user_id: i32,
    target: RdpTarget,
    params: ConnectParams,
    expires_at: DateTime<Utc>,
}

fn tickets() -> &'static Mutex<HashMap<String, Ticket>> {
    TICKETS.get_or_init(|| Mutex::new(HashMap::new()))
}

async fn configured_targets(ctx: &AppContext) -> ApiResult<Vec<RdpTarget>> {
    let value = system_settings::string_value(&ctx.db, TARGETS_KEY, "[]").await?;
    let targets: Vec<RdpTarget> = serde_json::from_str(&value)
        .map_err(|_| ApiError::bad_request("RDP 目标配置不是有效 JSON"))?;
    validate_targets(&targets)?;
    Ok(targets)
}

fn validate_targets(targets: &[RdpTarget]) -> ApiResult<()> {
    let mut keys = HashSet::new();
    if targets.len() > 100 {
        return Err(ApiError::bad_request("最多配置 100 个 RDP 目标"));
    }
    for target in targets {
        if target.key.trim().is_empty()
            || target.name.trim().is_empty()
            || target.host.trim().is_empty()
            || target.port == 0
            || target.host.len() > 255
            || target.key.len() > 100
            || target.name.len() > 200
            || target.host.chars().any(char::is_whitespace)
            || target.host.chars().any(char::is_control)
            || !keys.insert(&target.key)
        {
            return Err(ApiError::bad_request(
                "RDP 目标名称、唯一标识、主机或端口无效",
            ));
        }
    }
    Ok(())
}

#[utoipa::path(get, path = "/api/admin/rdp/targets", tag = "admin-rdp", security(("bearer_auth" = [])), responses((status = 200, body = crate::responses::ApiResponse<Vec<RdpTarget>>)))]
pub async fn targets(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
) -> ApiResult<impl IntoResponse> {
    authorize(&ctx, &auth, "system:rdp:list").await?;
    Ok(responses::ok(configured_targets(&ctx).await?))
}

#[utoipa::path(put, path = "/api/admin/rdp/targets", tag = "admin-rdp", security(("bearer_auth" = [])), request_body = Vec<RdpTarget>, responses((status = 200, body = crate::responses::ApiResponse<Vec<RdpTarget>>)))]
pub async fn save_targets(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(targets): Json<Vec<RdpTarget>>,
) -> ApiResult<impl IntoResponse> {
    let user = authorize(&ctx, &auth, "system:rdp:update").await?;
    validate_targets(&targets)?;
    let existing = settings::Entity::find()
        .filter(settings::Column::Key.eq(TARGETS_KEY))
        .one(&ctx.db)
        .await?;
    let mut model = existing.map_or_else(
        || settings::ActiveModel {
            key: Set(TARGETS_KEY.to_string()),
            name: Set("RDP 远程桌面目标".to_string()),
            group_key: Set("rdp_manager".to_string()),
            value_type: Set("json".to_string()),
            is_public: Set(false),
            is_builtin: Set(true),
            is_encrypted: Set(false),
            sort_order: Set(226),
            created_by: Set(Some(user.id)),
            created_at: Set(Utc::now().into()),
            ..Default::default()
        },
        IntoActiveModel::into_active_model,
    );
    model.value =
        Set(serde_json::to_string(&targets)
            .map_err(|_| ApiError::internal("无法序列化 RDP 目标"))?);
    model.updated_by = Set(Some(user.id));
    model.updated_at = Set(Utc::now().into());
    model.save(&ctx.db).await?;
    log(&ctx, user.id, "update", "更新 RDP 目标配置").await;
    Ok(responses::ok(targets))
}

#[utoipa::path(post, path = "/api/admin/rdp/tickets", tag = "admin-rdp", security(("bearer_auth" = [])), request_body = ConnectParams, responses((status = 200, body = crate::responses::ApiResponse<TicketRecord>)))]
pub async fn create_ticket(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(mut params): Json<ConnectParams>,
) -> ApiResult<impl IntoResponse> {
    let user = authorize(&ctx, &auth, "system:rdp:connect").await?;
    let target = configured_targets(&ctx)
        .await?
        .into_iter()
        .find(|t| t.key == params.target_key && t.enabled)
        .ok_or_else(|| ApiError::bad_request("RDP 目标不存在或已停用"))?;
    let secret = ctx.config.get_jwt_config()?.secret.clone();
    if params.use_saved_credentials {
        let saved =
            rdp_credentials::load(&ctx.db, &secret, user.id, &target.key, &endpoint(&target))
                .await?
                .ok_or_else(|| ApiError::bad_request("没有有效的已保存凭据，请重新输入账号密码"))?;
        params.username = saved.username;
        params.password = saved.password;
        params.domain = saved.domain;
    }
    if params.username.trim().is_empty()
        || params.username.len() > 256
        || params.password.len() > 4096
        || params.domain.len() > 256
    {
        return Err(ApiError::bad_request("RDP 用户名、密码或域无效"));
    }
    if params.remember_credentials && !params.use_saved_credentials {
        rdp_credentials::save(
            &ctx.db,
            &secret,
            user.id,
            &target.key,
            &endpoint(&target),
            &rdp_credentials::Credentials {
                username: params.username.clone(),
                password: params.password.clone(),
                domain: params.domain.clone(),
            },
        )
        .await?;
        log(&ctx, user.id, "save_credentials", "保存 RDP 登录凭据").await;
    }
    let ticket = Uuid::new_v4().to_string();
    let expires_at = Utc::now() + chrono::Duration::seconds(60);
    {
        let mut store = tickets()
            .lock()
            .map_err(|_| ApiError::internal("RDP 票据存储不可用"))?;
        store.retain(|_, t| t.expires_at > Utc::now());
        if store.len() >= 1024 {
            return Err(ApiError::bad_request("待连接的 RDP 请求过多"));
        }
        store.insert(
            ticket.clone(),
            Ticket {
                user_id: user.id,
                target,
                params,
                expires_at,
            },
        );
    }
    let expiry_ticket = ticket.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(60)).await;
        if let Ok(mut store) = tickets().lock() {
            store.remove(&expiry_ticket);
        }
    });
    Ok(responses::ok(TicketRecord {
        ticket,
        expires_at: expires_at.to_rfc3339(),
    }))
}

pub async fn connect(
    State(ctx): State<AppContext>,
    Path(token): Path<String>,
    ws: WebSocketUpgrade,
) -> ApiResult<impl IntoResponse> {
    let ticket = tickets()
        .lock()
        .map_err(|_| ApiError::internal("RDP 票据存储不可用"))?
        .remove(&token)
        .filter(|t| t.expires_at > Utc::now())
        .ok_or_else(|| ApiError::unauthorized("RDP 票据无效或已过期"))?;
    crate::models::rbac::assert_permission(&ctx.db, ticket.user_id, "system:rdp:connect").await?;
    // Re-read the target to honor disable/edit operations since ticket creation.
    let target = configured_targets(&ctx)
        .await?
        .into_iter()
        .find(|t| t.key == ticket.target.key && t.enabled)
        .ok_or_else(|| ApiError::forbidden("RDP 目标已停用"))?;
    if endpoint(&target) != endpoint(&ticket.target) {
        return Err(ApiError::bad_request("RDP 目标地址已变更，请重新连接"));
    }
    let ticket = Ticket { target, ..ticket };
    Ok(ws
        .protocols(["guacamole"])
        .max_message_size(MAX_INSTRUCTION)
        .on_upgrade(move |socket| run(socket, ctx, ticket)))
}

async fn log(ctx: &AppContext, user_id: i32, action: &'static str, message: &str) {
    admin_logs::record(
        &ctx.db,
        admin_logs::LogInput {
            log_type: "operation",
            level: "info",
            module: "rdp",
            action,
            message,
            user_id: Some(user_id),
            operator: None,
            method: None,
            path: None,
            status: None,
            error_message: None,
        },
    )
    .await;
}

fn instruction(parts: &[&str]) -> String {
    let fields: Vec<_> = parts
        .iter()
        .map(|s| format!("{}.{}", s.chars().count(), s))
        .collect();
    format!("{};", fields.join(","))
}

// Read complete instructions, respecting Unicode lengths and separators inside values.
// This also prevents splitting UTF-8 characters across WebSocket text messages.
async fn read_instruction<R: AsyncRead + Unpin>(reader: &mut R) -> std::io::Result<Vec<String>> {
    let invalid =
        || std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid guacd instruction");
    let mut fields = Vec::new();
    let mut total = 0usize;
    loop {
        let mut length = 0usize;
        let mut digits = 0;
        loop {
            let b = reader.read_u8().await?;
            if b == b'.' && digits > 0 {
                break;
            }
            if !b.is_ascii_digit() || digits >= 8 {
                return Err(invalid());
            }
            length = length * 10 + usize::from(b - b'0');
            digits += 1;
        }
        if length > MAX_INSTRUCTION || fields.len() >= 256 {
            return Err(invalid());
        }
        let mut bytes = Vec::new();
        for _ in 0..length {
            let b = reader.read_u8().await?;
            let count = match b {
                0..=127 => 1,
                0xc2..=0xdf => 2,
                0xe0..=0xef => 3,
                0xf0..=0xf4 => 4,
                _ => return Err(invalid()),
            };
            bytes.push(b);
            for _ in 1..count {
                bytes.push(reader.read_u8().await?);
            }
            total += count;
            if total > MAX_INSTRUCTION {
                return Err(invalid());
            }
        }
        fields.push(String::from_utf8(bytes).map_err(|_| invalid())?);
        match reader.read_u8().await? {
            b';' => return Ok(fields),
            b',' => {}
            _ => return Err(invalid()),
        }
    }
}

async fn handshake(ticket: &Ticket) -> ApiResult<BufReader<TcpStream>> {
    // Gateway location is server-controlled and never accepted from the browser.
    let address = gateway_address();
    let stream = TcpStream::connect(address).await.map_err(|_| {
        ApiError::bad_request("无法连接 RDP 网关，请启动 guacd 并检查 RDP_GUACD_ADDRESS")
    })?;
    handshake_stream(ticket, stream).await
}

async fn handshake_stream(ticket: &Ticket, stream: TcpStream) -> ApiResult<BufReader<TcpStream>> {
    let mut reader = BufReader::new(stream);
    write_instruction(&mut reader, &["select", "rdp"]).await?;
    let args = read_instruction(&mut reader)
        .await
        .map_err(|_| ApiError::bad_request("RDP 网关握手失败"))?;
    if args.first().map(String::as_str) != Some("args") {
        return Err(ApiError::bad_request("RDP 网关未返回连接参数"));
    }
    let width = ticket
        .params
        .width
        .unwrap_or(1280)
        .clamp(320, 3840)
        .to_string();
    let height = ticket
        .params
        .height
        .unwrap_or(800)
        .clamp(240, 2160)
        .to_string();
    write_instruction(&mut reader, &["size", &width, &height, "96"]).await?;
    write_instruction(&mut reader, &["audio"]).await?;
    write_instruction(&mut reader, &["video"]).await?;
    write_instruction(&mut reader, &["image", "image/png", "image/jpeg"]).await?;
    let port = ticket.target.port.to_string();
    let mut values = vec!["connect"];
    for arg in args.iter().skip(1) {
        values.push(match arg.as_str() {
            version if version.starts_with("VERSION_") => "VERSION_1_1_0",
            "hostname" => &ticket.target.host,
            "port" => &port,
            "username" => &ticket.params.username,
            "password" => &ticket.params.password,
            "domain" => &ticket.params.domain,
            "security" => "nla",
            "ignore-cert" => {
                if ticket.target.ignore_certificate {
                    "true"
                } else {
                    "false"
                }
            }
            "width" => &width,
            "height" => &height,
            "dpi" => "96",
            "resize-method" => "display-update",
            "disable-audio" => "true",
            "enable-drive" | "enable-printing" | "enable-sftp" => "false",
            _ => "",
        });
    }
    write_instruction(&mut reader, &values).await?;
    let ready = read_instruction(&mut reader)
        .await
        .map_err(|_| ApiError::bad_request("RDP 登录失败，请检查远程桌面、账号和网络"))?;
    if ready.first().map(String::as_str) != Some("ready") {
        return Err(ApiError::bad_request(
            "RDP 登录失败，请检查账号、NLA 和服务器证书配置",
        ));
    }
    Ok(reader)
}

async fn write_instruction(reader: &mut BufReader<TcpStream>, fields: &[&str]) -> ApiResult<()> {
    reader
        .get_mut()
        .write_all(instruction(fields).as_bytes())
        .await
        .map_err(|_| ApiError::bad_request("RDP 网关连接已断开"))
}

async fn run(mut socket: WebSocket, ctx: AppContext, ticket: Ticket) {
    let result = timeout(IO_TIMEOUT, handshake(&ticket)).await;
    let user_id = ticket.user_id;
    let target_name = ticket.target.name.clone();
    // Do not keep login credentials for the duration of the desktop session.
    drop(ticket);
    let reader = match result {
        Ok(Ok(reader)) => reader,
        error => {
            let message = match error {
                Ok(Err(ref e)) => e.message(),
                _ => "RDP 连接超时，请检查网关和目标电脑",
            };
            let _ = socket
                .send(Message::Text(
                    instruction(&["error", message, "519"]).into(),
                ))
                .await;
            let _ = socket.close().await;
            log(
                &ctx,
                user_id,
                "connect_failed",
                &format!("RDP 连接失败：{target_name}"),
            )
            .await;
            return;
        }
    };
    log(
        &ctx,
        user_id,
        "connect",
        &format!("连接 RDP：{target_name}"),
    )
    .await;
    let _ = socket
        .send(Message::Text(
            instruction(&["", &Uuid::new_v4().to_string()]).into(),
        ))
        .await;
    proxy(&mut socket, reader).await;
    let _ = timeout(IO_TIMEOUT, socket.close()).await;
    log(
        &ctx,
        user_id,
        "disconnect",
        &format!("断开 RDP：{target_name}"),
    )
    .await;
}

async fn proxy(socket: &mut WebSocket, mut reader: BufReader<TcpStream>) {
    // A dedicated reader task avoids cancelling partially-read instructions when input arrives.
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let buffered = reader.buffer().to_vec();
    reader.consume(buffered.len());
    let (read_half, mut write_half) = reader.into_inner().into_split();
    let read_task = tokio::spawn(async move {
        let mut input = BufReader::new(std::io::Cursor::new(buffered).chain(read_half));
        while let Ok(parts) = read_instruction(&mut input).await {
            let refs: Vec<_> = parts.iter().map(String::as_str).collect();
            if tx.send(instruction(&refs)).await.is_err() {
                break;
            }
        }
    });
    loop {
        tokio::select! {
            data = rx.recv() => {
                let Some(data) = data else { break; };
                if !matches!(timeout(IO_TIMEOUT, socket.send(Message::Text(data.into()))).await, Ok(Ok(()))) { break; }
            }
            message = timeout(Duration::from_secs(60), socket.recv()) => {
                match message {
                    Ok(Some(Ok(Message::Text(data)))) => {
                        // Internal WebSocket tunnel pings must not be forwarded to guacd.
                        if data.starts_with("0.,") {
                            if !matches!(timeout(IO_TIMEOUT, socket.send(Message::Text(data))).await, Ok(Ok(()))) { break; }
                        } else if !matches!(timeout(IO_TIMEOUT, write_half.write_all(data.as_bytes())).await, Ok(Ok(()))) { break; }
                    }
                    Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => {},
                    _ => break,
                }
            }
        }
    }
    read_task.abort();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn gateway_handshake_maps_requested_parameters_and_preserves_buffered_output() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(stream);
            assert_eq!(
                read_instruction(&mut reader).await.unwrap(),
                vec!["select", "rdp"]
            );
            write_instruction(
                &mut reader,
                &[
                    "args",
                    "VERSION_1_5_0",
                    "password",
                    "hostname",
                    "port",
                    "username",
                    "security",
                    "ignore-cert",
                    "unknown",
                ],
            )
            .await
            .unwrap();
            assert_eq!(
                read_instruction(&mut reader).await.unwrap(),
                vec!["size", "320", "2160", "96"]
            );
            for opcode in ["audio", "video", "image"] {
                assert_eq!(read_instruction(&mut reader).await.unwrap()[0], opcode);
            }
            assert_eq!(
                read_instruction(&mut reader).await.unwrap(),
                vec![
                    "connect",
                    "VERSION_1_1_0",
                    "密;码,🔑",
                    "pc.internal",
                    "3389",
                    "user",
                    "nla",
                    "false",
                    ""
                ]
            );
            reader
                .get_mut()
                .write_all(b"5.ready,2.id;4.sync,3.123;")
                .await
                .unwrap();
        });
        let ticket = Ticket {
            user_id: 1,
            expires_at: Utc::now(),
            target: RdpTarget {
                key: "pc".into(),
                name: "PC".into(),
                host: "pc.internal".into(),
                port: 3389,
                enabled: true,
                ignore_certificate: false,
            },
            params: ConnectParams {
                target_key: "pc".into(),
                username: "user".into(),
                password: "密;码,🔑".into(),
                domain: String::new(),
                width: Some(1),
                height: Some(65535),
                remember_credentials: false,
                use_saved_credentials: false,
            },
        };
        let mut reader = timeout(Duration::from_secs(5), async {
            handshake_stream(&ticket, TcpStream::connect(address).await.unwrap())
                .await
                .unwrap()
        })
        .await
        .unwrap();
        assert_eq!(
            read_instruction(&mut reader).await.unwrap(),
            vec!["sync", "123"]
        );
        server.await.unwrap();
    }
    #[tokio::test]
    async fn protocol_handles_unicode_delimiters_and_multiple_instructions() {
        let wire = format!(
            "{}{}",
            instruction(&["connect", "中文;,", "🔑", ""]),
            instruction(&["sync", "123"])
        );
        let mut reader = std::io::Cursor::new(wire.into_bytes());
        assert_eq!(
            read_instruction(&mut reader).await.unwrap(),
            vec!["connect", "中文;,", "🔑", ""]
        );
        assert_eq!(
            read_instruction(&mut reader).await.unwrap(),
            vec!["sync", "123"]
        );
    }
    #[tokio::test]
    async fn protocol_rejects_invalid_lengths_and_truncated_data() {
        for wire in ["99999999.a;", "x.a;", "1.a!", "3.a;"] {
            assert!(read_instruction(&mut std::io::Cursor::new(wire.as_bytes()))
                .await
                .is_err());
        }
    }
    #[test]
    fn targets_reject_duplicates_and_zero_ports() {
        let mut target = RdpTarget {
            key: "pc".into(),
            name: "PC".into(),
            host: "192.168.1.2".into(),
            port: 3389,
            enabled: true,
            ignore_certificate: false,
        };
        assert!(validate_targets(&[target.clone()]).is_ok());
        assert!(validate_targets(&[target.clone(), target.clone()]).is_err());
        target.port = 0;
        assert!(validate_targets(&[target]).is_err());
    }
}
