use crate::hash::{copy_and_hash_with_controlled_progress, hash_file_sha256, SHA256_ALGORITHM};
use crate::secure_fs::{create_private_dir_all, create_private_file, set_private_dir_permissions};
use dasobjectstore_core::ids::{DiskId, ObjectId};
use sha2::{Digest, Sha256};
use std::fmt::{self, Display};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Instant;

pub const HDD_COPY_CONTENT_HASH_ALGORITHM: &str = SHA256_ALGORITHM;

#[cfg(test)]
static ACTIVE_FANOUT_WRITERS: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
static MAX_ACTIVE_FANOUT_WRITERS: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HddCopyRequest {
    pub object_id: ObjectId,
    pub disk_id: DiskId,
    pub copy_number: u8,
    pub source_path: PathBuf,
    pub destination_path: PathBuf,
    pub expected_content_hash: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HddInlineHashCopyRequest {
    pub object_id: ObjectId,
    pub disk_id: DiskId,
    pub copy_number: u8,
    pub source_path: PathBuf,
    pub destination_path: PathBuf,
}

impl HddInlineHashCopyRequest {
    pub fn new(
        object_id: ObjectId,
        disk_id: DiskId,
        copy_number: u8,
        source_path: impl Into<PathBuf>,
        destination_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            object_id,
            disk_id,
            copy_number,
            source_path: source_path.into(),
            destination_path: destination_path.into(),
        }
    }
}

impl HddCopyRequest {
    pub fn new(
        object_id: ObjectId,
        disk_id: DiskId,
        copy_number: u8,
        source_path: impl Into<PathBuf>,
        destination_path: impl Into<PathBuf>,
        expected_content_hash: impl Into<String>,
    ) -> Self {
        Self {
            object_id,
            disk_id,
            copy_number,
            source_path: source_path.into(),
            destination_path: destination_path.into(),
            expected_content_hash: expected_content_hash.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HddCopyReport {
    pub object_id: ObjectId,
    pub disk_id: DiskId,
    pub copy_number: u8,
    pub destination_path: PathBuf,
    pub bytes_written: u64,
    pub content_hash_algorithm: String,
    pub content_hash: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HddInlineHashCopyProgress {
    BytesWritten {
        bytes_written: u64,
    },
    FsyncStarted {
        bytes_written: u64,
    },
    FsyncComplete {
        bytes_written: u64,
        duration_millis: u64,
    },
}

#[derive(Debug)]
pub enum HddCopyError {
    Io(std::io::Error),
    Cancelled,
    HashMismatch { expected: String, actual: String },
}

impl Display for HddCopyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(formatter, "HDD copy IO failed: {err}"),
            Self::Cancelled => formatter.write_str("HDD copy cancelled"),
            Self::HashMismatch { expected, actual } => {
                write!(
                    formatter,
                    "HDD copy hash mismatch: expected {expected}, got {actual}"
                )
            }
        }
    }
}

impl std::error::Error for HddCopyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Cancelled => None,
            Self::HashMismatch { .. } => None,
        }
    }
}

impl From<std::io::Error> for HddCopyError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

pub fn write_verified_hdd_copy(request: &HddCopyRequest) -> Result<HddCopyReport, HddCopyError> {
    write_verified_hdd_copy_with_progress(request, |_| {})
}

pub fn write_verified_hdd_copy_with_progress(
    request: &HddCopyRequest,
    mut progress: impl FnMut(u64),
) -> Result<HddCopyReport, HddCopyError> {
    write_verified_hdd_copy_with_controlled_progress(request, |bytes_written| {
        progress(bytes_written);
        Ok(())
    })
}

pub fn write_verified_hdd_copy_with_controlled_progress(
    request: &HddCopyRequest,
    progress: impl FnMut(u64) -> Result<(), HddCopyError>,
) -> Result<HddCopyReport, HddCopyError> {
    if let Some(parent) = request.destination_path.parent() {
        create_private_dir_all(parent)?;
        restrict_object_tree_dirs(parent)?;
    }

    let report = write_verified_hdd_copy_inner(request, progress);
    if report.is_err() {
        let _ = fs::remove_file(&request.destination_path);
    }

    report
}

