use sea_orm_migration::prelude::*;

use crate::data::{count, exists, insert_with_fixture_id};

#[derive(DeriveMigrationName)]
pub struct Migration;

const MENU_ID: i32 = 50;
const MENU_PATH: &str = "/admin/system/startup-commands";
const SYSTEM_MENU_ID: i32 = 2;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
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
                    "开机自启命令".into(),
                    MENU_PATH.into(),
                    "schedule".into(),
                    "system:scheduled_task:list".into(),
                    95.into(),
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
            .await
    }
}
