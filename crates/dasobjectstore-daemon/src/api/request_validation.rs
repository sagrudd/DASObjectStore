use super::*;

pub(super) fn remote_easyconnect_validation_error(
    err: RemoteEasyconnectValidationError,
) -> DaemonRequestValidationError {
    match err {
        RemoteEasyconnectValidationError::BlankField { field } => {
            DaemonRequestValidationError::BlankField { field }
        }
        RemoteEasyconnectValidationError::InvalidUrl { field, value } => {
            DaemonRequestValidationError::UnsupportedFieldValue { field, value }
        }
        RemoteEasyconnectValidationError::InvalidLoopbackUrl { field, value } => {
            DaemonRequestValidationError::UnsupportedFieldValue { field, value }
        }
        RemoteEasyconnectValidationError::InvalidRequestedLifetime { seconds } => {
            DaemonRequestValidationError::UnsupportedFieldValue {
                field: "requested_session_lifetime_seconds",
                value: seconds.to_string(),
            }
        }
        RemoteEasyconnectValidationError::EmptyObjectStoreGrants => {
            DaemonRequestValidationError::BlankField {
                field: "allowed_object_stores",
            }
        }
        RemoteEasyconnectValidationError::EmptyUploadSelection => {
            DaemonRequestValidationError::BlankField {
                field: "selected_files",
            }
        }
        RemoteEasyconnectValidationError::GrantWithoutAccess { object_store } => {
            DaemonRequestValidationError::UnsupportedFieldValue {
                field: "allowed_object_stores.access",
                value: object_store,
            }
        }
        RemoteEasyconnectValidationError::AbsoluteUploadSelectionPath { display_path } => {
            DaemonRequestValidationError::UnsupportedFieldValue {
                field: "selected_files.display_path",
                value: display_path,
            }
        }
        RemoteEasyconnectValidationError::UploadSelectionByteMismatch { expected, actual } => {
            DaemonRequestValidationError::UnsupportedFieldValue {
                field: "total_bytes",
                value: format!("expected {expected}, got {actual}"),
            }
        }
        RemoteEasyconnectValidationError::EmptyAwsCliArgs => {
            DaemonRequestValidationError::BlankField { field: "args" }
        }
        RemoteEasyconnectValidationError::InvalidAwsCliEnvironmentVariable { name } => {
            DaemonRequestValidationError::UnsupportedFieldValue {
                field: "environment.name",
                value: name,
            }
        }
        RemoteEasyconnectValidationError::UnsupportedCompletionProvider { provider } => {
            DaemonRequestValidationError::UnsupportedFieldValue {
                field: "completion.provider",
                value: provider,
            }
        }
        RemoteEasyconnectValidationError::ZeroObjectVersion => {
            DaemonRequestValidationError::UnsupportedFieldValue {
                field: "completion.object_version",
                value: "0".to_string(),
            }
        }
        RemoteEasyconnectValidationError::InvalidCompletionChecksum => {
            DaemonRequestValidationError::UnsupportedFieldValue {
                field: "completion.expected_checksum",
                value: "invalid sha256 digest".to_string(),
            }
        }
        RemoteEasyconnectValidationError::InvalidCompletionObjectKey => {
            DaemonRequestValidationError::UnsupportedFieldValue {
                field: "completion.object_key",
                value: "unsafe relative key".to_string(),
            }
        }
    }
}

pub(super) fn endpoint_inventory_validation_error(
    err: EndpointInventoryValidationError,
) -> DaemonRequestValidationError {
    match err {
        EndpointInventoryValidationError::BlankField { field } => {
            DaemonRequestValidationError::BlankField { field }
        }
        EndpointInventoryValidationError::UnsafeLocalName { field, value } => {
            DaemonRequestValidationError::UnsafeLocalName { field, value }
        }
        EndpointInventoryValidationError::InvalidUrl { field, value } => {
            DaemonRequestValidationError::UnsupportedFieldValue { field, value }
        }
        EndpointInventoryValidationError::BlankClientRequestId => {
            DaemonRequestValidationError::BlankClientRequestId
        }
        EndpointInventoryValidationError::ConfirmationMismatch => {
            DaemonRequestValidationError::ConfirmationMismatch {
                expected: ENDPOINT_RECORD_CONFIRMATION,
            }
        }
    }
}

