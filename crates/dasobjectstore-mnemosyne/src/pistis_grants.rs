//! Deployment-owned Pistis principal-to-ObjectStore grant resolution.

use dasobjectstore_core::store::ExportPolicy;
use dasobjectstore_daemon::{
    api::remote_easyconnect_control_operations, PistisGrantPolicyError, PistisGrantPolicyStore,
    PistisObjectStoreGrantRecord, RemoteEasyconnectApprovalContext, RemoteEasyconnectAuthProvider,
    RemoteEasyconnectObjectStoreGrant,
};
use dasobjectstore_gui_api::{
    AuthenticatedGuiActor, DasCapability, DasRolePolicy, PistisApprovalResolutionError,
    PistisEasyconnectApprovalResolver, VerifiedHostAuthenticatedContext,
};
use dasobjectstore_object_service::{bucket_name_for_definition, read_store_registry};
use std::path::PathBuf;
use uuid::Uuid;

/// Request-bound resolver that never accepts identity or grant facts from the browser.
#[derive(Clone, Debug)]
pub struct FilePistisEasyconnectApprovalResolver {
    authority_id: Uuid,
    principal_id: Uuid,
    session_id: Uuid,
    grant_registry_path: PathBuf,
    store_registry_path: PathBuf,
}

impl FilePistisEasyconnectApprovalResolver {
    pub fn new(
        authority_id: Uuid,
        principal_id: Uuid,
        session_id: Uuid,
        grant_registry_path: impl Into<PathBuf>,
        store_registry_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            authority_id,
            principal_id,
            session_id,
            grant_registry_path: grant_registry_path.into(),
            store_registry_path: store_registry_path.into(),
        }
    }

    fn resolve_exact_grant(
        &self,
        requested_object_store: &str,
    ) -> Result<
        (
            PistisObjectStoreGrantRecord,
            RemoteEasyconnectObjectStoreGrant,
        ),
        PistisGrantResolutionError,
    > {
        if requested_object_store.trim().is_empty() {
            return Err(PistisGrantResolutionError::UnknownObjectStore);
        }
        let registry = PistisGrantPolicyStore::new(&self.grant_registry_path)
            .inspect()?
            .ok_or_else(|| {
                PistisGrantPolicyError::RegistryUnavailable(
                    "grant registry does not exist".to_owned(),
                )
            })?;
        let matches = registry
            .records
            .iter()
            .filter(|record| {
                record.active
                    && record.authority_id == self.authority_id
                    && record.principal_id == self.principal_id
                    && record.object_store_id == requested_object_store
            })
            .cloned()
            .collect::<Vec<_>>();
        let [record] = matches.as_slice() else {
            return if matches.is_empty() {
                Err(PistisGrantResolutionError::GrantNotFound)
            } else {
                Err(PistisGrantResolutionError::AmbiguousGrant)
            };
        };
        if !record.can_write {
            return Err(PistisGrantResolutionError::WriteNotGranted);
        }
        let definitions = read_store_registry(&self.store_registry_path)
            .map_err(|error| PistisGrantResolutionError::StoreRegistry(error.to_string()))?;
        let stores = definitions
            .iter()
            .filter(|definition| definition.store_id.as_str() == requested_object_store)
            .collect::<Vec<_>>();
        let [definition] = stores.as_slice() else {
            return Err(PistisGrantResolutionError::UnknownObjectStore);
        };
        if definition.policy.export_policy != ExportPolicy::S3 {
            return Err(PistisGrantResolutionError::StoreNotS3Exported);
        }
        let bucket = bucket_name_for_definition(definition)
            .map_err(|error| PistisGrantResolutionError::StoreRegistry(error.to_string()))?;
        let grant = RemoteEasyconnectObjectStoreGrant {
            object_store: requested_object_store.to_owned(),
            bucket,
            can_read: record.can_read,
            can_write: record.can_write,
            writer_group: definition.writer_group.clone(),
            object_type: definition.policy.class.name().to_owned(),
            control_operations: remote_easyconnect_control_operations(record.can_write),
            allowed_prefixes: record.allowed_prefixes.clone(),
        };
        grant
            .validate()
            .map_err(|error| PistisGrantResolutionError::InvalidGrant(error.to_string()))?;
        Ok((record.clone(), grant))
    }
}

