//! 数据库管理 (`/api/admin/db-manager`): saved database connections.
//!
//! PostgreSQL, `MySQL` and `SQLite` connections get an object browser, a
//! table data grid with filters and edits, and a SQL editor with history and
//! saved queries.
//!
//! Connections belong to the user who created them; others get
//! `DB_CONNECTION_NOT_FOUND`. Opening a connection asks for the password
//! confirmation like SSH/VNC/RDP do (`security.remote_reauth_enabled`).
#![allow(clippy::missing_errors_doc)]

use std::time::Duration;

use axum::{body::Body, http::header};
use chrono::{Local, Utc};
use loco_rs::prelude::*;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect, Set,
};
use serde::{Deserialize, Serialize};

use crate::{
    controllers::admin::{authorize, require_remote_reauth, SessionExt},
    errors::{ApiError, ApiResult},
    models::{
        _entities::{db_connections, db_query_logs, db_saved_queries},
        db_connections::{decrypt_password, encrypt_password},
        rbac, users,
    },
    responses::{self, ApiResponse, EmptyData, PageResponse},
    services::db_manager::{
        self,
        driver::ConnectConfig,
        meta::{DbObject, TableStructure},
        sql::Dialect,
        ChangeSummary, ExecuteOptions, ExecuteResult, ExportFormat, FilterSpec, RowChange,
        RowsPage, RowsQuery, SortSpec, Target,
    },
};

