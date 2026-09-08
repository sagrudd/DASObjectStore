//! Bounded Linux observations of the actual system manager and credential mount.
//! This is not companion admission, ciphertext authentication or native qualification.
//! Credential arrays use busctl: systemd v255 and v259 expose a(ss), which
//! systemctl-show does not render. See upstream src/core/dbus-execute.c and
//! src/systemctl/systemctl-show.c at those tags. No secret values are logged.
use dasobjectstore_object_service::custody::CustodyReadDeadline;
use dasobjectstore_object_service::custody_reader::{ReaderBindingV1, ReaderError};
use std::{fs::File, path::Path};

pub(super) fn verify(
    binding: &ReaderBindingV1,
    encrypted_source: &Path,
    credential_directory: &File,
    credential_path: &Path,
    deadline: CustodyReadDeadline,
) -> Result<(), ReaderError> {
    #[cfg(target_os = "linux")]
    {
        linux::verify(
            binding,
            encrypted_source,
            credential_directory,
            credential_path,
            deadline,
        )
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (
            binding,
            encrypted_source,
            credential_directory,
            credential_path,
            deadline,
        );
        Err(ReaderError::Boundary)
    }
}

#[cfg(any(target_os = "linux", test))]
const PROPERTIES: &str =
    "Id,LoadState,ActiveState,MainPID,User,InvocationID,NoNewPrivileges,ControlGroup,PrivateMounts,NeedDaemonReload,ExecMainStartTimestampMonotonic";
#[cfg(any(target_os = "linux", test))]
const MAX_OUTPUT: usize = 65536;

#[cfg(any(target_os = "linux", test))]
fn unit_object(unit: &str) -> Result<String, ReaderError> {
    // This platform adapter supports literal service names, not a unit pattern,
    // path, option, escaped alias or an inferred production identity.
    if !unit.ends_with(".service")
        || unit.len() > 255
        || unit.len() <= 8
        || !unit.as_bytes()[0].is_ascii_alphanumeric()
        || !unit
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_.@-".contains(&c))
    {
        return Err(ReaderError::Binding);
    }
    let mut path = String::from("/org/freedesktop/systemd1/unit/");
    for (index, c) in unit.bytes().enumerate() {
        if c.is_ascii_alphabetic() || (index > 0 && c.is_ascii_digit()) {
            path.push(char::from(c));
        } else {
            use std::fmt::Write as _;
            write!(&mut path, "_{c:02x}").map_err(|_| ReaderError::Boundary)?;
        }
    }
    Ok(path)
}

#[cfg(any(target_os = "linux", test))]
fn properties(raw: &[u8]) -> Result<std::collections::BTreeMap<&str, &str>, ReaderError> {
    if raw.len() > MAX_OUTPUT {
        return Err(ReaderError::Boundary);
    }
    let text = std::str::from_utf8(raw).map_err(|_| ReaderError::Boundary)?;
    let mut result = std::collections::BTreeMap::new();
    for line in text.lines() {
        let (key, value) = line.split_once('=').ok_or(ReaderError::Boundary)?;
        if !PROPERTIES.split(',').any(|expected| expected == key)
            || value.bytes().any(|c| c.is_ascii_control())
            || result.insert(key, value).is_some()
        {
            return Err(ReaderError::Boundary);
        }
    }
    if result.len() != PROPERTIES.split(',').count() {
        return Err(ReaderError::Boundary);
    }
    Ok(result)
}

#[cfg(any(target_os = "linux", test))]
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BusValue {
    #[serde(rename = "type")]
    signature: String,
    data: serde_json::Value,
}
#[cfg(any(target_os = "linux", test))]
fn credential_property(
    raw: &[u8],
    signature: &str,
    expected: serde_json::Value,
) -> Result<(), ReaderError> {
    if raw.len() > MAX_OUTPUT {
        return Err(ReaderError::Boundary);
    }
    // v259 busctl.c get_property -> bus_message_dump(SUBTREE_ONLY) enters
    // the variant; bus-dump-json.c json_transform_variant transforms ONE value.
    // Thus there is no additional full-message body array around this value.
    // The only accepted nonempty shape is exactly one string pair. Deserialization
    // rejects duplicate outer fields; inner values are arrays/scalars, never maps.
    let value: BusValue = serde_json::from_slice(raw).map_err(|_| ReaderError::Boundary)?;
    if value.signature != signature || value.data != expected {
        return Err(ReaderError::Binding);
    }
    Ok(())
}

