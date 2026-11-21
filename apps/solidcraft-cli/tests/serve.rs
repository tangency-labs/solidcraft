//! `solidcraft-cli serve` over real HTTP: the token is required, files stay in the data folder,
//! MCP tools run over the streamable-HTTP transport, and the file API saves and loads.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use serde_json::{Value, json};

const TOKEN: &str = "0123456789abcdef0123456789abcdef-test-token";

/// A server on a free loopback port over a fresh data folder; killed when dropped.
struct Server {
    child: Child,
    addr: String,
    data: PathBuf,
    _dir: TempDir,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let d = std::env::temp_dir().join(format!("solidcraft-serve-test-{tag}-{}-{seed:x}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        TempDir(std::fs::canonicalize(&d).unwrap())
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn start(tag: &str, extra: &[&str]) -> Server {
    start_with(tag, extra, |_| None, Some(TOKEN))
}

/// `start`, with `prepare` making the data folder's files first and naming what `--dir` gets
/// (`None`: the folder), and `token` in the environment (`None`: no token given).
fn start_with(tag: &str, extra: &[&str], prepare: impl FnOnce(&std::path::Path) -> Option<PathBuf>, token: Option<&str>) -> Server {
    let dir = TempDir::new(tag);
    let data = dir.0.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let target = prepare(&data).unwrap_or_else(|| data.clone());
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli"));
    cmd.args(["serve", "--listen", "127.0.0.1:0", "--dir"]).arg(&target).args(extra);
    match token {
        Some(t) => cmd.env("SOLIDCRAFT_SERVE_TOKEN", t),
        None => cmd.env_remove("SOLIDCRAFT_SERVE_TOKEN"),
    };
    let mut child = cmd.env("SOLIDCRAFT_GPU", "cpu").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap();
    let mut err = BufReader::new(child.stderr.take().unwrap());
    let mut seen = String::new();
    let addr = loop {
        let mut line = String::new();
        if err.read_line(&mut line).unwrap() == 0 {
            let _ = child.kill();
            panic!("serve stopped before listening:\n{seen}");
        }
        seen.push_str(&line);
        if let Some(rest) = line.split("listening on http://").nth(1) {
            break rest.split_whitespace().next().unwrap().to_string();
        }
    };
    // Keep reading stderr so the server never blocks on a full pipe.
    std::thread::spawn(move || {
        let mut sink = Vec::new();
        let _ = err.read_to_end(&mut sink);
    });
    Server { child, addr, data, _dir: dir }
}

struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Reply {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&self.body)))
    }
}

/// Send raw request bytes and read the whole reply (the server closes after each one).
fn raw(addr: &str, request: &[u8]) -> Reply {
    let mut s = TcpStream::connect(addr).unwrap();
    s.write_all(request).unwrap();
    let mut out = Vec::new();
    s.read_to_end(&mut out).unwrap();
    let split = out.windows(4).position(|w| w == b"\r\n\r\n").expect("a reply head");
    let head = String::from_utf8_lossy(&out[..split]).into_owned();
    let mut lines = head.split("\r\n");
    let status = lines.next().unwrap().split(' ').nth(1).unwrap().parse().unwrap();
    let headers = lines.filter_map(|l| l.split_once(": ")).map(|(n, v)| (n.to_string(), v.to_string())).collect();
    Reply { status, headers, body: out[split + 4..].to_vec() }
}

