//! Local loopback MCP Streamable HTTP transport (`POST /mcp`, `GET /mcp`, `DELETE /mcp`).
//!
//! Zero external HTTP dependencies: built directly on [`std::net::TcpListener`] with strict
//! security defaults:
//! - Loopback-only binding (`127.0.0.1` / `[::1]`).
//! - Mandatory `Authorization: Bearer <token>` verified in constant time.
//! - DNS rebinding protection: rejects non-loopback `Host` and `Origin` headers.
//! - Bounded header and body sizes (16 MiB default), bounded concurrent connections and sessions.
//! - Strict FIFO serialized dispatch to the single shared [`Server`], so multiple local MCP
//!   clients share one warm backend session without data races or corruption.

use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::fmt;
use std::hash::{BuildHasher, Hasher};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::server::{PARSE_ERROR, Server, Tools, error as rpc_error};

/// Canonical MCP Streamable HTTP endpoint path.
pub const MCP_PATH: &str = "/mcp";
/// Default maximum HTTP request body size (16 MiB).
pub const DEFAULT_MAX_BODY: usize = 16 << 20;
/// Default maximum concurrent HTTP TCP connections.
pub const DEFAULT_MAX_CONNECTIONS: usize = 64;
/// Default maximum active `Mcp-Session-Id` sessions tracked concurrently.
pub const DEFAULT_MAX_SESSIONS: usize = 256;

const MAX_HEADER_LINE: usize = 8 * 1024;
const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_HEADERS: usize = 128;
const MAX_CHUNKS: usize = 65_536;
const MAX_CHUNK_LINE: usize = 128;
const MAX_TRAILER_LINES: usize = 64;
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const SESSION_IDLE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const STOP_POLL: Duration = Duration::from_millis(50);
const ACCEPT_POLL: Duration = Duration::from_millis(5);

/// Type alias for an optional cryptographically random session ID generator.
pub type SessionIdGenerator = Arc<dyn Fn() -> Option<String> + Send + Sync>;

/// Configuration for [`Server::serve_http`] and [`Server::serve_http_listener`].
#[derive(Clone)]
pub struct HttpConfig {
    /// Mandatory bearer token required in `Authorization: Bearer <token>`.
    pub token: String,
    /// Maximum JSON-RPC request body size in bytes (default: 16 MiB).
    pub max_body: usize,
    /// Maximum concurrent TCP connections served at once (default: 64).
    pub max_connections: usize,
    /// Maximum active `Mcp-Session-Id` entries tracked at once (default: 256).
    pub max_sessions: usize,
    /// Shutdown signal. Setting this to `true` stops the listener and drains cleanly.
    pub shutdown: Arc<AtomicBool>,
    /// Optional generator for `Mcp-Session-Id` values.
    pub session_id_generator: Option<SessionIdGenerator>,
}

impl fmt::Debug for HttpConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpConfig")
            .field("token", &"[REDACTED]")
            .field("max_body", &self.max_body)
            .field("max_connections", &self.max_connections)
            .field("max_sessions", &self.max_sessions)
            .field("shutdown", &self.shutdown.load(Ordering::Relaxed))
            .finish()
    }
}

impl HttpConfig {
    /// New HTTP transport configuration with `token` and safe defaults.
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            max_body: DEFAULT_MAX_BODY,
            max_connections: DEFAULT_MAX_CONNECTIONS,
            max_sessions: DEFAULT_MAX_SESSIONS,
            shutdown: Arc::new(AtomicBool::new(false)),
            session_id_generator: None,
        }
    }

    pub fn with_max_body(mut self, max_body: usize) -> Self {
        self.max_body = max_body;
        self
    }

    pub fn with_max_connections(mut self, max_connections: usize) -> Self {
        self.max_connections = max_connections;
        self
    }

    pub fn with_max_sessions(mut self, max_sessions: usize) -> Self {
        self.max_sessions = max_sessions;
        self
    }

    pub fn with_shutdown(mut self, shutdown: Arc<AtomicBool>) -> Self {
        self.shutdown = shutdown;
        self
    }

    pub fn with_session_id_generator(mut self, generator: impl Fn() -> Option<String> + Send + Sync + 'static) -> Self {
        self.session_id_generator = Some(Arc::new(generator));
        self
    }

    /// Validate that the configuration is safe to serve.
    pub fn validate(&self) -> io::Result<()> {
        if self.token.is_empty() || !self.token.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "MCP HTTP bearer token must be non-empty printable ASCII without whitespace"));
        }
        if self.max_body == 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "max_body must be positive"));
        }
        if self.max_connections == 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "max_connections must be positive"));
        }
        if self.max_sessions == 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "max_sessions must be positive"));
        }
        Ok(())
    }
}

/// Handle to stop a running MCP HTTP server cleanly.
#[derive(Clone, Debug)]
pub struct HttpStop {
    addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
}

impl HttpStop {
    pub fn new(addr: SocketAddr, shutdown: Arc<AtomicBool>) -> Self {
        Self { addr, shutdown }
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn shutdown_flag(&self) -> &Arc<AtomicBool> {
        &self.shutdown
    }

    /// Signal the server to stop accepting new connections and exit cleanly.
    pub fn stop(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }
}

/// Parse an `--http` bind address, rejecting any non-loopback address.
///
/// Accepts `127.0.0.1:PORT`, `[::1]:PORT`, any `127.x.y.z:PORT`, or `localhost:PORT`
/// (mapped directly to `127.0.0.1:PORT`). Port `0` selects an ephemeral port.
pub fn parse_loopback_addr(raw: &str) -> Result<SocketAddr, String> {
    let trimmed = raw.trim();
    if let Some((host, port_str)) = trimmed.rsplit_once(':')
        && host.eq_ignore_ascii_case("localhost")
    {
        let port = port_str
            .parse::<u16>()
            .map_err(|_| format!("invalid --http address `{raw}`: expected a loopback HOST:PORT such as 127.0.0.1:8765 or 127.0.0.1:0"))?;
        return Ok(SocketAddr::from(([127, 0, 0, 1], port)));
    }
    let addr = trimmed
        .parse::<SocketAddr>()
        .map_err(|_| format!("invalid --http address `{raw}`: expected a loopback HOST:PORT such as 127.0.0.1:8765, 127.0.0.1:0, or [::1]:8765"))?;
    if !addr.ip().is_loopback() {
        return Err(format!(
            "refusing to bind MCP HTTP server to non-loopback address {addr}: only loopback addresses (127.0.0.1 or [::1]) are permitted"
        ));
    }
    Ok(addr)
}

/// Bind a [`TcpListener`] on `addr`, refusing any non-loopback address.
pub fn bind_loopback(addr: SocketAddr) -> io::Result<TcpListener> {
    if !addr.ip().is_loopback() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("refusing to bind MCP HTTP server to non-loopback address {addr}: only loopback addresses (127.0.0.1 or [::1]) are permitted"),
        ));
    }
    let listener = TcpListener::bind(addr)?;
    let local = listener.local_addr()?;
    if !local.ip().is_loopback() {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("refusing to serve MCP HTTP on non-loopback address {local}")));
    }
    Ok(listener)
}

