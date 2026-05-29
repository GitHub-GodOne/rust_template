use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_payment_tables(m).await?;
        create_payment_indexes(m).await?;
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        drop_payment_indexes(m).await?;
        drop_table(m, "payment_callbacks").await?;
        drop_table(m, "payment_refunds").await?;
        drop_table(m, "payment_orders").await?;
        drop_table(m, "payment_channels").await?;
        Ok(())
    }
}

async fn create_payment_tables(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    create_table(
        m,
        "payment_channels",
        &[
            ("id", ColType::PkAuto),
            ("name", ColType::String),
            ("provider", ColType::String),
            ("channel_code", ColType::StringUniq),
            ("currency", ColType::String),
            ("config", ColType::Text),
            ("secret_config", ColType::TextNull),
            ("notify_url", ColType::StringNull),
            ("return_url", ColType::StringNull),
            ("enabled", ColType::BooleanWithDefault(true)),
            ("sort_order", ColType::IntegerWithDefault(0)),
            ("description", ColType::TextNull),
        ],
        &[
            ("tenants?", "tenant_id"),
            ("users?", "created_by"),
            ("users?", "updated_by"),
        ],
    )
    .await?;

    create_table(
        m,
        "payment_orders",
        &[
            ("id", ColType::PkAuto),
            ("order_no", ColType::StringUniq),
            ("merchant_order_no", ColType::StringNull),
            ("subject", ColType::String),
            ("body", ColType::TextNull),
            ("amount", ColType::String),
            ("currency", ColType::String),
            ("provider", ColType::String),
            ("status", ColType::String),
            ("paid_at", ColType::TimestampWithTimeZoneNull),
            ("expired_at", ColType::TimestampWithTimeZoneNull),
            ("client_ip", ColType::StringNull),
            ("payer_id", ColType::StringNull),
            ("trade_no", ColType::StringNull),
            ("metadata", ColType::TextNull),
        ],
        &[
            ("tenants?", "tenant_id"),
            ("payment_channels?", "channel_id"),
            ("users?", "created_by"),
        ],
    )
    .await?;

    create_table(
        m,
        "payment_callbacks",
        &[
            ("id", ColType::PkAuto),
            ("provider", ColType::String),
            ("event_type", ColType::String),
            ("trade_no", ColType::StringNull),
            ("payload", ColType::Text),
            ("signature", ColType::TextNull),
            ("verified", ColType::BooleanWithDefault(false)),
            ("processed", ColType::BooleanWithDefault(false)),
            ("error_message", ColType::TextNull),
        ],
        &[
            ("tenants?", "tenant_id"),
            ("payment_orders?", "payment_order_id"),
        ],
    )
    .await?;

    create_table(
        m,
        "payment_refunds",
        &[
            ("id", ColType::PkAuto),
            ("refund_no", ColType::StringUniq),
            ("amount", ColType::String),
            ("reason", ColType::TextNull),
            ("status", ColType::String),
            ("provider_refund_no", ColType::StringNull),
            ("reviewed_at", ColType::TimestampWithTimeZoneNull),
            ("metadata", ColType::TextNull),
        ],
        &[
            ("tenants?", "tenant_id"),
            ("payment_orders", "payment_order_id"),
            ("users?", "requested_by"),
            ("users?", "reviewed_by"),
        ],
    )
    .await
}

async fn create_payment_indexes(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    create_payment_channel_indexes(m).await?;
    create_payment_order_indexes(m).await?;
    create_payment_callback_indexes(m).await?;
    create_payment_refund_indexes(m).await
}

async fn create_payment_channel_indexes(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    m.create_index(
        Index::create()
            .name("idx_payment_channels_tenant_provider")
            .table(PaymentChannels::Table)
            .col(PaymentChannels::TenantId)
            .col(PaymentChannels::Provider)
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_payment_channels_enabled")
            .table(PaymentChannels::Table)
            .col(PaymentChannels::Enabled)
            .to_owned(),
    )
    .await
}

async fn create_payment_order_indexes(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    m.create_index(
        Index::create()
            .name("idx_payment_orders_tenant_status")
            .table(PaymentOrders::Table)
            .col(PaymentOrders::TenantId)
            .col(PaymentOrders::Status)
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_payment_orders_tenant_provider")
            .table(PaymentOrders::Table)
            .col(PaymentOrders::TenantId)
            .col(PaymentOrders::Provider)
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_payment_orders_channel")
            .table(PaymentOrders::Table)
            .col(PaymentOrders::ChannelId)
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_payment_orders_merchant_order_no")
            .table(PaymentOrders::Table)
            .col(PaymentOrders::MerchantOrderNo)
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_payment_orders_created_at")
            .table(PaymentOrders::Table)
            .col(PaymentOrders::CreatedAt)
            .to_owned(),
    )
    .await
}

