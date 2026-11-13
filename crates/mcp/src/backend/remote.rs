//! The bridge to a running app over its loopback control port: [`Remote`], and [`Auto`], which
//! gives way to it. Native only, as the shared control client is: the browser build drives the
//! app through the page instead.

use std::time::Duration;

use serde_json::{Value, json};
use solidcraft_control::client::{CallError, Connection, Request, outcome};
use solidcraft_engine::control_pending;
use solidcraft_engine::panels::ActionSink;
use solidcraft_engine::panels::commands::LayoutCatalogue;
use solidcraft_engine::schema::ParamCheck;

use super::{Backend, Headless, step_command};

/// How long the app may take to answer `mcp.options`: the window answers it within a frame, so a
/// longer wait means the app is busy and the server asks again on a later call.
const OPTIONS_TIMEOUT: Duration = Duration::from_secs(2);

/// How long to wait for a request still running (tests shorten them).
#[cfg(test)]
pub(crate) use solidcraft_control::client::Waits;

/// The level a control reply says its request was checked at (#9184), if it says one.
fn echoed(v: &Value) -> Option<ParamCheck> {
    v.get("param_check").and_then(Value::as_str).and_then(ParamCheck::parse)
}

/// A running SolidCraft app reached through its loopback control port
/// (`solidcraft --control PORT`): the shared control client ([`Connection`]), with the agent's
/// folders on every request and the calls checked against their commands' contracts.
pub struct Remote {
    conn: Connection,
    /// How calls are checked (`--param-check`): by the app, when it checks a request's own
    /// level, else here before they are sent.
    param_check: ParamCheck,
    /// Whether the connected app checks a request's `param_check` itself (#9184), asked once per
    /// connection (the connection's serial number beside it); `None` until asked.
    app_checks: Option<(u64, bool)>,
    /// The level the app said it checked the last reply at (`param_check` beside `ok`).
    echoed: Option<ParamCheck>,
}

/// A long wait for a request still running, on stderr.
fn note(message: &str) {
    solidcraft_engine::note!("solidcraft-mcp: {message}");
}

impl Remote {
    /// Connect to the app at `addr`, with its control token found the usual way
    /// (`SOLIDCRAFT_CONTROL_TOKEN`, else the app's token file for that port).
    pub fn connect(addr: &str) -> std::io::Result<Self> {
        Self::connect_with_token(addr, None)
    }

    /// [`Remote::connect`] with the token given (`--token`); `None` finds it the usual way.
    pub fn connect_with_token(addr: &str, token: Option<&str>) -> std::io::Result<Self> {
        let mut r = Self::new(addr, token);
        r.conn.connect()?;
        Ok(r)
    }

    /// A bridge to the app at `addr` that connects when a call needs it: an MCP server started
    /// before the app keeps its tools, and they reach the app once it runs.
    pub fn new(addr: &str, token: Option<&str>) -> Self {
        Self {
            conn: Connection::new(solidcraft_engine::control_auth::APP, addr, token).with_progress(note),
            param_check: ParamCheck::Off,
            app_checks: None,
            echoed: None,
        }
    }

    /// Check calls against their commands' contracts at `level` (`--param-check`): an app that
    /// checks a request's own level is sent it, and an older one gets only the calls that pass.
    pub fn with_param_check(mut self, level: ParamCheck) -> Self {
        self.param_check = level;
        self
    }

    /// Shorter waits for requests still running (tests).
    #[cfg(test)]
    pub(crate) fn set_waits(&mut self, waits: Waits) {
        self.conn.set_waits(waits);
    }

    /// See whether the app is there now (`new` does not); the error says why not. The connection
    /// doesn't stay open: the app closes one that shows no token within a few seconds, and the
    /// first call may be long away. That call connects again.
    pub fn try_connect(&mut self) -> std::io::Result<()> {
        self.conn.try_connect()
    }

