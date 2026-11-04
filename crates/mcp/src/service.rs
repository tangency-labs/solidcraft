//! Background MCP service and transport controller.
//!
//! Provides an embedded Model Context Protocol (MCP) server that runs directly inside
//! SolidCraft without requiring an external terminal or solidcraft-cli invocation.
//! Supports SSE / HTTP (Streamable HTTP & SSE) and direct transports with live status,
//! telemetry logging, and dynamic AI client configuration.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use solidcraft_engine::MAX_LINE;

use crate::agent_scope;
use crate::backend::{Backend, Remote};
use crate::server::Server;

/// Transport mode for the embedded MCP server.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum McpTransportMode {
    #[default]
    SseHttp,
    Stdio,
    WebSocket,
}

impl McpTransportMode {
    pub const ALL: [Self; 3] = [Self::SseHttp, Self::Stdio, Self::WebSocket];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::SseHttp => "sse",
            Self::Stdio => "stdio",
            Self::WebSocket => "websocket",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::SseHttp => "SSE / HTTP (Local Port)",
            Self::Stdio => "Stdio (Subprocess)",
            Self::WebSocket => "WebSocket",
        }
    }

    pub fn from_str_loose(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "stdio" => Self::Stdio,
            "websocket" | "ws" => Self::WebSocket,
            _ => Self::SseHttp,
        }
    }
}

/// Status of the embedded MCP server.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum McpStatus {
    Stopped,
    Starting,
    Running { host: String, port: u16, transport: McpTransportMode, endpoint_url: String },
    Error(String),
}

impl Default for McpStatus {
    fn default() -> Self {
        Self::Stopped
    }
}

impl McpStatus {
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running { .. })
    }

    pub fn badge_text(&self) -> &'static str {
        match self {
            Self::Stopped => "Stopped",
            Self::Starting => "Starting...",
            Self::Running { .. } => "Running",
            Self::Error(_) => "Error",
        }
    }

    pub fn summary_text(&self) -> String {
        match self {
            Self::Stopped => "MCP Server stopped".to_string(),
            Self::Starting => "MCP Server starting...".to_string(),
            Self::Running { host, port, .. } => format!("Running & listening on http://{host}:{port}/sse"),
            Self::Error(err) => format!("MCP Server error: {err}"),
        }
    }
}

/// Log severity levels for telemetry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum McpLogLevel {
    Info,
    Request,
    Response,
    Error,
}

impl McpLogLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Request => "REQ",
            Self::Response => "RES",
            Self::Error => "ERR",
        }
    }
}

/// A single telemetry log line recorded by the MCP server.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpLogEntry {
    pub timestamp: String,
    pub level: McpLogLevel,
    pub message: String,
}

/// Configuration options passed to start the server.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub host: String,
    pub port: u16,
    pub transport: McpTransportMode,
    pub workspace_path: Option<PathBuf>,
    pub control_port: Option<u16>,
    pub control_token: Option<String>,
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 8989,
            transport: McpTransportMode::SseHttp,
            workspace_path: None,
            control_port: None,
            control_token: None,
        }
    }
}

const MAX_LOG_ENTRIES: usize = 500;

fn current_timestamp() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    format!("{h:02}:{m:02}:{s:02}")
}

struct Inner {
    status: RwLock<McpStatus>,
    logs: Mutex<VecDeque<McpLogEntry>>,
    shutdown_flag: AtomicBool,
    session_counter: AtomicU64,
    sessions: Mutex<HashMap<String, Sender<String>>>,
    active_config: RwLock<Option<McpServerConfig>>,
}

/// Thread-safe controller for the embedded MCP background server.
#[derive(Clone)]
pub struct McpServerController {
    inner: Arc<Inner>,
}

impl Default for McpServerController {
    fn default() -> Self {
        Self::new()
    }
}

