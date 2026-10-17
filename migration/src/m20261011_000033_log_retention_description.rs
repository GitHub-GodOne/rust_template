use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

const KEY: &str = "security.log_retention_days";

/// The retention setting now drives automatic cleanup; say so in 系统设置.
/// Kept in sync with `src/fixtures/system_settings.yaml`.
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .exec_stmt(
                Query::update()
                    .table(Alias::new("system_settings"))
                    .value(
                        Alias::new("description"),
                        "操作日志保留天数，超过的日志每 6 小时自动清理一次；0 表示永久保留",
                    )
                    .and_where(Expr::col(Alias::new("key")).eq(KEY))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .exec_stmt(
                Query::update()
                    .table(Alias::new("system_settings"))
                    .value(Alias::new("description"), "操作日志建议保留天数")
                    .and_where(Expr::col(Alias::new("key")).eq(KEY))
                    .to_owned(),
            )
            .await
    }
}
