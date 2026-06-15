#![allow(clippy::missing_errors_doc)]

use sea_orm::{ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter};

use super::_entities::system_settings;

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
        .and_then(|setting| setting.value.parse::<i64>().ok())
        .unwrap_or(default_value);
    Ok(value)
}