fn request(addr: &str, method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> Reply {
    let mut r = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Length: {}\r\n", body.len());
    for (n, v) in headers {
        r.push_str(&format!("{n}: {v}\r\n"));
    }
    r.push_str("\r\n");
    let mut bytes = r.into_bytes();
    bytes.extend_from_slice(body);
    raw(addr, &bytes)
}

fn bearer() -> String {
    format!("Bearer {TOKEN}")
}

fn authed(addr: &str, method: &str, path: &str, extra: &[(&str, &str)], body: &[u8]) -> Reply {
    let auth = bearer();
    let mut headers = vec![("Authorization", auth.as_str())];
    headers.extend_from_slice(extra);
    request(addr, method, path, &headers, body)
}

/// One MCP message in, its reply out.
fn mcp(addr: &str, session: Option<&str>, msg: &Value) -> Reply {
    let mut extra = vec![("Content-Type", "application/json"), ("Accept", "application/json, text/event-stream")];
    if let Some(s) = session {
        extra.push(("Mcp-Session-Id", s));
    }
    authed(addr, "POST", "/mcp", &extra, msg.to_string().as_bytes())
}

/// An initialized MCP session's id.
fn initialize(addr: &str) -> String {
    let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}}});
    let r = mcp(addr, None, &init);
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    assert!(r.json()["result"]["serverInfo"]["name"].is_string(), "{}", r.json());
    let id = r.header("Mcp-Session-Id").expect("a session id").to_string();
    let note = mcp(addr, Some(&id), &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    assert_eq!(note.status, 202);
    id
}

