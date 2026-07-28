//! Credential-free Pistis actor boundary for Monas-hosted routes.
//!
//! Monas resolves a live Pistis session through Prosopikon on every request.
//! This module accepts only that non-secret result: it has no credential,
//! protocol verifier, or session store.

use dasobjectstore_gui_api::{
    accept_host_authenticated_context, HostAuthenticatedContext, HostAuthenticationAuthority,
    HostAuthenticationContextVerifier, VerifiedHostAuthenticatedContext, HOST_AUTH_AUDIENCE,
    HOST_AUTH_CONTEXT_SCHEMA_VERSION,
};
use prosopikon_core::{
    AssignmentStatus, AudienceBoundActorContext, AuthorityScope, EntitlementSubject,
    PrincipalStatus, ProductGrant,
};
use std::fmt::{self, Display};
use uuid::Uuid;

pub const PISTIS_DASOBJECTSTORE_PRODUCT_ID: &str = "dasobjectstore";

/// Non-secret same-origin bindings supplied by the Monas adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PistisHostRequestBindings {
    pub correlation_id: String,
    /// SHA-256 binding only; never the browser secret.
    pub csrf_binding_sha256: String,
}

/// Exactly one Prosopikon authority trusted by a host mount.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PistisActorBoundary {
    pub authority_id: Uuid,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PistisActorError {
    WrongAuthority,
    WrongAudience,
    InactivePrincipal,
    SessionPrincipalMismatch,
    SessionNotCurrent,
    InvalidBinding(String),
}

impl Display for PistisActorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongAuthority => formatter.write_str("unexpected Prosopikon authority"),
            Self::WrongAudience => formatter.write_str("actor is not bound to DASObjectStore"),
            Self::InactivePrincipal => formatter.write_str("Prosopikon principal is not active"),
            Self::SessionPrincipalMismatch => {
                formatter.write_str("session and principal identities do not match")
            }
            Self::SessionNotCurrent => formatter.write_str("Prosopikon session is not current"),
            Self::InvalidBinding(message) => write!(formatter, "invalid host binding: {message}"),
        }
    }
}

impl std::error::Error for PistisActorError {}

/// Adapt only a current, audience-bound Prosopikon actor. The caller must have
/// resolved it from Prosopikon on the current request.
pub fn accept_preverified_pistis_actor(
    actor: &AudienceBoundActorContext,
    bindings: &PistisHostRequestBindings,
    boundary: PistisActorBoundary,
    accepted_at_unix_seconds: i64,
) -> Result<VerifiedHostAuthenticatedContext, PistisActorError> {
    if actor.authority_id != boundary.authority_id {
        return Err(PistisActorError::WrongAuthority);
    }
    if actor.audience != PISTIS_DASOBJECTSTORE_PRODUCT_ID || actor.audience != HOST_AUTH_AUDIENCE {
        return Err(PistisActorError::WrongAudience);
    }
    if actor.actor.principal.status != PrincipalStatus::Active {
        return Err(PistisActorError::InactivePrincipal);
    }
    if actor.actor.session.principal_id != actor.actor.principal.principal_id {
        return Err(PistisActorError::SessionPrincipalMismatch);
    }
    let session = &actor.actor.session;
    if session.issued_at_utc.timestamp() > accepted_at_unix_seconds
        || session.verified_at_utc.timestamp() > accepted_at_unix_seconds
        || session.expires_at_utc.timestamp() <= accepted_at_unix_seconds
    {
        return Err(PistisActorError::SessionNotCurrent);
    }
    let context = HostAuthenticatedContext {
        schema_version: HOST_AUTH_CONTEXT_SCHEMA_VERSION.to_owned(),
        authority: HostAuthenticationAuthority::MonasStandalone,
        issuer: HostAuthenticationAuthority::MonasStandalone
            .issuer()
            .to_owned(),
        audience: HOST_AUTH_AUDIENCE.to_owned(),
        subject_id: actor.actor.principal.username.clone(),
        session_id: session.session_id.to_string(),
        roles: mapped_roles(actor),
        issued_at_unix_seconds: session.verified_at_utc.timestamp(),
        expires_at_unix_seconds: session.expires_at_utc.timestamp(),
        correlation_id: bindings.correlation_id.clone(),
        csrf_binding_sha256: bindings.csrf_binding_sha256.clone(),
    };
    accept_host_authenticated_context(
        context,
        accepted_at_unix_seconds,
        &BoundActorVerifier { actor },
    )
    .map_err(|error| PistisActorError::InvalidBinding(error.to_string()))
}

