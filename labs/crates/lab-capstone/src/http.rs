//! Minimal HTTP/1.1 message handling (RFC 9112): request-line, header fields, and
//! `Connection: close` responses with `Content-Length`.
//!
//! Deliberately not supported: request bodies, chunked encoding, keep-alive, obs-fold,
//! bare-LF line endings. Every response closes the connection.

/// Upper bound on the request head (request-line + header fields + empty line).
pub const MAX_HEAD: usize = 8 * 1024;

/// Size of the `/big` body, large enough to need many `send` calls with [`crate::LAB_SNDBUF`].
pub const BIG_BODY: usize = 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub struct Request<'a> {
    pub method: &'a str,
    pub target: &'a str,
    pub version: &'a str,
    pub headers: Vec<(&'a str, &'a str)>,
}

impl Request<'_> {
    /// Field names are case-insensitive.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| *v)
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ParseError {
    /// No empty line within [`MAX_HEAD`] bytes.
    HeadTooLarge,
    NotUtf8,
    BadRequestLine,
    /// Well-formed `HTTP/x.y`, but major version is not 1.
    UnsupportedVersion,
    BadHeader,
    /// HTTP/1.1 request without `Host`.
    MissingHost,
}

/// Parses the head of a request.
///
/// Returns `Ok(None)` while the empty line that ends the head has not arrived yet; the caller
/// keeps the bytes and calls again after the next read. On success also returns the number of
/// bytes the head occupied (body bytes, if any, start there).
pub fn parse_request(buf: &[u8]) -> Result<Option<(Request<'_>, usize)>, ParseError> {
    let Some(end) = find(buf, b"\r\n\r\n") else {
        return if buf.len() > MAX_HEAD {
            Err(ParseError::HeadTooLarge)
        } else {
            Ok(None)
        };
    };
    if end + 4 > MAX_HEAD {
        return Err(ParseError::HeadTooLarge);
    }
    let head = std::str::from_utf8(&buf[..end]).map_err(|_| ParseError::NotUtf8)?;
    let mut lines = head.split("\r\n");

    // request-line = method SP request-target SP HTTP-version
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(ParseError::BadRequestLine);
    };
    if method.is_empty() || !method.bytes().all(is_tchar) || target.is_empty() {
        return Err(ParseError::BadRequestLine);
    }
    // HTTP-version = "HTTP" "/" DIGIT "." DIGIT
    let v = version.as_bytes();
    if v.len() != 8 || &v[..5] != b"HTTP/" || !v[5].is_ascii_digit() || v[6] != b'.' || !v[7].is_ascii_digit() {
        return Err(ParseError::BadRequestLine);
    }
    if v[5] != b'1' {
        return Err(ParseError::UnsupportedVersion);
    }

    // field-line = field-name ":" OWS field-value OWS
    let mut headers = Vec::new();
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(ParseError::BadHeader)?;
        // An empty name, whitespace before the colon, or a line starting with whitespace
        // (obs-fold) all fail the token check.
        if name.is_empty() || !name.bytes().all(is_tchar) {
            return Err(ParseError::BadHeader);
        }
        headers.push((name, value.trim_matches([' ', '\t'])));
    }

    let request = Request { method, target, version, headers };
    if request.version == "HTTP/1.1" && request.header("host").is_none() {
        return Err(ParseError::MissingHost);
    }
    Ok(Some((request, end + 4)))
}

/// tchar from RFC 9110 section 5.6.2: the characters allowed in a method and a field name.
fn is_tchar(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Serialises a complete `Connection: close` response.
///
/// With `head_only` (a HEAD request) the `Content-Length` still describes the body a GET
/// would get, but no body bytes are sent.
pub fn response(status: u16, extra_headers: &str, body: &[u8], head_only: bool) -> Vec<u8> {
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {len}\r\nContent-Type: text/plain\r\n{extra_headers}Connection: close\r\n\r\n",
        reason = reason(status),
        len = body.len(),
    );
    let mut out = Vec::with_capacity(head.len() + if head_only { 0 } else { body.len() });
    out.extend_from_slice(head.as_bytes());
    if !head_only {
        out.extend_from_slice(body);
    }
    out
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        501 => "Not Implemented",
        505 => "HTTP Version Not Supported",
        _ => "Unknown",
    }
}

pub fn error_response(err: ParseError) -> Vec<u8> {
    let status = match err {
        ParseError::UnsupportedVersion => 505,
        _ => 400,
    };
    response(status, "", format!("{err:?}\n").as_bytes(), false)
}

/// The routes both servers serve. `stats` renders the server's own counters for `/stats`.
pub fn route(req: &Request<'_>, stats: impl FnOnce() -> String) -> Vec<u8> {
    let head_only = match req.method {
        "GET" => false,
        "HEAD" => true,
        _ => return response(501, "", b"only GET and HEAD\n", false),
    };
    match req.target {
        "/" => response(200, "", b"hello from p07\n", head_only),
        "/big" => response(200, "", &vec![b'x'; BIG_BODY], head_only),
        "/stats" => response(200, "", format!("{}\n", stats()).as_bytes(), head_only),
        _ => response(404, "", b"not found\n", head_only),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incomplete_head_asks_for_more() {
        assert_eq!(parse_request(b"GET / HTTP/1.1\r\nHost: a\r\n"), Ok(None));
    }

    #[test]
    fn parses_request_line_and_fields() {
        let raw = b"GET /x?y=1 HTTP/1.1\r\nHost: example\r\nX-A:  v \t\r\n\r\nBODY";
        let (req, used) = parse_request(raw).unwrap().unwrap();
        assert_eq!((req.method, req.target, req.version), ("GET", "/x?y=1", "HTTP/1.1"));
        assert_eq!(req.header("x-a"), Some("v"));
        assert_eq!(&raw[used..], b"BODY");
    }

    #[test]
    fn whitespace_before_colon_is_rejected() {
        assert_eq!(
            parse_request(b"GET / HTTP/1.1\r\nHost : a\r\n\r\n").unwrap_err(),
            ParseError::BadHeader
        );
    }

    #[test]
    fn http11_requires_host_but_http10_does_not() {
        assert_eq!(parse_request(b"GET / HTTP/1.1\r\n\r\n").unwrap_err(), ParseError::MissingHost);
        assert!(parse_request(b"GET / HTTP/1.0\r\n\r\n").unwrap().is_some());
    }

    #[test]
    fn rejects_malformed_request_lines() {
        for raw in [
            &b"GET  / HTTP/1.1\r\nHost: a\r\n\r\n"[..],
            b"GET /\r\nHost: a\r\n\r\n",
            b"GET / HTTP/11\r\nHost: a\r\n\r\n",
        ] {
            assert_eq!(parse_request(raw).unwrap_err(), ParseError::BadRequestLine);
        }
        assert_eq!(
            parse_request(b"GET / HTTP/2.0\r\n\r\n").unwrap_err(),
            ParseError::UnsupportedVersion
        );
    }

    #[test]
    fn oversized_head_is_rejected_without_terminator() {
        let raw = vec![b'a'; MAX_HEAD + 1];
        assert_eq!(parse_request(&raw).unwrap_err(), ParseError::HeadTooLarge);
    }

    #[test]
    fn head_response_keeps_content_length_without_body() {
        let (req, _) = parse_request(b"HEAD / HTTP/1.1\r\nHost: a\r\n\r\n").unwrap().unwrap();
        let out = String::from_utf8(route(&req, String::new)).unwrap();
        assert!(out.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(out.contains("Content-Length: 15\r\n"));
        assert!(out.ends_with("\r\n\r\n"));
    }
}
