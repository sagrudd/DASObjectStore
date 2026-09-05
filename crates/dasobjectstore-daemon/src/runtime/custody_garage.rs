//! Concrete Garage boundary for the local trusted-administrator custody overlay.
//!
//! This is intentionally separate from the normal registry, credential, and
//! profile backends. It invokes Garage for a fresh bucket and AWS-compatible
//! S3 commands for the only admitted data operation: create-if-absent content
//! retention followed by an independent GET readback.

use super::service::{
    docker_compose_args, garage_exec_args, GarageServiceRuntimeConfig, ServiceCommandRunner,
};
use dasobjectstore_object_service::{
    custody_object_key, custody_provisioning_request_sha256, custody_store_definition_sha256,
    plan_custody_garage_provisioning, CustodyFreshBucketProofV1, CustodyGarageCredential,
    CustodyGarageProvisionerIdentity, CustodyGarageProvisioningRequest, CustodyObjectLockPolicyV1,
    CustodyObjectReader, CustodyObjectState, CustodyObjectWriter, CustodyStoreDefinitionV1,
    ObjectServiceError, CUSTODY_FRESH_BUCKET_PROOF_SCHEMA_V1, CUSTODY_OBJECT_LOCK_HOLD_AUTHORITY,
    CUSTODY_OBJECT_LOCK_POLICY_ID,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
#[cfg(test)]
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(test)]
use std::sync::Mutex;

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

static CUSTODY_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Which sealed custody role is being resolved. There is deliberately no
/// provisioner runtime role: the attended provisioner may create one fresh
/// bucket but can never participate in object retention.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CustodyRuntimeCredentialRole {
    Writer,
    Reader,
}

/// In-memory credentials returned by an attended, one-use daemon-local
/// authority. This value has no serializer and its Debug form never reveals a
/// secret. It must not be written to a normal registry, the custody catalog,
/// ledger, command display arguments, or daemon API response.
pub struct CustodyRuntimeCredential {
    pub identity: String,
    environment: Vec<(String, String)>,
}

impl CustodyRuntimeCredential {
    pub fn new(
        identity: impl Into<String>,
        environment: Vec<(String, String)>,
    ) -> Result<Self, ObjectServiceError> {
        let credential = Self {
            identity: identity.into(),
            environment,
        };
        if credential.identity.trim().is_empty()
            || credential.environment.is_empty()
            || credential
                .environment
                .iter()
                .any(|(name, value)| name.trim().is_empty() || value.trim().is_empty())
        {
            return Err(invalid("custody runtime credential is incomplete"));
        }
        Ok(credential)
    }

    pub(crate) fn into_parts(self) -> (String, Vec<(String, String)>) {
        (self.identity, self.environment)
    }
}

impl std::fmt::Debug for CustodyRuntimeCredential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CustodyRuntimeCredential")
            .field("identity", &self.identity)
            .field("environment", &"REDACTED")
            .finish()
    }
}

/// Boundary supplied by the attended host integration. A successful lookup
/// atomically consumes the reference for exactly one store, sealed definition
/// digest, and role; failures are terminal. Production uses only the systemd
/// service-credential handoff below. Test resolvers remain test-only; there is
/// no registry, API, ordinary environment, Keychain, or network fallback.
pub trait CustodyRuntimeCredentialResolver: Send + Sync {
    fn consume_one_use(
        &self,
        role: CustodyRuntimeCredentialRole,
        handoff_reference: &str,
        store_id: &str,
        configuration_sha256: &str,
    ) -> Result<CustodyRuntimeCredential, ObjectServiceError>;
}

/// Daemon-local, attended authority for the single non-idempotent custody
/// provision operation.  The public API carries only an opaque handoff name;
/// this boundary resolves the sealed Garage plan in-process and consumes that
/// name before the first absence probe.  It is deliberately separate from
/// normal store credentials and cannot be invoked through a Garage CLI path.
pub trait CustodyAdmissionProvisioningAuthority: Send + Sync {
    fn consume_one_use_provisioning_request(
        &self,
        handoff_reference: &str,
        definition: &CustodyStoreDefinitionV1,
    ) -> Result<CustodyGarageProvisioningRequest, ObjectServiceError>;
}

/// The only production source for a custody writer or reader credential. A
/// systemd unit supplies the material through its private
/// `CREDENTIALS_DIRECTORY`; DASObjectStore never accepts it through an API,
/// environment variable, ordinary registry, catalog, ledger, or log.
pub const SYSTEMD_CREDENTIALS_DIRECTORY_ENV: &str = "CREDENTIALS_DIRECTORY";
pub const SYSTEMD_CUSTODY_HANDOFF_REFERENCE_PREFIX: &str = "systemd-credential://";
pub const CUSTODY_HANDOFF_CONSUMPTION_DIRECTORY: &str = "custody-handoff-consumptions";

/// Resolves exactly one opaque systemd service credential, then creates an
/// atomic hash-only consumption marker. The marker records no secret or raw
/// handoff reference and turns a daemon restart, race, malformed handoff, or
/// binding mismatch into a terminal first use rather than a replay.
pub struct SystemdServiceCredentialHandoffResolver {
    credentials_directory: PathBuf,
    consumption_root: PathBuf,
}

impl SystemdServiceCredentialHandoffResolver {
    /// Construct only from systemd's service credential directory. Absence is
    /// intentional fail-closed evidence that custody activation was not
    /// explicitly attended and configured.
    pub fn from_service_environment(
        consumption_root: impl Into<PathBuf>,
    ) -> Result<Self, ObjectServiceError> {
        let credentials_directory = std::env::var_os(SYSTEMD_CREDENTIALS_DIRECTORY_ENV)
            .map(PathBuf::from)
            .ok_or_else(|| {
                invalid(
                    "custody credential handoff requires systemd CREDENTIALS_DIRECTORY; no fallback source is supported",
                )
            })?;
        Self::from_systemd_credential_directory(credentials_directory, consumption_root.into())
    }