fn mapped_roles(context: &AudienceBoundActorContext) -> Vec<String> {
    let principal_id = context.actor.principal.principal_id;
    let active_tenant_id = context.actor.active_tenant_id;
    let mut grants = (false, false, false);
    for value in &context.actor.product_entitlements {
        let matching_subject = match value.subject {
            EntitlementSubject::Principal {
                principal_id: value,
            } => value == principal_id,
            EntitlementSubject::Tenant { tenant_id } => Some(tenant_id) == active_tenant_id,
        };
        if value.status != AssignmentStatus::Active
            || value.product_id != PISTIS_DASOBJECTSTORE_PRODUCT_ID
            || !matching_subject
        {
            continue;
        }
        match value.grant {
            ProductGrant::View => grants.0 = true,
            ProductGrant::Operate => grants.1 = true,
            ProductGrant::Administer => grants.2 = true,
        }
    }
    for value in &context.actor.role_assignments {
        if value.status != AssignmentStatus::Active
            || value.principal_id != principal_id
            || value.role.domain != PISTIS_DASOBJECTSTORE_PRODUCT_ID
            || !scope_applies(&value.scope, active_tenant_id)
        {
            continue;
        }
        match value.role.role_name.as_str() {
            "viewer" => grants.0 = true,
            "operator" => grants.1 = true,
            "administrator" => grants.2 = true,
            _ => {}
        }
    }
    let mut roles = vec!["authenticated".to_owned()];
    if grants.0 || grants.1 || grants.2 {
        roles.push("storage_viewer".to_owned());
    }
    if grants.1 || grants.2 {
        roles.push("storage_operator".to_owned());
    }
    if grants.2 {
        roles.push("storage_administrator".to_owned());
    }
    roles
}

fn scope_applies(scope: &AuthorityScope, active_tenant_id: Option<Uuid>) -> bool {
    match scope {
        AuthorityScope::System => true,
        AuthorityScope::Product { product_id } => product_id == PISTIS_DASOBJECTSTORE_PRODUCT_ID,
        AuthorityScope::Tenant { tenant_id } => Some(*tenant_id) == active_tenant_id,
        AuthorityScope::TenantProduct {
            tenant_id,
            product_id,
        } => Some(*tenant_id) == active_tenant_id && product_id == PISTIS_DASOBJECTSTORE_PRODUCT_ID,
        AuthorityScope::HostProject { .. } => false,
    }
}

struct BoundActorVerifier<'a> {
    actor: &'a AudienceBoundActorContext,
}

