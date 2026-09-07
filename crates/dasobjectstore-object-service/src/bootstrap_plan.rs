//! Pure, untrusted-input custody bootstrap planning. No live observation or effects.

mod strict_json;

use crate::CustodyStoreDefinitionV1;
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

/// Maximum raw input size accepted by the planner and its file adapter.
pub const MAX_INPUT_BYTES: usize = 1_048_576;
/// Closed manifest wire coordinate; this is not a live execution companion.
pub const MANIFEST_SCHEMA: &str = "dasobjectstore.custody-bootstrap-manifest.v1";
/// Explicitly untrusted supplied-observation wire coordinate.
pub const OBSERVATION_SCHEMA: &str = "dasobjectstore.custody-bootstrap-observation.v1";
/// Redacted, non-authoritative planning result wire coordinate.
pub const PLAN_SCHEMA: &str = "dasobjectstore.custody-bootstrap-plan.v1";
/// Planning-only coordinate for a future executor-generated terminal receipt.
pub const TERMINAL_RECEIPT_SCHEMA: &str = "dasobjectstore.custody-bootstrap-terminal-receipt.v1";
const TERMINAL_BINDINGS: [&str; 7] = [
    "target_identity",
    "raw_companion_digest",
    "attempt_identity",
    "attempt_marker_digest",
    "terminal_outcome",
    "observed_completion_time",
    "retained_inventory_digest",
];

/// A fixed public denial code. Never includes caller-controlled input or paths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlanDenial {
    /// Invalid, oversized, duplicate-key or open-schema JSON.
    Encoding,
    /// Unsupported schema, target, coordinate or source binding.
    Binding,
    /// Invalid or contradictory supplied time interval.
    Time,
    /// Store, role, inventory or retention mismatch.
    Custody,
    /// Path, endpoint, namespace or ordinary-plane exclusion contradiction.
    Isolation,
    /// Missing or contradictory independent verification/continuation inputs.
    Evidence,
}

impl std::fmt::Display for PlanDenial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Encoding => "bootstrap_plan_encoding_denied",
            Self::Binding => "bootstrap_plan_binding_denied",
            Self::Time => "bootstrap_plan_time_denied",
            Self::Custody => "bootstrap_plan_custody_denied",
            Self::Isolation => "bootstrap_plan_isolation_denied",
            Self::Evidence => "bootstrap_plan_evidence_denied",
        })
    }
}
impl std::error::Error for PlanDenial {}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    transaction_id: String,
    purpose: String,
    issued_at_utc: String,
    expires_at_utc: String,
    maximum_runtime_seconds: u64,
    target: Target,
    source: Source,
    isolation: Isolation,
    stores: Vec<Store>,
    verifier: Verifier,
    reader_continuation: Continuation,
    marker_exclusion_evidence_sha256: String,
}

