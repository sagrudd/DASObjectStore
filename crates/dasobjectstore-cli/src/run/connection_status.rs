//! Host connection probing, assessment, and operator recommendations.

use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HostConnectionStatus {
    pub(super) platform: HostPlatform,
    pub(super) disks: Vec<DiskConnectionStatus>,
    pub(super) warnings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct DiskConnectionStatus {
    pub(super) device_path: Option<String>,
    pub(super) model_hint: Option<String>,
    pub(super) size_bytes: Option<u64>,
    pub(super) transport: Transport,
    pub(super) direct_attached_hint: Option<bool>,
    pub(super) removable_hint: Option<bool>,
    pub(super) enclosure_topology_path: Option<String>,
    pub(super) assessment: ConnectionAssessment,
    pub(super) warnings: Vec<String>,
    pub(super) recommendation: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ConnectionAssessment {
    Good,
    Warning,
    Unknown,
}

impl ConnectionAssessment {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Good => "good",
            Self::Warning => "warning",
            Self::Unknown => "unknown",
        }
    }
}

impl DiskConnectionStatus {
    fn from_observed(disk: &ObservedDisk, preferred: Option<&PreferredConnectionPath>) -> Self {
        let mut warnings = Vec::new();
        let assessment = match disk.transport {
            Transport::Usb => {
                warnings.push(
                    "USB-attached DAS detected; this probe cannot verify negotiated USB link speed. Use a fast USB-C, USB 3.x, USB4, or Thunderbolt path because slow USB links will reduce ingest, destage, and object-service performance."
                        .to_string(),
                );
                ConnectionAssessment::Warning
            }
            Transport::Thunderbolt | Transport::Sata | Transport::Nvme => {
                ConnectionAssessment::Good
            }
            Transport::Unknown => {
                warnings.push(
                    "Disk transport is unknown; verify the DAS is not connected through a slow USB hub or fallback cable."
                        .to_string(),
                );
                ConnectionAssessment::Unknown
            }
        };
        let recommendation = connection_recommendation(disk, assessment, preferred);

        Self {
            device_path: disk.device_path.clone(),
            model_hint: disk.model_hint.clone(),
            size_bytes: disk.size_bytes,
            transport: disk.transport,
            direct_attached_hint: disk.direct_attached_hint,
            removable_hint: disk.removable_hint,
            enclosure_topology_path: disk.enclosure_topology_path.clone(),
            assessment,
            warnings,
            recommendation,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PreferredConnectionPath {
    device_path: Option<String>,
    transport: Transport,
    enclosure_topology_path: Option<String>,
}

pub(super) fn read_current_platform_connection_status() -> Result<HostConnectionStatus, CliError> {
    let mut probe = probe_current_platform()?;
    probe.enclosures = group_enclosures(&probe.disks);

    Ok(connection_status_from_probe(&probe))
}

pub(super) fn connection_status_from_probe(probe: &ProbeReport) -> HostConnectionStatus {
    let preferred = preferred_connection_path(&probe.disks);
    let disks: Vec<DiskConnectionStatus> = probe
        .disks
        .iter()
        .map(|disk| DiskConnectionStatus::from_observed(disk, preferred.as_ref()))
        .collect();
    let warnings: Vec<String> = probe
        .warnings
        .iter()
        .map(|warning| format!("{}: {}", warning.code, warning.message))
        .collect();

    HostConnectionStatus {
        platform: probe.platform.clone(),
        disks,
        warnings,
    }
}

fn preferred_connection_path(disks: &[ObservedDisk]) -> Option<PreferredConnectionPath> {
    disks
        .iter()
        .find(|disk| disk.transport == Transport::Thunderbolt)
        .map(|disk| PreferredConnectionPath {
            device_path: disk.device_path.clone(),
            transport: disk.transport,
            enclosure_topology_path: disk.enclosure_topology_path.clone(),
        })
}

fn connection_recommendation(
    disk: &ObservedDisk,
    assessment: ConnectionAssessment,
    preferred: Option<&PreferredConnectionPath>,
) -> Option<String> {
    if assessment == ConnectionAssessment::Good {
        return None;
    }

    if let Some(preferred) = preferred {
        if disk.device_path != preferred.device_path {
            return Some(format!(
                "Prefer the observed {} path used by {}{} for DAS workloads.",
                transport_label(preferred.transport),
                preferred
                    .device_path
                    .as_deref()
                    .unwrap_or("<unknown device>"),
                topology_suffix(preferred.enclosure_topology_path.as_deref())
            ));
        }
    }

    Some(
        "No faster attached DAS path is visible in this probe; move the DAS directly to a host USB-C, USB4, or Thunderbolt port and avoid hubs or fallback cables."
            .to_string(),
    )
}

fn transport_label(transport: Transport) -> &'static str {
    match transport {
        Transport::Usb => "USB",
        Transport::Thunderbolt => "Thunderbolt",
        Transport::Sata => "SATA",
        Transport::Nvme => "NVMe",
        Transport::Unknown => "unknown",
    }
}

fn topology_suffix(topology: Option<&str>) -> String {
    topology
        .map(|value| format!(" at topology {value}"))
        .unwrap_or_default()
}
