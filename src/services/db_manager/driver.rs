//! Connections to the managed databases and turning their rows into JSON.
//!
//! Every cell becomes `null`, a boolean, a number (integers only while they
//! fit a JavaScript number) or a string; binary values are shown as `0x…`.

use std::{fmt::Write as _, str::FromStr, time::Duration};

use futures_util::{Stream, TryStreamExt};
use sea_orm::sqlx::{
    self,
    mysql::{
        MySqlConnectOptions, MySqlConnection, MySqlDatabaseError, MySqlPool, MySqlPoolOptions,
        MySqlQueryResult, MySqlRow, MySqlSslMode,
    },
    pool::PoolConnection,
    postgres::{
        types::{Oid, PgInterval, PgMoney, PgTimeTz},
        PgConnectOptions, PgConnection, PgPool, PgPoolOptions, PgQueryResult, PgRow, PgSslMode,
    },
    sqlite::{
        SqliteConnectOptions, SqliteConnection, SqlitePool, SqlitePoolOptions, SqliteQueryResult,
        SqliteRow,
    },
    Column as _, Connection as _, Either, Executor as _, MySql, Postgres, Row as _, Sqlite,
    TypeInfo as _, ValueRef as _,
};
use serde::Serialize;
use serde_json::Value;

use super::sql::Dialect;
use crate::errors::{ApiError, ApiResult};

const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
/// Binary cells show this many bytes in the grid.
const BINARY_PREVIEW_BYTES: usize = 64;
const POOL_SIZE: u32 = 4;
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(15);
const IDLE_TIMEOUT: Duration = Duration::from_secs(300);
const APPLICATION_NAME: &str = "gpt_images-db-manager";