fn tool(addr: &str, session: &str, name: &str, args: Value) -> Value {
    let r = mcp(addr, Some(session), &json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": {"name": name, "arguments": args}}));
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    r.json()["result"].clone()
}

fn tool_text(result: &Value) -> String {
    result["content"].as_array().map(|c| c.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n")).unwrap_or_default()
}

#[test]
fn requests_without_the_token_are_refused() {
    let s = start("auth", &[]);
    for (method, path) in [("GET", "/api/v1/files"), ("GET", "/api/v1/info"), ("PUT", "/api/v1/files/a.sc"), ("POST", "/mcp")] {
        let none = request(&s.addr, method, path, &[], b"{}");
        assert_eq!(none.status, 401, "{method} {path} without a token");
        assert_eq!(none.header("WWW-Authenticate"), Some("Bearer"));
        let wrong = request(&s.addr, method, path, &[("Authorization", "Bearer 0123456789abcdef0123456789abcdef-test-tokeX")], b"{}");
        assert_eq!(wrong.status, 401, "{method} {path} with a wrong token");
        let basic = request(&s.addr, method, path, &[("Authorization", &format!("Basic {TOKEN}"))], b"{}");
        assert_eq!(basic.status, 401, "{method} {path} with another scheme");
    }
    assert!(!s.data.join("a.sc").exists(), "a refused PUT wrote nothing");
    // The health check needs none, and says nothing else.
    let health = request(&s.addr, "GET", "/api/v1/health", &[], b"");
    assert_eq!((health.status, health.json()), (200, json!({"ok": true})));
    // A page from another site can't use the API from a browser, even with the token.
    let cross = authed(&s.addr, "GET", "/api/v1/files", &[("Origin", "https://elsewhere.example")], b"");
    assert_eq!(cross.status, 403);
    let same = authed(&s.addr, "GET", "/api/v1/files", &[("Origin", &format!("http://{}", s.addr))], b"");
    assert_eq!(same.status, 200);
}

#[test]
fn the_server_will_not_start_without_a_good_token() {
    let dir = TempDir::new("start");
    let run = |token: Option<&str>| {
        let mut c = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli"));
        c.args(["serve", "--listen", "127.0.0.1:0", "--dir"]).arg(&dir.0).env_remove("SOLIDCRAFT_SERVE_TOKEN");
        if let Some(t) = token {
            c.env("SOLIDCRAFT_SERVE_TOKEN", t);
        }
        let out = c.output().unwrap();
        (out.status.success(), String::from_utf8_lossy(&out.stderr).into_owned())
    };
    let (ok, err) = run(None);
    assert!(!ok && err.contains("a token is required"), "{err}");
    let (ok, err) = run(Some("short"));
    assert!(!ok && err.contains("too short"), "{err}");
    let out = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli")).args(["serve", "--dir"]).arg(&dir.0).args(["--token", TOKEN]).output().unwrap();
    assert!(!out.status.success() && String::from_utf8_lossy(&out.stderr).contains("not on the command line"));
}

#[test]
fn files_outside_the_data_folder_are_refused() {
    let s = start("escape", &[]);
    let outside = s.data.parent().unwrap().join("secret.sc");
    std::fs::write(&outside, b"secret").unwrap();
    for path in [
        "/api/v1/files/../secret.sc",
        "/api/v1/files/%2e%2e/secret.sc",
        "/api/v1/files/a/%2E%2E/%2E%2E/secret.sc",
        "/api/v1/files/%2Fetc%2Fpasswd.sc",
        "/api/v1/files//etc/passwd.sc",
        "/api/v1/files/..%5Csecret.sc",
        "/api/v1/files/.hidden.sc",
    ] {
        let get = authed(&s.addr, "GET", path, &[], b"");
        assert!(matches!(get.status, 400 | 404), "GET {path}: {}", get.status);
        assert!(!String::from_utf8_lossy(&get.body).contains("secret\""), "GET {path} leaked the file");
        let put = authed(&s.addr, "PUT", path, &[], b"overwritten");
        assert!(matches!(put.status, 400 | 404), "PUT {path}: {}", put.status);
    }
    assert_eq!(authed(&s.addr, "GET", "/api/v1/files/notes.txt", &[], b"").status, 415);
    assert_eq!(std::fs::read(&outside).unwrap(), b"secret", "nothing outside was written");

    #[cfg(unix)]
    {
        // A link inside the data folder that leads out is refused, both ways.
        std::os::unix::fs::symlink(&outside, s.data.join("link.sc")).unwrap();
        std::os::unix::fs::symlink(s.data.parent().unwrap(), s.data.join("up")).unwrap();
        for path in ["/api/v1/files/link.sc", "/api/v1/files/up/secret.sc", "/api/v1/files/up/new.sc"] {
            assert_eq!(authed(&s.addr, "GET", path, &[], b"").status, 400, "GET {path}");
            assert_eq!(authed(&s.addr, "PUT", path, &[], b"overwritten").status, 400, "PUT {path}");
            assert_eq!(authed(&s.addr, "DELETE", path, &[], b"").status, 400, "DELETE {path}");
        }
        assert_eq!(std::fs::read(&outside).unwrap(), b"secret", "nothing was written through a link");
        assert!(!s.data.parent().unwrap().join("new.sc").exists());
        let listed = authed(&s.addr, "GET", "/api/v1/files", &[], b"").json();
        assert_eq!(listed["files"], json!([]), "links are not listed: {listed}");

        // MCP tools reach only the data folder too.
        let session = initialize(&s.addr);
        let opened = tool(&s.addr, &session, "open", json!({"path": outside.to_string_lossy()}));
        assert_eq!(opened["isError"], true, "{opened}");
        assert!(tool_text(&opened).contains("outside the folders"), "{opened}");
        tool(&s.addr, &session, "execute", json!({"command": "solid.box", "params": {"length": 5, "width": 5, "height": 5}}));
        for path in [outside.parent().unwrap().join("escaped.sc"), s.data.join("up/escaped.sc")] {
            let saved = tool(&s.addr, &session, "save", json!({"path": path.to_string_lossy()}));
            assert_eq!(saved["isError"], true, "save to {}: {saved}", path.display());
        }
        assert!(!outside.parent().unwrap().join("escaped.sc").exists());
    }
}

#[test]
fn an_mcp_tool_call_over_http_saves_into_the_data_folder() {
    let s = start("mcp", &[]);
    // Not initialized: no session.
    let early = mcp(&s.addr, None, &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}));
    assert_eq!(early.status, 400);
    let gone = mcp(&s.addr, Some("no-such-session"), &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}));
    assert_eq!(gone.status, 404);

    let session = initialize(&s.addr);
    let tools = mcp(&s.addr, Some(&session), &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).json();
    let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(names.contains(&"execute") && names.contains(&"save"), "{names:?}");

    let made = tool(
        &s.addr,
        &session,
        "execute",
        json!({"command": "solid.box", "params": {"length": 30, "width": 20, "height": 10, "body_name": "Served"}}),
    );
    assert_ne!(made["isError"], true, "{made}");
    // Relative to the data folder.
    let saved = tool(&s.addr, &session, "save", json!({"path": "served.sc"}));
    assert_ne!(saved["isError"], true, "{saved}");
    assert!(s.data.join("served.sc").is_file());

    // The browser's API sees it.
    let listed = authed(&s.addr, "GET", "/api/v1/files", &[], b"").json();
    assert_eq!(listed["files"][0]["path"], "served.sc", "{listed}");

    // Plugins are off unless the server allows them.
    let plugin = tool(&s.addr, &session, "execute", json!({"command": "plugin.list", "params": {}}));
    assert_eq!(plugin["isError"], true, "{plugin}");
    assert!(tool_text(&plugin).contains("plugins are off"), "{plugin}");

    // A second session is a design of its own.
    let other = initialize(&s.addr);
    let inspected = tool(&s.addr, &other, "measure", json!({}));
    assert!(!inspected.to_string().contains("Served"), "{inspected}");

    // Ending a session ends it.
    let auth = bearer();
    let ended = request(&s.addr, "DELETE", "/mcp", &[("Authorization", &auth), ("Mcp-Session-Id", &session)], b"");
    assert_eq!(ended.status, 204);
    let after = mcp(&s.addr, Some(&session), &json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list"}));
    assert_eq!(after.status, 404);
    assert_eq!(authed(&s.addr, "GET", "/mcp", &[], b"").status, 405);
}

