use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

use crate::data::{count, exists, insert_with_fixture_id};

#[derive(DeriveMigrationName)]
pub struct Migration;

const PERMISSION_ID: i32 = 160;
const PERMISSION_CODE: &str = "system:log:config";

/// Per-feature switches for request audit logging. A missing row means the
/// feature uses its built-in defaults; `feature_key = "*"` is the global switch.
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            "audit_log_rules",
            &[
                ("id", ColType::PkAuto),
                ("feature_key", ColType::StringUniq),
                ("enabled", ColType::Boolean),
                ("log_reads", ColType::Boolean),
                ("log_body", ColType::Boolean),
            ],
            &[],
        )
        .await?;

        // Kept in sync with `src/fixtures/permissions.yaml`.
        if count(manager, "permissions").await? > 0
            && !exists(manager, "permissions", "code", PERMISSION_CODE).await?
        {
            insert_with_fixture_id(
                manager,
                "permissions",
                PERMISSION_ID,
                vec!["name", "code", "group_name", "description"],
                vec![
                    "日志记录设置".into(),
                    PERMISSION_CODE.into(),
                    "system:log".into(),
                    "按功能开启或关闭日志记录".into(),
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
            .await?;
        drop_table(manager, "audit_log_rules").await
    }
}
