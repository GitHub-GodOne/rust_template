#![allow(clippy::missing_errors_doc)]

use std::{
    collections::HashMap,
    fs,
    path::Path as FsPath,
    sync::{Mutex, OnceLock},
};

use axum::{
    extract::{ws::Message, ws::WebSocket, ws::WebSocketUpgrade, Path},
    http::{header, HeaderMap, HeaderName, HeaderValue},
    response::IntoResponse,
};
use chrono::{Duration, Utc};
use loco_rs::prelude::*;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, EntityTrait, IntoActiveModel, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, Set,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const COMMAND_LOG_TICKET_TTL_SECONDS: i64 = 60;

static COMMAND_LOG_TICKETS: OnceLock<Mutex<HashMap<String, CommandLogTicket>>> = OnceLock::new();

#[derive(Debug, Clone)]
struct CommandLogTicket {
    run_id: i32,
    expires_at: chrono::DateTime<Utc>,
}

use crate::{
    controllers::admin::authorize,
    errors::{ApiError, ApiResult},
    models::{
        _entities::{
            command_api_keys, command_run_logs, command_runs, command_templates,
            command_workflow_run_steps, command_workflow_runs, command_workflow_steps,
            command_workflows, users,
        },
        rbac,
    },
    responses::{self, ApiResponse, EmptyData, PageResponse},
    services::{
        command_runner::{self, CommandRunRequest},
        command_workflow_runner,
    },
};

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct CommandTemplateQueryParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub keyword: Option<String>,
    pub enabled: Option<bool>,
}

impl CommandTemplateQueryParams {
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
pub struct CommandRunQueryParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub template_id: Option<i32>,
    pub status: Option<String>,
    pub keyword: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct CommandApiKeyQueryParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub keyword: Option<String>,
    pub enabled: Option<bool>,
}

impl CommandApiKeyQueryParams {
    #[must_use]
    pub fn page(&self) -> u64 {
        self.page.unwrap_or(1).max(1)
    }

    #[must_use]
    pub fn page_size(&self) -> u64 {
        self.page_size.unwrap_or(10).clamp(1, 100)
    }
}

impl CommandRunQueryParams {
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
pub struct CommandWorkflowQueryParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub keyword: Option<String>,
    pub enabled: Option<bool>,
}

impl CommandWorkflowQueryParams {
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
pub struct CommandWorkflowRunQueryParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub workflow_id: Option<i32>,
    pub status: Option<String>,
    pub keyword: Option<String>,
}

