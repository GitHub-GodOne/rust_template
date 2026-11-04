use sea_orm_migration::prelude::*;

use crate::data::{count, exists, insert_with_fixture_id};

#[derive(DeriveMigrationName)]
pub struct Migration;

const ID: i32 = 49;
const KEY: &str = "backup.telegram_destinations";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if count(manager, "system_settings").await? == 0
            || exists(manager, "system_settings", "key", KEY).await?
        {
            return Ok(());
        }
        insert_with_fixture_id(
            manager,
            "system_settings",
            ID,
            vec![
                "key",
                "name",
                "group_key",
                "value",
                "value_type",
                "default_value",
                "description",
                "is_public",
                "is_builtin",
                "is_encrypted",
                "sort_order",
            ],
            vec![
                KEY.into(),
                "Telegram 备份目标".into(),
                "backup".into(),
                "[]".into(),
                "secret".into(),
                "[]".into(),
                "JSON 数组；每项包含 id、name、bot_token、chat_id、enabled，可配置多个群组".into(),
                false.into(),
                true.into(),
                true.into(),
                85.into(),
            ],
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .exec_stmt(
                Query::delete()
                    .from_table(Alias::new("system_settings"))
                    .and_where(Expr::col(Alias::new("key")).eq(KEY))
                    .to_owned(),
            )
            .await
    }
}
