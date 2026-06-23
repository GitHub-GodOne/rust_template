use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.alter_table(
            Table::alter()
                .table(CommandTemplates::Table)
                .add_column(
                    ColumnDef::new(CommandTemplates::PreviewPathTemplate)
                        .text()
                        .null(),
                )
                .to_owned(),
        )
        .await?;

        m.alter_table(
            Table::alter()
                .table(CommandRuns::Table)
                .add_column(
                    ColumnDef::new(CommandRuns::PreviewPathTemplate)
                        .text()
                        .null(),
                )
                .add_column(ColumnDef::new(CommandRuns::PreviewPath).text().null())
                .to_owned(),
        )
        .await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.alter_table(
            Table::alter()
                .table(CommandRuns::Table)
                .drop_column(CommandRuns::PreviewPath)
                .drop_column(CommandRuns::PreviewPathTemplate)
                .to_owned(),
        )
        .await?;

        m.alter_table(
            Table::alter()
                .table(CommandTemplates::Table)
                .drop_column(CommandTemplates::PreviewPathTemplate)
                .to_owned(),
        )
        .await
    }
}

#[derive(DeriveIden)]
enum CommandTemplates {
    Table,
    PreviewPathTemplate,
}

#[derive(DeriveIden)]
enum CommandRuns {
    Table,
    PreviewPathTemplate,
    PreviewPath,
}
