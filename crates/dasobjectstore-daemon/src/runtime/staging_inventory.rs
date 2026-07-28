//! Path-free accounting for daemon-owned transient storage.
//!
//! The inventory is deliberately separate from reclamation. It may classify
//! bytes as unsafe or unknown, but it never mutates a managed path.

use super::{
    profile_binding_registry_path, read_profile_bindings, run_garbage_collection,
    GarbageCollectDecision, GarbageCollectKind, GarbageCollectMode, GarbageCollectTrigger,
    GarbageCollectorConfig,
};
use crate::api::{
    StagingByteDisposition, StagingInventory, StagingInventoryCoverage, StagingInventoryGroup,
    StagingRetentionReason, StagingRootKind, STAGING_INVENTORY_SCHEMA_VERSION,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const MAX_GROUPS: usize = 64;

#[derive(Default)]
struct InventoryBuilder {
    groups: BTreeMap<
        (
            StagingRootKind,
            StagingByteDisposition,
            StagingRetentionReason,
        ),
        (u64, u64),
    >,
    observed_bytes: u64,
    unaccounted_bytes: u64,
    omitted_items: u64,
    partial: bool,
    unavailable: bool,
}

impl InventoryBuilder {
    fn record(
        &mut self,
        root: StagingRootKind,
        disposition: StagingByteDisposition,
        reason: StagingRetentionReason,
        bytes: u64,
    ) {
        self.observed_bytes = self.observed_bytes.saturating_add(bytes);
        let entry = self.groups.entry((root, disposition, reason)).or_default();
        entry.0 = entry.0.saturating_add(1);
        entry.1 = entry.1.saturating_add(bytes);
    }

    fn unknown(&mut self, root: StagingRootKind, bytes: u64) {
        self.partial = true;
        self.record(
            root,
            StagingByteDisposition::Unknown,
            StagingRetentionReason::UnsafeEntry,
            bytes,
        );
    }

    fn failed_root(&mut self, root: StagingRootKind) {
        self.partial = true;
        self.record(
            root,
            StagingByteDisposition::Unknown,
            StagingRetentionReason::ScanUnavailable,
            0,
        );
    }

    fn finish(mut self, generated_at_utc: &str) -> StagingInventory {
        let mut groups = self
            .groups
            .into_iter()
            .map(
                |((root_kind, disposition, reason), (items, bytes))| StagingInventoryGroup {
                    root_kind,
                    disposition,
                    reason,
                    items,
                    bytes,
                },
            )
            .collect::<Vec<_>>();
        if groups.len() > MAX_GROUPS {
            self.partial = true;
            self.omitted_items = self
                .omitted_items
                .saturating_add((groups.len() - MAX_GROUPS) as u64);
            groups.truncate(MAX_GROUPS);
        }
        let accounted_bytes = groups
            .iter()
            .map(|group| group.bytes)
            .sum::<u64>()
            .min(self.observed_bytes);
        self.unaccounted_bytes = self
            .unaccounted_bytes
            .saturating_add(self.observed_bytes.saturating_sub(accounted_bytes));
        StagingInventory {
            schema_version: STAGING_INVENTORY_SCHEMA_VERSION.to_string(),
            generated_at_utc: generated_at_utc.to_string(),
            coverage: if self.unavailable && self.observed_bytes == 0 {
                StagingInventoryCoverage::Unavailable
            } else if self.unavailable
                || self.partial
                || self.unaccounted_bytes != 0
                || self.omitted_items != 0
            {
                StagingInventoryCoverage::Partial
            } else {
                StagingInventoryCoverage::Complete
            },
            observed_bytes: self.observed_bytes,
            accounted_bytes,
            unaccounted_bytes: self.unaccounted_bytes,
            omitted_items: self.omitted_items,
            groups,
        }
    }
}

/// Build a current, mutation-free staging inventory.
pub fn build_staging_inventory(
    config: &GarbageCollectorConfig,
    generated_at_utc: &str,
    now: SystemTime,
) -> StagingInventory {
    let mut builder = InventoryBuilder::default();
    match run_garbage_collection(
        config,
        GarbageCollectMode::Inventory,
        GarbageCollectTrigger::Scheduled,
        "staging-inventory",
        generated_at_utc,
        now,
    ) {
        Ok(report) => {
            for item in report.items {
                builder.record(
                    root_for_general_kind(item.kind),
                    disposition_for_gc(item.decision, &item.reason),
                    typed_reason(&item.reason),
                    item.bytes,
                );
            }
            if builder
                .groups
                .values()
                .map(|(items, _)| *items)
                .sum::<u64>()
                >= config.maximum_items_per_run as u64
            {
                builder.partial = true;
                builder.omitted_items = builder.omitted_items.saturating_add(1);
                builder.record(
                    StagingRootKind::IngestJob,
                    StagingByteDisposition::Unknown,
                    StagingRetentionReason::InventoryLimitReached,
                    0,
                );
            }
        }
        Err(_) => {
            builder.unavailable = true;
            builder.failed_root(StagingRootKind::IngestJob);
            builder.failed_root(StagingRootKind::PerformanceTest);
            builder.failed_root(StagingRootKind::DirectS3Multipart);
        }
    }

    scan_reconciliation(config, &mut builder);
    scan_direct_s3_uploads(config, &mut builder);
    scan_direct_profile_staging(config, &mut builder);
    scan_bound_profile_staging(config, &mut builder);
    scan_gc_quarantine(config, &mut builder);
    builder.finish(generated_at_utc)
}

fn scan_reconciliation(config: &GarbageCollectorConfig, builder: &mut InventoryBuilder) {
    let root = config.ssd_root.join(".dasobjectstore/remote-s3-reconcile");
    for store in safe_directories(&root, StagingRootKind::RemoteS3Reconciliation, builder) {
        for snapshot in safe_directories(&store, StagingRootKind::RemoteS3Reconciliation, builder) {
            let (bytes, safe) = observed_tree_size(&snapshot);
            if !safe {
                builder.unknown(StagingRootKind::RemoteS3Reconciliation, bytes);
                continue;
            }
            let states = fs::read(snapshot.join(".dasobjectstore/reconciliation-manifest.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                .and_then(|value| {
                    value.get("entries")?.as_object().map(|entries| {
                        entries
                            .values()
                            .filter_map(|entry| entry.get("state")?.as_str())
                            .map(str::to_string)
                            .collect::<Vec<_>>()
                    })
                });
            let (disposition, reason) = match states {
                Some(states)
                    if !states.is_empty() && states.iter().all(|state| state == "complete") =>
                {
                    (
                        StagingByteDisposition::Blocked,
                        StagingRetentionReason::DurabilityNotProven,
                    )
                }
                Some(_) => (
                    StagingByteDisposition::Resumable,
                    StagingRetentionReason::ResumableCheckpoint,
                ),
                None => (
                    StagingByteDisposition::Unknown,
                    StagingRetentionReason::UnsupportedMetadata,
                ),
            };
            builder.record(
                StagingRootKind::RemoteS3Reconciliation,
                disposition,
                reason,
                bytes,
            );
        }
    }
}

fn root_for_general_kind(kind: GarbageCollectKind) -> StagingRootKind {
    match kind {
        GarbageCollectKind::IngestJob => StagingRootKind::IngestJob,
        GarbageCollectKind::PerformanceTest => StagingRootKind::PerformanceTest,
        GarbageCollectKind::MultipartUpload => StagingRootKind::DirectS3Multipart,
    }
}

fn disposition_for_gc(decision: GarbageCollectDecision, reason: &str) -> StagingByteDisposition {
    match decision {
        GarbageCollectDecision::Reclaimable | GarbageCollectDecision::Reclaimed => {
            StagingByteDisposition::Reclaimable
        }
        GarbageCollectDecision::Failed => StagingByteDisposition::Unknown,
        GarbageCollectDecision::Retained if reason.contains("active") => {
            StagingByteDisposition::Active
        }
        GarbageCollectDecision::Retained
            if reason.contains("resumable") || reason.contains("grace") =>
        {
            StagingByteDisposition::Resumable
        }
        GarbageCollectDecision::Retained => StagingByteDisposition::Retained,
    }
}

fn typed_reason(reason: &str) -> StagingRetentionReason {
    if reason.contains("active") {
        StagingRetentionReason::ActiveOperation
    } else if reason.contains("resumable") || reason.contains("incomplete") {
        StagingRetentionReason::ResumableCheckpoint
    } else if reason.contains("grace") {
        StagingRetentionReason::TerminalGrace
    } else if reason.contains("durab") || reason.contains("copy_still_required") {
        StagingRetentionReason::DurabilityNotProven
    } else if reason.contains("metadata_missing")
        || reason.contains("without_object")
        || reason.contains("catalogue")
    {
        StagingRetentionReason::CatalogueEvidenceMissing
    } else if reason.contains("keep_requested") {
        StagingRetentionReason::ExplicitRetentionRequested
    } else if reason.contains("legacy") || reason.contains("unowned") {
        StagingRetentionReason::LegacyUnowned
    } else if reason.contains("schema") || reason.contains("unsupported") {
        StagingRetentionReason::UnsupportedMetadata
    } else if reason.contains("unsafe") || reason.contains("mismatch") || reason.contains("missing")
    {
        StagingRetentionReason::UnsafeEntry
    } else if reason.contains("reclaim") || reason.contains("verified") {
        StagingRetentionReason::ReclaimableAfterDurabilityProof
    } else {
        StagingRetentionReason::AmbiguousState
    }
}

fn scan_direct_s3_uploads(config: &GarbageCollectorConfig, builder: &mut InventoryBuilder) {
    let stores = config.ssd_root.join(".dasobjectstore/stores");
    for store in safe_directories(&stores, StagingRootKind::DirectS3Upload, builder) {
        let uploads = store.join("direct-s3/uploads");
        for upload in safe_directories(&uploads, StagingRootKind::DirectS3Upload, builder) {
            let (bytes, safe) = observed_tree_size(&upload);
            if !safe {
                builder.unknown(StagingRootKind::DirectS3Upload, bytes);
                continue;
            }
            let state = fs::read(upload.join("journal.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                .and_then(|value| value.get("state")?.as_str().map(str::to_string));
            let (disposition, reason) = match state.as_deref() {
                Some("receiving") => (
                    StagingByteDisposition::Resumable,
                    StagingRetentionReason::ResumableCheckpoint,
                ),
                Some("verified") | Some("published") => (
                    StagingByteDisposition::Blocked,
                    StagingRetentionReason::DurabilityNotProven,
                ),
                Some("accepted") => (
                    StagingByteDisposition::Blocked,
                    StagingRetentionReason::DurabilityNotProven,
                ),
                Some("aborted") => (
                    StagingByteDisposition::Reclaimable,
                    StagingRetentionReason::ExplicitlyAborted,
                ),
                _ => (
                    StagingByteDisposition::Unknown,
                    StagingRetentionReason::UnsupportedMetadata,
                ),
            };
            builder.record(StagingRootKind::DirectS3Upload, disposition, reason, bytes);
        }
    }
}

fn scan_direct_profile_staging(config: &GarbageCollectorConfig, builder: &mut InventoryBuilder) {
    let stores = config.ssd_root.join(".dasobjectstore/stores");
    for store in safe_directories(&stores, StagingRootKind::FolderStaging, builder) {
        let staging = store.join("direct-s3/profile/.dasobjectstore/staging");
        scan_immediate_entries(
            &staging,
            StagingRootKind::FolderStaging,
            StagingByteDisposition::Unknown,
            StagingRetentionReason::CatalogueEvidenceMissing,
            builder,
        );
    }
}

fn scan_bound_profile_staging(config: &GarbageCollectorConfig, builder: &mut InventoryBuilder) {
    let Some(state_dir) = config.report_journal_path.parent().and_then(Path::parent) else {
        builder.failed_root(StagingRootKind::FolderStaging);
        return;
    };
    let registry = profile_binding_registry_path(state_dir);
    if !registry.exists() {
        return;
    }
    let bindings = match read_profile_bindings(&registry) {
        Ok(bindings) => bindings,
        Err(_) => {
            builder.failed_root(StagingRootKind::FolderStaging);
            return;
        }
    };
    let mut roots = bindings
        .into_iter()
        .map(|binding| binding.backend_root.join(".dasobjectstore/staging"))
        .collect::<Vec<_>>();
    roots.sort();
    roots.dedup();
    for root in roots {
        scan_immediate_entries(
            &root,
            StagingRootKind::FolderStaging,
            StagingByteDisposition::Unknown,
            StagingRetentionReason::CatalogueEvidenceMissing,
            builder,
        );
    }
}

fn scan_gc_quarantine(config: &GarbageCollectorConfig, builder: &mut InventoryBuilder) {
    scan_immediate_entries(
        &config.ssd_root.join(".dasobjectstore/.gc-quarantine"),
        StagingRootKind::GarbageCollectionQuarantine,
        StagingByteDisposition::Blocked,
        StagingRetentionReason::InterruptedGarbageCollection,
        builder,
    );
}

fn scan_immediate_entries(
    root: &Path,
    kind: StagingRootKind,
    disposition: StagingByteDisposition,
    reason: StagingRetentionReason,
    builder: &mut InventoryBuilder,
) {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(_) => {
            builder.failed_root(kind);
            return;
        }
    };
    for entry in entries {
        let Ok(entry) = entry else {
            builder.failed_root(kind);
            continue;
        };
        let (bytes, safe) = observed_tree_size(&entry.path());
        if safe {
            builder.record(kind, disposition, reason, bytes);
        } else {
            builder.unknown(kind, bytes);
        }
    }
}

fn safe_directories(
    root: &Path,
    kind: StagingRootKind,
    builder: &mut InventoryBuilder,
) -> Vec<PathBuf> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(_) => {
            builder.failed_root(kind);
            return Vec::new();
        }
    };
    entries
        .filter_map(|entry| match entry {
            Ok(entry) => match entry.file_type() {
                Ok(file_type) if file_type.is_dir() && !file_type.is_symlink() => {
                    Some(entry.path())
                }
                Ok(_) => {
                    let (bytes, _) = observed_tree_size(&entry.path());
                    builder.unknown(kind, bytes);
                    None
                }
                Err(_) => {
                    builder.failed_root(kind);
                    None
                }
            },
            Err(_) => {
                builder.failed_root(kind);
                None
            }
        })
        .collect()
}

