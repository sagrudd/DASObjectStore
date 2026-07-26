//! Governed closure, expiry, and cleanup authority for mutable workspaces.

use crate::schema::LIVE_SCHEMA_SQL;
use dasobjectstore_core::ids::WorkspaceId;
use rusqlite::{params, Connection, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const CLEANUP_CONFIRMATION_PREFIX: &str = "CLEAN WORKSPACE ";

#[derive(Clone, Debug)]
pub struct CloseWorkspaceRequest {
    pub live_sqlite_path: PathBuf,
    pub workspace_id: WorkspaceId,
    pub actor_id: String,
    pub application_id: Option<String>,
    pub request_id: String,
    pub request_digest: String,
    pub closed_at_utc: String,
}

#[derive(Clone, Debug)]
pub struct RequestWorkspaceCleanup {
    pub live_sqlite_path: PathBuf,
    pub workspace_id: WorkspaceId,
    pub operation_id: String,
    pub actor_id: String,
    pub application_id: Option<String>,
    pub request_id: String,
    pub request_digest: String,
    pub confirmation_phrase: String,
    pub requested_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceExpiryCandidate {
    pub workspace_id: String,
    pub state: String,
    pub expires_at_utc: String,
    pub action: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceCleanupBranch {
    pub disk_id: String,
    pub branch_id: String,
    pub project_id: u32,
    pub quota_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceCleanupPlan {
    pub schema_version: String,
    pub workspace_id: String,
    pub operation_id: Option<String>,
    pub state: String,
    pub eligible: bool,
    pub blockers: Vec<String>,
    pub aggregate_mount_identity: Option<String>,
    pub minimum_free_bytes_per_disk: u64,
    pub branches: Vec<WorkspaceCleanupBranch>,
}

pub fn close_workspace(request: &CloseWorkspaceRequest) -> rusqlite::Result<WorkspaceCleanupPlan> {
    validate_identity(&request.actor_id)?;
    validate_identity(&request.request_id)?;
    let mut connection = open(&request.live_sqlite_path)?;
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let state: String = tx.query_row(
        "SELECT state FROM compute_workspaces WHERE workspace_id=?1",
        [request.workspace_id.as_str()],
        |row| row.get(0),
    )?;
    if state == "closed" {
        let plan = read_plan(&tx, request.workspace_id.as_str(), None)?;
        tx.commit()?;
        return Ok(plan);
    }
    if state != "ready" {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let blockers = closure_blockers(&tx, request.workspace_id.as_str())?;
    if !blockers.is_empty() {
        audit(
            &tx,
            request.workspace_id.as_str(),
            &request.actor_id,
            request.application_id.as_deref(),
            "close",
            Some(&request.request_digest),
            "denied",
            &blockers.join("; "),
            Some(&request.request_id),
            &request.closed_at_utc,
        )?;
        tx.commit()?;
        return read_cleanup_plan(&request.live_sqlite_path, &request.workspace_id);
    }
    tx.execute(
        "UPDATE compute_workspaces SET state='closed', generation=generation+1,
         updated_at_utc=?2 WHERE workspace_id=?1 AND state='ready'",
        params![request.workspace_id.as_str(), request.closed_at_utc],
    )?;
    audit(
        &tx,
        request.workspace_id.as_str(),
        &request.actor_id,
        request.application_id.as_deref(),
        "close",
        Some(&request.request_digest),
        "allowed",
        "workspace closed; cleanup remains separately confirmed",
        Some(&request.request_id),
        &request.closed_at_utc,
    )?;
    let plan = read_plan(&tx, request.workspace_id.as_str(), None)?;
    tx.commit()?;
    Ok(plan)
}

pub fn report_expired_workspaces(
    live_sqlite_path: &Path,
    now_utc: &str,
) -> rusqlite::Result<Vec<WorkspaceExpiryCandidate>> {
    let connection = open(live_sqlite_path)?;
    let mut statement = connection.prepare(
        "SELECT workspace_id,state,expires_at_utc FROM compute_workspaces
         WHERE state IN ('ready','expired') AND expires_at_utc<=?1
         ORDER BY expires_at_utc,workspace_id",
    )?;
    let candidates = statement
        .query_map([now_utc], |row| {
            let state = row.get::<_, String>(1)?;
            Ok(WorkspaceExpiryCandidate {
                workspace_id: row.get(0)?,
                action: if state == "expired" {
                    "already_expired"
                } else {
                    "report_only"
                }
                .to_string(),
                state,
                expires_at_utc: row.get(2)?,
            })
        })?
        .collect();
    candidates
}

pub fn apply_workspace_expiry(
    live_sqlite_path: &Path,
    workspace_id: &WorkspaceId,
    actor_id: &str,
    application_id: Option<&str>,
    now_utc: &str,
) -> rusqlite::Result<WorkspaceCleanupPlan> {
    validate_identity(actor_id)?;
    let mut connection = open(live_sqlite_path)?;
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (state, expires): (String, String) = tx.query_row(
        "SELECT state,expires_at_utc FROM compute_workspaces WHERE workspace_id=?1",
        [workspace_id.as_str()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if state == "expired" {
        let plan = read_plan(&tx, workspace_id.as_str(), None)?;
        tx.commit()?;
        return Ok(plan);
    }
    if state != "ready" || expires.as_str() > now_utc {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let blockers = closure_blockers(&tx, workspace_id.as_str())?;
    if !blockers.is_empty() {
        return Err(rusqlite::Error::InvalidQuery);
    }
    tx.execute(
        "UPDATE compute_workspaces SET state='expired',generation=generation+1,
         updated_at_utc=?2 WHERE workspace_id=?1 AND state='ready'",
        params![workspace_id.as_str(), now_utc],
    )?;
    audit(
        &tx,
        workspace_id.as_str(),
        actor_id,
        application_id,
        "apply_expiry",
        None,
        "allowed",
        "declared expiry applied after closure evidence",
        None,
        now_utc,
    )?;
    let plan = read_plan(&tx, workspace_id.as_str(), None)?;
    tx.commit()?;
    Ok(plan)
}

pub fn read_cleanup_plan(
    live_sqlite_path: &Path,
    workspace_id: &WorkspaceId,
) -> rusqlite::Result<WorkspaceCleanupPlan> {
    let connection = open(live_sqlite_path)?;
    read_plan(&connection, workspace_id.as_str(), None)
}

pub fn request_workspace_cleanup(
    request: &RequestWorkspaceCleanup,
) -> rusqlite::Result<WorkspaceCleanupPlan> {
    validate_identity(&request.actor_id)?;
    validate_identity(&request.operation_id)?;
    validate_identity(&request.request_id)?;
    if request.confirmation_phrase
        != format!(
            "{CLEANUP_CONFIRMATION_PREFIX}{}",
            request.workspace_id.as_str()
        )
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let mut connection = open(&request.live_sqlite_path)?;
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut plan = read_plan(&tx, request.workspace_id.as_str(), None)?;
    if !plan.eligible {
        audit(
            &tx,
            request.workspace_id.as_str(),
            &request.actor_id,
            request.application_id.as_deref(),
            "request_cleanup",
            Some(&request.request_digest),
            "denied",
            &plan.blockers.join("; "),
            Some(&request.request_id),
            &request.requested_at_utc,
        )?;
        tx.commit()?;
        return Ok(plan);
    }
    tx.execute(
        "INSERT INTO compute_workspace_operations (
           operation_id,workspace_id,operation_kind,request_id,request_digest,
           state,stage,total_units,max_attempts,recovery_disposition,
           created_at_utc,updated_at_utc
         ) VALUES (?1,?2,'cleanup',?3,?4,'queued','cleanup_planned',?5,8,
                   'retry_idempotent',?6,?6)
         ON CONFLICT(workspace_id,operation_kind,request_id) DO NOTHING",
        params![
            request.operation_id,
            request.workspace_id.as_str(),
            request.request_id,
            request.request_digest,
            plan.branches.len() as u64,
            request.requested_at_utc,
        ],
    )?;
    let identity: (String, String) = tx.query_row(
        "SELECT operation_id,request_digest FROM compute_workspace_operations
         WHERE workspace_id=?1 AND operation_kind='cleanup' AND request_id=?2",
        params![request.workspace_id.as_str(), request.request_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if identity != (request.operation_id.clone(), request.request_digest.clone()) {
        return Err(rusqlite::Error::InvalidQuery);
    }
    tx.execute(
        "UPDATE compute_workspaces SET state='cleanup_pending',
         generation=generation+1,updated_at_utc=?2
         WHERE workspace_id=?1 AND state IN ('closed','expired')",
        params![request.workspace_id.as_str(), request.requested_at_utc],
    )?;
    audit(
        &tx,
        request.workspace_id.as_str(),
        &request.actor_id,
        request.application_id.as_deref(),
        "request_cleanup",
        Some(&request.request_digest),
        "allowed",
        "explicit confirmation accepted; daemon cleanup queued",
        Some(&request.operation_id),
        &request.requested_at_utc,
    )?;
    plan = read_plan(
        &tx,
        request.workspace_id.as_str(),
        Some(&request.operation_id),
    )?;
    tx.commit()?;
    Ok(plan)
}

pub fn cancel_workspace_cleanup(
    live_sqlite_path: &Path,
    workspace_id: &WorkspaceId,
    operation_id: &str,
    actor_id: &str,
    cancelled_at_utc: &str,
) -> rusqlite::Result<WorkspaceCleanupPlan> {
    let mut connection = open(live_sqlite_path)?;
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let operation_state: String = tx.query_row(
        "SELECT state FROM compute_workspace_operations
         WHERE operation_id=?1 AND workspace_id=?2 AND operation_kind='cleanup'",
        params![operation_id, workspace_id.as_str()],
        |row| row.get(0),
    )?;
    if !matches!(
        operation_state.as_str(),
        "queued" | "retry_wait" | "cancelled"
    ) {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let removed: i64 = tx.query_row(
        "SELECT COUNT(*) FROM compute_workspace_branches
         WHERE workspace_id=?1 AND state='released'",
        [workspace_id.as_str()],
        |row| row.get(0),
    )?;
    if removed != 0 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    tx.execute(
        "UPDATE compute_workspace_operations SET state='cancelled',
         cancellation_requested=1,recovery_disposition='terminal',
         lease_owner=NULL,lease_expires_at_utc=NULL,completed_at_utc=?3,
         generation=generation+1,updated_at_utc=?3
         WHERE operation_id=?2 AND workspace_id=?1
           AND operation_kind='cleanup' AND state NOT IN ('succeeded','failed','needs_review')",
        params![workspace_id.as_str(), operation_id, cancelled_at_utc],
    )?;
    tx.execute(
        "UPDATE compute_workspaces SET state='closed',generation=generation+1,
         updated_at_utc=?2 WHERE workspace_id=?1 AND state='cleanup_pending'",
        params![workspace_id.as_str(), cancelled_at_utc],
    )?;
    audit(
        &tx,
        workspace_id.as_str(),
        actor_id,
        None,
        "cancel_cleanup",
        None,
        "allowed",
        "cleanup cancelled before any branch release",
        Some(operation_id),
        cancelled_at_utc,
    )?;
    let plan = read_plan(&tx, workspace_id.as_str(), Some(operation_id))?;
    tx.commit()?;
    Ok(plan)
}

pub fn complete_workspace_cleanup(
    live_sqlite_path: &Path,
    workspace_id: &WorkspaceId,
    operation_id: &str,
    lease_owner: &str,
    completed_at_utc: &str,
) -> rusqlite::Result<WorkspaceCleanupPlan> {
    let mut connection = open(live_sqlite_path)?;
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let pending: i64 = tx.query_row(
        "SELECT COUNT(*) FROM compute_workspace_branches
         WHERE workspace_id=?1 AND state!='removed'",
        [workspace_id.as_str()],
        |row| row.get(0),
    )?;
    if pending != 0 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    tx.execute(
        "UPDATE compute_workspace_branches SET state='released',released_at_utc=?2
         WHERE workspace_id=?1 AND state='removed'",
        params![workspace_id.as_str(), completed_at_utc],
    )?;
    tx.execute(
        "UPDATE disk_capacity_claims SET state='released',released_at_utc=?2,
         updated_at_utc=?2,lease_owner=NULL,lease_expires_at_utc=NULL
         WHERE claim_kind='workspace' AND owner_id=?1 AND state='active'",
        params![workspace_id.as_str(), completed_at_utc],
    )?;
    tx.execute(
        "UPDATE compute_workspace_operations SET state='succeeded',stage='cleaned',
         completed_units=total_units,recovery_disposition='terminal',
         result_json='{\"cleaned\":true}',lease_owner=NULL,lease_expires_at_utc=NULL,
         completed_at_utc=?4,generation=generation+1,updated_at_utc=?4
         WHERE operation_id=?2 AND workspace_id=?1 AND lease_owner=?3 AND state='running'",
        params![
            workspace_id.as_str(),
            operation_id,
            lease_owner,
            completed_at_utc
        ],
    )?;
    tx.execute(
        "UPDATE compute_workspaces SET state='cleaned',bytes_reclaimable=0,
         generation=generation+1,updated_at_utc=?2
         WHERE workspace_id=?1 AND state='cleanup_pending'",
        params![workspace_id.as_str(), completed_at_utc],
    )?;
    audit(
        &tx,
        workspace_id.as_str(),
        lease_owner,
        Some("dasobjectstored"),
        "complete_cleanup",
        None,
        "allowed",
        "all marker-owned branches removed; capacity claims released",
        Some(operation_id),
        completed_at_utc,
    )?;
    let plan = read_plan(&tx, workspace_id.as_str(), Some(operation_id))?;
    tx.commit()?;
    Ok(plan)
}

pub fn record_workspace_branch_removed(
    live_sqlite_path: &Path,
    workspace_id: &WorkspaceId,
    disk_id: &str,
    operation_id: &str,
    lease_owner: &str,
    recorded_at_utc: &str,
) -> rusqlite::Result<()> {
    let connection = open(live_sqlite_path)?;
    let owned: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM compute_workspace_operations
         WHERE operation_id=?1 AND workspace_id=?2 AND operation_kind='cleanup'
           AND state='running' AND lease_owner=?3)",
        params![operation_id, workspace_id.as_str(), lease_owner],
        |row| row.get(0),
    )?;
    if !owned {
        return Err(rusqlite::Error::InvalidQuery);
    }
    connection.execute(
        "UPDATE compute_workspace_branches SET state='removed'
         WHERE workspace_id=?1 AND disk_id=?2 AND state!='released'",
        params![workspace_id.as_str(), disk_id],
    )?;
    connection.execute(
        "UPDATE compute_workspace_operations SET stage='removing_branches',
         completed_units=(SELECT COUNT(*) FROM compute_workspace_branches
                          WHERE workspace_id=?1 AND state='removed'),
         generation=generation+1,updated_at_utc=?4
         WHERE operation_id=?2 AND lease_owner=?3",
        params![
            workspace_id.as_str(),
            operation_id,
            lease_owner,
            recorded_at_utc
        ],
    )?;
    Ok(())
}

fn closure_blockers(connection: &Connection, workspace_id: &str) -> rusqlite::Result<Vec<String>> {
    let mut blockers = Vec::new();
    let active_attachments: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM compute_workspace_attachments
         WHERE workspace_id=?1 AND state NOT IN ('detached','cancelled'))",
        [workspace_id],
        |row| row.get(0),
    )?;
    if active_attachments {
        blockers.push("active attachments must be detached".to_string());
    }
    let active_operations: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM compute_workspace_operations
         WHERE workspace_id=?1 AND state NOT IN ('succeeded','failed','needs_review','cancelled'))",
        [workspace_id],
        |row| row.get(0),
    )?;
    if active_operations {
        blockers.push("active workspace operations remain".to_string());
    }
    let unsafe_checkpoints: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM compute_workspace_checkpoints c
         WHERE c.workspace_id=?1 AND (
           c.removable_after_promotion=0 OR NOT EXISTS(
             SELECT 1 FROM compute_workspace_promotions p
             WHERE p.checkpoint_id=c.checkpoint_id AND p.state='completed')))",
        [workspace_id],
        |row| row.get(0),
    )?;
    if unsafe_checkpoints {
        blockers.push("required checkpoint promotion evidence is incomplete".to_string());
    }
    Ok(blockers)
}

fn read_plan(
    connection: &Connection,
    workspace_id: &str,
    operation_id: Option<&str>,
) -> rusqlite::Result<WorkspaceCleanupPlan> {
    let (state, mount, minimum): (String, Option<String>, u64) = connection.query_row(
        "SELECT state,aggregate_mount_identity,minimum_free_bytes_per_disk
         FROM compute_workspaces WHERE workspace_id=?1",
        [workspace_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let mut blockers = closure_blockers(connection, workspace_id)?;
    if !matches!(state.as_str(), "closed" | "expired" | "cleanup_pending") {
        blockers.push("workspace is not closed or expired".to_string());
    }
    let mut statement = connection.prepare(
        "SELECT disk_id,branch_id,project_id,project_quota_bytes
         FROM compute_workspace_branches WHERE workspace_id=?1 AND state!='released'
         ORDER BY disk_id",
    )?;
    let branches = statement
        .query_map([workspace_id], |row| {
            let project_id = row
                .get::<_, Option<u32>>(2)?
                .ok_or(rusqlite::Error::InvalidQuery)?;
            let quota_bytes = row
                .get::<_, Option<u64>>(3)?
                .ok_or(rusqlite::Error::InvalidQuery)?;
            Ok(WorkspaceCleanupBranch {
                disk_id: row.get(0)?,
                branch_id: row.get(1)?,
                project_id,
                quota_bytes,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(WorkspaceCleanupPlan {
        schema_version: "dasobjectstore.workspace_cleanup_plan.v1".to_string(),
        workspace_id: workspace_id.to_string(),
        operation_id: operation_id.map(ToOwned::to_owned),
        state,
        eligible: blockers.is_empty(),
        blockers,
        aggregate_mount_identity: mount,
        minimum_free_bytes_per_disk: minimum,
        branches,
    })
}

#[allow(clippy::too_many_arguments)]
fn audit(
    connection: &Connection,
    workspace_id: &str,
    actor_id: &str,
    application_id: Option<&str>,
    operation: &str,
    request_digest: Option<&str>,
    decision: &str,
    result: &str,
    reference_id: Option<&str>,
    recorded_at_utc: &str,
) -> rusqlite::Result<()> {
    let identity = format!(
        "audit-{:x}",
        Sha256::digest(
            format!("{workspace_id}\0{operation}\0{reference_id:?}\0{recorded_at_utc}").as_bytes()
        )
    );
    connection.execute(
        "INSERT INTO compute_workspace_audit_events (
           event_id,workspace_id,actor_id,application_id,operation,request_digest,
           decision,result,reference_id,recorded_at_utc
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
         ON CONFLICT(event_id) DO NOTHING",
        params![
            identity,
            workspace_id,
            actor_id,
            application_id,
            operation,
            request_digest,
            decision,
            result,
            reference_id,
            recorded_at_utc
        ],
    )?;
    Ok(())
}

fn open(path: &Path) -> rusqlite::Result<Connection> {
    let connection = Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    Ok(connection)
}

fn validate_identity(value: &str) -> rusqlite::Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::LIVE_SCHEMA_SQL;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn closure_and_cleanup_request_are_separate_audited_transactions() {
        let database = fixture("governed");
        let workspace = WorkspaceId::new("workspace-a").expect("workspace");
        let closed = close_workspace(&CloseWorkspaceRequest {
            live_sqlite_path: database.clone(),
            workspace_id: workspace.clone(),
            actor_id: "operator-a".to_string(),
            application_id: None,
            request_id: "close-a".to_string(),
            request_digest: format!("sha256:{}", "a".repeat(64)),
            closed_at_utc: "2026-07-26T12:00:00Z".to_string(),
        })
        .expect("close");
        assert_eq!(closed.state, "closed");
        assert!(closed.eligible);
        let queued = request_workspace_cleanup(&RequestWorkspaceCleanup {
            live_sqlite_path: database.clone(),
            workspace_id: workspace,
            operation_id: "cleanup-a".to_string(),
            actor_id: "operator-a".to_string(),
            application_id: None,
            request_id: "cleanup-request-a".to_string(),
            request_digest: format!("sha256:{}", "b".repeat(64)),
            confirmation_phrase: "CLEAN WORKSPACE workspace-a".to_string(),
            requested_at_utc: "2026-07-26T12:01:00Z".to_string(),
        })
        .expect("queue");
        assert_eq!(queued.state, "cleanup_pending");
        let connection = Connection::open(&database).expect("open");
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM compute_workspace_audit_events",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .expect("audit"),
            2
        );
        fs::remove_file(database).expect("cleanup");
    }

    #[test]
    fn closure_refuses_active_attachment_without_mutating_state() {
        let database = fixture("attached");
        Connection::open(&database)
            .expect("open")
            .execute(
                "INSERT INTO compute_workspace_attachments (
                   workspace_id,client_id,address_or_cidr,mode,export_options_json,state
                 ) VALUES ('workspace-a','client-a','10.0.0.1','read_write','{}','attached')",
                [],
            )
            .expect("attachment");
        let workspace = WorkspaceId::new("workspace-a").expect("workspace");
        let plan = close_workspace(&CloseWorkspaceRequest {
            live_sqlite_path: database.clone(),
            workspace_id: workspace,
            actor_id: "operator-a".to_string(),
            application_id: None,
            request_id: "close-a".to_string(),
            request_digest: format!("sha256:{}", "a".repeat(64)),
            closed_at_utc: "2026-07-26T12:00:00Z".to_string(),
        })
        .expect("denied plan");
        assert_eq!(plan.state, "ready");
        assert!(plan
            .blockers
            .iter()
            .any(|value| value.contains("attachments")));
        fs::remove_file(database).expect("cleanup");
    }

    fn fixture(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "dos-workspace-cleanup-{label}-{}-{}.sqlite",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let connection = Connection::open(&path).expect("open");
        connection.execute_batch(LIVE_SCHEMA_SQL).expect("schema");
        connection
            .execute_batch(
                "INSERT INTO pools (pool_id,state,created_at_utc,updated_at_utc)
                 VALUES ('pool-a','Healthy','now','now');
                 INSERT INTO disks (
                   disk_id,pool_id,role,state,size_bytes,created_at_utc,updated_at_utc
                 ) VALUES ('disk-a','pool-a','data','Ready',100000,'now','now');
                 INSERT INTO compute_workspaces (
                   workspace_id,schema_version,request_id,request_digest,pool_id,state,
                   owner,project,purpose,requested_capacity_bytes,reserved_capacity_bytes,
                   quota_bytes,minimum_free_bytes_per_disk,aggregation_provider,
                   aggregate_mount_identity,close_cleanup_policy_json,generation,
                   created_at_utc,updated_at_utc,expires_at_utc
                 ) VALUES (
                   'workspace-a',1,'reserve-a','sha256:reserve','pool-a','ready',
                   'owner','project','test',1000,1000,1000,10,'mergerfs',
                   'workspace-a','{}',1,'now','now','2026-07-27T00:00:00Z'
                 );
                 INSERT INTO compute_workspace_branches (
                   workspace_id,disk_id,branch_id,branch_relative_path,project_id,
                   project_quota_bytes,reserved_bytes,state,created_at_utc
                 ) VALUES (
                   'workspace-a','disk-a','branch-a','workspaces/branch-a',10001,
                   1000,1000,'ready','now'
                 );
                 INSERT INTO disk_capacity_claims (
                   claim_id,claim_kind,owner_id,request_id,request_digest,disk_id,
                   state,reserved_bytes,consumed_bytes,created_at_utc,updated_at_utc
                 ) VALUES (
                   'workspace:workspace-a:disk-a','workspace','workspace-a',
                   'reserve-a','sha256:reserve','disk-a','active',1000,0,'now','now'
                 );",
            )
            .expect("fixture");
        path
    }
}