impl CommandWorkflowRunQueryParams {
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
pub struct CommandRunLogQueryParams {
    pub after_seq: Option<i32>,
    pub limit: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CommandTemplateRecord {
    pub id: i32,
    pub name: String,
    pub code: String,
    pub description: Option<String>,
    pub working_directory: String,
    pub command: String,
    pub default_args: Option<String>,
    pub env_vars: Option<String>,
    pub setup_script: Option<String>,
    pub python_venv_path: Option<String>,
    pub timeout_seconds: Option<i32>,
    pub preview_path_template: Option<String>,
    pub enabled: bool,
    pub created_by: Option<i32>,
    pub updated_by: Option<i32>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CommandRunRecord {
    pub id: i32,
    pub template_id: Option<i32>,
    pub name: String,
    pub working_directory: String,
    pub command_line: String,
    pub effective_script: String,
    pub status: String,
    pub retry_on_failure: bool,
    pub max_retries: i32,
    pub retry_delay_seconds: i32,
    pub attempt_count: i32,
    pub exit_code: Option<i32>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub duration_ms: Option<i32>,
    pub triggered_by: String,
    pub created_by: Option<i32>,
    pub error_message: Option<String>,
    pub output_tail: Option<String>,
    pub preview_path_template: Option<String>,
    pub preview_path: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CommandApiKeyRecord {
    pub id: i32,
    pub name: String,
    pub token: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub last_used_at: Option<String>,
    pub created_by: Option<i32>,
    pub updated_by: Option<i32>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CommandRunLogTicketRecord {
    pub ticket: String,
    pub expires_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CommandRunLogRecord {
    pub id: i32,
    pub run_id: i32,
    pub seq: i32,
    pub stream: String,
    pub chunk: String,
    pub created_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CommandWorkflowStepRecord {
    pub id: i32,
    pub workflow_id: i32,
    pub template_id: i32,
    pub name: String,
    pub sort_order: i32,
    pub args: Option<String>,
    pub env_vars: Option<String>,
    pub working_directory: Option<String>,
    pub timeout_seconds: Option<i32>,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CommandWorkflowRecord {
    pub id: i32,
    pub name: String,
    pub code: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub created_by: Option<i32>,
    pub updated_by: Option<i32>,
    pub steps: Vec<CommandWorkflowStepRecord>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CommandWorkflowRunStepRecord {
    pub id: i32,
    pub workflow_run_id: i32,
    pub workflow_step_id: Option<i32>,
    pub command_run_id: Option<i32>,
    pub step_name: String,
    pub sort_order: i32,
    pub status: String,
    pub resolved_args: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CommandWorkflowRunRecord {
    pub id: i32,
    pub workflow_id: Option<i32>,
    pub name: String,
    pub status: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub duration_ms: Option<i32>,
    pub created_by: Option<i32>,
    pub error_message: Option<String>,
    pub steps: Vec<CommandWorkflowRunStepRecord>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveCommandTemplateParams {
    pub name: String,
    pub code: String,
    pub description: Option<String>,
    pub working_directory: String,
    pub command: String,
    pub default_args: Option<String>,
    pub env_vars: Option<String>,
    pub setup_script: Option<String>,
    pub python_venv_path: Option<String>,
    pub timeout_seconds: Option<i32>,
    pub preview_path_template: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct RunCommandParams {
    pub name: Option<String>,
    pub working_directory: Option<String>,
    pub command_line: Option<String>,
    pub setup_script: Option<String>,
    pub python_venv_path: Option<String>,
    pub env_vars: Option<String>,
    pub timeout_seconds: Option<i32>,
    pub retry_on_failure: Option<bool>,
    pub max_retries: Option<i32>,
    pub retry_delay_seconds: Option<i32>,
    pub preview_path_template: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CommandApiRunParams {
    pub template_id: Option<i32>,
    pub template_code: Option<String>,
    pub name: Option<String>,
    pub working_directory: Option<String>,
    pub command_line: Option<String>,
    pub setup_script: Option<String>,
    pub python_venv_path: Option<String>,
    pub env_vars: Option<String>,
    pub timeout_seconds: Option<i32>,
    pub retry_on_failure: Option<bool>,
    pub max_retries: Option<i32>,
    pub retry_delay_seconds: Option<i32>,
    pub preview_path_template: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveCommandApiKeyParams {
    pub name: String,
    pub token: Option<String>,
    pub description: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CommandApiRunStatusRecord {
    pub run: CommandRunRecord,
    pub logs: Vec<CommandRunLogRecord>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveCommandWorkflowStepParams {
    pub id: Option<i32>,
    pub template_id: i32,
    pub name: String,
    pub sort_order: i32,
    pub args: Option<String>,
    pub env_vars: Option<String>,
    pub working_directory: Option<String>,
    pub timeout_seconds: Option<i32>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveCommandWorkflowParams {
    pub name: String,
    pub code: String,
    pub description: Option<String>,
    pub enabled: Option<bool>,
    pub steps: Vec<SaveCommandWorkflowStepParams>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct RunCommandWorkflowParams {
    pub name: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/admin/commands",
    tag = "admin-commands",
    security(("bearer_auth" = [])),
    params(CommandTemplateQueryParams),
    responses((status = 200, body = ApiResponse<PageResponse<CommandTemplateRecord>>))
)]
#[debug_handler]
pub async fn list(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<CommandTemplateQueryParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:command:list").await?;
    command_runner::mark_stale_runs_failed(&ctx.db).await?;
    let page = params.page();
    let page_size = params.page_size();
    let mut query = command_templates::Entity::find().order_by_desc(command_templates::Column::Id);

    if let Some(keyword) = params.keyword.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(
            Condition::any()
                .add(command_templates::Column::Name.contains(keyword))
                .add(command_templates::Column::Code.contains(keyword))
                .add(command_templates::Column::Command.contains(keyword)),
        );
    }
    if let Some(enabled) = params.enabled {
        query = query.filter(command_templates::Column::Enabled.eq(enabled));
    }

    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(CommandTemplateRecord::from)
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
    path = "/api/admin/commands/{id}",
    tag = "admin-commands",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<CommandTemplateRecord>))
)]
#[debug_handler]
pub async fn get(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:command:list").await?;
    let template = find_template(&ctx, id).await?;
    Ok(responses::ok(CommandTemplateRecord::from(template)))
}

#[utoipa::path(
    post,
    path = "/api/admin/commands",
    tag = "admin-commands",
    security(("bearer_auth" = [])),
    request_body = SaveCommandTemplateParams,
    responses((status = 200, body = ApiResponse<CommandTemplateRecord>))
)]
#[debug_handler]
pub async fn create(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SaveCommandTemplateParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:command:create").await?;
    validate_template(&params)?;
    ensure_template_code_unique(&ctx, &params.code, None).await?;
    let template = command_templates::ActiveModel {
        name: Set(params.name),
        code: Set(params.code.trim().to_string()),
        description: Set(normalize_optional(params.description)),
        working_directory: Set(params.working_directory),
        command: Set(params.command),
        default_args: Set(normalize_optional(params.default_args)),
        env_vars: Set(normalize_optional(params.env_vars)),
        setup_script: Set(normalize_optional(params.setup_script)),
        python_venv_path: Set(normalize_optional(params.python_venv_path)),
        timeout_seconds: Set(params.timeout_seconds),
        preview_path_template: Set(normalize_optional(params.preview_path_template)),
        enabled: Set(params.enabled.unwrap_or(true)),
        created_by: Set(Some(actor.id)),
        updated_by: Set(Some(actor.id)),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;
    Ok(responses::ok(CommandTemplateRecord::from(template)))
}

#[utoipa::path(
    put,
    path = "/api/admin/commands/{id}",
    tag = "admin-commands",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = SaveCommandTemplateParams,
    responses((status = 200, body = ApiResponse<CommandTemplateRecord>))
)]
#[debug_handler]
pub async fn update(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<SaveCommandTemplateParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:command:update").await?;
    validate_template(&params)?;
    ensure_template_code_unique(&ctx, &params.code, Some(id)).await?;
    let template = find_template(&ctx, id).await?;
    let mut active = template.into_active_model();
    active.name = Set(params.name);
    active.code = Set(params.code.trim().to_string());
    active.description = Set(normalize_optional(params.description));
    active.working_directory = Set(params.working_directory);
    active.command = Set(params.command);
    active.default_args = Set(normalize_optional(params.default_args));
    active.env_vars = Set(normalize_optional(params.env_vars));
    active.setup_script = Set(normalize_optional(params.setup_script));
    active.python_venv_path = Set(normalize_optional(params.python_venv_path));
    active.timeout_seconds = Set(params.timeout_seconds);
    active.preview_path_template = Set(normalize_optional(params.preview_path_template));
    active.enabled = Set(params.enabled.unwrap_or(true));
    active.updated_by = Set(Some(actor.id));
    let template = active.update(&ctx.db).await?;
    Ok(responses::ok(CommandTemplateRecord::from(template)))
}

#[utoipa::path(
    delete,
    path = "/api/admin/commands/{id}",
    tag = "admin-commands",
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
    authorize(&ctx, &auth, "system:command:delete").await?;
    find_template(&ctx, id).await?;
    command_templates::Entity::delete_by_id(id)
        .exec(&ctx.db)
        .await?;
    Ok(responses::empty())
}

#[utoipa::path(
    get,
    path = "/api/admin/api-keys",
    tag = "admin-api-keys",
    security(("bearer_auth" = [])),
    params(CommandApiKeyQueryParams),
    responses((status = 200, body = ApiResponse<PageResponse<CommandApiKeyRecord>>))
)]
#[debug_handler]
pub async fn list_api_keys(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<CommandApiKeyQueryParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:api_key:list").await?;
    let page = params.page();
    let page_size = params.page_size();
    let mut query = command_api_keys::Entity::find().order_by_desc(command_api_keys::Column::Id);
    if let Some(keyword) = params.keyword.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(
            Condition::any()
                .add(command_api_keys::Column::Name.contains(keyword))
                .add(command_api_keys::Column::Description.contains(keyword)),
        );
    }
    if let Some(enabled) = params.enabled {
        query = query.filter(command_api_keys::Column::Enabled.eq(enabled));
    }
    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(CommandApiKeyRecord::from)
        .collect();
    Ok(responses::ok(PageResponse {
        items,
        page,
        page_size,
        total,
    }))
}

#[utoipa::path(
    post,
    path = "/api/admin/api-keys",
    tag = "admin-api-keys",
    security(("bearer_auth" = [])),
    request_body = SaveCommandApiKeyParams,
    responses((status = 200, body = ApiResponse<CommandApiKeyRecord>))
)]
#[debug_handler]
pub async fn create_api_key(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SaveCommandApiKeyParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:api_key:create").await?;
    validate_api_key(&params)?;
    let token = normalize_optional(params.token).unwrap_or_else(generate_api_token);
    ensure_api_key_token_unique(&ctx, &token, None).await?;
    let key = command_api_keys::ActiveModel {
        name: Set(params.name.trim().to_string()),
        token: Set(token),
        description: Set(normalize_optional(params.description)),
        enabled: Set(params.enabled.unwrap_or(true)),
        created_by: Set(Some(actor.id)),
        updated_by: Set(Some(actor.id)),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;
    Ok(responses::ok(CommandApiKeyRecord::from(key)))
}

#[utoipa::path(
    put,
    path = "/api/admin/api-keys/{id}",
    tag = "admin-api-keys",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = SaveCommandApiKeyParams,
    responses((status = 200, body = ApiResponse<CommandApiKeyRecord>))
)]
#[debug_handler]
pub async fn update_api_key(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<SaveCommandApiKeyParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:api_key:update").await?;
    validate_api_key(&params)?;
    let token = normalize_optional(params.token).unwrap_or_else(generate_api_token);
    ensure_api_key_token_unique(&ctx, &token, Some(id)).await?;
    let key = find_api_key(&ctx, id).await?;
    let owner_id = key.created_by.or(Some(actor.id));
    let mut active = key.into_active_model();
    active.name = Set(params.name.trim().to_string());
    active.token = Set(token);
    active.description = Set(normalize_optional(params.description));
    active.enabled = Set(params.enabled.unwrap_or(true));
    active.created_by = Set(owner_id);
    active.updated_by = Set(Some(actor.id));
    let key = active.update(&ctx.db).await?;
    Ok(responses::ok(CommandApiKeyRecord::from(key)))
}

#[utoipa::path(
    delete,
    path = "/api/admin/api-keys/{id}",
    tag = "admin-api-keys",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn delete_api_key(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:api_key:delete").await?;
    find_api_key(&ctx, id).await?;
    command_api_keys::Entity::delete_by_id(id)
        .exec(&ctx.db)
        .await?;
    Ok(responses::empty())
}

#[utoipa::path(
    get,
    path = "/api/admin/command-workflows",
    tag = "admin-commands",
    security(("bearer_auth" = [])),
    params(CommandWorkflowQueryParams),
    responses((status = 200, body = ApiResponse<PageResponse<CommandWorkflowRecord>>))
)]
#[debug_handler]
pub async fn list_workflows(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<CommandWorkflowQueryParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:command:list").await?;
    command_runner::mark_stale_runs_failed(&ctx.db).await?;
    let page = params.page();
    let page_size = params.page_size();
    let mut query = command_workflows::Entity::find().order_by_desc(command_workflows::Column::Id);
    if let Some(keyword) = params.keyword.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(
            Condition::any()
                .add(command_workflows::Column::Name.contains(keyword))
                .add(command_workflows::Column::Code.contains(keyword)),
        );
    }
    if let Some(enabled) = params.enabled {
        query = query.filter(command_workflows::Column::Enabled.eq(enabled));
    }
    let total = query.clone().count(&ctx.db).await?;
    let workflows = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?;
    let mut items = Vec::with_capacity(workflows.len());
    for workflow in workflows {
        items.push(workflow_record(&ctx, workflow).await?);
    }
    Ok(responses::ok(PageResponse {
        items,
        page,
        page_size,
        total,
    }))
}

