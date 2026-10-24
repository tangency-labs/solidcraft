//! Where MCP tool calls end up: a control-channel method call (`docs/control-protocol.md`).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use solidcraft_engine::Session;
use solidcraft_engine::control_pending;
use solidcraft_engine::render::StandardView;
use solidcraft_engine::view::Guides;

/// Something that answers control-channel methods: `engine.execute`, `engine.commands`,
/// `document.inspect`, `ui.view`, `ui.render`, `ui.screenshot`, `control.running`, and
/// `control.pending` (the result of a request that was still running when its call returned:
/// `{pending: what | null, done?, reply?, lost?, running_s?}`).
pub trait Backend {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String>;
    /// True when a real window is attached (`ui.screenshot` can capture it).
    fn has_ui(&self) -> bool;
    fn describe(&self) -> String;
    /// The open design's file, when this side knows it (its folder joins the agent's folders,
    /// see `vfs::PathScope::with_design`). A running app adds its own, so `Remote` says nothing.
    fn design_path(&self) -> Option<String> {
        None
    }
    /// Commands run in this process, on this thread (the kernel's time in them can be measured
    /// here, #1833).
    fn in_process(&self) -> bool {
        false
    }
}

/// Longest reply line accepted from the app.
const MAX_REPLY: u64 = 64 << 20;

/// How long a reply may take: kernel operations (booleans, fillets) on big parts can take a while.
const READ_TIMEOUT: Duration = Duration::from_secs(300);

/// How long the app may take to run an `engine.batch`: a little longer than its control server
/// waits (3600 s), so its "still running" reply arrives first.
const BATCH_TIMEOUT: Duration = Duration::from_secs(3720);

/// How long a call goes on waiting, asking `control.pending`, after the app said its request is
/// still running (the app says so after 60 s; #1618), before it says so itself.
const PENDING_WAIT: Duration = Duration::from_secs(240);

/// How long a later call, or the `pending` tool, waits for a request still running from an
/// earlier call.
const SETTLE_WAIT: Duration = Duration::from_secs(60);

/// How often `control.pending` is asked while waiting.
const POLL_EVERY: Duration = Duration::from_millis(500);

/// How often a long wait is reported on stderr.
const PROGRESS_EVERY: Duration = Duration::from_secs(30);

/// How a tool error starts when a request is still running in the app after the wait: the
/// request was not cancelled and its reply waits on the connection; the `pending` tool gets it.
/// `batch` uses it to tell "still running" from "failed".
pub(crate) const STILL_RUNNING: &str = "still running:";

/// A request the app said is still running (#1618). Its reply will come on this connection,
/// through `control.pending`, so the connection is kept for it.
struct Outstanding {
    what: String,
    /// What it does when it finishes; `None`: not known (a request this bridge did not send).
    effect: Option<control_pending::Effect>,
    since: Instant,
}

impl Outstanding {
    fn secs(&self) -> u64 {
        self.since.elapsed().as_secs()
    }
}

/// What became of a request still running when its call returned, once a later call waited it
/// out: kept for the `pending` tool.
enum Settled {
    Finished { what: String, reply: Value },
    Lost { what: String },
}

/// The end of a wait for an outstanding request.
enum Waited {
    /// It finished: its own reply.
    Done(Value),
    /// The app holds nothing (any more) for this connection.
    Nothing,
    /// Still running when the wait ran out.
    Running,
}

/// How long to wait for a request still running (tests shorten them).
#[derive(Clone, Copy)]
pub(crate) struct Waits {
    pub pending: Duration,
    pub settle: Duration,
    pub poll: Duration,
}

const WAITS: Waits = Waits { pending: PENDING_WAIT, settle: SETTLE_WAIT, poll: POLL_EVERY };

/// A control reply as a call's result.
fn outcome(v: &Value) -> Result<Value, String> {
    if v.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(v.get("result").cloned().unwrap_or(Value::Null))
    } else {
        Err(v.get("error").and_then(Value::as_str).unwrap_or("error").to_string())
    }
}

