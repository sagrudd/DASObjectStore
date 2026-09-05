//! Durable, daemon-owned indication that the isolated custody plane is active.
//!
//! Legacy direct CLI code cannot prove which daemon configuration is live, so
//! it never uses a caller-selected configuration file to make this decision.
//! Instead the daemon creates this one canonical, hash-only marker before it
//! composes an enabled custody plane. The marker intentionally survives a
//! daemon stop, restart, and later inactive configuration until a separately
//! authorised attended deactivation transaction is introduced; this source
//! release exposes no CLI or ordinary API removal path.

use super::config::{DaemonCustodyRuntimeConfig, DEFAULT_DAEMON_STATE_DIR};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const CUSTODY_ACTIVATION_MARKER_FILE_NAME: &str = "custody-activation.json";
pub const CUSTODY_ACTIVATION_MARKER_SCHEMA: &str = "dasobjectstore.custody-activation.v1";

/// Secret-free durable state consumed by legacy CLI route admission. Its
/// configuration digest binds the complete custody configuration without
/// persisting paths, opaque references, or any credential material.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CustodyActivationMarkerV1 {
    pub schema: String,
    pub state: String,
    pub activation_id: String,
    pub custody_configuration_sha256: String,
    pub activated_at_utc: String,
}

/// The sole production path a legacy CLI may inspect. It is independent of
/// `dasobjectstored --config`, daemon `state_dir`, environment, and any
/// caller-provided CLI option.
pub fn canonical_custody_activation_marker_path() -> PathBuf {
    custody_activation_marker_path_for_state_dir(DEFAULT_DAEMON_STATE_DIR)
}

pub fn custody_activation_marker_path_for_state_dir(state_dir: impl AsRef<Path>) -> PathBuf {
    state_dir.as_ref().join(CUSTODY_ACTIVATION_MARKER_FILE_NAME)
}

/// Writes (or re-verifies) the active marker before the daemon can compose an
/// enabled custody plane. An existing marker is accepted only when it binds
/// the exact same custody configuration; a malformed, foreign, or changed
/// marker prevents startup instead of being adopted or overwritten.
pub fn ensure_custody_activation_marker(
    custody: &DaemonCustodyRuntimeConfig,
    activated_at_utc: &str,
) -> Result<CustodyActivationMarkerV1, String> {
    ensure_custody_activation_marker_at(
        &canonical_custody_activation_marker_path(),
        custody,
        activated_at_utc,
    )
}

pub fn ensure_custody_activation_marker_at(
    marker_path: &Path,
    custody: &DaemonCustodyRuntimeConfig,
    activated_at_utc: &str,
) -> Result<CustodyActivationMarkerV1, String> {
    if !custody.enabled {
        return Err("cannot create a custody activation marker for an inactive plane".to_string());
    }
    validate_timestamp("custody activation time", activated_at_utc)?;
    let expected_digest = custody_configuration_sha256(custody)?;
    let expected = CustodyActivationMarkerV1 {
        schema: CUSTODY_ACTIVATION_MARKER_SCHEMA.to_string(),
        state: "active".to_string(),
        activation_id: Uuid::new_v4().to_string(),
        custody_configuration_sha256: expected_digest,
        activated_at_utc: activated_at_utc.to_string(),
    };
    let parent = marker_path
        .parent()
        .ok_or_else(|| "custody activation marker has no parent directory".to_string())?;
    ensure_marker_parent(parent)?;

    match read_custody_activation_marker_at(marker_path) {
        Ok(Some(existing)) => {
            verify_active_marker(&existing)?;
            if existing.custody_configuration_sha256 != expected.custody_configuration_sha256 {
                return Err(
                    "custody activation marker binds a different configuration; attended deactivation is required before changing the active plane"
                        .to_string(),
                );
            }
            return Ok(existing);
        }
        Ok(None) => {}
        Err(error) => return Err(format!("custody activation marker is not usable: {error}")),
    }

    let raw = serde_json::to_vec(&expected)
        .map_err(|error| format!("serialize custody activation marker: {error}"))?;
    let temporary = parent.join(format!(".custody-activation-{}.tmp", Uuid::new_v4()));
    write_private_new_file(&temporary, &raw)?;
    match fs::hard_link(&temporary, marker_path) {
        Ok(()) => {
            fs::remove_file(&temporary)
                .map_err(|error| format!("remove temporary custody activation marker: {error}"))?;
            sync_directory(parent)?;
            Ok(expected)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(&temporary);
            let existing = read_custody_activation_marker_at(marker_path)?.ok_or_else(|| {
                "custody activation marker disappeared during an activation race".to_string()
            })?;
            verify_active_marker(&existing)?;
            if existing.custody_configuration_sha256 != expected.custody_configuration_sha256 {
                return Err(
                    "custody activation marker raced with a different configuration".to_string(),
                );
            }
            Ok(existing)
        }
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            Err(format!(
                "atomically publish custody activation marker: {error}"
            ))
        }
    }
}

