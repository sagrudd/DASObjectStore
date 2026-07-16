use clap::Parser;
use prosopikon_core::ProsopikonAuthStore;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

const CONFIRMATION: &str = "confirm auth migration";
const MARKER_SCHEMA: &str = "dasobjectstore.auth_migration.v1";
const MARKER_FILE: &str = "dasobjectstore-auth-migration.json";

#[derive(Debug, Parser)]
#[command(name = "dasobjectstore-auth-migrate", version = dasobjectstore_core::VERSION)]
struct Cli {
    /// Existing DASObjectStore Prosopikon-compatible authentication root.
    #[arg(long)]
    source_root: PathBuf,
    /// Monas/Prosopikon authentication root to initialize.
    #[arg(long)]
    target_root: PathBuf,
    /// Write the validated registry and migration marker. Omit for a dry run.
    #[arg(long)]
    apply: bool,
    /// Required exact confirmation when --apply is used.
    #[arg(long)]
    confirm: Option<String>,
    /// Emit the report as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Serialize)]
struct MigrationReport {
    schema_version: &'static str,
    source_root: PathBuf,
    target_root: PathBuf,
    source_registry_sha256: String,
    registry_schema_version: u32,
    users: usize,
    registered_users: usize,
    sessions: usize,
    groups: usize,
    group_memberships: usize,
    rights: usize,
    device_tokens: usize,
    source_retained: bool,
    browser_reauthentication_required: bool,
    outcome: &'static str,
}

#[derive(Debug, Serialize)]
struct MigrationMarker<'a> {
    schema_version: &'static str,
    source_registry_sha256: &'a str,
    source_root: &'a Path,
    target_root: &'a Path,
    registry_schema_version: u32,
    users: usize,
    sessions: usize,
    source_retained: bool,
    applied_at_unix_seconds: u64,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(report) => {
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).expect("report serializes")
                );
            } else {
                print_report(&report);
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<MigrationReport, String> {
    if cli.source_root == cli.target_root {
        return Err("source and target authentication roots must be different".to_string());
    }
    if cli.apply && cli.confirm.as_deref() != Some(CONFIRMATION) {
        return Err(format!("--apply requires --confirm \"{CONFIRMATION}\""));
    }

    reject_symlink(&cli.source_root, "source root")?;
    let source_store = ProsopikonAuthStore::new(&cli.source_root);
    let source_path = source_store.registry_path();
    reject_non_regular_or_symlink(&source_path, "source registry")?;
    let source_bytes = fs::read(&source_path).map_err(|error| io_error(&source_path, error))?;
    let registry = source_store
        .load_registry()
        .map_err(|error| format!("source registry validation failed: {error}"))?;
    let source_hash = format!("sha256:{:x}", Sha256::digest(&source_bytes));
    let sessions = registry.users.iter().map(|user| user.sessions.len()).sum();
    let registered_users = registry
        .users
        .iter()
        .filter(|user| user.password_hash.is_some())
        .count();

    let mut report = MigrationReport {
        schema_version: MARKER_SCHEMA,
        source_root: cli.source_root.clone(),
        target_root: cli.target_root.clone(),
        source_registry_sha256: source_hash,
        registry_schema_version: registry.schema_version,
        users: registry.users.len(),
        registered_users,
        sessions,
        groups: registry.groups.len(),
        group_memberships: registry.group_memberships.len(),
        rights: registry.rights.len(),
        device_tokens: registry.device_tokens.len(),
        source_retained: true,
        browser_reauthentication_required: true,
        outcome: "dry_run",
    };
    validate_target(&cli.target_root, &source_bytes)?;
    validate_existing_marker(&cli.target_root, &report)?;

    if cli.apply {
        create_private_dir_all(&cli.target_root)?;
        reject_symlink(&cli.target_root, "target root")?;
        atomic_write_if_missing(&cli.target_root, "users.json", &source_bytes)?;
        let stable_source =
            fs::read(&source_path).map_err(|error| io_error(&source_path, error))?;
        if stable_source != source_bytes {
            return Err(
                "source registry changed during migration; leave the unmarked target offline and retry into a new empty target after stopping both authentication services"
                    .to_string(),
            );
        }
        let marker = MigrationMarker {
            schema_version: MARKER_SCHEMA,
            source_registry_sha256: &report.source_registry_sha256,
            source_root: &report.source_root,
            target_root: &report.target_root,
            registry_schema_version: report.registry_schema_version,
            users: report.users,
            sessions: report.sessions,
            source_retained: true,
            applied_at_unix_seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|error| error.to_string())?
                .as_secs(),
        };
        let marker_bytes = serde_json::to_vec_pretty(&marker).map_err(|error| error.to_string())?;
        atomic_write_if_missing(&cli.target_root, MARKER_FILE, &marker_bytes)?;
        report.outcome = "applied";
    }
    Ok(report)
}