#[test]
fn a_saved_file_loads_back_and_a_stale_save_is_refused() {
    let s = start("files", &["--max-upload", "1MB"]);
    let bytes = b"a design's bytes \x00\x01\x02";
    let put = authed(&s.addr, "PUT", "/api/v1/files/Parts/bracket%20v1.sc", &[("If-None-Match", "*")], bytes);
    assert_eq!(put.status, 200, "{}", String::from_utf8_lossy(&put.body));
    let etag = put.header("ETag").unwrap().to_string();
    assert_eq!(std::fs::read(s.data.join("Parts/bracket v1.sc")).unwrap(), bytes);

    let get = authed(&s.addr, "GET", "/api/v1/files/Parts/bracket%20v1.sc", &[], b"");
    assert_eq!((get.status, get.body.as_slice()), (200, &bytes[..]));
    assert_eq!(get.header("ETag"), Some(etag.as_str()));

    let listed = authed(&s.addr, "GET", "/api/v1/files", &[], b"").json();
    assert_eq!(listed["files"][0]["path"], "Parts/bracket v1.sc");
    assert_eq!(listed["files"][0]["size"], bytes.len());

    // Saved over with the version read: fine. With that version again: someone saved meanwhile.
    let second = authed(&s.addr, "PUT", "/api/v1/files/Parts/bracket%20v1.sc", &[("If-Match", &etag)], b"second");
    assert_eq!(second.status, 200);
    std::thread::sleep(std::time::Duration::from_millis(20));
    let stale = authed(&s.addr, "PUT", "/api/v1/files/Parts/bracket%20v1.sc", &[("If-Match", &etag)], b"third");
    assert_eq!(stale.status, 412);
    assert_eq!(std::fs::read(s.data.join("Parts/bracket v1.sc")).unwrap(), b"second");
    assert_eq!(authed(&s.addr, "PUT", "/api/v1/files/Parts/bracket%20v1.sc", &[("If-None-Match", "*")], b"x").status, 412);

    // Too large, and chunked, are refused without writing.
    assert_eq!(authed(&s.addr, "PUT", "/api/v1/files/big.sc", &[], &vec![b'x'; (1 << 20) + 1]).status, 413);
    let chunked = raw(
        &s.addr,
        format!(
            "PUT /api/v1/files/chunked.sc HTTP/1.1\r\nHost: {}\r\nAuthorization: {}\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n0\r\n\r\n",
            s.addr,
            bearer()
        )
        .as_bytes(),
    );
    assert_eq!(chunked.status, 411);
    assert!(!s.data.join("big.sc").exists() && !s.data.join("chunked.sc").exists());
    let leftovers: Vec<_> = std::fs::read_dir(s.data.join("Parts")).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(leftovers.len(), 1, "no temporary files are left behind: {leftovers:?}");

    let deleted = authed(&s.addr, "DELETE", "/api/v1/files/Parts/bracket%20v1.sc", &[], b"");
    assert_eq!(deleted.status, 204);
    assert!(!s.data.join("Parts/bracket v1.sc").exists());
    assert!(s.data.join("Parts").is_dir(), "folders stay");
}

