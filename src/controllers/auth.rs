#![allow(clippy::missing_errors_doc)]

use crate::{
    errors::{ApiError, ApiResult},
    mailers::auth::AuthMailer,
    models::{
        _entities::{tenants, users},
        admin_logs, rbac, refresh_tokens,
        users::{LoginParams, RegisterParams},
    },
    responses::{self, ApiResponse, EmptyData},
    views::auth::{CurrentResponse, LoginResponse, RefreshResponse},
};
use loco_rs::prelude::*;
use regex::Regex;
use sea_orm::EntityTrait;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

pub static EMAIL_DOMAIN_RE: OnceLock<Regex> = OnceLock::new();

fn get_allow_email_domain_re() -> &'static Regex {
    EMAIL_DOMAIN_RE.get_or_init(|| {
        Regex::new(r"@example\.com$|@gmail\.com$").expect("Failed to compile regex")
    })
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct ForgotParams {
    pub email: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct ResetParams {
    pub token: String,
    pub password: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct MagicLinkParams {
    pub email: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct ResendVerificationParams {
    pub email: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct RefreshParams {
    pub refresh_token: String,
}

fn jwt_for_user(ctx: &AppContext, user: &users::Model) -> ApiResult<String> {
    let jwt_secret = ctx.config.get_jwt_config()?;
    user.generate_jwt(&jwt_secret.secret, jwt_secret.expiration)
        .map_err(|_| ApiError::unauthorized("unauthorized!"))
}

async fn login_response(ctx: &AppContext, user: &users::Model) -> ApiResult<LoginResponse> {
    let token = jwt_for_user(ctx, user)?;
    let refresh_token = refresh_tokens::Model::issue(&ctx.db, user).await?;
    Ok(LoginResponse::new(user, &token, &refresh_token.token))
}

/// Register function creates a new user with the given parameters and sends a
/// welcome email to the user
#[debug_handler]
pub async fn register(
    State(ctx): State<AppContext>,
    Json(params): Json<RegisterParams>,
) -> ApiResult<Response> {
    let res = users::Model::create_with_password(&ctx.db, &params).await;

    let user = match res {
        Ok(user) => user,
        Err(err) => {
            tracing::info!(
                message = err.to_string(),
                user_email = &params.email,
                "could not register user",
            );
            return Ok(responses::empty());
        }
    };

    let user = user
        .into_active_model()
        .set_email_verification_sent(&ctx.db)
        .await?;

    AuthMailer::send_welcome(&ctx, &user).await?;

    Ok(responses::empty())
}

/// Verify register user. if the user not verified his email, he can't login to
/// the system.
#[debug_handler]
pub async fn verify(
    State(ctx): State<AppContext>,
    Path(token): Path<String>,
) -> ApiResult<Response> {
    let Ok(user) = users::Model::find_by_verification_token(&ctx.db, &token).await else {
        return Err(ApiError::unauthorized("invalid token"));
    };

    if user.email_verified_at.is_some() {
        tracing::info!(pid = user.pid.to_string(), "user already verified");
    } else {
        let active_model = user.into_active_model();
        let user = active_model.verified(&ctx.db).await?;
        tracing::info!(pid = user.pid.to_string(), "user verified");
    }

    Ok(responses::empty())
}

/// In case the user forgot his password this endpoint generates a forgot token
/// and sends email to the user.
#[utoipa::path(
    post,
    path = "/api/auth/forgot",
    tag = "auth",
    request_body = ForgotParams,
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn forgot(
    State(ctx): State<AppContext>,
    Json(params): Json<ForgotParams>,
) -> ApiResult<Response> {
    let Ok(user) = users::Model::find_by_email(&ctx.db, &params.email).await else {
        return Ok(responses::empty());
    };

    let user = user
        .into_active_model()
        .set_forgot_password_sent(&ctx.db)
        .await?;

    AuthMailer::forgot_password(&ctx, &user).await?;

    Ok(responses::empty())
}

/// Reset user password by the given parameters.
#[utoipa::path(
    post,
    path = "/api/auth/reset",
    tag = "auth",
    request_body = ResetParams,
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn reset(
    State(ctx): State<AppContext>,
    Json(params): Json<ResetParams>,
) -> ApiResult<Response> {
    let Ok(user) = users::Model::find_by_reset_token(&ctx.db, &params.token).await else {
        tracing::info!("reset token not found");
        return Ok(responses::empty());
    };

    user.into_active_model()
        .reset_password(&ctx.db, &params.password)
        .await?;

    Ok(responses::empty())
}

/// Creates a user login and returns access and refresh tokens.
#[utoipa::path(
    post,
    path = "/api/auth/login",
    tag = "auth",
    request_body = LoginParams,
    responses(
        (status = 200, body = ApiResponse<LoginResponse>),
        (status = 401, body = ApiResponse<EmptyData>)
    )
)]
#[debug_handler]
pub async fn login(
    State(ctx): State<AppContext>,
    Json(params): Json<LoginParams>,
) -> ApiResult<Response> {
    let Ok(user) = users::Model::find_by_email(&ctx.db, &params.email).await else {
        tracing::debug!(
            email = %params.email,
            "login attempt with non-existent email"
        );
        admin_logs::record(
            &ctx.db,
            admin_logs::LogInput {
                log_type: "login",
                level: "warn",
                module: "auth",
                action: "login_failed",
                message: "登录失败：用户不存在",
                user_id: None,
                operator: Some(params.email.clone()),
                method: Some("POST"),
                path: Some("/api/auth/login"),
                status: Some(401),
                error_message: Some("user not found".to_string()),
            },
        )
        .await;
        return Err(ApiError::unauthorized("Invalid credentials!"));
    };

    if !user.verify_password(&params.password) {
        admin_logs::record(
            &ctx.db,
            admin_logs::LogInput {
                log_type: "login",
                level: "warn",
                module: "auth",
                action: "login_failed",
                message: "登录失败：密码错误",
                user_id: Some(user.id),
                operator: Some(user.email.clone()),
                method: Some("POST"),
                path: Some("/api/auth/login"),
                status: Some(401),
                error_message: Some("invalid password".to_string()),
            },
        )
        .await;
        return Err(ApiError::unauthorized("unauthorized!"));
    }

    let response = login_response(&ctx, &user).await?;
    admin_logs::record(
        &ctx.db,
        admin_logs::LogInput {
            log_type: "login",
            level: "info",
            module: "auth",
            action: "login_success",
            message: "管理员登录后台",
            user_id: Some(user.id),
            operator: Some(user.email.clone()),
            method: Some("POST"),
            path: Some("/api/auth/login"),
            status: Some(200),
            error_message: None,
        },
    )
    .await;

    Ok(responses::ok(response))
}

#[utoipa::path(
    post,
    path = "/api/auth/refresh",
    tag = "auth",
    request_body = RefreshParams,
    responses(
        (status = 200, body = ApiResponse<RefreshResponse>),
        (status = 401, body = ApiResponse<EmptyData>)
    )
)]
#[debug_handler]
pub async fn refresh(
    State(ctx): State<AppContext>,
    Json(params): Json<RefreshParams>,
) -> ApiResult<Response> {
    let refresh_token = refresh_tokens::Model::find_valid_by_token(&ctx.db, &params.refresh_token)
        .await
        .map_err(|_| ApiError::unauthorized("invalid refresh token"))?;

    let user = users::Entity::find_by_id(refresh_token.user_id)
        .one(&ctx.db)
        .await
        .map_err(|err| {
            tracing::error!(error = err.to_string(), "failed to load refresh token user");
            ApiError::internal("model operation failed")
        })?
        .ok_or_else(|| ApiError::unauthorized("invalid refresh token"))?;

    refresh_token.into_active_model().revoke(&ctx.db).await?;
    let issued_refresh_token = refresh_tokens::Model::issue(&ctx.db, &user).await?;
    let token = jwt_for_user(&ctx, &user)?;

    Ok(responses::ok(RefreshResponse::new(
        &token,
        &issued_refresh_token.token,
    )))
}