    /// `params` with the agent's folders: the app confines the files a request reaches to them
    /// (control.rs), and to the folders it opened or saved a design in, which count as its open
    /// design's.
    fn scoped(params: Value) -> Value {
        let mut params = params;
        if let (Some(scope), Some(o)) = (solidcraft_engine::io::vfs::current_scope(), params.as_object_mut()) {
            o.insert("allow_paths".into(), json!(scope.roots().iter().map(|r| r.to_string_lossy()).collect::<Vec<_>>()));
            if !scope.design_roots().is_empty() {
                o.insert("design_folders".into(), json!(scope.design_roots().iter().map(|r| r.to_string_lossy()).collect::<Vec<_>>()));
            }
        }
        params
    }

    /// `method` sent to the app as the shared client sends a request: named and with its effect
    /// for messages, `engine.batch` never sent twice (#743), and the level the app checked the
    /// reply at kept.
    fn call_once(&mut self, method: &str, params: Value, retry_busy: bool) -> Result<Value, String> {
        self.echoed = None;
        let request = Request {
            method: method.to_string(),
            what: control_pending::describe(method, &params),
            effect: control_pending::effect(method, &params),
            once: method == "engine.batch",
            params: Self::scoped(params),
        };
        let reply = self.conn.exchange(request, retry_busy)?;
        self.echoed = echoed(&reply);
        Ok(self.conn.outcome(&reply)?)
    }
}

/// What is wrong with `params` for `id` by this server's contracts, as messages; none when it has
/// no contract for `id` (the app may have commands this server lacks).
fn contract_issues(id: &str, params: &Value) -> Vec<String> {
    solidcraft_engine::schema::issues(id, params).into_iter().map(|i| i.message).collect()
}

/// Give `reply` `issues` as its `param_warnings`, as a headless session at the warn level does; a
/// reply that has its own (an app that checked the call itself) keeps those.
fn add_param_warnings(reply: &mut Value, issues: Vec<String>) {
    if !issues.is_empty()
        && let Some(o) = reply.as_object_mut()
    {
        o.entry("param_warnings").or_insert_with(|| json!(issues));
    }
}

impl Remote {
    /// Whether the app checks a request's `param_check` itself (#9184): asked once per
    /// connection with `control.capabilities`, which lists the levels it takes; an older app
    /// answers "unknown method". Any other answer (no app, a busy one) is asked again next time.
    fn app_checks(&mut self) -> bool {
        if let Some((serial, known)) = self.app_checks
            && self.conn.is_connected()
            && serial == self.conn.serial()
        {
            return known;
        }
        // Sent ahead of settling a request still running on this connection: the app answers
        // `control.capabilities` at once, as `control.running`, without taking that one's reply.
        let reply = self.conn.ask("control.capabilities", Self::scoped(json!({}))).ok();
        let known = match reply.as_ref().map(outcome) {
            Some(Ok(c)) => Some(c["param_check"].as_array().is_some_and(|levels| levels.iter().any(|l| l == self.param_check.name()))),
            Some(Err(e)) if e.starts_with("unknown method") => Some(false),
            _ => None,
        };
        self.app_checks = if self.conn.is_connected() { known.map(|k| (self.conn.serial(), k)) } else { None };
        known.unwrap_or(false)
    }

    /// The reply to a call sent with this bridge's level: at `strict`, one that doesn't say the
    /// app checked it at that level is an error, since the call may have run unchecked; and the
    /// next calls are checked here.
    fn confirmed(&mut self, reply: Result<Value, String>) -> Result<Value, String> {
        let unconfirmed = reply.is_ok() && self.echoed != Some(self.param_check);
        if unconfirmed {
            self.app_checks = Some((self.conn.serial(), false));
        }
        match reply {
            Ok(_) if unconfirmed && self.param_check == ParamCheck::Strict => {
                Err("the app did not say it checked this call at --param-check strict, so it may have run unchecked: inspect the design \
                 before sending it again (the next calls are checked here before they are sent)"
                    .into())
            }
            other => other,
        }
    }

    /// `params` with this bridge's level as the request's `param_check`, for an app that checks it.
    fn at_level(&self, mut params: Value) -> Value {
        if let Some(o) = params.as_object_mut() {
            o.insert("param_check".into(), json!(self.param_check.name()));
        }
        params
    }

