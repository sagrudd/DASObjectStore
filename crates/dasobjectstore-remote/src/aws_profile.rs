//! Safe installation and verification of server-authoritative S3 sessions.

use crate::authenticate::RemoteConnectionContext;
use serde::{Deserialize, Serialize};
use std::env;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AwsProfileAssociation {
    pub profile: String,
    pub appliance_host: String,
    pub store_id: String,
    pub endpoint_url: String,
    pub bucket: String,
    pub region: String,
    pub addressing_style: String,
    pub temporary_credentials: bool,
    pub expires_at: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct S3ProfileStatus {
    pub store_id: String,
    pub profile: String,
    pub profile_present: bool,
    pub endpoint_match: bool,
    pub bucket_associated: bool,
    pub credential_expired: bool,
    pub verified: bool,
    pub reauthentication_required: bool,
    pub endpoint_url: String,
    pub bucket: String,
    pub region: String,
    pub addressing_style: String,
}

#[derive(Debug)]
pub enum AwsProfileError {
    Io(io::Error),
    Invalid(String),
    Conflict(String),
    Compatibility(String),
    Verification(String),
}

impl fmt::Display for AwsProfileError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(out, "AWS profile update failed: {error}"),
            Self::Invalid(message)
            | Self::Conflict(message)
            | Self::Compatibility(message)
            | Self::Verification(message) => out.write_str(message),
        }
    }
}

impl std::error::Error for AwsProfileError {}
impl From<io::Error> for AwsProfileError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

pub fn default_profile_name(store: &str) -> Result<String, AwsProfileError> {
    validate_profile_component(store)?;
    Ok(format!("dasobjectstore-{store}"))
}

pub fn install_profile(
    context: &RemoteConnectionContext,
    profile: &str,
    existing: Option<&AwsProfileAssociation>,
    force: bool,
    verify: bool,
) -> Result<(AwsProfileAssociation, bool), AwsProfileError> {
    validate_profile_component(profile)?;
    if let Some(existing) = existing {
        let same = existing.store_id == context.object_store
            && existing.endpoint_url == context.endpoint_url
            && existing.bucket == context.bucket
            && existing.profile == profile;
        if !same && !force {
            return Err(AwsProfileError::Conflict(format!(
                "AWS profile {profile} is associated with a different endpoint, ObjectStore, or bucket; pass --force to replace it"
            )));
        }
    }
    let paths = AwsPaths::discover()?;
    let _locks = lock_paths(&paths)?;
    let config_existed = paths.config.exists();
    let credentials_existed = paths.credentials.exists();
    let config_before = read_or_empty(&paths.config)?;
    let credentials_before = read_or_empty(&paths.credentials)?;
    let config_section = if profile == "default" {
        "default".to_string()
    } else {
        format!("profile {profile}")
    };
    let config_after = replace_section(
        &config_before,
        &config_section,
        &[
            format!("region = {}", context.region),
            format!("endpoint_url = {}", context.endpoint_url),
            "s3 =".to_string(),
            format!("    addressing_style = {}", context.addressing_style),
        ],
    );
    let mut credential_lines = vec![
        format!("aws_access_key_id = {}", context.access_key_id),
        format!("aws_secret_access_key = {}", context.secret_access_key),
    ];
    if let Some(token) = &context.session_token {
        credential_lines.push(format!("aws_session_token = {token}"));
    }
    let credentials_after = replace_section(&credentials_before, profile, &credential_lines);
    atomic_pair_write(
        &paths.config,
        config_after.as_bytes(),
        &paths.credentials,
        credentials_after.as_bytes(),
    )?;
    let association = AwsProfileAssociation {
        profile: profile.to_string(),
        appliance_host: context.appliance_host.clone(),
        store_id: context.object_store.clone(),
        endpoint_url: context.endpoint_url.clone(),
        bucket: context.bucket.clone(),
        region: context.region.clone(),
        addressing_style: context.addressing_style.clone(),
        temporary_credentials: context.session_token.is_some(),
        expires_at: context
            .session_token
            .as_ref()
            .map(|_| context.expires_at_utc.clone()),
    };
    let verified = if verify {
        if let Err(error) = verify_profile(&paths, &association) {
            restore(
                &paths.config,
                config_existed.then_some(config_before.as_bytes()),
            )?;
            restore(
                &paths.credentials,
                credentials_existed.then_some(credentials_before.as_bytes()),
            )?;
            return Err(error);
        }
        true
    } else {
        false
    };
    Ok((association, verified))
}

