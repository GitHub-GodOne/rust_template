#![allow(clippy::missing_errors_doc)]

use sea_orm::{ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter};
use serde::de::DeserializeOwned;

use super::_entities::system_settings;

pub const DEFAULT_UPLOAD_MAX_SIZE_MB: i64 = 20;
const BYTES_PER_MIB: i64 = 1024 * 1024;

pub async fn string_value(
    db: &DatabaseConnection,
    key: &str,
    default_value: &str,
) -> Result<String, DbErr> {
    let value = system_settings::Entity::find()
        .filter(system_settings::Column::Key.eq(key))
        .one(db)
        .await?
        .map(|setting| setting.value)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default_value.to_string());
    Ok(value)
}

pub async fn number_i64(
    db: &DatabaseConnection,
    key: &str,
    default_value: i64,
) -> Result<i64, DbErr> {
    let value = system_settings::Entity::find()
        .filter(system_settings::Column::Key.eq(key))
        .one(db)
        .await?
        .and_then(|setting| setting.value.trim().parse::<i64>().ok())
        .unwrap_or(default_value);
    Ok(value)
}

pub async fn upload_max_size_mb(db: &DatabaseConnection) -> Result<i64, DbErr> {
    let configured = number_i64(db, "upload.max_size_mb", DEFAULT_UPLOAD_MAX_SIZE_MB).await?;
    Ok(normalize_upload_max_size_mb(configured))
}

pub async fn upload_limit_bytes(db: &DatabaseConnection) -> Result<i64, DbErr> {
    Ok(upload_limit_bytes_from_mb(upload_max_size_mb(db).await?))
}

#[must_use]
pub fn upload_limit_error_message(max_bytes: i64) -> String {
    let max_mb = (max_bytes / BYTES_PER_MIB).max(1);
    format!("上传文件超过当前 {max_mb} MB 限制，请在系统设置中调整 upload.max_size_mb")
}

const fn normalize_upload_max_size_mb(value: i64) -> i64 {
    if value > 0 {
        value
    } else {
        DEFAULT_UPLOAD_MAX_SIZE_MB
    }
}

const fn upload_limit_bytes_from_mb(max_mb: i64) -> i64 {
    max_mb.saturating_mul(BYTES_PER_MIB)
}

pub async fn json_value<T>(db: &DatabaseConnection, key: &str, default_value: T) -> Result<T, DbErr>
where
    T: DeserializeOwned,
{
    let Some(setting) = system_settings::Entity::find()
        .filter(system_settings::Column::Key.eq(key))
        .one(db)
        .await?
    else {
        return Ok(default_value);
    };

    let value = setting.value.trim();
    if value.is_empty() {
        return Ok(default_value);
    }

    Ok(serde_json::from_str(value).unwrap_or(default_value))
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_upload_max_size_mb, upload_limit_bytes_from_mb, DEFAULT_UPLOAD_MAX_SIZE_MB,
    };

    #[test]
    fn upload_limit_normalization_accepts_any_positive_integer() {
        assert_eq!(normalize_upload_max_size_mb(1), 1);
        assert_eq!(normalize_upload_max_size_mb(200), 200);
        assert_eq!(normalize_upload_max_size_mb(10_241), 10_241);
        assert_eq!(normalize_upload_max_size_mb(i64::MAX), i64::MAX);
        assert_eq!(normalize_upload_max_size_mb(0), DEFAULT_UPLOAD_MAX_SIZE_MB);
        assert_eq!(normalize_upload_max_size_mb(-1), DEFAULT_UPLOAD_MAX_SIZE_MB);
    }

    #[test]
    fn upload_limit_byte_conversion_does_not_overflow() {
        assert_eq!(upload_limit_bytes_from_mb(10_241), 10_738_466_816);
        assert_eq!(upload_limit_bytes_from_mb(i64::MAX), i64::MAX);
    }
}
