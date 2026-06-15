#![allow(clippy::missing_errors_doc)]

use axum::extract::DefaultBodyLimit;
use loco_rs::prelude::*;

use crate::{
    errors::ApiResult,
    models::{rbac, users},
};

pub mod backups;
pub mod content;
pub mod data_scopes;
pub mod departments;
pub mod dicts;
pub mod email_templates;
pub mod file_manager;
pub mod logs;
pub mod menus;
pub mod monitoring;
pub mod notifications;
pub mod payments;
pub mod permissions;
pub mod rate_limits;
pub mod roles;
pub mod scheduled_tasks;
pub mod settings;
pub mod ssh;
pub mod storage_profiles;
pub mod tenants;
pub mod uploads;
pub mod users_admin;
pub mod work_orders;

pub async fn authorize(
    ctx: &AppContext,
    auth: &auth::JWT,
    permission: &str,
) -> ApiResult<users::Model> {
    let user = users::Model::find_by_pid(&ctx.db, &auth.claims.pid).await?;
    rbac::assert_permission(&ctx.db, user.id, permission).await?;
    Ok(user)
}

#[must_use]
pub fn routes() -> Routes {
    let routes = core_routes(Routes::new());
    let routes = operations_routes(routes);
    let routes = content_routes(routes);
    extension_routes(routes)
}

fn core_routes(routes: Routes) -> Routes {
    routes
        .add(
            "/api/admin/users",
            get(users_admin::list).post(users_admin::create),
        )
        .add(
            "/api/admin/users/{id}",
            get(users_admin::get)
                .put(users_admin::update)
                .delete(users_admin::delete),
        )
        .add(
            "/api/admin/users/{id}/roles",
            get(users_admin::roles).put(users_admin::save_roles),
        )
        .add(
            "/api/admin/users/{id}/departments",
            get(users_admin::departments).put(users_admin::save_departments),
        )
        .add("/api/admin/roles", get(roles::list).post(roles::create))
        .add(
            "/api/admin/roles/{id}",
            get(roles::get).put(roles::update).delete(roles::delete),
        )
        .add(
            "/api/admin/roles/{id}/permissions",
            get(roles::permissions).put(roles::save_permissions),
        )
        .add(
            "/api/admin/roles/{id}/menus",
            get(roles::menus).put(roles::save_menus),
        )
        .add(
            "/api/admin/roles/{id}/data-scopes",
            get(roles::data_scopes).put(roles::save_data_scopes),
        )
        .add("/api/admin/menus", get(menus::list).post(menus::create))
        .add(
            "/api/admin/menus/{id}",
            get(menus::get).put(menus::update).delete(menus::delete),
        )
        .add(
            "/api/admin/permissions",
            get(permissions::list).post(permissions::create),
        )
        .add(
            "/api/admin/permissions/{id}",
            get(permissions::get)
                .put(permissions::update)
                .delete(permissions::delete),
        )
        .add(
            "/api/admin/tenants",
            get(tenants::list).post(tenants::create),
        )
        .add(
            "/api/admin/tenants/{id}",
            get(tenants::get)
                .put(tenants::update)
                .delete(tenants::delete),
        )
        .add(
            "/api/admin/departments",
            get(departments::list).post(departments::create),
        )
        .add(
            "/api/admin/departments/{id}",
            get(departments::get)
                .put(departments::update)
                .delete(departments::delete),
        )
        .add("/api/admin/data-scopes", get(data_scopes::list))
}

fn operations_routes(routes: Routes) -> Routes {
    let routes = operations_infrastructure_routes(routes);
    let routes = work_order_routes(routes);
    payment_routes(routes)
}

