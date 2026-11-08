use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// Stores the optional client IP/CIDR allowlist for each domain mapping.
/// An empty JSON array means every client IP is allowed.
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Alias::new("domain_mappings"))
                    .add_column(
                        ColumnDef::new(Alias::new("allowed_ips"))
                            .text()
                            .not_null()
                            .default("[]")
                            .to_owned(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Alias::new("domain_mappings"))
                    .drop_column(Alias::new("allowed_ips"))
                    .to_owned(),
            )
            .await
    }
}
