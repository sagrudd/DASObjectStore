//! Strictly non-mutating, fixed r237 bootstrap-storage planning contracts.
//!
//! This is not a dry run and deliberately has no apply API. It assesses a
//! supplied read-only inventory for one reviewed tuple and contains no
//! filesystem, network, daemon, Garage, credential, process, or mutation API.

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeSet;

pub const R237_BOOTSTRAP_PLAN_INVENTORY_V1_SCHEMA: &str =
    "dasobjectstore.r237_bootstrap_plan_inventory.v1";
pub const R237_BOOTSTRAP_PROVISION_PLAN_V1_SCHEMA: &str =
    "dasobjectstore.r237_bootstrap_provision_plan.v1";

/// The canonical-programme-main merge. A branch/source commit is not proof.
pub const R237_CANONICAL_PROGRAMME_MAIN_REVISION: &str = "ab4c7319ad398621052643a0eef07551f7ba969f";
/// The source commit that contains the reviewed transaction document.
pub const R237_TRANSACTION_DOCUMENT_SOURCE_REVISION: &str =
    "34b44650b22606f1dcc9fc7383d847513c670805";
pub const R237_TRANSACTION_DOCUMENT_SHA256: &str =
    "297c8ee79a0780a160c652e1944d0f5bf89b5b9998c57edccbd6e57c786d8570";

pub const R237_NUC_HOST: &str = "192.168.0.193";
pub const R237_STORE_ID: &str = "r237_s4_bootstrap_custody";
pub const R237_BUCKET_NAME: &str = "dos-r237-s4-bootstrap-custody";
pub const R237_WRITER_GROUP: &str = "mnemosyne-r237-custody";
pub const R237_CORPUS_KEY_PREFIX: &str = "corpus/sha256";
pub const R237_RECEIPT_KEY_PREFIX: &str = "receipts/sha256";
pub const R237_STORE_CLASS: &str = "critical_metadata";
pub const R237_COPIES: u8 = 3;
pub const R237_TOTAL_OBJECT_BODY_LIMIT_BYTES: u64 = 16 * 1024 * 1024 * 1024;
pub const R237_PER_OBJECT_LIMIT_BYTES: u64 = 256 * 1024 * 1024;
pub const R237_MINIMUM_FREE_BYTES_PER_SELECTED_HDD: u64 = 24 * 1024 * 1024 * 1024;
/// A semantic purpose, never a marker path.
pub const R237_MARKER_ROOT_PURPOSE: &str = "r237 non-WORM one-use bootstrap custody guard";

/// Opaque digest of NUC identity evidence from an attended read-only audit.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct R237BootstrapPlanTargetV1 {
    pub host: String,
    pub identity_sha256: String,
}

/// Redacted physical HDD facts. `physical_member_id` must be post-alias and
/// post-partition collapse; it must never be a path, serial, root, or secret.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct R237BootstrapPlanHddMemberV1 {
    pub physical_member_id: String,
    pub enclosure_id: String,
    pub media_kind: String,
    pub mounted: bool,
    pub writable: bool,
    pub degraded: bool,
    pub smart_status: String,
    pub available_bytes: u64,
}

/// Inventory may only assert absence if both namespace enumerations are known
/// complete. Unknown is therefore a refusal, not an empty result.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct R237BootstrapPlanInventoryV1 {
    pub schema_version: String,
    pub target: R237BootstrapPlanTargetV1,
    pub store_inventory_complete: bool,
    pub bucket_inventory_complete: bool,
    pub existing_store_ids: Vec<String>,
    pub existing_bucket_names: Vec<String>,
    pub hdd_members: Vec<R237BootstrapPlanHddMemberV1>,
}

/// Fixed non-secret tuple, emitted only after every invariant is proven.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct R237BootstrapProvisionTupleV1 {
    pub store_id: String,
    pub bucket_name: String,
    pub store_class: String,
    pub copies: u8,
    pub required_healthy_distinct_hdd_members: u8,
    pub total_object_body_limit_bytes: u64,
    pub per_object_limit_bytes: u64,
    pub minimum_free_bytes_per_selected_hdd: u64,
    pub corpus_key_prefix: String,
    pub receipt_key_prefix: String,
    pub writer_group: String,
    pub marker_root_purpose: String,
}

