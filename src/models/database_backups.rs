#![allow(clippy::missing_errors_doc)]

use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

use chrono::{offset::Local, Datelike};
use loco_rs::prelude::*;
use reqwest::multipart;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{
    errors::{ApiError, ApiResult},
    models::system_settings as settings,
    services::http_client::{self, HttpClientRuntimeOverrides},
};

pub use super::_entities::database_backups::{self, ActiveModel, Entity, Model};
use super::_entities::{database_restores, system_settings};

pub const RESTORE_CONFIRM_PHRASE: &str = "RESTORE DATABASE";

#[derive(Debug, Clone, Copy)]
pub enum BackupTrigger {
    Manual,
    Scheduled,
}

impl BackupTrigger {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Scheduled => "scheduled",
        }
    }
}

pub async fn create_postgres_backup(
    db: &DatabaseConnection,
    database_url: &str,
    created_by: Option<i32>,
    trigger: BackupTrigger,
) -> ApiResult<Model> {
    let started_at = Local::now();
    let timer = Instant::now();
    let filename = format!("db_{}.dump", started_at.format("%Y%m%d%H%M%S"));
    let object_key = format!(
        "{}/{:02}/{:02}/{}",
        started_at.year(),
        started_at.month(),
        started_at.day(),
        filename
    );
    let storage_root = settings::string_value(db, "backup.storage_root", "storage/backups").await?;
    let storage_path = PathBuf::from(storage_root).join(&object_key);

    if let Some(parent) = storage_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|_| ApiError::internal("failed to prepare backup storage"))?;
    }

    let result = Command::new("pg_dump")
        .arg("--format=custom")
        .arg("--file")
        .arg(&storage_path)
        .arg(database_url)
        .output()
        .map_err(|err| err.to_string())
        .and_then(|output| {
            if output.status.success() {
                Ok(())
            } else {
                Err(String::from_utf8_lossy(&output.stderr).to_string())
            }
        });

    let (status, size_bytes, sha256, error_message) = match result {
        Ok(()) => {
            let bytes = fs::read(&storage_path)
                .map_err(|_| ApiError::internal("failed to read backup file"))?;
            let digest = Sha256::digest(&bytes);
            (
                "success",
                i64::try_from(bytes.len()).unwrap_or(0),
                Some(hex::encode(digest)),
                None,
            )
        }
        Err(message) => {
            let _ = fs::remove_file(&storage_path);
            ("failed", 0, None, Some(trim_message(&message)))
        }
    };

    let duration_ms = i32::try_from(timer.elapsed().as_millis()).unwrap_or(i32::MAX);
    let model = database_backups::ActiveModel {
        filename: Set(filename),
        storage_path: Set(storage_path.display().to_string()),
        size_bytes: Set(size_bytes),
        sha256: Set(sha256),
        status: Set(status.to_string()),
        trigger_type: Set(trigger.as_str().to_string()),
        started_at: Set(started_at.into()),
        finished_at: Set(Some(Local::now().into())),
        duration_ms: Set(Some(duration_ms)),
        delivery_targets: Set(None),
        delivery_status: Set(None),
        error_message: Set(error_message),
        created_by: Set(created_by),
        ..Default::default()
    }
    .insert(db)
    .await?;

    Ok(model)
}

pub struct RestoreOptions {
    pub confirm_phrase: String,
    pub database_url: String,
}

pub async fn restore_postgres_backup(
    db: &DatabaseConnection,
    backup: Model,
    restored_by: Option<i32>,
    options: RestoreOptions,
) -> ApiResult<database_restores::Model> {
    validate_restore_confirmation(&options.confirm_phrase)?;
    validate_restorable_backup(&backup)?;
    verify_backup_file(&backup)?;

    let pre_restore_backup = create_postgres_backup(
        db,
        &options.database_url,
        restored_by,
        BackupTrigger::Manual,
    )
    .await?;
    if pre_restore_backup.status != "success" {
        return Err(ApiError::bad_request(
            "pre-restore safety backup did not complete successfully",
        ));
    }

    let started_at = Local::now();
    let timer = Instant::now();
    let running = database_restores::ActiveModel {
        backup_id: Set(backup.id),
        status: Set("running".to_string()),
        confirm_phrase: Set(RESTORE_CONFIRM_PHRASE.to_string()),
        pre_restore_backup_id: Set(Some(pre_restore_backup.id)),
        started_at: Set(started_at.into()),
        restored_by: Set(restored_by),
        ..Default::default()
    }
    .insert(db)
    .await?;

    let result = run_pg_restore(&options.database_url, &backup.storage_path);
    let finished_at = Local::now();
    let duration_ms = i32::try_from(timer.elapsed().as_millis()).unwrap_or(i32::MAX);
    let (status, output, error_message) = match result {
        Ok(output) => ("success", Some(output), None),
        Err(error) => ("failed", None, Some(error)),
    };

    persist_restore_result(
        db,
        running,
        RestorePersistResult {
            backup_id: backup.id,
            status,
            pre_restore_backup_id: Some(pre_restore_backup.id),
            started_at,
            finished_at,
            duration_ms,
            output,
            error_message,
            restored_by,
        },
    )
    .await
}

