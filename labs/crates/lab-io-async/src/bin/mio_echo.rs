//! Lab (b): the same edge-triggered echo server, written against mio.
//!
//! Usage: mio_echo [PORT=7102] [CONNS=4]
//! mio picks the OS selector (epoll on Linux, kqueue on macOS), so this lab runs
//! on both: `cargo run -p lab-io-async --bin mio_echo` or under ./labs/linux.sh.

use std::collections::HashMap;
use std::io::{self, ErrorKind, Read, Write};
use std::net::SocketAddr;

use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Poll, Token};
use lab_io_async::{ServerArgs, thread_count_text};

const LISTENER: Token = Token(0);

struct Conn {
    stream: TcpStream,
    out: Vec<u8>,
}

fn main() -> io::Result<()> {
    let args = ServerArgs::parse(7102)?;
    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(64);

    let addr: SocketAddr = ([127, 0, 0, 1], args.port).into();
    // mio's TcpListener::bind already puts the socket in non-blocking mode.
    let mut listener = TcpListener::bind(addr)?;
    poll.registry().register(&mut listener, LISTENER, Interest::READABLE)?;
    eprintln!("[mio] listening on {addr}");

    let mut conns: HashMap<Token, Conn> = HashMap::new();
    let mut next = 1usize;
    let (mut served, mut polls, mut n_events, mut would_block) = (0, 0u64, 0u64, 0u64);

    while served < args.conns {
        match poll.poll(&mut events, None) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
        polls += 1;
        for event in events.iter() {
            n_events += 1;
            match event.token() {
                LISTENER => loop {
                    // Drain the accept queue: mio's events are edge-triggered.
                    match listener.accept() {
                        Ok((mut stream, peer)) => {
                            let token = Token(next);
                            next += 1;
                            poll.registry().register(
                                &mut stream,
                                token,
                                Interest::READABLE | Interest::WRITABLE,
                            )?;
                            eprintln!("[mio] accepted {peer} as {token:?}");
                            conns.insert(token, Conn { stream, out: Vec::new() });
                        }
                        Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                        Err(e) => return Err(e),
                    }
                },
                token => {
                    let Some(conn) = conns.get_mut(&token) else { continue };
                    let closed = match drain_and_echo(conn, &mut would_block) {
                        Ok(closed) => closed,
                        Err(e) => {
                            eprintln!("[mio] {token:?}: {e}");
                            true
                        }
                    };
                    if closed {
                        let mut conn = conns.remove(&token).expect("present");
                        poll.registry().deregister(&mut conn.stream)?;
                        served += 1;
                    }
                }
            }
        }
    }

    println!(
        "[mio] served {served} conns: Poll::poll calls={polls}, events={n_events}, \
         reads -> WouldBlock={would_block}, threads={}",
        thread_count_text()
    );
    Ok(())
}

/// mio documents that readiness must be drained: read until WouldBlock.
fn drain_and_echo(conn: &mut Conn, would_block: &mut u64) -> io::Result<bool> {
    flush(conn)?;
    let mut buf = [0u8; 4096];
    loop {
        match conn.stream.read(&mut buf) {
            Ok(0) => return Ok(true),
            Ok(n) => {
                conn.out.extend_from_slice(&buf[..n]);
                flush(conn)?;
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                *would_block += 1;
                return Ok(false);
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
}

fn flush(conn: &mut Conn) -> io::Result<()> {
    while !conn.out.is_empty() {
        match conn.stream.write(&conn.out) {
            Ok(n) => {
                conn.out.drain(..n);
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => return Ok(()),
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
