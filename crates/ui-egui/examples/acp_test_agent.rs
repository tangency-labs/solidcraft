//! A scripted ACP agent for trying the agent chat without an account (docs/agent-chat.md):
//! it speaks the Agent Client Protocol on stdin and stdout like the real adapters, starts the MCP
//! server the chat gives it (`solidcraft-cli mcp --connect …`) and models through it. No model
//! runs: what it does is fixed.
//!
//! ```text
//! cargo build --release -p solidcraft-ui-egui --example acp_test_agent
//! # Preferences → General → Agent chat → Add custom command: target/release/examples/acp_test_agent
//! ```
//!
//! A message: a plan, a box made with one `batch`, a fillet, a measurement. A message with "save"
//! in it only saves to the user's Documents folder (which the chat holds for Allow or Deny);
//! with "ask" in it the agent first asks its own permission for the batch.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

fn send(v: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

fn update(session: &str, u: Value) {
    send(&json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": session, "update": u}}));
}

fn say(session: &str, text: &str) {
    update(session, json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}}));
}

fn tool(session: &str, id: &str, title: &str, kind: &str, status: &str, output: Option<&str>) {
    let mut u = json!({"sessionUpdate": "tool_call_update", "toolCallId": id, "title": title, "kind": kind, "status": status});
    if status == "pending" {
        u["sessionUpdate"] = json!("tool_call");
    }
    if let Some(o) = output {
        u["content"] = json!([{"type": "content", "content": {"type": "text", "text": o}}]);
    }
    update(session, u);
}

fn plan(session: &str, done: usize) {
    let steps = ["Make a 40 × 30 × 20 mm block", "Round its vertical edges", "Measure it"];
    let entries: Vec<Value> = steps
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let status = if i < done {
                "completed"
            } else if i == done {
                "in_progress"
            } else {
                "pending"
            };
            json!({"content": s, "priority": "medium", "status": status})
        })
        .collect();
    update(session, json!({"sessionUpdate": "plan", "entries": entries}));
}

/// The MCP server the chat handed over, as a child speaking JSON-RPC lines.
struct Mcp {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: u64,
}