#[utoipa::path(
    post,
    path = "/api/auth/logout",
    tag = "auth",
    request_body = RefreshParams,
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn logout(
    State(ctx): State<AppContext>,
    Json(params): Json<RefreshParams>,
) -> ApiResult<Response> {
    if let Err(err) = refresh_tokens::Model::revoke(&ctx.db, &params.refresh_token).await {
        tracing::debug!(
            error = err.to_string(),
            "refresh token already invalid during logout"
        );
    }

    Ok(responses::empty())
}

#[utoipa::path(
    get,
    path = "/api/auth/current",
    tag = "auth",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, body = ApiResponse<CurrentResponse>),
        (status = 401, body = ApiResponse<EmptyData>)
    )
)]
#[debug_handler]
pub async fn current(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    let user = users::Model::find_by_pid(&ctx.db, &auth.claims.pid).await?;
    let roles = rbac::load_user_roles(&ctx.db, user.id).await?;
    let permissions = rbac::load_user_permissions(&ctx.db, user.id).await?;
    let menus = rbac::load_user_menus(&ctx.db, user.id).await?;
    let data_scopes = rbac::load_user_data_scopes(&ctx.db, user.id).await?;
    let effective_data_scope = rbac::resolve_data_scope(&ctx.db, &user).await?;
    let tenant = if let Some(tenant_id) = user.tenant_id {
        tenants::Entity::find_by_id(tenant_id).one(&ctx.db).await?
    } else {
        None
    };

    Ok(responses::ok(CurrentResponse::new(
        &user,
        rbac::to_current_roles(roles),
        rbac::permission_codes(permissions),
        menus,
        tenant.map(Into::into),
        data_scopes.into_iter().map(Into::into).collect(),
        effective_data_scope.code().to_string(),
    )))
}

