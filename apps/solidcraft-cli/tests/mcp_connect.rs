//! `mcp --connect` to a port where no app listens yet (#270): the server still starts, its tools
//! say how to start the app, and once the app runs they reach it with that launch's token.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

#[test]
fn mcp_connect_serves_before_the_app_runs_and_reaches_it_once_it_does() {
    // This test needs the port closed until its second phase. A port obtained
    // with bind(:0) and released can immediately be reused by another parallel
    // test's mock server. Stay below the usual ephemeral ranges,
    // with a process-specific starting point, and skip any occupied ports.
    let port = (0..512u32)
        .find_map(|offset| {
            let port = 16000 + (std::process::id() + offset) % 16000;
            TcpListener::bind(("127.0.0.1", port as u16)).ok().map(|listener| listener.local_addr().unwrap().port())
        })
        .expect("an unused non-ephemeral test port");
    let addr = format!("127.0.0.1:{port}");
    let control_dir = std::env::temp_dir().join(format!("solidcraft-mcp-connect-{}-{port}", std::process::id()));
    std::fs::create_dir_all(&control_dir).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli"))
        .args(["mcp", "--connect", &addr, "--allow-any-path"])
        .env("SOLIDCRAFT_CONTROL_DIR", &control_dir)
        .env_remove("SOLIDCRAFT_CONTROL_TOKEN")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut ask = |id: u64, method: &str, params: Value| -> Value {
        writeln!(stdin, "{}", json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})).unwrap();
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("{e}: {line:?}"))
    };
    let measure = json!({"name": "measure", "arguments": {}});
    let text = |r: &Value| r["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string();

    // The server answers without the app, and its instructions say the app isn't running.
    let init = ask(1, "initialize", json!({"protocolVersion": "2025-06-18"}));
    let instructions = init["result"]["instructions"].as_str().unwrap_or_default();
    assert!(instructions.contains("not running") && instructions.contains(&format!("--control {port}")), "{instructions}");
    let tools = ask(2, "tools/list", json!({}));
    assert!(tools["result"]["tools"].as_array().is_some_and(|t| !t.is_empty()), "{tools}");

    // A tool call says the app is not running and how to start it.
    let r = ask(3, "tools/call", measure.clone());
    assert_eq!(r["result"]["isError"], true, "{r}");
    assert!(text(&r).contains("not running") && text(&r).contains(&format!("solidcraft --control {port}")), "{r}");

    // The app starts: a token file for this launch, and its control port.
    let token = "0123456789abcdef".repeat(4);
    std::fs::write(control_dir.join(format!("{port}.token")), &token).unwrap();
    let listener = TcpListener::bind(&addr).unwrap();
    let app = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut line = String::new();
        BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
        let req: Value = serde_json::from_str(&line).unwrap();
        let reply = if req["token"] == json!(token) {
            json!({"id": req["id"], "ok": true, "result": {"bodies": [], "method": req["method"]}})
        } else {
            json!({"id": req["id"], "ok": false, "error": format!("unauthorized: token {}", req["token"])})
        };
        writeln!(&stream, "{reply}").unwrap();
    });
    let r = ask(4, "tools/call", measure);
    assert_ne!(r["result"]["isError"], true, "{r}");
    assert!(text(&r).contains("engine.execute"), "{r}");
    app.join().unwrap();

    drop(stdin);
    assert!(child.wait().unwrap().success());
    std::fs::remove_dir_all(&control_dir).ok();
}

#[test]
fn mcp_connect_without_a_port_is_refused_at_start() {
    for addr in ["localhost", "127.0.0.1:0"] {
        let output = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli")).args(["mcp", "--connect", addr]).stdin(Stdio::null()).output().unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{addr}: {stderr}");
        assert!(stderr.contains("HOST:PORT"), "{addr}: {stderr}");
    }
}

/// `mcp` with no arguments finds the running app through the discovery file and uses that
/// launch's token; with no app behind the file it stays a headless session.
#[test]
fn mcp_without_arguments_finds_the_running_app() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let control_dir = std::env::temp_dir().join(format!("solidcraft-mcp-discover-{}-{port}", std::process::id()));
    std::fs::create_dir_all(&control_dir).unwrap();
    let token = "fedcba9876543210".repeat(4);
    std::fs::write(control_dir.join(format!("{port}.token")), &token).unwrap();
    std::fs::write(control_dir.join("current.port"), format!("{port}\n")).unwrap();
    let app = std::thread::spawn(move || {
        // The discovery probe connects and leaves without a request; the tool call follows.
        let (stream, line) = loop {
            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
            if !line.is_empty() {
                break (stream, line);
            }
        };
        let req: Value = serde_json::from_str(&line).unwrap();
        let ok = req["token"] == json!(token);
        let reply = json!({"id": req["id"], "ok": ok, "result": {"bodies": [], "method": req["method"]}, "error": "unauthorized"});
        writeln!(&stream, "{reply}").unwrap();
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli"))
        .args(["mcp", "--allow-any-path"])
        .env("SOLIDCRAFT_CONTROL_DIR", &control_dir)
        .env_remove("SOLIDCRAFT_CONTROL_TOKEN")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    writeln!(stdin, "{}", json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "measure", "arguments": {}}})).unwrap();
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    let r: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("{e}: {line:?}"));
    assert_ne!(r["result"]["isError"], true, "{r}");
    assert!(r["result"]["content"][0]["text"].as_str().unwrap_or_default().contains("engine.execute"), "{r}");
    app.join().unwrap();
    drop(stdin);
    assert!(child.wait().unwrap().success());

    // The file names a port nothing listens on (the app crashed): a headless session instead.
    let closed = TcpListener::bind("127.0.0.1:0").unwrap();
    let dead = closed.local_addr().unwrap().port();
    drop(closed);
    std::fs::write(control_dir.join("current.port"), format!("{dead}\n")).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli"))
        .args(["mcp", "--allow-any-path"])
        .env("SOLIDCRAFT_CONTROL_DIR", &control_dir)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&out.stderr).contains("bridged to the running app"), "{}", String::from_utf8_lossy(&out.stderr));
    std::fs::remove_dir_all(&control_dir).ok();
}

