//! `bench-assistant`: the built-in v1 test or public format-1 sets, run against a desktop app.
//!
//! The app is already running with `--control PORT`; everything goes through its control channel
//! (`docs/control-protocol.md`), so the command needs no window code of its own. For each task it
//! starts a new design, runs the task's prescribed setup (a box, for some), adds the endpoint as an
//! API agent (or selects an installed coding agent) with All permissions, starts a new chat, sends the prompt once and waits for the turn
//! to end. The result is checked on the design (`document.inspect` with measurements), never on
//! what the model says, except for the one question whose answer is the reply. A failed task is
//! put in one of the issue's failure categories from the conversation's items.
//!
//! ```text
//! solidcraft-cli bench-assistant --connect 127.0.0.1:PORT --base-url URL --model NAME
//!     [--kind openai|anthropic] [--key-env VAR] (or --agent ID [--agent-model ID] instead of API options)
//!     [--token T] [--tasks 1,2,5] [--repeat N] [--timeout-s 300]
//!     [--json out.json] [--markdown out.md]
//! ```
//! prints a table of the results; `--markdown` writes the issue's comment template filled in,
//! `--json` every task's rules, conversation and design before and after.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

mod sets;

/// Standard test v1, as #4851 states it. Changing a prompt or a rule makes it another test.
const TASKS: &str = r#"[
  {"id": "01-cube", "prompt": "Make a 20 mm cube.", "check": {"bodies": 1, "volume": 8000, "bbox_size": [20, 20, 20]}},
  {"id": "02-cylinder", "prompt": "Make a cylinder 30 mm in diameter and 50 mm tall.", "check": {"bodies": 1, "volume": 35342.92, "bbox_size": [30, 30, 50]}},
  {"id": "03-block", "prompt": "Create a block 60 mm long, 40 mm wide and 10 mm thick.", "check": {"bodies": 1, "volume": 24000, "bbox_size": [10, 40, 60]}},
  {"id": "04-sketch-extrude", "prompt": "Sketch a 50 by 30 mm rectangle on the XY plane and extrude it 15 mm.", "check": {"bodies": 1, "volume": 22500, "bbox_size": [15, 30, 50], "feature_kinds": ["Extrude"]}},
  {"id": "05-plate-hole", "prompt": "Make a plate 80 x 60 x 10 mm with a 20 mm diameter hole straight through the middle.", "check": {"bodies": 1, "volume": 44858.41, "bbox_size": [10, 60, 80], "note": "80*60*10 - pi*10^2*10"}},
  {"id": "06-fillet-box", "prompt": "Round all the edges of the box with a 3 mm radius.", "setup": [{"command": "solid.box", "params": {"length": 40, "width": 40, "height": 40}}], "check": {"bodies": 1, "volume": 63108.53, "tol": 0.003, "feature_kinds": ["Fillet"], "note": "all 12 edges of a 40 mm cube rounded r=3"}},
  {"id": "07-move-body", "prompt": "Move the box 50 mm along the X axis.", "setup": [{"command": "solid.box", "params": {"length": 20, "width": 20, "height": 20}}], "check": {"bodies": 1, "volume": 8000, "bbox_min": [50, 0, 0]}},
  {"id": "08-rename-body", "prompt": "Rename the body to Bracket.", "setup": [{"command": "solid.box", "params": {"length": 30, "width": 20, "height": 10}}], "check": {"bodies": 1, "volume": 6000, "body_name": "Bracket"}},
  {"id": "09-volume-question", "prompt": "What is the volume of the part?", "setup": [{"command": "solid.box", "params": {"length": 30, "width": 20, "height": 10}}], "check": {"bodies": 1, "volume": 6000, "unchanged": true, "reply_number": "6[ ,. ]?000(?![0-9])|6(\\.0+)? ?cm"}},
  {"id": "10-two-holes", "prompt": "Make a 100 x 50 x 5 mm plate and drill two 10 mm holes through it, each 15 mm in from the short ends and centred across the width.", "check": {"bodies": 1, "volume": 24214.6, "bbox_size": [5, 50, 100], "note": "100*50*5 - 2*pi*5^2*5"}},
  {"id": "11-any-change", "prompt": "can you use the software to make a change to the current shape, any change at all", "setup": [{"command": "solid.box", "params": {"length": 30, "width": 20, "height": 10}}], "check": {"changed": true, "note": "verbatim user prompt from #4846; pass = the design changed"}}
]"#;

/// The agent the test adds. A fixed id replaces it on every run instead of adding another.
const AGENT_ID: &str = "api-bench-assistant";
const AGENT_NAME: &str = "bench";
/// Longest control reply read (the channel's own request limit).
const MAX_REPLY: usize = 16 << 20;
/// Most result data kept for `--json`.
const MAX_RESULTS: usize = 64 << 20;
/// Deadline of the requests that only set up or read (each is answered at once by the app).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
/// How often a running turn is polled.
const POLL: Duration = Duration::from_millis(250);
/// After this long, an idle chat with no reply yet counts as a turn that ended without one.
const START_GRACE: Duration = Duration::from_secs(30);
/// Absolute tolerance of the lowest corner, in millimetres.
const MIN_CORNER_TOL: f64 = 0.05;

/// A JSON-lines connection to the app. Requests are sent once; a lost reply is never retried,
/// so nothing runs twice.
struct Control {
    addr: SocketAddr,
    token: String,
    reader: BufReader<TcpStream>,
    id: u64,
}

impl Control {
    fn connect(addr: &str, token: Option<&str>) -> Result<Self, String> {
        let addr: SocketAddr = addr.parse().map_err(|_| format!("--connect: `{addr}` is not an IP address and port"))?;
        if !addr.ip().is_loopback() || addr.port() == 0 {
            return Err("--connect requires a loopback address and a nonzero port (the app listens on 127.0.0.1)".into());
        }
        let token = solidcraft_engine::control_auth::client_token(addr.port(), token)
            .map(|t| t.token)
            .ok_or("no control token: pass --token, set SOLIDCRAFT_CONTROL_TOKEN, or run on the app's computer (its token file)")?;
        if token.len() > 4096 {
            return Err("the control token is longer than 4096 bytes".into());
        }
        Self::open(addr, token)
    }

