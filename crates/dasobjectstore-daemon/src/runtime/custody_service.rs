//! Custody-only service composition. No ordinary registry or service lifecycle is owned here.
mod batch;
use super::custody_garage::retain_garage_custody_object_with_readback;
use super::service::{
    validate_distinct_custody_plane, DaemonServiceRuntimeError, GarageServiceRuntimeConfig,
    ServiceCommandRunner,
};
use super::{
    CustodyAdmissionProvisioningAuthority, CustodyRuntimeCredentialResolver,
    CustodyRuntimeCredentialRole, GarageCustodyS3Reader, GarageCustodyS3Writer,
};
pub use batch::{
    CustodyBatchError, CustodyBatchPhase, CustodyFiniteInventory, CustodyInventoryObject,
};
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_object_service::{
    append_claimed_custody_catalog_entry, claim_custody_catalog_admission,
    custody_ledger_path_for_catalog, inspect_custody_ledger, read_custody_catalog,
    CustodyCatalogBinding,
};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
};

/// Explicit dependencies supplied by service composition, never by a transport request.
/// The excluded ordinary-plane configuration is comparison data only; this controller
/// cannot start, stop or mutate that plane. Configuration separation is not proof of
/// operating-system or credential isolation.
pub struct CustodyServiceBindings<'a> {
    /// Dedicated custody Garage configuration.
    pub custody_plane: &'a GarageServiceRuntimeConfig,
    /// Existing ordinary-plane coordinates that custody must not alias.
    pub excluded_ordinary_plane: &'a GarageServiceRuntimeConfig,
    /// Explicit resolved catalogue, with no fallback path.
    pub catalog: &'a CustodyCatalogBinding,
    /// Existing attended one-use credential authority; absence denies retention.
    pub credentials: Option<&'a Arc<dyn CustodyRuntimeCredentialResolver>>,
    /// Existing attended provisioner authority; absence denies admission.
    pub provisioner: Option<&'a Arc<dyn CustodyAdmissionProvisioningAuthority>>,
}

/// Pending in-process admission results. Durable namespace claims remain in the
/// catalogue and cannot be reset by dropping this state. No credentials are stored here.
/// The first valid controller permanently binds this state to its exact catalogue
/// and both plane configurations; another composition cannot reuse pending results.
#[derive(Default)]
pub struct CustodyServiceState {
    binding: Mutex<Option<CustodyStateBinding>>,
    pending: Mutex<BTreeMap<String, PendingCustodyAdmission>>,
}

#[derive(Clone, Eq, PartialEq)]
struct CustodyStateBinding {
    custody: GarageServiceRuntimeConfig,
    excluded: GarageServiceRuntimeConfig,
    catalog: CustodyCatalogBinding,
}

/// An in-process bridge between the attended fresh-bucket provisioner and
/// admission. Its durable create-new claim protects against recovery or a
/// restart; retaining the unforgeable in-process operation result here stops
/// a detached API caller from inventing a fresh-bucket proof.
struct PendingCustodyAdmission {
    definition: dasobjectstore_object_service::CustodyStoreDefinitionV1,
    fresh_bucket_proof: dasobjectstore_object_service::CustodyFreshBucketProofV1,
    claim: dasobjectstore_object_service::CustodyCatalogAdmissionClaim,
}

/// Narrow custody orchestrator shared by the existing daemon and future reviewed
/// custody-only composition. Constructing it grants no execution admission and performs
/// no provisioning, lifecycle operation or credential lookup.
pub struct CustodyServiceController<'a, R> {
    bindings: CustodyServiceBindings<'a>,
    runner: &'a R,
    state: &'a CustodyServiceState,
}

impl<'a, R: ServiceCommandRunner> CustodyServiceController<'a, R> {
    /// Validate explicit composition without external effects. The first valid
    /// construction records this state's immutable in-process configuration binding.
    ///
    /// # Errors
    /// Denies invalid or aliased plane coordinates using the existing daemon validator.
    pub fn new(
        bindings: CustodyServiceBindings<'a>,
        runner: &'a R,
        state: &'a CustodyServiceState,
    ) -> Result<Self, DaemonServiceRuntimeError> {
        bindings.custody_plane.validate()?;
        validate_distinct_custody_plane(bindings.excluded_ordinary_plane, bindings.custody_plane)?;
        let identity = CustodyStateBinding {
            custody: bindings.custody_plane.clone(),
            excluded: bindings.excluded_ordinary_plane.clone(),
            catalog: bindings.catalog.clone(),
        };
        let mut bound =
            state
                .binding
                .lock()
                .map_err(|_| DaemonServiceRuntimeError::UnsupportedOperation {
                    operation: "custody service state binding is unavailable".to_string(),
                })?;
        if bound.as_ref().is_some_and(|existing| existing != &identity) {
            return Err(DaemonServiceRuntimeError::UnsupportedOperation {
                operation: "custody service state cannot be rebound to another composition"
                    .to_string(),
            });
        }
        if bound.is_none() {
            *bound = Some(identity);
        }
        drop(bound);
        Ok(Self {
            bindings,
            runner,
            state,
        })
    }