fn validate_target(target_root: &Path, source_bytes: &[u8]) -> Result<(), String> {
    if target_root.exists() {
        reject_symlink(target_root, "target root")?;
    }
    let target_registry = target_root.join("users.json");
    if !target_registry.exists() {
        return Ok(());
    }
    reject_non_regular_or_symlink(&target_registry, "target registry")?;
    let target_bytes =
        fs::read(&target_registry).map_err(|error| io_error(&target_registry, error))?;
    if target_bytes != source_bytes {
        return Err(format!(
            "target registry {} already exists with different content",
            target_registry.display()
        ));
    }
    Ok(())
}

fn create_private_dir_all(path: &Path) -> Result<(), String> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path).map_err(|error| io_error(path, error))
}

fn atomic_write_if_missing(root: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    let destination = root.join(name);
    if destination.exists() {
        let existing = fs::read(&destination).map_err(|error| io_error(&destination, error))?;
        if name == "users.json" && existing == bytes {
            return Ok(());
        }
        if name == MARKER_FILE {
            return Ok(());
        }
        return Err(format!("refusing to replace {}", destination.display()));
    }
    let temporary = root.join(format!(".{name}.migration-{}", std::process::id()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| io_error(&temporary, error))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| io_error(&temporary, error))?;
    fs::rename(&temporary, &destination).map_err(|error| io_error(&destination, error))?;
    Ok(())
}

fn validate_existing_marker(root: &Path, report: &MigrationReport) -> Result<(), String> {
    let path = root.join(MARKER_FILE);
    if !path.exists() {
        return Ok(());
    }
    reject_non_regular_or_symlink(&path, "migration marker")?;
    let marker: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).map_err(|error| io_error(&path, error))?)
            .map_err(|error| format!("invalid migration marker {}: {error}", path.display()))?;
    let matches = marker["schema_version"] == MARKER_SCHEMA
        && marker["source_registry_sha256"] == report.source_registry_sha256
        && marker["source_root"] == report.source_root.to_string_lossy().as_ref()
        && marker["target_root"] == report.target_root.to_string_lossy().as_ref()
        && marker["source_retained"] == true;
    if !matches {
        return Err(format!(
            "migration marker {} does not match this source and target",
            path.display()
        ));
    }
    Ok(())
}

fn reject_non_regular_or_symlink(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| io_error(path, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "{label} must be a regular non-symlink file: {}",
            path.display()
        ));
    }
    Ok(())
}

fn reject_symlink(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| io_error(path, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "{label} must be a non-symlink directory: {}",
            path.display()
        ));
    }
    Ok(())
}

fn io_error(path: &Path, error: io::Error) -> String {
    format!(
        "authentication migration IO failed at {}: {error}",
        path.display()
    )
}

fn print_report(report: &MigrationReport) {
    println!("Authentication migration {}", report.outcome);
    println!(
        "Users: {} ({} registered)",
        report.users, report.registered_users
    );
    println!("Sessions preserved in registry: {}", report.sessions);
    println!("Source retained for rollback: yes");
    println!("Browser reauthentication required: yes");
    println!("Registry checksum: {}", report.source_registry_sha256);
}

#[cfg(test)]
mod tests {
    use super::{run, Cli, CONFIRMATION, MARKER_FILE};
    use prosopikon_core::ProsopikonAuthStore;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn dry_run_and_apply_preserve_source_and_are_idempotent() {
        let root = temp_root("idempotent");
        let source = root.join("source");
        let target = root.join("target");
        let store = ProsopikonAuthStore::new(&source);
        store.create_user("operator").expect("user");
        let token = store
            .issue_registration_token("operator", 1)
            .expect("token");
        store
            .register_with_token("operator", &token, "migration-only-password")
            .expect("registration");
        let original = std::fs::read(store.registry_path()).expect("source bytes");

        let dry_run = run(&Cli {
            source_root: source.clone(),
            target_root: target.clone(),
            apply: false,
            confirm: None,
            json: true,
        })
        .expect("dry run");
        assert_eq!(dry_run.outcome, "dry_run");
        assert!(!target.exists());

        let apply = Cli {
            source_root: source.clone(),
            target_root: target.clone(),
            apply: true,
            confirm: Some(CONFIRMATION.to_string()),
            json: true,
        };
        assert_eq!(run(&apply).expect("apply").outcome, "applied");
        assert_eq!(run(&apply).expect("idempotent replay").outcome, "applied");
        assert_eq!(std::fs::read(store.registry_path()).unwrap(), original);
        assert_eq!(std::fs::read(target.join("users.json")).unwrap(), original);
        assert!(target.join(MARKER_FILE).is_file());
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn apply_rejects_missing_confirmation_and_conflicting_target() {
        let root = temp_root("conflict");
        let source = root.join("source");
        let target = root.join("target");
        ProsopikonAuthStore::new(&source)
            .create_user("operator")
            .expect("source user");
        ProsopikonAuthStore::new(&target)
            .create_user("different")
            .expect("target user");
        let mut cli = Cli {
            source_root: source,
            target_root: target,
            apply: true,
            confirm: None,
            json: false,
        };
        assert!(run(&cli).unwrap_err().contains("requires --confirm"));
        cli.confirm = Some(CONFIRMATION.to_string());
        assert!(run(&cli).unwrap_err().contains("different content"));
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    fn temp_root(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dasobjectstore-auth-migrate-{label}-{}-{nonce}",
            std::process::id()
        ))
    }
}
