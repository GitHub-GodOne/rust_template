#![allow(clippy::missing_errors_doc)]

use chrono::Local;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, IntoActiveModel, QueryFilter,
    QueryOrder, Set,
};
use tokio::time::{sleep, Duration};

use crate::{
    errors::{ApiError, ApiResult},
    models::_entities::{
        command_runs, command_templates, command_workflow_run_steps, command_workflow_runs,
        command_workflow_steps,
    },
    services::command_runner::{self, CommandRunRequest},
};

const POLL_INTERVAL_MS: u64 = 800;
const MAX_COMMAND_RUN_POLL_DB_ERRORS: u8 = 60;

pub async fn start_workflow_run(
    db: DatabaseConnection,
    workflow_id: i32,
    name: String,
    created_by: Option<i32>,
) -> ApiResult<command_workflow_runs::Model> {
    let run = command_workflow_runs::ActiveModel {
        workflow_id: Set(Some(workflow_id)),
        name: Set(name),
        status: Set("queued".to_string()),
        started_at: Set(None),
        finished_at: Set(None),
        duration_ms: Set(None),
        created_by: Set(created_by),
        error_message: Set(None),
        ..Default::default()
    }
    .insert(&db)
    .await?;

    tokio::spawn(run_workflow_task(db, run.id));
    Ok(run)
}

async fn run_workflow_task(db: DatabaseConnection, workflow_run_id: i32) {
    if let Err(error) = run_workflow_steps(&db, workflow_run_id).await {
        tracing::error!(
            workflow_run_id,
            error = error.message(),
            "failed to run command workflow"
        );
        let _ = finish_workflow_run(
            &db,
            workflow_run_id,
            "failed",
            Some(error.message().to_string()),
        )
        .await;
    }
}

async fn run_workflow_steps(db: &DatabaseConnection, workflow_run_id: i32) -> ApiResult<()> {
    mark_workflow_running(db, workflow_run_id).await?;
    let workflow_run = find_workflow_run(db, workflow_run_id).await?;
    let workflow_id = workflow_run
        .workflow_id
        .ok_or_else(|| ApiError::bad_request("workflow run has no workflow"))?;
    let steps = command_workflow_steps::Entity::find()
        .filter(command_workflow_steps::Column::WorkflowId.eq(workflow_id))
        .filter(command_workflow_steps::Column::Enabled.eq(true))
        .order_by_asc(command_workflow_steps::Column::SortOrder)
        .all(db)
        .await?;
    if steps.is_empty() {
        return finish_workflow_run(
            db,
            workflow_run_id,
            "failed",
            Some("workflow has no enabled steps".to_string()),
        )
        .await
        .map(|_| ());
    }

    for step in steps {
        let run_step = create_run_step(db, workflow_run_id, &step).await?;
        let template = command_templates::Entity::find_by_id(step.template_id)
            .one(db)
            .await?
            .ok_or_else(|| ApiError::bad_request("command template not found"))?;
        if !template.enabled {
            let message = format!("command template {} is disabled", template.name);
            mark_run_step_finished(db, run_step.id, "failed", None, Some(message.clone())).await?;
            finish_workflow_run(db, workflow_run_id, "failed", Some(message)).await?;
            return Ok(());
        }

        let args = step.args.clone().or_else(|| template.default_args.clone());
        let command_line = build_command_line(&template.command, args.as_deref());
        let command_run = command_runner::start_command_run(
            db.clone(),
            CommandRunRequest {
                template_id: Some(template.id),
                name: format!("{} / {}", workflow_run.name, step.name),
                working_directory: step
                    .working_directory
                    .clone()
                    .unwrap_or_else(|| template.working_directory.clone()),
                command_line,
                setup_script: template.setup_script.clone(),
                python_venv_path: template.python_venv_path.clone(),
                env_vars: step.env_vars.clone().or_else(|| template.env_vars.clone()),
                timeout_seconds: step.timeout_seconds.or(template.timeout_seconds),
                preview_path_template: template.preview_path_template.clone(),
                triggered_by: "workflow".to_string(),
                created_by: workflow_run.created_by,
            },
        )
        .await?;
        mark_run_step_started(db, run_step.id, command_run.id, args).await?;
        let command_run = wait_for_command_run(db, command_run.id).await?;
        if command_run.status == "success" {
            mark_run_step_finished(db, run_step.id, "success", Some(command_run.id), None).await?;
            continue;
        }
        let message = command_run
            .error_message
            .clone()
            .unwrap_or_else(|| format!("command finished with status {}", command_run.status));
        mark_run_step_finished(
            db,
            run_step.id,
            "failed",
            Some(command_run.id),
            Some(message.clone()),
        )
        .await?;
        finish_workflow_run(db, workflow_run_id, "failed", Some(message)).await?;
        return Ok(());
    }

    finish_workflow_run(db, workflow_run_id, "success", None).await?;
    Ok(())
}