/// A running SolidCraft app reached through its loopback control port
/// (`solidcraft --control PORT`).
pub struct Remote {
    addr: String,
    port: u16,
    conn: Option<(BufReader<TcpStream>, TcpStream)>,
    next_id: u64,
    /// `--token`, when given. Otherwise each request looks the token up again, since every
    /// launch of the app makes a new one (docs/control-protocol.md).
    explicit_token: Option<String>,
    /// A request still running in the app; its reply waits on `conn`.
    outstanding: Option<Outstanding>,
    /// An outstanding request's end, once a later call waited for it (for `pending`).
    settled: Option<Settled>,
    waits: Waits,
}

/// How a tool error starts when no app listens at the bridged address. The agent reads it, and
/// `batch` uses it to tell "never sent" from "sent, reply lost".
pub(crate) const NOT_RUNNING: &str = "no SolidCraft app is listening at";

impl Remote {
    /// Connect to the app at `addr`, with its control token found the usual way
    /// (`SOLIDCRAFT_CONTROL_TOKEN`, else the app's token file for that port).
    pub fn connect(addr: &str) -> std::io::Result<Self> {
        Self::connect_with_token(addr, None)
    }

    /// [`Remote::connect`] with the token given (`--token`); `None` finds it the usual way.
    pub fn connect_with_token(addr: &str, token: Option<&str>) -> std::io::Result<Self> {
        let mut r = Self::new(addr, token);
        r.reconnect()?;
        Ok(r)
    }

    /// A bridge to the app at `addr` that connects when a call needs it: an MCP server started
    /// before the app keeps its tools, and they reach the app once it runs.
    pub fn new(addr: &str, token: Option<&str>) -> Self {
        let port = addr.rsplit(':').next().and_then(|p| p.parse::<u16>().ok()).unwrap_or(0);
        let explicit_token = token.map(str::to_string);
        Self { addr: addr.to_string(), port, conn: None, next_id: 1, explicit_token, outstanding: None, settled: None, waits: WAITS }
    }

    /// Shorter waits for requests still running (tests).
    #[cfg(test)]
    pub(crate) fn set_waits(&mut self, waits: Waits) {
        self.waits = waits;
    }

    /// Close the connection. A request still running loses its reply with it.
    fn drop_conn(&mut self) {
        self.conn = None;
        if let Some(o) = self.outstanding.take() {
            self.settled = Some(Settled::Lost { what: o.what });
        }
    }

    /// Connect now, if the app is there (`new` does not); the error says why not.
    pub fn try_connect(&mut self) -> std::io::Result<()> {
        self.reconnect()
    }

    /// Failing to reach the app at all is `NotConnected`, so [`Remote::explain`] can tell it from
    /// a connection lost during a call.
    fn reconnect(&mut self) -> std::io::Result<()> {
        self.drop_conn();
        let not_connected = |e: std::io::Error| std::io::Error::new(std::io::ErrorKind::NotConnected, e);
        let mut last = std::io::Error::new(std::io::ErrorKind::NotFound, format!("cannot resolve {}", self.addr));
        for sa in self.addr.to_socket_addrs().map_err(not_connected)? {
            match TcpStream::connect_timeout(&sa, Duration::from_millis(800)) {
                Ok(s) => {
                    s.set_nodelay(true).ok();
                    s.set_read_timeout(Some(READ_TIMEOUT)).ok();
                    let read = s.try_clone()?;
                    self.conn = Some((BufReader::new(read), s));
                    return Ok(());
                }
                Err(e) => last = e,
            }
        }
        Err(not_connected(last))
    }

    /// The tool error for a failed call: whether the app is not running (and how to start it) or
    /// went away during the call.
    fn explain(&self, e: &std::io::Error) -> String {
        let (addr, port) = (&self.addr, self.port);
        if e.kind() == std::io::ErrorKind::NotConnected {
            format!(
                "{NOT_RUNNING} {addr} ({e}): it is not running, or it quit. Start it with `solidcraft --control {port}` \
                 (or ask the user to); the next call connects to it"
            )
        } else {
            format!(
                "the SolidCraft app at {addr} did not answer ({e}): it may have quit or crashed during this call, after                  running it or not. Inspect the design before sending a change again; if the app quit, start it with                  `solidcraft --control {port}`"
            )
        }
    }

