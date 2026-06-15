use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "storage_profiles",
            &[
                ("id", ColType::PkAuto),
                ("name", ColType::String),
                ("code", ColType::String),
                ("provider", ColType::String),
                ("enabled", ColType::BooleanWithDefault(true)),
                ("is_default", ColType::BooleanWithDefault(false)),
                ("endpoint", ColType::StringNull),
                ("region", ColType::StringNull),
                ("access_key_id", ColType::StringNull),
                ("secret_access_key", ColType::TextNull),
                ("public_base_url", ColType::StringNull),
                ("path_style", ColType::BooleanWithDefault(false)),
                ("description", ColType::TextNull),
            ],
            &[("tenants", "tenant_id")],
        )
        .await?;

        create_table(
            m,
            "storage_buckets",
            &[
                ("id", ColType::PkAuto),
                ("name", ColType::String),
                ("bucket", ColType::String),
                ("base_prefix", ColType::StringNull),
                ("local_root", ColType::StringNull),
                ("public_prefix", ColType::StringNull),
                ("enabled", ColType::BooleanWithDefault(true)),
                ("is_default", ColType::BooleanWithDefault(false)),
            ],
            &[
                ("storage_profiles", "storage_profile_id"),
                ("tenants", "tenant_id"),
            ],
        )
        .await?;

        m.alter_table(
            Table::alter()
                .table(UploadFiles::Table)
                .add_column(
                    ColumnDef::new(UploadFiles::StorageProfileId)
                        .integer()
                        .null(),
                )
                .add_column(
                    ColumnDef::new(UploadFiles::StorageBucketId)
                        .integer()
                        .null(),
                )
                .add_column(ColumnDef::new(UploadFiles::Bucket).string().null())
                .add_column(ColumnDef::new(UploadFiles::Prefix).string().null())
                .add_column(ColumnDef::new(UploadFiles::Etag).string().null())
                .to_owned(),
        )
        .await?;

        m.create_foreign_key(
            ForeignKey::create()
                .name("fk_upload_files_storage_profile_id")
                .from(UploadFiles::Table, UploadFiles::StorageProfileId)
                .to(StorageProfiles::Table, StorageProfiles::Id)
                .on_update(ForeignKeyAction::Cascade)
                .on_delete(ForeignKeyAction::SetNull)
                .to_owned(),
        )
        .await?;
        m.create_foreign_key(
            ForeignKey::create()
                .name("fk_upload_files_storage_bucket_id")
                .from(UploadFiles::Table, UploadFiles::StorageBucketId)
                .to(StorageBuckets::Table, StorageBuckets::Id)
                .on_update(ForeignKeyAction::Cascade)
                .on_delete(ForeignKeyAction::SetNull)
                .to_owned(),
        )
        .await?;

        create_indexes(m).await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        drop_indexes(m).await?;
        m.alter_table(
            Table::alter()
                .table(UploadFiles::Table)
                .drop_foreign_key(Alias::new("fk_upload_files_storage_bucket_id"))
                .drop_foreign_key(Alias::new("fk_upload_files_storage_profile_id"))
                .drop_column(UploadFiles::Etag)
                .drop_column(UploadFiles::Prefix)
                .drop_column(UploadFiles::Bucket)
                .drop_column(UploadFiles::StorageBucketId)
                .drop_column(UploadFiles::StorageProfileId)
                .to_owned(),
        )
        .await?;
        drop_table(m, "storage_buckets").await?;
        drop_table(m, "storage_profiles").await
    }
}

async fn create_indexes(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    m.create_index(
        Index::create()
            .name("idx_storage_profiles_tenant_code")
            .table(StorageProfiles::Table)
            .col(StorageProfiles::TenantId)
            .col(StorageProfiles::Code)
            .unique()
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_storage_profiles_tenant_default")
            .table(StorageProfiles::Table)
            .col(StorageProfiles::TenantId)
            .col(StorageProfiles::IsDefault)
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_storage_buckets_profile_bucket")
            .table(StorageBuckets::Table)
            .col(StorageBuckets::StorageProfileId)
            .col(StorageBuckets::Bucket)
            .unique()
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_storage_buckets_tenant_default")
            .table(StorageBuckets::Table)
            .col(StorageBuckets::TenantId)
            .col(StorageBuckets::IsDefault)
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_upload_files_storage_bucket_prefix")
            .table(UploadFiles::Table)
            .col(UploadFiles::StorageBucketId)
            .col(UploadFiles::Prefix)
            .to_owned(),
    )
    .await
}

async fn drop_indexes(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    for (table, name) in [
        (
            UploadFiles::Table.into_iden(),
            "idx_upload_files_storage_bucket_prefix",
        ),
        (
            StorageBuckets::Table.into_iden(),
            "idx_storage_buckets_tenant_default",
        ),
        (
            StorageBuckets::Table.into_iden(),
            "idx_storage_buckets_profile_bucket",
        ),
        (
            StorageProfiles::Table.into_iden(),
            "idx_storage_profiles_tenant_default",
        ),
        (
            StorageProfiles::Table.into_iden(),
            "idx_storage_profiles_tenant_code",
        ),
    ] {
        m.drop_index(Index::drop().name(name).table(table).to_owned())
            .await?;
    }
    Ok(())
}

#[derive(DeriveIden)]
enum StorageProfiles {
    Table,
    Id,
    TenantId,
    Code,
    IsDefault,
}

#[derive(DeriveIden)]
enum StorageBuckets {
    Table,
    Id,
    StorageProfileId,
    TenantId,
    Bucket,
    IsDefault,
}

#[derive(DeriveIden)]
enum UploadFiles {
    Table,
    StorageProfileId,
    StorageBucketId,
    Bucket,
    Prefix,
    Etag,
}
