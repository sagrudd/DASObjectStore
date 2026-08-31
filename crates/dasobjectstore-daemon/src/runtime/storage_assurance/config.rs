use super::StorageAssuranceError;
use std::path::{Path, PathBuf};

pub const DEFAULT_ASSURANCE_POLL_SECONDS: u64 = 30;
pub const DEFAULT_ASSURANCE_IDLE_GRACE_SECONDS: u64 = 10 * 60;
/// Nine weeks keeps every settled placement within the requested eight-to-ten
/// week physical-movement and checksum-verification window when a safe target
/// disk exists. If replication leaves no eligible target, housekeeping
/// re-hashes the source instead of weakening copy separation.
pub const DEFAULT_ASSURANCE_VERIFY_AFTER_SECONDS: u64 = 9 * 7 * 24 * 60 * 60;
pub const DEFAULT_ASSURANCE_IMBALANCE_BASIS_POINTS: u16 = 500;
pub const DEFAULT_ASSURANCE_MAX_OBJECT_BYTES: u64 = 128 * 1024 * 1024 * 1024;
pub const DEFAULT_ASSURANCE_IDLE_IO_BYTES_PER_SECOND: u64 = 1024 * 1024;
pub const DEFAULT_DISK_HOUSEKEEPING_IO_PERCENT: u8 = 10;
/// The conservative appliance commissioning baseline. The effective
/// housekeeping limit is always a percentage of this value and deployments
/// should replace it with their measured available IO throughput.
pub const DEFAULT_DISK_HOUSEKEEPING_AVAILABLE_IO_BYTES_PER_SECOND: u64 = 100 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageAssuranceConfig {
    pub enabled: bool,
    pub poll_seconds: u64,
    pub idle_grace_seconds: u64,
    pub verify_after_seconds: u64,
    pub imbalance_basis_points: u16,
    pub max_object_bytes: u64,
    pub idle_io_bytes_per_second: u64,
    pub available_io_bytes_per_second: u64,
    pub io_percent: u8,
    pub live_sqlite_path: PathBuf,
    pub hdd_root: PathBuf,
    pub latest_report_path: PathBuf,
    pub operation_journal_path: PathBuf,
}

