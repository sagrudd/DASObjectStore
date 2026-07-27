use super::*;

pub(super) fn exports_directory() -> PathBuf {
    PathBuf::from(EXPORTS_DIRECTORY)
}

pub(super) fn export_fragment_path(workspace_id: &str, client_id: &str) -> PathBuf {
    exports_directory().join(format!(
        "dasobjectstore-workspace-{workspace_id}-{client_id}.exports"
    ))
}

pub(super) fn expected_export_line(
    config: &BrokerConfig,
    workspace_id: &str,
    export: &NfsExportPlan,
) -> Result<String, BrokerError> {
    let target = aggregate_root(config)?.join(workspace_id);
    let client = config.nfs_clients.get(&export.client_id).ok_or_else(|| {
        BrokerError::InvalidRequest(format!("NFS client {} is not registered", export.client_id))
    })?;
    let access = match export.access_mode {
        NfsAccessMode::ReadOnly => "ro",
        NfsAccessMode::ReadWrite => "rw",
    };
    let fsid = workspace_export_fsid(workspace_id, &export.client_id);
    Ok(format!(
        "{} {}({access},sync,no_subtree_check,root_squash,secure,fsid={fsid})\n",
        target.display(),
        client.address_or_cidr
    ))
}

pub(super) fn workspace_export_fsid(workspace_id: &str, client_id: &str) -> u32 {
    let digest = Sha256::digest(format!("{workspace_id}\0{client_id}").as_bytes());
    u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]) & 0x7fff_ffff | 1
}

pub(super) fn inspect_nfs(
    config: &BrokerConfig,
    workspace_id: &str,
    export: &NfsExportPlan,
) -> Result<NfsExportInspection, BrokerError> {
    let target = aggregate_root(config)?.join(workspace_id);
    let aggregate_ready = mounted_mergerfs_entry(&target)?.is_some_and(|(_, options)| {
        options.split(',').any(|option| {
            option == format!("fsname=dasobjectstore-workspace-{}", export.mount_identity)
        })
    });
    let expected = expected_export_line(config, workspace_id, export)?;
    let path = export_fragment_path(workspace_id, &export.client_id);
    let (state, published, address_matches, access_mode_matches) = match fs::symlink_metadata(&path)
    {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (
            if aggregate_ready {
                NfsExportRecoveryState::Absent
            } else {
                NfsExportRecoveryState::AggregateUnavailable
            },
            false,
            false,
            false,
        ),
        Err(error) => return Err(BrokerError::Io("stat NFS export fragment", error)),
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => (
            NfsExportRecoveryState::UnsafeFilesystemEntry,
            false,
            false,
            false,
        ),
        Ok(_) => {
            let actual = fs::read_to_string(&path)
                .map_err(|error| BrokerError::Io("read NFS export fragment", error))?;
            let client = &config.nfs_clients[&export.client_id].address_or_cidr;
            let address_matches = actual.contains(&format!(" {client}("));
            let access = match export.access_mode {
                NfsAccessMode::ReadOnly => "ro",
                NfsAccessMode::ReadWrite => "rw",
            };
            let access_mode_matches = actual.contains(&format!("({access},"));
            (
                if actual == expected && aggregate_ready {
                    NfsExportRecoveryState::Ready
                } else if !aggregate_ready {
                    NfsExportRecoveryState::AggregateUnavailable
                } else {
                    NfsExportRecoveryState::FragmentConflict
                },
                actual == expected,
                address_matches,
                access_mode_matches,
            )
        }
    };
    Ok(NfsExportInspection {
        mount_identity: export.mount_identity.clone(),
        client_id: export.client_id.clone(),
        resolved_address_or_cidr: config.nfs_clients[&export.client_id]
            .address_or_cidr
            .clone(),
        state,
        published,
        root_squash: published,
        address_matches,
        access_mode_matches,
    })
}