impl Mcp {
    fn start(server: &Value) -> Option<Mcp> {
        let program = server.get("command")?.as_str()?;
        let args: Vec<String> = server.get("args")?.as_array()?.iter().filter_map(|a| a.as_str().map(str::to_string)).collect();
        let mut child = Command::new(program).args(&args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
        let stdin = child.stdin.take()?;
        let stdout = BufReader::new(child.stdout.take()?);
        let mut m = Mcp { child, stdin, stdout, next: 0 };
        m.call("initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "acp-test-agent", "version": "1"}}))?;
        let _ = writeln!(m.stdin, "{}", json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        Some(m)
    }

    fn call(&mut self, method: &str, params: Value) -> Option<Value> {
        self.next += 1;
        let id = self.next;
        writeln!(self.stdin, "{}", json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})).ok()?;
        self.stdin.flush().ok()?;
        loop {
            let mut line = String::new();
            if self.stdout.read_line(&mut line).ok()? == 0 {
                return None;
            }
            let v: Value = serde_json::from_str(line.trim()).ok()?;
            if v.get("id").and_then(Value::as_u64) == Some(id) {
                return Some(v);
            }
        }
    }

    /// A tool's text and whether it failed.
    fn tool(&mut self, name: &str, args: Value) -> (String, bool) {
        match self.call("tools/call", json!({"name": name, "arguments": args})) {
            Some(r) => {
                let text = r.pointer("/result/content/0/text").and_then(Value::as_str).unwrap_or("").to_string();
                let failed = r.pointer("/result/isError").and_then(Value::as_bool).unwrap_or(false) || r.get("error").is_some();
                let text = if text.is_empty() { r.get("error").map(Value::to_string).unwrap_or_default() } else { text };
                (text.chars().take(600).collect(), failed)
            }
            None => ("the MCP server did not answer".into(), true),
        }
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn main() {
    let session = "test-session-1";
    let mut mcp: Option<Mcp> = None;
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    while let Some(Ok(line)) = lines.next() {
        let Ok(msg) = serde_json::from_str::<Value>(line.trim()) else { continue };
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        let reply = |result: Value| send(&json!({"jsonrpc": "2.0", "id": id, "result": result}));
        match msg.get("method").and_then(Value::as_str) {
            Some("initialize") => reply(json!({
                "protocolVersion": 1,
                "agentInfo": {"name": "acp-test-agent", "title": "SolidCraft test agent", "version": env!("CARGO_PKG_VERSION")},
                "agentCapabilities": {"promptCapabilities": {"image": true}},
                "authMethods": [],
            })),
            Some("session/new") => {
                mcp = msg.pointer("/params/mcpServers/0").and_then(Mcp::start);
                reply(json!({"sessionId": session, "modes": {"currentModeId": "default", "availableModes": [
                    {"id": "default", "name": "Default"}, {"id": "acceptEdits", "name": "Accept edits"}, {"id": "plan", "name": "Plan"},
                ]}}));
            }
            Some("session/prompt") => {
                let text: String = msg
                    .pointer("/params/prompt")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|b| b.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join(" "))
                    .unwrap_or_default()
                    .to_lowercase();
                let images = msg.pointer("/params/prompt").and_then(Value::as_array).map_or(0, |a| a.iter().filter(|b| b["type"] == "image").count());
                let Some(m) = mcp.as_mut() else {
                    say(session, "I have no SolidCraft tools in this session.");
                    reply(json!({"stopReason": "end_turn"}));
                    continue;
                };
                // "save": only the save (the design is there already).
                if text.contains("save") {
                    let path = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default().join("Documents/block.sc");
                    let title = format!("save {}", path.display());
                    say(session, "I'll save the design to your Documents folder.");
                    tool(session, "t4", &title, "edit", "in_progress", None);
                    let (out, failed) = m.tool("save", json!({"path": path.to_string_lossy()}));
                    tool(session, "t4", &title, "edit", if failed { "failed" } else { "completed" }, Some(&out));
                    say(session, if failed { "\nThe save didn't go through." } else { "\nSaved." });
                    reply(json!({"stopReason": "end_turn"}));
                    continue;
                }
                plan(session, 0);
                if images > 0 {
                    say(session, "Thanks for the picture of the view. ");
                }
                say(session, "I'll make a 40 × 30 × 20 mm block with one batch, round its vertical edges, then measure it.");
                if text.contains("ask") {
                    tool(session, "t1", "batch: solid.box 40 × 30 × 20", "edit", "pending", None);
                    send(&json!({"jsonrpc": "2.0", "id": 900, "method": "session/request_permission", "params": {
                        "sessionId": session,
                        // Named as the Claude Code adapter names an MCP tool.
                        "toolCall": {"toolCallId": "t1", "title": "mcp__solidcraft__batch"},
                        "options": [{"optionId": "allow", "name": "Allow", "kind": "allow_once"}, {"optionId": "reject", "name": "Reject", "kind": "reject_once"}],
                    }}));
                    // Wait for the answer (other lines are not expected meanwhile).
                    let mut allowed = false;
                    for l in lines.by_ref().map_while(Result::ok) {
                        let Ok(a) = serde_json::from_str::<Value>(l.trim()) else { continue };
                        if a.get("id").and_then(Value::as_u64) == Some(900) {
                            allowed = a.pointer("/result/outcome/optionId").and_then(Value::as_str) == Some("allow");
                            break;
                        }
                    }
                    if !allowed {
                        tool(session, "t1", "batch: solid.box 40 × 30 × 20", "edit", "failed", Some("rejected"));
                        say(session, "\nOK, I left the design as it was.");
                        reply(json!({"stopReason": "end_turn"}));
                        continue;
                    }
                }
                tool(session, "t1", "batch: solid.box 40 × 30 × 20", "edit", "in_progress", None);
                let (out, failed) = m.tool(
                    "batch",
                    json!({"commands": [{"command": "solid.box", "params": {"length": 40, "width": 30, "height": 20, "corner": [0, 0, 0], "body_name": "Block"}}], "verbose": false}),
                );
                tool(session, "t1", "batch: solid.box 40 × 30 × 20", "edit", if failed { "failed" } else { "completed" }, Some(&out));
                plan(session, 1);
                // The block's vertical edges, by a point on each (its corner is at the origin).
                let vertical = json!([[0, 0, 10], [40, 0, 10], [40, 30, 10], [0, 30, 10]]);
                tool(session, "t2", "execute: solid.fillet r = 4 mm", "edit", "in_progress", None);
                let (out, failed) =
                    m.tool("execute", json!({"command": "solid.fillet", "params": {"edges": vertical, "radius": 4}, "verbose": false}));
                tool(session, "t2", "execute: solid.fillet r = 4 mm", "edit", if failed { "failed" } else { "completed" }, Some(&out));
                plan(session, 2);
                tool(session, "t3", "measure", "read", "in_progress", None);
                let (out, failed) = m.tool("measure", json!({}));
                tool(session, "t3", "measure", "read", if failed { "failed" } else { "completed" }, Some(&out));
                plan(session, 3);
                say(session, "\nDone: the block is 40 × 30 × 20 mm with 4 mm rounds on its vertical edges.");
                reply(json!({"stopReason": "end_turn"}));
            }
            Some("session/set_mode") => reply(json!({})),
            Some("session/cancel") => {}
            Some(_) if !id.is_null() => send(&json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "method not found"}})),
            _ => {}
        }
    }
}