pub(super) fn application_identity_registration_validation_error(
    error: ApplicationIdentityRegistrationValidationError,
) -> DaemonRequestValidationError {
    DaemonRequestValidationError::InvalidPolicy {
        message: error.to_string(),
    }
}

pub(super) fn application_key_registration_validation_error(
    error: ApplicationKeyRegistrationValidationError,
) -> DaemonRequestValidationError {
    DaemonRequestValidationError::InvalidPolicy {
        message: error.to_string(),
    }
}

pub(super) fn application_credential_revocation_validation_error(
    error: ApplicationCredentialRevocationValidationError,
) -> DaemonRequestValidationError {
    DaemonRequestValidationError::InvalidPolicy {
        message: error.to_string(),
    }
}

pub(super) fn capacity_admission_validation_error(
    err: CapacityAdmissionValidationError,
) -> DaemonRequestValidationError {
    match err {
        CapacityAdmissionValidationError::InvalidStoreId => {
            DaemonRequestValidationError::UnsafeLocalName {
                field: "store_id",
                value: "invalid store id".to_string(),
            }
        }
        CapacityAdmissionValidationError::InvalidCopyCount => {
            DaemonRequestValidationError::InvalidCopyCount { copies: 0 }
        }
        CapacityAdmissionValidationError::BlankClientRequestId => {
            DaemonRequestValidationError::BlankClientRequestId
        }
    }
}

pub(super) fn create_object_store_validation_error(
    err: CreateObjectStoreValidationError,
) -> DaemonRequestValidationError {
    match err {
        CreateObjectStoreValidationError::BlankField { field } => {
            DaemonRequestValidationError::BlankField { field }
        }
        CreateObjectStoreValidationError::UnsafeName { field, value } => {
            DaemonRequestValidationError::UnsafeLocalName { field, value }
        }
        CreateObjectStoreValidationError::InvalidCopyCount { copies } => {
            DaemonRequestValidationError::InvalidCopyCount { copies }
        }
        CreateObjectStoreValidationError::RelativePath { field, path } => {
            DaemonRequestValidationError::RelativePath { field, path }
        }
        CreateObjectStoreValidationError::BlankClientRequestId => {
            DaemonRequestValidationError::BlankClientRequestId
        }
        CreateObjectStoreValidationError::ConfirmationMismatch => {
            DaemonRequestValidationError::ConfirmationMismatch {
                expected: OBJECT_STORE_CREATE_CONFIRMATION,
            }
        }
        CreateObjectStoreValidationError::InvalidFieldValue { field, value } => {
            DaemonRequestValidationError::UnsupportedFieldValue { field, value }
        }
        CreateObjectStoreValidationError::InvalidPolicy { message } => {
            DaemonRequestValidationError::InvalidPolicy { message }
        }
    }
}

pub(super) fn profile_binding_validation_error(
    err: ProfileBindingValidationError,
) -> DaemonRequestValidationError {
    match err {
        ProfileBindingValidationError::InvalidManifest(message) => {
            DaemonRequestValidationError::InvalidPolicy { message }
        }
        ProfileBindingValidationError::InvalidCapacity(message) => {
            DaemonRequestValidationError::InvalidPolicy { message }
        }
        ProfileBindingValidationError::FiniteCapacityRequired => {
            DaemonRequestValidationError::InvalidPolicy {
                message: "bounded profile requires a finite logical capacity limit".to_string(),
            }
        }
        error @ (ProfileBindingValidationError::StoreIdMismatch
        | ProfileBindingValidationError::CapacityMismatch) => {
            DaemonRequestValidationError::InvalidPolicy {
                message: error.to_string(),
            }
        }
        ProfileBindingValidationError::RelativePath { field, path } => {
            DaemonRequestValidationError::RelativePath { field, path }
        }
        ProfileBindingValidationError::BlankClientRequestId => {
            DaemonRequestValidationError::BlankClientRequestId
        }
        ProfileBindingValidationError::BlankAdministratorActor => {
            DaemonRequestValidationError::BlankField {
                field: "administrator_actor",
            }
        }
        ProfileBindingValidationError::ConfirmationMismatch => {
            DaemonRequestValidationError::ConfirmationMismatch {
                expected: PROFILE_BINDING_CONFIRMATION,
            }
        }
    }
}

