//! r237 local bootstrap observer contracts.
//!
//! These types describe a read-only local observation and its fail-closed
//! assessment. They intentionally contain no execution, provisioning, daemon,
//! Garage, credential, marker-creation, filesystem-write, or network API.

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

pub const R237_BOOTSTRAP_LOCAL_OBSERVATION_V1_SCHEMA: &str =
    "dasobjectstore.r237_bootstrap_local_observation.v1";
pub const R237_BOOTSTRAP_OBSERVER_REPORT_V1_SCHEMA: &str =
    "dasobjectstore.r237_bootstrap_observer_report.v1";
pub const R237_CANONICAL_PROGRAMME_MAIN_REVISION: &str = "ab4c7319ad398621052643a0eef07551f7ba969f";
pub const R237_TRANSACTION_DOCUMENT_SOURCE_REVISION: &str =
    "34b44650b22606f1dcc9fc7383d847513c670805";
pub const R237_TRANSACTION_DOCUMENT_SHA256: &str =
    "297c8ee79a0780a160c652e1944d0f5bf89b5b9998c57edccbd6e57c786d8570";
pub const R237_NUC_HOST: &str = "192.168.0.193";
pub const R237_STORE_ID: &str = "r237_s4_bootstrap_custody";
pub const R237_BUCKET_NAME: &str = "dos-r237-s4-bootstrap-custody";
pub const R237_STORE_CLASS: &str = "critical_metadata";
pub const R237_WRITER_GROUP: &str = "mnemosyne-r237-custody";
pub const R237_CORPUS_KEY_PREFIX: &str = "corpus/sha256";
pub const R237_RECEIPT_KEY_PREFIX: &str = "receipts/sha256";
pub const R237_PER_OBJECT_LIMIT_BYTES: u64 = 256 * 1024 * 1024;
/// A semantic purpose only; the observer never exposes or creates a path.
pub const R237_MARKER_ROOT_PURPOSE: &str = "r237 non-WORM one-use bootstrap custody guard";
pub const R237_TOTAL_OBJECT_BODY_LIMIT_BYTES: u64 = 16 * 1024 * 1024 * 1024;
pub const R237_MINIMUM_FREE_BYTES_AFTER_ALLOCATION: u64 = 24 * 1024 * 1024 * 1024;
pub const R237_REQUIRED_FREE_BYTES_PER_SELECTED_HDD: u64 =
    R237_TOTAL_OBJECT_BODY_LIMIT_BYTES + R237_MINIMUM_FREE_BYTES_AFTER_ALLOCATION;
pub const R237_REQUIRED_HDD_MEMBERS: usize = 3;

/// A redacted local proof state. `Unavailable` is never interpreted as
/// absence, and a report is always denied while any required proof is absent.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum R237ObservationStatusV1 {
    Verified,
    Absent,
    Present,
    Unavailable,
    Conflicted,
    Invalid,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct R237ObservationCheckV1 {
    pub status: R237ObservationStatusV1,
    /// JCS SHA-256 of non-secret, locally observed evidence when available.
    pub evidence_sha256: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum R237ObservedMediaV1 {
    Hdd,
    Ssd,
    Unknown,
}

/// A physical-media observation with all host-local identity details hashed.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct R237HddObservationV1 {
    pub physical_member_sha256: String,
    pub media: R237ObservedMediaV1,
    pub mounted: bool,
    pub writable: bool,
    pub mount_mapping_verified: bool,
    pub available_bytes: u64,
    pub smart: R237ObservationStatusV1,
}

/// The fixed non-secret r237 transaction tuple. This is documentation of the
/// reviewed scope, not a provision request and cannot be supplied by a caller.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct R237ReviewedBootstrapTupleV1 {
    pub logical_store: String,
    pub bucket: String,
    pub storage_class: String,
    pub required_healthy_distinct_hdds: usize,
    pub total_object_body_limit_bytes: u64,
    pub per_object_limit_bytes: u64,
    pub corpus_key_prefix: String,
    pub receipt_key_prefix: String,
    pub writer_group: String,
    pub marker_root_purpose: String,
}

