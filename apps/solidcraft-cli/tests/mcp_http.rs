//! End-to-end tests for `solidcraft-cli mcp --http` (local loopback MCP Streamable HTTP).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("solidcraft-mcp-http-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct ChildGuard(Option<Child>);

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self(Some(child))
    }

    fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        match self.0.as_mut() {
            Some(c) => c.try_wait(),
            None => Ok(None),
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

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

fn post_mcp(addr: SocketAddr, token: Option<&str>, session_id: Option<&str>, body: &Value) -> HttpResponse {
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
    send_http(addr, req.as_bytes())
}

fn wait_for_info(info_path: &Path, child: &mut ChildGuard) -> (SocketAddr, String, u32) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(text) = std::fs::read_to_string(info_path)
            && let Ok(v) = serde_json::from_str::<Value>(&text)
            && let (Some(url), Some(token), Some(pid)) = (v["url"].as_str(), v["token"].as_str(), v["pid"].as_u64())
        {
            let host_port = url.strip_prefix("http://").unwrap().strip_suffix("/mcp").unwrap();
            let addr: SocketAddr = host_port.parse().unwrap();
            return (addr, token.to_string(), pid as u32);
        }
        if let Some(status) = child.try_wait().unwrap() {
            panic!("solidcraft-cli mcp --http exited early with {status}");
        }
        assert!(Instant::now() < deadline, "timed out waiting for {}", info_path.display());
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn wait_for_listening_addr(stderr_path: &Path, child: &mut ChildGuard) -> SocketAddr {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(text) = std::fs::read_to_string(stderr_path)
            && let Some(rest) = text.split("listening on http://").nth(1)
            && let Some(host_port) = rest.split("/mcp").next()
            && let Ok(addr) = host_port.parse::<SocketAddr>()
        {
            return addr;
        }
        if let Some(status) = child.try_wait().unwrap() {
            let stderr = std::fs::read_to_string(stderr_path).unwrap_or_default();
            panic!("solidcraft-cli mcp --http exited early with {status}: {stderr}");
        }
        assert!(Instant::now() < deadline, "timed out waiting for listening URL in {}", stderr_path.display());
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn cli_mcp_http_rejects_non_loopback_address() {
    for addr in ["0.0.0.0:8765", "192.168.1.5:8765", "[::]:8765"] {
        let out =
            Command::new(env!("CARGO_BIN_EXE_solidcraft-cli")).args(["mcp", "--headless", "--http", addr]).stdin(Stdio::null()).output().unwrap();
        assert!(!out.status.success(), "{addr} should be refused");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("loopback"), "{addr}: {stderr}");
    }
}

#[test]
fn cli_mcp_http_rejects_missing_or_wrong_token_and_shares_warm_session_across_clients() {
    let dir = TempDir::new("shared");
    let info_path = dir.path().join("mcp-info.json");
    let stderr_path = dir.path().join("stderr.log");
    let stderr_file = std::fs::File::create(&stderr_path).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli"))
        .args(["mcp", "--headless", "--allow-any-path", "--http", "127.0.0.1:0", "--http-info-file"])
        .arg(&info_path)
        .env_remove("SOLIDCRAFT_MCP_TOKEN")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .unwrap();
    let expected_pid = child.id();
    let mut guard = ChildGuard::new(child);

    let (addr, token, pid) = wait_for_info(&info_path, &mut guard);
    assert_eq!(pid, expected_pid);
    assert_eq!(token.len(), 64);

    let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}});

    // Missing or wrong bearer token gets 401 Unauthorized.
    assert_eq!(post_mcp(addr, None, None, &init).status, 401);
    assert_eq!(post_mcp(addr, Some("wrong-bearer-token-0123456789"), None, &init).status, 401);

    // Non-loopback Origin header is rejected with 403 Forbidden (DNS rebinding protection).
    let bad_origin = format!(
        "POST /mcp HTTP/1.1\r\nHost: {addr}\r\nOrigin: https://evil.example\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
    );
    assert_eq!(send_http(addr, bad_origin.as_bytes()).status, 403);

    // Client 1 initializes and creates a box.
    let init1 = post_mcp(addr, Some(&token), None, &init);
    assert_eq!(init1.status, 200);
    let sid1 = init1.header("mcp-session-id").unwrap().to_string();
    let make_box = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {"name": "execute", "arguments": {"command": "solid.box", "params": {"length": 10, "width": 20, "height": 30}}}
    });
    let box_resp = post_mcp(addr, Some(&token), Some(&sid1), &make_box);
    assert_eq!(box_resp.status, 200);
    assert_ne!(box_resp.json()["result"]["isError"], true);

    // Client 2 initializes and immediately sees Client 1's box without reloading.
    let init2 = post_mcp(addr, Some(&token), None, &init);
    assert_eq!(init2.status, 200);
    let sid2 = init2.header("mcp-session-id").unwrap().to_string();
    assert_ne!(sid1, sid2);
    let measure = json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "measure", "arguments": {}}});
    let m_resp = post_mcp(addr, Some(&token), Some(&sid2), &measure);
    assert_eq!(m_resp.status, 200);
    let m_text = m_resp.json()["result"]["content"][0]["text"].as_str().unwrap().to_string();
    let m_val: Value = serde_json::from_str(&m_text).unwrap();
    assert_eq!(m_val["body_count"], 1, "{m_val}");
    assert!((m_val["total"]["volume_mm3"].as_f64().unwrap() - 6000.0).abs() < 1e-4);

    // DELETE /mcp closes Client 1's session while Client 2 keeps working.
    let del = format!("DELETE /mcp HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nMcp-Session-Id: {sid1}\r\nConnection: close\r\n\r\n");
    assert_eq!(send_http(addr, del.as_bytes()).status, 204);
    assert_eq!(post_mcp(addr, Some(&token), Some(&sid1), &measure).status, 404);
    assert_eq!(post_mcp(addr, Some(&token), Some(&sid2), &measure).status, 200);

    // Generated bearer token must never be printed to stderr; stderr logs the info-file path instead.
    let stderr = std::fs::read_to_string(&stderr_path).unwrap();
    assert!(!stderr.contains(&token), "generated bearer token must never appear on stderr: {stderr}");
    assert!(stderr.contains(&info_path.display().to_string()), "stderr should print the info-file path: {stderr}");
}

