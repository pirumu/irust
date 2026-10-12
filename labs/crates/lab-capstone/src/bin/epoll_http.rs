//! Project 1: HTTP/1.1 server on raw epoll, edge-triggered, one thread.
//!
//! Run (Linux only): `./labs/linux.sh cargo run -p lab-capstone --bin epoll_http -- 8080`
//!
//! Edge-triggered contract this file follows: every descriptor is non-blocking, and we only
//! go back to `epoll_wait` for a descriptor after `accept4`/`read`/`send` on it returned
//! `EAGAIN` (or after we are done with it).

#[cfg(target_os = "linux")]
mod server {
    use lab_capstone::{LAB_SNDBUF, http, sys};
    use std::collections::HashMap;
    use std::fmt;
    use std::io::{self, ErrorKind};
    use std::os::fd::{AsRawFd, OwnedFd, RawFd};

    const LISTENER: u64 = 0;

    #[derive(Default)]
    struct Stats {
        accepted: u64,
        requests: u64,
        epoll_waits: u64,
        read_eagain: u64,
        write_eagain: u64,
        partial_writes: u64,
        peer_closed_early: u64,
        open: usize,
    }

    impl fmt::Display for Stats {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                f,
                "accepted={} requests={} epoll_waits={} read_eagain={} write_eagain={} partial_writes={} peer_closed_early={} open={}",
                self.accepted,
                self.requests,
                self.epoll_waits,
                self.read_eagain,
                self.write_eagain,
                self.partial_writes,
                self.peer_closed_early,
                self.open
            )
        }
    }

    /// One client. It first collects the request head, then flushes `out`, then is closed.
    struct Conn {
        fd: OwnedFd,
        inbuf: Vec<u8>,
        out: Vec<u8>,
        written: usize,
        responding: bool,
    }

    pub fn main() -> io::Result<()> {
        let mut port = 8080;
        let mut read_once = false;
        for arg in std::env::args().skip(1) {
            match arg.as_str() {
                "--buggy-read-once" => read_once = true,
                p => port = p.parse().map_err(io::Error::other)?,
            }
        }
        let listener = sys::listen_tcp(port)?;
        let epoll = sys::Epoll::new()?;
        epoll.add(listener.as_raw_fd(), sys::EPOLLIN | sys::EPOLLET, LISTENER)?;
        println!("epoll_http: listening on 127.0.0.1:{port}, edge-triggered (EPOLLET)");
        if read_once {
            println!("epoll_http: BUGGY mode: one 16-byte read per event, no drain to EAGAIN");
        }

        let mut conns: HashMap<u64, Conn> = HashMap::new();
        let mut next_token = LISTENER + 1;
        let mut events = Vec::with_capacity(256);
        let mut stats = Stats::default();

        loop {
            epoll.wait(&mut events, -1)?;
            stats.epoll_waits += 1;
            for ev in &events {
                let (token, flags) = sys::event_parts(ev);
                if token == LISTENER {
                    accept_all(listener.as_raw_fd(), &epoll, &mut conns, &mut next_token, &mut stats);
                    continue;
                }
                // A token can be stale: an earlier event in this same batch may have closed it.
                let Some(conn) = conns.get_mut(&token) else { continue };
                if drive(conn, flags, read_once, &mut stats) {
                    let conn = conns.remove(&token).expect("present above");
                    epoll.delete(conn.fd.as_raw_fd())?;
                    stats.open = conns.len();
                    // Dropping the OwnedFd closes the socket.
                }
            }
        }
    }

    /// Edge-triggered listener: one EPOLLIN edge may stand for many queued connections, so
    /// accept until EAGAIN.
    fn accept_all(
        listener: RawFd,
        epoll: &sys::Epoll,
        conns: &mut HashMap<u64, Conn>,
        next_token: &mut u64,
        stats: &mut Stats,
    ) {
        loop {
            match sys::accept(listener) {
                Ok(fd) => {
                    let raw = fd.as_raw_fd();
                    let token = *next_token;
                    *next_token += 1;
                    // Registered once for both directions; with EPOLLET each direction only
                    // reports a new edge, so there is no need to toggle EPOLLOUT on and off.
                    let registered = sys::set_sndbuf(raw, LAB_SNDBUF)
                        .and_then(|()| epoll.add(raw, sys::EPOLLIN | sys::EPOLLOUT | sys::EPOLLET, token));
                    match registered {
                        Ok(()) => {
                            conns.insert(
                                token,
                                Conn { fd, inbuf: Vec::new(), out: Vec::new(), written: 0, responding: false },
                            );
                            stats.accepted += 1;
                            stats.open = conns.len();
                        }
                        Err(e) => eprintln!("epoll_http: register failed: {e}"),
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => return,
                Err(e) if matches!(e.kind(), ErrorKind::Interrupted | ErrorKind::ConnectionAborted) => {}
                Err(e) => {
                    // EMFILE and friends. Returning leaves connections in the backlog that
                    // no new edge will announce; see the "bẫy" section of the doc.
                    eprintln!("epoll_http: accept4: {e}");
                    return;
                }
            }
        }
    }

    /// Advances one connection as far as the socket allows. Returns true when it must be
    /// closed (response fully sent, peer gone, or error).
    ///
    /// `read_once` reproduces the classic edge-triggered bug on purpose (lab step M1.4).
    fn drive(conn: &mut Conn, flags: u32, read_once: bool, stats: &mut Stats) -> bool {
        if flags & sys::EPOLLERR != 0 {
            return true;
        }
        let fd = conn.fd.as_raw_fd();

        if !conn.responding {
            // Drain to EAGAIN before going back to epoll_wait: the edge for these bytes has
            // already been reported and will not be reported again.
            let mut chunk = [0u8; 4096];
            let chunk_len = if read_once { 16 } else { chunk.len() };
            let mut eof = false;
            loop {
                match sys::read(fd, &mut chunk[..chunk_len]) {
                    Ok(0) => {
                        // The peer may have sent a full request and then shut down its write
                        // side (nc, `shutdown(SHUT_WR)`): EOF arrives in the same drain, so
                        // parse what we have before deciding to close.
                        eof = true;
                        break;
                    }
                    Ok(n) => {
                        conn.inbuf.extend_from_slice(&chunk[..n]);
                        if read_once {
                            break; // BUG: the rest of the request stays in the kernel buffer
                        }
                        if conn.inbuf.len() > http::MAX_HEAD {
                            // parse_request will reject it; we are about to close anyway.
                            break;
                        }
                    }
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {
                        stats.read_eagain += 1;
                        break;
                    }
                    Err(e) if e.kind() == ErrorKind::Interrupted => {}
                    Err(_) => return true,
                }
            }
            conn.out = match http::parse_request(&conn.inbuf) {
                Ok(None) if eof => {
                    stats.peer_closed_early += 1;
                    return true;
                }
                Ok(None) => return false, // head incomplete: wait for the next EPOLLIN edge
                Ok(Some((req, _))) => {
                    stats.requests += 1;
                    http::route(&req, || stats.to_string())
                }
                Err(e) => http::error_response(e),
            };
            conn.responding = true;
        }

        while conn.written < conn.out.len() {
            match sys::send(fd, &conn.out[conn.written..]) {
                Ok(n) => {
                    if conn.written + n < conn.out.len() {
                        stats.partial_writes += 1;
                    }
                    conn.written += n;
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    stats.write_eagain += 1;
                    return false; // wait for the next EPOLLOUT edge
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(_) => return true,
            }
        }
        true // Connection: close
    }
}

#[cfg(target_os = "linux")]
fn main() -> std::io::Result<()> {
    server::main()
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("epoll_http dùng epoll, chỉ chạy trên Linux. Chạy trong container:");
    eprintln!("  ./labs/linux.sh cargo run -p lab-capstone --bin epoll_http -- 8080");
    std::process::exit(1);
}
