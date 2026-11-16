//! Interactive file operations run off the window thread. The old design stays intact until
//! the command and display meshes are ready; failures and cancelled jobs never replace it.
//!
//! While one runs, the window keeps drawing frames, and the operation is a row of the operations
//! panel ([`crate::operations`]) with its progress and Cancel; several wait their turn there. The
//! design stays usable meanwhile, and a result lands after the person's edits:
//! - A file opened (or recovered) lands as it always did when the person is not in the middle of
//!   something; otherwise (a dialog, a tool or another sketch open, their own command computing)
//!   it goes in a new tab behind the one shown, and the status says so.
//! - An insert, an export and Update Links belong to the tab shown when they were asked for: one
//!   finished while another tab is shown, or while the person is in the middle of something, waits
//!   ("Ready") until they are done. One whose design changed meanwhile runs again for the design
//!   as it is, with the file it read still cached, at most [`MAX_TRIES`] times.
//!
//! With Preferences > Layout > "Wait for files to open" on, it is as it always was: a light
//! overlay over the design shows the file's progress with Cancel, the overlay takes the pointer,
//! and shortcuts and commands wait. Either way, control-channel commands that change the design
//! are held until the file is in (an agent's open and its edits go in that order), and a control
//! (MCP) `doc.open` or import is answered when its file finishes loading.
//!
//! Update Links from the toolbar runs the same way (#5329): it reads every linked design again
//! and rebuilds it, which takes a while in an assembly of many parts, so the overlay names the
//! linked design being read and its rebuild's progress. Insert Design reads the saved design it
//! inserts the same way (#11593): a large one rebuilds for a while.

use crate::i18n::LocaleNames as _;
use crate::ui_kit::{Container, Surface};
#[cfg(not(target_arch = "wasm32"))]
use solidcraft_engine::geom::prelude::*;
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
use crate::operations::Outcome;
use crate::ui_kit;

/// What a file operation does with its file.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// Open the design (or a CAD, mesh or drawing file as a new design) in a tab of its own.
    Open,
    /// Insert CAD bodies, meshes or sketch drawing geometry into the active design.
    Insert,
    /// Insert a saved design into the active design as a linked component
    /// (`component.insert_design`): reading a large design takes a while (#11593).
    InsertDesign,
    /// Open a design an earlier run autosaved before it closed.
    Recover { id: String, dir: Option<String> },
    /// Export a snapshot of the current design without adopting the worker's session.
    ExportDxf,
    /// Read the active design's linked designs again (`component.update_links`, with the
    /// request's options: `component` or `components` reads only those links).
    UpdateLinks,
    /// Read a point cloud's file again (`pointcloud.reload`, with the request's `cloud`).
    ReloadCloud,
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
    /// Insert the file as a mesh whatever its extension says: a PLY goes to `pointcloud.insert`
    /// unless the person chose Insert Mesh for it (then it needs its faces).
    pub mesh: bool,
    /// The level the caller's own `param_check` named and the warnings it found before the run:
    /// the reply says both (#9184).
    pub(crate) param_check: Option<(solidcraft_engine::schema::ParamCheck, Vec<String>)>,
    /// The status line once the file is in, in place of the usual one.
    pub note: Option<String>,
    /// Which operation it is ([`crate::operations::serial`]).
    pub serial: u64,
    /// The tab whose design it works on (an insert, an export, Update Links), set when it is
    /// queued: it starts and lands only while that tab is shown.
    pub tab: Option<u64>,
    /// How many times it ran again because the design changed while it ran.
    pub tries: u8,
    /// The Verify preference added `verify` to the options; unlike a caller's own option it must
    /// not stop an already-open design from being reused. Its check then runs separately.
    pub(crate) auto_verify: bool,
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
            mesh: false,
            param_check: None,
            note: None,
            serial: crate::operations::serial(),
            tab: None,
            tries: 0,
            auto_verify: false,
        }
    }
    /// It adds what it reads to the active design.
    fn insert(&self) -> bool {
        matches!(self.kind, Kind::Insert | Kind::InsertDesign)
    }
    /// It reads files the active design already names, and changes that design in place.
    fn reads_again(&self) -> bool {
        matches!(self.kind, Kind::UpdateLinks | Kind::ReloadCloud)
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
            Kind::Insert if self.mesh || solidcraft_engine::io::is_mesh_path(&self.path) => ("file.insert_mesh", self.params()),
            Kind::Insert if solidcraft_engine::io::is_point_cloud_path(&self.path) => ("pointcloud.insert", self.params()),
            Kind::Insert => (insert_command(&self.path), self.params()),
            Kind::InsertDesign => ("component.insert_design", self.params()),
            Kind::Recover { id, dir } => ("doc.recover", json!({"id": id, "dir": dir})),
            Kind::ExportDxf => ("file.export_view_dxf", Value::Object(self.options.clone())),
            Kind::UpdateLinks => ("component.update_links", Value::Object(self.options.clone())),
            Kind::ReloadCloud => ("pointcloud.reload", Value::Object(self.options.clone())),
        }
    }
    fn answer(&self, r: Result<Value, String>) {
        if let Some(reply) = &self.reply {
            let mut response = if crate::control::chat_turn_cancelled(self.chat_turn.as_deref()) {
                crate::control::cancelled_chat_response()
            } else {
                match r {
                    Ok(v) => json!({"ok": true, "result": v}),
                    Err(e) => json!({"ok": false, "error": e}),
                }
            };
            if let Some((level, warnings)) = &self.param_check {
                crate::control::add_param_warnings(&mut response, warnings.clone());
                if let Some(o) = response.as_object_mut() {
                    o.insert("param_check".into(), json!(level.name()));
                }
            }
            let _ = reply.send(response);
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
    /// Its result, once in, while it waits to land ([`Land::Wait`]).
    ready: Option<Job>,
    /// The sketch open when it started (an insert into it): still open, it doesn't keep it waiting.
    sketch: Option<u64>,
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
    /// Results that are in and wait to land: their design's tab is not shown, or the person is in
    /// the middle of something. They do not hold the worker, so the next request starts while
    /// they wait, and they land as soon as they can.
    waiting: Vec<Running>,
    /// The result of an insert that runs again for the design as it is now, kept until it is in:
    /// the file it read stays cached ([`solidcraft_engine::kernel::step_import_shared`]), so the
    /// second run doesn't read it again.
    warm: Option<(u64, Box<Session>)>,
    /// Whether the card over the design is showing this frame: the file operation's progress and
    /// Cancel when the operations panel cannot show its row (Home, Wait for files, off, or no room).
    pub(crate) card_shown: bool,
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
        self.running.is_none() && self.waiting.is_empty() && self.queued.is_empty()
    }
    /// Signal cancellation, release the import slot and drop queued requests.
    pub fn cancel(&mut self) {
        self.cancel_running();
        for w in std::mem::take(&mut self.waiting) {
            w.request.answer(Err("file operation cancelled".into()));
        }
        for q in std::mem::take(&mut self.queued) {
            q.answer(Err("file operation cancelled".into()));
        }
        // Nothing is left to run again for.
        self.warm = None;
    }
    /// Signal the running request's cancellation and release its slot; queued requests go on.
    pub(crate) fn cancel_running(&mut self) {
        if let Some(r) = &self.running {
            r.progress.cancel();
            if self.warm.as_ref().is_some_and(|(serial, _)| *serial == r.request.serial) {
                self.warm = None;
            }
        }
        // Imports only change a scratch session. Disconnect their result and preview channels
        // so a slow cancellation checkpoint cannot block the next file or apply a late result.
        // Exports publish a file: keep their reply until the worker reports whether it committed.
        if self.running.as_ref().is_some_and(|r| r.request.kind != Kind::ExportDxf)
            && let Some(r) = self.running.take()
        {
            r.request.answer(Err("file operation cancelled".into()));
        }
    }
    /// Drop the `index`th queued request (from 0), answering it as cancelled.
    pub(crate) fn cancel_queued(&mut self, index: usize) {
        if let Some(q) = self.queued.remove(index) {
            self.drop_warm(q.serial);
            q.answer(Err("file operation cancelled".into()));
        }
    }
    /// Forget a retry's cached session once its request is gone (cancelled, or its tab closed).
    fn drop_warm(&mut self, serial: u64) {
        if self.warm.as_ref().is_some_and(|(warm, _)| *warm == serial) {
            self.warm = None;
        }
    }
    /// The operations panel's view of the requests: the running one (if any), then the queued
    /// ones in the order they will run. Each is its command, its file's name, how far it is
    /// (`done` of `total`, when that is known) and whether it was cancelled.
    pub(crate) fn operations(&self) -> Vec<FileOperation> {
        let entry = |request: &Request| FileOperation {
            serial: request.serial,
            command: request.command().0,
            name: match &request.kind {
                Kind::Recover { .. } => request.path.clone(),
                _ => std::path::Path::new(&request.path).file_name().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
            },
            steps: None,
            percent: false,
            queued: true,
            cancelled: false,
            waiting: false,
        };
        let running = self.running.as_ref().map(|r| FileOperation {
            steps: steps(r),
            percent: r.request.kind != Kind::UpdateLinks && work(r).is_some_and(|(stage, _, _)| crate::operations::bytes_stage(stage)),
            queued: false,
            cancelled: r.cancelled(),
            waiting: r.ready.as_ref().is_some_and(Result::is_ok),
            ..entry(&r.request)
        });
        let waiting = self.waiting.iter().map(|r| FileOperation {
            steps: steps(r),
            queued: false,
            cancelled: r.cancelled(),
            waiting: true,
            ..entry(&r.request)
        });
        running.into_iter().chain(waiting).chain(self.queued.iter().map(entry)).collect()
    }
}

