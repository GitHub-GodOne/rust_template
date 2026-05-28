#![allow(clippy::missing_errors_doc)]

use std::{
    collections::BTreeMap, fs, path::PathBuf, process::Command, time::Duration, time::Instant,
};

use chrono::{offset::Local, Datelike};
use loco_rs::prelude::*;
use reqwest::multipart;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::errors::{ApiError, ApiResult};

pub use super::_entities::database_backups::{self, ActiveModel, Entity, Model};
use super::_entities::system_settings;

const STORAGE_ROOT: &str = "storage/backups";
const DELIVERY_TIMEOUT_SECONDS: u64 = 10;

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
    created_by: Option<i32>,
    trigger: BackupTrigger,
) -> ApiResult<Model> {
    let started_at = Local::now();
    let timer = Instant::now();
    let filename = format!("db_{}.dump", started_at.format("%Y%m%d%H%M%S"));
    let object_key = format!(
        "{}/{:02}/{}",
        started_at.year(),
        started_at.month(),
        filename
    );
    let storage_path = PathBuf::from(STORAGE_ROOT).join(&object_key);

    if let Some(parent) = storage_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|_| ApiError::internal("failed to prepare backup storage"))?;
    }

    let result = std::env::var("DATABASE_URL").ok().map_or_else(
        || Err("DATABASE_URL is not configured".to_string()),
        |database_url| {
            Command::new("pg_dump")
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
                })
        },
    );

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

pub async fn deliver_backup(db: &DatabaseConnection, backup: Model) -> ApiResult<Model> {
    let settings = BackupDeliverySettings::load(db).await?;
    let targets = settings.targets();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(DELIVERY_TIMEOUT_SECONDS))
        .build()
        .map_err(|_| ApiError::internal("failed to initialize delivery client"))?;

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
