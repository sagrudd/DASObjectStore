//! Capability-based backend contract shared by folder, drive, and appliance
//! implementations.

use crate::manifest::{ObjectStoreManifest, ObjectStoreManifestValidationError};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};
use std::io::Read;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct BackendCapabilities {
    pub validation: bool,
    pub reservation: bool,
    pub staging: bool,
    pub durable_finalization: bool,
    pub reads: bool,
    pub enumeration: bool,
    pub verification: bool,
    pub health: bool,
    pub reconciliation: bool,
    pub removal: bool,
}

impl BackendCapabilities {
    pub const fn complete() -> Self {
        Self {
            validation: true,
            reservation: true,
            staging: true,
            durable_finalization: true,
            reads: true,
            enumeration: true,
            verification: true,
            health: true,
            reconciliation: true,
            removal: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BackendObjectKey {
    pub object_id: String,
    pub version: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BackendObjectRecord {
    pub key: BackendObjectKey,
    pub size_bytes: u64,
    pub checksum: String,
    pub location: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BackendHealth {
    pub state: String,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackendError {
    Manifest(ObjectStoreManifestValidationError),
    Unsupported(&'static str),
    InvalidRequest(String),
    NotFound(String),
    Io(String),
}

impl Display for BackendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manifest(error) => write!(formatter, "invalid backend manifest: {error}"),
            Self::Unsupported(operation) => {
                write!(formatter, "backend does not support {operation}")
            }
            Self::InvalidRequest(message) => formatter.write_str(message),
            Self::NotFound(key) => write!(formatter, "backend object not found: {key}"),
            Self::Io(message) => write!(formatter, "backend I/O failed: {message}"),
        }
    }
}

impl std::error::Error for BackendError {}

pub trait ObjectStoreBackend {
    fn capabilities(&self) -> BackendCapabilities;

    fn validate_manifest(&self, manifest: &ObjectStoreManifest) -> Result<(), BackendError>;

    fn reserve(&mut self, reservation_id: &str, bytes: u64) -> Result<(), BackendError>;

    fn stage(
        &mut self,
        reservation_id: &str,
        key: &BackendObjectKey,
        source: &mut dyn Read,
    ) -> Result<BackendObjectRecord, BackendError>;

    /// Implementations must fsync/rename (or their equivalent) before this
    /// returns. Catalogue commit remains a daemon transaction after finalization.
    fn finalize(
        &mut self,
        staged: BackendObjectRecord,
    ) -> Result<BackendObjectRecord, BackendError>;

    fn read(&self, key: &BackendObjectKey) -> Result<Box<dyn Read + Send>, BackendError>;

    fn enumerate(&self, prefix: Option<&str>) -> Result<Vec<BackendObjectRecord>, BackendError>;

    fn verify(&self, key: &BackendObjectKey) -> Result<BackendObjectRecord, BackendError>;

    fn health(&self) -> Result<BackendHealth, BackendError>;

    fn reconcile(&mut self) -> Result<Vec<BackendObjectRecord>, BackendError>;

    fn remove(&mut self, key: &BackendObjectKey) -> Result<(), BackendError>;
}

#[cfg(test)]
mod tests {
    use super::{BackendCapabilities, ObjectStoreBackend};

    #[test]
    fn complete_capability_set_covers_every_contract_operation() {
        let capabilities = BackendCapabilities::complete();
        assert!(capabilities.validation);
        assert!(capabilities.reservation);
        assert!(capabilities.staging);
        assert!(capabilities.durable_finalization);
        assert!(capabilities.reads);
        assert!(capabilities.enumeration);
        assert!(capabilities.verification);
        assert!(capabilities.health);
        assert!(capabilities.reconciliation);
        assert!(capabilities.removal);

        fn accepts_contract<T: ObjectStoreBackend>() {}
        let _ = accepts_contract::<TestBackend>;
    }

    struct TestBackend;
    impl ObjectStoreBackend for TestBackend {
        fn capabilities(&self) -> BackendCapabilities {
            BackendCapabilities::default()
        }
        fn validate_manifest(
            &self,
            _manifest: &crate::manifest::ObjectStoreManifest,
        ) -> Result<(), super::BackendError> {
            Ok(())
        }
        fn reserve(
            &mut self,
            _reservation_id: &str,
            _bytes: u64,
        ) -> Result<(), super::BackendError> {
            Ok(())
        }
        fn stage(
            &mut self,
            _reservation_id: &str,
            _key: &super::BackendObjectKey,
            _source: &mut dyn std::io::Read,
        ) -> Result<super::BackendObjectRecord, super::BackendError> {
            Err(super::BackendError::Unsupported("stage"))
        }
        fn finalize(
            &mut self,
            staged: super::BackendObjectRecord,
        ) -> Result<super::BackendObjectRecord, super::BackendError> {
            Ok(staged)
        }
        fn read(
            &self,
            _key: &super::BackendObjectKey,
        ) -> Result<Box<dyn std::io::Read + Send>, super::BackendError> {
            Err(super::BackendError::Unsupported("read"))
        }
        fn enumerate(
            &self,
            _prefix: Option<&str>,
        ) -> Result<Vec<super::BackendObjectRecord>, super::BackendError> {
            Ok(Vec::new())
        }
        fn verify(
            &self,
            _key: &super::BackendObjectKey,
        ) -> Result<super::BackendObjectRecord, super::BackendError> {
            Err(super::BackendError::Unsupported("verify"))
        }
        fn health(&self) -> Result<super::BackendHealth, super::BackendError> {
            Ok(super::BackendHealth {
                state: "healthy".to_string(),
                message: None,
            })
        }
        fn reconcile(&mut self) -> Result<Vec<super::BackendObjectRecord>, super::BackendError> {
            Ok(Vec::new())
        }
        fn remove(&mut self, _key: &super::BackendObjectKey) -> Result<(), super::BackendError> {
            Ok(())
        }
    }
}
