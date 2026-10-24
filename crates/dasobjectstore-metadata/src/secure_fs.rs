use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

#[cfg(unix)]
pub(crate) const PRIVATE_DIR_MODE: u32 = 0o770;
#[cfg(unix)]
pub(crate) const PRIVATE_FILE_MODE: u32 = 0o660;

pub(crate) fn create_private_dir_all(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    set_private_dir_permissions(path)
}

/// Create generic copy parents without changing any existing directory mode.
/// Existing directory symlinks retain their previous resolution semantics.
pub(crate) fn create_copy_parent_all(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::mem::MaybeUninit;
        use std::os::fd::{AsRawFd, FromRawFd};
        use std::os::unix::ffi::OsStrExt;
        use std::path::Component;

        let new_flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC;
        // Linux O_PATH preserves traversal of existing search-only directories.
        #[cfg(target_os = "linux")]
        let flags = libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC;
        #[cfg(not(target_os = "linux"))]
        let flags = new_flags;
        let mut parent = OpenOptions::new()
            .read(true)
            .custom_flags(flags)
            .open(if path.is_absolute() { "/" } else { "." })?;
        for component in path.components() {
            let name = match component {
                Component::Normal(name) => name,
                Component::ParentDir => std::ffi::OsStr::new(".."),
                Component::RootDir | Component::CurDir => continue,
                Component::Prefix(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "unsupported copy parent prefix",
                    ))
                }
            };
            let name = CString::new(name.as_bytes())
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in copy parent"))?;
            // Existing components, including directory aliases, are opened only.
            let mut fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
            if fd >= 0 {
                // SAFETY: openat returned a new owned descriptor.
                parent = unsafe { File::from_raw_fd(fd) };
                continue;
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::NotFound {
                return Err(error);
            }
            // Keep the new node private until its owner and inode are checked.
            let created = unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } == 0;
            if !created {
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::AlreadyExists {
                    return Err(error);
                }
            }
            let mut before = MaybeUninit::<libc::stat>::uninit();
            if created
                && unsafe {
                    libc::fstatat(
                        parent.as_raw_fd(),
                        name.as_ptr(),
                        before.as_mut_ptr(),
                        libc::AT_SYMLINK_NOFOLLOW,
                    )
                } != 0
            {
                return Err(io::Error::last_os_error());
            }
            fd = unsafe {
                libc::openat(
                    parent.as_raw_fd(),
                    name.as_ptr(),
                    if created {
                        new_flags | libc::O_NOFOLLOW
                    } else {
                        flags
                    },
                )
            };
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: openat returned a new owned descriptor; File closes it on every error.
            let next = unsafe { File::from_raw_fd(fd) };
            if created {
                let mut after = MaybeUninit::<libc::stat>::uninit();
                if unsafe { libc::fstat(next.as_raw_fd(), after.as_mut_ptr()) } != 0 {
                    return Err(io::Error::last_os_error());
                }
                // SAFETY: both stat calls above succeeded in this branch.
                let (before, after) = unsafe { (before.assume_init(), after.assume_init()) };
                if before.st_dev != after.st_dev
                    || before.st_ino != after.st_ino
                    || after.st_uid != unsafe { libc::geteuid() }
                    || !next.metadata()?.is_dir()
                {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "new copy parent changed owner or inode",
                    ));
                }
                next.set_permissions(fs::Permissions::from_mode(PRIVATE_DIR_MODE))?;
            }
            // EEXIST is an existing node: never apply permissions to it.
            parent = next;
        }
        Ok(())
    }
    #[cfg(not(unix))]
    fs::create_dir_all(path)
}

pub(crate) fn create_private_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);

    #[cfg(unix)]
    options.mode(PRIVATE_FILE_MODE);

    let file = options.open(path)?;

    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(PRIVATE_FILE_MODE))?;

    Ok(file)
}

