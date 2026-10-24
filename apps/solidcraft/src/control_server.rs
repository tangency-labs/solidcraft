//! Loopback JSON-lines control server: one request per line, one reply per line.
//! Agents and tests drive the app through it (adapted from CADCraft, MIT OR Apache-2.0).

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use solidcraft_engine::MAX_LINE;
use solidcraft_engine::control_auth;
use solidcraft_engine::control_pending;
use solidcraft_ui_egui::ControlRequest;

/// Connections served at once, so a flood must not be able to spawn threads without end.
const MAX_CONNECTIONS: usize = 16;
/// Lines that are not JSON before the connection is closed, so garbage cannot loop forever.
const MAX_BAD_LINES: usize = 8;

static OPEN: AtomicUsize = AtomicUsize::new(0);
/// The token files this launch wrote (its control port, the agent chat's endpoint), removed
/// when the app ends.
static TOKEN_FILES: std::sync::Mutex<Vec<std::path::PathBuf>> = std::sync::Mutex::new(Vec::new());

/// A new random token: 32 bytes from the operating system, as hex.
fn new_token() -> Option<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).ok()?;
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Remove this launch's token file (at exit).
pub fn remove_token_file() {
    if let Ok(files) = TOKEN_FILES.lock() {
        for p in files.iter() {
            let _ = std::fs::remove_file(p);
        }
    }
    if let Some(port) = APP_PORT.lock().ok().and_then(|p| *p) {
        control_auth::remove_current_port(port);
    }
}

/// The port the app's own channel listens on, for removing the discovery file at exit.
static APP_PORT: std::sync::Mutex<Option<u16>> = std::sync::Mutex::new(None);

pub fn start(listen_on: control_auth::Listen, ctx: egui::Context) -> Receiver<ControlRequest> {
    let (tx, rx) = channel::<ControlRequest>();
    // Every connection must show the token before anything runs (docs/control-protocol.md).
    match control_auth::env_token().or_else(new_token) {
        Some(token) => {
            let listener = match listen_on {
                control_auth::Listen::Exact(port) => TcpListener::bind(("127.0.0.1", port)),
                control_auth::Listen::Default => control_auth::bind_with_fallback(control_auth::DEFAULT_PORT, control_auth::PORT_TRIES),
            };
            match listener {
                Ok(l) => {
                    if let Some(port) = listen_on_listener(l, token, tx, ctx, false) {
                        if matches!(listen_on, control_auth::Listen::Default) {
                            eprintln!("solidcraft: control channel on the default port 127.0.0.1:{port} (Preferences > Assistant turns it off)");
                        }
                        // The discovery file: where `solidcraft-cli mcp` finds this app with no arguments.
                        match control_auth::write_current_port(port) {
                            Ok(_) => {
                                if let Ok(mut p) = APP_PORT.lock() {
                                    *p = Some(port);
                                }
                            }
                            Err(e) => eprintln!("solidcraft: could not write the control discovery file ({e})"),
                        }
                    }
                }
                Err(e) => eprintln!("solidcraft: control server failed to bind 127.0.0.1: {e}"),
            }
        }
        None => eprintln!("solidcraft: control server not started: no random token available"),
    }
    rx
}

/// The agent chat's own endpoint for its agent's MCP server (docs/agent-chat.md): a free loopback
/// port with a fresh token of its own (never `SOLIDCRAFT_CONTROL_TOKEN`), written to the usual
/// token file, where the MCP server finds it, so the token never passes through the agent. Its
/// requests go to `tx`, where the chat's gate reads them; one may wait long for the user's Allow.
pub fn listen_agent(tx: Sender<ControlRequest>, ctx: egui::Context) -> Option<u16> {
    let Some(token) = new_token() else {
        eprintln!("solidcraft: agent chat endpoint not started: no random token available");
        return None;
    };
    let listener = match TcpListener::bind(("127.0.0.1", 0)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("solidcraft: control server failed to bind 127.0.0.1: {e}");
            return None;
        }
    };
    listen_on_listener(listener, token, tx, ctx, true)
}

