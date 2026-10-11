use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use chrono::offset::Local;
use loco_rs::{
    app::AppContext,
    hash,
    task::{Task, TaskInfo, Vars},
    Error, Result,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait, DatabaseBackend,
    DatabaseConnection, EntityTrait, IntoActiveModel, QueryFilter, Statement, TransactionTrait,
};
use serde::Deserialize;

use crate::models::_entities::{
    menus, permissions, role_menus, role_permissions, roles, user_roles, users,
};

const DEFAULT_ROLE_CODE: &str = "super_admin";
const PERMISSIONS_FIXTURE: &str = include_str!("../fixtures/permissions.yaml");
const MENUS_FIXTURE: &str = include_str!("../fixtures/menus.yaml");
const ROLE_PERMISSIONS_FIXTURE: &str = include_str!("../fixtures/role_permissions.yaml");
const ROLE_MENUS_FIXTURE: &str = include_str!("../fixtures/role_menus.yaml");

pub struct RecoverAdmin;
pub struct UnlockAdmin;
pub struct SyncFixtureBaseline;

#[async_trait]
impl Task for RecoverAdmin {
    fn task(&self) -> TaskInfo {
        TaskInfo {
            name: "admin.recover".to_string(),
            detail: "Create or reset an admin user and bind the super admin role".to_string(),
        }
    }

    async fn run(&self, ctx: &AppContext, vars: &Vars) -> Result<()> {
        let email = required_arg(vars, "email")?;
        let password = required_arg(vars, "password")?;
        let name = vars.cli.get("name").map(String::as_str);
        let role_code = vars
            .cli
            .get("role-code")
            .map_or(DEFAULT_ROLE_CODE, String::as_str);
        let tenant_id = optional_i32_arg(vars, "tenant-id")?;

        let user = recover_admin(&ctx.db, email, password, name, role_code, tenant_id).await?;
        tracing::info!(
            user_id = user.id,
            email = user.email,
            role_code,
            "admin recovery completed"
        );
        Ok(())
    }
}

#[async_trait]
impl Task for UnlockAdmin {
    fn task(&self) -> TaskInfo {
        TaskInfo {
            name: "admin.unlock".to_string(),
            detail: "Clear login failures and unlock an admin account".to_string(),
        }
    }

    async fn run(&self, ctx: &AppContext, vars: &Vars) -> Result<()> {
        let email = required_arg(vars, "email")?;
        let user = unlock_admin(&ctx.db, email).await?;
        tracing::info!(
            user_id = user.id,
            email = user.email,
            "admin account unlocked"
        );
        Ok(())
    }
}

#[async_trait]
impl Task for SyncFixtureBaseline {
    fn task(&self) -> TaskInfo {
        TaskInfo {
            name: "admin.sync_fixture_baseline".to_string(),
            detail: "Upsert fixture permissions, menus, and role grants without truncating business data"
                .to_string(),
        }
    }

    async fn run(&self, ctx: &AppContext, _vars: &Vars) -> Result<()> {
        let permissions_rows = parse_fixture::<permissions::Model>(PERMISSIONS_FIXTURE)?;
        let menu_rows = parse_fixture::<menus::Model>(MENUS_FIXTURE)?;
        let role_permission_rows =
            parse_fixture::<role_permissions::Model>(ROLE_PERMISSIONS_FIXTURE)?;
        let role_menu_rows = parse_fixture::<role_menus::Model>(ROLE_MENUS_FIXTURE)?;

        let txn = ctx.db.begin().await?;

        let mut synced_permissions = 0usize;
        for row in &permissions_rows {
            sync_permission(&txn, row).await?;
            synced_permissions += 1;
        }

        let menu_ids = sync_menu_layout(&txn, &menu_rows).await?;

        let mut synced_role_permissions = 0usize;
        for row in &role_permission_rows {
            if sync_role_permission(&txn, row).await? {
                synced_role_permissions += 1;
            }
        }

        let mut synced_role_menus = 0usize;
        for row in &role_menu_rows {
            let mut mapped = row.clone();
            mapped.menu_id = *menu_ids.get(&row.menu_id).ok_or_else(|| {
                Error::Message(format!(
                    "role menu references missing fixture menu {}",
                    row.menu_id
                ))
            })?;
            if sync_role_menu(&txn, &mapped).await? {
                synced_role_menus += 1;
            }
        }

        reset_pk_sequence(&txn, "permissions", "id").await?;
        reset_pk_sequence(&txn, "menus", "id").await?;

        txn.commit().await?;

        tracing::info!(
            permissions = synced_permissions,
            menus = menu_ids.len(),
            fixture_source = "embedded",
            role_permissions = synced_role_permissions,
            role_menus = synced_role_menus,
            "fixture baseline sync completed"
        );
        Ok(())
    }
}