pub fn status(
    association: &AwsProfileAssociation,
    verify: bool,
) -> Result<S3ProfileStatus, AwsProfileError> {
    let paths = AwsPaths::discover()?;
    let config = read_or_empty(&paths.config)?;
    let section = if association.profile == "default" {
        "default".to_string()
    } else {
        format!("profile {}", association.profile)
    };
    let profile_present = has_section(&config, &section);
    let endpoint_match = section_value(&config, &section, "endpoint_url")
        .is_some_and(|value| value == association.endpoint_url);
    let credential_expired = association
        .expires_at
        .as_deref()
        .and_then(dasobjectstore_core::utc::parse_canonical_utc_timestamp_seconds)
        .is_some_and(|expires| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|now| now.as_secs() as i64 >= expires)
                .unwrap_or(true)
        });
    let verified = profile_present
        && endpoint_match
        && !credential_expired
        && (!verify || verify_profile(&paths, association).is_ok());
    Ok(S3ProfileStatus {
        store_id: association.store_id.clone(),
        profile: association.profile.clone(),
        profile_present,
        endpoint_match,
        bucket_associated: true,
        credential_expired,
        verified,
        reauthentication_required: !verified,
        endpoint_url: association.endpoint_url.clone(),
        bucket: association.bucket.clone(),
        region: association.region.clone(),
        addressing_style: association.addressing_style.clone(),
    })
}

struct AwsPaths {
    config: PathBuf,
    credentials: PathBuf,
}

pub struct AwsProfileBackup {
    config_path: PathBuf,
    credentials_path: PathBuf,
    config: Option<Vec<u8>>,
    credentials: Option<Vec<u8>>,
}

pub fn snapshot_profile_state() -> Result<AwsProfileBackup, AwsProfileError> {
    let paths = AwsPaths::discover()?;
    Ok(AwsProfileBackup {
        config_path: paths.config.clone(),
        credentials_path: paths.credentials.clone(),
        config: fs::read(&paths.config).ok(),
        credentials: fs::read(&paths.credentials).ok(),
    })
}

pub fn restore_profile_state(backup: &AwsProfileBackup) -> Result<(), AwsProfileError> {
    let paths = AwsPaths {
        config: backup.config_path.clone(),
        credentials: backup.credentials_path.clone(),
    };
    let _locks = lock_paths(&paths)?;
    restore(&paths.config, backup.config.as_deref())?;
    restore(&paths.credentials, backup.credentials.as_deref())
}
impl AwsPaths {
    fn discover() -> Result<Self, AwsProfileError> {
        let home = env::var_os("HOME")
            .or_else(|| env::var_os("USERPROFILE"))
            .ok_or_else(|| {
                AwsProfileError::Invalid(
                    "HOME is not set; AWS profile paths cannot be resolved".to_string(),
                )
            })?;
        let home = PathBuf::from(home);
        Ok(Self {
            config: nonblank_env_path("AWS_CONFIG_FILE")
                .unwrap_or_else(|| home.join(".aws/config")),
            credentials: nonblank_env_path("AWS_SHARED_CREDENTIALS_FILE")
                .unwrap_or_else(|| home.join(".aws/credentials")),
        })
    }
}

fn nonblank_env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn lock_paths(paths: &AwsPaths) -> Result<Vec<File>, AwsProfileError> {
    let mut lock_paths = vec![
        sibling_lock(&paths.config),
        sibling_lock(&paths.credentials),
    ];
    lock_paths.sort();
    lock_paths.dedup();
    let mut locks = Vec::new();
    for path in lock_paths {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(path)?;
        file.lock()?;
        locks.push(file);
    }
    Ok(locks)
}

fn sibling_lock(path: &Path) -> PathBuf {
    path.parent()
        .unwrap_or_else(|| Path::new("."))
        .join(".dasobjectstore-aws.lock")
}

fn read_or_empty(path: &Path) -> Result<String, AwsProfileError> {
    match fs::read_to_string(path) {
        Ok(raw) => Ok(raw),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error.into()),
    }
}

fn replace_section(raw: &str, section: &str, lines: &[String]) -> String {
    let header = format!("[{section}]");
    let mut output = Vec::new();
    let mut skipping = false;
    for line in raw.lines() {
        if line.trim_start().starts_with('[') && line.trim_end().ends_with(']') {
            skipping = line.trim() == header;
        }
        if !skipping {
            output.push(line.to_string());
        }
    }
    while output.last().is_some_and(|line| line.is_empty()) {
        output.pop();
    }
    if !output.is_empty() {
        output.push(String::new());
    }
    output.push(header);
    output.extend(lines.iter().cloned());
    output.push(String::new());
    output.join("\n")
}

