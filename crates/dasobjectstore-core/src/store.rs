//! Store classes and policy.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt::{self, Display};
use std::str::FromStr;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum StoreClass {
    ReproducibleCache,
    GeneratedData,
    CriticalMetadata,
    ExportBundle,
    IngestStaging,
}

impl StoreClass {
    pub const ALL: [Self; 5] = [
        Self::ReproducibleCache,
        Self::GeneratedData,
        Self::CriticalMetadata,
        Self::ExportBundle,
        Self::IngestStaging,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::ReproducibleCache => "reproducible_cache",
            Self::GeneratedData => "generated_data",
            Self::CriticalMetadata => "critical_metadata",
            Self::ExportBundle => "export_bundle",
            Self::IngestStaging => "ingest_staging",
        }
    }
}

impl FromStr for StoreClass {
    type Err = StoreClassParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "reproducible_cache" => Ok(Self::ReproducibleCache),
            "generated_data" => Ok(Self::GeneratedData),
            "critical_metadata" => Ok(Self::CriticalMetadata),
            "export_bundle" => Ok(Self::ExportBundle),
            "ingest_staging" => Ok(Self::IngestStaging),
            _ => Err(StoreClassParseError {
                value: value.to_string(),
            }),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreClassParseError {
    value: String,
}

impl Display for StoreClassParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "unknown store class `{}`; expected one of: reproducible_cache, generated_data, critical_metadata, export_bundle, ingest_staging",
            self.value
        )
    }
}

