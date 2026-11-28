//! Local loopback MCP Streamable HTTP helpers for SolidCraft: OS-random bearer token
//! resolution, connection metadata files (`--http-info-file`), and re-exports of the
//! shared HTTP transport from [`solidcraft_mcp_server::http`].

use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

pub use solidcraft_mcp_server::http::{
    DEFAULT_MAX_BODY, DEFAULT_MAX_CONNECTIONS, DEFAULT_MAX_SESSIONS, HttpConfig, HttpStop, MCP_PATH, bind_loopback, endpoint_url,
    is_loopback_host_port, is_loopback_origin, parse_loopback_addr, tokens_match,
};

/// Environment variable providing an explicit Bearer token for `solidcraft-cli mcp --http`.
pub const TOKEN_ENV: &str = "SOLIDCRAFT_MCP_TOKEN";

/// Minimum length accepted for an externally supplied MCP HTTP bearer token.
pub const MIN_TOKEN_LEN: usize = 16;

const MAX_TOKEN_FILE_BYTES: u64 = 4096;
const MAX_INFO_FILE_BYTES: u64 = 4096;

/// Where the HTTP server's bearer token came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenSource {
    File(PathBuf),
    Env,
    Generated,
}

/// Validate a user-supplied bearer token without echoing the secret in any error message.
fn validate_token(token: &str, source: &str) -> Result<(), String> {
    if token.is_empty() {
        return Err(format!("{source}: bearer token is empty"));
    }
    if token.len() < MIN_TOKEN_LEN {
        return Err(format!("{source}: bearer token must be at least {MIN_TOKEN_LEN} characters"));
    }
    if !token.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(format!("{source}: bearer token must contain only printable ASCII characters without whitespace"));
    }
    Ok(())
}

/// Resolve the bearer token from `--http-token-file`, else `env_token` (`SOLIDCRAFT_MCP_TOKEN`),
/// else a fresh 32-byte random hex token from OS entropy.
pub fn resolve_token(token_file: Option<&Path>, env_token: Option<&str>) -> Result<(String, TokenSource), String> {
    if let Some(path) = token_file {
        let file = std::fs::File::open(path).map_err(|e| format!("--http-token-file {}: {e}", path.display()))?;
        let mut text = String::new();
        file.take(MAX_TOKEN_FILE_BYTES + 1).read_to_string(&mut text).map_err(|e| format!("--http-token-file {}: {e}", path.display()))?;
        if text.len() as u64 > MAX_TOKEN_FILE_BYTES {
            return Err(format!("--http-token-file {}: file is too large (max {MAX_TOKEN_FILE_BYTES} bytes)", path.display()));
        }
        let token = text.trim();
        validate_token(token, &format!("--http-token-file {}", path.display()))?;
        return Ok((token.to_string(), TokenSource::File(path.to_path_buf())));
    }
    if let Some(raw) = env_token {
        let token = raw.trim();
        if !token.is_empty() {
            validate_token(token, TOKEN_ENV)?;
            return Ok((token.to_string(), TokenSource::Env));
        }
    }
    let token = solidcraft_control::server::new_token().ok_or_else(|| "could not generate a random bearer token from OS entropy".to_string())?;
    Ok((token, TokenSource::Generated))
}

/// [`resolve_token`] reading [`TOKEN_ENV`] from the current process environment.
pub fn resolve_token_from_env(token_file: Option<&Path>) -> Result<(String, TokenSource), String> {
    let env_token = match std::env::var(TOKEN_ENV) {
        Ok(v) => Some(v),
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => return Err(format!("{TOKEN_ENV}: bearer token is not valid UTF-8")),
    };
    resolve_token(token_file, env_token.as_deref())
}

/// Removes an `--http-info-file` on drop when it still holds this server's token.
pub struct InfoFileGuard {
    path: PathBuf,
    token: String,
}