/// Canonical Streamable HTTP endpoint URL for a bound loopback socket address.
pub fn endpoint_url(addr: SocketAddr) -> String {
    match addr {
        SocketAddr::V4(v4) => format!("http://{}:{}{MCP_PATH}", v4.ip(), v4.port()),
        SocketAddr::V6(v6) => format!("http://[{}]:{}{MCP_PATH}", v6.ip(), v6.port()),
    }
}

/// Constant-time token comparison.
pub fn tokens_match(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let diff = a.iter().zip(b.iter()).fold(0u8, |acc, (&x, &y)| acc | (x ^ y));
    std::hint::black_box(diff) == 0
}

/// Whether an HTTP `Origin` header identifies a loopback origin (`http://` or `https://` to
/// `localhost`, `127.x.y.z`, or `[::1]`, with optional port and no path/userinfo).
pub fn is_loopback_origin(origin: &str) -> bool {
    let rest = if origin.len() >= 7 && origin.is_char_boundary(7) && origin[..7].eq_ignore_ascii_case("http://") {
        &origin[7..]
    } else if origin.len() >= 8 && origin.is_char_boundary(8) && origin[..8].eq_ignore_ascii_case("https://") {
        &origin[8..]
    } else {
        return false;
    };
    if rest.is_empty() || rest.bytes().any(|b| matches!(b, b'/' | b'?' | b'#' | b'@' | b'\\') || b.is_ascii_whitespace()) {
        return false;
    }
    is_loopback_host_port(rest)
}

/// Whether an HTTP `Host` header value (or origin authority) names a loopback host with an
/// optional valid TCP port.
pub fn is_loopback_host_port(authority: &str) -> bool {
    if authority.is_empty() || authority.bytes().any(|b| b.is_ascii_whitespace() || !b.is_ascii()) {
        return false;
    }
    if let Some(rest) = authority.strip_prefix('[') {
        let Some((ip_str, after)) = rest.split_once(']') else {
            return false;
        };
        if !valid_optional_port(after) {
            return false;
        }
        return ip_str.parse::<Ipv6Addr>().is_ok_and(|ip| ip.is_loopback());
    }
    let host = match authority.rsplit_once(':') {
        Some((h, port)) => {
            if h.contains(':') || !valid_port_digits(port) {
                return false;
            }
            h
        }
        None => authority,
    };
    host.eq_ignore_ascii_case("localhost") || host.parse::<Ipv4Addr>().is_ok_and(|ip| ip.is_loopback())
}

fn valid_optional_port(after: &str) -> bool {
    if after.is_empty() {
        return true;
    }
    after.strip_prefix(':').is_some_and(valid_port_digits)
}

fn valid_port_digits(port: &str) -> bool {
    !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) && port.parse::<u16>().is_ok()
}

/// Fallback 32-byte hex session ID generator using OS-seeded `RandomState` hashers.
fn fallback_session_id() -> Option<String> {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0u128, |d| d.as_nanos());
    let mut out = String::with_capacity(64);
    for lane in 0..4u64 {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(lane);
        h.write_u64(seq);
        h.write_u32(pid);
        h.write_u128(nanos);
        let word = h.finish();
        for b in word.to_le_bytes() {
            let _ = fmt::Write::write_fmt(&mut out, format_args!("{b:02x}"));
        }
    }
    Some(out)
}

struct SessionStore {
    sessions: HashMap<String, Instant>,
    max_sessions: usize,
    generator: Option<SessionIdGenerator>,
}

impl SessionStore {
    fn new(max_sessions: usize, generator: Option<SessionIdGenerator>) -> Self {
        Self { sessions: HashMap::new(), max_sessions, generator }
    }

    fn create(&mut self) -> Option<String> {
        if self.sessions.len() >= self.max_sessions {
            let now = Instant::now();
            self.sessions.retain(|_, last_used| now.saturating_duration_since(*last_used) < SESSION_IDLE_TTL);
            if self.sessions.len() >= self.max_sessions {
                return None;
            }
        }
        for _ in 0..8 {
            let candidate = match &self.generator {
                Some(gen_fn) => gen_fn()?,
                None => fallback_session_id()?,
            };
            if candidate.is_empty() || !candidate.bytes().all(|b| b.is_ascii_graphic()) {
                continue;
            }
            if !self.sessions.contains_key(&candidate) {
                self.sessions.insert(candidate.clone(), Instant::now());
                return Some(candidate);
            }
        }
        None
    }

    fn touch(&mut self, id: &str) -> bool {
        if let Some(last_used) = self.sessions.get_mut(id) {
            *last_used = Instant::now();
            true
        } else {
            false
        }
    }

    fn remove(&mut self, id: &str) -> bool {
        self.sessions.remove(id).is_some()
    }
}

#[derive(Default)]
struct ActiveStreams {
    next_id: AtomicU64,
    streams: Mutex<Vec<(u64, TcpStream)>>,
}

impl ActiveStreams {
    fn lock(&self) -> MutexGuard<'_, Vec<(u64, TcpStream)>> {
        self.streams.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn insert(&self, stream: &TcpStream) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        if let Ok(cloned) = stream.try_clone() {
            self.lock().push((id, cloned));
        }
        id
    }

    fn remove(&self, id: u64) {
        self.lock().retain(|(entry_id, _)| *entry_id != id);
    }

    fn close_all(&self) {
        for (_, stream) in self.lock().drain(..) {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

struct ConnectionGuard<'a> {
    shared: &'a HttpShared,
    stream_id: u64,
}

impl Drop for ConnectionGuard<'_> {
    fn drop(&mut self) {
        self.shared.open_connections.fetch_sub(1, Ordering::Relaxed);
        self.shared.active_streams.remove(self.stream_id);
    }
}

struct HttpShared {
    token: String,
    max_body: usize,
    max_connections: usize,
    shutdown: Arc<AtomicBool>,
    sessions: Mutex<SessionStore>,
    open_connections: AtomicUsize,
    active_streams: ActiveStreams,
}

impl HttpShared {
    fn sessions(&self) -> MutexGuard<'_, SessionStore> {
        self.sessions.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

struct DispatchJob {
    value: Value,
    reply_tx: mpsc::Sender<Option<Value>>,
}

impl<T: Tools> Server<T> {
    /// Bind a loopback-only TCP listener on `addr` and serve MCP Streamable HTTP until
    /// `config.shutdown` is triggered.
    pub fn serve_http(&mut self, addr: SocketAddr, config: HttpConfig) -> io::Result<()> {
        config.validate()?;
        let listener = bind_loopback(addr)?;
        self.serve_http_listener(listener, config)
    }

