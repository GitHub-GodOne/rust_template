use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

use crate::data::{count, exists, insert_with_fixture_id};

#[derive(DeriveMigrationName)]
pub struct Migration;

struct Permission {
    id: i32,
    code: &'static str,
    name: &'static str,
    group: &'static str,
    description: &'static str,
}

const MENU_ID: i32 = 46;
const MENU_PATH: &str = "/admin/system/recycle-bin";
const SYSTEM_MENU_ID: i32 = 2;
const SETTING_ID: i32 = 43;
const SETTING_KEY: &str = "security.recycle_bin_retention_days";

/// Kept in sync with `src/fixtures/permissions.yaml`.
const PERMISSIONS: &[Permission] = &[
    Permission {
        id: 173,
        code: "system:work_order:export",
        name: "导出工单",
        group: "system:work_order",
        description: "按筛选条件导出工单列表为 CSV 文件",
    },
    Permission {
        id: 174,
        code: "system:payment_order:export",
        name: "导出支付订单",
        group: "system:payment_order",
        description: "按筛选条件导出支付订单为 CSV 文件",
    },
    Permission {
        id: 175,
        code: "system:recycle:list",
        name: "查看回收站",
        group: "system:recycle",
        description: "查看被删除的数据及其删除人、删除时间",
    },
    Permission {
        id: 176,
        code: "system:recycle:restore",
        name: "恢复已删除数据",
        group: "system:recycle",
        description: "把回收站中的数据恢复到删除前的状态",
    },
    Permission {
        id: 177,
        code: "system:recycle:purge",
        name: "彻底删除",
        group: "system:recycle",
        description: "从回收站永久删除数据，无法再恢复",
    },
];

/// Recycle bin for deleted records, plus export permissions for work orders
/// and payment orders.
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            "recycle_bins",
            &[
                ("id", ColType::PkAuto),
                // Resource key, e.g. "roles"; see services::recycle_bin.
                ("resource", ColType::String),
                ("record_id", ColType::String),
                ("title", ColType::String),
                // JSON snapshot needed to restore (rows, files, config).
                ("payload", ColType::Text),
                ("deleted_by_name", ColType::StringNull),
            ],
            &[("users?", "deleted_by")],
        )
        .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_recycle_bins_resource")
                    .table(Alias::new("recycle_bins"))
                    .col(Alias::new("resource"))
                    .to_owned(),
            )
            .await?;
        add_fixture_rows(manager).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .exec_stmt(
                Query::delete()
                    .from_table(Alias::new("menus"))
                    .and_where(Expr::col(Alias::new("path")).eq(MENU_PATH))
                    .to_owned(),
            )
            .await?;
        let codes: Vec<&str> = PERMISSIONS.iter().map(|item| item.code).collect();
        manager
            .exec_stmt(
                Query::delete()
                    .from_table(Alias::new("permissions"))
                    .and_where(Expr::col(Alias::new("code")).is_in(codes))
                    .to_owned(),
            )
            .await?;
        manager
            .exec_stmt(
                Query::delete()
                    .from_table(Alias::new("system_settings"))
                    .and_where(Expr::col(Alias::new("key")).eq(SETTING_KEY))
                    .to_owned(),
            )
            .await?;
        drop_table(manager, "recycle_bins").await
    }
}

async fn add_fixture_rows(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    // Fresh databases are filled by the fixture seed right after
    // migrating; only existing installations need rows added here.
    if count(manager, "permissions").await? > 0 {
        for permission in PERMISSIONS {
            if exists(manager, "permissions", "code", permission.code).await? {
                continue;
            }
            insert_with_fixture_id(
                manager,
                "permissions",
                permission.id,
                vec!["name", "code", "group_name", "description"],
                vec![
                    permission.name.into(),
                    permission.code.into(),
                    permission.group.into(),
                    permission.description.into(),
                ],
            )
            .await?;
        }
    }

    if count(manager, "system_settings").await? > 0
        && !exists(manager, "system_settings", "key", SETTING_KEY).await?
    {
        insert_with_fixture_id(
            manager,
            "system_settings",
            SETTING_ID,
            vec![
                "key",
                "name",
                "group_key",
                "value",
                "value_type",
                "default_value",
                "description",
                "is_public",
                "is_builtin",
                "is_encrypted",
                "sort_order",
            ],
            vec![
                SETTING_KEY.into(),
                "回收站保留天数".into(),
                "security".into(),
                "30".into(),
                "number".into(),
                "30".into(),
                "删除的数据在回收站保留的天数，到期后自动彻底删除；0 表示永久保留".into(),
                false.into(),
                true.into(),
                false.into(),
                75.into(),
            ],
        )
        .await?;
    }

    if count(manager, "menus").await? > 0 && !exists(manager, "menus", "path", MENU_PATH).await? {
        let parent: SimpleExpr = if exists(manager, "menus", "id", SYSTEM_MENU_ID).await? {
            SYSTEM_MENU_ID.into()
        } else {
            Option::<i32>::None.into()
        };
        insert_with_fixture_id(
            manager,
            "menus",
            MENU_ID,
            vec![
                "parent_id",
                "title",
                "path",
                "icon",
                "permission_code",
                "sort_order",
                "visible",
                "enabled",
            ],
            vec![
                parent,
                "回收站".into(),
                MENU_PATH.into(),
                "rest".into(),
                "system:recycle:list".into(),
                65.into(),
                true.into(),
                true.into(),
            ],
        )
        .await?;
    }
    Ok(())
}
