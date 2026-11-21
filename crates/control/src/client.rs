//! The control channel's client (docs/control-protocol.md): a connection to an app's loopback
//! control port, with the app's token ([`App::client_token`]) on every request, one bounded
//! round trip per request, a fresh connection after one is lost, and the wait for a request the
//! app says is still running (#1618), through `control.pending`. Its errors tell an app that is
//! not running from one that went away during a call ([`CallError`]). What a request means, and
//! anything an app adds to its parameters, is the caller's.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::Effect;
use crate::auth::App;

/// Longest reply line accepted from the app.
pub const MAX_REPLY: u64 = 64 << 20;

/// How long a reply may take: kernel operations (booleans, fillets) on big parts can take a while.
pub const READ_TIMEOUT: Duration = Duration::from_secs(300);

/// How long the app may take to answer a request sent [`Request::once`]: a little longer than its
/// control server waits for a long request (3600 s), so its "still running" reply arrives first.
pub const ONCE_TIMEOUT: Duration = Duration::from_secs(3720);

/// How long connecting to the app may take.
const CONNECT_TIMEOUT: Duration = Duration::from_millis(800);

/// How often a long wait is reported ([`Connection::with_progress`]).
const PROGRESS_EVERY: Duration = Duration::from_secs(30);

/// How an error starts when a request is still running in the app after the wait: the request
/// was not cancelled and its reply waits on the connection; [`Connection::pending`] gets it.
pub use crate::STILL_RUNNING;

/// How long to wait for a request the app says is still running.
#[derive(Clone, Copy, Debug)]
pub struct Waits {
    /// How long a call goes on waiting, asking `control.pending`, after the app said its request
    /// is still running (the app says so after 60 s), before it says so itself.
    pub pending: Duration,
    /// How long a later call, or [`Connection::pending`], waits for a request still running from
    /// an earlier call.
    pub settle: Duration,
    /// How often `control.pending` is asked while waiting.
    pub poll: Duration,
}

impl Default for Waits {
    fn default() -> Self {
        Waits { pending: Duration::from_secs(240), settle: Duration::from_secs(60), poll: Duration::from_millis(500) }
    }
}

/// Why a call has no result. Each holds the message for the caller (an agent reads it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallError {
    /// Nothing listens at the address: the app is not running, or it quit before the call was
    /// sent. The message says how to start it.
    NotRunning(String),
    /// The app went away or did not answer during the call, after running it or not.
    Lost(String),
    /// The request is still running in the app after the wait (it starts with [`STILL_RUNNING`]),
    /// or an earlier one is and this one was not sent.
    StillRunning(String),
    /// The app answered with an error (a refused token says which token file it came from).
    Refused(String),
    /// The reply was not a control reply.
    BadReply(String),
}

impl CallError {
    /// The message.
    pub fn message(&self) -> &str {
        match self {
            CallError::NotRunning(m) | CallError::Lost(m) | CallError::StillRunning(m) | CallError::Refused(m) | CallError::BadReply(m) => m,
        }
    }
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for CallError {}

impl From<CallError> for String {
    fn from(e: CallError) -> String {
        e.message().to_string()
    }
}

/// A request: its method and parameters, how messages name it and what it does if it is still
/// running when its call returns.
#[derive(Clone, Debug)]
pub struct Request {
    pub method: String,
    pub params: Value,
    /// The request in messages ("`engine.execute` (solid.fillet)").
    pub what: String,
    /// What it still does when it finishes after its wait.
    pub effect: Effect,
    /// Never sent twice: on a fresh connection, waiting up to [`ONCE_TIMEOUT`], and once it is
    /// being written any error is final, as the app may have run it. Otherwise a request that
    /// finds its connection lost is sent once more on a fresh one (the app may have restarted).
    pub once: bool,
}

impl Request {
    /// `method` with `params`, named "`method`" in messages, taken to change the document, sent
    /// again once on a fresh connection when its connection was lost.
    pub fn new(method: &str, params: Value) -> Self {
        Request { method: method.to_string(), params, what: format!("`{method}`"), effect: Effect::Changes, once: false }
    }
}

/// A control reply as a call's result: its `result`, or its `error`.
pub fn outcome(v: &Value) -> Result<Value, String> {
    if v.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(v.get("result").cloned().unwrap_or(Value::Null))
    } else {
        Err(v.get("error").and_then(Value::as_str).unwrap_or("error").to_string())
    }
}