    fn from_systemd_credential_directory(
        credentials_directory: PathBuf,
        consumption_root: PathBuf,
    ) -> Result<Self, ObjectServiceError> {
        if !credentials_directory.is_absolute() || !consumption_root.is_absolute() {
            return Err(invalid(
                "systemd custody credential and consumption paths must be absolute",
            ));
        }
        let metadata = fs::symlink_metadata(&credentials_directory).map_err(|error| {
            command_error(
                "inspect systemd custody credential directory",
                &credentials_directory,
                error,
            )
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(invalid(
                "systemd custody credential directory must be a real directory",
            ));
        }
        Ok(Self {
            credentials_directory,
            consumption_root,
        })
    }

    #[cfg(test)]
    pub(crate) fn from_test_credential_directory(
        credentials_directory: PathBuf,
        consumption_root: PathBuf,
    ) -> Result<Self, ObjectServiceError> {
        Self::from_systemd_credential_directory(credentials_directory, consumption_root)
    }

    fn credential_path(&self, handoff_reference: &str) -> Result<PathBuf, ObjectServiceError> {
        let name = handoff_credential_name(handoff_reference)?;
        Ok(self.credentials_directory.join(name))
    }

    /// Returns the sole durable one-use marker for an opaque systemd
    /// credential reference.  Deliberately do not include the requested
    /// role, store, configuration, or operation in this key: an attempted
    /// use with any wrong binding must terminally consume the same handoff,
    /// rather than leaving it available for a later, correctly-bound retry.
    fn consume_marker_path(&self, handoff_reference: &str) -> Result<PathBuf, ObjectServiceError> {
        handoff_credential_name(handoff_reference)?;
        let binding = format!("dasobjectstore-custody-handoff-reference-v2\\0{handoff_reference}");
        Ok(self
            .consumption_root
            .join(format!("{}.consumed", sha256_hex(binding.as_bytes()))))
    }
}

impl CustodyRuntimeCredentialResolver for SystemdServiceCredentialHandoffResolver {
    fn consume_one_use(
        &self,
        role: CustodyRuntimeCredentialRole,
        handoff_reference: &str,
        store_id: &str,
        configuration_sha256: &str,
    ) -> Result<CustodyRuntimeCredential, ObjectServiceError> {
        // Claim by opaque reference before loading or validating the secret.
        // This is the terminal boundary for malformed or wrong-bound use.
        let marker_path = self.consume_marker_path(handoff_reference)?;
        claim_systemd_handoff_marker(&marker_path)?;
        let credential_path = self.credential_path(handoff_reference)?;
        let handoff = read_systemd_handoff(&credential_path)?;
        if handoff.role != role
            || handoff.store_id != store_id
            || handoff.configuration_sha256 != configuration_sha256
        {
            return Err(invalid(
                "systemd custody credential does not match its sealed role, store, and definition",
            ));
        }
        let mut environment = vec![
            ("AWS_ACCESS_KEY_ID".to_string(), handoff.aws_access_key_id),
            (
                "AWS_SECRET_ACCESS_KEY".to_string(),
                handoff.aws_secret_access_key,
            ),
        ];
        if let Some(token) = handoff.aws_session_token {
            environment.push(("AWS_SESSION_TOKEN".to_string(), token));
        }
        CustodyRuntimeCredential::new(handoff.identity, environment)
    }
}

/// Production custody admission can only obtain a Garage provisioning plan
/// from the same private systemd credential handoff boundary as retain. The
/// opaque handoff is atomically claimed before its sealed definition is read;
/// its two short-lived Garage key secrets remain in memory only while the
/// daemon's provisioner executes. No normal registry, API field, log, or
/// persisted custody record can contain this plan or either secret.
impl CustodyAdmissionProvisioningAuthority for SystemdServiceCredentialHandoffResolver {
    fn consume_one_use_provisioning_request(
        &self,
        handoff_reference: &str,
        definition: &CustodyStoreDefinitionV1,
    ) -> Result<CustodyGarageProvisioningRequest, ObjectServiceError> {
        // Do not hash the caller-selected definition into the marker.  The
        // marker belongs to the opaque attended handoff itself, so a wrong
        // store/configuration/role attempt cannot be followed by a correct
        // reuse of the same capability.
        let marker_path = self.consume_marker_path(handoff_reference)?;
        claim_systemd_handoff_marker(&marker_path)?;
        let credential_path = self.credential_path(handoff_reference)?;
        let configuration_sha256 = custody_store_definition_sha256(definition)?;
        let handoff = read_systemd_provisioning_handoff(&credential_path)?;
        if handoff.store_id != definition.store_id.to_string()
            || handoff.configuration_sha256 != configuration_sha256
            || handoff.provisioner_identity != definition.profile.provisioner_identity
            || handoff_reference != definition.profile.provisioner_credential_reference
        {
            return Err(invalid(
                "systemd custody provisioning handoff does not match its sealed store, definition, provisioner identity, and reference",
            ));
        }
        Ok(CustodyGarageProvisioningRequest {
            store_id: definition.store_id.clone(),
            bucket_name: definition.bucket_name.clone(),
            profile: definition.profile.clone(),
            provisioner: CustodyGarageProvisionerIdentity {
                identity: handoff.provisioner_identity,
                credential_reference: handoff_reference.to_string(),
            },
            writer: CustodyGarageCredential::new(
                definition.profile.writer_credential_reference.clone(),
                handoff.writer_access_key_id,
                handoff.writer_secret_access_key,
            )?,
            reader: CustodyGarageCredential::new(
                definition.profile.reader_credential_reference.clone(),
                handoff.reader_access_key_id,
                handoff.reader_secret_access_key,
            )?,
        })
    }
}

struct SystemdCustodyHandoff {
    role: CustodyRuntimeCredentialRole,
    store_id: String,
    configuration_sha256: String,
    identity: String,
    aws_access_key_id: String,
    aws_secret_access_key: String,
    aws_session_token: Option<String>,
}

struct SystemdCustodyProvisioningHandoff {
    store_id: String,
    configuration_sha256: String,
    provisioner_identity: String,
    writer_access_key_id: String,
    writer_secret_access_key: String,
    reader_access_key_id: String,
    reader_secret_access_key: String,
}

/// Test-only stand-in for the separately reviewed, attended host credential
/// handoff boundary. It has no persistence, no environment lookup, and no
/// way to inspect a stored credential after construction. Each reference is
/// removed before its binding is checked, deliberately modelling a terminal
/// one-use handoff even when the caller supplies the wrong binding.
#[cfg(test)]
pub(crate) struct TestOnlyCustodyRuntimeCredentialResolver {
    handoffs: Mutex<BTreeMap<String, TestOnlyCustodyRuntimeCredentialHandoff>>,
}

/// Test-only authority for exercising the server admission boundary without
/// creating a credential source. Production composition must supply an
/// attended system credential authority; absence fails closed.
#[cfg(test)]
pub(crate) struct TestOnlyCustodyAdmissionProvisioningAuthority {
    requests: Mutex<BTreeMap<String, CustodyGarageProvisioningRequest>>,
}

#[cfg(test)]
impl TestOnlyCustodyAdmissionProvisioningAuthority {
    pub(crate) fn new(
        handoffs: impl IntoIterator<Item = (String, CustodyGarageProvisioningRequest)>,
    ) -> Result<Self, ObjectServiceError> {
        let mut requests = BTreeMap::new();
        for (reference, request) in handoffs {
            if reference.trim().is_empty() || requests.insert(reference, request).is_some() {
                return Err(invalid(
                    "test custody provisioner handoff references must be unique and nonblank",
                ));
            }
        }
        Ok(Self {
            requests: Mutex::new(requests),
        })
    }
}

#[cfg(test)]
impl CustodyAdmissionProvisioningAuthority for TestOnlyCustodyAdmissionProvisioningAuthority {
    fn consume_one_use_provisioning_request(
        &self,
        handoff_reference: &str,
        definition: &CustodyStoreDefinitionV1,
    ) -> Result<CustodyGarageProvisioningRequest, ObjectServiceError> {
        let request = self
            .requests
            .lock()
            .map_err(|_| invalid("test custody provisioner authority lock is poisoned"))?
            .remove(handoff_reference)
            .ok_or_else(|| {
                invalid("custody provisioner handoff is absent or has already been consumed")
            })?;
        if request.store_id != definition.store_id
            || request.bucket_name != definition.bucket_name
            || request.profile != definition.profile
        {
            return Err(invalid(
                "custody provisioner handoff does not match its sealed definition",
            ));
        }
        Ok(request)
    }
}

#[cfg(test)]
pub(crate) struct TestOnlyCustodyRuntimeCredentialHandoff {
    pub role: CustodyRuntimeCredentialRole,
    pub handoff_reference: String,
    pub store_id: String,
    pub configuration_sha256: String,
    pub credential: CustodyRuntimeCredential,
}

#[cfg(test)]
impl TestOnlyCustodyRuntimeCredentialResolver {
    pub(crate) fn new(
        handoffs: impl IntoIterator<Item = TestOnlyCustodyRuntimeCredentialHandoff>,
    ) -> Result<Self, ObjectServiceError> {
        let mut entries = BTreeMap::new();
        for handoff in handoffs {
            if handoff.handoff_reference.trim().is_empty()
                || handoff.store_id.trim().is_empty()
                || handoff.configuration_sha256.trim().is_empty()
                || entries
                    .insert(handoff.handoff_reference.clone(), handoff)
                    .is_some()
            {
                return Err(invalid(
                    "test custody handoff references and bindings must be unique and nonblank",
                ));
            }
        }
        Ok(Self {
            handoffs: Mutex::new(entries),
        })
    }
}

#[cfg(test)]
impl CustodyRuntimeCredentialResolver for TestOnlyCustodyRuntimeCredentialResolver {
    fn consume_one_use(
        &self,
        role: CustodyRuntimeCredentialRole,
        handoff_reference: &str,
        store_id: &str,
        configuration_sha256: &str,
    ) -> Result<CustodyRuntimeCredential, ObjectServiceError> {
        let handoff = self
            .handoffs
            .lock()
            .map_err(|_| invalid("test custody handoff authority lock is poisoned"))?
            .remove(handoff_reference)
            .ok_or_else(|| invalid("custody handoff is absent or has already been consumed"))?;
        if handoff.role != role
            || handoff.store_id != store_id
            || handoff.configuration_sha256 != configuration_sha256
        {
            return Err(invalid(
                "custody handoff binding does not match its sealed role, store, and definition",
            ));
        }
        Ok(handoff.credential)
    }
}

/// Attended, non-idempotent custody-bucket provisioner. A pre-existing
/// bucket is terminal and no provisioner identity is written to disk.
pub struct GarageCustodyProvisioner<'a, R> {
    config: &'a GarageServiceRuntimeConfig,
    runner: &'a R,
}

