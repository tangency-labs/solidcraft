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
//! everything at once, and so does the app unless the preference is on (Preferences > AI,
//! `agent_off_thread`, off by default).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};

use serde_json::{Value, json};

use solidcraft_engine::doc::KeptResults;

use crate::SolveApp;
use crate::control::ControlRequest;

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
    "inspect.measure",
    "document.inspect",
    "inspect.clearance",
    "inspect.interference",
    // Physical properties of a component or placement: every body of every placement (#8706).
    "component.properties",
    "occurrence.properties",
];

/// Exports and readings computing at once; more run here.
const MAX_ANSWERING: usize = 4;
/// At most this many sends to network folders wait for a worker; more are refused in words.
const MAX_SENDS_WAITING: usize = 64;

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

/// A request that only reads the design and can take long ([`READ_ON_A_COPY`]): an
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

/// Commands that can send to a network folder (#2304).
const SEND_TO_A_FOLDER: &[&str] = &["file.save_mesh", "file.send_to_slicer"];

/// A request that sends to one of the user's network folders: the command and its parameters. A
/// copy to a slow share takes seconds, and the window would answer nothing meanwhile.
fn sending_to_a_network_folder(app: &crate::SolveApp, req: &ControlRequest) -> Option<(&'static str, Value)> {
    if !matches!(req.method.as_str(), "engine.execute" | "command") {
        return None;
    }
    let (command, params) = executed(&req.params)?;
    let id = spec_id(command).filter(|id| SEND_TO_A_FOLDER.contains(id))?;
    let slicer = params.get("slicer").and_then(Value::as_str)?;
    solidcraft_engine::remote_slicers::resolve(&app.preferences.network_folders, slicer)?;
    Some((id, params))
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
    Answer,
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

/// A command to run on a copy of the session, taken when it was asked for. A send to a network
/// folder that waits for a worker is one: it sends the design as it was then.
pub(crate) struct OnACopy {
    req: ControlRequest,
    /// The command, as the toolbar names it.
    label: String,
    /// When it was asked for (ms).
    asked: f64,
    scope: Option<solidcraft_engine::io::vfs::PathScope>,
    /// The command on the copy, and its reply.
    run: Box<dyn FnOnce() -> Option<Value> + Send>,
}

fn label(app: &SolveApp, command: &str) -> String {
    let language = crate::i18n::Locale::for_language(&app.preferences.language);
    language.label(solidcraft_engine::find_command(command).map_or(command, |c| c.label)).to_string()
}

/// Run `f` on a worker thread (with the request's folder scope), answering on the returned
/// channel. `None` without threads (the web).
fn spawn(
    scope: Option<solidcraft_engine::io::vfs::PathScope>,
    f: impl FnOnce() -> Option<Value> + Send + 'static,
) -> Option<Receiver<Option<Value>>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("agent".into())
        .spawn(move || {
            let r = solidcraft_engine::io::vfs::scoped(scope, f);
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
    total: usize,
}

impl Steps {
    fn new(app: &SolveApp, request: &Value, commands: &[(String, Value)]) -> Arc<Steps> {
        // A batch counts all its steps, also those after the ones computed ahead.
        let total = request.get("commands").and_then(Value::as_array).map_or(commands.len(), Vec::len).max(commands.len());
        Arc::new(Steps { now: AtomicUsize::new(0), labels: commands.iter().map(|(c, _)| label(app, c)).collect(), total })
    }

    /// What the chip says: the command running, and in a batch which step it is
    /// (`3/21 · Extrude`).
    pub(crate) fn shown(&self) -> String {
        let now = self.now.load(Ordering::Relaxed).max(1);
        let label = self.labels.get(now - 1).or(self.labels.last()).cloned().unwrap_or_default();
        if self.total > 1 { format!("{now}/{} · {label}", self.total) } else { label }
    }
}

/// Evaluate `commands` ahead on a copy of the session, on a worker (with the folder scope
/// `request` names). What it computes is kept for the run here.
fn evaluate_ahead(
    app: &SolveApp,
    request: &Value,
    commands: Vec<(String, Value)>,
) -> Option<(Receiver<Option<Value>>, Arc<KeptResults>, Arc<Steps>)> {
    let scope = crate::control::request_scope(app, request).ok()?;
    let ahead = app.session.ahead();
    let kept = ahead.kept();
    let steps = Steps::new(app, request, &commands);
    let counting = steps.clone();
    let rx = spawn(scope, move || {
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
    let (rx, kept, steps) = evaluate_ahead(app, &req.params, commands.clone())?;
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
}

impl Ahead {
    /// Start evaluating the design changes of the `request` (`method`: an `engine.execute` or an
    /// `engine.batch`) the pointer acts out; `None` when it just runs once the pointer is there
    /// (not computed off the window thread, or nothing to compute).
    pub(crate) fn begin(app: &SolveApp, method: &str, request: &Value) -> Option<Ahead> {
        app.apply_off_thread?;
        if !app.preferences.agent_off_thread || app.file_load.busy() || app.file_picker.is_some() {
            return None;
        }
        let commands = ahead_commands(method, request);
        let label = label(app, &commands.first()?.0);
        let (done, kept, steps) = evaluate_ahead(app, request, commands)?;
        Some(Ahead { done, kept, started: crate::now_ms(), label, steps })
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
            let Some(steps) = p.get("commands").and_then(Value::as_array) else { return Vec::new() };
            // As the batch will run them: with `coalesce_parameters`, a run of parameter edits
            // as one edit (#5822).
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
                .map_while(|c| c.filter(|(command, params)| computes_ahead(command, params)))
                .collect()
        }
        _ => Vec::new(),
    }
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

    /// Stop waiting for the agent's design change that is computing: it is answered as stopped
    /// and nothing of it is applied (its worker finishes on its own, unseen). Requests waiting
    /// behind it go on.
    pub fn stop_agent_command(&mut self) {
        let stopped = match self.control_job.take() {
            Some(job) => Some((job.req.reply, job.label)),
            None => self.agent.stop(),
        };
        if let Some((reply, label)) = stopped {
            let _ = reply.send(json!({
                "ok": false,
                "stopped": true,
                "error": format!("stopped: the person at the app stopped `{label}` while it computed; nothing was applied"),
            }));
        }
    }

    /// Take a control request off the window thread when it computes long (see the module
    /// notes). Returns it when it should simply run here now.
    pub(crate) fn offload_control(&mut self, ctx: &egui::Context, req: ControlRequest) -> Option<ControlRequest> {
        if self.apply_off_thread.is_none() || self.file_load.busy() || self.file_picker.is_some() {
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
        // Sending to a network folder: on a copy too, whatever the preference, so the window
        // keeps answering while the file is copied (#2304). With every worker busy it waits for
        // one, never running here: a slow share would hold the window up.
        if let Some((id, params)) = sending_to_a_network_folder(self, &req) {
            let now = self.control_answering.len() < MAX_ANSWERING && self.control_sends_waiting.is_empty();
            if !now && self.control_sends_waiting.len() >= MAX_SENDS_WAITING {
                let _ = req.reply.send(json!({"ok": false, "error": "too many sends to network folders are waiting; try again"}));
                return None;
            }
            // The copy is taken now: a send that waits still sends the design as it is now, not
            // an edit made meanwhile or another tab's design.
            let work = match self.send_on_a_copy(req, id, params) {
                Ok(work) => work,
                Err(req) => return Some(req),
            };
            if now {
                return self.start_on_a_copy(work).err();
            }
            self.control_sends_waiting.push_back(work);
            return None;
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
        self.control_job = Some(job);
        None
    }

    /// Run command `id` on a copy of the session, off the window thread, and answer `req` from
    /// there: at once when it is done within the app's wait, otherwise once it is in. `None`: it
    /// is taken care of; `Some`: it runs here (no threads).
    fn answer_on_a_copy(&mut self, req: ControlRequest, id: &'static str, params: Value) -> Option<ControlRequest> {
        match self.on_a_copy(req, id, params) {
            Ok(work) => self.start_on_a_copy(work).err(),
            Err(req) => Some(req),
        }
    }

    /// Command `id` on a copy of the session as it is now, with the request's folder scope and
    /// level; the request back when it can't run on a copy.
    fn on_a_copy(&self, req: ControlRequest, id: &'static str, params: Value) -> Result<OnACopy, ControlRequest> {
        let Ok(scope) = crate::control::request_scope(self, &req.params) else { return Err(req) };
        // The request's own level (#9184) checks it on the copy, and the reply says so, as
        // `control::handle` does; a word that is no level is refused there.
        let Ok(level) = crate::control::request_param_check(&req.method, &req.params) else { return Err(req) };
        let mut copy = self.session.scratch();
        if let Some(level) = level {
            copy.param_check = level;
        }
        // The copy has no history and no file (#9624): an inspection reports the design's own.
        let s = &self.session;
        let own = (id == "document.inspect").then(|| (s.undo.len(), s.redo.len(), s.path.clone()));
        let run = Box::new(move || {
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
        Ok(OnACopy { label: label(self, id), asked: crate::now_ms(), req, scope, run })
    }

    /// Start `work` on a worker; the request back when no worker could start.
    fn start_on_a_copy(&mut self, work: OnACopy) -> Result<(), ControlRequest> {
        let OnACopy { req, label, asked, scope, run } = work;
        let Some(rx) = spawn(scope, run) else { return Err(req) };
        let job = ControlJob { label, req, work: Work::Answer, done: rx, started: asked, steps: None };
        match within_wait(self, &job.done) {
            Some(reply) => self.answered(job, reply),
            None => self.control_answering.push(job),
        }
        Ok(())
    }

    /// A send to a network folder, on a copy that takes the user's folders as they are now; the
    /// request back when it can't run there.
    fn send_on_a_copy(&mut self, req: ControlRequest, id: &'static str, params: Value) -> Result<OnACopy, ControlRequest> {
        self.session.network_folders = solidcraft_engine::remote_slicers::sanitized(&self.preferences.network_folders);
        self.on_a_copy(req, id, params)
    }

    /// An export's or a reading's reply, from its copy, with the echo an export leaves when it
    /// runs here.
    fn answered(&mut self, job: ControlJob, reply: Option<Value>) {
        if job.req.refuse_cancelled_chat() {
            return;
        }
        let reply = reply.unwrap_or_else(|| json!({"ok": false, "error": format!("`{}` ended without a reply", job.label)}));
        let export = executed(&job.req.params)
            .and_then(|(command, _)| spec_id(command))
            .is_some_and(|id| ANSWERED_ON_A_COPY.contains(&id) || SEND_TO_A_FOLDER.contains(&id));
        if reply["ok"] == true && export {
            let language = crate::i18n::Locale::for_language(&self.preferences.language);
            self.session.echo(language.format("ui.command_done", &[("command", job.label.as_str())]));
        }
        let _ = job.req.reply.send(reply);
    }

    /// Each frame: exports that are done reply; a design change whose evaluation is in runs.
    pub(crate) fn poll_control_jobs(&mut self, ctx: &egui::Context) {
        if !self.control_answering.is_empty() {
            let mut i = 0;
            while i < self.control_answering.len() {
                if self.control_answering.get(i).is_some_and(|job| job.req.refuse_cancelled_chat()) {
                    self.control_answering.remove(i);
                    continue;
                }
                match self.control_answering.get(i).and_then(ControlJob::poll) {
                    Some(reply) => {
                        let job = self.control_answering.remove(i);
                        self.answered(job, reply);
                    }
                    None => i += 1,
                }
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        // Sends that waited for a worker, in order, as workers come free, each on the copy taken
        // when it was asked for; never here, where a slow share would hold the window up.
        while self.control_answering.len() < MAX_ANSWERING
            && let Some(work) = self.control_sends_waiting.pop_front()
        {
            if work.req.refuse_cancelled_chat() {
                continue;
            }
            if let Err(req) = self.start_on_a_copy(work) {
                let _ = req.reply.send(json!({"ok": false, "error": "no worker could start the send to the network folder; try again"}));
            }
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
        if job.poll().is_none() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            return;
        }
        let Some(job) = self.control_job.take() else { return };
        // A file opening meanwhile: the request waits for it as any design change does.
        if self.file_load.busy() || self.file_picker.is_some() {
            self.control_held.push_front(job.req);
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
                self.control_job = Some(ControlJob { work, done: rx, steps: Some(steps), ..job });
                return;
            }
        }
        self.big_operation |= crate::now_ms() - job.started >= crate::recovery_ui::BIG_OPERATION_MS;
        self.run_with_kept(Some(kept), |app| app.run_control(ctx, job.req));
        ctx.request_repaint();
    }
}