    fn open(addr: SocketAddr, token: String) -> Result<Self, String> {
        let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).map_err(|e| format!("connecting to {addr}: {e}"))?;
        Ok(Self { addr, token, reader: BufReader::new(stream), id: 0 })
    }

    /// A new connection to the same app: a request that outlived its deadline keeps the old one
    /// busy until it is answered.
    fn reopen(&self) -> Result<Self, String> {
        Self::open(self.addr, self.token.clone())
    }

    fn call(&mut self, method: &str, params: Value, deadline: Instant) -> Result<Value, String> {
        let left = || deadline.checked_duration_since(Instant::now()).filter(|d| !d.is_zero()).ok_or_else(|| "timeout".to_string());
        let io = |e: std::io::Error| match e.kind() {
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => "timeout".to_string(),
            _ => format!("control channel: {e}"),
        };
        self.reader.get_ref().set_write_timeout(Some(left()?)).map_err(io)?;
        self.id += 1;
        // The first request on a connection carries the token; sending it on every one is allowed.
        let request = json!({"id": self.id, "method": method, "params": params, "token": self.token});
        writeln!(self.reader.get_mut(), "{request}").map_err(io)?;
        let mut line = Vec::new();
        loop {
            self.reader.get_ref().set_read_timeout(Some(left()?)).map_err(io)?;
            let bytes = self.reader.fill_buf().map_err(io)?;
            if bytes.is_empty() {
                return Err("the app closed the control connection".into());
            }
            let newline = bytes.iter().position(|b| *b == b'\n');
            let used = newline.map_or(bytes.len(), |n| n + 1);
            if line.len() + used > MAX_REPLY {
                return Err("a control reply is larger than 16 MiB".into());
            }
            line.extend_from_slice(bytes.get(..used).unwrap_or_default());
            self.reader.consume(used);
            if newline.is_some() {
                break;
            }
        }
        let reply: Value = serde_json::from_slice(&line).map_err(|e| format!("control reply: {e}"))?;
        if reply["id"].as_u64() != Some(self.id) {
            return Err("control reply for another request".into());
        }
        if reply["ok"] != true {
            return Err(format!("{method}: {}", reply["error"].as_str().unwrap_or("failed")));
        }
        reply.get("result").cloned().ok_or_else(|| format!("{method}: reply without a result"))
    }

    fn execute(&mut self, command: &str, params: Value) -> Result<Value, String> {
        self.call("engine.execute", json!({"command": command, "params": params, "animate": false}), Instant::now() + REQUEST_TIMEOUT)
    }

    fn chat(&mut self, params: Value) -> Result<Value, String> {
        self.call("ui.agent_chat", params, Instant::now() + REQUEST_TIMEOUT)
    }

    fn inspect(&mut self) -> Result<Value, String> {
        let design = self.call("document.inspect", json!({"measure": true}), Instant::now() + REQUEST_TIMEOUT)?;
        if design.is_object() { Ok(design) } else { Err("document.inspect returned no design".into()) }
    }
}

fn tasks() -> Result<Vec<Value>, String> {
    serde_json::from_str(TASKS).map_err(|e| format!("embedded tasks: {e}"))
}

fn triple(value: &Value) -> Option<[f64; 3]> {
    match value.as_array()?.as_slice() {
        [x, y, z] => {
            let n = |v: &Value| v.as_f64().filter(|n| n.is_finite());
            Some([n(x)?, n(y)?, n(z)?])
        }
        _ => None,
    }
}

/// What the model said (thoughts left out).
fn reply(items: &[Value]) -> String {
    items.iter().filter(|i| i["thought"] != true).filter_map(|i| i["agent"].as_str()).collect::<Vec<_>>().join("\n")
}

/// v1's reply pattern `6[ ,. ]?000(?![0-9])|6(\.0+)? ?cm`: "6000", "6 000", "6,000", "6.000" not
/// followed by a digit, or "6 cm", "6.0cm", … (a narrow no-break space also separates thousands).
fn volume_reply(text: &str) -> bool {
    text.match_indices('6').any(|(at, _)| {
        let tail = text.get(at + 1..).unwrap_or_default();
        let thousands = |t: &str| t.strip_prefix("000").is_some_and(|rest| !rest.starts_with(|c: char| c.is_ascii_digit()));
        let separated = tail.strip_prefix([' ', ',', '.', '\u{202f}']).is_some_and(thousands);
        let cm = {
            let t = tail.strip_prefix(".0").map_or(tail, |t| t.trim_start_matches('0'));
            t.strip_prefix(' ').unwrap_or(t).starts_with("cm")
        };
        thousands(tail) || separated || cm
    })
}

/// The design as the user sees it, without what reading or saving changes (path, saved state,
/// revision, selection, undo depth, evaluation timings).
fn content(design: &Value) -> Value {
    let mut v = design.clone();
    if let Some(o) = v.as_object_mut() {
        for key in ["path", "dirty", "revision", "selection", "undo", "redo"] {
            o.remove(key);
        }
    }
    for feature in v.get_mut("timeline").and_then(Value::as_array_mut).into_iter().flatten() {
        if let Some(o) = feature.as_object_mut() {
            o.remove("ms");
            o.remove("boolean_diagnostics");
        }
    }
    v
}

/// Whether the design changed: its content, or a step on the undo stack (a colour, say, that the
/// inspection does not list).
fn changed(before: &Value, after: &Value) -> bool {
    content(before) != content(after) || after["undo"].as_u64() > before["undo"].as_u64()
}

/// The rules a task's result breaks (none: it passed). `before` and `after` are
/// `document.inspect {measure: true}` before the prompt and after the turn.
pub(super) fn check(rule: &Value, before: &Value, after: &Value, items: &[Value]) -> Vec<String> {
    let Some(all) = after["bodies"].as_array() else { return vec!["the design could not be read".into()] };
    // Bodies the user sees; a hidden one (a tool body, a hidden original) is not the result.
    let bodies: Vec<&Value> = all.iter().filter(|b| b["visible"] != false).collect();
    let mut errors = Vec::new();
    if let Some(n) = rule["bodies"].as_u64()
        && bodies.len() as u64 != n
    {
        errors.push(format!("{} bodies, expected {n}", bodies.len()));
    }
    if let Some(expected) = rule["volume"].as_f64() {
        let tol = rule["tol"].as_f64().unwrap_or(0.01);
        let volumes: Vec<f64> = bodies.iter().filter_map(|b| b["volume_mm3"].as_f64()).filter(|v| v.is_finite()).collect();
        let total: f64 = volumes.iter().sum();
        if volumes.len() != bodies.len() || (total - expected).abs() > expected * tol {
            errors.push(format!("volume {total:.1} mm³, expected {expected} mm³ ±{}%", tol * 100.0));
        }
    }
    let first = bodies.first().copied().unwrap_or(&Value::Null);
    let min = triple(&first["bbox"]["min"]);
    if let Some(mut expected) = triple(&rule["bbox_size"]) {
        expected.sort_by(f64::total_cmp);
        let size = min.zip(triple(&first["bbox"]["max"])).map(|(lo, hi)| {
            let mut size = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
            size.sort_by(f64::total_cmp);
            size
        });
        if !size.is_some_and(|s| s.iter().zip(expected).all(|(a, e)| (a - e).abs() <= 0.01 * e.max(1.0))) {
            errors.push(format!("size {size:?}, expected {expected:?} in any orientation"));
        }
    }
    if let Some(expected) = triple(&rule["bbox_min"])
        && !min.is_some_and(|m| m.iter().zip(expected).all(|(a, e)| (a - e).abs() <= MIN_CORNER_TOL))
    {
        errors.push(format!("lowest corner {min:?}, expected {expected:?}"));
    }
    if let Some(name) = rule["body_name"].as_str()
        && !bodies.iter().any(|b| b["name"] == name)
    {
        errors.push(format!("no body named {name}"));
    }
    for kind in rule["feature_kinds"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        let live = |f: &&Value| f["suppressed"] != true && f["rolled_back"] != true && f["error"].is_null();
        if !after["timeline"].as_array().into_iter().flatten().filter(live).any(|f| f["type"].as_str().is_some_and(|t| t.contains(kind))) {
            errors.push(format!("no {kind} in the timeline"));
        }
    }
    if rule["changed"] == true || rule["unchanged"] == true {
        let changed = before.is_object() && changed(before, after);
        if rule["changed"] == true && !changed {
            errors.push("the design did not change".into());
        }
        if rule["unchanged"] == true && (changed || !before.is_object()) {
            errors.push("the design changed".into());
        }
    }
    if rule["reply_number"].is_string() && !volume_reply(&reply(items)) {
        errors.push("the reply does not give the volume".into());
    }
    errors
}