const LIST_PERMISSION: &str = "system:database:list";
const MANAGE_PERMISSION: &str = "system:database:manage";
const QUERY_PERMISSION: &str = "system:database:query";
const HISTORY_KEEP: u64 = 500;
const HISTORY_SQL_LIMIT: usize = 100_000;
const PG_SSL_MODES: &[&str] = &[
    "disable",
    "allow",
    "prefer",
    "require",
    "verify-ca",
    "verify-full",
];
const MYSQL_SSL_MODES: &[&str] = &[
    "disabled",
    "preferred",
    "required",
    "verify_ca",
    "verify_identity",
];

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DbConnectionRecord {
    pub id: i32,
    pub name: String,
    /// postgres | mysql | sqlite
    pub db_type: String,
    pub host: Option<String>,
    pub port: Option<i32>,
    pub username: Option<String>,
    /// Default database; the file path for `SQLite`.
    pub database_name: Option<String>,
    pub ssl_mode: Option<String>,
    pub read_only: bool,
    pub group_name: Option<String>,
    pub color: Option<String>,
    pub remark: Option<String>,
    pub sort_order: i32,
    pub password_saved: bool,
    pub last_connected_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<db_connections::Model> for DbConnectionRecord {
    fn from(model: db_connections::Model) -> Self {
        Self {
            id: model.id,
            name: model.name,
            db_type: model.db_type,
            host: model.host,
            port: model.port,
            username: model.username,
            database_name: model.database_name,
            ssl_mode: model.ssl_mode,
            read_only: model.read_only,
            group_name: model.group_name,
            color: model.color,
            remark: model.remark,
            sort_order: model.sort_order,
            password_saved: model.encrypted_password.is_some(),
            last_connected_at: model.last_connected_at.map(|value| value.to_rfc3339()),
            created_at: model.created_at.to_rfc3339(),
            updated_at: model.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveDbConnectionParams {
    /// Only for 测试连接: an existing connection whose saved password is used
    /// when `password` is empty.
    pub id: Option<i32>,
    pub name: String,
    pub db_type: String,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub username: Option<String>,
    /// Empty keeps the saved password when editing.
    pub password: Option<String>,
    /// Removes the saved password.
    pub clear_password: Option<bool>,
    pub database_name: Option<String>,
    pub ssl_mode: Option<String>,
    pub read_only: Option<bool>,
    pub group_name: Option<String>,
    pub color: Option<String>,
    pub remark: Option<String>,
    pub sort_order: Option<i32>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DbTestRecord {
    pub version: String,
    pub duration_ms: u64,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DbConnectRecord {
    pub connection_id: i32,
    pub db_type: String,
    pub version: String,
    pub databases: Vec<String>,
    pub default_database: String,
    /// Whether objects are grouped by schema (PostgreSQL).
    pub has_schemas: bool,
    pub read_only: bool,
}

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct DbLocationParams {
    pub database: Option<String>,
    pub schema: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct DbTableParams {
    pub database: Option<String>,
    pub schema: Option<String>,
    pub table: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct DbRoutineParams {
    pub database: Option<String>,
    pub schema: Option<String>,
    pub name: String,
    /// function | procedure
    pub kind: String,
    /// PostgreSQL: the function oid from the object list.
    pub identity: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DbDdlRecord {
    pub ddl: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct DbRowsParams {
    pub database: Option<String>,
    pub schema: Option<String>,
    pub table: String,
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    /// JSON `[{"column": "id", "desc": true}]`.
    pub sort: Option<String>,
    /// JSON `[{"column": "name", "op": "contains", "value": "a"}]`.
    pub filters: Option<String>,
    /// A hand-written condition; needs `system:database:query`.
    #[serde(rename = "where")]
    #[param(rename = "where")]
    pub where_clause: Option<String>,
    /// Export only: csv | json | sql.
    pub format: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct DbChangesParams {
    pub database: Option<String>,
    pub schema: Option<String>,
    pub table: String,
    pub changes: Vec<RowChange>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct DbExecuteParams {
    pub database: Option<String>,
    /// Identifies the editor tab; each tab keeps its own connection.
    pub session_key: String,
    pub sql: String,
    pub max_rows: Option<usize>,
    pub timeout_seconds: Option<u64>,
    /// Stop at the first failing statement (default `true`).
    pub stop_on_error: Option<bool>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DbCancelRecord {
    pub cancelled: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DbCompletionRecord {
    pub tables: std::collections::BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct DbHistoryParams {
    pub connection_id: Option<i32>,
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub keyword: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DbHistoryRecord {
    pub id: i32,
    pub connection_id: i32,
    pub database_name: Option<String>,
    pub sql_text: String,
    pub status: String,
    pub statement_count: i32,
    pub affected_rows: i64,
    pub duration_ms: i64,
    pub error_message: Option<String>,
    pub created_at: String,
}

impl From<db_query_logs::Model> for DbHistoryRecord {
    fn from(model: db_query_logs::Model) -> Self {
        Self {
            id: model.id,
            connection_id: model.connection_id,
            database_name: model.database_name,
            sql_text: model.sql_text,
            status: model.status,
            statement_count: model.statement_count,
            affected_rows: model.affected_rows,
            duration_ms: model.duration_ms,
            error_message: model.error_message,
            created_at: model.created_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct DbSavedQueryParams {
    pub connection_id: Option<i32>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveDbQueryParams {
    pub connection_id: i32,
    pub name: String,
    pub database_name: Option<String>,
    pub sql_text: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DbSavedQueryRecord {
    pub id: i32,
    pub connection_id: i32,
    pub name: String,
    pub database_name: Option<String>,
    pub sql_text: String,
    pub created_at: String,
    pub updated_at: String,
}

impl From<db_saved_queries::Model> for DbSavedQueryRecord {
    fn from(model: db_saved_queries::Model) -> Self {
        Self {
            id: model.id,
            connection_id: model.connection_id,
            name: model.name,
            database_name: model.database_name,
            sql_text: model.sql_text,
            created_at: model.created_at.to_rfc3339(),
            updated_at: model.updated_at.to_rfc3339(),
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn not_found() -> ApiError {
    ApiError::not_found("数据库连接不存在").with_code("DB_CONNECTION_NOT_FOUND")
}

async fn find_owned(ctx: &AppContext, user_id: i32, id: i32) -> ApiResult<db_connections::Model> {
    db_connections::Entity::find_by_id(id)
        .filter(db_connections::Column::UserId.eq(user_id))
        .one(&ctx.db)
        .await?
        .ok_or_else(not_found)
}

fn jwt_secret(ctx: &AppContext) -> ApiResult<String> {
    Ok(ctx.config.get_jwt_config()?.secret.clone())
}

fn clean(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Validated connection settings from a form.
struct ConnectionForm {
    name: String,
    dialect: Dialect,
    host: Option<String>,
    port: Option<u16>,
    username: Option<String>,
    database_name: Option<String>,
    ssl_mode: Option<String>,
    read_only: bool,
}

fn validate_form(params: &SaveDbConnectionParams) -> ApiResult<ConnectionForm> {
    let name = params.name.trim().to_string();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(ApiError::bad_request(
            "连接名称不能为空，且不超过 100 个字符",
        ));
    }
    let dialect = Dialect::parse(params.db_type.trim())?;
    let database_name = clean(params.database_name.as_deref());
    let ssl_mode = clean(params.ssl_mode.as_deref());
    let read_only = params.read_only.unwrap_or(false);
    if dialect == Dialect::Sqlite {
        if database_name.is_none() {
            return Err(ApiError::bad_request("请填写 SQLite 数据库文件路径"));
        }
        return Ok(ConnectionForm {
            name,
            dialect,
            host: None,
            port: None,
            username: None,
            database_name,
            ssl_mode: None,
            read_only,
        });
    }
    let host = clean(params.host.as_deref())
        .filter(|host| host.len() <= 255)
        .ok_or_else(|| ApiError::bad_request("请填写主机地址"))?;
    let username =
        clean(params.username.as_deref()).ok_or_else(|| ApiError::bad_request("请填写用户名"))?;
    let port = params
        .port
        .or_else(|| dialect.default_port())
        .filter(|port| *port > 0);
    if let Some(mode) = ssl_mode.as_deref() {
        let modes = if dialect == Dialect::Postgres {
            PG_SSL_MODES
        } else {
            MYSQL_SSL_MODES
        };
        if !modes.contains(&mode) {
            return Err(ApiError::bad_request(format!("无效的 SSL 模式：{mode}")));
        }
    }
    Ok(ConnectionForm {
        name,
        dialect,
        host: Some(host),
        port,
        username: Some(username),
        database_name,
        ssl_mode,
        read_only,
    })
}

fn config_from_form(form: &ConnectionForm, password: Option<String>) -> ConnectConfig {
    ConnectConfig {
        dialect: form.dialect,
        host: form.host.clone().unwrap_or_default(),
        port: form
            .port
            .or_else(|| form.dialect.default_port())
            .unwrap_or(0),
        username: form.username.clone().unwrap_or_default(),
        password,
        database: form.database_name.clone(),
        ssl_mode: form.ssl_mode.clone(),
        read_only: form.read_only,
    }
}

fn target_of(ctx: &AppContext, model: &db_connections::Model) -> ApiResult<Target> {
    let dialect = Dialect::parse(&model.db_type)?;
    let password = match model.encrypted_password.as_deref() {
        Some(payload) => Some(decrypt_password(
            &jwt_secret(ctx)?,
            model.user_id,
            model.id,
            payload,
        )?),
        None => None,
    };
    Ok(Target {
        connection_id: model.id,
        user_id: model.user_id,
        config: ConnectConfig {
            dialect,
            host: model.host.clone().unwrap_or_default(),
            port: model
                .port
                .and_then(|port| u16::try_from(port).ok())
                .or_else(|| dialect.default_port())
                .unwrap_or(0),
            username: model.username.clone().unwrap_or_default(),
            password,
            database: model.database_name.clone(),
            ssl_mode: model.ssl_mode.clone(),
            read_only: model.read_only,
        },
    })
}

/// Resolves a connection of the user; asks for the password confirmation
/// before a new connection to the database is opened.
async fn open_target(
    ctx: &AppContext,
    session: &SessionExt,
    user: &users::Model,
    id: i32,
    database: Option<&str>,
) -> ApiResult<(Target, String)> {
    let model = find_owned(ctx, user.id, id).await?;
    let target = target_of(ctx, &model)?;
    let database = target.database(database);
    if !db_manager::is_connected(&target, &database) {
        require_remote_reauth(ctx, session).await?;
    }
    Ok((target, database))
}

fn parse_json_list<T: serde::de::DeserializeOwned>(
    value: Option<&str>,
    label: &str,
) -> ApiResult<Vec<T>> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map_or_else(
            || Ok(Vec::new()),
            |value| {
                serde_json::from_str(value)
                    .map_err(|_| ApiError::bad_request(format!("{label}格式不正确")))
            },
        )
}

async fn rows_query(
    ctx: &AppContext,
    user: &users::Model,
    database: String,
    params: &DbRowsParams,
) -> ApiResult<RowsQuery> {
    let where_clause = clean(params.where_clause.as_deref());
    if where_clause.is_some() {
        rbac::assert_permission(&ctx.db, user.id, QUERY_PERMISSION).await?;
    }
    Ok(RowsQuery {
        database,
        schema: clean(params.schema.as_deref()),
        table: params.table.clone(),
        page: params.page.unwrap_or(1),
        page_size: params.page_size.unwrap_or(100),
        sort: parse_json_list::<SortSpec>(params.sort.as_deref(), "排序")?,
        filters: parse_json_list::<FilterSpec>(params.filters.as_deref(), "筛选条件")?,
        where_clause,
    })
}

fn truncate_chars(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        value.to_string()
    } else {
        value.chars().take(limit).collect()
    }
}

async fn record_history(
    ctx: &AppContext,
    target: &Target,
    database: &str,
    sql_text: &str,
    result: Result<&ExecuteResult, &ApiError>,
) {
    let (status, statement_count, affected, duration, error) = match result {
        Ok(result) => (
            if result.first_error().is_some() {
                "failed"
            } else {
                "success"
            },
            i32::try_from(result.statement_count).unwrap_or(i32::MAX),
            i64::try_from(result.affected_rows()).unwrap_or(i64::MAX),
            i64::try_from(result.duration_ms).unwrap_or(i64::MAX),
            result.first_error().map(str::to_string),
        ),
        Err(error) => ("failed", 0, 0, 0, Some(error.message().to_string())),
    };
    let row = db_query_logs::ActiveModel {
        connection_id: Set(target.connection_id),
        user_id: Set(target.user_id),
        database_name: Set(Some(database.to_string()).filter(|name| !name.is_empty())),
        sql_text: Set(truncate_chars(sql_text, HISTORY_SQL_LIMIT)),
        status: Set(status.to_string()),
        statement_count: Set(statement_count),
        affected_rows: Set(affected),
        duration_ms: Set(duration),
        error_message: Set(error),
        ..Default::default()
    };
    if let Err(error) = row.insert(&ctx.db).await {
        tracing::warn!(error = %error, "failed to record database query history");
        return;
    }
    // Keep the newest entries of this user and connection.
    let cutoff = db_query_logs::Entity::find()
        .filter(db_query_logs::Column::ConnectionId.eq(target.connection_id))
        .filter(db_query_logs::Column::UserId.eq(target.user_id))
        .order_by_desc(db_query_logs::Column::Id)
        .offset(HISTORY_KEEP)
        .one(&ctx.db)
        .await;
    if let Ok(Some(cutoff)) = cutoff {
        let _ = db_query_logs::Entity::delete_many()
            .filter(db_query_logs::Column::ConnectionId.eq(target.connection_id))
            .filter(db_query_logs::Column::UserId.eq(target.user_id))
            .filter(db_query_logs::Column::Id.lte(cutoff.id))
            .exec(&ctx.db)
            .await;
    }
}

// ---------------------------------------------------------------------------
// Connections
// ---------------------------------------------------------------------------

#[utoipa::path(
    get,
    path = "/api/admin/db-manager/connections",
    tag = "admin-db-manager",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<Vec<DbConnectionRecord>>))
)]
#[debug_handler]
pub async fn list_connections(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, LIST_PERMISSION).await?;
    let rows = db_connections::Entity::find()
        .filter(db_connections::Column::UserId.eq(user.id))
        .order_by_asc(db_connections::Column::SortOrder)
        .order_by_asc(db_connections::Column::Id)
        .all(&ctx.db)
        .await?;
    Ok(responses::ok(
        rows.into_iter()
            .map(DbConnectionRecord::from)
            .collect::<Vec<_>>(),
    ))
}

#[utoipa::path(
    post,
    path = "/api/admin/db-manager/connections",
    tag = "admin-db-manager",
    security(("bearer_auth" = [])),
    request_body = SaveDbConnectionParams,
    responses((status = 200, body = ApiResponse<DbConnectionRecord>))
)]
#[debug_handler]
pub async fn create_connection(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SaveDbConnectionParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, MANAGE_PERMISSION).await?;
    let form = validate_form(&params)?;
    let model = db_connections::ActiveModel {
        name: Set(form.name),
        db_type: Set(form.dialect.as_str().to_string()),
        host: Set(form.host),
        port: Set(form.port.map(i32::from)),
        username: Set(form.username),
        database_name: Set(form.database_name),
        ssl_mode: Set(form.ssl_mode),
        read_only: Set(form.read_only),
        group_name: Set(clean(params.group_name.as_deref())),
        color: Set(clean(params.color.as_deref())),
        remark: Set(clean(params.remark.as_deref())),
        sort_order: Set(params.sort_order.unwrap_or(0)),
        user_id: Set(user.id),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;
    let model = match params.password.as_deref().filter(|value| !value.is_empty()) {
        Some(password) => {
            let mut active = model.clone().into_active_model();
            active.encrypted_password = Set(Some(encrypt_password(
                &jwt_secret(&ctx)?,
                user.id,
                model.id,
                password,
            )?));
            active.update(&ctx.db).await?
        }
        None => model,
    };
    Ok(responses::ok(DbConnectionRecord::from(model)))
}

#[utoipa::path(
    put,
    path = "/api/admin/db-manager/connections/{id}",
    tag = "admin-db-manager",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = SaveDbConnectionParams,
    responses((status = 200, body = ApiResponse<DbConnectionRecord>))
)]
#[debug_handler]
pub async fn update_connection(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<SaveDbConnectionParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, MANAGE_PERMISSION).await?;
    let model = find_owned(&ctx, user.id, id).await?;
    let form = validate_form(&params)?;
    let mut active = model.into_active_model();
    active.name = Set(form.name);
    active.db_type = Set(form.dialect.as_str().to_string());
    active.host = Set(form.host);
    active.port = Set(form.port.map(i32::from));
    active.username = Set(form.username);
    active.database_name = Set(form.database_name);
    active.ssl_mode = Set(form.ssl_mode);
    active.read_only = Set(form.read_only);
    active.group_name = Set(clean(params.group_name.as_deref()));
    active.color = Set(clean(params.color.as_deref()));
    active.remark = Set(clean(params.remark.as_deref()));
    if let Some(sort_order) = params.sort_order {
        active.sort_order = Set(sort_order);
    }
    if let Some(password) = params.password.as_deref().filter(|value| !value.is_empty()) {
        active.encrypted_password = Set(Some(encrypt_password(
            &jwt_secret(&ctx)?,
            user.id,
            id,
            password,
        )?));
    } else if params.clear_password.unwrap_or(false) {
        active.encrypted_password = Set(None);
    }
    let model = active.update(&ctx.db).await?;
    // Open pools and editor sessions used the old settings.
    db_manager::disconnect(id);
    Ok(responses::ok(DbConnectionRecord::from(model)))
}

#[utoipa::path(
    delete,
    path = "/api/admin/db-manager/connections/{id}",
    tag = "admin-db-manager",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn delete_connection(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, MANAGE_PERMISSION).await?;
    let model = find_owned(&ctx, user.id, id).await?;
    crate::services::recycle_bin::delete_row(
        &ctx.db,
        crate::services::recycle_bin::Deletion::new("db_connections", id, &model.name, &user),
        "db_connections",
        i64::from(id),
        &[],
    )
    .await?;
    db_manager::disconnect(id);
    Ok(responses::empty())
}

#[utoipa::path(
    post,
    path = "/api/admin/db-manager/connections/test",
    tag = "admin-db-manager",
    security(("bearer_auth" = [])),
    request_body = SaveDbConnectionParams,
    responses((status = 200, body = ApiResponse<DbTestRecord>))
)]
#[debug_handler]
pub async fn test_connection(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    session: SessionExt,
    Json(params): Json<SaveDbConnectionParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, MANAGE_PERMISSION).await?;
    let form = validate_form(&params)?;
    let password = match params.password.as_deref().filter(|value| !value.is_empty()) {
        Some(password) => Some(password.to_string()),
        None => match params.id {
            Some(id) if !params.clear_password.unwrap_or(false) => {
                target_of(&ctx, &find_owned(&ctx, user.id, id).await?)?
                    .config
                    .password
            }
            _ => None,
        },
    };
    require_remote_reauth(&ctx, &session).await?;
    let started = std::time::Instant::now();
    let version = db_manager::test(&config_from_form(&form, password)).await?;
    Ok(responses::ok(DbTestRecord {
        version,
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    }))
}

#[utoipa::path(
    post,
    path = "/api/admin/db-manager/connections/{id}/connect",
    tag = "admin-db-manager",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<DbConnectRecord>))
)]
#[debug_handler]
pub async fn connect(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    session: SessionExt,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, LIST_PERMISSION).await?;
    let (target, database) = open_target(&ctx, &session, &user, id, None).await?;
    let pool = db_manager::pool(&target, &database).await?;
    let mut conn = pool.acquire().await?;
    let mut conn = conn.as_mut();
    let version = db_manager::meta::server_version(&mut conn).await?;
    let mut databases = db_manager::meta::databases(&mut conn).await?;
    if !database.is_empty() && !databases.contains(&database) {
        databases.insert(0, database.clone());
    }
    let mut active = find_owned(&ctx, user.id, id).await?.into_active_model();
    active.last_connected_at = Set(Some(Utc::now().into()));
    active.update(&ctx.db).await?;
    Ok(responses::ok(DbConnectRecord {
        connection_id: id,
        db_type: target.dialect().as_str().to_string(),
        version,
        databases,
        default_database: database,
        has_schemas: target.dialect().has_schemas(),
        read_only: target.config.read_only,
    }))
}

#[utoipa::path(
    post,
    path = "/api/admin/db-manager/connections/{id}/disconnect",
    tag = "admin-db-manager",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn disconnect(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, LIST_PERMISSION).await?;
    find_owned(&ctx, user.id, id).await?;
    db_manager::disconnect(id);
    Ok(responses::empty())
}

// ---------------------------------------------------------------------------
// Object browser
// ---------------------------------------------------------------------------

#[utoipa::path(
    get,
    path = "/api/admin/db-manager/connections/{id}/schemas",
    tag = "admin-db-manager",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path), DbLocationParams),
    responses((status = 200, body = ApiResponse<Vec<String>>))
)]
#[debug_handler]
pub async fn schemas(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    session: SessionExt,
    Path(id): Path<i32>,
    Query(params): Query<DbLocationParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, LIST_PERMISSION).await?;
    let (target, database) =
        open_target(&ctx, &session, &user, id, params.database.as_deref()).await?;
    let pool = db_manager::pool(&target, &database).await?;
    let mut conn = pool.acquire().await?;
    Ok(responses::ok(
        db_manager::meta::schemas(&mut conn.as_mut()).await?,
    ))
}