pub(super) fn update_object_store_ingest_policy_validation_error(
    err: UpdateObjectStoreIngestPolicyValidationError,
) -> DaemonRequestValidationError {
    match err {
        UpdateObjectStoreIngestPolicyValidationError::InvalidStoreId(value) => {
            DaemonRequestValidationError::UnsafeLocalName {
                field: "store_id",
                value,
            }
        }
        UpdateObjectStoreIngestPolicyValidationError::InvalidIngestMode(value) => {
            DaemonRequestValidationError::UnsupportedFieldValue {
                field: "ingest_mode",
                value,
            }
        }
        UpdateObjectStoreIngestPolicyValidationError::BlankClientRequestId => {
            DaemonRequestValidationError::BlankClientRequestId
        }
        UpdateObjectStoreIngestPolicyValidationError::ConfirmationMismatch => {
            DaemonRequestValidationError::ConfirmationMismatch {
                expected: DIRECT_TO_HDD_POLICY_CONFIRMATION,
            }
        }
    }
}

pub(super) fn prepare_enclosure_validation_error(
    err: PrepareEnclosureValidationError,
) -> DaemonRequestValidationError {
    match err {
        PrepareEnclosureValidationError::RelativePath { field, path } => {
            DaemonRequestValidationError::RelativePath { field, path }
        }
        PrepareEnclosureValidationError::NoHddDevices => DaemonRequestValidationError::BlankField {
            field: "hdd_devices",
        },
        PrepareEnclosureValidationError::BlankHddDiskId => {
            DaemonRequestValidationError::BlankField { field: "disk_id" }
        }
        PrepareEnclosureValidationError::UnsafeName { field, value } => {
            DaemonRequestValidationError::UnsafeLocalName { field, value }
        }
        PrepareEnclosureValidationError::DuplicateHddDiskId { disk_id } => {
            DaemonRequestValidationError::DuplicateFieldValue {
                field: "hdd_devices.disk_id",
                value: disk_id,
            }
        }
        PrepareEnclosureValidationError::DuplicateHddDevicePath { device_path } => {
            DaemonRequestValidationError::DuplicateFieldValue {
                field: "hdd_devices.device_path",
                value: device_path.display().to_string(),
            }
        }
        PrepareEnclosureValidationError::FormatNotAllowed => {
            DaemonRequestValidationError::FormatNotAllowed
        }
        PrepareEnclosureValidationError::ExistingDataNotAcknowledged => {
            DaemonRequestValidationError::ExistingDataNotAcknowledged
        }
        PrepareEnclosureValidationError::ConfirmationMismatch => {
            DaemonRequestValidationError::ConfirmationMismatch {
                expected: ENCLOSURE_PREPARE_CONFIRMATION,
            }
        }
        PrepareEnclosureValidationError::BlankClientRequestId => {
            DaemonRequestValidationError::BlankClientRequestId
        }
    }
}

pub(super) fn disk_lockdown_validation_error(
    error: DiskLockdownValidationError,
) -> DaemonRequestValidationError {
    match error {
        DiskLockdownValidationError::RelativePath { path } => {
            DaemonRequestValidationError::RelativePath {
                field: "mount_root",
                path,
            }
        }
        DiskLockdownValidationError::UnsafeName { field } => {
            DaemonRequestValidationError::UnsafeLocalName {
                field,
                value: "<redacted>".to_string(),
            }
        }
        DiskLockdownValidationError::ConfirmationMismatch => {
            DaemonRequestValidationError::ConfirmationMismatch {
                expected: DISK_LOCKDOWN_CONFIRMATION,
            }
        }
    }
}