/// The startup lifecycle guard for every daemon configuration, not only one
/// that currently requests custody. A durable active marker makes an inactive
/// or alternate daemon configuration unsafe: it could otherwise compose normal
/// registry/capacity/service paths alongside the sealed custody plane. The
/// caller must invoke this before any mutable runtime composition.
pub fn validate_daemon_custody_activation(
    custody: &DaemonCustodyRuntimeConfig,
    activated_at_utc: &str,
) -> Result<(), String> {
    validate_daemon_custody_activation_at(
        &canonical_custody_activation_marker_path(),
        custody,
        activated_at_utc,
    )
}

pub fn validate_daemon_custody_activation_at(
    marker_path: &Path,
    custody: &DaemonCustodyRuntimeConfig,
    activated_at_utc: &str,
) -> Result<(), String> {
    if custody.enabled {
        ensure_custody_activation_marker_at(marker_path, custody, activated_at_utc)?;
        return Ok(());
    }
    if custody_activation_blocks_legacy_cli_at(marker_path)? {
        return Err(
            "daemon startup with custody disabled is denied while a durable active custody marker exists; an attended deactivation transaction is required before normal-plane composition"
                .to_string(),
        );
    }
    Ok(())
}

/// Returns `true` when ordinary legacy CLI routes must be denied. Any problem
/// reading a present marker is an error for the caller to fail closed on.
pub fn custody_activation_blocks_legacy_cli() -> Result<bool, String> {
    custody_activation_blocks_legacy_cli_at(&canonical_custody_activation_marker_path())
}

pub fn custody_activation_blocks_legacy_cli_at(marker_path: &Path) -> Result<bool, String> {
    match read_custody_activation_marker_at(marker_path)? {
        Some(marker) => {
            verify_active_marker(&marker)?;
            Ok(true)
        }
        None => Ok(false),
    }
}

pub fn read_custody_activation_marker_at(
    marker_path: &Path,
) -> Result<Option<CustodyActivationMarkerV1>, String> {
    let metadata = match fs::symlink_metadata(marker_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "inspect custody activation marker {}: {error}",
                marker_path.display()
            ));
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("custody activation marker must be a regular non-symlink file".to_string());
    }
    #[cfg(unix)]
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err("custody activation marker must not be group- or world-readable".to_string());
    }
    let raw = fs::read(marker_path).map_err(|error| {
        format!(
            "read custody activation marker {}: {error}",
            marker_path.display()
        )
    })?;
    let marker: CustodyActivationMarkerV1 = serde_json::from_slice(&raw)
        .map_err(|error| format!("parse custody activation marker: {error}"))?;
    verify_active_marker(&marker)?;
    Ok(Some(marker))
}

fn custody_configuration_sha256(custody: &DaemonCustodyRuntimeConfig) -> Result<String, String> {
    let raw = serde_jcs::to_vec(custody)
        .map_err(|error| format!("canonicalize custody activation configuration: {error}"))?;
    Ok(hex::encode(Sha256::digest(raw)))
}

fn verify_active_marker(marker: &CustodyActivationMarkerV1) -> Result<(), String> {
    if marker.schema != CUSTODY_ACTIVATION_MARKER_SCHEMA || marker.state != "active" {
        return Err("custody activation marker has an unsupported schema or state".to_string());
    }
    Uuid::parse_str(&marker.activation_id)
        .map_err(|_| "custody activation marker has an invalid activation id".to_string())?;
    if marker.custody_configuration_sha256.len() != 64
        || !marker
            .custody_configuration_sha256
            .bytes()
            .all(|value| value.is_ascii_hexdigit())
    {
        return Err("custody activation marker has an invalid configuration digest".to_string());
    }
    validate_timestamp(
        "custody activation marker timestamp",
        &marker.activated_at_utc,
    )
}

fn validate_timestamp(field: &str, value: &str) -> Result<(), String> {
    let _ = DateTime::parse_from_rfc3339(value)
        .map_err(|error| format!("{field} must be RFC3339: {error}"))?
        .with_timezone(&Utc);
    Ok(())
}