#[utoipa::path(
    get,
    path = "/api/admin/db-manager/connections/{id}/objects",
    tag = "admin-db-manager",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path), DbLocationParams),
    responses((status = 200, body = ApiResponse<Vec<DbObject>>))
)]
#[debug_handler]
pub async fn objects(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    session: SessionExt,
    Path(id): Path<i32>,
    Query(params): Query<DbLocationParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, LIST_PERMISSION).await?;
    let (target, database) =
        open_target(&ctx, &session, &user, id, params.database.as_deref()).await?;
    let pool = db_manager::pool(&target, &database).await?;
    let mut conn = pool.acquire().await?;
    Ok(responses::ok(
        db_manager::meta::objects(&mut conn.as_mut(), &database, params.schema.as_deref()).await?,
    ))
}

#[utoipa::path(
    get,
    path = "/api/admin/db-manager/connections/{id}/structure",
    tag = "admin-db-manager",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path), DbTableParams),
    responses((status = 200, body = ApiResponse<TableStructure>))
)]
#[debug_handler]
pub async fn structure(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    session: SessionExt,
    Path(id): Path<i32>,
    Query(params): Query<DbTableParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, LIST_PERMISSION).await?;
    let (target, database) =
        open_target(&ctx, &session, &user, id, params.database.as_deref()).await?;
    let pool = db_manager::pool(&target, &database).await?;
    let mut conn = pool.acquire().await?;
    Ok(responses::ok(
        db_manager::meta::structure(
            &mut conn.as_mut(),
            &database,
            clean(params.schema.as_deref()).as_deref(),
            &params.table,
        )
        .await?,
    ))
}

