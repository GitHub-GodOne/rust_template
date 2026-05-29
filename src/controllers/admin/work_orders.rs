#![allow(clippy::missing_errors_doc)]

use chrono::offset::Local;
use loco_rs::prelude::*;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, EntityTrait, IntoActiveModel, PaginatorTrait,
    QueryFilter, QueryOrder, Set,
};
use serde::{Deserialize, Serialize};

use crate::{
    controllers::admin::authorize,
    errors::{ApiError, ApiResult},
    models::{
        _entities::{
            upload_files, users, work_order_assignments, work_order_attachments,
            work_order_comments, work_orders,
        },
        rbac,
    },
    responses::{self, ApiResponse, EmptyData, PageResponse},
};

const PRIORITIES: &[&str] = &["low", "normal", "high", "urgent"];
const STATUSES: &[&str] = &[
    "open",
    "assigned",
    "in_progress",
    "pending",
    "resolved",
    "closed",
    "cancelled",
];

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct WorkOrderQueryParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub keyword: Option<String>,
    pub status: Option<String>,
    pub priority: Option<String>,
    pub category: Option<String>,
    pub assignee_id: Option<i32>,
    pub creator_id: Option<i32>,
}

impl WorkOrderQueryParams {
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
pub struct WorkOrderRecord {
    pub id: i32,
    pub order_no: String,
    pub title: String,
    pub description: String,
    pub category: Option<String>,
    pub priority: String,
    pub status: String,
    pub source: String,
    pub tenant_id: Option<i32>,
    pub creator_id: Option<i32>,
    pub assignee_id: Option<i32>,
    pub assigned_at: Option<String>,
    pub resolved_at: Option<String>,
    pub closed_at: Option<String>,
    pub due_at: Option<String>,
    pub last_comment_at: Option<String>,
    pub metadata: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct WorkOrderDetailRecord {
    #[serde(flatten)]
    pub work_order: WorkOrderRecord,
    pub comments: Vec<WorkOrderCommentRecord>,
    pub assignments: Vec<WorkOrderAssignmentRecord>,
    pub attachments: Vec<WorkOrderAttachmentRecord>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct WorkOrderCommentRecord {
    pub id: i32,
    pub tenant_id: Option<i32>,
    pub work_order_id: i32,
    pub author_id: Option<i32>,
    pub body: String,
    pub comment_type: String,
    pub from_status: Option<String>,
    pub to_status: Option<String>,
    pub metadata: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct WorkOrderAssignmentRecord {
    pub id: i32,
    pub tenant_id: Option<i32>,
    pub work_order_id: i32,
    pub assignee_id: i32,
    pub assigned_by_id: Option<i32>,
    pub note: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct WorkOrderAttachmentRecord {
    pub id: i32,
    pub tenant_id: Option<i32>,
    pub work_order_id: i32,
    pub upload_file_id: i32,
    pub uploaded_by_id: Option<i32>,
    pub description: Option<String>,
    pub original_name: Option<String>,
    pub url: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveWorkOrderParams {
    pub title: String,
    pub description: String,
    pub category: Option<String>,
    pub priority: Option<String>,
    pub assignee_id: Option<i32>,
    pub due_at: Option<String>,
    pub metadata: Option<String>,
    pub attachment_file_ids: Option<Vec<i32>>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct TransitionWorkOrderParams {
    pub status: String,
    pub comment: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CreateWorkOrderCommentParams {
    pub body: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct AssignWorkOrderParams {
    pub assignee_id: i32,
    pub note: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CreateWorkOrderAttachmentParams {
    pub upload_file_id: i32,
    pub description: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/admin/work-orders",
    tag = "admin-work-orders",
    security(("bearer_auth" = [])),
    params(WorkOrderQueryParams),
    responses((status = 200, body = ApiResponse<PageResponse<WorkOrderRecord>>))
)]
#[debug_handler]
pub async fn list(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<WorkOrderQueryParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:work_order:list").await?;
    let page = params.page();
    let page_size = params.page_size();
    let mut query = work_orders::Entity::find().order_by_desc(work_orders::Column::Id);

    if !rbac::is_super_admin(&ctx.db, actor.id).await? {
        query = query.filter(visible_condition(&actor));
    }
    if let Some(keyword) = params.keyword.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(
            Condition::any()
                .add(work_orders::Column::OrderNo.contains(keyword))
                .add(work_orders::Column::Title.contains(keyword))
                .add(work_orders::Column::Description.contains(keyword)),
        );
    }
    if let Some(status) = params.status.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(work_orders::Column::Status.eq(status));
    }
    if let Some(priority) = params.priority.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(work_orders::Column::Priority.eq(priority));
    }
    if let Some(category) = params.category.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(work_orders::Column::Category.eq(category));
    }
    if let Some(assignee_id) = params.assignee_id {
        query = query.filter(work_orders::Column::AssigneeId.eq(assignee_id));
    }
    if let Some(creator_id) = params.creator_id {
        query = query.filter(work_orders::Column::CreatorId.eq(creator_id));
    }

    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(WorkOrderRecord::from)
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
    path = "/api/admin/work-orders/{id}",
    tag = "admin-work-orders",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<WorkOrderDetailRecord>))
)]
#[debug_handler]
pub async fn get(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:work_order:list").await?;
    let work_order = find_visible_work_order(&ctx, &actor, id).await?;
    let detail = load_detail(&ctx, work_order).await?;
    Ok(responses::ok(detail))
}

#[utoipa::path(
    post,
    path = "/api/admin/work-orders",
    tag = "admin-work-orders",
    security(("bearer_auth" = [])),
    request_body = SaveWorkOrderParams,
    responses((status = 200, body = ApiResponse<WorkOrderRecord>))
)]
#[debug_handler]
pub async fn create(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SaveWorkOrderParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:work_order:create").await?;
    validate_save_params(&params)?;
    ensure_assignee(&ctx, params.assignee_id).await?;

    let priority = params.priority.unwrap_or_else(|| "normal".to_string());
    let assigned_at = params.assignee_id.map(|_| Local::now().into());
    let status = if params.assignee_id.is_some() {
        "assigned"
    } else {
        "open"
    };
    let work_order = work_orders::ActiveModel {
        order_no: Set(next_order_no()),
        title: Set(params.title.trim().to_string()),
        description: Set(params.description.trim().to_string()),
        category: Set(trim_optional(params.category)),
        priority: Set(priority),
        status: Set(status.to_string()),
        source: Set("admin".to_string()),
        tenant_id: Set(actor.tenant_id),
        creator_id: Set(Some(actor.id)),
        assignee_id: Set(params.assignee_id),
        assigned_at: Set(assigned_at),
        resolved_at: Set(None),
        closed_at: Set(None),
        due_at: Set(parse_time(params.due_at.as_deref())?),
        last_comment_at: Set(None),
        metadata: Set(trim_optional(params.metadata)),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;

    if let Some(assignee_id) = work_order.assignee_id {
        create_assignment_record(&ctx, &work_order, assignee_id, Some(actor.id), None).await?;
    }
    if let Some(file_ids) = params.attachment_file_ids {
        for file_id in file_ids {
            add_attachment_record(&ctx, &work_order, file_id, Some(actor.id), None).await?;
        }
    }

    Ok(responses::ok(WorkOrderRecord::from(work_order)))
}

#[utoipa::path(
    put,
    path = "/api/admin/work-orders/{id}",
    tag = "admin-work-orders",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = SaveWorkOrderParams,
    responses((status = 200, body = ApiResponse<WorkOrderRecord>))
)]
#[debug_handler]
pub async fn update(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<SaveWorkOrderParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:work_order:update").await?;
    validate_save_params(&params)?;
    ensure_assignee(&ctx, params.assignee_id).await?;
    let work_order = find_visible_work_order(&ctx, &actor, id).await?;
    let assignee_changed = work_order.assignee_id != params.assignee_id;

    let mut active = work_order.clone().into_active_model();
    active.title = Set(params.title.trim().to_string());
    active.description = Set(params.description.trim().to_string());
    active.category = Set(trim_optional(params.category));
    active.priority = Set(params.priority.unwrap_or_else(|| "normal".to_string()));
    active.assignee_id = Set(params.assignee_id);
    if assignee_changed {
        active.assigned_at = Set(params.assignee_id.map(|_| Local::now().into()));
        if params.assignee_id.is_some() && work_order.status == "open" {
            active.status = Set("assigned".to_string());
        }
    }
    active.due_at = Set(parse_time(params.due_at.as_deref())?);
    active.metadata = Set(trim_optional(params.metadata));
    let work_order = active.update(&ctx.db).await?;

    if assignee_changed {
        if let Some(assignee_id) = work_order.assignee_id {
            create_assignment_record(&ctx, &work_order, assignee_id, Some(actor.id), None).await?;
        }
    }
    if let Some(file_ids) = params.attachment_file_ids {
        for file_id in file_ids {
            add_attachment_record(&ctx, &work_order, file_id, Some(actor.id), None).await?;
        }
    }

    Ok(responses::ok(WorkOrderRecord::from(work_order)))
}

