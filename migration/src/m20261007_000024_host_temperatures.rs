use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            "host_temperatures",
            &[
                ("id", ColType::PkAuto),
                ("hostname", ColType::String),
                ("captured_at", ColType::TimestampWithTimeZone),
                ("max_temperature_celsius", ColType::Float),
                ("peak_sensor", ColType::String),
                ("sensor_count", ColType::Integer),
                ("sensors_json", ColType::Text),
            ],
            &[],
        )
        .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_host_temperatures_captured_at")
                    .table(HostTemperatures::Table)
                    .col(HostTemperatures::CapturedAt)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_table(manager, "host_temperatures").await
    }
}

#[derive(DeriveIden)]
enum HostTemperatures {
    Table,
    CapturedAt,
}
