//! Script mode: the tools it lists, what a script builds, and what a script cannot do.

use serde_json::{Value, json};

use super::sandbox::{self, Job};
use crate::{Headless, Options, Server};

fn scx_server() -> Server {
    let mut s = Server::new(Box::new(Headless::default())).with_options(Options { scx: true, ..Default::default() });
    let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}});
    assert!(s.handle_line(&init.to_string()).is_some());
    s
}

/// The reply of a tool call: its text and whether it is an error.
fn call(s: &mut Server, name: &str, args: Value) -> (String, bool) {
    let msg = json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": {"name": name, "arguments": args}});
    let reply: Value = serde_json::from_str(&s.handle_line(&msg.to_string()).unwrap_or_default()).unwrap_or_default();
    let r = &reply["result"];
    let text = r["content"].as_array().into_iter().flatten().filter_map(|c| c["text"].as_str()).collect::<Vec<_>>().join("\n");
    (text, r["isError"] == true)
}

fn run(s: &mut Server, code: &str) -> (String, bool) {
    call(s, "run", json!({"code": code}))
}

#[test]
fn script_mode_lists_three_tools_and_its_own_instructions() {
    let mut s = Server::new(Box::new(Headless::default())).with_options(Options { scx: true, ..Default::default() });
    let init = s.handle_line(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}).to_string()).unwrap_or_default();
    assert!(init.contains("Send Python scripts to `run`"), "{init}");
    let list = s.handle_line(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}).to_string()).unwrap_or_default();
    let list: Value = serde_json::from_str(&list).unwrap_or_default();
    let names: Vec<&str> = list["result"]["tools"].as_array().into_iter().flatten().filter_map(|t| t["name"].as_str()).collect();
    assert_eq!(names, ["run", "design", "look"]);
    // The usual tools are not there to call.
    let (text, _) = call(&mut s, "execute", json!({"command": "solid.box", "params": {}}));
    assert!(text.is_empty(), "{text}");
}

#[test]
fn off_by_default() {
    let list = crate::tool_definitions_with(Options::default());
    assert!(list.iter().all(|t| t["name"] != "run"));
    assert!(list.iter().any(|t| t["name"] == "execute"));
}

