use crate::config::validate_identity;
use crate::marker::sync_directory;
use crate::quota::{apply_project_quota, verify_project_quota};
use crate::{
    AggregateInspection, AggregatePlan, AggregateRecoveryState, BranchInspection, BranchMarker,
    BranchPlan, BrokerConfig, BrokerRequest, BrokerResponse, NfsAccessMode, NfsExportInspection,
    NfsExportPlan, NfsExportRecoveryState, RecoveryState, WorkspaceHostOperation, PROTOCOL_VERSION,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

const AGGREGATE_MARKER_FILE: &str = ".dasobjectstore-aggregate.json";
const AGGREGATE_MARKER_SCHEMA: &str = "dasobjectstore.workspace_aggregate.v1";
const EXPORTS_DIRECTORY: &str = "/etc/exports.d";

#[derive(Debug)]
pub enum BrokerError {
    Io(&'static str, std::io::Error),
    Protocol(String),
    InvalidRequest(String),
    UnsafeConfig(String),
    UnsafeEntry(String),
    MarkerConflict(String),
    Quota(String),
    Unsupported(String),
}

impl fmt::Display for BrokerError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(context, error) => write!(output, "{context}: {error}"),
            Self::Protocol(message) => write!(output, "protocol error: {message}"),
            Self::InvalidRequest(message) => write!(output, "invalid request: {message}"),
            Self::UnsafeConfig(message) => write!(output, "unsafe configuration: {message}"),
            Self::UnsafeEntry(message) => write!(output, "unsafe filesystem entry: {message}"),
            Self::MarkerConflict(message) => write!(output, "marker conflict: {message}"),
            Self::Quota(message) => write!(output, "project quota error: {message}"),
            Self::Unsupported(message) => write!(output, "unsupported operation: {message}"),
        }
    }
}

impl std::error::Error for BrokerError {}

pub fn execute_request(
    config: &BrokerConfig,
    request: &BrokerRequest,
) -> Result<BrokerResponse, BrokerError> {
    validate_request(config, request)?;
    let (branches, aggregate, export, materialization, checkpoint, promotion) =
        match &request.operation {
            WorkspaceHostOperation::Provision { branches } => (
                provision_all(config, &request.workspace_id, branches)?,
                None,
                None,
                None,
                None,
                None,
            ),
            WorkspaceHostOperation::Inspect { branches } => (
                inspect_all(
                    config,
                    &request.workspace_id,
                    branches,
                    cfg!(target_os = "linux"),
                )?,
                None,
                None,
                None,
                None,
                None,
            ),
            WorkspaceHostOperation::Rollback { branches } => (
                rollback_all(config, &request.workspace_id, branches)?,
                None,
                None,
                None,
                None,
                None,
            ),
            WorkspaceHostOperation::Cleanup { branches } => (
                cleanup_all(config, &request.workspace_id, branches)?,
                None,
                None,
                None,
                None,
                None,
            ),
            WorkspaceHostOperation::MountAggregate { aggregate } => (
                Vec::new(),
                Some(mount_aggregate(config, &request.workspace_id, aggregate)?),
                None,
                None,
                None,
                None,
            ),
            WorkspaceHostOperation::InspectAggregate { aggregate } => (
                Vec::new(),
                Some(inspect_aggregate(config, &request.workspace_id, aggregate)?),
                None,
                None,
                None,
                None,
            ),
            WorkspaceHostOperation::UnmountAggregate { aggregate } => (
                Vec::new(),
                Some(unmount_aggregate(config, &request.workspace_id, aggregate)?),
                None,
                None,
                None,
                None,
            ),
            WorkspaceHostOperation::AttachNfs { export } => (
                Vec::new(),
                None,
                Some(attach_nfs(config, &request.workspace_id, export)?),
                None,
                None,
                None,
            ),
            WorkspaceHostOperation::InspectNfs { export } => (
                Vec::new(),
                None,
                Some(inspect_nfs(config, &request.workspace_id, export)?),
                None,
                None,
                None,
            ),
            WorkspaceHostOperation::DetachNfs { export } => (
                Vec::new(),
                None,
                Some(detach_nfs(config, &request.workspace_id, export)?),
                None,
                None,
                None,
            ),
            WorkspaceHostOperation::MaterializeInspect { materialization } => (
                Vec::new(),
                None,
                None,
                Some(crate::materialize::inspect(
                    config,
                    &request.workspace_id,
                    materialization,
                )?),
                None,
                None,
            ),
            WorkspaceHostOperation::MaterializeStep { materialization } => (
                Vec::new(),
                None,
                None,
                Some(crate::materialize::copy_step(
                    config,
                    &request.workspace_id,
                    materialization,
                )?),
                None,
                None,
            ),
            WorkspaceHostOperation::CheckpointInventory { checkpoint } => (
                Vec::new(),
                None,
                None,
                None,
                Some(crate::checkpoint::inventory(
                    config,
                    &request.workspace_id,
                    checkpoint,
                )?),
                None,
            ),
            WorkspaceHostOperation::PromotionInspect { promotion } => (
                Vec::new(),
                None,
                None,
                None,
                None,
                Some(crate::promotion::inspect(
                    config,
                    &request.workspace_id,
                    promotion,
                )?),
            ),
            WorkspaceHostOperation::PromotionStep { promotion } => (
                Vec::new(),
                None,
                None,
                None,
                None,
                Some(crate::promotion::copy_step(
                    config,
                    &request.workspace_id,
                    promotion,
                )?),
            ),
        };
    Ok(BrokerResponse {
        protocol_version: PROTOCOL_VERSION,
        request_id: request.request_id.clone(),
        workspace_id: request.workspace_id.clone(),
        ok: true,
        error_code: None,
        error_message: None,
        branches,
        aggregate,
        export,
        materialization,
        checkpoint,
        promotion,
    })
}

