use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        add_login_lockout_columns(m).await?;
        create_chat_messages_table(m).await?;
        create_chat_message_attachments_table(m).await?;
        create_upload_file_fk(m).await?;
        create_chat_indexes(m).await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.drop_index(
            Index::drop()
                .name("idx_chat_message_attachments_upload")
                .table(ChatMessageAttachments::Table)
                .to_owned(),
        )
        .await?;
        m.drop_index(
            Index::drop()
                .name("idx_chat_message_attachments_message")
                .table(ChatMessageAttachments::Table)
                .to_owned(),
        )
        .await?;
        m.drop_index(
            Index::drop()
                .name("idx_chat_messages_created_by")
                .table(ChatMessages::Table)
                .to_owned(),
        )
        .await?;
        m.drop_index(
            Index::drop()
                .name("idx_chat_messages_status_created")
                .table(ChatMessages::Table)
                .to_owned(),
        )
        .await?;
        m.drop_foreign_key(
            ForeignKey::drop()
                .name("fk_chat_message_attachments_upload_file_id")
                .table(ChatMessageAttachments::Table)
                .to_owned(),
        )
        .await?;
        drop_table(m, "chat_message_attachments").await?;
        drop_table(m, "chat_messages").await?;
        m.alter_table(
            Table::alter()
                .table(Users::Table)
                .drop_column(Users::LastFailedLoginAt)
                .drop_column(Users::LockedUntil)
                .drop_column(Users::FailedLoginAttempts)
                .to_owned(),
        )
        .await
    }
}

async fn add_login_lockout_columns(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    m.alter_table(
        Table::alter()
            .table(Users::Table)
            .add_column(
                ColumnDef::new(Users::FailedLoginAttempts)
                    .integer()
                    .not_null()
                    .default(0),
            )
            .add_column(
                ColumnDef::new(Users::LockedUntil)
                    .timestamp_with_time_zone()
                    .null(),
            )
            .add_column(
                ColumnDef::new(Users::LastFailedLoginAt)
                    .timestamp_with_time_zone()
                    .null(),
            )
            .to_owned(),
    )
    .await
}

async fn create_chat_messages_table(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    create_table(
        m,
        "chat_messages",
        &[
            ("id", ColType::PkAuto),
            ("title", ColType::String),
            ("content", ColType::Text),
            ("status", ColType::String),
            ("sent_at", ColType::TimestampWithTimeZoneNull),
            ("copy_count", ColType::Integer),
            ("last_copied_at", ColType::TimestampWithTimeZoneNull),
            ("remark", ColType::TextNull),
        ],
        &[("users?", "created_by"), ("users?", "updated_by")],
    )
    .await
}

async fn create_chat_message_attachments_table(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    create_table(
        m,
        "chat_message_attachments",
        &[
            ("id", ColType::PkAuto),
            ("source_type", ColType::String),
            ("upload_file_id", ColType::IntegerNull),
            ("file_root_key", ColType::StringNull),
            ("file_path", ColType::TextNull),
            ("display_name", ColType::String),
            ("mime_type", ColType::StringNull),
            ("size_bytes", ColType::BigInteger),
            ("sort_order", ColType::Integer),
        ],
        &[
            ("chat_messages", "chat_message_id"),
            ("users?", "created_by"),
        ],
    )
    .await
}

async fn create_upload_file_fk(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    m.create_foreign_key(
        ForeignKey::create()
            .name("fk_chat_message_attachments_upload_file_id")
            .from(
                ChatMessageAttachments::Table,
                ChatMessageAttachments::UploadFileId,
            )
            .to(UploadFiles::Table, UploadFiles::Id)
            .on_update(ForeignKeyAction::Cascade)
            .on_delete(ForeignKeyAction::SetNull)
            .to_owned(),
    )
    .await
}

async fn create_chat_indexes(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    m.create_index(
        Index::create()
            .name("idx_chat_messages_status_created")
            .table(ChatMessages::Table)
            .col(ChatMessages::Status)
            .col(ChatMessages::CreatedAt)
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_chat_messages_created_by")
            .table(ChatMessages::Table)
            .col(ChatMessages::CreatedBy)
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_chat_message_attachments_message")
            .table(ChatMessageAttachments::Table)
            .col(ChatMessageAttachments::ChatMessageId)
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_chat_message_attachments_upload")
            .table(ChatMessageAttachments::Table)
            .col(ChatMessageAttachments::UploadFileId)
            .to_owned(),
    )
    .await
}

#[derive(DeriveIden)]
enum Users {
    Table,
    FailedLoginAttempts,
    LockedUntil,
    LastFailedLoginAt,
}

#[derive(DeriveIden)]
enum ChatMessages {
    Table,
    Status,
    CreatedAt,
    CreatedBy,
}

#[derive(DeriveIden)]
enum ChatMessageAttachments {
    Table,
    ChatMessageId,
    UploadFileId,
}

#[derive(DeriveIden)]
enum UploadFiles {
    Table,
    Id,
}
