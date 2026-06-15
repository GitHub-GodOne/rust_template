#![allow(clippy::missing_errors_doc)]

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
        _entities::{storage_buckets, storage_profiles, tenants, upload_files, users},
        rbac::{self, EffectiveDataScope},
    },
    responses::{self, ApiResponse, EmptyData, PageParams, PageResponse},
    services::storage,
};

const SECRET_MASK: &str = "******";

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct StorageProfileRecord {
    pub id: i32,
    pub tenant_id: i32,
    pub name: String,
    pub code: String,
    pub provider: String,
    pub enabled: bool,
    pub is_default: bool,
    pub endpoint: Option<String>,
    pub region: Option<String>,
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
    pub public_base_url: Option<String>,
    pub path_style: bool,
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct StorageBucketRecord {
    pub id: i32,
    pub storage_profile_id: i32,
    pub tenant_id: i32,
    pub name: String,
    pub bucket: String,
    pub base_prefix: Option<String>,
    pub local_root: Option<String>,
    pub public_prefix: Option<String>,
    pub enabled: bool,
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveStorageProfileParams {
    pub tenant_id: Option<i32>,
    pub name: String,
    pub code: String,
    pub provider: String,
    pub enabled: Option<bool>,
    pub is_default: Option<bool>,
    pub endpoint: Option<String>,
    pub region: Option<String>,
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
    pub public_base_url: Option<String>,
    pub path_style: Option<bool>,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveStorageBucketParams {
    pub tenant_id: Option<i32>,
    pub name: String,
    pub bucket: String,
    pub base_prefix: Option<String>,
    pub local_root: Option<String>,
    pub public_prefix: Option<String>,
    pub enabled: Option<bool>,
    pub is_default: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct StorageTestRecord {
    pub ok: bool,
    pub message: String,
}

#[utoipa::path(
    get,
    path = "/api/admin/storage-profiles",
    tag = "admin-storage",
    security(("bearer_auth" = [])),
    params(PageParams),
    responses((status = 200, body = ApiResponse<PageResponse<StorageProfileRecord>>))
)]
#[debug_handler]
pub async fn list(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<PageParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:storage:list").await?;
    let scope = rbac::resolve_data_scope(&ctx.db, &actor).await?;
    let page = params.page();
    let page_size = params.page_size();
    let mut query = apply_profile_scope(storage_profiles::Entity::find(), &scope)
        .order_by_asc(storage_profiles::Column::TenantId)
        .order_by_desc(storage_profiles::Column::IsDefault)
        .order_by_asc(storage_profiles::Column::Id);

    if let Some(keyword) = params.keyword.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(
            Condition::any()
                .add(storage_profiles::Column::Name.contains(keyword))
                .add(storage_profiles::Column::Code.contains(keyword))
                .add(storage_profiles::Column::Provider.contains(keyword)),
        );
    }

    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(StorageProfileRecord::from)
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
    path = "/api/admin/storage-profiles/{id}",
    tag = "admin-storage",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<StorageProfileRecord>))
)]
#[debug_handler]
pub async fn get(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:storage:list").await?;
    let profile = find_profile(&ctx, id).await?;
    assert_profile_visible(&ctx, &actor, &profile).await?;
    Ok(responses::ok(StorageProfileRecord::from(profile)))
}

