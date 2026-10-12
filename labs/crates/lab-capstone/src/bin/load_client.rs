//! Load client: many concurrent connections against either lab server, with checks.
//!
//! ```text
//! load_client <host:port> [--conns N] [--reqs N] [--path P] [--split]
//! ```
//!
//! Starts `--conns` threads behind a barrier so their connections open at the same time.
//! Each thread sends `--reqs` requests, one fresh connection each (the servers close after
//! every response). `--split` sends each request head in two writes 20 ms apart, so every
//! server sees incomplete heads on many connections at once.
//!
//! Every response is checked: a status line, a `Content-Length` header, and exactly that
//! many body bytes before EOF. Exit code 1 if any request failed.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

struct Args {
    addr: String,
    conns: usize,
    reqs: usize,
    path: String,
    split: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let addr = it.next().ok_or("usage: load_client <host:port> [--conns N] [--reqs N] [--path P] [--split]")?;
    let mut args = Args { addr, conns: 50, reqs: 20, path: "/".into(), split: false };
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--conns" => args.conns = value()?.parse().map_err(|e| format!("--conns: {e}"))?,
            "--reqs" => args.reqs = value()?.parse().map_err(|e| format!("--reqs: {e}"))?,
            "--path" => args.path = value()?,
            "--split" => args.split = true,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(args)
}

/// Returns (status, body length) after checking the body against Content-Length.
fn one_request(addr: &str, path: &str, split: bool) -> Result<(u16, usize), String> {
    let mut s = TcpStream::connect(addr).map_err(|e| format!("connect: {e}"))?;
    let req = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nUser-Agent: p07-load-client\r\nConnection: close\r\n\r\n");
    let bytes = req.as_bytes();
    if split {
        let mid = bytes.len() / 2;
        s.write_all(&bytes[..mid]).map_err(|e| format!("write: {e}"))?;
        thread::sleep(Duration::from_millis(20));
        s.write_all(&bytes[mid..]).map_err(|e| format!("write: {e}"))?;
    } else {
        s.write_all(bytes).map_err(|e| format!("write: {e}"))?;
    }
    let mut resp = Vec::new();
    s.read_to_end(&mut resp).map_err(|e| format!("read: {e}"))?;

    let head_end = resp
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| format!("no end of head in {} bytes", resp.len()))?;
    let head = std::str::from_utf8(&resp[..head_end]).map_err(|_| "head is not UTF-8")?;
    let mut lines = head.split("\r\n");
    let status: u16 = lines
        .next()
        .and_then(|l| l.strip_prefix("HTTP/1.1 "))
        .and_then(|l| l.get(..3))
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| format!("bad status line in {head:?}"))?;
    let content_length: usize = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(n, _)| n.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse().ok())
        .ok_or("missing Content-Length")?;
    let body = resp.len() - head_end - 4;
    if body != content_length {
        return Err(format!("Content-Length {content_length} but {body} body bytes"));
    }
    Ok((status, body))
}

struct ThreadResult {
    latencies: Vec<Duration>,
    statuses: BTreeMap<u16, usize>,
    body_bytes: usize,
    errors: Vec<String>,
}

fn main() {
    let args = match parse_args() {
        Ok(a) => Arc::new(a),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let barrier = Arc::new(Barrier::new(args.conns));
    let started = Instant::now();
    let handles: Vec<_> = (0..args.conns)
        .map(|_| {
            let args = Arc::clone(&args);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                let mut r = ThreadResult {
                    latencies: Vec::with_capacity(args.reqs),
                    statuses: BTreeMap::new(),
                    body_bytes: 0,
                    errors: Vec::new(),
                };
                barrier.wait();
                for _ in 0..args.reqs {
                    let t = Instant::now();
                    match one_request(&args.addr, &args.path, args.split) {
                        Ok((status, body)) => {
                            r.latencies.push(t.elapsed());
                            *r.statuses.entry(status).or_default() += 1;
                            r.body_bytes += body;
                        }
                        Err(e) => r.errors.push(e),
                    }
                }
                r
            })
        })
        .collect();

    let mut latencies = Vec::new();
    let mut statuses = BTreeMap::new();
    let mut body_bytes = 0;
    let mut errors = Vec::new();
    for h in handles {
        let r = h.join().expect("client thread panicked");
        latencies.extend(r.latencies);
        for (s, n) in r.statuses {
            *statuses.entry(s).or_insert(0) += n;
        }
        body_bytes += r.body_bytes;
        errors.extend(r.errors);
    }
    let elapsed = started.elapsed();
    latencies.sort();
    let pct = |p: f64| {
        latencies
            .get(((latencies.len() as f64 * p) as usize).min(latencies.len().saturating_sub(1)))
            .copied()
            .unwrap_or_default()
    };

    let total = args.conns * args.reqs;
    println!(
        "target={} path={} conns={} reqs/conn={} split={}",
        args.addr, args.path, args.conns, args.reqs, args.split
    );
    println!(
        "ok={} failed={} elapsed={:.3}s rate={:.0} req/s body_bytes={}",
        latencies.len(),
        errors.len(),
        elapsed.as_secs_f64(),
        total as f64 / elapsed.as_secs_f64(),
        body_bytes
    );
    let statuses: Vec<String> = statuses.iter().map(|(s, n)| format!("{s}x{n}")).collect();
    println!("status: {}", statuses.join(" "));
    println!(
        "latency: p50={:?} p99={:?} max={:?}",
        pct(0.50),
        pct(0.99),
        latencies.last().copied().unwrap_or_default()
    );
    for e in errors.iter().take(5) {
        eprintln!("error: {e}");
    }
    if !errors.is_empty() {
        std::process::exit(1);
    }
}
