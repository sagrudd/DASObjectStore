//! Fail-closed compatibility types for handlers being removed from standalone
//! composition. They never inspect the OS and never provide authentication.

use serde::{Deserialize, Serialize};
use std::{fmt, io};

pub const SUDO_ADMIN_GROUPS: [&str; 0] = [];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LocalUserMetadata {
    pub username: String,
    pub groups: Vec<String>,
    pub sudo_administrator: bool,
}

impl LocalUserMetadata {
    pub fn from_username_and_groups(username: impl Into<String>, groups: Vec<String>) -> Self {
        Self {
            username: username.into(),
            groups,
            sudo_administrator: false,
        }
    }
}

#[derive(Debug)]
pub enum LocalUserDiscoveryError {
    MissingUsername,
    Io {
        path: &'static str,
        source: io::Error,
    },
}

impl fmt::Display for LocalUserDiscoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "appliance-local human authority is removed")
    }
}

impl std::error::Error for LocalUserDiscoveryError {}

pub fn discover_local_user(_username: &str) -> Result<LocalUserMetadata, LocalUserDiscoveryError> {
    Err(LocalUserDiscoveryError::MissingUsername)
}

pub fn discover_current_local_user() -> Result<LocalUserMetadata, LocalUserDiscoveryError> {
    Err(LocalUserDiscoveryError::MissingUsername)
}

pub fn local_user_metadata_from_unix_account_files(
    _username: &str,
) -> Result<LocalUserMetadata, LocalUserDiscoveryError> {
    Err(LocalUserDiscoveryError::MissingUsername)
}