#[utoipa::path(
    get,
    path = "/api/admin/command-workflows/{id}",
    tag = "admin-commands",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<CommandWorkflowRecord>))
)]
#[debug_handler]
pub async fn get_workflow(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:command:list").await?;
    let workflow = find_workflow(&ctx, id).await?;
    Ok(responses::ok(workflow_record(&ctx, workflow).await?))
}

#[utoipa::path(
    post,
    path = "/api/admin/command-workflows",
    tag = "admin-commands",
    security(("bearer_auth" = [])),
    request_body = SaveCommandWorkflowParams,
    responses((status = 200, body = ApiResponse<CommandWorkflowRecord>))
)]
#[debug_handler]
pub async fn create_workflow(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SaveCommandWorkflowParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:command:create").await?;
    validate_workflow(&ctx, &params).await?;
    ensure_workflow_code_unique(&ctx, &params.code, None).await?;
    let workflow = command_workflows::ActiveModel {
        name: Set(params.name),
        code: Set(params.code.trim().to_string()),
        description: Set(normalize_optional(params.description)),
        enabled: Set(params.enabled.unwrap_or(true)),
        created_by: Set(Some(actor.id)),
        updated_by: Set(Some(actor.id)),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;
    save_workflow_steps(&ctx, workflow.id, params.steps).await?;
    Ok(responses::ok(workflow_record(&ctx, workflow).await?))
}

#[utoipa::path(
    put,
    path = "/api/admin/command-workflows/{id}",
    tag = "admin-commands",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = SaveCommandWorkflowParams,
    responses((status = 200, body = ApiResponse<CommandWorkflowRecord>))
)]
#[debug_handler]
pub async fn update_workflow(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<SaveCommandWorkflowParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:command:update").await?;
    validate_workflow(&ctx, &params).await?;
    ensure_workflow_code_unique(&ctx, &params.code, Some(id)).await?;
    let workflow = find_workflow(&ctx, id).await?;
    let mut active = workflow.into_active_model();
    active.name = Set(params.name);
    active.code = Set(params.code.trim().to_string());
    active.description = Set(normalize_optional(params.description));
    active.enabled = Set(params.enabled.unwrap_or(true));
    active.updated_by = Set(Some(actor.id));
    let workflow = active.update(&ctx.db).await?;
    command_workflow_steps::Entity::delete_many()
        .filter(command_workflow_steps::Column::WorkflowId.eq(id))
        .exec(&ctx.db)
        .await?;
    save_workflow_steps(&ctx, id, params.steps).await?;
    Ok(responses::ok(workflow_record(&ctx, workflow).await?))
}