impl<'a, R: ServiceCommandRunner> GarageCustodyProvisioner<'a, R> {
    pub fn new(config: &'a GarageServiceRuntimeConfig, runner: &'a R) -> Self {
        Self { config, runner }
    }

    pub fn provision_fresh(
        &self,
        request: &CustodyGarageProvisioningRequest,
        created_at_utc: &str,
        creation_nonce: impl Into<String>,
    ) -> Result<CustodyFreshBucketProofV1, ObjectServiceError> {
        self.config.validate().map_err(runtime_error)?;
        let plan = plan_custody_garage_provisioning(request)?;
        // Reject an invalid identity/grant request before the first Garage
        // probe. A failed input must not be mistaken for fresh-bucket
        // evidence or create a side effect at the custody boundary.
        let absence = self.bucket_absence_evidence(&request.bucket_name)?;
        for command in &plan.commands {
            let args = docker_compose_args(
                self.config,
                garage_exec_args(&self.config.service_name, command.argv()),
            );
            let display_args = docker_compose_args(
                self.config,
                garage_exec_args(&self.config.service_name, command.redacted_argv()),
            );
            // Unlike normal provisioning, every command conflict is terminal:
            // it could mean an existing bucket/key/grant was substituted.
            self.runner
                .run_with_display_args("docker", &args, &display_args)
                .map_err(runtime_error)?;
        }
        let creation = self.bucket_creation_evidence(&request.bucket_name)?;
        verify_exact_custody_grants(
            &creation,
            &request.writer.access_key_id,
            &request.reader.access_key_id,
        )?;
        Ok(CustodyFreshBucketProofV1 {
            schema: CUSTODY_FRESH_BUCKET_PROOF_SCHEMA_V1.to_string(),
            store_id: request.store_id.clone(),
            bucket_name: request.bucket_name.clone(),
            target_id: request.profile.target_id.clone(),
            provisioner_identity: request.provisioner.identity.clone(),
            provisioner_credential_reference: request.provisioner.credential_reference.clone(),
            provisioning_request_sha256: custody_provisioning_request_sha256(request)?,
            absence_evidence_sha256: sha256_hex(absence.as_bytes()),
            creation_evidence_sha256: sha256_hex(creation.as_bytes()),
            creation_nonce: creation_nonce.into(),
            created_at_utc: created_at_utc.to_string(),
        })
    }

    fn bucket_absence_evidence(&self, bucket: &str) -> Result<String, ObjectServiceError> {
        let args = docker_compose_args(
            self.config,
            garage_exec_args(
                &self.config.service_name,
                vec!["bucket".into(), "info".into(), bucket.into()],
            ),
        );
        match self.runner.run("docker", &args) {
            Ok(_) => Err(invalid(
                "custody bucket already exists; fresh creation is required",
            )),
            Err(error) if is_missing_bucket_error(&error.to_string()) => Ok(error.to_string()),
            Err(error) => Err(runtime_error(error)),
        }
    }

    fn bucket_creation_evidence(&self, bucket: &str) -> Result<String, ObjectServiceError> {
        let args = docker_compose_args(
            self.config,
            garage_exec_args(
                &self.config.service_name,
                vec!["bucket".into(), "info".into(), bucket.into()],
            ),
        );
        self.runner
            .run("docker", &args)
            .map(|output| output.stdout)
            .map_err(runtime_error)
    }
}

/// S3 writer holding only the dedicated write credential in memory. Garage
/// 2.3 has no native Object Lock/retention/legal-hold API: the exact policy is
/// enforced by the sealed DAS ledger, conditional content-addressed put,
/// immutable receipt, and subsequent metadata/readback divergence detection.
/// We deliberately do not send unsupported AWS Object Lock headers and never
/// claim provider COMPLIANCE/WORM.
pub struct GarageCustodyS3Writer<'a, R> {
    runner: &'a R,
    endpoint: String,
    bucket: String,
    identity: String,
    environment: Vec<(String, String)>,
    scratch_root: PathBuf,
    object_lock_policy: CustodyObjectLockPolicyV1,
    retention_until_utc: String,
}

/// S3 reader holding only the separate read credential in memory.
pub struct GarageCustodyS3Reader<'a, R> {
    runner: &'a R,
    endpoint: String,
    bucket: String,
    identity: String,
    environment: Vec<(String, String)>,
    scratch_root: PathBuf,
}

impl<'a, R: ServiceCommandRunner> GarageCustodyS3Writer<'a, R> {
    pub fn new(
        runner: &'a R,
        endpoint: impl Into<String>,
        bucket: impl Into<String>,
        identity: impl Into<String>,
        environment: Vec<(String, String)>,
        scratch_root: impl Into<PathBuf>,
    ) -> Self {
        Self::new_with_object_lock(
            runner,
            endpoint,
            bucket,
            identity,
            environment,
            scratch_root,
            CustodyObjectLockPolicyV1::required(),
            "2099-01-01T00:00:00Z",
        )
        .expect("fixed local custody object-lock policy is valid")
    }

    /// Construct the only writer admitted by the custody daemon. The policy
    /// and retention are read from the sealed ledger, never from a normal
    /// profile, request, or S3 client. The timestamp is ledger evidence rather
    /// than a Garage-native lock because Garage has no supported Object Lock
    /// operation; a raw S3 divergence therefore fails custody readback.
    pub fn new_with_object_lock(
        runner: &'a R,
        endpoint: impl Into<String>,
        bucket: impl Into<String>,
        identity: impl Into<String>,
        environment: Vec<(String, String)>,
        scratch_root: impl Into<PathBuf>,
        object_lock_policy: CustodyObjectLockPolicyV1,
        retention_until_utc: impl Into<String>,
    ) -> Result<Self, ObjectServiceError> {
        object_lock_policy.validate()?;
        let retention_until_utc = retention_until_utc.into();
        chrono::DateTime::parse_from_rfc3339(&retention_until_utc).map_err(|error| {
            invalid(format!(
                "custody object-lock retention timestamp is invalid: {error}"
            ))
        })?;
        Ok(Self {
            runner,
            endpoint: endpoint.into(),
            bucket: bucket.into(),
            identity: identity.into(),
            environment,
            scratch_root: scratch_root.into(),
            object_lock_policy,
            retention_until_utc,
        })
    }
}

impl<'a, R: ServiceCommandRunner> GarageCustodyS3Reader<'a, R> {
    pub fn new(
        runner: &'a R,
        endpoint: impl Into<String>,
        bucket: impl Into<String>,
        identity: impl Into<String>,
        environment: Vec<(String, String)>,
        scratch_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            runner,
            endpoint: endpoint.into(),
            bucket: bucket.into(),
            identity: identity.into(),
            environment,
            scratch_root: scratch_root.into(),
        }
    }
}

