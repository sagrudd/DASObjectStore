//! Redacted, daemon-owned audit events for application credentials.
//!
//! Events deliberately retain policy identity and operation metadata only.
//! Request reasons are stored as a SHA-256 digest so an operator can correlate
//! repeated actions without persisting secrets, host paths, or bearer tokens.

use super::DaemonServiceRuntimeError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub const APPLICATION_AUDIT_SCHEMA: &str = "dasobjectstore.application_audit.v1";
pub const APPLICATION_AUDIT_FILE_NAME: &str = "application-audit.json";
pub const APPLICATION_AUDIT_PATH_ENV: &str = "DASOBJECTSTORE_APPLICATION_AUDIT_PATH";
pub const APPLICATION_AUDIT_MAX_EVENTS: usize = 10_000;

static APPLICATION_AUDIT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub fn application_audit_log_path(state_dir: impl AsRef<Path>) -> PathBuf {
    std::env::var_os(APPLICATION_AUDIT_PATH_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| state_dir.as_ref().join(APPLICATION_AUDIT_FILE_NAME))
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationAuditEvent {
    pub schema_version: String,
    pub event_id: String,
    pub occurred_at_utc: String,
    pub operation: String,
    pub application_id: String,
    pub key_id: Option<String>,
    pub administrator_actor: Option<String>,
    pub reason_sha256: String,
    pub dry_run: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ApplicationAuditFile {
    schema_version: String,
    events: Vec<ApplicationAuditEvent>,
}

impl Default for ApplicationAuditFile {
    fn default() -> Self {
        Self {
            schema_version: APPLICATION_AUDIT_SCHEMA.to_string(),
            events: Vec::new(),
        }
    }
}

pub fn read_application_audit_events(
    path: impl AsRef<Path>,
) -> Result<Vec<ApplicationAuditEvent>, DaemonServiceRuntimeError> {
    Ok(read_file(path.as_ref())?.events)
}

pub fn record_application_audit_event(
    path: impl AsRef<Path>,
    occurred_at_utc: &str,
    operation: &str,
    application_id: &str,
    key_id: Option<&str>,
    administrator_actor: Option<&str>,
    reason: &str,
    dry_run: bool,
) -> Result<ApplicationAuditEvent, DaemonServiceRuntimeError> {
    if occurred_at_utc.trim().is_empty()
        || operation.trim().is_empty()
        || application_id.trim().is_empty()
        || reason.trim().is_empty()
    {
        return Err(invalid_audit(
            "audit identity, operation, and reason are required",
        ));
    }
    if key_id.is_some_and(|value| value.trim().is_empty())
        || administrator_actor.is_some_and(|value| value.trim().is_empty())
    {
        return Err(invalid_audit("optional audit fields must not be blank"));
    }
    let reason_sha256 = sha256_digest(reason);
    let event_id = format!(
        "{}-{}-{}",
        operation,
        application_id,
        occurred_at_utc
            .bytes()
            .filter(|byte| byte.is_ascii_alphanumeric())
            .map(char::from)
            .collect::<String>()
    );
    let event = ApplicationAuditEvent {
        schema_version: APPLICATION_AUDIT_SCHEMA.to_string(),
        event_id,
        occurred_at_utc: occurred_at_utc.to_string(),
        operation: operation.to_string(),
        application_id: application_id.to_string(),
        key_id: key_id.map(str::to_string),
        administrator_actor: administrator_actor.map(str::to_string),
        reason_sha256,
        dry_run,
    };
    event.validate()?;

    let path = path.as_ref();
    let lock = APPLICATION_AUDIT_LOCK.get_or_init(|| Mutex::new(()));
    let _guard = lock.lock().expect("application audit lock poisoned");
    let mut file = read_file(path)?;
    file.events
        .retain(|existing| existing.event_id != event.event_id);
    file.events.push(event.clone());
    if file.events.len() > APPLICATION_AUDIT_MAX_EVENTS {
        let excess = file.events.len() - APPLICATION_AUDIT_MAX_EVENTS;
        file.events.drain(..excess);
    }
    write_file(path, &file)?;
    Ok(event)
}

impl ApplicationAuditEvent {
    fn validate(&self) -> Result<(), DaemonServiceRuntimeError> {
        if self.schema_version != APPLICATION_AUDIT_SCHEMA
            || self.event_id.trim().is_empty()
            || self.occurred_at_utc.trim().is_empty()
            || self.operation.trim().is_empty()
            || self.application_id.trim().is_empty()
            || !self.reason_sha256.starts_with("sha256:")
            || self.reason_sha256.len() != "sha256:".len() + 64
            || !self.reason_sha256["sha256:".len()..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(invalid_audit("invalid application audit event"));
        }
        Ok(())
    }
}

fn read_file(path: &Path) -> Result<ApplicationAuditFile, DaemonServiceRuntimeError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(ApplicationAuditFile::default())
        }
        Err(error) => return Err(audit_io(path, error)),
    };
    let file: ApplicationAuditFile = serde_json::from_reader(file).map_err(|error| {
        invalid_audit(format!(
            "invalid application audit log {}: {error}",
            path.display()
        ))
    })?;
    if file.schema_version != APPLICATION_AUDIT_SCHEMA {
        return Err(invalid_audit(format!(
            "unsupported application audit schema {}",
            file.schema_version
        )));
    }
    for event in &file.events {
        event.validate()?;
    }
    Ok(file)
}