#[utoipa::path(
    delete,
    path = "/api/admin/command-workflows/{id}",
    tag = "admin-commands",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn delete_workflow(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:command:delete").await?;
    find_workflow(&ctx, id).await?;
    command_workflows::Entity::delete_by_id(id)
        .exec(&ctx.db)
        .await?;
    Ok(responses::empty())
}

#[utoipa::path(
    post,
    path = "/api/admin/command-workflows/{id}/run",
    tag = "admin-commands",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = RunCommandWorkflowParams,
    responses((status = 200, body = ApiResponse<CommandWorkflowRunRecord>))
)]
#[debug_handler]
pub async fn run_workflow(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<RunCommandWorkflowParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:command:run").await?;
    let workflow = find_workflow(&ctx, id).await?;
    if !workflow.enabled {
        return Err(ApiError::bad_request("command workflow is disabled"));
    }
    let name = params.name.unwrap_or_else(|| workflow.name.clone());
    let run = command_workflow_runner::start_workflow_run(
        ctx.db.clone(),
        workflow.id,
        name,
        Some(actor.id),
    )
    .await?;
    Ok(responses::ok(workflow_run_record(&ctx, run).await?))
}

#[utoipa::path(
    get,
    path = "/api/admin/command-workflow-runs",
    tag = "admin-commands",
    security(("bearer_auth" = [])),
    params(CommandWorkflowRunQueryParams),
    responses((status = 200, body = ApiResponse<PageResponse<CommandWorkflowRunRecord>>))
)]
#[debug_handler]
pub async fn list_workflow_runs(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<CommandWorkflowRunQueryParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:command:list").await?;
    let page = params.page();
    let page_size = params.page_size();
    let mut query =
        command_workflow_runs::Entity::find().order_by_desc(command_workflow_runs::Column::Id);
    if let Some(workflow_id) = params.workflow_id {
        query = query.filter(command_workflow_runs::Column::WorkflowId.eq(workflow_id));
    }
    if let Some(status) = params.status.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(command_workflow_runs::Column::Status.eq(status));
    }
    if let Some(keyword) = params.keyword.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(command_workflow_runs::Column::Name.contains(keyword));
    }
    let total = query.clone().count(&ctx.db).await?;
    let runs = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?;
    let mut items = Vec::with_capacity(runs.len());
    for run in runs {
        items.push(workflow_run_record(&ctx, run).await?);
    }
    Ok(responses::ok(PageResponse {
        items,
        page,
        page_size,
        total,
    }))
}

#[utoipa::path(
    get,
    path = "/api/admin/command-workflow-runs/{id}",
    tag = "admin-commands",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<CommandWorkflowRunRecord>))
)]
#[debug_handler]
pub async fn get_workflow_run(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:command:list").await?;
    let run = find_workflow_run(&ctx, id).await?;
    Ok(responses::ok(workflow_run_record(&ctx, run).await?))
}

#[utoipa::path(
    post,
    path = "/api/admin/commands/{id}/run",
    tag = "admin-commands",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = RunCommandParams,
    responses((status = 200, body = ApiResponse<CommandRunRecord>))
)]
#[debug_handler]
pub async fn run_template(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<RunCommandParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:command:run").await?;
    let template = find_template(&ctx, id).await?;
    if !template.enabled {
        return Err(ApiError::bad_request("command template is disabled"));
    }
    let command_line = params
        .command_line
        .unwrap_or_else(|| build_template_command_line(&template));
    let request = CommandRunRequest {
        template_id: Some(template.id),
        name: params.name.unwrap_or(template.name),
        working_directory: params
            .working_directory
            .unwrap_or(template.working_directory),
        command_line,
        setup_script: params.setup_script.or(template.setup_script),
        python_venv_path: params.python_venv_path.or(template.python_venv_path),
        env_vars: params.env_vars.or(template.env_vars),
        timeout_seconds: params.timeout_seconds.or(template.timeout_seconds),
        preview_path_template: params
            .preview_path_template
            .or(template.preview_path_template),
        retry_policy: command_runner::RetryPolicy::from_options(
            params.retry_on_failure,
            params.max_retries,
            params.retry_delay_seconds,
        )?,
        triggered_by: "manual".to_string(),
        created_by: Some(actor.id),
    };
    let run = command_runner::start_command_run(ctx.db.clone(), request).await?;
    Ok(responses::ok(CommandRunRecord::from(run)))
}

#[utoipa::path(
    post,
    path = "/api/admin/command-runs",
    tag = "admin-commands",
    security(("bearer_auth" = [])),
    request_body = RunCommandParams,
    responses((status = 200, body = ApiResponse<CommandRunRecord>))
)]
#[debug_handler]
pub async fn run_ad_hoc(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<RunCommandParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:command:run").await?;
    let command_line = params
        .command_line
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::bad_request("command line is required"))?;
    let working_directory = params
        .working_directory
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::bad_request("working directory is required"))?;
    let request = CommandRunRequest {
        template_id: None,
        name: params.name.unwrap_or_else(|| "临时命令".to_string()),
        working_directory,
        command_line,
        setup_script: params.setup_script,
        python_venv_path: params.python_venv_path,
        env_vars: params.env_vars,
        timeout_seconds: params.timeout_seconds,
        preview_path_template: params.preview_path_template,
        retry_policy: command_runner::RetryPolicy::from_options(
            params.retry_on_failure,
            params.max_retries,
            params.retry_delay_seconds,
        )?,
        triggered_by: "manual".to_string(),
        created_by: Some(actor.id),
    };
    let run = command_runner::start_command_run(ctx.db.clone(), request).await?;
    Ok(responses::ok(CommandRunRecord::from(run)))
}

