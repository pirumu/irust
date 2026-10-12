//! Lab (a): non-blocking echo server on raw epoll(7), edge-triggered, via `libc`.
//!
//! Usage: epoll_echo [PORT=7101] [CONNS=4] [--read-once]
//!   Serves CONNS connections on 127.0.0.1:PORT, then prints counters and exits.
//!   --read-once  deliberately breaks the edge-triggered contract: one 16-byte
//!                read(2) per event instead of reading until EAGAIN.
//!
//! Linux only: `./labs/linux.sh cargo run -p lab-io-async --bin epoll_echo`.

#[cfg(target_os = "linux")]
fn main() -> std::io::Result<()> {
    linux::run()
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("epoll(7) exists only on Linux. Run inside the lab container:");
    eprintln!("  ./labs/linux.sh cargo run -p lab-io-async --bin epoll_echo");
    std::process::exit(2);
}

#[cfg(target_os = "linux")]
mod linux {
    use std::collections::HashMap;
    use std::io::{self, ErrorKind};
    use std::net::TcpListener;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

    use lab_io_async::{ServerArgs, thread_count_text};

    struct Conn {
        fd: OwnedFd,
        /// Bytes read but not yet written back (the socket send buffer was full).
        out: Vec<u8>,
    }

    #[derive(Default)]
    struct Stats {
        epoll_waits: u64,
        events: u64,
        reads_with_data: u64,
        reads_eagain: u64,
        bytes: u64,
    }

    pub fn run() -> io::Result<()> {
        let args = ServerArgs::parse(7101)?;
        let read_once = args.has_flag("--read-once");

        // socket() + bind() + listen() come from std; everything after that is raw.
        let listener = TcpListener::bind(("127.0.0.1", args.port))?;
        let lfd = listener.as_raw_fd();
        set_nonblocking(lfd)?;

        // SAFETY: epoll_create1 returned a fresh fd that nothing else owns.
        let ep = unsafe { OwnedFd::from_raw_fd(cvt(libc::epoll_create1(libc::EPOLL_CLOEXEC))?) };
        let epfd = ep.as_raw_fd();
        // The listener stays level-triggered: an un-accepted connection keeps reporting.
        ctl(epfd, libc::EPOLL_CTL_ADD, lfd, libc::EPOLLIN as u32)?;
        // Connections are edge-triggered. IN and OUT are registered once, so a full
        // send buffer never needs an EPOLL_CTL_MOD.
        let conn_events = if read_once {
            (libc::EPOLLIN | libc::EPOLLET) as u32
        } else {
            (libc::EPOLLIN | libc::EPOLLOUT | libc::EPOLLRDHUP | libc::EPOLLET) as u32
        };
        eprintln!(
            "[epoll] listening on 127.0.0.1:{} (epfd={epfd}, listener fd={lfd}, mode={})",
            args.port,
            if read_once { "ET, read once (broken)" } else { "ET, drain until EAGAIN" }
        );

        let mut conns: HashMap<RawFd, Conn> = HashMap::new();
        let mut st = Stats::default();
        let mut served = 0;
        let mut events = [libc::epoll_event { events: 0, u64: 0 }; 64];

        while served < args.conns {
            // timeout -1: sleep in the kernel until at least one fd is ready.
            let n = unsafe { libc::epoll_wait(epfd, events.as_mut_ptr(), events.len() as i32, -1) };
            if n < 0 {
                let err = io::Error::last_os_error();
                if err.kind() == ErrorKind::Interrupted {
                    continue;
                }
                return Err(err);
            }
            st.epoll_waits += 1;
            st.events += n as u64;

            for ev in &events[..n as usize] {
                // `data.u64` is whatever we stored at EPOLL_CTL_ADD: here, the fd itself.
                let fd = ev.u64 as RawFd;
                if fd == lfd {
                    accept_all(&listener, epfd, conn_events, &mut conns)?;
                    continue;
                }
                let Some(conn) = conns.get_mut(&fd) else { continue };
                let result = if read_once {
                    read_once_and_echo(conn, &mut st)
                } else {
                    drain_and_echo(conn, &mut st)
                };
                let closed = match result {
                    Ok(closed) => closed,
                    Err(e) => {
                        eprintln!("[epoll] fd {fd}: {e}");
                        true
                    }
                };
                if closed {
                    ctl(epfd, libc::EPOLL_CTL_DEL, fd, 0)?;
                    conns.remove(&fd); // dropping OwnedFd closes the socket
                    served += 1;
                }
            }
        }

        println!(
            "[epoll] served {served} conns: epoll_wait calls={}, events={}, reads with data={}, \
             reads -> EAGAIN={}, bytes echoed={}, threads={}",
            st.epoll_waits,
            st.events,
            st.reads_with_data,
            st.reads_eagain,
            st.bytes,
            thread_count_text()
        );
        Ok(())
    }

