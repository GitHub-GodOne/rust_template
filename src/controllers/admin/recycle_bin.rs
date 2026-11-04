//! 回收站: list, restore and permanently delete removed records.

#![allow(clippy::missing_errors_doc)]

use loco_rs::prelude::*;
use sea_orm::{ColumnTrait, Condition, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder};
use serde::{Deserialize, Serialize};

use crate::{
    controllers::admin::{ai_images, authorize, require_reauth, static_hosting, SessionExt},
    errors::ApiResult,
    models::{_entities::recycle_bin, admin_logs, users},
    responses::{self, ApiResponse, PageResponse},
    services::recycle_bin::{self as bin, RestoreHook},
};

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct RecycleQueryParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub keyword: Option<String>,
    pub resource: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct RecycleEntryRecord {
    pub id: i32,
    pub resource: String,
    pub resource_label: String,
    pub record_id: String,
    pub title: String,
    /// Rows (or files) held, including cascaded children.
    pub item_count: usize,
    pub deleted_by: Option<i32>,
    pub deleted_by_name: Option<String>,
    pub deleted_at: String,
    /// When automatic cleanup removes it; `None` when kept forever.
    pub expires_at: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct RecycleResourceOption {
    pub key: String,
    pub label: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct RecycleSummary {
    pub retention_days: i64,
    pub resources: Vec<RecycleResourceOption>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct PurgedCount {
    pub purged: usize,
}

/// The registered key for a stored resource name.
fn resource_key(resource: &str) -> &'static str {
    bin::RESOURCES
        .iter()
        .find(|item| item.key == resource)
        .map_or("settings", |item| item.key)
}

fn to_record(entry: recycle_bin::Model, retention_days: i64) -> RecycleEntryRecord {
    let item_count = bin::parse_payload(&entry).map_or(0, |payload| payload.row_count());
    RecycleEntryRecord {
        id: entry.id,
        resource_label: bin::resource_label(&entry.resource).to_string(),
        resource: entry.resource,
        record_id: entry.record_id,
        title: entry.title,
        item_count,
        deleted_by: entry.deleted_by,
        deleted_by_name: entry.deleted_by_name,
        deleted_at: entry.created_at.to_rfc3339(),
        expires_at: (retention_days > 0)
            .then(|| (entry.created_at + chrono::Duration::days(retention_days)).to_rfc3339()),
    }
}

async fn log(ctx: &AppContext, actor: &users::Model, action: &str, message: &str) {
    admin_logs::record(
        &ctx.db,
        admin_logs::LogInput {
            log_type: "operation",
            level: "warn",
            module: "recycle_bin",
            action,
            message,
            user_id: Some(actor.id),
            operator: Some(actor.email.clone()),
            method: None,
            path: Some("/api/admin/recycle-bin"),
            status: Some(200),
            error_message: None,
        },
    )
    .await;
}

#[utoipa::path(
    get,
    path = "/api/admin/recycle-bin",
    tag = "admin-recycle-bin",
    security(("bearer_auth" = [])),
    params(RecycleQueryParams),
    responses((status = 200, body = ApiResponse<PageResponse<RecycleEntryRecord>>))
)]
#[debug_handler]
pub async fn list(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<RecycleQueryParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:recycle:list").await?;
    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(20).clamp(1, 100);
    let mut query = recycle_bin::Entity::find().order_by_desc(recycle_bin::Column::Id);
    if let Some(resource) = params.resource.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(recycle_bin::Column::Resource.eq(resource));
    }
    if let Some(keyword) = params
        .keyword
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        query = query.filter(
            Condition::any()
                .add(recycle_bin::Column::Title.contains(keyword))
                .add(recycle_bin::Column::RecordId.contains(keyword))
                .add(recycle_bin::Column::DeletedByName.contains(keyword)),
        );
    }
    let retention_days = bin::retention_days(&ctx.db).await?;
    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(|entry| to_record(entry, retention_days))
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
    path = "/api/admin/recycle-bin/summary",
    tag = "admin-recycle-bin",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<RecycleSummary>))
)]
#[debug_handler]
pub async fn summary(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:recycle:list").await?;
    Ok(responses::ok(RecycleSummary {
        retention_days: bin::retention_days(&ctx.db).await?,
        resources: bin::RESOURCES
            .iter()
            .map(|resource| RecycleResourceOption {
                key: resource.key.to_string(),
                label: resource.label.to_string(),
            })
            .collect(),
    }))
}

#[utoipa::path(
    post,
    path = "/api/admin/recycle-bin/{id}/restore",
    tag = "admin-recycle-bin",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<RecycleEntryRecord>))
)]
#[debug_handler]
pub async fn restore(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:recycle:restore").await?;
    let (entry, hook) = bin::restore(&ctx.db, id).await?;
    if let RestoreHook::ConfigItem { collection, item } = hook {
        let restored = match collection.as_str() {
            "static_sites" => static_hosting::restore_site(&ctx, &actor, item).await,
            "ai_image_configs" => ai_images::restore_config(&ctx, &actor, item).await,
            _ => Err(crate::errors::ApiError::bad_request(
                "unknown recycle collection",
            )),
        };
        if let Err(error) = restored {
            // Keep the entry so the restore can be retried.
            bin::record(
                &ctx.db,
                bin::Deletion::new(
                    resource_key(&entry.resource),
                    &entry.record_id,
                    &entry.title,
                    &actor,
                ),
                bin::parse_payload(&entry)?,
            )
            .await?;
            return Err(error);
        }
    }
    log(
        &ctx,
        &actor,
        "restore",
        &format!(
            "从回收站恢复{}：{}",
            bin::resource_label(&entry.resource),
            entry.title
        ),
    )
    .await;
    let retention_days = bin::retention_days(&ctx.db).await?;
    Ok(responses::ok(to_record(entry, retention_days)))
}

#[utoipa::path(
    delete,
    path = "/api/admin/recycle-bin/{id}",
    tag = "admin-recycle-bin",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<PurgedCount>))
)]
#[debug_handler]
pub async fn purge(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    session: SessionExt,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:recycle:purge").await?;
    require_reauth(&ctx, &session).await?;
    let entry = bin::purge(&ctx.db, id).await?;
    log(
        &ctx,
        &actor,
        "purge",
        &format!(
            "彻底删除{}：{}",
            bin::resource_label(&entry.resource),
            entry.title
        ),
    )
    .await;
    Ok(responses::ok(PurgedCount { purged: 1 }))
}

#[utoipa::path(
    post,
    path = "/api/admin/recycle-bin/empty",
    tag = "admin-recycle-bin",
    security(("bearer_auth" = [])),
    params(RecycleQueryParams),
    responses((status = 200, body = ApiResponse<PurgedCount>))
)]
#[debug_handler]
pub async fn empty(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    session: SessionExt,
    Query(params): Query<RecycleQueryParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:recycle:purge").await?;
    require_reauth(&ctx, &session).await?;
    let mut query = recycle_bin::Entity::find();
    if let Some(resource) = params.resource.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(recycle_bin::Column::Resource.eq(resource));
    }
    let entries = query.all(&ctx.db).await?;
    let mut purged = 0;
    for entry in entries {
        if bin::purge(&ctx.db, entry.id).await.is_ok() {
            purged += 1;
        }
    }
    log(
        &ctx,
        &actor,
        "empty",
        &format!("清空回收站：彻底删除 {purged} 条"),
    )
    .await;
    Ok(responses::ok(PurgedCount { purged }))
}
