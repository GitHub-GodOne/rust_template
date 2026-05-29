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
    models::_entities::{content_articles, content_categories},
    responses::{self, ApiResponse, EmptyData, PageResponse},
};

const ARTICLE_STATUSES: &[&str] = &["draft", "published", "archived"];

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct ContentCategoryQueryParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub keyword: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct ContentArticleQueryParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub keyword: Option<String>,
    pub category_id: Option<i32>,
    pub status: Option<String>,
    pub is_featured: Option<bool>,
}

impl ContentCategoryQueryParams {
    #[must_use]
    pub fn page(&self) -> u64 {
        self.page.unwrap_or(1).max(1)
    }

    #[must_use]
    pub fn page_size(&self) -> u64 {
        self.page_size.unwrap_or(10).clamp(1, 100)
    }
}

impl ContentArticleQueryParams {
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
pub struct ContentCategoryRecord {
    pub id: i32,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub sort_order: i32,
    pub enabled: bool,
    pub created_by: Option<i32>,
    pub updated_by: Option<i32>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveContentCategoryParams {
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub sort_order: Option<i32>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct ContentArticleRecord {
    pub id: i32,
    pub category_id: i32,
    pub title: String,
    pub slug: String,
    pub summary: Option<String>,
    pub content: String,
    pub cover_image_url: Option<String>,
    pub status: String,
    pub is_featured: bool,
    pub published_at: Option<String>,
    pub seo_title: Option<String>,
    pub seo_description: Option<String>,
    pub created_by: Option<i32>,
    pub updated_by: Option<i32>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveContentArticleParams {
    pub category_id: i32,
    pub title: String,
    pub slug: String,
    pub summary: Option<String>,
    pub content: String,
    pub cover_image_url: Option<String>,
    pub status: Option<String>,
    pub is_featured: Option<bool>,
    pub published_at: Option<String>,
    pub seo_title: Option<String>,
    pub seo_description: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/admin/content-categories",
    tag = "admin-content",
    security(("bearer_auth" = [])),
    params(ContentCategoryQueryParams),
    responses((status = 200, body = ApiResponse<PageResponse<ContentCategoryRecord>>))
)]
#[debug_handler]
pub async fn list_categories(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<ContentCategoryQueryParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:content_category:list").await?;
    let page = params.page();
    let page_size = params.page_size();
    let mut query = content_categories::Entity::find()
        .order_by_asc(content_categories::Column::SortOrder)
        .order_by_desc(content_categories::Column::Id);

    if let Some(keyword) = params.keyword.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(
            Condition::any()
                .add(content_categories::Column::Name.contains(keyword))
                .add(content_categories::Column::Slug.contains(keyword))
                .add(content_categories::Column::Description.contains(keyword)),
        );
    }
    if let Some(enabled) = params.enabled {
        query = query.filter(content_categories::Column::Enabled.eq(enabled));
    }

    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(ContentCategoryRecord::from)
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
    path = "/api/admin/content-categories/{id}",
    tag = "admin-content",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<ContentCategoryRecord>))
)]
#[debug_handler]
pub async fn get_category(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:content_category:list").await?;
    let category = find_category(&ctx, id).await?;
    Ok(responses::ok(ContentCategoryRecord::from(category)))
}

#[utoipa::path(
    post,
    path = "/api/admin/content-categories",
    tag = "admin-content",
    security(("bearer_auth" = [])),
    request_body = SaveContentCategoryParams,
    responses((status = 200, body = ApiResponse<ContentCategoryRecord>))
)]
#[debug_handler]
pub async fn create_category(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SaveContentCategoryParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:content_category:create").await?;
    validate_category_params(&params)?;
    let category = content_categories::ActiveModel {
        name: Set(params.name.trim().to_string()),
        slug: Set(params.slug.trim().to_string()),
        description: Set(trim_optional(params.description)),
        sort_order: Set(params.sort_order.unwrap_or_default()),
        enabled: Set(params.enabled.unwrap_or(true)),
        created_by: Set(Some(actor.id)),
        updated_by: Set(Some(actor.id)),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;

    Ok(responses::ok(ContentCategoryRecord::from(category)))
}

#[utoipa::path(
    put,
    path = "/api/admin/content-categories/{id}",
    tag = "admin-content",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = SaveContentCategoryParams,
    responses((status = 200, body = ApiResponse<ContentCategoryRecord>))
)]
#[debug_handler]
pub async fn update_category(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<SaveContentCategoryParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:content_category:update").await?;
    validate_category_params(&params)?;
    let category = find_category(&ctx, id).await?;
    let mut active = category.into_active_model();
    active.name = Set(params.name.trim().to_string());
    active.slug = Set(params.slug.trim().to_string());
    active.description = Set(trim_optional(params.description));
    active.sort_order = Set(params.sort_order.unwrap_or_default());
    active.enabled = Set(params.enabled.unwrap_or(true));
    active.updated_by = Set(Some(actor.id));
    let category = active.update(&ctx.db).await?;

    Ok(responses::ok(ContentCategoryRecord::from(category)))
}

