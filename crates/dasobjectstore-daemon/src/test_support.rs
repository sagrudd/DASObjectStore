//! Disposable, feature-gated integration support for the Synoptikon gateway.
//! This module is absent from normal package builds.

use crate::api::{
    DaemonServiceLifecycleRequest, DaemonServiceLifecycleResponse, DaemonServiceProvisionRequest,
    DaemonServiceProvisionResponse, DaemonServiceStatusRequest, DaemonServiceStatusResponse,
};
use crate::runtime::{
    run_one_durable_destage, upsert_profile_binding, BackendProfileBinding,
    CapacityAdmissionProvider, DaemonServiceRuntimeError, DurableDestageOutcome,
    DurableDestageWorkerConfig, FileBackedCapacityAdmissionProvider, StatvfsCapacitySpaceProbe,
};
use crate::{DaemonRequestHandler, DaemonServiceOrchestrator, FixedDaemonClock};
use dasobjectstore_core::deployment::{DeploymentProfile, HostMode};
use dasobjectstore_core::ids::{PoolId, StoreId};
use dasobjectstore_core::manifest::{
    BackendReference, ObjectStoreManifest, OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
};
use dasobjectstore_core::protection::ProtectionPolicy;
use dasobjectstore_core::store::{
    AcknowledgementPolicy, CapacityPolicy, EnclosurePlacement, StoreClass, StorePolicy,
};
use dasobjectstore_metadata::{initialize_pool, PoolInitOptions};
use dasobjectstore_object_service::{
    upsert_store_definition, ObjectServiceProviderId, ServiceState, StoreServiceDefinition,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const SYNOPTIKON_TEST_FIXTURE_MARKER: &str =
    "dasobjectstore.synoptikon_projection_test_fixture.v1\n";

pub struct SynoptikonProjectionTestService {
    capacity: Arc<FileBackedCapacityAdmissionProvider<StatvfsCapacitySpaceProbe>>,
}

impl DaemonServiceOrchestrator for SynoptikonProjectionTestService {
    fn capacity_provider(&self) -> Option<Arc<dyn CapacityAdmissionProvider>> {
        Some(self.capacity.clone())
    }

    fn status(
        &self,
        _request: DaemonServiceStatusRequest,
    ) -> Result<DaemonServiceStatusResponse, DaemonServiceRuntimeError> {
        Ok(DaemonServiceStatusResponse {
            provider_id: ObjectServiceProviderId::Garage,
            state: ServiceState::Running,
            endpoint: Some("https://192.168.0.193:3900".to_owned()),
            message: None,
            detail: None,
        })
    }

    fn lifecycle(
        &self,
        _request: DaemonServiceLifecycleRequest,
        _accepted_at_utc: &str,
    ) -> Result<DaemonServiceLifecycleResponse, DaemonServiceRuntimeError> {
        Err(unsupported("test fixture lifecycle"))
    }

    fn provision(
        &self,
        _request: DaemonServiceProvisionRequest,
        _accepted_at_utc: &str,
    ) -> Result<DaemonServiceProvisionResponse, DaemonServiceRuntimeError> {
        Err(unsupported("test fixture provisioning"))
    }
}

fn unsupported(operation: &str) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::UnsupportedOperation {
        operation: operation.to_owned(),
    }
}

#[derive(Clone, Debug)]
pub struct SynoptikonProjectionTestFixture {
    pub root: PathBuf,
    pub store_registry_path: PathBuf,
    pub subobject_registry_path: PathBuf,
    pub profile_binding_registry_path: PathBuf,
    pub live_sqlite_path: PathBuf,
    pub ssd_root: PathBuf,
    pub backend_root: PathBuf,
    pub hdd_root: PathBuf,
    pub projection_ledger_path: PathBuf,
    now_utc: String,
}

