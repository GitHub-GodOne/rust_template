use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_notification_table(m).await?;
        create_scheduled_task_tables(m).await?;
        create_backup_table(m).await?;
        create_rate_limit_tables(m).await?;
        create_operations_indexes(m).await?;
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.drop_index(
            Index::drop()
                .name("idx_rate_limit_events_occurred_at")
                .table(RateLimitEvents::Table)
                .to_owned(),
        )
        .await?;
        m.drop_index(
            Index::drop()
                .name("idx_scheduled_task_runs_task_id")
                .table(ScheduledTaskRuns::Table)
                .to_owned(),
        )
        .await?;
        m.drop_index(
            Index::drop()
                .name("idx_system_notifications_target")
                .table(SystemNotifications::Table)
                .to_owned(),
        )
        .await?;
        drop_table(m, "rate_limit_events").await?;
        drop_table(m, "rate_limit_rules").await?;
        drop_table(m, "database_backups").await?;
        drop_table(m, "scheduled_task_runs").await?;
        drop_table(m, "scheduled_tasks").await?;
        drop_table(m, "system_notifications").await?;
        Ok(())
    }
}

async fn create_notification_table(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    create_table(
        m,
        "system_notifications",
        &[
            ("id", ColType::PkAuto),
            ("title", ColType::String),
            ("content", ColType::Text),
            ("level", ColType::String),
            ("category", ColType::String),
            ("target_type", ColType::String),
            ("read_at", ColType::TimestampWithTimeZoneNull),
        ],
        &[
            ("users?", "target_user_id"),
            ("tenants?", "tenant_id"),
            ("users?", "created_by"),
        ],
    )
    .await
}

async fn create_scheduled_task_tables(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    create_table(
        m,
        "scheduled_tasks",
        &[
            ("id", ColType::PkAuto),
            ("name", ColType::String),
            ("code", ColType::StringUniq),
            ("task_type", ColType::String),
            ("cron_expr", ColType::String),
            ("payload", ColType::TextNull),
            ("enabled", ColType::BooleanWithDefault(true)),
            ("status", ColType::String),
            ("last_run_at", ColType::TimestampWithTimeZoneNull),
            ("next_run_at", ColType::TimestampWithTimeZoneNull),
        ],
        &[("users?", "created_by"), ("users?", "updated_by")],
    )
    .await?;

    create_table(
        m,
        "scheduled_task_runs",
        &[
            ("id", ColType::PkAuto),
            ("code", ColType::String),
            ("status", ColType::String),
            ("started_at", ColType::TimestampWithTimeZone),
            ("finished_at", ColType::TimestampWithTimeZoneNull),
            ("duration_ms", ColType::IntegerNull),
            ("output", ColType::TextNull),
            ("error_message", ColType::TextNull),
            ("triggered_by", ColType::String),
        ],
        &[("scheduled_tasks", "task_id")],
    )
    .await
}

async fn create_backup_table(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    create_table(
        m,
        "database_backups",
        &[
            ("id", ColType::PkAuto),
            ("filename", ColType::String),
            ("storage_path", ColType::String),
            ("size_bytes", ColType::BigInteger),
            ("sha256", ColType::StringNull),
            ("status", ColType::String),
            ("trigger_type", ColType::String),
            ("started_at", ColType::TimestampWithTimeZone),
            ("finished_at", ColType::TimestampWithTimeZoneNull),
            ("duration_ms", ColType::IntegerNull),
            ("delivery_targets", ColType::TextNull),
            ("delivery_status", ColType::TextNull),
            ("error_message", ColType::TextNull),
        ],
        &[("users?", "created_by")],
    )
    .await
}

async fn create_rate_limit_tables(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    create_table(
        m,
        "rate_limit_rules",
        &[
            ("id", ColType::PkAuto),
            ("name", ColType::String),
            ("scope", ColType::String),
            ("path_pattern", ColType::String),
            ("method", ColType::StringNull),
            ("limit_count", ColType::Integer),
            ("window_seconds", ColType::Integer),
            ("enabled", ColType::BooleanWithDefault(true)),
            ("description", ColType::TextNull),
        ],
        &[],
    )
    .await?;

    create_table(
        m,
        "rate_limit_events",
        &[
            ("id", ColType::PkAuto),
            ("ip", ColType::String),
            ("method", ColType::String),
            ("path", ColType::String),
            ("occurred_at", ColType::TimestampWithTimeZone),
        ],
        &[("rate_limit_rules?", "rule_id"), ("users?", "user_id")],
    )
    .await
}

async fn create_operations_indexes(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    m.create_index(
        Index::create()
            .name("idx_system_notifications_target")
            .table(SystemNotifications::Table)
            .col(SystemNotifications::TargetType)
            .col(SystemNotifications::TargetUserId)
            .col(SystemNotifications::TenantId)
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_scheduled_task_runs_task_id")
            .table(ScheduledTaskRuns::Table)
            .col(ScheduledTaskRuns::TaskId)
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_rate_limit_events_occurred_at")
            .table(RateLimitEvents::Table)
            .col(RateLimitEvents::OccurredAt)
            .to_owned(),
    )
    .await
}

#[derive(DeriveIden)]
enum SystemNotifications {
    Table,
    TargetType,
    TargetUserId,
    TenantId,
}

#[derive(DeriveIden)]
enum ScheduledTaskRuns {
    Table,
    TaskId,
}

#[derive(DeriveIden)]
enum RateLimitEvents {
    Table,
    OccurredAt,
}