impl StorageAssuranceConfig {
    pub fn from_environment(state_dir: &Path) -> Result<Self, StorageAssuranceError> {
        let ssd_root = std::env::var_os("DASOBJECTSTORE_SSD_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/srv/dasobjectstore/ssd"));
        let hdd_root = std::env::var_os("DASOBJECTSTORE_HDD_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/srv/dasobjectstore/hdd"));
        let config = Self {
            enabled: env_bool_with_legacy(
                "DASOBJECTSTORE_DISK_HOUSEKEEPING_ENABLED",
                "DASOBJECTSTORE_ASSURANCE_ENABLED",
                true,
            )?,
            poll_seconds: env_u64_with_legacy(
                "DASOBJECTSTORE_DISK_HOUSEKEEPING_POLL_SECONDS",
                "DASOBJECTSTORE_ASSURANCE_POLL_SECONDS",
                DEFAULT_ASSURANCE_POLL_SECONDS,
            )?,
            idle_grace_seconds: env_u64_with_legacy(
                "DASOBJECTSTORE_DISK_HOUSEKEEPING_IDLE_GRACE_SECONDS",
                "DASOBJECTSTORE_ASSURANCE_IDLE_GRACE_SECONDS",
                DEFAULT_ASSURANCE_IDLE_GRACE_SECONDS,
            )?,
            verify_after_seconds: env_u64_with_legacy(
                "DASOBJECTSTORE_DISK_HOUSEKEEPING_ROTATE_AFTER_SECONDS",
                "DASOBJECTSTORE_ASSURANCE_VERIFY_AFTER_SECONDS",
                DEFAULT_ASSURANCE_VERIFY_AFTER_SECONDS,
            )?,
            imbalance_basis_points: u16::try_from(env_u64_with_legacy(
                "DASOBJECTSTORE_DISK_HOUSEKEEPING_IMBALANCE_BASIS_POINTS",
                "DASOBJECTSTORE_ASSURANCE_IMBALANCE_BASIS_POINTS",
                u64::from(DEFAULT_ASSURANCE_IMBALANCE_BASIS_POINTS),
            )?)
            .map_err(|_| {
                StorageAssuranceError::InvalidConfiguration(
                    "imbalance basis points exceed u16".to_string(),
                )
            })?,
            max_object_bytes: env_u64_with_legacy(
                "DASOBJECTSTORE_DISK_HOUSEKEEPING_MAX_OBJECT_BYTES",
                "DASOBJECTSTORE_ASSURANCE_MAX_OBJECT_BYTES",
                DEFAULT_ASSURANCE_MAX_OBJECT_BYTES,
            )?,
            idle_io_bytes_per_second: env_u64_with_legacy(
                "DASOBJECTSTORE_DISK_HOUSEKEEPING_IDLE_IO_BYTES_PER_SECOND",
                "DASOBJECTSTORE_ASSURANCE_IDLE_IO_BYTES_PER_SECOND",
                DEFAULT_ASSURANCE_IDLE_IO_BYTES_PER_SECOND,
            )?,
            available_io_bytes_per_second: env_u64(
                "DASOBJECTSTORE_DISK_HOUSEKEEPING_AVAILABLE_IO_BYTES_PER_SECOND",
                DEFAULT_DISK_HOUSEKEEPING_AVAILABLE_IO_BYTES_PER_SECOND,
            )?,
            io_percent: u8::try_from(env_u64(
                "DASOBJECTSTORE_DISK_HOUSEKEEPING_IO_PERCENT",
                u64::from(DEFAULT_DISK_HOUSEKEEPING_IO_PERCENT),
            )?)
            .map_err(|_| {
                StorageAssuranceError::InvalidConfiguration(
                    "disk housekeeping IO percentage exceeds u8".to_string(),
                )
            })?,
            live_sqlite_path: ssd_root.join(".dasobjectstore/live.sqlite"),
            hdd_root,
            latest_report_path: state_dir.join("disk_housekeeping/latest.json"),
            operation_journal_path: state_dir.join("disk_housekeeping/operation.json"),
        };
        config.validate()?;
        Ok(config)
    }

    pub(super) fn validate(&self) -> Result<(), StorageAssuranceError> {
        if self.poll_seconds == 0
            || self.idle_grace_seconds < self.poll_seconds
            || self.verify_after_seconds == 0
            || self.max_object_bytes == 0
            || self.available_io_bytes_per_second == 0
            || self.io_percent == 0
            || self.io_percent > DEFAULT_DISK_HOUSEKEEPING_IO_PERCENT
            || self.imbalance_basis_points > 10_000
        {
            return Err(StorageAssuranceError::InvalidConfiguration(
                "poll must be non-zero, idle grace must cover one poll, rotation/max size and available IO must be non-zero, IO percentage must be 1..=10, and imbalance must be <=10000 basis points".to_string(),
            ));
        }
        Ok(())
    }
}

fn env_u64(name: &str, default: u64) -> Result<u64, StorageAssuranceError> {
    match std::env::var(name) {
        Ok(value) => value.parse().map_err(|_| {
            StorageAssuranceError::InvalidConfiguration(format!("{name} must be an integer"))
        }),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(StorageAssuranceError::InvalidConfiguration(format!(
            "{name}: {error}"
        ))),
    }
}

fn env_u64_with_legacy(
    name: &str,
    legacy_name: &str,
    default: u64,
) -> Result<u64, StorageAssuranceError> {
    match std::env::var(name) {
        Ok(value) => value.parse().map_err(|_| {
            StorageAssuranceError::InvalidConfiguration(format!("{name} must be an integer"))
        }),
        Err(std::env::VarError::NotPresent) => env_u64(legacy_name, default),
        Err(error) => Err(StorageAssuranceError::InvalidConfiguration(format!(
            "{name}: {error}"
        ))),
    }
}

fn env_bool(name: &str, default: bool) -> Result<bool, StorageAssuranceError> {
    match std::env::var(name) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(true),
            "0" | "false" | "no" | "off" => Ok(false),
            _ => Err(StorageAssuranceError::InvalidConfiguration(format!(
                "{name} must be true or false"
            ))),
        },
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(StorageAssuranceError::InvalidConfiguration(format!(
            "{name}: {error}"
        ))),
    }
}

fn env_bool_with_legacy(
    name: &str,
    legacy_name: &str,
    default: bool,
) -> Result<bool, StorageAssuranceError> {
    match std::env::var(name) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(true),
            "0" | "false" | "no" | "off" => Ok(false),
            _ => Err(StorageAssuranceError::InvalidConfiguration(format!(
                "{name} must be true or false"
            ))),
        },
        Err(std::env::VarError::NotPresent) => env_bool(legacy_name, default),
        Err(error) => Err(StorageAssuranceError::InvalidConfiguration(format!(
            "{name}: {error}"
        ))),
    }
}
