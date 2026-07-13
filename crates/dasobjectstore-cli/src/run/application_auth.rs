//! CLI bridge for daemon-owned application credential operations.
//!
//! Requests are loaded from path-free JSON.  The CLI deliberately does not
//! accept private keys or mint credentials locally; registration, revocation,
//! proof verification, and issuance remain daemon authorities.

use super::{CliError, DaemonClient, DaemonRuntimeConfig, UnixSocketDaemonTransport};
use crate::cli::{ApplicationAuthArgs, ApplicationAuthCommand};
use dasobjectstore_daemon::api::{
    ApplicationAccessTokenExchangeRequest, ApplicationCredentialRevocationRequest,
    ApplicationIdentityRegistrationRequest, ApplicationKeyRegistrationRequest,
};
use std::fs::File;
use std::io::Write;
use std::path::Path;

pub(super) fn run_application_auth(
    args: &ApplicationAuthArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    match args.command() {
        ApplicationAuthCommand::Exchange(request) => {
            let json = request.json();
            let request: ApplicationAccessTokenExchangeRequest = read_request(request.request())?;
            let response = client().exchange_application_access_token(request)?;
            if json {
                serde_json::to_writer_pretty(&mut *writer, &response)?;
                writer.write_all(b"\n")?;
            } else {
                writeln!(writer, "Application access-token exchange accepted")?;
                writeln!(writer, "Application: {}", response.claims.application_id)?;
                writeln!(writer, "Audience: {}", response.claims.audience)?;
                writeln!(writer, "Token claim: {}", response.claims.token_id)?;
                writeln!(
                    writer,
                    "Expires: {}",
                    response.claims.expires_at_unix_seconds
                )?;
            }
            Ok(())
        }
        ApplicationAuthCommand::RegisterIdentity(request) => {
            let json = request.json();
            let request: ApplicationIdentityRegistrationRequest = read_request(request.request())?;
            let response = client().register_application_identity(request)?;
            if json {
                serde_json::to_writer_pretty(&mut *writer, &response)?;
                writer.write_all(b"\n")?;
            } else {
                writeln!(writer, "Application identity registration accepted")?;
                writeln!(writer, "Application: {}", response.identity.application_id)?;
                writeln!(writer, "Replaced: {}", response.replaced)?;
                writeln!(writer, "Job: {}", response.accepted.job_id)?;
            }
            Ok(())
        }
        ApplicationAuthCommand::RegisterKey(request) => {
            let json = request.json();
            let request: ApplicationKeyRegistrationRequest = read_request(request.request())?;
            let response = client().register_application_key(request)?;
            if json {
                serde_json::to_writer_pretty(&mut *writer, &response)?;
                writer.write_all(b"\n")?;
            } else {
                writeln!(writer, "Application key registration accepted")?;
                writeln!(writer, "Application: {}", response.key.application_id)?;
                writeln!(writer, "Key: {}", response.key.key_id)?;
                writeln!(writer, "Replaced: {}", response.replaced)?;
                writeln!(writer, "Job: {}", response.accepted.job_id)?;
            }
            Ok(())
        }
        ApplicationAuthCommand::Revoke(request) => {
            let json = request.json();
            let request: ApplicationCredentialRevocationRequest = read_request(request.request())?;
            let response = client().revoke_application_credential(request)?;
            if json {
                serde_json::to_writer_pretty(&mut *writer, &response)?;
                writer.write_all(b"\n")?;
            } else {
                writeln!(writer, "Application credential revocation accepted")?;
                writeln!(writer, "Application: {}", response.application_id)?;
                if let Some(key_id) = response.key_id {
                    writeln!(writer, "Key: {key_id}")?;
                }
                writeln!(writer, "Revoked: {}", response.revoked)?;
                writeln!(writer, "Job: {}", response.accepted.job_id)?;
            }
            Ok(())
        }
    }
}

fn client() -> DaemonClient<UnixSocketDaemonTransport> {
    let config = DaemonRuntimeConfig::default_packaged();
    DaemonClient::new(UnixSocketDaemonTransport::new(config.socket_path))
}

fn read_request<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, CliError> {
    let file = File::open(path)?;
    let value: serde_json::Value = serde_json::from_reader(file).map_err(CliError::Json)?;
    reject_secret_fields(&value)?;
    serde_json::from_value(value).map_err(CliError::Json)
}

fn reject_secret_fields(value: &serde_json::Value) -> Result<(), CliError> {
    const FORBIDDEN_KEYS: &[&str] = &[
        "private_key",
        "private_key_material",
        "secret_key",
        "secret_access_key",
        "bearer_token",
        "access_token",
        "renewal_token",
    ];
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                if FORBIDDEN_KEYS.contains(&key.as_str()) {
                    return Err(CliError::CommandFailed(format!(
                        "application-auth request must not contain secret field `{key}`"
                    )));
                }
                reject_secret_fields(child)?;
            }
        }
        serde_json::Value::Array(values) => {
            for child in values {
                reject_secret_fields(child)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::reject_secret_fields;

    #[test]
    fn request_guard_rejects_private_and_bearer_material() {
        for field in ["private_key", "secret_access_key", "bearer_token"] {
            let value = serde_json::json!({"nested": {field: "redacted"}});
            assert!(reject_secret_fields(&value).is_err(), "field {field}");
        }
    }
}
