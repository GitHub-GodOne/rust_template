//! 数据库管理: browsing and editing databases through saved connections.
//!
//! Supports PostgreSQL, `MySQL` and `SQLite`.
//!
//! Each connection keeps a small pool per database for catalog queries and
//! table browsing. SQL editor tabs get their own connection (an editor
//! session) taken out of that pool, so `BEGIN` … `COMMIT` and session
//! settings carry over between runs, like in desktop database tools.
//! Sessions and pools idle for 30 minutes are closed; an open transaction
//! in a closed session is rolled back by the server.

#![allow(clippy::missing_errors_doc)]

pub mod driver;
pub mod meta;
pub mod sql;

use std::{
    collections::HashMap,
    fmt::Write as _,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use self::{
    driver::{
        error_text, query_error, Bind, ConnMut, ConnectConfig, DbConn, DbPool, DecodeOptions,
        ResultColumn,
    },
    meta::ColumnInfo,
    sql::Dialect,
};
use crate::errors::{ApiError, ApiResult};

const IDLE_LIMIT: Duration = Duration::from_secs(30 * 60);
pub const MAX_RESULT_ROWS: usize = 10_000;
pub const DEFAULT_RESULT_ROWS: usize = 1_000;
pub const MAX_TIMEOUT_SECONDS: u64 = 3_600;
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 60;
pub const MAX_PAGE_SIZE: u64 = 1_000;
pub const MAX_EXPORT_ROWS: i64 = 50_000;
const MAX_STATEMENTS: usize = 5_000;
const MAX_CHANGES: usize = 1_000;

/// A saved connection resolved for one user.
#[derive(Debug, Clone)]
pub struct Target {
    pub connection_id: i32,
    pub user_id: i32,
    pub config: ConnectConfig,
}

impl Target {
    #[must_use]
    pub const fn dialect(&self) -> Dialect {
        self.config.dialect
    }

    /// Changes whenever anything that affects the connection changes.
    fn fingerprint(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(serde_json::to_vec(&self.config).unwrap_or_default());
        hash.update(self.config.password.as_deref().unwrap_or_default());
        hex::encode(hash.finalize())
    }

    /// The database a request works in.
    #[must_use]
    pub fn database(&self, requested: Option<&str>) -> String {
        if self.dialect() == Dialect::Sqlite {
            return "main".to_string();
        }
        requested
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .or_else(|| {
                self.config
                    .database
                    .clone()
                    .filter(|name| !name.trim().is_empty())
            })
            .unwrap_or_else(|| {
                if self.dialect() == Dialect::Postgres {
                    "postgres".to_string()
                } else {
                    String::new()
                }
            })
    }
}

struct PoolEntry {
    pool: DbPool,
    fingerprint: String,
    last_used: Mutex<Instant>,
}

type PoolKey = (i32, String);

fn pools() -> &'static Mutex<HashMap<PoolKey, Arc<PoolEntry>>> {
    static POOLS: OnceLock<Mutex<HashMap<PoolKey, Arc<PoolEntry>>>> = OnceLock::new();
    POOLS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn sweep_pools() {
    let stale: Vec<Arc<PoolEntry>> = {
        let mut pools = pools()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let keys: Vec<PoolKey> = pools
            .iter()
            .filter(|(_, entry)| {
                entry
                    .last_used
                    .lock()
                    .map(|last| last.elapsed() > IDLE_LIMIT)
                    .unwrap_or(true)
            })
            .map(|(key, _)| key.clone())
            .collect();
        let removed = keys.iter().filter_map(|key| pools.remove(key)).collect();
        drop(pools);
        removed
    };
    for entry in stale {
        tokio::spawn(async move { entry.pool.close().await });
    }
}

/// Whether a request would open a new connection (and so needs the
/// remote-connection password confirmation).
#[must_use]
pub fn is_connected(target: &Target, database: &str) -> bool {
    let fingerprint = target.fingerprint();
    pools()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&(target.connection_id, database.to_string()))
        .is_some_and(|entry| entry.fingerprint == fingerprint)
}

pub async fn pool(target: &Target, database: &str) -> ApiResult<DbPool> {
    sweep_pools();
    sweep_sessions();
    let key = (target.connection_id, database.to_string());
    let fingerprint = target.fingerprint();
    let existing = pools()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&key)
        .filter(|entry| entry.fingerprint == fingerprint)
        .cloned();
    if let Some(entry) = existing {
        if let Ok(mut last) = entry.last_used.lock() {
            *last = Instant::now();
        }
        return Ok(entry.pool.clone());
    }
    let pool = driver::connect_pool(&target.config, Some(database)).await?;
    let replaced = pools()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(
            key,
            Arc::new(PoolEntry {
                pool: pool.clone(),
                fingerprint,
                last_used: Mutex::new(Instant::now()),
            }),
        );
    if let Some(old) = replaced {
        tokio::spawn(async move { old.pool.close().await });
    }
    Ok(pool)
}

