//! The SolidCraft engine.
//!
//! Every user-visible action is a command with a stable, dotted, lower-case id
//! (`solid.extrude`, `sketch.create`, `solid.fillet`, `timeline.rollback`). Commands take JSON
//! parameters and never open dialogs; the UI, the command palette, scripts, the CLI and the
//! control channel all run the same commands through [`Session::execute`].
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod agent_setup;
pub mod batch_params;
pub mod batch_preflight;
pub mod capture;
pub mod cmd;
pub mod control_auth;
pub mod control_pending;
pub mod cosmetic_threads;
pub mod crash;
pub mod easyprint;
pub mod frames;
pub mod fuzz;
pub mod licences;
pub mod occurrences;
pub mod params;
pub mod product_scene;
pub mod recovery;
pub mod render_jobs;
pub mod render_lights;
pub mod sample;
pub mod schema;
#[cfg(not(target_arch = "wasm32"))]
pub mod slicer_files;
mod slicer_icon;
pub mod slicers;
pub mod snap;
pub mod status_details;
pub mod status_items;
pub mod timing;
pub mod view;

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcraft_doc::{Document, Model, ModelState};
use solidcraft_geom::Vec3;

pub use cmd::settings::{MAX_PROFILE_NAME, profile_name};
pub use cmd::{
    Addition, CommandInfo, CommandSpec, addition, auto_operation, command_specs, dimension_roles, extrude_reach, find_command, resolve_design_links,
    shown_scope, standard_view,
};
pub use solidcraft_doc as doc;
pub use solidcraft_doc::Visibility;
pub use solidcraft_geom as geom;
pub use solidcraft_io as io;
pub use solidcraft_kernel as kernel;
pub use solidcraft_profile as profile;
pub use solidcraft_render as render;
pub use solidcraft_sketch as sketch;

/// Longest accepted request line on the JSON-lines channels (MCP over stdio and the control
/// server over loopback), so both refuse runaway input at the same size.
pub const MAX_LINE: usize = 16 << 20;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("unknown command `{0}`{hint}", hint = cmd::did_you_mean(.0))]
    UnknownCommand(String),
    #[error("`{0}` is not available right now: {1}")]
    Disabled(String, String),
    #[error("invalid parameters for `{cmd}`: {msg}")]
    BadParams { cmd: String, msg: String },
    #[error("{0}")]
    Other(String),
    /// The host's pre-command check ([`Session::set_command_check`]) refused the command; the
    /// message is the reason it gave.
    #[error("{reason}")]
    Refused { cmd: String, reason: String },
    /// A command panicked; the document was kept as it was (a bug: please report it).
    #[error("internal error in `{0}` (the design was kept as it was): {1}")]
    Internal(String, String),
}

impl From<solidcraft_doc::DocError> for EngineError {
    fn from(e: solidcraft_doc::DocError) -> Self {
        EngineError::Other(e.to_string())
    }
}
impl From<solidcraft_sketch::SketchError> for EngineError {
    fn from(e: solidcraft_sketch::SketchError) -> Self {
        EngineError::Other(e.to_string())
    }
}
impl From<solidcraft_kernel::KernelError> for EngineError {
    fn from(e: solidcraft_kernel::KernelError) -> Self {
        EngineError::Other(e.to_string())
    }
}
impl From<solidcraft_io::IoError> for EngineError {
    fn from(e: solidcraft_io::IoError) -> Self {
        EngineError::Other(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, EngineError>;

/// Something selected in the model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Sel {
    /// An exact placement, including its containing occurrence placements.
    Occurrence {
        path: Vec<u64>,
    },
    Body {
        name: String,
    },
    /// An edge, identified by a point on it.
    Edge {
        body: String,
        index: usize,
        point: Vec3,
    },
    Face {
        body: String,
        index: usize,
        point: Vec3,
    },
    Feature {
        id: u64,
    },
    /// A sketch curve or point (by id) of the active sketch. A curve picked into a dialog also
    /// names its `sketch`: curve ids repeat from one sketch to the next (#3726).
    SketchCurve {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sketch: Option<u64>,
    },
    SketchPoint {
        id: String,
    },
    /// The associative midpoint of a line in the active sketch. Selection alone creates no geometry.
    SketchMidpoint {
        id: String,
    },
    /// Geometry of a different sketch, retained for projection preselection.
    ExternalSketchCurve {
        sketch: u64,
        id: String,
    },
    ExternalSketchPoint {
        sketch: u64,
        id: String,
    },
    /// A constraint or dimension (by id) of the active sketch.
    SketchConstraint {
        id: String,
    },
    /// A closed profile of a sketch (by index).
    Profile {
        sketch: u64,
        index: usize,
    },
    /// An origin plane (XY, XZ, YZ) or a construction plane (by feature name).
    Plane {
        name: String,
    },
    /// A construction point, by feature name.
    ConstructionPoint {
        name: String,
    },
    /// An origin axis (X, Y, Z) or a construction axis (by feature name).
    Axis {
        name: String,
    },
    /// A body vertex at a point.
    Vertex {
        body: String,
        point: Vec3,
    },
}

impl Sel {
    /// The body this item names, when it names one: the body itself, or the body a face, edge
    /// or vertex belongs to. Session state that names a body has to follow its renames (#9737).
    pub(crate) fn body_name(&self) -> Option<&str> {
        match self {
            Sel::Body { name } => Some(name),
            Sel::Face { body, .. } | Sel::Edge { body, .. } | Sel::Vertex { body, .. } => Some(body),
            _ => None,
        }
    }
}

/// Whether the document differs from the one its file holds, as last worked out for this pair
/// of documents at this revision. The window asks every frame for every tab (the tab strip, the
/// autosave), and a large design compared field by field cost more than the rest of an idle
/// frame. Holding both documents keeps their contents fixed and their addresses unused by
/// another document while the answer stands: shared, a document can't be edited in place (an
/// edit copies it to a new address), and [`Session::doc_mut`] lets the answer go first so an
/// edit stays in place.
struct DirtyAnswer {
    revision: u64,
    doc: Arc<Document>,
    saved: Arc<Document>,
    differs: bool,
}

/// A body rename between two designs (see `Session::body_renames`).
#[derive(Clone)]
struct BodyRename {
    before: std::sync::Weak<Document>,
    after: std::sync::Weak<Document>,
    old: String,
    new: String,
    names_only: bool,
}

/// Bodies, component placements or timeline features copied in one design, for pasting into another:
/// that design's model as it was when they were copied,
/// so the copy is what was copied, whatever happens to that design (or its tab) after.
#[derive(Clone, Debug)]
pub struct ForeignCopy {
    /// The design's name (it names what the paste adds).
    pub design: String,
    /// Timeline features, in their original order (`timeline.paste` with `from_design`).
    pub features: Vec<solidcraft_doc::Feature>,
    /// Its placed model: what its Browser and view showed.
    pub world: Arc<solidcraft_doc::ModelState>,
    /// The bodies copied, as its Browser names them (a body seen in another placement under the
    /// name it has there).
    pub bodies: Vec<String>,
    /// The name each of `bodies` has in its component (without the placement a body seen in
    /// another placement is named after): the name it pastes as.
    pub names: Vec<String>,
    /// The component placements copied, by path.
    pub placements: Vec<Vec<u64>>,
    /// Each of `placements`: the component placed, the occurrence and where it is in the world.
    pub placed: Vec<(u64, u64, solidcraft_doc::Mat)>,
    /// The design, its evaluated model and its placements moved but not yet applied: what a
    /// pasted component's history is copied from.
    pub doc: Arc<Document>,
    pub model: Model,
    pub pending_moves: std::collections::BTreeMap<u64, solidcraft_doc::Mat>,
}

impl ForeignCopy {
    /// What `session` copies when `bodies` and `placements` are copied in it now.
    pub fn of(session: &Session, bodies: Vec<String>, placements: Vec<Vec<u64>>) -> ForeignCopy {
        let frames = session.placed_document().body_frames_with_source(&session.model.state());
        let names = bodies.iter().map(|b| frames.iter().find(|f| f.0 == *b).map_or_else(|| b.clone(), |f| f.1.clone())).collect();
        let placed = placements.iter().filter_map(|p| session.occurrence_placement(p).ok()).map(|p| (p.component, p.occurrence, p.world)).collect();
        ForeignCopy {
            design: session.doc.name.clone(),
            features: Vec::new(),
            world: session.world_state(),
            bodies,
            names,
            placements,
            placed,
            doc: session.doc.clone(),
            model: session.model.clone(),
            pending_moves: session.pending_moves.clone(),
        }
    }

    /// Timeline Copy's snapshot. No placed body geometry is needed to paste sketches.
    pub fn of_features(session: &Session) -> ForeignCopy {
        ForeignCopy {
            design: session.doc.name.clone(),
            features: session.clipboard.clone(),
            world: session.model.state(),
            bodies: Vec::new(),
            names: Vec::new(),
            placements: Vec::new(),
            placed: Vec::new(),
            doc: session.doc.clone(),
            model: session.model.clone(),
            pending_moves: session.pending_moves.clone(),
        }
    }
}

/// One undo step.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub label: String,
    pub doc: Arc<Document>,
    pub active_sketch: Option<u64>,
    pub pending_moves: std::collections::BTreeMap<u64, solidcraft_doc::Mat>,
    /// What was hidden or shown then (`browser.visibility`). It is saved with the design, so
    /// hiding a body is undone and redone like any other change.
    pub visibility: Visibility,
}

/// Where the open sketch was entered (`sketch.create`, `sketch.edit`): the design before it and
/// the undo history then, so `sketch.cancel` can tell the entry still describes the history.
#[derive(Clone)]
pub(crate) struct SketchEntry {
    pub(crate) sketch: u64,
    pub(crate) doc: Arc<Document>,
    pub(crate) undo_len: usize,
    pub(crate) undo_top: Option<Arc<Document>>,
}

/// A copy of a session that evaluates commands ahead of it ([`Session::ahead`]).
pub struct Ahead(Session);

impl Ahead {
    /// What the evaluation computes, kept for the run that follows here whatever the evaluation
    /// cache drops meanwhile: hand it to [`solidcraft_doc::Model::keep`] for that run.
    pub fn kept(&self) -> Arc<solidcraft_doc::KeptResults> {
        self.0.model.kept().unwrap_or_default()
    }

    /// Run `commands` (their results stay in the shared evaluation cache, and in [`Ahead::kept`]),
    /// then make the display meshes and plans of the bodies they end with, so the window draws,
    /// fits and picks without tessellating anything. What they evaluated comes back for
    /// [`Session::prime`], so running them for real evaluates nothing again: the shared cache
    /// drops results past its budget, and a large design's states are past it.
    pub fn evaluate(self, commands: &[(String, Value)]) -> Result<Evaluated> {
        self.evaluate_counting(commands, &std::sync::atomic::AtomicUsize::new(0))
    }

    /// [`Ahead::evaluate`], setting `step` to the number of the command running (from 1), so a
    /// front end can say how far a batch has got.
    pub fn evaluate_counting(self, commands: &[(String, Value)], step: &std::sync::atomic::AtomicUsize) -> Result<Evaluated> {
        let (after, _) = self.0.preview_in_place(commands, Some(step))?;
        after.world_state()?.warm_display();
        Ok(Evaluated(after.model))
    }

