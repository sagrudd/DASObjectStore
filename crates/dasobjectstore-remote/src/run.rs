use crate::auth::{
    request_s3_credentials, RemoteAuthAuthority, RemoteAuthError, RemoteS3Credentials,
};
use crate::authenticate::{
    authenticate, prepare_appliance_trust, RemoteAuthenticateError, RemoteConnectionContext,
};
use crate::aws_profile::{
    default_profile_name, install_profile, restore_profile_state, snapshot_profile_state,
    status as s3_profile_status, AwsProfileAssociation, AwsProfileError,
};
use crate::cli::{
    AuthenticateArgs, ConfigCommand, EasyconnectArgs, ObjectReconcileS3Args, ObjectSnapshotArgs,
    ObjectsCommand, OperationStatusArgs, OperationWaitArgs, OperationsCommand, RemoteCli,
    RemoteCommand, S3Command, StoreListArgs, StoreReadinessArgs, StoresCommand, TrustCommand,
    UploadArgs,
};
use crate::config::{
    acquire_config_transaction, default_config_path, doctor_config, read_optional_config,
    repair_config, write_config, write_config_locked, RemoteConfig, RemoteConfigError,
    RemoteConfigOverrides, RemoteObjectStoreGrant, RemotePairedAppliance, RemoteSessionBinding,
    RemoteSessionCredentials, RemoteSessionRenewalMetadata, RemoteUploadSession, DEFAULT_PROFILE,
    DEFAULT_REGION, REMOTE_CONFIG_SCHEMA_VERSION,
};
use crate::control::{
    renew_store_session_if_due, ReconcileS3Request, RemoteControlClient, RemoteControlError,
};
use crate::easyconnect::{
    define_easyconnect_contract, run_complete_easyconnect_pairing_with_ready,
    RemoteEasyconnectContract, RemoteEasyconnectContractError, RemoteEasyconnectContractRequest,
    RemoteEasyconnectPairingError, RemoteEasyconnectPairingOptions, SystemBrowserLauncher,
};
use crate::s3::{
    execute_aws_plan, parse_list_buckets, plan_list_stores, plan_upload_with_credentials,
    AwsS3CredentialSource, RemoteS3Error,
};
use dasobjectstore_core::utc::parse_canonical_utc_timestamp_seconds as parse_rfc3339_utc_seconds;
use dasobjectstore_daemon::{
    DaemonClient, DaemonClientError, DaemonClientTransport, DaemonJobEvent, DaemonJobSummary,
    RemoteEasyconnectAwsCliEnvironmentVariable, RemoteEasyconnectSubmitAwsCliUploadRequest,
    RemoteEasyconnectSubmitAwsCliUploadResponse, RemoteEasyconnectUploadProgressTelemetry,
    UnixSocketDaemonTransport, DEFAULT_DAEMON_SOCKET_FILE_NAME, LINUX_DAEMON_RUNTIME_DIR,
};
use std::fmt;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn run(cli: &RemoteCli, writer: &mut impl Write) -> Result<(), RemoteRunError> {
    match cli.command() {
        RemoteCommand::Authenticate(args) => run_authenticate(cli, args, writer),
        RemoteCommand::Trust(args) => match args.command() {
            TrustCommand::Enroll(args) => run_trust_enroll(
                args.host_or_ip(),
                args.https_port(),
                args.fingerprint(),
                args.tls_server_name(),
                writer,
            ),
            TrustCommand::Inspect(args) => {
                run_trust_inspect(args.host_or_ip(), args.https_port(), args.json(), writer)
            }
            TrustCommand::List(args) => run_trust_list(args.json(), writer),
            TrustCommand::Remove(args) => run_trust_remove(args.appliance_id(), args.yes(), writer),
            TrustCommand::Rotate(args) => {
                run_trust_rotate(args.appliance_id(), args.trust_fingerprint(), writer)
            }
        },
        RemoteCommand::S3(args) => match args.command() {
            S3Command::Status(args) => {
                run_s3_status(cli, args.store(), args.profile(), args.json(), writer)
            }
        },
        RemoteCommand::Easyconnect(args) => run_easyconnect(cli, args, writer),
        RemoteCommand::Config(args) => match args.command() {
            ConfigCommand::Set(args) => run_config_set(cli, args, writer),
            ConfigCommand::Show(args) => run_config_show(cli, args.json(), writer),
            ConfigCommand::Doctor(args) => run_config_doctor(cli, args.json(), writer),
            ConfigCommand::Repair(args) => {
                run_config_repair(cli, args.apply(), args.json(), writer)
            }
        },
        RemoteCommand::Stores(args) => match args.command() {
            StoresCommand::List(args) => run_store_list(cli, args, writer),
            StoresCommand::Readiness(args) => run_store_readiness(cli, args, writer),
        },
        RemoteCommand::Objects(args) => match args.command() {
            ObjectsCommand::Snapshot(args) => run_object_snapshot(cli, args, writer),
            ObjectsCommand::GroupStatus(args) => {
                let config = resolved_control_config(cli, args.store())?;
                let (client, _) = RemoteControlClient::for_store(&config, args.store(), false)?;
                write_control_json(
                    client.group_status(args.store(), args.key())?,
                    args.json(),
                    writer,
                )
            }
            ObjectsCommand::ReconcileS3(args) => run_object_reconcile(cli, args, writer),
        },
        RemoteCommand::Operations(args) => match args.command() {
            OperationsCommand::Status(args) => run_operation_status(cli, args, writer),
            OperationsCommand::Wait(args) => run_operation_wait(cli, args, writer),
        },
        RemoteCommand::Upload(args) => run_upload(cli, args, writer),
    }
}

