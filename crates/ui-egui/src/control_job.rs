//! Control-channel (MCP) requests that compute off the window thread (#1989): an agent's slow
//! command (a boolean, a fillet, a big batch, an export) used to run on the window thread, and
//! the window froze until it was done: no redraw, no orbit, no way to stop it.
//!
//! The window thread still owns the design. What leaves it is the computing:
//! - **Design changes** (`engine.execute` of a modelling command, `engine.batch`) are evaluated
//!   ahead on a copy of the session ([`solidcraft_engine::Session::ahead`]), as the history
//!   marker and undo are ([`crate::deferred`]). What that computes is kept
//!   ([`solidcraft_engine::doc::KeptResults`]); once it is in, the request runs here for real, as
//!   it always did (undo step, recovery autosave, crash journal, agent cursor, reply), and finds
//!   every feature it computed there, whatever the bounded evaluation cache dropped meanwhile.
//!   One design change computes at a time; later ones wait their turn, in order.
//! - **Exports** (`file.export`, `file.save_mesh`) change nothing in the design: they run on a
//!   copy of it as it was when they were asked for, and their reply comes from there. Nothing
//!   waits behind them.
//!
//! Meanwhile the window goes on drawing the design as it was, `ui.*` requests and inspection are
//! answered, and a chip says which agent command is computing, with a Stop button. A person's
//! own edits meanwhile go first: the agent's command is then computed again for the design as it
//! is and lands after them, never in between. If the design itself was replaced (another tab, a
//! file opened) the agent's command is refused, saying so, and nothing is applied.
//!
//! Done within the app's wait ([`crate::dialogs::APPLY_AT_ONCE`]), a request just runs, as
//! before. Headless use, tests and the web build ([`SolveApp::apply_off_thread`] unset) run
//! everything at once, and so does the app with the preference off (Preferences > AI,
//! `agent_off_thread`, on by default).
//!
//! The client that sent a request can cancel it (`control.cancel`, through its [`Ticket`]): one
//! still waiting its turn is answered as cancelled without running, and one computing off the
//! window thread is stopped as Stop stops it. Its worker is told to stop too: the evaluation
//! skips the features it has not reached, and the kernel starts no further boolean
//! ([`solidcraft_engine::doc::progress`]); what it computed after that is not kept. One running
//! on the window thread can't be cancelled.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};

use serde_json::{Value, json};

use solidcraft_engine::doc::KeptResults;
use solidcraft_engine::doc::progress::Progress;

use crate::SolveApp;
use crate::control::ControlRequest;

pub use solidcraft_control::{Cancel, Ticket};

/// A stop flag for the work computing `ticket`'s request, which its cancel sets.
fn progress_of(ticket: &Ticket) -> Arc<Progress> {
    let progress = Arc::new(Progress::default());
    let stop = Arc::clone(&progress);
    ticket.on_stop(move || stop.cancel());
    progress
}

/// Order inspection registrations across worker completions and commands run on the window.
pub(crate) fn next_plugin_inspection() -> usize {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// The reply to a request its client cancelled before it ran.
pub(crate) fn cancelled_reply(method: &str) -> Value {
    let method: String = method.chars().take(128).collect();
    json!({"ok": false, "cancelled": true, "error": format!("cancelled: `{method}` was cancelled (control.cancel) before it ran; nothing was applied")})
}

/// The reply to a request its client cancelled while it computed off the window thread.
pub(crate) fn stopped_reply(label: &str, writes_file: bool) -> Value {
    let error = if writes_file {
        format!("stopped: `{label}` was cancelled (control.cancel) while it computed; the design is unchanged, but its file may still be written")
    } else {
        format!("stopped: `{label}` was cancelled (control.cancel) while it computed; nothing was applied")
    };
    json!({"ok": false, "stopped": true, "error": error})
}

/// The same stop flag reaches both evaluation and each parallel display worker.
pub(crate) fn warm_plugin_display(
    prepared: &solidcraft_engine::ComputedCommands,
    quality: solidcraft_engine::doc::display_quality::DisplayQuality,
    progress: &Progress,
) -> solidcraft_engine::Result<()> {
    prepared.warm_display(quality, || !progress.is_cancelled())
}

/// Areas whose commands only compute the design (no files written, nothing started outside the
/// app), so evaluating them ahead on a copy has no effect but a warm cache.
const AHEAD_AREAS: &[&str] = &[
    "appearance",
    "assembly",
    "body",
    "component",
    "config",
    "construct",
    "contact",
    "explode",
    "expr",
    "face",
    "joint",
    "material",
    "mesh",
    "model",
    "occurrence",
    "parameters",
    "plastic",
    "sheet",
    "sketch",
    "solid",
    "surface",
    "timeline",
];

/// Parameters that name a file or folder: a command given one runs here, as before.
const NAMES_A_FILE: &[&str] = &["path", "paths", "file", "files", "dir", "folder", "url", "out"];

/// Commands that change nothing in the design and write a file: they run on a copy.
const ANSWERED_ON_A_COPY: &[&str] =
    &["file.export", "file.export_3mf", "file.export_obj", "file.export_step", "file.export_stl", "file.export_urdf", "file.save_mesh"];

/// Commands that only read the design, which can take long on a large one (a measure of every
/// body, an inspection that measures them, #4604; a clearance or interference check of every pair
/// of bodies, minutes on an assembly of 150): they run on a copy too, whatever the preference,
/// since an answer from the design as it was when asked has no order to keep.
const READ_ON_A_COPY: &[&str] = &[
    "model.mesh",
    "plugin.inspect",
    "inspect.measure",
    "document.inspect",
    "inspect.clearance",
    "inspect.interference",
    "inspect.parameter_variants",
    // Physical properties of a component or placement: every body of every placement (#8706).
    "component.properties",
    "occurrence.properties",
];

/// Exports and readings computing at once; more run here.
const MAX_ANSWERING: usize = 4;

/// How often a design change is computed again because the design changed under it before it
/// runs here anyway.
const MAX_TRIES: u8 = 3;

/// The command's own id (as registered), if it has one.
fn spec_id(command: &str) -> Option<&'static str> {
    solidcraft_engine::find_command(command).map(|c| c.id)
}