pub fn write_hdd_copy_with_inline_hash_with_controlled_progress(
    request: &HddInlineHashCopyRequest,
    progress: impl FnMut(u64) -> Result<(), HddCopyError>,
) -> Result<HddCopyReport, HddCopyError> {
    if let Some(parent) = request.destination_path.parent() {
        create_private_dir_all(parent)?;
        restrict_object_tree_dirs(parent)?;
    }

    let report = write_hdd_copy_with_inline_hash_inner(request, progress);
    if report.is_err() {
        let _ = fs::remove_file(&request.destination_path);
    }

    report
}

/// Copy one HDD payload while calculating its checksum from the same source
/// read. Callers that have an expected digest may compare the returned report
/// after the copy; no strict pre-copy source read is required.
pub fn write_hdd_copy_with_inline_hash(
    request: &HddInlineHashCopyRequest,
) -> Result<HddCopyReport, HddCopyError> {
    write_hdd_copy_with_inline_hash_with_controlled_progress(request, |_| Ok(()))
}

/// Copies one source stream to distinct HDD destinations concurrently.
///
/// The source is opened and read once. Each bounded writer queue provides
/// backpressure from its physical disk, while each writer calculates a local
/// checksum and `fsync`s its temporary payload before reporting completion.
pub(crate) fn write_hdd_copies_with_inline_hash_fanout_with_controlled_progress(
    requests: &[HddInlineHashCopyRequest],
    mut progress: impl FnMut(&DiskId, u8, HddInlineHashCopyProgress) -> Result<(), HddCopyError>,
) -> Result<Vec<HddCopyReport>, HddCopyError> {
    let source_path = validate_fanout_requests(requests)?;
    let mut source = File::open(source_path)?;
    write_hdd_copies_from_reader_with_controlled_progress(
        &mut source,
        requests,
        |disk_id, copy, bytes| progress(disk_id, copy, bytes),
    )
}

fn write_hdd_copies_from_reader_with_controlled_progress(
    source: &mut impl io::Read,
    requests: &[HddInlineHashCopyRequest],
    mut progress: impl FnMut(&DiskId, u8, HddInlineHashCopyProgress) -> Result<(), HddCopyError>,
) -> Result<Vec<HddCopyReport>, HddCopyError> {
    validate_fanout_requests(requests)?;
    let mut destinations = Vec::with_capacity(requests.len());
    let prepared = (|| -> Result<(), HddCopyError> {
        for request in requests {
            if let Some(parent) = request.destination_path.parent() {
                create_private_dir_all(parent)?;
                restrict_object_tree_dirs(parent)?;
            }
            destinations.push(create_private_file(&request.destination_path)?);
        }
        Ok(())
    })();
    if let Err(error) = prepared {
        cleanup_fanout_destinations(requests);
        return Err(error);
    }

    let result = write_hdd_copies_to_open_destinations(
        source,
        requests,
        destinations,
        |disk_id, copy, bytes| progress(disk_id, copy, bytes),
    );
    if result.is_err() {
        cleanup_fanout_destinations(requests);
    }
    result
}

