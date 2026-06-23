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

        let mut synced_menus = 0usize;
        for row in &menu_rows {
            sync_menu(&txn, row).await?;
            synced_menus += 1;
        }

        let mut synced_role_permissions = 0usize;
        for row in &role_permission_rows {
            if sync_role_permission(&txn, row).await? {
                synced_role_permissions += 1;
            }
        }

        let mut synced_role_menus = 0usize;
        for row in &role_menu_rows {
            if sync_role_menu(&txn, row).await? {
                synced_role_menus += 1;
            }
        }

        reset_pk_sequence(&txn, "permissions", "id").await?;
        reset_pk_sequence(&txn, "menus", "id").await?;

        txn.commit().await?;

        tracing::info!(
            permissions = synced_permissions,
            menus = synced_menus,
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

async fn sync_menu<C>(db: &C, row: &menus::Model) -> Result<()>
where
    C: ConnectionTrait,
{
    let existing = menus::Entity::find_by_id(row.id).one(db).await?;
    if let Some(existing) = existing {
        ensure_menu_identity(&existing, row)?;
        let mut active = existing.into_active_model();
        active.parent_id = ActiveValue::set(row.parent_id);
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
        return Ok(());
    }

    if let Some(path) = row.path.as_deref() {
        if let Some(existing) = menus::Entity::find()
            .filter(menus::Column::Path.eq(path))
            .one(db)
            .await?
        {
            return Err(Error::Message(format!(
                "menu path {} already exists with id {}, expected {}",
                path, existing.id, row.id
            )));
        }
    }

    row.clone().into_active_model().insert(db).await?;
    Ok(())
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

fn ensure_menu_identity(existing: &menus::Model, row: &menus::Model) -> Result<()> {
    if existing.path != row.path && existing.path.is_some() && row.path.is_some() {
        return Err(Error::Message(format!(
            "menu id {} has path {:?}, fixture expects {:?}",
            existing.id, existing.path, row.path
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