fn names_a_file(params: &Value) -> bool {
    params.as_object().is_some_and(|o| o.keys().any(|k| NAMES_A_FILE.contains(&k.as_str())))
}

/// Can `command` be evaluated ahead on a copy of the session with no effect outside it?
pub(crate) fn computes_ahead(command: &str, params: &Value) -> bool {
    let Some(id) = spec_id(command) else { return false };
    if matches!(id, "edit.undo" | "edit.redo") {
        return true;
    }
    let area = id.split('.').next().unwrap_or_default();
    AHEAD_AREAS.contains(&area) && !["export", "import", "save", "library"].iter().any(|w| id.contains(w)) && !names_a_file(params)
}

/// The command an `engine.execute` request (its `params`) runs and its parameters.
fn executed(params: &Value) -> Option<(&str, Value)> {
    let command = params.get("command").or_else(|| params.get("id")).and_then(Value::as_str)?;
    Some((command, params.get("params").cloned().filter(|v| !v.is_null()).unwrap_or(json!({}))))
}

/// A request that only reads design data or an inert package and can take long ([`READ_ON_A_COPY`]): an
/// `engine.execute` of such a command, or the `document.inspect` method. The command and its
/// parameters.
fn reading(req: &ControlRequest) -> Option<(&'static str, Value)> {
    match req.method.as_str() {
        "document.inspect" => Some(("document.inspect", req.params.clone())),
        "engine.execute" | "command" => {
            let (command, params) = executed(&req.params)?;
            spec_id(command).filter(|id| READ_ON_A_COPY.contains(id)).map(|id| (id, params))
        }
        _ => None,
    }
}

/// Requests that change the design: they wait while one computes.
pub(crate) fn changes_the_design(method: &str) -> bool {
    matches!(method, "engine.execute" | "command" | "engine.script" | "engine.batch" | "document.import" | "document.saved")
}

/// Whether `doc` is the design `session` shows, as it is or edited since (an undo step away).
fn shows_design(session: &solidcraft_engine::Session, doc: &Arc<solidcraft_engine::doc::Document>) -> bool {
    Arc::ptr_eq(&session.doc, doc) || session.undo.iter().chain(&session.redo).any(|snap| Arc::ptr_eq(&snap.doc, doc))
}

/// The reply to a request whose design left the window while it computed.
fn design_replaced(label: &str) -> Value {
    json!({
        "ok": false,
        "error": format!("the design was replaced (another tab, or a file opened) while `{label}` computed; nothing was applied"),
    })
}

/// What a request computes off the window thread.
enum Work {
    /// Evaluated ahead: once in, the request runs here. `doc` and `revision` are the design it
    /// was computed for.
    Ahead { commands: Vec<(String, Value)>, doc: Arc<solidcraft_engine::doc::Document>, revision: u64, tries: u8, kept: Arc<KeptResults> },
    /// Run on a copy; the reply comes from there.
    Answer { inspection: Option<(u64, usize)> },
    /// External code executes once on its worker. Stale/cancelled results are discarded.
    Plugin {
        prepared: Receiver<Result<solidcraft_engine::ComputedCommands, String>>,
        disabled: std::collections::BTreeSet<String>,
        limits: solidcraft_engine::plugin::HostLimits,
        level: Option<solidcraft_engine::schema::ParamCheck>,
    },
}

/// A control request computing off the window thread.
pub struct ControlJob {
    req: ControlRequest,
    work: Work,
    /// The worker's result: `Some` reply for [`Work::Answer`].
    done: Receiver<Option<Value>>,
    /// When it was asked for (ms).
    pub(crate) started: f64,
    /// The command, as the toolbar names it.
    pub(crate) label: String,
    /// How far it has got (evaluated ahead).
    steps: Option<Arc<Steps>>,
}

impl ControlJob {
    fn poll(&self) -> Option<Option<Value>> {
        match self.done.try_recv() {
            Ok(r) => Some(r),
            Err(TryRecvError::Disconnected) => Some(None),
            Err(TryRecvError::Empty) => None,
        }
    }
}

/// Most steps still to come that the agent row's tooltip lists: a 100-step batch must not fill
/// the screen.
pub(crate) const MAX_NEXT_SHOWN: usize = 12;

fn label(app: &SolveApp, command: &str) -> String {
    crate::agent_label::name_in(solidcraft_ui_text::Locale::for_language(&app.preferences.language), command)
}

/// Run `f` on a worker thread (with the request's folder scope, stopping early once `progress`
/// is cancelled), answering on the returned channel. `None` without threads (the web).
fn spawn(
    scope: Option<solidcraft_engine::io::vfs::PathScope>,
    progress: Arc<Progress>,
    f: impl FnOnce() -> Option<Value> + Send + 'static,
) -> Option<Receiver<Option<Value>>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("agent".into())
        .spawn(move || {
            let r = solidcraft_engine::io::vfs::scoped(scope, || solidcraft_engine::doc::progress::watch(progress, f));
            let _ = tx.send(r);
        })
        .ok()?;
    Some(rx)
}