fn write_hdd_copies_to_open_destinations(
    source: &mut impl io::Read,
    requests: &[HddInlineHashCopyRequest],
    destinations: Vec<File>,
    mut progress: impl FnMut(&DiskId, u8, HddInlineHashCopyProgress) -> Result<(), HddCopyError>,
) -> Result<Vec<HddCopyReport>, HddCopyError> {
    const FANOUT_QUEUE_CAPACITY: usize = 2;

    let (write_progress_tx, write_progress_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();
    let mut senders = Vec::with_capacity(requests.len());
    let mut workers = Vec::with_capacity(requests.len());

    for (index, (request, destination)) in requests.iter().cloned().zip(destinations).enumerate() {
        let (sender, receiver) = mpsc::sync_channel(FANOUT_QUEUE_CAPACITY);
        let write_progress_tx = write_progress_tx.clone();
        let result_tx = result_tx.clone();
        workers.push(thread::spawn(move || {
            #[cfg(test)]
            let _probe = FanoutWriterProbe::new();
            let result = write_fanout_destination(request, destination, receiver, |progress| {
                write_progress_tx
                    .send((index, progress))
                    .map_err(|_| HddCopyError::Cancelled)
            });
            let _ = result_tx.send(result);
        }));
        senders.push(sender);
    }
    drop(write_progress_tx);
    drop(result_tx);

    let mut source_hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let copy_result = (|| -> Result<(), HddCopyError> {
        loop {
            let read = source.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            source_hasher.update(&buffer[..read]);
            let chunk: Arc<[u8]> = Arc::from(&buffer[..read]);
            for sender in &senders {
                sender
                    .send(Arc::clone(&chunk))
                    .map_err(|_| HddCopyError::Cancelled)?;
            }
            drain_fanout_progress(&write_progress_rx, requests, &mut progress)?;
        }
        Ok(())
    })();
    drop(senders);

    let mut reports = Vec::with_capacity(requests.len());
    let mut first_error = copy_result.err();
    let mut results_received = 0;
    while results_received < requests.len() {
        if first_error.is_none() {
            if let Err(error) = drain_fanout_progress(&write_progress_rx, requests, &mut progress) {
                first_error = Some(error);
            }
        }
        match result_rx.recv() {
            Ok(Ok(report)) => {
                reports.push(report);
                results_received += 1;
            }
            Ok(Err(error)) => {
                first_error.get_or_insert(error);
                results_received += 1;
            }
            Err(_) => {
                first_error.get_or_insert(HddCopyError::Cancelled);
                break;
            }
        }
    }
    for worker in workers {
        worker.join().map_err(|_| {
            HddCopyError::Io(io::Error::other("HDD fan-out writer thread panicked"))
        })?;
    }
    if let Some(error) = first_error {
        return Err(error);
    }
    drain_fanout_progress(&write_progress_rx, requests, &mut progress)?;

    let source_hash = encode_hash(source_hasher.finalize());
    for report in &reports {
        if report.content_hash != source_hash {
            return Err(HddCopyError::HashMismatch {
                expected: source_hash,
                actual: report.content_hash.clone(),
            });
        }
    }
    reports.sort_by_key(|report| report.copy_number);
    Ok(reports)
}

fn write_fanout_destination(
    request: HddInlineHashCopyRequest,
    mut destination: File,
    receiver: mpsc::Receiver<Arc<[u8]>>,
    mut progress: impl FnMut(HddInlineHashCopyProgress) -> Result<(), HddCopyError>,
) -> Result<HddCopyReport, HddCopyError> {
    let mut hasher = Sha256::new();
    let mut bytes_written = 0_u64;
    for chunk in receiver {
        destination.write_all(&chunk)?;
        hasher.update(&chunk);
        bytes_written = bytes_written.saturating_add(chunk.len() as u64);
        progress(HddInlineHashCopyProgress::BytesWritten { bytes_written })?;
    }
    progress(HddInlineHashCopyProgress::FsyncStarted { bytes_written })?;
    let fsync_started_at = Instant::now();
    destination.sync_all()?;
    progress(HddInlineHashCopyProgress::FsyncComplete {
        bytes_written,
        duration_millis: fsync_started_at
            .elapsed()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64,
    })?;

    Ok(HddCopyReport {
        object_id: request.object_id,
        disk_id: request.disk_id,
        copy_number: request.copy_number,
        destination_path: request.destination_path,
        bytes_written,
        content_hash_algorithm: HDD_COPY_CONTENT_HASH_ALGORITHM.to_string(),
        content_hash: encode_hash(hasher.finalize()),
    })
}

fn validate_fanout_requests(requests: &[HddInlineHashCopyRequest]) -> Result<&Path, HddCopyError> {
    let Some(first) = requests.first() else {
        return Err(HddCopyError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "HDD fan-out requires at least one destination",
        )));
    };
    if requests
        .iter()
        .any(|request| request.source_path != first.source_path)
    {
        return Err(HddCopyError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "HDD fan-out destinations must share one source path",
        )));
    }
    Ok(&first.source_path)
}