/// Produced only by the local observer. This is not a caller-supplied plan or
/// inventory format and has no generic target or namespace override.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct R237BootstrapLocalObservationV1 {
    pub schema_version: String,
    pub target_ip: R237ObservationCheckV1,
    pub machine_identity: R237ObservationCheckV1,
    pub appliance_identity: R237ObservationCheckV1,
    pub clone_detection: R237ObservationCheckV1,
    pub store_registry_namespace: R237ObservationCheckV1,
    pub marker_root: R237ObservationCheckV1,
    pub writer_group: R237ObservationCheckV1,
    pub hdd_members: Vec<R237HddObservationV1>,
    /// Existing safe local sources cannot establish a complete Garage bucket
    /// inventory without a new authority contract.
    pub garage_bucket_inventory: R237ObservationCheckV1,
    /// Existing DAS APIs cannot bind a future provision to exact physical HDDs.
    pub exact_physical_placement: R237ObservationCheckV1,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum R237BootstrapObserverDispositionV1 {
    Denied,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum R237BootstrapObserverDenialV1 {
    InvalidObservation,
    TargetIpUnverified,
    MachineIdentityUnverified,
    ApplianceIdentityUnverified,
    CloneDetectionUnavailable,
    StoreRegistryNamespaceUnverified,
    MarkerRootUnverified,
    WriterGroupUnverified,
    InsufficientVerifiedHdds,
    GarageBucketInventoryUnavailable,
    ExactPhysicalPlacementUnavailable,
}

/// Redacted report body. It has no `ready`, `eligible`, provision, or apply
/// field: this release cannot make a positive bootstrap provision decision.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct R237BootstrapObserverReportBodyV1 {
    pub schema_version: String,
    pub canonical_programme_main_revision: String,
    pub transaction_document_source_revision: String,
    pub transaction_document_sha256: String,
    pub reviewed_tuple: R237ReviewedBootstrapTupleV1,
    pub reviewed_tuple_jcs_sha256: String,
    /// The fixed reviewed target, not a claim that the local IP check passed.
    pub reviewed_target_host: String,
    pub observation_jcs_sha256: String,
    pub disposition: R237BootstrapObserverDispositionV1,
    pub denials: Vec<R237BootstrapObserverDenialV1>,
    /// Kept explicit rather than inferred from the denial list: a caller must
    /// never mistake an unavailable Garage namespace proof for an empty one.
    pub garage_bucket_inventory_status: R237ObservationStatusV1,
    /// Existing contracts cannot prove an eventual Garage placement maps to
    /// the observed physical members, so this release exposes the gap.
    pub exact_physical_placement_status: R237ObservationStatusV1,
    pub not_s4: bool,
    pub not_custody_acceptance: bool,
    pub not_remote_deployment: bool,
    pub not_service_activation: bool,
    pub non_worm_bootstrap_only: bool,
}

/// The report digest is self-excluding: it covers `report`, not this wrapper.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct R237BootstrapObserverReportV1 {
    pub report: R237BootstrapObserverReportBodyV1,
    pub report_sha256: String,
}

