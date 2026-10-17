#![allow(elided_lifetimes_in_paths)]
#![allow(clippy::wildcard_imports)]
pub use sea_orm_migration::prelude::*;

mod data;
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
mod m20260527_000011_departments;
mod m20260527_000012_resource_storage;
mod m20260527_000013_upload_tasks;
mod m20260615_000014_ai_image_generations;
mod m20260616_000015_command_management;
mod m20260619_000016_command_orchestration;
mod m20260621_000017_command_artifact_preview;
mod m20260623_000018_http_request_manager;
mod m20260703_000019_chat_management;
mod m20260703_000020_managed_file_access_rules;
mod m20260717_000021_command_api_keys;
mod m20260828_000022_vnc_credentials;
mod m20260916_000023_rdp_credentials;

mod m20261007_000024_host_temperatures;

mod m20261010_000025_command_retries;
mod m20261010_000026_dashboard_home_title;
mod m20261010_000027_ddns_module;
mod m20261011_000028_user_preferences;
mod m20261011_000029_project_cli_module;
mod m20261011_000030_project_cli_runs;
mod m20261011_000031_audit_log_rules;
mod m20261011_000032_log_export_permission;
mod m20261011_000033_log_retention_description;

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
            Box::new(m20260527_000011_departments::Migration),
            Box::new(m20260527_000012_resource_storage::Migration),
            Box::new(m20260527_000013_upload_tasks::Migration),
            Box::new(m20260615_000014_ai_image_generations::Migration),
            Box::new(m20260616_000015_command_management::Migration),
            Box::new(m20260619_000016_command_orchestration::Migration),
            Box::new(m20260621_000017_command_artifact_preview::Migration),
            Box::new(m20260623_000018_http_request_manager::Migration),
            Box::new(m20260703_000019_chat_management::Migration),
            Box::new(m20260703_000020_managed_file_access_rules::Migration),
            Box::new(m20260717_000021_command_api_keys::Migration),
            Box::new(m20260828_000022_vnc_credentials::Migration),
            Box::new(m20260916_000023_rdp_credentials::Migration),
            Box::new(m20261007_000024_host_temperatures::Migration),
            Box::new(m20261010_000025_command_retries::Migration),
            Box::new(m20261010_000026_dashboard_home_title::Migration),
            Box::new(m20261010_000027_ddns_module::Migration),
            Box::new(m20261011_000028_user_preferences::Migration),
            Box::new(m20261011_000029_project_cli_module::Migration),
            Box::new(m20261011_000030_project_cli_runs::Migration),
            Box::new(m20261011_000031_audit_log_rules::Migration),
            Box::new(m20261011_000032_log_export_permission::Migration),
            Box::new(m20261011_000033_log_retention_description::Migration),
            // inject-above (do not remove this comment)
        ]
    }
}
