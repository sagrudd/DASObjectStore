use crate::initialize::METADATA_DIR_NAME;
use dasobjectstore_core::ids::IngestJobId;
use std::fs;
use std::path::{Path, PathBuf};

pub const INGEST_DIR_NAME: &str = "ingest";
pub const INGEST_JOBS_DIR_NAME: &str = "jobs";
pub const INGEST_PAYLOAD_FILE_NAME: &str = "payload";
pub const INGEST_SCRATCH_DIR_NAME: &str = "scratch";

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
        fs::create_dir_all(&self.jobs_root)
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
        fs::create_dir_all(&self.scratch_dir)
    }
}

fn encode_path_component(value: &str) -> String {
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
        IngestStagingLayout, INGEST_DIR_NAME, INGEST_JOBS_DIR_NAME, INGEST_PAYLOAD_FILE_NAME,
        INGEST_SCRATCH_DIR_NAME,
    };
    use dasobjectstore_core::ids::IngestJobId;
    use std::fs;
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

        fs::remove_dir_all(root).expect("cleanup temp root");
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
}