#[utoipa::path(
    get,
    path = "/api/admin/db-manager/connections/{id}/routine",
    tag = "admin-db-manager",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path), DbRoutineParams),
    responses((status = 200, body = ApiResponse<DbDdlRecord>))
)]
#[debug_handler]
pub async fn routine(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    session: SessionExt,
    Path(id): Path<i32>,
    Query(params): Query<DbRoutineParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, LIST_PERMISSION).await?;
    let (target, database) =
        open_target(&ctx, &session, &user, id, params.database.as_deref()).await?;
    let pool = db_manager::pool(&target, &database).await?;
    let mut conn = pool.acquire().await?;
    let ddl = db_manager::meta::routine_ddl(
        &mut conn.as_mut(),
        &database,
        &params.kind,
        &params.name,
        params.identity.as_deref(),
    )
    .await?;
    Ok(responses::ok(DbDdlRecord { ddl }))
}

#[utoipa::path(
    get,
    path = "/api/admin/db-manager/connections/{id}/completion",
    tag = "admin-db-manager",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path), DbLocationParams),
    responses((status = 200, body = ApiResponse<DbCompletionRecord>))
)]
#[debug_handler]
pub async fn completion(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    session: SessionExt,
    Path(id): Path<i32>,
    Query(params): Query<DbLocationParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, LIST_PERMISSION).await?;
    let (target, database) =
        open_target(&ctx, &session, &user, id, params.database.as_deref()).await?;
    let pool = db_manager::pool(&target, &database).await?;
    let mut conn = pool.acquire().await?;
    let tables = db_manager::meta::completion(
        &mut conn.as_mut(),
        &database,
        clean(params.schema.as_deref()).as_deref(),
    )
    .await?;
    Ok(responses::ok(DbCompletionRecord { tables }))
}