async fn create_run_step(
    db: &DatabaseConnection,
    workflow_run_id: i32,
    step: &command_workflow_steps::Model,
) -> ApiResult<command_workflow_run_steps::Model> {
    Ok(command_workflow_run_steps::ActiveModel {
        workflow_run_id: Set(workflow_run_id),
        workflow_step_id: Set(Some(step.id)),
        command_run_id: Set(None),
        step_name: Set(step.name.clone()),
        sort_order: Set(step.sort_order),
        status: Set("queued".to_string()),
        resolved_args: Set(None),
        started_at: Set(None),
        finished_at: Set(None),
        error_message: Set(None),
        ..Default::default()
    }
    .insert(db)
    .await?)
}

async fn mark_workflow_running(
    db: &DatabaseConnection,
    workflow_run_id: i32,
) -> ApiResult<command_workflow_runs::Model> {
    let run = find_workflow_run(db, workflow_run_id).await?;
    let mut active = run.into_active_model();
    active.status = Set("running".to_string());
    active.started_at = Set(Some(Local::now().into()));
    Ok(active.update(db).await?)
}

async fn mark_run_step_started(
    db: &DatabaseConnection,
    run_step_id: i32,
    command_run_id: i32,
    args: Option<String>,
) -> ApiResult<command_workflow_run_steps::Model> {
    let run_step = find_run_step(db, run_step_id).await?;
    let mut active = run_step.into_active_model();
    active.status = Set("running".to_string());
    active.command_run_id = Set(Some(command_run_id));
    active.resolved_args = Set(args);
    active.started_at = Set(Some(Local::now().into()));
    Ok(active.update(db).await?)
}

async fn mark_run_step_finished(
    db: &DatabaseConnection,
    run_step_id: i32,
    status: &str,
    command_run_id: Option<i32>,
    error_message: Option<String>,
) -> ApiResult<command_workflow_run_steps::Model> {
    let run_step = find_run_step(db, run_step_id).await?;
    let mut active = run_step.into_active_model();
    active.status = Set(status.to_string());
    if command_run_id.is_some() {
        active.command_run_id = Set(command_run_id);
    }
    active.finished_at = Set(Some(Local::now().into()));
    active.error_message = Set(error_message);
    Ok(active.update(db).await?)
}

async fn finish_workflow_run(
    db: &DatabaseConnection,
    workflow_run_id: i32,
    status: &str,
    error_message: Option<String>,
) -> ApiResult<command_workflow_runs::Model> {
    let run = find_workflow_run(db, workflow_run_id).await?;
    let finished_at = Local::now();
    let duration_ms = run.started_at.map(|started_at| {
        i32::try_from((finished_at.fixed_offset() - started_at).num_milliseconds()).unwrap_or(0)
    });
    let mut active = run.into_active_model();
    active.status = Set(status.to_string());
    active.finished_at = Set(Some(finished_at.into()));
    active.duration_ms = Set(duration_ms);
    active.error_message = Set(error_message);
    Ok(active.update(db).await?)
}

async fn wait_for_command_run(
    db: &DatabaseConnection,
    command_run_id: i32,
) -> ApiResult<command_runs::Model> {
    let mut consecutive_db_errors = 0_u8;
    loop {
        let run = match command_runs::Entity::find_by_id(command_run_id)
            .one(db)
            .await
        {
            Ok(Some(run)) => {
                consecutive_db_errors = 0;
                run
            }
            Ok(None) => return Err(ApiError::bad_request("command run not found")),
            Err(error) => {
                consecutive_db_errors = consecutive_db_errors.saturating_add(1);
                tracing::warn!(
                    command_run_id,
                    consecutive_db_errors,
                    error = error.to_string(),
                    "failed to poll command run status"
                );
                if consecutive_db_errors >= MAX_COMMAND_RUN_POLL_DB_ERRORS {
                    return Err(ApiError::internal(
                        "command run polling failed after repeated database errors",
                    ));
                }
                sleep(Duration::from_millis(POLL_INTERVAL_MS)).await;
                continue;
            }
        };
        if !matches!(run.status.as_str(), "queued" | "running") {
            return Ok(run);
        }
        sleep(Duration::from_millis(POLL_INTERVAL_MS)).await;
    }
}

async fn find_workflow_run(
    db: &DatabaseConnection,
    workflow_run_id: i32,
) -> ApiResult<command_workflow_runs::Model> {
    command_workflow_runs::Entity::find_by_id(workflow_run_id)
        .one(db)
        .await?
        .ok_or_else(|| ApiError::bad_request("workflow run not found"))
}

async fn find_run_step(
    db: &DatabaseConnection,
    run_step_id: i32,
) -> ApiResult<command_workflow_run_steps::Model> {
    command_workflow_run_steps::Entity::find_by_id(run_step_id)
        .one(db)
        .await?
        .ok_or_else(|| ApiError::bad_request("workflow run step not found"))
}

fn build_command_line(command: &str, args: Option<&str>) -> String {
    let command = command.trim();
    args.map(str::trim)
        .filter(|value| !value.is_empty())
        .map_or_else(|| command.to_string(), |args| format!("{command} {args}"))
}
