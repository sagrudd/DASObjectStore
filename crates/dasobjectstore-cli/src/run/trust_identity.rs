use crate::cli::TrustIdentityArgs;
use crate::run::CliError;
use base64::Engine;
use dasobjectstore_core::DEFAULT_STANDALONE_CONFIG_PATH;
use dasobjectstore_gui_api::{
    load_appliance_identity, ApplianceIdentityRecord, StandaloneServerConfig,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use x509_parser::extensions::GeneralName;
use x509_parser::parse_x509_certificate;

#[derive(Serialize)]
struct ApplianceTrustIdentityReport {
    schema_version: &'static str,
    appliance_id: String,
    certificate_fingerprint_sha256: String,
    certificate_subject: String,
    certificate_issuer: String,
    subject_alt_names: Vec<String>,
    not_before: String,
    not_after: String,
}

pub(super) fn run_trust_identity(
    args: &TrustIdentityArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let config: StandaloneServerConfig =
        serde_json::from_slice(&fs::read(DEFAULT_STANDALONE_CONFIG_PATH)?)?;
    let identity = load_appliance_identity(
        &dasobjectstore_daemon::DaemonRuntimeConfig::default_packaged().state_dir,
    )?;
    let report = inspect_identity(&identity, &fs::read(&config.tls.certificate_path)?)?;
    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &report)?;
        writer.write_all(b"\n")?;
    } else {
        writeln!(writer, "Appliance ID: {}", report.appliance_id)?;
        writeln!(
            writer,
            "Certificate SHA-256: {}",
            report.certificate_fingerprint_sha256
        )?;
        writeln!(writer, "Subject: {}", report.certificate_subject)?;
        writeln!(writer, "Issuer: {}", report.certificate_issuer)?;
        writeln!(writer, "SANs: {}", report.subject_alt_names.join(", "))?;
        writeln!(
            writer,
            "Validity: {} through {}",
            report.not_before, report.not_after
        )?;
    }
    Ok(())
}

fn inspect_identity(
    identity: &ApplianceIdentityRecord,
    certificate_pem: &[u8],
) -> Result<ApplianceTrustIdentityReport, CliError> {
    let der = decode_first_certificate(certificate_pem)?;
    let (_, certificate) = parse_x509_certificate(&der).map_err(|error| {
        CliError::CommandFailed(format!("invalid appliance TLS certificate: {error}"))
    })?;
    let subject_alt_names = certificate
        .subject_alternative_name()
        .map_err(|error| CliError::CommandFailed(format!("invalid certificate SAN: {error}")))?
        .map(|extension| {
            extension
                .value
                .general_names
                .iter()
                .filter_map(|name| match name {
                    GeneralName::DNSName(value) => Some(format!("DNS:{value}")),
                    GeneralName::IPAddress(bytes) if bytes.len() == 4 => Some(format!(
                        "IP:{}.{}.{}.{}",
                        bytes[0], bytes[1], bytes[2], bytes[3]
                    )),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(ApplianceTrustIdentityReport {
        schema_version: "dasobjectstore.trust_identity.v1",
        appliance_id: identity.appliance_id.clone(),
        certificate_fingerprint_sha256: Sha256::digest(&der)
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(":"),
        certificate_subject: certificate.subject().to_string(),
        certificate_issuer: certificate.issuer().to_string(),
        subject_alt_names,
        not_before: certificate.validity().not_before.to_string(),
        not_after: certificate.validity().not_after.to_string(),
    })
}

fn decode_first_certificate(pem: &[u8]) -> Result<Vec<u8>, CliError> {
    let text = std::str::from_utf8(pem)
        .map_err(|_| CliError::CommandFailed("certificate PEM is not UTF-8".to_string()))?;
    let body = text
        .split("-----BEGIN CERTIFICATE-----")
        .nth(1)
        .and_then(|value| value.split("-----END CERTIFICATE-----").next())
        .ok_or_else(|| {
            CliError::CommandFailed("certificate PEM contains no certificate".to_string())
        })?
        .lines()
        .map(str::trim)
        .collect::<String>();
    base64::engine::general_purpose::STANDARD
        .decode(body)
        .map_err(|error| CliError::CommandFailed(format!("invalid certificate PEM: {error}")))
}
