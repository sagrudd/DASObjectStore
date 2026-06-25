//! Disk health scoring model.

use crate::lifecycle::HealthState;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HealthSignalWeights {
    pub smart_warning: u8,
    pub io_error: u8,
    pub checksum_failure: u8,
    pub usb_reset: u8,
    pub high_temperature: u8,
    pub benchmark_drift: u8,
    pub low_user_trust: u8,
}

impl Default for HealthSignalWeights {
    fn default() -> Self {
        Self {
            smart_warning: 25,
            io_error: 20,
            checksum_failure: 35,
            usb_reset: 10,
            high_temperature: 15,
            benchmark_drift: 15,
            low_user_trust: 30,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum UserTrust {
    Trusted,
    Normal,
    Low,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HealthSignals {
    pub smart_warnings: u16,
    pub io_errors: u16,
    pub checksum_failures: u16,
    pub usb_resets: u16,
    pub temperature_celsius: Option<u8>,
    pub benchmark_drift_percent: Option<u8>,
    pub user_trust: UserTrust,
}

impl Default for HealthSignals {
    fn default() -> Self {
        Self {
            smart_warnings: 0,
            io_errors: 0,
            checksum_failures: 0,
            usb_resets: 0,
            temperature_celsius: None,
            benchmark_drift_percent: None,
            user_trust: UserTrust::Normal,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HealthScore {
    pub value: u8,
    pub state: HealthState,
}

impl HealthScore {
    pub fn from_signals(signals: &HealthSignals) -> Self {
        Self::from_weighted_signals(signals, HealthSignalWeights::default())
    }

    pub fn from_weighted_signals(signals: &HealthSignals, weights: HealthSignalWeights) -> Self {
        let penalty = signal_penalty(signals, weights);
        let value = 100_u8.saturating_sub(penalty);

        Self {
            value,
            state: state_for_score(value),
        }
    }
}

pub fn health_state_score(health_state: HealthState) -> u8 {
    match health_state {
        HealthState::Healthy => 100,
        HealthState::Watch => 70,
        HealthState::Suspect => 20,
        HealthState::Draining | HealthState::Retired | HealthState::Failed => 0,
    }
}

fn state_for_score(value: u8) -> HealthState {
    match value {
        90..=u8::MAX => HealthState::Healthy,
        60..=89 => HealthState::Watch,
        1..=59 => HealthState::Suspect,
        0 => HealthState::Failed,
    }
}

fn signal_penalty(signals: &HealthSignals, weights: HealthSignalWeights) -> u8 {
    let mut penalty = 0_u16;

    penalty += bounded_count_penalty(signals.smart_warnings, weights.smart_warning);
    penalty += bounded_count_penalty(signals.io_errors, weights.io_error);
    penalty += bounded_count_penalty(signals.checksum_failures, weights.checksum_failure);
    penalty += bounded_count_penalty(signals.usb_resets, weights.usb_reset);
    penalty += temperature_penalty(signals.temperature_celsius, weights.high_temperature);
    penalty += drift_penalty(signals.benchmark_drift_percent, weights.benchmark_drift);
    penalty += user_trust_penalty(signals.user_trust, weights.low_user_trust);

    penalty.min(100) as u8
}

fn bounded_count_penalty(count: u16, weight: u8) -> u16 {
    count.min(3) * weight as u16
}

fn temperature_penalty(temperature_celsius: Option<u8>, weight: u8) -> u16 {
    match temperature_celsius {
        Some(temperature) if temperature >= 55 => weight as u16,
        _ => 0,
    }
}

fn drift_penalty(benchmark_drift_percent: Option<u8>, weight: u8) -> u16 {
    match benchmark_drift_percent {
        Some(drift) if drift >= 25 => weight as u16,
        _ => 0,
    }
}

fn user_trust_penalty(user_trust: UserTrust, weight: u8) -> u16 {
    match user_trust {
        UserTrust::Trusted | UserTrust::Normal => 0,
        UserTrust::Low => weight as u16,
    }
}

#[cfg(test)]
mod tests {
    use super::{health_state_score, HealthScore, HealthSignalWeights, HealthSignals, UserTrust};
    use crate::lifecycle::HealthState;

    #[test]
    fn healthy_disk_scores_full_value_without_signals() {
        let score = HealthScore::from_signals(&HealthSignals::default());

        assert_eq!(score.value, 100);
        assert_eq!(score.state, HealthState::Healthy);
    }

    #[test]
    fn warning_signals_degrade_disk_to_watch() {
        let signals = HealthSignals {
            smart_warnings: 1,
            usb_resets: 1,
            ..HealthSignals::default()
        };

        let score = HealthScore::from_signals(&signals);

        assert_eq!(score.value, 65);
        assert_eq!(score.state, HealthState::Watch);
    }

    #[test]
    fn severe_signals_degrade_disk_to_failed() {
        let signals = HealthSignals {
            smart_warnings: 1,
            io_errors: 1,
            checksum_failures: 1,
            user_trust: UserTrust::Low,
            ..HealthSignals::default()
        };

        let score = HealthScore::from_signals(&signals);

        assert_eq!(score.value, 0);
        assert_eq!(score.state, HealthState::Failed);
    }

    #[test]
    fn count_penalties_are_capped_per_signal_type() {
        let signals = HealthSignals {
            usb_resets: 10,
            ..HealthSignals::default()
        };

        let score = HealthScore::from_signals(&signals);

        assert_eq!(score.value, 70);
        assert_eq!(score.state, HealthState::Watch);
    }

    #[test]
    fn temperature_and_drift_penalties_use_thresholds() {
        let signals = HealthSignals {
            temperature_celsius: Some(55),
            benchmark_drift_percent: Some(25),
            ..HealthSignals::default()
        };

        let score = HealthScore::from_signals(&signals);

        assert_eq!(score.value, 70);
        assert_eq!(score.state, HealthState::Watch);
    }

    #[test]
    fn custom_weights_allow_policy_tuning() {
        let signals = HealthSignals {
            io_errors: 1,
            ..HealthSignals::default()
        };
        let weights = HealthSignalWeights {
            io_error: 60,
            ..HealthSignalWeights::default()
        };

        let score = HealthScore::from_weighted_signals(&signals, weights);

        assert_eq!(score.value, 40);
        assert_eq!(score.state, HealthState::Suspect);
    }

    #[test]
    fn exposes_state_score_for_placement() {
        assert_eq!(health_state_score(HealthState::Healthy), 100);
        assert_eq!(health_state_score(HealthState::Watch), 70);
        assert_eq!(health_state_score(HealthState::Suspect), 20);
        assert_eq!(health_state_score(HealthState::Draining), 0);
        assert_eq!(health_state_score(HealthState::Retired), 0);
        assert_eq!(health_state_score(HealthState::Failed), 0);
    }
}
