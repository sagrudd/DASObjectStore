use super::health::DaemonSsdPressure;
use super::ingest::{DaemonIngressLandingMode, DaemonIngressOrigin};
use super::jobs::DaemonJobEvent;
use crate::auth::{
    authorize_store_read, authorize_store_write, DaemonLocalActor, DaemonStoreAccessPolicy,
};
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::remote_upload::{
    RemoteUploadBackpressureAction, RemoteUploadBackpressurePolicy,
};
use serde::{Deserialize, Serialize};

pub const REMOTE_EASYCONNECT_DISCOVERY_ROUTE: &str = "/api/v1/remote/easyconnect/discovery";
pub const REMOTE_EASYCONNECT_PAIRINGS_ROUTE: &str = "/api/v1/remote/easyconnect/pairings";
pub const REMOTE_EASYCONNECT_PAIRING_APPROVAL_ROUTE_TEMPLATE: &str =
    "/api/v1/remote/easyconnect/pairings/{pairing_id}/approve";
pub const REMOTE_EASYCONNECT_PAIRING_EXCHANGE_ROUTE: &str =
    "/api/v1/remote/easyconnect/pairings/exchange";
pub const REMOTE_EASYCONNECT_SESSIONS_ROUTE: &str = "/api/v1/remote/easyconnect/sessions";
pub const REMOTE_EASYCONNECT_SESSION_ROUTE_TEMPLATE: &str =
    "/api/v1/remote/easyconnect/sessions/{session_id}";
pub const REMOTE_EASYCONNECT_SESSION_RENEW_ROUTE_TEMPLATE: &str =
    "/api/v1/remote/easyconnect/sessions/{session_id}/renew";
pub const REMOTE_EASYCONNECT_LOCAL_AGENT_HANDOFF_ROUTE: &str =
    "/v1/dasobjectstore/remote/uploads/handoffs";