// ---------------------------------------------------------------------------
// Table data
// ---------------------------------------------------------------------------

#[utoipa::path(
    get,
    path = "/api/admin/db-manager/connections/{id}/rows",
    tag = "admin-db-manager",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path), DbRowsParams),
    responses((status = 200, body = ApiResponse<RowsPage>))
)]
#[debug_handler]
pub async fn rows(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    session: SessionExt,
    Path(id): Path<i32>,
    Query(params): Query<DbRowsParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, LIST_PERMISSION).await?;
    let (target, database) =
        open_target(&ctx, &session, &user, id, params.database.as_deref()).await?;
    let query = rows_query(&ctx, &user, database, &params).await?;
    Ok(responses::ok(db_manager::rows(&target, &query).await?))
}

#[utoipa::path(
    get,
    path = "/api/admin/db-manager/connections/{id}/export",
    tag = "admin-db-manager",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path), DbRowsParams),
    responses((status = 200, description = "CSV, JSON or SQL file"))
)]
#[debug_handler]
pub async fn export(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    session: SessionExt,
    Path(id): Path<i32>,
    Query(params): Query<DbRowsParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, LIST_PERMISSION).await?;
    let format = ExportFormat::parse(params.format.as_deref().unwrap_or("csv"))?;
    let (target, database) =
        open_target(&ctx, &session, &user, id, params.database.as_deref()).await?;
    let query = rows_query(&ctx, &user, database, &params).await?;
    let export = db_manager::export_rows(&target, &query).await?;
    let base = format!(
        "{}-{}",
        params
            .table
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            })
            .collect::<String>(),
        Local::now().format("%Y%m%d-%H%M%S")
    );
    let names: Vec<String> = export
        .columns
        .iter()
        .map(|column| column.name.clone())
        .collect();
    let (body, content_type, extension) = match format {
        ExportFormat::Csv => {
            let headers: Vec<&str> = names.iter().map(String::as_str).collect();
            let rows: Vec<Vec<String>> = export
                .rows
                .iter()
                .map(|row| row.iter().map(db_manager::driver::text).collect())
                .collect();
            (
                crate::services::csv_export::document(&headers, &rows),
                "text/csv; charset=utf-8",
                "csv",
            )
        }
        ExportFormat::Json => {
            let items: Vec<serde_json::Map<String, serde_json::Value>> = export
                .rows
                .iter()
                .map(|row| names.iter().cloned().zip(row.iter().cloned()).collect())
                .collect();
            (
                serde_json::to_string_pretty(&items)
                    .map_err(|error| ApiError::internal(error.to_string()))?,
                "application/json; charset=utf-8",
                "json",
            )
        }
        ExportFormat::Sql => {
            let dialect = target.dialect();
            let qualified =
                dialect.qualified(&query.database, query.schema.as_deref(), &query.table);
            (
                db_manager::insert_script(dialect, &qualified, &export),
                "application/sql; charset=utf-8",
                "sql",
            )
        }
    };
    Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{base}.{extension}\""),
        )
        .header("x-total-count", export.rows.len().to_string())
        .body(Body::from(body))
        .map_err(|error| ApiError::internal(error.to_string()))
}

