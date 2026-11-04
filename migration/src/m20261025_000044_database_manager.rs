use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

use crate::data::{count, exists, insert_with_fixture_id};

#[derive(DeriveMigrationName)]
pub struct Migration;

struct Permission {
    id: i32,
    code: &'static str,
    name: &'static str,
    description: &'static str,
}

/// Kept in sync with `src/fixtures/permissions.yaml`.
const PERMISSIONS: &[Permission] = &[
    Permission {
        id: 183,
        code: "system:database:list",
        name: "查看数据库管理",
        description: "使用自己保存的数据库连接，浏览库、表结构和表数据",
    },
    Permission {
        id: 184,
        code: "system:database:manage",
        name: "管理数据库连接",
        description: "新建、编辑、测试和删除数据库连接",
    },
    Permission {
        id: 185,
        code: "system:database:query",
        name: "执行 SQL",
        description: "在 SQL 编辑器中执行任意语句，并新增、修改、删除表数据",
    },
];
const MENU_ID: i32 = 49;
const MENU_PATH: &str = "/admin/system/database";
const TOOLS_MENU_ID: i32 = 41;

/// 数据库管理: saved connections to PostgreSQL / MySQL / SQLite databases,
/// with their query history and saved queries.
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "db_connections",
            &[
                ("id", ColType::PkAuto),
                ("name", ColType::String),
                // postgres | mysql | sqlite
                ("db_type", ColType::String),
                ("host", ColType::StringNull),
                ("port", ColType::IntegerNull),
                ("username", ColType::StringNull),
                // Default database; the file path for SQLite.
                ("database_name", ColType::StringNull),
                ("encrypted_password", ColType::TextNull),
                ("ssl_mode", ColType::StringNull),
                ("read_only", ColType::Boolean),
                ("group_name", ColType::StringNull),
                ("color", ColType::StringNull),
                ("remark", ColType::TextNull),
                ("sort_order", ColType::Integer),
                ("last_connected_at", ColType::TimestampWithTimeZoneNull),
            ],
            &[("users", "user_id")],
        )
        .await?;
        m.create_index(
            Index::create()
                .name("idx_db_connections_user_id")
                .table(DbConnections::Table)
                .col(DbConnections::UserId)
                .to_owned(),
        )
        .await?;

        create_table(
            m,
            "db_query_logs",
            &[
                ("id", ColType::PkAuto),
                ("database_name", ColType::StringNull),
                ("sql_text", ColType::Text),
                // success | failed
                ("status", ColType::String),
                ("statement_count", ColType::Integer),
                ("affected_rows", ColType::BigInteger),
                ("duration_ms", ColType::BigInteger),
                ("error_message", ColType::TextNull),
            ],
            &[("db_connections", "connection_id"), ("users", "user_id")],
        )
        .await?;
        m.create_index(
            Index::create()
                .name("idx_db_query_logs_connection_created")
                .table(DbQueryLogs::Table)
                .col(DbQueryLogs::ConnectionId)
                .col(DbQueryLogs::CreatedAt)
                .to_owned(),
        )
        .await?;

        create_table(
            m,
            "db_saved_queries",
            &[
                ("id", ColType::PkAuto),
                ("name", ColType::String),
                ("database_name", ColType::StringNull),
                ("sql_text", ColType::Text),
            ],
            &[("db_connections", "connection_id"), ("users", "user_id")],
        )
        .await?;
        m.create_index(
            Index::create()
                .name("idx_db_saved_queries_connection_id")
                .table(DbSavedQueries::Table)
                .col(DbSavedQueries::ConnectionId)
                .to_owned(),
        )
        .await?;
        seed_access(m).await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.exec_stmt(
            Query::delete()
                .from_table(Alias::new("menus"))
                .and_where(Expr::col(Alias::new("path")).eq(MENU_PATH))
                .to_owned(),
        )
        .await?;
        let codes: Vec<&str> = PERMISSIONS
            .iter()
            .map(|permission| permission.code)
            .collect();
        m.exec_stmt(
            Query::delete()
                .from_table(Alias::new("permissions"))
                .and_where(Expr::col(Alias::new("code")).is_in(codes))
                .to_owned(),
        )
        .await?;
        drop_table(m, "db_saved_queries").await?;
        drop_table(m, "db_query_logs").await?;
        drop_table(m, "db_connections").await
    }
}

/// Fresh databases are filled by the fixture seed right after migrating;
/// only existing installations need the permissions and menu added here.
async fn seed_access(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    if count(m, "permissions").await? > 0 {
        for permission in PERMISSIONS {
            if exists(m, "permissions", "code", permission.code).await? {
                continue;
            }
            insert_with_fixture_id(
                m,
                "permissions",
                permission.id,
                vec!["name", "code", "group_name", "description"],
                vec![
                    permission.name.into(),
                    permission.code.into(),
                    "system:database".into(),
                    permission.description.into(),
                ],
            )
            .await?;
        }
    }
    if count(m, "menus").await? > 0 && !exists(m, "menus", "path", MENU_PATH).await? {
        let parent: SimpleExpr = if exists(m, "menus", "id", TOOLS_MENU_ID).await? {
            TOOLS_MENU_ID.into()
        } else {
            Option::<i32>::None.into()
        };
        insert_with_fixture_id(
            m,
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
                "数据库管理".into(),
                MENU_PATH.into(),
                "database".into(),
                "system:database:list".into(),
                15.into(),
                true.into(),
                true.into(),
            ],
        )
        .await?;
    }
    Ok(())
}

#[derive(DeriveIden)]
enum DbConnections {
    Table,
    UserId,
}

#[derive(DeriveIden)]
enum DbQueryLogs {
    Table,
    ConnectionId,
    CreatedAt,
}

#[derive(DeriveIden)]
enum DbSavedQueries {
    Table,
    ConnectionId,
}