impl SynoptikonProjectionTestFixture {
    pub fn new(root: impl AsRef<Path>, now_utc: impl Into<String>) -> Result<Self, String> {
        let root = root.as_ref().to_path_buf();
        if root.exists() {
            return Err("fixture root must not already exist".to_owned());
        }
        fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        let marker_path = root.join(".synoptikon-test-fixture");
        fs::write(&marker_path, SYNOPTIKON_TEST_FIXTURE_MARKER)
            .map_err(|error| error.to_string())?;
        fs::set_permissions(&marker_path, fs::Permissions::from_mode(0o600))
            .map_err(|error| error.to_string())?;
        let ssd_root = root.join("ssd");
        let report = initialize_pool(&PoolInitOptions::new(
            &ssd_root,
            PoolId::new("synoptikon-fixture-pool").map_err(|error| error.to_string())?,
            now_utc.into(),
        ))
        .map_err(|error| error.to_string())?;
        let now_utc = rusqlite::Connection::open(&report.live_sqlite_path)
            .and_then(|connection| {
                connection.query_row(
                    "SELECT created_at_utc FROM pools WHERE pool_id='synoptikon-fixture-pool'",
                    [],
                    |row| row.get::<_, String>(0),
                )
            })
            .map_err(|error| error.to_string())?;
        let backend_root = ssd_root.join("synoptikon-backend");
        fs::create_dir_all(&backend_root).map_err(|error| error.to_string())?;
        let hdd_root = root.join("hdd");
        let disk_root = hdd_root.join("fixture-disk-1");
        fs::create_dir_all(disk_root.join(".dasobjectstore")).map_err(|error| error.to_string())?;
        fs::write(
            disk_root.join(".dasobjectstore/device.env"),
            "role=hdd:fixture-disk-1\n",
        )
        .map_err(|error| error.to_string())?;
        rusqlite::Connection::open(&report.live_sqlite_path)
            .and_then(|connection| {
                connection.execute(
                    "INSERT INTO stores (
                        store_id,pool_id,class,policy_json,created_at_utc,updated_at_utc
                     ) VALUES (?1,?2,'generated_data','{}',?3,?3)",
                    rusqlite::params!["synoptikon-demo", "synoptikon-fixture-pool", &now_utc,],
                )?;
                connection.execute(
                    "INSERT INTO disks (
                        disk_id,pool_id,role,state,created_at_utc,updated_at_utc
                     ) VALUES (?1,?2,'hdd','Healthy',?3,?3)",
                    rusqlite::params!["fixture-disk-1", "synoptikon-fixture-pool", &now_utc,],
                )
            })
            .map_err(|error| error.to_string())?;

        let store_registry_path = root.join("stores.json");
        let subobject_registry_path = root.join("subobjects.json");
        let profile_binding_registry_path = root.join("profile-bindings.json");
        let projection_ledger_path = root.join("projection-authority/ledger.json");
        let projection_authority_dir = projection_ledger_path
            .parent()
            .ok_or_else(|| "projection authority directory is unavailable".to_owned())?;
        fs::create_dir_all(projection_authority_dir).map_err(|error| error.to_string())?;
        fs::set_permissions(projection_authority_dir, fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
        let store_id = StoreId::new("synoptikon-demo").map_err(|error| error.to_string())?;
        let mut policy = StorePolicy::defaults_for(StoreClass::GeneratedData);
        policy.acknowledgement_policy = AcknowledgementPolicy::AfterHddPlacement;
        policy.copies = 1;
        policy.enclosure_placement = EnclosurePlacement::Ignore;
        policy.capacity = CapacityPolicy::bounded(1024 * 1024 * 1024, 1024 * 1024);
        upsert_store_definition(
            &store_registry_path,
            StoreServiceDefinition {
                store_id: store_id.clone(),
                policy: policy.clone(),
                bucket_name: Some("synoptikon-demo".to_owned()),
                reader_group: None,
                writer_group: None,
                public: false,
            },
        )
        .map_err(|error| error.to_string())?;
        upsert_profile_binding(
            &profile_binding_registry_path,
            BackendProfileBinding {
                manifest: ObjectStoreManifest {
                    schema_version: OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
                    store_id: store_id.clone(),
                    deployment_profile: DeploymentProfile::Folder,
                    host_mode: HostMode::PerUser,
                    protection: ProtectionPolicy::LocalOnly,
                    backend: BackendReference::Folder {
                        root_identity: "synoptikon-test-fixture".to_owned(),
                    },
                },
                backend_root: backend_root.clone(),
                ssd_staging_root: None,
            },
        )
        .map_err(|error| error.to_string())?;
        FileBackedCapacityAdmissionProvider::new(
            &store_registry_path,
            root.join("capacity-ledgers"),
            &hdd_root,
            &backend_root,
            StatvfsCapacitySpaceProbe,
        )
        .with_subobject_registry_path(&subobject_registry_path)
        .with_profile_binding_registry_path(&profile_binding_registry_path)
        .initialize_store(&store_id, policy.capacity.clone())
        .map_err(|error| error.to_string())?;
        Ok(Self {
            root,
            store_registry_path,
            subobject_registry_path,
            profile_binding_registry_path,
            live_sqlite_path: report.live_sqlite_path,
            ssd_root,
            backend_root,
            hdd_root,
            projection_ledger_path,
            now_utc,
        })
    }

    /// Reopen the exact disposable fixture after a daemon-process restart.
    /// This is feature-gated test support and cannot select production paths.
    pub fn open_existing(
        root: impl AsRef<Path>,
        now_utc: impl Into<String>,
    ) -> Result<Self, String> {
        let root = root.as_ref().to_path_buf();
        let marker_path = root.join(".synoptikon-test-fixture");
        let marker = fs::read_to_string(&marker_path)
            .map_err(|_| "existing fixture marker is absent".to_owned())?;
        if marker != SYNOPTIKON_TEST_FIXTURE_MARKER {
            return Err("existing fixture marker is invalid".to_owned());
        }
        let fixture = Self {
            store_registry_path: root.join("stores.json"),
            subobject_registry_path: root.join("subobjects.json"),
            profile_binding_registry_path: root.join("profile-bindings.json"),
            live_sqlite_path: root.join("ssd/.dasobjectstore/live.sqlite"),
            ssd_root: root.join("ssd"),
            backend_root: root.join("ssd/synoptikon-backend"),
            hdd_root: root.join("hdd"),
            projection_ledger_path: root.join("projection-authority/ledger.json"),
            root,
            now_utc: now_utc.into(),
        };
        for required in [
            &fixture.store_registry_path,
            &fixture.profile_binding_registry_path,
            &fixture.live_sqlite_path,
            &fixture.backend_root,
            &fixture.hdd_root,
            &fixture.projection_ledger_path,
        ] {
            if !required.exists() {
                return Err(format!(
                    "existing fixture is missing {}",
                    required.display()
                ));
            }
        }
        Ok(fixture)
    }

    pub fn handler(
        &self,
    ) -> DaemonRequestHandler<SynoptikonProjectionTestService, FixedDaemonClock> {
        let capacity = Arc::new(
            FileBackedCapacityAdmissionProvider::new(
                &self.store_registry_path,
                self.root.join("capacity-ledgers"),
                &self.hdd_root,
                &self.backend_root,
                StatvfsCapacitySpaceProbe,
            )
            .with_subobject_registry_path(&self.subobject_registry_path)
            .with_profile_binding_registry_path(&self.profile_binding_registry_path),
        );
        DaemonRequestHandler::new(
            SynoptikonProjectionTestService { capacity },
            FixedDaemonClock::new(self.now_utc.clone()),
        )
        .with_synoptikon_test_paths(
            &self.store_registry_path,
            &self.subobject_registry_path,
            &self.profile_binding_registry_path,
            &self.live_sqlite_path,
            &self.hdd_root,
            &self.projection_ledger_path,
        )
    }

    pub fn settle_one_hdd_placement(&self) -> Result<DurableDestageOutcome, String> {
        run_one_durable_destage(
            &DurableDestageWorkerConfig {
                live_sqlite_path: self.live_sqlite_path.clone(),
                ssd_root: self.backend_root.clone(),
                hdd_root: self.hdd_root.clone(),
                worker_id: "synoptikon-test-destage".to_owned(),
            },
            &self.now_utc,
            None,
        )
        .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{
        DaemonApiRequest, DaemonApiResponse, SynoptikonProjectionPrepareRequest,
        SYNOPTIKON_PROJECTION_PREPARE_V1_SCHEMA,
    };

    #[test]
    fn fixture_seeds_real_handler_paths_and_fixed_peer_prepare() {
        let root = std::env::temp_dir().join(format!(
            "das-synoptikon-test-support-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let fixture =
            SynoptikonProjectionTestFixture::new(&root, "2026-08-18T10:00:00Z").expect("fixture");
        assert!(fixture.live_sqlite_path.is_file());
        assert!(fixture.store_registry_path.is_file());
        assert!(fixture.profile_binding_registry_path.is_file());
        SynoptikonProjectionTestFixture::open_existing(&root, "2026-08-18T10:00:01Z")
            .expect("marked fixture reopens");
        let response = fixture
            .handler()
            .handle_with_progress_for_actor(
                DaemonApiRequest::PrepareSynoptikonProjection(SynoptikonProjectionPrepareRequest {
                    schema_version: SYNOPTIKON_PROJECTION_PREPARE_V1_SCHEMA.to_owned(),
                    logical_name: "hello.txt".to_owned(),
                    size_bytes: 5,
                    sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
                        .to_owned(),
                }),
                Some(&crate::auth::DaemonLocalActor::new(4242).with_username("dasobjectstore")),
                |_| Ok(()),
            )
            .expect("prepare handled");
        assert!(
            matches!(
                &response,
                DaemonApiResponse::SynoptikonProjectionPrepared(_)
            ),
            "unexpected prepare response: {response:?}"
        );
        std::fs::remove_dir_all(root).expect("cleanup fixture");
    }

    #[test]
    fn fixture_reopen_rejects_an_unmarked_existing_root() {
        let root = std::env::temp_dir().join(format!(
            "das-synoptikon-unmarked-test-support-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("unmarked root");
        assert!(
            SynoptikonProjectionTestFixture::open_existing(&root, "2026-08-18T10:00:01Z").is_err()
        );
        std::fs::remove_dir_all(root).expect("cleanup unmarked root");
    }
}
