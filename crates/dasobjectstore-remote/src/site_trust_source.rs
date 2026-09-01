//! Pinned-SSH acquisition for public Site Trust envelopes.
//!
//! This module deliberately has no HTTPS client. Appliance HTTPS is the
//! consumer of the resulting trust record and cannot be used to bootstrap it.
//! A package or configuration-management system installs one root-owned source
//! record per endpoint, pinning the SSH host key and the authority identity.

use crate::site_trust::{
    provision_envelope, LoadedSiteTrust, ProvisionEnvelopeRequest, SiteTrustError,
};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde::Deserialize;
use sha2::Digest as _;

const SCHEMA: &str = "dasobjectstore.remote_site_trust_source.v1";
const TRANSPORT: &str = "pinned-ssh-domain-cert-public-export-v1";
const SSH_EXPORT_USER: &str = "mnemosyne-site-trust-export";
const DEFAULT_DIRECTORY: &str = "/etc/dasobjectstore-remote/site-trust-sources.d";
const MAXIMUM_SOURCE_BYTES: u64 = 16 * 1024;
const MAXIMUM_ENVELOPE_BYTES: usize = 9_000;
const MAXIMUM_STDERR_BYTES: usize = 4_096;
const SSH_TIMEOUT: Duration = Duration::from_secs(30);

/// Configuration for a single independently authenticated provisioning
/// source. The record is deliberately package/configuration-managed rather
/// than supplied by the person running the login command.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PinnedSshProvisioningSource {
    schema: String,
    endpoint_host: String,
    endpoint_port: u16,
    transport: String,
    ssh_host: String,
    ssh_port: u16,
    ssh_user: String,
    ssh_known_hosts_file: PathBuf,
    ssh_identity_file: PathBuf,
    site_uuid: String,
}

pub struct ConfiguredProvisionRequest<'a> {
    pub host: &'a str,
    pub port: u16,
    pub source: Option<&'a Path>,
    pub output: Option<&'a Path>,
}

/// Provision a Site Trust record through the endpoint's configured pinned SSH
/// source. There is intentionally no fallback to the appliance HTTPS URL.
pub fn provision_from_configured_source(
    request: ConfiguredProvisionRequest<'_>,
) -> Result<LoadedSiteTrust, SiteTrustError> {
    let source = load_source(request.host, request.port, request.source)?;
    provision_from_source_with_channel(
        request.host,
        request.port,
        request.output,
        &source,
        &PinnedSshChannel,
    )
}

fn provision_from_source_with_channel(
    host: &str,
    port: u16,
    output: Option<&Path>,
    source: &PinnedSshProvisioningSource,
    channel: &impl ProvisioningChannel,
) -> Result<LoadedSiteTrust, SiteTrustError> {
    validate_source_for_endpoint(source, host, port)?;
    let envelope = channel.fetch(source).map_err(map_fetch_error)?;
    if envelope.len() > MAXIMUM_ENVELOPE_BYTES {
        return Err(SiteTrustError::ProvisioningChannel(
            "pinned SSH provisioning response exceeds the bounded PXCE/v1 envelope size"
                .to_string(),
        ));
    }
    let envelope_sha256 = sha2::Sha256::digest(&envelope);
    provision_envelope(ProvisionEnvelopeRequest {
        host,
        port,
        site_uuid_hex: &source.site_uuid,
        envelope: &envelope,
        authenticated_envelope_sha256_hex: &hex::encode(envelope_sha256),
        output,
    })
}

