#![allow(clippy::missing_errors_doc)]

use chrono::offset::Local;
use loco_rs::prelude::*;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, EntityTrait, IntoActiveModel, PaginatorTrait,
    QueryFilter, QueryOrder, Set,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    controllers::admin::authorize,
    errors::{ApiError, ApiResult},
    models::{
        _entities::{payment_callbacks, payment_channels, payment_orders, payment_refunds, users},
        rbac,
    },
    responses::{self, ApiResponse, EmptyData, PageResponse},
};

const SECRET_MASK: &str = "******";
const PROVIDERS: &[&str] = &[
    "yipay",
    "paypal",
    "stripe",
    "alipay",
    "wechat_pay",
    "tokenpay",
    "bepusdt",
    "epusdt",
    "okpay",
];
const PAYMENT_STATUSES: &[&str] = &[
    "pending",
    "paying",
    "paid",
    "failed",
    "cancelled",
    "expired",
    "refunding",
    "refunded",
];
const REFUND_STATUSES: &[&str] = &[
    "pending",
    "approved",
    "processing",
    "succeeded",
    "failed",
    "rejected",
];

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct PaymentChannelQueryParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub keyword: Option<String>,
    pub provider: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct PaymentOrderQueryParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub keyword: Option<String>,
    pub provider: Option<String>,
    pub status: Option<String>,
    pub channel_id: Option<i32>,
    pub merchant_order_no: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct PaymentCallbackQueryParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub provider: Option<String>,
    pub processed: Option<bool>,
    pub payment_order_id: Option<i32>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct PaymentRefundQueryParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub status: Option<String>,
    pub payment_order_id: Option<i32>,
}

impl PaymentChannelQueryParams {
    #[must_use]
    pub fn page(&self) -> u64 {
        self.page.unwrap_or(1).max(1)
    }

    #[must_use]
    pub fn page_size(&self) -> u64 {
        self.page_size.unwrap_or(10).clamp(1, 100)
    }
}

impl PaymentOrderQueryParams {
    #[must_use]
    pub fn page(&self) -> u64 {
        self.page.unwrap_or(1).max(1)
    }

    #[must_use]
    pub fn page_size(&self) -> u64 {
        self.page_size.unwrap_or(10).clamp(1, 100)
    }
}

impl PaymentCallbackQueryParams {
    #[must_use]
    pub fn page(&self) -> u64 {
        self.page.unwrap_or(1).max(1)
    }

    #[must_use]
    pub fn page_size(&self) -> u64 {
        self.page_size.unwrap_or(10).clamp(1, 100)
    }
}

