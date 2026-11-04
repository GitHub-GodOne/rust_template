//! Recycle bin: deleting a record keeps a JSON snapshot so it can be
//! restored later.
//!
//! * Database rows: the row plus every row that `ON DELETE CASCADE` would
//!   remove with it, discovered from the PostgreSQL catalog, and the
//!   `ON DELETE SET NULL` references to re-link on restore.
//! * Files on disk: moved under `storage/recycle_bin/<entry id>/` and moved
//!   back on restore.
//! * Resources kept elsewhere (soft-deleted uploads, static-site configs)
//!   store whatever their restore needs.
//!
//! Entries expire after `security.recycle_bin_retention_days`.

#![allow(clippy::missing_errors_doc)]

use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use chrono::Local;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DbBackend, EntityTrait,
    QueryFilter, Set, Statement, TransactionTrait, Value as DbValue,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    errors::{ApiError, ApiResult},
    models::{
        _entities::{recycle_bin, upload_files, users},
        system_settings,
    },
};

pub const TRASH_DIR: &str = "storage/recycle_bin";
pub const RETENTION_SETTING: &str = "security.recycle_bin_retention_days";
/// Rows a single deletion may carry into the bin (row + cascaded children).
const MAX_ROWS: usize = 20_000;
const MAX_DEPTH: usize = 6;
/// Never resurrected: logins and the bin itself.
const SKIP_TABLES: &[&str] = &["user_sessions", "refresh_tokens", "recycle_bins"];
const CLEAN_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

pub struct Resource {
    pub key: &'static str,
    pub label: &'static str,
}

const fn resource(key: &'static str, label: &'static str) -> Resource {
    Resource { key, label }
}

/// Everything that can end up in the bin, for labels and filters.
pub const RESOURCES: &[Resource] = &[
    resource("users", "用户"),
    resource("roles", "角色"),
    resource("menus", "菜单"),
    resource("permissions", "权限"),
    resource("departments", "部门"),
    resource("tenants", "租户"),
    resource("dict_types", "字典类型"),
    resource("dict_items", "字典项"),
    resource("settings", "系统设置"),
    resource("email_templates", "邮件模板"),
    resource("notifications", "通知"),
    resource("scheduled_tasks", "定时任务"),
    resource("rate_limits", "限流规则"),
    resource("backups", "数据库备份"),
    resource("work_orders", "工单"),
    resource("work_order_attachments", "工单附件"),
    resource("payment_channels", "支付渠道"),
    resource("content_articles", "文章"),
    resource("content_categories", "文章分类"),
    resource("chat_messages", "聊天消息"),
    resource("im_conversations", "聊天群组"),
    resource("commands", "命令模板"),
    resource("command_workflows", "命令编排"),
    resource("api_keys", "API 授权"),
    resource("http_requests", "HTTP 请求预设"),
    resource("db_connections", "数据库连接"),
    resource("ai_image_configs", "AI 图片配置"),
    resource("storage_profiles", "存储配置"),
    resource("storage_buckets", "存储桶"),
    resource("certificates", "证书"),
    resource("domain_mappings", "域名映射"),
    resource("uploads", "素材"),
    resource("static_sites", "静态托管站点"),
    resource("static_files", "静态托管文件"),
    resource("files", "文件管理"),
];