    fn custody_catalog_path(&self) -> &Path {
        self.bindings.catalog.path()
    }

    fn custody_plane_config(
        &self,
    ) -> Result<&GarageServiceRuntimeConfig, DaemonServiceRuntimeError> {
        self.bindings.custody_plane.validate()?;
        validate_distinct_custody_plane(
            self.bindings.excluded_ordinary_plane,
            self.bindings.custody_plane,
        )?;
        Ok(self.bindings.custody_plane)
    }

    /// Execute the custody-only, non-idempotent Garage provisioner. The
    /// provisioner key is supplied only for this attended call and is neither
    /// written to the normal credential registry nor returned in the proof.
    pub(super) fn provision_fresh_custody_bucket(
        &self,
        request: &dasobjectstore_object_service::CustodyGarageProvisioningRequest,
        created_at_utc: &str,
        creation_nonce: impl Into<String>,
    ) -> Result<dasobjectstore_object_service::CustodyFreshBucketProofV1, DaemonServiceRuntimeError>
    {
        let custody_config = self.custody_plane_config()?;
        // Validate the complete three-identity provisioner request before
        // making a durable claim. Invalid input must not reserve a namespace
        // or reach Garage's absence probe.
        let _ = dasobjectstore_object_service::plan_custody_garage_provisioning(request)?;
        let definition = dasobjectstore_object_service::CustodyStoreDefinitionV1 {
            store_id: request.store_id.clone(),
            bucket_name: request.bucket_name.clone(),
            profile: request.profile.clone(),
        };
        // Reserve both identity coordinates before the first Garage command.
        // If proving or creating the bucket fails, the claim deliberately
        // remains terminal rather than opening a recovery/reuse route.
        let claim = claim_custody_catalog_admission(self.custody_catalog_path(), &definition)?;
        let proof = super::GarageCustodyProvisioner::new(custody_config, self.runner)
            .provision_fresh(request, created_at_utc, creation_nonce)
            .map_err(DaemonServiceRuntimeError::ObjectService)?;
        let mut pending = self.state.pending.lock().map_err(|_| {
            DaemonServiceRuntimeError::UnsupportedOperation {
                operation: "custody pending-admission authority is unavailable".to_string(),
            }
        })?;
        if pending
            .insert(
                definition.store_id.to_string(),
                PendingCustodyAdmission {
                    definition,
                    fresh_bucket_proof: proof.clone(),
                    claim,
                },
            )
            .is_some()
        {
            return Err(DaemonServiceRuntimeError::UnsupportedOperation {
                operation: "duplicate in-process custody admission is forbidden".to_string(),
            });
        }
        Ok(proof)
    }

