use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // One ALTER per column also works with the SQLite regression database.
        for column in [
            ColumnDef::new(Alias::new("retry_on_failure"))
                .boolean()
                .not_null()
                .default(false)
                .to_owned(),
            ColumnDef::new(Alias::new("max_retries"))
                .integer()
                .not_null()
                .default(3)
                .to_owned(),
            ColumnDef::new(Alias::new("retry_delay_seconds"))
                .integer()
                .not_null()
                .default(5)
                .to_owned(),
            ColumnDef::new(Alias::new("attempt_count"))
                .integer()
                .not_null()
                .default(0)
                .to_owned(),
        ] {
            manager
                .alter_table(
                    Table::alter()
                        .table(Alias::new("command_runs"))
                        .add_column(column)
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for name in [
            "attempt_count",
            "retry_delay_seconds",
            "max_retries",
            "retry_on_failure",
        ] {
            manager
                .alter_table(
                    Table::alter()
                        .table(Alias::new("command_runs"))
                        .drop_column(Alias::new(name))
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
}
