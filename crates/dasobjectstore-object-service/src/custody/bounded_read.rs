//! Existing-only snapshot verification. No credential acquisition or endpoint.
use super::*;
use std::time::{Duration, Instant};
#[cfg(all(test, unix))]
#[path = "bounded_read_review_tests.rs"]
mod review_tests;

/// Explicit resource budget, not custody admission or object policy.
#[derive(Clone, Copy)]
pub struct CustodyReadLimits {
    /// Maximum accepted object bytes, representable as an allocation.
    pub maximum_bytes: u64,
    /// Whole operation ceiling, nonzero and at most 300 seconds.
    pub timeout: Duration,
}
/// Monotonic deadline shared by database acquisition and the concrete reader.
#[derive(Clone, Copy)]
pub struct CustodyReadDeadline(Instant);
impl CustodyReadLimits {
    /// Validate a resource budget and start a deadline; grants no authority.
    ///
    /// # Errors
    /// Rejects zero/unrepresentable bytes and zero or greater-than-300s timeouts.
    pub fn start(self) -> Result<CustodyReadDeadline, CustodyReadError> {
        if self.maximum_bytes == 0
            || self.maximum_bytes > isize::MAX as u64
            || self.timeout.is_zero()
            || self.timeout > Duration::from_secs(300)
        {
            return Err(CustodyReadError::Input);
        }
        Ok(CustodyReadDeadline(
            Instant::now()
                .checked_add(self.timeout)
                .ok_or(CustodyReadError::Input)?,
        ))
    }
}
impl CustodyReadDeadline {
    /// Remaining duration; expiration cannot be renewed by an adapter.
    pub fn remaining(self) -> Result<Duration, CustodyReadError> {
        self.0
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or(CustodyReadError::Deadline)
    }
}
/// Fixed redacted denial; no partial bytes or backend diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CustodyReadError {
    /// Invalid budget or expected record.
    Input,
    /// Unsafe path, sidecar, missing ledger or detectable replacement.
    Boundary,
    /// Wrong schema, snapshot, receipt, event chain or policy.
    Ledger,
    /// Read acquisition, process, byte count or digest failure.
    Acquisition,
    /// Whole-call time budget expired.
    Deadline,
    /// Platform cannot enforce this boundary.
    Unsupported,
}
impl fmt::Display for CustodyReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "custody read denied: {self:?}")
    }
}
impl std::error::Error for CustodyReadError {}
/// New bounded capability; historical unbounded reader is unchanged.
pub trait BoundedCustodyObjectReader {
    /// Already-bound sealed reader identity.
    fn identity(&self) -> &str;
    /// Acquire exactly this verified object's bytes within the supplied deadline.
    fn read_bounded(
        &mut self,
        key: &str,
        length: u64,
        deadline: CustodyReadDeadline,
    ) -> Result<Vec<u8>, CustodyReadError>;
}
/// Exact snapshot evidence; not an off-NUC attestation or current-time authority.
pub struct VerifiedCustodyRead {
    /// Exact persisted receipt that was verified.
    pub receipt: CustodyIntegrityReceiptV1,
    /// Verified sealed configuration digest.
    pub configuration_sha256: String,
    /// Verified snapshot's last event digest.
    pub ledger_head_sha256: String,
    /// Exact verified body, never partial.
    pub bytes: Vec<u8>,
}

