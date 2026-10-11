use sea_orm_migration::prelude::*;

use crate::data::{count, exists, insert_with_fixture_id};

#[derive(DeriveMigrationName)]
pub struct Migration;

const PERMISSION_ID: i32 = 161;
const PERMISSION_CODE: &str = "system:log:export";

/// Permission for exporting logs. Kept in sync with `src/fixtures/permissions.yaml`.
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if count(manager, "permissions").await? > 0
            && !exists(manager, "permissions", "code", PERMISSION_CODE).await?
        {
            insert_with_fixture_id(
                manager,
                "permissions",
                PERMISSION_ID,
                vec!["name", "code", "group_name", "description"],
                vec![
                    "导出日志".into(),
                    PERMISSION_CODE.into(),
                    "system:log".into(),
                    "按筛选条件导出日志为 CSV 文件".into(),
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
                    .from_table(Alias::new("permissions"))
                    .and_where(Expr::col(Alias::new("code")).eq(PERMISSION_CODE))
                    .to_owned(),
            )
            .await
    }
}