#[must_use]
pub fn resource_label(key: &str) -> &str {
    RESOURCES
        .iter()
        .find(|resource| resource.key == key)
        .map_or(key, |resource| resource.label)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableRows {
    pub table: String,
    pub rows: Vec<Value>,
}

/// An `ON DELETE SET NULL` reference cleared by the deletion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relink {
    pub table: String,
    pub column: String,
    /// `id`s of the referencing rows, as text.
    pub ids: Vec<String>,
    pub value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrashedFile {
    pub original: String,
    pub trashed: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Payload {
    Rows {
        tables: Vec<TableRows>,
        relinks: Vec<Relink>,
        #[serde(default)]
        files: Vec<TrashedFile>,
    },
    UploadStatus {
        id: i32,
        status: String,
    },
    /// An entry removed from a list kept in a JSON setting, e.g. a static
    /// site or an AI image config; `collection` names the list.
    ConfigItem {
        collection: String,
        item: Value,
    },
    Files {
        files: Vec<TrashedFile>,
    },
}

impl Payload {
    #[must_use]
    pub fn row_count(&self) -> usize {
        match self {
            Self::Rows { tables, .. } => tables.iter().map(|table| table.rows.len()).sum(),
            Self::Files { files } => files.len(),
            Self::UploadStatus { .. } | Self::ConfigItem { .. } => 1,
        }
    }
}

fn db_error(error: &sea_orm::DbErr) -> ApiError {
    ApiError::bad_request(format!("回收站操作失败：{error}"))
}

fn ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn stmt(sql: String, values: Vec<DbValue>) -> Statement {
    Statement::from_sql_and_values(DbBackend::Postgres, sql, values)
}

fn ensure_postgres(db: &impl ConnectionTrait) -> ApiResult<()> {
    if db.get_database_backend() == DbBackend::Postgres {
        Ok(())
    } else {
        Err(ApiError::internal("recycle bin requires PostgreSQL"))
    }
}

struct ForeignKey {
    child_table: String,
    child_column: String,
    parent_column: String,
    cascade: bool,
}

async fn foreign_keys_to(conn: &impl ConnectionTrait, table: &str) -> ApiResult<Vec<ForeignKey>> {
    let rows = conn
        .query_all(stmt(
            "SELECT cl.relname::text, att.attname::text, ref_att.attname::text, con.confdeltype::text \
             FROM pg_constraint con \
             JOIN pg_class cl ON cl.oid = con.conrelid \
             JOIN pg_class ref ON ref.oid = con.confrelid \
             JOIN pg_namespace ns ON ns.oid = ref.relnamespace \
             JOIN pg_attribute att ON att.attrelid = con.conrelid AND att.attnum = con.conkey[1] \
             JOIN pg_attribute ref_att ON ref_att.attrelid = con.confrelid AND ref_att.attnum = con.confkey[1] \
             WHERE con.contype = 'f' AND ref.relname = $1 AND ns.nspname = current_schema() \
               AND array_length(con.conkey, 1) = 1 AND con.confdeltype IN ('c', 'n') \
             ORDER BY cl.relname, att.attname"
                .to_string(),
            vec![table.into()],
        ))
        .await
        .map_err(|error| db_error(&error))?;
    rows.into_iter()
        .map(|row| {
            let get = |index: usize| -> ApiResult<String> {
                row.try_get_by_index::<String>(index)
                    .map_err(|error| db_error(&error))
            };
            Ok(ForeignKey {
                child_table: get(0)?,
                child_column: get(1)?,
                parent_column: get(2)?,
                cascade: get(3)? == "c",
            })
        })
        .collect()
}

async fn has_id_column(conn: &impl ConnectionTrait, table: &str) -> ApiResult<bool> {
    let row = conn
        .query_one(stmt(
            "SELECT 1 FROM information_schema.columns \
             WHERE table_schema = current_schema() AND table_name = $1 AND column_name = 'id'"
                .to_string(),
            vec![table.into()],
        ))
        .await
        .map_err(|error| db_error(&error))?;
    Ok(row.is_some())
}

fn placeholders(count: usize) -> String {
    (1..=count)
        .map(|index| format!("${index}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Rows of `table` whose `column` (compared as text) is one of `values`.
async fn select_rows(
    conn: &impl ConnectionTrait,
    table: &str,
    column: &str,
    values: &[String],
) -> ApiResult<Vec<Value>> {
    let mut out = Vec::new();
    for chunk in values.chunks(1000) {
        let sql = format!(
            "SELECT row_to_json(t)::text FROM {} t WHERE t.{}::text IN ({})",
            ident(table),
            ident(column),
            placeholders(chunk.len())
        );
        let params = chunk.iter().map(|value| value.clone().into()).collect();
        for row in conn
            .query_all(stmt(sql, params))
            .await
            .map_err(|error| db_error(&error))?
        {
            let text: String = row.try_get_by_index(0).map_err(|error| db_error(&error))?;
            out.push(serde_json::from_str(&text).map_err(|_| ApiError::internal("bad row json"))?);
        }
    }
    Ok(out)
}

fn as_text(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    }
}

#[derive(Default)]
struct Snapshot {
    tables: Vec<TableRows>,
    relinks: Vec<Relink>,
    seen: HashSet<String>,
    rows: usize,
}

impl Snapshot {
    fn add(&mut self, table: &str, rows: Vec<Value>) -> ApiResult<Vec<Value>> {
        let fresh: Vec<Value> = rows
            .into_iter()
            .filter(|row| self.seen.insert(format!("{table}:{row}")))
            .collect();
        self.rows += fresh.len();
        if self.rows > MAX_ROWS {
            return Err(ApiError::bad_request(format!(
                "关联数据超过 {MAX_ROWS} 行，无法放入回收站，请先清理关联数据后再删除"
            )));
        }
        if !fresh.is_empty() {
            self.tables.push(TableRows {
                table: table.to_string(),
                rows: fresh.clone(),
            });
        }
        Ok(fresh)
    }
}

/// Collects `rows` of `table` and, depth first, what cascades from them.
fn collect<'a>(
    conn: &'a impl ConnectionTrait,
    table: &'a str,
    rows: Vec<Value>,
    depth: usize,
    snapshot: &'a mut Snapshot,
) -> futures_util::future::BoxFuture<'a, ApiResult<()>> {
    Box::pin(async move {
        let rows = snapshot.add(table, rows)?;
        if rows.is_empty() || depth >= MAX_DEPTH {
            return Ok(());
        }
        for key in foreign_keys_to(conn, table).await? {
            if SKIP_TABLES.contains(&key.child_table.as_str()) {
                continue;
            }
            let parent_values: Vec<String> = rows
                .iter()
                .filter_map(|row| row.get(&key.parent_column).and_then(as_text))
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            if parent_values.is_empty() {
                continue;
            }
            let children =
                select_rows(conn, &key.child_table, &key.child_column, &parent_values).await?;
            if children.is_empty() {
                continue;
            }
            if key.cascade {
                collect(conn, &key.child_table, children, depth + 1, snapshot).await?;
            } else if has_id_column(conn, &key.child_table).await? {
                let mut by_value: BTreeMap<String, (Value, Vec<String>)> = BTreeMap::new();
                for child in &children {
                    let (Some(value), Some(id)) = (
                        child.get(&key.child_column).filter(|v| !v.is_null()),
                        child.get("id").and_then(as_text),
                    ) else {
                        continue;
                    };
                    by_value
                        .entry(value.to_string())
                        .or_insert_with(|| (value.clone(), Vec::new()))
                        .1
                        .push(id);
                }
                for (_, (value, ids)) in by_value {
                    snapshot.relinks.push(Relink {
                        table: key.child_table.clone(),
                        column: key.child_column.clone(),
                        ids,
                        value,
                    });
                }
            }
        }
        Ok(())
    })
}

#[derive(Debug, Clone)]
pub struct Deletion<'a> {
    pub resource: &'static str,
    pub record_id: String,
    pub title: String,
    pub actor: &'a users::Model,
}

impl<'a> Deletion<'a> {
    // Callers pass numeric ids and string keys alike.
    #[allow(clippy::needless_pass_by_value)]
    pub fn new(
        resource: &'static str,
        record_id: impl ToString,
        title: impl Into<String>,
        actor: &'a users::Model,
    ) -> Self {
        Self {
            resource,
            record_id: record_id.to_string(),
            title: title.into(),
            actor,
        }
    }
}

async fn insert_entry(
    conn: &impl ConnectionTrait,
    deletion: &Deletion<'_>,
    payload: &Payload,
) -> ApiResult<recycle_bin::Model> {
    let title: String = deletion.title.chars().take(250).collect();
    recycle_bin::ActiveModel {
        resource: Set(deletion.resource.to_string()),
        record_id: Set(deletion.record_id.clone()),
        title: Set(if title.trim().is_empty() {
            deletion.record_id.clone()
        } else {
            title
        }),
        payload: Set(serde_json::to_string(payload)
            .map_err(|_| ApiError::internal("failed to encode recycle payload"))?),
        deleted_by: Set(Some(deletion.actor.id)),
        deleted_by_name: Set(Some(
            Some(deletion.actor.name.clone())
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| deletion.actor.email.clone()),
        )),
        ..Default::default()
    }
    .insert(conn)
    .await
    .map_err(|error| db_error(&error))
}