#[utoipa::path(
    post,
    path = "/api/admin/storage-profiles",
    tag = "admin-storage",
    security(("bearer_auth" = [])),
    request_body = SaveStorageProfileParams,
    responses((status = 200, body = ApiResponse<StorageProfileRecord>))
)]
#[debug_handler]
pub async fn create(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SaveStorageProfileParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:storage:create").await?;
    let tenant_id = resolve_storage_tenant(&ctx, &actor, params.tenant_id).await?;
    validate_profile(&ctx, tenant_id, None, &params).await?;
    if params.is_default.unwrap_or(false) {
        clear_default_profiles(&ctx, tenant_id).await?;
    }

    let profile = storage_profiles::ActiveModel {
        tenant_id: Set(tenant_id),
        name: Set(params.name),
        code: Set(params.code),
        provider: Set(params.provider),
        enabled: Set(params.enabled.unwrap_or(true)),
        is_default: Set(params.is_default.unwrap_or(false)),
        endpoint: Set(blank_to_none(params.endpoint)),
        region: Set(blank_to_none(params.region)),
        access_key_id: Set(blank_to_none(params.access_key_id)),
        secret_access_key: Set(blank_to_none(params.secret_access_key)),
        public_base_url: Set(blank_to_none(params.public_base_url)),
        path_style: Set(params.path_style.unwrap_or(false)),
        description: Set(params.description),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;

    Ok(responses::ok(StorageProfileRecord::from(profile)))
}

#[utoipa::path(
    put,
    path = "/api/admin/storage-profiles/{id}",
    tag = "admin-storage",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = SaveStorageProfileParams,
    responses((status = 200, body = ApiResponse<StorageProfileRecord>))
)]
#[debug_handler]
pub async fn update(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<SaveStorageProfileParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:storage:update").await?;
    let profile = find_profile(&ctx, id).await?;
    assert_profile_visible(&ctx, &actor, &profile).await?;
    let tenant_id = resolve_storage_tenant(&ctx, &actor, params.tenant_id).await?;
    if profile.tenant_id != tenant_id {
        return Err(ApiError::forbidden(
            "cannot move storage profile across tenants",
        ));
    }
    validate_profile(&ctx, tenant_id, Some(id), &params).await?;
    if params.is_default.unwrap_or(false) {
        clear_default_profiles(&ctx, tenant_id).await?;
    }

    let mut active = profile.into_active_model();
    let existing_secret = active.secret_access_key.clone().unwrap();
    active.name = Set(params.name);
    active.code = Set(params.code);
    active.provider = Set(params.provider);
    active.enabled = Set(params.enabled.unwrap_or(true));
    active.is_default = Set(params.is_default.unwrap_or(false));
    active.endpoint = Set(blank_to_none(params.endpoint));
    active.region = Set(blank_to_none(params.region));
    active.access_key_id = Set(blank_to_none(params.access_key_id));
    active.secret_access_key = Set(match params.secret_access_key.as_deref() {
        Some(SECRET_MASK) => existing_secret,
        _ => blank_to_none(params.secret_access_key),
    });
    active.public_base_url = Set(blank_to_none(params.public_base_url));
    active.path_style = Set(params.path_style.unwrap_or(false));
    active.description = Set(params.description);
    let profile = active.update(&ctx.db).await?;

    Ok(responses::ok(StorageProfileRecord::from(profile)))
}

#[utoipa::path(
    delete,
    path = "/api/admin/storage-profiles/{id}",
    tag = "admin-storage",
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
    let actor = authorize(&ctx, &auth, "system:storage:delete").await?;
    let profile = find_profile(&ctx, id).await?;
    assert_profile_visible(&ctx, &actor, &profile).await?;
    let bucket_count = storage_buckets::Entity::find()
        .filter(storage_buckets::Column::StorageProfileId.eq(id))
        .count(&ctx.db)
        .await?;
    let file_count = upload_files::Entity::find()
        .filter(upload_files::Column::StorageProfileId.eq(id))
        .count(&ctx.db)
        .await?;
    if bucket_count > 0 || file_count > 0 {
        return Err(ApiError::bad_request(
            "storage profile has buckets or files",
        ));
    }
    storage_profiles::Entity::delete_by_id(id)
        .exec(&ctx.db)
        .await?;
    Ok(responses::empty())
}