/// Closes every pool and editor session of a connection.
pub fn disconnect(connection_id: i32) {
    let removed: Vec<Arc<PoolEntry>> = {
        let mut pools = pools()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let keys: Vec<PoolKey> = pools
            .keys()
            .filter(|(id, _)| *id == connection_id)
            .cloned()
            .collect();
        let removed = keys.iter().filter_map(|key| pools.remove(key)).collect();
        drop(pools);
        removed
    };
    for entry in removed {
        tokio::spawn(async move { entry.pool.close().await });
    }
    let sessions: Vec<Arc<EditorSession>> = {
        let mut sessions = sessions()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let keys: Vec<String> = sessions
            .iter()
            .filter(|(_, session)| session.connection_id == connection_id)
            .map(|(key, _)| key.clone())
            .collect();
        let removed = keys.iter().filter_map(|key| sessions.remove(key)).collect();
        drop(sessions);
        removed
    };
    for session in sessions {
        close_session_connection(session);
    }
}

/// Connects without keeping anything, for the 测试连接 button.
pub async fn test(config: &ConnectConfig) -> ApiResult<String> {
    let pool = driver::connect_pool(config, None).await?;
    let result = async {
        let mut conn = pool.acquire().await?;
        meta::server_version(&mut conn.as_mut()).await
    }
    .await;
    pool.close().await;
    result
}

// ---------------------------------------------------------------------------
// Browsing table data
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SortSpec {
    pub column: String,
    #[serde(default)]
    pub desc: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
pub struct FilterSpec {
    pub column: String,
    /// `eq`, `ne`, `gt`, `gte`, `lt`, `lte`, `contains`, `not_contains`,
    /// `starts_with`, `ends_with`, `is_null`, `not_null`, `is_empty` or
    /// `not_empty`.
    pub op: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RowsQuery {
    pub database: String,
    pub schema: Option<String>,
    pub table: String,
    pub page: u64,
    pub page_size: u64,
    pub sort: Vec<SortSpec>,
    pub filters: Vec<FilterSpec>,
    /// Hand-written `WHERE` condition; only for users allowed to run SQL.
    pub where_clause: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct RowsPage {
    pub columns: Vec<ColumnInfo>,
    pub primary_key: Vec<String>,
    #[schema(value_type = Vec<Vec<Object>>)]
    pub rows: Vec<Vec<Value>>,
    pub total: i64,
    pub page: u64,
    pub page_size: u64,
    /// The `SELECT` that produced the page.
    pub sql: String,
    pub read_only: bool,
}

fn find_column<'a>(columns: &'a [ColumnInfo], name: &str) -> ApiResult<&'a ColumnInfo> {
    columns
        .iter()
        .find(|column| column.name == name)
        .ok_or_else(|| ApiError::bad_request(format!("字段不存在：{name}")))
}

struct WhereBuilder {
    dialect: Dialect,
    parts: Vec<String>,
    binds: Vec<Bind>,
}

impl WhereBuilder {
    const fn new(dialect: Dialect) -> Self {
        Self {
            dialect,
            parts: Vec::new(),
            binds: Vec::new(),
        }
    }

    fn placeholder(&mut self, bind: Bind) -> String {
        self.binds.push(bind);
        self.dialect.placeholder(self.binds.len())
    }

    /// Placeholder for a value compared with or stored into `column`.
    fn typed_placeholder(&mut self, column: &ColumnInfo, bind: Bind) -> String {
        let placeholder = self.placeholder(bind);
        if self.dialect == Dialect::Postgres {
            format!("CAST({placeholder} AS {})", column.data_type)
        } else {
            placeholder
        }
    }

    fn as_text(&self, column: &ColumnInfo) -> String {
        let quoted = self.dialect.quote(&column.name);
        if self.dialect == Dialect::Postgres {
            format!("CAST({quoted} AS text)")
        } else {
            quoted
        }
    }

    fn like(&mut self, column: &ColumnInfo, pattern: String, negate: bool) -> String {
        let target = self.as_text(column);
        let placeholder = self.placeholder(Bind::Text(pattern));
        let operator = match (self.dialect, negate) {
            (Dialect::Postgres, false) => "ILIKE",
            (Dialect::Postgres, true) => "NOT ILIKE",
            (_, false) => "LIKE",
            (_, true) => "NOT LIKE",
        };
        let escape = if self.dialect == Dialect::Sqlite {
            " ESCAPE '\\'"
        } else {
            ""
        };
        format!("{target} {operator} {placeholder}{escape}")
    }

    fn filter(&mut self, column: &ColumnInfo, filter: &FilterSpec) -> ApiResult<()> {
        let quoted = self.dialect.quote(&column.name);
        let value = filter.value.clone().unwrap_or_default();
        let condition = match filter.op.as_str() {
            "is_null" => format!("{quoted} IS NULL"),
            "not_null" => format!("{quoted} IS NOT NULL"),
            "is_empty" => format!("({quoted} IS NULL OR {} = '')", self.as_text(column)),
            "not_empty" => format!("({quoted} IS NOT NULL AND {} <> '')", self.as_text(column)),
            "contains" => self.like(column, format!("%{}%", sql::like_escape(&value)), false),
            "not_contains" => self.like(column, format!("%{}%", sql::like_escape(&value)), true),
            "starts_with" => self.like(column, format!("{}%", sql::like_escape(&value)), false),
            "ends_with" => self.like(column, format!("%{}", sql::like_escape(&value)), false),
            op => {
                let operator = match op {
                    "eq" => "=",
                    "ne" => "<>",
                    "gt" => ">",
                    "gte" => ">=",
                    "lt" => "<",
                    "lte" => "<=",
                    _ => return Err(ApiError::bad_request(format!("不支持的筛选条件：{op}"))),
                };
                let placeholder = self.typed_placeholder(column, Bind::Text(value));
                format!("{quoted} {operator} {placeholder}")
            }
        };
        self.parts.push(condition);
        Ok(())
    }

    fn sql(&self) -> String {
        if self.parts.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", self.parts.join(" AND "))
        }
    }
}