/// Redacted JCS input. The selected physical member list is only represented
/// by a count and digest, never by member IDs or host-local topology details.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct R237BootstrapProvisionPlanV1 {
    pub schema_version: String,
    pub canonical_programme_main_revision: String,
    pub transaction_document_source_revision: String,
    pub transaction_document_sha256: String,
    pub target: R237BootstrapPlanTargetV1,
    pub source_inventory_sha256: String,
    pub provision: R237BootstrapProvisionTupleV1,
    pub selected_healthy_distinct_hdd_member_count: u8,
    pub selected_healthy_distinct_hdd_members_sha256: String,
    pub non_mutating: bool,
    pub non_worm_bootstrap_only: bool,
    /// Exact physical placement cannot presently be bound by DAS apply APIs.
    pub later_guarded_apply_compatible: bool,
}

/// The `plan_sha256` is self-excluding: it is the JCS SHA-256 of `plan` only.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct R237BootstrapProvisionPlanOutputV1 {
    pub plan: R237BootstrapProvisionPlanV1,
    pub plan_sha256: String,
}

/// A safe, evidence-producing assessment. `ready = false` is a terminal
/// refusal for this invocation, not a request to discover, mutate, or retry.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct R237BootstrapPlanAssessmentV1 {
    pub ready: bool,
    pub denial_reason: Option<R237BootstrapPlanDenialReasonV1>,
    pub plan: Option<R237BootstrapProvisionPlanOutputV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum R237BootstrapPlanDenialReasonV1 {
    InvalidInventory,
    TargetMismatch,
    ExistingStoreConflict,
    ExistingBucketConflict,
    AmbiguousPhysicalMembers,
    InsufficientHealthyDistinctHdds,
    CanonicalizationFailed,
}

/// Pure assessment of the one r237 tuple. It cannot create an admin job,
/// registry, ACL, credential, intent, marker, or file; nor call Garage,
/// restart a service, or contact NUC/DGX.
pub fn assess_r237_nuc_bootstrap_plan(
    inventory: &R237BootstrapPlanInventoryV1,
) -> R237BootstrapPlanAssessmentV1 {
    let denial = assess_denial(inventory);
    if let Some(denial_reason) = denial {
        return denied(denial_reason);
    }

    let source_inventory_sha256 = match canonical_sha256(inventory) {
        Some(value) => value,
        None => return denied(R237BootstrapPlanDenialReasonV1::CanonicalizationFailed),
    };
    let selected_hdd_member_ids = select_healthy_hdds(inventory);
    let selected_healthy_distinct_hdd_members_sha256 =
        match canonical_sha256(&selected_hdd_member_ids) {
            Some(value) => value,
            None => return denied(R237BootstrapPlanDenialReasonV1::CanonicalizationFailed),
        };
    let plan = R237BootstrapProvisionPlanV1 {
        schema_version: R237_BOOTSTRAP_PROVISION_PLAN_V1_SCHEMA.to_owned(),
        canonical_programme_main_revision: R237_CANONICAL_PROGRAMME_MAIN_REVISION.to_owned(),
        transaction_document_source_revision: R237_TRANSACTION_DOCUMENT_SOURCE_REVISION.to_owned(),
        transaction_document_sha256: R237_TRANSACTION_DOCUMENT_SHA256.to_owned(),
        target: inventory.target.clone(),
        source_inventory_sha256,
        provision: r237_provision_tuple(),
        selected_healthy_distinct_hdd_member_count: R237_COPIES,
        selected_healthy_distinct_hdd_members_sha256,
        non_mutating: true,
        non_worm_bootstrap_only: true,
        later_guarded_apply_compatible: false,
    };
    let Some(plan_sha256) = canonical_sha256(&plan) else {
        return denied(R237BootstrapPlanDenialReasonV1::CanonicalizationFailed);
    };
    R237BootstrapPlanAssessmentV1 {
        ready: true,
        denial_reason: None,
        plan: Some(R237BootstrapProvisionPlanOutputV1 { plan, plan_sha256 }),
    }
}