/// Creates or resets a user and ensures it has the requested admin role.
///
/// # Errors
///
/// Returns an error when required arguments are invalid, the role is missing,
/// password hashing fails, or a database operation fails.
pub async fn recover_admin(
    db: &DatabaseConnection,
    email: &str,
    password: &str,
    name: Option<&str>,
    role_code: &str,
    tenant_id: Option<i32>,
) -> Result<users::Model> {
    let email = email.trim().to_ascii_lowercase();
    let role_code = role_code.trim();
    if email.is_empty() || password.trim().is_empty() || role_code.is_empty() {
        return Err(Error::Message(
            "email, password and role-code are required".to_string(),
        ));
    }

    let txn = db.begin().await?;
    let role = roles::Entity::find()
        .filter(roles::Column::Code.eq(role_code))
        .filter(roles::Column::Enabled.eq(true))
        .one(&txn)
        .await?
        .ok_or_else(|| Error::Message(format!("enabled role {role_code} not found")))?;
    let password_hash =
        hash::hash_password(password).map_err(|err| Error::Message(err.to_string()))?;
    let verified_at = Some(Local::now().into());

    let user = if let Some(existing) = users::Entity::find()
        .filter(users::Column::Email.eq(&email))
        .one(&txn)
        .await?
    {
        let mut active = existing.into_active_model();
        active.password = ActiveValue::set(password_hash);
        active.email_verified_at = ActiveValue::set(verified_at);
        active.reset_token = ActiveValue::set(None);
        active.reset_sent_at = ActiveValue::set(None);
        active.failed_login_attempts = ActiveValue::set(0);
        active.locked_until = ActiveValue::set(None);
        active.last_failed_login_at = ActiveValue::set(None);
        if let Some(name) = name.filter(|value| !value.trim().is_empty()) {
            active.name = ActiveValue::set(name.trim().to_string());
        }
        if tenant_id.is_some() {
            active.tenant_id = ActiveValue::set(tenant_id);
        }
        active.update(&txn).await?
    } else {
        users::ActiveModel {
            email: ActiveValue::set(email.clone()),
            password: ActiveValue::set(password_hash),
            name: ActiveValue::set(default_name(&email, name)),
            email_verified_at: ActiveValue::set(verified_at),
            tenant_id: ActiveValue::set(tenant_id),
            ..Default::default()
        }
        .insert(&txn)
        .await?
    };

    if user_roles::Entity::find()
        .filter(user_roles::Column::UserId.eq(user.id))
        .filter(user_roles::Column::RoleId.eq(role.id))
        .one(&txn)
        .await?
        .is_none()
    {
        user_roles::ActiveModel {
            user_id: ActiveValue::set(user.id),
            role_id: ActiveValue::set(role.id),
            ..Default::default()
        }
        .insert(&txn)
        .await?;
    }

    txn.commit().await?;
    Ok(user)
}

/// Clears login failure counters and unlocks an existing user without changing the password.
///
/// # Errors
///
/// Returns an error when the email is empty, the user does not exist, or the database update fails.
pub async fn unlock_admin(db: &DatabaseConnection, email: &str) -> Result<users::Model> {
    let email = email.trim().to_ascii_lowercase();
    if email.is_empty() {
        return Err(Error::Message("email is required".to_string()));
    }

    let user = users::Entity::find()
        .filter(users::Column::Email.eq(&email))
        .one(db)
        .await?
        .ok_or_else(|| Error::Message(format!("user {email} not found")))?;
    let mut active = user.into_active_model();
    active.failed_login_attempts = ActiveValue::set(0);
    active.locked_until = ActiveValue::set(None);
    active.last_failed_login_at = ActiveValue::set(None);
    Ok(active.update(db).await?)
}

fn required_arg<'a>(vars: &'a Vars, key: &str) -> Result<&'a str> {
    vars.cli_arg(key).map(std::string::String::as_str)
}

