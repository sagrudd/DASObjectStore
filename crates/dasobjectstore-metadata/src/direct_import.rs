use crate::{write_verified_hdd_copy, HddCopyError, HddCopyReport, HddCopyRequest};
use dasobjectstore_core::ids::{DiskId, ObjectId};
use dasobjectstore_core::object_type::ObjectType;
use dasobjectstore_core::risk::{
    ActionConfirmation, RiskGate, RiskGateError, RiskPolicy, RiskyOperation,
};
use dasobjectstore_core::store::{
    IngestMode, StoreClass, StorePolicy, StorePolicyValidationErrors,
};
use serde::Serialize;
use std::fmt::{self, Display};
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectHddImportRequest {
    pub object_id: ObjectId,
    pub disk_id: DiskId,
    pub source_path: PathBuf,
    pub destination_path: PathBuf,
    pub expected_content_hash: String,
    pub object_type: ObjectType,
    pub source_uri: Option<String>,
    pub store_policy: StorePolicy,
    pub risk_policy: RiskPolicy,
    pub confirmation: ActionConfirmation,
}

impl DirectHddImportRequest {
    pub fn new(
        object_id: ObjectId,
        disk_id: DiskId,
        source_path: impl Into<PathBuf>,
        destination_path: impl Into<PathBuf>,
        expected_content_hash: impl Into<String>,
        store_policy: StorePolicy,
        risk_policy: RiskPolicy,
        confirmation: ActionConfirmation,
    ) -> Self {
        Self {
            object_id,
            disk_id,
            source_path: source_path.into(),
            destination_path: destination_path.into(),
            expected_content_hash: expected_content_hash.into(),
            object_type: ObjectType::Naive,
            source_uri: None,
            store_policy,
            risk_policy,
            confirmation,
        }
    }

    pub fn with_source_uri(mut self, source_uri: impl Into<String>) -> Self {
        self.source_uri = Some(source_uri.into());
        self
    }

    pub fn with_object_type(mut self, object_type: ObjectType) -> Self {
        self.object_type = object_type;
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DirectHddImportReport {
    pub object_id: ObjectId,
    pub disk_id: DiskId,
    pub source_path: PathBuf,
    pub destination_path: PathBuf,
    pub object_type: ObjectType,
    pub source_uri: Option<String>,
    pub bytes_written: u64,
    pub content_hash_algorithm: String,
    pub content_hash: String,
    pub warning: String,
}

#[derive(Debug)]
pub enum DirectHddImportError {
    StorePolicy(StorePolicyValidationErrors),
    StoreMustBeReproducibleCache { class: StoreClass },
    StoreMustAllowDirectToHdd,
    MissingExpectedContentHash,
    RiskGate(RiskGateError),
    Copy(HddCopyError),
}

impl Display for DirectHddImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StorePolicy(err) => write!(formatter, "{err}"),
            Self::StoreMustBeReproducibleCache { class } => write!(
                formatter,
                "direct-to-HDD import is only supported for reproducible_cache stores, got {}",
                class.name()
            ),
            Self::StoreMustAllowDirectToHdd => {
                formatter.write_str("store policy must use direct-to-HDD ingest")
            }
            Self::MissingExpectedContentHash => {
                formatter.write_str("direct-to-HDD import requires an expected content hash")
            }
            Self::RiskGate(err) => write!(formatter, "{err}"),
            Self::Copy(err) => write!(formatter, "{err}"),
        }
    }
}

impl std::error::Error for DirectHddImportError {}

impl From<HddCopyError> for DirectHddImportError {
    fn from(err: HddCopyError) -> Self {
        Self::Copy(err)
    }
}

pub fn import_reproducible_object_direct_to_hdd(
    request: &DirectHddImportRequest,
) -> Result<DirectHddImportReport, DirectHddImportError> {
    validate_direct_hdd_import_request(request)?;

    let copy_report = write_verified_hdd_copy(&HddCopyRequest::new(
        request.object_id.clone(),
        request.disk_id.clone(),
        1,
        &request.source_path,
        &request.destination_path,
        request.expected_content_hash.clone(),
    ))?;

    Ok(report_from_copy(request, copy_report))
}

fn validate_direct_hdd_import_request(
    request: &DirectHddImportRequest,
) -> Result<(), DirectHddImportError> {
    request
        .store_policy
        .validate()
        .map_err(DirectHddImportError::StorePolicy)?;

    if request.store_policy.class != StoreClass::ReproducibleCache {
        return Err(DirectHddImportError::StoreMustBeReproducibleCache {
            class: request.store_policy.class,
        });
    }

    if request.store_policy.ingest_mode != IngestMode::DirectToHdd {
        return Err(DirectHddImportError::StoreMustAllowDirectToHdd);
    }

    if request.expected_content_hash.trim().is_empty() {
        return Err(DirectHddImportError::MissingExpectedContentHash);
    }

    RiskGate::new(request.risk_policy)
        .evaluate(RiskyOperation::DirectToHddImport, &request.confirmation)
        .map_err(DirectHddImportError::RiskGate)
}

