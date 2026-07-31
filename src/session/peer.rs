//! Peer credential helpers for Unix sockets.

use std::io;
use std::os::unix::io::AsRawFd;

/// Return the peer process UID for a connected Unix stream.
pub fn peer_uid<S: AsRawFd>(stream: &S) -> io::Result<u32> {
    let fd = stream.as_raw_fd();
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd", target_os = "openbsd", target_os = "netbsd"))]
    {
        let mut uid: libc::uid_t = 0;
        let mut gid: libc::gid_t = 0;
        let rc = unsafe { libc::getpeereid(fd, &mut uid, &mut gid) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(uid as u32)
    }
    #[cfg(target_os = "linux")]
    {
        let mut cred = libc::ucred {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        let rc = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                &mut cred as *mut _ as *mut libc::c_void,
                &mut len,
            )
        };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(cred.uid as u32)
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "linux"
    )))]
    {
        let _ = fd;
        Ok(unsafe { libc::geteuid() } as u32)
    }
}

pub fn current_uid() -> u32 {
    unsafe { libc::geteuid() as u32 }
}

pub fn peer_uid_matches_self<S: AsRawFd>(stream: &S) -> bool {
    match peer_uid(stream) {
        Ok(uid) => uid == current_uid(),
        Err(_) => false,
    }
}