/// Deletes `table` row `id` (and its cascade) into the bin. `files` are
/// moved into the bin too, e.g. a certificate's key folder.
pub async fn delete_row(
    db: &DatabaseConnection,
    deletion: Deletion<'_>,
    table: &str,
    id: i64,
    files: &[PathBuf],
) -> ApiResult<()> {
    ensure_postgres(db)?;
    let txn = db.begin().await.map_err(|error| db_error(&error))?;
    let root = select_rows(&txn, table, "id", &[id.to_string()]).await?;
    if root.is_empty() {
        return Err(ApiError::not_found("record not found"));
    }
    let mut snapshot = Snapshot::default();
    collect(&txn, table, root, 0, &mut snapshot).await?;
    txn.execute(stmt(
        format!("DELETE FROM {} WHERE id = $1", ident(table)),
        vec![id.into()],
    ))
    .await
    .map_err(|error| db_error(&error))?;
    let mut payload = Payload::Rows {
        tables: snapshot.tables,
        relinks: snapshot.relinks,
        files: Vec::new(),
    };
    let entry = insert_entry(&txn, &deletion, &payload).await?;
    if !files.is_empty() {
        let trashed = trash_files(entry.id, files)?;
        if let Payload::Rows { files, .. } = &mut payload {
            *files = trashed;
        }
        update_payload(&txn, entry, &payload).await?;
    }
    txn.commit().await.map_err(|error| db_error(&error))?;
    Ok(())
}

