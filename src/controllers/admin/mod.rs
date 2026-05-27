#![allow(clippy::missing_errors_doc)]

use loco_rs::prelude::*;

use crate::{
    errors::ApiResult,
    models::{rbac, users},
};

pub mod data_scopes;
pub mod dicts;
pub mod logs;
pub mod menus;
pub mod permissions;
pub mod roles;
pub mod settings;
pub mod tenants;
pub mod uploads;
pub mod users_admin;

pub async fn authorize(
    ctx: &AppContext,
    auth: &auth::JWT,
    permission: &str,
) -> ApiResult<users::Model> {
    let user = users::Model::find_by_pid(&ctx.db, &auth.claims.pid).await?;
    rbac::assert_permission(&ctx.db, user.id, permission).await?;
    Ok(user)
}

pub fn routes() -> Routes {
    Routes::new()
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
        .add("/api/admin/data-scopes", get(data_scopes::list))
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
            "/api/admin/uploads",
            get(uploads::list).post(uploads::create),
        )
        .add(
            "/api/admin/uploads/{id}",
            get(uploads::get)
                .put(uploads::update)
                .delete(uploads::delete),
        )
        .add("/api/admin/uploads/{id}/download", get(uploads::download))
}