    /// Run `commands` one after another, each reply or error in turn (what they evaluate stays
    /// in the shared evaluation cache): the checks a front end makes before a command, such as
    /// what a delete takes with it (#2678), computed off its window thread.
    pub fn replies(mut self, commands: &[(String, Value)]) -> Vec<Result<Value>> {
        commands.iter().map(|(id, params)| self.0.execute(id, params)).collect()
    }
}

/// The model a session copy evaluated ahead ([`Ahead::evaluate`]).
#[derive(Clone, Debug)]
pub struct Evaluated(solidcraft_doc::Model);

/// What some commands would make, evaluated on a scratch copy of a session
/// ([`Session::preview`]). The session itself is not changed.
#[derive(Clone, Debug)]
pub struct Preview {
    /// The model now.
    pub before: Arc<ModelState>,
    /// The model after the commands.
    pub after: Arc<ModelState>,
    /// What each command replied (as `execute` returns it).
    pub replies: Vec<Value>,
}

/// Prepared world geometry depends on design/model identity and placement context, not
/// selection or the view revision. Retained Arcs prevent pointer reuse after a design closes.
#[derive(Clone)]
struct WorldCache {
    doc: Arc<Document>,
    model: Arc<solidcraft_doc::ModelState>,
    pending_moves: std::collections::BTreeMap<u64, solidcraft_doc::Mat>,
    active_component: u64,
    active_occurrence_path: Option<Vec<u64>>,
    /// The placed model, or why it could not be placed (kept too, so a model that cannot be
    /// placed is not placed again on every read).
    world: std::result::Result<Arc<solidcraft_doc::ModelState>, String>,
    /// The moved components' bodies as placed for `world`, reused by the next world state.
    placed: Arc<std::sync::Mutex<solidcraft_doc::PlacedBodies>>,
    /// Every placed body's world name, own name and frame for `world` ([`Session::body_frames`]),
    /// worked out when a command first maps a body name.
    frames: Arc<std::sync::OnceLock<BodyFrames>>,
}

/// Every placed body's world name, its component's own name for it, and its frame.
pub(crate) type BodyFrames = Arc<Vec<(String, String, solidcraft_doc::Mat)>>;

impl WorldCache {
    fn matches(&self, session: &Session, model: &Arc<solidcraft_doc::ModelState>) -> bool {
        Arc::ptr_eq(&self.doc, &session.doc)
            && Arc::ptr_eq(&self.model, model)
            && self.pending_moves == session.pending_moves
            && self.active_component == session.active_component
            && self.active_occurrence_path == session.active_occurrence_path
    }
}

/// Source of [`Session::uid`].
static NEXT_UID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Commands evaluated in isolation, ready for an atomic handoff to their source session.
pub struct ComputedCommands {
    source: Session,
    computed: Session,
    steps: Vec<ComputedCommand>,
    editor_original: Option<Arc<Document>>,
}

struct ComputedCommand {
    id: String,
    params: Value,
    before: Arc<Document>,
    after: Arc<Document>,
    changed: bool,
    reply: Value,
}

impl ComputedCommands {
    /// Whether the source design and authoring context still match this result.
    pub fn is_current(&self, session: &Session) -> bool {
        let source = &self.source;
        session.same_document_context(source) && session.selection == source.selection
    }

    /// Prepare display tessellation before returning the transaction to the window; a model that
    /// cannot be placed is the error.
    pub fn warm_meshes(&self, keep_going: impl Fn() -> bool + Sync) -> Result<()> {
        self.warm_display(solidcraft_doc::display_quality::DisplayQuality::Normal, keep_going)
    }

    /// Prepare both the reference picking mesh and the GPU plan/highlight mesh at the
    /// selected quality. Warming only the reference mesh leaves cold planning on the UI.
    pub fn warm_display(&self, quality: solidcraft_doc::display_quality::DisplayQuality, keep_going: impl Fn() -> bool + Sync) -> Result<()> {
        self.computed.world_state()?.warm_display_at(quality, keep_going);
        Ok(())
    }
}

/// A check an app that embeds the engine runs before every command ([`Session::set_command_check`]):
/// it is shown the command's id, its parameters and the session (read only), and returns `Ok` to
/// let the command run or `Err(reason)` to refuse it.
pub type CommandCheck = Arc<dyn Fn(&str, &Value, &Session) -> std::result::Result<(), String> + Send + Sync>;

/// An editing session on one design.
/// Starts an upload of a file (its name, its bytes) to EasyPrint and returns at once; the host
/// reports how it went and opens the answer itself. `Err`: it couldn't start.
pub type EasyPrintUpload = Arc<dyn Fn(&str, Vec<u8>) -> std::result::Result<(), String> + Send + Sync>;

/// See [`Session::made_bodies`].
type MadeBodies = std::collections::HashMap<u64, (std::sync::Weak<ModelState>, Vec<String>)>;

pub struct Session {
    pub doc: Arc<Document>,
    pub model: Model,
    pub undo: Vec<Snapshot>,
    pub redo: Vec<Snapshot>,
    /// Sketch feature being edited (sketch mode).
    pub active_sketch: Option<u64>,
    /// The design as it was when the open sketch was entered, for Cancel Sketch.
    pub(crate) sketch_entry: Option<SketchEntry>,
    /// Sketch temporarily shown while editing a feature that consumes it. View state only.
    pub feature_edit_sketch: Option<u64>,
    pub selection: Vec<Sel>,
    pub path: Option<String>,
    /// Tells this document from every other one opened or started in this process: new for each
    /// `Session::new` (so for each New, Open and Recover), the same for the life of the document.
    /// Not saved.
    pub uid: u64,
    /// `path`'s size and modification time when it was opened or last saved: a Save to it
    /// finding them changed would replace another program's or computer's work (#5467).
    pub disk: Option<solidcraft_io::design_lock::FileStamp>,
    /// Opened read-only (`doc.open {read_only}`, the "open elsewhere" notice): saving to its own
    /// file is refused; Save As to another file is how its changes are kept.
    pub read_only: bool,
    /// Whether the design's name is one its user chose for it (`file.new {name}`) rather than
    /// one it borrowed — from a file's name, a sample's name, or a file that stored another
    /// design's name. A borrowed name follows Save As to the new file's name (#4907); a name
    /// the user chose is kept.
    pub user_named: bool,
    /// One-shot display-only notification during doc.open, before authoritative evaluation.
    /// Interactive loaders may show it provisionally; it never becomes model state.
    pub open_preview: Option<Box<dyn Fn(solidcraft_io::DesignPreview) + Send + Sync>>,
    saved: Arc<Document>,
    /// Messages for the command line / log.
    pub log: Vec<String>,
    /// Increments on every document change (views use it to refresh).
    pub revision: u64,
    /// Revision increments caused only by visibility. Comparing both counters proves that
    /// every intervening increment left an editor's document and model untouched.
    visibility_revision: u64,
    /// [`Document::browser_stamp`] when the browser's groups and order were last checked.
    browser_stamp: [u64; 6],
    /// This open design's id for programs that follow it (`document.revision`, #6646): kept
    /// while it stays open, through saves and renames; another for each design started or opened.
    pub document_id: String,
    /// The design as `document.revision` last saw it, and the revision it gave it.
    pub(crate) followed: Option<cmd::inspect::Followed>,
    /// Project a face's edges into a sketch created on it (the default; `sketch.auto_project`).
    pub auto_project: bool,
    /// How calls that break a command's parameter contract are treated (off by default). It
    /// belongs to the session, not the design, so it survives opening and starting designs.
    pub param_check: schema::ParamCheck,
    /// How the host uploads a model to EasyPrint (`file.open_in_easyprint`); `None`: it can't. It
    /// belongs to the session like `param_check`, so it survives opening and starting designs.
    pub easyprint_upload: Option<EasyPrintUpload>,
    /// After a live frame: its sketch, the document it left and the revision. Its links were
    /// brought up to date by that frame, so the gesture's next frame needn't project them again
    /// while nothing else has run (#3222).
    pub(crate) live_links: Option<(u64, Arc<Document>, u64)>,
    /// The component new sketches and features go into (0 = the root).
    pub active_component: u64,
    /// The placement used when authoring shared component geometry.
    pub active_occurrence_path: Option<Vec<u64>>,
    /// Occurrence moves not captured yet (Capture Position keeps them, a recompute drops them).
    pub pending_moves: std::collections::BTreeMap<u64, solidcraft_doc::Mat>,
    /// The placements the exploded view (`explode.show`) put in `pending_moves` to be shown:
    /// display only. An entry of `pending_moves` still equal to its one here is the exploded view's,
    /// not a move of the design's, so the placements the design reports and Capture Position
    /// keeps leave it out ([`Session::uncaptured_moves`]).
    pub exploded: std::collections::BTreeMap<u64, solidcraft_doc::Mat>,
    world_cache: std::sync::Mutex<Option<WorldCache>>,
    /// The design the joints were last solved for, as the solve left it, and the model it read
    /// (see [`Session::solve_joints`]).
    joints_settled: Option<(Arc<Document>, std::sync::Weak<solidcraft_doc::ModelState>)>,
    occurrence_visibility_cache: std::sync::Mutex<Option<occurrences::VisibilityCache>>,
    /// The hidden joints and folders of the view, as sets: the browser asks about every joint
    /// of the design on every frame (#3892), and an answer that walked the hidden lists made a
    /// frame cost grow with the size of the design.
    visibility_index_cache: std::sync::Mutex<Option<occurrences::VisibilityIndex>>,
    /// The bodies each feature made, by name, with the state they were read from
    /// ([`Session::pin_body_names`]).
    made_bodies: MadeBodies,
    removed_later_cache: std::sync::Mutex<Option<occurrences::RemovedLaterCache>>,
    occurrence_bodies_cache: std::sync::Mutex<Option<occurrences::OccurrenceBodiesCache>>,
    placement_index_cache: std::sync::Mutex<Option<occurrences::PlacementIndex>>,
    /// The last answer of [`Session::is_dirty`] on the document (see [`DirtyAnswer`]).
    dirty_cache: std::sync::Mutex<Option<DirtyAnswer>>,
    /// Pairs `inspect.clearance` measured, by the bodies' geometry where they stood (shared with
    /// scratch copies): a body that did not change or move is not measured again.
    clearance_cache: std::sync::Arc<std::sync::Mutex<cmd::clearance::PairCache>>,
    /// Recent body renames: the design before and after each, and the names, so undo and redo
    /// follow them without evaluating either.
    body_renames: Vec<BodyRename>,
    /// Bodies' contact meshes (shared with scratch copies): a joint driven again meshes no body
    /// that did not change.
    contact_meshes: std::sync::Arc<std::sync::Mutex<cmd::motion::contact_sweep::ContactMeshes>>,
    /// Features copied with `timeline.copy` (pasted by `timeline.paste`).
    pub clipboard: Vec<solidcraft_doc::Feature>,
    /// Sketch geometry copied with `sketch.copy` (pasted by `sketch.paste`).
    pub sketch_clipboard: Option<cmd::sketch_clipboard::SketchClip>,
    /// Resolved appearance copied by appearance.copy; transient, outside design/history.
    pub appearance_clipboard: Option<solidcraft_doc::appearance::Look>,
    /// What was copied in another design, to paste here (`component.paste_from_design`). The
    /// host puts it here; each design's own copies stay in its own clipboard.
    pub foreign_copy: Option<Arc<ForeignCopy>>,
    /// Section Analysis: a view cut by a plane (origin, unit normal; the normal side is hidden).
    pub section: Option<(Vec3, Vec3)>,
    /// Whether the section's cut faces are hatched (`inspect.section`'s `hatch`; on by default).
    pub section_hatch: bool,
    /// Component identification colours are enabled when present; view state only.
    pub colored_components: Option<()>,
    /// Surface analysis shading the model (zebra, draft, curvature map).
    pub analysis: Option<SurfaceAnalysis>,
    /// The seams `inspect.continuity` last measured, for the viewport to shade. View state: not
    /// part of the design and not an undo step.
    pub continuity: Vec<ContinuitySeam>,
    /// The faces Isocurve Analysis shows the isoparametric curves of, for the viewport to draw
    /// ([`Session::isocurve_lines`]). View state: not part of the design and not an undo step.
    pub isocurves: Vec<IsoPick>,
    /// The curvature combs `inspect.curvature_comb` shows, for the viewport to draw; `None` when
    /// the analysis is off. View state: not part of the design and not an undo step.
    pub comb: Option<CombView>,
    /// Oldest active slot first; replacing an effect retains its row position.
    status_order: Vec<status_items::StatusId>,
    /// Successful source activations, independently of row order and sharing policy.
    status_activation: Vec<status_items::StatusId>,
    single_surface_analysis: bool,
    latest_analysis: Option<status_items::StatusId>,
    /// What is hidden or shown in the view (`browser.visibility`): saved with the design and an
    /// undo step of its own ([`Snapshot::visibility`]). A design brings its saved visibility when
    /// it opens (`Session::new`).
    pub visibility: Visibility,
    /// The camera `view.apply` asked the view to use (view state, not saved). The desktop app
    /// moves its viewport there and takes it; headless renders keep using it until a standard
    /// view is chosen.
    pub camera: Option<render::Camera>,
    /// Host-owned viewport metadata, never saved or included in history.
    render_viewport: Option<Arc<render::ExecutorDescriptor>>,
    render_jobs: render_jobs::Jobs,
    /// The visibility the design file holds (what `is_dirty` compares with).
    saved_visibility: Visibility,
    /// States to go back to (`edit.checkpoint`, `edit.restore_checkpoint`): how batches of
    /// commands are undone however long the undo history is.
    pub(crate) checkpoints: Vec<Checkpoint>,
    next_checkpoint: u64,
    /// How many times [`Session::replace_design`] gave this session another design. A command
    /// that did is no edit of the design before it, so [`Session::execute`] never takes it for a
    /// change of render intent or part properties alone.
    replacements: u64,
    /// The host's pre-command check (none by default: every command may run). It belongs to the
    /// host, not the design: opening or starting a design keeps it, and scratch copies carry it.
    command_check: Option<CommandCheck>,
    /// Record this session's top-level commands in the crash journal (the app's own design;
    /// never previews or other scratch sessions). See [`crash`].
    pub crash_journal: bool,
    /// Full model evaluations (`refresh`), for tests.
    #[cfg(test)]
    pub(crate) evaluations: usize,
    /// Resolved associative trace inputs, independent of the document revision and view state.
    pub(crate) motion_traces: cmd::motion::associative::Cache,
}

/// A sketch changed by one frame of an interactive gesture (`live: true`).
#[derive(Clone, Debug)]
pub struct LiveEdit {
    pub sketch: u64,
    /// What the frame's solve found: a full solve's report, or, when the sketch was solved
    /// without a rank analysis, its last report's degrees of freedom and determined flags (a
    /// drag does not change them; the evaluation at the gesture's end brings them up to date).
    pub report: solidcraft_sketch::SolveReport,
}

/// The design, active sketch, pending positions and undo history at an `edit.checkpoint`.
#[derive(Clone, Debug)]
pub(crate) struct Checkpoint {
    pub id: u64,
    pub doc: Arc<Document>,
    pub active_sketch: Option<u64>,
    pub pending_moves: std::collections::BTreeMap<u64, solidcraft_doc::Mat>,
    pub visibility: Visibility,
    pub undo: Vec<Snapshot>,
    pub redo: Vec<Snapshot>,
}

/// A way of shading the model's faces to judge their shape (view state, not part of the design).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SurfaceAnalysis {
    /// Reflected stripes: `stripes` across the view.
    Zebra { stripes: f64 },
    /// Faces by draft angle to `pull`: past +`angle` green, past −`angle` red, between yellow.
    Draft { pull: Vec3, angle: f64 },
    /// Colour by curvature: blue flat, through green, to red at 1 / `radius`.
    Curvature { radius: f64 },
    /// Mirror finish reflecting a studio (sky, horizon, floor, light panels).
    Environment,
    /// Green where a tool coming along `dir` reaches the face (facing it, nothing above), red
    /// elsewhere.
    Access { dir: Vec3 },
}

