//! Interactive file operations run off the window thread. The old design stays intact until
//! the command and display meshes are ready; failures and cancelled jobs never replace it.
//!
//! While one runs, the window keeps drawing frames: the title bar still moves, resizes,
//! minimizes and maximizes the window, and a light overlay over the design shows the file's
//! progress with Cancel. The overlay takes the pointer, shortcuts and commands wait, and
//! control-channel commands that change the design are held until the file is in. A control
//! (MCP) `doc.open` or import is answered when its file finishes loading.
//!
//! Update Links from the toolbar runs the same way (#5329): it reads every linked design again
//! and rebuilds it, which takes a while in an assembly of many parts, so the overlay names the
//! linked design being read and its rebuild's progress.

use crate::ui_kit::{Container, Surface};
use std::collections::VecDeque;
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
    mpsc::{Receiver, Sender, TryRecvError},
};

use serde_json::{Value, json};
use solidcraft_engine::{Session, doc::Document, doc::progress::Progress};

use crate::SolveApp;
use crate::control::ControlResponse;
use crate::ui_kit;

/// What a file operation does with its file.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// Open the design (or a CAD, mesh or drawing file as a new design) in a tab of its own.
    Open,
    /// Insert CAD bodies, meshes or sketch drawing geometry into the active design.
    Insert,
    /// Open a design an earlier run autosaved before it closed.
    Recover { id: String, dir: Option<String> },
    /// Export a snapshot of the current design without adopting the worker's session.
    ExportDxf,
    /// Read the active design's linked designs again (`component.update_links`, with the
    /// request's options: `component` or `components` reads only those links).
    UpdateLinks,
}

pub struct Request {
    /// The file, or for a recovery the design's name.
    pub path: String,
    pub kind: Kind,
    /// A control-channel caller waiting for the command's reply.
    pub reply: Option<Sender<ControlResponse>>,
    /// The folders that caller's agent may use: the worker reads the file inside them.
    pub scope: Option<solidcraft_engine::io::vfs::PathScope>,
    /// What that caller asked of an open or import besides its path (`up`, `names`, `matrix`,
    /// `name`, ...): the command gets them as given and checks them as it does run directly.
    pub options: serde_json::Map<String, Value>,
    /// Network servers the user allowed for this one request ("Allow once", `network_prompt`).
    pub allow_hosts: Vec<String>,
    /// The saved design this request is for, when it must not run on another: one shown in its
    /// place by the time the request starts drops it.
    pub design: Option<String>,
    /// The chat turn that requested the load. It must still be current when the file lands.
    pub(crate) chat_turn: Option<Arc<std::sync::atomic::AtomicBool>>,
}

impl Request {
    pub fn new(path: &str, kind: Kind) -> Self {
        Request {
            path: path.into(),
            kind,
            reply: None,
            scope: solidcraft_engine::io::vfs::current_scope(),
            options: Default::default(),
            allow_hosts: Vec::new(),
            design: None,
            chat_turn: None,
        }
    }
    fn insert(&self) -> bool {
        self.kind == Kind::Insert
    }
    /// The command's parameters: the path and the caller's import options.
    fn params(&self) -> Value {
        let mut params = self.options.clone();
        params.insert("path".into(), json!(self.path));
        Value::Object(params)
    }
    fn command(&self) -> (&'static str, Value) {
        match &self.kind {
            Kind::Open => ("doc.open", self.params()),
            Kind::Insert if solidcraft_engine::io::is_mesh_path(&self.path) => ("file.insert_mesh", self.params()),
            Kind::Insert if solidcraft_engine::io::is_point_cloud_path(&self.path) => ("pointcloud.insert", self.params()),
            Kind::Insert => (insert_command(&self.path), self.params()),
            Kind::Recover { id, dir } => ("doc.recover", json!({"id": id, "dir": dir})),
            Kind::ExportDxf => ("file.export_view_dxf", Value::Object(self.options.clone())),
            Kind::UpdateLinks => ("component.update_links", Value::Object(self.options.clone())),
        }
    }
    fn answer(&self, r: Result<Value, String>) {
        if let Some(reply) = &self.reply {
            if crate::control::chat_turn_cancelled(self.chat_turn.as_deref()) {
                let _ = reply.send(crate::control::cancelled_chat_response());
                return;
            }
            let _ = reply.send(match r {
                Ok(v) => json!({"ok": true, "result": v}),
                Err(e) => json!({"ok": false, "error": e}),
            });
        }
    }
}

/// Route drawing files to the same commands as the sketch's Insert DXF and Insert SVG.
pub(crate) fn insert_command(path: &str) -> &'static str {
    solidcraft_engine::cmd::drawing_import_command(path)
        .unwrap_or_else(|| if solidcraft_engine::io::is_mesh_path(path) { "file.insert_mesh" } else { "file.insert_step" })
}

struct Made {
    session: Session,
    result: Value,
    /// Draw its Home picture once it is open (off the UI thread: a large design takes a while).
    thumbnail: bool,
}
type Job = Result<Made, String>;
struct Running {
    request: Request,
    base: Arc<Document>,
    base_revision: u64,
    before: Vec<String>,
    rx: Receiver<Job>,
    phase: Arc<AtomicU8>,
    progress: Arc<Progress>,
    preview_rx: Option<Receiver<egui::ColorImage>>,
    preview: Option<egui::TextureHandle>,
    /// Pictures of the model as far as it is built (the picture, features built).
    partial_rx: Option<Receiver<(egui::ColorImage, usize)>>,
    /// The picture shown is of the first this many features (`None`: the saved preview).
    built: Option<usize>,
    /// A STEP file read a piece per frame on this thread (the web app), instead of on a worker.
    staged: Option<Staged>,
}

impl Drop for Running {
    fn drop(&mut self) {
        if self.request.kind == Kind::ExportDxf {
            self.progress.cancel();
        }
    }
}

/// A STEP import done a piece per frame, for the web app: it has one thread, which must keep
/// drawing (and offer Cancel) while a large file comes in. The file is parsed in one frame,
/// then its solids are converted a few at a time, each frame for about [`STAGED_FRAME_MS`];
/// the import is then cached, and the command that inserts or opens the file finds it read.
/// A design is opened the same way, a few features a frame ([`step_opening`]).
struct Staged {
    tx: Sender<Job>,
    session: Option<Session>,
    command: &'static str,
    params: Value,
    reading: Option<solidcraft_engine::kernel::StepReading>,
    /// A design being opened: its features evaluated so far.
    opening: Option<solidcraft_engine::OpenSteps>,
    /// The feature of that design whose imported STEP file was read last (by `reading`), and
    /// the result, held until the feature has been evaluated, which finds the file read.
    read_for: Option<usize>,
    held: Option<Arc<solidcraft_engine::kernel::StepImport>>,
    /// The command's result once it has run; then the bodies' meshes are made, a few a frame,
    /// so the first frame that draws the design doesn't make them all at once.
    ran: Option<Value>,
    /// Bodies meshed so far, and their triangles.
    warmed: usize,
    triangles: usize,
    /// The overlay has been drawn once (the first frame does no work, so it shows at once).
    shown: bool,
}

/// Designs up to this many triangles get a thumbnail (a picture of more takes a while).
const STAGED_THUMBNAIL_TRIANGLES: usize = 1_000_000;

/// How long a frame spends on a staged STEP import.
const STAGED_FRAME_MS: f64 = 30.0;

/// Files read in stages: STEP (not IGES, FreeCAD or Rhino, which are first restated as STEP) to
/// insert or open, and designs to open (#10198).
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn staged_kind(request: &Request) -> bool {
    let p = &request.path;
    let step = matches!(request.kind, Kind::Insert | Kind::Open)
        && solidcraft_engine::io::is_step_path(p)
        && !solidcraft_engine::io::is_iges_path(p)
        && !solidcraft_engine::io::is_native_cad_path(p);
    step || (request.kind == Kind::Open && solidcraft_engine::io::is_design_path(p))
}

impl Running {
    fn matches(&self, session: &Session) -> bool {
        Arc::ptr_eq(&session.doc, &self.base) && session.revision == self.base_revision
    }
    fn cancelled(&self) -> bool {
        self.progress.is_cancelled() || crate::control::chat_turn_cancelled(self.request.chat_turn.as_deref())
    }
    /// Features rebuilt and the timeline's length: a design opened in stages counts its own.
    fn features(&self) -> (usize, usize) {
        self.staged.as_ref().and_then(|s| s.opening.as_ref()).map_or_else(|| self.progress.features(), |o| o.progress())
    }
    /// Long work inside a feature for the card. A design opened in stages has some only while
    /// it reads a feature's STEP file: the kernel's last count stays behind once the file is
    /// read, and would hide "Rebuilding feature N of M" until the next import (#10198).
    fn work(&self) -> Option<(&'static str, usize, usize)> {
        match &self.staged {
            Some(s) if s.opening.is_some() && s.reading.is_none() => None,
            _ => self.progress.work(),
        }
    }
}

#[derive(Default)]
pub struct FileLoad {
    queued: VecDeque<Request>,
    running: Option<Running>,
    /// Tests: the next load waits before this feature until cancelled.
    #[cfg(test)]
    pub hold_at: Option<usize>,
    /// Tests: report the worker thread, then wait for the window's explicit release.
    #[cfg(test)]
    pub worker_gate: Option<(Sender<std::thread::ThreadId>, Receiver<()>)>,
}

