#![allow(clippy::missing_errors_doc)]

use loco_rs::prelude::*;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    Set,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    controllers::admin::authorize,
    errors::{ApiError, ApiResult},
    mailers::auth::AuthMailer,
    models::email_templates::{self, RenderedEmailTemplate},
    responses::{self, ApiResponse, EmptyData, PageResponse},
};

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct EmailTemplateQueryParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub keyword: Option<String>,
    pub template_type: Option<String>,
    pub enabled: Option<bool>,
}

impl EmailTemplateQueryParams {
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
pub struct EmailTemplateRecord {
    pub id: i32,
    pub code: String,
    pub name: String,
    pub template_type: String,
    pub subject: String,
    pub html_body: String,
    pub text_body: String,
    pub variables: String,
    pub enabled: bool,
    pub is_builtin: bool,
    pub description: Option<String>,
    pub created_by: Option<i32>,
    pub updated_by: Option<i32>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveEmailTemplateParams {
    pub code: String,
    pub name: String,
    pub template_type: String,
    pub subject: String,
    pub html_body: String,
    pub text_body: String,
    pub variables: String,
    pub enabled: Option<bool>,
    pub is_builtin: Option<bool>,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct PreviewEmailTemplateParams {
    pub locals: Value,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct TestSendEmailTemplateParams {
    pub to: String,
    pub locals: Value,
}

#[utoipa::path(
    get,
    path = "/api/admin/email-templates",
    tag = "admin-email-templates",
    security(("bearer_auth" = [])),
    params(EmailTemplateQueryParams),
    responses((status = 200, body = ApiResponse<PageResponse<EmailTemplateRecord>>))
)]
#[debug_handler]
pub async fn list(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<EmailTemplateQueryParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:email_template:list").await?;

    let page = params.page();
    let page_size = params.page_size();
    let mut query = email_templates::Entity::find()
        .order_by_asc(email_templates::Column::TemplateType)
        .order_by_asc(email_templates::Column::Id);

    if let Some(keyword) = params.keyword.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(
            Condition::any()
                .add(email_templates::Column::Code.contains(keyword))
                .add(email_templates::Column::Name.contains(keyword))
                .add(email_templates::Column::Description.contains(keyword)),
        );
    }
    if let Some(template_type) = params
        .template_type
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        query = query.filter(email_templates::Column::TemplateType.eq(template_type));
    }
    if let Some(enabled) = params.enabled {
        query = query.filter(email_templates::Column::Enabled.eq(enabled));
    }

    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(EmailTemplateRecord::from)
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
    path = "/api/admin/email-templates/{id}",
    tag = "admin-email-templates",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<EmailTemplateRecord>))
)]
#[debug_handler]
pub async fn get(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:email_template:list").await?;
    let template = find_template(&ctx, id).await?;
    Ok(responses::ok(EmailTemplateRecord::from(template)))
}

#[utoipa::path(
    post,
    path = "/api/admin/email-templates",
    tag = "admin-email-templates",
    security(("bearer_auth" = [])),
    request_body = SaveEmailTemplateParams,
    responses((status = 200, body = ApiResponse<EmailTemplateRecord>))
)]
#[debug_handler]
pub async fn create(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SaveEmailTemplateParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:email_template:create").await?;
    validate_template(&params)?;

    let template = email_templates::ActiveModel {
        code: Set(params.code),
        name: Set(params.name),
        template_type: Set(params.template_type),
        subject: Set(params.subject),
        html_body: Set(params.html_body),
        text_body: Set(params.text_body),
        variables: Set(params.variables),
        enabled: Set(params.enabled.unwrap_or(true)),
        is_builtin: Set(params.is_builtin.unwrap_or(false)),
        description: Set(params.description),
        created_by: Set(Some(user.id)),
        updated_by: Set(Some(user.id)),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;

    Ok(responses::ok(EmailTemplateRecord::from(template)))
}

#[utoipa::path(
    put,
    path = "/api/admin/email-templates/{id}",
    tag = "admin-email-templates",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = SaveEmailTemplateParams,
    responses((status = 200, body = ApiResponse<EmailTemplateRecord>))
)]
#[debug_handler]
pub async fn update(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<SaveEmailTemplateParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:email_template:update").await?;
    validate_template(&params)?;
    let template = find_template(&ctx, id).await?;

    let mut active = template.into_active_model();
    active.code = Set(params.code);
    active.name = Set(params.name);
    active.template_type = Set(params.template_type);
    active.subject = Set(params.subject);
    active.html_body = Set(params.html_body);
    active.text_body = Set(params.text_body);
    active.variables = Set(params.variables);
    active.enabled = Set(params.enabled.unwrap_or(true));
    active.is_builtin = Set(params.is_builtin.unwrap_or(false));
    active.description = Set(params.description);
    active.updated_by = Set(Some(user.id));
    let template = active.update(&ctx.db).await?;

    Ok(responses::ok(EmailTemplateRecord::from(template)))
}

