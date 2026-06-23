use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "ai_image_generations",
            &[
                ("id", ColType::PkAuto),
                ("batch_id", ColType::String),
                ("config_key", ColType::String),
                ("config_name", ColType::String),
                ("prompt", ColType::Text),
                ("model", ColType::String),
                ("size", ColType::String),
                ("quality", ColType::String),
                ("output_index", ColType::IntegerWithDefault(0)),
                ("save_mode", ColType::String),
                ("local_output_path", ColType::TextNull),
                ("original_name", ColType::String),
                ("mime_type", ColType::StringNull),
                ("status", ColType::String),
                ("error_message", ColType::TextNull),
                ("reference_summary", ColType::TextNull),
                ("reference_count", ColType::IntegerWithDefault(0)),
            ],
            &[
                ("storage_profiles?", "storage_profile_id"),
                ("storage_buckets?", "storage_bucket_id"),
                ("upload_files?", "output_upload_file_id"),
                ("users?", "created_by"),
            ],
        )
        .await?;

        m.create_index(
            Index::create()
                .name("idx_ai_image_generations_created_status")
                .table(AiImageGenerations::Table)
                .col(AiImageGenerations::CreatedBy)
                .col(AiImageGenerations::Status)
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("idx_ai_image_generations_batch")
                .table(AiImageGenerations::Table)
                .col(AiImageGenerations::BatchId)
                .col(AiImageGenerations::OutputIndex)
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("idx_ai_image_generations_upload_file")
                .table(AiImageGenerations::Table)
                .col(AiImageGenerations::OutputUploadFileId)
                .to_owned(),
        )
        .await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.drop_index(
            Index::drop()
                .name("idx_ai_image_generations_upload_file")
                .table(AiImageGenerations::Table)
                .to_owned(),
        )
        .await?;
        m.drop_index(
            Index::drop()
                .name("idx_ai_image_generations_batch")
                .table(AiImageGenerations::Table)
                .to_owned(),
        )
        .await?;
        m.drop_index(
            Index::drop()
                .name("idx_ai_image_generations_created_status")
                .table(AiImageGenerations::Table)
                .to_owned(),
        )
        .await?;
        drop_table(m, "ai_image_generations").await
    }
}

#[derive(DeriveIden)]
enum AiImageGenerations {
    Table,
    CreatedBy,
    Status,
    BatchId,
    OutputIndex,
    OutputUploadFileId,
}
