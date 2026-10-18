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

const MENU_ID: i32 = 44;
const MENU_PATH: &str = "/admin/system/certificates";
const REMOTE_MENU_ID: i32 = 43;

/// Kept in sync with `src/fixtures/permissions.yaml`.
const PERMISSIONS: &[Permission] = &[
    Permission {
        id: 162,
        code: "system:certificate:list",
        name: "查看证书与域名映射",
        description: "查看域名证书、域名映射和运行状态",
    },
    Permission {
        id: 163,
        code: "system:certificate:create",
        name: "新增证书与域名映射",
        description: "申请域名证书、新增域名映射",
    },
    Permission {
        id: 164,
        code: "system:certificate:update",
        name: "修改证书与域名映射",
        description: "修改域名映射、证书续签和 Cloudflare/ACME 设置",
    },
    Permission {
        id: 165,
        code: "system:certificate:delete",
        name: "删除证书与域名映射",
        description: "删除域名证书和域名映射",
    },
    Permission {
        id: 166,
        code: "system:certificate:issue",
        name: "签发与同步证书",
        description: "立即签发/续签证书、同步 DNS 记录与 Cloudflare Tunnel",
    },
];

/// Certificates issued through ACME and the domain → local service mappings
/// served by the built-in HTTPS proxy or a Cloudflare Tunnel.
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_tables(manager).await?;
        add_permissions_and_menu(manager).await
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
        let codes: Vec<&str> = PERMISSIONS
            .iter()
            .map(|permission| permission.code)
            .collect();
        manager
            .exec_stmt(
                Query::delete()
                    .from_table(Alias::new("permissions"))
                    .and_where(Expr::col(Alias::new("code")).is_in(codes))
                    .to_owned(),
            )
            .await?;
        drop_table(manager, "domain_mappings").await?;
        drop_table(manager, "tls_certificates").await
    }
}

async fn create_tables(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    create_table(
        manager,
        "tls_certificates",
        &[
            ("id", ColType::PkAuto),
            ("domain", ColType::StringUniq),
            // JSON array of extra subject names, e.g. ["*.example.com"].
            ("alt_names", ColType::Text),
            ("status", ColType::String),
            ("auto_renew", ColType::Boolean),
            ("issuer", ColType::StringNull),
            ("not_before", ColType::TimestampWithTimeZoneNull),
            ("not_after", ColType::TimestampWithTimeZoneNull),
            ("cert_path", ColType::TextNull),
            ("key_path", ColType::TextNull),
            ("last_error", ColType::TextNull),
            ("last_attempt_at", ColType::TimestampWithTimeZoneNull),
            ("renewed_at", ColType::TimestampWithTimeZoneNull),
        ],
        &[("users?", "created_by")],
    )
    .await?;
    create_table(
        manager,
        "domain_mappings",
        &[
            ("id", ColType::PkAuto),
            ("name", ColType::String),
            ("domain", ColType::StringUniq),
            // "direct" (built-in HTTPS proxy) or "tunnel" (cloudflared).
            ("mode", ColType::String),
            ("upstream_host", ColType::String),
            ("upstream_port", ColType::Integer),
            ("public_port", ColType::Integer),
            // "off", "auto" or "manual".
            ("ipv4_mode", ColType::String),
            ("ipv4_address", ColType::StringNull),
            ("ipv6_mode", ColType::String),
            ("ipv6_address", ColType::StringNull),
            ("enabled", ColType::Boolean),
            ("status", ColType::String),
            ("last_error", ColType::TextNull),
            ("synced_at", ColType::TimestampWithTimeZoneNull),
        ],
        &[
            ("users?", "created_by"),
            ("tls_certificates?", "certificate_id"),
        ],
    )
    .await
}

async fn add_permissions_and_menu(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
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
                    "system:certificate".into(),
                    permission.description.into(),
                ],
            )
            .await?;
        }
    }

    if count(manager, "menus").await? > 0 && !exists(manager, "menus", "path", MENU_PATH).await? {
        let parent: SimpleExpr = if exists(manager, "menus", "id", REMOTE_MENU_ID).await? {
            REMOTE_MENU_ID.into()
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
                "证书与域名".into(),
                MENU_PATH.into(),
                "safety-certificate".into(),
                "system:certificate:list".into(),
                40.into(),
                true.into(),
                true.into(),
            ],
        )
        .await?;
    }
    Ok(())
}