/// One seam measured by `inspect.continuity`, where the viewport shades it: the stations it was
/// sampled at, in world coordinates, and how it reads. View state only: the design is untouched.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ContinuitySeam {
    /// The body the seam is an edge of.
    pub body: String,
    /// The edge's index in the body's display mesh edge order.
    pub edge: usize,
    /// Sample points along the seam, in world coordinates.
    pub points: Vec<Vec3>,
    /// The seam's worst verdict along its length.
    pub verdict: kernel::SeamVerdict,
    /// The highest continuity order it meets along its whole length (`None`: the faces do not
    /// meet).
    pub order: Option<kernel::Continuity>,
    /// Successful measurement settings and diagnostics, owned by this particular seam.
    pub details: status_details::ContinuityDetail,
}

/// A face Isocurve Analysis shows: the body, the picked point on the face (its face is found
/// from it again whenever the body changes) and how many curves each way. View state only.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct IsoPick {
    pub body: String,
    pub at: Vec3,
    pub count: usize,
}

/// What Curvature Comb Analysis shows: the picks, the options every comb is drawn with, and
/// the teeth drawn, all at one scale so their lengths compare. View state only.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct CombView {
    pub picks: Vec<CombPick>,
    /// Teeth per curve.
    pub density: usize,
    /// The multiplier on the automatic scale.
    pub length: f64,
    /// A fixed comb length per unit curvature, in place of the automatic one.
    pub fixed_scale: Option<f64>,
    /// The comb length per unit curvature the teeth are drawn at, shared by every comb.
    pub scale: f64,
    /// Each comb's teeth as (foot, tip), in world coordinates.
    pub teeth: Vec<Vec<[Vec3; 2]>>,
}

/// A curve Curvature Comb Analysis shows, as it was picked (found again whenever it is drawn).
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CombPick {
    /// A sketch curve.
    Curve { sketch: u64, curve: String },
    /// The body edge whose exact curve passes nearest `at`.
    Edge { at: Vec3 },
    /// The face at `at`, combed along `count` iso curves each way.
    Face { at: Vec3, count: usize },
}

impl SurfaceAnalysis {
    /// How a picture shades the faces for this analysis.
    pub fn mode(self) -> render::AnalysisMode {
        use render::AnalysisMode as M;
        match self {
            SurfaceAnalysis::Zebra { stripes } => M::Zebra { stripes },
            SurfaceAnalysis::Draft { pull, angle } => M::Draft { pull, angle },
            SurfaceAnalysis::Curvature { radius } => M::Curvature { radius },
            SurfaceAnalysis::Environment => M::Environment,
            SurfaceAnalysis::Access { dir } => M::Access { dir },
        }
    }
}

const MAX_UNDO: usize = 200;
/// Checkpoints kept (the oldest go first).
const MAX_CHECKPOINTS: usize = 16;
const MAX_LOG: usize = 2000;

impl Default for Session {
    fn default() -> Self {
        Session::new(Document::new("Untitled"))
    }
}

impl Session {
    /// A session on an in-memory document. For a document read from a file, use
    /// [`Self::opened_at`] so relative component links are resolved against that file.
    pub fn new(doc: Document) -> Self {
        Self::opened(doc, None)
    }

    /// A session on `doc` whose evaluation is the model its file kept (`saved`), when that is
    /// `doc`'s evaluation ([`Model::adopt`]); otherwise the timeline is evaluated.
    /// Use [`Self::opened_at`] when the document's file location is known.
    /// `SOLIDCRAFT_NO_MODEL_CACHE` set always evaluates.
    pub fn opened(doc: Document, saved: Option<&solidcraft_doc::SavedModel>) -> Self {
        Self::opened_validating(doc, saved, solidcraft_kernel::Validation::default())
    }

    /// [`Self::opened`], its features evaluated under `validation` ([`Session::set_validation`]).
    pub fn opened_validating(mut doc: Document, saved: Option<&solidcraft_doc::SavedModel>, validation: solidcraft_kernel::Validation) -> Self {
        // A design's font files are checked like its other paths before it is evaluated.
        solidcraft_io::vfs::install_checks();
        // The session keeps what is hidden (an undo step of its own); the design file carries it.
        let visibility = std::mem::take(&mut doc.visibility);
        let doc = Arc::new(doc);
        // A model saved under another check mode holds that mode's results: evaluated again.
        let adopted = saved
            .filter(|_| solidcraft_geom::env::var_os("NO_MODEL_CACHE").is_none())
            .and_then(|saved| Model::adopt(&doc, saved).map_err(|e| log::info!("the saved model is not used: {e}")).ok())
            .filter(|model| model.validation() == validation);
        let model = match adopted {
            Some(model) => model,
            None => {
                let mut model = Model::new();
                model.set_validation(validation);
                model.evaluate_shared(&doc);
                model
            }
        };
        Session {
            saved: doc.clone(),
            doc,
            model,
            undo: Vec::new(),
            redo: Vec::new(),
            active_sketch: None,
            sketch_entry: None,
            feature_edit_sketch: None,
            selection: Vec::new(),
            path: None,
            uid: NEXT_UID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            disk: None,
            read_only: false,
            user_named: false,
            open_preview: None,
            log: Vec::new(),
            revision: 1,
            visibility_revision: 0,
            browser_stamp: [0; 6],
            document_id: cmd::inspect::new_document_id(),
            followed: None,
            auto_project: true,
            param_check: schema::ParamCheck::Off,
            easyprint_upload: None,
            live_links: None,
            active_component: 0,
            active_occurrence_path: None,
            pending_moves: Default::default(),
            exploded: Default::default(),
            world_cache: Default::default(),
            joints_settled: None,
            occurrence_visibility_cache: Default::default(),
            visibility_index_cache: Default::default(),
            made_bodies: MadeBodies::new(),
            removed_later_cache: Default::default(),
            occurrence_bodies_cache: Default::default(),
            placement_index_cache: Default::default(),
            dirty_cache: Default::default(),
            clearance_cache: Default::default(),
            body_renames: Vec::new(),
            contact_meshes: Default::default(),
            clipboard: Vec::new(),
            sketch_clipboard: None,
            appearance_clipboard: None,
            foreign_copy: None,
            section: None,
            section_hatch: true,
            colored_components: None,
            analysis: None,
            continuity: Vec::new(),
            isocurves: Vec::new(),
            comb: None,
            status_order: Vec::new(),
            status_activation: Vec::new(),
            single_surface_analysis: true,
            latest_analysis: None,
            checkpoints: Vec::new(),
            next_checkpoint: 1,
            replacements: 0,
            saved_visibility: visibility.clone(),
            visibility,
            camera: None,
            render_viewport: None,
            render_jobs: Default::default(),
            #[cfg(test)]
            evaluations: 0,
            motion_traces: Default::default(),
            command_check: None,
            crash_journal: false,
        }
    }

    /// Become `fresh` (another design opened, recovered or started), keeping the settings that
    /// are the user's rather than the design's: `auto_project` is a preference (#680).
    /// Checkpoints survive too: a batch that failed after `file.new` or `doc.open` goes back
    /// from the checkpoint it took before its first step, and that checkpoint belongs to the
    /// design before the batch, which is what the batch promised (#3242). The design is what a
    /// checkpoint keeps, not the replaced session's file path (docs/mcp.md).
    pub fn replace_design(&mut self, fresh: Session) {
        let auto_project = self.auto_project;
        let validation = self.validation();
        let single_surface_analysis = self.single_surface_analysis;
        let param_check = self.param_check;
        let easyprint_upload = self.easyprint_upload.take();
        let render_viewport = self.render_viewport.clone();
        let render_jobs = self.render_jobs.scratch();
        let checkpoints = std::mem::take(&mut self.checkpoints);
        let next_checkpoint = self.next_checkpoint;
        // The design on screen stays journaled for crash reports at once, not from the app's
        // next frame, so commands sent right after `doc.open` or `file.new` are kept.
        let crash_journal = self.crash_journal;
        let replacements = self.replacements.wrapping_add(1);
        let command_check = self.command_check.take();
        *self = fresh;
        self.crash_journal = crash_journal;
        self.command_check = command_check;
        self.auto_project = auto_project;
        // The user's, like `auto_project`: the commands that open a design evaluate it under it
        // already (`Session::opened_validating`).
        self.set_validation(validation);
        self.set_single_surface_analysis(single_surface_analysis);
        self.param_check = param_check;
        self.easyprint_upload = easyprint_upload;
        self.render_viewport = render_viewport;
        self.render_jobs = render_jobs;
        self.checkpoints = checkpoints;
        self.next_checkpoint = next_checkpoint;
        self.replacements = replacements;
    }

    /// Install (or with `None` remove) the check [`Session::execute`] runs before every command,
    /// for an app that embeds the engine and decides which commands may run. A refusal comes back
    /// as [`EngineError::Refused`] and leaves the session as it was.
    pub fn set_command_check(&mut self, check: Option<CommandCheck>) {
        self.command_check = check;
    }

    /// The check installed with [`Session::set_command_check`], to hand on to another session.
    pub fn command_check(&self) -> Option<CommandCheck> {
        self.command_check.clone()
    }

