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
use solidcraft_ui_egui::control::{DESIGN_WATCH, DesignWatch, Subscriber};
use solidcraft_ui_egui::control_job::{Cancel, Ticket as JobTicket};

/// Connections served at once, so a flood must not be able to spawn threads without end.
const MAX_CONNECTIONS: usize = 16;
/// Lines that are not JSON before the connection is closed, so garbage cannot loop forever.
const MAX_BAD_LINES: usize = 8;
/// How long a subscribed connection waits after a `design.changed` line before the next: what
/// changes meanwhile (a parameter dragged) goes out as one line, the latest (#6985).
const EVENT_GAP: Duration = Duration::from_millis(100);

static OPEN: AtomicUsize = AtomicUsize::new(0);
/// The token files this launch wrote (its control port, the agent chat's endpoint) and the
/// token in each, removed when the app ends unless another launch has written the file since.
static TOKEN_FILES: std::sync::Mutex<Vec<(std::path::PathBuf, String)>> = std::sync::Mutex::new(Vec::new());

/// A new random token: 32 bytes from the operating system, as hex.
fn new_token() -> Option<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).ok()?;
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Remove this launch's token files and discovery file (at exit), leaving any that another
/// launch has written since (#8794).
pub fn remove_token_file() {
    if let Ok(files) = TOKEN_FILES.lock() {
        for (path, token) in files.iter() {
            control_auth::remove_token_file(path, token);
        }
    }
    if let Some((port, token)) = APP_PORT.lock().ok().and_then(|p| p.clone()) {
        control_auth::remove_current_port(port, &token);
    }
}