#[test]
fn the_browser_app_is_served_with_its_caching_rules() {
    let web = TempDir::new("web");
    std::fs::write(web.0.join("index.html"), "<canvas id=solidcraft_canvas>").unwrap();
    std::fs::write(web.0.join("solidcraft-web-0123456789abcdef_bg.wasm"), b"\0asm").unwrap();
    std::fs::write(web.0.join(".env"), "SECRET=1").unwrap();
    let s = start("web", &["--web", web.0.to_str().unwrap()]);
    let index = request(&s.addr, "GET", "/", &[], b"");
    assert_eq!(index.status, 200);
    assert_eq!(index.header("Cache-Control"), Some("no-cache"));
    assert!(String::from_utf8_lossy(&index.body).contains("solidcraft_canvas"));
    let wasm = request(&s.addr, "GET", "/solidcraft-web-0123456789abcdef_bg.wasm", &[], b"");
    assert_eq!(wasm.header("Content-Type"), Some("application/wasm"));
    assert!(wasm.header("Cache-Control").unwrap().contains("immutable"));
    for path in ["/.env", "/../data", "/%2e%2e/", "/no-such-file"] {
        assert_eq!(request(&s.addr, "GET", path, &[], b"").status, 404, "{path}");
    }
}

#[test]
fn the_folder_browser_lists_one_folder_without_git_or_dotfiles() {
    let s = start_with(
        "browse",
        &[],
        |data| {
            // A git checkout, as the server is often pointed at one.
            std::fs::create_dir_all(data.join(".git/objects")).unwrap();
            std::fs::write(data.join(".git/config"), "[core]").unwrap();
            std::fs::write(data.join(".env"), "SECRET=1").unwrap();
            std::fs::create_dir_all(data.join("mechanical/old")).unwrap();
            std::fs::write(data.join("mechanical/case.sc"), b"case").unwrap();
            std::fs::write(data.join("mechanical/notes.txt"), b"not a design").unwrap();
            std::fs::write(data.join("README.md"), b"readme").unwrap();
            None
        },
        Some(TOKEN),
    );
    let root = authed(&s.addr, "GET", "/api/v1/folder", &[], b"").json();
    assert_eq!(root["folders"], json!([{"name": "mechanical", "path": "mechanical"}]), "{root}");
    assert_eq!(root["files"], json!([]), "{root}");
    let mech = authed(&s.addr, "GET", "/api/v1/folder/mechanical", &[], b"").json();
    assert_eq!(mech["folders"], json!([{"name": "old", "path": "mechanical/old"}]), "{mech}");
    assert_eq!(mech["files"].as_array().unwrap().len(), 1, "{mech}");
    assert_eq!(mech["files"][0]["path"], "mechanical/case.sc");
    for hidden in ["/api/v1/folder/.git", "/api/v1/folder/..", "/api/v1/files/.git/config.sc"] {
        assert_eq!(authed(&s.addr, "GET", hidden, &[], b"").status, 400, "{hidden}");
    }
    // New folder, then a save in it.
    assert_eq!(authed(&s.addr, "PUT", "/api/v1/folder/mechanical/v2", &[], b"").status, 201);
    assert!(s.data.join("mechanical/v2").is_dir());
    assert_eq!(authed(&s.addr, "PUT", "/api/v1/folder/mechanical/v2", &[], b"").status, 200, "making it again is fine");
    assert_eq!(authed(&s.addr, "PUT", "/api/v1/folder/no/such/parent", &[], b"").status, 404);
    assert_eq!(authed(&s.addr, "PUT", "/api/v1/folder/.hidden", &[], b"").status, 400);

    // Saved in place over the checkout's file: the same file, no temporary files beside it.
    let read = authed(&s.addr, "GET", "/api/v1/files/mechanical/case.sc", &[], b"");
    let etag = read.header("ETag").unwrap().to_string();
    assert_eq!(authed(&s.addr, "PUT", "/api/v1/files/mechanical/case.sc", &[("If-Match", &etag)], b"case v2").status, 200);
    assert_eq!(std::fs::read(s.data.join("mechanical/case.sc")).unwrap(), b"case v2");
    let mut names: Vec<String> =
        std::fs::read_dir(s.data.join("mechanical")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    assert_eq!(names, ["case.sc", "notes.txt", "old", "v2"]);
}

#[test]
fn dir_naming_a_design_serves_its_folder_and_opens_it() {
    let s = start_with(
        "design",
        &[],
        |data| {
            let script = data.join("make.json");
            std::fs::write(
                &script,
                json!([{"command": "solid.box", "params": {"length": 12, "width": 8, "height": 4, "body_name": "Opened"}}]).to_string(),
            )
            .unwrap();
            let saved =
                Command::new(env!("CARGO_BIN_EXE_solidcraft-cli")).arg("run").arg(&script).arg("--save").arg(data.join("case.sc")).output().unwrap();
            assert!(saved.status.success(), "{}", String::from_utf8_lossy(&saved.stderr));
            std::fs::remove_file(&script).unwrap();
            Some(data.join("case.sc"))
        },
        Some(TOKEN),
    );
    let info = authed(&s.addr, "GET", "/api/v1/info", &[], b"").json();
    assert_eq!(info["open"], "case.sc", "{info}");
    let listed = authed(&s.addr, "GET", "/api/v1/files", &[], b"").json();
    assert_eq!(listed["files"][0]["path"], "case.sc", "the design's folder is served: {listed}");
    let session = initialize(&s.addr);
    let measured = tool(&s.addr, &session, "measure", json!({}));
    assert!(measured.to_string().contains("Opened"), "the session starts with the design open: {measured}");
}

#[test]
fn no_token_is_only_for_this_computer() {
    let s = start_with("notoken", &["--no-token"], |_| None, None);
    let ok = request(&s.addr, "GET", "/api/v1/files", &[], b"");
    assert_eq!(ok.status, 200, "{}", String::from_utf8_lossy(&ok.body));
    // A page that reached it through another name (DNS rebinding) is refused.
    let mut r = raw(&s.addr, b"GET /api/v1/files HTTP/1.1\r\nHost: attacker.example:8787\r\n\r\n");
    assert_eq!(r.status, 401);
    r = raw(&s.addr, format!("GET /api/v1/files HTTP/1.1\r\nHost: {}\r\nOrigin: http://attacker.example\r\n\r\n", s.addr).as_bytes());
    assert_eq!(r.status, 403);

    let dir = TempDir::new("notoken-wide");
    let out = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli"))
        .args(["serve", "--no-token", "--listen", "0.0.0.0:0", "--dir"])
        .arg(&dir.0)
        .env_remove("SOLIDCRAFT_SERVE_TOKEN")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("only for --listen on 127.0.0.1"), "{}", String::from_utf8_lossy(&out.stderr));
}
