//! Initial protected publication and actual continuation read composition.
//! No activation, key provisioning, backend revocation or generation rotation.
mod ciphertext;
pub mod endpoint;
mod files;
#[cfg(test)]
mod manager_tests;
#[cfg(test)]
mod publication_faults;
mod systemd;
use super::custody_garage::{GarageCustodyS3Reader, SystemdServiceCredentialHandoffResolver};
use super::service::ServiceCommandRunner;
pub use ciphertext::CredentialProtection;
use dasobjectstore_object_service::custody::{
    verify_custody_readback_existing, verify_reader_seal_existing, CustodyReadLimits,
    VerifiedCustodyRead,
};
use dasobjectstore_object_service::custody_reader::{
    raw_sha256, ReaderBindingV1, ReaderCurrentV1, ReaderError, ReaderSealV1,
};
use dasobjectstore_object_service::CustodyIntegrityReceiptV1;
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Explicit protected installation selection. These public facts must originate
/// from the admitted companion, not an HTTP request; no admission is minted here.
pub struct ReaderSelection {
    /// Exact accepted binding; encrypted source has already been provisioned separately.
    pub binding: ReaderBindingV1,
    /// Actual manager-owned existing directory, excluded from reader writes/restores.
    pub directory: PathBuf,
    /// Independently configured manager UID, distinct from the reader.
    pub manager_uid: u32,
    /// Exact existing custody ledger.
    pub ledger: PathBuf,
    /// Selected existing backend inventory's digest and bytes are admitted upstream.
    pub inventory: Vec<(String, u64)>,
    /// Digest supplied with those objects by the trusted admitted-companion decoder.
    /// This low-level composition does not authenticate arbitrary inventory bytes.
    pub selected_inventory_sha256: String,
    /// Protected encrypted source selected by the admitted companion; never read
    /// by the service. Its exact path must match actual systemd unit delivery.
    pub encrypted_source: PathBuf,
    /// Explicit protection selected at provisioning, checked against stored mode.
    pub protection: CredentialProtection,
    /// Backend URL selected by the admitted companion. Its independently measured
    /// authority/TLS digests are not hashes of this string and are not inferred here.
    pub backend_endpoint: String,
    /// Canonical qualified AWS helper path and its selected raw executable hash.
    pub aws_executable: PathBuf,
    /// Raw SHA256 from the companion's qualified helper provenance.
    pub aws_executable_sha256: String,
}