impl McpServerController {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                status: RwLock::new(McpStatus::Stopped),
                logs: Mutex::new(VecDeque::with_capacity(MAX_LOG_ENTRIES)),
                shutdown_flag: AtomicBool::new(false),
                session_counter: AtomicU64::new(1),
                sessions: Mutex::new(HashMap::new()),
                active_config: RwLock::new(None),
            }),
        }
    }

    pub fn status(&self) -> McpStatus {
        self.inner.status.read().map(|s| s.clone()).unwrap_or(McpStatus::Stopped)
    }

    pub fn is_running(&self) -> bool {
        self.status().is_running()
    }

    pub fn logs(&self) -> Vec<McpLogEntry> {
        self.inner.logs.lock().map(|l| l.iter().cloned().collect()).unwrap_or_default()
    }

    pub fn clear_logs(&self) {
        if let Ok(mut l) = self.inner.logs.lock() {
            l.clear();
        }
    }

    pub fn active_config(&self) -> Option<McpServerConfig> {
        self.inner.active_config.read().ok().and_then(|c| c.clone())
    }

    pub fn log(&self, level: McpLogLevel, message: impl Into<String>) {
        let entry = McpLogEntry { timestamp: current_timestamp(), level, message: message.into() };
        if let Ok(mut l) = self.inner.logs.lock() {
            if l.len() >= MAX_LOG_ENTRIES {
                l.pop_front();
            }
            l.push_back(entry);
        }
    }

    /// Stops the embedded server and frees network ports immediately.
    pub fn stop(&self) {
        self.inner.shutdown_flag.store(true, Ordering::SeqCst);
        if let Ok(mut sessions) = self.inner.sessions.lock() {
            sessions.clear();
        }
        if let Ok(mut s) = self.inner.status.write() {
            if s.is_running() {
                *s = McpStatus::Stopped;
                self.log(McpLogLevel::Info, "Embedded MCP server stopped. Port freed.");
            }
        }
        if let Ok(mut c) = self.inner.active_config.write() {
            *c = None;
        }
    }

    /// Spawns the MCP server asynchronously on a background thread.
    pub fn start(&self, config: McpServerConfig) -> Result<(), String> {
        // Stop any running server first
        if self.is_running() {
            self.stop();
            std::thread::sleep(Duration::from_millis(100));
        }

        self.inner.shutdown_flag.store(false, Ordering::SeqCst);
        if let Ok(mut s) = self.inner.status.write() {
            *s = McpStatus::Starting;
        }
        self.log(McpLogLevel::Info, format!("Starting MCP server on {}:{} ({:?})...", config.host, config.port, config.transport));

        let bind_addr = format!("{}:{}", config.host, config.port);
        let listener = match TcpListener::bind(&bind_addr) {
            Ok(l) => l,
            Err(e) => {
                let err_msg = format!("Failed to bind to {bind_addr}: {e}");
                if let Ok(mut s) = self.inner.status.write() {
                    *s = McpStatus::Error(err_msg.clone());
                }
                self.log(McpLogLevel::Error, &err_msg);
                return Err(err_msg);
            }
        };

        let local_port = listener.local_addr().map(|a| a.port()).unwrap_or(config.port);
        let endpoint_url = format!("http://{}:{local_port}/sse", config.host);

        let running_status =
            McpStatus::Running { host: config.host.clone(), port: local_port, transport: config.transport, endpoint_url: endpoint_url.clone() };

        if let Ok(mut s) = self.inner.status.write() {
            *s = running_status;
        }
        if let Ok(mut c) = self.inner.active_config.write() {
            *c = Some(config.clone());
        }

        self.log(McpLogLevel::Info, format!("MCP server listening at {endpoint_url}"));

        let controller = self.clone();
        std::thread::Builder::new()
            .name("solidcraft-mcp-listener".into())
            .spawn(move || {
                run_listener(listener, controller, config);
            })
            .map_err(|e| format!("Failed to spawn listener thread: {e}"))?;

        Ok(())
    }
}

fn run_listener(listener: TcpListener, controller: McpServerController, config: McpServerConfig) {
    let _ = listener.set_nonblocking(true);

    while !controller.inner.shutdown_flag.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, peer_addr)) => {
                let ctrl = controller.clone();
                let cfg = config.clone();
                std::thread::Builder::new()
                    .name(format!("mcp-client-{peer_addr}"))
                    .spawn(move || {
                        handle_connection(stream, ctrl, cfg);
                    })
                    .ok();
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                if !controller.inner.shutdown_flag.load(Ordering::Relaxed) {
                    controller.log(McpLogLevel::Error, format!("Accept error: {e}"));
                }
                break;
            }
        }
    }

    controller.log(McpLogLevel::Info, "Listener loop terminated cleanly.");
}

