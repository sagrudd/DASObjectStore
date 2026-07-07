use super::telemetry::DaemonIngestPressure;
use crate::api::health::DaemonSsdPressure;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonSourceReadBackpressurePolicy {
    pub throttle_error_rate_per_minute: u32,
    pub block_error_rate_per_minute: u32,
}

impl Default for DaemonSourceReadBackpressurePolicy {
    fn default() -> Self {
        Self {
            throttle_error_rate_per_minute: 3,
            block_error_rate_per_minute: 10,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonSourceReadPriority {
    BestEffort,
    Normal,
    SourceToSsdFirst,
    Recovery,
}

impl Default for DaemonSourceReadPriority {
    fn default() -> Self {
        Self::SourceToSsdFirst
    }
}

impl DaemonSourceReadPriority {
    fn tolerates_elevated_downstream_pressure(self) -> bool {
        matches!(self, Self::SourceToSsdFirst | Self::Recovery)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonSourceReadBackpressureAction {
    Run,
    Throttle,
    Block,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonSourceReadBackpressureReason {
    None,
    SsdPressure,
    RamPressure,
    HddBacklog,
    VerificationBacklog,
    ErrorRate,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonSourceReadBackpressureDecision {
    pub action: DaemonSourceReadBackpressureAction,
    pub reason: DaemonSourceReadBackpressureReason,
    pub priority: DaemonSourceReadPriority,
}

impl DaemonSourceReadBackpressureDecision {
    pub fn should_run(&self) -> bool {
        self.action == DaemonSourceReadBackpressureAction::Run
    }

    pub fn should_throttle(&self) -> bool {
        self.action == DaemonSourceReadBackpressureAction::Throttle
    }

    pub fn should_block(&self) -> bool {
        self.action == DaemonSourceReadBackpressureAction::Block
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestErrorRate {
    pub errors: u32,
    pub window_seconds: u32,
}

impl DaemonIngestErrorRate {
    pub fn errors_per_minute(&self) -> u32 {
        if self.window_seconds == 0 {
            return if self.errors == 0 { 0 } else { u32::MAX };
        }

        u64::from(self.errors)
            .saturating_mul(60)
            .div_ceil(u64::from(self.window_seconds))
            .min(u64::from(u32::MAX)) as u32
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonSourceReadBackpressureInput {
    pub priority: DaemonSourceReadPriority,
    pub ssd_pressure: DaemonSsdPressure,
    pub ram_pressure: DaemonIngestPressure,
    pub hdd_backlog: DaemonIngestPressure,
    pub verification_backlog: DaemonIngestPressure,
    pub error_rate: DaemonIngestErrorRate,
}

impl Default for DaemonSourceReadBackpressureInput {
    fn default() -> Self {
        Self {
            priority: DaemonSourceReadPriority::default(),
            ssd_pressure: DaemonSsdPressure::AcceptingWrites,
            ram_pressure: DaemonIngestPressure::Normal,
            hdd_backlog: DaemonIngestPressure::Normal,
            verification_backlog: DaemonIngestPressure::Normal,
            error_rate: DaemonIngestErrorRate::default(),
        }
    }
}

impl DaemonSourceReadBackpressureInput {
    pub fn classify(
        &self,
        policy: &DaemonSourceReadBackpressurePolicy,
    ) -> DaemonSourceReadBackpressureDecision {
        let error_rate = self.error_rate.errors_per_minute();

        if self.ssd_pressure == DaemonSsdPressure::Critical {
            return self.decision(
                DaemonSourceReadBackpressureAction::Block,
                DaemonSourceReadBackpressureReason::SsdPressure,
            );
        }
        if self.ram_pressure == DaemonIngestPressure::Critical {
            return self.decision(
                DaemonSourceReadBackpressureAction::Block,
                DaemonSourceReadBackpressureReason::RamPressure,
            );
        }
        if self.hdd_backlog == DaemonIngestPressure::Critical {
            return self.decision(
                DaemonSourceReadBackpressureAction::Block,
                DaemonSourceReadBackpressureReason::HddBacklog,
            );
        }
        if self.verification_backlog == DaemonIngestPressure::Critical {
            return self.decision(
                DaemonSourceReadBackpressureAction::Block,
                DaemonSourceReadBackpressureReason::VerificationBacklog,
            );
        }
        if error_rate >= policy.block_error_rate_per_minute {
            return self.decision(
                DaemonSourceReadBackpressureAction::Block,
                DaemonSourceReadBackpressureReason::ErrorRate,
            );
        }

        if self.ssd_pressure == DaemonSsdPressure::High {
            return self.decision(
                DaemonSourceReadBackpressureAction::Throttle,
                DaemonSourceReadBackpressureReason::SsdPressure,
            );
        }
        if self.ram_pressure.severity() >= DaemonIngestPressure::High.severity() {
            return self.decision(
                DaemonSourceReadBackpressureAction::Throttle,
                DaemonSourceReadBackpressureReason::RamPressure,
            );
        }
        if self.hdd_backlog.severity() >= DaemonIngestPressure::High.severity()
            || (self.hdd_backlog == DaemonIngestPressure::Elevated
                && !self.priority.tolerates_elevated_downstream_pressure())
        {
            return self.decision(
                DaemonSourceReadBackpressureAction::Throttle,
                DaemonSourceReadBackpressureReason::HddBacklog,
            );
        }
        if self.verification_backlog.severity() >= DaemonIngestPressure::High.severity()
            || (self.verification_backlog == DaemonIngestPressure::Elevated
                && !self.priority.tolerates_elevated_downstream_pressure())
        {
            return self.decision(
                DaemonSourceReadBackpressureAction::Throttle,
                DaemonSourceReadBackpressureReason::VerificationBacklog,
            );
        }
        if error_rate >= policy.throttle_error_rate_per_minute {
            return self.decision(
                DaemonSourceReadBackpressureAction::Throttle,
                DaemonSourceReadBackpressureReason::ErrorRate,
            );
        }

        self.decision(
            DaemonSourceReadBackpressureAction::Run,
            DaemonSourceReadBackpressureReason::None,
        )
    }

    fn decision(
        &self,
        action: DaemonSourceReadBackpressureAction,
        reason: DaemonSourceReadBackpressureReason,
    ) -> DaemonSourceReadBackpressureDecision {
        DaemonSourceReadBackpressureDecision {
            action,
            reason,
            priority: self.priority,
        }
    }
}