#[utoipa::path(
    post,
    path = "/api/command-api/runs",
    tag = "command-api",
    security(("bearer_auth" = [])),
    request_body = CommandApiRunParams,
    responses((status = 200, body = ApiResponse<CommandRunRecord>))
)]
#[debug_handler]
pub async fn api_run_command(
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    Json(params): Json<CommandApiRunParams>,
) -> ApiResult<Response> {
    let key = authorize_command_api_key(&ctx, &headers).await?;
    let request = build_api_command_request(&ctx, params, &key).await?;
    let run = command_runner::start_command_run(ctx.db.clone(), request).await?;
    Ok(responses::ok(CommandRunRecord::from(run)))
}

#[utoipa::path(
    get,
    path = "/api/command-api/runs/{id}",
    tag = "command-api",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path), CommandRunLogQueryParams),
    responses((status = 200, body = ApiResponse<CommandApiRunStatusRecord>))
)]
#[debug_handler]
pub async fn api_get_run_status(
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    Path(id): Path<i32>,
    Query(params): Query<CommandRunLogQueryParams>,
) -> ApiResult<Response> {
    authorize_command_api_key(&ctx, &headers).await?;
    let run = find_run(&ctx, id).await?;
    let logs = run_logs(&ctx, id, &params).await?;
    Ok(responses::ok(CommandApiRunStatusRecord {
        run: CommandRunRecord::from(run),
        logs,
    }))
}

#[utoipa::path(
    get,
    path = "/api/admin/command-runs",
    tag = "admin-commands",
    security(("bearer_auth" = [])),
    params(CommandRunQueryParams),
    responses((status = 200, body = ApiResponse<PageResponse<CommandRunRecord>>))
)]
#[debug_handler]
pub async fn list_runs(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<CommandRunQueryParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:command:list").await?;
    command_runner::mark_stale_runs_failed(&ctx.db).await?;
    let page = params.page();
    let page_size = params.page_size();
    let mut query = command_runs::Entity::find().order_by_desc(command_runs::Column::Id);
    if let Some(template_id) = params.template_id {
        query = query.filter(command_runs::Column::TemplateId.eq(template_id));
    }
    if let Some(status) = params.status.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(command_runs::Column::Status.eq(status));
    }
    if let Some(keyword) = params.keyword.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(
            Condition::any()
                .add(command_runs::Column::Name.contains(keyword))
                .add(command_runs::Column::CommandLine.contains(keyword))
                .add(command_runs::Column::WorkingDirectory.contains(keyword)),
        );
    }
    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(CommandRunRecord::from)
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
    path = "/api/admin/command-runs/{id}",
    tag = "admin-commands",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<CommandRunRecord>))
)]
#[debug_handler]
pub async fn get_run(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:command:list").await?;
    let run = find_run(&ctx, id).await?;
    Ok(responses::ok(CommandRunRecord::from(run)))
}

#[utoipa::path(
    get,
    path = "/api/admin/command-runs/{id}/logs",
    tag = "admin-commands",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path), CommandRunLogQueryParams),
    responses((status = 200, body = ApiResponse<Vec<CommandRunLogRecord>>))
)]
#[debug_handler]
pub async fn list_run_logs(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Query(params): Query<CommandRunLogQueryParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:command:list").await?;
    find_run(&ctx, id).await?;
    Ok(responses::ok(run_logs(&ctx, id, &params).await?))
}

#[utoipa::path(
    get,
    path = "/api/admin/command-runs/{id}/preview",
    tag = "admin-commands",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, description = "Preview command run artifact inline"))
)]
#[debug_handler]
pub async fn preview_run_artifact(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:command:list").await?;
    let run = find_run(&ctx, id).await?;
    let preview_path = run
        .preview_path
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::bad_request("command run has no preview artifact"))?;
    let path = FsPath::new(preview_path);
    let metadata =
        fs::metadata(path).map_err(|_| ApiError::bad_request("preview file not found"))?;
    if !metadata.is_file() {
        return Err(ApiError::bad_request("preview path is not a file"));
    }
    let bytes = fs::read(path).map_err(|_| ApiError::bad_request("preview file not found"))?;
    let filename = path.file_name().map_or_else(
        || format!("command-run-{id}"),
        |value| value.to_string_lossy().to_string(),
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        mime_guess::from_path(path)
            .first_raw()
            .unwrap_or("application/octet-stream")
            .parse()
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "inline; filename=\"{}\"",
            sanitize_filename(&filename)
        ))
        .map_err(|_| ApiError::internal("failed to build preview response"))?,
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    Ok((headers, bytes).into_response())
}

#[utoipa::path(
    post,
    path = "/api/admin/command-runs/{id}/cancel",
    tag = "admin-commands",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<CommandRunRecord>))
)]
#[debug_handler]
pub async fn cancel_run(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:command:cancel").await?;
    let run = command_runner::cancel_run(&ctx.db, id).await?;
    Ok(responses::ok(CommandRunRecord::from(run)))
}

#[utoipa::path(
    post,
    path = "/api/admin/command-runs/{id}/log-ticket",
    tag = "admin-commands",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<CommandRunLogTicketRecord>))
)]
#[debug_handler]
pub async fn create_log_ticket(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:command:list").await?;
    find_run(&ctx, id).await?;
    let ticket = Uuid::new_v4().to_string();
    let expires_at = Utc::now() + Duration::seconds(COMMAND_LOG_TICKET_TTL_SECONDS);
    log_ticket_store()
        .lock()
        .map_err(|_| ApiError::internal("failed to lock command log ticket store"))?
        .insert(
            ticket.clone(),
            CommandLogTicket {
                run_id: id,
                expires_at,
            },
        );
    Ok(responses::ok(CommandRunLogTicketRecord {
        ticket,
        expires_at: expires_at.to_rfc3339(),
    }))
}

#[debug_handler]
pub async fn run_ws(
    State(_ctx): State<AppContext>,
    Path(ticket): Path<String>,
    ws: WebSocketUpgrade,
) -> ApiResult<Response> {
    let run_id = consume_log_ticket(&ticket)?;
    Ok(ws
        .on_upgrade(move |socket| stream_run_logs(socket, run_id))
        .into_response())
}

