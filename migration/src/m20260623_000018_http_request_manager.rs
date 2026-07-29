use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "http_request_presets",
            &[
                ("id", ColType::PkAuto),
                ("name", ColType::String),
                ("code", ColType::StringUniq),
                ("description", ColType::TextNull),
                ("method", ColType::String),
                ("url", ColType::Text),
                ("headers_json", ColType::TextNull),
                ("query_json", ColType::TextNull),
                ("body_type", ColType::String),
                ("body_text", ColType::TextNull),
                ("content_type", ColType::StringNull),
                ("source_format", ColType::String),
                ("last_used_at", ColType::TimestampWithTimeZoneNull),
                ("enabled", ColType::BooleanWithDefault(true)),
            ],
            &[("users?", "created_by"), ("users?", "updated_by")],
        )
        .await?;

        create_table(
            m,
            "http_request_runs",
            &[
                ("id", ColType::PkAuto),
                ("name", ColType::String),
                ("method", ColType::String),
                ("url", ColType::Text),
                ("headers_json", ColType::TextNull),
                ("query_json", ColType::TextNull),
                ("request_body_type", ColType::String),
                ("request_body_text", ColType::TextNull),
                ("request_body_size_bytes", ColType::BigInteger),
                ("status", ColType::String),
                ("response_status_code", ColType::IntegerNull),
                ("response_headers_json", ColType::TextNull),
                ("response_content_type", ColType::StringNull),
                ("response_size_bytes", ColType::BigInteger),
                ("duration_ms", ColType::Integer),
                ("error_message", ColType::TextNull),
                ("response_storage_path", ColType::TextNull),
                ("response_preview_text", ColType::TextNull),
            ],
            &[
                ("http_request_presets?", "preset_id"),
                ("users?", "created_by"),
            ],
        )
        .await?;

        m.create_index(
            Index::create()
                .name("idx_http_request_presets_enabled")
                .table(HttpRequestPresets::Table)
                .col(HttpRequestPresets::Enabled)
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("idx_http_request_runs_preset_created")
                .table(HttpRequestRuns::Table)
                .col(HttpRequestRuns::PresetId)
                .col(HttpRequestRuns::CreatedAt)
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("idx_http_request_runs_status")
                .table(HttpRequestRuns::Table)
                .col(HttpRequestRuns::Status)
                .to_owned(),
        )
        .await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.drop_index(
            Index::drop()
                .name("idx_http_request_runs_status")
                .table(HttpRequestRuns::Table)
                .to_owned(),
        )
        .await?;
        m.drop_index(
            Index::drop()
                .name("idx_http_request_runs_preset_created")
                .table(HttpRequestRuns::Table)
                .to_owned(),
        )
        .await?;
        m.drop_index(
            Index::drop()
                .name("idx_http_request_presets_enabled")
                .table(HttpRequestPresets::Table)
                .to_owned(),
        )
        .await?;
        drop_table(m, "http_request_runs").await?;
        drop_table(m, "http_request_presets").await
    }
}

#[derive(DeriveIden)]
enum HttpRequestPresets {
    Table,
    Enabled,
}

#[derive(DeriveIden)]
enum HttpRequestRuns {
    Table,
    PresetId,
    CreatedAt,
    Status,
}
