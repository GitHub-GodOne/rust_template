use sea_orm_migration::prelude::*;

use crate::data::{count, exists, insert_with_fixture_id};

#[derive(DeriveMigrationName)]
pub struct Migration;

struct Permission {
    id: i32,
    code: &'static str,
    name: &'static str,
    description: &'static str,
}

/// Kept in sync with `src/fixtures/permissions.yaml`.
const PERMISSIONS: &[Permission] = &[
    Permission {
        id: 178,
        code: "system:session:remote_list",
        name: "查看远程连接",
        description: "查看所有用户正在使用的 SSH、VNC、RDP 连接",
    },
    Permission {
        id: 179,
        code: "system:session:remote_close",
        name: "强制断开远程连接",
        description: "关闭其他用户的 SSH、VNC、RDP 连接，对方页面立即断开",
    },
];

/// Admin view over every user's SSH / VNC / RDP sessions (在线会话 → 远程连接).
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Fresh databases are filled by the fixture seed right after
        // migrating; only existing installations need rows added here.
        if count(manager, "permissions").await? == 0 {
            return Ok(());
        }
        for permission in PERMISSIONS {
            if exists(manager, "permissions", "code", permission.code).await? {
                continue;
            }
            insert_with_fixture_id(
                manager,
                "permissions",
                permission.id,
                vec!["name", "code", "group_name", "description"],
                vec![
                    permission.name.into(),
                    permission.code.into(),
                    "system:session".into(),
                    permission.description.into(),
                ],
            )
            .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let codes: Vec<&str> = PERMISSIONS
            .iter()
            .map(|permission| permission.code)
            .collect();
        manager
            .exec_stmt(
                Query::delete()
                    .from_table(Alias::new("permissions"))
                    .and_where(Expr::col(Alias::new("code")).is_in(codes))
                    .to_owned(),
            )
            .await
    }
}
