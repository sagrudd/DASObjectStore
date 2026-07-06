use crate::{
    validate_synoptikon_integrated_host_boundary, validation::validate_context_id_like,
    SynoptikonIntegratedHostBoundaryContext, SynoptikonIntegratedHostBoundaryError,
};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

pub const SYNOPTIKON_INTEGRATED_SESSION_SCHEMA_VERSION: &str =
    "dasobjectstore.synoptikon_integrated_session.v1";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SynoptikonIntegratedSessionIssue {
    pub request_id: String,
    pub issued_at_unix_seconds: i64,
    pub expires_at_unix_seconds: i64,
    pub context: SynoptikonIntegratedHostBoundaryContext,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SynoptikonIntegratedAcceptedSession {
    pub schema_version: String,
    pub request_id: String,
    pub accepted_at_unix_seconds: i64,
    pub issued_at_unix_seconds: i64,
    pub expires_at_unix_seconds: i64,
    pub actor: SynoptikonIntegratedActor,
    pub correlation_id: String,
    pub storage_binding_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SynoptikonIntegratedActor {
    pub tenant_id: String,
    pub account_id: String,
    pub user_id: String,
    pub project_id: String,
    pub entitlement_id: String,
    pub roles: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SynoptikonIntegratedSessionError {
    HostBoundary(SynoptikonIntegratedHostBoundaryError),
    InvalidRequestId { reason: String },
    NegativeIssuedAt { value: i64 },
    NegativeExpiresAt { value: i64 },
    ExpiryPrecedesIssue { issued_at: i64, expires_at: i64 },
    Expired { expires_at: i64, accepted_at: i64 },
}

impl Display for SynoptikonIntegratedSessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HostBoundary(err) => write!(formatter, "{err}"),
            Self::InvalidRequestId { reason } => {
                write!(formatter, "invalid Synoptikon request_id: {reason}")
            }
            Self::NegativeIssuedAt { value } => {
                write!(formatter, "issued_at_unix_seconds must be non-negative: {value}")
            }
            Self::NegativeExpiresAt { value } => write!(
                formatter,
                "expires_at_unix_seconds must be non-negative: {value}"
            ),
            Self::ExpiryPrecedesIssue {
                issued_at,
                expires_at,
            } => write!(
                formatter,
                "expires_at_unix_seconds {expires_at} must not precede issued_at_unix_seconds {issued_at}"
            ),
            Self::Expired {
                expires_at,
                accepted_at,
            } => write!(
                formatter,
                "Synoptikon integrated session expired at {expires_at}; accepted at {accepted_at}"
            ),
        }
    }
}

impl std::error::Error for SynoptikonIntegratedSessionError {}

pub fn accept_synoptikon_integrated_session(
    issue: &SynoptikonIntegratedSessionIssue,
    accepted_at_unix_seconds: i64,
) -> Result<SynoptikonIntegratedAcceptedSession, SynoptikonIntegratedSessionError> {
    validate_synoptikon_integrated_host_boundary(&issue.context)
        .map_err(SynoptikonIntegratedSessionError::HostBoundary)?;
    validate_issue_window(issue, accepted_at_unix_seconds)?;
    validate_context_id_like("request_id", &issue.request_id)
        .map_err(|err| SynoptikonIntegratedSessionError::InvalidRequestId { reason: err.reason })?;

    Ok(SynoptikonIntegratedAcceptedSession {
        schema_version: SYNOPTIKON_INTEGRATED_SESSION_SCHEMA_VERSION.to_string(),
        request_id: issue.request_id.clone(),
        accepted_at_unix_seconds,
        issued_at_unix_seconds: issue.issued_at_unix_seconds,
        expires_at_unix_seconds: issue.expires_at_unix_seconds,
        actor: SynoptikonIntegratedActor {
            tenant_id: issue.context.tenant_id.clone(),
            account_id: issue.context.account_id.clone(),
            user_id: issue.context.user_id.clone(),
            project_id: issue.context.project_id.clone(),
            entitlement_id: issue.context.entitlement_id.clone(),
            roles: issue.context.roles.clone(),
        },
        correlation_id: issue.context.correlation_id.clone(),
        storage_binding_id: issue.context.storage_binding_id.clone(),
    })
}

fn validate_issue_window(
    issue: &SynoptikonIntegratedSessionIssue,
    accepted_at_unix_seconds: i64,
) -> Result<(), SynoptikonIntegratedSessionError> {
    if issue.issued_at_unix_seconds < 0 {
        return Err(SynoptikonIntegratedSessionError::NegativeIssuedAt {
            value: issue.issued_at_unix_seconds,
        });
    }
    if issue.expires_at_unix_seconds < 0 {
        return Err(SynoptikonIntegratedSessionError::NegativeExpiresAt {
            value: issue.expires_at_unix_seconds,
        });
    }
    if issue.expires_at_unix_seconds < issue.issued_at_unix_seconds {
        return Err(SynoptikonIntegratedSessionError::ExpiryPrecedesIssue {
            issued_at: issue.issued_at_unix_seconds,
            expires_at: issue.expires_at_unix_seconds,
        });
    }
    if issue.expires_at_unix_seconds <= accepted_at_unix_seconds {
        return Err(SynoptikonIntegratedSessionError::Expired {
            expires_at: issue.expires_at_unix_seconds,
            accepted_at: accepted_at_unix_seconds,
        });
    }
    Ok(())
}