    /// Serve MCP Streamable HTTP on an already-bound loopback `listener` until `config.shutdown`
    /// is triggered.
    ///
    /// Concurrent HTTP clients are accepted on bounded worker threads and dispatched onto this
    /// server in strict FIFO order on the calling thread, so multiple clients share one warm
    /// backend without requiring `T: Send` and without corrupting shared state.
    pub fn serve_http_listener(&mut self, listener: TcpListener, config: HttpConfig) -> io::Result<()> {
        config.validate()?;
        let local_addr = listener.local_addr()?;
        if !local_addr.ip().is_loopback() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("refusing to serve MCP HTTP on non-loopback address {local_addr}: only loopback addresses are permitted"),
            ));
        }
        listener.set_nonblocking(true)?;

        let shared = Arc::new(HttpShared {
            token: config.token,
            max_body: config.max_body,
            max_connections: config.max_connections,
            shutdown: config.shutdown,
            sessions: Mutex::new(SessionStore::new(config.max_sessions, config.session_id_generator)),
            open_connections: AtomicUsize::new(0),
            active_streams: ActiveStreams::default(),
        });
        let (work_tx, work_rx) = mpsc::channel::<DispatchJob>();

        std::thread::scope(|scope| {
            let accept_shared = Arc::clone(&shared);
            let accept_tx = work_tx;
            scope.spawn(move || {
                accept_loop(listener, scope, &accept_shared, accept_tx);
            });

            for job in work_rx {
                let reply = self.handle_value(job.value);
                let _ = job.reply_tx.send(reply);
            }
        });

        Ok(())
    }
}

fn accept_loop<'scope>(
    listener: TcpListener,
    scope: &'scope std::thread::Scope<'scope, '_>,
    shared: &Arc<HttpShared>,
    work_tx: mpsc::Sender<DispatchJob>,
) {
    loop {
        if shared.shutdown.load(Ordering::SeqCst) {
            break;
        }
        let stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(ACCEPT_POLL);
                continue;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        if shared.shutdown.load(Ordering::SeqCst) {
            drop(stream);
            break;
        }
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_nodelay(true);

        if shared.open_connections.fetch_add(1, Ordering::Relaxed) >= shared.max_connections {
            shared.open_connections.fetch_sub(1, Ordering::Relaxed);
            let mut stream = stream;
            let _ = write_json_response(&mut stream, 503, "Service Unavailable", None, &json!({"error": "too many concurrent connections"}), false);
            let _ = stream.shutdown(Shutdown::Both);
            continue;
        }

        let stream_id = shared.active_streams.insert(&stream);
        if shared.shutdown.load(Ordering::SeqCst) {
            shared.open_connections.fetch_sub(1, Ordering::Relaxed);
            shared.active_streams.remove(stream_id);
            let _ = stream.shutdown(Shutdown::Both);
            break;
        }

        let conn_shared = Arc::clone(shared);
        let conn_tx = work_tx.clone();
        scope.spawn(move || {
            let _guard = ConnectionGuard { shared: &conn_shared, stream_id };
            handle_connection(stream, &conn_shared, &conn_tx);
        });
    }
    drop(listener);
    shared.active_streams.close_all();
    drop(work_tx);
}

struct TimedStream {
    socket: TcpStream,
    shutdown: Arc<AtomicBool>,
    deadline: Option<Instant>,
    reading_request: bool,
}

impl TimedStream {
    fn new(socket: TcpStream, shutdown: Arc<AtomicBool>) -> Self {
        Self { socket, shutdown, deadline: None, reading_request: false }
    }

    fn begin_idle(&mut self) {
        self.reading_request = false;
        self.deadline = Some(Instant::now() + IDLE_TIMEOUT);
    }

    fn begin_request(&mut self) {
        self.reading_request = true;
        self.deadline = Some(Instant::now() + REQUEST_TIMEOUT);
    }
}

impl Read for TimedStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            if self.shutdown.load(Ordering::SeqCst) {
                return if self.reading_request { Err(io::Error::new(io::ErrorKind::ConnectionAborted, "server shutting down")) } else { Ok(0) };
            }
            let wait = match self.deadline {
                Some(deadline) => {
                    let left = deadline.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return Err(io::Error::new(io::ErrorKind::TimedOut, "request timed out"));
                    }
                    left.min(STOP_POLL)
                }
                None => STOP_POLL,
            };
            self.socket.set_read_timeout(Some(wait))?;
            match self.socket.read(buf) {
                Err(e) if matches!(e.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) => continue,
                other => return other,
            }
        }
    }
}

struct RequestHead {
    method: String,
    path: String,
    http_1_1: bool,
    headers: Vec<(String, String)>,
}

