//! Catalog queries: databases, schemas, tables and their structure.

use std::{collections::BTreeMap, fmt::Write as _};

use serde::Serialize;
use serde_json::Value;

use super::{
    driver::{flag, int, opt_text, text, Bind, ConnMut},
    sql::Dialect,
};
use crate::errors::{ApiError, ApiResult};

/// PostgreSQL types the driver decodes itself; other columns are read as text.
const PG_DECODED_TYPES: &[&str] = &[
    "bool",
    "int2",
    "int4",
    "int8",
    "oid",
    "float4",
    "float8",
    "numeric",
    "money",
    "text",
    "varchar",
    "bpchar",
    "name",
    "char",
    "uuid",
    "json",
    "jsonb",
    "date",
    "time",
    "timetz",
    "timestamp",
    "timestamptz",
    "interval",
    "inet",
    "cidr",
    "bytea",
];
const COMPLETION_LIMIT: usize = 20_000;

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct DbObject {
    pub name: String,
    /// `table`, `view`, `materialized_view`, `foreign_table`, `sequence`,
    /// `function` or `procedure`.
    pub kind: String,
    pub rows: Option<i64>,
    pub size_bytes: Option<i64>,
    pub comment: Option<String>,
    pub engine: Option<String>,
    /// Functions: their oid (PostgreSQL), used to fetch the definition.
    pub identity: Option<String>,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct ColumnInfo {
    pub name: String,
    /// Full type, e.g. `character varying(50)` or `int(10) unsigned`.
    pub data_type: String,
    /// Base type name, e.g. `varchar`.
    pub base_type: String,
    pub nullable: bool,
    pub default_value: Option<String>,
    pub comment: Option<String>,
    pub primary_key: bool,
    pub auto_increment: bool,
    /// Binary columns are shown as hex and not edited in the grid.
    pub binary: bool,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct IndexInfo {
    pub name: String,
    pub columns: String,
    pub unique: bool,
    pub primary: bool,
    pub method: Option<String>,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct ForeignKeyInfo {
    pub name: String,
    pub columns: String,
    pub referenced_table: String,
    pub referenced_columns: String,
    pub on_update: Option<String>,
    pub on_delete: Option<String>,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct TableStructure {
    pub name: String,
    pub kind: String,
    pub comment: Option<String>,
    pub columns: Vec<ColumnInfo>,
    pub primary_key: Vec<String>,
    pub indexes: Vec<IndexInfo>,
    pub foreign_keys: Vec<ForeignKeyInfo>,
    pub ddl: String,
}

fn cell(row: &[Value], index: usize) -> &Value {
    row.get(index).unwrap_or(&Value::Null)
}

/// `quote_ident(schema) || '.' || quote_ident(table)` as a `regclass`.
const PG_REGCLASS: &str = "(quote_ident($1) || '.' || quote_ident($2))::regclass";

fn binds(values: &[&str]) -> Vec<Bind> {
    values.iter().map(|value| Bind::from(*value)).collect()
}

pub async fn server_version(conn: &mut ConnMut<'_>) -> ApiResult<String> {
    let sql = match conn.dialect() {
        Dialect::Postgres => "SHOW server_version",
        Dialect::MySql => "SELECT VERSION()",
        Dialect::Sqlite => "SELECT sqlite_version()",
    };
    let rows = conn.rows(sql, &[]).await?;
    Ok(rows
        .first()
        .map(|row| text(cell(row, 0)))
        .unwrap_or_default())
}

pub async fn databases(conn: &mut ConnMut<'_>) -> ApiResult<Vec<String>> {
    let sql = match conn.dialect() {
        Dialect::Postgres => {
            "SELECT datname FROM pg_database WHERE datallowconn AND NOT datistemplate ORDER BY datname"
        }
        Dialect::MySql => "SHOW DATABASES",
        Dialect::Sqlite => return Ok(vec!["main".to_string()]),
    };
    Ok(conn
        .rows(sql, &[])
        .await?
        .iter()
        .map(|row| text(cell(row, 0)))
        .collect())
}

pub async fn schemas(conn: &mut ConnMut<'_>) -> ApiResult<Vec<String>> {
    if conn.dialect() != Dialect::Postgres {
        return Ok(Vec::new());
    }
    let rows = conn
        .rows(
            "SELECT nspname FROM pg_namespace \
             WHERE nspname NOT LIKE 'pg\\_%' AND nspname <> 'information_schema' \
             ORDER BY nspname = 'public' DESC, nspname",
            &[],
        )
        .await?;
    Ok(rows.iter().map(|row| text(cell(row, 0))).collect())
}

fn object_rows(rows: &[Vec<Value>]) -> Vec<DbObject> {
    rows.iter()
        .map(|row| DbObject {
            name: text(cell(row, 0)),
            kind: text(cell(row, 1)),
            rows: int(cell(row, 2)),
            size_bytes: int(cell(row, 3)),
            comment: opt_text(cell(row, 4)).filter(|comment| !comment.is_empty()),
            engine: opt_text(cell(row, 5)),
            identity: opt_text(cell(row, 6)),
        })
        .collect()
}

pub async fn objects(
    conn: &mut ConnMut<'_>,
    database: &str,
    schema: Option<&str>,
) -> ApiResult<Vec<DbObject>> {
    let mut objects = Vec::new();
    match conn.dialect() {
        Dialect::Postgres => {
            let schema = schema.unwrap_or("public");
            let rows = conn
                .rows(
                    "SELECT c.relname, \
                       CASE c.relkind WHEN 'r' THEN 'table' WHEN 'p' THEN 'table' WHEN 'v' THEN 'view' \
                         WHEN 'm' THEN 'materialized_view' WHEN 'f' THEN 'foreign_table' ELSE 'sequence' END, \
                       CASE WHEN c.relkind IN ('r','p','m') AND c.reltuples >= 0 THEN c.reltuples::bigint END, \
                       CASE WHEN c.relkind IN ('r','p','m') THEN pg_total_relation_size(c.oid) END, \
                       obj_description(c.oid, 'pg_class'), NULL::text, NULL::text \
                     FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
                     WHERE n.nspname = $1 AND c.relkind IN ('r','p','v','m','f','S') \
                     ORDER BY c.relname",
                    &binds(&[schema]),
                )
                .await?;
            objects.extend(object_rows(&rows));
            // `prokind` exists since PostgreSQL 11.
            let functions = match conn
                .rows(
                    "SELECT p.proname || '(' || pg_get_function_identity_arguments(p.oid) || ')', \
                       CASE WHEN p.prokind = 'p' THEN 'procedure' ELSE 'function' END, \
                       NULL::bigint, NULL::bigint, obj_description(p.oid, 'pg_proc'), NULL::text, p.oid::text \
                     FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
                     WHERE n.nspname = $1 AND p.prokind IN ('f','p') ORDER BY 1",
                    &binds(&[schema]),
                )
                .await
            {
                Ok(rows) => rows,
                Err(_) => {
                    conn.rows(
                        "SELECT p.proname || '(' || pg_get_function_identity_arguments(p.oid) || ')', \
                           'function', NULL::bigint, NULL::bigint, obj_description(p.oid, 'pg_proc'), NULL::text, p.oid::text \
                         FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
                         WHERE n.nspname = $1 AND NOT p.proisagg ORDER BY 1",
                        &binds(&[schema]),
                    )
                    .await?
                }
            };
            objects.extend(object_rows(&functions));
        }
        Dialect::MySql => {
            let rows = conn
                .rows(
                    "SELECT TABLE_NAME, CASE WHEN TABLE_TYPE = 'BASE TABLE' THEN 'table' ELSE 'view' END, \
                       TABLE_ROWS, DATA_LENGTH + INDEX_LENGTH, \
                       CASE WHEN TABLE_TYPE = 'BASE TABLE' THEN TABLE_COMMENT END, ENGINE, NULL \
                     FROM information_schema.TABLES WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME",
                    &binds(&[database]),
                )
                .await?;
            objects.extend(object_rows(&rows));
            let routines = conn
                .rows(
                    "SELECT ROUTINE_NAME, LOWER(ROUTINE_TYPE), NULL, NULL, ROUTINE_COMMENT, NULL, NULL \
                     FROM information_schema.ROUTINES WHERE ROUTINE_SCHEMA = ? ORDER BY ROUTINE_NAME",
                    &binds(&[database]),
                )
                .await?;
            objects.extend(object_rows(&routines));
        }
        Dialect::Sqlite => {
            let rows = conn
                .rows(
                    "SELECT name, type, NULL, NULL, NULL, NULL, NULL FROM sqlite_master \
                     WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' \
                     ORDER BY name",
                    &[],
                )
                .await?;
            objects.extend(object_rows(&rows));
        }
    }
    Ok(objects)
}

#[allow(clippy::too_many_lines)]
pub async fn columns(
    conn: &mut ConnMut<'_>,
    database: &str,
    schema: Option<&str>,
    table: &str,
) -> ApiResult<Vec<ColumnInfo>> {
    let columns = match conn.dialect() {
        Dialect::Postgres => {
            let schema = schema.unwrap_or("public");
            let primary = pg_primary_key(conn, schema, table).await?;
            conn.rows(
                &format!(
                    "SELECT a.attname, format_type(a.atttypid, a.atttypmod), NOT a.attnotnull, \
                       pg_get_expr(d.adbin, d.adrelid), col_description(a.attrelid, a.attnum), \
                       t.typname, a.attidentity::text \
                     FROM pg_attribute a \
                     JOIN pg_type t ON t.oid = a.atttypid \
                     LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
                     WHERE a.attrelid = {PG_REGCLASS} AND a.attnum > 0 AND NOT a.attisdropped \
                     ORDER BY a.attnum"
                ),
                &binds(&[schema, table]),
            )
            .await?
            .iter()
            .map(|row| {
                let name = text(cell(row, 0));
                let default_value = opt_text(cell(row, 3));
                let identity = text(cell(row, 6));
                let base_type = text(cell(row, 5));
                ColumnInfo {
                    primary_key: primary.contains(&name),
                    auto_increment: !identity.is_empty()
                        || default_value
                            .as_deref()
                            .is_some_and(|value| value.starts_with("nextval(")),
                    binary: base_type == "bytea",
                    name,
                    data_type: text(cell(row, 1)),
                    nullable: flag(cell(row, 2)),
                    default_value,
                    comment: opt_text(cell(row, 4)),
                    base_type,
                }
            })
            .collect()
        }
        Dialect::MySql => conn
            .rows(
                "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE = 'YES', COLUMN_DEFAULT, COLUMN_COMMENT, \
                   DATA_TYPE, EXTRA, COLUMN_KEY \
                 FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? \
                 ORDER BY ORDINAL_POSITION",
                &binds(&[database, table]),
            )
            .await?
            .iter()
            .map(|row| {
                let base_type = text(cell(row, 5)).to_ascii_lowercase();
                ColumnInfo {
                    name: text(cell(row, 0)),
                    data_type: text(cell(row, 1)),
                    nullable: flag(cell(row, 2)),
                    default_value: opt_text(cell(row, 3)),
                    comment: opt_text(cell(row, 4)).filter(|comment| !comment.is_empty()),
                    auto_increment: text(cell(row, 6))
                        .to_ascii_lowercase()
                        .contains("auto_increment"),
                    primary_key: text(cell(row, 7)) == "PRI",
                    binary: base_type.contains("binary") || base_type.contains("blob"),
                    base_type,
                }
            })
            .collect(),
        Dialect::Sqlite => {
            let rows = conn
                .rows(
                    "SELECT name, type, \"notnull\" = 0, dflt_value, pk FROM pragma_table_info(?) ORDER BY cid",
                    &binds(&[table]),
                )
                .await?;
            let primary_count = rows.iter().filter(|row| int(cell(row, 4)).unwrap_or(0) > 0).count();
            rows.iter()
                .map(|row| {
                    let data_type = text(cell(row, 1));
                    let primary_key = int(cell(row, 4)).unwrap_or(0) > 0;
                    let base_type = data_type
                        .split('(')
                        .next()
                        .unwrap_or_default()
                        .trim()
                        .to_ascii_lowercase();
                    ColumnInfo {
                        name: text(cell(row, 0)),
                        nullable: flag(cell(row, 2)) && !primary_key,
                        default_value: opt_text(cell(row, 3)),
                        comment: None,
                        auto_increment: primary_key && primary_count == 1 && base_type == "integer",
                        primary_key,
                        binary: base_type == "blob",
                        data_type,
                        base_type,
                    }
                })
                .collect()
        }
    };
    Ok(columns)
}

async fn pg_primary_key(
    conn: &mut ConnMut<'_>,
    schema: &str,
    table: &str,
) -> ApiResult<Vec<String>> {
    Ok(conn
        .rows(
            &format!(
                "SELECT a.attname FROM pg_index i \
                 CROSS JOIN LATERAL unnest(i.indkey::int2[]) WITH ORDINALITY AS k(attnum, ord) \
                 JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = k.attnum \
                 WHERE i.indrelid = {PG_REGCLASS} AND i.indisprimary ORDER BY k.ord"
            ),
            &binds(&[schema, table]),
        )
        .await?
        .iter()
        .map(|row| text(cell(row, 0)))
        .collect())
}

/// Primary key columns in key order.
pub async fn primary_key(
    conn: &mut ConnMut<'_>,
    database: &str,
    schema: Option<&str>,
    table: &str,
) -> ApiResult<Vec<String>> {
    match conn.dialect() {
        Dialect::Postgres => pg_primary_key(conn, schema.unwrap_or("public"), table).await,
        Dialect::MySql => Ok(conn
            .rows(
                "SELECT COLUMN_NAME FROM information_schema.KEY_COLUMN_USAGE \
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND CONSTRAINT_NAME = 'PRIMARY' \
                 ORDER BY ORDINAL_POSITION",
                &binds(&[database, table]),
            )
            .await?
            .iter()
            .map(|row| text(cell(row, 0)))
            .collect()),
        Dialect::Sqlite => Ok(conn
            .rows(
                "SELECT name FROM pragma_table_info(?) WHERE pk > 0 ORDER BY pk",
                &binds(&[table]),
            )
            .await?
            .iter()
            .map(|row| text(cell(row, 0)))
            .collect()),
    }
}

const fn pg_action(code: &str) -> &'static str {
    match code.as_bytes() {
        b"r" => "RESTRICT",
        b"c" => "CASCADE",
        b"n" => "SET NULL",
        b"d" => "SET DEFAULT",
        _ => "NO ACTION",
    }
}

async fn indexes(
    conn: &mut ConnMut<'_>,
    database: &str,
    schema: &str,
    table: &str,
) -> ApiResult<(Vec<IndexInfo>, Vec<String>)> {
    match conn.dialect() {
        Dialect::Postgres => {
            let rows = conn
                .rows(
                    &format!(
                        "SELECT ic.relname, i.indisunique, i.indisprimary, am.amname, \
                           (SELECT string_agg(pg_get_indexdef(i.indexrelid, k, true), ', ' ORDER BY k) \
                              FROM generate_series(1, i.indnatts) AS k), \
                           pg_get_indexdef(i.indexrelid), \
                           EXISTS (SELECT 1 FROM pg_constraint con WHERE con.conindid = i.indexrelid \
                                   AND con.conrelid = i.indrelid AND con.contype IN ('p','u','x')) \
                         FROM pg_index i JOIN pg_class ic ON ic.oid = i.indexrelid \
                         JOIN pg_am am ON am.oid = ic.relam \
                         WHERE i.indrelid = {PG_REGCLASS} ORDER BY i.indisprimary DESC, ic.relname"
                    ),
                    &binds(&[schema, table]),
                )
                .await?;
            let definitions = rows
                .iter()
                .filter(|row| !flag(cell(row, 6)))
                .map(|row| format!("{};", text(cell(row, 5))))
                .collect();
            Ok((
                rows.iter()
                    .map(|row| IndexInfo {
                        name: text(cell(row, 0)),
                        unique: flag(cell(row, 1)),
                        primary: flag(cell(row, 2)),
                        method: opt_text(cell(row, 3)),
                        columns: text(cell(row, 4)),
                    })
                    .collect(),
                definitions,
            ))
        }
        Dialect::MySql => {
            let rows = conn
                .rows(
                    "SELECT INDEX_NAME, NON_UNIQUE = 0, INDEX_NAME = 'PRIMARY', INDEX_TYPE, \
                       GROUP_CONCAT(COLUMN_NAME ORDER BY SEQ_IN_INDEX SEPARATOR ', ') \
                     FROM information_schema.STATISTICS WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? \
                     GROUP BY INDEX_NAME, NON_UNIQUE, INDEX_TYPE \
                     ORDER BY INDEX_NAME = 'PRIMARY' DESC, INDEX_NAME",
                    &binds(&[database, table]),
                )
                .await?;
            Ok((
                rows.iter()
                    .map(|row| IndexInfo {
                        name: text(cell(row, 0)),
                        unique: flag(cell(row, 1)),
                        primary: flag(cell(row, 2)),
                        method: opt_text(cell(row, 3)),
                        columns: text(cell(row, 4)),
                    })
                    .collect(),
                Vec::new(),
            ))
        }
        Dialect::Sqlite => {
            let rows = conn
                .rows(
                    "SELECT il.name, il.\"unique\", il.origin = 'pk', il.origin, \
                       (SELECT group_concat(ii.name, ', ') FROM pragma_index_info(il.name) ii) \
                     FROM pragma_index_list(?) il ORDER BY il.origin = 'pk' DESC, il.name",
                    &binds(&[table]),
                )
                .await?;
            Ok((
                rows.iter()
                    .map(|row| IndexInfo {
                        name: text(cell(row, 0)),
                        unique: flag(cell(row, 1)),
                        primary: flag(cell(row, 2)),
                        method: opt_text(cell(row, 3)),
                        columns: text(cell(row, 4)),
                    })
                    .collect(),
                Vec::new(),
            ))
        }
    }
}

async fn foreign_keys(
    conn: &mut ConnMut<'_>,
    database: &str,
    schema: &str,
    table: &str,
) -> ApiResult<Vec<ForeignKeyInfo>> {
    let foreign_keys = match conn.dialect() {
        Dialect::Postgres => conn
            .rows(
                &format!(
                    "SELECT con.conname, \
                       (SELECT string_agg(a.attname, ', ' ORDER BY k.ord) \
                          FROM unnest(con.conkey) WITH ORDINALITY AS k(attnum, ord) \
                          JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.attnum), \
                       con.confrelid::regclass::text, \
                       (SELECT string_agg(a.attname, ', ' ORDER BY k.ord) \
                          FROM unnest(con.confkey) WITH ORDINALITY AS k(attnum, ord) \
                          JOIN pg_attribute a ON a.attrelid = con.confrelid AND a.attnum = k.attnum), \
                       con.confupdtype::text, con.confdeltype::text \
                     FROM pg_constraint con WHERE con.conrelid = {PG_REGCLASS} AND con.contype = 'f' \
                     ORDER BY con.conname"
                ),
                &binds(&[schema, table]),
            )
            .await?
            .iter()
            .map(|row| ForeignKeyInfo {
                name: text(cell(row, 0)),
                columns: text(cell(row, 1)),
                referenced_table: text(cell(row, 2)),
                referenced_columns: text(cell(row, 3)),
                on_update: Some(pg_action(&text(cell(row, 4))).to_string()),
                on_delete: Some(pg_action(&text(cell(row, 5))).to_string()),
            })
            .collect(),
        Dialect::MySql => conn
            .rows(
                "SELECT k.CONSTRAINT_NAME, \
                   GROUP_CONCAT(k.COLUMN_NAME ORDER BY k.ORDINAL_POSITION SEPARATOR ', '), \
                   MAX(k.REFERENCED_TABLE_NAME), \
                   GROUP_CONCAT(k.REFERENCED_COLUMN_NAME ORDER BY k.ORDINAL_POSITION SEPARATOR ', '), \
                   MAX(r.UPDATE_RULE), MAX(r.DELETE_RULE) \
                 FROM information_schema.KEY_COLUMN_USAGE k \
                 JOIN information_schema.REFERENTIAL_CONSTRAINTS r \
                   ON r.CONSTRAINT_SCHEMA = k.CONSTRAINT_SCHEMA AND r.CONSTRAINT_NAME = k.CONSTRAINT_NAME \
                   AND r.TABLE_NAME = k.TABLE_NAME \
                 WHERE k.TABLE_SCHEMA = ? AND k.TABLE_NAME = ? AND k.REFERENCED_TABLE_NAME IS NOT NULL \
                 GROUP BY k.CONSTRAINT_NAME ORDER BY k.CONSTRAINT_NAME",
                &binds(&[database, table]),
            )
            .await?
            .iter()
            .map(|row| ForeignKeyInfo {
                name: text(cell(row, 0)),
                columns: text(cell(row, 1)),
                referenced_table: text(cell(row, 2)),
                referenced_columns: text(cell(row, 3)),
                on_update: opt_text(cell(row, 4)),
                on_delete: opt_text(cell(row, 5)),
            })
            .collect(),
        Dialect::Sqlite => conn
            .rows(
                "SELECT id, \"table\", group_concat(\"from\", ', '), group_concat(\"to\", ', '), \
                   on_update, on_delete \
                 FROM pragma_foreign_key_list(?) GROUP BY id, \"table\", on_update, on_delete ORDER BY id",
                &binds(&[table]),
            )
            .await?
            .iter()
            .map(|row| ForeignKeyInfo {
                name: format!("fk_{}", text(cell(row, 0))),
                columns: text(cell(row, 2)),
                referenced_table: text(cell(row, 1)),
                referenced_columns: text(cell(row, 3)),
                on_update: opt_text(cell(row, 4)),
                on_delete: opt_text(cell(row, 5)),
            })
            .collect(),
    };
    Ok(foreign_keys)
}

/// Kind of a table-like object (`table`, `view`, …), or `None` when missing.
async fn relation_kind(
    conn: &mut ConnMut<'_>,
    database: &str,
    schema: &str,
    table: &str,
) -> ApiResult<Option<(String, Option<String>)>> {
    let rows = match conn.dialect() {
        Dialect::Postgres => {
            conn.rows(
                "SELECT CASE c.relkind WHEN 'v' THEN 'view' WHEN 'm' THEN 'materialized_view' \
                   WHEN 'f' THEN 'foreign_table' WHEN 'S' THEN 'sequence' ELSE 'table' END, \
                   obj_description(c.oid, 'pg_class') \
                 FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
                 WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('r','p','v','m','f','S')",
                &binds(&[schema, table]),
            )
            .await?
        }
        Dialect::MySql => {
            conn.rows(
                "SELECT CASE WHEN TABLE_TYPE = 'BASE TABLE' THEN 'table' ELSE 'view' END, \
                   CASE WHEN TABLE_TYPE = 'BASE TABLE' THEN TABLE_COMMENT END \
                 FROM information_schema.TABLES WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?",
                &binds(&[database, table]),
            )
            .await?
        }
        Dialect::Sqlite => {
            conn.rows(
                "SELECT type, NULL FROM sqlite_master WHERE name = ? AND type IN ('table', 'view')",
                &binds(&[table]),
            )
            .await?
        }
    };
    Ok(rows.first().map(|row| {
        (
            text(cell(row, 0)),
            opt_text(cell(row, 1)).filter(|comment| !comment.is_empty()),
        )
    }))
}

#[allow(clippy::too_many_lines)]
pub async fn structure(
    conn: &mut ConnMut<'_>,
    database: &str,
    schema: Option<&str>,
    table: &str,
) -> ApiResult<TableStructure> {
    let dialect = conn.dialect();
    let schema_name = schema.unwrap_or("public");
    let (kind, comment) = relation_kind(conn, database, schema_name, table)
        .await?
        .ok_or_else(|| {
            ApiError::not_found(format!("表不存在：{table}")).with_code("DB_TABLE_NOT_FOUND")
        })?;
    if kind == "sequence" {
        return Ok(TableStructure {
            name: table.to_string(),
            ddl: sequence_ddl(conn, schema_name, table).await?,
            kind,
            comment,
            columns: Vec::new(),
            primary_key: Vec::new(),
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
        });
    }
    let columns = columns(conn, database, schema, table).await?;
    let primary_key = primary_key(conn, database, schema, table).await?;
    let (indexes, index_definitions) = if kind == "view" {
        (Vec::new(), Vec::new())
    } else {
        indexes(conn, database, schema_name, table).await?
    };
    let foreign_keys = if kind == "table" {
        foreign_keys(conn, database, schema_name, table).await?
    } else {
        Vec::new()
    };
    let qualified = dialect.qualified(database, Some(schema_name), table);
    let ddl = match dialect {
        Dialect::Postgres => match kind.as_str() {
            "view" | "materialized_view" => {
                let rows = conn
                    .rows(
                        &format!("SELECT pg_get_viewdef({PG_REGCLASS}, true)"),
                        &binds(&[schema_name, table]),
                    )
                    .await?;
                let body = rows
                    .first()
                    .map(|row| text(cell(row, 0)))
                    .unwrap_or_default();
                let head = if kind == "view" {
                    "CREATE OR REPLACE VIEW"
                } else {
                    "CREATE MATERIALIZED VIEW"
                };
                format!("{head} {qualified} AS\n{}", body.trim_end())
            }
            _ => {
                pg_table_ddl(
                    conn,
                    schema_name,
                    table,
                    &qualified,
                    &columns,
                    comment.as_deref(),
                    &index_definitions,
                )
                .await?
            }
        },
        Dialect::MySql => {
            let statement = if kind == "view" { "VIEW" } else { "TABLE" };
            let rows = conn
                .rows(&format!("SHOW CREATE {statement} {qualified}"), &[])
                .await?;
            rows.first()
                .map(|row| format!("{};", text(cell(row, 1))))
                .unwrap_or_default()
        }
        Dialect::Sqlite => {
            let mut parts: Vec<String> = conn
                .rows(
                    "SELECT sql FROM sqlite_master WHERE name = ? AND type IN ('table', 'view')",
                    &binds(&[table]),
                )
                .await?
                .iter()
                .map(|row| format!("{};", text(cell(row, 0))))
                .collect();
            parts.extend(
                conn.rows(
                    "SELECT sql FROM sqlite_master WHERE tbl_name = ? AND type IN ('index', 'trigger') \
                     AND sql IS NOT NULL ORDER BY type, name",
                    &binds(&[table]),
                )
                .await?
                .iter()
                .map(|row| format!("{};", text(cell(row, 0)))),
            );
            parts.join("\n\n")
        }
    };
    Ok(TableStructure {
        name: table.to_string(),
        kind,
        comment,
        columns,
        primary_key,
        indexes,
        foreign_keys,
        ddl,
    })
}

#[allow(clippy::too_many_arguments)]
async fn pg_table_ddl(
    conn: &mut ConnMut<'_>,
    schema: &str,
    table: &str,
    qualified: &str,
    columns: &[ColumnInfo],
    comment: Option<&str>,
    index_definitions: &[String],
) -> ApiResult<String> {
    let dialect = Dialect::Postgres;
    let identities: BTreeMap<String, String> = conn
        .rows(
            &format!(
                "SELECT attname, attidentity::text FROM pg_attribute \
                 WHERE attrelid = {PG_REGCLASS} AND attnum > 0 AND NOT attisdropped AND attidentity <> ''"
            ),
            &binds(&[schema, table]),
        )
        .await?
        .iter()
        .map(|row| (text(cell(row, 0)), text(cell(row, 1))))
        .collect();
    let mut lines: Vec<String> = columns
        .iter()
        .map(|column| {
            let mut line = format!("  {} {}", dialect.quote(&column.name), column.data_type);
            match identities.get(&column.name).map(String::as_str) {
                Some("a") => line.push_str(" GENERATED ALWAYS AS IDENTITY"),
                Some("d") => line.push_str(" GENERATED BY DEFAULT AS IDENTITY"),
                _ => {
                    if let Some(default_value) = &column.default_value {
                        line.push_str(" DEFAULT ");
                        line.push_str(default_value);
                    }
                }
            }
            if !column.nullable {
                line.push_str(" NOT NULL");
            }
            line
        })
        .collect();
    let constraints = conn
        .rows(
            &format!(
                "SELECT conname, pg_get_constraintdef(oid, true) FROM pg_constraint \
                 WHERE conrelid = {PG_REGCLASS} AND contype IN ('p','u','c','f','x') \
                 ORDER BY CASE contype WHEN 'p' THEN 0 WHEN 'u' THEN 1 WHEN 'c' THEN 2 WHEN 'f' THEN 3 ELSE 4 END, conname"
            ),
            &binds(&[schema, table]),
        )
        .await?;
    lines.extend(constraints.iter().map(|row| {
        format!(
            "  CONSTRAINT {} {}",
            dialect.quote(&text(cell(row, 0))),
            text(cell(row, 1))
        )
    }));
    let mut ddl = format!("CREATE TABLE {qualified} (\n{}\n);", lines.join(",\n"));
    for definition in index_definitions {
        ddl.push_str("\n\n");
        ddl.push_str(definition);
    }
    if let Some(comment) = comment {
        let _ = write!(
            ddl,
            "\n\nCOMMENT ON TABLE {qualified} IS {};",
            dialect.string_literal(comment)
        );
    }
    for column in columns {
        if let Some(comment) = &column.comment {
            let _ = write!(
                ddl,
                "\nCOMMENT ON COLUMN {qualified}.{} IS {};",
                dialect.quote(&column.name),
                dialect.string_literal(comment)
            );
        }
    }
    Ok(ddl)
}

async fn sequence_ddl(conn: &mut ConnMut<'_>, schema: &str, name: &str) -> ApiResult<String> {
    let rows = conn
        .rows(
            "SELECT data_type::text, increment_by, min_value, max_value, start_value, cache_size, cycle \
             FROM pg_sequences WHERE schemaname = $1 AND sequencename = $2",
            &binds(&[schema, name]),
        )
        .await?;
    let qualified = Dialect::Postgres.qualified("", Some(schema), name);
    Ok(rows.first().map_or_else(
        || format!("CREATE SEQUENCE {qualified};"),
        |row| {
            format!(
                "CREATE SEQUENCE {qualified}\n  AS {}\n  INCREMENT BY {}\n  MINVALUE {}\n  MAXVALUE {}\n  START WITH {}\n  CACHE {}\n  {}CYCLE;",
                text(cell(row, 0)),
                text(cell(row, 1)),
                text(cell(row, 2)),
                text(cell(row, 3)),
                text(cell(row, 4)),
                text(cell(row, 5)),
                if flag(cell(row, 6)) { "" } else { "NO " }
            )
        },
    ))
}

/// Definition of a function or procedure.
pub async fn routine_ddl(
    conn: &mut ConnMut<'_>,
    database: &str,
    kind: &str,
    name: &str,
    identity: Option<&str>,
) -> ApiResult<String> {
    match conn.dialect() {
        Dialect::Postgres => {
            let oid = identity
                .filter(|oid| oid.chars().all(|c| c.is_ascii_digit()) && !oid.is_empty())
                .ok_or_else(|| ApiError::bad_request("缺少函数标识"))?;
            let rows = conn
                .rows("SELECT pg_get_functiondef($1::oid)", &binds(&[oid]))
                .await?;
            Ok(rows
                .first()
                .map(|row| text(cell(row, 0)))
                .unwrap_or_default())
        }
        Dialect::MySql => {
            let statement = if kind == "procedure" {
                "PROCEDURE"
            } else {
                "FUNCTION"
            };
            let rows = conn
                .rows(
                    &format!(
                        "SHOW CREATE {statement} {}",
                        Dialect::MySql.qualified(database, None, name)
                    ),
                    &[],
                )
                .await?;
            Ok(rows
                .first()
                .map(|row| text(cell(row, 2)))
                .unwrap_or_default())
        }
        Dialect::Sqlite => Err(ApiError::bad_request("SQLite 没有存储过程")),
    }
}

/// Table name → column names, for SQL editor completion.
pub async fn completion(
    conn: &mut ConnMut<'_>,
    database: &str,
    schema: Option<&str>,
) -> ApiResult<BTreeMap<String, Vec<String>>> {
    let rows = match conn.dialect() {
        Dialect::Postgres => {
            conn.rows(
                "SELECT c.relname, a.attname FROM pg_class c \
                 JOIN pg_namespace n ON n.oid = c.relnamespace \
                 JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped \
                 WHERE n.nspname = $1 AND c.relkind IN ('r','p','v','m','f') \
                 ORDER BY c.relname, a.attnum LIMIT 20000",
                &binds(&[schema.unwrap_or("public")]),
            )
            .await?
        }
        Dialect::MySql => {
            conn.rows(
                "SELECT TABLE_NAME, COLUMN_NAME FROM information_schema.COLUMNS \
                 WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME, ORDINAL_POSITION LIMIT 20000",
                &binds(&[database]),
            )
            .await?
        }
        Dialect::Sqlite => {
            conn.rows(
                "SELECT m.name, p.name FROM sqlite_master m JOIN pragma_table_info(m.name) p \
                 WHERE m.type IN ('table', 'view') ORDER BY m.name, p.cid",
                &[],
            )
            .await?
        }
    };
    let mut tables: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for row in rows.iter().take(COMPLETION_LIMIT) {
        tables
            .entry(text(cell(row, 0)))
            .or_default()
            .push(text(cell(row, 1)));
    }
    Ok(tables)
}

/// Select-list entry for browsing: PostgreSQL types the driver does not
/// decode are read as their text form, which also round-trips on edit.
#[must_use]
pub fn select_expression(dialect: Dialect, column: &ColumnInfo) -> String {
    let quoted = dialect.quote(&column.name);
    if dialect == Dialect::Postgres && !PG_DECODED_TYPES.contains(&column.base_type.as_str()) {
        format!("{quoted}::text AS {quoted}")
    } else {
        quoted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str, base_type: &str) -> ColumnInfo {
        ColumnInfo {
            name: name.to_string(),
            data_type: base_type.to_string(),
            base_type: base_type.to_string(),
            nullable: true,
            default_value: None,
            comment: None,
            primary_key: false,
            auto_increment: false,
            binary: false,
        }
    }

    #[test]
    fn undecoded_postgres_types_are_read_as_text() {
        assert_eq!(
            select_expression(Dialect::Postgres, &column("tags", "_text")),
            "\"tags\"::text AS \"tags\""
        );
        assert_eq!(
            select_expression(Dialect::Postgres, &column("id", "int4")),
            "\"id\""
        );
        assert_eq!(
            select_expression(Dialect::MySql, &column("x", "set")),
            "`x`"
        );
        assert_eq!(pg_action("c"), "CASCADE");
        assert_eq!(pg_action("a"), "NO ACTION");
    }
}
