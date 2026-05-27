use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "operation_logs",
            &[
                ("id", ColType::PkAuto),
                ("trace_id", ColType::StringNull),
                ("log_type", ColType::String),
                ("level", ColType::String),
                ("module", ColType::String),
                ("action", ColType::String),
                ("message", ColType::Text),
                ("method", ColType::StringNull),
                ("path", ColType::StringNull),
                ("status", ColType::IntegerNull),
                ("duration_ms", ColType::IntegerNull),
                ("ip", ColType::StringNull),
                ("user_agent", ColType::TextNull),
                ("operator", ColType::StringNull),
                ("request_summary", ColType::TextNull),
                ("response_summary", ColType::TextNull),
                ("error_message", ColType::TextNull),
            ],
            &[("users?", "user_id")],
        )
        .await?;

        create_table(
            m,
            "system_settings",
            &[
                ("id", ColType::PkAuto),
                ("key", ColType::StringUniq),
                ("name", ColType::String),
                ("group_key", ColType::String),
                ("value", ColType::Text),
                ("value_type", ColType::String),
                ("default_value", ColType::TextNull),
                ("description", ColType::TextNull),
                ("is_public", ColType::BooleanWithDefault(false)),
                ("is_builtin", ColType::BooleanWithDefault(false)),
                ("is_encrypted", ColType::BooleanWithDefault(false)),
                ("sort_order", ColType::IntegerWithDefault(0)),
            ],
            &[("users?", "created_by"), ("users?", "updated_by")],
        )
        .await?;

        create_table(
            m,
            "dict_types",
            &[
                ("id", ColType::PkAuto),
                ("code", ColType::StringUniq),
                ("name", ColType::String),
                ("description", ColType::TextNull),
                ("enabled", ColType::BooleanWithDefault(true)),
                ("is_builtin", ColType::BooleanWithDefault(false)),
                ("sort_order", ColType::IntegerWithDefault(0)),
            ],
            &[],
        )
        .await?;

        create_table(
            m,
            "dict_items",
            &[
                ("id", ColType::PkAuto),
                ("label", ColType::String),
                ("value", ColType::String),
                ("color", ColType::StringNull),
                ("extra", ColType::TextNull),
                ("enabled", ColType::BooleanWithDefault(true)),
                ("is_default", ColType::BooleanWithDefault(false)),
                ("sort_order", ColType::IntegerWithDefault(0)),
            ],
            &[("dict_types", "dict_type_id")],
        )
        .await?;

        create_table(
            m,
            "upload_files",
            &[
                ("id", ColType::PkAuto),
                ("storage", ColType::String),
                ("object_key", ColType::StringUniq),
                ("url", ColType::String),
                ("original_name", ColType::String),
                ("filename", ColType::String),
                ("extension", ColType::StringNull),
                ("mime_type", ColType::StringNull),
                ("size_bytes", ColType::BigInteger),
                ("sha256", ColType::String),
                ("category", ColType::StringNull),
                ("tags", ColType::TextNull),
                ("visibility", ColType::String),
                ("status", ColType::String),
            ],
            &[("users?", "uploader_id")],
        )
        .await?;

        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        drop_table(m, "upload_files").await?;
        drop_table(m, "dict_items").await?;
        drop_table(m, "dict_types").await?;
        drop_table(m, "system_settings").await?;
        drop_table(m, "operation_logs").await?;
        Ok(())
    }
}