    /// `engine.execute` checked at this bridge's level. An app that checks a request's level is
    /// sent it, and checks against its own commands (its reply must say so, see `confirmed`).
    /// For an older one the check is made here:
    /// `strict` refuses a call that breaks its command's contract without sending it, and `warn`
    /// sends it and adds the issues to the app's reply.
    fn execute_checked(&mut self, params: Value) -> Result<Value, String> {
        if let Some(id) = step_command(&params).filter(|id| solidcraft_engine::panels::commands::is_command(id)) {
            let empty = json!({});
            solidcraft_engine::panels::commands::parse_command(id, params.get("params").filter(|p| !p.is_null()).unwrap_or(&empty))
                .map_err(solidcraft_engine::panels::commands::error_text)?;
        }
        if self.app_checks() {
            let params = self.at_level(params);
            let reply = self.call_once("engine.execute", params, true);
            return self.confirmed(reply);
        }
        let id = params.get("command").and_then(Value::as_str).unwrap_or_default();
        let issues = contract_issues(id, params.get("params").unwrap_or(&Value::Null));
        if self.param_check == ParamCheck::Strict
            && let Some(first) = issues.first()
        {
            return Err(solidcraft_engine::params::bad(id, first.as_str()).to_string());
        }
        let mut reply = self.call_once("engine.execute", params, true)?;
        add_param_warnings(&mut reply, issues);
        Ok(reply)
    }

    /// `engine.batch` checked at this bridge's level, as `execute_checked` checks a call: by an
    /// app that checks a request's level, else here, where `strict` refuses a batch with a step
    /// that breaks its command's contract before anything is sent, as the app would refuse it
    /// unrun, and `warn` adds each step's issues to its result.
    fn batch_checked(&mut self, params: Value) -> Result<Value, String> {
        if self.app_checks() {
            let params = self.at_level(params);
            let reply = self.call_once("engine.batch", params, true);
            return self.confirmed(reply);
        }
        let steps = params.get("commands").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
        if self.param_check == ParamCheck::Strict {
            if let Some(refusal) = solidcraft_engine::batch_preflight::contracts(steps) {
                return Ok(refusal.reply(steps.len(), params.get("rollback").and_then(Value::as_bool).unwrap_or(true)));
            }
            return self.call_once("engine.batch", params, true);
        }
        let issues: Vec<Vec<String>> = steps
            .iter()
            .map(|step| {
                let id = step.get("command").or_else(|| step.get("id")).and_then(Value::as_str).unwrap_or_default();
                contract_issues(id, step.get("params").unwrap_or(&Value::Null))
            })
            .collect();
        let mut reply = self.call_once("engine.batch", params, true)?;
        if let Some(results) = reply.get_mut("results").and_then(Value::as_array_mut) {
            for (result, issues) in results.iter_mut().zip(issues) {
                add_param_warnings(result, issues);
            }
        }
        Ok(reply)
    }
}

