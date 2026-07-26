use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceAttachmentSnapshot {
    pub workspace_id: String,
    pub client_id: String,
    pub address_or_cidr: String,
    pub mode: String,
    pub export_options_json: String,
    pub state: String,
}

pub fn list_workspace_attachments(
    live_sqlite_path: &Path,
) -> rusqlite::Result<Vec<WorkspaceAttachmentSnapshot>> {
    let connection = Connection::open(live_sqlite_path)?;
    let mut statement = connection.prepare(
        "SELECT workspace_id, client_id, address_or_cidr, mode,
                export_options_json, state
         FROM compute_workspace_attachments
         WHERE state <> 'detached'
         ORDER BY workspace_id, client_id",
    )?;
    let attachments = statement
        .query_map([], |row| {
            Ok(WorkspaceAttachmentSnapshot {
                workspace_id: row.get(0)?,
                client_id: row.get(1)?,
                address_or_cidr: row.get(2)?,
                mode: row.get(3)?,
                export_options_json: row.get(4)?,
                state: row.get(5)?,
            })
        })?
        .collect();
    attachments
}

pub fn publish_workspace_attachment_state(
    live_sqlite_path: &Path,
    workspace_id: &str,
    client_id: &str,
    expected_state: &str,
    state: &str,
    address_or_cidr: &str,
    export_options_json: &str,
    now_utc: &str,
) -> rusqlite::Result<bool> {
    let connection = Connection::open(live_sqlite_path)?;
    let changed = connection.execute(
        "UPDATE compute_workspace_attachments
         SET state = ?1, address_or_cidr = ?2, export_options_json = ?3,
             attached_at_utc = CASE WHEN ?1 = 'attached' THEN COALESCE(attached_at_utc, ?4)
                                    ELSE attached_at_utc END,
             detached_at_utc = CASE WHEN ?1 = 'detached' THEN ?4 ELSE NULL END
         WHERE workspace_id = ?5 AND client_id = ?6 AND state = ?7",
        params![
            state,
            address_or_cidr,
            export_options_json,
            now_utc,
            workspace_id,
            client_id,
            expected_state
        ],
    )?;
    Ok(changed == 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn attachment_state_publish_is_compare_and_set_and_restart_readable() {
        let path = std::env::temp_dir().join(format!(
            "dasobjectstore-workspace-attachments-{}-{}.sqlite",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let connection = Connection::open(&path).expect("database");
        connection
            .execute_batch(
                "CREATE TABLE compute_workspace_attachments (
                    workspace_id TEXT NOT NULL,
                    client_id TEXT NOT NULL,
                    address_or_cidr TEXT NOT NULL,
                    mode TEXT NOT NULL,
                    export_options_json TEXT NOT NULL,
                    state TEXT NOT NULL,
                    attached_at_utc TEXT,
                    detached_at_utc TEXT,
                    PRIMARY KEY (workspace_id, client_id)
                 );
                 INSERT INTO compute_workspace_attachments VALUES
                    ('workspace-a', 'compute-a', '', 'read_write', '{}',
                     'requested', NULL, NULL);",
            )
            .expect("fixture");
        drop(connection);

        assert!(publish_workspace_attachment_state(
            &path,
            "workspace-a",
            "compute-a",
            "requested",
            "attached",
            "192.168.1.48",
            r#"{"root_squash":true}"#,
            "2026-07-26T00:00:00Z",
        )
        .expect("publish"));
        assert!(!publish_workspace_attachment_state(
            &path,
            "workspace-a",
            "compute-a",
            "requested",
            "attached",
            "192.168.1.48",
            r#"{"root_squash":true}"#,
            "2026-07-26T00:00:00Z",
        )
        .expect("stale replay"));
        let rows = list_workspace_attachments(&path).expect("restart read");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].state, "attached");
        assert_eq!(rows[0].address_or_cidr, "192.168.1.48");
        fs::remove_file(path).expect("cleanup");
    }
}
