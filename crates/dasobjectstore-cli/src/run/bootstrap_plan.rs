//! Offline r237 bootstrap-plan command handler.
//!
//! The only external operation here is a Linux `O_NOATIME | O_NOFOLLOW` read
//! of an operator-supplied inventory. There is deliberately no fallback read,
//! daemon client, process launch, socket, Garage call, registry, credential,
//! marker, admin-job, or apply path.

use super::*;
use dasobjectstore_core::{
    assess_r237_nuc_bootstrap_plan, R237BootstrapPlanAssessmentV1, R237BootstrapPlanInventoryV1,
};
#[cfg(target_os = "linux")]
use std::fs::OpenOptions;
#[cfg(target_os = "linux")]
use std::io::Read;

#[cfg(target_os = "linux")]
const MAX_INVENTORY_BYTES: u64 = 1024 * 1024;

pub(super) fn run_store_r237_bootstrap_plan(
    args: &StoreR237BootstrapPlanArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let bytes = read_inventory_bytes_without_atime(args.inventory())?;
    let assessment = assess_inventory_bytes(&bytes)?;
    serde_json::to_writer_pretty(&mut *writer, &assessment)?;
    writer.write_all(b"\n")?;
    Ok(())
}

fn assess_inventory_bytes(bytes: &[u8]) -> Result<R237BootstrapPlanAssessmentV1, CliError> {
    let inventory: R237BootstrapPlanInventoryV1 = serde_json::from_slice(bytes).map_err(|_| {
        CliError::CommandFailed(
            "r237 bootstrap inventory is malformed or does not satisfy the strict v1 schema"
                .to_owned(),
        )
    })?;
    Ok(assess_r237_nuc_bootstrap_plan(&inventory))
}

/// Opens only a regular inventory file without updating its access time.  The
/// command fails closed on platforms that cannot provide the needed no-atime,
/// no-follow read semantics; it never retries through a normal file read.
#[cfg(target_os = "linux")]
fn read_inventory_bytes_without_atime(path: &Path) -> Result<Vec<u8>, CliError> {
    use std::os::unix::fs::OpenOptionsExt;

    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOATIME | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| {
            CliError::CommandFailed(
                "r237 bootstrap inventory could not be opened with required no-atime/no-follow protection"
                    .to_owned(),
            )
        })?;
    if !file.metadata().map_err(CliError::Io)?.file_type().is_file() {
        return Err(CliError::CommandFailed(
            "r237 bootstrap inventory must be a regular file".to_owned(),
        ));
    }
    let mut bytes = Vec::new();
    let mut limited = file.take(MAX_INVENTORY_BYTES + 1);
    limited.read_to_end(&mut bytes).map_err(CliError::Io)?;
    if bytes.len() as u64 > MAX_INVENTORY_BYTES {
        return Err(CliError::CommandFailed(
            "r237 bootstrap inventory exceeds the 1 MiB read-only limit".to_owned(),
        ));
    }
    Ok(bytes)
}

#[cfg(not(target_os = "linux"))]
fn read_inventory_bytes_without_atime(_path: &Path) -> Result<Vec<u8>, CliError> {
    Err(CliError::CommandFailed(
        "r237 bootstrap planning requires Linux O_NOATIME/O_NOFOLLOW inventory reads; refusing an unprotected read"
            .to_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dasobjectstore_core::{
        R237BootstrapPlanDenialReasonV1, R237BootstrapPlanHddMemberV1, R237BootstrapPlanTargetV1,
        R237_BOOTSTRAP_PLAN_INVENTORY_V1_SCHEMA, R237_NUC_HOST,
    };

    fn inventory_json() -> Vec<u8> {
        let inventory = R237BootstrapPlanInventoryV1 {
            schema_version: R237_BOOTSTRAP_PLAN_INVENTORY_V1_SCHEMA.to_owned(),
            target: R237BootstrapPlanTargetV1 {
                host: R237_NUC_HOST.to_owned(),
                identity_sha256: "a".repeat(64),
            },
            store_inventory_complete: true,
            bucket_inventory_complete: true,
            existing_store_ids: Vec::new(),
            existing_bucket_names: Vec::new(),
            hdd_members: (0..3)
                .map(|index| R237BootstrapPlanHddMemberV1 {
                    physical_member_id: format!("physical-{index}"),
                    enclosure_id: "qnap-d800c".to_owned(),
                    media_kind: "hdd".to_owned(),
                    mounted: true,
                    writable: true,
                    degraded: false,
                    smart_status: "passed".to_owned(),
                    available_bytes: 24 * 1024 * 1024 * 1024,
                })
                .collect(),
        };
        serde_json::to_vec(&inventory).expect("serializes")
    }

    #[test]
    fn byte_only_assessment_is_redacted_and_has_no_writer_or_host_dependency() {
        let assessment = assess_inventory_bytes(&inventory_json()).expect("assessment");
        assert!(assessment.ready);
        let output = assessment.plan.expect("plan");
        let encoded = serde_json::to_string(&output).expect("serializes");
        assert!(!encoded.contains("physical-0"));
        assert!(!encoded.contains("qnap-d800c"));
        assert!(!encoded.contains("credential"));
        assert!(output.plan.non_mutating);
        assert!(!output.plan.later_guarded_apply_compatible);
    }

    #[test]
    fn malformed_or_incomplete_input_never_causes_a_discovery_fallback() {
        assert!(assess_inventory_bytes(b"not-json").is_err());
        let mut inventory: R237BootstrapPlanInventoryV1 =
            serde_json::from_slice(&inventory_json()).expect("fixture");
        inventory.store_inventory_complete = false;
        let bytes = serde_json::to_vec(&inventory).expect("serializes");
        let assessment = assess_inventory_bytes(&bytes).expect("assessment");
        assert!(!assessment.ready);
        assert_eq!(
            assessment.denial_reason,
            Some(R237BootstrapPlanDenialReasonV1::InvalidInventory)
        );
        assert!(assessment.plan.is_none());
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn unsupported_platform_refuses_instead_of_normal_file_read() {
        let error = read_inventory_bytes_without_atime(Path::new("ignored"))
            .expect_err("unprotected read must fail");
        assert!(error.to_string().contains("refusing an unprotected read"));
    }
}
