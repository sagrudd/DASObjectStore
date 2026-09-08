//! Direct bounded Unix GET process; legacy runner behavior is not changed.
use super::*;
use dasobjectstore_object_service::custody::{
    BoundedCustodyObjectReader, CustodyReadDeadline, CustodyReadError,
};
use std::io::Read;
use std::process::{Child, Command, Stdio};
#[cfg(all(test, unix))]
mod tests;

/// A concrete bounded view of an already supplied Garage reader credential.
/// It never resolves/reopens a handoff or selects a lifecycle.
pub struct BoundedGarageCustodyReader<'a, R> {
    reader: GarageCustodyS3Reader<'a, R>,
    executable: PathBuf,
}
impl<'a, R> GarageCustodyS3Reader<'a, R> {
    /// Consume this in-memory reader into a bounded acquisition adapter.
    /// Executable selection is trusted composition, not proven provenance.
    ///
    /// # Errors
    /// Rejects a nonabsolute, missing, aliased or nonregular executable.
    pub fn into_bounded(
        self,
        executable: PathBuf,
    ) -> Result<BoundedGarageCustodyReader<'a, R>, CustodyReadError> {
        if !executable.is_absolute()
            || executable
                .canonicalize()
                .map_err(|_| CustodyReadError::Boundary)?
                != executable
            || !fs::symlink_metadata(&executable)
                .map_err(|_| CustodyReadError::Boundary)?
                .is_file()
        {
            return Err(CustodyReadError::Boundary);
        }
        Ok(BoundedGarageCustodyReader {
            reader: self,
            executable,
        })
    }
}
impl<R> BoundedCustodyObjectReader for BoundedGarageCustodyReader<'_, R> {
    fn identity(&self) -> &str {
        &self.reader.identity
    }
    fn read_bounded(
        &mut self,
        key: &str,
        length: u64,
        deadline: CustodyReadDeadline,
    ) -> Result<Vec<u8>, CustodyReadError> {
        #[cfg(unix)]
        {
            acquire(&self.executable, &self.reader, key, length, deadline)
        }
        #[cfg(not(unix))]
        {
            let _ = (key, length, deadline);
            Err(CustodyReadError::Unsupported)
        }
    }
}

