use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "roles",
            &[
                ("id", ColType::PkAuto),
                ("name", ColType::String),
                ("code", ColType::StringUniq),
                ("description", ColType::TextNull),
                ("is_system", ColType::BooleanWithDefault(false)),
                ("enabled", ColType::BooleanWithDefault(true)),
            ],
            &[],
        )
        .await?;

        create_table(
            m,
            "permissions",
            &[
                ("id", ColType::PkAuto),
                ("name", ColType::String),
                ("code", ColType::StringUniq),
                ("group_name", ColType::String),
                ("description", ColType::TextNull),
            ],
            &[],
        )
        .await?;

        create_table(
            m,
            "menus",
            &[
                ("id", ColType::PkAuto),
                ("parent_id", ColType::IntegerNull),
                ("title", ColType::String),
                ("path", ColType::StringNull),
                ("icon", ColType::StringNull),
                ("permission_code", ColType::StringNull),
                ("sort_order", ColType::IntegerWithDefault(0)),
                ("visible", ColType::BooleanWithDefault(true)),
                ("enabled", ColType::BooleanWithDefault(true)),
            ],
            &[],
        )
        .await?;

        create_table(
            m,
            "data_scopes",
            &[
                ("id", ColType::PkAuto),
                ("name", ColType::String),
                ("code", ColType::StringUniq),
                ("rule", ColType::TextNull),
                ("description", ColType::TextNull),
            ],
            &[],
        )
        .await?;

        create_join_table(m, "user_roles", &[], &[("users", ""), ("roles", "")]).await?;
        create_join_table(
            m,
            "role_permissions",
            &[],
            &[("roles", ""), ("permissions", "")],
        )
        .await?;
        create_join_table(
            m,
            "role_menus",
            &[
                ("can_create", ColType::BooleanWithDefault(false)),
                ("can_update", ColType::BooleanWithDefault(false)),
                ("can_delete", ColType::BooleanWithDefault(false)),
                ("can_import", ColType::BooleanWithDefault(false)),
                ("can_export", ColType::BooleanWithDefault(false)),
                ("can_print", ColType::BooleanWithDefault(false)),
                ("can_help", ColType::BooleanWithDefault(false)),
            ],
            &[("roles", ""), ("menus", "")],
        )
        .await?;
        create_join_table(
            m,
            "role_data_scopes",
            &[],
            &[("roles", ""), ("data_scopes", "")],
        )
        .await?;

        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        drop_table(m, "role_data_scopes").await?;
        drop_table(m, "role_menus").await?;
        drop_table(m, "role_permissions").await?;
        drop_table(m, "user_roles").await?;
        drop_table(m, "data_scopes").await?;
        drop_table(m, "menus").await?;
        drop_table(m, "permissions").await?;
        drop_table(m, "roles").await?;
        Ok(())
    }
}