fn load_source(
    host: &str,
    port: u16,
    explicit_path: Option<&Path>,
) -> Result<PinnedSshProvisioningSource, SiteTrustError> {
    // This command intentionally has no source-record import or creation
    // path. Creating a pin or provisioning identity from caller-supplied data
    // would make an untrusted local input the root of the HTTPS trust chain.
    // An independently authenticated organisation deployment channel owns the
    // source record and its adjacent known-hosts/identity files.
    let path = explicit_path
        .map(Path::to_path_buf)
        .unwrap_or(default_source_path(host, port)?);
    if !path.exists() {
        return Err(SiteTrustError::ProvisioningSourceNotConfigured {
            host: host.to_string(),
            port,
            path,
        });
    }
    let bytes = read_root_owned_file(&path, MAXIMUM_SOURCE_BYTES, "provisioning source")?;
    let source: PinnedSshProvisioningSource = serde_json::from_slice(&bytes).map_err(|_| {
        SiteTrustError::Invalid("pinned SSH Site Trust source record is malformed".to_string())
    })?;
    validate_source_for_endpoint(&source, host, port)?;
    read_root_owned_file(
        &source.ssh_known_hosts_file,
        MAXIMUM_SOURCE_BYTES,
        "pinned SSH known-hosts file",
    )?;
    read_safe_identity_file(&source.ssh_identity_file)?;
    Ok(source)
}

fn validate_source_for_endpoint(
    source: &PinnedSshProvisioningSource,
    host: &str,
    port: u16,
) -> Result<(), SiteTrustError> {
    if source.schema != SCHEMA {
        return Err(SiteTrustError::Invalid(
            "pinned SSH Site Trust source record schema is unsupported".to_string(),
        ));
    }
    if source.transport != TRANSPORT {
        return Err(SiteTrustError::Invalid(
            "pinned SSH Site Trust source is required; refusing untrusted HTTPS bootstrap"
                .to_string(),
        ));
    }
    if source.endpoint_host != canonical_host(host)? || source.endpoint_port != port {
        return Err(SiteTrustError::Invalid(
            "pinned SSH Site Trust source is configured for a different HTTPS endpoint".to_string(),
        ));
    }
    canonical_host(&source.ssh_host)?;
    if source.ssh_port == 0 {
        return Err(SiteTrustError::Invalid(
            "pinned SSH Site Trust source port must not be zero".to_string(),
        ));
    }
    if source.ssh_user != SSH_EXPORT_USER {
        return Err(SiteTrustError::Invalid(
            "pinned SSH Site Trust source must use the constrained mnemosyne-site-trust-export user"
                .to_string(),
        ));
    }
    let site_uuid = hex::decode(&source.site_uuid).map_err(|_| {
        SiteTrustError::Invalid(
            "pinned SSH Site Trust source Site UUID is not hexadecimal".to_string(),
        )
    })?;
    if site_uuid.len() != 16 || hex::encode(&site_uuid) != source.site_uuid {
        return Err(SiteTrustError::Invalid(
            "pinned SSH Site Trust source Site UUID must be canonical lower-case 32-hex"
                .to_string(),
        ));
    }
    if !source.ssh_known_hosts_file.is_absolute() || !source.ssh_identity_file.is_absolute() {
        return Err(SiteTrustError::Invalid(
            "pinned SSH Site Trust source paths must be absolute".to_string(),
        ));
    }
    Ok(())
}

trait ProvisioningChannel {
    fn fetch(&self, source: &PinnedSshProvisioningSource) -> Result<Vec<u8>, FetchError>;
}

struct PinnedSshChannel;

impl ProvisioningChannel for PinnedSshChannel {
    fn fetch(&self, source: &PinnedSshProvisioningSource) -> Result<Vec<u8>, FetchError> {
        run_bounded_ssh(pinned_ssh_command(source))
    }
}