    /// Ask the host's check, if any, whether `spec` may run with `params`. A check that panics
    /// refuses the command as an internal error instead of taking the session down.
    fn check_command(&self, spec: &CommandSpec, params: &Value) -> Result<()> {
        let Some(check) = &self.command_check else { return Ok(()) };
        // The check sees the parameters as the command will (no parameters: an empty object).
        let empty = Value::Object(Default::default());
        let params = if params.is_null() { &empty } else { params };
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check(spec.id, params, self))) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(reason)) => Err(EngineError::Refused { cmd: spec.id.to_string(), reason }),
            Err(_) => Err(EngineError::Internal(spec.id.to_string(), "the host's command check failed".into())),
        }
    }

    /// The design has changes not in its file: in the document, or in what is hidden.
    pub fn is_dirty(&self) -> bool {
        self.document_differs() || self.visibility != self.saved_visibility
    }

    /// The document is not the one its file holds: compared once per pair of documents and
    /// revision ([`DirtyAnswer`]).
    fn document_differs(&self) -> bool {
        if Arc::ptr_eq(&self.doc, &self.saved) {
            return false;
        }
        let Ok(mut answer) = self.dirty_cache.lock() else { return *self.doc != *self.saved };
        let same = |a: &DirtyAnswer| a.revision == self.revision && Arc::ptr_eq(&a.doc, &self.doc) && Arc::ptr_eq(&a.saved, &self.saved);
        if let Some(a) = answer.as_ref().filter(|a| same(a)) {
            return a.differs;
        }
        let differs = *self.doc != *self.saved;
        *answer = Some(DirtyAnswer { revision: self.revision, doc: self.doc.clone(), saved: self.saved.clone(), differs });
        differs
    }

    /// Let the unsaved-changes answer go, with the documents it holds.
    fn forget_dirty_answer(&self) {
        if let Ok(mut answer) = self.dirty_cache.lock() {
            *answer = None;
        }
    }

    pub fn mark_saved(&mut self) {
        self.forget_dirty_answer();
        self.saved = self.doc.clone();
        self.saved_visibility = self.visibility.clone();
    }

    /// The name Save As offers for the design's file, without its extension: the name of the
    /// file it was opened from or last saved to, unless the design has been renamed since, and
    /// the design's own name otherwise. The name stored in a file need not be the file's: a
    /// design named `Plate` saved as `Plate-a.sc` and copied to `Plate-b.sc` is `Plate` in both,
    /// and offering `Plate.sc` for each invites replacing one file with the other (#8329).
    pub fn save_as_name(&self) -> String {
        let stem = self.path.as_deref().and_then(|p| std::path::Path::new(p).file_stem()).map(|s| s.to_string_lossy().trim().to_string());
        match stem {
            Some(stem) if !stem.is_empty() && self.doc.name == self.saved.name => stem,
            _ => self.doc.name.clone(),
        }
    }

    pub fn render_backend(&self) -> render::BackendDescriptor {
        let mut backend = render::builtin_backend();
        if let Some(viewport) = &self.render_viewport {
            backend.executors.push((**viewport).clone());
        }
        if let Some(executor) = &self.render_jobs.descriptor {
            let mut executor = executor.clone();
            executor.availability = image_host_availability(self.render_jobs.host.as_ref(), &executor.id);
            backend.executors.push(executor);
        }
        if let Some(raytrace) = render_jobs::cpu_raytrace_descriptor(self.render_jobs.host.as_ref()) {
            backend.executors.push(raytrace);
        }
        for (host, descriptor) in &self.render_jobs.additional {
            let mut descriptor = descriptor.clone();
            descriptor.availability = image_host_availability(Some(host), &descriptor.id);
            backend.executors.push(descriptor);
        }
        backend
    }

    pub fn render_viewport(&self) -> Option<&render::ExecutorDescriptor> {
        self.render_viewport.as_deref()
    }

    /// Installed hosts supply pure metadata; document commands cannot invent a device.
    pub fn set_render_viewport(&mut self, viewport: Option<render::ExecutorDescriptor>) -> Result<()> {
        if self.render_viewport.as_deref() == viewport.as_ref() {
            return Ok(());
        }
        if let Some(executor) = &viewport {
            if executor.id.as_str() == "cpu-raytrace" {
                return Err(EngineError::Other("cpu-raytrace is reserved for the native CPU raytrace integrator".into()));
            }
            if executor.profiles.is_empty() || executor.profiles.iter().any(|p| p.operation != render::RenderOperation::Viewport) {
                return Err(EngineError::Other("host viewport executor must contain only viewport profiles".into()));
            }
            let mut backend = render::builtin_backend();
            if let Some(executor) = &self.render_jobs.descriptor {
                backend.executors.push(executor.clone());
            }
            if let Some(raytrace) = render_jobs::cpu_raytrace_descriptor(self.render_jobs.host.as_ref()) {
                backend.executors.push(raytrace);
            }
            backend.executors.extend(self.render_jobs.additional.iter().map(|(_, d)| d.clone()));
            backend.executors.push(executor.clone());
            // Resolution validates the whole registry, including duplicate executor IDs.
            let request = render::RenderRequest {
                backend: None,
                mode: render::RenderMode::Preview,
                operation: render::RenderOperation::Image,
                width: 1,
                height: 1,
                format: Some(render::RenderFormat::Png),
                features: Vec::new(),
                policy: Default::default(),
            };
            render::resolve_request(&[backend], &request).map_err(|e| EngineError::Other(e.to_string()))?;
        }
        self.render_viewport = viewport.map(Arc::new);
        Ok(())
    }

    /// The design as its current file holds it, including visibility and relative link paths.
    pub fn design(&self) -> std::borrow::Cow<'_, Document> {
        self.design_at(self.path.as_deref())
    }
    /// The design has changes not in its file (a recovered design).
    pub fn mark_unsaved(&mut self) {
        self.saved = Arc::new(Document::new(""));
    }

    pub fn echo(&mut self, s: impl Into<String>) {
        self.log.push(s.into());
        if self.log.len() > MAX_LOG {
            let n = self.log.len() - MAX_LOG;
            self.log.drain(..n);
        }
    }

    /// Mutable document (copy-on-write).
    pub fn doc_mut(&mut self) -> &mut Document {
        // The placement index keeps the document alive with its answers; let it go before an
        // in-place edit, so the edit stays in place (a shared handle would clone the document
        // to a new address behind the crash journal's back, #7502) and no stale lookup outlives it.
        if let Ok(mut index) = self.placement_index_cache.lock() {
            *index = None;
        }
        self.forget_dirty_answer();
        self.model.forget_fingerprints();
        Arc::make_mut(&mut self.doc)
    }

    /// Prepared placements only: optional save caches must not compute a cold world.
    pub(crate) fn cached_world_state(&self) -> Option<Arc<solidcraft_doc::ModelState>> {
        let state = self.model.state();
        if self.doc.occurrences.is_empty() {
            return Some(state);
        }
        self.world_cache.lock().ok()?.as_ref().filter(|cache| cache.matches(self, &state)).and_then(|cache| cache.world.clone().ok())
    }

    /// Re-evaluate the model after a document change.
    /// The model placed in the world through component occurrences (and uncaptured moves): what
    /// to display, measure, pick and export. The same as `model.state()` without components.
    /// An error when the model cannot be placed (see [`solidcraft_doc::world_state`]).
    pub fn world_state(&self) -> Result<Arc<solidcraft_doc::ModelState>> {
        let st = self.model.state();
        if self.doc.occurrences.is_empty() {
            return Ok(st);
        }
        if let Ok(c) = self.world_cache.lock()
            && let Some(cached) = c.as_ref()
            && cached.matches(self, &st)
        {
            return cached.world.clone().map_err(EngineError::Other);
        }
        // The bodies the last world state placed: unchanged ones are not transformed and meshed
        // again (a design built command by command changed one body at a time).
        let placed = self.world_cache.lock().ok().and_then(|c| c.as_ref().map(|c| c.placed.clone())).unwrap_or_default();
        // Taken out while placing, so a scratch copy of the session sharing it never waits.
        let mut reuse = placed.lock().map(|mut p| std::mem::take(&mut *p)).unwrap_or_default();
        let w = {
            if self.pending_moves.is_empty() {
                solidcraft_doc::world_state_reusing(&self.doc, &st, &mut reuse)
            } else {
                let mut d = (*self.doc).clone();
                for o in &mut d.occurrences {
                    if let Some(m) = self.pending_moves.get(&o.id) {
                        o.transform = *m;
                    }
                }
                solidcraft_doc::world_state_reusing(&d, &st, &mut reuse)
            }
        };
        if let Ok(mut p) = placed.lock() {
            *p = reuse;
        }
        let w = w.map_err(EngineError::from).and_then(|w| self.authored_sketches(w)).map(Arc::new);
        if let Ok(mut c) = self.world_cache.lock() {
            *c = Some(WorldCache {
                doc: self.doc.clone(),
                model: st,
                pending_moves: self.pending_moves.clone(),
                active_component: self.active_component,
                active_occurrence_path: self.active_occurrence_path.clone(),
                world: w.as_ref().map(Arc::clone).map_err(ToString::to_string),
                placed,
                frames: Arc::default(),
            });
        }
        w
    }

    /// `w` with the active component's sketches shown at the placement being authored.
    fn authored_sketches(&self, mut w: solidcraft_doc::ModelState) -> Result<solidcraft_doc::ModelState> {
        // Sketch definitions are displayed once, at the placement currently being authored.
        if self.active_occurrence_path.is_some() && self.active_component != 0 {
            let first = self.occurrence_placement(&self.doc.first_path(self.active_component)).map(|p| p.world).unwrap_or(solidcraft_doc::IDENTITY);
            let inverse = solidcraft_doc::mat_inverse(&first)
                .ok_or_else(|| EngineError::Other(format!("the placement of component {} cannot be inverted", self.active_component)))?;
            {
                let delta = solidcraft_doc::mat_mul(&self.active_component_frame(), &inverse);
                for sketch in w.sketches.iter_mut().filter(|sk| self.doc.feature(sk.feature).is_some_and(|f| f.component == self.active_component)) {
                    let sketch = Arc::make_mut(sketch);
                    let plane = sketch.plane;
                    sketch.plane = solidcraft_geom::Plane::new(
                        solidcraft_doc::apply_point(&delta, plane.origin),
                        solidcraft_doc::apply_vector(&delta, plane.x),
                        solidcraft_doc::apply_vector(&delta, plane.y),
                    )
                    .ok_or_else(|| EngineError::Other("a sketch plane cannot be placed at the authored placement".into()))?;
                    for wire in &mut sketch.sketch.wires {
                        if wire.space == solidcraft_doc::sketch::WireSpace::Component {
                            solidcraft_doc::map_wire(wire, &delta);
                        }
                    }
                }
            }
        }
        Ok(w)
    }

    /// Every placed body's world name, own name and frame. Kept with the world state while it is
    /// current, so commands naming a body in a large assembly do not each name every body again.
    pub(crate) fn body_frames(&self) -> BodyFrames {
        let st = self.model.state();
        let compute = || -> BodyFrames {
            Arc::new(self.placed_document().body_frames_with_source(&st).into_iter().map(|(world, own, _, _, m)| (world, own, m)).collect())
        };
        let cell = self.world_cache.lock().ok().and_then(|c| c.as_ref().filter(|c| c.matches(self, &st)).map(|c| c.frames.clone()));
        match cell {
            Some(cell) => cell.get_or_init(compute).clone(),
            None => compute(),
        }
    }

    /// When SMLib's check runs on the features evaluated: after every sweep, boolean and blend,
    /// once on the bodies each feature keeps (the default), or not at all. A user's setting,
    /// kept when another design is opened (`diagnostics.validation`, the app's preference).
    pub fn validation(&self) -> solidcraft_kernel::Validation {
        self.model.validation()
    }

    /// Evaluate the features computed from now on under `validation` ([`Session::validation`]);
    /// the results already there stand until their features are computed again
    /// (`timeline.compute_all` computes every one).
    pub fn set_validation(&mut self, validation: solidcraft_kernel::Validation) {
        self.model.set_validation(validation);
    }

    /// The model body a name means: one of the model's own bodies as it is named, else the body
    /// a placed copy shows (`KB (K:2)`, the name `inspect.measure` lists), as `appearance.assign`
    /// takes it (#9740). `None` when neither names it.
    pub(crate) fn own_body_name(&self, name: &str) -> Option<String> {
        if self.model.state().body(name).is_some() {
            return Some(name.to_string());
        }
        self.body_frames().iter().find(|(world, ..)| world == name).map(|(_, own, _)| own.clone())
    }

    /// A component by its id or name, or by the name of one of its placements (`K:2`); never the
    /// root.
    pub(crate) fn component_or_placement(&self, key: &str) -> Option<u64> {
        self.doc.find_component(key).or_else(|| self.doc.occurrences.iter().find(|o| o.name == key).map(|o| o.component)).filter(|id| *id != 0)
    }

    pub fn refresh(&mut self) {
        if self.model.evaluate_shared(&self.doc) {
            #[cfg(test)]
            {
                self.evaluations += 1;
            }
        }
        self.revision += 1;
        self.prune_occurrence_state();
        // Looked at only when what the groups can name has changed, not on every refresh.
        let stamp = self.doc.browser_stamp();
        if stamp != self.browser_stamp {
            if let Some((groups, order)) = self.doc.browser_pruned() {
                let d = self.doc_mut();
                (d.browser_groups, d.browser_order) = (groups, order);
            }
            self.browser_stamp = self.doc.browser_stamp();
        }
        // Joints follow the geometry their origins snap to.
        if !self.doc.assembly.joints.is_empty() {
            // A solve that moves parts changes what features reading where a part is placed
            // (a mirror of a placed body) make: the model is evaluated again for the placements
            // the design now holds, and the joints solved for that model, a few times at most.
            for _ in 0..3 {
                let before = self.doc.clone();
                self.solve_joints();
                if Arc::ptr_eq(&before, &self.doc) || before.occurrences == self.doc.occurrences {
                    break;
                }
                self.model.evaluate(&self.doc);
                #[cfg(test)]
                {
                    self.evaluations += 1;
                }
            }
        }
        cmd::motion::associative::refresh(self);
        if let Some(id) = self.active_sketch
            && self.doc.feature(id).is_none()
        {
            self.active_sketch = None;
        }
    }

    /// Solve the joints, unless nothing they read changed since this session last solved them:
    /// the model is the one that solve read and the design is as the solve left it, apart from
    /// its features (the joints read those only through the model). A command that refreshes on
    /// its own (a parameter change) is refreshed again after it, and that second solve would
    /// find every occurrence where it is.
    fn solve_joints(&mut self) {
        let state = self.model.state();
        let settled = self.joints_settled.as_ref().is_some_and(|(doc, model)| {
            std::sync::Weak::as_ptr(model) == Arc::as_ptr(&state) && (Arc::ptr_eq(doc, &self.doc) || same_but_features(doc, &self.doc))
        });
        if settled {
            return;
        }
        cmd::joints::place(self);
        self.joints_settled = Some((self.doc.clone(), Arc::downgrade(&self.model.state())));
    }

    /// [`Session::refresh`] after the design changed from `previous`; a change that only renamed
    /// a body or a feature (or undid or redid such a rename) renames it in the model instead,
    /// which keeps every result and mesh (#3044).
    fn refresh_from(&mut self, previous: &Arc<Document>, visibility_restored: bool) {
        let rename = self.body_rename(previous);
        // Undo/redo restored visibility in the target document's names already. Remapping
        // those names again can transfer a deleted body's stale hide to the restored body.
        if !visibility_restored {
            self.follow_plane_renames(previous);
            self.follow_axis_point_renames(previous);
            self.remap_hidden_bodies(previous, &self.model.state(), rename.as_ref().map(|(old, new, _)| (old.as_str(), new.as_str())));
            // Locked bodies follow explicit renames and outside edits too (#9737).
            if let Some((old, new, _)) = &rename {
                for b in self.visibility.locked_bodies.iter_mut().filter(|b| **b == *old) {
                    *b = new.clone();
                }
            }
        }
        if let Some((old, new, names_only)) = rename
            && names_only
            && self.model.rename_body(&self.doc, &old, &new)
        {
            self.revision += 1;
            if !self.doc.assembly.joints.is_empty() {
                self.solve_joints();
            }
            return;
        }
        // A feature's name labels it and builds nothing.
        if let Some(id) = self.doc.feature_rename_from(previous)
            && self.model.rename_feature(&self.doc, id)
        {
            self.revision += 1;
            return;
        }
        self.refresh();
    }

    /// Pin the name of every body the features made, as if the body had been renamed to it, and
    /// reserve the `Body<N>` names, so that no later command renames a body or hands its number
    /// to another. It is part of the command that made the bodies. A design saved before names
    /// were pinned is pinned as it opens ([`Self::opened_at`]), with the names it was saved
    /// with; one made in memory ([`Self::new`]) by its first command that changes it, with the
    /// names it has after that command (a delete, reorder, suppress or rollback renumbers the
    /// later counted bodies once).
    fn pin_body_names(&mut self) {
        if self.model.is_stale() {
            return;
        }
        let mut pins: Vec<(u64, Vec<String>)> = Vec::new();
        let mut reserve: Vec<String> = Vec::new();
        // The names of the bodies each feature made, kept with the state they were read from:
        // every command pins, and reading them again searched every result's bodies (features ×
        // bodies per command). A state is the same while it is the same allocation (the weak
        // reference keeps that address from being reused), and its bodies never change in place.
        let mut known = std::mem::take(&mut self.made_bodies);
        let mut made_bodies = MadeBodies::with_capacity(known.len());
        for (i, f) in self.doc.features.iter().enumerate() {
            if matches!(f.kind, solidcraft_doc::FeatureKind::FrameMember { .. }) {
                continue;
            }
            // Results are in timeline order: the feature's own index first, the search only when not.
            let r = self.model.results.get(i).filter(|r| r.id == f.id).or_else(|| self.model.result(f.id));
            let Some(r) = r.filter(|r| r.error.is_none() && !r.skipped && !r.stale) else { continue };
            let made = match known.remove(&f.id).filter(|(state, _)| std::sync::Weak::as_ptr(state) == Arc::as_ptr(&r.state)) {
                Some(entry) => made_bodies.entry(f.id).or_insert(entry),
                None => made_bodies.entry(f.id).or_insert_with(|| {
                    let names = r.state.bodies.iter().filter(|b| b.feature == f.id).map(|b| b.name.clone()).collect();
                    (Arc::downgrade(&r.state), names)
                }),
            };
            let made: Vec<&str> = made.1.iter().map(String::as_str).collect();
            reserve.extend(
                made.iter().filter(|n| solidcraft_doc::is_counted_body_name(n) && !self.doc.reserved_body_names.contains(**n)).map(|n| n.to_string()),
            );
            // Nothing to pin when every slot the bodies take already holds a name.
            if made.len() <= f.body_names.len() && f.body_names.iter().take(made.len()).all(|slot| !slot.trim().is_empty()) {
                continue;
            }
            let mut names = f.body_names.clone();
            names.resize(names.len().max(made.len()), String::new());
            for (slot, name) in names.iter_mut().zip(&made) {
                if slot.trim().is_empty() {
                    *slot = name.to_string();
                }
            }
            if names != f.body_names {
                pins.push((f.id, names));
            }
        }
        self.made_bodies = made_bodies;
        if pins.is_empty() && reserve.is_empty() {
            return;
        }
        // The fingerprints that stand: taken before the pins change the design.
        let renamed: Vec<u64> = pins.iter().map(|(id, _)| *id).collect();
        let keep = self.model.pinned_prefix(&self.doc, &renamed);
        let doc = self.doc_mut();
        for (id, names) in pins {
            if let Some(f) = doc.feature_mut(id) {
                f.body_names = names;
            }
        }
        doc.reserved_body_names.extend(reserve);
        // The pinned names are the ones the bodies have: no result changes, so none is evaluated.
        self.model.refingerprint_shared(&self.doc, keep);
    }

    /// A hidden construction plane stays hidden when the design renames it, or undo or redo
    /// gives it another name (#4353): the hidden list keeps planes by name.
    fn follow_plane_renames(&mut self, previous: &Document) {
        if self.visibility.hidden_origin.is_empty() {
            return;
        }
        let before: std::collections::HashMap<u64, &str> = previous
            .features
            .iter()
            .filter(|f| matches!(f.kind, solidcraft_doc::FeatureKind::ConstructionPlane { .. }))
            .map(|f| (f.id, f.name.as_str()))
            .collect();
        for f in &self.doc.features {
            if let Some(old) = before.get(&f.id).filter(|old| **old != f.name) {
                for n in self.visibility.hidden_origin.iter_mut().filter(|n| *n == old) {
                    n.clone_from(&f.name);
                }
            }
        }
    }

    /// The body rename that turned `previous` into the design (old name, new name, whether
    /// only names changed): one this session made (either way round), else one the designs show.
    fn body_rename(&mut self, previous: &Arc<Document>) -> Option<(String, String, bool)> {
        let holds = |w: &std::sync::Weak<Document>, d: &Arc<Document>| w.upgrade().is_some_and(|x| Arc::ptr_eq(&x, d));
        self.body_renames.retain(|r| r.before.strong_count() > 0 && r.after.strong_count() > 0);
        for r in &self.body_renames {
            if holds(&r.before, previous) && holds(&r.after, &self.doc) {
                return Some((r.old.clone(), r.new.clone(), r.names_only));
            }
            if holds(&r.after, previous) && holds(&r.before, &self.doc) {
                return Some((r.new.clone(), r.old.clone(), r.names_only));
            }
        }
        let (old, new) = self.inferred_body_rename(previous)?;
        self.record_body_rename(previous, &old, &new, true);
        Some((old, new, true))
    }

    fn inferred_body_rename(&self, previous: &Document) -> Option<(String, String)> {
        self.doc.body_rename_from(previous, |id| {
            self.model.result(id).map(|r| r.state.bodies.iter().filter(|b| b.feature == id).map(|b| b.name.clone()).collect()).unwrap_or_default()
        })
    }

    /// Remember explicit renames too: features that refer to the body also change their
    /// parameters, so the document-only fast-path proof may not recognize their undo/redo.
    fn remember_body_rename(&mut self, previous: &Arc<Document>, old: &str, new: &str) {
        let names_only = self.inferred_body_rename(previous).is_some();
        self.record_body_rename(previous, old, new, names_only);
    }

    fn record_body_rename(&mut self, previous: &Arc<Document>, old: &str, new: &str, names_only: bool) {
        self.body_renames.push(BodyRename {
            before: Arc::downgrade(previous),
            after: Arc::downgrade(&self.doc),
            old: old.into(),
            new: new.into(),
            names_only,
        });
        if self.body_renames.len() > MAX_UNDO {
            self.body_renames.remove(0);
        }
    }

    /// Only persistent render intent, part properties, appearances or the browser's groups changed.
    /// Never evaluate or clear a stale model here.
    fn render_only_change(&self, before: &Snapshot) -> bool {
        self.active_sketch == before.active_sketch
            && self.pending_moves == before.pending_moves
            && (self.doc.differs_only_in_render(&before.doc)
                || self.doc.differs_only_in_part_properties(&before.doc)
                || self.doc.differs_only_in_appearance(&before.doc)
                || self.doc.differs_only_in_browser(&before.doc))
    }

    /// Only what is hidden or shown changed (`browser.visibility`): the document and the model
    /// are untouched, so the view refreshes without evaluating anything again.
    pub(crate) fn visibility_only_change(&self, before: &Snapshot) -> bool {
        Arc::ptr_eq(&self.doc, &before.doc) && self.visibility != before.visibility
    }

    fn refresh_render(&mut self, previous: &Arc<Document>) {
        self.revision += 1;
        // Geometry/placements have been proved identical. Retarget the cache's document key
        // instead of discarding prepared meshes merely because the document Arc changed.
        if let Ok(mut cache) = self.world_cache.lock()
            && let Some(cache) = cache.as_mut()
            && Arc::ptr_eq(&cache.doc, previous)
        {
            cache.doc = self.doc.clone();
        }
    }

    /// Only what is hidden or shown changed: the model is untouched, so only the view revision
    /// moves (the visibility caches are keyed on it) and the viewport redraws.
    pub(crate) fn refresh_display(&mut self) {
        self.revision += 1;
        self.visibility_revision += 1;
    }

    /// Show the sketch a live frame changed, leaving the features after it as they were last
    /// evaluated (marked stale); a full `refresh` when the model cannot show it that way.
    fn refresh_live(&mut self, edit: LiveEdit) {
        // design-only: a live drag shows the design's sketch in the model.
        let sketch = self.doc.sketch(edit.sketch).ok().cloned();
        if sketch.is_some_and(|sketch| self.model.show_live_sketch(edit.sketch, sketch, edit.report)) {
            self.revision += 1;
        } else {
            self.refresh();
        }
    }

    /// The sketches whose last evaluation solved, each with its own parameter-driven values
    /// (taken before a value edit, for [`Session::settle_value_edit`]).
    pub(crate) fn solved_sketches(&self) -> SketchValues {
        self.model
            .results
            .iter()
            .filter_map(|r| r.state.sketch(r.id).filter(|k| k.report.keeps_orientation()).map(|k| (r.id, own_values(&k.sketch))))
            .collect()
    }

    /// After the document's values changed (parameters, a configuration, the units): evaluate,
    /// and refuse the edit when a sketch that solved before no longer does, or turns over,
    /// because its own parameter-driven values changed (the command fails, so `execute`
    /// restores the document's values and geometry). A sketch broken by a change upstream
    /// (a projected or linked edge moved) is not the edit's to refuse: it carries a warning,
    /// as before. Otherwise store each sketch whose values changed as solved, so the next edit
    /// continues from it (see `Model::adopt_solved_sketches`).
    pub(crate) fn settle_value_edit(&mut self, solved_before: &SketchValues) -> Result<()> {
        // The solved geometry each sketch had before the edit, for the shrink/fling checks
        // after the refresh below (the same checks `settle` runs on an edit's own solve).
        let solved_geometry: std::collections::BTreeMap<u64, solidcraft_sketch::Sketch> =
            self.model.results.iter().filter_map(|r| r.state.sketch(r.id).map(|k| (r.id, k.sketch.clone()))).collect();
        // Quantity changes alter topology, so store the regenerated instances in the same
        // transaction as the parameter edit. Later picks and undo must see those same ids.
        let mut updated: Vec<_> = self
            .doc
            .features
            .iter()
            .filter_map(|feature| match &feature.kind {
                solidcraft_doc::FeatureKind::Sketch { sketch, .. }
                    if sketch.patterns.iter().any(|p| p.count_expression.is_some() || !p.expressions.is_empty()) =>
                {
                    Some((feature.id, sketch.clone()))
                }
                _ => None,
            })
            .collect();
        if !updated.is_empty() {
            let (vals, _) = self.doc.param_values();
            for (_, sketch) in &mut updated {
                self.doc.apply_pattern_values(&vals, sketch)?;
            }
        }
        for (id, sketch) in updated {
            // design-only: a parameter edit rewrites the design's pattern-quantity sketches.
            *self.doc_mut().sketch_mut(id)? = sketch;
        }
        self.refresh();
        for r in &self.model.results {
            let (Some(before), Some(k)) = (solved_before.get(&r.id), r.state.sketch(r.id)) else { continue };
            if own_values(&k.sketch) == *before {
                continue;
            }
            if !k.report.keeps_orientation() {
                let why = k.report.failure(|f| format!("its constraints would conflict ({f})"));
                return Err(EngineError::Other(format!("sketch `{}`: {why}; the values were not changed", r.name)));
            }
            // The value changed this sketch and it still solves: refuse it the way `settle`
            // refuses an edit's solve when the curve was squeezed away or points thrown far,
            // so a zero (or wild) value is never stored as a sketch that can't be solved back.
            if let Some(before_sketch) = solved_geometry.get(&r.id)
                && let Some(why) = cmd::collapse_reason(before_sketch, &k.sketch)
            {
                return Err(EngineError::Other(format!("sketch `{}`: {why}; the values were not changed", r.name)));
            }
        }
        // Checked first: storing makes the document unique (a copy when the undo step shares it).
        if self.model.edited_sketches(&self.doc).next().is_some() {
            if let Ok(mut index) = self.placement_index_cache.lock() {
                *index = None;
            }
            self.forget_dirty_answer();
            self.model.adopt_solved_sketches(Arc::make_mut(&mut self.doc));
        }
        Ok(())
    }

    /// Run a command with JSON parameters (no dialogs). Undoable commands record an undo step
    /// when they change the document; a failing or panicking command leaves it unchanged. The
    /// host's check ([`Session::set_command_check`]) is asked first, on every call (commands that
    /// a command runs through `execute` included).
    ///
    /// A command that takes `live` ([`CommandSpec::live`]) run with `live: true` is one frame of
    /// an interactive gesture: the model shows its sketch's new solution and keeps every later
    /// feature's last result (stale) instead of evaluating them. Any other command that reads
    /// the model ([`CommandSpec::reads_model`]) first evaluates a stale model in full, so the
    /// gesture's end (`sketch.drag_end`, or whatever runs next after an abandoned gesture) brings
    /// the model up to date before anything reads it.
    pub fn execute(&mut self, id: &str, params: &Value) -> Result<Value> {
        let spec = find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.to_string()))?;
        // The command, the evaluation it causes and the refresh after it: one profiler span.
        let _span = solidcraft_profile::span("command", spec.id);
        let host_viewport = self.render_viewport.clone();
        let host_jobs = self.render_jobs.host.clone();
        let additional_jobs = self.render_jobs.additional.clone();
        let live = spec.live.filter(|_| params.get("live").and_then(Value::as_bool) == Some(true));
        if live.is_none() {
            self.live_links = None;
            if spec.reads_model && self.model.is_stale() {
                self.refresh();
            }
        }
        (spec.enabled)(self).map_err(|why| EngineError::Disabled(spec.id.to_string(), why))?;
        // Before anything changes: a refusal leaves the session exactly as it was.
        self.check_command(spec, params)?;
        let before = Snapshot {
            label: spec.label.to_string(),
            doc: self.doc.clone(),
            active_sketch: self.active_sketch,
            pending_moves: self.pending_moves.clone(),
            visibility: self.visibility.clone(),
        };
        // Where new geometry goes: not in the design, but a replay needs it (component.activate).
        let active_before = (self.active_component, self.active_occurrence_path.clone());
        let replacements = self.replacements;
        let null = Value::Object(Default::default());
        let params = if params.is_null() { &null } else { params };
        let given = params;
        // Picks in the world go into their component's frame (once: commands run by commands
        // already have them there).
        let (_nested, outermost) = frames::Nested::enter(spec.id);
        let mapped = if outermost {
            frames::forget_picks();
            frames::to_local(self, spec.id, params)?
        } else {
            None
        };
        let params = mapped.as_ref().unwrap_or(params);
        if outermost {
            crash::command_begin(self, spec.id, given);
        }
        let scope = crash::Scope::enter();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match live {
            Some(run) => run(self, params),
            None => (spec.run)(self, params).map(|v| (v, None)),
        }));
        drop(scope);
        let r = match r {
            Ok(r) => r,
            Err(p) => {
                let msg = p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_default();
                Err(EngineError::Internal(spec.id.to_string(), msg))
            }
        };
        // A value typed as `name = expression` defines the user parameter `name` (#4071), in the
        // same undo step; a refused definition fails the command, which leaves the design as it was.
        let r = match r {
            Ok((v, live_edit)) if params::has_definition(params) => self.define_inline(spec.id, v).map(|v| (v, live_edit)),
            r => r,
        };
        // A parameter error that did not name the command names it here: the engine knows which
        // command ran, and the reader of the parameters may not (#6651).
        let r = match r {
            Ok(r) => Ok(r),
            Err(EngineError::BadParams { cmd, msg }) if cmd.is_empty() => Err(EngineError::BadParams { cmd: spec.id.to_string(), msg }),
            Err(e) => Err(e),
        };
        // New/open/recover can replace the Session. Device ownership belongs to the host,
        // so its capability metadata survives those document lifecycle commands.
        self.render_viewport = host_viewport;
        if self.render_jobs.host.is_none() && host_jobs.is_some() {
            self.set_render_job_host(host_jobs)?;
        }
        for (host, _) in additional_jobs {
            self.register_render_job_host(host)?;
        }
        let crash_outcome = if outermost && crash::enabled() {
            let failed = r.as_ref().err().map(ToString::to_string);
            // Commands that only read leave no trace (the journal keeps what a replay needs).
            let changed = !Arc::ptr_eq(&self.doc, &before.doc)
                || self.active_sketch != before.active_sketch
                || self.pending_moves != before.pending_moves
                || self.active_component != active_before.0
                || self.active_occurrence_path != active_before.1
                || spec.id.starts_with("select.");
            Some((before.doc.clone(), changed, failed))
        } else {
            None
        };
        let changed = !Arc::ptr_eq(&self.doc, &before.doc);
        // Hiding or showing (`browser.visibility`) leaves the document alone, so it needs its own
        // test: it is saved with the design and has an undo step of its own.
        let hidden = self.visibility != before.visibility;
        let result = match r {
            Ok((v, live_edit)) => {
                let v = if changed { self.with_created(spec.id, params, &before, v) } else { v };
                if changed || hidden {
                    // Another design opened, started or recovered is refreshed in full, however
                    // little it differs from the one before: the shortcut proves a session's own
                    // edit kept its model and joints valid, and a new session has had none.
                    let render_only = self.replacements == replacements && self.render_only_change(&before);
                    let display_only = !changed && hidden;
                    let previous_doc = before.doc.clone();
                    if spec.undoable {
                        self.undo.push(before);
                        if self.undo.len() > MAX_UNDO {
                            self.undo.remove(0);
                        }
                        self.redo.clear();
                    }
                    let pin = outermost && live_edit.is_none() && !render_only;
                    match live_edit {
                        Some(edit) => {
                            let sketch = edit.sketch;
                            self.refresh_live(edit);
                            self.live_links = Some((sketch, self.doc.clone(), self.revision));
                        }
                        None if render_only => self.refresh_render(&previous_doc),
                        None if display_only => self.refresh_display(),
                        None => self.refresh_from(&previous_doc, matches!(spec.id, "edit.undo" | "edit.redo")),
                    }
                    if pin {
                        self.pin_body_names();
                    }
                }
                Ok(v)
            }
            Err(e) => {
                if changed || hidden || self.pending_moves != before.pending_moves {
                    self.doc = before.doc;
                    self.active_sketch = before.active_sketch;
                    self.pending_moves = before.pending_moves;
                    self.visibility = before.visibility;
                    self.refresh();
                }
                Err(e)
            }
        };
        if let Some((before, changed, failed)) = crash_outcome {
            // Refresh and body-name pinning can copy the shared document. Remember the final
            // document, so the next command does not mistake that work for an outside edit
            // and discard the replay. Failed commands have restored their document by now.
            crash::command_done(self.crash_journal, changed, &before, &self.doc, spec.id, given, failed.map_or(Ok(()), Err));
        }
        result
    }

    /// Store the definitions a command left in the design (`Document::define_inline`): its
    /// result names them, `defined` the first (as `sketch.dimension` does) and `definitions` all.
    fn define_inline(&mut self, id: &str, mut v: Value) -> Result<Value> {
        let mut doc = (*self.doc).clone();
        let defined = doc.define_inline().map_err(|e| EngineError::BadParams { cmd: id.to_string(), msg: e.to_string() })?;
        let Some(first) = defined.first() else { return Ok(v) };
        if let Some(out) = v.as_object_mut() {
            out.insert("defined".into(), serde_json::json!(first.name));
            out.insert("definitions".into(), serde_json::json!(defined));
        }
        self.doc = Arc::new(doc);
        Ok(v)
    }

    /// A sketch command's result gets `created`: the ids of the points, curves, constraints and
    /// dimensions it added to its sketch (the `sketch` param, else the active sketch), so every
    /// drawing and modify command reports them the same way (see `cmd::sketch_created`). A command
    /// that runs another one (`sketch.draw.dimensioned`) gets the whole diff, replacing the nested
    /// command's narrower `created`.
    fn with_created(&self, id: &str, params: &Value, before: &Snapshot, mut v: Value) -> Value {
        if !id.starts_with("sketch.") {
            return v;
        }
        let Some(out) = v.as_object_mut() else { return v };
        let named = match params.get("sketch") {
            Some(Value::Number(n)) => self.doc.find_feature(&n.to_string()).map(|f| f.id),
            Some(Value::String(x)) => self.doc.find_feature(x).map(|f| f.id),
            _ => None,
        };
        let Some(sk) = named.or(self.active_sketch).or(before.active_sketch) else { return v };
        // design-only: a command's `created` compares two snapshots of the design.
        let Ok(after) = self.doc.sketch(sk) else { return v };
        out.insert("created".into(), cmd::sketch_created(before.doc.sketch(sk).ok(), after));
        v
    }

    /// Remember the design, active sketch, pending positions and undo history; returns the checkpoint's id.
    pub fn checkpoint(&mut self) -> u64 {
        let id = self.next_checkpoint;
        self.next_checkpoint += 1;
        self.checkpoints.push(Checkpoint {
            id,
            doc: self.doc.clone(),
            active_sketch: self.active_sketch,
            undo: self.undo.clone(),
            pending_moves: self.pending_moves.clone(),
            visibility: self.visibility.clone(),
            redo: self.redo.clone(),
        });
        if self.checkpoints.len() > MAX_CHECKPOINTS {
            self.checkpoints.remove(0);
        }
        id
    }

    /// Go back to checkpoint `id` as if nothing since had happened (the undo history too). It
    /// and later checkpoints are used up.
    pub fn restore_checkpoint(&mut self, id: u64) -> Result<()> {
        let missing = || {
            EngineError::Other(format!(
                "no checkpoint {id} (used up by restoring or forgetting it or an earlier one, or dropped: only the last {MAX_CHECKPOINTS} are kept)"
            ))
        };
        let i = self.checkpoints.iter().position(|c| c.id == id).ok_or_else(missing)?;
        let Some(c) = self.checkpoints.drain(i..).next() else { return Err(missing()) };
        let before = Snapshot {
            label: String::new(),
            doc: self.doc.clone(),
            active_sketch: self.active_sketch,
            pending_moves: self.pending_moves.clone(),
            visibility: self.visibility.clone(),
        };
        self.doc = c.doc;
        self.active_sketch = c.active_sketch;
        self.pending_moves = c.pending_moves;
        self.visibility = c.visibility;
        self.undo = c.undo;
        self.redo = c.redo;
        self.selection.clear();
        if self.render_only_change(&before) {
            self.refresh_render(&before.doc);
        } else if self.visibility_only_change(&before) {
            self.refresh_display();
        } else if !Arc::ptr_eq(&self.doc, &before.doc) || self.active_sketch != before.active_sketch || self.pending_moves != before.pending_moves {
            self.refresh();
        }
        Ok(())
    }

    /// Forget checkpoint `id` (and later ones) without going back.
    pub fn drop_checkpoint(&mut self, id: u64) {
        if let Some(i) = self.checkpoints.iter().position(|c| c.id == id) {
            self.checkpoints.truncate(i);
        }
    }

    /// A scratch copy of the session (same design and model, no history or log) to try
    /// commands on without touching this one.
    pub fn scratch(&self) -> Session {
        Session {
            doc: self.doc.clone(),
            model: self.model.clone(),
            undo: Vec::new(),
            redo: Vec::new(),
            active_sketch: self.active_sketch,
            sketch_entry: None,
            feature_edit_sketch: self.feature_edit_sketch,
            selection: self.selection.clone(),
            path: None,
            uid: self.uid,
            disk: self.disk,
            read_only: self.read_only,
            user_named: self.user_named,
            open_preview: None,
            saved: self.saved.clone(),
            log: Vec::new(),
            revision: self.revision,
            visibility_revision: self.visibility_revision,
            browser_stamp: self.browser_stamp,
            document_id: self.document_id.clone(),
            followed: None,
            auto_project: self.auto_project,
            param_check: self.param_check,
            easyprint_upload: self.easyprint_upload.clone(),
            live_links: None,
            active_component: self.active_component,
            active_occurrence_path: self.active_occurrence_path.clone(),
            pending_moves: self.pending_moves.clone(),
            exploded: self.exploded.clone(),
            world_cache: std::sync::Mutex::new(self.world_cache.lock().ok().and_then(|cache| cache.clone())),
            joints_settled: self.joints_settled.clone(),
            // Read again by the copy: handing it these references measured more peak memory on a
            // large design (the rover, evaluated ahead) for no change in what it pins.
            made_bodies: MadeBodies::new(),
            occurrence_visibility_cache: Default::default(),
            visibility_index_cache: Default::default(),
            removed_later_cache: Default::default(),
            occurrence_bodies_cache: Default::default(),
            placement_index_cache: Default::default(),
            dirty_cache: Default::default(),
            clearance_cache: self.clearance_cache.clone(),
            body_renames: self.body_renames.clone(),
            contact_meshes: self.contact_meshes.clone(),
            clipboard: self.clipboard.clone(),
            sketch_clipboard: self.sketch_clipboard.clone(),
            appearance_clipboard: self.appearance_clipboard.clone(),
            foreign_copy: self.foreign_copy.clone(),
            section: None,
            section_hatch: true,
            colored_components: None,
            analysis: None,
            continuity: Vec::new(),
            isocurves: Vec::new(),
            comb: None,
            status_order: Vec::new(),
            status_activation: Vec::new(),
            single_surface_analysis: self.single_surface_analysis,
            latest_analysis: None,
            checkpoints: Vec::new(),
            next_checkpoint: 1,
            replacements: 0,
            visibility: self.visibility.clone(),
            camera: self.camera,
            render_viewport: self.render_viewport.clone(),
            render_jobs: self.render_jobs.scratch(),
            saved_visibility: self.saved_visibility.clone(),
            #[cfg(test)]
            evaluations: 0,
            motion_traces: self.motion_traces.clone(),
            command_check: self.command_check.clone(),
            crash_journal: false,
        }
    }

    /// Evaluate a feature dialog's complete transaction, including the dependent timeline.
    /// `marker` restores an editor's original marker before recording its undo snapshot.
    /// The source remains untouched, including its history and recovery journal.
    pub fn compute_commands(self, commands: &[(String, Value)], marker: Option<Option<usize>>) -> Result<ComputedCommands> {
        self.compute_commands_inner(commands, marker, None)
    }

    /// Start an edit from its pinned original model, so restoring the original marker cannot
    /// recompute the obsolete suffix even when previews have evicted its evaluation cache.
    pub fn compute_editor_commands(self, commands: &[(String, Value)], source: &Session, original: Session) -> Result<ComputedCommands> {
        if !self.same_document_context(source) {
            return Err(EngineError::Other("the design changed while the feature editor was open".into()));
        }
        let mut expected = (*original.doc).clone();
        expected.marker = source.doc.marker;
        if expected != *source.doc {
            return Err(EngineError::Other("the original editor model does not match its source design".into()));
        }
        self.compute_commands_inner(commands, Some(original.doc.marker), Some(original))
    }

    fn compute_commands_inner(
        self,
        commands: &[(String, Value)],
        marker: Option<Option<usize>>,
        original: Option<Session>,
    ) -> Result<ComputedCommands> {
        let editor_original = original.as_ref().map(|session| session.doc.clone());
        let mut computed = self.scratch();
        let mut steps = Vec::new();
        if let Some(marker) = marker {
            let before = computed.doc.clone();
            let params = marker.map(|position| serde_json::json!({"position":position})).unwrap_or_else(|| serde_json::json!({}));
            let reply = if let Some(original) = original {
                computed.doc = original.doc;
                computed.model = original.model;
                computed.world_cache = original.world_cache;
                serde_json::json!({"marker":marker.unwrap_or(computed.doc.features.len()),"features":computed.doc.features.len()})
            } else {
                let undo = std::mem::take(&mut computed.undo);
                let redo = std::mem::take(&mut computed.redo);
                let selection = computed.selection.clone();
                let reply = computed.execute("timeline.roll_to", &params)?;
                computed.undo = undo;
                computed.redo = redo;
                computed.selection = selection;
                reply
            };
            let changed = !Arc::ptr_eq(&before, &computed.doc);
            steps.push(ComputedCommand { id: "timeline.roll_to".into(), params, before, after: computed.doc.clone(), changed, reply });
        }
        for (id, params) in commands {
            let before = computed.doc.clone();
            let active_sketch = computed.active_sketch;
            let pending_moves = computed.pending_moves.clone();
            let reply = computed.execute(id, params)?;
            let changed = !Arc::ptr_eq(&before, &computed.doc)
                || active_sketch != computed.active_sketch
                || pending_moves != computed.pending_moves
                || id.starts_with("select.");
            steps.push(ComputedCommand { id: id.clone(), params: params.clone(), before, after: computed.doc.clone(), changed, reply });
        }
        // What the source was primed with stays out of the session that is adopted (#6901).
        computed.unprime();
        Ok(ComputedCommands { source: self, computed, steps, editor_original })
    }

    /// Adopt evaluated commands only if their document and authoring context are still current.
    /// No model evaluation occurs here. Rejected or cancelled work never enters the journal.
    pub fn accept_commands(&mut self, mut prepared: ComputedCommands) -> Result<Vec<(String, Value)>> {
        if !prepared.is_current(self) {
            return Err(EngineError::Other("the design changed while the feature was computing".into()));
        }
        if let Some(original) = &prepared.editor_original {
            self.restore_editor_history(&prepared.source.doc, original);
        }
        // Visibility may have changed while the worker ran. Undoing its first command must
        // restore that current view, including sketches, folders and joint glyphs.
        if let Some(before) = prepared.computed.undo.first_mut() {
            before.visibility.clone_from(&self.visibility);
        }
        // Only joints newly hidden by the computation (e.g. an import's rigid group) join
        // the live view. A joint shown meanwhile must not inherit the worker's old hide.
        prepared.computed.visibility.hidden_joints.retain(|joint| !prepared.source.visibility.hidden_joints.contains(joint));
        for step in &prepared.steps {
            crash::accepted_command(self, &step.before, &step.after, &step.id, &step.params, step.changed);
        }
        let replies = prepared.steps.into_iter().map(|step| (step.id, step.reply)).collect();
        self.accept_computed(prepared.computed);
        Ok(replies)
    }

    fn same_document_context(&self, source: &Session) -> bool {
        Arc::ptr_eq(&self.doc, &source.doc)
            && self
                .revision
                .checked_sub(source.revision)
                .is_some_and(|delta| self.visibility_revision.checked_sub(source.visibility_revision) == Some(delta))
            && self.active_sketch == source.active_sketch
            && self.active_component == source.active_component
            && self.active_occurrence_path == source.active_occurrence_path
            && self.pending_moves == source.pending_moves
    }

    /// Cancel an unchanged feature editor by restoring its original evaluated model, even
    /// after preview work has evicted the old suffix from the shared evaluation cache.
    pub fn restore_editor(&mut self, source: &Session, mut original: Session) -> Result<()> {
        if !self.same_document_context(source) {
            return Err(EngineError::Other("the design changed while the feature editor was open; its current timeline was preserved".into()));
        }
        let params = original.doc.marker.map(|position| serde_json::json!({"position":position})).unwrap_or_else(|| serde_json::json!({}));
        crash::accepted_command(self, &self.doc, &original.doc, "timeline.roll_to", &params, !Arc::ptr_eq(&self.doc, &original.doc));
        self.restore_editor_history(&source.doc, &original.doc);
        let undo = std::mem::take(&mut self.undo);
        let redo = std::mem::take(&mut self.redo);
        let selection = std::mem::take(&mut self.selection);
        original.visibility.hidden_joints.clear();
        self.accept_computed(original);
        self.undo = undo;
        self.redo = redo;
        self.selection = selection;
        Ok(())
    }

    /// Hide/show steps taken in an editor belong to the complete design, not its temporary
    /// rollback. Keep each step's visibility and ordering, on both sides of the history.
    fn restore_editor_history(&mut self, source: &Arc<Document>, original: &Arc<Document>) {
        for snapshot in self.undo.iter_mut().chain(&mut self.redo) {
            if Arc::ptr_eq(&snapshot.doc, source) {
                snapshot.doc = original.clone();
            }
        }
    }

    /// Refresh UI caches when adopting a session or switching tabs, without discarding its
    /// already prepared world-space geometry. This does not change the document.
    pub fn invalidate_view(&mut self, previous_revision: u64) {
        let old = self.revision;
        self.revision = old.max(previous_revision) + 1;
    }

    /// Offer what a copy of this session evaluated ahead for the commands about to run
    /// ([`Ahead::evaluate`]): until [`Session::unprime`], evaluating takes each feature result
    /// whose inputs match instead of computing it again (a design changed meanwhile matches
    /// less, or nothing, and is evaluated as usual).
    pub fn prime(&mut self, evaluated: &Evaluated) {
        self.model.prime(&evaluated.0);
    }

    /// Stop offering what [`Session::prime`] offered.
    pub fn unprime(&mut self) {
        self.model.unprime();
    }

    /// Commit commands evaluated on a scratch copy without losing this session's file,
    /// saved state, view state, checkpoints or earlier undo steps.
    pub fn accept_computed(&mut self, mut computed: Session) {
        let previous_revision = self.revision;
        // The first computed step starts from this document. Its undo must restore the view
        // at acceptance, including a body hidden while the computation was running.
        if let Some(before) = computed.undo.first_mut()
            && Arc::ptr_eq(&before.doc, &self.doc)
        {
            before.visibility.hidden_bodies.clone_from(&self.visibility.hidden_bodies);
        }
        let rename = computed.body_rename(&self.doc);
        computed.visibility.hidden_bodies.clone_from(&self.visibility.hidden_bodies);
        computed.remap_hidden_bodies(&self.doc, &self.model.state(), rename.as_ref().map(|(old, new, _)| (old.as_str(), new.as_str())));
        self.visibility.hidden_bodies = std::mem::take(&mut computed.visibility.hidden_bodies);
        self.body_renames = computed.body_renames;
        self.doc = computed.doc;
        self.model = computed.model;
        self.world_cache = computed.world_cache;
        self.undo.append(&mut computed.undo);
        if self.undo.len() > MAX_UNDO {
            self.undo.drain(..self.undo.len() - MAX_UNDO);
        }
        self.redo = computed.redo;
        self.active_sketch = computed.active_sketch;
        self.selection = computed.selection;
        self.revision = computed.revision;
        self.invalidate_view(previous_revision);
        self.active_component = computed.active_component;
        self.active_occurrence_path = computed.active_occurrence_path;
        self.pending_moves = computed.pending_moves;
        self.exploded = computed.exploded;
        // Joints the computation hid (an import's rigid group) stay hidden.
        for j in computed.visibility.hidden_joints {
            if !self.visibility.hidden_joints.contains(&j) {
                self.visibility.hidden_joints.push(j);
            }
        }
        for message in computed.log {
            self.echo(message);
        }
    }

    /// Live preview: run `commands` (id, parameters) on a scratch copy and return the model
    /// before and after. Nothing in this session changes (document, undo history, selection).
    /// A `timeline.redefine` is shown even while the timeline is rolled back to the feature
    /// being edited.
    pub fn preview(&self, commands: &[(String, Value)]) -> Result<Preview> {
        self.scratch().preview_in_place(commands, None).map(|(after, replies)| Preview {
            before: self.model.state(),
            after: after.model.state(),
            replies,
        })
    }

    /// The same evaluated preview in occurrence coordinates, including uncaptured moves.
    pub fn preview_world(&self, commands: &[(String, Value)]) -> Result<Preview> {
        let (after, replies) = self.scratch().preview_in_place(commands, None)?;
        Ok(Preview { before: self.world_state()?, after: after.world_state()?, replies })
    }

    /// World preview with each session's visibility applied, including new body placements.
    pub fn preview_visible_world(&self, commands: &[(String, Value)]) -> Result<Preview> {
        let visible = |session: &Session| -> Result<Arc<solidcraft_doc::ModelState>> {
            let world = session.world_state()?;
            let mut shown = (*world).clone();
            shown.bodies.retain(|body| session.body_visible(&body.name));
            Ok(Arc::new(shown))
        };
        let (after, replies) = self.scratch().preview_in_place(commands, None)?;
        Ok(Preview { before: visible(self)?, after: visible(&after)?, replies })
    }

    /// Run `commands` on a [`Session::scratch`] copy that keeps the undo and redo stacks: what
    /// they evaluate lands in the evaluation cache the copy shares with this session, so running
    /// them here afterwards finds it there. For a front end that computes a command which may
    /// take a while (undo, the history marker moved) off its window thread first.
    pub fn ahead(&self) -> Ahead {
        let mut copy = self.scratch();
        copy.undo.clone_from(&self.undo);
        copy.redo.clone_from(&self.redo);
        copy.model.keep(Some(Arc::default()));
        Ahead(copy)
    }

    fn preview_in_place(mut self, commands: &[(String, Value)], step: Option<&std::sync::atomic::AtomicUsize>) -> Result<(Self, Vec<Value>)> {
        if commands.is_empty() {
            return Err(EngineError::Other("nothing to preview".into()));
        }
        let mut replies = Vec::with_capacity(commands.len());
        for (i, (id, params)) in commands.iter().enumerate() {
            if let Some(step) = step {
                step.store(i + 1, std::sync::atomic::Ordering::Relaxed);
            }
            let r = self.execute(id, params)?;
            replies.push(r.clone());
            if id == "timeline.redefine"
                && let Some(fid) = r.get("feature").and_then(Value::as_u64)
                && let Some(idx) = self.doc.feature_index(fid)
                && self.doc.marker.is_some_and(|m| m <= idx)
            {
                self.doc_mut().marker = Some(idx + 1);
                self.refresh();
                if let Some(e) = self.model.result(fid).and_then(|r| r.error.clone()) {
                    return Err(EngineError::Other(e));
                }
            }
        }
        Ok((self, replies))
    }

    /// Measure one or two selections (see `inspect.measure` with `items`); errors come back as
    /// `{"error": …}`.
    pub fn measure_items(&self, items: &[Sel]) -> Value {
        let v = serde_json::to_value(items).unwrap_or(Value::Null);
        cmd::measure_items(self, &v).unwrap_or_else(|e| serde_json::json!({ "error": e.to_string() }))
    }

    /// Run a script: a JSON array of `{"command": id, "params": {...}}` (or an object with a
    /// `commands` array). Stops at the first error. Returns each command's result.
    pub fn run_script(&mut self, script: &Value) -> Result<Vec<Value>> {
        Ok(self.run_script_with(script, None, &mut |_, _| {})?.0)
    }

    /// [`Session::run_script`], calling `each(index, result)` as each command succeeds, so a
    /// caller sees the steps before a later one fails.
    pub fn run_script_each(&mut self, script: &Value, each: &mut dyn FnMut(usize, &Value)) -> Result<Vec<Value>> {
        Ok(self.run_script_with(script, None, each)?.0)
    }

    /// [`Session::run_script`], timing each command (#1833): the results and
    /// `{"timings": [{"index", "command", "ms", "profile"?}], "total_ms"}`, with `profile` the
    /// kernel's share of each command ([`timing`]). `each` is called as in
    /// [`Session::run_script_each`].
    pub fn run_script_timed(&mut self, script: &Value, profile: bool, each: &mut dyn FnMut(usize, &Value)) -> Result<(Vec<Value>, Value)> {
        let (results, timings) = self.run_script_with(script, Some(profile), each)?;
        Ok((results, timings.unwrap_or(Value::Null)))
    }

    fn run_script_with(&mut self, script: &Value, profile: Option<bool>, each: &mut dyn FnMut(usize, &Value)) -> Result<(Vec<Value>, Option<Value>)> {
        let list = match script {
            Value::Array(a) => a.clone(),
            Value::Object(o) => {
                o.get("commands").and_then(Value::as_array).cloned().ok_or_else(|| EngineError::Other("script: missing `commands` array".into()))?
            }
            _ => return Err(EngineError::Other("script must be a JSON array or {\"commands\": [...]}".into())),
        };
        if list.len() > 100_000 {
            return Err(EngineError::Other("script too long".into()));
        }
        let mut out = Vec::new();
        let mut steps = Vec::new();
        let clock = timing::Clock::start();
        for (i, c) in list.iter().enumerate() {
            let id = c
                .get("command")
                .or_else(|| c.get("id"))
                .and_then(Value::as_str)
                .ok_or_else(|| EngineError::Other(format!("script step {}: missing `command`", i + 1)))?;
            let p = c.get("params").cloned().unwrap_or(Value::Null);
            let r = match profile {
                Some(profile) => {
                    let (r, step) = timing::time(profile, || self.execute(id, &p));
                    steps.push(step.to_json(i, id));
                    r
                }
                None => self.execute(id, &p),
            };
            let r = r.map_err(|e| EngineError::Other(format!("step {} ({id}): {e}", i + 1)))?;
            each(i, &r);
            out.push(r);
        }
        let timings = profile.map(|_| serde_json::json!({"timings": steps, "total_ms": timing::round_ms(clock.ms())}));
        Ok((out, timings))
    }
}