/// A request the app said is still running. Its reply will come on this connection, through
/// `control.pending`, so the connection is kept for it.
struct Outstanding {
    what: String,
    /// What it does when it finishes; `None`: not known (a request this client did not send).
    effect: Option<Effect>,
    since: Instant,
}

impl Outstanding {
    fn secs(&self) -> u64 {
        self.since.elapsed().as_secs()
    }
}

/// What became of a request still running when its call returned, once a later call waited it
/// out: kept for [`Connection::pending`].
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

/// A connection to the app listening at an address. It connects when a call needs it, so a
/// client started before the app reaches it once it runs.
pub struct Connection {
    app: App,
    /// What the app calls its document in messages ("design").
    document: &'static str,
    addr: String,
    port: u16,
    conn: Option<(BufReader<TcpStream>, TcpStream)>,
    /// Counts the connections made, so a caller can tell a fresh one from the one it asked.
    serial: u64,
    next_id: u64,
    /// The token given, when one was. Otherwise each request looks the token up again, since
    /// every launch of the app makes a new one.
    explicit_token: Option<String>,
    /// A request still running in the app; its reply waits on `conn`.
    outstanding: Option<Outstanding>,
    /// An outstanding request's end, once a later call waited for it (for `pending`).
    settled: Option<Settled>,
    waits: Waits,
    /// Where a long wait is reported.
    progress: fn(&str),
}

impl Connection {
    /// A connection to `app` at `addr` (`host:port`), with `token` when given, else the app's
    /// token found the usual way ([`App::client_token`]); not connected yet.
    pub fn new(app: App, addr: &str, token: Option<&str>) -> Self {
        let port = addr.rsplit(':').next().and_then(|p| p.parse::<u16>().ok()).unwrap_or(0);
        Self {
            app,
            document: "design",
            addr: addr.to_string(),
            port,
            conn: None,
            serial: 0,
            next_id: 1,
            explicit_token: token.map(str::to_string),
            outstanding: None,
            settled: None,
            waits: Waits::default(),
            progress: |_| {},
        }
    }

    /// What the app calls its document in messages ("design", the default).
    #[must_use]
    pub fn with_document(mut self, document: &'static str) -> Self {
        self.document = document;
        self
    }

    /// Report a long wait for a request still running to `progress`, every 30 s.
    #[must_use]
    pub fn with_progress(mut self, progress: fn(&str)) -> Self {
        self.progress = progress;
        self
    }

    /// How long to wait for a request still running.
    pub fn set_waits(&mut self, waits: Waits) {
        self.waits = waits;
    }

    /// The address it connects to.
    pub fn addr(&self) -> &str {
        &self.addr
    }

    /// The port of the address (0 when it names none).
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The token given, if one was.
    pub fn explicit_token(&self) -> Option<&str> {
        self.explicit_token.as_deref()
    }

    /// Whether a connection is open now.
    pub fn is_connected(&self) -> bool {
        self.conn.is_some()
    }

    /// The number of connections made so far: the same number while the same connection stays
    /// open.
    pub fn serial(&self) -> u64 {
        self.serial
    }

    /// Whether a request of this connection is still running in the app.
    pub fn has_outstanding(&self) -> bool {
        self.outstanding.is_some()
    }

    /// Close the connection. A request still running loses its reply with it.
    pub fn disconnect(&mut self) {
        self.conn = None;
        if let Some(o) = self.outstanding.take() {
            self.settled = Some(Settled::Lost { what: o.what });
        }
    }

