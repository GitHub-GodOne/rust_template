use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_work_order_tables(m).await?;
        create_work_order_indexes(m).await?;
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        drop_work_order_indexes(m).await?;
        drop_table(m, "work_order_attachments").await?;
        drop_table(m, "work_order_assignments").await?;
        drop_table(m, "work_order_comments").await?;
        drop_table(m, "work_orders").await?;
        Ok(())
    }
}

async fn create_work_order_tables(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    create_table(
        m,
        "work_orders",
        &[
            ("id", ColType::PkAuto),
            ("order_no", ColType::StringUniq),
            ("title", ColType::String),
            ("description", ColType::Text),
            ("category", ColType::StringNull),
            ("priority", ColType::String),
            ("status", ColType::String),
            ("source", ColType::String),
            ("assigned_at", ColType::TimestampWithTimeZoneNull),
            ("resolved_at", ColType::TimestampWithTimeZoneNull),
            ("closed_at", ColType::TimestampWithTimeZoneNull),
            ("due_at", ColType::TimestampWithTimeZoneNull),
            ("last_comment_at", ColType::TimestampWithTimeZoneNull),
            ("metadata", ColType::TextNull),
        ],
        &[
            ("tenants?", "tenant_id"),
            ("users?", "creator_id"),
            ("users?", "assignee_id"),
        ],
    )
    .await?;

    create_table(
        m,
        "work_order_comments",
        &[
            ("id", ColType::PkAuto),
            ("body", ColType::Text),
            ("comment_type", ColType::String),
            ("from_status", ColType::StringNull),
            ("to_status", ColType::StringNull),
            ("metadata", ColType::TextNull),
        ],
        &[
            ("tenants?", "tenant_id"),
            ("work_orders", "work_order_id"),
            ("users?", "author_id"),
        ],
    )
    .await?;

    create_table(
        m,
        "work_order_assignments",
        &[("id", ColType::PkAuto), ("note", ColType::TextNull)],
        &[
            ("tenants?", "tenant_id"),
            ("work_orders", "work_order_id"),
            ("users", "assignee_id"),
            ("users?", "assigned_by_id"),
        ],
    )
    .await?;

    create_table(
        m,
        "work_order_attachments",
        &[("id", ColType::PkAuto), ("description", ColType::TextNull)],
        &[
            ("tenants?", "tenant_id"),
            ("work_orders", "work_order_id"),
            ("upload_files", "upload_file_id"),
            ("users?", "uploaded_by_id"),
        ],
    )
    .await
}

async fn create_work_order_indexes(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    for (name, table, columns) in [
        (
            "idx_work_orders_tenant_status",
            WorkOrders::Table,
            vec![WorkOrders::TenantId, WorkOrders::Status],
        ),
        (
            "idx_work_orders_tenant_assignee",
            WorkOrders::Table,
            vec![WorkOrders::TenantId, WorkOrders::AssigneeId],
        ),
        (
            "idx_work_orders_tenant_creator",
            WorkOrders::Table,
            vec![WorkOrders::TenantId, WorkOrders::CreatorId],
        ),
        (
            "idx_work_orders_tenant_priority",
            WorkOrders::Table,
            vec![WorkOrders::TenantId, WorkOrders::Priority],
        ),
        (
            "idx_work_orders_created_at",
            WorkOrders::Table,
            vec![WorkOrders::CreatedAt],
        ),
    ] {
        let mut index = Index::create();
        index.name(name).table(table);
        for column in columns {
            index.col(column);
        }
        m.create_index(index.clone()).await?;
    }

    m.create_index(
        Index::create()
            .name("idx_work_order_comments_order_created")
            .table(WorkOrderComments::Table)
            .col(WorkOrderComments::WorkOrderId)
            .col(WorkOrderComments::CreatedAt)
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_work_order_assignments_order_created")
            .table(WorkOrderAssignments::Table)
            .col(WorkOrderAssignments::WorkOrderId)
            .col(WorkOrderAssignments::CreatedAt)
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_work_order_assignments_assignee")
            .table(WorkOrderAssignments::Table)
            .col(WorkOrderAssignments::AssigneeId)
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_work_order_attachments_order")
            .table(WorkOrderAttachments::Table)
            .col(WorkOrderAttachments::WorkOrderId)
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_work_order_attachments_unique_file")
            .table(WorkOrderAttachments::Table)
            .col(WorkOrderAttachments::WorkOrderId)
            .col(WorkOrderAttachments::UploadFileId)
            .unique()
            .to_owned(),
    )
    .await
}

async fn drop_work_order_indexes(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    m.drop_index(
        Index::drop()
            .name("idx_work_order_attachments_unique_file")
            .table(WorkOrderAttachments::Table)
            .to_owned(),
    )
    .await?;
    m.drop_index(
        Index::drop()
            .name("idx_work_order_attachments_order")
            .table(WorkOrderAttachments::Table)
            .to_owned(),
    )
    .await?;
    m.drop_index(
        Index::drop()
            .name("idx_work_order_assignments_assignee")
            .table(WorkOrderAssignments::Table)
            .to_owned(),
    )
    .await?;
    m.drop_index(
        Index::drop()
            .name("idx_work_order_assignments_order_created")
            .table(WorkOrderAssignments::Table)
            .to_owned(),
    )
    .await?;
    m.drop_index(
        Index::drop()
            .name("idx_work_order_comments_order_created")
            .table(WorkOrderComments::Table)
            .to_owned(),
    )
    .await?;

    for name in [
        "idx_work_orders_created_at",
        "idx_work_orders_tenant_priority",
        "idx_work_orders_tenant_creator",
        "idx_work_orders_tenant_assignee",
        "idx_work_orders_tenant_status",
    ] {
        m.drop_index(Index::drop().name(name).table(WorkOrders::Table).to_owned())
            .await?;
    }
    Ok(())
}

#[derive(Copy, Clone, DeriveIden)]
enum WorkOrders {
    Table,
    TenantId,
    CreatorId,
    AssigneeId,
    Status,
    Priority,
    CreatedAt,
}

#[derive(Copy, Clone, DeriveIden)]
enum WorkOrderComments {
    Table,
    WorkOrderId,
    CreatedAt,
}

#[derive(Copy, Clone, DeriveIden)]
enum WorkOrderAssignments {
    Table,
    WorkOrderId,
    AssigneeId,
    CreatedAt,
}

#[derive(Copy, Clone, DeriveIden)]
enum WorkOrderAttachments {
    Table,
    WorkOrderId,
    UploadFileId,
}