impl RequestHead {
    fn header_values<'a, 'b>(&'a self, name: &'b str) -> impl Iterator<Item = &'a str> + use<'a, 'b> {
        self.headers.iter().filter(move |(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    fn single_header(&self, name: &str) -> Result<Option<&str>, ()> {
        let mut iter = self.header_values(name);
        let Some(first) = iter.next() else {
            return Ok(None);
        };
        if iter.next().is_some() {
            return Err(());
        }
        Ok(Some(first))
    }

    fn wants_keep_alive(&self) -> bool {
        let mut has_close = false;
        let mut has_keep_alive = false;
        for val in self.header_values("connection") {
            for token in val.split(',').map(str::trim) {
                if token.eq_ignore_ascii_case("close") {
                    has_close = true;
                } else if token.eq_ignore_ascii_case("keep-alive") {
                    has_keep_alive = true;
                }
            }
        }
        if self.http_1_1 { !has_close } else { has_keep_alive && !has_close }
    }
}

enum HeadError {
    Eof,
    BadRequest(&'static str),
    HeadersTooLarge,
    UnsupportedVersion,
    Io,
}

enum BodyError {
    BadRequest(&'static str),
    PayloadTooLarge,
    UnsupportedTransferEncoding,
    Io,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ResponseFormat {
    Json,
    Sse,
}

fn read_line_bounded(reader: &mut impl BufRead, max_len: usize) -> Result<Option<Vec<u8>>, HeadError> {
    let mut line = Vec::new();
    let mut received = false;
    loop {
        let chunk = match reader.fill_buf() {
            Ok(c) => c,
            Err(e) if e.kind() == io::ErrorKind::TimedOut => return Err(if received { HeadError::Io } else { HeadError::Eof }),
            Err(_) => return Err(HeadError::Io),
        };
        if chunk.is_empty() {
            return if received { Err(HeadError::Io) } else { Ok(None) };
        }
        received = true;
        let (found, used) = match chunk.iter().position(|&b| b == b'\n') {
            Some(pos) => (true, pos + 1),
            None => (false, chunk.len()),
        };
        if line.len().saturating_add(used) > max_len {
            reader.consume(used);
            return Err(HeadError::HeadersTooLarge);
        }
        line.extend_from_slice(&chunk[..used]);
        reader.consume(used);
        if found {
            break;
        }
    }
    if line.last() == Some(&b'\n') {
        line.pop();
    }
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    Ok(Some(line))
}

fn read_request_head(reader: &mut BufReader<TimedStream>) -> Result<RequestHead, HeadError> {
    reader.get_mut().begin_idle();
    let Some(first_line) = read_line_bounded(reader, MAX_HEADER_LINE)? else {
        return Err(HeadError::Eof);
    };
    reader.get_mut().begin_request();
    let mut total_bytes = first_line.len();
    let request_line = std::str::from_utf8(&first_line).map_err(|_| HeadError::BadRequest("invalid UTF-8 in request line"))?;
    let mut parts = request_line.split(' ');
    let (Some(method), Some(target), Some(version), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return Err(HeadError::BadRequest("malformed HTTP request line"));
    };
    if method.is_empty() || !method.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(HeadError::BadRequest("invalid HTTP method"));
    }
    let http_1_1 = match version {
        "HTTP/1.1" => true,
        "HTTP/1.0" => false,
        _ => return Err(HeadError::UnsupportedVersion),
    };
    if !target.starts_with('/') {
        return Err(HeadError::BadRequest("request target must be origin-form"));
    }
    let path = target.split_once('?').map_or(target, |(p, _)| p).to_string();

    let mut headers = Vec::new();
    loop {
        let Some(raw) = read_line_bounded(reader, MAX_HEADER_LINE)? else {
            return Err(HeadError::BadRequest("unexpected EOF in HTTP headers"));
        };
        if raw.is_empty() {
            break;
        }
        total_bytes = total_bytes.saturating_add(raw.len());
        if total_bytes > MAX_HEADER_BYTES || headers.len() >= MAX_HEADERS {
            return Err(HeadError::HeadersTooLarge);
        }
        let line = std::str::from_utf8(&raw).map_err(|_| HeadError::BadRequest("invalid UTF-8 in HTTP header"))?;
        if line.starts_with([' ', '\t']) {
            return Err(HeadError::BadRequest("obsolete header folding is not allowed"));
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(HeadError::BadRequest("malformed HTTP header"));
        };
        if name.is_empty() || !name.bytes().all(is_header_token_byte) {
            return Err(HeadError::BadRequest("invalid HTTP header name"));
        }
        let value = value.trim_matches([' ', '\t']).to_string();
        headers.push((name.to_ascii_lowercase(), value));
    }

    Ok(RequestHead { method: method.to_string(), path, http_1_1, headers })
}

fn is_header_token_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~')
}

enum BodyKind {
    Empty,
    Length(usize),
    Chunked,
}

fn classify_body(head: &RequestHead, max_body: usize) -> Result<BodyKind, BodyError> {
    let te_values: Vec<&str> = head.header_values("transfer-encoding").collect();
    let cl_values: Vec<&str> = head.header_values("content-length").collect();
    if !te_values.is_empty() && !cl_values.is_empty() {
        return Err(BodyError::BadRequest("both Transfer-Encoding and Content-Length present"));
    }
    match te_values.as_slice() {
        [] => {}
        [single] if single.trim().eq_ignore_ascii_case("chunked") => return Ok(BodyKind::Chunked),
        _ => return Err(BodyError::UnsupportedTransferEncoding),
    }
    match cl_values.as_slice() {
        [] => Ok(BodyKind::Empty),
        [raw] => {
            if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
                return Err(BodyError::BadRequest("invalid Content-Length"));
            }
            let len = raw.parse::<usize>().map_err(|_| BodyError::PayloadTooLarge)?;
            if len > max_body {
                return Err(BodyError::PayloadTooLarge);
            }
            Ok(BodyKind::Length(len))
        }
        _ => Err(BodyError::BadRequest("duplicate Content-Length header")),
    }
}

fn read_body(reader: &mut BufReader<TimedStream>, kind: BodyKind, max_body: usize) -> Result<Vec<u8>, BodyError> {
    match kind {
        BodyKind::Empty => Ok(Vec::new()),
        BodyKind::Length(len) => {
            let mut body = Vec::with_capacity(len.min(64 * 1024));
            let mut limited = reader.by_ref().take(len as u64);
            limited.read_to_end(&mut body).map_err(|_| BodyError::Io)?;
            if body.len() != len {
                return Err(BodyError::Io);
            }
            Ok(body)
        }
        BodyKind::Chunked => read_chunked_body(reader, max_body),
    }
}

fn read_chunked_body(reader: &mut BufReader<TimedStream>, max_body: usize) -> Result<Vec<u8>, BodyError> {
    let mut body = Vec::new();
    for _ in 0..MAX_CHUNKS {
        let line = match read_line_bounded(reader, MAX_CHUNK_LINE) {
            Ok(Some(l)) => l,
            Ok(None) | Err(HeadError::Io | HeadError::Eof) => return Err(BodyError::Io),
            Err(_) => return Err(BodyError::BadRequest("invalid chunk size line")),
        };
        let line = std::str::from_utf8(&line).map_err(|_| BodyError::BadRequest("non-UTF-8 chunk size"))?;
        let size_hex = line.split_once(';').map_or(line, |(s, _)| s).trim();
        if size_hex.is_empty() || !size_hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(BodyError::BadRequest("invalid chunk size"));
        }
        let chunk_size = usize::from_str_radix(size_hex, 16).map_err(|_| BodyError::PayloadTooLarge)?;
        if chunk_size == 0 {
            for _ in 0..MAX_TRAILER_LINES {
                let trailer = match read_line_bounded(reader, MAX_HEADER_LINE) {
                    Ok(Some(l)) => l,
                    Ok(None) | Err(HeadError::Io | HeadError::Eof) => return Err(BodyError::Io),
                    Err(_) => return Err(BodyError::BadRequest("invalid chunk trailer")),
                };
                if trailer.is_empty() {
                    return Ok(body);
                }
            }
            return Err(BodyError::BadRequest("too many chunk trailers"));
        }
        if body.len().saturating_add(chunk_size) > max_body {
            return Err(BodyError::PayloadTooLarge);
        }
        let start = body.len();
        body.reserve(chunk_size.min(64 * 1024));
        let mut limited = reader.by_ref().take(chunk_size as u64);
        limited.read_to_end(&mut body).map_err(|_| BodyError::Io)?;
        if body.len().saturating_sub(start) != chunk_size {
            return Err(BodyError::Io);
        }
        let mut crlf = [0u8; 2];
        reader.read_exact(&mut crlf).map_err(|_| BodyError::Io)?;
        if crlf != *b"\r\n" {
            return Err(BodyError::BadRequest("chunk missing trailing CRLF"));
        }
    }
    Err(BodyError::PayloadTooLarge)
}

fn check_bearer_auth(head: &RequestHead, expected_token: &str) -> bool {
    let Ok(Some(value)) = head.single_header("authorization") else {
        return false;
    };
    let Some((scheme, rest)) = value.split_once([' ', '\t']) else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("bearer") {
        return false;
    }
    let token = rest.trim_matches([' ', '\t']);
    !token.is_empty() && tokens_match(token, expected_token)
}

fn is_json_content_type(value: &str) -> bool {
    value.split(';').next().is_some_and(|media| media.trim().eq_ignore_ascii_case("application/json"))
}

fn negotiate_post_format(accept: Option<&str>) -> Option<ResponseFormat> {
    let Some(raw) = accept.map(str::trim).filter(|s| !s.is_empty()) else {
        return Some(ResponseFormat::Json);
    };
    let mut accepts_json = false;
    let mut accepts_sse = false;
    for part in raw.split(',') {
        let mut segments = part.split(';');
        let Some(range) = segments.next().map(str::trim) else {
            continue;
        };
        let zero_q = segments.any(|param| {
            param.split_once('=').is_some_and(|(k, v)| k.trim().eq_ignore_ascii_case("q") && v.trim().parse::<f32>().is_ok_and(|q| q <= 0.0))
        });
        if zero_q {
            continue;
        }
        if range.eq_ignore_ascii_case("application/json") || range.eq_ignore_ascii_case("application/*") || range == "*/*" {
            accepts_json = true;
        }
        if range.eq_ignore_ascii_case("text/event-stream") || range.eq_ignore_ascii_case("text/*") || range == "*/*" {
            accepts_sse = true;
        }
    }
    if accepts_json {
        Some(ResponseFormat::Json)
    } else if accepts_sse {
        Some(ResponseFormat::Sse)
    } else {
        None
    }
}

fn is_initialize_request(v: &Value) -> bool {
    v.as_object().is_some_and(|o| o.get("method").and_then(Value::as_str) == Some("initialize") && o.get("id").is_some_and(|id| !id.is_null()))
}

fn handle_connection(stream: TcpStream, shared: &HttpShared, work_tx: &mpsc::Sender<DispatchJob>) {
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(TimedStream::new(stream, Arc::clone(&shared.shutdown)));

    loop {
        if shared.shutdown.load(Ordering::SeqCst) {
            break;
        }
        let head = match read_request_head(&mut reader) {
            Ok(head) => head,
            Err(HeadError::Eof | HeadError::Io) => break,
            Err(HeadError::BadRequest(msg)) => {
                let _ = write_json_response(&mut writer, 400, "Bad Request", None, &json!({"error": msg}), false);
                break;
            }
            Err(HeadError::HeadersTooLarge) => {
                let _ = write_json_response(&mut writer, 431, "Request Header Fields Too Large", None, &json!({"error": "headers too large"}), false);
                break;
            }
            Err(HeadError::UnsupportedVersion) => {
                let _ = write_json_response(&mut writer, 505, "HTTP Version Not Supported", None, &json!({"error": "HTTP/1.1 required"}), false);
                break;
            }
        };

        // 1. DNS rebinding protection: Host header must be loopback (and present in HTTP/1.1).
        match head.single_header("host") {
            Ok(Some(host)) if is_loopback_host_port(host) => {}
            Ok(None) if !head.http_1_1 => {}
            Ok(None) => {
                let _ = write_json_response(&mut writer, 400, "Bad Request", None, &json!({"error": "missing Host header"}), false);
                break;
            }
            _ => {
                let _ = write_json_response(&mut writer, 403, "Forbidden", None, &json!({"error": "forbidden Host header"}), false);
                break;
            }
        }

        // 2. DNS rebinding protection: Origin header, if present, must be loopback.
        match head.single_header("origin") {
            Ok(None) => {}
            Ok(Some(origin)) if is_loopback_origin(origin) => {}
            _ => {
                let _ = write_json_response(&mut writer, 403, "Forbidden", None, &json!({"error": "forbidden Origin header"}), false);
                break;
            }
        }

        // 3. Mandatory Bearer token authentication (constant-time).
        if !check_bearer_auth(&head, &shared.token) {
            let _ = write_unauthorized(&mut writer);
            break;
        }

        // 4. Endpoint path check.
        if head.path != MCP_PATH {
            let _ = write_json_response(&mut writer, 404, "Not Found", None, &json!({"error": "not found"}), false);
            break;
        }

        let keep_alive = head.wants_keep_alive() && !shared.shutdown.load(Ordering::SeqCst);

        match head.method.as_str() {
            "GET" => {
                // Validate session ID if one was sent; otherwise return 405 per MCP Streamable
                // HTTP spec (this server does not push unsolicited server-initiated SSE events).
                match head.single_header("mcp-session-id") {
                    Ok(None) => {}
                    Ok(Some(sid)) => {
                        if !shared.sessions().touch(sid) {
                            let _ = write_json_response(&mut writer, 404, "Not Found", None, &json!({"error": "session not found"}), keep_alive);
                            if !keep_alive {
                                break;
                            }
                            continue;
                        }
                    }
                    Err(()) => {
                        let _ =
                            write_json_response(&mut writer, 400, "Bad Request", None, &json!({"error": "duplicate Mcp-Session-Id header"}), false);
                        break;
                    }
                }
                if write_method_not_allowed(&mut writer, "POST, DELETE", keep_alive).is_err() || !keep_alive {
                    break;
                }
            }
            "DELETE" => {
                let sid = match head.single_header("mcp-session-id") {
                    Ok(Some(sid)) if !sid.is_empty() => sid,
                    Ok(_) => {
                        let _ = write_json_response(&mut writer, 400, "Bad Request", None, &json!({"error": "missing Mcp-Session-Id header"}), false);
                        break;
                    }
                    Err(()) => {
                        let _ =
                            write_json_response(&mut writer, 400, "Bad Request", None, &json!({"error": "duplicate Mcp-Session-Id header"}), false);
                        break;
                    }
                };
                let removed = shared.sessions().remove(sid);
                let sent = if removed {
                    write_no_content(&mut writer, keep_alive)
                } else {
                    write_json_response(&mut writer, 404, "Not Found", None, &json!({"error": "session not found"}), keep_alive)
                };
                if sent.is_err() || !keep_alive {
                    break;
                }
            }
            "POST" => {
                if !handle_post(&head, &mut reader, &mut writer, shared, work_tx, keep_alive) || !keep_alive {
                    break;
                }
            }
            _ => {
                let _ = write_method_not_allowed(&mut writer, "POST, DELETE", false);
                break;
            }
        }
    }
}

/// Handle one `POST /mcp` request. Returns `true` if the connection stream remains clean and may
/// be reused for HTTP keep-alive.
fn handle_post(
    head: &RequestHead,
    reader: &mut BufReader<TimedStream>,
    writer: &mut TcpStream,
    shared: &HttpShared,
    work_tx: &mpsc::Sender<DispatchJob>,
    keep_alive: bool,
) -> bool {
    if let Ok(Some(ct)) = head.single_header("content-type")
        && !is_json_content_type(ct)
    {
        let _ = write_json_response(writer, 415, "Unsupported Media Type", None, &json!({"error": "Content-Type must be application/json"}), false);
        return false;
    }

    let Some(format) = negotiate_post_format(head.single_header("accept").ok().flatten()) else {
        let _ = write_json_response(writer, 406, "Not Acceptable", None, &json!({"error": "Accept must allow application/json"}), false);
        return false;
    };

    let req_session_id = match head.single_header("mcp-session-id") {
        Ok(sid) => sid.map(str::to_owned),
        Err(()) => {
            let _ = write_json_response(writer, 400, "Bad Request", None, &json!({"error": "duplicate Mcp-Session-Id header"}), false);
            return false;
        }
    };

    if let Some(sid) = req_session_id.as_deref()
        && !shared.sessions().touch(sid)
    {
        let _ = write_json_response(writer, 404, "Not Found", None, &json!({"error": "session not found"}), false);
        return false;
    }

    let body_kind = match classify_body(head, shared.max_body) {
        Ok(kind) => kind,
        Err(BodyError::BadRequest(msg)) => {
            let _ = write_json_response(writer, 400, "Bad Request", None, &json!({"error": msg}), false);
            return false;
        }
        Err(BodyError::PayloadTooLarge) => {
            let _ = write_json_response(writer, 413, "Payload Too Large", None, &json!({"error": "request body too large"}), false);
            return false;
        }
        Err(BodyError::UnsupportedTransferEncoding) => {
            let _ = write_json_response(writer, 501, "Not Implemented", None, &json!({"error": "unsupported Transfer-Encoding"}), false);
            return false;
        }
        Err(BodyError::Io) => return false,
    };

    if head.header_values("expect").any(|v| v.eq_ignore_ascii_case("100-continue"))
        && (writer.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").is_err() || writer.flush().is_err())
    {
        return false;
    }

    let body = match read_body(reader, body_kind, shared.max_body) {
        Ok(body) => body,
        Err(BodyError::BadRequest(msg)) => {
            let _ = write_json_response(writer, 400, "Bad Request", None, &json!({"error": msg}), false);
            return false;
        }
        Err(BodyError::PayloadTooLarge) => {
            let _ = write_json_response(writer, 413, "Payload Too Large", None, &json!({"error": "request body too large"}), false);
            return false;
        }
        Err(BodyError::UnsupportedTransferEncoding) => {
            let _ = write_json_response(writer, 501, "Not Implemented", None, &json!({"error": "unsupported Transfer-Encoding"}), false);
            return false;
        }
        Err(BodyError::Io) => return false,
    };

    let text = match std::str::from_utf8(&body) {
        Ok(t) => t,
        Err(_) => {
            let err = rpc_error(Value::Null, PARSE_ERROR, "parse error: input is not UTF-8");
            return write_json_response(writer, 400, "Bad Request", req_session_id.as_deref(), &err, keep_alive).is_ok();
        }
    };
    let parsed: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            let err = rpc_error(Value::Null, PARSE_ERROR, format!("parse error: {e}"));
            return write_json_response(writer, 400, "Bad Request", req_session_id.as_deref(), &err, keep_alive).is_ok();
        }
    };