/// A tool call written into the reply as JSON: `{"name": …`, `{"command": …`, `{"function": …`,
/// `{"tool": …`.
fn call_as_text(text: &str) -> bool {
    text.match_indices('{').any(|(at, _)| {
        let rest = text.get(at + 1..).unwrap_or_default().trim_start();
        ["\"name\"", "\"command\"", "\"function\"", "\"tool\""]
            .iter()
            .any(|key| rest.strip_prefix(key).is_some_and(|r| r.trim_start().starts_with(':')))
    })
}

/// How a failed task failed, in the issue's words, from the conversation's items.
pub(super) fn classify(items: &[Value], timed_out: bool) -> &'static str {
    if timed_out {
        return "timeout";
    }
    let tools: Vec<&Value> = items.iter().filter(|i| i.get("tool").is_some()).collect();
    // A failed call, or a batch whose answer reports a failed step (`"ok": false`).
    let failed: Vec<String> = tools
        .iter()
        .map(|t| t["output"].as_str().unwrap_or_default())
        .zip(&tools)
        .filter(|(output, t)| t["status"] == "Failed" || serde_json::from_str::<Value>(output).is_ok_and(|v| v["ok"] == false))
        .map(|(output, _)| output.to_lowercase())
        .collect();
    let notices: Vec<String> = items.iter().filter_map(|i| i["notice"].as_str()).map(str::to_lowercase).collect();
    let answer = reply(items);
    if notices.iter().any(|n| n.contains("http 400")) {
        return "the endpoint refused tools (HTTP 400)";
    }
    if notices.iter().any(|n| n.contains("max_turn_requests")) || answer.contains("requests for one message") {
        return "hit the request limit";
    }
    if call_as_text(&answer) {
        return "wrote the call as text";
    }
    if failed.iter().any(|o| o.contains("unknown command") || o.starts_with("unknown tool")) {
        return "wrong or unknown command";
    }
    if failed.iter().any(|o| o.contains("invalid parameters") || o.starts_with("not run:") || o.contains("not valid json")) {
        return "wrong arguments";
    }
    if !failed.is_empty() || notices.iter().any(|n| n.starts_with("http ")) || items.iter().any(|i| i["error"] == true) {
        return "gave up after errors";
    }
    if tools.is_empty() {
        return if answer.trim().is_empty() { "an empty reply" } else { "only talked" };
    }
    "wrong result"
}

/// One run's results in the issue's comment template (`run`: which repetition).
pub(super) fn markdown(model: &str, version: &str, results: &[Value], run: u64) -> String {
    let clean = |s: &str| s.replace(['\n', '\r', '|'], " ");
    let rows: Vec<&Value> = results.iter().filter(|r| r["repeat"] == run).collect();
    let passed = rows.iter().filter(|r| r["passed"] == true).count();
    let mut out = format!(
        "### {} (<quant>, <download size>)\nRuntime: <Ollama/LM Studio/... version>, context <num_ctx if changed>\nSolidCraft: {}\nResult: {passed}/11\n| # | pass/fail | how it failed |\n|---|---|---|\n",
        clean(model),
        clean(version)
    );
    for n in 1..=11 {
        match rows.iter().find(|r| r["task"] == n) {
            Some(r) if r["passed"] == true => out.push_str(&format!("| {n} | pass | |\n")),
            Some(r) => out.push_str(&format!("| {n} | fail | {} |\n", clean(r["failure"].as_str().unwrap_or_default()))),
            None => out.push_str(&format!("| {n} | | not run |\n")),
        }
    }
    out.push_str(if rows.len() == 11 {
        "Notes: <anything odd, e.g. very slow, needed a retry>\n"
    } else {
        "Notes: partial run; the tasks marked \"not run\" were left out.\n"
    });
    out
}

/// Agent reports have measured metadata rather than the API comment template.
fn report_markdown(route: &Value, version: &str, results: &[Value], run: u64) -> String {
    if route["agent"].is_null() {
        return markdown(&route_label(route), version, results, run);
    }
    let clean = |s: &str| s.replace(['\n', '\r', '|'], " ");
    let template = markdown("", version, results, run);
    let table = template.find("| # |").unwrap_or(template.len());
    let end = template.rfind("Notes:").unwrap_or(template.len());
    let passed = results.iter().filter(|r| r["repeat"] == run && r["passed"] == true).count();
    format!(
        "### {} {}, model {}\nSolidCraft: {}\nResult: {passed}/11\n{}",
        clean(route["name"].as_str().or(route["agent"].as_str()).unwrap_or("unknown")),
        clean(route["version"].as_str().unwrap_or("unknown")),
        clean(route["model"].as_str().filter(|s| !s.is_empty()).unwrap_or("unknown")),
        clean(version),
        &template[table..end]
    )
}

fn failure_detail(items: &[Value], timed_out: bool) -> String {
    if !timed_out {
        let failed: Vec<_> = items
            .iter()
            .filter(|i| i.get("tool").is_some() && i["status"] == "Failed")
            .map(|i| format!("tool call failed: {} (status Failed)", i["title"].as_str().unwrap_or("unknown")))
            .collect();
        if !failed.is_empty() {
            return failed.join("; ");
        }
    }
    classify(items, timed_out).into()
}

/// Unique empty workspace, retained until the task's agent has stopped, including on errors.
struct AgentCwd(std::path::PathBuf);
impl AgentCwd {
    fn new() -> Result<Self, String> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        for _ in 0..100 {
            let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
            let path = std::env::temp_dir().join(format!("solidcraft-bench-{}-{stamp}-{n}", std::process::id()));
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("benchmark workspace: {e}")),
            }
        }
        Err("could not create a unique benchmark workspace".into())
    }
}
impl Drop for AgentCwd {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn validate_model(chat: &Value, model: &str) -> Result<(), String> {
    let available: Vec<_> = chat["models"].as_array().into_iter().flatten().filter_map(|m| m["id"].as_str()).collect();
    if available.contains(&model) {
        Ok(())
    } else {
        Err(format!(
            "agent model {model:?} is unavailable; available models: {}",
            if available.is_empty() { "(none reported)".into() } else { available.join(", ") }
        ))
    }
}

fn ready_agent(control: &mut Control, mut chat: Value) -> Result<Value, String> {
    let end = Instant::now() + REQUEST_TIMEOUT;
    loop {
        if let Some(error) = chat["items"].as_array().into_iter().flatten().find(|i| i["error"] == true) {
            return Err(format!("agent session: {}", error["notice"].as_str().unwrap_or("failed")));
        }
        match chat["state"].as_str() {
            Some("ready") if chat["modelPending"] != true => return Ok(chat),
            Some("starting" | "ready") => {}
            _ => return Err(format!("agent session did not become ready: {}", chat["state"])),
        }
        if Instant::now() >= end {
            return Err("timeout waiting for agent session".into());
        }
        std::thread::sleep(POLL);
        chat = control.call("ui.agent_chat", json!({}), end)?;
    }
}

fn write(path: &str, text: &str) -> Result<(), String> {
    solidcraft_engine::io::vfs::write(path, text.as_bytes()).map_err(|e| format!("{path}: {e}"))
}

struct Options<'a> {
    set: Option<&'a str>,
    references_only: bool,
    connect: &'a str,
    token: Option<&'a str>,
    base_url: &'a str,
    model: &'a str,
    kind: &'a str,
    agent: Option<&'a str>,
    agent_model: Option<&'a str>,
    key: Option<String>,
    tasks: Vec<usize>,
    repeat: u64,
    timeout: Duration,
    json: Option<&'a str>,
    markdown: Option<&'a str>,
}