#[cfg(test)]
mod boolean_regression_tests;
#[cfg(test)]
mod computed_commands_tests;
#[cfg(test)]
mod construct_tests;
#[cfg(test)]
mod delete_face_tests;
#[cfg(test)]
mod derived_units_tests;
#[cfg(test)]
mod display_mesh_tests;
#[cfg(test)]
mod exact_tool_tests;
#[cfg(test)]
mod extent_tests;
#[cfg(test)]
mod face_name_tests;
#[cfg(test)]
mod hole_face_tests;
#[cfg(test)]
mod hole_matrix_tests;
#[cfg(test)]
mod hole_reference_tests;
#[cfg(test)]
mod large_assembly_tests;
#[cfg(test)]
mod measure_centre_tests;
#[cfg(test)]
mod naming_history_tests;
#[cfg(test)]
mod pattern_suppress_tests;
#[cfg(test)]
mod profile_curves_tests;
#[cfg(test)]
mod profile_fallback_tests;
#[cfg(test)]
mod profile_refs_tests;
#[cfg(test)]
mod sketch_edit_removed_tests;
#[cfg(test)]
mod snap_tests;
#[cfg(test)]
mod split_face_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod thread_tests;
#[cfg(test)]
mod view_bounds_tests;
#[cfg(test)]
mod world_placement_cache_tests;