#[derive(Debug, Clone, Serialize)]
pub struct ConnectConfig {
    pub dialect: Dialect,
    pub host: String,
    pub port: u16,
    pub username: String,
    #[serde(skip)]
    pub password: Option<String>,
    /// Default database; the file for `SQLite`.
    pub database: Option<String>,
    pub ssl_mode: Option<String>,
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct ResultColumn {
    pub name: String,
    pub type_name: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DecodeOptions {
    /// Metadata queries: binary strings are text.
    pub text_bytes: bool,
    /// Exports: binary values are not shortened.
    pub full_bytes: bool,
}

#[derive(Debug, Clone)]
pub enum Bind {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
}

impl From<&str> for Bind {
    fn from(value: &str) -> Self {
        Self::Text(value.to_string())
    }
}

#[derive(Debug, Default)]
pub struct Collected {
    pub columns: Option<Vec<ResultColumn>>,
    pub rows: Vec<Vec<Value>>,
    pub affected: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone)]
pub enum DbPool {
    Postgres(PgPool),
    MySql(MySqlPool),
    Sqlite(SqlitePool),
}

pub enum PooledConn {
    Postgres(PoolConnection<Postgres>),
    MySql(PoolConnection<MySql>),
    Sqlite(PoolConnection<Sqlite>),
}

/// A connection taken out of its pool, owned by one SQL editor tab.
pub enum DbConn {
    Postgres(PgConnection),
    MySql(MySqlConnection),
    Sqlite(SqliteConnection),
}

pub enum ConnMut<'a> {
    Postgres(&'a mut PgConnection),
    MySql(&'a mut MySqlConnection),
    Sqlite(&'a mut SqliteConnection),
}

/// Runs `$body` with `$c` bound to the concrete `&mut` connection.
macro_rules! with_conn {
    ($conn:expr, |$c:ident| $body:expr) => {
        match $conn {
            ConnMut::Postgres($c) => $body,
            ConnMut::MySql($c) => $body,
            ConnMut::Sqlite($c) => $body,
        }
    };
}

macro_rules! bind_all {
    ($query:expr, $binds:expr) => {{
        let mut query = $query;
        for bind in $binds {
            query = match bind {
                Bind::Null => query.bind(None::<String>),
                Bind::Bool(value) => query.bind(*value),
                Bind::Int(value) => query.bind(*value),
                Bind::Float(value) => query.bind(*value),
                Bind::Text(value) => query.bind(value.clone()),
            };
        }
        query
    }};
}

fn connect_error(error: &sqlx::Error) -> ApiError {
    ApiError::bad_request(format!("连接数据库失败：{error}")).with_code("DB_CONNECT_FAILED")
}

#[allow(clippy::too_many_lines)]
pub async fn connect_pool(config: &ConnectConfig, database: Option<&str>) -> ApiResult<DbPool> {
    let database = database
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .or_else(|| config.database.clone().filter(|name| !name.is_empty()));
    match config.dialect {
        Dialect::Postgres => {
            let mut options = PgConnectOptions::new_without_pgpass()
                .host(&config.host)
                .port(config.port)
                .username(&config.username)
                .password(config.password.as_deref().unwrap_or_default())
                .database(database.as_deref().unwrap_or("postgres"))
                .application_name(APPLICATION_NAME);
            if let Some(mode) = config.ssl_mode.as_deref().filter(|mode| !mode.is_empty()) {
                options = options.ssl_mode(
                    PgSslMode::from_str(mode)
                        .map_err(|_| ApiError::bad_request(format!("无效的 SSL 模式：{mode}")))?,
                );
            }
            if config.read_only {
                options = options.options([("default_transaction_read_only", "on")]);
            }
            PgPoolOptions::new()
                .max_connections(POOL_SIZE)
                .min_connections(0)
                .acquire_timeout(ACQUIRE_TIMEOUT)
                .idle_timeout(IDLE_TIMEOUT)
                .connect_with(options)
                .await
                .map(DbPool::Postgres)
                .map_err(|error| connect_error(&error))
        }
        Dialect::MySql => {
            let mut options = MySqlConnectOptions::new()
                .host(&config.host)
                .port(config.port)
                .username(&config.username)
                .charset("utf8mb4")
                // Keep the server's own time zone and SQL mode.
                .timezone(None)
                .pipes_as_concat(false)
                .no_engine_substitution(false);
            if let Some(password) = config.password.as_deref().filter(|value| !value.is_empty()) {
                options = options.password(password);
            }
            if let Some(database) = database.as_deref() {
                options = options.database(database);
            }
            if let Some(mode) = config.ssl_mode.as_deref().filter(|mode| !mode.is_empty()) {
                options = options.ssl_mode(
                    MySqlSslMode::from_str(mode)
                        .map_err(|_| ApiError::bad_request(format!("无效的 SSL 模式：{mode}")))?,
                );
            }
            let mut pool = MySqlPoolOptions::new()
                .max_connections(POOL_SIZE)
                .min_connections(0)
                .acquire_timeout(ACQUIRE_TIMEOUT)
                .idle_timeout(IDLE_TIMEOUT);
            if config.read_only {
                pool = pool.after_connect(|conn, _| {
                    Box::pin(async move {
                        conn.execute("SET SESSION TRANSACTION READ ONLY").await?;
                        Ok(())
                    })
                });
            }
            pool.connect_with(options)
                .await
                .map(DbPool::MySql)
                .map_err(|error| connect_error(&error))
        }
        Dialect::Sqlite => {
            let path = config
                .database
                .as_deref()
                .filter(|path| !path.trim().is_empty())
                .ok_or_else(|| ApiError::bad_request("请填写 SQLite 数据库文件路径"))?;
            if !std::path::Path::new(path).is_file() {
                return Err(
                    ApiError::bad_request(format!("SQLite 数据库文件不存在：{path}"))
                        .with_code("DB_CONNECT_FAILED"),
                );
            }
            let options = SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(false)
                .read_only(config.read_only)
                .busy_timeout(Duration::from_secs(5));
            SqlitePoolOptions::new()
                .max_connections(POOL_SIZE)
                .min_connections(0)
                .acquire_timeout(ACQUIRE_TIMEOUT)
                .idle_timeout(IDLE_TIMEOUT)
                .connect_with(options)
                .await
                .map(DbPool::Sqlite)
                .map_err(|error| connect_error(&error))
        }
    }
}

impl DbPool {
    pub async fn acquire(&self) -> ApiResult<PooledConn> {
        let result = match self {
            Self::Postgres(pool) => pool.acquire().await.map(PooledConn::Postgres),
            Self::MySql(pool) => pool.acquire().await.map(PooledConn::MySql),
            Self::Sqlite(pool) => pool.acquire().await.map(PooledConn::Sqlite),
        };
        result.map_err(|error| connect_error(&error))
    }

    pub async fn close(&self) {
        match self {
            Self::Postgres(pool) => pool.close().await,
            Self::MySql(pool) => pool.close().await,
            Self::Sqlite(pool) => pool.close().await,
        }
    }
}

impl PooledConn {
    pub fn as_mut(&mut self) -> ConnMut<'_> {
        match self {
            Self::Postgres(conn) => ConnMut::Postgres(conn),
            Self::MySql(conn) => ConnMut::MySql(conn),
            Self::Sqlite(conn) => ConnMut::Sqlite(conn),
        }
    }

    /// Takes the connection out of the pool for an editor session.
    #[must_use]
    pub fn detach(self) -> DbConn {
        match self {
            Self::Postgres(conn) => DbConn::Postgres(conn.detach()),
            Self::MySql(conn) => DbConn::MySql(conn.detach()),
            Self::Sqlite(conn) => DbConn::Sqlite(conn.detach()),
        }
    }
}

impl DbConn {
    pub const fn as_mut(&mut self) -> ConnMut<'_> {
        match self {
            Self::Postgres(conn) => ConnMut::Postgres(conn),
            Self::MySql(conn) => ConnMut::MySql(conn),
            Self::Sqlite(conn) => ConnMut::Sqlite(conn),
        }
    }

    pub async fn close(self) {
        let _ = match self {
            Self::Postgres(conn) => conn.close().await,
            Self::MySql(conn) => conn.close().await,
            Self::Sqlite(conn) => conn.close().await,
        };
    }
}

impl ConnMut<'_> {
    #[must_use]
    pub const fn dialect(&self) -> Dialect {
        match self {
            Self::Postgres(_) => Dialect::Postgres,
            Self::MySql(_) => Dialect::MySql,
            Self::Sqlite(_) => Dialect::Sqlite,
        }
    }

