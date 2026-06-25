//! Store classes and policy.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum StoreClass {
    ReproducibleCache,
    GeneratedData,
    CriticalMetadata,
    ExportBundle,
    IngestStaging,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum IngestMode {
    SsdFirst,
    DirectToHdd,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PlacementStrategy {
    WeightedHealthCapacityPerformance,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum EnclosurePlacement {
    Ignore,
    PreferDistinct,
    RequireDistinct,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RetentionPolicy {
    ImmediateDelete,
    TombstoneThenGc,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RepairPolicy {
    RestoreFromCopy,
    RedownloadOrRehydrate,
    EvacuateIfCapacityAvailable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CapacityBehavior {
    RejectWrites,
    BackpressureByPriority,
    MarkRedownloadRequired,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CredentialPolicy {
    PerStore,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ExportPolicy {
    S3,
    ReadOnlyFileExport,
    Disabled,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StorePolicy {
    pub class: StoreClass,
    pub ingest_mode: IngestMode,
    pub copies: u8,
    pub placement_strategy: PlacementStrategy,
    pub enclosure_placement: EnclosurePlacement,
    pub retention_policy: RetentionPolicy,
    pub repair_policy: RepairPolicy,
    pub capacity_behavior: CapacityBehavior,
    pub credential_policy: CredentialPolicy,
    pub export_policy: ExportPolicy,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PoolPolicyDefaults {
    pub ingest_mode: IngestMode,
    pub copies: u8,
    pub placement_strategy: PlacementStrategy,
    pub enclosure_placement: EnclosurePlacement,
    pub retention_policy: RetentionPolicy,
    pub repair_policy: RepairPolicy,
    pub capacity_behavior: CapacityBehavior,
    pub credential_policy: CredentialPolicy,
    pub export_policy: ExportPolicy,
}

impl PoolPolicyDefaults {
    pub fn generated_data_defaults() -> Self {
        Self::from_policy(StorePolicy::defaults_for(StoreClass::GeneratedData))
    }

    pub fn apply_overrides(
        &self,
        class: StoreClass,
        overrides: StorePolicyOverrides,
    ) -> StorePolicy {
        StorePolicy {
            class,
            ingest_mode: overrides.ingest_mode.unwrap_or(self.ingest_mode),
            copies: overrides.copies.unwrap_or(self.copies),
            placement_strategy: overrides
                .placement_strategy
                .unwrap_or(self.placement_strategy),
            enclosure_placement: overrides
                .enclosure_placement
                .unwrap_or(self.enclosure_placement),
            retention_policy: overrides.retention_policy.unwrap_or(self.retention_policy),
            repair_policy: overrides.repair_policy.unwrap_or(self.repair_policy),
            capacity_behavior: overrides
                .capacity_behavior
                .unwrap_or(self.capacity_behavior),
            credential_policy: overrides
                .credential_policy
                .unwrap_or(self.credential_policy),
            export_policy: overrides.export_policy.unwrap_or(self.export_policy),
        }
    }

    fn from_policy(policy: StorePolicy) -> Self {
        Self {
            ingest_mode: policy.ingest_mode,
            copies: policy.copies,
            placement_strategy: policy.placement_strategy,
            enclosure_placement: policy.enclosure_placement,
            retention_policy: policy.retention_policy,
            repair_policy: policy.repair_policy,
            capacity_behavior: policy.capacity_behavior,
            credential_policy: policy.credential_policy,
            export_policy: policy.export_policy,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct StorePolicyOverrides {
    pub ingest_mode: Option<IngestMode>,
    pub copies: Option<u8>,
    pub placement_strategy: Option<PlacementStrategy>,
    pub enclosure_placement: Option<EnclosurePlacement>,
    pub retention_policy: Option<RetentionPolicy>,
    pub repair_policy: Option<RepairPolicy>,
    pub capacity_behavior: Option<CapacityBehavior>,
    pub credential_policy: Option<CredentialPolicy>,
    pub export_policy: Option<ExportPolicy>,
}

impl StorePolicy {
    pub fn defaults_for(class: StoreClass) -> Self {
        match class {
            StoreClass::ReproducibleCache => Self {
                class,
                ingest_mode: IngestMode::SsdFirst,
                copies: 1,
                placement_strategy: PlacementStrategy::WeightedHealthCapacityPerformance,
                enclosure_placement: EnclosurePlacement::Ignore,
                retention_policy: RetentionPolicy::ImmediateDelete,
                repair_policy: RepairPolicy::EvacuateIfCapacityAvailable,
                capacity_behavior: CapacityBehavior::MarkRedownloadRequired,
                credential_policy: CredentialPolicy::PerStore,
                export_policy: ExportPolicy::S3,
            },
            StoreClass::GeneratedData => Self {
                class,
                ingest_mode: IngestMode::SsdFirst,
                copies: 2,
                placement_strategy: PlacementStrategy::WeightedHealthCapacityPerformance,
                enclosure_placement: EnclosurePlacement::PreferDistinct,
                retention_policy: RetentionPolicy::TombstoneThenGc,
                repair_policy: RepairPolicy::RestoreFromCopy,
                capacity_behavior: CapacityBehavior::BackpressureByPriority,
                credential_policy: CredentialPolicy::PerStore,
                export_policy: ExportPolicy::S3,
            },
            StoreClass::CriticalMetadata => Self {
                class,
                ingest_mode: IngestMode::SsdFirst,
                copies: 3,
                placement_strategy: PlacementStrategy::WeightedHealthCapacityPerformance,
                enclosure_placement: EnclosurePlacement::PreferDistinct,
                retention_policy: RetentionPolicy::TombstoneThenGc,
                repair_policy: RepairPolicy::RestoreFromCopy,
                capacity_behavior: CapacityBehavior::RejectWrites,
                credential_policy: CredentialPolicy::PerStore,
                export_policy: ExportPolicy::S3,
            },
            StoreClass::ExportBundle => Self {
                class,
                ingest_mode: IngestMode::SsdFirst,
                copies: 2,
                placement_strategy: PlacementStrategy::WeightedHealthCapacityPerformance,
                enclosure_placement: EnclosurePlacement::PreferDistinct,
                retention_policy: RetentionPolicy::TombstoneThenGc,
                repair_policy: RepairPolicy::RestoreFromCopy,
                capacity_behavior: CapacityBehavior::BackpressureByPriority,
                credential_policy: CredentialPolicy::PerStore,
                export_policy: ExportPolicy::ReadOnlyFileExport,
            },
            StoreClass::IngestStaging => Self {
                class,
                ingest_mode: IngestMode::SsdFirst,
                copies: 1,
                placement_strategy: PlacementStrategy::WeightedHealthCapacityPerformance,
                enclosure_placement: EnclosurePlacement::Ignore,
                retention_policy: RetentionPolicy::ImmediateDelete,
                repair_policy: RepairPolicy::RedownloadOrRehydrate,
                capacity_behavior: CapacityBehavior::BackpressureByPriority,
                credential_policy: CredentialPolicy::PerStore,
                export_policy: ExportPolicy::Disabled,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CapacityBehavior, EnclosurePlacement, ExportPolicy, PoolPolicyDefaults, RepairPolicy,
        StoreClass, StorePolicy, StorePolicyOverrides,
    };

    #[test]
    fn reproducible_cache_defaults_to_single_copy_cache_behavior() {
        let policy = StorePolicy::defaults_for(StoreClass::ReproducibleCache);

        assert_eq!(policy.copies, 1);
        assert_eq!(policy.enclosure_placement, EnclosurePlacement::Ignore);
        assert_eq!(
            policy.repair_policy,
            RepairPolicy::EvacuateIfCapacityAvailable
        );
        assert_eq!(
            policy.capacity_behavior,
            CapacityBehavior::MarkRedownloadRequired
        );
    }

    #[test]
    fn generated_data_defaults_to_two_copies() {
        let policy = StorePolicy::defaults_for(StoreClass::GeneratedData);

        assert_eq!(policy.copies, 2);
        assert_eq!(
            policy.enclosure_placement,
            EnclosurePlacement::PreferDistinct
        );
        assert_eq!(policy.repair_policy, RepairPolicy::RestoreFromCopy);
    }

    #[test]
    fn critical_metadata_defaults_to_three_copies() {
        let policy = StorePolicy::defaults_for(StoreClass::CriticalMetadata);

        assert_eq!(policy.copies, 3);
        assert_eq!(policy.capacity_behavior, CapacityBehavior::RejectWrites);
    }

    #[test]
    fn round_trips_store_policy() {
        let policy = StorePolicy::defaults_for(StoreClass::GeneratedData);

        let encoded = serde_json::to_string(&policy).expect("policy serializes");
        let decoded: StorePolicy = serde_json::from_str(&encoded).expect("policy deserializes");

        assert_eq!(decoded, policy);
    }

    #[test]
    fn pool_defaults_materialize_store_policy() {
        let defaults = PoolPolicyDefaults::generated_data_defaults();

        let policy =
            defaults.apply_overrides(StoreClass::ExportBundle, StorePolicyOverrides::default());

        assert_eq!(policy.class, StoreClass::ExportBundle);
        assert_eq!(policy.copies, 2);
        assert_eq!(
            policy.enclosure_placement,
            EnclosurePlacement::PreferDistinct
        );
        assert_eq!(policy.export_policy, ExportPolicy::S3);
    }

    #[test]
    fn per_store_overrides_replace_pool_defaults() {
        let defaults = PoolPolicyDefaults::generated_data_defaults();
        let overrides = StorePolicyOverrides {
            copies: Some(3),
            capacity_behavior: Some(CapacityBehavior::RejectWrites),
            export_policy: Some(ExportPolicy::ReadOnlyFileExport),
            ..StorePolicyOverrides::default()
        };

        let policy = defaults.apply_overrides(StoreClass::CriticalMetadata, overrides);

        assert_eq!(policy.class, StoreClass::CriticalMetadata);
        assert_eq!(policy.copies, 3);
        assert_eq!(policy.capacity_behavior, CapacityBehavior::RejectWrites);
        assert_eq!(policy.export_policy, ExportPolicy::ReadOnlyFileExport);
        assert_eq!(policy.repair_policy, RepairPolicy::RestoreFromCopy);
    }
}
