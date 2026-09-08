//! Protected public records; no plaintext credential storage or automatic recovery.
use super::*;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

pub(super) struct Directory {
    path: PathBuf,
    owner: u32,
    identity: (u64, u64),
    file: File,
}
impl Directory {
    pub(super) fn lock_publication(&self) -> Result<File, ReaderError> {
        self.check()?;
        // SAFETY: retained directory fd and static relative name. A fresh open
        // description (not dup) gives separate threads independent flock owners.
        let fd = unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                c".".as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(ReaderError::Boundary);
        }
        // SAFETY: this fresh fd has no other Rust owner.
        let lock = unsafe { File::from_raw_fd(fd) };
        let m = lock.metadata().map_err(|_| ReaderError::Boundary)?;
        if !m.is_dir() || (m.dev(), m.ino()) != self.identity {
            return Err(ReaderError::Boundary);
        }
        // SAFETY: lock is a valid owned directory fd; LOCK_NB never waits/retries.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::WouldBlock {
                    ReaderError::Conflict
                } else {
                    ReaderError::Boundary
                },
            );
        }
        self.check()?;
        Ok(lock) // closing this independent descriptor releases its advisory lock
    }
    pub(super) fn descriptor(&self) -> &File {
        &self.file
    }
    pub(super) fn require_names(&self, names: &[String]) -> Result<(), ReaderError> {
        self.check()?;
        let mut observed = std::collections::BTreeSet::new();
        for entry in fs::read_dir(&self.path).map_err(|_| ReaderError::Boundary)? {
            let entry = entry.map_err(|_| ReaderError::Boundary)?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| ReaderError::Boundary)?;
            if observed.len() >= names.len() || !observed.insert(name) {
                return Err(ReaderError::Conflict);
            }
        }
        self.check()?;
        if observed != names.iter().cloned().collect() {
            return Err(ReaderError::Conflict);
        }
        Ok(())
    }
    pub(super) fn same(&self, other: &Self) -> bool {
        self.path == other.path && self.identity == other.identity && self.owner == other.owner
    }
    pub(super) fn open(path: PathBuf, owner: u32) -> Result<Self, ReaderError> {
        if !path.is_absolute() || path.canonicalize().map_err(|_| ReaderError::Boundary)? != path {
            return Err(ReaderError::Boundary);
        }
        for (i, p) in path.ancestors().enumerate() {
            let m = fs::symlink_metadata(p).map_err(|_| ReaderError::Boundary)?;
            if !m.is_dir()
                || m.file_type().is_symlink()
                || m.mode() & 0o022 != 0
                || (i == 0 && m.uid() != owner)
                || (m.uid() != owner && m.uid() != 0)
            {
                return Err(ReaderError::Boundary);
            }
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
            .open(&path)
            .map_err(|_| ReaderError::Boundary)?;
        let m = file.metadata().map_err(|_| ReaderError::Boundary)?;
        let value = Self {
            path,
            owner,
            identity: (m.dev(), m.ino()),
            file,
        };
        value.check()?;
        Ok(value)
    }
    pub(super) fn check(&self) -> Result<(), ReaderError> {
        let m = fs::symlink_metadata(&self.path).map_err(|_| ReaderError::Boundary)?;
        if !m.is_dir()
            || m.file_type().is_symlink()
            || m.uid() != self.owner
            || m.mode() & 0o022 != 0
            || (m.dev(), m.ino()) != self.identity
        {
            return Err(ReaderError::Boundary);
        }
        Ok(())
    }
    fn child(&self, name: &str) -> Result<PathBuf, ReaderError> {
        if name.is_empty() || name.contains('/') || name == "." || name == ".." {
            return Err(ReaderError::Boundary);
        }
        self.check()?;
        Ok(self.path.join(name))
    }
    fn open_child(&self, name: &str, flags: i32) -> Result<File, ReaderError> {
        self.child(name)?;
        let name = std::ffi::CString::new(name).map_err(|_| ReaderError::Boundary)?;
        // SAFETY: live retained directory fd, valid NUL-terminated child name,
        // and an explicit mode for O_CREAT. Ownership transfers only on success.
        let fd = unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                name.as_ptr(),
                flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o644,
            )
        };
        if fd < 0 {
            return Err(
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::AlreadyExists {
                    ReaderError::Conflict
                } else {
                    ReaderError::Boundary
                },
            );
        }
        // SAFETY: fd was freshly returned by openat and has no other owner.
        Ok(unsafe { File::from_raw_fd(fd) })
    }
    pub(super) fn read(&self, name: &str, limit: usize) -> Result<Vec<u8>, ReaderError> {
        self.read_mode(name, limit, false)
    }
    pub(super) fn open_ledger(&self, name: &str) -> Result<File, ReaderError> {
        self.open_child(name, libc::O_RDONLY | libc::O_NONBLOCK)
    }
    pub(super) fn read_private(&self, name: &str, limit: usize) -> Result<Vec<u8>, ReaderError> {
        self.read_mode(name, limit, true)
    }
    pub(super) fn read_systemd_credential(
        &self,
        name: &str,
        uid: u32,
        deadline: dasobjectstore_object_service::custody::CustodyReadDeadline,
    ) -> Result<zeroize::Zeroizing<Vec<u8>>, ReaderError> {
        deadline.remaining().map_err(|_| ReaderError::Read)?;
        let mut file = self.open_child(name, libc::O_RDONLY | libc::O_NONBLOCK)?;
        super::systemd::verify_credential_file_permissions(&file, uid)?;
        let before = file.metadata().map_err(|_| ReaderError::Boundary)?;
        if before.nlink() != 1 || before.len() > 65536 {
            return Err(ReaderError::Boundary);
        }
        // Fixed allocation before reading: no reallocations can leave an old
        // plaintext allocation behind. RAII covers every subsequent error path
        // and remains active in the caller through handoff decoding.
        let mut bytes = zeroize::Zeroizing::new(vec![0; 65537]);
        let mut count = 0;
        loop {
            deadline.remaining().map_err(|_| ReaderError::Read)?;
            let n = file
                .read(&mut bytes[count..])
                .map_err(|_| ReaderError::Boundary)?;
            deadline.remaining().map_err(|_| ReaderError::Read)?;
            if n == 0 {
                break;
            }
            count += n;
            if count > 65536 {
                return Err(ReaderError::Boundary);
            }
        }
        bytes.truncate(count);
        super::systemd::verify_credential_file_permissions(&file, uid)?;
        let named = self.open_child(name, libc::O_RDONLY | libc::O_NONBLOCK)?;
        super::systemd::verify_credential_file_permissions(&named, uid)?;
        self.check()?;
        if bytes.len() > 65536
            || identity(&before) != identity(&file.metadata().map_err(|_| ReaderError::Boundary)?)
            || identity(&before) != identity(&named.metadata().map_err(|_| ReaderError::Boundary)?)
        {
            return Err(ReaderError::Boundary);
        }
        deadline.remaining().map_err(|_| ReaderError::Read)?;
        Ok(bytes)
    }
    fn read_mode(&self, name: &str, limit: usize, private: bool) -> Result<Vec<u8>, ReaderError> {
        let mut file = self.open_child(name, libc::O_RDONLY | libc::O_NONBLOCK)?;
        let m = file.metadata().map_err(|_| ReaderError::Boundary)?;
        if !m.is_file()
            || m.uid() != self.owner
            || m.mode() & if private { 0o077 } else { 0o022 } != 0
            || m.nlink() != 1
            || m.len() > limit as u64
        {
            return Err(ReaderError::Boundary);
        }
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| ReaderError::Boundary)?;
        let after = self
            .open_child(name, libc::O_RDONLY | libc::O_NONBLOCK)?
            .metadata()
            .map_err(|_| ReaderError::Boundary)?;
        self.check()?;
        if bytes.len() > limit
            || !after.is_file()
            || after.uid() != self.owner
            || after.mode() & if private { 0o077 } else { 0o022 } != 0
            || after.nlink() != 1
            || identity(&m) != identity(&after)
        {
            return Err(ReaderError::Boundary);
        }
        Ok(bytes)
    }
    pub(super) fn executable_hash(
        &self,
        name: &str,
        deadline: dasobjectstore_object_service::custody::CustodyReadDeadline,
    ) -> Result<String, ReaderError> {
        use sha2::{Digest, Sha256};
        let mut file = self.open_child(name, libc::O_RDONLY | libc::O_NONBLOCK)?;
        let before = file.metadata().map_err(|_| ReaderError::Boundary)?;
        if !before.is_file()
            || before.uid() != self.owner
            || before.mode() & 0o022 != 0
            || before.mode() & 0o111 == 0
            || before.nlink() != 1
            || before.len() > 512 * 1024 * 1024
        {
            return Err(ReaderError::Boundary);
        }
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 65536];
        let mut total = 0u64;
        loop {
            deadline.remaining().map_err(|_| ReaderError::Read)?;
            let n = file.read(&mut buffer).map_err(|_| ReaderError::Boundary)?;
            if n == 0 {
                break;
            }
            total += n as u64;
            if total > before.len() {
                return Err(ReaderError::Boundary);
            }
            hash.update(&buffer[..n]);
        }
        let after = self
            .open_child(name, libc::O_RDONLY | libc::O_NONBLOCK)?
            .metadata()
            .map_err(|_| ReaderError::Boundary)?;
        self.check()?;
        if total != before.len() || identity(&before) != identity(&after) {
            return Err(ReaderError::Boundary);
        }
        Ok(format!("{:x}", hash.finalize()))
    }
    pub(super) fn absent(&self, name: &str) -> Result<bool, ReaderError> {
        match fs::symlink_metadata(self.child(name)?) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(true),
            Ok(_) => Ok(false),
            Err(_) => Err(ReaderError::Boundary),
        }
    }
    pub(super) fn create(&self, name: &str, bytes: &[u8]) -> Result<(), ReaderError> {
        let mut file = self.open_child(name, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL)?;
        #[cfg(test)]
        {
            super::publication_faults::point(&self.path, name, "after_open")?;
            let split = bytes.len() / 2;
            file.write_all(&bytes[..split])
                .map_err(|_| ReaderError::Boundary)?;
            super::publication_faults::point(&self.path, name, "partial_write")?;
            file.write_all(&bytes[split..])
                .map_err(|_| ReaderError::Boundary)?;
            super::publication_faults::point(&self.path, name, "after_write")?;
        }
        #[cfg(not(test))]
        file.write_all(bytes).map_err(|_| ReaderError::Boundary)?;
        file.sync_all().map_err(|_| ReaderError::Boundary)?;
        #[cfg(test)]
        super::publication_faults::point(&self.path, name, "after_file_sync")?;
        self.sync()?;
        #[cfg(test)]
        super::publication_faults::point(&self.path, name, "after_dir_sync")?;
        Ok(())
    }
    pub(super) fn sync(&self) -> Result<(), ReaderError> {
        self.check()?;
        self.file.sync_all().map_err(|_| ReaderError::Boundary)?;
        self.check()
    }
    pub(super) fn remove_claim(&self) -> Result<(), ReaderError> {
        self.read("manager.claim", 0)?;
        self.check()?;
        let name = c"manager.claim";
        // SAFETY: retained directory fd and static NUL-terminated child name;
        // no recursive operation or symlink traversal occurs.
        if unsafe { libc::unlinkat(self.file.as_raw_fd(), name.as_ptr(), 0) } != 0 {
            return Err(ReaderError::Boundary);
        }
        #[cfg(test)]
        super::publication_faults::point(&self.path, "manager.claim", "after_unlink")?;
        self.sync()?;
        #[cfg(test)]
        super::publication_faults::point(&self.path, "manager.claim", "after_unlink_sync")?;
        Ok(())
    }
}
fn identity(m: &fs::Metadata) -> (u64, u64, u64, i64, i64, i64, i64, u32, u32, u64) {
    (
        m.dev(),
        m.ino(),
        m.len(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
        m.uid(),
        m.mode(),
        m.nlink(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let parent = PathBuf::from(std::env::var_os("HOME").unwrap())
                .canonicalize()
                .unwrap();
            let path = parent.join(format!(".das-reader-files-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            Self(path)
        }
        fn open(&self) -> Directory {
            // SAFETY: read-only process identity query.
            Directory::open(self.0.clone(), unsafe { libc::geteuid() }).unwrap()
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    #[test]
    fn exact_names_create_only_and_durable_claim() {
        let scratch = Scratch::new();
        let directory = scratch.open();
        directory.require_names(&[]).unwrap();
        directory.create("manager.claim", b"").unwrap();
        assert!(directory.create("manager.claim", b"overwrite").is_err());
        assert!(directory.require_names(&[]).is_err());
        directory.require_names(&["manager.claim".into()]).unwrap();
        assert_eq!(directory.read("manager.claim", 0).unwrap(), b"");
        directory.remove_claim().unwrap();
        directory.require_names(&[]).unwrap();
    }
    #[test]
    fn hardlinks_permissions_aliases_and_bounds_deny() {
        let scratch = Scratch::new();
        let directory = scratch.open();
        directory.create("record", b"public").unwrap();
        fs::set_permissions(scratch.0.join("record"), fs::Permissions::from_mode(0o644)).unwrap();
        assert!(directory.read_private("record", 6).is_err());
        fs::set_permissions(scratch.0.join("record"), fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(directory.read_private("record", 6).unwrap(), b"public");
        assert_eq!(directory.read("record", 6).unwrap(), b"public");
        assert!(directory.read("record", 5).is_err());
        fs::hard_link(scratch.0.join("record"), scratch.0.join("second")).unwrap();
        assert!(directory.read("record", 6).is_err());
        fs::remove_file(scratch.0.join("second")).unwrap();
        fs::set_permissions(scratch.0.join("record"), fs::Permissions::from_mode(0o666)).unwrap();
        assert!(directory.read("record", 6).is_err());
        symlink("record", scratch.0.join("alias")).unwrap();
        assert!(directory.read("alias", 6).is_err());
        assert!(directory.create("../escape", b"x").is_err());
    }
    #[test]
    fn replaced_parent_denies_before_new_target_write() {
        let scratch = Scratch::new();
        let directory = scratch.open();
        let old = scratch.0.with_extension("retained");
        fs::rename(&scratch.0, &old).unwrap();
        fs::create_dir(&scratch.0).unwrap();
        fs::set_permissions(&scratch.0, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(directory.create("new-record", b"x").is_err());
        assert!(!scratch.0.join("new-record").exists());
        assert!(!old.join("new-record").exists());
        fs::remove_dir(old).unwrap();
    }
}