fn report_from_copy(
    request: &DirectHddImportRequest,
    copy_report: HddCopyReport,
) -> DirectHddImportReport {
    DirectHddImportReport {
        object_id: copy_report.object_id,
        disk_id: copy_report.disk_id,
        source_path: request.source_path.clone(),
        destination_path: copy_report.destination_path,
        object_type: request.object_type,
        source_uri: request.source_uri.clone(),
        bytes_written: copy_report.bytes_written,
        content_hash_algorithm: copy_report.content_hash_algorithm,
        content_hash: copy_report.content_hash,
        warning: "SSD ingest was bypassed; use only for reproducible data with source metadata"
            .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        import_reproducible_object_direct_to_hdd, DirectHddImportError, DirectHddImportRequest,
    };
    use crate::hash::hash_file_sha256;
    use dasobjectstore_core::ids::{DiskId, ObjectId};
    use dasobjectstore_core::object_type::ObjectType;
    use dasobjectstore_core::risk::{ActionConfirmation, RiskGateError, RiskPolicy};
    use dasobjectstore_core::store::{IngestMode, StoreClass, StorePolicy};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn imports_reproducible_object_directly_to_hdd_with_verification() {
        let root = temp_root("direct-import-ok");
        let source_path = root.join("downloads").join("reference.fa.zst");
        let destination_path = root.join("hdd-a").join("objects").join("reference.fa.zst");
        fs::create_dir_all(source_path.parent().expect("source parent")).expect("source dir");
        fs::write(&source_path, b"public reference payload").expect("source payload");
        let expected_hash = hash_file_sha256(&source_path).expect("source hash");
        let request = request(source_path, destination_path.clone(), expected_hash.clone())
            .with_source_uri("https://example.invalid/reference.fa.zst")
            .with_object_type(ObjectType::EnaSra);

        let report =
            import_reproducible_object_direct_to_hdd(&request).expect("direct import succeeds");

        assert_eq!(report.object_id.as_str(), "object-a");
        assert_eq!(report.object_type, ObjectType::EnaSra);
        assert_eq!(report.disk_id.as_str(), "disk-a");
        assert_eq!(report.destination_path, destination_path);
        assert_eq!(report.content_hash, expected_hash);
        assert_eq!(report.bytes_written, 24);
        assert_eq!(
            fs::read(report.destination_path).expect("destination payload"),
            b"public reference payload"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_import_requires_risk_gate_confirmation() {
        let root = temp_root("direct-import-risk");
        let source_path = root.join("source");
        fs::create_dir_all(&root).expect("create temp root");
        fs::write(&source_path, b"payload").expect("source payload");
        let expected_hash = hash_file_sha256(&source_path).expect("source hash");
        let mut request = request(source_path, root.join("dest"), expected_hash);
        request.confirmation = ActionConfirmation::default();

        let err =
            import_reproducible_object_direct_to_hdd(&request).expect_err("confirmation required");

        assert!(matches!(
            err,
            DirectHddImportError::RiskGate(RiskGateError::MissingConfirmation { .. })
        ));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_import_rejects_generated_data_store() {
        let root = temp_root("direct-import-store");
        let source_path = root.join("source");
        fs::create_dir_all(&root).expect("create temp root");
        fs::write(&source_path, b"payload").expect("source payload");
        let expected_hash = hash_file_sha256(&source_path).expect("source hash");
        let mut request = request(source_path, root.join("dest"), expected_hash);
        request.store_policy = StorePolicy::defaults_for(StoreClass::GeneratedData);

        let err =
            import_reproducible_object_direct_to_hdd(&request).expect_err("store class rejected");

        assert!(matches!(
            err,
            DirectHddImportError::StoreMustBeReproducibleCache { .. }
        ));

        let _ = fs::remove_dir_all(root);
    }

    fn request(
        source_path: PathBuf,
        destination_path: PathBuf,
        expected_hash: String,
    ) -> DirectHddImportRequest {
        let mut policy = StorePolicy::defaults_for(StoreClass::ReproducibleCache);
        policy.ingest_mode = IngestMode::DirectToHdd;

        DirectHddImportRequest::new(
            ObjectId::new("object-a").expect("object id"),
            DiskId::new("disk-a").expect("disk id"),
            source_path,
            destination_path,
            expected_hash,
            policy,
            RiskPolicy {
                allow_direct_to_hdd_import: true,
                ..RiskPolicy::default()
            },
            ActionConfirmation::new("confirm direct-to-hdd import"),
        )
    }

    fn temp_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dasobjectstore-metadata-{name}-{}-{nanos}",
            std::process::id()
        ))
    }
}
