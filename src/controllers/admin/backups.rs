#![allow(clippy::missing_errors_doc)]

use loco_rs::prelude::*;
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder};
use serde::{Deserialize, Serialize};

use crate::{
    controllers::admin::authorize,
    errors::{ApiError, ApiResult},
    models::{
        _entities::{database_backups, database_restores},
        database_backups::{
            create_postgres_backup, deliver_backup, restore_postgres_backup, BackupTrigger,
            RestoreOptions,
        },
    },
    responses::{self, ApiResponse, EmptyData, PageResponse},
};

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct BackupQueryParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub status: Option<String>,
    pub trigger_type: Option<String>,
}

impl BackupQueryParams {
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
pub struct BackupRecord {
    pub id: i32,
    pub filename: String,
    pub storage_path: String,
    pub size_bytes: i64,
    pub sha256: Option<String>,
    pub status: String,
    pub trigger_type: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub duration_ms: Option<i32>,
    pub delivery_targets: Option<String>,
    pub delivery_status: Option<String>,
    pub error_message: Option<String>,
    pub created_by: Option<i32>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct RestoreBackupParams {
    pub confirm_phrase: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct RestoreRecord {
    pub id: i32,
    pub backup_id: i32,
    pub status: String,
    pub confirm_phrase: String,
    pub pre_restore_backup_id: Option<i32>,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub duration_ms: Option<i32>,
    pub output: Option<String>,
    pub error_message: Option<String>,
    pub restored_by: Option<i32>,
    pub created_at: String,
    pub updated_at: String,
}

#[utoipa::path(
    get,
    path = "/api/admin/backups",
    tag = "admin-backups",
    security(("bearer_auth" = [])),
    params(BackupQueryParams),
    responses((status = 200, body = ApiResponse<PageResponse<BackupRecord>>))
)]
#[debug_handler]
pub async fn list(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<BackupQueryParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:backup:list").await?;
    let page = params.page();
    let page_size = params.page_size();
    let mut query = database_backups::Entity::find().order_by_desc(database_backups::Column::Id);

    if let Some(status) = params.status.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(database_backups::Column::Status.eq(status));
    }
    if let Some(trigger_type) = params
        .trigger_type
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        query = query.filter(database_backups::Column::TriggerType.eq(trigger_type));
    }

    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(BackupRecord::from)
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
    path = "/api/admin/backups/{id}",
    tag = "admin-backups",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<BackupRecord>))
)]
#[debug_handler]
pub async fn get(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:backup:list").await?;
    let backup = find_backup(&ctx, id).await?;
    Ok(responses::ok(BackupRecord::from(backup)))
}

#[utoipa::path(
    post,
    path = "/api/admin/backups",
    tag = "admin-backups",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<BackupRecord>))
)]
#[debug_handler]
pub async fn create(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:backup:create").await?;
    let backup = create_postgres_backup(&ctx.db, Some(actor.id), BackupTrigger::Manual).await?;
    let backup = deliver_backup(&ctx.db, backup).await?;
    Ok(responses::ok(BackupRecord::from(backup)))
}

#[utoipa::path(
    post,
    path = "/api/admin/backups/{id}/deliver",
    tag = "admin-backups",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<BackupRecord>))
)]
#[debug_handler]
pub async fn deliver(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:backup:deliver").await?;
    let backup = find_backup(&ctx, id).await?;
    let backup = deliver_backup(&ctx.db, backup).await?;

    Ok(responses::ok(BackupRecord::from(backup)))
}

#[utoipa::path(
    get,
    path = "/api/admin/backups/{id}/restores",
    tag = "admin-backups",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<Vec<RestoreRecord>>))
)]
#[debug_handler]
pub async fn list_restores(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:backup:list").await?;
    find_backup(&ctx, id).await?;
    let restores = database_restores::Entity::find()
        .filter(database_restores::Column::BackupId.eq(id))
        .order_by_desc(database_restores::Column::Id)
        .all(&ctx.db)
        .await?
        .into_iter()
        .map(RestoreRecord::from)
        .collect::<Vec<_>>();
    Ok(responses::ok(restores))
}

#[utoipa::path(
    post,
    path = "/api/admin/backups/{id}/restore",
    tag = "admin-backups",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = RestoreBackupParams,
    responses((status = 200, body = ApiResponse<RestoreRecord>))
)]
#[debug_handler]
pub async fn restore(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<RestoreBackupParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:backup:restore").await?;
    let backup = find_backup(&ctx, id).await?;
    let restore = restore_postgres_backup(
        &ctx.db,
        backup,
        Some(actor.id),
        RestoreOptions {
            confirm_phrase: params.confirm_phrase,
        },
    )
    .await?;

    Ok(responses::ok(RestoreRecord::from(restore)))
}

#[utoipa::path(
    delete,
    path = "/api/admin/backups/{id}",
    tag = "admin-backups",
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
    authorize(&ctx, &auth, "system:backup:delete").await?;
    find_backup(&ctx, id).await?;
    database_backups::Entity::delete_by_id(id)
        .exec(&ctx.db)
        .await?;
    Ok(responses::empty())
}

async fn find_backup(ctx: &AppContext, id: i32) -> ApiResult<database_backups::Model> {
    database_backups::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("backup not found"))
}

impl From<database_restores::Model> for RestoreRecord {
    fn from(restore: database_restores::Model) -> Self {
        Self {
            id: restore.id,
            backup_id: restore.backup_id,
            status: restore.status,
            confirm_phrase: restore.confirm_phrase,
            pre_restore_backup_id: restore.pre_restore_backup_id,
            started_at: restore.started_at.to_rfc3339(),
            finished_at: restore.finished_at.map(|value| value.to_rfc3339()),
            duration_ms: restore.duration_ms,
            output: restore.output,
            error_message: restore.error_message,
            restored_by: restore.restored_by,
            created_at: restore.created_at.to_rfc3339(),
            updated_at: restore.updated_at.to_rfc3339(),
        }
    }
}

impl From<database_backups::Model> for BackupRecord {
    fn from(backup: database_backups::Model) -> Self {
        Self {
            id: backup.id,
            filename: backup.filename,
            storage_path: backup.storage_path,
            size_bytes: backup.size_bytes,
            sha256: backup.sha256,
            status: backup.status,
            trigger_type: backup.trigger_type,
            started_at: backup.started_at.to_rfc3339(),
            finished_at: backup.finished_at.map(|value| value.to_rfc3339()),
            duration_ms: backup.duration_ms,
            delivery_targets: backup.delivery_targets,
            delivery_status: backup.delivery_status,
            error_message: backup.error_message,
            created_by: backup.created_by,
            created_at: backup.created_at.to_rfc3339(),
            updated_at: backup.updated_at.to_rfc3339(),
        }
    }
}
