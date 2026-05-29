use async_trait::async_trait;
use chrono::offset::Local;
use loco_rs::{
    app::AppContext,
    hash,
    task::{Task, TaskInfo, Vars},
    Error, Result,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, DatabaseConnection, EntityTrait, IntoActiveModel,
    QueryFilter, TransactionTrait,
};

use crate::models::_entities::{roles, user_roles, users};

const DEFAULT_ROLE_CODE: &str = "super_admin";

pub struct RecoverAdmin;

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