struct PreparedSelect {
    columns: Vec<ColumnInfo>,
    primary_key: Vec<String>,
    select_list: String,
    /// `FROM … WHERE …`
    from: String,
    order: String,
    binds: Vec<Bind>,
}

async fn prepare_select(conn: &mut ConnMut<'_>, query: &RowsQuery) -> ApiResult<PreparedSelect> {
    let dialect = conn.dialect();
    let schema = query.schema.as_deref();
    let columns = meta::columns(conn, &query.database, schema, &query.table).await?;
    if columns.is_empty() {
        return Err(ApiError::not_found(format!("表不存在：{}", query.table))
            .with_code("DB_TABLE_NOT_FOUND"));
    }
    let primary_key = meta::primary_key(conn, &query.database, schema, &query.table).await?;
    let mut builder = WhereBuilder::new(dialect);
    for filter in &query.filters {
        let column = find_column(&columns, &filter.column)?;
        builder.filter(column, filter)?;
    }
    if let Some(condition) = query
        .where_clause
        .as_deref()
        .map(str::trim)
        .filter(|condition| !condition.is_empty())
    {
        if sql::split_statements(condition, dialect).len() > 1 {
            return Err(ApiError::bad_request("筛选条件只能是一个表达式"));
        }
        builder.parts.push(format!("({condition})"));
    }
    let mut order: Vec<String> = Vec::new();
    for sort in &query.sort {
        let column = find_column(&columns, &sort.column)?;
        order.push(format!(
            "{}{}",
            dialect.quote(&column.name),
            if sort.desc { " DESC" } else { "" }
        ));
    }
    if order.is_empty() {
        order = primary_key.iter().map(|name| dialect.quote(name)).collect();
    }
    let select_list = columns
        .iter()
        .map(|column| meta::select_expression(dialect, column))
        .collect::<Vec<_>>()
        .join(", ");
    let qualified = dialect.qualified(&query.database, schema, &query.table);
    Ok(PreparedSelect {
        select_list,
        from: format!("FROM {qualified}{}", builder.sql()),
        order: if order.is_empty() {
            String::new()
        } else {
            format!(" ORDER BY {}", order.join(", "))
        },
        binds: builder.binds,
        columns,
        primary_key,
    })
}

async fn count(conn: &mut ConnMut<'_>, prepared: &PreparedSelect) -> ApiResult<i64> {
    let rows = conn
        .rows(
            &format!("SELECT COUNT(*) {}", prepared.from),
            &prepared.binds,
        )
        .await?;
    Ok(rows
        .first()
        .and_then(|row| row.first())
        .and_then(driver::int)
        .unwrap_or(0))
}