/// How far a request evaluating ahead has got: the step running (from 1) of how many, and
/// each step's command as the toolbar names it.
pub(crate) struct Steps {
    now: AtomicUsize,
    labels: Vec<String>,
    /// The batch's steps after those computed ahead (the first that can't ends them), for the
    /// list of what is still to come only.
    later: Vec<String>,
    total: usize,
    /// The evaluation's progress inside the step it is on, and its Cancel: Stop stops it at its
    /// next feature rather than leaving it to compute unseen.
    pub(crate) progress: Arc<solidcraft_engine::doc::progress::Progress>,
}

impl Steps {
    fn new(app: &SolveApp, request: &Value, commands: &[(String, Value)]) -> Arc<Steps> {
        // A batch counts all its steps, also those after the ones computed ahead.
        let total = request.get("commands").and_then(Value::as_array).map_or(commands.len(), Vec::len).max(commands.len());
        Arc::new(Steps {
            now: AtomicUsize::new(0),
            labels: commands.iter().map(|(c, _)| label(app, c)).collect(),
            later: batch_runs(request).into_iter().skip(commands.len()).flatten().map(|(c, _)| label(app, &c)).collect(),
            total,
            progress: Default::default(),
        })
    }

    /// What the chip says: the command running, and in a batch which step it is
    /// (`3/21 · Extrude`).
    pub(crate) fn shown(&self) -> String {
        let (done, total) = self.counts();
        let label = self.current();
        if total > 1 { format!("{}/{total} · {label}", done + 1) } else { label }
    }

    /// The command running, as the toolbar names it.
    pub(crate) fn current(&self) -> String {
        let now = self.now.load(Ordering::Relaxed).max(1);
        self.labels.get(now - 1).or(self.labels.last()).cloned().unwrap_or_default()
    }

    /// The steps still to come, as (index, command), after the one running.
    pub(crate) fn upcoming(&self) -> impl Iterator<Item = (usize, &str)> {
        let running = self.now.load(Ordering::Relaxed).max(1);
        self.labels.iter().chain(&self.later).enumerate().skip(running).map(|(i, l)| (i, l.as_str()))
    }

    /// Steps done (the one running not yet) and how many there are.
    pub(crate) fn counts(&self) -> (usize, usize) {
        (self.now.load(Ordering::Relaxed).max(1).saturating_sub(1).min(self.total), self.total)
    }
}

/// Evaluate `commands` ahead on a copy of the session, on a worker (with the folder scope
/// `request` names, stopping early once `ticket` is cancelled). What it computes is kept for the
/// run here.
fn evaluate_ahead(
    app: &SolveApp,
    request: &Value,
    ticket: &Ticket,
    commands: Vec<(String, Value)>,
) -> Option<(Receiver<Option<Value>>, Arc<KeptResults>, Arc<Steps>)> {
    let scope = crate::control::request_scope(app, request).ok()?;
    let ahead = app.session.ahead();
    let kept = ahead.kept();
    let steps = Steps::new(app, request, &commands);
    let counting = steps.clone();
    let rx = spawn(scope, progress_of(ticket), move || {
        let _ = ahead.evaluate_counting(&commands, &counting.now);
        None
    })?;
    Some((rx, kept, steps))
}

/// Start computing a design change ahead, for the design as it is now.
fn begin_ahead(
    app: &SolveApp,
    req: &ControlRequest,
    commands: Vec<(String, Value)>,
    tries: u8,
) -> Option<(Work, Receiver<Option<Value>>, Arc<Steps>)> {
    let (rx, kept, steps) = evaluate_ahead(app, &req.params, &req.ticket, commands.clone())?;
    Some((Work::Ahead { commands, doc: app.session.doc.clone(), revision: app.session.revision, tries, kept }, rx, steps))
}

impl SolveApp {
    /// Run a control request here with what was computed ahead for it at hand ([`KeptResults`]):
    /// its features are found there rather than computed again on the window thread.
    pub(crate) fn run_with_kept<R>(&mut self, kept: Option<Arc<KeptResults>>, run: impl FnOnce(&mut SolveApp) -> R) -> R {
        let keeps = kept.is_some();
        if keeps {
            self.session.model.keep(kept);
        }
        let r = run(self);
        if keeps {
            self.session.model.keep(None);
        }
        r
    }
}

/// A command the agent cursor acts out, evaluating ahead while the pointer glides to it.
pub struct Ahead {
    done: Receiver<Option<Value>>,
    /// What it computes, for the run once the pointer is there.
    pub(crate) kept: Arc<KeptResults>,
    /// When it started (ms).
    pub(crate) started: f64,
    /// The command, as the toolbar names it.
    pub(crate) label: String,
    /// How far it has got.
    pub(crate) steps: Arc<Steps>,
    /// What it computes, and for which design: `doc` and `revision`, as in [`Work::Ahead`].
    commands: Vec<(String, Value)>,
    doc: Arc<solidcraft_engine::doc::Document>,
    revision: u64,
    tries: u8,
}