#[utoipa::path(
    delete,
    path = "/api/admin/work-orders/{id}",
    tag = "admin-work-orders",
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
    let actor = authorize(&ctx, &auth, "system:work_order:delete").await?;
    find_visible_work_order(&ctx, &actor, id).await?;
    work_order_attachments::Entity::delete_many()
        .filter(work_order_attachments::Column::WorkOrderId.eq(id))
        .exec(&ctx.db)
        .await?;
    work_order_assignments::Entity::delete_many()
        .filter(work_order_assignments::Column::WorkOrderId.eq(id))
        .exec(&ctx.db)
        .await?;
    work_order_comments::Entity::delete_many()
        .filter(work_order_comments::Column::WorkOrderId.eq(id))
        .exec(&ctx.db)
        .await?;
    work_orders::Entity::delete_by_id(id).exec(&ctx.db).await?;
    Ok(responses::empty())
}

#[utoipa::path(
    post,
    path = "/api/admin/work-orders/{id}/transition",
    tag = "admin-work-orders",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = TransitionWorkOrderParams,
    responses((status = 200, body = ApiResponse<WorkOrderRecord>))
)]
#[debug_handler]
pub async fn transition(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<TransitionWorkOrderParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:work_order:transition").await?;
    let work_order = find_visible_work_order(&ctx, &actor, id).await?;
    validate_transition(&work_order.status, &params.status)?;

    let now = Local::now();
    let mut active = work_order.clone().into_active_model();
    active.status = Set(params.status.clone());
    if params.status == "resolved" {
        active.resolved_at = Set(Some(now.into()));
    }
    if params.status == "closed" {
        active.closed_at = Set(Some(now.into()));
    }
    let updated = active.update(&ctx.db).await?;
    let body = params
        .comment
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("status changed to {}", params.status));
    create_comment_record(
        &ctx,
        &updated,
        Some(actor.id),
        body,
        "status_change",
        Some(work_order.status),
        Some(params.status),
    )
    .await?;

    Ok(responses::ok(WorkOrderRecord::from(updated)))
}