    /// Connect afresh. Failing to reach the app at all is `NotConnected`, so [`Connection::explain`]
    /// can tell it from a connection lost during a call.
    ///
    /// # Errors
    /// `NotConnected` when nothing listens at the address (or it does not resolve).
    pub fn connect(&mut self) -> std::io::Result<()> {
        self.disconnect();
        let not_connected = |e: std::io::Error| std::io::Error::new(std::io::ErrorKind::NotConnected, e);
        let mut last = std::io::Error::new(std::io::ErrorKind::NotFound, format!("cannot resolve {}", self.addr));
        for sa in self.addr.to_socket_addrs().map_err(not_connected)? {
            match TcpStream::connect_timeout(&sa, CONNECT_TIMEOUT) {
                Ok(s) => {
                    s.set_nodelay(true).ok();
                    s.set_read_timeout(Some(READ_TIMEOUT)).ok();
                    let read = s.try_clone()?;
                    self.conn = Some((BufReader::new(read), s));
                    self.serial += 1;
                    return Ok(());
                }
                Err(e) => last = e,
            }
        }
        Err(not_connected(last))
    }

    /// See whether the app is there now; the error says why not. The connection doesn't stay
    /// open: the app closes one that shows no token within a few seconds, and the first call may
    /// be long away. That call connects again.
    ///
    /// # Errors
    /// As [`Connection::connect`].
    pub fn try_connect(&mut self) -> std::io::Result<()> {
        self.connect()?;
        self.disconnect();
        Ok(())
    }

    /// The error for a failed exchange: whether the app is not running (and how to start it) or
    /// went away during the call.
    pub fn explain(&self, e: &std::io::Error) -> CallError {
        let (addr, port, name, document) = (&self.addr, self.port, self.app.name, self.document);
        let start = format!("{} --control {port}", name.to_lowercase());
        if e.kind() == std::io::ErrorKind::NotConnected {
            CallError::NotRunning(format!(
                "no {name} app is listening at {addr} ({e}): it is not running, or it quit. Start it with `{start}` (or ask the user to); the \
                 next call connects to it"
            ))
        } else {
            CallError::Lost(format!(
                "the {name} app at {addr} did not answer ({e}): it may have quit or crashed during this call, after running it or not. Inspect \
                 the {document} before sending a change again; if the app quit, start it with `{start}`"
            ))
        }
    }

    /// The request line for `method`: a fresh id and the app's token (looked up again for every
    /// request: an app restarted mid-session has a new one).
    pub fn request_line(&mut self, method: &str, params: Value) -> String {
        let id = self.next_id;
        self.next_id += 1;
        let mut msg = json!({"id": id, "method": method, "params": params});
        if let Some(t) = self.app.client_token(self.port, self.explicit_token.as_deref()) {
            msg["token"] = json!(t.token);
        }
        msg.to_string()
    }