    /// Runs one statement, keeping at most `max_rows` rows.
    // `fetch_many` is deprecated for multi-statement queries only; scripts are
    // split into single statements first and it is the one call that yields
    // both rows and affected-row counts.
    #[allow(deprecated)]
    pub async fn run(
        &mut self,
        sql: &str,
        binds: &[Bind],
        max_rows: usize,
        options: DecodeOptions,
    ) -> Result<Collected, sqlx::Error> {
        let result = with_conn!(self, |c| {
            let stream = bind_all!(sqlx::query(sql).persistent(false), binds).fetch_many(&mut **c);
            collect(stream, max_rows, options).await
        });
        match result {
            // Statements MySQL cannot prepare run as plain text instead.
            Err(error) if binds.is_empty() && needs_text_protocol(&error) => {
                with_conn!(self, |c| {
                    let stream = sqlx::raw_sql(sql).fetch_many(&mut **c);
                    collect(stream, max_rows, options).await
                })
            }
            other => other,
        }
    }

    /// Runs a statement and returns its rows (metadata queries).
    pub async fn rows(&mut self, sql: &str, binds: &[Bind]) -> ApiResult<Vec<Vec<Value>>> {
        self.run(
            sql,
            binds,
            usize::MAX,
            DecodeOptions {
                text_bytes: true,
                full_bytes: false,
            },
        )
        .await
        .map(|collected| collected.rows)
        .map_err(|error| query_error(&error))
    }