pub(crate) fn set_private_dir_permissions(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        if let Err(err) = fs::set_permissions(path, fs::Permissions::from_mode(PRIVATE_DIR_MODE)) {
            if err.kind() == io::ErrorKind::PermissionDenied && path.is_dir() {
                return Ok(());
            }
            return Err(err);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        create_copy_parent_all, create_private_dir_all, create_private_file, PRIVATE_DIR_MODE,
        PRIVATE_FILE_MODE,
    };
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    #[cfg(unix)]
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    #[cfg(unix)]
    fn managed_filesystem_paths_are_group_writable() {
        let root = temp_root("secure-fs");
        let dir = root.join("managed");
        let file = dir.join("payload");

        create_private_dir_all(&dir).expect("dir created");
        create_private_file(&file).expect("file created");

        assert_eq!(
            fs::metadata(&dir)
                .expect("dir metadata")
                .permissions()
                .mode()
                & 0o777,
            PRIVATE_DIR_MODE
        );
        assert_eq!(
            fs::metadata(&file)
                .expect("file metadata")
                .permissions()
                .mode()
                & 0o777,
            PRIVATE_FILE_MODE
        );
        assert_ne!(PRIVATE_DIR_MODE & 0o020, 0);
        assert_ne!(PRIVATE_FILE_MODE & 0o020, 0);

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    #[cfg(unix)]
    fn copy_parent_creation_preserves_existing_modes_and_restricts_new_nodes() {
        let root = temp_root("copy-parent-modes");
        let existing = root.join("existing");
        fs::create_dir_all(&existing).expect("owned existing parent");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).expect("private root");
        fs::set_permissions(&existing, fs::Permissions::from_mode(0o750)).expect("existing mode");
        let created = existing.join("new-prefix/new-object");
        create_copy_parent_all(&created).expect("new parents");
        create_private_file(&created.join("payload")).expect("new payload");
        for (path, mode) in [
            (&root, 0o700),
            (&existing, 0o750),
            (&existing.join("new-prefix"), PRIVATE_DIR_MODE),
            (&created, PRIVATE_DIR_MODE),
        ] {
            assert_eq!(
                fs::metadata(path)
                    .expect("parent mode")
                    .permissions()
                    .mode()
                    & 0o7777,
                mode
            );
        }
        assert_eq!(
            fs::metadata(created.join("payload"))
                .expect("payload mode")
                .permissions()
                .mode()
                & 0o7777,
            PRIVATE_FILE_MODE
        );
        fs::set_permissions(&created, fs::Permissions::from_mode(0o700))
            .expect("existing protected node");
        create_copy_parent_all(&created).expect("existing parents");
        assert_eq!(
            fs::metadata(&created)
                .expect("existing mode")
                .permissions()
                .mode()
                & 0o7777,
            0o700
        );
        fs::remove_dir_all(root).expect("owned cleanup");
    }

    #[test]
    #[cfg(unix)]
    fn copy_parent_alias_resolution_preserves_the_existing_target_mode() {
        use std::os::unix::fs::symlink;
        let root = temp_root("copy-parent-alias");
        let target = root.join("target");
        fs::create_dir_all(&target).expect("owned target");
        fs::set_permissions(&target, fs::Permissions::from_mode(0o700)).expect("protected target");
        symlink(&target, root.join("alias")).expect("existing directory alias");
        create_copy_parent_all(&root.join("alias/new")).expect("prior alias semantics");
        assert_eq!(
            fs::metadata(&target)
                .expect("target mode")
                .permissions()
                .mode()
                & 0o7777,
            0o700
        );
        assert_eq!(
            fs::metadata(target.join("new"))
                .expect("new mode")
                .permissions()
                .mode()
                & 0o7777,
            PRIVATE_DIR_MODE
        );
        assert!(fs::symlink_metadata(root.join("alias"))
            .expect("alias remains")
            .is_symlink());
        fs::remove_dir_all(root).expect("owned cleanup");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn copy_parent_creation_accepts_existing_search_only_directory() {
        let root = temp_root("copy-parent-search-only");
        fs::create_dir_all(&root).expect("owned parent");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o300)).expect("write/search only");
        create_copy_parent_all(&root.join("new")).expect("search-only traversal");
        assert_eq!(
            fs::metadata(&root)
                .expect("existing mode")
                .permissions()
                .mode()
                & 0o7777,
            0o300
        );
        assert_eq!(
            fs::metadata(root.join("new"))
                .expect("new mode")
                .permissions()
                .mode()
                & 0o7777,
            PRIVATE_DIR_MODE
        );
        // Only this test-owned directory needs read permission for recursive cleanup.
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).expect("owned cleanup mode");
        fs::remove_dir_all(root).expect("owned cleanup");
    }

    #[test]
    #[cfg(unix)]
    fn copy_parent_empty_relative_path_does_not_change_the_current_directory() {
        let before = fs::metadata(".")
            .expect("current before")
            .permissions()
            .mode();
        create_copy_parent_all(Path::new("")).expect("empty relative parent");
        assert_eq!(
            fs::metadata(".")
                .expect("current after")
                .permissions()
                .mode(),
            before
        );
    }

    fn temp_root(name: &str) -> std::path::PathBuf {
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