#[utoipa::path(
    post,
    path = "/api/admin/db-manager/connections/{id}/changes",
    tag = "admin-db-manager",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = DbChangesParams,
    responses((status = 200, body = ApiResponse<ChangeSummary>))
)]
#[debug_handler]
pub async fn apply_changes(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    session: SessionExt,
    Path(id): Path<i32>,
    Json(params): Json<DbChangesParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, QUERY_PERMISSION).await?;
    let (target, database) =
        open_target(&ctx, &session, &user, id, params.database.as_deref()).await?;
    let summary = db_manager::apply_changes(
        &target,
        &database,
        clean(params.schema.as_deref()).as_deref(),
        &params.table,
        &params.changes,
    )
    .await?;
    Ok(responses::ok(summary))
}

// ---------------------------------------------------------------------------
// SQL editor
// ---------------------------------------------------------------------------

#[utoipa::path(
    post,
    path = "/api/admin/db-manager/connections/{id}/execute",
    tag = "admin-db-manager",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = DbExecuteParams,
    responses((status = 200, body = ApiResponse<ExecuteResult>))
)]
#[debug_handler]
pub async fn execute(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    session: SessionExt,
    Path(id): Path<i32>,
    Json(params): Json<DbExecuteParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, QUERY_PERMISSION).await?;
    db_manager::validate_session_key(&params.session_key)?;
    let (target, database) =
        open_target(&ctx, &session, &user, id, params.database.as_deref()).await?;
    let options = ExecuteOptions {
        max_rows: params
            .max_rows
            .unwrap_or(db_manager::DEFAULT_RESULT_ROWS)
            .clamp(1, db_manager::MAX_RESULT_ROWS),
        timeout: Duration::from_secs(
            params
                .timeout_seconds
                .unwrap_or(db_manager::DEFAULT_TIMEOUT_SECONDS)
                .clamp(1, db_manager::MAX_TIMEOUT_SECONDS),
        ),
        stop_on_error: params.stop_on_error.unwrap_or(true),
    };
    let result = db_manager::execute(
        &target,
        &database,
        &params.session_key,
        &params.sql,
        options,
    )
    .await;
    match &result {
        Err(error) if error.code() == "DB_SESSION_BUSY" => {}
        other => record_history(&ctx, &target, &database, &params.sql, other.as_ref()).await,
    }
    Ok(responses::ok(result?))
}

