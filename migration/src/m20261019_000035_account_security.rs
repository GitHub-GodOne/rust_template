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

struct Setting {
    id: i32,
    key: &'static str,
    name: &'static str,
    value: &'static str,
    value_type: &'static str,
    description: &'static str,
    sort_order: i32,
}

const MENU_ID: i32 = 45;
const MENU_PATH: &str = "/admin/system/sessions";
const SYSTEM_MENU_ID: i32 = 2;

/// Kept in sync with `src/fixtures/permissions.yaml`.
const PERMISSIONS: &[Permission] = &[
    Permission {
        id: 167,
        code: "system:user:status",
        name: "启用/禁用用户",
        group: "system:user",
        description: "禁用后用户无法登录，已登录的会话立即失效",
    },
    Permission {
        id: 168,
        code: "system:user:reset_password",
        name: "重置用户密码",
        group: "system:user",
        description: "为其他用户设置新密码，并让其已登录的会话失效",
    },
    Permission {
        id: 169,
        code: "system:user:unlock",
        name: "解锁用户",
        group: "system:user",
        description: "解除因登录失败次数过多造成的账号锁定",
    },
    Permission {
        id: 170,
        code: "system:user:export",
        name: "导出用户",
        group: "system:user",
        description: "按筛选条件导出用户列表为 CSV 文件",
    },
    Permission {
        id: 171,
        code: "system:session:list",
        name: "查看在线会话",
        group: "system:session",
        description: "查看所有用户的登录会话、设备和最近活动时间",
    },
    Permission {
        id: 172,
        code: "system:session:revoke",
        name: "强制下线",
        group: "system:session",
        description: "让指定会话或用户的全部会话立即失效",
    },
];

/// Kept in sync with `src/fixtures/system_settings.yaml`.
const SETTINGS: &[Setting] = &[
    Setting {
        id: 38,
        key: "security.reauth_enabled",
        name: "敏感操作二次确认",
        value: "true",
        value_type: "boolean",
        description: "开启后，连接 SSH/VNC/RDP、执行命令、删除备份等敏感操作前需要重新输入登录密码",
        sort_order: 50,
    },
    Setting {
        id: 39,
        key: "security.reauth_minutes",
        name: "二次确认有效分钟数",
        value: "10",
        value_type: "number",
        description: "确认密码后，在这段时间内同一登录会话再做敏感操作不再重复询问",
        sort_order: 55,
    },
    Setting {
        id: 40,
        key: "security.admin_ip_allowlist",
        name: "后台访问 IP 白名单",
        value: "",
        value_type: "string",
        description: "只允许这些 IP 或网段登录和访问后台接口，多个用逗号或换行分隔，例如 203.0.113.8,192.168.1.0/24；留空表示不限制，本机地址始终允许",
        sort_order: 60,
    },
    Setting {
        id: 41,
        key: "security.api_docs_access",
        name: "接口文档访问",
        value: "login",
        value_type: "string",
        description: "public：任何人可访问；login：需登录且有接口文档权限（默认）；disabled：完全关闭",
        sort_order: 65,
    },
    Setting {
        id: 42,
        key: "security.login_captcha",
        name: "登录验证码",
        value: "after_failures",
        value_type: "string",
        description: "off：不需要；after_failures：同一账号或 IP 登录失败后需要（默认）；always：每次登录都需要",
        sort_order: 70,
    },
];

/// Account security: user status and last-login info, server-side login
/// sessions (revocable access tokens), and the related permissions, menu and
/// security settings.
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // One ALTER per column also works with the SQLite regression database.
        for column in [
            ColumnDef::new(Alias::new("status"))
                .string()
                .not_null()
                .default("active")
                .to_owned(),
            ColumnDef::new(Alias::new("last_login_at"))
                .timestamp_with_time_zone()
                .null()
                .to_owned(),
            ColumnDef::new(Alias::new("last_login_ip"))
                .string()
                .null()
                .to_owned(),
            ColumnDef::new(Alias::new("password_changed_at"))
                .timestamp_with_time_zone()
                .null()
                .to_owned(),
        ] {
            manager
                .alter_table(
                    Table::alter()
                        .table(Alias::new("users"))
                        .add_column(column)
                        .to_owned(),
                )
                .await?;
        }

        create_table(
            manager,
            "user_sessions",
            &[
                ("id", ColType::PkAuto),
                // Random id carried in the access token's `sid` claim.
                ("sid", ColType::StringUniq),
                ("ip", ColType::StringNull),
                ("user_agent", ColType::TextNull),
                ("last_seen_at", ColType::TimestampWithTimeZone),
                ("expires_at", ColType::TimestampWithTimeZone),
                ("revoked_at", ColType::TimestampWithTimeZoneNull),
                ("revoked_reason", ColType::StringNull),
                // Last password re-confirmation for sensitive operations.
                ("reauth_at", ColType::TimestampWithTimeZoneNull),
            ],
            &[("users", "user_id")],
        )
        .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Alias::new("refresh_tokens"))
                    .add_column(ColumnDef::new(Alias::new("session_id")).integer().null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_refresh_tokens_session_id")
                    .from(Alias::new("refresh_tokens"), Alias::new("session_id"))
                    .to(Alias::new("user_sessions"), Alias::new("id"))
                    .on_update(ForeignKeyAction::Cascade)
                    .on_delete(ForeignKeyAction::Cascade)
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
        let keys: Vec<&str> = SETTINGS.iter().map(|item| item.key).collect();
        manager
            .exec_stmt(
                Query::delete()
                    .from_table(Alias::new("system_settings"))
                    .and_where(Expr::col(Alias::new("key")).is_in(keys))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .name("fk_refresh_tokens_session_id")
                    .table(Alias::new("refresh_tokens"))
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Alias::new("refresh_tokens"))
                    .drop_column(Alias::new("session_id"))
                    .to_owned(),
            )
            .await?;
        drop_table(manager, "user_sessions").await?;
        for name in [
            "password_changed_at",
            "last_login_ip",
            "last_login_at",
            "status",
        ] {
            manager
                .alter_table(
                    Table::alter()
                        .table(Alias::new("users"))
                        .drop_column(Alias::new(name))
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
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

    if count(manager, "system_settings").await? > 0 {
        for setting in SETTINGS {
            if exists(manager, "system_settings", "key", setting.key).await? {
                continue;
            }
            insert_with_fixture_id(
                manager,
                "system_settings",
                setting.id,
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
                    setting.key.into(),
                    setting.name.into(),
                    "security".into(),
                    setting.value.into(),
                    setting.value_type.into(),
                    setting.value.into(),
                    setting.description.into(),
                    false.into(),
                    true.into(),
                    false.into(),
                    setting.sort_order.into(),
                ],
            )
            .await?;
        }
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
                "在线会话".into(),
                MENU_PATH.into(),
                "laptop".into(),
                "system:session:list".into(),
                55.into(),
                true.into(),
                true.into(),
            ],
        )
        .await?;
    }
    Ok(())
}