#[utoipa::path(
    delete,
    path = "/api/admin/email-templates/{id}",
    tag = "admin-email-templates",
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
    authorize(&ctx, &auth, "system:email_template:delete").await?;
    let template = find_template(&ctx, id).await?;
    if template.is_builtin {
        return Err(ApiError::bad_request(
            "builtin email template cannot be deleted",
        ));
    }

    email_templates::Entity::delete_by_id(id)
        .exec(&ctx.db)
        .await?;
    Ok(responses::empty())
}

#[utoipa::path(
    post,
    path = "/api/admin/email-templates/{id}/preview",
    tag = "admin-email-templates",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = PreviewEmailTemplateParams,
    responses((status = 200, body = ApiResponse<RenderedEmailTemplate>))
)]
#[debug_handler]
pub async fn preview(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<PreviewEmailTemplateParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:email_template:list").await?;
    let template = find_template(&ctx, id).await?;
    Ok(responses::ok(email_templates::render_model(
        &template,
        &params.locals,
    )))
}

#[utoipa::path(
    post,
    path = "/api/admin/email-templates/{id}/test-send",
    tag = "admin-email-templates",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = TestSendEmailTemplateParams,
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn test_send(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<TestSendEmailTemplateParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:email_template:test").await?;
    if params.to.trim().is_empty() {
        return Err(ApiError::bad_request("recipient email is required"));
    }

    let template = find_template(&ctx, id).await?;
    let rendered = email_templates::render_model(&template, &params.locals);
    AuthMailer::mail(&ctx, &rendered.into_email(params.to)).await?;
    Ok(responses::empty())
}

async fn find_template(ctx: &AppContext, id: i32) -> ApiResult<email_templates::Model> {
    email_templates::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("email template not found"))
}

fn validate_template(params: &SaveEmailTemplateParams) -> ApiResult<()> {
    for (field, value) in [
        ("code", params.code.as_str()),
        ("name", params.name.as_str()),
        ("template_type", params.template_type.as_str()),
        ("subject", params.subject.as_str()),
        ("html_body", params.html_body.as_str()),
        ("text_body", params.text_body.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(ApiError::bad_request(format!("{field} is required")));
        }
    }

    serde_json::from_str::<Value>(&params.variables)
        .map(|_| ())
        .map_err(|_| ApiError::bad_request("variables must be valid json"))
}

impl From<email_templates::Model> for EmailTemplateRecord {
    fn from(template: email_templates::Model) -> Self {
        Self {
            id: template.id,
            code: template.code,
            name: template.name,
            template_type: template.template_type,
            subject: template.subject,
            html_body: template.html_body,
            text_body: template.text_body,
            variables: template.variables,
            enabled: template.enabled,
            is_builtin: template.is_builtin,
            description: template.description,
            created_by: template.created_by,
            updated_by: template.updated_by,
            created_at: template.created_at.to_rfc3339(),
            updated_at: template.updated_at.to_rfc3339(),
        }
    }
}
