#![allow(clippy::missing_errors_doc)]

use loco_rs::prelude::*;
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{
    controllers::admin::authorize,
    errors::{ApiError, ApiResult},
    models::{
        _entities::{database_backups, database_restores, db_connections},
        database_backups::{
            create_postgres_backup_with_options, deliver_backup, restore_postgres_backup,
            telegram_destination_summaries, BackupOptions, BackupTrigger, RestoreOptions,
        },
        db_connections::decrypt_password,
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
    pub database_connection_id: Option<i32>,
    pub database_label: Option<String>,
    pub created_by: Option<i32>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct RestoreBackupParams {
    pub confirm_phrase: String,
}

#[derive(Debug, Default, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CreateBackupParams {
    /// Saved PostgreSQL connection; omitted means the application's database.
    pub connection_id: Option<i32>,
    /// Optional per-backup delivery target ids, e.g. ["telegram"].
    pub delivery_targets: Option<Vec<String>>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct BackupDestinationRecord {
    pub id: String,
    pub name: Option<String>,
    pub enabled: bool,
}

#[utoipa::path(
    get,
    path = "/api/admin/backups/destinations",
    tag = "admin-backups",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<Vec<BackupDestinationRecord>>))
)]
#[debug_handler]
pub async fn destinations(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:backup:list").await?;
    let destinations = telegram_destination_summaries(&ctx.db).await?;
    Ok(responses::ok(
        destinations
            .into_iter()
            .map(|destination| BackupDestinationRecord {
                id: destination.id,
                name: destination.name,
                enabled: destination.enabled,
            })
            .collect::<Vec<_>>(),
    ))
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
    request_body = CreateBackupParams,
    responses((status = 200, body = ApiResponse<BackupRecord>))
)]
#[debug_handler]
pub async fn create(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<CreateBackupParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:backup:create").await?;
    let (database_url, database_connection_id, database_label) =
        resolve_backup_database(&ctx, actor.id, params.connection_id).await?;
    let delivery_targets = normalize_delivery_targets(params.delivery_targets)?;
    let backup = create_postgres_backup_with_options(
        &ctx.db,
        &database_url,
        Some(actor.id),
        BackupTrigger::Manual,
        BackupOptions {
            database_connection_id,
            database_label,
            delivery_targets,
        },
    )
    .await?;
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
    session: crate::controllers::admin::SessionExt,
    Path(id): Path<i32>,
    Json(params): Json<RestoreBackupParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:backup:restore").await?;
    crate::controllers::admin::require_reauth(&ctx, &session).await?;
    let backup = find_backup(&ctx, id).await?;
    let restore = restore_postgres_backup(
        &ctx.db,
        backup,
        Some(actor.id),
        RestoreOptions {
            confirm_phrase: params.confirm_phrase,
            database_url: ctx.config.database.uri.clone(),
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
    session: crate::controllers::admin::SessionExt,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:backup:delete").await?;
    crate::controllers::admin::require_reauth(&ctx, &session).await?;
    let backup = find_backup(&ctx, id).await?;
    crate::services::recycle_bin::delete_row(
        &ctx.db,
        crate::services::recycle_bin::Deletion::new("backups", id, &backup.filename, &actor),
        "database_backups",
        i64::from(id),
        &[],
    )
    .await?;
    Ok(responses::empty())
}

async fn find_backup(ctx: &AppContext, id: i32) -> ApiResult<database_backups::Model> {
    database_backups::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("backup not found"))
}

pub(crate) async fn resolve_backup_database(
    ctx: &AppContext,
    user_id: i32,
    connection_id: Option<i32>,
) -> ApiResult<(String, Option<i32>, Option<String>)> {
    let Some(connection_id) = connection_id else {
        return Ok((ctx.config.database.uri.clone(), None, None));
    };
    let connection = db_connections::Entity::find_by_id(connection_id)
        .filter(db_connections::Column::UserId.eq(user_id))
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("数据库连接不存在或无权访问"))?;
    if !connection.db_type.eq_ignore_ascii_case("postgres") {
        return Err(ApiError::bad_request(
            "数据库备份目前只支持 PostgreSQL 连接",
        ));
    }
    let mut url = Url::parse("postgres://localhost")
        .map_err(|_| ApiError::internal("无法初始化 PostgreSQL 连接地址"))?;
    if let Some(username) = connection.username.as_deref() {
        url.set_username(username)
            .map_err(|_| ApiError::bad_request("数据库用户名无效"))?;
    }
    if let Some(payload) = connection.encrypted_password.as_deref() {
        let secret = ctx.config.get_jwt_config()?.secret.clone();
        let password = decrypt_password(&secret, user_id, connection.id, payload)?;
        url.set_password(Some(&password))
            .map_err(|_| ApiError::bad_request("数据库密码无效"))?;
    }
    url.set_host(connection.host.as_deref().filter(|value| !value.is_empty()))
        .map_err(|_| ApiError::bad_request("数据库主机地址无效"))?;
    if let Some(port) = connection.port.and_then(|value| u16::try_from(value).ok()) {
        url.set_port(Some(port))
            .map_err(|_| ApiError::bad_request("数据库端口无效"))?;
    }
    if let Some(database) = connection
        .database_name
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        url.set_path(&format!("/{database}"));
    }
    if let Some(ssl_mode) = connection
        .ssl_mode
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        url.query_pairs_mut().append_pair("sslmode", ssl_mode);
    }
    Ok((url.to_string(), Some(connection.id), Some(connection.name)))
}

pub(crate) fn normalize_delivery_targets(
    targets: Option<Vec<String>>,
) -> ApiResult<Option<Vec<String>>> {
    let Some(targets) = targets else {
        return Ok(None);
    };
    let mut normalized = Vec::new();
    for target in targets {
        let target = target.trim().to_ascii_lowercase();
        if target.is_empty() {
            continue;
        }
        if !matches!(
            target.as_str(),
            "telegram" | "wecom" | "dingtalk" | "custom"
        ) && !target.starts_with("telegram:")
        {
            return Err(ApiError::bad_request(format!(
                "不支持的备份推送目标：{target}"
            )));
        }
        if !normalized.contains(&target) {
            normalized.push(target);
        }
    }
    Ok(Some(normalized))
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
            database_connection_id: backup.database_connection_id,
            database_label: backup.database_label,
            created_by: backup.created_by,
            created_at: backup.created_at.to_rfc3339(),
            updated_at: backup.updated_at.to_rfc3339(),
        }
    }
}