impl PaymentRefundQueryParams {
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
pub struct PaymentChannelRecord {
    pub id: i32,
    pub tenant_id: Option<i32>,
    pub name: String,
    pub provider: String,
    pub channel_code: String,
    pub currency: String,
    pub config: String,
    pub secret_config: Option<String>,
    pub notify_url: Option<String>,
    pub return_url: Option<String>,
    pub enabled: bool,
    pub sort_order: i32,
    pub description: Option<String>,
    pub created_by: Option<i32>,
    pub updated_by: Option<i32>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct PaymentChannelSummary {
    pub id: i32,
    pub name: String,
    pub provider: String,
    pub channel_code: String,
    pub currency: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct PaymentOrderRecord {
    pub id: i32,
    pub tenant_id: Option<i32>,
    pub channel_id: Option<i32>,
    pub order_no: String,
    pub merchant_order_no: Option<String>,
    pub subject: String,
    pub body: Option<String>,
    pub amount: String,
    pub currency: String,
    pub provider: String,
    pub status: String,
    pub paid_at: Option<String>,
    pub expired_at: Option<String>,
    pub client_ip: Option<String>,
    pub payer_id: Option<String>,
    pub trade_no: Option<String>,
    pub metadata: Option<String>,
    pub created_by: Option<i32>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct PaymentOrderDetailRecord {
    #[serde(flatten)]
    pub order: PaymentOrderRecord,
    pub channel: Option<PaymentChannelSummary>,
    pub callbacks: Vec<PaymentCallbackRecord>,
    pub refunds: Vec<PaymentRefundRecord>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct PaymentCallbackRecord {
    pub id: i32,
    pub tenant_id: Option<i32>,
    pub payment_order_id: Option<i32>,
    pub provider: String,
    pub event_type: String,
    pub trade_no: Option<String>,
    pub payload: String,
    pub signature: Option<String>,
    pub verified: bool,
    pub processed: bool,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct PaymentRefundRecord {
    pub id: i32,
    pub tenant_id: Option<i32>,
    pub payment_order_id: i32,
    pub refund_no: String,
    pub amount: String,
    pub reason: Option<String>,
    pub status: String,
    pub provider_refund_no: Option<String>,
    pub requested_by: Option<i32>,
    pub reviewed_by: Option<i32>,
    pub reviewed_at: Option<String>,
    pub metadata: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SavePaymentChannelParams {
    pub name: String,
    pub provider: String,
    pub channel_code: String,
    pub currency: Option<String>,
    pub config: String,
    pub secret_config: Option<String>,
    pub notify_url: Option<String>,
    pub return_url: Option<String>,
    pub enabled: Option<bool>,
    pub sort_order: Option<i32>,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CreatePaymentOrderParams {
    pub channel_id: Option<i32>,
    pub merchant_order_no: Option<String>,
    pub subject: String,
    pub body: Option<String>,
    pub amount: String,
    pub currency: Option<String>,
    pub provider: Option<String>,
    pub expired_at: Option<String>,
    pub client_ip: Option<String>,
    pub payer_id: Option<String>,
    pub metadata: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct MarkPaymentPaidParams {
    pub trade_no: Option<String>,
    pub payer_id: Option<String>,
    pub payload: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CreatePaymentRefundParams {
    pub amount: String,
    pub reason: Option<String>,
    pub metadata: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/admin/payment-channels",
    tag = "admin-payments",
    security(("bearer_auth" = [])),
    params(PaymentChannelQueryParams),
    responses((status = 200, body = ApiResponse<PageResponse<PaymentChannelRecord>>))
)]
#[debug_handler]
pub async fn list_channels(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<PaymentChannelQueryParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:payment_channel:list").await?;
    let page = params.page();
    let page_size = params.page_size();
    let mut query = payment_channels::Entity::find().order_by_desc(payment_channels::Column::Id);

    if !rbac::is_super_admin(&ctx.db, actor.id).await? {
        query = query.filter(channel_visible_condition(&actor));
    }
    if let Some(keyword) = params.keyword.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(
            Condition::any()
                .add(payment_channels::Column::Name.contains(keyword))
                .add(payment_channels::Column::ChannelCode.contains(keyword)),
        );
    }
    if let Some(provider) = params.provider.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(payment_channels::Column::Provider.eq(provider));
    }
    if let Some(enabled) = params.enabled {
        query = query.filter(payment_channels::Column::Enabled.eq(enabled));
    }

    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(PaymentChannelRecord::from)
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
    path = "/api/admin/payment-channels",
    tag = "admin-payments",
    security(("bearer_auth" = [])),
    request_body = SavePaymentChannelParams,
    responses((status = 200, body = ApiResponse<PaymentChannelRecord>))
)]
#[debug_handler]
pub async fn create_channel(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SavePaymentChannelParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:payment_channel:create").await?;
    validate_channel_params(&params)?;
    let channel = payment_channels::ActiveModel {
        tenant_id: Set(actor.tenant_id),
        name: Set(params.name.trim().to_string()),
        provider: Set(params.provider),
        channel_code: Set(params.channel_code.trim().to_string()),
        currency: Set(currency_or_default(params.currency)),
        config: Set(params.config),
        secret_config: Set(normalize_secret(params.secret_config)?),
        notify_url: Set(trim_optional(params.notify_url)),
        return_url: Set(trim_optional(params.return_url)),
        enabled: Set(params.enabled.unwrap_or(true)),
        sort_order: Set(params.sort_order.unwrap_or_default()),
        description: Set(trim_optional(params.description)),
        created_by: Set(Some(actor.id)),
        updated_by: Set(Some(actor.id)),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;

    Ok(responses::ok(PaymentChannelRecord::from(channel)))
}

#[utoipa::path(
    get,
    path = "/api/admin/payment-channels/{id}",
    tag = "admin-payments",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<PaymentChannelRecord>))
)]
#[debug_handler]
pub async fn get_channel(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:payment_channel:list").await?;
    let channel = find_visible_channel(&ctx, &actor, id).await?;
    Ok(responses::ok(PaymentChannelRecord::from(channel)))
}

#[utoipa::path(
    put,
    path = "/api/admin/payment-channels/{id}",
    tag = "admin-payments",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = SavePaymentChannelParams,
    responses((status = 200, body = ApiResponse<PaymentChannelRecord>))
)]
#[debug_handler]
pub async fn update_channel(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<SavePaymentChannelParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:payment_channel:update").await?;
    validate_channel_params(&params)?;
    let channel = find_visible_channel(&ctx, &actor, id).await?;
    let secret_config = match params.secret_config {
        Some(value) if value == SECRET_MASK => channel.secret_config.clone(),
        value => normalize_secret(value)?,
    };

    let mut active = channel.into_active_model();
    active.name = Set(params.name.trim().to_string());
    active.provider = Set(params.provider);
    active.channel_code = Set(params.channel_code.trim().to_string());
    active.currency = Set(currency_or_default(params.currency));
    active.config = Set(params.config);
    active.secret_config = Set(secret_config);
    active.notify_url = Set(trim_optional(params.notify_url));
    active.return_url = Set(trim_optional(params.return_url));
    active.enabled = Set(params.enabled.unwrap_or(true));
    active.sort_order = Set(params.sort_order.unwrap_or_default());
    active.description = Set(trim_optional(params.description));
    active.updated_by = Set(Some(actor.id));
    let channel = active.update(&ctx.db).await?;

    Ok(responses::ok(PaymentChannelRecord::from(channel)))
}

#[utoipa::path(
    delete,
    path = "/api/admin/payment-channels/{id}",
    tag = "admin-payments",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn delete_channel(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:payment_channel:delete").await?;
    find_visible_channel(&ctx, &actor, id).await?;
    if payment_orders::Entity::find()
        .filter(payment_orders::Column::ChannelId.eq(id))
        .one(&ctx.db)
        .await?
        .is_some()
    {
        return Err(ApiError::bad_request("payment channel is used by orders"));
    }
    payment_channels::Entity::delete_by_id(id)
        .exec(&ctx.db)
        .await?;
    Ok(responses::empty())
}

#[utoipa::path(
    get,
    path = "/api/admin/payment-orders",
    tag = "admin-payments",
    security(("bearer_auth" = [])),
    params(PaymentOrderQueryParams),
    responses((status = 200, body = ApiResponse<PageResponse<PaymentOrderRecord>>))
)]
#[debug_handler]
pub async fn list_orders(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<PaymentOrderQueryParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:payment_order:list").await?;
    let page = params.page();
    let page_size = params.page_size();
    let mut query = payment_orders::Entity::find().order_by_desc(payment_orders::Column::Id);

    if !rbac::is_super_admin(&ctx.db, actor.id).await? {
        query = query.filter(order_visible_condition(&actor));
    }
    if let Some(keyword) = params.keyword.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(
            Condition::any()
                .add(payment_orders::Column::OrderNo.contains(keyword))
                .add(payment_orders::Column::MerchantOrderNo.contains(keyword))
                .add(payment_orders::Column::Subject.contains(keyword)),
        );
    }
    if let Some(provider) = params.provider.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(payment_orders::Column::Provider.eq(provider));
    }
    if let Some(status) = params.status.as_deref().filter(|value| !value.is_empty()) {
        validate_value(status, PAYMENT_STATUSES, "unsupported payment status")?;
        query = query.filter(payment_orders::Column::Status.eq(status));
    }
    if let Some(channel_id) = params.channel_id {
        query = query.filter(payment_orders::Column::ChannelId.eq(channel_id));
    }
    if let Some(merchant_order_no) = params
        .merchant_order_no
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        query = query.filter(payment_orders::Column::MerchantOrderNo.eq(merchant_order_no));
    }

    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(PaymentOrderRecord::from)
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
    path = "/api/admin/payment-orders",
    tag = "admin-payments",
    security(("bearer_auth" = [])),
    request_body = CreatePaymentOrderParams,
    responses((status = 200, body = ApiResponse<PaymentOrderRecord>))
)]
#[debug_handler]
pub async fn create_order(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<CreatePaymentOrderParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:payment_order:create").await?;
    validate_order_params(&params)?;
    let channel = load_channel_for_order(&ctx, &actor, params.channel_id).await?;
    let provider = channel.as_ref().map_or_else(
        || params.provider.clone().unwrap_or_default(),
        |value| value.provider.clone(),
    );
    validate_value(&provider, PROVIDERS, "unsupported payment provider")?;
    let currency = channel.as_ref().map_or_else(
        || currency_or_default(params.currency),
        |value| value.currency.clone(),
    );

    let order = payment_orders::ActiveModel {
        tenant_id: Set(actor.tenant_id),
        channel_id: Set(channel.as_ref().map(|value| value.id)),
        order_no: Set(next_no("PAY")),
        merchant_order_no: Set(trim_optional(params.merchant_order_no)),
        subject: Set(params.subject.trim().to_string()),
        body: Set(trim_optional(params.body)),
        amount: Set(normalize_amount(&params.amount)?),
        currency: Set(currency),
        provider: Set(provider),
        status: Set("pending".to_string()),
        paid_at: Set(None),
        expired_at: Set(parse_time(params.expired_at.as_deref())?),
        client_ip: Set(trim_optional(params.client_ip)),
        payer_id: Set(trim_optional(params.payer_id)),
        trade_no: Set(None),
        metadata: Set(trim_optional(params.metadata)),
        created_by: Set(Some(actor.id)),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;

    Ok(responses::ok(PaymentOrderRecord::from(order)))
}

#[utoipa::path(
    get,
    path = "/api/admin/payment-orders/{id}",
    tag = "admin-payments",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<PaymentOrderDetailRecord>))
)]
#[debug_handler]
pub async fn get_order(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:payment_order:list").await?;
    let order = find_visible_order(&ctx, &actor, id).await?;
    Ok(responses::ok(load_order_detail(&ctx, order).await?))
}

