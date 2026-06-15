#![allow(clippy::struct_excessive_bools)]

use serde::{Deserialize, Serialize};

use crate::models::_entities::{data_scopes, departments, tenants, users};

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct LoginResponse {
    pub token: String,
    pub refresh_token: String,
    pub pid: String,
    pub name: String,
    pub is_verified: bool,
}

impl LoginResponse {
    #[must_use]
    pub fn new(user: &users::Model, token: &str, refresh_token: &str) -> Self {
        Self {
            token: token.to_string(),
            refresh_token: refresh_token.to_string(),
            pid: user.pid.to_string(),
            name: user.name.clone(),
            is_verified: user.email_verified_at.is_some(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct RefreshResponse {
    pub token: String,
    pub refresh_token: String,
}

impl RefreshResponse {
    #[must_use]
    pub fn new(token: &str, refresh_token: &str) -> Self {
        Self {
            token: token.to_string(),
            refresh_token: refresh_token.to_string(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CurrentMenuActions {
    pub create: bool,
    pub update: bool,
    pub delete: bool,
    pub import: bool,
    pub export: bool,
    pub print: bool,
    pub help: bool,
}

impl CurrentMenuActions {
    #[must_use]
    pub const fn all() -> Self {
        Self {
            create: true,
            update: true,
            delete: true,
            import: true,
            export: true,
            print: true,
            help: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CurrentMenuItem {
    pub id: i32,
    pub key: String,
    pub label: String,
    pub title: String,
    pub path: Option<String>,
    pub icon: Option<String>,
    pub permission: Option<String>,
    pub permission_code: Option<String>,
    pub actions: CurrentMenuActions,
    #[schema(no_recursion)]
    pub children: Vec<CurrentMenuItem>,
}

#[derive(Clone, Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CurrentRole {
    pub id: i32,
    pub name: String,
    pub code: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CurrentTenant {
    pub id: i32,
    pub name: String,
    pub code: String,
    pub departments_enabled: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CurrentDepartment {
    pub id: i32,
    pub tenant_id: i32,
    pub name: String,
    pub code: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CurrentDataScope {
    pub id: i32,
    pub name: String,
    pub code: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CurrentResponse {
    pub pid: String,
    pub name: String,
    pub email: String,
    pub roles: Vec<CurrentRole>,
    pub permissions: Vec<String>,
    pub menus: Vec<CurrentMenuItem>,
    pub tenant: Option<CurrentTenant>,
    pub departments: Vec<CurrentDepartment>,
    pub current_department: Option<CurrentDepartment>,
    pub data_scopes: Vec<CurrentDataScope>,
    pub effective_data_scope: String,
}

impl CurrentResponse {
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        user: &users::Model,
        roles: Vec<CurrentRole>,
        permissions: Vec<String>,
        menus: Vec<CurrentMenuItem>,
        tenant: Option<CurrentTenant>,
        departments: Vec<CurrentDepartment>,
        current_department: Option<CurrentDepartment>,
        data_scopes: Vec<CurrentDataScope>,
        effective_data_scope: String,
    ) -> Self {
        Self {
            pid: user.pid.to_string(),
            name: user.name.clone(),
            email: user.email.clone(),
            roles,
            permissions,
            menus,
            tenant,
            departments,
            current_department,
            data_scopes,
            effective_data_scope,
        }
    }
}

impl From<tenants::Model> for CurrentTenant {
    fn from(tenant: tenants::Model) -> Self {
        Self {
            id: tenant.id,
            name: tenant.name,
            code: tenant.code,
            departments_enabled: tenant.departments_enabled,
        }
    }
}

impl From<departments::Model> for CurrentDepartment {
    fn from(department: departments::Model) -> Self {
        Self {
            id: department.id,
            tenant_id: department.tenant_id,
            name: department.name,
            code: department.code,
        }
    }
}

impl From<data_scopes::Model> for CurrentDataScope {
    fn from(scope: data_scopes::Model) -> Self {
        Self {
            id: scope.id,
            name: scope.name,
            code: scope.code,
        }
    }
}
