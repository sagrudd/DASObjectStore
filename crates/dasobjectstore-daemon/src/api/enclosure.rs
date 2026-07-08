use crate::api::{DaemonJobAcceptedResponse, DaemonJobId, DaemonJobKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt::{self, Display};
use std::path::PathBuf;

pub const ENCLOSURE_PREPARE_CONFIRMATION: &str = "confirm prepare das";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PrepareEnclosureRequest {
    pub ssd_device: PathBuf,
    #[serde(default)]
    pub hdd_devices: Vec<PrepareEnclosureHddDevice>,
    pub mount_root: PathBuf,
    pub filesystem: PrepareEnclosureFilesystem,
    pub owner: Option<String>,
    pub dry_run: bool,
    pub client_request_id: Option<String>,
    pub administrator_actor: Option<String>,
    pub allow_format: bool,
    #[serde(default)]
    pub existing_data_acknowledged: bool,
    pub confirmation_marker: String,
}

impl PrepareEnclosureRequest {
    pub fn validate(&self) -> Result<(), PrepareEnclosureValidationError> {
        validate_absolute_path("ssd_device", &self.ssd_device)?;
        validate_absolute_path("mount_root", &self.mount_root)?;
        if self.hdd_devices.is_empty() {
            return Err(PrepareEnclosureValidationError::NoHddDevices);
        }
        if !self.allow_format {
            return Err(PrepareEnclosureValidationError::FormatNotAllowed);
        }
        if !self.existing_data_acknowledged {
            return Err(PrepareEnclosureValidationError::ExistingDataNotAcknowledged);
        }
        if self.confirmation_marker.trim() != ENCLOSURE_PREPARE_CONFIRMATION {
            return Err(PrepareEnclosureValidationError::ConfirmationMismatch);
        }
        validate_optional_safe_name("owner", self.owner.as_deref())?;
        validate_optional_safe_name("administrator_actor", self.administrator_actor.as_deref())?;
        if self
            .client_request_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(PrepareEnclosureValidationError::BlankClientRequestId);
        }

        let mut disk_ids = BTreeSet::new();
        let mut device_paths = BTreeSet::new();
        for hdd_device in &self.hdd_devices {
            validate_hdd_device(hdd_device)?;
            if !disk_ids.insert(hdd_device.disk_id.as_str()) {
                return Err(PrepareEnclosureValidationError::DuplicateHddDiskId {
                    disk_id: hdd_device.disk_id.clone(),
                });
            }
            if !device_paths.insert(hdd_device.device_path.as_path()) {
                return Err(PrepareEnclosureValidationError::DuplicateHddDevicePath {
                    device_path: hdd_device.device_path.clone(),
                });
            }
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PrepareEnclosureHddDevice {
    pub disk_id: String,
    pub device_path: PathBuf,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrepareEnclosureFilesystem {
    Ext4,
    Xfs,
}

impl Display for PrepareEnclosureFilesystem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ext4 => formatter.write_str("ext4"),
            Self::Xfs => formatter.write_str("xfs"),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PrepareEnclosureResponse {
    pub accepted: DaemonJobAcceptedResponse,
    pub ssd_device: PathBuf,
    pub hdd_devices: Vec<PrepareEnclosureHddDevice>,
    pub mount_root: PathBuf,
    pub filesystem: PrepareEnclosureFilesystem,
    pub owner: Option<String>,
    pub administrator_actor: Option<String>,
}

impl PrepareEnclosureResponse {
    #[allow(clippy::too_many_arguments)]
    pub fn accepted(
        job_id: DaemonJobId,
        accepted_at_utc: impl Into<String>,
        dry_run: bool,
        ssd_device: PathBuf,
        hdd_devices: Vec<PrepareEnclosureHddDevice>,
        mount_root: PathBuf,
        filesystem: PrepareEnclosureFilesystem,
        owner: Option<String>,
        administrator_actor: Option<String>,
    ) -> Self {
        Self {
            accepted: DaemonJobAcceptedResponse {
                job_id,
                kind: DaemonJobKind::EnclosurePreparation,
                accepted_at_utc: accepted_at_utc.into(),
                dry_run,
            },
            ssd_device,
            hdd_devices,
            mount_root,
            filesystem,
            owner,
            administrator_actor,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PrepareEnclosureValidationError {
    RelativePath { field: &'static str, path: PathBuf },
    NoHddDevices,
    BlankHddDiskId,
    UnsafeName { field: &'static str, value: String },
    DuplicateHddDiskId { disk_id: String },
    DuplicateHddDevicePath { device_path: PathBuf },
    FormatNotAllowed,
    ExistingDataNotAcknowledged,
    ConfirmationMismatch,
    BlankClientRequestId,
}

impl Display for PrepareEnclosureValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RelativePath { field, path } => {
                write!(
                    formatter,
                    "{field} must be an absolute path: {}",
                    path.display()
                )
            }
            Self::NoHddDevices => formatter.write_str("at least one HDD device is required"),
            Self::BlankHddDiskId => formatter.write_str("hdd disk_id must not be blank"),
            Self::UnsafeName { field, value } => write!(
                formatter,
                "{field} must be a conservative POSIX-style local name: {value}"
            ),
            Self::DuplicateHddDiskId { disk_id } => {
                write!(formatter, "duplicate hdd disk_id: {disk_id}")
            }
            Self::DuplicateHddDevicePath { device_path } => {
                write!(
                    formatter,
                    "duplicate hdd device path: {}",
                    device_path.display()
                )
            }
            Self::FormatNotAllowed => {
                formatter.write_str("allow_format must be true for enclosure preparation")
            }
            Self::ExistingDataNotAcknowledged => formatter
                .write_str("existing_data_acknowledged must be true for enclosure preparation"),
            Self::ConfirmationMismatch => write!(
                formatter,
                "confirmation_marker must exactly match \"{ENCLOSURE_PREPARE_CONFIRMATION}\""
            ),
            Self::BlankClientRequestId => {
                formatter.write_str("client_request_id must not be blank")
            }
        }
    }
}

impl std::error::Error for PrepareEnclosureValidationError {}

fn validate_hdd_device(
    device: &PrepareEnclosureHddDevice,
) -> Result<(), PrepareEnclosureValidationError> {
    if device.disk_id.trim().is_empty() {
        return Err(PrepareEnclosureValidationError::BlankHddDiskId);
    }
    validate_optional_safe_name("disk_id", Some(&device.disk_id))?;
    validate_absolute_path("hdd_device", &device.device_path)
}

fn validate_absolute_path(
    field: &'static str,
    path: &PathBuf,
) -> Result<(), PrepareEnclosureValidationError> {
    if !path.is_absolute() {
        return Err(PrepareEnclosureValidationError::RelativePath {
            field,
            path: path.clone(),
        });
    }
    Ok(())
}

fn validate_optional_safe_name(
    field: &'static str,
    value: Option<&str>,
) -> Result<(), PrepareEnclosureValidationError> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.trim().is_empty() || !is_safe_posixish_name(value) {
        return Err(PrepareEnclosureValidationError::UnsafeName {
            field,
            value: value.to_string(),
        });
    }
    Ok(())
}

fn is_safe_posixish_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 || !value.is_ascii() {
        return false;
    }

    let first = bytes[0];
    if !(first == b'_' || first.is_ascii_lowercase()) {
        return false;
    }

    bytes[1..].iter().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-'
    })
}

#[cfg(test)]
mod tests {
    use super::{
        PrepareEnclosureFilesystem, PrepareEnclosureHddDevice, PrepareEnclosureRequest,
        PrepareEnclosureResponse, PrepareEnclosureValidationError, ENCLOSURE_PREPARE_CONFIRMATION,
    };
    use crate::api::{DaemonJobId, DaemonJobKind};
    use std::path::PathBuf;

    fn valid_request() -> PrepareEnclosureRequest {
        PrepareEnclosureRequest {
            ssd_device: PathBuf::from("/dev/disk/by-id/nvme-ssd"),
            hdd_devices: vec![PrepareEnclosureHddDevice {
                disk_id: "qnap-1057".to_string(),
                device_path: PathBuf::from("/dev/disk/by-id/usb-qnap-1057"),
            }],
            mount_root: PathBuf::from("/srv/dasobjectstore"),
            filesystem: PrepareEnclosureFilesystem::Ext4,
            owner: Some("stephen".to_string()),
            dry_run: false,
            client_request_id: Some("request-1".to_string()),
            administrator_actor: Some("operator".to_string()),
            allow_format: true,
            existing_data_acknowledged: true,
            confirmation_marker: ENCLOSURE_PREPARE_CONFIRMATION.to_string(),
        }
    }

    #[test]
    fn request_serializes_with_stable_filesystem_case() {
        let encoded = serde_json::to_value(valid_request()).expect("request serializes");

        assert_eq!(encoded["filesystem"], "ext4");
        assert_eq!(encoded["hdd_devices"][0]["disk_id"], "qnap-1057");
    }

    #[test]
    fn validates_supported_enclosure_preparation_request() {
        valid_request().validate().expect("valid request");
    }

    #[test]
    fn rejects_relative_ssd_device() {
        let request = PrepareEnclosureRequest {
            ssd_device: PathBuf::from("nvme0n1"),
            ..valid_request()
        };

        assert!(matches!(
            request.validate(),
            Err(PrepareEnclosureValidationError::RelativePath {
                field: "ssd_device",
                ..
            })
        ));
    }

    #[test]
    fn rejects_missing_format_allowance() {
        let request = PrepareEnclosureRequest {
            allow_format: false,
            ..valid_request()
        };

        assert_eq!(
            request.validate(),
            Err(PrepareEnclosureValidationError::FormatNotAllowed)
        );
    }

    #[test]
    fn rejects_missing_existing_data_acknowledgement() {
        let request = PrepareEnclosureRequest {
            existing_data_acknowledged: false,
            ..valid_request()
        };

        assert_eq!(
            request.validate(),
            Err(PrepareEnclosureValidationError::ExistingDataNotAcknowledged)
        );
    }

    #[test]
    fn rejects_confirmation_mismatch() {
        let request = PrepareEnclosureRequest {
            confirmation_marker: "wrong".to_string(),
            ..valid_request()
        };

        assert_eq!(
            request.validate(),
            Err(PrepareEnclosureValidationError::ConfirmationMismatch)
        );
    }

    #[test]
    fn rejects_duplicate_hdd_disk_id() {
        let request = PrepareEnclosureRequest {
            hdd_devices: vec![
                PrepareEnclosureHddDevice {
                    disk_id: "qnap-1057".to_string(),
                    device_path: PathBuf::from("/dev/disk/by-id/usb-qnap-1057"),
                },
                PrepareEnclosureHddDevice {
                    disk_id: "qnap-1057".to_string(),
                    device_path: PathBuf::from("/dev/disk/by-id/usb-qnap-1058"),
                },
            ],
            ..valid_request()
        };

        assert!(matches!(
            request.validate(),
            Err(PrepareEnclosureValidationError::DuplicateHddDiskId { .. })
        ));
    }

    #[test]
    fn response_uses_enclosure_preparation_job_kind() {
        let response = PrepareEnclosureResponse::accepted(
            DaemonJobId::new("enclosure-prepare-1").expect("job id"),
            "2026-07-08T19:40:00Z",
            false,
            PathBuf::from("/dev/disk/by-id/nvme-ssd"),
            vec![PrepareEnclosureHddDevice {
                disk_id: "qnap-1057".to_string(),
                device_path: PathBuf::from("/dev/disk/by-id/usb-qnap-1057"),
            }],
            PathBuf::from("/srv/dasobjectstore"),
            PrepareEnclosureFilesystem::Ext4,
            Some("stephen".to_string()),
            Some("operator".to_string()),
        );

        assert_eq!(response.accepted.kind, DaemonJobKind::EnclosurePreparation);
        assert_eq!(response.hdd_devices.len(), 1);
    }
}