#[cfg(unix)]
struct Scratch {
    directory: PathBuf,
    fifo: PathBuf,
    directory_identity: (u64, u64),
    fifo_identity: Option<(u64, u64)>,
}
#[cfg(unix)]
impl Scratch {
    fn create(root: &Path) -> Result<Self, CustodyReadError> {
        use std::os::unix::{
            ffi::OsStrExt,
            fs::{DirBuilderExt, MetadataExt},
        };
        if !root.is_absolute()
            || root
                .canonicalize()
                .map_err(|_| CustodyReadError::Boundary)?
                != root
        {
            return Err(CustodyReadError::Boundary);
        }
        for parent in root.ancestors() {
            let m = fs::symlink_metadata(parent).map_err(|_| CustodyReadError::Boundary)?;
            if !m.is_dir() || m.file_type().is_symlink() || m.mode() & 0o022 != 0 {
                return Err(CustodyReadError::Boundary);
            }
        }
        let directory = root.join(format!(
            "bounded-get-{}-{}",
            std::process::id(),
            CUSTODY_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .map_err(|_| CustodyReadError::Boundary)?;
        let metadata = fs::symlink_metadata(&directory).map_err(|_| CustodyReadError::Boundary)?;
        let mut value = Self {
            fifo: directory.join("body"),
            directory,
            directory_identity: (metadata.dev(), metadata.ino()),
            fifo_identity: None,
        };
        let path = std::ffi::CString::new(value.fifo.as_os_str().as_bytes())
            .map_err(|_| CustodyReadError::Boundary)?;
        // SAFETY: CString is terminated and lives through the call. The directory
        // is exclusively create-new owned; mode creates only this FIFO.
        if unsafe { libc::mkfifo(path.as_ptr(), 0o600) } != 0 {
            return Err(CustodyReadError::Boundary);
        }
        let metadata = fs::symlink_metadata(&value.fifo).map_err(|_| CustodyReadError::Boundary)?;
        value.fifo_identity = Some((metadata.dev(), metadata.ino()));
        Ok(value)
    }
    fn cleanup(&self) -> Result<(), CustodyReadError> {
        // No recursive removal or adoption. Missing/replaced output is a denial.
        use std::os::unix::fs::{FileTypeExt, MetadataExt};
        let parent =
            fs::symlink_metadata(&self.directory).map_err(|_| CustodyReadError::Boundary)?;
        if !parent.is_dir() || (parent.dev(), parent.ino()) != self.directory_identity {
            return Err(CustodyReadError::Boundary);
        }
        if self.fifo_identity.is_none() {
            return fs::remove_dir(&self.directory).map_err(|_| CustodyReadError::Boundary);
        }
        let m = fs::symlink_metadata(&self.fifo).map_err(|_| CustodyReadError::Boundary)?;
        if !m.file_type().is_fifo() || Some((m.dev(), m.ino())) != self.fifo_identity {
            return Err(CustodyReadError::Boundary);
        }
        fs::remove_file(&self.fifo).map_err(|_| CustodyReadError::Boundary)?;
        fs::remove_dir(&self.directory).map_err(|_| CustodyReadError::Boundary)
    }
}
#[cfg(unix)]
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

#[cfg(unix)]
struct OwnedProcess(Child);
#[cfg(unix)]
impl OwnedProcess {
    fn observed_success(&self) -> Result<Option<bool>, CustodyReadError> {
        // SAFETY: siginfo_t is a C output POD; waitid initializes it. WNOWAIT
        // observes only our direct child without reaping, keeping its PID and
        // process-group identity reserved until group cleanup in Drop.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                self.0.id(),
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result != 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                return Ok(None);
            }
            return Err(CustodyReadError::Acquisition);
        }
        // SAFETY: successful waitid populates the child-event union, or zero PID
        // when no event is available under WNOHANG.
        if unsafe { info.si_pid() } == 0 {
            return Ok(None);
        }
        Ok(Some(
            info.si_code == libc::CLD_EXITED && unsafe { info.si_status() } == 0,
        ))
    }
}
#[cfg(unix)]
impl Drop for OwnedProcess {
    fn drop(&mut self) {
        if let Ok(pid) = i32::try_from(self.0.id()) {
            // SAFETY: the child was launched into its own process group with id
            // equal to its PID. No caller chooses the signal target.
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
        let _ = self.0.wait();
    }
}
#[cfg(unix)]
fn nonblocking(fd: &impl std::os::fd::AsRawFd) -> Result<(), CustodyReadError> {
    // SAFETY: descriptor is exclusively owned, valid throughout both calls;
    // flags are changed only on this operation's pipe, never shared caller I/O.
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(CustodyReadError::Acquisition);
    }
    Ok(())
}

#[cfg(unix)]
fn drain(
    reader: &mut impl Read,
    count: &mut usize,
    limit: usize,
    mut body: Option<&mut Vec<u8>>,
) -> Result<bool, CustodyReadError> {
    let mut buffer = [0u8; 8192];
    // One bounded quantum lets the other pipes and deadline make progress even
    // when a producer continuously floods one stream.
    let allowed = limit
        .saturating_sub(*count)
        .saturating_add(1)
        .min(buffer.len());
    match reader.read(&mut buffer[..allowed]) {
        Ok(0) => Ok(true),
        Ok(n) => {
            *count = count.checked_add(n).ok_or(CustodyReadError::Acquisition)?;
            if *count > limit {
                return Err(CustodyReadError::Acquisition);
            }
            if let Some(bytes) = body.as_mut() {
                bytes.extend_from_slice(&buffer[..n]);
            }
            Ok(false)
        }
        Err(e)
            if e.kind() == std::io::ErrorKind::WouldBlock
                || e.kind() == std::io::ErrorKind::Interrupted =>
        {
            Ok(false)
        }
        Err(_) => Err(CustodyReadError::Acquisition),
    }
}

