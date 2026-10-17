use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

use crate::data::{count, exists, insert_with_fixture_id};

#[derive(DeriveMigrationName)]
pub struct Migration;

const MENU_ID: i32 = 34;
const MENU_PATH: &str = "/admin/system/dock";
const SYSTEM_MENU_ID: i32 = 2;

/// Per-user UI preferences (e.g. the quick-launch dock), stored as JSON.
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            "user_preferences",
            &[
                ("id", ColType::PkAuto),
                ("pref_key", ColType::String),
                ("value", ColType::Text),
            ],
            &[("users", "user_id")],
        )
        .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_user_preferences_user_key")
                    .table(UserPreferences::Table)
                    .col(UserPreferences::UserId)
                    .col(UserPreferences::PrefKey)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // Existing installations get the "快捷 Dock" menu; fresh ones are
        // seeded from fixtures. Kept in sync with `src/fixtures/menus.yaml`.
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
                    "sort_order",
                    "visible",
                    "enabled",
                ],
                vec![
                    parent,
                    "快捷 Dock".into(),
                    MENU_PATH.into(),
                    "appstore".into(),
                    244.into(),
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
        drop_table(manager, "user_preferences").await
    }
}

#[derive(DeriveIden)]
enum UserPreferences {
    Table,
    UserId,
    PrefKey,
}