fn has_section(raw: &str, section: &str) -> bool {
    raw.lines()
        .any(|line| line.trim() == format!("[{section}]"))
}

fn section_value(raw: &str, section: &str, key: &str) -> Option<String> {
    let mut inside = false;
    for line in raw.lines() {
        if line.trim_start().starts_with('[') && line.trim_end().ends_with(']') {
            inside = line.trim() == format!("[{section}]");
            continue;
        }
        if inside {
            if let Some((candidate, value)) = line.split_once('=') {
                if candidate.trim() == key {
                    return Some(value.trim().to_string());
                }
            }
        }
    }
    None
}

fn atomic_pair_write(
    first: &Path,
    first_bytes: &[u8],
    second: &Path,
    second_bytes: &[u8],
) -> Result<(), AwsProfileError> {
    let first_before = fs::read(first).ok();
    let second_before = fs::read(second).ok();
    atomic_write(first, first_bytes, 0o600)?;
    if let Err(error) = atomic_write(second, second_bytes, 0o600) {
        restore(first, first_before.as_deref())?;
        restore(second, second_before.as_deref())?;
        return Err(error);
    }
    Ok(())
}

fn restore(path: &Path, bytes: Option<&[u8]>) -> Result<(), AwsProfileError> {
    match bytes {
        Some(bytes) => atomic_write(path, bytes, 0o600),
        None => match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        },
    }
}

fn atomic_write(path: &Path, bytes: &[u8], mode: u32) -> Result<(), AwsProfileError> {
    let parent = path
        .parent()
        .ok_or_else(|| AwsProfileError::Invalid("AWS file has no parent".to_string()))?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".dasobjectstore-{}-{}.tmp",
        std::process::id(),
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&temporary, path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    if let Ok(directory) = File::open(parent) {
        let _ = directory.sync_all();
    }
    Ok(())
}

fn verify_profile(
    paths: &AwsPaths,
    association: &AwsProfileAssociation,
) -> Result<(), AwsProfileError> {
    let configured = Command::new("aws")
        .args([
            "configure",
            "get",
            "endpoint_url",
            "--profile",
            &association.profile,
        ])
        .env("AWS_CONFIG_FILE", &paths.config)
        .env("AWS_SHARED_CREDENTIALS_FILE", &paths.credentials)
        .stderr(Stdio::null())
        .output()
        .map_err(|error| {
            AwsProfileError::Compatibility(format!(
                "AWS CLI is required for profile endpoint verification: {error}"
            ))
        })?;
    if !configured.status.success()
        || String::from_utf8_lossy(&configured.stdout).trim() != association.endpoint_url
    {
        return Err(AwsProfileError::Compatibility(
            "installed AWS CLI does not honor profile-level endpoint_url; configuration was retained but cannot be used safely".to_string(),
        ));
    }
    let status = Command::new("aws")
        .args([
            "--profile",
            &association.profile,
            "--region",
            &association.region,
            "--endpoint-url",
            &association.endpoint_url,
            "--cli-connect-timeout",
            "5",
            "--cli-read-timeout",
            "10",
            "s3api",
            "list-objects-v2",
            "--bucket",
            &association.bucket,
            "--max-keys",
            "1",
            "--output",
            "json",
        ])
        .env("AWS_CONFIG_FILE", &paths.config)
        .env("AWS_SHARED_CREDENTIALS_FILE", &paths.credentials)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| {
            AwsProfileError::Verification(format!("run bounded S3 verification: {error}"))
        })?;
    if !status.success() {
        return Err(AwsProfileError::Verification(
            "S3 profile verification failed: the endpoint was unreachable, TLS validation failed, credentials expired or were rejected, or bucket access was denied".to_string(),
        ));
    }
    Ok(())
}

fn validate_profile_component(value: &str) -> Result<(), AwsProfileError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        return Err(AwsProfileError::Invalid(
            "AWS profile and ObjectStore names must use only ASCII letters, digits, dot, underscore, or hyphen".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{replace_section, section_value};

    #[test]
    fn preserves_unrelated_profiles_and_replaces_target() {
        let raw = "[other]\nkey = value\n\n[target]\nstale = yes\n";
        let updated = replace_section(raw, "target", &["fresh = yes".to_string()]);
        assert!(updated.contains("[other]\nkey = value"));
        assert!(!updated.contains("stale"));
        assert_eq!(
            section_value(&updated, "target", "fresh").as_deref(),
            Some("yes")
        );
    }
}
