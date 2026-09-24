//! Encrypted, per-user RDP credentials. Passwords never appear in API responses.
#![allow(clippy::missing_errors_doc)]
use super::_entities::rdp_credentials as entity;
use crate::errors::{ApiError, ApiResult};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::Utc;
use ring::{
    aead::{self, Aad, LessSafeKey, Nonce, UnboundKey},
    rand::{SecureRandom, SystemRandom},
};
use sea_orm::{
    sea_query::OnConflict, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Serialize, Deserialize)]
pub struct Credentials {
    pub username: String,
    pub password: String,
    pub domain: String,
}

fn key(secret: &str) -> ApiResult<LessSafeKey> {
    let mut hash = Sha256::new();
    hash.update(b"gpt-images:rdp-credentials:v1:");
    hash.update(secret.as_bytes());
    UnboundKey::new(&aead::AES_256_GCM, &hash.finalize())
        .map(LessSafeKey::new)
        .map_err(|_| ApiError::internal("无法初始化 RDP 凭据加密"))
}

fn encrypt(secret: &str, binding: &str, credentials: &Credentials) -> ApiResult<String> {
    let mut nonce = [0u8; 12];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| ApiError::internal("无法生成 RDP 凭据随机数"))?;
    let mut data =
        serde_json::to_vec(credentials).map_err(|_| ApiError::internal("无法编码 RDP 凭据"))?;
    key(secret)?
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(binding.as_bytes()),
            &mut data,
        )
        .map_err(|_| ApiError::internal("无法加密 RDP 凭据"))?;
    let mut payload = nonce.to_vec();
    payload.extend(data);
    Ok(format!("v1:{}", URL_SAFE_NO_PAD.encode(payload)))
}

fn decrypt(secret: &str, binding: &str, payload: &str) -> ApiResult<Credentials> {
    let invalid = || ApiError::bad_request("已保存凭据已失效，请重新输入并保存");
    let mut bytes = URL_SAFE_NO_PAD
        .decode(payload.strip_prefix("v1:").ok_or_else(invalid)?)
        .map_err(|_| invalid())?;
    if bytes.len() < 28 {
        return Err(invalid());
    }
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&bytes[..12]);
    let data = key(secret)?
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(binding.as_bytes()),
            &mut bytes[12..],
        )
        .map_err(|_| invalid())?;
    serde_json::from_slice(data).map_err(|_| invalid())
}

// Bind to user, target identity, host and port so an edited target cannot receive old credentials.
fn binding(user_id: i32, target_key: &str, endpoint: &str) -> String {
    serde_json::json!([user_id, target_key, endpoint]).to_string()
}