impl Ahead {
    /// Start evaluating the design changes of the `request` (`method`: an `engine.execute` or an
    /// `engine.batch`) the pointer acts out; `None` when it just runs once the pointer is there
    /// (not computed off the window thread, or nothing to compute).
    pub(crate) fn begin(app: &SolveApp, method: &str, request: &Value, ticket: &Ticket) -> Option<Ahead> {
        app.apply_off_thread?;
        if !app.preferences.agent_off_thread || app.file_load_holds() || app.file_picker.is_some() {
            return None;
        }
        Self::start(app, request, ticket, ahead_commands(method, request), 0)
    }

    fn start(app: &SolveApp, request: &Value, ticket: &Ticket, commands: Vec<(String, Value)>, tries: u8) -> Option<Ahead> {
        let label = label(app, &commands.first()?.0);
        let (done, kept, steps) = evaluate_ahead(app, request, ticket, commands.clone())?;
        let (doc, revision) = (app.session.doc.clone(), app.session.revision);
        Some(Ahead { done, kept, started: crate::now_ms(), label, steps, commands, doc, revision, tries })
    }

    /// A person's edit went first (as for [`Work::Ahead`]): this was computed for another design.
    pub(crate) fn stale(&self, app: &SolveApp) -> bool {
        (!Arc::ptr_eq(&app.session.doc, &self.doc) || app.session.revision != self.revision) && self.tries < MAX_TRIES
    }

    /// Computed again for the design as it is now, off the window thread.
    pub(crate) fn restart(&self, app: &SolveApp, request: &Value, ticket: &Ticket) -> Option<Ahead> {
        let again = Self::start(app, request, ticket, self.commands.clone(), self.tries + 1)?;
        ticket.computing(false);
        Some(Ahead { started: self.started, ..again })
    }

    /// The evaluation is in (or its worker ended).
    pub(crate) fn finished(&self) -> bool {
        !matches!(self.done.try_recv(), Err(TryRecvError::Empty))
    }
}

/// The design changes a request (`method`, `params`) would make that can be computed ahead: an
/// `engine.execute` of such a command, or the steps of a batch up to the first that can't.
/// Empty: it just runs.
fn ahead_commands(method: &str, p: &Value) -> Vec<(String, Value)> {
    match method {
        "engine.execute" | "command" => match executed(p) {
            Some((command, params)) if computes_ahead(command, &params) => vec![(command.to_string(), params)],
            _ => Vec::new(),
        },
        "engine.batch" => {
            // Timings measure the run here: it must do the work.
            if ["timings", "profile"].iter().any(|k| p.get(*k).and_then(Value::as_bool) == Some(true)) {
                return Vec::new();
            }
            batch_runs(p).into_iter().map_while(|c| c.filter(|(command, params)| computes_ahead(command, params))).collect()
        }
        _ => Vec::new(),
    }
}

/// Every step of a batch as it will run: its command and params, `None` for a step that names
/// no command.
fn batch_runs(p: &Value) -> Vec<Option<(String, Value)>> {
    let Some(steps) = p.get("commands").and_then(Value::as_array) else { return Vec::new() };
    // With `coalesce_parameters`, a run of parameter edits is one edit (#5822).
    let coalesce = p.get("coalesce_parameters").and_then(Value::as_bool).unwrap_or(false);
    solidcraft_engine::batch_params::plan(steps, coalesce)
        .into_iter()
        .map(|run| match run {
            solidcraft_engine::batch_params::BatchRun::Step(i) => {
                let step = steps.get(i)?;
                let command = step.get("command").or_else(|| step.get("id")).and_then(Value::as_str)?;
                Some((command.to_string(), step.get("params").cloned().filter(|v| !v.is_null()).unwrap_or(json!({}))))
            }
            solidcraft_engine::batch_params::BatchRun::Params { changes, .. } => {
                Some(("parameters.change".to_string(), solidcraft_engine::batch_params::combined_params(&changes)))
            }
        })
        .collect()
}

/// Wait the app's wait for a worker: `Some(result)` when it finished within it.
fn within_wait(app: &SolveApp, rx: &Receiver<Option<Value>>) -> Option<Option<Value>> {
    let wait = app.apply_off_thread?;
    if wait.is_zero() {
        return None;
    }
    match rx.recv_timeout(wait) {
        Ok(r) => Some(r),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Some(None),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
    }
}

impl SolveApp {
    /// Since when an agent's design change has been computing off the window thread (ms), and
    /// its command.
    pub fn agent_computing(&self) -> Option<(f64, String)> {
        self.control_job
            .as_ref()
            .map(|j| (j.started, j.steps.as_ref().map_or_else(|| j.label.clone(), |s| s.shown())))
            .or_else(|| self.agent.computing())
    }

    /// How far the agent's design change computing off the window thread is, when it counts its
    /// steps ([`Steps`]).
    pub(crate) fn agent_steps(&self) -> Option<Arc<Steps>> {
        self.control_job.as_ref().map_or_else(|| self.agent.computing_steps(), |j| j.steps.clone())
    }

    /// The steps of the computing batch still to come, as (number from 1, command), at most
    /// [`MAX_NEXT_SHOWN`].
    pub(crate) fn agent_next(&self) -> Vec<(usize, String)> {
        let steps = self.agent_steps();
        steps.iter().flat_map(|s| s.upcoming()).take(MAX_NEXT_SHOWN).map(|(i, l)| (i + 1, l.to_string())).collect()
    }