#[utoipa::path(
    post,
    path = "/api/admin/payment-orders/{id}/mark-paid",
    tag = "admin-payments",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = MarkPaymentPaidParams,
    responses((status = 200, body = ApiResponse<PaymentOrderRecord>))
)]
#[debug_handler]
pub async fn mark_order_paid(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<MarkPaymentPaidParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:payment_order:action").await?;
    let order = find_visible_order(&ctx, &actor, id).await?;
    if !matches!(order.status.as_str(), "pending" | "paying") {
        return Err(ApiError::bad_request("payment order cannot be marked paid"));
    }

    let trade_no = trim_optional(params.trade_no).unwrap_or_else(|| next_no("TRADE"));
    let mut active = order.clone().into_active_model();
    active.status = Set("paid".to_string());
    active.paid_at = Set(Some(Local::now().into()));
    active.trade_no = Set(Some(trade_no.clone()));
    if params.payer_id.is_some() {
        active.payer_id = Set(trim_optional(params.payer_id));
    }
    let updated = active.update(&ctx.db).await?;
    create_callback_record(
        &ctx,
        &updated,
        NewPaymentCallback {
            event_type: "manual_record",
            trade_no: Some(trade_no),
            payload: params.payload.unwrap_or_else(|| {
                json!({"action":"mark_paid","operator_id":actor.id}).to_string()
            }),
            verified: true,
            processed: true,
            error_message: None,
        },
    )
    .await?;

    Ok(responses::ok(PaymentOrderRecord::from(updated)))
}

