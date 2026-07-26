use crate::schema::LIVE_SCHEMA_SQL;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Component, Path};
use std::time::Duration;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceCheckpointMember {
    pub relative_path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RegisterWorkspaceCheckpoint {
    pub checkpoint_id: String,
    pub workspace_id: String,
    pub relative_prefix: String,
    pub role: String,
    pub reproducibility_class: String,
    pub manifest_sha256: String,
    pub logical_bytes: u64,
    pub removable_after_promotion: bool,
    pub created_at_utc: String,
    pub retention_deadline_utc: String,
    pub members: Vec<WorkspaceCheckpointMember>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceCheckpointSnapshot {
    pub checkpoint_id: String,
    pub workspace_id: String,
    pub relative_prefix: String,
    pub role: String,
    pub reproducibility_class: String,
    pub manifest_sha256: String,
    pub logical_bytes: u64,
    pub removable_after_promotion: bool,
    pub member_count: u64,
    pub created_at_utc: String,
    pub retention_deadline_utc: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceCapacityReport {
    pub quota_bytes: u64,
    pub registered_bytes: u64,
    pub remaining_bytes: u64,
    pub checkpoint_bytes: u64,
    pub materialized_bytes: u64,
    pub checkpoint_count: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceHealthReport {
    pub schema_version: String,
    pub workspace_id: String,
    pub state: String,
    pub health: String,
    pub reasons: Vec<String>,
    pub capacity: WorkspaceCapacityReport,
    pub aggregate_ready: bool,
    pub branches_ready: u64,
    pub branches_total: u64,
    pub active_attachments: u64,
    pub active_operations: u64,
}

pub fn register_workspace_checkpoint(
    live_sqlite_path: &Path,
    request: &RegisterWorkspaceCheckpoint,
) -> rusqlite::Result<WorkspaceCheckpointSnapshot> {
    validate_request(request)?;
    let mut connection = open(live_sqlite_path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if let Some(existing) = read_checkpoint(&transaction, &request.checkpoint_id)? {
        if checkpoint_matches(&existing, request) {
            transaction.commit()?;
            return Ok(existing);
        }
        return Err(rusqlite::Error::InvalidQuery);
    }
    let workspace: Option<(String, i64)> = transaction
        .query_row(
            "SELECT state, quota_bytes FROM compute_workspaces WHERE workspace_id = ?1",
            [&request.workspace_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((state, quota_bytes)) = workspace else {
        return Err(rusqlite::Error::InvalidQuery);
    };
    if state != "ready"
        || transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM compute_workspace_attachments
                 WHERE workspace_id = ?1
                   AND state IN ('requested', 'attached', 'detach_requested')
             )",
            [&request.workspace_id],
            |row| row.get::<_, bool>(0),
        )?
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    transaction.execute(
        "INSERT INTO compute_workspace_checkpoints (
             checkpoint_id, workspace_id, relative_prefix, role,
             reproducibility_class, logical_bytes, checkpoint_manifest_id,
             removable_after_promotion, created_at_utc, updated_at_utc,
             retention_deadline_utc
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, ?10)",
        params![
            request.checkpoint_id,
            request.workspace_id,
            request.relative_prefix,
            request.role,
            request.reproducibility_class,
            to_i64(request.logical_bytes)?,
            request.manifest_sha256,
            request.removable_after_promotion,
            request.created_at_utc,
            request.retention_deadline_utc,
        ],
    )?;
    for member in &request.members {
        let full_path = format!("{}/{}", request.relative_prefix, member.relative_path);
        let conflict: Option<(i64, String)> = transaction
            .query_row(
                "SELECT size_bytes, sha256
                 FROM compute_workspace_checkpoint_members m
                 JOIN compute_workspace_checkpoints c USING (checkpoint_id)
                 WHERE c.workspace_id = ?1 AND m.workspace_relative_path = ?2
                 LIMIT 1",
                params![request.workspace_id, full_path],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if conflict.is_some_and(|(size, sha256)| {
            size != member.size_bytes as i64 || normalize_sha256(&sha256) != member.sha256
        }) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        transaction.execute(
            "INSERT INTO compute_workspace_checkpoint_members (
                 checkpoint_id, workspace_relative_path, size_bytes, sha256
             ) VALUES (?1, ?2, ?3, ?4)",
            params![
                request.checkpoint_id,
                full_path,
                to_i64(member.size_bytes)?,
                member.sha256,
            ],
        )?;
    }
    let registered_bytes = registered_bytes(&transaction, &request.workspace_id)?;
    if registered_bytes > quota_bytes {
        return Err(rusqlite::Error::InvalidQuery);
    }
    transaction.execute(
        "UPDATE compute_workspaces
         SET bytes_written = ?1, generation = generation + 1, updated_at_utc = ?2
         WHERE workspace_id = ?3",
        params![
            registered_bytes,
            request.created_at_utc,
            request.workspace_id
        ],
    )?;
    let snapshot = read_checkpoint(&transaction, &request.checkpoint_id)?
        .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    transaction.commit()?;
    Ok(snapshot)
}

pub fn read_workspace_health(
    live_sqlite_path: &Path,
    workspace_id: &str,
) -> rusqlite::Result<WorkspaceHealthReport> {
    let connection = open(live_sqlite_path)?;
    let (state, quota, aggregate): (String, i64, Option<String>) = connection.query_row(
        "SELECT state, quota_bytes, aggregate_mount_identity
         FROM compute_workspaces WHERE workspace_id = ?1",
        [workspace_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let registered = registered_bytes(&connection, workspace_id)?;
    let checkpoint_bytes: i64 = connection.query_row(
        "SELECT COALESCE(SUM(logical_bytes), 0)
         FROM compute_workspace_checkpoints WHERE workspace_id = ?1",
        [workspace_id],
        |row| row.get(0),
    )?;
    let materialized: i64 = connection.query_row(
        "SELECT COALESCE(SUM(expected_size_bytes), 0)
         FROM compute_workspace_materializations
         WHERE workspace_id = ?1 AND state NOT IN ('failed', 'needs_review', 'cancelled')",
        [workspace_id],
        |row| row.get(0),
    )?;
    let checkpoint_count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM compute_workspace_checkpoints WHERE workspace_id = ?1",
        [workspace_id],
        |row| row.get(0),
    )?;
    let (branches_ready, branches_total): (i64, i64) = connection.query_row(
        "SELECT COALESCE(SUM(CASE WHEN state = 'ready' THEN 1 ELSE 0 END), 0), COUNT(*)
         FROM compute_workspace_branches WHERE workspace_id = ?1",
        [workspace_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let active_attachments: i64 = connection.query_row(
        "SELECT COUNT(*) FROM compute_workspace_attachments
         WHERE workspace_id = ?1
           AND state IN ('requested', 'attached', 'detach_requested')",
        [workspace_id],
        |row| row.get(0),
    )?;
    let active_operations: i64 = connection.query_row(
        "SELECT COUNT(*) FROM compute_workspace_operations
         WHERE workspace_id = ?1
           AND state NOT IN ('succeeded', 'failed', 'needs_review', 'cancelled')",
        [workspace_id],
        |row| row.get(0),
    )?;
    let mut reasons = Vec::new();
    if aggregate.is_none() {
        reasons.push("aggregate identity is not registered".to_string());
    }
    if branches_ready != branches_total || branches_total == 0 {
        reasons.push("not all workspace branches are ready".to_string());
    }
    if registered > quota {
        reasons.push("registered logical bytes exceed quota".to_string());
    }
    if state == "failed" {
        reasons.push("workspace is failed".to_string());
    }
    let health = if reasons.is_empty() {
        "healthy"
    } else if state == "failed" || registered > quota {
        "needs_review"
    } else {
        "degraded"
    };
    Ok(WorkspaceHealthReport {
        schema_version: "dasobjectstore.workspace_health.v1".to_string(),
        workspace_id: workspace_id.to_string(),
        state,
        health: health.to_string(),
        reasons,
        capacity: WorkspaceCapacityReport {
            quota_bytes: quota as u64,
            registered_bytes: registered as u64,
            remaining_bytes: (quota - registered).max(0) as u64,
            checkpoint_bytes: checkpoint_bytes as u64,
            materialized_bytes: materialized as u64,
            checkpoint_count: checkpoint_count as u64,
        },
        aggregate_ready: aggregate.is_some(),
        branches_ready: branches_ready as u64,
        branches_total: branches_total as u64,
        active_attachments: active_attachments as u64,
        active_operations: active_operations as u64,
    })
}

fn registered_bytes(connection: &Connection, workspace_id: &str) -> rusqlite::Result<i64> {
    connection.query_row(
        "SELECT COALESCE(SUM(size_bytes), 0) FROM (
             SELECT workspace_relative_path, MAX(size_bytes) AS size_bytes FROM (
                 SELECT m.workspace_relative_path, m.size_bytes
                 FROM compute_workspace_checkpoint_members m
                 JOIN compute_workspace_checkpoints c USING (checkpoint_id)
                 WHERE c.workspace_id = ?1
                 UNION ALL
                 SELECT destination_relative_path, expected_size_bytes
                 FROM compute_workspace_materializations
                 WHERE workspace_id = ?1
                   AND state NOT IN ('failed', 'needs_review', 'cancelled')
             )
             GROUP BY workspace_relative_path
         )",
        [workspace_id],
        |row| row.get(0),
    )
}

fn read_checkpoint(
    connection: &Connection,
    checkpoint_id: &str,
) -> rusqlite::Result<Option<WorkspaceCheckpointSnapshot>> {
    connection
        .query_row(
            "SELECT c.checkpoint_id, c.workspace_id, c.relative_prefix, c.role,
                    c.reproducibility_class, c.checkpoint_manifest_id,
                    c.logical_bytes, c.removable_after_promotion,
                    COUNT(m.workspace_relative_path), c.created_at_utc,
                    c.retention_deadline_utc
             FROM compute_workspace_checkpoints c
             LEFT JOIN compute_workspace_checkpoint_members m USING (checkpoint_id)
             WHERE c.checkpoint_id = ?1
             GROUP BY c.checkpoint_id",
            [checkpoint_id],
            |row| {
                Ok(WorkspaceCheckpointSnapshot {
                    checkpoint_id: row.get(0)?,
                    workspace_id: row.get(1)?,
                    relative_prefix: row.get(2)?,
                    role: row.get(3)?,
                    reproducibility_class: row.get(4)?,
                    manifest_sha256: row.get(5)?,
                    logical_bytes: row.get::<_, i64>(6)? as u64,
                    removable_after_promotion: row.get(7)?,
                    member_count: row.get::<_, i64>(8)? as u64,
                    created_at_utc: row.get(9)?,
                    retention_deadline_utc: row.get(10)?,
                })
            },
        )
        .optional()
}

fn checkpoint_matches(
    existing: &WorkspaceCheckpointSnapshot,
    request: &RegisterWorkspaceCheckpoint,
) -> bool {
    existing.workspace_id == request.workspace_id
        && existing.relative_prefix == request.relative_prefix
        && existing.role == request.role
        && existing.reproducibility_class == request.reproducibility_class
        && existing.manifest_sha256 == request.manifest_sha256
        && existing.logical_bytes == request.logical_bytes
        && existing.removable_after_promotion == request.removable_after_promotion
        && existing.member_count == request.members.len() as u64
        && existing.retention_deadline_utc == request.retention_deadline_utc
}

fn validate_request(request: &RegisterWorkspaceCheckpoint) -> rusqlite::Result<()> {
    if !valid_identity(&request.checkpoint_id)
        || !valid_identity(&request.workspace_id)
        || !valid_relative_path(&request.relative_prefix)
        || !valid_label(&request.role)
        || !valid_label(&request.reproducibility_class)
        || !valid_sha256(&request.manifest_sha256)
        || request.logical_bytes > i64::MAX as u64
        || request.members.is_empty()
        || request.members.len() > 4096
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let mut total = 0_u64;
    let mut paths = BTreeMap::new();
    for member in &request.members {
        if !valid_relative_path(&member.relative_path)
            || !valid_sha256(&member.sha256)
            || member.size_bytes > i64::MAX as u64
            || paths.insert(&member.relative_path, ()).is_some()
        {
            return Err(rusqlite::Error::InvalidQuery);
        }
        total = total
            .checked_add(member.size_bytes)
            .ok_or(rusqlite::Error::InvalidQuery)?;
    }
    if total != request.logical_bytes {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(())
}

fn open(path: &Path) -> rusqlite::Result<Connection> {
    let connection = Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    Ok(connection)
}

fn valid_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn valid_label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn valid_relative_path(value: &str) -> bool {
    let path = Path::new(value);
    !value.is_empty()
        && value.len() <= 4096
        && !path.is_absolute()
        && path.components().all(
            |component| matches!(component, Component::Normal(part) if part != "." && part != ".."),
        )
}

fn valid_sha256(value: &str) -> bool {
    let value = value.strip_prefix("sha256:").unwrap_or(value);
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn normalize_sha256(value: &str) -> String {
    format!(
        "sha256:{}",
        value
            .strip_prefix("sha256:")
            .unwrap_or(value)
            .to_ascii_lowercase()
    )
}

fn to_i64(value: u64) -> rusqlite::Result<i64> {
    i64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::{
        read_workspace_health, register_workspace_checkpoint, RegisterWorkspaceCheckpoint,
        WorkspaceCheckpointMember,
    };
    use crate::schema::LIVE_SCHEMA_SQL;
    use rusqlite::Connection;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn checkpoint_registration_is_atomic_idempotent_and_updates_unique_accounting() {
        let database = fixture("register", 1_000);
        let first = request("checkpoint-a", "outputs", 600);
        let registered =
            register_workspace_checkpoint(&database, &first).expect("checkpoint registers");
        assert_eq!(registered.logical_bytes, 600);
        assert_eq!(
            register_workspace_checkpoint(&database, &first).expect("exact retry"),
            registered
        );
        let second = request("checkpoint-b", "outputs", 600);
        register_workspace_checkpoint(&database, &second)
            .expect("same immutable members do not double count");
        let health = read_workspace_health(&database, "workspace-a").expect("health");
        assert_eq!(health.capacity.registered_bytes, 600);
        assert_eq!(health.capacity.remaining_bytes, 400);
        assert_eq!(health.capacity.checkpoint_count, 2);
        assert_eq!(health.health, "healthy");
        fs::remove_file(database).expect("cleanup");
    }

    #[test]
    fn checkpoint_over_quota_rolls_back_without_partial_manifest() {
        let database = fixture("quota", 100);
        assert!(register_workspace_checkpoint(
            &database,
            &request("checkpoint-too-large", "outputs", 600)
        )
        .is_err());
        let connection = Connection::open(&database).expect("open");
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM compute_workspace_checkpoints",
                    [],
                    |row| { row.get::<_, i64>(0) }
                )
                .expect("count"),
            0
        );
        assert_eq!(
            connection
                .query_row("SELECT bytes_written FROM compute_workspaces", [], |row| {
                    row.get::<_, i64>(0)
                })
                .expect("bytes"),
            0
        );
        fs::remove_file(database).expect("cleanup");
    }

    #[test]
    fn attached_workspace_cannot_be_checkpointed() {
        let database = fixture("attached", 1_000);
        let connection = Connection::open(&database).expect("open");
        connection
            .execute(
                "INSERT INTO compute_workspace_attachments (
                     workspace_id, client_id, address_or_cidr, mode,
                     export_options_json, state
                 ) VALUES ('workspace-a', 'client-a', '10.0.0.2', 'read_write', '{}', 'attached')",
                [],
            )
            .expect("attachment");
        assert!(
            register_workspace_checkpoint(&database, &request("checkpoint-a", "outputs", 600))
                .is_err()
        );
        fs::remove_file(database).expect("cleanup");
    }

    fn request(id: &str, prefix: &str, bytes: u64) -> RegisterWorkspaceCheckpoint {
        RegisterWorkspaceCheckpoint {
            checkpoint_id: id.to_string(),
            workspace_id: "workspace-a".to_string(),
            relative_prefix: prefix.to_string(),
            role: "result".to_string(),
            reproducibility_class: "derived".to_string(),
            manifest_sha256: format!("sha256:{}", "a".repeat(64)),
            logical_bytes: bytes,
            removable_after_promotion: true,
            created_at_utc: "2026-07-26T10:00:00Z".to_string(),
            retention_deadline_utc: "2026-08-26T10:00:00Z".to_string(),
            members: vec![WorkspaceCheckpointMember {
                relative_path: "result.bin".to_string(),
                size_bytes: bytes,
                sha256: format!("sha256:{}", "b".repeat(64)),
            }],
        }
    }

    fn fixture(name: &str, quota: u64) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "dasobjectstore-workspace-checkpoint-{name}-{}-{}.sqlite",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let _ = fs::remove_file(&path);
        let connection = Connection::open(&path).expect("open");
        connection.execute_batch(LIVE_SCHEMA_SQL).expect("schema");
        connection
            .execute_batch(&format!(
                "INSERT INTO pools VALUES ('pool-a', 'Clean', 'now', 'now');
                 INSERT INTO disks (
                     disk_id, pool_id, role, state, size_bytes,
                     created_at_utc, updated_at_utc
                 ) VALUES ('disk-a', 'pool-a', 'Data', 'Healthy', {quota},
                           'now', 'now');
                 INSERT INTO compute_workspaces (
                     workspace_id, schema_version, request_id, request_digest, pool_id, state,
                     owner, project, purpose, promotion_store_id,
                     requested_capacity_bytes, reserved_capacity_bytes,
                     quota_bytes, minimum_free_bytes_per_disk,
                     aggregation_provider, aggregate_mount_identity,
                     close_cleanup_policy_json, generation,
                     created_at_utc, updated_at_utc, expires_at_utc
                 ) VALUES (
                     'workspace-a', 1, 'request-a', 'sha256:a', 'pool-a', 'ready',
                     'owner', 'project', 'tests', NULL, {quota}, {quota}, {quota},
                     1, 'mergerfs', 'workspace-a', '{{}}', 1, 'now', 'now', 'later'
                 );
                 INSERT INTO compute_workspace_branches (
                     workspace_id, disk_id, branch_id, branch_relative_path,
                     project_id, project_quota_bytes, reserved_bytes, state,
                     created_at_utc
                 ) VALUES (
                     'workspace-a', 'disk-a', 'branch-a', 'workspace-a',
                     1000, {quota}, {quota}, 'ready', 'now'
                 );"
            ))
            .expect("fixture");
        path
    }
}