#[cfg(unix)]
#[derive(Eq, PartialEq)]
pub(super) struct Guard(Vec<(PathBuf, u64, u64, u64, i64, i64)>);
#[cfg(unix)]
impl Guard {
    pub(super) fn capture(path: &Path) -> Result<Self, CustodyReadError> {
        use std::os::unix::fs::MetadataExt;
        if !path.is_absolute()
            || path
                .canonicalize()
                .map_err(|_| CustodyReadError::Boundary)?
                != path
        {
            return Err(CustodyReadError::Boundary);
        }
        for suffix in ["-wal", "-shm", "-journal"] {
            let mut name = path.as_os_str().to_os_string();
            name.push(suffix);
            match fs::symlink_metadata(PathBuf::from(name)) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err(CustodyReadError::Boundary),
            }
        }
        let file = fs::symlink_metadata(path).map_err(|_| CustodyReadError::Boundary)?;
        if !file.is_file() || file.file_type().is_symlink() || file.mode() & 0o022 != 0 {
            return Err(CustodyReadError::Boundary);
        }
        let owner = file.uid();
        let mut identity = vec![(
            path.to_path_buf(),
            file.dev(),
            file.ino(),
            file.len(),
            file.mtime(),
            file.mtime_nsec(),
        )];
        for parent in path.ancestors().skip(1) {
            let m = fs::symlink_metadata(parent).map_err(|_| CustodyReadError::Boundary)?;
            if !m.is_dir()
                || m.file_type().is_symlink()
                || (m.uid() != 0 && m.uid() != owner)
                || m.mode() & 0o022 != 0
            {
                return Err(CustodyReadError::Boundary);
            }
            identity.push((parent.to_path_buf(), m.dev(), m.ino(), 0, 0, 0));
        }
        Ok(Self(identity))
    }
}

pub(super) fn schema(
    connection: &Connection,
) -> Result<Vec<(String, String, String, Option<String>)>, CustodyReadError> {
    connection
        .prepare("SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name,tbl_name")
        .map_err(|_| CustodyReadError::Ledger)?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .map_err(|_| CustodyReadError::Ledger)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| CustodyReadError::Ledger)
}

