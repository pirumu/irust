//! Lab (a): blocking TCP echo server, one OS thread per connection.
//!
//! Usage: echo_server [ADDR] [MAX_CONNS]
//!   ADDR       default 127.0.0.1:7878
//!   MAX_CONNS  stop accepting after this many connections, wait for them, exit
//!              (handy under strace); omitted = serve forever.

use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

fn main() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let addr = args.next().unwrap_or_else(|| "127.0.0.1:7878".to_string());
    let max_conns: Option<usize> = match args.next() {
        Some(n) => Some(
            n.parse()
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "MAX_CONNS must be a number"))?,
        ),
        None => None,
    };

    // bind() + listen() happen inside TcpListener::bind.
    let listener = TcpListener::bind(&addr)?;
    eprintln!("[server] listening on {}", listener.local_addr()?);

    let mut workers = Vec::new();
    // incoming() calls accept() in a loop; each accept blocks until a client connects.
    for (i, conn) in listener.incoming().enumerate() {
        let stream = conn?;
        let peer = stream.peer_addr()?;
        eprintln!("[server] accepted {peer}");
        workers.push(thread::spawn(move || {
            match echo(stream) {
                Ok(n) => eprintln!("[server] {peer} closed after {n} bytes"),
                Err(e) => eprintln!("[server] {peer} error: {e}"),
            }
        }));
        if max_conns.is_some_and(|max| i + 1 >= max) {
            break;
        }
    }
    for w in workers {
        w.join().expect("worker panicked");
    }
    Ok(())
}

/// Copies everything the peer sends back to it until the peer half-closes (read returns 0).
fn echo(mut stream: TcpStream) -> io::Result<u64> {
    let mut buf = [0u8; 4096];
    let mut total = 0u64;
    loop {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            return Ok(total);
        }
        stream.write_all(&buf[..n])?;
        total += n as u64;
    }
}
