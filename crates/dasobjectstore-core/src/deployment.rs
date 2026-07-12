//! Compatibility-sensitive deployment profile and host-mode vocabulary.
//!
//! Profiles describe the physical/backend boundary of an ObjectStore. Host
//! mode describes who owns the service and authentication authority; the two
//! axes are intentionally independent so a folder can be user-owned or
//! system-managed without changing its storage semantics.

use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};
use std::str::FromStr;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentProfile {
    Folder,
    Drive,
    Appliance,
}

impl DeploymentProfile {
    pub const ALL: [Self; 3] = [Self::Folder, Self::Drive, Self::Appliance];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Folder => "folder",
            Self::Drive => "drive",
            Self::Appliance => "appliance",
        }
    }
}

impl Display for DeploymentProfile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

impl FromStr for DeploymentProfile {
    type Err = DeploymentProfileParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "folder" => Ok(Self::Folder),
            "drive" => Ok(Self::Drive),
            "appliance" => Ok(Self::Appliance),
            _ => Err(DeploymentProfileParseError {
                value: value.to_string(),
            }),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeploymentProfileParseError {
    value: String,
}

impl Display for DeploymentProfileParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "unknown deployment profile `{}`", self.value)
    }
}

impl std::error::Error for DeploymentProfileParseError {}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostMode {
    PerUser,
    System,
    Integrated,
}

impl HostMode {
    pub const ALL: [Self; 3] = [Self::PerUser, Self::System, Self::Integrated];

    pub const fn name(self) -> &'static str {
        match self {
            Self::PerUser => "per_user",
            Self::System => "system",
            Self::Integrated => "integrated",
        }
    }
}

impl Display for HostMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

impl FromStr for HostMode {
    type Err = HostModeParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "per_user" => Ok(Self::PerUser),
            "system" => Ok(Self::System),
            "integrated" => Ok(Self::Integrated),
            _ => Err(HostModeParseError {
                value: value.to_string(),
            }),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostModeParseError {
    value: String,
}

impl Display for HostModeParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "unknown host mode `{}`", self.value)
    }
}

impl std::error::Error for HostModeParseError {}

#[cfg(test)]
mod tests {
    use super::{DeploymentProfile, HostMode};
    use std::str::FromStr;

    #[test]
    fn deployment_profiles_have_stable_wire_names_and_round_trip() {
        assert_eq!(
            DeploymentProfile::ALL.map(DeploymentProfile::name),
            ["folder", "drive", "appliance"]
        );
        for profile in DeploymentProfile::ALL {
            let encoded = serde_json::to_string(&profile).expect("profile serializes");
            assert_eq!(encoded.trim_matches('"'), profile.name());
            assert_eq!(DeploymentProfile::from_str(profile.name()), Ok(profile));
        }
    }

    #[test]
    fn host_modes_are_orthogonal_and_have_stable_wire_names() {
        assert_eq!(
            HostMode::ALL.map(HostMode::name),
            ["per_user", "system", "integrated"]
        );
        for mode in HostMode::ALL {
            let encoded = serde_json::to_string(&mode).expect("host mode serializes");
            assert_eq!(encoded.trim_matches('"'), mode.name());
            assert_eq!(HostMode::from_str(mode.name()), Ok(mode));
        }
        assert_ne!(DeploymentProfile::Folder, DeploymentProfile::Appliance);
    }

    #[test]
    fn unknown_names_are_rejected() {
        assert!(DeploymentProfile::from_str("ssd").is_err());
        assert!(HostMode::from_str("global").is_err());
    }
}