impl HostAuthenticationContextVerifier for BoundActorVerifier<'_> {
    fn verify_live_session(&self, context: &HostAuthenticatedContext) -> Result<(), String> {
        let session = &self.actor.actor.session;
        if context.session_id != session.session_id.to_string()
            || context.subject_id != self.actor.actor.principal.username
            || context.expires_at_unix_seconds != session.expires_at_utc.timestamp()
        {
            return Err("derived context does not match preverified actor".to_owned());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    const NOW: i64 = 1_750_000_000;

    fn fixture() -> AudienceBoundActorContext {
        serde_json::from_value(json!({
            "authority_id": "00000000-0000-0000-0000-000000000001",
            "audience": "dasobjectstore",
            "actor": {
                "session": {
                    "session_id": "00000000-0000-0000-0000-000000000002",
                    "principal_id": "00000000-0000-0000-0000-000000000003",
                    "authentication_method": "device",
                    "issued_at_utc": "2025-06-15T14:39:00Z",
                    "verified_at_utc": "2025-06-15T14:39:10Z",
                    "expires_at_utc": "2025-06-15T15:39:00Z",
                    "authority_revision": 7
                },
                "principal": {
                    "principal_id": "00000000-0000-0000-0000-000000000003",
                    "username": "stephen",
                    "display_name": "Stephen",
                    "email": "stephen@mnemosyne.co.uk",
                    "kind": "person",
                    "status": "active",
                    "created_at_utc": "2025-06-01T00:00:00Z",
                    "updated_at_utc": "2025-06-01T00:00:00Z"
                },
                "active_tenant_id": null,
                "memberships": [],
                "role_assignments": [],
                "product_entitlements": [{
                    "assignment_id": "00000000-0000-0000-0000-000000000004",
                    "subject": {
                        "kind": "principal",
                        "principal_id": "00000000-0000-0000-0000-000000000003"
                    },
                    "product_id": "dasobjectstore",
                    "grant": "administer",
                    "status": "active",
                    "created_at_utc": "2025-06-01T00:00:00Z",
                    "updated_at_utc": "2025-06-01T00:00:00Z"
                }]
            }
        }))
        .expect("valid fixture")
    }

    fn bindings() -> PistisHostRequestBindings {
        PistisHostRequestBindings {
            correlation_id: "pistis:test:1".to_owned(),
            csrf_binding_sha256: format!("sha256:{}", "a".repeat(64)),
        }
    }

    fn boundary(actor: &AudienceBoundActorContext) -> PistisActorBoundary {
        PistisActorBoundary {
            authority_id: actor.authority_id,
        }
    }

    #[test]
    fn maps_only_explicit_das_admin_grant() {
        let actor = fixture();
        let accepted = accept_preverified_pistis_actor(&actor, &bindings(), boundary(&actor), NOW)
            .expect("accepted actor");
        assert_eq!(accepted.context().subject_id, "stephen");
        assert_eq!(
            accepted.context().roles,
            [
                "authenticated",
                "storage_viewer",
                "storage_operator",
                "storage_administrator"
            ]
        );
    }

    #[test]
    fn unrelated_and_suspended_grants_cannot_elevate_actor() {
        let base = serde_json::to_value(fixture()).expect("serialize fixture");
        for mutation in [
            ("product_id", Value::String("jenkins".to_owned())),
            ("status", Value::String("suspended".to_owned())),
        ] {
            let mut value = base.clone();
            value["actor"]["product_entitlements"][0][mutation.0] = mutation.1;
            let actor: AudienceBoundActorContext =
                serde_json::from_value(value).expect("mutated actor");
            let accepted =
                accept_preverified_pistis_actor(&actor, &bindings(), boundary(&actor), NOW)
                    .expect("accepted without elevation");
            assert_eq!(accepted.context().roles, ["authenticated"]);
        }
    }

    #[test]
    fn rejects_absent_authority_wrong_audience_and_stale_session() {
        let actor = fixture();
        let wrong_authority = PistisActorBoundary {
            authority_id: Uuid::nil(),
        };
        assert!(matches!(
            accept_preverified_pistis_actor(&actor, &bindings(), wrong_authority, NOW),
            Err(PistisActorError::WrongAuthority)
        ));

        let mut wrong_audience = actor.clone();
        wrong_audience.audience = "jenkins".to_owned();
        assert!(matches!(
            accept_preverified_pistis_actor(&wrong_audience, &bindings(), boundary(&actor), NOW),
            Err(PistisActorError::WrongAudience)
        ));

        assert!(matches!(
            accept_preverified_pistis_actor(
                &actor,
                &bindings(),
                boundary(&actor),
                actor.actor.session.expires_at_utc.timestamp()
            ),
            Err(PistisActorError::SessionNotCurrent)
        ));
    }

    #[test]
    fn rejects_inactive_or_mismatched_principal_and_malformed_bindings() {
        let actor = fixture();
        let mut inactive = serde_json::to_value(&actor).expect("serialize");
        inactive["actor"]["principal"]["status"] = json!("locked");
        let inactive = serde_json::from_value(inactive).expect("inactive actor");
        assert!(matches!(
            accept_preverified_pistis_actor(&inactive, &bindings(), boundary(&actor), NOW),
            Err(PistisActorError::InactivePrincipal)
        ));

        let mut mismatch = actor.clone();
        mismatch.actor.session.principal_id = Uuid::nil();
        assert!(matches!(
            accept_preverified_pistis_actor(&mismatch, &bindings(), boundary(&actor), NOW),
            Err(PistisActorError::SessionPrincipalMismatch)
        ));

        let mut malformed = bindings();
        malformed.csrf_binding_sha256 = "raw-secret".to_owned();
        assert!(matches!(
            accept_preverified_pistis_actor(&actor, &malformed, boundary(&actor), NOW),
            Err(PistisActorError::InvalidBinding(_))
        ));
    }
}
