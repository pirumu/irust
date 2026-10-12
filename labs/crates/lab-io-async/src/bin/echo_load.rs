//! Load client for every echo server in this crate.
//!
//! Usage: echo_load [PORT] [CONNS] [MSGS] [MSG_SIZE]
//!   opens CONNS connections at once (one client thread each), sends MSGS messages
//!   of MSG_SIZE bytes on each, and checks that every byte comes back unchanged.
//!   A server that stops answering is reported after a 3 s read timeout.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

fn main() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let port: u16 = num(args.next(), 7101)?;
    let conns: usize = num(args.next(), 4)?;
    let msgs: usize = num(args.next(), 100)?;
    let size: usize = num(args.next(), 64)?;

    let start = Instant::now();
    let clients: Vec<_> = (0..conns)
        .map(|c| thread::spawn(move || client(port, c, msgs, size)))
        .collect();
    let mut failed = 0;
    for (c, h) in clients.into_iter().enumerate() {
        if let Err(e) = h.join().expect("client thread panicked") {
            eprintln!("[load] conn {c}: {e}");
            failed += 1;
        }
    }
    println!(
        "[load] {conns} conns x {msgs} msgs x {size} B: {} ok, {failed} failed, {} ms",
        conns - failed,
        start.elapsed().as_millis()
    );
    if failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}

fn client(port: u16, c: usize, msgs: usize, size: usize) -> io::Result<()> {
    let mut s = TcpStream::connect(("127.0.0.1", port))?;
    s.set_read_timeout(Some(Duration::from_secs(3)))?;
    let mut back = vec![0u8; size];
    for m in 0..msgs {
        let msg: Vec<u8> = (0..size).map(|i| (c + m + i) as u8).collect();
        s.write_all(&msg)?;
        s.read_exact(&mut back)
            .map_err(|e| io::Error::new(e.kind(), format!("message {m}: {e}")))?;
        if back != msg {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("message {m} corrupted")));
        }
    }
    s.shutdown(Shutdown::Write)?;
    // Wait for the server to close its side, so the server's connection count is final.
    let mut rest = Vec::new();
    s.read_to_end(&mut rest)?;
    Ok(())
}

fn num<T: std::str::FromStr>(arg: Option<String>, default: T) -> io::Result<T> {
    match arg {
        Some(a) => a
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, format!("not a number: {a}"))),
        None => Ok(default),
    }
}
