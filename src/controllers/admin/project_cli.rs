#![allow(clippy::missing_errors_doc)]

use axum::extract::{Path, Query};
use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    controllers::admin::authorize,
    errors::ApiResult,
    responses::{self, ApiResponse, PageResponse},
    services::project_cli::{
        self, ProjectCliCatalog, ProjectCliRunDetail, ProjectCliRunSummary, RunProjectCliParams,
    },
};

#[derive(Debug, Default, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct ProjectCliCatalogQuery {
    /// Re-read every command's help instead of using the cached catalog.
    pub refresh: Option<bool>,
}

#[derive(Debug, Default, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct ProjectCliRunQuery {
    /// Only return output chunks after this sequence number.
    pub after: Option<i32>,
}

#[derive(Debug, Default, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct ProjectCliRunListQuery {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
}

#[utoipa::path(
    get,
    path = "/api/admin/project-cli",
    tag = "admin-project-cli",
    security(("bearer_auth" = [])),
    params(ProjectCliCatalogQuery),
    responses((status = 200, body = ApiResponse<ProjectCliCatalog>))
)]
#[debug_handler]
pub async fn catalog(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(query): Query<ProjectCliCatalogQuery>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:project_cli:view").await?;
    let environment = ctx.environment.to_string();
    Ok(responses::ok(
        project_cli::catalog(&environment, query.refresh.unwrap_or(false)).await,
    ))
}

#[utoipa::path(
    get,
    path = "/api/admin/project-cli/runs",
    tag = "admin-project-cli",
    security(("bearer_auth" = [])),
    params(ProjectCliRunListQuery),
    responses((status = 200, body = ApiResponse<PageResponse<ProjectCliRunSummary>>))
)]
#[debug_handler]
pub async fn list_runs(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(query): Query<ProjectCliRunListQuery>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:project_cli:view").await?;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    Ok(responses::ok(
        project_cli::list_runs(&ctx.db, page, page_size).await?,
    ))
}

#[utoipa::path(
    post,
    path = "/api/admin/project-cli/runs",
    tag = "admin-project-cli",
    security(("bearer_auth" = [])),
    request_body = RunProjectCliParams,
    responses((status = 200, body = ApiResponse<ProjectCliRunSummary>))
)]
#[debug_handler]
pub async fn run(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<RunProjectCliParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:project_cli:run").await?;
    let environment = ctx.environment.to_string();
    Ok(responses::ok(
        project_cli::start_run(&ctx.db, &environment, &user, &params).await?,
    ))
}

#[utoipa::path(
    get,
    path = "/api/admin/project-cli/runs/{id}",
    tag = "admin-project-cli",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path, description = "Run id"), ProjectCliRunQuery),
    responses((status = 200, body = ApiResponse<ProjectCliRunDetail>))
)]
#[debug_handler]
pub async fn get_run(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Query(query): Query<ProjectCliRunQuery>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:project_cli:view").await?;
    Ok(responses::ok(
        project_cli::run_detail(&ctx.db, id, query.after.unwrap_or(0)).await?,
    ))
}

#[utoipa::path(
    post,
    path = "/api/admin/project-cli/runs/{id}/cancel",
    tag = "admin-project-cli",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path, description = "Run id")),
    responses((status = 200, body = ApiResponse<ProjectCliRunSummary>))
)]
#[debug_handler]
pub async fn cancel_run(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:project_cli:run").await?;
    Ok(responses::ok(project_cli::cancel_run(&ctx.db, id).await?))
}