/// Count only directory entries themselves; never follow links or mounts.
fn observed_tree_size(root: &Path) -> (u64, bool) {
    let Ok(root_metadata) = fs::symlink_metadata(root) else {
        return (0, false);
    };
    #[cfg(unix)]
    let root_device = {
        use std::os::unix::fs::MetadataExt;
        root_metadata.dev()
    };
    let mut bytes = 0_u64;
    let mut safe = true;
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            safe = false;
            continue;
        };
        if metadata.file_type().is_symlink() {
            safe = false;
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.dev() != root_device {
                safe = false;
                continue;
            }
        }
        if metadata.is_file() {
            bytes = bytes.saturating_add(metadata.len());
        } else if metadata.is_dir() {
            match fs::read_dir(path) {
                Ok(entries) => {
                    for entry in entries {
                        match entry {
                            Ok(entry) => pending.push(entry.path()),
                            Err(_) => safe = false,
                        }
                    }
                }
                Err(_) => safe = false,
            }
        } else {
            safe = false;
        }
    }
    (bytes, safe)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn groups_direct_upload_folder_staging_and_quarantine_without_paths() {
        let root =
            std::env::temp_dir().join(format!("dos-staging-inventory-{}", std::process::id()));
        let ssd = root.join("ssd");
        let state = root.join("state");
        fs::create_dir_all(ssd.join(".dasobjectstore/ingest/jobs")).unwrap();
        fs::create_dir_all(ssd.join(".dasobjectstore/performance-test")).unwrap();
        let live = ssd.join(".dasobjectstore/live.sqlite");
        Connection::open(&live).unwrap();
        let store = ssd.join(".dasobjectstore/stores").join("a".repeat(64));
        let upload = store.join("direct-s3/uploads/tx");
        fs::create_dir_all(&upload).unwrap();
        fs::write(upload.join("journal.json"), br#"{"state":"receiving"}"#).unwrap();
        fs::write(upload.join("payload.part"), b"payload").unwrap();
        let staging = store.join("direct-s3/profile/.dasobjectstore/staging");
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("orphan.part"), b"orphan").unwrap();
        let quarantine = ssd.join(".dasobjectstore/.gc-quarantine/entry");
        fs::create_dir_all(&quarantine).unwrap();
        fs::write(quarantine.join("payload"), b"held").unwrap();
        let config = GarbageCollectorConfig {
            ssd_root: ssd,
            live_sqlite_path: live,
            report_journal_path: state.join("gc.json"),
            terminal_grace: Duration::ZERO,
            maximum_items_per_run: 100,
        };
        let inventory = build_staging_inventory(&config, "2026-01-01T00:00:00Z", UNIX_EPOCH);
        assert!(inventory.accounting_is_complete());
        assert!(inventory
            .groups
            .iter()
            .any(|group| group.root_kind == StagingRootKind::DirectS3Upload
                && group.disposition == StagingByteDisposition::Resumable));
        assert!(inventory
            .groups
            .iter()
            .any(|group| group.root_kind == StagingRootKind::FolderStaging));
        assert!(inventory.groups.iter().any(|group| {
            group.root_kind == StagingRootKind::GarbageCollectionQuarantine
                && group.reason == StagingRetentionReason::InterruptedGarbageCollection
        }));
        assert!(!serde_json::to_string(&inventory)
            .unwrap()
            .contains(root.to_str().unwrap()));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unsafe_entry_is_accounted_as_unknown() {
        let root = std::env::temp_dir().join(format!(
            "dos-staging-inventory-unsafe-{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/outside", root.join("link")).unwrap();
        let mut builder = InventoryBuilder::default();
        scan_immediate_entries(
            &root,
            StagingRootKind::FolderStaging,
            StagingByteDisposition::Unknown,
            StagingRetentionReason::CatalogueEvidenceMissing,
            &mut builder,
        );
        let inventory = builder.finish("2026-01-01T00:00:00Z");
        assert!(inventory.accounting_is_complete());
        assert_eq!(inventory.coverage, StagingInventoryCoverage::Partial);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unexpected_file_in_directory_namespace_keeps_its_bytes_visible() {
        let root =
            std::env::temp_dir().join(format!("dos-staging-inventory-file-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("unexpected.part"), b"retained").unwrap();
        let mut builder = InventoryBuilder::default();
        let directories = safe_directories(&root, StagingRootKind::DirectS3Upload, &mut builder);
        assert!(directories.is_empty());
        let inventory = builder.finish("2026-01-01T00:00:00Z");
        assert_eq!(inventory.observed_bytes, 8);
        assert_eq!(inventory.accounted_bytes, 8);
        assert_eq!(inventory.unaccounted_bytes, 0);
        assert_eq!(inventory.coverage, StagingInventoryCoverage::Partial);
        fs::remove_dir_all(root).unwrap();
    }
}