fn ensure_marker_parent(parent: &Path) -> Result<(), String> {
    fs::create_dir_all(parent)
        .map_err(|error| format!("create custody activation marker directory: {error}"))?;
    let metadata = fs::symlink_metadata(parent)
        .map_err(|error| format!("inspect custody activation marker directory: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("custody activation marker directory must be a real directory".to_string());
    }
    #[cfg(unix)]
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("protect custody activation marker directory: {error}"))?;
    Ok(())
}

fn write_private_new_file(path: &Path, raw: &[u8]) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(path)
        .map_err(|error| format!("create temporary custody activation marker: {error}"))?;
    file.write_all(raw)
        .map_err(|error| format!("write temporary custody activation marker: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("sync temporary custody activation marker: {error}"))
}

fn sync_directory(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("sync custody activation marker directory: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{
        custody_activation_blocks_legacy_cli_at, ensure_custody_activation_marker_at,
        read_custody_activation_marker_at, validate_daemon_custody_activation_at,
    };
    use crate::runtime::DaemonRuntimeConfig;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_marker_path(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!(
                "das-custody-activation-{name}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("clock")
                    .as_nanos()
            ))
            .join("state/custody-activation.json")
    }

    #[test]
    fn active_marker_is_durable_and_binds_the_exact_custody_configuration() {
        let marker_path = temp_marker_path("durable");
        let mut config = DaemonRuntimeConfig::linux_packaged();
        config.custody.enabled = true;
        let marker = ensure_custody_activation_marker_at(
            &marker_path,
            &config.custody,
            "2026-09-05T12:00:00Z",
        )
        .expect("daemon writes activation before composing custody");
        assert_eq!(
            read_custody_activation_marker_at(&marker_path).expect("read marker"),
            Some(marker.clone())
        );
        assert!(custody_activation_blocks_legacy_cli_at(&marker_path).expect("active blocks"));
        assert_eq!(
            ensure_custody_activation_marker_at(
                &marker_path,
                &config.custody,
                "2026-09-05T12:01:00Z",
            )
            .expect("restart preserves exact active marker"),
            marker
        );
        config.custody.endpoint = "http://127.0.0.1:3902".to_string();
        assert!(ensure_custody_activation_marker_at(
            &marker_path,
            &config.custody,
            "2026-09-05T12:02:00Z",
        )
        .is_err());
        let _ = fs::remove_dir_all(marker_path.ancestors().nth(2).expect("temp root"));
    }

    #[test]
    fn absent_or_malformed_marker_never_looks_like_an_inactive_plane() {
        let marker_path = temp_marker_path("malformed");
        assert!(!custody_activation_blocks_legacy_cli_at(&marker_path).expect("absent inactive"));
        fs::create_dir_all(marker_path.parent().expect("parent")).expect("parent");
        fs::write(&marker_path, b"not-json").expect("malformed marker");
        #[cfg(unix)]
        fs::set_permissions(&marker_path, fs::Permissions::from_mode(0o600)).expect("marker mode");
        assert!(custody_activation_blocks_legacy_cli_at(&marker_path).is_err());
        let _ = fs::remove_dir_all(marker_path.ancestors().nth(2).expect("temp root"));
    }

    #[test]
    fn active_marker_denies_an_inactive_alternate_daemon_before_normal_composition() {
        let marker_path = temp_marker_path("inactive-alternate");
        let root = marker_path
            .ancestors()
            .nth(2)
            .expect("temp root")
            .to_path_buf();
        let mut active = DaemonRuntimeConfig::linux_packaged();
        active.custody.enabled = true;
        ensure_custody_activation_marker_at(&marker_path, &active.custody, "2026-09-05T12:00:00Z")
            .expect("active marker");
        let inactive_alternate = DaemonRuntimeConfig::linux_packaged();
        let normal_registry = root.join("normal/stores.json");
        let capacity_state = root.join("normal/capacity.json");
        let normal_service_state = root.join("normal/garage.started");

        let composition = (|| -> Result<(), String> {
            validate_daemon_custody_activation_at(
                &marker_path,
                &inactive_alternate.custody,
                "2026-09-05T12:01:00Z",
            )?;
            // These model the registry, capacity, and normal service work
            // that production startup performs only after the guard returns.
            fs::create_dir_all(normal_registry.parent().expect("normal parent"))
                .map_err(|error| error.to_string())?;
            fs::write(&normal_registry, b"registry").map_err(|error| error.to_string())?;
            fs::write(&capacity_state, b"capacity").map_err(|error| error.to_string())?;
            fs::write(&normal_service_state, b"service").map_err(|error| error.to_string())?;
            Ok(())
        })();

        assert!(composition.is_err());
        assert!(!normal_registry.exists());
        assert!(!capacity_state.exists());
        assert!(!normal_service_state.exists());
        let _ = fs::remove_dir_all(root);
    }
}
