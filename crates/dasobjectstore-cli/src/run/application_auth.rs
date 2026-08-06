//! CLI bridge for daemon-owned application credential operations.
//!
//! Requests are loaded from path-free JSON.  The CLI deliberately does not
//! accept private keys or mint credentials locally; registration, revocation,
//! proof verification, and issuance remain daemon authorities.

use super::{CliError, DaemonClient, DaemonRuntimeConfig, UnixSocketDaemonTransport};
use crate::cli::{ApplicationAuthArgs, ApplicationAuthCommand};
use dasobjectstore_daemon::api::{
    ApplicationAccessTokenExchangeRequest, GovernedBindingAuthorityAdmissionRequest,
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
        ApplicationAuthCommand::RegisterIdentity(_) => {
            reject_direct_human_authority("application identity registration")
        }
        ApplicationAuthCommand::RegisterKey(_) => {
            reject_direct_human_authority("application key registration")
        }
        ApplicationAuthCommand::Revoke(_) => {
            reject_direct_human_authority("application credential revocation")
        }
        ApplicationAuthCommand::TrustBinding(request) => {
            let json = request.json();
            let request: GovernedBindingAuthorityAdmissionRequest =
                read_request(request.request())?;
            let response = client().admit_governed_binding_authority(request)?;
            if json {
                serde_json::to_writer_pretty(&mut *writer, &response)?;
                writer.write_all(b"\n")?;
            } else {
                writeln!(writer, "Governed binding authority admitted")?;
                writeln!(writer, "Binding: {}", response.binding_id)?;
                writeln!(writer, "ObjectStore: {}", response.object_store_id)?;
                writeln!(writer, "Dry run: {}", response.dry_run)?;
            }
            Ok(())
        }
    }
}

fn reject_direct_human_authority(operation: &str) -> Result<(), CliError> {
    Err(CliError::CommandFailed(format!(
        "{operation} must be submitted through Monas using the fixed DAS service peer and a verified Pistis subject"
    )))
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
    use super::{reject_direct_human_authority, reject_secret_fields};

    #[test]
    fn request_guard_rejects_private_and_bearer_material() {
        for field in ["private_key", "secret_access_key", "bearer_token"] {
            let value = serde_json::json!({"nested": {field: "redacted"}});
            assert!(reject_secret_fields(&value).is_err(), "field {field}");
        }
    }

    #[test]
    fn direct_human_authority_operations_fail_closed() {
        for operation in [
            "application identity registration",
            "application key registration",
            "application credential revocation",
        ] {
            let error = reject_direct_human_authority(operation).expect_err("direct CLI rejected");
            assert!(error.to_string().contains("verified Pistis subject"));
        }
    }
}