    /// Plain-text statements such as `BEGIN`, `COMMIT` or `KILL QUERY 7`.
    pub async fn execute_raw(&mut self, sql: &str) -> Result<(), sqlx::Error> {
        // A bare `&str` has no arguments and goes over the simple query protocol.
        with_conn!(self, |c| (&mut **c).execute(sql).await.map(|_| ()))
    }

    /// Rows of a statement sent over the simple query protocol (one message,
    /// so PostgreSQL runs it as a single implicit transaction).
    #[allow(deprecated)]
    pub async fn simple_rows(&mut self, sql: &str) -> Result<Vec<Vec<Value>>, sqlx::Error> {
        let options = DecodeOptions {
            text_bytes: true,
            full_bytes: false,
        };
        with_conn!(self, |c| {
            let stream = (&mut **c).fetch_many(sql);
            collect(stream, usize::MAX, options)
                .await
                .map(|collected| collected.rows)
        })
    }

    /// Column names of a statement without running it.
    pub async fn describe(&mut self, sql: &str) -> Option<Vec<ResultColumn>> {
        with_conn!(self, |c| {
            let described = (&mut **c).describe(sql).await.ok()?;
            Some(
                described
                    .columns()
                    .iter()
                    .map(|column| ResultColumn {
                        name: column.name().to_string(),
                        type_name: column.type_info().name().to_string(),
                    })
                    .collect(),
            )
        })
    }

    pub async fn clear_cached_statements(&mut self) {
        let _ = with_conn!(self, |c| c.clear_cached_statements().await);
    }

    pub async fn ping(&mut self) -> bool {
        with_conn!(self, |c| c.ping().await.is_ok())
    }
}

#[must_use]
pub fn query_error(error: &sqlx::Error) -> ApiError {
    ApiError::bad_request(error_text(error)).with_code("DB_QUERY_FAILED")
}

/// The database's own message, with its code when it has one.
#[must_use]
pub fn error_text(error: &sqlx::Error) -> String {
    error.as_database_error().map_or_else(
        || error.to_string(),
        |db_error| {
            db_error.code().map_or_else(
                || db_error.message().to_string(),
                |code| format!("[{code}] {}", db_error.message()),
            )
        },
    )
}

fn needs_text_protocol(error: &sqlx::Error) -> bool {
    // ER_UNSUPPORTED_PS: "This command is not supported in the prepared statement protocol yet".
    error
        .as_database_error()
        .and_then(|db_error| db_error.try_downcast_ref::<MySqlDatabaseError>())
        .is_some_and(|db_error| db_error.number() == 1295)
}

pub trait RowsAffected {
    fn affected(&self) -> u64;
}

impl RowsAffected for PgQueryResult {
    fn affected(&self) -> u64 {
        self.rows_affected()
    }
}

impl RowsAffected for MySqlQueryResult {
    fn affected(&self) -> u64 {
        self.rows_affected()
    }
}

impl RowsAffected for SqliteQueryResult {
    fn affected(&self) -> u64 {
        self.rows_affected()
    }
}

pub trait DecodeCells: sqlx::Row {
    fn cell(&self, index: usize, options: DecodeOptions) -> Value;
}

async fn collect<R, Q, S>(
    mut stream: S,
    max_rows: usize,
    options: DecodeOptions,
) -> Result<Collected, sqlx::Error>
where
    R: DecodeCells,
    Q: RowsAffected,
    S: Stream<Item = Result<Either<Q, R>, sqlx::Error>> + Unpin,
{
    let mut collected = Collected::default();
    while let Some(item) = stream.try_next().await? {
        match item {
            Either::Left(result) => collected.affected += result.affected(),
            Either::Right(row) => {
                if collected.columns.is_none() {
                    collected.columns = Some(
                        row.columns()
                            .iter()
                            .map(|column| ResultColumn {
                                name: column.name().to_string(),
                                type_name: column.type_info().name().to_string(),
                            })
                            .collect(),
                    );
                }
                if collected.rows.len() >= max_rows {
                    // Keep reading so the connection stays usable.
                    collected.truncated = true;
                    continue;
                }
                collected.rows.push(
                    (0..row.columns().len())
                        .map(|index| row.cell(index, options))
                        .collect(),
                );
            }
        }
    }
    Ok(collected)
}

