use super::*;

impl<S, C> DaemonRequestHandler<S, C>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    pub fn with_application_upload_paths(
        mut self,
        capability_path: impl Into<PathBuf>,
        replay_path: impl Into<PathBuf>,
    ) -> Self {
        self.application_upload_capability_path = capability_path.into();
        self.application_capability_replay_path = replay_path.into();
        self
    }
    pub fn with_live_sqlite_path(mut self, live_sqlite_path: impl Into<PathBuf>) -> Self {
        self.live_sqlite_path = live_sqlite_path.into();
        self
    }

    pub fn with_live_status_registry(
        mut self,
        registry: Arc<crate::runtime::LiveStatusRegistry>,
    ) -> Self {
        self.live_status_registry = registry;
        self
    }

    pub fn with_hdd_root_path(mut self, hdd_root_path: impl Into<PathBuf>) -> Self {
        self.hdd_root_path = hdd_root_path.into();
        self
    }

    pub fn with_profile_binding_registry_path(
        mut self,
        profile_binding_registry_path: impl Into<PathBuf>,
    ) -> Self {
        self.profile_binding_registry_path = profile_binding_registry_path.into();
        self
    }

    pub fn with_profile_migration_state_root(mut self, path: impl Into<PathBuf>) -> Self {
        self.profile_migration_state_root = path.into();
        self
    }

    pub fn with_application_identity_registry_path(
        mut self,
        application_identity_registry_path: impl Into<PathBuf>,
    ) -> Self {
        self.application_identity_registry_path = application_identity_registry_path.into();
        self
    }

    pub fn with_application_key_registry_path(
        mut self,
        application_key_registry_path: impl Into<PathBuf>,
    ) -> Self {
        self.application_key_registry_path = application_key_registry_path.into();
        self
    }

    pub fn with_application_audit_log_path(
        mut self,
        application_audit_log_path: impl Into<PathBuf>,
    ) -> Self {
        self.application_audit_log_path = application_audit_log_path.into();
        self
    }

    pub fn with_appliance_telemetry_state_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.appliance_telemetry_state_path = path.into();
        self
    }

    pub fn with_remote_easyconnect_session_store_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.remote_easyconnect_session_store_path = path.into();
        self
    }

    pub fn with_remote_easyconnect_pairing_store_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.remote_easyconnect_pairing_store_path = path.into();
        self
    }

    pub fn with_credential_registry_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.credential_registry_path = path.into();
        self
    }

    pub fn with_remote_upload_admission_gate(
        mut self,
        remote_upload_admission_gate: Arc<RemoteUploadAdmissionGate>,
    ) -> Self {
        self.remote_upload_admission_gate = remote_upload_admission_gate;
        self
    }
}
