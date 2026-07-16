use super::model::{
    AuthRegistry, AuthTokenResetReport, AuthenticatedUser, LoginResponse, LogoutResponse,
    RegisterResponse, RegistrationTokenRecord, SessionCheckResponse, SessionTokenRecord,
    UserSummary,
};
use prosopikon_core::{
    AuthError as ProsopikonAuthError, AuthRegistry as ProsopikonAuthRegistry,
    AuthenticatedUser as ProsopikonAuthenticatedUser, LoginResponse as ProsopikonLoginResponse,
    LogoutResponse as ProsopikonLogoutResponse, ProsopikonAuthStore,
    RegisterResponse as ProsopikonRegisterResponse,
    RegistrationTokenRecord as ProsopikonRegistrationTokenRecord,
    SessionCheckResponse as ProsopikonSessionCheckResponse,
    SessionTokenRecord as ProsopikonSessionTokenRecord, UserSummary as ProsopikonUserSummary,
};
use std::ffi::OsString;
use std::fmt::{self, Display};
use std::io;
use std::path::{Path, PathBuf};

pub const DASOBJECTSTORE_AUTH_ROOT_ENV: &str = "DASOBJECTSTORE_AUTH_ROOT";
pub const DEFAULT_DASOBJECTSTORE_AUTH_ROOT: &str = "/var/lib/dasobjectstore/auth";

#[derive(Clone, Debug)]
pub struct LocalAuthStore {
    inner: ProsopikonAuthStore,
}

impl LocalAuthStore {
    pub fn default_standalone() -> Self {
        Self {
            inner: ProsopikonAuthStore::new(standalone_auth_root_from_env(std::env::var_os(
                DASOBJECTSTORE_AUTH_ROOT_ENV,
            ))),
        }
    }

    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            inner: ProsopikonAuthStore::new(root),
        }
    }

    /// Share the host's Prosopikon authority without creating a second
    /// registry or browser-session issuer.
    pub fn from_prosopikon(inner: ProsopikonAuthStore) -> Self {
        Self { inner }
    }

    pub fn root(&self) -> &Path {
        self.inner.root()
    }

    pub fn registry_path(&self) -> PathBuf {
        self.inner.registry_path()
    }

    pub fn create_user(
        &self,
        username: impl Into<String>,
    ) -> Result<UserSummary, LocalAuthStoreError> {
        self.inner
            .create_user(username)
            .map(user_summary_from_prosopikon)
            .map_err(LocalAuthStoreError::from)
    }

    pub fn issue_registration_token(
        &self,
        username: &str,
        ttl_seconds: Option<i64>,
    ) -> Result<String, LocalAuthStoreError> {
        self.inner
            .issue_registration_token_seconds(username, ttl_seconds)
            .map_err(LocalAuthStoreError::from)
    }

    pub fn register_with_token(
        &self,
        username: &str,
        token: &str,
        password: &str,
    ) -> Result<RegisterResponse, LocalAuthStoreError> {
        self.inner
            .register_with_token(username, token, password)
            .map(register_response_from_prosopikon)
            .map_err(LocalAuthStoreError::from)
    }

    pub fn login(
        &self,
        username: &str,
        password: &str,
    ) -> Result<LoginResponse, LocalAuthStoreError> {
        self.login_with_session_ttl_seconds(username, password, None)
    }

    pub fn login_with_session_ttl_seconds(
        &self,
        username: &str,
        password: &str,
        ttl_seconds: Option<i64>,
    ) -> Result<LoginResponse, LocalAuthStoreError> {
        self.inner
            .login_with_session_ttl_seconds(username, password, ttl_seconds)
            .map(login_response_from_prosopikon)
            .map_err(LocalAuthStoreError::from)
    }

    pub fn create_session_for_authenticated_local_user(
        &self,
        username: &str,
        ttl_seconds: Option<i64>,
    ) -> Result<LoginResponse, LocalAuthStoreError> {
        self.inner
            .create_session_for_authenticated_local_user(username, ttl_seconds)
            .map(login_response_from_prosopikon)
            .map_err(LocalAuthStoreError::from)
    }

    pub fn verify_session(
        &self,
        username: &str,
        session_token: &str,
    ) -> Result<SessionCheckResponse, LocalAuthStoreError> {
        self.inner
            .verify_session(username, session_token)
            .map(session_check_response_from_prosopikon)
            .map_err(LocalAuthStoreError::from)
    }

    pub fn logout(
        &self,
        username: &str,
        session_token: &str,
    ) -> Result<LogoutResponse, LocalAuthStoreError> {
        self.inner
            .logout(username, session_token)
            .map(logout_response_from_prosopikon)
            .map_err(LocalAuthStoreError::from)
    }

    pub fn revoke_all_sessions(&self) -> Result<usize, LocalAuthStoreError> {
        self.inner
            .revoke_all_sessions()
            .map_err(LocalAuthStoreError::from)
    }

    pub fn reset_all_tokens(&self) -> Result<AuthTokenResetReport, LocalAuthStoreError> {
        self.inner
            .reset_all_tokens()
            .map(|report| AuthTokenResetReport {
                revoked_sessions: report.revoked_sessions,
                revoked_registration_tokens: report.revoked_registration_tokens,
            })
            .map_err(LocalAuthStoreError::from)
    }

    pub fn list_users(&self) -> Result<Vec<UserSummary>, LocalAuthStoreError> {
        self.inner
            .list_users()
            .map(|users| {
                users
                    .into_iter()
                    .map(user_summary_from_prosopikon)
                    .collect()
            })
            .map_err(LocalAuthStoreError::from)
    }

    pub fn load_registry(&self) -> Result<AuthRegistry, LocalAuthStoreError> {
        self.inner
            .load_registry()
            .map(registry_from_prosopikon)
            .map_err(LocalAuthStoreError::from)
    }

    #[cfg(test)]
    pub fn expire_sessions_for_test(&self, username: &str) -> Result<(), LocalAuthStoreError> {
        let username = normalize_username(username);
        let mut registry = self
            .inner
            .load_registry()
            .map_err(LocalAuthStoreError::from)?;
        let user = registry
            .users
            .iter_mut()
            .find(|user| user.username == username)
            .ok_or(LocalAuthStoreError::UserNotFound { username })?;
        for session in &mut user.sessions {
            session.expires_at_utc = session.issued_at_utc;
        }
        self.inner
            .save_registry(&registry)
            .map_err(LocalAuthStoreError::from)
    }
}