impl PistisEasyconnectApprovalResolver for FilePistisEasyconnectApprovalResolver {
    fn resolve(
        &self,
        actor: &AuthenticatedGuiActor,
        verified: &VerifiedHostAuthenticatedContext,
        requested_object_store: &str,
    ) -> Result<RemoteEasyconnectApprovalContext, PistisApprovalResolutionError> {
        let context = verified.context();
        if actor.subject_id != self.principal_id.to_string()
            || context.subject_id != actor.subject_id
            || context.session_id != self.session_id.to_string()
            || !DasRolePolicy::from_verified(verified).permits(DasCapability::Operate)
        {
            return Err(PistisApprovalResolutionError::new(
                "verified actor lacks the configured Pistis storage-operator grant",
            ));
        }
        let (record, grant) = self
            .resolve_exact_grant(requested_object_store)
            .map_err(|error| PistisApprovalResolutionError::new(error.to_string()))?;
        Ok(RemoteEasyconnectApprovalContext {
            authority_id: self.authority_id.to_string(),
            principal_id: self.principal_id.to_string(),
            session_id: self.session_id.to_string(),
            auth_provider: RemoteEasyconnectAuthProvider::Pistis,
            allowed_object_stores: vec![grant],
            host_session_expires_at_utc: dasobjectstore_core::utc::format_utc_timestamp_seconds(
                context.expires_at_unix_seconds,
            ),
            correlation_id: context.correlation_id.clone(),
            audit_identity: format!(
                "pistis-grant:{}:{}",
                record.policy_revision, record.record_id
            ),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PistisGrantResolutionError {
    RegistryUnavailable(String),
    InvalidRegistry(String),
    StoreRegistry(String),
    GrantNotFound,
    AmbiguousGrant,
    WriteNotGranted,
    UnknownObjectStore,
    StoreNotS3Exported,
    InvalidGrant(String),
}

impl From<PistisGrantPolicyError> for PistisGrantResolutionError {
    fn from(error: PistisGrantPolicyError) -> Self {
        match error {
            PistisGrantPolicyError::RegistryUnavailable(message) => {
                Self::RegistryUnavailable(message)
            }
            other => Self::InvalidRegistry(other.to_string()),
        }
    }
}

impl std::fmt::Display for PistisGrantResolutionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RegistryUnavailable(_) => {
                formatter.write_str("Pistis grant registry unavailable")
            }
            Self::InvalidRegistry(_) => formatter.write_str("Pistis grant registry is invalid"),
            Self::StoreRegistry(_) => formatter.write_str("ObjectStore registry is invalid"),
            Self::GrantNotFound => formatter.write_str("exact Pistis ObjectStore grant not found"),
            Self::AmbiguousGrant => formatter.write_str("Pistis ObjectStore grant is ambiguous"),
            Self::WriteNotGranted => formatter.write_str("Pistis ObjectStore grant is read-only"),
            Self::UnknownObjectStore => formatter.write_str("requested ObjectStore is unknown"),
            Self::StoreNotS3Exported => {
                formatter.write_str("requested ObjectStore is not S3 exported")
            }
            Self::InvalidGrant(_) => formatter.write_str("derived ObjectStore grant is invalid"),
        }
    }
}

impl std::error::Error for PistisGrantResolutionError {}

#[cfg(test)]
mod tests {
    use super::*;
    use dasobjectstore_core::utc::format_utc_timestamp_seconds;
    use dasobjectstore_core::{
        ids::StoreId,
        store::{StoreClass, StorePolicy},
    };
    use dasobjectstore_daemon::runtime::{
        FileBackedRemoteEasyconnectPairedSessionStore, RemoteEasyconnectPairedSessionStore,
    };
    use dasobjectstore_daemon::{
        DaemonClient, DaemonClientError, DaemonRequestHandler, DaemonServiceLifecycleRequest,
        DaemonServiceLifecycleResponse, DaemonServiceOrchestrator, DaemonServiceProvisionRequest,
        DaemonServiceProvisionResponse, DaemonServiceRuntimeError, DaemonServiceStatusRequest,
        DaemonServiceStatusResponse, FixedDaemonClock, InProcessDaemonTransport,
        PistisObjectStoreGrantRegistry, RemoteEasyconnectApprovePairingRequest,
        RemoteEasyconnectAuthProvider, RemoteEasyconnectCreatePairingRequest,
        RemoteEasyconnectExchangePairingRequest, PISTIS_GRANT_REGISTRY_SCHEMA_VERSION,
    };
    use dasobjectstore_gui_api::{
        accept_host_authenticated_context, AuthenticatedActorAuthority, HostAuthenticatedContext,
        HostAuthenticationAuthority, HostAuthenticationContextVerifier, HOST_AUTH_AUDIENCE,
        HOST_AUTH_CONTEXT_SCHEMA_VERSION,
    };
    use dasobjectstore_object_service::StoreServiceDefinition;
    use std::fs;