impl FileLoad {
    /// How many more requests can wait.
    pub(crate) fn room(&self) -> usize {
        crate::documents::MAX_OPEN.saturating_sub(self.queued.len())
    }
    /// A file operation is under way: the design waits for it.
    pub fn busy(&self) -> bool {
        self.running.as_ref().is_some_and(|r| !r.cancelled()) || !self.queued.is_empty()
    }
    /// The running load's progress: (features rebuilt, timeline length).
    pub fn features(&self) -> Option<(usize, usize)> {
        self.running.as_ref().map(Running::features)
    }
    /// No active or queued request; cancelled import workers may still be unwinding.
    pub fn idle(&self) -> bool {
        self.running.is_none() && self.queued.is_empty()
    }
    /// Signal cancellation, release the import slot and drop queued requests.
    pub fn cancel(&mut self) {
        if let Some(r) = &self.running {
            r.progress.cancel();
        }
        // Imports only change a scratch session. Disconnect their result and preview channels
        // so a slow cancellation checkpoint cannot block the next file or apply a late result.
        // Exports publish a file: keep their reply until the worker reports whether it committed.
        if self.running.as_ref().is_some_and(|r| r.request.kind != Kind::ExportDxf)
            && let Some(r) = self.running.take()
        {
            r.request.answer(Err("file operation cancelled".into()));
        }
        for q in self.queued.drain(..) {
            q.answer(Err("file operation cancelled".into()));
        }
    }
}

pub fn begin(app: &mut SolveApp, path: &str, insert: bool) {
    let _ = begin_request(app, Request::new(path, if insert { Kind::Insert } else { Kind::Open }));
}

/// Queue a file operation; on the web (no threads) it runs now.
pub fn begin_request(app: &mut SolveApp, request: Request) -> Result<(), String> {
    if crate::control::chat_turn_cancelled(request.chat_turn.as_deref()) {
        request.answer(Err("the agent chat turn ended".into()));
        return Ok(());
    }
    if app.file_load.idle() && reuse_open(app, &request) {
        return Ok(());
    }
    #[cfg(target_arch = "wasm32")]
    {
        if staged_kind(&request) {
            app.file_load.queued.push_back(request);
            start_staged(app);
            return Ok(());
        }
        let r = match &request.kind {
            Kind::Insert => {
                app.insert_path(&request.path);
                Ok(Value::Null)
            }
            Kind::Open => {
                app.open_path(&request.path);
                // Opened in place of the blank tab it made, with no switch to it.
                crate::edit_in_place::attach(app);
                Ok(Value::Null)
            }
            Kind::Recover { .. } => {
                let (command, params) = request.command();
                let r = app.run(command, params);
                if r.is_ok() {
                    recovered(app, &request);
                }
                r
            }
            Kind::ExportDxf => {
                let (command, params) = request.command();
                let r = app.run(command, params);
                if r.is_ok() && !request.path.is_empty() {
                    app.set_status(crate::i18n::format("ui.saved_to", &[("path", &request.path)]), false);
                }
                r
            }
            Kind::UpdateLinks => {
                let (command, params) = request.command();
                solidcraft_engine::io::vfs::allowing_hosts(&request.allow_hosts, || app.run(command, params))
            }
        };
        request.answer(r);
        Ok(())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // A design open in another SolidCraft asks first (#5467).
        let Some(request) = crate::open_elsewhere::check_open(app, request) else { return Ok(()) };
        queue(app, request)
    }
}

/// Queue a file operation and start it when nothing runs (the desktop app).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn queue(app: &mut SolveApp, request: Request) -> Result<(), String> {
    if app.file_load.queued.len() >= crate::documents::MAX_OPEN {
        let e = "too many queued files".to_string();
        app.set_status(e.clone(), true);
        request.answer(Err(e.clone()));
        return Err(e);
    }
    app.file_load.queued.push_back(request);
    start(app);
    Ok(())
}

/// Answer each ordinary open separately, including queued requests for the same file.
/// Validate the caller's scope before looking at existing sessions or resolving paths.
fn reuse_open(app: &mut SolveApp, request: &Request) -> bool {
    if request.kind != Kind::Open {
        return false;
    }
    let reused = solidcraft_engine::io::vfs::scoped(request.scope.clone(), || {
        solidcraft_engine::io::vfs::check(&request.path).map_err(|e| e.to_string())?;
        // Let the command validate import-specific options, even for an existing tab.
        if !request.options.is_empty() {
            return Ok(false);
        }
        crate::agent_chat::with_tool_request(app, request.chat_turn.as_ref(), |app| crate::documents::reuse_path(app, &request.path))
    });
    match reused {
        Ok(false) => false,
        Ok(true) => {
            let s = &app.session;
            request.answer(Ok(json!({"path": request.path, "features": s.doc.features.len(),
                "errors": s.model.results.iter().filter(|r| r.error.is_some()).count(), "name": s.doc.name})));
            true
        }
        Err(e) => {
            app.set_status(e.clone(), true);
            request.answer(Err(e));
            true
        }
    }
}

/// A control-channel (MCP) `doc.open` or STEP/mesh import runs like File > Open: off the window
/// thread, answered when the file is in. Other requests come back to be handled now.
pub fn control(app: &mut SolveApp, req: crate::control::ControlRequest) -> Option<crate::control::ControlRequest> {
    if cfg!(target_arch = "wasm32") || !matches!(req.method.as_str(), "engine.execute" | "command") {
        return Some(req);
    }
    let command = req.params.get("command").or(req.params.get("id")).and_then(Value::as_str);
    let kind = match command {
        Some("doc.open") => Kind::Open,
        Some("file.insert_step" | "file.insert_mesh" | "pointcloud.insert") => Kind::Insert,
        Some("file.export_view_dxf") => Kind::ExportDxf,
        _ => return Some(req),
    };
    let params = req.params.get("params");
    let path = match params.and_then(|p| p.get("path")).and_then(Value::as_str) {
        Some(path) => path,
        None if kind == Kind::ExportDxf => "",
        None => return Some(req),
    };
    let scope = match crate::control::request_scope(app, &req.params) {
        Ok(scope) => scope,
        Err(e) => {
            let _ = req.reply.send(json!({"ok": false, "error": e}));
            return None;
        }
    };
    // Everything but the path, as given: taking only `up` and `names` dropped an import's
    // `matrix` and `name`, so it landed at the origin with nothing said (#4676).
    let mut options = params.and_then(Value::as_object).cloned().unwrap_or_default();
    if kind != Kind::ExportDxf {
        options.remove("path");
        options.retain(|_, v| !v.is_null());
    }
    let request = Request {
        path: path.into(),
        kind,
        reply: Some(req.reply.clone()),
        scope,
        options,
        allow_hosts: Vec::new(),
        design: None,
        chat_turn: req.chat_turn,
    };
    let _ = begin_request(app, request);
    None
}

/// An autosave that failed to load goes back in the recovery list, so it can be discarded (or
/// left for later) instead of coming back unasked.
fn reoffer(app: &mut SolveApp, id: &str, dir: Option<&str>) {
    let dir = dir.map(std::path::PathBuf::from).or_else(|| app.autosave.as_ref().map(|a| a.dir.clone()));
    let Some(dir) = dir else { return };
    if app.recoverable.iter().any(|e| e.id == id) {
        return;
    }
    if let Some(e) = solidcraft_engine::recovery::orphans(&dir).into_iter().find(|e| e.id == id) {
        app.recoverable.insert(0, e);
    }
}

/// A recovered design is ours now: autosave it under this app's entry before the old one goes.
fn recovered(app: &mut SolveApp, request: &Request) {
    let Kind::Recover { id, dir } = &request.kind else { return };
    let kept = app.autosave.as_mut().map(|a| a.save(&app.session));
    if let Some(Ok(_)) = kept {
        let _ = app.session.execute("doc.recovery_discard", &json!({"id": id, "dir": dir}));
    }
    app.set_status(format!("recovered {}: save it to keep it", request.path), false);
}