async fn stream_run_logs(mut socket: WebSocket, run_id: i32) {
    let mut rx = command_runner::subscribe_run(run_id);
    loop {
        match rx.recv().await {
            Ok(event) => {
                let Ok(payload) = serde_json::to_string(&event) else {
                    break;
                };
                if socket.send(Message::Text(payload.into())).await.is_err() {
                    break;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}

fn log_ticket_store() -> &'static Mutex<HashMap<String, CommandLogTicket>> {
    COMMAND_LOG_TICKETS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn consume_log_ticket(ticket: &str) -> ApiResult<i32> {
    let now = Utc::now();
    let mut store = log_ticket_store()
        .lock()
        .map_err(|_| ApiError::internal("failed to lock command log ticket store"))?;
    store.retain(|_, ticket| ticket.expires_at > now);
    let ticket = store
        .remove(ticket)
        .ok_or_else(|| ApiError::unauthorized("invalid or expired command log ticket"))?;
    drop(store);
    if ticket.expires_at <= now {
        return Err(ApiError::unauthorized(
            "invalid or expired command log ticket",
        ));
    }
    Ok(ticket.run_id)
}

async fn authorize_command_api_key(
    ctx: &AppContext,
    headers: &HeaderMap,
) -> ApiResult<command_api_keys::Model> {
    let token = bearer_token(headers)?;
    let key = command_api_keys::Entity::find()
        .filter(command_api_keys::Column::Token.eq(token))
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::unauthorized("invalid command api key"))?;
    if !key.enabled {
        return Err(ApiError::unauthorized("command api key is disabled"));
    }
    let owner_id = key
        .created_by
        .ok_or_else(|| ApiError::unauthorized("API key is not bound to a user"))?;
    let owner = users::Entity::find_by_id(owner_id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::unauthorized("API key owner no longer exists"))?;
    rbac::assert_permission(&ctx.db, owner.id, "system:command:run").await?;
    let mut active = key.clone().into_active_model();
    active.last_used_at = Set(Some(Utc::now().fixed_offset()));
    let key = active.update(&ctx.db).await?;
    Ok(key)
}

fn bearer_token(headers: &HeaderMap) -> ApiResult<String> {
    let value = headers
        .get(header::AUTHORIZATION)
        .ok_or_else(|| ApiError::unauthorized("missing bearer token"))?;
    let value = value
        .to_str()
        .map_err(|_| ApiError::unauthorized("invalid bearer token"))?
        .trim();
    let token = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
        .ok_or_else(|| ApiError::unauthorized("missing bearer token"))?
        .trim();
    if token.is_empty() {
        return Err(ApiError::unauthorized("missing bearer token"));
    }
    Ok(token.to_string())
}

async fn build_api_command_request(
    ctx: &AppContext,
    params: CommandApiRunParams,
    key: &command_api_keys::Model,
) -> ApiResult<CommandRunRequest> {
    let triggered_by = format!("api:{}", key.name);
    if params.template_id.is_some()
        || params
            .template_code
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
    {
        let template = find_template_for_api(ctx, params.template_id, params.template_code).await?;
        if !template.enabled {
            return Err(ApiError::bad_request("command template is disabled"));
        }
        let command_line = params
            .command_line
            .unwrap_or_else(|| build_template_command_line(&template));
        return Ok(CommandRunRequest {
            template_id: Some(template.id),
            name: params.name.unwrap_or(template.name),
            working_directory: params
                .working_directory
                .unwrap_or(template.working_directory),
            command_line,
            setup_script: params.setup_script.or(template.setup_script),
            python_venv_path: params.python_venv_path.or(template.python_venv_path),
            env_vars: params.env_vars.or(template.env_vars),
            timeout_seconds: params.timeout_seconds.or(template.timeout_seconds),
            preview_path_template: params
                .preview_path_template
                .or(template.preview_path_template),
            retry_policy: command_runner::RetryPolicy::from_options(
                params.retry_on_failure,
                params.max_retries,
                params.retry_delay_seconds,
            )?,
            triggered_by,
            created_by: key.created_by,
        });
    }

    let command_line = params
        .command_line
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::bad_request("command line is required"))?;
    let working_directory = params
        .working_directory
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::bad_request("working directory is required"))?;
    Ok(CommandRunRequest {
        template_id: None,
        name: params.name.unwrap_or_else(|| "API 命令".to_string()),
        working_directory,
        command_line,
        setup_script: params.setup_script,
        python_venv_path: params.python_venv_path,
        env_vars: params.env_vars,
        timeout_seconds: params.timeout_seconds,
        preview_path_template: params.preview_path_template,
        retry_policy: command_runner::RetryPolicy::from_options(
            params.retry_on_failure,
            params.max_retries,
            params.retry_delay_seconds,
        )?,
        triggered_by,
        created_by: key.created_by,
    })
}

async fn find_template_for_api(
    ctx: &AppContext,
    template_id: Option<i32>,
    template_code: Option<String>,
) -> ApiResult<command_templates::Model> {
    if let Some(id) = template_id {
        return find_template(ctx, id).await;
    }
    let code = template_code
        .and_then(|value| normalize_optional(Some(value)))
        .ok_or_else(|| ApiError::bad_request("template id or code is required"))?;
    command_templates::Entity::find()
        .filter(command_templates::Column::Code.eq(code))
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("command template not found"))
}

async fn run_logs(
    ctx: &AppContext,
    run_id: i32,
    params: &CommandRunLogQueryParams,
) -> ApiResult<Vec<CommandRunLogRecord>> {
    let mut query = command_run_logs::Entity::find()
        .filter(command_run_logs::Column::RunId.eq(run_id))
        .order_by_asc(command_run_logs::Column::Seq);
    if let Some(after_seq) = params.after_seq {
        query = query.filter(command_run_logs::Column::Seq.gt(after_seq));
    }
    Ok(query
        .limit(params.limit.unwrap_or(500).clamp(1, 2000))
        .all(&ctx.db)
        .await?
        .into_iter()
        .map(CommandRunLogRecord::from)
        .collect::<Vec<_>>())
}

async fn find_template(ctx: &AppContext, id: i32) -> ApiResult<command_templates::Model> {
    command_templates::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("command template not found"))
}

async fn find_api_key(ctx: &AppContext, id: i32) -> ApiResult<command_api_keys::Model> {
    command_api_keys::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("command api key not found"))
}

async fn find_run(ctx: &AppContext, id: i32) -> ApiResult<command_runs::Model> {
    command_runs::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("command run not found"))
}

