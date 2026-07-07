use super::format_size_label;
use crate::resource::{ResourcePolicyDisplay, ResourcePolicySummary};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceCap<T> {
    Automatic,
    Explicit(T),
}

impl<T: Copy> ResourceCap<T> {
    pub fn explicit_value(&self) -> Option<T> {
        match self {
            Self::Automatic => None,
            Self::Explicit(value) => Some(*value),
        }
    }
}

impl ResourceCap<u16> {
    pub fn parse_count(value: &str) -> Result<Self, String> {
        parse_resource_cap(value, |input| {
            input
                .parse::<u16>()
                .map_err(|_| format!("expected 'auto' or a positive whole number, got '{input}'"))
        })
    }
}

impl ResourceCap<u64> {
    pub fn parse_bytes(value: &str) -> Result<Self, String> {
        parse_resource_cap(value, |input| {
            input
                .parse::<u64>()
                .map_err(|_| format!("expected 'auto' or a byte count, got '{input}'"))
        })
    }
}

fn parse_resource_cap<T>(
    value: &str,
    parse_explicit: impl FnOnce(&str) -> Result<T, String>,
) -> Result<ResourceCap<T>, String>
where
    T: PartialEq + From<u8>,
{
    let normalized = value.trim().to_ascii_lowercase();
    if matches!(normalized.as_str(), "auto" | "automatic") {
        return Ok(ResourceCap::Automatic);
    }

    let explicit = parse_explicit(value)?;
    if explicit == T::from(0) {
        return Err("explicit resource caps must be greater than zero".to_string());
    }

    Ok(ResourceCap::Explicit(explicit))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceUsePlan {
    pub policy: ResourcePolicySummary,
    pub core_cap: ResourceCap<u16>,
    pub memory_cap_bytes: ResourceCap<u64>,
    pub ssd_reserve_bytes: ResourceCap<u64>,
    pub hdd_write_concurrency: ResourceCap<u16>,
}

impl ResourceUsePlan {
    pub fn new(
        policy: ResourcePolicySummary,
        core_cap: ResourceCap<u16>,
        memory_cap_bytes: ResourceCap<u64>,
        ssd_reserve_bytes: ResourceCap<u64>,
        hdd_write_concurrency: ResourceCap<u16>,
    ) -> Self {
        Self {
            policy,
            core_cap,
            memory_cap_bytes,
            ssd_reserve_bytes,
            hdd_write_concurrency,
        }
    }

    pub fn display_data(&self) -> ResourcePolicyDisplay {
        let mut display = self.policy.display_data();
        display.worker_counts_label = format!(
            "{}; core use {}",
            display.worker_counts_label,
            count_cap_label(self.core_cap)
        );
        display.memory_budget_label = bytes_cap_label(self.memory_cap_bytes, "cap");
        display.ssd_reserve_label = bytes_cap_label(self.ssd_reserve_bytes, "reserve");
        display.hdd_queue_depth_label = format!(
            "{}; write concurrency {}",
            display.hdd_queue_depth_label,
            count_cap_label(self.hdd_write_concurrency)
        );
        display
    }
}

fn count_cap_label(cap: ResourceCap<u16>) -> String {
    match cap {
        ResourceCap::Automatic => "automatic".to_string(),
        ResourceCap::Explicit(value) => format!("explicit cap {value}"),
    }
}

fn bytes_cap_label(cap: ResourceCap<u64>, noun: &str) -> String {
    match cap {
        ResourceCap::Automatic => "automatic".to_string(),
        ResourceCap::Explicit(bytes) => format!("explicit {noun} {}", format_size_label(bytes)),
    }
}