fn denied(denial_reason: R237BootstrapPlanDenialReasonV1) -> R237BootstrapPlanAssessmentV1 {
    R237BootstrapPlanAssessmentV1 {
        ready: false,
        denial_reason: Some(denial_reason),
        plan: None,
    }
}

fn assess_denial(
    inventory: &R237BootstrapPlanInventoryV1,
) -> Option<R237BootstrapPlanDenialReasonV1> {
    if inventory.schema_version != R237_BOOTSTRAP_PLAN_INVENTORY_V1_SCHEMA
        || !valid_target(&inventory.target)
        || !inventory.store_inventory_complete
        || !inventory.bucket_inventory_complete
        || !unique_identifiers(&inventory.existing_store_ids)
        || !unique_bucket_names(&inventory.existing_bucket_names)
        || inventory.hdd_members.is_empty()
    {
        return Some(R237BootstrapPlanDenialReasonV1::InvalidInventory);
    }
    if inventory.target.host != R237_NUC_HOST {
        return Some(R237BootstrapPlanDenialReasonV1::TargetMismatch);
    }
    if inventory
        .existing_store_ids
        .iter()
        .any(|id| id == R237_STORE_ID)
    {
        return Some(R237BootstrapPlanDenialReasonV1::ExistingStoreConflict);
    }
    if inventory
        .existing_bucket_names
        .iter()
        .any(|bucket| bucket == R237_BUCKET_NAME)
    {
        return Some(R237BootstrapPlanDenialReasonV1::ExistingBucketConflict);
    }
    let mut physical_members = BTreeSet::new();
    if inventory.hdd_members.iter().any(|member| {
        !valid_identifier(&member.physical_member_id)
            || !valid_identifier(&member.enclosure_id)
            || member.media_kind != "hdd"
            || !physical_members.insert(member.physical_member_id.as_str())
    }) {
        return Some(R237BootstrapPlanDenialReasonV1::AmbiguousPhysicalMembers);
    }
    if select_healthy_hdds(inventory).len() != usize::from(R237_COPIES) {
        return Some(R237BootstrapPlanDenialReasonV1::InsufficientHealthyDistinctHdds);
    }
    None
}

fn select_healthy_hdds(inventory: &R237BootstrapPlanInventoryV1) -> Vec<String> {
    let mut eligible: Vec<&R237BootstrapPlanHddMemberV1> = inventory
        .hdd_members
        .iter()
        .filter(|member| {
            member.media_kind == "hdd"
                && member.mounted
                && member.writable
                && !member.degraded
                && member.smart_status == "passed"
                && member.available_bytes >= R237_MINIMUM_FREE_BYTES_PER_SELECTED_HDD
        })
        .collect();
    eligible.sort_by(|left, right| {
        right
            .available_bytes
            .cmp(&left.available_bytes)
            .then_with(|| left.physical_member_id.cmp(&right.physical_member_id))
    });
    if eligible.len() < usize::from(R237_COPIES) {
        return Vec::new();
    }
    let mut selected: Vec<String> = eligible
        .into_iter()
        .take(usize::from(R237_COPIES))
        .map(|member| member.physical_member_id.clone())
        .collect();
    selected.sort();
    selected
}

fn r237_provision_tuple() -> R237BootstrapProvisionTupleV1 {
    R237BootstrapProvisionTupleV1 {
        store_id: R237_STORE_ID.to_owned(),
        bucket_name: R237_BUCKET_NAME.to_owned(),
        store_class: R237_STORE_CLASS.to_owned(),
        copies: R237_COPIES,
        required_healthy_distinct_hdd_members: R237_COPIES,
        total_object_body_limit_bytes: R237_TOTAL_OBJECT_BODY_LIMIT_BYTES,
        per_object_limit_bytes: R237_PER_OBJECT_LIMIT_BYTES,
        minimum_free_bytes_per_selected_hdd: R237_MINIMUM_FREE_BYTES_PER_SELECTED_HDD,
        corpus_key_prefix: R237_CORPUS_KEY_PREFIX.to_owned(),
        receipt_key_prefix: R237_RECEIPT_KEY_PREFIX.to_owned(),
        writer_group: R237_WRITER_GROUP.to_owned(),
        marker_root_purpose: R237_MARKER_ROOT_PURPOSE.to_owned(),
    }
}