#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, PartialEq, Eq)]
struct ManagerTimes {
    load: u64,
    finish: u64,
}

#[cfg(any(target_os = "linux", test))]
fn unsigned_property(raw: &[u8]) -> Result<u64, ReaderError> {
    if raw.len() > MAX_OUTPUT {
        return Err(ReaderError::Boundary);
    }
    let value: BusValue = serde_json::from_slice(raw).map_err(|_| ReaderError::Boundary)?;
    if value.signature != "t" {
        return Err(ReaderError::Boundary);
    }
    value.data.as_u64().ok_or(ReaderError::Boundary)
}

#[cfg(any(target_os = "linux", test))]
fn start_after_load(
    start: u64,
    before: ManagerTimes,
    after: ManagerTimes,
) -> Result<(), ReaderError> {
    // Exact upstream systemd 9ca433482f2281d71718718705ca8cd3bf562ad6:
    // manager.c manager_reloading_start refreshes UNITS_LOAD before reload's
    // serialization; manager-serialize.c preserves it. service.c serializes
    // main-exec-status-start unchanged across reload. UNITS_LOAD_FINISH covers
    // initial boot enumeration. All are same-boot monotonic microseconds.
    // Conservatively require a reader restart after ANY manager daemon-reload,
    // even an unrelated unit change; no automatic restart is performed here.
    if before != after || before.finish == 0 || start <= before.load.max(before.finish) {
        return Err(ReaderError::Binding);
    }
    Ok(())
}

#[cfg(any(target_os = "linux", test))]
fn start_identity(raw: &[u8], pid: u32) -> Result<u64, ReaderError> {
    let text = std::str::from_utf8(raw).map_err(|_| ReaderError::Boundary)?;
    if !text.starts_with(&format!("{pid} (")) {
        return Err(ReaderError::Boundary);
    }
    // comm can contain spaces and ')'; the final closing delimiter precedes state.
    let (_, suffix) = text.rsplit_once(") ").ok_or(ReaderError::Boundary)?;
    let value = suffix
        .split_whitespace()
        .nth(19)
        .ok_or(ReaderError::Boundary)?;
    let value = value.parse::<u64>().map_err(|_| ReaderError::Boundary)?;
    if value == 0 {
        return Err(ReaderError::Boundary);
    }
    Ok(value)
}

#[cfg(any(target_os = "linux", test))]
fn mount_id(raw: &[u8]) -> Result<u64, ReaderError> {
    let text = std::str::from_utf8(raw).map_err(|_| ReaderError::Boundary)?;
    let mut found = None;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("mnt_id:") {
            if found.is_some() {
                return Err(ReaderError::Boundary);
            }
            found = Some(
                value
                    .trim()
                    .parse::<u64>()
                    .map_err(|_| ReaderError::Boundary)?,
            );
        }
    }
    found.filter(|v| *v > 0).ok_or(ReaderError::Boundary)
}

