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

const MENU_ID: i32 = 35;
const MENU_PATH: &str = "/admin/system/project-cli";
const SYSTEM_MENU_ID: i32 = 2;

/// Kept in sync with `src/fixtures/permissions.yaml`.
const PERMISSIONS: &[Permission] = &[
    Permission {
        id: 158,
        code: "system:project_cli:view",
        name: "查看项目命令",
        description: "查看项目命令行列表和运行记录",
    },
    Permission {
        id: 159,
        code: "system:project_cli:run",
        name: "执行项目命令",
        description: "在服务器上执行项目命令行，并停止运行中的命令",
    },
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Fresh databases are filled by the fixture seed right after
        // migrating; only existing installations need rows added here.
        if count(manager, "permissions").await? > 0 {
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
                        "system:project_cli".into(),
                        permission.description.into(),
                    ],
                )
                .await?;
            }
        }

        if count(manager, "menus").await? > 0
            && !exists(manager, "menus", "path", MENU_PATH).await?
        {
            let parent: SimpleExpr = if exists(manager, "menus", "id", SYSTEM_MENU_ID).await? {
                SYSTEM_MENU_ID.into()
            } else {
                Option::<i32>::None.into()
            };
            insert_with_fixture_id(
                manager,
                "menus",
                MENU_ID,
                vec![
                    "parent_id",
                    "title",
                    "path",
                    "icon",
                    "permission_code",
                    "sort_order",
                    "visible",
                    "enabled",
                ],
                vec![
                    parent,
                    "项目命令".into(),
                    MENU_PATH.into(),
                    "build".into(),
                    "system:project_cli:view".into(),
                    219.into(),
                    true.into(),
                    true.into(),
                ],
            )
            .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .exec_stmt(
                Query::delete()
                    .from_table(Alias::new("menus"))
                    .and_where(Expr::col(Alias::new("path")).eq(MENU_PATH))
                    .to_owned(),
            )
            .await?;
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
