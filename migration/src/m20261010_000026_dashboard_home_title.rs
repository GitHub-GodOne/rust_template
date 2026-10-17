use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

const DASHBOARD_PATH: &str = "/admin/dashboard";
const DASHBOARD_PERMISSION: &str = "dashboard:view";

#[derive(Clone, Copy)]
struct Labels {
    menu_title: &'static str,
    menu_icon: &'static str,
    permission_name: &'static str,
    permission_description: &'static str,
}

const DASHBOARD: Labels = Labels {
    menu_title: "仪表盘",
    menu_icon: "dashboard",
    permission_name: "查看仪表盘",
    permission_description: "允许访问后台仪表盘",
};

const HOME: Labels = Labels {
    menu_title: "首页",
    menu_icon: "home",
    permission_name: "查看首页",
    permission_description: "允许访问后台首页",
};

/// Renames the seeded "仪表盘" menu and permission to "首页". Columns that an
/// administrator already customised are left untouched.
async fn rename(manager: &SchemaManager<'_>, from: Labels, to: Labels) -> Result<(), DbErr> {
    for (column, from_value, to_value) in [
        ("title", from.menu_title, to.menu_title),
        ("icon", from.menu_icon, to.menu_icon),
    ] {
        manager
            .exec_stmt(
                Query::update()
                    .table(Alias::new("menus"))
                    .value(Alias::new(column), to_value)
                    .and_where(Expr::col(Alias::new("path")).eq(DASHBOARD_PATH))
                    .and_where(Expr::col(Alias::new(column)).eq(from_value))
                    .to_owned(),
            )
            .await?;
    }
    for (column, from_value, to_value) in [
        ("name", from.permission_name, to.permission_name),
        (
            "description",
            from.permission_description,
            to.permission_description,
        ),
    ] {
        manager
            .exec_stmt(
                Query::update()
                    .table(Alias::new("permissions"))
                    .value(Alias::new(column), to_value)
                    .and_where(Expr::col(Alias::new("code")).eq(DASHBOARD_PERMISSION))
                    .and_where(Expr::col(Alias::new(column)).eq(from_value))
                    .to_owned(),
            )
            .await?;
    }
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        rename(manager, DASHBOARD, HOME).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        rename(manager, HOME, DASHBOARD).await
    }
}
