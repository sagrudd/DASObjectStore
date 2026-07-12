use super::*;

pub(super) struct ManagedHddDevice {
    pub(super) disk_id: DiskId,
    pub(super) root_path: PathBuf,
    pub(super) device_path: PathBuf,
}

pub(super) fn enforce_supported_das_for_store_create(
    args: &StoreCreateArgs,
) -> Result<(), CliError> {
    if args.registry_path().is_some() {
        return Ok(());
    }

    let managed_hdds = managed_hdd_devices(&default_hdd_root())?;
    let mut report = probe_current_platform()?;
    report.enclosures = group_enclosures(&report.disks);
    validate_managed_hdds_on_supported_das(&managed_hdds, &report)
}

fn managed_hdd_devices(hdd_root: &Path) -> Result<Vec<ManagedHddDevice>, CliError> {
    let roots = discover_managed_hdd_roots(hdd_root)?;
    let mut devices = Vec::new();

    for root in roots {
        let marker = read_device_marker(&root.root_path)?;
        let device_path = device_path_from_marker(&marker).ok_or_else(|| {
            CliError::CommandFailed(format!(
                "managed HDD {} at {} is missing device= in .dasobjectstore/device.env",
                root.disk_id,
                root.root_path.display()
            ))
        })?;
        devices.push(ManagedHddDevice {
            disk_id: root.disk_id,
            root_path: root.root_path,
            device_path: PathBuf::from(device_path),
        });
    }

    Ok(devices)
}

pub(super) fn validate_managed_hdds_on_supported_das(
    managed_hdds: &[ManagedHddDevice],
    report: &ProbeReport,
) -> Result<(), CliError> {
    if managed_hdds.is_empty() {
        return Err(CliError::CommandFailed(
            "object store creation requires at least one managed HDD on a supported, identifiable DAS enclosure; currently supported: QNAP TL-D800C".to_string(),
        ));
    }

    let supported_topology_paths = supported_das_topology_paths(report);
    if supported_topology_paths.is_empty() {
        return Err(CliError::CommandFailed(
            "object store creation requires supported, identifiable DAS enclosure mapping; no QNAP TL-D800C enclosure was detected in the current probe".to_string(),
        ));
    }

    for managed_hdd in managed_hdds {
        let Some(disk) = report
            .disks
            .iter()
            .find(|disk| probed_disk_matches_device(disk, &managed_hdd.device_path))
        else {
            return Err(CliError::CommandFailed(format!(
                "managed HDD {} at {} points to {}, but that device was not found in the current probe",
                managed_hdd.disk_id,
                managed_hdd.root_path.display(),
                managed_hdd.device_path.display()
            )));
        };

        let Some(topology_path) = disk.enclosure_topology_path.as_deref() else {
            return Err(CliError::CommandFailed(format!(
                "managed HDD {} at {} is not mapped to a supported DAS enclosure; currently supported: QNAP TL-D800C",
                managed_hdd.disk_id,
                managed_hdd.root_path.display()
            )));
        };

        if !supported_topology_paths.contains(topology_path) {
            return Err(CliError::CommandFailed(format!(
                "managed HDD {} at {} is mapped to unsupported enclosure topology {}; currently supported: QNAP TL-D800C",
                managed_hdd.disk_id,
                managed_hdd.root_path.display(),
                topology_path
            )));
        }
    }

    Ok(())
}

fn supported_das_topology_paths(report: &ProbeReport) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    for enclosure in &report.enclosures {
        if enclosure.identity.vendor_hint.as_deref() == Some("QNAP")
            && enclosure.identity.product_hint.as_deref() == Some("TL-D800C")
        {
            if let Some(topology_path) = enclosure.identity.usb_topology_path.as_deref() {
                paths.insert(format!("qnap-tl-d800c@{topology_path}"));
            }
        }
    }
    paths
}

fn probed_disk_matches_device(disk: &ObservedDisk, expected_device_path: &Path) -> bool {
    let Some(probed_path) = disk.device_path.as_deref() else {
        return false;
    };
    paths_refer_to_same_device(Path::new(probed_path), expected_device_path)
}

fn paths_refer_to_same_device(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn device_path_from_marker(marker: &str) -> Option<String> {
    marker
        .lines()
        .find_map(|line| line.strip_prefix("device=").map(ToOwned::to_owned))
}

pub(super) fn default_ssd_root() -> PathBuf {
    std::env::var_os("DASOBJECTSTORE_SSD_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/srv/dasobjectstore/ssd"))
}

pub(super) fn default_hdd_root() -> PathBuf {
    std::env::var_os("DASOBJECTSTORE_HDD_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/srv/dasobjectstore/hdd"))
}

pub(super) fn is_known_ssd_root(path: &Path) -> bool {
    read_device_marker(path).is_ok_and(|marker| marker.lines().any(|line| line == "role=ssd"))
}

pub(super) fn validate_known_ssd_root(path: &Path) -> Result<(), CliError> {
    let marker = read_device_marker(path).map_err(|err| {
        CliError::PortableRegistry(format!(
            "{} is not a known DASObjectStore SSD root: {err}",
            path.display()
        ))
    })?;
    if !marker.lines().any(|line| line == "role=ssd") {
        return Err(CliError::PortableRegistry(format!(
            "{} is not a DASObjectStore SSD root; expected role=ssd in .dasobjectstore/device.env",
            path.display()
        )));
    }

    Ok(())
}

fn read_device_marker(path: &Path) -> Result<String, std::io::Error> {
    fs::read_to_string(path.join(".dasobjectstore").join("device.env"))
}

pub(super) fn discover_managed_hdd_roots(hdd_root: &Path) -> Result<Vec<DiskCopyRoot>, CliError> {
    let mut roots = Vec::new();
    let entries = match fs::read_dir(hdd_root) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(roots),
        Err(err) => return Err(CliError::Io(err)),
    };

    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let root_path = entry.path();
        let marker = match read_device_marker(&root_path) {
            Ok(marker) => marker,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => return Err(CliError::Io(err)),
        };
        let Some(disk_id) = hdd_disk_id_from_marker(&marker)? else {
            continue;
        };
        roots.push(DiskCopyRoot::new(disk_id, root_path));
    }

    roots.sort_by(|left, right| left.disk_id.cmp(&right.disk_id));
    Ok(roots)
}

fn hdd_disk_id_from_marker(marker: &str) -> Result<Option<DiskId>, CliError> {
    for line in marker.lines() {
        let Some(role) = line.strip_prefix("role=") else {
            continue;
        };
        let Some(disk_id) = role.strip_prefix("hdd:") else {
            return Ok(None);
        };
        return DiskId::new(disk_id)
            .map(Some)
            .map_err(|err| CliError::CommandFailed(format!("invalid managed HDD marker: {err}")));
    }

    Ok(None)
}
