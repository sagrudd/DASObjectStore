use dasobjectstore_core::object_type::ObjectType;
use dasobjectstore_metadata::ObjectPutReport;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectPutRequest {
    pub object_id: String,
    pub source_path: PathBuf,
    pub ssd_root: PathBuf,
    pub disk_roots: Vec<String>,
    pub copies: u8,
    pub object_type: ObjectType,
}

impl ObjectPutRequest {
    pub fn validate(&self) -> Result<(), ObjectPutValidationError> {
        if self.object_id.trim().is_empty() {
            return Err(ObjectPutValidationError::BlankField { field: "object_id" });
        }
        if !self.source_path.is_absolute() {
            return Err(ObjectPutValidationError::RelativePath {
                field: "source_path",
                path: self.source_path.clone(),
            });
        }
        if !self.ssd_root.is_absolute() {
            return Err(ObjectPutValidationError::RelativePath {
                field: "ssd_root",
                path: self.ssd_root.clone(),
            });
        }
        if self.copies == 0 {
            return Err(ObjectPutValidationError::InvalidCopyCount);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectPutResponse {
    pub report: ObjectPutReport,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObjectPutValidationError {
    BlankField { field: &'static str },
    RelativePath { field: &'static str, path: PathBuf },
    InvalidCopyCount,
}

impl std::fmt::Display for ObjectPutValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BlankField { field } => write!(formatter, "{field} must not be blank"),
            Self::RelativePath { field, path } => {
                write!(formatter, "{field} must be absolute: {}", path.display())
            }
            Self::InvalidCopyCount => formatter.write_str("copies must be greater than zero"),
        }
    }
}

impl std::error::Error for ObjectPutValidationError {}

#[cfg(test)]
mod tests {
    use super::{ObjectPutRequest, ObjectPutValidationError};
    use dasobjectstore_core::object_type::ObjectType;

    fn request() -> ObjectPutRequest {
        ObjectPutRequest {
            object_id: "object-a".to_string(),
            source_path: "/tmp/source".into(),
            ssd_root: "/tmp/ssd".into(),
            disk_roots: vec!["disk-a=/tmp/disk-a".to_string()],
            copies: 1,
            object_type: ObjectType::Naive,
        }
    }

    #[test]
    fn rejects_relative_source_path() {
        let mut request = request();
        request.source_path = "relative/source".into();
        assert!(matches!(
            request.validate(),
            Err(ObjectPutValidationError::RelativePath {
                field: "source_path",
                ..
            })
        ));
    }

    #[test]
    fn rejects_zero_copies() {
        let mut request = request();
        request.copies = 0;
        assert_eq!(
            request.validate(),
            Err(ObjectPutValidationError::InvalidCopyCount)
        );
    }
}