fn handle_connection(stream: TcpStream, controller: McpServerController, config: McpServerConfig) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(300)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));

    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });

    let mut first_line = String::new();
    if reader.read_line(&mut first_line).is_err() || first_line.trim().is_empty() {
        return;
    }

    let trimmed = first_line.trim();

    // Check if it's an HTTP request
    if trimmed.starts_with("GET ") || trimmed.starts_with("POST ") || trimmed.starts_with("OPTIONS ") {
        handle_http_request(first_line, reader, stream, controller, config);
    } else {
        // Raw JSON-RPC over TCP line
        handle_raw_jsonrpc(first_line, reader, stream, controller, config);
    }
}

fn create_backend(config: &McpServerConfig) -> Box<dyn Backend> {
    let addr = config.control_port.map(|p| format!("127.0.0.1:{p}")).unwrap_or_else(|| {
        let p = solidcraft_engine::control_auth::read_current_port().unwrap_or(solidcraft_engine::control_auth::DEFAULT_PORT);
        format!("127.0.0.1:{p}")
    });
    Box::new(Remote::new(&addr, config.control_token.as_deref()))
}

fn handle_raw_jsonrpc(
    first_line: String,
    mut reader: BufReader<TcpStream>,
    mut stream: TcpStream,
    controller: McpServerController,
    config: McpServerConfig,
) {
    let backend = create_backend(&config);
    let scope = agent_scope(config.workspace_path.into_iter());
    let mut server = Server::new(backend).with_path_scope(Some(scope));

    let mut current_line = Some(first_line);
    loop {
        let line = match current_line.take() {
            Some(l) => l,
            None => {
                let mut next = String::new();
                match reader.read_line(&mut next) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => next,
                }
            }
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        controller.log(McpLogLevel::Request, trimmed);
        if let Some(reply) = server.handle_line(trimmed) {
            controller.log(McpLogLevel::Response, &reply);
            if stream.write_all(reply.as_bytes()).is_err() || stream.write_all(b"\n").is_err() || stream.flush().is_err() {
                break;
            }
        }
    }
}

fn handle_http_request(
    request_line: String,
    mut reader: BufReader<TcpStream>,
    mut stream: TcpStream,
    controller: McpServerController,
    config: McpServerConfig,
) {
    let mut headers = HashMap::new();
    let mut header_bytes = 0usize;

    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                header_bytes = header_bytes.saturating_add(n);
                if header_bytes > 64 * 1024 {
                    send_http_response(&mut stream, 431, "Request Header Fields Too Large", "text/plain", b"Header too large");
                    return;
                }
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    break;
                }
                if let Some((k, v)) = trimmed.split_once(':') {
                    headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
                }
            }
        }
    }

    let parts: Vec<&str> = request_line.trim().split_whitespace().collect();
    if parts.len() < 2 {
        send_http_response(&mut stream, 400, "Bad Request", "text/plain", b"Malformed request line");
        return;
    }

    let method = parts[0];
    let raw_path = parts[1];
    let path = raw_path.split('?').next().unwrap_or(raw_path);

    // CORS preflight
    if method == "OPTIONS" {
        send_cors_response(&mut stream);
        return;
    }

    // SSE stream endpoint
    if method == "GET" && (path == "/sse" || path == "/") {
        handle_sse_stream(stream, controller);
        return;
    }

    // Message receiver (POST /message?sessionId=... or POST /mcp or POST /)
    if method == "POST" {
        let content_length = headers.get("content-length").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
        if content_length > MAX_LINE {
            send_http_response(&mut stream, 413, "Payload Too Large", "text/plain", b"Payload exceeds maximum allowed limit");
            return;
        }

        let mut body = vec![0u8; content_length];
        if reader.read_exact(&mut body).is_err() {
            send_http_response(&mut stream, 400, "Bad Request", "text/plain", b"Could not read request body");
            return;
        }

        let body_str = match String::from_utf8(body) {
            Ok(s) => s,
            Err(_) => {
                send_http_response(&mut stream, 400, "Bad Request", "text/plain", b"Body is not valid UTF-8");
                return;
            }
        };

        handle_post_message(raw_path, body_str, stream, controller, config);
        return;
    }

    // Helpful status for browser inspection
    if method == "GET" {
        let html = format!(
            "<!DOCTYPE html><html><head><title>SolidCraft MCP</title></head>\
             <body style='font-family:sans-serif;padding:2rem;background:#18181b;color:#f4f4f5;'>\
             <h2>SolidCraft Model Context Protocol (MCP) Server</h2>\
             <p>Status: <strong style='color:#4ade80;'>Running</strong></p>\
             <p>SSE Endpoint: <code>/sse</code></p>\
             <p>JSON-RPC Endpoint: <code>/message</code></p>\
             </body></html>"
        );
        send_http_response(&mut stream, 200, "OK", "text/html", html.as_bytes());
        return;
    }

    send_http_response(&mut stream, 404, "Not Found", "text/plain", b"Not Found");
}