fn validate_request(config: &BrokerConfig, request: &BrokerRequest) -> Result<(), BrokerError> {
    if request.protocol_version != PROTOCOL_VERSION {
        return Err(BrokerError::InvalidRequest(
            "unsupported protocol version".to_string(),
        ));
    }
    validate_identity("request_id", &request.request_id)?;
    validate_identity("workspace_id", &request.workspace_id)?;
    let branches = match &request.operation {
        WorkspaceHostOperation::Provision { branches }
        | WorkspaceHostOperation::Inspect { branches }
        | WorkspaceHostOperation::Rollback { branches }
        | WorkspaceHostOperation::Cleanup { branches } => branches,
        WorkspaceHostOperation::MountAggregate { aggregate }
        | WorkspaceHostOperation::InspectAggregate { aggregate }
        | WorkspaceHostOperation::UnmountAggregate { aggregate } => {
            validate_identity("mount_identity", &aggregate.mount_identity)?;
            if aggregate.mount_identity != request.workspace_id {
                return Err(BrokerError::InvalidRequest(
                    "mount identity must equal workspace identity".to_string(),
                ));
            }
            &aggregate.branches
        }
        WorkspaceHostOperation::AttachNfs { export }
        | WorkspaceHostOperation::InspectNfs { export }
        | WorkspaceHostOperation::DetachNfs { export } => {
            validate_identity("mount_identity", &export.mount_identity)?;
            validate_identity("client_id", &export.client_id)?;
            if export.mount_identity != request.workspace_id {
                return Err(BrokerError::InvalidRequest(
                    "NFS mount identity must equal workspace identity".to_string(),
                ));
            }
            if !config.nfs_clients.contains_key(&export.client_id) {
                return Err(BrokerError::InvalidRequest(format!(
                    "NFS client {} is not registered",
                    export.client_id
                )));
            }
            return Ok(());
        }
        WorkspaceHostOperation::MaterializeInspect { materialization }
        | WorkspaceHostOperation::MaterializeStep { materialization } => {
            crate::materialize::validate_plan(materialization)?;
            return Ok(());
        }
        WorkspaceHostOperation::CheckpointInventory { checkpoint } => {
            crate::checkpoint::validate_plan(checkpoint)?;
            return Ok(());
        }
        WorkspaceHostOperation::PromotionInspect { promotion }
        | WorkspaceHostOperation::PromotionStep { promotion } => {
            crate::promotion::validate_plan(promotion)?;
            return Ok(());
        }
    };
    if branches.is_empty() || branches.len() > 256 {
        return Err(BrokerError::InvalidRequest(
            "branch count must be between 1 and 256".to_string(),
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    for branch in branches {
        validate_identity("disk_id", &branch.disk_id)?;
        validate_identity("branch_id", &branch.branch_id)?;
        if branch.project_id < 1000 || branch.quota_bytes == 0 {
            return Err(BrokerError::InvalidRequest(
                "project_id must be >= 1000 and quota_bytes must be positive".to_string(),
            ));
        }
        if !config.disks.contains_key(&branch.disk_id) {
            return Err(BrokerError::InvalidRequest(format!(
                "disk {} is not configured",
                branch.disk_id
            )));
        }
        if !seen.insert((&branch.disk_id, &branch.branch_id)) {
            return Err(BrokerError::InvalidRequest(
                "duplicate branch identity".to_string(),
            ));
        }
    }
    Ok(())
}

mod aggregate;
mod branches;
mod nfs;

pub(crate) use aggregate::{aggregate_root, mounted_mergerfs_entry};
use aggregate::{inspect_aggregate, mount_aggregate, unmount_aggregate};
use branches::*;
use nfs::*;

#[cfg(test)]
mod tests;
