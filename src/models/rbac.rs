#![allow(clippy::missing_errors_doc)]

use std::collections::BTreeMap;

use loco_rs::model::{ModelError, ModelResult};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};

use crate::{
    errors::{ApiError, ApiResult},
    models::_entities::{
        data_scopes, menus, permissions, role_data_scopes, role_menus, role_permissions, roles,
        user_roles, users,
    },
    views::auth::{CurrentMenuActions, CurrentMenuItem, CurrentRole},
};

pub const SUPER_ADMIN_ROLE: &str = "super_admin";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EffectiveDataScope {
    All,
    Tenant {
        tenant_id: i32,
    },
    SelfOnly {
        user_id: i32,
        tenant_id: Option<i32>,
    },
    None,
}

impl EffectiveDataScope {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Tenant { .. } => "tenant",
            Self::SelfOnly { .. } => "self",
            Self::None => "none",
        }
    }

    #[must_use]
    pub const fn is_all(&self) -> bool {
        matches!(self, Self::All)
    }
}

pub async fn load_user_roles(
    db: &DatabaseConnection,
    user_id: i32,
) -> ModelResult<Vec<roles::Model>> {
    let links = user_roles::Entity::find()
        .filter(user_roles::Column::UserId.eq(user_id))
        .all(db)
        .await?;

    let role_ids = unique_ids(links.iter().map(|link| link.role_id));
    if role_ids.is_empty() {
        return Ok(Vec::new());
    }

    roles::Entity::find()
        .filter(roles::Column::Id.is_in(role_ids))
        .filter(roles::Column::Enabled.eq(true))
        .order_by_asc(roles::Column::Id)
        .all(db)
        .await
        .map_err(ModelError::from)
}

pub async fn load_user_permissions(
    db: &DatabaseConnection,
    user_id: i32,
) -> ModelResult<Vec<permissions::Model>> {
    let user_roles = load_user_roles(db, user_id).await?;
    if has_super_admin_role(&user_roles) {
        return permissions::Entity::find()
            .order_by_asc(permissions::Column::GroupName)
            .order_by_asc(permissions::Column::Id)
            .all(db)
            .await
            .map_err(ModelError::from);
    }

    let role_ids = unique_ids(user_roles.iter().map(|role| role.id));
    if role_ids.is_empty() {
        return Ok(Vec::new());
    }

    let links = role_permissions::Entity::find()
        .filter(role_permissions::Column::RoleId.is_in(role_ids))
        .all(db)
        .await?;
    let permission_ids = unique_ids(links.iter().map(|link| link.permission_id));
    if permission_ids.is_empty() {
        return Ok(Vec::new());
    }

    permissions::Entity::find()
        .filter(permissions::Column::Id.is_in(permission_ids))
        .order_by_asc(permissions::Column::GroupName)
        .order_by_asc(permissions::Column::Id)
        .all(db)
        .await
        .map_err(ModelError::from)
}

pub async fn load_user_menus(
    db: &DatabaseConnection,
    user_id: i32,
) -> ModelResult<Vec<CurrentMenuItem>> {
    let user_roles = load_user_roles(db, user_id).await?;
    if has_super_admin_role(&user_roles) {
        let menus = menus::Entity::find()
            .filter(menus::Column::Enabled.eq(true))
            .filter(menus::Column::Visible.eq(true))
            .order_by_asc(menus::Column::SortOrder)
            .order_by_asc(menus::Column::Id)
            .all(db)
            .await?;
        let items = menus
            .into_iter()
            .map(|menu| (menu, CurrentMenuActions::all()))
            .collect();
        return Ok(build_menu_tree(items));
    }

    let role_ids = unique_ids(user_roles.iter().map(|role| role.id));
    if role_ids.is_empty() {
        return Ok(Vec::new());
    }

    let role_menu_rows = role_menus::Entity::find()
        .filter(role_menus::Column::RoleId.is_in(role_ids))
        .all(db)
        .await?;
    let menu_ids = unique_ids(role_menu_rows.iter().map(|row| row.menu_id));
    if menu_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut action_by_menu = BTreeMap::new();
    for row in role_menu_rows {
        let entry = action_by_menu
            .entry(row.menu_id)
            .or_insert_with(empty_actions);
        merge_actions(entry, &row);
    }

    let menus = menus::Entity::find()
        .filter(menus::Column::Id.is_in(menu_ids))
        .filter(menus::Column::Enabled.eq(true))
        .filter(menus::Column::Visible.eq(true))
        .order_by_asc(menus::Column::SortOrder)
        .order_by_asc(menus::Column::Id)
        .all(db)
        .await?;

    let items = menus
        .into_iter()
        .filter_map(|menu| {
            action_by_menu
                .remove(&menu.id)
                .map(|actions| (menu, actions))
        })
        .collect();

    Ok(build_menu_tree(items))
}

