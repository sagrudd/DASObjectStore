use crate::format::{FormatVersion, MetadataArtifact};

pub const LIVE_SCHEMA_FORMAT_VERSION: FormatVersion =
    FormatVersion::new(MetadataArtifact::LiveSqlite, 0, 8);

pub const LIVE_SCHEMA_SQL: &str = r#"
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS metadata_format_versions (
    artifact TEXT PRIMARY KEY NOT NULL,
    major INTEGER NOT NULL CHECK (major >= 0),
    minor INTEGER NOT NULL CHECK (minor >= 0),
    updated_at_utc TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS metadata_migrations (
    migration_id INTEGER PRIMARY KEY NOT NULL,
    name TEXT NOT NULL UNIQUE,
    applied_at_utc TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS pools (
    pool_id TEXT PRIMARY KEY NOT NULL,
    state TEXT NOT NULL,
    created_at_utc TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS pool_state_markers (
    marker_id INTEGER PRIMARY KEY NOT NULL,
    pool_id TEXT NOT NULL REFERENCES pools(pool_id),
    marker_kind TEXT NOT NULL,
    previous_state TEXT,
    next_state TEXT NOT NULL,
    import_mode TEXT,
    reason TEXT,
    recorded_at_utc TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS disks (
    disk_id TEXT PRIMARY KEY NOT NULL,
    pool_id TEXT NOT NULL REFERENCES pools(pool_id),
    role TEXT NOT NULL,
    state TEXT NOT NULL,
    size_bytes INTEGER,
    serial_hint TEXT,
    model_hint TEXT,
    enclosure_topology_path TEXT,
    created_at_utc TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS stores (
    store_id TEXT PRIMARY KEY NOT NULL,
    pool_id TEXT NOT NULL REFERENCES pools(pool_id),
    class TEXT NOT NULL,
    policy_json TEXT NOT NULL,
    created_at_utc TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS objects (
    object_id TEXT PRIMARY KEY NOT NULL,
    store_id TEXT NOT NULL REFERENCES stores(store_id),
    object_type TEXT NOT NULL DEFAULT 'naive',
    state TEXT NOT NULL,
    size_bytes INTEGER,
    content_hash TEXT,
    created_at_utc TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS placements (
    placement_id TEXT PRIMARY KEY NOT NULL,
    object_id TEXT NOT NULL REFERENCES objects(object_id),
    disk_id TEXT NOT NULL REFERENCES disks(disk_id),
    relative_path TEXT NOT NULL,
    content_hash TEXT,
    verified_at_utc TEXT,
    created_at_utc TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS ingest_jobs (
    ingest_job_id TEXT PRIMARY KEY NOT NULL,
    store_id TEXT NOT NULL REFERENCES stores(store_id),
    object_id TEXT REFERENCES objects(object_id),
    object_type TEXT NOT NULL DEFAULT 'naive',
    state TEXT NOT NULL,
    ingest_mode TEXT NOT NULL,
    acknowledgement_policy TEXT NOT NULL,
    priority INTEGER NOT NULL DEFAULT 0,
    staging_path TEXT NOT NULL,
    expected_size_bytes INTEGER,
    received_bytes INTEGER NOT NULL DEFAULT 0 CHECK (received_bytes >= 0),
    content_hash TEXT,
    content_hash_algorithm TEXT,
    failure_message TEXT,
    created_at_utc TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_ingest_jobs_store_state_priority
ON ingest_jobs (store_id, state, priority DESC, created_at_utc);

CREATE INDEX IF NOT EXISTS idx_ingest_jobs_object
ON ingest_jobs (object_id);

-- Durable SSD acknowledgement and asynchronous HDD settlement are kept
-- separate from the legacy HDD-only placements table.  One row is the
-- authoritative managed SSD copy and one row is the idempotent unit of work.
CREATE TABLE IF NOT EXISTS ssd_object_placements (
    object_id TEXT PRIMARY KEY NOT NULL REFERENCES objects(object_id),
    store_id TEXT NOT NULL REFERENCES stores(store_id),
    relative_path TEXT NOT NULL,
    size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
    content_hash_algorithm TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    verified_at_utc TEXT NOT NULL,
    eviction_eligible INTEGER NOT NULL DEFAULT 0 CHECK (eviction_eligible IN (0, 1)),
    evicted_at_utc TEXT,
    created_at_utc TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS destage_queue (
    destage_job_id TEXT PRIMARY KEY NOT NULL,
    store_id TEXT NOT NULL REFERENCES stores(store_id),
    object_id TEXT NOT NULL UNIQUE REFERENCES objects(object_id),
    state TEXT NOT NULL,
    expected_size_bytes INTEGER NOT NULL CHECK (expected_size_bytes >= 0),
    content_hash_algorithm TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    acknowledgement_policy TEXT NOT NULL,
    required_copy_count INTEGER NOT NULL CHECK (required_copy_count > 0),
    verified_copy_count INTEGER NOT NULL DEFAULT 0 CHECK (verified_copy_count >= 0),
    priority INTEGER NOT NULL DEFAULT 0,
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    max_attempts INTEGER NOT NULL DEFAULT 8 CHECK (max_attempts > 0),
    last_error TEXT,
    next_retry_at_utc TEXT,
    lease_owner TEXT,
    lease_expires_at_utc TEXT,
    cancellation_requested INTEGER NOT NULL DEFAULT 0 CHECK (cancellation_requested IN (0, 1)),
    created_at_utc TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_destage_queue_runnable
ON destage_queue (state, next_retry_at_utc, priority DESC, created_at_utc);

CREATE INDEX IF NOT EXISTS idx_destage_queue_store
ON destage_queue (store_id, state, priority DESC, created_at_utc);

-- Stable external S3 identity is distinct from the daemon's internal object
-- identity. The binding is committed atomically with native SSD acceptance so
-- catalogue visibility and S3 visibility cannot diverge.
CREATE TABLE IF NOT EXISTS s3_object_bindings (
    store_id TEXT NOT NULL REFERENCES stores(store_id),
    object_key TEXT NOT NULL,
    object_version INTEGER NOT NULL CHECK (object_version > 0),
    object_id TEXT NOT NULL UNIQUE REFERENCES objects(object_id),
    size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
    content_hash_algorithm TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    created_at_utc TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL,
    PRIMARY KEY (store_id, object_key, object_version)
);

CREATE INDEX IF NOT EXISTS idx_s3_object_bindings_list
ON s3_object_bindings (store_id, object_key, object_version);

-- Profile-neutral catalogue handoffs are deliberately isolated from the
-- legacy objects/placements tables.  The latter derive appliance paths from
-- disk rows; these rows retain the portable namespace, transaction, and
-- version contract until a daemon-owned adapter performs a checked handoff.
CREATE TABLE IF NOT EXISTS profile_catalogue_transactions (
    transaction_id TEXT PRIMARY KEY NOT NULL,
    profile_namespace TEXT NOT NULL,
    store_id TEXT NOT NULL REFERENCES stores(store_id),
    schema_version INTEGER NOT NULL CHECK (schema_version > 0),
    source_retained INTEGER NOT NULL CHECK (source_retained IN (0, 1)),
    catalogue_json TEXT NOT NULL,
    committed_at_utc TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS profile_catalogue_objects (
    profile_namespace TEXT NOT NULL,
    store_id TEXT NOT NULL REFERENCES stores(store_id),
    object_id TEXT NOT NULL,
    object_version INTEGER NOT NULL CHECK (object_version > 0),
    transaction_id TEXT NOT NULL REFERENCES profile_catalogue_transactions(transaction_id),
    object_json TEXT NOT NULL,
    committed_at_utc TEXT NOT NULL,
    PRIMARY KEY (profile_namespace, store_id, object_id, object_version)
);

CREATE INDEX IF NOT EXISTS idx_profile_catalogue_objects_transaction
ON profile_catalogue_objects (transaction_id);

-- Mutable compute workspaces are deliberately separate from immutable objects
-- and placements. Capacity is reserved per disk in the same transaction as
-- the workspace row, while provider paths remain daemon-private.
CREATE TABLE IF NOT EXISTS compute_workspaces (
    workspace_id TEXT PRIMARY KEY NOT NULL,
    schema_version INTEGER NOT NULL CHECK (schema_version > 0),
    request_id TEXT NOT NULL UNIQUE,
    request_digest TEXT NOT NULL,
    pool_id TEXT NOT NULL REFERENCES pools(pool_id),
    promotion_store_id TEXT REFERENCES stores(store_id),
    state TEXT NOT NULL,
    owner TEXT NOT NULL,
    project TEXT NOT NULL,
    purpose TEXT NOT NULL,
    workflow_id TEXT,
    workflow_run_id TEXT,
    repository_revision TEXT,
    requested_capacity_bytes INTEGER NOT NULL CHECK (requested_capacity_bytes > 0),
    reserved_capacity_bytes INTEGER NOT NULL CHECK (reserved_capacity_bytes > 0),
    quota_bytes INTEGER NOT NULL CHECK (quota_bytes > 0),
    bytes_written INTEGER NOT NULL DEFAULT 0 CHECK (bytes_written >= 0),
    bytes_reclaimable INTEGER NOT NULL DEFAULT 0 CHECK (bytes_reclaimable >= 0),
    minimum_free_bytes_per_disk INTEGER NOT NULL CHECK (minimum_free_bytes_per_disk >= 0),
    aggregation_provider TEXT NOT NULL,
    aggregate_mount_identity TEXT,
    close_cleanup_policy_json TEXT NOT NULL,
    failure_reason TEXT,
    generation INTEGER NOT NULL DEFAULT 1 CHECK (generation > 0),
    created_at_utc TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL,
    expires_at_utc TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_compute_workspaces_pool_state
ON compute_workspaces (pool_id, state, expires_at_utc);

CREATE TABLE IF NOT EXISTS compute_workspace_branches (
    workspace_id TEXT NOT NULL REFERENCES compute_workspaces(workspace_id) ON DELETE CASCADE,
    disk_id TEXT NOT NULL REFERENCES disks(disk_id),
    branch_id TEXT NOT NULL,
    branch_relative_path TEXT,
    reserved_bytes INTEGER NOT NULL CHECK (reserved_bytes > 0),
    state TEXT NOT NULL,
    created_at_utc TEXT NOT NULL,
    released_at_utc TEXT,
    PRIMARY KEY (workspace_id, disk_id),
    UNIQUE (branch_id)
);

CREATE INDEX IF NOT EXISTS idx_compute_workspace_branches_active_disk
ON compute_workspace_branches (disk_id, state);

CREATE TABLE IF NOT EXISTS compute_workspace_attachments (
    workspace_id TEXT NOT NULL REFERENCES compute_workspaces(workspace_id) ON DELETE CASCADE,
    client_id TEXT NOT NULL,
    address_or_cidr TEXT NOT NULL,
    mode TEXT NOT NULL,
    export_options_json TEXT NOT NULL,
    state TEXT NOT NULL,
    attached_at_utc TEXT,
    detached_at_utc TEXT,
    PRIMARY KEY (workspace_id, client_id)
);

CREATE TABLE IF NOT EXISTS compute_workspace_operations (
    operation_id TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL REFERENCES compute_workspaces(workspace_id) ON DELETE CASCADE,
    operation_kind TEXT NOT NULL,
    request_id TEXT NOT NULL,
    request_digest TEXT NOT NULL,
    state TEXT NOT NULL,
    stage TEXT NOT NULL,
    completed_bytes INTEGER NOT NULL DEFAULT 0 CHECK (completed_bytes >= 0),
    total_bytes INTEGER CHECK (total_bytes >= 0),
    completed_units INTEGER NOT NULL DEFAULT 0 CHECK (completed_units >= 0),
    total_units INTEGER CHECK (total_units >= 0),
    cancellation_requested INTEGER NOT NULL DEFAULT 0 CHECK (cancellation_requested IN (0, 1)),
    retry_count INTEGER NOT NULL DEFAULT 0 CHECK (retry_count >= 0),
    lease_owner TEXT,
    lease_expires_at_utc TEXT,
    failure_code TEXT,
    failure_message TEXT,
    result_json TEXT,
    created_at_utc TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL,
    UNIQUE (workspace_id, operation_kind, request_id)
);

CREATE INDEX IF NOT EXISTS idx_compute_workspace_operations_runnable
ON compute_workspace_operations (state, lease_expires_at_utc, created_at_utc);

CREATE TABLE IF NOT EXISTS compute_workspace_materializations (
    workspace_id TEXT NOT NULL REFERENCES compute_workspaces(workspace_id) ON DELETE CASCADE,
    operation_id TEXT NOT NULL UNIQUE REFERENCES compute_workspace_operations(operation_id),
    source_object_id TEXT NOT NULL REFERENCES objects(object_id),
    source_placement_id TEXT NOT NULL REFERENCES placements(placement_id),
    destination_relative_path TEXT NOT NULL,
    expected_size_bytes INTEGER NOT NULL CHECK (expected_size_bytes >= 0),
    expected_sha256 TEXT NOT NULL,
    observed_sha256 TEXT,
    state TEXT NOT NULL,
    completed_at_utc TEXT,
    PRIMARY KEY (workspace_id, destination_relative_path)
);

CREATE TABLE IF NOT EXISTS compute_workspace_checkpoints (
    checkpoint_id TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL REFERENCES compute_workspaces(workspace_id) ON DELETE CASCADE,
    relative_prefix TEXT NOT NULL,
    role TEXT NOT NULL,
    reproducibility_class TEXT NOT NULL,
    logical_bytes INTEGER CHECK (logical_bytes >= 0),
    checkpoint_manifest_id TEXT,
    removable_after_promotion INTEGER NOT NULL CHECK (removable_after_promotion IN (0, 1)),
    created_at_utc TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL,
    retention_deadline_utc TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS compute_workspace_promotions (
    promotion_id TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL REFERENCES compute_workspaces(workspace_id) ON DELETE CASCADE,
    operation_id TEXT NOT NULL UNIQUE REFERENCES compute_workspace_operations(operation_id),
    target_store_id TEXT NOT NULL REFERENCES stores(store_id),
    manifest_digest TEXT NOT NULL,
    state TEXT NOT NULL,
    created_at_utc TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL,
    UNIQUE (workspace_id, target_store_id, manifest_digest)
);

CREATE TABLE IF NOT EXISTS compute_workspace_promotion_members (
    promotion_id TEXT NOT NULL REFERENCES compute_workspace_promotions(promotion_id) ON DELETE CASCADE,
    source_relative_path TEXT NOT NULL,
    object_id TEXT NOT NULL,
    object_type TEXT NOT NULL,
    required INTEGER NOT NULL CHECK (required IN (0, 1)),
    size_bytes INTEGER CHECK (size_bytes >= 0),
    sha256 TEXT,
    state TEXT NOT NULL,
    accepted_at_utc TEXT,
    PRIMARY KEY (promotion_id, object_id)
);

CREATE TABLE IF NOT EXISTS compute_workspace_audit_events (
    event_id TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL REFERENCES compute_workspaces(workspace_id) ON DELETE CASCADE,
    actor_id TEXT NOT NULL,
    application_id TEXT,
    operation TEXT NOT NULL,
    request_digest TEXT,
    decision TEXT NOT NULL,
    result TEXT NOT NULL,
    reference_id TEXT,
    recorded_at_utc TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_compute_workspace_audit_events_workspace
ON compute_workspace_audit_events (workspace_id, recorded_at_utc);

-- One physical-capacity authority protects immutable writes and mutable
-- workspace reservations from overcommitting the same disk. Reserved capacity
-- is reduced by accounted consumption so filesystem free-space measurements
-- and outstanding claims are not double-counted.
CREATE TABLE IF NOT EXISTS disk_capacity_claims (
    claim_id TEXT PRIMARY KEY NOT NULL,
    claim_kind TEXT NOT NULL,
    owner_id TEXT NOT NULL,
    request_id TEXT NOT NULL,
    request_digest TEXT NOT NULL,
    disk_id TEXT NOT NULL REFERENCES disks(disk_id),
    state TEXT NOT NULL,
    reserved_bytes INTEGER NOT NULL CHECK (reserved_bytes > 0),
    consumed_bytes INTEGER NOT NULL DEFAULT 0
        CHECK (consumed_bytes >= 0 AND consumed_bytes <= reserved_bytes),
    lease_owner TEXT,
    lease_expires_at_utc TEXT,
    created_at_utc TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL,
    released_at_utc TEXT,
    UNIQUE (claim_kind, owner_id, disk_id)
);

CREATE INDEX IF NOT EXISTS idx_disk_capacity_claims_active_disk
ON disk_capacity_claims (disk_id, state, released_at_utc);

CREATE INDEX IF NOT EXISTS idx_disk_capacity_claims_owner
ON disk_capacity_claims (claim_kind, owner_id, state);
"#;

#[cfg(test)]
mod tests {
    use super::{LIVE_SCHEMA_FORMAT_VERSION, LIVE_SCHEMA_SQL};
    use crate::format::MetadataArtifact;
    use rusqlite::Connection;

    #[test]
    fn live_schema_has_expected_format_version() {
        assert_eq!(
            LIVE_SCHEMA_FORMAT_VERSION.artifact,
            MetadataArtifact::LiveSqlite
        );
        assert_eq!(LIVE_SCHEMA_FORMAT_VERSION.major, 0);
        assert_eq!(LIVE_SCHEMA_FORMAT_VERSION.minor, 8);
    }

    #[test]
    fn live_schema_applies_to_empty_sqlite_database() {
        let connection = Connection::open_in_memory().expect("open in-memory sqlite");

        connection
            .execute_batch(LIVE_SCHEMA_SQL)
            .expect("schema applies cleanly");

        let tables = table_names(&connection);
        assert_eq!(
            tables,
            vec![
                "compute_workspace_attachments",
                "compute_workspace_audit_events",
                "compute_workspace_branches",
                "compute_workspace_checkpoints",
                "compute_workspace_materializations",
                "compute_workspace_operations",
                "compute_workspace_promotion_members",
                "compute_workspace_promotions",
                "compute_workspaces",
                "destage_queue",
                "disk_capacity_claims",
                "disks",
                "ingest_jobs",
                "metadata_format_versions",
                "metadata_migrations",
                "objects",
                "placements",
                "pool_state_markers",
                "pools",
                "profile_catalogue_objects",
                "profile_catalogue_transactions",
                "s3_object_bindings",
                "ssd_object_placements",
                "stores",
            ]
        );
    }

    #[test]
    fn live_schema_enforces_pool_foreign_keys() {
        let connection = Connection::open_in_memory().expect("open in-memory sqlite");
        connection
            .execute_batch(LIVE_SCHEMA_SQL)
            .expect("schema applies cleanly");

        let err = connection
            .execute(
                "INSERT INTO disks (
                    disk_id,
                    pool_id,
                    role,
                    state,
                    created_at_utc,
                    updated_at_utc
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                (
                    "disk-a",
                    "missing-pool",
                    "hdd_capacity",
                    "candidate",
                    "2026-01-01T00:00:00Z",
                    "2026-01-01T00:00:00Z",
                ),
            )
            .expect_err("missing pool should violate foreign key");

        assert!(err.to_string().contains("FOREIGN KEY constraint failed"));
    }

    #[test]
    fn live_schema_defines_ingest_job_columns() {
        let connection = Connection::open_in_memory().expect("open in-memory sqlite");
        connection
            .execute_batch(LIVE_SCHEMA_SQL)
            .expect("schema applies cleanly");

        let columns = table_columns(&connection, "ingest_jobs");

        assert_eq!(
            columns,
            vec![
                "ingest_job_id",
                "store_id",
                "object_id",
                "object_type",
                "state",
                "ingest_mode",
                "acknowledgement_policy",
                "priority",
                "staging_path",
                "expected_size_bytes",
                "received_bytes",
                "content_hash",
                "content_hash_algorithm",
                "failure_message",
                "created_at_utc",
                "updated_at_utc",
            ]
        );
    }

    #[test]
    fn live_schema_defines_durable_destage_columns() {
        let connection = Connection::open_in_memory().expect("open in-memory sqlite");
        connection.execute_batch(LIVE_SCHEMA_SQL).expect("schema");
        assert_eq!(
            table_columns(&connection, "ssd_object_placements"),
            vec![
                "object_id",
                "store_id",
                "relative_path",
                "size_bytes",
                "content_hash_algorithm",
                "content_hash",
                "verified_at_utc",
                "eviction_eligible",
                "evicted_at_utc",
                "created_at_utc",
                "updated_at_utc"
            ]
        );
        assert_eq!(
            table_columns(&connection, "destage_queue"),
            vec![
                "destage_job_id",
                "store_id",
                "object_id",
                "state",
                "expected_size_bytes",
                "content_hash_algorithm",
                "content_hash",
                "acknowledgement_policy",
                "required_copy_count",
                "verified_copy_count",
                "priority",
                "attempt_count",
                "max_attempts",
                "last_error",
                "next_retry_at_utc",
                "lease_owner",
                "lease_expires_at_utc",
                "cancellation_requested",
                "created_at_utc",
                "updated_at_utc"
            ]
        );
    }

    #[test]
    fn live_schema_defines_object_type_columns() {
        let connection = Connection::open_in_memory().expect("open in-memory sqlite");
        connection
            .execute_batch(LIVE_SCHEMA_SQL)
            .expect("schema applies cleanly");

        let columns = table_columns(&connection, "objects");

        assert_eq!(
            columns,
            vec![
                "object_id",
                "store_id",
                "object_type",
                "state",
                "size_bytes",
                "content_hash",
                "created_at_utc",
                "updated_at_utc",
            ]
        );
    }

    #[test]
    fn live_schema_indexes_ingest_job_queue_views() {
        let connection = Connection::open_in_memory().expect("open in-memory sqlite");
        connection
            .execute_batch(LIVE_SCHEMA_SQL)
            .expect("schema applies cleanly");

        assert_eq!(
            index_names(&connection, "ingest_jobs"),
            vec![
                "idx_ingest_jobs_object",
                "idx_ingest_jobs_store_state_priority",
                "sqlite_autoindex_ingest_jobs_1",
            ]
        );
    }

    fn table_names(connection: &Connection) -> Vec<String> {
        let mut statement = connection
            .prepare(
                "SELECT name
                 FROM sqlite_schema
                 WHERE type = 'table'
                 ORDER BY name",
            )
            .expect("prepare table query");
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .expect("query table names");

        rows.map(|row| row.expect("table name")).collect()
    }

    fn table_columns(connection: &Connection, table_name: &str) -> Vec<String> {
        let mut statement = connection
            .prepare(&format!("PRAGMA table_info({table_name})"))
            .expect("prepare table info");
        let rows = statement
            .query_map([], |row| row.get::<_, String>(1))
            .expect("query table info");

        rows.map(|row| row.expect("column name")).collect()
    }

    fn index_names(connection: &Connection, table_name: &str) -> Vec<String> {
        let mut statement = connection
            .prepare(&format!("PRAGMA index_list({table_name})"))
            .expect("prepare index list");
        let rows = statement
            .query_map([], |row| row.get::<_, String>(1))
            .expect("query index list");
        let mut names: Vec<String> = rows.map(|row| row.expect("index name")).collect();
        names.sort();
        names
    }
}
