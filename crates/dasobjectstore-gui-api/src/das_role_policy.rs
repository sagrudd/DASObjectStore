//! DASObjectStore authorization derived only from a verified host authority.
//!
//! This is deliberately a small, closed role vocabulary.  It does not inspect
//! POSIX users, groups, sudo state, request headers, cookies, passwords, or a
//! local authentication store.  Monas/Pistis remains responsible for deriving
//! the roles in a live, audience-bound host context.

use crate::VerifiedHostAuthenticatedContext;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DasCapability {
    View,
    Operate,
    Administer,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DasRolePolicy {
    may_view: bool,
    may_operate: bool,
    may_administer: bool,
}

impl DasRolePolicy {
    pub fn from_verified(context: &VerifiedHostAuthenticatedContext) -> Self {
        let mut policy = Self::default();
        for role in &context.context().roles {
            match role.as_str() {
                "storage_administrator" => {
                    policy.may_view = true;
                    policy.may_operate = true;
                    policy.may_administer = true;
                }
                "storage_operator" => {
                    policy.may_view = true;
                    policy.may_operate = true;
                }
                "storage_viewer" => policy.may_view = true,
                // Host contexts carry product-independent roles too.  They
                // must not imply a DAS permission.
                _ => {}
            }
        }
        policy
    }

    pub const fn permits(self, capability: DasCapability) -> bool {
        match capability {
            DasCapability::View => self.may_view,
            DasCapability::Operate => self.may_operate,
            DasCapability::Administer => self.may_administer,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DasCapability, DasRolePolicy};
    use crate::{
        accept_host_authenticated_context, HostAuthenticatedContext, HostAuthenticationAuthority,
        HostAuthenticationContextVerifier, VerifiedHostAuthenticatedContext, HOST_AUTH_AUDIENCE,
        HOST_AUTH_CONTEXT_SCHEMA_VERSION,
    };

    struct Live;

    impl HostAuthenticationContextVerifier for Live {
        fn verify_live_session(&self, _context: &HostAuthenticatedContext) -> Result<(), String> {
            Ok(())
        }
    }

    fn verified(roles: &[&str]) -> VerifiedHostAuthenticatedContext {
        accept_host_authenticated_context(
            HostAuthenticatedContext {
                schema_version: HOST_AUTH_CONTEXT_SCHEMA_VERSION.to_owned(),
                authority: HostAuthenticationAuthority::MonasStandalone,
                issuer: "monas".to_owned(),
                audience: HOST_AUTH_AUDIENCE.to_owned(),
                subject_id: "pistis-subject-1".to_owned(),
                session_id: "monas-session-1".to_owned(),
                roles: roles.iter().map(|role| (*role).to_owned()).collect(),
                issued_at_unix_seconds: 1_000,
                expires_at_unix_seconds: 2_000,
                correlation_id: "corr-1".to_owned(),
                csrf_binding_sha256: format!("sha256:{}", "a".repeat(64)),
            },
            1_500,
            &Live,
        )
        .expect("valid verified context")
    }

    #[test]
    fn hierarchy_is_derived_from_verified_storage_roles_only() {
        let viewer = DasRolePolicy::from_verified(&verified(&["storage_viewer"]));
        assert!(viewer.permits(DasCapability::View));
        assert!(!viewer.permits(DasCapability::Operate));
        assert!(!viewer.permits(DasCapability::Administer));

        let operator = DasRolePolicy::from_verified(&verified(&["storage_operator"]));
        assert!(operator.permits(DasCapability::View));
        assert!(operator.permits(DasCapability::Operate));
        assert!(!operator.permits(DasCapability::Administer));

        let administrator = DasRolePolicy::from_verified(&verified(&["storage_administrator"]));
        assert!(administrator.permits(DasCapability::View));
        assert!(administrator.permits(DasCapability::Operate));
        assert!(administrator.permits(DasCapability::Administer));
    }

    #[test]
    fn authenticated_and_unknown_roles_do_not_grant_storage_access() {
        let policy = DasRolePolicy::from_verified(&verified(&["authenticated", "unknown-role"]));
        assert!(!policy.permits(DasCapability::View));
        assert!(!policy.permits(DasCapability::Operate));
        assert!(!policy.permits(DasCapability::Administer));
    }
}
