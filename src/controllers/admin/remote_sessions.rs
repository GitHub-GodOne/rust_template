//! 在线会话 → 远程连接: every user's SSH / VNC / RDP sessions, with forced
//! close. Users themselves only ever see their own sessions on those pages.

#![allow(clippy::missing_errors_doc)]

use std::collections::HashMap;

use loco_rs::prelude::*;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};

use crate::{
    controllers::admin::{
        authorize, rdp, remote_audio,
        sessions::{assert_target_visible, record, visible_user_ids},
        ssh, vnc,
    },
    errors::{ApiError, ApiResult},
    models::_entities::users,
    responses::{self, ApiResponse},
};

#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
pub struct RemoteSessionRecord {
    /// `ssh`, `vnc`, `rdp` or `audio` (远程音频).
    pub kind: String,
    pub id: String,
    /// The user who opened the session.
    pub user_id: i32,
    pub user_name: Option<String>,
    pub user_email: Option<String>,
    pub target_key: String,
    pub target_name: String,
    /// `host:port`; `None` for the local shell.
    pub address: Option<String>,
    pub status: String,
    /// Browser windows attached right now.
    pub viewers: usize,
    pub created_at: String,
    pub last_active_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct RemoteSessionQueryParams {
    /// `ssh`, `vnc`, `rdp` or `audio`; every kind when empty.
    pub kind: Option<String>,
    /// Matches user name, email, target name or address.
    pub keyword: Option<String>,
    pub user_id: Option<i32>,
}

fn sessions_of(kind: Option<&str>) -> Vec<RemoteSessionRecord> {
    let mut sessions = Vec::new();
    if kind.is_none_or(|kind| kind == "ssh") {
        sessions.extend(ssh::remote_sessions());
    }
    if kind.is_none_or(|kind| kind == "vnc") {
        sessions.extend(vnc::remote_sessions());
    }
    if kind.is_none_or(|kind| kind == "rdp") {
        sessions.extend(rdp::remote_sessions());
    }
    if kind.is_none_or(|kind| kind == "audio") {
        sessions.extend(remote_audio::remote_sessions());
    }
    sessions
}

fn session_not_found() -> ApiError {
    ApiError::not_found("remote session not found")
}

/// Ends every SSH, VNC, RDP and audio session of a user (disabled or
/// deleted account).
#[must_use = "returns how many sessions were closed"]
pub fn close_user_sessions(user_id: i32) -> usize {
    ssh::close_user_sessions(user_id)
        + vnc::close_user_sessions(user_id)
        + rdp::close_user_sessions(user_id)
        + remote_audio::close_user_sessions(user_id)
}

#[utoipa::path(
    get,
    path = "/api/admin/sessions/remote",
    tag = "admin-sessions",
    security(("bearer_auth" = [])),
    params(RemoteSessionQueryParams),
    responses((status = 200, body = ApiResponse<Vec<RemoteSessionRecord>>))
)]
#[debug_handler]
pub async fn list(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<RemoteSessionQueryParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:session:remote_list").await?;
    let kind = params
        .kind
        .as_deref()
        .map(str::trim)
        .filter(|kind| !kind.is_empty());
    let mut items = sessions_of(kind);
    if let Some(ids) = visible_user_ids(&ctx, &actor).await? {
        items.retain(|item| ids.contains(&item.user_id));
    }
    if let Some(user_id) = params.user_id {
        items.retain(|item| item.user_id == user_id);
    }

    let user_ids: Vec<i32> = items.iter().map(|item| item.user_id).collect();
    let users: HashMap<i32, users::Model> = users::Entity::find()
        .filter(users::Column::Id.is_in(user_ids))
        .all(&ctx.db)
        .await?
        .into_iter()
        .map(|user| (user.id, user))
        .collect();
    for item in &mut items {
        if let Some(user) = users.get(&item.user_id) {
            item.user_name = Some(user.name.clone());
            item.user_email = Some(user.email.clone());
        }
    }

    if let Some(keyword) = params
        .keyword
        .as_deref()
        .map(|keyword| keyword.trim().to_lowercase())
        .filter(|keyword| !keyword.is_empty())
    {
        items.retain(|item| {
            [
                item.user_name.as_deref(),
                item.user_email.as_deref(),
                Some(item.target_name.as_str()),
                item.address.as_deref(),
            ]
            .into_iter()
            .flatten()
            .any(|value| value.to_lowercase().contains(&keyword))
        });
    }
    // RFC 3339 UTC timestamps sort chronologically as strings.
    items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(responses::ok(items))
}

#[utoipa::path(
    delete,
    path = "/api/admin/sessions/remote/{kind}/{id}",
    tag = "admin-sessions",
    security(("bearer_auth" = [])),
    params(("kind" = String, Path), ("id" = String, Path)),
    responses((status = 200, body = ApiResponse<RemoteSessionRecord>))
)]
#[debug_handler]
pub async fn close(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path((kind, id)): Path<(String, String)>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:session:remote_close").await?;
    let session = sessions_of(Some(&kind))
        .into_iter()
        .find(|session| session.id == id)
        .ok_or_else(session_not_found)?;
    let user = assert_target_visible(&ctx, &actor, session.user_id).await?;
    let mut closed = match kind.as_str() {
        "ssh" => ssh::close_remote_session(&id),
        "vnc" => vnc::close_remote_session(&id),
        "rdp" => rdp::close_remote_session(&id),
        "audio" => remote_audio::close_remote_session(&id),
        _ => None,
    }
    .ok_or_else(session_not_found)?;
    record(
        &ctx,
        &actor,
        "close_remote_session",
        &format!(
            "强制断开 {} 连接：{}（{}）",
            kind.to_uppercase(),
            user.email,
            closed.target_name
        ),
    )
    .await;
    closed.user_name = Some(user.name);
    closed.user_email = Some(user.email);
    Ok(responses::ok(closed))
}