pub async fn rows(target: &Target, query: &RowsQuery) -> ApiResult<RowsPage> {
    let pool = pool(target, &query.database).await?;
    let mut conn = pool.acquire().await?;
    let mut conn = conn.as_mut();
    let prepared = prepare_select(&mut conn, query).await?;
    let page_size = query.page_size.clamp(1, MAX_PAGE_SIZE);
    let page = query.page.max(1);
    let sql = format!(
        "SELECT {} {}{} LIMIT {page_size} OFFSET {}",
        prepared.select_list,
        prepared.from,
        prepared.order,
        (page - 1) * page_size
    );
    let collected = conn
        .run(
            &sql,
            &prepared.binds,
            usize::try_from(page_size).unwrap_or(usize::MAX),
            DecodeOptions::default(),
        )
        .await
        .map_err(|error| query_error(&error))?;
    let total = count(&mut conn, &prepared).await?;
    Ok(RowsPage {
        columns: prepared.columns,
        primary_key: prepared.primary_key,
        rows: collected.rows,
        total,
        page,
        page_size,
        sql,
        read_only: target.config.read_only,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Csv,
    Json,
    Sql,
}

impl ExportFormat {
    pub fn parse(value: &str) -> ApiResult<Self> {
        match value {
            "csv" => Ok(Self::Csv),
            "json" => Ok(Self::Json),
            "sql" => Ok(Self::Sql),
            _ => Err(ApiError::bad_request(format!("不支持的导出格式：{value}"))),
        }
    }
}

pub struct Export {
    pub columns: Vec<ColumnInfo>,
    pub rows: Vec<Vec<Value>>,
}

/// Every row matching the filters (up to [`MAX_EXPORT_ROWS`]).
pub async fn export_rows(target: &Target, query: &RowsQuery) -> ApiResult<Export> {
    let pool = pool(target, &query.database).await?;
    let mut conn = pool.acquire().await?;
    let mut conn = conn.as_mut();
    let prepared = prepare_select(&mut conn, query).await?;
    let total = count(&mut conn, &prepared).await?;
    if total > MAX_EXPORT_ROWS {
        return Err(ApiError::bad_request(format!(
            "符合条件的数据有 {total} 条，超过单次导出上限 {MAX_EXPORT_ROWS} 条，请增加筛选条件"
        )));
    }
    let sql = format!(
        "SELECT {} {}{}",
        prepared.select_list, prepared.from, prepared.order
    );
    let collected = conn
        .run(
            &sql,
            &prepared.binds,
            usize::try_from(MAX_EXPORT_ROWS).unwrap_or(usize::MAX),
            DecodeOptions {
                text_bytes: false,
                full_bytes: true,
            },
        )
        .await
        .map_err(|error| query_error(&error))?;
    Ok(Export {
        columns: prepared.columns,
        rows: collected.rows,
    })
}

/// `INSERT` statements for exported rows.
#[must_use]
pub fn insert_script(dialect: Dialect, qualified: &str, export: &Export) -> String {
    let names = export
        .columns
        .iter()
        .map(|column| dialect.quote(&column.name))
        .collect::<Vec<_>>()
        .join(", ");
    let mut script = String::new();
    for row in &export.rows {
        let values = row
            .iter()
            .zip(&export.columns)
            .map(|(value, column)| dialect.literal(value, column.binary))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(
            script,
            "INSERT INTO {qualified} ({names}) VALUES ({values});"
        );
    }
    script
}

// ---------------------------------------------------------------------------
// Editing rows
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
pub struct RowChange {
    /// insert | update | delete
    pub kind: String,
    /// Primary key values of the row (update / delete).
    #[schema(value_type = Option<Object>)]
    pub key: Option<Map<String, Value>>,
    /// New column values (insert / update).
    #[schema(value_type = Option<Object>)]
    pub values: Option<Map<String, Value>>,
}

#[derive(Debug, Default, Serialize, utoipa::ToSchema)]
pub struct ChangeSummary {
    pub inserted: u64,
    pub updated: u64,
    pub deleted: u64,
}

fn value_bind(dialect: Dialect, value: &Value) -> Bind {
    match (dialect, value) {
        (_, Value::Null) => Bind::Null,
        (Dialect::Postgres, Value::Bool(flag)) => Bind::Text(flag.to_string()),
        (Dialect::Postgres, Value::Number(number)) => Bind::Text(number.to_string()),
        (_, Value::Bool(flag)) => Bind::Bool(*flag),
        (_, Value::Number(number)) => number
            .as_i64()
            .map(Bind::Int)
            .or_else(|| number.as_f64().map(Bind::Float))
            .unwrap_or_else(|| Bind::Text(number.to_string())),
        (_, Value::String(text)) => Bind::Text(text.clone()),
        (_, other) => Bind::Text(other.to_string()),
    }
}

fn change_statement(
    dialect: Dialect,
    qualified: &str,
    columns: &[ColumnInfo],
    primary_key: &[String],
    change: &RowChange,
) -> ApiResult<Option<(String, Vec<Bind>, bool)>> {
    let mut builder = WhereBuilder::new(dialect);
    let empty = Map::new();
    let values = change.values.as_ref().unwrap_or(&empty);
    let key_condition = |builder: &mut WhereBuilder| -> ApiResult<String> {
        if primary_key.is_empty() {
            return Err(ApiError::bad_request(
                "该表没有主键，无法直接修改或删除数据，请使用 SQL 编辑器",
            ));
        }
        let key = change
            .key
            .as_ref()
            .ok_or_else(|| ApiError::bad_request("缺少要修改的行的主键"))?;
        let mut parts = Vec::new();
        for name in primary_key {
            let column = find_column(columns, name)?;
            let value = key
                .get(name)
                .ok_or_else(|| ApiError::bad_request(format!("缺少主键字段：{name}")))?;
            if value.is_null() {
                parts.push(format!("{} IS NULL", dialect.quote(name)));
            } else {
                let placeholder = builder.typed_placeholder(column, value_bind(dialect, value));
                parts.push(format!("{} = {placeholder}", dialect.quote(name)));
            }
        }
        Ok(parts.join(" AND "))
    };
    let statement = match change.kind.as_str() {
        "insert" => {
            if values.is_empty() {
                let sql = if dialect == Dialect::MySql {
                    format!("INSERT INTO {qualified} () VALUES ()")
                } else {
                    format!("INSERT INTO {qualified} DEFAULT VALUES")
                };
                return Ok(Some((sql, Vec::new(), false)));
            }
            let mut names = Vec::new();
            let mut placeholders = Vec::new();
            for (name, value) in values {
                let column = find_column(columns, name)?;
                names.push(dialect.quote(name));
                placeholders.push(builder.typed_placeholder(column, value_bind(dialect, value)));
            }
            (
                format!(
                    "INSERT INTO {qualified} ({}) VALUES ({})",
                    names.join(", "),
                    placeholders.join(", ")
                ),
                false,
            )
        }
        "update" => {
            if values.is_empty() {
                return Ok(None);
            }
            let mut assignments = Vec::new();
            for (name, value) in values {
                let column = find_column(columns, name)?;
                let placeholder = builder.typed_placeholder(column, value_bind(dialect, value));
                assignments.push(format!("{} = {placeholder}", dialect.quote(name)));
            }
            let condition = key_condition(&mut builder)?;
            (
                format!(
                    "UPDATE {qualified} SET {} WHERE {condition}",
                    assignments.join(", ")
                ),
                true,
            )
        }
        "delete" => {
            let condition = key_condition(&mut builder)?;
            (format!("DELETE FROM {qualified} WHERE {condition}"), true)
        }
        kind => return Err(ApiError::bad_request(format!("不支持的修改类型：{kind}"))),
    };
    Ok(Some((statement.0, builder.binds, statement.1)))
}

/// Applies grid edits in one transaction; updates and deletes must each hit
/// exactly one row, otherwise nothing is saved.
pub async fn apply_changes(
    target: &Target,
    database: &str,
    schema: Option<&str>,
    table: &str,
    changes: &[RowChange],
) -> ApiResult<ChangeSummary> {
    if target.config.read_only {
        return Err(ApiError::bad_request("只读连接不能修改数据").with_code("DB_READ_ONLY"));
    }
    if changes.is_empty() {
        return Ok(ChangeSummary::default());
    }
    if changes.len() > MAX_CHANGES {
        return Err(ApiError::bad_request(format!(
            "一次最多提交 {MAX_CHANGES} 条修改"
        )));
    }
    let pool = pool(target, database).await?;
    let mut pooled = pool.acquire().await?;
    let mut conn = pooled.as_mut();
    let dialect = conn.dialect();
    let columns = meta::columns(&mut conn, database, schema, table).await?;
    if columns.is_empty() {
        return Err(
            ApiError::not_found(format!("表不存在：{table}")).with_code("DB_TABLE_NOT_FOUND")
        );
    }
    let primary_key = meta::primary_key(&mut conn, database, schema, table).await?;
    let qualified = dialect.qualified(database, schema, table);
    let mut statements = Vec::new();
    for change in changes {
        if let Some(statement) =
            change_statement(dialect, &qualified, &columns, &primary_key, change)?
        {
            statements.push((change.kind.as_str(), statement));
        }
    }

    conn.execute_raw("BEGIN")
        .await
        .map_err(|error| query_error(&error))?;
    let mut summary = ChangeSummary::default();
    for (index, (kind, (sql, binds, single_row))) in statements.iter().enumerate() {
        let result = conn.run(sql, binds, 0, DecodeOptions::default()).await;
        let failure = match result {
            Ok(collected) if *single_row && collected.affected != 1 => {
                Some(if collected.affected == 0 {
                    "记录不存在或已被修改，请刷新后重试".to_string()
                } else {
                    format!("会影响 {} 行，已取消", collected.affected)
                })
            }
            Ok(collected) => {
                match *kind {
                    "insert" => summary.inserted += collected.affected.max(1),
                    "update" => summary.updated += collected.affected,
                    _ => summary.deleted += collected.affected,
                }
                None
            }
            Err(error) => Some(error_text(&error)),
        };
        if let Some(message) = failure {
            let _ = conn.execute_raw("ROLLBACK").await;
            return Err(
                ApiError::bad_request(format!("第 {} 条修改失败：{message}", index + 1))
                    .with_code("DB_QUERY_FAILED"),
            );
        }
    }
    if let Err(error) = conn.execute_raw("COMMIT").await {
        let _ = conn.execute_raw("ROLLBACK").await;
        return Err(query_error(&error));
    }
    Ok(summary)
}

// ---------------------------------------------------------------------------
// SQL editor sessions
// ---------------------------------------------------------------------------

struct EditorSession {
    connection_id: i32,
    database: String,
    fingerprint: String,
    /// `None` after the connection was dropped (timeout) or broke.
    conn: tokio::sync::Mutex<Option<DbConn>>,
    /// Server-side id used to cancel a running statement.
    backend_id: Mutex<Option<i64>>,
    in_transaction: AtomicBool,
    last_used: Mutex<Instant>,
}

fn sessions() -> &'static Mutex<HashMap<String, Arc<EditorSession>>> {
    static SESSIONS: OnceLock<Mutex<HashMap<String, Arc<EditorSession>>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn session_map_key(user_id: i32, key: &str) -> String {
    format!("{user_id}:{key}")
}

fn close_session_connection(session: Arc<EditorSession>) {
    tokio::spawn(async move {
        let conn = session.conn.lock().await.take();
        if let Some(conn) = conn {
            conn.close().await;
        }
    });
}

fn sweep_sessions() {
    let stale: Vec<Arc<EditorSession>> = {
        let mut sessions = sessions()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let keys: Vec<String> = sessions
            .iter()
            .filter(|(_, session)| {
                session
                    .last_used
                    .lock()
                    .map(|last| last.elapsed() > IDLE_LIMIT)
                    .unwrap_or(true)
            })
            .map(|(key, _)| key.clone())
            .collect();
        let removed = keys.iter().filter_map(|key| sessions.remove(key)).collect();
        drop(sessions);
        removed
    };
    for session in stale {
        close_session_connection(session);
    }
}

pub fn validate_session_key(key: &str) -> ApiResult<()> {
    if key.is_empty()
        || key.len() > 64
        || !key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(ApiError::bad_request("无效的查询窗口标识"));
    }
    Ok(())
}

/// Closes an editor tab's connection; an open transaction is rolled back.
pub fn close_session(user_id: i32, key: &str) -> bool {
    let removed = sessions()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&session_map_key(user_id, key));
    removed.map(close_session_connection).is_some()
}