#[utoipa::path(
    post,
    path = "/api/admin/payment-orders/{id}/cancel",
    tag = "admin-payments",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<PaymentOrderRecord>))
)]
#[debug_handler]
pub async fn cancel_order(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:payment_order:action").await?;
    let order = find_visible_order(&ctx, &actor, id).await?;
    if !matches!(order.status.as_str(), "pending" | "paying") {
        return Err(ApiError::bad_request("payment order cannot be cancelled"));
    }
    let mut active = order.into_active_model();
    active.status = Set("cancelled".to_string());
    let order = active.update(&ctx.db).await?;
    Ok(responses::ok(PaymentOrderRecord::from(order)))
}

#[utoipa::path(
    get,
    path = "/api/admin/payment-callbacks",
    tag = "admin-payments",
    security(("bearer_auth" = [])),
    params(PaymentCallbackQueryParams),
    responses((status = 200, body = ApiResponse<PageResponse<PaymentCallbackRecord>>))
)]
#[debug_handler]
pub async fn list_callbacks(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<PaymentCallbackQueryParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:payment_callback:list").await?;
    let page = params.page();
    let page_size = params.page_size();
    let mut query = payment_callbacks::Entity::find().order_by_desc(payment_callbacks::Column::Id);

    if !rbac::is_super_admin(&ctx.db, actor.id).await? {
        query = query.filter(callback_visible_condition(&actor));
    }
    if let Some(provider) = params.provider.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(payment_callbacks::Column::Provider.eq(provider));
    }
    if let Some(processed) = params.processed {
        query = query.filter(payment_callbacks::Column::Processed.eq(processed));
    }
    if let Some(payment_order_id) = params.payment_order_id {
        query = query.filter(payment_callbacks::Column::PaymentOrderId.eq(payment_order_id));
    }

    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(PaymentCallbackRecord::from)
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
    path = "/api/admin/payment-callbacks/{id}",
    tag = "admin-payments",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<PaymentCallbackRecord>))
)]
#[debug_handler]
pub async fn get_callback(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:payment_callback:list").await?;
    let callback = find_visible_callback(&ctx, &actor, id).await?;
    Ok(responses::ok(PaymentCallbackRecord::from(callback)))
}

