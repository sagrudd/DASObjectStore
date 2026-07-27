//! One-time logical identity migration evidence inspection.

use crate::logical_identity::{
    LogicalIdentityError, LOGICAL_IDENTITY_MIGRATION_ID, LOGICAL_IDENTITY_MIGRATION_NAME,
};
use rusqlite::{Connection, OptionalExtension};
use std::path::Path;

/// Returns whether the appliance-wide logical identity backfill has committed.
///
/// Startup callers use this evidence to avoid replaying a full-catalogue
/// inspection. A conflicting migration identifier remains a hard failure.
pub fn logical_identity_migration_applied(
    path: impl AsRef<Path>,
) -> Result<bool, LogicalIdentityError> {
    let connection = Connection::open(path)?;
    let table_exists = connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM sqlite_master
            WHERE type='table' AND name='metadata_migrations'
        )",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if !table_exists {
        return Ok(false);
    }
    let migration_name = connection
        .query_row(
            "SELECT name FROM metadata_migrations WHERE migration_id=?1",
            [LOGICAL_IDENTITY_MIGRATION_ID],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    match migration_name.as_deref() {
        None => Ok(false),
        Some(LOGICAL_IDENTITY_MIGRATION_NAME) => Ok(true),
        Some(_) => Err(LogicalIdentityError::MigrationEvidenceConflict(
            LOGICAL_IDENTITY_MIGRATION_ID,
        )),
    }
}
