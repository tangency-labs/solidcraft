//! `bench-assistant`: the standard Assistant model test (v1, #4851), run against a desktop app.
//!
//! The app is already running with `--control PORT`; everything goes through its control channel
//! (`docs/control-protocol.md`), so the command needs no window code of its own. For each task it
//! starts a new design, runs the task's prescribed setup (a box, for some), adds the endpoint as an
//! API agent with All permissions, starts a new chat, sends the prompt once and waits for the turn
//! to end. The result is checked on the design (`document.inspect` with measurements), never on
//! what the model says, except for the one question whose answer is the reply. A failed task is
//! put in one of the issue's failure categories from the conversation's items.
//!
//! ```text
//! solidcraft-cli bench-assistant --connect 127.0.0.1:PORT --base-url URL --model NAME
//!     [--token T] [--tasks 1,2,5] [--repeat N] [--timeout-s 300]
//!     [--json out.json] [--markdown out.md]
//! ```
//! prints a table of the results; `--markdown` writes the issue's comment template filled in,
//! `--json` every task's rules, conversation and design before and after.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

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

fn write(path: &str, text: &str) -> Result<(), String> {
    solidcraft_engine::io::vfs::write(path, text.as_bytes()).map_err(|e| format!("{path}: {e}"))
}

struct Options<'a> {
    connect: &'a str,
    token: Option<&'a str>,
    base_url: &'a str,
    model: &'a str,
    tasks: Vec<usize>,
    repeat: u64,
    timeout: Duration,
    json: Option<&'a str>,
    markdown: Option<&'a str>,
}

fn options(args: &[String]) -> Result<Options<'_>, String> {
    const FLAGS: [&str; 9] = ["--connect", "--token", "--base-url", "--model", "--tasks", "--repeat", "--timeout-s", "--json", "--markdown"];
    let mut given = std::collections::BTreeMap::new();
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        let Some(flag) = FLAGS.iter().find(|f| **f == flag.as_str()) else { return Err(format!("unknown bench-assistant option: {flag}")) };
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
    let tasks = match given.get("--tasks") {
        None => (1..=11).collect(),
        Some(list) => {
            let mut tasks = Vec::new();
            for n in list.split(',') {
                let n = n
                    .trim()
                    .parse::<usize>()
                    .ok()
                    .filter(|n| (1..=11).contains(n))
                    .ok_or("--tasks takes task numbers 1 to 11, separated by commas")?;
                if tasks.contains(&n) {
                    return Err(format!("--tasks lists task {n} twice"));
                }
                tasks.push(n);
            }
            tasks
        }
    };
    Ok(Options {
        connect: required("--connect")?,
        token: given.get("--token").copied(),
        base_url: required("--base-url")?,
        model: required("--model")?,
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
        if idle && (seen_busy || answered || matches!(state, "exited" | "failed" | "auth" | "off") || began.elapsed() >= START_GRACE) {
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

pub fn run(args: &[String]) -> Result<(), String> {
    let o = options(args)?;
    let tasks = tasks()?;
    let mut control = Control::connect(o.connect, o.token)?;
    let help = control.call("ui.help", json!({}), Instant::now() + REQUEST_TIMEOUT)?;
    let version = help["version"].as_str().filter(|s| !s.is_empty() && s.len() <= 256).ok_or("the app reported no version")?.to_string();
    let config = control.chat(json!({
        "features": true,
        "add_agent": {"id": AGENT_ID, "name": AGENT_NAME, "kind": "api", "base_url": o.base_url, "model": o.model},
    }))?;
    if !config["agents"].as_array().into_iter().flatten().any(|a| a["id"] == AGENT_ID) {
        return Err("the app did not add the benchmark's agent".into());
    }
    // Put the user's permission setting back afterwards.
    let permissions = config["permissions"].as_str().map(str::to_string);
    let result = run_tasks(&mut control, &o, &tasks, &version);
    let _ = control.chat(json!({"stop": true}));
    if let Some(p) = permissions {
        let _ = control.chat(json!({"permissions": p}));
    }
    result
}

fn run_tasks(control: &mut Control, o: &Options, tasks: &[Value], version: &str) -> Result<(), String> {
    let mut results: Vec<Value> = Vec::new();
    let mut kept = 0;
    let mut stdout = std::io::stdout().lock();
    let mut say = |line: String| writeln!(stdout, "{line}").map_err(|e| format!("stdout: {e}"));
    say(format!("bench-assistant v1: {} on SolidCraft {version}", o.model))?;
    say("| run | # | task | pass/fail | how it failed | seconds |\n|---|---|---|---|---|---|".into())?;
    for run in 1..=o.repeat {
        for &n in &o.tasks {
            let task = tasks.get(n - 1).ok_or("missing embedded task")?;
            // A fresh design and a fresh chat, with nothing of the last task still running.
            control.chat(json!({"stop": true}))?;
            control.execute("file.new", json!({"name": "Benchmark"}))?;
            for step in task["setup"].as_array().into_iter().flatten() {
                control.execute(step["command"].as_str().ok_or("embedded setup without a command")?, step["params"].clone())?;
            }
            control.chat(json!({"agent": AGENT_ID, "permissions": "all", "new_chat": true, "attach_view": false}))?;
            let before = control.inspect()?;
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
            let after = control.inspect()?;
            let failed_rules = check(&task["check"], &before, &after, &items);
            let passed = failed_rules.is_empty();
            let failure = if passed { "" } else { classify(&items, timed_out) };
            let seconds = began.elapsed().as_secs_f64();
            say(format!(
                "| {run} | {n} | {} | {} | {failure} | {seconds:.0} |",
                task["id"].as_str().unwrap_or_default(),
                if passed { "pass" } else { "fail" }
            ))?;
            let result = json!({
                "task": n, "id": task["id"], "prompt": task["prompt"], "repeat": run, "passed": passed, "failure": failure,
                "failed_rules": failed_rules, "timed_out": timed_out, "seconds": seconds, "check": task["check"],
                "items": items, "before": before, "after": after,
            });
            kept += result.to_string().len();
            if kept > MAX_RESULTS {
                return Err("the results passed 64 MiB; run fewer tasks or repetitions".into());
            }
            results.push(result);
            // Written after every task, so an interrupted run keeps what it measured.
            if let Some(path) = o.json {
                let all = json!({"test": "v1", "model": o.model, "base_url": o.base_url, "solidcraft": version, "results": results});
                write(path, &serde_json::to_string_pretty(&all).map_err(|e| e.to_string())?)?;
            }
            if let Some(path) = o.markdown {
                write(path, &(1..=run).map(|r| markdown(o.model, version, &results, r)).collect::<Vec<_>>().join("\n"))?;
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
