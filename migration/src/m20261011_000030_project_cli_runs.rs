use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// Run history and output of the 项目命令 page.
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            "project_cli_runs",
            &[
                ("id", ColType::PkAuto),
                // JSON array of command words, e.g. ["db", "migrate"].
                ("path", ColType::Text),
                // JSON array of arguments with secrets masked.
                ("args", ColType::Text),
                ("command_line", ColType::Text),
                ("environment", ColType::String),
                ("status", ColType::String),
                ("exit_code", ColType::IntegerNull),
                ("error", ColType::TextNull),
                ("started_at", ColType::TimestampWithTimeZone),
                ("finished_at", ColType::TimestampWithTimeZoneNull),
                ("duration_ms", ColType::BigIntegerNull),
                ("created_by_name", ColType::String),
                ("last_seq", ColType::Integer),
                ("output_bytes", ColType::BigInteger),
                ("truncated", ColType::Boolean),
            ],
            &[("users?", "created_by")],
        )
        .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_project_cli_runs_status")
                    .table(ProjectCliRuns::Table)
                    .col(ProjectCliRuns::Status)
                    .to_owned(),
            )
            .await?;

        create_table(
            manager,
            "project_cli_run_logs",
            &[
                ("id", ColType::PkAuto),
                ("seq", ColType::Integer),
                ("stream", ColType::String),
                ("text", ColType::Text),
            ],
            &[("project_cli_runs", "run_id")],
        )
        .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_project_cli_run_logs_run_seq")
                    .table(ProjectCliRunLogs::Table)
                    .col(ProjectCliRunLogs::RunId)
                    .col(ProjectCliRunLogs::Seq)
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_table(manager, "project_cli_run_logs").await?;
        drop_table(manager, "project_cli_runs").await
    }
}

#[derive(DeriveIden)]
enum ProjectCliRuns {
    Table,
    Status,
}

#[derive(DeriveIden)]
enum ProjectCliRunLogs {
    Table,
    RunId,
    Seq,
}