fn drain_fanout_progress(
    receiver: &mpsc::Receiver<(usize, HddInlineHashCopyProgress)>,
    requests: &[HddInlineHashCopyRequest],
    progress: &mut impl FnMut(&DiskId, u8, HddInlineHashCopyProgress) -> Result<(), HddCopyError>,
) -> Result<(), HddCopyError> {
    while let Ok((index, copy_progress)) = receiver.try_recv() {
        let request = &requests[index];
        progress(&request.disk_id, request.copy_number, copy_progress)?;
    }
    Ok(())
}

fn cleanup_fanout_destinations(requests: &[HddInlineHashCopyRequest]) {
    for request in requests {
        let _ = fs::remove_file(&request.destination_path);
    }
}

#[cfg(test)]
struct FanoutWriterProbe;

#[cfg(test)]
impl FanoutWriterProbe {
    fn new() -> Self {
        let active = ACTIVE_FANOUT_WRITERS.fetch_add(1, Ordering::SeqCst) + 1;
        MAX_ACTIVE_FANOUT_WRITERS.fetch_max(active, Ordering::SeqCst);
        thread::sleep(std::time::Duration::from_millis(10));
        Self
    }
}

#[cfg(test)]
impl Drop for FanoutWriterProbe {
    fn drop(&mut self) {
        ACTIVE_FANOUT_WRITERS.fetch_sub(1, Ordering::SeqCst);
    }
}

