#![allow(elided_lifetimes_in_paths)]
#![allow(clippy::wildcard_imports)]
pub use sea_orm_migration::prelude::*;
mod m20220101_000001_users;
mod m20260527_000001_refresh_tokens;
mod m20260527_000002_rbac;
mod m20260527_000003_admin_extensions;
mod m20260527_000004_tenants;
mod m20260527_000005_operations_infra;
mod m20260527_000006_email_templates;
mod m20260527_000007_work_orders;
mod m20260527_000008_payments;
mod m20260527_000009_database_restores;
mod m20260527_000010_content_management;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20220101_000001_users::Migration),
            Box::new(m20260527_000001_refresh_tokens::Migration),
            Box::new(m20260527_000002_rbac::Migration),
            Box::new(m20260527_000003_admin_extensions::Migration),
            Box::new(m20260527_000004_tenants::Migration),
            Box::new(m20260527_000005_operations_infra::Migration),
            Box::new(m20260527_000006_email_templates::Migration),
            Box::new(m20260527_000007_work_orders::Migration),
            Box::new(m20260527_000008_payments::Migration),
            Box::new(m20260527_000009_database_restores::Migration),
            Box::new(m20260527_000010_content_management::Migration),
            // inject-above (do not remove this comment)
        ]
    }
}
