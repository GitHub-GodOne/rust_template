use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "command_workflows",
            &[
                ("id", ColType::PkAuto),
                ("name", ColType::String),
                ("code", ColType::StringUniq),
                ("description", ColType::TextNull),
                ("enabled", ColType::BooleanWithDefault(true)),
            ],
            &[("users?", "created_by"), ("users?", "updated_by")],
        )
        .await?;

        create_table(
            m,
            "command_workflow_steps",
            &[
                ("id", ColType::PkAuto),
                ("name", ColType::String),
                ("sort_order", ColType::Integer),
                ("args", ColType::TextNull),
                ("env_vars", ColType::TextNull),
                ("working_directory", ColType::StringNull),
                ("timeout_seconds", ColType::IntegerNull),
                ("enabled", ColType::BooleanWithDefault(true)),
            ],
            &[
                ("command_workflows", "workflow_id"),
                ("command_templates", "template_id"),
            ],
        )
        .await?;

        create_table(
            m,
            "command_workflow_runs",
            &[
                ("id", ColType::PkAuto),
                ("name", ColType::String),
                ("status", ColType::String),
                ("started_at", ColType::TimestampWithTimeZoneNull),
                ("finished_at", ColType::TimestampWithTimeZoneNull),
                ("duration_ms", ColType::IntegerNull),
                ("error_message", ColType::TextNull),
            ],
            &[
                ("command_workflows?", "workflow_id"),
                ("users?", "created_by"),
            ],
        )
        .await?;

        create_table(
            m,
            "command_workflow_run_steps",
            &[
                ("id", ColType::PkAuto),
                ("step_name", ColType::String),
                ("sort_order", ColType::Integer),
                ("status", ColType::String),
                ("resolved_args", ColType::TextNull),
                ("started_at", ColType::TimestampWithTimeZoneNull),
                ("finished_at", ColType::TimestampWithTimeZoneNull),
                ("error_message", ColType::TextNull),
            ],
            &[
                ("command_workflow_runs", "workflow_run_id"),
                ("command_workflow_steps?", "workflow_step_id"),
                ("command_runs?", "command_run_id"),
            ],
        )
        .await?;

        m.create_index(
            Index::create()
                .name("idx_command_workflow_steps_workflow_order")
                .table(CommandWorkflowSteps::Table)
                .col(CommandWorkflowSteps::WorkflowId)
                .col(CommandWorkflowSteps::SortOrder)
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("idx_command_workflow_runs_status")
                .table(CommandWorkflowRuns::Table)
                .col(CommandWorkflowRuns::Status)
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("idx_command_workflow_run_steps_run_order")
                .table(CommandWorkflowRunSteps::Table)
                .col(CommandWorkflowRunSteps::WorkflowRunId)
                .col(CommandWorkflowRunSteps::SortOrder)
                .to_owned(),
        )
        .await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.drop_index(
            Index::drop()
                .name("idx_command_workflow_run_steps_run_order")
                .table(CommandWorkflowRunSteps::Table)
                .to_owned(),
        )
        .await?;
        m.drop_index(
            Index::drop()
                .name("idx_command_workflow_runs_status")
                .table(CommandWorkflowRuns::Table)
                .to_owned(),
        )
        .await?;
        m.drop_index(
            Index::drop()
                .name("idx_command_workflow_steps_workflow_order")
                .table(CommandWorkflowSteps::Table)
                .to_owned(),
        )
        .await?;
        drop_table(m, "command_workflow_run_steps").await?;
        drop_table(m, "command_workflow_runs").await?;
        drop_table(m, "command_workflow_steps").await?;
        drop_table(m, "command_workflows").await
    }
}

#[derive(DeriveIden)]
enum CommandWorkflowSteps {
    Table,
    WorkflowId,
    SortOrder,
}

#[derive(DeriveIden)]
enum CommandWorkflowRuns {
    Table,
    Status,
}

#[derive(DeriveIden)]
enum CommandWorkflowRunSteps {
    Table,
    WorkflowRunId,
    SortOrder,
}
