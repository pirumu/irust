//! Lab (d), client side: a minimal HTTP/1.1 client that writes a raw GET by hand.
//!
//! Usage: http_client [HOST:PORT] [PATH]
//!   default 127.0.0.1:8080 /
//! Prints the exact request bytes, then the parsed status, headers and body.

use std::io::{self, Write};
use std::net::{TcpStream, ToSocketAddrs};

use lab_networking::read_response;

fn main() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let authority = args.next().unwrap_or_else(|| "127.0.0.1:8080".to_string());
    let path = args.next().unwrap_or_else(|| "/".to_string());

    // ToSocketAddrs resolves "host:port"; for a hostname this goes through getaddrinfo(3).
    let addrs: Vec<_> = authority.to_socket_addrs()?.collect();
    println!("[client] {authority} resolved to {addrs:?}");
    let mut stream = TcpStream::connect(&addrs[..])?;

    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {authority}\r\nUser-Agent: p04-http-client\r\nConnection: close\r\n\r\n"
    );
    println!("[client] >>> {request:?}");
    stream.write_all(request.as_bytes())?;

    let resp = read_response(&mut stream)?;
    println!("[client] <<< status {} {:?}", resp.status, resp.reason);
    for (name, value) in &resp.headers {
        println!("[client] <<< {name}: {value}");
    }
    println!("[client] <<< body ({} bytes): {:?}", resp.body.len(), String::from_utf8_lossy(&resp.body));
    Ok(())
}