fn operations_infrastructure_routes(routes: Routes) -> Routes {
    routes
        .add(
            "/api/admin/notifications",
            get(notifications::list).post(notifications::create),
        )
        .add(
            "/api/admin/notifications/{id}",
            get(notifications::get).delete(notifications::delete),
        )
        .add(
            "/api/admin/notifications/{id}/read",
            put(notifications::mark_read),
        )
        .add(
            "/api/admin/scheduled-tasks",
            get(scheduled_tasks::list).post(scheduled_tasks::create),
        )
        .add(
            "/api/admin/scheduled-tasks/{id}",
            get(scheduled_tasks::get)
                .put(scheduled_tasks::update)
                .delete(scheduled_tasks::delete),
        )
        .add(
            "/api/admin/scheduled-tasks/{id}/run",
            post(scheduled_tasks::run),
        )
        .add(
            "/api/admin/scheduled-task-runs",
            get(scheduled_tasks::list_runs),
        )
        .add(
            "/api/admin/backups",
            get(backups::list).post(backups::create),
        )
        .add(
            "/api/admin/backups/{id}",
            get(backups::get).delete(backups::delete),
        )
        .add("/api/admin/backups/{id}/deliver", post(backups::deliver))
        .add(
            "/api/admin/backups/{id}/restores",
            get(backups::list_restores),
        )
        .add("/api/admin/backups/{id}/restore", post(backups::restore))
        .add(
            "/api/admin/rate-limits",
            get(rate_limits::list).post(rate_limits::create),
        )
        .add(
            "/api/admin/rate-limits/{id}",
            get(rate_limits::get)
                .put(rate_limits::update)
                .delete(rate_limits::delete),
        )
        .add(
            "/api/admin/rate-limit-events",
            get(rate_limits::list_events),
        )
        .add("/api/admin/monitoring/overview", get(monitoring::overview))
        .add("/api/admin/monitoring/server", get(monitoring::server))
        .add(
            "/api/admin/monitoring/processes",
            get(monitoring::processes),
        )
}

fn work_order_routes(routes: Routes) -> Routes {
    routes
        .add(
            "/api/admin/work-orders",
            get(work_orders::list).post(work_orders::create),
        )
        .add(
            "/api/admin/work-orders/{id}",
            get(work_orders::get)
                .put(work_orders::update)
                .delete(work_orders::delete),
        )
        .add(
            "/api/admin/work-orders/{id}/transition",
            post(work_orders::transition),
        )
        .add(
            "/api/admin/work-orders/{id}/comments",
            get(work_orders::list_comments).post(work_orders::create_comment),
        )
        .add(
            "/api/admin/work-orders/{id}/assign",
            post(work_orders::assign),
        )
        .add(
            "/api/admin/work-orders/{id}/attachments",
            get(work_orders::list_attachments).post(work_orders::create_attachment),
        )
        .add(
            "/api/admin/work-orders/{id}/attachments/{attachment_id}",
            delete(work_orders::delete_attachment),
        )
}

fn payment_routes(routes: Routes) -> Routes {
    routes
        .add(
            "/api/admin/payment-channels",
            get(payments::list_channels).post(payments::create_channel),
        )
        .add(
            "/api/admin/payment-channels/{id}",
            get(payments::get_channel)
                .put(payments::update_channel)
                .delete(payments::delete_channel),
        )
        .add(
            "/api/admin/payment-orders",
            get(payments::list_orders).post(payments::create_order),
        )
        .add("/api/admin/payment-orders/{id}", get(payments::get_order))
        .add(
            "/api/admin/payment-orders/{id}/mark-paid",
            post(payments::mark_order_paid),
        )
        .add(
            "/api/admin/payment-orders/{id}/cancel",
            post(payments::cancel_order),
        )
        .add(
            "/api/admin/payment-orders/{id}/refunds",
            post(payments::create_refund),
        )
        .add(
            "/api/admin/payment-callbacks",
            get(payments::list_callbacks),
        )
        .add(
            "/api/admin/payment-callbacks/{id}",
            get(payments::get_callback),
        )
        .add("/api/admin/payment-refunds", get(payments::list_refunds))
        .add(
            "/api/admin/payment-refunds/{id}/approve",
            post(payments::approve_refund),
        )
        .add(
            "/api/admin/payment-refunds/{id}/reject",
            post(payments::reject_refund),
        )
        .add(
            "/api/admin/payment-refunds/{id}/mark-succeeded",
            post(payments::mark_refund_succeeded),
        )
}

fn content_routes(routes: Routes) -> Routes {
    routes
        .add(
            "/api/admin/content-categories",
            get(content::list_categories).post(content::create_category),
        )
        .add(
            "/api/admin/content-categories/{id}",
            get(content::get_category)
                .put(content::update_category)
                .delete(content::delete_category),
        )
        .add(
            "/api/admin/content-articles",
            get(content::list_articles).post(content::create_article),
        )
        .add(
            "/api/admin/content-articles/{id}",
            get(content::get_article)
                .put(content::update_article)
                .delete(content::delete_article),
        )
        .add(
            "/api/admin/content-articles/{id}/publish",
            post(content::publish_article),
        )
        .add(
            "/api/admin/content-articles/{id}/archive",
            post(content::archive_article),
        )
}