#[allow(clippy::cast_possible_wrap)]
fn int_value(value: i64) -> Value {
    if value.unsigned_abs() <= MAX_SAFE_INTEGER {
        Value::from(value)
    } else {
        Value::String(value.to_string())
    }
}

fn uint_value(value: u64) -> Value {
    if value <= MAX_SAFE_INTEGER {
        Value::from(value)
    } else {
        Value::String(value.to_string())
    }
}

fn float_value(value: f64) -> Value {
    serde_json::Number::from_f64(value)
        .map_or_else(|| Value::String(value.to_string()), Value::Number)
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn bytes_value(bytes: &[u8], options: DecodeOptions) -> Value {
    if options.text_bytes {
        return Value::String(String::from_utf8_lossy(bytes).into_owned());
    }
    if options.full_bytes || bytes.len() <= BINARY_PREVIEW_BYTES {
        return Value::String(format!("0x{}", hex(bytes)));
    }
    Value::String(format!(
        "0x{}… ({} bytes)",
        hex(&bytes[..BINARY_PREVIEW_BYTES]),
        bytes.len()
    ))
}

/// Text when the bytes read as text, otherwise hex.
fn text_or_bytes(bytes: &[u8], options: DecodeOptions) -> Value {
    match std::str::from_utf8(bytes) {
        Ok(text)
            if !text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r')) =>
        {
            Value::String(text.to_string())
        }
        _ => bytes_value(bytes, options),
    }
}

/// PostgreSQL array text form: `{a,"b c",NULL}`.
fn pg_array(items: Vec<Option<String>>) -> Value {
    let items: Vec<String> = items
        .into_iter()
        .map(|item| {
            item.map_or_else(
                || "NULL".to_string(),
                |item| {
                    let plain = !item.is_empty()
                        && !item.eq_ignore_ascii_case("null")
                        && !item.chars().any(|c| {
                            matches!(c, '{' | '}' | ',' | '"' | '\\') || c.is_whitespace()
                        });
                    if plain {
                        item
                    } else {
                        format!("\"{}\"", item.replace('\\', "\\\\").replace('"', "\\\""))
                    }
                },
            )
        })
        .collect();
    Value::String(format!("{{{}}}", items.join(",")))
}

fn to_text<T: ToString>(items: Vec<Option<T>>) -> Vec<Option<String>> {
    items
        .into_iter()
        .map(|item| item.map(|item| item.to_string()))
        .collect()
}

fn interval_text(interval: &PgInterval) -> String {
    let mut parts = Vec::new();
    let years = interval.months / 12;
    let months = interval.months % 12;
    if years != 0 {
        parts.push(format!(
            "{years} year{}",
            if years.abs() == 1 { "" } else { "s" }
        ));
    }
    if months != 0 {
        parts.push(format!(
            "{months} mon{}",
            if months.abs() == 1 { "" } else { "s" }
        ));
    }
    if interval.days != 0 {
        parts.push(format!(
            "{} day{}",
            interval.days,
            if interval.days.abs() == 1 { "" } else { "s" }
        ));
    }
    if interval.microseconds != 0 || parts.is_empty() {
        let sign = if interval.microseconds < 0 { "-" } else { "" };
        let micros = interval.microseconds.unsigned_abs();
        let seconds = micros / 1_000_000;
        let fraction = micros % 1_000_000;
        let mut time = format!(
            "{sign}{:02}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        );
        if fraction != 0 {
            let _ = write!(time, ".{fraction:06}");
            time = time.trim_end_matches('0').to_string();
        }
        parts.push(time);
    }
    parts.join(" ")
}

/// `inet` / `cidr` in their binary form: family, bits, `is_cidr`, length, address.
fn pg_inet(bytes: &[u8]) -> Option<String> {
    let (&family, rest) = bytes.split_first()?;
    let (&bits, rest) = rest.split_first()?;
    let (&is_cidr, rest) = rest.split_first()?;
    let (&length, address) = rest.split_first()?;
    let address = address.get(..usize::from(length))?;
    let (text, max_bits) = match family {
        2 => (
            std::net::Ipv4Addr::from(<[u8; 4]>::try_from(address).ok()?).to_string(),
            32,
        ),
        3 => (
            std::net::Ipv6Addr::from(<[u8; 16]>::try_from(address).ok()?).to_string(),
            128,
        ),
        _ => return None,
    };
    Some(if is_cidr != 0 || bits != max_bits {
        format!("{text}/{bits}")
    } else {
        text
    })
}

impl DecodeCells for PgRow {
    fn cell(&self, index: usize, options: DecodeOptions) -> Value {
        let Ok(raw) = self.try_get_raw(index) else {
            return Value::Null;
        };
        if raw.is_null() {
            return Value::Null;
        }
        let type_name = raw.type_info().name().to_string();
        let bytes = raw.as_bytes().unwrap_or_default();
        pg_value(self, index, &type_name, bytes, options)
            .unwrap_or_else(|| text_or_bytes(bytes, options))
    }
}

fn pg_value(
    row: &PgRow,
    index: usize,
    type_name: &str,
    bytes: &[u8],
    options: DecodeOptions,
) -> Option<Value> {
    use sea_orm::sqlx::types::{
        chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Utc},
        BigDecimal, Uuid,
    };
    macro_rules! get {
        ($type:ty) => {
            row.try_get_unchecked::<$type, _>(index).ok()?
        };
    }
    Some(match type_name {
        "BOOL" => Value::Bool(get!(bool)),
        "INT2" => Value::from(get!(i16)),
        "INT4" => Value::from(get!(i32)),
        "INT8" => int_value(get!(i64)),
        "OID" => Value::from(get!(Oid).0),
        "FLOAT4" => float_value(f64::from(get!(f32))),
        "FLOAT8" => float_value(get!(f64)),
        "NUMERIC" => Value::String(get!(BigDecimal).to_plain_string()),
        "MONEY" => {
            let cents = get!(PgMoney).0;
            let sign = if cents < 0 { "-" } else { "" };
            let cents = cents.unsigned_abs();
            Value::String(format!("{sign}{}.{:02}", cents / 100, cents % 100))
        }
        "UUID" => Value::String(get!(Uuid).to_string()),
        "JSON" | "JSONB" => Value::String(get!(Value).to_string()),
        "DATE" => Value::String(get!(NaiveDate).to_string()),
        "TIME" => Value::String(get!(NaiveTime).to_string()),
        "TIMETZ" => {
            let value = get!(PgTimeTz<NaiveTime, FixedOffset>);
            Value::String(format!("{}{}", value.time, value.offset))
        }
        "TIMESTAMP" => Value::String(
            get!(NaiveDateTime)
                .format("%Y-%m-%d %H:%M:%S%.f")
                .to_string(),
        ),
        "TIMESTAMPTZ" => Value::String(
            get!(DateTime<Utc>)
                .format("%Y-%m-%d %H:%M:%S%.f%:z")
                .to_string(),
        ),
        "INTERVAL" => Value::String(interval_text(&get!(PgInterval))),
        "INET" | "CIDR" => Value::String(pg_inet(bytes)?),
        "MACADDR" if bytes.len() == 6 => Value::String(
            bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<Vec<_>>()
                .join(":"),
        ),
        "BYTEA" => bytes_value(&get!(Vec<u8>), options),
        "TEXT[]" | "VARCHAR[]" | "NAME[]" | "BPCHAR[]" => pg_array(get!(Vec<Option<String>>)),
        "INT2[]" => pg_array(to_text(get!(Vec<Option<i16>>))),
        "INT4[]" => pg_array(to_text(get!(Vec<Option<i32>>))),
        "INT8[]" => pg_array(to_text(get!(Vec<Option<i64>>))),
        "FLOAT4[]" => pg_array(to_text(get!(Vec<Option<f32>>))),
        "FLOAT8[]" => pg_array(to_text(get!(Vec<Option<f64>>))),
        "BOOL[]" => pg_array(
            get!(Vec<Option<bool>>)
                .into_iter()
                .map(|item| item.map(|flag| if flag { "t" } else { "f" }.to_string()))
                .collect(),
        ),
        "UUID[]" => pg_array(to_text(get!(Vec<Option<Uuid>>))),
        "NUMERIC[]" => pg_array(
            get!(Vec<Option<BigDecimal>>)
                .into_iter()
                .map(|item| item.map(|value| value.to_plain_string()))
                .collect(),
        ),
        _ => return None,
    })
}

impl DecodeCells for MySqlRow {
    fn cell(&self, index: usize, options: DecodeOptions) -> Value {
        use sea_orm::sqlx::{
            mysql::types::MySqlTime,
            types::chrono::{NaiveDate, NaiveDateTime},
        };
        let Ok(raw) = self.try_get_raw(index) else {
            return Value::Null;
        };
        if raw.is_null() {
            return Value::Null;
        }
        let type_name = raw.type_info().name().to_string();
        macro_rules! get {
            ($type:ty) => {
                self.try_get_unchecked::<$type, _>(index)
            };
        }
        let bytes = || get!(Vec<u8>).unwrap_or_default();
        match type_name.as_str() {
            "BOOLEAN" | "TINYINT" | "SMALLINT" | "MEDIUMINT" | "INT" | "BIGINT" | "YEAR" => {
                get!(i64).map_or_else(|_| text_or_bytes(&bytes(), options), int_value)
            }
            name if name.ends_with("UNSIGNED") => {
                get!(u64).map_or_else(|_| text_or_bytes(&bytes(), options), uint_value)
            }
            "FLOAT" => get!(f32).map_or(Value::Null, |value| float_value(f64::from(value))),
            "DOUBLE" => get!(f64).map_or(Value::Null, float_value),
            "DATE" => Value::String(
                get!(NaiveDate).map_or_else(|_| "0000-00-00".to_string(), |date| date.to_string()),
            ),
            "DATETIME" | "TIMESTAMP" => Value::String(get!(NaiveDateTime).map_or_else(
                |_| "0000-00-00 00:00:00".to_string(),
                |value| value.format("%Y-%m-%d %H:%M:%S%.f").to_string(),
            )),
            "TIME" => get!(MySqlTime).map_or_else(
                |_| text_or_bytes(&bytes(), options),
                |time| {
                    let mut text = format!(
                        "{}{:02}:{:02}:{:02}",
                        if time.is_negative() { "-" } else { "" },
                        time.hours(),
                        time.minutes(),
                        time.seconds()
                    );
                    if time.microseconds() > 0 {
                        let _ = write!(text, ".{:06}", time.microseconds());
                    }
                    Value::String(text)
                },
            ),
            "BIT" => {
                let bytes = bytes();
                if bytes.len() <= 8 {
                    uint_value(
                        bytes
                            .iter()
                            .fold(0_u64, |acc, byte| (acc << 8) | u64::from(*byte)),
                    )
                } else {
                    bytes_value(&bytes, options)
                }
            }
            "BINARY" | "VARBINARY" | "TINYBLOB" | "BLOB" | "MEDIUMBLOB" | "LONGBLOB"
            | "GEOMETRY" => bytes_value(&bytes(), options),
            _ => Value::String(String::from_utf8_lossy(&bytes()).into_owned()),
        }
    }
}

impl DecodeCells for SqliteRow {
    fn cell(&self, index: usize, options: DecodeOptions) -> Value {
        let Ok(raw) = self.try_get_raw(index) else {
            return Value::Null;
        };
        if raw.is_null() {
            return Value::Null;
        }
        let type_name = raw.type_info().name().to_string();
        match type_name.as_str() {
            "INTEGER" | "BOOLEAN" => self
                .try_get_unchecked::<i64, _>(index)
                .map_or(Value::Null, int_value),
            "REAL" => self
                .try_get_unchecked::<f64, _>(index)
                .map_or(Value::Null, float_value),
            "BLOB" => self
                .try_get_unchecked::<Vec<u8>, _>(index)
                .map_or(Value::Null, |bytes| bytes_value(&bytes, options)),
            _ => self
                .try_get_unchecked::<String, _>(index)
                .map_or(Value::Null, Value::String),
        }
    }
}

/// Reads a metadata cell as text.
#[must_use]
pub fn text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

#[must_use]
pub fn opt_text(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        other => Some(text(other)),
    }
}

