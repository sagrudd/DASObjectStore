//! Full raw-ledger binding while the caller holds the verified readonly SQLite snapshot.
use super::*;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;

pub(super) fn verify(
    path: &Path,
    file: &mut fs::File,
    expected: &str,
    deadline: CustodyReadDeadline,
) -> Result<(), CustodyReadError> {
    validate_sha256("selected raw ledger", expected).map_err(|_| CustodyReadError::Input)?;
    let before = file.metadata().map_err(|_| CustodyReadError::Boundary)?;
    let named = fs::symlink_metadata(path).map_err(|_| CustodyReadError::Boundary)?;
    if !before.is_file()
        || before.nlink() != 1
        || before.mode() & 0o022 != 0
        || named.file_type().is_symlink()
        || identity(&before) != identity(&named)
    {
        return Err(CustodyReadError::Boundary);
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| CustodyReadError::Boundary)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 65536];
    let mut count = 0_u64;
    loop {
        deadline.remaining()?;
        let n = file
            .read(&mut buffer)
            .map_err(|_| CustodyReadError::Boundary)?;
        if n == 0 {
            break;
        }
        count = count
            .checked_add(n as u64)
            .ok_or(CustodyReadError::Boundary)?;
        if count > before.len() {
            return Err(CustodyReadError::Boundary);
        }
        hash.update(&buffer[..n]);
    }
    deadline.remaining()?;
    let after = file.metadata().map_err(|_| CustodyReadError::Boundary)?;
    let named = fs::symlink_metadata(path).map_err(|_| CustodyReadError::Boundary)?;
    if count != before.len()
        || identity(&before) != identity(&after)
        || named.file_type().is_symlink()
        || identity(&after) != identity(&named)
        || format!("{:x}", hash.finalize()) != expected
    {
        return Err(CustodyReadError::Boundary);
    }
    Ok(())
}
fn identity(m: &fs::Metadata) -> (u64, u64, u64, u32, u32, u64, i64, i64, i64, i64) {
    (
        m.dev(),
        m.ino(),
        m.len(),
        m.uid(),
        m.mode(),
        m.nlink(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
    )
}