    fn roundtrip(&mut self, line: &str) -> std::io::Result<String> {
        if self.conn.is_none() {
            self.reconnect()?;
        }
        let Some((reader, writer)) = self.conn.as_mut() else { return Err(std::io::Error::other("not connected")) };
        // One write: a request that loses its newline still reaches the app as a request.
        let sent = writer.write_all(format!("{line}\n").as_bytes()).and_then(|()| writer.flush());
        if let Err(e) = sent {
            self.drop_conn();
            return Err(e);
        }
        let mut reply = String::new();
        let n = match reader.by_ref().take(MAX_REPLY).read_line(&mut reply) {
            Ok(n) => n,
            Err(e) => {
                // A timed-out read leaves a late reply on the stream: start afresh next time.
                self.drop_conn();
                return Err(e);
            }
        };
        if n == 0 || !reply.ends_with('\n') {
            self.drop_conn();
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "connection closed"));
        }
        Ok(reply)
    }

    fn set_read_timeout(&self, t: Duration) {
        if let Some((_, writer)) = &self.conn {
            writer.set_read_timeout(Some(t)).ok();
        }
    }

    /// `engine.batch` is never sent twice (#743): it goes on a fresh connection (an idle one may
    /// have been closed by an app restart; connecting again sends nothing), and once it is being
    /// written any error is final, as the app may have run it.
    fn send_batch(&mut self, line: &str) -> std::io::Result<String> {
        self.reconnect().or_else(|_| self.reconnect())?;
        self.set_read_timeout(BATCH_TIMEOUT);
        let r = self.roundtrip(line);
        self.set_read_timeout(READ_TIMEOUT);
        r
    }
}

impl Remote {
    /// The request line for `method`: a fresh id, the agent's folders and the app's token.
    fn request_line(&mut self, method: &str, params: Value) -> String {
        let id = self.next_id;
        self.next_id += 1;
        let mut params = params;
        // The app confines the files this request reaches to the agent's folders (control.rs).
        if let (Some(scope), Some(o)) = (solidcraft_engine::io::vfs::current_scope(), params.as_object_mut()) {
            o.insert("allow_paths".into(), json!(scope.roots().iter().map(|r| r.to_string_lossy()).collect::<Vec<_>>()));
        }
        let mut msg = json!({"id": id, "method": method, "params": params});
        // The token goes with every request (an app restarted mid-session has a new one).
        if let Some(t) = solidcraft_engine::control_auth::client_token(self.port, self.explicit_token.as_deref()) {
            msg["token"] = json!(t);
        }
        msg.to_string()
    }

    /// Ask the app, on this connection only, about the request it still runs for it.
    fn ask_pending(&mut self) -> Result<Value, String> {
        if self.conn.is_none() {
            self.drop_conn();
            return Err("the connection to the app was lost".into());
        }
        let line = self.request_line("control.pending", json!({}));
        let reply = self.roundtrip(&line).map_err(|e| self.explain(&e))?;
        let v: Value = serde_json::from_str(&reply).map_err(|e| format!("bad reply from the app: {e}"))?;
        outcome(&v).inspect_err(|_| self.drop_conn())
    }

    /// `control.cancel` for the request still running on this connection (a timed-out call's,
    /// docs/control-protocol.md). Sent at once, without waiting for that request first, and never
    /// on a fresh connection, which has nothing to cancel. Once the app no longer holds it
    /// (removed, stopped or finished), later calls don't wait for it.
    fn cancel_outstanding(&mut self, params: Value) -> Result<Value, String> {
        if self.conn.is_none() {
            return Ok(json!({"cancelled": null, "note": "nothing to cancel: no request of this bridge is running in the app"}));
        }
        let line = self.request_line("control.cancel", params);
        let reply = self.roundtrip(&line).map_err(|e| self.explain(&e))?;
        let v: Value = serde_json::from_str(&reply).map_err(|e| format!("bad reply from the app: {e}"))?;
        let r = outcome(&v);
        if r.is_ok() {
            self.outstanding = None;
        }
        r
    }