async fn find_workflow(ctx: &AppContext, id: i32) -> ApiResult<command_workflows::Model> {
    command_workflows::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("command workflow not found"))
}

async fn find_workflow_run(ctx: &AppContext, id: i32) -> ApiResult<command_workflow_runs::Model> {
    command_workflow_runs::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("command workflow run not found"))
}

async fn workflow_record(
    ctx: &AppContext,
    workflow: command_workflows::Model,
) -> ApiResult<CommandWorkflowRecord> {
    let steps = command_workflow_steps::Entity::find()
        .filter(command_workflow_steps::Column::WorkflowId.eq(workflow.id))
        .order_by_asc(command_workflow_steps::Column::SortOrder)
        .all(&ctx.db)
        .await?
        .into_iter()
        .map(CommandWorkflowStepRecord::from)
        .collect();
    Ok(CommandWorkflowRecord {
        id: workflow.id,
        name: workflow.name,
        code: workflow.code,
        description: workflow.description,
        enabled: workflow.enabled,
        created_by: workflow.created_by,
        updated_by: workflow.updated_by,
        steps,
        created_at: workflow.created_at.to_rfc3339(),
        updated_at: workflow.updated_at.to_rfc3339(),
    })
}

async fn workflow_run_record(
    ctx: &AppContext,
    run: command_workflow_runs::Model,
) -> ApiResult<CommandWorkflowRunRecord> {
    let steps = command_workflow_run_steps::Entity::find()
        .filter(command_workflow_run_steps::Column::WorkflowRunId.eq(run.id))
        .order_by_asc(command_workflow_run_steps::Column::SortOrder)
        .all(&ctx.db)
        .await?
        .into_iter()
        .map(CommandWorkflowRunStepRecord::from)
        .collect();
    Ok(CommandWorkflowRunRecord {
        id: run.id,
        workflow_id: run.workflow_id,
        name: run.name,
        status: run.status,
        started_at: run.started_at.map(|value| value.to_rfc3339()),
        finished_at: run.finished_at.map(|value| value.to_rfc3339()),
        duration_ms: run.duration_ms,
        created_by: run.created_by,
        error_message: run.error_message,
        steps,
        created_at: run.created_at.to_rfc3339(),
        updated_at: run.updated_at.to_rfc3339(),
    })
}

async fn save_workflow_steps(
    ctx: &AppContext,
    workflow_id: i32,
    steps: Vec<SaveCommandWorkflowStepParams>,
) -> ApiResult<()> {
    for (index, step) in steps.into_iter().enumerate() {
        command_workflow_steps::ActiveModel {
            workflow_id: Set(workflow_id),
            template_id: Set(step.template_id),
            name: Set(step.name),
            sort_order: Set(if step.sort_order > 0 {
                step.sort_order
            } else {
                i32::try_from(index + 1).unwrap_or(i32::MAX)
            }),
            args: Set(normalize_optional(step.args)),
            env_vars: Set(normalize_optional(step.env_vars)),
            working_directory: Set(normalize_optional(step.working_directory)),
            timeout_seconds: Set(step.timeout_seconds),
            enabled: Set(step.enabled.unwrap_or(true)),
            ..Default::default()
        }
        .insert(&ctx.db)
        .await?;
    }
    Ok(())
}

fn validate_template(params: &SaveCommandTemplateParams) -> ApiResult<()> {
    if params.name.trim().is_empty()
        || params.code.trim().is_empty()
        || params.working_directory.trim().is_empty()
        || params.command.trim().is_empty()
    {
        return Err(ApiError::bad_request(
            "name, code, working directory and command are required",
        ));
    }
    validate_json_object(params.env_vars.as_deref(), "env_vars")?;
    if params.timeout_seconds.is_some_and(|value| value <= 0) {
        return Err(ApiError::bad_request("timeout_seconds must be positive"));
    }
    Ok(())
}

fn validate_api_key(params: &SaveCommandApiKeyParams) -> ApiResult<()> {
    if params.name.trim().is_empty() {
        return Err(ApiError::bad_request("name is required"));
    }
    if params
        .token
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(ApiError::bad_request("token must not be empty"));
    }
    if let Some(token) = params.token.as_deref().map(str::trim) {
        if token.chars().any(char::is_whitespace) {
            return Err(ApiError::bad_request("token must not contain whitespace"));
        }
        if token_looks_like_jwt(token) {
            return Err(ApiError::bad_request(
                "token must not use the three-segment JWT format",
            ));
        }
    }
    Ok(())
}

fn token_looks_like_jwt(token: &str) -> bool {
    let mut segments = token.split('.');
    matches!(
        (segments.next(), segments.next(), segments.next(), segments.next()),
        (Some(header), Some(payload), Some(signature), None)
            if !header.is_empty() && !payload.is_empty() && !signature.is_empty()
    )
}

async fn ensure_api_key_token_unique(
    ctx: &AppContext,
    token: &str,
    ignore_id: Option<i32>,
) -> ApiResult<()> {
    let existing = command_api_keys::Entity::find()
        .filter(command_api_keys::Column::Token.eq(token))
        .one(&ctx.db)
        .await?;
    if existing.is_some_and(|key| Some(key.id) != ignore_id) {
        return Err(ApiError::bad_request("授权密钥已存在"));
    }
    Ok(())
}

fn generate_api_token() -> String {
    format!("eyJ0_{}", Uuid::new_v4().simple())
}

async fn ensure_template_code_unique(
    ctx: &AppContext,
    code: &str,
    ignore_id: Option<i32>,
) -> ApiResult<()> {
    let existing = command_templates::Entity::find()
        .filter(command_templates::Column::Code.eq(code.trim()))
        .one(&ctx.db)
        .await?;
    if existing.is_some_and(|template| Some(template.id) != ignore_id) {
        return Err(ApiError::bad_request("命令模板编码已存在"));
    }
    Ok(())
}

async fn ensure_workflow_code_unique(
    ctx: &AppContext,
    code: &str,
    ignore_id: Option<i32>,
) -> ApiResult<()> {
    let existing = command_workflows::Entity::find()
        .filter(command_workflows::Column::Code.eq(code.trim()))
        .one(&ctx.db)
        .await?;
    if existing.is_some_and(|workflow| Some(workflow.id) != ignore_id) {
        return Err(ApiError::bad_request("任务编排编码已存在"));
    }
    Ok(())
}

