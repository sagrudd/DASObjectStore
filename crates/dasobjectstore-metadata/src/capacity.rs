use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};
use std::path::{Path, PathBuf};

pub const DEFAULT_SSD_HIGH_WATERMARK_PERCENT: u8 = 85;
pub const DEFAULT_SSD_CRITICAL_WATERMARK_PERCENT: u8 = 95;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SsdCapacity {
    pub path: PathBuf,
    pub total_bytes: u64,
    pub available_bytes: u64,
}

impl SsdCapacity {
    pub fn new(path: impl Into<PathBuf>, total_bytes: u64, available_bytes: u64) -> Self {
        Self {
            path: path.into(),
            total_bytes,
            available_bytes,
        }
    }

    pub fn used_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.available_bytes)
    }

    pub fn used_percent_floor(&self) -> u8 {
        if self.total_bytes == 0 {
            return 0;
        }

        ((u128::from(self.used_bytes()) * 100) / u128::from(self.total_bytes)) as u8
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SsdCapacityPolicy {
    pub high_watermark_percent: u8,
    pub critical_watermark_percent: u8,
    pub minimum_free_bytes: u64,
}

impl SsdCapacityPolicy {
    pub fn new(
        high_watermark_percent: u8,
        critical_watermark_percent: u8,
        minimum_free_bytes: u64,
    ) -> Result<Self, SsdCapacityPolicyError> {
        let policy = Self {
            high_watermark_percent,
            critical_watermark_percent,
            minimum_free_bytes,
        };
        policy.validate()?;

        Ok(policy)
    }

    pub fn validate(&self) -> Result<(), SsdCapacityPolicyError> {
        if self.high_watermark_percent == 0 || self.high_watermark_percent >= 100 {
            return Err(SsdCapacityPolicyError::InvalidHighWatermark {
                high_watermark_percent: self.high_watermark_percent,
            });
        }

        if self.critical_watermark_percent <= self.high_watermark_percent
            || self.critical_watermark_percent > 100
        {
            return Err(SsdCapacityPolicyError::InvalidCriticalWatermark {
                high_watermark_percent: self.high_watermark_percent,
                critical_watermark_percent: self.critical_watermark_percent,
            });
        }

        Ok(())
    }

    pub fn evaluate(&self, capacity: &SsdCapacity) -> Result<SsdPressure, SsdCapacityPolicyError> {
        self.validate()?;

        if capacity.total_bytes == 0 {
            return Ok(SsdPressure::Critical);
        }

        if capacity.available_bytes < self.minimum_free_bytes
            || threshold_reached(capacity, self.critical_watermark_percent)
        {
            return Ok(SsdPressure::Critical);
        }

        if threshold_reached(capacity, self.high_watermark_percent) {
            return Ok(SsdPressure::HighWatermark);
        }

        Ok(SsdPressure::AcceptingWrites)
    }
}

impl Default for SsdCapacityPolicy {
    fn default() -> Self {
        Self {
            high_watermark_percent: DEFAULT_SSD_HIGH_WATERMARK_PERCENT,
            critical_watermark_percent: DEFAULT_SSD_CRITICAL_WATERMARK_PERCENT,
            minimum_free_bytes: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum SsdPressure {
    AcceptingWrites,
    HighWatermark,
    Critical,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SsdCapacityPolicyError {
    InvalidHighWatermark {
        high_watermark_percent: u8,
    },
    InvalidCriticalWatermark {
        high_watermark_percent: u8,
        critical_watermark_percent: u8,
    },
}

impl Display for SsdCapacityPolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidHighWatermark {
                high_watermark_percent,
            } => write!(
                formatter,
                "invalid SSD high watermark {high_watermark_percent}; expected 1..99"
            ),
            Self::InvalidCriticalWatermark {
                high_watermark_percent,
                critical_watermark_percent,
            } => write!(
                formatter,
                "invalid SSD critical watermark {critical_watermark_percent}; expected greater than high watermark {high_watermark_percent} and at most 100"
            ),
        }
    }
}

impl std::error::Error for SsdCapacityPolicyError {}

#[derive(Debug)]
pub enum SsdCapacityMeasurementError {
    InvalidPath(PathBuf),
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    UnsupportedPlatform,
}

impl Display for SsdCapacityMeasurementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath(path) => write!(
                formatter,
                "cannot measure SSD capacity for path with interior NUL byte: {}",
                path.to_string_lossy()
            ),
            Self::Io { path, source } => write!(
                formatter,
                "failed to measure SSD capacity for {}: {source}",
                path.to_string_lossy()
            ),
            Self::UnsupportedPlatform => {
                formatter.write_str("SSD capacity measurement is unsupported on this platform")
            }
        }
    }
}

impl std::error::Error for SsdCapacityMeasurementError {}

