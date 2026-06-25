//! Store classes and policy.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreClass {
    ReproducibleCache,
    GeneratedData,
    CriticalMetadata,
    ExportBundle,
    IngestStaging,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IngestMode {
    SsdFirst,
    DirectToHdd,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlacementStrategy {
    WeightedHealthCapacityPerformance,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnclosurePlacement {
    Ignore,
    PreferDistinct,
    RequireDistinct,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetentionPolicy {
    ImmediateDelete,
    TombstoneThenGc,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepairPolicy {
    RestoreFromCopy,
    RedownloadOrRehydrate,
    EvacuateIfCapacityAvailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapacityBehavior {
    RejectWrites,
    BackpressureByPriority,
    MarkRedownloadRequired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialPolicy {
    PerStore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExportPolicy {
    S3,
    ReadOnlyFileExport,
    Disabled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
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
    use super::{CapacityBehavior, EnclosurePlacement, RepairPolicy, StoreClass, StorePolicy};

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
}