/// Assess an in-memory local observation. The outcome is always denied; the
/// explicit reasons prevent unavailable proof from being misreported as safe.
pub fn assess_r237_bootstrap_local_observation(
    observation: &R237BootstrapLocalObservationV1,
) -> R237BootstrapObserverReportV1 {
    let observation_jcs_sha256 = canonical_sha256(observation).unwrap_or_else(|| "0".repeat(64));
    let mut denials = validation_denials(observation);
    if observation.schema_version != R237_BOOTSTRAP_LOCAL_OBSERVATION_V1_SCHEMA {
        denials.push(R237BootstrapObserverDenialV1::InvalidObservation);
    }
    if observation.target_ip.status != R237ObservationStatusV1::Verified {
        denials.push(R237BootstrapObserverDenialV1::TargetIpUnverified);
    }
    if observation.machine_identity.status != R237ObservationStatusV1::Verified {
        denials.push(R237BootstrapObserverDenialV1::MachineIdentityUnverified);
    }
    if observation.appliance_identity.status != R237ObservationStatusV1::Verified {
        denials.push(R237BootstrapObserverDenialV1::ApplianceIdentityUnverified);
    }
    if observation.clone_detection.status != R237ObservationStatusV1::Verified {
        denials.push(R237BootstrapObserverDenialV1::CloneDetectionUnavailable);
    }
    if observation.store_registry_namespace.status != R237ObservationStatusV1::Absent {
        denials.push(R237BootstrapObserverDenialV1::StoreRegistryNamespaceUnverified);
    }
    if observation.marker_root.status != R237ObservationStatusV1::Absent {
        denials.push(R237BootstrapObserverDenialV1::MarkerRootUnverified);
    }
    if observation.writer_group.status != R237ObservationStatusV1::Absent {
        denials.push(R237BootstrapObserverDenialV1::WriterGroupUnverified);
    }
    if verified_hdd_member_count(&observation.hdd_members) < R237_REQUIRED_HDD_MEMBERS {
        denials.push(R237BootstrapObserverDenialV1::InsufficientVerifiedHdds);
    }
    denials.push(R237BootstrapObserverDenialV1::GarageBucketInventoryUnavailable);
    denials.push(R237BootstrapObserverDenialV1::ExactPhysicalPlacementUnavailable);
    // These proofs have no trusted producer in 0.178. A caller-supplied
    // `Verified` status is invalid and is never reflected in the report.
    if observation.garage_bucket_inventory.status != R237ObservationStatusV1::Unavailable
        || observation.exact_physical_placement.status != R237ObservationStatusV1::Unavailable
    {
        denials.push(R237BootstrapObserverDenialV1::InvalidObservation);
    }
    denials.sort_by_key(|value| *value as u8);
    denials.dedup();
    let reviewed_tuple = r237_reviewed_tuple();
    let reviewed_tuple_jcs_sha256 =
        canonical_sha256(&reviewed_tuple).unwrap_or_else(|| "0".repeat(64));
    let report = R237BootstrapObserverReportBodyV1 {
        schema_version: R237_BOOTSTRAP_OBSERVER_REPORT_V1_SCHEMA.to_owned(),
        canonical_programme_main_revision: R237_CANONICAL_PROGRAMME_MAIN_REVISION.to_owned(),
        transaction_document_source_revision: R237_TRANSACTION_DOCUMENT_SOURCE_REVISION.to_owned(),
        transaction_document_sha256: R237_TRANSACTION_DOCUMENT_SHA256.to_owned(),
        reviewed_tuple,
        reviewed_tuple_jcs_sha256,
        reviewed_target_host: R237_NUC_HOST.to_owned(),
        observation_jcs_sha256,
        disposition: R237BootstrapObserverDispositionV1::Denied,
        denials,
        garage_bucket_inventory_status: R237ObservationStatusV1::Unavailable,
        exact_physical_placement_status: R237ObservationStatusV1::Unavailable,
        not_s4: true,
        not_custody_acceptance: true,
        not_remote_deployment: true,
        not_service_activation: true,
        non_worm_bootstrap_only: true,
    };
    let report_sha256 = canonical_sha256(&report).unwrap_or_else(|| "0".repeat(64));
    R237BootstrapObserverReportV1 {
        report,
        report_sha256,
    }
}

fn r237_reviewed_tuple() -> R237ReviewedBootstrapTupleV1 {
    R237ReviewedBootstrapTupleV1 {
        logical_store: R237_STORE_ID.to_owned(),
        bucket: R237_BUCKET_NAME.to_owned(),
        storage_class: R237_STORE_CLASS.to_owned(),
        required_healthy_distinct_hdds: R237_REQUIRED_HDD_MEMBERS,
        total_object_body_limit_bytes: R237_TOTAL_OBJECT_BODY_LIMIT_BYTES,
        per_object_limit_bytes: R237_PER_OBJECT_LIMIT_BYTES,
        corpus_key_prefix: R237_CORPUS_KEY_PREFIX.to_owned(),
        receipt_key_prefix: R237_RECEIPT_KEY_PREFIX.to_owned(),
        writer_group: R237_WRITER_GROUP.to_owned(),
        marker_root_purpose: R237_MARKER_ROOT_PURPOSE.to_owned(),
    }
}

/// Canonical wire bytes for the standalone observer. No pretty-JSON fallback
/// is permitted, so downstream evidence cannot hash a different encoding.
pub fn canonical_r237_bootstrap_observer_report(
    report: &R237BootstrapObserverReportV1,
) -> Option<Vec<u8>> {
    serde_jcs::to_vec(report).ok()
}

fn validation_denials(
    observation: &R237BootstrapLocalObservationV1,
) -> Vec<R237BootstrapObserverDenialV1> {
    let mut denials = Vec::new();
    let required_checks = [
        &observation.target_ip,
        &observation.machine_identity,
        &observation.appliance_identity,
        &observation.clone_detection,
        &observation.store_registry_namespace,
        &observation.marker_root,
        &observation.writer_group,
        &observation.garage_bucket_inventory,
        &observation.exact_physical_placement,
    ];
    if required_checks.iter().any(|check| {
        !matches!(check.status, R237ObservationStatusV1::Unavailable)
            && !check.evidence_sha256.as_deref().is_some_and(valid_sha256)
    }) || observation
        .hdd_members
        .iter()
        .any(|member| !valid_sha256(&member.physical_member_sha256))
    {
        denials.push(R237BootstrapObserverDenialV1::InvalidObservation);
    }
    denials
}