/// `mcp setup` against a temporary home: the client folders it finds are the only ones it writes.
mod setup {
    use serde_json::{Value, json};
    use std::path::{Path, PathBuf};
    use std::process::Command;

    fn home(name: &str) -> PathBuf {
        let h = std::env::temp_dir().join(format!("solidcraft-mcp-setup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&h);
        std::fs::create_dir_all(&h).unwrap();
        h
    }

    fn run(home: &Path, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_solidcraft-cli"))
            .args(["mcp", "setup"])
            .args(args)
            .env_clear()
            .env("HOME", home)
            .env("USERPROFILE", home)
            .env("APPDATA", home.join("AppData/Roaming"))
            .env("LOCALAPPDATA", home.join("AppData/Local"))
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("PATH", "")
            .output()
            .unwrap()
    }

    fn json_of(p: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    fn exe() -> String {
        let p = Path::new(env!("CARGO_BIN_EXE_solidcraft-cli")).canonicalize().unwrap();
        p.to_string_lossy().trim_start_matches(r"\\?\").to_owned()
    }

    #[test]
    fn merges_into_opencode_and_cursor_keeping_other_servers() {
        let h = home("merge");
        let oc = h.join(".config/opencode");
        std::fs::create_dir_all(&oc).unwrap();
        let old = json!({"$schema": "https://opencode.ai/config.json", "theme": "x", "mcp": {"other": {"type": "local", "command": ["o"]}}});
        std::fs::write(oc.join("opencode.json"), old.to_string()).unwrap();
        std::fs::create_dir_all(h.join(".cursor")).unwrap();
        std::fs::write(h.join(".cursor/mcp.json"), r#"{"mcpServers": {"other": {"command": "o"}}}"#).unwrap();

        let out = run(&h, &["--live"]);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let v = json_of(&oc.join("opencode.json"));
        assert_eq!(v["theme"], "x");
        assert_eq!(v["mcp"]["other"], old["mcp"]["other"]);
        assert_eq!(v["mcp"]["solidcraft"], json!({"type": "local", "command": [exe(), "mcp"], "enabled": true}));
        assert_eq!(v["mcp"]["solidcraft-live"]["command"], json!([exe(), "mcp", "--connect", "127.0.0.1:7878"]));
        assert_eq!(json_of(&oc.join("opencode.json.bak")), old);
        let c = json_of(&h.join(".cursor/mcp.json"));
        assert_eq!(c["mcpServers"]["other"], json!({"command": "o"}));
        assert_eq!(c["mcpServers"]["solidcraft"], json!({"command": exe(), "args": ["mcp"]}));

        // Codex (TOML) and Claude Code (a big file the CLI owns) are edited in place too.
        std::fs::create_dir_all(h.join(".codex")).unwrap();
        std::fs::write(h.join(".codex/config.toml"), "# mine\nmodel = \"x\"\n").unwrap();
        let cc = "{\n  \"numStartups\": 3,\n  \"projects\": {}\n}\n";
        std::fs::write(h.join(".claude.json"), cc).unwrap();
        assert!(run(&h, &[]).status.success());
        let toml = std::fs::read_to_string(h.join(".codex/config.toml")).unwrap();
        assert!(toml.starts_with("# mine\nmodel = \"x\"\n") && toml.contains("[mcp_servers.solidcraft]"), "{toml}");
        let c = json_of(&h.join(".claude.json"));
        assert_eq!(c["numStartups"], 3);
        assert_eq!(c["mcpServers"]["solidcraft"], json!({"type": "stdio", "command": exe(), "args": ["mcp"]}));
        assert_eq!(std::fs::read_to_string(h.join(".claude.json.bak")).unwrap(), cc);

        // Running again updates the same entries and leaves one of each.
        assert!(run(&h, &[]).status.success());
        assert_eq!(json_of(&h.join(".cursor/mcp.json"))["mcpServers"].as_object().unwrap().len(), 3);
    }

    #[test]
    fn dry_run_writes_nothing_and_missing_clients_are_skipped() {
        let h = home("dry");
        std::fs::create_dir_all(h.join(".cursor")).unwrap();
        let out = run(&h, &["--dry-run"]);
        assert!(out.status.success());
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("would write") && text.contains("solidcraft"), "{text}");
        assert!(!h.join(".cursor/mcp.json").exists());
        assert!(!h.join(".config/opencode").exists(), "OpenCode was not installed, so nothing is created for it");
        let out = run(&h, &["--client", "claude-desktop"]);
        assert!(String::from_utf8_lossy(&out.stdout).contains("not found"));
    }

    #[test]
    fn a_file_that_is_not_json_is_left_alone() {
        let h = home("bad");
        std::fs::create_dir_all(h.join(".cursor")).unwrap();
        std::fs::write(h.join(".cursor/mcp.json"), "{ // comment").unwrap();
        let out = run(&h, &[]);
        assert!(!out.status.success());
        assert_eq!(std::fs::read_to_string(h.join(".cursor/mcp.json")).unwrap(), "{ // comment");
    }

    #[test]
    fn claude_desktop_msix_folder_is_found() {
        let h = home("msix");
        let pkg = h.join("AppData/Local/Packages/Claude_abc/LocalCache/Roaming/Claude");
        std::fs::create_dir_all(&pkg).unwrap();
        // The MSIX folder is only read through LOCALAPPDATA, set for every platform here.
        assert!(run(&h, &["--client", "claude-desktop"]).status.success());
        assert!(json_of(&pkg.join("claude_desktop_config.json"))["mcpServers"]["solidcraft"].is_object());
    }
}

/// Scratch files belong to the CLI process; exercise real startup, requests and shutdown.
mod scratch_cleanup {
    use super::*;
    use std::path::{Path, PathBuf};

    fn command(case: &str) -> (PathBuf, Command) {
        let dir = std::env::temp_dir().join(format!("solidcraft-mcp-lifecycle-{case}-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli"));
        command
            .args(["mcp", "--headless"])
            .env("TMPDIR", &dir)
            .env("TMP", &dir)
            .env("TEMP", &dir)
            .env("SOLIDCRAFT_RECOVERY_DIR", dir.join("recovery"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        (dir, command)
    }

    fn scratch_folders(dir: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.file_name().unwrap().to_string_lossy().starts_with("solidcraft-mcp-"))
            .collect()
    }

    fn initialize() -> Value {
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}})
    }

    fn run(mut command: Command, request: Value) -> std::process::Output {
        let mut child = command.spawn().unwrap();
        writeln!(child.stdin.take().unwrap(), "{request}").unwrap();
        child.wait_with_output().unwrap()
    }

    #[test]
    fn eof_removes_only_the_process_scratch_folder() {
        let (dir, command) = command("eof");
        let unrelated = dir.join("solidcraft-mcp-unrelated");
        std::fs::create_dir(&unrelated).unwrap();
        let out = run(command, initialize());
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let reply: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert!(reply.get("result").is_some(), "{reply}");
        assert_eq!(scratch_folders(&dir), vec![unrelated]);
        assert!(dir.join("recovery").is_dir(), "cleanup must leave the recovery folder alone");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn startup_error_removes_the_process_scratch_folder() {
        let (dir, mut command) = command("startup-error");
        let out = command.args(["--param-check", "invalid"]).stdin(Stdio::null()).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success());
        assert!(stderr.contains("--param-check wants off, warn or strict"), "{stderr}");
        assert!(scratch_folders(&dir).is_empty(), "startup error left a scratch folder in {dir:?}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn shutdown_preserves_nonempty_scratch_folder() {
        let (dir, mut command) = command("nonempty");
        let mut child = command.spawn().unwrap();
        let mut input = child.stdin.take().unwrap();
        writeln!(input, "{}", initialize()).unwrap();
        let mut reply = String::new();
        BufReader::new(child.stdout.take().unwrap()).read_line(&mut reply).unwrap();
        let reply: Value = serde_json::from_str(&reply).unwrap();
        assert!(reply.get("result").is_some(), "{reply}");
        let scratch = scratch_folders(&dir);
        assert_eq!(scratch.len(), 1);
        let saved = scratch[0].join("keep.txt");
        std::fs::write(&saved, "saved output").unwrap();
        drop(input);
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(std::fs::read_to_string(saved).unwrap(), "saved output");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_screenshot_scratch_folder_is_removed_without_a_path_scope() {
        let (dir, mut command) = command("screenshot");
        command.arg("--allow-any-path");
        // A headless server cannot capture a window. The request still allocates a scratch
        // path before the backend refuses it, without relying on a GPU or installed renderer.
        let out = run(
            command,
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": {"name": "screenshot", "arguments": {"source": "window"}},
            }),
        );
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let reply: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(reply["result"]["isError"], true, "{reply}");
        assert!(reply["result"]["content"][0]["text"].as_str().unwrap().contains("the window gave no picture"), "{reply}");
        assert!(scratch_folders(&dir).is_empty(), "failed screenshot left a scratch folder in {dir:?}");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