impl<R: ServiceCommandRunner> CustodyObjectWriter for GarageCustodyS3Writer<'_, R> {
    fn identity(&self) -> &str {
        &self.identity
    }

    fn object_state(&mut self, object_key: &str) -> Result<CustodyObjectState, ObjectServiceError> {
        let args = head_args(&self.bucket, object_key, &self.endpoint);
        match self
            .runner
            .run_with_display_args_and_env("aws", &args, &args, &self.environment)
        {
            Ok(output) => {
                let head: GarageHead = serde_json::from_str(&output.stdout).map_err(|error| {
                    invalid(format!("custody Garage HEAD response is invalid: {error}"))
                })?;
                let sha = head
                    .metadata
                    .dasobjectstore_sha256
                    .as_deref()
                    .ok_or_else(|| invalid("custody Garage object omits SHA-256 metadata"))?;
                self.validate_object_lock_metadata(&head.metadata)?;
                Ok(CustodyObjectState::Existing {
                    content_sha256: sha.to_string(),
                    content_length: head.content_length,
                })
            }
            Err(error) if is_missing_bucket_error(&error.to_string()) => {
                Ok(CustodyObjectState::Missing)
            }
            Err(error) => Err(runtime_error(error)),
        }
    }

    fn put_if_absent(&mut self, object_key: &str, bytes: &[u8]) -> Result<(), ObjectServiceError> {
        self.object_lock_policy.validate()?;
        let expected_key = custody_object_key(&sha256_hex(bytes))?;
        if object_key != expected_key {
            return Err(invalid(
                "custody writer refuses a non-content-addressed object key",
            ));
        }
        let path = write_scratch(&self.scratch_root, "put", bytes)?;
        let args = vec![
            "s3api".into(),
            "put-object".into(),
            "--bucket".into(),
            self.bucket.clone(),
            "--key".into(),
            object_key.to_string(),
            "--body".into(),
            path.display().to_string(),
            "--if-none-match".into(),
            "*".into(),
            "--metadata".into(),
            format!(
                "dasobjectstore-sha256={},dasobjectstore-object-lock-policy={},dasobjectstore-object-lock-shortening-forbidden=true,dasobjectstore-object-lock-delete-forbidden=true,dasobjectstore-object-lock-hold-authority={},dasobjectstore-object-lock-retention-until-utc={}",
                sha256_hex(bytes),
                CUSTODY_OBJECT_LOCK_POLICY_ID,
                CUSTODY_OBJECT_LOCK_HOLD_AUTHORITY,
                self.retention_until_utc,
            ),
            "--endpoint-url".into(),
            self.endpoint.clone(),
        ];
        let result =
            self.runner
                .run_with_display_args_and_env("aws", &args, &args, &self.environment);
        let _ = fs::remove_file(&path);
        result.map(|_| ()).map_err(runtime_error)
    }
}

impl<R: ServiceCommandRunner> GarageCustodyS3Writer<'_, R> {
    fn validate_object_lock_metadata(
        &self,
        metadata: &GarageMetadata,
    ) -> Result<(), ObjectServiceError> {
        if metadata.object_lock_policy.as_deref() != Some(CUSTODY_OBJECT_LOCK_POLICY_ID)
            || metadata.shortening_forbidden.as_deref() != Some("true")
            || metadata.delete_forbidden.as_deref() != Some("true")
            || metadata.hold_authority.as_deref() != Some(CUSTODY_OBJECT_LOCK_HOLD_AUTHORITY)
            || metadata.retention_until_utc.as_deref() != Some(self.retention_until_utc.as_str())
        {
            return Err(invalid(
                "raw S3 bypass detected: custody object lacks the sealed local trusted-administrator Object Lock policy metadata; Garage remains a trusted-administrator limitation, not provider COMPLIANCE/WORM",
            ));
        }
        Ok(())
    }
}

impl<R: ServiceCommandRunner> CustodyObjectReader for GarageCustodyS3Reader<'_, R> {
    fn identity(&self) -> &str {
        &self.identity
    }

    fn read_exact(&mut self, object_key: &str) -> Result<Vec<u8>, ObjectServiceError> {
        let path = scratch_path(&self.scratch_root, "get")?;
        let args = vec![
            "s3api".into(),
            "get-object".into(),
            "--bucket".into(),
            self.bucket.clone(),
            "--key".into(),
            object_key.to_string(),
            "--endpoint-url".into(),
            self.endpoint.clone(),
            path.display().to_string(),
        ];
        self.runner
            .run_with_display_args_and_env("aws", &args, &args, &self.environment)
            .map_err(runtime_error)?;
        let bytes = fs::read(&path)
            .map_err(|error| invalid(format!("read custody Garage GET result: {error}")))?;
        let _ = fs::remove_file(path);
        Ok(bytes)
    }
}

#[derive(Deserialize)]
struct GarageHead {
    #[serde(rename = "ContentLength")]
    content_length: u64,
    #[serde(rename = "Metadata", default)]
    metadata: GarageMetadata,
}
#[derive(Default, Deserialize)]
struct GarageMetadata {
    #[serde(rename = "dasobjectstore-sha256", alias = "dasobjectstore_sha256")]
    dasobjectstore_sha256: Option<String>,
    #[serde(rename = "dasobjectstore-object-lock-policy")]
    object_lock_policy: Option<String>,
    #[serde(rename = "dasobjectstore-object-lock-shortening-forbidden")]
    shortening_forbidden: Option<String>,
    #[serde(rename = "dasobjectstore-object-lock-delete-forbidden")]
    delete_forbidden: Option<String>,
    #[serde(rename = "dasobjectstore-object-lock-hold-authority")]
    hold_authority: Option<String>,
    #[serde(rename = "dasobjectstore-object-lock-retention-until-utc")]
    retention_until_utc: Option<String>,
}

fn head_args(bucket: &str, key: &str, endpoint: &str) -> Vec<String> {
    vec![
        "s3api".into(),
        "head-object".into(),
        "--bucket".into(),
        bucket.into(),
        "--key".into(),
        key.into(),
        "--endpoint-url".into(),
        endpoint.into(),
        "--output".into(),
        "json".into(),
    ]
}
fn scratch_path(root: &Path, operation: &str) -> Result<PathBuf, ObjectServiceError> {
    fs::create_dir_all(root)
        .map_err(|error| invalid(format!("create custody scratch directory: {error}")))?;
    Ok(root.join(format!(
        "custody-{operation}-{}",
        CUSTODY_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )))
}
fn write_scratch(
    root: &Path,
    operation: &str,
    bytes: &[u8],
) -> Result<PathBuf, ObjectServiceError> {
    let path = scratch_path(root, operation)?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .map_err(|error| invalid(format!("create custody scratch object: {error}")))?;
    file.write_all(bytes)
        .map_err(|error| invalid(format!("write custody scratch object: {error}")))?;
    file.sync_all()
        .map_err(|error| invalid(format!("sync custody scratch object: {error}")))?;
    Ok(path)
}
fn sha256_hex(value: &[u8]) -> String {
    hex::encode(Sha256::digest(value))
}
fn is_missing_bucket_error(message: &str) -> bool {
    message.contains("NotFound")
        || message.contains("not found")
        || message.contains("404")
        || message.contains("NoSuch")
}

fn handoff_credential_name(handoff_reference: &str) -> Result<&str, ObjectServiceError> {
    let name = handoff_reference
        .strip_prefix(SYSTEMD_CUSTODY_HANDOFF_REFERENCE_PREFIX)
        .ok_or_else(|| {
            invalid("custody handoff reference must use the opaque systemd-credential:// scheme")
        })?;
    if name.len() < 8
        || name.len() > 128
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(invalid(
            "custody systemd handoff reference must be a bounded opaque credential name",
        ));
    }
    Ok(name)
}