#[utoipa::path(
    get,
    path = "/api/admin/storage-profiles/{id}/buckets",
    tag = "admin-storage",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<Vec<StorageBucketRecord>>))
)]
#[debug_handler]
pub async fn list_buckets(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:storage:list").await?;
    let profile = find_profile(&ctx, id).await?;
    assert_profile_visible(&ctx, &actor, &profile).await?;
    let buckets = storage_buckets::Entity::find()
        .filter(storage_buckets::Column::StorageProfileId.eq(id))
        .order_by_desc(storage_buckets::Column::IsDefault)
        .order_by_asc(storage_buckets::Column::Id)
        .all(&ctx.db)
        .await?
        .into_iter()
        .map(StorageBucketRecord::from)
        .collect::<Vec<_>>();
    Ok(responses::ok(buckets))
}

#[utoipa::path(
    post,
    path = "/api/admin/storage-profiles/{id}/buckets",
    tag = "admin-storage",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = SaveStorageBucketParams,
    responses((status = 200, body = ApiResponse<StorageBucketRecord>))
)]
#[debug_handler]
pub async fn create_bucket(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<SaveStorageBucketParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:storage:create").await?;
    let profile = find_profile(&ctx, id).await?;
    assert_profile_visible(&ctx, &actor, &profile).await?;
    let tenant_id = resolve_storage_tenant(&ctx, &actor, params.tenant_id).await?;
    if profile.tenant_id != tenant_id {
        return Err(ApiError::forbidden(
            "bucket tenant must match profile tenant",
        ));
    }
    validate_bucket(&ctx, id, tenant_id, None, &params, &profile.provider).await?;
    if params.is_default.unwrap_or(false) {
        clear_default_buckets(&ctx, tenant_id).await?;
    }

    let bucket = storage_buckets::ActiveModel {
        storage_profile_id: Set(id),
        tenant_id: Set(tenant_id),
        name: Set(params.name),
        bucket: Set(params.bucket),
        base_prefix: Set(normalized_optional_prefix(params.base_prefix.as_deref())?),
        local_root: Set(blank_to_none(params.local_root)),
        public_prefix: Set(blank_to_none(params.public_prefix)),
        enabled: Set(params.enabled.unwrap_or(true)),
        is_default: Set(params.is_default.unwrap_or(false)),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;

    Ok(responses::ok(StorageBucketRecord::from(bucket)))
}

#[utoipa::path(
    put,
    path = "/api/admin/storage-buckets/{id}",
    tag = "admin-storage",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = SaveStorageBucketParams,
    responses((status = 200, body = ApiResponse<StorageBucketRecord>))
)]
#[debug_handler]
pub async fn update_bucket(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<SaveStorageBucketParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:storage:update").await?;
    let bucket = find_bucket(&ctx, id).await?;
    assert_bucket_visible(&ctx, &actor, &bucket).await?;
    let profile = find_profile(&ctx, bucket.storage_profile_id).await?;
    let tenant_id = resolve_storage_tenant(&ctx, &actor, params.tenant_id).await?;
    if bucket.tenant_id != tenant_id || profile.tenant_id != tenant_id {
        return Err(ApiError::forbidden(
            "cannot move storage bucket across tenants",
        ));
    }
    validate_bucket(
        &ctx,
        bucket.storage_profile_id,
        tenant_id,
        Some(id),
        &params,
        &profile.provider,
    )
    .await?;
    if params.is_default.unwrap_or(false) {
        clear_default_buckets(&ctx, tenant_id).await?;
    }

    let mut active = bucket.into_active_model();
    active.name = Set(params.name);
    active.bucket = Set(params.bucket);
    active.base_prefix = Set(normalized_optional_prefix(params.base_prefix.as_deref())?);
    active.local_root = Set(blank_to_none(params.local_root));
    active.public_prefix = Set(blank_to_none(params.public_prefix));
    active.enabled = Set(params.enabled.unwrap_or(true));
    active.is_default = Set(params.is_default.unwrap_or(false));
    let bucket = active.update(&ctx.db).await?;

    Ok(responses::ok(StorageBucketRecord::from(bucket)))
}

