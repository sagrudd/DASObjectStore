//! Host-federated authentication context accepted from Monas or Synoptikon.
//!
//! This contract carries authenticated identity and session metadata only.
//! It deliberately cannot grant daemon storage permissions; routes continue
//! to apply local group, administrator, ObjectStore, and action policy after
//! extracting the actor.

use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

pub const HOST_AUTH_CONTEXT_SCHEMA_VERSION: &str = "dasobjectstore.host_auth_context.v1";
pub const HOST_AUTH_AUDIENCE: &str = "dasobjectstore";
pub const MAX_HOST_AUTH_CONTEXT_TTL_SECONDS: i64 = 8 * 60 * 60;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostAuthenticationAuthority {
    MonasStandalone,
    SynoptikonIntegrated,
}

impl HostAuthenticationAuthority {
    pub const fn issuer(self) -> &'static str {
        match self {
            Self::MonasStandalone => "monas",
            Self::SynoptikonIntegrated => "synoptikon",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostAuthenticatedContext {
    pub schema_version: String,
    pub authority: HostAuthenticationAuthority,
    pub issuer: String,
    pub audience: String,
    pub subject_id: String,
    pub session_id: String,
    pub roles: Vec<String>,
    pub issued_at_unix_seconds: i64,
    pub expires_at_unix_seconds: i64,
    pub correlation_id: String,
    /// Digest binding to the host's CSRF state. The raw host token never
    /// crosses into DASObjectStore.
    pub csrf_binding_sha256: String,
}

impl HostAuthenticatedContext {
    pub fn validate(&self, accepted_at_unix_seconds: i64) -> Result<(), HostAuthContextError> {
        if self.schema_version != HOST_AUTH_CONTEXT_SCHEMA_VERSION {
            return Err(HostAuthContextError::UnsupportedSchema);
        }
        if self.issuer != self.authority.issuer() {
            return Err(HostAuthContextError::IssuerAuthorityMismatch);
        }
        if self.audience != HOST_AUTH_AUDIENCE {
            return Err(HostAuthContextError::InvalidAudience);
        }
        validate_id("subject_id", &self.subject_id)?;
        validate_id("session_id", &self.session_id)?;
        validate_id("correlation_id", &self.correlation_id)?;
        if self.roles.is_empty() {
            return Err(HostAuthContextError::EmptyRoles);
        }
        let mut roles = self.roles.clone();
        roles.sort();
        roles.dedup();
        if roles.len() != self.roles.len() {
            return Err(HostAuthContextError::DuplicateRoles);
        }
        for role in &self.roles {
            validate_id("roles", role)?;
        }
        if self.issued_at_unix_seconds < 0
            || self.expires_at_unix_seconds <= self.issued_at_unix_seconds
            || self.issued_at_unix_seconds > accepted_at_unix_seconds
            || self.expires_at_unix_seconds <= accepted_at_unix_seconds
        {
            return Err(HostAuthContextError::InvalidLifetime);
        }
        if self.expires_at_unix_seconds - self.issued_at_unix_seconds
            > MAX_HOST_AUTH_CONTEXT_TTL_SECONDS
        {
            return Err(HostAuthContextError::LifetimeTooLong);
        }
        validate_sha256(&self.csrf_binding_sha256)?;
        Ok(())
    }
}

/// The embedding host must verify its live session/revocation authority before
/// DASObjectStore accepts the context. This boundary permits Monas and
/// Synoptikon adapters without making DASObjectStore a second session issuer.
pub trait HostAuthenticationContextVerifier {
    fn verify_live_session(&self, context: &HostAuthenticatedContext) -> Result<(), String>;
}

#[derive(Clone, Debug)]
pub struct VerifiedHostAuthenticatedContext(HostAuthenticatedContext);

impl VerifiedHostAuthenticatedContext {
    pub fn context(&self) -> &HostAuthenticatedContext {
        &self.0
    }
}

pub fn accept_host_authenticated_context(
    context: HostAuthenticatedContext,
    accepted_at_unix_seconds: i64,
    verifier: &impl HostAuthenticationContextVerifier,
) -> Result<VerifiedHostAuthenticatedContext, HostAuthContextError> {
    context.validate(accepted_at_unix_seconds)?;
    verifier
        .verify_live_session(&context)
        .map_err(HostAuthContextError::HostVerificationFailed)?;
    Ok(VerifiedHostAuthenticatedContext(context))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostAuthContextError {
    UnsupportedSchema,
    IssuerAuthorityMismatch,
    InvalidAudience,
    InvalidField { field: &'static str },
    EmptyRoles,
    DuplicateRoles,
    InvalidLifetime,
    LifetimeTooLong,
    InvalidCsrfBinding,
    HostVerificationFailed(String),
}

impl Display for HostAuthContextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema => formatter.write_str("unsupported host-auth context schema"),
            Self::IssuerAuthorityMismatch => {
                formatter.write_str("host-auth issuer does not match authority")
            }
            Self::InvalidAudience => {
                formatter.write_str("host-auth audience must be dasobjectstore")
            }
            Self::InvalidField { field } => write!(formatter, "invalid host-auth field {field}"),
            Self::EmptyRoles => formatter.write_str("host-auth roles must not be empty"),
            Self::DuplicateRoles => formatter.write_str("host-auth roles must be unique"),
            Self::InvalidLifetime => {
                formatter.write_str("host-auth context is not currently valid")
            }
            Self::LifetimeTooLong => write!(
                formatter,
                "host-auth context exceeds {MAX_HOST_AUTH_CONTEXT_TTL_SECONDS} seconds"
            ),
            Self::InvalidCsrfBinding => {
                formatter.write_str("invalid host-auth CSRF binding digest")
            }
            Self::HostVerificationFailed(message) => {
                write!(formatter, "host session verification failed: {message}")
            }
        }
    }
}

impl std::error::Error for HostAuthContextError {}

fn validate_id(field: &'static str, value: &str) -> Result<(), HostAuthContextError> {
    let valid = !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':' | b'/')
        });
    if valid {
        Ok(())
    } else {
        Err(HostAuthContextError::InvalidField { field })
    }
}

