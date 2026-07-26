//! Narrow privileged host boundary for managed compute workspace branches.
//!
//! The broker accepts only typed, versioned requests over a local Unix socket.
//! Disk roots come exclusively from root-owned configuration; callers submit
//! opaque disk and branch identities, never arbitrary host paths.

mod broker;
mod checkpoint;
mod config;
mod marker;
mod materialize;
mod protocol;
mod quota;

pub use broker::{execute_request, BrokerError};
pub use config::{BrokerConfig, ManagedDiskRoot, ManagedNfsClient};
pub use marker::{BranchMarker, MARKER_FILE, MARKER_SCHEMA_VERSION};
pub use protocol::{
    AggregateInspection, AggregatePlan, AggregateRecoveryState, BranchInspection, BranchPlan,
    BrokerRequest, BrokerResponse, CheckpointInventory, CheckpointMember, CheckpointPlan,
    MaterializationInspection, MaterializationPlan, MaterializationRecoveryState, NfsAccessMode,
    NfsExportInspection, NfsExportPlan, NfsExportRecoveryState, RecoveryState,
    WorkspaceHostOperation, PROTOCOL_VERSION,
};

use std::io::{BufRead, BufReader, Write};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::path::Path;

/// Submit exactly one bounded request to the local host broker.
#[cfg(unix)]
pub fn request_broker(
    socket_path: &Path,
    request: &BrokerRequest,
) -> Result<BrokerResponse, BrokerError> {
    let mut stream = UnixStream::connect(socket_path)
        .map_err(|error| BrokerError::Io("connect broker socket", error))?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(30)))
        .map_err(|error| BrokerError::Io("set broker read timeout", error))?;
    serde_json::to_writer(&mut stream, request)
        .map_err(|error| BrokerError::Protocol(error.to_string()))?;
    stream
        .write_all(b"\n")
        .map_err(|error| BrokerError::Io("write broker request", error))?;
    let mut response = String::new();
    BufReader::new(stream)
        .read_line(&mut response)
        .map_err(|error| BrokerError::Io("read broker response", error))?;
    serde_json::from_str(&response).map_err(|error| BrokerError::Protocol(error.to_string()))
}