#[utoipa::path(
    get,
    path = "/api/admin/work-orders/{id}/comments",
    tag = "admin-work-orders",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<Vec<WorkOrderCommentRecord>>))
)]
#[debug_handler]
pub async fn list_comments(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:work_order:list").await?;
    find_visible_work_order(&ctx, &actor, id).await?;
    Ok(responses::ok(load_comments(&ctx, id).await?))
}

#[utoipa::path(
    post,
    path = "/api/admin/work-orders/{id}/comments",
    tag = "admin-work-orders",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = CreateWorkOrderCommentParams,
    responses((status = 200, body = ApiResponse<WorkOrderCommentRecord>))
)]
#[debug_handler]
pub async fn create_comment(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<CreateWorkOrderCommentParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:work_order:comment").await?;
    if params.body.trim().is_empty() {
        return Err(ApiError::bad_request("comment body is required"));
    }
    let work_order = find_visible_work_order(&ctx, &actor, id).await?;
    let comment = create_comment_record(
        &ctx,
        &work_order,
        Some(actor.id),
        params.body,
        "comment",
        None,
        None,
    )
    .await?;
    Ok(responses::ok(WorkOrderCommentRecord::from(comment)))
}

#[utoipa::path(
    post,
    path = "/api/admin/work-orders/{id}/assign",
    tag = "admin-work-orders",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = AssignWorkOrderParams,
    responses((status = 200, body = ApiResponse<WorkOrderRecord>))
)]
#[debug_handler]
pub async fn assign(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<AssignWorkOrderParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:work_order:assign").await?;
    ensure_assignee(&ctx, Some(params.assignee_id)).await?;
    let work_order = find_visible_work_order(&ctx, &actor, id).await?;

    let mut active = work_order.into_active_model();
    active.assignee_id = Set(Some(params.assignee_id));
    active.assigned_at = Set(Some(Local::now().into()));
    active.status = Set("assigned".to_string());
    let updated = active.update(&ctx.db).await?;
    create_assignment_record(
        &ctx,
        &updated,
        params.assignee_id,
        Some(actor.id),
        params.note.clone(),
    )
    .await?;
    create_comment_record(
        &ctx,
        &updated,
        Some(actor.id),
        params
            .note
            .unwrap_or_else(|| "work order assigned".to_string()),
        "assignment",
        None,
        None,
    )
    .await?;

    Ok(responses::ok(WorkOrderRecord::from(updated)))
}