fn options(args: &[String]) -> Result<Options<'_>, String> {
    options_with_env(args, |name| std::env::var(name).ok())
}

fn options_with_env(args: &[String], env: impl Fn(&str) -> Option<String>) -> Result<Options<'_>, String> {
    const FLAGS: [&str; 15] = [
        "--set",
        "--test",
        "--connect",
        "--token",
        "--base-url",
        "--model",
        "--kind",
        "--key-env",
        "--agent",
        "--agent-model",
        "--tasks",
        "--repeat",
        "--timeout-s",
        "--json",
        "--markdown",
    ];
    let mut given = std::collections::BTreeMap::new();
    let references_only = args.iter().any(|a| a == "--references-only");
    let filtered: Vec<_> = args.iter().filter(|a| a.as_str() != "--references-only").collect();
    let mut args = filtered.into_iter();
    while let Some(flag) = args.next() {
        let Some(flag) = FLAGS.iter().find(|f| **f == flag.as_str()) else { return Err("unknown bench-assistant option".into()) };
        let value = args
            .next()
            .filter(|v| !v.starts_with("--") && !v.is_empty() && v.len() <= 4096)
            .ok_or_else(|| format!("{flag} needs a value (at most 4096 bytes)"))?;
        if given.insert(*flag, value.as_str()).is_some() {
            return Err(format!("{flag} given twice"));
        }
    }
    let required = |key: &str| given.get(key).copied().ok_or_else(|| format!("bench-assistant needs {key}"));
    let number = |key: &str, default: u64, max: u64| -> Result<u64, String> {
        given
            .get(key)
            .map_or(Ok(default), |s| s.parse::<u64>().ok().filter(|n| (1..=max).contains(n)).ok_or_else(|| format!("{key} must be 1 to {max}")))
    };
    if given.get("--test").is_some_and(|v| *v != "v1") || (given.contains_key("--test") && given.contains_key("--set")) {
        return Err("use --test v1 or --set DIR".into());
    }
    if references_only && !given.contains_key("--set") {
        return Err("--references-only requires --set DIR".into());
    }
    let count = if let Some(dir) = given.get("--set") { sets::load(std::path::Path::new(dir), "tasks.json")?.len() } else { 11 };
    let tasks = match given.get("--tasks") {
        None => (1..=count).collect(),
        Some(list) => {
            let mut tasks = Vec::new();
            for n in list.split(',') {
                let n = n
                    .trim()
                    .parse::<usize>()
                    .ok()
                    .filter(|n| (1..=count).contains(n))
                    .ok_or("--tasks takes valid task numbers, separated by commas")?;
                if tasks.contains(&n) {
                    return Err(format!("--tasks lists task {n} twice"));
                }
                tasks.push(n);
            }
            tasks
        }
    };
    let agent = given.get("--agent").copied();
    if agent.is_some() && ["--base-url", "--model", "--kind", "--key-env"].iter().any(|f| given.contains_key(f)) {
        return Err("--agent cannot be combined with --base-url, --model, --kind, or --key-env".into());
    }
    if agent.is_none() && given.contains_key("--agent-model") {
        return Err("--agent-model requires --agent".into());
    }
    let kind = given.get("--kind").copied().unwrap_or("openai");
    if !matches!(kind, "openai" | "anthropic") {
        return Err("--kind must be openai or anthropic".into());
    }
    let key = given
        .get("--key-env")
        .map(|name| {
            if !name.bytes().enumerate().all(|(i, b)| b == b'_' || b.is_ascii_alphabetic() || (i > 0 && b.is_ascii_digit())) {
                return Err("--key-env needs an environment variable name".to_string());
            }
            env(name)
                .filter(|k| !k.trim().is_empty() && k.len() <= 4096 && !k.contains(['\r', '\n', '\0']))
                .ok_or_else(|| "--key-env variable is missing, empty, or invalid (at most 4096 bytes)".to_string())
        })
        .transpose()?;
    Ok(Options {
        set: given.get("--set").copied(),
        references_only,
        connect: required("--connect")?,
        token: given.get("--token").copied(),
        base_url: if agent.is_none() && !references_only { required("--base-url")? } else { "" },
        model: if agent.is_none() && !references_only { required("--model")? } else { "" },
        kind,
        agent,
        agent_model: given.get("--agent-model").copied(),
        key,
        tasks,
        repeat: number("--repeat", 1, 100)?,
        timeout: Duration::from_secs(number("--timeout-s", 300, 3600)?),
        json: given.get("--json").copied(),
        markdown: given.get("--markdown").copied(),
    })
}

/// Waits for the turn the prompt started to end. Returns the chat's last state and whether the
/// deadline passed first.
fn wait_for_turn(control: &mut Control, mut chat: Value, began: Instant, end: Instant) -> Result<(Value, bool), String> {
    let mut seen_busy = false;
    loop {
        let state = chat["state"].as_str().unwrap_or_default();
        let turn = !chat["turn"].is_null();
        let queued = chat["pending"].as_array().is_some_and(|p| !p.is_empty());
        seen_busy |= state == "busy" || turn;
        let idle = !matches!(state, "busy" | "starting") && !turn && !queued;
        // A turn can start and end between two polls: then the conversation has its answer.
        let answered = chat["items"].as_array().into_iter().flatten().any(|i| i.get("user").is_none());
        if idle && (seen_busy || answered || matches!(state, "exited" | "auth" | "off") || began.elapsed() >= START_GRACE) {
            return Ok((chat, false));
        }
        let Some(left) = end.checked_duration_since(Instant::now()).filter(|d| !d.is_zero()) else { return Ok((chat, true)) };
        std::thread::sleep(POLL.min(left));
        match control.call("ui.agent_chat", json!({}), end) {
            Ok(next) => chat = next,
            Err(e) if e == "timeout" => return Ok((chat, true)),
            Err(e) => return Err(e),
        }
    }
}

