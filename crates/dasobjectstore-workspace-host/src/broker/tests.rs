use super::*;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn fixture() -> (PathBuf, BrokerConfig, BranchPlan) {
    let root = std::env::temp_dir().join(format!(
        "dos-workspace-host-{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos(),
        FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).expect("root");
    let config = BrokerConfig {
        schema_version: 1,
        live_metadata_path: None,
        aggregate_root: Some(root.join("aggregates")),
        nfs_clients: BTreeMap::from([(
            "compute-a".to_string(),
            crate::ManagedNfsClient {
                address_or_cidr: "192.168.1.48".to_string(),
            },
        )]),
        disks: BTreeMap::from([(
            "disk-a".to_string(),
            crate::ManagedDiskRoot {
                root: root.clone(),
                workspace_directory: ".workspaces".to_string(),
            },
        )]),
    };
    let branch = BranchPlan {
        disk_id: "disk-a".to_string(),
        branch_id: "branch-a".to_string(),
        project_id: 1001,
        quota_bytes: 4096,
    };
    (root, config, branch)
}

#[test]
fn nfs_export_is_derived_from_registered_client_and_forces_root_squash() {
    let (root, config, _) = fixture();
    let export = NfsExportPlan {
        mount_identity: "workspace-a".to_string(),
        client_id: "compute-a".to_string(),
        access_mode: NfsAccessMode::ReadWrite,
    };
    let line = expected_export_line(&config, "workspace-a", &export).expect("export line");
    assert_eq!(
        line,
        format!(
            "{} 192.168.1.48(rw,sync,no_subtree_check,root_squash,secure,fsid={})\n",
            root.join("aggregates/workspace-a").display(),
            workspace_export_fsid("workspace-a", "compute-a")
        )
    );
    assert!(!line.contains("no_root_squash"));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn nfs_request_rejects_unregistered_client_and_mismatched_mount() {
    let (root, config, _) = fixture();
    for (mount_identity, client_id) in [("other", "compute-a"), ("workspace-a", "unknown")] {
        let request = BrokerRequest {
            protocol_version: PROTOCOL_VERSION,
            request_id: "request-nfs".to_string(),
            workspace_id: "workspace-a".to_string(),
            operation: WorkspaceHostOperation::InspectNfs {
                export: NfsExportPlan {
                    mount_identity: mount_identity.to_string(),
                    client_id: client_id.to_string(),
                    access_mode: NfsAccessMode::ReadOnly,
                },
            },
        };
        assert!(matches!(
            execute_request(&config, &request),
            Err(BrokerError::InvalidRequest(_))
        ));
    }
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn inspect_reports_absent_without_creating_paths() {
    let (root, config, branch) = fixture();
    let inspection = inspect_one(&config, "workspace-a", &branch, false).expect("inspect");
    assert_eq!(inspection.state, RecoveryState::Absent);
    assert!(!root.join(".workspaces").exists());
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn marker_conflict_and_nonempty_rollback_fail_closed() {
    let (root, config, branch) = fixture();
    let directory = root.join(".workspaces").join("branch-a");
    fs::create_dir_all(&directory).expect("branch");
    BranchMarker::expected("other-workspace", &branch)
        .create_exclusive(&directory)
        .expect("marker");
    assert!(matches!(
        rollback_one(&config, "workspace-a", &branch),
        Err(BrokerError::MarkerConflict(_))
    ));
    fs::remove_file(directory.join(crate::MARKER_FILE)).expect("remove marker");
    BranchMarker::expected("workspace-a", &branch)
        .create_exclusive(&directory)
        .expect("marker");
    fs::write(directory.join("payload"), b"data").expect("payload");
    assert!(matches!(
        rollback_one(&config, "workspace-a", &branch),
        Err(BrokerError::UnsafeEntry(_))
    ));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn cleanup_removes_only_exact_marker_owned_regular_tree() {
    let (root, config, branch) = fixture();
    let directory = root.join(".workspaces").join("branch-a");
    fs::create_dir_all(directory.join("nested")).expect("branch");
    BranchMarker::expected("workspace-a", &branch)
        .create_exclusive(&directory)
        .expect("marker");
    fs::write(directory.join("nested/result.bin"), b"result").expect("payload");
    cleanup_one(&config, "workspace-a", &branch).expect("cleanup");
    assert!(!directory.exists());
    fs::remove_dir_all(root).expect("fixture cleanup");
}

#[cfg(unix)]
#[test]
fn cleanup_preflight_rejects_symlink_without_removing_marker_or_data() {
    use std::os::unix::fs::symlink;
    let (root, config, branch) = fixture();
    let directory = root.join(".workspaces").join("branch-a");
    fs::create_dir_all(&directory).expect("branch");
    BranchMarker::expected("workspace-a", &branch)
        .create_exclusive(&directory)
        .expect("marker");
    fs::write(directory.join("kept.bin"), b"kept").expect("payload");
    symlink("/tmp", directory.join("unsafe")).expect("symlink");
    assert!(matches!(
        cleanup_one(&config, "workspace-a", &branch),
        Err(BrokerError::UnsafeEntry(_))
    ));
    assert!(directory.join(crate::MARKER_FILE).exists());
    assert!(directory.join("kept.bin").exists());
    fs::remove_dir_all(root).expect("fixture cleanup");
}

#[cfg(unix)]
#[test]
fn symlinked_branch_is_never_followed() {
    use std::os::unix::fs::symlink;
    let (root, config, branch) = fixture();
    fs::create_dir(root.join(".workspaces")).expect("namespace");
    symlink("/tmp", root.join(".workspaces/branch-a")).expect("symlink");
    let inspection = inspect_one(&config, "workspace-a", &branch, false).expect("inspect");
    assert_eq!(inspection.state, RecoveryState::UnsafeFilesystemEntry);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn invalid_or_unknown_identifiers_are_rejected_before_filesystem_work() {
    let (root, config, branch) = fixture();
    let request = BrokerRequest {
        protocol_version: PROTOCOL_VERSION,
        request_id: "request-a".to_string(),
        workspace_id: "../escape".to_string(),
        operation: WorkspaceHostOperation::Inspect {
            branches: vec![branch.clone()],
        },
    };
    assert!(matches!(
        execute_request(&config, &request),
        Err(BrokerError::InvalidRequest(_))
    ));
    let mut unknown = branch;
    unknown.disk_id = "unknown".to_string();
    let request = BrokerRequest {
        protocol_version: PROTOCOL_VERSION,
        request_id: "request-b".to_string(),
        workspace_id: "workspace-a".to_string(),
        operation: WorkspaceHostOperation::Inspect {
            branches: vec![unknown],
        },
    };
    assert!(matches!(
        execute_request(&config, &request),
        Err(BrokerError::InvalidRequest(_))
    ));
    assert!(!root.join(".workspaces").exists());
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn aggregate_inspection_is_path_redacted_and_marker_conflicts_fail_closed() {
    let (root, config, branch) = fixture();
    let aggregate = AggregatePlan {
        mount_identity: "workspace-a".to_string(),
        branches: vec![branch],
        minimum_free_bytes: 1024,
    };
    let absent =
        inspect_aggregate(&config, "workspace-a", &aggregate).expect("inspect absent aggregate");
    assert_eq!(absent.state, AggregateRecoveryState::Absent);
    assert_eq!(absent.mount_identity, "workspace-a");

    let target = config
        .aggregate_root
        .as_ref()
        .expect("aggregate root")
        .join("workspace-a");
    fs::create_dir_all(&target).expect("aggregate directory");
    let missing =
        inspect_aggregate(&config, "workspace-a", &aggregate).expect("inspect missing marker");
    assert_eq!(missing.state, AggregateRecoveryState::MarkerMissing);
    fs::write(
            target.join(AGGREGATE_MARKER_FILE),
            br#"{"schema":"wrong","workspace_id":"workspace-a","mount_identity":"workspace-a","branch_ids":["branch-a"],"minimum_free_bytes":1024}"#,
        )
        .expect("conflicting marker");
    let conflict = inspect_aggregate(&config, "workspace-a", &aggregate).expect("inspect conflict");
    assert_eq!(conflict.state, AggregateRecoveryState::MarkerConflict);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn aggregate_request_requires_configured_root_and_matching_identity() {
    let (root, mut config, branch) = fixture();
    config.aggregate_root = None;
    let request = BrokerRequest {
        protocol_version: PROTOCOL_VERSION,
        request_id: "request-a".to_string(),
        workspace_id: "workspace-a".to_string(),
        operation: WorkspaceHostOperation::InspectAggregate {
            aggregate: AggregatePlan {
                mount_identity: "different".to_string(),
                branches: vec![branch],
                minimum_free_bytes: 1024,
            },
        },
    };
    assert!(matches!(
        execute_request(&config, &request),
        Err(BrokerError::InvalidRequest(_))
    ));
    fs::remove_dir_all(root).expect("cleanup");
}

#[cfg(target_os = "linux")]
#[test]
fn mergerfs_process_identity_uses_trailing_target_and_preceding_sources() {
    let arguments = vec![
        "/usr/bin/mergerfs".to_string(),
        "-o".to_string(),
        "fsname=dasobjectstore-workspace-workspace-a,allow_other".to_string(),
        "/mnt/disk-a/branch:/mnt/disk-b/branch".to_string(),
        "/srv/dasobjectstore/workspaces/workspace-a".to_string(),
    ];
    assert_eq!(
        parse_mergerfs_process(
            &arguments,
            Path::new("/srv/dasobjectstore/workspaces/workspace-a")
        ),
        Some((
            "/mnt/disk-a/branch:/mnt/disk-b/branch".to_string(),
            "fsname=dasobjectstore-workspace-workspace-a,allow_other".to_string()
        ))
    );
    assert!(parse_mergerfs_process(&arguments, Path::new("/srv/another-workspace")).is_none());
}

#[cfg(not(target_os = "linux"))]
#[test]
fn quota_failure_rolls_back_new_empty_branch() {
    let (root, config, branch) = fixture();
    assert!(matches!(
        provision_one(&config, "workspace-a", &branch),
        Err(BrokerError::Unsupported(_))
    ));
    assert!(!root.join(".workspaces/branch-a").exists());
    fs::remove_dir_all(root).expect("cleanup");
}