fn extension_routes(routes: Routes) -> Routes {
    let routes = routes
        .add("/api/admin/logs", get(logs::list))
        .add("/api/admin/logs/{id}", get(logs::get).delete(logs::delete))
        .add(
            "/api/admin/settings",
            get(settings::list).post(settings::create),
        )
        .add(
            "/api/admin/settings/{id}",
            get(settings::get)
                .put(settings::update)
                .delete(settings::delete),
        )
        .add(
            "/api/admin/email-templates",
            get(email_templates::list).post(email_templates::create),
        )
        .add(
            "/api/admin/email-templates/{id}",
            get(email_templates::get)
                .put(email_templates::update)
                .delete(email_templates::delete),
        )
        .add(
            "/api/admin/email-templates/{id}/preview",
            post(email_templates::preview),
        )
        .add(
            "/api/admin/email-templates/{id}/test-send",
            post(email_templates::test_send),
        )
        .add(
            "/api/admin/dict-types",
            get(dicts::list_types).post(dicts::create_type),
        )
        .add(
            "/api/admin/dict-types/{id}",
            get(dicts::get_type)
                .put(dicts::update_type)
                .delete(dicts::delete_type),
        )
        .add("/api/admin/dict-types/{id}/items", get(dicts::list_items))
        .add("/api/admin/dict-items", post(dicts::create_item))
        .add(
            "/api/admin/dict-items/{id}",
            put(dicts::update_item).delete(dicts::delete_item),
        )
        .add(
            "/api/admin/storage-profiles",
            get(storage_profiles::list).post(storage_profiles::create),
        )
        .add(
            "/api/admin/storage-profiles/{id}",
            get(storage_profiles::get)
                .put(storage_profiles::update)
                .delete(storage_profiles::delete),
        )
        .add(
            "/api/admin/storage-profiles/{id}/buckets",
            get(storage_profiles::list_buckets).post(storage_profiles::create_bucket),
        )
        .add(
            "/api/admin/storage-profiles/{id}/test",
            post(storage_profiles::test),
        )
        .add(
            "/api/admin/storage-buckets/{id}",
            put(storage_profiles::update_bucket).delete(storage_profiles::delete_bucket),
        );
    let routes = file_manager_routes(routes);
    let routes = ssh_routes(routes);
    upload_routes(routes)
}

fn file_manager_routes(routes: Routes) -> Routes {
    routes
        .add("/api/admin/files/roots", get(file_manager::roots))
        .add("/api/admin/files/browser", get(file_manager::browser))
        .add(
            "/api/admin/files/folders",
            post(file_manager::create_folder),
        )
        .add(
            "/api/admin/files/upload",
            post(file_manager::upload).layer(DefaultBodyLimit::disable()),
        )
        .add("/api/admin/files/rename", put(file_manager::rename))
        .add("/api/admin/files", delete(file_manager::delete))
        .add("/api/admin/files/preview", get(file_manager::preview))
        .add("/api/admin/files/download", get(file_manager::download))
}

fn ssh_routes(routes: Routes) -> Routes {
    routes
        .add("/api/admin/ssh/targets", get(ssh::targets))
        .add("/api/admin/ssh/tickets", post(ssh::create_ticket))
        .add("/api/admin/ssh/sessions/{ticket}/ws", get(ssh::terminal_ws))
}

fn upload_routes(routes: Routes) -> Routes {
    routes
        .add(
            "/api/admin/uploads",
            get(uploads::list)
                .post(uploads::create)
                .layer(DefaultBodyLimit::disable()),
        )
        .add(
            "/api/admin/uploads/tasks",
            get(uploads::list_tasks).post(uploads::create_task),
        )
        .add(
            "/api/admin/uploads/tasks/{id}/chunks/{chunk_index}",
            post(uploads::upload_task_chunk).layer(DefaultBodyLimit::disable()),
        )
        .add(
            "/api/admin/uploads/tasks/{id}/complete",
            post(uploads::complete_task),
        )
        .add(
            "/api/admin/uploads/{id}",
            get(uploads::get)
                .put(uploads::update)
                .delete(uploads::delete),
        )
        .add("/api/admin/uploads/browser", get(uploads::browser))
        .add("/api/admin/uploads/folders", post(uploads::create_folder))
        .add(
            "/api/admin/uploads/import-object",
            post(uploads::import_object),
        )
        .add(
            "/api/admin/uploads/import-objects",
            post(uploads::import_objects),
        )
        .add("/api/admin/uploads/{id}/rename", put(uploads::rename))
        .add("/api/admin/uploads/{id}/preview", get(uploads::preview))
        .add("/api/admin/uploads/{id}/download", get(uploads::download))
}