    /// Wait up to `limit` for the outstanding request, asking `control.pending` now and then and
    /// saying on stderr how long it takes. An error: the connection, and so the reply, was lost.
    fn wait_outstanding(&mut self, limit: Duration) -> Result<Waited, String> {
        let start = Instant::now();
        let mut told = start;
        loop {
            let Some(o) = &self.outstanding else { return Ok(Waited::Nothing) };
            let what = o.what.clone();
            let r = self.ask_pending()?;
            if r.get("pending").is_none_or(Value::is_null) {
                self.outstanding = None;
                return Ok(Waited::Nothing);
            }
            if r.get("done").and_then(Value::as_bool) == Some(true) {
                self.outstanding = None;
                return Ok(Waited::Done(r.get("reply").cloned().unwrap_or(Value::Null)));
            }
            let waited = start.elapsed();
            if waited >= limit {
                return Ok(Waited::Running);
            }
            if told.elapsed() >= PROGRESS_EVERY {
                let secs = self.outstanding.as_ref().map_or(0, Outstanding::secs);
                eprintln!("solidcraft-mcp: {what} is still running in the app ({secs} s); waiting for it");
                told = Instant::now();
            }
            std::thread::sleep(self.waits.poll.min(limit.saturating_sub(waited)));
        }
    }

    /// The tool error for the outstanding request still running after its wait.
    fn still_running(&self, earlier: bool) -> String {
        let Some(o) = &self.outstanding else { return format!("{STILL_RUNNING} a request is still running in the app") };
        let effect = o.effect.map_or("it was not cancelled and may still change the design when it finishes", control_pending::Effect::when_done);
        let settle = self.waits.settle.as_secs();
        if earlier {
            format!(
                "{STILL_RUNNING} an earlier request, {}, is still running in the app ({} s; {effect}), so this call was not sent. Call the \
                 `pending` tool to wait for its result, then send this call again",
                o.what,
                o.secs()
            )
        } else {
            format!(
                "{STILL_RUNNING} {} is still running in the app after {} s ({effect}); don't send it again. Call the `pending` tool to \
                 wait for its result (up to {settle} s per call); other calls wait for it first",
                o.what,
                o.secs()
            )
        }
    }

    /// Before a new request: wait for one still running from an earlier call, keeping its end for
    /// `pending`. Still running after the wait: the new request is not sent.
    fn settle_earlier(&mut self) -> Result<(), String> {
        let Some(what) = self.outstanding.as_ref().map(|o| o.what.clone()) else { return Ok(()) };
        match self.wait_outstanding(self.waits.settle) {
            Ok(Waited::Done(reply)) => self.settled = Some(Settled::Finished { what, reply }),
            Ok(Waited::Nothing) => {}
            Ok(Waited::Running) => return Err(self.still_running(true)),
            // The connection was lost, and the earlier reply with it (`pending` says so): the new
            // request goes on a fresh connection.
            Err(_) => {}
        }
        Ok(())
    }

    /// `control.pending` for the bridge: the end of the request still running from an earlier
    /// call, waiting for it up to the settle wait. Never sent on a fresh connection, which has
    /// nothing pending.
    fn pending_status(&mut self) -> Result<Value, String> {
        if let Some(s) = self.settled.take() {
            return Ok(match s {
                Settled::Finished { what, reply } => json!({"pending": what, "done": true, "reply": reply}),
                Settled::Lost { what } => json!({"pending": what, "lost": true}),
            });
        }
        let Some(what) = self.outstanding.as_ref().map(|o| o.what.clone()) else { return Ok(json!({"pending": null})) };
        match self.wait_outstanding(self.waits.settle) {
            Ok(Waited::Done(reply)) => Ok(json!({"pending": what, "done": true, "reply": reply})),
            Ok(Waited::Nothing) => Ok(json!({"pending": null})),
            Ok(Waited::Running) => Ok(json!({"pending": what, "done": false, "running_s": self.outstanding.as_ref().map_or(0, Outstanding::secs)})),
            Err(_) => {
                self.settled = None;
                Ok(json!({"pending": what, "lost": true}))
            }
        }
    }