pub(super) fn attach_nfs(
    config: &BrokerConfig,
    workspace_id: &str,
    export: &NfsExportPlan,
) -> Result<NfsExportInspection, BrokerError> {
    let before = inspect_nfs(config, workspace_id, export)?;
    if before.state == NfsExportRecoveryState::Ready {
        return Ok(before);
    }
    if before.state != NfsExportRecoveryState::Absent {
        return Err(BrokerError::UnsafeEntry(format!(
            "NFS export is not safely attachable: {:?}",
            before.state
        )));
    }
    let directory = exports_directory();
    ensure_real_directory(&directory)?;
    let path = export_fragment_path(workspace_id, &export.client_id);
    let temporary = directory.join(format!(
        ".dasobjectstore-workspace-{workspace_id}-{}.exports.{}.tmp",
        export.client_id,
        std::process::id(),
    ));
    let bytes = expected_export_line(config, workspace_id, export)?;
    write_new_synced_file(&temporary, bytes.as_bytes())?;
    if let Err(error) = fs::hard_link(&temporary, &path) {
        let _ = fs::remove_file(&temporary);
        return Err(BrokerError::Io(
            "publish NFS export fragment without replacement",
            error,
        ));
    }
    fs::remove_file(&temporary)
        .map_err(|error| BrokerError::Io("remove NFS export temporary", error))?;
    sync_directory(&directory)?;
    if let Err(error) = reload_exports() {
        let _ = fs::remove_file(&path);
        let _ = sync_directory(&directory);
        let _ = reload_exports();
        return Err(error);
    }
    let after = inspect_nfs(config, workspace_id, export)?;
    if after.state != NfsExportRecoveryState::Ready {
        return Err(BrokerError::UnsafeEntry(
            "NFS reload returned without the expected export becoming ready".to_string(),
        ));
    }
    Ok(after)
}

pub(super) fn detach_nfs(
    config: &BrokerConfig,
    workspace_id: &str,
    export: &NfsExportPlan,
) -> Result<NfsExportInspection, BrokerError> {
    let before = inspect_nfs(config, workspace_id, export)?;
    if before.state == NfsExportRecoveryState::Absent {
        return Ok(before);
    }
    if before.state != NfsExportRecoveryState::Ready {
        return Err(BrokerError::UnsafeEntry(
            "refusing to detach an NFS export whose identity is not proven".to_string(),
        ));
    }
    let directory = exports_directory();
    let path = export_fragment_path(workspace_id, &export.client_id);
    let retained = directory.join(format!(
        ".dasobjectstore-workspace-{workspace_id}-{}.exports.{}.detaching",
        export.client_id,
        std::process::id(),
    ));
    fs::rename(&path, &retained)
        .map_err(|error| BrokerError::Io("retain NFS export during detach", error))?;
    sync_directory(&directory)?;
    if let Err(error) = reload_exports() {
        let _ = fs::rename(&retained, &path);
        let _ = sync_directory(&directory);
        let _ = reload_exports();
        return Err(error);
    }
    fs::remove_file(&retained)
        .map_err(|error| BrokerError::Io("remove detached NFS export", error))?;
    sync_directory(&directory)?;
    inspect_nfs(config, workspace_id, export)
}

pub(super) fn write_new_synced_file(path: &Path, bytes: &[u8]) -> Result<(), BrokerError> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(path)
        .map_err(|error| BrokerError::Io("create NFS export fragment", error))?;
    file.write_all(bytes)
        .map_err(|error| BrokerError::Io("write NFS export fragment", error))?;
    file.sync_all()
        .map_err(|error| BrokerError::Io("sync NFS export fragment", error))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o644))
        .map_err(|error| BrokerError::Io("set NFS export fragment permissions", error))
}

pub(super) fn reload_exports() -> Result<(), BrokerError> {
    let status = Command::new("/usr/sbin/exportfs")
        .arg("-ra")
        .status()
        .map_err(|error| BrokerError::Io("reload NFS exports", error))?;
    if status.success() {
        Ok(())
    } else {
        Err(BrokerError::Unsupported(format!(
            "exportfs reload exited with {status}"
        )))
    }
}