    let initializing = is_initialize_request(&parsed);
    if initializing {
        if req_session_id.is_some() {
            return write_json_response(
                writer,
                400,
                "Bad Request",
                None,
                &json!({"error": "initialize must not include Mcp-Session-Id"}),
                keep_alive,
            )
            .is_ok();
        }
    } else if req_session_id.is_none() {
        return write_json_response(writer, 400, "Bad Request", None, &json!({"error": "missing Mcp-Session-Id header"}), keep_alive).is_ok();
    }

    let (reply_tx, reply_rx) = mpsc::channel();
    if work_tx.send(DispatchJob { value: parsed, reply_tx }).is_err() {
        let _ = write_json_response(writer, 503, "Service Unavailable", None, &json!({"error": "server shutting down"}), false);
        return false;
    }
    let Ok(reply) = reply_rx.recv() else {
        let _ = write_json_response(writer, 503, "Service Unavailable", None, &json!({"error": "server shutting down"}), false);
        return false;
    };

    let response_session_id = if initializing {
        if reply.as_ref().is_some_and(|r| r.get("result").is_some()) {
            let Some(sid) = shared.sessions().create() else {
                return write_json_response(writer, 503, "Service Unavailable", None, &json!({"error": "too many active MCP sessions"}), keep_alive)
                    .is_ok();
            };
            Some(sid)
        } else {
            None
        }
    } else {
        req_session_id
    };

