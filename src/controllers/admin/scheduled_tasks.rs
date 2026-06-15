#![allow(clippy::missing_errors_doc)]

use chrono::offset::Local;
use loco_rs::prelude::*;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    Set,
};
use serde::{Deserialize, Serialize};

use crate::{
    controllers::admin::authorize,
    errors::{ApiError, ApiResult},
    models::{
        _entities::{scheduled_task_runs, scheduled_tasks},
        database_backups::{self, deliver_backup, BackupTrigger},
    },
    responses::{self, ApiResponse, EmptyData, PageResponse},
};

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct ScheduledTaskQueryParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub keyword: Option<String>,
    pub task_type: Option<String>,
    pub status: Option<String>,
    pub enabled: Option<bool>,
}

impl ScheduledTaskQueryParams {
    #[must_use]
    pub fn page(&self) -> u64 {
        self.page.unwrap_or(1).max(1)
    }

    #[must_use]
    pub fn page_size(&self) -> u64 {
        self.page_size.unwrap_or(10).clamp(1, 100)
    }
}

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct TaskRunQueryParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub task_id: Option<i32>,
    pub status: Option<String>,
}

impl TaskRunQueryParams {
    #[must_use]
    pub fn page(&self) -> u64 {
        self.page.unwrap_or(1).max(1)
    }

    #[must_use]
    pub fn page_size(&self) -> u64 {
        self.page_size.unwrap_or(10).clamp(1, 100)
    }
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct ScheduledTaskRecord {
    pub id: i32,
    pub name: String,
    pub code: String,
    pub task_type: String,
    pub cron_expr: String,
    pub payload: Option<String>,
    pub enabled: bool,
    pub status: String,
    pub last_run_at: Option<String>,
    pub next_run_at: Option<String>,
    pub created_by: Option<i32>,
    pub updated_by: Option<i32>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct TaskRunRecord {
    pub id: i32,
    pub task_id: i32,
    pub code: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub duration_ms: Option<i32>,
    pub output: Option<String>,
    pub error_message: Option<String>,
    pub triggered_by: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveScheduledTaskParams {
    pub name: String,
    pub code: String,
    pub task_type: String,
    pub cron_expr: String,
    pub payload: Option<String>,
    pub enabled: Option<bool>,
    pub status: Option<String>,
    pub next_run_at: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/admin/scheduled-tasks",
    tag = "admin-scheduled-tasks",
    security(("bearer_auth" = [])),
    params(ScheduledTaskQueryParams),
    responses((status = 200, body = ApiResponse<PageResponse<ScheduledTaskRecord>>))
)]
#[debug_handler]
pub async fn list(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<ScheduledTaskQueryParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:scheduled_task:list").await?;
    let page = params.page();
    let page_size = params.page_size();
    let mut query = scheduled_tasks::Entity::find().order_by_desc(scheduled_tasks::Column::Id);

    if let Some(keyword) = params.keyword.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(
            Condition::any()
                .add(scheduled_tasks::Column::Name.contains(keyword))
                .add(scheduled_tasks::Column::Code.contains(keyword)),
        );
    }
    if let Some(task_type) = params
        .task_type
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        query = query.filter(scheduled_tasks::Column::TaskType.eq(task_type));
    }
    if let Some(status) = params.status.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(scheduled_tasks::Column::Status.eq(status));
    }
    if let Some(enabled) = params.enabled {
        query = query.filter(scheduled_tasks::Column::Enabled.eq(enabled));
    }

    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(ScheduledTaskRecord::from)
        .collect();

    Ok(responses::ok(PageResponse {
        items,
        page,
        page_size,
        total,
    }))
}

#[utoipa::path(
    get,
    path = "/api/admin/scheduled-tasks/{id}",
    tag = "admin-scheduled-tasks",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<ScheduledTaskRecord>))
)]
#[debug_handler]
pub async fn get(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:scheduled_task:list").await?;
    let task = find_task(&ctx, id).await?;
    Ok(responses::ok(ScheduledTaskRecord::from(task)))
}