fn optional_i32_arg(vars: &Vars, key: &str) -> Result<Option<i32>> {
    vars.cli
        .get(key)
        .map(|value| {
            value
                .parse::<i32>()
                .map_err(|_| Error::Message(format!("{key} must be an integer")))
        })
        .transpose()
}

fn default_name(email: &str, name: Option<&str>) -> String {
    name.filter(|value| !value.trim().is_empty()).map_or_else(
        || email.split('@').next().unwrap_or("admin").to_string(),
        |value| value.trim().to_string(),
    )
}

fn parse_fixture<T>(input: &str) -> Result<Vec<T>>
where
    T: for<'de> Deserialize<'de>,
{
    serde_yaml::from_str::<Vec<T>>(input)
        .map_err(|error| Error::Message(format!("failed to parse fixture yaml: {error}")))
}

async fn sync_permission<C>(db: &C, row: &permissions::Model) -> Result<()>
where
    C: ConnectionTrait,
{
    let existing = permissions::Entity::find_by_id(row.id).one(db).await?;
    if let Some(existing) = existing {
        ensure_permission_identity(&existing, row)?;
        let mut active = existing.into_active_model();
        active.name = ActiveValue::set(row.name.clone());
        active.code = ActiveValue::set(row.code.clone());
        active.group_name = ActiveValue::set(row.group_name.clone());
        active.description = ActiveValue::set(row.description.clone());
        active.created_at = ActiveValue::set(row.created_at);
        active.updated_at = ActiveValue::set(row.updated_at);
        active.update(db).await?;
        return Ok(());
    }

    if let Some(existing) = permissions::Entity::find()
        .filter(permissions::Column::Code.eq(&row.code))
        .one(db)
        .await?
    {
        return Err(Error::Message(format!(
            "permission code {} already exists with id {}, expected {}",
            row.code, existing.id, row.id
        )));
    }

    row.clone().into_active_model().insert(db).await?;
    Ok(())
}

fn order_menu_rows(rows: &[menus::Model]) -> Result<Vec<&menus::Model>> {
    let ids = rows.iter().map(|row| row.id).collect::<BTreeSet<_>>();
    if ids.len() != rows.len() {
        return Err(Error::Message("duplicate menu IDs in fixture".to_string()));
    }
    let mut ordered = Vec::with_capacity(rows.len());
    let mut resolved = BTreeSet::new();
    while ordered.len() < rows.len() {
        let before = ordered.len();
        for row in rows {
            if !resolved.contains(&row.id)
                && row
                    .parent_id
                    .is_none_or(|parent| resolved.contains(&parent))
            {
                ordered.push(row);
                resolved.insert(row.id);
            }
        }
        if before == ordered.len() {
            return Err(Error::Message(
                "menu fixture contains a cycle or a missing parent".to_string(),
            ));
        }
    }
    Ok(ordered)
}

async fn sync_menu_layout<C>(db: &C, rows: &[menus::Model]) -> Result<BTreeMap<i32, i32>>
where
    C: ConnectionTrait,
{
    let mut ids = BTreeMap::new();
    for row in order_menu_rows(rows)? {
        let parent_id = row.parent_id.map(|parent| ids[&parent]);
        let actual_id = sync_menu(db, row, parent_id).await?;
        if ids.values().any(|id| *id == actual_id) {
            return Err(Error::Message(
                "duplicate menu identity in fixture".to_string(),
            ));
        }
        ids.insert(row.id, actual_id);
    }
    Ok(ids)
}

