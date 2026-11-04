use sea_orm_migration::prelude::*;

use crate::data::{count, exists, insert_with_fixture_id};

#[derive(DeriveMigrationName)]
pub struct Migration;

const ID: i32 = 47;
const KEY: &str = "ssh_manager.keep_sessions_on_restart";
const NAME: &str = "服务重启后保留 SSH 会话";
const DESCRIPTION: &str = "开启后，停止或重启服务时 SSH 会话继续在后台运行，服务再次启动后可以接着连接；关闭后，服务退出时一并结束所有 SSH 会话";

/// The limits no longer end at a service stop when sessions are kept.
const LIMIT_DESCRIPTIONS: [(&str, &str, &str); 2] = [
    (
        "ssh_manager.idle_timeout_minutes",
        "没有页面接入且没有输入输出超过这么多分钟后自动关闭 SSH 会话；0 表示一直保持，直到会话退出或服务停止",
        "没有页面接入且没有输入输出超过这么多分钟后自动关闭 SSH 会话；0 表示不自动关闭",
    ),
    (
        "ssh_manager.max_session_hours",
        "SSH 会话从创建起最多保持多少小时，到时自动关闭；0 表示不限制，直到会话退出或服务停止",
        "SSH 会话从创建起最多保持多少小时，到时自动关闭；0 表示不限制",
    ),
];

/// Whether SSH sessions outlive a server restart (on by default). Kept in
/// sync with `src/fixtures/system_settings.yaml`.
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Fresh databases are filled by the fixture seed right after
        // migrating; only existing installations need rows added here.
        if count(manager, "system_settings").await? == 0 {
            return Ok(());
        }
        for (key, old, new) in LIMIT_DESCRIPTIONS {
            set_description(manager, key, old, new).await?;
        }
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
                NAME.into(),
                "ssh_manager".into(),
                "true".into(),
                "boolean".into(),
                "true".into(),
                DESCRIPTION.into(),
                false.into(),
                true.into(),
                false.into(),
                190.into(),
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
        for (key, old, new) in LIMIT_DESCRIPTIONS {
            set_description(manager, key, new, old).await?;
        }
        Ok(())
    }
}

/// Only rewrites a description nobody edited.
async fn set_description(
    manager: &SchemaManager<'_>,
    key: &str,
    from: &str,
    to: &str,
) -> Result<(), DbErr> {
    manager
        .exec_stmt(
            Query::update()
                .table(Alias::new("system_settings"))
                .value(Alias::new("description"), to)
                .and_where(Expr::col(Alias::new("key")).eq(key))
                .and_where(Expr::col(Alias::new("description")).eq(from))
                .to_owned(),
        )
        .await
}