#[utoipa::path(
    post,
    path = "/api/admin/scheduled-tasks",
    tag = "admin-scheduled-tasks",
    security(("bearer_auth" = [])),
    request_body = SaveScheduledTaskParams,
    responses((status = 200, body = ApiResponse<ScheduledTaskRecord>))
)]
#[debug_handler]
pub async fn create(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SaveScheduledTaskParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:scheduled_task:create").await?;
    validate_task(&params)?;

    let task = scheduled_tasks::ActiveModel {
        name: Set(params.name),
        code: Set(params.code),
        task_type: Set(params.task_type),
        cron_expr: Set(params.cron_expr),
        payload: Set(params.payload),
        enabled: Set(params.enabled.unwrap_or(true)),
        status: Set(params.status.unwrap_or_else(|| "idle".to_string())),
        last_run_at: Set(None),
        next_run_at: Set(parse_time(params.next_run_at.as_deref())?),
        created_by: Set(Some(actor.id)),
        updated_by: Set(Some(actor.id)),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;

    Ok(responses::ok(ScheduledTaskRecord::from(task)))
}

#[utoipa::path(
    put,
    path = "/api/admin/scheduled-tasks/{id}",
    tag = "admin-scheduled-tasks",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = SaveScheduledTaskParams,
    responses((status = 200, body = ApiResponse<ScheduledTaskRecord>))
)]
#[debug_handler]
pub async fn update(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<SaveScheduledTaskParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:scheduled_task:update").await?;
    validate_task(&params)?;
    let task = find_task(&ctx, id).await?;

    let mut active = task.into_active_model();
    active.name = Set(params.name);
    active.code = Set(params.code);
    active.task_type = Set(params.task_type);
    active.cron_expr = Set(params.cron_expr);
    active.payload = Set(params.payload);
    active.enabled = Set(params.enabled.unwrap_or(true));
    active.status = Set(params.status.unwrap_or_else(|| "idle".to_string()));
    active.next_run_at = Set(parse_time(params.next_run_at.as_deref())?);
    active.updated_by = Set(Some(actor.id));
    let task = active.update(&ctx.db).await?;

    Ok(responses::ok(ScheduledTaskRecord::from(task)))
}

#[utoipa::path(
    delete,
    path = "/api/admin/scheduled-tasks/{id}",
    tag = "admin-scheduled-tasks",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn delete(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:scheduled_task:delete").await?;
    find_task(&ctx, id).await?;
    scheduled_tasks::Entity::delete_by_id(id)
        .exec(&ctx.db)
        .await?;
    Ok(responses::empty())
}

#[utoipa::path(
    post,
    path = "/api/admin/scheduled-tasks/{id}/run",
    tag = "admin-scheduled-tasks",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<TaskRunRecord>))
)]
#[debug_handler]
pub async fn run(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:scheduled_task:run").await?;
    let task = find_task(&ctx, id).await?;
    let run = run_task(&ctx, &task, Some(actor.id), "manual").await?;
    Ok(responses::ok(TaskRunRecord::from(run)))
}

#[utoipa::path(
    get,
    path = "/api/admin/scheduled-task-runs",
    tag = "admin-scheduled-tasks",
    security(("bearer_auth" = [])),
    params(TaskRunQueryParams),
    responses((status = 200, body = ApiResponse<PageResponse<TaskRunRecord>>))
)]
#[debug_handler]
pub async fn list_runs(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<TaskRunQueryParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:scheduled_task:list").await?;
    let page = params.page();
    let page_size = params.page_size();
    let mut query =
        scheduled_task_runs::Entity::find().order_by_desc(scheduled_task_runs::Column::Id);

    if let Some(task_id) = params.task_id {
        query = query.filter(scheduled_task_runs::Column::TaskId.eq(task_id));
    }
    if let Some(status) = params.status.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(scheduled_task_runs::Column::Status.eq(status));
    }

    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(TaskRunRecord::from)
        .collect();

    Ok(responses::ok(PageResponse {
        items,
        page,
        page_size,
        total,
    }))
}