async fn sync_menu<C>(db: &C, row: &menus::Model, parent_id: Option<i32>) -> Result<i32>
where
    C: ConnectionTrait,
{
    // IDs can differ between machines. Routes identify pages; title and parent
    // identify groups. Never overwrite a different menu just because its ID matches.
    let query = menus::Entity::find();
    let query = if let Some(path) = row.path.as_deref() {
        query.filter(menus::Column::Path.eq(path))
    } else {
        let query = query
            .filter(menus::Column::Path.is_null())
            .filter(menus::Column::Title.eq(&row.title));
        if let Some(parent) = parent_id {
            query.filter(menus::Column::ParentId.eq(parent))
        } else {
            query.filter(menus::Column::ParentId.is_null())
        }
    };
    let mut matches = query.all(db).await?;
    if matches.len() > 1 {
        return Err(Error::Message(format!(
            "ambiguous menu identity: {}",
            row.title
        )));
    }
    if let Some(existing) = matches.pop() {
        let id = existing.id;
        let mut active = existing.into_active_model();
        active.parent_id = ActiveValue::set(parent_id);
        active.title = ActiveValue::set(row.title.clone());
        active.path = ActiveValue::set(row.path.clone());
        active.icon = ActiveValue::set(row.icon.clone());
        active.permission_code = ActiveValue::set(row.permission_code.clone());
        active.sort_order = ActiveValue::set(row.sort_order);
        active.visible = ActiveValue::set(row.visible);
        active.enabled = ActiveValue::set(row.enabled);
        active.created_at = ActiveValue::set(row.created_at);
        active.updated_at = ActiveValue::set(row.updated_at);
        active.update(db).await?;
        return Ok(id);
    }

    let mut active = row.clone().into_active_model();
    active.parent_id = ActiveValue::set(parent_id);
    if menus::Entity::find_by_id(row.id).one(db).await?.is_some() {
        reset_pk_sequence(db, "menus", "id").await?;
        active.id = ActiveValue::NotSet;
    }
    Ok(active.insert(db).await?.id)
}

async fn sync_role_permission<C>(db: &C, row: &role_permissions::Model) -> Result<bool>
where
    C: ConnectionTrait,
{
    if roles::Entity::find_by_id(row.role_id)
        .one(db)
        .await?
        .is_none()
    {
        tracing::warn!(
            role_id = row.role_id,
            "skip role permission fixture because role is missing"
        );
        return Ok(false);
    }
    if permissions::Entity::find_by_id(row.permission_id)
        .one(db)
        .await?
        .is_none()
    {
        tracing::warn!(
            permission_id = row.permission_id,
            "skip role permission fixture because permission is missing"
        );
        return Ok(false);
    }
    if role_permissions::Entity::find_by_id((row.role_id, row.permission_id))
        .one(db)
        .await?
        .is_some()
    {
        return Ok(true);
    }
    row.clone().into_active_model().insert(db).await?;
    Ok(true)
}

async fn sync_role_menu<C>(db: &C, row: &role_menus::Model) -> Result<bool>
where
    C: ConnectionTrait,
{
    if roles::Entity::find_by_id(row.role_id)
        .one(db)
        .await?
        .is_none()
    {
        tracing::warn!(
            role_id = row.role_id,
            "skip role menu fixture because role is missing"
        );
        return Ok(false);
    }
    if menus::Entity::find_by_id(row.menu_id)
        .one(db)
        .await?
        .is_none()
    {
        tracing::warn!(
            menu_id = row.menu_id,
            "skip role menu fixture because menu is missing"
        );
        return Ok(false);
    }
    if let Some(existing) = role_menus::Entity::find_by_id((row.role_id, row.menu_id))
        .one(db)
        .await?
    {
        let mut active = existing.into_active_model();
        active.can_create = ActiveValue::set(row.can_create);
        active.can_update = ActiveValue::set(row.can_update);
        active.can_delete = ActiveValue::set(row.can_delete);
        active.can_import = ActiveValue::set(row.can_import);
        active.can_export = ActiveValue::set(row.can_export);
        active.can_print = ActiveValue::set(row.can_print);
        active.can_help = ActiveValue::set(row.can_help);
        active.created_at = ActiveValue::set(row.created_at);
        active.updated_at = ActiveValue::set(row.updated_at);
        active.update(db).await?;
        return Ok(true);
    }
    row.clone().into_active_model().insert(db).await?;
    Ok(true)
}

fn ensure_permission_identity(
    existing: &permissions::Model,
    row: &permissions::Model,
) -> Result<()> {
    if existing.code != row.code {
        return Err(Error::Message(format!(
            "permission id {} has code {}, fixture expects {}",
            existing.id, existing.code, row.code
        )));
    }
    Ok(())
}

