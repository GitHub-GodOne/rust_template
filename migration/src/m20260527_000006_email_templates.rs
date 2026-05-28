use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "email_templates",
            &[
                ("id", ColType::PkAuto),
                ("code", ColType::StringUniq),
                ("name", ColType::String),
                ("template_type", ColType::String),
                ("subject", ColType::Text),
                ("html_body", ColType::Text),
                ("text_body", ColType::Text),
                ("variables", ColType::Text),
                ("enabled", ColType::BooleanWithDefault(true)),
                ("is_builtin", ColType::BooleanWithDefault(false)),
                ("description", ColType::TextNull),
            ],
            &[("users?", "created_by"), ("users?", "updated_by")],
        )
        .await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        drop_table(m, "email_templates").await
    }
}
