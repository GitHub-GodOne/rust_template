use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "refresh_tokens",
            &[
                ("id", ColType::PkAuto),
                ("token_hash", ColType::StringUniq),
                ("expires_at", ColType::TimestampWithTimeZone),
                ("revoked_at", ColType::TimestampWithTimeZoneNull),
                ("created_by_ip", ColType::StringNull),
                ("user_agent", ColType::StringNull),
            ],
            &[("users", "user_id")],
        )
        .await?;
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        drop_table(m, "refresh_tokens").await?;
        Ok(())
    }
}