    const NOW: i64 = 1_750_000_000;

    struct LiveVerifier;

    impl HostAuthenticationContextVerifier for LiveVerifier {
        fn verify_live_session(&self, _: &HostAuthenticatedContext) -> Result<(), String> {
            Ok(())
        }
    }

    fn fixture(
        records: Vec<PistisObjectStoreGrantRecord>,
    ) -> (
        FilePistisEasyconnectApprovalResolver,
        AuthenticatedGuiActor,
        VerifiedHostAuthenticatedContext,
        PathBuf,
    ) {
        let authority_id = Uuid::from_u128(1);
        let principal_id = Uuid::from_u128(2);
        let session_id = Uuid::from_u128(3);
        let root = std::env::temp_dir().join(format!("das-pistis-grants-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("temporary directory");
        let grants_path = root.join("pistis-grants.json");
        let stores_path = root.join("stores.json");
        fs::write(
            &grants_path,
            serde_json::to_vec_pretty(&PistisObjectStoreGrantRegistry {
                schema_version: PISTIS_GRANT_REGISTRY_SCHEMA_VERSION.to_owned(),
                revision: 7,
                records,
                audit_events: Vec::new(),
            })
            .expect("serialize grants"),
        )
        .expect("write grants");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&grants_path, fs::Permissions::from_mode(0o600))
                .expect("private grant permissions");
        }
        fs::write(
            &stores_path,
            serde_json::to_vec_pretty(&vec![StoreServiceDefinition {
                store_id: StoreId::new("epic_collection").expect("store id"),
                policy: StorePolicy::defaults_for(StoreClass::GeneratedData),
                bucket_name: Some("dos-epic-collection".to_owned()),
                reader_group: None,
                writer_group: Some("mnemosyne".to_owned()),
                public: false,
            }])
            .expect("serialize stores"),
        )
        .expect("write stores");
        let host = HostAuthenticatedContext {
            schema_version: HOST_AUTH_CONTEXT_SCHEMA_VERSION.to_owned(),
            authority: HostAuthenticationAuthority::MonasStandalone,
            issuer: HostAuthenticationAuthority::MonasStandalone
                .issuer()
                .to_owned(),
            audience: HOST_AUTH_AUDIENCE.to_owned(),
            subject_id: principal_id.to_string(),
            session_id: session_id.to_string(),
            roles: vec!["authenticated".to_owned(), "storage_operator".to_owned()],
            issued_at_unix_seconds: NOW - 60,
            expires_at_unix_seconds: NOW + 600,
            correlation_id: "test:correlation".to_owned(),
            csrf_binding_sha256: format!("sha256:{}", "a".repeat(64)),
        };
        let verified =
            accept_host_authenticated_context(host, NOW, &LiveVerifier).expect("verified context");
        let actor = AuthenticatedGuiActor {
            subject_id: principal_id.to_string(),
            authority: AuthenticatedActorAuthority::MonasStandalone,
            roles: vec!["authenticated".to_owned(), "storage_operator".to_owned()],
            expires_at_unix_seconds: Some(NOW + 600),
            correlation_id: Some("test:correlation".to_owned()),
        };
        (
            FilePistisEasyconnectApprovalResolver::new(
                authority_id,
                principal_id,
                session_id,
                grants_path,
                stores_path,
            ),
            actor,
            verified,
            root,
        )
    }

