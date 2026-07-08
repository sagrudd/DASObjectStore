use super::model::{
    AuthRegistry, AuthTokenResetReport, AuthenticatedUser, LoginResponse, LogoutResponse,
    RegisterResponse, RegistrationTokenRecord, SessionCheckResponse, SessionTokenRecord,
    UserSummary, DEFAULT_REGISTRATION_TTL_SECONDS, DEFAULT_SESSION_TTL_SECONDS,
    MAX_SESSION_TTL_SECONDS,
};
use super::token::{hash_password, new_token, token_hash, unix_now_seconds, verify_password};
use dasobjectstore_core::DEFAULT_PRODUCT_ROOT;
use std::fmt::{self, Display};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct LocalAuthStore {
    root: PathBuf,
}

impl LocalAuthStore {
    pub fn default_standalone() -> Self {
        Self::new(DEFAULT_PRODUCT_ROOT)
    }

    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn registry_path(&self) -> PathBuf {
        self.root.join("users.json")
    }

    pub fn create_user(
        &self,
        username: impl Into<String>,
    ) -> Result<UserSummary, LocalAuthStoreError> {
        let username = normalize_username(username.into());
        reject_blank_username(&username)?;
        let mut registry = self.load_registry()?;
        if registry.users.iter().any(|user| user.username == username) {
            return Err(LocalAuthStoreError::UserAlreadyExists { username });
        }

        let now = unix_now_seconds();
        let user = AuthenticatedUser {
            username,
            created_at_unix_seconds: now,
            password_hash: None,
            registered_at_unix_seconds: None,
            registration_tokens: Vec::new(),
            sessions: Vec::new(),
        };
        let summary = user_summary(&user, now);
        registry.users.push(user);
        self.save_registry(&registry)?;
        Ok(summary)
    }

    pub fn issue_registration_token(
        &self,
        username: &str,
        ttl_seconds: Option<i64>,
    ) -> Result<String, LocalAuthStoreError> {
        let ttl_seconds = ttl_seconds
            .filter(|seconds| *seconds > 0)
            .unwrap_or(DEFAULT_REGISTRATION_TTL_SECONDS);
        let mut registry = self.load_registry()?;
        let user = find_user_mut(&mut registry, username)?;
        let token = new_token();
        let now = unix_now_seconds();
        user.registration_tokens.push(RegistrationTokenRecord {
            token_hash: token_hash(&token),
            issued_at_unix_seconds: now,
            expires_at_unix_seconds: now + ttl_seconds,
            used_at_unix_seconds: None,
        });
        self.save_registry(&registry)?;
        Ok(token)
    }

    pub fn register_with_token(
        &self,
        username: &str,
        token: &str,
        password: &str,
    ) -> Result<RegisterResponse, LocalAuthStoreError> {
        let mut registry = self.load_registry()?;
        let user = find_user_mut(&mut registry, username)?;
        if user.password_hash.is_some() {
            return Err(LocalAuthStoreError::UserAlreadyRegistered {
                username: user.username.clone(),
            });
        }

        let now = unix_now_seconds();
        let token_digest = token_hash(token);
        let registration_token = user
            .registration_tokens
            .iter_mut()
            .find(|entry| entry.token_hash == token_digest)
            .ok_or(LocalAuthStoreError::InvalidRegistrationToken)?;
        if registration_token.used_at_unix_seconds.is_some() {
            return Err(LocalAuthStoreError::UsedRegistrationToken);
        }
        if registration_token.expires_at_unix_seconds < now {
            return Err(LocalAuthStoreError::ExpiredRegistrationToken);
        }

        registration_token.used_at_unix_seconds = Some(now);
        user.password_hash = Some(hash_password(password)?);
        user.registered_at_unix_seconds = Some(now);
        let (session_token, expires_at_unix_seconds) = push_session(user, None, now);
        self.save_registry(&registry)?;
        Ok(RegisterResponse {
            username: normalize_username(username),
            session_token,
            expires_at_unix_seconds,
        })
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
        let mut registry = self.load_registry()?;
        let user = find_user_mut(&mut registry, username)?;
        let Some(password_hash) = &user.password_hash else {
            return Err(LocalAuthStoreError::UserNotRegistered {
                username: normalize_username(username),
            });
        };
        verify_password(password_hash, password)?;

        let now = unix_now_seconds();
        let (session_token, expires_at_unix_seconds) = push_session(user, ttl_seconds, now);
        self.save_registry(&registry)?;
        Ok(LoginResponse {
            username: normalize_username(username),
            session_token,
            expires_at_unix_seconds,
        })
    }

