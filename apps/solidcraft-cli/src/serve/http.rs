//! Just enough HTTP/1.1 for `serve`: one request per connection, read strictly, and a response
//! with `Connection: close`. Request heads are capped, bodies need `Content-Length` (chunked
//! request bodies are refused, so a proxy and this server can never disagree on where a request
//! ends), and nothing is read past what the head announced.

use std::io::{Read, Write};

/// The longest request head (request line and headers).
pub const MAX_HEAD: usize = 16 << 10;
/// The most headers a request may have.
pub const MAX_HEADERS: usize = 64;

/// A refusal, as a status and words for the client.
#[derive(Debug)]
pub struct HttpError {
    pub status: u16,
    pub message: String,
}

impl HttpError {
    pub fn new(status: u16, message: impl Into<String>) -> Self {
        HttpError { status, message: message.into() }
    }
}

/// A request's head, its body still on the connection.
#[derive(Debug)]
pub struct Head {
    pub method: String,
    /// The path, percent-decoded, without the query.
    pub path: String,
    /// Header names in lower case, values trimmed.
    pub headers: Vec<(String, String)>,
    /// What the client sent after the head in the same read: the start of the body.
    pub rest: Vec<u8>,
}

impl Head {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    /// The body's length: `None` for no body, an error for a body this server won't read.
    pub fn content_length(&self) -> Result<Option<u64>, HttpError> {
        if self.header("transfer-encoding").is_some() {
            return Err(HttpError::new(411, "send the body with Content-Length; chunked request bodies are not accepted"));
        }
        let mut lengths = self.headers.iter().filter(|(n, _)| n == "content-length");
        let Some((_, first)) = lengths.next() else { return Ok(None) };
        if lengths.next().is_some() {
            return Err(HttpError::new(400, "more than one Content-Length"));
        }
        if first.is_empty() || !first.bytes().all(|b| b.is_ascii_digit()) {
            return Err(HttpError::new(400, "Content-Length is not a number"));
        }
        first.parse::<u64>().map(Some).map_err(|_| HttpError::new(400, "Content-Length is too large"))
    }

    /// The body, at most `max` bytes, read whole into memory.
    pub fn read_body(&self, stream: &mut impl Read, max: u64) -> Result<Vec<u8>, HttpError> {
        let mut body = Vec::new();
        self.copy_body(stream, max, &mut body)?;
        Ok(body)
    }

    /// Copy the body, at most `max` bytes, to `out`. A missing length is an error only when
    /// the client sent bytes after the head.
    pub fn copy_body(&self, stream: &mut impl Read, max: u64, out: &mut impl Write) -> Result<u64, HttpError> {
        let len = match self.content_length()? {
            Some(len) => len,
            None if self.rest.is_empty() => 0,
            None => return Err(HttpError::new(411, "send the body with Content-Length")),
        };
        if len > max {
            return Err(HttpError::new(413, format!("the body is {len} bytes; this server takes at most {max}")));
        }
        let first = self.rest.get(..usize::try_from(len).unwrap_or(usize::MAX).min(self.rest.len())).unwrap_or(&[]);
        out.write_all(first).map_err(io_error)?;
        let remaining = len - first.len() as u64;
        let copied = std::io::copy(&mut stream.take(remaining), out).map_err(io_error)?;
        if copied < remaining {
            return Err(HttpError::new(400, "the connection closed before the whole body arrived"));
        }
        Ok(len)
    }
}

fn io_error(e: std::io::Error) -> HttpError {
    HttpError::new(500, format!("could not read or write: {e}"))
}