fn validate_sha256(value: &str) -> Result<(), HostAuthContextError> {
    let digest = value.strip_prefix("sha256:").unwrap_or_default();
    if digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err(HostAuthContextError::InvalidCsrfBinding)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct LiveVerifier(bool);

    impl HostAuthenticationContextVerifier for LiveVerifier {
        fn verify_live_session(&self, _context: &HostAuthenticatedContext) -> Result<(), String> {
            self.0.then_some(()).ok_or_else(|| "revoked".to_string())
        }
    }

    fn context(authority: HostAuthenticationAuthority) -> HostAuthenticatedContext {
        HostAuthenticatedContext {
            schema_version: HOST_AUTH_CONTEXT_SCHEMA_VERSION.to_string(),
            authority,
            issuer: authority.issuer().to_string(),
            audience: HOST_AUTH_AUDIENCE.to_string(),
            subject_id: "user-1".to_string(),
            session_id: "session-1".to_string(),
            roles: vec!["storage_operator".to_string()],
            issued_at_unix_seconds: 1_000,
            expires_at_unix_seconds: 2_000,
            correlation_id: "corr-1".to_string(),
            csrf_binding_sha256: format!("sha256:{}", "a".repeat(64)),
        }
    }

    #[test]
    fn accepts_live_monas_and_synoptikon_contexts() {
        for authority in [
            HostAuthenticationAuthority::MonasStandalone,
            HostAuthenticationAuthority::SynoptikonIntegrated,
        ] {
            let accepted =
                accept_host_authenticated_context(context(authority), 1_500, &LiveVerifier(true))
                    .expect("live context");
            assert_eq!(accepted.context().authority, authority);
        }
    }

    #[test]
    fn rejects_expired_revoked_mismatched_and_unknown_contexts() {
        let mut expired = context(HostAuthenticationAuthority::MonasStandalone);
        expired.expires_at_unix_seconds = 1_500;
        assert!(accept_host_authenticated_context(expired, 1_500, &LiveVerifier(true)).is_err());
        assert!(accept_host_authenticated_context(
            context(HostAuthenticationAuthority::MonasStandalone),
            1_500,
            &LiveVerifier(false)
        )
        .is_err());
        let mut mismatch = context(HostAuthenticationAuthority::MonasStandalone);
        mismatch.issuer = "synoptikon".to_string();
        assert!(mismatch.validate(1_500).is_err());
        let mut encoded =
            serde_json::to_value(context(HostAuthenticationAuthority::SynoptikonIntegrated))
                .expect("serialize");
        encoded["storage_write_authorized"] = serde_json::json!(true);
        assert!(serde_json::from_value::<HostAuthenticatedContext>(encoded).is_err());
    }
}
