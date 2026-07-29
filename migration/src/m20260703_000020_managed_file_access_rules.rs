use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "managed_file_access_rules",
            &[
                ("id", ColType::PkAuto),
                ("root_key", ColType::String),
                ("path", ColType::Text),
                ("visibility", ColType::String),
                ("public_token", ColType::StringNull),
                ("password_hash", ColType::TextNull),
                ("enabled", ColType::BooleanWithDefault(true)),
            ],
            &[("users?", "created_by"), ("users?", "updated_by")],
        )
        .await?;

        m.create_index(
            Index::create()
                .name("idx_managed_file_access_rules_root_path")
                .table(ManagedFileAccessRules::Table)
                .col(ManagedFileAccessRules::RootKey)
                .col(ManagedFileAccessRules::Path)
                .unique()
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("idx_managed_file_access_rules_public_token")
                .table(ManagedFileAccessRules::Table)
                .col(ManagedFileAccessRules::PublicToken)
                .unique()
                .to_owned(),
        )
        .await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.drop_index(
            Index::drop()
                .name("idx_managed_file_access_rules_public_token")
                .table(ManagedFileAccessRules::Table)
                .to_owned(),
        )
        .await?;
        m.drop_index(
            Index::drop()
                .name("idx_managed_file_access_rules_root_path")
                .table(ManagedFileAccessRules::Table)
                .to_owned(),
        )
        .await?;
        drop_table(m, "managed_file_access_rules").await
    }
}

#[derive(DeriveIden)]
enum ManagedFileAccessRules {
    Table,
    RootKey,
    Path,
    PublicToken,
}