impl Options<'_> {
    fn setup(&self) -> Value {
        if self.agent.is_some() {
            json!({"features": true, "refresh": true})
        } else {
            let mut add = json!({"id": AGENT_ID, "name": AGENT_NAME, "kind": if self.kind == "openai" { "api" } else { "anthropic" }, "base_url": self.base_url, "model": self.model});
            if let Some(key) = &self.key {
                add["api_key"] = json!(key);
            }
            json!({"features": true, "add_agent": add})
        }
    }

    fn route(&self, config: &Value) -> Result<Value, String> {
        match self.agent {
            Some(id) => {
                let entry = config["agents"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|a| a["id"] == id && a["kind"] == "acp" && a["found"] == true)
                    .ok_or("--agent was not found among installed coding agents")?;
                Ok(
                    json!({"agent": id, "name": entry["name"].as_str().unwrap_or(id), "version": entry["version"].as_str().filter(|v| !v.is_empty() && v.len() <= 256), "note": "Coding agents bring their own prompt; results are not directly comparable with API runs."}),
                )
            }
            None => Ok(json!({"kind": self.kind, "model": self.model})),
        }
    }
}

fn route_label(route: &Value) -> String {
    if let Some(id) = route["agent"].as_str() {
        format!("agent {id} (version {}) — {}", route["version"].as_str().unwrap_or("unknown"), route["note"].as_str().unwrap_or_default())
    } else {
        format!("{} API, model {}", route["kind"].as_str().unwrap_or_default(), route["model"].as_str().unwrap_or_default())
    }
}

// Redact even echoed credentials in endpoint errors and conversation exports.
fn redact(text: &str, key: Option<&str>) -> String {
    match key {
        Some(key) => {
            let escaped = json!(key).to_string();
            let escaped = escaped.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(key);
            text.replace(escaped, "[redacted]").replace(key, "[redacted]")
        }
        None => text.to_string(),
    }
}

fn redact_value(value: &mut Value, key: Option<&str>) {
    match value {
        Value::String(text) => *text = redact(text, key),
        Value::Array(items) => items.iter_mut().for_each(|v| redact_value(v, key)),
        Value::Object(fields) => {
            *fields = std::mem::take(fields)
                .into_iter()
                .map(|(name, mut value)| {
                    redact_value(&mut value, key);
                    (redact(&name, key), value)
                })
                .collect();
        }
        _ => {}
    }
}

pub fn run(args: &[String]) -> Result<(), String> {
    let o = options(args)?;
    run_with_options(&o).map_err(|e| redact(&e, o.key.as_deref()))
}

fn run_with_options(o: &Options) -> Result<(), String> {
    let tasks = if let Some(dir) = o.set { sets::load(std::path::Path::new(dir), "tasks.json")? } else { tasks()? };
    let mut control = Control::connect(o.connect, o.token)?;
    let help = control.call("ui.help", json!({}), Instant::now() + REQUEST_TIMEOUT)?;
    let version = help["version"].as_str().filter(|s| !s.is_empty() && s.len() <= 256).ok_or("the app reported no version")?.to_string();
    if o.references_only {
        return sets::references(&mut control, o, &tasks, &version);
    }
    // Setup errors can contain endpoint replies; the outer call redacts credentials.
    let mut config = control.chat(o.setup())?;
    if o.agent.is_some() {
        let end = Instant::now() + REQUEST_TIMEOUT;
        while config["detection"].is_null() || config["detecting"] == true {
            std::thread::sleep(POLL);
            config = control.call("ui.agent_chat", json!({}), end)?;
        }
    } else if !config["agents"].as_array().into_iter().flatten().any(|a| a["id"] == AGENT_ID) {
        return Err("the app did not add the benchmark's agent".into());
    }
    if o.agent.is_some() && config.get("cwd").is_none() {
        return Err("the app does not support agent session cwd; update the app before using --agent".into());
    }
    let route = o.route(&config)?;
    // Put the user's permission setting back afterwards.
    let permissions = config["permissions"].as_str().map(str::to_string);
    let mut workspaces = Vec::new();
    let result = run_tasks(&mut control, o, &tasks, &version, &route, &mut workspaces);
    let _ = control.chat(json!({"stop": true}));
    if let Some(p) = permissions {
        let _ = control.chat(json!({"permissions": p}));
    }
    result
}

