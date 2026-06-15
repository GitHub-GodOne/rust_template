#![allow(clippy::missing_errors_doc)]

use chrono::{offset::Local, Duration};
use loco_rs::{hash, model::ModelResult, prelude::*};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, Set};
use sha2::{Digest, Sha256};

pub use super::_entities::refresh_tokens::{self, ActiveModel, Entity, Model};
use super::{_entities::users, system_settings};

#[derive(Debug, Clone)]
pub struct IssuedRefreshToken {
    pub token: String,
    pub model: Model,
}

#[async_trait::async_trait]
impl ActiveModelBehavior for ActiveModel {}

impl Model {
    #[must_use]
    pub fn hash_token(token: &str) -> String {
        let digest = Sha256::digest(token.as_bytes());
        hex::encode(digest)
    }

    pub async fn issue(
        db: &DatabaseConnection,
        user: &users::Model,
    ) -> ModelResult<IssuedRefreshToken> {
        let token_length = usize::try_from(
            system_settings::number_i64(db, "auth.refresh_token_length", 64)
                .await?
                .clamp(32, 256),
        )
        .unwrap_or(64);
        let expires_days = system_settings::number_i64(db, "auth.refresh_token_days", 30)
            .await?
            .clamp(1, 365);
        let token = hash::random_string(token_length);
        let token_hash = Self::hash_token(&token);
        let expires_at = Local::now() + Duration::days(expires_days);

        let model = refresh_tokens::ActiveModel {
            user_id: Set(user.id),
            token_hash: Set(token_hash),
            expires_at: Set(expires_at.into()),
            ..Default::default()
        }
        .insert(db)
        .await?;

        Ok(IssuedRefreshToken { token, model })
    }

    pub async fn find_valid_by_token(db: &DatabaseConnection, token: &str) -> ModelResult<Self> {
        let token_hash = Self::hash_token(token);
        let model = refresh_tokens::Entity::find()
            .filter(refresh_tokens::Column::TokenHash.eq(token_hash))
            .one(db)
            .await?
            .ok_or_else(|| ModelError::EntityNotFound)?;

        if model.revoked_at.is_some() || model.expires_at <= Local::now() {
            return Err(ModelError::EntityNotFound);
        }

        Ok(model)
    }

    pub async fn revoke(db: &DatabaseConnection, token: &str) -> ModelResult<()> {
        let token = Self::find_valid_by_token(db, token).await?;
        token.into_active_model().revoke(db).await?;
        Ok(())
    }
}

impl ActiveModel {
    pub async fn revoke(mut self, db: &DatabaseConnection) -> ModelResult<Model> {
        self.revoked_at = Set(Some(Local::now().into()));
        self.update(db).await.map_err(ModelError::from)
    }
}
