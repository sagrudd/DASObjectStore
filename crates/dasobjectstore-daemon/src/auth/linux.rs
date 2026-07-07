use super::DaemonLocalActor;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinuxPeerCredentials {
    pub pid: u32,
    pub uid: u32,
    pub gid: u32,
}

impl LinuxPeerCredentials {
    pub fn to_actor(&self) -> DaemonLocalActor {
        DaemonLocalActor::new(self.uid).with_primary_gid(self.gid)
    }
}

pub fn read_linux_peer_credentials(stream: &UnixStream) -> io::Result<LinuxPeerCredentials> {
    let mut credentials = std::mem::MaybeUninit::<libc::ucred>::uninit();
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            credentials.as_mut_ptr().cast(),
            &mut length,
        )
    };

    if result == -1 {
        return Err(io::Error::last_os_error());
    }

    let credentials = unsafe { credentials.assume_init() };
    Ok(LinuxPeerCredentials {
        pid: credentials.pid as u32,
        uid: credentials.uid,
        gid: credentials.gid,
    })
}
