//! Credential-free Pistis approval resolution boundary.

use crate::{AuthenticatedGuiActor, VerifiedHostAuthenticatedContext};
use dasobjectstore_daemon::RemoteEasyconnectApprovalContext;
use std::fmt::{self, Display};
use std::sync::Arc;

/// Fail-closed error returned by a deployment-owned Pistis grant resolver.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PistisApprovalResolutionError {
    message: String,
}

impl PistisApprovalResolutionError {
    /// Construct a redacted operator-safe resolution failure.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for PistisApprovalResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for PistisApprovalResolutionError {}

/// Resolve one exact DAS-owned grant for a live, host-verified Pistis actor.
///
/// Implementations must use deployment-owned policy and storage registries.
/// Browser request fields may select the exact ObjectStore but cannot supply
/// identity, bucket, permissions, prefixes, or control operations.
pub trait PistisEasyconnectApprovalResolver: Send + Sync {
    fn resolve(
        &self,
        actor: &AuthenticatedGuiActor,
        verified: &VerifiedHostAuthenticatedContext,
        requested_object_store: &str,
    ) -> Result<RemoteEasyconnectApprovalContext, PistisApprovalResolutionError>;
}

/// Cloneable Axum extension for a request-bound resolver.
#[derive(Clone)]
pub struct SharedPistisEasyconnectApprovalResolver(Arc<dyn PistisEasyconnectApprovalResolver>);

impl SharedPistisEasyconnectApprovalResolver {
    /// Wrap a concrete request-bound resolver.
    pub fn new(resolver: impl PistisEasyconnectApprovalResolver + 'static) -> Self {
        Self(Arc::new(resolver))
    }

    pub(crate) fn resolve(
        &self,
        actor: &AuthenticatedGuiActor,
        verified: &VerifiedHostAuthenticatedContext,
        requested_object_store: &str,
    ) -> Result<RemoteEasyconnectApprovalContext, PistisApprovalResolutionError> {
        self.0.resolve(actor, verified, requested_object_store)
    }
}