#[utoipa::path(
    get,
    path = "/api/admin/payment-refunds",
    tag = "admin-payments",
    security(("bearer_auth" = [])),
    params(PaymentRefundQueryParams),
    responses((status = 200, body = ApiResponse<PageResponse<PaymentRefundRecord>>))
)]
#[debug_handler]
pub async fn list_refunds(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<PaymentRefundQueryParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:payment_refund:list").await?;
    let page = params.page();
    let page_size = params.page_size();
    let mut query = payment_refunds::Entity::find().order_by_desc(payment_refunds::Column::Id);

    if !rbac::is_super_admin(&ctx.db, actor.id).await? {
        query = query.filter(refund_visible_condition(&actor));
    }
    if let Some(status) = params.status.as_deref().filter(|value| !value.is_empty()) {
        query = query.filter(payment_refunds::Column::Status.eq(status));
    }
    if let Some(payment_order_id) = params.payment_order_id {
        query = query.filter(payment_refunds::Column::PaymentOrderId.eq(payment_order_id));
    }

    let total = query.clone().count(&ctx.db).await?;
    let items = query
        .paginate(&ctx.db, page_size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(PaymentRefundRecord::from)
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
    path = "/api/admin/payment-orders/{id}/refunds",
    tag = "admin-payments",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = CreatePaymentRefundParams,
    responses((status = 200, body = ApiResponse<PaymentRefundRecord>))
)]
#[debug_handler]
pub async fn create_refund(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<CreatePaymentRefundParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:payment_refund:create").await?;
    let order = find_visible_order(&ctx, &actor, id).await?;
    if !matches!(order.status.as_str(), "paid" | "refunding") {
        return Err(ApiError::bad_request("payment order cannot be refunded"));
    }
    let refund_amount = parse_amount(&params.amount)?;
    if refund_amount > parse_amount(&order.amount)? {
        return Err(ApiError::bad_request(
            "refund amount exceeds payment amount",
        ));
    }
    if let Some(metadata) = params.metadata.as_deref().filter(|value| !value.is_empty()) {
        validate_json("metadata", metadata)?;
    }

    let refund = payment_refunds::ActiveModel {
        tenant_id: Set(order.tenant_id),
        payment_order_id: Set(order.id),
        refund_no: Set(next_no("REF")),
        amount: Set(normalize_amount(&params.amount)?),
        reason: Set(trim_optional(params.reason)),
        status: Set("pending".to_string()),
        provider_refund_no: Set(None),
        requested_by: Set(Some(actor.id)),
        reviewed_by: Set(None),
        reviewed_at: Set(None),
        metadata: Set(trim_optional(params.metadata)),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;

    let mut active = order.into_active_model();
    active.status = Set("refunding".to_string());
    active.update(&ctx.db).await?;

    Ok(responses::ok(PaymentRefundRecord::from(refund)))
}

#[utoipa::path(
    post,
    path = "/api/admin/payment-refunds/{id}/approve",
    tag = "admin-payments",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<PaymentRefundRecord>))
)]
#[debug_handler]
pub async fn approve_refund(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    review_refund(ctx, auth, id, "approved").await
}

#[utoipa::path(
    post,
    path = "/api/admin/payment-refunds/{id}/reject",
    tag = "admin-payments",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<PaymentRefundRecord>))
)]
#[debug_handler]
pub async fn reject_refund(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    review_refund(ctx, auth, id, "rejected").await
}

#[utoipa::path(
    post,
    path = "/api/admin/payment-refunds/{id}/mark-succeeded",
    tag = "admin-payments",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<PaymentRefundRecord>))
)]
#[debug_handler]
pub async fn mark_refund_succeeded(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:payment_refund:review").await?;
    let refund = find_visible_refund(&ctx, &actor, id).await?;
    if !matches!(refund.status.as_str(), "approved" | "processing") {
        return Err(ApiError::bad_request("refund cannot be marked succeeded"));
    }
    let mut active = refund.clone().into_active_model();
    active.status = Set("succeeded".to_string());
    active.reviewed_by = Set(Some(actor.id));
    active.reviewed_at = Set(Some(Local::now().into()));
    let refund = active.update(&ctx.db).await?;

    let order = payment_orders::Entity::find_by_id(refund.payment_order_id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("payment order not found"))?;
    let mut active = order.into_active_model();
    active.status = Set("refunded".to_string());
    active.update(&ctx.db).await?;

    Ok(responses::ok(PaymentRefundRecord::from(refund)))
}

async fn review_refund(
    ctx: AppContext,
    auth: auth::JWT,
    id: i32,
    status: &str,
) -> ApiResult<Response> {
    validate_value(status, REFUND_STATUSES, "unsupported refund status")?;
    let actor = authorize(&ctx, &auth, "system:payment_refund:review").await?;
    let refund = find_visible_refund(&ctx, &actor, id).await?;
    if refund.status != "pending" {
        return Err(ApiError::bad_request("refund is not pending review"));
    }
    let mut active = refund.into_active_model();
    active.status = Set(status.to_string());
    active.reviewed_by = Set(Some(actor.id));
    active.reviewed_at = Set(Some(Local::now().into()));
    let refund = active.update(&ctx.db).await?;
    Ok(responses::ok(PaymentRefundRecord::from(refund)))
}