pub async fn deliver_backup(db: &DatabaseConnection, backup: Model) -> ApiResult<Model> {
    let settings = BackupDeliverySettings::load(db).await?;
    let targets = settings.targets();
    let delivery_timeout_seconds =
        u64::try_from(settings.delivery_timeout_seconds.clamp(1, 300)).unwrap_or(10);
    let client = http_client::build_http_client_with_overrides(
        db,
        HttpClientRuntimeOverrides {
            request_timeout_seconds: Some(delivery_timeout_seconds),
        },
    )
    .await?;

    let mut statuses = BTreeMap::new();
    if targets.is_empty() {
        statuses.insert(
            "status".to_string(),
            "skipped: no delivery targets configured".to_string(),
        );
    }

    for target in &targets {
        let result = match target.as_str() {
            "telegram" => send_telegram(&client, &settings, &backup).await,
            "wecom" => send_text_webhook(&client, &settings.wecom_webhook_url, &backup).await,
            "dingtalk" => send_text_webhook(&client, &settings.dingtalk_webhook_url, &backup).await,
            "custom" => send_custom(&client, &settings, &backup).await,
            _ => Err("skipped: unsupported delivery target".to_string()),
        };
        statuses.insert(
            target.clone(),
            result.unwrap_or_else(|error| trim_message(&error)),
        );
    }

    let mut active = backup.into_active_model();
    active.delivery_targets = Set(Some(json_string(&targets)));
    active.delivery_status = Set(Some(json_string(&statuses)));
    Ok(active.update(db).await?)
}

struct BackupDeliverySettings {
    delivery_targets: String,
    telegram_bot_token: String,
    telegram_chat_id: String,
    wecom_webhook_url: String,
    dingtalk_webhook_url: String,
    custom_webhook_url: String,
    delivery_timeout_seconds: i64,
}

impl BackupDeliverySettings {
    async fn load(db: &DatabaseConnection) -> ApiResult<Self> {
        let settings = system_settings::Entity::find()
            .filter(system_settings::Column::GroupKey.eq("backup"))
            .all(db)
            .await?;
        let values = settings
            .into_iter()
            .map(|setting| (setting.key, setting.value))
            .collect::<BTreeMap<_, _>>();

        Ok(Self {
            delivery_targets: setting_value(&values, "backup.delivery_targets"),
            telegram_bot_token: setting_value(&values, "backup.telegram_bot_token"),
            telegram_chat_id: setting_value(&values, "backup.telegram_chat_id"),
            wecom_webhook_url: setting_value(&values, "backup.wecom_webhook_url"),
            dingtalk_webhook_url: setting_value(&values, "backup.dingtalk_webhook_url"),
            custom_webhook_url: setting_value(&values, "backup.custom_webhook_url"),
            delivery_timeout_seconds: setting_value(&values, "backup.delivery_timeout_seconds")
                .parse()
                .unwrap_or(10),
        })
    }

    fn targets(&self) -> Vec<String> {
        parse_targets(&self.delivery_targets)
    }
}

async fn send_telegram(
    client: &reqwest::Client,
    settings: &BackupDeliverySettings,
    backup: &Model,
) -> Result<String, String> {
    if settings.telegram_bot_token.trim().is_empty() || settings.telegram_chat_id.trim().is_empty()
    {
        return Err("skipped: missing telegram token or chat id".to_string());
    }

    let url = format!(
        "https://api.telegram.org/bot{}/sendMessage",
        settings.telegram_bot_token
    );
    post_json(
        client,
        &url,
        json!({
            "chat_id": settings.telegram_chat_id,
            "text": backup_message(backup),
        }),
    )
    .await
}

