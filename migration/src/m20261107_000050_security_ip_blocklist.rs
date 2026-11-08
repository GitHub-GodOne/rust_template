use sea_orm_migration::prelude::*;

use crate::data::{count, exists, insert_with_fixture_id};

#[derive(DeriveMigrationName)]
pub struct Migration;

const SETTING_ID: i32 = 50;
const SETTING_KEY: &str = "security.admin_ip_blocklist";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if count(manager, "system_settings").await? > 0
            && !exists(manager, "system_settings", "key", SETTING_KEY).await?
        {
            insert_with_fixture_id(
                manager,
                "system_settings",
                SETTING_ID,
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
                    SETTING_KEY.into(),
                    "后台访问 IP 黑名单".into(),
                    "security".into(),
                    "".into(),
                    "string".into(),
                    "".into(),
                    "命中后拒绝登录和后台接口访问，多个用逗号或换行分隔，黑名单优先于白名单；留空表示不限制".into(),
                    false.into(),
                    true.into(),
                    false.into(),
                    61.into(),
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
                    .from_table(Alias::new("system_settings"))
                    .and_where(Expr::col(Alias::new("key")).eq(SETTING_KEY))
                    .to_owned(),
            )
            .await
    }
}
