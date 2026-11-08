use crate::data::{count, exists, insert_with_fixture_id};
use sea_orm_migration::prelude::*;
#[derive(DeriveMigrationName)]
pub struct Migration;
const ITEMS: &[(i32, &str, &str, &str)] = &[
    (
        186,
        "system:rathole:list",
        "查看内网穿透",
        "查看 Rathole 配置和运行状态",
    ),
    (
        187,
        "system:rathole:manage",
        "管理内网穿透",
        "新增、编辑、删除和校验 Rathole 配置",
    ),
    (
        188,
        "system:rathole:run",
        "运行内网穿透",
        "启动、停止和重启 Rathole 实例",
    ),
];
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        if count(m, "permissions").await? > 0 {
            for (id, code, name, desc) in ITEMS {
                if !exists(m, "permissions", "code", *code).await? {
                    insert_with_fixture_id(
                        m,
                        "permissions",
                        *id,
                        vec!["name", "code", "group_name", "description"],
                        vec![
                            (*name).into(),
                            (*code).into(),
                            "system:rathole".into(),
                            (*desc).into(),
                        ],
                    )
                    .await?;
                }
            }
        }
        if count(m, "menus").await? > 0
            && !exists(m, "menus", "path", "/admin/system/rathole").await?
        {
            insert_with_fixture_id(
                m,
                "menus",
                51,
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
                    2.into(),
                    "内网穿透（Rathole）".into(),
                    "/admin/system/rathole".into(),
                    "global".into(),
                    "system:rathole:list".into(),
                    96.into(),
                    true.into(),
                    true.into(),
                ],
            )
            .await?;
        }
        Ok(())
    }
    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.exec_stmt(
            Query::delete()
                .from_table(Alias::new("menus"))
                .and_where(Expr::col(Alias::new("path")).eq("/admin/system/rathole"))
                .to_owned(),
        )
        .await?;
        m.exec_stmt(
            Query::delete()
                .from_table(Alias::new("permissions"))
                .and_where(
                    Expr::col(Alias::new("code"))
                        .is_in(ITEMS.iter().map(|x| x.1).collect::<Vec<_>>()),
                )
                .to_owned(),
        )
        .await
    }
}