/// Records a deletion that the caller already carried out.
pub async fn record(
    db: &DatabaseConnection,
    deletion: Deletion<'_>,
    payload: Payload,
) -> ApiResult<recycle_bin::Model> {
    insert_entry(db, &deletion, &payload).await
}

/// Moves files or folders on disk into the bin.
pub async fn delete_files(
    db: &DatabaseConnection,
    deletion: Deletion<'_>,
    paths: &[PathBuf],
) -> ApiResult<()> {
    let txn = db.begin().await.map_err(|error| db_error(&error))?;
    let entry = insert_entry(&txn, &deletion, &Payload::Files { files: Vec::new() }).await?;
    let files = trash_files(entry.id, paths)?;
    update_payload(&txn, entry, &Payload::Files { files }).await?;
    txn.commit().await.map_err(|error| db_error(&error))?;
    Ok(())
}

async fn update_payload(
    conn: &impl ConnectionTrait,
    entry: recycle_bin::Model,
    payload: &Payload,
) -> ApiResult<()> {
    let mut active: recycle_bin::ActiveModel = entry.into();
    active.payload = Set(serde_json::to_string(payload)
        .map_err(|_| ApiError::internal("failed to encode recycle payload"))?);
    active
        .update(conn)
        .await
        .map_err(|error| db_error(&error))?;
    Ok(())
}

fn entry_dir(entry_id: i32) -> PathBuf {
    Path::new(TRASH_DIR).join(entry_id.to_string())
}

fn trash_files(entry_id: i32, paths: &[PathBuf]) -> ApiResult<Vec<TrashedFile>> {
    let dir = entry_dir(entry_id);
    fs::create_dir_all(&dir).map_err(|error| {
        ApiError::internal(format!("无法创建回收站目录 {}：{error}", dir.display()))
    })?;
    let mut moved = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        if !path.exists() {
            continue;
        }
        let name = path.file_name().map_or_else(
            || "item".to_string(),
            |name| name.to_string_lossy().to_string(),
        );
        let target = dir.join(format!("{index}-{name}"));
        if let Err(error) = move_path(path, &target) {
            // Put back what already moved so nothing is half deleted.
            for file in &moved {
                let TrashedFile { original, trashed } = file;
                let _ = move_path(Path::new(trashed), Path::new(original));
            }
            return Err(ApiError::internal(format!(
                "无法移动 {} 到回收站：{error}",
                path.display()
            )));
        }
        moved.push(TrashedFile {
            original: path.to_string_lossy().to_string(),
            trashed: target.to_string_lossy().to_string(),
        });
    }
    Ok(moved)
}

fn move_path(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    // Different file systems: copy, then remove the source.
    copy_recursive(from, to)?;
    if from.is_dir() {
        fs::remove_dir_all(from)
    } else {
        fs::remove_file(from)
    }
}

fn copy_recursive(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        fs::copy(from, to).map(|_| ())
    }
}

pub fn parse_payload(entry: &recycle_bin::Model) -> ApiResult<Payload> {
    serde_json::from_str(&entry.payload).map_err(|_| ApiError::internal("回收站数据已损坏"))
}