#[derive(Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Target {
    machine_identity_sha256: String,
    endpoint: String,
    os: String,
    architecture: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Source {
    repository: String,
    revision: String,
    cargo_lock_sha256: String,
    source_tree_sha256: String,
    executable_sha256: String,
    qualification_sha256: String,
    image_digest: String,
    compiler_identity: String,
    locked_commands: Vec<QualifiedCommand>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct QualifiedCommand {
    operation: String,
    arguments: Vec<String>,
    working_directory: String,
    output_sha256: String,
    exit_status: u8,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Isolation {
    service_identity: String,
    unit: String,
    namespace: String,
    garage_project: String,
    garage_service: String,
    private_endpoint: String,
    custody_endpoint: String,
    paths: Paths,
    marker_path: String,
    ordinary_paths: Vec<String>,
    ordinary_endpoints: Vec<String>,
    old_client_exclusion_evidence_sha256: String,
    configuration_sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Paths {
    data: String,
    metadata: String,
    catalog: String,
    ledger: String,
    configuration: String,
}
impl Paths {
    fn values(&self) -> [&str; 5] {
        [
            &self.data,
            &self.metadata,
            &self.catalog,
            &self.ledger,
            &self.configuration,
        ]
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Store {
    purpose: String,
    namespace: String,
    definition: CustodyStoreDefinitionV1,
    hold_authority_identity: String,
    content_policy: ContentPolicy,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum ContentPolicy {
    #[serde(rename = "preknown_inventory")]
    PreknownInventory { objects: Vec<Object> },
    #[serde(rename = "generated_terminal_receipt")]
    GeneratedTerminalReceipt {
        schema: String,
        maximum_count: u8,
        maximum_size_bytes: u64,
        payload_source: String,
        required_bindings: Vec<String>,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Object {
    content_sha256: String,
    size_bytes: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Verifier {
    machine_identity_sha256: String,
    executable_sha256: String,
    authority_sha256: String,
    journal_identity_sha256: String,
    administration_exclusion_evidence_sha256: String,
    consumer_identity: String,
    endpoint: String,
    tls_identity_sha256: String,
    provenance_sha256: String,
    journal_backup_exclusion_evidence_sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Continuation {
    service_identity: String,
    configuration_sha256: String,
    available_until_utc: String,
    read_only: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Observation {
    schema: String,
    manifest_sha256: String,
    observed_at_utc: String,
    target: Target,
    existing_paths: Vec<String>,
    existing_buckets: Vec<String>,
    existing_store_ids: Vec<String>,
    existing_namespaces: Vec<String>,
    symlink_paths: Vec<String>,
    ordinary_plane_excluded: bool,
    marker_excluded_from_backup: bool,
    verifier_independent: bool,
    reader_continuation_available: bool,
}

/// Redacted consistency result, never an execution plan or observed acceptance.
#[derive(Debug, Serialize, Eq, PartialEq)]
pub struct BootstrapPlan {
    /// Versioned output format.
    pub schema: &'static str,
    /// Constant explicit non-authoritative status.
    pub status: &'static str,
    /// SHA-256 of exact raw manifest bytes, not canonicalized JSON.
    pub manifest_sha256: String,
    /// SHA-256 of exact raw untrusted observation bytes.
    pub observation_sha256: String,
    /// Number of distinct store-purpose definitions checked.
    pub store_count: usize,
    /// Number of preknown inventory entries checked, excluding future receipts.
    pub object_count: usize,
    /// Maximum generated receipt count; no receipt is generated or accepted.
    pub generated_receipt_limit: u8,
    /// False even when all supplied booleans are affirmative.
    pub live_target_verified: bool,
    /// Always false: no custody or release authority is issued.
    pub execution_authorized: bool,
}

/// Check supplied inputs for consistency without reading time, files or hosts.
///
/// All observations and evidence digests remain untrusted. A successful result
/// is not an independently verified fact, a receipt or permission to execute.
///
/// # Errors
/// Returns a fixed redacted denial on malformed or contradictory supplied data.
pub fn plan_bootstrap(manifest: &[u8], observation: &[u8]) -> Result<BootstrapPlan, PlanDenial> {
    let m: Manifest = strict_json::decode(manifest)?;
    let o: Observation = strict_json::decode(observation)?;
    let manifest_sha256 = digest(manifest);
    if m.schema != MANIFEST_SCHEMA
        || o.schema != OBSERVATION_SCHEMA
        || m.purpose != "r239-custody-bootstrap-plan"
        || !identifier(&m.transaction_id)
        || m.transaction_id.contains("r237")
        || o.manifest_sha256 != manifest_sha256
        || m.target != o.target
        || m.target.endpoint != "192.168.0.193"
        || m.target.os != "linux"
        || m.target.architecture != "amd64"
        || !sha(&m.target.machine_identity_sha256)
    {
        return Err(PlanDenial::Binding);
    }
    validate_source(&m.source)?;
    let issued = timestamp(&m.issued_at_utc)?;
    let expires = timestamp(&m.expires_at_utc)?;
    let observed = timestamp(&o.observed_at_utc)?;
    if observed < issued
        || observed >= expires
        || m.maximum_runtime_seconds == 0
        || m.maximum_runtime_seconds > (expires - observed).num_seconds() as u64
    {
        return Err(PlanDenial::Time);
    }
    validate_isolation(&m.isolation, &o)?;
    let object_count = validate_stores(&m, &o, expires)?;
    validate_evidence(&m, &o)?;
    Ok(BootstrapPlan {
        schema: PLAN_SCHEMA,
        status: "untrusted-input-consistency-only",
        manifest_sha256,
        observation_sha256: digest(observation),
        store_count: m.stores.len(),
        object_count,
        generated_receipt_limit: 1,
        live_target_verified: false,
        execution_authorized: false,
    })
}

fn validate_source(s: &Source) -> Result<(), PlanDenial> {
    if s.repository != "https://github.com/sagrudd/DASObjectStore"
        || !hex(&s.revision, 40)
        || !identifier(&s.compiler_identity)
        || [
            &s.cargo_lock_sha256,
            &s.source_tree_sha256,
            &s.executable_sha256,
            &s.qualification_sha256,
            &s.image_digest,
        ]
        .iter()
        .any(|v| !sha(v))
    {
        return Err(PlanDenial::Binding);
    }
    let mut operations = BTreeSet::new();
    for command in &s.locked_commands {
        if !matches!(command.operation.as_str(), "build" | "test")
            || !operations.insert(command.operation.as_str())
            || command.arguments.first().map(String::as_str) != Some(command.operation.as_str())
            || !command.arguments.iter().any(|v| v == "--locked")
            || command.arguments.len() > 32
            || command.arguments.iter().any(|v| {
                v.is_empty()
                    || v.len() > 256
                    || !v.is_ascii()
                    || v.bytes().any(|b| b.is_ascii_control())
            })
            || !absolute_path(&command.working_directory)
            || !sha(&command.output_sha256)
            || command.exit_status != 0
        {
            return Err(PlanDenial::Binding);
        }
    }
    if operations != BTreeSet::from(["build", "test"]) {
        return Err(PlanDenial::Binding);
    }
    Ok(())
}

fn validate_isolation(i: &Isolation, o: &Observation) -> Result<(), PlanDenial> {
    let names = [
        &i.service_identity,
        &i.unit,
        &i.namespace,
        &i.garage_project,
        &i.garage_service,
    ];
    if names.iter().any(|v| !identifier(v) || v.contains("r237"))
        || names.iter().collect::<BTreeSet<_>>().len() != names.len()
        || i.ordinary_paths.is_empty()
        || i.ordinary_endpoints.is_empty()
        || !sha(&i.old_client_exclusion_evidence_sha256)
        || !sha(&i.configuration_sha256)
        || !o.ordinary_plane_excluded
    {
        return Err(PlanDenial::Isolation);
    }
    if o.existing_paths
        .iter()
        .chain(&o.symlink_paths)
        .any(|v| !absolute_path(v))
        || o.existing_buckets
            .iter()
            .chain(&o.existing_store_ids)
            .chain(&o.existing_namespaces)
            .any(|v| !identifier(v))
    {
        return Err(PlanDenial::Isolation);
    }
    let paths: Vec<&str> = i
        .paths
        .values()
        .into_iter()
        .chain(std::iter::once(i.marker_path.as_str()))
        .collect();
    if paths
        .iter()
        .chain(
            i.ordinary_paths
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .iter(),
        )
        .any(|v| !absolute_path(v))
        || paths
            .iter()
            .any(|p| p.contains("r237") || *p == "/var/lib/dasobjectstore/custody-activation.json")
        || paths
            .iter()
            .enumerate()
            .any(|(n, a)| paths[n + 1..].iter().any(|b| overlaps(a, b)))
        || paths
            .iter()
            .any(|p| i.ordinary_paths.iter().any(|b| overlaps(p, b)))
        || paths
            .iter()
            .any(|p| o.existing_paths.iter().any(|b| overlaps(p, b)))
        || paths
            .iter()
            .any(|p| o.symlink_paths.iter().any(|b| overlaps(p, b)))
    {
        return Err(PlanDenial::Isolation);
    }
    let private = endpoint(&i.private_endpoint).ok_or(PlanDenial::Isolation)?;
    let custody = endpoint(&i.custody_endpoint).ok_or(PlanDenial::Isolation)?;
    if private == custody || !i.custody_endpoint.starts_with("https://") {
        return Err(PlanDenial::Isolation);
    }
    for ordinary in &i.ordinary_endpoints {
        let ordinary = endpoint(ordinary).ok_or(PlanDenial::Isolation)?;
        if ordinary == private || ordinary == custody {
            return Err(PlanDenial::Isolation);
        }
    }
    Ok(())
}

fn validate_stores(
    m: &Manifest,
    o: &Observation,
    expires: DateTime<Utc>,
) -> Result<usize, PlanDenial> {
    let mut purposes = BTreeSet::new();
    let mut stores = BTreeSet::new();
    let mut buckets = BTreeSet::new();
    let mut namespaces = BTreeSet::new();
    let mut roles = BTreeSet::new();
    let mut references = BTreeSet::new();
    let mut count = 0;
    if m.stores.len() != 3 {
        return Err(PlanDenial::Custody);
    }
    for s in &m.stores {
        s.definition.validate().map_err(|_| PlanDenial::Custody)?;
        let p = &s.definition.profile;
        let id = s.definition.store_id.as_str();
        if !purposes.insert(s.purpose.as_str())
            || !stores.insert(id)
            || !buckets.insert(s.definition.bucket_name.as_str())
            || !namespaces.insert(s.namespace.as_str())
            || !identifier(id)
            || id.contains("r237")
            || s.definition.bucket_name.contains("r237")
            || !identifier(&s.namespace)
            || s.namespace.contains("r237")
            || p.target_id != m.target.machine_identity_sha256
            || timestamp(&p.retention_until_utc)? <= expires
            || timestamp(&m.reader_continuation.available_until_utc)?
                < timestamp(&p.retention_until_utc)?
            || o.existing_buckets.contains(&s.definition.bucket_name)
            || o.existing_store_ids.iter().any(|v| v == id)
            || o.existing_namespaces.contains(&s.namespace)
        {
            return Err(PlanDenial::Custody);
        }
        for role in [
            &p.provisioner_identity,
            &p.writer_identity,
            &p.reader_identity,
            &s.hold_authority_identity,
        ] {
            if !identifier(role) || !roles.insert(role.as_str()) {
                return Err(PlanDenial::Custody);
            }
        }
        for reference in [
            &p.provisioner_credential_reference,
            &p.writer_credential_reference,
            &p.reader_credential_reference,
        ] {
            if !identifier(reference) || !references.insert(reference.as_str()) {
                return Err(PlanDenial::Custody);
            }
        }
        count += validate_content_policy(&s.purpose, &s.content_policy)?;
    }
    if purposes != BTreeSet::from(["builder-corpus", "nuc-delivery", "terminal-receipt"]) {
        return Err(PlanDenial::Custody);
    }
    Ok(count)
}

fn validate_content_policy(purpose: &str, policy: &ContentPolicy) -> Result<usize, PlanDenial> {
    match policy {
        ContentPolicy::PreknownInventory { objects } if purpose != "terminal-receipt" => {
            if objects.is_empty() || objects.len() > 4096 {
                return Err(PlanDenial::Custody);
            }
            let mut digests = BTreeSet::new();
            for object in objects {
                if !sha(&object.content_sha256)
                    || object.size_bytes == 0
                    || !digests.insert(&object.content_sha256)
                {
                    return Err(PlanDenial::Custody);
                }
            }
            Ok(objects.len())
        }
        ContentPolicy::GeneratedTerminalReceipt {
            schema,
            maximum_count,
            maximum_size_bytes,
            payload_source,
            required_bindings,
        } if purpose == "terminal-receipt" => {
            if schema != TERMINAL_RECEIPT_SCHEMA
                || *maximum_count != 1
                || *maximum_size_bytes != 65536
                || payload_source != "executor_only"
                || required_bindings.len() != TERMINAL_BINDINGS.len()
                || required_bindings
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>()
                    != BTreeSet::from(TERMINAL_BINDINGS)
            {
                return Err(PlanDenial::Custody);
            }
            Ok(0)
        }
        _ => Err(PlanDenial::Custody),
    }
}

fn validate_evidence(m: &Manifest, o: &Observation) -> Result<(), PlanDenial> {
    let v = &m.verifier;
    if [
        &v.machine_identity_sha256,
        &v.executable_sha256,
        &v.authority_sha256,
        &v.journal_identity_sha256,
        &v.administration_exclusion_evidence_sha256,
        &v.tls_identity_sha256,
        &v.provenance_sha256,
        &v.journal_backup_exclusion_evidence_sha256,
        &m.marker_exclusion_evidence_sha256,
        &m.reader_continuation.configuration_sha256,
    ]
    .iter()
    .any(|v| !sha(v))
        || v.machine_identity_sha256 == m.target.machine_identity_sha256
        || v.endpoint != m.isolation.custody_endpoint
        || !identifier(&v.consumer_identity)
        || !identifier(&m.reader_continuation.service_identity)
        || m.reader_continuation.service_identity == m.isolation.service_identity
        || !m.reader_continuation.read_only
        || !o.marker_excluded_from_backup
        || !o.verifier_independent
        || !o.reader_continuation_available
    {
        return Err(PlanDenial::Evidence);
    }
    Ok(())
}

fn timestamp(value: &str) -> Result<DateTime<Utc>, PlanDenial> {
    let parsed = DateTime::parse_from_rfc3339(value)
        .map_err(|_| PlanDenial::Time)?
        .with_timezone(&Utc);
    if parsed.to_rfc3339_opts(SecondsFormat::Secs, true) != value {
        return Err(PlanDenial::Time);
    }
    Ok(parsed)
}
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|v| v.is_ascii_lowercase() || v.is_ascii_digit() || b"-_.".contains(&v))
        && value != "."
        && value != ".."
}
fn absolute_path(value: &str) -> bool {
    value.starts_with('/')
        && value.len() <= 4096
        && value != "/"
        && value[1..].split('/').all(identifier)
}
fn overlaps(a: &str, b: &str) -> bool {
    a == b
        || a.strip_prefix(b).is_some_and(|v| v.starts_with('/'))
        || b.strip_prefix(a).is_some_and(|v| v.starts_with('/'))
}
fn endpoint(value: &str) -> Option<(String, u16)> {
    let authority = value
        .strip_prefix("http://")
        .or_else(|| value.strip_prefix("https://"))?;
    let (host, port) = authority.rsplit_once(':')?;
    let port = port.parse::<u16>().ok()?;
    if port == 0 || port.to_string() != authority.rsplit_once(':')?.1 {
        return None;
    }
    let host = match host {
        // All accepted addresses name this single target. Different bindings
        // on the same port are not evidence of independently isolated planes.
        "localhost" | "127.0.0.1" | "[::1]" | "192.168.0.193" => "nuc".to_owned(),
        _ => return None,
    };
    Some((host, port))
}
fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
}
fn sha(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|v| hex(v, 64))
}
fn digest(value: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(value))
}

#[cfg(test)]
mod tests;
