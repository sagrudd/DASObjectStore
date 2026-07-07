use crate::hash::{copy_and_hash_with_progress, SHA256_ALGORITHM};
use crate::initialize::METADATA_DIR_NAME;
use crate::secure_fs::{create_private_dir_all, create_private_file, set_private_dir_permissions};
use dasobjectstore_core::ids::IngestJobId;
use std::io::Read;
use std::path::{Path, PathBuf};

pub const INGEST_DIR_NAME: &str = "ingest";
pub const INGEST_JOBS_DIR_NAME: &str = "jobs";
pub const INGEST_PAYLOAD_FILE_NAME: &str = "payload";
pub const INGEST_SCRATCH_DIR_NAME: &str = "scratch";
pub const INGEST_CONTENT_HASH_ALGORITHM: &str = SHA256_ALGORITHM;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IngestStagingLayout {
    pub ingest_root: PathBuf,
    pub jobs_root: PathBuf,
}

impl IngestStagingLayout {
    pub fn for_ssd_root(ssd_root: impl AsRef<Path>) -> Self {
        let ingest_root = ssd_root
            .as_ref()
            .join(METADATA_DIR_NAME)
            .join(INGEST_DIR_NAME);
        let jobs_root = ingest_root.join(INGEST_JOBS_DIR_NAME);

        Self {
            ingest_root,
            jobs_root,
        }
    }

    pub fn create_base_directories(&self) -> Result<(), std::io::Error> {
        create_private_dir_all(&self.jobs_root)?;
        set_private_dir_permissions(&self.ingest_root)?;
        if let Some(metadata_root) = self.ingest_root.parent() {
            set_private_dir_permissions(metadata_root)?;
        }

        Ok(())
    }

