//! Self-service account page: every signed-in user edits their own name and
//! password and manages their own login sessions. No admin permission needed.

#![allow(clippy::missing_errors_doc)]

use loco_rs::{hash, prelude::*};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set};
use serde::{Deserialize, Serialize};

use crate::{
    controllers::admin::SessionExt,
    errors::{ApiError, ApiResult},
    models::{_entities::user_sessions as sessions, admin_logs, user_sessions, users},
    responses::{self, ApiResponse, EmptyData},
};

pub const MIN_PASSWORD_CHARS: usize = 8;

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct ProfileRecord {
    pub id: i32,
    pub pid: String,
    pub name: String,
    pub email: String,
    pub status: String,
    pub is_verified: bool,
    pub last_login_at: Option<String>,
    pub last_login_ip: Option<String>,
    pub password_changed_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct UpdateProfileParams {
    pub name: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct ChangePasswordParams {
    pub current_password: String,
    pub new_password: String,
    /// Also sign out every other device (default true).
    pub sign_out_others: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SessionRecord {
    pub id: i32,
    pub user_id: i32,
    pub user_name: Option<String>,
    pub user_email: Option<String>,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: String,
    pub last_seen_at: String,
    pub expires_at: String,
    pub revoked_at: Option<String>,
    pub revoked_reason: Option<String>,
    pub active: bool,
    /// The session making this request.
    pub current: bool,
}

impl SessionRecord {
    #[must_use]
    pub fn new(
        session: sessions::Model,
        user: Option<&users::Model>,
        current: Option<i32>,
    ) -> Self {
        Self {
            active: user_sessions::is_active(&session),
            current: current == Some(session.id),
            id: session.id,
            user_id: session.user_id,
            user_name: user.map(|user| user.name.clone()),
            user_email: user.map(|user| user.email.clone()),
            ip: session.ip,
            user_agent: session.user_agent,
            created_at: session.created_at.to_rfc3339(),
            last_seen_at: session.last_seen_at.to_rfc3339(),
            expires_at: session.expires_at.to_rfc3339(),
            revoked_at: session.revoked_at.map(|at| at.to_rfc3339()),
            revoked_reason: session.revoked_reason,
        }
    }
}

impl From<users::Model> for ProfileRecord {
    fn from(user: users::Model) -> Self {
        Self {
            id: user.id,
            pid: user.pid.to_string(),
            name: user.name,
            email: user.email,
            status: user.status,
            is_verified: user.email_verified_at.is_some(),
            last_login_at: user.last_login_at.map(|at| at.to_rfc3339()),
            last_login_ip: user.last_login_ip,
            password_changed_at: user.password_changed_at.map(|at| at.to_rfc3339()),
            created_at: user.created_at.to_rfc3339(),
        }
    }
}

/// Shared password rule for self-service changes and admin resets.
pub fn validate_new_password(password: &str) -> ApiResult<()> {
    if password.chars().count() < MIN_PASSWORD_CHARS {
        return Err(ApiError::bad_request(format!(
            "新密码至少需要 {MIN_PASSWORD_CHARS} 个字符"
        )));
    }
    if password.chars().count() > 128 {
        return Err(ApiError::bad_request("新密码不能超过 128 个字符"));
    }
    let has_letter = password.chars().any(char::is_alphabetic);
    let has_digit = password.chars().any(|c| c.is_ascii_digit());
    if !(has_letter && has_digit) {
        return Err(ApiError::bad_request("新密码需要同时包含字母和数字"));
    }
    Ok(())
}

pub fn hash_password(password: &str) -> ApiResult<String> {
    hash::hash_password(password).map_err(|_| ApiError::internal("failed to hash password"))
}

fn current_session_id(session: &SessionExt) -> Option<i32> {
    session.as_ref().map(|axum::Extension(session)| session.id)
}

async fn current_user(ctx: &AppContext, auth: &auth::JWT) -> ApiResult<users::Model> {
    Ok(users::Model::find_by_pid(&ctx.db, &auth.claims.pid).await?)
}

#[utoipa::path(
    get,
    path = "/api/user/profile",
    tag = "user-profile",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<ProfileRecord>))
)]
#[debug_handler]
pub async fn get(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    Ok(responses::ok(ProfileRecord::from(
        current_user(&ctx, &auth).await?,
    )))
}

