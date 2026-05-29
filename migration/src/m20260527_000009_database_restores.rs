use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "database_restores",
            &[
                ("id", ColType::PkAuto),
                ("status", ColType::String),
                ("confirm_phrase", ColType::String),
                ("started_at", ColType::TimestampWithTimeZone),
                ("finished_at", ColType::TimestampWithTimeZoneNull),
                ("duration_ms", ColType::IntegerNull),
                ("output", ColType::TextNull),
                ("error_message", ColType::TextNull),
            ],
            &[
                ("database_backups", "backup_id"),
                ("database_backups?", "pre_restore_backup_id"),
                ("users?", "restored_by"),
            ],
        )
        .await?;
        create_restore_indexes(m).await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        drop_restore_indexes(m).await?;
        drop_table(m, "database_restores").await
    }
}

async fn create_restore_indexes(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    m.create_index(
        Index::create()
            .name("idx_database_restores_backup_created")
            .table(DatabaseRestores::Table)
            .col(DatabaseRestores::BackupId)
            .col(DatabaseRestores::CreatedAt)
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_database_restores_status_created")
            .table(DatabaseRestores::Table)
            .col(DatabaseRestores::Status)
            .col(DatabaseRestores::CreatedAt)
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_database_restores_restored_by_created")
            .table(DatabaseRestores::Table)
            .col(DatabaseRestores::RestoredBy)
            .col(DatabaseRestores::CreatedAt)
            .to_owned(),
    )
    .await
}

async fn drop_restore_indexes(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    m.drop_index(
        Index::drop()
            .name("idx_database_restores_restored_by_created")
            .table(DatabaseRestores::Table)
            .to_owned(),
    )
    .await?;
    m.drop_index(
        Index::drop()
            .name("idx_database_restores_status_created")
            .table(DatabaseRestores::Table)
            .to_owned(),
    )
    .await?;
    m.drop_index(
        Index::drop()
            .name("idx_database_restores_backup_created")
            .table(DatabaseRestores::Table)
            .to_owned(),
    )
    .await
}

#[derive(DeriveIden)]
enum DatabaseRestores {
    Table,
    BackupId,
    Status,
    RestoredBy,
    CreatedAt,
}
