use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(RatholeProfiles::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(RatholeProfiles::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(RatholeProfiles::Name).string().not_null())
                    .col(ColumnDef::new(RatholeProfiles::Mode).string().not_null())
                    .col(
                        ColumnDef::new(RatholeProfiles::ConfigToml)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(RatholeProfiles::Enabled)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(RatholeProfiles::Status)
                            .string()
                            .not_null()
                            .default("stopped"),
                    )
                    .col(ColumnDef::new(RatholeProfiles::LastError).text())
                    .col(ColumnDef::new(RatholeProfiles::LastStartedAt).timestamp_with_time_zone())
                    .col(ColumnDef::new(RatholeProfiles::LastStoppedAt).timestamp_with_time_zone())
                    .col(ColumnDef::new(RatholeProfiles::CreatedBy).integer())
                    .col(ColumnDef::new(RatholeProfiles::UpdatedBy).integer())
                    .col(
                        ColumnDef::new(RatholeProfiles::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(RatholeProfiles::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(RatholeProfiles::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum RatholeProfiles {
    Table,
    Id,
    Name,
    Mode,
    ConfigToml,
    Enabled,
    Status,
    LastError,
    LastStartedAt,
    LastStoppedAt,
    CreatedBy,
    UpdatedBy,
    CreatedAt,
    UpdatedAt,
}