pub async fn load_user_data_scopes(
    db: &DatabaseConnection,
    user_id: i32,
) -> ModelResult<Vec<data_scopes::Model>> {
    let user_roles = load_user_roles(db, user_id).await?;
    if has_super_admin_role(&user_roles) {
        return data_scopes::Entity::find()
            .order_by_asc(data_scopes::Column::Id)
            .all(db)
            .await
            .map_err(ModelError::from);
    }

    let role_ids = unique_ids(user_roles.iter().map(|role| role.id));
    if role_ids.is_empty() {
        return Ok(Vec::new());
    }

    let links = role_data_scopes::Entity::find()
        .filter(role_data_scopes::Column::RoleId.is_in(role_ids))
        .all(db)
        .await?;
    let data_scope_ids = unique_ids(links.iter().map(|link| link.data_scope_id));
    if data_scope_ids.is_empty() {
        return Ok(Vec::new());
    }

    data_scopes::Entity::find()
        .filter(data_scopes::Column::Id.is_in(data_scope_ids))
        .order_by_asc(data_scopes::Column::Id)
        .all(db)
        .await
        .map_err(ModelError::from)
}

pub async fn resolve_data_scope(
    db: &DatabaseConnection,
    user: &users::Model,
) -> ModelResult<EffectiveDataScope> {
    let roles = load_user_roles(db, user.id).await?;
    if has_super_admin_role(&roles) {
        return Ok(EffectiveDataScope::All);
    }

    let role_ids = unique_ids(roles.iter().map(|role| role.id));
    if role_ids.is_empty() {
        return Ok(EffectiveDataScope::None);
    }

    let links = role_data_scopes::Entity::find()
        .filter(role_data_scopes::Column::RoleId.is_in(role_ids))
        .all(db)
        .await?;
    let data_scope_ids = unique_ids(links.iter().map(|link| link.data_scope_id));
    if data_scope_ids.is_empty() {
        return Ok(EffectiveDataScope::None);
    }

    let scopes = data_scopes::Entity::find()
        .filter(data_scopes::Column::Id.is_in(data_scope_ids))
        .all(db)
        .await?;

    if scopes.iter().any(|scope| scope.code == "all") {
        return Ok(EffectiveDataScope::All);
    }
    if scopes.iter().any(|scope| scope.code == "tenant") {
        return user
            .tenant_id
            .map_or(Ok(EffectiveDataScope::None), |tenant_id| {
                Ok(EffectiveDataScope::Tenant { tenant_id })
            });
    }
    if scopes.iter().any(|scope| scope.code == "self") {
        return Ok(EffectiveDataScope::SelfOnly {
            user_id: user.id,
            tenant_id: user.tenant_id,
        });
    }

    Ok(EffectiveDataScope::None)
}

#[must_use]
pub fn data_scope_codes(scopes: Vec<data_scopes::Model>) -> Vec<String> {
    scopes.into_iter().map(|scope| scope.code).collect()
}

pub async fn is_super_admin(db: &DatabaseConnection, user_id: i32) -> ModelResult<bool> {
    load_user_roles(db, user_id)
        .await
        .map(|roles| has_super_admin_role(&roles))
}