fn standalone_auth_root_from_env(root_env: Option<OsString>) -> PathBuf {
    match root_env {
        Some(root) if !root.is_empty() => PathBuf::from(root),
        _ => PathBuf::from(DEFAULT_DASOBJECTSTORE_AUTH_ROOT),
    }
}

#[derive(Debug)]
pub enum LocalAuthStoreError {
    Io { path: PathBuf, source: io::Error },
    Json(serde_json::Error),
    ProsopikonStore(String),
    UserNameRequired,
    UserAlreadyExists { username: String },
    UserAlreadyRegistered { username: String },
    UserNotFound { username: String },
    UserNotRegistered { username: String },
    InvalidRegistrationToken,
    UsedRegistrationToken,
    ExpiredRegistrationToken,
    InvalidSessionToken,
    ExpiredSessionToken,
    PasswordRequired,
    PasswordHash,
    InvalidPassword,
}

impl Display for LocalAuthStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(
                    formatter,
                    "local auth IO failed at {}: {source}",
                    path.display()
                )
            }
            Self::Json(err) => write!(formatter, "local auth JSON failed: {err}"),
            Self::ProsopikonStore(err) => write!(formatter, "prosopikon auth store failed: {err}"),
            Self::UserNameRequired => write!(formatter, "username is required"),
            Self::UserAlreadyExists { username } => {
                write!(formatter, "local auth user already exists: {username}")
            }
            Self::UserAlreadyRegistered { username } => {
                write!(
                    formatter,
                    "local auth user is already registered: {username}"
                )
            }
            Self::UserNotFound { username } => {
                write!(formatter, "local auth user not found: {username}")
            }
            Self::UserNotRegistered { username } => {
                write!(formatter, "local auth user is not registered: {username}")
            }
            Self::InvalidRegistrationToken => write!(formatter, "invalid registration token"),
            Self::UsedRegistrationToken => {
                write!(formatter, "registration token has already been used")
            }
            Self::ExpiredRegistrationToken => write!(formatter, "registration token has expired"),
            Self::InvalidSessionToken => write!(formatter, "invalid session token"),
            Self::ExpiredSessionToken => write!(formatter, "session token has expired"),
            Self::PasswordRequired => write!(formatter, "password is required"),
            Self::PasswordHash => write!(formatter, "password hashing failed"),
            Self::InvalidPassword => write!(formatter, "invalid password"),
        }
    }
}

impl std::error::Error for LocalAuthStoreError {}

impl From<ProsopikonAuthError> for LocalAuthStoreError {
    fn from(err: ProsopikonAuthError) -> Self {
        match err {
            ProsopikonAuthError::Io(message) | ProsopikonAuthError::Json(message) => {
                Self::ProsopikonStore(message)
            }
            ProsopikonAuthError::UserNameRequired => Self::UserNameRequired,
            ProsopikonAuthError::UserAlreadyExists { username } => {
                Self::UserAlreadyExists { username }
            }
            ProsopikonAuthError::UserNotFound { username } => Self::UserNotFound { username },
            ProsopikonAuthError::UserAlreadyRegistered { username } => {
                Self::UserAlreadyRegistered { username }
            }
            ProsopikonAuthError::UserNotRegistered { username } => {
                Self::UserNotRegistered { username }
            }
            ProsopikonAuthError::InvalidRegistrationToken => Self::InvalidRegistrationToken,
            ProsopikonAuthError::ExpiredRegistrationToken => Self::ExpiredRegistrationToken,
            ProsopikonAuthError::UsedRegistrationToken => Self::UsedRegistrationToken,
            ProsopikonAuthError::InvalidPassword => Self::InvalidPassword,
            ProsopikonAuthError::InvalidSessionToken => Self::InvalidSessionToken,
            ProsopikonAuthError::ExpiredSessionToken => Self::ExpiredSessionToken,
            ProsopikonAuthError::PasswordRequired => Self::PasswordRequired,
            ProsopikonAuthError::PasswordHash => Self::PasswordHash,
        }
    }
}

