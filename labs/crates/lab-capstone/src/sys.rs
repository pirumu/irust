//! Thin, Linux-only wrappers over the syscalls the two servers use.
//!
//! Every wrapper turns `-1` into `io::Error::last_os_error()`, so `EAGAIN`/`EWOULDBLOCK`
//! arrives as `io::ErrorKind::WouldBlock` and `EINTR` as `io::ErrorKind::Interrupted`.

use std::io;
use std::mem;
use std::net::Ipv4Addr;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

pub const EPOLLIN: u32 = libc::EPOLLIN as u32;
pub const EPOLLOUT: u32 = libc::EPOLLOUT as u32;
pub const EPOLLRDHUP: u32 = libc::EPOLLRDHUP as u32;
pub const EPOLLERR: u32 = libc::EPOLLERR as u32;
pub const EPOLLHUP: u32 = libc::EPOLLHUP as u32;
pub const EPOLLET: u32 = libc::EPOLLET as u32;

fn cvt(ret: libc::c_int) -> io::Result<libc::c_int> {
    if ret == -1 { Err(io::Error::last_os_error()) } else { Ok(ret) }
}

fn cvt_size(ret: libc::ssize_t) -> io::Result<usize> {
    if ret == -1 { Err(io::Error::last_os_error()) } else { Ok(ret as usize) }
}

/// `socket` + `SO_REUSEADDR` + `bind(127.0.0.1:port)` + `listen`, then `O_NONBLOCK` via `fcntl`.
pub fn listen_tcp(port: u16) -> io::Result<OwnedFd> {
    // SAFETY: plain syscalls on a descriptor we own; pointers refer to live locals whose
    // sizes are passed alongside.
    unsafe {
        let raw = cvt(libc::socket(libc::AF_INET, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0))?;
        let fd = OwnedFd::from_raw_fd(raw);
        let one: libc::c_int = 1;
        cvt(libc::setsockopt(
            raw,
            libc::SOL_SOCKET,
            libc::SO_REUSEADDR,
            (&one as *const libc::c_int).cast(),
            mem::size_of::<libc::c_int>() as libc::socklen_t,
        ))?;
        let mut addr: libc::sockaddr_in = mem::zeroed();
        addr.sin_family = libc::AF_INET as libc::sa_family_t;
        addr.sin_port = port.to_be();
        addr.sin_addr.s_addr = u32::from(Ipv4Addr::LOCALHOST).to_be();
        cvt(libc::bind(
            raw,
            (&addr as *const libc::sockaddr_in).cast(),
            mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
        ))?;
        cvt(libc::listen(raw, 1024))?;
        set_nonblocking(raw)?;
        Ok(fd)
    }
}

/// Read-modify-write of the file status flags: keep whatever is set, add `O_NONBLOCK`.
pub fn set_nonblocking(fd: RawFd) -> io::Result<()> {
    // SAFETY: fcntl with F_GETFL/F_SETFL takes no pointers.
    unsafe {
        let flags = cvt(libc::fcntl(fd, libc::F_GETFL))?;
        cvt(libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK))?;
    }
    Ok(())
}

/// One `accept4` with `SOCK_NONBLOCK | SOCK_CLOEXEC`: the new socket does not inherit
/// `O_NONBLOCK` from the listener, so it has to be requested here.
pub fn accept(listener: RawFd) -> io::Result<OwnedFd> {
    // SAFETY: null addr/addrlen is allowed when the peer address is not wanted.
    let raw = cvt(unsafe {
        libc::accept4(
            listener,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
        )
    })?;
    // SAFETY: accept4 just returned this descriptor and nobody else owns it.
    Ok(unsafe { OwnedFd::from_raw_fd(raw) })
}

pub fn set_sndbuf(fd: RawFd, bytes: usize) -> io::Result<()> {
    let size = bytes as libc::c_int;
    // SAFETY: pointer to a live c_int with its size.
    cvt(unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_SNDBUF,
            (&size as *const libc::c_int).cast(),
            mem::size_of::<libc::c_int>() as libc::socklen_t,
        )
    })?;
    Ok(())
}

pub fn read(fd: RawFd, buf: &mut [u8]) -> io::Result<usize> {
    // SAFETY: buf is valid for buf.len() writable bytes.
    cvt_size(unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) })
}

/// `send` is `write` plus flags; `MSG_NOSIGNAL` turns "peer is gone" into `EPIPE` instead of
/// a `SIGPIPE` to the whole process.
pub fn send(fd: RawFd, buf: &[u8]) -> io::Result<usize> {
    // SAFETY: buf is valid for buf.len() readable bytes.
    cvt_size(unsafe { libc::send(fd, buf.as_ptr().cast(), buf.len(), libc::MSG_NOSIGNAL) })
}

pub struct Epoll {
    fd: OwnedFd,
}

impl Epoll {
    pub fn new() -> io::Result<Self> {
        // SAFETY: no pointers; the returned descriptor is owned by us.
        let raw = cvt(unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) })?;
        Ok(Self { fd: unsafe { OwnedFd::from_raw_fd(raw) } })
    }

    /// Adds `fd` to the interest list; `token` comes back in every event for it.
    pub fn add(&self, fd: RawFd, events: u32, token: u64) -> io::Result<()> {
        let mut ev = libc::epoll_event { events, u64: token };
        // SAFETY: ev lives across the call.
        cvt(unsafe { libc::epoll_ctl(self.fd.as_raw_fd(), libc::EPOLL_CTL_ADD, fd, &mut ev) })?;
        Ok(())
    }

    pub fn delete(&self, fd: RawFd) -> io::Result<()> {
        // SAFETY: EPOLL_CTL_DEL ignores the event argument (non-null required only before 2.6.9).
        cvt(unsafe {
            libc::epoll_ctl(self.fd.as_raw_fd(), libc::EPOLL_CTL_DEL, fd, std::ptr::null_mut())
        })?;
        Ok(())
    }

    /// Blocks until at least one event (or `timeout_ms` elapses; -1 = forever) and fills
    /// `events` up to its capacity. Retries on `EINTR`.
    pub fn wait(&self, events: &mut Vec<libc::epoll_event>, timeout_ms: i32) -> io::Result<()> {
        events.clear();
        let max = events.capacity().min(i32::MAX as usize) as libc::c_int;
        let n = loop {
            // SAFETY: the kernel writes at most `max` entries into spare capacity.
            match cvt(unsafe {
                libc::epoll_wait(self.fd.as_raw_fd(), events.as_mut_ptr(), max, timeout_ms)
            }) {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                r => break r?,
            }
        };
        // SAFETY: epoll_wait initialised exactly n entries.
        unsafe { events.set_len(n as usize) };
        Ok(())
    }
}

/// `(token, event flags)` of one event. `epoll_event` is `repr(packed)` on x86_64, so the
/// fields are copied out by value instead of being borrowed.
pub fn event_parts(ev: &libc::epoll_event) -> (u64, u32) {
    (ev.u64, ev.events)
}