    fn grant() -> PistisObjectStoreGrantRecord {
        PistisObjectStoreGrantRecord {
            record_id: Uuid::from_u128(4),
            authority_id: Uuid::from_u128(1),
            principal_id: Uuid::from_u128(2),
            object_store_id: "epic_collection".to_owned(),
            policy_revision: 7,
            active: true,
            can_read: true,
            can_write: true,
            allowed_prefixes: vec!["".to_owned()],
        }
    }

    #[test]
    fn resolves_one_exact_write_grant_from_deployment_registries() {
        let (resolver, actor, verified, root) = fixture(vec![grant()]);
        let context = resolver
            .resolve(&actor, &verified, "epic_collection")
            .expect("exact grant");
        assert_eq!(context.authority_id, Uuid::from_u128(1).to_string());
        assert_eq!(context.principal_id, Uuid::from_u128(2).to_string());
        assert_eq!(context.allowed_object_stores.len(), 1);
        assert_eq!(
            context.allowed_object_stores[0].bucket,
            "dos-epic-collection"
        );
        assert!(context.allowed_object_stores[0].can_write);
        assert!(context.audit_identity.starts_with("pistis-grant:7:"));
        fs::remove_dir_all(root).expect("remove temporary directory");
    }

    #[test]
    fn resolved_grant_drives_durable_create_approve_restart_and_exchange() {
        let (resolver, actor, verified, root) = fixture(vec![grant()]);
        let pairing_path = root.join("state/remote/pairings.json");
        let session_path = root.join("state/remote/sessions.json");
        let clock = format_utc_timestamp_seconds(NOW);
        let client = daemon_client(&pairing_path, &session_path, &clock);
        let created = client
            .remote_easyconnect_create_pairing(RemoteEasyconnectCreatePairingRequest {
                client_name: "dasobjectstore-remote".to_owned(),
                callback_url:
                    "http://127.0.0.1:49321/products/dasobjectstore/remote/easyconnect/callback"
                        .to_owned(),
                requested_object_store: Some("epic_collection".to_owned()),
                requested_session_lifetime_seconds: Some(300),
                client_request_id: Some("client-request-1".to_owned()),
            })
            .expect("pairing is created");
        let approval_context = resolver
            .resolve(&actor, &verified, "epic_collection")
            .expect("deployment-owned grant resolves");
        assert_eq!(
            approval_context.auth_provider,
            RemoteEasyconnectAuthProvider::Pistis
        );
        assert!(approval_context
            .audit_identity
            .starts_with("pistis-grant:7:"));
        let approved = client
            .remote_easyconnect_approve_pairing(RemoteEasyconnectApprovePairingRequest {
                pairing_id: created.pairing_id.clone(),
                user_code: created.user_code.clone(),
                approval_context,
            })
            .expect("pairing is approved");
        drop(client);

        let restarted = daemon_client(&pairing_path, &session_path, &clock);
        let exchange_request = RemoteEasyconnectExchangePairingRequest {
            pairing_id: created.pairing_id.clone(),
            exchange_code: approved.exchange_code,
            client_request_id: Some("client-request-2".to_owned()),
        };
        let exchanged = restarted
            .remote_easyconnect_exchange_pairing(exchange_request.clone())
            .expect("approved pairing survives restart and exchanges");
        assert_eq!(
            exchanged.auth_provider,
            RemoteEasyconnectAuthProvider::Pistis
        );
        assert_eq!(exchanged.approved_actor, Uuid::from_u128(2).to_string());
        assert_eq!(exchanged.object_stores.len(), 1);
        assert_eq!(exchanged.object_stores[0].object_store, "epic_collection");
        assert_eq!(exchanged.object_stores[0].bucket, "dos-epic-collection");
        assert!(exchanged.object_stores[0].can_write);
        assert!(
            restarted
                .remote_easyconnect_exchange_pairing(exchange_request)
                .is_err(),
            "exchange code is one-use"
        );

        let persisted = FileBackedRemoteEasyconnectPairedSessionStore::new(&session_path)
            .get_by_pairing_id(&created.pairing_id)
            .expect("session store remains readable")
            .expect("one session was persisted");
        assert_eq!(persisted.authority_id, Uuid::from_u128(1).to_string());
        assert_eq!(persisted.principal_id, Uuid::from_u128(2).to_string());
        assert_eq!(
            persisted.originating_authority_expires_at_utc,
            Some(format_utc_timestamp_seconds(NOW + 600))
        );
        let evidence = format!(
            "{}\n{}",
            fs::read_to_string(&pairing_path).expect("pairing evidence"),
            fs::read_to_string(&session_path).expect("session evidence")
        );
        assert!(!evidence.contains("stephen@mnemosyne.co.uk"));
        fs::remove_dir_all(root).expect("remove temporary directory");
    }

