use crate::schema::LIVE_SCHEMA_SQL;
use dasobjectstore_core::workspace::WorkspaceOperationKind;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Component, Path};
use std::time::Duration;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspacePromotionMemberRequest {
    pub source_relative_path: String,
    pub object_id: String,
    pub object_type: String,
    pub required: bool,
    pub size_bytes: u64,
    pub sha256: String,
    pub parent_object_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RegisterWorkspacePromotion {
    pub promotion_id: String,
    pub workspace_id: String,
    pub operation_id: String,
    pub checkpoint_id: String,
    pub target_store_id: String,
    pub manifest_digest: String,
    pub created_at_utc: String,
    pub members: Vec<WorkspacePromotionMemberRequest>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspacePromotionMemberSnapshot {
    pub source_relative_path: String,
    pub object_id: String,
    pub object_type: String,
    pub required: bool,
    pub size_bytes: u64,
    pub sha256: String,
    pub state: String,
    pub accepted_at_utc: Option<String>,
    pub parent_object_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspacePromotionSnapshot {
    pub promotion_id: String,
    pub workspace_id: String,
    pub operation_id: String,
    pub checkpoint_id: String,
    pub target_store_id: String,
    pub manifest_digest: String,
    pub state: String,
    pub members: Vec<WorkspacePromotionMemberSnapshot>,
}

pub fn workspace_promotion_manifest_digest(
    workspace_id: &str,
    checkpoint_id: &str,
    target_store_id: &str,
    members: &[WorkspacePromotionMemberRequest],
) -> String {
    let mut digest = Sha256::new();
    for value in [workspace_id, checkpoint_id, target_store_id] {
        digest.update(value.as_bytes());
        digest.update([0]);
    }
    for member in members {
        digest.update(member.source_relative_path.as_bytes());
        digest.update([0]);
        digest.update(member.object_id.as_bytes());
        digest.update([0]);
        digest.update(member.object_type.as_bytes());
        digest.update([member.required as u8]);
        digest.update(member.size_bytes.to_be_bytes());
        digest.update(normalize_sha256(&member.sha256).as_bytes());
        digest.update([0]);
        for parent in &member.parent_object_ids {
            digest.update(parent.as_bytes());
            digest.update([0]);
        }
        digest.update([0xff]);
    }
    format!("sha256:{:x}", digest.finalize())
}

pub fn register_workspace_promotion(
    live_sqlite_path: &Path,
    request: &RegisterWorkspacePromotion,
) -> rusqlite::Result<WorkspacePromotionSnapshot> {
    validate_request(request)?;
    let mut connection = open(live_sqlite_path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if let Some(existing) = read_promotion(&transaction, &request.promotion_id)? {
        if promotion_matches(&existing, request) {
            transaction.commit()?;
            return Ok(existing);
        }
        return Err(rusqlite::Error::InvalidQuery);
    }
    let expected_digest = workspace_promotion_manifest_digest(
        &request.workspace_id,
        &request.checkpoint_id,
        &request.target_store_id,
        &request.members,
    );
    if normalize_sha256(&request.manifest_digest) != expected_digest {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let workspace: Option<(String, Option<String>)> = transaction
        .query_row(
            "SELECT state, promotion_store_id FROM compute_workspaces WHERE workspace_id = ?1",
            [&request.workspace_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if !workspace.is_some_and(|(state, target)| {
        state == "ready" && target.as_deref() == Some(request.target_store_id.as_str())
    }) || has_active_attachment(&transaction, &request.workspace_id)?
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let operation: Option<(String, String, Option<i64>, Option<i64>)> = transaction
        .query_row(
            "SELECT operation_kind, state, total_bytes, total_units
             FROM compute_workspace_operations
             WHERE operation_id = ?1 AND workspace_id = ?2",
            params![request.operation_id, request.workspace_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let total_bytes = request
        .members
        .iter()
        .try_fold(0_u64, |total, member| total.checked_add(member.size_bytes))
        .ok_or(rusqlite::Error::InvalidQuery)?;
    if operation
        != Some((
            WorkspaceOperationKind::Promote.as_str().to_string(),
            "queued".to_string(),
            Some(to_i64(total_bytes)?),
            Some(to_i64(request.members.len() as u64)?),
        ))
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    if !transaction.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM compute_workspace_checkpoints
             WHERE checkpoint_id = ?1 AND workspace_id = ?2
         )",
        params![request.checkpoint_id, request.workspace_id],
        |row| row.get::<_, bool>(0),
    )? || !transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM stores WHERE store_id = ?1)",
        [&request.target_store_id],
        |row| row.get::<_, bool>(0),
    )? {
        return Err(rusqlite::Error::InvalidQuery);
    }
    for member in &request.members {
        let evidence: Option<(i64, String)> = transaction
            .query_row(
                "SELECT size_bytes, sha256
                 FROM compute_workspace_checkpoint_members
                 WHERE checkpoint_id = ?1 AND workspace_relative_path = ?2",
                params![request.checkpoint_id, member.source_relative_path],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if !evidence.is_some_and(|(size, sha256)| {
            size == member.size_bytes as i64
                && normalize_sha256(&sha256) == normalize_sha256(&member.sha256)
        }) || member.parent_object_ids.iter().any(|parent| {
            !transaction
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM objects WHERE object_id = ?1)",
                    [parent],
                    |row| row.get::<_, bool>(0),
                )
                .unwrap_or(false)
        }) {
            return Err(rusqlite::Error::InvalidQuery);
        }
    }
    transaction.execute(
        "INSERT INTO compute_workspace_promotions (
             promotion_id, workspace_id, operation_id, checkpoint_id,
             target_store_id, manifest_digest, state, created_at_utc, updated_at_utc
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'registered', ?7, ?7)",
        params![
            request.promotion_id,
            request.workspace_id,
            request.operation_id,
            request.checkpoint_id,
            request.target_store_id,
            request.manifest_digest,
            request.created_at_utc,
        ],
    )?;
    for member in &request.members {
        transaction.execute(
            "INSERT INTO compute_workspace_promotion_members (
                 promotion_id, source_relative_path, object_id, object_type,
                 required, size_bytes, sha256, state
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending')",
            params![
                request.promotion_id,
                member.source_relative_path,
                member.object_id,
                member.object_type,
                member.required,
                to_i64(member.size_bytes)?,
                normalize_sha256(&member.sha256),
            ],
        )?;
        for parent in &member.parent_object_ids {
            transaction.execute(
                "INSERT INTO compute_workspace_promotion_lineage (
                     promotion_id, object_id, parent_object_id
                 ) VALUES (?1, ?2, ?3)",
                params![request.promotion_id, member.object_id, parent],
            )?;
        }
    }
    transaction.execute(
        "UPDATE compute_workspaces
         SET state = 'promotion_pending', generation = generation + 1,
             updated_at_utc = ?1
         WHERE workspace_id = ?2 AND state = 'ready'",
        params![request.created_at_utc, request.workspace_id],
    )?;
    let snapshot = read_promotion(&transaction, &request.promotion_id)?
        .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    transaction.commit()?;
    Ok(snapshot)
}

pub fn list_active_workspace_promotions(
    live_sqlite_path: &Path,
) -> rusqlite::Result<Vec<WorkspacePromotionSnapshot>> {
    let connection = open(live_sqlite_path)?;
    let mut statement = connection.prepare(
        "SELECT promotion_id FROM compute_workspace_promotions
         WHERE state NOT IN ('completed', 'failed', 'needs_review', 'cancelled')
         ORDER BY created_at_utc, promotion_id",
    )?;
    let ids = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    ids.into_iter()
        .map(|id| read_promotion(&connection, &id)?.ok_or(rusqlite::Error::QueryReturnedNoRows))
        .collect()
}

pub fn accept_workspace_promotion_member(
    live_sqlite_path: &Path,
    promotion_id: &str,
    object_id: &str,
    accepted_at_utc: &str,
) -> rusqlite::Result<bool> {
    let mut connection = open(live_sqlite_path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let evidence: Option<(String, i64, String, String, String)> = transaction
        .query_row(
            "SELECT p.target_store_id, m.size_bytes, m.sha256, m.state, o.state
             FROM compute_workspace_promotions p
             JOIN compute_workspace_promotion_members m USING (promotion_id)
             JOIN objects o ON o.object_id = m.object_id
             JOIN destage_queue d ON d.object_id = m.object_id
             WHERE p.promotion_id = ?1 AND m.object_id = ?2
               AND o.store_id = p.target_store_id
               AND o.size_bytes = m.size_bytes
               AND o.content_hash = m.sha256
               AND d.state NOT IN ('needs_review', 'cancelled')",
            params![promotion_id, object_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()?;
    let Some((_store, _size, _sha256, state, _object_state)) = evidence else {
        return Err(rusqlite::Error::InvalidQuery);
    };
    if state == "accepted" {
        transaction.commit()?;
        return Ok(false);
    }
    if state != "pending" && state != "staging" {
        return Err(rusqlite::Error::InvalidQuery);
    }
    transaction.execute(
        "UPDATE compute_workspace_promotion_members
         SET state = 'accepted', accepted_at_utc = ?1
         WHERE promotion_id = ?2 AND object_id = ?3
           AND state IN ('pending', 'staging')",
        params![accepted_at_utc, promotion_id, object_id],
    )?;
    transaction.execute(
        "UPDATE compute_workspace_promotions SET state = 'publishing', updated_at_utc = ?1
         WHERE promotion_id = ?2 AND state = 'registered'",
        params![accepted_at_utc, promotion_id],
    )?;
    transaction.commit()?;
    Ok(true)
}

pub fn complete_workspace_promotion(
    live_sqlite_path: &Path,
    promotion_id: &str,
    lease_owner: &str,
    expected_operation_generation: u64,
    completed_at_utc: &str,
) -> rusqlite::Result<bool> {
    let mut connection = open(live_sqlite_path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let authority: Option<(String, String, String, i64)> = transaction
        .query_row(
            "SELECT p.workspace_id, p.operation_id, p.state,
                    SUM(CASE WHEN m.required = 1 AND m.state != 'accepted' THEN 1 ELSE 0 END)
             FROM compute_workspace_promotions p
             JOIN compute_workspace_promotion_members m USING (promotion_id)
             WHERE p.promotion_id = ?1
             GROUP BY p.promotion_id",
            [promotion_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let Some((workspace_id, operation_id, state, incomplete)) = authority else {
        return Err(rusqlite::Error::InvalidQuery);
    };
    if state == "completed" {
        transaction.commit()?;
        return Ok(false);
    }
    if incomplete != 0 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let result_json = serde_json::json!({
        "schema_version": "dasobjectstore.workspace_promotion_result.v1",
        "promotion_id": promotion_id,
        "status": "completed",
    })
    .to_string();
    let changed = transaction.execute(
        "UPDATE compute_workspace_operations
         SET state = 'succeeded', stage = 'complete', result_json = ?1,
             failure_code = NULL, failure_message = NULL, lease_owner = NULL,
             lease_expires_at_utc = NULL, recovery_disposition = 'terminal',
             completed_bytes = (
                 SELECT SUM(size_bytes) FROM compute_workspace_promotion_members
                 WHERE promotion_id = ?6
             ),
             completed_units = (
                 SELECT COUNT(*) FROM compute_workspace_promotion_members
                 WHERE promotion_id = ?6
             ),
             generation = generation + 1, completed_at_utc = ?2, updated_at_utc = ?2
         WHERE operation_id = ?3 AND operation_kind = 'promote'
           AND state = 'running' AND lease_owner = ?4 AND generation = ?5",
        params![
            result_json,
            completed_at_utc,
            operation_id,
            lease_owner,
            to_i64(expected_operation_generation)?,
            promotion_id,
        ],
    )?;
    if changed != 1 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    transaction.execute(
        "UPDATE compute_workspace_promotions
         SET state = 'completed', updated_at_utc = ?1 WHERE promotion_id = ?2",
        params![completed_at_utc, promotion_id],
    )?;
    transaction.execute(
        "UPDATE compute_workspaces
         SET state = 'ready', generation = generation + 1, updated_at_utc = ?1
         WHERE workspace_id = ?2 AND state = 'promotion_pending'",
        params![completed_at_utc, workspace_id],
    )?;
    transaction.commit()?;
    Ok(true)
}

pub fn cancel_workspace_promotion(
    live_sqlite_path: &Path,
    promotion_id: &str,
    cancelled_at_utc: &str,
) -> rusqlite::Result<WorkspacePromotionSnapshot> {
    let mut connection = open(live_sqlite_path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current =
        read_promotion(&transaction, promotion_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    if matches!(current.state.as_str(), "cancelled" | "needs_review") {
        transaction.commit()?;
        return Ok(current);
    }
    let accepted = current
        .members
        .iter()
        .filter(|member| member.state == "accepted")
        .count();
    let (promotion_state, operation_state, workspace_state) = if accepted == 0 {
        ("cancelled", "cancelled", "ready")
    } else {
        ("needs_review", "needs_review", "promotion_pending")
    };
    transaction.execute(
        "UPDATE compute_workspace_promotions
         SET state = ?2, updated_at_utc = ?3 WHERE promotion_id = ?1",
        params![promotion_id, promotion_state, cancelled_at_utc],
    )?;
    transaction.execute(
        "UPDATE compute_workspace_operations
         SET state = ?2, cancellation_requested = 1,
             lease_owner = NULL, lease_expires_at_utc = NULL,
             recovery_disposition = 'terminal',
             failure_code = CASE WHEN ?2 = 'needs_review' THEN 'partial_promotion' ELSE NULL END,
             failure_message = CASE WHEN ?2 = 'needs_review'
                 THEN 'cancellation followed one or more immutable member publications'
                 ELSE NULL END,
             completed_at_utc = COALESCE(completed_at_utc, ?3),
             generation = generation + 1, updated_at_utc = ?3
         WHERE operation_id = ?1",
        params![current.operation_id, operation_state, cancelled_at_utc],
    )?;
    transaction.execute(
        "UPDATE compute_workspaces SET state = ?2, updated_at_utc = ?3
         WHERE workspace_id = ?1",
        params![current.workspace_id, workspace_state, cancelled_at_utc],
    )?;
    let result =
        read_promotion(&transaction, promotion_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    transaction.commit()?;
    Ok(result)
}

fn read_promotion(
    connection: &Connection,
    promotion_id: &str,
) -> rusqlite::Result<Option<WorkspacePromotionSnapshot>> {
    let header: Option<(String, String, String, String, String, String, String)> = connection
        .query_row(
            "SELECT promotion_id, workspace_id, operation_id, checkpoint_id,
                    target_store_id, manifest_digest, state
             FROM compute_workspace_promotions WHERE promotion_id = ?1",
            [promotion_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .optional()?;
    let Some(header) = header else {
        return Ok(None);
    };
    let mut statement = connection.prepare(
        "SELECT source_relative_path, object_id, object_type, required,
                size_bytes, sha256, state, accepted_at_utc
         FROM compute_workspace_promotion_members
         WHERE promotion_id = ?1 ORDER BY object_id",
    )?;
    let rows = statement
        .query_map([promotion_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, bool>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut members = Vec::with_capacity(rows.len());
    for row in rows {
        let mut lineage = connection.prepare(
            "SELECT parent_object_id FROM compute_workspace_promotion_lineage
             WHERE promotion_id = ?1 AND object_id = ?2 ORDER BY parent_object_id",
        )?;
        let parents = lineage
            .query_map(params![promotion_id, row.1], |parent| parent.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        members.push(WorkspacePromotionMemberSnapshot {
            source_relative_path: row.0,
            object_id: row.1,
            object_type: row.2,
            required: row.3,
            size_bytes: row.4 as u64,
            sha256: row.5,
            state: row.6,
            accepted_at_utc: row.7,
            parent_object_ids: parents,
        });
    }
    Ok(Some(WorkspacePromotionSnapshot {
        promotion_id: header.0,
        workspace_id: header.1,
        operation_id: header.2,
        checkpoint_id: header.3,
        target_store_id: header.4,
        manifest_digest: header.5,
        state: header.6,
        members,
    }))
}

fn open(path: &Path) -> rusqlite::Result<Connection> {
    let connection = Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    ensure_upgrade(&connection)?;
    Ok(connection)
}

fn ensure_upgrade(connection: &Connection) -> rusqlite::Result<()> {
    let has_checkpoint = connection
        .prepare("PRAGMA table_info(compute_workspace_promotions)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .any(|column| column == "checkpoint_id");
    if !has_checkpoint {
        connection.execute(
            "ALTER TABLE compute_workspace_promotions ADD COLUMN checkpoint_id TEXT",
            [],
        )?;
    }
    connection.execute(
        "INSERT INTO metadata_format_versions (artifact, major, minor, updated_at_utc)
         VALUES ('live_sqlite', 0, 12, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
         ON CONFLICT(artifact) DO UPDATE SET
             major = excluded.major,
             minor = excluded.minor,
             updated_at_utc = excluded.updated_at_utc
         WHERE metadata_format_versions.major < excluded.major
            OR (metadata_format_versions.major = excluded.major
                AND metadata_format_versions.minor < excluded.minor)",
        [],
    )?;
    Ok(())
}

fn has_active_attachment(
    transaction: &Transaction<'_>,
    workspace_id: &str,
) -> rusqlite::Result<bool> {
    transaction.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM compute_workspace_attachments
             WHERE workspace_id = ?1
               AND state IN ('requested', 'attached', 'detach_requested')
         )",
        [workspace_id],
        |row| row.get(0),
    )
}

fn validate_request(request: &RegisterWorkspacePromotion) -> rusqlite::Result<()> {
    if !valid_identity(&request.promotion_id)
        || !valid_identity(&request.workspace_id)
        || !valid_identity(&request.operation_id)
        || !valid_identity(&request.checkpoint_id)
        || request.target_store_id.trim().is_empty()
        || !valid_sha256(&request.manifest_digest)
        || request.members.is_empty()
        || request.members.len() > 4096
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let mut objects = BTreeSet::new();
    for member in &request.members {
        if !valid_relative_path(&member.source_relative_path)
            || member.object_id.trim().is_empty()
            || member.object_type.trim().is_empty()
            || member.size_bytes == 0
            || member.size_bytes > i64::MAX as u64
            || !valid_sha256(&member.sha256)
            || !member.required
            || !objects.insert(&member.object_id)
        {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let mut parents = BTreeSet::new();
        if member
            .parent_object_ids
            .iter()
            .any(|parent| parent.trim().is_empty() || !parents.insert(parent))
        {
            return Err(rusqlite::Error::InvalidQuery);
        }
    }
    Ok(())
}

fn promotion_matches(
    snapshot: &WorkspacePromotionSnapshot,
    request: &RegisterWorkspacePromotion,
) -> bool {
    snapshot.workspace_id == request.workspace_id
        && snapshot.operation_id == request.operation_id
        && snapshot.checkpoint_id == request.checkpoint_id
        && snapshot.target_store_id == request.target_store_id
        && normalize_sha256(&snapshot.manifest_digest) == normalize_sha256(&request.manifest_digest)
        && snapshot.members.len() == request.members.len()
}

fn valid_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
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
        accept_workspace_promotion_member, cancel_workspace_promotion,
        complete_workspace_promotion, register_workspace_promotion,
        workspace_promotion_manifest_digest, RegisterWorkspacePromotion,
        WorkspacePromotionMemberRequest,
    };
    use crate::schema::LIVE_SCHEMA_SQL;
    use rusqlite::Connection;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn manifest_digest_commits_lineage_and_member_order() {
        let member = WorkspacePromotionMemberRequest {
            source_relative_path: "outputs/result.bin".to_string(),
            object_id: "store/result.bin".to_string(),
            object_type: "naive".to_string(),
            required: true,
            size_bytes: 10,
            sha256: format!("sha256:{}", "a".repeat(64)),
            parent_object_ids: vec!["store/input.bin".to_string()],
        };
        let first = workspace_promotion_manifest_digest(
            "workspace-a",
            "checkpoint-a",
            "store",
            std::slice::from_ref(&member),
        );
        let mut changed = member;
        changed.parent_object_ids = vec!["store/other.bin".to_string()];
        assert_ne!(
            first,
            workspace_promotion_manifest_digest("workspace-a", "checkpoint-a", "store", &[changed])
        );
    }

    #[test]
    fn registration_is_checkpoint_bound_atomic_and_idempotent() {
        let database = fixture("register");
        let member = member();
        let digest = workspace_promotion_manifest_digest(
            "workspace-a",
            "checkpoint-a",
            "store-out",
            std::slice::from_ref(&member),
        );
        let request = RegisterWorkspacePromotion {
            promotion_id: "promotion-a".to_string(),
            workspace_id: "workspace-a".to_string(),
            operation_id: "operation-a".to_string(),
            checkpoint_id: "checkpoint-a".to_string(),
            target_store_id: "store-out".to_string(),
            manifest_digest: digest,
            created_at_utc: "2026-07-26T12:00:00Z".to_string(),
            members: vec![member],
        };
        let first = register_workspace_promotion(&database, &request).expect("promotion registers");
        assert_eq!(first.members.len(), 1);
        assert_eq!(
            register_workspace_promotion(&database, &request).expect("exact retry"),
            first
        );
        let connection = Connection::open(&database).expect("open");
        assert_eq!(
            connection
                .query_row(
                    "SELECT state FROM compute_workspaces WHERE workspace_id = 'workspace-a'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .expect("state"),
            "promotion_pending"
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM compute_workspace_promotion_lineage",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .expect("lineage"),
            1
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT major, minor FROM metadata_format_versions
                     WHERE artifact = 'live_sqlite'",
                    [],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
                )
                .expect("format version"),
            (0, 12)
        );
        fs::remove_file(database).expect("cleanup");
    }

    #[test]
    fn changed_manifest_does_not_leave_partial_promotion() {
        let database = fixture("changed");
        let mut member = member();
        member.sha256 = format!("sha256:{}", "c".repeat(64));
        let request = RegisterWorkspacePromotion {
            promotion_id: "promotion-a".to_string(),
            workspace_id: "workspace-a".to_string(),
            operation_id: "operation-a".to_string(),
            checkpoint_id: "checkpoint-a".to_string(),
            target_store_id: "store-out".to_string(),
            manifest_digest: workspace_promotion_manifest_digest(
                "workspace-a",
                "checkpoint-a",
                "store-out",
                std::slice::from_ref(&member),
            ),
            created_at_utc: "2026-07-26T12:00:00Z".to_string(),
            members: vec![member],
        };
        assert!(register_workspace_promotion(&database, &request).is_err());
        let connection = Connection::open(&database).expect("open");
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM compute_workspace_promotions",
                    [],
                    |row| { row.get::<_, i64>(0) }
                )
                .expect("count"),
            0
        );
        fs::remove_file(database).expect("cleanup");
    }

    #[test]
    fn completion_requires_catalogue_and_destage_evidence_and_is_atomic() {
        let database = fixture("complete");
        let member = member();
        let request = RegisterWorkspacePromotion {
            promotion_id: "promotion-a".to_string(),
            workspace_id: "workspace-a".to_string(),
            operation_id: "operation-a".to_string(),
            checkpoint_id: "checkpoint-a".to_string(),
            target_store_id: "store-out".to_string(),
            manifest_digest: workspace_promotion_manifest_digest(
                "workspace-a",
                "checkpoint-a",
                "store-out",
                std::slice::from_ref(&member),
            ),
            created_at_utc: "2026-07-26T12:00:00Z".to_string(),
            members: vec![member],
        };
        register_workspace_promotion(&database, &request).expect("register");
        assert!(accept_workspace_promotion_member(
            &database,
            "promotion-a",
            "store-out/result.bin",
            "2026-07-26T12:01:00Z"
        )
        .is_err());
        let connection = Connection::open(&database).expect("open");
        connection
            .execute_batch(
                "INSERT INTO objects (
                     object_id, store_id, state, size_bytes, content_hash,
                     created_at_utc, updated_at_utc
                 ) VALUES (
                     'store-out/result.bin', 'store-out', 'PlacementPlanned', 10,
                     'sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
                     'now', 'now'
                 );
                 INSERT INTO destage_queue (
                     destage_job_id, store_id, object_id, state,
                     expected_size_bytes, content_hash_algorithm, content_hash,
                     acknowledgement_policy, required_copy_count, priority,
                     max_attempts, created_at_utc, updated_at_utc
                 ) VALUES (
                     'destage-a', 'store-out', 'store-out/result.bin', 'queued_for_hdd',
                     10, 'sha256',
                     'sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
                     'after_ssd_ingest', 1, 0, 8, 'now', 'now'
                 );
                 UPDATE compute_workspace_operations
                 SET state = 'running', lease_owner = 'worker-a',
                     lease_expires_at_utc = 'later', generation = 2
                 WHERE operation_id = 'operation-a';",
            )
            .expect("publication evidence");
        assert!(accept_workspace_promotion_member(
            &database,
            "promotion-a",
            "store-out/result.bin",
            "2026-07-26T12:01:00Z"
        )
        .expect("accepts"));
        assert!(complete_workspace_promotion(
            &database,
            "promotion-a",
            "worker-a",
            2,
            "2026-07-26T12:02:00Z"
        )
        .expect("completes"));
        assert_eq!(
            connection
                .query_row(
                    "SELECT p.state, o.state, w.state
                     FROM compute_workspace_promotions p
                     JOIN compute_workspace_operations o USING (operation_id)
                     JOIN compute_workspaces w USING (workspace_id)",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    }
                )
                .expect("states"),
            (
                "completed".to_string(),
                "succeeded".to_string(),
                "ready".to_string()
            )
        );
        fs::remove_file(database).expect("cleanup");
    }

    #[test]
    fn cancellation_before_publication_restores_ready_workspace_atomically() {
        let database = fixture("cancel");
        let member = member();
        let request = RegisterWorkspacePromotion {
            promotion_id: "promotion-a".to_string(),
            workspace_id: "workspace-a".to_string(),
            operation_id: "operation-a".to_string(),
            checkpoint_id: "checkpoint-a".to_string(),
            target_store_id: "store-out".to_string(),
            manifest_digest: workspace_promotion_manifest_digest(
                "workspace-a",
                "checkpoint-a",
                "store-out",
                std::slice::from_ref(&member),
            ),
            created_at_utc: "2026-07-26T12:00:00Z".to_string(),
            members: vec![member],
        };
        register_workspace_promotion(&database, &request).expect("register");
        let result = cancel_workspace_promotion(&database, "promotion-a", "2026-07-26T12:01:00Z")
            .expect("cancel");
        assert_eq!(result.state, "cancelled");
        let connection = Connection::open(&database).expect("open");
        assert_eq!(
            connection
                .query_row(
                    "SELECT state FROM compute_workspaces WHERE workspace_id = 'workspace-a'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .expect("workspace state"),
            "ready"
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT state FROM compute_workspace_operations
                     WHERE operation_id = 'operation-a'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .expect("operation state"),
            "cancelled"
        );
        fs::remove_file(database).expect("cleanup");
    }

    fn member() -> WorkspacePromotionMemberRequest {
        WorkspacePromotionMemberRequest {
            source_relative_path: "outputs/result.bin".to_string(),
            object_id: "store-out/result.bin".to_string(),
            object_type: "naive".to_string(),
            required: true,
            size_bytes: 10,
            sha256: format!("sha256:{}", "b".repeat(64)),
            parent_object_ids: vec!["store-in/input.bin".to_string()],
        }
    }

    fn fixture(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "dasobjectstore-workspace-promotion-{name}-{}.sqlite",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        let connection = Connection::open(&path).expect("open");
        connection.execute_batch(LIVE_SCHEMA_SQL).expect("schema");
        connection
            .execute_batch(
                "INSERT INTO pools VALUES ('pool-a', 'Clean', 'now', 'now');
                 INSERT INTO disks (
                     disk_id, pool_id, role, state, size_bytes,
                     created_at_utc, updated_at_utc
                 ) VALUES ('disk-a', 'pool-a', 'Data', 'Healthy', 1000, 'now', 'now');
                 INSERT INTO stores (
                     store_id, pool_id, class, policy_json, created_at_utc, updated_at_utc
                 ) VALUES
                     ('store-in', 'pool-a', 'GeneratedData', '{}', 'now', 'now'),
                     ('store-out', 'pool-a', 'GeneratedData', '{}', 'now', 'now');
                 INSERT INTO objects (
                     object_id, store_id, state, size_bytes, content_hash,
                     created_at_utc, updated_at_utc
                 ) VALUES (
                     'store-in/input.bin', 'store-in', 'Available', 4, 'sha256:a',
                     'now', 'now'
                 );
                 INSERT INTO compute_workspaces (
                     workspace_id, schema_version, request_id, request_digest, pool_id,
                     promotion_store_id, state, owner, project, purpose,
                     requested_capacity_bytes, reserved_capacity_bytes, quota_bytes,
                     minimum_free_bytes_per_disk, aggregation_provider,
                     aggregate_mount_identity, close_cleanup_policy_json, generation,
                     created_at_utc, updated_at_utc, expires_at_utc
                 ) VALUES (
                     'workspace-a', 1, 'request-a', 'sha256:a', 'pool-a',
                     'store-out', 'ready', 'owner', 'project', 'tests',
                     1000, 1000, 1000, 1, 'mergerfs', 'workspace-a', '{}', 1,
                     'now', 'now', 'later'
                 );
                 INSERT INTO compute_workspace_branches (
                     workspace_id, disk_id, branch_id, branch_relative_path,
                     project_id, project_quota_bytes, reserved_bytes, state,
                     created_at_utc
                 ) VALUES (
                     'workspace-a', 'disk-a', 'branch-a', 'workspace-a',
                     1000, 1000, 1000, 'ready', 'now'
                 );
                 INSERT INTO compute_workspace_operations (
                     operation_id, workspace_id, operation_kind, request_id,
                     request_digest, state, stage, total_bytes, total_units, max_attempts,
                     recovery_disposition, created_at_utc, updated_at_utc
                 ) VALUES (
                     'operation-a', 'workspace-a', 'promote', 'promotion-request',
                     'sha256:a', 'queued', 'registered', 10, 1, 3, 'resume_checkpoint',
                     'now', 'now'
                 );
                 INSERT INTO compute_workspace_checkpoints (
                     checkpoint_id, workspace_id, relative_prefix, role,
                     reproducibility_class, logical_bytes, checkpoint_manifest_id,
                     removable_after_promotion, created_at_utc, updated_at_utc,
                     retention_deadline_utc
                 ) VALUES (
                     'checkpoint-a', 'workspace-a', 'outputs', 'result', 'derived',
                     10, 'sha256:b', 1, 'now', 'now', 'later'
                 );
                 INSERT INTO compute_workspace_checkpoint_members (
                     checkpoint_id, workspace_relative_path, size_bytes, sha256
                 ) VALUES (
                     'checkpoint-a', 'outputs/result.bin', 10,
                     'sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'
                 );",
            )
            .expect("fixture");
        path
    }
}
