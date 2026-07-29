use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "command_api_keys",
            &[
                ("id", ColType::PkAuto),
                ("name", ColType::String),
                ("token", ColType::Text),
                ("description", ColType::TextNull),
                ("enabled", ColType::BooleanWithDefault(true)),
                ("last_used_at", ColType::TimestampWithTimeZoneNull),
            ],
            &[("users?", "created_by"), ("users?", "updated_by")],
        )
        .await?;

        m.create_index(
            Index::create()
                .name("idx_command_api_keys_token")
                .table(CommandApiKeys::Table)
                .col(CommandApiKeys::Token)
                .unique()
                .to_owned(),
        )
        .await?;

        m.create_index(
            Index::create()
                .name("idx_command_api_keys_enabled")
                .table(CommandApiKeys::Table)
                .col(CommandApiKeys::Enabled)
                .to_owned(),
        )
        .await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.drop_index(
            Index::drop()
                .name("idx_command_api_keys_enabled")
                .table(CommandApiKeys::Table)
                .to_owned(),
        )
        .await?;
        m.drop_index(
            Index::drop()
                .name("idx_command_api_keys_token")
                .table(CommandApiKeys::Table)
                .to_owned(),
        )
        .await?;
        drop_table(m, "command_api_keys").await
    }
}

#[derive(DeriveIden)]
enum CommandApiKeys {
    Table,
    Token,
    Enabled,
}
