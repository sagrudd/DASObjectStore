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
    use super::{create_private_dir_all, create_private_file, PRIVATE_DIR_MODE, PRIVATE_FILE_MODE};
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
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