/// Magic link authentication provides a secure and passwordless way to log in to the application.
#[utoipa::path(
    post,
    path = "/api/auth/magic-link",
    tag = "auth",
    request_body = MagicLinkParams,
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
pub async fn magic_link(
    State(ctx): State<AppContext>,
    Json(params): Json<MagicLinkParams>,
) -> ApiResult<Response> {
    let email_regex = get_allow_email_domain_re();
    if !email_regex.is_match(&params.email) {
        tracing::debug!(
            email = params.email,
            "The provided email is invalid or does not match the allowed domains"
        );
        return Err(ApiError::bad_request("invalid request"));
    }

    let Ok(user) = users::Model::find_by_email(&ctx.db, &params.email).await else {
        tracing::debug!(email = params.email, "user not found by email");
        return Ok(responses::empty());
    };

    let user = user.into_active_model().create_magic_link(&ctx.db).await?;
    AuthMailer::send_magic_link(&ctx, &user).await?;

    Ok(responses::empty())
}

/// Verifies a magic link token and authenticates the user.
pub async fn magic_link_verify(
    Path(token): Path<String>,
    State(ctx): State<AppContext>,
) -> ApiResult<Response> {
    let Ok(user) = users::Model::find_by_magic_token(&ctx.db, &token).await else {
        return Err(ApiError::unauthorized("unauthorized!"));
    };

    let user = user.into_active_model().clear_magic_link(&ctx.db).await?;
    Ok(responses::ok(login_response(&ctx, &user).await?))
}

#[utoipa::path(
    post,
    path = "/api/auth/resend-verification-mail",
    tag = "auth",
    request_body = ResendVerificationParams,
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn resend_verification_email(
    State(ctx): State<AppContext>,
    Json(params): Json<ResendVerificationParams>,
) -> ApiResult<Response> {
    let Ok(user) = users::Model::find_by_email(&ctx.db, &params.email).await else {
        tracing::info!(
            email = params.email,
            "User not found for resend verification"
        );
        return Ok(responses::empty());
    };

    if user.email_verified_at.is_some() {
        tracing::info!(
            pid = user.pid.to_string(),
            "User already verified, skipping resend"
        );
        return Ok(responses::empty());
    }

    let user = user
        .into_active_model()
        .set_email_verification_sent(&ctx.db)
        .await?;

    AuthMailer::send_welcome(&ctx, &user).await?;
    tracing::info!(pid = user.pid.to_string(), "Verification email re-sent");

    Ok(responses::empty())
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/auth")
        .add("/register", post(register))
        .add("/verify/{token}", get(verify))
        .add("/login", post(login))
        .add("/refresh", post(refresh))
        .add("/logout", post(logout))
        .add("/forgot", post(forgot))
        .add("/reset", post(reset))
        .add("/current", get(current))
        .add("/magic-link", post(magic_link))
        .add("/magic-link/{token}", get(magic_link_verify))
        .add("/resend-verification-mail", post(resend_verification_email))
}