    match reply {
        Some(reply_value) => match format {
            ResponseFormat::Json => write_json_response(writer, 200, "OK", response_session_id.as_deref(), &reply_value, keep_alive).is_ok(),
            ResponseFormat::Sse => write_sse_response(writer, response_session_id.as_deref(), &reply_value, keep_alive).is_ok(),
        },
        None => write_accepted(writer, response_session_id.as_deref(), keep_alive).is_ok(),
    }
}

fn connection_header(keep_alive: bool) -> &'static str {
    if keep_alive { "keep-alive" } else { "close" }
}

fn write_json_response(
    writer: &mut TcpStream,
    status: u16,
    reason: &str,
    session_id: Option<&str>,
    body: &Value,
    keep_alive: bool,
) -> io::Result<()> {
    let payload = serde_json::to_vec(body).map_err(io::Error::other)?;
    let mut buf = Vec::with_capacity(256 + payload.len());
    let _ = write!(
        buf,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: {}\r\n",
        payload.len(),
        connection_header(keep_alive),
    );
    if let Some(sid) = session_id {
        let _ = write!(buf, "Mcp-Session-Id: {sid}\r\n");
    }
    buf.extend_from_slice(b"\r\n");
    buf.extend_from_slice(&payload);
    writer.write_all(&buf)?;
    writer.flush()
}

