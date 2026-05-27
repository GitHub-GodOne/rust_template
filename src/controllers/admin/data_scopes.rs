#![allow(clippy::missing_errors_doc)]

use loco_rs::prelude::*;
use sea_orm::{EntityTrait, QueryOrder};
use serde::{Deserialize, Serialize};

use crate::{
    controllers::admin::authorize,
    errors::ApiResult,
    models::_entities::data_scopes,
    responses::{self, ApiResponse},
};

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct DataScopeRecord {
    pub id: i32,
    pub name: String,
    pub code: String,
    pub rule: Option<String>,
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[utoipa::path(
    get,
    path = "/api/admin/data-scopes",
    tag = "admin-data-scopes",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<Vec<DataScopeRecord>>))
)]
#[debug_handler]
pub async fn list(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:data_scope:list").await?;
    let items = data_scopes::Entity::find()
        .order_by_asc(data_scopes::Column::Id)
        .all(&ctx.db)
        .await?
        .into_iter()
        .map(DataScopeRecord::from)
        .collect::<Vec<_>>();

    Ok(responses::ok(items))
}

impl From<data_scopes::Model> for DataScopeRecord {
    fn from(scope: data_scopes::Model) -> Self {
        Self {
            id: scope.id,
            name: scope.name,
            code: scope.code,
            rule: scope.rule,
            description: scope.description,
            created_at: scope.created_at.to_rfc3339(),
            updated_at: scope.updated_at.to_rfc3339(),
        }
    }
}
