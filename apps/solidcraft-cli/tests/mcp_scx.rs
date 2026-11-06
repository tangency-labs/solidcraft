//! `mcp --scx` (script mode): scripts run in a `scx-worker` child, so one that allocates without
//! end ends that child, not the server, which goes on answering.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

#[test]
fn scripts_run_in_a_capped_worker_and_the_server_outlives_them() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli"))
        // --headless: never bridge to an app the developer has running.
        .args(["mcp", "--headless", "--allow-any-path", "--scx"])
        .env_remove("SOLIDCRAFT_MCP_SCX")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut id = 0;
    let mut ask = |method: &str, params: Value| -> Value {
        id += 1;
        writeln!(stdin, "{}", json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})).unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    };
    let tools = ask("tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
    assert_eq!(names, ["run", "design", "look"]);
    let mut run = |code: &str| -> (String, bool) {
        let r = ask("tools/call", json!({"name": "run", "arguments": {"code": code}}));
        let text = r["result"]["content"][0]["text"].as_str().unwrap_or_default().to_owned();
        (text, r["result"]["isError"] == true)
    };
    let (text, err) = run("box(10, 10, 10, name='A')");
    assert!(!err && text.contains("+ A: vol 1000"), "{text}");
    for code in ["l = []\nwhile True:\n    l.append('x' * 1000000)", "s = 'a' * (10**10)", "x = [0] * (10**10)"] {
        let (text, err) = run(code);
        assert!(err, "{code} ran: {text}");
        assert!(text.contains("MemoryError") || text.contains("ran out of memory"), "{code}: {text}");
    }
    let (text, err) = run("open('Cargo.toml').read()");
    assert!(err && text.contains("PermissionError"), "{text}");
    let (text, err) = run("box(5, 5, 5, at=(20, 0, 0), name='B')");
    assert!(!err && text.contains("bodies now: 2"), "the server and the design outlive the scripts: {text}");
    drop(ask);
    let _ = child.kill();
    let _ = child.wait();
}