/// Serve `listener` (loopback) with `token`; the port it listens on.
/// `patient`: every request may wait as long as a batch (a person decides on it).
fn listen_on_listener(listener: TcpListener, token: String, tx: Sender<ControlRequest>, ctx: egui::Context, patient: bool) -> Option<u16> {
    let port = listener.local_addr().ok()?.port();
    match control_auth::token_path(port) {
        Some(path) => match control_auth::write_token_file(&path, &token) {
            Ok(()) => {
                eprintln!("solidcraft: control server listening on 127.0.0.1:{port} (token in {})", path.display());
                if let Ok(mut files) = TOKEN_FILES.lock() {
                    files.push(path);
                }
            }
            Err(e) => eprintln!(
                "solidcraft: control server listening on 127.0.0.1:{port}, but its token file {} could not be written ({e}); \
                 clients need SOLIDCRAFT_CONTROL_TOKEN",
                path.display()
            ),
        },
        None => {
            eprintln!("solidcraft: control server listening on 127.0.0.1:{port}; no data folder for its token, clients need SOLIDCRAFT_CONTROL_TOKEN")
        }
    }
    let token = std::sync::Arc::new(token);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            if OPEN.fetch_add(1, Ordering::Relaxed) >= MAX_CONNECTIONS {
                OPEN.fetch_sub(1, Ordering::Relaxed);
                let mut refused = stream;
                let _ = writeln!(refused, "{}", json!({"ok": false, "error": "too many connections"}));
                continue;
            }
            let tx = tx.clone();
            let ctx = ctx.clone();
            let token = token.clone();
            std::thread::spawn(move || {
                serve(stream, &token, tx, ctx, patient);
                OPEN.fetch_sub(1, Ordering::Relaxed);
            });
        }
    });
    Some(port)
}

/// One line without its newline, or `None` at the end of the stream. A client that sends
/// gigabytes with no newline would otherwise make this thread allocate until the process is
/// killed, so the line is refused as soon as it passes `max`.
fn read_line(r: &mut impl BufRead, max: usize) -> std::io::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    loop {
        let (done, used) = {
            let available = r.fill_buf()?;
            if available.is_empty() {
                return Ok((!line.is_empty()).then_some(line));
            }
            match available.iter().position(|b| *b == b'\n') {
                Some(i) => {
                    line.extend_from_slice(available.get(..i).unwrap_or(&[]));
                    (true, i + 1)
                }
                None => {
                    line.extend_from_slice(available);
                    (false, available.len())
                }
            }
        };
        r.consume(used);
        if done {
            return Ok(Some(line));
        }
        if line.len() > max {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "request too large"));
        }
    }
}

/// The reply to a connection that didn't show the token; the connection is then closed.
fn unauthorized() -> Value {
    json!({"ok": false, "error": "unauthorized: send the control token as \"token\" (see docs/control-protocol.md)"})
}

/// Refuse the connection: the reply, then the end of what we send, then what the client had
/// already sent read and dropped (a little, briefly). Closing a socket with unread input resets
/// the connection, and a reset can reach the client before it has read the refusal.
fn refuse(out: &mut TcpStream, reader: &mut BufReader<TcpStream>) {
    use std::io::Read;
    let _ = writeln!(out, "{}", unauthorized());
    let _ = out.shutdown(std::net::Shutdown::Write);
    let _ = reader.get_ref().set_read_timeout(Some(Duration::from_millis(500)));
    let _ = std::io::copy(&mut reader.by_ref().take(64 * 1024), &mut std::io::sink());
}

/// How long a request waits for the app's reply: most requests, and a batch (or every request,
/// on a patient connection).
#[derive(Clone, Copy)]
struct Waits {
    short: Duration,
    long: Duration,
}

const WAITS: Waits = Waits { short: Duration::from_secs(60), long: Duration::from_secs(3600) };

/// A request that timed out but is still running in the app (#1618): its id and method, and where
/// its reply will land.
struct Pending {
    id: Value,
    method: String,
    /// The request in messages ("`engine.execute` (solid.fillet)").
    what: String,
    reply: Receiver<Value>,
}

/// Every request the app has been sent and not answered yet, from every connection, for
/// `control.running` (#3654). Each connection's own thread answers that, so it never waits behind
/// a running request.
struct Running {
    list: Mutex<Vec<InFlight>>,
    connections: AtomicU64,
    keys: AtomicU64,
}

/// What `control.running` says about a request: never its parameters beyond the command id.
struct InFlight {
    key: u64,
    connection: u64,
    id: Value,
    method: String,
    command: Option<&'static str>,
    steps: Option<usize>,
    since: Instant,
    started: Arc<AtomicBool>,
}