#[cfg(unix)]
pub fn measure_ssd_capacity(
    path: impl AsRef<Path>,
) -> Result<SsdCapacity, SsdCapacityMeasurementError> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let path = path.as_ref();
    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| SsdCapacityMeasurementError::InvalidPath(path.to_path_buf()))?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();

    let result = unsafe { libc::statvfs(c_path.as_ptr(), stats.as_mut_ptr()) };
    if result != 0 {
        return Err(SsdCapacityMeasurementError::Io {
            path: path.to_path_buf(),
            source: std::io::Error::last_os_error(),
        });
    }

    let stats = unsafe { stats.assume_init() };
    let block_size = statvfs_block_size(&stats);
    let total_bytes = u64::from(stats.f_blocks).saturating_mul(block_size);
    let available_bytes = u64::from(stats.f_bavail).saturating_mul(block_size);

    Ok(SsdCapacity::new(path, total_bytes, available_bytes))
}

#[cfg(not(unix))]
pub fn measure_ssd_capacity(
    _path: impl AsRef<Path>,
) -> Result<SsdCapacity, SsdCapacityMeasurementError> {
    Err(SsdCapacityMeasurementError::UnsupportedPlatform)
}

#[cfg(unix)]
fn statvfs_block_size(stats: &libc::statvfs) -> u64 {
    #[allow(clippy::unnecessary_cast)]
    let fragment_size = stats.f_frsize as u64;
    #[allow(clippy::unnecessary_cast)]
    let block_size = stats.f_bsize as u64;

    if fragment_size == 0 {
        block_size
    } else {
        fragment_size
    }
}

fn threshold_reached(capacity: &SsdCapacity, watermark_percent: u8) -> bool {
    u128::from(capacity.used_bytes()) * 100
        >= u128::from(capacity.total_bytes) * u128::from(watermark_percent)
}

#[cfg(test)]
mod tests {
    use super::{
        measure_ssd_capacity, SsdCapacity, SsdCapacityPolicy, SsdCapacityPolicyError, SsdPressure,
        DEFAULT_SSD_CRITICAL_WATERMARK_PERCENT, DEFAULT_SSD_HIGH_WATERMARK_PERCENT,
    };

    #[test]
    fn default_policy_uses_documented_watermarks() {
        let policy = SsdCapacityPolicy::default();

        assert_eq!(
            policy.high_watermark_percent,
            DEFAULT_SSD_HIGH_WATERMARK_PERCENT
        );
        assert_eq!(
            policy.critical_watermark_percent,
            DEFAULT_SSD_CRITICAL_WATERMARK_PERCENT
        );
        assert_eq!(policy.minimum_free_bytes, 0);
        policy.validate().expect("default policy is valid");
    }

    #[test]
    fn policy_marks_capacity_below_high_watermark_as_accepting_writes() {
        let policy = SsdCapacityPolicy::new(80, 95, 0).expect("policy");
        let capacity = SsdCapacity::new("/ssd", 1_000, 250);

        assert_eq!(
            policy.evaluate(&capacity).expect("evaluation"),
            SsdPressure::AcceptingWrites
        );
        assert_eq!(capacity.used_bytes(), 750);
        assert_eq!(capacity.used_percent_floor(), 75);
    }

    #[test]
    fn policy_marks_high_watermark_pressure() {
        let policy = SsdCapacityPolicy::new(80, 95, 0).expect("policy");
        let capacity = SsdCapacity::new("/ssd", 1_000, 200);

        assert_eq!(
            policy.evaluate(&capacity).expect("evaluation"),
            SsdPressure::HighWatermark
        );
    }

    #[test]
    fn policy_marks_critical_pressure() {
        let policy = SsdCapacityPolicy::new(80, 95, 0).expect("policy");
        let capacity = SsdCapacity::new("/ssd", 1_000, 50);

        assert_eq!(
            policy.evaluate(&capacity).expect("evaluation"),
            SsdPressure::Critical
        );
    }

    #[test]
    fn minimum_free_bytes_can_force_critical_pressure() {
        let policy = SsdCapacityPolicy::new(80, 95, 100).expect("policy");
        let capacity = SsdCapacity::new("/ssd", 1_000, 99);

        assert_eq!(
            policy.evaluate(&capacity).expect("evaluation"),
            SsdPressure::Critical
        );
    }

    #[test]
    fn rejects_invalid_watermarks() {
        assert_eq!(
            SsdCapacityPolicy::new(0, 95, 0).expect_err("high watermark invalid"),
            SsdCapacityPolicyError::InvalidHighWatermark {
                high_watermark_percent: 0
            }
        );
        assert_eq!(
            SsdCapacityPolicy::new(90, 90, 0).expect_err("critical watermark invalid"),
            SsdCapacityPolicyError::InvalidCriticalWatermark {
                high_watermark_percent: 90,
                critical_watermark_percent: 90,
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn measures_existing_temp_directory_capacity() {
        let capacity = measure_ssd_capacity(std::env::temp_dir()).expect("capacity measured");

        assert!(capacity.total_bytes > 0);
        assert!(capacity.available_bytes <= capacity.total_bytes);
    }
}
