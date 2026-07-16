//! Concrete host-session adapters for the GUI authentication boundary.

use crate::{
    accept_synoptikon_integrated_session, SynoptikonIntegratedAcceptedSession,
    SynoptikonIntegratedSessionIssue,
};
use dasobjectstore_gui_api::{
    accept_host_authenticated_context, HostAuthContextError, HostAuthenticatedContext,
    HostAuthenticationAuthority, HostAuthenticationContextVerifier,
    VerifiedHostAuthenticatedContext, HOST_AUTH_AUDIENCE, HOST_AUTH_CONTEXT_SCHEMA_VERSION,
};
use prosopikon_core::ProsopikonAuthStore;
use sha2::{Digest, Sha256};
use std::fmt::{self, Display};

pub const HOST_ADAPTER_CONTEXT_TTL_SECONDS: i64 = 5 * 60;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MonasHostSessionIssue {
    pub username: String,
    pub session_token: String,
    pub correlation_id: String,
    pub csrf_binding_sha256: String,
}

pub trait SynoptikonLiveSessionVerifier {
    fn verify_live_session(
        &self,
        session: &SynoptikonIntegratedAcceptedSession,
    ) -> Result<(), String>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostSessionAdapterError {
    MonasSession(String),
    SynoptikonSession(String),
    HostContext(HostAuthContextError),
}

impl Display for HostSessionAdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MonasSession(message) => write!(formatter, "Monas session rejected: {message}"),
            Self::SynoptikonSession(message) => {
                write!(formatter, "Synoptikon session rejected: {message}")
            }
            Self::HostContext(err) => write!(formatter, "host authentication rejected: {err}"),
        }
    }
}

impl std::error::Error for HostSessionAdapterError {}

pub fn accept_monas_host_session(
    auth_store: &ProsopikonAuthStore,
    issue: &MonasHostSessionIssue,
    accepted_at_unix_seconds: i64,
) -> Result<VerifiedHostAuthenticatedContext, HostSessionAdapterError> {
    let session = auth_store
        .verify_session(&issue.username, &issue.session_token)
        .map_err(|err| HostSessionAdapterError::MonasSession(err.to_string()))?;
    let session_expiry = session.expires_at_utc.timestamp();
    let context_expiry = session_expiry.min(
        accepted_at_unix_seconds
            .checked_add(HOST_ADAPTER_CONTEXT_TTL_SECONDS)
            .unwrap_or(i64::MAX),
    );
    let context = HostAuthenticatedContext {
        schema_version: HOST_AUTH_CONTEXT_SCHEMA_VERSION.to_string(),
        authority: HostAuthenticationAuthority::MonasStandalone,
        issuer: HostAuthenticationAuthority::MonasStandalone
            .issuer()
            .to_string(),
        audience: HOST_AUTH_AUDIENCE.to_string(),
        subject_id: session.username,
        session_id: session_digest(&issue.session_token),
        roles: vec!["authenticated".to_string()],
        issued_at_unix_seconds: accepted_at_unix_seconds,
        expires_at_unix_seconds: context_expiry,
        correlation_id: issue.correlation_id.clone(),
        csrf_binding_sha256: issue.csrf_binding_sha256.clone(),
    };
    let verifier = MonasLiveVerifier {
        auth_store,
        issue,
        expected_expiry: session_expiry,
    };
    accept_host_authenticated_context(context, accepted_at_unix_seconds, &verifier)
        .map_err(HostSessionAdapterError::HostContext)
}

pub fn accept_synoptikon_host_session(
    issue: &SynoptikonIntegratedSessionIssue,
    csrf_binding_sha256: impl Into<String>,
    accepted_at_unix_seconds: i64,
    verifier: &impl SynoptikonLiveSessionVerifier,
) -> Result<VerifiedHostAuthenticatedContext, HostSessionAdapterError> {
    let session = accept_synoptikon_integrated_session(issue, accepted_at_unix_seconds)
        .map_err(|err| HostSessionAdapterError::SynoptikonSession(err.to_string()))?;
    let context = HostAuthenticatedContext {
        schema_version: HOST_AUTH_CONTEXT_SCHEMA_VERSION.to_string(),
        authority: HostAuthenticationAuthority::SynoptikonIntegrated,
        issuer: HostAuthenticationAuthority::SynoptikonIntegrated
            .issuer()
            .to_string(),
        audience: HOST_AUTH_AUDIENCE.to_string(),
        subject_id: session.actor.user_id.clone(),
        session_id: session.request_id.clone(),
        roles: session.actor.roles.clone(),
        issued_at_unix_seconds: session.issued_at_unix_seconds,
        expires_at_unix_seconds: session.expires_at_unix_seconds,
        correlation_id: session.correlation_id.clone(),
        csrf_binding_sha256: csrf_binding_sha256.into(),
    };
    let verifier = SynoptikonVerifier {
        session: &session,
        verifier,
    };
    accept_host_authenticated_context(context, accepted_at_unix_seconds, &verifier)
        .map_err(HostSessionAdapterError::HostContext)
}

struct MonasLiveVerifier<'a> {
    auth_store: &'a ProsopikonAuthStore,
    issue: &'a MonasHostSessionIssue,
    expected_expiry: i64,
}

impl HostAuthenticationContextVerifier for MonasLiveVerifier<'_> {
    fn verify_live_session(&self, context: &HostAuthenticatedContext) -> Result<(), String> {
        let session = self
            .auth_store
            .verify_session(&self.issue.username, &self.issue.session_token)
            .map_err(|err| err.to_string())?;
        if session.username != context.subject_id
            || session.expires_at_utc.timestamp() != self.expected_expiry
            || context.session_id != session_digest(&self.issue.session_token)
        {
            return Err("Monas session identity changed during adaptation".to_string());
        }
        Ok(())
    }
}

struct SynoptikonVerifier<'a, V> {
    session: &'a SynoptikonIntegratedAcceptedSession,
    verifier: &'a V,
}

impl<V: SynoptikonLiveSessionVerifier> HostAuthenticationContextVerifier
    for SynoptikonVerifier<'_, V>
{
    fn verify_live_session(&self, context: &HostAuthenticatedContext) -> Result<(), String> {
        if context.subject_id != self.session.actor.user_id
            || context.session_id != self.session.request_id
            || context.correlation_id != self.session.correlation_id
        {
            return Err("Synoptikon session identity changed during adaptation".to_string());
        }
        self.verifier.verify_live_session(self.session)
    }
}

fn session_digest(session_token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(session_token.as_bytes());
    format!("prosopikon:sha256:{:x}", hasher.finalize())
}
