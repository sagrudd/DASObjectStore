use crate::auth::RemoteS3Credentials;
use crate::config::RemoteConfig;
use dasobjectstore_core::remote_upload::RemoteUploadBackpressurePolicy;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AccessibleStore {
    pub bucket: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AwsS3Operation {
    ListStores,
    UploadFile {
        source: PathBuf,
        destination: String,
    },
    UploadFolder {
        source: PathBuf,
        destination: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AwsS3CredentialSource {
    AwsProfile,
    Environment,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AwsS3CommandPlan {
    pub program: String,
    pub args: Vec<String>,
    pub operation: AwsS3Operation,
    pub backpressure_policy: RemoteUploadBackpressurePolicy,
}

impl AwsS3CommandPlan {
    pub fn display_command(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .map(shell_quote)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

pub fn plan_list_stores(config: &RemoteConfig) -> AwsS3CommandPlan {
    AwsS3CommandPlan {
        program: "aws".to_string(),
        args: vec![
            "--profile".to_string(),
            config.profile.clone(),
            "--endpoint-url".to_string(),
            config.endpoint_url.clone(),
            "s3api".to_string(),
            "list-buckets".to_string(),
            "--output".to_string(),
            "json".to_string(),
        ],
        operation: AwsS3Operation::ListStores,
        backpressure_policy: RemoteUploadBackpressurePolicy::default(),
    }
}

pub fn plan_upload(
    config: &RemoteConfig,
    store: &str,
    source: &Path,
    prefix: Option<&str>,
    key: Option<&str>,
    dry_run: bool,
    progress: bool,
) -> Result<AwsS3CommandPlan, RemoteS3Error> {
    plan_upload_with_credentials(
        config,
        store,
        source,
        prefix,
        key,
        dry_run,
        progress,
        AwsS3CredentialSource::AwsProfile,
    )
}

pub fn plan_upload_with_credentials(
    config: &RemoteConfig,
    store: &str,
    source: &Path,
    prefix: Option<&str>,
    key: Option<&str>,
    dry_run: bool,
    progress: bool,
    credential_source: AwsS3CredentialSource,
) -> Result<AwsS3CommandPlan, RemoteS3Error> {
    let metadata = std::fs::metadata(source)?;
    validate_store_name(store)?;
    if metadata.is_file() {
        let object_key = file_destination_key(source, prefix, key)?;
        let destination = format!("s3://{store}/{object_key}");
        let mut args = aws_base_args(config, credential_source);
        args.extend(["s3".to_string(), "cp".to_string()]);
        if dry_run {
            args.push("--dryrun".to_string());
        }
        if !progress {
            args.push("--no-progress".to_string());
        }
        args.push(source.display().to_string());
        args.push(destination.clone());
        Ok(AwsS3CommandPlan {
            program: "aws".to_string(),
            args,
            operation: AwsS3Operation::UploadFile {
                source: source.to_path_buf(),
                destination,
            },
            backpressure_policy: RemoteUploadBackpressurePolicy::default(),
        })
    } else if metadata.is_dir() {
        if key.is_some() {
            return Err(RemoteS3Error::InvalidUpload(
                "--key is only valid when uploading a single file".to_string(),
            ));
        }
        let destination = folder_destination_uri(store, prefix);
        let mut args = aws_base_args(config, credential_source);
        args.extend(["s3".to_string(), "sync".to_string()]);
        if dry_run {
            args.push("--dryrun".to_string());
        }
        if !progress {
            args.push("--no-progress".to_string());
        }
        args.push(source.display().to_string());
        args.push(destination.clone());
        Ok(AwsS3CommandPlan {
            program: "aws".to_string(),
            args,
            operation: AwsS3Operation::UploadFolder {
                source: source.to_path_buf(),
                destination,
            },
            backpressure_policy: RemoteUploadBackpressurePolicy::default(),
        })
    } else {
        Err(RemoteS3Error::InvalidUpload(format!(
            "{} is neither a regular file nor a directory",
            source.display()
        )))
    }
}

pub fn execute_aws_plan(
    plan: &AwsS3CommandPlan,
    credentials: Option<&RemoteS3Credentials>,
) -> Result<String, RemoteS3Error> {
    let mut command = Command::new(&plan.program);
    command.args(&plan.args);
    if let Some(credentials) = credentials {
        command
            .env("AWS_ACCESS_KEY_ID", &credentials.access_key_id)
            .env("AWS_SECRET_ACCESS_KEY", &credentials.secret_access_key);
        if let Some(session_token) = &credentials.session_token {
            command.env("AWS_SESSION_TOKEN", session_token);
        }
    }
    let output = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;
    if !output.status.success() {
        return Err(RemoteS3Error::AwsFailed(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub fn parse_list_buckets(raw: &str) -> Result<Vec<AccessibleStore>, RemoteS3Error> {
    let parsed: ListBucketsResponse = serde_json::from_str(raw)?;
    Ok(parsed
        .buckets
        .into_iter()
        .map(|bucket| AccessibleStore {
            bucket: bucket.name,
            created_at: bucket.creation_date,
        })
        .collect())
}

fn aws_base_args(config: &RemoteConfig, credential_source: AwsS3CredentialSource) -> Vec<String> {
    let mut args = Vec::new();
    if credential_source == AwsS3CredentialSource::AwsProfile {
        args.extend(["--profile".to_string(), config.profile.clone()]);
    }
    args.extend(["--endpoint-url".to_string(), config.endpoint_url.clone()]);
    args
}

fn file_destination_key(
    source: &Path,
    prefix: Option<&str>,
    key: Option<&str>,
) -> Result<String, RemoteS3Error> {
    if let Some(key) = normalized_optional_segment(key) {
        validate_object_key(&key)?;
        return Ok(key);
    }
    let filename = source
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            RemoteS3Error::InvalidUpload(format!(
                "{} does not have a valid filename",
                source.display()
            ))
        })?;
    let prefix = normalized_optional_segment(prefix);
    let key = match prefix {
        Some(prefix) => format!("{prefix}/{filename}"),
        None => filename.to_string(),
    };
    validate_object_key(&key)?;
    Ok(key)
}

fn folder_destination_uri(store: &str, prefix: Option<&str>) -> String {
    match normalized_optional_segment(prefix) {
        Some(prefix) => format!("s3://{store}/{prefix}/"),
        None => format!("s3://{store}/"),
    }
}

fn normalized_optional_segment(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .map(|value| value.trim_matches('/'))
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn validate_store_name(value: &str) -> Result<(), RemoteS3Error> {
    let valid = !value.is_empty()
        && value.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '-'
                || character == '.'
        });
    if valid {
        Ok(())
    } else {
        Err(RemoteS3Error::InvalidUpload(
            "store/bucket names must contain lowercase letters, digits, dots, and hyphens only"
                .to_string(),
        ))
    }
}

fn validate_object_key(value: &str) -> Result<(), RemoteS3Error> {
    if value.is_empty() || value.contains('\0') {
        return Err(RemoteS3Error::InvalidUpload(
            "object key must not be blank".to_string(),
        ));
    }
    Ok(())
}

fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "-_./:=@%+".contains(character))
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ListBucketsResponse {
    #[serde(default)]
    buckets: Vec<ListBucket>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ListBucket {
    name: String,
    creation_date: Option<String>,
}

#[derive(Debug)]
pub enum RemoteS3Error {
    Io(std::io::Error),
    Json(serde_json::Error),
    AwsFailed(String),
    InvalidUpload(String),
}

impl fmt::Display for RemoteS3Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Json(error) => write!(formatter, "{error}"),
            Self::AwsFailed(message) if message.is_empty() => formatter.write_str("aws CLI failed"),
            Self::AwsFailed(message) => write!(formatter, "aws CLI failed: {message}"),
            Self::InvalidUpload(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for RemoteS3Error {}

impl From<std::io::Error> for RemoteS3Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for RemoteS3Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        parse_list_buckets, plan_upload, plan_upload_with_credentials, AwsS3CredentialSource,
        AwsS3Operation,
    };
    use crate::auth::RemoteAuthAuthority;
    use crate::config::RemoteConfig;
    use std::fs;

    #[test]
    fn plans_file_upload_to_prefix() {
        let root = temp_root("remote-file");
        let file = root.join("sample.fastq.gz");
        fs::create_dir_all(&root).expect("create temp");
        fs::write(&file, b"ACGT").expect("write file");

        let plan = plan_upload(
            &config(),
            "dos-generated",
            &file,
            Some("runs/001"),
            None,
            false,
            false,
        )
        .expect("upload plan");

        assert!(matches!(plan.operation, AwsS3Operation::UploadFile { .. }));
        assert!(plan.args.contains(&"cp".to_string()));
        assert!(plan.args.contains(&"--no-progress".to_string()));
        assert_eq!(plan.backpressure_policy.max_s3_transfer_concurrency, 2);
        assert_eq!(
            plan.args.last().unwrap(),
            "s3://dos-generated/runs/001/sample.fastq.gz"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn plans_folder_upload_as_sync() {
        let root = temp_root("remote-folder");
        fs::create_dir_all(root.join("nested")).expect("create temp");

        let plan = plan_upload(
            &config(),
            "dos-generated",
            &root,
            Some("runs/001/"),
            None,
            true,
            true,
        )
        .expect("upload plan");

        assert!(matches!(
            plan.operation,
            AwsS3Operation::UploadFolder { .. }
        ));
        assert!(plan.args.contains(&"sync".to_string()));
        assert!(plan.args.contains(&"--dryrun".to_string()));
        assert_eq!(plan.args.last().unwrap(), "s3://dos-generated/runs/001/");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn session_credential_upload_plan_omits_aws_profile() {
        let root = temp_root("remote-session-credentials");
        let file = root.join("sample.fastq.gz");
        fs::create_dir_all(&root).expect("create temp");
        fs::write(&file, b"ACGT").expect("write file");

        let plan = plan_upload_with_credentials(
            &config(),
            "dos-generated",
            &file,
            None,
            None,
            true,
            true,
            AwsS3CredentialSource::Environment,
        )
        .expect("upload plan");

        assert!(!plan.args.contains(&"--profile".to_string()));
        assert!(plan.args.contains(&"--endpoint-url".to_string()));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn parses_list_buckets_response() {
        let stores = parse_list_buckets(
            r#"{"Buckets":[{"Name":"dos-a","CreationDate":"2026-01-01T00:00:00Z"}]}"#,
        )
        .expect("parse buckets");

        assert_eq!(stores[0].bucket, "dos-a");
        assert_eq!(
            stores[0].created_at.as_deref(),
            Some("2026-01-01T00:00:00Z")
        );
    }

    fn config() -> RemoteConfig {
        RemoteConfig {
            endpoint_url: "http://192.168.1.192:3900".to_string(),
            region: "garage".to_string(),
            profile: "dasobjectstore".to_string(),
            auth_authority: RemoteAuthAuthority::AwsProfile,
            username: None,
            credential_helper: None,
            default_appliance_id: None,
            paired_appliances: Vec::new(),
        }
    }

    fn temp_root(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dasobjectstore-remote-{name}-{}-{nanos}",
            std::process::id()
        ))
    }
}