fn write_sse_response(writer: &mut TcpStream, session_id: Option<&str>, body: &Value, keep_alive: bool) -> io::Result<()> {
    let json_text = body.to_string();
    let payload = format!("event: message\ndata: {json_text}\n\n");
    let mut buf = Vec::with_capacity(256 + payload.len());
    let _ = write!(
        buf,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: {}\r\n",
        payload.len(),
        connection_header(keep_alive),
    );
    if let Some(sid) = session_id {
        let _ = write!(buf, "Mcp-Session-Id: {sid}\r\n");
    }
    buf.extend_from_slice(b"\r\n");
    buf.extend_from_slice(payload.as_bytes());
    writer.write_all(&buf)?;
    writer.flush()
}

fn write_accepted(writer: &mut TcpStream, session_id: Option<&str>, keep_alive: bool) -> io::Result<()> {
    let mut buf = Vec::with_capacity(256);
    let _ =
        write!(buf, "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\nCache-Control: no-store\r\nConnection: {}\r\n", connection_header(keep_alive),);
    if let Some(sid) = session_id {
        let _ = write!(buf, "Mcp-Session-Id: {sid}\r\n");
    }
    buf.extend_from_slice(b"\r\n");
    writer.write_all(&buf)?;
    writer.flush()
}

fn write_no_content(writer: &mut TcpStream, keep_alive: bool) -> io::Result<()> {
    let response = format!("HTTP/1.1 204 No Content\r\nCache-Control: no-store\r\nConnection: {}\r\n\r\n", connection_header(keep_alive));
    writer.write_all(response.as_bytes())?;
    writer.flush()
}

fn write_unauthorized(writer: &mut TcpStream) -> io::Result<()> {
    let payload = br#"{"error":"unauthorized"}"#;
    let mut buf = Vec::with_capacity(256 + payload.len());
    let _ = write!(
        buf,
        "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Bearer realm=\"solidcraft-mcp\"\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        payload.len(),
    );
    buf.extend_from_slice(payload);
    writer.write_all(&buf)?;
    writer.flush()
}