async fn restore_rows(
    conn: &impl ConnectionTrait,
    tables: &[TableRows],
    relinks: &[Relink],
) -> ApiResult<()> {
    for table in tables {
        let rows = serde_json::to_string(&table.rows)
            .map_err(|_| ApiError::internal("failed to encode rows"))?;
        conn.execute(stmt(
            format!(
                "INSERT INTO {0} SELECT * FROM json_populate_recordset(NULL::{0}, $1::json)",
                ident(&table.table)
            ),
            vec![rows.into()],
        ))
        .await
        .map_err(|error| restore_error(&error))?;
    }
    for relink in relinks {
        for chunk in relink.ids.chunks(1000) {
            let mut values: Vec<DbValue> =
                vec![serde_json::json!({ relink.column.clone(): relink.value })
                    .to_string()
                    .into()];
            values.extend(chunk.iter().map(|id| DbValue::from(id.clone())));
            let ids = (2..=chunk.len() + 1)
                .map(|index| format!("${index}"))
                .collect::<Vec<_>>()
                .join(", ");
            conn.execute(stmt(
                format!(
                    "UPDATE {0} SET {1} = (json_populate_record(NULL::{0}, $1::json)).{1} \
                     WHERE id::text IN ({ids}) AND {1} IS NULL",
                    ident(&relink.table),
                    ident(&relink.column)
                ),
                values,
            ))
            .await
            .map_err(|error| restore_error(&error))?;
        }
    }
    let restored: HashSet<&str> = tables.iter().map(|table| table.table.as_str()).collect();
    for table in restored {
        // Join tables (composite keys) have no id sequence.
        if !has_id_column(conn, table).await? {
            continue;
        }
        conn.execute(stmt(
            format!(
                "SELECT setval(seq, GREATEST((SELECT COALESCE(MAX(id), 1) FROM {0}), 1)) \
                 FROM (SELECT pg_get_serial_sequence('{1}', 'id') AS seq) s WHERE seq IS NOT NULL",
                ident(table),
                table.replace('\'', "''")
            ),
            Vec::new(),
        ))
        .await
        .map_err(|error| restore_error(&error))?;
    }
    Ok(())
}

fn restore_error(error: &sea_orm::DbErr) -> ApiError {
    let text = error.to_string();
    if text.contains("duplicate key") || text.contains("unique") {
        ApiError::bad_request(format!(
            "无法恢复：已有相同的唯一字段（如名称、编码或邮箱）的数据，请先修改或删除它。{text}"
        ))
    } else if text.contains("foreign key") {
        ApiError::bad_request(format!(
            "无法恢复：它引用的数据已不存在，请先恢复被引用的数据。{text}"
        ))
    } else {
        ApiError::bad_request(format!("恢复失败：{text}"))
    }
}

fn restore_files(files: &[TrashedFile]) -> ApiResult<()> {
    for file in files {
        let original = Path::new(&file.original);
        if original.exists() {
            return Err(ApiError::bad_request(format!(
                "无法恢复：原位置已存在 {}",
                file.original
            )));
        }
    }
    for file in files {
        move_path(Path::new(&file.trashed), Path::new(&file.original)).map_err(|error| {
            ApiError::internal(format!("无法恢复文件 {}：{error}", file.original))
        })?;
    }
    Ok(())
}

/// What a restore needs from outside this module.
pub enum RestoreHook {
    /// Put this entry back into the named JSON-setting list.
    ConfigItem {
        collection: String,
        item: Value,
    },
    None,
}

/// Restores an entry and removes it from the bin.
pub async fn restore(
    db: &DatabaseConnection,
    entry_id: i32,
) -> ApiResult<(recycle_bin::Model, RestoreHook)> {
    let entry = find(db, entry_id).await?;
    let payload = parse_payload(&entry)?;
    let txn = db.begin().await.map_err(|error| db_error(&error))?;
    let hook = match &payload {
        Payload::Rows {
            tables,
            relinks,
            files,
        } => {
            ensure_postgres(db)?;
            restore_rows(&txn, tables, relinks).await?;
            restore_files(files)?;
            RestoreHook::None
        }
        Payload::UploadStatus { id, status } => {
            let upload = upload_files::Entity::find_by_id(*id)
                .one(&txn)
                .await
                .map_err(|error| db_error(&error))?
                .ok_or_else(|| ApiError::bad_request("素材记录已不存在，无法恢复"))?;
            let mut active: upload_files::ActiveModel = upload.into();
            active.status = Set(status.clone());
            active
                .update(&txn)
                .await
                .map_err(|error| db_error(&error))?;
            RestoreHook::None
        }
        Payload::ConfigItem { collection, item } => RestoreHook::ConfigItem {
            collection: collection.clone(),
            item: item.clone(),
        },
        Payload::Files { files } => {
            restore_files(files)?;
            RestoreHook::None
        }
    };
    recycle_bin::Entity::delete_by_id(entry.id)
        .exec(&txn)
        .await
        .map_err(|error| db_error(&error))?;
    txn.commit().await.map_err(|error| db_error(&error))?;
    let _ = fs::remove_dir_all(entry_dir(entry.id));
    Ok((entry, hook))
}