impl Backend for Remote {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        match method {
            "control.pending" => Ok(self.conn.pending()),
            "engine.execute" if self.param_check != ParamCheck::Off => self.execute_checked(params),
            "engine.batch" if self.param_check != ParamCheck::Off => self.batch_checked(params),
            "engine.script" if self.param_check != ParamCheck::Off => self.call_once(method, self.at_level(params), true),
            // Not behind the request it cancels, as other calls are.
            "control.cancel" => Ok(self.conn.cancel(Self::scoped(params))?),
            _ => self.call_once(method, params, true),
        }
    }
    fn param_check(&self) -> ParamCheck {
        self.param_check
    }
    fn has_ui(&self) -> bool {
        true
    }
    /// The same app with this bridge's token, not yet connected: it connects when asked.
    fn side_channel(&self) -> Option<Remote> {
        Some(Remote::new(self.conn.addr(), self.conn.explicit_token()).with_param_check(self.param_check))
    }
    fn app_options(&mut self) -> Result<Option<Value>, String> {
        // This runs on the serve loop before a tool call, so it never waits: not behind a request
        // still running (ask again on a later call), and one try with no retry, because a retry
        // reconnects with READ_TIMEOUT and could block for minutes.
        match self.conn.quick("mcp.options", Self::scoped(json!({})), OPTIONS_TIMEOUT) {
            Ok(v) => Ok(Some(v)),
            // An app from before the setting (its agent chat refuses methods it doesn't know):
            // the server's own options decide.
            Err(CallError::Refused(e)) if e.starts_with("unknown method") || e.contains("is not available in the agent chat") => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    fn describe(&self) -> String {
        if self.conn.is_connected() {
            format!("connected to the SolidCraft app at {} (the user watches the viewport)", self.conn.addr())
        } else {
            // Still "connected to the SolidCraft app": the skills tell bridged from headless by it.
            format!(
                "connected to the SolidCraft app at {} once it runs (the user watches the viewport). It was not running \
                 when this server started; until it is (`solidcraft --control {}`), tool calls say so",
                self.conn.addr(),
                self.conn.port()
            )
        }
    }
}

/// `solidcraft-cli mcp` with no `--connect`: a headless session while no app runs, which gives
/// way to the app the moment one is found, as long as the session has no work of its own yet.
/// Once it has, it stays headless (the work is not carried over) and every tool result says so.
pub struct Auto {
    headless: Option<Headless>,
    remote: Option<Remote>,
    /// The port of the running app, when there is one.
    discover: Box<dyn FnMut() -> Option<u16>>,
    token: Option<String>,
}

impl Auto {
    pub fn new(headless: Headless, discover: Box<dyn FnMut() -> Option<u16>>, token: Option<&str>) -> Self {
        Auto { headless: Some(headless), remote: None, discover, token: token.map(str::to_owned) }
    }

    /// Switch to the app when the session is still empty and one is running.
    fn look_for_app(&mut self) {
        let empty = self.headless.as_ref().is_some_and(|h| {
            h.session.doc.features.is_empty()
                && h.panels.as_ref().is_ok_and(|panels| panels.service.inspect().revision == 0 && panels.catalogue.state().user_count() == 0)
        });
        if !empty {
            return;
        }
        if let Some(port) = (self.discover)() {
            solidcraft_engine::note!("solidcraft mcp: the SolidCraft app is running now; bridged to 127.0.0.1:{port}");
            let level = self.param_check();
            self.remote = Some(Remote::new(&format!("127.0.0.1:{port}"), self.token.as_deref()).with_param_check(level));
            self.headless = None;
        }
    }
}

impl Backend for Auto {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        if self.remote.is_none() {
            self.look_for_app();
        }
        match (&mut self.remote, &mut self.headless) {
            (Some(r), _) => r.call(method, params),
            (None, Some(h)) => h.call(method, params),
            (None, None) => Err("no session".into()),
        }
    }
    fn has_ui(&self) -> bool {
        self.remote.is_some()
    }
    fn describe(&self) -> String {
        match (&self.remote, &self.headless) {
            (Some(r), _) => r.describe(),
            (None, Some(h)) => format!(
                "{}, because the SolidCraft app was not running when this server started; if it starts before any design work \
                 is done here, the next call is bridged to it",
                h.describe()
            ),
            (None, None) => String::new(),
        }
    }
    fn design_path(&self) -> Option<String> {
        self.headless.as_ref().and_then(Headless::design_path)
    }
    fn in_process(&self) -> bool {
        self.remote.is_none()
    }
    fn param_check(&self) -> ParamCheck {
        match (&self.remote, &self.headless) {
            (Some(r), _) => r.param_check(),
            (None, Some(h)) => h.param_check(),
            (None, None) => ParamCheck::Off,
        }
    }
    fn app_options(&mut self) -> Result<Option<Value>, String> {
        // The app may have started since: bridge to it first, as a tool call would.
        if self.remote.is_none() {
            self.look_for_app();
        }
        match &mut self.remote {
            Some(r) => r.app_options(),
            // Headless for now: the app may yet start, so ask again on a later call.
            None => Err("no app yet".into()),
        }
    }
    fn side_channel(&self) -> Option<Remote> {
        self.remote.as_ref().and_then(Remote::side_channel)
    }
    fn notice(&self) -> Option<String> {
        self.headless.as_ref().map(|_| {
            "Note: this is a headless session because the SolidCraft app was not found; the user cannot see this work. \
             Ask them to start SolidCraft. While this session has no features, the next call connects to the app; once it \
             has work, it stays here."
                .to_owned()
        })
    }
}