    #[test]
    fn rejects_missing_read_only_ambiguous_and_substituted_grants() {
        for records in [
            vec![],
            {
                let mut value = grant();
                value.can_write = false;
                vec![value]
            },
            vec![grant(), grant()],
        ] {
            let (resolver, actor, verified, root) = fixture(records);
            assert!(resolver
                .resolve(&actor, &verified, "epic_collection")
                .is_err());
            fs::remove_dir_all(root).expect("remove temporary directory");
        }
        let (resolver, actor, verified, root) = fixture(vec![grant()]);
        assert!(resolver
            .resolve(&actor, &verified, "different-store")
            .is_err());
        fs::remove_dir_all(root).expect("remove temporary directory");
    }

    #[test]
    fn rejects_stale_registry_and_actor_substitution() {
        let mut stale = grant();
        stale.policy_revision = 6;
        let (resolver, actor, verified, root) = fixture(vec![stale]);
        assert!(resolver
            .resolve(&actor, &verified, "epic_collection")
            .is_err());
        fs::remove_dir_all(root).expect("remove temporary directory");

        let (resolver, mut actor, verified, root) = fixture(vec![grant()]);
        actor.subject_id = Uuid::from_u128(99).to_string();
        assert!(resolver
            .resolve(&actor, &verified, "epic_collection")
            .is_err());
        fs::remove_dir_all(root).expect("remove temporary directory");
    }

    fn daemon_client(
        pairing_path: &std::path::Path,
        session_path: &std::path::Path,
        now: &str,
    ) -> DaemonClient<impl dasobjectstore_daemon::DaemonClientTransport> {
        let handler = DaemonRequestHandler::new(
            AcceptanceOrchestrator,
            FixedDaemonClock::new(now.to_owned()),
        )
        .with_remote_easyconnect_pairing_store_path(pairing_path)
        .with_remote_easyconnect_session_store_path(session_path);
        DaemonClient::new(InProcessDaemonTransport::new(move |request| {
            handler
                .handle(request)
                .map_err(|error| DaemonClientError::Transport(error.to_string()))
        }))
    }

    struct AcceptanceOrchestrator;

    impl DaemonServiceOrchestrator for AcceptanceOrchestrator {
        fn status(
            &self,
            _: DaemonServiceStatusRequest,
        ) -> Result<DaemonServiceStatusResponse, DaemonServiceRuntimeError> {
            Err(unsupported("status"))
        }

        fn lifecycle(
            &self,
            _: DaemonServiceLifecycleRequest,
            _: &str,
        ) -> Result<DaemonServiceLifecycleResponse, DaemonServiceRuntimeError> {
            Err(unsupported("lifecycle"))
        }

        fn provision(
            &self,
            _: DaemonServiceProvisionRequest,
            _: &str,
        ) -> Result<DaemonServiceProvisionResponse, DaemonServiceRuntimeError> {
            Err(unsupported("provision"))
        }
    }

    fn unsupported(operation: &str) -> DaemonServiceRuntimeError {
        DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!("{operation} is outside Pistis grant acceptance"),
        }
    }
}