#[utoipa::path(
    delete,
    path = "/api/admin/storage-buckets/{id}",
    tag = "admin-storage",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn delete_bucket(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:storage:delete").await?;
    let bucket = find_bucket(&ctx, id).await?;
    assert_bucket_visible(&ctx, &actor, &bucket).await?;
    let file_count = upload_files::Entity::find()
        .filter(upload_files::Column::StorageBucketId.eq(id))
        .count(&ctx.db)
        .await?;
    if file_count > 0 {
        return Err(ApiError::bad_request("storage bucket has files"));
    }
    storage_buckets::Entity::delete_by_id(id)
        .exec(&ctx.db)
        .await?;
    Ok(responses::empty())
}

#[utoipa::path(
    post,
    path = "/api/admin/storage-profiles/{id}/test",
    tag = "admin-storage",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<StorageTestRecord>))
)]
#[debug_handler]
pub async fn test(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:storage:test").await?;
    let profile = find_profile(&ctx, id).await?;
    assert_profile_visible(&ctx, &actor, &profile).await?;
    let bucket = default_bucket_for_profile(&ctx, id).await?;
    storage::list_objects(&profile, &bucket, None).await?;
    Ok(responses::ok(StorageTestRecord {
        ok: true,
        message: "storage connection is available".to_string(),
    }))
}

pub async fn resolve_default_bucket(
    ctx: &AppContext,
    actor: &users::Model,
) -> ApiResult<(storage_profiles::Model, storage_buckets::Model)> {
    let tenant_id = actor
        .tenant_id
        .ok_or_else(|| ApiError::bad_request("tenant_id is required"))?;
    let scope = rbac::resolve_data_scope(&ctx.db, actor).await?;
    ensure_tenant_allowed(&scope, tenant_id)?;
    let bucket = storage_buckets::Entity::find()
        .filter(storage_buckets::Column::TenantId.eq(tenant_id))
        .filter(storage_buckets::Column::Enabled.eq(true))
        .order_by_desc(storage_buckets::Column::IsDefault)
        .order_by_asc(storage_buckets::Column::Id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("default storage bucket not found"))?;
    let profile = find_profile(ctx, bucket.storage_profile_id).await?;
    if !profile.enabled {
        return Err(ApiError::bad_request("storage profile is disabled"));
    }
    Ok((profile, bucket))
}

pub async fn resolve_bucket(
    ctx: &AppContext,
    actor: &users::Model,
    bucket_id: Option<i32>,
) -> ApiResult<(storage_profiles::Model, storage_buckets::Model)> {
    let Some(bucket_id) = bucket_id else {
        return resolve_default_bucket(ctx, actor).await;
    };
    let bucket = find_bucket(ctx, bucket_id).await?;
    assert_bucket_visible(ctx, actor, &bucket).await?;
    if !bucket.enabled {
        return Err(ApiError::bad_request("storage bucket is disabled"));
    }
    let profile = find_profile(ctx, bucket.storage_profile_id).await?;
    if !profile.enabled {
        return Err(ApiError::bad_request("storage profile is disabled"));
    }
    Ok((profile, bucket))
}

#[must_use]
pub fn apply_profile_scope(
    query: sea_orm::Select<storage_profiles::Entity>,
    scope: &EffectiveDataScope,
) -> sea_orm::Select<storage_profiles::Entity> {
    match scope {
        EffectiveDataScope::All => query,
        EffectiveDataScope::Tenant { tenant_id }
        | EffectiveDataScope::Department { tenant_id, .. } => {
            query.filter(storage_profiles::Column::TenantId.eq(*tenant_id))
        }
        EffectiveDataScope::SelfOnly { tenant_id, .. } => match tenant_id {
            Some(tenant_id) => query.filter(storage_profiles::Column::TenantId.eq(*tenant_id)),
            None => query.filter(storage_profiles::Column::Id.eq(-1)),
        },
        EffectiveDataScope::None => query.filter(storage_profiles::Column::Id.eq(-1)),
    }
}

