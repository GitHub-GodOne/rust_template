use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

use crate::data::{count, exists, insert_with_fixture_id};

#[derive(DeriveMigrationName)]
pub struct Migration;

const PERMISSION_ID: i32 = 180;
const PERMISSION_CODE: &str = "system:im:chat";
const MENU_ID: i32 = 47;
const MENU_PATH: &str = "/admin/system/im";
const UTILITIES_MENU_ID: i32 = 41;

/// 在线聊天: direct and group conversations between back-office users.
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "im_conversations",
            &[
                ("id", ColType::PkAuto),
                ("kind", ColType::String),
                ("name", ColType::StringNull),
                // "<smaller user id>:<larger user id>" for direct chats.
                ("direct_key", ColType::StringNull),
                ("last_message_at", ColType::TimestampWithTimeZoneNull),
            ],
            &[("users?", "owner_id")],
        )
        .await?;
        create_table(
            m,
            "im_members",
            &[
                ("id", ColType::PkAuto),
                ("role", ColType::String),
                ("last_read_message_id", ColType::IntegerNull),
            ],
            &[
                ("im_conversations", "conversation_id"),
                ("users", "user_id"),
            ],
        )
        .await?;
        create_table(
            m,
            "im_messages",
            &[
                ("id", ColType::PkAuto),
                ("kind", ColType::String),
                ("content", ColType::Text),
                ("file_name", ColType::StringNull),
                ("file_path", ColType::TextNull),
                ("mime_type", ColType::StringNull),
                ("size_bytes", ColType::BigIntegerNull),
            ],
            &[
                ("im_conversations", "conversation_id"),
                ("users?", "sender_id"),
            ],
        )
        .await?;
        create_indexes(m).await?;
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
        m.exec_stmt(
            Query::delete()
                .from_table(Alias::new("permissions"))
                .and_where(Expr::col(Alias::new("code")).eq(PERMISSION_CODE))
                .to_owned(),
        )
        .await?;
        drop_table(m, "im_messages").await?;
        drop_table(m, "im_members").await?;
        drop_table(m, "im_conversations").await
    }
}

async fn create_indexes(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    m.create_index(
        Index::create()
            .name("idx_im_conversations_direct_key")
            .table(ImConversations::Table)
            .col(ImConversations::DirectKey)
            .unique()
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_im_members_conversation_user")
            .table(ImMembers::Table)
            .col(ImMembers::ConversationId)
            .col(ImMembers::UserId)
            .unique()
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_im_members_user")
            .table(ImMembers::Table)
            .col(ImMembers::UserId)
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_im_messages_conversation_id")
            .table(ImMessages::Table)
            .col(ImMessages::ConversationId)
            .col(ImMessages::Id)
            .to_owned(),
    )
    .await
}

/// Fresh databases are filled by the fixture seed right after migrating;
/// only existing installations need the permission and menu added here.
async fn seed_access(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    if count(m, "permissions").await? > 0
        && !exists(m, "permissions", "code", PERMISSION_CODE).await?
    {
        insert_with_fixture_id(
            m,
            "permissions",
            PERMISSION_ID,
            vec!["name", "code", "group_name", "description"],
            vec![
                "使用在线聊天".into(),
                PERMISSION_CODE.into(),
                "system:im".into(),
                "与其他后台用户单聊、建群并收发消息、图片和文件".into(),
            ],
        )
        .await?;
    }
    if count(m, "menus").await? > 0 && !exists(m, "menus", "path", MENU_PATH).await? {
        let parent: SimpleExpr = if exists(m, "menus", "id", UTILITIES_MENU_ID).await? {
            UTILITIES_MENU_ID.into()
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
                "在线聊天".into(),
                MENU_PATH.into(),
                "comment".into(),
                PERMISSION_CODE.into(),
                5.into(),
                true.into(),
                true.into(),
            ],
        )
        .await?;
    }
    Ok(())
}

#[derive(DeriveIden)]
enum ImConversations {
    Table,
    DirectKey,
}

#[derive(DeriveIden)]
enum ImMembers {
    Table,
    ConversationId,
    UserId,
}

#[derive(DeriveIden)]
enum ImMessages {
    Table,
    Id,
    ConversationId,
}