fn pinned_ssh_command(source: &PinnedSshProvisioningSource) -> Command {
    let destination = format!("{}@{}", source.ssh_user, source.ssh_host);
    let mut command = Command::new("/usr/bin/ssh");
    command
        .arg("-F")
        .arg("/dev/null")
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("StrictHostKeyChecking=yes")
        .arg("-o")
        .arg(format!(
            "UserKnownHostsFile={}",
            source.ssh_known_hosts_file.display()
        ))
        .arg("-o")
        .arg("GlobalKnownHostsFile=/dev/null")
        .arg("-o")
        .arg("UpdateHostKeys=no")
        .arg("-o")
        .arg("CanonicalizeHostname=no")
        .arg("-o")
        .arg("PasswordAuthentication=no")
        .arg("-o")
        .arg("KbdInteractiveAuthentication=no")
        .arg("-o")
        .arg("ForwardAgent=no")
        .arg("-o")
        .arg("ClearAllForwardings=yes")
        .arg("-o")
        .arg("RequestTTY=no")
        .arg("-o")
        .arg("IdentitiesOnly=yes")
        .arg("-i")
        .arg(&source.ssh_identity_file)
        .arg("-p")
        .arg(source.ssh_port.to_string())
        .arg(destination)
        .arg(remote_export_script(&source.site_uuid))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn remote_export_script(site_uuid: &str) -> String {
    // The UUID is validated as canonical lower-case hexadecimal in the
    // root-owned source record before it reaches this fixed command. The
    // authority package owns this constrained exporter and its optional SSH
    // forced-command wrapper; the remote client never sends a shell fragment,
    // sudo request, or credential to the authority.
    format!("/usr/libexec/mnemosyne-domain-cert-site-trust-export-v1 {site_uuid}")
}

fn run_bounded_ssh(mut command: Command) -> Result<Vec<u8>, FetchError> {
    let mut child = command.spawn().map_err(|_| {
        FetchError::Transport("could not start pinned SSH provisioning".to_string())
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| FetchError::Transport("pinned SSH stdout was unavailable".to_string()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| FetchError::Transport("pinned SSH stderr was unavailable".to_string()))?;
    let stdout_reader = thread::spawn(move || read_bounded(stdout, MAXIMUM_ENVELOPE_BYTES));
    let stderr_reader = thread::spawn(move || read_bounded(stderr, MAXIMUM_STDERR_BYTES));
    let started = Instant::now();
    let (status, timed_out) = loop {
        if let Some(status) = child.try_wait().map_err(|_| {
            FetchError::Transport("could not observe pinned SSH provisioning".to_string())
        })? {
            break (status, false);
        }
        if started.elapsed() >= SSH_TIMEOUT {
            child.kill().map_err(|_| {
                FetchError::Transport(
                    "could not stop timed-out pinned SSH provisioning".to_string(),
                )
            })?;
            break (
                child.wait().map_err(|_| {
                    FetchError::Transport(
                        "could not reap timed-out pinned SSH provisioning".to_string(),
                    )
                })?,
                true,
            );
        }
        thread::sleep(Duration::from_millis(50));
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| FetchError::Transport("pinned SSH stdout reader failed".to_string()))??;
    let stderr = stderr_reader
        .join()
        .map_err(|_| FetchError::Transport("pinned SSH stderr reader failed".to_string()))??;
    if timed_out {
        return Err(FetchError::Transport(
            "pinned SSH provisioning timed out before an envelope was received".to_string(),
        ));
    }
    if stdout.overflowed {
        return Err(FetchError::Transport(
            "pinned SSH provisioning returned an oversized envelope".to_string(),
        ));
    }
    if !status.success() {
        return Err(classify_ssh_failure(&stderr.bytes));
    }
    if stdout.bytes.is_empty() {
        return Err(FetchError::Transport(
            "pinned SSH provisioning returned no Site Trust envelope".to_string(),
        ));
    }
    Ok(stdout.bytes)
}

struct BoundedRead {
    bytes: Vec<u8>,
    overflowed: bool,
}

fn read_bounded(mut reader: impl Read, maximum: usize) -> Result<BoundedRead, FetchError> {
    let mut bytes = Vec::new();
    let mut overflowed = false;
    let mut buffer = [0_u8; 4096];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|_| FetchError::Transport("could not read pinned SSH response".to_string()))?;
        if read == 0 {
            break;
        }
        let remaining = maximum.saturating_sub(bytes.len());
        bytes.extend_from_slice(&buffer[..read.min(remaining)]);
        overflowed |= read > remaining;
    }
    Ok(BoundedRead { bytes, overflowed })
}

#[derive(Clone, Debug)]
enum FetchError {
    Authentication,
    CapabilityUnavailable,
    CurrentEnvelopeUnavailable,
    Transport(String),
}

fn classify_ssh_failure(stderr: &[u8]) -> FetchError {
    let text = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    if text.contains("__das_site_trust_export_envelope_unavailable__") {
        FetchError::CurrentEnvelopeUnavailable
    } else if text.contains("__das_site_trust_export_capability_unavailable__")
        || text.contains("mnemosyne-domain-cert-site-trust-export-v1: not found")
        || text.contains("mnemosyne-domain-cert-site-trust-export-v1: command not found")
    {
        FetchError::CapabilityUnavailable
    } else if text.contains("host key verification failed")
        || text.contains("remote host identification has changed")
        || text.contains("permission denied")
        || text.contains("no supported authentication methods")
        || text.contains("identity file")
    {
        FetchError::Authentication
    } else {
        FetchError::Transport("pinned SSH provisioning command failed".to_string())
    }
}

fn map_fetch_error(error: FetchError) -> SiteTrustError {
    match error {
        FetchError::Authentication => SiteTrustError::ProvisioningChannel(
            "pinned SSH host-key or authentication verification failed; Site Trust was not changed"
                .to_string(),
        ),
        FetchError::CapabilityUnavailable => SiteTrustError::ProvisioningChannel(
            "configured Site authority does not support domain-cert site-root public-export; install a qualified domain-cert release before retrying"
                .to_string(),
        ),
        FetchError::CurrentEnvelopeUnavailable => SiteTrustError::ProvisioningChannel(
            "configured Site authority could not produce the current public Site Trust envelope; Site Trust was not changed"
                .to_string(),
        ),
        FetchError::Transport(message) => SiteTrustError::ProvisioningChannel(message),
    }
}

fn read_root_owned_file(
    path: &Path,
    maximum_bytes: u64,
    label: &str,
) -> Result<Vec<u8>, SiteTrustError> {
    let metadata = fs::symlink_metadata(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
        if metadata.uid() != 0 || metadata.permissions().mode() & 0o022 != 0 {
            return Err(SiteTrustError::Invalid(format!(
                "{label} must be root-owned and not group- or world-writable"
            )));
        }
    }
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > maximum_bytes
    {
        return Err(SiteTrustError::Invalid(format!(
            "{label} is not a safe bounded regular file"
        )));
    }
    Ok(fs::read(path)?)
}

fn read_safe_identity_file(path: &Path) -> Result<(), SiteTrustError> {
    let metadata = fs::symlink_metadata(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(SiteTrustError::Invalid(
                "pinned SSH identity file must not be accessible by group or other users"
                    .to_string(),
            ));
        }
    }
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() == 0 {
        return Err(SiteTrustError::Invalid(
            "pinned SSH identity file is not a safe regular file".to_string(),
        ));
    }
    Ok(())
}