async fn validate_workflow(ctx: &AppContext, params: &SaveCommandWorkflowParams) -> ApiResult<()> {
    if params.name.trim().is_empty() || params.code.trim().is_empty() {
        return Err(ApiError::bad_request("name and code are required"));
    }
    let enabled_steps = params
        .steps
        .iter()
        .filter(|step| step.enabled.unwrap_or(true))
        .collect::<Vec<_>>();
    if enabled_steps.is_empty() {
        return Err(ApiError::bad_request("workflow must have enabled steps"));
    }
    for step in enabled_steps {
        if step.name.trim().is_empty() {
            return Err(ApiError::bad_request("step name is required"));
        }
        if step.sort_order <= 0 {
            return Err(ApiError::bad_request("step sort order must be positive"));
        }
        if step.timeout_seconds.is_some_and(|value| value <= 0) {
            return Err(ApiError::bad_request(
                "step timeout_seconds must be positive",
            ));
        }
        validate_json_object(step.env_vars.as_deref(), "env_vars")?;
        find_template(ctx, step.template_id).await?;
    }
    Ok(())
}

fn validate_json_object(value: Option<&str>, field: &str) -> ApiResult<()> {
    if let Some(raw) = value.filter(|value| !value.trim().is_empty()) {
        let parsed = serde_json::from_str::<serde_json::Value>(raw)
            .map_err(|_| ApiError::bad_request(format!("{field} must be JSON")))?;
        if !parsed.is_object() {
            return Err(ApiError::bad_request(format!(
                "{field} must be a JSON object"
            )));
        }
    }
    Ok(())
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim().to_string();
        (!trimmed.is_empty()).then_some(trimmed)
    })
}

fn sanitize_filename(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|char| match char {
            '/' | '\\' | '\r' | '\n' | '\t' | '"' => '_',
            _ => char,
        })
        .collect::<String>();
    if sanitized.trim().is_empty() {
        "download".to_string()
    } else {
        sanitized
    }
}

fn build_template_command_line(template: &command_templates::Model) -> String {
    let Some(default_args) = template
        .default_args
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    else {
        return template.command.clone();
    };
    format!("{} {}", template.command, default_args)
}

impl From<command_templates::Model> for CommandTemplateRecord {
    fn from(template: command_templates::Model) -> Self {
        Self {
            id: template.id,
            name: template.name,
            code: template.code,
            description: template.description,
            working_directory: template.working_directory,
            command: template.command,
            default_args: template.default_args,
            env_vars: template.env_vars,
            setup_script: template.setup_script,
            python_venv_path: template.python_venv_path,
            timeout_seconds: template.timeout_seconds,
            preview_path_template: template.preview_path_template,
            enabled: template.enabled,
            created_by: template.created_by,
            updated_by: template.updated_by,
            created_at: template.created_at.to_rfc3339(),
            updated_at: template.updated_at.to_rfc3339(),
        }
    }
}

impl From<command_api_keys::Model> for CommandApiKeyRecord {
    fn from(key: command_api_keys::Model) -> Self {
        Self {
            id: key.id,
            name: key.name,
            token: key.token,
            description: key.description,
            enabled: key.enabled,
            last_used_at: key.last_used_at.map(|value| value.to_rfc3339()),
            created_by: key.created_by,
            updated_by: key.updated_by,
            created_at: key.created_at.to_rfc3339(),
            updated_at: key.updated_at.to_rfc3339(),
        }
    }
}

impl From<command_runs::Model> for CommandRunRecord {
    fn from(run: command_runs::Model) -> Self {
        Self {
            id: run.id,
            template_id: run.template_id,
            name: run.name,
            working_directory: run.working_directory,
            command_line: run.command_line,
            effective_script: run.effective_script,
            status: run.status,
            retry_on_failure: run.retry_on_failure,
            max_retries: run.max_retries,
            retry_delay_seconds: run.retry_delay_seconds,
            attempt_count: run.attempt_count,
            exit_code: run.exit_code,
            started_at: run.started_at.map(|value| value.to_rfc3339()),
            finished_at: run.finished_at.map(|value| value.to_rfc3339()),
            duration_ms: run.duration_ms,
            triggered_by: run.triggered_by,
            created_by: run.created_by,
            error_message: run.error_message,
            output_tail: run.output_tail,
            preview_path_template: run.preview_path_template,
            preview_path: run.preview_path,
            created_at: run.created_at.to_rfc3339(),
            updated_at: run.updated_at.to_rfc3339(),
        }
    }
}

impl From<command_run_logs::Model> for CommandRunLogRecord {
    fn from(log: command_run_logs::Model) -> Self {
        Self {
            id: log.id,
            run_id: log.run_id,
            seq: log.seq,
            stream: log.stream,
            chunk: log.chunk,
            created_at: log.created_at.to_rfc3339(),
        }
    }
}

impl From<command_workflow_steps::Model> for CommandWorkflowStepRecord {
    fn from(step: command_workflow_steps::Model) -> Self {
        Self {
            id: step.id,
            workflow_id: step.workflow_id,
            template_id: step.template_id,
            name: step.name,
            sort_order: step.sort_order,
            args: step.args,
            env_vars: step.env_vars,
            working_directory: step.working_directory,
            timeout_seconds: step.timeout_seconds,
            enabled: step.enabled,
            created_at: step.created_at.to_rfc3339(),
            updated_at: step.updated_at.to_rfc3339(),
        }
    }
}

impl From<command_workflow_run_steps::Model> for CommandWorkflowRunStepRecord {
    fn from(step: command_workflow_run_steps::Model) -> Self {
        Self {
            id: step.id,
            workflow_run_id: step.workflow_run_id,
            workflow_step_id: step.workflow_step_id,
            command_run_id: step.command_run_id,
            step_name: step.step_name,
            sort_order: step.sort_order,
            status: step.status,
            resolved_args: step.resolved_args,
            started_at: step.started_at.map(|value| value.to_rfc3339()),
            finished_at: step.finished_at.map(|value| value.to_rfc3339()),
            error_message: step.error_message,
            created_at: step.created_at.to_rfc3339(),
            updated_at: step.updated_at.to_rfc3339(),
        }
    }
}