impl std::error::Error for StoreClassParseError {}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum IngestMode {
    SsdFirst,
    DirectToHdd,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AcknowledgementPolicy {
    AfterSsdIngest,
    AfterHddPlacement,
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
pub enum MutabilityPolicy {
    Immutable,
    Mutable,
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CapacityPolicy {
    /// `None` preserves the legacy appliance policy until a finite limit is
    /// explicitly configured. New bounded profiles must provide a value.
    pub logical_limit_bytes: Option<u64>,
    pub backend_reserve_bytes: u64,
    pub warning_threshold_basis_points: u16,
    pub critical_threshold_basis_points: u16,
}

impl Default for CapacityPolicy {
    fn default() -> Self {
        Self {
            logical_limit_bytes: None,
            backend_reserve_bytes: 0,
            warning_threshold_basis_points: 8_000,
            critical_threshold_basis_points: 9_500,
        }
    }
}

impl CapacityPolicy {
    pub fn bounded(logical_limit_bytes: u64, backend_reserve_bytes: u64) -> Self {
        Self {
            logical_limit_bytes: Some(logical_limit_bytes),
            backend_reserve_bytes,
            ..Self::default()
        }
    }

    pub fn validation_error(&self) -> Option<CapacityPolicyValidationError> {
        if self
            .logical_limit_bytes
            .is_some_and(|limit| limit == 0 || self.backend_reserve_bytes >= limit)
        {
            return Some(CapacityPolicyValidationError::InvalidLimitOrReserve);
        }
        if self.warning_threshold_basis_points > 10_000
            || self.critical_threshold_basis_points > 10_000
            || self.warning_threshold_basis_points > self.critical_threshold_basis_points
        {
            return Some(CapacityPolicyValidationError::InvalidThresholds);
        }
        None
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapacityAdmissionInput {
    pub requested_bytes: u64,
    pub copy_count: u8,
    pub requires_ssd_staging: bool,
    pub used_bytes: u64,
    pub reserved_bytes: u64,
    pub backend_free_bytes: u64,
    pub ssd_free_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapacityAdmission {
    pub requires_ssd_staging: bool,
    pub logical_available_bytes: Option<u64>,
    pub backend_available_bytes: u64,
    pub ssd_available_bytes: u64,
    pub required_backend_bytes: u64,
    pub required_ssd_bytes: u64,
}

impl CapacityAdmission {
    pub fn strictest_available_bytes(&self) -> Option<u64> {
        let mut available = if self.requires_ssd_staging {
            self.backend_available_bytes.min(self.ssd_available_bytes)
        } else {
            self.backend_available_bytes
        };
        if let Some(logical) = self.logical_available_bytes {
            available = available.min(logical);
        }
        Some(available)
    }
}

pub fn evaluate_capacity_admission(
    policy: &CapacityPolicy,
    input: CapacityAdmissionInput,
) -> Result<CapacityAdmission, CapacityAdmissionError> {
    if input.copy_count == 0 {
        return Err(CapacityAdmissionError::InvalidCopyCount);
    }
    let required_backend_bytes = input
        .requested_bytes
        .checked_mul(u64::from(input.copy_count))
        .ok_or(CapacityAdmissionError::Overflow)?;
    let logical_available_bytes = policy.logical_limit_bytes.map(|limit| {
        limit
            .saturating_sub(policy.backend_reserve_bytes)
            .saturating_sub(input.used_bytes)
            .saturating_sub(input.reserved_bytes)
    });
    let backend_available_bytes = input
        .backend_free_bytes
        .saturating_sub(policy.backend_reserve_bytes);
    let admission = CapacityAdmission {
        requires_ssd_staging: input.requires_ssd_staging,
        logical_available_bytes,
        backend_available_bytes,
        ssd_available_bytes: input.ssd_free_bytes,
        required_backend_bytes,
        required_ssd_bytes: input
            .requires_ssd_staging
            .then_some(input.requested_bytes)
            .unwrap_or(0),
    };
    if logical_available_bytes.is_some_and(|available| input.requested_bytes > available) {
        return Err(CapacityAdmissionError::LogicalQuota {
            available_bytes: logical_available_bytes.unwrap_or_default(),
        });
    }
    if required_backend_bytes > backend_available_bytes {
        return Err(CapacityAdmissionError::BackendReserve {
            available_bytes: backend_available_bytes,
        });
    }
    if input.requires_ssd_staging && input.requested_bytes > input.ssd_free_bytes {
        return Err(CapacityAdmissionError::SsdStaging {
            available_bytes: input.ssd_free_bytes,
        });
    }
    Ok(admission)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapacityAdmissionError {
    InvalidCopyCount,
    Overflow,
    LogicalQuota { available_bytes: u64 },
    BackendReserve { available_bytes: u64 },
    SsdStaging { available_bytes: u64 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapacityPolicyValidationError {
    InvalidLimitOrReserve,
    InvalidThresholds,
}

impl Display for CapacityPolicyValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimitOrReserve => {
                formatter.write_str("capacity logical limit must be positive and exceed backend reserve")
            }
            Self::InvalidThresholds => formatter.write_str(
                "capacity warning/critical thresholds must be ordered and within 0..=10000 basis points",
            ),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogicalObjectVersionCharge {
    logical_size_bytes: u64,
}

impl LogicalObjectVersionCharge {
    /// Charge one logical object version at its full logical size. Physical
    /// copy amplification and staging are intentionally not part of this
    /// logical quota primitive; admission reports those separately.
    pub const fn new(logical_size_bytes: u64) -> Self {
        Self { logical_size_bytes }
    }

    pub const fn logical_size_bytes(&self) -> u64 {
        self.logical_size_bytes
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapacityReservationLedger {
    policy: CapacityPolicy,
    used_bytes: u64,
    reservations: HashMap<String, u64>,
}

pub const CAPACITY_LEDGER_SNAPSHOT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CapacityReservationLedgerSnapshot {
    pub schema_version: u32,
    pub policy: CapacityPolicy,
    pub used_bytes: u64,
    pub reservations: BTreeMap<String, u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapacityPressureState {
    Unbounded,
    Normal,
    Warning,
    Critical,
    OverQuota,
}

impl CapacityReservationLedger {
    pub fn new(policy: CapacityPolicy, used_bytes: u64) -> Result<Self, CapacityLedgerError> {
        if let Some(error) = policy.validation_error() {
            return Err(CapacityLedgerError::InvalidPolicy(error));
        }
        Ok(Self {
            policy,
            used_bytes,
            reservations: HashMap::new(),
        })
    }

    pub fn used_bytes(&self) -> u64 {
        self.used_bytes
    }

    pub fn reserved_bytes(&self) -> u64 {
        self.reservations.values().copied().sum()
    }

    pub fn reservation_bytes(&self, reservation_id: &str) -> Option<u64> {
        self.reservations.get(reservation_id).copied()
    }

    pub fn policy(&self) -> &CapacityPolicy {
        &self.policy
    }

    pub fn snapshot(&self) -> CapacityReservationLedgerSnapshot {
        CapacityReservationLedgerSnapshot {
            schema_version: CAPACITY_LEDGER_SNAPSHOT_SCHEMA_VERSION,
            policy: self.policy.clone(),
            used_bytes: self.used_bytes,
            reservations: self
                .reservations
                .iter()
                .map(|(id, bytes)| (id.clone(), *bytes))
                .collect(),
        }
    }

    pub fn from_snapshot(
        snapshot: CapacityReservationLedgerSnapshot,
    ) -> Result<Self, CapacityLedgerError> {
        if snapshot.schema_version != CAPACITY_LEDGER_SNAPSHOT_SCHEMA_VERSION {
            return Err(CapacityLedgerError::InvalidSnapshotSchema {
                schema_version: snapshot.schema_version,
            });
        }
        let mut ledger = Self::new(snapshot.policy, snapshot.used_bytes)?;
        for (reservation_id, bytes) in snapshot.reservations {
            ledger.reserve(reservation_id, bytes)?;
        }
        Ok(ledger)
    }

    /// Replace capacity policy without deleting data. Lowering a limit below
    /// current usage is allowed so operators can remediate an over-quota store;
    /// subsequent reservations remain rejected until usage falls below policy.
    pub fn update_policy(&mut self, policy: CapacityPolicy) -> Result<(), CapacityLedgerError> {
        if let Some(error) = policy.validation_error() {
            return Err(CapacityLedgerError::InvalidPolicy(error));
        }
        self.policy = policy;
        Ok(())
    }

    pub fn pressure_state(&self) -> CapacityPressureState {
        let Some(limit) = self.policy.logical_limit_bytes else {
            return CapacityPressureState::Unbounded;
        };
        let effective_limit = limit.saturating_sub(self.policy.backend_reserve_bytes);
        let usage = self.used_bytes.saturating_add(self.reserved_bytes());
        if usage > effective_limit {
            return CapacityPressureState::OverQuota;
        }
        let basis_points = (u128::from(usage) * 10_000 / u128::from(effective_limit)) as u64;
        if basis_points >= u64::from(self.policy.critical_threshold_basis_points) {
            CapacityPressureState::Critical
        } else if basis_points >= u64::from(self.policy.warning_threshold_basis_points) {
            CapacityPressureState::Warning
        } else {
            CapacityPressureState::Normal
        }
    }

    pub fn available_bytes(&self) -> Option<u64> {
        self.policy.logical_limit_bytes.map(|limit| {
            limit
                .saturating_sub(self.policy.backend_reserve_bytes)
                .saturating_sub(self.used_bytes)
                .saturating_sub(self.reserved_bytes())
        })
    }

    pub fn reserve(
        &mut self,
        reservation_id: impl Into<String>,
        bytes: u64,
    ) -> Result<(), CapacityLedgerError> {
        let reservation_id = reservation_id.into();
        if reservation_id.trim().is_empty() || self.reservations.contains_key(&reservation_id) {
            return Err(CapacityLedgerError::InvalidReservationId);
        }
        let outstanding = self
            .used_bytes
            .checked_add(self.reserved_bytes())
            .and_then(|value| value.checked_add(bytes))
            .ok_or(CapacityLedgerError::Overflow)?;
        if let Some(limit) = self.policy.logical_limit_bytes {
            let admitted_limit = limit.saturating_sub(self.policy.backend_reserve_bytes);
            if outstanding > admitted_limit {
                return Err(CapacityLedgerError::InsufficientCapacity {
                    available_bytes: admitted_limit
                        .saturating_sub(self.used_bytes.saturating_add(self.reserved_bytes())),
                });
            }
        }
        self.reservations.insert(reservation_id, bytes);
        Ok(())
    }

    /// Reserve a complete logical object version. Two versions with the same
    /// content still consume two logical charges; deduplication and physical
    /// placement are evaluated by separate backend/admission contracts.
    pub fn reserve_object_version(
        &mut self,
        reservation_id: impl Into<String>,
        charge: LogicalObjectVersionCharge,
    ) -> Result<(), CapacityLedgerError> {
        self.reserve(reservation_id, charge.logical_size_bytes)
    }

    pub fn commit(&mut self, reservation_id: &str) -> Result<(), CapacityLedgerError> {
        let bytes = self
            .reservations
            .remove(reservation_id)
            .ok_or(CapacityLedgerError::UnknownReservation)?;
        self.used_bytes = self
            .used_bytes
            .checked_add(bytes)
            .ok_or(CapacityLedgerError::Overflow)?;
        Ok(())
    }

    pub fn debit_used_bytes(&mut self, bytes: u64) -> Result<(), CapacityLedgerError> {
        if bytes > self.used_bytes {
            return Err(CapacityLedgerError::UsedBytesUnderflow {
                used_bytes: self.used_bytes,
                requested_bytes: bytes,
            });
        }
        self.used_bytes -= bytes;
        Ok(())
    }

    pub fn release(&mut self, reservation_id: &str) -> Result<u64, CapacityLedgerError> {
        self.reservations
            .remove(reservation_id)
            .ok_or(CapacityLedgerError::UnknownReservation)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CapacityLedgerError {
    InvalidPolicy(CapacityPolicyValidationError),
    InvalidSnapshotSchema {
        schema_version: u32,
    },
    InvalidReservationId,
    UnknownReservation,
    UsedBytesUnderflow {
        used_bytes: u64,
        requested_bytes: u64,
    },
    InsufficientCapacity {
        available_bytes: u64,
    },
    Overflow,
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
    pub acknowledgement_policy: AcknowledgementPolicy,
    pub copies: u8,
    pub placement_strategy: PlacementStrategy,
    pub enclosure_placement: EnclosurePlacement,
    pub retention_policy: RetentionPolicy,
    pub mutability_policy: MutabilityPolicy,
    pub repair_policy: RepairPolicy,
    pub capacity_behavior: CapacityBehavior,
    pub credential_policy: CredentialPolicy,
    pub export_policy: ExportPolicy,
    #[serde(default)]
    pub capacity: CapacityPolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EnclosurePlacementContext {
    pub available_enclosure_count: u8,
}

impl EnclosurePlacementContext {
    pub fn new(available_enclosure_count: u8) -> Self {
        Self {
            available_enclosure_count,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PoolPolicyDefaults {
    pub ingest_mode: IngestMode,
    pub acknowledgement_policy: AcknowledgementPolicy,
    pub copies: u8,
    pub placement_strategy: PlacementStrategy,
    pub enclosure_placement: EnclosurePlacement,
    pub retention_policy: RetentionPolicy,
    pub mutability_policy: MutabilityPolicy,
    pub repair_policy: RepairPolicy,
    pub capacity_behavior: CapacityBehavior,
    pub credential_policy: CredentialPolicy,
    pub export_policy: ExportPolicy,
    #[serde(default)]
    pub capacity: CapacityPolicy,
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
            acknowledgement_policy: overrides
                .acknowledgement_policy
                .unwrap_or(self.acknowledgement_policy),
            copies: overrides.copies.unwrap_or(self.copies),
            placement_strategy: overrides
                .placement_strategy
                .unwrap_or(self.placement_strategy),
            enclosure_placement: overrides
                .enclosure_placement
                .unwrap_or(self.enclosure_placement),
            retention_policy: overrides.retention_policy.unwrap_or(self.retention_policy),
            mutability_policy: overrides
                .mutability_policy
                .unwrap_or(self.mutability_policy),
            repair_policy: overrides.repair_policy.unwrap_or(self.repair_policy),
            capacity_behavior: overrides
                .capacity_behavior
                .unwrap_or(self.capacity_behavior),
            credential_policy: overrides
                .credential_policy
                .unwrap_or(self.credential_policy),
            export_policy: overrides.export_policy.unwrap_or(self.export_policy),
            capacity: overrides.capacity.unwrap_or_else(|| self.capacity.clone()),
        }
    }

    fn from_policy(policy: StorePolicy) -> Self {
        Self {
            ingest_mode: policy.ingest_mode,
            acknowledgement_policy: policy.acknowledgement_policy,
            copies: policy.copies,
            placement_strategy: policy.placement_strategy,
            enclosure_placement: policy.enclosure_placement,
            retention_policy: policy.retention_policy,
            mutability_policy: policy.mutability_policy,
            repair_policy: policy.repair_policy,
            capacity_behavior: policy.capacity_behavior,
            credential_policy: policy.credential_policy,
            export_policy: policy.export_policy,
            capacity: policy.capacity,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct StorePolicyOverrides {
    pub ingest_mode: Option<IngestMode>,
    pub acknowledgement_policy: Option<AcknowledgementPolicy>,
    pub copies: Option<u8>,
    pub placement_strategy: Option<PlacementStrategy>,
    pub enclosure_placement: Option<EnclosurePlacement>,
    pub retention_policy: Option<RetentionPolicy>,
    pub mutability_policy: Option<MutabilityPolicy>,
    pub repair_policy: Option<RepairPolicy>,
    pub capacity_behavior: Option<CapacityBehavior>,
    pub credential_policy: Option<CredentialPolicy>,
    pub export_policy: Option<ExportPolicy>,
    pub capacity: Option<CapacityPolicy>,
}

impl StorePolicy {
    pub fn built_in_defaults() -> Vec<Self> {
        StoreClass::ALL
            .into_iter()
            .map(Self::defaults_for)
            .collect()
    }

    pub fn defaults_for(class: StoreClass) -> Self {
        match class {
            StoreClass::ReproducibleCache => Self {
                class,
                ingest_mode: IngestMode::SsdFirst,
                acknowledgement_policy: AcknowledgementPolicy::AfterSsdIngest,
                copies: 1,
                placement_strategy: PlacementStrategy::WeightedHealthCapacityPerformance,
                enclosure_placement: EnclosurePlacement::Ignore,
                retention_policy: RetentionPolicy::ImmediateDelete,
                mutability_policy: MutabilityPolicy::Immutable,
                repair_policy: RepairPolicy::EvacuateIfCapacityAvailable,
                capacity_behavior: CapacityBehavior::MarkRedownloadRequired,
                credential_policy: CredentialPolicy::PerStore,
                export_policy: ExportPolicy::S3,
                capacity: CapacityPolicy::default(),
            },
            StoreClass::GeneratedData => Self {
                class,
                ingest_mode: IngestMode::SsdFirst,
                acknowledgement_policy: AcknowledgementPolicy::AfterHddPlacement,
                copies: 2,
                placement_strategy: PlacementStrategy::WeightedHealthCapacityPerformance,
                enclosure_placement: EnclosurePlacement::PreferDistinct,
                retention_policy: RetentionPolicy::TombstoneThenGc,
                mutability_policy: MutabilityPolicy::Immutable,
                repair_policy: RepairPolicy::RestoreFromCopy,
                capacity_behavior: CapacityBehavior::BackpressureByPriority,
                credential_policy: CredentialPolicy::PerStore,
                export_policy: ExportPolicy::S3,
                capacity: CapacityPolicy::default(),
            },
            StoreClass::CriticalMetadata => Self {
                class,
                ingest_mode: IngestMode::SsdFirst,
                acknowledgement_policy: AcknowledgementPolicy::AfterHddPlacement,
                copies: 3,
                placement_strategy: PlacementStrategy::WeightedHealthCapacityPerformance,
                enclosure_placement: EnclosurePlacement::PreferDistinct,
                retention_policy: RetentionPolicy::TombstoneThenGc,
                mutability_policy: MutabilityPolicy::Immutable,
                repair_policy: RepairPolicy::RestoreFromCopy,
                capacity_behavior: CapacityBehavior::RejectWrites,
                credential_policy: CredentialPolicy::PerStore,
                export_policy: ExportPolicy::S3,
                capacity: CapacityPolicy::default(),
            },
            StoreClass::ExportBundle => Self {
                class,
                ingest_mode: IngestMode::SsdFirst,
                acknowledgement_policy: AcknowledgementPolicy::AfterHddPlacement,
                copies: 2,
                placement_strategy: PlacementStrategy::WeightedHealthCapacityPerformance,
                enclosure_placement: EnclosurePlacement::PreferDistinct,
                retention_policy: RetentionPolicy::TombstoneThenGc,
                mutability_policy: MutabilityPolicy::Immutable,
                repair_policy: RepairPolicy::RestoreFromCopy,
                capacity_behavior: CapacityBehavior::BackpressureByPriority,
                credential_policy: CredentialPolicy::PerStore,
                export_policy: ExportPolicy::ReadOnlyFileExport,
                capacity: CapacityPolicy::default(),
            },
            StoreClass::IngestStaging => Self {
                class,
                ingest_mode: IngestMode::SsdFirst,
                acknowledgement_policy: AcknowledgementPolicy::AfterSsdIngest,
                copies: 1,
                placement_strategy: PlacementStrategy::WeightedHealthCapacityPerformance,
                enclosure_placement: EnclosurePlacement::Ignore,
                retention_policy: RetentionPolicy::ImmediateDelete,
                mutability_policy: MutabilityPolicy::Mutable,
                repair_policy: RepairPolicy::RedownloadOrRehydrate,
                capacity_behavior: CapacityBehavior::BackpressureByPriority,
                credential_policy: CredentialPolicy::PerStore,
                export_policy: ExportPolicy::Disabled,
                capacity: CapacityPolicy::default(),
            },
        }
    }

    pub fn validate(&self) -> Result<(), StorePolicyValidationErrors> {
        StorePolicyValidationErrors::from_errors(self.validation_errors())
    }

    pub fn validate_for_enclosures(
        &self,
        context: EnclosurePlacementContext,
    ) -> Result<(), StorePolicyValidationErrors> {
        let mut errors = self.validation_errors();
        self.validate_enclosure_availability(context, &mut errors);

        StorePolicyValidationErrors::from_errors(errors)
    }

    fn validation_errors(&self) -> Vec<StorePolicyValidationError> {
        let mut errors = Vec::new();

        if !(1..=3).contains(&self.copies) {
            errors.push(StorePolicyValidationError::InvalidCopyCount {
                copies: self.copies,
            });
        }

        if self.enclosure_placement == EnclosurePlacement::RequireDistinct && self.copies < 2 {
            errors.push(StorePolicyValidationError::DistinctPlacementNeedsMultipleCopies);
        }

        if self.is_protected_class() {
            if self.ingest_mode == IngestMode::DirectToHdd {
                errors.push(StorePolicyValidationError::ProtectedStoreDirectToHdd {
                    class: self.class,
                });
            }
            if self.retention_policy == RetentionPolicy::ImmediateDelete {
                errors.push(StorePolicyValidationError::ProtectedStoreImmediateDelete {
                    class: self.class,
                });
            }
            if self.mutability_policy == MutabilityPolicy::Mutable {
                errors
                    .push(StorePolicyValidationError::ProtectedStoreMutable { class: self.class });
            }
            if self.capacity_behavior == CapacityBehavior::MarkRedownloadRequired {
                errors.push(
                    StorePolicyValidationError::ProtectedStoreMarksRedownloadRequired {
                        class: self.class,
                    },
                );
            }
        }

        if self.class == StoreClass::IngestStaging && self.export_policy != ExportPolicy::Disabled {
            errors.push(StorePolicyValidationError::IngestStagingExportEnabled);
        }

        if let Some(error) = self.capacity.validation_error() {
            errors.push(StorePolicyValidationError::InvalidCapacity { error });
        }

        errors
    }

    fn validate_enclosure_availability(
        &self,
        context: EnclosurePlacementContext,
        errors: &mut Vec<StorePolicyValidationError>,
    ) {
        if self.enclosure_placement == EnclosurePlacement::RequireDistinct
            && (1..=3).contains(&self.copies)
            && self.copies >= 2
            && context.available_enclosure_count < self.copies
        {
            errors.push(
                StorePolicyValidationError::RequiredEnclosureDiversityUnavailable {
                    copies: self.copies,
                    available_enclosure_count: context.available_enclosure_count,
                },
            );
        }
    }

    pub fn is_protected_class(&self) -> bool {
        matches!(
            self.class,
            StoreClass::GeneratedData | StoreClass::CriticalMetadata | StoreClass::ExportBundle
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorePolicyValidationErrors {
    pub errors: Vec<StorePolicyValidationError>,
}

impl StorePolicyValidationErrors {
    fn from_errors(errors: Vec<StorePolicyValidationError>) -> Result<(), Self> {
        if errors.is_empty() {
            Ok(())
        } else {
            Err(Self { errors })
        }
    }
}

impl Display for StorePolicyValidationErrors {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "store policy is invalid: {} validation error(s)",
            self.errors.len()
        )?;
        for error in &self.errors {
            write!(formatter, "; {error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for StorePolicyValidationErrors {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorePolicyValidationError {
    InvalidCopyCount {
        copies: u8,
    },
    DistinctPlacementNeedsMultipleCopies,
    RequiredEnclosureDiversityUnavailable {
        copies: u8,
        available_enclosure_count: u8,
    },
    ProtectedStoreDirectToHdd {
        class: StoreClass,
    },
    ProtectedStoreImmediateDelete {
        class: StoreClass,
    },
    ProtectedStoreMutable {
        class: StoreClass,
    },
    ProtectedStoreMarksRedownloadRequired {
        class: StoreClass,
    },
    IngestStagingExportEnabled,
    InvalidCapacity {
        error: CapacityPolicyValidationError,
    },
}

impl Display for StorePolicyValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCopyCount { copies } => {
                write!(formatter, "copy count must be between 1 and 3, got {copies}")
            }
            Self::DistinctPlacementNeedsMultipleCopies => formatter.write_str(
                "required distinct enclosure placement needs at least two copies",
            ),
            Self::RequiredEnclosureDiversityUnavailable {
                copies,
                available_enclosure_count,
            } => write!(
                formatter,
                "required {copies} distinct enclosure(s), got {available_enclosure_count}"
            ),
            Self::ProtectedStoreDirectToHdd { class } => write!(
                formatter,
                "protected store class {} cannot use direct-to-HDD ingest",
                class.name()
            ),
            Self::ProtectedStoreImmediateDelete { class } => write!(
                formatter,
                "protected store class {} cannot use immediate delete retention",
                class.name()
            ),
            Self::ProtectedStoreMutable { class } => {
                write!(formatter, "protected store class {} must be immutable", class.name())
            }
            Self::ProtectedStoreMarksRedownloadRequired { class } => write!(
                formatter,
                "protected store class {} cannot mark data redownload-required on capacity pressure",
                class.name()
            ),
            Self::IngestStagingExportEnabled => {
                formatter.write_str("ingest staging store export policy must be disabled")
            }
            Self::InvalidCapacity { error } => write!(formatter, "invalid capacity policy: {error}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        evaluate_capacity_admission, AcknowledgementPolicy, CapacityAdmissionError,
        CapacityAdmissionInput, CapacityBehavior, CapacityLedgerError, CapacityPolicy,
        CapacityPressureState, EnclosurePlacement, EnclosurePlacementContext, ExportPolicy,
        IngestMode, LogicalObjectVersionCharge, MutabilityPolicy, PoolPolicyDefaults, RepairPolicy,
        RetentionPolicy, StoreClass, StorePolicy, StorePolicyOverrides, StorePolicyValidationError,
    };

    #[test]
    fn bounded_capacity_reservations_are_transactional_and_respect_backend_reserve() {
        let policy = CapacityPolicy::bounded(100, 20);
        let mut ledger = super::CapacityReservationLedger::new(policy, 50)
            .expect("bounded capacity policy is valid");

        ledger.reserve("upload-a", 25).expect("reservation fits");
        assert_eq!(ledger.reserved_bytes(), 25);
        assert_eq!(
            ledger.reserve("upload-b", 10),
            Err(CapacityLedgerError::InsufficientCapacity { available_bytes: 5 })
        );
        ledger.commit("upload-a").expect("reservation commits");
        assert_eq!(ledger.used_bytes(), 75);
        assert_eq!(ledger.reserved_bytes(), 0);
        assert_eq!(
            ledger.release("missing"),
            Err(CapacityLedgerError::UnknownReservation)
        );
    }

    #[test]
    fn logical_object_versions_each_charge_full_size_independently() {
        let mut ledger = super::CapacityReservationLedger::new(CapacityPolicy::bounded(250, 0), 0)
            .expect("bounded capacity policy is valid");
        let first = LogicalObjectVersionCharge::new(100);
        let second = LogicalObjectVersionCharge::new(100);
        assert_eq!(first.logical_size_bytes(), 100);
        assert_eq!(LogicalObjectVersionCharge::new(0).logical_size_bytes(), 0);

        ledger
            .reserve_object_version("version-a", first)
            .expect("first version fits");
        ledger
            .reserve_object_version("version-b", second)
            .expect("second version fits");
        assert_eq!(ledger.reserved_bytes(), 200);
        ledger.commit("version-a").expect("first version commits");
        ledger.commit("version-b").expect("second version commits");
        assert_eq!(ledger.used_bytes(), 200);
        assert_eq!(ledger.available_bytes(), Some(50));
    }

    #[test]
    fn capacity_policy_rejects_invalid_reserve_and_thresholds() {
        assert!(CapacityPolicy::bounded(10, 10).validation_error().is_some());
        assert!(CapacityPolicy {
            logical_limit_bytes: Some(100),
            backend_reserve_bytes: 1,
            warning_threshold_basis_points: 9_500,
            critical_threshold_basis_points: 9_000,
        }
        .validation_error()
        .is_some());
    }

    #[test]
    fn quota_reduction_marks_over_quota_without_deleting_or_blocking_reads() {
        let mut ledger =
            super::CapacityReservationLedger::new(CapacityPolicy::bounded(1_000, 100), 600)
                .expect("capacity policy is valid");
        assert_eq!(ledger.pressure_state(), CapacityPressureState::Normal);
        ledger
            .update_policy(CapacityPolicy::bounded(500, 100))
            .expect("lower quota remains valid");
        assert_eq!(ledger.used_bytes(), 600);
        assert_eq!(ledger.pressure_state(), CapacityPressureState::OverQuota);
        assert_eq!(
            ledger.reserve("blocked", 1),
            Err(CapacityLedgerError::InsufficientCapacity { available_bytes: 0 })
        );
    }

    #[test]
    fn capacity_pressure_uses_warning_and_critical_thresholds() {
        let mut ledger =
            super::CapacityReservationLedger::new(CapacityPolicy::bounded(1_000, 0), 0)
                .expect("capacity policy is valid");
        ledger
            .reserve("warning", 850)
            .expect("warning reservation fits");
        assert_eq!(ledger.pressure_state(), CapacityPressureState::Warning);
        ledger.commit("warning").expect("warning commits");
        ledger
            .update_policy(CapacityPolicy {
                logical_limit_bytes: Some(1_000),
                backend_reserve_bytes: 0,
                warning_threshold_basis_points: 8_000,
                critical_threshold_basis_points: 8_500,
            })
            .expect("threshold update succeeds");
        assert_eq!(ledger.pressure_state(), CapacityPressureState::Critical);
    }

    #[test]
    fn concurrent_reservations_never_overbook_logical_capacity() {
        use std::sync::{Arc, Mutex};
        use std::thread;

        let ledger = Arc::new(Mutex::new(
            super::CapacityReservationLedger::new(CapacityPolicy::bounded(100, 0), 0)
                .expect("capacity policy is valid"),
        ));
        let workers = (0..8)
            .map(|index| {
                let ledger = Arc::clone(&ledger);
                thread::spawn(move || {
                    let mut ledger = ledger.lock().expect("ledger lock is not poisoned");
                    ledger.reserve(format!("concurrent-{index}"), 30).is_ok()
                })
            })
            .collect::<Vec<_>>();
        let successes = workers
            .into_iter()
            .map(|worker| worker.join().expect("reservation worker joins"))
            .filter(|success| *success)
            .count();
        let ledger = ledger.lock().expect("ledger lock is not poisoned");
        assert_eq!(successes, 3);
        assert_eq!(ledger.reserved_bytes(), 90);
        assert!(ledger.used_bytes() + ledger.reserved_bytes() <= 100);
    }

    #[test]
    fn capacity_ledger_snapshot_restores_usage_and_reservations() {
        let mut ledger =
            super::CapacityReservationLedger::new(CapacityPolicy::bounded(1_000, 25), 100)
                .expect("capacity policy is valid");
        ledger.reserve("in-flight", 200).expect("reservation fits");

        let snapshot = ledger.snapshot();
        let encoded = serde_json::to_vec(&snapshot).expect("snapshot serializes");
        let decoded = serde_json::from_slice(&encoded).expect("snapshot decodes");
        let restored =
            super::CapacityReservationLedger::from_snapshot(decoded).expect("snapshot restores");

        assert_eq!(restored.used_bytes(), 100);
        assert_eq!(restored.reserved_bytes(), 200);
        assert_eq!(restored.reservation_bytes("in-flight"), Some(200));
        assert_eq!(restored.policy(), ledger.policy());
    }

    #[test]
    fn capacity_ledger_snapshot_rejects_unknown_schema_and_overbooking() {
        let mut snapshot =
            super::CapacityReservationLedger::new(CapacityPolicy::bounded(100, 0), 0)
                .expect("capacity policy is valid")
                .snapshot();
        snapshot.schema_version += 1;
        assert_eq!(
            super::CapacityReservationLedger::from_snapshot(snapshot),
            Err(CapacityLedgerError::InvalidSnapshotSchema {
                schema_version: super::CAPACITY_LEDGER_SNAPSHOT_SCHEMA_VERSION + 1,
            })
        );

        let mut overbooked =
            super::CapacityReservationLedger::new(CapacityPolicy::bounded(100, 0), 0)
                .expect("capacity policy is valid")
                .snapshot();
        overbooked.reservations.insert("too-large".to_string(), 101);
        assert!(matches!(
            super::CapacityReservationLedger::from_snapshot(overbooked),
            Err(CapacityLedgerError::InsufficientCapacity { .. })
        ));
    }

    #[test]
    fn unbounded_capacity_has_no_pressure_state_and_large_values_are_safe() {
        let unbounded = super::CapacityReservationLedger::new(CapacityPolicy::default(), 0)
            .expect("legacy policy is valid");
        assert_eq!(unbounded.pressure_state(), CapacityPressureState::Unbounded);

        let ledger = super::CapacityReservationLedger::new(
            CapacityPolicy::bounded(u64::MAX, 0),
            u64::MAX / 2,
        )
        .expect("large policy is valid");
        assert_eq!(ledger.pressure_state(), CapacityPressureState::Normal);
    }

    #[test]
    fn used_bytes_can_be_debited_only_for_existing_accounting() {
        let mut ledger = super::CapacityReservationLedger::new(CapacityPolicy::bounded(100, 0), 40)
            .expect("capacity policy is valid");
        ledger.debit_used_bytes(15).expect("debit fits");
        assert_eq!(ledger.used_bytes(), 25);
        assert_eq!(
            ledger.debit_used_bytes(30),
            Err(CapacityLedgerError::UsedBytesUnderflow {
                used_bytes: 25,
                requested_bytes: 30,
            })
        );
        assert_eq!(ledger.used_bytes(), 25);
    }

    #[test]
    fn capacity_admission_uses_logical_backend_ssd_and_copy_constraints() {
        let policy = CapacityPolicy::bounded(1_000, 100);
        let admission = evaluate_capacity_admission(
            &policy,
            CapacityAdmissionInput {
                requested_bytes: 200,
                copy_count: 2,
                requires_ssd_staging: true,
                used_bytes: 100,
                reserved_bytes: 50,
                backend_free_bytes: 1_000,
                ssd_free_bytes: 500,
            },
        )
        .expect("admission succeeds");
        assert_eq!(admission.logical_available_bytes, Some(750));
        assert_eq!(admission.backend_available_bytes, 900);
        assert_eq!(admission.strictest_available_bytes(), Some(500));

        let error = evaluate_capacity_admission(
            &policy,
            CapacityAdmissionInput {
                requested_bytes: 600,
                copy_count: 1,
                requires_ssd_staging: true,
                used_bytes: 0,
                reserved_bytes: 0,
                backend_free_bytes: 2_000,
                ssd_free_bytes: 500,
            },
        )
        .expect_err("SSD staging rejects admission");
        assert_eq!(
            error,
            CapacityAdmissionError::SsdStaging {
                available_bytes: 500
            }
        );
    }

    #[test]
    fn direct_admission_bypasses_only_ssd_constraint() {
        let admission = evaluate_capacity_admission(
            &CapacityPolicy::bounded(1_000, 100),
            CapacityAdmissionInput {
                requested_bytes: 200,
                copy_count: 1,
                requires_ssd_staging: false,
                used_bytes: 0,
                reserved_bytes: 0,
                backend_free_bytes: 500,
                ssd_free_bytes: 0,
            },
        )
        .expect("direct admission ignores SSD free space");
        assert_eq!(admission.required_ssd_bytes, 0);
        assert_eq!(admission.strictest_available_bytes(), Some(400));

        let error = evaluate_capacity_admission(
            &CapacityPolicy::bounded(100, 0),
            CapacityAdmissionInput {
                requested_bytes: 200,
                copy_count: 1,
                requires_ssd_staging: false,
                used_bytes: 0,
                reserved_bytes: 0,
                backend_free_bytes: 500,
                ssd_free_bytes: 0,
            },
        )
        .expect_err("direct admission still enforces logical quota");
        assert!(matches!(error, CapacityAdmissionError::LogicalQuota { .. }));
    }

    #[test]
    fn store_class_names_are_stable_snake_case() {
        assert_eq!(StoreClass::ReproducibleCache.name(), "reproducible_cache");
        assert_eq!(StoreClass::GeneratedData.name(), "generated_data");
        assert_eq!(StoreClass::CriticalMetadata.name(), "critical_metadata");
        assert_eq!(StoreClass::ExportBundle.name(), "export_bundle");
        assert_eq!(StoreClass::IngestStaging.name(), "ingest_staging");
    }

    #[test]
    fn parses_store_class_from_stable_snake_case_name() {
        assert_eq!(
            "reproducible_cache".parse::<StoreClass>(),
            Ok(StoreClass::ReproducibleCache)
        );
        assert_eq!(
            "generated_data".parse::<StoreClass>(),
            Ok(StoreClass::GeneratedData)
        );
        assert!("generated-data".parse::<StoreClass>().is_err());
    }

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
    fn accepts_valid_public_cache_policy() {
        let policy = StorePolicy::defaults_for(StoreClass::ReproducibleCache);

        assert_eq!(policy.class, StoreClass::ReproducibleCache);
        assert_eq!(policy.ingest_mode, IngestMode::SsdFirst);
        assert_eq!(
            policy.acknowledgement_policy,
            AcknowledgementPolicy::AfterSsdIngest
        );
        assert_eq!(policy.copies, 1);
        assert_eq!(policy.retention_policy, RetentionPolicy::ImmediateDelete);
        assert_eq!(policy.mutability_policy, MutabilityPolicy::Immutable);
        assert_eq!(
            policy.repair_policy,
            RepairPolicy::EvacuateIfCapacityAvailable
        );
        assert_eq!(
            policy.capacity_behavior,
            CapacityBehavior::MarkRedownloadRequired
        );
        assert_eq!(policy.export_policy, ExportPolicy::S3);

        policy.validate().expect("public cache policy is valid");
        policy
            .validate_for_enclosures(EnclosurePlacementContext::new(1))
            .expect("public cache policy ignores enclosure diversity");
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
    fn accepts_valid_generated_data_policy() {
        let policy = StorePolicy::defaults_for(StoreClass::GeneratedData);

        assert_eq!(policy.class, StoreClass::GeneratedData);
        assert_eq!(policy.ingest_mode, IngestMode::SsdFirst);
        assert_eq!(
            policy.acknowledgement_policy,
            AcknowledgementPolicy::AfterHddPlacement
        );
        assert_eq!(policy.copies, 2);
        assert_eq!(
            policy.enclosure_placement,
            EnclosurePlacement::PreferDistinct
        );
        assert_eq!(policy.retention_policy, RetentionPolicy::TombstoneThenGc);
        assert_eq!(policy.mutability_policy, MutabilityPolicy::Immutable);
        assert_eq!(policy.repair_policy, RepairPolicy::RestoreFromCopy);
        assert_eq!(
            policy.capacity_behavior,
            CapacityBehavior::BackpressureByPriority
        );
        assert_eq!(policy.export_policy, ExportPolicy::S3);

        policy.validate().expect("generated data policy is valid");
        policy
            .validate_for_enclosures(EnclosurePlacementContext::new(1))
            .expect("preferred enclosure diversity remains best effort");
        policy
            .validate_for_enclosures(EnclosurePlacementContext::new(2))
            .expect("generated data policy is valid with two enclosures");
    }

    #[test]
    fn critical_metadata_defaults_to_three_copies() {
        let policy = StorePolicy::defaults_for(StoreClass::CriticalMetadata);

        assert_eq!(policy.copies, 3);
        assert_eq!(policy.capacity_behavior, CapacityBehavior::RejectWrites);
    }

    #[test]
    fn accepts_valid_critical_metadata_policy() {
        let policy = StorePolicy::defaults_for(StoreClass::CriticalMetadata);

        assert_eq!(policy.class, StoreClass::CriticalMetadata);
        assert_eq!(policy.ingest_mode, IngestMode::SsdFirst);
        assert_eq!(
            policy.acknowledgement_policy,
            AcknowledgementPolicy::AfterHddPlacement
        );
        assert_eq!(policy.copies, 3);
        assert_eq!(
            policy.enclosure_placement,
            EnclosurePlacement::PreferDistinct
        );
        assert_eq!(policy.retention_policy, RetentionPolicy::TombstoneThenGc);
        assert_eq!(policy.mutability_policy, MutabilityPolicy::Immutable);
        assert_eq!(policy.repair_policy, RepairPolicy::RestoreFromCopy);
        assert_eq!(policy.capacity_behavior, CapacityBehavior::RejectWrites);
        assert_eq!(policy.export_policy, ExportPolicy::S3);

        policy
            .validate()
            .expect("critical metadata policy is valid");
        policy
            .validate_for_enclosures(EnclosurePlacementContext::new(1))
            .expect("preferred enclosure diversity remains best effort");

        let mut required = policy;
        required.enclosure_placement = EnclosurePlacement::RequireDistinct;
        required
            .validate_for_enclosures(EnclosurePlacementContext::new(3))
            .expect("required enclosure diversity is valid with three enclosures");
    }

    #[test]
    fn round_trips_store_policy() {
        let policy = StorePolicy::defaults_for(StoreClass::GeneratedData);

        let encoded = serde_json::to_string(&policy).expect("policy serializes");
        let decoded: StorePolicy = serde_json::from_str(&encoded).expect("policy deserializes");

        assert_eq!(decoded, policy);
    }

    #[test]
    fn built_in_defaults_cover_all_store_classes() {
        let defaults = StorePolicy::built_in_defaults();
        let classes: Vec<StoreClass> = defaults.iter().map(|policy| policy.class).collect();

        assert_eq!(classes, StoreClass::ALL);
        assert_eq!(
            defaults
                .iter()
                .find(|policy| policy.class == StoreClass::ReproducibleCache)
                .expect("reproducible cache default")
                .copies,
            1
        );
        assert_eq!(
            defaults
                .iter()
                .find(|policy| policy.class == StoreClass::CriticalMetadata)
                .expect("critical metadata default")
                .copies,
            3
        );
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
            acknowledgement_policy: Some(AcknowledgementPolicy::AfterSsdIngest),
            capacity_behavior: Some(CapacityBehavior::RejectWrites),
            export_policy: Some(ExportPolicy::ReadOnlyFileExport),
            ..StorePolicyOverrides::default()
        };

        let policy = defaults.apply_overrides(StoreClass::CriticalMetadata, overrides);

        assert_eq!(policy.class, StoreClass::CriticalMetadata);
        assert_eq!(policy.copies, 3);
        assert_eq!(
            policy.acknowledgement_policy,
            AcknowledgementPolicy::AfterSsdIngest
        );
        assert_eq!(policy.capacity_behavior, CapacityBehavior::RejectWrites);
        assert_eq!(policy.export_policy, ExportPolicy::ReadOnlyFileExport);
        assert_eq!(policy.repair_policy, RepairPolicy::RestoreFromCopy);
    }

    #[test]
    fn accepts_builtin_store_policy_defaults() {
        for policy in StorePolicy::built_in_defaults() {
            policy.validate().expect("built-in default is valid");
        }
    }

    #[test]
    fn rejects_invalid_copy_count_and_distinct_single_copy() {
        let mut policy = StorePolicy::defaults_for(StoreClass::ReproducibleCache);
        policy.copies = 0;
        policy.enclosure_placement = EnclosurePlacement::RequireDistinct;

        let err = policy.validate().expect_err("policy should fail");

        assert_eq!(
            err.errors,
            vec![
                StorePolicyValidationError::InvalidCopyCount { copies: 0 },
                StorePolicyValidationError::DistinctPlacementNeedsMultipleCopies
            ]
        );
    }

    #[test]
    fn rejects_copy_count_above_supported_range() {
        let mut policy = StorePolicy::defaults_for(StoreClass::GeneratedData);
        policy.copies = 4;

        let err = policy.validate().expect_err("policy should fail");

        assert_eq!(
            err.errors,
            vec![StorePolicyValidationError::InvalidCopyCount { copies: 4 }]
        );
    }

    #[test]
    fn rejects_protected_policy_with_unsafe_semantics() {
        for class in [
            StoreClass::GeneratedData,
            StoreClass::CriticalMetadata,
            StoreClass::ExportBundle,
        ] {
            let mut policy = StorePolicy::defaults_for(class);
            policy.ingest_mode = IngestMode::DirectToHdd;
            policy.retention_policy = RetentionPolicy::ImmediateDelete;
            policy.mutability_policy = MutabilityPolicy::Mutable;
            policy.capacity_behavior = CapacityBehavior::MarkRedownloadRequired;

            let err = policy.validate().expect_err("policy should fail");

            assert_eq!(
                err.errors,
                vec![
                    StorePolicyValidationError::ProtectedStoreDirectToHdd { class },
                    StorePolicyValidationError::ProtectedStoreImmediateDelete { class },
                    StorePolicyValidationError::ProtectedStoreMutable { class },
                    StorePolicyValidationError::ProtectedStoreMarksRedownloadRequired { class }
                ]
            );
        }
    }

    #[test]
    fn rejects_export_enabled_ingest_staging_policy() {
        let mut policy = StorePolicy::defaults_for(StoreClass::IngestStaging);
        policy.export_policy = ExportPolicy::S3;

        let err = policy.validate().expect_err("policy should fail");

        assert_eq!(
            err.errors,
            vec![StorePolicyValidationError::IngestStagingExportEnabled]
        );
    }

    #[test]
    fn rejects_required_distinct_enclosures_when_unavailable() {
        let mut policy = StorePolicy::defaults_for(StoreClass::GeneratedData);
        policy.enclosure_placement = EnclosurePlacement::RequireDistinct;

        let err = policy
            .validate_for_enclosures(EnclosurePlacementContext::new(1))
            .expect_err("required enclosure diversity should fail");

        assert_eq!(
            err.errors,
            vec![
                StorePolicyValidationError::RequiredEnclosureDiversityUnavailable {
                    copies: 2,
                    available_enclosure_count: 1
                }
            ]
        );
    }

    #[test]
    fn accepts_required_distinct_enclosures_when_available() {
        let mut policy = StorePolicy::defaults_for(StoreClass::CriticalMetadata);
        policy.enclosure_placement = EnclosurePlacement::RequireDistinct;

        policy
            .validate_for_enclosures(EnclosurePlacementContext::new(3))
            .expect("required enclosure diversity should pass");
    }

    #[test]
    fn accepts_preferred_distinct_enclosures_when_unavailable() {
        let policy = StorePolicy::defaults_for(StoreClass::GeneratedData);

        policy
            .validate_for_enclosures(EnclosurePlacementContext::new(1))
            .expect("preferred enclosure diversity should remain best effort");
    }
}