    pub fn create_session_for_authenticated_local_user(
        &self,
        username: &str,
        ttl_seconds: Option<i64>,
    ) -> Result<LoginResponse, LocalAuthStoreError> {
        let username = normalize_username(username);
        reject_blank_username(&username)?;
        let mut registry = self.load_registry()?;
        let now = unix_now_seconds();
        let user_index = if let Some(index) = registry
            .users
            .iter()
            .position(|user| user.username == username)
        {
            index
        } else {
            registry.users.push(AuthenticatedUser {
                username: username.clone(),
                created_at_unix_seconds: now,
                password_hash: None,
                registered_at_unix_seconds: None,
                registration_tokens: Vec::new(),
                sessions: Vec::new(),
            });
            registry.users.len() - 1
        };
        let user = registry
            .users
            .get_mut(user_index)
            .expect("session user index was just resolved");
        let (session_token, expires_at_unix_seconds) = push_session(user, ttl_seconds, now);
        self.save_registry(&registry)?;
        Ok(LoginResponse {
            username,
            session_token,
            expires_at_unix_seconds,
        })
    }

    pub fn verify_session(
        &self,
        username: &str,
        session_token: &str,
    ) -> Result<SessionCheckResponse, LocalAuthStoreError> {
        let registry = self.load_registry()?;
        let user = find_user(&registry, username)?;
        let digest = token_hash(session_token);
        let session = user
            .sessions
            .iter()
            .find(|entry| entry.token_hash == digest)
            .ok_or(LocalAuthStoreError::InvalidSessionToken)?;
        if session.revoked_at_unix_seconds.is_some() {
            return Err(LocalAuthStoreError::InvalidSessionToken);
        }
        if session.expires_at_unix_seconds < unix_now_seconds() {
            return Err(LocalAuthStoreError::ExpiredSessionToken);
        }

        Ok(SessionCheckResponse {
            username: normalize_username(username),
            valid: true,
            expires_at_unix_seconds: session.expires_at_unix_seconds,
        })
    }

    pub fn logout(
        &self,
        username: &str,
        session_token: &str,
    ) -> Result<LogoutResponse, LocalAuthStoreError> {
        let mut registry = self.load_registry()?;
        let user = find_user_mut(&mut registry, username)?;
        let digest = token_hash(session_token);
        let session = user
            .sessions
            .iter_mut()
            .find(|entry| entry.token_hash == digest)
            .ok_or(LocalAuthStoreError::InvalidSessionToken)?;
        session.revoked_at_unix_seconds = Some(unix_now_seconds());
        self.save_registry(&registry)?;
        Ok(LogoutResponse {
            username: normalize_username(username),
            disconnected: true,
        })
    }

    pub fn revoke_all_sessions(&self) -> Result<usize, LocalAuthStoreError> {
        let mut registry = self.load_registry()?;
        let now = unix_now_seconds();
        let mut revoked = 0;
        for user in &mut registry.users {
            for session in &mut user.sessions {
                if session.revoked_at_unix_seconds.is_none() {
                    session.revoked_at_unix_seconds = Some(now);
                    revoked += 1;
                }
            }
        }
        if revoked > 0 {
            self.save_registry(&registry)?;
        }
        Ok(revoked)
    }

    pub fn reset_all_tokens(&self) -> Result<AuthTokenResetReport, LocalAuthStoreError> {
        let mut registry = self.load_registry()?;
        let now = unix_now_seconds();
        let mut revoked_sessions = 0;
        let mut revoked_registration_tokens = 0;
        for user in &mut registry.users {
            for session in &mut user.sessions {
                if session.revoked_at_unix_seconds.is_none() {
                    session.revoked_at_unix_seconds = Some(now);
                    revoked_sessions += 1;
                }
            }
            for token in &mut user.registration_tokens {
                if token.used_at_unix_seconds.is_none() {
                    token.used_at_unix_seconds = Some(now);
                    revoked_registration_tokens += 1;
                }
            }
        }
        if revoked_sessions > 0 || revoked_registration_tokens > 0 {
            self.save_registry(&registry)?;
        }
        Ok(AuthTokenResetReport {
            revoked_sessions,
            revoked_registration_tokens,
        })
    }