mod authentication;
mod config_commands;
mod control_commands;
mod upload;

use authentication::*;
use config_commands::*;
use control_commands::*;
use upload::*;

#[derive(Debug)]
pub enum RemoteRunError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Config(RemoteConfigError),
    Easyconnect(RemoteEasyconnectContractError),
    EasyconnectPairing(RemoteEasyconnectPairingError),
    Auth(RemoteAuthError),
    Authenticate(RemoteAuthenticateError),
    Trust(crate::trust::TrustError),
    Control(RemoteControlError),
    S3(RemoteS3Error),
    AwsProfile(AwsProfileError),
    Daemon(DaemonClientError),
    Clock(String),
    UploadRouting(String),
}

impl fmt::Display for RemoteRunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Json(error) => write!(formatter, "{error}"),
            Self::Config(error) => write!(formatter, "{error}"),
            Self::Easyconnect(error) => write!(formatter, "{error}"),
            Self::EasyconnectPairing(error) => write!(formatter, "{error}"),
            Self::Auth(error) => write!(formatter, "{error}"),
            Self::Authenticate(error) => write!(formatter, "{error}"),
            Self::Trust(error) => write!(formatter, "{error}"),
            Self::Control(error) => write!(formatter, "{error}"),
            Self::S3(error) => write!(formatter, "{error}"),
            Self::AwsProfile(error) => write!(formatter, "{error}"),
            Self::Daemon(error) => write!(formatter, "{error}"),
            Self::Clock(error) => write!(formatter, "{error}"),
            Self::UploadRouting(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for RemoteRunError {}

impl From<std::io::Error> for RemoteRunError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for RemoteRunError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<RemoteConfigError> for RemoteRunError {
    fn from(error: RemoteConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<RemoteEasyconnectContractError> for RemoteRunError {
    fn from(error: RemoteEasyconnectContractError) -> Self {
        Self::Easyconnect(error)
    }
}

impl From<RemoteEasyconnectPairingError> for RemoteRunError {
    fn from(error: RemoteEasyconnectPairingError) -> Self {
        Self::EasyconnectPairing(error)
    }
}

impl From<RemoteAuthError> for RemoteRunError {
    fn from(error: RemoteAuthError) -> Self {
        Self::Auth(error)
    }
}

impl From<RemoteAuthenticateError> for RemoteRunError {
    fn from(error: RemoteAuthenticateError) -> Self {
        Self::Authenticate(error)
    }
}

impl From<crate::trust::TrustError> for RemoteRunError {
    fn from(error: crate::trust::TrustError) -> Self {
        Self::Trust(error)
    }
}

impl From<AwsProfileError> for RemoteRunError {
    fn from(error: AwsProfileError) -> Self {
        Self::AwsProfile(error)
    }
}

impl From<RemoteControlError> for RemoteRunError {
    fn from(error: RemoteControlError) -> Self {
        Self::Control(error)
    }
}

impl From<RemoteS3Error> for RemoteRunError {
    fn from(error: RemoteS3Error) -> Self {
        Self::S3(error)
    }
}

#[cfg(test)]
mod tests;