#[utoipa::path(
    delete,
    path = "/api/admin/content-categories/{id}",
    tag = "admin-content",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn delete_category(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:content_category:delete").await?;
    find_category(&ctx, id).await?;
    let article_count = content_articles::Entity::find()
        .filter(content_articles::Column::CategoryId.eq(id))
        .count(&ctx.db)
        .await?;
    if article_count > 0 {
        return Err(ApiError::bad_request("content category has articles"));
    }

    content_categories::Entity::delete_by_id(id)
        .exec(&ctx.db)
        .await?;
    Ok(responses::empty())
}

#[utoipa::path(
    get,
    path = "/api/admin/content-articles",
    tag = "admin-content",
    security(("bearer_auth" = [])),
    params(ContentArticleQueryParams),
    responses((status = 200, body = ApiResponse<PageResponse<ContentArticleRecord>>))
)]
#[debug_handler]
pub async fn list_articles(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<ContentArticleQueryParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:content_article:list").await?;
    let page = params.page();
    let page_size = params.page_size();
    let mut query = content_articles::Entity::find().order_by_desc(content_articles::Column::Id);

    if let Some(keyword) = params.keyword.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(
            Condition::any()
                .add(content_articles::Column::Title.contains(keyword))
                .add(content_articles::Column::Slug.contains(keyword))
                .add(content_articles::Column::Summary.contains(keyword)),
        );
    }
    if let Some(category_id) = params.category_id {
        query = query.filter(content_articles::Column::CategoryId.eq(category_id));
    }
    if let Some(status) = params.status.as_deref().filter(|value| !value.is_empty()) {
        validate_status(status)?;
        query = query.filter(content_articles::Column::Status.eq(status));
    }
    if let Some(is_featured) = params.is_featured {
        query = query.filter(content_articles::Column::IsFeatured.eq(is_featured));
    }

    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(ContentArticleRecord::from)
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
    path = "/api/admin/content-articles/{id}",
    tag = "admin-content",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<ContentArticleRecord>))
)]
#[debug_handler]
pub async fn get_article(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:content_article:list").await?;
    let article = find_article(&ctx, id).await?;
    Ok(responses::ok(ContentArticleRecord::from(article)))
}

#[utoipa::path(
    post,
    path = "/api/admin/content-articles",
    tag = "admin-content",
    security(("bearer_auth" = [])),
    request_body = SaveContentArticleParams,
    responses((status = 200, body = ApiResponse<ContentArticleRecord>))
)]
#[debug_handler]
pub async fn create_article(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SaveContentArticleParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:content_article:create").await?;
    validate_article_params(&ctx, &params).await?;
    let status = params
        .status
        .as_deref()
        .unwrap_or("draft")
        .trim()
        .to_string();
    let article = content_articles::ActiveModel {
        category_id: Set(params.category_id),
        title: Set(params.title.trim().to_string()),
        slug: Set(params.slug.trim().to_string()),
        summary: Set(trim_optional(params.summary)),
        content: Set(params.content.trim().to_string()),
        cover_image_url: Set(trim_optional(params.cover_image_url)),
        status: Set(status),
        is_featured: Set(params.is_featured.unwrap_or(false)),
        published_at: Set(parse_optional_datetime(params.published_at)?),
        seo_title: Set(trim_optional(params.seo_title)),
        seo_description: Set(trim_optional(params.seo_description)),
        created_by: Set(Some(actor.id)),
        updated_by: Set(Some(actor.id)),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;

    Ok(responses::ok(ContentArticleRecord::from(article)))
}

#[utoipa::path(
    put,
    path = "/api/admin/content-articles/{id}",
    tag = "admin-content",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = SaveContentArticleParams,
    responses((status = 200, body = ApiResponse<ContentArticleRecord>))
)]
#[debug_handler]
pub async fn update_article(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<SaveContentArticleParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:content_article:update").await?;
    validate_article_params(&ctx, &params).await?;
    let article = find_article(&ctx, id).await?;
    let mut active = article.into_active_model();
    active.category_id = Set(params.category_id);
    active.title = Set(params.title.trim().to_string());
    active.slug = Set(params.slug.trim().to_string());
    active.summary = Set(trim_optional(params.summary));
    active.content = Set(params.content.trim().to_string());
    active.cover_image_url = Set(trim_optional(params.cover_image_url));
    active.status = Set(params
        .status
        .as_deref()
        .unwrap_or("draft")
        .trim()
        .to_string());
    active.is_featured = Set(params.is_featured.unwrap_or(false));
    active.published_at = Set(parse_optional_datetime(params.published_at)?);
    active.seo_title = Set(trim_optional(params.seo_title));
    active.seo_description = Set(trim_optional(params.seo_description));
    active.updated_by = Set(Some(actor.id));
    let article = active.update(&ctx.db).await?;

    Ok(responses::ok(ContentArticleRecord::from(article)))
}