/// Verify one existing quiesced ledger snapshot and exact bounded object bytes.
///
/// # Errors
/// Denies unsafe/missing/changed paths, sidecars, schema or record mismatch,
/// acquisition faults and deadline expiry without creating or repairing state.
#[cfg(unix)]
pub fn verify_custody_readback_existing(
    path: &Path,
    expected: &CustodyIntegrityReceiptV1,
    reader: &mut impl BoundedCustodyObjectReader,
    limits: CustodyReadLimits,
) -> Result<VerifiedCustodyRead, CustodyReadError> {
    if limits.maximum_bytes == 0
        || limits.maximum_bytes > isize::MAX as u64
        || expected.content_length == 0
        || expected.content_length > limits.maximum_bytes
        || limits.timeout.is_zero()
        || limits.timeout > Duration::from_secs(300)
    {
        return Err(CustodyReadError::Input);
    }
    let deadline = limits.start()?;
    let guard = Guard::capture(path)?;
    let mut connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| CustodyReadError::Ledger)?;
    configure_read_connection(&connection, deadline)?;
    let transaction = connection
        .transaction()
        .map_err(|_| CustodyReadError::Ledger)?;
    let mut template = Connection::open_in_memory().map_err(|_| CustodyReadError::Ledger)?;
    initialise_schema(&mut template).map_err(|_| CustodyReadError::Ledger)?;
    if schema(&transaction)? != schema(&template)? {
        return Err(CustodyReadError::Ledger);
    }
    deadline.remaining()?;
    let configuration =
        read_sealed_configuration_from(&transaction).map_err(|_| CustodyReadError::Ledger)?;
    verify_event_chain_checked(&transaction, || {
        deadline
            .remaining()
            .map(|_| ())
            .map_err(|_| invalid("custody read deadline"))
    })
    .map_err(|_| {
        if deadline.remaining().is_err() {
            CustodyReadError::Deadline
        } else {
            CustodyReadError::Ledger
        }
    })?;
    validate_reader_identity(&configuration.profile, reader.identity())
        .map_err(|_| CustodyReadError::Ledger)?;
    let stored = latest_object_version(&transaction, &expected.object_id)
        .map_err(|_| CustodyReadError::Ledger)?
        .ok_or(CustodyReadError::Ledger)?;
    let persisted = existing_receipt(&transaction, &expected.object_id, stored.version)
        .map_err(|_| CustodyReadError::Ledger)?
        .ok_or(CustodyReadError::Ledger)?;
    let (receipt_jcs, receipt_sha256): (String, String) = transaction.query_row(
        "SELECT receipt_jcs,receipt_sha256 FROM custody_readback_receipts WHERE object_id=?1 AND version=?2",
        params![expected.object_id, stored.version], |r| Ok((r.get(0)?,r.get(1)?))).map_err(|_| CustodyReadError::Ledger)?;
    if sha256_hex(receipt_jcs.as_bytes()) != receipt_sha256
        || canonical_json(&persisted).map_err(|_| CustodyReadError::Ledger)? != receipt_jcs
    {
        return Err(CustodyReadError::Ledger);
    }
    if &persisted != expected
        || expected.schema != CUSTODY_RECEIPT_SCHEMA_V1
        || expected.assurance_class != CUSTODY_ASSURANCE_CLASS_LOCAL_TRUSTED_ADMINISTRATOR_OVERLAY
        || expected.store_id != configuration.store_id
        || expected.bucket_name != configuration.bucket_name
        || expected.target_id != configuration.profile.target_id
        || expected.version != stored.version
    {
        return Err(CustodyReadError::Ledger);
    }
    let head = transaction
        .query_row(
            "SELECT event_sha256 FROM custody_events ORDER BY sequence DESC LIMIT 1",
            [],
            |r| r.get::<_, String>(0),
        )
        .map_err(|_| CustodyReadError::Ledger)?;
    let configuration_sha256 =
        sealed_configuration_sha256(&configuration).map_err(|_| CustodyReadError::Ledger)?;
    deadline.remaining()?;
    if Guard::capture(path)? != guard {
        return Err(CustodyReadError::Boundary);
    }
    let bytes = reader.read_bounded(&expected.object_key, expected.content_length, deadline)?;
    deadline.remaining()?;
    if u64::try_from(bytes.len()).map_err(|_| CustodyReadError::Acquisition)?
        != expected.content_length
    {
        return Err(CustodyReadError::Acquisition);
    }
    verify_receipt_fields(
        &configuration,
        &stored,
        expected,
        &CustodyReadbackObservationV1 {
            reader_identity: reader.identity().to_string(),
            observed_at_utc: expected.observed_at_utc.clone(),
            content_sha256: sha256_hex(&bytes),
            content_length: bytes.len() as u64,
        },
    )
    .map_err(|_| CustodyReadError::Acquisition)?;
    if Guard::capture(path)? != guard {
        return Err(CustodyReadError::Boundary);
    }
    transaction.commit().map_err(|_| CustodyReadError::Ledger)?;
    deadline.remaining()?;
    Ok(VerifiedCustodyRead {
        receipt: persisted,
        configuration_sha256,
        ledger_head_sha256: head,
        bytes,
    })
}

/// Unsupported platforms fail before filesystem or reader effects.
#[cfg(not(unix))]
pub fn verify_custody_readback_existing(
    _path: &Path,
    _expected: &CustodyIntegrityReceiptV1,
    _reader: &mut impl BoundedCustodyObjectReader,
    _limits: CustodyReadLimits,
) -> Result<VerifiedCustodyRead, CustodyReadError> {
    Err(CustodyReadError::Unsupported)
}

pub(super) fn configure_read_connection(
    connection: &Connection,
    deadline: CustodyReadDeadline,
) -> Result<(), CustodyReadError> {
    deadline.remaining()?;
    connection.progress_handler(1000, Some(move || deadline.remaining().is_err()));
    // No busy wait renews the remaining whole-operation budget.
    connection
        .busy_timeout(Duration::ZERO)
        .map_err(|_| CustodyReadError::Ledger)?;
    connection
        .execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF;")
        .map_err(|_| CustodyReadError::Ledger)
}