/// Per sketch feature: its parameter-driven dimensions' values (see [`own_values`]).
pub(crate) type SketchValues = std::collections::BTreeMap<u64, std::collections::BTreeMap<String, f64>>;

/// The values of a sketch's driving dimensions that come from parameters: what a value edit
/// changes in it (as opposed to geometry it projects or links from upstream).
fn own_values(sk: &solidcraft_sketch::Sketch) -> std::collections::BTreeMap<String, f64> {
    let mut values: std::collections::BTreeMap<_, _> =
        sk.constraints.iter().filter(|c| c.param.is_some() && !c.driven).filter_map(|c| Some((c.id.clone(), c.kind.value()?))).collect();
    for p in &sk.patterns {
        use solidcraft_sketch::SketchPatternKind;
        let inputs = match p.kind {
            SketchPatternKind::Circular { count, angle, .. } => vec![("count", count as f64), ("angle", angle)],
            SketchPatternKind::Rectangular { count, spacing, count2, spacing2, .. } => {
                vec![("count", count as f64), ("spacing", spacing), ("count2", count2 as f64), ("spacing2", spacing2)]
            }
        };
        for (key, value) in inputs {
            if p.expressions.contains_key(key) || (key == "count" && p.count_expression.is_some()) {
                values.insert(format!("{}.{}", p.id, key), value);
            }
        }
    }
    values
}