    /// Admit a custody ledger only after the dedicated provisioner has
    /// supplied a fresh-bucket proof. This deliberately never calls the
    /// normal mutable registry, credential registry, or owner provisioner.
    ///
    /// # Errors
    /// Denies invalid requests, absent or consumed authority, namespace collisions,
    /// provisioning failures and ledger/catalogue failures without adopting partial work.
    pub fn admit_custody_store(
        &self,
        request: crate::api::CustodyAdmissionRequest,
        accepted_at_utc: &str,
    ) -> Result<crate::api::CustodyAdmissionResponse, DaemonServiceRuntimeError> {
        request
            .validate()
            .map_err(|error| DaemonServiceRuntimeError::UnsupportedOperation {
                operation: error.to_string(),
            })?;
        let _ = self.custody_plane_config()?;
        let catalog_path = self.custody_catalog_path().to_path_buf();
        if request.dry_run {
            // Read-only collision preflight only. A dry run must not reserve a
            // store/bucket claim or create any ledger/catalog parent.
            dasobjectstore_object_service::reject_catalogued_custody_definition(
                &catalog_path,
                &request.definition.store_id,
                &request.definition.bucket_name,
                "custody admission preflight",
            )?;
        } else {
            // The opaque API reference is itself sealed configuration, not a
            // selector for an otherwise definition-shaped credential.  Check
            // it before touching the one-use authority so a client cannot
            // consume a different valid provisioner handoff for this store.
            if request.provisioner_handoff_reference
                != request.definition.profile.provisioner_credential_reference
            {
                return Err(DaemonServiceRuntimeError::UnsupportedOperation {
                    operation: "custody admission provisioner handoff reference is not the exact sealed profile reference"
                        .to_string(),
                });
            }
            let authority = self
                .bindings.provisioner
                .as_ref()
                .ok_or_else(|| DaemonServiceRuntimeError::UnsupportedOperation {
                    operation: "custody admission requires a daemon-owned attended sealed provisioning authority"
                        .to_string(),
                })?;
            let provision_request = authority.consume_one_use_provisioning_request(
                &request.provisioner_handoff_reference,
                &request.definition,
            )?;
            if provision_request.store_id != request.definition.store_id
                || provision_request.bucket_name != request.definition.bucket_name
                || provision_request.profile != request.definition.profile
                || provision_request.provisioner.credential_reference
                    != request.definition.profile.provisioner_credential_reference
            {
                return Err(DaemonServiceRuntimeError::UnsupportedOperation {
                    operation: "daemon-owned custody provisioning authority returned a plan that is not exactly bound to the sealed definition"
                        .to_string(),
                });
            }
            let creation_nonce = format!(
                "daemon-custody-admission:{}:{}",
                request.definition.store_id, accepted_at_utc
            );
            self.provision_fresh_custody_bucket(
                &provision_request,
                accepted_at_utc,
                creation_nonce,
            )?;
            if read_custody_catalog(&catalog_path)?
                .iter()
                .any(|entry| entry.definition.store_id == request.definition.store_id)
            {
                return Err(DaemonServiceRuntimeError::UnsupportedOperation {
                    operation: format!(
                        "custody store {} already has an immutable catalog admission",
                        request.definition.store_id
                    ),
                });
            }
            let pending = self
                .state.pending
                .lock()
                .map_err(|_| DaemonServiceRuntimeError::UnsupportedOperation {
                    operation: "custody pending-admission authority is unavailable".to_string(),
                })?
                .remove(request.definition.store_id.as_str())
                .ok_or_else(|| DaemonServiceRuntimeError::UnsupportedOperation {
                    operation: "custody admission requires the same daemon-owned fresh, claimed provisioner result"
                        .to_string(),
                })?;
            if pending.definition != request.definition {
                return Err(DaemonServiceRuntimeError::UnsupportedOperation {
                    operation: "custody admission does not match its daemon-owned pending fresh-bucket provision"
                        .to_string(),
                });
            }
            let ledger_path =
                custody_ledger_path_for_catalog(&catalog_path, &request.definition.store_id)?;
            dasobjectstore_object_service::create_custody_ledger_from_definition(
                &ledger_path,
                request.definition.clone(),
                pending.fresh_bucket_proof.clone(),
                accepted_at_utc,
            )?;
            let inspection = inspect_custody_ledger(&ledger_path)?;
            // A failure after the create-new ledger but before this durable
            // append leaves an unreachable orphan. It is intentionally not
            // adopted, deleted, or replaced on a retry.
            append_claimed_custody_catalog_entry(
                pending.claim,
                &request.definition,
                &ledger_path,
                &inspection.configuration_sha256,
                accepted_at_utc,
            )?;
        }
        let job_id = crate::api::DaemonJobId::new(format!(
            "custody-admission-{}",
            accepted_at_utc
                .chars()
                .map(|value| if value.is_ascii_alphanumeric() {
                    value
                } else {
                    '-'
                })
                .collect::<String>()
                .trim_matches('-')
        ))
        .map_err(|_| {
            DaemonServiceRuntimeError::InvalidJobId("custody admission job id".to_string())
        })?;
        Ok(crate::api::CustodyAdmissionResponse::accepted(
            job_id,
            accepted_at_utc,
            &request,
        ))
    }