fn write_file(path: &Path, file: &ApplicationAuditFile) -> Result<(), DaemonServiceRuntimeError> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid_audit("application audit log has no parent"))?;
    fs::create_dir_all(parent).map_err(|error| audit_io(parent, error))?;
    let temporary = parent.join(format!(
        ".{}.tmp-{}-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("application-audit"),
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    let bytes = serde_json::to_vec_pretty(file)
        .map_err(|error| invalid_audit(format!("serialize application audit log: {error}")))?;
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| audit_io(&temporary, error))?;
    output
        .write_all(&bytes)
        .and_then(|_| output.sync_all())
        .map_err(|error| audit_io(&temporary, error))?;
    drop(output);
    fs::rename(&temporary, path).map_err(|error| audit_io(path, error))?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| audit_io(parent, error))
}

fn sha256_digest(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    let encoded = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("sha256:{encoded}")
}

fn invalid_audit(message: impl Into<String>) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::UnsupportedOperation {
        operation: format!("invalid application audit event: {}", message.into()),
    }
}

fn audit_io(path: &Path, error: io::Error) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::UnsupportedOperation {
        operation: format!("application audit I/O {}: {error}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        application_audit_log_path, read_application_audit_events, record_application_audit_event,
        APPLICATION_AUDIT_SCHEMA,
    };
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn audit_log_is_state_scoped_and_redacts_reason() {
        let root = temp_root("record");
        let path = root.join("application-audit.json");
        let event = record_application_audit_event(
            &path,
            "2026-07-13T17:00:00Z",
            "revoke_identity",
            "synoptikon-ingest",
            None,
            Some("root"),
            "credential rotation at /private/path",
            false,
        )
        .expect("record event");
        assert_eq!(event.schema_version, APPLICATION_AUDIT_SCHEMA);
        assert!(event.reason_sha256.starts_with("sha256:"));
        let encoded = serde_json::to_string(&event).expect("encode");
        assert!(!encoded.contains("credential rotation"));
        assert!(!encoded.contains("/private/path"));
        assert_eq!(read_application_audit_events(&path).expect("read").len(), 1);
        assert_eq!(
            application_audit_log_path(&root),
            root.join("application-audit.json")
        );
        cleanup(&root);
    }

    fn temp_root(label: &str) -> PathBuf {
        let root = std::env::var_os("DASOBJECTSTORE_CODEX_VALIDATION_ROOT")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|home| PathBuf::from(home).join(".dasobjectstore-codex-validation"))
            })
            .unwrap_or_else(std::env::temp_dir)
            .join(format!("application-audit-{label}-{}", std::process::id()));
        fs::create_dir_all(&root).expect("fixture root");
        root
    }

    fn cleanup(root: &PathBuf) {
        let _ = fs::remove_dir_all(root);
    }
}
