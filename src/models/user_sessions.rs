//! Server-side login sessions. Every access token carries its session id
//! (`sid` claim), so revoking a session ends that login immediately instead
//! of waiting for the token to expire.

#![allow(clippy::missing_errors_doc)]

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{Duration as StdDuration, Instant},
};

use chrono::{Duration, Local};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, IntoActiveModel,
    QueryFilter, Set,
};
use uuid::Uuid;

pub use super::_entities::user_sessions::{self, ActiveModel, Entity, Model};
use super::{
    _entities::{refresh_tokens, users},
    system_settings,
};
use crate::services::{client_info::ClientInfo, realtime};

pub const SID_CLAIM: &str = "sid";
/// Access tokens minted for an API key carry this claim instead of `sid`.
pub const API_KEY_CLAIM: &str = "api_key";
pub const USER_ACTIVE: &str = "active";
pub const USER_DISABLED: &str = "disabled";

/// How long a validated session is trusted without asking the database.
const CACHE_TTL: StdDuration = StdDuration::from_secs(15);
/// `last_seen_at` is written at most this often per session.
const TOUCH_INTERVAL: StdDuration = StdDuration::from_secs(60);

/// The session behind the current request, added by the session guard.
#[derive(Debug, Clone)]
pub struct CurrentSession {
    pub id: i32,
    pub sid: String,
    pub user_id: i32,
}

#[derive(Debug, Clone, Copy)]
struct CachedSession {
    id: i32,
    user_id: i32,
    checked_at: Instant,
    touched_at: Instant,
}

static CACHE: OnceLock<Mutex<HashMap<String, CachedSession>>> = OnceLock::new();

fn cache() -> &'static Mutex<HashMap<String, CachedSession>> {
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn forget(predicate: impl Fn(&CachedSession) -> bool) {
    if let Ok(mut cache) = cache().lock() {
        cache.retain(|_, session| !predicate(session));
    }
}

async fn lifetime_days(db: &DatabaseConnection) -> Result<i64, DbErr> {
    Ok(
        system_settings::number_i64(db, "auth.refresh_token_days", 30)
            .await?
            .clamp(1, 365),
    )
}

/// Opens a session for a successful login.
pub async fn start(
    db: &DatabaseConnection,
    user: &users::Model,
    client: &ClientInfo,
) -> Result<Model, DbErr> {
    let now = Local::now();
    ActiveModel {
        sid: Set(Uuid::new_v4().simple().to_string()),
        user_id: Set(user.id),
        ip: Set(client.ip.clone()),
        user_agent: Set(client.user_agent.clone()),
        last_seen_at: Set(now.into()),
        expires_at: Set((now + Duration::days(lifetime_days(db).await?)).into()),
        ..Default::default()
    }
    .insert(db)
    .await
}

/// Pushes the expiry out again when the session's refresh token rotates.
pub async fn extend(db: &DatabaseConnection, session: Model) -> Result<Model, DbErr> {
    let now = Local::now();
    let days = lifetime_days(db).await?;
    let mut active = session.into_active_model();
    active.last_seen_at = Set(now.into());
    active.expires_at = Set((now + Duration::days(days)).into());
    active.update(db).await
}

#[must_use]
pub fn is_active(session: &Model) -> bool {
    session.revoked_at.is_none() && session.expires_at > Local::now()
}

pub async fn find_by_id(db: &DatabaseConnection, id: i32) -> Result<Option<Model>, DbErr> {
    Entity::find_by_id(id).one(db).await
}

/// Resolves the session behind an access token, or `None` when it was
/// revoked, has expired, or belongs to a disabled user.
pub async fn validate(
    db: &DatabaseConnection,
    sid: &str,
    pid: &str,
) -> Result<Option<CurrentSession>, DbErr> {
    let now = Instant::now();
    let cached = cache()
        .lock()
        .ok()
        .and_then(|cache| cache.get(sid).copied());
    if let Some(cached) = cached {
        if now.duration_since(cached.checked_at) < CACHE_TTL {
            if now.duration_since(cached.touched_at) >= TOUCH_INTERVAL {
                touch(db, sid, cached).await;
            }
            return Ok(Some(CurrentSession {
                id: cached.id,
                sid: sid.to_string(),
                user_id: cached.user_id,
            }));
        }
    }

    let Some(session) = Entity::find()
        .filter(user_sessions::Column::Sid.eq(sid))
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    let user = users::Entity::find_by_id(session.user_id).one(db).await?;
    let valid = is_active(&session)
        && user.is_some_and(|user| user.pid.to_string() == pid && user.status != USER_DISABLED);
    if !valid {
        forget(|cached| cached.id == session.id);
        return Ok(None);
    }
    let entry = CachedSession {
        id: session.id,
        user_id: session.user_id,
        checked_at: now,
        touched_at: cached.map_or(now, |cached| cached.touched_at),
    };
    if let Ok(mut cache) = cache().lock() {
        cache.insert(sid.to_string(), entry);
    }
    if now.duration_since(entry.touched_at) >= TOUCH_INTERVAL || cached.is_none() {
        touch(db, sid, entry).await;
    }
    Ok(Some(CurrentSession {
        id: session.id,
        sid: sid.to_string(),
        user_id: session.user_id,
    }))
}

