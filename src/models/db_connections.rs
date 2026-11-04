//! Saved database connections. Passwords are encrypted with a key derived
//! from the JWT secret and bound to the owner and the connection id, and
//! never appear in API responses.
#![allow(clippy::missing_errors_doc)]
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ring::{
    aead::{self, Aad, LessSafeKey, Nonce, UnboundKey},
    rand::{SecureRandom, SystemRandom},
};
use sha2::{Digest, Sha256};

use crate::errors::{ApiError, ApiResult};

fn key(secret: &str) -> ApiResult<LessSafeKey> {
    let mut hash = Sha256::new();
    hash.update(b"gpt-images:db-connections:v1:");
    hash.update(secret.as_bytes());
    UnboundKey::new(&aead::AES_256_GCM, &hash.finalize())
        .map(LessSafeKey::new)
        .map_err(|_| ApiError::internal("无法初始化数据库密码加密"))
}

fn binding(user_id: i32, connection_id: i32) -> String {
    serde_json::json!([user_id, connection_id]).to_string()
}

pub fn encrypt_password(
    secret: &str,
    user_id: i32,
    connection_id: i32,
    password: &str,
) -> ApiResult<String> {
    let mut nonce = [0u8; 12];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| ApiError::internal("无法生成数据库密码随机数"))?;
    let mut data = password.as_bytes().to_vec();
    key(secret)?
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(binding(user_id, connection_id).as_bytes()),
            &mut data,
        )
        .map_err(|_| ApiError::internal("无法加密数据库密码"))?;
    let mut payload = nonce.to_vec();
    payload.extend(data);
    Ok(format!("v1:{}", URL_SAFE_NO_PAD.encode(payload)))
}

pub fn decrypt_password(
    secret: &str,
    user_id: i32,
    connection_id: i32,
    payload: &str,
) -> ApiResult<String> {
    let invalid = || {
        ApiError::bad_request("已保存的数据库密码已失效，请编辑连接重新输入密码")
            .with_code("DB_PASSWORD_INVALID")
    };
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
            Aad::from(binding(user_id, connection_id).as_bytes()),
            &mut bytes[12..],
        )
        .map_err(|_| invalid())?;
    String::from_utf8(data.to_vec()).map_err(|_| invalid())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwords_are_encrypted_and_bound_to_owner_and_connection() {
        let payload = encrypt_password("key", 7, 3, "s3cret密码").unwrap();
        assert!(!payload.contains("s3cret"));
        assert_ne!(
            payload,
            encrypt_password("key", 7, 3, "s3cret密码").unwrap()
        );
        assert_eq!(
            decrypt_password("key", 7, 3, &payload).unwrap(),
            "s3cret密码"
        );
        assert!(decrypt_password("key", 8, 3, &payload).is_err());
        assert!(decrypt_password("key", 7, 4, &payload).is_err());
        assert!(decrypt_password("other", 7, 3, &payload).is_err());
        assert!(decrypt_password("key", 7, 3, "v1:AAAA").is_err());
    }
}
