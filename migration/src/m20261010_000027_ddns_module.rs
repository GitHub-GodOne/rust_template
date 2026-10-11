use sea_orm_migration::prelude::*;

use crate::data::{count, exists, insert_with_fixture_id};

#[derive(DeriveMigrationName)]
pub struct Migration;

struct Setting {
    key: &'static str,
    name: &'static str,
    value: &'static str,
    value_type: &'static str,
    description: &'static str,
    is_encrypted: bool,
    sort_order: i32,
}

struct Permission {
    id: i32,
    code: &'static str,
    name: &'static str,
    description: &'static str,
}

const MENU_ID: i32 = 33;
const MENU_PATH: &str = "/admin/system/ddns";
const SYSTEM_MENU_ID: i32 = 2;

/// Cloudflare IPv6 DDNS settings. Kept in sync with `src/fixtures/system_settings.yaml`.
const SETTINGS: &[Setting] = &[
    Setting {
        key: "ddns.enabled",
        name: "启用 IPv6 动态解析",
        value: "false",
        value_type: "boolean",
        description: "开启后定时检测本机公网 IPv6，并自动更新到 Cloudflare 的 AAAA 记录",
        is_encrypted: false,
        sort_order: 210,
    },
    Setting {
        key: "ddns.cloudflare_api_token",
        name: "Cloudflare API Token",
        value: "",
        value_type: "secret",
        description: "Cloudflare API 令牌，需要 Zone.DNS 编辑权限（建议只授权目标域名）",
        is_encrypted: true,
        sort_order: 215,
    },
    Setting {
        key: "ddns.cloudflare_zone_id",
        name: "Cloudflare Zone ID",
        value: "",
        value_type: "string",
        description: "域名所在 Zone 的 ID；留空时根据域名自动查找",
        is_encrypted: false,
        sort_order: 220,
    },
    Setting {
        key: "ddns.record_names",
        name: "解析域名",
        value: "",
        value_type: "string",
        description: "需要更新的完整域名，多个用逗号分隔，例如 home.example.com,nas.example.com",
        is_encrypted: false,
        sort_order: 225,
    },
    Setting {
        key: "ddns.proxied",
        name: "Cloudflare 代理",
        value: "false",
        value_type: "boolean",
        description: "是否开启 Cloudflare 代理（橙色云朵）；直连 IPv6 时请保持 false",
        is_encrypted: false,
        sort_order: 230,
    },
    Setting {
        key: "ddns.ttl",
        name: "解析 TTL",
        value: "1",
        value_type: "number",
        description: "记录 TTL 秒数，1 表示自动，也可填 60 到 86400",
        is_encrypted: false,
        sort_order: 235,
    },
    Setting {
        key: "ddns.check_interval_seconds",
        name: "检测间隔（秒）",
        value: "300",
        value_type: "number",
        description: "检测本机 IPv6 是否变化的间隔，30 到 86400 秒",
        is_encrypted: false,
        sort_order: 240,
    },
    Setting {
        key: "ddns.ipv6_source",
        name: "IPv6 获取方式",
        value: "auto",
        value_type: "string",
        description:
            "auto：优先读取网卡，失败再查外部地址；interface：只读网卡；url：只通过外部地址查询；manual：使用手动指定的地址",
        is_encrypted: false,
        sort_order: 245,
    },
    Setting {
        key: "ddns.interface_name",
        name: "指定网卡",
        value: "",
        value_type: "string",
        description: "只使用该网卡上的 IPv6（如 eth0、en0、以太网），留空自动选择稳定的公网地址",
        is_encrypted: false,
        sort_order: 250,
    },
    Setting {
        key: "ddns.ipv6_lookup_url",
        name: "IPv6 查询地址",
        value: "https://api6.ipify.org",
        value_type: "string",
        description: "返回纯文本 IPv6 的查询地址，url/auto 模式使用",
        is_encrypted: false,
        sort_order: 255,
    },
    Setting {
        key: "ddns.cloudflare_api_base",
        name: "Cloudflare API 地址",
        value: "https://api.cloudflare.com/client/v4",
        value_type: "string",
        description: "Cloudflare API 根地址，一般无需修改",
        is_encrypted: false,
        sort_order: 260,
    },
    Setting {
        key: "ddns.manual_ipv6",
        name: "手动指定 IPv6",
        value: "",
        value_type: "string",
        description: "获取方式为 manual 时同步到域名的固定 IPv6 地址",
        is_encrypted: false,
        sort_order: 265,
    },
];

/// Kept in sync with `src/fixtures/permissions.yaml`.
const PERMISSIONS: &[Permission] = &[
    Permission {
        id: 156,
        code: "system:ddns:view",
        name: "查看 IPv6 动态解析",
        description: "查看 Cloudflare IPv6 动态解析的状态和配置",
    },
    Permission {
        id: 157,
        code: "system:ddns:update",
        name: "管理 IPv6 动态解析",
        description: "修改 IPv6 动态解析配置并手动同步到 Cloudflare",
    },
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Fresh databases are filled by the fixture seed right after
        // migrating; only existing installations need rows added here.
        if count(manager, "system_settings").await? > 0 {
            for setting in SETTINGS {
                if exists(manager, "system_settings", "key", setting.key).await? {
                    continue;
                }
                manager
                    .exec_stmt(
                        Query::insert()
                            .into_table(Alias::new("system_settings"))
                            .columns([
                                Alias::new("key"),
                                Alias::new("name"),
                                Alias::new("group_key"),
                                Alias::new("value"),
                                Alias::new("value_type"),
                                Alias::new("default_value"),
                                Alias::new("description"),
                                Alias::new("is_public"),
                                Alias::new("is_builtin"),
                                Alias::new("is_encrypted"),
                                Alias::new("sort_order"),
                            ])
                            .values_panic([
                                setting.key.into(),
                                setting.name.into(),
                                "ddns".into(),
                                setting.value.into(),
                                setting.value_type.into(),
                                setting.value.into(),
                                setting.description.into(),
                                false.into(),
                                true.into(),
                                setting.is_encrypted.into(),
                                setting.sort_order.into(),
                            ])
                            .to_owned(),
                    )
                    .await?;
            }
        }

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
                        "system:ddns".into(),
                        permission.description.into(),
                    ],
                )
                .await?;
            }
        }

        if count(manager, "menus").await? > 0
            && !exists(manager, "menus", "path", MENU_PATH).await?
        {
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
                    "IPv6 动态解析".into(),
                    MENU_PATH.into(),
                    "wifi".into(),
                    "system:ddns:view".into(),
                    245.into(),
                    true.into(),
                    true.into(),
                ],
            )
            .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let keys: Vec<&str> = SETTINGS.iter().map(|setting| setting.key).collect();
        manager
            .exec_stmt(
                Query::delete()
                    .from_table(Alias::new("system_settings"))
                    .and_where(Expr::col(Alias::new("key")).is_in(keys))
                    .to_owned(),
            )
            .await?;
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
            .await
    }
}