async fn assert_profile_visible(
    ctx: &AppContext,
    actor: &users::Model,
    profile: &storage_profiles::Model,
) -> ApiResult<()> {
    let scope = rbac::resolve_data_scope(&ctx.db, actor).await?;
    ensure_tenant_allowed(&scope, profile.tenant_id)
}

async fn assert_bucket_visible(
    ctx: &AppContext,
    actor: &users::Model,
    bucket: &storage_buckets::Model,
) -> ApiResult<()> {
    let scope = rbac::resolve_data_scope(&ctx.db, actor).await?;
    ensure_tenant_allowed(&scope, bucket.tenant_id)
}

fn ensure_tenant_allowed(scope: &EffectiveDataScope, tenant_id: i32) -> ApiResult<()> {
    match scope {
        EffectiveDataScope::All => Ok(()),
        EffectiveDataScope::Tenant {
            tenant_id: scope_tenant_id,
        }
        | EffectiveDataScope::Department {
            tenant_id: scope_tenant_id,
            ..
        } if *scope_tenant_id == tenant_id => Ok(()),
        _ => Err(ApiError::forbidden("data scope denied")),
    }
}

async fn resolve_storage_tenant(
    ctx: &AppContext,
    actor: &users::Model,
    requested_tenant_id: Option<i32>,
) -> ApiResult<i32> {
    match rbac::resolve_data_scope(&ctx.db, actor).await? {
        EffectiveDataScope::All => requested_tenant_id
            .or(actor.tenant_id)
            .ok_or_else(|| ApiError::bad_request("tenant_id is required")),
        EffectiveDataScope::Tenant { tenant_id }
        | EffectiveDataScope::Department { tenant_id, .. } => {
            if requested_tenant_id.is_some_and(|id| id != tenant_id) {
                return Err(ApiError::forbidden("cannot manage another tenant storage"));
            }
            Ok(tenant_id)
        }
        _ => Err(ApiError::forbidden("data scope denied")),
    }
}

async fn validate_profile(
    ctx: &AppContext,
    tenant_id: i32,
    profile_id: Option<i32>,
    params: &SaveStorageProfileParams,
) -> ApiResult<()> {
    if params.name.trim().is_empty() || params.code.trim().is_empty() {
        return Err(ApiError::bad_request(
            "storage profile name and code are required",
        ));
    }
    storage::ensure_provider_supported(&params.provider)?;
    tenants::Entity::find_by_id(tenant_id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("tenant not found"))?;
    let duplicate = storage_profiles::Entity::find()
        .filter(storage_profiles::Column::TenantId.eq(tenant_id))
        .filter(storage_profiles::Column::Code.eq(params.code.clone()))
        .one(&ctx.db)
        .await?;
    if duplicate.is_some_and(|profile| Some(profile.id) != profile_id) {
        return Err(ApiError::bad_request("storage profile code already exists"));
    }
    if params.provider == "s3_compatible" {
        for (value, message) in [
            (&params.endpoint, "storage endpoint is required"),
            (&params.access_key_id, "access key id is required"),
            (&params.secret_access_key, "secret access key is required"),
        ] {
            if value.as_deref().is_none_or(|value| value.trim().is_empty()) {
                return Err(ApiError::bad_request(message));
            }
        }
    }
    Ok(())
}

async fn validate_bucket(
    ctx: &AppContext,
    profile_id: i32,
    tenant_id: i32,
    bucket_id: Option<i32>,
    params: &SaveStorageBucketParams,
    provider: &str,
) -> ApiResult<()> {
    if params.name.trim().is_empty() || params.bucket.trim().is_empty() {
        return Err(ApiError::bad_request(
            "storage bucket name and bucket are required",
        ));
    }
    if provider == "local"
        && params
            .local_root
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
    {
        return Err(ApiError::bad_request("local root is required"));
    }
    let duplicate = storage_buckets::Entity::find()
        .filter(storage_buckets::Column::StorageProfileId.eq(profile_id))
        .filter(storage_buckets::Column::Bucket.eq(params.bucket.clone()))
        .one(&ctx.db)
        .await?;
    if duplicate.is_some_and(|bucket| Some(bucket.id) != bucket_id) {
        return Err(ApiError::bad_request("storage bucket already exists"));
    }
    if tenant_id <= 0 {
        return Err(ApiError::bad_request("tenant_id is required"));
    }
    Ok(())
}