pub(super) fn local_admin_validation_error(
    err: DaemonLocalAdminValidationError,
) -> DaemonRequestValidationError {
    match err {
        DaemonLocalAdminValidationError::BlankName { field } => {
            DaemonRequestValidationError::BlankField { field }
        }
        DaemonLocalAdminValidationError::UnsafeName { field, value } => {
            DaemonRequestValidationError::UnsafeLocalName { field, value }
        }
        DaemonLocalAdminValidationError::BlankClientRequestId => {
            DaemonRequestValidationError::BlankClientRequestId
        }
        DaemonLocalAdminValidationError::BlankAdministratorActor => {
            DaemonRequestValidationError::BlankField {
                field: "administrator_actor",
            }
        }
        DaemonLocalAdminValidationError::BlankConfirmationMarker => {
            DaemonRequestValidationError::BlankConfirmationMarker
        }
    }
}

pub(super) fn store_drain_validation_error(
    err: StoreDrainValidationError,
) -> DaemonRequestValidationError {
    match err {
        StoreDrainValidationError::BlankField { field } => {
            DaemonRequestValidationError::BlankField { field }
        }
        StoreDrainValidationError::ConfirmationMismatch => {
            DaemonRequestValidationError::ConfirmationMismatch {
                expected: STORE_DRAIN_CONFIRMATION,
            }
        }
    }
}

pub(super) fn store_delete_validation_error(
    err: StoreDeleteValidationError,
) -> DaemonRequestValidationError {
    match err {
        StoreDeleteValidationError::BlankField { field } => {
            DaemonRequestValidationError::BlankField { field }
        }
        StoreDeleteValidationError::ConfirmationMismatch => {
            DaemonRequestValidationError::ConfirmationMismatch {
                expected: STORE_DELETE_CONFIRMATION,
            }
        }
    }
}

pub(super) fn object_put_validation_error(
    err: ObjectPutValidationError,
) -> DaemonRequestValidationError {
    match err {
        ObjectPutValidationError::BlankField { field } => {
            DaemonRequestValidationError::BlankField { field }
        }
        ObjectPutValidationError::RelativePath { field, path } => {
            DaemonRequestValidationError::RelativePath { field, path }
        }
        ObjectPutValidationError::InvalidCopyCount => {
            DaemonRequestValidationError::InvalidCopyCount { copies: 0 }
        }
    }
}

pub(super) fn disk_retire_validation_error(
    err: DiskRetireValidationError,
) -> DaemonRequestValidationError {
    match err {
        DiskRetireValidationError::BlankDiskId => {
            DaemonRequestValidationError::BlankField { field: "disk_id" }
        }
        DiskRetireValidationError::ConfirmationMismatch => {
            DaemonRequestValidationError::ConfirmationMismatch {
                expected: FORCE_DISK_RETIRE_CONFIRMATION,
            }
        }
    }
}

pub(super) fn ingest_queue_drain_validation_error(
    err: IngestQueueDrainValidationError,
) -> DaemonRequestValidationError {
    match err {
        IngestQueueDrainValidationError::BlankField { field } => {
            DaemonRequestValidationError::BlankField { field }
        }
        IngestQueueDrainValidationError::ConfirmationMismatch => {
            DaemonRequestValidationError::ConfirmationMismatch {
                expected: INGEST_QUEUE_DRAIN_CONFIRMATION,
            }
        }
    }
}

pub(super) fn ingest_control_validation_error(
    err: IngestControlValidationError,
) -> DaemonRequestValidationError {
    match err {
        IngestControlValidationError::BlankReason => {
            DaemonRequestValidationError::BlankField { field: "reason" }
        }
        IngestControlValidationError::ConfirmationMismatch => {
            DaemonRequestValidationError::ConfirmationMismatch {
                expected: INGEST_CONTROL_CONFIRMATION,
            }
        }
    }
}

pub(super) fn generic_job_validation_error(
    err: DaemonJobValidationError,
) -> DaemonRequestValidationError {
    match err {
        DaemonJobValidationError::BlankCancellationReason => {
            DaemonRequestValidationError::BlankCancellationReason
        }
    }
}