/// A file operation as the operations panel shows it ([`FileLoad::operations`]).
#[derive(Clone, Debug)]
pub(crate) struct FileOperation {
    pub serial: u64,
    pub command: &'static str,
    pub name: String,
    pub steps: Option<(usize, usize)>,
    /// The steps are bytes (the file being read).
    pub percent: bool,
    pub queued: bool,
    pub cancelled: bool,
    /// Done, and waiting to land ([`Land::Wait`]).
    pub waiting: bool,
}

/// How far a running request is: the long work inside a feature (a large STEP file read), the
/// linked designs read again, or the features rebuilt; `None` when that is not known yet.
/// The long work inside one feature the running request is in (a large STEP file read), if any.
fn work(r: &Running) -> Option<(&'static str, usize, usize)> {
    let phase = r.phase.load(Ordering::Relaxed);
    r.work().filter(|(_, d, t)| r.request.kind == Kind::ExportDxf || (phase == 0 && d < t))
}

fn steps(r: &Running) -> Option<(usize, usize)> {
    if r.request.kind == Kind::UpdateLinks
        && let Some((_, i, n)) = r.progress.item()
    {
        return Some((i, n));
    }
    // As the card reads them: in the web app a design opened a few features a frame counts them
    // itself ([`Running::features`], [`Running::work`]).
    let phase = r.phase.load(Ordering::Relaxed);
    if let Some((_, d, t)) = work(r) {
        return Some((d, t));
    }
    let (done, total) = r.features();
    (total > 0).then_some(if phase == 0 { (done, total) } else { (total, total) })
}

impl SolveApp {
    /// A file operation holds the design: with Preferences > Layout > "Wait for files to open"
    /// on, the design waits (commands, shortcuts and edits) while one runs or waits its turn.
    /// Off, the design stays usable: an operation's result lands after the person's edits.
    pub fn file_load_holds(&self) -> bool {
        self.preferences.file_operations_wait && self.file_load.busy()
    }
}

pub fn begin(app: &mut SolveApp, path: &str, insert: bool) {
    // An STL or OBJ file states no unit: ask which, then import (see `crate::mesh_units`).
    if crate::mesh_units::ask(app, path, insert) {
        return;
    }
    let mut request = Request::new(path, if insert { Kind::Insert } else { Kind::Open });
    if !insert && app.preferences.verify_opened_designs && solidcraft_engine::io::is_design_path(path) {
        request.options.insert("verify".into(), json!(true));
        request.auto_verify = true;
    }
    let _ = begin_request(app, request);
}

/// A STEP, IGES, FreeCAD or Rhino file the app opens or inserts gets a `Group #N` history folder
/// while "Fold the History bar into groups" is on, and none while it is off (#8885), unless the
/// caller chose (`folder`).
pub(crate) fn step_folder_option(app: &SolveApp, path: &str, options: &mut serde_json::Map<String, Value>) {
    if solidcraft_engine::io::is_step_path(path) {
        options.entry("folder").or_insert(Value::Bool(app.preferences.history_bar_groups));
    }
}

/// The preference's `reuse_import` default for an import-backed open only: a native design's
/// request keeps empty options, so `reuse_open` can still switch to its open tab.
fn default_reuse_import(app: &SolveApp, request: &mut Request) {
    if request.kind == Kind::Open && solidcraft_engine::io::is_step_path(&request.path) {
        request.options.entry("reuse_import").or_insert(Value::Bool(app.preferences.reuse_imports));
    }
}

/// Import options the preferences add for `path` (with `history: solids` a FreeCAD file brings
/// only its solids, as before its history was rebuilt).
pub(crate) fn preferred_options(app: &SolveApp, path: &str) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();
    let freecad = std::path::Path::new(path).extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("fcstd"));
    if freecad && !app.preferences.freecad_history {
        out.insert("history".into(), json!("solids"));
    }
    out
}