async fn send_text_webhook(
    client: &reqwest::Client,
    webhook_url: &str,
    backup: &Model,
) -> Result<String, String> {
    if webhook_url.trim().is_empty() {
        return Err("skipped: missing webhook url".to_string());
    }

    post_json(
        client,
        webhook_url,
        json!({
            "msgtype": "text",
            "text": {
                "content": backup_message(backup),
            },
        }),
    )
    .await
}

async fn send_custom(
    client: &reqwest::Client,
    settings: &BackupDeliverySettings,
    backup: &Model,
) -> Result<String, String> {
    if settings.custom_webhook_url.trim().is_empty() {
        return Err("skipped: missing custom webhook url".to_string());
    }

    let metadata = backup_metadata(backup);
    let path = PathBuf::from(&backup.storage_path);
    if backup.status == "success" && path.exists() {
        let bytes = fs::read(&path).map_err(|err| format!("failed to read backup file: {err}"))?;
        let file_part = multipart::Part::bytes(bytes)
            .file_name(backup.filename.clone())
            .mime_str("application/octet-stream")
            .map_err(|err| err.to_string())?;
        let form = multipart::Form::new()
            .text("metadata", metadata.to_string())
            .part("file", file_part);
        let response = client
            .post(settings.custom_webhook_url.trim())
            .multipart(form)
            .send()
            .await
            .map_err(|err| err.to_string())?;
        return response_status(&response);
    }

    post_json(client, &settings.custom_webhook_url, metadata).await
}

async fn post_json(
    client: &reqwest::Client,
    url: &str,
    payload: serde_json::Value,
) -> Result<String, String> {
    let response = client
        .post(url.trim())
        .json(&payload)
        .send()
        .await
        .map_err(|err| err.to_string())?;
    response_status(&response)
}

fn response_status(response: &reqwest::Response) -> Result<String, String> {
    let status = response.status();
    if status.is_success() {
        Ok("success".to_string())
    } else {
        Err(format!("failed: http {status}"))
    }
}

fn backup_message(backup: &Model) -> String {
    format!(
        "数据库备份 #{id}\n文件：{filename}\n状态：{status}\n大小：{size} B\nSHA-256：{sha}\n开始：{started}\n结束：{finished}\n错误：{error}",
        id = backup.id,
        filename = backup.filename,
        status = backup.status,
        size = backup.size_bytes,
        sha = backup.sha256.as_deref().unwrap_or("-"),
        started = backup.started_at.to_rfc3339(),
        finished = backup
            .finished_at
            .map_or_else(|| "-".to_string(), |value| value.to_rfc3339()),
        error = backup.error_message.as_deref().unwrap_or("-"),
    )
}

fn backup_metadata(backup: &Model) -> serde_json::Value {
    json!({
        "id": backup.id,
        "filename": backup.filename,
        "storage_path": backup.storage_path,
        "size_bytes": backup.size_bytes,
        "sha256": backup.sha256,
        "status": backup.status,
        "trigger_type": backup.trigger_type,
        "started_at": backup.started_at.to_rfc3339(),
        "finished_at": backup.finished_at.map(|value| value.to_rfc3339()),
        "duration_ms": backup.duration_ms,
        "error_message": backup.error_message,
    })
}

struct RestorePersistResult {
    backup_id: i32,
    status: &'static str,
    pre_restore_backup_id: Option<i32>,
    started_at: chrono::DateTime<Local>,
    finished_at: chrono::DateTime<Local>,
    duration_ms: i32,
    output: Option<String>,
    error_message: Option<String>,
    restored_by: Option<i32>,
}