/// The app's requests, from both of its listeners.
static RUNNING: Running = Running::new();

impl Running {
    const fn new() -> Self {
        Running { list: Mutex::new(Vec::new()), connections: AtomicU64::new(0), keys: AtomicU64::new(0) }
    }

    fn lock(&self) -> MutexGuard<'_, Vec<InFlight>> {
        self.list.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// List `req`, sent on `connection`, until the returned ticket is dropped.
    fn add(&'static self, connection: u64, id: &Value, req: &ControlRequest) -> Ticket {
        let key = self.keys.fetch_add(1, Ordering::Relaxed);
        // Ids and methods come from the client: only short ones are repeated to the others.
        let id = match id {
            Value::Number(_) => id.clone(),
            Value::String(s) if s.len() <= 64 => id.clone(),
            _ => Value::Null,
        };
        let p = &req.params;
        let command = match req.method.as_str() {
            "engine.execute" | "command" => p.get("command").and_then(Value::as_str).and_then(solidcraft_engine::find_command).map(|c| c.id),
            _ => None,
        };
        let steps = match req.method.as_str() {
            "engine.batch" | "engine.script" => p.get("commands").and_then(Value::as_array).map(Vec::len),
            _ => None,
        };
        let method = req.method.chars().take(64).collect();
        let started = req.started.clone();
        self.lock().push(InFlight { key, connection, id, method, command, steps, since: Instant::now(), started });
        Ticket { running: self, key }
    }

    /// The `control.running` result, as `asker`'s connection sees it.
    fn result(&self, asker: u64) -> Value {
        let rows: Vec<Value> = self
            .lock()
            .iter()
            .map(|e| {
                let mut r = json!({
                    "connection": e.connection,
                    "id": e.id,
                    "method": e.method,
                    "running_s": (e.since.elapsed().as_secs_f64() * 10.0).round() / 10.0,
                    "queued": !e.started.load(Ordering::Relaxed),
                });
                if let Some(c) = e.command {
                    r["command"] = json!(c);
                }
                if let Some(n) = e.steps {
                    r["steps"] = json!(n);
                }
                if e.connection == asker {
                    r["self"] = json!(true);
                }
                r
            })
            .collect();
        json!({"running": rows})
    }
}

/// A listed request; dropped when its reply arrives.
struct Ticket {
    running: &'static Running,
    key: u64,
}

impl Drop for Ticket {
    fn drop(&mut self) {
        let key = self.key;
        self.running.lock().retain(|e| e.key != key);
    }
}

/// The reply to a timed-out request, received on a thread of its own so the request stays listed
/// until the app answers, also once its connection has closed (#3654).
fn watch(reply: Receiver<Value>, ticket: Ticket) -> Receiver<Value> {
    let (late_tx, late_rx) = channel();
    // A small thread per timed-out request (each connection has at most one). If none can be
    // made, the reply is lost and `control.pending` says the app ended the request without one.
    // The reply is passed on before the request leaves the list: once `control.running` no longer
    // shows it, its connection's `control.pending` has it.
    let _ = std::thread::Builder::new().stack_size(64 * 1024).spawn(move || {
        if let Ok(v) = reply.recv() {
            let _ = late_tx.send(v);
        }
        drop(late_tx);
        drop(ticket);
    });
    late_rx
}

/// The reply text for a request that outlasts its wait. A request that only reads changes
/// nothing when it finishes; an export or a save still writes its file, and any other request
/// still applies, as it was not cancelled.
fn timeout_message(what: &str, effect: control_pending::Effect) -> String {
    match effect {
        control_pending::Effect::ReadsOnly => {
            format!("timeout: {what} is still running in the app ({}); ask `control.pending` for its answer", effect.when_done())
        }
        control_pending::Effect::WritesFile => format!(
            "timeout: {what} is still running in the app and will still write the file when it finishes (it was not cancelled: \
             don't send it again; the design does not change); ask `control.pending` for its result"
        ),
        control_pending::Effect::Changes => format!(
            "timeout: {what} is still running in the app and will still apply when it finishes (it was not cancelled: don't send it \
             again); ask `control.pending` for its result"
        ),
    }
}

/// The reply text for a request refused because an earlier one on the connection still runs.
fn busy_message(p: &Pending, method: &str) -> String {
    let method: String = method.chars().take(128).collect();
    format!(
        "busy: request {} ({}) is still running, so `{method}` was not run; ask `control.pending` for that request's result, then \
         send this one again",
        p.id, p.what
    )
}