fn claim_systemd_handoff_marker(marker_path: &Path) -> Result<(), ObjectServiceError> {
    let parent = marker_path
        .parent()
        .ok_or_else(|| invalid("custody handoff marker path has no parent"))?;
    fs::create_dir_all(parent)
        .map_err(|error| command_error("create custody handoff marker directory", parent, error))?;
    restrict_custody_directory(parent)?;

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o640);
    let marker = options.open(marker_path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            invalid("custody systemd credential handoff has already been consumed")
        } else {
            command_error(
                "create custody handoff consumption marker",
                marker_path,
                error,
            )
        }
    })?;
    marker.sync_all().map_err(|error| {
        command_error(
            "sync custody handoff consumption marker",
            marker_path,
            error,
        )
    })?;
    sync_directory(parent)
}

fn read_systemd_handoff(path: &Path) -> Result<SystemdCustodyHandoff, ObjectServiceError> {
    let bytes = read_systemd_handoff_bytes(path)?;
    parse_systemd_handoff(&bytes)
}

fn read_systemd_provisioning_handoff(
    path: &Path,
) -> Result<SystemdCustodyProvisioningHandoff, ObjectServiceError> {
    let bytes = read_systemd_handoff_bytes(path)?;
    parse_systemd_provisioning_handoff(&bytes)
}

fn read_systemd_handoff_bytes(path: &Path) -> Result<Vec<u8>, ObjectServiceError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| command_error("inspect systemd custody credential", path, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(invalid(
            "systemd custody credential must be a regular non-symlink file",
        ));
    }
    #[cfg(unix)]
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(invalid(
            "systemd custody credential must not be group- or world-readable",
        ));
    }
    let mut file = File::open(path)
        .map_err(|error| command_error("open systemd custody credential", path, error))?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(8193)
        .read_to_end(&mut bytes)
        .map_err(|error| command_error("read systemd custody credential", path, error))?;
    if bytes.len() > 8192 {
        return Err(invalid(
            "systemd custody credential exceeds the bounded handoff size",
        ));
    }
    Ok(bytes)
}

fn parse_systemd_handoff(bytes: &[u8]) -> Result<SystemdCustodyHandoff, ObjectServiceError> {
    let encoded = std::str::from_utf8(bytes)
        .map_err(|_| invalid("systemd custody credential is not UTF-8"))?;
    let mut fields = std::collections::BTreeMap::new();
    for line in encoded.lines() {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| invalid("systemd custody credential has an invalid field"))?;
        if key.is_empty()
            || value.is_empty()
            || !matches!(
                key,
                "version"
                    | "role"
                    | "store_id"
                    | "configuration_sha256"
                    | "identity"
                    | "aws_access_key_id"
                    | "aws_secret_access_key"
                    | "aws_session_token"
            )
            || fields.insert(key, value).is_some()
        {
            return Err(invalid(
                "systemd custody credential has duplicate, blank, or unsupported fields",
            ));
        }
    }
    let required = |name| {
        fields
            .get(name)
            .filter(|value| !value.trim().is_empty())
            .copied()
            .ok_or_else(|| invalid("systemd custody credential is missing a required field"))
    };
    if required("version")? != "1" {
        return Err(invalid("systemd custody credential version is unsupported"));
    }
    let role = match required("role")? {
        "writer" => CustodyRuntimeCredentialRole::Writer,
        "reader" => CustodyRuntimeCredentialRole::Reader,
        _ => return Err(invalid("systemd custody credential role is unsupported")),
    };
    let store_id = required("store_id")?.to_string();
    if dasobjectstore_core::ids::StoreId::new(store_id.clone()).is_err() {
        return Err(invalid("systemd custody credential store id is invalid"));
    }
    let configuration_sha256 = required("configuration_sha256")?.to_string();
    if configuration_sha256.len() != 64
        || !configuration_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(invalid(
            "systemd custody credential configuration digest is invalid",
        ));
    }
    Ok(SystemdCustodyHandoff {
        role,
        store_id,
        configuration_sha256,
        identity: required("identity")?.to_string(),
        aws_access_key_id: required("aws_access_key_id")?.to_string(),
        aws_secret_access_key: required("aws_secret_access_key")?.to_string(),
        aws_session_token: fields
            .get("aws_session_token")
            .map(|value| (*value).to_string()),
    })
}

fn parse_systemd_provisioning_handoff(
    bytes: &[u8],
) -> Result<SystemdCustodyProvisioningHandoff, ObjectServiceError> {
    let encoded = std::str::from_utf8(bytes)
        .map_err(|_| invalid("systemd custody provisioning credential is not UTF-8"))?;
    let mut fields = std::collections::BTreeMap::new();
    for line in encoded.lines() {
        let (key, value) = line.split_once('=').ok_or_else(|| {
            invalid("systemd custody provisioning credential has an invalid field")
        })?;
        if key.is_empty()
            || value.is_empty()
            || !matches!(
                key,
                "version"
                    | "role"
                    | "store_id"
                    | "configuration_sha256"
                    | "provisioner_identity"
                    | "writer_access_key_id"
                    | "writer_secret_access_key"
                    | "reader_access_key_id"
                    | "reader_secret_access_key"
            )
            || fields.insert(key, value).is_some()
        {
            return Err(invalid(
                "systemd custody provisioning credential has duplicate, blank, or unsupported fields",
            ));
        }
    }
    let required = |name| {
        fields
            .get(name)
            .filter(|value| !value.trim().is_empty())
            .copied()
            .ok_or_else(|| {
                invalid("systemd custody provisioning credential is missing a required field")
            })
    };
    if required("version")? != "1" || required("role")? != "provisioner" {
        return Err(invalid(
            "systemd custody provisioning credential version or role is unsupported",
        ));
    }
    let store_id = required("store_id")?.to_string();
    if dasobjectstore_core::ids::StoreId::new(store_id.clone()).is_err() {
        return Err(invalid("systemd custody provisioning store id is invalid"));
    }
    let configuration_sha256 = required("configuration_sha256")?.to_string();
    if configuration_sha256.len() != 64
        || !configuration_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(invalid(
            "systemd custody provisioning configuration digest is invalid",
        ));
    }
    Ok(SystemdCustodyProvisioningHandoff {
        store_id,
        configuration_sha256,
        provisioner_identity: required("provisioner_identity")?.to_string(),
        writer_access_key_id: required("writer_access_key_id")?.to_string(),
        writer_secret_access_key: required("writer_secret_access_key")?.to_string(),
        reader_access_key_id: required("reader_access_key_id")?.to_string(),
        reader_secret_access_key: required("reader_secret_access_key")?.to_string(),
    })
}

fn restrict_custody_directory(path: &Path) -> Result<(), ObjectServiceError> {
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o750))
        .map_err(|error| command_error("restrict custody handoff marker directory", path, error))?;
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), ObjectServiceError> {
    #[cfg(unix)]
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| command_error("sync custody handoff marker directory", path, error))?;
    Ok(())
}

fn command_error(action: &str, path: &Path, error: std::io::Error) -> ObjectServiceError {
    ObjectServiceError::CommandFailed(format!("{action} {}: {error}", path.display()))
}

fn invalid(message: impl Into<String>) -> ObjectServiceError {
    ObjectServiceError::InvalidConfiguration(message.into())
}
fn runtime_error(error: impl std::fmt::Display) -> ObjectServiceError {
    ObjectServiceError::CommandFailed(error.to_string())
}