#[cfg(not(target_arch = "wasm32"))]
fn start(app: &mut SolveApp) {
    if app.file_load.running.is_some() {
        return;
    }
    let Some(request) = app.file_load.queued.pop_front() else { return };
    if crate::control::chat_turn_cancelled(request.chat_turn.as_deref()) {
        request.answer(Err("the agent chat turn ended".into()));
        return;
    }
    if reuse_open(app, &request) {
        return;
    }
    if request.design.as_deref().is_some_and(|d| !app.session.path.as_deref().is_some_and(|p| crate::documents::same_path(p, d))) {
        request.answer(Err("the design it was for is no longer shown".into()));
        return;
    }
    if matches!(request.kind, Kind::Open | Kind::Recover { .. })
        && app.docs.count() >= crate::documents::MAX_OPEN
        && !crate::documents::active_is_blank(app)
    {
        let e = "too many open designs; close a tab before opening another";
        app.set_status(e, true);
        request.answer(Err(e.into()));
        return;
    }
    let insert = request.insert();
    let before = if insert { app.session.world_state().bodies.iter().map(|b| b.name.clone()).collect() } else { Vec::new() };
    let update = request.kind == Kind::UpdateLinks;
    let mut scratch = if insert || update || request.kind == Kind::ExportDxf { app.session.scratch() } else { Session::default() };
    if insert {
        scratch.selection = app.session.selection.clone();
    }
    if update {
        // Link paths are kept relative to the design's own file.
        scratch.path.clone_from(&app.session.path);
    }
    let base = app.session.doc.clone();
    let base_revision = app.session.revision;
    let phase = Arc::new(AtomicU8::new(0));
    let worker_phase = phase.clone();
    let progress = Arc::new(Progress::default());
    #[cfg(test)]
    if let Some(i) = app.file_load.hold_at.take() {
        progress.hold_at(i);
    }
    let worker_progress = progress.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let (preview_tx, preview_rx) = std::sync::mpsc::channel();
    let (command, params) = request.command();
    let scope = request.scope.clone();
    let allow_hosts = request.allow_hosts.clone();
    let thumbnail = request.kind == Kind::Open;
    // While a design rebuilds, a picture of what is built so far (a large design takes a while).
    let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let partial_rx = (command == "doc.open" || command == "doc.recover").then(|| {
        progress.want_partial();
        let (partial_tx, partial_rx) = std::sync::mpsc::channel();
        let (watch, finished) = (progress.clone(), finished.clone());
        let _ = std::thread::Builder::new().name("file-load-picture".into()).spawn(move || {
            let stop = || finished.load(Ordering::Relaxed) || watch.is_cancelled();
            while !stop() {
                std::thread::sleep(std::time::Duration::from_millis(250));
                let Some(partial) = watch.take_partial() else { continue };
                let began = std::time::Instant::now();
                let picture = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| partial_picture(&partial.world, &stop))).ok().flatten();
                if solidcraft_engine::geom::env::var_os("PROFILE_OPEN").is_some() {
                    eprintln!(
                        "SolidCraft partial picture: features={} bodies={} drawn={} ms={:.1}",
                        partial.features,
                        partial.world.bodies.len(),
                        picture.is_some(),
                        began.elapsed().as_secs_f64() * 1000.0
                    );
                }
                if let Some(picture) = picture.filter(|_| !stop()) {
                    let _ = partial_tx.send((picture, partial.features));
                }
            }
        });
        partial_rx
    });
    let worker_finished = finished;
    #[cfg(test)]
    let worker_gate = app.file_load.worker_gate.take();
    let spawned = std::thread::Builder::new().name("file-load".into()).spawn(move || {
        #[cfg(test)]
        if let Some((started, release)) = worker_gate {
            let _ = started.send(std::thread::current().id());
            if release.recv_timeout(std::time::Duration::from_secs(60)).is_err() {
                worker_finished.store(true, Ordering::Relaxed);
                let _ = tx.send(Err("test file worker was not released".into()));
                return;
            }
        }
        let preview_began = std::time::Instant::now();
        if command == "doc.open" {
            let progress = worker_progress.clone();
            scratch.open_preview = Some(Box::new(move |preview| {
                if !progress.is_cancelled() {
                    // Render on the file worker, never the window thread.
                    let picture = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| saved_picture(preview))).ok().flatten();
                    if let Some(picture) = picture.filter(|_| !progress.is_cancelled()) {
                        let _ = preview_tx.send(picture);
                        if solidcraft_engine::geom::env::var_os("PROFILE_OPEN").is_some() {
                            eprintln!("SolidCraft saved preview: ready_ms={:.3}", preview_began.elapsed().as_secs_f64() * 1000.0);
                        }
                    }
                }
            }));
        }
        // Servers allowed once hold for this request only, on this worker thread.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            solidcraft_engine::io::vfs::allowing_hosts(&allow_hosts, || {
                compute(scratch, command, &params, scope, thumbnail, &worker_phase, &worker_progress)
            })
        }))
        .unwrap_or_else(|_| Err("the file operation failed; the current design is unchanged".into()));
        worker_finished.store(true, Ordering::Relaxed);
        let _ = tx.send(result);
    });
    match spawned {
        Ok(_) => {
            app.file_load.running = Some(Running {
                request,
                base,
                base_revision,
                before,
                rx,
                phase,
                progress,
                preview_rx: Some(preview_rx),
                preview: None,
                partial_rx,
                built: None,
                staged: None,
            })
        }
        Err(e) => {
            let e = format!("could not start file operation: {e}");
            request.answer(Err(e.clone()));
            app.set_status(e, true);
        }
    }
}

/// Start the next queued STEP file as a [`Staged`] import (see [`step_staged`]).
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn start_staged(app: &mut SolveApp) {
    if app.file_load.running.is_some() {
        return;
    }
    let Some(request) = app.file_load.queued.pop_front() else { return };
    let insert = request.insert();
    let before = if insert { app.session.world_state().bodies.iter().map(|b| b.name.clone()).collect() } else { Vec::new() };
    let mut session = if insert { app.session.scratch() } else { Session::default() };
    if insert {
        session.selection = app.session.selection.clone();
    }
    let (command, params) = request.command();
    let (tx, rx) = std::sync::mpsc::channel();
    let progress = Arc::new(Progress::default());
    #[cfg(test)]
    if let Some(i) = app.file_load.hold_at.take() {
        progress.hold_at(i);
    }
    app.file_load.running = Some(Running {
        base: app.session.doc.clone(),
        base_revision: app.session.revision,
        before,
        rx,
        phase: Arc::new(AtomicU8::new(0)),
        progress,
        preview_rx: None,
        preview: None,
        partial_rx: None,
        built: None,
        staged: Some(Staged {
            tx,
            session: Some(session),
            command,
            params,
            reading: None,
            opening: None,
            read_for: None,
            held: None,
            ran: None,
            warmed: 0,
            triangles: 0,
            shown: false,
        }),
        request,
    });
}

/// One frame of a staged STEP import: parse the file, or convert solids for a frame's worth of
/// time, or, once every solid is in, run the command (which finds the file read) and hand its
/// result to [`tick`] as a worker would.
fn step_staged(running: &mut Running) {
    let Some(staged) = running.staged.as_mut() else { return };
    let progress = running.progress.clone();
    let scope = running.request.scope.clone();
    let path = running.request.path.clone();
    let send = |staged: &mut Staged, job: Job| {
        let _ = staged.tx.send(job);
        staged.session = None;
        staged.reading = None;
        staged.opening = None;
        staged.held = None;
    };
    if staged.session.is_none() {
        return;
    }
    if progress.is_cancelled() {
        send(staged, Err("file operation cancelled".into()));
        return;
    }
    if !staged.shown {
        staged.shown = true;
        return;
    }
    let began = web_time::Instant::now();
    let in_budget = || began.elapsed().as_secs_f64() * 1000.0 < STAGED_FRAME_MS;
    if staged.ran.is_some() {
        let Some(session) = staged.session.as_ref() else { return };
        let world = session.world_state();
        while staged.warmed < world.bodies.len() && in_budget() {
            if let Some(body) = world.bodies.get(staged.warmed) {
                staged.triangles += body.mesh().triangles.len();
            }
            staged.warmed += 1;
        }
        if staged.warmed < world.bodies.len() {
            return;
        }
        let (Some(session), Some(result)) = (staged.session.take(), staged.ran.take()) else { return };
        let thumbnail = staged.command == "doc.open" && staged.triangles <= STAGED_THUMBNAIL_TRIANGLES;
        send(staged, Ok(Made { session, result, thumbnail }));
        return;
    }
    if staged.command == "doc.open" && solidcraft_engine::io::is_design_path(&path) {
        match step_opening(staged, &progress, scope, &in_budget) {
            None => {}
            Some(Ok(result)) => {
                // Next frames: the meshes ("Preparing the view").
                running.phase.store(1, Ordering::Relaxed);
                staged.ran = Some(result);
            }
            Some(Err(e)) => send(staged, Err(e)),
        }
        return;
    }
    let Some(reading) = staged.reading.as_mut() else {
        // Parse and walk the file. The reading keeps the watch: its steps report progress and
        // stop on Cancel.
        let read = solidcraft_engine::io::vfs::scoped(scope, || {
            let bytes = solidcraft_engine::io::limits::read(&path, solidcraft_engine::io::limits::Kind::Step).map_err(|e| format!("{path}: {e}"))?;
            // The same text the import command makes of the bytes, so it finds this reading.
            let text = String::from_utf8_lossy(&bytes).into_owned();
            solidcraft_engine::doc::progress::watch(progress.clone(), || solidcraft_engine::kernel::StepReading::new(&text))
                .map_err(|e| format!("{path}: {e}"))
        });
        match read {
            Ok(reading) => staged.reading = Some(reading),
            Err(e) => send(staged, Err(e)),
        }
        return;
    };
    while in_budget() {
        if !reading.step() {
            break;
        }
    }
    let (done, total) = reading.progress();
    if done < total && !progress.is_cancelled() {
        return;
    }
    let Some(reading) = staged.reading.take() else { return };
    // Held until the command has read the file from the cache.
    let import = reading.finish_shared();
    let import = match import {
        _ if progress.is_cancelled() => Err("file operation cancelled".to_string()),
        Ok(import) => Ok(import),
        Err(e) => Err(format!("{path}: {e}")),
    };
    let (Some(mut session), Ok(import)) = (staged.session.take(), import.clone()) else {
        send(staged, Err(import.err().unwrap_or_else(|| "file operation cancelled".into())));
        return;
    };
    let (command, params) = (staged.command, staged.params.clone());
    let result = solidcraft_engine::io::vfs::scoped(running.request.scope.clone(), || {
        solidcraft_engine::doc::progress::watch(progress.clone(), || session.execute(command, &params))
    });
    drop(import);
    match result {
        Ok(result) => {
            // Next frames: the meshes ("Preparing the view").
            running.phase.store(1, Ordering::Relaxed);
            staged.session = Some(session);
            staged.ran = Some(result);
        }
        Err(e) => send(staged, Err(e.to_string())),
    }
}