async fn reset_pk_sequence<C>(db: &C, table: &str, column: &str) -> Result<()>
where
    C: ConnectionTrait,
{
    if db.get_database_backend() != DatabaseBackend::Postgres {
        return Ok(());
    }

    let statement = Statement::from_string(
        DatabaseBackend::Postgres,
        format!(
            "SELECT setval(pg_get_serial_sequence('{table}', '{column}'), COALESCE((SELECT MAX({column}) FROM {table}), 1), true)"
        ),
    );
    db.execute(statement).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use migration::{Migrator, MigratorTrait, SchemaManager};
    use sea_orm::{Database, DbBackend, PaginatorTrait};

    async fn menu_database() -> DatabaseConnection {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE users (id INTEGER PRIMARY KEY)".to_string(),
        ))
        .await
        .unwrap();
        let manager = SchemaManager::new(&db);
        let migration = Migrator::migrations()
            .into_iter()
            .find(|migration| migration.name() == "m20260527_000002_rbac")
            .unwrap();
        migration.up(&manager).await.unwrap();
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE roles ADD COLUMN tenant_id INTEGER".to_string(),
        ))
        .await
        .unwrap();
        let role = parse_fixture::<roles::Model>(include_str!("../fixtures/roles.yaml"))
            .unwrap()
            .remove(0);
        role.into_active_model().insert(&db).await.unwrap();
        db
    }

    fn fixture_menus() -> Vec<menus::Model> {
        parse_fixture(MENUS_FIXTURE).unwrap()
    }

    async fn apply_layout(db: &DatabaseConnection, rows: &[menus::Model]) -> BTreeMap<i32, i32> {
        let txn = db.begin().await.unwrap();
        let ids = sync_menu_layout(&txn, rows).await.unwrap();
        for mut grant in parse_fixture::<role_menus::Model>(ROLE_MENUS_FIXTURE).unwrap() {
            grant.menu_id = ids[&grant.menu_id];
            assert!(sync_role_menu(&txn, &grant).await.unwrap());
        }
        txn.commit().await.unwrap();
        ids
    }

    #[test]
    fn embedded_layout_has_the_requested_groups_and_complete_grants() {
        let rows = fixture_menus();
        let mut roots = rows
            .iter()
            .filter(|row| row.parent_id.is_none())
            .collect::<Vec<_>>();
        roots.sort_by_key(|row| (row.sort_order, row.id));
        assert_eq!(
            roots
                .iter()
                .map(|row| row.title.as_str())
                .collect::<Vec<_>>(),
            [
                "首页",
                "存储管理",
                "接口管理",
                "便捷管理",
                "系统管理",
                "远程管理"
            ]
        );
        let grant_ids = parse_fixture::<role_menus::Model>(ROLE_MENUS_FIXTURE)
            .unwrap()
            .into_iter()
            .filter(|row| row.role_id == 1)
            .map(|row| row.menu_id)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            rows.iter().map(|row| row.id).collect::<BTreeSet<_>>(),
            grant_ids
        );
        assert_eq!(
            rows.iter()
                .find(|row| row.path.as_deref() == Some("/admin/system/ssh"))
                .unwrap()
                .parent_id,
            Some(43)
        );
        assert_eq!(
            rows.iter()
                .find(|row| row.path.as_deref() == Some("/admin/system/uploads"))
                .unwrap()
                .parent_id,
            Some(40)
        );
    }

    #[tokio::test]
    async fn upgrades_flat_menus_and_preserves_existing_ids_and_grants() {
        let db = menu_database().await;
        let rows = fixture_menus();
        for row in rows.iter().filter(|row| row.id <= 35) {
            let mut old = row.clone();
            if old.id > 2 {
                old.parent_id = Some(2);
            }
            old.sort_order = old.id * 10;
            old.into_active_model().insert(&db).await.unwrap();
        }
        let original = parse_fixture::<role_menus::Model>(ROLE_MENUS_FIXTURE)
            .unwrap()
            .into_iter()
            .find(|row| row.menu_id == 24)
            .unwrap();
        original.into_active_model().insert(&db).await.unwrap();
        let ids = apply_layout(&db, &rows).await;
        assert!(ids.iter().all(|(fixture, actual)| fixture == actual));
        let ssh = menus::Entity::find_by_id(24)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ssh.parent_id, Some(43));
        assert_eq!(ssh.sort_order, 10);
        assert!(role_menus::Entity::find_by_id((1, 24))
            .one(&db)
            .await
            .unwrap()
            .is_some());
        assert_eq!(menus::Entity::find().count(&db).await.unwrap(), 39);
        assert_eq!(role_menus::Entity::find().count(&db).await.unwrap(), 39);
        assert_eq!(apply_layout(&db, &rows).await, ids);
        assert_eq!(menus::Entity::find().count(&db).await.unwrap(), 39);
        assert_eq!(role_menus::Entity::find().count(&db).await.unwrap(), 39);
    }

    #[tokio::test]
    async fn maps_different_menu_ids_without_overwriting_custom_menus() {
        let db = menu_database().await;
        let rows = fixture_menus();
        // Simulate a second machine with different IDs, including a route whose
        // fixture ID is occupied by a custom group. References must use actual IDs.
        let group = rows.iter().find(|row| row.id == 43).unwrap();
        let mut existing_group = group.clone();
        existing_group.id = 100;
        existing_group
            .into_active_model()
            .insert(&db)
            .await
            .unwrap();
        let ssh = rows.iter().find(|row| row.id == 24).unwrap();
        let mut existing_ssh = ssh.clone();
        existing_ssh.id = 101;
        existing_ssh.parent_id = Some(100);
        existing_ssh.sort_order = 999;
        existing_ssh.into_active_model().insert(&db).await.unwrap();
        let mut custom = group.clone();
        custom.id = 32;
        custom.title = "自定义分组".to_string();
        custom.into_active_model().insert(&db).await.unwrap();
        let mut colliding_group = group.clone();
        colliding_group.id = 40;
        colliding_group.title = "其他自定义分组".to_string();
        colliding_group
            .into_active_model()
            .insert(&db)
            .await
            .unwrap();
        let mut reversed = rows.clone();
        reversed.reverse();
        let ids = apply_layout(&db, &reversed).await;
        assert_eq!(ids[&43], 100);
        assert_eq!(ids[&24], 101);
        assert_ne!(ids[&32], 32);
        assert_ne!(ids[&40], 40);
        let actual_ssh = menus::Entity::find_by_id(101)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(actual_ssh.parent_id, Some(100));
        assert_eq!(actual_ssh.sort_order, 10);
        let uploads = menus::Entity::find_by_id(ids[&8])
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(uploads.parent_id, Some(ids[&40]));
        assert_eq!(
            menus::Entity::find_by_id(32)
                .one(&db)
                .await
                .unwrap()
                .unwrap()
                .title,
            "自定义分组"
        );
        assert_eq!(
            menus::Entity::find_by_id(40)
                .one(&db)
                .await
                .unwrap()
                .unwrap()
                .title,
            "其他自定义分组"
        );
        assert!(role_menus::Entity::find_by_id((1, 101))
            .one(&db)
            .await
            .unwrap()
            .is_some());
        assert!(role_menus::Entity::find_by_id((1, ids[&32]))
            .one(&db)
            .await
            .unwrap()
            .is_some());
        assert_eq!(menus::Entity::find().count(&db).await.unwrap(), 41);
        assert_eq!(apply_layout(&db, &rows).await, ids);
        assert_eq!(menus::Entity::find().count(&db).await.unwrap(), 41);
    }

    #[test]
    fn invalid_menu_parents_and_duplicate_ids_are_rejected() {
        let mut rows = fixture_menus();
        rows[0].parent_id = Some(9999);
        assert!(order_menu_rows(&rows).is_err());
        rows[0].parent_id = Some(rows[0].id);
        assert!(order_menu_rows(&rows).is_err());
        rows[0].parent_id = None;
        rows.push(rows[0].clone());
        assert!(order_menu_rows(&rows).is_err());
    }

    #[tokio::test]
    async fn ambiguous_menu_identity_rolls_back_prior_updates() {
        let db = menu_database().await;
        let rows = fixture_menus();
        let mut root = rows[0].clone();
        root.sort_order = 999;
        root.into_active_model().insert(&db).await.unwrap();
        let ssh = rows.iter().find(|row| row.id == 24).unwrap();
        for id in [100, 101] {
            let mut duplicate = ssh.clone();
            duplicate.id = id;
            duplicate.parent_id = None;
            duplicate.into_active_model().insert(&db).await.unwrap();
        }
        let txn = db.begin().await.unwrap();
        assert!(sync_menu_layout(&txn, &rows).await.is_err());
        txn.rollback().await.unwrap();
        assert_eq!(
            menus::Entity::find_by_id(1)
                .one(&db)
                .await
                .unwrap()
                .unwrap()
                .sort_order,
            999
        );
        assert_eq!(menus::Entity::find().count(&db).await.unwrap(), 3);
    }
}
