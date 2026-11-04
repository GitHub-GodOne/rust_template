//! 在线会话: every user's login sessions, with forced sign-out.

#![allow(clippy::missing_errors_doc)]

use std::collections::HashMap;

use loco_rs::prelude::*;
use sea_orm::{
    ColumnTrait, Condition, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect,
};
use serde::{Deserialize, Serialize};

use crate::{
    controllers::{
        admin::{authorize, users_admin::apply_user_scope, SessionExt},
        profile::SessionRecord,
    },
    errors::{ApiError, ApiResult},
    models::{
        _entities::{user_sessions as sessions, users},
        admin_logs, rbac, user_sessions,
    },
    responses::{self, ApiResponse, EmptyData, PageResponse},
};

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct SessionQueryParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    /// Matches user name, email or IP.
    pub keyword: Option<String>,
    pub user_id: Option<i32>,
    /// `active` (default), `ended` or `all`.
    pub status: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct RevokedCount {
    pub revoked: u64,
}

/// Users the actor may see under their data scope.
pub(super) async fn visible_user_ids(
    ctx: &AppContext,
    actor: &users::Model,
) -> ApiResult<Option<Vec<i32>>> {
    let scope = rbac::resolve_data_scope(&ctx.db, actor).await?;
    if matches!(scope, rbac::EffectiveDataScope::All) {
        return Ok(None);
    }
    let ids = apply_user_scope(users::Entity::find(), &scope)
        .select_only()
        .column(users::Column::Id)
        .into_tuple::<i32>()
        .all(&ctx.db)
        .await?;
    Ok(Some(ids))
}

#[utoipa::path(
    get,
    path = "/api/admin/sessions",
    tag = "admin-sessions",
    security(("bearer_auth" = [])),
    params(SessionQueryParams),
    responses((status = 200, body = ApiResponse<PageResponse<SessionRecord>>))
)]
#[debug_handler]
pub async fn list(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    session: SessionExt,
    Query(params): Query<SessionQueryParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:session:list").await?;
    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(20).clamp(1, 100);
    let now = chrono::Local::now();

    let mut query = sessions::Entity::find().order_by_desc(sessions::Column::LastSeenAt);
    match params.status.as_deref().unwrap_or("active") {
        "all" => {}
        "ended" => {
            query = query.filter(
                Condition::any()
                    .add(sessions::Column::RevokedAt.is_not_null())
                    .add(sessions::Column::ExpiresAt.lte(now)),
            );
        }
        _ => {
            query = query
                .filter(sessions::Column::RevokedAt.is_null())
                .filter(sessions::Column::ExpiresAt.gt(now));
        }
    }
    if let Some(ids) = visible_user_ids(&ctx, &actor).await? {
        query = query.filter(sessions::Column::UserId.is_in(ids));
    }
    if let Some(user_id) = params.user_id {
        query = query.filter(sessions::Column::UserId.eq(user_id));
    }
    if let Some(keyword) = params
        .keyword
        .as_deref()
        .map(str::trim)
        .filter(|keyword| !keyword.is_empty())
    {
        let matching_users = users::Entity::find()
            .filter(
                Condition::any()
                    .add(users::Column::Name.contains(keyword))
                    .add(users::Column::Email.contains(keyword)),
            )
            .select_only()
            .column(users::Column::Id)
            .into_tuple::<i32>()
            .all(&ctx.db)
            .await?;
        query = query.filter(
            Condition::any()
                .add(sessions::Column::Ip.contains(keyword))
                .add(sessions::Column::UserId.is_in(matching_users)),
        );
    }

    let total = query.clone().count(&ctx.db).await?;
    let models = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?;
    let user_ids: Vec<i32> = models.iter().map(|model| model.user_id).collect();
    let users: HashMap<i32, users::Model> = users::Entity::find()
        .filter(users::Column::Id.is_in(user_ids))
        .all(&ctx.db)
        .await?
        .into_iter()
        .map(|user| (user.id, user))
        .collect();
    let current = session.as_ref().map(|axum::Extension(session)| session.id);
    let items = models
        .into_iter()
        .map(|model| {
            let user = users.get(&model.user_id);
            SessionRecord::new(model, user, current)
        })
        .collect();
    Ok(responses::ok(PageResponse {
        items,
        page,
        page_size,
        total,
    }))
}

pub(super) async fn record(ctx: &AppContext, actor: &users::Model, action: &str, message: &str) {
    admin_logs::record(
        &ctx.db,
        admin_logs::LogInput {
            log_type: "operation",
            level: "warn",
            module: "sessions",
            action,
            message,
            user_id: Some(actor.id),
            operator: Some(actor.email.clone()),
            method: Some("POST"),
            path: Some("/api/admin/sessions"),
            status: Some(200),
            error_message: None,
        },
    )
    .await;
}

pub(super) async fn assert_target_visible(
    ctx: &AppContext,
    actor: &users::Model,
    user_id: i32,
) -> ApiResult<users::Model> {
    let user = users::Entity::find_by_id(user_id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::not_found("user not found"))?;
    if let Some(ids) = visible_user_ids(ctx, actor).await? {
        if !ids.contains(&user_id) {
            return Err(ApiError::forbidden("data scope denied"));
        }
    }
    Ok(user)
}

#[utoipa::path(
    delete,
    path = "/api/admin/sessions/{id}",
    tag = "admin-sessions",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn revoke(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:session:revoke").await?;
    let session = user_sessions::find_by_id(&ctx.db, id)
        .await?
        .ok_or_else(|| ApiError::not_found("session not found"))?;
    let user = assert_target_visible(&ctx, &actor, session.user_id).await?;
    user_sessions::revoke(&ctx.db, id, "admin").await?;
    record(
        &ctx,
        &actor,
        "revoke_session",
        &format!(
            "强制下线会话：{}（{}）",
            user.email,
            session.ip.unwrap_or_default()
        ),
    )
    .await;
    Ok(responses::empty())
}

#[utoipa::path(
    post,
    path = "/api/admin/users/{id}/sessions/revoke",
    tag = "admin-sessions",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<RevokedCount>))
)]
#[debug_handler]
pub async fn revoke_user(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    session: SessionExt,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:session:revoke").await?;
    let user = assert_target_visible(&ctx, &actor, id).await?;
    // Signing yourself out everywhere keeps the session doing it.
    let keep = (actor.id == id)
        .then(|| session.as_ref().map(|axum::Extension(session)| session.id))
        .flatten();
    let revoked = user_sessions::revoke_user(&ctx.db, id, "admin", keep).await?;
    record(
        &ctx,
        &actor,
        "revoke_user_sessions",
        &format!("强制下线用户全部会话：{}（{revoked} 个）", user.email),
    )
    .await;
    Ok(responses::ok(RevokedCount { revoked }))
}