#[cfg(unix)]
fn acquire<R>(
    executable: &Path,
    reader: &GarageCustodyS3Reader<'_, R>,
    key: &str,
    length: u64,
    deadline: CustodyReadDeadline,
) -> Result<Vec<u8>, CustodyReadError> {
    use std::os::unix::{
        fs::{MetadataExt, OpenOptionsExt},
        process::CommandExt,
    };
    let length = usize::try_from(length).map_err(|_| CustodyReadError::Input)?;
    if length == 0 || length > isize::MAX as usize {
        return Err(CustodyReadError::Input);
    }
    deadline.remaining()?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| CustodyReadError::Input)?;
    let scratch = Scratch::create(&reader.scratch_root)?;
    let before = fs::symlink_metadata(&scratch.fifo).map_err(|_| CustodyReadError::Boundary)?;
    let mut fifo = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(&scratch.fifo)
        .map_err(|_| CustodyReadError::Boundary)?;
    let mut command = Command::new(executable);
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("AWS_CONFIG_FILE", "/dev/null")
        .env("AWS_SHARED_CREDENTIALS_FILE", "/dev/null")
        .env("AWS_EC2_METADATA_DISABLED", "true")
        .env("AWS_MAX_ATTEMPTS", "1")
        .env("AWS_PAGER", "")
        .env("AWS_CLI_AUTO_PROMPT", "off");
    for (name, value) in &reader.environment {
        if !matches!(
            name.as_str(),
            "AWS_ACCESS_KEY_ID"
                | "AWS_SECRET_ACCESS_KEY"
                | "AWS_SESSION_TOKEN"
                | "AWS_DEFAULT_REGION"
                | "AWS_REGION"
        ) {
            return Err(CustodyReadError::Input);
        }
        command.env(name, value);
    }
    command
        .args([
            "s3api",
            "get-object",
            "--bucket",
            &reader.bucket,
            "--key",
            key,
            "--endpoint-url",
            &reader.endpoint,
        ])
        .arg(&scratch.fifo)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut process = OwnedProcess(command.spawn().map_err(|_| CustodyReadError::Acquisition)?);
    let mut stdout = process
        .0
        .stdout
        .take()
        .ok_or(CustodyReadError::Acquisition)?;
    let mut stderr = process
        .0
        .stderr
        .take()
        .ok_or(CustodyReadError::Acquisition)?;
    nonblocking(&stdout)?;
    nonblocking(&stderr)?;
    let (mut body_count, mut out_count, mut err_count) = (0, 0, 0);
    loop {
        deadline.remaining()?;
        let body_eof = drain(&mut fifo, &mut body_count, length, Some(&mut bytes))?;
        let out_eof = drain(&mut stdout, &mut out_count, 65536, None)?;
        let err_eof = drain(&mut stderr, &mut err_count, 65536, None)?;
        if let Some(success) = process.observed_success()? {
            if !success {
                return Err(CustodyReadError::Acquisition);
            }
            if body_eof && out_eof && err_eof {
                if body_count != length {
                    return Err(CustodyReadError::Acquisition);
                }
                break;
            }
        }
        std::thread::sleep(
            deadline
                .remaining()?
                .min(std::time::Duration::from_millis(2)),
        );
    }
    let after = fs::symlink_metadata(&scratch.fifo).map_err(|_| CustodyReadError::Boundary)?;
    if (before.dev(), before.ino()) != (after.dev(), after.ino()) {
        return Err(CustodyReadError::Boundary);
    }
    drop(process);
    drop(fifo);
    scratch.cleanup()?;
    deadline.remaining()?;
    Ok(bytes)
}