fn default_source_path(host: &str, port: u16) -> Result<PathBuf, SiteTrustError> {
    let name = canonical_host(host)?
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() {
                char::from(byte)
            } else {
                '_'
            }
        })
        .collect::<String>();
    Ok(Path::new(DEFAULT_DIRECTORY).join(format!("{name}-{port}.json")))
}

fn canonical_host(host: &str) -> Result<String, SiteTrustError> {
    let value = host.trim();
    if value.is_empty()
        || value.len() > 253
        || value.contains('/')
        || value.contains('\\')
        || value.contains(':')
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
    {
        return Err(SiteTrustError::Invalid(
            "Site Trust host must be a single hostname or IP address".to_string(),
        ));
    }
    Ok(value.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use coset::{iana, CborSerializable as _, CoseSign1Builder, HeaderBuilder};
    use ed25519_dalek::{Signer as _, SigningKey};
    use proxenos::site_root_public_consumer_v1::{
        assemble_site_root_public_consumer_v1, encode_site_root_public_consumer_envelope_v1,
        encode_site_root_public_consumer_unsigned_v1, SiteRootPublicConsumerActionV1,
        SiteRootPublicConsumerSourceV1, SiteRootPublicConsumerVerificationKeyV1,
    };
    use rcgen::{BasicConstraints, CertificateParams, IsCa, KeyPair};
    use sha2::Sha256;
    use std::cell::Cell;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct StaticChannel {
        result: Result<Vec<u8>, FetchError>,
        calls: Cell<u8>,
    }

    impl ProvisioningChannel for StaticChannel {
        fn fetch(&self, _: &PinnedSshProvisioningSource) -> Result<Vec<u8>, FetchError> {
            self.calls.set(self.calls.get() + 1);
            self.result.clone()
        }
    }

    #[test]
    fn trusted_pinned_source_provisions_without_https_or_os_ca_mutation() {
        let site_uuid = [1; 16];
        let channel = StaticChannel {
            result: Ok(valid_envelope(site_uuid)),
            calls: Cell::new(0),
        };
        let directory = unique_path("trusted-source");
        fs::create_dir_all(&directory).unwrap();
        let record_path = directory.join("trust.json");
        let trust = provision_from_source_with_channel(
            "192.168.0.193",
            8443,
            Some(&record_path),
            &source(site_uuid),
            &channel,
        )
        .unwrap();
        assert_eq!(channel.calls.get(), 1);
        assert_eq!(trust.record.site_uuid, hex::encode(site_uuid));
        assert!(trust.ca_bundle_path.exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn missing_source_fails_before_any_https_request() {
        let path = unique_path("missing-source.json");
        let error = load_source("192.168.0.193", 8443, Some(&path)).unwrap_err();
        assert!(error
            .to_string()
            .contains("site trust provisioning source is not configured"));
        assert!(error
            .to_string()
            .contains("Refusing untrusted appliance HTTPS bootstrap"));
        assert!(
            !path.exists(),
            "the client must never create a source record from untrusted local input"
        );
    }

    #[test]
    fn pinned_ssh_host_key_or_authentication_failure_does_not_write_trust() {
        let directory = unique_path("auth-failure");
        fs::create_dir_all(&directory).unwrap();
        let record_path = directory.join("trust.json");
        let channel = StaticChannel {
            result: Err(FetchError::Authentication),
            calls: Cell::new(0),
        };
        let error = provision_from_source_with_channel(
            "192.168.0.193",
            8443,
            Some(&record_path),
            &source([1; 16]),
            &channel,
        )
        .unwrap_err();
        assert!(error.to_string().contains("host-key or authentication"));
        assert_eq!(channel.calls.get(), 1);
        assert!(!record_path.exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn unsigned_or_substituted_envelope_is_rejected_without_writing_trust() {
        let directory = unique_path("substituted-envelope");
        fs::create_dir_all(&directory).unwrap();
        let record_path = directory.join("trust.json");
        let mut envelope = valid_envelope([1; 16]);
        envelope[100] ^= 1;
        let channel = StaticChannel {
            result: Ok(envelope),
            calls: Cell::new(0),
        };
        let error = provision_from_source_with_channel(
            "192.168.0.193",
            8443,
            Some(&record_path),
            &source([1; 16]),
            &channel,
        )
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("signed Site Trust envelope was rejected"));
        assert!(!record_path.exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn mismatched_site_envelope_is_rejected_without_writing_trust() {
        let directory = unique_path("wrong-site-envelope");
        fs::create_dir_all(&directory).unwrap();
        let record_path = directory.join("trust.json");
        let channel = StaticChannel {
            result: Ok(valid_envelope([2; 16])),
            calls: Cell::new(0),
        };
        let error = provision_from_source_with_channel(
            "192.168.0.193",
            8443,
            Some(&record_path),
            &source([1; 16]),
            &channel,
        )
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("signed Site Trust envelope was rejected"));
        assert!(!record_path.exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn https_source_is_rejected_before_the_channel_is_called() {
        let mut source = source([1; 16]);
        source.transport = "https".to_string();
        let channel = StaticChannel {
            result: Ok(valid_envelope([1; 16])),
            calls: Cell::new(0),
        };
        let error =
            provision_from_source_with_channel("192.168.0.193", 8443, None, &source, &channel)
                .unwrap_err();
        assert!(error
            .to_string()
            .contains("refusing untrusted HTTPS bootstrap"));
        assert_eq!(channel.calls.get(), 0);
    }

    #[test]
    fn pinned_source_uses_only_the_hardened_ssh_export_protocol() {
        let command = pinned_ssh_command(&source([1; 16]));
        assert_eq!(command.get_program(), "/usr/bin/ssh");
        let args = command
            .get_args()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(args.windows(2).any(|pair| pair == ["-F", "/dev/null"]));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["-o", "StrictHostKeyChecking=yes"]));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["-o", "PasswordAuthentication=no"]));
        assert!(args.iter().all(|argument| !argument.contains("https://")));
        assert!(args.last().is_some_and(|argument| {
            argument
                == "/usr/libexec/mnemosyne-domain-cert-site-trust-export-v1 01010101010101010101010101010101"
        }));
    }

    #[test]
    fn ssh_failure_distinguishes_an_unsupported_exporter_from_an_unavailable_envelope() {
        assert!(matches!(
            classify_ssh_failure(b"__DAS_SITE_TRUST_EXPORT_CAPABILITY_UNAVAILABLE__"),
            FetchError::CapabilityUnavailable
        ));
        assert!(matches!(
            classify_ssh_failure(b"__DAS_SITE_TRUST_EXPORT_ENVELOPE_UNAVAILABLE__"),
            FetchError::CurrentEnvelopeUnavailable
        ));
        assert!(map_fetch_error(FetchError::CurrentEnvelopeUnavailable)
            .to_string()
            .contains("could not produce the current public Site Trust envelope"));
    }

    fn source(site_uuid: [u8; 16]) -> PinnedSshProvisioningSource {
        PinnedSshProvisioningSource {
            schema: SCHEMA.to_string(),
            endpoint_host: "192.168.0.193".to_string(),
            endpoint_port: 8443,
            transport: TRANSPORT.to_string(),
            ssh_host: "192.168.0.193".to_string(),
            ssh_port: 22,
            ssh_user: SSH_EXPORT_USER.to_string(),
            ssh_known_hosts_file: PathBuf::from("/etc/dasobjectstore-remote/nuc.known_hosts"),
            ssh_identity_file: PathBuf::from("/etc/dasobjectstore-remote/nuc_identity"),
            site_uuid: hex::encode(site_uuid),
        }
    }

    fn valid_envelope(site_uuid: [u8; 16]) -> Vec<u8> {
        let mut root_params = CertificateParams::new(Vec::<String>::new()).unwrap();
        root_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let root_key = KeyPair::generate().unwrap();
        let root = root_params.self_signed(&root_key).unwrap().der().to_vec();
        let signer = SigningKey::from_bytes(&[7; 32]);
        let key = SiteRootPublicConsumerVerificationKeyV1 {
            generation: 4,
            public_key: signer.verifying_key().to_bytes(),
            registration_digest: [5; 32],
            current: true,
        };
        let source = SiteRootPublicConsumerSourceV1 {
            site_uuid,
            action: SiteRootPublicConsumerActionV1::Install,
            root_generation: 2,
            trust_state_revision: 3,
            root_fingerprint: Sha256::digest(&root).into(),
            previous_root_fingerprint: None,
            root_der: root,
            receipt_generation: key.generation,
            receipt_public_key: key.public_key,
            receipt_registration_digest: key.registration_digest,
        };
        let now = crate::site_trust::now_millis().unwrap();
        let unsigned =
            encode_site_root_public_consumer_unsigned_v1(&source, now, now + 300_000).unwrap();
        let proof = CoseSign1Builder::new()
            .protected(
                HeaderBuilder::new()
                    .algorithm(iana::Algorithm::EdDSA)
                    .content_type("application/vnd.mnemosyne.pxpc.v1".to_owned())
                    .key_id(key.generation.to_be_bytes().to_vec())
                    .build(),
            )
            .create_detached_signature(&unsigned, &[], |input| {
                signer.sign(input).to_bytes().to_vec()
            })
            .build()
            .to_vec()
            .unwrap();
        let artifact = assemble_site_root_public_consumer_v1(&unsigned, &proof, key).unwrap();
        encode_site_root_public_consumer_envelope_v1(&artifact, source.site_uuid, key, now).unwrap()
    }

    fn unique_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dasobjectstore-site-trust-source-{}-{nonce}-{name}",
            std::process::id()
        ))
    }
}
