use crate::hash::{copy_and_hash_with_progress, SHA256_ALGORITHM};
use crate::initialize::METADATA_DIR_NAME;
use crate::secure_fs::{create_private_dir_all, create_private_file, set_private_dir_permissions};
use dasobjectstore_core::ids::{DiskId, IngestJobId};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum IngestJournalFileState {
    Planned,
    Staged,
    Written,
    Verified,
    Failed,
    Retried,
    Cancelled,
    Finalized,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IngestJournalContentHash {
    pub algorithm: String,
    pub value: String,
}

impl IngestJournalContentHash {
    pub fn new(algorithm: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            algorithm: algorithm.into(),
            value: value.into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IngestJournalHddWrite {
    pub disk_id: DiskId,
    pub copy_number: u8,
    pub planned_bytes: u64,
    pub written_bytes: u64,
    pub verified: bool,
}

impl IngestJournalHddWrite {
    pub fn new(disk_id: DiskId, copy_number: u8, planned_bytes: u64, written_bytes: u64) -> Self {
        Self {
            disk_id,
            copy_number,
            planned_bytes,
            written_bytes,
            verified: false,
        }
    }

    pub fn is_fully_written(&self) -> bool {
        self.written_bytes >= self.planned_bytes
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IngestJournalFinalizationReadiness {
    pub staged: bool,
    pub required_hdd_copies: u8,
    pub fully_written_hdd_copies: u8,
    pub verified_hdd_copies: u8,
    pub ready: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IngestJournalFileRecord {
    pub ingest_job_id: IngestJobId,
    pub source_path: PathBuf,
    pub expected_size_bytes: u64,
    pub staged_bytes: u64,
    pub content_hash: Option<IngestJournalContentHash>,
    pub hdd_writes: Vec<IngestJournalHddWrite>,
    pub required_hdd_copies: u8,
    pub retry_count: u32,
    pub failure_message: Option<String>,
    pub state: IngestJournalFileState,
}

impl IngestJournalFileRecord {
    pub fn planned(
        ingest_job_id: IngestJobId,
        source_path: impl Into<PathBuf>,
        expected_size_bytes: u64,
        required_hdd_copies: u8,
    ) -> Self {
        Self {
            ingest_job_id,
            source_path: source_path.into(),
            expected_size_bytes,
            staged_bytes: 0,
            content_hash: None,
            hdd_writes: Vec::new(),
            required_hdd_copies,
            retry_count: 0,
            failure_message: None,
            state: IngestJournalFileState::Planned,
        }
    }

    pub fn record_staged_progress(
        &mut self,
        staged_bytes: u64,
    ) -> Result<(), IngestJournalTransitionError> {
        self.ensure_active("record staged progress")?;
        if staged_bytes > self.expected_size_bytes {
            return Err(IngestJournalTransitionError::BytesExceedExpected {
                field: "staged_bytes",
                value: staged_bytes,
                expected: self.expected_size_bytes,
            });
        }

        self.staged_bytes = staged_bytes;
        self.refresh_progress_state();
        Ok(())
    }

    pub fn mark_staged(
        &mut self,
        content_hash_algorithm: impl Into<String>,
        content_hash: impl Into<String>,
    ) -> Result<(), IngestJournalTransitionError> {
        self.ensure_active("mark staged")?;
        self.staged_bytes = self.expected_size_bytes;
        self.content_hash = Some(IngestJournalContentHash::new(
            content_hash_algorithm,
            content_hash,
        ));
        self.state = IngestJournalFileState::Staged;
        self.failure_message = None;
        Ok(())
    }

    pub fn record_hdd_write_progress(
        &mut self,
        disk_id: DiskId,
        copy_number: u8,
        written_bytes: u64,
    ) -> Result<(), IngestJournalTransitionError> {
        self.ensure_active("record HDD write progress")?;
        self.ensure_staged("record HDD write progress")?;
        if written_bytes > self.expected_size_bytes {
            return Err(IngestJournalTransitionError::BytesExceedExpected {
                field: "written_bytes",
                value: written_bytes,
                expected: self.expected_size_bytes,
            });
        }

        let expected_size_bytes = self.expected_size_bytes;
        if let Some(write) = self.hdd_write_for_mut(&disk_id, copy_number) {
            write.written_bytes = written_bytes;
            write.planned_bytes = expected_size_bytes;
            if !write.is_fully_written() {
                write.verified = false;
            }
        } else {
            self.hdd_writes.push(IngestJournalHddWrite::new(
                disk_id,
                copy_number,
                expected_size_bytes,
                written_bytes,
            ));
        }

        self.refresh_progress_state();
        Ok(())
    }

    pub fn mark_hdd_verified(
        &mut self,
        disk_id: &DiskId,
        copy_number: u8,
    ) -> Result<(), IngestJournalTransitionError> {
        self.ensure_active("mark HDD verified")?;
        self.ensure_staged("mark HDD verified")?;
        let expected_size_bytes = self.expected_size_bytes;
        let write = self
            .hdd_write_for_mut(disk_id, copy_number)
            .ok_or_else(|| IngestJournalTransitionError::MissingHddWrite {
                disk_id: disk_id.clone(),
                copy_number,
            })?;
        if write.written_bytes < expected_size_bytes {
            return Err(
                IngestJournalTransitionError::VerificationBeforeWriteComplete {
                    disk_id: disk_id.clone(),
                    copy_number,
                    written_bytes: write.written_bytes,
                    expected: expected_size_bytes,
                },
            );
        }

        write.verified = true;
        self.refresh_progress_state();
        Ok(())
    }

    pub fn mark_failed(
        &mut self,
        failure_message: impl Into<String>,
    ) -> Result<(), IngestJournalTransitionError> {
        self.ensure_active("mark failed")?;
        let failure_message = failure_message.into();
        if failure_message.trim().is_empty() {
            return Err(IngestJournalTransitionError::EmptyFailureMessage);
        }

        self.failure_message = Some(failure_message);
        self.state = IngestJournalFileState::Failed;
        Ok(())
    }

    pub fn mark_retried(&mut self) -> Result<(), IngestJournalTransitionError> {
        if self.state != IngestJournalFileState::Failed {
            return Err(IngestJournalTransitionError::InvalidState {
                action: "mark retried",
                current: self.state,
            });
        }

        self.retry_count += 1;
        self.failure_message = None;
        self.state = IngestJournalFileState::Retried;
        Ok(())
    }

    pub fn cancel(&mut self) -> Result<(), IngestJournalTransitionError> {
        self.ensure_not_terminal("cancel")?;
        self.state = IngestJournalFileState::Cancelled;
        Ok(())
    }

    pub fn finalize(&mut self) -> Result<(), IngestJournalTransitionError> {
        self.ensure_active("finalize")?;
        let readiness = self.finalization_readiness();
        if !readiness.ready {
            return Err(IngestJournalTransitionError::FinalizationNotReady(
                readiness,
            ));
        }

        self.state = IngestJournalFileState::Finalized;
        Ok(())
    }

    pub fn is_finalization_ready(&self) -> bool {
        self.finalization_readiness().ready
    }

    pub fn finalization_readiness(&self) -> IngestJournalFinalizationReadiness {
        let staged = self.is_staged();
        let fully_written_hdd_copies = self.fully_written_hdd_copies();
        let verified_hdd_copies = self.verified_hdd_copies();
        let ready = staged
            && self.required_hdd_copies > 0
            && fully_written_hdd_copies >= self.required_hdd_copies
            && verified_hdd_copies >= self.required_hdd_copies
            && matches!(self.state, IngestJournalFileState::Verified);

        IngestJournalFinalizationReadiness {
            staged,
            required_hdd_copies: self.required_hdd_copies,
            fully_written_hdd_copies,
            verified_hdd_copies,
            ready,
        }
    }

    fn ensure_not_terminal(
        &self,
        action: &'static str,
    ) -> Result<(), IngestJournalTransitionError> {
        if matches!(
            self.state,
            IngestJournalFileState::Cancelled | IngestJournalFileState::Finalized
        ) {
            return Err(IngestJournalTransitionError::InvalidState {
                action,
                current: self.state,
            });
        }

        Ok(())
    }

    fn ensure_active(&self, action: &'static str) -> Result<(), IngestJournalTransitionError> {
        if matches!(
            self.state,
            IngestJournalFileState::Failed
                | IngestJournalFileState::Cancelled
                | IngestJournalFileState::Finalized
        ) {
            return Err(IngestJournalTransitionError::InvalidState {
                action,
                current: self.state,
            });
        }

        Ok(())
    }

    fn ensure_staged(&self, action: &'static str) -> Result<(), IngestJournalTransitionError> {
        if !self.is_staged() {
            return Err(IngestJournalTransitionError::InvalidState {
                action,
                current: self.state,
            });
        }

        Ok(())
    }

    fn is_staged(&self) -> bool {
        self.staged_bytes == self.expected_size_bytes && self.content_hash.is_some()
    }

    fn fully_written_hdd_copies(&self) -> u8 {
        self.hdd_writes
            .iter()
            .filter(|write| write.is_fully_written())
            .count()
            .min(u8::MAX as usize) as u8
    }

    fn verified_hdd_copies(&self) -> u8 {
        self.hdd_writes
            .iter()
            .filter(|write| write.verified && write.is_fully_written())
            .count()
            .min(u8::MAX as usize) as u8
    }

    fn hdd_write_for_mut(
        &mut self,
        disk_id: &DiskId,
        copy_number: u8,
    ) -> Option<&mut IngestJournalHddWrite> {
        self.hdd_writes
            .iter_mut()
            .find(|write| &write.disk_id == disk_id && write.copy_number == copy_number)
    }

    fn refresh_progress_state(&mut self) {
        if matches!(
            self.state,
            IngestJournalFileState::Failed
                | IngestJournalFileState::Cancelled
                | IngestJournalFileState::Finalized
        ) {
            return;
        }

        if self.required_hdd_copies > 0 && self.verified_hdd_copies() >= self.required_hdd_copies {
            self.state = IngestJournalFileState::Verified;
        } else if self.required_hdd_copies > 0
            && self.fully_written_hdd_copies() >= self.required_hdd_copies
        {
            self.state = IngestJournalFileState::Written;
        } else if self.is_staged() {
            self.state = IngestJournalFileState::Staged;
        } else if self.state != IngestJournalFileState::Retried {
            self.state = IngestJournalFileState::Planned;
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IngestJournalTransitionError {
    InvalidState {
        action: &'static str,
        current: IngestJournalFileState,
    },
    BytesExceedExpected {
        field: &'static str,
        value: u64,
        expected: u64,
    },
    MissingHddWrite {
        disk_id: DiskId,
        copy_number: u8,
    },
    VerificationBeforeWriteComplete {
        disk_id: DiskId,
        copy_number: u8,
        written_bytes: u64,
        expected: u64,
    },
    FinalizationNotReady(IngestJournalFinalizationReadiness),
    EmptyFailureMessage,
}

impl Display for IngestJournalTransitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidState { action, current } => {
                write!(formatter, "cannot {action} from ingest journal state {current:?}")
            }
            Self::BytesExceedExpected {
                field,
                value,
                expected,
            } => write!(
                formatter,
                "ingest journal {field} {value} exceeds expected size {expected}"
            ),
            Self::MissingHddWrite {
                disk_id,
                copy_number,
            } => write!(
                formatter,
                "ingest journal has no HDD write for disk {disk_id} copy {copy_number}"
            ),
            Self::VerificationBeforeWriteComplete {
                disk_id,
                copy_number,
                written_bytes,
                expected,
            } => write!(
                formatter,
                "cannot verify disk {disk_id} copy {copy_number}: {written_bytes} of {expected} bytes written"
            ),
            Self::FinalizationNotReady(readiness) => write!(
                formatter,
                "ingest journal is not finalization-ready: {}/{} written copies, {}/{} verified copies",
                readiness.fully_written_hdd_copies,
                readiness.required_hdd_copies,
                readiness.verified_hdd_copies,
                readiness.required_hdd_copies
            ),
            Self::EmptyFailureMessage => {
                formatter.write_str("ingest journal failure message must not be empty")
            }
        }
    }
}

impl std::error::Error for IngestJournalTransitionError {}

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
        IngestJournalFileRecord, IngestJournalFileState, IngestJournalTransitionError,
        IngestStagingLayout, INGEST_CONTENT_HASH_ALGORITHM, INGEST_DIR_NAME, INGEST_JOBS_DIR_NAME,
        INGEST_PAYLOAD_FILE_NAME, INGEST_SCRATCH_DIR_NAME,
    };
    #[cfg(unix)]
    use crate::secure_fs::{PRIVATE_DIR_MODE, PRIVATE_FILE_MODE};
    use crate::{initialize_pool, read_ingest_queue, PoolInitOptions};
    use dasobjectstore_core::ids::{DiskId, IngestJobId, PoolId};
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
    fn journal_record_transitions_to_finalized_after_required_verified_writes() {
        let mut record = journal_record("job-finalize", 128, 2);
        let disk_a = disk_id("disk-a");
        let disk_b = disk_id("disk-b");

        record
            .mark_staged(INGEST_CONTENT_HASH_ALGORITHM, "hash-a")
            .expect("staged");
        assert_eq!(record.state, IngestJournalFileState::Staged);
        assert!(!record.is_finalization_ready());

        record
            .record_hdd_write_progress(disk_a.clone(), 1, 128)
            .expect("first write");
        assert_eq!(record.state, IngestJournalFileState::Staged);
        record
            .record_hdd_write_progress(disk_b.clone(), 2, 128)
            .expect("second write");
        assert_eq!(record.state, IngestJournalFileState::Written);
        assert!(!record.is_finalization_ready());

        record.mark_hdd_verified(&disk_a, 1).expect("first verify");
        assert_eq!(record.state, IngestJournalFileState::Written);
        record.mark_hdd_verified(&disk_b, 2).expect("second verify");
        assert_eq!(record.state, IngestJournalFileState::Verified);

        let readiness = record.finalization_readiness();
        assert!(readiness.ready);
        assert_eq!(readiness.fully_written_hdd_copies, 2);
        assert_eq!(readiness.verified_hdd_copies, 2);

        record.finalize().expect("finalized");
        assert_eq!(record.state, IngestJournalFileState::Finalized);
    }

    #[test]
    fn journal_record_preserves_interrupted_partial_stage_and_write_progress() {
        let mut record = journal_record("job-partial", 100, 1);
        let disk_a = disk_id("disk-a");

        record
            .record_staged_progress(40)
            .expect("partial stage recorded");

        assert_eq!(record.state, IngestJournalFileState::Planned);
        assert_eq!(record.staged_bytes, 40);
        assert!(!record.is_finalization_ready());
        assert!(matches!(
            record.record_hdd_write_progress(disk_a.clone(), 1, 10),
            Err(IngestJournalTransitionError::InvalidState { .. })
        ));

        record
            .mark_staged(INGEST_CONTENT_HASH_ALGORITHM, "hash-a")
            .expect("staged");
        record
            .record_hdd_write_progress(disk_a.clone(), 1, 64)
            .expect("partial hdd write recorded");

        assert_eq!(record.state, IngestJournalFileState::Staged);
        assert_eq!(record.hdd_writes.len(), 1);
        assert_eq!(record.hdd_writes[0].written_bytes, 64);
        assert!(!record.hdd_writes[0].verified);
        assert!(!record.is_finalization_ready());
        assert!(matches!(
            record.mark_hdd_verified(&disk_a, 1),
            Err(IngestJournalTransitionError::VerificationBeforeWriteComplete { .. })
        ));
    }

    #[test]
    fn journal_record_tracks_failure_retry_and_cancel_states() {
        let mut record = journal_record("job-retry", 16, 1);

        record.mark_failed("source read failed").expect("failed");
        assert_eq!(record.state, IngestJournalFileState::Failed);
        assert_eq!(
            record.failure_message.as_deref(),
            Some("source read failed")
        );
        assert_eq!(record.retry_count, 0);
        assert!(matches!(
            record.record_staged_progress(16),
            Err(IngestJournalTransitionError::InvalidState {
                current: IngestJournalFileState::Failed,
                ..
            })
        ));

        record.mark_retried().expect("retried");
        assert_eq!(record.state, IngestJournalFileState::Retried);
        assert_eq!(record.retry_count, 1);
        assert_eq!(record.failure_message, None);

        record
            .mark_failed("ssd write failed")
            .expect("failed again");
        record.mark_retried().expect("retried again");
        assert_eq!(record.retry_count, 2);

        record.cancel().expect("cancelled");
        assert_eq!(record.state, IngestJournalFileState::Cancelled);
        assert!(matches!(
            record.finalize(),
            Err(IngestJournalTransitionError::InvalidState {
                current: IngestJournalFileState::Cancelled,
                ..
            })
        ));
    }

    #[test]
    fn journal_record_rejects_finalization_until_hdd_write_and_verification_are_complete() {
        let mut record = journal_record("job-gated", 32, 1);
        let disk_a = disk_id("disk-a");

        assert!(matches!(
            record.finalize(),
            Err(IngestJournalTransitionError::FinalizationNotReady(readiness))
                if !readiness.staged && !readiness.ready
        ));

        record
            .mark_staged(INGEST_CONTENT_HASH_ALGORITHM, "hash-a")
            .expect("staged");
        assert!(matches!(
            record.finalize(),
            Err(IngestJournalTransitionError::FinalizationNotReady(readiness))
                if readiness.staged
                    && readiness.fully_written_hdd_copies == 0
                    && readiness.verified_hdd_copies == 0
        ));

        record
            .record_hdd_write_progress(disk_a.clone(), 1, 32)
            .expect("written");
        assert_eq!(record.state, IngestJournalFileState::Written);
        assert!(matches!(
            record.finalize(),
            Err(IngestJournalTransitionError::FinalizationNotReady(readiness))
                if readiness.fully_written_hdd_copies == 1
                    && readiness.verified_hdd_copies == 0
        ));
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

    fn journal_record(
        ingest_job_id: &str,
        expected_size_bytes: u64,
        required_hdd_copies: u8,
    ) -> IngestJournalFileRecord {
        IngestJournalFileRecord::planned(
            IngestJobId::new(ingest_job_id).expect("ingest job id"),
            format!("/source/{ingest_job_id}.bin"),
            expected_size_bytes,
            required_hdd_copies,
        )
    }

    fn disk_id(value: &str) -> DiskId {
        DiskId::new(value).expect("disk id")
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
