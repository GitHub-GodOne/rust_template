use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "rdp_credentials",
            &[
                ("id", ColType::PkAuto),
                ("target_key", ColType::String),
                ("encrypted_payload", ColType::Text),
            ],
            &[("users", "user_id")],
        )
        .await?;

        m.create_index(
            Index::create()
                .name("idx_rdp_credentials_user_target")
                .table(RdpCredentials::Table)
                .col(RdpCredentials::UserId)
                .col(RdpCredentials::TargetKey)
                .unique()
                .to_owned(),
        )
        .await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.drop_index(
            Index::drop()
                .name("idx_rdp_credentials_user_target")
                .table(RdpCredentials::Table)
                .to_owned(),
        )
        .await?;
        drop_table(m, "rdp_credentials").await
    }
}

#[derive(DeriveIden)]
enum RdpCredentials {
    Table,
    UserId,
    TargetKey,
}
