//! Daemon-backed local-group and enclosure administration clients.

use super::*;

pub(super) trait LocalPasswordAuthenticator: Send + Sync {
    fn authenticate(&self, username: &str, password: &str) -> Result<(), LocalPasswordAuthError>;
}

#[derive(Default)]
pub(super) struct SystemLocalPasswordAuthenticator {
    pam: PamLocalPasswordAuthenticator,
}

impl LocalPasswordAuthenticator for SystemLocalPasswordAuthenticator {
    fn authenticate(&self, username: &str, password: &str) -> Result<(), LocalPasswordAuthError> {
        self.pam.authenticate(username, password)
    }
}

pub(super) trait LocalUserAuthorityProvider: Send + Sync {
    fn local_user(
        &self,
        username: &str,
    ) -> Result<crate::LocalUserMetadata, crate::LocalUserDiscoveryError>;
}

pub(super) struct SystemLocalUserAuthorityProvider;

impl LocalUserAuthorityProvider for SystemLocalUserAuthorityProvider {
    fn local_user(
        &self,
        username: &str,
    ) -> Result<crate::LocalUserMetadata, crate::LocalUserDiscoveryError> {
        discover_local_user(username)
    }
}

pub(super) trait StandaloneLocalGroupAdminClient: Send + Sync {
    fn submit_local_group_operation(
        &self,
        request: StandaloneLocalGroupAdminDaemonRequest,
    ) -> Result<StandaloneLocalGroupAdminResponse, StandaloneLocalGroupAdminClientError>;
}

pub(super) trait StandaloneEnclosureAdminClient: Send + Sync {
    fn submit_prepare_enclosure(
        &self,
        request: StandaloneEnclosurePrepareDaemonRequest,
    ) -> Result<StandaloneEnclosurePrepareResponse, StandaloneEnclosureAdminClientError>;

    fn submit_create_object_store(
        &self,
        request: DaemonCreateObjectStoreRequest,
    ) -> Result<StandaloneCreateObjectStoreResponse, StandaloneEnclosureAdminClientError>;

    fn submit_update_object_store_ingest_policy(
        &self,
        request: DaemonUpdateObjectStoreIngestPolicyRequest,
    ) -> Result<StandaloneObjectStoreIngestPolicyResponse, StandaloneEnclosureAdminClientError>;

    fn submit_endpoint_inventory_upsert(
        &self,
        request: DaemonUpsertEndpointInventoryRequest,
    ) -> Result<StandaloneEndpointInventoryUpsertResponse, StandaloneEnclosureAdminClientError>;

    fn job_status(
        &self,
        request: StandaloneAdminJobStatusDaemonRequest,
    ) -> Result<StandaloneAdminJobStatusResponse, StandaloneEnclosureAdminClientError>;

