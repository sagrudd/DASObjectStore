use dasobjectstore_core::workspace::WorkspaceOperationKind;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceMaterializationSnapshot {
    pub workspace_id: String,
    pub operation_id: String,
    pub source_object_id: String,
    pub source_placement_id: String,
    pub destination_relative_path: String,
    pub expected_size_bytes: u64,
    pub expected_sha256: String,
    pub observed_sha256: Option<String>,
    pub state: String,
}

pub fn list_active_workspace_materializations(
    live_sqlite_path: &Path,
) -> rusqlite::Result<Vec<WorkspaceMaterializationSnapshot>> {
    let connection = Connection::open(live_sqlite_path)?;
    let mut statement = connection.prepare(
        "SELECT m.workspace_id, m.operation_id, m.source_object_id,
                m.source_placement_id, m.destination_relative_path,
                m.expected_size_bytes, m.expected_sha256, m.observed_sha256,
                m.state
         FROM compute_workspace_materializations m
         JOIN compute_workspace_operations o USING (operation_id)
         WHERE m.state NOT IN ('completed', 'failed', 'needs_review', 'cancelled')
           AND o.operation_kind = 'materialize'
           AND o.state NOT IN ('succeeded', 'failed', 'cancelled', 'needs_review')
         ORDER BY o.created_at_utc, m.operation_id",
    )?;
    let rows = statement
        .query_map([], |row| {
            let size = row.get::<_, i64>(5)?;
            Ok(WorkspaceMaterializationSnapshot {
                workspace_id: row.get(0)?,
                operation_id: row.get(1)?,
                source_object_id: row.get(2)?,
                source_placement_id: row.get(3)?,
                destination_relative_path: row.get(4)?,
                expected_size_bytes: u64::try_from(size)
                    .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(5, i64::MAX))?,
                expected_sha256: row.get(6)?,
                observed_sha256: row.get(7)?,
                state: row.get(8)?,
            })
        })?
        .collect();
    rows
}

