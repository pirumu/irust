//! Lab (e): UDP ping-pong on loopback, plus the three UDP properties that bite:
//! message boundaries, truncation, and errors reported on a later call.
//!
//! Usage: udp_pingpong [ROUNDS]   (default 5)

use std::io;
use std::net::UdpSocket;
use std::thread;
use std::time::{Duration, Instant};

fn main() -> io::Result<()> {
    let rounds: u32 = std::env::args()
        .nth(1)
        .map(|s| s.parse())
        .transpose()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "ROUNDS must be a number"))?
        .unwrap_or(5);

    // Port 0: the kernel picks a free port. No listen/accept: UDP has no connections.
    let server = UdpSocket::bind("127.0.0.1:0")?;
    let server_addr = server.local_addr()?;
    let pong = thread::spawn(move || -> io::Result<()> {
        let mut buf = [0u8; 1500];
        loop {
            let (n, from) = server.recv_from(&mut buf)?;
            let msg = &buf[..n];
            if msg == b"bye" {
                return Ok(());
            }
            let reply = [b"pong:".as_slice(), msg].concat();
            server.send_to(&reply, from)?;
        }
    });

    let client = UdpSocket::bind("127.0.0.1:0")?;
    // connect() on UDP only records the default peer and filters what recv() accepts.
    client.connect(server_addr)?;
    // UDP gives no delivery guarantee, so a real client must not block forever.
    client.set_read_timeout(Some(Duration::from_secs(1)))?;
    println!("[udp] client {} -> server {server_addr}", client.local_addr()?);

    println!("== 1. ping-pong ==");
    let mut buf = [0u8; 1500];
    for i in 0..rounds {
        let msg = format!("ping {i}");
        let t0 = Instant::now();
        client.send(msg.as_bytes())?;
        let n = client.recv(&mut buf)?;
        println!(
            "  {:?} rtt={:?}",
            String::from_utf8_lossy(&buf[..n]),
            t0.elapsed()
        );
    }

    // Talk to a second, silent socket so the replies of the echo thread do not interfere.
    let sink = UdpSocket::bind("127.0.0.1:0")?;
    let sender = UdpSocket::bind("127.0.0.1:0")?;
    sender.connect(sink.local_addr()?)?;

    println!("== 2. message boundaries: 3 sends -> 3 recvs ==");
    for m in ["a", "bb", "ccc"] {
        sender.send(m.as_bytes())?;
    }
    for _ in 0..3 {
        let n = sink.recv(&mut buf)?;
        println!("  recv -> {n} bytes {:?}", String::from_utf8_lossy(&buf[..n]));
    }

    println!("== 3. truncation: 100-byte datagram into a 10-byte buffer ==");
    sender.send(&[b'x'; 100])?;
    sender.send(b"next")?;
    let mut small = [0u8; 10];
    let n = sink.recv(&mut small)?;
    println!("  first recv -> {n} bytes (the other 90 are gone)");
    let n = sink.recv(&mut buf)?;
    println!("  second recv -> {:?}", String::from_utf8_lossy(&buf[..n]));

    println!("== 4. send to a port nobody listens on ==");
    let closed_port = {
        let tmp = UdpSocket::bind("127.0.0.1:0")?;
        tmp.local_addr()?
    }; // tmp dropped: nothing is bound to that port any more
    let lonely = UdpSocket::bind("127.0.0.1:0")?;
    lonely.connect(closed_port)?;
    lonely.set_read_timeout(Some(Duration::from_millis(500)))?;
    match lonely.send(b"anyone?") {
        Ok(n) => println!("  send() -> Ok({n})"),
        Err(e) => println!("  send() -> Err(kind={:?}, {e})", e.kind()),
    }
    match lonely.recv(&mut buf) {
        Ok(n) => println!("  recv() -> Ok({n})"),
        Err(e) => println!("  recv() -> Err(kind={:?}, {e})", e.kind()),
    }

    client.send(b"bye")?;
    pong.join().expect("server thread panicked")?;
    Ok(())
}