    /// One request line out and one reply line back, at most [`MAX_REPLY`] long, connecting
    /// first when not connected. Any failure closes the connection (a timed-out read leaves a
    /// late reply on the stream).
    fn roundtrip(&mut self, line: &str) -> std::io::Result<String> {
        if self.conn.is_none() {
            self.connect()?;
        }
        let Some((reader, writer)) = self.conn.as_mut() else { return Err(std::io::Error::other("not connected")) };
        // One write: a request that loses its newline still reaches the app as a request.
        let sent = writer.write_all(format!("{line}\n").as_bytes()).and_then(|()| writer.flush());
        if let Err(e) = sent {
            self.disconnect();
            return Err(e);
        }
        let mut reply = String::new();
        let n = match reader.by_ref().take(MAX_REPLY).read_line(&mut reply) {
            Ok(n) => n,
            Err(e) => {
                self.disconnect();
                return Err(e);
            }
        };
        if n == 0 || !reply.ends_with('\n') {
            self.disconnect();
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "connection closed"));
        }
        Ok(reply)
    }

    fn set_read_timeout(&self, t: Duration) {
        if let Some((_, writer)) = &self.conn {
            writer.set_read_timeout(Some(t)).ok();
        }
    }

    /// A reply line as JSON.
    fn parse(reply: &str) -> Result<Value, CallError> {
        serde_json::from_str(reply).map_err(|e| CallError::BadReply(format!("bad reply from the app: {e}")))
    }

    /// A control reply as a call's result; a refused token says which token file it came from
    /// (and whether it is stale).
    ///
    /// # Errors
    /// [`CallError::Refused`] with the app's error.
    pub fn outcome(&self, v: &Value) -> Result<Value, CallError> {
        outcome(v).map_err(|error| {
            CallError::Refused(
                self.app.client_token(self.port, self.explicit_token.as_deref()).map_or_else(|| error.clone(), |t| t.explain_error(&error)),
            )
        })
    }

    /// `method` with `params`, at once and once: not behind a request still running on this
    /// connection, no second try. The app's whole reply (`ok`, `result` or `error`, and anything
    /// it adds), for a request the app answers at once on any connection
    /// (`control.capabilities`, `control.running`).
    ///
    /// # Errors
    /// As [`Connection::explain`]; [`CallError::BadReply`].
    pub fn ask(&mut self, method: &str, params: Value) -> Result<Value, CallError> {
        let line = self.request_line(method, params);
        let reply = self.roundtrip(&line).map_err(|e| self.explain(&e))?;
        Self::parse(&reply)
    }

    /// `method` with `params`, waiting at most `timeout` for the reply and never behind a request
    /// still running: for a request the app answers within a frame, asked again later when the
    /// app is busy. One try.
    ///
    /// # Errors
    /// [`CallError::StillRunning`] when a request of this connection is still running; as
    /// [`Connection::ask`] and [`Connection::outcome`].
    pub fn quick(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value, CallError> {
        if self.outstanding.is_some() {
            return Err(CallError::StillRunning("a request is still running in the app".into()));
        }
        if self.conn.is_none() {
            self.connect().map_err(|e| self.explain(&e))?;
        }
        let line = self.request_line(method, params);
        self.set_read_timeout(timeout);
        let reply = self.roundtrip(&line);
        self.set_read_timeout(READ_TIMEOUT);
        let reply = reply.map_err(|e| self.explain(&e))?;
        let v = Self::parse(&reply)?;
        self.outcome(&v)
    }

    /// Ask the app, on this connection only, about the request it still runs for it.
    fn ask_pending(&mut self) -> Result<Value, CallError> {
        if self.conn.is_none() {
            self.disconnect();
            return Err(CallError::Lost("the connection to the app was lost".into()));
        }
        let v = self.ask("control.pending", json!({}))?;
        outcome(&v).map_err(CallError::Refused).inspect_err(|_| self.disconnect())
    }

    /// `control.cancel` for the request still running on this connection. Sent at once, without
    /// waiting for that request first, and never on a fresh connection, which has nothing to
    /// cancel. Once the app no longer holds it (removed, stopped or finished), later calls don't
    /// wait for it.
    ///
    /// # Errors
    /// As [`Connection::ask`]; [`CallError::Refused`].
    pub fn cancel(&mut self, params: Value) -> Result<Value, CallError> {
        if self.conn.is_none() {
            return Ok(json!({"cancelled": null, "note": "nothing to cancel: no request of this client is running in the app"}));
        }
        let v = self.ask("control.cancel", params)?;
        let r = outcome(&v).map_err(CallError::Refused);
        if r.is_ok() {
            self.outstanding = None;
        }
        r
    }

    /// Wait up to `limit` for the outstanding request, asking `control.pending` now and then and
    /// reporting how long it takes. An error: the connection, and so the reply, was lost.
    fn wait_outstanding(&mut self, limit: Duration) -> Result<Waited, CallError> {
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
                (self.progress)(&format!("{what} is still running in the app ({secs} s); waiting for it"));
                told = Instant::now();
            }
            std::thread::sleep(self.waits.poll.min(limit.saturating_sub(waited)));
        }
    }

    /// The error for the outstanding request still running after its wait.
    fn still_running(&self, earlier: bool) -> CallError {
        let Some(o) = &self.outstanding else { return CallError::StillRunning(format!("{STILL_RUNNING} a request is still running in the app")) };
        let document = self.document;
        let effect =
            o.effect.map_or_else(|| format!("it was not cancelled and may still change the {document} when it finishes"), |e| e.when_done(document));
        let settle = self.waits.settle.as_secs();
        CallError::StillRunning(if earlier {
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
        })
    }

    /// Before a new request: wait for one still running from an earlier call, keeping its end for
    /// `pending`. Still running after the wait: the new request is not sent.
    fn settle_earlier(&mut self) -> Result<(), CallError> {
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

    /// `control.pending` for the client: the end of the request still running from an earlier
    /// call, waiting for it up to the settle wait: `{pending: what | null, done?, reply?, lost?,
    /// running_s?}`. Never sent on a fresh connection, which has nothing pending.
    pub fn pending(&mut self) -> Value {
        if let Some(s) = self.settled.take() {
            return match s {
                Settled::Finished { what, reply } => json!({"pending": what, "done": true, "reply": reply}),
                Settled::Lost { what } => json!({"pending": what, "lost": true}),
            };
        }
        let Some(what) = self.outstanding.as_ref().map(|o| o.what.clone()) else { return json!({"pending": null}) };
        match self.wait_outstanding(self.waits.settle) {
            Ok(Waited::Done(reply)) => json!({"pending": what, "done": true, "reply": reply}),
            Ok(Waited::Nothing) => json!({"pending": null}),
            Ok(Waited::Running) => json!({"pending": what, "done": false, "running_s": self.outstanding.as_ref().map_or(0, Outstanding::secs)}),
            Err(_) => {
                self.settled = None;
                json!({"pending": what, "lost": true})
            }
        }
    }

    /// Send `request` and wait for its reply: the app's whole reply (`ok`, `result` or `error`,
    /// and anything it adds). First a request still running from an earlier call is waited for
    /// (except for `control.running`, which the app answers at once); a request the app says is
    /// still running is waited for through `control.pending`; a request refused unrun as busy
    /// behind one this client did not know of is sent once more after that one, when
    /// `retry_busy`.
    ///
    /// # Errors
    /// As [`Connection::explain`]: a request whose connection is lost is sent once more on a fresh
    /// one (unless [`Request::once`]), and when nothing listens any more the app went away, maybe
    /// while running it ([`CallError::Lost`]); [`CallError::StillRunning`]; [`CallError::BadReply`].
    pub fn exchange(&mut self, request: Request, retry_busy: bool) -> Result<Value, CallError> {
        let Request { method, params, what, effect, once } = request;
        if method != "control.running" {
            self.settle_earlier()?;
        }
        let again = retry_busy.then(|| params.clone());
        let sent = Instant::now();
        let line = self.request_line(&method, params);
        let reply = if once {
            // A fresh connection (an idle one may have been closed by an app restart; connecting
            // again sends nothing).
            self.connect().or_else(|_| self.connect()).and_then(|()| {
                self.set_read_timeout(ONCE_TIMEOUT);
                let r = self.roundtrip(&line);
                self.set_read_timeout(READ_TIMEOUT);
                r
            })
        } else {
            self.roundtrip(&line).or_else(|lost| {
                self.roundtrip(&line).map_err(|e| if e.kind() == std::io::ErrorKind::NotConnected && lost.kind() != e.kind() { lost } else { e })
            })
        }
        .map_err(|e| self.explain(&e))?;
        let v = Self::parse(&reply)?;
        // Timed out in the app, still running: wait for its result on this connection.
        if v.get("running").and_then(Value::as_bool) == Some(true) {
            self.outstanding = Some(Outstanding { what: what.clone(), effect: Some(effect), since: sent });
            (self.progress)(&format!(
                "{what} is still running in the app after {} s; waiting up to {} s more for it",
                sent.elapsed().as_secs(),
                self.waits.pending.as_secs()
            ));
            let document = self.document;
            return match self.wait_outstanding(self.waits.pending)? {
                Waited::Done(reply) => Ok(reply),
                Waited::Nothing => Err(CallError::Lost(format!(
                    "{what} timed out in the app, which kept no result for it: inspect the {document} before sending it again"
                ))),
                Waited::Running => Err(self.still_running(false)),
            };
        }
        // Refused unrun behind a request this client did not know of: wait for that one, then
        // send this one once more.
        if v.get("busy").and_then(Value::as_bool) == Some(true)
            && let Some(params) = again
        {
            self.outstanding = Some(Outstanding { what: "an earlier request".into(), effect: None, since: Instant::now() });
            self.settle_earlier()?;
            return self.exchange(Request { method, params, what, effect, once }, false);
        }
        Ok(v)
    }

    /// [`Connection::exchange`] (retrying a busy refusal once) and the reply's result.
    ///
    /// # Errors
    /// As [`Connection::exchange`] and [`Connection::outcome`].
    pub fn call(&mut self, request: Request) -> Result<Value, CallError> {
        let v = self.exchange(request, true)?;
        self.outcome(&v)
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