    /// The listener is non-blocking, so accept until it says EAGAIN: with several
    /// clients connecting at once, one event can stand for several connections.
    fn accept_all(
        listener: &TcpListener,
        epfd: RawFd,
        conn_events: u32,
        conns: &mut HashMap<RawFd, Conn>,
    ) -> io::Result<()> {
        loop {
            match listener.accept() {
                Ok((stream, peer)) => {
                    let fd: OwnedFd = stream.into();
                    let raw = fd.as_raw_fd();
                    set_nonblocking(raw)?;
                    ctl(epfd, libc::EPOLL_CTL_ADD, raw, conn_events)?;
                    eprintln!("[epoll] accepted {peer} as fd {raw}");
                    conns.insert(raw, Conn { fd, out: Vec::new() });
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => return Ok(()),
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
    }

    /// Correct edge-triggered handling: read until EAGAIN, because the kernel will
    /// not report this fd again until new data arrives. Returns true when the peer closed.
    fn drain_and_echo(conn: &mut Conn, st: &mut Stats) -> io::Result<bool> {
        flush(conn)?;
        let mut buf = [0u8; 4096];
        loop {
            match read(conn.fd.as_raw_fd(), &mut buf) {
                Ok(0) => return Ok(true),
                Ok(n) => {
                    st.reads_with_data += 1;
                    st.bytes += n as u64;
                    conn.out.extend_from_slice(&buf[..n]);
                    flush(conn)?;
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    st.reads_eagain += 1;
                    return Ok(false);
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
    }

    /// The bug from epoll(7): consume only part of the input buffer, then go back
    /// to epoll_wait. No new edge arrives for the bytes left behind.
    fn read_once_and_echo(conn: &mut Conn, st: &mut Stats) -> io::Result<bool> {
        let mut buf = [0u8; 16];
        match read(conn.fd.as_raw_fd(), &mut buf) {
            Ok(0) => Ok(true),
            Ok(n) => {
                st.reads_with_data += 1;
                st.bytes += n as u64;
                conn.out.extend_from_slice(&buf[..n]);
                flush(conn)?;
                Ok(false)
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                st.reads_eagain += 1;
                Ok(false)
            }
            Err(e) => Err(e),
        }
    }

    /// Writes as much of `conn.out` as the socket accepts; the rest waits for EPOLLOUT.
    fn flush(conn: &mut Conn) -> io::Result<()> {
        while !conn.out.is_empty() {
            let n = unsafe {
                libc::write(conn.fd.as_raw_fd(), conn.out.as_ptr().cast(), conn.out.len())
            };
            if n < 0 {
                let err = io::Error::last_os_error();
                match err.kind() {
                    ErrorKind::WouldBlock => return Ok(()),
                    ErrorKind::Interrupted => continue,
                    _ => return Err(err),
                }
            }
            conn.out.drain(..n as usize);
        }
        Ok(())
    }

    fn read(fd: RawFd, buf: &mut [u8]) -> io::Result<usize> {
        let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
        if n < 0 { Err(io::Error::last_os_error()) } else { Ok(n as usize) }
    }

    /// O_NONBLOCK through fcntl(2), the way open(2)/fcntl(2) describe it.
    fn set_nonblocking(fd: RawFd) -> io::Result<()> {
        let flags = cvt(unsafe { libc::fcntl(fd, libc::F_GETFL) })?;
        cvt(unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) })?;
        Ok(())
    }

    fn ctl(epfd: RawFd, op: libc::c_int, fd: RawFd, events: u32) -> io::Result<()> {
        let mut ev = libc::epoll_event { events, u64: fd as u64 };
        cvt(unsafe { libc::epoll_ctl(epfd, op, fd, &mut ev) })?;
        Ok(())
    }

    fn cvt(ret: libc::c_int) -> io::Result<libc::c_int> {
        if ret < 0 { Err(io::Error::last_os_error()) } else { Ok(ret) }
    }
}