fn handle_sse_stream(mut stream: TcpStream, controller: McpServerController) {
    let session_id = format!("sess_{}", controller.inner.session_counter.fetch_add(1, Ordering::SeqCst));
    let (tx, rx) = channel::<String>();

    if let Ok(mut map) = controller.inner.sessions.lock() {
        map.insert(session_id.clone(), tx);
    }

    controller.log(McpLogLevel::Info, format!("SSE connection opened for {session_id}"));

    let headers = format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: text/event-stream\r\n\
         Cache-Control: no-cache\r\n\
         Connection: keep-alive\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Headers: *\r\n\
         \r\n\
         event: endpoint\r\ndata: /message?sessionId={session_id}\r\n\r\n"
    );

    if stream.write_all(headers.as_bytes()).is_err() || stream.flush().is_err() {
        return;
    }

    // Keep stream alive and send pending messages
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    while !controller.inner.shutdown_flag.load(Ordering::Relaxed) {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(msg) => {
                let event_payload = format!("event: message\r\ndata: {msg}\r\n\r\n");
                if stream.write_all(event_payload.as_bytes()).is_err() || stream.flush().is_err() {
                    break;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // Keep-alive comment line
                if stream.write_all(b": keep-alive\r\n\r\n").is_err() || stream.flush().is_err() {
                    break;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    if let Ok(mut map) = controller.inner.sessions.lock() {
        map.remove(&session_id);
    }
    controller.log(McpLogLevel::Info, format!("SSE connection closed for {session_id}"));
}

fn handle_post_message(raw_path: &str, body: String, mut stream: TcpStream, controller: McpServerController, config: McpServerConfig) {
    let session_id = raw_path.split('?').nth(1).and_then(|q| {
        q.split('&').find_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            if parts.next()? == "sessionId" { parts.next() } else { None }
        })
    });

    controller.log(McpLogLevel::Request, body.trim());

    let backend = create_backend(&config);
    let scope = agent_scope(config.workspace_path.into_iter());
    let mut server = Server::new(backend).with_path_scope(Some(scope));

    let reply = server.handle_line(body.trim());

    if let Some(session_id) = session_id {
        // Push reply over existing SSE session
        let delivered = if let Ok(map) = controller.inner.sessions.lock() {
            if let Some(tx) = map.get(session_id) {
                if let Some(ref r) = reply {
                    controller.log(McpLogLevel::Response, r);
                    tx.send(r.clone()).is_ok()
                } else {
                    true
                }
            } else {
                false
            }
        } else {
            false
        };

        if delivered {
            send_http_response(&mut stream, 202, "Accepted", "text/plain", b"Accepted");
            return;
        }
    }

    // Direct HTTP response (Streamable HTTP / synchronous MCP)
    if let Some(r) = reply {
        controller.log(McpLogLevel::Response, &r);
        send_http_response(&mut stream, 200, "OK", "application/json", r.as_bytes());
    } else {
        send_http_response(&mut stream, 204, "No Content", "text/plain", b"");
    }
}

fn send_cors_response(stream: &mut TcpStream) {
    let res = "HTTP/1.1 204 No Content\r\n\
               Access-Control-Allow-Origin: *\r\n\
               Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
               Access-Control-Allow-Headers: Content-Type, Authorization, Accept\r\n\
               Access-Control-Max-Age: 86400\r\n\
               \r\n";
    let _ = stream.write_all(res.as_bytes());
    let _ = stream.flush();
}

fn send_http_response(stream: &mut TcpStream, status_code: u16, reason: &str, content_type: &str, body: &[u8]) {
    let res = format!(
        "HTTP/1.1 {status_code} {reason}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Headers: Content-Type, Authorization, Accept\r\n\
         Connection: close\r\n\
         \r\n",
        body.len()
    );
    let _ = stream.write_all(res.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_controller_lifecycle() {
        let ctrl = McpServerController::new();
        assert_eq!(ctrl.status(), McpStatus::Stopped);
        assert!(!ctrl.is_running());

        ctrl.log(McpLogLevel::Info, "Testing log entry");
        let logs = ctrl.logs();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].level, McpLogLevel::Info);
        assert_eq!(logs[0].message, "Testing log entry");

        ctrl.clear_logs();
        assert!(ctrl.logs().is_empty());

        let cfg = McpServerConfig {
            host: "127.0.0.1".into(),
            port: 0, // OS assigns an ephemeral port
            transport: McpTransportMode::SseHttp,
            workspace_path: None,
            control_port: None,
            control_token: None,
        };

        let started = ctrl.start(cfg);
        assert!(started.is_ok(), "{started:?}");
        assert!(ctrl.is_running());
        let status = ctrl.status();
        match status {
            McpStatus::Running { host, port, transport, endpoint_url } => {
                assert_eq!(host, "127.0.0.1");
                assert!(port > 0);
                assert_eq!(transport, McpTransportMode::SseHttp);
                assert!(endpoint_url.contains(&port.to_string()));
            }
            _ => panic!("Expected running status, got {status:?}"),
        }

        ctrl.stop();
        assert_eq!(ctrl.status(), McpStatus::Stopped);
        assert!(!ctrl.is_running());
    }

    #[test]
    fn transport_modes_and_strings() {
        assert_eq!(McpTransportMode::from_str_loose("sse"), McpTransportMode::SseHttp);
        assert_eq!(McpTransportMode::from_str_loose("stdio"), McpTransportMode::Stdio);
        assert_eq!(McpTransportMode::from_str_loose("ws"), McpTransportMode::WebSocket);
        assert_eq!(McpTransportMode::from_str_loose("websocket"), McpTransportMode::WebSocket);
        assert_eq!(McpTransportMode::SseHttp.as_str(), "sse");
    }

    #[test]
    fn sse_endpoint_and_direct_http_post() {
        let ctrl = McpServerController::new();
        let cfg = McpServerConfig {
            host: "127.0.0.1".into(),
            port: 0,
            transport: McpTransportMode::SseHttp,
            workspace_path: None,
            control_port: None,
            control_token: None,
        };
        ctrl.start(cfg).unwrap();
        let status = ctrl.status();
        let port = match status {
            McpStatus::Running { port, .. } => port,
            _ => panic!("Expected running"),
        };

        // 1. Direct HTTP POST /message with JSON-RPC ping
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let ping_req = r#"{"jsonrpc":"2.0","id":42,"method":"ping"}"#;
        let post_data = format!(
            "POST /message HTTP/1.1\r\n\
             Host: 127.0.0.1:{port}\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\
             \r\n\
             {ping_req}",
            ping_req.len()
        );
        stream.write_all(post_data.as_bytes()).unwrap();

        let mut reader = BufReader::new(stream);
        let mut status_line = String::new();
        reader.read_line(&mut status_line).unwrap();
        assert!(status_line.contains("200 OK"), "Got status: {status_line}");

        // 2. Browser GET / check
        let mut stream2 = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream2.write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();
        let mut reader2 = BufReader::new(stream2);
        let mut get_status = String::new();
        reader2.read_line(&mut get_status).unwrap();
        assert!(get_status.contains("200 OK"), "Got GET status: {get_status}");

        ctrl.stop();
        assert!(!ctrl.is_running());
    }
}
