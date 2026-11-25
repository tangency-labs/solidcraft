//! The SolidCraft engine.
//!
//! Every user-visible action is a command with a stable, dotted, lower-case id
//! (`solid.extrude`, `sketch.create`, `solid.fillet`, `timeline.rollback`). Commands take JSON
//! parameters and never open dialogs; the UI, the command palette, scripts, the CLI and the
//! control channel all run the same commands through [`Session::execute`].
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod agent_setup;
pub mod agent_tools;
pub mod batch_params;
pub mod batch_preflight;
pub mod browser_find;
pub mod capture;
pub mod cmd;
pub mod control_auth;
pub mod control_pending;
pub mod cosmetic_threads;
pub mod crash;
pub mod drawing_balloon_paper;
pub mod drawing_dimension_paper;
pub mod drawing_note_paper;
pub mod drawing_picture;
pub mod drawing_pictures;
pub mod drawing_revision_paper;
pub mod drawing_scene;
mod drawing_table_paper;
pub mod drawing_view;
pub mod easyprint;
mod edit_start;
mod failure_codes;
pub mod frames;
pub mod fuzz;
#[cfg(not(target_arch = "wasm32"))]
pub mod gpg;
pub mod licences;
mod mesh_lines;
pub mod occurrences;
pub mod origins;
pub mod panels;
pub mod params;
pub mod pick_pieces;
pub mod plugin;
pub mod point_cloud_pick;
pub mod product_scene;
pub mod recovery;
pub mod render_jobs;
pub mod render_lights;
pub mod render_preparation;
pub mod report_issue;
pub mod sample;
pub mod schema;
pub mod selection_context;
#[cfg(not(target_arch = "wasm32"))]
pub mod slicer_files;
mod slicer_icon;
pub mod slicers;
pub mod snap;
pub mod status_details;
pub mod status_items;
pub mod timing;
pub mod title_block_text;
pub mod upright_image;
#[cfg(feature = "usage-stats")]
pub mod usage;
pub mod view;
mod view_context;

use solidcraft_geom::prelude::*;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore_record::Vec3;
use solidcraft_doc::drawing::Owner;
use solidcraft_doc::{Document, Model, ModelState};

pub use cmd::Addition;
pub use cmd::CommandInfo;
pub use cmd::CommandSpec;
pub use cmd::FromSelection;
pub use cmd::LinkProblem;
pub use cmd::LinkState;
pub use cmd::OpenSteps;
pub use cmd::addition;
pub use cmd::auto_operation;
pub use cmd::command_specs;
pub use cmd::dimension_roles;
pub use cmd::drawing_export::print_pdf;
pub use cmd::extrude_reach;
pub use cmd::find_command;
pub use cmd::normalize_command_id;
pub use cmd::resolve_design_links;
pub use cmd::settings::MAX_PROFILE_NAME;
pub use cmd::settings::profile_name;
pub use cmd::shown_scope;
pub use cmd::sketch_selection;
pub use cmd::standard_view;
pub use cmd::use_import_cache_dir;
pub use edit_start::EditStart;
pub use edit_start::RefreshedSketchModel;
pub use edit_start::SketchDomain;
pub use edit_start::SketchFinishEvent;
pub use solidcraft_doc as doc;
pub use solidcraft_doc::Visibility;
pub use solidcraft_geom as geom;
pub use solidcraft_geom::note;
pub use solidcraft_io as io;
pub use solidcraft_kernel as kernel;
pub use solidcraft_profile as profile;
pub use solidcraft_render as render;
pub use solidcraft_sketch as sketch;
pub use view_context::ViewContext;
pub use view_context::ViewDescriptor;

/// Longest accepted request line on the JSON-lines channels (MCP over stdio and the control
/// server over loopback), so both refuse runaway input at the same size.
pub const MAX_LINE: usize = 16 << 20;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("{text}", text = panels::commands::error_text(.0.clone()))]
    Panel(#[from] panels::PanelError),
    #[error("unknown command `{0}`{hint}", hint = cmd::did_you_mean(.0))]
    UnknownCommand(String),
    #[error("`{0}` is not available right now: {1}")]
    Disabled(String, String),
    #[error("invalid parameters for `{cmd}`: {msg}")]
    BadParams { cmd: String, msg: String },
    #[error("{0}")]
    Other(String),
    /// A feature that failed with a kernel call's error: its error, as [`EngineError::Other`]
    /// words it, and the call's stable code ([`solidcraft_kernel::failure`]).
    #[error("{}", .0.error)]
    Feature(Box<FeatureFailure>),
    #[error("plugin: {0}")]
    Plugin(#[from] craft_plugin::HostError),
    /// A plugin failed while it ran or was refused before it ran: named, with its file.
    #[error("{}", .0.text())]
    PluginFailed(Box<plugin::PluginFailure>),
    /// The host's pre-command check ([`Session::set_command_check`]) refused the command; the
    /// message is the reason it gave.
    #[error("{reason}")]
    Refused { cmd: String, reason: String },
    /// A command panicked; the document was kept as it was (a bug: please report it).
    #[error("internal error in `{0}` (the design was kept as it was): {1}")]
    Internal(String, String),
    /// The work running the command was cancelled ([`solidcraft_doc::progress::Progress::cancel`])
    /// while it ran: nothing was applied, whatever was computed before the cancel came in.
    #[error("`{0}` was cancelled; nothing was applied")]
    Cancelled(String),
}

impl From<solidcraft_doc::DocError> for EngineError {
    fn from(e: solidcraft_doc::DocError) -> Self {
        match e {
            solidcraft_doc::DocError::Kernel(e) => e.into(),
            e => EngineError::Other(e.to_string()),
        }
    }
}
impl From<solidcraft_sketch::SketchError> for EngineError {
    fn from(e: solidcraft_sketch::SketchError) -> Self {
        EngineError::Other(e.to_string())
    }
}
impl From<solidcraft_kernel::KernelError> for EngineError {
    fn from(e: solidcraft_kernel::KernelError) -> Self {
        match e {
            // A panic the kernel caught: internal, as one in the command would be. The command's id
            // is filled in where the command is run (`Session::execute`).
            solidcraft_kernel::KernelError::Internal(m) => EngineError::Internal(String::new(), m),
            e => EngineError::Other(e.to_string()),
        }
    }
}

impl EngineError {
    /// The plugin host's error underneath, for a program to tell refusals apart.
    pub fn host_error(&self) -> Option<&craft_plugin::HostError> {
        match self {
            EngineError::Plugin(error) => Some(error),
            EngineError::PluginFailed(failure) => Some(&failure.error),
            _ => None,
        }
    }
    /// The error a failed feature's result reports: internal when the kernel caught a panic,
    /// with the kernel call's code when the result has one.
    pub(crate) fn of_feature(r: &solidcraft_doc::FeatureResult, error: String) -> EngineError {
        match &r.error_code {
            _ if r.internal => EngineError::Internal(String::new(), error),
            Some(code) => {
                failure_codes::note(&error, code);
                EngineError::Feature(Box::new(FeatureFailure { error, code: code.clone() }))
            }
            None => EngineError::Other(error),
        }
    }
    /// The stable code of the kernel call this error was made from, when it was made from one: a
    /// failed feature's ([`EngineError::Feature`]).
    pub fn error_code(&self) -> Option<&solidcraft_kernel::ErrorCode> {
        match self {
            EngineError::Feature(f) => Some(&f.code),
            _ => None,
        }
    }
}

/// A feature that failed with a kernel call's error ([`EngineError::Feature`]).
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureFailure {
    /// The feature's error, as its result words it.
    pub error: String,
    /// The kernel call's stable code.
    pub code: solidcraft_kernel::ErrorCode,
}
impl From<solidcraft_io::IoError> for EngineError {
    fn from(e: solidcraft_io::IoError) -> Self {
        EngineError::Other(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, EngineError>;

pub use failure_codes::failure_code_of;

/// Why [`Session::drawn`] gives no design for a drawing's source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Undrawn {
    /// The drawing has no source of that id.
    NoSource,
    /// The source is another design's file, which a drawing can't read yet.
    Linked,
}

impl std::fmt::Display for Undrawn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Undrawn::NoSource => f.write_str("the drawing has no such source"),
            Undrawn::Linked => f.write_str("not supported yet: reading a linked design (#3890)"),
        }
    }
}

/// What a click on the model picks while no dialog or tool takes the pick: the window's selection
/// mode (#4255, `select.mode`). The selection filter narrows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PickMode {
    /// Faces, edges and vertices.
    #[default]
    Faces,
    /// The body of the face clicked.
    Bodies,
    /// The feature that made the face clicked.
    Features,
    /// The innermost placement holding the body clicked.
    Parts,
    /// The top-level placement holding the body clicked.
    Components,
    /// Sketch curves, points and profiles only.
    Sketch,
}

impl PickMode {
    pub const ALL: [PickMode; 6] = [PickMode::Faces, PickMode::Bodies, PickMode::Features, PickMode::Parts, PickMode::Components, PickMode::Sketch];

