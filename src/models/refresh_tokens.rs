#![allow(clippy::missing_errors_doc)]

use chrono::{offset::Local, Duration};
use loco_rs::{hash, model::ModelResult, prelude::*};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, Set};
use sha2::{Digest, Sha256};

pub use super::_entities::refresh_tokens::{self, ActiveModel, Entity, Model};
use super::_entities::users;

pub const REFRESH_TOKEN_DAYS: i64 = 30;
pub const REFRESH_TOKEN_LENGTH: usize = 64;

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
        let token = hash::random_string(REFRESH_TOKEN_LENGTH);
        let token_hash = Self::hash_token(&token);
        let expires_at = Local::now() + Duration::days(REFRESH_TOKEN_DAYS);

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
