use sea_orm_migration::prelude::*;

use crate::data::{count, exists, insert_with_fixture_id};

#[derive(DeriveMigrationName)]
pub struct Migration;

const ID: i32 = 48;
const KEY: &str = "ssh_manager.scrollback_lines";

/// How much terminal output SSH sessions keep (0 = no limit of its own).
/// Kept in sync with `src/fixtures/system_settings.yaml`.
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Fresh databases are filled by the fixture seed right after
        // migrating; only existing installations need the row added here.
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
                "SSH 终端保留行数".into(),
                "ssh_manager".into(),
                "0".into(),
                "number".into(),
                "0".into(),
                "终端能往回翻看、断开重连后能恢复的输出行数；0 表示不限制（最多保留最近 100000 行，避免浏览器内存耗尽），也可填 1 到 100000".into(),
                false.into(),
                true.into(),
                false.into(),
                193.into(),
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