fn registry_from_prosopikon(registry: ProsopikonAuthRegistry) -> AuthRegistry {
    AuthRegistry {
        users: registry
            .users
            .into_iter()
            .map(authenticated_user_from_prosopikon)
            .collect(),
    }
}

fn authenticated_user_from_prosopikon(user: ProsopikonAuthenticatedUser) -> AuthenticatedUser {
    AuthenticatedUser {
        username: user.username,
        created_at_unix_seconds: user.created_at_utc.timestamp(),
        password_hash: user.password_hash,
        registered_at_unix_seconds: user
            .registered_at_utc
            .map(|timestamp| timestamp.timestamp()),
        registration_tokens: user
            .registration_tokens
            .into_iter()
            .map(registration_token_from_prosopikon)
            .collect(),
        sessions: user
            .sessions
            .into_iter()
            .map(session_token_from_prosopikon)
            .collect(),
    }
}

fn registration_token_from_prosopikon(
    token: ProsopikonRegistrationTokenRecord,
) -> RegistrationTokenRecord {
    RegistrationTokenRecord {
        token_hash: token.token_hash,
        issued_at_unix_seconds: token.issued_at_utc.timestamp(),
        expires_at_unix_seconds: token.expires_at_utc.timestamp(),
        used_at_unix_seconds: token.used_at_utc.map(|timestamp| timestamp.timestamp()),
    }
}

fn session_token_from_prosopikon(token: ProsopikonSessionTokenRecord) -> SessionTokenRecord {
    SessionTokenRecord {
        token_hash: token.token_hash,
        issued_at_unix_seconds: token.issued_at_utc.timestamp(),
        expires_at_unix_seconds: token.expires_at_utc.timestamp(),
        revoked_at_unix_seconds: token.revoked_at_utc.map(|timestamp| timestamp.timestamp()),
    }
}

fn user_summary_from_prosopikon(user: ProsopikonUserSummary) -> UserSummary {
    UserSummary {
        username: user.username,
        registered: user.registered,
        created_at_unix_seconds: user.created_at_utc.timestamp(),
        registered_at_unix_seconds: user
            .registered_at_utc
            .map(|timestamp| timestamp.timestamp()),
        active_session_count: user.active_session_count,
    }
}

fn register_response_from_prosopikon(response: ProsopikonRegisterResponse) -> RegisterResponse {
    RegisterResponse {
        username: response.username,
        session_token: response.session_token,
        expires_at_unix_seconds: response.expires_at_utc.timestamp(),
    }
}

fn login_response_from_prosopikon(response: ProsopikonLoginResponse) -> LoginResponse {
    LoginResponse {
        username: response.username,
        session_token: response.session_token,
        expires_at_unix_seconds: response.expires_at_utc.timestamp(),
    }
}

fn session_check_response_from_prosopikon(
    response: ProsopikonSessionCheckResponse,
) -> SessionCheckResponse {
    SessionCheckResponse {
        username: response.username,
        valid: response.valid,
        expires_at_unix_seconds: response.expires_at_utc.timestamp(),
    }
}

fn logout_response_from_prosopikon(response: ProsopikonLogoutResponse) -> LogoutResponse {
    LogoutResponse {
        username: response.username,
        disconnected: response.disconnected,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        standalone_auth_root_from_env, DASOBJECTSTORE_AUTH_ROOT_ENV,
        DEFAULT_DASOBJECTSTORE_AUTH_ROOT,
    };
    use std::ffi::OsString;
    use std::path::PathBuf;

    #[test]
    fn standalone_auth_root_defaults_to_dasobjectstore_state_directory() {
        assert_eq!(
            standalone_auth_root_from_env(None),
            PathBuf::from(DEFAULT_DASOBJECTSTORE_AUTH_ROOT)
        );
    }

    #[test]
    fn standalone_auth_root_ignores_empty_environment_value() {
        assert_eq!(
            standalone_auth_root_from_env(Some(OsString::new())),
            PathBuf::from(DEFAULT_DASOBJECTSTORE_AUTH_ROOT)
        );
    }

    #[test]
    fn standalone_auth_root_can_be_overridden_for_non_packaged_runs() {
        let override_root = PathBuf::from("/tmp/dasobjectstore-auth-test");

        assert_eq!(
            standalone_auth_root_from_env(Some(override_root.clone().into_os_string())),
            override_root
        );
        assert_eq!(DASOBJECTSTORE_AUTH_ROOT_ENV, "DASOBJECTSTORE_AUTH_ROOT");
    }
}

#[cfg(test)]
fn normalize_username(username: impl AsRef<str>) -> String {
    username.as_ref().trim().to_string()
}