/// Garage 2.3's `bucket info` command renders a `KEYS FOR THIS BUCKET` table
/// with three permission flags (read, write, owner), access key, and name.
/// Treat a format we cannot prove as terminal: granting more than the sealed
/// writer/read roles is an unsafe substitute for a successful inspection.
fn verify_exact_custody_grants(
    bucket_info: &str,
    writer_access_key_id: &str,
    reader_access_key_id: &str,
) -> Result<(), ObjectServiceError> {
    let (_, key_table) = bucket_info
        .split_once("==== KEYS FOR THIS BUCKET ====")
        .ok_or_else(|| invalid("Garage bucket info lacks its key-grant table"))?;
    if key_table.to_ascii_lowercase().contains("owner") {
        return Err(invalid(
            "Garage custody bucket inspection reports an owner grant",
        ));
    }
    let rows = key_table
        .lines()
        .filter_map(parse_garage_bucket_permission_row)
        .collect::<Vec<_>>();
    if rows.len() != 2 {
        return Err(invalid(
            "Garage custody bucket must expose exactly the sealed writer and reader grants",
        ));
    }
    let writer = rows
        .iter()
        .find(|row| row.access_key_id == writer_access_key_id)
        .ok_or_else(|| invalid("Garage custody bucket omits the sealed writer grant"))?;
    let reader = rows
        .iter()
        .find(|row| row.access_key_id == reader_access_key_id)
        .ok_or_else(|| invalid("Garage custody bucket omits the sealed reader grant"))?;
    if writer_access_key_id == reader_access_key_id
        || writer.permissions != "W"
        || reader.permissions != "R"
    {
        return Err(invalid(
            "Garage custody bucket grants must be exactly writer=write and reader=read without owner",
        ));
    }
    Ok(())
}

#[derive(Debug, Eq, PartialEq)]
struct GarageBucketPermissionRow<'a> {
    permissions: String,
    access_key_id: &'a str,
}