pub async fn save(
    db: &DatabaseConnection,
    secret: &str,
    user_id: i32,
    target_key: &str,
    endpoint: &str,
    credentials: &Credentials,
) -> ApiResult<()> {
    let payload = encrypt(secret, &binding(user_id, target_key, endpoint), credentials)?;
    entity::Entity::insert(entity::ActiveModel {
        user_id: Set(user_id),
        target_key: Set(target_key.to_owned()),
        encrypted_payload: Set(payload),
        created_at: Set(Utc::now().into()),
        updated_at: Set(Utc::now().into()),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::columns([entity::Column::UserId, entity::Column::TargetKey])
            .update_columns([entity::Column::EncryptedPayload, entity::Column::UpdatedAt])
            .to_owned(),
    )
    .exec_without_returning(db)
    .await?;
    Ok(())
}

pub async fn load(
    db: &DatabaseConnection,
    secret: &str,
    user_id: i32,
    target_key: &str,
    endpoint: &str,
) -> ApiResult<Option<Credentials>> {
    let record = entity::Entity::find()
        .filter(entity::Column::UserId.eq(user_id))
        .filter(entity::Column::TargetKey.eq(target_key))
        .one(db)
        .await?;
    // Changed encryption keys or endpoints require new credentials, without disclosing old passwords.
    Ok(record.and_then(|record| {
        decrypt(
            secret,
            &binding(user_id, target_key, endpoint),
            &record.encrypted_payload,
        )
        .ok()
    }))
}

pub async fn delete(db: &DatabaseConnection, user_id: i32, target_key: &str) -> ApiResult<()> {
    entity::Entity::delete_many()
        .filter(entity::Column::UserId.eq(user_id))
        .filter(entity::Column::TargetKey.eq(target_key))
        .exec(db)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn database_save_update_reload_and_delete_are_scoped_to_user() {
        use migration::{Migrator, MigratorTrait, SchemaManager};
        use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE users (id INTEGER PRIMARY KEY);".to_string(),
        ))
        .await
        .unwrap();
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO users (id) VALUES (7), (8);".to_string(),
        ))
        .await
        .unwrap();
        let migrations = Migrator::migrations();
        let migration = migrations
            .iter()
            .find(|m| m.name() == "m20260916_000023_rdp_credentials")
            .unwrap();
        let manager = SchemaManager::new(&db);
        migration.up(&manager).await.unwrap();
        let mut credentials = Credentials {
            username: "user".into(),
            password: "old-password".into(),
            domain: String::new(),
        };
        save(&db, "key", 7, "pc", "host:3389", &credentials)
            .await
            .unwrap();
        save(&db, "key", 8, "pc", "host:3389", &credentials)
            .await
            .unwrap();
        credentials.password = "new-password".into();
        save(&db, "key", 7, "pc", "host:3389", &credentials)
            .await
            .unwrap();
        assert_eq!(
            load(&db, "key", 7, "pc", "host:3389")
                .await
                .unwrap()
                .unwrap()
                .password,
            "new-password"
        );
        assert!(load(&db, "key", 7, "pc", "other:3389")
            .await
            .unwrap()
            .is_none());
        assert!(load(&db, "rotated-key", 7, "pc", "host:3389")
            .await
            .unwrap()
            .is_none());
        assert!(load(&db, "key", 7, "other", "host:3389")
            .await
            .unwrap()
            .is_none());
        delete(&db, 7, "pc").await.unwrap();
        assert!(load(&db, "key", 7, "pc", "host:3389")
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            load(&db, "key", 8, "pc", "host:3389")
                .await
                .unwrap()
                .unwrap()
                .password,
            "old-password"
        );
        migration.down(&manager).await.unwrap();
    }
    #[test]
    fn round_trip_is_encrypted_randomized_and_bound_to_user_and_endpoint() {
        let credentials = Credentials {
            username: "user".into(),
            password: "secret中文🔑".into(),
            domain: "DOMAIN".into(),
        };
        let aad = binding(7, "pc", "host:3389");
        let payload = encrypt("key", &aad, &credentials).unwrap();
        assert!(!payload.contains(&credentials.password));
        assert_ne!(payload, encrypt("key", &aad, &credentials).unwrap());
        let restored = decrypt("key", &aad, &payload).unwrap();
        assert_eq!(restored.password, credentials.password);
        assert_eq!(restored.username, credentials.username);
        assert_eq!(restored.domain, credentials.domain);
        for other in [
            binding(8, "pc", "host:3389"),
            binding(7, "other", "host:3389"),
            binding(7, "pc", "changed:3389"),
        ] {
            assert!(decrypt("key", &other, &payload).is_err());
        }
        assert!(decrypt("other-key", &aad, &payload).is_err());
        assert!(decrypt("key", &aad, "v1:AAAA").is_err());
        let mut tampered = URL_SAFE_NO_PAD
            .decode(payload.strip_prefix("v1:").unwrap())
            .unwrap();
        tampered[15] ^= 1;
        assert!(decrypt(
            "key",
            &aad,
            &format!("v1:{}", URL_SAFE_NO_PAD.encode(tampered))
        )
        .is_err());
    }
}