#[utoipa::path(
    get,
    path = "/api/admin/work-orders/{id}/attachments",
    tag = "admin-work-orders",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<Vec<WorkOrderAttachmentRecord>>))
)]
#[debug_handler]
pub async fn list_attachments(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:work_order:list").await?;
    find_visible_work_order(&ctx, &actor, id).await?;
    Ok(responses::ok(load_attachments(&ctx, id).await?))
}

#[utoipa::path(
    post,
    path = "/api/admin/work-orders/{id}/attachments",
    tag = "admin-work-orders",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = CreateWorkOrderAttachmentParams,
    responses((status = 200, body = ApiResponse<WorkOrderAttachmentRecord>))
)]
#[debug_handler]
pub async fn create_attachment(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<CreateWorkOrderAttachmentParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:work_order:attachment").await?;
    let work_order = find_visible_work_order(&ctx, &actor, id).await?;
    let attachment = add_attachment_record(
        &ctx,
        &work_order,
        params.upload_file_id,
        Some(actor.id),
        params.description,
    )
    .await?;
    Ok(responses::ok(attachment))
}

#[utoipa::path(
    delete,
    path = "/api/admin/work-orders/{id}/attachments/{attachment_id}",
    tag = "admin-work-orders",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path), ("attachment_id" = i32, Path)),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn delete_attachment(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path((id, attachment_id)): Path<(i32, i32)>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:work_order:attachment").await?;
    find_visible_work_order(&ctx, &actor, id).await?;
    let attachment = work_order_attachments::Entity::find_by_id(attachment_id)
        .filter(work_order_attachments::Column::WorkOrderId.eq(id))
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("work order attachment not found"))?;
    work_order_attachments::Entity::delete_by_id(attachment.id)
        .exec(&ctx.db)
        .await?;
    Ok(responses::empty())
}

async fn load_detail(
    ctx: &AppContext,
    work_order: work_orders::Model,
) -> ApiResult<WorkOrderDetailRecord> {
    let id = work_order.id;
    Ok(WorkOrderDetailRecord {
        work_order: WorkOrderRecord::from(work_order),
        comments: load_comments(ctx, id).await?,
        assignments: load_assignments(ctx, id).await?,
        attachments: load_attachments(ctx, id).await?,
    })
}

async fn load_comments(ctx: &AppContext, id: i32) -> ApiResult<Vec<WorkOrderCommentRecord>> {
    Ok(work_order_comments::Entity::find()
        .filter(work_order_comments::Column::WorkOrderId.eq(id))
        .order_by_asc(work_order_comments::Column::Id)
        .all(&ctx.db)
        .await?
        .into_iter()
        .map(WorkOrderCommentRecord::from)
        .collect())
}

async fn load_assignments(ctx: &AppContext, id: i32) -> ApiResult<Vec<WorkOrderAssignmentRecord>> {
    Ok(work_order_assignments::Entity::find()
        .filter(work_order_assignments::Column::WorkOrderId.eq(id))
        .order_by_asc(work_order_assignments::Column::Id)
        .all(&ctx.db)
        .await?
        .into_iter()
        .map(WorkOrderAssignmentRecord::from)
        .collect())
}

async fn load_attachments(ctx: &AppContext, id: i32) -> ApiResult<Vec<WorkOrderAttachmentRecord>> {
    let attachments = work_order_attachments::Entity::find()
        .filter(work_order_attachments::Column::WorkOrderId.eq(id))
        .order_by_asc(work_order_attachments::Column::Id)
        .all(&ctx.db)
        .await?;
    let mut records = Vec::with_capacity(attachments.len());
    for attachment in attachments {
        records.push(attachment_record(ctx, attachment).await?);
    }
    Ok(records)
}

async fn find_visible_work_order(
    ctx: &AppContext,
    actor: &users::Model,
    id: i32,
) -> ApiResult<work_orders::Model> {
    let work_order = work_orders::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("work order not found"))?;
    if rbac::is_super_admin(&ctx.db, actor.id).await? || is_visible(actor, &work_order) {
        Ok(work_order)
    } else {
        Err(ApiError::forbidden("work order is not visible"))
    }
}