async fn persist_restore_result(
    db: &DatabaseConnection,
    running: database_restores::Model,
    result: RestorePersistResult,
) -> ApiResult<database_restores::Model> {
    let mut active = running.into_active_model();
    active.status = Set(result.status.to_string());
    active.finished_at = Set(Some(result.finished_at.into()));
    active.duration_ms = Set(Some(result.duration_ms));
    active.output = Set(result.output.clone());
    active.error_message = Set(result.error_message.clone());

    match active.update(db).await {
        Ok(restore) => Ok(restore),
        Err(_) => Ok(database_restores::ActiveModel {
            backup_id: Set(result.backup_id),
            status: Set(result.status.to_string()),
            confirm_phrase: Set(RESTORE_CONFIRM_PHRASE.to_string()),
            pre_restore_backup_id: Set(result.pre_restore_backup_id),
            started_at: Set(result.started_at.into()),
            finished_at: Set(Some(result.finished_at.into())),
            duration_ms: Set(Some(result.duration_ms)),
            output: Set(result.output),
            error_message: Set(result.error_message),
            restored_by: Set(result.restored_by),
            ..Default::default()
        }
        .insert(db)
        .await?),
    }
}

fn validate_restorable_backup(backup: &Model) -> ApiResult<()> {
    if backup.status != "success" {
        return Err(ApiError::bad_request(
            "only successful backups can be restored",
        ));
    }
    if backup.sha256.is_none() {
        return Err(ApiError::bad_request("backup checksum is missing"));
    }
    Ok(())
}

pub fn validate_restore_confirmation(confirm_phrase: &str) -> ApiResult<()> {
    if confirm_phrase.trim() == RESTORE_CONFIRM_PHRASE {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "restore confirmation phrase is invalid",
        ))
    }
}

fn verify_backup_file(backup: &Model) -> ApiResult<()> {
    let path = PathBuf::from(&backup.storage_path);
    if !path.is_file() {
        return Err(ApiError::bad_request("backup file is missing"));
    }

    let bytes = fs::read(&path).map_err(|_| ApiError::internal("failed to read backup file"))?;
    let digest = hex::encode(Sha256::digest(&bytes));
    if backup.sha256.as_deref() == Some(digest.as_str()) {
        Ok(())
    } else {
        Err(ApiError::bad_request("backup checksum does not match"))
    }
}

fn run_pg_restore(database_url: &str, storage_path: &str) -> Result<String, String> {
    let output = Command::new("pg_restore")
        .args(pg_restore_args(database_url, Path::new(storage_path)))
        .output()
        .map_err(|err| err.to_string())?;

    if output.status.success() {
        Ok(trim_message(&String::from_utf8_lossy(&output.stdout)))
    } else {
        Err(trim_message(&String::from_utf8_lossy(&output.stderr)))
    }
}

#[must_use]
pub fn pg_restore_args(database_url: &str, storage_path: &Path) -> Vec<OsString> {
    vec![
        "--clean".into(),
        "--if-exists".into(),
        "--no-owner".into(),
        "--no-privileges".into(),
        "--dbname".into(),
        database_url.into(),
        storage_path.as_os_str().to_os_string(),
    ]
}

fn parse_targets(value: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(value)
        .unwrap_or_else(|_| {
            value
                .split(',')
                .map(str::trim)
                .filter(|target| !target.is_empty())
                .map(ToString::to_string)
                .collect()
        })
        .into_iter()
        .map(|target| target.trim().to_ascii_lowercase())
        .filter(|target| !target.is_empty())
        .collect()
}

fn setting_value(values: &BTreeMap<String, String>, key: &str) -> String {
    values.get(key).cloned().unwrap_or_default()
}

fn json_string<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
}

fn trim_message(message: &str) -> String {
    message.chars().take(1000).collect()
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, path::Path};

    use super::{pg_restore_args, validate_restore_confirmation, RESTORE_CONFIRM_PHRASE};

    #[test]
    fn validates_restore_confirmation_phrase() {
        assert!(validate_restore_confirmation(RESTORE_CONFIRM_PHRASE).is_ok());
        assert!(validate_restore_confirmation(" RESTORE DATABASE ").is_ok());
        assert!(validate_restore_confirmation("restore database").is_err());
    }

    #[test]
    fn builds_pg_restore_args_without_shell_interpolation() {
        let args = pg_restore_args(
            "postgres://user:pass@localhost/db?sslmode=disable",
            Path::new("storage/backups/2026/05/db.dump"),
        );

        assert_eq!(
            args,
            vec![
                OsString::from("--clean"),
                OsString::from("--if-exists"),
                OsString::from("--no-owner"),
                OsString::from("--no-privileges"),
                OsString::from("--dbname"),
                OsString::from("postgres://user:pass@localhost/db?sslmode=disable"),
                OsString::from("storage/backups/2026/05/db.dump"),
            ]
        );
    }
}