    /// The design changes held behind what is running, in the order they run, as (serial, command).
    pub(crate) fn agent_held(&self) -> Vec<(u64, String)> {
        let held = self.control_held.iter().filter(|r| changes_the_design(&r.method));
        held.map(|r| {
            let first = match r.method.as_str() {
                "engine.execute" | "command" => executed(&r.params).map(|(c, _)| c),
                _ => r.params.get("commands").and_then(Value::as_array).and_then(|s| s.first()).and_then(crate::control::batch_step_command),
            };
            let is_batch = matches!(r.method.as_str(), "engine.batch" | "engine.script");
            let steps: Vec<String> = if is_batch {
                let all = r.params.get("commands").and_then(Value::as_array);
                all.into_iter().flatten().take(4).filter_map(crate::control::batch_step_command).map(|c| label(self, c)).collect()
            } else {
                Vec::new()
            };
            if steps.len() > 1 {
                let count = r.params.get("commands").and_then(Value::as_array).map_or(0, Vec::len);
                let names = if count > steps.len() { steps[..3.min(steps.len())].iter().cloned().chain(["…".into()]).collect() } else { steps };
                let language = solidcraft_ui_text::Locale::for_language(&self.preferences.language);
                return (r.serial, crate::agent_label::batch_in(language, &names));
            }
            (r.serial, label(self, first.unwrap_or(&r.method)))
        })
        .collect()
    }

    /// Cancel the held request with this serial (its client is told it was cancelled; it never runs).
    pub(crate) fn cancel_held(&mut self, serial: u64) {
        let at = self.control_held.iter().position(|r| r.serial == serial);
        if let Some(req) = at.and_then(|at| self.control_held.remove(at)) {
            req.ticket.cancel();
            let _ = req.reply.send(cancelled_reply(&req.method));
        }
    }

    /// Stop waiting for the agent's design change that is computing: it is answered as stopped
    /// and nothing of it is applied (its worker finishes on its own, unseen). Requests waiting
    /// behind it go on.
    pub fn stop_agent_command(&mut self) {
        // Its evaluation stops at its next feature: nothing of it is wanted any more.
        if let Some(steps) = self.agent_steps() {
            steps.progress.cancel();
        }
        crate::operations::ended(self, "agent", crate::operations::Outcome::Cancelled);
        let stopped = match self.control_job.take() {
            Some(job) => {
                job.req.ticket.stop();
                let writes_file = matches!(job.work, Work::Plugin { .. });
                Some((job.req.reply, job.label, writes_file))
            }
            None => self.agent.stop().map(|(reply, label)| (reply, label, false)),
        };
        if let Some((reply, label, writes_file)) = stopped {
            let _ = reply.send(json!({
                "ok": false,
                "stopped": true,
                "error": if writes_file {
                    format!("stopped: the person at the app stopped `{label}` while it computed; the design is unchanged, but its file may still be written")
                } else {
                    format!("stopped: the person at the app stopped `{label}` while it computed; nothing was applied")
                },
            }));
        }
    }

    /// Take a control request off the window thread when it computes long (see the module
    /// notes). Returns it when it should simply run here now.
    pub(crate) fn offload_control(&mut self, ctx: &egui::Context, req: ControlRequest) -> Option<ControlRequest> {
        crate::plugins::sync_policy(self);
        if cfg!(not(target_arch = "wasm32"))
            && matches!(req.method.as_str(), "engine.execute" | "command")
            && let Some((command, params)) = executed(&req.params)
            && command.eq_ignore_ascii_case("plugin.run")
        {
            if self.control_job.is_some() {
                self.control_held.push_back(req);
                return None;
            }
            let scope = match crate::control::request_scope(self, &req.params) {
                Ok(scope) => scope,
                Err(e) => {
                    let _ = req.reply.send(json!({"ok":false,"error":e}));
                    return None;
                }
            };
            let Ok(level) = crate::control::request_param_check(&req.method, &req.params) else { return Some(req) };
            if crate::control_params::parse::<crate::control_params::Execute>(&req.params).is_err() {
                return Some(req);
            }
            let mut copy = self.session.scratch();
            if let Some(level) = level {
                copy.param_check = level;
            }
            let quality = solidcraft_engine::doc::display_quality::current();
            let (tx, prepared) = std::sync::mpsc::channel();
            #[cfg(test)]
            let gate = self.apply_gate.clone();
            let progress = progress_of(&req.ticket);
            let warm_progress = progress.clone();
            let Some(done) = spawn(scope, progress, move || {
                #[cfg(test)]
                if let Some(gate) = gate {
                    let _ = gate.lock().ok()?.recv();
                }
                let result = copy
                    .compute_commands(&[("plugin.run".into(), params)], None)
                    .and_then(|prepared| {
                        warm_plugin_display(&prepared, quality, &warm_progress)?;
                        Ok(prepared)
                    })
                    .map_err(|e| e.to_string());
                let _ = tx.send(result);
                None
            }) else {
                let _ = req.reply.send(json!({"ok":false,"error":self.tr("ui.plugin_start_failed")}));
                return None;
            };
            req.ticket.computing(true);
            self.control_job = Some(ControlJob {
                label: label(self, "plugin.run"),
                req,
                work: Work::Plugin { prepared, disabled: self.preferences.disabled_plugins.clone(), limits: self.session.plugin_limits, level },
                done,
                started: crate::now_ms(),
                steps: None,
            });
            return None;
        }
        if self.apply_off_thread.is_none() || self.file_load_holds() || self.file_picker.is_some() {
            return Some(req);
        }
        // Reading the design (a measure, an inspection that measures): on a copy, answered from
        // there, preference or not. On a large assembly it took a minute on the window thread,
        // and the window, answering nothing meanwhile, was closed as not responding (#4604).
        if self.control_answering.len() < MAX_ANSWERING
            && let Some((id, params)) = reading(&req)
        {
            return self.answer_on_a_copy(req, id, params);
        }
        if !self.preferences.agent_off_thread {
            return Some(req);
        }
        // An export: on a copy, answered from there.
        if matches!(req.method.as_str(), "engine.execute" | "command")
            && self.control_answering.len() < MAX_ANSWERING
            && let Some((command, params)) = executed(&req.params)
            && let Some(id) = spec_id(command).filter(|id| ANSWERED_ON_A_COPY.contains(id))
        {
            return self.answer_on_a_copy(req, id, params);
        }
        let commands = ahead_commands(&req.method, &req.params);
        let Some(first) = commands.first().map(|(c, _)| c.clone()) else { return Some(req) };
        let Some((work, rx, steps)) = begin_ahead(self, &req, commands, 0) else { return Some(req) };
        let job = ControlJob { label: label(self, &first), req, work, done: rx, started: crate::now_ms(), steps: Some(steps) };
        if within_wait(self, &job.done).is_some() {
            // Computed within the wait: it runs now, as before, finding what was computed.
            let Work::Ahead { kept, .. } = job.work else { return Some(job.req) };
            self.run_with_kept(Some(kept), |app| app.run_control(ctx, job.req));
            return None;
        }
        job.req.ticket.computing(false);
        self.control_job = Some(job);
        None
    }