/// Queue a file operation; on the web (no threads) it runs now.
pub fn begin_request(app: &mut SolveApp, mut request: Request) -> Result<(), String> {
    if request.kind == Kind::Open
        && app.preferences.verify_opened_designs
        && solidcraft_engine::io::is_design_path(&request.path)
        && !request.options.contains_key("verify")
    {
        request.options.insert("verify".into(), json!(true));
        request.auto_verify = true;
    }
    if crate::control::chat_turn_cancelled(request.chat_turn.as_deref()) {
        request.answer(Err("the agent chat turn ended".into()));
        return Ok(());
    }
    default_reuse_import(app, &mut request);
    if app.file_load.idle() && reuse_open(app, &request) {
        return Ok(());
    }
    if matches!(request.kind, Kind::Open | Kind::Insert) {
        // After `reuse_open`: an already open tab is kept whatever the import preferences say,
        // as the folder option is.
        for (k, v) in preferred_options(app, &request.path) {
            request.options.entry(k).or_insert(v);
        }
        step_folder_option(app, &request.path, &mut request.options);
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
                app.insert_path_with(&request.path, &request.options, request.mesh);
                Ok(Value::Null)
            }
            Kind::InsertDesign => {
                let (command, params) = request.command();
                let r = solidcraft_engine::io::vfs::allowing_hosts(&request.allow_hosts, || app.run(command, params));
                if r.is_ok() {
                    app.fit_view();
                    if request.note.is_none()
                        && let Some(note) = linked_signing_note(&request.path)
                    {
                        app.set_status(note, false);
                    }
                    if let Some(note) = &request.note {
                        app.set_status(note.clone(), false);
                    }
                }
                r
            }
            Kind::Open => {
                app.open_path_with(&request.path, &request.options);
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
                    app.set_status(solidcraft_ui_text::format("ui.saved_to", &[("path", &request.path)]), false);
                }
                r
            }
            Kind::UpdateLinks | Kind::ReloadCloud => {
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
pub(crate) fn queue(app: &mut SolveApp, mut request: Request) -> Result<(), String> {
    // One for a given saved design (`design`) is dropped when that design isn't shown; the
    // others wait for their tab.
    if !matches!(request.kind, Kind::Open | Kind::Recover { .. }) && request.tab.is_none() && request.design.is_none() {
        request.tab = Some(app.docs.active_tab());
    }
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
        // Let the command validate import-specific options, even for an existing tab. The
        // caller's own `verify` is such an option; the Verify preference's automatic one is not.
        let only_auto_verify =
            request.auto_verify && request.options.len() == 1 && request.options.get("verify").and_then(Value::as_bool) == Some(true);
        if !request.options.is_empty() && !only_auto_verify {
            return Ok(false);
        }
        crate::agent_chat::with_tool_request(app, request.chat_turn.as_ref(), |app| crate::documents::reuse_path(app, &request.path))
    });
    match reused {
        Ok(false) => false,
        Ok(true) => {
            // The session is already open; its saved file still gets the check the preference
            // asked for (on a worker, so a large design does not hold up this request).
            if request.auto_verify {
                crate::verify_job::start_after_reuse(app);
            }
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
    // In any spelling the engine takes: another would run on the window thread.
    let Some(id) =
        req.params.get("command").or(req.params.get("id")).and_then(Value::as_str).and_then(solidcraft_engine::find_command).map(|spec| spec.id)
    else {
        return Some(req);
    };
    let kind = match id {
        "doc.open" => Kind::Open,
        "file.insert_step" | "file.insert_mesh" | "pointcloud.insert" => Kind::Insert,
        "file.export_view_dxf" => Kind::ExportDxf,
        "pointcloud.reload" => Kind::ReloadCloud,
        _ => return Some(req),
    };
    let params = req.params.get("params");
    let path = match params.and_then(|p| p.get("path")).and_then(Value::as_str) {
        Some(path) => path,
        None if matches!(kind, Kind::ExportDxf | Kind::ReloadCloud) => "",
        None => return Some(req),
    };
    // This never reaches `control::handle`, so the request's own `param_check` is taken here. The
    // worker's session goes on to be the design's, which keeps its own level: the check comes first.
    let level = match crate::control::request_param_check(&req.method, &req.params) {
        Ok(level) => level,
        Err(e) => {
            let _ = req.reply.send(json!({"ok": false, "error": e}));
            return None;
        }
    };
    let param_check = match level
        .map(|level| crate::control::contract_check_before_run(id, params.unwrap_or(&Value::Null), level).map(|found| (level, found)))
        .transpose()
    {
        Ok(param_check) => param_check,
        Err(e) => {
            let _ = req.reply.send(json!({"ok": false, "error": e, "param_check": level.map(|level| level.name())}));
            return None;
        }
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
    if !matches!(kind, Kind::ExportDxf | Kind::ReloadCloud) {
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
        mesh: false,
        param_check,
        note: None,
        serial: crate::operations::serial(),
        tab: None,
        tries: 0,
        auto_verify: false,
    };
    let _ = begin_request(app, request);
    None
}

/// What to say once a linked design is inserted: whether its file carries a signature, without
/// reading every payload to check it. `None` for an unsigned file (nothing to add to the usual
/// "Inserted …" message).
fn linked_signing_note(path: &str) -> Option<String> {
    let summary = solidcraft_engine::io::read_design_provenance_summary_file(path)?;
    let name = std::path::Path::new(path).file_name().map_or_else(|| path.to_string(), |name| name.to_string_lossy().into_owned());
    Some(match &summary.signer {
        Some(signer) => solidcraft_ui_text::format("ui.insert_signed", &[("name", &name), ("key", signer)]),
        None => solidcraft_ui_text::format("ui.insert_signed_unknown", &[("name", &name)]),
    })
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
    app.say_newer_file();
}

/// A recovered design that landed in a background tab: save it under that tab's own autosaver
/// first, then discard the recovery entry it came from. [`recovered`] saves the shown design, so
/// it cannot be used here; until this runs a crash would offer the old (stale) entry again.
fn recovered_behind(app: &mut SolveApp, uid: u64, id: &str, dir: Option<&str>) {
    let (autosave, docs) = (&mut app.autosave, &mut app.docs);
    let Some(slot) = docs.background_slot_mut(uid) else { return };
    let kept = autosave.as_mut().map(|a| a.save_background(slot));
    if let Some(Ok(_)) = kept {
        let _ = slot.session.execute("doc.recovery_discard", &json!({"id": id, "dir": dir}));
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn start(app: &mut SolveApp) {
    if app.file_load.running.is_some() {
        return;
    }
    // A request for a tab that was closed has nowhere to land: answer it so it does not wait for
    // a tab number that will never match again and hold the queue.
    let mut at = 0;
    while at < app.file_load.queued.len() {
        let orphan = app.file_load.queued[at].tab.is_some_and(|tab| !app.docs.tab_open(tab));
        if orphan {
            if let Some(request) = app.file_load.queued.remove(at) {
                orphaned_request(app, request);
            }
        } else {
            at += 1;
        }
    }
    // The first that can start: one for a design in a tab not shown waits for it.
    let shown = app.docs.active_tab();
    let Some(next) = app.file_load.queued.iter().position(|r| r.tab.is_none_or(|tab| tab == shown)) else { return };
    let Some(request) = app.file_load.queued.remove(next) else { return };
    if crate::control::chat_turn_cancelled(request.chat_turn.as_deref()) {
        request.answer(Err("the agent chat turn ended".into()));
        return;
    }
    if reuse_open(app, &request) {
        return;
    }
    if request.design.as_deref().is_some_and(|d| !app.session.path.as_deref().is_some_and(|p| crate::documents::same_path(p, d))) {
        crate::operations::ended(app, &format!("file:{}", request.serial), Outcome::Failed("the design it was for is no longer shown".into()));
        request.answer(Err("the design it was for is no longer shown".into()));
        return;
    }
    if matches!(request.kind, Kind::Open | Kind::Recover { .. })
        && app.docs.count() >= crate::documents::MAX_OPEN
        && !crate::documents::active_is_blank(app)
    {
        let e = "too many open designs; close a tab before opening another";
        crate::operations::ended(app, &format!("file:{}", request.serial), Outcome::Failed(e.into()));
        app.set_status(e, true);
        if let Kind::Recover { id, dir } = &request.kind {
            reoffer(app, id, dir.as_deref());
        }
        request.answer(Err(e.into()));
        return;
    }
    let insert = request.insert();
    let before = if insert { crate::open_meshing::session_world(&app.session).bodies.iter().map(|b| b.name.clone()).collect() } else { Vec::new() };
    let update = request.reads_again();
    let mut scratch = if insert || update || request.kind == Kind::ExportDxf { app.session.scratch() } else { Session::default() };
    // A design opened or recovered here is run through the host's pre-command check too, and
    // evaluated under the user's validation mode.
    scratch.set_command_check(app.session.command_check());
    scratch.set_validation(app.preferences.validation);
    crate::print_face_ui::sync(&mut scratch, &app.preferences);
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
                ready: None,
                sketch: app.session.active_sketch,
            })
        }
        Err(e) => {
            let e = format!("could not start file operation: {e}");
            crate::operations::ended(app, &format!("file:{}", request.serial), Outcome::Failed(e.clone()));
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
    // The first that can start: one for a design in a tab not shown waits for it.
    let shown = app.docs.active_tab();
    let Some(next) = app.file_load.queued.iter().position(|r| r.tab.is_none_or(|tab| tab == shown)) else { return };
    let Some(request) = app.file_load.queued.remove(next) else { return };
    let insert = request.insert();
    let before = if insert { crate::open_meshing::session_world(&app.session).bodies.iter().map(|b| b.name.clone()).collect() } else { Vec::new() };
    let mut session = if insert { app.session.scratch() } else { Session::default() };
    session.set_command_check(app.session.command_check());
    session.set_validation(app.preferences.validation);
    crate::print_face_ui::sync(&mut session, &app.preferences);
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
        ready: None,
        sketch: app.session.active_sketch,
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
        let world = crate::open_meshing::session_world(session);
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
        let validation = session.validation();
        match scoped(scope.clone(), || watch(progress.clone(), || solidcraft_engine::OpenSteps::start(&params, validation))) {
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
    use solidcraft_engine::geom::Aabb3;
    use solidcraft_viewport::{Camera, Rgb, Scene, SceneMesh, StandardView};
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
    camera.fit(&solidcraft_engine::render::view_bounds(&bounds));
    scene.radius = bounds.diagonal();
    if stop() || !preview_work_fits(&scene, &camera) {
        return None;
    }
    preview_image(&scene, &camera)
}

#[cfg(not(target_arch = "wasm32"))]
fn saved_picture(preview: solidcraft_engine::io::DesignPreview) -> Option<egui::ColorImage> {
    use solidcore_record::Vec3;
    use solidcraft_engine::geom::{Aabb3, Mesh};
    use solidcraft_viewport::{Camera, Rgb, Scene, SceneMesh, StandardView};
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
    camera.fit(&solidcraft_engine::render::view_bounds(&bounds));
    scene.radius = bounds.diagonal();
    if !preview_work_fits(&scene, &camera) {
        return None;
    }
    preview_image(&scene, &camera)
}

/// The preview picture, drawn on the GPU by the renderer the app installs (none: no picture).
#[cfg(not(target_arch = "wasm32"))]
fn preview_image(scene: &solidcraft_viewport::Scene, camera: &solidcraft_viewport::Camera) -> Option<egui::ColorImage> {
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
fn preview_work_fits(scene: &solidcraft_viewport::Scene, camera: &solidcraft_viewport::Camera) -> bool {
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
    let world = crate::open_meshing::session_world(&session);
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
    if let Some(r) = app.file_load.running.as_mut()
        && r.ready.is_none()
    {
        r.ready = match r.rx.try_recv() {
            Ok(job) => Some(job),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("the file operation stopped; the current design is unchanged".into())),
        };
    }
    // A ready result that cannot land yet (its design's tab is not shown, or the person is busy)
    // leaves `running` for `waiting`: it stops holding the worker, so the next request starts
    // while it waits, and it lands the moment it can.
    if let Some(r) = app.file_load.running.as_ref()
        && r.ready.is_some()
    {
        let land = landing(app, r);
        match land {
            Land::Wait => {
                if let Some(running) = app.file_load.running.take() {
                    app.file_load.waiting.push(running);
                }
            }
            Land::Now => {
                if let Some(running) = app.file_load.running.take() {
                    land_now(app, running);
                }
            }
            Land::Again => {
                if let Some(running) = app.file_load.running.take() {
                    again(app, running);
                }
            }
            Land::Behind => {
                if let Some(running) = app.file_load.running.take() {
                    behind(app, running);
                }
            }
            Land::Gone => {
                if let Some(running) = app.file_load.running.take() {
                    gone(app, running);
                }
            }
        }
    }
    // Land each waiting result that can go now, oldest first.
    let mut at = 0;
    while at < app.file_load.waiting.len() {
        let land = landing(app, &app.file_load.waiting[at]);
        if land == Land::Wait {
            at += 1;
            continue;
        }
        let running = app.file_load.waiting.remove(at);
        match land {
            Land::Now => land_now(app, running),
            Land::Again => again(app, running),
            Land::Behind => behind(app, running),
            Land::Gone => gone(app, running),
            Land::Wait => {}
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

/// What a finished file operation does now.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Land {
    /// Taken up now, as it always was.
    Now,
    /// Kept until the person is done with what they are doing, or its design's tab is shown.
    Wait,
    /// Its design changed while it ran: run again for the design as it is now.
    Again,
    /// A design opened while the person is busy: in a new tab behind the one shown.
    Behind,
    /// Its design's tab was closed (or replaced): nothing is left to land in.
    Gone,
}

/// How many times an insert runs again for a design that keeps changing before it gives up.
const MAX_TRIES: u8 = 3;

/// The person isn't in the middle of something a design arriving would cut short: no dialog,
/// tool or sketch open, nothing of theirs computing or being saved, no "save changes?" asked.
fn quiet(app: &SolveApp, r: &Running) -> bool {
    app.dialog.is_none()
        && app.tool.is_none()
        && app.session.active_sketch.is_none_or(|sketch| r.sketch == Some(sketch))
        && app.applying.is_none()
        && app.deferred.is_none()
        && app.deferred_queue.is_empty()
        && app.delete_check.is_none()
        && app.pending_feature_edit.is_none()
        && app.saving.is_none()
        && app.file_picker.is_none()
        && app.control_job.is_none()
        && app.docs.closing.is_none()
        && app.docs.quitting.is_none()
        && app.docs.batch.is_none()
}

fn landing(app: &SolveApp, r: &Running) -> Land {
    let Some(job) = r.ready.as_ref() else { return Land::Wait };
    // As it always was: the design waited for it.
    if app.preferences.file_operations_wait || r.cancelled() || job.is_err() || r.request.kind == Kind::ExportDxf {
        return Land::Now;
    }
    // The tab list is being edited (a close prompt, a quit, Close Other/All Tabs): landing a
    // design behind would insert a slot and shift the indices those hold. Wait until it is over.
    if app.docs.closing.is_some() || app.docs.quitting.is_some() || app.docs.batch.is_some() {
        return Land::Wait;
    }
    // Its design's tab was closed (or replaced): there is nothing left to land in.
    if r.request.tab.is_some_and(|tab| !app.docs.tab_open(tab)) {
        return Land::Gone;
    }
    match r.request.kind {
        Kind::Open | Kind::Recover { .. } if quiet(app, r) => Land::Now,
        Kind::Open | Kind::Recover { .. } => Land::Behind,
        _ if r.request.tab.is_some_and(|tab| tab != app.docs.current_tab()) || !quiet(app, r) => Land::Wait,
        _ if r.matches(&app.session) || r.request.tries >= MAX_TRIES => Land::Now,
        _ => Land::Again,
    }
}

/// Run an insert (or Update Links) again for the design as it is now, first in line; its first
/// result is kept meanwhile, so the file it read is still cached.
fn again(app: &mut SolveApp, mut running: Running) {
    let mut request = std::mem::replace(&mut running.request, Request::new("", Kind::Open));
    request.tries += 1;
    if let Some(Ok(made)) = running.ready.take() {
        app.file_load.warm = Some((request.serial, Box::new(made.session)));
    }
    app.file_load.queued.push_front(request);
}

/// A design opened while the person is busy goes in a new tab behind the one shown.
fn behind(app: &mut SolveApp, mut running: Running) {
    let Some(Ok(made)) = running.ready.take() else { return };
    let request = &running.request;
    let key = format!("file:{}", request.serial);
    let uid = made.session.uid;
    match crate::documents::open_tab_behind(app, made.session) {
        Ok(()) => {
            crate::operations::ended(app, &key, Outcome::Done);
            if request.kind == Kind::Open {
                app.home.add_recent(&request.path);
                crate::documents::design_opened(app, &request.path);
            }
            // A recovered design takes its entry with it into the background tab: saved there
            // first, so the old entry can go without the design being lost to a crash.
            if let Kind::Recover { id, dir } = &request.kind {
                recovered_behind(app, uid, id, dir.as_deref());
            }
            let name = std::path::Path::new(&request.path).file_name().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
            app.set_status(solidcraft_ui_text::format("file_load.opened_behind", &[("name", &name)]), false);
            request.answer(Ok(made.result));
        }
        Err(e) => {
            crate::operations::ended(app, &key, Outcome::Failed(e.clone()));
            app.set_status(e.clone(), true);
            request.answer(Err(e));
        }
    }
    // The launch's reopen marker names this file until its open finishes. A background open
    // finishes here too, or the next launch reports a stuck reopen and skips the saved tabs.
    if request.kind == Kind::Open
        && let Some(dir) = &app.reopen_guard_dir
    {
        solidcraft_engine::recovery::reopen_finished(dir, &request.path);
    }
}

/// Answer a result whose design's tab was closed: nothing is left to land in.
fn gone(app: &mut SolveApp, running: Running) {
    app.file_load.drop_warm(running.request.serial);
    let e = "the design it was for was closed";
    crate::operations::ended(app, &format!("file:{}", running.request.serial), Outcome::Failed(e.into()));
    app.set_status(e, false);
    running.request.answer(Err(e.into()));
}

/// Answer a queued request whose design's tab was closed: it would wait for a number that never
/// matches again, holding the queue.
fn orphaned_request(app: &mut SolveApp, request: Request) {
    app.file_load.drop_warm(request.serial);
    let e = "the design it was for was closed";
    crate::operations::ended(app, &format!("file:{}", request.serial), Outcome::Failed(e.into()));
    app.set_status(e, false);
    request.answer(Err(e.into()));
}

/// Cancel a result that is in and waits to land (its row's X in the operations panel).
pub(crate) fn cancel_waiting(app: &mut SolveApp, serial: u64) {
    let Some(at) = app.file_load.waiting.iter().position(|r| r.request.serial == serial) else { return };
    let running = app.file_load.waiting.remove(at);
    app.file_load.drop_warm(serial);
    crate::operations::ended(app, &format!("file:{serial}"), Outcome::Cancelled);
    app.set_status("file operation cancelled", false);
    running.request.answer(Err("file operation cancelled".into()));
}

/// Take up a finished file operation now.
fn land_now(app: &mut SolveApp, mut running: Running) {
    let Some(job) = running.ready.take() else { return };
    if app.file_load.warm.as_ref().is_some_and(|(serial, _)| *serial == running.request.serial) {
        app.file_load.warm = None;
    }
    // Opened while the person kept working: it lands, whatever they changed in the design shown.
    let opens = !app.preferences.file_operations_wait && matches!(running.request.kind, Kind::Open | Kind::Recover { .. });
    let key = format!("file:{}", running.request.serial);
    let mut outcome = Outcome::Done;
    {
        let request = &running.request;
        match job {
            Ok(made) if request.kind == Kind::ExportDxf => {
                if !request.path.is_empty() {
                    app.set_status(solidcraft_ui_text::format("ui.saved_to", &[("path", &request.path)]), false);
                }
                request.answer(Ok(made.result));
            }
            _ if running.cancelled() => {
                outcome = Outcome::Cancelled;
                request.answer(Err("file operation cancelled".into()));
            }
            Ok(made) if request.reads_again() && running.matches(&app.session) => {
                let (id, _) = request.command();
                app.session.accept_computed(made.session);
                app.big_operation = true;
                if request.kind == Kind::UpdateLinks {
                    crate::linked_source::read_again(app, &request.options, &made.result);
                }
                let r = Ok(made.result);
                app.finish_command(id, &r);
                // The reload is its own undo step: the open card folds only what comes after it.
                if request.kind == Kind::ReloadCloud
                    && let Some(panel) = app.tree.cloud_panel.as_mut()
                {
                    panel.depth = app.session.undo.len();
                }
                let language = solidcraft_ui_text::Locale::for_language(&app.preferences.language);
                app.set_status(language.format("ui.command_done", &[("command", language.label(crate::command_label(id)))]), false);
                // A design it could not read is said as when that was all it was asked.
                if let Some(f) = r.as_ref().ok().and_then(|v| v["failed"].as_array()).and_then(|f| f.first()) {
                    let (name, why) = (f["name"].as_str().unwrap_or_default(), f["error"].as_str().unwrap_or_default());
                    app.set_status(format!("{name}: {why}"), true);
                }
                // What the file's reading left out is said too.
                if let Some(w) =
                    r.as_ref().ok().filter(|_| request.kind == Kind::ReloadCloud).and_then(|v| v["warnings"].as_array()).and_then(|w| w.first())
                {
                    app.set_status(w.as_str().unwrap_or_default().to_string(), false);
                }
                request.answer(r);
            }
            Ok(_) if reuse_open(app, request) => {}
            Ok(made) if opens || running.matches(&app.session) => 'open: {
                // Not meshed yet: framing it while it goes in must not mesh it here.
                app.open_meshing_next = true;
                crate::frame_watch::step(app, "before the open");
                let tabs = app.docs.count();
                if request.insert() {
                    app.session.accept_computed(made.session);
                    app.big_operation = true;
                } else {
                    // No tab left for it: `adopt` said why, and nothing else of the open happens.
                    if !crate::agent_chat::with_tool_request(app, request.chat_turn.as_ref(), |app| crate::documents::adopt(app, made.session)) {
                        app.open_meshing_next = false;
                        // A design to recover stays offered, as when its read fails.
                        if let Kind::Recover { id, dir } = &request.kind {
                            reoffer(app, id, dir.as_deref());
                        }
                        request.answer(Err(app.status.as_ref().map_or_else(String::new, |s| s.0.clone())));
                        break 'open;
                    }
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
                            solidcraft_ui_text::tr("document.opened_read_only").to_string()
                        } else if let Some(summary) = crate::timeline::import_status(app, &made.result) {
                            summary
                        } else if let Some(w) = warnings.filter(|w| !w.is_empty()) {
                            format!("imported with {} warning(s): {}", w.len(), w.first().and_then(Value::as_str).unwrap_or(""))
                        } else {
                            // Built here, outside the window's language scope: in the language
                            // chosen, as the other statuses of this file are.
                            let language = solidcraft_ui_text::Locale::for_language(&app.preferences.language);
                            // An opened design is named as its tab names it, an inserted one as
                            // its component is named.
                            let name = if request.insert() {
                                made.result.get("component_name").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| {
                                    std::path::Path::new(&request.path)
                                        .file_stem()
                                        .map_or_else(|| request.path.clone(), |n| n.to_string_lossy().into_owned())
                                })
                            } else {
                                language.given_name(&app.session.doc.name)
                            };
                            // A blank design is replaced in place; anything else gets a tab of its own.
                            let key = if request.insert() {
                                "file_load.inserted"
                            } else if app.docs.count() > tabs {
                                "file_load.opened_in_tab"
                            } else {
                                "file_load.opened"
                            };
                            language.format(key, &[("name", &name)])
                        },
                        false,
                    );
                    if !request.insert() {
                        app.say_newer_file();
                        app.say_signature(&made.result);
                    } else if request.note.is_none()
                        && let Some(note) = linked_signing_note(&request.path)
                    {
                        app.set_status(note, false);
                    }
                    if let Some(note) = &request.note {
                        app.set_status(note.clone(), false);
                    }
                }
                if request.kind == Kind::Open {
                    crate::documents::design_opened(app, &request.path);
                }
                request.answer(Ok(made.result));
            }
            Ok(_) => {
                let e = "the active design changed while loading; the file operation was discarded";
                outcome = Outcome::Failed(e.into());
                request.answer(Err(e.into()));
                app.set_status(e, true);
            }
            Err(e) => {
                // A real cancel took the `running.cancelled()` arm above; this is a failure.
                outcome = Outcome::Failed(e.clone());
                request.answer(Err(e.clone()));
                if request.reads_again() {
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
        // #5064: opened or not, this design is no longer a reopen the session could die on;
        // it comes off the marker the launch wrote before reopening anything.
        if request.kind == Kind::Open
            && let Some(dir) = &app.reopen_guard_dir
        {
            solidcraft_engine::recovery::reopen_finished(dir, &request.path);
        }
    }
    crate::operations::ended(app, &key, outcome);
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

/// The three steps the card lists for an open or an import (`cad`: a STEP or IGES file), the one
/// under way and how far it is. A STEP file is read, its parts checked and repaired one by one
/// (the kernel's `converting`), then the model is built and shown; a design is read, its
/// features rebuilt, then shown. `work` is the kernel's last report, which stays at its end
/// once the step is over; `features` is (built, total).
fn load_steps(
    cad: bool,
    phase: u8,
    work: Option<(&'static str, usize, usize)>,
    features: (usize, usize),
) -> ([&'static str; 3], usize, Option<String>) {
    let (done, total) = features;
    let labels = if cad {
        ["file_load.step_read", "file_load.step_repair", "file_load.step_build"]
    } else {
        ["file_load.step_read", "file_load.step_rebuild", "file_load.step_show"]
    };
    let count = |key: &str, done: usize, total: usize| {
        Some(solidcraft_ui_text::format(key, &[("done", &(done + 1).min(total).to_string()), ("total", &total.to_string())]))
    };
    let percent = |d: usize, t: usize| Some(solidcraft_ui_text::format("file_load.step_percent", &[("percent", &(d * 100 / t.max(1)).to_string())]));
    let (current, detail) = match (phase, work) {
        (1.., _) => (2, None),
        (_, Some(("reading", d, t))) if cad => (0, percent(d, t)),
        (_, Some((_, d, t))) if cad && d < t => (1, count("file_load.step_part", d, t)),
        (_, Some(_)) if cad => (2, None),
        _ if total == 0 => (0, None),
        _ if cad => (1, None),
        _ => (1, count("file_load.step_feature", done, total)),
    };
    (labels, current, detail)
}

pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    // The operations panel shows the file operation as a row with its progress and Cancel; the
    // card is the fallback where it cannot: Home, "Wait for files to open", the panel turned off,
    // or no room for it in the view. The panel is laid out after this decision, so the last frame's
    // `panel_drew` says whether it had room (true until the first frame: the default panel fits).
    let panel_shows = !app.home.open && app.preferences.operations_panel && app.operations.panel_drew;
    app.file_load.card_shown = app.preferences.file_operations_wait || !panel_shows;
    if !app.file_load.card_shown {
        return;
    }
    let Some(r) = app.file_load.running.as_ref().or_else(|| app.file_load.waiting.first()) else { return };
    let cancelled = r.cancelled();
    if cancelled && app.file_load.queued.is_empty() {
        return;
    }
    let name = match &r.request.kind {
        Kind::Recover { .. } => r.request.path.clone(),
        // The linked design being read again, once the worker is on one.
        Kind::UpdateLinks => match r.progress.item() {
            Some((name, i, n)) => solidcraft_ui_text::format(
                "file_load.link_item",
                &[("name", &name), ("done", &(i + 1).min(n).to_string()), ("total", &n.to_string())],
            ),
            None => app.session.doc.name.clone(),
        },
        Kind::ReloadCloud => app.session.doc.name.clone(),
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
    let id = egui::Id::unique("file_loading");
    let card_id = id.with("card");
    // Above the dialogs over the design; menus, tooltips and the resize edges stay above it.
    Container::bring_to_front(ctx, egui::LayerId::new(egui::Order::Middle, id));
    Container::bring_to_front(ctx, egui::LayerId::new(egui::Order::Middle, card_id));
    Container::new(ctx, Surface::InputShield).area(id).order(egui::Order::Middle).fixed_pos(cover.min).interactable(true).show(ctx, |ui| {
        ui.set_min_size(cover.size());
        ui.interact(cover, egui::Id::unique("file_loading_block"), egui::Sense::click_and_drag());
        let shield = Container::new(ctx, Surface::InputShield);
        shield.paint_background(ui.painter(), shield.frame(ctx, egui::Color32::from_black_alpha(70), crate::frame::Side::Left), cover);
    });
    // Anchored at its top: the card grows downwards (the saved preview) and Cancel stays put.
    let top = egui::pos2(centre.x, (centre.y - 120.0).max(cover.top() + 8.0));
    let title = if cancelled && export {
        solidcraft_ui_text::tr_label("dxf.cancelling")
    } else if cancelled {
        solidcraft_ui_text::tr("file_load.waiting")
    } else {
        solidcraft_ui_text::tr(match r.request.kind {
            Kind::Insert | Kind::InsertDesign => "file_load.inserting",
            Kind::Open => "file_load.opening",
            Kind::Recover { .. } => "file_load.recovering",
            Kind::ExportDxf => "dxf.exporting",
            Kind::UpdateLinks => "file_load.updating_links",
            Kind::ReloadCloud => "file_load.reloading_cloud",
        })
    };
    let icon = match r.request.kind {
        Kind::Insert | Kind::InsertDesign => "import",
        Kind::Open => "open",
        Kind::Recover { .. } => "restore",
        Kind::ExportDxf => "export",
        Kind::UpdateLinks => "update",
        Kind::ReloadCloud => "import",
    };
    let (closed, action) = ui_kit::Window::new(card_id, icon, title)
        .cards(((cover.width() - 16.0) / solidcraft_theme::Tokens::get().form_card_width).clamp(0.5, 1.3))
        .placed(egui::Align2::CENTER_TOP, top)
        .actions("", solidcraft_ui_text::tr("ui.cancel"), None)
        .show(ctx, true, |ui| {
            ui_kit::text(ui, &format!("{name}…"), ui_kit::Text::Report("name", false));
            if let Some(preview) = r.preview.as_ref().filter(|_| !cancelled) {
                let w = ui.available_width();
                ui.image((preview.id(), egui::vec2(w, w * 200.0 / 320.0)));
                let note = match r.built {
                    Some(built) => solidcraft_ui_text::format("file_load.built", &[("built", &built.to_string()), ("total", &total.to_string())]),
                    None => solidcraft_ui_text::tr("file_load.preview_note").to_string(),
                };
                ui_kit::text(ui, &note, ui_kit::Text::Note("preview"));
            }
            // An open or an import goes through three steps the card lists; the other operations
            // (and a cancelled one) say what they are doing in a line.
            let stepped = !cancelled && matches!(r.request.kind, Kind::Open | Kind::Insert | Kind::InsertDesign | Kind::Recover { .. });
            if stepped {
                let cad = matches!(r.request.kind, Kind::Open | Kind::Insert)
                    && (solidcraft_engine::io::is_step_path(&r.request.path) || solidcraft_engine::io::is_iges_path(&r.request.path));
                let (labels, current, detail) = load_steps(cad, phase, r.progress.work(), (done, total));
                let labels = labels.map(solidcraft_ui_text::tr);
                ui_kit::steps(ui, "file_load", &labels, current, detail.as_deref());
            } else {
                let status = if cancelled && export {
                    solidcraft_ui_text::tr_label("dxf.cancel_wait").to_string()
                } else if cancelled {
                    solidcraft_ui_text::tr("file_load.stopping").to_string()
                } else {
                    match (phase, work) {
                        (_, work) if r.request.kind == Kind::ExportDxf => crate::dialogs_view_dxf::progress_text(work),
                        (_, Some(("reading", d, t))) => {
                            solidcraft_ui_text::format("file_load.reading_percent", &[("percent", &(d * 100 / t.max(1)).to_string())])
                        }
                        (_, Some((_, d, t))) => {
                            solidcraft_ui_text::format("file_load.converting", &[("done", &(d + 1).to_string()), ("total", &t.to_string())])
                        }
                        (1, _) => solidcraft_ui_text::tr("file_load.preparing_view").to_string(),
                        (2, _) => solidcraft_ui_text::tr("file_load.preparing_thumbnail").to_string(),
                        _ if total > 0 => solidcraft_ui_text::format(
                            "file_load.rebuilding",
                            &[("done", &(done + 1).min(total).to_string()), ("total", &total.to_string())],
                        ),
                        _ => solidcraft_ui_text::tr("file_load.reading").to_string(),
                    }
                };
                ui_kit::steps(ui, "file_load", &[status.as_str()], 0, None);
            }
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
        for op in app.file_load.operations() {
            crate::operations::ended(app, &format!("file:{}", op.serial), Outcome::Cancelled);
        }
        app.file_load.cancel();
        app.set_status(if export { solidcraft_ui_text::tr_label("dxf.cancelled") } else { "file operation cancelled" }, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cancelling the queued retry forgets the session it read the file into: nothing runs again
    /// for a request that is gone.
    #[test]
    fn cancelling_the_queued_retry_forgets_its_cached_session() {
        let mut load = FileLoad::default();
        let mut request = Request::new("part.step", Kind::Insert);
        request.serial = 7;
        load.queued.push_back(request);
        load.warm = Some((7, Box::new(Session::default())));
        load.cancel_queued(0);
        assert!(load.warm.is_none(), "the cached session goes with its cancelled request");
    }

    /// A STEP file goes through its three steps in order as the kernel reports them, and stays
    /// on the last once the parts are done; a design's steps follow its features.
    #[test]
    fn the_card_follows_an_import_through_its_three_steps() {
        crate::i18n::install();
        let step = |phase, work, features| {
            let (_, current, detail) = load_steps(true, phase, work, features);
            (current, detail)
        };
        assert_eq!(step(0, None, (0, 0)), (0, None));
        assert_eq!(step(0, Some(("reading", 40, 80)), (0, 0)), (0, Some("50%".into())));
        assert_eq!(step(0, Some(("converting", 11, 140)), (0, 1)), (1, Some("Part 12 of 140".into())));
        assert_eq!(step(0, Some(("converting", 140, 140)), (0, 1)), (2, None));
        assert_eq!(step(1, Some(("converting", 140, 140)), (1, 1)), (2, None));
        let (labels, current, detail) = load_steps(false, 0, None, (2, 5));
        assert_eq!((labels[1], current, detail.as_deref()), ("file_load.step_rebuild", 1, Some("Feature 3 of 5")));
    }

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
            ready: None,
            sketch: None,
        });
    }

    /// An Open says which design it brought in, by the name its tab shows, and whether it took
    /// a tab of its own or replaced the blank design.
    #[test]
    fn an_open_names_the_design_and_says_when_it_got_a_new_tab() {
        let ctx = egui::Context::default();
        let mut app = SolveApp::new(Session::default(), Default::default());
        let open = |app: &mut SolveApp, name: &str| {
            let mut part = Session::default();
            part.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
            Arc::make_mut(&mut part.doc).name = name.into();
            part.path = Some(format!("{name}.solidcraft"));
            completed_open(app, Request::new(&format!("{name}.solidcraft"), Kind::Open), part);
            tick(app, &ctx);
            app.status.clone().map(|s| (s.0, s.2)).unwrap_or_default()
        };
        assert_eq!(open(&mut app, "plate"), ("Opened plate".into(), false), "the blank design is replaced in its tab");
        assert_eq!(app.docs.count(), 1);
        assert_eq!(open(&mut app, "post"), ("Opened post in a new tab".into(), false));
        assert_eq!(app.docs.count(), 2);
        // In the language chosen, though the status is set outside the window's language scope
        app.preferences.language = "de".into();
        assert_eq!(open(&mut app, "knob"), ("knob in neuem Tab geöffnet".into(), false));
        // No tab left (and no blank design to take): the open says so, and names nothing it did not open
        while app.docs.count() < crate::documents::MAX_OPEN {
            let mut saved = Session::default();
            saved.path = Some(format!("tab{}.solidcraft", app.docs.count()));
            crate::documents::open_tab(&mut app, saved).unwrap();
        }
        let mut request = Request::new("full.solidcraft", Kind::Open);
        let (tx, reply) = std::sync::mpsc::channel();
        request.reply = Some(tx);
        completed_open(&mut app, request, Session::default());
        tick(&mut app, &ctx);
        let (status, _, error) = app.status.clone().unwrap_or_default();
        assert!(error && status.contains(&crate::documents::MAX_OPEN.to_string()), "{status}");
        assert_eq!(app.docs.count(), crate::documents::MAX_OPEN);
        assert!(reply.try_recv().is_ok_and(|r| r["ok"] == json!(false)), "the caller hears it failed");
    }

    #[test]
    fn a_ply_is_a_point_cloud_unless_inserted_as_a_mesh() {
        let mut request = Request::new("scan.ply", Kind::Insert);
        assert_eq!(request.command().0, "pointcloud.insert");
        assert_eq!(Request::new("scan.ply", Kind::Open).command().0, "doc.open");
        request.mesh = true;
        assert_eq!(request.command().0, "file.insert_mesh");
    }

    /// A FreeCAD file comes in as its rebuilt features; with the preference off, as its solids in
    /// one Import feature, as before.
    #[test]
    fn freecad_history_preference_keeps_the_solids_only_import() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../io/src/native_cad/fixtures/bracket.FCStd");
        assert!(crate::prefs::Prefs::default().freecad_history, "on by default");
        for (on, types) in [(true, vec!["Sketch", "ExtrudeFeature", "Sketch", "ExtrudeFeature", "FilletFeature"]), (false, vec!["BaseFeature"])] {
            let mut app = SolveApp::new(Session::default(), Default::default());
            app.preferences.freecad_history = on;
            app.insert_path(path);
            let got: Vec<&str> = app.session.doc.features.iter().map(|f| f.kind.type_name()).collect();
            assert_eq!(got, types, "preference {on}");
        }
    }

    #[test]
    fn the_reuse_import_default_is_added_to_import_opens_only() {
        let mut app = SolveApp::new(Session::default(), Default::default());
        for (path, expected) in [("part.step", Some(true)), ("part.IGS", Some(true)), ("design.sc", None), ("old.solidcraft", None)] {
            let mut request = Request::new(path, Kind::Open);
            default_reuse_import(&app, &mut request);
            assert_eq!(request.options.get("reuse_import").and_then(Value::as_bool), expected, "{path}");
        }
        app.preferences.reuse_imports = false;
        let mut request = Request::new("part.step", Kind::Open);
        default_reuse_import(&app, &mut request);
        assert_eq!(request.options.get("reuse_import").and_then(Value::as_bool), Some(false), "the old value restores the old behaviour");
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
    fn automatic_verify_still_reuses_an_open_design() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let path = h.dir.join("Plate.solidcraft");
        h.app.run("file.save_as", json!({"path": path.to_string_lossy()})).unwrap();
        h.app.preferences.verify_opened_designs = true;
        let tabs = h.app.docs.count();
        let mut request = Request::new(&path.to_string_lossy(), Kind::Open);
        request.options.insert("verify".into(), json!(true));
        request.auto_verify = true;
        begin_request(&mut h.app, request).unwrap();
        assert_eq!(h.app.docs.count(), tabs, "the open design is reused, not opened again");
        assert!(h.app.verifying.is_some(), "the automatic check runs separately");
    }

    #[test]
    fn finished_open_reuses_existing_dirty_tab_and_scope_still_applies() {
        let mut app = SolveApp::new(Session::default(), Default::default());
        app.session.path = Some("existing.solidcraft".into());
        app.session.execute("solid.box", &json!({"length": 2, "width": 3, "height": 4})).unwrap();
        let dirty = app.session.doc.clone();
        let undo = app.session.undo.len();
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
        assert_eq!(app.session.undo.len(), undo);
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
            solidcraft_engine::io::Geom2::Line(solidcore_record::Vec2::ZERO, solidcore_record::Vec2::new(10.0, 0.0)),
            solidcraft_engine::io::Geom2::Arc {
                c: solidcore_record::Vec2::ZERO,
                a: solidcore_record::Vec2::new(5.0, 0.0),
                b: solidcore_record::Vec2::new(0.0, 5.0),
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
            assert_eq!(h.app.session.model.state().sketch(id).unwrap().plane, solidcore_record::Plane::XY);
            assert_eq!(h.app.session.doc.sketch(id).unwrap().curves.len(), 2);
            h.app.cam.set_view(solidcraft_viewport::StandardView::Top);
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
        // The card's Cancel (Preferences > Wait for files).
        h.app.preferences.file_operations_wait = true;
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
        h.frames(3);
        let base = h.app.session.doc.clone();
        let undo = h.app.session.undo.len();
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
            ready: None,
            sketch: None,
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
        assert_eq!(h.app.session.undo.len(), undo);
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
            ready: None,
            sketch: None,
        });
        tx.send(Ok(Made { session: other, result: json!({"visible":12}), thumbnail: false })).unwrap();
        h.frames(2);
        assert_eq!(reply_rx.try_recv().unwrap()["result"]["visible"], 12);
        assert!(Arc::ptr_eq(&base, &h.app.session.doc));
        assert_eq!(h.app.session.undo.len(), undo);
        assert_eq!(h.app.cam, cam);
    }

    #[test]
    fn opening_result_and_preview_cannot_overwrite_a_newer_design() {
        for (insert, change_document) in [(false, true), (true, true), (false, false)] {
            let mut app = SolveApp::new(Session::default(), Default::default());
            // As it always did with the design waiting for the file: a newer design isn't replaced.
            app.preferences.file_operations_wait = true;
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
                ready: None,
                sketch: None,
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
                corner_normals: Default::default(),
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
                corner_normals: Default::default(),
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
            ready: None,
            sketch: None,
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
                ready: None,
                sketch: None,
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
        let bodies = |app: &SolveApp| app.session.world_state().unwrap().bodies.len();
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
        assert_eq!(app.status.as_ref().map(|s| s.0.as_str()), Some("Inserted boxes"), "named as its component is");
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
        let names = |s: &Session| s.world_state().unwrap().bodies.iter().map(|b| b.name.clone()).collect::<Vec<_>>();

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
        assert_eq!(app.status.as_ref().map(|s| s.0.as_str()), Some("Opened staged"), "named as its tab is");
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

    /// The app asks a STEP-family open or insert for a `Group #N` history folder only while "Fold
    /// the History bar into groups" is on (#8885); a caller's own `folder` stays, and other files
    /// are left alone.
    #[test]
    fn a_step_open_asks_for_a_history_folder_as_the_bar_groups() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        let asked = |app: &SolveApp, path: &str, folder: Option<bool>| {
            let mut options = serde_json::Map::new();
            if let Some(folder) = folder {
                options.insert("folder".into(), Value::Bool(folder));
            }
            step_folder_option(app, path, &mut options);
            options.get("folder").cloned()
        };
        assert!(!app.preferences.history_bar_groups, "off by default");
        assert_eq!(asked(&app, "a.step", None), Some(Value::Bool(false)), "no folder while the bar's groups are off");
        app.preferences.history_bar_groups = true;
        assert_eq!(asked(&app, "a.stp", None), Some(Value::Bool(true)), "a folder while they are on");
        assert_eq!(asked(&app, "a.step", Some(false)), Some(Value::Bool(false)), "the caller's choice stays");
        assert_eq!(asked(&app, "a.stl", None), None, "a mesh has no folders");
    }
}
