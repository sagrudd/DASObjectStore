//! Read-only descriptor-relative input adapter: no symlinks, devices or FIFO reads.
use dasobjectstore_object_service::bootstrap_plan::MAX_INPUT_BYTES;
use std::path::Path;

#[cfg(unix)]
pub(super) fn read(path: &Path) -> Result<Vec<u8>, &'static str> {
    use std::ffi::CString;
    use std::fs::File;
    use std::io::Read;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::path::Component;
    let denied = "bootstrap_plan_input_denied";
    if !path.is_absolute() {
        return Err(denied);
    }
    let components: Vec<_> = path.components().collect();
    // Compare the byte spelling as well: Path::components normalizes redundant
    // separators and '.' before we can reject them.
    let raw = path.as_os_str().as_bytes();
    if raw.windows(2).any(|v| v == b"//")
        || raw.ends_with(b"/")
        || raw
            .split(|v| *v == b'/')
            .skip(1)
            .any(|v| v == b"." || v == b"..")
        || components.len() < 2
    {
        return Err(denied);
    }
    let root = CString::new("/").map_err(|_| denied)?;
    // SAFETY: NUL-terminated fixed path and valid flags; the returned descriptor
    // is uniquely owned by File only after checking for a negative result.
    let fd = unsafe {
        libc::open(
            root.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(denied);
    }
    // SAFETY: fd is a fresh valid descriptor, transferred exactly once.
    let mut file = unsafe { File::from_raw_fd(fd) };
    for (index, component) in components.iter().enumerate().skip(1) {
        let Component::Normal(name) = component else {
            return Err(denied);
        };
        let name = CString::new(name.as_bytes()).map_err(|_| denied)?;
        let final_component = index == components.len() - 1;
        let mut flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK;
        if !final_component {
            flags |= libc::O_DIRECTORY;
        }
        // SAFETY: file owns a live directory fd; name is NUL-terminated; openat
        // uses only read flags and no-follow on each individual path component.
        let fd = unsafe { libc::openat(file.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(denied);
        }
        // SAFETY: fd was freshly returned by openat and is transferred once.
        file = unsafe { File::from_raw_fd(fd) };
    }
    let metadata = file.metadata().map_err(|_| denied)?;
    if !metadata.is_file() || metadata.len() > MAX_INPUT_BYTES as u64 {
        return Err(denied);
    }
    let mut bytes = Vec::new();
    file.take(MAX_INPUT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| denied)?;
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(denied);
    }
    Ok(bytes)
}

#[cfg(not(unix))]
pub(super) fn read(_: &Path) -> Result<Vec<u8>, &'static str> {
    Err("bootstrap_plan_platform_denied")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn regular_read_preserves_bytes_and_rejects_aliases_devices_and_missing() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("custody-plan-reader-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let input = root.join("input");
        std::fs::write(&input, b"synthetic input").unwrap();
        symlink(&input, root.join("alias")).unwrap();
        symlink(&root, root.join("parent-alias")).unwrap();
        assert_eq!(read(&input).unwrap(), b"synthetic input");
        for denied in [
            root.join("alias"),
            root.join("parent-alias/input"),
            root.clone(),
            root.join("missing"),
            Path::new("/dev/null").to_owned(),
            Path::new("relative").to_owned(),
        ] {
            assert!(read(&denied).is_err());
        }
        assert_eq!(std::fs::read(&input).unwrap(), b"synthetic input");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 3);
        let oversized = root.join("oversized");
        let file = std::fs::File::create(&oversized).unwrap();
        file.set_len(MAX_INPUT_BYTES as u64 + 1).unwrap();
        assert!(read(&oversized).is_err());
        std::fs::remove_file(oversized).unwrap();
        std::fs::remove_file(root.join("alias")).unwrap();
        std::fs::remove_file(root.join("parent-alias")).unwrap();
        std::fs::remove_file(input).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