#[utoipa::path(
    post,
    path = "/api/admin/db-manager/connections/{id}/sessions/{key}/cancel",
    tag = "admin-db-manager",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path), ("key" = String, Path)),
    responses((status = 200, body = ApiResponse<DbCancelRecord>))
)]
#[debug_handler]
pub async fn cancel(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path((id, key)): Path<(i32, String)>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, QUERY_PERMISSION).await?;
    let target = target_of(&ctx, &find_owned(&ctx, user.id, id).await?)?;
    let cancelled = db_manager::cancel(&target, &key).await?;
    Ok(responses::ok(DbCancelRecord { cancelled }))
}

#[utoipa::path(
    delete,
    path = "/api/admin/db-manager/connections/{id}/sessions/{key}",
    tag = "admin-db-manager",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path), ("key" = String, Path)),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn close_session(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path((id, key)): Path<(i32, String)>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, LIST_PERMISSION).await?;
    find_owned(&ctx, user.id, id).await?;
    db_manager::close_session(user.id, &key);
    Ok(responses::empty())
}

// ---------------------------------------------------------------------------
// History and saved queries
// ---------------------------------------------------------------------------

#[utoipa::path(
    get,
    path = "/api/admin/db-manager/history",
    tag = "admin-db-manager",
    security(("bearer_auth" = [])),
    params(DbHistoryParams),
    responses((status = 200, body = ApiResponse<PageResponse<DbHistoryRecord>>))
)]
#[debug_handler]
pub async fn history(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<DbHistoryParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, LIST_PERMISSION).await?;
    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(20).clamp(1, 100);
    let mut query = db_query_logs::Entity::find().filter(db_query_logs::Column::UserId.eq(user.id));
    if let Some(connection_id) = params.connection_id {
        query = query.filter(db_query_logs::Column::ConnectionId.eq(connection_id));
    }
    if let Some(keyword) = clean(params.keyword.as_deref()) {
        query = query.filter(db_query_logs::Column::SqlText.contains(keyword));
    }
    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .order_by_desc(db_query_logs::Column::Id)
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(DbHistoryRecord::from)
        .collect();
    Ok(responses::ok(PageResponse {
        items,
        page,
        page_size,
        total,
    }))
}

#[utoipa::path(
    delete,
    path = "/api/admin/db-manager/history",
    tag = "admin-db-manager",
    security(("bearer_auth" = [])),
    params(DbHistoryParams),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn clear_history(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<DbHistoryParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, LIST_PERMISSION).await?;
    let mut delete =
        db_query_logs::Entity::delete_many().filter(db_query_logs::Column::UserId.eq(user.id));
    if let Some(connection_id) = params.connection_id {
        delete = delete.filter(db_query_logs::Column::ConnectionId.eq(connection_id));
    }
    delete.exec(&ctx.db).await?;
    Ok(responses::empty())
}