    /// A click on a face picks a whole object (its body, feature or placement).
    pub fn whole(self) -> bool {
        matches!(self, PickMode::Bodies | PickMode::Features | PickMode::Parts | PickMode::Components)
    }
}

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
    /// A view, dimension or balloon of the drawing open.
    Drawing {
        #[serde(with = "solidcraft_doc::ids")]
        drawing: u64,
        #[serde(with = "solidcraft_doc::ids")]
        object: u64,
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
    /// A point of a component sketch, picked on another copy of its part than the one the sketch
    /// is edited at: the copy's placement (occurrence ids from the root), as a joint's origin
    /// takes it.
    PlacedSketchPoint {
        sketch: u64,
        id: String,
        path: Vec<u64>,
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
    /// A sampled point of an imported point cloud (#8933).
    PointCloudPoint {
        cloud: u64,
        index: usize,
        point: Vec3,
    },
}

impl Sel {
    /// The body this item names, when it names one: the body itself, or the body a face, edge
    /// or vertex belongs to. Session state that names a body has to follow its renames (#9737).
    pub(crate) fn body_name(&self) -> Option<&str> {
        match self {
            Sel::Body { name } => Some(name),
            Sel::Face { body, .. } | Sel::Edge { body, .. } | Sel::Vertex { body, .. } if !body.is_empty() => Some(body),
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
    pub fn of(session: &Session, bodies: Vec<String>, placements: Vec<Vec<u64>>) -> Result<ForeignCopy> {
        let frames = session.placed_document().body_frames_with_source(&session.model.state());
        let names = bodies.iter().map(|b| frames.iter().find(|f| f.0 == *b).map_or_else(|| b.clone(), |f| f.1.clone())).collect();
        let placed = placements.iter().filter_map(|p| session.occurrence_placement(p).ok()).map(|p| (p.component, p.occurrence, p.world)).collect();
        Ok(ForeignCopy {
            design: session.doc.name.clone(),
            features: Vec::new(),
            world: session.world_state()?,
            bodies,
            names,
            placements,
            placed,
            doc: session.doc.clone(),
            model: session.model.clone(),
            pending_moves: session.pending_moves.clone(),
        })
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

/// A drawing-sketch editing entry. Its owner, a sheet or a view of the drawing, is never a model
/// feature id. Original geometry is only the cancel boundary; Session keeps the one undo history.
#[derive(Clone, Debug, PartialEq)]
pub struct SheetSketchEdit {
    pub drawing: u64,
    pub owner: Owner,
    /// Stable host Sheet tile, independent of the drawing view the geometry belongs to.
    /// Headless sessions have no tile identity.
    pub view: Option<u64>,
    pub sketch: u64,
    pub(crate) original: Option<Arc<solidcraft_doc::drawing::SheetSketch>>,
}

impl SheetSketchEdit {
    /// The sheet displayed by the editing tile, including sketches owned by a drawing view.
    pub fn sheet(&self, document: &Document) -> Option<u64> {
        let drawing = document.drawing(self.drawing)?;
        match self.owner {
            Owner::Sheet(sheet) => drawing.sheets.iter().any(|record| record.id == sheet).then_some(sheet),
            Owner::View(view) => drawing.view(view).map(|record| record.sheet),
        }
    }

    /// Whether the sketch being edited is among `gone`, taken out of drawing `drawing`: a delete
    /// that would take it waits until it is finished or cancelled.
    pub(crate) fn goes_with(&self, drawing: u64, gone: &solidcraft_doc::drawing::OnSheet) -> bool {
        self.drawing == drawing && gone.sketches.binary_search(&self.sketch).is_ok()
    }
}

/// One undo step.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub label: String,
    pub doc: Arc<Document>,
    pub active_sketch: Option<u64>,
    pub sketching_view: Option<u64>,
    /// Temporary editor position to finalize if its former view has since closed.
    pub sketch_restore_marker: Option<Option<usize>>,
    /// Explicitly owned sheet edit, independent of the model sketch being edited.
    pub active_sheet_sketch: Option<Arc<SheetSketchEdit>>,
    pub pending_moves: std::collections::BTreeMap<u64, solidcraft_doc::Mat>,
    /// What was hidden or shown then (`browser.visibility`). It is saved with the design, so
    /// hiding a body is undone and redone like any other change.
    pub visibility: Visibility,
    /// The part of the design the step changed: a drawing (by id), or the model and the list of
    /// drawings (`None`). Undo and redo go through one part's steps (`edit.undo`).
    pub part: Option<u64>,
    /// The Section Analysis before a step that changed it (none: the step left it alone).
    pub section: Option<SectionState>,
}

impl Snapshot {
    /// The session as it is now, as a step of the model, under `label`: the one place a step is
    /// taken.
    pub fn of(s: &Session, label: &str) -> Snapshot {
        Snapshot {
            label: label.to_string(),
            doc: s.doc.clone(),
            active_sketch: s.active_sketch,
            sketching_view: s.sketching_view,
            sketch_restore_marker: s.sketch_restore_marker(),
            active_sheet_sketch: s.active_sheet_sketch.clone(),
            pending_moves: s.pending_moves.clone(),
            visibility: s.visibility.clone(),
            part: None,
            section: None,
        }
    }
}

/// A Section Analysis as an undo step keeps it. View state, never saved with the design.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SectionState {
    pub planes: Vec<SectionPlane>,
    pub hidden: Vec<SectionPlane>,
    pub hatch: bool,
    pub folder: Option<Vec<u64>>,
}

/// A joint drag's frame, drawn before the drag applies it (#6325): the transforms (in their
/// parents) its solve gave the occurrences it moved, which the world state places them by.
/// Running `joint.reach` on every pointer move evaluated, re-placed and redrew the whole design
/// and pushed an undo step each frame; the drag now solves on a copy of the design and shows the
/// result here, and its release applies the last frame as one `joint.reach`. It is no part of
/// the design, its history or a save, and stands only while the design is the one it was
/// worked out for: any change to the design leaves it out.
#[derive(Clone, Debug)]
pub struct DragPreview {
    /// The design the frame was worked out for.
    pub design: Arc<Document>,
    /// The occurrences the frame moved, and where it moved them (in their parents).
    pub moves: std::collections::BTreeMap<u64, solidcraft_doc::Mat>,
}

/// Where the open sketch was entered (`sketch.create`, `sketch.edit`): the design before it and
/// the undo history then, so `sketch.cancel` can tell the entry still describes the history.
#[derive(Clone)]
pub(crate) struct SketchEntry {
    pub(crate) sketch: u64,
    pub(crate) doc: Arc<Document>,
    pub(crate) undo_len: usize,
    pub(crate) undo_top: Option<Arc<Document>>,
    /// A temporary editor rollback, restored by the engine even without a window.
    pub(crate) restore_marker: Option<Option<usize>>,
    pub(crate) pending_moves: std::collections::BTreeMap<u64, solidcraft_doc::Mat>,
    pub(crate) visibility: Visibility,
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
        let stop = self.0.stop.clone();
        stop.within(|| {
            let (after, _) = self.0.preview_in_place(commands, Some(step))?;
            // Stopped by its watcher: nothing it ends with will be shown.
            if !solidcraft_doc::progress::cancelled() {
                after.world_state()?.warm_display();
            }
            Ok(Evaluated(after.model))
        })
    }

    /// Run `commands` one after another, each reply or error in turn (what they evaluate stays
    /// in the shared evaluation cache): the checks a front end makes before a command, such as
    /// what a delete takes with it (#2678), computed off its window thread.
    pub fn replies(mut self, commands: &[(String, Value)]) -> Vec<Result<Value>> {
        let stop = self.0.stop.clone();
        stop.within(|| commands.iter().map(|(id, params)| self.0.execute(id, params)).collect())
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
    /// Sketches shown after the commands, using the same visibility rule as Home/Fit.
    /// The models retain hidden sketches for geometry comparisons and editing previews.
    pub shown_sketches: std::collections::BTreeSet<u64>,
    /// What each command replied (as `execute` returns it).
    pub replies: Vec<Value>,
    /// The joints the commands posed, with their effective values now: the ones they drove and
    /// the ones they moved (a motion link's follower, a joint a closed loop moved). Their values
    /// are as the solver placed them, after motion links and limits, as `joint.inspect` and
    /// `joint.drive` give them. Empty when the commands posed no joint.
    pub joints: Vec<crate::schema::joints::ReachedJoint>,
    /// The document after the commands: for what a front end works out of a previewed feature
    /// (Frame Member's centrelines).
    pub doc: Arc<Document>,
    /// The evaluation of `doc`, which gives each feature's state before it (`Model::state_before`).
    pub model: Model,
}

impl Preview {
    fn new(before: Arc<ModelState>, after: Arc<ModelState>, source: &Session, result: &Session, replies: Vec<Value>) -> Self {
        let shown_sketches = after.sketches.iter().filter(|sketch| view::sketch_shown(result, sketch.feature)).map(|sketch| sketch.feature).collect();
        let driven = replies.iter().filter_map(|reply| reply.get("joint").and_then(Value::as_u64));
        let joints = cmd::joints::posed_joints(&source.doc, &result.doc, driven);
        Self { before, after, shown_sketches, replies, joints, doc: result.doc.clone(), model: result.model.clone() }
    }

    fn visible_world(before: &Session, after: &Session, replies: Vec<Value>) -> Result<Self> {
        let visible = |session: &Session| -> Result<Arc<ModelState>> {
            let world = session.world_state()?;
            let mut shown = (*world).clone();
            shown.bodies.retain(|body| session.body_visible(&body.name));
            Ok(Arc::new(shown))
        };
        Ok(Self::new(visible(before)?, visible(after)?, before, after, replies))
    }
}

/// Prepared world geometry depends on design/model identity and placement context, not
/// selection or the view revision. Retained Arcs prevent pointer reuse after a design closes.
#[derive(Clone)]
struct WorldCache {
    doc: Arc<Document>,
    model: Arc<solidcraft_doc::ModelState>,
    pending_moves: std::collections::BTreeMap<u64, solidcraft_doc::Mat>,
    drag_moves: std::collections::BTreeMap<u64, solidcraft_doc::Mat>,
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
            && &self.drag_moves == session.drag_moves()
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
        session.same_document_context(source)
            && session.checked_sketch_owner(self.computed.active_sketch, self.computed.sketching_view).is_ok()
            && session.checked_sheet_sketch_owner(&session.view_context, self.computed.active_sheet_sketch.as_deref(), &self.computed.doc).is_ok()
            && self.selection_is_current(session)
            && (!self.steps.iter().any(|step| {
                step.id.eq_ignore_ascii_case("plugin.run")
                    || step.id.split_once('.').is_some_and(|(package, _)| source.plugin_state.packages.contains_key(package))
            }) || (session.plugin_limits == source.plugin_limits
                && session.disabled_plugins == source.disabled_plugins
                && session.plugin_state.same_execution_context(&source.plugin_state)))
    }

    /// Whether the resulting document differs in content from its source.
    pub fn changes_document(&self) -> bool {
        self.source.doc != self.computed.doc
    }

    /// Show the prepared model in occurrence coordinates, with visibility applied, and return
    /// the replies from its evaluation.
    pub fn preview_visible_world(&self) -> Result<Preview> {
        Preview::visible_world(&self.source, &self.computed, self.steps.iter().map(|step| step.reply.clone()).collect())
    }

    fn selection_is_current(&self, session: &Session) -> bool {
        if session.selection == self.source.selection && session.selection_all_bodies == self.source.selection_all_bodies {
            return true;
        }
        // A Model transaction keeps its captured picks when focus moves to the retained
        // Drawing. That receiver's new picks neither change the Model inputs nor belong to
        // the worker. Drawing/mixed transactions and same-receiver changes stay strict.
        (self.source.model_focus || self.source.open_drawing.is_none())
            && !session.model_focus
            && session.open_drawing.is_some()
            && self.steps.iter().all(|step| crate::find_command(&step.id).is_some_and(|spec| spec.part == cmd::Part::Model))
            && session.selection.iter().all(|item| match item {
                Sel::Drawing { drawing, object } if Some(*drawing) == session.open_drawing => {
                    session.doc.drawing(*drawing).is_some_and(|record| record.has_object(*object))
                }
                _ => false,
            })
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
/// Only redo owners relevant to a scratch's parent. Keys are fixed when the scratch
/// is made; they do not grow as unrelated drawings are edited or history is evicted.
type RedoWatch = std::collections::BTreeMap<Option<u64>, bool>;

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
    /// Scratch-only owner provenance, independent of the bounded undo vector.
    /// Ordinary sessions keep None; adoption never installs a child's watch map.
    redo_watch: Option<RedoWatch>,
    /// Sketch feature being edited (sketch mode).
    pub active_sketch: Option<u64>,
    pub sketching_view: Option<u64>,
    /// Live host view identities; never restored from history or a worker snapshot.
    pub view_context: ViewContext,
    sketch_finish_events: std::collections::VecDeque<SketchFinishEvent>,
    sketch_finish_sequence: u64,
    sketch_finish_pending: bool,
    /// The sketch a person entered from the UI (not a command): finishing it empty deletes it.
    sketch_by_person: Option<u64>,
    edit_source_sketch: Option<u64>,
    prepared_edit_source: Option<(String, SketchFinishEvent)>,
    edit_entry_depth: u32,
    edit_start_sequence: u64,
    prepared_edit_intent: Option<String>,
    /// Explicitly owned sheet edit, independent of the model sketch being edited.
    pub active_sheet_sketch: Option<Arc<SheetSketchEdit>>,
    /// The drawing open (by id): drawing commands change it. The model's commands work with it
    /// open too ([`cmd::Part`]); the views they leave out of date wait for the drawing's Update.
    pub open_drawing: Option<u64>,
    /// The host shows the model (the app's Design and Render): undo, redo and the shared sketch
    /// tools take the model's part even with a drawing open or a sheet sketch entered in it, so
    /// each workspace works on its own part and none holds another. Off (the default), they take
    /// the open drawing's ([`Session::history_part`], [`Session::sheet_sketching`]).
    pub model_focus: bool,
    /// The design as it was when the open sketch was entered, for Cancel Sketch.
    pub(crate) sketch_entry: Option<SketchEntry>,
    /// Sketch temporarily shown while editing a feature that consumes it. View state only.
    pub feature_edit_sketch: Option<u64>,
    pub selection: Vec<Sel>,
    /// Every shown body of the design is selected: "select all" in the body scope is a *scope*
    /// rather than a list of 56,100 names. [`Session::selected_items`] resolves it where an
    /// operation needs the bodies; code that only asks whether anything is selected reads
    /// [`Session::has_selection`]. Any explicit select replaces the scope.
    pub selection_all_bodies: bool,
    pub path: Option<String>,
    /// The file of the design this session is a scratch copy of ([`Session::scratch`]): the copy's
    /// own `path` stays `None` so nothing run on it saves over that file. Read only by `doc.info`;
    /// never saved, never adopted. `None` in a session that is not a copy.
    pub(crate) source_path: Option<String>,
    /// Where the drawing commands' object ids come from: one source for the session, kept across
    /// a file change, so a session seeded alike and run alike gives the same ids
    /// ([`Session::seed_ids`]). A failed command draws none.
    pub(crate) ids: solidcraft_doc::ids::IdSource,
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
    /// The provenance records the open file carries; a signed save appends one to them.
    pub provenance_chain: Option<solidcraft_io::ProvenanceChain>,
    /// The last provenance verification of this design, when one was asked for.
    pub provenance: Option<solidcraft_io::ProvenanceReport>,
    /// Set only by the app's own Save, after the user chose the key in Preferences: a script,
    /// agent or control-channel caller cannot make the engine sign with a keyring key on its own.
    /// Not saved and not copied by [`Session::scratch`].
    pub signing_allowed: bool,
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
    /// Revision increments caused only by what is shown ([`Session::refresh_display`]).
    /// Comparing both counters proves that every intervening increment left an editor's
    /// document and model untouched.
    visibility_revision: u64,
    /// [`Document::browser_stamp`] when the browser's groups and order were last checked.
    browser_stamp: [u64; 6],
    /// The open-session design identity for programs that follow it (`document.revision`).
    pub document_id: String,
    /// The current plugin revision, outside authored data and undo snapshots.
    design_revision: String,
    /// The immutable authored state named by `design_revision`. A weak association
    /// retains no snapshot or document contents. Mutation invalidates it through
    /// copy-on-write; automatic refresh, pinning and save-name changes may retarget
    /// an equivalent representation without allocating another revision.
    design_state: std::sync::Weak<Document>,
    /// The design as `document.revision` last saw it, and the revision it gave it.
    pub(crate) followed: Option<cmd::inspect::Followed>,
    /// Project a face's edges into a sketch created on it (the default; `sketch.auto_project`).
    pub auto_project: bool,
    /// Project, Intersect, Include 3D Geometry and the face edges projected into a new sketch
    /// make construction geometry rather than normal geometry (off by default; #13142). A
    /// preference, kept like `auto_project` when another design is opened or started.
    pub project_construction: bool,
    /// Host preferences; never saved in a design.
    pub disabled_plugins: std::collections::BTreeSet<String>,
    /// User resource ceilings; never loaded from or saved in a design.
    pub plugin_limits: plugin::HostLimits,
    /// Trusted package locations and folder grants, kept outside the design.
    pub plugin_state: plugin::AppState,
    /// Trusted UI invocation context; never a command parameter or a saved preference.
    pub plugin_forms_interactive: bool,
    /// How calls that break a command's parameter contract are treated (off by default). It
    /// belongs to the session, not the design, so it survives opening and starting designs.
    pub param_check: schema::ParamCheck,
    /// How the host uploads a model to EasyPrint (`file.open_in_easyprint`); `None`: it can't. It
    /// belongs to the session like `param_check`, so it survives opening and starting designs.
    pub easyprint_upload: Option<EasyPrintUpload>,
    /// Whether print faces (`body.print_face`) are saved with the design and kept when one is
    /// opened (#11512). On by default, so scripts and the command line keep them; the app sets it
    /// from its "Keep print faces" preference, which is off unless the user turns it on. Off: a
    /// save leaves them out of the file and an opened design starts without them. The user's,
    /// like `param_check`, so it survives opening and starting designs.
    pub keep_print_faces: bool,
    /// Whether a save keeps the bodies of linked designs in the design's file. On for a new
    /// design. Off: `file.save` writes each link without its bodies, and opening the file reads
    /// them from the linked design files again (#8170). The design's own choice, set by
    /// `file.save`'s `linked_bodies` and carried by its file
    /// ([`Document::linked_bodies_left_out`]), so it follows the design and not the user.
    pub linked_bodies: bool,
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
    /// The design as a configuration row last put it or took it in (`config.activate`,
    /// `config.update_from_design`), with the session's document id: what
    /// `config.update_from_design` compares the design with to name the changes the table has no
    /// column for. The session's only, never saved.
    pub(crate) config_baseline: Option<(String, Arc<Document>)>,
    /// The model each of the last two configuration rows left built, by row id
    /// (`config.activate`): going back to one offers its results to the evaluation
    /// ([`solidcraft_doc::Model::prime`]), which takes each whose feature has the same inputs,
    /// instead of computing what the cache has dropped meanwhile. The session's only, never saved.
    pub(crate) row_models: Vec<(u64, Model)>,
    /// A joint drag's frame, drawn before the drag applies it (#6325): see [`DragPreview`].
    pub drag_preview: Option<DragPreview>,
    /// Stops the work computing off the window thread for this design, shared with its copies.
    stop: solidcraft_kernel::Stop,
    /// A copy of a design ([`Session::scratch`]), sharing its token.
    is_copy: bool,
    /// A design's refresh shows its results without waiting for the SMLib checks of its last
    /// features ([`Session::set_background_checks`]). Never a copy's.
    background_checks: bool,
    world_cache: std::sync::Mutex<Option<WorldCache>>,
    /// The design the joints were last solved for, as the solve left it, and the model it read
    /// (see [`Session::solve_joints`]).
    joints_settled: Option<(Arc<Document>, std::sync::Weak<solidcraft_doc::ModelState>)>,
    /// The design and model the last body-name pinning pass saw: a command that changes neither
    /// the features nor the model's results (a joint drag frame) has no new body to pin.
    names_pinned: Option<(Arc<Document>, std::sync::Weak<solidcraft_doc::ModelState>)>,
    occurrence_visibility_cache: std::sync::Mutex<Option<occurrences::VisibilityCache>>,
    /// The hidden joints, folders and bodies of the view, as sets: the browser asks about every
    /// joint of the design on every frame (#3892), the viewport about every body, and an answer
    /// that walked the hidden lists made a frame cost grow with the size of the design.
    visibility_index_cache: std::sync::Mutex<Option<occurrences::VisibilityIndex>>,
    /// The sketches a feature uses, with the design they were read from
    /// ([`view::sketch_consumed`]): the viewport asks about every sketch several times a frame,
    /// and walking the features for each made that cost grow with sketches times features.
    /// Keyed on the design itself, so an edit of any kind builds the set again.
    consumed_sketches_cache: std::sync::Mutex<Option<view::ConsumedSketches>>,
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
    /// Section Analysis: a view cut by planes, at most [`view::MAX_SECTION_PLANES`]: what lies
    /// on the normal side of any one is hidden (none: no section).
    pub section: Vec<SectionPlane>,
    /// The section's planes hidden with their eye: kept, cutting nothing (`inspect.section`'s
    /// `hide` and `show`).
    pub section_hidden: Vec<SectionPlane>,
    /// The Analysis folder hidden with its eye (`inspect.section`'s `folder: false`): the hidden
    /// planes that come back when it is shown (none: the folder is shown).
    pub section_folder: Option<Vec<u64>>,
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
    pub camera: Option<solidcraft_viewport::Camera>,
    /// Host-owned viewport metadata, never saved or included in history.
    render_viewport: Option<Arc<render::ExecutorDescriptor>>,
    render_jobs: render_jobs::Jobs,
    /// The sheet scenes of the drawing open, for the sheet page ([`drawing_scene::extract`]): view
    /// state, never saved or an undo step, dropped when the drawing closes.
    pub(crate) drawing_scenes: drawing_scene::Cache,
    /// The visibility the design file holds (what `is_dirty` compares with).
    saved_visibility: Visibility,
    /// The design as `doc.open` made it from a STEP, IGES or mesh file, never saved yet: while it
    /// is still that, closing it loses nothing the file doesn't hold ([`Session::unchanged_import`]).
    opened_import: Option<Arc<Document>>,
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
    /// Count the commands the person runs in this session for the app's usage statistics
    /// ([`usage`]): the app's own design only, like [`Session::crash_journal`].
    #[cfg(feature = "usage-stats")]
    pub count_usage: bool,
    /// The last value each drawing dimension read this session, by drawing and dimension: what a
    /// dimension that can't be measured now last measured. Only the file's copy of the document
    /// keeps it ([`as_saved`]): a read is no change to undo or to ask about saving.
    pub(crate) dimension_values: DimensionValues,
    /// Live-model evaluations after this session's initialization; never serialized.
    pub(crate) evaluations: usize,
    /// Body-name pinning passes that walked the features (`pin_body_names`), for tests.
    #[cfg(test)]
    pub(crate) body_name_passes: usize,
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

/// The design, active sketch, open drawing, pending positions, undo history and section at an `edit.checkpoint`.
#[derive(Clone, Debug)]
pub(crate) struct Checkpoint {
    pub id: u64,
    pub document_id: String,
    pub doc: Arc<Document>,
    pub active_sketch: Option<u64>,
    pub sketching_view: Option<u64>,
    /// Temporary editor position to finalize if its former view has since closed.
    pub sketch_restore_marker: Option<Option<usize>>,
    /// Explicitly owned sheet edit, independent of the model sketch being edited.
    pub active_sheet_sketch: Option<Arc<SheetSketchEdit>>,
    pub open_drawing: Option<u64>,
    pub pending_moves: std::collections::BTreeMap<u64, solidcraft_doc::Mat>,
    pub visibility: Visibility,
    pub undo: Vec<Snapshot>,
    pub redo: Vec<Snapshot>,
    redo_watch: Option<RedoWatch>,
    pub section: SectionState,
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

/// One plane of a Section Analysis: what lies on its normal's side is hidden, whatever the other
/// planes keep. View state only.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct SectionPlane {
    /// Its number in the section, from 1 (`inspect.section`'s `remove` takes it).
    pub id: u64,
    pub origin: Vec3,
    /// Unit length.
    pub normal: Vec3,
}

impl SectionPlane {
    /// The plane as (normal, d): what lies where normal·p > d is hidden.
    pub fn clip(&self) -> (Vec3, f64) {
        (self.normal, self.normal.dot(self.origin))
    }

    /// Its cut faces' hatch (an index into `solidcraft_viewport::SECTION_HATCH_ANGLES`), kept by its id.
    pub fn hatch(&self) -> usize {
        solidcraft_viewport::section_hatch_of(self.id)
    }

    /// Whether the plane keeps `p` (within `eps`).
    pub fn keeps(&self, p: Vec3, eps: f64) -> bool {
        self.normal.dot(p - self.origin) <= eps
    }
}

impl Session {
    /// The Section Analysis as an undo step keeps it.
    pub fn section_state(&self) -> SectionState {
        SectionState {
            planes: self.section.clone(),
            hidden: self.section_hidden.clone(),
            hatch: self.section_hatch,
            folder: self.section_folder.clone(),
        }
    }

    pub fn set_section_state(&mut self, state: SectionState) {
        let was_on = !self.section.is_empty();
        (self.section, self.section_hidden, self.section_hatch, self.section_folder) = (state.planes, state.hidden, state.hatch, state.folder);
        self.status_slot(crate::status_items::StatusId::Section, !self.section.is_empty());
        if !was_on && !self.section.is_empty() {
            self.status_activated(crate::status_items::StatusId::Section);
        }
        self.refresh_display();
    }
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

/// What a view shows of its design besides the camera: the section cut and the analyses. View
/// state only (not saved, not an undo step), so each view of a design keeps its own and they differ
/// while the design stays one. [`Session::inspect`] reads it and [`Session::set_inspect`] shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct InspectState {
    // Retaining the snapshot makes document mutation change its copy-on-write identity.
    source: Arc<Document>,
    pub colored_components: Option<()>,
    pub section: SectionState,
    pub analysis: Option<SurfaceAnalysis>,
    pub continuity: Vec<ContinuitySeam>,
    pub isocurves: Vec<IsoPick>,
    pub comb: Option<CombView>,
}

impl Session {
    /// What the session shows now: the section and the analyses.
    pub fn inspect(&self) -> InspectState {
        InspectState {
            source: self.doc.clone(),
            colored_components: self.colored_components,
            section: self.section_state(),
            analysis: self.analysis,
            continuity: self.continuity.clone(),
            isocurves: self.isocurves.clone(),
            comb: self.comb.clone(),
        }
    }

    /// Show `state` in place of what is shown now.
    pub fn set_inspect(&mut self, state: InspectState) {
        let current = Arc::ptr_eq(&state.source, &self.doc);
        self.colored_components = state.colored_components;
        // Passive view swaps restore the complete cut without publishing a display edit or
        // changing the shared window's status ordering on every tile paint.
        let SectionState { planes, hidden, hatch, folder } = state.section;
        (self.section, self.section_hidden, self.section_hatch, self.section_folder) = (planes, hidden, hatch, folder);
        self.analysis = state.analysis;
        // Requested analysis survives an edit; sampled world geometry does not.
        self.continuity = if current { state.continuity } else { Vec::new() };
        self.isocurves = state.isocurves;
        self.comb = state.comb.map(|mut comb| {
            if !current {
                comb.teeth.clear();
            }
            comb
        });
    }
}

impl SurfaceAnalysis {
    /// How a picture shades the faces for this analysis.
    pub fn mode(self) -> solidcraft_viewport::AnalysisMode {
        use solidcraft_viewport::AnalysisMode as M;
        match self {
            // Bars standing upright around the viewer, half white: the picture as it always was.
            SurfaceAnalysis::Zebra { stripes } => M::Zebra { stripes, direction: Vec3::new(0.0, 0.0, 1.0), width: 0.5 },
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
        // Likewise the choice to save without linked bodies: the session keeps it.
        let linked_bodies = !std::mem::take(&mut doc.linked_bodies_left_out);
        let mut doc = Arc::new(doc);
        // A model saved under another check mode holds that mode's results: evaluated again.
        let model = match Self::saved_model(&doc, saved).filter(|model| model.validation() == validation) {
            Some(model) => model,
            None => {
                // Stored body names follow the numbering of the STEP reader that inserted each
                // file; this version's reader may number the bodies otherwise (#13382). The
                // reads are held so the evaluation doesn't read the files again.
                let _reads = cmd::realign_imports(Arc::make_mut(&mut doc));
                let mut model = Model::new();
                model.set_validation(validation);
                model.evaluate_shared(&doc);
                model
            }
        };
        Session { linked_bodies, ..Self::with_model(doc, visibility, model) }
    }

    /// The model `doc`'s file kept, when it is `doc`'s evaluation and the cache is not turned off.
    /// A model evaluated with the tolerance heal preference ([`solidcraft_kernel::heals_tolerance_defects`])
    /// set the other way holds other results (its imports healed or as read, Repair healing first
    /// or not): evaluated again.
    pub(crate) fn saved_model(doc: &Arc<Document>, saved: Option<&solidcraft_doc::SavedModel>) -> Option<Model> {
        saved
            .filter(|_| solidcraft_geom::env::var_os("NO_MODEL_CACHE").is_none())
            .and_then(|saved| Model::adopt(doc, saved).map_err(|e| log::info!("the saved model is not used: {e}")).ok())
            .filter(|model| model.heals() == solidcraft_kernel::heals_tolerance_defects())
    }

    /// `doc` with each fold that keeps no side of its line that stays (a design saved before
    /// folds kept one) given the side it folds now, and `model` evaluated on it: from then on that
    /// side stays, wherever the fold's line moves. As they are where every fold keeps its side.
    fn with_fold_sides(doc: Arc<Document>, mut model: Model) -> (Arc<Document>, Model) {
        match doc.with_fold_sides(&model) {
            Some(settled) => {
                let doc = Arc::new(settled);
                model.evaluate_shared(&doc);
                (doc, model)
            }
            None => (doc, model),
        }
    }

    /// A session on `doc` evaluated as `model`, with what it hides. A design opened from before
    /// folds kept which side stays has each fold's side settled first ([`Self::with_fold_sides`]).
    pub(crate) fn with_model(doc: Arc<Document>, visibility: Visibility, model: Model) -> Self {
        let (doc, model) = Self::with_fold_sides(doc, model);
        let document_id = solidcraft_doc::ids::new_document_id();
        let design_state = Arc::downgrade(&doc);
        Session {
            stop: Default::default(),
            is_copy: false,
            background_checks: false,
            saved: doc.clone(),
            doc,
            model,
            undo: Vec::new(),
            redo: Vec::new(),
            redo_watch: None,
            active_sketch: None,
            sketching_view: None,
            view_context: ViewContext::default(),
            sketch_finish_events: Default::default(),
            sketch_finish_sequence: 0,
            sketch_finish_pending: false,
            sketch_by_person: None,
            edit_source_sketch: None,
            prepared_edit_source: None,
            edit_entry_depth: 0,
            edit_start_sequence: 0,
            prepared_edit_intent: None,
            active_sheet_sketch: None,
            open_drawing: None,
            model_focus: false,
            sketch_entry: None,
            feature_edit_sketch: None,
            selection: Vec::new(),
            selection_all_bodies: false,
            path: None,
            source_path: None,
            ids: solidcraft_doc::ids::IdSource::from_entropy(),
            uid: NEXT_UID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            disk: None,
            read_only: false,
            provenance_chain: None,
            provenance: None,
            signing_allowed: false,
            user_named: false,
            open_preview: None,
            log: Vec::new(),
            revision: 1,
            visibility_revision: 0,
            browser_stamp: [0; 6],
            document_id,
            design_revision: solidcraft_doc::ids::new_design_revision(),
            design_state,
            followed: None,
            auto_project: true,
            project_construction: false,
            disabled_plugins: Default::default(),
            plugin_limits: plugin::default_host_limits(),
            plugin_state: plugin::AppState::for_process(),
            plugin_forms_interactive: false,
            param_check: schema::ParamCheck::Off,
            easyprint_upload: None,
            keep_print_faces: true,
            linked_bodies: true,
            live_links: None,
            active_component: 0,
            active_occurrence_path: None,
            pending_moves: Default::default(),
            exploded: Default::default(),
            config_baseline: None,
            row_models: Vec::new(),
            drag_preview: None,
            world_cache: Default::default(),
            joints_settled: None,
            names_pinned: None,
            occurrence_visibility_cache: Default::default(),
            visibility_index_cache: Default::default(),
            consumed_sketches_cache: Default::default(),
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
            section: Vec::new(),
            section_hidden: Vec::new(),
            section_folder: None,
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
            opened_import: None,
            visibility,
            camera: None,
            render_viewport: None,
            render_jobs: Default::default(),
            dimension_values: Default::default(),
            drawing_scenes: Default::default(),
            evaluations: 0,
            #[cfg(test)]
            body_name_passes: 0,
            motion_traces: Default::default(),
            command_check: None,
            crash_journal: false,
            #[cfg(feature = "usage-stats")]
            count_usage: false,
        }
    }

    /// Become `fresh` (another design opened, recovered or started), keeping the settings that
    /// are the user's rather than the design's: `auto_project` is a preference (#680).
    /// Checkpoints survive too: a batch that failed after `file.new` or `doc.open` goes back
    /// from the checkpoint it took before its first step, and that checkpoint belongs to the
    /// design before the batch, which is what the batch promised (#3242). The design is what a
    /// checkpoint keeps, not the replaced session's file path (docs/mcp.md).
    pub fn replace_design(&mut self, fresh: Session) {
        // Nothing still computing for the design that leaves has anywhere to go. A copy leaves
        // the design it came from going, and its token with it.
        if !self.is_copy {
            self.stop_computing();
        }
        let auto_project = self.auto_project;
        let project_construction = self.project_construction;
        let validation = self.validation();
        let single_surface_analysis = self.single_surface_analysis;
        let disabled_plugins = self.disabled_plugins.clone();
        let plugin_limits = self.plugin_limits;
        let mut plugin_state = self.plugin_state.clone();
        for package in plugin_state.panels.values_mut() {
            for data in package.data.values_mut() {
                *data = Default::default();
            }
        }
        plugin_state.ui_caller = None;
        let param_check = self.param_check;
        let easyprint_upload = self.easyprint_upload.take();
        let keep_print_faces = self.keep_print_faces;
        let render_viewport = self.render_viewport.clone();
        let render_jobs = self.render_jobs.scratch();
        let checkpoints = std::mem::take(&mut self.checkpoints);
        let next_checkpoint = self.next_checkpoint;
        let ids = self.ids.clone();
        // The design on screen stays journaled for crash reports at once, not from the app's
        // next frame, so commands sent right after `doc.open` or `file.new` are kept.
        let crash_journal = self.crash_journal;
        #[cfg(feature = "usage-stats")]
        let count_usage = self.count_usage;
        let replacements = self.replacements.wrapping_add(1);
        let command_check = self.command_check.take();
        let previous_doc = self.doc.clone();
        let hidden = self.visibility != fresh.visibility;
        let redo_watch = self.redo_watch.take();
        *self = fresh;
        self.crash_journal = crash_journal;
        #[cfg(feature = "usage-stats")]
        {
            self.count_usage = count_usage;
        }
        self.command_check = command_check;
        self.auto_project = auto_project;
        self.project_construction = project_construction;
        // The user's, like `auto_project`: the commands that open a design evaluate it under it
        // already (`Session::opened_validating`).
        self.set_validation(validation);
        self.set_single_surface_analysis(single_surface_analysis);
        self.disabled_plugins = disabled_plugins;
        self.plugin_limits = plugin_limits;
        self.plugin_state = plugin_state;
        self.plugin_state.link_changes = Default::default();
        self.plugin_state.link_reads = Default::default();
        self.plugin_state.link_read_source = Default::default();
        self.param_check = param_check;
        self.easyprint_upload = easyprint_upload;
        self.keep_print_faces = keep_print_faces;
        self.render_viewport = render_viewport;
        self.render_jobs = render_jobs;
        self.checkpoints = checkpoints;
        self.next_checkpoint = next_checkpoint;
        self.replacements = replacements;
        self.ids = ids;
        self.redo_watch = redo_watch;
        self.note_redo_changes(&previous_doc, hidden);
    }

    /// Object ids from `seed` from here on, as a replay of a session whose ids stood there
    /// (`replay.json`'s `ids`) gives.
    pub fn seed_ids(&mut self, seed: u64) {
        self.ids = solidcraft_doc::ids::IdSource::seeded(seed);
    }

    /// The token over the work running off the window thread for this design, on any of its
    /// copies: a job started from a copy runs [`solidcraft_kernel::Stop::within`] it.
    pub fn stop_token(&self) -> solidcraft_kernel::Stop {
        self.stop.clone()
    }

    /// This design is going away: the work still computing for it stops at the kernel's next
    /// check, and its results go nowhere (they are only ever for this design's evaluation cache).
    /// A kernel call that doesn't check runs to its end, on a kernel of its own, holding up nothing.
    pub fn stop_computing(&self) {
        self.stop.stop();
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
    pub(crate) fn check_command(&self, spec: &CommandSpec, params: &Value) -> Result<()> {
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

    /// The design has changes not in its file: in the document, in what is hidden, or a
    /// component moved and not captured yet (#9736).
    pub fn is_dirty(&self) -> bool {
        self.document_differs() || self.visibility != self.saved_visibility || self.moved_uncaptured()
    }

    /// The design is a STEP, IGES or mesh file just opened (`doc.open`) and not changed since:
    /// not saved as a design yet, so [`Session::is_dirty`], but closing it loses nothing the file
    /// doesn't hold, and its tab closes without asking (#15111).
    pub fn unchanged_import(&self) -> bool {
        self.path.is_none()
            && self.opened_import.as_ref().is_some_and(|opened| Arc::ptr_eq(opened, &self.doc) || **opened == *self.doc)
            && self.visibility == self.saved_visibility
            && !self.moved_uncaptured()
    }

    /// Remember the design as `doc.open` just made it from another kind of file.
    pub(crate) fn opened_from_import(&mut self) {
        self.opened_import = Some(self.doc.clone());
    }

    /// A component was moved and its position not captured yet (#9736). The placements of an
    /// exploded view shown are not such a move: they last only while it is shown.
    pub fn moved_uncaptured(&self) -> bool {
        !self.pending_moves.is_empty() && !cmd::motion::shows_exploded_view(self)
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

    /// An undo or document replacement cannot leave an edit aimed at another owner.
    pub(crate) fn prune_sheet_sketch_edit(&mut self) {
        if self.active_sheet_sketch.as_ref().is_some_and(|entry| {
            (entry.view.is_none() && self.open_drawing != Some(entry.drawing))
                || self.checked_sheet_sketch_owner(&self.view_context, Some(entry), &self.doc).is_err()
                || self
                    .doc
                    .drawing(entry.drawing)
                    .and_then(|drawing| drawing.sheet_sketch(entry.sketch))
                    .is_none_or(|record| record.owner != entry.owner)
        }) {
            self.active_sheet_sketch = None;
        }
    }

    /// The design as its current file holds it: what is hidden, what its dimensions last read and
    /// its links relative to that file.
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

    /// Let the used sketches' set go before an edit in place: it holds the document weakly, and
    /// `Arc::make_mut` would move a weakly held document to a new address.
    fn forget_consumed_sketches(&self) {
        if let Ok(mut cache) = self.consumed_sketches_cache.lock() {
            *cache = None;
        }
    }

    /// The marker an active temporary sketch edit returns to. `Some(None)` means the end of
    /// the timeline; `None` means this sketch was opened without a temporary rollback.
    pub fn sketch_restore_marker(&self) -> Option<Option<usize>> {
        self.sketch_entry.as_ref().filter(|entry| Some(entry.sketch) == self.active_sketch).and_then(|entry| entry.restore_marker)
    }

    /// Mutable document (copy-on-write).
    pub fn doc_mut(&mut self) -> &mut Document {
        self.forget_document_caches();
        #[cfg(test)]
        if Arc::strong_count(&self.doc) > 1 {
            tests::doc_copies::record();
        }
        Arc::make_mut(&mut self.doc)
    }

    /// Make `doc` the design, as `*doc_mut() = doc` does, without first copying a shared
    /// document only to overwrite the copy: a command's undo step still holds the design it
    /// started from, so `doc_mut` would copy all of it (every feature and sketch) on each
    /// sketch drag frame. The design lands at a new address either way, and the command's end
    /// tells the crash journal that address (#7502).
    pub(crate) fn set_doc(&mut self, doc: Document) {
        self.forget_document_caches();
        self.doc = Arc::new(doc);
    }

    /// Before the document changes: the placement index keeps the document alive with its
    /// answers; let it go, so an in-place edit stays in place (a shared handle would clone the
    /// document to a new address behind the crash journal's back, #7502) and no stale lookup
    /// outlives it.
    fn forget_document_caches(&mut self) {
        if let Ok(mut index) = self.placement_index_cache.lock() {
            *index = None;
        }
        self.forget_consumed_sketches();
        self.forget_dirty_answer();
        self.model.forget_fingerprints();
    }

    /// Whose steps undo and redo go through: the open drawing's (by id), or the model's (`None`),
    /// with none open or when [`Session::model_focus`] is set.
    pub fn history_part(&self) -> Option<u64> {
        self.open_drawing.filter(|_| !self.model_focus)
    }

    /// A sheet sketch of the open drawing is being edited, and the host isn't showing the model
    /// ([`Session::model_focus`]): the shared sketch tools work on it, not on the model's sketch.
    pub fn sheet_sketching(&self) -> bool {
        !self.model_focus && self.active_sheet_sketch.as_ref().is_some_and(|entry| self.open_drawing == Some(entry.drawing))
    }

    /// The drawing open, for `cmd`.
    pub fn drawing(&self, cmd: &str) -> Result<&Arc<solidcraft_doc::drawing::Drawing>> {
        self.open_drawing.and_then(|id| self.doc.drawing(id)).ok_or_else(|| params::bad(cmd, "open a drawing first (drawing.open)"))
    }

    /// The design source `source` of drawing `d` names, to read from: this session for the design the
    /// drawing is in. The one answer to "which design does this source mean": every reader of a
    /// source's design (a view, a sheet's title block, a parts list) asks it here. Another design's
    /// file can't be read yet (#3890): refused, never answered with this design.
    pub(crate) fn drawn(&self, d: &solidcraft_doc::drawing::Drawing, source: u64) -> std::result::Result<&Session, Undrawn> {
        match d.sources.iter().find(|named| named.id == source) {
            None => Err(Undrawn::NoSource),
            Some(named) if named.path.is_none() => Ok(self),
            Some(_) => Err(Undrawn::Linked),
        }
    }

    /// [`Self::drawn`] as doc's readers of a source's design take it: that design and its model
    /// now, or why it can't be read, in words.
    pub(crate) fn source_design(&self, d: &solidcraft_doc::drawing::Drawing, source: u64) -> solidcraft_doc::drawing::balloon::Design<'_> {
        self.drawn(d, source).map(|on| (&*on.doc, on.model.state())).map_err(|why| why.to_string())
    }

    /// Whether `object` can be selected in `drawing`: a view, dimension or balloon of the drawing open.
    pub(crate) fn selectable_in_drawing(&self, drawing: u64, object: u64) -> bool {
        self.open_drawing == Some(drawing) && self.doc.drawing(drawing).is_some_and(|d| d.has_object(object))
    }

    /// Drops the selected drawing objects no longer there: an undo took them, or their drawing closed.
    fn prune_drawing_selection(&mut self) {
        let gone = |s: &Session, x: &Sel| matches!(x, Sel::Drawing { drawing, object } if !s.selectable_in_drawing(*drawing, *object));
        if self.selection.iter().any(|x| gone(self, x)) {
            let selection = std::mem::take(&mut self.selection);
            self.selection = selection.into_iter().filter(|x| !gone(self, x)).collect();
        }
    }

    /// Change the drawing open, for `cmd`: kept when it still keeps the record's rules, and only
    /// when it changed (one undo step of the drawing, through `execute`; the design's other
    /// drawings shared).
    pub fn edit_drawing<T>(&mut self, cmd: &str, change: impl FnOnce(&mut solidcraft_doc::drawing::Drawing) -> Result<T>) -> Result<T> {
        let open = self.drawing(cmd)?;
        let mut d = (**open).clone();
        let out = change(&mut d)?;
        if d.id != open.id {
            return Err(params::bad(cmd, "a drawing keeps its id"));
        }
        // Preserve the last valid filled region immediately before a geometry
        // edit loses its identity. Only changed, filled sketches are solved;
        // changing names, sheets, patterns or other paper data does no such work.
        let affected: std::collections::BTreeSet<u64> = open
            .sketches
            .iter()
            .filter_map(|previous| d.sheet_sketch(previous.id).filter(|next| previous.sketch != next.sketch).map(|_| previous.id))
            .collect();
        if !affected.is_empty() && !d.fills.is_empty() {
            let mut previous = (**open).clone();
            previous.refresh_fill_regions(&self.doc, &affected).map_err(|error| params::bad(cmd, error.to_string()))?;
            for slot in &mut d.fills {
                if let Some(saved) = previous.sheet_fill(slot.id)
                    && saved.sketch == slot.sketch
                    && saved.boundary == slot.boundary
                    && saved.last_region != slot.last_region
                {
                    let mut fill = (**slot).clone();
                    fill.last_region = saved.last_region.clone();
                    *slot = Arc::new(fill);
                }
            }
            d.refresh_fill_regions(&self.doc, &affected).map_err(|error| params::bad(cmd, error.to_string()))?;
        }
        d.validate().map_err(|e| params::bad(cmd, e.to_string()))?;
        if d != **open {
            self.doc_mut().put_drawing(Arc::new(d));
        }
        Ok(out)
    }

    /// Source parameters can change a paper sketch without editing its record.
    /// Retention snapshots join that same model command, never a new Drawing
    /// undo step. Prepare every replacement before publishing any of them.
    fn refresh_sheet_fill_sources(&mut self, previous: &Arc<Document>, cmd: &str) -> Result<()> {
        self.doc = Self::refreshed_sheet_fill_source_document(previous, &self.doc, cmd)?;
        Ok(())
    }

    fn refreshed_sheet_fill_source_document(previous: &Arc<Document>, current: &Arc<Document>, cmd: &str) -> Result<Arc<Document>> {
        if current.drawings.iter().all(|drawing| drawing.fills.is_empty()) {
            return Ok(current.clone());
        }
        if Arc::ptr_eq(current, previous) {
            return Ok(current.clone());
        }
        if current.units == previous.units && current.param_defs() == previous.param_defs() {
            return Ok(current.clone());
        }
        let mut changed = Vec::new();
        let mut sketches = 0usize;
        for drawing in &current.drawings {
            if drawing.fills.is_empty() {
                continue;
            }
            let Some(before) = previous.drawing(drawing.id) else { continue };
            sketches = sketches
                .checked_add(drawing.fills.iter().map(|fill| fill.sketch).collect::<std::collections::BTreeSet<_>>().len())
                .filter(|count| *count <= solidcraft_doc::drawing::MAX_FILLED_SKETCHES)
                .ok_or_else(|| params::bad(cmd, "source update exceeds the filled-sketch refresh budget"))?;
            let mut candidate = (**drawing).clone();
            candidate.refresh_fill_regions_after_source_change(before, previous, current).map_err(|error| params::bad(cmd, error.to_string()))?;
            if candidate != **drawing {
                changed.push(Arc::new(candidate));
            }
        }
        let mut result = current.clone();
        if !changed.is_empty() {
            let doc = Arc::make_mut(&mut result);
            for drawing in changed {
                doc.put_drawing(drawing);
            }
        }
        Ok(result)
    }

    /// History adoption prepares retained geometry before either history stack
    /// changes. Drawing history keeps current retention only for matching fill
    /// identities; complete checkpoints retain their own complete snapshots.
    fn refreshed_sheet_fill_history_document(
        previous: &Arc<Document>,
        current: &Arc<Document>,
        part: Option<u64>,
        cmd: &str,
    ) -> Result<Arc<Document>> {
        let Some(id) = part else { return Self::refreshed_sheet_fill_source_document(previous, current, cmd) };
        let (Some(before), Some(after)) = (previous.drawing(id), current.drawing(id)) else { return Ok(current.clone()) };
        if after.fills.is_empty() {
            return Ok(current.clone());
        }
        let mut drawing = (**after).clone();
        drawing.refresh_fill_regions_after_source_change(before, previous, current).map_err(|error| params::bad(cmd, error.to_string()))?;
        if drawing == **after {
            return Ok(current.clone());
        }
        let mut result = current.clone();
        Arc::make_mut(&mut result).put_drawing(Arc::new(drawing));
        Ok(result)
    }

    /// Open-session identity and committed history state for plugin results.
    /// View changes and saves leave this token alone. Authored edits and history
    /// traversal receive fresh revisions without changing the document contents.
    pub fn design_token(&self) -> craft_plugin::DesignToken {
        craft_plugin::DesignToken { document_id: self.document_id.clone(), revision: self.design_revision.clone() }
    }

    fn design_token_is_current(&self) -> bool {
        std::ptr::eq(self.design_state.as_ptr(), Arc::as_ptr(&self.doc))
    }

    fn follow_design_token(&mut self) {
        self.design_state = Arc::downgrade(&self.doc);
    }

    fn commit_design_token(&mut self) {
        if !self.design_token_is_current() {
            self.design_revision = solidcraft_doc::ids::new_design_revision();
            self.follow_design_token();
        }
    }

    /// The open design's version, for an issued revision to name: nothing in the browser, for an
    /// unsaved design, and until version control gives versions (#2869), which fills this in.
    pub(crate) fn design_version(&self) -> Option<solidcraft_geom::revision::ModelVersion> {
        None
    }

    /// Prepared placements only: optional save caches must not compute a cold world, and a
    /// joint drag's frame drawn by moving its bodies checks it placed nothing. `None` when the design has
    /// components and nothing has placed them since it last changed.
    pub fn cached_world_state(&self) -> Option<Arc<solidcraft_doc::ModelState>> {
        let state = self.model.state();
        if self.doc.occurrences.is_empty() {
            return Some(state);
        }
        self.world_cache.lock().ok()?.as_ref().filter(|cache| cache.matches(self, &state)).and_then(|cache| cache.world.clone().ok())
    }

    /// The moves of the joint drag's frame shown ([`DragPreview`]); none once the design is not
    /// the one it was worked out for.
    pub fn drag_moves(&self) -> &std::collections::BTreeMap<u64, solidcraft_doc::Mat> {
        static NONE: std::collections::BTreeMap<u64, solidcraft_doc::Mat> = std::collections::BTreeMap::new();
        self.drag_preview.as_ref().filter(|p| Arc::ptr_eq(&p.design, &self.doc)).map_or(&NONE, |p| &p.moves)
    }

    /// Re-evaluate the model after a document change.
    /// Whether the whole-body scope is in force: Select All set it, no item has been named since
    /// and no sketch is open. Anything that names items or clears the selection ends it, so a
    /// stale flag never outlives the list it stood for.
    pub fn all_bodies_selected(&self) -> bool {
        self.selection_all_bodies && self.selection.is_empty() && self.active_sketch.is_none()
    }

    /// Clear the selection: the named items and the whole-body scope.
    pub fn clear_selection(&mut self) {
        self.selection.clear();
        self.selection_all_bodies = false;
    }

    /// Whether anything is selected: a list of items or the whole-body scope.
    pub fn has_selection(&self) -> bool {
        !self.selection.is_empty() || self.all_bodies_selected()
    }

    /// The selection as the items an operation works on: the whole-body scope resolves to the
    /// name of every shown body, in the world's order.
    pub fn selected_items(&self) -> Result<Vec<Sel>> {
        if !self.all_bodies_selected() {
            return Ok(self.selection.clone());
        }
        Ok(self.shown_body_names()?.into_iter().map(|name| Sel::Body { name }).collect())
    }

    /// How many things are selected, resolving the whole-body scope against the world.
    pub fn selected_count(&self) -> Result<usize> {
        if !self.all_bodies_selected() {
            return Ok(self.selection.len());
        }
        Ok(self.world_state()?.bodies.iter().filter(|b| self.body_visible(&b.name)).count())
    }

    /// The bodies the whole-body scope covers: every body the view shows (not hidden itself, nor
    /// through its occurrence or folder), in the world's order.
    pub(crate) fn shown_body_names(&self) -> Result<Vec<String>> {
        Ok(self.world_state()?.bodies.iter().filter(|b| self.body_visible(&b.name)).map(|b| b.name.clone()).collect())
    }

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
        let drag = self.drag_moves();
        // A drag's frame places its occurrences over any move not captured yet.
        let dragged;
        let moves = if drag.is_empty() {
            &self.pending_moves
        } else {
            dragged = self.pending_moves.iter().chain(drag).map(|(o, m)| (*o, *m)).collect();
            &dragged
        };
        let w = solidcraft_doc::world_state_moved_reusing(&self.doc, &st, moves, &mut reuse);
        if let Ok(mut p) = placed.lock() {
            *p = reuse;
        }
        let w = w.map_err(EngineError::from).and_then(|w| self.authored_sketches(w)).map(|w| self.sketch_copies(w, moves)).map(Arc::new);
        if let Ok(mut c) = self.world_cache.lock() {
            *c = Some(WorldCache {
                doc: self.doc.clone(),
                model: st,
                pending_moves: self.pending_moves.clone(),
                drag_moves: drag.clone(),
                active_component: self.active_component,
                active_occurrence_path: self.active_occurrence_path.clone(),
                world: w.as_ref().map(Arc::clone).map_err(ToString::to_string),
                placed,
                frames: Arc::default(),
            });
        }
        w
    }

    /// `sketches`, built for this design's model (failed sketches shown with their last geometry,
    /// [`solidcraft_doc::Model::failed_sketch_shown`]), placed where [`Session::world_state`]
    /// places sketches. Display only: no state a command reads has them.
    pub fn sketches_placed(&self, sketches: Vec<Arc<solidcraft_doc::SolvedSketch>>) -> Result<Vec<Arc<solidcraft_doc::SolvedSketch>>> {
        if sketches.is_empty() || self.doc.occurrences.is_empty() {
            return Ok(sketches);
        }
        let mut only = solidcraft_doc::ModelState::default();
        only.sketches = sketches;
        let placed = solidcraft_doc::world_state_reusing(&self.placed_document(), &only, &mut Default::default())?;
        Ok(self.authored_sketches(placed)?.sketches)
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
                    sketch.plane = solidcore_record::Plane::new(
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

    /// `w` with the other placements of each component sketch ([`solidcraft_doc::SketchCopy`]):
    /// each placement of its component under `moves`, but the one it is drawn at
    /// ([`Session::feature_path`]), as a move from there.
    fn sketch_copies(
        &self,
        mut w: solidcraft_doc::ModelState,
        moves: &std::collections::BTreeMap<u64, solidcraft_doc::Mat>,
    ) -> solidcraft_doc::ModelState {
        let mut owners: std::collections::BTreeMap<u64, Vec<u64>> = std::collections::BTreeMap::new();
        for sketch in &w.sketches {
            if let Some(f) = self.doc.feature(sketch.feature)
                && f.component != 0
            {
                owners.entry(f.component).or_default().push(sketch.feature);
            }
        }
        if owners.is_empty() {
            return w;
        }
        let mut of: std::collections::BTreeMap<u64, Vec<(Vec<u64>, solidcraft_doc::Mat)>> = std::collections::BTreeMap::new();
        for (component, path, m) in self.doc.placements_moved(moves) {
            if owners.contains_key(&component) {
                of.entry(component).or_default().push((path, m));
            }
        }
        for (component, sketches) in owners {
            let placements = of.remove(&component).unwrap_or_default();
            if placements.len() < 2 {
                continue;
            }
            // Where the sketch is drawn: the placement being authored, at its frame as
            // `authored_sketches` put it there, else the first, where the world state placed it.
            let shown = self.feature_path(component);
            let drawn = if self.active_occurrence_path.is_some() && self.active_component == component {
                Some(self.active_component_frame())
            } else {
                placements.iter().find(|(path, _)| *path == shown).map(|(_, m)| *m)
            };
            let Some(inverse) = drawn.and_then(|m| solidcraft_doc::mat_inverse(&m)) else { continue };
            let copies: Arc<[solidcraft_doc::SketchCopy]> = placements
                .into_iter()
                .filter(|(path, _)| *path != shown)
                .map(|(path, m)| solidcraft_doc::SketchCopy { path, delta: solidcraft_doc::mat_mul(&m, &inverse) })
                .collect();
            for sketch in sketches {
                w.sketch_copies.insert(sketch, copies.clone());
            }
        }
        w
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
    pub(crate) fn component_or_placement(&self, key: &str) -> solidcraft_doc::Result<Option<u64>> {
        Ok(self
            .doc
            .resolve_component(key)?
            .or_else(|| self.doc.occurrences.iter().find(|o| o.name == key).map(|o| o.component))
            .filter(|id| *id != 0))
    }

    /// Number of live-model evaluations after this session's initialization.
    /// Construction, other sessions and trial evaluations of cloned models are
    /// excluded. This is not a count of all geometry work. Reading the counter
    /// never evaluates geometry or warms display meshes.
    pub fn evaluation_count(&self) -> usize {
        self.evaluations
    }

    /// The open sketch was entered by a person from the UI, not by a command: finishing it with
    /// nothing drawn deletes it, where a sketch a script or an agent made stays.
    pub fn mark_sketch_entered_by_person(&mut self) {
        self.sketch_by_person = self.active_sketch;
    }

    pub fn refresh(&mut self) {
        self.sketch_finish_pending = false;
        let token_current = self.design_token_is_current();
        let _ = self.refresh_with_joint_report();
        if token_current {
            self.follow_design_token();
        }
    }

    /// Let this design's refreshes show their results without waiting for the SMLib checks of
    /// the bodies their last features keep ([`solidcraft_doc::Model::set_background_checks`]):
    /// for a window, which settles them as they end ([`Session::settle_feature_checks`]). Off
    /// (the default: scripts, the CLI, agents' commands and tests), a refresh waits for them.
    pub fn set_background_checks(&mut self, on: bool) {
        self.background_checks = on;
    }

    /// Whether refreshes leave their last features' checks running ([`Session::set_background_checks`]).
    pub fn background_checks(&self) -> bool {
        self.background_checks
    }

    /// Settle the SMLib checks of features the last refresh left running, once they have ended
    /// (or after waiting for them, with `wait`), and when one refused a feature's bodies,
    /// evaluate the design again, which fails that feature with the check's error: except during
    /// a live edit, which a refresh would end (a later call does it). Returns whether it
    /// refreshed.
    pub fn settle_feature_checks(&mut self, wait: bool) -> bool {
        if self.model.settle_feature_checks(wait) && self.live_links.is_none() {
            self.refresh();
            return true;
        }
        false
    }

    /// Settle what the imports' background checks found ([`solidcraft_doc::Model::settle_import_checks`]),
    /// and when they healed parts the design does not show yet, evaluate it again: except during
    /// a live edit, which a refresh would end (a later call does it). Returns whether it refreshed.
    pub fn settle_import_checks(&mut self) -> bool {
        if self.model.settle_import_checks() && self.live_links.is_none() {
            self.refresh();
            return true;
        }
        false
    }

    /// The bodies that left this session's model let go of their kernel forms
    /// ([`solidcraft_doc::Model::release_left`], #19629): after a refresh, when an edit computed
    /// on a copy is adopted, and at the end of every command, which is when a copy a command
    /// only read through (a trial of parameter values) is done with. A front end that runs a
    /// command on a copy of this session calls it when the copy's answer is back. A copy lets
    /// go of nothing: its session still holds the bodies it started from.
    pub fn release_left(&mut self) {
        if !self.is_copy {
            self.model.release_left();
        }
    }

    /// Refresh normally, retaining the last joint solve for commands that report its outcome.
    fn refresh_with_joint_report(&mut self) -> Option<solidcraft_doc::joints::Solution> {
        let mut solved = None;
        self.model.set_background_checks(self.background_checks && !self.is_copy);
        let evaluated = self.model.evaluate_shared(&self.doc);
        self.model.set_background_checks(false);
        if evaluated {
            self.evaluations = self.evaluations.saturating_add(1);
        }
        // Component Pattern features keep their count/spacing expressions; place their copies
        // after the model rebuild so a parameter change updates the placements (#12116).
        // Only a design with such a Pattern is touched, and only when the placements change: the
        // live document stays the same document otherwise (undo, checkpoints and Save rely on it).
        let patterns_components =
            self.doc.features.iter().any(|f| matches!(&f.kind, solidcraft_doc::FeatureKind::Pattern { components, .. } if !components.is_empty()));
        if patterns_components {
            let state = self.model.state();
            let mut doc = (*self.doc).clone();
            if solidcraft_doc::reconcile_component_patterns(&mut doc, &state).is_ok() && doc != *self.doc {
                *self.doc_mut() = doc;
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

        // Occurrences moved by expressions follow the parameters they name (#8192); features that
        // read where a part is placed are evaluated again for where it now is.
        if cmd::follow_expressed_moves(self) {
            self.model.evaluate(&self.doc);
            self.evaluations = self.evaluations.saturating_add(1);
        }
        self.prune_sheet_sketch_edit();
        // Joints follow the geometry their origins snap to.
        if !self.doc.assembly.joints.is_empty() {
            // A solve that moves parts changes what features reading where a part is placed
            // (a mirror of a placed body) make: the model is evaluated again for the placements
            // the design now holds, and the joints solved for that model, a few times at most.
            for _ in 0..3 {
                let before = self.doc.clone();
                if let Some(solution) = self.solve_joints() {
                    solved = Some(solution);
                }
                if Arc::ptr_eq(&before, &self.doc) || before.occurrences == self.doc.occurrences {
                    break;
                }
                self.model.evaluate(&self.doc);
                self.evaluations = self.evaluations.saturating_add(1);
            }
        }
        cmd::motion::associative::refresh(self);
        if let Some(id) = self.active_sketch
            && self.doc.feature(id).is_none()
        {
            self.active_sketch = None;
            self.sketching_view = None;
        }
        self.release_left();
        solved
    }

    /// Solve the joints, unless nothing they read changed since this session last solved them:
    /// the model is the one that solve read and the design is as the solve left it, including
    /// which component patterns are active. A command that refreshes on
    /// its own (a parameter change) is refreshed again after it, and that second solve would
    /// find every occurrence where it is.
    fn solve_joints(&mut self) -> Option<solidcraft_doc::joints::Solution> {
        let state = self.model.state();
        let settled = self.joints_settled.as_ref().is_some_and(|(doc, model)| {
            std::sync::Weak::as_ptr(model) == Arc::as_ptr(&state)
                && (Arc::ptr_eq(doc, &self.doc)
                    || (same_but_features(doc, &self.doc) && doc.inactive_pattern_occurrences() == self.doc.inactive_pattern_occurrences()))
        });
        if settled {
            return None;
        }
        let solved = cmd::joints::place(self);
        self.joints_placed();
        Some(solved)
    }

    /// The joints were just solved for this design and model and the design moved to the
    /// solution: the refresh after the command need not solve them again unless the model
    /// changes (see [`Session::solve_joints`]). `joint.reach` says so after its own solve, which
    /// every frame of a joint drag runs.
    pub(crate) fn joints_placed(&mut self) {
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
                let _ = self.solve_joints();
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
    ///
    /// Skipped when nothing the pass reads changed since the last one: the features (with the
    /// names they pin), the reserved names and the model's results. A joint drag frame changes
    /// only where occurrences are and the joints' values; walking every feature's result for it
    /// took 40 ms a frame in a 5,484-feature assembly. The model is compared by allocation: the
    /// weak reference keeps the allocation, so a later model can't be mistaken for it.
    fn pin_body_names(&mut self) {
        if self.model.is_stale() {
            return;
        }
        let state = self.model.state();
        let unchanged = self.names_pinned.as_ref().is_some_and(|(doc, model)| {
            std::sync::Weak::as_ptr(model) == Arc::as_ptr(&state)
                && (Arc::ptr_eq(doc, &self.doc) || (doc.features == self.doc.features && doc.reserved_body_names == self.doc.reserved_body_names))
        });
        if unchanged {
            return;
        }
        #[cfg(test)]
        {
            self.body_name_passes += 1;
        }
        self.pin_body_names_pass();
        self.names_pinned = Some((self.doc.clone(), Arc::downgrade(&self.model.state())));
    }

    /// The pass of [`Session::pin_body_names`] over every feature's result, run when something
    /// it reads changed.
    fn pin_body_names_pass(&mut self) {
        // With no body left and no feature holding a stored name, nothing the reserved numbers
        // guard is left, so the next body starts again at `Body1`.
        if !self.doc.reserved_body_names.is_empty()
            && self.model.state().bodies.is_empty()
            && self.doc.features.iter().all(|f| f.body_names.iter().all(|n| n.trim().is_empty()))
        {
            self.doc_mut().reserved_body_names.clear();
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
            // A pattern of several bodies stores its names source by source, so after its count
            // changes they are stored again as the copies have them now (#12114). Filled in
            // slot by slot, a name of the old count would stay in a later source's place. One
            // saved before then reads its names in order; stored as its copies have them, they
            // mean the same copies read source by source. The copies of a source the pattern
            // left out keep the names stored for them, in their places.
            let left_out = r.state.pattern_left_out(f.id);
            if f.pins_every_body_name() && (f.pattern_names_in_order || made.len() != f.body_names.len() || !left_out.is_empty()) {
                let made: Vec<String> = made.iter().map(|n| n.to_string()).collect();
                let names = f.pattern_pins(&made, left_out).map_or(made, |(names, _)| names);
                if f.pattern_names_in_order || names != f.body_names {
                    pins.push((f.id, names));
                }
                continue;
            }
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
                // Stored as the copies have them now: read source by source from here on (#12114).
                f.pattern_names_in_order = false;
            }
        }
        doc.reserved_body_names.extend(reserve);
        // The pinned names are the ones the bodies have: no result changes, so none is evaluated.
        self.model.refingerprint_shared(&self.doc, keep);
    }

    /// Whether a link of a sketch still has the guessed name its design dropped as it opened.
    pub(crate) fn has_dropped_link_names(&self) -> bool {
        self.doc.features.iter().any(|f| self.doc.sketch(f.id).is_ok_and(|sk| sk.links.iter().any(|l| l.name_dropped)))
    }

    /// The names `whole` (this design with its whole timeline evaluated, names pinned) found for
    /// the links this design still has marked `name_dropped`, as `(sketch, link, vertex, edge)`.
    pub(crate) fn dropped_link_pins(&self, whole: &Session) -> Vec<(u64, String, Option<String>, Option<String>)> {
        let mut pins = Vec::new();
        for f in &self.doc.features {
            let (Ok(mine), Ok(theirs)) = (self.doc.sketch(f.id), whole.doc.sketch(f.id)) else { continue };
            for l in mine.links.iter().filter(|l| l.name_dropped) {
                let Some(found) = theirs.links.iter().find(|x| x.id == l.id && !x.name_dropped) else { continue };
                if found.vertex_name.is_some() || found.edge_name.is_some() {
                    pins.push((f.id, l.id.clone(), found.vertex_name.clone(), found.edge_name.clone()));
                }
            }
        }
        pins
    }

    /// Store the vertex and edge names of the links whose guessed names their design dropped as
    /// it opened (`Link::name_dropped`, #17229): each was found by its point, and keeps the name
    /// of what it was found on from now on, as an edit of its sketch stores it. The evaluated
    /// sketches already give the links those names, so nothing is evaluated again. A link not
    /// found (its sketch failed or is rolled back) stays marked.
    fn pin_dropped_link_names(&mut self) {
        if self.model.is_stale() {
            return;
        }
        let mut pins: Vec<(u64, String, Option<String>, Option<String>)> = Vec::new();
        for f in &self.doc.features {
            let Ok(saved) = self.doc.sketch(f.id) else { continue };
            if !saved.links.iter().any(|l| l.name_dropped) {
                continue;
            }
            let Some(r) = self.model.result(f.id).filter(|r| r.error.is_none() && !r.skipped && !r.stale) else { continue };
            let Some(solved) = r.state.sketch(f.id) else { continue };
            for l in saved.links.iter().filter(|l| l.name_dropped) {
                let Some(found) = solved.sketch.links.iter().find(|x| x.id == l.id && !x.lost) else { continue };
                if found.vertex_name.is_some() || found.edge_name.is_some() {
                    pins.push((f.id, l.id.clone(), found.vertex_name.clone(), found.edge_name.clone()));
                }
            }
        }
        if pins.is_empty() {
            return;
        }
        let ids: Vec<u64> = pins.iter().map(|(id, ..)| *id).collect();
        let keep = self.model.pinned_prefix(&self.doc, &ids);
        store_link_names(self.doc_mut(), pins);
        // The stored names are the ones the evaluation found: no result changes.
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

    /// Only persistent render intent, part properties, appearances, the browser's groups or drawings
    /// changed. Never evaluate or clear a stale model here.
    fn render_only_change(&self, before: &Snapshot) -> bool {
        self.active_sketch == before.active_sketch
            && self.sketching_view == before.sketching_view
            && self.pending_moves == before.pending_moves
            && (self.doc.differs_only_in_render(&before.doc)
                || self.doc.differs_only_in_part_properties(&before.doc)
                || self.doc.differs_only_in_appearance(&before.doc)
                || self.doc.differs_only_in_browser(&before.doc)
                || self.doc.differs_only_in_drawings(&before.doc))
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

    /// Only what is shown changed (hidden or shown items, a section or analysis, the selection):
    /// the design and model are untouched, so only the view revision moves and the viewport
    /// redraws. An open feature editor stays valid across it (#10687).
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
        // Whose 3D constraints held before the edit: only those can it break (#10608).
        let wires_held: std::collections::BTreeSet<u64> =
            self.model.results.iter().filter(|r| r.state.sketch(r.id).is_some_and(|k| k.report.wires_ok())).map(|r| r.id).collect();
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
            // A value a 3D dimension or point of this sketch cannot take (#10608) is refused like
            // one that conflicts; an error from upstream is not the edit's to refuse.
            if r.error.is_some()
                && solved_before.contains_key(&r.id)
                && let Ok(sk) = self.doc.sketch(r.id)
                && !sk.wire_constraints.is_empty()
                && let Err(e) = self.doc.apply_wire_values(&self.doc.evaluation_values(), &mut sk.clone())
            {
                return Err(EngineError::Other(format!("sketch `{}`: {e}; the values were not changed", r.name)));
            }
            if r.error.is_some()
                && let Some(before) = solved_before.get(&r.id)
                && let Ok(sk) = self.doc.sketch(r.id)
                && (!sk.offsets.is_empty() || !sk.freeform_offsets.is_empty() || !sk.freeform_chain_offsets.is_empty())
            {
                let vals = self.doc.evaluation_values();
                let mut trial = sk.clone();
                let applied = self.doc.apply_dimension_values(&vals, &mut trial);
                let offset_err = if own_values(&trial) == *before {
                    None
                } else {
                    match applied {
                        Ok(()) => {
                            let mut solved_trial = sk.clone();
                            self.doc
                                .solve_sketch_with_analysis(
                                    &vals,
                                    &mut solved_trial,
                                    &sk.driving_values(),
                                    Some(sk),
                                    solidcraft_sketch::Analysis::StatusOnly,
                                )
                                .err()
                                .map(|e| e.to_string())
                        }
                        Err(e) => {
                            let msg = e.to_string();
                            msg.contains("offset dimension").then_some(msg)
                        }
                    }
                };
                if let Some(e) = offset_err {
                    return Err(EngineError::Other(format!("sketch `{}`: {e}; the values were not changed", r.name)));
                }
            }
            let (Some(before), Some(k)) = (solved_before.get(&r.id), r.state.sketch(r.id)) else { continue };
            if own_values(&k.sketch) == *before {
                continue;
            }
            if !k.report.keeps_orientation() {
                let why = k.report.failure(|f| format!("its constraints would conflict ({f})"));
                return Err(EngineError::Other(format!("sketch `{}`: {why}; the values were not changed", r.name)));
            }
            if let Some(w) = k.report.wires.as_ref().filter(|w| !w.ok && wires_held.contains(&r.id)) {
                let why = w.failure(|f| format!("its 3D constraints would conflict ({f})"));
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
            self.forget_consumed_sketches();
            self.forget_dirty_answer();
            self.model.adopt_solved_sketches(Arc::make_mut(&mut self.doc));
        }
        Ok(())
    }

    /// Install the host's validated live view registry without orphaning the current edit.
    pub fn set_view_context(&mut self, context: ViewContext) -> std::result::Result<(), String> {
        if self.active_sketch.is_some()
            && let Some(owner) = self.sketching_view
        {
            context.resolve_sketch_target(Some(owner))?;
        }
        self.checked_sheet_sketch_owner(&context, self.active_sheet_sketch.as_deref(), &self.doc).map_err(|error| error.to_string())?;
        self.view_context = context;
        Ok(())
    }

    /// Check a computed owner against today's open views, before adopting it.
    pub(crate) fn checked_sketch_owner(&self, active: Option<u64>, owner: Option<u64>) -> Result<Option<u64>> {
        if active.is_none() {
            return Ok(None);
        }
        self.view_context.resolve_sketch_target(owner).map(Some).map_err(EngineError::Other)
    }

    /// Geometry history remains usable after its former editing view closes. A closed owner
    /// never transfers sketch mode to another view; callers finalize any temporary marker.
    pub(crate) fn historical_sketch_owner(&self, active: Option<u64>, owner: Option<u64>) -> Option<u64> {
        active.and_then(|_| self.view_context.resolve_sketch_target(owner).ok())
    }

    pub(crate) fn requested_sketch_view(&self, params: &Value, cmd: &str) -> Result<u64> {
        let requested = match params.get("view") {
            None | Some(Value::Null) => None,
            Some(value) => Some(
                value
                    .as_u64()
                    .filter(|id| (1..=1 << 53).contains(id))
                    .ok_or_else(|| EngineError::BadParams { cmd: cmd.into(), msg: "view must be an open view ID from 1 to 2^53".into() })?,
            ),
        };
        self.view_context.resolve_sketch_target(requested).map_err(|msg| EngineError::BadParams { cmd: cmd.into(), msg })
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
        if let Some(call) = plugin::resolve_call(self, id, params)? {
            return self.execute("plugin.run", &call);
        }
        let spec = find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.into()))?;
        self.with_sketch_exit_receiver(spec.id, |session| session.execute_registered(spec, params))
    }

    fn execute_registered(&mut self, spec: &CommandSpec, params: &Value) -> Result<Value> {
        self.check_command(spec, params)?;
        self.note_edit_entry(spec, params);
        let event = self.prepare_command_edit(spec, params)?;
        let prepared = self.take_prepared_edit_source(spec, params);
        let event = event.or(prepared);
        let source = self.edit_source_sketch;
        let depth = self.edit_entry_depth;
        if let Some(event) = event.as_ref().filter(|event| event.domain == SketchDomain::Model) {
            self.edit_source_sketch = Some(event.sketch);
        }
        if self.command_starts_edit(spec, params) {
            self.edit_entry_depth = depth.saturating_add(1);
        }
        let result = self.execute_command(spec.id, params);
        self.edit_source_sketch = source;
        self.edit_entry_depth = depth;
        result.map(|value| edit_start::finished_reply(value, event.as_ref()))
    }

    fn execute_command(&mut self, id: &str, params: &Value) -> Result<Value> {
        if let Some(call) = plugin::resolve_call(self, id, params)? {
            return self.execute("plugin.run", &call);
        }
        let spec = find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.to_string()))?;
        self.execute_spec(spec, params)
    }

    fn execute_spec(&mut self, spec: &cmd::CommandSpec, params: &Value) -> Result<Value> {
        // Plugin data is transient view state, so it stays out of undo snapshots.
        // Restore it for every command error, including failures after guest publication.
        let plugin_panels_before = (spec.id == "plugin.run").then(|| self.plugin_state.panels.clone());
        let result = self.execute_spec_inner(spec, params);
        if result.is_err()
            && let Some(panels) = plugin_panels_before
        {
            self.plugin_state.panels = panels;
        }
        result
    }

    fn execute_spec_inner(&mut self, spec: &cmd::CommandSpec, params: &Value) -> Result<Value> {
        if panels::commands::is_command(spec.id) {
            // Layout is application state. Even a missing capability must not
            // clear live links, map picks, snapshot, evaluate or refresh a design.
            panels::commands::parse_command(spec.id, params)?;
            return Err(EngineError::Panel(panels::error(panels::Rule::Capability, "application layout backend was not injected")));
        }
        // The command, the evaluation it causes and the refresh after it: one profiler span.
        let _span = solidcraft_profile::span("command", spec.id);
        let host_viewport = self.render_viewport.clone();
        let model_focus = self.model_focus;
        let host_jobs = self.render_jobs.host.clone();
        let additional_jobs = self.render_jobs.additional.clone();
        let live = spec.live.filter(|_| params.get("live").and_then(Value::as_bool) == Some(true));
        // A disabled command must not evaluate a stale model merely because it was tried from a
        // drawing. Shared sheet tools read only drawing data.
        if self.open_drawing.is_some() {
            spec.allowed(self).map_err(|why| EngineError::Disabled(spec.id.to_string(), why))?;
        }
        let reads_model = spec.reads_model
            && !(self.sheet_sketching() && spec.part == cmd::Part::Sketch)
            && !(spec.id == "selection.delete" && cmd::delete::in_drawing(self, params));
        // What an import's SMLib check found since it was shown is in the model a command reads.
        self.model.settle_import_checks();
        if live.is_none() {
            self.live_links = None;
            // A command that reads the model reads it checked: the checks the window's last
            // refresh left running end first, and a refusal among them leaves the model stale,
            // evaluated just below ([`Session::set_background_checks`]).
            if reads_model {
                self.model.settle_feature_checks(true);
            }
            if reads_model && (self.model.is_stale() || self.sketch_finish_pending) {
                // Evaluated for the command to read: its checks end before it reads.
                let background = std::mem::replace(&mut self.background_checks, false);
                self.refresh();
                self.background_checks = background;
            }
        }
        spec.allowed(self).map_err(|why| EngineError::Disabled(spec.id.to_string(), why))?;
        // Before anything changes: a refusal leaves the session exactly as it was.
        let open_drawing = self.open_drawing;
        let redo_watch_before = self.redo_watch.clone();
        let mut before = Snapshot {
            part: open_drawing.filter(|_| match spec.part {
                cmd::Part::Drawing => true,
                // A shared sketch tool's step is the drawing's on a sheet sketch, else the model's;
                // a delete's is the drawing's when it deletes the drawing's objects.
                cmd::Part::Sketch => self.sheet_sketching(),
                cmd::Part::Open => cmd::delete::in_drawing(self, params),
                cmd::Part::Model | cmd::Part::Any => false,
            }),
            ..Snapshot::of(self, spec.label)
        };
        let link_read_before = (spec.id == "component.relink").then(|| before.doc.clone());
        let link_observations_before = self.plugin_state.link_changes.clone();
        let link_reads_before = self.plugin_state.link_reads.clone();
        let link_read_source_before = self.plugin_state.link_read_source.clone();
        let ids = self.ids.clone();
        let design_before = self.design_revision.clone();
        let identity_before = self.document_id.clone();
        let section_before = self.section_state();
        // Where new geometry goes: not in the design, but a replay needs it (component.activate).
        let active_before = (self.active_component, self.active_occurrence_path.clone());
        let replacements = self.replacements;
        let null = Value::Object(Default::default());
        let params = if params.is_null() { &null } else { params };
        let given = params;
        // Picks in the world go into their component's frame (once: commands run by commands
        // already have them there).
        let (_nested, outermost) = frames::Nested::enter(spec.id);
        // The person's or an agent's action, for usage statistics: timed, and counted below.
        #[cfg(feature = "usage-stats")]
        let usage_clock = (outermost && live.is_none() && self.count_usage).then(web_time::Instant::now);
        #[cfg(feature = "usage-stats")]
        let mut usage_operation = false;
        if outermost {
            frames::reset_inner_undoable();
        }
        let mut mapped = if outermost {
            frames::forget_picks();
            frames::to_local(self, spec.id, params)?
        } else {
            None
        };
        let _copy_context = outermost.then(|| frames::copy_context(spec.id, mapped.as_mut()));
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
        // A virtual component holds nothing, whichever change tries to put something in it (opening
        // a design, which no undo step follows, reads it as it is).
        let r = match r {
            Ok(v) if spec.undoable && outermost && !Arc::ptr_eq(&self.doc, &before.doc) => {
                cmd::refuse_filled_virtual(&before.doc, &self.doc).map(|()| v)
            }
            r => r,
        };
        // Cancelled while it ran: no result, even one finished before the cancel came in, and
        // the design goes back as it was (solidcore#3247). Opening, starting or recovering a
        // design (which replaces the session) keeps its own Cancel: the features not yet built
        // are skipped and what is built so far is shown (#10577).
        let r = if solidcraft_doc::progress::cancelled() && self.replacements == replacements {
            Err(EngineError::Cancelled(spec.id.to_string()))
        } else {
            r
        };
        // The kernel's internal errors come up without the command's id.
        let r = r.map_err(|e| match e {
            EngineError::Internal(id, msg) if id.is_empty() => {
                crash::kernel_caught();
                EngineError::Internal(spec.id.to_string(), msg)
            }
            e => e,
        });
        // A value typed as `name = expression` defines the user parameter `name` (#4071), in the
        // same undo step; a refused definition fails the command, which leaves the design as it was.
        let r = match r {
            Ok((v, live_edit)) if params::has_definition(params) => self.define_inline(spec.id, v).map(|v| (v, live_edit)),
            r => r,
        };
        let r = match r {
            Ok(value) if outermost && spec.undoable && self.replacements == replacements => {
                self.refresh_sheet_fill_sources(&before.doc, spec.id).map(|()| value)
            }
            value => value,
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
        // Which workspace undoes what is the host's too, whichever design is open.
        self.model_focus = model_focus;
        if self.render_jobs.host.is_none() && host_jobs.is_some() {
            self.set_render_job_host(host_jobs)?;
        }
        for (host, _) in additional_jobs {
            self.register_render_job_host(host)?;
        }
        // A model sketch entered while a sheet sketch is edited, with the host not showing the
        // model (a script), ends the sheet sketch as Finish would: its edits are already in the
        // drawing. The shared sketch tools can reach one sketch only, and the model's is what
        // the command asked for; the model's command is never the one refused.
        if !self.model_focus && self.active_sketch.is_some() && before.active_sketch.is_none() && self.active_sheet_sketch.is_some() {
            self.active_sheet_sketch = None;
        }
        // A command that closed the drawing, or opened another, ends its sheet scenes.
        drawing_scene::forget_closed(self);
        self.prune_drawing_selection();
        let crash_outcome = if outermost && crash::enabled() {
            let failed = r.as_ref().err().map(ToString::to_string);
            // Commands that only read leave no trace (the journal keeps what a replay needs).
            let changed = !Arc::ptr_eq(&self.doc, &before.doc)
                || self.active_sketch != before.active_sketch
                || self.active_sheet_sketch != before.active_sheet_sketch
                || self.pending_moves != before.pending_moves
                || self.open_drawing != open_drawing
                || self.active_component != active_before.0
                || self.active_occurrence_path != active_before.1
                || spec.id.starts_with("select.");
            Some((before.doc.clone(), changed, failed, ids.seed()))
        } else {
            None
        };
        let changed = !Arc::ptr_eq(&self.doc, &before.doc);
        // Hiding or showing (`browser.visibility`) leaves the document alone, so it needs its own
        // test: it is saved with the design and has an undo step of its own.
        let hidden = self.visibility != before.visibility;
        // A Section Analysis is view state outside the design, with undo steps of its own.
        let sectioned = self.section_state() != section_before;
        if sectioned {
            before.section = Some(section_before);
        }
        let result = match r {
            Ok((v, live_edit)) => {
                let v = if changed { self.with_created(spec.id, params, &before, v) } else { v };
                if changed && *self.doc != *before.doc && self.replacements == replacements && !matches!(spec.id, "file.save" | "file.save_as") {
                    self.commit_design_token();
                }
                if changed || hidden {
                    // Another design opened, started or recovered is refreshed in full, however
                    // little it differs from the one before: the shortcut proves a session's own
                    // edit kept its model and joints valid, and a new session has had none.
                    let render_only = self.replacements == replacements && self.render_only_change(&before);
                    let display_only = !changed && hidden;
                    let previous_doc = before.doc.clone();
                    // One undo step per command: the outermost one saves it, for itself or for an
                    // undoable command it ran (#12393), unless it replaced the session (opening a design starts
                    // a fresh history). A command run inside another saves none.
                    let saves = if outermost {
                        spec.undoable || (frames::inner_undoable() && self.replacements == replacements)
                    } else {
                        if spec.undoable {
                            frames::note_inner_undoable();
                        }
                        false
                    };
                    if saves {
                        // An operation for usage statistics: the action that made this undo step
                        // (`usage`). A drag's frames make none until it ends; replays make none.
                        #[cfg(feature = "usage-stats")]
                        if usage_clock.is_some() {
                            usage::note(usage::Bucket::Operations, spec.id);
                            usage_operation = true;
                        }
                        // A new step ends what could be redone in its part; the other parts' redo
                        // steps still hold, as undo puts back only the part a step changed.
                        let part = before.part;
                        self.undo.push(before);
                        if self.undo.len() > MAX_UNDO {
                            self.undo.remove(0);
                        }
                        self.redo.retain(|step| step.part != part);
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
                    // A row the change added takes its item number in the same undo step; numbers
                    // draw nothing, so the model and the placed world stay.
                    if spec.undoable && outermost && live.is_none() {
                        let numbered = self.doc.clone();
                        if cmd::record_item_numbers(self) {
                            self.refresh_render(&numbered);
                        }
                    }
                    if pin {
                        self.pin_body_names();
                        self.pin_dropped_link_names();
                    }
                    // Checkpoint restoration restores matching provenance with history;
                    // its rollback delta must not re-mark the reverted owners.
                    if spec.id != "edit.restore_checkpoint" {
                        self.note_redo_changes(&previous_doc, hidden);
                    }
                } else if sectioned && spec.undoable {
                    let part = before.part;
                    self.undo.push(before);
                    if self.undo.len() > MAX_UNDO {
                        self.undo.remove(0);
                    }
                    self.redo.retain(|step| step.part != part);
                }
                Ok(v)
            }
            Err(e) => {
                // A failed command draws no ids, so a replay of what succeeded draws the same.
                self.ids = ids;
                self.plugin_state.link_changes = link_observations_before;
                self.plugin_state.link_reads = link_reads_before;
                self.plugin_state.link_read_source = link_read_source_before;
                self.design_revision = design_before;
                self.document_id = identity_before;
                self.redo_watch = redo_watch_before;
                self.open_drawing = open_drawing;
                self.active_sheet_sketch = before.active_sheet_sketch.clone();
                self.active_sketch = before.active_sketch;
                self.sketching_view = before.sketching_view;
                if let Some(section) = before.section.take() {
                    self.set_section_state(section);
                }
                if changed || hidden || self.pending_moves != before.pending_moves {
                    // A refused sheet edit can have published its candidate before a later
                    // constraint-policy check. The same proof used for successful drawing
                    // edits keeps rollback from evaluating or clearing a stale source model.
                    let render_only = self.replacements == replacements && self.render_only_change(&before);
                    let previous_doc = self.doc.clone();
                    self.doc = before.doc;
                    self.active_sketch = before.active_sketch;
                    self.pending_moves = before.pending_moves;
                    self.visibility = before.visibility;
                    if render_only {
                        self.refresh_render(&previous_doc);
                    } else {
                        self.refresh();
                    }
                }
                Err(e)
            }
        };
        // Refresh/pinning preserve the authored state just committed. A failure
        // restored its immutable snapshot and pre-call token; saves keep their token.
        self.follow_design_token();
        // A sketch open for editing that failed (a projected edge lost by name) is still shown, its
        // lost link marked, so it can be repaired from inside it (#12085).
        if outermost {
            self.model.hold_failed_sketch(&self.doc, self.active_sketch);
            self.release_left();
        }
        if let Some((before, changed, failed, seed)) = crash_outcome {
            // Refresh and body-name pinning can copy the shared document. Remember the final
            // document, so the next command does not mistake that work for an outside edit
            // and discard the replay. Failed commands have restored their document by now.
            crash::command_done(self.crash_journal, changed, &before, &self.doc, (spec.id, given, seed), failed.map_or(Ok(()), Err));
        }
        // An operation's or a failure's time, and the failure's kind (never its message).
        #[cfg(feature = "usage-stats")]
        if let Some(clock) = usage_clock {
            let failure = result.as_ref().err().and_then(usage::error_kind);
            if usage_operation || failure.is_some() {
                usage::note_compute(spec.id, clock.elapsed().as_secs_f64() * 1000.0);
            }
            if let Some(kind) = failure {
                usage::note_failure(spec.id, kind);
            }
        }
        if result.is_ok() {
            if let Some(before) = link_read_before {
                plugin::record_changed_link_reads(self, &before);
            }
            self.plugin_state.prune_link_changes(&self.doc);
        }
        result
    }

    /// Store the definitions a command left in the design (`Document::define_inline`): its
    /// result names them, `defined` the first (as `sketch.dimension` does) and `definitions` all.
    fn define_inline(&mut self, id: &str, mut v: Value) -> Result<Value> {
        let mut doc = (*self.doc).clone();
        let defined = doc.define_inline().map_err(|e| EngineError::BadParams { cmd: id.to_string(), msg: e.to_string() })?;
        let Some(first) = defined.first() else {
            // Only definitions of parameters that already exist, typed again as they were made:
            // nothing new to report, but the text now names the parameter.
            if doc != *self.doc {
                self.doc = Arc::new(doc);
            }
            return Ok(v);
        };
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
        if let Some(entry) = self
            .active_sheet_sketch
            .as_ref()
            .or(before.active_sheet_sketch.as_ref())
            .filter(|entry| !self.model_focus && self.open_drawing == Some(entry.drawing))
        {
            let current = self.doc.drawing(entry.drawing).and_then(|drawing| drawing.sheet_sketch(entry.sketch));
            if let Some(after) = current.filter(|record| record.owner == entry.owner) {
                let previous = before.doc.drawing(entry.drawing).and_then(|drawing| drawing.sheet_sketch(entry.sketch));
                out.insert("created".into(), cmd::sketch_created(previous.map(|record| &record.sketch), &after.sketch));
            }
            return v; // never fall back to the model after a sheet cancellation
        }
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

    /// Remember the design, active sketch, open drawing, pending positions, undo history and
    /// section; returns the checkpoint's id.
    pub fn checkpoint(&mut self) -> u64 {
        let id = self.next_checkpoint;
        self.next_checkpoint += 1;
        self.checkpoints.push(Checkpoint {
            id,
            document_id: self.document_id.clone(),
            doc: self.doc.clone(),
            active_sketch: self.active_sketch,
            sketching_view: self.sketching_view,
            sketch_restore_marker: self.sketch_restore_marker(),
            active_sheet_sketch: self.active_sheet_sketch.clone(),
            open_drawing: self.open_drawing,
            undo: self.undo.clone(),
            pending_moves: self.pending_moves.clone(),
            visibility: self.visibility.clone(),
            redo: self.redo.clone(),
            redo_watch: self.redo_watch.clone(),
            section: self.section_state(),
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
                "no checkpoint {id} (used up by restoring it or an earlier one, or by forgetting it, or dropped: only the last {MAX_CHECKPOINTS} are kept)"
            ))
        };
        let i = self.checkpoints.iter().position(|c| c.id == id).ok_or_else(missing)?;
        let Some(c) = self.checkpoints.drain(i..).next() else { return Err(missing()) };
        let before = Snapshot::of(self, "");
        self.doc = c.doc;
        self.document_id = c.document_id;
        self.design_revision = solidcraft_doc::ids::new_design_revision();
        // A row put on the design after the checkpoint is gone with it.
        self.config_baseline = None;
        self.row_models.clear();
        self.sketching_view = self.historical_sketch_owner(c.active_sketch, c.sketching_view);
        self.active_sketch = c.active_sketch.filter(|_| self.sketching_view.is_some());
        if c.active_sketch.is_some() && self.active_sketch.is_none() {
            self.sketch_entry = None;
            if let Some(marker) = c.sketch_restore_marker {
                let marker = marker.filter(|position| *position < self.doc.features.len());
                if self.doc.marker != marker {
                    self.doc_mut().marker = marker;
                }
            }
        }
        self.active_sheet_sketch =
            c.active_sheet_sketch.filter(|entry| self.checked_sheet_sketch_owner(&self.view_context, Some(entry), &self.doc).is_ok());
        self.open_drawing = c.open_drawing;
        self.pending_moves = c.pending_moves;
        self.visibility = c.visibility;
        self.undo = c.undo;
        self.redo = c.redo;
        self.redo_watch = c.redo_watch;
        if self.section_state() != c.section {
            self.set_section_state(c.section);
        }
        self.clear_selection();
        self.prune_sheet_sketch_edit();
        if self.render_only_change(&before) {
            self.refresh_render(&before.doc);
        } else if self.visibility_only_change(&before) {
            self.refresh_display();
        } else if !Arc::ptr_eq(&self.doc, &before.doc) || self.active_sketch != before.active_sketch || self.pending_moves != before.pending_moves {
            self.refresh();
        }
        self.follow_design_token();
        Ok(())
    }

    /// Forget checkpoint `id` without going back. Later checkpoints stay: a batch forgets the
    /// checkpoint it took before its first step, and one a step of it took outlives the batch.
    pub fn drop_checkpoint(&mut self, id: u64) {
        self.checkpoints.retain(|c| c.id != id);
    }

    /// A scratch copy of the session (same design and model, no history or log) to try
    /// commands on without touching this one.
    pub fn scratch(&self) -> Session {
        let mut redo_watch: RedoWatch = self.redo_watch.as_ref().map(|watch| watch.keys().map(|part| (*part, false)).collect()).unwrap_or_default();
        for step in &self.redo {
            redo_watch.entry(step.part).or_insert(false);
        }
        Session {
            stop: self.stop.clone(),
            is_copy: true,
            background_checks: false,
            doc: self.doc.clone(),
            model: self.model.clone(),
            undo: Vec::new(),
            redo: Vec::new(),
            redo_watch: Some(redo_watch),
            active_sketch: self.active_sketch,
            sketching_view: self.sketching_view,
            view_context: self.view_context.clone(),
            sketch_finish_events: Default::default(),
            sketch_finish_sequence: self.sketch_finish_sequence,
            sketch_finish_pending: self.sketch_finish_pending,
            sketch_by_person: self.sketch_by_person,
            edit_source_sketch: self.edit_source_sketch,
            prepared_edit_source: self.prepared_edit_source.clone(),
            edit_entry_depth: 0,
            edit_start_sequence: 0,
            prepared_edit_intent: self.prepared_edit_intent.clone(),
            active_sheet_sketch: self.active_sheet_sketch.clone(),
            open_drawing: self.open_drawing,
            model_focus: self.model_focus,
            sketch_entry: self.sketch_entry.clone(),
            feature_edit_sketch: self.feature_edit_sketch,
            selection: self.selection.clone(),
            selection_all_bodies: self.selection_all_bodies,
            path: None,
            source_path: self.path.clone().or_else(|| self.source_path.clone()),
            ids: self.ids.clone(),
            uid: self.uid,
            disk: self.disk,
            read_only: self.read_only,
            provenance_chain: self.provenance_chain.clone(),
            provenance: self.provenance.clone(),
            signing_allowed: false,
            user_named: self.user_named,
            open_preview: None,
            saved: self.saved.clone(),
            log: Vec::new(),
            revision: self.revision,
            visibility_revision: self.visibility_revision,
            browser_stamp: self.browser_stamp,
            document_id: self.document_id.clone(),
            design_revision: self.design_revision.clone(),
            design_state: self.design_state.clone(),
            followed: self.followed.clone(),
            auto_project: self.auto_project,
            project_construction: self.project_construction,
            disabled_plugins: self.disabled_plugins.clone(),
            plugin_limits: self.plugin_limits,
            plugin_state: {
                let mut state = self.plugin_state.clone();
                state.link_reads = Default::default();
                state.link_read_source = state.link_changes.clone();
                state
            },
            plugin_forms_interactive: self.plugin_forms_interactive,
            param_check: self.param_check,
            easyprint_upload: self.easyprint_upload.clone(),
            keep_print_faces: self.keep_print_faces,
            linked_bodies: self.linked_bodies,
            live_links: None,
            active_component: self.active_component,
            active_occurrence_path: self.active_occurrence_path.clone(),
            pending_moves: self.pending_moves.clone(),
            exploded: self.exploded.clone(),
            config_baseline: self.config_baseline.clone(),
            row_models: self.row_models.clone(),
            drag_preview: None,
            world_cache: std::sync::Mutex::new(self.world_cache.lock().ok().and_then(|cache| cache.clone())),
            joints_settled: self.joints_settled.clone(),
            // Read again by the copy: handing it these references measured more peak memory on a
            // large design (the rover, evaluated ahead) for no change in what it pins.
            made_bodies: MadeBodies::new(),
            names_pinned: None,
            occurrence_visibility_cache: Default::default(),
            visibility_index_cache: Default::default(),
            consumed_sketches_cache: Default::default(),
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
            section: Vec::new(),
            section_hidden: Vec::new(),
            section_folder: None,
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
            // The open drawing's lines are the sheet page's: a copy for an export takes them
            // (`scratch_as_shown`), the many others never read them.
            drawing_scenes: Default::default(),
            saved_visibility: self.saved_visibility.clone(),
            dimension_values: self.dimension_values.clone(),
            opened_import: self.opened_import.clone(),
            evaluations: 0,
            #[cfg(test)]
            body_name_passes: 0,
            motion_traces: self.motion_traces.clone(),
            command_check: self.command_check.clone(),
            crash_journal: false,
            #[cfg(feature = "usage-stats")]
            count_usage: false,
        }
    }

    /// [`Session::scratch`], showing the drawing open as this session does: a deferred drawing held
    /// since its last update keeps its views' last lines. The copy a sheet export runs on beside the
    /// window.
    pub fn scratch_as_shown(&self) -> Session {
        Session { drawing_scenes: self.drawing_scenes.shown_copy(self.open_drawing), ..self.scratch() }
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
        // The temporary rollback is another revision of the same model inputs.
        expected.marker = source.doc.marker;
        // Rolling the marker solves the joints again and stores where that puts each occurrence...
        if expected.occurrences.len() == source.doc.occurrences.len() {
            for e in &mut expected.occurrences {
                if let Some(s) = source.doc.occurrences.iter().find(|s| s.id == e.id) {
                    e.transform = s.transform;
                    e.expressed_moves.clone_from(&s.expressed_moves);
                }
            }
        }
        // ...and stores the solved degrees of freedom, offsets, and angles of matching joints...
        if expected.assembly.joints.len() == source.doc.assembly.joints.len() {
            for e in &mut expected.assembly.joints {
                if let Some(s) = source.doc.assembly.joints.iter().find(|s| s.id == e.id) {
                    e.values.clone_from(&s.values);
                    e.offset = s.offset;
                    e.angle = s.angle;
                }
            }
        }
        // ...and works out each coupled joint's ratio and lift from the faces at that marker.
        if expected.assembly.links.len() == source.doc.assembly.links.len() {
            for e in &mut expected.assembly.links {
                if let Some(s) = source.doc.assembly.links.iter().find(|s| (s.a, s.ia, s.b, s.ib) == (e.a, e.ia, e.b, e.ib)) {
                    e.ratio = s.ratio;
                    e.lift.clone_from(&s.lift);
                }
            }
        }
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
        let stop = self.stop.clone();
        stop.within(|| self.compute_commands_under(commands, marker, original))
    }

    fn compute_commands_under(
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
                let all_bodies = computed.selection_all_bodies;
                let reply = computed.execute("timeline.roll_to", &params)?;
                computed.undo = undo;
                computed.redo = redo;
                computed.selection = selection;
                computed.selection_all_bodies = all_bodies;
                reply
            };
            let changed = !Arc::ptr_eq(&before, &computed.doc);
            steps.push(ComputedCommand { id: "timeline.roll_to".into(), params, before, after: computed.doc.clone(), changed, reply });
        }
        for (id, params) in commands {
            let before = computed.doc.clone();
            let active_sketch = computed.active_sketch;
            let sketching_view = computed.sketching_view;
            let active_sheet_sketch = computed.active_sheet_sketch.clone();
            let open_drawing = computed.open_drawing;
            let pending_moves = computed.pending_moves.clone();
            let reply = computed.execute(id, params)?;
            let changed = !Arc::ptr_eq(&before, &computed.doc)
                || active_sketch != computed.active_sketch
                || sketching_view != computed.sketching_view
                || active_sheet_sketch != computed.active_sheet_sketch
                || open_drawing != computed.open_drawing
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
        self.checked_sketch_owner(prepared.computed.active_sketch, prepared.computed.sketching_view)?;
        self.checked_sheet_sketch_owner(&self.view_context, prepared.computed.active_sheet_sketch.as_deref(), &prepared.computed.doc)?;
        // A plugin is evaluated only once on its worker. Recheck its precondition while
        // holding the live transaction, before accepting any of its computed steps.
        if !prepared.is_current(self) {
            for step in &prepared.steps {
                if (step.id.eq_ignore_ascii_case("plugin.run")
                    || step.id.split_once('.').is_some_and(|(package, _)| prepared.source.plugin_state.packages.contains_key(package)))
                    && let Some(expected) = step.params.get("expected_design").filter(|expected| !expected.is_null())
                {
                    let expected: craft_plugin::DesignToken =
                        serde_json::from_value(expected.clone()).map_err(|error| params::bad(&step.id, error.to_string()))?;
                    let actual = self.design_token();
                    if expected != actual {
                        return Err(craft_plugin::HostError::StaleRevision { expected, actual }.into());
                    }
                }
            }
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
            // Computed off the window thread on a scratch copy, which counts nothing: each command
            // that changed the design is the person's operation here, with the time it took.
            #[cfg(feature = "usage-stats")]
            if self.count_usage
                && step.changed
                && let Some(spec) = find_command(&step.id)
            {
                usage::note(usage::Bucket::Operations, spec.id);
                if let Some(ms) = step.reply.get("ms").and_then(Value::as_f64) {
                    usage::note_compute(spec.id, ms);
                }
            }
        }
        // A model worker that did not update panels must keep newer view state on
        // the window (including data merged into a frame or a panel the user closed).
        if prepared.computed.plugin_state.panels != prepared.source.plugin_state.panels {
            self.plugin_state.panels.clone_from(&prepared.computed.plugin_state.panels);
        }
        // Disk checks and no-op source reads may finish after a newer live observation.
        // Merge only entries whose source observation is still current in the live session.
        let observations: Vec<_> = prepared
            .computed
            .plugin_state
            .link_changes
            .values()
            .filter(|change| {
                let source = prepared.source.plugin_state.link_changes.get(&change.component);
                source != Some(*change) && self.plugin_state.link_changes.get(&change.component) == source
            })
            .cloned()
            .collect();
        let replies = prepared.steps.into_iter().map(|step| (step.id, step.reply)).collect();
        self.accept_computed(prepared.computed)?;
        for change in observations {
            if self.doc.links.get(&change.component).is_some_and(|link| link.path == change.path) {
                plugin::record_link_change(self, change.component, change.path, change.digest, change.read);
            }
        }
        Ok(replies)
    }

    fn same_document_context(&self, source: &Session) -> bool {
        Arc::ptr_eq(&self.doc, &source.doc)
            && self.document_id == source.document_id
            && self.design_revision == source.design_revision
            && self.ids.seed() == source.ids.seed()
            && self
                .revision
                .checked_sub(source.revision)
                .is_some_and(|delta| self.visibility_revision.checked_sub(source.visibility_revision) == Some(delta))
            && self.active_sketch == source.active_sketch
            && self.sketching_view == source.sketching_view
            && self.active_sheet_sketch == source.active_sheet_sketch
            && self.open_drawing == source.open_drawing
            && self.active_component == source.active_component
            && self.active_occurrence_path == source.active_occurrence_path
            && self.pending_moves == source.pending_moves
    }

    /// Cancel an unchanged feature editor by restoring its original evaluated model, even
    /// after preview work has evicted the old suffix from the shared evaluation cache.
    pub fn restore_editor(&mut self, source: &Session, mut original: Session) -> Result<()> {
        self.checked_sketch_owner(original.active_sketch, original.sketching_view)?;
        self.checked_sheet_sketch_owner(&self.view_context, original.active_sheet_sketch.as_deref(), &original.doc)?;
        if !self.same_document_context(source) {
            return Err(EngineError::Other("the design changed while the feature editor was open; its current timeline was preserved".into()));
        }
        let params = original.doc.marker.map(|position| serde_json::json!({"position":position})).unwrap_or_else(|| serde_json::json!({}));
        crash::accepted_command(self, &self.doc, &original.doc, "timeline.roll_to", &params, !Arc::ptr_eq(&self.doc, &original.doc));
        self.restore_editor_history(&source.doc, &original.doc);
        let undo = std::mem::take(&mut self.undo);
        let redo = std::mem::take(&mut self.redo);
        let selection = std::mem::take(&mut self.selection);
        let all_bodies = self.selection_all_bodies;
        let redo_watch = self.redo_watch.clone();
        let ids = self.ids.clone();
        original.visibility.hidden_joints.clear();
        // The editor's original authored snapshot carries an old session token.
        // Cancelling restores its bytes, while remaining a fresh traversal.
        original.design_revision = solidcraft_doc::ids::new_design_revision();
        original.follow_design_token();
        self.accept_computed(original)?;
        self.undo = undo;
        self.redo = redo;
        // Cancelling the editor restores its history, not a newly edited branch.
        self.redo_watch = redo_watch;
        self.ids = ids;
        self.selection = selection;
        self.selection_all_bodies = all_bodies;
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

    /// Retain scratch provenance for relevant owners even after MAX_UNDO eviction.
    /// Drawing undo/redo and lifecycle commands cannot be classified by their
    /// generic CommandSpec part, so inspect the actual drawing/model delta.
    /// Visibility belongs to the model history even when the document is unchanged.
    fn note_redo_changes(&mut self, previous: &Document, hidden: bool) {
        let Some(watch) = &mut self.redo_watch else { return };
        let model_changed = watch.get(&None) == Some(&false)
            && (hidden
                || self.doc.drawings.len() != previous.drawings.len()
                || previous.drawings.iter().any(|drawing| self.doc.drawing(drawing.id).is_none())
                || (!self.doc.differs_only_in_drawings(previous) && *self.doc != *previous));
        for (part, touched) in watch {
            if !*touched {
                *touched = match part {
                    None => model_changed,
                    Some(id) => match (self.doc.drawing(*id), previous.drawing(*id)) {
                        (Some(after), Some(before)) => !Arc::ptr_eq(after, before),
                        (None, None) => false,
                        _ => true,
                    },
                };
            }
        }
    }

    /// Commit commands evaluated on a scratch copy without losing this session's file,
    /// saved state, view state, checkpoints or earlier undo steps. A vanished sketch owner is
    /// refused before any part of the computed transaction is installed.
    pub fn accept_computed(&mut self, mut computed: Session) -> Result<()> {
        let owner = self.checked_sketch_owner(computed.active_sketch, computed.sketching_view)?;
        self.checked_sheet_sketch_owner(&self.view_context, computed.active_sheet_sketch.as_deref(), &computed.doc)?;
        computed.sketching_view = owner;
        // Audited direct-mutation jobs may not have passed through execute. Their
        // accepted document is still a new committed state with a fresh token.
        let newer = match (design_revision_generation(&computed.design_revision), design_revision_generation(&self.design_revision)) {
            (Some((incoming_epoch, incoming)), Some((current_epoch, current))) => incoming_epoch == current_epoch && incoming > current,
            _ => false,
        };
        // Reads usually share their document; only an older or untracked candidate
        // needs a content comparison. A newly issued revision already names its edit.
        let same_content = Arc::ptr_eq(&computed.doc, &self.doc) || (!newer && *computed.doc == *self.doc);
        if !newer && computed.design_revision != self.design_revision {
            // A retained original editor or another historical candidate must
            // not restore an issued token, even when it restores authored bytes.
            computed.design_revision = if same_content { self.design_revision.clone() } else { solidcraft_doc::ids::new_design_revision() };
            computed.follow_design_token();
        } else if computed.design_revision == self.design_revision && same_content {
            computed.follow_design_token();
        } else {
            computed.commit_design_token();
        }
        let previous_revision = self.revision;
        // Audited direct-mutation callers edit one current owner without commands.
        // Command provenance takes precedence if nested tools erased their history.
        let tracked = computed.redo_watch.as_ref().is_some_and(|watch| watch.values().any(|touched| *touched));
        let implicit_part = (computed.undo.is_empty() && computed.redo.is_empty() && !tracked && !Arc::ptr_eq(&self.doc, &computed.doc))
            .then_some(computed.open_drawing);
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
        // Only committed source reads cross back from the worker. A no-op read must not
        // replace a disk observation made since this evaluation began; changed source bytes
        // being committed still describe a new design state and publish their read observation.
        let link_reads: Vec<_> = computed
            .plugin_state
            .link_changes
            .values()
            .filter(|change| {
                let source_changed = self
                    .doc
                    .links
                    .get(&change.component)
                    .is_none_or(|link| link.path != change.path || link.file_sha256.as_ref() != Some(&change.digest));
                let observed = self.plugin_state.link_changes.get(&change.component);
                let started = computed.plugin_state.link_read_source.get(&change.component);
                change.read
                    && computed.plugin_state.link_reads.contains(&change.component)
                    && computed
                        .doc
                        .links
                        .get(&change.component)
                        .is_some_and(|link| link.path == change.path && link.file_sha256.as_ref() == Some(&change.digest))
                    && (source_changed || observed == started)
            })
            .cloned()
            .collect();
        self.doc = computed.doc;
        for change in link_reads {
            Arc::make_mut(&mut self.plugin_state.link_reads).insert(change.component);
            plugin::record_link_change(self, change.component, change.path, change.digest, true);
        }
        self.plugin_state.prune_link_changes(&self.doc);
        self.design_state = computed.design_state;
        self.design_revision = computed.design_revision;
        self.ids = computed.ids;
        self.model = computed.model;
        self.release_left();
        self.world_cache = computed.world_cache;
        let replaces = |part| {
            Some(part) == implicit_part
                || computed.redo_watch.as_ref().and_then(|watch| watch.get(&part)).copied().unwrap_or(false)
                || computed.undo.iter().chain(&computed.redo).any(|edit| edit.part == part)
        };
        self.redo.retain(|step| !replaces(step.part));
        if let Some(watch) = &mut self.redo_watch {
            // Propagate only this parent's already-watched keys. Child-local redo
            // owners do not turn this into a growing, lifetime edit log.
            for (part, touched) in watch {
                *touched |= replaces(*part);
            }
        }
        self.redo.append(&mut computed.redo);
        self.undo.append(&mut computed.undo);
        if self.undo.len() > MAX_UNDO {
            self.undo.drain(..self.undo.len() - MAX_UNDO);
        }
        self.active_sketch = computed.active_sketch;
        self.sketching_view = computed.sketching_view;
        self.active_sheet_sketch = computed.active_sheet_sketch;
        self.open_drawing = computed.open_drawing;
        self.prune_sheet_sketch_edit();
        drawing_scene::forget_closed(self);
        if self.model_focus == computed.model_focus {
            self.selection = computed.selection;
            self.selection_all_bodies = computed.selection_all_bodies;
        }
        self.revision = computed.revision;
        self.invalidate_view(previous_revision);
        self.active_component = computed.active_component;
        self.active_occurrence_path = computed.active_occurrence_path;
        self.pending_moves = computed.pending_moves;
        self.exploded = computed.exploded;
        self.config_baseline = computed.config_baseline;
        self.row_models = computed.row_models;
        // Joints the computation hid (an import's rigid group) stay hidden.
        for j in computed.visibility.hidden_joints {
            if !self.visibility.hidden_joints.contains(&j) {
                self.visibility.hidden_joints.push(j);
            }
        }
        self.sketch_finish_pending = computed.sketch_finish_pending;
        self.sketch_by_person = computed.sketch_by_person;
        self.prepared_edit_source = computed.prepared_edit_source;
        self.prepared_edit_intent = computed.prepared_edit_intent;
        self.edit_start_sequence = self.edit_start_sequence.saturating_add(computed.edit_start_sequence);
        for event in computed.sketch_finish_events {
            self.publish_sketch_finish(event);
        }
        for message in computed.log {
            self.echo(message);
        }
        Ok(())
    }

    /// Read a primary origin for the Joint dialog without modifying the session.
    pub fn joint_origin_for_alignment(&self, params: &serde_json::Value) -> Result<doc::joints::JointOrigin> {
        cmd::joints::origin_param(self, Some(params), "joint.create")
    }

    /// Validate a secondary selection through the command resolver without authoring it.
    pub fn joint_alignment_frame(&self, origin: doc::joints::JointOrigin, alignment: &serde_json::Value) -> Result<doc::Mat> {
        let origin = cmd::joints::alignment_param(self, origin, Some(alignment), "joint.create")?;
        doc::joints::resolve_origin(&self.doc, &self.model.state(), &origin).map_err(Into::into)
    }

    /// Live preview: run `commands` (id, parameters) on a scratch copy and return the model
    /// before and after. Nothing in this session changes (document, undo history, selection).
    /// A `timeline.redefine` is shown even while the timeline is rolled back to the feature
    /// being edited.
    pub fn preview(&self, commands: &[(String, Value)]) -> Result<Preview> {
        self.scratch()
            .preview_in_place(commands, None)
            .map(|(after, replies)| Preview::new(self.model.state(), after.model.state(), self, &after, replies))
    }

    /// The same evaluated preview in occurrence coordinates, including uncaptured moves.
    pub fn preview_world(&self, commands: &[(String, Value)]) -> Result<Preview> {
        let (after, replies) = self.scratch().preview_in_place(commands, None)?;
        Ok(Preview::new(self.world_state()?, after.world_state()?, self, &after, replies))
    }

    /// World preview with each session's visibility applied, including new body placements.
    pub fn preview_visible_world(&self, commands: &[(String, Value)]) -> Result<Preview> {
        let (after, replies) = self.scratch().preview_in_place(commands, None)?;
        Preview::visible_world(self, &after, replies)
    }

    /// Run `commands` on a [`Session::scratch`] copy that keeps the undo and redo stacks: what
    /// they evaluate lands in the evaluation cache the copy shares with this session, so running
    /// them here afterwards finds it there. For a front end that computes a command which may
    /// take a while (undo, the history marker moved) off its window thread first.
    pub fn ahead(&self) -> Ahead {
        let mut copy = self.scratch();
        copy.undo.clone_from(&self.undo);
        copy.redo.clone_from(&self.redo);
        copy.sketch_entry.clone_from(&self.sketch_entry);
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
                if let Some(r) = self.model.result(fid)
                    && let Some(e) = r.error.clone()
                {
                    return Err(EngineError::of_feature(r, e));
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
        let run = self.run_script_kept(script, profile, each)?;
        match run.failure {
            Some(f) => Err(f.into_error()),
            None => Ok((run.results, run.timings)),
        }
    }

    /// [`Session::run_script_each`], timing each command as [`Session::run_script_timed`] does
    /// when `profile` is given, that keeps what ran when a step fails (#19964): the results of
    /// the steps before it, the timings of those and of the failing one, and the failure. Only a
    /// script that is not a list of steps is an error.
    pub fn run_script_kept(&mut self, script: &Value, profile: Option<bool>, each: &mut dyn FnMut(usize, &Value)) -> Result<ScriptRun> {
        let list = script_steps(script)?;
        let mut results = Vec::new();
        let mut steps = Vec::new();
        let mut failure = None;
        let clock = timing::Clock::start();
        for (i, c) in list.iter().enumerate() {
            let Some(id) = c.get("command").or_else(|| c.get("id")).and_then(Value::as_str) else {
                failure = Some(StepFailure { index: i, command: None, error: EngineError::Other("missing `command`".into()) });
                break;
            };
            let p = c.get("params").cloned().unwrap_or(Value::Null);
            let r = match profile {
                Some(profile) => {
                    let (r, step) = timing::time(profile, || self.execute(id, &p));
                    steps.push(step.to_json(i, id));
                    r
                }
                None => self.execute(id, &p),
            };
            match r {
                Ok(r) => {
                    each(i, &r);
                    results.push(r);
                }
                Err(error) => {
                    failure = Some(StepFailure { index: i, command: Some(id.to_string()), error });
                    break;
                }
            }
        }
        let timings = profile.map(|_| serde_json::json!({"timings": steps, "total_ms": timing::round_ms(clock.ms())}));
        Ok(ScriptRun { results, timings, failure })
    }
}

/// What [`Session::run_script_kept`] ran.
#[derive(Debug)]
pub struct ScriptRun {
    /// The result of each step that worked, in order.
    pub results: Vec<Value>,
    /// When timed: `{"timings": [{"index", "command", "ms", "profile"?}], "total_ms"}`, the
    /// failing step's time included.
    pub timings: Option<Value>,
    /// The step that failed, which ended the run; `None` when every step worked.
    pub failure: Option<StepFailure>,
}

/// The step a script stopped at.
#[derive(Debug)]
pub struct StepFailure {
    /// The step's index, from 0.
    pub index: usize,
    /// The command it named; `None` when it named none.
    pub command: Option<String>,
    /// Its error.
    pub error: EngineError,
}

impl StepFailure {
    /// `{"step": its number from 1, "command"?, "error": the error's words}`.
    pub fn to_json(&self) -> Value {
        let mut v = serde_json::json!({"step": self.index + 1, "error": self.error.to_string()});
        if let (Some(o), Some(c)) = (v.as_object_mut(), &self.command) {
            o.insert("command".into(), Value::String(c.clone()));
        }
        v
    }

    /// The error a script that stops here returns, named after the step, as it always was.
    pub fn into_error(self) -> EngineError {
        match self.command {
            Some(id) => EngineError::Other(format!("step {} ({id}): {}", self.index + 1, self.error)),
            None => EngineError::Other(format!("script step {}: {}", self.index + 1, self.error)),
        }
    }
}

/// A script's steps: the JSON array itself, or the `commands` array of an object; at most 100 000.
pub fn script_steps(script: &Value) -> Result<&Vec<Value>> {
    let list = match script {
        Value::Array(a) => a,
        Value::Object(o) => {
            o.get("commands").and_then(Value::as_array).ok_or_else(|| EngineError::Other("script: missing `commands` array".into()))?
        }
        _ => return Err(EngineError::Other("script must be a JSON array or {\"commands\": [...]}".into())),
    };
    if list.len() > 100_000 {
        return Err(EngineError::Other("script too long".into()));
    }
    Ok(list)
}

#[cfg(test)]
mod boolean_regression_tests;
#[cfg(test)]
mod browser_find_tests;
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
mod drawing_dimension_tests;
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
mod loft_sheet_tests;
#[cfg(test)]
mod measure_centre_tests;
#[cfg(test)]
mod measure_surface_tests;
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

/// What each drawing dimension last read, by drawing and dimension.
pub(crate) type DimensionValues = std::collections::BTreeMap<(u64, u64), f64>;

/// A design as its file holds it: `doc` with what is hidden in it and what its dimensions last read.
pub(crate) fn as_saved<'a>(doc: &'a Document, visibility: &Visibility, dimension_values: &DimensionValues) -> std::borrow::Cow<'a, Document> {
    if visibility.is_empty() && dimension_values.is_empty() {
        return std::borrow::Cow::Borrowed(doc);
    }
    let mut copy = doc.clone();
    if !visibility.is_empty() {
        copy.visibility = visibility.clone();
    }
    copy.keep_last_good(dimension_values);
    std::borrow::Cow::Owned(copy)
}

/// The values of a sketch's driving dimensions that come from parameters: what a value edit
/// changes in it (as opposed to geometry it projects or links from upstream).
fn own_values(sk: &solidcraft_sketch::Sketch) -> std::collections::BTreeMap<String, f64> {
    let mut values: std::collections::BTreeMap<_, _> = sk
        .constraints
        .iter()
        .filter(|c| c.param.is_some() && !c.driven)
        .filter_map(|c| {
            let v = match c.kind {
                solidcraft_sketch::ConstraintKind::OffsetDistance { value, reverse, .. } => {
                    if reverse {
                        -value
                    } else {
                        value
                    }
                }
                _ => c.kind.value()?,
            };
            Some((c.id.clone(), v))
        })
        .collect();
    // 3D dimensions, and 3D points placed by expressions (#10608).
    for c in &sk.wire_constraints {
        match &c.kind {
            solidcraft_sketch::WireConstraintKind::Fix { at, exprs: Some(_), .. } => {
                values.extend([(format!("{}.x", c.id), at.x), (format!("{}.y", c.id), at.y), (format!("{}.z", c.id), at.z)]);
            }
            k if c.param.is_some() && !c.driven => values.extend(k.value().map(|v| (c.id.clone(), v))),
            _ => {}
        }
    }
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

/// Whether a saved file is the current design or one of its linked components.
pub fn session_uses_file(session: &Session, path: &str) -> bool {
    cmd::file_library_uses_file(session, path)
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

/// Only the host compares generation order internally, to identify historical
/// candidate tokens. Plugin clients continue to compare complete tokens for equality.
fn design_revision_generation(token: &str) -> Option<(&str, u64)> {
    if token.len() > 128 {
        return None;
    }
    let (epoch, generation) = token.split_once(':')?;
    if epoch.len() != 32
        || !epoch.bytes().all(|byte| byte.is_ascii_hexdigit())
        || generation.is_empty()
        || generation.len() > 20
        || !generation.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    Some((epoch, generation.parse().ok()?))
}

/// Whether two designs differ at most in their features. Every field is named, so a new one has
/// to be decided on here.
fn same_but_features(a: &Document, b: &Document) -> bool {
    let Document {
        newer_file: _,
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
        collision_shapes,
        user_materials,
        appearances,
        body_components,
        body_offsets,
        browser_groups,
        timeline_groups,
        browser_order,
        named_views,
        notes,
        visibility,
        drawings,
        next_component_color_index,
        plugin_data,
        linked_bodies_left_out,
    } = a;
    *plugin_data == b.plugin_data
        && *linked_bodies_left_out == b.linked_bodies_left_out
        && *render == b.render
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
        && *collision_shapes == b.collision_shapes
        && *user_materials == b.user_materials
        && *appearances == b.appearances
        && *body_components == b.body_components
        && *body_offsets == b.body_offsets
        && *browser_groups == b.browser_groups
        && *timeline_groups == b.timeline_groups
        && *browser_order == b.browser_order
        && *named_views == b.named_views
        && *notes == b.notes
        && *visibility == b.visibility
        && *drawings == b.drawings
        && *next_component_color_index == b.next_component_color_index
}

/// Store `pins` (sketch, link, vertex name, edge name) on the links they name: a name the link
/// already has stays, and the link is no longer marked `name_dropped`.
pub(crate) fn store_link_names(doc: &mut Document, pins: Vec<(u64, String, Option<String>, Option<String>)>) {
    for (id, link, vertex, edge) in pins {
        let Ok(sk) = doc.sketch_mut(id) else { continue };
        if let Some(l) = sk.links.iter_mut().find(|l| l.id == link) {
            l.vertex_name = l.vertex_name.take().or(vertex);
            l.edge_name = l.edge_name.take().or(edge);
            l.name_dropped = false;
        }
    }
}