async fn open_session_connection(
    target: &Target,
    database: &str,
) -> ApiResult<(DbConn, Option<i64>)> {
    let pool = pool(target, database).await?;
    let mut pooled = pool.acquire().await?;
    let id_query = match target.dialect() {
        Dialect::Postgres => Some("SELECT pg_backend_pid()"),
        Dialect::MySql => Some("SELECT CONNECTION_ID()"),
        Dialect::Sqlite => None,
    };
    let backend_id = match id_query {
        Some(sql) => pooled
            .as_mut()
            .rows(sql, &[])
            .await?
            .first()
            .and_then(|row| row.first())
            .and_then(driver::int),
        None => None,
    };
    Ok((pooled.detach(), backend_id))
}

// The map stays locked while the session is looked up and replaced.
#[allow(clippy::significant_drop_tightening)]
fn session_for(target: &Target, key: &str, database: &str) -> Arc<EditorSession> {
    let fingerprint = target.fingerprint();
    let map_key = session_map_key(target.user_id, key);
    let mut sessions = sessions()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(existing) = sessions.get(&map_key) {
        if existing.connection_id == target.connection_id
            && existing.database == database
            && existing.fingerprint == fingerprint
        {
            return existing.clone();
        }
        if let Some(old) = sessions.remove(&map_key) {
            close_session_connection(old);
        }
    }
    let session = Arc::new(EditorSession {
        connection_id: target.connection_id,
        database: database.to_string(),
        fingerprint,
        conn: tokio::sync::Mutex::new(None),
        backend_id: Mutex::new(None),
        in_transaction: AtomicBool::new(false),
        last_used: Mutex::new(Instant::now()),
    });
    sessions.insert(map_key, session.clone());
    session
}

