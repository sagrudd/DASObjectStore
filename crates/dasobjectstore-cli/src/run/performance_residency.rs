use super::performance_plan::{PerformancePayload, PerformanceWorkload};
use super::{format_bytes, measure_ssd_capacity, CliError};
use dasobjectstore_metadata::SsdCapacityPolicy;
use std::fs;
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PerformanceSsdResidencyBudget {
    pub(super) safe_bytes: u64,
    pub(super) available_bytes: u64,
}

pub(super) fn performance_ssd_residency_budget(
    ssd_root: &Path,
) -> Result<PerformanceSsdResidencyBudget, CliError> {
    fs::create_dir_all(ssd_root)?;
    let capacity = measure_ssd_capacity(ssd_root)?;
    let policy = SsdCapacityPolicy::default();
    let high_watermark_used_bytes = ((u128::from(capacity.total_bytes)
        * u128::from(policy.high_watermark_percent))
        / 100) as u64;
    let high_watermark_headroom = high_watermark_used_bytes.saturating_sub(capacity.used_bytes());
    let minimum_free_headroom = capacity
        .available_bytes
        .saturating_sub(policy.minimum_free_bytes);
    let safe_bytes = high_watermark_headroom.min(minimum_free_headroom);
    Ok(PerformanceSsdResidencyBudget {
        safe_bytes,
        available_bytes: capacity.available_bytes,
    })
}

pub(super) fn plan_ssd_residency_batches(
    workload: &PerformanceWorkload,
    budget: PerformanceSsdResidencyBudget,
) -> Result<Vec<Vec<PerformancePayload>>, CliError> {
    if workload.payloads.is_empty() {
        return Ok(Vec::new());
    }
    let mut batches = Vec::<Vec<PerformancePayload>>::new();
    let mut current = Vec::<PerformancePayload>::new();
    let mut current_bytes = 0_u64;

    for payload in &workload.payloads {
        let payload_bytes = payload.size_bytes;
        validate_performance_payload_fits_ssd(payload, budget)?;
        let payload_fits_safe_batch = payload_bytes <= budget.safe_bytes;
        let effective_budget = if payload_fits_safe_batch {
            budget.safe_bytes
        } else {
            payload_bytes
        };
        if !current.is_empty() && current_bytes.saturating_add(payload_bytes) > effective_budget {
            batches.push(std::mem::take(&mut current));
            current_bytes = 0;
        }
        current.push(payload.clone());
        current_bytes = current_bytes.saturating_add(payload_bytes);
    }

    if !current.is_empty() {
        batches.push(current);
    }
    Ok(batches)
}

pub(super) fn validate_performance_payload_fits_ssd(
    payload: &PerformancePayload,
    budget: PerformanceSsdResidencyBudget,
) -> Result<(), CliError> {
    if payload.size_bytes > budget.available_bytes {
        return Err(CliError::CommandFailed(format!(
            "performance-test payload {} ({}) is larger than available SSD space ({})",
            payload.relative_path.display(),
            format_bytes(payload.size_bytes as f64),
            format_bytes(budget.available_bytes as f64)
        )));
    }
    Ok(())
}

pub(super) fn performance_ssd_can_admit_payload(
    resident_bytes: u64,
    payload_bytes: u64,
    budget: PerformanceSsdResidencyBudget,
) -> bool {
    resident_bytes.saturating_add(payload_bytes) <= budget.safe_bytes
        || (resident_bytes == 0 && payload_bytes <= budget.available_bytes)
}

#[cfg(test)]
mod tests {
    use super::{performance_ssd_can_admit_payload, PerformanceSsdResidencyBudget};

    #[test]
    fn admission_respects_safe_and_available_capacity_boundaries() {
        let budget = PerformanceSsdResidencyBudget {
            safe_bytes: 100,
            available_bytes: 150,
        };
        assert!(performance_ssd_can_admit_payload(0, 100, budget));
        assert!(!performance_ssd_can_admit_payload(1, 100, budget));
        assert!(performance_ssd_can_admit_payload(0, 150, budget));
        assert!(!performance_ssd_can_admit_payload(1, 150, budget));
        assert!(!performance_ssd_can_admit_payload(0, 151, budget));
    }
}