#[utoipa::path(
    get,
    path = "/api/admin/db-manager/saved-queries",
    tag = "admin-db-manager",
    security(("bearer_auth" = [])),
    params(DbSavedQueryParams),
    responses((status = 200, body = ApiResponse<Vec<DbSavedQueryRecord>>))
)]
#[debug_handler]
pub async fn list_saved_queries(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<DbSavedQueryParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, LIST_PERMISSION).await?;
    let mut query =
        db_saved_queries::Entity::find().filter(db_saved_queries::Column::UserId.eq(user.id));
    if let Some(connection_id) = params.connection_id {
        query = query.filter(db_saved_queries::Column::ConnectionId.eq(connection_id));
    }
    let items: Vec<DbSavedQueryRecord> = query
        .order_by_asc(db_saved_queries::Column::Name)
        .all(&ctx.db)
        .await?
        .into_iter()
        .map(DbSavedQueryRecord::from)
        .collect();
    Ok(responses::ok(items))
}

fn validate_saved_query(params: &SaveDbQueryParams) -> ApiResult<String> {
    let name = params.name.trim().to_string();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(ApiError::bad_request(
            "查询名称不能为空，且不超过 100 个字符",
        ));
    }
    if params.sql_text.trim().is_empty() {
        return Err(ApiError::bad_request("SQL 不能为空"));
    }
    Ok(name)
}

#[utoipa::path(
    post,
    path = "/api/admin/db-manager/saved-queries",
    tag = "admin-db-manager",
    security(("bearer_auth" = [])),
    request_body = SaveDbQueryParams,
    responses((status = 200, body = ApiResponse<DbSavedQueryRecord>))
)]
#[debug_handler]
pub async fn create_saved_query(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SaveDbQueryParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, QUERY_PERMISSION).await?;
    let name = validate_saved_query(&params)?;
    find_owned(&ctx, user.id, params.connection_id).await?;
    let model = db_saved_queries::ActiveModel {
        connection_id: Set(params.connection_id),
        user_id: Set(user.id),
        name: Set(name),
        database_name: Set(clean(params.database_name.as_deref())),
        sql_text: Set(params.sql_text),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;
    Ok(responses::ok(DbSavedQueryRecord::from(model)))
}

async fn find_saved_query(
    ctx: &AppContext,
    user_id: i32,
    id: i32,
) -> ApiResult<db_saved_queries::Model> {
    db_saved_queries::Entity::find_by_id(id)
        .filter(db_saved_queries::Column::UserId.eq(user_id))
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::not_found("查询不存在").with_code("DB_QUERY_NOT_FOUND"))
}

#[utoipa::path(
    put,
    path = "/api/admin/db-manager/saved-queries/{id}",
    tag = "admin-db-manager",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = SaveDbQueryParams,
    responses((status = 200, body = ApiResponse<DbSavedQueryRecord>))
)]
#[debug_handler]
pub async fn update_saved_query(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<SaveDbQueryParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, QUERY_PERMISSION).await?;
    let name = validate_saved_query(&params)?;
    let model = find_saved_query(&ctx, user.id, id).await?;
    find_owned(&ctx, user.id, params.connection_id).await?;
    let mut active = model.into_active_model();
    active.connection_id = Set(params.connection_id);
    active.name = Set(name);
    active.database_name = Set(clean(params.database_name.as_deref()));
    active.sql_text = Set(params.sql_text);
    Ok(responses::ok(DbSavedQueryRecord::from(
        active.update(&ctx.db).await?,
    )))
}

#[utoipa::path(
    delete,
    path = "/api/admin/db-manager/saved-queries/{id}",
    tag = "admin-db-manager",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn delete_saved_query(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, QUERY_PERMISSION).await?;
    find_saved_query(&ctx, user.id, id).await?;
    db_saved_queries::Entity::delete_by_id(id)
        .exec(&ctx.db)
        .await?;
    Ok(responses::empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(db_type: &str) -> SaveDbConnectionParams {
        SaveDbConnectionParams {
            id: None,
            name: " 本地库 ".into(),
            db_type: db_type.into(),
            host: Some(" 127.0.0.1 ".into()),
            port: None,
            username: Some("root".into()),
            password: None,
            clear_password: None,
            database_name: Some(String::new()),
            ssl_mode: None,
            read_only: None,
            group_name: None,
            color: None,
            remark: None,
            sort_order: None,
        }
    }

    #[test]
    fn connection_forms_are_validated_per_type() {
        let form = validate_form(&params("mysql")).unwrap();
        assert_eq!(form.name, "本地库");
        assert_eq!(form.host.as_deref(), Some("127.0.0.1"));
        assert_eq!(form.port, Some(3306));
        assert_eq!(form.database_name, None);
        assert_eq!(validate_form(&params("postgres")).unwrap().port, Some(5432));

        let mut sqlite = params("sqlite");
        assert!(validate_form(&sqlite).is_err());
        sqlite.database_name = Some("data/app.db".into());
        let form = validate_form(&sqlite).unwrap();
        assert_eq!(form.host, None);
        assert_eq!(form.username, None);

        let mut bad_ssl = params("postgres");
        bad_ssl.ssl_mode = Some("preferred".into());
        assert!(validate_form(&bad_ssl).is_err());
        bad_ssl.ssl_mode = Some("require".into());
        assert!(validate_form(&bad_ssl).is_ok());

        let mut no_host = params("postgres");
        no_host.host = Some("  ".into());
        assert!(validate_form(&no_host).is_err());
        assert!(validate_form(&params("oracle")).is_err());
    }
}