async fn create_payment_callback_indexes(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    m.create_index(
        Index::create()
            .name("idx_payment_callbacks_order_created")
            .table(PaymentCallbacks::Table)
            .col(PaymentCallbacks::PaymentOrderId)
            .col(PaymentCallbacks::CreatedAt)
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_payment_callbacks_provider_created")
            .table(PaymentCallbacks::Table)
            .col(PaymentCallbacks::Provider)
            .col(PaymentCallbacks::CreatedAt)
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_payment_callbacks_processed")
            .table(PaymentCallbacks::Table)
            .col(PaymentCallbacks::Processed)
            .to_owned(),
    )
    .await
}

async fn create_payment_refund_indexes(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    m.create_index(
        Index::create()
            .name("idx_payment_refunds_order_created")
            .table(PaymentRefunds::Table)
            .col(PaymentRefunds::PaymentOrderId)
            .col(PaymentRefunds::CreatedAt)
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_payment_refunds_tenant_status")
            .table(PaymentRefunds::Table)
            .col(PaymentRefunds::TenantId)
            .col(PaymentRefunds::Status)
            .to_owned(),
    )
    .await
}

async fn drop_payment_indexes(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    m.drop_index(
        Index::drop()
            .name("idx_payment_refunds_tenant_status")
            .table(PaymentRefunds::Table)
            .to_owned(),
    )
    .await?;
    m.drop_index(
        Index::drop()
            .name("idx_payment_refunds_order_created")
            .table(PaymentRefunds::Table)
            .to_owned(),
    )
    .await?;
    m.drop_index(
        Index::drop()
            .name("idx_payment_callbacks_processed")
            .table(PaymentCallbacks::Table)
            .to_owned(),
    )
    .await?;
    m.drop_index(
        Index::drop()
            .name("idx_payment_callbacks_provider_created")
            .table(PaymentCallbacks::Table)
            .to_owned(),
    )
    .await?;
    m.drop_index(
        Index::drop()
            .name("idx_payment_callbacks_order_created")
            .table(PaymentCallbacks::Table)
            .to_owned(),
    )
    .await?;
    m.drop_index(
        Index::drop()
            .name("idx_payment_orders_created_at")
            .table(PaymentOrders::Table)
            .to_owned(),
    )
    .await?;
    m.drop_index(
        Index::drop()
            .name("idx_payment_orders_merchant_order_no")
            .table(PaymentOrders::Table)
            .to_owned(),
    )
    .await?;
    m.drop_index(
        Index::drop()
            .name("idx_payment_orders_channel")
            .table(PaymentOrders::Table)
            .to_owned(),
    )
    .await?;
    m.drop_index(
        Index::drop()
            .name("idx_payment_orders_tenant_provider")
            .table(PaymentOrders::Table)
            .to_owned(),
    )
    .await?;
    m.drop_index(
        Index::drop()
            .name("idx_payment_orders_tenant_status")
            .table(PaymentOrders::Table)
            .to_owned(),
    )
    .await?;
    m.drop_index(
        Index::drop()
            .name("idx_payment_channels_enabled")
            .table(PaymentChannels::Table)
            .to_owned(),
    )
    .await?;
    m.drop_index(
        Index::drop()
            .name("idx_payment_channels_tenant_provider")
            .table(PaymentChannels::Table)
            .to_owned(),
    )
    .await?;
    Ok(())
}

#[derive(Copy, Clone, DeriveIden)]
enum PaymentChannels {
    Table,
    TenantId,
    Provider,
    Enabled,
}

#[derive(Copy, Clone, DeriveIden)]
enum PaymentOrders {
    Table,
    TenantId,
    ChannelId,
    Provider,
    Status,
    MerchantOrderNo,
    CreatedAt,
}

#[derive(Copy, Clone, DeriveIden)]
enum PaymentCallbacks {
    Table,
    PaymentOrderId,
    Provider,
    Processed,
    CreatedAt,
}

#[derive(Copy, Clone, DeriveIden)]
enum PaymentRefunds {
    Table,
    TenantId,
    PaymentOrderId,
    Status,
    CreatedAt,
}
