//! Shared helpers for the P4 networking labs: a minimal HTTP/1.1 message reader
//! (just enough of RFC 9112 for the capstone) and, on Linux, a wrapper around `ss`.

use std::io::{self, Read};

/// Upper bound for a request/response head; a peer that sends more is rejected.
pub const MAX_HEAD: usize = 8 * 1024;

/// Reads from `r` until the blank line that ends an HTTP head (`\r\n\r\n`).
///
/// Returns the head (without the blank line) and whatever body bytes arrived in
/// the same reads. TCP is a byte stream, so the head can arrive split across
/// several `read` calls, or glued to the start of the body.
pub fn read_head(r: &mut impl Read) -> io::Result<(String, Vec<u8>)> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        if let Some(end) = find_head_end(&buf) {
            let head = String::from_utf8(buf[..end].to_vec())
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "head is not UTF-8"))?;
            return Ok((head, buf[end + 4..].to_vec()));
        }
        if buf.len() > MAX_HEAD {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "head too large"));
        }
        let n = r.read(&mut chunk)?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "peer closed before end of head",
            ));
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

fn find_head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

/// Header lines of a head (everything after the start line), as (name, value).
pub fn header_fields(head: &str) -> Vec<(&str, &str)> {
    head.split("\r\n")
        .skip(1)
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name, value.trim()))
        })
        .collect()
}

/// Case-insensitive lookup: field names in HTTP are case-insensitive.
pub fn header<'a>(fields: &[(&'a str, &'a str)], name: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| *v)
}

/// A parsed response: status line plus a body framed by Content-Length or by close.
#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub reason: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Reads one HTTP/1.1 response from `r`.
///
/// Body framing follows the order of RFC 9112 section 6.3, reduced to the cases
/// this lab needs: `Content-Length` gives the exact length; without it the body
/// ends when the server closes the connection. Chunked encoding is refused.
pub fn read_response(r: &mut impl Read) -> io::Result<Response> {
    let (head, mut body) = read_head(r)?;
    let status_line = head.split("\r\n").next().unwrap_or_default();
    let mut parts = status_line.splitn(3, ' ');
    let version = parts.next().unwrap_or_default();
    if !version.starts_with("HTTP/1.") {
        return Err(bad(format!("not an HTTP/1.x status line: {status_line:?}")));
    }
    let status = parts
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| bad(format!("bad status code in {status_line:?}")))?;
    let reason = parts.next().unwrap_or_default().to_string();
    let fields = header_fields(&head);

    if header(&fields, "Transfer-Encoding").is_some() {
        return Err(bad("Transfer-Encoding is not supported by this lab client".into()));
    }
    match header(&fields, "Content-Length") {
        Some(len) => {
            let len: usize = len
                .parse()
                .map_err(|_| bad(format!("bad Content-Length {len:?}")))?;
            if body.len() > len {
                return Err(bad("more bytes than Content-Length".into()));
            }
            let already = body.len();
            body.resize(len, 0);
            r.read_exact(&mut body[already..])?;
        }
        None => {
            r.read_to_end(&mut body)?;
        }
    }
    Ok(Response {
        status,
        reason,
        headers: fields
            .into_iter()
            .map(|(n, v)| (n.to_string(), v.to_string()))
            .collect(),
        body,
    })
}

fn bad(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

/// Runs `ss <args>` and returns its stdout (Linux only; `ss` is in iproute2).
#[cfg(target_os = "linux")]
pub fn ss(args: &[&str]) -> io::Result<String> {
    let out = std::process::Command::new("ss").args(args).output()?;
    if !out.status.success() {
        return Err(io::Error::other(format!(
            "ss {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Message printed by the Linux-only binaries when built for another OS.
pub const LINUX_ONLY: &str = "This lab needs Linux (ss, /proc, Linux socket options). Run it with:\n  ./labs/linux.sh cargo run -p lab-networking --bin <name>";

#[cfg(test)]
mod tests {
    use super::*;

    /// Splits the input into reads of at most `step` bytes, like a TCP peer
    /// whose segments arrive one at a time.
    struct Trickle<'a> {
        data: &'a [u8],
        step: usize,
    }

    impl Read for Trickle<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = self.step.min(buf.len()).min(self.data.len());
            buf[..n].copy_from_slice(&self.data[..n]);
            self.data = &self.data[n..];
            Ok(n)
        }
    }

    #[test]
    fn content_length_body_survives_split_reads() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nX-A: b\r\n\r\nhello";
        let resp = read_response(&mut Trickle { data: raw, step: 3 }).unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.reason, "OK");
        assert_eq!(resp.body, b"hello");
    }

    #[test]
    fn content_length_stops_before_trailing_bytes() {
        // A keep-alive server may already have the next response queued.
        let raw = b"HTTP/1.1 404 Not Found\r\ncontent-length: 2\r\n\r\nnoHTTP/1.1 200";
        let mut r = Trickle { data: raw, step: 1 };
        let resp = read_response(&mut r).unwrap();
        assert_eq!(resp.status, 404);
        assert_eq!(resp.body, b"no");
        assert_eq!(r.data, b"HTTP/1.1 200");
    }

    #[test]
    fn body_without_length_ends_at_close() {
        let raw = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nuntil eof";
        let resp = read_response(&mut Trickle { data: raw, step: 4 }).unwrap();
        assert_eq!(resp.body, b"until eof");
    }

    #[test]
    fn eof_inside_head_is_an_error() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Le";
        let err = read_response(&mut Trickle { data: raw, step: 64 }).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn chunked_is_refused() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n";
        let err = read_response(&mut Trickle { data: raw, step: 64 }).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }
}
