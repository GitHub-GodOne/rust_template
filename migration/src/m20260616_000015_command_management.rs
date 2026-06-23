use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "command_templates",
            &[
                ("id", ColType::PkAuto),
                ("name", ColType::String),
                ("code", ColType::StringUniq),
                ("description", ColType::TextNull),
                ("working_directory", ColType::String),
                ("command", ColType::Text),
                ("default_args", ColType::TextNull),
                ("env_vars", ColType::TextNull),
                ("setup_script", ColType::TextNull),
                ("python_venv_path", ColType::StringNull),
                ("timeout_seconds", ColType::IntegerNull),
                ("enabled", ColType::BooleanWithDefault(true)),
            ],
            &[("users?", "created_by"), ("users?", "updated_by")],
        )
        .await?;

        create_table(
            m,
            "command_runs",
            &[
                ("id", ColType::PkAuto),
                ("name", ColType::String),
                ("working_directory", ColType::String),
                ("command_line", ColType::Text),
                ("effective_script", ColType::Text),
                ("status", ColType::String),
                ("exit_code", ColType::IntegerNull),
                ("started_at", ColType::TimestampWithTimeZoneNull),
                ("finished_at", ColType::TimestampWithTimeZoneNull),
                ("duration_ms", ColType::IntegerNull),
                ("triggered_by", ColType::String),
                ("error_message", ColType::TextNull),
                ("output_tail", ColType::TextNull),
            ],
            &[
                ("command_templates?", "template_id"),
                ("users?", "created_by"),
            ],
        )
        .await?;

        create_table(
            m,
            "command_run_logs",
            &[
                ("id", ColType::PkAuto),
                ("seq", ColType::Integer),
                ("stream", ColType::String),
                ("chunk", ColType::Text),
            ],
            &[("command_runs", "run_id")],
        )
        .await?;

        m.create_index(
            Index::create()
                .name("idx_command_runs_template_status")
                .table(CommandRuns::Table)
                .col(CommandRuns::TemplateId)
                .col(CommandRuns::Status)
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("idx_command_runs_status")
                .table(CommandRuns::Table)
                .col(CommandRuns::Status)
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("idx_command_run_logs_run_seq")
                .table(CommandRunLogs::Table)
                .col(CommandRunLogs::RunId)
                .col(CommandRunLogs::Seq)
                .to_owned(),
        )
        .await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.drop_index(
            Index::drop()
                .name("idx_command_run_logs_run_seq")
                .table(CommandRunLogs::Table)
                .to_owned(),
        )
        .await?;
        m.drop_index(
            Index::drop()
                .name("idx_command_runs_status")
                .table(CommandRuns::Table)
                .to_owned(),
        )
        .await?;
        m.drop_index(
            Index::drop()
                .name("idx_command_runs_template_status")
                .table(CommandRuns::Table)
                .to_owned(),
        )
        .await?;
        drop_table(m, "command_run_logs").await?;
        drop_table(m, "command_runs").await?;
        drop_table(m, "command_templates").await
    }
}

#[derive(DeriveIden)]
enum CommandRuns {
    Table,
    TemplateId,
    Status,
}

#[derive(DeriveIden)]
enum CommandRunLogs {
    Table,
    RunId,
    Seq,
}