fn verified_hdd_member_count(members: &[R237HddObservationV1]) -> usize {
    let mut distinct = std::collections::BTreeSet::new();
    members
        .iter()
        .filter(|member| {
            member.media == R237ObservedMediaV1::Hdd
                && member.mounted
                && member.writable
                && member.mount_mapping_verified
                && member.smart == R237ObservationStatusV1::Verified
                && member.available_bytes >= R237_REQUIRED_FREE_BYTES_PER_SELECTED_HDD
                && valid_sha256(&member.physical_member_sha256)
        })
        .filter(|member| distinct.insert(member.physical_member_sha256.as_str()))
        .count()
}

fn canonical_sha256(value: &impl Serialize) -> Option<String> {
    let bytes = serde_jcs::to_vec(value).ok()?;
    Some(format!("{:x}", Sha256::digest(bytes)))
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(status: R237ObservationStatusV1) -> R237ObservationCheckV1 {
        R237ObservationCheckV1 {
            status,
            evidence_sha256: (status != R237ObservationStatusV1::Unavailable)
                .then(|| "a".repeat(64)),
        }
    }

    fn observation() -> R237BootstrapLocalObservationV1 {
        R237BootstrapLocalObservationV1 {
            schema_version: R237_BOOTSTRAP_LOCAL_OBSERVATION_V1_SCHEMA.to_owned(),
            target_ip: check(R237ObservationStatusV1::Verified),
            machine_identity: check(R237ObservationStatusV1::Verified),
            appliance_identity: check(R237ObservationStatusV1::Verified),
            clone_detection: check(R237ObservationStatusV1::Unavailable),
            store_registry_namespace: check(R237ObservationStatusV1::Absent),
            marker_root: check(R237ObservationStatusV1::Absent),
            writer_group: check(R237ObservationStatusV1::Absent),
            hdd_members: (0..3)
                .map(|index| R237HddObservationV1 {
                    physical_member_sha256: format!("{index:064x}"),
                    media: R237ObservedMediaV1::Hdd,
                    mounted: true,
                    writable: true,
                    mount_mapping_verified: true,
                    available_bytes: R237_REQUIRED_FREE_BYTES_PER_SELECTED_HDD,
                    smart: R237ObservationStatusV1::Verified,
                })
                .collect(),
            garage_bucket_inventory: check(R237ObservationStatusV1::Unavailable),
            exact_physical_placement: check(R237ObservationStatusV1::Unavailable),
        }
    }

    #[test]
    fn report_is_jcs_deterministic_self_excluding_and_never_eligible() {
        let first = assess_r237_bootstrap_local_observation(&observation());
        let second = assess_r237_bootstrap_local_observation(&observation());
        assert_eq!(first, second);
        assert_eq!(
            first.report_sha256,
            canonical_sha256(&first.report).expect("report digest")
        );
        assert_eq!(
            first.report.observation_jcs_sha256,
            canonical_sha256(&observation()).expect("observation digest")
        );
        assert_eq!(
            first.report.disposition,
            R237BootstrapObserverDispositionV1::Denied
        );
        assert!(first.report.not_s4);
        assert!(first.report.not_custody_acceptance);
        assert!(first.report.not_remote_deployment);
        assert!(first.report.not_service_activation);
        assert!(canonical_r237_bootstrap_observer_report(&first).is_some());
        assert!(first
            .report
            .denials
            .contains(&R237BootstrapObserverDenialV1::GarageBucketInventoryUnavailable));
        assert!(first
            .report
            .denials
            .contains(&R237BootstrapObserverDenialV1::ExactPhysicalPlacementUnavailable));
        assert_eq!(
            first.report.garage_bucket_inventory_status,
            R237ObservationStatusV1::Unavailable
        );
        assert_eq!(
            first.report.exact_physical_placement_status,
            R237ObservationStatusV1::Unavailable
        );
        assert_eq!(
            first.report.reviewed_tuple_jcs_sha256,
            canonical_sha256(&first.report.reviewed_tuple).expect("tuple digest")
        );
        assert_eq!(first.report.reviewed_target_host, R237_NUC_HOST);
        assert_eq!(
            first.report.reviewed_tuple.storage_class,
            "critical_metadata"
        );
        assert_eq!(
            first.report.reviewed_tuple.per_object_limit_bytes,
            256 * 1024 * 1024
        );
    }

    #[test]
    fn strict_observation_schema_rejects_unknown_fields() {
        let encoded = serde_json::to_string(&observation()).expect("fixture");
        let forged = encoded.replacen('}', ",\"unknown\":true}", 1);
        assert!(serde_json::from_str::<R237BootstrapLocalObservationV1>(&forged).is_err());
    }

    #[test]
    fn identity_and_local_prerequisite_failures_remain_denied() {
        let mut target = observation();
        target.target_ip = check(R237ObservationStatusV1::Unavailable);
        assert!(assess_r237_bootstrap_local_observation(&target)
            .report
            .denials
            .contains(&R237BootstrapObserverDenialV1::TargetIpUnverified));
        let mut clone = observation();
        clone.clone_detection = check(R237ObservationStatusV1::Conflicted);
        assert!(assess_r237_bootstrap_local_observation(&clone)
            .report
            .denials
            .contains(&R237BootstrapObserverDenialV1::CloneDetectionUnavailable));
        let mut group = observation();
        group.writer_group = check(R237ObservationStatusV1::Present);
        assert!(assess_r237_bootstrap_local_observation(&group)
            .report
            .denials
            .contains(&R237BootstrapObserverDenialV1::WriterGroupUnverified));

        let mut forged_external_proof = observation();
        forged_external_proof.garage_bucket_inventory = check(R237ObservationStatusV1::Verified);
        forged_external_proof.exact_physical_placement = check(R237ObservationStatusV1::Verified);
        let report = assess_r237_bootstrap_local_observation(&forged_external_proof);
        assert_eq!(
            report.report.garage_bucket_inventory_status,
            R237ObservationStatusV1::Unavailable
        );
        assert_eq!(
            report.report.exact_physical_placement_status,
            R237ObservationStatusV1::Unavailable
        );
        assert!(report
            .report
            .denials
            .contains(&R237BootstrapObserverDenialV1::InvalidObservation));
        assert!(report
            .report
            .denials
            .contains(&R237BootstrapObserverDenialV1::GarageBucketInventoryUnavailable));
        assert!(report
            .report
            .denials
            .contains(&R237BootstrapObserverDenialV1::ExactPhysicalPlacementUnavailable));
    }

    #[test]
    fn report_never_leaks_raw_observation_values_to_json_debug_or_digest() {
        let raw = "TOPSECRET:/srv/das/wwn-0x5000cca123/serial-99/smart-warning";
        let mut untrusted = observation();
        untrusted.schema_version = raw.to_owned();
        untrusted.target_ip.evidence_sha256 = Some(raw.to_owned());
        untrusted.hdd_members[0].physical_member_sha256 = raw.to_owned();
        let report = assess_r237_bootstrap_local_observation(&untrusted);
        let json = String::from_utf8(
            canonical_r237_bootstrap_observer_report(&report).expect("canonical report"),
        )
        .expect("utf8 report");
        let debug = format!("{report:?}");
        for forbidden in [
            raw,
            "TOPSECRET",
            "/srv/das",
            "wwn-0x5000cca123",
            "serial-99",
            "smart-warning",
        ] {
            assert!(!json.contains(forbidden));
            assert!(!debug.contains(forbidden));
            assert!(!report.report_sha256.contains(forbidden));
            assert!(!report.report.observation_jcs_sha256.contains(forbidden));
        }
    }

    #[test]
    fn requires_distinct_hdds_with_40_gib_available_before_allocation() {
        let mut insufficient = observation();
        insufficient.hdd_members[0].available_bytes = R237_REQUIRED_FREE_BYTES_PER_SELECTED_HDD - 1;
        assert!(assess_r237_bootstrap_local_observation(&insufficient)
            .report
            .denials
            .contains(&R237BootstrapObserverDenialV1::InsufficientVerifiedHdds));
        let mut aliases = observation();
        aliases.hdd_members[2].physical_member_sha256 =
            aliases.hdd_members[1].physical_member_sha256.clone();
        assert!(assess_r237_bootstrap_local_observation(&aliases)
            .report
            .denials
            .contains(&R237BootstrapObserverDenialV1::InsufficientVerifiedHdds));
        assert_eq!(
            R237_REQUIRED_FREE_BYTES_PER_SELECTED_HDD,
            40 * 1024 * 1024 * 1024
        );
    }
}
