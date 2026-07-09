use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthRegistry {
    pub users: Vec<AuthenticatedUser>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthenticatedUser {
    pub username: String,
    pub created_at_unix_seconds: i64,
    pub password_hash: Option<String>,
    pub registered_at_unix_seconds: Option<i64>,
    pub registration_tokens: Vec<RegistrationTokenRecord>,
    pub sessions: Vec<SessionTokenRecord>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RegistrationTokenRecord {
    pub token_hash: String,
    pub issued_at_unix_seconds: i64,
    pub expires_at_unix_seconds: i64,
    pub used_at_unix_seconds: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionTokenRecord {
    pub token_hash: String,
    pub issued_at_unix_seconds: i64,
    pub expires_at_unix_seconds: i64,
    pub revoked_at_unix_seconds: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UserSummary {
    pub username: String,
    pub registered: bool,
    pub created_at_unix_seconds: i64,
    pub registered_at_unix_seconds: Option<i64>,
    pub active_session_count: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RegisterResponse {
    pub username: String,
    pub session_token: String,
    pub expires_at_unix_seconds: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LoginResponse {
    pub username: String,
    pub session_token: String,
    pub expires_at_unix_seconds: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionCheckResponse {
    pub username: String,
    pub valid: bool,
    pub expires_at_unix_seconds: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LogoutResponse {
    pub username: String,
    pub disconnected: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthTokenResetReport {
    pub revoked_sessions: usize,
    pub revoked_registration_tokens: usize,
}
