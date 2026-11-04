//! The header bell: the signed-in user's own notifications with per-user
//! read state, and the server-sent event stream that pushes new ones.
//! No admin permission needed.

#![allow(clippy::missing_errors_doc)]

use std::{collections::HashSet, convert::Infallible, time::Duration};

use axum::response::sse::{Event, KeepAlive, Sse};
use loco_rs::prelude::*;
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::{Deserialize, Serialize};

use crate::{
    controllers::admin::{notifications::visible_condition, SessionExt},
    errors::{ApiError, ApiResult},
    models::{
        _entities::{notification_reads, system_notifications},
        user_sessions, users,
    },
    responses::{self, ApiResponse, EmptyData},
    services::realtime::{self, Listener, StreamTicket},
};

/// How often an open stream re-checks that its session is still valid.
const SESSION_CHECK: Duration = Duration::from_secs(30);

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct FeedParams {
    pub limit: Option<u64>,
    pub unread_only: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct FeedItem {
    pub id: i32,
    pub title: String,
    pub content: String,
    pub level: String,
    pub category: String,
    pub created_at: String,
    pub read: bool,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct FeedResponse {
    pub items: Vec<FeedItem>,
    pub unread_count: u64,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct StreamTicketRecord {
    pub ticket: String,
}

#[derive(Debug, Deserialize)]
pub struct StreamParams {
    ticket: String,
}

async fn current_user(ctx: &AppContext, auth: &auth::JWT) -> ApiResult<users::Model> {
    Ok(users::Model::find_by_pid(&ctx.db, &auth.claims.pid).await?)
}

fn read_ids_query(user_id: i32) -> sea_orm::Select<notification_reads::Entity> {
    notification_reads::Entity::find().filter(notification_reads::Column::UserId.eq(user_id))
}

async fn unread_count(ctx: &AppContext, user: &users::Model) -> ApiResult<u64> {
    let read_ids: Vec<i32> = read_ids_query(user.id)
        .select_only()
        .column(notification_reads::Column::NotificationId)
        .into_tuple()
        .all(&ctx.db)
        .await?;
    Ok(system_notifications::Entity::find()
        .filter(visible_condition(user))
        .filter(system_notifications::Column::Id.is_not_in(read_ids))
        .count(&ctx.db)
        .await?)
}

#[utoipa::path(
    get,
    path = "/api/user/notifications",
    tag = "user-notifications",
    security(("bearer_auth" = [])),
    params(FeedParams),
    responses((status = 200, body = ApiResponse<FeedResponse>))
)]
#[debug_handler]
pub async fn feed(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<FeedParams>,
) -> ApiResult<Response> {
    let user = current_user(&ctx, &auth).await?;
    let read_ids: HashSet<i32> = read_ids_query(user.id)
        .select_only()
        .column(notification_reads::Column::NotificationId)
        .into_tuple::<i32>()
        .all(&ctx.db)
        .await?
        .into_iter()
        .collect();
    let mut query = system_notifications::Entity::find()
        .filter(visible_condition(&user))
        .order_by_desc(system_notifications::Column::Id)
        .limit(params.limit.unwrap_or(20).clamp(1, 100));
    if params.unread_only.unwrap_or(false) {
        query = query.filter(system_notifications::Column::Id.is_not_in(read_ids.iter().copied()));
    }
    let items = query
        .all(&ctx.db)
        .await?
        .into_iter()
        .map(|notification| FeedItem {
            read: read_ids.contains(&notification.id),
            id: notification.id,
            title: notification.title,
            content: notification.content,
            level: notification.level,
            category: notification.category,
            created_at: notification.created_at.to_rfc3339(),
        })
        .collect();
    Ok(responses::ok(FeedResponse {
        items,
        unread_count: unread_count(&ctx, &user).await?,
    }))
}

async fn mark(ctx: &AppContext, user: &users::Model, ids: Vec<i32>) -> ApiResult<()> {
    let already: HashSet<i32> = read_ids_query(user.id)
        .filter(notification_reads::Column::NotificationId.is_in(ids.clone()))
        .select_only()
        .column(notification_reads::Column::NotificationId)
        .into_tuple::<i32>()
        .all(&ctx.db)
        .await?
        .into_iter()
        .collect();
    let rows: Vec<notification_reads::ActiveModel> = ids
        .into_iter()
        .filter(|id| !already.contains(id))
        .map(|notification_id| notification_reads::ActiveModel {
            notification_id: sea_orm::Set(notification_id),
            user_id: sea_orm::Set(user.id),
            ..Default::default()
        })
        .collect();
    if !rows.is_empty() {
        notification_reads::Entity::insert_many(rows)
            .exec(&ctx.db)
            .await?;
    }
    Ok(())
}

#[utoipa::path(
    post,
    path = "/api/user/notifications/{id}/read",
    tag = "user-notifications",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn mark_read(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let user = current_user(&ctx, &auth).await?;
    let visible = system_notifications::Entity::find_by_id(id)
        .filter(visible_condition(&user))
        .one(&ctx.db)
        .await?
        .is_some();
    if !visible {
        return Err(ApiError::not_found("notification not found"));
    }
    mark(&ctx, &user, vec![id]).await?;
    Ok(responses::empty())
}

#[utoipa::path(
    post,
    path = "/api/user/notifications/read-all",
    tag = "user-notifications",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn mark_all_read(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    let user = current_user(&ctx, &auth).await?;
    let ids: Vec<i32> = system_notifications::Entity::find()
        .filter(visible_condition(&user))
        .select_only()
        .column(system_notifications::Column::Id)
        .into_tuple()
        .all(&ctx.db)
        .await?;
    mark(&ctx, &user, ids).await?;
    Ok(responses::empty())
}

#[utoipa::path(
    post,
    path = "/api/user/events/ticket",
    tag = "user-notifications",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<StreamTicketRecord>))
)]
#[debug_handler]
pub async fn stream_ticket(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    session: SessionExt,
) -> ApiResult<Response> {
    let user = current_user(&ctx, &auth).await?;
    let Some(axum::Extension(session)) = session else {
        return Err(ApiError::bad_request("API 密钥不能订阅实时通知"));
    };
    let ticket = realtime::issue_ticket(StreamTicket {
        listener: Listener {
            user_id: user.id,
            tenant_id: user.tenant_id,
            session_id: session.id,
        },
        sid: session.sid,
        pid: user.pid.to_string(),
    });
    Ok(responses::ok(StreamTicketRecord { ticket }))
}