/// Actual initial public-record publisher. It does not create its root, credential,
/// backend grant or approval. Only an already authorized manager may invoke it.
pub struct ReaderManager {
    directory: files::Directory,
}
impl ReaderManager {
    /// Open the existing manager-owned directory using actual current process UID.
    ///
    /// # Errors
    /// Denies unsafe ownership, ancestors or aliases. No directories are created.
    pub fn open(path: PathBuf) -> Result<Self, ReaderError> {
        // SAFETY: geteuid has no memory-safety preconditions and mutates no state.
        let owner = unsafe { libc::geteuid() };
        Ok(Self {
            directory: files::Directory::open(path, owner)?,
        })
    }
    /// Publish the actual complete initial seal and selected current record once.
    /// No existing state is adopted or overwritten. An incomplete publication
    /// retains its claim; failure after removing the claim may leave the complete
    /// publication with a lost return. Both deny re-entry; no retry is supplied.
    ///
    /// # Errors
    /// Denies mismatches before writes; later faults preserve incomplete evidence.
    pub fn publish_initial(
        &self,
        selection: &ReaderSelection,
        seal: &ReaderSealV1,
        current: &ReaderCurrentV1,
        limits: CustodyReadLimits,
    ) -> Result<(), ReaderError> {
        let deadline = limits.start().map_err(|_| ReaderError::Read)?;
        let _publication_lock = self.directory.lock_publication()?;
        let guard = files::Directory::open(selection.directory.clone(), selection.manager_uid)?;
        if selection.binding.uid == selection.manager_uid
            || selection.selected_inventory_sha256 != selection.binding.inventory_sha256
        {
            return Err(ReaderError::Binding);
        }
        selection.binding.verify(current, seal, &clock_now())?;
        ciphertext::verify(
            &selection.encrypted_source,
            selection.manager_uid,
            &selection.binding.encrypted_source_sha256,
            selection.protection,
            deadline,
        )?;
        verify_reader_seal_existing(
            &selection.ledger,
            seal,
            &selection.binding.reader_identity,
            &selection.inventory,
            deadline,
        )
        .map_err(|_| ReaderError::Read)?;
        // Check actual manager path correspondence, not just caller facts.
        if guard.read("current.jcs", 16384).is_ok()
            || !guard.absent("current.jcs")?
            || !self.directory.same(&guard)
        {
            return Err(ReaderError::Conflict);
        }
        self.directory.require_names(&[])?;
        #[cfg(test)]
        publication_faults::point(&selection.directory, "manager", "after_empty")?;
        self.directory.create("manager.claim", &[])?;
        // Recheck the whole immutable selection after winning the external claim.
        selection.binding.verify(current, seal, &clock_now())?;
        ciphertext::verify(
            &selection.encrypted_source,
            selection.manager_uid,
            &selection.binding.encrypted_source_sha256,
            selection.protection,
            deadline,
        )?;
        verify_reader_seal_existing(
            &selection.ledger,
            seal,
            &selection.binding.reader_identity,
            &selection.inventory,
            deadline,
        )
        .map_err(|_| ReaderError::Read)?;
        let seal_raw = seal.encode()?;
        let binding_raw = selection.binding.encode()?;
        let current_raw = current.encode()?;
        self.directory
            .create(&format!("seal-{}.jcs", raw_sha256(&seal_raw)), &seal_raw)?;
        self.directory.create(
            &format!("binding-{}.jcs", raw_sha256(&binding_raw)),
            &binding_raw,
        )?;
        self.directory.create(
            &format!("state-{}.jcs", raw_sha256(&current_raw)),
            &current_raw,
        )?;
        deadline.remaining().map_err(|_| ReaderError::Read)?;
        selection.binding.verify(current, seal, &clock_now())?;
        self.directory.create("current.jcs", &current_raw)?;
        self.directory.remove_claim()
    }
}

