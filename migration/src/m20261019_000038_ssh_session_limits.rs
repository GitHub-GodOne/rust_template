use sea_orm_migration::prelude::*;

use crate::data::{count, exists, insert_with_fixture_id};

#[derive(DeriveMigrationName)]
pub struct Migration;

struct Setting {
    id: i32,
    key: &'static str,
    name: &'static str,
    description: &'static str,
    sort_order: i32,
}

/// Kept in sync with `src/fixtures/system_settings.yaml`.
const SETTINGS: &[Setting] = &[
    Setting {
        id: 44,
        key: "ssh_manager.idle_timeout_minutes",
        name: "SSH 会话空闲超时（分钟）",
        description: "没有页面接入且没有输入输出超过这么多分钟后自动关闭 SSH 会话；0 表示一直保持，直到会话退出或服务停止",
        sort_order: 191,
    },
    Setting {
        id: 45,
        key: "ssh_manager.max_session_hours",
        name: "SSH 会话最长保持（小时）",
        description: "SSH 会话从创建起最多保持多少小时，到时自动关闭；0 表示不限制，直到会话退出或服务停止",
        sort_order: 192,
    },
];

/// Optional idle / lifetime limits for SSH sessions (both off by default).
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Fresh databases are filled by the fixture seed right after
        // migrating; only existing installations need rows added here.
        if count(manager, "system_settings").await? == 0 {
            return Ok(());
        }
        for setting in SETTINGS {
            if exists(manager, "system_settings", "key", setting.key).await? {
                continue;
            }
            insert_with_fixture_id(
                manager,
                "system_settings",
                setting.id,
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
                    setting.key.into(),
                    setting.name.into(),
                    "ssh_manager".into(),
                    "0".into(),
                    "number".into(),
                    "0".into(),
                    setting.description.into(),
                    false.into(),
                    true.into(),
                    false.into(),
                    setting.sort_order.into(),
                ],
            )
            .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let keys: Vec<&str> = SETTINGS.iter().map(|setting| setting.key).collect();
        manager
            .exec_stmt(
                Query::delete()
                    .from_table(Alias::new("system_settings"))
                    .and_where(Expr::col(Alias::new("key")).is_in(keys))
                    .to_owned(),
            )
            .await
    }
}