#[utoipa::path(
    post,
    path = "/api/admin/content-articles/{id}/publish",
    tag = "admin-content",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<ContentArticleRecord>))
)]
#[debug_handler]
pub async fn publish_article(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:content_article:publish").await?;
    let article = find_article(&ctx, id).await?;
    let mut active = article.into_active_model();
    active.status = Set("published".to_string());
    active.published_at = Set(Some(Local::now().into()));
    active.updated_by = Set(Some(actor.id));
    let article = active.update(&ctx.db).await?;

    Ok(responses::ok(ContentArticleRecord::from(article)))
}

#[utoipa::path(
    post,
    path = "/api/admin/content-articles/{id}/archive",
    tag = "admin-content",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<ContentArticleRecord>))
)]
#[debug_handler]
pub async fn archive_article(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:content_article:update").await?;
    let article = find_article(&ctx, id).await?;
    let mut active = article.into_active_model();
    active.status = Set("archived".to_string());
    active.updated_by = Set(Some(actor.id));
    let article = active.update(&ctx.db).await?;

    Ok(responses::ok(ContentArticleRecord::from(article)))
}

#[utoipa::path(
    delete,
    path = "/api/admin/content-articles/{id}",
    tag = "admin-content",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn delete_article(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:content_article:delete").await?;
    find_article(&ctx, id).await?;
    content_articles::Entity::delete_by_id(id)
        .exec(&ctx.db)
        .await?;
    Ok(responses::empty())
}

async fn find_category(ctx: &AppContext, id: i32) -> ApiResult<content_categories::Model> {
    content_categories::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("content category not found"))
}

async fn find_article(ctx: &AppContext, id: i32) -> ApiResult<content_articles::Model> {
    content_articles::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("content article not found"))
}

fn validate_category_params(params: &SaveContentCategoryParams) -> ApiResult<()> {
    require_non_empty("name", &params.name)?;
    validate_slug(&params.slug)
}

async fn validate_article_params(
    ctx: &AppContext,
    params: &SaveContentArticleParams,
) -> ApiResult<()> {
    find_category(ctx, params.category_id).await?;
    require_non_empty("title", &params.title)?;
    require_non_empty("content", &params.content)?;
    validate_slug(&params.slug)?;
    validate_status(params.status.as_deref().unwrap_or("draft"))?;
    parse_optional_datetime(params.published_at.clone()).map(|_| ())
}

fn require_non_empty(field: &str, value: &str) -> ApiResult<()> {
    if value.trim().is_empty() {
        return Err(ApiError::bad_request(format!("{field} is required")));
    }
    Ok(())
}

fn validate_slug(slug: &str) -> ApiResult<()> {
    let slug = slug.trim();
    if slug.is_empty() {
        return Err(ApiError::bad_request("slug is required"));
    }
    if !slug
        .chars()
        .all(|value| value.is_ascii_lowercase() || value.is_ascii_digit() || value == '-')
    {
        return Err(ApiError::bad_request(
            "slug only supports lowercase letters, numbers and hyphens",
        ));
    }
    Ok(())
}

fn validate_status(status: &str) -> ApiResult<()> {
    let value = status.trim();
    if ARTICLE_STATUSES.contains(&value) {
        return Ok(());
    }
    Err(ApiError::bad_request("unsupported content article status"))
}

fn trim_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_optional_datetime(
    value: Option<String>,
) -> ApiResult<Option<chrono::DateTime<chrono::FixedOffset>>> {
    value
        .and_then(|value| trim_optional(Some(value)))
        .as_deref()
        .map(chrono::DateTime::parse_from_rfc3339)
        .transpose()
        .map_err(|_| ApiError::bad_request("datetime must be RFC3339"))
}

impl From<content_categories::Model> for ContentCategoryRecord {
    fn from(category: content_categories::Model) -> Self {
        Self {
            id: category.id,
            name: category.name,
            slug: category.slug,
            description: category.description,
            sort_order: category.sort_order,
            enabled: category.enabled,
            created_by: category.created_by,
            updated_by: category.updated_by,
            created_at: category.created_at.to_rfc3339(),
            updated_at: category.updated_at.to_rfc3339(),
        }
    }
}

impl From<content_articles::Model> for ContentArticleRecord {
    fn from(article: content_articles::Model) -> Self {
        Self {
            id: article.id,
            category_id: article.category_id,
            title: article.title,
            slug: article.slug,
            summary: article.summary,
            content: article.content,
            cover_image_url: article.cover_image_url,
            status: article.status,
            is_featured: article.is_featured,
            published_at: article.published_at.map(|value| value.to_rfc3339()),
            seo_title: article.seo_title,
            seo_description: article.seo_description,
            created_by: article.created_by,
            updated_by: article.updated_by,
            created_at: article.created_at.to_rfc3339(),
            updated_at: article.updated_at.to_rfc3339(),
        }
    }
}