enum Step {
    Event(realtime::RealtimeEvent),
    Check,
    Lagged,
    Closed,
}

/// `text/event-stream` of `notification` and `session_revoked` events.
pub async fn stream(
    State(ctx): State<AppContext>,
    Query(params): Query<StreamParams>,
) -> ApiResult<Response> {
    let ticket = realtime::redeem_ticket(&params.ticket)
        .ok_or_else(|| ApiError::unauthorized("实时通知票据无效或已过期"))?;
    let receiver = realtime::subscribe();
    let state = (receiver, ticket, ctx.db, false);
    let events =
        futures_util::stream::unfold(state, |(mut receiver, ticket, db, ended)| async move {
            if ended {
                return None;
            }
            loop {
                let step = tokio::select! {
                    next = receiver.recv() => match next {
                        Ok(event) => Step::Event(event),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => Step::Lagged,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => Step::Closed,
                    },
                    () = tokio::time::sleep(SESSION_CHECK) => Step::Check,
                };
                match step {
                    Step::Event(event) if event.is_for(&ticket.listener) => {
                        let revoked = event.kind == "session_revoked";
                        let sse = Event::default()
                            .event(event.kind)
                            .data(event.data.to_string());
                        return Some((Ok::<_, Infallible>(sse), (receiver, ticket, db, revoked)));
                    }
                    Step::Event(_) | Step::Lagged => {}
                    Step::Check => {
                        let valid = user_sessions::validate(&db, &ticket.sid, &ticket.pid)
                            .await
                            .is_ok_and(|session| session.is_some());
                        if !valid {
                            let sse = Event::default().event("session_revoked").data("{}");
                            return Some((Ok(sse), (receiver, ticket, db, true)));
                        }
                    }
                    Step::Closed => return None,
                }
            }
        });
    Ok(Sse::new(events)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(20)))
        .into_response())
}

/// Publishes a newly created notification to whoever can see it.
pub fn publish_notification(notification: &system_notifications::Model) {
    let audience = match (
        notification.target_type.as_str(),
        notification.target_user_id,
        notification.tenant_id,
    ) {
        ("all", _, _) => realtime::Audience::All,
        (_, Some(user_id), _) => realtime::Audience::User(user_id),
        (_, None, Some(tenant_id)) => realtime::Audience::Tenant(tenant_id),
        _ => return,
    };
    realtime::publish(
        "notification",
        audience,
        serde_json::json!({
            "id": notification.id,
            "title": notification.title,
            "content": notification.content,
            "level": notification.level,
            "category": notification.category,
            "created_at": notification.created_at.to_rfc3339(),
        }),
    );
}

#[must_use]
pub fn routes() -> Routes {
    Routes::new()
        .add("/api/user/notifications", get(feed))
        .add("/api/user/notifications/read-all", post(mark_all_read))
        .add("/api/user/notifications/{id}/read", post(mark_read))
        .add("/api/user/events/ticket", post(stream_ticket))
        .add("/api/user/events", get(stream))
}