    pub fn list_users(&self) -> Result<Vec<UserSummary>, LocalAuthStoreError> {
        let registry = self.load_registry()?;
        let now = unix_now_seconds();
        Ok(registry
            .users
            .iter()
            .map(|user| user_summary(user, now))
            .collect())
    }

    pub fn load_registry(&self) -> Result<AuthRegistry, LocalAuthStoreError> {
        let path = self.registry_path();
        if !path.exists() {
            return Ok(AuthRegistry::default());
        }
        let Some(data) = read_registry_with_empty_retry(&path)? else {
            return Ok(AuthRegistry::default());
        };
        serde_json::from_str(&data).map_err(LocalAuthStoreError::Json)
    }

    fn save_registry(&self, registry: &AuthRegistry) -> Result<(), LocalAuthStoreError> {
        fs::create_dir_all(&self.root).map_err(|source| LocalAuthStoreError::Io {
            path: self.root.clone(),
            source,
        })?;
        let data = serde_json::to_string_pretty(registry).map_err(LocalAuthStoreError::Json)?;
        fs::write(self.registry_path(), format!("{data}\n")).map_err(|source| {
            LocalAuthStoreError::Io {
                path: self.registry_path(),
                source,
            }
        })
    }
}

#[derive(Debug)]
pub enum LocalAuthStoreError {
    Io { path: PathBuf, source: io::Error },
    Json(serde_json::Error),
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

fn read_registry_with_empty_retry(path: &Path) -> Result<Option<String>, LocalAuthStoreError> {
    const ATTEMPTS: usize = 5;
    for attempt in 0..ATTEMPTS {
        let data = fs::read_to_string(path).map_err(|source| LocalAuthStoreError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if !data.trim().is_empty() {
            return Ok(Some(data));
        }
        if attempt + 1 < ATTEMPTS {
            thread::sleep(Duration::from_millis(20));
        }
    }
    Ok(None)
}

fn find_user_mut<'a>(
    registry: &'a mut AuthRegistry,
    username: &str,
) -> Result<&'a mut AuthenticatedUser, LocalAuthStoreError> {
    let username = normalize_username(username);
    registry
        .users
        .iter_mut()
        .find(|user| user.username == username)
        .ok_or(LocalAuthStoreError::UserNotFound { username })
}

fn find_user<'a>(
    registry: &'a AuthRegistry,
    username: &str,
) -> Result<&'a AuthenticatedUser, LocalAuthStoreError> {
    let username = normalize_username(username);
    registry
        .users
        .iter()
        .find(|user| user.username == username)
        .ok_or(LocalAuthStoreError::UserNotFound { username })
}

fn normalize_username(username: impl AsRef<str>) -> String {
    username.as_ref().trim().to_string()
}

fn reject_blank_username(username: &str) -> Result<(), LocalAuthStoreError> {
    if username.is_empty() {
        return Err(LocalAuthStoreError::UserNameRequired);
    }
    Ok(())
}

fn user_summary(user: &AuthenticatedUser, now: i64) -> UserSummary {
    UserSummary {
        username: user.username.clone(),
        registered: user.password_hash.is_some(),
        created_at_unix_seconds: user.created_at_unix_seconds,
        registered_at_unix_seconds: user.registered_at_unix_seconds,
        active_session_count: user
            .sessions
            .iter()
            .filter(|session| {
                session.revoked_at_unix_seconds.is_none() && session.expires_at_unix_seconds > now
            })
            .count(),
    }
}

fn push_session(user: &mut AuthenticatedUser, ttl_seconds: Option<i64>, now: i64) -> (String, i64) {
    let session_token = new_token();
    let ttl_seconds = ttl_seconds
        .filter(|seconds| *seconds > 0)
        .unwrap_or(DEFAULT_SESSION_TTL_SECONDS)
        .min(MAX_SESSION_TTL_SECONDS);
    let expires_at_unix_seconds = now + ttl_seconds;
    user.sessions.push(SessionTokenRecord {
        token_hash: token_hash(&session_token),
        issued_at_unix_seconds: now,
        expires_at_unix_seconds,
        revoked_at_unix_seconds: None,
    });
    (session_token, expires_at_unix_seconds)
}
