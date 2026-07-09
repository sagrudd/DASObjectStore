//! Shared ingress-origin classification and landing policy.

use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IngressOrigin {
    #[default]
    LocalServer,
    RemoteS3,
    WebUpload,
    Synoptikon,
    Mneion,
}

impl IngressOrigin {
    pub fn landing_mode(self) -> IngressLandingMode {
        match self {
            Self::LocalServer => IngressLandingMode::DirectToHddWhenPolicyAllows,
            Self::RemoteS3 | Self::WebUpload | Self::Synoptikon | Self::Mneion => {
                IngressLandingMode::SsdFirst
            }
        }
    }

    pub fn requires_ssd_staging(self) -> bool {
        self.landing_mode() == IngressLandingMode::SsdFirst
    }
}

impl Display for IngressOrigin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::LocalServer => "local_server",
            Self::RemoteS3 => "remote_s3",
            Self::WebUpload => "web_upload",
            Self::Synoptikon => "synoptikon",
            Self::Mneion => "mneion",
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IngressLandingMode {
    SsdFirst,
    DirectToHddWhenPolicyAllows,
}

impl Display for IngressLandingMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::SsdFirst => "ssd_first",
            Self::DirectToHddWhenPolicyAllows => "direct_to_hdd_when_policy_allows",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{IngressLandingMode, IngressOrigin};

    #[test]
    fn maps_origins_to_deterministic_landing_modes() {
        assert_eq!(
            IngressOrigin::LocalServer.landing_mode(),
            IngressLandingMode::DirectToHddWhenPolicyAllows
        );

        for origin in [
            IngressOrigin::RemoteS3,
            IngressOrigin::WebUpload,
            IngressOrigin::Synoptikon,
            IngressOrigin::Mneion,
        ] {
            assert_eq!(origin.landing_mode(), IngressLandingMode::SsdFirst);
            assert!(origin.requires_ssd_staging());
        }

        assert!(!IngressOrigin::LocalServer.requires_ssd_staging());
    }

    #[test]
    fn uses_stable_snake_case_wire_names() {
        assert_eq!(
            serde_json::to_value(IngressOrigin::RemoteS3).expect("origin serializes"),
            serde_json::json!("remote_s3")
        );
        assert_eq!(
            serde_json::to_value(IngressOrigin::WebUpload).expect("origin serializes"),
            serde_json::json!("web_upload")
        );
        assert_eq!(
            serde_json::to_value(IngressLandingMode::SsdFirst).expect("mode serializes"),
            serde_json::json!("ssd_first")
        );
    }
}
