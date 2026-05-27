use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "tenants",
            &[
                ("id", ColType::PkAuto),
                ("name", ColType::String),
                ("code", ColType::StringUniq),
                ("description", ColType::TextNull),
                ("enabled", ColType::BooleanWithDefault(true)),
                ("is_system", ColType::BooleanWithDefault(false)),
            ],
            &[],
        )
        .await?;

        m.alter_table(
            Table::alter()
                .table(Users::Table)
                .add_column(ColumnDef::new(Users::TenantId).integer().null())
                .to_owned(),
        )
        .await?;
        m.alter_table(
            Table::alter()
                .table(Roles::Table)
                .add_column(ColumnDef::new(Roles::TenantId).integer().null())
                .to_owned(),
        )
        .await?;
        m.create_foreign_key(
            ForeignKey::create()
                .name("fk_users_tenant_id")
                .from(Users::Table, Users::TenantId)
                .to(Tenants::Table, Tenants::Id)
                .on_update(ForeignKeyAction::Cascade)
                .on_delete(ForeignKeyAction::SetNull)
                .to_owned(),
        )
        .await?;
        m.create_foreign_key(
            ForeignKey::create()
                .name("fk_roles_tenant_id")
                .from(Roles::Table, Roles::TenantId)
                .to(Tenants::Table, Tenants::Id)
                .on_update(ForeignKeyAction::Cascade)
                .on_delete(ForeignKeyAction::SetNull)
                .to_owned(),
        )
        .await?;

        m.create_index(
            Index::create()
                .name("idx_users_tenant_id")
                .table(Users::Table)
                .col(Users::TenantId)
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("idx_roles_tenant_id")
                .table(Roles::Table)
                .col(Roles::TenantId)
                .to_owned(),
        )
        .await?;

        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.drop_index(
            Index::drop()
                .name("idx_roles_tenant_id")
                .table(Roles::Table)
                .to_owned(),
        )
        .await?;
        m.drop_index(
            Index::drop()
                .name("idx_users_tenant_id")
                .table(Users::Table)
                .to_owned(),
        )
        .await?;
        m.alter_table(
            Table::alter()
                .table(Roles::Table)
                .drop_foreign_key(Alias::new("fk_roles_tenant_id"))
                .drop_column(Roles::TenantId)
                .to_owned(),
        )
        .await?;
        m.alter_table(
            Table::alter()
                .table(Users::Table)
                .drop_foreign_key(Alias::new("fk_users_tenant_id"))
                .drop_column(Users::TenantId)
                .to_owned(),
        )
        .await?;
        drop_table(m, "tenants").await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Tenants {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    TenantId,
}

#[derive(DeriveIden)]
enum Roles {
    Table,
    TenantId,
}