    fn call_once(&mut self, method: &str, params: Value, retry_busy: bool) -> Result<Value, String> {
        // The app answers `control.running` at once, even behind a request still running: it is
        // not held back for that one as other calls are.
        if method != "control.running" {
            self.settle_earlier()?;
        }
        let what = control_pending::describe(method, &params);
        let effect = control_pending::effect(method, &params);
        let again = retry_busy.then(|| params.clone());
        let sent = Instant::now();
        let line = self.request_line(method, params);
        let reply = if method == "engine.batch" {
            self.send_batch(&line)
        } else {
            // One retry on a fresh connection (the app may have restarted). When nothing listens
            // any more, the app went away, maybe while running this request: say that, not
            // just "not running".
            self.roundtrip(&line).or_else(|lost| {
                self.roundtrip(&line).map_err(|e| if e.kind() == std::io::ErrorKind::NotConnected && lost.kind() != e.kind() { lost } else { e })
            })
        }
        .map_err(|e| self.explain(&e))?;
        let v: Value = serde_json::from_str(&reply).map_err(|e| format!("bad reply from the app: {e}"))?;
        // Timed out in the app, still running (#1618): wait for its result on this connection.
        if v.get("running").and_then(Value::as_bool) == Some(true) {
            self.outstanding = Some(Outstanding { what: what.clone(), effect: Some(effect), since: sent });
            eprintln!(
                "solidcraft-mcp: {what} is still running in the app after {} s; waiting up to {} s more for it",
                sent.elapsed().as_secs(),
                self.waits.pending.as_secs()
            );
            return match self.wait_outstanding(self.waits.pending)? {
                Waited::Done(reply) => outcome(&reply),
                Waited::Nothing => {
                    Err(format!("{what} timed out in the app, which kept no result for it: inspect the design before sending it again"))
                }
                Waited::Running => Err(self.still_running(false)),
            };
        }
        // Refused unrun behind a request this bridge did not know of: wait for that one, then
        // send this one once more.
        if v.get("busy").and_then(Value::as_bool) == Some(true)
            && let Some(params) = again
        {
            self.outstanding = Some(Outstanding { what: "an earlier request".into(), effect: None, since: Instant::now() });
            self.settle_earlier()?;
            return self.call_once(method, params, false);
        }
        outcome(&v)
    }
}

impl Backend for Remote {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        if method == "control.pending" {
            return self.pending_status();
        }
        // Not behind the request it cancels, as other calls are.
        if method == "control.cancel" {
            return self.cancel_outstanding(params);
        }
        self.call_once(method, params, true)
    }
    fn has_ui(&self) -> bool {
        true
    }
    fn describe(&self) -> String {
        if self.conn.is_some() {
            format!("connected to the SolidCraft app at {} (the user watches the viewport)", self.addr)
        } else {
            // Still "connected to the SolidCraft app": the skills tell bridged from headless by it.
            format!(
                "connected to the SolidCraft app at {} once it runs (the user watches the viewport). It was not running \
                 when this server started; until it is (`solidcraft --control {}`), tool calls say so",
                self.addr, self.port
            )
        }
    }
}

/// An in-process headless session (no window); renders with the CPU rasterizer.
pub struct Headless {
    pub session: Session,
    /// Orientation for renders; the camera is refitted to the model every time.
    pub view: StandardView,
}

impl Default for Headless {
    fn default() -> Self {
        Headless::new(Session::default())
    }
}

impl Headless {
    pub fn new(mut session: Session) -> Self {
        match solidcraft_engine::render::job::CpuHost::for_platform() {
            Ok(host) => {
                if let Err(e) = session.set_render_job_host(Some(host)) {
                    session.log.push(format!("image host unavailable: {e}"));
                }
            }
            Err(e) => session.log.push(format!("image host unavailable: {e}")),
        }
        Headless { session, view: StandardView::Iso }
    }
}