#[must_use]
pub fn int(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64().or_else(|| {
            #[allow(clippy::cast_possible_truncation)]
            number.as_f64().map(|value| value as i64)
        }),
        Value::String(text) => text.parse().ok(),
        Value::Bool(flag) => Some(i64::from(*flag)),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

#[must_use]
pub fn flag(value: &Value) -> bool {
    match value {
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_i64().is_some_and(|value| value != 0),
        Value::String(text) => matches!(
            text.to_ascii_lowercase().as_str(),
            "t" | "true" | "1" | "yes" | "y"
        ),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_beyond_javascript_precision_become_text() {
        assert_eq!(int_value(42), Value::from(42));
        assert_eq!(
            int_value(9_007_199_254_740_993),
            Value::String("9007199254740993".into())
        );
        assert_eq!(uint_value(u64::MAX), Value::String(u64::MAX.to_string()));
        assert_eq!(float_value(f64::NAN), Value::String("NaN".into()));
    }

    #[test]
    fn binary_values_are_hex_and_shortened() {
        assert_eq!(
            bytes_value(&[0xde, 0xad], DecodeOptions::default()),
            Value::String("0xdead".into())
        );
        let long = vec![1_u8; 100];
        let shown = text(&bytes_value(&long, DecodeOptions::default()));
        assert!(shown.ends_with("… (100 bytes)"));
        let full = text(&bytes_value(
            &long,
            DecodeOptions {
                text_bytes: false,
                full_bytes: true,
            },
        ));
        assert_eq!(full.len(), 202);
        assert_eq!(
            text_or_bytes(b"plain", DecodeOptions::default()),
            Value::String("plain".into())
        );
        assert_eq!(
            text_or_bytes(&[0, 1], DecodeOptions::default()),
            Value::String("0x0001".into())
        );
    }

    #[test]
    fn postgres_text_forms() {
        assert_eq!(
            pg_array(vec![
                Some("a".into()),
                Some("b c".into()),
                None,
                Some(String::new())
            ]),
            Value::String("{a,\"b c\",NULL,\"\"}".into())
        );
        assert_eq!(
            interval_text(&PgInterval {
                months: 14,
                days: 3,
                microseconds: 3_723_500_000,
            }),
            "1 year 2 mons 3 days 01:02:03.5"
        );
        assert_eq!(
            interval_text(&PgInterval {
                months: 0,
                days: 0,
                microseconds: 0,
            }),
            "00:00:00"
        );
        assert_eq!(pg_inet(&[2, 32, 0, 4, 10, 0, 0, 1]).unwrap(), "10.0.0.1");
        assert_eq!(pg_inet(&[2, 24, 1, 4, 10, 0, 0, 0]).unwrap(), "10.0.0.0/24");
    }

    #[test]
    fn metadata_cells() {
        assert!(flag(&Value::String("YES".into())));
        assert!(flag(&Value::from(1)));
        assert!(!flag(&Value::Null));
        assert_eq!(int(&Value::String("12".into())), Some(12));
        assert_eq!(opt_text(&Value::Null), None);
    }
}