fn write_method_not_allowed(writer: &mut TcpStream, allow: &str, keep_alive: bool) -> io::Result<()> {
    let payload = br#"{"error":"method not allowed"}"#;
    let mut buf = Vec::with_capacity(256 + payload.len());
    let _ = write!(
        buf,
        "HTTP/1.1 405 Method Not Allowed\r\nAllow: {allow}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: {}\r\n\r\n",
        payload.len(),
        connection_header(keep_alive),
    );
    buf.extend_from_slice(payload);
    writer.write_all(&buf)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Effect, Info, object, string, text_result, tool};

    #[derive(Default)]
    struct Counter {
        count: usize,
        items: Vec<String>,
    }

    impl Tools for Counter {
        fn instructions(&self) -> String {
            "Counter test tools.".into()
        }
        fn list(&mut self) -> Vec<Value> {
            vec![
                tool("add", "Add", "Add an item.", object(json!({"item": string("Item")}), &["item"]), Effect::Edits),
                tool("count", "Count", "Count items.", object(json!({}), &[]), Effect::ReadOnly),
            ]
        }
        fn call(&mut self, name: &str, args: &Value) -> Value {
            match name {
                "add" => {
                    let item = args["item"].as_str().unwrap_or_default().to_string();
                    self.count += 1;
                    self.items.push(item);
                    text_result(json!({"count": self.count}).to_string(), false)
                }
                "count" => text_result(json!({"count": self.count, "items": self.items}).to_string(), false),
                _ => text_result("unknown tool", true),
            }
        }
    }

    struct RawResponse {
        status: u16,
        headers: Vec<(String, String)>,
        body: String,
    }

    impl RawResponse {
        fn header(&self, name: &str) -> Option<&str> {
            self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
        }

        fn json(&self) -> Value {
            serde_json::from_str(&self.body).unwrap_or_else(|e| panic!("not JSON ({e}): {:?}", self.body))
        }
    }

    fn send_raw(addr: SocketAddr, request: &[u8]) -> RawResponse {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        stream.write_all(request).unwrap();
        stream.flush().unwrap();

        let mut reader = BufReader::new(stream);
        let mut status_line = String::new();
        reader.read_line(&mut status_line).unwrap();
        let status = status_line.split_whitespace().nth(1).unwrap().parse::<u16>().unwrap();
        let mut headers = Vec::new();
        let mut content_length = 0usize;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if trimmed.is_empty() {
                break;
            }
            let (k, v) = trimmed.split_once(':').unwrap();
            let v = v.trim().to_string();
            if k.eq_ignore_ascii_case("content-length") {
                content_length = v.parse().unwrap();
            }
            headers.push((k.to_ascii_lowercase(), v));
        }
        let mut body = vec![0u8; content_length];
        if content_length > 0 {
            reader.read_exact(&mut body).unwrap();
        }
        RawResponse { status, headers, body: String::from_utf8(body).unwrap() }
    }

    fn post_json(addr: SocketAddr, token: Option<&str>, session_id: Option<&str>, body: &Value) -> RawResponse {
        let payload = body.to_string();
        let mut req = format!(
            "POST /mcp HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n",
            payload.len()
        );
        if let Some(t) = token {
            req.push_str(&format!("Authorization: Bearer {t}\r\n"));
        }
        if let Some(sid) = session_id {
            req.push_str(&format!("Mcp-Session-Id: {sid}\r\n"));
        }
        req.push_str("\r\n");
        req.push_str(&payload);
        send_raw(addr, req.as_bytes())
    }

    fn start_test_server(token: &str) -> (SocketAddr, HttpStop, std::thread::JoinHandle<io::Result<()>>) {
        start_test_server_with_config(HttpConfig::new(token))
    }

    fn start_test_server_with_config(config: HttpConfig) -> (SocketAddr, HttpStop, std::thread::JoinHandle<io::Result<()>>) {
        let listener = bind_loopback(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
        let addr = listener.local_addr().unwrap();
        let stop = HttpStop::new(addr, Arc::clone(&config.shutdown));
        let handle = std::thread::spawn(move || {
            let info = Info { name: "counter", title: "Counter", version: "1.0.0", max_line: 1 << 16 };
            let mut server = Server::new(info, Counter::default());
            server.serve_http_listener(listener, config)
        });
        (addr, stop, handle)
    }

    #[test]
    fn non_loopback_addresses_are_rejected() {
        for bad in ["0.0.0.0:8765", "[::]:8765", "192.168.1.10:8765", "10.0.0.1:8765", "example.com:8765", "not-an-addr"] {
            assert!(parse_loopback_addr(bad).is_err(), "{bad} should be rejected");
        }
        for good in ["127.0.0.1:8765", "127.0.0.1:0", "[::1]:8765", "[::1]:0", "localhost:8765"] {
            let addr = parse_loopback_addr(good).unwrap_or_else(|e| panic!("{good}: {e}"));
            assert!(addr.ip().is_loopback(), "{good} -> {addr}");
        }
        let non_loopback = SocketAddr::from(([0, 0, 0, 0], 0));
        let err = bind_loopback(non_loopback).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn origin_and_host_rebinding_checks_reject_non_loopback() {
        for ok in ["http://127.0.0.1", "http://127.0.0.1:8765", "https://localhost:3000", "http://localhost", "http://[::1]", "http://[::1]:8765"] {
            assert!(is_loopback_origin(ok), "{ok} should be allowed");
        }
        for bad in [
            "null",
            "http://evil.com",
            "https://127.0.0.1.evil.com",
            "http://localhost.evil.com",
            "http://localhost@evil.com",
            "http://127.0.0.1:8765/path",
            "http://0.0.0.0:8765",
            "http://[::]:8765",
            "http://127.0.0.1:99999",
            "javascript:alert(1)",
        ] {
            assert!(!is_loopback_origin(bad), "{bad} should be rejected");
        }

        let token = "test-token-0123456789abcdef";
        let (addr, stop, handle) = start_test_server(token);

        let bad_origin = format!(
            "POST /mcp HTTP/1.1\r\nHost: {addr}\r\nOrigin: https://attacker.example\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
        );
        assert_eq!(send_raw(addr, bad_origin.as_bytes()).status, 403);

        let bad_host = format!(
            "POST /mcp HTTP/1.1\r\nHost: attacker.example:8765\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
        );
        assert_eq!(send_raw(addr, bad_host.as_bytes()).status, 403);

        stop.stop();
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn missing_or_wrong_bearer_token_returns_401() {
        let token = "secret-bearer-token-0123456789abcdef";
        let (addr, stop, handle) = start_test_server(token);
        let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}});

        let missing = post_json(addr, None, None, &init);
        assert_eq!(missing.status, 401);
        assert!(missing.header("www-authenticate").is_some_and(|v| v.starts_with("Bearer")));
        assert!(!missing.body.contains(token));

        let wrong = post_json(addr, Some("wrong-token"), None, &init);
        assert_eq!(wrong.status, 401);
        assert!(!wrong.body.contains(token));

        let dup = format!(
            "POST /mcp HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nAuthorization: Bearer {token}\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
        );
        assert_eq!(send_raw(addr, dup.as_bytes()).status, 401);

        stop.stop();
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn streamable_http_session_lifecycle_and_delete() {
        let token = "lifecycle-token-0123456789abcdef";
        let (addr, stop, handle) = start_test_server_with_config(HttpConfig::new(token).with_max_body(4096));

        // Non-initialize without session ID -> 400.
        let ping = json!({"jsonrpc": "2.0", "id": 1, "method": "ping"});
        assert_eq!(post_json(addr, Some(token), None, &ping).status, 400);

        // Initialize creates a session ID.
        let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}});
        let r = post_json(addr, Some(token), None, &init);
        assert_eq!(r.status, 200);
        let sid = r.header("mcp-session-id").unwrap().to_string();
        assert!(!sid.is_empty());
        assert_eq!(r.json()["result"]["serverInfo"]["name"], "counter");

        // Notification returns 202 Accepted with empty body.
        let notif = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        let r = post_json(addr, Some(token), Some(&sid), &notif);
        assert_eq!(r.status, 202);
        assert!(r.body.is_empty());

        // GET /mcp returns 405 Method Not Allowed with Allow header.
        let get_req =
            format!("GET /mcp HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nMcp-Session-Id: {sid}\r\nConnection: close\r\n\r\n");
        let get_resp = send_raw(addr, get_req.as_bytes());
        assert_eq!(get_resp.status, 405);
        assert_eq!(get_resp.header("allow"), Some("POST, DELETE"));

        // Chunked POST works cleanly.
        let call = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "add", "arguments": {"item": "chunked"}}});
        let payload = call.to_string();
        let chunked = format!(
            "POST /mcp HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nMcp-Session-Id: {sid}\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{payload}\r\n0\r\n\r\n",
            payload.len()
        );
        let chunked_resp = send_raw(addr, chunked.as_bytes());
        assert_eq!(chunked_resp.status, 200);
        assert_eq!(chunked_resp.json()["result"]["content"][0]["text"], r#"{"count":1}"#);

        // Oversized body returns 413 Payload Too Large.
        let too_big = format!(
            "POST /mcp HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nMcp-Session-Id: {sid}\r\nContent-Type: application/json\r\nContent-Length: 5000\r\nConnection: close\r\n\r\n"
        );
        assert_eq!(send_raw(addr, too_big.as_bytes()).status, 413);

        // DELETE closes only that session.
        let del =
            format!("DELETE /mcp HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nMcp-Session-Id: {sid}\r\nConnection: close\r\n\r\n");
        assert_eq!(send_raw(addr, del.as_bytes()).status, 204);
        assert_eq!(post_json(addr, Some(token), Some(&sid), &ping).status, 404);

        stop.stop();
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn concurrent_http_clients_serialize_cleanly() {
        let token = "concurrent-token-0123456789abcdef";
        let (addr, stop, handle) = start_test_server(token);

        let threads: Vec<_> = (0..6)
            .map(|t| {
                let token = token.to_string();
                std::thread::spawn(move || {
                    let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}});
                    let r = post_json(addr, Some(&token), None, &init);
                    assert_eq!(r.status, 200);
                    let sid = r.header("mcp-session-id").unwrap().to_string();
                    for i in 0..10 {
                        let req = json!({
                            "jsonrpc": "2.0",
                            "id": i + 2,
                            "method": "tools/call",
                            "params": {"name": "add", "arguments": {"item": format!("t{t}-i{i}")}}
                        });
                        let resp = post_json(addr, Some(&token), Some(&sid), &req);
                        assert_eq!(resp.status, 200);
                        assert!(resp.json().get("result").is_some());
                    }
                    let del = format!(
                        "DELETE /mcp HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nMcp-Session-Id: {sid}\r\nConnection: close\r\n\r\n"
                    );
                    assert_eq!(send_raw(addr, del.as_bytes()).status, 204);
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }

        // Verify all 60 serialized updates landed with zero loss or corruption.
        let init = post_json(
            addr,
            Some(token),
            None,
            &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}}),
        );
        let sid = init.header("mcp-session-id").unwrap().to_string();
        let count_resp = post_json(
            addr,
            Some(token),
            Some(&sid),
            &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "count", "arguments": {}}}),
        );
        assert_eq!(count_resp.status, 200);
        let text = count_resp.json()["result"]["content"][0]["text"].as_str().unwrap().to_string();
        let state: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(state["count"], 60);
        assert_eq!(state["items"].as_array().unwrap().len(), 60);

        stop.stop();
        handle.join().unwrap().unwrap();
    }
}