    /// Run command `id` on a copy of the session, off the window thread, and answer `req` from
    /// there: at once when it is done within the app's wait, otherwise once it is in. `None`: it
    /// is taken care of; `Some`: it runs here (no threads).
    fn answer_on_a_copy(&mut self, req: ControlRequest, id: &'static str, params: Value) -> Option<ControlRequest> {
        let Ok(scope) = crate::control::request_scope(self, &req.params) else { return Some(req) };
        // The request's own level (#9184) checks it on the copy, and the reply says so, as
        // `control::handle` does; a word that is no level is refused there.
        let Ok(level) = crate::control::request_param_check(&req.method, &req.params) else { return Some(req) };
        let mut copy = self.session.scratch();
        if let Some(level) = level {
            copy.param_check = level;
        }
        // The copy has no history and no file (#9624): an inspection reports the design's own.
        let s = &self.session;
        let own = (id == "document.inspect").then(|| (s.undo.len(), s.redo.len(), s.path.clone()));
        let inspection = (id == "plugin.inspect").then(|| (self.session.uid, next_plugin_inspection()));
        #[cfg(test)]
        let gate = inspection.and(self.apply_gate.clone());
        let rx = spawn(scope, progress_of(&req.ticket), move || {
            #[cfg(test)]
            if let Some(gate) = gate {
                let _ = gate.lock().ok()?.recv();
            }
            let mut reply = match copy.execute(id, &params) {
                Ok(mut v) => {
                    if let (Some((undo, redo, path)), Some(o)) = (own, v.as_object_mut()) {
                        o.insert("undo".into(), json!(undo));
                        o.insert("redo".into(), json!(redo));
                        o.insert("path".into(), json!(path));
                    }
                    json!({"ok": true, "result": v})
                }
                Err(e) => json!({"ok": false, "error": e.to_string()}),
            };
            if let Some(level) = level {
                reply["param_check"] = json!(level.name());
            }
            Some(reply)
        });
        let Some(rx) = rx else { return Some(req) };
        let job = ControlJob { label: label(self, id), req, work: Work::Answer { inspection }, done: rx, started: crate::now_ms(), steps: None };
        match within_wait(self, &job.done) {
            Some(reply) => self.answered(job, reply),
            None => {
                // Stopped, an export's worker may still write its file; a reading writes none.
                job.req.ticket.computing(ANSWERED_ON_A_COPY.contains(&id));
                self.control_answering.push(job);
            }
        }
        None
    }

    /// An export's or a reading's reply, from its copy, with the echo an export leaves when it
    /// runs here.
    fn answered(&mut self, job: ControlJob, reply: Option<Value>) {
        if job.req.refuse_cancelled_chat() {
            return;
        }
        if !job.req.ticket.back() {
            let _ = job.req.reply.send(stopped_reply(&job.label, true));
            return;
        }
        let mut reply = reply.unwrap_or_else(|| json!({"ok": false, "error": format!("`{}` ended without a reply", job.label)}));
        // Inspection runs on a scratch session. Retain its inert registration so namespaced
        // entries resolve on the live session without loading or executing code on the window.
        if reply["ok"] == true
            && let Some((command, params)) = executed(&job.req.params)
            && spec_id(command) == Some("plugin.inspect")
            && let (Some(id), Some(path)) = (reply.pointer("/result/manifest/id").and_then(Value::as_str), params.get("path").and_then(Value::as_str))
        {
            let current = match job.work {
                Work::Answer { inspection: Some((uid, version)) } if uid == self.session.uid => {
                    let key = (uid, id.to_owned());
                    if self.plugin_inspection_versions.get(&key).is_some_and(|latest| *latest > version) {
                        false
                    } else {
                        self.plugin_inspection_versions.insert(key, version);
                        true
                    }
                }
                _ => false,
            };
            if current {
                self.session.plugin_state.packages.insert(id.to_owned(), path.to_owned());
            } else {
                reply = json!({"ok":false,"error":self.tr("ui.plugin_context_changed")});
            }
        }
        let export = executed(&job.req.params).and_then(|(command, _)| spec_id(command)).is_some_and(|id| ANSWERED_ON_A_COPY.contains(&id));
        if reply["ok"] == true && export {
            let language = solidcraft_ui_text::Locale::for_language(&self.preferences.language);
            self.session.echo(language.format("ui.command_done", &[("command", job.label.as_str())]));
        }
        let _ = job.req.reply.send(reply);
    }