async fn load_order_detail(
    ctx: &AppContext,
    order: payment_orders::Model,
) -> ApiResult<PaymentOrderDetailRecord> {
    let channel = if let Some(channel_id) = order.channel_id {
        payment_channels::Entity::find_by_id(channel_id)
            .one(&ctx.db)
            .await?
            .map(PaymentChannelSummary::from)
    } else {
        None
    };
    let callbacks = load_callbacks(ctx, order.id).await?;
    let refunds = load_refunds(ctx, order.id).await?;
    Ok(PaymentOrderDetailRecord {
        order: PaymentOrderRecord::from(order),
        channel,
        callbacks,
        refunds,
    })
}

async fn load_callbacks(ctx: &AppContext, order_id: i32) -> ApiResult<Vec<PaymentCallbackRecord>> {
    Ok(payment_callbacks::Entity::find()
        .filter(payment_callbacks::Column::PaymentOrderId.eq(order_id))
        .order_by_asc(payment_callbacks::Column::Id)
        .all(&ctx.db)
        .await?
        .into_iter()
        .map(PaymentCallbackRecord::from)
        .collect())
}

async fn load_refunds(ctx: &AppContext, order_id: i32) -> ApiResult<Vec<PaymentRefundRecord>> {
    Ok(payment_refunds::Entity::find()
        .filter(payment_refunds::Column::PaymentOrderId.eq(order_id))
        .order_by_asc(payment_refunds::Column::Id)
        .all(&ctx.db)
        .await?
        .into_iter()
        .map(PaymentRefundRecord::from)
        .collect())
}

async fn load_channel_for_order(
    ctx: &AppContext,
    actor: &users::Model,
    channel_id: Option<i32>,
) -> ApiResult<Option<payment_channels::Model>> {
    if let Some(channel_id) = channel_id {
        let channel = find_visible_channel(ctx, actor, channel_id).await?;
        if !channel.enabled {
            return Err(ApiError::bad_request("payment channel is disabled"));
        }
        Ok(Some(channel))
    } else {
        Ok(None)
    }
}

async fn find_visible_channel(
    ctx: &AppContext,
    actor: &users::Model,
    id: i32,
) -> ApiResult<payment_channels::Model> {
    let channel = payment_channels::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("payment channel not found"))?;
    if rbac::is_super_admin(&ctx.db, actor.id).await? || channel_is_visible(actor, &channel) {
        Ok(channel)
    } else {
        Err(ApiError::forbidden("payment channel is not visible"))
    }
}

async fn find_visible_order(
    ctx: &AppContext,
    actor: &users::Model,
    id: i32,
) -> ApiResult<payment_orders::Model> {
    let order = payment_orders::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("payment order not found"))?;
    if rbac::is_super_admin(&ctx.db, actor.id).await? || order_is_visible(actor, &order) {
        Ok(order)
    } else {
        Err(ApiError::forbidden("payment order is not visible"))
    }
}

async fn find_visible_callback(
    ctx: &AppContext,
    actor: &users::Model,
    id: i32,
) -> ApiResult<payment_callbacks::Model> {
    let callback = payment_callbacks::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("payment callback not found"))?;
    if rbac::is_super_admin(&ctx.db, actor.id).await?
        || tenant_is_visible(actor, callback.tenant_id)
    {
        Ok(callback)
    } else {
        Err(ApiError::forbidden("payment callback is not visible"))
    }
}

async fn find_visible_refund(
    ctx: &AppContext,
    actor: &users::Model,
    id: i32,
) -> ApiResult<payment_refunds::Model> {
    let refund = payment_refunds::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("payment refund not found"))?;
    if rbac::is_super_admin(&ctx.db, actor.id).await? || tenant_is_visible(actor, refund.tenant_id)
    {
        Ok(refund)
    } else {
        Err(ApiError::forbidden("payment refund is not visible"))
    }
}

fn channel_visible_condition(actor: &users::Model) -> Condition {
    let mut condition = Condition::any().add(payment_channels::Column::TenantId.is_null());
    if let Some(tenant_id) = actor.tenant_id {
        condition = condition.add(payment_channels::Column::TenantId.eq(tenant_id));
    }
    condition
}

fn order_visible_condition(actor: &users::Model) -> Condition {
    let mut condition = Condition::any().add(payment_orders::Column::CreatedBy.eq(actor.id));
    if let Some(tenant_id) = actor.tenant_id {
        condition = condition.add(payment_orders::Column::TenantId.eq(tenant_id));
    }
    condition
}

fn callback_visible_condition(actor: &users::Model) -> Condition {
    actor.tenant_id.map_or_else(
        || Condition::all().add(payment_callbacks::Column::TenantId.is_null()),
        |tenant_id| Condition::all().add(payment_callbacks::Column::TenantId.eq(tenant_id)),
    )
}