pub async fn user_has_permission(
    db: &DatabaseConnection,
    user_id: i32,
    code: &str,
) -> ModelResult<bool> {
    let user_roles = load_user_roles(db, user_id).await?;
    if has_super_admin_role(&user_roles) {
        return Ok(true);
    }

    let role_ids = unique_ids(user_roles.iter().map(|role| role.id));
    if role_ids.is_empty() {
        return Ok(false);
    }

    let permission = permissions::Entity::find()
        .filter(permissions::Column::Code.eq(code))
        .one(db)
        .await?
        .ok_or_else(|| ModelError::EntityNotFound)?;

    let role_permission = role_permissions::Entity::find()
        .filter(role_permissions::Column::RoleId.is_in(role_ids))
        .filter(role_permissions::Column::PermissionId.eq(permission.id))
        .one(db)
        .await?;

    Ok(role_permission.is_some())
}

pub async fn assert_permission(db: &DatabaseConnection, user_id: i32, code: &str) -> ApiResult<()> {
    if user_has_permission(db, user_id, code).await? {
        Ok(())
    } else {
        Err(ApiError::forbidden("permission denied"))
    }
}

#[must_use]
pub fn to_current_roles(roles: Vec<roles::Model>) -> Vec<CurrentRole> {
    roles
        .into_iter()
        .map(|role| CurrentRole {
            id: role.id,
            name: role.name,
            code: role.code,
        })
        .collect()
}

#[must_use]
pub fn permission_codes(permissions: Vec<permissions::Model>) -> Vec<String> {
    permissions
        .into_iter()
        .map(|permission| permission.code)
        .collect()
}

fn has_super_admin_role(roles: &[roles::Model]) -> bool {
    roles.iter().any(|role| role.code == SUPER_ADMIN_ROLE)
}

fn unique_ids(ids: impl Iterator<Item = i32>) -> Vec<i32> {
    let mut ids = ids.collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();
    ids
}

const fn empty_actions() -> CurrentMenuActions {
    CurrentMenuActions {
        create: false,
        update: false,
        delete: false,
        import: false,
        export: false,
        print: false,
        help: false,
    }
}

const fn merge_actions(actions: &mut CurrentMenuActions, row: &role_menus::Model) {
    actions.create |= row.can_create;
    actions.update |= row.can_update;
    actions.delete |= row.can_delete;
    actions.import |= row.can_import;
    actions.export |= row.can_export;
    actions.print |= row.can_print;
    actions.help |= row.can_help;
}

fn build_menu_tree(items: Vec<(menus::Model, CurrentMenuActions)>) -> Vec<CurrentMenuItem> {
    let mut children_by_parent: BTreeMap<Option<i32>, Vec<(menus::Model, CurrentMenuActions)>> =
        BTreeMap::new();

    for item in items {
        children_by_parent
            .entry(item.0.parent_id)
            .or_default()
            .push(item);
    }

    build_menu_children(None, &children_by_parent)
}

fn build_menu_children(
    parent_id: Option<i32>,
    children_by_parent: &BTreeMap<Option<i32>, Vec<(menus::Model, CurrentMenuActions)>>,
) -> Vec<CurrentMenuItem> {
    children_by_parent
        .get(&parent_id)
        .map(|children| {
            children
                .iter()
                .map(|(menu, actions)| CurrentMenuItem {
                    id: menu.id,
                    key: menu.path.clone().unwrap_or_else(|| menu.id.to_string()),
                    label: menu.title.clone(),
                    title: menu.title.clone(),
                    path: menu.path.clone(),
                    icon: menu.icon.clone(),
                    permission: menu.permission_code.clone(),
                    permission_code: menu.permission_code.clone(),
                    actions: actions.clone(),
                    children: build_menu_children(Some(menu.id), children_by_parent),
                })
                .collect()
        })
        .unwrap_or_default()
}
