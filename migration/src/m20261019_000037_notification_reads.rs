use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// Per-user read state for the header notification bell; the old
/// `system_notifications.read_at` is a single flag shared by everyone.
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            "notification_reads",
            &[("id", ColType::PkAuto)],
            &[
                ("system_notifications", "notification_id"),
                ("users", "user_id"),
            ],
        )
        .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_notification_reads_notification_user")
                    .table(Alias::new("notification_reads"))
                    .col(Alias::new("notification_id"))
                    .col(Alias::new("user_id"))
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_table(manager, "notification_reads").await
    }
}