/// Restartable read composition loaded from the real systemd credential directory.
/// The selected current generation is immutable for this object; drift denies reads.
pub struct ReaderContinuation<'a, R> {
    selection: ReaderSelection,
    directory: files::Directory,
    current_raw: Vec<u8>,
    seal: ReaderSealV1,
    reader: super::custody_garage::BoundedGarageCustodyReader<'a, R>,
    last_time: String,
}
impl<'a, R: ServiceCommandRunner> ReaderContinuation<'a, R> {
    /// Load a separately provisioned continuation credential, never a consumed handoff.
    /// Endpoint/executable/scratch are exact trusted installation inputs, not API fields.
    ///
    /// # Errors
    /// Denies wrong identity/current selection/seal/credential or unsafe boundaries.
    pub fn load(
        selection: ReaderSelection,
        runner: &'a R,
        scratch: &Path,
        limits: CustodyReadLimits,
    ) -> Result<Self, ReaderError> {
        let now = clock_now();
        let deadline = limits.start().map_err(|_| ReaderError::Read)?;
        // SAFETY: geteuid has no memory-safety preconditions.
        if unsafe { libc::geteuid() } != selection.binding.uid
            || selection.manager_uid == selection.binding.uid
        {
            return Err(ReaderError::Binding);
        }
        let directory = files::Directory::open(selection.directory.clone(), selection.manager_uid)?;
        let (current_raw, seal) = load_records(&directory, &selection, &now)?;
        verify_aws(&selection, deadline)?;
        verify_reader_seal_existing(
            &selection.ledger,
            &seal,
            &selection.binding.reader_identity,
            &selection.inventory,
            deadline,
        )
        .map_err(|_| ReaderError::Read)?;
        let credential_path = PathBuf::from(
            std::env::var_os(super::custody_garage::SYSTEMD_CREDENTIALS_DIRECTORY_ENV)
                .ok_or(ReaderError::Boundary)?,
        );
        use std::os::unix::fs::MetadataExt;
        let owner = fs::symlink_metadata(&credential_path)
            .map_err(|_| ReaderError::Boundary)?
            .uid();
        if owner != 0 && owner != selection.binding.uid {
            return Err(ReaderError::Boundary);
        }
        let credentials = files::Directory::open(credential_path.clone(), owner)?;
        systemd::verify(
            &selection.binding,
            &selection.encrypted_source,
            credentials.descriptor(),
            &credential_path,
            deadline,
        )?;
        let mut secret = credentials.read_systemd_credential(
            &selection.binding.credential_name,
            selection.binding.uid,
            deadline,
        )?;
        let decoded = SystemdServiceCredentialHandoffResolver::decode_continuation(
            &selection.binding,
            &secret,
        );
        secret.fill(0);
        let credential = decoded?;
        systemd::verify(
            &selection.binding,
            &selection.encrypted_source,
            credentials.descriptor(),
            &credential_path,
            deadline,
        )?;
        let (identity, environment) = credential.into_parts();
        let reader = GarageCustodyS3Reader::new(
            runner,
            &selection.backend_endpoint,
            &selection.binding.bucket_name,
            identity,
            environment,
            scratch,
        )
        .into_bounded(selection.aws_executable.clone())
        .map_err(|_| ReaderError::Read)?;
        let value = Self {
            selection,
            directory,
            current_raw,
            seal,
            reader,
            last_time: now,
        };
        value.recheck(&clock_now())?;
        deadline.remaining().map_err(|_| ReaderError::Read)?;
        Ok(value)
    }
    fn recheck(&self, now: &str) -> Result<(), ReaderError> {
        let (raw, seal) = load_records(&self.directory, &self.selection, now)?;
        if raw != self.current_raw || seal != self.seal || now < self.last_time.as_str() {
            return Err(ReaderError::Binding);
        }
        Ok(())
    }
    /// Verify a selected existing receipt and acquire actual bounded Garage bytes.
    /// Fresh protected selection is checked before and after; no success on drift.
    /// The actual system clock is checked freshly at both effect boundaries.
    ///
    /// # Errors
    /// Denies foreign receipt, stale generation, ledger drift or acquisition failure.
    pub fn read(
        &mut self,
        receipt: &CustodyIntegrityReceiptV1,
        limits: CustodyReadLimits,
    ) -> Result<VerifiedCustodyRead, ReaderError> {
        self.read_inner(receipt, limits, None)
    }
    fn read_inner(
        &mut self,
        receipt: &CustodyIntegrityReceiptV1,
        limits: CustodyReadLimits,
        raw_ledger_digest: Option<&str>,
    ) -> Result<VerifiedCustodyRead, ReaderError> {
        let deadline = limits.start().map_err(|_| ReaderError::Read)?;
        let now = clock_now();
        self.recheck(&now)?;
        self.last_time = now;
        verify_aws(&self.selection, deadline)?;
        if !self.seal.receipt_jcs_sha256.contains(&raw_sha256(
            &serde_jcs::to_vec(receipt).map_err(|_| ReaderError::Format)?,
        )) {
            return Err(ReaderError::Binding);
        }
        verify_reader_seal_existing(
            &self.selection.ledger,
            &self.seal,
            &self.selection.binding.reader_identity,
            &self.selection.inventory,
            deadline,
        )
        .map_err(|_| ReaderError::Read)?;
        let limits = CustodyReadLimits {
            maximum_bytes: limits.maximum_bytes,
            timeout: deadline.remaining().map_err(|_| ReaderError::Read)?,
        };
        let value = if let Some(digest) = raw_ledger_digest {
            use std::os::unix::fs::MetadataExt;
            let parent = self
                .selection
                .ledger
                .parent()
                .ok_or(ReaderError::Boundary)?;
            let owner = fs::symlink_metadata(parent)
                .map_err(|_| ReaderError::Boundary)?
                .uid();
            let directory = files::Directory::open(parent.to_path_buf(), owner)?;
            let name = self
                .selection
                .ledger
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or(ReaderError::Boundary)?;
            let mut ledger = directory.open_ledger(name)?;
            dasobjectstore_object_service::custody::verify_custody_readback_existing_bound(
                &self.selection.ledger,
                receipt,
                &mut self.reader,
                limits,
                &mut ledger,
                digest,
            )
        } else {
            verify_custody_readback_existing(
                &self.selection.ledger,
                receipt,
                &mut self.reader,
                limits,
            )
        }
        .map_err(|_| ReaderError::Read)?;
        if value.ledger_head_sha256 != self.seal.ledger_head_sha256
            || value.configuration_sha256 != self.seal.configuration_sha256
        {
            return Err(ReaderError::Binding);
        }
        let now = clock_now();
        self.recheck(&now)?;
        deadline.remaining().map_err(|_| ReaderError::Read)?;
        self.last_time = now;
        Ok(value)
    }
}
fn load_records(
    directory: &files::Directory,
    selection: &ReaderSelection,
    now: &str,
) -> Result<(Vec<u8>, ReaderSealV1), ReaderError> {
    if !directory.absent("manager.claim")? {
        return Err(ReaderError::Conflict);
    }
    if selection.selected_inventory_sha256 != selection.binding.inventory_sha256 {
        return Err(ReaderError::Binding);
    }
    let raw = directory.read("current.jcs", 16384)?;
    let current = ReaderCurrentV1::decode(&raw)?;
    if directory.read(&format!("state-{}.jcs", raw_sha256(&raw)), 16384)? != raw {
        return Err(ReaderError::Binding);
    }
    let binding_raw = directory.read(&format!("binding-{}.jcs", current.binding_sha256), 16384)?;
    if binding_raw != selection.binding.encode()? {
        return Err(ReaderError::Binding);
    }
    let seal = ReaderSealV1::decode(&directory.read(
        &format!("seal-{}.jcs", selection.binding.seal_sha256),
        1048576,
    )?)?;
    directory.require_names(&[
        "current.jcs".to_owned(),
        format!("state-{}.jcs", raw_sha256(&raw)),
        format!("binding-{}.jcs", current.binding_sha256),
        format!("seal-{}.jcs", selection.binding.seal_sha256),
    ])?;
    selection.binding.verify(&current, &seal, now)?;
    Ok((raw, seal))
}
fn clock_now() -> String {
    chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
fn verify_aws(
    selection: &ReaderSelection,
    deadline: dasobjectstore_object_service::custody::CustodyReadDeadline,
) -> Result<(), ReaderError> {
    let path = &selection.aws_executable;
    if !path.is_absolute() || path.canonicalize().map_err(|_| ReaderError::Boundary)? != *path {
        return Err(ReaderError::Boundary);
    }
    use std::os::unix::fs::MetadataExt;
    let owner = fs::symlink_metadata(path)
        .map_err(|_| ReaderError::Boundary)?
        .uid();
    if owner != 0 && owner != selection.manager_uid {
        return Err(ReaderError::Boundary);
    }
    let parent = files::Directory::open(
        path.parent().ok_or(ReaderError::Boundary)?.to_path_buf(),
        owner,
    )?;
    let name = path
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or(ReaderError::Boundary)?;
    if parent.executable_hash(name, deadline)? != selection.aws_executable_sha256 {
        return Err(ReaderError::Binding);
    }
    Ok(())
}