#[derive(Debug, Clone, Copy)]
pub struct ExecuteOptions {
    pub max_rows: usize,
    pub timeout: Duration,
    pub stop_on_error: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct StatementResult {
    pub sql: String,
    /// Whether the statement produced a result set (possibly empty).
    pub returns_rows: bool,
    pub columns: Vec<ResultColumn>,
    #[schema(value_type = Vec<Vec<Object>>)]
    pub rows: Vec<Vec<Value>>,
    pub truncated: bool,
    pub affected_rows: u64,
    pub duration_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ExecuteResult {
    pub statements: Vec<StatementResult>,
    /// Statements found in the script; later ones are skipped after an error.
    pub statement_count: usize,
    pub in_transaction: bool,
    pub duration_ms: u64,
}

impl ExecuteResult {
    #[must_use]
    pub fn first_error(&self) -> Option<&str> {
        self.statements
            .iter()
            .find_map(|statement| statement.error.as_deref())
    }

    #[must_use]
    pub fn affected_rows(&self) -> u64 {
        self.statements
            .iter()
            .map(|statement| statement.affected_rows)
            .sum()
    }
}

/// Transaction control and session statements, which affect no rows.
fn is_control_statement(statement: &str) -> bool {
    matches!(
        sql::leading_keyword(statement).as_str(),
        "BEGIN" | "START" | "COMMIT" | "END" | "ROLLBACK" | "SAVEPOINT" | "RELEASE" | "SET" | "USE"
    )
}

/// Transaction state after a statement, for drivers that do not report it.
fn track_transaction(statement: &str, dialect: Dialect, current: bool) -> bool {
    let keyword = sql::leading_keyword(statement);
    let upper = statement.trim().to_ascii_uppercase();
    match keyword.as_str() {
        "BEGIN" | "START" | "SAVEPOINT" => true,
        "COMMIT" | "END" => false,
        "ROLLBACK" => upper.contains(" TO "),
        // MySQL commits implicitly before most DDL.
        "CREATE" | "ALTER" | "DROP" | "TRUNCATE" | "RENAME" if dialect == Dialect::MySql => false,
        _ => current,
    }
}

fn millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Runs a script in an editor tab's own connection.
#[allow(clippy::too_many_lines)]
pub async fn execute(
    target: &Target,
    database: &str,
    key: &str,
    script: &str,
    options: ExecuteOptions,
) -> ApiResult<ExecuteResult> {
    validate_session_key(key)?;
    let dialect = target.dialect();
    let statements = sql::split_statements(script, dialect);
    if statements.is_empty() {
        return Err(ApiError::bad_request("没有可执行的 SQL 语句"));
    }
    if statements.len() > MAX_STATEMENTS {
        return Err(ApiError::bad_request(format!(
            "一次最多执行 {MAX_STATEMENTS} 条语句"
        )));
    }
    let session = session_for(target, key, database);
    let Ok(mut guard) = session.conn.try_lock() else {
        return Err(
            ApiError::bad_request("该查询窗口仍在执行上一段 SQL").with_code("DB_SESSION_BUSY")
        );
    };
    if let Ok(mut last) = session.last_used.lock() {
        *last = Instant::now();
    }
    let alive = match guard.as_mut() {
        Some(conn) => conn.as_mut().ping().await,
        None => false,
    };
    if !alive {
        if let Some(conn) = guard.take() {
            conn.close().await;
        }
        let (conn, backend_id) = open_session_connection(target, database).await?;
        *guard = Some(conn);
        if let Ok(mut id) = session.backend_id.lock() {
            *id = backend_id;
        }
        session.in_transaction.store(false, Ordering::Relaxed);
    }

    let started = Instant::now();
    let mut results = Vec::new();
    let mut in_transaction = session.in_transaction.load(Ordering::Relaxed);
    let mut dropped = false;
    for statement in &statements {
        let Some(conn) = guard.as_mut() else { break };
        let mut conn = conn.as_mut();
        let statement_started = Instant::now();
        let outcome = tokio::time::timeout(
            options.timeout,
            conn.run(statement, &[], options.max_rows, DecodeOptions::default()),
        )
        .await;
        let mut result = StatementResult {
            sql: statement.clone(),
            returns_rows: false,
            columns: Vec::new(),
            rows: Vec::new(),
            truncated: false,
            affected_rows: 0,
            duration_ms: 0,
            error: None,
        };
        match outcome {
            Ok(Ok(collected)) => {
                let columns = match collected.columns {
                    Some(columns) => Some(columns),
                    None if sql::returns_rows(statement) => conn.describe(statement).await,
                    None => None,
                };
                result.returns_rows = columns.as_ref().is_some_and(|columns| !columns.is_empty());
                result.columns = columns.unwrap_or_default();
                result.rows = collected.rows;
                result.truncated = collected.truncated;
                // SQLite repeats the previous change count for statements
                // that change nothing, such as `COMMIT`.
                result.affected_rows = if result.returns_rows || is_control_statement(statement) {
                    0
                } else {
                    collected.affected
                };
                in_transaction = track_transaction(statement, dialect, in_transaction);
            }
            Ok(Err(error)) => result.error = Some(error_text(&error)),
            Err(_) => {
                result.error = Some(format!(
                    "执行超过 {} 秒已取消，该查询窗口的连接已断开（未提交的事务已回滚）",
                    options.timeout.as_secs()
                ));
                dropped = true;
            }
        }
        result.duration_ms = millis(statement_started);
        let failed = result.error.is_some();
        results.push(result);
        if dropped || (failed && options.stop_on_error) {
            break;
        }
    }

    if dropped {
        let backend_id = session.backend_id.lock().ok().and_then(|id| *id);
        if let Some(conn) = guard.take() {
            drop(conn);
        }
        cancel_backend(target, database, backend_id).await;
        in_transaction = false;
    } else if let Some(conn) = guard.as_mut() {
        let mut conn = conn.as_mut();
        if dialect == Dialect::Postgres {
            // Inside a transaction `now()` is when it began, not this statement.
            // An error means "current transaction is aborted".
            in_transaction = conn
                .simple_rows("SELECT now() <> statement_timestamp()")
                .await
                .map_or(true, |rows| {
                    rows.first()
                        .and_then(|row| row.first())
                        .is_some_and(driver::flag)
                });
        }
        conn.clear_cached_statements().await;
    }
    session
        .in_transaction
        .store(in_transaction, Ordering::Relaxed);
    if let Ok(mut last) = session.last_used.lock() {
        *last = Instant::now();
    }
    Ok(ExecuteResult {
        statements: results,
        statement_count: statements.len(),
        in_transaction,
        duration_ms: millis(started),
    })
}

async fn cancel_backend(target: &Target, database: &str, backend_id: Option<i64>) {
    let Some(backend_id) = backend_id else { return };
    let Ok(pool) = pool(target, database).await else {
        return;
    };
    let Ok(mut pooled) = pool.acquire().await else {
        return;
    };
    let mut conn = pooled.as_mut();
    let _ = match target.dialect() {
        Dialect::Postgres => conn
            .rows(
                "SELECT pg_cancel_backend(CAST($1 AS integer))",
                &[Bind::Text(backend_id.to_string())],
            )
            .await
            .map(|_| ()),
        Dialect::MySql => conn
            .execute_raw(&format!("KILL QUERY {backend_id}"))
            .await
            .map_err(|error| query_error(&error)),
        Dialect::Sqlite => Ok(()),
    };
}

/// Stops the statement running in an editor tab (PostgreSQL / `MySQL`).
pub async fn cancel(target: &Target, key: &str) -> ApiResult<bool> {
    if target.dialect() == Dialect::Sqlite {
        return Err(ApiError::bad_request("SQLite 不支持取消正在执行的语句"));
    }
    let session = sessions()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&session_map_key(target.user_id, key))
        .filter(|session| session.connection_id == target.connection_id)
        .cloned();
    let Some(session) = session else {
        return Ok(false);
    };
    let backend_id = session.backend_id.lock().ok().and_then(|id| *id);
    cancel_backend(target, &session.database, backend_id).await;
    Ok(backend_id.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str, data_type: &str) -> ColumnInfo {
        ColumnInfo {
            name: name.to_string(),
            data_type: data_type.to_string(),
            base_type: data_type.to_string(),
            nullable: true,
            default_value: None,
            comment: None,
            primary_key: name == "id",
            auto_increment: false,
            binary: false,
        }
    }

    #[test]
    fn filters_bind_values_and_cast_them_on_postgres() {
        let id = column("id", "integer");
        let name = column("name", "text");
        let mut builder = WhereBuilder::new(Dialect::Postgres);
        builder
            .filter(
                &id,
                &FilterSpec {
                    column: "id".into(),
                    op: "gte".into(),
                    value: Some("5".into()),
                },
            )
            .unwrap();
        builder
            .filter(
                &name,
                &FilterSpec {
                    column: "name".into(),
                    op: "contains".into(),
                    value: Some("50%".into()),
                },
            )
            .unwrap();
        assert_eq!(
            builder.sql(),
            " WHERE \"id\" >= CAST($1 AS integer) AND CAST(\"name\" AS text) ILIKE $2"
        );
        assert!(matches!(&builder.binds[1], Bind::Text(pattern) if pattern == "%50\\%%"));
        assert!(builder
            .filter(
                &id,
                &FilterSpec {
                    column: "id".into(),
                    op: "drop".into(),
                    value: None,
                },
            )
            .is_err());

        let mut sqlite = WhereBuilder::new(Dialect::Sqlite);
        sqlite
            .filter(
                &name,
                &FilterSpec {
                    column: "name".into(),
                    op: "starts_with".into(),
                    value: Some("a".into()),
                },
            )
            .unwrap();
        assert_eq!(sqlite.sql(), " WHERE \"name\" LIKE ? ESCAPE '\\'");
    }

    #[test]
    fn row_changes_need_a_primary_key_and_known_columns() {
        let columns = vec![column("id", "integer"), column("name", "text")];
        let primary = vec!["id".to_string()];
        let mut values = Map::new();
        values.insert("name".into(), Value::String("x".into()));
        let mut key = Map::new();
        key.insert("id".into(), Value::from(7));
        let update = RowChange {
            kind: "update".into(),
            key: Some(key.clone()),
            values: Some(values.clone()),
        };
        let (sql, binds, single) =
            change_statement(Dialect::Postgres, "\"t\"", &columns, &primary, &update)
                .unwrap()
                .unwrap();
        assert_eq!(
            sql,
            "UPDATE \"t\" SET \"name\" = CAST($1 AS text) WHERE \"id\" = CAST($2 AS integer)"
        );
        assert!(single);
        assert!(matches!(&binds[1], Bind::Text(id) if id == "7"));

        let (sql, binds, _) =
            change_statement(Dialect::MySql, "`d`.`t`", &columns, &primary, &update)
                .unwrap()
                .unwrap();
        assert_eq!(sql, "UPDATE `d`.`t` SET `name` = ? WHERE `id` = ?");
        assert!(matches!(binds[1], Bind::Int(7)));

        assert!(change_statement(Dialect::MySql, "t", &columns, &[], &update).is_err());
        let mut unknown = values;
        unknown.insert("missing".into(), Value::Null);
        let insert = RowChange {
            kind: "insert".into(),
            key: None,
            values: Some(unknown),
        };
        assert!(change_statement(Dialect::Sqlite, "t", &columns, &primary, &insert).is_err());
        let delete = RowChange {
            kind: "delete".into(),
            key: Some(key),
            values: None,
        };
        let (sql, _, _) = change_statement(Dialect::Sqlite, "\"t\"", &columns, &primary, &delete)
            .unwrap()
            .unwrap();
        assert_eq!(sql, "DELETE FROM \"t\" WHERE \"id\" = ?");
    }

    #[test]
    fn transaction_tracking_follows_statements() {
        assert!(track_transaction("BEGIN", Dialect::MySql, false));
        assert!(track_transaction(
            "start transaction",
            Dialect::MySql,
            false
        ));
        assert!(track_transaction(
            "ROLLBACK TO SAVEPOINT a",
            Dialect::Sqlite,
            true
        ));
        assert!(!track_transaction("rollback", Dialect::Sqlite, true));
        assert!(!track_transaction(
            "CREATE TABLE t (id int)",
            Dialect::MySql,
            true
        ));
        assert!(track_transaction(
            "CREATE TABLE t (id int)",
            Dialect::Sqlite,
            true
        ));
        assert!(track_transaction(
            "UPDATE t SET a = 1",
            Dialect::MySql,
            true
        ));
    }

    #[test]
    fn session_keys_are_short_and_plain() {
        assert!(validate_session_key("tab-1_a").is_ok());
        assert!(validate_session_key("").is_err());
        assert!(validate_session_key("a:b").is_err());
        assert!(validate_session_key(&"x".repeat(65)).is_err());
    }
}
