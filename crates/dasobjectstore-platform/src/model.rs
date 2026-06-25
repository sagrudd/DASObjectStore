use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum HostPlatform {
    Linux,
    Macos,
    Other(String),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProbeReport {
    pub platform: HostPlatform,
    pub disks: Vec<ObservedDisk>,
    pub enclosures: Vec<ObservedEnclosure>,
    pub warnings: Vec<ProbeWarning>,
}

impl ProbeReport {
    pub fn empty(platform: HostPlatform) -> Self {
        Self {
            platform,
            disks: Vec::new(),
            enclosures: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObservedDisk {
    pub device_path: Option<String>,
    pub size_bytes: Option<u64>,
    pub serial_hint: Option<String>,
    pub model_hint: Option<String>,
    pub partition_hints: Vec<PartitionHint>,
    pub filesystem_hints: Vec<FilesystemHint>,
    pub direct_attached_hint: Option<bool>,
    pub removable_hint: Option<bool>,
    pub transport: Transport,
    pub enclosure_topology_path: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObservedEnclosure {
    pub identity: EnclosureIdentity,
    pub disk_device_paths: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EnclosureIdentity {
    pub usb_topology_path: Option<String>,
    pub vendor_hint: Option<String>,
    pub product_hint: Option<String>,
    pub bridge_hint: Option<String>,
    pub user_assigned_name: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PartitionHint {
    pub name: Option<String>,
    pub size_bytes: Option<u64>,
    pub kind: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FilesystemHint {
    pub name: Option<String>,
    pub kind: Option<String>,
    pub mount_point: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Transport {
    Usb,
    Thunderbolt,
    Sata,
    Nvme,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProbeWarning {
    pub code: String,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::{HostPlatform, ProbeReport};

    #[test]
    fn creates_empty_probe_report_for_platform() {
        let report = ProbeReport::empty(HostPlatform::Macos);

        assert_eq!(report.platform, HostPlatform::Macos);
        assert!(report.disks.is_empty());
        assert!(report.enclosures.is_empty());
        assert!(report.warnings.is_empty());
    }
}
