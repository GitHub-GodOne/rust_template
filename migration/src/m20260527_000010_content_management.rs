use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_content_tables(m).await?;
        create_content_indexes(m).await?;
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        drop_content_indexes(m).await?;
        drop_table(m, "content_articles").await?;
        drop_table(m, "content_categories").await
    }
}

async fn create_content_tables(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    create_table(
        m,
        "content_categories",
        &[
            ("id", ColType::PkAuto),
            ("name", ColType::String),
            ("slug", ColType::StringUniq),
            ("description", ColType::TextNull),
            ("sort_order", ColType::IntegerWithDefault(0)),
            ("enabled", ColType::BooleanWithDefault(true)),
        ],
        &[("users?", "created_by"), ("users?", "updated_by")],
    )
    .await?;

    create_table(
        m,
        "content_articles",
        &[
            ("id", ColType::PkAuto),
            ("title", ColType::String),
            ("slug", ColType::StringUniq),
            ("summary", ColType::TextNull),
            ("content", ColType::Text),
            ("cover_image_url", ColType::StringNull),
            ("status", ColType::String),
            ("is_featured", ColType::BooleanWithDefault(false)),
            ("published_at", ColType::TimestampWithTimeZoneNull),
            ("seo_title", ColType::StringNull),
            ("seo_description", ColType::TextNull),
        ],
        &[
            ("content_categories", "category_id"),
            ("users?", "created_by"),
            ("users?", "updated_by"),
        ],
    )
    .await
}

async fn create_content_indexes(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    create_category_indexes(m).await?;
    create_article_indexes(m).await
}

async fn create_category_indexes(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    m.create_index(
        Index::create()
            .name("idx_content_categories_enabled_sort")
            .table(ContentCategories::Table)
            .col(ContentCategories::Enabled)
            .col(ContentCategories::SortOrder)
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_content_categories_created_at")
            .table(ContentCategories::Table)
            .col(ContentCategories::CreatedAt)
            .to_owned(),
    )
    .await
}

async fn create_article_indexes(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    m.create_index(
        Index::create()
            .name("idx_content_articles_category_status")
            .table(ContentArticles::Table)
            .col(ContentArticles::CategoryId)
            .col(ContentArticles::Status)
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_content_articles_status_published")
            .table(ContentArticles::Table)
            .col(ContentArticles::Status)
            .col(ContentArticles::PublishedAt)
            .to_owned(),
    )
    .await?;
    m.create_index(
        Index::create()
            .name("idx_content_articles_featured_created")
            .table(ContentArticles::Table)
            .col(ContentArticles::IsFeatured)
            .col(ContentArticles::CreatedAt)
            .to_owned(),
    )
    .await
}

async fn drop_content_indexes(m: &SchemaManager<'_>) -> Result<(), DbErr> {
    for name in [
        "idx_content_articles_featured_created",
        "idx_content_articles_status_published",
        "idx_content_articles_category_status",
    ] {
        m.drop_index(
            Index::drop()
                .name(name)
                .table(ContentArticles::Table)
                .to_owned(),
        )
        .await?;
    }

    for name in [
        "idx_content_categories_created_at",
        "idx_content_categories_enabled_sort",
    ] {
        m.drop_index(
            Index::drop()
                .name(name)
                .table(ContentCategories::Table)
                .to_owned(),
        )
        .await?;
    }

    Ok(())
}

#[derive(DeriveIden)]
enum ContentCategories {
    Table,
    Enabled,
    SortOrder,
    CreatedAt,
}

#[derive(DeriveIden)]
enum ContentArticles {
    Table,
    CategoryId,
    Status,
    PublishedAt,
    IsFeatured,
    CreatedAt,
}
