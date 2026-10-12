//! Lab (d), server side: a tiny HTTP/1.1 server on std::net, one thread per connection.
//!
//! Usage: http_server [ADDR] [MAX_CONNS]
//!   ADDR       default 127.0.0.1:8080
//!   MAX_CONNS  exit after serving this many connections; omitted = forever.
//!
//! One request per connection: every response carries `Connection: close`
//! and the server closes after writing it.

use std::io::{self, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

use lab_networking::{header, header_fields, read_head};

fn main() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let addr = args.next().unwrap_or_else(|| "127.0.0.1:8080".to_string());
    let max_conns: Option<usize> = match args.next() {
        Some(n) => Some(
            n.parse()
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "MAX_CONNS must be a number"))?,
        ),
        None => None,
    };

    let listener = TcpListener::bind(&addr)?;
    eprintln!("[http] listening on {}", listener.local_addr()?);
    let mut workers = Vec::new();
    for (i, conn) in listener.incoming().enumerate() {
        let stream = conn?;
        workers.push(thread::spawn(move || {
            if let Err(e) = serve(stream) {
                eprintln!("[http] connection error: {e}");
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

fn serve(mut stream: TcpStream) -> io::Result<()> {
    let (head, _body) = match read_head(&mut stream) {
        Ok(v) => v,
        Err(e) if e.kind() == io::ErrorKind::InvalidData => {
            return respond(&mut stream, 400, "Bad Request", "bad request head\n");
        }
        Err(e) => return Err(e),
    };
    let request_line = head.split("\r\n").next().unwrap_or_default();
    eprintln!("[http] {} {request_line}", stream.peer_addr()?);

    let parts: Vec<&str> = request_line.split(' ').collect();
    let [method, target, version] = parts[..] else {
        return respond(&mut stream, 400, "Bad Request", "bad request line\n");
    };
    if version != "HTTP/1.1" {
        return respond(&mut stream, 505, "HTTP Version Not Supported", "only HTTP/1.1\n");
    }
    // RFC 9112 section 3.2: a server MUST answer 400 to an HTTP/1.1 request without Host.
    let fields = header_fields(&head);
    let Some(host) = header(&fields, "Host") else {
        return respond(&mut stream, 400, "Bad Request", "missing Host header\n");
    };
    match (method, target) {
        ("GET", "/") => respond(&mut stream, 200, "OK", &format!("hello from p04 http_server, Host={host}\n")),
        ("GET", _) => respond(&mut stream, 404, "Not Found", "no such path\n"),
        _ => respond(&mut stream, 405, "Method Not Allowed", "only GET\n"),
    }
}

fn respond(stream: &mut TcpStream, status: u16, reason: &str, body: &str) -> io::Result<()> {
    let msg = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    // One write_all of head + body: no small separate writes for Nagle to hold back.
    stream.write_all(msg.as_bytes())
}