fn canonical_sha256(value: &impl Serialize) -> Option<String> {
    let bytes = serde_jcs::to_vec(value).ok()?;
    Some(format!("{:x}", Sha256::digest(bytes)))
}

fn valid_target(target: &R237BootstrapPlanTargetV1) -> bool {
    !target.host.is_empty()
        && target.host == target.host.trim()
        && target
            .host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b':'))
        && valid_sha256(&target.identity_sha256)
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn valid_bucket_name(value: &str) -> bool {
    value.len() >= 3
        && value.len() <= 63
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !value.starts_with('-')
        && !value.ends_with('-')
}

fn unique_identifiers(values: &[String]) -> bool {
    values.iter().all(|value| valid_identifier(value))
        && values.iter().collect::<BTreeSet<_>>().len() == values.len()
}

fn unique_bucket_names(values: &[String]) -> bool {
    values.iter().all(|value| valid_bucket_name(value))
        && values.iter().collect::<BTreeSet<_>>().len() == values.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDENTITY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn inventory() -> R237BootstrapPlanInventoryV1 {
        R237BootstrapPlanInventoryV1 {
            schema_version: R237_BOOTSTRAP_PLAN_INVENTORY_V1_SCHEMA.to_owned(),
            target: R237BootstrapPlanTargetV1 {
                host: R237_NUC_HOST.to_owned(),
                identity_sha256: IDENTITY.to_owned(),
            },
            store_inventory_complete: true,
            bucket_inventory_complete: true,
            existing_store_ids: Vec::new(),
            existing_bucket_names: Vec::new(),
            hdd_members: vec![
                member("physical-a", 25),
                member("physical-b", 26),
                member("physical-c", 27),
                member("physical-d", 28),
            ],
        }
    }

    fn member(id: &str, gib: u64) -> R237BootstrapPlanHddMemberV1 {
        R237BootstrapPlanHddMemberV1 {
            physical_member_id: id.to_owned(),
            enclosure_id: "qnap-d800c".to_owned(),
            media_kind: "hdd".to_owned(),
            mounted: true,
            writable: true,
            degraded: false,
            smart_status: "passed".to_owned(),
            available_bytes: gib * 1024 * 1024 * 1024,
        }
    }

    #[test]
    fn fixed_r237_plan_is_redacted_jcs_bound_non_worm_and_not_apply_compatible() {
        let assessment = assess_r237_nuc_bootstrap_plan(&inventory());
        assert!(assessment.ready);
        assert_eq!(assessment.denial_reason, None);
        let output = assessment.plan.expect("ready plan");
        assert_eq!(
            output.plan.canonical_programme_main_revision,
            R237_CANONICAL_PROGRAMME_MAIN_REVISION
        );
        assert_eq!(
            output.plan.transaction_document_source_revision,
            R237_TRANSACTION_DOCUMENT_SOURCE_REVISION
        );
        assert_eq!(
            output.plan.transaction_document_sha256,
            R237_TRANSACTION_DOCUMENT_SHA256
        );
        assert_eq!(output.plan.target.host, R237_NUC_HOST);
        assert_eq!(output.plan.provision.store_id, R237_STORE_ID);
        assert_eq!(output.plan.provision.bucket_name, R237_BUCKET_NAME);
        assert_eq!(output.plan.provision.store_class, R237_STORE_CLASS);
        assert_eq!(output.plan.provision.copies, R237_COPIES);
        assert_eq!(
            output.plan.provision.total_object_body_limit_bytes,
            R237_TOTAL_OBJECT_BODY_LIMIT_BYTES
        );
        assert_eq!(
            output.plan.provision.per_object_limit_bytes,
            R237_PER_OBJECT_LIMIT_BYTES
        );
        assert_eq!(
            output.plan.provision.minimum_free_bytes_per_selected_hdd,
            R237_MINIMUM_FREE_BYTES_PER_SELECTED_HDD
        );
        assert_eq!(
            output.plan.provision.corpus_key_prefix,
            R237_CORPUS_KEY_PREFIX
        );
        assert_eq!(
            output.plan.provision.receipt_key_prefix,
            R237_RECEIPT_KEY_PREFIX
        );
        assert_eq!(output.plan.provision.writer_group, R237_WRITER_GROUP);
        assert_eq!(
            output.plan.provision.marker_root_purpose,
            R237_MARKER_ROOT_PURPOSE
        );
        assert!(output.plan.non_mutating);
        assert!(output.plan.non_worm_bootstrap_only);
        assert!(!output.plan.later_guarded_apply_compatible);
        assert_eq!(
            output.plan_sha256,
            canonical_sha256(&output.plan).expect("digest")
        );
        let encoded = serde_json::to_string(&output).expect("serializes");
        assert!(!encoded.contains("physical-a"));
        assert!(!encoded.contains("qnap-d800c"));
    }

    #[test]
    fn plan_digest_is_deterministic_across_input_member_order() {
        let first = assess_r237_nuc_bootstrap_plan(&inventory());
        let mut reordered = inventory();
        reordered.hdd_members.reverse();
        assert_eq!(first, assess_r237_nuc_bootstrap_plan(&reordered));
    }

    #[test]
    fn target_namespace_and_unknown_inventory_are_terminal_refusals() {
        let mut wrong_target = inventory();
        wrong_target.target.host = "192.168.0.48".to_owned();
        assert_eq!(
            assess_r237_nuc_bootstrap_plan(&wrong_target).denial_reason,
            Some(R237BootstrapPlanDenialReasonV1::TargetMismatch)
        );

        let mut store = inventory();
        store.existing_store_ids.push(R237_STORE_ID.to_owned());
        assert_eq!(
            assess_r237_nuc_bootstrap_plan(&store).denial_reason,
            Some(R237BootstrapPlanDenialReasonV1::ExistingStoreConflict)
        );

        let mut bucket = inventory();
        bucket
            .existing_bucket_names
            .push(R237_BUCKET_NAME.to_owned());
        assert_eq!(
            assess_r237_nuc_bootstrap_plan(&bucket).denial_reason,
            Some(R237BootstrapPlanDenialReasonV1::ExistingBucketConflict)
        );

        let mut unknown = inventory();
        unknown.bucket_inventory_complete = false;
        assert_eq!(
            assess_r237_nuc_bootstrap_plan(&unknown).denial_reason,
            Some(R237BootstrapPlanDenialReasonV1::InvalidInventory)
        );

        let mut invalid_identity = inventory();
        invalid_identity.target.identity_sha256 = "not-a-digest".to_owned();
        assert_eq!(
            assess_r237_nuc_bootstrap_plan(&invalid_identity).denial_reason,
            Some(R237BootstrapPlanDenialReasonV1::InvalidInventory)
        );
    }

    #[test]
    fn physical_aliases_and_each_health_requirement_are_refused() {
        let mut aliases = inventory();
        aliases.hdd_members[3].physical_member_id = "physical-c".to_owned();
        assert_eq!(
            assess_r237_nuc_bootstrap_plan(&aliases).denial_reason,
            Some(R237BootstrapPlanDenialReasonV1::AmbiguousPhysicalMembers)
        );

        for change in [
            "unmounted",
            "unwritable",
            "degraded",
            "smart-warning",
            "small",
        ] {
            let mut changed = inventory();
            for member in &mut changed.hdd_members[..2] {
                match change {
                    "unmounted" => member.mounted = false,
                    "unwritable" => member.writable = false,
                    "degraded" => member.degraded = true,
                    "smart-warning" => member.smart_status = "warning".to_owned(),
                    "small" => {
                        member.available_bytes = R237_MINIMUM_FREE_BYTES_PER_SELECTED_HDD - 1
                    }
                    _ => unreachable!("fixed change label"),
                }
            }
            assert_eq!(
                assess_r237_nuc_bootstrap_plan(&changed).denial_reason,
                Some(R237BootstrapPlanDenialReasonV1::InsufficientHealthyDistinctHdds)
            );
        }
    }
}