fn dim(p: &Value, k: &str, default: f64) -> u32 {
    p.get(k).and_then(Value::as_f64).filter(|x| x.is_finite()).unwrap_or(default).clamp(16.0, 4096.0) as u32
}

impl Backend for Headless {
    fn call(&mut self, method: &str, p: Value) -> Result<Value, String> {
        let s = &mut self.session;
        let str_p = |k: &str| p.get(k).and_then(Value::as_str);
        match method {
            "engine.execute" | "command" => {
                let id = str_p("command").ok_or("missing `command`")?;
                let params = p.get("params").cloned().filter(|v| !v.is_null()).unwrap_or(json!({}));
                s.execute(id, &params).map_err(|e| e.to_string())
            }
            "engine.script" => s.run_script(&p).map(Value::from).map_err(|e| e.to_string()),
            "engine.commands" => s.execute("engine.commands", &json!({})).map_err(|e| e.to_string()),
            "document.inspect" => s.execute("document.inspect", &p).map_err(|e| e.to_string()),
            "ui.view" => {
                let v = str_p("view").unwrap_or("home");
                match v {
                    // Fit keeps the orientation, also of a camera `view.apply` chose.
                    "fit" => {
                        let b = solidcraft_engine::view::bounds(s);
                        if let Some(c) = s.camera.as_mut() {
                            c.fit(&b);
                        }
                    }
                    _ => {
                        self.view = StandardView::parse(v).ok_or_else(|| format!("unknown view `{v}`"))?;
                        s.camera = None;
                    }
                }
                Ok(json!({"view": self.view}))
            }
            "ui.render" => {
                let (w, h) = (dim(&p, "width", 1280.0), dim(&p, "height", 800.0));
                // The camera `view.apply` chose, else the standard view framing the model.
                let cam = s.camera.unwrap_or_else(|| {
                    let mut cam = solidcraft_engine::view::home_camera(s);
                    cam.set_view(self.view);
                    cam
                });
                // The grid and the axes are drawn unless the request leaves them out.
                let shown = |k: &str| p.get(k).and_then(Value::as_bool).unwrap_or(true);
                let guides = Guides { grid: shown("grid"), axes: shown("axes") };
                let path = str_p("path").ok_or("missing `path`")?;
                // A path outside the agent's folders is refused before anything is drawn.
                solidcraft_engine::io::vfs::check(path).map_err(|e| format!("{path}: {e}"))?;
                let scene = solidcraft_engine::view::scene_with_guides(s, &cam, s.section, guides);
                // Drawn on the GPU by the renderer the host installed (`solidcraft-cli mcp`).
                let png = solidcraft_engine::capture::png(&scene, &cam, w, h)?;
                solidcraft_engine::io::vfs::write(path, &png.bytes).map_err(|e| format!("{path}: {e}"))?;
                Ok(json!({
                    "path": path,
                    "bytes": png.bytes.len(),
                    "width": w,
                    "height": h,
                    "renderer": png.renderer,
                    "warning": solidcraft_engine::capture::warning(png.renderer),
                }))
            }
            "ui.screenshot" => Err("no window in headless mode; use ui.render".into()),
            // Calls run here one at a time: none is running while this one is asked, and nothing
            // is ever left running.
            "control.running" => Ok(json!({"running": []})),
            "control.pending" => Ok(json!({"pending": null})),
            "control.cancel" => Err("nothing can be cancelled in headless mode: each call runs to its end before it returns".into()),
            other => Err(format!("unknown method `{other}`")),
        }
    }
    fn has_ui(&self) -> bool {
        false
    }
    fn describe(&self) -> String {
        "headless in-process SolidCraft session".into()
    }
    fn design_path(&self) -> Option<String> {
        self.session.path.clone()
    }
    fn in_process(&self) -> bool {
        true
    }
}