async fn clear_default_profiles(ctx: &AppContext, tenant_id: i32) -> ApiResult<()> {
    let profiles = storage_profiles::Entity::find()
        .filter(storage_profiles::Column::TenantId.eq(tenant_id))
        .filter(storage_profiles::Column::IsDefault.eq(true))
        .all(&ctx.db)
        .await?;
    for profile in profiles {
        let mut active = profile.into_active_model();
        active.is_default = Set(false);
        active.update(&ctx.db).await?;
    }
    Ok(())
}

async fn clear_default_buckets(ctx: &AppContext, tenant_id: i32) -> ApiResult<()> {
    let buckets = storage_buckets::Entity::find()
        .filter(storage_buckets::Column::TenantId.eq(tenant_id))
        .filter(storage_buckets::Column::IsDefault.eq(true))
        .all(&ctx.db)
        .await?;
    for bucket in buckets {
        let mut active = bucket.into_active_model();
        active.is_default = Set(false);
        active.update(&ctx.db).await?;
    }
    Ok(())
}

async fn default_bucket_for_profile(
    ctx: &AppContext,
    profile_id: i32,
) -> ApiResult<storage_buckets::Model> {
    storage_buckets::Entity::find()
        .filter(storage_buckets::Column::StorageProfileId.eq(profile_id))
        .filter(storage_buckets::Column::Enabled.eq(true))
        .order_by_desc(storage_buckets::Column::IsDefault)
        .order_by_asc(storage_buckets::Column::Id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("storage bucket not found"))
}

pub async fn find_profile(ctx: &AppContext, id: i32) -> ApiResult<storage_profiles::Model> {
    storage_profiles::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("storage profile not found"))
}

pub async fn find_bucket(ctx: &AppContext, id: i32) -> ApiResult<storage_buckets::Model> {
    storage_buckets::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("storage bucket not found"))
}

fn normalized_optional_prefix(value: Option<&str>) -> ApiResult<Option<String>> {
    storage::normalize_prefix(value)
}

fn blank_to_none(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim().to_string();
        if value.is_empty() {
            None
        } else {
            Some(value)
        }
    })
}

impl From<storage_profiles::Model> for StorageProfileRecord {
    fn from(profile: storage_profiles::Model) -> Self {
        Self {
            id: profile.id,
            tenant_id: profile.tenant_id,
            name: profile.name,
            code: profile.code,
            provider: profile.provider,
            enabled: profile.enabled,
            is_default: profile.is_default,
            endpoint: profile.endpoint,
            region: profile.region,
            access_key_id: profile.access_key_id,
            secret_access_key: profile.secret_access_key.map(|_| SECRET_MASK.to_string()),
            public_base_url: profile.public_base_url,
            path_style: profile.path_style,
            description: profile.description,
            created_at: profile.created_at.to_rfc3339(),
            updated_at: profile.updated_at.to_rfc3339(),
        }
    }
}

impl From<storage_buckets::Model> for StorageBucketRecord {
    fn from(bucket: storage_buckets::Model) -> Self {
        Self {
            id: bucket.id,
            storage_profile_id: bucket.storage_profile_id,
            tenant_id: bucket.tenant_id,
            name: bucket.name,
            bucket: bucket.bucket,
            base_prefix: bucket.base_prefix,
            local_root: bucket.local_root,
            public_prefix: bucket.public_prefix,
            enabled: bucket.enabled,
            is_default: bucket.is_default,
            created_at: bucket.created_at.to_rfc3339(),
            updated_at: bucket.updated_at.to_rfc3339(),
        }
    }
}
