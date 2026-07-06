use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use rand_core::OsRng;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use super::store::LocalAuthStoreError;

pub fn hash_password(password: &str) -> Result<String, LocalAuthStoreError> {
    if password.is_empty() {
        return Err(LocalAuthStoreError::PasswordRequired);
    }

    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| LocalAuthStoreError::PasswordHash)
}

pub fn verify_password(hash: &str, password: &str) -> Result<(), LocalAuthStoreError> {
    let parsed = PasswordHash::new(hash).map_err(|_| LocalAuthStoreError::InvalidPassword)?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .map_err(|_| LocalAuthStoreError::InvalidPassword)
}

pub fn new_token() -> String {
    Uuid::new_v4().to_string()
}

pub fn token_hash(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex_lower(&hasher.finalize())
}

pub fn unix_now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after unix epoch")
        .as_secs() as i64
}

fn hex_lower(bytes: &[u8]) -> String {
    const TABLE: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(TABLE[(byte >> 4) as usize] as char);
        encoded.push(TABLE[(byte & 0x0f) as usize] as char);
    }
    encoded
}
