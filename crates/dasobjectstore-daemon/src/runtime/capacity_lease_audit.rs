//! Atomic, redacted audit trail for capacity reservation lease maintenance.

use super::capacity_lease::{CapacityReservationLeaseAction, CapacityReservationLeaseEvent};
use super::service::DaemonServiceRuntimeError;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub const CAPACITY_LEASE_AUDIT_SCHEMA: &str = "dasobjectstore.capacity_lease_audit.v1";
pub const CAPACITY_LEASE_AUDIT_FILE_NAME: &str = "capacity-lease-audit.json";
pub const CAPACITY_LEASE_AUDIT_MAX_EVENTS: usize = 10_000;

static AUDIT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CapacityReservationLeaseAuditRecord {
    pub recorded_at_unix_seconds: u64,
    pub store_id: String,
    pub reservation_id_sha256: String,
    pub action: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CapacityReservationLeaseAuditLog {
    schema_version: String,
    events: Vec<CapacityReservationLeaseAuditRecord>,
}

pub fn capacity_lease_audit_path(state_dir: impl AsRef<Path>) -> PathBuf {
    state_dir.as_ref().join(CAPACITY_LEASE_AUDIT_FILE_NAME)
}

pub fn record_capacity_lease_audit_events(
    path: impl AsRef<Path>,
    recorded_at_unix_seconds: u64,
    events: &[CapacityReservationLeaseEvent],
) -> Result<(), DaemonServiceRuntimeError> {
    if events.is_empty() {
        return Ok(());
    }
    if recorded_at_unix_seconds == 0 {
        return Err(invalid_audit("audit timestamp must be nonzero"));
    }
    let path = path.as_ref();
    let _guard = AUDIT_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| invalid_audit("audit lock poisoned"))?;
    let mut log = read_log(path)?;
    for event in events {
        if !valid_digest(&event.reservation_id_sha256) {
            return Err(invalid_audit("reservation digest is invalid"));
        }
        log.events.push(CapacityReservationLeaseAuditRecord {
            recorded_at_unix_seconds,
            store_id: event.store_id.as_str().to_string(),
            reservation_id_sha256: event.reservation_id_sha256.clone(),
            action: action_name(event.action).to_string(),
            bytes: event.bytes,
        });
    }
    if log.events.len() > CAPACITY_LEASE_AUDIT_MAX_EVENTS {
        log.events
            .drain(..log.events.len() - CAPACITY_LEASE_AUDIT_MAX_EVENTS);
    }
    write_log(path, &log)
}

pub fn read_capacity_lease_audit_events(
    path: impl AsRef<Path>,
) -> Result<Vec<CapacityReservationLeaseAuditRecord>, DaemonServiceRuntimeError> {
    Ok(read_log(path.as_ref())?.events)
}

fn read_log(path: &Path) -> Result<CapacityReservationLeaseAuditLog, DaemonServiceRuntimeError> {
    if !path.exists() {
        return Ok(CapacityReservationLeaseAuditLog {
            schema_version: CAPACITY_LEASE_AUDIT_SCHEMA.to_string(),
            events: Vec::new(),
        });
    }
    let bytes = fs::read(path).map_err(|error| audit_io(path, error))?;
    let log: CapacityReservationLeaseAuditLog = serde_json::from_slice(&bytes)
        .map_err(|error| invalid_audit(format!("parse {}: {error}", path.display())))?;
    if log.schema_version != CAPACITY_LEASE_AUDIT_SCHEMA
        || log.events.iter().any(|event| {
            event.recorded_at_unix_seconds == 0
                || event.store_id.trim().is_empty()
                || !valid_digest(&event.reservation_id_sha256)
        })
    {
        return Err(invalid_audit("audit log failed validation"));
    }
    Ok(log)
}

fn write_log(
    path: &Path,
    log: &CapacityReservationLeaseAuditLog,
) -> Result<(), DaemonServiceRuntimeError> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid_audit("audit path has no parent"))?;
    fs::create_dir_all(parent).map_err(|error| audit_io(parent, error))?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("capacity-lease-audit"),
        std::process::id()
    ));
    let bytes = serde_json::to_vec_pretty(log)
        .map_err(|error| invalid_audit(format!("serialize audit log: {error}")))?;
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| audit_io(&temporary, error))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| audit_io(&temporary, error))?;
    fs::rename(&temporary, path).map_err(|error| audit_io(path, error))?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| audit_io(parent, error))
}

fn action_name(action: CapacityReservationLeaseAction) -> &'static str {
    match action {
        CapacityReservationLeaseAction::Renewed => "renewed",
        CapacityReservationLeaseAction::Expired => "expired",
        CapacityReservationLeaseAction::LegacyRetained => "legacy_retained",
    }
}

fn valid_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn invalid_audit(message: impl Into<String>) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::UnsupportedOperation {
        operation: format!("invalid capacity lease audit: {}", message.into()),
    }
}

fn audit_io(path: &Path, error: std::io::Error) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::UnsupportedOperation {
        operation: format!("capacity lease audit I/O {}: {error}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dasobjectstore_core::ids::StoreId;

    #[test]
    fn persists_only_redacted_reservation_identity() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-capacity-lease-audit-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let path = root.join("audit.json");
        let event = CapacityReservationLeaseEvent {
            store_id: StoreId::new("store-1").expect("store"),
            reservation_id_sha256: format!("sha256:{}", "a".repeat(64)),
            action: CapacityReservationLeaseAction::Expired,
            bytes: 42,
        };
        record_capacity_lease_audit_events(&path, 100, &[event]).expect("record audit");
        let text = fs::read_to_string(&path).expect("read audit");
        assert!(!text.contains("reservation-1"));
        let events = read_capacity_lease_audit_events(&path).expect("read events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, "expired");
        let _ = fs::remove_dir_all(root);
    }
}