    fn cancel_job(
        &self,
        request: StandaloneAdminJobCancelDaemonRequest,
    ) -> Result<StandaloneAdminJobCancelResponse, StandaloneEnclosureAdminClientError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct StandaloneLocalGroupAdminDaemonRequest {
    pub(super) operation: StandaloneLocalGroupOperation,
    pub(super) group_name: String,
    pub(super) username: Option<String>,
    pub(super) dry_run: bool,
    pub(super) client_request_id: Option<String>,
    pub(super) administrator_actor: Option<String>,
    pub(super) confirmation_marker: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct StandaloneEnclosurePrepareDaemonRequest {
    pub(super) ssd_device: String,
    pub(super) hdd_devices: Vec<PrepareEnclosureHddDeviceRequest>,
    pub(super) mount_root: String,
    pub(super) filesystem: DaemonPrepareEnclosureFilesystem,
    pub(super) owner: Option<String>,
    pub(super) dry_run: bool,
    pub(super) client_request_id: Option<String>,
    pub(super) administrator_actor: Option<String>,
    pub(super) allow_format: bool,
    pub(super) existing_data_acknowledged: bool,
    pub(super) confirmation_marker: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct StandaloneAdminJobStatusDaemonRequest {
    pub(super) job_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct StandaloneAdminJobCancelDaemonRequest {
    pub(super) job_id: String,
    pub(super) reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct StandaloneLocalGroupAdminClientError {
    pub(super) message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct StandaloneEnclosureAdminClientError {
    pub(super) message: String,
}

pub(super) struct DaemonStandaloneLocalGroupAdminClient {
    client: DaemonClient<UnixSocketDaemonTransport>,
}

impl DaemonStandaloneLocalGroupAdminClient {
    pub(super) fn default_packaged() -> Self {
        Self {
            client: DaemonClient::new(UnixSocketDaemonTransport::new(
                DaemonRuntimeConfig::default_packaged().socket_path,
            )),
        }
    }
}

impl StandaloneLocalGroupAdminClient for DaemonStandaloneLocalGroupAdminClient {
    fn submit_local_group_operation(
        &self,
        request: StandaloneLocalGroupAdminDaemonRequest,
    ) -> Result<StandaloneLocalGroupAdminResponse, StandaloneLocalGroupAdminClientError> {
        match request.operation {
            StandaloneLocalGroupOperation::CreateGroup => self
                .client
                .create_local_group(DaemonCreateLocalGroupRequest {
                    group_name: request.group_name,
                    dry_run: request.dry_run,
                    client_request_id: request.client_request_id,
                    administrator_actor: request.administrator_actor,
                    confirmation_marker: request.confirmation_marker,
                })
                .map(create_local_group_response_from_daemon)
                .map_err(standalone_admin_client_error),
            StandaloneLocalGroupOperation::AddUserToGroup => self
                .client
                .assign_local_user_to_local_group(DaemonAssignLocalUserToLocalGroupRequest {
                    username: request.username.ok_or_else(|| {
                        StandaloneLocalGroupAdminClientError {
                            message: "username is required".to_string(),
                        }
                    })?,
                    group_name: request.group_name,
                    dry_run: request.dry_run,
                    client_request_id: request.client_request_id,
                    administrator_actor: request.administrator_actor,
                    confirmation_marker: request.confirmation_marker,
                })
                .map(assign_local_user_to_group_response_from_daemon)
                .map_err(standalone_admin_client_error),
        }
    }
}

pub(super) struct DaemonStandaloneEnclosureAdminClient {
    client: DaemonClient<UnixSocketDaemonTransport>,
}

impl DaemonStandaloneEnclosureAdminClient {
    pub(super) fn default_packaged() -> Self {
        Self {
            client: DaemonClient::new(UnixSocketDaemonTransport::new(
                DaemonRuntimeConfig::default_packaged().socket_path,
            )),
        }
    }
}

impl StandaloneEnclosureAdminClient for DaemonStandaloneEnclosureAdminClient {
    fn submit_prepare_enclosure(
        &self,
        request: StandaloneEnclosurePrepareDaemonRequest,
    ) -> Result<StandaloneEnclosurePrepareResponse, StandaloneEnclosureAdminClientError> {
        self.client
            .prepare_enclosure(DaemonPrepareEnclosureRequest {
                ssd_device: request.ssd_device.into(),
                hdd_devices: request
                    .hdd_devices
                    .into_iter()
                    .map(|device| DaemonPrepareEnclosureHddDevice {
                        disk_id: device.disk_id,
                        device_path: device.device_path.into(),
                    })
                    .collect(),
                mount_root: request.mount_root.into(),
                filesystem: request.filesystem,
                owner: request.owner,
                dry_run: request.dry_run,
                client_request_id: request.client_request_id,
                administrator_actor: request.administrator_actor,
                allow_format: request.allow_format,
                existing_data_acknowledged: request.existing_data_acknowledged,
                confirmation_marker: request.confirmation_marker,
            })
            .map(enclosure_prepare_response_from_daemon)
            .map_err(standalone_enclosure_admin_client_error)
    }

    fn submit_create_object_store(
        &self,
        request: DaemonCreateObjectStoreRequest,
    ) -> Result<StandaloneCreateObjectStoreResponse, StandaloneEnclosureAdminClientError> {
        self.client
            .create_object_store(request)
            .map(create_object_store_response_from_daemon)
            .map_err(standalone_enclosure_admin_client_error)
    }

    fn submit_update_object_store_ingest_policy(
        &self,
        request: DaemonUpdateObjectStoreIngestPolicyRequest,
    ) -> Result<StandaloneObjectStoreIngestPolicyResponse, StandaloneEnclosureAdminClientError>
    {
        self.client
            .update_object_store_ingest_policy(request)
            .map(ingest_policy_response_from_daemon)
            .map_err(standalone_enclosure_admin_client_error)
    }

    fn submit_endpoint_inventory_upsert(
        &self,
        request: DaemonUpsertEndpointInventoryRequest,
    ) -> Result<StandaloneEndpointInventoryUpsertResponse, StandaloneEnclosureAdminClientError>
    {
        self.client
            .upsert_endpoint_inventory(request)
            .map(endpoint_inventory_upsert_response_from_daemon)
            .map_err(standalone_enclosure_admin_client_error)
    }

    fn job_status(
        &self,
        request: StandaloneAdminJobStatusDaemonRequest,
    ) -> Result<StandaloneAdminJobStatusResponse, StandaloneEnclosureAdminClientError> {
        let job_id = DaemonJobId::new(request.job_id).map_err(|err| {
            StandaloneEnclosureAdminClientError {
                message: err.to_string(),
            }
        })?;
        self.client
            .job_status(DaemonJobStatusRequest { job_id })
            .map(admin_job_status_response_from_daemon)
            .map_err(standalone_enclosure_admin_client_error)
    }

    fn cancel_job(
        &self,
        request: StandaloneAdminJobCancelDaemonRequest,
    ) -> Result<StandaloneAdminJobCancelResponse, StandaloneEnclosureAdminClientError> {
        let job_id = DaemonJobId::new(request.job_id).map_err(|err| {
            StandaloneEnclosureAdminClientError {
                message: err.to_string(),
            }
        })?;
        self.client
            .cancel_job(DaemonJobCancelRequest {
                job_id,
                reason: request.reason,
            })
            .map(admin_job_cancel_response_from_daemon)
            .map_err(standalone_enclosure_admin_client_error)
    }
}