/// A frame of a design opened in stages (#10198): read the file, then evaluate its features
/// for a frame's worth of time, reading the STEP file a feature imports a piece per frame first,
/// as a staged STEP import does. Once every feature is in, the design takes the scratch
/// session's place as `doc.open` would leave it, and its answer is returned; `None` until then.
fn step_opening(
    staged: &mut Staged,
    progress: &Arc<Progress>,
    scope: Option<solidcraft_engine::io::vfs::PathScope>,
    in_budget: &dyn Fn() -> bool,
) -> Option<Result<Value, String>> {
    use solidcraft_engine::doc::progress::watch;
    use solidcraft_engine::io::vfs::scoped;
    let session = staged.session.as_mut()?;
    let Some(opening) = staged.opening.as_mut() else {
        let params = staged.params.clone();
        match scoped(scope.clone(), || watch(progress.clone(), || solidcraft_engine::OpenSteps::start(&params))) {
            Ok(Some(opening)) => staged.opening = Some(opening),
            // Options only `doc.open` takes: it opens the file at once.
            Ok(None) => {
                return Some(scoped(scope, || watch(progress.clone(), || session.execute(staged.command, &params))).map_err(|e| e.to_string()));
            }
            Err(e) => return Some(Err(e.to_string())),
        }
        return None;
    };
    while in_budget() {
        if progress.is_cancelled() {
            return Some(Err("file operation cancelled".into()));
        }
        if let Some(reading) = staged.reading.as_mut() {
            if !reading.step() {
                // A file that cannot be read fails in its feature, which says why.
                staged.held = staged.reading.take().and_then(|r| r.finish_shared().ok());
            }
            continue;
        }
        let next = opening.progress().0;
        if staged.read_for != Some(next)
            && let Some(text) = opening.next_import()
        {
            staged.read_for = Some(next);
            staged.reading = watch(progress.clone(), || solidcraft_engine::kernel::StepReading::new(&text)).ok();
            continue;
        }
        let more = scoped(scope.clone(), || watch(progress.clone(), || opening.step()));
        staged.held = None;
        if !more {
            let opening = staged.opening.take()?;
            return Some(scoped(scope, || watch(progress.clone(), || opening.finish(session))).map_err(|e| e.to_string()));
        }
    }
    None
}

/// A small picture of the bodies built so far (their display meshes, made here and kept for
/// the finished model), or `None` when there is nothing to show yet, too much to draw, or the
/// load ended meanwhile (`stop`).
#[cfg(not(target_arch = "wasm32"))]
fn partial_picture(world: &solidcraft_engine::doc::ModelState, stop: &dyn Fn() -> bool) -> Option<egui::ColorImage> {
    use solidcraft_engine::{
        geom::Aabb3,
        render::{Camera, Rgb, Scene, SceneMesh, StandardView},
    };
    let mut scene = Scene::default();
    let mut bounds = Aabb3::EMPTY;
    let mut triangles = 0usize;
    for body in &world.bodies {
        if stop() {
            return None;
        }
        let mesh = body.mesh();
        triangles += mesh.triangles.len();
        if triangles > 4_000_000 {
            return None;
        }
        bounds = bounds.union(&mesh.bounds());
        scene.meshes.push(SceneMesh::solid(mesh, Rgb(176, 186, 199)));
    }
    if bounds.is_empty() {
        return None;
    }
    let mut camera = Camera::default();
    camera.set_view(StandardView::Iso);
    camera.fit(&bounds);
    scene.radius = bounds.diagonal();
    if stop() || !preview_work_fits(&scene, &camera) {
        return None;
    }
    preview_image(&scene, &camera)
}

#[cfg(not(target_arch = "wasm32"))]
fn saved_picture(preview: solidcraft_engine::io::DesignPreview) -> Option<egui::ColorImage> {
    use solidcraft_engine::{
        geom::{Aabb3, Mesh, Vec3},
        render::{Camera, Rgb, Scene, SceneMesh, StandardView},
    };
    let mut scene = Scene::default();
    let mut bounds = Aabb3::EMPTY;
    for cached in preview.meshes {
        let mut mesh = Mesh {
            positions: cached.positions.as_chunks::<3>().0.iter().map(|p| Vec3::new(p[0], p[1], p[2])).collect(),
            triangles: cached.triangles.as_chunks::<3>().0.to_vec(),
            ..Default::default()
        };
        mesh.normals.resize(mesh.positions.len(), Vec3::ZERO);
        for triangle in &mesh.triangles {
            let a = *mesh.positions.get(*triangle.first()? as usize)?;
            let b = *mesh.positions.get(*triangle.get(1)? as usize)?;
            let c = *mesh.positions.get(*triangle.get(2)? as usize)?;
            let normal = (b - a).cross(c - a).normalized().unwrap_or(Vec3::Z);
            for index in triangle {
                if let Some(n) = mesh.normals.get_mut(*index as usize) {
                    *n += normal;
                }
            }
        }
        for normal in &mut mesh.normals {
            *normal = normal.normalized().unwrap_or(Vec3::Z);
        }
        bounds = bounds.union(&mesh.bounds());
        let [r, g, b] = cached.color.unwrap_or([0.69, 0.73, 0.78]).map(|c| (c * 255.0).round() as u8);
        scene.meshes.push(SceneMesh::solid(Arc::new(mesh), Rgb(r, g, b)));
    }
    if bounds.is_empty() {
        return None;
    }
    let mut camera = Camera::default();
    camera.set_view(StandardView::Iso);
    camera.fit(&bounds);
    scene.radius = bounds.diagonal();
    if !preview_work_fits(&scene, &camera) {
        return None;
    }
    preview_image(&scene, &camera)
}

/// The preview picture, drawn on the GPU by the renderer the app installs (none: no picture).
#[cfg(not(target_arch = "wasm32"))]
fn preview_image(scene: &solidcraft_engine::render::Scene, camera: &solidcraft_engine::render::Camera) -> Option<egui::ColorImage> {
    match solidcraft_engine::capture::rgba(scene, camera, 320, 200) {
        Ok(picture) => Some(egui::ColorImage::from_rgba_unmultiplied([320, 200], &picture.rgba)),
        Err(e) => {
            log::warn!("no preview picture: {e}");
            None
        }
    }
}

/// Bound drawing work as well as file size: repeated full-screen triangles (a hostile file) must
/// not monopolize the file worker or a software GPU. Counts each triangle's pixel box at twice the
/// picture's size.
#[cfg(not(target_arch = "wasm32"))]
fn preview_work_fits(scene: &solidcraft_engine::render::Scene, camera: &solidcraft_engine::render::Camera) -> bool {
    let mut pixels = 0.0;
    let projection = camera.view_proj(640.0 / 400.0, scene.radius.max(1.0));
    for body in &scene.meshes {
        let points: Vec<_> = body
            .mesh
            .positions
            .iter()
            .map(|p| {
                let q = projection.apply(*p);
                (q[3] > 1e-9).then(|| ((q[0] / q[3] + 1.0) * 320.0, (1.0 - q[1] / q[3]) * 200.0, q[2] / q[3]))
            })
            .collect();
        for triangle in &body.mesh.triangles {
            let mut lo = [f64::INFINITY; 2];
            let mut hi = [f64::NEG_INFINITY; 2];
            let mut visible = true;
            for index in triangle {
                if let Some(Some((x, y, _))) = points.get(*index as usize) {
                    lo = [lo[0].min(*x), lo[1].min(*y)];
                    hi = [hi[0].max(*x), hi[1].max(*y)];
                } else {
                    visible = false;
                    break;
                }
            }
            if visible {
                let width = (hi[0].ceil().min(639.0) - lo[0].floor().max(0.0) + 1.0).max(0.0);
                let height = (hi[1].ceil().min(399.0) - lo[1].floor().max(0.0) + 1.0).max(0.0);
                pixels += width * height;
                if !pixels.is_finite() || pixels > 16_000_000.0 {
                    return false;
                }
            }
        }
    }
    true
}

#[cfg(not(target_arch = "wasm32"))]
fn compute(
    mut session: Session,
    command: &str,
    params: &Value,
    scope: Option<solidcraft_engine::io::vfs::PathScope>,
    thumbnail: bool,
    phase: &AtomicU8,
    progress: &Arc<Progress>,
) -> Job {
    let profile = solidcraft_engine::geom::env::var_os("PROFILE_OPEN").is_some();
    let began = std::time::Instant::now();
    let cancelled = || Err("file operation cancelled".to_string());
    if progress.is_cancelled() {
        return cancelled();
    }
    // The timeline's rebuild reports each feature and stops at the next one after Cancel. An
    // agent's scope is per thread, so it is set again here.
    let result =
        solidcraft_engine::io::vfs::scoped(scope, || solidcraft_engine::doc::progress::watch(progress.clone(), || session.execute(command, params)))
            .map_err(|e| e.to_string())?;
    if command == "file.export_view_dxf" {
        // A successful export has already published its complete file. A Cancel arriving
        // after that commit must not turn a completed save into a misleading cancellation.
        return Ok(Made { session, result, thumbnail: false });
    }
    if progress.is_cancelled() {
        return cancelled();
    }
    let geometry_ms = began.elapsed().as_secs_f64() * 1000.0;
    let view_began = std::time::Instant::now();
    phase.store(1, Ordering::Relaxed);
    let world = session.world_state();
    let placement_ms = view_began.elapsed().as_secs_f64() * 1000.0;
    // The display meshes are made once the design is in the window, which shows its bodies as
    // they come ([`crate::open_meshing`]): framing, picking and drawing never mesh on the UI.
    // Its outlines frame it in the window until it is meshed: measured here, not there.
    solidcraft_engine::kernel::for_each_on_threads(&world.bodies, 16, |b| {
        let _ = b.outline_bounds();
    });
    drop(world);
    let tessellation_ms = 0.0;
    phase.store(2, Ordering::Relaxed);
    let view_ms = view_began.elapsed().as_secs_f64() * 1000.0;
    if profile {
        eprintln!(
            "SolidCraft open phases: geometry_ms={geometry_ms:.3} placement_ms={placement_ms:.3} tessellation_ms={tessellation_ms:.3} display_warmup_ms={view_ms:.3} total_ms={:.3}",
            began.elapsed().as_secs_f64() * 1000.0
        );
    }
    Ok(Made { session, result, thumbnail })
}