#[test]
fn a_script_builds_and_the_reply_says_what_changed() {
    let mut s = scx_server();
    let (text, err) = run(&mut s, "box(60, 40, 8, name='Plate')\nfor x, y in [(8, 8), (52, 8), (8, 32), (52, 32)]:\n    hole(at=(x, y, 8), d=6)");
    assert!(!err, "{text}");
    assert!(text.contains("+ Plate: vol 18295.2"), "{text}");
    assert!(text.contains("bodies now: 1"), "{text}");
    let (text, err) = run(&mut s, "with part('Lid'):\n    box(60, 40, 2, at=(0, 0, 20), name='LidPlate')\nsorted(measure())");
    assert!(!err, "{text}");
    assert!(text.contains(r#"["LidPlate","Plate"]"#), "the last value is printed: {text}");
    assert!(text.contains("part Lid") && text.contains("parts: 0 -> 1"), "{text}");
    let (text, err) = call(&mut s, "design", json!({}));
    assert!(!err, "{text}");
    assert!(text.starts_with("2 bodies, 1 parts"), "{text}");
    assert!(text.contains("part Lid: LidPlate"), "{text}");
}

#[test]
fn errors_name_the_line_and_keep_what_was_built() {
    let mut s = scx_server();
    let (text, err) = run(&mut s, "box(10, 10, 10, name='A')\nbox(1, 1, 1, op='bogus')");
    assert!(err);
    assert!(text.contains("error at line 2: op must be one of"), "{text}");
    assert!(text.contains("+ A: vol 1000"), "steps before the error stay: {text}");
    let (text, err) = run(&mut s, "A.volume");
    assert!(err);
    assert!(text.contains("hint: A is a body name, so quote it"), "{text}");
    let (text, _) = run(&mut s, "cmd('solid.nonsense')");
    assert!(text.contains("unknown command"), "{text}");
}

#[test]
fn new_starts_an_empty_design() {
    let mut s = scx_server();
    run(&mut s, "box(10, 10, 10)");
    let (text, err) = call(&mut s, "run", json!({"code": "measure()", "new": true}));
    assert!(!err, "{text}");
    assert!(text.starts_with("Started a new, empty design.\n{}"), "{text}");
}

#[test]
fn scripts_cannot_reach_the_host() {
    let mut s = scx_server();
    for (code, says) in [
        ("open('Cargo.toml').read()", "PermissionError"),
        ("import os\nos.listdir('.')", "PermissionError"),
        ("import os\nos.getenv('PATH')", "PermissionError"),
        ("import subprocess", ""),
        ("import socket", ""),
        ("__import__('os')", "not available"),
        ("_call('screenshot', '{}')", "not one of the tools a script can call"),
        ("_call('execute')", "not available"),
        ("eval('__import__')('os')", ""),
        ("(1).__class__.__mro__", "AttributeError"),
        ("[].__class__.__base__.__subclasses__()", "AttributeError"),
    ] {
        let (text, err) = run(&mut s, code);
        assert!(err, "{code} ran: {text}");
        assert!(text.contains(says), "{code}: {text}");
        assert!(text.contains("no geometry changed"), "{code}: {text}");
    }
}

#[test]
fn limits_end_a_script_not_the_server() {
    let mut s = scx_server();
    // Allocating without end is the worker's to stop, with its memory cap (apps/solidcraft-cli/tests/mcp_scx.rs).
    let (text, err) = run(&mut s, "def f(n):\n    return f(n + 1)\nf(0)");
    assert!(err && text.contains("RecursionError"), "{text}");
    let (text, err) = run(&mut s, "while True:\n    print('spam' * 100)");
    assert!(err);
    assert!(text.len() < 20_000, "printing is capped in the reply: {} bytes", text.len());
    assert!(text.contains("more characters"), "{}", &text[text.len().saturating_sub(400)..]);
    let (text, err) = run(&mut s, "box(1, 1, 1)");
    assert!(!err, "the server still works: {text}");
}

#[test]
fn the_worker_protocol_runs_a_job_over_pipes() {
    // What `solidcraft-cli scx-worker` does, with the server's answers scripted.
    let job = json!({"job": "run", "code": "print(_call('measure', '{}'))\n41 + 1", "new": null}).to_string() + "\n";
    // _begin asks measure (summary) and component.list; the script asks measure; _end the same again.
    let measure = json!({"ok": "{\"bodies\": []}"}).to_string();
    let parts = json!({"ok": "{\"components\": []}"}).to_string();
    let answers = [&measure, &parts, &measure, &measure, &parts].map(|a| format!("{a}\n")).concat();
    let mut out = Vec::new();
    let input = std::io::Cursor::new(format!("{job}{answers}"));
    assert!(sandbox::worker(input, &mut out).is_ok());
    let out = String::from_utf8(out).unwrap_or_default();
    let last: Value = serde_json::from_str(out.lines().last().unwrap_or_default()).unwrap_or_default();
    let text = last["done"].as_str().unwrap_or_default();
    assert!(text.contains("{\"bodies\": []}\n42"), "{out}");
    assert_eq!(last["error"], false);
    assert!(out.lines().next().is_some_and(|l| l.contains(r#""call":["measure""#)), "{out}");
}

#[test]
fn a_design_job_needs_no_script() {
    let mut host = |tool: &str, _: &str| -> Result<String, String> {
        Ok(match tool {
            "measure" => json!({"bodies": [{"name": "B", "volume_mm3": 8.0, "bbox": {"min": [0, 0, 0], "max": [2, 2, 2]}, "faces": 6}]}).to_string(),
            _ => json!({"components": []}).to_string(),
        })
    };
    let o = sandbox::execute(&Job::Design, &mut host);
    assert!(!o.error, "{}", o.text);
    assert!(o.text.starts_with("1 bodies, 0 parts, total volume 8"), "{}", o.text);
}

/// A worker's line past the limit stops the script with a `fail`, read in bounded memory; the
/// lines before it pass through.
#[test]
fn a_worker_line_past_the_limit_stops_the_script() {
    let (tx, rx) = std::sync::mpsc::channel();
    sandbox::forward_lines(
        std::io::Cursor::new(
            b"{\"call\":1}
0123456789012345678901234
{}
"
            .to_vec(),
        ),
        &tx,
        16,
    );
    assert_eq!(
        rx.recv().unwrap().as_deref(),
        Some(
            "{\"call\":1}
"
        )
    );
    let fail: Value = serde_json::from_str(&rx.recv().unwrap().unwrap()).unwrap();
    assert!(fail["fail"].as_str().is_some_and(|f| f.contains("longer than 16 bytes")), "{fail}");
    assert!(rx.try_recv().is_err(), "nothing is read after it");
    let (tx, rx) = std::sync::mpsc::channel();
    sandbox::forward_lines(
        std::io::Cursor::new(
            b"{}
"
            .to_vec(),
        ),
        &tx,
        16,
    );
    assert_eq!(
        rx.recv().unwrap().as_deref(),
        Some(
            "{}
"
        )
    );
    assert_eq!(rx.recv().unwrap(), None, "the end of the output");
}
