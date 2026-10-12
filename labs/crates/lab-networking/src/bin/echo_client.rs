//! Echo client that shows a TCP half-close.
//!
//! Usage: echo_client [ADDR] [MESSAGE]
//! It writes MESSAGE, calls shutdown(Write) so the server sees EOF (our FIN),
//! then reads the echo until the server closes its side too.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};

fn main() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let addr = args.next().unwrap_or_else(|| "127.0.0.1:7878".to_string());
    let msg = args.next().unwrap_or_else(|| "hello, tcp\n".to_string());

    // connect() runs the three-way handshake and returns once it completes.
    let mut stream = TcpStream::connect(&addr)?;
    println!(
        "[client] connected {} -> {}",
        stream.local_addr()?,
        stream.peer_addr()?
    );
    stream.write_all(msg.as_bytes())?;
    // No more data from us: the kernel sends FIN, our read side stays open.
    stream.shutdown(Shutdown::Write)?;

    let mut echoed = Vec::new();
    stream.read_to_end(&mut echoed)?;
    println!("[client] echoed {} bytes: {:?}", echoed.len(), String::from_utf8_lossy(&echoed));
    Ok(())
}