async fn touch(db: &DatabaseConnection, sid: &str, mut entry: CachedSession) {
    entry.touched_at = Instant::now();
    if let Ok(mut cache) = cache().lock() {
        cache.insert(sid.to_string(), entry);
    }
    let result = Entity::update_many()
        .col_expr(
            user_sessions::Column::LastSeenAt,
            sea_orm::sea_query::Expr::value(chrono::DateTime::<chrono::FixedOffset>::from(
                Local::now(),
            )),
        )
        .filter(user_sessions::Column::Id.eq(entry.id))
        .exec(db)
        .await;
    if let Err(error) = result {
        tracing::warn!(
            error = error.to_string(),
            "failed to update session activity"
        );
    }
}

/// Ends one session and its refresh tokens.
pub async fn revoke(db: &DatabaseConnection, session_id: i32, reason: &str) -> Result<bool, DbErr> {
    let Some(session) = find_by_id(db, session_id).await? else {
        return Ok(false);
    };
    forget(|cached| cached.id == session_id);
    if session.revoked_at.is_some() {
        return Ok(false);
    }
    let mut active = session.into_active_model();
    active.revoked_at = Set(Some(Local::now().into()));
    active.revoked_reason = Set(Some(reason.to_string()));
    active.update(db).await?;
    revoke_refresh_tokens(db, refresh_tokens::Column::SessionId.eq(session_id)).await?;
    realtime::publish(
        "session_revoked",
        realtime::Audience::Session(session_id),
        serde_json::json!({ "reason": reason }),
    );
    Ok(true)
}

/// Ends every session of a user, optionally keeping the caller's own.
pub async fn revoke_user(
    db: &DatabaseConnection,
    user_id: i32,
    reason: &str,
    keep_session: Option<i32>,
) -> Result<u64, DbErr> {
    forget(|cached| cached.user_id == user_id && Some(cached.id) != keep_session);
    let mut sessions = Entity::update_many()
        .col_expr(
            user_sessions::Column::RevokedAt,
            sea_orm::sea_query::Expr::value(chrono::DateTime::<chrono::FixedOffset>::from(
                Local::now(),
            )),
        )
        .col_expr(
            user_sessions::Column::RevokedReason,
            sea_orm::sea_query::Expr::value(reason.to_string()),
        )
        .filter(user_sessions::Column::UserId.eq(user_id))
        .filter(user_sessions::Column::RevokedAt.is_null());
    let mut tokens = refresh_tokens::Column::UserId.eq(user_id);
    if let Some(keep) = keep_session {
        sessions = sessions.filter(user_sessions::Column::Id.ne(keep));
        tokens = tokens.and(
            refresh_tokens::Column::SessionId
                .ne(keep)
                .or(refresh_tokens::Column::SessionId.is_null()),
        );
    }
    let revoked = sessions.exec(db).await?.rows_affected;
    revoke_refresh_tokens(db, tokens).await?;
    realtime::publish(
        "session_revoked",
        realtime::Audience::UserSessions {
            user_id,
            except: keep_session,
        },
        serde_json::json!({ "reason": reason }),
    );
    Ok(revoked)
}

async fn revoke_refresh_tokens(
    db: &DatabaseConnection,
    condition: sea_orm::sea_query::SimpleExpr,
) -> Result<(), DbErr> {
    refresh_tokens::Entity::update_many()
        .col_expr(
            refresh_tokens::Column::RevokedAt,
            sea_orm::sea_query::Expr::value(chrono::DateTime::<chrono::FixedOffset>::from(
                Local::now(),
            )),
        )
        .filter(condition)
        .filter(refresh_tokens::Column::RevokedAt.is_null())
        .exec(db)
        .await?;
    Ok(())
}

/// True while a recent password confirmation still covers this session.
#[must_use]
pub fn reauth_fresh(session: &Model, minutes: i64) -> bool {
    session
        .reauth_at
        .is_some_and(|at| at + Duration::minutes(minutes) > Local::now())
}

pub async fn mark_reauth(db: &DatabaseConnection, session_id: i32) -> Result<(), DbErr> {
    if let Some(session) = find_by_id(db, session_id).await? {
        let mut active = session.into_active_model();
        active.reauth_at = Set(Some(Local::now().into()));
        active.update(db).await?;
    }
    Ok(())
}
