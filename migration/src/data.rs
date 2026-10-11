//! Helpers for migrations that add fixture rows to existing installations.

use sea_orm_migration::{
    prelude::*,
    sea_orm::{ConnectionTrait, DatabaseBackend, Statement, Value as DbValue},
};

pub async fn count(manager: &SchemaManager<'_>, table: &str) -> Result<i64, DbErr> {
    let db = manager.get_connection();
    Ok(db
        .query_one(
            db.get_database_backend().build(
                Query::select()
                    .expr(Expr::col(Alias::new("id")).count())
                    .from(Alias::new(table)),
            ),
        )
        .await?
        .and_then(|row| row.try_get_by_index::<i64>(0).ok())
        .unwrap_or(0))
}

pub async fn exists(
    manager: &SchemaManager<'_>,
    table: &str,
    column: &str,
    value: impl Into<SimpleExpr>,
) -> Result<bool, DbErr> {
    let db = manager.get_connection();
    Ok(db
        .query_one(
            db.get_database_backend().build(
                Query::select()
                    .column(Alias::new("id"))
                    .from(Alias::new(table))
                    .and_where(Expr::col(Alias::new(column)).eq(value)),
            ),
        )
        .await?
        .is_some())
}

/// Inserts a row, keeping the fixture id when it is still free so that
/// `admin.sync_fixture_baseline` can match it later.
pub async fn insert_with_fixture_id(
    manager: &SchemaManager<'_>,
    table: &str,
    id: i32,
    columns: Vec<&str>,
    values: Vec<SimpleExpr>,
) -> Result<(), DbErr> {
    let mut columns: Vec<Alias> = columns.into_iter().map(Alias::new).collect();
    let mut values = values;
    if !exists(manager, table, "id", id).await? {
        columns.insert(0, Alias::new("id"));
        values.insert(0, id.into());
    }
    manager
        .exec_stmt(
            Query::insert()
                .into_table(Alias::new(table))
                .columns(columns)
                .values_panic(values)
                .to_owned(),
        )
        .await?;
    reset_sequence(manager, table).await
}

pub async fn reset_sequence(manager: &SchemaManager<'_>, table: &str) -> Result<(), DbErr> {
    let db = manager.get_connection();
    if db.get_database_backend() != DatabaseBackend::Postgres {
        return Ok(());
    }
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        format!(
            "SELECT setval(pg_get_serial_sequence('{table}', 'id'), COALESCE((SELECT MAX(id) FROM {table}), 1), true)"
        ),
        Vec::<DbValue>::new(),
    ))
    .await?;
    Ok(())
}