fn serve(stream: TcpStream, token: &str, tx: Sender<ControlRequest>, ctx: egui::Context, patient: bool) {
    serve_with(stream, token, tx, ctx, patient, WAITS, &RUNNING);
}

fn serve_with(
    stream: TcpStream,
    token: &str,
    tx: Sender<ControlRequest>,
    ctx: egui::Context,
    patient: bool,
    waits: Waits,
    running: &'static Running,
) {
    let connection = running.connections.fetch_add(1, Ordering::Relaxed) + 1;
    let Ok(read) = stream.try_clone() else { return };
    let mut out = stream;
    let mut reader = BufReader::new(read);
    let mut bad = 0usize;
    let mut authenticated = false;
    let mut pending: Option<Pending> = None;
    loop {
        let line = match read_line(&mut reader, MAX_LINE) {
            Ok(Some(l)) => String::from_utf8_lossy(&l).into_owned(),
            Ok(None) => break,
            Err(e) => {
                let _ = writeln!(out, "{}", json!({"ok": false, "error": e.to_string()}));
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => {
                // The first request must carry the token; later ones on the same connection may.
                if !authenticated {
                    match msg.get("token").and_then(Value::as_str) {
                        Some(t) if control_auth::tokens_match(t, token) => authenticated = true,
                        _ => {
                            refuse(&mut out, &mut reader);
                            break;
                        }
                    }
                }
                let id = msg.get("id").cloned().unwrap_or(Value::Null);
                let method = msg.get("method").and_then(Value::as_str).unwrap_or("").to_string();
                let params = msg.get("params").cloned().unwrap_or(json!({}));
                // A request that timed out may have finished since (#1618). `control.running` only
                // looks, so it leaves that reply where it is.
                let late = pending.as_ref().filter(|_| method != "control.running").and_then(|p| match p.reply.try_recv() {
                    Ok(reply) => Some(reply),
                    Err(TryRecvError::Disconnected) => Some(json!({"ok": false, "error": "the app ended the request without a reply"})),
                    Err(TryRecvError::Empty) => None,
                });
                let mut r = if method == "control.running" {
                    json!({"ok": true, "result": running.result(connection)})
                } else if method == "control.pending" {
                    match (pending.take(), late) {
                        (None, _) => json!({"ok": true, "result": {"pending": null}}),
                        (Some(p), Some(reply)) => json!({"ok": true, "result": {"pending": p.id, "method": p.method, "done": true, "reply": reply}}),
                        (Some(p), None) => {
                            let r = json!({"ok": true, "result": {"pending": p.id, "method": p.method, "done": false}});
                            pending = Some(p);
                            r
                        }
                    }
                } else if let Some(p) = pending.as_ref().filter(|_| late.is_none()) {
                    // Nothing more is queued behind it, to run unseen when it ends.
                    json!({"ok": false, "busy": true, "error": busy_message(p, &method)})
                } else {
                    if late.is_some() {
                        // Finished, and the client moved on without asking: it is dropped.
                        pending = None;
                    }
                    // A batch replaces many short requests: the whole of it may take long (#743).
                    let wait = if patient || method == "engine.batch" { waits.long } else { waits.short };
                    let what = control_pending::describe(&method, &params);
                    let effect = control_pending::effect(&method, &params);
                    let (req, rrx) = ControlRequest::new(method.clone(), params);
                    let ticket = running.add(connection, &id, &req);
                    if tx.send(req).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                    match rrx.recv_timeout(wait) {
                        Ok(r) => r,
                        Err(_) => {
                            let r = json!({"ok": false, "running": true, "error": timeout_message(&what, effect)});
                            pending = Some(Pending { id: id.clone(), method, what, reply: watch(rrx, ticket) });
                            r
                        }
                    }
                };
                if let Some(o) = r.as_object_mut() {
                    o.insert("id".into(), id);
                }
                r
            }
            Err(_) if !authenticated => {
                refuse(&mut out, &mut reader);
                break;
            }
            Err(e) => {
                bad += 1;
                json!({"ok": false, "error": format!("bad JSON: {e}")})
            }
        };
        if writeln!(out, "{reply}").is_err() {
            break;
        }
        if bad >= MAX_BAD_LINES {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #1618: a request that outlasts its wait says it is still running; the connection keeps its
    /// reply for `control.pending`, and refuses other requests as `busy` until it lands, so none
    /// is queued to run unseen behind it.
    #[test]
    fn a_timed_out_request_can_be_asked_for_later() {
        let token = "0123456789abcdef0123456789abcdef";
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = channel::<ControlRequest>();
        // The slow request is held until the test has seen it time out, so the test does not
        // race the clock on a loaded machine.
        let (gate_tx, gate_rx) = channel::<()>();
        let ran = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let seen = ran.clone();
        std::thread::spawn(move || {
            for req in rx {
                seen.lock().unwrap().push(req.method.clone());
                if req.method == "slow" {
                    let _ = gate_rx.recv();
                }
                let _ = req.reply.send(json!({"ok": true, "result": req.method}));
            }
        });
        let waits = Waits { short: Duration::from_millis(100), long: Duration::from_millis(100) };
        static RUNNING: Running = Running::new();
        std::thread::spawn(move || {
            if let Some(stream) = listener.incoming().flatten().next() {
                serve_with(stream, token, tx, egui::Context::default(), false, waits, &RUNNING);
            }
        });
        let s = TcpStream::connect(addr).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut out = s.try_clone().unwrap();
        let mut lines = BufReader::new(s);
        let mut ask = |v: Value| -> Value {
            writeln!(out, "{v}").unwrap();
            let mut l = String::new();
            lines.read_line(&mut l).unwrap();
            serde_json::from_str(&l).unwrap()
        };
        let r = ask(json!({"id": 1, "method": "slow", "token": token}));
        assert!(r["ok"] == false && r["running"] == true && r["error"].as_str().is_some_and(|e| e.starts_with("timeout")), "{r}");
        let r = ask(json!({"id": 2, "method": "ui.view"}));
        assert!(r["busy"] == true && r["id"] == 2, "{r}");
        let r = ask(json!({"id": 3, "method": "control.pending"}));
        assert_eq!((&r["result"]["pending"], &r["result"]["done"]), (&json!(1), &json!(false)), "{r}");
        gate_tx.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let r = loop {
            let r = ask(json!({"id": 4, "method": "control.pending"}));
            if r["result"]["done"] == true || std::time::Instant::now() > deadline {
                break r;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(
            (&r["result"]["pending"], &r["result"]["done"], &r["result"]["reply"]["result"]),
            (&json!(1), &json!(true), &json!("slow")),
            "{r}"
        );
        let r = ask(json!({"id": 5, "method": "control.pending"}));
        assert_eq!(r["result"]["pending"], Value::Null, "{r}");
        let r = ask(json!({"id": 6, "method": "ui.view"}));
        assert_eq!((&r["ok"], &r["result"]), (&json!(true), &json!("ui.view")), "{r}");
        assert_eq!(*ran.lock().unwrap(), ["slow", "ui.view"], "the busy request never reached the app");
    }

    /// #3654: `control.running` answers on every connection, at once, with what any connection
    /// has the app running or queued (command ids, never parameters); a timed-out request stays
    /// listed until the app answers it, and an idle app lists nothing.
    #[test]
    fn every_connection_sees_what_the_app_is_running() {
        static RUNNING: Running = Running::new();
        let token = "0123456789abcdef0123456789abcdef";
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = channel::<ControlRequest>();
        // The window: one request at a time, as `drain_control` takes them; the export is held
        // until the test lets it go.
        let (gate_tx, gate_rx) = channel::<()>();
        std::thread::spawn(move || {
            for req in rx {
                req.started.store(true, Ordering::Relaxed);
                if req.method == "engine.execute" {
                    let _ = gate_rx.recv();
                }
                let _ = req.reply.send(json!({"ok": true, "result": req.method}));
            }
        });
        let waits = Waits { short: Duration::from_millis(100), long: Duration::from_secs(60) };
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let tx = tx.clone();
                std::thread::spawn(move || serve_with(stream, token, tx, egui::Context::default(), false, waits, &RUNNING));
            }
        });
        let open = || {
            let s = TcpStream::connect(addr).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            let mut out = s.try_clone().unwrap();
            let mut lines = BufReader::new(s);
            move |v: Value| -> Value {
                writeln!(out, "{v}").unwrap();
                let mut l = String::new();
                lines.read_line(&mut l).unwrap();
                serde_json::from_str(&l).unwrap()
            }
        };
        let running = |ask: &mut dyn FnMut(Value) -> Value, until: &dyn Fn(&[Value]) -> bool| {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let r = ask(json!({"id": 0, "method": "control.running", "token": token}));
                let rows = r["result"]["running"].as_array().cloned().unwrap_or_default();
                if until(&rows) || Instant::now() > deadline {
                    return (r, rows);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        let mut watcher = open();
        let (r, rows) = running(&mut watcher, &|_| true);
        assert!(r["ok"] == true && rows.is_empty(), "idle: {r}");

        let mut export = open();
        let path = "/private/part.step";
        let params = json!({"command": "file.export", "params": {"path": path}});
        let r = export(json!({"id": 7, "method": "engine.execute", "params": params, "token": token}));
        assert_eq!(r["running"], true, "{r}");
        // A batch from another connection waits behind the export.
        let mut batch = open();
        let queued = std::thread::spawn(move || {
            let steps = json!([{"command": "solid.box"}, {"command": "solid.box"}]);
            batch(json!({"id": "b", "method": "engine.batch", "params": {"commands": steps}, "token": token}))
        });
        // The window may not have taken the export yet, even though it timed out.
        let (r, rows) = running(&mut watcher, &|rows| rows.len() == 2 && rows[0]["queued"] == false);
        assert_eq!(rows.len(), 2, "{r}");
        let (e, b) = (&rows[0], &rows[1]);
        assert_eq!(
            (&e["id"], &e["method"], &e["command"], &e["queued"]),
            (&json!(7), &json!("engine.execute"), &json!("file.export"), &json!(false)),
            "{r}"
        );
        assert!(e["running_s"].as_f64().is_some_and(|s| s >= 0.1), "{r}");
        assert_eq!((&b["id"], &b["method"], &b["steps"], &b["queued"]), (&json!("b"), &json!("engine.batch"), &json!(2), &json!(true)), "{r}");
        assert_ne!(e["connection"], b["connection"], "{r}");
        assert!(rows.iter().all(|row| row.get("self").is_none()), "{r}");
        assert!(!r.to_string().contains(path), "parameters stay with their connection: {r}");
        // The connection whose request timed out may ask too, and sees its own marked.
        let r = export(json!({"id": 8, "method": "control.running"}));
        let own: Vec<&Value> = r["result"]["running"].as_array().unwrap().iter().filter(|row| row["self"] == true).collect();
        assert_eq!(own.len(), 1, "{r}");
        assert_eq!(own[0]["command"], "file.export", "{r}");

        gate_tx.send(()).unwrap();
        assert_eq!(queued.join().unwrap()["result"], "engine.batch");
        let (r, rows) = running(&mut watcher, &|rows| rows.is_empty());
        assert!(rows.is_empty(), "done: {r}");
        // Asking `control.running` did not take the export's reply from its connection.
        let r = export(json!({"id": 9, "method": "control.pending"}));
        assert_eq!((&r["result"]["pending"], &r["result"]["done"]), (&json!(7), &json!(true)), "{r}");
    }

    /// A timed-out request that only reads is not said to "still apply", nor is one that only
    /// writes a file (an export, #1989); a change is, by name.
    #[test]
    fn timeout_text_tells_reads_from_changes() {
        let says = |method: &str, p: Value| timeout_message(&control_pending::describe(method, &p), control_pending::effect(method, &p));
        let read = says("ui.inspect", json!({}));
        assert!(read.starts_with("timeout") && read.contains("only reads") && !read.contains("will still apply"), "{read}");
        let read = says("engine.execute", json!({"command": "sketch.inspect"}));
        assert!(read.contains("only reads") && read.contains("sketch.inspect"), "{read}");
        let change = says("engine.execute", json!({"command": "solid.fillet"}));
        assert!(change.starts_with("timeout") && change.contains("will still apply") && change.contains("solid.fillet"), "{change}");
        let export = says("engine.execute", json!({"command": "file.export", "params": {"path": "/tmp/wall.step"}}));
        assert!(export.contains("will still write the file") && !export.contains("apply") && export.contains("file.export"), "{export}");
        let save = says("engine.execute", json!({"command": "file.save"}));
        assert!(save.contains("will still write the file") && !save.contains("apply"), "{save}");
        let render = says("ui.render", json!({"path": "/tmp/a.png"}));
        assert!(render.contains("will still write the file"), "{render}");
    }

    /// #205: a connection runs nothing until a request carries the launch's token; a wrong or
    /// missing token, or garbage, is refused and the connection closed. Once shown, the token
    /// need not be repeated on that connection.
    #[test]
    fn requests_run_only_after_the_token() {
        use std::io::Read;
        let token = "0123456789abcdef0123456789abcdef";
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = channel::<ControlRequest>();
        let ran = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let seen = ran.clone();
        std::thread::spawn(move || {
            for req in rx {
                seen.lock().unwrap().push(req.method.clone());
                let _ = req.reply.send(json!({"ok": true, "result": null}));
            }
        });
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let tx = tx.clone();
                std::thread::spawn(move || serve(stream, token, tx, egui::Context::default(), false));
            }
        });
        let session = |lines: &[String]| {
            let mut s = TcpStream::connect(addr).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            for l in lines {
                let _ = writeln!(s, "{l}");
            }
            let _ = s.shutdown(std::net::Shutdown::Write);
            let mut text = String::new();
            let _ = s.read_to_string(&mut text);
            text.lines().map(|l| serde_json::from_str::<Value>(l).unwrap()).collect::<Vec<_>>()
        };
        for first in [
            json!({"id": 1, "method": "app.quit", "params": {"force": true}}).to_string(),
            json!({"id": 1, "method": "app.quit", "token": "0123456789abcdef0123456789abcdee"}).to_string(),
            json!({"id": 1, "method": "app.quit", "token": "0123"}).to_string(),
            json!({"id": 1, "method": "app.quit", "token": 7}).to_string(),
            "not json".to_string(),
        ] {
            let replies = session(&[first.clone(), json!({"id": 2, "method": "app.quit", "token": token}).to_string()]);
            assert_eq!(replies.len(), 1, "{first}: closed after refusing: {replies:?}");
            assert!(replies[0]["error"].as_str().is_some_and(|e| e.starts_with("unauthorized")), "{first}: {replies:?}");
        }
        assert!(ran.lock().unwrap().is_empty(), "nothing ran without the token");
        let replies = session(&[
            json!({"id": 1, "method": "ui.inspect", "token": token}).to_string(),
            json!({"id": 2, "method": "engine.commands"}).to_string(),
        ]);
        assert_eq!(replies.len(), 2, "{replies:?}");
        assert!(replies.iter().all(|r| r["ok"] == true), "{replies:?}");
        assert_eq!(*ran.lock().unwrap(), ["ui.inspect", "engine.commands"]);
    }

    /// A request longer than the cap is refused instead of allocated.
    #[test]
    fn lines_past_the_cap_are_refused() {
        let flood = vec![b'A'; MAX_LINE + 2];
        let mut r = BufReader::new(flood.as_slice());
        let e = read_line(&mut r, MAX_LINE).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::InvalidData);
        assert!(e.to_string().contains("request too large"), "{e}");
    }

    /// Ordinary framing: lines split on newlines, blank lines kept, no trailing newline needed.
    #[test]
    fn lines_split_on_newlines() {
        let text = b"{\"a\":1}\n\n{\"b\":2}\nlast".to_vec();
        let mut r = BufReader::new(text.as_slice());
        let mut got = Vec::new();
        while let Some(line) = read_line(&mut r, MAX_LINE).unwrap() {
            got.push(String::from_utf8(line).unwrap());
        }
        assert_eq!(got, vec!["{\"a\":1}", "", "{\"b\":2}", "last"]);
        assert!(read_line(&mut r, MAX_LINE).unwrap().is_none());
    }

    /// A line that spans several reads of the underlying stream still arrives whole.
    #[test]
    fn lines_longer_than_one_buffer_arrive_whole() {
        let long = "x".repeat(200_000);
        let text = format!("{long}\n{long}\n").into_bytes();
        let mut r = BufReader::with_capacity(64, text.as_slice());
        assert_eq!(read_line(&mut r, MAX_LINE).unwrap().unwrap().len(), long.len());
        assert_eq!(read_line(&mut r, MAX_LINE).unwrap().unwrap().len(), long.len());
        assert!(read_line(&mut r, MAX_LINE).unwrap().is_none());
        // The same line against a smaller cap is refused.
        let mut r = BufReader::with_capacity(64, text.as_slice());
        assert!(read_line(&mut r, 1000).is_err());
    }
}
