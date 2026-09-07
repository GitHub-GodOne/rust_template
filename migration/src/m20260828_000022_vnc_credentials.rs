use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "vnc_credentials",
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
                .name("idx_vnc_credentials_user_target")
                .table(VncCredentials::Table)
                .col(VncCredentials::UserId)
                .col(VncCredentials::TargetKey)
                .unique()
                .to_owned(),
        )
        .await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.drop_index(
            Index::drop()
                .name("idx_vnc_credentials_user_target")
                .table(VncCredentials::Table)
                .to_owned(),
        )
        .await?;
        drop_table(m, "vnc_credentials").await
    }
}

#[derive(DeriveIden)]
enum VncCredentials {
    Table,
    UserId,
    TargetKey,
}