fn parse_garage_bucket_permission_row(line: &str) -> Option<GarageBucketPermissionRow<'_>> {
    if line.contains("Permissions") || line.trim().is_empty() {
        return None;
    }
    let permission_end = line
        .char_indices()
        .take_while(|(_, character)| matches!(character, 'R' | 'W' | 'O' | ' ' | '\t' | '|'))
        .last()
        .map(|(index, character)| index + character.len_utf8())?;
    let permissions = line[..permission_end]
        .chars()
        .filter(|character| matches!(character, 'R' | 'W' | 'O'))
        .collect::<String>();
    if permissions.is_empty() {
        return None;
    }
    let access_key_id = line[permission_end..]
        .split(|character: char| character.is_whitespace() || character == '|')
        .find(|field| !field.is_empty())?;
    Some(GarageBucketPermissionRow {
        permissions,
        access_key_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{DaemonServiceRuntimeError, ServiceCommandOutput};
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_object_service::{
        CustodyAssuranceClass, CustodyGarageCredential, CustodyGarageProvisionerIdentity,
        CustodyRetentionPolicyV1, CustodyStoreProfileV1, CUSTODY_OVERLAY_SCHEMA_V1,
        CUSTODY_PROFILE_V1,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn systemd_handoff_is_opaque_one_use_and_persists_no_secret_material() {
        let root = std::env::temp_dir().join(format!(
            "das-custody-systemd-handoff-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let credential_directory = root.join("credentials");
        let consumption_root = root.join("consumed");
        fs::create_dir_all(&credential_directory).expect("credential dir");
        let credential_path = credential_directory.join("opaque-writer-once");
        let secret = "never-persist-or-log-this";
        fs::write(
            &credential_path,
            format!(
                "version=1\nrole=writer\nstore_id=custody-store\nconfiguration_sha256={}\nidentity=custody-writer\naws_access_key_id=writer-access\naws_secret_access_key={secret}\n",
                "a".repeat(64)
            ),
        )
        .expect("credential fixture");
        #[cfg(unix)]
        fs::set_permissions(&credential_path, fs::Permissions::from_mode(0o600))
            .expect("credential mode");
        let resolver = SystemdServiceCredentialHandoffResolver::from_test_credential_directory(
            credential_directory,
            consumption_root.clone(),
        )
        .expect("systemd resolver");

        let credential = resolver
            .consume_one_use(
                CustodyRuntimeCredentialRole::Writer,
                "systemd-credential://opaque-writer-once",
                "custody-store",
                &"a".repeat(64),
            )
            .expect("first credential use");
        assert!(!format!("{credential:?}").contains(secret));
        assert!(resolver
            .consume_one_use(
                CustodyRuntimeCredentialRole::Writer,
                "systemd-credential://opaque-writer-once",
                "custody-store",
                &"a".repeat(64),
            )
            .is_err());
        let markers = fs::read_dir(&consumption_root)
            .expect("marker root")
            .collect::<Result<Vec<_>, _>>()
            .expect("marker entries");
        assert_eq!(markers.len(), 1);
        assert!(markers[0]
            .file_name()
            .to_string_lossy()
            .ends_with(".consumed"));
        assert!(fs::read(markers[0].path())
            .expect("marker bytes")
            .is_empty());
        assert!(!markers[0].file_name().to_string_lossy().contains(secret));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn systemd_handoff_rejects_non_opaque_references_before_marker_creation() {
        let root = std::env::temp_dir().join(format!(
            "das-custody-systemd-reference-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let credential_directory = root.join("credentials");
        let consumption_root = root.join("consumed");
        fs::create_dir_all(&credential_directory).expect("credential dir");
        let resolver = SystemdServiceCredentialHandoffResolver::from_test_credential_directory(
            credential_directory,
            consumption_root.clone(),
        )
        .expect("systemd resolver");

        assert!(resolver
            .consume_one_use(
                CustodyRuntimeCredentialRole::Reader,
                "file:///tmp/credential",
                "custody-store",
                &"a".repeat(64),
            )
            .is_err());
        assert!(!consumption_root.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn test_handoff_authority_consumes_a_reference_before_binding_failure() {
        let resolver = TestOnlyCustodyRuntimeCredentialResolver::new(vec![
            TestOnlyCustodyRuntimeCredentialHandoff {
                role: CustodyRuntimeCredentialRole::Writer,
                handoff_reference: "writer-once".to_string(),
                store_id: "custody-store".to_string(),
                configuration_sha256: "definition-digest".to_string(),
                credential: CustodyRuntimeCredential::new(
                    "writer",
                    vec![("AWS_ACCESS_KEY_ID".to_string(), "writer-key".to_string())],
                )
                .expect("credential"),
            },
        ])
        .expect("resolver");

        assert!(resolver
            .consume_one_use(
                CustodyRuntimeCredentialRole::Reader,
                "writer-once",
                "custody-store",
                "definition-digest",
            )
            .is_err());
        assert!(resolver
            .consume_one_use(
                CustodyRuntimeCredentialRole::Writer,
                "writer-once",
                "custody-store",
                "definition-digest",
            )
            .is_err());
    }

    #[test]
    fn systemd_runtime_handoff_wrong_binding_terminally_consumes_the_same_opaque_reference() {
        let root = std::env::temp_dir().join(format!(
            "das-custody-systemd-terminal-binding-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let credential_directory = root.join("credentials");
        let consumption_root = root.join("consumed");
        fs::create_dir_all(&credential_directory).expect("credential dir");
        let digest = "a".repeat(64);

        for (suffix, wrong_role, wrong_store, wrong_digest) in [
            (
                "wrong-role",
                CustodyRuntimeCredentialRole::Reader,
                "custody-store",
                digest.clone(),
            ),
            (
                "wrong-store",
                CustodyRuntimeCredentialRole::Writer,
                "other-store",
                digest.clone(),
            ),
            (
                "wrong-configuration",
                CustodyRuntimeCredentialRole::Writer,
                "custody-store",
                "b".repeat(64),
            ),
        ] {
            let reference = format!("systemd-credential://writer-{suffix}");
            let credential_path = credential_directory.join(format!("writer-{suffix}"));
            fs::write(
                &credential_path,
                format!(
                    "version=1\nrole=writer\nstore_id=custody-store\nconfiguration_sha256={digest}\nidentity=custody-writer\naws_access_key_id=writer-access\naws_secret_access_key=writer-secret\n"
                ),
            )
            .expect("credential fixture");
            #[cfg(unix)]
            fs::set_permissions(&credential_path, fs::Permissions::from_mode(0o600))
                .expect("credential mode");
            let resolver = SystemdServiceCredentialHandoffResolver::from_test_credential_directory(
                credential_directory.clone(),
                consumption_root.clone(),
            )
            .expect("systemd resolver");

            assert!(resolver
                .consume_one_use(wrong_role, &reference, wrong_store, &wrong_digest)
                .is_err());
            assert!(resolver
                .consume_one_use(
                    CustodyRuntimeCredentialRole::Writer,
                    &reference,
                    "custody-store",
                    &digest,
                )
                .is_err());
        }
        let markers = fs::read_dir(&consumption_root)
            .expect("marker root")
            .collect::<Result<Vec<_>, _>>()
            .expect("marker entries");
        assert_eq!(markers.len(), 3);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn systemd_provisioning_handoff_is_server_only_one_use_and_never_exposes_a_plan() {
        let root = std::env::temp_dir().join(format!(
            "das-custody-systemd-provisioning-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let credential_directory = root.join("credentials");
        let consumption_root = root.join("consumed");
        fs::create_dir_all(&credential_directory).expect("credential dir");
        let mut request = provisioning_request();
        request.profile.provisioner_credential_reference =
            "systemd-credential://provision-once".to_string();
        request.provisioner.credential_reference =
            request.profile.provisioner_credential_reference.clone();
        let definition = CustodyStoreDefinitionV1 {
            store_id: request.store_id.clone(),
            bucket_name: request.bucket_name.clone(),
            profile: request.profile.clone(),
        };
        let digest = custody_store_definition_sha256(&definition).expect("definition digest");
        let secret = "never-crosses-api-or-persistence";
        let credential_path = credential_directory.join("provision-once");
        fs::write(
            &credential_path,
            format!(
                "version=1\nrole=provisioner\nstore_id={}\nconfiguration_sha256={digest}\nprovisioner_identity={}\nwriter_access_key_id={}\nwriter_secret_access_key={secret}\nreader_access_key_id={}\nreader_secret_access_key=another-{secret}\n",
                definition.store_id,
                definition.profile.provisioner_identity,
                request.writer.access_key_id,
                request.reader.access_key_id,
            ),
        )
        .expect("credential fixture");
        #[cfg(unix)]
        fs::set_permissions(&credential_path, fs::Permissions::from_mode(0o600))
            .expect("credential mode");
        let resolver = SystemdServiceCredentialHandoffResolver::from_test_credential_directory(
            credential_directory,
            consumption_root.clone(),
        )
        .expect("systemd resolver");
        let plan = resolver
            .consume_one_use_provisioning_request(
                &definition.profile.provisioner_credential_reference,
                &definition,
            )
            .expect("one sealed provisioning plan");
        assert_eq!(plan.store_id, definition.store_id);
        assert!(!format!("{plan:?}").contains(secret));
        assert!(resolver
            .consume_one_use_provisioning_request(
                &definition.profile.provisioner_credential_reference,
                &definition,
            )
            .is_err());
        let markers = fs::read_dir(&consumption_root)
            .expect("marker root")
            .collect::<Result<Vec<_>, _>>()
            .expect("marker entries");
        assert_eq!(markers.len(), 1);
        assert!(fs::read(markers[0].path())
            .expect("marker bytes")
            .is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn systemd_provisioning_handoff_wrong_definition_terminally_consumes_the_same_reference() {
        let root = std::env::temp_dir().join(format!(
            "das-custody-systemd-provisioning-terminal-binding-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let credential_directory = root.join("credentials");
        let consumption_root = root.join("consumed");
        fs::create_dir_all(&credential_directory).expect("credential dir");
        let mut request = provisioning_request();
        request.profile.provisioner_credential_reference =
            "systemd-credential://provision-terminal".to_string();
        request.provisioner.credential_reference =
            request.profile.provisioner_credential_reference.clone();
        let definition = CustodyStoreDefinitionV1 {
            store_id: request.store_id.clone(),
            bucket_name: request.bucket_name.clone(),
            profile: request.profile.clone(),
        };
        let digest = custody_store_definition_sha256(&definition).expect("definition digest");
        let credential_path = credential_directory.join("provision-terminal");
        fs::write(
            &credential_path,
            format!(
                "version=1\nrole=provisioner\nstore_id={}\nconfiguration_sha256={digest}\nprovisioner_identity={}\nwriter_access_key_id={}\nwriter_secret_access_key=writer-secret\nreader_access_key_id={}\nreader_secret_access_key=reader-secret\n",
                definition.store_id,
                definition.profile.provisioner_identity,
                request.writer.access_key_id,
                request.reader.access_key_id,
            ),
        )
        .expect("credential fixture");
        #[cfg(unix)]
        fs::set_permissions(&credential_path, fs::Permissions::from_mode(0o600))
            .expect("credential mode");
        let resolver = SystemdServiceCredentialHandoffResolver::from_test_credential_directory(
            credential_directory,
            consumption_root,
        )
        .expect("systemd resolver");
        let mut wrong_definition = definition.clone();
        wrong_definition.bucket_name = "other-custody-bucket".to_string();

        assert!(resolver
            .consume_one_use_provisioning_request(
                &definition.profile.provisioner_credential_reference,
                &wrong_definition,
            )
            .is_err());
        assert!(resolver
            .consume_one_use_provisioning_request(
                &definition.profile.provisioner_credential_reference,
                &definition,
            )
            .is_err());
        let _ = fs::remove_dir_all(root);
    }

    struct Runner {
        calls: Mutex<Vec<Vec<String>>>,
    }
    impl ServiceCommandRunner for Runner {
        fn run(
            &self,
            _program: &str,
            args: &[String],
        ) -> Result<ServiceCommandOutput, DaemonServiceRuntimeError> {
            self.calls.lock().unwrap().push(args.to_vec());
            if args.iter().any(|arg| arg == "head-object") {
                return Err(DaemonServiceRuntimeError::CommandFailed {
                    program: "aws".into(),
                    args: args.to_vec(),
                    status: "1".into(),
                    stderr: "NotFound".into(),
                });
            }
            if args.iter().any(|arg| arg == "get-object") {
                fs::write(args.last().unwrap(), b"custody bytes").unwrap();
            }
            Ok(ServiceCommandOutput {
                stdout: String::new(),
            })
        }
    }

    #[test]
    fn real_s3_adapter_uses_conditional_put_and_separate_get_credentials() {
        let runner = Runner {
            calls: Mutex::new(Vec::new()),
        };
        let root = std::env::temp_dir().join(format!("das-custody-adapter-{}", std::process::id()));
        let bytes = b"custody bytes";
        let key = custody_object_key(&sha256_hex(bytes)).unwrap();
        let mut writer = GarageCustodyS3Writer::new(
            &runner,
            "http://garage.invalid",
            "custody",
            "writer",
            vec![("AWS_ACCESS_KEY_ID".into(), "writer-key".into())],
            &root,
        );
        assert_eq!(
            writer.object_state(&key).unwrap(),
            CustodyObjectState::Missing
        );
        writer.put_if_absent(&key, bytes).unwrap();
        let mut reader = GarageCustodyS3Reader::new(
            &runner,
            "http://garage.invalid",
            "custody",
            "reader",
            vec![("AWS_ACCESS_KEY_ID".into(), "reader-key".into())],
            &root,
        );
        assert_eq!(reader.read_exact(&key).unwrap(), bytes);
        let calls = runner.calls.lock().unwrap();
        let put = calls
            .iter()
            .find(|args| args.iter().any(|arg| arg == "put-object"))
            .unwrap();
        assert!(put.windows(2).any(|args| args == ["--if-none-match", "*"]));
        assert!(put
            .iter()
            .any(|arg| arg.contains("local_trusted_administrator_non_shortenable")));
        assert!(put.iter().all(|arg| !arg.starts_with("--object-lock-")));
        assert!(put.iter().all(|arg| arg != "--delete" && arg != "--owner"));
        assert!(calls
            .iter()
            .any(|args| args.iter().any(|arg| arg == "get-object")));
        let _ = fs::remove_dir_all(root);
    }

    struct RawS3BypassRunner;

    impl ServiceCommandRunner for RawS3BypassRunner {
        fn run(
            &self,
            _program: &str,
            args: &[String],
        ) -> Result<ServiceCommandOutput, DaemonServiceRuntimeError> {
            if args.iter().any(|argument| argument == "head-object") {
                return Ok(ServiceCommandOutput {
                    stdout: r#"{"ContentLength":1,"Metadata":{"dasobjectstore-sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}"#.to_string(),
                });
            }
            Ok(ServiceCommandOutput {
                stdout: String::new(),
            })
        }
    }

    #[test]
    fn raw_s3_object_without_the_sealed_overlay_metadata_is_not_custody() {
        let runner = RawS3BypassRunner;
        let mut writer = GarageCustodyS3Writer::new(
            &runner,
            "http://garage.invalid",
            "custody",
            "writer",
            vec![("AWS_ACCESS_KEY_ID".into(), "writer-key".into())],
            std::env::temp_dir(),
        );
        let error = writer
            .object_state(
                "custody/sha256/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            )
            .expect_err("raw S3 metadata cannot impersonate the local custody overlay");
        assert!(error.to_string().contains("raw S3 bypass"));
        assert!(error.to_string().contains("not provider COMPLIANCE/WORM"));
    }

    #[test]
    fn grant_inspection_rejects_owner_extra_and_wrong_permissions() {
        let exact = "==== KEYS FOR THIS BUCKET ====\nPermissions\tAccess key\tLocal aliases\n W\twriter-access\twriter\nR  \treader-access\treader\n";
        verify_exact_custody_grants(exact, "writer-access", "reader-access").unwrap();
        assert!(verify_exact_custody_grants(
            "==== KEYS FOR THIS BUCKET ====\nOW\twriter-access\twriter\nR\treader-access\treader\n",
            "writer-access",
            "reader-access",
        )
        .is_err());
        assert!(verify_exact_custody_grants(
            "==== KEYS FOR THIS BUCKET ====\nRW\twriter-access\twriter\nR\treader-access\treader\n",
            "writer-access",
            "reader-access",
        )
        .is_err());
        assert!(verify_exact_custody_grants(
            "==== KEYS FOR THIS BUCKET ====\nW\twriter-access\twriter\nR\treader-access\treader\nR\textra\textra\n",
            "writer-access",
            "reader-access",
        )
        .is_err());
    }

    struct ProvisionRunner {
        calls: Mutex<Vec<Vec<String>>>,
        bucket_info_calls: AtomicUsize,
        first_bucket_info: Result<ServiceCommandOutput, DaemonServiceRuntimeError>,
        final_bucket_info: String,
    }

    impl ServiceCommandRunner for ProvisionRunner {
        fn run(
            &self,
            _program: &str,
            args: &[String],
        ) -> Result<ServiceCommandOutput, DaemonServiceRuntimeError> {
            self.calls.lock().unwrap().push(args.to_vec());
            if args.windows(2).any(|args| args == ["bucket", "info"]) {
                if self.bucket_info_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    return self.first_bucket_info.clone();
                }
                return Ok(ServiceCommandOutput {
                    stdout: self.final_bucket_info.clone(),
                });
            }
            Ok(ServiceCommandOutput {
                stdout: String::new(),
            })
        }
    }

    fn provisioning_config() -> GarageServiceRuntimeConfig {
        GarageServiceRuntimeConfig {
            compose_file: PathBuf::from("/etc/dasobjectstore/garage.compose.yml"),
            project_directory: Some(PathBuf::from("/var/lib/dasobjectstore/garage")),
            compose_project: "dasobjectstore".to_string(),
            service_name: "garage".to_string(),
            config_path: PathBuf::from("/etc/dasobjectstore/garage.toml"),
            metadata_path: PathBuf::from("/var/lib/dasobjectstore/garage/meta"),
            data_path: PathBuf::from("/srv/dasobjectstore/hdd/garage"),
            endpoint: "http://127.0.0.1:3900".to_string(),
        }
    }

    fn provisioning_request() -> CustodyGarageProvisioningRequest {
        CustodyGarageProvisioningRequest {
            store_id: StoreId::new("custody-store").unwrap(),
            bucket_name: "dos-custody-store".to_string(),
            profile: CustodyStoreProfileV1 {
                schema: CUSTODY_OVERLAY_SCHEMA_V1.to_string(),
                profile: CUSTODY_PROFILE_V1.to_string(),
                assurance_class: CustodyAssuranceClass::LocalTrustedAdministratorOverlay,
                retention: CustodyRetentionPolicyV1::required(),
                target_id: "nuc-192.168.0.193".to_string(),
                retention_until_utc: "2027-09-05T10:00:00Z".to_string(),
                legal_hold: true,
                provisioner_credential_reference: "secret://custody/provisioner".to_string(),
                provisioner_identity: "custody-provisioner-v1".to_string(),
                writer_credential_reference: "secret://custody/writer".to_string(),
                writer_identity: "custody-writer-v1".to_string(),
                reader_credential_reference: "secret://custody/reader".to_string(),
                reader_identity: "custody-reader-v1".to_string(),
            },
            provisioner: CustodyGarageProvisionerIdentity {
                identity: "custody-provisioner-v1".to_string(),
                credential_reference: "secret://custody/provisioner".to_string(),
            },
            writer: CustodyGarageCredential::new(
                "secret://custody/writer",
                "writer-access",
                "writer-secret",
            )
            .unwrap(),
            reader: CustodyGarageCredential::new(
                "secret://custody/reader",
                "reader-access",
                "reader-secret",
            )
            .unwrap(),
        }
    }

    #[test]
    fn fresh_provisioning_requires_absence_and_exact_real_cli_grants() {
        let runner = ProvisionRunner {
            calls: Mutex::new(Vec::new()),
            bucket_info_calls: AtomicUsize::new(0),
            first_bucket_info: Err(DaemonServiceRuntimeError::CommandFailed {
                program: "docker".to_string(),
                args: Vec::new(),
                status: "1".to_string(),
                stderr: "NotFound".to_string(),
            }),
            final_bucket_info: "==== KEYS FOR THIS BUCKET ====\nPermissions\tAccess key\tLocal aliases\n W\twriter-access\twriter\nR  \treader-access\treader\n".to_string(),
        };
        let request = provisioning_request();
        let proof = GarageCustodyProvisioner::new(&provisioning_config(), &runner)
            .provision_fresh(&request, "2026-09-05T10:00:00Z", "fresh-nonce")
            .unwrap();
        proof
            .validate_for(&request, "2026-09-05T10:00:00Z")
            .unwrap();
        let calls = runner.calls.lock().unwrap();
        assert!(calls
            .iter()
            .any(|args| args.windows(2).any(|args| args == ["bucket", "create"])));
        assert!(calls
            .iter()
            .all(|args| !args.iter().any(|arg| arg == "--owner")));
    }

    #[test]
    fn fresh_provisioning_treats_existing_bucket_as_terminal_before_mutation() {
        let runner = ProvisionRunner {
            calls: Mutex::new(Vec::new()),
            bucket_info_calls: AtomicUsize::new(0),
            first_bucket_info: Ok(ServiceCommandOutput {
                stdout: "existing bucket".to_string(),
            }),
            final_bucket_info: String::new(),
        };
        assert!(
            GarageCustodyProvisioner::new(&provisioning_config(), &runner)
                .provision_fresh(
                    &provisioning_request(),
                    "2026-09-05T10:00:00Z",
                    "fresh-nonce"
                )
                .is_err()
        );
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert!(!calls[0].iter().any(|arg| arg == "create"));
    }
}