#[utoipa::path(
    put,
    path = "/api/user/profile",
    tag = "user-profile",
    security(("bearer_auth" = [])),
    request_body = UpdateProfileParams,
    responses((status = 200, body = ApiResponse<ProfileRecord>))
)]
#[debug_handler]
pub async fn update(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<UpdateProfileParams>,
) -> ApiResult<Response> {
    let name = params.name.trim();
    if name.is_empty() || name.chars().count() > 64 {
        return Err(ApiError::bad_request("姓名不能为空且不能超过 64 个字符"));
    }
    let user = current_user(&ctx, &auth).await?;
    let mut active = user.into_active_model();
    active.name = Set(name.to_string());
    Ok(responses::ok(ProfileRecord::from(
        active.update(&ctx.db).await?,
    )))
}

#[utoipa::path(
    post,
    path = "/api/user/profile/password",
    tag = "user-profile",
    security(("bearer_auth" = [])),
    request_body = ChangePasswordParams,
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn change_password(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    session: SessionExt,
    Json(params): Json<ChangePasswordParams>,
) -> ApiResult<Response> {
    let user = current_user(&ctx, &auth).await?;
    if !user.verify_password(&params.current_password) {
        return Err(ApiError::bad_request("当前密码不正确").with_code("WRONG_PASSWORD"));
    }
    if params.current_password == params.new_password {
        return Err(ApiError::bad_request("新密码不能与当前密码相同"));
    }
    validate_new_password(&params.new_password)?;

    let mut active = user.clone().into_active_model();
    active.password = Set(hash_password(&params.new_password)?);
    active.password_changed_at = Set(Some(chrono::Local::now().into()));
    active.update(&ctx.db).await?;
    if params.sign_out_others.unwrap_or(true) {
        user_sessions::revoke_user(
            &ctx.db,
            user.id,
            "password_changed",
            current_session_id(&session),
        )
        .await?;
    }
    admin_logs::record(
        &ctx.db,
        admin_logs::LogInput {
            log_type: "operation",
            level: "info",
            module: "user_preferences",
            action: "change_password",
            message: "修改自己的登录密码",
            user_id: Some(user.id),
            operator: Some(user.email.clone()),
            method: Some("POST"),
            path: Some("/api/user/profile/password"),
            status: Some(200),
            error_message: None,
        },
    )
    .await;
    Ok(responses::empty())
}

#[utoipa::path(
    get,
    path = "/api/user/profile/sessions",
    tag = "user-profile",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<Vec<SessionRecord>>))
)]
#[debug_handler]
pub async fn list_sessions(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    session: SessionExt,
) -> ApiResult<Response> {
    let user = current_user(&ctx, &auth).await?;
    let current = current_session_id(&session);
    let records = sessions::Entity::find()
        .filter(sessions::Column::UserId.eq(user.id))
        .filter(sessions::Column::RevokedAt.is_null())
        .filter(sessions::Column::ExpiresAt.gt(chrono::Local::now()))
        .order_by_desc(sessions::Column::LastSeenAt)
        .all(&ctx.db)
        .await?
        .into_iter()
        .map(|model| SessionRecord::new(model, Some(&user), current))
        .collect::<Vec<_>>();
    Ok(responses::ok(records))
}

#[utoipa::path(
    delete,
    path = "/api/user/profile/sessions/{id}",
    tag = "user-profile",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn revoke_session(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let user = current_user(&ctx, &auth).await?;
    let owned = user_sessions::find_by_id(&ctx.db, id)
        .await?
        .is_some_and(|session| session.user_id == user.id);
    if !owned {
        return Err(ApiError::not_found("session not found"));
    }
    user_sessions::revoke(&ctx.db, id, "self").await?;
    Ok(responses::empty())
}

#[utoipa::path(
    post,
    path = "/api/user/profile/sessions/revoke-others",
    tag = "user-profile",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn revoke_other_sessions(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    session: SessionExt,
) -> ApiResult<Response> {
    let user = current_user(&ctx, &auth).await?;
    user_sessions::revoke_user(&ctx.db, user.id, "self", current_session_id(&session)).await?;
    Ok(responses::empty())
}

#[must_use]
pub fn routes() -> Routes {
    Routes::new()
        .add("/api/user/profile", axum::routing::get(get).put(update))
        .add("/api/user/profile/password", post(change_password))
        .add(
            "/api/user/profile/sessions",
            axum::routing::get(list_sessions),
        )
        .add(
            "/api/user/profile/sessions/revoke-others",
            post(revoke_other_sessions),
        )
        .add("/api/user/profile/sessions/{id}", delete(revoke_session))
}