fn refund_visible_condition(actor: &users::Model) -> Condition {
    actor.tenant_id.map_or_else(
        || Condition::all().add(payment_refunds::Column::TenantId.is_null()),
        |tenant_id| Condition::all().add(payment_refunds::Column::TenantId.eq(tenant_id)),
    )
}

fn channel_is_visible(actor: &users::Model, channel: &payment_channels::Model) -> bool {
    channel.tenant_id.is_none()
        || actor
            .tenant_id
            .is_some_and(|tenant_id| channel.tenant_id == Some(tenant_id))
}

fn order_is_visible(actor: &users::Model, order: &payment_orders::Model) -> bool {
    order.created_by == Some(actor.id) || tenant_is_visible(actor, order.tenant_id)
}

fn tenant_is_visible(actor: &users::Model, tenant_id: Option<i32>) -> bool {
    actor
        .tenant_id
        .is_some_and(|actor_tenant_id| tenant_id == Some(actor_tenant_id))
}

struct NewPaymentCallback {
    event_type: &'static str,
    trade_no: Option<String>,
    payload: String,
    verified: bool,
    processed: bool,
    error_message: Option<String>,
}

async fn create_callback_record(
    ctx: &AppContext,
    order: &payment_orders::Model,
    callback: NewPaymentCallback,
) -> ApiResult<payment_callbacks::Model> {
    Ok(payment_callbacks::ActiveModel {
        tenant_id: Set(order.tenant_id),
        payment_order_id: Set(Some(order.id)),
        provider: Set(order.provider.clone()),
        event_type: Set(callback.event_type.to_string()),
        trade_no: Set(callback.trade_no),
        payload: Set(callback.payload),
        signature: Set(None),
        verified: Set(callback.verified),
        processed: Set(callback.processed),
        error_message: Set(callback.error_message),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?)
}

fn validate_channel_params(params: &SavePaymentChannelParams) -> ApiResult<()> {
    if params.name.trim().is_empty() || params.channel_code.trim().is_empty() {
        return Err(ApiError::bad_request(
            "payment channel name and code are required",
        ));
    }
    validate_value(&params.provider, PROVIDERS, "unsupported payment provider")?;
    validate_json("config", &params.config)?;
    if let Some(secret_config) = params
        .secret_config
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        if secret_config != SECRET_MASK {
            validate_json("secret_config", secret_config)?;
        }
    }
    Ok(())
}

fn validate_order_params(params: &CreatePaymentOrderParams) -> ApiResult<()> {
    if params.subject.trim().is_empty() {
        return Err(ApiError::bad_request("payment order subject is required"));
    }
    normalize_amount(&params.amount)?;
    if params.channel_id.is_none() {
        let provider = params
            .provider
            .as_deref()
            .ok_or_else(|| ApiError::bad_request("payment provider is required"))?;
        validate_value(provider, PROVIDERS, "unsupported payment provider")?;
    }
    if let Some(metadata) = params.metadata.as_deref().filter(|value| !value.is_empty()) {
        validate_json("metadata", metadata)?;
    }
    parse_time(params.expired_at.as_deref())?;
    Ok(())
}

fn validate_json(field: &str, value: &str) -> ApiResult<()> {
    serde_json::from_str::<serde_json::Value>(value)
        .map(|_| ())
        .map_err(|_| ApiError::bad_request(format!("{field} must be valid json")))
}

fn validate_value(value: &str, supported: &[&str], message: &str) -> ApiResult<()> {
    if supported.contains(&value) {
        Ok(())
    } else {
        Err(ApiError::bad_request(message))
    }
}

fn normalize_secret(value: Option<String>) -> ApiResult<Option<String>> {
    value
        .and_then(|value| trim_optional(Some(value)))
        .map(|value| {
            validate_json("secret_config", &value)?;
            Ok(value)
        })
        .transpose()
}

fn normalize_amount(value: &str) -> ApiResult<String> {
    parse_amount(value)?;
    let trimmed = value.trim();
    if let Some((whole, fraction)) = trimmed.split_once('.') {
        Ok(format!("{whole}.{fraction:0<2}"))
    } else {
        Ok(format!("{trimmed}.00"))
    }
}