/// Capabilities are fixed at registration; availability is live device/host state.
fn image_host_availability(host: Option<&render::job::ImageHost>, expected: &render::ExecutorId) -> render::Availability {
    match host.map(render::job::ImageHost::descriptor) {
        Some(Ok(current)) if current.id == *expected => current.availability,
        Some(Ok(_)) => render::Availability::Unavailable { reason: "image executor identity changed after registration".into() },
        Some(Err(error)) => render::Availability::Unavailable { reason: format!("image executor status unavailable: {error}") },
        None => render::Availability::Unavailable { reason: "image host is unavailable".into() },
    }
}

/// Whether two designs differ at most in their features. Every field is named, so a new one has
/// to be decided on here.
fn same_but_features(a: &Document, b: &Document) -> bool {
    let Document {
        render,
        format,
        format_pending,
        name,
        units,
        metadata,
        params,
        text_params,
        features: _,
        marker,
        next_id,
        reserved_body_names,
        components,
        occurrences,
        assembly,
        sheet,
        plastic,
        parts,
        links,
        configs,
        favorites,
        canvases,
        point_clouds,
        param_comments,
        materials,
        print_faces,
        user_materials,
        appearances,
        body_components,
        body_offsets,
        browser_groups,
        timeline_groups,
        browser_order,
        named_views,
        visibility,
        next_component_color_index,
    } = a;
    *render == b.render
        && *format == b.format
        && *format_pending == b.format_pending
        && *name == b.name
        && *units == b.units
        && *metadata == b.metadata
        && *params == b.params
        && *text_params == b.text_params
        && *marker == b.marker
        && *next_id == b.next_id
        && *reserved_body_names == b.reserved_body_names
        && *components == b.components
        && *occurrences == b.occurrences
        && *assembly == b.assembly
        && *sheet == b.sheet
        && *plastic == b.plastic
        && *parts == b.parts
        && *links == b.links
        && *configs == b.configs
        && *favorites == b.favorites
        && *canvases == b.canvases
        && *point_clouds == b.point_clouds
        && *param_comments == b.param_comments
        && *materials == b.materials
        && *print_faces == b.print_faces
        && *user_materials == b.user_materials
        && *appearances == b.appearances
        && *body_components == b.body_components
        && *body_offsets == b.body_offsets
        && *browser_groups == b.browser_groups
        && *timeline_groups == b.timeline_groups
        && *browser_order == b.browser_order
        && *named_views == b.named_views
        && *visibility == b.visibility
        && *next_component_color_index == b.next_component_color_index
}
