use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.alter_table(
            Table::alter()
                .table(Tenants::Table)
                .add_column(
                    ColumnDef::new(Tenants::DepartmentsEnabled)
                        .boolean()
                        .not_null()
                        .default(false),
                )
                .to_owned(),
        )
        .await?;

        create_table(
            m,
            "departments",
            &[
                ("id", ColType::PkAuto),
                ("parent_id", ColType::IntegerNull),
                ("name", ColType::String),
                ("code", ColType::String),
                ("description", ColType::TextNull),
                ("sort_order", ColType::IntegerWithDefault(0)),
                ("enabled", ColType::BooleanWithDefault(true)),
                ("is_system", ColType::BooleanWithDefault(false)),
            ],
            &[("tenants", "tenant_id")],
        )
        .await?;

        m.create_foreign_key(
            ForeignKey::create()
                .name("fk_departments_parent_id")
                .from(Departments::Table, Departments::ParentId)
                .to(Departments::Table, Departments::Id)
                .on_update(ForeignKeyAction::Cascade)
                .on_delete(ForeignKeyAction::SetNull)
                .to_owned(),
        )
        .await?;

        create_join_table(
            m,
            "user_departments",
            &[("is_primary", ColType::BooleanWithDefault(false))],
            &[("users", ""), ("departments", "")],
        )
        .await?;

        m.alter_table(
            Table::alter()
                .table(Users::Table)
                .add_column(ColumnDef::new(Users::CurrentDepartmentId).integer().null())
                .to_owned(),
        )
        .await?;
        m.create_foreign_key(
            ForeignKey::create()
                .name("fk_users_current_department_id")
                .from(Users::Table, Users::CurrentDepartmentId)
                .to(Departments::Table, Departments::Id)
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
                .table(Users::Table)
                .drop_foreign_key(Alias::new("fk_users_current_department_id"))
                .drop_column(Users::CurrentDepartmentId)
                .to_owned(),
        )
        .await?;
        drop_table(m, "user_departments").await?;
        m.drop_foreign_key(
            ForeignKey::drop()
                .name("fk_departments_parent_id")
                .table(Departments::Table)
                .to_owned(),
        )
        .await?;
        drop_table(m, "departments").await?;
        m.alter_table(
            Table::alter()
                .table(Tenants::Table)
                .drop_column(Tenants::DepartmentsEnabled)
                .to_owned(),
        )
        .await
    }
}

async fn create_indexes(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    m.create_index(
        Index::create()
            .name("idx_departments_tenant_parent")
            .table(Departments::Table)
            .col(Departments::TenantId)
            .col(Departments::ParentId)
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_departments_tenant_code")
            .table(Departments::Table)
            .col(Departments::TenantId)
            .col(Departments::Code)
            .unique()
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_departments_enabled_sort")
            .table(Departments::Table)
            .col(Departments::Enabled)
            .col(Departments::SortOrder)
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_user_departments_department")
            .table(UserDepartments::Table)
            .col(UserDepartments::DepartmentId)
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_users_current_department_id")
            .table(Users::Table)
            .col(Users::CurrentDepartmentId)
            .to_owned(),
    )
    .await
}

async fn drop_indexes(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    for (table, name) in [
        (Users::Table.into_iden(), "idx_users_current_department_id"),
        (
            UserDepartments::Table.into_iden(),
            "idx_user_departments_department",
        ),
        (
            Departments::Table.into_iden(),
            "idx_departments_enabled_sort",
        ),
        (
            Departments::Table.into_iden(),
            "idx_departments_tenant_code",
        ),
        (
            Departments::Table.into_iden(),
            "idx_departments_tenant_parent",
        ),
    ] {
        m.drop_index(Index::drop().name(name).table(table).to_owned())
            .await?;
    }
    Ok(())
}

#[derive(DeriveIden)]
enum Tenants {
    Table,
    DepartmentsEnabled,
}

#[derive(DeriveIden)]
enum Departments {
    Table,
    Id,
    TenantId,
    ParentId,
    Code,
    Enabled,
    SortOrder,
}

#[derive(DeriveIden)]
enum UserDepartments {
    Table,
    DepartmentId,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    CurrentDepartmentId,
}