    pub fn job_paths(&self, job_id: &IngestJobId) -> IngestJobPaths {
        let job_root = self.jobs_root.join(encode_path_component(job_id.as_str()));

        IngestJobPaths {
            payload_path: job_root.join(INGEST_PAYLOAD_FILE_NAME),
            scratch_dir: job_root.join(INGEST_SCRATCH_DIR_NAME),
            job_root,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IngestJobPaths {
    pub job_root: PathBuf,
    pub payload_path: PathBuf,
    pub scratch_dir: PathBuf,
}

impl IngestJobPaths {
    pub fn create_directories(&self) -> Result<(), std::io::Error> {
        create_private_dir_all(&self.scratch_dir)?;
        set_private_dir_permissions(&self.job_root)?;

        Ok(())
    }

    pub fn write_payload_with_hash(
        &self,
        reader: &mut impl Read,
    ) -> Result<IngestWriteReport, std::io::Error> {
        self.write_payload_with_hash_progress(reader, |_| {})
    }

    pub fn write_payload_with_hash_progress(
        &self,
        reader: &mut impl Read,
        progress: impl FnMut(u64),
    ) -> Result<IngestWriteReport, std::io::Error> {
        self.create_directories()?;

        let mut file = create_private_file(&self.payload_path)?;
        let report = copy_and_hash_with_progress(reader, &mut file, progress)?;
        file.sync_all()?;

        Ok(IngestWriteReport {
            bytes_written: report.bytes_written,
            content_hash_algorithm: INGEST_CONTENT_HASH_ALGORITHM.to_string(),
            content_hash: report.content_hash,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IngestWriteReport {
    pub bytes_written: u64,
    pub content_hash_algorithm: String,
    pub content_hash: String,
}

pub(crate) fn encode_path_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::{
        IngestStagingLayout, INGEST_CONTENT_HASH_ALGORITHM, INGEST_DIR_NAME, INGEST_JOBS_DIR_NAME,
        INGEST_PAYLOAD_FILE_NAME, INGEST_SCRATCH_DIR_NAME,
    };
    #[cfg(unix)]
    use crate::secure_fs::{PRIVATE_DIR_MODE, PRIVATE_FILE_MODE};
    use crate::{initialize_pool, read_ingest_queue, PoolInitOptions};
    use dasobjectstore_core::ids::{IngestJobId, PoolId};
    use rusqlite::Connection;
    use std::fs;
    use std::io::Cursor;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn builds_staging_layout_under_metadata_root() {
        let root = PathBuf::from("/tmp/pool-ssd");
        let layout = IngestStagingLayout::for_ssd_root(&root);

        assert_eq!(
            layout.ingest_root,
            root.join(".dasobjectstore").join(INGEST_DIR_NAME)
        );
        assert_eq!(
            layout.jobs_root,
            root.join(".dasobjectstore")
                .join(INGEST_DIR_NAME)
                .join(INGEST_JOBS_DIR_NAME)
        );
    }

    #[test]
    fn derives_safe_job_paths_from_ingest_job_id() {
        let root = PathBuf::from("/tmp/pool-ssd");
        let layout = IngestStagingLayout::for_ssd_root(&root);
        let job_id = IngestJobId::new("store-a/object/1").expect("job id");
        let paths = layout.job_paths(&job_id);

        assert_eq!(
            paths.job_root,
            root.join(".dasobjectstore")
                .join(INGEST_DIR_NAME)
                .join(INGEST_JOBS_DIR_NAME)
                .join("store-a%2Fobject%2F1")
        );
        assert_eq!(
            paths.payload_path,
            paths.job_root.join(INGEST_PAYLOAD_FILE_NAME)
        );
        assert_eq!(
            paths.scratch_dir,
            paths.job_root.join(INGEST_SCRATCH_DIR_NAME)
        );
    }

    #[test]
    fn creates_base_and_job_directories() {
        let root = temp_root("ingest-layout");
        let layout = IngestStagingLayout::for_ssd_root(&root);
        let job_id = IngestJobId::new("job-a").expect("job id");
        let paths = layout.job_paths(&job_id);

        layout
            .create_base_directories()
            .expect("base directories created");
        paths.create_directories().expect("job directories created");

        assert!(layout.jobs_root.is_dir());
        assert!(paths.job_root.is_dir());
        assert!(paths.scratch_dir.is_dir());
        assert!(!paths.payload_path.exists());
        #[cfg(unix)]
        {
            assert_private_dir(&layout.ingest_root);
            assert_private_dir(&layout.jobs_root);
            assert_private_dir(&paths.job_root);
            assert_private_dir(&paths.scratch_dir);
        }

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn streams_payload_and_computes_sha256() {
        let root = temp_root("ingest-hash");
        let layout = IngestStagingLayout::for_ssd_root(&root);
        let job_id = IngestJobId::new("job-a").expect("job id");
        let paths = layout.job_paths(&job_id);
        let mut reader = Cursor::new(b"dasobjectstore ingest bytes".repeat(8));

        let report = paths
            .write_payload_with_hash(&mut reader)
            .expect("payload writes");

        assert_eq!(report.bytes_written, 216);
        assert_eq!(report.content_hash_algorithm, INGEST_CONTENT_HASH_ALGORITHM);
        assert_eq!(
            report.content_hash,
            "6cb54538da3c679ac8d03aaa5ae9fc7d824a8c823bcab8e3962432d6caf23092"
        );
        assert_eq!(
            fs::read(&paths.payload_path).expect("read payload"),
            b"dasobjectstore ingest bytes".repeat(8)
        );
        #[cfg(unix)]
        assert_private_file(&paths.payload_path);

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn restart_preserves_pre_settlement_ingest_job_and_payload() {
        let root = temp_root("ingest-restart-before-settlement");
        let init = initialize_pool(&PoolInitOptions::new(
            &root,
            PoolId::new("pool-a").expect("pool id"),
            "2026-01-01T00:00:00Z",
        ))
        .expect("pool initializes");
        let layout = IngestStagingLayout::for_ssd_root(&root);
        let job_id = IngestJobId::new("job-before-settlement").expect("job id");
        let paths = layout.job_paths(&job_id);
        let mut reader = Cursor::new(b"pre-settlement payload".repeat(4));
        let report = paths
            .write_payload_with_hash(&mut reader)
            .expect("payload writes");
        let staging_path = paths.payload_path.to_string_lossy().into_owned();

        {
            let connection = Connection::open(&init.live_sqlite_path).expect("open live sqlite");
            insert_store(&connection);
            insert_pre_settlement_job(&connection, job_id.as_str(), &staging_path, &report);
        }

        let queue = read_ingest_queue(&init.live_sqlite_path).expect("queue survives restart");

        assert_eq!(queue.jobs.len(), 1);
        assert_eq!(queue.jobs[0].ingest_job_id, job_id);
        assert_eq!(queue.jobs[0].state, "ReadyForPlacement");
        assert_eq!(queue.jobs[0].received_bytes, report.bytes_written);
        assert_eq!(
            queue.jobs[0].content_hash.as_deref(),
            Some(report.content_hash.as_str())
        );
        assert_eq!(
            fs::read(&paths.payload_path).expect("payload survives restart"),
            b"pre-settlement payload".repeat(4)
        );

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn restart_preserves_ingest_job_after_metadata_commit() {
        let root = temp_root("ingest-restart-after-metadata-commit");
        let init = initialize_pool(&PoolInitOptions::new(
            &root,
            PoolId::new("pool-a").expect("pool id"),
            "2026-01-01T00:00:00Z",
        ))
        .expect("pool initializes");
        let layout = IngestStagingLayout::for_ssd_root(&root);
        let job_id = IngestJobId::new("job-after-commit").expect("job id");
        let paths = layout.job_paths(&job_id);
        let mut reader = Cursor::new(b"post-commit payload".repeat(4));
        let report = paths
            .write_payload_with_hash(&mut reader)
            .expect("payload writes");
        let staging_path = paths.payload_path.to_string_lossy().into_owned();

        {
            let connection = Connection::open(&init.live_sqlite_path).expect("open live sqlite");
            insert_store(&connection);
            insert_committed_object(&connection, "object-after-commit", &report);
            insert_post_commit_job(
                &connection,
                job_id.as_str(),
                "object-after-commit",
                &staging_path,
                &report,
            );
        }

        let queue = read_ingest_queue(&init.live_sqlite_path).expect("queue survives restart");
        let connection = Connection::open(&init.live_sqlite_path).expect("reopen live sqlite");
        let object_state: String = connection
            .query_row(
                "SELECT state FROM objects WHERE object_id = 'object-after-commit'",
                [],
                |row| row.get(0),
            )
            .expect("object row survives restart");

        assert_eq!(object_state, "HashVerified");
        assert_eq!(queue.jobs.len(), 1);
        assert_eq!(queue.jobs[0].ingest_job_id, job_id);
        assert_eq!(
            queue.jobs[0]
                .object_id
                .as_ref()
                .expect("object id linked")
                .as_str(),
            "object-after-commit"
        );
        assert_eq!(queue.jobs[0].state, "ReadyForPlacement");
        assert_eq!(queue.jobs[0].received_bytes, report.bytes_written);

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    fn insert_store(connection: &Connection) {
        connection
            .execute(
                "INSERT INTO stores (
                    store_id,
                    pool_id,
                    class,
                    policy_json,
                    created_at_utc,
                    updated_at_utc
                 ) VALUES (
                    'store-a',
                    'pool-a',
                    'generated_data',
                    '{}',
                    '2026-01-01T00:00:00Z',
                    '2026-01-01T00:00:00Z'
                 )",
                [],
            )
            .expect("store inserts");
    }

    fn insert_pre_settlement_job(
        connection: &Connection,
        ingest_job_id: &str,
        staging_path: &str,
        report: &super::IngestWriteReport,
    ) {
        connection
            .execute(
                "INSERT INTO ingest_jobs (
                    ingest_job_id,
                    store_id,
                    state,
                    ingest_mode,
                    acknowledgement_policy,
                    priority,
                    staging_path,
                    expected_size_bytes,
                    received_bytes,
                    content_hash,
                    content_hash_algorithm,
                    created_at_utc,
                    updated_at_utc
                 ) VALUES (?1, 'store-a', 'ReadyForPlacement', 'SsdFirst', 'AfterHddPlacement', 10, ?2, ?3, ?3, ?4, ?5, ?6, ?6)",
                (
                    ingest_job_id,
                    staging_path,
                    report.bytes_written,
                    report.content_hash.as_str(),
                    report.content_hash_algorithm.as_str(),
                    "2026-01-01T00:00:01Z",
                ),
            )
            .expect("ingest job inserts");
    }

    fn insert_committed_object(
        connection: &Connection,
        object_id: &str,
        report: &super::IngestWriteReport,
    ) {
        connection
            .execute(
                "INSERT INTO objects (
                    object_id,
                    store_id,
                    state,
                    size_bytes,
                    content_hash,
                    created_at_utc,
                    updated_at_utc
                 ) VALUES (?1, 'store-a', 'HashVerified', ?2, ?3, ?4, ?4)",
                (
                    object_id,
                    report.bytes_written,
                    report.content_hash.as_str(),
                    "2026-01-01T00:00:01Z",
                ),
            )
            .expect("object inserts");
    }

    fn insert_post_commit_job(
        connection: &Connection,
        ingest_job_id: &str,
        object_id: &str,
        staging_path: &str,
        report: &super::IngestWriteReport,
    ) {
        connection
            .execute(
                "INSERT INTO ingest_jobs (
                    ingest_job_id,
                    store_id,
                    object_id,
                    state,
                    ingest_mode,
                    acknowledgement_policy,
                    priority,
                    staging_path,
                    expected_size_bytes,
                    received_bytes,
                    content_hash,
                    content_hash_algorithm,
                    created_at_utc,
                    updated_at_utc
                 ) VALUES (?1, 'store-a', ?2, 'ReadyForPlacement', 'SsdFirst', 'AfterHddPlacement', 10, ?3, ?4, ?4, ?5, ?6, ?7, ?7)",
                (
                    ingest_job_id,
                    object_id,
                    staging_path,
                    report.bytes_written,
                    report.content_hash.as_str(),
                    report.content_hash_algorithm.as_str(),
                    "2026-01-01T00:00:02Z",
                ),
            )
            .expect("post-commit ingest job inserts");
    }

    fn temp_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dasobjectstore-{name}-{}-{nanos}",
            std::process::id()
        ))
    }

    #[cfg(unix)]
    fn assert_private_dir(path: &std::path::Path) {
        assert_eq!(
            fs::metadata(path)
                .expect("directory metadata")
                .permissions()
                .mode()
                & 0o777,
            PRIVATE_DIR_MODE
        );
    }

    #[cfg(unix)]
    fn assert_private_file(path: &std::path::Path) {
        assert_eq!(
            fs::metadata(path)
                .expect("file metadata")
                .permissions()
                .mode()
                & 0o777,
            PRIVATE_FILE_MODE
        );
    }
}