impl Drop for InfoFileGuard {
    fn drop(&mut self) {
        let Ok(file) = std::fs::File::open(&self.path) else {
            return;
        };
        let mut text = String::new();
        if file.take(MAX_INFO_FILE_BYTES + 1).read_to_string(&mut text).is_err() || text.len() as u64 > MAX_INFO_FILE_BYTES {
            return;
        }
        let matches = serde_json::from_str::<Value>(&text)
            .ok()
            .is_some_and(|v| v.get("token").and_then(Value::as_str).is_some_and(|t| tokens_match(t, &self.token)));
        if matches {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Write connection metadata `{"url": ..., "token": ..., "pid": ...}` to `path` (`0600` on Unix;
/// on Windows the file inherits its parent directory's ACL and does not receive an owner-only ACL,
/// so place it inside a user-private directory). Returns a guard that cleans up the file on
/// graceful shutdown.
pub fn write_info_file(path: &Path, url: &str, token: &str, pid: u32) -> io::Result<InfoFileGuard> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(path)?;
    let payload = json!({
        "url": url,
        "token": token,
        "pid": pid,
    });
    let mut bytes = serde_json::to_vec_pretty(&payload).map_err(io::Error::other)?;
    bytes.push(b'\n');
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(InfoFileGuard { path: path.to_path_buf(), token: token.to_string() })
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::net::{SocketAddr, TcpStream};
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;
    use crate::{Headless, Server};

    struct HttpResponse {
        status: u16,
        headers: Vec<(String, String)>,
        body: String,
    }

    impl HttpResponse {
        fn header(&self, name: &str) -> Option<&str> {
            self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
        }

        fn json(&self) -> Value {
            serde_json::from_str(&self.body).unwrap_or_else(|e| panic!("not JSON ({e}): {:?}", self.body))
        }
    }

    fn send_http(addr: SocketAddr, request: &[u8]) -> HttpResponse {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
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
        HttpResponse { status, headers, body: String::from_utf8(body).unwrap() }
    }

    fn post_mcp(addr: SocketAddr, token: &str, session_id: Option<&str>, body: &Value) -> HttpResponse {
        let payload = body.to_string();
        let mut req = format!(
            "POST /mcp HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n",
            payload.len()
        );
        if let Some(sid) = session_id {
            req.push_str(&format!("Mcp-Session-Id: {sid}\r\n"));
        }
        req.push_str("\r\n");
        req.push_str(&payload);
        send_http(addr, req.as_bytes())
    }

    fn delete_mcp(addr: SocketAddr, token: &str, session_id: &str) -> HttpResponse {
        let req = format!(
            "DELETE /mcp HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nMcp-Session-Id: {session_id}\r\nConnection: close\r\n\r\n"
        );
        send_http(addr, req.as_bytes())
    }

    fn tool_json(resp: &HttpResponse) -> Value {
        let rpc = resp.json();
        assert_ne!(rpc["result"]["isError"], true, "tool error: {rpc}");
        let text = rpc["result"]["content"][0]["text"].as_str().unwrap();
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn two_http_clients_share_one_warm_headless_design_and_delete_is_isolated() {
        let (token, source) = resolve_token(None, None).unwrap();
        assert_eq!(source, TokenSource::Generated);
        assert_eq!(token.len(), 64);

        let listener = bind_loopback(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
        let addr = listener.local_addr().unwrap();
        let config = HttpConfig::new(token.clone());
        let stop = HttpStop::new(addr, Arc::clone(&config.shutdown));
        let server_thread = std::thread::spawn(move || {
            let mut server = Server::new(Box::new(Headless::default()));
            server.serve_http_listener(listener, config)
        });

        // Client 1 initializes and builds a 10 x 20 x 30 mm box (volume = 6000 mm^3).
        let init_req = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}});
        let init1 = post_mcp(addr, &token, None, &init_req);
        assert_eq!(init1.status, 200);
        let sid1 = init1.header("mcp-session-id").unwrap().to_string();
        assert_eq!(post_mcp(addr, &token, Some(&sid1), &json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).status, 202);

        let make_box = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {"name": "execute", "arguments": {"command": "solid.box", "params": {"length": 10, "width": 20, "height": 30}}}
        });
        let box_resp = post_mcp(addr, &token, Some(&sid1), &make_box);
        assert_eq!(box_resp.status, 200);
        assert_ne!(box_resp.json()["result"]["isError"], true);

        // Client 2 initializes its own MCP session and immediately measures the same warm design.
        let init2 = post_mcp(addr, &token, None, &init_req);
        assert_eq!(init2.status, 200);
        let sid2 = init2.header("mcp-session-id").unwrap().to_string();
        assert_ne!(sid1, sid2);

        let measure_req = json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "measure", "arguments": {}}});
        let measured = tool_json(&post_mcp(addr, &token, Some(&sid2), &measure_req));
        assert_eq!(measured["body_count"], 1, "{measured}");
        let vol = measured["total"]["volume_mm3"].as_f64().unwrap();
        assert!((vol - 6000.0).abs() < 1e-4, "expected 6000 mm^3, got {vol}");

        // Client 1 closes its MCP session with DELETE /mcp; Client 2 keeps working cleanly.
        assert_eq!(delete_mcp(addr, &token, &sid1).status, 204);
        assert_eq!(post_mcp(addr, &token, Some(&sid1), &measure_req).status, 404);

        let still_warm = tool_json(&post_mcp(addr, &token, Some(&sid2), &measure_req));
        assert_eq!(still_warm["body_count"], 1, "{still_warm}");
        assert!((still_warm["total"]["volume_mm3"].as_f64().unwrap() - 6000.0).abs() < 1e-4);

        stop.stop();
        server_thread.join().unwrap().unwrap();
    }

    #[test]
    fn concurrent_clients_mutating_shared_headless_session_serialize_cleanly() {
        let token = "concurrent-headless-token-0123456789";
        let listener = bind_loopback(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
        let addr = listener.local_addr().unwrap();
        let config = HttpConfig::new(token);
        let stop = HttpStop::new(addr, Arc::clone(&config.shutdown));
        let server_thread = std::thread::spawn(move || {
            let mut server = Server::new(Box::new(Headless::default()));
            server.serve_http_listener(listener, config)
        });

        let workers: Vec<_> = (0..4)
            .map(|w| {
                let token = token.to_string();
                std::thread::spawn(move || {
                    let init = post_mcp(
                        addr,
                        &token,
                        None,
                        &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}}),
                    );
                    assert_eq!(init.status, 200);
                    let sid = init.header("mcp-session-id").unwrap().to_string();
                    for i in 0..3 {
                        let x = (w * 3 + i) * 20;
                        let req = json!({
                            "jsonrpc": "2.0",
                            "id": i + 2,
                            "method": "tools/call",
                            "params": {
                                "name": "execute",
                                "arguments": {
                                    "command": "solid.box",
                                    "params": {"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0], "body_name": format!("Box_{w}_{i}")}
                                }
                            }
                        });
                        let resp = post_mcp(addr, &token, Some(&sid), &req);
                        assert_eq!(resp.status, 200);
                        assert_ne!(resp.json()["result"]["isError"], true, "worker {w} step {i}: {}", resp.body);
                    }
                    assert_eq!(delete_mcp(addr, &token, &sid).status, 204);
                })
            })
            .collect();
        for w in workers {
            w.join().unwrap();
        }

        // Verify all 12 boxes (12 * 1000 = 12000 mm^3) exist with zero corruption.
        let init =
            post_mcp(addr, token, None, &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}}));
        let sid = init.header("mcp-session-id").unwrap().to_string();
        let measured = tool_json(&post_mcp(
            addr,
            token,
            Some(&sid),
            &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "measure", "arguments": {}}}),
        ));
        assert_eq!(measured["body_count"], 12, "{measured}");
        let vol = measured["total"]["volume_mm3"].as_f64().unwrap();
        assert!((vol - 12_000.0).abs() < 1e-3, "expected 12000 mm^3, got {vol}");

        stop.stop();
        server_thread.join().unwrap().unwrap();
    }

    #[test]
    fn token_resolution_and_info_file_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let token_path = dir.path().join("bearer.token");
        std::fs::write(&token_path, "0123456789abcdef0123456789abcdef\n").unwrap();

        let (tok, src) = resolve_token(Some(&token_path), Some("env-token-should-not-win-0123456")).unwrap();
        assert_eq!(tok, "0123456789abcdef0123456789abcdef");
        assert_eq!(src, TokenSource::File(token_path.clone()));

        let (tok, src) = resolve_token(None, Some("env-token-0123456789abcdef")).unwrap();
        assert_eq!(tok, "env-token-0123456789abcdef");
        assert_eq!(src, TokenSource::Env);

        let (generated, src) = resolve_token(None, None).unwrap();
        assert_eq!(src, TokenSource::Generated);
        assert_eq!(generated.len(), 64);

        // Short or whitespace tokens are refused.
        assert!(resolve_token(None, Some("short")).is_err());
        assert!(resolve_token(None, Some("has space inside 0123456789")).is_err());

        // Info file writes JSON metadata with owner-only permissions on Unix and cleans up on drop.
        let info_path = dir.path().join("mcp-info.json");
        let guard = write_info_file(&info_path, "http://127.0.0.1:8765/mcp", &generated, 4242).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&info_path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        let info: Value = serde_json::from_str(&std::fs::read_to_string(&info_path).unwrap()).unwrap();
        assert_eq!(info, json!({"url": "http://127.0.0.1:8765/mcp", "token": generated, "pid": 4242}));
        drop(guard);
        assert!(!info_path.exists());
    }
}
