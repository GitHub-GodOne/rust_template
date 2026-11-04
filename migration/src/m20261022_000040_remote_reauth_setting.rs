use sea_orm_migration::prelude::*;

use crate::data::{count, exists, insert_with_fixture_id};

#[derive(DeriveMigrationName)]
pub struct Migration;

const ID: i32 = 46;
const KEY: &str = "security.remote_reauth_enabled";
const REAUTH_KEY: &str = "security.reauth_enabled";
const REAUTH_DESCRIPTION: &str = "开启后，执行命令、删除备份、重置密码等敏感操作前需要重新输入登录密码；连接 SSH/VNC/RDP 另由「远程连接二次确认」控制";
const OLD_REAUTH_DESCRIPTION: &str =
    "开启后，连接 SSH/VNC/RDP、执行命令、删除备份等敏感操作前需要重新输入登录密码";

/// Opening SSH/VNC/RDP connections gets its own password-confirmation switch,
/// off by default. Kept in sync with `src/fixtures/system_settings.yaml`.
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Fresh databases are filled by the fixture seed right after
        // migrating; only existing installations need rows added here.
        if count(manager, "system_settings").await? == 0 {
            return Ok(());
        }
        set_reauth_description(manager, REAUTH_DESCRIPTION).await?;
        if exists(manager, "system_settings", "key", KEY).await? {
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
                "远程连接二次确认".into(),
                "security".into(),
                "false".into(),
                "boolean".into(),
                "false".into(),
                "开启后，连接 SSH/VNC/RDP 前也需要重新输入登录密码（需同时开启「敏感操作二次确认」）"
                    .into(),
                false.into(),
                true.into(),
                false.into(),
                52.into(),
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
            .await?;
        set_reauth_description(manager, OLD_REAUTH_DESCRIPTION).await
    }
}

async fn set_reauth_description(
    manager: &SchemaManager<'_>,
    description: &str,
) -> Result<(), DbErr> {
    manager
        .exec_stmt(
            Query::update()
                .table(Alias::new("system_settings"))
                .value(Alias::new("description"), description)
                .and_where(Expr::col(Alias::new("key")).eq(REAUTH_KEY))
                .to_owned(),
        )
        .await
}