fn visible_condition(actor: &users::Model) -> Condition {
    let mut condition = Condition::any()
        .add(work_orders::Column::CreatorId.eq(actor.id))
        .add(work_orders::Column::AssigneeId.eq(actor.id));
    if let Some(tenant_id) = actor.tenant_id {
        condition = condition.add(work_orders::Column::TenantId.eq(tenant_id));
    }
    condition
}

fn is_visible(actor: &users::Model, work_order: &work_orders::Model) -> bool {
    work_order.creator_id == Some(actor.id)
        || work_order.assignee_id == Some(actor.id)
        || actor
            .tenant_id
            .is_some_and(|tenant_id| work_order.tenant_id == Some(tenant_id))
}

async fn ensure_assignee(ctx: &AppContext, assignee_id: Option<i32>) -> ApiResult<()> {
    if let Some(assignee_id) = assignee_id {
        users::Entity::find_by_id(assignee_id)
            .one(&ctx.db)
            .await?
            .ok_or_else(|| ApiError::bad_request("assignee not found"))?;
    }
    Ok(())
}

async fn create_assignment_record(
    ctx: &AppContext,
    work_order: &work_orders::Model,
    assignee_id: i32,
    assigned_by_id: Option<i32>,
    note: Option<String>,
) -> ApiResult<work_order_assignments::Model> {
    Ok(work_order_assignments::ActiveModel {
        tenant_id: Set(work_order.tenant_id),
        work_order_id: Set(work_order.id),
        assignee_id: Set(assignee_id),
        assigned_by_id: Set(assigned_by_id),
        note: Set(note.and_then(|value| trim_optional(Some(value)))),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?)
}

async fn create_comment_record(
    ctx: &AppContext,
    work_order: &work_orders::Model,
    author_id: Option<i32>,
    body: String,
    comment_type: &str,
    from_status: Option<String>,
    to_status: Option<String>,
) -> ApiResult<work_order_comments::Model> {
    let comment = work_order_comments::ActiveModel {
        tenant_id: Set(work_order.tenant_id),
        work_order_id: Set(work_order.id),
        author_id: Set(author_id),
        body: Set(body.trim().to_string()),
        comment_type: Set(comment_type.to_string()),
        from_status: Set(from_status),
        to_status: Set(to_status),
        metadata: Set(None),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;

    let mut active = work_order.clone().into_active_model();
    active.last_comment_at = Set(Some(comment.created_at));
    active.update(&ctx.db).await?;
    Ok(comment)
}

async fn add_attachment_record(
    ctx: &AppContext,
    work_order: &work_orders::Model,
    upload_file_id: i32,
    uploaded_by_id: Option<i32>,
    description: Option<String>,
) -> ApiResult<WorkOrderAttachmentRecord> {
    upload_files::Entity::find_by_id(upload_file_id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("upload file not found"))?;
    if work_order_attachments::Entity::find()
        .filter(work_order_attachments::Column::WorkOrderId.eq(work_order.id))
        .filter(work_order_attachments::Column::UploadFileId.eq(upload_file_id))
        .one(&ctx.db)
        .await?
        .is_some()
    {
        return Err(ApiError::bad_request("upload file already attached"));
    }

    let attachment = work_order_attachments::ActiveModel {
        tenant_id: Set(work_order.tenant_id),
        work_order_id: Set(work_order.id),
        upload_file_id: Set(upload_file_id),
        uploaded_by_id: Set(uploaded_by_id),
        description: Set(description.and_then(|value| trim_optional(Some(value)))),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;
    attachment_record(ctx, attachment).await
}

async fn attachment_record(
    ctx: &AppContext,
    attachment: work_order_attachments::Model,
) -> ApiResult<WorkOrderAttachmentRecord> {
    let file = upload_files::Entity::find_by_id(attachment.upload_file_id)
        .one(&ctx.db)
        .await?;
    Ok(WorkOrderAttachmentRecord {
        id: attachment.id,
        tenant_id: attachment.tenant_id,
        work_order_id: attachment.work_order_id,
        upload_file_id: attachment.upload_file_id,
        uploaded_by_id: attachment.uploaded_by_id,
        description: attachment.description,
        original_name: file.as_ref().map(|value| value.original_name.clone()),
        url: file.map(|value| value.url),
        created_at: attachment.created_at.to_rfc3339(),
        updated_at: attachment.updated_at.to_rfc3339(),
    })
}

fn validate_save_params(params: &SaveWorkOrderParams) -> ApiResult<()> {
    if params.title.trim().is_empty() || params.description.trim().is_empty() {
        return Err(ApiError::bad_request(
            "work order title and description are required",
        ));
    }
    if let Some(priority) = params.priority.as_deref() {
        validate_value(priority, PRIORITIES, "unsupported work order priority")?;
    }
    if let Some(metadata) = params.metadata.as_deref().filter(|value| !value.is_empty()) {
        serde_json::from_str::<serde_json::Value>(metadata)
            .map_err(|_| ApiError::bad_request("metadata must be valid json"))?;
    }
    parse_time(params.due_at.as_deref())?;
    Ok(())
}

fn validate_transition(from: &str, to: &str) -> ApiResult<()> {
    validate_value(to, STATUSES, "unsupported work order status")?;
    let allowed = match from {
        "open" => matches!(to, "assigned" | "in_progress" | "cancelled"),
        "assigned" => matches!(to, "in_progress" | "pending" | "cancelled"),
        "in_progress" => matches!(to, "pending" | "resolved" | "cancelled"),
        "pending" => matches!(to, "in_progress" | "resolved" | "cancelled"),
        "resolved" => matches!(to, "closed" | "in_progress"),
        _ => false,
    };
    if allowed {
        Ok(())
    } else {
        Err(ApiError::bad_request("unsupported work order transition"))
    }
}

fn validate_value(value: &str, supported: &[&str], message: &str) -> ApiResult<()> {
    if supported.contains(&value) {
        Ok(())
    } else {
        Err(ApiError::bad_request(message))
    }
}

fn trim_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_time(value: Option<&str>) -> ApiResult<Option<chrono::DateTime<chrono::FixedOffset>>> {
    value
        .filter(|value| !value.is_empty())
        .map(chrono::DateTime::parse_from_rfc3339)
        .transpose()
        .map_err(|_| ApiError::bad_request("time must be RFC3339"))
}

fn next_order_no() -> String {
    format!("WO{}", Local::now().format("%Y%m%d%H%M%S%3f"))
}

impl From<work_orders::Model> for WorkOrderRecord {
    fn from(work_order: work_orders::Model) -> Self {
        Self {
            id: work_order.id,
            order_no: work_order.order_no,
            title: work_order.title,
            description: work_order.description,
            category: work_order.category,
            priority: work_order.priority,
            status: work_order.status,
            source: work_order.source,
            tenant_id: work_order.tenant_id,
            creator_id: work_order.creator_id,
            assignee_id: work_order.assignee_id,
            assigned_at: work_order.assigned_at.map(|value| value.to_rfc3339()),
            resolved_at: work_order.resolved_at.map(|value| value.to_rfc3339()),
            closed_at: work_order.closed_at.map(|value| value.to_rfc3339()),
            due_at: work_order.due_at.map(|value| value.to_rfc3339()),
            last_comment_at: work_order.last_comment_at.map(|value| value.to_rfc3339()),
            metadata: work_order.metadata,
            created_at: work_order.created_at.to_rfc3339(),
            updated_at: work_order.updated_at.to_rfc3339(),
        }
    }
}

impl From<work_order_comments::Model> for WorkOrderCommentRecord {
    fn from(comment: work_order_comments::Model) -> Self {
        Self {
            id: comment.id,
            tenant_id: comment.tenant_id,
            work_order_id: comment.work_order_id,
            author_id: comment.author_id,
            body: comment.body,
            comment_type: comment.comment_type,
            from_status: comment.from_status,
            to_status: comment.to_status,
            metadata: comment.metadata,
            created_at: comment.created_at.to_rfc3339(),
            updated_at: comment.updated_at.to_rfc3339(),
        }
    }
}

impl From<work_order_assignments::Model> for WorkOrderAssignmentRecord {
    fn from(assignment: work_order_assignments::Model) -> Self {
        Self {
            id: assignment.id,
            tenant_id: assignment.tenant_id,
            work_order_id: assignment.work_order_id,
            assignee_id: assignment.assignee_id,
            assigned_by_id: assignment.assigned_by_id,
            note: assignment.note,
            created_at: assignment.created_at.to_rfc3339(),
            updated_at: assignment.updated_at.to_rfc3339(),
        }
    }
}