#[allow(clippy::too_many_arguments)]
pub fn register_workspace_materialization(
    live_sqlite_path: &Path,
    workspace_id: &str,
    operation_id: &str,
    source_object_id: &str,
    source_placement_id: &str,
    destination_relative_path: &str,
    expected_size_bytes: u64,
    expected_sha256: &str,
) -> rusqlite::Result<WorkspaceMaterializationSnapshot> {
    if expected_size_bytes == 0
        || expected_size_bytes > i64::MAX as u64
        || !valid_sha256(expected_sha256)
        || !valid_relative_path(destination_relative_path)
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let mut connection = Connection::open(live_sqlite_path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let operation: Option<(String, String)> = transaction
        .query_row(
            "SELECT operation_kind, state FROM compute_workspace_operations
             WHERE operation_id = ?1 AND workspace_id = ?2",
            params![operation_id, workspace_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((operation_kind, operation_state)) = operation else {
        return Err(rusqlite::Error::InvalidQuery);
    };
    if operation_kind != WorkspaceOperationKind::Materialize.as_str() {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let expected_size = i64::try_from(expected_size_bytes)
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, i64::MAX))?;
    if transaction
        .query_row(
            "SELECT 1 FROM compute_workspace_materializations WHERE operation_id = ?1",
            [operation_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some()
    {
        let snapshot = read_in_transaction(&transaction, operation_id)?;
        if snapshot.workspace_id != workspace_id
            || snapshot.source_object_id != source_object_id
            || snapshot.source_placement_id != source_placement_id
            || snapshot.destination_relative_path != destination_relative_path
            || snapshot.expected_size_bytes != expected_size_bytes
            || normalized_sha256(&snapshot.expected_sha256) != normalized_sha256(expected_sha256)
        {
            return Err(rusqlite::Error::InvalidQuery);
        }
        transaction.commit()?;
        return Ok(snapshot);
    }
    if operation_state != "queued" {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let source = transaction
        .query_row(
            "SELECT o.size_bytes, o.content_hash, p.content_hash,
                    p.verified_at_utc IS NOT NULL
             FROM placements p JOIN objects o USING (object_id)
             WHERE p.placement_id = ?1 AND o.object_id = ?2",
            params![source_placement_id, source_object_id],
            |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, bool>(3)?,
                ))
            },
        )
        .optional()?;
    let expected_digest = normalized_sha256(expected_sha256);
    if !source.is_some_and(|(size, object_hash, placement_hash, verified)| {
        verified
            && size == Some(expected_size)
            && object_hash.as_deref().map(normalized_sha256) == Some(expected_digest.clone())
            && placement_hash.as_deref().map(normalized_sha256) == Some(expected_digest.clone())
    }) {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let (quota_bytes, existing_bytes): (i64, i64) = transaction.query_row(
        "SELECT w.quota_bytes,
                COALESCE((SELECT SUM(expected_size_bytes)
                          FROM compute_workspace_materializations
                          WHERE workspace_id = w.workspace_id
                            AND state NOT IN ('failed', 'needs_review')), 0)
         FROM compute_workspaces w
         WHERE w.workspace_id = ?1 AND w.state = 'ready'",
        [workspace_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if existing_bytes.saturating_add(expected_size) > quota_bytes {
        return Err(rusqlite::Error::InvalidQuery);
    }
    transaction.execute(
        "INSERT INTO compute_workspace_materializations (
             workspace_id, operation_id, source_object_id, source_placement_id,
             destination_relative_path, expected_size_bytes, expected_sha256,
             state
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'queued')
         ON CONFLICT(workspace_id, destination_relative_path) DO NOTHING",
        params![
            workspace_id,
            operation_id,
            source_object_id,
            source_placement_id,
            destination_relative_path,
            expected_size,
            expected_sha256
        ],
    )?;
    let snapshot = read_in_transaction(&transaction, operation_id)?;
    if snapshot.source_object_id != source_object_id
        || snapshot.source_placement_id != source_placement_id
        || snapshot.destination_relative_path != destination_relative_path
        || snapshot.expected_size_bytes != expected_size_bytes
        || snapshot.expected_sha256 != expected_sha256
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    transaction.commit()?;
    Ok(snapshot)
}

fn valid_sha256(value: &str) -> bool {
    let value = value.strip_prefix("sha256:").unwrap_or(value);
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn normalized_sha256(value: &str) -> String {
    value
        .strip_prefix("sha256:")
        .unwrap_or(value)
        .to_ascii_lowercase()
}

fn valid_relative_path(value: &str) -> bool {
    let path = Path::new(value);
    !value.is_empty()
        && value.len() <= 4096
        && !path.is_absolute()
        && path.components().all(|component| {
            matches!(component, std::path::Component::Normal(part) if part != "." && part != "..")
        })
}

pub fn publish_workspace_materialization_state(
    live_sqlite_path: &Path,
    operation_id: &str,
    expected_state: &str,
    state: &str,
    observed_sha256: Option<&str>,
    completed_at_utc: Option<&str>,
) -> rusqlite::Result<bool> {
    let connection = Connection::open(live_sqlite_path)?;
    Ok(connection.execute(
        "UPDATE compute_workspace_materializations
         SET state = ?1, observed_sha256 = COALESCE(?2, observed_sha256),
             completed_at_utc = COALESCE(?3, completed_at_utc)
         WHERE operation_id = ?4 AND state = ?5",
        params![
            state,
            observed_sha256,
            completed_at_utc,
            operation_id,
            expected_state
        ],
    )? == 1)
}

#[allow(clippy::too_many_arguments)]
pub fn finish_workspace_materialization(
    live_sqlite_path: &Path,
    operation_id: &str,
    lease_owner: &str,
    expected_generation: u64,
    materialization_state: &str,
    operation_state: &str,
    observed_sha256: Option<&str>,
    result_json: Option<&str>,
    failure_code: Option<&str>,
    failure_message: Option<&str>,
    completed_at_utc: &str,
) -> rusqlite::Result<bool> {
    if !matches!(
        materialization_state,
        "completed" | "needs_review" | "cancelled"
    ) || !matches!(operation_state, "succeeded" | "needs_review" | "cancelled")
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let mut connection = Connection::open(live_sqlite_path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let materialization_changed = transaction.execute(
        "UPDATE compute_workspace_materializations
         SET state = ?1, observed_sha256 = COALESCE(?2, observed_sha256),
             completed_at_utc = CASE WHEN ?1 = 'completed' THEN ?3 ELSE NULL END
         WHERE operation_id = ?4 AND state = 'copying'",
        params![
            materialization_state,
            observed_sha256,
            completed_at_utc,
            operation_id
        ],
    )?;
    let operation_changed = transaction.execute(
        "UPDATE compute_workspace_operations
         SET state = ?1, lease_owner = NULL, lease_expires_at_utc = NULL,
             recovery_disposition = 'terminal', result_json = ?2,
             failure_code = ?3, failure_message = ?4, completed_at_utc = ?5,
             generation = generation + 1, updated_at_utc = ?5
         WHERE operation_id = ?6 AND state = 'running' AND lease_owner = ?7
           AND generation = ?8",
        params![
            operation_state,
            result_json,
            failure_code,
            failure_message,
            completed_at_utc,
            operation_id,
            lease_owner,
            i64::try_from(expected_generation)
                .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(8, i64::MAX))?
        ],
    )?;
    if materialization_changed != 1 || operation_changed != 1 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    transaction.commit()?;
    Ok(true)
}

fn read_in_transaction(
    transaction: &rusqlite::Transaction<'_>,
    operation_id: &str,
) -> rusqlite::Result<WorkspaceMaterializationSnapshot> {
    transaction.query_row(
        "SELECT workspace_id, operation_id, source_object_id, source_placement_id,
                destination_relative_path, expected_size_bytes, expected_sha256,
                observed_sha256, state
         FROM compute_workspace_materializations WHERE operation_id = ?1",
        [operation_id],
        |row| {
            let size = row.get::<_, i64>(5)?;
            Ok(WorkspaceMaterializationSnapshot {
                workspace_id: row.get(0)?,
                operation_id: row.get(1)?,
                source_object_id: row.get(2)?,
                source_placement_id: row.get(3)?,
                destination_relative_path: row.get(4)?,
                expected_size_bytes: u64::try_from(size)
                    .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(5, i64::MAX))?,
                expected_sha256: row.get(6)?,
                observed_sha256: row.get(7)?,
                state: row.get(8)?,
            })
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LIVE_SCHEMA_SQL;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn registration_is_verified_quota_bounded_and_idempotent() {
        let path = std::env::temp_dir().join(format!(
            "dos-workspace-materialization-{}-{}.sqlite",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let connection = Connection::open(&path).expect("database");
        connection.execute_batch(LIVE_SCHEMA_SQL).expect("schema");
        let hash = format!("sha256:{}", "a".repeat(64));
        connection
            .execute_batch(&format!(
                "INSERT INTO pools VALUES ('pool-a','active','t','t');
                 INSERT INTO disks (disk_id,pool_id,role,state,created_at_utc,updated_at_utc)
                    VALUES ('disk-a','pool-a','hdd','active','t','t');
                 INSERT INTO stores VALUES ('store-a','pool-a','generated_data','{{}}','t','t');
                 INSERT INTO objects VALUES
                    ('object-a','store-a','naive','available',64,'{hash}','t','t');
                 INSERT INTO placements VALUES
                    ('placement-a','object-a','disk-a','objects/a','{hash}','t','t');
                 INSERT INTO compute_workspaces (
                    workspace_id,schema_version,request_id,request_digest,pool_id,
                    state,owner,project,purpose,requested_capacity_bytes,
                    reserved_capacity_bytes,quota_bytes,minimum_free_bytes_per_disk,
                    aggregation_provider,close_cleanup_policy_json,generation,
                    created_at_utc,updated_at_utc,expires_at_utc
                 ) VALUES (
                    'workspace-a','dasobjectstore.compute_workspace.v1','r','d',
                    'pool-a','ready','owner','project','purpose',128,128,128,1,
                    'mergerfs','{{}}',1,'t','t','z'
                 );
                 INSERT INTO compute_workspace_operations (
                    operation_id,workspace_id,operation_kind,request_id,request_digest,
                    state,stage,completed_bytes,total_bytes,completed_units,total_units,
                    cancellation_requested,retry_count,max_attempts,lease_epoch,
                    recovery_disposition,generation,created_at_utc,updated_at_utc
                 ) VALUES (
                    'operation-a','workspace-a','materialize','mr','md','queued',
                    'queued',0,64,0,1,0,0,3,0,'replay_safe',1,'t','t'
                 );"
            ))
            .expect("fixture");
        drop(connection);

        let first = register_workspace_materialization(
            &path,
            "workspace-a",
            "operation-a",
            "object-a",
            "placement-a",
            "inputs/a.bin",
            64,
            &hash,
        )
        .expect("register");
        assert_eq!(first.state, "queued");
        assert_eq!(
            register_workspace_materialization(
                &path,
                "workspace-a",
                "operation-a",
                "object-a",
                "placement-a",
                "inputs/a.bin",
                64,
                &hash,
            )
            .expect("idempotent"),
            first
        );
        assert!(register_workspace_materialization(
            &path,
            "workspace-a",
            "operation-a",
            "object-a",
            "placement-a",
            "../escape",
            64,
            &hash,
        )
        .is_err());
        let connection = Connection::open(&path).expect("reopen");
        connection
            .execute_batch(
                "UPDATE compute_workspace_materializations SET state='copying'
                 WHERE operation_id='operation-a';
                 UPDATE compute_workspace_operations
                 SET state='running', lease_owner='worker-a', generation=2
                 WHERE operation_id='operation-a';",
            )
            .expect("running fixture");
        drop(connection);
        assert!(finish_workspace_materialization(
            &path,
            "operation-a",
            "worker-a",
            2,
            "completed",
            "succeeded",
            Some(&hash),
            Some(r#"{"completed":true}"#),
            None,
            None,
            "2026-07-26T00:00:00Z",
        )
        .expect("atomic finish"));
        let connection = Connection::open(&path).expect("verify");
        let states: (String, String) = connection
            .query_row(
                "SELECT m.state, o.state FROM compute_workspace_materializations m
                 JOIN compute_workspace_operations o USING(operation_id)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("states");
        assert_eq!(states, ("completed".to_string(), "succeeded".to_string()));
        drop(connection);
        fs::remove_file(path).expect("cleanup");
    }
}