#[test]
fn cli_mcp_http_accepts_token_file_and_env_var() {
    let dir = TempDir::new("token");

    // --http-token-file without --http is refused.
    let out = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli"))
        .args(["mcp", "--headless", "--http-token-file", "some.token"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("require --http"));

    // Explicit --http-token-file works end-to-end and never prints the token to stderr.
    let token_path = dir.path().join("fixed.token");
    let info_path = dir.path().join("info.json");
    let stderr_path = dir.path().join("stderr.log");
    let stderr_file = std::fs::File::create(&stderr_path).unwrap();
    let fixed_token = "fixed-file-token-0123456789abcdef";
    std::fs::write(&token_path, format!("{fixed_token}\n")).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli"))
        .args(["mcp", "--headless", "--allow-any-path", "--http", "127.0.0.1:0", "--http-token-file"])
        .arg(&token_path)
        .arg("--http-info-file")
        .arg(&info_path)
        .env("SOLIDCRAFT_MCP_TOKEN", "env-token-ignored-0123456789")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .unwrap();
    let mut guard = ChildGuard::new(child);
    let (addr, token, _) = wait_for_info(&info_path, &mut guard);
    assert_eq!(token, fixed_token);

    let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}});
    assert_eq!(post_mcp(addr, Some(fixed_token), None, &init).status, 200);
    assert_eq!(post_mcp(addr, Some("env-token-ignored-0123456789"), None, &init).status, 401);

    let stderr = std::fs::read_to_string(&stderr_path).unwrap();
    assert!(!stderr.contains(fixed_token), "bearer token must never appear on stderr: {stderr}");
    assert!(stderr.contains(&info_path.display().to_string()), "stderr should print the info-file path: {stderr}");
}

#[test]
fn cli_mcp_http_requires_info_file_when_generating_token_and_allows_env_token_without_info_file() {
    // Generating a token without --http-info-file is refused so the token is never lost or printed to stderr.
    let out = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli"))
        .args(["mcp", "--headless", "--http", "127.0.0.1:0"])
        .env_remove("SOLIDCRAFT_MCP_TOKEN")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!out.status.success(), "--http without token or --http-info-file should be refused");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--http-info-file"), "error should mention --http-info-file: {err}");
    assert!(err.contains("--http-token-file"), "error should mention --http-token-file: {err}");
    assert!(err.contains("SOLIDCRAFT_MCP_TOKEN"), "error should mention SOLIDCRAFT_MCP_TOKEN: {err}");

    // Supplying SOLIDCRAFT_MCP_TOKEN works without --http-info-file and never prints the token to stderr.
    let dir = TempDir::new("env-token");
    let stderr_path = dir.path().join("stderr.log");
    let stderr_file = std::fs::File::create(&stderr_path).unwrap();
    let env_token = "env-bearer-token-0123456789abcdef";
    let child = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli"))
        .args(["mcp", "--headless", "--allow-any-path", "--http", "127.0.0.1:0"])
        .env("SOLIDCRAFT_MCP_TOKEN", env_token)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .unwrap();
    let mut guard = ChildGuard::new(child);

    let addr = wait_for_listening_addr(&stderr_path, &mut guard);
    let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}});
    assert_eq!(post_mcp(addr, Some(env_token), None, &init).status, 200);
    assert_eq!(post_mcp(addr, None, None, &init).status, 401);

    let stderr = std::fs::read_to_string(&stderr_path).unwrap();
    assert!(!stderr.contains(env_token), "env bearer token must never appear on stderr: {stderr}");
    assert!(stderr.contains("SOLIDCRAFT_MCP_TOKEN"), "stderr should note token source: {stderr}");
}