    /// Retain through the sole daemon custody route. Both capability handoffs
    /// are atomically consumed by the attended resolver before any S3 command;
    /// neither raw credential material nor a caller-selected registry/ledger
    /// path exists in the transport request.
    ///
    /// # Errors
    /// Denies mismatched sealed bindings, absent or consumed handoffs, or failed
    /// conditional retention/readback. Consumed handoffs are not restored on failure.
    pub fn retain_custody_object(
        &self,
        request: crate::api::CustodyRetainRequest,
    ) -> Result<crate::api::CustodyRetainResponse, DaemonServiceRuntimeError> {
        request
            .validate()
            .map_err(|error| DaemonServiceRuntimeError::UnsupportedOperation {
                operation: error.to_string(),
            })?;
        let custody_config = self.custody_plane_config()?;
        let store_id = StoreId::new(request.store_id.clone()).map_err(|error| {
            DaemonServiceRuntimeError::UnsupportedOperation {
                operation: format!("custody retain store id is invalid: {error}"),
            }
        })?;
        let entry = read_custody_catalog(self.custody_catalog_path())?
            .into_iter()
            .find(|entry| entry.definition.store_id == store_id)
            .ok_or_else(|| DaemonServiceRuntimeError::UnsupportedOperation {
                operation: format!(
                    "custody retain store {} has no immutable daemon catalog admission",
                    store_id
                ),
            })?;
        let inspection = inspect_custody_ledger(&entry.ledger_path)?;
        if inspection.store_id != entry.definition.store_id
            || inspection.bucket_name != entry.definition.bucket_name
            || inspection.configuration_sha256 != entry.ledger_configuration_sha256
        {
            return Err(DaemonServiceRuntimeError::UnsupportedOperation {
                operation: "custody catalog and sealed ledger binding disagree".to_string(),
            });
        }
        // The request transports only opaque one-use references.  Match them
        // to the sealed definition *before* resolving either handoff so a
        // same-store/role-looking attacker reference cannot consume authority
        // or cause a Garage effect.
        if request.writer_handoff_reference != entry.definition.profile.writer_credential_reference
            || request.reader_handoff_reference
                != entry.definition.profile.reader_credential_reference
        {
            return Err(DaemonServiceRuntimeError::UnsupportedOperation {
                operation: "custody retain handoff references must exactly match the sealed writer and reader profile references".to_string(),
            });
        }
        let resolver = self.bindings.credentials.as_ref().ok_or_else(|| {
            DaemonServiceRuntimeError::UnsupportedOperation {
                operation: "custody retain requires an attended one-use credential authority"
                    .to_string(),
            }
        })?;
        let writer = resolver.consume_one_use(
            CustodyRuntimeCredentialRole::Writer,
            &request.writer_handoff_reference,
            store_id.as_str(),
            &entry.configuration_sha256,
        )?;
        let reader = resolver.consume_one_use(
            CustodyRuntimeCredentialRole::Reader,
            &request.reader_handoff_reference,
            store_id.as_str(),
            &entry.configuration_sha256,
        )?;
        let (writer_identity, writer_environment) = writer.into_parts();
        let (reader_identity, reader_environment) = reader.into_parts();
        if writer_identity != entry.definition.profile.writer_identity
            || reader_identity != entry.definition.profile.reader_identity
            || writer_identity == reader_identity
        {
            return Err(DaemonServiceRuntimeError::UnsupportedOperation {
                operation: "custody retention credentials do not match the sealed roles".into(),
            });
        }
        let scratch_root = entry
            .ledger_path
            .parent()
            .ok_or_else(|| DaemonServiceRuntimeError::UnsupportedOperation {
                operation: "custody ledger path has no parent for daemon scratch boundary"
                    .to_string(),
            })?
            .join(".custody-scratch");
        let mut writer = GarageCustodyS3Writer::new_with_object_lock(
            self.runner,
            &custody_config.endpoint,
            &entry.definition.bucket_name,
            writer_identity,
            writer_environment,
            &scratch_root,
            inspection.object_lock_policy,
            inspection.retention_until_utc,
        )?;
        let reader = GarageCustodyS3Reader::new(
            self.runner,
            &custody_config.endpoint,
            &entry.definition.bucket_name,
            reader_identity,
            reader_environment,
            &scratch_root,
        );
        let receipt = retain_garage_custody_object_with_readback(
            &entry.ledger_path,
            request.input,
            &mut writer,
            &reader,
        )?;
        Ok(crate::api::CustodyRetainResponse { receipt })
    }
}