/// Read a request head from `stream`. `Ok(None)`: the client closed without sending one.
pub fn read_head(stream: &mut impl Read) -> Result<Option<Head>, HttpError> {
    let mut buf: Vec<u8> = Vec::with_capacity(1024);
    let mut chunk = [0u8; 4096];
    let end = loop {
        if let Some(i) = find(&buf, b"\r\n\r\n") {
            break i;
        }
        if buf.len() > MAX_HEAD {
            return Err(HttpError::new(431, "the request head is too large"));
        }
        let n = match stream.read(&mut chunk) {
            Ok(0) if buf.is_empty() => return Ok(None),
            Ok(0) => return Err(HttpError::new(400, "the connection closed in the request head")),
            Ok(n) => n,
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                return Err(HttpError::new(408, "the request took too long to arrive"));
            }
            Err(e) => return Err(io_error(e)),
        };
        buf.extend_from_slice(chunk.get(..n).unwrap_or(&[]));
    };
    if end > MAX_HEAD {
        return Err(HttpError::new(431, "the request head is too large"));
    }
    let rest = buf.get(end + 4..).unwrap_or(&[]).to_vec();
    let text = std::str::from_utf8(buf.get(..end).unwrap_or(&[])).map_err(|_| HttpError::new(400, "the request head is not text"))?;
    let mut lines = text.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split(' ');
    let (Some(method), Some(target), Some(version), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return Err(HttpError::new(400, "a bad request line"));
    };
    if !matches!(version, "HTTP/1.1" | "HTTP/1.0") {
        return Err(HttpError::new(505, "only HTTP/1.1 is served"));
    }
    if method.is_empty() || !method.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(HttpError::new(400, "a bad method"));
    }
    if !target.starts_with('/') {
        return Err(HttpError::new(400, "the request target must be a path"));
    }
    let raw_path = target.split(['?', '#']).next().unwrap_or("/");
    let path = percent_decode(raw_path).ok_or_else(|| HttpError::new(400, "a bad percent escape in the path"))?;
    let mut headers = Vec::new();
    for line in lines {
        if headers.len() >= MAX_HEADERS {
            return Err(HttpError::new(431, "too many headers"));
        }
        // Folded continuation lines are obsolete and a classic smuggling vector.
        if line.starts_with([' ', '\t']) {
            return Err(HttpError::new(400, "folded headers are not accepted"));
        }
        let Some((name, value)) = line.split_once(':') else { return Err(HttpError::new(400, "a header without a colon")) };
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)) {
            return Err(HttpError::new(400, "a bad header name"));
        }
        if value.bytes().any(|b| b < 0x20 && b != b'\t') {
            return Err(HttpError::new(400, "a bad header value"));
        }
        headers.push((name.to_ascii_lowercase(), value.trim().to_string()));
    }
    Ok(Some(Head { method: method.to_string(), path, headers, rest }))
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// `%XX` escapes decoded; `None` for a bad escape, a decoded NUL or text that is not UTF-8.
pub fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        if b == b'%' {
            let hex = bytes.get(i + 1..i + 3)?;
            let v = u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?;
            out.push(v);
            i += 3;
        } else {
            out.push(b);
            i += 1;
        }
    }
    if out.contains(&0) {
        return None;
    }
    String::from_utf8(out).ok()
}

/// A response's body.
pub enum Body {
    Bytes(Vec<u8>),
    /// A file and its length, copied to the connection.
    File(std::fs::File, u64),
}

pub struct Response {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: Body,
}

impl Response {
    pub fn new(status: u16, content_type: &str, body: Vec<u8>) -> Self {
        Response { status, headers: vec![("Content-Type", content_type.to_string())], body: Body::Bytes(body) }
    }
    pub fn json(status: u16, v: &serde_json::Value) -> Self {
        Response::new(status, "application/json", v.to_string().into_bytes()).with("Cache-Control", "no-store")
    }
    pub fn empty(status: u16) -> Self {
        Response { status, headers: Vec::new(), body: Body::Bytes(Vec::new()) }
    }
    pub fn error(e: &HttpError) -> Self {
        let mut r = Response::json(e.status, &serde_json::json!({"error": e.message}));
        if e.status == 401 {
            r = r.with("WWW-Authenticate", "Bearer");
        }
        r
    }
    pub fn with(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }

    /// Write the response; with `head_only` (a HEAD request) without its body.
    pub fn write_to(self, out: &mut impl Write, head_only: bool) -> std::io::Result<()> {
        let len = match &self.body {
            Body::Bytes(b) => b.len() as u64,
            Body::File(_, len) => *len,
        };
        let mut head = format!("HTTP/1.1 {} {}\r\n", self.status, reason(self.status));
        for (n, v) in &self.headers {
            // Values are ours, but never let one end the head early.
            let v: String = v.chars().filter(|c| !c.is_control()).collect();
            head.push_str(&format!("{n}: {v}\r\n"));
        }
        head.push_str(&format!(
            "Content-Length: {len}\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\n\r\n"
        ));
        out.write_all(head.as_bytes())?;
        if !head_only {
            match self.body {
                Body::Bytes(b) => out.write_all(&b)?,
                Body::File(f, len) => {
                    std::io::copy(&mut f.take(len), out)?;
                }
            }
        }
        out.flush()
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        409 => "Conflict",
        411 => "Length Required",
        412 => "Precondition Failed",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        505 => "HTTP Version Not Supported",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(raw: &str) -> Result<Option<Head>, HttpError> {
        read_head(&mut raw.as_bytes())
    }

    #[test]
    fn reads_a_request_head() {
        let h = head("PUT /api/v1/files/a%20b.sc?x=1 HTTP/1.1\r\nHost: h\r\nContent-Length: 3\r\n\r\nabc").unwrap().unwrap();
        assert_eq!(h.method, "PUT");
        assert_eq!(h.path, "/api/v1/files/a b.sc");
        assert_eq!(h.header("host"), Some("h"));
        assert_eq!(h.content_length().unwrap(), Some(3));
        assert_eq!(h.read_body(&mut &b""[..], 10).unwrap(), b"abc");
    }

    #[test]
    fn refuses_what_could_be_smuggled_or_is_too_big() {
        let te = head("POST /mcp HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n").unwrap().unwrap();
        assert_eq!(te.content_length().unwrap_err().status, 411);
        let two = head("POST /mcp HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\n").unwrap().unwrap();
        assert_eq!(two.content_length().unwrap_err().status, 400);
        let plus = head("POST /mcp HTTP/1.1\r\nContent-Length: +1\r\n\r\n").unwrap().unwrap();
        assert_eq!(plus.content_length().unwrap_err().status, 400);
        assert_eq!(head("GET / HTTP/1.1\r\nA: b\r\n c\r\n\r\n").unwrap_err().status, 400);
        assert_eq!(head("GET http://x/ HTTP/1.1\r\n\r\n").unwrap_err().status, 400);
        assert_eq!(head("GET /%00 HTTP/1.1\r\n\r\n").unwrap_err().status, 400);
        assert_eq!(head("GET /%zz HTTP/1.1\r\n\r\n").unwrap_err().status, 400);
        let big = format!("GET / HTTP/1.1\r\nA: {}\r\n\r\n", "x".repeat(MAX_HEAD));
        assert_eq!(head(&big).unwrap_err().status, 431);
        let many: String = (0..=MAX_HEADERS).map(|i| format!("H{i}: v\r\n")).collect();
        assert_eq!(head(&format!("GET / HTTP/1.1\r\n{many}\r\n")).unwrap_err().status, 431);
        let long = head("PUT /x HTTP/1.1\r\nContent-Length: 11\r\n\r\n").unwrap().unwrap();
        assert_eq!(long.read_body(&mut &b"hello world"[..], 10).unwrap_err().status, 413);
        let short = head("PUT /x HTTP/1.1\r\nContent-Length: 5\r\n\r\nab").unwrap().unwrap();
        assert_eq!(short.read_body(&mut &b""[..], 10).unwrap_err().status, 400);
        assert!(head("").unwrap().is_none());
    }
}