fn parse_amount(value: &str) -> ApiResult<i64> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.starts_with('-') {
        return Err(ApiError::bad_request("amount must be a positive decimal"));
    }
    let (whole, fraction) = trimmed.split_once('.').unwrap_or((trimmed, ""));
    if whole.is_empty()
        || !whole.chars().all(|value| value.is_ascii_digit())
        || !fraction.chars().all(|value| value.is_ascii_digit())
        || fraction.len() > 2
    {
        return Err(ApiError::bad_request("amount must be a positive decimal"));
    }
    let cents = whole
        .parse::<i64>()
        .map_err(|_| ApiError::bad_request("amount is too large"))?
        .checked_mul(100)
        .ok_or_else(|| ApiError::bad_request("amount is too large"))?;
    let fraction = format!("{fraction:0<2}");
    let cents = cents
        .checked_add(
            fraction[..2]
                .parse::<i64>()
                .map_err(|_| ApiError::bad_request("amount must be a positive decimal"))?,
        )
        .ok_or_else(|| ApiError::bad_request("amount is too large"))?;
    if cents == 0 {
        return Err(ApiError::bad_request("amount must be greater than zero"));
    }
    Ok(cents)
}

fn trim_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn currency_or_default(value: Option<String>) -> String {
    trim_optional(value).unwrap_or_else(|| "CNY".to_string())
}

fn parse_time(value: Option<&str>) -> ApiResult<Option<chrono::DateTime<chrono::FixedOffset>>> {
    value
        .filter(|value| !value.is_empty())
        .map(chrono::DateTime::parse_from_rfc3339)
        .transpose()
        .map_err(|_| ApiError::bad_request("time must be RFC3339"))
}

fn next_no(prefix: &str) -> String {
    format!("{prefix}{}", Local::now().format("%Y%m%d%H%M%S%3f"))
}

impl From<payment_channels::Model> for PaymentChannelRecord {
    fn from(channel: payment_channels::Model) -> Self {
        Self {
            id: channel.id,
            tenant_id: channel.tenant_id,
            name: channel.name,
            provider: channel.provider,
            channel_code: channel.channel_code,
            currency: channel.currency,
            config: channel.config,
            secret_config: channel.secret_config.map(|_| SECRET_MASK.to_string()),
            notify_url: channel.notify_url,
            return_url: channel.return_url,
            enabled: channel.enabled,
            sort_order: channel.sort_order,
            description: channel.description,
            created_by: channel.created_by,
            updated_by: channel.updated_by,
            created_at: channel.created_at.to_rfc3339(),
            updated_at: channel.updated_at.to_rfc3339(),
        }
    }
}

impl From<payment_channels::Model> for PaymentChannelSummary {
    fn from(channel: payment_channels::Model) -> Self {
        Self {
            id: channel.id,
            name: channel.name,
            provider: channel.provider,
            channel_code: channel.channel_code,
            currency: channel.currency,
        }
    }
}

impl From<payment_orders::Model> for PaymentOrderRecord {
    fn from(order: payment_orders::Model) -> Self {
        Self {
            id: order.id,
            tenant_id: order.tenant_id,
            channel_id: order.channel_id,
            order_no: order.order_no,
            merchant_order_no: order.merchant_order_no,
            subject: order.subject,
            body: order.body,
            amount: order.amount,
            currency: order.currency,
            provider: order.provider,
            status: order.status,
            paid_at: order.paid_at.map(|value| value.to_rfc3339()),
            expired_at: order.expired_at.map(|value| value.to_rfc3339()),
            client_ip: order.client_ip,
            payer_id: order.payer_id,
            trade_no: order.trade_no,
            metadata: order.metadata,
            created_by: order.created_by,
            created_at: order.created_at.to_rfc3339(),
            updated_at: order.updated_at.to_rfc3339(),
        }
    }
}

impl From<payment_callbacks::Model> for PaymentCallbackRecord {
    fn from(callback: payment_callbacks::Model) -> Self {
        Self {
            id: callback.id,
            tenant_id: callback.tenant_id,
            payment_order_id: callback.payment_order_id,
            provider: callback.provider,
            event_type: callback.event_type,
            trade_no: callback.trade_no,
            payload: callback.payload,
            signature: callback.signature,
            verified: callback.verified,
            processed: callback.processed,
            error_message: callback.error_message,
            created_at: callback.created_at.to_rfc3339(),
            updated_at: callback.updated_at.to_rfc3339(),
        }
    }
}

impl From<payment_refunds::Model> for PaymentRefundRecord {
    fn from(refund: payment_refunds::Model) -> Self {
        Self {
            id: refund.id,
            tenant_id: refund.tenant_id,
            payment_order_id: refund.payment_order_id,
            refund_no: refund.refund_no,
            amount: refund.amount,
            reason: refund.reason,
            status: refund.status,
            provider_refund_no: refund.provider_refund_no,
            requested_by: refund.requested_by,
            reviewed_by: refund.reviewed_by,
            reviewed_at: refund.reviewed_at.map(|value| value.to_rfc3339()),
            metadata: refund.metadata,
            created_at: refund.created_at.to_rfc3339(),
            updated_at: refund.updated_at.to_rfc3339(),
        }
    }
}