pub const REMOTE_EASYCONNECT_MIN_SESSION_LIFETIME_SECONDS: u64 = 60;
pub const REMOTE_EASYCONNECT_DEFAULT_SESSION_LIFETIME_SECONDS: u64 = 8 * 60 * 60;
pub const REMOTE_EASYCONNECT_MAX_SESSION_LIFETIME_SECONDS: u64 = 24 * 60 * 60;
pub const REMOTE_EASYCONNECT_RENEWAL_LEAD_SECONDS: u64 = 60 * 60;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectDiscoveryRequest;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectDiscoveryResponse {
    pub appliance_id: String,
    pub product_id: String,
    pub display_name: String,
    pub pairing_create_url: String,
    pub pairing_exchange_url: String,
    pub session_revoke_url_template: String,
    pub session_renew_url_template: String,
    pub default_session_lifetime_seconds: u64,
    pub session_policy: RemoteEasyconnectSessionPolicy,
    pub auth_providers: Vec<RemoteEasyconnectAuthProvider>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectSessionPolicy {
    pub default_lifetime_seconds: u64,
    pub min_lifetime_seconds: u64,
    pub max_lifetime_seconds: u64,
    pub renewal_lead_seconds: u64,
    pub renewal_requires_password_reauthentication: bool,
    pub renewal_token_rotates: bool,
}

impl Default for RemoteEasyconnectSessionPolicy {
    fn default() -> Self {
        Self {
            default_lifetime_seconds: REMOTE_EASYCONNECT_DEFAULT_SESSION_LIFETIME_SECONDS,
            min_lifetime_seconds: REMOTE_EASYCONNECT_MIN_SESSION_LIFETIME_SECONDS,
            max_lifetime_seconds: REMOTE_EASYCONNECT_MAX_SESSION_LIFETIME_SECONDS,
            renewal_lead_seconds: REMOTE_EASYCONNECT_RENEWAL_LEAD_SECONDS,
            renewal_requires_password_reauthentication: false,
            renewal_token_rotates: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteEasyconnectAuthProvider {
    StandaloneLocalUser,
    Synoptikon,
    Mneion,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectCreatePairingRequest {
    pub client_name: String,
    pub callback_url: String,
    pub requested_object_store: Option<String>,
    pub requested_session_lifetime_seconds: Option<u64>,
    pub client_request_id: Option<String>,
}

impl RemoteEasyconnectCreatePairingRequest {
    pub fn validate(&self) -> Result<(), RemoteEasyconnectValidationError> {
        require_non_blank("client_name", &self.client_name)?;
        require_http_url("callback_url", &self.callback_url)?;
        validate_optional_non_blank(
            "requested_object_store",
            self.requested_object_store.as_deref(),
        )?;
        validate_optional_non_blank("client_request_id", self.client_request_id.as_deref())?;
        validate_requested_lifetime(self.requested_session_lifetime_seconds)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectCreatePairingResponse {
    pub pairing_id: String,
    pub browser_login_url: String,
    pub callback_url: String,
    pub expires_at_utc: String,
    pub polling_url: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectApprovePairingRequest {
    pub pairing_id: String,
    pub approved_actor: String,
    pub auth_provider: RemoteEasyconnectAuthProvider,
    pub allowed_object_stores: Vec<RemoteEasyconnectObjectStoreGrant>,
    pub approval_expires_at_utc: String,
}

impl RemoteEasyconnectApprovePairingRequest {
    pub fn validate(&self) -> Result<(), RemoteEasyconnectValidationError> {
        require_non_blank("pairing_id", &self.pairing_id)?;
        require_non_blank("approved_actor", &self.approved_actor)?;
        require_non_blank("approval_expires_at_utc", &self.approval_expires_at_utc)?;
        if self.allowed_object_stores.is_empty() {
            return Err(RemoteEasyconnectValidationError::EmptyObjectStoreGrants);
        }
        for grant in &self.allowed_object_stores {
            grant.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectApprovePairingResponse {
    pub pairing_id: String,
    pub exchange_code: String,
    pub callback_url: String,
    pub expires_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectExchangePairingRequest {
    pub pairing_id: String,
    pub exchange_code: String,
    pub client_request_id: Option<String>,
}

impl RemoteEasyconnectExchangePairingRequest {
    pub fn validate(&self) -> Result<(), RemoteEasyconnectValidationError> {
        require_non_blank("pairing_id", &self.pairing_id)?;
        require_non_blank("exchange_code", &self.exchange_code)?;
        validate_optional_non_blank("client_request_id", self.client_request_id.as_deref())?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectExchangePairingResponse {
    pub appliance_id: String,
    pub appliance_base_url: String,
    pub session: RemoteEasyconnectSession,
    pub object_stores: Vec<RemoteEasyconnectObjectStoreGrant>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectRevokeSessionRequest {
    pub session_id: String,
    pub reason: Option<String>,
}

impl RemoteEasyconnectRevokeSessionRequest {
    pub fn validate(&self) -> Result<(), RemoteEasyconnectValidationError> {
        require_non_blank("session_id", &self.session_id)?;
        validate_optional_non_blank("reason", self.reason.as_deref())?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectRevokeSessionResponse {
    pub session_id: String,
    pub revoked: bool,
    pub revoked_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectRenewSessionRequest {
    pub session_id: String,
    pub renewal_token: String,
    pub requested_lifetime_seconds: Option<u64>,
}

impl RemoteEasyconnectRenewSessionRequest {
    pub fn validate(&self) -> Result<(), RemoteEasyconnectValidationError> {
        require_non_blank("session_id", &self.session_id)?;
        require_non_blank("renewal_token", &self.renewal_token)?;
        validate_requested_lifetime(self.requested_lifetime_seconds)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectRenewSessionResponse {
    pub session: RemoteEasyconnectSession,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectSession {
    pub session_id: String,
    pub issued_at_utc: String,
    pub expires_at_utc: String,
    pub credentials: RemoteEasyconnectSessionCredentials,
    pub renewal: RemoteEasyconnectSessionRenewal,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectSessionCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectSessionRenewal {
    pub renew_url: String,
    pub renew_after_utc: String,
    pub renewal_token: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectObjectStoreGrant {
    pub object_store: String,
    pub bucket: String,
    pub can_read: bool,
    pub can_write: bool,
    pub writer_group: Option<String>,
    pub object_type: String,
}

impl RemoteEasyconnectObjectStoreGrant {
    pub fn validate(&self) -> Result<(), RemoteEasyconnectValidationError> {
        require_non_blank("object_store", &self.object_store)?;
        require_non_blank("bucket", &self.bucket)?;
        validate_optional_non_blank("writer_group", self.writer_group.as_deref())?;
        require_non_blank("object_type", &self.object_type)?;
        if !self.can_read && !self.can_write {
            return Err(RemoteEasyconnectValidationError::GrantWithoutAccess {
                object_store: self.object_store.clone(),
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectObjectStoreAccessPolicy {
    pub object_store: String,
    pub bucket: String,
    pub reader_group: Option<String>,
    pub writer_group: Option<String>,
    pub admin_group: Option<String>,
    pub public_read: bool,
    pub writable: bool,
    pub object_type: String,
}

impl RemoteEasyconnectObjectStoreAccessPolicy {
    pub fn validate(&self) -> Result<(), RemoteEasyconnectValidationError> {
        require_non_blank("object_store", &self.object_store)?;
        require_non_blank("bucket", &self.bucket)?;
        validate_optional_non_blank("reader_group", self.reader_group.as_deref())?;
        validate_optional_non_blank("writer_group", self.writer_group.as_deref())?;
        validate_optional_non_blank("admin_group", self.admin_group.as_deref())?;
        require_non_blank("object_type", &self.object_type)
    }

    fn daemon_policy(&self) -> DaemonStoreAccessPolicy {
        let mut policy = DaemonStoreAccessPolicy::new(
            StoreId::new(self.object_store.clone())
                .expect("object_store was checked non-blank before building daemon access policy"),
        )
        .with_public_read(self.public_read);
        if let Some(reader_group) = &self.reader_group {
            policy = policy.with_reader_group(reader_group.clone());
        }
        if let Some(writer_group) = &self.writer_group {
            policy = policy.with_writer_group(writer_group.clone());
        }
        if let Some(admin_group) = &self.admin_group {
            policy = policy.with_admin_group(admin_group.clone());
        }
        policy
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteEasyconnectUploadHandoffMode {
    LoopbackPost,
    BrowserMediated,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteEasyconnectUploadHandoffState {
    ConfirmationRequired,
    ReadyForAgent,
    AgentUnreachable,
    Cancelled,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectUploadSelectionEntry {
    pub display_path: String,
    pub size_bytes: u64,
}

impl RemoteEasyconnectUploadSelectionEntry {
    pub fn validate(&self) -> Result<(), RemoteEasyconnectValidationError> {
        require_non_blank("selected_files.display_path", &self.display_path)?;
        if display_path_is_absolute(&self.display_path) {
            return Err(
                RemoteEasyconnectValidationError::AbsoluteUploadSelectionPath {
                    display_path: self.display_path.clone(),
                },
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectUploadHandoffRequest {
    pub session_id: String,
    pub object_store: String,
    pub bucket: String,
    pub local_agent_base_url: String,
    pub selected_files: Vec<RemoteEasyconnectUploadSelectionEntry>,
    pub total_bytes: u64,
    pub browser_origin: String,
    pub client_request_id: Option<String>,
}

impl RemoteEasyconnectUploadHandoffRequest {
    pub fn validate(&self) -> Result<(), RemoteEasyconnectValidationError> {
        require_non_blank("session_id", &self.session_id)?;
        require_non_blank("object_store", &self.object_store)?;
        require_non_blank("bucket", &self.bucket)?;
        require_loopback_http_url("local_agent_base_url", &self.local_agent_base_url)?;
        require_http_url("browser_origin", &self.browser_origin)?;
        validate_optional_non_blank("client_request_id", self.client_request_id.as_deref())?;
        if self.selected_files.is_empty() {
            return Err(RemoteEasyconnectValidationError::EmptyUploadSelection);
        }
        let mut selected_bytes = 0_u64;
        for file in &self.selected_files {
            file.validate()?;
            selected_bytes = selected_bytes.saturating_add(file.size_bytes);
        }
        if selected_bytes != self.total_bytes {
            return Err(
                RemoteEasyconnectValidationError::UploadSelectionByteMismatch {
                    expected: selected_bytes,
                    actual: self.total_bytes,
                },
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectUploadHandoffResponse {
    pub handoff_id: String,
    pub mode: RemoteEasyconnectUploadHandoffMode,
    pub state: RemoteEasyconnectUploadHandoffState,
    pub ingress_origin: DaemonIngressOrigin,
    pub landing_mode: DaemonIngressLandingMode,
    pub backpressure_policy: RemoteUploadBackpressurePolicy,
    pub local_agent_handoff_url: String,
    pub confirmation_phrase: String,
    pub path_privacy: String,
    pub message: String,
    pub failure_states: Vec<RemoteEasyconnectUploadHandoffFailure>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectUploadHandoffFailure {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectUploadAdmissionRequest {
    #[serde(default)]
    pub policy: RemoteUploadBackpressurePolicy,
    pub ssd_pressure: DaemonSsdPressure,
    pub active_s3_transfers: u16,
    pub ssd_stage_queue_depth: u32,
    pub hdd_landing_queue_depth: u32,
    pub verification_queue_depth: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectUploadAdmissionDecision {
    pub action: RemoteUploadBackpressureAction,
    pub reason: RemoteEasyconnectUploadBackpressureReason,
    pub retry_after_seconds: Option<u64>,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectSubmitAwsCliUploadRequest {
    pub job_id: String,
    pub object_store: String,
    pub source_bytes: u64,
    #[serde(default)]
    pub policy: RemoteUploadBackpressurePolicy,
    pub ssd_pressure: DaemonSsdPressure,
    pub program: String,
    pub args: Vec<String>,
    pub display_args: Vec<String>,
    #[serde(default)]
    pub environment: Vec<RemoteEasyconnectAwsCliEnvironmentVariable>,
    #[serde(default)]
    pub progress_telemetry: Option<RemoteEasyconnectUploadProgressTelemetry>,
    pub progress_message: Option<String>,
    /// Optional daemon-owned completion contract. When present, transfer
    /// success is provisional until the daemon independently verifies the S3
    /// object and atomically publishes its provider placement.
    #[serde(default)]
    pub completion: Option<RemoteEasyconnectUploadCompletion>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteEasyconnectUploadCompletion {
    pub upload_id: String,
    pub provider: String,
    pub bucket: String,
    pub object_id: String,
    pub object_version: u64,
    pub object_key: String,
    pub expected_checksum: String,
    pub endpoint_url: String,
}

impl RemoteEasyconnectSubmitAwsCliUploadRequest {
    pub fn validate(&self) -> Result<(), RemoteEasyconnectValidationError> {
        require_non_blank("job_id", &self.job_id)?;
        require_non_blank("object_store", &self.object_store)?;
        require_non_blank("program", &self.program)?;
        if self.args.is_empty() {
            return Err(RemoteEasyconnectValidationError::EmptyAwsCliArgs);
        }
        for variable in &self.environment {
            variable.validate()?;
        }
        validate_optional_non_blank("progress_message", self.progress_message.as_deref())?;
        if let Some(completion) = &self.completion {
            completion.validate()?;
            if completion.provider != "garage" {
                return Err(
                    RemoteEasyconnectValidationError::UnsupportedCompletionProvider {
                        provider: completion.provider.clone(),
                    },
                );
            }
            if completion.object_version == 0 {
                return Err(RemoteEasyconnectValidationError::ZeroObjectVersion);
            }
            if completion.expected_checksum.len() != 71
                || !completion.expected_checksum.starts_with("sha256:")
                || !completion.expected_checksum[7..]
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(RemoteEasyconnectValidationError::InvalidCompletionChecksum);
            }
        }
        Ok(())
    }
}

impl RemoteEasyconnectUploadCompletion {
    fn validate(&self) -> Result<(), RemoteEasyconnectValidationError> {
        require_non_blank("completion.upload_id", &self.upload_id)?;
        require_non_blank("completion.provider", &self.provider)?;
        require_non_blank("completion.bucket", &self.bucket)?;
        require_non_blank("completion.object_id", &self.object_id)?;
        require_non_blank("completion.object_key", &self.object_key)?;
        require_non_blank("completion.endpoint_url", &self.endpoint_url)?;
        if self.object_key.starts_with('/')
            || self.object_key.contains('\\')
            || self
                .object_key
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(RemoteEasyconnectValidationError::InvalidCompletionObjectKey);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectUploadProgressTelemetry {
    pub source_scan_count: Option<u64>,
    pub staged_bytes: Option<u64>,
    pub s3_bytes_per_second: Option<u64>,
    pub ssd_queue_depth: Option<u32>,
    pub hdd_landing_queue_depth: Option<u32>,
    pub active_hdd_writers: Option<u16>,
    pub verification_state: Option<String>,
    pub session_renewal_status: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectAwsCliEnvironmentVariable {
    pub name: String,
    pub value: String,
}

impl RemoteEasyconnectAwsCliEnvironmentVariable {
    pub fn validate(&self) -> Result<(), RemoteEasyconnectValidationError> {
        if self.name.trim().is_empty() || self.name.contains('=') {
            return Err(
                RemoteEasyconnectValidationError::InvalidAwsCliEnvironmentVariable {
                    name: self.name.clone(),
                },
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectSubmitAwsCliUploadResponse {
    pub running_event: Option<DaemonJobEvent>,
    pub progress_events: Vec<DaemonJobEvent>,
    pub final_event: DaemonJobEvent,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteEasyconnectUploadBackpressureReason {
    AllClear,
    SsdHighPressure,
    SsdCriticalPressure,
    S3TransferConcurrencyFull,
    SsdStageQueueFull,
    HddLandingQueueFull,
    VerificationQueueFull,
}

pub fn decide_remote_easyconnect_upload_admission(
    request: RemoteEasyconnectUploadAdmissionRequest,
) -> RemoteEasyconnectUploadAdmissionDecision {
    match request.ssd_pressure {
        DaemonSsdPressure::Critical => {
            return admission_decision(
                request.policy.ssd_critical_pressure_action,
                RemoteEasyconnectUploadBackpressureReason::SsdCriticalPressure,
                "SSD pressure is critical; remote upload intake must wait for daemon drain.",
            );
        }
        DaemonSsdPressure::High => {
            return admission_decision(
                request.policy.ssd_high_pressure_action,
                RemoteEasyconnectUploadBackpressureReason::SsdHighPressure,
                "SSD pressure is high; pause new remote uploads while existing work drains.",
            );
        }
        DaemonSsdPressure::AcceptingWrites => {}
    }

    if request.active_s3_transfers >= request.policy.max_s3_transfer_concurrency {
        return admission_decision(
            RemoteUploadBackpressureAction::PauseNewTransfers,
            RemoteEasyconnectUploadBackpressureReason::S3TransferConcurrencyFull,
            "Remote S3 transfer concurrency is full; wait before starting another upload.",
        );
    }
    if request.ssd_stage_queue_depth >= request.policy.max_ssd_stage_queue_depth {
        return admission_decision(
            RemoteUploadBackpressureAction::PauseNewTransfers,
            RemoteEasyconnectUploadBackpressureReason::SsdStageQueueFull,
            "SSD staging queue is full; wait for staged objects to drain.",
        );
    }
    if request.hdd_landing_queue_depth >= request.policy.max_hdd_landing_queue_depth {
        return admission_decision(
            RemoteUploadBackpressureAction::PauseNewTransfers,
            RemoteEasyconnectUploadBackpressureReason::HddLandingQueueFull,
            "HDD landing queue is full; wait for daemon-selected HDD writers to catch up.",
        );
    }
    if request.verification_queue_depth >= request.policy.max_verification_queue_depth {
        return admission_decision(
            RemoteUploadBackpressureAction::PauseNewTransfers,
            RemoteEasyconnectUploadBackpressureReason::VerificationQueueFull,
            "Verification queue is full; wait for completed writes to verify.",
        );
    }

    admission_decision(
        RemoteUploadBackpressureAction::Accept,
        RemoteEasyconnectUploadBackpressureReason::AllClear,
        "Remote upload intake is available.",
    )
}

fn admission_decision(
    action: RemoteUploadBackpressureAction,
    reason: RemoteEasyconnectUploadBackpressureReason,
    message: impl Into<String>,
) -> RemoteEasyconnectUploadAdmissionDecision {
    RemoteEasyconnectUploadAdmissionDecision {
        action,
        reason,
        retry_after_seconds: match action {
            RemoteUploadBackpressureAction::Accept => None,
            RemoteUploadBackpressureAction::PauseNewTransfers
            | RemoteUploadBackpressureAction::RejectNewTransfers => Some(30),
        },
        message: message.into(),
    }
}

pub fn plan_remote_easyconnect_upload_handoff(
    request: RemoteEasyconnectUploadHandoffRequest,
) -> Result<RemoteEasyconnectUploadHandoffResponse, RemoteEasyconnectValidationError> {
    request.validate()?;
    let handoff_id = request
        .client_request_id
        .clone()
        .unwrap_or_else(|| format!("handoff-{}", request.session_id));
    let base_url = request.local_agent_base_url.trim_end_matches('/');
    Ok(RemoteEasyconnectUploadHandoffResponse {
        handoff_id,
        mode: RemoteEasyconnectUploadHandoffMode::LoopbackPost,
        state: RemoteEasyconnectUploadHandoffState::ConfirmationRequired,
        ingress_origin: DaemonIngressOrigin::RemoteS3,
        landing_mode: DaemonIngressOrigin::RemoteS3.landing_mode(),
        backpressure_policy: RemoteUploadBackpressurePolicy::default(),
        local_agent_handoff_url: format!("{base_url}{REMOTE_EASYCONNECT_LOCAL_AGENT_HANDOFF_ROUTE}"),
        confirmation_phrase: format!("confirm upload to {}", request.object_store),
        path_privacy: "browser sends relative display paths and byte counts only; absolute local paths stay with the paired dasobjectstore-remote agent".to_string(),
        message: "Browser selection metadata is ready for explicit user confirmation before the local agent performs byte transfer.".to_string(),
        failure_states: remote_upload_handoff_failure_states(),
    })
}

fn remote_upload_handoff_failure_states() -> Vec<RemoteEasyconnectUploadHandoffFailure> {
    vec![
        RemoteEasyconnectUploadHandoffFailure {
            code: "agent_unreachable".to_string(),
            message: "The browser could not reach the paired dasobjectstore-remote loopback agent."
                .to_string(),
            retryable: true,
        },
        RemoteEasyconnectUploadHandoffFailure {
            code: "confirmation_cancelled".to_string(),
            message:
                "The user cancelled the upload before the local agent received transfer authority."
                    .to_string(),
            retryable: true,
        },
        RemoteEasyconnectUploadHandoffFailure {
            code: "path_privacy_violation".to_string(),
            message:
                "The handoff attempted to send absolute source paths through the browser contract."
                    .to_string(),
            retryable: false,
        },
    ]
}

pub fn remote_easyconnect_object_store_grants_for_actor(
    actor: &DaemonLocalActor,
    stores: &[RemoteEasyconnectObjectStoreAccessPolicy],
) -> Result<Vec<RemoteEasyconnectObjectStoreGrant>, RemoteEasyconnectValidationError> {
    let mut grants = Vec::new();

    for store in stores {
        store.validate()?;
        let policy = store.daemon_policy();
        let can_read = authorize_store_read(actor, &policy).is_ok();
        let can_write = store.writable && authorize_store_write(actor, &policy).is_ok();
        if can_read || can_write {
            grants.push(RemoteEasyconnectObjectStoreGrant {
                object_store: store.object_store.clone(),
                bucket: store.bucket.clone(),
                can_read,
                can_write,
                writer_group: store.writer_group.clone(),
                object_type: store.object_type.clone(),
            });
        }
    }

    Ok(grants)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RemoteEasyconnectValidationError {
    BlankField { field: &'static str },
    InvalidUrl { field: &'static str, value: String },
    InvalidLoopbackUrl { field: &'static str, value: String },
    InvalidRequestedLifetime { seconds: u64 },
    EmptyObjectStoreGrants,
    GrantWithoutAccess { object_store: String },
    EmptyUploadSelection,
    AbsoluteUploadSelectionPath { display_path: String },
    UploadSelectionByteMismatch { expected: u64, actual: u64 },
    EmptyAwsCliArgs,
    InvalidAwsCliEnvironmentVariable { name: String },
    UnsupportedCompletionProvider { provider: String },
    ZeroObjectVersion,
    InvalidCompletionChecksum,
    InvalidCompletionObjectKey,
}

pub fn resolve_remote_easyconnect_session_lifetime_seconds(
    requested_seconds: Option<u64>,
) -> Result<u64, RemoteEasyconnectValidationError> {
    validate_requested_lifetime(requested_seconds)?;
    Ok(requested_seconds.unwrap_or(REMOTE_EASYCONNECT_DEFAULT_SESSION_LIFETIME_SECONDS))
}

pub fn remote_easyconnect_renew_after_offset_seconds(
    lifetime_seconds: u64,
) -> Result<u64, RemoteEasyconnectValidationError> {
    validate_requested_lifetime(Some(lifetime_seconds))?;
    let lead_seconds = REMOTE_EASYCONNECT_RENEWAL_LEAD_SECONDS.min(lifetime_seconds / 2);
    Ok(lifetime_seconds.saturating_sub(lead_seconds).max(1))
}

impl std::fmt::Display for RemoteEasyconnectValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BlankField { field } => write!(formatter, "{field} must not be blank"),
            Self::InvalidUrl { field, value } => {
                write!(
                    formatter,
                    "{field} must be an http or https URL, got {value}"
                )
            }
            Self::InvalidLoopbackUrl { field, value } => write!(
                formatter,
                "{field} must be a loopback http URL for the paired local agent, got {value}"
            ),
            Self::InvalidRequestedLifetime { seconds } => write!(
                formatter,
                "requested session lifetime must be between 60 and 86400 seconds, got {seconds}"
            ),
            Self::EmptyObjectStoreGrants => {
                formatter.write_str("at least one object store grant is required")
            }
            Self::GrantWithoutAccess { object_store } => write!(
                formatter,
                "object store grant for {object_store} must allow read or write access"
            ),
            Self::EmptyUploadSelection => {
                formatter.write_str("remote upload handoff requires at least one selected file")
            }
            Self::AbsoluteUploadSelectionPath { display_path } => write!(
                formatter,
                "remote upload handoff display path must be relative for privacy, got {display_path}"
            ),
            Self::UploadSelectionByteMismatch { expected, actual } => write!(
                formatter,
                "remote upload handoff selected file bytes total {expected}, got declared total {actual}"
            ),
            Self::EmptyAwsCliArgs => {
                formatter.write_str("remote easyconnect AWS CLI upload requires command arguments")
            }
            Self::InvalidAwsCliEnvironmentVariable { name } => write!(
                formatter,
                "remote easyconnect AWS CLI environment variable name is invalid: {name}"
            ),
            Self::UnsupportedCompletionProvider { provider } => write!(
                formatter,
                "remote upload completion provider is unsupported: {provider}"
            ),
            Self::ZeroObjectVersion => {
                formatter.write_str("remote upload completion object version must be non-zero")
            }
            Self::InvalidCompletionChecksum => formatter.write_str(
                "remote upload completion checksum must be a sha256 digest",
            ),
            Self::InvalidCompletionObjectKey => formatter.write_str(
                "remote upload completion object key must be a safe relative key",
            ),
        }
    }
}

impl std::error::Error for RemoteEasyconnectValidationError {}

fn require_non_blank(
    field: &'static str,
    value: &str,
) -> Result<(), RemoteEasyconnectValidationError> {
    if value.trim().is_empty() {
        return Err(RemoteEasyconnectValidationError::BlankField { field });
    }
    Ok(())
}

fn validate_optional_non_blank(
    field: &'static str,
    value: Option<&str>,
) -> Result<(), RemoteEasyconnectValidationError> {
    if value.is_some_and(|value| value.trim().is_empty()) {
        return Err(RemoteEasyconnectValidationError::BlankField { field });
    }
    Ok(())
}

fn require_http_url(
    field: &'static str,
    value: &str,
) -> Result<(), RemoteEasyconnectValidationError> {
    require_non_blank(field, value)?;
    if value.starts_with("http://") || value.starts_with("https://") {
        Ok(())
    } else {
        Err(RemoteEasyconnectValidationError::InvalidUrl {
            field,
            value: value.to_string(),
        })
    }
}

fn require_loopback_http_url(
    field: &'static str,
    value: &str,
) -> Result<(), RemoteEasyconnectValidationError> {
    require_non_blank(field, value)?;
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("http://127.0.0.1:")
        || lower.starts_with("http://localhost:")
        || lower.starts_with("http://[::1]:")
    {
        Ok(())
    } else {
        Err(RemoteEasyconnectValidationError::InvalidLoopbackUrl {
            field,
            value: value.to_string(),
        })
    }
}

fn display_path_is_absolute(value: &str) -> bool {
    value.starts_with('/') || value.starts_with('\\') || value.as_bytes().get(1) == Some(&b':')
}

fn validate_requested_lifetime(
    seconds: Option<u64>,
) -> Result<(), RemoteEasyconnectValidationError> {
    if let Some(seconds) = seconds {
        if !(REMOTE_EASYCONNECT_MIN_SESSION_LIFETIME_SECONDS
            ..=REMOTE_EASYCONNECT_MAX_SESSION_LIFETIME_SECONDS)
            .contains(&seconds)
        {
            return Err(RemoteEasyconnectValidationError::InvalidRequestedLifetime { seconds });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::health::DaemonSsdPressure;
    use super::super::ingest::{DaemonIngressLandingMode, DaemonIngressOrigin};
    use super::{
        decide_remote_easyconnect_upload_admission, plan_remote_easyconnect_upload_handoff,
        remote_easyconnect_object_store_grants_for_actor,
        remote_easyconnect_renew_after_offset_seconds,
        resolve_remote_easyconnect_session_lifetime_seconds, RemoteEasyconnectAuthProvider,
        RemoteEasyconnectCreatePairingRequest, RemoteEasyconnectExchangePairingRequest,
        RemoteEasyconnectObjectStoreAccessPolicy, RemoteEasyconnectObjectStoreGrant,
        RemoteEasyconnectRenewSessionRequest, RemoteEasyconnectRenewSessionResponse,
        RemoteEasyconnectRevokeSessionRequest, RemoteEasyconnectRevokeSessionResponse,
        RemoteEasyconnectSession, RemoteEasyconnectSessionCredentials,
        RemoteEasyconnectSessionPolicy, RemoteEasyconnectSessionRenewal,
        RemoteEasyconnectUploadAdmissionRequest, RemoteEasyconnectUploadBackpressureReason,
        RemoteEasyconnectUploadHandoffMode, RemoteEasyconnectUploadHandoffRequest,
        RemoteEasyconnectUploadHandoffResponse, RemoteEasyconnectUploadHandoffState,
        RemoteEasyconnectUploadSelectionEntry, RemoteEasyconnectValidationError,
        REMOTE_EASYCONNECT_DEFAULT_SESSION_LIFETIME_SECONDS, REMOTE_EASYCONNECT_PAIRINGS_ROUTE,
        REMOTE_EASYCONNECT_PAIRING_EXCHANGE_ROUTE, REMOTE_EASYCONNECT_SESSION_RENEW_ROUTE_TEMPLATE,
        REMOTE_EASYCONNECT_SESSION_ROUTE_TEMPLATE,
    };
    use crate::auth::DaemonLocalActor;
    use dasobjectstore_core::remote_upload::{
        RemoteUploadBackpressureAction, RemoteUploadBackpressurePolicy,
    };

    #[test]
    fn validates_create_pairing_contract() {
        let request = RemoteEasyconnectCreatePairingRequest {
            client_name: "macbook-pro".to_string(),
            callback_url:
                "http://127.0.0.1:49321/products/dasobjectstore/remote/easyconnect/callback"
                    .to_string(),
            requested_object_store: Some("zymo_fecal_2025.05".to_string()),
            requested_session_lifetime_seconds: Some(28_800),
            client_request_id: Some("request-1".to_string()),
        };

        request.validate().expect("request validates");

        let encoded = serde_json::to_value(request).expect("request serializes");
        assert_eq!(encoded["client_name"], "macbook-pro");
        assert_eq!(encoded["requested_session_lifetime_seconds"], 28_800);
        assert_eq!(
            REMOTE_EASYCONNECT_PAIRINGS_ROUTE,
            "/api/v1/remote/easyconnect/pairings"
        );
    }

    #[test]
    fn rejects_invalid_callback_url() {
        let request = RemoteEasyconnectCreatePairingRequest {
            client_name: "macbook-pro".to_string(),
            callback_url: "127.0.0.1:49321/callback".to_string(),
            requested_object_store: None,
            requested_session_lifetime_seconds: None,
            client_request_id: None,
        };

        let err = request.validate().expect_err("invalid URL rejected");

        assert!(matches!(
            err,
            RemoteEasyconnectValidationError::InvalidUrl {
                field: "callback_url",
                ..
            }
        ));
    }

    #[test]
    fn defaults_session_lifetime_to_eight_hours_and_renews_one_hour_before_expiry() {
        let policy = RemoteEasyconnectSessionPolicy::default();

        assert_eq!(
            policy.default_lifetime_seconds,
            REMOTE_EASYCONNECT_DEFAULT_SESSION_LIFETIME_SECONDS
        );
        assert_eq!(
            resolve_remote_easyconnect_session_lifetime_seconds(None).expect("default resolves"),
            28_800
        );
        assert_eq!(
            resolve_remote_easyconnect_session_lifetime_seconds(Some(3_600))
                .expect("requested resolves"),
            3_600
        );
        assert_eq!(
            remote_easyconnect_renew_after_offset_seconds(28_800).expect("renewal offset"),
            25_200
        );
        assert!(!policy.renewal_requires_password_reauthentication);
        assert!(policy.renewal_token_rotates);
    }

    #[test]
    fn short_sessions_become_renewable_halfway_through() {
        assert_eq!(
            remote_easyconnect_renew_after_offset_seconds(60).expect("minimum lifetime valid"),
            30
        );
    }

    #[test]
    fn serializes_auth_provider_names() {
        let encoded = serde_json::to_value(RemoteEasyconnectAuthProvider::StandaloneLocalUser)
            .expect("provider serializes");

        assert_eq!(encoded, "standalone_local_user");
    }

    #[test]
    fn validates_exchange_pairing_contract() {
        let request = RemoteEasyconnectExchangePairingRequest {
            pairing_id: "pair-1".to_string(),
            exchange_code: "code-1".to_string(),
            client_request_id: None,
        };

        request.validate().expect("request validates");
        assert_eq!(
            REMOTE_EASYCONNECT_PAIRING_EXCHANGE_ROUTE,
            "/api/v1/remote/easyconnect/pairings/exchange"
        );
        assert_eq!(
            REMOTE_EASYCONNECT_SESSION_RENEW_ROUTE_TEMPLATE,
            "/api/v1/remote/easyconnect/sessions/{session_id}/renew"
        );
    }

    #[test]
    fn validates_session_revoke_contract() {
        let request = RemoteEasyconnectRevokeSessionRequest {
            session_id: "session-1".to_string(),
            reason: Some("operator requested revocation".to_string()),
        };

        request.validate().expect("request validates");
        let encoded = serde_json::to_value(&request).expect("request serializes");
        assert_eq!(encoded["session_id"], "session-1");
        assert_eq!(encoded["reason"], "operator requested revocation");
        assert_eq!(
            REMOTE_EASYCONNECT_SESSION_ROUTE_TEMPLATE,
            "/api/v1/remote/easyconnect/sessions/{session_id}"
        );

        let blank_reason = RemoteEasyconnectRevokeSessionRequest {
            session_id: "session-1".to_string(),
            reason: Some(" ".to_string()),
        };
        assert!(matches!(
            blank_reason.validate().expect_err("blank reason rejected"),
            RemoteEasyconnectValidationError::BlankField { field: "reason" }
        ));

        let response = RemoteEasyconnectRevokeSessionResponse {
            session_id: "session-1".to_string(),
            revoked: true,
            revoked_at_utc: "2026-07-09T13:20:00Z".to_string(),
        };
        let encoded = serde_json::to_value(response).expect("response serializes");
        assert_eq!(encoded["revoked"], true);
        assert_eq!(encoded["revoked_at_utc"], "2026-07-09T13:20:00Z");
    }

    #[test]
    fn validates_session_renewal_contract_for_active_uploads() {
        let request = RemoteEasyconnectRenewSessionRequest {
            session_id: "session-1".to_string(),
            renewal_token: "old-renewal-token".to_string(),
            requested_lifetime_seconds: Some(28_800),
        };

        request.validate().expect("request validates");

        let blank_token = RemoteEasyconnectRenewSessionRequest {
            session_id: "session-1".to_string(),
            renewal_token: " ".to_string(),
            requested_lifetime_seconds: Some(28_800),
        };
        assert!(matches!(
            blank_token
                .validate()
                .expect_err("blank renewal token rejected"),
            RemoteEasyconnectValidationError::BlankField {
                field: "renewal_token"
            }
        ));

        let too_short = RemoteEasyconnectRenewSessionRequest {
            session_id: "session-1".to_string(),
            renewal_token: "old-renewal-token".to_string(),
            requested_lifetime_seconds: Some(59),
        };
        assert!(matches!(
            too_short.validate().expect_err("short lifetime rejected"),
            RemoteEasyconnectValidationError::InvalidRequestedLifetime { seconds: 59 }
        ));

        let response = RemoteEasyconnectRenewSessionResponse {
            session: RemoteEasyconnectSession {
                session_id: "session-1".to_string(),
                issued_at_utc: "2026-07-09T13:20:00Z".to_string(),
                expires_at_utc: "2026-07-09T21:20:00Z".to_string(),
                credentials: RemoteEasyconnectSessionCredentials {
                    access_key_id: "AKIAEXAMPLE".to_string(),
                    secret_access_key: "redacted-in-tests".to_string(),
                    session_token: Some("session-token".to_string()),
                },
                renewal: RemoteEasyconnectSessionRenewal {
                    renew_url: "/api/v1/remote/easyconnect/sessions/session-1/renew".to_string(),
                    renew_after_utc: "2026-07-09T20:20:00Z".to_string(),
                    renewal_token: "rotated-renewal-token".to_string(),
                },
            },
        };
        let encoded = serde_json::to_value(response).expect("response serializes");
        assert_eq!(encoded["session"]["expires_at_utc"], "2026-07-09T21:20:00Z");
        assert_eq!(
            encoded["session"]["renewal"]["renew_after_utc"],
            "2026-07-09T20:20:00Z"
        );
        assert_eq!(
            encoded["session"]["renewal"]["renewal_token"],
            "rotated-renewal-token"
        );
    }

    #[test]
    fn rejects_grant_without_access() {
        let grant = RemoteEasyconnectObjectStoreGrant {
            object_store: "zymo".to_string(),
            bucket: "dos-zymo".to_string(),
            can_read: false,
            can_write: false,
            writer_group: None,
            object_type: "fastq".to_string(),
        };

        let err = grant.validate().expect_err("access required");

        assert!(matches!(
            err,
            RemoteEasyconnectValidationError::GrantWithoutAccess { .. }
        ));
    }

    #[test]
    fn filters_remote_grants_to_actor_read_and_write_membership() {
        let actor = DaemonLocalActor::new(1000)
            .with_username("stephen")
            .with_groups(["mnemosyne", "readers"]);
        let stores = vec![
            store("generated", "dos-generated")
                .with_reader_group("readers")
                .with_writer_group("mnemosyne"),
            store("archive", "dos-archive")
                .with_reader_group("readers")
                .with_writer_group("archive-writers"),
            store("private", "dos-private").with_writer_group("private-writers"),
        ];

        let grants =
            remote_easyconnect_object_store_grants_for_actor(&actor, &stores).expect("grants");

        assert_eq!(grants.len(), 2);
        assert_eq!(grants[0].object_store, "generated");
        assert!(grants[0].can_read);
        assert!(grants[0].can_write);
        assert_eq!(grants[1].object_store, "archive");
        assert!(grants[1].can_read);
        assert!(!grants[1].can_write);
    }

    #[test]
    fn public_read_does_not_grant_remote_write_without_writer_membership() {
        let actor = DaemonLocalActor::new(1001)
            .with_username("guest")
            .with_groups(["users"]);
        let stores = vec![store("public-cache", "dos-public-cache")
            .with_public_read(true)
            .with_writer_group("cache-writers")];

        let grants =
            remote_easyconnect_object_store_grants_for_actor(&actor, &stores).expect("grants");

        assert_eq!(grants.len(), 1);
        assert_eq!(grants[0].object_store, "public-cache");
        assert!(grants[0].can_read);
        assert!(!grants[0].can_write);
    }

    #[test]
    fn locked_store_never_grants_remote_write_even_for_writer_member() {
        let actor = DaemonLocalActor::new(1000)
            .with_username("stephen")
            .with_groups(["mnemosyne"]);
        let stores = vec![store("locked", "dos-locked")
            .with_writer_group("mnemosyne")
            .with_writable(false)];

        let grants =
            remote_easyconnect_object_store_grants_for_actor(&actor, &stores).expect("grants");

        assert_eq!(grants.len(), 1);
        assert!(grants[0].can_read);
        assert!(!grants[0].can_write);
    }

    #[test]
    fn admin_group_can_receive_remote_write_grant() {
        let actor = DaemonLocalActor::new(1000)
            .with_username("operator")
            .with_groups(["dasobjectstore-admin"]);
        let stores = vec![store("generated", "dos-generated")
            .with_writer_group("mnemosyne")
            .with_admin_group("dasobjectstore-admin")];

        let grants =
            remote_easyconnect_object_store_grants_for_actor(&actor, &stores).expect("grants");

        assert_eq!(grants.len(), 1);
        assert!(grants[0].can_read);
        assert!(grants[0].can_write);
    }

    #[test]
    fn plans_loopback_remote_upload_handoff_with_confirmation_and_privacy() {
        let response =
            plan_remote_easyconnect_upload_handoff(upload_handoff_request()).expect("handoff plan");

        assert_eq!(response.handoff_id, "handoff-1");
        assert_eq!(
            response.mode,
            RemoteEasyconnectUploadHandoffMode::LoopbackPost
        );
        assert_eq!(
            response.state,
            RemoteEasyconnectUploadHandoffState::ConfirmationRequired
        );
        assert_eq!(response.ingress_origin, DaemonIngressOrigin::RemoteS3);
        assert_eq!(response.landing_mode, DaemonIngressLandingMode::SsdFirst);
        assert_eq!(response.backpressure_policy.max_s3_transfer_concurrency, 2);
        assert_eq!(response.backpressure_policy.max_ssd_stage_queue_depth, 4);
        assert_eq!(
            response.local_agent_handoff_url,
            "http://127.0.0.1:49329/v1/dasobjectstore/remote/uploads/handoffs"
        );
        assert_eq!(
            response.confirmation_phrase,
            "confirm upload to zymo_fecal_2025.05"
        );
        assert!(response.path_privacy.contains("relative display paths"));
        assert!(response
            .failure_states
            .iter()
            .any(|failure| failure.code == "agent_unreachable" && failure.retryable));
    }

    #[test]
    fn accepts_drag_drop_folder_expansion_as_browser_agent_handoff_metadata() {
        let mut request = upload_handoff_request();
        request.selected_files = vec![
            RemoteEasyconnectUploadSelectionEntry {
                display_path: "run-42/Sample_A/L001/R1.fastq.gz".to_string(),
                size_bytes: 4096,
            },
            RemoteEasyconnectUploadSelectionEntry {
                display_path: "run-42/Sample_A/L001/R2.fastq.gz".to_string(),
                size_bytes: 8192,
            },
            RemoteEasyconnectUploadSelectionEntry {
                display_path: "run-42/manifests/sample-sheet.csv".to_string(),
                size_bytes: 512,
            },
        ];
        request.total_bytes = 12_800;

        let response =
            plan_remote_easyconnect_upload_handoff(request).expect("folder-expanded selection");

        assert_eq!(
            response.mode,
            RemoteEasyconnectUploadHandoffMode::LoopbackPost
        );
        assert_eq!(
            response.state,
            RemoteEasyconnectUploadHandoffState::ConfirmationRequired
        );
        assert_eq!(
            response.local_agent_handoff_url,
            "http://127.0.0.1:49329/v1/dasobjectstore/remote/uploads/handoffs"
        );
        assert!(response.message.contains("explicit user confirmation"));
    }

    #[test]
    fn reports_pre_transfer_agent_and_user_cancellation_failure_states() {
        let response =
            plan_remote_easyconnect_upload_handoff(upload_handoff_request()).expect("handoff plan");

        assert_eq!(
            response.state,
            RemoteEasyconnectUploadHandoffState::ConfirmationRequired
        );
        assert_handoff_failure(&response, "agent_unreachable", true);
        assert_handoff_failure(&response, "confirmation_cancelled", true);
        assert_handoff_failure(&response, "path_privacy_violation", false);
        assert!(response
            .failure_states
            .iter()
            .find(|failure| failure.code == "confirmation_cancelled")
            .expect("cancellation failure state")
            .message
            .contains("before the local agent received transfer authority"));
    }

    #[test]
    fn rejects_non_loopback_remote_upload_handoff_url() {
        let mut request = upload_handoff_request();
        request.local_agent_base_url = "https://192.168.1.23:49329".to_string();

        let err = plan_remote_easyconnect_upload_handoff(request)
            .expect_err("non-loopback handoff rejected");

        assert!(matches!(
            err,
            RemoteEasyconnectValidationError::InvalidLoopbackUrl {
                field: "local_agent_base_url",
                ..
            }
        ));
    }

    #[test]
    fn rejects_empty_or_inconsistent_remote_upload_selection() {
        let mut empty = upload_handoff_request();
        empty.selected_files.clear();
        assert!(matches!(
            plan_remote_easyconnect_upload_handoff(empty).expect_err("empty selection rejected"),
            RemoteEasyconnectValidationError::EmptyUploadSelection
        ));

        let mut mismatch = upload_handoff_request();
        mismatch.total_bytes = 99;
        assert!(matches!(
            plan_remote_easyconnect_upload_handoff(mismatch).expect_err("byte mismatch rejected"),
            RemoteEasyconnectValidationError::UploadSelectionByteMismatch { .. }
        ));
    }

    #[test]
    fn rejects_absolute_browser_display_paths_for_privacy() {
        for display_path in [
            "/Users/stephen/private.fastq.gz",
            r"C:\Users\stephen\private.fastq.gz",
            r"\Users\stephen\private.fastq.gz",
        ] {
            let mut request = upload_handoff_request();
            request.selected_files[0].display_path = display_path.to_string();

            let err = plan_remote_easyconnect_upload_handoff(request)
                .expect_err("absolute source path rejected");

            assert!(matches!(
                err,
                RemoteEasyconnectValidationError::AbsoluteUploadSelectionPath { .. }
            ));
        }
    }

    #[test]
    fn remote_upload_admission_accepts_when_queues_have_capacity() {
        let decision = decide_remote_easyconnect_upload_admission(admission_request());

        assert_eq!(decision.action, RemoteUploadBackpressureAction::Accept);
        assert_eq!(
            decision.reason,
            RemoteEasyconnectUploadBackpressureReason::AllClear
        );
        assert_eq!(decision.retry_after_seconds, None);
    }

    #[test]
    fn remote_upload_admission_rejects_new_intake_on_critical_ssd_pressure() {
        let mut request = admission_request();
        request.ssd_pressure = DaemonSsdPressure::Critical;

        let decision = decide_remote_easyconnect_upload_admission(request);

        assert_eq!(
            decision.action,
            RemoteUploadBackpressureAction::RejectNewTransfers
        );
        assert_eq!(
            decision.reason,
            RemoteEasyconnectUploadBackpressureReason::SsdCriticalPressure
        );
        assert_eq!(decision.retry_after_seconds, Some(30));
    }

    #[test]
    fn remote_upload_admission_pauses_when_any_bounded_queue_is_full() {
        let policy = RemoteUploadBackpressurePolicy::default();
        let mut request = admission_request();
        request.active_s3_transfers = policy.max_s3_transfer_concurrency;
        assert_paused_for(
            request,
            RemoteEasyconnectUploadBackpressureReason::S3TransferConcurrencyFull,
        );

        let mut request = admission_request();
        request.ssd_stage_queue_depth = policy.max_ssd_stage_queue_depth;
        assert_paused_for(
            request,
            RemoteEasyconnectUploadBackpressureReason::SsdStageQueueFull,
        );

        let mut request = admission_request();
        request.hdd_landing_queue_depth = policy.max_hdd_landing_queue_depth;
        assert_paused_for(
            request,
            RemoteEasyconnectUploadBackpressureReason::HddLandingQueueFull,
        );

        let mut request = admission_request();
        request.verification_queue_depth = policy.max_verification_queue_depth;
        assert_paused_for(
            request,
            RemoteEasyconnectUploadBackpressureReason::VerificationQueueFull,
        );
    }

    fn assert_paused_for(
        request: RemoteEasyconnectUploadAdmissionRequest,
        reason: RemoteEasyconnectUploadBackpressureReason,
    ) {
        let decision = decide_remote_easyconnect_upload_admission(request);

        assert_eq!(
            decision.action,
            RemoteUploadBackpressureAction::PauseNewTransfers
        );
        assert_eq!(decision.reason, reason);
        assert_eq!(decision.retry_after_seconds, Some(30));
    }

    fn assert_handoff_failure(
        response: &RemoteEasyconnectUploadHandoffResponse,
        code: &str,
        retryable: bool,
    ) {
        let failure = response
            .failure_states
            .iter()
            .find(|failure| failure.code == code)
            .expect("expected handoff failure state");

        assert_eq!(failure.retryable, retryable);
    }

    fn store(object_store: &str, bucket: &str) -> RemoteEasyconnectObjectStoreAccessPolicy {
        RemoteEasyconnectObjectStoreAccessPolicy {
            object_store: object_store.to_string(),
            bucket: bucket.to_string(),
            reader_group: None,
            writer_group: None,
            admin_group: None,
            public_read: false,
            writable: true,
            object_type: "fastq".to_string(),
        }
    }

    fn upload_handoff_request() -> RemoteEasyconnectUploadHandoffRequest {
        RemoteEasyconnectUploadHandoffRequest {
            session_id: "session-1".to_string(),
            object_store: "zymo_fecal_2025.05".to_string(),
            bucket: "dos-zymo-fecal-2025-05".to_string(),
            local_agent_base_url: "http://127.0.0.1:49329".to_string(),
            selected_files: vec![
                RemoteEasyconnectUploadSelectionEntry {
                    display_path: "zymo/raw/r1.fastq.gz".to_string(),
                    size_bytes: 1024,
                },
                RemoteEasyconnectUploadSelectionEntry {
                    display_path: "zymo/raw/r2.fastq.gz".to_string(),
                    size_bytes: 2048,
                },
            ],
            total_bytes: 3072,
            browser_origin: "https://192.168.1.192:8448".to_string(),
            client_request_id: Some("handoff-1".to_string()),
        }
    }

    fn admission_request() -> RemoteEasyconnectUploadAdmissionRequest {
        RemoteEasyconnectUploadAdmissionRequest {
            policy: RemoteUploadBackpressurePolicy::default(),
            ssd_pressure: DaemonSsdPressure::AcceptingWrites,
            active_s3_transfers: 1,
            ssd_stage_queue_depth: 1,
            hdd_landing_queue_depth: 1,
            verification_queue_depth: 1,
        }
    }

    trait StorePolicyFixture {
        fn with_reader_group(self, group: &str) -> Self;
        fn with_writer_group(self, group: &str) -> Self;
        fn with_admin_group(self, group: &str) -> Self;
        fn with_public_read(self, public_read: bool) -> Self;
        fn with_writable(self, writable: bool) -> Self;
    }

    impl StorePolicyFixture for RemoteEasyconnectObjectStoreAccessPolicy {
        fn with_reader_group(mut self, group: &str) -> Self {
            self.reader_group = Some(group.to_string());
            self
        }

        fn with_writer_group(mut self, group: &str) -> Self {
            self.writer_group = Some(group.to_string());
            self
        }

        fn with_admin_group(mut self, group: &str) -> Self {
            self.admin_group = Some(group.to_string());
            self
        }

        fn with_public_read(mut self, public_read: bool) -> Self {
            self.public_read = public_read;
            self
        }

        fn with_writable(mut self, writable: bool) -> Self {
            self.writable = writable;
            self
        }
    }
}