pub async fn run_task(
    ctx: &AppContext,
    task: &scheduled_tasks::Model,
    actor_id: Option<i32>,
    triggered_by: &str,
) -> ApiResult<scheduled_task_runs::Model> {
    let started_at = Local::now();
    let result = execute_task(ctx, task, actor_id).await;
    let finished_at = Local::now();
    let duration_ms = i32::try_from((finished_at - started_at).num_milliseconds()).unwrap_or(0);
    let (status, output, error_message) = match result {
        Ok(output) => ("success", Some(output), None),
        Err(error) => ("failed", None, Some(error)),
    };

    let run = scheduled_task_runs::ActiveModel {
        task_id: Set(task.id),
        code: Set(task.code.clone()),
        status: Set(status.to_string()),
        started_at: Set(started_at.into()),
        finished_at: Set(Some(finished_at.into())),
        duration_ms: Set(Some(duration_ms)),
        output: Set(output),
        error_message: Set(error_message),
        triggered_by: Set(triggered_by.to_string()),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;

    let mut active = task.clone().into_active_model();
    active.status = Set(status.to_string());
    active.last_run_at = Set(Some(finished_at.into()));
    active.update(&ctx.db).await?;

    Ok(run)
}

async fn execute_task(
    ctx: &AppContext,
    task: &scheduled_tasks::Model,
    actor_id: Option<i32>,
) -> Result<String, String> {
    match task.task_type.as_str() {
        "cleanup_logs" => Ok("cleanup_logs task acknowledged".to_string()),
        "webhook_notify" => Ok("webhook_notify task acknowledged".to_string()),
        "database_backup" => {
            let backup = database_backups::create_postgres_backup(
                &ctx.db,
                &ctx.config.database.uri,
                actor_id,
                BackupTrigger::Scheduled,
            )
            .await
            .map_err(|error| format!("{error:?}"))?;
            let backup = deliver_backup(&ctx.db, backup)
                .await
                .map_err(|error| format!("{error:?}"))?;
            Ok(format!(
                "database backup {} finished with {}, delivery {}",
                backup.id,
                backup.status,
                backup.delivery_status.as_deref().unwrap_or("-")
            ))
        }
        _ => Err("unsupported task type".to_string()),
    }
}

async fn find_task(ctx: &AppContext, id: i32) -> ApiResult<scheduled_tasks::Model> {
    scheduled_tasks::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("scheduled task not found"))
}

fn validate_task(params: &SaveScheduledTaskParams) -> ApiResult<()> {
    if params.name.trim().is_empty()
        || params.code.trim().is_empty()
        || params.cron_expr.trim().is_empty()
    {
        return Err(ApiError::bad_request(
            "task name, code and cron are required",
        ));
    }
    match params.task_type.as_str() {
        "database_backup" | "webhook_notify" | "cleanup_logs" => Ok(()),
        _ => Err(ApiError::bad_request("unsupported task type")),
    }
}

fn parse_time(value: Option<&str>) -> ApiResult<Option<chrono::DateTime<chrono::FixedOffset>>> {
    value
        .filter(|value| !value.is_empty())
        .map(chrono::DateTime::parse_from_rfc3339)
        .transpose()
        .map_err(|_| ApiError::bad_request("time must be RFC3339"))
}

impl From<scheduled_tasks::Model> for ScheduledTaskRecord {
    fn from(task: scheduled_tasks::Model) -> Self {
        Self {
            id: task.id,
            name: task.name,
            code: task.code,
            task_type: task.task_type,
            cron_expr: task.cron_expr,
            payload: task.payload,
            enabled: task.enabled,
            status: task.status,
            last_run_at: task.last_run_at.map(|value| value.to_rfc3339()),
            next_run_at: task.next_run_at.map(|value| value.to_rfc3339()),
            created_by: task.created_by,
            updated_by: task.updated_by,
            created_at: task.created_at.to_rfc3339(),
            updated_at: task.updated_at.to_rfc3339(),
        }
    }
}

impl From<scheduled_task_runs::Model> for TaskRunRecord {
    fn from(run: scheduled_task_runs::Model) -> Self {
        Self {
            id: run.id,
            task_id: run.task_id,
            code: run.code,
            status: run.status,
            started_at: run.started_at.to_rfc3339(),
            finished_at: run.finished_at.map(|value| value.to_rfc3339()),
            duration_ms: run.duration_ms,
            output: run.output,
            error_message: run.error_message,
            triggered_by: run.triggered_by,
            created_at: run.created_at.to_rfc3339(),
            updated_at: run.updated_at.to_rfc3339(),
        }
    }
}
