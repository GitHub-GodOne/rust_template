#![allow(clippy::missing_errors_doc)]

use chrono::offset::Local;
use loco_rs::prelude::*;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    Set,
};
use serde::{Deserialize, Serialize};

use crate::{
    controllers::admin::authorize,
    errors::{ApiError, ApiResult},
    models::{
        _entities::{system_notifications, users},
        rbac,
    },
    responses::{self, ApiResponse, EmptyData, PageResponse},
};

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct NotificationQueryParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub keyword: Option<String>,
    pub level: Option<String>,
    pub category: Option<String>,
    pub read: Option<bool>,
}

impl NotificationQueryParams {
    #[must_use]
    pub fn page(&self) -> u64 {
        self.page.unwrap_or(1).max(1)
    }

    #[must_use]
    pub fn page_size(&self) -> u64 {
        self.page_size.unwrap_or(10).clamp(1, 100)
    }
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct NotificationRecord {
    pub id: i32,
    pub title: String,
    pub content: String,
    pub level: String,
    pub category: String,
    pub target_type: String,
    pub target_user_id: Option<i32>,
    pub tenant_id: Option<i32>,
    pub read_at: Option<String>,
    pub created_by: Option<i32>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveNotificationParams {
    pub title: String,
    pub content: String,
    pub level: String,
    pub category: String,
    pub target_type: String,
    pub target_user_id: Option<i32>,
    pub tenant_id: Option<i32>,
}

#[utoipa::path(
    get,
    path = "/api/admin/notifications",
    tag = "admin-notifications",
    security(("bearer_auth" = [])),
    params(NotificationQueryParams),
    responses((status = 200, body = ApiResponse<PageResponse<NotificationRecord>>))
)]
#[debug_handler]
pub async fn list(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<NotificationQueryParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:notification:list").await?;
    let page = params.page();
    let page_size = params.page_size();
    let mut query =
        system_notifications::Entity::find().order_by_desc(system_notifications::Column::Id);

    if !rbac::is_super_admin(&ctx.db, actor.id).await? {
        query = query.filter(visible_condition(&actor));
    }
    if let Some(keyword) = params.keyword.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(
            Condition::any()
                .add(system_notifications::Column::Title.contains(keyword))
                .add(system_notifications::Column::Content.contains(keyword)),
        );
    }
    if let Some(level) = params.level.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(system_notifications::Column::Level.eq(level));
    }
    if let Some(category) = params.category.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(system_notifications::Column::Category.eq(category));
    }
    if let Some(read) = params.read {
        query = if read {
            query.filter(system_notifications::Column::ReadAt.is_not_null())
        } else {
            query.filter(system_notifications::Column::ReadAt.is_null())
        };
    }

    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(NotificationRecord::from)
        .collect();

    Ok(responses::ok(PageResponse {
        items,
        page,
        page_size,
        total,
    }))
}

#[utoipa::path(
    get,
    path = "/api/admin/notifications/{id}",
    tag = "admin-notifications",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<NotificationRecord>))
)]
#[debug_handler]
pub async fn get(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:notification:list").await?;
    let notification = find_visible_notification(&ctx, &actor, id).await?;
    Ok(responses::ok(NotificationRecord::from(notification)))
}

#[utoipa::path(
    post,
    path = "/api/admin/notifications",
    tag = "admin-notifications",
    security(("bearer_auth" = [])),
    request_body = SaveNotificationParams,
    responses((status = 200, body = ApiResponse<NotificationRecord>))
)]
#[debug_handler]
pub async fn create(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SaveNotificationParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:notification:create").await?;
    validate_notification(&params)?;

    let notification = system_notifications::ActiveModel {
        title: Set(params.title),
        content: Set(params.content),
        level: Set(params.level),
        category: Set(params.category),
        target_type: Set(params.target_type),
        target_user_id: Set(params.target_user_id),
        tenant_id: Set(params.tenant_id),
        read_at: Set(None),
        created_by: Set(Some(actor.id)),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;

    Ok(responses::ok(NotificationRecord::from(notification)))
}

#[utoipa::path(
    put,
    path = "/api/admin/notifications/{id}/read",
    tag = "admin-notifications",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<NotificationRecord>))
)]
#[debug_handler]
pub async fn mark_read(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:notification:update").await?;
    let notification = find_visible_notification(&ctx, &actor, id).await?;
    let mut active = notification.into_active_model();
    active.read_at = Set(Some(Local::now().into()));
    let notification = active.update(&ctx.db).await?;
    Ok(responses::ok(NotificationRecord::from(notification)))
}

#[utoipa::path(
    delete,
    path = "/api/admin/notifications/{id}",
    tag = "admin-notifications",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn delete(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:notification:delete").await?;
    find_notification(&ctx, id).await?;
    system_notifications::Entity::delete_by_id(id)
        .exec(&ctx.db)
        .await?;
    Ok(responses::empty())
}

async fn find_visible_notification(
    ctx: &AppContext,
    actor: &users::Model,
    id: i32,
) -> ApiResult<system_notifications::Model> {
    let notification = find_notification(ctx, id).await?;
    if rbac::is_super_admin(&ctx.db, actor.id).await? || is_visible(actor, &notification) {
        Ok(notification)
    } else {
        Err(ApiError::forbidden("notification is not visible"))
    }
}

async fn find_notification(ctx: &AppContext, id: i32) -> ApiResult<system_notifications::Model> {
    system_notifications::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("notification not found"))
}

fn visible_condition(actor: &users::Model) -> Condition {
    let mut condition = Condition::any()
        .add(system_notifications::Column::TargetType.eq("all"))
        .add(system_notifications::Column::TargetUserId.eq(actor.id));
    if let Some(tenant_id) = actor.tenant_id {
        condition = condition.add(system_notifications::Column::TenantId.eq(tenant_id));
    }
    condition
}

fn is_visible(actor: &users::Model, notification: &system_notifications::Model) -> bool {
    notification.target_type == "all"
        || notification.target_user_id == Some(actor.id)
        || actor
            .tenant_id
            .is_some_and(|tenant_id| notification.tenant_id == Some(tenant_id))
}

fn validate_notification(params: &SaveNotificationParams) -> ApiResult<()> {
    if params.title.trim().is_empty() || params.content.trim().is_empty() {
        return Err(ApiError::bad_request(
            "notification title and content are required",
        ));
    }
    match params.target_type.as_str() {
        "all" | "user" | "tenant" => Ok(()),
        _ => Err(ApiError::bad_request(
            "unsupported notification target type",
        )),
    }
}

impl From<system_notifications::Model> for NotificationRecord {
    fn from(notification: system_notifications::Model) -> Self {
        Self {
            id: notification.id,
            title: notification.title,
            content: notification.content,
            level: notification.level,
            category: notification.category,
            target_type: notification.target_type,
            target_user_id: notification.target_user_id,
            tenant_id: notification.tenant_id,
            read_at: notification.read_at.map(|value| value.to_rfc3339()),
            created_by: notification.created_by,
            created_at: notification.created_at.to_rfc3339(),
            updated_at: notification.updated_at.to_rfc3339(),
        }
    }
}