fn encode_hash(hash: impl AsRef<[u8]>) -> String {
    hash.as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn write_verified_hdd_copy_inner(
    request: &HddCopyRequest,
    mut progress: impl FnMut(u64) -> Result<(), HddCopyError>,
) -> Result<HddCopyReport, HddCopyError> {
    let mut source = File::open(&request.source_path)?;
    let mut destination = create_private_file(&request.destination_path)?;
    let write_report =
        copy_and_hash_with_controlled_progress(&mut source, &mut destination, |bytes_written| {
            progress(bytes_written).map_err(hdd_copy_error_to_io)
        })
        .map_err(hdd_copy_error_from_io)?;
    destination.sync_all()?;

    if write_report.content_hash != request.expected_content_hash {
        return Err(HddCopyError::HashMismatch {
            expected: request.expected_content_hash.clone(),
            actual: write_report.content_hash,
        });
    }

    Ok(HddCopyReport {
        object_id: request.object_id.clone(),
        disk_id: request.disk_id.clone(),
        copy_number: request.copy_number,
        destination_path: request.destination_path.clone(),
        bytes_written: write_report.bytes_written,
        content_hash_algorithm: HDD_COPY_CONTENT_HASH_ALGORITHM.to_string(),
        content_hash: request.expected_content_hash.clone(),
    })
}

fn write_hdd_copy_with_inline_hash_inner(
    request: &HddInlineHashCopyRequest,
    mut progress: impl FnMut(u64) -> Result<(), HddCopyError>,
) -> Result<HddCopyReport, HddCopyError> {
    let mut source = File::open(&request.source_path)?;
    let mut destination = create_private_file(&request.destination_path)?;
    let write_report =
        copy_and_hash_with_controlled_progress(&mut source, &mut destination, |bytes_written| {
            progress(bytes_written).map_err(hdd_copy_error_to_io)
        })
        .map_err(hdd_copy_error_from_io)?;
    destination.sync_all()?;

    Ok(HddCopyReport {
        object_id: request.object_id.clone(),
        disk_id: request.disk_id.clone(),
        copy_number: request.copy_number,
        destination_path: request.destination_path.clone(),
        bytes_written: write_report.bytes_written,
        content_hash_algorithm: HDD_COPY_CONTENT_HASH_ALGORITHM.to_string(),
        content_hash: write_report.content_hash,
    })
}

fn hdd_copy_error_to_io(error: HddCopyError) -> io::Error {
    match error {
        HddCopyError::Io(error) => error,
        HddCopyError::Cancelled => io::Error::new(io::ErrorKind::Interrupted, "HDD copy cancelled"),
        HddCopyError::HashMismatch { expected, actual } => io::Error::other(format!(
            "HDD copy hash mismatch: expected {expected}, got {actual}"
        )),
    }
}

fn hdd_copy_error_from_io(error: io::Error) -> HddCopyError {
    if error.kind() == io::ErrorKind::Interrupted {
        HddCopyError::Cancelled
    } else {
        HddCopyError::Io(error)
    }
}

fn restrict_object_tree_dirs(payload_parent: &Path) -> Result<(), HddCopyError> {
    set_private_dir_permissions(payload_parent)?;
    if let Some(prefix_dir) = payload_parent.parent() {
        set_private_dir_permissions(prefix_dir)?;
        if let Some(objects_dir) = prefix_dir.parent() {
            set_private_dir_permissions(objects_dir)?;
        }
    }

    Ok(())
}

pub fn verify_hdd_copy_hash(
    copy_path: impl AsRef<Path>,
    expected_content_hash: &str,
) -> Result<String, HddCopyError> {
    let actual = hash_file_sha256(copy_path)?;
    if actual != expected_content_hash {
        return Err(HddCopyError::HashMismatch {
            expected: expected_content_hash.to_string(),
            actual,
        });
    }

    Ok(actual)
}

#[cfg(test)]
mod tests {
    use super::{
        write_hdd_copies_from_reader_with_controlled_progress, write_verified_hdd_copy,
        write_verified_hdd_copy_with_controlled_progress, HddCopyError, HddCopyRequest,
        HddInlineHashCopyRequest, MAX_ACTIVE_FANOUT_WRITERS,
    };
    use crate::hash::hash_file_sha256;
    #[cfg(unix)]
    use crate::secure_fs::{PRIVATE_DIR_MODE, PRIVATE_FILE_MODE};
    use dasobjectstore_core::ids::{DiskId, ObjectId};
    use std::fs;
    use std::io::{Cursor, Read};
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::atomic::Ordering;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn writes_hdd_copy_and_verifies_hash_inline() {
        let root = temp_root("hdd-copy-ok");
        let source_path = root.join("ssd").join("payload");
        let destination_path = root.join("hdd-a").join("objects").join("object-a");
        let payload = b"bioinformatics object payload";
        fs::create_dir_all(source_path.parent().expect("source parent")).expect("source dir");
        fs::write(&source_path, payload).expect("source payload");
        let expected_hash = hash_file_sha256(&source_path).expect("source hash");
        let request = request(source_path, destination_path.clone(), expected_hash.clone());

        let report = write_verified_hdd_copy(&request).expect("verified copy");

        assert_eq!(report.object_id.as_str(), "object-a");
        assert_eq!(report.disk_id.as_str(), "disk-a");
        assert_eq!(report.copy_number, 1);
        assert_eq!(report.destination_path, destination_path);
        assert_eq!(report.bytes_written, payload.len() as u64);
        assert_eq!(report.content_hash_algorithm, "sha256");
        assert_eq!(report.content_hash, expected_hash);
        assert_eq!(
            fs::read(report.destination_path).expect("destination payload"),
            payload
        );
        #[cfg(unix)]
        assert_private_payload_tree(&destination_path);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reports_hash_mismatch_for_invalid_copy_payload() {
        let root = temp_root("hdd-copy-mismatch");
        let source_path = root.join("ssd").join("payload");
        let destination_path = root.join("hdd-a").join("objects").join("object-a");
        fs::create_dir_all(source_path.parent().expect("source parent")).expect("source dir");
        fs::write(&source_path, b"unexpected payload").expect("source payload");
        let request = request(
            source_path,
            destination_path,
            "not-the-real-hash".to_string(),
        );

        let err = write_verified_hdd_copy(&request).expect_err("hash mismatch");

        assert!(matches!(err, HddCopyError::HashMismatch { .. }));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn removes_partial_destination_when_hdd_copy_is_cancelled() {
        let root = temp_root("hdd-copy-cancelled");
        let source_path = root.join("ssd").join("payload");
        let destination_path = root.join("hdd-a").join("objects").join("object-a");
        fs::create_dir_all(source_path.parent().expect("source parent")).expect("source dir");
        fs::write(&source_path, vec![7_u8; 128 * 1024]).expect("source payload");
        let expected_hash = hash_file_sha256(&source_path).expect("source hash");
        let request = request(source_path, destination_path.clone(), expected_hash);

        let err = write_verified_hdd_copy_with_controlled_progress(&request, |_| {
            Err(HddCopyError::Cancelled)
        })
        .expect_err("copy cancelled");

        assert!(matches!(err, HddCopyError::Cancelled));
        assert!(
            !destination_path.exists(),
            "cancelled HDD copy should remove partial destination payload"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_fanout_reads_the_source_once_and_writes_each_target() {
        let root = temp_root("direct-fanout-single-read");
        let payload = vec![0x5a_u8; 192 * 1024];
        let object_id = ObjectId::new("object-a").expect("object id");
        let source_path = root.join("source.bin");
        let requests = [
            HddInlineHashCopyRequest::new(
                object_id.clone(),
                DiskId::new("disk-a").expect("disk id"),
                1,
                &source_path,
                root.join("disk-a/payload.tmp"),
            ),
            HddInlineHashCopyRequest::new(
                object_id.clone(),
                DiskId::new("disk-b").expect("disk id"),
                2,
                &source_path,
                root.join("disk-b/payload.tmp"),
            ),
            HddInlineHashCopyRequest::new(
                object_id,
                DiskId::new("disk-c").expect("disk id"),
                3,
                &source_path,
                root.join("disk-c/payload.tmp"),
            ),
        ];
        let mut source = CountingReader::new(payload.clone());
        let mut active_targets = Vec::new();
        MAX_ACTIVE_FANOUT_WRITERS.store(0, Ordering::SeqCst);

        let reports = write_hdd_copies_from_reader_with_controlled_progress(
            &mut source,
            &requests,
            |disk_id, copy_number, _| {
                active_targets.push((disk_id.as_str().to_string(), copy_number));
                Ok(())
            },
        )
        .expect("fan-out copy succeeds");

        assert_eq!(source.bytes_read, payload.len());
        assert!(
            MAX_ACTIVE_FANOUT_WRITERS.load(Ordering::SeqCst) >= 2,
            "fan-out must overlap at least two physical-disk writers"
        );
        assert_eq!(reports.len(), 3);
        for request in &requests {
            assert_eq!(
                fs::read(&request.destination_path).expect("target payload"),
                payload
            );
            assert!(active_targets
                .iter()
                .any(|(disk_id, copy_number)| disk_id == request.disk_id.as_str()
                    && *copy_number == request.copy_number));
        }

        let _ = fs::remove_dir_all(root);
    }

    struct CountingReader {
        inner: Cursor<Vec<u8>>,
        bytes_read: usize,
    }

    impl CountingReader {
        fn new(bytes: Vec<u8>) -> Self {
            Self {
                inner: Cursor::new(bytes),
                bytes_read: 0,
            }
        }
    }

    impl Read for CountingReader {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let read = self.inner.read(buffer)?;
            self.bytes_read += read;
            Ok(read)
        }
    }

    fn request(
        source_path: PathBuf,
        destination_path: PathBuf,
        expected_hash: String,
    ) -> HddCopyRequest {
        HddCopyRequest::new(
            ObjectId::new("object-a").expect("object id"),
            DiskId::new("disk-a").expect("disk id"),
            1,
            source_path,
            destination_path,
            expected_hash,
        )
    }

    fn temp_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dasobjectstore-metadata-{name}-{}-{nanos}",
            std::process::id()
        ))
    }

    #[cfg(unix)]
    fn assert_private_payload_tree(payload_path: &std::path::Path) {
        assert_eq!(
            fs::metadata(payload_path)
                .expect("payload metadata")
                .permissions()
                .mode()
                & 0o777,
            PRIVATE_FILE_MODE
        );

        let object_dir = payload_path.parent().expect("object dir");
        let prefix_dir = object_dir.parent().expect("prefix dir");
        let objects_dir = prefix_dir.parent().expect("objects dir");
        for directory in [object_dir, prefix_dir, objects_dir] {
            assert_eq!(
                fs::metadata(directory)
                    .expect("directory metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                PRIVATE_DIR_MODE
            );
        }
    }
}