#[cfg(any(target_os = "linux", test))]
fn check_mount(raw: &[u8], id: u64, path: &Path) -> Result<(), ReaderError> {
    let path = path.to_str().ok_or(ReaderError::Boundary)?;
    // Escaped/whitespace paths are deliberately unsupported by this narrow
    // adapter, not treated as equivalent to their unescaped spelling.
    if !path.starts_with('/') || path.bytes().any(|v| v.is_ascii_whitespace() || v == b'\\') {
        return Err(ReaderError::Boundary);
    }
    let text = std::str::from_utf8(raw).map_err(|_| ReaderError::Boundary)?;
    let mut matched = false;
    for line in text.lines() {
        let (left, right) = line.split_once(" - ").ok_or(ReaderError::Boundary)?;
        let fields: Vec<_> = left.split_whitespace().collect();
        let tail: Vec<_> = right.split_whitespace().collect();
        if fields.len() < 6 || tail.len() != 3 {
            return Err(ReaderError::Boundary);
        }
        let current = fields[0]
            .parse::<u64>()
            .map_err(|_| ReaderError::Boundary)?;
        if current == id {
            if matched
                || fields[4] != path
                || !matches!(tail[0], "tmpfs" | "ramfs")
                || !["ro", "nosuid", "nodev", "noexec"]
                    .iter()
                    .all(|wanted| fields[5].split(',').any(|v| v == *wanted))
                || fields[5].split(',').any(|v| v == "rw")
            {
                return Err(ReaderError::Boundary);
            }
            matched = true;
        }
        // A nested writable mount cannot be used as the credential file source.
        if fields[4].starts_with(&format!("{path}/")) {
            return Err(ReaderError::Boundary);
        }
    }
    if !matched {
        return Err(ReaderError::Boundary);
    }
    Ok(())
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use sha2::{Digest as _, Sha256};
    use std::{
        fs::{self, OpenOptions},
        io::Read as _,
        os::{
            fd::AsRawFd as _,
            unix::{
                fs::{MetadataExt as _, OpenOptionsExt as _},
                process::CommandExt as _,
            },
        },
        process::{Child, Command, Stdio},
        time::Duration,
    };

    fn remaining(deadline: CustodyReadDeadline) -> Result<Duration, ReaderError> {
        deadline.remaining().map_err(|_| ReaderError::Boundary)
    }
    fn read(
        path: &Path,
        maximum: usize,
        deadline: CustodyReadDeadline,
    ) -> Result<Vec<u8>, ReaderError> {
        remaining(deadline)?;
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(path)
            .map_err(|_| ReaderError::Boundary)?;
        let mut result = Vec::new();
        (&mut file)
            .take(maximum as u64 + 1)
            .read_to_end(&mut result)
            .map_err(|_| ReaderError::Boundary)?;
        remaining(deadline)?;
        if result.len() > maximum {
            return Err(ReaderError::Boundary);
        }
        Ok(result)
    }
    fn protected_tool(path: &Path) -> Result<(), ReaderError> {
        if !path.is_absolute() || path.canonicalize().map_err(|_| ReaderError::Boundary)? != path {
            return Err(ReaderError::Boundary);
        }
        for (index, item) in path.ancestors().enumerate() {
            let m = fs::symlink_metadata(item).map_err(|_| ReaderError::Boundary)?;
            if m.uid() != 0
                || m.mode() & 0o022 != 0
                || m.file_type().is_symlink()
                || (index == 0 && !m.is_file())
                || (index != 0 && !m.is_dir())
            {
                return Err(ReaderError::Boundary);
            }
        }
        Ok(())
    }
    struct Process(Child);
    impl Drop for Process {
        fn drop(&mut self) {
            // Child is not reaped until this point, so its process-group identity
            // cannot be recycled while cleanup signals the owned group.
            if let Ok(pid) = i32::try_from(self.0.id()) {
                // SAFETY: only the process group created for our child is signaled.
                unsafe {
                    libc::kill(-pid, libc::SIGKILL);
                }
            }
            let _ = self.0.wait();
        }
    }
    fn exited(child: &Child) -> Result<Option<bool>, ReaderError> {
        // SAFETY: waitid initializes its output and WNOWAIT preserves PID ownership.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                child.id(),
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result != 0 {
            return Err(ReaderError::Boundary);
        }
        // SAFETY: successful waitid provides this child-event union (or zero PID).
        if unsafe { info.si_pid() } == 0 {
            return Ok(None);
        }
        Ok(Some(
            info.si_code == libc::CLD_EXITED && unsafe { info.si_status() } == 0,
        ))
    }
    fn capture(
        program: &str,
        args: &[&str],
        deadline: CustodyReadDeadline,
    ) -> Result<Vec<u8>, ReaderError> {
        remaining(deadline)?;
        protected_tool(Path::new(program))?;
        let mut child = Process(
            Command::new(program)
                .args(args)
                .env_clear()
                .env("LC_ALL", "C")
                .env("SYSTEMD_COLORS", "0")
                .env("SYSTEMD_PAGERSECURE", "1")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .process_group(0)
                .spawn()
                .map_err(|_| ReaderError::Boundary)?,
        );
        let mut stdout = child.0.stdout.take().ok_or(ReaderError::Boundary)?;
        // SAFETY: the newly created pipe is exclusively owned here.
        let flags = unsafe { libc::fcntl(stdout.as_raw_fd(), libc::F_GETFL) };
        if flags < 0
            || unsafe { libc::fcntl(stdout.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) }
                < 0
        {
            return Err(ReaderError::Boundary);
        }
        let mut output = Vec::new();
        let mut eof = false;
        loop {
            let available = remaining(deadline)?;
            if !eof {
                let mut buffer = [0; 4096];
                match stdout.read(&mut buffer) {
                    Ok(0) => eof = true,
                    Ok(n) => {
                        if output.len() + n > MAX_OUTPUT {
                            return Err(ReaderError::Boundary);
                        }
                        output.extend_from_slice(&buffer[..n]);
                    }
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                        ) => {}
                    Err(_) => return Err(ReaderError::Boundary),
                }
            }
            if let Some(success) = exited(&child.0)? {
                if !success {
                    return Err(ReaderError::Boundary);
                }
                if eof {
                    return Ok(output);
                }
            }
            std::thread::sleep(available.min(Duration::from_millis(2)));
        }
    }
    fn user_uid(user: &str, deadline: CustodyReadDeadline) -> Result<u32, ReaderError> {
        if user.bytes().all(|c| c.is_ascii_digit()) && !user.is_empty() {
            let uid = user.parse::<u32>().map_err(|_| ReaderError::Boundary)?;
            if uid.to_string() != user {
                return Err(ReaderError::Boundary);
            }
            return Ok(uid);
        }
        // No NSS/network lookup with an unbounded timeout. This concrete adapter
        // requires a selected local passwd account (or exact numeric User=).
        protected_tool(Path::new("/etc/passwd"))?;
        let raw = read(Path::new("/etc/passwd"), 1048576, deadline)?;
        let mut found = None;
        for line in std::str::from_utf8(&raw)
            .map_err(|_| ReaderError::Boundary)?
            .lines()
        {
            let fields: Vec<_> = line.split(':').collect();
            if fields.first() == Some(&user) {
                if fields.len() != 7 || found.is_some() {
                    return Err(ReaderError::Boundary);
                }
                found = Some(
                    fields[2]
                        .parse::<u32>()
                        .map_err(|_| ReaderError::Boundary)?,
                );
            }
        }
        found.ok_or(ReaderError::Boundary)
    }
    fn scalar(unit: &str, uid: u32, deadline: CustodyReadDeadline) -> Result<Vec<u8>, ReaderError> {
        let output = capture(
            "/usr/bin/systemctl",
            &[
                "--system",
                "--no-pager",
                "--no-ask-password",
                "show",
                "--all",
                "--property",
                PROPERTIES,
                "--",
                unit,
            ],
            deadline,
        )?;
        let values = properties(&output)?;
        if values["Id"] != unit
            || values["LoadState"] != "loaded"
            || !matches!(values["ActiveState"], "active" | "activating")
            || values["MainPID"] != std::process::id().to_string()
            || user_uid(values["User"], deadline)? != uid
            || values["NoNewPrivileges"] != "yes"
            || values["PrivateMounts"] != "yes"
            || values["NeedDaemonReload"] != "no"
            || values["InvocationID"].len() != 32
            || !values["InvocationID"]
                .bytes()
                .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
            || values["InvocationID"].bytes().all(|v| v == b'0')
        {
            return Err(ReaderError::Binding);
        }
        let actual = read(Path::new("/proc/self/cgroup"), 65536, deadline)?;
        let expected = format!("0::{}\n", values["ControlGroup"]);
        if !values["ControlGroup"].starts_with('/') || actual != expected.as_bytes() {
            // This concrete qualified adapter uses unified cgroup v2, with the
            // reader itself in the selected unit, not an arbitrary descendant.
            return Err(ReaderError::Binding);
        }
        Ok(output)
    }
    fn credentials(
        object: &str,
        name: &str,
        source: &Path,
        deadline: CustodyReadDeadline,
    ) -> Result<(), ReaderError> {
        let source = source
            .to_str()
            .filter(|v| v.starts_with('/'))
            .ok_or(ReaderError::Binding)?;
        for (property, signature, expected) in [
            (
                "LoadCredentialEncrypted",
                "a(ss)",
                serde_json::json!([[name, source]]),
            ),
            ("LoadCredential", "a(ss)", serde_json::json!([])),
            ("SetCredential", "a(say)", serde_json::json!([])),
            ("SetCredentialEncrypted", "a(say)", serde_json::json!([])),
            ("ImportCredential", "as", serde_json::json!([])),
            ("ImportCredentialEx", "a(ss)", serde_json::json!([])),
        ] {
            let output = capture(
                "/usr/bin/busctl",
                &[
                    "--system",
                    "--no-pager",
                    "--json=short",
                    "--auto-start=no",
                    "--allow-interactive-authorization=no",
                    "get-property",
                    "org.freedesktop.systemd1",
                    object,
                    "org.freedesktop.systemd1.Service",
                    property,
                ],
                deadline,
            )?;
            credential_property(&output, signature, expected)?;
        }
        Ok(())
    }
    fn manager_times(deadline: CustodyReadDeadline) -> Result<ManagerTimes, ReaderError> {
        let mut values = [0; 2];
        for (index, name) in [
            "UnitsLoadTimestampMonotonic",
            "UnitsLoadFinishTimestampMonotonic",
        ]
        .into_iter()
        .enumerate()
        {
            let output = capture(
                "/usr/bin/busctl",
                &[
                    "--system",
                    "--no-pager",
                    "--json=short",
                    "--auto-start=no",
                    "--allow-interactive-authorization=no",
                    "get-property",
                    "org.freedesktop.systemd1",
                    "/org/freedesktop/systemd1",
                    "org.freedesktop.systemd1.Manager",
                    name,
                ],
                deadline,
            )?;
            values[index] = unsigned_property(&output)?;
        }
        Ok(ManagerTimes {
            load: values[0],
            finish: values[1],
        })
    }
    fn directory(
        file: &File,
        path: &Path,
        uid: u32,
        deadline: CustodyReadDeadline,
    ) -> Result<(), ReaderError> {
        let m = file.metadata().map_err(|_| ReaderError::Boundary)?;
        let p = fs::symlink_metadata(path).map_err(|_| ReaderError::Boundary)?;
        if !m.is_dir()
            || !p.is_dir()
            || p.file_type().is_symlink()
            || (m.dev(), m.ino()) != (p.dev(), p.ino())
            || (m.uid() != 0 && m.uid() != uid)
            || m.mode() & 0o077 != 0
        {
            return Err(ReaderError::Boundary);
        }
        let fdinfo = read(
            Path::new(&format!("/proc/self/fdinfo/{}", file.as_raw_fd())),
            4096,
            deadline,
        )?;
        let raw = read(Path::new("/proc/self/mountinfo"), 1048576, deadline)?;
        check_mount(&raw, mount_id(&fdinfo)?, path)?;
        // SAFETY: fstatvfs initializes the POD for a valid retained directory fd.
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::fstatvfs(file.as_raw_fd(), &mut stat) } != 0
            || stat.f_flag & libc::ST_RDONLY == 0
        {
            return Err(ReaderError::Boundary);
        }
        Ok(())
    }
    pub(super) fn verify(
        binding: &ReaderBindingV1,
        source: &Path,
        dir: &File,
        path: &Path,
        deadline: CustodyReadDeadline,
    ) -> Result<(), ReaderError> {
        binding.encode()?;
        let object = unit_object(&binding.service_identity)?;
        // SAFETY: get*id calls take no pointers and mutate no state.
        if unsafe { libc::geteuid() } != binding.uid || unsafe { libc::getuid() } != binding.uid {
            return Err(ReaderError::Binding);
        }
        let pid = std::process::id();
        let status = read(Path::new("/proc/self/status"), 65536, deadline)?;
        let status = std::str::from_utf8(&status).map_err(|_| ReaderError::Boundary)?;
        for (name, wanted) in [
            ("NoNewPrivs:", "1"),
            ("CapEff:", "0000000000000000"),
            ("CapPrm:", "0000000000000000"),
        ] {
            let values = status
                .lines()
                .filter_map(|line| line.strip_prefix(name))
                .map(str::trim)
                .collect::<Vec<_>>();
            if values != [wanted] {
                return Err(ReaderError::Boundary);
            }
        }
        let start = start_identity(&read(Path::new("/proc/self/stat"), 4096, deadline)?, pid)?;
        let ns = fs::metadata("/proc/self/ns/mnt").map_err(|_| ReaderError::Boundary)?;
        // Do not require dereferencing root's /proc/1/ns/mnt from this
        // unprivileged reader: Linux applies PTRACE_MODE_READ_FSCREDS there.
        // Construction is instead checked by live unit PrivateMounts=yes,
        // MainPID/cgroup plus the actual dedicated readonly credential fd mount.
        // Real-systemd positive qualification is still mandatory. Manager-load
        // timestamps below additionally reject old activations after daemon-reload;
        // NeedDaemonReload=no by itself would not establish that property.
        directory(dir, path, binding.uid, deadline)?;
        let manager_before = manager_times(deadline)?;
        let before = scalar(&binding.service_identity, binding.uid, deadline)?;
        let started = properties(&before)?["ExecMainStartTimestampMonotonic"]
            .parse::<u64>()
            .map_err(|_| ReaderError::Boundary)?;
        start_after_load(started, manager_before, manager_before)?;
        credentials(&object, &binding.credential_name, source, deadline)?;
        let mut executable = File::open("/proc/self/exe").map_err(|_| ReaderError::Boundary)?;
        let metadata = executable.metadata().map_err(|_| ReaderError::Boundary)?;
        if !metadata.is_file()
            || metadata.uid() != 0
            || metadata.mode() & 0o022 != 0
            || metadata.len() > 268435456
        {
            return Err(ReaderError::Boundary);
        }
        let mut hash = Sha256::new();
        let mut count = 0_u64;
        loop {
            remaining(deadline)?;
            let mut buffer = [0; 65536];
            let n = executable
                .read(&mut buffer)
                .map_err(|_| ReaderError::Boundary)?;
            if n == 0 {
                break;
            }
            count += n as u64;
            if count > 268435456 {
                return Err(ReaderError::Boundary);
            }
            hash.update(&buffer[..n]);
        }
        if format!("{:x}", hash.finalize()) != binding.executable_sha256 || count != metadata.len()
        {
            return Err(ReaderError::Binding);
        }
        credentials(&object, &binding.credential_name, source, deadline)?;
        if scalar(&binding.service_identity, binding.uid, deadline)? != before
            || start_identity(&read(Path::new("/proc/self/stat"), 4096, deadline)?, pid)? != start
        {
            return Err(ReaderError::Binding);
        }
        let after = fs::metadata("/proc/self/ns/mnt").map_err(|_| ReaderError::Boundary)?;
        if (ns.dev(), ns.ino()) != (after.dev(), after.ino()) {
            return Err(ReaderError::Boundary);
        }
        directory(dir, path, binding.uid, deadline)?;
        start_after_load(started, manager_before, manager_times(deadline)?)?;
        remaining(deadline)?;
        Ok(())
    }

    #[cfg(test)]
    mod capture_tests {
        use super::*;
        use dasobjectstore_object_service::custody::CustodyReadLimits;
        fn deadline() -> CustodyReadDeadline {
            CustodyReadLimits {
                maximum_bytes: 1,
                timeout: Duration::from_millis(50),
            }
            .start()
            .unwrap()
        }
        #[test]
        fn real_child_timeout_and_output_overflow_are_terminal() {
            // Actual unprivileged utility children, not a mocked systemd positive.
            let started = std::time::Instant::now();
            assert!(capture("/usr/bin/sleep", &["10"], deadline()).is_err());
            assert!(started.elapsed() < Duration::from_secs(2));
            assert!(capture("/usr/bin/yes", &[], deadline()).is_err());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires reviewed disposable full-systemd VM and synthetic fixture"]
    fn actual_systemd_vm_adapter_boundary() {
        use dasobjectstore_object_service::custody::CustodyReadLimits;
        use std::os::unix::fs::MetadataExt as _;
        use std::time::{Duration, Instant};
        let root = Path::new("/run/das-systemd-vm-fixture");
        let permit = std::fs::symlink_metadata(root.join("permit")).unwrap();
        assert!(permit.is_file() && permit.uid() == 0 && permit.mode() & 0o022 == 0);
        assert!(std::fs::read("/proc/1/comm").unwrap() == b"systemd\n");
        let raw = std::fs::read(root.join("binding.json")).unwrap();
        let binding = ReaderBindingV1::decode(&raw).unwrap();
        let mode = std::fs::read_to_string(root.join("mode")).unwrap();
        let path = Path::new("/run/credentials/das-vm-reader.service");
        let directory = File::open(path).unwrap();
        let check = || {
            verify(
                &binding,
                Path::new("/var/lib/das-vm/reader.enc"),
                &directory,
                path,
                CustodyReadLimits {
                    maximum_bytes: 4096,
                    timeout: Duration::from_secs(15),
                }
                .start()
                .unwrap(),
            )
        };
        if mode == "positive" {
            assert!(check().is_ok(), "actual systemd adapter positive denied");
        } else if mode == "reload" {
            assert!(check().is_ok(), "pre-reload adapter positive denied");
            std::fs::write("/run/das-vm-results/ready", b"ready").unwrap();
            let deadline = Instant::now() + Duration::from_secs(30);
            while !root.join("continue").exists() {
                assert!(Instant::now() < deadline, "fixture reload signal timed out");
                std::thread::sleep(Duration::from_millis(20));
            }
            assert!(
                check().is_err(),
                "unchanged activation survived manager reload"
            );
        } else {
            assert!(matches!(
                mode.as_str(),
                "wrong-name" | "wrong-executable" | "plaintext"
            ));
            assert!(check().is_err(), "invalid systemd boundary accepted");
        }
        // Only coarse outcome leaves the service. Never read or display secrets.
        std::fs::write("/run/das-vm-results/passed", b"PASS").unwrap();
    }

    #[test]
    fn unit_path_escaping_is_injective_and_rejects_patterns() {
        assert_eq!(
            unit_object("1reader.service").unwrap(),
            "/org/freedesktop/systemd1/unit/_31reader_2eservice"
        );
        assert_eq!(
            unit_object("reader-a_b@x.service").unwrap(),
            "/org/freedesktop/systemd1/unit/reader_2da_5fb_40x_2eservice"
        );
        for name in [
            "--system",
            "reader*.service",
            "../reader.service",
            "reader\\x2d.service",
            ".service",
            "reader.service\n",
        ] {
            assert!(unit_object(name).is_err());
        }
        assert_ne!(
            unit_object("reader-a.service").unwrap(),
            unit_object("reader_2da.service").unwrap()
        );
    }
    #[test]
    fn typed_bus_credentials_deny_substitution_and_plaintext() {
        let expected = serde_json::json!([["reader", "/protected/reader.enc"]]);
        assert!(credential_property(
            br#"{"type":"a(ss)","data":[["reader","/protected/reader.enc"]]}"#,
            "a(ss)",
            expected.clone()
        )
        .is_ok());
        for raw in [
            br#"{"type":"as","data":[["reader","/protected/reader.enc"]]}"#.as_slice(),
            br#"{"type":"a(ss)","type":"a(ss)","data":[]}"#,
            br#"{"type":"a(ss)","data":[["reader","/other"]]}"#,
            br#"{"type":"a(ss)","data":[],"extra":0}"#,
            br#"{"type":"a(ss)","data":[["reader","/protected/reader.enc"],["writer","/writer"]]}"#,
            br#"{"type":"a(ss)","data":[[["reader","/protected/reader.enc"]]]}"#,
        ] {
            assert!(credential_property(raw, "a(ss)", expected.clone()).is_err());
        }
        assert!(credential_property(
            br#"{"type":"a(say)","data":[["reader",[1]]]}"#,
            "a(say)",
            serde_json::json!([])
        )
        .is_err());
    }
    #[test]
    fn metadata_requires_all_properties_once() {
        let raw=b"Id=a.service\nLoadState=loaded\nActiveState=active\nMainPID=1\nUser=1000\nInvocationID=abc\nNoNewPrivileges=yes\nControlGroup=/system.slice/a.service\nPrivateMounts=yes\nNeedDaemonReload=no\nExecMainStartTimestampMonotonic=1000\n";
        assert!(properties(raw).is_ok());
        let mut duplicate = raw.to_vec();
        duplicate.extend_from_slice(b"MainPID=2\n");
        assert!(properties(&duplicate).is_err());
        assert!(properties(b"Id=a.service\n").is_err());
        assert!(properties(b"Id=a.service\nOther=1\n").is_err());
    }
    #[test]
    fn credential_mount_is_bound_to_retained_descriptor_and_exact_ro_mount() {
        let raw =
            b"42 20 0:9 / /run/credentials/a.service ro,nosuid,nodev,noexec - tmpfs tmpfs rw\n";
        let path = Path::new("/run/credentials/a.service");
        assert!(check_mount(raw, 42, path).is_ok());
        for changed in [
            String::from_utf8(raw.to_vec())
                .unwrap()
                .replace("ro,nosuid", "rw,nosuid"),
            String::from_utf8(raw.to_vec())
                .unwrap()
                .replace(" - tmpfs", " - ext4"),
            String::from_utf8(raw.to_vec())
                .unwrap()
                .replace(",noexec", ""),
        ] {
            assert!(check_mount(changed.as_bytes(), 42, path).is_err());
        }
        assert!(check_mount(raw, 43, path).is_err());
        assert!(check_mount(raw, 42, Path::new("/run/credentials/other.service")).is_err());
        assert!(mount_id(b"mnt_id:\t42\nmnt_id:\t43\n").is_err());
    }
    #[test]
    fn proc_start_uses_final_comm_delimiter() {
        let fields = (0..20)
            .map(|v| {
                if v == 19 {
                    "123".to_owned()
                } else {
                    "1".to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(
            start_identity(format!("42 (name ) with space) {fields}").as_bytes(), 42).unwrap(),
            123
        );
        assert!(start_identity(b"43 (name) R", 42).is_err());
    }

    #[test]
    fn manager_reload_requires_new_activation_and_stable_snapshot() {
        let boot = ManagerTimes {
            load: 0,
            finish: 100,
        };
        assert!(start_after_load(101, boot, boot).is_ok());
        let loaded = ManagerTimes {
            load: 200,
            finish: 100,
        };
        assert!(start_after_load(201, loaded, loaded).is_ok());
        for start in [0, 99, 100, 199, 200] {
            assert!(start_after_load(start, loaded, loaded).is_err());
        }
        assert!(start_after_load(201, boot, loaded).is_err());
        assert!(start_after_load(201, loaded, boot).is_err());
        assert!(start_after_load(
            201,
            ManagerTimes { load: 0, finish: 0 },
            ManagerTimes { load: 0, finish: 0 }
        )
        .is_err());
        assert_eq!(
            unsigned_property(br#"{"type":"t","data":123}"#).unwrap(),
            123
        );
        for raw in [
            br#"{"type":"t","data":[123]}"#.as_slice(),
            br#"{"type":"t","data":-1}"#,
            br#"{"type":"s","data":"123"}"#,
            br#"{"type":"t","data":1.5}"#,
            br#"{"type":"t","data":18446744073709551616}"#,
            br#"{"type":"t","data":1,"data":2}"#,
        ] {
            assert!(unsigned_property(raw).is_err());
        }
    }
}