pub fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    // Stop must release the waiting slot even if the worker has not reached its next check.
    let chat_cancelled = app.file_load.running.as_ref().is_some_and(|r| crate::control::chat_turn_cancelled(r.request.chat_turn.as_deref()));
    if chat_cancelled && let Some(running) = &app.file_load.running {
        running.progress.cancel();
    }
    // As with FileLoad::cancel, a publishing export retains its worker until it reports back.
    if chat_cancelled
        && app.file_load.running.as_ref().is_some_and(|r| r.request.kind != Kind::ExportDxf)
        && let Some(running) = app.file_load.running.take()
    {
        running.request.answer(Err("the agent chat turn ended".into()));
    }
    if let Some(running) = &mut app.file_load.running {
        step_staged(running);
    }
    if let Some(running) = &mut app.file_load.running {
        if !running.cancelled() && running.matches(&app.session) {
            if let Some(image) = running.preview_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
                running.preview = Some(ctx.load_texture("opening-preview", image, egui::TextureOptions::LINEAR));
                running.built = None;
            }
            // The model as far as it is built, unless the file's saved picture (all of it) is up.
            if let Some((image, built)) = running.partial_rx.as_ref().and_then(|rx| rx.try_iter().last())
                && (running.preview.is_none() || running.built.is_some())
            {
                running.preview = Some(ctx.load_texture("opening-preview", image, egui::TextureOptions::LINEAR));
                running.built = Some(built);
            }
        } else {
            running.preview = None;
        }
    }
    let result = app.file_load.running.as_ref().and_then(|r| match r.rx.try_recv() {
        Ok(job) => Some(job),
        Err(TryRecvError::Empty) => None,
        Err(TryRecvError::Disconnected) => Some(Err("the file operation stopped; the current design is unchanged".into())),
    });
    if let Some(job) = result
        && let Some(running) = app.file_load.running.take()
    {
        let request = &running.request;
        match job {
            Ok(made) if request.kind == Kind::ExportDxf => {
                if !request.path.is_empty() {
                    app.set_status(crate::i18n::format("ui.saved_to", &[("path", &request.path)]), false);
                }
                request.answer(Ok(made.result));
            }
            _ if running.cancelled() => request.answer(Err("file operation cancelled".into())),
            Ok(made) if request.kind == Kind::UpdateLinks && running.matches(&app.session) => {
                let (id, _) = request.command();
                app.session.accept_computed(made.session);
                app.big_operation = true;
                crate::linked_source::read_again(app, &request.options, &made.result);
                let r = Ok(made.result);
                app.finish_command(id, &r);
                let language = crate::i18n::Locale::for_language(&app.preferences.language);
                app.set_status(language.format("ui.command_done", &[("command", language.label(crate::command_label(id)))]), false);
                // A design it could not read is said as when that was all it was asked.
                if let Some(f) = r.as_ref().ok().and_then(|v| v["failed"].as_array()).and_then(|f| f.first()) {
                    let (name, why) = (f["name"].as_str().unwrap_or_default(), f["error"].as_str().unwrap_or_default());
                    app.set_status(format!("{name}: {why}"), true);
                }
                request.answer(r);
            }
            Ok(_) if reuse_open(app, request) => {}
            Ok(made) if running.matches(&app.session) => {
                // Not meshed yet: framing it while it goes in must not mesh it here.
                app.open_meshing_next = true;
                crate::frame_watch::step(app, "before the open");
                if request.insert() {
                    app.session.accept_computed(made.session);
                    app.big_operation = true;
                } else {
                    crate::agent_chat::with_tool_request(app, request.chat_turn.as_ref(), |app| crate::documents::adopt(app, made.session));
                    if request.kind == Kind::Open {
                        app.home.add_recent(&request.path);
                    }
                }
                app.home.open = false;
                crate::frame_watch::step(app, "adopt the design");
                // Shown as its bodies are meshed, framed from their outlines meanwhile; its Home
                // picture is drawn once they all are.
                crate::open_meshing::start(app, (request.kind == Kind::Open && made.thumbnail).then(|| request.path.clone()));
                crate::frame_watch::step(app, "start meshing and frame");
                // A caller that said which way is up has been answered: no offer to stand the part up.
                if (request.kind == Kind::Open || request.insert()) && !request.options.contains_key("up") {
                    crate::stand_up::offer(app, &request.path, &running.before, Some(&made.result));
                }
                crate::frame_watch::step(app, "stand-up offer");
                let warnings = made.result.get("warnings").and_then(Value::as_array);
                if let Kind::Recover { .. } = request.kind {
                    recovered(app, request);
                } else {
                    app.set_status(
                        if made.result.get("read_only").and_then(Value::as_bool) == Some(true) {
                            crate::i18n::tr("document.opened_read_only").to_string()
                        } else if let Some(w) = warnings.filter(|w| !w.is_empty()) {
                            format!("imported with {} warning(s): {}", w.len(), w.first().and_then(Value::as_str).unwrap_or(""))
                        } else {
                            format!("{} done", if request.insert() { "Insert" } else { "Open" })
                        },
                        false,
                    );
                }
                request.answer(Ok(made.result));
            }
            Ok(_) => {
                let e = "the active design changed while loading; the file operation was discarded";
                request.answer(Err(e.into()));
                app.set_status(e, true);
            }
            Err(e) => {
                request.answer(Err(e.clone()));
                if request.kind == Kind::UpdateLinks {
                    // Said as when the command ran here (the status line, the run's messages).
                    app.finish_command(request.command().0, &Err(e));
                } else if let Kind::Recover { id, dir } = &request.kind {
                    reoffer(app, id, dir.as_deref());
                    app.set_status(format!("could not recover {}: {e}. Discard it or choose Later", request.path), true);
                } else {
                    app.set_status(e, true);
                }
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    start(app);
    #[cfg(target_arch = "wasm32")]
    start_staged(app);
    if app.file_load.busy() {
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}

/// The light overlay over everything under the title bar while a file loads. The title bar,
/// its window buttons and the window's resize edges stay live; the design underneath takes no
/// pointer input until the file is in.
/// What a file operation under way is doing (for the frame watch), if one is.
pub(crate) fn phase(app: &SolveApp) -> Option<&'static str> {
    let r = app.file_load.running.as_ref()?;
    Some(match (r.staged.is_some(), r.phase.load(Ordering::Relaxed)) {
        (true, 0) => "reading the file (staged)",
        (_, 0) => "reading the file",
        (_, 1) => "preparing the view",
        _ => "finishing the open",
    })
}

pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(r) = &app.file_load.running else { return };
    let cancelled = r.cancelled();
    if cancelled && app.file_load.queued.is_empty() {
        return;
    }
    let name = match &r.request.kind {
        Kind::Recover { .. } => r.request.path.clone(),
        // The linked design being read again, once the worker is on one.
        Kind::UpdateLinks => match r.progress.item() {
            Some((name, i, n)) => {
                crate::i18n::format("file_load.link_item", &[("name", &name), ("done", &(i + 1).min(n).to_string()), ("total", &n.to_string())])
            }
            None => app.session.doc.name.clone(),
        },
        _ => std::path::Path::new(&r.request.path).file_name().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
    };
    let phase = r.phase.load(Ordering::Relaxed);
    let (done, total) = r.features();
    // Long work inside one feature (a large STEP file) has progress of its own.
    let export = r.request.kind == Kind::ExportDxf;
    let work = r.work().filter(|(_, d, t)| export || (phase == 0 && d < t));
    let screen = ctx.content_rect();
    // Leave the title bar to the window. The resize edges are a foreground layer above this.
    let cover = egui::Rect::from_min_max(egui::pos2(screen.left(), screen.top() + crate::toolbar::APP_BAR_HEIGHT), screen.max);
    if cover.width() <= 0.0 || cover.height() <= 0.0 {
        return;
    }
    let centre = app.viewport.rect.filter(|v| cover.contains_rect(*v)).unwrap_or(cover).center();
    // Typing goes nowhere until the design is in.
    ctx.memory_mut(|m| {
        if let Some(id) = m.focused() {
            m.surrender_focus(id);
        }
    });
    let id = egui::Id::new("file_loading");
    let card_id = id.with("card");
    // Above the dialogs over the design; menus, tooltips and the resize edges stay above it.
    ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, id));
    ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, card_id));
    Container::new(ctx, Surface::InputShield).area(id).order(egui::Order::Middle).fixed_pos(cover.min).interactable(true).show(ctx, |ui| {
        ui.set_min_size(cover.size());
        ui.interact(cover, egui::Id::new("file_loading_block"), egui::Sense::click_and_drag());
        ui.painter().rect_filled(cover, 0.0, egui::Color32::from_black_alpha(70));
    });
    // Anchored at its top: the card grows downwards (the saved preview) and Cancel stays put.
    let top = egui::pos2(centre.x, (centre.y - 120.0).max(cover.top() + 8.0));
    let title = if cancelled && export {
        crate::i18n::tr_label("dxf.cancelling")
    } else if cancelled {
        crate::i18n::tr("file_load.waiting")
    } else {
        crate::i18n::tr(match r.request.kind {
            Kind::Insert => "file_load.inserting",
            Kind::Open => "file_load.opening",
            Kind::Recover { .. } => "file_load.recovering",
            Kind::ExportDxf => "dxf.exporting",
            Kind::UpdateLinks => "file_load.updating_links",
        })
    };
    let icon = match r.request.kind {
        Kind::Insert => "import",
        Kind::Open => "open",
        Kind::Recover { .. } => "restore",
        Kind::ExportDxf => "export",
        Kind::UpdateLinks => "update",
    };
    let (closed, action) = ui_kit::Window::new(card_id, icon, title)
        .cards(((cover.width() - 16.0) / crate::theme::Tokens::get().form_card_width).clamp(0.5, 1.3))
        .placed(egui::Align2::CENTER_TOP, top)
        .actions("", crate::i18n::tr("ui.cancel"), None)
        .show(ctx, true, |ui| {
            ui_kit::text(ui, &format!("{name}…"), ui_kit::Text::Report("name", false));
            if let Some(preview) = r.preview.as_ref().filter(|_| !cancelled) {
                let w = ui.available_width();
                ui.image((preview.id(), egui::vec2(w, w * 200.0 / 320.0)));
                let note = match r.built {
                    Some(built) => crate::i18n::format("file_load.built", &[("built", &built.to_string()), ("total", &total.to_string())]),
                    None => crate::i18n::tr("file_load.preview_note").to_string(),
                };
                ui_kit::text(ui, &note, ui_kit::Text::Note("preview"));
            }
            let status = if cancelled && export {
                crate::i18n::tr_label("dxf.cancel_wait").to_string()
            } else if cancelled {
                crate::i18n::tr("file_load.stopping").to_string()
            } else {
                match (phase, work) {
                    (_, work) if r.request.kind == Kind::ExportDxf => crate::dialogs_view_dxf::progress_text(work),
                    (_, Some(("reading", d, t))) => {
                        crate::i18n::format("file_load.reading_percent", &[("percent", &(d * 100 / t.max(1)).to_string())])
                    }
                    (_, Some((_, d, t))) => crate::i18n::format("file_load.converting", &[("done", &(d + 1).to_string()), ("total", &t.to_string())]),
                    (1, _) => crate::i18n::tr("file_load.preparing_view").to_string(),
                    (2, _) => crate::i18n::tr("file_load.preparing_thumbnail").to_string(),
                    _ if total > 0 => {
                        crate::i18n::format("file_load.rebuilding", &[("done", &(done + 1).min(total).to_string()), ("total", &total.to_string())])
                    }
                    _ => crate::i18n::tr("file_load.reading").to_string(),
                }
            };
            ui.horizontal(|ui| {
                ui.spinner();
                ui_kit::text(ui, &status, ui_kit::Text::Report("status", false));
            });
            // Always there, so the card keeps its height as the phases change.
            let fraction = if cancelled {
                0.0
            } else if let Some((_, d, t)) = work {
                d as f32 / t.max(1) as f32
            } else if total == 0 {
                0.0
            } else if phase == 0 {
                done as f32 / total as f32
            } else {
                1.0
            };
            // Linked designs read again: how far through them, the one being read in part.
            let fraction = match r.progress.item().filter(|_| r.request.kind == Kind::UpdateLinks && !cancelled) {
                Some((_, i, n)) => (i as f32 + fraction.clamp(0.0, 1.0)) / n.max(1) as f32,
                None => fraction,
            };
            ui_kit::progress(ui, "file_load", fraction);
        });
    // Cancel (and Esc) was a handle of its own before the card became a kit Prompt.
    if let Some(at) = crate::scenario::handle_at("dialog:cancel") {
        crate::scenario::publish_handle("file_loading:cancel", at);
    }
    let cancel = closed || action == ui_kit::Action::Cancel;
    if cancel && !cancelled {
        // The worker stops at its next cancellation checkpoint; its scratch design and
        // result channels are isolated from any file operation started afterwards.
        app.file_load.cancel();
        app.set_status(if export { crate::i18n::tr_label("dxf.cancelled") } else { "file operation cancelled" }, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn completed_open(app: &mut SolveApp, request: Request, session: Session) {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(Ok(Made { session, result: json!({"loaded": true}), thumbnail: false })).unwrap();
        app.file_load.running = Some(Running {
            request,
            base: app.session.doc.clone(),
            base_revision: app.session.revision,
            before: Vec::new(),
            rx,
            phase: Arc::new(AtomicU8::new(0)),
            progress: Arc::new(Progress::default()),
            preview_rx: None,
            preview: None,
            partial_rx: None,
            built: None,
            staged: None,
        });
    }

    #[test]
    fn repeated_queued_opens_reply_once_and_keep_one_session() {
        let mut app = SolveApp::new(Session::default(), Default::default());
        let mut session = Session::default();
        session.path = Some("queued.solidcraft".into());
        let mut first = Request::new("queued.solidcraft", Kind::Open);
        let (tx, first_reply) = std::sync::mpsc::channel();
        first.reply = Some(tx);
        completed_open(&mut app, first, session);
        let mut replies = vec![first_reply];
        for _ in 0..3 {
            let mut request = Request::new("queued.solidcraft", Kind::Open);
            let (tx, rx) = std::sync::mpsc::channel();
            request.reply = Some(tx);
            replies.push(rx);
            begin_request(&mut app, request).unwrap();
        }
        let ctx = egui::Context::default();
        tick(&mut app, &ctx);
        let loaded = app.session.doc.clone();
        app.session.execute("solid.box", &json!({"length": 2, "width": 3, "height": 4})).unwrap();
        let dirty = app.session.doc.clone();
        assert!(!Arc::ptr_eq(&loaded, &dirty));
        for _ in 0..3 {
            tick(&mut app, &ctx);
        }
        assert!(app.file_load.idle());
        assert_eq!(app.docs.count(), 1);
        assert!(Arc::ptr_eq(&dirty, &app.session.doc));
        assert!(app.session.is_dirty());
        for rx in replies {
            assert_eq!(rx.try_recv().unwrap()["ok"], true);
            assert!(rx.try_recv().is_err());
        }
    }

    #[test]
    fn finished_open_reuses_existing_dirty_tab_and_scope_still_applies() {
        let mut app = SolveApp::new(Session::default(), Default::default());
        app.session.path = Some("existing.solidcraft".into());
        app.session.execute("solid.box", &json!({"length": 2, "width": 3, "height": 4})).unwrap();
        let dirty = app.session.doc.clone();
        let undo = app.session.history.len();
        let camera = app.cam;
        crate::documents::new_tab(&mut app);
        let mut loaded = Session::default();
        loaded.path = Some("existing.solidcraft".into());
        let mut request = Request::new("existing.solidcraft", Kind::Open);
        let (tx, rx) = std::sync::mpsc::channel();
        request.reply = Some(tx);
        completed_open(&mut app, request, loaded);
        tick(&mut app, &egui::Context::default());
        assert_eq!(rx.try_recv().unwrap()["result"]["features"], 1);
        assert_eq!(app.docs.count(), 2);
        assert_eq!(app.docs.active, 0);
        assert!(Arc::ptr_eq(&dirty, &app.session.doc));
        assert_eq!(app.session.history.len(), undo);
        assert_eq!(app.cam, camera);
        assert!(app.session.is_dirty());

        let mut denied = Request::new("existing.solidcraft", Kind::Open);
        denied.scope = Some(Default::default());
        let (tx, rx) = std::sync::mpsc::channel();
        denied.reply = Some(tx);
        begin_request(&mut app, denied).unwrap();
        assert_eq!(rx.try_recv().unwrap()["ok"], false);
        assert!(rx.try_recv().is_err());
        assert!(app.file_load.idle());
        assert!(Arc::ptr_eq(&dirty, &app.session.doc));
        // ui.file dispatch carries its scope on the calling thread; preserve it in the queue.
        solidcraft_engine::io::vfs::scoped(Some(Default::default()), || {
            begin(&mut app, "existing.solidcraft", false);
        });
        assert!(app.status.as_ref().is_some_and(|s| s.2 && s.0.contains("outside the folders")));
        assert!(!reuse_open(&mut app, &Request::new("existing.solidcraft", Kind::Insert)));
        assert!(!reuse_open(&mut app, &Request::new("existing.solidcraft", Kind::Recover { id: "test".into(), dir: None })));
    }

    #[test]
    fn drawing_insert_and_drop_reuse_the_active_sketch_or_create_xy() {
        let mut h = crate::scenario::Harness::new();
        let dxf = solidcraft_engine::io::write_dxf(&[
            solidcraft_engine::io::Geom2::Line(solidcraft_engine::geom::Vec2::ZERO, solidcraft_engine::geom::Vec2::new(10.0, 0.0)),
            solidcraft_engine::io::Geom2::Arc {
                c: solidcraft_engine::geom::Vec2::ZERO,
                a: solidcraft_engine::geom::Vec2::new(5.0, 0.0),
                b: solidcraft_engine::geom::Vec2::new(0.0, 5.0),
            },
        ]);
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0 L10 0 L10 10"/></svg>"#;
        for (file, text) in [("outline.DXF", dxf.as_str()), ("outline.SVG", svg)] {
            h.app.run("file.new", json!({})).unwrap();
            h.app.run("sketch.create", json!({"plane":"YZ"})).unwrap();
            h.app.run("sketch.finish", json!({})).unwrap();
            let path = h.dir.join(file).to_string_lossy().into_owned();
            std::fs::write(&path, text).unwrap();
            h.app.insert_path_async(&path);
            let wait = |h: &mut crate::scenario::Harness| {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
                while h.app.file_load.busy() && std::time::Instant::now() < deadline {
                    h.frames(1);
                    std::thread::yield_now();
                }
                assert!(!h.app.file_load.busy());
            };
            wait(&mut h);
            assert_eq!(h.app.session.doc.features.len(), 2, "{:?}", h.app.status);
            let id = h.app.session.active_sketch.unwrap();
            assert_eq!(h.app.session.model.state().sketch(id).unwrap().plane, solidcraft_engine::geom::Plane::XY);
            assert_eq!(h.app.session.doc.sketch(id).unwrap().curves.len(), 2);
            h.app.cam.set_view(solidcraft_engine::render::StandardView::Top);
            h.app.fit_view();
            h.run(&json!([{ "shot": format!("insert-{file}") }])).unwrap();
            h.app.drop_path(&path);
            wait(&mut h);
            assert_eq!(h.app.session.active_sketch, Some(id));
            assert_eq!(h.app.session.doc.features.len(), 2);
            assert_eq!(h.app.session.doc.sketch(id).unwrap().curves.len(), 4);
            h.app.run("sketch.finish", json!({})).unwrap();
            h.app.drop_path(&path);
            wait(&mut h);
            assert_eq!(h.app.session.doc.features.len(), 3);
            let new_id = h.app.session.active_sketch.unwrap();
            assert_ne!(id, new_id);
            assert_eq!(h.app.session.doc.sketch(new_id).unwrap().curves.len(), 2);
            h.app.insert_path(&path);
            assert_eq!(h.app.session.doc.sketch(new_id).unwrap().curves.len(), 4);
        }
    }

    #[test]
    fn point_cloud_insert_keeps_all_options_on_the_worker_request() {
        let mut request = Request::new("scan.PLY", Kind::Insert);
        request.options =
            json!({"units": "m", "max_points": 1000, "color_by": "height", "height_range": [1, 20], "name": "Scan"}).as_object().unwrap().clone();
        let (command, params) = request.command();
        assert_eq!(command, "pointcloud.insert");
        assert_eq!(
            params,
            json!({"path": "scan.PLY", "units": "m", "max_points": 1000, "color_by": "height", "height_range": [1, 20], "name": "Scan"})
        );
    }

    #[test]
    fn dxf_export_progress_keeps_frames_alive_and_never_adopts_a_worker_document() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
        h.frames(3);
        let base = h.app.session.doc.clone();
        let undo = h.app.session.history.len();
        let cam = h.app.cam;
        let progress = Arc::new(Progress::default());
        solidcraft_engine::doc::progress::watch(progress.clone(), || {
            solidcraft_engine::kernel::watch::current().unwrap().progress("dxf_occlusion", 64, 128);
        });
        let (tx, rx) = std::sync::mpsc::channel();
        h.app.file_load.running = Some(Running {
            request: Request::new("drawing.dxf", Kind::ExportDxf),
            base: base.clone(),
            base_revision: h.app.session.revision,
            before: Vec::new(),
            rx,
            phase: Arc::new(AtomicU8::new(0)),
            progress: progress.clone(),
            preview_rx: None,
            preview: None,
            partial_rx: None,
            built: None,
            staged: None,
        });
        // No worker result is available: drawing a frame must not wait for one.
        h.frames(4);
        assert!(h.app.file_load.busy());
        let at = crate::scenario::handle_at("file_loading:cancel").unwrap();
        let (request, _reply) = crate::control::ControlRequest::new("ui.click", json!({"x":at.x,"y":at.y}));
        let _ = crate::control::handle(&mut h.app, &h.ctx, &request);
        h.frames(4);
        assert!(progress.is_cancelled());
        tx.send(Err("DXF export cancelled".into())).unwrap();
        h.frames(2);
        assert!(h.app.file_load.idle());
        assert!(Arc::ptr_eq(&base, &h.app.session.doc));
        assert_eq!(h.app.session.history.len(), undo);
        assert_eq!(h.app.cam, cam);

        // The success path is equally read-only, even if the worker has another session.
        let mut other = Session::default();
        other.execute("file.new", &json!({"name":"Worker must not replace the current design"})).unwrap();
        let mut request = Request::new("drawing.dxf", Kind::ExportDxf);
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        request.reply = Some(reply_tx);
        let (tx, rx) = std::sync::mpsc::channel();
        h.app.file_load.running = Some(Running {
            request,
            base: base.clone(),
            base_revision: h.app.session.revision,
            before: Vec::new(),
            rx,
            phase: Arc::new(AtomicU8::new(0)),
            progress: Arc::new(Progress::default()),
            preview_rx: None,
            preview: None,
            partial_rx: None,
            built: None,
            staged: None,
        });
        tx.send(Ok(Made { session: other, result: json!({"visible":12}), thumbnail: false })).unwrap();
        h.frames(2);
        assert_eq!(reply_rx.try_recv().unwrap()["result"]["visible"], 12);
        assert!(Arc::ptr_eq(&base, &h.app.session.doc));
        assert_eq!(h.app.session.history.len(), undo);
        assert_eq!(h.app.cam, cam);
    }

    #[test]
    fn opening_result_and_preview_cannot_overwrite_a_newer_design() {
        for (insert, change_document) in [(false, true), (true, true), (false, false)] {
            let mut app = SolveApp::new(Session::default(), Default::default());
            let base = app.session.doc.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            let (preview_tx, preview_rx) = std::sync::mpsc::channel();
            preview_tx.send(egui::ColorImage::new([2, 2], vec![egui::Color32::RED; 4])).unwrap();
            app.file_load.running = Some(Running {
                request: Request::new("old.solidcraft", if insert { Kind::Insert } else { Kind::Open }),
                base,
                base_revision: app.session.revision,
                before: Vec::new(),
                rx,
                phase: Arc::new(AtomicU8::new(0)),
                progress: Arc::new(Progress::default()),
                preview_rx: Some(preview_rx),
                preview: None,
                partial_rx: None,
                built: None,
                staged: None,
            });
            if change_document {
                app.session.execute("file.new", &serde_json::json!({"name":"Newer design"})).unwrap();
            } else {
                // An edit and undo can return to the original Arc; the generation still changed.
                app.session.revision += 1;
            }
            let newer = app.session.doc.clone();
            tick(&mut app, &crate::fonts::test_context());
            assert!(app.file_load.running.as_ref().unwrap().preview.is_none());
            tx.send(Ok(Made { session: Session::default(), result: Value::Null, thumbnail: false })).unwrap();
            tick(&mut app, &crate::fonts::test_context());
            assert!(Arc::ptr_eq(&newer, &app.session.doc));
            assert!(app.status.as_ref().unwrap().0.contains("discarded"));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn provisional_picture_renders_without_adopting_cached_meshes() {
        if !crate::gpu::test_picture_renderer() {
            return;
        }
        let preview = solidcraft_engine::io::DesignPreview {
            meshes: vec![solidcraft_engine::doc::MeshData {
                name: "Saved triangle".into(),
                positions: vec![0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 10.0, 0.0].into(),
                triangles: vec![0, 1, 2].into(),
                color: Some([0.8, 0.2, 0.1]),
            }],
        };
        let picture = saved_picture(preview).unwrap();
        assert_eq!(picture.size, [320, 200]);
        assert!(picture.pixels.iter().any(|p| u16::from(p.r()) > u16::from(p.g()) * 2));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn repeated_large_preview_triangles_are_a_cache_miss_before_drawing() {
        let preview = solidcraft_engine::io::DesignPreview {
            meshes: vec![solidcraft_engine::doc::MeshData {
                name: "Hostile overdraw".into(),
                positions: vec![0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 10.0, 0.0].into(),
                triangles: (0..10_000).flat_map(|_| [0, 1, 2]).collect(),
                color: None,
            }],
        };
        assert!(saved_picture(preview).is_none());
    }
    #[test]
    fn failed_import_keeps_the_current_document() {
        let mut app = SolveApp::new(Session::default(), Default::default());
        let old = app.session.doc.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(Err("bad STEP".into())).unwrap();
        app.file_load.running = Some(Running {
            request: Request::new("bad.step", Kind::Insert),
            base: old.clone(),
            base_revision: app.session.revision,
            before: Vec::new(),
            rx,
            phase: Arc::new(AtomicU8::new(0)),
            progress: Arc::new(Progress::default()),
            preview_rx: None,
            preview: None,
            partial_rx: None,
            built: None,
            staged: None,
        });
        tick(&mut app, &crate::fonts::test_context());
        assert!(Arc::ptr_eq(&app.session.doc, &old));
        assert!(!app.file_load.busy());
        assert!(app.status.as_ref().unwrap().0.contains("bad STEP"));
    }
    #[test]
    fn cancelling_disconnects_late_results_and_answers_queued_callers() {
        for finished in [false, true] {
            let mut app = SolveApp::new(Session::default(), Default::default());
            let old = app.session.doc.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            let (reply_tx, reply_rx) = std::sync::mpsc::channel();
            let mut request = Request::new("cancelled.step", Kind::Insert);
            request.reply = Some(reply_tx);
            let progress = Arc::new(Progress::default());
            app.file_load.running = Some(Running {
                request,
                base: old.clone(),
                base_revision: app.session.revision,
                before: Vec::new(),
                rx,
                phase: Arc::new(AtomicU8::new(0)),
                progress: progress.clone(),
                preview_rx: None,
                preview: None,
                partial_rx: None,
                built: None,
                staged: None,
            });
            let (queued_tx, queued_rx) = std::sync::mpsc::channel();
            let mut queued = Request::new("queued.step", Kind::Open);
            queued.reply = Some(queued_tx);
            app.file_load.queued.push_back(queued);
            let mut computed = Session::default();
            computed.execute("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
            let made = Ok(Made { session: computed, result: Value::Null, thumbnail: false });
            // Both races: the result was queued before Cancel, or arrives afterwards.
            let late = if finished {
                tx.send(made).unwrap();
                None
            } else {
                Some(made)
            };
            app.file_load.cancel();
            assert!(progress.is_cancelled());
            assert!(app.file_load.idle());
            for reply in [&reply_rx, &queued_rx] {
                assert_eq!(reply.try_recv().unwrap(), json!({"ok":false,"error":"file operation cancelled"}));
                assert!(reply.try_recv().is_err(), "each caller is answered once");
            }
            app.session.execute("solid.box", &json!({"length":20,"width":20,"height":20})).unwrap();
            let newer = app.session.doc.clone();
            if let Some(late) = late {
                assert!(tx.send(late).is_err(), "the old worker cannot reach the next operation");
            }
            tick(&mut app, &crate::fonts::test_context());
            assert!(Arc::ptr_eq(&newer, &app.session.doc));
        }
    }

    /// #1376: the web app reads a STEP file a piece per frame. The first frame only shows the
    /// overlay, the next parses, and the solids follow, frame by frame, until the insert
    /// lands as a worker's would; Cancel between frames leaves the design as it was.
    #[test]
    fn a_staged_step_import_lands_over_frames_and_cancels_between_them() {
        let dir = std::env::temp_dir().join(format!("solidcraft-staged-step-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("boxes.step").to_string_lossy().into_owned();
        let mut part = Session::default();
        for i in 0..6 {
            part.execute("solid.box", &serde_json::json!({"length": 10, "width": 10, "height": 10, "corner": [i * 20, 0, 0]})).unwrap();
        }
        part.execute("file.export", &serde_json::json!({"path": path})).unwrap();
        let ctx = crate::fonts::test_context();
        let mut app = SolveApp::new(Session::default(), Default::default());
        let bodies = |app: &SolveApp| app.session.world_state().bodies.len();
        assert!(staged_kind(&Request::new(&path, Kind::Insert)) && staged_kind(&Request::new(&path, Kind::Open)));
        assert!(!staged_kind(&Request::new("part.igs", Kind::Insert)) && !staged_kind(&Request::new("part.solidcraft", Kind::Insert)));
        app.file_load.queued.push_back(Request::new(&path, Kind::Insert));
        start_staged(&mut app);
        tick(&mut app, &ctx);
        assert!(app.file_load.busy() && bodies(&app) == 0, "the first frame only shows the overlay");
        tick(&mut app, &ctx);
        let reading = app.file_load.running.as_ref().and_then(|r| r.staged.as_ref()).is_some_and(|s| s.reading.is_some());
        assert!(reading, "the next frame parses the file");
        let mut frames = 2;
        while app.file_load.running.is_some() && frames < 1000 {
            tick(&mut app, &ctx);
            frames += 1;
        }
        assert_eq!(bodies(&app), 6, "{:?}", app.status);
        assert!(app.status.as_ref().is_some_and(|s| s.0.contains("Insert done")), "{:?}", app.status);
        // Cancelled after the parse: nothing is inserted, and nothing waits.
        let before = app.session.doc.clone();
        app.file_load.queued.push_back(Request::new(&path, Kind::Insert));
        start_staged(&mut app);
        tick(&mut app, &ctx);
        tick(&mut app, &ctx);
        app.file_load.cancel();
        tick(&mut app, &ctx);
        assert!(app.file_load.running.is_none() && !app.file_load.busy());
        assert!(Arc::ptr_eq(&before, &app.session.doc));
        assert_eq!(bodies(&app), 6);
        // A file that isn't STEP fails like the worker's would, and the design stays.
        let junk = dir.join("junk.step").to_string_lossy().into_owned();
        std::fs::write(&junk, "not a step file").unwrap();
        app.file_load.queued.push_back(Request::new(&junk, Kind::Insert));
        start_staged(&mut app);
        for _ in 0..4 {
            tick(&mut app, &ctx);
        }
        assert!(app.file_load.running.is_none());
        assert!(app.status.as_ref().is_some_and(|s| s.2 && s.0.contains("junk.step")), "{:?}", app.status);
        assert!(Arc::ptr_eq(&before, &app.session.doc));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #10198: the web app opens a design a few features a frame, reading the STEP file a feature
    /// imports in pieces first, behind the same progress card and Cancel as a STEP file; the
    /// design that lands is the one `doc.open` opens.
    #[test]
    fn a_staged_design_open_lands_over_frames_and_cancels_between_them() {
        let dir = std::env::temp_dir().join(format!("solidcraft-staged-open-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("staged.sc").to_string_lossy().into_owned();
        let step = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/step-conformance/cylinder_seam.step");
        let mut part = Session::default();
        part.execute("solid.box", &serde_json::json!({"length": 10, "width": 10, "height": 10})).unwrap();
        part.execute("file.insert_step", &serde_json::json!({"path": step})).unwrap();
        part.execute("component.activate", &serde_json::json!({"component": "root"})).unwrap();
        part.execute("solid.box", &serde_json::json!({"length": 5, "width": 5, "height": 5, "corner": [40, 0, 0]})).unwrap();
        part.execute("file.save", &serde_json::json!({"path": path, "cache": false})).unwrap();
        let mut whole = Session::default();
        whole.execute("doc.open", &serde_json::json!({"path": path})).unwrap();
        let names = |s: &Session| s.world_state().bodies.iter().map(|b| b.name.clone()).collect::<Vec<_>>();

        let ctx = crate::fonts::test_context();
        let mut app = SolveApp::new(Session::default(), Default::default());
        assert!(staged_kind(&Request::new(&path, Kind::Open)) && !staged_kind(&Request::new(&path, Kind::Insert)));
        let opening =
            |app: &SolveApp| app.file_load.running.as_ref().and_then(|r| r.staged.as_ref()).and_then(|s| s.opening.as_ref()).map(|o| o.progress());
        app.file_load.queued.push_back(Request::new(&path, Kind::Open));
        start_staged(&mut app);
        tick(&mut app, &ctx);
        assert!(app.file_load.busy() && opening(&app).is_none(), "the first frame only shows the overlay");
        tick(&mut app, &ctx);
        let features = whole.doc.features.len();
        assert_eq!(opening(&app), Some((0, features)), "the next frame reads the file, evaluating nothing yet");
        assert_eq!(app.file_load.features(), Some((0, features)), "the card counts the design's features");
        let mut frames = 2;
        while app.file_load.running.is_some() && frames < 1000 {
            tick(&mut app, &ctx);
            frames += 1;
        }
        assert!(app.status.as_ref().is_some_and(|s| s.0.contains("Open done")), "{:?}", app.status);
        assert_eq!(app.session.path.as_deref(), Some(path.as_str()));
        assert_eq!(app.session.doc, whole.doc);
        assert_eq!(names(&app.session), names(&whole));
        assert_eq!(names(&app.session).len(), 3);

        // Cancelled after the file is read: the design before stays, and nothing waits.
        let before = app.session.doc.clone();
        app.file_load.queued.push_back(Request::new(&path, Kind::Open));
        start_staged(&mut app);
        tick(&mut app, &ctx);
        tick(&mut app, &ctx);
        app.file_load.cancel();
        tick(&mut app, &ctx);
        assert!(app.file_load.running.is_none() && !app.file_load.busy());
        assert!(Arc::ptr_eq(&before, &app.session.doc));
        // A file that isn't a design fails, and the design stays.
        let junk = dir.join("junk.sc").to_string_lossy().into_owned();
        std::fs::write(&junk, "not a design").unwrap();
        app.file_load.queued.push_back(Request::new(&junk, Kind::Open));
        start_staged(&mut app);
        for _ in 0..4 {
            tick(&mut app, &ctx);
        }
        assert!(app.file_load.running.is_none());
        assert!(app.status.as_ref().is_some_and(|s| s.2 && s.0.contains("junk.sc")), "{:?}", app.status);
        assert!(Arc::ptr_eq(&before, &app.session.doc));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #10198: while a design opens in stages, the card shows an import's progress only while
    /// its STEP file is read, and "Rebuilding feature N of M" again once it is; the kernel's
    /// last count of that file must not stay up until the next import.
    #[test]
    fn a_staged_design_open_shows_an_import_only_while_its_file_is_read() {
        let dir = std::env::temp_dir().join(format!("solidcraft-staged-work-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("staged.sc").to_string_lossy().into_owned();
        let step = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/step-conformance/cylinder_seam.step");
        let mut part = Session::default();
        part.execute("file.insert_step", &serde_json::json!({"path": step})).unwrap();
        part.execute("component.activate", &serde_json::json!({"component": "root"})).unwrap();
        part.execute("solid.box", &serde_json::json!({"length": 5, "width": 5, "height": 5, "corner": [40, 0, 0]})).unwrap();
        part.execute("file.save", &serde_json::json!({"path": path, "cache": false})).unwrap();

        let ctx = crate::fonts::test_context();
        let mut app = SolveApp::new(Session::default(), Default::default());
        app.file_load.queued.push_back(Request::new(&path, Kind::Open));
        start_staged(&mut app);
        tick(&mut app, &ctx);
        tick(&mut app, &ctx);
        // One step of the open at a time, as if every step filled a frame.
        let (mut read, mut after_read) = (false, false);
        for _ in 0..10_000 {
            let Some(r) = app.file_load.running.as_mut() else { break };
            let Some(staged) = r.staged.as_mut() else { break };
            if staged.opening.is_none() {
                break;
            }
            let once = std::cell::Cell::new(true);
            if step_opening(staged, &r.progress, None, &|| once.replace(false)).is_some() {
                break;
            }
            let reading = r.staged.as_ref().is_some_and(|s| s.reading.is_some());
            let work = r.work();
            if reading {
                read |= work.is_some();
            } else {
                after_read |= read;
                assert_eq!(work, None, "no import's progress once its file is read");
            }
        }
        assert!(read && after_read, "the import's progress showed while its file was read, then the open went on");
    }
}
