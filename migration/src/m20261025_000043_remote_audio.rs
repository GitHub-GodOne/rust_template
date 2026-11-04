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
        id: 181,
        code: "system:remote_audio:list",
        name: "查看远程音频",
        description: "查看远程音频页面和每次收听的流量记录",
    },
    Permission {
        id: 182,
        code: "system:remote_audio:listen",
        name: "收听远程音频",
        description: "实时收听本机正在播放的声音",
    },
];
const MENU_ID: i32 = 48;
const MENU_PATH: &str = "/admin/system/remote-audio";
const REMOTE_MENU_ID: i32 = 43;

/// 远程音频: listening to what this machine plays, with the traffic of every
/// listening session.
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "remote_audio_sessions",
            &[
                ("id", ColType::PkAuto),
                ("quality", ColType::String),
                ("sample_rate", ColType::Integer),
                ("channels", ColType::Integer),
                // Audio payload sent to the browser.
                ("bytes_sent", ColType::BigInteger),
                // Audio actually sent; silence is skipped.
                ("audio_ms", ColType::BigInteger),
                ("status", ColType::String),
                ("close_reason", ColType::StringNull),
                ("error_message", ColType::TextNull),
                ("device", ColType::StringNull),
                ("client_ip", ColType::StringNull),
                ("user_agent", ColType::TextNull),
                ("started_at", ColType::TimestampWithTimeZone),
                ("ended_at", ColType::TimestampWithTimeZoneNull),
            ],
            &[("users?", "user_id")],
        )
        .await?;
        m.create_index(
            Index::create()
                .name("idx_remote_audio_sessions_started_at")
                .table(RemoteAudioSessions::Table)
                .col(RemoteAudioSessions::StartedAt)
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("idx_remote_audio_sessions_user_id")
                .table(RemoteAudioSessions::Table)
                .col(RemoteAudioSessions::UserId)
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
        drop_table(m, "remote_audio_sessions").await
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
                    "system:remote_audio".into(),
                    permission.description.into(),
                ],
            )
            .await?;
        }
    }
    if count(m, "menus").await? > 0 && !exists(m, "menus", "path", MENU_PATH).await? {
        let parent: SimpleExpr = if exists(m, "menus", "id", REMOTE_MENU_ID).await? {
            REMOTE_MENU_ID.into()
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
                "远程音频".into(),
                MENU_PATH.into(),
                "sound".into(),
                "system:remote_audio:list".into(),
                35.into(),
                true.into(),
                true.into(),
            ],
        )
        .await?;
    }
    Ok(())
}

#[derive(DeriveIden)]
enum RemoteAudioSessions {
    Table,
    StartedAt,
    UserId,
}