/// Permanently deletes an entry and whatever it still holds.
pub async fn purge(db: &DatabaseConnection, entry_id: i32) -> ApiResult<recycle_bin::Model> {
    let entry = find(db, entry_id).await?;
    if let Ok(Payload::UploadStatus { id, .. }) = parse_payload(&entry) {
        upload_files::Entity::delete_by_id(id)
            .exec(db)
            .await
            .map_err(|error| db_error(&error))?;
    }
    recycle_bin::Entity::delete_by_id(entry.id)
        .exec(db)
        .await
        .map_err(|error| db_error(&error))?;
    let dir = entry_dir(entry.id);
    if dir.exists() {
        if let Err(error) = fs::remove_dir_all(&dir) {
            tracing::warn!(error = error.to_string(), path = %dir.display(), "failed to remove recycled files");
        }
    }
    Ok(entry)
}

pub async fn find(db: &DatabaseConnection, entry_id: i32) -> ApiResult<recycle_bin::Model> {
    recycle_bin::Entity::find_by_id(entry_id)
        .one(db)
        .await?
        .ok_or_else(|| ApiError::not_found("回收站中没有这条数据"))
}

pub async fn retention_days(db: &DatabaseConnection) -> Result<i64, sea_orm::DbErr> {
    Ok(system_settings::number_i64(db, RETENTION_SETTING, 30)
        .await?
        .max(0))
}

/// Purges entries older than the retention period; returns how many.
pub async fn purge_expired(db: &DatabaseConnection) -> ApiResult<usize> {
    let days = retention_days(db).await?;
    if days == 0 {
        return Ok(0);
    }
    let cutoff = Local::now() - chrono::Duration::days(days);
    let expired = recycle_bin::Entity::find()
        .filter(recycle_bin::Column::CreatedAt.lt(cutoff))
        .all(db)
        .await?;
    let mut purged = 0;
    for entry in expired {
        if purge(db, entry.id).await.is_ok() {
            purged += 1;
        }
    }
    Ok(purged)
}

/// Background cleaner for expired entries.
pub fn start_cleaner(db: DatabaseConnection) {
    tokio::spawn(async move {
        loop {
            match purge_expired(&db).await {
                Ok(0) => {}
                Ok(count) => tracing::info!(count, "purged expired recycle bin entries"),
                Err(error) => tracing::warn!(error = error.message(), "recycle bin cleanup failed"),
            }
            tokio::time::sleep(CLEAN_INTERVAL).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{move_path, Payload, TrashedFile};

    #[test]
    fn payloads_round_trip_with_their_kind() {
        let payload = Payload::Files {
            files: vec![TrashedFile {
                original: "a".into(),
                trashed: "b".into(),
            }],
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("\"kind\":\"files\""));
        assert!(matches!(
            serde_json::from_str::<Payload>(&json).unwrap(),
            Payload::Files { .. }
        ));
    }

    #[test]
    fn folders_move_and_come_back() {
        let root = std::env::temp_dir().join(format!("recycle-test-{}", std::process::id()));
        let source = root.join("site");
        std::fs::create_dir_all(source.join("nested")).unwrap();
        std::fs::write(source.join("nested/index.html"), "hi").unwrap();
        let trashed = root.join("bin/0-site");
        move_path(&source, &trashed).unwrap();
        assert!(!source.exists());
        assert!(trashed.join("nested/index.html").exists());
        move_path(&trashed, &source).unwrap();
        assert_eq!(
            std::fs::read_to_string(source.join("nested/index.html")).unwrap(),
            "hi"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
