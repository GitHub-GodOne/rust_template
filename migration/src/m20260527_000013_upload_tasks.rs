use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "upload_tasks",
            &[
                ("id", ColType::PkAuto),
                ("storage", ColType::String),
                ("bucket", ColType::StringNull),
                ("prefix", ColType::StringNull),
                ("object_key", ColType::String),
                ("original_name", ColType::String),
                ("filename", ColType::String),
                ("extension", ColType::StringNull),
                ("mime_type", ColType::StringNull),
                ("size_bytes", ColType::BigInteger),
                ("chunk_size", ColType::BigInteger),
                ("total_chunks", ColType::Integer),
                ("uploaded_chunks", ColType::Text),
                ("uploaded_bytes", ColType::BigInteger),
                ("sha256", ColType::StringNull),
                ("category", ColType::StringNull),
                ("tags", ColType::StringNull),
                ("visibility", ColType::String),
                ("status", ColType::String),
                ("error_message", ColType::TextNull),
                ("completed_at", ColType::TimestampWithTimeZoneNull),
                ("upload_file_id", ColType::IntegerNull),
            ],
            &[
                ("storage_profiles?", "storage_profile_id"),
                ("storage_buckets?", "storage_bucket_id"),
                ("users?", "uploader_id"),
            ],
        )
        .await?;

        m.create_index(
            Index::create()
                .name("idx_upload_tasks_uploader_status")
                .table(UploadTasks::Table)
                .col(UploadTasks::UploaderId)
                .col(UploadTasks::Status)
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("idx_upload_tasks_bucket_object")
                .table(UploadTasks::Table)
                .col(UploadTasks::StorageBucketId)
                .col(UploadTasks::ObjectKey)
                .to_owned(),
        )
        .await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.drop_index(
            Index::drop()
                .name("idx_upload_tasks_bucket_object")
                .table(UploadTasks::Table)
                .to_owned(),
        )
        .await?;
        m.drop_index(
            Index::drop()
                .name("idx_upload_tasks_uploader_status")
                .table(UploadTasks::Table)
                .to_owned(),
        )
        .await?;
        drop_table(m, "upload_tasks").await
    }
}

#[derive(DeriveIden)]
enum UploadTasks {
    Table,
    UploaderId,
    Status,
    StorageBucketId,
    ObjectKey,
}