/// The port the app's own channel listens on and its token, for removing the discovery file at exit.
static APP_PORT: std::sync::Mutex<Option<(u16, String)>> = std::sync::Mutex::new(None);

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
                    if let Some(port) = listen_on_listener(l, token.clone(), tx, ctx, false) {
                        if matches!(listen_on, control_auth::Listen::Default) {
                            eprintln!("solidcraft: control channel on the default port 127.0.0.1:{port} (Preferences > Assistant turns it off)");
                        }
                        // The discovery file: where `solidcraft-cli mcp` finds this app with no arguments.
                        match control_auth::write_current_port(port) {
                            Ok(_) => {
                                if let Ok(mut p) = APP_PORT.lock() {
                                    *p = Some((port, token));
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
                    files.push((path, token.clone()));
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
                let _ = send(&mut refused, &json!({"ok": false, "error": "too many connections"}));
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

/// A reply line, written to the socket at once. `writeln!` with a JSON value hands the socket each
/// of its pieces apart, a system call for every string, number and comma: half a second for a
/// design's component tree when it has thousands of components.
fn send(out: &mut impl Write, reply: &Value) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(reply).map_err(std::io::Error::other)?;
    line.push(b'\n');
    out.write_all(&line)
}

/// The `"token"` of a request line that is a JSON object, read without building the rest of it.
/// Until a connection shows the token, a line costs only its own bytes: parsed whole, a 16 MiB
/// line of `[0,0,…]` is hundreds of MB, and 16 such connections could exhaust memory before any
/// token was checked.
fn token_of(line: &str) -> Option<String> {
    struct Token;
    impl<'de> serde::de::Visitor<'de> for Token {
        type Value = Option<String>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a request object")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut token = None;
            while let Some(key) = map.next_key::<String>()? {
                if key == "token" {
                    token = Some(map.next_value::<String>()?);
                } else {
                    map.next_value::<serde::de::IgnoredAny>()?;
                }
            }
            Ok(token)
        }
    }
    let mut de = serde_json::Deserializer::from_str(line);
    let token = serde::Deserializer::deserialize_map(&mut de, Token).ok()?;
    de.end().ok()?;
    token
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
    let _ = send(out, &unauthorized());
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
    /// Where it is in the app, to cancel it (`control.cancel`).
    job: std::sync::Arc<JobTicket>,
}

/// `control.cancel {id?}`: cancel the request this connection still waits for (the one
/// `control.pending` reports; `id`, when given, must name it). A connection can only cancel its
/// own requests. Waiting its turn, it is removed; computing off the window thread, it is stopped
/// as the app's Stop stops it. Either way nothing of it is applied and the connection is free.
/// One that runs on the window thread can't be cancelled; one that finished is answered.
fn cancel(pending: &mut Option<Pending>, late: Option<Value>, id: Option<&Value>) -> Value {
    let id = id.filter(|v| !v.is_null());
    let mine = pending.as_ref().filter(|p| id.is_none_or(|id| *id == p.id));
    let Some(p) = mine else {
        return match id {
            None => json!({"ok": true, "result": {"cancelled": null, "note": "nothing to cancel: no request of this connection is running"}}),
            Some(id) => json!({"ok": false, "error": format!(
                "nothing to cancel: request {} is not running on this connection (a connection can only cancel its own requests)",
                id.to_string().chars().take(64).collect::<String>()
            )}),
        };
    };
    let (what, id) = (format!("request {} ({})", p.id, p.what), p.id.clone());
    let finished = |p: Pending, reply: Value| {
        json!({"ok": true, "result": {"cancelled": null, "pending": p.id, "method": p.method, "done": true, "reply": reply,
            "note": format!("nothing to cancel: {what} had already finished; its reply is here")}})
    };
    if let Some(reply) = late {
        return pending.take().map_or(Value::Null, |p| finished(p, reply));
    }
    let outcome = p.job.cancel();
    let note = match outcome {
        Cancel::Removed => format!("cancelled: {what} was still waiting its turn and will not run; nothing was applied"),
        Cancel::Stopped { writes_file: false } => format!("stopped: {what} was computing off the window thread; nothing was applied"),
        Cancel::Stopped { writes_file: true } => {
            format!("stopped: {what} was computing on a copy of the design, which is unchanged; its worker may still write the file")
        }
        Cancel::Already => format!("{what} was already cancelled or stopped; nothing of it was applied"),
        Cancel::Running => {
            // It may have just finished.
            if let Some(reply) = pending.as_ref().and_then(|p| p.reply.try_recv().ok()) {
                return pending.take().map_or(Value::Null, |p| finished(p, reply));
            }
            return json!({"ok": false, "cancellable": false, "error": format!(
                "cannot be cancelled: {what} already runs in the app (on its window thread, or as a file it opens) and will \
                 still finish; ask `control.pending` for its result. Only a request still waiting its turn, or a design change or \
                 export computing off the window thread (Preferences > AI), can be cancelled"
            )});
        }
    };
    let method = pending.take().map(|p| p.method);
    let how = match outcome {
        Cancel::Removed => "removed",
        Cancel::Stopped { .. } => "stopped",
        _ => "already",
    };
    json!({"ok": true, "result": {"cancelled": id, "method": method, "outcome": how, "note": note}})
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
             don't send it again; the design does not change); ask `control.pending` for its result, or `control.cancel` to cancel it"
        ),
        control_pending::Effect::Changes => format!(
            "timeout: {what} is still running in the app and will still apply when it finishes (it was not cancelled: don't send it \
             again); ask `control.pending` for its result, or `control.cancel` to cancel it"
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

/// A subscribed connection's socket, for its replies and its events: each line is written whole
/// and alone, so an event never lands inside a reply (#6985).
struct Lines {
    socket: TcpStream,
    turn: Mutex<()>,
}

impl Lines {
    fn send(&self, line: &Value) -> std::io::Result<()> {
        let _turn = self.turn.lock().unwrap_or_else(PoisonError::into_inner);
        send(&mut &self.socket, line)
    }
}

/// A connection's `control.subscribe` (#6985): its `design.changed` lines, sent from a thread of
/// their own until the connection ends.
struct Subscription {
    lines: Arc<Lines>,
    stop: Arc<AtomicBool>,
}

impl Subscription {
    /// Write `reply`, the window's answer to `control.subscribe` naming the design `seen`, on
    /// `out`, then send `design`'s changes after it: the reply goes out before any event. `None`,
    /// with nothing written, when no thread or socket was left for the events.
    fn start(
        design: &'static DesignWatch,
        counted: Subscriber,
        out: &TcpStream,
        seen: (String, u64),
        reply: &Value,
    ) -> Option<(Self, std::io::Result<()>)> {
        let lines = Arc::new(Lines { socket: out.try_clone().ok()?, turn: Mutex::new(()) });
        let stop = Arc::new(AtomicBool::new(false));
        let first = lines.turn.lock().unwrap_or_else(PoisonError::into_inner);
        let (l, s) = (lines.clone(), stop.clone());
        // The connection's seat on the watch rides the events thread: it leaves when the thread
        // does, so the next failed write to a closed client retires it even while the connection's
        // own thread is slow to see the end of the stream (#7226).
        std::thread::Builder::new().stack_size(64 * 1024).spawn(move || send_events(design, &l, &s, seen, counted)).ok()?;
        let sent = send(&mut &lines.socket, reply);
        drop(first);
        Some((Subscription { lines, stop }, sent))
    }
}

/// The design a `control.subscribe` reply names.
fn design_of(reply: &Value) -> Option<(String, u64)> {
    let result = reply.get("result")?;
    Some((result.get("document_id")?.as_str()?.to_string(), result.get("revision")?.as_u64()?))
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // The socket closes now, not when the thread next looks; a line blocked on a client that
        // stopped reading ends too.
        let _ = self.lines.socket.shutdown(std::net::Shutdown::Both);
    }
}

/// The `design.changed` lines of a subscribed connection, each time the design differs from the
/// last one it was told of (`seen`), until `stop`. Owns the connection's seat on the watch
/// (`counted`), so the seat leaves exactly when this thread does: on `stop`, or on the first
/// write a closed client refuses (#7226).
fn send_events(design: &DesignWatch, lines: &Lines, stop: &AtomicBool, mut seen: (String, u64), _counted: Subscriber) {
    while !stop.load(Ordering::Relaxed) {
        let Some(now) = design.wait(&seen, stop, Duration::from_secs(1)).filter(|now| *now != seen) else { continue };
        if lines.send(&json!({"event": "design.changed", "params": {"document_id": now.0, "revision": now.1}})).is_err() {
            break;
        }
        seen = now;
        std::thread::sleep(EVENT_GAP);
    }
}

/// Write `reply` on the connection: through its subscription once it has one.
fn answer(out: &mut TcpStream, events: Option<&Subscription>, reply: &Value) -> std::io::Result<()> {
    match events {
        Some(e) => e.lines.send(reply),
        None => send(out, reply),
    }
}

fn serve(stream: TcpStream, token: &str, tx: Sender<ControlRequest>, ctx: egui::Context, patient: bool) {
    serve_with(stream, token, tx, ctx, patient, WAITS, &RUNNING, &DESIGN_WATCH);
}

#[allow(clippy::too_many_arguments)]
fn serve_with(
    stream: TcpStream,
    token: &str,
    tx: Sender<ControlRequest>,
    ctx: egui::Context,
    patient: bool,
    waits: Waits,
    running: &'static Running,
    design: &'static DesignWatch,
) {
    let connection = running.connections.fetch_add(1, Ordering::Relaxed) + 1;
    let Ok(read) = stream.try_clone() else { return };
    let mut out = stream;
    let mut reader = BufReader::new(read);
    let mut bad = 0usize;
    let mut authenticated = false;
    let mut pending: Option<Pending> = None;
    let mut events: Option<Subscription> = None;
    loop {
        // A first `control.subscribe` on its way to the window (#6985).
        let mut joining = None;
        let line = match read_line(&mut reader, MAX_LINE) {
            Ok(Some(l)) => String::from_utf8_lossy(&l).into_owned(),
            Ok(None) => break,
            Err(e) => {
                let _ = answer(&mut out, events.as_ref(), &json!({"ok": false, "error": e.to_string()}));
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        // The first request must carry the token; later ones on the same connection may.
        if !authenticated {
            match token_of(&line) {
                Some(t) if control_auth::tokens_match(&t, token) => authenticated = true,
                _ => {
                    refuse(&mut out, &mut reader);
                    break;
                }
            }
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => {
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
                } else if method == "control.cancel" {
                    let r = cancel(&mut pending, late, params.get("id"));
                    ctx.request_repaint();
                    r
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
                } else if method == "control.subscribe" && events.is_some() {
                    if late.is_some() {
                        // As below: finished, and the client moved on without asking.
                        pending = None;
                    }
                    // Its events already say how the design moves; a second answer could only
                    // arrive behind a newer one.
                    json!({"ok": false, "error": "already subscribed: this connection is sent design.changed lines; ask document.revision for the design now"})
                } else {
                    if late.is_some() {
                        // Finished, and the client moved on without asking: it is dropped.
                        pending = None;
                    }
                    // A batch replaces many short requests: the whole of it may take long (#743).
                    let wait = if patient || method == "engine.batch" { waits.long } else { waits.short };
                    let what = control_pending::describe(&method, &params);
                    let effect = control_pending::effect(&method, &params);
                    // Counted before the window answers, so it publishes every change from its
                    // answer on.
                    joining = (method == "control.subscribe").then(|| design.join());
                    let (req, rrx) = ControlRequest::new(method.clone(), params);
                    let ticket = running.add(connection, &id, &req);
                    let job = req.ticket.clone();
                    if tx.send(req).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                    match rrx.recv_timeout(wait) {
                        Ok(r) => r,
                        Err(_) => {
                            let r = json!({"ok": false, "running": true, "error": timeout_message(&what, effect)});
                            pending = Some(Pending { id: id.clone(), method, what, reply: watch(rrx, ticket), job });
                            r
                        }
                    }
                };
                if let Some(o) = r.as_object_mut() {
                    o.insert("id".into(), id);
                }
                r
            }
            Err(e) => {
                bad += 1;
                json!({"ok": false, "error": format!("bad JSON: {e}")})
            }
        };
        // Subscribed once the window named the design (not when it refused or timed out).
        let sent = match joining.and_then(|counted| design_of(&reply).map(|seen| (counted, seen))) {
            Some((counted, seen)) => match Subscription::start(design, counted, &out, seen, &reply) {
                Some((subscription, sent)) => {
                    events = Some(subscription);
                    sent
                }
                None => {
                    send(&mut out, &json!({"id": reply.get("id"), "ok": false, "error": "not subscribed: no thread or socket left for the events"}))
                }
            },
            None => answer(&mut out, events.as_ref(), &reply),
        };
        if sent.is_err() {
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
                serve_with(stream, token, tx, egui::Context::default(), false, waits, &RUNNING, &DESIGN_WATCH);
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
                std::thread::spawn(move || serve_with(stream, token, tx, egui::Context::default(), false, waits, &RUNNING, &DESIGN_WATCH));
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

    /// #6985: a connection that sends `control.subscribe` gets the design in the reply, then a
    /// `design.changed` line each time the window publishes another: never before the reply,
    /// none for the same design, the latest after a burst, each line whole between its replies.
    /// A second subscribe is refused without reaching the window. A connection that did not
    /// subscribe gets no events, and one that closed is no longer counted.
    #[test]
    fn subscribed_connections_are_told_of_design_changes() {
        static RUNNING: Running = Running::new();
        static WATCH: DesignWatch = DesignWatch::new();
        let token = "0123456789abcdef0123456789abcdef";
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = channel::<ControlRequest>();
        // The window: it answers `control.subscribe` as the app does, publishing the design first;
        // then the design changes at once, before the reply may have been written.
        std::thread::spawn(move || {
            for req in rx {
                if req.method == "control.subscribe" {
                    WATCH.publish("d-1", 3);
                    let _ = req.reply.send(json!({"ok": true, "result": {"document_id": "d-1", "revision": 3}}));
                    WATCH.publish("d-1", 4);
                } else {
                    let _ = req.reply.send(json!({"ok": true, "result": req.method}));
                }
            }
        });
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let tx = tx.clone();
                std::thread::spawn(move || serve_with(stream, token, tx, egui::Context::default(), false, WAITS, &RUNNING, &WATCH));
            }
        });
        let open = || {
            let s = TcpStream::connect(addr).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            (s.try_clone().unwrap(), BufReader::new(s))
        };
        let next = |lines: &mut BufReader<TcpStream>| -> Value {
            let mut l = String::new();
            lines.read_line(&mut l).unwrap();
            serde_json::from_str(&l).unwrap()
        };
        let changed = |id: &str, revision: u64| json!({"event": "design.changed", "params": {"document_id": id, "revision": revision}});

        let (mut quiet_out, mut quiet) = open();
        writeln!(quiet_out, "{}", json!({"id": 1, "method": "ui.view", "token": token})).unwrap();
        assert_eq!(next(&mut quiet)["id"], 1);

        let (mut out, mut lines) = open();
        writeln!(out, "{}", json!({"id": 1, "method": "control.subscribe", "token": token})).unwrap();
        assert_eq!(next(&mut lines), json!({"id": 1, "ok": true, "result": {"document_id": "d-1", "revision": 3}}));
        assert_eq!(next(&mut lines), changed("d-1", 4), "the change after the window's answer comes after the reply");
        assert!(WATCH.subscribed());
        // The design it was told of is no news; another revision is.
        WATCH.publish("d-1", 4);
        WATCH.publish("d-1", 5);
        assert_eq!(next(&mut lines), changed("d-1", 5));
        // A burst ends with its latest.
        for revision in 6..=10 {
            WATCH.publish("d-1", revision);
        }
        let mut last = 5;
        while last < 10 {
            let e = next(&mut lines);
            let revision = e["params"]["revision"].as_u64().unwrap();
            assert!(e["event"] == "design.changed" && revision > last, "{e} after {last}");
            last = revision;
        }
        // Another tab: another id, whatever its revision.
        WATCH.publish("d-2", 2);
        assert_eq!(next(&mut lines), changed("d-2", 2));
        // Subscribed already: refused here (the window would have published d-1 again).
        writeln!(out, "{}", json!({"id": 2, "method": "control.subscribe"})).unwrap();
        let r = next(&mut lines);
        assert!(r["id"] == 2 && r["ok"] == false && r["error"].as_str().is_some_and(|e| e.starts_with("already subscribed")), "{r}");
        // Replies still come, whole, and nothing else was sent meanwhile.
        writeln!(out, "{}", json!({"id": 3, "method": "ui.view"})).unwrap();
        assert_eq!(next(&mut lines), json!({"id": 3, "ok": true, "result": "ui.view"}));
        writeln!(quiet_out, "{}", json!({"id": 2, "method": "ui.view"})).unwrap();
        assert_eq!(next(&mut quiet), json!({"id": 2, "ok": true, "result": "ui.view"}), "no events without subscribing");
        // Closed, it is no longer counted. Each publish asks the events thread for one more line,
        // and its write to the closed connection is what retires the seat; that keeps the wait off
        // the connection's own thread, which anything else this process is doing may hold up past
        // the deadline (#7226).
        drop((out, lines));
        let mut revision = 11;
        let deadline = Instant::now() + Duration::from_secs(10);
        while WATCH.subscribed() && Instant::now() < deadline {
            WATCH.publish("d-1", revision);
            revision += 1;
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!WATCH.subscribed());
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

    /// The token is read from a request object without building the rest of it: other fields,
    /// however large or nested, are only skipped. Anything but an object with a string token is
    /// no token.
    #[test]
    fn token_is_read_without_parsing_the_request() {
        assert_eq!(token_of(r#"{"id":1,"token":"abc","params":{"a":[1,[2,{"b":null}]]}}"#).as_deref(), Some("abc"));
        let flood = format!(r#"{{"params":[{}0],"token":"t"}}"#, "0,".repeat(1 << 20));
        assert_eq!(token_of(&flood).as_deref(), Some("t"));
        for no in [r#"["abc"]"#, r#"{"token":5}"#, r#"{"token":"a"} x"#, r#"{"id":1}"#, "GET / HTTP/1.1", r#""token""#] {
            assert_eq!(token_of(no), None, "{no}");
        }
    }

    /// #1989: `control.cancel` cancels the connection's own outstanding request. One still
    /// waiting its turn is removed and never runs, and the connection is free at once; one that
    /// already runs in the app can't be, and says so; another connection's request is refused;
    /// with nothing outstanding there is nothing to cancel.
    #[test]
    fn control_cancel_cancels_only_this_connections_request() {
        static RUNNING: Running = Running::new();
        let token = "0123456789abcdef0123456789abcdef";
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = channel::<ControlRequest>();
        let (gate_tx, gate_rx) = channel::<()>();
        let ran = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let seen = ran.clone();
        // A stand-in for the window thread: `queued` waits behind the others (never taken until
        // `slow` is done), `slow` runs until the gate opens.
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for req in rx {
                if req.method == "queued" {
                    held.push(req);
                    continue;
                }
                if !req.ticket.take() {
                    continue;
                }
                seen.lock().unwrap().push(req.method.clone());
                if req.method == "slow" {
                    let _ = gate_rx.recv();
                }
                let _ = req.reply.send(json!({"ok": true, "result": req.method}));
                if req.method == "slow" {
                    for req in held.drain(..) {
                        if req.ticket.take() {
                            seen.lock().unwrap().push(req.method.clone());
                        }
                    }
                }
            }
        });
        let waits = Waits { short: Duration::from_millis(100), long: Duration::from_millis(100) };
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let tx = tx.clone();
                std::thread::spawn(move || serve_with(stream, token, tx, egui::Context::default(), false, waits, &RUNNING, &DESIGN_WATCH));
            }
        });
        let connect = || {
            let s = TcpStream::connect(addr).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            let out = s.try_clone().unwrap();
            (out, BufReader::new(s))
        };
        let ask = |(out, lines): &mut (TcpStream, BufReader<TcpStream>), v: Value| -> Value {
            writeln!(out, "{v}").unwrap();
            let mut l = String::new();
            lines.read_line(&mut l).unwrap();
            serde_json::from_str(&l).unwrap()
        };
        let mut a = connect();
        let mut b = connect();
        let r = ask(&mut b, json!({"id": 1, "method": "control.cancel", "token": token}));
        assert!(r["ok"] == true && r["result"]["cancelled"].is_null(), "nothing to cancel: {r}");

        // Waiting its turn: removed, never run, and the connection is free.
        let r = ask(&mut a, json!({"id": 1, "method": "queued", "token": token}));
        assert_eq!(r["running"], true, "{r}");
        let r = ask(&mut a, json!({"id": 2, "method": "control.cancel", "params": {"id": 1}}));
        assert_eq!((&r["ok"], &r["result"]["cancelled"], &r["result"]["outcome"]), (&json!(true), &json!(1), &json!("removed")), "{r}");
        assert!(r["result"]["note"].as_str().is_some_and(|n| n.contains("will not run")), "{r}");
        let r = ask(&mut a, json!({"id": 3, "method": "control.pending"}));
        assert_eq!(r["result"]["pending"], Value::Null, "{r}");

        // Already running in the app: it can't be, and it still answers.
        let r = ask(&mut a, json!({"id": 4, "method": "slow"}));
        assert_eq!(r["running"], true, "{r}");
        let r = ask(&mut b, json!({"id": 2, "method": "control.cancel", "params": {"id": 4}}));
        assert!(r["ok"] == false && r["error"].as_str().is_some_and(|e| e.contains("its own requests")), "another connection's: {r}");
        let r = ask(&mut a, json!({"id": 5, "method": "control.cancel", "params": {"id": 99}}));
        assert!(r["ok"] == false && r["error"].as_str().is_some_and(|e| e.starts_with("nothing to cancel")), "{r}");
        let r = ask(&mut a, json!({"id": 6, "method": "control.cancel"}));
        assert!(r["ok"] == false && r["cancellable"] == false, "{r}");
        assert!(r["error"].as_str().is_some_and(|e| e.starts_with("cannot be cancelled") && e.contains("control.pending")), "{r}");
        gate_tx.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let r = loop {
            let r = ask(&mut a, json!({"id": 7, "method": "control.pending"}));
            if r["result"]["done"] == true || std::time::Instant::now() > deadline {
                break r;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(r["result"]["reply"]["result"], "slow", "{r}");
        let r = ask(&mut a, json!({"id": 8, "method": "control.cancel"}));
        assert!(r["ok"] == true && r["result"]["cancelled"].is_null(), "{r}");
        assert_eq!(*ran.lock().unwrap(), ["slow"], "the cancelled request never ran");
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

    /// A large reply reaches the socket in one write, not one per JSON piece (a component tree of
    /// thousands of components took half a second to send), and reads back as it was.
    #[test]
    fn a_large_reply_is_written_at_once() {
        struct Counted {
            writes: usize,
            bytes: Vec<u8>,
        }
        impl Write for Counted {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.writes += 1;
                self.bytes.extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let components: Vec<Value> = (0..2000).map(|i| json!({"id": i, "name": format!("Part {i}"), "bodies": ["a", "b"], "parent": 0})).collect();
        let reply = json!({"ok": true, "id": 7, "result": {"components": components}});
        let mut out = Counted { writes: 0, bytes: Vec::new() };
        send(&mut out, &reply).unwrap();
        assert_eq!(out.writes, 1);
        assert_eq!(out.bytes.last(), Some(&b'\n'));
        assert_eq!(serde_json::from_slice::<Value>(&out.bytes).unwrap(), reply);
    }
}
