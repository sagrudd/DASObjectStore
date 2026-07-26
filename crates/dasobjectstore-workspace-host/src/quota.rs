use crate::BrokerError;
use std::path::Path;

#[cfg(target_os = "linux")]
const FS_XFLAG_PROJINHERIT: u32 = 0x0000_0200;

#[cfg(target_os = "linux")]
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Fsxattr {
    fsx_xflags: u32,
    fsx_extsize: u32,
    fsx_nextents: u32,
    fsx_projid: u32,
    fsx_cowextsize: u32,
    fsx_pad: [u8; 8],
}

#[cfg(target_os = "linux")]
const FS_IOC_FSGETXATTR: libc::c_ulong = 0x801c_581f;
#[cfg(target_os = "linux")]
const FS_IOC_FSSETXATTR: libc::c_ulong = 0x401c_5820;

/// Apply and verify the directory project identity and inheritance flag.
///
/// The filesystem quota limit itself is set by the reviewed `setquota`
/// interface because Linux exposes incompatible ext4/XFS quotactl formats.
/// Arguments are fixed and derived solely from validated numeric/config data.
#[cfg(target_os = "linux")]
pub fn apply_project_quota(
    directory: &Path,
    project_id: u32,
    quota_bytes: u64,
) -> Result<(), BrokerError> {
    use std::fs::OpenOptions;
    use std::os::fd::AsRawFd;
    use std::process::Command;

    let file = OpenOptions::new()
        .read(true)
        .open(directory)
        .map_err(|error| BrokerError::Io("open branch for quota", error))?;
    let mut attributes = Fsxattr::default();
    // SAFETY: the ioctl writes exactly one kernel fsxattr into an initialized,
    // correctly sized C representation while the file descriptor remains open.
    if unsafe { libc::ioctl(file.as_raw_fd(), FS_IOC_FSGETXATTR, &mut attributes) } != 0 {
        return Err(BrokerError::Quota(
            std::io::Error::last_os_error().to_string(),
        ));
    }
    attributes.fsx_projid = project_id;
    attributes.fsx_xflags |= FS_XFLAG_PROJINHERIT;
    // SAFETY: the ioctl reads the correctly sized C representation and the
    // descriptor remains valid for the duration of the call.
    if unsafe { libc::ioctl(file.as_raw_fd(), FS_IOC_FSSETXATTR, &attributes) } != 0 {
        return Err(BrokerError::Quota(
            std::io::Error::last_os_error().to_string(),
        ));
    }

    let kibibytes = quota_bytes
        .checked_add(1023)
        .ok_or_else(|| BrokerError::Quota("quota size overflow".to_string()))?
        / 1024;
    let mount = mount_point_for(directory)?;
    let status = Command::new("/usr/sbin/setquota")
        .args([
            "-P",
            &project_id.to_string(),
            "0",
            &kibibytes.to_string(),
            "0",
            "0",
        ])
        .arg(&mount)
        .status()
        .map_err(|error| BrokerError::Io("execute setquota", error))?;
    if !status.success() {
        return Err(BrokerError::Quota(format!(
            "setquota exited with {status}; project quotas may not be enabled"
        )));
    }
    verify_project_quota(directory, project_id)
}

#[cfg(target_os = "linux")]
fn mount_point_for(directory: &Path) -> Result<std::path::PathBuf, BrokerError> {
    use std::os::unix::ffi::OsStringExt;
    let canonical = directory
        .canonicalize()
        .map_err(|error| BrokerError::Io("resolve quota directory", error))?;
    let mountinfo = std::fs::read_to_string("/proc/self/mountinfo")
        .map_err(|error| BrokerError::Io("read mount table", error))?;
    mountinfo
        .lines()
        .filter_map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            let encoded = fields.get(4)?;
            let bytes = decode_mountinfo_path(encoded)?;
            let path = std::path::PathBuf::from(std::ffi::OsString::from_vec(bytes));
            canonical.starts_with(&path).then_some(path)
        })
        .max_by_key(|path| path.as_os_str().len())
        .ok_or_else(|| BrokerError::Quota("no containing mount point found".to_string()))
}

#[cfg(target_os = "linux")]
fn decode_mountinfo_path(value: &str) -> Option<Vec<u8>> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            let octal = bytes.get(index + 1..index + 4)?;
            if !octal.iter().all(u8::is_ascii_digit) {
                return None;
            }
            let text = std::str::from_utf8(octal).ok()?;
            decoded.push(u8::from_str_radix(text, 8).ok()?);
            index += 4;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    Some(decoded)
}

#[cfg(target_os = "linux")]
pub fn verify_project_quota(directory: &Path, project_id: u32) -> Result<(), BrokerError> {
    use std::fs::File;
    use std::os::fd::AsRawFd;
    let file =
        File::open(directory).map_err(|error| BrokerError::Io("open branch for inspect", error))?;
    let mut attributes = Fsxattr::default();
    // SAFETY: see `apply_project_quota`; the buffer and descriptor are valid.
    if unsafe { libc::ioctl(file.as_raw_fd(), FS_IOC_FSGETXATTR, &mut attributes) } != 0 {
        return Err(BrokerError::Quota(
            std::io::Error::last_os_error().to_string(),
        ));
    }
    if attributes.fsx_projid != project_id || attributes.fsx_xflags & FS_XFLAG_PROJINHERIT == 0 {
        return Err(BrokerError::Quota(
            "project identity or inheritance flag does not match".to_string(),
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn apply_project_quota(
    _directory: &Path,
    _project_id: u32,
    _quota_bytes: u64,
) -> Result<(), BrokerError> {
    Err(BrokerError::Unsupported(
        "project quotas require Linux".to_string(),
    ))
}

#[cfg(not(target_os = "linux"))]
pub fn verify_project_quota(_directory: &Path, _project_id: u32) -> Result<(), BrokerError> {
    Err(BrokerError::Unsupported(
        "project quotas require Linux".to_string(),
    ))
}