fn run_tasks(
    control: &mut Control,
    o: &Options,
    tasks: &[Value],
    version: &str,
    route: &Value,
    workspaces: &mut Vec<AgentCwd>,
) -> Result<(), String> {
    let mut route = route.clone();
    let mut results: Vec<Value> = Vec::new();
    let mut kept = 0;
    let mut stdout = std::io::stdout().lock();
    let mut say = |line: String| writeln!(stdout, "{}", redact(&line, o.key.as_deref())).map_err(|e| format!("stdout: {e}"));
    say(format!("bench-assistant {}: {} on SolidCraft {version}", sets::name(o), route_label(&route)))?;
    say("| run | # | task | pass/fail | how it failed | seconds |\n|---|---|---|---|---|---|".into())?;
    for run in 1..=o.repeat {
        for &n in &o.tasks {
            let task = tasks.get(n - 1).ok_or("missing embedded task")?;
            // A fresh design and a fresh chat, with nothing of the last task still running.
            control.chat(json!({"stop": true}))?;
            if o.agent.is_some() {
                workspaces.push(AgentCwd::new()?);
            }
            let cwd = workspaces.last();
            let mut start = json!({"agent": o.agent.unwrap_or(AGENT_ID), "permissions": "all", "new_chat": true, "attach_view": false});
            if let Some(cwd) = cwd {
                start["cwd"] = json!(cwd.0.to_str().ok_or("benchmark workspace path is not valid UTF-8")?);
                start["connect"] = json!(true);
            }
            let chat = control.chat(start)?;
            if o.agent.is_some() {
                let chat = ready_agent(control, chat)?;
                if chat["cwd"].as_str() != cwd.and_then(|cwd| cwd.0.to_str()) {
                    return Err("the app did not start the agent in the benchmark workspace".into());
                }
                let chat = if let Some(model) = o.agent_model {
                    validate_model(&chat, model)?;
                    let chat = control.chat(json!({"model": model}))?;
                    ready_agent(control, chat)?
                } else {
                    chat
                };
                route["model"] = chat["model"].clone();
            }
            control.execute("file.new", json!({"name": "Benchmark"}))?;
            sets::start(control, task)?;
            let before = if o.set.is_some() { sets::inspect(control, &task["check"])? } else { control.inspect()? };
            let began = Instant::now();
            let end = began + o.timeout;
            let (chat, timed_out) = match control.call("ui.agent_chat", json!({"send": task["prompt"]}), end) {
                Ok(chat) => wait_for_turn(control, chat, began, end)?,
                Err(e) if e == "timeout" => (json!({}), true),
                Err(e) => return Err(e),
            };
            if timed_out {
                // The late reply would arrive on the old connection; stop the model on a new one
                // before reading the design.
                *control = control.reopen()?;
            }
            // Stop the agent before reading the result, so nothing changes the design meanwhile.
            let stopped = control.chat(json!({"stop": true}))?;
            let items = stopped["items"].as_array().or(chat["items"].as_array()).cloned().unwrap_or_default();
            workspaces.clear();
            let after = if o.set.is_some() { sets::inspect(control, &task["check"])? } else { control.inspect()? };
            let mut failed_rules = if o.set.is_some() {
                sets::check(&task["check"], &before, &after, &reply(&items))
            } else {
                check(&task["check"], &before, &after, &items)
            };
            if o.set.is_some() && timed_out {
                failed_rules.push("assistant turn timed out".into());
            }
            let passed = failed_rules.is_empty();
            let failure = if passed {
                String::new()
            } else if o.agent.is_some() {
                failure_detail(&items, timed_out)
            } else {
                classify(&items, timed_out).into()
            };
            let seconds = began.elapsed().as_secs_f64();
            say(format!(
                "| {run} | {n} | {} | {} | {failure} | {seconds:.0} |",
                task["title"].as_str().or(task["id"].as_str()).unwrap_or_default(),
                if passed { "pass" } else { "fail" }
            ))?;
            let mut result = json!({
                "task": n, "title": task["title"], "id": task["id"], "prompt": task["prompt"], "repeat": run, "passed": passed, "failure": failure,
                "failed_rules": failed_rules, "timed_out": timed_out, "seconds": seconds, "check": task["check"],
                "items": items, "before": before, "after": after,
            });
            if o.agent.is_some() {
                result["model"] = route["model"].clone();
            }
            kept += result.to_string().len();
            if kept > MAX_RESULTS {
                return Err("the results passed 64 MiB; run fewer tasks or repetitions".into());
            }
            results.push(result);
            // Written after every task, so an interrupted run keeps what it measured.
            if let Some(path) = o.json {
                let mut all = json!({"test": sets::name(o), "model": route["model"], "base_url": o.base_url, "route": route, "solidcraft": version, "results": results});
                redact_value(&mut all, o.key.as_deref());
                write(path, &serde_json::to_string_pretty(&all).map_err(|e| e.to_string())?)?;
            }
            if let Some(path) = o.markdown {
                write(
                    path,
                    &redact(
                        &(1..=run)
                            .map(|r| {
                                if o.set.is_some() {
                                    sets::markdown(o, version, &results, r, &route)
                                } else {
                                    report_markdown(&route, version, &results, r)
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("\n"),
                        o.key.as_deref(),
                    ),
                )?;
            }
        }
    }
    for run in 1..=o.repeat {
        let rows: Vec<&Value> = results.iter().filter(|r| r["repeat"] == run).collect();
        say(format!("run {run}: {}/{} passed", rows.iter().filter(|r| r["passed"] == true).count(), rows.len()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_markdown_has_filled_metadata() {
        let route = json!({"agent": "claude", "name": "Claude", "version": "0.90.0", "model": "haiku"});
        let report = report_markdown(&route, "1.2.3", &[json!({"task": 1, "repeat": 1, "passed": true})], 1);
        assert!(report.starts_with("### Claude 0.90.0, model haiku\nSolidCraft: 1.2.3\nResult: 1/11\n| # |"));
        assert!(!report.contains('<') && !report.contains("Notes:") && !report.contains("Runtime:"));
    }

    #[test]
    fn failed_tool_titles_survive_null_output() {
        let mut items = vec![json!({"tool": "1", "title": "Create cube", "status": "Failed", "output": null})];
        assert_eq!(failure_detail(&items, false), "tool call failed: Create cube (status Failed)");
        items.push(json!({"tool": "2", "title": "Inspect", "status": "Completed"}));
        assert_eq!(failure_detail(&items, false), "tool call failed: Create cube (status Failed)");
        items.push(json!({"tool": "3", "title": "Move cube", "status": "Failed", "output": null}));
        assert_eq!(failure_detail(&items, false), "tool call failed: Create cube (status Failed); tool call failed: Move cube (status Failed)");
        assert_eq!(failure_detail(&items, true), "timeout");
    }

    /// The app-facing fake exposes the same session model list as the ACP backend.
    #[test]
    fn agent_model_selection_workspace_and_results() {
        for requested in [None, Some("smart"), Some("missing")] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let (stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream);
                let mut cwd = None;
                let mut model = "fast".to_string();
                let mut prompts = 0;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap() == 0 {
                        break;
                    }
                    let r: Value = serde_json::from_str(&line).unwrap();
                    let p = &r["params"];
                    if let Some(path) = p["cwd"].as_str() {
                        let path = std::path::PathBuf::from(path);
                        assert!(path.is_absolute() && path != std::env::current_dir().unwrap());
                        assert_eq!(std::fs::read_dir(&path).unwrap().count(), 0);
                        std::fs::write(path.join("agent-output"), "temporary").unwrap();
                        cwd = Some(path);
                    }
                    if let Some(m) = p["model"].as_str() {
                        model = m.into();
                    }
                    if p.get("send").is_some() {
                        prompts += 1;
                    }
                    let result = match r["method"].as_str().unwrap() {
                        "document.inspect" => json!({"bodies": []}),
                        _ => {
                            json!({"state": "ready", "cwd": cwd.as_ref().and_then(|p| p.to_str()), "model": model, "models": [{"id": "fast"}, {"id": "smart"}],
                            "items": [{"agent": "done"}]})
                        }
                    };
                    writeln!(reader.get_mut(), "{}", json!({"id": r["id"], "ok": true, "result": result})).unwrap();
                }
                (cwd.unwrap(), model, prompts)
            });
            let output = AgentCwd::new().unwrap();
            let json_path = output.0.join("results.json");
            let mut args = vec![
                "--connect".into(),
                addr.to_string(),
                "--agent".into(),
                "fake".into(),
                "--tasks".into(),
                "1".into(),
                "--json".into(),
                json_path.to_str().unwrap().into(),
            ];
            if let Some(m) = requested {
                args.extend(["--agent-model".into(), m.into()]);
            }
            let o = options_with_env(&args, |_| None).unwrap();
            let mut control = Control::open(addr, "test".into()).unwrap();
            let mut workspaces = Vec::new();
            let result = run_tasks(
                &mut control,
                &o,
                &[json!({"id": "fake", "prompt": "test", "check": {"bodies": 0}})],
                "1",
                &json!({"agent": "fake"}),
                &mut workspaces,
            );
            // The outer runner always stops the process before dropping workspaces, even on errors.
            control.chat(json!({"stop": true})).unwrap();
            drop(workspaces);
            drop(control);
            let (cwd, model, prompts) = server.join().unwrap();
            assert!(!cwd.exists());
            if requested == Some("missing") {
                let error = result.unwrap_err();
                assert!(error.contains("missing") && error.contains("fast, smart"), "{error}");
                assert_eq!(prompts, 0);
            } else {
                result.unwrap();
                assert_eq!(model, requested.unwrap_or("fast"));
                let result: Value = serde_json::from_slice(&std::fs::read(&json_path).unwrap()).unwrap();
                assert_eq!(result["model"], model);
                assert_eq!(prompts, 1);
            }
        }
    }

    fn design(bodies: Value, timeline: Value) -> Value {
        json!({"name": "Benchmark", "params": [], "timeline": timeline, "bodies": bodies, "sketches": [], "undo": 1, "revision": 3, "dirty": true})
    }

    fn body(name: &str, volume: f64, min: [f64; 3], max: [f64; 3]) -> Value {
        json!({"name": name, "volume_mm3": volume, "bbox": {"min": min, "max": max}, "visible": true})
    }

    #[test]
    fn the_embedded_tasks_are_v1() {
        let tasks = tasks().unwrap();
        assert_eq!(tasks.len(), 11);
        assert_eq!(tasks[0]["prompt"], "Make a 20 mm cube.");
        assert_eq!(tasks[10]["prompt"], "can you use the software to make a change to the current shape, any change at all");
        assert_eq!(tasks[5]["check"]["tol"], 0.003);
        assert_eq!(tasks[6]["check"]["bbox_min"], json!([50, 0, 0]));
        let boxes: Vec<_> = tasks.iter().filter(|t| t["setup"].is_array()).map(|t| t["id"].as_str().unwrap()).collect();
        assert_eq!(boxes, ["06-fillet-box", "07-move-body", "08-rename-body", "09-volume-question", "11-any-change"]);
    }

    #[test]
    fn pass_rules() {
        let tasks = tasks().unwrap();
        let rule = |n: usize| tasks[n - 1]["check"].clone();
        let empty = design(json!([]), json!([]));
        let pass = |n: usize, before: &Value, after: &Value, items: &[Value]| check(&rule(n), before, after, items);
        // 3: any orientation, within 1%.
        let block = design(json!([body("Body1", 24_100.0, [0.0; 3], [40.0, 10.0, 60.2])]), json!([{"type": "Box"}]));
        assert!(pass(3, &empty, &block, &[]).is_empty());
        let thick = design(json!([body("Body1", 24_000.0, [0.0; 3], [40.0, 12.0, 60.0])]), json!([]));
        assert_eq!(pass(3, &empty, &thick, &[]).len(), 1);
        let two = design(json!([body("A", 12_000.0, [0.0; 3], [60.0, 40.0, 5.0]), body("B", 12_000.0, [0.0; 3], [60.0, 40.0, 5.0])]), json!([]));
        assert!(!pass(3, &empty, &two, &[]).is_empty());
        // A hidden body is not part of the result.
        let mut hidden = two.clone();
        hidden["bodies"][1]["visible"] = json!(false);
        hidden["bodies"][0]["volume_mm3"] = json!(24_000.0);
        hidden["bodies"][0]["bbox"]["max"] = json!([60.0, 40.0, 10.0]);
        assert!(pass(3, &empty, &hidden, &[]).is_empty());
        // 4: the timeline needs a working Extrude.
        let plate = |kind: Value| design(json!([body("Body1", 22_500.0, [0.0; 3], [50.0, 30.0, 15.0])]), kind);
        assert!(pass(4, &empty, &plate(json!([{"type": "Sketch"}, {"type": "Extrude"}])), &[]).is_empty());
        assert!(!pass(4, &empty, &plate(json!([{"type": "Box"}])), &[]).is_empty());
        for flag in [json!({"suppressed": true}), json!({"rolled_back": true}), json!({"error": "failed"})] {
            let mut feature = json!({"type": "Extrude"});
            feature.as_object_mut().unwrap().extend(flag.as_object().unwrap().clone());
            assert!(!pass(4, &empty, &plate(json!([feature])), &[]).is_empty(), "{flag}");
        }
        // 6: the fillet's tolerance is 0.3%.
        let start = design(json!([body("Body1", 64_000.0, [0.0; 3], [40.0; 3])]), json!([{"type": "Box"}]));
        let filleted = |v: f64| design(json!([body("Body1", v, [0.0; 3], [40.0; 3])]), json!([{"type": "Box"}, {"type": "Fillet"}]));
        assert!(pass(6, &start, &filleted(63_108.53 * 1.002), &[]).is_empty());
        assert!(!pass(6, &start, &filleted(63_108.53 * 1.004), &[]).is_empty());
        // 7: the lowest corner.
        let moved = |x: f64| design(json!([body("Body1", 8000.0, [x, 0.0, 0.0], [x + 20.0, 20.0, 20.0])]), json!([]));
        assert!(pass(7, &empty, &moved(50.0), &[]).is_empty());
        assert!(!pass(7, &empty, &moved(0.0), &[]).is_empty());
        // 8: the name.
        let named = |name: &str| design(json!([body(name, 6000.0, [0.0; 3], [30.0, 20.0, 10.0])]), json!([]));
        assert!(pass(8, &empty, &named("Bracket"), &[]).is_empty());
        assert!(!pass(8, &empty, &named("Body1"), &[]).is_empty());
        // 9: unchanged, and the reply gives the volume.
        let start = named("Body1");
        let said = |text: &str| [json!({"user": "What is the volume of the part?"}), json!({"agent": text, "thought": false})];
        let mut read = start.clone();
        read["revision"] = json!(7);
        read["dirty"] = json!(false);
        read["selection"] = json!([{"body": "Body1"}]);
        assert!(pass(9, &start, &read, &said("The volume is 6000 mm³.")).is_empty());
        assert!(!pass(9, &start, &read, &said("About 60 cm³.")).is_empty());
        assert!(!pass(9, &start, &named("Bracket"), &said("6 000 mm³")).is_empty());
        let mut undone = start.clone();
        undone["undo"] = json!(2);
        assert!(!pass(9, &start, &undone, &said("6000 mm³")).is_empty());
        // 11: any change counts; reading does not.
        assert!(pass(11, &start, &named("Bracket"), &[]).is_empty());
        assert!(pass(11, &start, &undone, &[]).is_empty());
        assert!(!pass(11, &start, &read, &[]).is_empty());
        // An unreadable design passes nothing.
        assert!(!pass(11, &start, &json!({}), &[]).is_empty());
    }

    #[test]
    fn the_volume_reply() {
        for yes in ["6000", "6 000 mm³", "6,000", "6.000", "6\u{202f}000", "6 cm³", "6.00cm3", "is 6000."] {
            assert!(volume_reply(yes), "{yes}");
        }
        for no in ["60000", "6,0001", "60 cm³", "600", "6.01 cm³", ""] {
            assert!(!volume_reply(no), "{no}");
        }
    }

    #[test]
    fn failure_categories() {
        for (items, expected) in [
            (
                json!([{"user": "Make a 20 mm cube."}, {"agent": "{\"name\": \"execute\", \"arguments\": {}}", "thought": false}]),
                "wrote the call as text",
            ),
            (
                json!([{"notice": "HTTP 400: registry.ollama.ai/library/gemma3:4b does not support tools", "error": true}]),
                "the endpoint refused tools (HTTP 400)",
            ),
            (json!([{"user": "Make a 20 mm cube."}, {"agent": "", "thought": false}]), "an empty reply"),
            (json!([{"agent": "Let me think", "thought": true}]), "an empty reply"),
            (json!([{"agent": "Sure, here is how to make a cube.", "thought": false}]), "only talked"),
            (
                json!([{"tool": "1", "status": "Failed", "output": "unknown command `solid.cube` (did you mean `solid.box`?)"}]),
                "wrong or unknown command",
            ),
            (json!([{"tool": "1", "status": "Failed", "output": "unknown tool `make_cube`"}]), "wrong or unknown command"),
            (json!([{"tool": "1", "status": "Failed", "output": "invalid parameters for `solid.box`: `length` is required"}]), "wrong arguments"),
            (json!([{"tool": "1", "status": "Failed", "output": "Not run: the tool arguments were not valid JSON"}]), "wrong arguments"),
            (json!([{"tool": "1", "status": "Failed", "output": "no body `Box`"}]), "gave up after errors"),
            (
                json!([{"tool": "1", "status": "Completed", "output": "{\"ok\": false, \"failed\": {\"error\": \"unknown command `solid.delete`\"}}"}]),
                "wrong or unknown command",
            ),
            (
                json!([{"tool": "1", "status": "Completed"}, {"agent": "\n(stopped after 24 requests for one message; send another message to go on)"}]),
                "hit the request limit",
            ),
            (json!([{"tool": "1", "status": "Completed", "output": "{}"}]), "wrong result"),
        ] {
            let items = items.as_array().unwrap();
            assert_eq!(classify(items, false), expected, "{items:?}");
            assert_eq!(classify(items, true), "timeout");
        }
    }

    #[test]
    fn markdown_is_the_comment_template() {
        let rows: Vec<Value> =
            (1..=11).map(|n| json!({"task": n, "repeat": 1, "passed": n != 2, "failure": if n == 2 { "an empty reply" } else { "" }})).collect();
        let report = markdown("qwen2.5:7b", "0.19.0", &rows, 1);
        assert!(report.starts_with(
            "### qwen2.5:7b (<quant>, <download size>)\nRuntime: <Ollama/LM Studio/... version>, context <num_ctx if changed>\nSolidCraft: 0.19.0\nResult: 10/11\n| # | pass/fail | how it failed |\n|---|---|---|\n| 1 | pass | |\n| 2 | fail | an empty reply |\n"
        ));
        assert!(report.ends_with("| 11 | pass | |\nNotes: <anything odd, e.g. very slow, needed a retry>\n"));
        let partial = markdown("m|x", "v", &rows[..1], 1);
        assert!(partial.starts_with("### m x ("));
        assert!(partial.contains("Result: 1/11") && partial.contains("| 2 | | not run |") && partial.contains("partial run"));
    }

    #[test]
    fn options_support_api_kinds_and_installed_agents() {
        let parse = |args: &[&str]| options(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>()).map(|_| ());
        assert!(
            parse(&["--connect", "127.0.0.1:9", "--kind", "anthropic", "--base-url", "https://api.anthropic.com", "--model", "claude-haiku-5-5"])
                .is_ok()
        );
        assert!(parse(&["--connect", "127.0.0.1:9", "--agent", "claude"]).is_ok());
        assert!(parse(&["--connect", "127.0.0.1:9", "--agent", "claude", "--agent-model", "smart"]).is_ok());
        assert!(parse(&["--connect", "127.0.0.1:9", "--base-url", "u", "--model", "m", "--agent-model", "smart"]).is_err());
        for args in [&["--agent"][..], &["--kind"], &["--key-env"], &["--api-key", "synthetic-secret"]] {
            let error = options(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>()).err().unwrap();
            assert!(!error.contains("synthetic-secret"));
        }
        for flag in ["--base-url", "--model", "--kind", "--key-env"] {
            assert!(parse(&["--connect", "127.0.0.1:9", "--agent", "claude", flag, "value"]).is_err());
        }
        assert!(parse(&["--connect", "127.0.0.1:9", "--kind", "other", "--base-url", "u", "--model", "m"]).is_err());
        let args: Vec<String> = ["--connect", "127.0.0.1:9", "--kind", "anthropic", "--base-url", "https://example.com", "--model", "m"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let o = options(&args).unwrap();
        assert_eq!(o.setup()["add_agent"]["kind"], "anthropic");
        assert_eq!(o.route(&json!({})).unwrap(), json!({"kind": "anthropic", "model": "m"}));
    }

    #[test]
    fn options_validate_key_environment_without_echoing_values() {
        let args: Vec<String> = ["--connect", "127.0.0.1:9", "--base-url", "https://example.com", "--model", "m", "--key-env", "BENCH_TEST_KEY"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let too_long = "x".repeat(4097);
        for value in [None, Some(""), Some("secret\nvalue"), Some(too_long.as_str())] {
            let error = options_with_env(&args, |_| value.map(str::to_string)).err().unwrap();
            assert!(error.contains("--key-env"));
            assert!(!error.contains("secret"));
        }
        let secret = "synthetic-credential-\"quoted\"";
        let o = options_with_env(&args, |_| Some(secret.into())).unwrap();
        assert_eq!(o.kind, "openai");
        assert_eq!(o.setup()["add_agent"]["kind"], "api");
        assert_eq!(o.setup()["add_agent"]["api_key"], secret);
        let route = o.route(&json!({})).unwrap();
        let mut exported = json!({"route": route, "items": [{"notice": secret}]});
        redact_value(&mut exported, o.key.as_deref());
        let serialized = serde_json::to_string(&exported).unwrap();
        assert!(!serialized.contains("synthetic-credential"));
        assert!(serde_json::from_str::<Value>(&serialized).is_ok());

        assert!(!route.to_string().contains(secret));
        for output in [route_label(&route), markdown(&route_label(&route), "1", &[], 1), format!("error: {secret}")] {
            let clean = redact(&output, o.key.as_deref());
            assert!(!clean.contains(secret));
            assert!(!clean.contains("synthetic-credential"));
        }
    }

    #[test]
    fn options_record_installed_agent_version_and_prompt() {
        let args: Vec<String> = ["--connect", "127.0.0.1:9", "--agent", "claude"].iter().map(|s| s.to_string()).collect();
        let o = options(&args).unwrap();
        assert!(o.setup().get("add_agent").is_none());
        assert!(o.route(&json!({"agents": []})).is_err());
        let route = o.route(&json!({"agents": [{"id": "claude", "kind": "acp", "found": true, "version": "1.2.3"}]})).unwrap();
        assert_eq!(route["agent"], "claude");
        assert_eq!(route["version"], "1.2.3");
        let label = route_label(&route);
        assert!(label.contains("claude") && label.contains("1.2.3") && label.contains("own prompt"));
        assert!(markdown(&label, "1", &[], 1).contains("own prompt"));
    }

    #[test]
    fn options_are_checked_before_connecting() {
        for extra in [
            &["--repeat", "0"][..],
            &["--repeat", "101"],
            &["--timeout-s", "3601"],
            &["--tasks", "0"],
            &["--tasks", "12"],
            &["--tasks", "1,1"],
            &["--model", "again"],
            &["--unexpected", "1"],
            &["--token"],
        ] {
            let mut args: Vec<String> =
                ["--connect", "127.0.0.1:9", "--model", "m", "--base-url", "http://127.0.0.1:11434/v1"].iter().map(|s| s.to_string()).collect();
            args.extend(extra.iter().map(|s| s.to_string()));
            assert!(options(&args).is_err(), "{extra:?}");
        }
        let args: Vec<String> =
            ["--connect", "127.0.0.1:9", "--model", "m", "--base-url", "u", "--tasks", "2, 5"].iter().map(|s| s.to_string()).collect();
        let o = options(&args).unwrap();
        assert_eq!((o.tasks, o.repeat, o.timeout), (vec![2, 5], 1, Duration::from_secs(300)));
        assert!(Control::connect("10.0.0.1:9271", Some("t")).is_err());
        assert!(Control::connect("localhost", Some("t")).is_err());
    }
}