    /// Each frame: exports that are done reply; a design change whose evaluation is in runs.
    /// Those their client cancelled are answered as stopped.
    pub(crate) fn poll_control_jobs(&mut self, ctx: &egui::Context) {
        if !self.agent_replies.is_empty() {
            self.forward_agent_replies();
        }
        if !self.control_answering.is_empty() {
            let mut i = 0;
            while i < self.control_answering.len() {
                if self.control_answering.get(i).is_some_and(|job| job.req.refuse_cancelled_chat()) {
                    self.control_answering.remove(i);
                    continue;
                }
                let cancelled = self.control_answering.get(i).is_some_and(|j| j.req.ticket.is_cancelled());
                match self.control_answering.get(i).and_then(ControlJob::poll).or_else(|| cancelled.then_some(None)) {
                    Some(reply) => {
                        let job = self.control_answering.remove(i);
                        self.answered(job, reply);
                    }
                    None => i += 1,
                }
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let Some(job) = &self.control_job else { return };
        if job.req.refuse_cancelled_chat() {
            self.control_job = None;
            return;
        }
        // The design it computes for has left the window (closed, another tab, a file opened):
        // refused now, however long its computation has left, so the requests behind it, which
        // are for the design shown, don't wait for it.
        let left = matches!(&job.work, Work::Ahead { doc, .. } if !shows_design(&self.session, doc));
        if left {
            if let Some(job) = self.control_job.take() {
                let _ = job.req.reply.send(design_replaced(&job.label));
            }
            return;
        }
        if job.poll().is_none() && !job.req.ticket.is_cancelled() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            return;
        }
        let Some(job) = self.control_job.take() else { return };
        ctx.request_repaint();
        if let Work::Plugin { prepared, disabled, limits, level } = &job.work {
            if !job.req.ticket.back() {
                let _ = job.req.reply.send(stopped_reply(&job.label, true));
                return;
            }
            let result = if self.file_load_holds()
                || self.file_picker.is_some()
                || disabled != &self.preferences.disabled_plugins
                || *limits != crate::plugins::host_limits(&self.preferences)
            {
                Err(self.tr("ui.plugin_context_changed").to_string())
            } else {
                prepared
                    .try_recv()
                    .map_err(|_| self.tr("ui.plugin_no_result").to_string())
                    .and_then(|r| r)
                    .and_then(|prepared| {
                        let before = self.session.doc.clone();
                        // Move the redo stack instead of cloning the whole history on the window thread.
                        let redo = std::mem::take(&mut self.session.redo);
                        let replies = self.session.accept_commands(prepared);
                        if replies.is_err() || Arc::ptr_eq(&before, &self.session.doc) {
                            self.session.redo = redo;
                        }
                        replies.map_err(|_| self.tr("ui.plugin_context_changed").to_string())
                    })
                    .and_then(|replies| replies.into_iter().last().map(|(_, reply)| reply).ok_or_else(|| self.tr("ui.plugin_no_result").to_string()))
            };
            self.big_operation |= crate::now_ms() - job.started >= crate::recovery_ui::BIG_OPERATION_MS;
            let result = result.and_then(|reply| {
                if let Some(exits) = reply.get("sketch_exits").and_then(Value::as_array) {
                    for sketch in exits {
                        self.sketch_closed(sketch.as_u64());
                    }
                }
                crate::render_workspace::apply_command(self, "plugin.run", &reply)?;
                Ok(reply)
            });
            let result = self.command_completed("plugin.run", result);
            let mut reply = match result {
                Ok(result) => json!({"ok":true,"result":result}),
                Err(error) => json!({"ok":false,"error":error}),
            };
            if let Some(level) = level {
                reply["param_check"] = json!(level.name());
            }
            let _ = job.req.reply.send(reply);
            ctx.request_repaint();
            return;
        }
        // A file opening meanwhile: the request waits for it as any design change does.
        if (self.file_load_holds() || self.file_picker.is_some()) && job.req.ticket.wait_again() {
            self.control_held.push_front(job.req);
            return;
        }
        // Cancelled by its client while it computed (requests behind it go on).
        if !job.req.ticket.back() {
            let _ = job.req.reply.send(stopped_reply(&job.label, false));
            return;
        }
        let Work::Ahead { commands, doc, revision, tries, kept } = job.work else { return };
        let s = &self.session;
        let same = Arc::ptr_eq(&s.doc, &doc);
        if !shows_design(s, &doc) {
            let _ = job.req.reply.send(design_replaced(&job.label));
            return;
        }
        if (!same || s.revision != revision) && tries < MAX_TRIES {
            // A person's edit went first: computed again for the design as it is now.
            if let Some((work, rx, steps)) = begin_ahead(self, &job.req, commands, tries + 1) {
                job.req.ticket.computing(false);
                self.control_job = Some(ControlJob { work, done: rx, steps: Some(steps), ..job });
                return;
            }
        }
        self.big_operation |= crate::now_ms() - job.started >= crate::recovery_ui::BIG_OPERATION_MS;
        // Its reply goes to the agent as ever, seen on the way for the operations panel.
        let mut req = job.req;
        let (tap, tapped) = std::sync::mpsc::channel();
        let reply_to = std::mem::replace(&mut req.reply, tap);
        self.run_with_kept(Some(kept), |app| app.run_control(ctx, req));
        self.agent_replies.push((tapped, reply_to));
        self.forward_agent_replies();
    }

    /// Pass the replies of agents' commands that computed off the window thread on to the agent,
    /// noting each command's outcome (a reply held for later goes on when it comes).
    fn forward_agent_replies(&mut self) {
        let mut i = 0;
        while i < self.agent_replies.len() {
            let Some((tapped, _)) = self.agent_replies.get(i) else { break };
            match tapped.try_recv() {
                Ok(reply) => {
                    let (_, reply_to) = self.agent_replies.remove(i);
                    // A batch stopped by a failing step answers `ok` with that step in `failed`.
                    let failed_step = reply["result"]["failed"].as_object().map(|step| {
                        let (command, error) = (step.get("command").and_then(Value::as_str), step.get("error").and_then(Value::as_str));
                        error.map_or_else(|| "a step failed".to_string(), |e| command.map_or_else(|| e.to_string(), |c| crate::friendly_error(c, e)))
                    });
                    let outcome = if let Some(why) = failed_step {
                        crate::operations::Outcome::Failed(why)
                    } else if reply["ok"] == true {
                        crate::operations::Outcome::Done
                    } else if reply["stopped"] == true {
                        crate::operations::Outcome::Cancelled
                    } else {
                        crate::operations::Outcome::Failed(reply["error"].as_str().unwrap_or("failed").to_string())
                    };
                    crate::operations::ended(self, "agent", outcome);
                    let _ = reply_to.send(reply);
                }
                // Dropped unanswered: the agent's own receiver sees that too, as before.
                Err(TryRecvError::Disconnected) => {
                    self.agent_replies.remove(i);
                }
                Err(TryRecvError::Empty) => i += 1,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A computing batch is one row of the operations panel; its tooltip and `next` list the
    /// steps still to come (at most [`MAX_NEXT_SHOWN`]) and move up as the batch advances.
    #[test]
    fn a_computing_batch_is_one_row_listing_its_next_steps() {
        let mut h = crate::scenario::Harness::new();
        let hole = |x: f64| json!({"command": "solid.cylinder", "params": {"base": [x, 10, -1], "axis": [0, 0, 1], "radius": 3, "height": 20, "operation": "cut"}});
        let request = json!({"commands": (0..20).map(|i| hole(20.0 + 3.0 * f64::from(i))).collect::<Vec<_>>()});
        let commands = ahead_commands("engine.batch", &request);
        assert_eq!(commands.len(), 20);
        let steps = Steps::new(&h.app, &request, &commands);
        let (req, _reply) = ControlRequest::new("engine.batch", request);
        // The worker never answers: the batch stays where the test puts it.
        let (_worker, done) = std::sync::mpsc::channel();
        h.app.control_job =
            Some(ControlJob { req, work: Work::Answer { inspection: None }, done, started: 0.0, label: String::new(), steps: Some(steps.clone()) });
        let rows = |h: &mut crate::scenario::Harness| {
            let listed = crate::operations::control(&mut h.app, &json!({})).unwrap();
            listed["operations"].as_array().unwrap().clone()
        };
        for (running, first, count) in [(6, 7, MAX_NEXT_SHOWN), (19, 20, 1), (20, 0, 0)] {
            steps.now.store(running, Ordering::Relaxed);
            let rows = rows(&mut h);
            assert_eq!(rows.len(), 1, "a batch is one operation: {rows:?}");
            assert_eq!(rows[0]["steps"], json!({"done": running - 1, "total": 20}));
            let next = rows[0]["next"].as_array().unwrap();
            assert_eq!(next.len(), count, "running step {running}: {next:?}");
            assert!(next.iter().all(|n| n.as_str().is_some_and(|n| !n.is_empty() && !n.contains('.'))), "{next:?}");
            let numbers: Vec<_> =
                crate::operations::current(&h.app).iter().find(|r| r.key == "agent").unwrap().next.iter().map(|(n, _)| *n).collect();
            assert_eq!(numbers, (first..first + count).collect::<Vec<_>>());
        }
        // A step that can't be computed ahead ends the prefix, but the steps after it still come.
        let box_step = json!({"command": "solid.box", "params": {"corner": [0, 0, 0], "length": 5, "width": 5, "height": 5, "operation": "join"}});
        let mixed = json!({"commands": [box_step, {"command": "file.export_step", "params": {"path": "out.step"}}, hole(20.0)]});
        let commands = ahead_commands("engine.batch", &mixed);
        assert_eq!(commands.len(), 1, "only the box is computed ahead: {commands:?}");
        let steps = Steps::new(&h.app, &mixed, &commands);
        let (req, _reply) = ControlRequest::new("engine.batch", mixed);
        let (_worker, done) = std::sync::mpsc::channel();
        h.app.control_job =
            Some(ControlJob { req, work: Work::Answer { inspection: None }, done, started: 0.0, label: String::new(), steps: Some(steps.clone()) });
        steps.now.store(1, Ordering::Relaxed);
        let rows = rows(&mut h);
        assert_eq!(rows[0]["steps"], json!({"done": 0, "total": 3}));
        let next = rows[0]["next"].as_array().unwrap();
        assert_eq!(next.len(), 2, "the export and the hole: {next:?}");
        assert!(next.iter().all(|n| n.as_str().is_some_and(|n| !n.is_empty())), "{next:?}");
    }
}
