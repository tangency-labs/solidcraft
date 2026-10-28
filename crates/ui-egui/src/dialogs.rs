//! Command dialogs (shown at the right of the viewport). A dialog collects values and selection
//! inputs (geometry picked in the viewport, never typed), then runs its command; it never
//! changes the design itself. Things selected before the command starts become its input.

use crate::ui_kit::choice_row;
use std::hash::{Hash, Hasher};

use egui::{RichText, vec2};
use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::Session;
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::doc::{
    AxisRef, Direction, FeatureKind, FilletStyle, HoleKind, Operation, PatternKind, PlaneRef, ProfileSel, ThinSide, ThreadHandedness,
};
use solidcraft_engine::geom::{Aabb3, Vec3};

use crate::SolveApp;
use crate::selection::{
    self, AXES, AXIS_PICKS, Accept, BODIES, CURVES, EDGES, FACES, PLANAR_FACES, PLANES, PROFILES, SelInput, SelectionExt as _, point_value as pt,
    polyline_mid,
};
use crate::theme::Tokens;
use crate::tool_modifiers::{Audit, Scope, ToolModifiers};
use crate::viewport::Hit;

#[path = "dialogs_dense.rs"]
mod dense;
#[cfg(test)]
pub(crate) use dense::primitive_label;

const SELECT_ONLY: &[Scope] = &[Scope::Selection, Scope::FeaturePreview];
const SELECT_MANIP: &[Scope] = &[Scope::Selection, Scope::Manipulator, Scope::FeaturePreview];

/// Per-tool modifier descriptors owned by this host file (SOLID / construct / inspect dialogs).
pub const MODIFIERS: &[ToolModifiers] = &[
    ToolModifiers { tool: "sketch.create", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.extrude", scopes: SELECT_MANIP, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.revolve", scopes: SELECT_MANIP, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.fillet", scopes: SELECT_MANIP, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.chamfer", scopes: SELECT_MANIP, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.shell", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.draft", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.mirror", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.rib", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.web", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.emboss", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.replace_face", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.boundary_fill", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.align", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.remove", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    // Pattern parameters are dialog-only - no viewport modifier chords.
    ToolModifiers { tool: "solid.pattern.path", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.pattern.rectangular", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.pattern.circular", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.pipe", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.bounding_solid", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "construct.plane.offset", scopes: SELECT_MANIP, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "construct.plane.along_path", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "construct.plane.at_angle", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.split_body", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.scale", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.offset_face", scopes: SELECT_MANIP, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.thread", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "material.assign", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "inspect.section", scopes: SELECT_MANIP, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "inspect.measure", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.loft", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.sweep", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    // Move: no Alt-copy / inventable chords; inherits Selection + Manipulator only.
    ToolModifiers { tool: "solid.move", scopes: SELECT_MANIP, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.hole", scopes: SELECT_MANIP, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.box", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.cylinder", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.sphere", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.torus", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.cone", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.coil", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "solid.combine", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
];

pub(crate) const OPS: [&str; 4] = ["new", "join", "cut", "intersect"];
pub(crate) const OP_LABELS: [&str; 4] = ["New Body", "Join", "Cut", "Intersect"];
const DIRS: [&str; 5] = ["positive", "negative", "symmetric", "positive", "negative"];
/// The "Two sides" direction (a second distance the other way).
pub const TWO_SIDES: usize = 3;
/// Two sides, reversed: the first side goes against the normal and the second along it (#2700).
pub const TWO_SIDES_REVERSED: usize = 4;

/// An extrude direction with a second side.
pub fn two_sided(direction: usize) -> bool {
    direction == TWO_SIDES || direction == TWO_SIDES_REVERSED
}
/// Thin Extrude wall locations, as `solid.extrude` names them, and their labels.
const WALL_SIDES: [&str; 4] = ["center", "inside", "outside", "two_sides"];
const WALL_LABELS: [&str; 4] = ["Center", "Inside", "Outside", "Two Sides"];
/// What an extrude's first input picks: profiles or planar faces, or curves too when thin.
const EXTRUDE_ACCEPT: Accept = PROFILES | PLANAR_FACES;
const THIN_ACCEPT: Accept = PROFILES | CURVES;
const PROJECT_GEOMETRY: Accept =
    EDGES | FACES | selection::VERTICES | PLANES | AXES | selection::SKETCH_LINES | selection::EXTERNAL_POINTS | selection::ORIGIN;
/// What Intersect cuts with the sketch plane, picked one by one: faces, model edges and the
/// curves of other sketches (#10484); an edge or a sketch line gives the points where it crosses.
const INTERSECT_GEOMETRY: Accept = EDGES | FACES | selection::SKETCH_LINES;
const HOLE_TYPES: [&str; 3] = ["simple", "counterbore", "countersink"];
const HOLE_LABELS: [&str; 3] = ["Simple", "Counterbore", "Countersink"];

crate::dialog_descriptor::dialog_descriptors! {
    #[derive(Clone, Debug)]
    pub enum Kind {
        Shell("solid.shell", "SHELL", [("Faces/Body", FACES, true)],
            FeatureKind::Shell { thickness, .. }) {
            thickness: String = "2 mm".into() => shown_expr(thickness, ValueKind::Length),
        } => json!({"thickness": thickness});
        Draft("solid.draft", "DRAFT", [("Faces", FACES, true), ("Neutral plane", PLANES | PLANAR_FACES, false)],
            FeatureKind::Draft { angle, .. }) {
            angle: String = "5 deg".into() => shown_expr(angle, ValueKind::Angle),
        } => json!({"angle": angle});
        // A point picked as well puts the plane through it instead of at the distance.
        OffsetPlane("construct.plane.offset", "OFFSET PLANE", [("Plane", PLANES | PLANAR_FACES, false), ("Through point", selection::VERTICES | selection::POINTS, false)],
            FeatureKind::ConstructionPlane { plane: PlaneRef::Offset { distance, .. } }) {
            offset: String = "0 mm".into() => shown_expr(distance, ValueKind::Length),
        } => json!({"offset": offset});
        AnglePlane("construct.plane.at_angle", "PLANE AT ANGLE", [("Plane", PLANES | PLANAR_FACES, false), ("Axis", AXIS_PICKS, false)],
            FeatureKind::ConstructionPlane { plane: PlaneRef::AtAngle { angle, .. } }) {
            angle: String = "45 deg".into() => shown_expr(angle, ValueKind::Angle),
        } => json!({"angle": angle});
        Scale("solid.scale", "SCALE", [("Bodies", BODIES, true)],
            FeatureKind::Scale { factor, .. }) {
            factor: String = "2".into() => factor.to_string(),
        } => json!({"factor": factor});
        OffsetFaces("solid.offset_face", "OFFSET FACE", [("Faces", FACES, true)],
            FeatureKind::OffsetFace { distance, .. }) {
            distance: String = "2 mm".into() => shown_expr(distance, ValueKind::Length),
        } => json!({"distance": distance});
        Stock("solid.bounding_solid", "STOCK", [("Bodies (all if none)", BODIES, true)],
            FeatureKind::BoundingSolid { margin, .. }) {
            margin: String = "2 mm".into() => shown_expr(margin, ValueKind::Length),
        } => json!({"margin": margin});
        Pipe("solid.pipe", "PIPE", [("Path", CURVES, true)],
            FeatureKind::Pipe { diameter, wall, operation, .. }) {
            diameter: String = "5 mm".into() => shown_expr(diameter, ValueKind::Length),
            wall: String = String::new() => wall.as_deref().map(|e| shown_expr(e, ValueKind::Length)).unwrap_or_default(),
            operation: usize = 0 => op_index(operation),
        } => json!({"diameter": diameter, "operation": OPS.get(*operation).copied().unwrap_or("new")});
        Emboss("solid.emboss", "EMBOSS", [("Profiles", PROFILES, true), ("Target Face (optional)", FACES, false)],
            FeatureKind::Emboss { depth, deboss, .. }) {
            depth: String = "1 mm".into() => shown_expr(depth, ValueKind::Length),
            deboss: bool = false => *deboss,
        } => json!({"depth": depth, "mode": if *deboss { "deboss" } else { "emboss" }});
        ; custom {
            /// Pick a plane or planar face; the sketch starts on it at once.
            Sketch,
            Extrude {
                distance: String,
                direction: usize,
                operation: usize,
                /// Into a body: cut; additions: join when a solid exists, otherwise new.
                /// until the user picks one.
                auto_op: bool,
                /// The direction from the picked side and the latched operation (#1257).
                smart: crate::smart_extrude::Smart,
                /// The other side's distance (two sides).
                distance2: String,
                /// Taper angle (empty or zero: none).
                taper: String,
                /// An explicitly authored zero taper is still a model parameter.
                taper_present: bool,
                /// Optional distance beyond the selected end object.
                to_offset: String,
                /// Start offset from the profile plane (empty: start on the profile plane).
                start: String,
                /// Extent type All (through everything) instead of a distance.
                all: bool,
                /// Thin Extrude: a wall along the picked curves (or profiles' outlines).
                thin: bool,
                /// The wall thickness (inside, with Two Sides).
                wall: String,
                /// Wall location: an index into [`WALL_SIDES`].
                wall_side: usize,
                /// The thickness outside, with Two Sides.
                wall2: String,
            },
            Revolve {
                angle: String,
                operation: usize,
                /// An arbitrary stored line until the user deliberately picks a replacement.
                axis: Option<Value>,
            },
            Fillet {
                radius: String,
                chamfer: bool,
                chain: bool,
                /// Chamfer type: 0 equal distance, 1 two distances, 2 distance and angle; with the
                /// second distance, the angle and which face the first distance is on.
                ctype: usize,
                distance2: String,
                angle: String,
                flip: bool,
            },
            Mirror,
            /// Rectangular pattern of bodies: along one direction, optionally a second.
            PatternRect {
                count: String,
                spacing: String,
                count2: String,
                spacing2: String,
            },
            /// Circular pattern of bodies around an axis.
            PatternCirc {
                count: String,
                angle: String,
            },
            /// Loft through profiles of different sketches, in pick order; `rail_fit` how its guide
            /// rails reshape the sections (an index into the sweep dialog's rail fits).
            Loft {
                operation: usize,
                rail_fit: usize,
            },
            /// Sweep a profile along a path of sketch curves.
            Sweep {
                operation: usize,
                opts: crate::dialogs_sweep::SweepOpts,
            },
            /// Halfway between two planes or planar faces.
            Midplane,
            /// An imported mesh body rebuilt as editable faces (#3001).
            MeshConvert {
                tolerance: String,
            },
            /// A construction axis, point or plane made from picks alone (#2474): input `i` is the
            /// command's parameter `keys[i]`, and `t` (along a path) when it takes one.
            Construct {
                command: &'static str,
                title: &'static str,
                keys: Vec<&'static str>,
                t: Option<String>,
            },
            /// A plane perpendicular to one selected path at a fractional distance.
            PathPlane {
                t: String,
                sketch: Option<u64>,
            },
            /// Split a body by a plane.
            Split,
            /// A cosmetic thread on a cylindrical face.
            Thread {
                designation: String,
                length: String,
                handedness: ThreadHandedness,
                /// The last automatic size; a different value is a manual override. Keep automatic
                /// features automatic when applying, even though their resolved size is visible.
                suggested: Option<String>,
                /// The picks used for the suggestion, so an unchanged dialog does no geometry work.
                of: Vec<Sel>,
            },
            /// A rib (one wall along open curves) or a web (a wall per curve).
            Rib {
                web: bool,
                thickness: String,
                depth: String,
                flip: bool,
                /// Grow along the sketch normal rather than across the plane.
                normal: bool,
            },
            /// Move planar faces onto a target plane or face.
            DeleteFaces,
            ReplaceFace,
            /// Boundary Fill: the cells the tool bodies make (a point inside, a description, filled
            /// or not), found again when the tools change.
            BoundaryFill {
                operation: usize,
                cells: Vec<(Vec3, String, bool)>,
                of: Vec<Sel>,
            },
            /// Move bodies or component occurrences so a face or point meets another.
            Align {
                flip: bool,
                components: bool,
                /// Point-to-point: which of the world X, Y and Z axes take the offset.
                axes: [bool; 3],
            },
            /// Remove bodies from here on in the timeline.
            Remove,
            /// Pattern bodies along a path of sketch curves.
            PathPattern {
                count: String,
                spacing: String,
            },
            /// Physical material (and with it the appearance) of bodies.
            Material {
                index: usize,
                editor: crate::material_editor::Editor,
            },
            /// Section Analysis: cut the view by a plane, moved along its normal.
            Section {
                offset: String,
                flip: bool,
                /// Hatch the cut faces.
                hatch: bool,
            },
            /// Measure one or two picked items (vertices, edges, faces): the result of the last
            /// measurement and the items it was for.
            Measure {
                result: Option<Value>,
                of: Vec<Sel>,
                /// The measure chosen in the panel (centre, min or max); `None` follows the result's
                /// own default (centre where a round item gives one).
                show: Option<Shown>,
                /// Where the labels in the view were dragged to (#8510).
                labels: crate::canvas::MeasureLabels,
            },
            /// Move bodies by a distance along X, Y and Z.
            /// Move/Copy (see `dialogs_move`): the move type, X/Y/Z distances (Point to Position: the
            /// target), X/Y/Z angles (Free Move) and the angle (Rotate).
            Move {
                mode: usize,
                x: String,
                y: String,
                z: String,
                rx: String,
                ry: String,
                rz: String,
                angle: String,
            },
            Hole {
                diameter: String,
                depth: String,
                kind: usize,
                cb_diameter: String,
                cb_depth: String,
                cs_diameter: String,
                cs_angle: String,
                opts: HoleOpts,
            },
            Primitive {
                cmd: &'static str,
                fields: Vec<(&'static str, String)>,
                operation: usize,
                /// Where it goes: a picked point on a face or vertex, with the face's normal when the
                /// face is planar. `None` keeps the command's own placement (the origin, or the edited
                /// feature's).
                at: Option<(Vec3, Option<Vec3>)>,
                /// A Cylinder's reference edge distances on that face (`hole_refs`, #6380).
                refs: Box<crate::hole_refs::Refs>,
            },
            Combine {
                operation: usize,
                keep_tools: bool,
            },
            EditParam {
                name: String,
                expr: String,
            },
            Rename {
                feature: u64,
                name: String,
            },
            /// Delete a feature that others depend on.
            ConfirmDelete {
                features: Vec<u64>,
                /// Timeline folders deleted with the features.
                groups: Vec<u64>,
                with: Vec<String>,
                fail: Vec<String>,
            },
            /// Pick what the agent chat's next message is about (#2286): elements, features and bodies.
            /// OK hands the picks to the chat (`agent_chat::pick_done`); nothing is run.
            ChatContext,
            /// Joints and components (`dialogs_assembly`).
            Assembly(crate::dialogs_assembly::Asm),
            /// Sheet metal (`dialogs_sheet`).
            Sheet(crate::dialogs_sheet::Sm),
            /// Surface tools and Split Face (`dialogs_surface`).
            Surface(crate::dialogs_surface::Surf),
            SaveMesh(crate::dialogs_save_mesh::SaveMesh),
            ViewDxf {
                dashed: bool,
                tolerance: f64,
            },
            /// Insert SVG / DXF: where the picked drawing goes (`dialogs_insert_drawing`).
            InsertDrawing(crate::dialogs_insert_drawing::Place),
            /// Plastic features (`dialogs_plastic`).
            Plastic(crate::dialogs_plastic::Pl),
            /// Frame members (`dialogs_frame`).
            Frame(crate::dialogs_frame::Member),
            /// Appearance (`dialogs_appearance`).
            Appearance(crate::dialogs_appearance::Ap),
            /// Contact sets, motion studies, exploded views, configurations (`dialogs_motion`).
            Motion(crate::dialogs_motion::Mo),
            /// Insert Part from the standard parts library (`dialogs_parts`).
            Part(crate::dialogs_parts::Pt),
            PartSize(crate::dialogs_part_size::Resize),
            /// Edit Text: a sketch text's content and placement (`sketch_text`).
            Text(Box<crate::sketch_text::Tx>),
            TextMove(crate::sketch_text_move::TextMove),
            /// Sketch-space modify tools: scale and offset, and later fillet, chamfer, the patterns,
            /// mirror and move (`sketch_modify_ui`).
            SketchModify(crate::sketch_modify_ui::Mod),
            SketchIntersect {
                bodies: bool,
                projection_link: bool,
            },
            SketchProject {
                bodies: bool,
                projection_link: bool,
            },
        }
    }
}

/// The Single hole placement's input: places on planar faces. A pick on or near a sketch point
/// that lies on the face (a circle's centre) is taken at that point, as Multiple picks it.
/// Holes go on any face (flat or curved; the engine drills against the face's normal there).
const HOLE_FACES: Accept = PLANAR_FACES | FACES | selection::POINTS;

/// The Single hole placement's inputs: the face, then up to two reference edges (#37).
fn single_hole_inputs() -> Vec<SelInput> {
    let mut v = vec![SelInput::new("Face", HOLE_FACES, true)];
    v.extend(crate::hole_refs::inputs());
    v
}

/// The rest of the Hole dialog: placement, extents, tap type and drill point.
#[derive(Clone, Debug, PartialEq)]
pub struct HoleOpts {
    /// Placement at several sketch points instead of one face point.
    pub multiple: bool,
    /// Extents: through all instead of a distance.
    pub all: bool,
    /// Drill point: an angled tip instead of a flat bottom.
    pub angled: bool,
    pub tip_angle: String,
    /// Tap type: tapped (a cosmetic thread) instead of simple.
    pub tapped: bool,
    pub thread: String,
    /// Custom nominal diameter and pitch being edited, separate from the saved designation.
    pub custom_thread: Option<crate::dialogs_thread::HoleThreadDraft>,
    /// The Single hole's reference edge distances (`hole_refs`, #37).
    pub refs: Box<crate::hole_refs::Refs>,
    /// Use the full useful cylindrical wall, independently of the drill extents.
    pub thread_full_depth: bool,
    pub thread_depth: String,
    pub handedness: ThreadHandedness,
}

impl Default for HoleOpts {
    fn default() -> Self {
        HoleOpts {
            multiple: false,
            all: false,
            angled: true,
            tip_angle: "118 deg".into(),
            tapped: false,
            thread: "M5".into(),
            custom_thread: None,
            refs: Default::default(),
            thread_full_depth: true,
            thread_depth: "10 mm".into(),
            handedness: ThreadHandedness::Right,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Dialog {
    pub kind: Kind,
    /// Selection inputs, in order; clicks in the viewport go to the active one.
    pub inputs: Vec<SelInput>,
    pub active: usize,
    pub error: Option<String>,
    /// Editing an existing feature (its id, and the timeline marker to restore afterwards).
    pub editing: Option<(u64, Option<usize>)>,
    /// Hidden face-profile sources shown only for this editor's lifetime. Visibility itself is
    /// unchanged, so OK, Cancel, failed apply and replacing the dialog cannot leak an override.
    pub(crate) edit_source_bodies: Vec<String>,
    /// A user pick or clear makes the profile selector explicit, even if all are reselected.
    pub profile_selection_changed: bool,
    /// Give the on-canvas value box the keyboard (and select its text) on the next frame.
    pub focus: bool,
    /// Last focused value editor, shared by the form and its on-canvas value box.
    value_focus: Option<egui::Id>,
    /// A viewport press or click returns the keyboard after selection and canvas updates.
    pub(crate) restore_value_focus: bool,
    /// Folded to its header (the shared disclosure chevron).
    pub collapsed: bool,
    reset_scroll: bool,
    /// Transient display preference; never part of a command or a saved feature.
    pub preview_enabled: bool,
    /// Preserve Apply intent across asynchronous evaluation; never a command parameter.
    repeat_after_apply: bool,
    /// Parameters the dialog doesn't show but the command needs (kept when editing).
    pub extra: serde_json::Map<String, Value>,
    /// What the inputs were when OK was refused: `error` stays only while they still are.
    pub refused: Option<Refused>,
    /// Create Sketch opened by a sketch tool's shortcut: that tool starts once the sketch is open.
    pub then_start: Option<&'static str>,
    /// Edges picked by hand in a Fillet or Chamfer dialog, before tangent-chain expansion.
    pub(crate) picked_edges: Vec<Sel>,
    /// Whether `inputs[0]` currently includes the tangent chains of `picked_edges`.
    pub(crate) chain_applied: bool,
    /// A pattern's objects of the Object Type not shown now, restored when it is chosen again.
    pub(crate) other_objects: Vec<Sel>,
    /// Shell feedback is derived from the picks, never saved as additional face references.
    shell_chain: std::cell::RefCell<Option<ShellChain>>,
    /// An extrude Press Pull opened with nothing selected: its first pick may be a curved face or
    /// an edge, which reopens Press Pull on it (an offset or a fillet).
    pub press_pull: bool,
    /// The operation the window chooses by itself (#6901).
    pub op_auto: crate::op_auto::OpAuto,
    /// Where the face that reopened this fillet or chamfer was clicked (Press Pull): its arrow
    /// sits on the edge nearest it. UI state only, never saved.
    pub(crate) anchor_hint: Option<Vec3>,
}

#[derive(Clone, Debug)]
struct ShellChain {
    revision: u64,
    picks: Vec<Sel>,
    faces: Vec<Sel>,
}

/// Pin the pre-edit model so Cancel never depends on the shared cache retaining it.
pub(crate) struct EditingState {
    pub(crate) source: Session,
    pub(crate) original: Session,
}

struct DialogComputation {
    session: Session,
    cmds: Vec<(String, Value)>,
    marker: Option<Option<usize>>,
    editing: Option<EditingState>,
}

impl DialogComputation {
    fn new(app: &SolveApp, d: &Dialog) -> Result<Self, String> {
        if app.file_load.busy() || app.file_picker_busy() {
            return Err("a file operation is in progress".into());
        }
        let cmds = apply_commands(app, d)?;
        let mut session = app.session.scratch();
        // The window's operation check already evaluated this Join (#6901).
        if let Some(joined) = app.op_check.joined_for(&cmds, app.session.revision) {
            session.prime(&joined);
        }
        Ok(Self {
            session,
            cmds,
            marker: d.editing.map(|(_, marker)| marker),
            // The editor's pinned original is for a dialog that previews (as `open_feature_edit`
            // keeps it); one that no longer does (Modeled unticked) edits the design as it is.
            editing: d
                .editing
                .filter(|_| d.previews())
                .and(app.editing_state.as_ref())
                .map(|state| EditingState { source: state.source.scratch(), original: state.original.scratch() }),
        })
    }

    fn compute(self) -> Result<solidcraft_engine::ComputedCommands, String> {
        match self.editing {
            Some(state) => self.session.compute_editor_commands(&self.cmds, &state.source, state.original),
            None => self.session.compute_commands(&self.cmds, self.marker),
        }
        .map_err(|e| e.to_string())
    }
}

fn adopt_dialog(app: &mut SolveApp, prepared: solidcraft_engine::ComputedCommands) -> Result<(), String> {
    if app.file_load.busy() || app.file_picker_busy() {
        return Err("a file operation is in progress".into());
    }
    let replies = app.session.accept_commands(prepared).map_err(|e| e.to_string())?;
    app.big_operation = true;
    for (id, reply) in replies {
        let stop = (id == "joint.drive").then(|| crate::dialogs_assembly::drive_stop(&reply)).flatten();
        app.command_completed(&id, Ok(reply))?;
        if let Some(why) = stop {
            app.set_status(why, false);
        }
    }
    Ok(())
}

/// A complete feature transaction, evaluated without changing the displayed session.
pub struct Applying {
    pub(crate) done: std::sync::mpsc::Receiver<Result<solidcraft_engine::ComputedCommands, String>>,
    pub(crate) ready: Option<Result<solidcraft_engine::ComputedCommands, String>>,
    pub(crate) seen: bool,
    /// When OK was pressed (ms).
    pub(crate) started: f64,
    pub(crate) cmds: Vec<(String, Value)>,
    pub(crate) progress: std::sync::Arc<solidcraft_engine::doc::progress::Progress>,
    pub(crate) quality: solidcraft_engine::doc::display_quality::DisplayQuality,
}

impl Applying {
    fn finished(&mut self) -> Option<Result<solidcraft_engine::ComputedCommands, String>> {
        if !self.seen {
            self.seen = true;
            return None;
        }
        self.ready.take().or_else(|| match self.done.try_recv() {
            Ok(result) => Some(result),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Err("feature computation ended without a result".into())),
        })
    }
}

impl Drop for Applying {
    fn drop(&mut self) {
        self.progress.cancel();
    }
}

/// How long OK waits before displaying its computing state.
pub const APPLY_AT_ONCE: std::time::Duration = std::time::Duration::from_millis(150);

fn display_quality(app: &SolveApp) -> solidcraft_engine::doc::display_quality::DisplayQuality {
    solidcraft_engine::doc::display_quality::DisplayQuality::parse(&app.preferences.display_quality).unwrap_or_default()
}

/// Desktop feature dialogs evaluate the entire command and its dependent suffix once; so does
/// any feature edit, with a live preview or without. Results wait until a later frame before
/// adoption, keeping the computing state visible.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn begin_applying(app: &SolveApp, d: &Dialog) -> Option<Applying> {
    let wait = app.apply_off_thread?;
    if !d.previews() && d.editing.is_none() {
        return None;
    }
    let computation = DialogComputation::new(app, d).ok()?;
    let cmds = computation.cmds.clone();
    #[cfg(test)]
    let gate = app.apply_gate.clone();
    Some(start_applying(cmds, display_quality(app), crate::now_ms(), wait, move || {
        let result = computation.compute();
        #[cfg(test)]
        if let Some(gate) = gate {
            let _ = gate.lock().unwrap().recv();
        }
        result
    }))
}

#[cfg(not(target_arch = "wasm32"))]
fn start_applying(
    cmds: Vec<(String, Value)>,
    quality: solidcraft_engine::doc::display_quality::DisplayQuality,
    started: f64,
    wait: std::time::Duration,
    compute: impl FnOnce() -> Result<solidcraft_engine::ComputedCommands, String> + Send + 'static,
) -> Applying {
    let progress = std::sync::Arc::new(solidcraft_engine::doc::progress::Progress::default());
    let watching = progress.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = std::thread::Builder::new().name("apply".into()).spawn(move || {
        let result = solidcraft_engine::doc::progress::watch(watching.clone(), || {
            let prepared = compute()?;
            if watching.is_cancelled() {
                return Err("feature computation cancelled".into());
            }
            prepared.warm_display(quality, || !watching.is_cancelled());
            if watching.is_cancelled() {
                return Err("feature computation cancelled".into());
            }
            Ok(prepared)
        });
        let _ = tx.send(result);
    });
    let ready = match worker {
        Err(error) => Some(Err(format!("could not start feature computation: {error}"))),
        Ok(_) => match rx.recv_timeout(wait) {
            Ok(result) => Some(result),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Some(Err("feature computation ended without a result".into())),
        },
    };
    Applying { done: rx, ready, seen: false, started, cmds, progress, quality }
}

#[cfg(not(target_arch = "wasm32"))]
fn warm_applying(app: &SolveApp, prepared: solidcraft_engine::ComputedCommands, cmds: Vec<(String, Value)>, started: f64) -> Option<Applying> {
    Some(start_applying(cmds, display_quality(app), started, std::time::Duration::ZERO, move || Ok(prepared)))
}

#[cfg(target_arch = "wasm32")]
fn warm_applying(_app: &SolveApp, _prepared: solidcraft_engine::ComputedCommands, _cmds: Vec<(String, Value)>, _started: f64) -> Option<Applying> {
    None
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn begin_applying(_app: &SolveApp, _d: &Dialog) -> Option<Applying> {
    None
}

/// The commands a dialog's inputs make (or why they make none), its picks, and the design's
/// revision (a parameter the commands name can change without changing them).
#[derive(Clone, Debug, PartialEq)]
pub struct Refused {
    commands: Result<Vec<(String, Value)>, String>,
    picks: Vec<SelInput>,
    revision: u64,
}

impl Refused {
    fn now(app: &SolveApp, d: &Dialog) -> Refused {
        Refused { commands: apply_commands(app, d), picks: d.inputs.clone(), revision: app.session.revision }
    }
}

/// Can a selection go into an input that accepts `a`? (Planar-ness is checked when picking.)
fn fits(a: Accept, s: &Sel) -> bool {
    match s {
        Sel::Profile { .. } => a & PROFILES != 0,
        Sel::Edge { .. } => a & EDGES != 0,
        Sel::Face { .. } => a & (FACES | PLANAR_FACES) != 0,
        Sel::Body { .. } => a & BODIES != 0,
        Sel::Plane { .. } => a & PLANES != 0,
        Sel::Axis { .. } => a & AXES != 0,
        Sel::ConstructionPoint { .. } => a & selection::CONSTRUCTION_POINTS != 0,
        Sel::SketchCurve { .. } => a & (AXES | CURVES | selection::SKETCH_LINES) != 0,
        Sel::ExternalSketchCurve { .. } => a & selection::SKETCH_LINES != 0,
        Sel::Vertex { .. } => a & selection::VERTICES != 0,
        Sel::Feature { .. } => a & selection::FEATURES != 0,
        // Sketch points go into a sketch command's own inputs (Entities, a base point), so a
        // box selection and a pre-selection reach them the way a click already does.
        Sel::SketchPoint { .. } => a & (selection::POINTS | selection::SKETCH_POINTS) != 0,
        Sel::ExternalSketchPoint { .. } => a & selection::EXTERNAL_POINTS != 0,
        Sel::SketchMidpoint { .. } | Sel::SketchConstraint { .. } | Sel::Occurrence { .. } => false,
    }
}

impl Dialog {
    /// The command this dialog runs, whose icon and label its header shows.
    pub(crate) fn command(&self) -> Option<&'static str> {
        dense::command(&self.kind)
    }

    /// Show why OK (or starting the sketch) was refused, for as long as the inputs stay as they are.
    fn refuse(&mut self, app: &SolveApp, error: String) {
        self.refused = Some(Refused::now(app, self));
        self.error = Some(error);
    }

    pub(crate) fn new(kind: Kind, inputs: Vec<SelInput>) -> Dialog {
        let chain_applied = matches!(kind, Kind::Fillet { chain: true, .. });
        Dialog {
            kind,
            inputs,
            active: 0,
            error: None,
            editing: None,
            edit_source_bodies: Vec::new(),
            profile_selection_changed: false,
            focus: true,
            value_focus: None,
            restore_value_focus: false,
            collapsed: false,
            reset_scroll: true,
            preview_enabled: true,
            repeat_after_apply: false,
            extra: serde_json::Map::new(),
            refused: None,
            then_start: None,
            picked_edges: Vec::new(),
            chain_applied,
            other_objects: Vec::new(),
            shell_chain: Default::default(),
            press_pull: false,
            op_auto: Default::default(),
            anchor_hint: None,
        }
    }

    /// The picker for the agent chat's context, holding what is already picked.
    pub fn chat_context(picked: &[Sel]) -> Dialog {
        let mut inputs = vec![
            SelInput::new("Elements", FACES | EDGES | selection::VERTICES, true),
            SelInput::new("Features", selection::FEATURES, true),
            SelInput::new("Bodies", BODIES, true),
        ];
        for sel in picked {
            if let Some(input) = inputs.iter_mut().find(|i| fits(i.accept, sel)) {
                input.items.push(sel.clone());
            }
        }
        Dialog::new(Kind::ChatContext, inputs)
    }

    /// A component picked in the browser is not a body selection. A command that takes bodies,
    /// Move/Copy included, still starts with that component's bodies.
    fn take_component_bodies(app: &SolveApp, d: &mut Dialog) {
        if app.tree.picked_components.is_empty() {
            return;
        }
        let mut names = Vec::new();
        for id in &app.tree.picked_components {
            for name in crate::browser::bodies_of_component(app, *id) {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        let Some(inp) = d.inputs.iter_mut().find(|i| i.accept == BODIES && (i.multi || i.items.is_empty())) else {
            return;
        };
        for name in names {
            let body = Sel::Body { name };
            if !inp.multi {
                if inp.items.is_empty() {
                    inp.items.push(body);
                }
                break;
            }
            if !inp.items.contains(&body) {
                inp.items.push(body);
            }
        }
    }

    pub fn for_command(app: &SolveApp, id: &str) -> Option<Dialog> {
        let s = &app.session;
        let has_bodies = !s.world_state().bodies.is_empty();
        let mut d = match id {
            "sketch.create" => Dialog::new(Kind::Sketch, vec![SelInput::new("Plane", PLANES | PLANAR_FACES, false)]),
            "sketch.intersect" => {
                let bodies = s.selection.iter().any(|sel| matches!(sel, Sel::Body { .. }));
                Dialog::new(
                    Kind::SketchIntersect { bodies, projection_link: true },
                    vec![SelInput::new("ui.sketch_intersect_geometry", if bodies { BODIES } else { INTERSECT_GEOMETRY }, true)],
                )
            }
            "sketch.project" => {
                let bodies = s.selection.iter().any(|sel| matches!(sel, Sel::Body { .. }));
                Dialog::new(
                    Kind::SketchProject { bodies, projection_link: true },
                    vec![SelInput::new("ui.sketch_project_geometry", if bodies { BODIES } else { PROJECT_GEOMETRY }, true)],
                )
            }
            "solid.extrude" => Dialog::new(
                Kind::Extrude {
                    distance: length_default(s, "10"),
                    direction: 0,
                    operation: usize::from(has_bodies),
                    auto_op: true,
                    smart: crate::smart_extrude::Smart { auto_to_object: app.preferences.extrude_auto_to_object, ..Default::default() },
                    distance2: length_default(s, "10"),
                    taper: "0 deg".into(),
                    taper_present: false,
                    to_offset: String::new(),
                    start: String::new(),
                    all: false,
                    thin: false,
                    wall: length_default(s, "2"),
                    wall_side: 0,
                    wall2: length_default(s, "1"),
                },
                vec![SelInput::new("Profiles", EXTRUDE_ACCEPT, true)],
            ),
            "solid.revolve" => Dialog::new(
                Kind::Revolve { angle: "360 deg".into(), operation: 0, axis: None },
                vec![SelInput::new("Profile", PROFILES | PLANAR_FACES, true), SelInput::new("Axis", AXIS_PICKS, false)],
            ),
            "solid.fillet" => Dialog::new(fillet_kind(&length_default(s, "2"), false), vec![SelInput::new("Edges", EDGES | FACES, true)]),
            "solid.chamfer" => Dialog::new(fillet_kind(&length_default(s, "1"), true), vec![SelInput::new("Edges", EDGES | FACES, true)]),
            "solid.shell" => {
                let (kind, inputs) = Kind::simple_start(id)?;
                let mut d = Dialog::new(crate::length_input::defaults(&s.doc, kind), inputs);
                // Faces joined smoothly to the picked ones too.
                d.extra.insert("tangent_chain".into(), json!(true));
                d
            }
            "solid.mirror" => {
                Dialog::new(Kind::Mirror, vec![SelInput::new("Objects", BODIES, true), SelInput::new("Mirror Plane", PLANES | PLANAR_FACES, false)])
            }
            "solid.rib" | "solid.web" => Dialog::new(
                Kind::Rib { web: id == "solid.web", thickness: length_default(s, "2"), depth: String::new(), flip: false, normal: false },
                vec![SelInput::new("Curves", CURVES, true)],
            ),
            "solid.replace_face" => Dialog::new(
                Kind::ReplaceFace,
                vec![SelInput::new("Faces", PLANAR_FACES, true), SelInput::new("Target", PLANES | PLANAR_FACES, false)],
            ),
            "face.delete" => Dialog::new(Kind::DeleteFaces, vec![SelInput::new("Faces", FACES, true)]),
            "solid.boundary_fill" => {
                Dialog::new(Kind::BoundaryFill { operation: 0, cells: Vec::new(), of: Vec::new() }, vec![SelInput::new("Tools", BODIES, true)])
            }
            "solid.align" => {
                let components = s.selection.iter().any(|x| matches!(x, Sel::Occurrence { .. }));
                Dialog::new(
                    Kind::Align { flip: false, components, axes: [true; 3] },
                    vec![
                        SelInput::new(if components { "Components" } else { "Bodies" }, BODIES, true),
                        SelInput::new("From", FACES | EDGES | selection::VERTICES | PLANES | selection::ORIGIN, false),
                        SelInput::new("To", FACES | EDGES | selection::VERTICES | PLANES | selection::ORIGIN, false),
                    ],
                )
            }
            "solid.remove" => Dialog::new(Kind::Remove, vec![SelInput::new("Bodies", BODIES, true)]),
            "solid.pattern.path" => Dialog::new(
                Kind::PathPattern { count: "4".into(), spacing: length_default(s, "20") },
                vec![SelInput::new("Objects", BODIES, true), SelInput::new("Path", CURVES, true)],
            ),
            "mesh.convert" => Dialog::new(
                Kind::MeshConvert { tolerance: crate::length_input::model(&s.doc, 0.05) },
                vec![SelInput::new("Mesh body", BODIES, false)],
            ),
            "construct.plane.midplane" => Dialog::new(
                Kind::Midplane,
                vec![SelInput::new("First plane", PLANES | PLANAR_FACES, false), SelInput::new("Second plane", PLANES | PLANAR_FACES, false)],
            ),
            id if construct_picks(id).is_some() => {
                let (command, title, picks) = construct_picks(id)?;
                let inputs = picks.iter().map(|(label, accept, _)| SelInput::new(label, *accept, false)).collect();
                let keys = picks.iter().map(|(_, _, key)| *key).collect();
                let t = (command == "construct.point.along_path").then(|| "0.5".to_string());
                Dialog::new(Kind::Construct { command, title, keys, t }, inputs)
            }
            "construct.plane.along_path" => {
                Dialog::new(Kind::PathPlane { t: "0.5".into(), sketch: None }, vec![SelInput::new("Path", CURVES | EDGES, false)])
            }
            "solid.split_body" => Dialog::new(
                Kind::Split,
                vec![SelInput::new("Body to split", BODIES, false), SelInput::new("Splitting tool", PLANES | FACES | AXES, false)],
            ),
            "solid.thread" => Dialog::new(
                Kind::Thread {
                    designation: String::new(),
                    length: String::new(),
                    handedness: ThreadHandedness::Right,
                    suggested: None,
                    of: Vec::new(),
                },
                vec![SelInput::new("Cylindrical face", FACES, false)],
            ),
            "material.assign" => {
                Dialog::new(Kind::Material { index: usize::MAX, editor: Default::default() }, vec![SelInput::new("Bodies", BODIES, true)])
            }
            "inspect.section" => Dialog::new(
                Kind::Section { offset: length_default(s, "0"), flip: false, hatch: true },
                vec![SelInput::new("Plane", PLANES | PLANAR_FACES, false)],
            ),
            "inspect.measure" => Dialog::new(
                Kind::Measure { result: None, of: Vec::new(), show: None, labels: Default::default() },
                vec![SelInput::new(
                    "Items",
                    FACES
                        | EDGES
                        | selection::VERTICES
                        | CURVES
                        | selection::POINTS
                        | PROFILES
                        | selection::SKETCH_LINES
                        | selection::EXTERNAL_POINTS
                        | PLANES
                        | AXES
                        | selection::CONSTRUCTION_POINTS
                        | selection::ORIGIN,
                    true,
                )],
            ),
            "solid.pattern.rectangular" => Dialog::new(
                Kind::PatternRect { count: "3".into(), spacing: length_default(s, "20"), count2: "1".into(), spacing2: length_default(s, "20") },
                vec![
                    SelInput::new("Objects", BODIES, true),
                    SelInput::new("Direction", AXES | EDGES, false),
                    SelInput::new("Direction 2", AXES | EDGES, false),
                ],
            ),
            "solid.pattern.circular" => Dialog::new(
                Kind::PatternCirc { count: "6".into(), angle: "360 deg".into() },
                vec![SelInput::new("Objects", BODIES, true), SelInput::new("Axis", AXIS_PICKS, false)],
            ),
            "solid.loft" => Dialog::new(Kind::Loft { operation: usize::from(has_bodies), rail_fit: 0 }, {
                let [rail, rail2] = crate::dialogs_sweep::rail_inputs();
                vec![SelInput::new("Profiles", PROFILES | selection::POINTS, true), rail, rail2]
            }),
            "solid.sweep" => Dialog::new(Kind::Sweep { operation: usize::from(has_bodies), opts: crate::dialogs_sweep::SweepOpts::default() }, {
                let [rail, rail2] = crate::dialogs_sweep::rail_inputs();
                vec![
                    SelInput::new("Profile", PROFILES, true),
                    SelInput::new("Path", CURVES, true),
                    crate::dialogs_sweep::direction_input(),
                    rail,
                    rail2,
                ]
            }),
            "solid.move" => Dialog::new(crate::dialogs_move::kind(0, &s.doc), crate::dialogs_move::inputs(0)),
            "solid.hole" => Dialog::new(hole_defaults(s), single_hole_inputs()),
            "solid.box" => Dialog::new(
                Kind::Primitive {
                    cmd: "solid.box",
                    fields: vec![("length", length_default(s, "20")), ("width", length_default(s, "20")), ("height", length_default(s, "20"))],
                    operation: 0,
                    at: None,
                    refs: Default::default(),
                },
                crate::primitive_placement::inputs("solid.box"),
            ),
            "solid.cylinder" => Dialog::new(
                Kind::Primitive {
                    cmd: "solid.cylinder",
                    fields: vec![("diameter", length_default(s, "20")), ("height", length_default(s, "20"))],
                    operation: 0,
                    at: None,
                    refs: Default::default(),
                },
                crate::hole_refs::cylinder_inputs(),
            ),
            "solid.sphere" => Dialog::new(
                Kind::Primitive {
                    cmd: "solid.sphere",
                    fields: vec![("diameter", length_default(s, "20"))],
                    operation: 0,
                    at: None,
                    refs: Default::default(),
                },
                crate::primitive_placement::inputs("solid.sphere"),
            ),
            "solid.torus" => Dialog::new(
                Kind::Primitive {
                    cmd: "solid.torus",
                    fields: vec![("major", length_default(s, "20")), ("minor", length_default(s, "5"))],
                    operation: 0,
                    at: None,
                    refs: Default::default(),
                },
                crate::primitive_placement::inputs("solid.torus"),
            ),
            "solid.cone" => Dialog::new(
                Kind::Primitive {
                    cmd: "solid.cone",
                    fields: vec![
                        ("diameter", length_default(s, "20")),
                        ("top_diameter", length_default(s, "0")),
                        ("height", length_default(s, "20")),
                    ],
                    operation: 0,
                    at: None,
                    refs: Default::default(),
                },
                vec![],
            ),
            "solid.coil" => Dialog::new(
                Kind::Primitive {
                    cmd: "solid.coil",
                    fields: vec![
                        ("diameter", length_default(s, "40")),
                        ("revolutions", "4".into()),
                        ("pitch", length_default(s, "8")),
                        ("section_size", length_default(s, "3")),
                    ],
                    operation: 0,
                    at: None,
                    refs: Default::default(),
                },
                vec![SelInput::new("Profile", PROFILES, false)],
            ),
            "solid.combine" => Dialog::new(
                Kind::Combine { operation: 1, keep_tools: false },
                vec![SelInput::new("Target Body", BODIES, false), SelInput::new("Tool Bodies", BODIES, true)],
            ),
            _ => {
                let (kind, inputs) = Kind::simple_start(id)
                    .map(|(kind, inputs)| (crate::length_input::defaults(&s.doc, kind), inputs))
                    .or_else(|| crate::dialogs_assembly::start(app, id))
                    .or_else(|| crate::dialogs_sheet::start(app, id))
                    .or_else(|| crate::dialogs_surface::start(app, id))
                    .or_else(|| crate::dialogs_save_mesh::start(app, id))
                    .or_else(|| crate::dialogs_view_dxf::start(app, id))
                    .or_else(|| crate::dialogs_plastic::start(app, id))
                    .or_else(|| crate::dialogs_frame::start(app, id))
                    .or_else(|| crate::dialogs_appearance::start(app, id))
                    .or_else(|| crate::dialogs_motion::start(app, id))
                    .or_else(|| crate::dialogs_parts::start(app, id))
                    .or_else(|| crate::dialogs_part_size::start(app, id))
                    .or_else(|| crate::sketch_text::start(app, id))
                    .or_else(|| crate::sketch_text_move::start(app, id))
                    .or_else(|| crate::sketch_modify_ui::start(app, id))?;
                Dialog::new(kind, inputs)
            }
        };
        // Patterns of features picked in the timeline: the objects are features.
        if is_pattern(&d.kind)
            && s.selection.iter().any(|x| matches!(x, Sel::Feature { .. }))
            && let Some(inp) = d.inputs.first_mut()
        {
            inp.accept = selection::FEATURES;
            inp.label = objects_label(inp.accept);
        }
        // Pre-selection: what is selected now becomes the input (first input that takes it); a
        // face or edge stands for its body where bodies are wanted. (Edit Text's selection is the
        // text it edits, not its path.)
        let preselection: &[Sel] = if matches!(d.kind, Kind::Text(_)) { &[] } else { &s.selection };
        for sel in preselection {
            // A sketch line drawn over a body edge is what a click there picks; a command that
            // takes edges and no sketch curves takes it as that edge (#8927).
            let as_edge = (!d.inputs.iter().any(|i| fits(i.accept, sel)) && d.inputs.iter().any(|i| i.accept & EDGES != 0))
                .then(|| sketch_curve_edge(s, sel))
                .flatten();
            let sel = as_edge.as_ref().unwrap_or(sel);
            if matches!(d.kind, Kind::Align { components: true, .. }) {
                let occ = match sel {
                    Sel::Occurrence { path } if !path.is_empty() => Some(Sel::Occurrence { path: path.clone() }),
                    Sel::Body { name } => occurrence_path_of_body(s, name).map(|path| Sel::Occurrence { path }),
                    Sel::Face { body, .. } | Sel::Edge { body, .. } | Sel::Vertex { body, .. } => {
                        occurrence_path_of_body(s, body).map(|path| Sel::Occurrence { path })
                    }
                    _ => None,
                };
                if let Some(occ) = occ {
                    if let Some(inp) = d.inputs.first_mut()
                        && !inp.items.contains(&occ)
                    {
                        inp.items.push(occ);
                    }
                    continue;
                }
            }
            let as_body = match sel {
                Sel::Face { body, .. } | Sel::Edge { body, .. } | Sel::Vertex { body, .. } => Some(Sel::Body { name: body.to_string() }),
                _ => None,
            };
            if let Some(b) = as_body
                && let Some(inp) = d.inputs.iter_mut().find(|i| i.accept == BODIES && (i.multi || i.items.is_empty()))
            {
                if !inp.items.contains(&b) {
                    inp.items.push(b);
                }
                continue;
            }
            // A selected component is its bodies when the command takes bodies. One selected
            // placement still starts component Move before this dialog is built.
            if let Sel::Occurrence { path } = sel
                && let Some(inp) = d.inputs.iter_mut().find(|i| i.accept == BODIES && (i.multi || i.items.is_empty()))
            {
                if let Ok(names) = s.occurrence_bodies(path, true) {
                    for name in names {
                        let body = Sel::Body { name };
                        if !inp.multi {
                            if inp.items.is_empty() {
                                inp.items.push(body);
                            }
                            break;
                        }
                        if !inp.items.contains(&body) {
                            inp.items.push(body);
                        }
                    }
                }
                continue;
            }
            // Planar-only inputs take planar faces only.
            let planar_ok = match sel {
                Sel::Face { body, index, .. } => planar_face(s, body, *index).is_some(),
                _ => true,
            };
            let takes = |i: &SelInput| {
                fits(i.accept, sel)
                    && (i.multi || i.items.is_empty())
                    && (planar_ok || i.accept & FACES != 0 || !matches!(sel, Sel::Face { .. }))
                    && (i.accept != AXIS_PICKS || takes_as_axis(s, sel))
            };
            if let Some(inp) = d.inputs.iter_mut().find(|i| takes(i)) {
                if matches!(d.kind, Kind::Fillet { .. }) {
                    for x in crate::viewport::grouped_selection(s, sel) {
                        if !d.picked_edges.iter().any(|y| selection::same_item(y, &x)) {
                            d.picked_edges.push(x);
                        }
                    }
                }
                // Fillets and chamfers take an edge with its tangent chain, as a pick does.
                let picked = match (&d.kind, sel) {
                    (Kind::Fillet { chain, .. }, _) => fillet_edges(s, sel, *chain),
                    (Kind::Measure { .. }, _) => vec![crate::viewport::measurement_selection(s, sel)],
                    _ if inp.multi => crate::viewport::grouped_selection(s, sel),
                    _ => vec![sel.clone()],
                };
                for x in picked {
                    if !inp.items.iter().any(|y| selection::same_item(y, &x)) {
                        inp.items.push(x);
                    }
                }
            }
        }
        Self::take_component_bodies(app, &mut d);
        // A primitive goes where a face or vertex is picked, standing on a planar face.
        if let Kind::Primitive { at, .. } = &mut d.kind {
            *at = s.selection.iter().find_map(|sel| match sel {
                Sel::Face { body, index, point } => Some((*point, planar_face(s, body, *index).map(|(_, n)| n))),
                Sel::Vertex { point, .. } => Some((*point, None)),
                _ => None,
            });
        }
        // With nothing pre-selected, a single profile in the sketch being used is taken (#684:
        // not one a feature already builds from).
        if matches!(d.kind, Kind::Extrude { .. } | Kind::Revolve { .. })
            && let Some(inp) = d.inputs.first_mut()
            && inp.items.is_empty()
        {
            inp.items = default_profiles(s);
        }
        d.sync_thread(s);
        d.sync_material(s);
        d.advance();
        // Preselected items are already the dialog's inputs: the control reply and the first
        // viewport paint must see the same measurement, without waiting for the panel to draw.
        update_measure(app, &mut d);
        Some(d)
    }

    fn sync_material(&mut self, s: &Session) {
        let Kind::Material { index, editor } = &mut self.kind else { return };
        let bodies = self.inputs.first().map(|input| input.items.body_names()).unwrap_or_default();
        editor.sync_selection(&s.doc, &bodies, index);
    }

    /// Resolve a newly selected face without replacing an authored size or changing the
    /// document. Clearing the face also clears a suggestion that no longer applies.
    fn sync_thread(&mut self, s: &Session) {
        let Kind::Thread { designation, suggested, of, .. } = &mut self.kind else { return };
        if !designation.trim().is_empty() && suggested.as_deref() != Some(designation.as_str()) {
            return;
        }
        let picks = self.inputs.first().map(|i| i.items.as_slice()).unwrap_or_default();
        if picks == of.as_slice() {
            return;
        }
        *of = picks.to_vec();
        *suggested = picks.first().and_then(|sel| match sel {
            Sel::Face { point, .. } => solidcraft_engine::doc::suggested_thread(&s.world_state(), *point).ok().map(|value| {
                solidcraft_engine::doc::parse_metric_thread(&value).map(|(d, p)| crate::dialogs_thread::designation(d, p)).unwrap_or(value)
            }),
            _ => None,
        });
        *designation = suggested.clone().unwrap_or_default();
    }

    pub fn rename(feature: u64, name: &str) -> Dialog {
        Dialog::new(Kind::Rename { feature, name: name.to_string() }, vec![])
    }

    #[cfg(test)]
    pub fn confirm_delete(feature: u64, with: Vec<String>, fail: Vec<String>) -> Dialog {
        Dialog::confirm_delete_many(vec![feature], Vec::new(), with, fail)
    }

    pub fn confirm_delete_many(features: Vec<u64>, groups: Vec<u64>, with: Vec<String>, fail: Vec<String>) -> Dialog {
        Dialog::new(Kind::ConfirmDelete { features, groups, with, fail }, vec![])
    }

    pub fn edit_param(s: &Session, name: &str) -> Dialog {
        Dialog::new(Kind::EditParam { name: name.into(), expr: s.doc.param(name).map(|p| p.expr.clone()).unwrap_or_default() }, vec![])
    }

    /// An Extrude or Revolve started from inside a sketch takes that sketch's profile, as it would
    /// had the sketch stayed open.
    pub(crate) fn take_sketch_profile(&mut self, s: &Session, sketch: u64) {
        if matches!(self.kind, Kind::Extrude { .. } | Kind::Revolve { .. })
            && let Some(inp) = self.inputs.first_mut()
            && inp.items.is_empty()
        {
            inp.items = single_profile(s, sketch);
            self.advance();
        }
    }

    /// Make the first single input still empty (after the filled ones) active.
    fn advance(&mut self) {
        if let Some(i) = self.inputs.iter().position(|i| i.items.is_empty()) {
            self.active = i;
        } else {
            self.active = self.active.min(self.inputs.len().saturating_sub(1));
        }
    }

    /// Does this dialog make a feature that can be previewed live?
    pub fn previews(&self) -> bool {
        match &self.kind {
            Kind::Assembly(k) => return k.previews(),
            Kind::Sheet(k) => return k.previews(),
            Kind::Surface(_) => return true,
            Kind::SketchModify(k) => return k.previews(),
            Kind::Plastic(k) => return k.previews(),
            Kind::Frame(_) => return true,
            Kind::Part(_) | Kind::PartSize(_) | Kind::InsertDrawing(_) => return true,
            // Web previews run synchronously; a modelled thread can take seconds.
            Kind::Thread { .. } => return !cfg!(target_arch = "wasm32") && self.extra.get("modeled").and_then(Value::as_bool).unwrap_or(false),
            _ => {}
        }
        matches!(
            self.kind,
            Kind::Extrude { .. }
                | Kind::Revolve { .. }
                | Kind::Fillet { .. }
                | Kind::Shell { .. }
                | Kind::Draft { .. }
                | Kind::Mirror
                | Kind::Split
                | Kind::Rib { .. }
                | Kind::Emboss { .. }
                | Kind::ReplaceFace
                | Kind::DeleteFaces
                | Kind::Align { .. }
                | Kind::Remove
                | Kind::PathPattern { .. }
                | Kind::Pipe { .. }
                | Kind::Stock { .. }
                | Kind::Scale { .. }
                | Kind::OffsetFaces { .. }
                | Kind::PatternRect { .. }
                | Kind::PatternCirc { .. }
                | Kind::Loft { .. }
                | Kind::Sweep { .. }
                | Kind::AnglePlane { .. }
                | Kind::PathPlane { .. }
                | Kind::Midplane
                | Kind::Construct { .. }
                | Kind::Move { .. }
                | Kind::Hole { .. }
                | Kind::Primitive { .. }
                | Kind::Combine { .. }
                | Kind::BoundaryFill { .. }
        )
    }

    /// The main value (shown on the canvas too): label, kind and the expression.
    pub fn primary(&mut self) -> Option<(&'static str, ValueKind, &mut String)> {
        let move_field = crate::dialogs_move::value_field(self);
        let through_point = offset_through_point(self).is_some();
        Some(match &mut self.kind {
            Kind::Extrude { to_offset, .. } if to_object_on(&self.inputs) => ("To Offset", ValueKind::Length, to_offset),
            Kind::Extrude { distance, .. } => ("Distance", ValueKind::Length, distance),
            Kind::Revolve { angle, .. } => ("Angle", ValueKind::Angle, angle),
            Kind::Fillet { radius, chamfer, .. } => {
                let label = if *chamfer {
                    "Distance"
                } else if matches!(self.extra.get("type").and_then(Value::as_str), Some("curvature" | "g2")) {
                    "Equivalent Radius"
                } else {
                    "Radius"
                };
                (label, ValueKind::Length, radius)
            }
            Kind::Shell { thickness } => ("Thickness", ValueKind::Length, thickness),
            Kind::Draft { angle } => ("Angle", ValueKind::Angle, angle),
            Kind::Hole { diameter, opts, .. } => {
                if opts.tapped {
                    return None;
                }
                ("Diameter", ValueKind::Length, diameter)
            }
            // A move's main value follows its type: a distance for the ones that translate
            // (along the axis dragged or tabbed to last, X at first; #4776), the angle for
            // Rotate. Point to Point is two picks with nothing to type.
            Kind::Move { mode, x, y, z, rx, ry, rz, angle } => match *mode {
                crate::dialogs_move::ROTATE => ("Angle", ValueKind::Angle, angle),
                crate::dialogs_move::POINT_TO_POINT => return None,
                _ => match move_field {
                    1 => ("Y", ValueKind::Length, y),
                    2 => ("Z", ValueKind::Length, z),
                    3 => ("X Angle", ValueKind::Angle, rx),
                    4 => ("Y Angle", ValueKind::Angle, ry),
                    5 => ("Z Angle", ValueKind::Angle, rz),
                    _ => ("X", ValueKind::Length, x),
                },
            },
            Kind::PatternRect { spacing, .. } => ("Spacing", ValueKind::Length, spacing),
            Kind::PatternCirc { angle, .. } => ("Angle", ValueKind::Angle, angle),
            Kind::Section { offset, .. } => ("Distance", ValueKind::Length, offset),
            Kind::OffsetPlane { .. } if through_point => return None,
            Kind::OffsetPlane { offset } => ("Distance", ValueKind::Length, offset),
            Kind::PathPlane { t, .. } => ("Position (0 to 1)", ValueKind::Unitless, t),
            Kind::Rib { thickness, .. } => ("Thickness", ValueKind::Length, thickness),
            Kind::Emboss { depth, .. } => ("Depth", ValueKind::Length, depth),
            Kind::PathPattern { spacing, .. } => {
                (if self.extra.contains_key("distance") { "Distance" } else { "Spacing" }, ValueKind::Length, spacing)
            }
            Kind::Pipe { diameter, .. } => ("Diameter", ValueKind::Length, diameter),
            Kind::Stock { margin } => ("Margin", ValueKind::Length, margin),
            Kind::AnglePlane { angle } => ("Angle", ValueKind::Angle, angle),
            Kind::Scale { factor } => ("Scale", ValueKind::Unitless, factor),
            Kind::OffsetFaces { distance } => ("Distance", ValueKind::Length, distance),
            Kind::Sheet(k) => return k.primary(&self.inputs),
            Kind::Surface(k) => return k.primary(),
            Kind::Plastic(k) => return k.primary(),
            Kind::SketchModify(k) => return k.primary(),
            _ => return None,
        })
    }

    pub fn wants_picks(&self) -> bool {
        !self.inputs.is_empty()
    }

    pub fn active_input(&self) -> Option<&SelInput> {
        self.inputs.get(self.active)
    }

    /// Whether a round edge kept in `inp` stands for its centre: a construction point's input,
    /// or Move's pivot and points (`dialogs_move::sel_point`). Other point inputs take the
    /// centre as a vertex (`circle_centre_pick`).
    pub(crate) fn edge_is_centre(&self, inp: &SelInput) -> bool {
        inp.accept != selection::AXIS_PICKS
            && (matches!(self.kind, Kind::Move { .. }) || (matches!(self.kind, Kind::Construct { .. }) && inp.accept & selection::VERTICES != 0))
    }

    /// Does the active input pick component occurrences rather than bodies?
    pub(crate) fn takes_components(&self) -> bool {
        matches!(self.kind, Kind::Align { components: true, .. }) && self.active == 0
    }

    /// Everything the inputs hold (for highlighting).
    pub fn items(&self) -> Vec<Sel> {
        self.inputs.iter().flat_map(|i| i.items.iter().cloned()).collect()
    }

    /// Faces Shell will open, including the tangent chain. Keep the input itself as the
    /// user's picks so switching the option off, clearing, and editing all remain reversible.
    fn shell_faces(&self, s: &Session) -> Option<Vec<Sel>> {
        if !matches!(self.kind, Kind::Shell { .. }) || self.extra.get("tangent_chain").and_then(Value::as_bool) != Some(true) {
            return None;
        }
        let picks = sels(self, 0);
        if let Ok(cache) = self.shell_chain.try_borrow()
            && let Some(chain) = cache.as_ref()
            && chain.revision == s.revision
            && chain.picks == picks
        {
            return Some(chain.faces.clone());
        }
        let state = s.world_state();
        let mut by_body = std::collections::BTreeMap::<&str, Vec<Vec3>>::new();
        for pick in picks {
            if let Sel::Face { body, point, .. } = pick {
                by_body.entry(body).or_default().push(*point);
            }
        }
        let mut faces = Vec::new();
        for (name, points) in by_body {
            if let Some(body) = state.body(name) {
                faces.extend(solidcraft_engine::doc::tangent_face_chain(body, &points).into_iter().map(|(index, point)| Sel::Face {
                    body: name.to_string(),
                    index,
                    point,
                }));
            }
        }
        if let Ok(mut cache) = self.shell_chain.try_borrow_mut() {
            *cache = Some(ShellChain { revision: s.revision, picks: picks.to_vec(), faces: faces.clone() });
        }
        Some(faces)
    }

    pub(crate) fn highlighted(&self, s: &Session) -> Vec<Sel> {
        self.shell_faces(s).unwrap_or_else(|| self.items())
    }

    pub fn highlight_key(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        serde_json::to_string(&self.items()).unwrap_or_default().hash(&mut h);
        self.active.hash(&mut h);
        h.finish()
    }

    /// The selection a hit would add to the active input.
    pub fn candidate(&self, s: &Session, h: &Hit) -> Option<Sel> {
        self.candidate_on(s, h, None)
    }

    /// The face under the cursor (the nearest face the pick ray meets, first among `hits`), looked
    /// up once for all the sketch points and round edges' centres among `hits` when the active
    /// input snaps face places to them (the Single hole's); `None` otherwise.
    pub(crate) fn face_snap(&self, s: &Session, hits: &[Hit]) -> Option<FaceSnap> {
        if !self.active_input().is_some_and(|i| snaps_to_points(i.accept)) {
            return None;
        }
        let Some(Hit::Face { body, index, .. }) = hits.iter().find(|h| matches!(h, Hit::Face { .. })) else { return None };
        let st = s.world_state();
        let near: Vec<Vec3> = hits
            .iter()
            .filter_map(|h| match h {
                Hit::SketchPoint { sketch, at, .. } => st.sketch(*sketch).map(|ss| ss.plane.to_world(*at)),
                Hit::Edge { body, index, .. } => edge_circle_in(&st, body, *index).map(|(c, ..)| c),
                _ => None,
            })
            .collect();
        if near.is_empty() {
            return None;
        }
        Some(FaceSnap::new(s, body, *index, &near))
    }

    /// The selection a hit would add to the active input, given the face under the cursor
    /// ([`Dialog::face_snap`] of the same hits). A face place picked on or near a sketch point that
    /// lies on that face (a circle's centre) is taken at the point, as a points input picks it.
    pub(crate) fn candidate_on(&self, s: &Session, h: &Hit, snap: Option<&FaceSnap>) -> Option<Sel> {
        if matches!(self.kind, Kind::SketchModify(_)) {
            return crate::sketch_modify_ui::candidate(self, s, h);
        }
        if let Some(target) = self.extrude_target(s, h) {
            return Some(target);
        }
        let inp = self.active_input()?;
        if self.takes_components() {
            return match h {
                Hit::Face { body, .. } => occurrence_path_of_body(s, body).map(|path| Sel::Occurrence { path }),
                _ => None,
            };
        }
        // Features are also picked on the canvas: a face stands for the feature that made it.
        if inp.accept == selection::FEATURES {
            return match h {
                Hit::Face { body, index, point } => feature_of_face(s, body, *index, *point).map(|id| Sel::Feature { id }),
                _ => None,
            };
        }
        if snaps_to_points(inp.accept)
            && let Hit::SketchPoint { sketch, at, .. } = h
        {
            let snap = snap?;
            return snap.place_at(s.world_state().sketch(*sketch)?.plane.to_world(*at));
        }
        if let Some(sel) = self.circle_centre_pick(s, inp, h, snap) {
            return Some(sel);
        }
        // The snap looked the face under the cursor up already, planar or not.
        let sel = inp.accepts(h, s.active_sketch, |body, face| {
            snap.and_then(|f| f.planar(body, face)).unwrap_or_else(|| planar_face(s, body, face).is_some())
        })?;
        (inp.accept != AXIS_PICKS || takes_as_axis(s, &sel)).then_some(sel)
    }

    /// A round edge where a 3D point is wanted stands for its centre: a construction point's
    /// input keeps the edge (its centre is found again as the model changes), a face place
    /// (a hole's) is taken at the centre when it lies on the face under the cursor, and any other
    /// point input takes the centre as a point. Inputs that take edges themselves keep the edge
    /// (Move's points find its centre, `dialogs_move::sel_point`).
    fn circle_centre_pick(&self, s: &Session, inp: &SelInput, h: &Hit, snap: Option<&FaceSnap>) -> Option<Sel> {
        let Hit::Edge { body, index, mid, .. } = h else { return None };
        let a = inp.accept;
        let places = snaps_to_points(a);
        if a & EDGES != 0 || (a & selection::VERTICES == 0 && !places) {
            return None;
        }
        let (centre, ..) = edge_circle(s, body, *index)?;
        if places {
            return snap?.place_at(centre);
        }
        Some(if matches!(self.kind, Kind::Construct { .. }) {
            Sel::Edge { body: body.clone(), index: *index, point: *mid }
        } else {
            Sel::Vertex { body: body.clone(), point: centre }
        })
    }

    /// With automatic To Object enabled and profiles picked, a face or vertex off their plane,
    /// or a parallel plane, is the target rather than another profile. An explicitly active
    /// To Object input accepts parallel planes regardless of the automatic switching preference.
    fn extrude_target(&self, s: &Session, h: &Hit) -> Option<Sel> {
        let Kind::Extrude { thin: false, smart, .. } = &self.kind else { return None };
        let to_active = self.active_input().is_some_and(|i| i.label == TO_OBJECT);
        if !to_active && (self.active != 0 || !smart.auto_to_object) {
            return None;
        }
        let reference = profile_plane(s, self.inputs.first()?.items.first()?)?;
        let sel = match h {
            // A place on the plane: its origin (a browser pick carries no point).
            Hit::Plane { name, .. } => {
                let plane = plane_by_name(s, name)?;
                if plane.normal().dot(reference.normal()).abs() < 1.0 - 1e-9 {
                    return None;
                }
                Sel::Vertex { body: String::new(), point: plane.origin }
            }
            _ if to_active => return None,
            Hit::Face { body, index, point } => Sel::Face { body: body.clone(), index: *index, point: *point },
            Hit::Vertex { body, point } => Sel::Vertex { body: body.clone(), point: *point },
            _ => return None,
        };
        object_target(s, &reference, &sel).then_some(sel)
    }

    /// Take a pick as the extrude's To Object (see [`Dialog::extrude_target`]): the extent turns to
    /// To Object with it. False when the pick is not one.
    fn take_extrude_target(&mut self, s: &Session, sel: &Sel) -> bool {
        if self.active != 0
            || !matches!(self.kind, Kind::Extrude { thin: false, smart: crate::smart_extrude::Smart { auto_to_object: true, .. }, .. })
        {
            return false;
        }
        let Some(reference) = self.inputs.first().and_then(|i| i.items.first()).and_then(|first| profile_plane(s, first)) else { return false };
        if !object_target(s, &reference, sel) {
            return false;
        }
        set_object_input(&mut self.inputs, &mut self.active, TO_OBJECT, true);
        let Some(at) = self.inputs.iter().position(|i| i.label == TO_OBJECT) else { return false };
        if let Some(input) = self.inputs.get_mut(at) {
            input.items = vec![sel.clone()];
        }
        self.active = at;
        if let Kind::Extrude { all, .. } = &mut self.kind {
            *all = false;
        }
        self.focus = true;
        self.error = None;
        self.sync_thread(s);
        self.sync_material(s);
        true
    }

    /// Give the keyboard to the form's value field `id`, its text selected, once the pointer is
    /// idle: where a pick gives it when values have no boxes in the view (#9479).
    pub(crate) fn give_keyboard(&mut self, id: egui::Id) {
        self.value_focus = Some(id);
        self.restore_value_focus = true;
    }

    /// A sketch clicked in the Browser or the History while Extrude, Revolve or Emboss is open
    /// (#6106): Profiles takes every profile of the sketch, as when the sketch is selected before
    /// the command starts. The island inside another profile's hole is left out, or it would fill
    /// the hole. False when the click is not one of those (the caller selects it as usual).
    pub fn pick_sketch(&mut self, s: &Session, id: u64) -> bool {
        if self.refuse_sketch(s, id) {
            return true;
        }
        if self.active != 0
            || !matches!(self.kind, Kind::Extrude { .. } | Kind::Revolve { .. } | Kind::Emboss { .. })
            || !self.inputs.first().is_some_and(|i| i.accept & PROFILES != 0)
            || !s.doc.feature(id).is_some_and(|f| matches!(f.kind, FeatureKind::Sketch { .. }))
        {
            return false;
        }
        let profiles: Vec<Sel> = s
            .world_state()
            .sketch(id)
            .map(|ss| outermost_profiles(&ss.profiles).map(|index| Sel::Profile { sketch: id, index }).collect())
            .unwrap_or_default();
        let Some(input) = self.inputs.first_mut() else { return false };
        if profiles.is_empty() {
            return false;
        }
        input.items = profiles;
        self.profile_selection_changed = true;
        self.focus = true;
        self.error = None;
        self.sync_thread(s);
        true
    }

    /// A sketch clicked (in the view, the Browser or the History) while a pattern or Mirror waits
    /// for its bodies or features: a sketch is neither, so the dialog says what to do instead of
    /// taking nothing in silence (#10057). False when the click is not one of those.
    pub fn refuse_sketch(&mut self, s: &Session, id: u64) -> bool {
        if self.active != 0
            || !is_pattern(&self.kind)
            || !self.inputs.first().is_some_and(|i| i.accept == BODIES || i.accept == selection::FEATURES)
            || !s.doc.feature(id).is_some_and(|f| matches!(f.kind, FeatureKind::Sketch { .. }))
        {
            return false;
        }
        self.error = Some(SKETCH_NOT_OBJECT.to_string());
        true
    }

    /// A pick in the viewport (already accepted by the active input).
    pub fn pick(&mut self, s: &Session, sel: Sel) {
        if self.take_extrude_target(s, &sel) {
            return;
        }
        if let Some(key) = stored_input_reference_key(self, self.active) {
            self.extra.remove(key);
        }
        if let Kind::PathPlane { sketch, .. } = &mut self.kind {
            *sketch = None;
            self.extra.remove("path");
        }
        if self.active == 0 && matches!(self.kind, Kind::Extrude { .. } | Kind::Revolve { .. } | Kind::Emboss { .. }) {
            self.profile_selection_changed = true;
        }
        let measure = matches!(self.kind, Kind::Measure { .. });
        // The chat's picks stay what was clicked: a rim is one edge there, not its pieces. So do
        // print faces: a click on one takes it out, not the other pieces of its surface in.
        let one = measure || matches!(self.kind, Kind::ChatContext | Kind::SaveMesh(_));
        let sel = if measure { crate::viewport::measurement_selection(s, &sel) } else { sel };
        let chain = matches!(self.kind, Kind::Fillet { chain: true, .. });
        let single =
            if !one && self.active_input().is_some_and(|i| i.multi) { crate::viewport::picked_selection(s, &sel) } else { vec![sel.clone()] };
        // Tangent chain: an edge brings the edges that continue it smoothly.
        let picked = if chain { fillet_edges(s, &sel, true) } else { single.clone() };
        // Extrude and revolve take profiles of one sketch; both lofts take sections
        // from several sketches and must keep their pick order.
        if let Sel::Profile { sketch, .. } = &sel
            && !matches!(self.kind, Kind::Loft { .. } | Kind::Surface(crate::dialogs_surface::Surf::Loft { .. }))
            && let Some(inp) = self.inputs.get_mut(self.active)
        {
            inp.items.retain(|x| !matches!(x, Sel::Profile { sketch: s2, .. } if s2 != sketch));
        }
        let is_fillet = matches!(self.kind, Kind::Fillet { .. }) && self.active == 0;
        let Some(inp) = self.inputs.get_mut(self.active) else { return };
        if is_fillet {
            self.chain_applied = chain;
            if picked.iter().all(|p| inp.items.iter().any(|x| selection::same_item(x, p))) {
                self.picked_edges.retain(|x| !picked.iter().any(|p| selection::same_item(x, p)));
            } else {
                for p in single {
                    if !self.picked_edges.iter().any(|x| selection::same_item(x, &p)) {
                        self.picked_edges.push(p);
                    }
                }
            }
        }
        inp.toggle(picked);
        self.focus = true;
        // A measurement is between two items: a third pick starts over from the last one.
        if matches!(self.kind, Kind::Measure { .. }) && inp.items.len() > 2 {
            let keep = inp.items.split_off(inp.items.len() - 1);
            inp.items = keep;
        }
        if !inp.multi && !inp.items.is_empty() && self.active + 1 < self.inputs.len() {
            self.advance();
        }
        // Move by points or about an axis: once a body is in, go on to the empty From, Point or
        // Axis (#6170), so the next click picks it instead of another body. Free Move's Pivot is
        // optional and stays unvisited.
        if let Kind::Move { mode: crate::dialogs_move::ROTATE | crate::dialogs_move::POINT_TO_POINT | crate::dialogs_move::POINT_TO_POSITION, .. } =
            self.kind
            && self.active == 0
            && self.inputs.first().is_some_and(|i| !i.items.is_empty())
            && self.inputs.get(1).is_some_and(|i| i.items.is_empty())
        {
            self.active = 1;
        }
        self.error = None;
        self.sync_thread(s);
        self.sync_material(s);
        if let Kind::Primitive { cmd, at, .. } = &mut self.kind {
            let picked = self.inputs.get(self.active).and_then(|i| i.items.first());
            crate::primitive_placement::picked(s, cmd, at, &mut self.extra, picked);
        }
    }

    /// Box selection result: replace (or add to) the active input.
    pub fn take_box(&mut self, s: &Session, sels: Vec<Sel>, add: bool) {
        if let Some(key) = stored_input_reference_key(self, self.active) {
            self.extra.remove(key);
        }
        if let Kind::PathPlane { sketch, .. } = &mut self.kind {
            *sketch = None;
            self.extra.remove("path");
        }
        if self.active == 0 && matches!(self.kind, Kind::Extrude { .. } | Kind::Revolve { .. } | Kind::Emboss { .. }) {
            self.profile_selection_changed = true;
        }
        let measure = matches!(self.kind, Kind::Measure { .. });
        let takes_components = self.takes_components();
        let is_fillet = matches!(self.kind, Kind::Fillet { .. }) && self.active == 0;
        let chain = matches!(self.kind, Kind::Fillet { chain: true, .. });
        if is_fillet && !add {
            self.picked_edges.clear();
        }
        let Some(inp) = self.inputs.get_mut(self.active) else { return };
        if !add {
            inp.items.clear();
        }
        for x in sels {
            if takes_components {
                let occ = match x {
                    Sel::Occurrence { path } if !path.is_empty() => Some(Sel::Occurrence { path }),
                    Sel::Body { name } => occurrence_path_of_body(s, &name).map(|path| Sel::Occurrence { path }),
                    _ => None,
                };
                if let Some(occ) = occ
                    && !inp.items.contains(&occ)
                {
                    inp.items.push(occ);
                }
                continue;
            }
            let x = if measure { crate::viewport::measurement_selection(s, &x) } else { x };
            if !fits(inp.accept, &x) {
                continue;
            }
            if !inp.multi {
                inp.items.clear();
                inp.items.push(x);
                break;
            }
            if is_fillet {
                if !self.picked_edges.iter().any(|y| selection::same_item(y, &x)) {
                    self.picked_edges.push(x.clone());
                }
                for y in fillet_edges(s, &x, chain) {
                    if !inp.items.iter().any(|z| selection::same_item(z, &y)) {
                        inp.items.push(y);
                    }
                }
            } else if !inp.items.iter().any(|y| selection::same_item(y, &x)) {
                inp.items.push(x);
            }
        }
        self.sync_thread(s);
        self.sync_material(s);
    }

    /// Rebuild a Fillet or Chamfer dialog's edge input from the edges that were picked when
    /// Tangent Chain is ticked or unticked.
    pub fn sync_fillet_chain(&mut self, s: &Session) {
        let Kind::Fillet { chain, .. } = &self.kind else { return };
        if *chain == self.chain_applied {
            return;
        }
        self.chain_applied = *chain;
        let Some(inp) = self.inputs.first_mut() else { return };
        let mut items = Vec::new();
        for pick in &self.picked_edges {
            for x in fillet_edges(s, pick, *chain) {
                if !items.iter().any(|y| selection::same_item(y, &x)) {
                    items.push(x);
                }
            }
        }
        inp.items = items;
        self.error = None;
    }
}

/// The edges a Fillet or Chamfer pick stands for: its tangent chain when `chain` is on, else
/// the picked edge (both halves of a split circular rim).
fn fillet_edges(s: &Session, sel: &Sel, chain: bool) -> Vec<Sel> {
    let mut picked = crate::viewport::picked_selection(s, sel);
    if chain
        && let Sel::Edge { body, index, .. } = sel
        && let Some(b) = s.world_state().body(body)
    {
        let m = b.mesh();
        for i in m.tangent_chain(*index, 2f64.to_radians()) {
            if !picked.iter().any(|x| matches!(x, Sel::Edge { index: j, .. } if *j == i))
                && let Some(e) = m.edges.get(i)
            {
                picked.push(Sel::Edge { body: body.clone(), index: i, point: polyline_mid(e) });
            }
        }
    }
    picked
}

/// The profiles of a sketch that aren't the island inside another profile's hole: a circle in a
/// rectangle gives the rectangle-with-hole and the disc, and taking both would fill the hole.
/// Loops are compared as sets of curve ids, so where a loop starts or which way it runs doesn't matter.
fn outermost_profiles(profiles: &[solidcraft_engine::sketch::Profile]) -> impl Iterator<Item = usize> + '_ {
    let key = |ids: &[String]| {
        let mut v = ids.to_vec();
        v.sort();
        v
    };
    let holes: Vec<Vec<String>> = profiles.iter().flat_map(|p| p.hole_curves.iter().map(|h| key(h))).collect();
    (0..profiles.len()).filter(move |&i| profiles.get(i).is_some_and(|p| !holes.contains(&key(&p.outer_curves))))
}

/// Profiles of the sketch a feature would use, when there is just one: the active sketch's, else
/// the last sketch's while no feature builds from it yet. A used profile (a cube's sketch on its
/// bottom face) is only taken when it is picked.
fn default_profiles(s: &Session) -> Vec<Sel> {
    let sid = s.active_sketch.or_else(|| {
        s.doc.features.iter().rev().find(|f| matches!(f.kind, FeatureKind::Sketch { .. })).map(|f| f.id).filter(|&id| !profiles_used(s, id))
    });
    sid.map(|id| single_profile(s, id)).unwrap_or_default()
}

fn single_profile(s: &Session, sketch: u64) -> Vec<Sel> {
    match s.world_state().sketch(sketch).map(|ss| ss.profiles.len()) {
        Some(1) => vec![Sel::Profile { sketch, index: 0 }],
        _ => Vec::new(),
    }
}

/// The sketch point (its sketch and id) a Single hole's face place `x` sits on: the one it was
/// opened at, else a point a shown sketch shows exactly there (where a pick snapped to it).
fn single_hole_point(app: &SolveApp, d: &Dialog, x: &Sel) -> Option<(u64, String)> {
    let Sel::Face { point, .. } = x else { return None };
    if let Some(k) = d.extra.get("single_point").filter(|k| k["point"] == pt3(*point)) {
        return Some((k["sketch"].as_u64()?, k["id"].as_str()?.to_string()));
    }
    let st = app.session.world_state();
    crate::viewport::visible_sketches(app).into_iter().find_map(|sid| {
        let ss = st.sketch(sid)?;
        let shown = SnapPoints::of(&ss.sketch);
        let p = ss.sketch.points.iter().find(|p| shown.takes(p) && ss.plane.to_world(p.pos).dist(*point) < 1e-9)?;
        Some((sid, p.id.clone()))
    })
}

/// Does an input take face places that snap to sketch points on the face (the Single hole's)?
pub(crate) fn snaps_to_points(accept: Accept) -> bool {
    accept & selection::POINTS != 0 && accept & PLANAR_FACES != 0
}

/// Which of a sketch's points a face place snaps to (the Single hole's): the points of geometry
/// the sketch shows. Not generated offset nodes or a text outline's points, which are never drawn or picked on their own
/// (as `view::sketch_point_visible` has it), nor projected points while the sketch hides
/// projected geometry (Show Projected off), whose curves aren't picked either. Hidden sketches
/// offer no points at all. Show Points doesn't count: it only hides the markers of the sketch
/// being edited, and a finished sketch's circle centres are snapped to without markers.
pub(crate) struct SnapPoints<'a> {
    /// Show Projected is off: no linked point is taken.
    hide_linked: bool,
    shown: solidcraft_engine::view::SketchPointVisibility<'a>,
}

impl<'a> SnapPoints<'a> {
    pub(crate) fn of(sk: &'a solidcraft_engine::sketch::Sketch) -> Self {
        Self { hide_linked: sk.view.hide_projected, shown: solidcraft_engine::view::SketchPointVisibility::new(sk) }
    }

    /// Does a face place snap to point `p` of the sketch?
    pub(crate) fn takes(&self, p: &solidcraft_engine::sketch::SPoint) -> bool {
        self.shown.takes(p) && !(self.hide_linked && p.link.is_some())
    }
}

/// How near a sketch point must be to a face to lie on it: a micrometre, wherever the part is (a
/// planar face's triangles lie in its plane), by the true distance ([`triangle_dist`]).
const ON_FACE: f64 = 1e-3;

/// The distance from `p` to the triangle `abc`, exactly: from its plane when `p` is over it,
/// otherwise from its nearest side. (`preview::point_triangle_dist` takes a point just past a
/// side for one over the triangle, with an absolute tolerance that is wide for a sliver.)
fn triangle_dist(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> f64 {
    let sides = [(a, b), (b, c), (c, a)];
    let to_sides = || sides.iter().map(|(u, v)| p.dist_to_segment(*u, *v)).fold(f64::INFINITY, f64::min);
    // However small the triangle: only one without area (or not finite) has no plane.
    let n = (b - a).cross(c - a);
    let len = n.len();
    if !(len > 0.0 && len.is_finite()) {
        return to_sides();
    }
    let unit = n / len;
    let h = (p - a).dot(unit);
    let q = p - unit * h;
    if sides.iter().all(|(u, v)| (*v - *u).cross(q - *u).dot(unit) >= 0.0) { h.abs() } else { to_sides() }
}

/// The most point-in-triangle tests one pick makes for the snap. Points past it aren't snapped
/// to (the face as picked still is), so no sketch, however dense, can stall the viewport.
const SNAP_BUDGET: usize = 1 << 18;

#[cfg(test)]
thread_local! {
    /// A smaller [`SNAP_BUDGET`] for a test that runs it out.
    static TEST_BUDGET: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// Run the snap with a budget of `n` tests per pick on this thread (`None`: [`SNAP_BUDGET`]).
#[cfg(test)]
pub(crate) fn set_snap_budget(n: Option<usize>) {
    TEST_BUDGET.with(|b| b.set(n));
}

fn snap_budget() -> usize {
    #[cfg(test)]
    if let Some(n) = TEST_BUDGET.with(std::cell::Cell::get) {
        return n;
    }
    SNAP_BUDGET
}

/// The face under the cursor, looked up once for all the sketch points one pick offers a face
/// place to (the Single hole's snap): whether it is planar, its plane, and those of its
/// triangles near the points on that plane, with their bounding boxes. A dense sketch over a
/// finely tessellated part costs one planarity lookup and one pass over the body's triangles per
/// pick; then a point off the plane is turned down at once, and a point on it is tested against
/// the face's triangles near the cursor, within [`SNAP_BUDGET`] tests in all.
pub(crate) struct FaceSnap {
    body: String,
    face: usize,
    /// A point of the plane, its unit normal, and how far a point on the face may be from it
    /// (the triangles' own distance plus [`ON_FACE`]); `None` when the face isn't planar.
    plane: Option<(Vec3, Vec3, f64)>,
    tris: Vec<([Vec3; 3], Aabb3)>,
    /// Point-in-triangle tests left for this pick.
    budget: std::cell::Cell<usize>,
}

impl FaceSnap {
    /// Face `face` of `body`, for sketch points at `near` (in the world).
    fn new(s: &Session, body: &str, face: usize, near: &[Vec3]) -> FaceSnap {
        let mut snap = FaceSnap { body: body.to_string(), face, plane: None, tris: Vec::new(), budget: std::cell::Cell::new(snap_budget()) };
        let st = s.world_state();
        let (Some((origin, normal)), Some(b)) = (planar_face(s, body, face), st.body(body)) else { return snap };
        let m = b.mesh();
        #[cfg(test)]
        SNAP_WORK.with(|w| w.set((w.get().0, w.get().1 + m.triangles.len())));
        let tris: Vec<[Vec3; 3]> = m.triangles.iter().zip(&m.tri_face).filter(|(_, f)| **f as usize == face).filter_map(|(t, _)| m.tri(t)).collect();
        // Anything within ON_FACE of a triangle is within this of the plane.
        let stray = tris.iter().flatten().map(|v| (*v - origin).dot(normal).abs()).fold(0.0, f64::max);
        let plane = (origin, normal, stray + ON_FACE);
        let mut region = Aabb3::EMPTY;
        for p in near.iter().filter(|p| near_plane(plane, **p)) {
            region.add(*p);
        }
        snap.tris = tris
            .into_iter()
            .map(|t| {
                let mut b = Aabb3::EMPTY;
                t.iter().for_each(|v| b.add(*v));
                (t, b)
            })
            .filter(|(_, b)| boxes_meet(b, &region, ON_FACE))
            .collect();
        snap.plane = Some(plane);
        snap
    }

    /// Is face `face` of `body` planar, when it is this snap's face?
    fn planar(&self, body: &str, face: usize) -> Option<bool> {
        (self.face == face && self.body == body).then_some(self.plane.is_some())
    }

    /// The face place at `p` (one of the points the snap was made for), when `p` lies on the face.
    fn place_at(&self, p: Vec3) -> Option<Sel> {
        let plane = self.plane?;
        if !(p.is_finite() && near_plane(plane, p)) {
            return None;
        }
        let at = Aabb3 { min: p, max: p };
        for ([a, b, c], bb) in &self.tris {
            let left = self.budget.get();
            if left == 0 {
                return None;
            }
            self.budget.set(left - 1);
            #[cfg(test)]
            SNAP_WORK.with(|w| w.set((w.get().0, w.get().1 + 1)));
            if boxes_meet(bb, &at, ON_FACE) && triangle_dist(p, *a, *b, *c) < ON_FACE {
                return Some(Sel::Face { body: self.body.clone(), index: self.face, point: p });
            }
        }
        None
    }
}

/// Is `p` within reach of the plane (a point of it, unit normal, reach)?
fn near_plane((origin, normal, reach): (Vec3, Vec3, f64), p: Vec3) -> bool {
    (p - origin).dot(normal).abs() <= reach
}

/// Do boxes `a` and `b` meet once `b` is grown by `pad` on every side?
fn boxes_meet(a: &Aabb3, b: &Aabb3, pad: f64) -> bool {
    a.min.x <= b.max.x + pad
        && b.min.x - pad <= a.max.x
        && a.min.y <= b.max.y + pad
        && b.min.y - pad <= a.max.y
        && a.min.z <= b.max.z + pad
        && b.min.z - pad <= a.max.z
}

#[cfg(test)]
thread_local! {
    /// Planar face lookups (each tessellates the body) and triangles the snap looked at, on this
    /// thread: tests bound the work of a pick with them.
    static SNAP_WORK: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
}

/// (planar face lookups, triangles the snap looked at) on this thread since the last call.
#[cfg(test)]
pub(crate) fn take_snap_work() -> (usize, usize) {
    SNAP_WORK.with(|w| w.replace((0, 0)))
}

#[cfg(test)]
mod face_snap_tests {
    use super::*;

    /// A planar face's plane is found once per model state: an open Section Analysis on a face
    /// asks for it several times a frame, and each lookup tessellates the body.
    #[test]
    fn a_planar_face_is_looked_up_once_per_model_state() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length":40,"width":30,"height":20})).unwrap();
        let top = |s: &Session| {
            let st = s.world_state();
            let b = st.bodies.first().unwrap();
            let faces = b.body.faces(1e-3).unwrap();
            let f = faces.iter().max_by(|a, b| a.centroid.z.total_cmp(&b.centroid.z)).unwrap();
            (b.name.clone(), f.index)
        };
        let (body, face) = top(&s);
        take_snap_work();
        let first = planar_face(&s, &body, face).expect("the top face is planar");
        for _ in 0..5 {
            assert_eq!(planar_face(&s, &body, face), Some(first));
        }
        assert_eq!(take_snap_work().0, 1, "the same model state looks the face up once");
        assert!((first.0.z - 20.0).abs() < 1e-9 && (first.1 - Vec3::Z).len() < 1e-9, "{first:?}");

        // A changed model is looked at again.
        s.execute("solid.box", &json!({"length":10,"width":10,"height":50,"operation":"join"})).unwrap();
        let (body, face) = top(&s);
        let again = planar_face(&s, &body, face).expect("the new top face is planar");
        assert_eq!(take_snap_work().0, 1);
        assert!((again.0.z - 50.0).abs() < 1e-9, "{again:?}");
    }

    #[test]
    fn a_point_more_than_a_micrometre_off_a_sliver_is_off_the_face() {
        // A triangle with half-micrometre legs. Points 2 µm below its lower side and 1.6 µm past
        // its long side get past `preview::point_triangle_dist`'s inside test (an absolute
        // tolerance); the snap goes by the true distance and turns them down. Points on a side,
        // or half a micrometre beyond one, are on the face.
        let (a, b, c) = (Vec3::ZERO, Vec3::new(5e-4, 0.0, 0.0), Vec3::new(0.0, 5e-4, 0.0));
        let out = Vec3::new(1.0, 1.0, 0.0) * std::f64::consts::FRAC_1_SQRT_2;
        let mid = Vec3::new(2.5e-4, 2.5e-4, 0.0);
        let off = [Vec3::new(2.5e-4, -2e-3, 0.0), Vec3::new(1.4e-3, 1.4e-3, 0.0)];
        let on = [mid, mid + out * 5e-4, Vec3::new(1e-4, -5e-4, 0.0), Vec3::new(1e-4, 1e-4, 5e-4)];
        let mut bb = Aabb3::EMPTY;
        [a, b, c].into_iter().for_each(|v| bb.add(v));
        let snap = FaceSnap {
            body: "Body1".into(),
            face: 0,
            plane: Some((Vec3::ZERO, Vec3::Z, ON_FACE)),
            tris: vec![([a, b, c], bb)],
            budget: std::cell::Cell::new(SNAP_BUDGET),
        };
        for p in off {
            assert!(crate::preview::point_triangle_dist(p, a, b, c) < ON_FACE, "the inside test alone takes {p:?}");
            assert!(triangle_dist(p, a, b, c) > 1.5e-3, "{p:?}");
            assert_eq!(snap.place_at(p), None, "{p:?}");
        }
        for p in on {
            assert_eq!(snap.place_at(p), Some(Sel::Face { body: "Body1".into(), index: 0, point: p }), "{p:?}");
        }
    }

    #[test]
    fn the_distance_to_a_tiny_triangle_is_from_its_plane_over_it() {
        // Legs of 20 nm: the triangle still has a plane, and a point just under a micrometre
        // above it is that far from it (not from its nearest side), so it is on the face. A
        // triangle without area is measured from its sides; nothing here panics on non-finite input.
        let (a, b, c) = (Vec3::ZERO, Vec3::new(2e-5, 0.0, 0.0), Vec3::new(0.0, 2e-5, 0.0));
        let p = Vec3::new(5e-6, 5e-6, 0.99999e-3);
        assert!((triangle_dist(p, a, b, c) - 0.99999e-3).abs() < 1e-12, "{}", triangle_dist(p, a, b, c));
        assert!((triangle_dist(Vec3::new(0.0, 3.0, 4.0), a, b, b) - 5.0).abs() < 1e-9, "a triangle without area");
        let far = |d: f64| d.is_nan() || d >= ON_FACE;
        assert!(far(triangle_dist(Vec3::new(f64::NAN, 0.0, 0.0), a, b, c)), "a point that isn't finite is never near");
        assert!(far(triangle_dist(p, a, b, Vec3::new(f64::INFINITY, 0.0, 0.0))), "nor is a triangle that isn't finite");
    }
}

/// Does a feature build from this sketch's profiles (extrude, revolve, sweep, loft, emboss…)?
fn profiles_used(s: &Session, sketch: u64) -> bool {
    s.doc.features.iter().any(|f| f.kind.profile_sels().iter().any(|(id, _)| *id == sketch))
}

/// The plane of a planar body face: (point on it, unit normal).
/// The body edge a sketch curve lies on from end to end, if any: a sketch on a face draws its
/// lines over the face's edges.
fn sketch_curve_edge(s: &Session, sel: &Sel) -> Option<Sel> {
    let (sketch, id) = match sel {
        Sel::SketchCurve { id, sketch } => ((*sketch).or(s.active_sketch)?, id),
        Sel::ExternalSketchCurve { sketch, id } => (*sketch, id),
        _ => return None,
    };
    let st = s.world_state();
    let ss = st.sketch(sketch)?;
    let segs = ss.sketch.segs(ss.sketch.curve_index(id)?);
    let (a, b) = (ss.plane.to_world(segs.first()?.start()), ss.plane.to_world(segs.last()?.end()));
    let mid = ss.plane.to_world(segs.get(segs.len() / 2)?.mid());
    // Display edges are polylines: allow a curved edge's chords their sag.
    let tol = (a.dist(mid).max(b.dist(mid)) * 1e-2).max(1e-6);
    let near = |p: Vec3, pts: &[Vec3]| {
        pts.windows(2)
            .filter_map(|w| {
                let (Some(u), Some(v)) = (w.first(), w.get(1)) else { return None };
                let uv = *v - *u;
                let l2 = uv.dot(uv);
                let t = if l2 > 0.0 { ((p - *u).dot(uv) / l2).clamp(0.0, 1.0) } else { 0.0 };
                Some((*u + uv * t).dist(p))
            })
            .fold(f64::INFINITY, f64::min)
    };
    let mut best: Option<(f64, Sel)> = None;
    for body in &st.bodies {
        let (m, place) = body.mesh_in_place();
        for (index, e) in m.edges.iter().enumerate() {
            if !m.edge_pickable(index) {
                continue;
            }
            let pts: Vec<Vec3> = e.iter().map(|p| solidcraft_engine::doc::apply_point(&place, *p)).collect();
            let (Some(p), Some(q)) = (pts.first(), pts.last()) else { continue };
            if !((p.dist(a) < tol && q.dist(b) < tol) || (p.dist(b) < tol && q.dist(a) < tol)) {
                continue;
            }
            let off = near(mid, &pts);
            if off < tol && best.as_ref().is_none_or(|(d, _)| off < *d) {
                best = Some((off, Sel::Edge { body: body.name.clone(), index, point: crate::selection::polyline_mid(&pts) }));
            }
        }
    }
    best.map(|(_, e)| e)
}

pub fn planar_face(s: &Session, body: &str, face: usize) -> Option<(Vec3, Vec3)> {
    let st = s.world_state();
    let known = FACE_PLANES.with(|c| {
        let c = c.borrow();
        c.0.as_ref().filter(|k| std::sync::Arc::ptr_eq(k, &st)).and_then(|_| c.1.iter().find(|(b, f, _)| b == body && *f == face).map(|x| x.2))
    });
    if let Some(found) = known {
        return found;
    }
    #[cfg(test)]
    SNAP_WORK.with(|w| w.set((w.get().0 + 1, w.get().1)));
    let found = st.body(body).and_then(|b| {
        let tol = (b.body.size() * 1e-3).max(1e-3);
        let f = b.body.faces(tol).ok()?.into_iter().find(|f| f.index == face)?;
        Some((f.centroid, f.plane_normal?))
    });
    FACE_PLANES.with(|c| {
        let mut c = c.borrow_mut();
        if !c.0.as_ref().is_some_and(|k| std::sync::Arc::ptr_eq(k, &st)) {
            *c = (Some(st.clone()), Vec::new());
        }
        if c.1.len() >= 8 {
            c.1.remove(0);
        }
        c.1.push((body.to_string(), face, found));
    });
    found
}

/// The fillet or chamfer feature whose blend a body face is (a piece or copy of one too).
pub fn blend_feature(s: &Session, body: &str, face: usize) -> Option<u64> {
    use solidcraft_engine::doc::persistent_name;
    let st = s.world_state();
    let b = st.body(body)?;
    let names = solidcraft_engine::doc::naming::face_names(b);
    let name = names.get(face).filter(|n| persistent_name::is_blend(n))?;
    let f = s.doc.feature(persistent_name::made_by(name)?)?;
    matches!(f.kind, FeatureKind::Fillet { .. } | FeatureKind::Chamfer { .. }).then_some(f.id)
}

thread_local! {
    /// The planar faces looked up last, with the model state they were found in (kept, so the
    /// pointer stays its own). Each lookup tessellates the whole body, about 0.1 s for an
    /// imported part of a few million triangles, and an open Section Analysis on a face asks
    /// for its plane several times a frame.
    #[allow(clippy::type_complexity)]
    static FACE_PLANES: std::cell::RefCell<(Option<std::sync::Arc<solidcraft_engine::doc::ModelState>>, Vec<(String, usize, Option<(Vec3, Vec3)>)>)> =
        const { std::cell::RefCell::new((None, Vec::new())) };
}

/// A plane input as a command parameter: an origin/construction plane name or a face plane.
fn plane_value(s: &Session, sel: Option<&Sel>) -> Option<Value> {
    match sel? {
        Sel::Plane { name } => Some(json!(name)),
        Sel::Face { body, index, point } => {
            let (_, n) = planar_face(s, body, *index)?;
            Some(json!({"origin": pt(*point), "normal": pt(n)}))
        }
        _ => None,
    }
}

/// Put non-empty optional values into the parameters.
pub(crate) fn put(p: &mut Value, list: &[(&str, &str)]) {
    for (k, v) in list {
        if !v.trim().is_empty() {
            p[*k] = json!(v);
        }
    }
}

pub(crate) use crate::ui_kit::{field, row as form_row};

pub(crate) fn check(ui: &mut egui::Ui, on: &mut bool) {
    let r = crate::ui_kit::checkbox(ui, "", on);
    publish_check(ui, &r, *on);
}

pub(crate) fn combo(ui: &mut egui::Ui, id: &str, labels: &[&str], sel: &mut usize) {
    crate::ui_kit::dropdown(ui, id, sel, labels);
}

fn tr(text: &str) -> &str {
    crate::i18n::tr_label(text)
}

fn publish_check(ui: &egui::Ui, response: &egui::Response, selected: bool) {
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), selected, tr(crate::ui_kit::row_label(ui))));
}

fn title(k: &Kind) -> &'static str {
    match k {
        Kind::Sketch => "CREATE SKETCH",
        Kind::SketchIntersect { .. } => "INTERSECT",
        Kind::SketchProject { .. } => "PROJECT",
        Kind::Extrude { .. } => "EXTRUDE",
        Kind::Revolve { .. } => "REVOLVE",
        Kind::Fillet { chamfer: false, .. } => "FILLET",
        Kind::Fillet { chamfer: true, .. } => "CHAMFER",
        Kind::Mirror => "MIRROR",
        Kind::Measure { .. } => "MEASURE",
        Kind::Material { .. } => "PHYSICAL MATERIAL",
        Kind::Midplane => "MIDPLANE",
        Kind::MeshConvert { .. } => "CONVERT MESH",
        Kind::Construct { title, .. } => title,
        Kind::PathPlane { .. } => "PLANE ALONG PATH",
        Kind::Rib { web: false, .. } => "RIB",
        Kind::Rib { web: true, .. } => "WEB",
        Kind::ReplaceFace => "REPLACE FACE",
        Kind::DeleteFaces => "DELETE FACE",
        Kind::Align { .. } => "ALIGN",
        Kind::BoundaryFill { .. } => "BOUNDARY FILL",
        Kind::Remove => "REMOVE",
        Kind::ChatContext => "PICK CONTEXT",
        Kind::PathPattern { .. } => "PATTERN ON PATH",
        Kind::Split => "SPLIT BODY",
        Kind::Thread { .. } => "THREAD",
        Kind::Section { .. } => "SECTION ANALYSIS",
        Kind::PatternRect { .. } => "RECTANGULAR PATTERN",
        Kind::PatternCirc { .. } => "CIRCULAR PATTERN",
        Kind::Loft { .. } => "LOFT",
        Kind::Sweep { .. } => "SWEEP",
        Kind::Move { .. } => "MOVE",
        Kind::Hole { .. } => "HOLE",
        Kind::Primitive { cmd, .. } => match *cmd {
            "solid.box" => "BOX",
            "solid.cylinder" => "CYLINDER",
            "solid.sphere" => "SPHERE",
            "solid.coil" => "COIL",
            "solid.cone" => "CONE",
            _ => "TORUS",
        },
        Kind::Combine { .. } => "COMBINE",
        Kind::EditParam { .. } => "EDIT DIMENSION",
        Kind::Rename { .. } => "RENAME",
        Kind::ConfirmDelete { .. } => "DELETE FEATURE",
        Kind::Assembly(k) => k.title(),
        Kind::Sheet(k) => k.title(),
        Kind::Surface(k) => k.title(),
        Kind::SaveMesh(_) => "SAVE AS MESH",
        Kind::ViewDxf { .. } => "Export Current View as DXF",
        Kind::InsertDrawing(p) => crate::dialogs_insert_drawing::title(p),
        Kind::Plastic(k) => k.title(),
        Kind::Frame(_) => "FRAME MEMBER",
        Kind::Appearance(_) => "APPEARANCE",
        Kind::Motion(k) => k.title(),
        Kind::Part(_) => "INSERT PART",
        Kind::PartSize(_) => "Change Part Size",
        Kind::Text(_) => "EDIT TEXT",
        Kind::TextMove(_) => "MOVE/COPY TEXT",
        Kind::SketchModify(k) => k.title(),
        _ => k.simple_metadata().map_or("", |(_, title)| title),
    }
}

fn hint(inp: &SelInput) -> &'static str {
    let a = inp.accept;
    if a & PROFILES != 0 {
        "click profiles in the view"
    } else if a == AXIS_PICKS {
        "click an axis, a sketch line, a straight or circular edge, or a cylinder face"
    } else if a & EDGES != 0 {
        "click edges or faces"
    } else if a & PLANES != 0 {
        "click a plane or planar face"
    } else if a & AXES != 0 {
        "click an axis or sketch line"
    } else if a & selection::SKETCH_LINES != 0 {
        "click sketch lines"
    } else if a & CURVES != 0 {
        "click sketch curves"
    } else if a & (selection::POINTS | selection::SKETCH_POINTS) != 0 {
        "click sketch points"
    } else if inp.label == "Components" {
        "click components"
    } else if a & BODIES != 0 {
        "click bodies"
    } else {
        "click faces"
    }
}

thread_local! {
    /// Faces already traced to their feature, by (revision, body, face).
    static FACE_FEATURE: std::cell::RefCell<(u64, std::collections::HashMap<(String, usize), Option<u64>>)> = Default::default();
}

/// The feature that made a face: the latest feature before which the face's point lay on no
/// face of the model.
fn feature_of_face(s: &Session, body: &str, index: usize, p: Vec3) -> Option<u64> {
    let key = (body.to_string(), index);
    if let Some(hit) = FACE_FEATURE.with(|c| {
        let c = c.borrow();
        if c.0 == s.revision { c.1.get(&key).copied() } else { None }
    }) {
        return hit;
    }
    let on_model = |st: &solidcraft_engine::doc::ModelState| {
        st.bodies.iter().any(|b| {
            let m = b.mesh();
            let bb = m.bounds();
            let tol = 1e-4 * (1.0 + bb.diagonal());
            p.x >= bb.min.x - tol
                && p.y >= bb.min.y - tol
                && p.z >= bb.min.z - tol
                && p.x <= bb.max.x + tol
                && p.y <= bb.max.y + tol
                && p.z <= bb.max.z + tol
                && m.triangles.iter().filter_map(|t| m.tri(t)).any(|t| point_tri_dist(p, t) < tol)
        })
    };
    let found = s
        .doc
        .features
        .iter()
        .rev()
        .filter(|f| !matches!(f.kind, FeatureKind::Sketch { .. }) && s.model.result(f.id).is_some_and(|r| r.error.is_none()))
        .find(|f| !on_model(&s.model.state_before(f.id)))
        .map(|f| f.id);
    FACE_FEATURE.with(|c| {
        let mut c = c.borrow_mut();
        if c.0 != s.revision {
            *c = (s.revision, std::collections::HashMap::new());
        }
        c.1.insert(key, found);
    });
    found
}

/// Distance from a point to a triangle.
pub(crate) fn point_tri_dist(p: Vec3, [a, b, c]: [Vec3; 3]) -> f64 {
    let n = (b - a).cross(c - a);
    let nn = n.dot(n);
    if nn > 1e-24 {
        let q = p - n * ((p - a).dot(n) / nn);
        let inside = [(a, b), (b, c), (c, a)].iter().all(|(u, v)| (*v - *u).cross(q - *u).dot(n) >= 0.0);
        if inside {
            return (p - q).len();
        }
    }
    p.dist_to_segment(a, b).min(p.dist_to_segment(b, c)).min(p.dist_to_segment(c, a))
}

/// The pattern objects' label: features are picked in the timeline.
fn objects_label(accept: Accept) -> &'static str {
    if accept == selection::FEATURES { "Features" } else { "Objects" }
}

/// What a pattern or Mirror says when a sketch is clicked for its objects (#10057). It names what
/// to pick, so it shows as a hint rather than an error ([`missing_pick`]).
const SKETCH_NOT_OBJECT: &str =
    "Pick bodies or features: a sketch is neither. Repeat its curves with the sketch's own Pattern or Mirror, or extrude it first.";

fn is_pattern(k: &Kind) -> bool {
    matches!(k, Kind::PatternRect { .. } | Kind::PatternCirc { .. } | Kind::PathPattern { .. } | Kind::Mirror)
}

/// The Object Type row of a pattern: bodies (picked in the view) or features (picked in the
/// timeline, e.g. a hole).
fn object_type_row(d: &mut Dialog, ui: &mut egui::Ui) {
    let Some(inp) = d.inputs.first_mut() else { return };
    let cur = usize::from(inp.accept == selection::FEATURES);
    let mut ty = cur;
    // What the two kinds are, and that a sketch is neither (#10057).
    choice_row(ui, "Object Type", "pat_obj", &mut ty, &["Bodies", "Features"]).on_hover_text(crate::i18n::tr("ui.pattern_object_type_tip"));
    if ty != cur {
        inp.accept = if ty == 1 { selection::FEATURES } else { BODIES };
        inp.label = objects_label(inp.accept);
        // The pre-selection moved into this input when the dialog opened, so switching back
        // restores what the other type held instead of losing it.
        let back = std::mem::take(&mut d.other_objects);
        d.other_objects = std::mem::replace(&mut inp.items, back);
        d.active = 0;
    }
}

fn align_object_type_row(d: &mut Dialog, ui: &mut egui::Ui) {
    let Kind::Align { components, .. } = &mut d.kind else { return };
    let Some(inp) = d.inputs.first_mut() else { return };
    let cur = usize::from(*components);
    let mut ty = cur;
    choice_row(ui, "Object Type", "align_obj", &mut ty, &["Bodies", "Components"]);
    if ty != cur {
        *components = ty == 1;
        inp.label = if *components { "Components" } else { "Bodies" };
        let back = std::mem::take(&mut d.other_objects);
        d.other_objects = std::mem::replace(&mut inp.items, back);
        d.active = 0;
        d.error = None;
    }
}

/// Rows above the selection inputs: the pattern type and object type, the Hole placement (one
/// face point, or sketch points).
fn placement_row(d: &mut Dialog, ui: &mut egui::Ui, app: &SolveApp) {
    let session = &app.session;
    if matches!(d.kind, Kind::Mirror) {
        object_type_row(d, ui);
        return;
    }
    if matches!(d.kind, Kind::Align { .. }) {
        align_object_type_row(d, ui);
        return;
    }
    if matches!(d.kind, Kind::Move { .. }) {
        crate::dialogs_move::type_row(app, d, ui);
        return;
    }
    let current = match d.kind {
        Kind::PatternRect { .. } => Some(0),
        Kind::PatternCirc { .. } => Some(1),
        Kind::PathPattern { .. } => Some(2),
        _ => None,
    };
    if let Some(cur) = current {
        let mut ty = cur;
        choice_row(ui, "Type", "pat_type", &mut ty, &["Rectangular", "Circular", "Path"]);
        if ty != cur {
            // Same objects, the new pattern's other inputs.
            let objects = d.inputs.first().map(|i| i.items.clone()).unwrap_or_default();
            let (kind, mut inputs) = match ty {
                0 => (
                    Kind::PatternRect {
                        count: "3".into(),
                        spacing: length_default(session, "20"),
                        count2: "1".into(),
                        spacing2: length_default(session, "20"),
                    },
                    vec![
                        SelInput::new("Objects", BODIES, true),
                        SelInput::new("Direction", AXES | EDGES, false),
                        SelInput::new("Direction 2", AXES | EDGES, false),
                    ],
                ),
                1 => (
                    Kind::PatternCirc { count: "6".into(), angle: "360 deg".into() },
                    vec![SelInput::new("Objects", BODIES, true), SelInput::new("Axis", AXIS_PICKS, false)],
                ),
                _ => (
                    Kind::PathPattern { count: "4".into(), spacing: length_default(session, "20") },
                    vec![SelInput::new("Objects", BODIES, true), SelInput::new("Path", CURVES, true)],
                ),
            };
            let accept = d.inputs.first().map(|i| i.accept).unwrap_or(BODIES);
            if let Some(i) = inputs.first_mut() {
                i.items = objects;
                i.accept = accept;
                i.label = objects_label(accept);
            }
            d.kind = kind;
            d.inputs = inputs;
            d.extra.clear();
            d.advance();
        }
        object_type_row(d, ui);
        return;
    }
    let Kind::Hole { opts, .. } = &mut d.kind else { return };
    let mut m = usize::from(opts.multiple);
    choice_row(ui, "Placement", "hole_place", &mut m, &["Single", "Multiple"]);
    if (m == 1) != opts.multiple {
        opts.multiple = m == 1;
        d.inputs = if opts.multiple { vec![SelInput::new("Points", selection::POINTS, true)] } else { single_hole_inputs() };
        d.active = 0;
        d.extra.remove("direction");
    }
}

/// The placement a click on a placed component's world body picks (`None` for root-component
/// bodies): the one directly under the active component, as the browser shows it. With the design
/// active a part inside a subassembly picks the subassembly; with the subassembly (or the part)
/// active it picks the part. A body outside the active component picks its top-level placement.
pub(crate) fn occurrence_path_of_body(s: &Session, body: &str) -> Option<Vec<u64>> {
    let st = s.world_state();
    let b = st.body(body)?;
    if s.doc.model_body_component(b) == 0 {
        return None;
    }
    let model = s.model.state();
    let (_, _, _, path, _) = s.placed_document().body_frames_with_source(&model).into_iter().find(|(world, ..)| world == body)?;
    let depth = pick_depth(s, &path);
    let path: Vec<u64> = path.into_iter().take(depth).collect();
    (!path.is_empty()).then_some(path)
}

/// How many ids of a body's full placement `path` a component pick keeps (see
/// [`occurrence_path_of_body`]).
fn pick_depth(s: &Session, path: &[u64]) -> usize {
    let context = match &s.active_occurrence_path {
        Some(active) => Some(active.len()),
        // An active component without an exact placement: the placement of it on this path.
        None if s.active_component != 0 => {
            path.iter().position(|id| s.doc.occurrences.iter().any(|o| o.id == *id && o.component == s.active_component)).map(|i| i + 1)
        }
        None => Some(0),
    };
    match context {
        Some(n) if s.active_occurrence_path.as_deref().is_none_or(|active| path.starts_with(active)) => (n + 1).min(path.len()),
        _ => 1,
    }
}

/// The note Align shows when a picked component sits inside a subassembly placed more than once.
const ALIGN_SHARED_WARNING: &str = "This component is inside a subassembly placed more than once: aligning it moves it in every placement.";

/// Does aligning these Components picks move a placement shared by several subassembly copies?
fn align_moves_shared(s: &Session, items: &[Sel]) -> bool {
    occurrence_paths(s, items).iter().any(|path| s.occurrence_shared(path))
}

fn occurrence_paths(s: &Session, items: &[Sel]) -> Vec<Vec<u64>> {
    let mut out = Vec::new();
    for item in items {
        let path = match item {
            Sel::Occurrence { path } if !path.is_empty() => Some(path.clone()),
            Sel::Body { name } => occurrence_path_of_body(s, name),
            _ => None,
        };
        if let Some(p) = path
            && !out.contains(&p)
        {
            out.push(p);
        }
    }
    out
}

/// A picked item as its chip shows it. A round edge or face says what it stands for in `inp`:
/// its axis in an axis input, a circle's centre in a point input. An origin plane goes by
/// the name the preferences give it.
pub(crate) fn selection_label(s: &Session, names: crate::prefs::OriginPlaneNames, d: &Dialog, inp: &SelInput, item: &Sel) -> String {
    let axis = inp.accept == selection::AXIS_PICKS && round_axis(s, item).is_some();
    match item {
        Sel::SketchCurve { id, .. } => id.clone(),
        Sel::SketchPoint { id } => crate::sketch_modify_ui::point_id(s, item).unwrap_or(id).to_string(),
        Sel::Plane { name } => names.label(name).to_string(),
        Sel::Axis { name } | Sel::ConstructionPoint { name } | Sel::Body { name } => name.clone(),
        Sel::Face { body, index, .. } if axis => format!("{body} · face {index} axis"),
        Sel::Face { body, index, .. } => format!("{body} · face {index}"),
        Sel::Edge { body, .. } if axis => format!("{body} · circle axis"),
        Sel::Edge { body, index, .. } if d.edge_is_centre(inp) && edge_circle(s, body, *index).is_some() => {
            format!("{body} · circle centre")
        }
        Sel::Edge { body, index, .. } => format!("{body} · edge {index}"),
        // A point of the model: a vertex, or the centre a round edge stands for.
        Sel::Vertex { body, .. } if !body.is_empty() => format!("{body} · point"),
        Sel::Feature { id } => s.doc.feature(*id).map(|f| f.name.clone()).unwrap_or_else(|| format!("Feature {id}")),
        Sel::Occurrence { path } => s.occurrence_placement(path).map(|p| p.name).unwrap_or_else(|_| "Component".into()),
        Sel::Vertex { body, point } if body.is_empty() => {
            let state = s.world_state();
            let p = s.active_sketch.and_then(|id| state.sketch(id)).map(|sk| sk.plane.to_local(*point)).unwrap_or_default();
            format!("({:.2}, {:.2}) mm", p.x, p.y)
        }
        Sel::Vertex { body, .. } => format!("{body} · vertex"),
        _ => format!("{item:?}"),
    }
}

/// Selection input rows: label, "N selected" (or a hint) and a clear button; clicking a row
/// makes it the active input.
fn input_rows(d: &mut Dialog, ui: &mut egui::Ui, s: &Session, names: crate::prefs::OriginPlaneNames) {
    if !d.inputs.iter().any(|i| i.under_choice) {
        return input_rows_range(d, ui, s, names, 0..d.inputs.len());
    }
    // The inputs a form draws under the choices that add them are left out here.
    let listed: Vec<usize> = d.inputs.iter().enumerate().filter(|(_, i)| !i.under_choice).map(|(n, _)| n).collect();
    for i in listed {
        input_rows_range(d, ui, s, names, i..i + 1);
    }
}

/// The row of the selection input labelled `label`, when the dialog has one.
fn input_row(d: &mut Dialog, ui: &mut egui::Ui, s: &Session, names: crate::prefs::OriginPlaneNames, label: &str) {
    if let Some(i) = d.inputs.iter().position(|x| x.label == label) {
        input_rows_range(d, ui, s, names, i..i + 1);
    }
}

/// Saved vectors/axes are real inputs even when they have no origin-axis selection chip.
fn stored_input_reference_key(d: &Dialog, index: usize) -> Option<&'static str> {
    if let Kind::Construct { command: "construct.plane.axis_point", keys, .. } = &d.kind {
        return keys.get(index).copied();
    }
    if matches!(d.kind, Kind::Section { .. }) && index == 0 {
        return Some("plane");
    }
    if !d.inputs.first().is_some_and(|i| i.accept == selection::FEATURES) {
        return None;
    }
    match (&d.kind, index) {
        (Kind::PatternRect { .. }, 1) => Some("dir1"),
        (Kind::PatternRect { .. }, 2) => Some("dir2"),
        (Kind::PatternCirc { .. }, 1) => Some("axis"),
        _ => None,
    }
}

fn stored_revolve_axis(kind: &Kind) -> Option<&Value> {
    match kind {
        Kind::Revolve { axis, .. } | Kind::Surface(crate::dialogs_surface::Surf::Revolve { axis, .. }) => axis.as_ref(),
        _ => None,
    }
}

/// Give the keyboard back from whatever text editor holds it, so the pick field clicked into
/// becomes the one field that looks active (#10066).
fn surrender_keyboard_focus(ui: &egui::Ui) {
    if let Some(id) = ui.ctx().memory(|memory| memory.focused()) {
        ui.ctx().memory_mut(|memory| memory.surrender_focus(id));
    }
}

pub(crate) fn input_rows_range(d: &mut Dialog, ui: &mut egui::Ui, s: &Session, names: crate::prefs::OriginPlaneNames, range: std::ops::Range<usize>) {
    // Edit Text shows its Path input among its own rows.
    if matches!(d.kind, Kind::Text(_)) {
        return;
    }
    let t = Tokens::get();
    let mut clear: Option<usize> = None;
    let mut activate: Option<usize> = None;
    let mut remove = None;
    // Only one field may look active at a time (#10066): while a text editor holds the keyboard,
    // no pick field draws its accent ring, and clicking a pick field takes the keyboard back.
    let text_focus = crate::ui_kit::editor_has_focus(ui.ctx());
    let shell_count = d.shell_faces(s).map(|faces| faces.len());
    for (i, inp) in d.inputs.iter().enumerate().skip(range.start).take(range.end.saturating_sub(range.start)) {
        let stored_reference = stored_input_reference_key(d, i).and_then(|key| d.extra.get(key)).filter(|_| inp.items.is_empty());
        form_row(ui, inp.label, |ui| {
            ui.horizontal(|ui| {
                let active = i == d.active && !text_focus;
                // A chip: "↖ Select" (outlined) while empty, "N selected" (filled) once picked.
                let custom_axis = i == 1 && inp.items.is_empty() && stored_revolve_axis(&d.kind).is_some();
                let filled = !inp.items.is_empty() || custom_axis || stored_reference.is_some();
                let text = if filled {
                    RichText::new(if stored_reference.is_some() && matches!(d.kind, Kind::Section { .. }) {
                        crate::i18n::tr("ui.viewport_status_section_plane").into()
                    } else if custom_axis || stored_reference.is_some() {
                        crate::i18n::tr("dialog.custom_axis").into()
                    } else if !inp.multi {
                        inp.items.first().map(|item| selection_label(s, names, d, inp, item)).unwrap_or_default()
                    } else {
                        let count = if i == 0 { shell_count.unwrap_or(inp.items.len()) } else { inp.items.len() };
                        crate::i18n::format("dialog.selected_count", &[("count", &count.to_string())])
                    })
                    .color(t.on_accent)
                } else {
                    RichText::new(crate::i18n::tr("dialog.select")).color(t.text)
                };
                let selected_text = text.text().strip_prefix("↖ ").unwrap_or_else(|| text.text());
                let prompt = if !filled && inp.label == "Edges" { tr("Select edges") } else { selected_text };
                let chip = crate::ui_kit::selection_field(ui, prompt, crate::ui_kit::Selection { active, filled });
                if matches!(d.kind, Kind::Shell { .. }) && i == 0 {
                    chip.widget_info(|| {
                        let mut info = egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), tr(inp.label));
                        info.current_text_value = Some(prompt.to_string());
                        info
                    });
                }
                let chip_rect = chip.rect;
                let tooltip = if let Some(reference) = stored_reference {
                    reference.to_string()
                } else if custom_axis {
                    if let Some(axis) = stored_revolve_axis(&d.kind) {
                        match axis.get("at") {
                            // A kept edge or face of the model (one no longer found).
                            Some(at) => format!("Custom axis\nThe edge or face at {at}\nClick to select a replacement axis"),
                            None => format!(
                                "Custom axis\nOrigin: {}\nDirection: {}\nClick to select a replacement axis",
                                axis.get("origin").map(ToString::to_string).unwrap_or_default(),
                                axis.get("dir").map(ToString::to_string).unwrap_or_default()
                            ),
                        }
                    } else {
                        hint(inp).to_string()
                    }
                } else {
                    format!("{} (click to pick into this input)", hint(inp))
                };
                if chip.on_hover_text(tooltip).clicked() {
                    activate = Some(i);
                    surrender_keyboard_focus(ui);
                }
                if !inp.items.is_empty() || stored_reference.is_some() {
                    let rect = egui::Rect::from_center_size(
                        egui::pos2(chip_rect.right() - t.space_24 / 2.0, chip_rect.center().y),
                        vec2(t.space_24, t.space_24),
                    );
                    let button = crate::ui_kit::icon_button_at(ui, rect, "close", "Clear the selection");
                    crate::scenario::publish_handle(&format!("dialog:clear:{}", inp.label), button.rect.center());
                    if button.clicked() {
                        clear = Some(i);
                        surrender_keyboard_focus(ui);
                    }
                }
            });
        });
        if matches!(d.kind, Kind::SketchModify(_)) && inp.multi {
            for (index, item) in inp.items.iter().enumerate() {
                if crate::ui_kit::list_row(ui, &format!("{}:{index}", inp.label), &selection_label(s, names, d, inp, item)).clicked() {
                    remove = Some((i, index));
                }
            }
        }
    }
    if let Some((input, index)) = remove
        && let Some(input) = d.inputs.get_mut(input)
        && index < input.items.len()
    {
        input.items.remove(index);
    }
    if let Some(i) = activate {
        d.active = i;
    }
    if let Some(i) = clear {
        if let Some(key) = stored_input_reference_key(d, i) {
            d.extra.remove(key);
        }
        let Some(inp) = d.inputs.get_mut(i) else { return };
        inp.items.clear();
        if i == 0 {
            d.profile_selection_changed = true;
            d.picked_edges.clear();
        }
        d.active = i;
    }
    d.sync_thread(s);
}

/// Feature patterns use the same source-fixed slot ordering and controls as sketch patterns.
fn feature_pattern_alignment_row(ui: &mut egui::Ui, extra: &mut serde_json::Map<String, Value>, index: usize) {
    use solidcraft_engine::sketch::PatternAlignment;
    let (label, alignment_key, reversed_key) =
        if index == 0 { ("Alignment", "alignment", "reversed") } else { ("Alignment 2", "alignment2", "reversed2") };
    let alignment = extra.get(alignment_key).cloned().and_then(|v| serde_json::from_value::<PatternAlignment>(v).ok()).unwrap_or_default();
    let mut reversed = extra.get(reversed_key).and_then(Value::as_bool).unwrap_or(false);
    // Retain programmatic Left placement until the user changes its alignment.
    let mut centered = alignment == PatternAlignment::Center;
    let was_centered = centered;
    form_row(ui, label, |ui| {
        ui.horizontal(|ui| {
            pattern_toggle(ui, label, "Flip direction", "pattern_flip", &mut reversed);
            pattern_toggle(ui, label, "Center", "pattern_center", &mut centered);
        });
        extra.insert(reversed_key.into(), json!(reversed));
        if centered != was_centered {
            extra.insert(alignment_key.into(), json!(if centered { PatternAlignment::Center } else { PatternAlignment::Right }));
        }
    });
}

pub(crate) fn pattern_toggle(ui: &mut egui::Ui, row: &str, label: &str, icon: &str, selected: &mut bool) {
    crate::ui_kit::toggle_icon(ui, row, label, selected, icon);
}

/// A refusal that only says nothing is picked yet ("Select the curves to offset first", "select the
/// corner to cut first"): guidance, not a failure. The sources word it with or without a capital.
fn awaiting_selection(error: &str) -> bool {
    error.get(..7).is_some_and(|start| start.eq_ignore_ascii_case("select "))
}

/// A dialog's refusal that names what to pick ("select the plane first", "pick an axis").
fn missing_pick(error: &str) -> bool {
    awaiting_selection(error) || error.get(..5).is_some_and(|start| start.eq_ignore_ascii_case("pick "))
}

fn error_rows(app: &mut SolveApp, ctx: &egui::Context, d: &mut Dialog, ui: &mut egui::Ui) {
    if matches!(d.kind, Kind::SketchModify(_))
        && d.previews()
        && let Err(error) = apply_commands(app, d)
    {
        d.refuse(app, error);
    }
    let shown_id = egui::Id::new("sc_dialog_refusal_shown");
    if d.error.is_none() && app.preview.error.is_none() {
        ui.data_mut(|m| m.remove::<String>(shown_id));
    }
    if let Some(error) = d.error.as_ref().or(app.preview.error.as_ref()).cloned() {
        // "Select the curves to offset first" is not a failure: nothing is picked yet. The header hint
        // and the disabled OK already say so, so it is not drawn as a red error with an issue button.
        if matches!(d.kind, Kind::SketchModify(_)) && awaiting_selection(&error) {
            return;
        }
        // Other dialogs say so on OK ("select a plane first", "pick an axis"): that is what is
        // left to pick, not a failure, so it is shown plainly, with no Create Issue (#3292, #3773).
        let rect = if missing_pick(&error) {
            crate::ui_kit::text(ui, &shown_error(&error), crate::ui_kit::Text::Hint("missing_pick")).rect
        } else {
            ui.vertical(|ui| {
                crate::ui_kit::text(ui, &shown_error(&error), crate::ui_kit::Text::Report("error", true));
                let response = crate::ui_kit::button(ui, "Create Issue…", crate::ui_kit::Button::Normal);
                crate::scenario::publish_handle("dialog:create_issue", response.rect.center());
                if response.clicked() {
                    crate::issue_report::begin(app, ctx, Some(d), Some(&error));
                }
            })
            .response
            .rect
        };
        // The refusal comes last in the dialog, under the fold when the fields fill it: only its
        // first line showed, or none, and no Create Issue (#8454). The dialog scrolls a new one
        // into view once: on OK at once, a preview's once no field is being typed in (so the
        // field never moves away while it is typed in).
        let typing = ui.ctx().memory(|m| m.focused().is_some());
        if reveal_refusal(d.error.as_ref() == Some(&error), typing, ui.data(|m| m.get_temp::<String>(shown_id)).as_deref(), &error) {
            ui.scroll_to_rect(rect, None);
            ui.data_mut(|m| m.insert_temp(shown_id, error));
        }
    }
}

/// Whether a dialog scrolls `error` into view this frame: once per refusal, a refusal on OK at once
/// and a preview's once no field is typed in.
fn reveal_refusal(on_ok: bool, typing: bool, shown: Option<&str>, error: &str) -> bool {
    (on_ok || !typing) && shown != Some(error)
}

/// A refusal as the dialog shows it: without the engine's "invalid parameters for `id`: "
/// prefix (the dialog already names the command), and in the interface language when the
/// message is in the catalogs.
fn shown_error(error: &str) -> String {
    let msg = error.strip_prefix("invalid parameters for `").and_then(|rest| rest.split_once("`: ")).map_or(error, |(_, msg)| msg);
    crate::i18n::tr_diagnostic(crate::i18n::tr_label(msg))
}

/// Find the Boundary Fill cells again when the tool bodies changed (keeping the choices of
/// cells that are still there).
fn update_fill_cells(app: &SolveApp, d: &mut Dialog) {
    let tools: Vec<Sel> = sels(d, 0).to_vec();
    let Kind::BoundaryFill { cells, of, .. } = &mut d.kind else { return };
    if *of == tools {
        return;
    }
    let names: Vec<String> = tools.body_names();
    // Editing: the tools as they were before this feature (it may have removed them).
    let st = match d.editing {
        Some((id, _)) => app.session.model.state_before(id),
        None => app.session.world_state(),
    };
    let found = fill_cells(&st, &names);
    let kept: Vec<String> = cells.iter().filter(|c| c.2).map(|c| c.1.clone()).collect();
    *cells = found.into_iter().map(|(p, label)| (p, label.clone(), kept.is_empty() || kept.contains(&label))).collect();
    *of = tools;
}

/// The cells a set of tool bodies makes: each region inside some of them and outside the rest,
/// as a point inside it and which tools hold it. Found by sampling a grid over the tools.
pub(crate) fn fill_cells(st: &solidcraft_engine::doc::ModelState, tools: &[String]) -> Vec<(Vec3, String)> {
    let bodies: Vec<_> = tools.iter().filter_map(|n| st.body(n).map(|b| (n.clone(), b.mesh()))).take(16).collect();
    if bodies.is_empty() {
        return Vec::new();
    }
    let indexes: Vec<_> = bodies.iter().map(|(_, m)| m.inside_index()).collect();
    let bb = bodies.iter().fold(solidcraft_engine::geom::Aabb3::EMPTY, |b, (_, m)| b.union(&m.bounds()));
    let n = 20;
    let at = |i: usize, lo: f64, hi: f64| lo + (hi - lo) * (i as f64 + 0.5) / n as f64;
    // Signature (which tools hold the point) → the points with it.
    let mut groups: Vec<(u32, Vec<Vec3>)> = Vec::new();
    for i in 0..n {
        for j in 0..n {
            for k in 0..n {
                let p = Vec3::new(at(i, bb.min.x, bb.max.x), at(j, bb.min.y, bb.max.y), at(k, bb.min.z, bb.max.z));
                let sig = indexes.iter().enumerate().fold(0u32, |acc, (t, ix)| if ix.contains(p) { acc | (1 << t) } else { acc });
                if sig == 0 {
                    continue;
                }
                match groups.iter_mut().find(|(g, _)| *g == sig) {
                    Some((_, v)) => v.push(p),
                    None => groups.push((sig, vec![p])),
                }
            }
        }
    }
    groups.sort_by_key(|(sig, _)| *sig);
    groups
        .into_iter()
        .filter_map(|(sig, pts)| {
            let c = pts.iter().fold(Vec3::ZERO, |a, p| a + *p) * (1.0 / pts.len() as f64);
            // The sample nearest the middle (the middle itself may be outside a curved cell).
            let rep = pts.iter().min_by(|a, b| a.dist(c).total_cmp(&b.dist(c))).copied()?;
            let inside: Vec<&str> = bodies.iter().enumerate().filter(|(t, _)| sig & (1 << t) != 0).map(|(_, (name, _))| name.as_str()).collect();
            Some((rep, format!("Inside {}", inside.join(", "))))
        })
        .collect()
}

/// Measure again when the picked items changed.
fn update_measure(app: &SolveApp, d: &mut Dialog) {
    let items: Vec<Sel> = sels(d, 0).to_vec();
    if let Kind::Measure { result, of, labels, .. } = &mut d.kind
        && *of != items
    {
        // Another measurement: its labels start in their own places.
        *labels = Default::default();
        *result = if items.is_empty() { None } else { Some(app.session.measure_items(&items)) };
        *of = items;
    }
}

/// Which distance of a pair the Measure panel shows (and the canvas draws).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    /// Centre to centre, or from a round item's axis.
    Centre,
    /// Closest points.
    Min,
    /// Farthest points.
    Max,
}

/// A finite `[x, y, z]` of plain numbers.
pub(crate) fn vec3_of(v: &Value) -> Option<Vec3> {
    let a = v.as_array()?;
    let p = Vec3::new(a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?);
    p.is_finite().then_some(p)
}

/// The distance shown for a measured pair: which one, its value and its two ends. The chosen
/// one when the result has it, otherwise the result's default (centre, else the closest).
pub(crate) fn shown_distance(r: &Value, show: Option<Shown>) -> Option<(Shown, f64, Vec3, Vec3)> {
    let get = |which: Shown| -> Option<(Shown, f64, Vec3, Vec3)> {
        let (d, from, to) = match which {
            Shown::Centre => ("center_distance", "center_from", "center_to"),
            Shown::Min => ("distance_mm", "from", "to"),
            Shown::Max => ("max", "max_from", "max_to"),
        };
        Some((which, r.get(d)?.as_f64()?, vec3_of(r.get(from)?)?, vec3_of(r.get(to)?)?))
    };
    let default = if r.get("measure").and_then(Value::as_str) == Some("center") { Shown::Centre } else { Shown::Min };
    show.and_then(get).or_else(|| get(default)).or_else(|| get(Shown::Min))
}

/// A new To Object target of a new extrude sets the distance to how far it is, so the length
/// shows and stays when the extent goes back to Distance.
fn sync_reach(app: &SolveApp, d: &mut Dialog) {
    let Kind::Extrude { smart, start, to_offset, .. } = &d.kind else { return };
    if !smart.auto_to_object {
        return;
    }
    let target = d.inputs.iter().find(|i| i.label == TO_OBJECT).and_then(|i| i.items.first()).cloned();
    if smart.target == target {
        return;
    }
    // A start offset, a To offset or a From Object move the ends: the distance stays as it is.
    let plain = d.editing.is_none() && start.trim().is_empty() && to_offset.trim().is_empty() && !object_on(&d.inputs, FROM_OBJECT);
    // Picks and the profiles' plane are both in world space. A face running along the normal is
    // reached where it begins, not at the pick on it.
    let reach = match &target {
        Some(Sel::Face { point, .. } | Sel::Vertex { point, .. }) if plain => {
            d.inputs.first().and_then(|i| i.items.first()).and_then(|first| profile_plane(&app.session, first)).map(|plane| {
                let under = *point - plane.normal() * plane.height(*point);
                solidcraft_engine::doc::object_height(&app.session.world_state(), &plane, under, *point)
            })
        }
        _ => None,
    };
    let Kind::Extrude { smart, distance, direction, .. } = &mut d.kind else { return };
    smart.target = target;
    if let Some(reach) = reach.filter(|r| r.is_finite() && r.abs() > 1e-9) {
        let text = format!("{:.6}", reach.abs());
        *distance = format!("{} mm", text.trim_end_matches('0').trim_end_matches('.'));
        // The side the target is on, for when the extent goes back to Distance.
        if *direction < 2 {
            *direction = usize::from(reach < 0.0);
        }
    }
}

/// Smart Extrude (#1257, see [`crate::smart_extrude`]): a new pick sets the direction from the
/// side it was picked from, and the automatic operation follows the geometry, latched per side.
fn auto_operation(app: &SolveApp, d: &mut Dialog) {
    let Kind::Extrude { smart, .. } = &d.kind else { return };
    let picked = d.inputs.first().map(|i| i.items.as_slice()).unwrap_or_default();
    if smart.picked != picked {
        let new_first = smart.picked.first() != picked.first();
        let picked = picked.to_vec();
        let side_of_pick = if new_first && !smart.manual_dir { picked.first().and_then(|first| picked_side(app, d, first)) } else { None };
        let Kind::Extrude { direction, smart, .. } = &mut d.kind else { return };
        smart.picked = picked;
        smart.latch = None;
        if *direction < 2
            && let Some(dir) = side_of_pick
        {
            *direction = dir;
        }
    }
    let Kind::Extrude { direction, distance, .. } = &d.kind else { return };
    let Ok(dist) = crate::length_input::eval(&app.session.doc, distance, ValueKind::Length) else { return };
    let mut side = crate::smart_extrude::side(dist, *direction);
    if !matches!(d.kind, Kind::Extrude { auto_op: true, .. }) {
        return;
    }
    let Some(params) = dialog_commands(app, d).ok().and_then(|c| c.into_iter().next()).map(|(_, p)| p) else { return };
    // A new start (From Object, a start offset) puts the extrude somewhere else: the latch was
    // decided for the old one (#467).
    let start = json!([params.get("from"), params.get("start_offset")]);
    if let Kind::Extrude { smart, .. } = &mut d.kind
        && smart.start != start
    {
        smart.start = start;
        smart.latch = None;
    }
    // Showing or hiding a body changes what the extrude meets: the latch was decided among the
    // bodies shown before (#9919).
    let shown = solidcraft_engine::shown_scope(&app.session);
    if let Kind::Extrude { smart, .. } = &mut d.kind
        && smart.shown != shown
    {
        smart.shown = shown.clone();
        smart.latch = None;
    }
    // To an object, the extrude goes to it on whichever side it is: that is the side (#1633).
    if params.get("to").is_some()
        && let Some(reach) = solidcraft_engine::extrude_reach(&app.session, &params)
    {
        side = crate::smart_extrude::side(reach, 0);
    }
    // Hidden bodies are out of its scope (#6901). With none shown there is nothing to join or
    // cut: an empty `participants` would read as every body, hidden ones too (#9919).
    let mut params = params;
    let mut nothing_shown = false;
    if let Some(shown) = shown
        && params.get("participants").is_none()
    {
        nothing_shown = shown.is_empty();
        params["participants"] = json!(shown);
    }
    let fresh = if nothing_shown {
        Some("new")
    } else {
        // Asked again only when the parameters or the design change: it casts rays through
        // every triangle of the bodies, milliseconds a frame on a large part.
        let key = auto_operation_key(app, &params);
        let cached = match &d.kind {
            Kind::Extrude { smart, .. } => smart.auto.filter(|(k, _)| *k == key).map(|(_, op)| op),
            _ => None,
        };
        match cached {
            Some(op) => op,
            None => {
                #[cfg(test)]
                AUTO_OPERATION_ASKED.with(|n| n.set(n.get() + 1));
                let op = solidcraft_engine::auto_operation(&app.session, &params);
                if let Kind::Extrude { smart, .. } = &mut d.kind {
                    smart.auto = Some((key, op));
                }
                op
            }
        }
    };
    let Some(fresh) = fresh.and_then(|op| OPS.iter().position(|o| *o == op)) else {
        return;
    };
    if let Kind::Extrude { operation, smart, .. } = &mut d.kind {
        *operation = crate::smart_extrude::operation(fresh, *operation, side, &mut smart.latch);
    }
}

/// What [`solidcraft_engine::auto_operation`] depends on besides its parameters: the design
/// (its revision counts every change, shown bodies too), which design it is, and the active
/// component the extrude goes into.
fn auto_operation_key(app: &SolveApp, params: &Value) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    params.to_string().hash(&mut h);
    app.session.revision.hash(&mut h);
    app.session.document_id.hash(&mut h);
    app.session.active_component.hash(&mut h);
    app.session.active_occurrence_path.hash(&mut h);
    h.finish()
}

#[cfg(test)]
thread_local! {
    static AUTO_OPERATION_ASKED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How often the extrude window asked the engine for its automatic operation on this thread.
#[cfg(test)]
pub(crate) fn auto_operation_asked() -> usize {
    AUTO_OPERATION_ASKED.with(std::cell::Cell::get)
}

/// The one-side direction a new extrude takes for its first pick: toward the side of a sketch
/// profile (or curve) the camera looks from; None for picks that keep their own way.
fn picked_side(app: &SolveApp, d: &Dialog, first: &Sel) -> Option<usize> {
    let normal = match first {
        Sel::Profile { sketch, .. } => crate::canvas::profile_plane(app, *sketch)?.normal(),
        Sel::SketchCurve { .. } => app.session.world_state().sketch(picks_sketch(app, std::slice::from_ref(first), None)?)?.plane.normal(),
        Sel::Face { .. } => return Some(0),
        _ => return None,
    };
    let (at, _) = crate::canvas::anchor(app, d)?;
    Some(crate::smart_extrude::picked_direction(crate::smart_extrude::view_ray(&app.cam, at), normal, false))
}

/// Discard an editor whose target was removed, before its preview can draw again. Show the sketch
/// of the Extrude, Revolve, sheet metal base or contour flange, or Hole at sketch
/// points the open dialog edits, and only while it is open: whichever way the dialog closed (OK,
/// Cancel, another tool, another design), the sketch returns to its own visibility.
pub(crate) fn sync_feature_edit(app: &mut SolveApp) {
    if app.dialog.as_ref().and_then(|d| d.editing).is_some_and(|(id, _)| app.session.doc.feature(id).is_none()) {
        // Undo or deletion already chose the current document and history. Cancel's rollback
        // would try to restore the obsolete editing session; just discard its work instead.
        app.dialog = None;
        app.applying = None;
        if let Some(state) = app.editing_state.take() {
            // Undo can save the editor's temporary rollback in Redo (deletion saves it in
            // Undo). Those exact snapshots belong to the complete design, with its old marker.
            for snapshot in app.session.undo.iter_mut().chain(&mut app.session.redo) {
                if std::sync::Arc::ptr_eq(&snapshot.doc, &state.source.doc) {
                    snapshot.doc = state.original.doc.clone();
                }
            }
        }
        app.preview.reset();
        app.op_check.abandon();
    }
    app.session.feature_edit_sketch = app.dialog.as_ref().and_then(|d| d.editing).and_then(|(id, _)| match &app.session.doc.feature(id)?.kind {
        FeatureKind::Extrude { sketch, .. }
        | FeatureKind::Revolve { sketch, .. }
        | FeatureKind::SheetBase { sketch, .. }
        | FeatureKind::SheetContour { sketch, .. }
        | FeatureKind::Hole { points: Some(solidcraft_engine::doc::SketchPoints { sketch, single: false, .. }), .. } => Some(**sketch),
        _ => None,
    });
}

/// Does this dialog dock as a card (at the view's edge or in the left column)? A delete
/// confirmation is a centred prompt instead.
pub(crate) fn docks(d: &Dialog) -> bool {
    !matches!(d.kind, Kind::ConfirmDelete { .. })
}

/// The width a dialog's card asks for: the managers' wider form, a study's table, or a card.
pub(crate) fn card_width(d: &Dialog) -> f32 {
    let t = Tokens::get();
    if matches!(d.kind, Kind::Appearance(_) | Kind::Material { .. }) {
        t.form_card_width + t.form_label_width + t.space_6
    } else if matches!(&d.kind, Kind::Motion(k) if k.wide()) {
        t.form_card_width * 3.0
    } else {
        t.form_card_width
    }
}

/// A dialog's primary and closing actions. Measure only closes; a deletion's primary action
/// says what it does.
pub(crate) fn action_labels(kind: &Kind) -> (&'static str, &'static str) {
    match kind {
        Kind::Measure { .. } => ("", "Close"),
        Kind::ConfirmDelete { .. } => ("Delete", "Cancel"),
        Kind::SaveMesh(k) => (crate::dialogs_save_mesh::button(k), "Cancel"),
        Kind::ViewDxf { .. } => ("Export DXF…", "Cancel"),
        _ => ("OK", "Cancel"),
    }
}

/// Whether OK and Cancel in the view's corner act on the dialog (#6709): every docked dialog;
/// a deletion asks in a modal card of its own, which the corner must not answer.
pub(crate) fn corner_confirms(d: &Dialog) -> bool {
    !matches!(d.kind, Kind::ConfirmDelete { .. })
}

/// Show the active dialog (if any), docked flush to an edge of the viewport just below the axis
/// view, sized to its content. The right edge is its place, except while a sketch is open: the
/// Sketch Palette keeps the right there, and the dialog takes the left so both stay on screen.
/// With Dialogs › In the left column it holds the left column instead (`dialog_column`).
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let escape_routed_here = !crate::viewport_status::enabled(app) || app.escape_owner.is_none_or(|owner| owner == crate::EscapeOwner::Dialog);
    // A click in the view's corner is for the dialog open this frame, or for none.
    let corner = app.viewport.corner.take();
    app.viewport.corner_ok = false;
    sync_feature_edit(app);
    app.op_check.poll();
    if app.tree.redefine.is_none() {
        app.viewport.dialog_rect = None;
    }
    let Some(mut d) = app.dialog.take() else {
        // The window a check was for closed: it stops (#6901).
        app.op_check.abandon();
        return;
    };
    // Edit Text lasts as long as its text: other design changes leave it open.
    if let Kind::Text(k) = &mut d.kind
        && !crate::sketch_text::refresh(app, k)
    {
        return;
    }
    crate::sketch_text::sync_path(&mut d);
    let _language = crate::i18n::scope(&app.preferences.language);
    if let Kind::TextMove(k) = &d.kind
        && !crate::sketch_text_move::valid(app, k)
    {
        return;
    }
    sync_reach(app, &mut d);
    auto_operation(app, &mut d);
    crate::op_auto::update(app, &mut d, ctx);
    d.sync_thread(&app.session);
    d.sync_material(&app.session);
    update_measure(app, &mut d);
    update_fill_cells(app, &mut d);
    // A refused OK's error is about the inputs it ran with (#641). Once a value (in the dialog,
    // or on the canvas, which the viewport applied earlier this frame), a pick or the design (a
    // parameter the inputs use) changes, it gives way to what the inputs make now: their own
    // error, or nothing and the live preview. `built`: whether they make commands, for OK below.
    let mut built = None;
    if d.error.is_some()
        && let Some(was) = &d.refused
    {
        let now = Refused::now(app, &d);
        built = Some(now.commands.is_ok());
        if now != *was {
            d.error = now.commands.as_ref().err().cloned();
            d.refused = d.error.is_some().then_some(now);
        }
    }
    // Physical materials: the library's, then the design's own (material.create).
    let t = Tokens::get();
    let vp = crate::agent_chat::tool_space(app, crate::shell::content_rect(app, app.viewport.rect.unwrap_or_else(|| ctx.content_rect())));
    // The Sketch Palette owns the right edge for as long as a sketch is being edited. A dialog
    // started there docks at the left instead, so neither panel hides the other.
    let column = crate::dialog_column::place(app, ctx, &d);
    let side = if column.is_some() || (crate::sketch_palette::editing(app) && crate::sketch_palette::fits_beside_a_dialog(app, ctx)) {
        crate::frame::Side::Left
    } else {
        crate::frame::Side::Right
    };
    let top = crate::shell::panel_top(app, vp, vp.bottom(), side);
    // Empty/hostile viewport geometry cannot provide a safe window rectangle.
    if !vp.is_finite() || !vp.is_positive() {
        app.dialog = Some(d);
        return;
    }
    // Command dialogs share the last dragged offset on this side until the application
    // exits, kept inside the viewport. In the left column a dialog stays in its place.
    let (drag_key, moved, docked, drag_bounds, anchor) = if let Some(column) = column {
        crate::scenario::publish_rect("dialog:column", column);
        ("", egui::Vec2::ZERO, column.min, crate::dialog_column::bounds(app, column, vp), column.min)
    } else {
        let drag_key = side.dialog_offset_key();
        let moved = crate::frame::dock_offset(ctx, drag_key);
        let docked = egui::pos2(side.anchor_x(vp), top);
        let drag_bounds = crate::shell::tool_drag_rect(app, vp);
        (drag_key, moved, docked, drag_bounds, crate::frame::offset_anchor(docked, moved, drag_bounds, side))
    };
    let mut keep = true;
    let mut ok = false;
    let mut applied = false;
    let mut apply_again = app.applying.is_some() && d.repeat_after_apply;
    let mut adopted = false;
    let mut cancel = false;
    let mut enter = false;
    let mut value_fields = Vec::new();
    let wide = matches!(&d.kind, Kind::Motion(k) if k.wide());

    let raw_title = title(&d.kind);
    let heading = if d.editing.is_some() { crate::i18n::format("dialog.edit_title", &[("title", tr(raw_title))]) } else { tr(raw_title).to_string() };
    let frame = crate::ui_kit::Panel::feature_frame(ctx, app.ui.layout, side);
    // OK is offered once the inputs make a feature.
    let has_ok = !matches!(d.kind, Kind::Sketch | Kind::Measure { .. });
    let valid = match &d.kind {
        Kind::Sketch => false,
        // Nothing picked is an answer too: OK empties the chat's tray.
        Kind::ChatContext => true,
        Kind::Material { editor, .. } => !editor.editing() && apply_commands(app, &d).is_ok(),
        Kind::SaveMesh(_) => crate::dialogs_save_mesh::ready(&d.inputs),
        Kind::SketchModify(crate::sketch_modify_ui::Mod::Corner { .. }) => {
            // Corner dialogs: preview errors are visual only; validation errors from
            // `apply_commands` go to `d.error`. OK stays enabled when the preview fails.
            !has_ok || (built.unwrap_or_else(|| apply_commands(app, &d).is_ok()))
        }
        _ => !has_ok || !d.previews() || (built.unwrap_or_else(|| apply_commands(app, &d).is_ok()) && app.preview.error.is_none()),
    };
    let (ok_label, close_label) = action_labels(&d.kind);
    // What the footer's OK takes, for the check in the view's corner too (#6709).
    let ok_enabled = valid && !ok_label.is_empty() && app.applying.is_none();
    let panel = crate::ui_kit::Panel::new(&heading, egui::Id::new(("sc_dialog", title(&d.kind))), anchor, drag_bounds, side, frame)
        .actions(ok_label, close_label);
    let panel = if let Some(column) = column {
        panel.width(column.width()).joined(crate::ui_kit::DockEdges { left: true, top: true, ..Default::default() })
    } else if wide && side == crate::frame::Side::Left {
        let available = app.viewport.palette_rect.map_or(vp.width(), |r| (r.left() - vp.left() - t.space_8).max(0.0));
        panel.width(card_width(&d).min(available))
    } else {
        panel.width(card_width(&d))
    };
    let reset_scroll = d.reset_scroll;
    let panel =
        panel.reset_scroll(reset_scroll && matches!(d.kind, Kind::Appearance(_) | Kind::Motion(crate::dialogs_motion::Mo::Configurations { .. })));
    let window = panel.window(ctx);
    let repeatable =
        matches!(&d.kind, Kind::SketchModify(k) if crate::sketch_modify_ui::repeatable(k)) && d.editing.is_none() && !d.extra.contains_key("pattern");
    if let Kind::ConfirmDelete { with, fail, .. } = &d.kind {
        let detail = with.join(" · ");
        let (closed, action) = crate::ui_kit::Confirm::new(egui::Id::new(("sc_dialog", title(&d.kind))), "delete", &heading)
            .tone(crate::ui_kit::Tone::Danger)
            .actions("Delete", "Cancel", None)
            .detail(&detail)
            .show(ctx, true, |ui| {
                if !with.is_empty() {
                    crate::ui_kit::text(ui, tr("Also deletes"), crate::ui_kit::Text::Report("also_deletes", false));
                }
                if !fail.is_empty() {
                    crate::ui_kit::text(ui, &format!("{}: {}", tr("Will fail"), fail.join(", ")), crate::ui_kit::Text::Report("will_fail", true));
                }
            });
        ok = action == crate::ui_kit::Action::Ok;
        cancel = closed || action == crate::ui_kit::Action::Cancel;
        app.viewport.dialog_rect = crate::scenario::rect_of("dialog:window");
    } else {
        let response = window.open(&mut keep).show(ctx, |ui| {
            let spec = dense::command(&d.kind).and_then(solidcraft_engine::find_command);
            let icon = spec.as_ref().map_or("", |s| s.icon);
            let label = spec.as_ref().map_or(heading.as_str(), |s| tr(s.label));
            let label = if d.editing.is_some() { heading.as_str() } else { label };
            let heading = crate::ui_kit::FeatureOptions { icon, title: label, drag_key, moved, docked, apply_label: repeatable.then_some("Apply") };
            let applying = app.applying.is_some();
            let mut collapsed = d.collapsed;
            let manager = matches!(
                d.kind,
                Kind::Material { .. }
                    | Kind::Appearance(_)
                    | Kind::Motion(crate::dialogs_motion::Mo::Configurations { .. })
                    | Kind::Sheet(crate::dialogs_sheet::Sm::Rules { .. })
                    | Kind::Plastic(crate::dialogs_plastic::Pl::Rules { .. })
            );
            // A study/configuration owns Enter for its cells, including when folded.
            let cell_enter = matches!(&d.kind, Kind::Motion(k) if k.wide()) || matches!(&d.kind, Kind::Material { editor, .. } if editor.editing());
            if cell_enter && collapsed {
                ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            }
            let body = |ui: &mut egui::Ui| {
                d.sync_fillet_chain(&app.session);
                crate::hole_presets::rows(ui, &mut d, &mut app.preferences.hole_presets, &app.session.doc, reset_scroll);
                crate::personal_sheet_rules::rows(ui, &mut d, &mut app.preferences.personal_sheet_rules, &app.session.doc, reset_scroll);
                let (accepted, fields) = crate::ui_kit::value_fields(ctx, || dense::rows(app, ctx, &mut d, ui));
                enter |= accepted;
                value_fields = fields;
                d.sync_fillet_chain(&app.session);
                if cell_enter {
                    ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                }
                let text = app.applying.as_ref().map(|job| {
                    let seconds = ((crate::now_ms() - job.started) / 1000.0).floor().max(0.0);
                    crate::i18n::format("dialog.computing", &[("seconds", &format!("{seconds:.0}"))])
                });
                crate::ui_kit::ComputeStatus { text, busy: app.applying.is_some() || app.preview.busy, applying: app.applying.is_some() }
            };
            let (closed, action) = if manager {
                panel.feature_collapsible(ui, heading, applying, valid, &mut collapsed, body)
            } else {
                panel.feature(ui, heading, applying, valid, body)
            };
            d.reset_scroll = false;
            d.collapsed = collapsed;
            cancel |= closed;
            if app.issue_report.draft.is_none() {
                match action {
                    crate::ui_kit::Action::Ok => {
                        ok = true;
                        apply_again = false;
                    }
                    crate::ui_kit::Action::Cancel => {
                        if (!app.esc_handled && escape_routed_here) || !ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                            cancel = true;
                        }
                    }
                    crate::ui_kit::Action::Apply => {
                        ok = true;
                        apply_again = true;
                    }
                    crate::ui_kit::Action::None => {}
                }
            }
        });
        app.viewport.dialog_rect = response.map(|r| r.response.rect);
        // OK and Cancel in the view's corner do what the dialog's own do (#6709).
        if app.issue_report.draft.is_none() {
            match corner {
                Some(crate::ui_kit::Action::Ok) if ok_enabled => {
                    ok = true;
                    apply_again = false;
                }
                Some(crate::ui_kit::Action::Cancel) => cancel = true,
                _ => {}
            }
        }
        app.viewport.corner_ok = ok_enabled;
    }
    if let Some(r) = app.viewport.dialog_rect {
        crate::scenario::publish_rect("dialog:outer", r);
        crate::scenario::publish_target("dialog:card", r);
    }
    if let Some(field) = crate::ui_kit::canvas_value_focus(ctx) {
        value_fields.push(field);
    }
    // Without value boxes in the view (#9479), the keyboard the window's opening or a pick gives
    // the box goes to the form's value field, once the form has drawn one.
    if d.focus && !crate::canvas::boxes_shown(ctx) && !value_fields.is_empty() {
        d.focus = false;
        d.restore_value_focus = true;
    }
    // The editor loses the keyboard while the click is still being handled, possibly in a later
    // pass of the same frame, so give it back once the pointer is idle again.
    let pointer_busy = ctx.input(|i| i.pointer.any_down() || i.pointer.any_pressed() || i.pointer.any_released());
    if d.restore_value_focus && !pointer_busy {
        d.restore_value_focus = false;
        let target = value_fields.iter().find(|(id, _)| Some(*id) == d.value_focus).or_else(|| value_fields.first());
        if let Some(&(id, length)) = target {
            d.focus = false;
            ctx.memory_mut(|memory| memory.request_focus(id));
            if let Some(mut state) = egui::text_edit::TextEditState::load(ctx, id) {
                state
                    .cursor
                    .set_char_range(Some(egui::text_selection::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(length))));
                state.store(ctx, id);
            }
        }
    }
    if d.restore_value_focus {
        ctx.request_repaint();
    } else {
        d.value_focus = ctx.memory(|memory| memory.focused()).filter(|id| value_fields.iter().any(|(field, _)| field == id));
    }
    // Each prompt draws only for its own dialog, so both run (`|`, not `||`).
    let preset_prompt = crate::hole_presets::name_prompt(ctx, &mut d, &mut app.preferences.hole_presets, &app.session.doc)
        | crate::personal_sheet_rules::name_prompt(ctx, &mut d, &mut app.preferences.personal_sheet_rules, &app.session.doc);
    if preset_prompt {
        enter = false;
        ok = false;
        cancel = false;
    }
    // Keyboard: Enter applies (from a value field, or with nothing focused), Esc cancels.
    let (enter_free, esc) = ctx.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
    let nothing_focused = ctx.memory(|m| m.focused().is_none());
    let canvas_enter = enter_free && ctx.memory(|m| m.had_focus_last_frame(egui::Id::new("sc_canvas_value")));
    // Tables (configurations, a study's keys) take Enter for their cells.
    let enter_applies = !matches!(d.kind, Kind::Sketch | Kind::Measure { .. } | Kind::ConfirmDelete { .. })
        && !matches!(&d.kind, Kind::Motion(k) if k.wide())
        && !matches!(&d.kind, Kind::Material { editor, .. } if editor.editing());
    let report_active = app.issue_report.draft.is_some();
    if !preset_prompt && !report_active && enter_applies && app.applying.is_none() && (enter || canvas_enter || (enter_free && nothing_focused)) {
        ok = true;
    }
    if !preset_prompt && !report_active && esc && !app.esc_handled && escape_routed_here {
        app.esc_handled = true;
        cancel = true;
    }
    // A plane picked for a new sketch starts it right away.
    let mut then_start = None;
    if matches!(d.kind, Kind::Sketch)
        && d.extra.remove("fixed_plane").and_then(|v| v.as_bool()).unwrap_or(false)
        && let Some(sel) = d.inputs.first().and_then(|i| i.items.first())
        && let Some(mut plane) = crate::sketch_attachment_ui::face_plane(app, sel)
    {
        plane["fixed"] = json!(true);
        let before = app.settled_cam();
        let mut params = json!({"plane":plane});
        if app.preferences.sketch_face_activates_part {
            params["activate_part"] = json!(true);
        }
        match app.run("sketch.create", params) {
            Ok(_) => {
                app.look_at_opened_sketch(before);
                cancel = true;
                then_start = d.then_start;
            }
            Err(error) => d.refuse(app, error),
        }
    }
    if matches!(d.kind, Kind::Sketch)
        && !cancel
        && d.error.is_none()
        && let Some(sel) = d.inputs.first().and_then(|i| i.items.first()).cloned()
    {
        match start_sketch(app, &sel) {
            Ok(()) => {
                cancel = true;
                then_start = d.then_start;
            }
            Err(error) => d.refuse(app, error),
        }
    }
    cancel |= !keep;
    // A window still choosing its operation applies once its check is in (#6901).
    let ok = crate::op_auto::hold_ok(&mut d, ok, &mut apply_again, cancel);
    if ok && !cancel && app.applying.is_none() {
        d.repeat_after_apply = apply_again;
        // Edit Text applies its own way: its live changes and OK are one undo step.
        let run = match &mut d.kind {
            Kind::Text(k) => Some(crate::sketch_text::apply(app, k)),
            Kind::ChatContext => Some(crate::agent_chat::pick_done(app, &d)),
            _ => match begin_applying(app, &d) {
                // Computing: the dialog stays, saying so, until it is in.
                Some(job) => {
                    app.applying = Some(job);
                    None
                }
                None => Some(run_primed(app, &d)),
            },
        };
        match run {
            Some(Ok(())) => {
                applied = true;
                cancel = !apply_again;
                adopted = d.previews();
            }
            Some(Err(e)) => d.refuse(app, e),
            None => {}
        }
    }
    // Adopt the complete transaction. Never rerun a Boolean on the window thread.
    let finished = if cancel { None } else { app.applying.as_mut().and_then(Applying::finished) };
    // Preferences received after this frame's renderer setup take effect next frame.
    // Keep the result ready until the viewport will consume the prepared quality.
    let finished = if finished.is_some() && solidcraft_engine::doc::display_quality::current() != display_quality(app) {
        if let Some(job) = app.applying.as_mut() {
            job.ready = finished;
        }
        ctx.request_repaint();
        None
    } else {
        finished
    };
    if let Some(result) = finished {
        let commands = app.applying.as_ref().map(|job| job.cmds.clone());
        let quality = app.applying.as_ref().map(|job| job.quality);
        let started = app.applying.as_ref().map_or_else(crate::now_ms, |job| job.started);
        app.applying = None;
        let current = apply_commands(app, &d).ok();
        let source_current = result.as_ref().is_ok_and(|prepared| prepared.is_current(&app.session));
        if current.is_some() && current != commands && source_current {
            app.applying = begin_applying(app, &d);
        } else if current != commands && source_current {
            d.refuse(app, "the feature inputs changed while computing".into());
        } else if source_current && quality.is_some_and(|quality| quality != display_quality(app)) {
            // A different display quality needs new plans, never another Boolean evaluation.
            match result {
                Ok(prepared) => {
                    app.applying = warm_applying(app, prepared, current.unwrap_or_default(), started);
                    if app.applying.is_none() {
                        d.refuse(app, "the display quality changed while computing".into());
                    }
                }
                Err(error) => d.refuse(app, error),
            }
        } else {
            let run = result.and_then(|prepared| adopt_dialog(app, prepared));
            match run {
                Ok(()) => {
                    cancel = !apply_again;
                    applied = true;
                    adopted = true;
                }
                Err(error) => d.refuse(app, error),
            }
        }
    }
    if applied && apply_again {
        for input in &mut d.inputs {
            input.items.clear();
        }
        d.active = 0;
        d.error = None;
        d.refused = None;
        d.repeat_after_apply = false;
        let _ = app.session.execute("select.clear", &json!({}));
    }
    // Cancel while computing: the result goes (its worker runs on to its end).
    if cancel && !applied {
        app.applying = None;
    }
    if !keep || cancel {
        app.session.feature_edit_sketch = None;
        crate::dialogs_motion::closed(app, &mut d, applied);
        if let Kind::Text(k) = &mut d.kind {
            crate::sketch_text::closed(app, k, applied);
        }
    }
    // The edited text's outlines are new curves: the old selection would name stale ids.
    if applied && matches!(d.kind, Kind::Text(_) | Kind::TextMove(_)) {
        let _ = app.session.execute("select.clear", &json!({}));
    }
    if keep && !cancel {
        // The canvas follows Edit Text's fields.
        if let Kind::Text(k) = &mut d.kind
            && let Some(result) = crate::sketch_text::preview(app, k)
        {
            d.error = result.err();
        }
        app.dialog = Some(d);
    } else if let Some((_, marker)) = d.editing {
        if applied {
            app.editing_state = None;
            if !adopted && app.session.doc.marker != marker {
                let _ = roll_marker(app, marker);
            }
        } else {
            restore_editing(app, marker);
        }
    }
    // The shortcut that asked for this sketch starts its tool in it.
    if let Some(id) = then_start {
        app.start(id);
    }
}

/// Close the dialog without applying it (Esc, Cancel). An edit puts the timeline marker back.
pub fn cancel(app: &mut SolveApp) {
    app.session.feature_edit_sketch = None;
    if let Some(pending) = &mut app.pending_feature_edit {
        pending.cancel();
    }
    app.applying = None;
    if let Some(mut d) = app.dialog.take() {
        crate::dialogs_motion::closed(app, &mut d, false);
        if let Kind::Text(k) = &mut d.kind {
            crate::sketch_text::closed(app, k, false);
        }
        app.dialog = Some(d);
    }
    if let Some(d) = app.dialog.take()
        && let Some((_, marker)) = d.editing
    {
        restore_editing(app, marker);
    }
    app.editing_state = None;
}

pub(crate) fn restore_editing(app: &mut SolveApp, marker: Option<usize>) {
    if let Some(state) = app.editing_state.take() {
        if let Err(error) = app.session.restore_editor(&state.source, state.original) {
            app.set_status(error.to_string(), true);
        }
    } else {
        let _ = roll_marker(app, marker);
    }
}

/// Temporary rollback used by an editor is view state. Preserve both history stacks,
/// including redo and a history already at the undo limit.
pub(crate) fn roll_marker(app: &mut SolveApp, marker: Option<usize>) -> Result<(), String> {
    let undo = std::mem::take(&mut app.session.undo);
    let redo = std::mem::take(&mut app.session.redo);
    let selection = std::mem::take(&mut app.session.selection);
    let result = app.run("timeline.roll_to", marker.map(|m| json!({"position":m})).unwrap_or_else(|| json!({})));
    app.session.undo = undo;
    app.session.redo = redo;
    app.session.selection = selection;
    result.map(|_| ())
}

/// The pick input an extent of To Object adds (a face, vertex or point to go to).
const TO_OBJECT: &str = "To Object";
const FROM_OBJECT: &str = "From Object";

fn object_on(inputs: &[SelInput], label: &str) -> bool {
    inputs.iter().any(|i| i.label == label)
}

fn to_object_on(inputs: &[SelInput]) -> bool {
    inputs.iter().any(|i| i.label == TO_OBJECT)
}

/// Add or remove the To Object input; the active input moves to it when it is added.
fn set_to_object(inputs: &mut Vec<SelInput>, active: &mut usize, on: bool) {
    set_object(inputs, active, TO_OBJECT, on);
}

/// A personal preset remembers the extent mode, never its destination geometry.
pub(crate) fn preset_to_object(d: &mut Dialog, on: bool) {
    set_to_object(&mut d.inputs, &mut d.active, on);
    if let Some(input) = d.inputs.iter_mut().find(|input| input.label == TO_OBJECT) {
        input.items.clear();
    }
}

fn set_object(inputs: &mut Vec<SelInput>, active: &mut usize, label: &'static str, on: bool) {
    add_object(inputs, active, SelInput::new(label, FACES | selection::VERTICES, false), on);
}

/// [`set_object`] for an input the form draws itself, under the choice that adds it
/// ([`SelInput::under_choice`]): an extrude's To Object under Extent Type, its From Object under Start.
pub(crate) fn set_object_input(inputs: &mut Vec<SelInput>, active: &mut usize, label: &'static str, on: bool) {
    add_object(inputs, active, SelInput::new(label, FACES | selection::VERTICES, false).under_choice(), on);
}

fn add_object(inputs: &mut Vec<SelInput>, active: &mut usize, input: SelInput, on: bool) {
    let label = input.label;
    if on && !object_on(inputs, label) {
        inputs.push(input);
        *active = inputs.len() - 1;
    } else if !on && object_on(inputs, label) {
        inputs.retain(|i| i.label != label);
        *active = (*active).min(inputs.len().saturating_sub(1));
    }
}

/// The plane an extrude's picked profiles lie in: the first pick's sketch plane, or its planar face.
fn profile_plane(s: &Session, first: &Sel) -> Option<solidcraft_engine::geom::Plane> {
    let st = s.world_state();
    match first {
        Sel::Profile { sketch, .. } => st.sketch(*sketch).map(|ss| ss.plane),
        Sel::Face { body, index, .. } => st.body(body)?.body.planar_face(*index),
        _ => None,
    }
}

/// Is a pick an object to extrude to from profiles in `reference` (not one more profile)? A face
/// is unless it is planar and in that plane; a vertex or a place is unless it lies in it.
fn object_target(s: &Session, reference: &solidcraft_engine::geom::Plane, sel: &Sel) -> bool {
    const TOLERANCE: f64 = 1e-6;
    match sel {
        Sel::Face { body, index, .. } => {
            let st = s.world_state();
            let Some(b) = st.body(body) else { return false };
            b.body
                .planar_face(*index)
                .is_none_or(|face| reference.normal().dot(face.normal()).abs() < 1.0 - 1e-9 || reference.height(face.origin).abs() > TOLERANCE)
        }
        Sel::Vertex { point, .. } => reference.height(*point).abs() > TOLERANCE,
        _ => false,
    }
}

/// The picked To Object place.
fn to_object_point(inputs: &[SelInput]) -> Option<Value> {
    object_point(inputs, TO_OBJECT)
}

pub(crate) fn object_point(inputs: &[SelInput], label: &str) -> Option<Value> {
    inputs.iter().find(|i| i.label == label)?.items.first().and_then(|x| match x {
        Sel::Face { point, .. } | Sel::Vertex { point, .. } | Sel::Edge { point, .. } => Some(pt(*point)),
        _ => None,
    })
}

/// A To Object input filled from a stored place (editing).
fn with_to_object(d: &mut Dialog, s: &Session, to: Option<Vec3>) {
    with_object(d, s, TO_OBJECT, to);
}

fn with_object(d: &mut Dialog, s: &Session, label: &'static str, to: Option<Vec3>) {
    if let Some(p) = to {
        set_object(&mut d.inputs, &mut d.active, label, true);
        if let Some(inp) = d.inputs.iter_mut().find(|i| i.label == label) {
            inp.items = vec![face_sel(s, p).unwrap_or(Sel::Vertex { body: String::new(), point: p })];
        }
    }
}

/// A choice row stored in the dialog's extra parameters as `key: value` (the first choice, the
/// command's default, is stored as no key at all).
fn extra_choice(ui: &mut egui::Ui, extra: &mut serde_json::Map<String, Value>, id: &str, key: &str, choices: &[(&str, &str)]) -> usize {
    let cur = extra.get(key).and_then(Value::as_str).and_then(|v| choices.iter().position(|(_, x)| *x == v)).unwrap_or(0);
    let mut i = cur;
    let labels: Vec<&str> = choices.iter().map(|(l, _)| *l).collect();
    combo(ui, id, &labels, &mut i);
    if i != cur {
        match choices.get(i) {
            Some((_, v)) if i > 0 => {
                extra.insert(key.into(), json!(v));
            }
            _ => {
                extra.remove(key);
            }
        }
    }
    i
}

/// A checkbox stored in the dialog's extra parameters.
fn extra_check(ui: &mut egui::Ui, extra: &mut serde_json::Map<String, Value>, key: &str) {
    let mut on = extra.get(key).and_then(Value::as_bool).unwrap_or(false);
    let r = crate::ui_kit::checkbox(ui, "", &mut on);
    if r.changed() {
        extra.insert(key.into(), json!(on));
    }
    // Keep both the checkbox's accessibility label and its existing scenario handle.
    publish_check(ui, &r, on);
}

/// A value field stored in the dialog's extra parameters (`default` until edited).
fn extra_field(ui: &mut egui::Ui, extra: &mut serde_json::Map<String, Value>, key: &str, default: &str) -> bool {
    let mut v = extra.get(key).and_then(Value::as_str).unwrap_or(default).to_string();
    let enter = field(ui, &mut v);
    extra.insert(key.into(), json!(v));
    enter
}

/// A spline fillet's control radii after the start radius, as expressions. A dialog without a
/// spline yet gets a cubic one of two spans, all its control radii `radius`.
pub(crate) fn spline_radii(extra: &mut serde_json::Map<String, Value>, radius: &str) -> Vec<String> {
    let radii: Vec<String> = extra
        .get("radii")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|r| match r {
            Value::String(x) => Some(x.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .collect();
    let knots = extra.get("knots").and_then(Value::as_array).map_or(0, Vec::len);
    // All control radii (the start radius and these) plus the degree (1…3) plus one.
    if !radii.is_empty() && (radii.len() + 3..=radii.len() + 5).contains(&knots) {
        return radii;
    }
    extra.insert("knots".into(), json!([0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0]));
    vec![radius.to_string(); 4]
}

/// A variable fillet's middle radii as (position, radius expression).
fn mid_points(extra: &serde_json::Map<String, Value>) -> Vec<(f64, String)> {
    extra
        .get("mid")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let t = m.get(0)?.as_f64()?;
            let r = match m.get(1)? {
                Value::String(x) => x.clone(),
                Value::Number(n) => n.to_string(),
                _ => return None,
            };
            Some((t, r))
        })
        .collect()
}

pub(crate) const CHAMFER_TYPES: [&str; 3] = ["Equal Distance", "Two Distances", "Distance and Angle"];

/// A new Fillet or Chamfer dialog's values.
fn fillet_kind(size: &str, chamfer: bool) -> Kind {
    Kind::Fillet { radius: size.into(), chamfer, chain: true, ctype: 0, distance2: size.into(), angle: "45 deg".into(), flip: false }
}

/// A length field's first text: `number` in the design's units, with the unit written out so the
/// field shows it as the other dialogs show theirs ("20 mm", "20 in"). OK makes the size the bare
/// number would.
pub(crate) fn length_default(s: &Session, number: &str) -> String {
    crate::length_input::default(&s.doc, number)
}

/// A feature's length as its edit field shows it. A bare number is kept in millimetres: where the
/// design reads bare numbers in another unit, the field says "mm" (as the configuration table
/// does), so OK keeps the size. Anything else shows as stored.
pub(crate) fn stored_length(s: &Session, e: &str) -> String {
    if s.doc.with_design_unit(e, ValueKind::Length) == e { e.to_string() } else { format!("{} mm", e.trim()) }
}

/// A stored value as its edit field shows it (#1936): a bare number is written with the unit the
/// engine reads it in, "2" as "2 mm" for a length and "5" as "5 deg" for an angle, so the field
/// and the on-canvas tag name the unit as the other dialogs do. The number keeps its stored text.
/// Counts, units already written, parameter names, expressions and empty text show as stored.
pub(crate) fn shown_expr(e: &str, kind: ValueKind) -> String {
    let unit = match kind {
        ValueKind::Length => "mm",
        ValueKind::Angle => "deg",
        _ => return e.to_string(),
    };
    if solidcraft_engine::doc::expr::is_bare_number(e) { format!("{} {unit}", e.trim()) } else { e.to_string() }
}

fn hole_defaults(s: &Session) -> Kind {
    Kind::Hole {
        diameter: length_default(s, "5"),
        depth: length_default(s, "10"),
        kind: 0,
        cb_diameter: length_default(s, "9"),
        cb_depth: length_default(s, "3"),
        cs_diameter: length_default(s, "10"),
        cs_angle: "90 deg".into(),
        opts: HoleOpts::default(),
    }
}

/// Start a sketch on a picked plane or planar face and turn the view to face it.
pub fn start_sketch(app: &mut SolveApp, sel: &Sel) -> Result<(), String> {
    let mut params = match sel {
        Sel::Plane { name } => json!({ "plane": shown_origin_plane(app, name).unwrap_or_else(|| json!(name)) }),
        Sel::Face { .. } => json!({
            "plane": crate::sketch_attachment_ui::face_plane(app, sel).ok_or("Selected face placement no longer exists")?,
            "activate_part": app.preferences.sketch_face_activates_part,
        }),
        _ => return Err("Select an origin plane, construction plane or planar body face".into()),
    };
    if let Sel::Plane { name } = sel
        && let Some(flip) = flip_toward_viewer(app, name)
    {
        params["flip"] = json!(flip);
    }
    let before = app.settled_cam();
    app.run("sketch.create", params).map_err(|error| crate::friendly_error("sketch.create", &error))?;
    let turned = app.look_at_opened_sketch(before);
    // On a face: look at the picked point, not the plane's origin (a corner of the face),
    // which left the face filling the view off-centre like a big square.
    if turned
        && let Sel::Face { point, .. } = sel
        && let Some(mut to) = app.cam_anim.as_ref().map(|a| a.to)
    {
        to.target = *point;
        app.animate_to(to);
    }
    Ok(())
}

/// A new sketch on origin or construction plane `name` faces the viewer: when the camera is
/// behind the plane's normal, the in-plane axis to turn the plane about (`"x"` or `"y"`), chosen
/// so the sketch's y axis stays as upright on screen as it can (x then runs to the right).
fn flip_toward_viewer(app: &SolveApp, name: &str) -> Option<&'static str> {
    let plane = plane_by_name(&app.session, name)?;
    let n = plane.normal();
    // Where the eye is: in perspective its position, in orthographic only its direction counts.
    let side = if app.cam.fov > 0.0 { (app.cam.eye() - plane.origin).dot(n) } else { app.cam.basis().2.dot(n) };
    if side >= -1e-9 {
        return None;
    }
    let up = app.cam.up.dir();
    Some(if plane.y.dot(up) > 1e-9 { "y" } else { "x" })
}

/// The origin plane `name` as the viewport and the browser show it and the user picks it: the
/// design's, in the world, written out for `sketch.create`, which puts it into the active
/// component's frame once. By name it would be the active component's own plane, and once that
/// component has moved (a stood-up import is turned a quarter about X) its XY is the world's XZ:
/// the sketch, the view turning to it and the extrude all went the wrong way. `None` while the
/// active component sits at the origin (the name says the same) or for a construction plane.
fn shown_origin_plane(app: &SolveApp, name: &str) -> Option<Value> {
    let p = solidcraft_engine::geom::Plane::named(name)?;
    if app.session.doc.find_feature(name).is_some() || solidcraft_engine::doc::is_identity(&app.session.active_component_frame()) {
        return None;
    }
    Some(json!({"origin": [0.0, 0.0, 0.0], "x_dir": pt(p.x), "y_dir": pt(p.y)}))
}

/// The world plane of an origin plane or a construction plane by name, where it is shown.
pub(crate) fn plane_by_name(s: &Session, name: &str) -> Option<solidcraft_engine::geom::Plane> {
    if let Some(p) = solidcraft_engine::geom::Plane::named(name) {
        return Some(p);
    }
    let st = s.world_state();
    st.construct
        .iter()
        .find(|c| c.name == name)
        .and_then(|c| match &c.geom {
            solidcraft_engine::doc::construct::ConstructGeom::Plane(pl) => Some(*pl),
            _ => None,
        })
        // With occurrences the world state leaves construction geometry out; the viewport's
        // planes are evaluated in place.
        .or_else(|| solidcraft_engine::view::construction_planes(s).into_iter().find(|(_, n, _)| n == name).map(|(_, _, pl)| pl))
}

/// Animate the camera to look straight at the active sketch plane.
pub fn look_at_sketch(app: &mut SolveApp) {
    look_at_sketch_from(app, false);
}

/// Look At asked for by the user (Ctrl+8, the Look At buttons): like [`look_at_sketch`], and
/// from the sketch's other side when the view already faces it (#1886).
pub fn look_at_sketch_again(app: &mut SolveApp) {
    look_at_sketch_from(app, true);
}

fn look_at_sketch_from(app: &mut SolveApp, again: bool) {
    let st = app.session.world_state();
    if let Some(ss) = app.session.active_sketch.and_then(|id| st.sketch(id)) {
        let n = facing(app, ss.plane.normal(), again);
        let to = square_sketch_axes(app.cam.looking_from(n), ss.plane.x, ss.plane.y);
        app.animate_to(onto_sketch(to, ss));
    }
}

/// Orient the camera so the sketch's x axis runs right and its y axis is as upright as it can
/// be (#9029). Facing the plane alone left a sketch on a turned component tilted on screen.
/// Along world up/down, yaw turns the image (roll stays 0, as Top/Bottom views do); elsewhere
/// roll squares the axes.
pub(crate) fn square_sketch_axes(mut cam: solidcraft_engine::render::Camera, sketch_x: Vec3, sketch_y: Vec3) -> solidcraft_engine::render::Camera {
    let b = cam.back();
    let in_view = |v: Vec3| v - b * v.dot(b);
    let Some(mut x) = in_view(sketch_x).normalized() else {
        return cam;
    };
    // Prefer the sense of x that keeps sketch y pointing up on screen.
    if in_view(sketch_y).dot(b.cross(x)) < 0.0 {
        x = -x;
    }
    if b.dot(cam.up_dir()).abs() > 0.999 {
        // Looking straight up or down: yaw alone turns the image about the view.
        let framed = cam.up.to_frame(x);
        let yaw = if b.dot(cam.up_dir()) > 0.0 { (-framed.y).atan2(framed.x) } else { framed.y.atan2(framed.x) };
        if yaw.is_finite() {
            cam.yaw = yaw;
            cam.roll = 0.0;
        }
        return cam;
    }
    let (right, up, _) = cam.basis();
    let roll = x.dot(up).atan2(x.dot(right));
    if roll.is_finite() {
        cam.roll = roll;
    }
    cam
}

/// `to` (already facing the sketch) moved onto sketch `ss` (#3198): centred on its geometry, and
/// zoomed out when that would not fit in the view; it never zooms in, so a zoom the user chose
/// stays. A sketch without geometry keeps its plane's origin. Centring on the origin alone left a
/// sketch drawn away from it (a part placed far from the origin, as imports often are) out of view.
pub(crate) fn onto_sketch(mut to: solidcraft_engine::render::Camera, ss: &solidcraft_engine::doc::SolvedSketch) -> solidcraft_engine::render::Camera {
    let Some((lo, hi)) = ss.sketch.bounds() else {
        to.target = ss.plane.origin;
        return to;
    };
    let mut b = Aabb3::default();
    for p in [lo, hi, solidcraft_engine::geom::Vec2::new(lo.x, hi.y), solidcraft_engine::geom::Vec2::new(hi.x, lo.y)] {
        b.add(ss.plane.to_world(p));
    }
    let keep = to.distance;
    to.fit(&b);
    if keep.is_finite() && keep > to.distance {
        to.distance = keep;
    }
    to
}

/// The side to look from at a plane with normal `n`: its own side, or with `again` the other
/// side when the view already looks from `n`, so a second Look At turns the view round (#1886).
pub(crate) fn facing(app: &SolveApp, n: Vec3, again: bool) -> Vec3 {
    if again && app.cam.back().dot(n) > 0.999 { -n } else { n }
}

fn sels(d: &Dialog, i: usize) -> &[Sel] {
    selection::items(&d.inputs, i)
}

/// Section Analysis with a section on and no plane picked: OK turns the section off.
fn section_turns_off(s: &Session, inputs: &[selection::SelInput]) -> bool {
    s.section.is_some() && selection::items(inputs, 0).is_empty()
}

/// Run the dialog's commands on the window thread, taking the Join its operation check already
/// evaluated for them (#6901).
fn run_primed(app: &mut SolveApp, d: &Dialog) -> Result<(), String> {
    let joined = apply_commands(app, d).ok().and_then(|cmds| app.op_check.joined_for(&cmds, app.session.revision));
    if let Some(joined) = &joined {
        app.session.prime(joined);
    }
    let r = run_dialog(app, d);
    app.session.unprime();
    r
}

pub(crate) fn run_dialog(app: &mut SolveApp, d: &Dialog) -> Result<(), String> {
    if let Kind::ViewDxf { dashed, tolerance } = &d.kind {
        return crate::dialogs_view_dxf::run(app, *dashed, *tolerance);
    }
    // Saving asks for a file name first; `Err("")` is the user backing out of that.
    if let Kind::SaveMesh(k) = &d.kind {
        return crate::dialogs_save_mesh::run(app, k, &d.inputs);
    }
    if d.previews() {
        let prepared = DialogComputation::new(app, d)?.compute()?;
        return adopt_dialog(app, prepared);
    }
    let commands = apply_commands(app, d)?;
    if let Some((_, marker)) = d.editing {
        // Record the actual pre-edit timeline state in the command's undo snapshot.
        roll_marker(app, marker)?;
    }
    for (cmd, params) in commands {
        match app.run(&cmd, params) {
            Ok(reply) => {
                // A drive that stopped short says where, and why.
                if cmd == "joint.drive"
                    && let Some(why) = crate::dialogs_assembly::drive_stop(&reply)
                {
                    app.set_status(why, false);
                }
            }
            Err(error) => {
                if let Some((id, _)) = d.editing {
                    let position = app.session.doc.feature_index(id);
                    let _ = roll_marker(app, position);
                }
                return Err(error);
            }
        }
    }
    if let Kind::Section { offset, flip, hatch } = &d.kind
        && let Some(cut) = app.session.section
    {
        app.tree.section_dialog = if d.extra.contains_key("plane") && sels(d, 0).is_empty() {
            None // Programmatic geometry is reconstructed from the actual session, never cached.
        } else {
            Some(SectionDraft { offset: offset.clone(), flip: *flip, hatch: *hatch, plane: sels(d, 0).to_vec(), cut })
        };
    }
    Ok(())
}

/// Edit Section Analysis: its dialog, where the dialog that made the section shown (or hidden)
/// now left it. Programmatic cuts use their actual geometry in the same existing form.
pub fn edit_section(app: &mut SolveApp) {
    app.start_section_edit();
    let cut = app.session.section.or(app.tree.hidden_section);
    let last = app.tree.section_dialog.clone().filter(|last| cut.is_some_and(|c| same_cut(c, last.cut)));
    if app.preferences.viewport_section_status {
        if let Some((origin, normal)) = cut {
            let o = [origin.x, origin.y, origin.z];
            let n = [normal.x, normal.y, normal.z];
            if let Some((_, distance, canonical)) = crate::viewport_status::section_coordinates(o, n) {
                let flip = n.iter().zip(canonical).map(|(a, b)| a * b).sum::<f64>() < 0.0;
                let offset = format!("{distance} mm");
                let hatch = app.session.section_hatch;
                if let Some(d) = app.dialog.as_mut().filter(|d| matches!(d.kind, Kind::Section { .. })) {
                    d.kind = Kind::Section { offset, flip, hatch };
                    if let Some(input) = d.inputs.first_mut() {
                        input.items.clear();
                    }
                    let base = o.into_iter().zip(canonical).map(|(o, n)| o - n * distance).collect::<Vec<_>>();
                    d.extra.insert("plane".into(), json!({"origin":base,"normal":canonical}));
                }
            }
        }
        return;
    }
    let Some(last) = last else { return };
    if let Some(d) = app.dialog.as_mut().filter(|d| matches!(d.kind, Kind::Section { .. })) {
        d.kind = Kind::Section { offset: last.offset, flip: last.flip, hatch: last.hatch };
        if let Some(plane) = d.inputs.first_mut() {
            plane.items = last.plane;
        }
    }
}

/// What the Section Analysis dialog's OK made: its distance, flip and plane pick, and the cut
/// (origin, normal) they made, so Edit reopens the dialog where it was while that cut stands.
#[derive(Clone, Debug)]
pub struct SectionDraft {
    pub offset: String,
    pub flip: bool,
    pub hatch: bool,
    pub plane: Vec<Sel>,
    pub cut: (Vec3, Vec3),
}

/// The same cut, up to rounding (hiding and showing a section runs it again from its plane).
fn same_cut(a: (Vec3, Vec3), b: (Vec3, Vec3)) -> bool {
    let tol = 1e-9 * (1.0 + a.0.len().max(b.0.len()));
    (a.0 - b.0).len() <= tol && (a.1 - b.1).len() <= 1e-9
}

/// Placement used to edit a definition in the selected authoring occurrence.
pub(crate) fn feature_frame(s: &Session, component: u64) -> solidcraft_engine::doc::Mat {
    if component == s.active_component { s.active_component_frame() } else { s.placed_document().component_transform(component) }
}

/// The feature dialog's current operation name for a reviewed issue draft.
pub(crate) fn report_title(d: &Dialog) -> String {
    title(&d.kind).to_string()
}

/// The commands OK runs (an edit rebuilds its feature in place); the live preview runs the same.
pub fn apply_commands(app: &SolveApp, d: &Dialog) -> Result<Vec<(String, Value)>, String> {
    let mut normalized = d.clone();
    normalized.sync_material(&app.session);
    for value in crate::length_input::fields(&app.session.doc, &mut normalized.kind) {
        *value = crate::length_input::expression(&app.session.doc, value, ValueKind::Length);
    }
    crate::length_input::extra(&mut normalized.extra, |e| crate::length_input::expression(&app.session.doc, e, ValueKind::Length));
    let mut cmds = dialog_commands(app, &normalized)?;
    exact_regions(d, &mut cmds);
    crate::op_auto::scope_targets(app, d, &mut cmds)?;
    if let Some((id, _)) = d.editing {
        let (cmd, mut params) = cmds.into_iter().next().ok_or("nothing to apply")?;
        if let Some(feature) = app.session.doc.feature(id) {
            let original_all = match &feature.kind {
                FeatureKind::Extrude { sketch, profiles: ProfileSel::All, thin: None, .. }
                | FeatureKind::Revolve { sketch, profiles: ProfileSel::All, .. }
                | FeatureKind::Emboss { sketch, profiles: ProfileSel::All, .. } => Some(*sketch),
                _ => None,
            };
            if let Some(sketch) = original_all
                && !d.profile_selection_changed
                && let Some(ss) = app.session.world_state().sketch(sketch.0)
            {
                let mut selected: Vec<usize> = sels(d, 0)
                    .iter()
                    .filter_map(|item| match item {
                        Sel::Profile { sketch: id, index } if *id == sketch => Some(*index),
                        _ => None,
                    })
                    .collect();
                selected.sort_unstable();
                selected.dedup();
                if selected == (0..ss.profiles.len()).collect::<Vec<_>>() && sels(d, 0).len() == selected.len() {
                    params["profiles"] = json!("all");
                }
            }
        }
        return Ok(vec![("timeline.redefine".into(), json!({"feature": id, "command": cmd, "params": params}))]);
    }
    Ok(cmds)
}

/// A dialog highlights exactly the profile regions it picks, so its commands build exactly those:
/// a region cut off by the projected outline of the face a sketch is on stays the one region
/// clicked (#6724). Without `exact_regions` a command grows such a pick to the whole drawn profile
/// (#5302), which the highlight and the count would not show.
fn exact_regions(d: &Dialog, cmds: &mut [(String, Value)]) {
    if !d.inputs.iter().any(|input| input.items.iter().any(|item| matches!(item, Sel::Profile { .. }))) {
        return;
    }
    for (_, params) in cmds {
        if let Some(map) = params.as_object_mut()
            && (map.contains_key("profiles") || map.contains_key("sections"))
        {
            map.insert("exact_regions".into(), json!(true));
        }
    }
}

/// The commands the live preview runs: OK's, with a joint's motion while it animates.
pub fn preview_commands(app: &SolveApp, d: &Dialog) -> Result<Vec<(String, Value)>, String> {
    let mut cmds = apply_commands(app, d)?;
    if let Kind::Assembly(k) = &d.kind {
        crate::dialogs_assembly::animate(app, k, &mut cmds);
    }
    Ok(cmds)
}

/// A wrapped Emboss edit keeps its profile guide in the saved author occurrence.
/// An invalid saved context must not draw the guide at a different occurrence.
pub(crate) fn emboss_profile_plane(app: &SolveApp, sketch: u64) -> Result<Option<solidcraft_engine::geom::Plane>, String> {
    let Some(dialog) = app.dialog.as_ref().filter(|d| matches!(d.kind, Kind::Emboss { .. })) else { return Ok(None) };
    let Some((feature, _)) = dialog.editing else { return Ok(None) };
    let Some(feature) = app.session.doc.feature(feature) else { return Err("the edited feature is missing".into()) };
    let FeatureKind::Emboss { sketch: source, face: Some(face), .. } = &feature.kind else { return Ok(None) };
    if *source != sketch {
        return Ok(None);
    }
    let model = app.session.model.state();
    let sketch = model.sketch(sketch).ok_or("the Emboss sketch is missing")?;
    let body = model.body(&face.body).ok_or("the Emboss target body is missing")?;
    let owner = app.session.doc.feature(sketch.feature).map(|feature| feature.component).unwrap_or(0);
    let plane = sketch.plane;
    let frame = face.sketch_frame(&app.session.doc, feature.component, owner, body).map_err(|error| error.to_string())?;
    solidcraft_engine::geom::Plane::new(
        solidcraft_engine::doc::apply_point(&frame, plane.origin),
        solidcraft_engine::doc::apply_vector(&frame, plane.x),
        solidcraft_engine::doc::apply_vector(&frame, plane.y),
    )
    .map(Some)
    .ok_or_else(|| "the Emboss author frame is degenerate".into())
}

/// The commands (id, parameters) a dialog's OK runs.
fn dialog_commands(app: &SolveApp, d: &Dialog) -> Result<Vec<(String, Value)>, String> {
    let s = &app.session;
    let need = |i: usize, what: &str| -> Result<(), String> { if sels(d, i).is_empty() { Err(format!("select {what} first")) } else { Ok(()) } };
    let profiles = || -> (Value, Value) {
        let ps: Vec<(u64, usize)> = sels(d, 0).profiles();
        (ps.first().map(|x| json!(x.0)).unwrap_or(Value::Null), json!(ps.iter().map(|x| x.1).collect::<Vec<_>>()))
    };
    let face_points = |i: usize| -> Vec<Value> { sels(d, i).face_points().into_iter().map(pt).collect() };
    // An empty Thickness is no value to shell with: OK waits for one instead of the command
    // refusing it with an error that asks for an issue report (#960).
    if let Kind::Shell { thickness } = &d.kind
        && thickness.trim().is_empty()
    {
        return Err("enter the thickness".into());
    }
    let (cmd, params): (&str, Value) = match &d.kind {
        Kind::ChatContext => return Err("the chat takes these picks; nothing runs".into()),
        Kind::Extrude {
            distance,
            direction,
            operation,
            distance2,
            taper,
            taper_present,
            to_offset,
            start,
            all,
            thin,
            wall,
            wall_side,
            wall2,
            ..
        } => {
            need(0, if *thin { "curves or profiles" } else { "profiles or a planar face" })?;
            let mut common = json!({"distance": distance, "direction": DIRS.get(*direction).copied().unwrap_or("positive"), "operation": OPS.get(*operation).copied().unwrap_or("new")});
            let tapered = s.doc.eval(taper, ValueKind::Angle).map(|v| v.abs() > 1e-12).unwrap_or(!taper.trim().is_empty());
            if two_sided(*direction) {
                common["distance2"] = json!(distance2);
            } else if *direction < 2 && (*taper_present || tapered) {
                common["taper"] = json!(taper);
            }
            put(&mut common, &[("start_offset", start)]);
            if *all {
                common["through_all"] = json!(true);
            }
            if to_object_on(&d.inputs) {
                common["to"] = to_object_point(&d.inputs).ok_or("select the object to extrude to")?;
                put(&mut common, &[("to_offset", to_offset)]);
            }
            if object_on(&d.inputs, FROM_OBJECT) {
                common["from"] = object_point(&d.inputs, FROM_OBJECT).ok_or("select the object to extrude from")?;
            }
            let with = |extra: Value| -> Value {
                let mut p = common.clone();
                if let (Value::Object(m), Value::Object(e)) = (&mut p, extra) {
                    m.extend(e);
                    for (k, v) in &d.extra {
                        m.entry(k.clone()).or_insert_with(|| v.clone());
                    }
                }
                p
            };
            if *thin {
                // The picked curves, and the outlines (outer and holes) of picked profiles.
                let st = s.world_state();
                let mut curves = sels(d, 0).curve_ids();
                let mut sketch = None;
                for x in sels(d, 0) {
                    if let Sel::Profile { sketch: id, index } = x
                        && let Some(p) = st.sketch(*id).and_then(|ss| ss.profiles.get(*index))
                    {
                        sketch = Some(*id);
                        curves.extend(p.curve_ids());
                    }
                }
                let mut seen = std::collections::BTreeSet::new();
                curves.retain(|c| seen.insert(c.clone()));
                // A picked profile's sketch, or the edited extrude's, when it has every curve.
                let edited = d.editing.and_then(|(id, _)| s.doc.feature(id)).and_then(|f| match f.kind {
                    FeatureKind::Extrude { sketch, .. } => Some(sketch),
                    _ => None,
                });
                let has_all = |id: u64| curves.iter().all(|c| st.sketch(id).is_some_and(|ss| ss.sketch.curve_index(c).is_some()));
                let sketch = match sketch.or(edited.map(u64::from)) {
                    Some(id) if has_all(id) => id,
                    _ => picks_sketch(app, sels(d, 0), None).ok_or("the curves must be in one sketch")?,
                };
                let mut wall_params = json!({"sketch": sketch, "thin": true, "curves": curves, "thickness": wall, "side": WALL_SIDES.get(*wall_side).copied().unwrap_or("center")});
                if *wall_side == 3 {
                    wall_params["thickness2"] = json!(wall2);
                }
                return Ok(vec![("solid.extrude".to_string(), with(wall_params))]);
            }
            let mut out = Vec::new();
            let (sketch, idx) = profiles();
            let profile = (!sketch.is_null()).then(|| ("solid.extrude".to_string(), with(json!({"sketch": sketch, "profiles": idx}))));
            // A new extrude runs its picked faces before its profiles: the profiles' extrude can
            // cover a picked face, which is then gone when its own extrude looks for it (#3926).
            // An edit rebuilds the profiles' extrude, so that one stays first.
            let faces_first = d.editing.is_none();
            if !faces_first {
                out.extend(profile.clone());
            }
            // Coplanar faces of one body share one sketch and Boolean. Mixed selections
            // retain their original order and explicit face-body scope.
            let state = s.world_state();
            let mut group: Option<(String, solidcraft_engine::geom::Plane, Vec<Value>)> = None;
            let mut coplanar = true;
            for pick in sels(d, 0) {
                let Sel::Face { body, index, point } = pick else { continue };
                let Some(source) = state.body(body) else { return Err("selected body disappeared".into()) };
                let Some(plane) = source.body.planar_face(*index) else {
                    coplanar = false;
                    break;
                };
                let tolerance = (source.body.size() * 1e-9).max(1e-8);
                if let Some((name, base, points)) = &mut group {
                    if name != body || base.normal().dot(plane.normal()) <= 1.0 - 1e-9 || base.height(plane.origin).abs() > tolerance {
                        coplanar = false;
                        break;
                    }
                    points.push(pt(*point));
                } else {
                    group = Some((body.clone(), plane, vec![pt(*point)]));
                }
            }
            if coplanar
                && let Some((body, _, points)) = group
                && points.len() > 1
            {
                out.push(("solid.extrude".to_string(), with(json!({"faces": points, "face_body": body}))));
                if faces_first {
                    out.extend(profile);
                }
                return Ok(out);
            }
            for pick in sels(d, 0) {
                if let Sel::Face { body, point, .. } = pick {
                    out.push(("solid.extrude".to_string(), with(json!({"face": pt(*point), "face_body": body}))));
                }
            }
            if faces_first {
                out.extend(profile);
            }
            return Ok(out);
        }
        Kind::Revolve { angle, operation, axis: stored_axis } => {
            need(0, "profiles")?;
            let axis = match sels(d, 1).first() {
                Some(Sel::Axis { name }) => json!(name),
                Some(Sel::SketchCurve { id, .. }) => json!(id),
                Some(x) => axis_param(app, x)?,
                None => stored_axis.clone().ok_or("select an axis")?,
            };
            let (sketch, idx) = profiles();
            let mut p = json!({"axis": axis, "angle": angle, "operation": OPS.get(*operation).copied().unwrap_or("new")});
            // A planar body face revolves like a profile (#2747); profiles win when both are picked.
            let face = sels(d, 0).iter().find_map(|x| match x {
                Sel::Face { body, point, .. } => Some((body, point)),
                _ => None,
            });
            match face {
                Some((body, point)) if sketch.is_null() => {
                    p["face"] = pt(*point);
                    p["face_body"] = json!(body);
                }
                _ => {
                    p["sketch"] = sketch;
                    p["profiles"] = idx;
                }
            }
            if to_object_on(&d.inputs) {
                p["to"] = to_object_point(&d.inputs).ok_or("select the object to turn to")?;
            }
            ("solid.revolve", p)
        }
        Kind::Fillet { radius, chamfer, ctype, distance2, angle, flip, .. } => {
            need(0, "edges")?;
            // Faces stand for all their edges.
            let st = s.world_state();
            let mut pts: Vec<Vec3> = Vec::new();
            for x in sels(d, 0) {
                match x {
                    Sel::Edge { point, .. } => pts.push(*point),
                    Sel::Face { body, index, .. } => {
                        if let Some(b) = st.body(body) {
                            let m = b.mesh();
                            for e in m.face_edges(u32::try_from(*index).unwrap_or(u32::MAX)) {
                                if !m.seams.get(e).copied().unwrap_or(false)
                                    && let Some(p) = m.edges.get(e)
                                {
                                    pts.push(polyline_mid(p));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            let mut uniq: Vec<Vec3> = Vec::new();
            for p in pts {
                if !uniq.iter().any(|q| q.dist(p) < 1e-9) {
                    uniq.push(p);
                }
            }
            let edges: Vec<Value> = uniq.into_iter().map(pt).collect();
            if *chamfer {
                let mut p = json!({"edges": edges, "distance": radius});
                match *ctype {
                    1 => p["distance2"] = json!(distance2),
                    2 => p["angle"] = json!(angle),
                    _ => {}
                }
                if *ctype > 0 && *flip {
                    p["flip"] = json!(true);
                }
                ("solid.chamfer", p)
            } else {
                let mut p = json!({"edges": edges, "radius": radius});
                let style = d.extra.get("type").and_then(Value::as_str);
                if matches!(style, Some("variable" | "spline")) {
                    // The start radius goes at one end of the first edge (Swap Ends: the other).
                    let ends = sels(d, 0).iter().find_map(|x| match x {
                        Sel::Edge { body, index, .. } => {
                            st.body(body).and_then(|b| b.mesh().edges.get(*index).and_then(|e| Some((*e.first()?, *e.last()?))))
                        }
                        _ => None,
                    });
                    let swap = d.extra.get("swap_ends").and_then(Value::as_bool).unwrap_or(false);
                    if let Some((a, b)) = ends {
                        p["start"] = pt(if swap { b } else { a });
                    }
                    let mids = mid_points(&d.extra);
                    if style == Some("variable") && !mids.is_empty() {
                        p["mid"] = json!(mids.iter().map(|(t, r)| json!([t, r])).collect::<Vec<_>>());
                        if d.extra.get("smooth").and_then(Value::as_bool).unwrap_or(false) {
                            p["smooth"] = json!(true);
                        }
                    }
                }
                ("solid.fillet", p)
            }
        }
        Kind::Shell { .. } => {
            need(0, "faces to remove")?;
            let mut p = json!({"faces": face_points(0)});
            // The picked faces' body: a point alone may name a flush face of another body (#4555).
            let mut bodies = sels(d, 0).iter().filter_map(|s| if let Sel::Face { body, .. } = s { Some(body) } else { None });
            if let Some(first) = bodies.next()
                && bodies.all(|b| b == first)
            {
                p["body"] = json!(first);
            }
            ("solid.shell", p)
        }
        Kind::Draft { .. } => {
            need(0, "faces")?;
            need(1, "the neutral plane")?;
            let Some(Sel::Face { body, .. }) = sels(d, 0).first() else { return Err("select faces first".into()) };
            if sels(d, 0).iter().any(|pick| !matches!(pick, Sel::Face { body: other, .. } if other == body)) {
                return Err("pick faces of one body".into());
            }
            let neutral = plane_value(s, sels(d, 1).first()).ok_or("the neutral plane must be a plane or a planar face")?;
            ("solid.draft", json!({"body": body, "faces": face_points(0), "neutral": neutral}))
        }
        Kind::Mirror => {
            need(0, "objects")?;
            need(1, "the mirror plane")?;
            let plane = plane_value(s, sels(d, 1).first()).ok_or("the mirror plane must be a plane or a planar face")?;
            let mut p = json!({"plane": plane});
            pattern_objects(s, sels(d, 0), &mut p);
            ("solid.mirror", p)
        }
        Kind::PatternRect { count, spacing, count2, spacing2 } => {
            need(0, "objects")?;
            let kept = d.extra.get("dir1").and_then(|v| serde_json::from_value::<[f64; 3]>(v.clone()).ok()).map(|a| Vec3::new(a[0], a[1], a[2]));
            let d1 = match (sels(d, 1).first(), kept) {
                (Some(x), _) => axis_of(app, x).ok_or("the direction must be an axis, a sketch line or a straight edge")?.1,
                (None, Some(k)) => k,
                (None, None) => return Err("select a direction (an axis, a sketch line or a straight edge) first".into()),
            };
            let mut p = json!({"dir1": pt(d1), "count1": count, "spacing1": spacing});
            pattern_objects(s, sels(d, 0), &mut p);
            let kept2 = d.extra.get("dir2").and_then(|v| serde_json::from_value::<[f64; 3]>(v.clone()).ok()).map(|a| Vec3::new(a[0], a[1], a[2]));
            let d2 = match sels(d, 2).first() {
                Some(x) => Some(axis_of(app, x).ok_or("the second direction must be an axis, a sketch line or a straight edge")?.1),
                None => kept2,
            };
            if let Some(d2) = d2 {
                p["dir2"] = pt(d2);
                p["count2"] = json!(count2);
                p["spacing2"] = json!(spacing2);
            }
            ("solid.pattern.rectangular", p)
        }
        Kind::PatternCirc { count, angle } => {
            need(0, "objects")?;
            let axis = match sels(d, 1).first() {
                None if d.extra.contains_key("axis") => d.extra.get("axis").cloned().unwrap_or(Value::Null),
                Some(x) => axis_param(app, x)?,
                None => return Err("select an axis".into()),
            };
            let mut p = json!({"axis": axis, "count": count, "angle": angle});
            pattern_objects(s, sels(d, 0), &mut p);
            ("solid.pattern.circular", p)
        }
        Kind::Loft { operation, rail_fit } => {
            need(0, "profiles of two or more sketches")?;
            // Each picked profile is a section, in pick order, two of them from one sketch
            // included (#8221); a sketch point is a section of its own, an apex the loft closes
            // to (#4830).
            let mut sections: Vec<Value> = Vec::new();
            let (mut profile_sketches, mut apexes) = (Vec::new(), 0);
            for x in sels(d, 0) {
                match x {
                    Sel::Profile { sketch, index } => {
                        sections.push(json!({"sketch": sketch, "profiles": [index]}));
                        profile_sketches.push(*sketch);
                    }
                    Sel::SketchPoint { id } => {
                        if let Some((sk, point)) = id.split_once(':').and_then(|(sk, point)| Some((sk.parse::<u64>().ok()?, point))) {
                            sections.push(json!({"sketch": sk, "point": point}));
                            apexes += 1;
                        }
                    }
                    _ => {}
                }
            }
            profile_sketches.sort_unstable();
            profile_sketches.dedup();
            if profile_sketches.len() + apexes < 2 {
                return Err("pick profiles in two or more sketches, or a profile and a sketch point".into());
            }
            let mut p = json!({"sections": sections, "operation": OPS.get(*operation).copied().unwrap_or("new")});
            crate::dialogs_sweep::rail_params(app, *rail_fit, [sels(d, 1), sels(d, 2)], &mut p)?;
            ("solid.loft", p)
        }
        Kind::Sweep { operation, opts } => {
            need(0, "a profile")?;
            need(1, "the path")?;
            let (sketch, idx) = profiles();
            let path: Vec<String> = sels(d, 1).curve_ids();
            let path_sketch = picks_sketch(app, sels(d, 1), sketch.as_u64()).ok_or("the path curves must be in one sketch")?;
            let mut p = json!({"sketch": sketch, "profiles": idx, "path_sketch": path_sketch, "path": path, "operation": OPS.get(*operation).copied().unwrap_or("new")});
            crate::dialogs_sweep::params(app, opts, sels(d, 2).first(), [sels(d, 3), sels(d, 4)], &mut p)?;
            ("solid.sweep", p)
        }
        Kind::OffsetPlane { .. } => {
            need(0, "a plane")?;
            let base = plane_arg(app, sels(d, 0).first())?;
            match offset_through_point(d) {
                // A null `offset` keeps the descriptor's distance out: the plane goes through the point.
                Some(point) => ("construct.plane.offset", json!({"base": base, "through": construct_ref(app, point)?, "offset": null})),
                None => ("construct.plane.offset", json!({"base": base})),
            }
        }
        Kind::MeshConvert { tolerance } => {
            need(0, "the mesh body")?;
            let body = match sels(d, 0).first() {
                Some(Sel::Body { name }) => name.clone(),
                _ => return Err("pick the mesh body to convert".into()),
            };
            ("mesh.convert", json!({"body": body, "tolerance": tolerance}))
        }
        Kind::Midplane => {
            need(0, "the first plane")?;
            need(1, "the second plane")?;
            let a = plane_arg(app, sels(d, 0).first())?;
            let b = plane_arg(app, sels(d, 1).first())?;
            ("construct.plane.midplane", json!({"a": a, "b": b}))
        }
        Kind::Construct { command, keys, t, .. } => {
            let mut p = serde_json::Map::new();
            for (i, key) in keys.iter().enumerate() {
                let input = d.inputs.get(i).map_or("", |inp| inp.label);
                let sel = sels(d, i).first().ok_or_else(|| format!("pick the {}", input.to_lowercase()))?;
                p.insert(
                    (*key).to_string(),
                    match d.extra.get(*key) {
                        Some(saved) => saved.clone(),
                        None => construct_ref(app, sel)?,
                    },
                );
            }
            if let Some(t) = t {
                p.insert("t".into(), json!(t));
            }
            (*command, Value::Object(p))
        }
        Kind::PathPlane { t, sketch } => {
            need(0, "a path")?;
            let path = match sels(d, 0).first() {
                Some(Sel::SketchCurve { id, sketch: Some(named) }) => json!({"sketch": named, "curve": id}),
                Some(Sel::SketchCurve { id, .. }) => {
                    let st = app.session.world_state();
                    let matches: Vec<u64> = st
                        .sketches
                        .iter()
                        .filter(|ss| ss.sketch.curve_index(id).is_some() || ss.sketch.wire_index(id).is_some())
                        .map(|ss| ss.feature)
                        .collect();
                    let source = match sketch {
                        Some(source) if matches.contains(source) => *source,
                        None if matches.len() == 1 => *matches.first().ok_or("pick a path")?,
                        _ => return Err("pick the path in the viewport to identify its source sketch".into()),
                    };
                    json!({"sketch": source, "curve": id})
                }
                Some(Sel::Edge { point, .. }) => d.extra.get("path").cloned().unwrap_or_else(|| json!({"edge": pt(*point)})),
                _ => return Err("pick a sketch curve or model edge".into()),
            };
            ("construct.plane.along_path", json!({"path": path, "t": t}))
        }
        Kind::AnglePlane { .. } => {
            need(0, "a plane")?;
            let base = plane_arg(app, sels(d, 0).first())?;
            let axis = match sels(d, 1).first() {
                Some(x) => axis_param(app, x)?,
                None => d.extra.get("axis").cloned().ok_or("pick an axis")?,
            };
            ("construct.plane.at_angle", json!({"base": base, "axis": axis}))
        }
        Kind::Rib { web, thickness, depth, flip, normal } => {
            need(0, "open sketch curves")?;
            let curves = sels(d, 0).curve_ids();
            let sketch = picks_sketch(app, sels(d, 0), None).ok_or("the curves must be in one sketch")?;
            let mut p = json!({"sketch": sketch, "curves": curves, "thickness": thickness, "flip": flip});
            if *normal {
                p["direction"] = json!("normal");
            }
            put(&mut p, &[("depth", depth)]);
            (if *web { "solid.web" } else { "solid.rib" }, p)
        }
        Kind::Emboss { .. } => {
            need(0, "profiles")?;
            let (sketch, idx) = profiles();
            let mut p = json!({"sketch": sketch, "profiles": idx});
            if let Some(Sel::Face { body, point, .. }) = sels(d, 1).first() {
                p["face"] = pt(*point);
                p["targets"] = json!([body]);
            }
            ("solid.emboss", p)
        }
        Kind::ReplaceFace => {
            need(0, "faces to move")?;
            need(1, "the target")?;
            let mut p = json!({"faces": face_points(0)});
            match sels(d, 1).first() {
                Some(Sel::Plane { name }) => p["target"] = json!(name),
                Some(Sel::Face { point, .. }) => p["target_face"] = pt(*point),
                _ => return Err("pick a plane or planar face".into()),
            }
            ("solid.replace_face", p)
        }
        Kind::DeleteFaces => {
            need(0, "faces")?;
            ("face.delete", json!({"faces": face_points(0)}))
        }
        Kind::Align { flip, components, axes } => {
            need(0, if *components { "components" } else { "bodies" })?;
            need(1, "what to move from")?;
            need(2, "where to move it to")?;
            let mut p = if *components {
                let paths = occurrence_paths(s, sels(d, 0));
                if paths.is_empty() {
                    return Err("select components first".into());
                }
                json!({"paths": paths, "flip": flip})
            } else {
                let mut p = json!({"bodies": sels(d, 0).body_names(), "flip": flip});
                if *axes != [true; 3] {
                    p["axes"] = json!(axes);
                }
                p
            };
            for (i, key) in [(1, "from"), (2, "to")] {
                let snap = d.extra.get(&format!("{key}_snap")).and_then(Value::as_str).unwrap_or("picked");
                match (sels(d, i).first(), snap) {
                    (Some(Sel::Face { point, .. }), "center") => p[key] = json!({"snap": "face_center", "at": pt(*point)}),
                    (Some(Sel::Face { point, .. } | Sel::Edge { point, .. }), "circle") => {
                        p[key] = json!({"snap": "circle_center", "at": pt(*point)})
                    }
                    (Some(Sel::Face { point, .. }), _) => p[format!("{key}_face")] = pt(*point),
                    (Some(Sel::Edge { point, .. }), _) => p[key] = json!({"snap": "edge_mid", "at": pt(*point)}),
                    // The origin point (a vertex of no body) stays the origin.
                    (Some(Sel::Vertex { body, .. }), _) if body.is_empty() => p[key] = json!("origin"),
                    (Some(Sel::Vertex { point, .. }), _) => p[key] = pt(*point),
                    // A plane stays a plane, so the bodies follow it when it moves.
                    (Some(Sel::Plane { name }), _) => p[format!("{key}_plane")] = json!(name),
                    _ => return Err("pick faces, edges, vertices or planes".into()),
                }
            }
            (if *components { "occurrence.align" } else { "solid.align" }, p)
        }
        Kind::Remove => {
            need(0, "bodies")?;
            ("solid.remove", json!({"bodies": sels(d, 0).body_names()}))
        }
        Kind::PathPattern { count, spacing } => {
            need(0, "objects")?;
            need(1, "the path")?;
            let path = sels(d, 1).curve_ids();
            let sketch = picks_sketch(app, sels(d, 1), None).ok_or("the path curves must be in one sketch")?;
            let mut p = json!({"path_sketch": sketch, "path": path, "count": count});
            // A saved extent stays first-to-last distance when the quantity changes.
            p[if d.extra.contains_key("distance") { "distance" } else { "spacing" }] = json!(spacing);
            pattern_objects(s, sels(d, 0), &mut p);
            ("solid.pattern.path", p)
        }
        Kind::Pipe { wall, .. } => {
            need(0, "the path")?;
            let path = sels(d, 0).curve_ids();
            let sketch = picks_sketch(app, sels(d, 0), None).ok_or("the path curves must be in one sketch")?;
            let mut p = json!({"path_sketch": sketch, "path": path});
            put(&mut p, &[("wall", wall)]);
            ("solid.pipe", p)
        }
        Kind::Stock { .. } => {
            let mut p = json!({});
            if !sels(d, 0).is_empty() {
                p["bodies"] = json!(sels(d, 0).body_names());
            }
            ("solid.bounding_solid", p)
        }
        Kind::Split => {
            need(0, "the body to split")?;
            need(1, "the splitting plane")?;
            let body = sels(d, 0).body_names().into_iter().next().unwrap_or_default();
            // A plane, a planar face's plane, or any other face (its surface extended).
            let p = match sels(d, 1).first() {
                Some(Sel::Plane { name }) => json!({"body": body, "plane": name}),
                Some(Sel::Face { body: b, index, point }) => match planar_face(s, b, *index) {
                    Some((_, n)) => json!({"body": body, "plane": {"origin": pt(*point), "normal": pt(n)}}),
                    None => json!({"body": body, "tool": {"body": b, "point": pt(*point)}}),
                },
                // A straight sketch line, in the sketch it was picked in (#1552).
                Some(Sel::SketchCurve { id, sketch: named }) => {
                    let picked = named.or_else(|| d.extra.get("tool_sketch").and_then(Value::as_u64));
                    let st = s.world_state();
                    let holding: Vec<u64> = st.sketches.iter().filter(|ss| ss.sketch.curve_index(id).is_some()).map(|ss| ss.feature).collect();
                    let sketch = match picked {
                        Some(k) if holding.contains(&k) => k,
                        None if holding.len() == 1 => holding.first().copied().ok_or("pick a sketch line")?,
                        _ => return Err("pick the sketch line in the viewport".into()),
                    };
                    json!({"body": body, "sketch": sketch, "line": id})
                }
                _ => return Err("pick a plane, a face or a sketch line".into()),
            };
            ("solid.split_body", p)
        }
        Kind::Scale { .. } => {
            need(0, "bodies")?;
            ("solid.scale", json!({"bodies": sels(d, 0).body_names()}))
        }
        Kind::OffsetFaces { .. } => {
            need(0, "faces")?;
            ("solid.offset_face", json!({"faces": face_points(0)}))
        }
        Kind::Thread { designation, length, suggested, handedness, .. } => {
            need(0, "a cylindrical face")?;
            let mut p = json!({"face": face_points(0).into_iter().next().unwrap_or(Value::Null)});
            if !designation.trim().is_empty() && suggested.as_deref() != Some(designation.as_str()) {
                p["designation"] = json!(designation.trim());
            }
            put(&mut p, &[("length", length)]);
            if !handedness.is_right() {
                p["handedness"] = json!(handedness);
            }
            ("solid.thread", p)
        }
        Kind::Material { index, editor } => {
            if editor.editing() {
                return Err(crate::i18n::tr("material.editor.save_or_discard").to_string());
            }
            need(0, "bodies")?;
            let name = s.doc.material_names().get(*index).cloned().ok_or_else(|| crate::i18n::tr("material.selection.choose").to_string())?;
            ("material.assign", json!({"bodies": sels(d, 0).body_names(), "material": name}))
        }
        Kind::Section { offset, flip, hatch } => {
            if section_turns_off(s, &d.inputs) && !d.extra.contains_key("plane") {
                return Ok(vec![("inspect.section".into(), json!({"clear": true}))]);
            }
            if !d.extra.contains_key("plane") {
                need(0, "a plane or planar face")?;
            }
            let plane = match sels(d, 0).first() {
                None if d.extra.contains_key("plane") => d.extra.get("plane").cloned().ok_or("select a plane first")?,
                Some(Sel::Plane { name }) if matches!(name.as_str(), "XY" | "XZ" | "YZ") => json!(name),
                Some(Sel::Face { body, index, point }) => {
                    let (_, n) = planar_face(s, body, *index).ok_or("the face must be planar")?;
                    json!({"origin": pt(*point), "normal": pt(n)})
                }
                Some(Sel::Plane { name }) => {
                    let pl = solidcraft_engine::view::construction_planes(s)
                        .into_iter()
                        .find(|(_, n, _)| n == name)
                        .map(|(_, _, pl)| pl)
                        .ok_or("unknown plane")?;
                    json!({"origin": pt(pl.origin), "normal": pt(pl.normal())})
                }
                _ => return Err("pick a plane or a planar face".into()),
            };
            ("inspect.section", json!({"plane": plane, "offset": offset, "flip": flip, "hatch": hatch}))
        }
        Kind::Move { .. } => return Ok(vec![("solid.move".into(), crate::dialogs_move::params(app, d)?)]),
        Kind::Hole { diameter, depth, kind, cb_diameter, cb_depth, cs_diameter, cs_angle, opts } => {
            need(0, if opts.multiple { "sketch points" } else { "a face position" })?;
            let ty = match *kind {
                0 if opts.angled => "drilled",
                k => HOLE_TYPES.get(k).copied().unwrap_or("simple"),
            };
            // One hole per picked face position, or one Hole feature per sketch for sketch points.
            let mut places: Vec<Value> = Vec::new();
            if opts.multiple {
                let mut by_sketch: Vec<(u64, Vec<String>)> = Vec::new();
                for x in sels(d, 0) {
                    if let Sel::SketchPoint { id } = x
                        && let Some((sk, pid)) = id.split_once(':')
                        && let Ok(sk) = sk.parse::<u64>()
                    {
                        match by_sketch.iter_mut().find(|(s, _)| *s == sk) {
                            Some((_, v)) => v.push(pid.to_string()),
                            None => by_sketch.push((sk, vec![pid.to_string()])),
                        }
                    }
                }
                let all_points = d.extra.get("all_points").and_then(Value::as_bool).unwrap_or(false);
                let flip = d.extra.get("flip").and_then(Value::as_bool).unwrap_or(false);
                places.extend(
                    by_sketch
                        .into_iter()
                        .map(|(sk, ids)| if all_points { json!({ "sketch": sk }) } else { json!({"sketch": sk, "points": ids}) })
                        .map(|mut place| {
                            if flip {
                                place["flip"] = json!(true);
                            }
                            place
                        }),
                );
            } else {
                // One hole per pick: a curved surface split at its seam is picked whole (both
                // halves, the clicked one first), but the hole goes where it was clicked only.
                let mut taken: Vec<Sel> = Vec::new();
                for x in sels(d, 0) {
                    if !matches!(x, Sel::Face { .. }) || taken.iter().any(|t| same_face(t, x)) {
                        continue;
                    }
                    taken.extend(crate::viewport::grouped_selection(s, x));
                    let kept = d.extra.get("position_exprs").filter(|k| face_point(x).is_some_and(|p| p == k["point"])).map(|k| k["at"].clone());
                    // A place snapped to a sketch point keeps the point, so the hole follows it (#2110).
                    let snapped = if kept.is_none() { single_hole_point(app, d, x) } else { None };
                    places.extend(face_point(x).map(|p| match snapped {
                        Some((sk, id)) => json!({"sketch": sk, "points": [id], "single": true}),
                        None => json!({ "position": kept.unwrap_or(p) }),
                    }));
                }
                // Reference edges (#37) place the one hole by its distances from them, from where
                // it was picked.
                let references = crate::hole_refs::params(d);
                if !references.is_empty() {
                    let at = sels(d, 0).iter().find_map(face_point);
                    match (places.as_mut_slice(), at) {
                        ([place], Some(at)) => *place = json!({"position": at, "references": references}),
                        _ => return Err("reference edges place one hole: pick one place on the face".into()),
                    }
                }
            }
            let mut out = Vec::new();
            for place in places {
                let mut params = place;
                if !opts.tapped {
                    params["diameter"] = json!(diameter);
                }
                params["type"] = json!(ty);
                if to_object_on(&d.inputs) {
                    params["to"] = to_object_point(&d.inputs).ok_or("select the object to drill to")?;
                } else if !opts.all {
                    params["depth"] = json!(depth);
                }
                if ty == "drilled" || (opts.angled && !opts.all) {
                    params["tip_angle"] = json!(opts.tip_angle);
                }
                if opts.tapped {
                    if let Some(error) = opts.custom_thread.as_ref().and_then(|draft| draft.error.as_ref()) {
                        return Err(error.clone());
                    }
                    solidcraft_engine::doc::parse_tapped_thread(&opts.thread)
                        .ok_or("select a valid metric or catalogued UNC/UNF, G or Rp thread size")?;
                    params["thread"] = json!(opts.thread.trim());
                    if !opts.thread_full_depth {
                        params["thread_depth"] = json!(opts.thread_depth.trim());
                    }
                    if !opts.handedness.is_right() {
                        params["handedness"] = json!(opts.handedness);
                    }
                }
                match *kind {
                    1 => {
                        params["cb_diameter"] = json!(cb_diameter);
                        params["cb_depth"] = json!(cb_depth);
                    }
                    2 => {
                        params["cs_diameter"] = json!(cs_diameter);
                        params["cs_angle"] = json!(cs_angle);
                    }
                    _ => {}
                }
                out.push(("solid.hole".to_string(), params));
            }
            return Ok(out);
        }
        Kind::Primitive { cmd, fields, operation, at, .. } => {
            let mut p = serde_json::Map::new();
            for (k, v) in fields {
                p.insert((*k).into(), json!(v));
            }
            if *cmd == "solid.coil"
                && let Some((sketch, index)) = sels(d, 0).profiles().first().copied()
            {
                p.insert("sketch".into(), json!(sketch));
                p.insert("profiles".into(), json!([index]));
                p.remove("diameter");
                p.remove("section_size");
            }
            if let Some((point, normal)) = *at {
                match *cmd {
                    "solid.box" => {
                        p.insert("corner".into(), box_corner(s, fields, point, normal));
                    }
                    "solid.cylinder" | "solid.coil" | "solid.cone" => {
                        p.insert("base".into(), pt(point));
                        if let Some(n) = normal {
                            p.insert("axis".into(), pt(n));
                        }
                        // Reference edges (#6380) place the base by its distances from them.
                        let references = crate::hole_refs::params(d);
                        if !references.is_empty() {
                            p.insert("references".into(), json!(references));
                        }
                    }
                    _ => {
                        p.insert("center".into(), pt(point));
                    }
                }
            } else {
                crate::primitive_placement::params(s, cmd, &d.extra, &mut p);
            }
            p.insert("operation".into(), json!(OPS.get(*operation).copied().unwrap_or("new")));
            (cmd, Value::Object(p))
        }
        Kind::BoundaryFill { operation, cells, .. } => {
            need(0, "the tool bodies")?;
            let picked: Vec<Value> = cells.iter().filter(|c| c.2).map(|c| pt(c.0)).collect();
            if picked.is_empty() {
                return Err("tick the cells to fill".into());
            }
            (
                "solid.boundary_fill",
                json!({"tools": sels(d, 0).body_names(), "cells": picked, "operation": OPS.get(*operation).copied().unwrap_or("new")}),
            )
        }
        Kind::Combine { operation, keep_tools } => {
            need(0, "the target body")?;
            need(1, "tool bodies")?;
            let target = sels(d, 0).body_names().into_iter().next().unwrap_or_default();
            let tools: Vec<String> = sels(d, 1).body_names().into_iter().filter(|n| *n != target).collect();
            (
                "solid.combine",
                json!({"target": target, "tools": tools, "operation": OPS.get(*operation).copied().unwrap_or("join"), "keep_tools": keep_tools}),
            )
        }
        Kind::EditParam { name, expr } => ("parameters.change", json!({"name": name, "expression": expr})),
        Kind::Rename { feature, name } => ("timeline.rename", json!({"feature": feature, "name": name})),
        Kind::ConfirmDelete { features, groups, .. } => ("timeline.delete", crate::timeline::delete_params(features, groups)),
        Kind::SketchIntersect { projection_link, .. } => {
            need(0, "geometry or bodies")?;
            let refs = sels(d, 0)
                .iter()
                .map(|sel| match sel {
                    Sel::Body { name } => Ok(json!({"body": name})),
                    Sel::Face { body, point, .. } => Ok(json!({"face": [point.x, point.y, point.z], "body": body})),
                    Sel::Edge { body, point, .. } => Ok(json!({"edge": [point.x, point.y, point.z], "body": body})),
                    Sel::ExternalSketchCurve { sketch, id } => Ok(json!({"sketch": sketch, "curve": id})),
                    _ => Err("select geometry or bodies".to_string()),
                })
                .collect::<Result<Vec<_>, _>>()?;
            ("sketch.intersect", json!({"refs": refs, "projection_link": projection_link}))
        }
        Kind::SketchProject { projection_link, .. } => {
            need(0, "geometry or bodies")?;
            let refs =
                crate::tools::projection_preselection(sels(d, 0)).ok_or_else(|| "select geometry or bodies that can be projected".to_string())?;
            ("sketch.project", json!({"refs": refs, "projection_link": projection_link}))
        }
        Kind::Sketch | Kind::Measure { .. } => return Ok(Vec::new()),
        Kind::Assembly(k) => return crate::dialogs_assembly::commands(app, k, &d.inputs),
        Kind::Sheet(k) => return crate::dialogs_sheet::commands(app, k, &d.inputs, &d.extra),
        Kind::Surface(k) => return crate::dialogs_surface::commands(app, k, &d.inputs, &d.extra),
        Kind::SketchModify(k) => return crate::sketch_modify_ui::commands(app, k, &d.inputs, &d.extra),
        Kind::SaveMesh(k) => return crate::dialogs_save_mesh::commands(k, &d.inputs),
        Kind::ViewDxf { .. } => return Ok(Vec::new()),
        Kind::InsertDrawing(p) => return crate::dialogs_insert_drawing::commands(p),
        Kind::Plastic(k) => return crate::dialogs_plastic::commands(app, k, &d.inputs, &d.extra),
        Kind::Frame(k) => return crate::dialogs_frame::commands(app, k, &d.inputs, &d.extra),
        Kind::Appearance(k) => return Ok(crate::dialogs_appearance::commands(k, &d.inputs)),
        Kind::Motion(k) => return crate::dialogs_motion::commands(app, k, &d.inputs),
        Kind::Part(k) => return crate::dialogs_parts::commands(app, k, &d.inputs),
        Kind::PartSize(k) => return crate::dialogs_part_size::commands(app, k),
        Kind::Text(k) => return crate::sketch_text::commands(app, k),
        Kind::TextMove(k) => return crate::sketch_text_move::commands(app, k),
    };
    let mut params = params;
    d.kind.fill_simple_params(&mut params);
    if let Value::Object(m) = &mut params {
        for (k, v) in &d.extra {
            m.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    Ok(vec![(cmd.to_string(), params)])
}

/// The corner of a box placed at `p`: standing on a face whose normal is along an axis (centred
/// on `p` across the face), otherwise centred on `p`. Sizes that are expressions stay
/// expressions, so the box keeps standing on the face when they change.
fn box_corner(s: &Session, fields: &[(&'static str, String)], p: Vec3, normal: Option<Vec3>) -> Value {
    let size =
        |k: &str| fields.iter().find(|f| f.0 == k).map(|f| crate::length_input::expression(&s.doc, &f.1, ValueKind::Length)).unwrap_or_default();
    let along = |axis: Vec3| normal.map(|n| n.dot(axis)).filter(|d| d.abs() > 1.0 - 1e-9);
    let coord = |c: f64, size: String, axis: Vec3| {
        // How much of the size lies behind `p`: none (or all) along the face normal, half across it.
        let back = match along(axis) {
            Some(d) if d > 0.0 => return json!(c),
            Some(_) => 1.0,
            None => 0.5,
        };
        // A plain length ("20 mm", "2 in"; a bare number has the design's unit by now) gives a
        // plain number (mm).
        // A size typed as `l = 20` is placed by its value: `l` exists only once the box does.
        let size = solidcraft_engine::doc::expr::definition(&size).map_or(size.clone(), |(_, rhs)| rhs.to_string());
        let plain = solidcraft_engine::doc::expr::is_literal(&size)
            .then(|| solidcraft_engine::doc::Document::eval_in(&Default::default(), &size, ValueKind::Length).ok())
            .flatten();
        match plain.filter(|v| v.is_finite()) {
            Some(v) => json!(c - back * v),
            None if back < 1.0 => json!(format!("{c} mm - ({size}) / 2")),
            None => json!(format!("{c} mm - ({size})")),
        }
    };
    json!([coord(p.x, size("length"), Vec3::X), coord(p.y, size("width"), Vec3::Y), coord(p.z, size("height"), Vec3::Z)])
}

/// Switch an extrude's first input between profiles and faces, and (thin) profiles and curves;
/// picks the other kind doesn't take are dropped.
fn set_thin(inputs: &mut [SelInput], thin: bool) {
    if let Some(input) = inputs.first_mut() {
        input.accept = if thin { THIN_ACCEPT } else { EXTRUDE_ACCEPT };
        input.label = if thin { "Profiles/Curves" } else { "Profiles" };
        input.items.retain(|x| match x {
            Sel::Profile { .. } => true,
            Sel::SketchCurve { .. } => thin,
            Sel::Face { .. } => !thin,
            _ => false,
        });
    }
}

/// The features that made these bodies (what patterns and mirrors copy).
/// The features a pattern repeats: picked features as they are, picked bodies by the features
/// that made them.
/// What a pattern or mirror copies: picked features are replayed; picked bodies are copied as
/// they are now, with every feature that shaped them (a fillet after the extrude included).
fn pattern_objects(s: &Session, objects: &[Sel], p: &mut Value) {
    if objects.iter().any(|x| matches!(x, Sel::Feature { .. })) {
        p["features"] = json!(pattern_features(s, objects));
    } else {
        p["bodies"] = json!(objects.body_names());
    }
}

fn pattern_features(s: &Session, objects: &[Sel]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let bodies: Vec<String> = objects.body_names();
    for x in objects {
        if let Sel::Feature { id } = x
            && let Some(f) = s.doc.feature(*id)
            && !out.contains(&f.name)
        {
            out.push(f.name.clone());
        }
    }
    for f in source_features(s, &bodies) {
        if !out.contains(&f) {
            out.push(f);
        }
    }
    out
}

fn source_features(s: &Session, bodies: &[String]) -> Vec<String> {
    let st = s.world_state();
    let mut features: Vec<String> = Vec::new();
    for n in bodies {
        if let Some(f) = st.body(n).and_then(|b| s.doc.feature(b.feature)).map(|f| f.name.clone())
            && !features.contains(&f)
        {
            features.push(f);
        }
    }
    features
}

/// The sketch holding all these curve ids (the active sketch first, never `not`).
/// The sketch the curve picks `sels` are in: the one they name (a curve of the sketch being edited
/// names none), else the one [`curves_sketch`] finds for their ids. Curve ids repeat from one sketch
/// to the next, so a named sketch wins (#3726); picks in different sketches, or only in `not`, have
/// none.
pub(crate) fn picks_sketch(app: &SolveApp, sels: &[Sel], not: Option<u64>) -> Option<u64> {
    let named: Vec<Option<u64>> = sels.iter().filter_map(|x| if let Sel::SketchCurve { sketch, .. } = x { Some(*sketch) } else { None }).collect();
    if named.iter().all(Option::is_none) {
        return curves_sketch(app, &sels.curve_ids(), not);
    }
    // A curve of the sketch being edited names none: it is in the active sketch.
    let active = app.session.active_sketch;
    let first = named.iter().find_map(|k| k.or(active))?;
    named.iter().all(|k| k.or(active) == Some(first)).then_some(first).filter(|k| Some(*k) != not)
}

pub(crate) fn curves_sketch(app: &SolveApp, ids: &[String], not: Option<u64>) -> Option<u64> {
    let st = app.session.world_state();
    let has =
        |sid: u64| st.sketch(sid).is_some_and(|ss| ids.iter().all(|id| ss.sketch.curve_index(id).is_some() || ss.sketch.wire_index(id).is_some()));
    if let Some(a) = app.session.active_sketch
        && Some(a) != not
        && has(a)
    {
        return Some(a);
    }
    st.sketches.iter().rev().map(|ss| ss.feature).find(|sid| Some(*sid) != not && has(*sid))
}

/// An axis selection as a line: a point on it and its unit direction (origin axes, or a straight
/// sketch line).
pub fn axis_of(app: &SolveApp, sel: &Sel) -> Option<(Vec3, Vec3)> {
    match sel {
        Sel::Axis { name } => match name.as_str() {
            "X" => Some((Vec3::ZERO, Vec3::X)),
            "Y" => Some((Vec3::ZERO, Vec3::Y)),
            "Z" => Some((Vec3::ZERO, Vec3::Z)),
            // A construction axis.
            n => solidcraft_engine::view::construction_axes(&app.session).into_iter().find(|(_, an, _, _)| an == n).map(|(_, _, o, d)| (o, d)),
        },
        Sel::SketchCurve { id, .. } => {
            let st = app.session.world_state();
            let sid = picks_sketch(app, std::slice::from_ref(sel), None)?;
            let ss = st.sketch(sid)?;
            let (pa, pb) = if let Some(i) = ss.sketch.curve_index(id) {
                let c = ss.sketch.curves.get(i)?;
                let solidcraft_engine::sketch::CurveKind::Line { a, b } = c.kind else { return None };
                (ss.plane.to_world(ss.sketch.point(a)?), ss.plane.to_world(ss.sketch.point(b)?))
            } else {
                let w = ss.sketch.wires.get(ss.sketch.wire_index(id)?)?;
                if w.fit.len() != 2 {
                    return None;
                }
                (*w.pts.first()?, *w.pts.last()?)
            };
            Some((pa, (pb - pa).normalized()?))
        }
        // A straight model edge, along it; a circular one, about its axis.
        Sel::Edge { body, index, .. } => {
            let b = app.session.world_state().body(body).cloned()?;
            let m = b.mesh();
            let e = m.edges.get(*index)?;
            let (a, z) = (*e.first()?, *e.last()?);
            let Some(dir) = (z - a).normalized() else { return round_axis(&app.session, sel) };
            let tol = 1e-6 * a.dist(z).max(1.0);
            if !e.iter().all(|p| (*p - a).cross(dir).len() < tol) {
                return round_axis(&app.session, sel);
            }
            // An edge along an origin axis points the way the axis does (as picking the axis).
            let axis = [Vec3::X, Vec3::Y, Vec3::Z].into_iter().find(|x| dir.dot(*x).abs() > 1.0 - 1e-9);
            Some((a, axis.unwrap_or(dir)))
        }
        // A cylinder, cone or torus face: its axis.
        Sel::Face { .. } => round_axis(&app.session, sel),
        _ => None,
    }
}

/// The circle a picked edge lies on, from its exact curve, when the drawn edge lies on it:
/// (centre, unit normal, radius). A round edge stands for its circle wherever a 3D pick wants
/// an axis (through the centre, along the normal) or a point (the centre).
pub(crate) fn edge_circle(s: &Session, body: &str, index: usize) -> Option<(Vec3, Vec3, f64)> {
    edge_circle_in(&s.world_state(), body, index)
}

/// [`edge_circle`] in a given model state (a dialog editing a feature looks at the model before it).
pub(crate) fn edge_circle_in(st: &solidcraft_engine::doc::ModelState, body: &str, index: usize) -> Option<(Vec3, Vec3, f64)> {
    let b = st.body(body)?;
    let c = solidcraft_engine::kernel::edge_circle(&b.body, index)?;
    let tol = (b.body.size() * 1e-6).max(1e-9) * 10.0;
    let on = |p: &Vec3| (p.dist(c.center) - c.radius).abs() <= tol && (*p - c.center).dot(*c.axis).abs() <= tol;
    b.mesh().edges.get(index)?.iter().all(on).then_some((c.center, *c.axis, c.radius))
}

/// The axis of a circular edge (through its centre, along its normal) or of a cylinder, cone or
/// torus face, pointing the way the command will find it (`construct::canonical_dir`). `None`
/// for any other pick: those have no axis of their own.
pub(crate) fn round_axis(s: &Session, sel: &Sel) -> Option<(Vec3, Vec3)> {
    use solidcraft_engine::kernel::{self, FaceSurface};
    let (o, d) = match sel {
        Sel::Edge { body, index, .. } => edge_circle(s, body, *index).map(|(c, n, _)| (c, n))?,
        Sel::Face { body, index, .. } => {
            let st = s.world_state();
            let b = st.body(body)?;
            match kernel::guard("axis of a face", || Ok(kernel::face_surface(&b.body, *index))).ok().flatten()? {
                FaceSurface::Cylinder { origin, axis, .. } => (origin, *axis),
                FaceSurface::Cone { apex, axis, .. } => (apex, *axis),
                FaceSurface::Torus { center, axis, .. } => (center, *axis),
                _ => return None,
            }
        }
        _ => return None,
    };
    o.is_finite().then(|| (o, solidcraft_engine::doc::construct::canonical_dir(d)))
}

/// An axis pick that the command keeps as a reference to the model (`{edge|face: [x,y,z]}`), so
/// the feature follows the edge or face: a circular edge, or a cylinder, cone or torus face.
/// Straight edges stay a fixed line, as they always have.
fn round_axis_ref(app: &SolveApp, sel: &Sel) -> Option<Value> {
    round_axis(&app.session, sel)?;
    match sel {
        Sel::Edge { .. } if !is_straight_edge(&app.session, sel) => construct_ref(app, sel).ok(),
        Sel::Face { .. } => construct_ref(app, sel).ok(),
        _ => None,
    }
}

fn is_straight_edge(s: &Session, sel: &Sel) -> bool {
    let Sel::Edge { body, index, .. } = sel else { return false };
    let Some(b) = s.world_state().body(body).cloned() else { return false };
    let m = b.mesh();
    let Some(e) = m.edges.get(*index) else { return false };
    let (Some(a), Some(z)) = (e.first().copied(), e.last().copied()) else { return false };
    let Some(dir) = (z - a).normalized() else { return false };
    let tol = 1e-6 * a.dist(z).max(1.0);
    e.iter().all(|p| (*p - a).cross(dir).len() < tol)
}

/// Whether an axis input (`selection::AXIS_PICKS`) takes this pick: an edge or face only where
/// it has an axis (a straight or circular edge, a cylinder, cone or torus face).
pub(crate) fn takes_as_axis(s: &Session, sel: &Sel) -> bool {
    match sel {
        Sel::Edge { .. } => is_straight_edge(s, sel) || round_axis(s, sel).is_some(),
        Sel::Face { .. } => round_axis(s, sel).is_some(),
        _ => true,
    }
}

/// An axis input's pick as a command's `axis`: a world axis or construction axis by name, a
/// circular edge or round face by reference, anything else (a sketch line, a straight edge) as
/// its line.
pub(crate) fn axis_param(app: &SolveApp, sel: &Sel) -> Result<Value, String> {
    if let Sel::Axis { name } = sel {
        return Ok(json!(name));
    }
    if let Some(r) = round_axis_ref(app, sel) {
        return Ok(r);
    }
    let (o, dir) = axis_of(app, sel).ok_or(AXIS_PICK)?;
    Ok(json!({"origin": pt(o), "dir": pt(dir)}))
}

const AXIS_PICK: &str =
    "the axis must be an origin or construction axis, a sketch line, a straight or circular edge, or a cylinder, cone or torus face";

/// Which profiles of a sketch a profile selection means.
pub(crate) fn profile_indices(ss: &solidcraft_engine::doc::SolvedSketch, sel: &ProfileSel) -> Vec<usize> {
    let ps = &ss.profiles;
    match sel {
        ProfileSel::All => (0..ps.len()).collect(),
        ProfileSel::Indices { indices } => indices.iter().copied().filter(|i| *i < ps.len()).collect(),
        ProfileSel::OffsetBoundaries { .. } | ProfileSel::WholeBoundaries { .. } => {
            solidcraft_engine::doc::profile_selection_indices(ss, sel).unwrap_or_default()
        }
        ProfileSel::Curves { loops } => loops
            .iter()
            .filter_map(|l| {
                solidcraft_engine::doc::profile_selection_indices(ss, &ProfileSel::Curves { loops: vec![l.clone()] }).ok()?.first().copied()
            })
            .collect(),
        ProfileSel::Points { points } => points.iter().filter_map(|q| solidcraft_engine::doc::sketch::profile_at_point(ps, *q)).collect(),
    }
}

thread_local! {
    /// A dialog is being filled from a feature: its points are in component frames.
    static REFILL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// While filling a dialog from a feature (dropped at the end).
struct Refill;

impl Refill {
    fn start() -> Refill {
        REFILL.with(|r| r.set(true));
        Refill
    }
}

impl Drop for Refill {
    fn drop(&mut self) {
        REFILL.with(|r| r.set(false));
    }
}

/// A point stored in a feature, as it shows on world body `body`: features keep their picks in
/// the frame of the body they were on, so each body's placement takes the point to the world
/// (a moved component, a pasted instance). Other points are the world already.
fn stored_on(frames: &Option<Vec<(String, solidcraft_engine::doc::Mat)>>, body: &str, p: Vec3) -> Vec3 {
    frames.as_ref().and_then(|f| f.iter().find(|(n, _)| n == body)).map(|(_, m)| solidcraft_engine::doc::apply_point(m, p)).unwrap_or(p)
}

/// Body placements while filling a dialog from a feature (when anything is placed).
fn stored_frames(s: &Session) -> Option<Vec<(String, solidcraft_engine::doc::Mat)>> {
    if !REFILL.with(std::cell::Cell::get) || !solidcraft_engine::frames::placed(s) {
        return None;
    }
    let st = s.model.state();
    Some(s.doc.body_frames(&st).into_iter().map(|(w, _, _, m)| (w, m)).collect())
}

/// The edge of a visible body through (or nearest) a point.
pub(crate) fn edge_sel(s: &Session, p0: Vec3) -> Option<Sel> {
    let st = s.world_state();
    let frames = stored_frames(s);
    let mut best: Option<(f64, Sel)> = None;
    for b in &st.bodies {
        let p = stored_on(&frames, &b.name, p0);
        let m = b.mesh();
        for (i, e) in m.edges.iter().enumerate() {
            let d = e.windows(2).map(|w| p.dist_to_segment(w[0], w[1])).fold(f64::INFINITY, f64::min);
            if best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                best = Some((d, Sel::Edge { body: b.name.clone(), index: i, point: p }));
            }
        }
    }
    best.map(|x| x.1)
}

/// The body face containing a point (nearest triangle).
pub(crate) fn face_sel(s: &Session, p0: Vec3) -> Option<Sel> {
    face_sel_in(s, p0, stored_frames(s))
}

/// [`face_sel`] of a point already where the model is shown (not on a body's stored frame).
fn shown_face_sel(s: &Session, p0: Vec3) -> Option<Sel> {
    face_sel_in(s, p0, None)
}

fn face_sel_in(s: &Session, p0: Vec3, frames: Option<Vec<(String, solidcraft_engine::doc::Mat)>>) -> Option<Sel> {
    use solidcraft_engine::kernel::{self, FaceSurface};

    if !p0.is_finite() {
        return None;
    }
    let st = s.world_state();
    let mut best: Option<(f64, &solidcraft_engine::doc::ModelBody, usize, Vec3)> = None;
    for b in &st.bodies {
        let p = stored_on(&frames, &b.name, p0);
        let m = b.mesh();
        for (t, f) in m.triangles.iter().zip(&m.tri_face) {
            let Some([a, bb, c]) = m.tri(t) else { continue };
            let d = crate::preview::point_triangle_dist(p, a, bb, c);
            if best.as_ref().is_none_or(|(bd, _, _, _)| d < *bd) {
                best = Some((d, b, *f as usize, p));
            }
        }
    }
    let (d, body, index, point) = best?;
    let surface = kernel::guard("refill face surface", || Ok(kernel::face_surface(&body.body, index))).ok().flatten();
    // A curved face's viewport pick can be a full chord tolerance from the reference
    // facets. Planar and unclassified faces keep the tight limit so a stale pick cannot
    // silently select a nearby plane, including a flat cap on an otherwise curved body.
    let tolerance = match surface {
        Some(FaceSurface::Plane { .. }) | None => 1e-3,
        Some(_) => body.display_tol(),
    } + 1e-6 * point.len();
    (d <= tolerance).then(|| Sel::Face { body: body.name.clone(), index, point })
}

/// Resolve a stored name on the current input body before placing its pick in the viewport.
/// Keep the resolved index: a face point can lie on a boundary shared with another face.
fn named_refill(s: &Session, body: Option<&str>, name: &str, point: Vec3, edge: bool) -> Option<Sel> {
    use solidcraft_engine::doc::naming;

    if name.is_empty() {
        return None;
    }
    let st = s.model.state();
    let (source, index, point) = if edge {
        let (b, hit) = st
            .bodies
            .iter()
            .filter(|b| body.is_none_or(|name| b.name == name))
            .filter_map(|b| naming::locate_edge(b, name, point).ok().map(|hit| (b, hit)))
            .min_by(|(_, a), (_, b)| a.point.dist(point).total_cmp(&b.point.dist(point)))?;
        (b, hit.edge, hit.point)
    } else if let Some((b, (index, point, _))) = body.and_then(|n| st.body(n)).and_then(|b| naming::face_on_body(b, name, point).map(|hit| (b, hit)))
    {
        (b, index, point)
    } else {
        let hit = naming::locate_face(&st, name, point)?;
        (st.bodies.get(hit.body)?, hit.face, hit.point)
    };
    let (body, point) = s
        .doc
        .body_frames_with_source(&st)
        .into_iter()
        .find(|(_, definition, _, _, _)| definition == source.definition_name())
        .map(|(world, _, _, _, frame)| (world, solidcraft_engine::doc::apply_point(&frame, point)))
        .unwrap_or_else(|| (source.name.clone(), point));
    s.world_state().body(&body)?;
    Some(if edge { Sel::Edge { body, index, point } } else { Sel::Face { body, index, point } })
}

fn emboss_face_sel(s: &Session, component: u64, face: &solidcraft_engine::doc::EmbossFace, persistent: &str) -> Option<Sel> {
    let source = s.model.state();
    let body = source.body(&face.body)?;
    let index = solidcraft_engine::doc::naming::face_names(body).iter().position(|name| name == persistent)?;
    let owner = s.doc.model_body_component(body);
    let world_name = s.doc.world_body_name(&source, &face.body, owner, &face.path);
    let author = solidcraft_engine::doc::EmbossFace::component_frame(&s.doc, component, &face.author_path).ok()?;
    let point = solidcraft_engine::doc::apply_point(&author, face.at.0);
    s.world_state().body(&world_name)?;
    Some(Sel::Face { body: world_name, index, point })
}

/// The construct tools that take only picks (#2474, #2494): their command, dialog title and
/// inputs as (label, what it accepts, parameter). Offset Plane, Midplane, Plane Along Path and
/// Plane At Angle have dialogs of their own.
fn construct_picks(id: &str) -> Option<(&'static str, &'static str, &'static [(&'static str, Accept, &'static str)])> {
    const PLANE: Accept = PLANES | PLANAR_FACES;
    const POINT: Accept = selection::VERTICES | selection::POINTS | selection::CONSTRUCTION_POINTS;
    const LINE: Accept = EDGES | AXES;
    const CURVE: Accept = EDGES | CURVES;
    Some(match id {
        "construct.axis.through_cylinder" => ("construct.axis.through_cylinder", "AXIS THROUGH CYLINDER", &[("Face", FACES | EDGES, "face")]),
        "construct.axis.normal_to_face" => {
            ("construct.axis.normal_to_face", "AXIS PERPENDICULAR TO FACE", &[("Face", FACES, "face"), ("Point", POINT, "point")])
        }
        "construct.axis.two_planes" => {
            ("construct.axis.two_planes", "AXIS THROUGH TWO PLANES", &[("First plane", PLANE, "a"), ("Second plane", PLANE, "b")])
        }
        "construct.axis.two_points" => {
            ("construct.axis.two_points", "AXIS THROUGH TWO POINTS", &[("First point", POINT, "a"), ("Second point", POINT, "b")])
        }
        "construct.axis.edge" => ("construct.axis.edge", "AXIS THROUGH EDGE", &[("Edge", LINE, "edge")]),
        "construct.point.vertex" => ("construct.point.vertex", "POINT AT VERTEX", &[("Point", POINT, "point")]),
        "construct.point.two_edges" => {
            ("construct.point.two_edges", "POINT THROUGH TWO EDGES", &[("First edge", CURVE, "a"), ("Second edge", CURVE, "b")])
        }
        "construct.point.three_planes" => (
            "construct.point.three_planes",
            "POINT THROUGH THREE PLANES",
            &[("First plane", PLANE, "a"), ("Second plane", PLANE, "b"), ("Third plane", PLANE, "c")],
        ),
        "construct.point.center" => ("construct.point.center", "POINT AT CENTER", &[("Circle or sphere", FACES | CURVE, "of")]),
        "construct.point.edge_plane" => {
            ("construct.point.edge_plane", "POINT AT EDGE AND PLANE", &[("Edge", CURVE, "edge"), ("Plane", PLANE, "plane")])
        }
        "construct.point.along_path" => ("construct.point.along_path", "POINT ALONG PATH", &[("Path", CURVE, "path")]),
        "construct.plane.axis_point" => {
            ("construct.plane.axis_point", "cmd.construct.plane.axis_point", &[("Axis", LINE | CURVES, "axis"), ("Point", POINT, "point")])
        }
        "construct.plane.two_edges" => {
            ("construct.plane.two_edges", "PLANE THROUGH TWO EDGES", &[("First edge", LINE, "a"), ("Second edge", LINE, "b")])
        }
        "construct.plane.three_points" => (
            "construct.plane.three_points",
            "PLANE THROUGH THREE POINTS",
            &[("First point", POINT, "a"), ("Second point", POINT, "b"), ("Third point", POINT, "c")],
        ),
        "construct.plane.tangent" => ("construct.plane.tangent", "TANGENT PLANE", &[("Face", FACES, "face")]),
        "construct.plane.perpendicular" => {
            ("construct.plane.perpendicular", "PERPENDICULAR PLANE", &[("Plane", PLANE, "plane"), ("Line", LINE, "line")])
        }
        _ => return None,
    })
}

/// A dialog pick as a construction reference (`construct.*`'s `{face|edge|vertex: [x,y,z]}`,
/// `{plane}`, `{axis}` or `{sketch, point|curve}`).
fn construct_ref(app: &SolveApp, sel: &Sel) -> Result<Value, String> {
    Ok(match sel {
        Sel::Face { point, .. } => json!({"face": pt(*point)}),
        Sel::Edge { point, .. } => json!({"edge": pt(*point)}),
        Sel::Vertex { point, .. } => json!({"vertex": pt(*point)}),
        Sel::Plane { name } => json!({"plane": name}),
        Sel::Axis { name } => json!({"axis": name}),
        Sel::ConstructionPoint { name } => json!({"construct_point": name}),
        // Picked across sketches as "<sketch id>:<point id>".
        Sel::SketchPoint { id } => match id.split_once(':').and_then(|(sk, p)| Some((sk.parse::<u64>().ok()?, p))) {
            Some((sketch, point)) => json!({"sketch": sketch, "point": point}),
            None => return Err("pick the point in the viewport".into()),
        },
        Sel::SketchCurve { id, sketch: Some(sketch) } => json!({"sketch": sketch, "curve": id}),
        Sel::SketchCurve { id, .. } => {
            let st = app.session.world_state();
            let sketches: Vec<u64> = st
                .sketches
                .iter()
                .filter(|ss| ss.sketch.curve_index(id).is_some() || ss.sketch.wire_index(id).is_some())
                .map(|ss| ss.feature)
                .collect();
            match sketches.as_slice() {
                [sketch] => json!({"sketch": sketch, "curve": id}),
                _ => return Err("pick a model edge, or a sketch curve in only one sketch".into()),
            }
        }
        _ => return Err("this pick can't be used here".into()),
    })
}

/// A picked plane or planar face as `construct.plane.*` takes it.
fn plane_arg(app: &SolveApp, sel: Option<&Sel>) -> Result<Value, String> {
    match sel {
        Some(Sel::Plane { name }) => Ok(json!(name)),
        Some(face @ Sel::Face { .. }) => crate::sketch_attachment_ui::face_plane(app, face).ok_or("the picked face is no longer in the model".into()),
        _ => Err("pick an origin or construction plane, or a planar face".into()),
    }
}

/// The point an Offset Plane dialog puts its plane through, when one is picked.
pub(crate) fn offset_through_point(d: &Dialog) -> Option<&Sel> {
    if !matches!(d.kind, Kind::OffsetPlane { .. }) {
        return None;
    }
    sels(d, 1).first()
}

/// A stored point reference back into a dialog pick: a body vertex or a sketch point.
fn point_sel(s: &Session, g: &solidcraft_engine::doc::construct::GeoRef) -> Option<Sel> {
    use solidcraft_engine::doc::construct::GeoRef;
    match g {
        GeoRef::Vertex { at, .. } => match face_sel(s, *at)? {
            Sel::Face { body, .. } => Some(Sel::Vertex { body, point: *at }),
            _ => None,
        },
        GeoRef::SketchPoint { sketch, point } => Some(Sel::SketchPoint { id: format!("{sketch}:{point}") }),
        _ => None,
    }
}

/// A feature's axis on an edge or round face of the model back into a dialog pick: the edge or
/// face where it is now.
pub(crate) fn model_axis_sel(s: &Session, g: &solidcraft_engine::doc::construct::GeoRef) -> Option<Sel> {
    use solidcraft_engine::doc::construct::GeoRef;
    let at = g.picked_point(&s.world_state())?;
    match g {
        GeoRef::Edge { .. } => edge_sel(s, at),
        GeoRef::Face { .. } => face_sel(s, at),
        _ => None,
    }
}

/// A stored construction reference back into a dialog pick.
fn geo_sel(s: &Session, g: &solidcraft_engine::doc::construct::GeoRef) -> Option<Sel> {
    match g {
        solidcraft_engine::doc::construct::GeoRef::Plane { plane } => plane_sel(s, plane),
        solidcraft_engine::doc::construct::GeoRef::Face { at, .. } => face_sel(s, *at),
        _ => None,
    }
}

pub(crate) fn plane_sel(s: &Session, pl: &PlaneRef) -> Option<Sel> {
    match pl {
        PlaneRef::Origin { name } => Some(Sel::Plane { name: name.clone() }),
        PlaneRef::Construction { feature } => s.doc.feature(feature.0).map(|f| Sel::Plane { name: f.name.clone() }),
        PlaneRef::Custom { plane } => face_sel(s, plane.origin),
        // The point picked on the face: the plane's origin can lie off the face (#3567).
        PlaneRef::Face { at, .. } => face_sel(s, *at),
        PlaneRef::Flipped { base, .. } => plane_sel(s, base),
        _ => None,
    }
}

pub(crate) fn op_index(o: &Operation) -> usize {
    match o {
        Operation::NewBody => 0,
        Operation::Join => 1,
        Operation::Cut => 2,
        Operation::Intersect => 3,
    }
}

/// The origin axis a unit direction runs along (either way), if any.
fn world_axis(d: Vec3) -> Option<String> {
    [("X", Vec3::X), ("Y", Vec3::Y), ("Z", Vec3::Z)]
        .into_iter()
        .find(|(_, a)| d.cross(*a).len() < 1e-9 && d.dot(*a) > 0.0)
        .map(|(n, _)| n.to_string())
}

fn pt3(v: Vec3) -> Value {
    json!([v.x, v.y, v.z])
}

/// A point of expressions: numbers as numbers, expressions as strings.
fn pt_expr(p: &solidcraft_engine::doc::PointExpr) -> Value {
    Value::Array(p.iter().map(|e| e.parse::<f64>().ok().filter(|x| x.is_finite()).map_or_else(|| json!(e), |x| json!(x))).collect())
}

/// A dialog that edits an existing feature, filled from it. The timeline is rolled back to just
/// before the feature, so its references show on the geometry they refer to.
pub fn for_feature(app: &SolveApp, id: u64, marker: Option<usize>) -> Option<Dialog> {
    let _refill = Refill::start();
    let s = &app.session;
    let f = s.doc.feature(id)?.clone();
    let st = s.world_state();
    let start = |cmd: &str| Dialog::for_command(app, cmd);
    let mut d = match &f.kind {
        FeatureKind::Extrude { sketch, profiles, extent, operation, targets, thin } => {
            let mut d = start("solid.extrude")?;
            d.kind = Kind::Extrude {
                distance: extent.distance.to_string(),
                direction: match (extent.direction, &extent.distance2) {
                    (Direction::Negative, Some(_)) => TWO_SIDES_REVERSED,
                    (_, Some(_)) => TWO_SIDES,
                    (Direction::Positive, None) => 0,
                    (Direction::Negative, None) => 1,
                    (Direction::Symmetric, None) => 2,
                },
                operation: op_index(operation),
                auto_op: false,
                smart: crate::smart_extrude::Smart { manual_dir: true, auto_to_object: app.preferences.extrude_auto_to_object, ..Default::default() },
                distance2: extent.distance2.as_deref().map(str::to_string).unwrap_or_else(|| length_default(s, "10")),
                taper: extent.taper.as_deref().map(str::to_string).unwrap_or_else(|| "0 deg".into()),
                taper_present: extent.taper.is_some(),
                to_offset: extent.to_offset.as_deref().map(str::to_string).unwrap_or_default(),
                start: extent.start_offset.as_deref().map(str::to_string).unwrap_or_default(),
                all: extent.through_all,
                thin: thin.is_some(),
                wall: thin.as_ref().map_or_else(|| length_default(s, "2"), |t| t.thickness.to_string()),
                wall_side: thin.as_ref().map_or(0, |t| WALL_SIDES.iter().position(|w| ThinSide::parse(w) == Some(t.side)).unwrap_or(0)),
                wall2: thin.as_ref().and_then(|t| t.thickness2.as_deref().map(str::to_string)).unwrap_or_else(|| length_default(s, "1")),
            };
            d.extra.insert("convention".into(), json!(extent.convention));
            // Extent references belong to the extrude's authoring frame, including free
            // points. They are not body-local picks (a reference may be on another body).
            let frame = feature_frame(s, f.component);
            for (label, point) in [(TO_OBJECT, extent.to), (FROM_OBJECT, extent.from)] {
                if let Some(point) = point {
                    set_object_input(&mut d.inputs, &mut d.active, label, true);
                    // The face the point is on, as it was picked: a bare point's chip reads
                    // as sketch coordinates, (0.00, 0.00) mm with no sketch open.
                    let point = solidcraft_engine::doc::apply_point(&frame, point.0);
                    if let Some(input) = d.inputs.iter_mut().find(|i| i.label == label) {
                        input.items = vec![shown_face_sel(s, point).unwrap_or(Sel::Vertex { body: String::new(), point })];
                    }
                }
            }
            if let Some(thin) = thin {
                set_thin(&mut d.inputs, true);
                if let Some(inp) = d.inputs.first_mut() {
                    inp.items = thin.curves.iter().map(|id| crate::selection::curve_pick(id, sketch.0, s.active_sketch)).collect();
                }
            } else {
                let items = st.sketch(sketch.0).map(|ss| profile_indices(ss, profiles)).unwrap_or_default();
                if let Some(inp) = d.inputs.first_mut() {
                    inp.items = items.into_iter().map(|index| Sel::Profile { sketch: sketch.0, index }).collect();
                }
            }
            if !targets.is_empty() {
                d.extra.insert("targets".into(), json!(targets));
            }
            d
        }
        FeatureKind::Revolve { sketch, profiles, axis, angle, operation, targets, angle2, symmetric, convention, to } => {
            let mut d = start("solid.revolve")?;
            let frame = feature_frame(s, f.component);
            let stored_axis = match axis {
                AxisRef::Line { origin, dir } => Some(json!({
                    "origin": pt(solidcraft_engine::doc::apply_point(&frame, origin.0)),
                    "dir": pt(solidcraft_engine::doc::apply_vector(&frame, dir.0))
                })),
                // An edge or face that is gone stays the reference until another is picked.
                AxisRef::Model { geo } if model_axis_sel(s, geo).is_none() => serde_json::to_value(geo).ok(),
                _ => None,
            };
            d.kind = Kind::Revolve { angle: shown_expr(angle, ValueKind::Angle), operation: op_index(operation), axis: stored_axis };
            d.extra.insert("convention".into(), json!(convention));
            if let Some(a2) = angle2 {
                d.extra.insert("angle2".into(), json!(a2));
            }
            if *symmetric {
                d.extra.insert("direction".into(), json!("symmetric"));
            }
            if let Some(point) = to {
                set_to_object(&mut d.inputs, &mut d.active, true);
                let point = solidcraft_engine::doc::apply_point(&frame, point.0);
                if let Some(input) = d.inputs.iter_mut().find(|i| i.label == TO_OBJECT) {
                    input.items = vec![shown_face_sel(s, point).unwrap_or(Sel::Vertex { body: String::new(), point })];
                }
            }
            let items = st.sketch(sketch.0).map(|ss| profile_indices(ss, profiles)).unwrap_or_default();
            if let Some(inp) = d.inputs.get_mut(0) {
                inp.items = items.into_iter().map(|index| Sel::Profile { sketch: sketch.0, index }).collect();
            }
            let ax = match axis {
                AxisRef::World { axis } => Some(Sel::Axis { name: axis.to_ascii_uppercase() }),
                AxisRef::SketchAxis { axis } => Some(Sel::Axis { name: axis.to_ascii_lowercase() }),
                AxisRef::SketchLine { curve } => Some(crate::selection::curve_pick(curve, sketch.0, s.active_sketch)),
                AxisRef::Model { geo } => model_axis_sel(s, geo),
                AxisRef::Construction { name } => Some(Sel::Axis { name: name.to_string() }),
                AxisRef::Line { .. } => None,
            };
            if let Some(inp) = d.inputs.get_mut(1) {
                inp.items = ax.into_iter().collect();
            }
            if !targets.is_empty() {
                d.extra.insert("targets".into(), json!(targets));
            }
            d
        }
        FeatureKind::Fillet { edges, radius, body, .. } | FeatureKind::Chamfer { edges, distance: radius, body, .. } => {
            let edge_pick =
                |i: usize, p: Vec3| f.edge_names.get(i).and_then(|name| named_refill(s, body.as_deref(), name, p, true)).or_else(|| edge_sel(s, p));
            let chamfer = matches!(f.kind, FeatureKind::Chamfer { .. });
            let mut d = start(if chamfer { "solid.chamfer" } else { "solid.fillet" })?;
            d.kind = fillet_kind(&shown_expr(radius, ValueKind::Length), chamfer);
            if let Kind::Fillet { chain, ctype, distance2, angle, flip, .. } = &mut d.kind {
                *chain = false;
                // An unequal chamfer: two distances, or a distance and an angle.
                if let FeatureKind::Chamfer { distance2: d2, angle: a, flip: f, .. } = &f.kind {
                    if let Some(v) = d2 {
                        *ctype = 1;
                        *distance2 = shown_expr(v, ValueKind::Length);
                    } else if let Some(v) = a {
                        *ctype = 2;
                        *angle = shown_expr(v, ValueKind::Angle);
                    }
                    *flip = *f;
                }
            }
            if let FeatureKind::Fillet { corner, .. } = &f.kind
                && !corner.is_rolling()
            {
                d.extra.insert("corner".into(), json!("g2"));
            }
            // A fillet that reduces or tapers a radius too large for its edges.
            if let FeatureKind::Fillet { fit, .. } = &f.kind {
                use solidcraft_engine::doc::FilletFit;
                let mode = match fit {
                    FilletFit::Exact => None,
                    FilletFit::Reduce => Some("reduce"),
                    FilletFit::Taper => Some("taper"),
                };
                if let Some(mode) = mode {
                    d.extra.insert("fit".into(), json!(mode));
                }
            }
            // A chord-length or variable fillet.
            if let FeatureKind::Fillet { style, .. } = &f.kind {
                match style {
                    FilletStyle::Constant => {}
                    FilletStyle::Curvature => {
                        d.extra.insert("type".into(), json!("curvature"));
                    }
                    FilletStyle::Chord => {
                        d.extra.insert("type".into(), json!("chord"));
                    }
                    FilletStyle::Variable { start, .. } | FilletStyle::Spline { start, .. } => {
                        match style {
                            FilletStyle::Variable { radius2, mid, smooth, .. } => {
                                d.extra.insert("type".into(), json!("variable"));
                                d.extra.insert("radius2".into(), json!(radius2));
                                d.extra.insert("mid".into(), json!(mid.iter().map(|(t, r)| json!([t, r])).collect::<Vec<_>>()));
                                d.extra.insert("smooth".into(), json!(smooth));
                            }
                            FilletStyle::Spline { knots, radii, .. } => {
                                d.extra.insert("type".into(), json!("spline"));
                                d.extra.insert("knots".into(), json!(knots));
                                d.extra.insert("radii".into(), json!(radii));
                            }
                            FilletStyle::Constant | FilletStyle::Chord | FilletStyle::Curvature => {}
                        }
                        // The start was the first edge's last point: ends swapped.
                        let first_end = edges.first().and_then(|p| edge_pick(0, p.0)).and_then(|x| match x {
                            Sel::Edge { body, index, .. } => {
                                s.world_state().body(&body).and_then(|b| b.mesh().edges.get(index).and_then(|e| e.last().copied()))
                            }
                            _ => None,
                        });
                        let swapped = first_end.is_some_and(|e| e.dist(start.0) < 1e-6);
                        d.extra.insert("swap_ends".into(), json!(swapped));
                    }
                }
            }
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = edges.iter().enumerate().filter_map(|(i, p)| edge_pick(i, p.0)).collect();
                d.picked_edges = inp.items.clone();
            }
            d.chain_applied = false;
            d
        }
        FeatureKind::Shell { faces, direction, tangent_chain, .. } => {
            let mut d = start("solid.shell")?;
            d.kind = Kind::simple_restore(&f.kind)?;
            d.extra.insert("tangent_chain".into(), json!(tangent_chain));
            if !direction.is_empty() && direction != "inside" {
                d.extra.insert("direction".into(), json!(direction));
            }
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = faces
                    .iter()
                    .enumerate()
                    .filter_map(|(i, p)| {
                        f.face_names.get(i).and_then(|name| named_refill(s, f.kind.face_body(), name, p.0, false)).or_else(|| face_sel(s, p.0))
                    })
                    .collect();
            }
            d
        }
        FeatureKind::Draft { faces, neutral, pull, .. } => {
            let mut d = start("solid.draft")?;
            d.kind = Kind::simple_restore(&f.kind)?;
            if let Some(inp) = d.inputs.get_mut(0) {
                inp.items = faces
                    .iter()
                    .enumerate()
                    .filter_map(|(i, p)| {
                        f.face_names.get(i).and_then(|name| named_refill(s, f.kind.face_body(), name, p.0, false)).or_else(|| face_sel(s, p.0))
                    })
                    .collect();
            }
            if let Some(inp) = d.inputs.get_mut(1) {
                inp.items = plane_sel(s, neutral).into_iter().collect();
            }
            d.extra.insert("pull".into(), pt3(pull.0));
            d
        }
        FeatureKind::Hole { position, at, direction, diameter, depth, hole, points, thread, thread_depth, handedness, to, references } => {
            let mut d = start("solid.hole")?;
            let mut k = hole_defaults(s);
            if let Kind::Hole { diameter: dia, depth: dep, kind, cb_diameter, cb_depth, cs_diameter, cs_angle, opts } = &mut k {
                *dia = diameter.to_string();
                opts.all = depth.is_none();
                opts.angled = matches!(hole, HoleKind::Drilled { .. });
                opts.multiple = points.as_ref().is_some_and(|sp| !sp.single);
                opts.handedness = *handedness;
                if let Some(th) = thread {
                    opts.tapped = true;
                    opts.thread = th.clone();
                }
                opts.thread_full_depth = thread_depth.is_none();
                if let Some(depth) = thread_depth {
                    opts.thread_depth = depth.to_string();
                }
                if let Some(dd) = depth {
                    *dep = dd.to_string();
                }
                match hole {
                    HoleKind::Counterbore { cb_diameter: a, cb_depth: b, tip_angle } => {
                        *kind = 1;
                        *cb_diameter = a.to_string();
                        *cb_depth = b.to_string();
                        if let Some(angle) = tip_angle {
                            opts.angled = true;
                            opts.tip_angle = angle.to_string();
                        }
                    }
                    HoleKind::Countersink { cs_diameter: a, cs_angle: b, tip_angle } => {
                        *kind = 2;
                        *cs_diameter = a.to_string();
                        *cs_angle = b.to_string();
                        if let Some(angle) = tip_angle {
                            opts.angled = true;
                            opts.tip_angle = angle.to_string();
                        }
                    }
                    HoleKind::Drilled { tip_angle } => {
                        opts.tip_angle = tip_angle.to_string();
                    }
                    HoleKind::Simple => {}
                }
            }
            d.kind = k;
            match points {
                Some(sp) if sp.single => {
                    // A Single hole at a sketch point: picked on its face where the point is now.
                    let id = sp.ids.first().cloned().unwrap_or_default();
                    let st = s.world_state();
                    let at = st
                        .sketch(sp.sketch.0)
                        .and_then(|ss| ss.sketch.points.iter().find(|p| p.id == id).map(|p| ss.plane.to_world(p.pos)))
                        .unwrap_or(position.0);
                    if let Some(inp) = d.inputs.first_mut() {
                        inp.items = vec![face_sel(s, at).unwrap_or(Sel::Face { body: String::new(), index: 0, point: at })];
                    }
                    d.extra.insert("direction".into(), pt3(direction.0));
                    d.extra.insert("single_point".into(), json!({"point": pt3(at), "sketch": sp.sketch.0, "id": id}));
                }
                Some(sp) => {
                    d.inputs = vec![SelInput::new("Points", selection::POINTS, true)];
                    if let Some(inp) = d.inputs.first_mut() {
                        inp.items = sp.ids.iter().map(|id| Sel::SketchPoint { id: format!("{}:{id}", sp.sketch) }).collect();
                    }
                    if sp.flip {
                        d.extra.insert("flip".into(), json!(true));
                    }
                }
                None => {
                    if let Some(inp) = d.inputs.first_mut() {
                        inp.items = vec![face_sel(s, position.0).unwrap_or(Sel::Face { body: String::new(), index: 0, point: position.0 })];
                    }
                    d.extra.insert("direction".into(), pt3(direction.0));
                    // A position given as expressions (#1678) is sent again while the pick
                    // stays where the hole is; a new pick places it by the point picked.
                    if let Some(at) = at {
                        let exprs: Vec<&str> = at.iter().map(|e| e.as_str()).collect();
                        d.extra.insert("position_exprs".into(), json!({"at": exprs, "point": pt3(position.0)}));
                    }
                    crate::hole_refs::restore(s, &mut d, references, &f.edge_names, app.preferences.length_decimals);
                }
            }
            if to.is_some() {
                if let Kind::Hole { opts, .. } = &mut d.kind {
                    opts.all = false;
                }
                with_to_object(&mut d, s, to.map(Vec3::from));
            }
            // A hole at every point of a sketch.
            if points.as_ref().is_some_and(|sp| sp.ids.is_empty()) {
                d.extra.insert("all_points".into(), json!(true));
            }
            d
        }
        FeatureKind::Box { corner, length, width, height, operation } => {
            let mut d = start("solid.box")?;
            d.kind = Kind::Primitive {
                cmd: "solid.box",
                fields: vec![("length", stored_length(s, length)), ("width", stored_length(s, width)), ("height", stored_length(s, height))],
                operation: op_index(operation),
                at: None,
                refs: Default::default(),
            };
            d.extra.insert("corner".into(), pt_expr(corner));
            d
        }
        FeatureKind::Cylinder { base, axis, radius, height, operation, references } => {
            let mut d = start("solid.cylinder")?;
            d.kind = Kind::Primitive {
                cmd: "solid.cylinder",
                fields: vec![("radius", stored_length(s, radius)), ("height", stored_length(s, height))],
                operation: op_index(operation),
                at: None,
                refs: Default::default(),
            };
            d.extra.insert("base".into(), pt_expr(base));
            d.extra.insert("axis".into(), pt3(axis.0));
            // Standing on a flat face (a base of numbers there, square to it): placed there, so it
            // can take reference edges of that face (#6380), with its own picked again. Not the
            // picks a selection made when the window opened.
            d.inputs = crate::hole_refs::cylinder_inputs();
            let [x, y, z] = base.clone().map(|c| c.parse::<f64>().ok());
            if let (Some(x), Some(y), Some(z)) = (x, y, z)
                && let Some(face @ Sel::Face { .. }) = face_sel(s, Vec3::new(x, y, z))
                && let Sel::Face { body, index, .. } = &face
                && planar_face(s, body, *index).is_some_and(|(_, n)| n.dot(axis.0) > 1.0 - 1e-9)
                && let Kind::Primitive { at, .. } = &mut d.kind
            {
                *at = Some((Vec3::new(x, y, z), Some(axis.0)));
                if let Some(inp) = d.inputs.first_mut() {
                    inp.items = vec![face];
                }
                crate::hole_refs::restore(s, &mut d, references, &f.edge_names, app.preferences.length_decimals);
            }
            d
        }
        FeatureKind::Sphere { center, radius, operation } => {
            let mut d = start("solid.sphere")?;
            d.kind = Kind::Primitive {
                cmd: "solid.sphere",
                fields: vec![("radius", stored_length(s, radius))],
                operation: op_index(operation),
                at: None,
                refs: Default::default(),
            };
            d.extra.insert("center".into(), pt_expr(center));
            d
        }
        FeatureKind::Torus { center, major, minor, operation } => {
            let mut d = start("solid.torus")?;
            d.kind = Kind::Primitive {
                cmd: "solid.torus",
                fields: vec![("major", stored_length(s, major)), ("minor", stored_length(s, minor))],
                operation: op_index(operation),
                at: None,
                refs: Default::default(),
            };
            d.extra.insert("center".into(), pt_expr(center));
            d
        }
        FeatureKind::Coil {
            base,
            axis,
            sketch,
            profiles,
            diameter,
            pitch,
            turns,
            section_size,
            section,
            position,
            start_angle,
            clockwise,
            operation,
            targets,
        } => {
            let mut d = start("solid.coil")?;
            d.kind = Kind::Primitive {
                cmd: "solid.coil",
                fields: vec![
                    ("diameter", diameter.as_ref().map(|e| stored_length(s, e)).unwrap_or_default()),
                    ("revolutions", turns.to_string()),
                    ("pitch", stored_length(s, pitch)),
                    ("section_size", section_size.as_ref().map(|e| stored_length(s, e)).unwrap_or_default()),
                ],
                operation: op_index(operation),
                at: None,
                refs: Default::default(),
            };
            if let Some(sketch) = sketch {
                let items = st.sketch(sketch.0).map(|ss| profile_indices(ss, profiles)).unwrap_or_default();
                if let Some(inp) = d.inputs.get_mut(0) {
                    inp.items = items.into_iter().map(|index| Sel::Profile { sketch: sketch.0, index }).collect();
                }
            }
            d.extra.insert("base".into(), pt_expr(base));
            d.extra.insert("axis".into(), pt3(axis.0));
            if *section == solidcraft_engine::doc::CoilSection::Square {
                d.extra.insert("section".into(), json!("square"));
            }
            match position {
                ..0 => d.extra.insert("section_position".into(), json!("inside")),
                1.. => d.extra.insert("section_position".into(), json!("outside")),
                0 => None,
            };
            if let Some(a) = start_angle {
                d.extra.insert("start_angle".into(), json!(a.to_string()));
            }
            if *clockwise {
                d.extra.insert("clockwise".into(), json!(true));
            }
            if !targets.is_empty() {
                d.extra.insert("targets".into(), json!(targets));
            }
            d
        }
        FeatureKind::Cone { base, axis, radius, top_radius, height, operation } => {
            let mut d = start("solid.cone")?;
            d.kind = Kind::Primitive {
                cmd: "solid.cone",
                fields: vec![
                    ("radius", stored_length(s, radius)),
                    ("top_radius", stored_length(s, top_radius)),
                    ("height", stored_length(s, height)),
                ],
                operation: op_index(operation),
                at: None,
                refs: Default::default(),
            };
            d.extra.insert("base".into(), pt_expr(base));
            d.extra.insert("axis".into(), pt3(axis.0));
            d
        }
        FeatureKind::BoundaryFill { tools, cells, operation, remove_tools } => {
            let mut d = start("solid.boundary_fill")?;
            // The cells again, ticked where a stored cell point lies in the same cell.
            let before = s.model.state_before(f.id);
            let found = fill_cells(&before, &solidcraft_engine::doc::plain(tools));
            let filled: Vec<String> =
                cells.iter().filter_map(|c| found.iter().min_by(|a, b| a.0.dist(c.0).total_cmp(&b.0.dist(c.0))).map(|x| x.1.clone())).collect();
            let items: Vec<Sel> = tools.iter().map(|n| Sel::Body { name: n.to_string() }).collect();
            d.kind = Kind::BoundaryFill {
                operation: op_index(operation),
                cells: found.into_iter().map(|(p, l)| (p, l.clone(), filled.contains(&l))).collect(),
                of: items.clone(),
            };
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = items;
            }
            if *remove_tools {
                d.extra.insert("remove_tools".into(), json!(true));
            }
            d
        }
        FeatureKind::Combine { target, tools, operation, keep_tools } => {
            let mut d = start("solid.combine")?;
            d.kind = Kind::Combine { operation: op_index(operation).max(1), keep_tools: *keep_tools };
            if let Some(inp) = d.inputs.get_mut(0) {
                inp.items = vec![Sel::Body { name: target.to_string() }];
            }
            if let Some(inp) = d.inputs.get_mut(1) {
                inp.items = tools.iter().map(|n| Sel::Body { name: n.to_string() }).collect();
            }
            d
        }
        FeatureKind::Mirror { features, plane, bodies, placements, combine } => {
            let mut d = start("solid.mirror")?;
            let ids: Vec<u64> = features.iter().map(|f| f.0).collect();
            // Replay references stay features, including features that created a body.
            let as_features = !features.is_empty();
            if let Some(inp) = d.inputs.get_mut(0) {
                if !as_features {
                    // Bodies, though the editor opened with this feature selected in the timeline (#8207).
                    inp.accept = BODIES;
                    inp.label = objects_label(inp.accept);
                }
                if !bodies.is_empty() {
                    // Each body as the placement it was mirrored at shows it (an instance by its world name).
                    let own = s.model.state();
                    let world = |k: usize, name: &str| match (placements.get(k), own.body(name)) {
                        (Some(path), Some(b)) => s.doc.world_body_name(&own, name, s.doc.model_body_component(b), path),
                        _ => name.to_string(),
                    };
                    inp.items = bodies.iter().enumerate().map(|(k, name)| Sel::Body { name: world(k, name) }).collect();
                } else if as_features {
                    inp.accept = selection::FEATURES;
                    inp.label = objects_label(inp.accept);
                    inp.items = ids.iter().map(|id| Sel::Feature { id: *id }).collect();
                } else {
                    inp.items = st.bodies.iter().filter(|b| ids.contains(&b.feature)).map(|b| Sel::Body { name: b.name.clone() }).collect();
                }
            }
            if let Some(inp) = d.inputs.get_mut(1) {
                inp.items = plane_sel(s, plane).into_iter().collect();
            }
            // Body copies are a new body or joined to their source; a feature replay keeps its own operation.
            if !as_features {
                d.extra.insert("combine".into(), json!(combine));
            }
            d
        }
        FeatureKind::Pattern { features, pattern, bodies: copied } => {
            let ids: Vec<u64> = features.iter().map(|f| f.0).collect();
            let bodies: Vec<Sel> = if copied.is_empty() {
                st.bodies.iter().filter(|b| ids.contains(&b.feature)).map(|b| Sel::Body { name: b.name.clone() }).collect()
            } else {
                copied.iter().map(|name| Sel::Body { name: name.to_string() }).collect()
            };
            let mut d = match pattern {
                PatternKind::Rectangular { dir1, count1, spacing1, dir2, count2, spacing2, alignment, alignment2, reversed, reversed2 } => {
                    let mut d = start("solid.pattern.rectangular")?;
                    d.extra.insert("alignment".into(), json!(alignment));
                    d.extra.insert("alignment2".into(), json!(alignment2));
                    d.extra.insert("reversed".into(), json!(reversed));
                    d.extra.insert("reversed2".into(), json!(reversed2));
                    d.kind = Kind::PatternRect {
                        count: count1.to_string(),
                        spacing: spacing1.to_string(),
                        count2: count2.as_deref().map(str::to_string).unwrap_or_else(|| "1".into()),
                        spacing2: spacing2.as_deref().map(str::to_string).unwrap_or_else(|| length_default(s, "20")),
                    };
                    // Directions along the origin axes show as those axes; others are kept as is.
                    for (i, (key, dir)) in [("dir1", Some(*dir1)), ("dir2", *dir2)].into_iter().enumerate() {
                        let Some(dir) = dir else { continue };
                        match world_axis(dir.0) {
                            Some(name) => {
                                if let Some(inp) = d.inputs.get_mut(i + 1) {
                                    inp.items = vec![Sel::Axis { name }];
                                }
                            }
                            None => {
                                d.extra.insert(key.into(), pt3(dir.0));
                            }
                        }
                    }
                    d
                }
                PatternKind::Circular { origin, axis, count, angle, alignment, reversed, axis_ref, axis_feature } => {
                    let mut d = start("solid.pattern.circular")?;
                    d.extra.insert("alignment".into(), json!(alignment));
                    d.extra.insert("reversed".into(), json!(reversed));
                    d.kind = Kind::PatternCirc { count: count.to_string(), angle: angle.to_string() };
                    // The edge or face it turns about, else its construction axis, else an origin
                    // axis or its line.
                    let pick = match (axis_ref, axis_feature) {
                        (Some(g), _) => model_axis_sel(s, g),
                        (None, Some(name)) => Some(Sel::Axis { name: name.to_string() }),
                        (None, None) => world_axis(axis.0).filter(|_| origin.len() < 1e-9).map(|name| Sel::Axis { name }),
                    };
                    match (pick, axis_ref) {
                        (Some(pick), _) => {
                            if let Some(inp) = d.inputs.get_mut(1) {
                                inp.items = vec![pick];
                            }
                        }
                        // An edge or face that is gone stays the reference until another is picked.
                        (None, Some(g)) => {
                            d.extra.insert("axis".into(), serde_json::to_value(g).unwrap_or(Value::Null));
                        }
                        (None, None) => {
                            d.extra.insert("axis".into(), json!({"origin": pt3(origin.0), "dir": pt3(axis.0)}));
                        }
                    }
                    d
                }
                PatternKind::Path { path_sketch, path, count, spacing, extent, flip, orient } => {
                    let mut d = start("solid.pattern.path")?;
                    d.kind = Kind::PathPattern { count: count.to_string(), spacing: spacing.to_string() };
                    if let Some(inp) = d.inputs.get_mut(1) {
                        inp.items = path.iter().map(|id| crate::selection::curve_pick(id, path_sketch.0, s.active_sketch)).collect();
                    }
                    // Preserve options the current dialog does not expose as controls.
                    d.extra.insert("flip".into(), json!(flip));
                    d.extra.insert("orientation".into(), json!(if *orient { "path" } else { "identical" }));
                    if *extent {
                        d.extra.insert("distance".into(), json!(spacing));
                    }
                    d
                }
            };
            // Replay references stay features, including features that created a body.
            let as_features = !features.is_empty();
            if let Some(inp) = d.inputs.first_mut() {
                if as_features {
                    inp.accept = selection::FEATURES;
                    inp.label = objects_label(inp.accept);
                    inp.items = ids.iter().map(|id| Sel::Feature { id: *id }).collect();
                } else {
                    inp.accept = BODIES;
                    inp.label = objects_label(inp.accept);
                    inp.items = bodies;
                }
            }
            d
        }
        FeatureKind::Loft { sections, operation, targets, rails, rail_fit } => {
            let mut d = start("solid.loft")?;
            d.kind = Kind::Loft { operation: op_index(operation), rail_fit: crate::dialogs_sweep::rail_fit_index(*rail_fit) };
            for (inp, items) in d.inputs.iter_mut().skip(1).zip(crate::dialogs_sweep::rail_sels(rails, s.active_sketch)) {
                inp.items = items;
            }
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = sections
                    .iter()
                    .flat_map(|sec| {
                        let idx = st.sketch(sec.sketch.0).map(|ss| profile_indices(ss, &sec.profiles)).unwrap_or_default();
                        idx.into_iter().map(move |index| Sel::Profile { sketch: sec.sketch.0, index })
                    })
                    .collect();
            }
            if !targets.is_empty() {
                d.extra.insert("targets".into(), json!(targets));
            }
            d
        }
        FeatureKind::Sweep { sketch, profiles, path_sketch, path, operation, targets, orientation, twist, scale, rails, rail_fit, .. } => {
            let mut d = start("solid.sweep")?;
            let (opts, direction) = crate::dialogs_sweep::SweepOpts::of(orientation, twist, scale, *rail_fit, &feature_frame(s, f.component));
            d.kind = Kind::Sweep { operation: op_index(operation), opts };
            if let Some(inp) = d.inputs.get_mut(2) {
                inp.items = direction.into_iter().collect();
            }
            for (inp, items) in d.inputs.iter_mut().skip(3).zip(crate::dialogs_sweep::rail_sels(rails, s.active_sketch)) {
                inp.items = items;
            }
            let items = st.sketch(sketch.0).map(|ss| profile_indices(ss, profiles)).unwrap_or_default();
            if let Some(inp) = d.inputs.get_mut(0) {
                inp.items = items.into_iter().map(|index| Sel::Profile { sketch: sketch.0, index }).collect();
            }
            if let Some(inp) = d.inputs.get_mut(1) {
                inp.items = path.iter().map(|id| crate::selection::curve_pick(id, path_sketch.0, s.active_sketch)).collect();
            }
            if !targets.is_empty() {
                d.extra.insert("targets".into(), json!(targets));
            }
            d
        }
        FeatureKind::ConstructionPlane { plane: PlaneRef::Offset { base, .. } } => {
            let mut d = start("construct.plane.offset")?;
            d.kind = Kind::simple_restore(&f.kind)?;
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = plane_sel(s, base).into_iter().collect();
            }
            d
        }
        FeatureKind::ConstructionPlane { plane: PlaneRef::ThroughPoint { base, point } } => {
            let mut d = start("construct.plane.offset")?;
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = plane_sel(s, base).into_iter().collect();
            }
            if let Some(inp) = d.inputs.get_mut(1) {
                inp.items = point_sel(s, point).into_iter().collect();
            }
            d
        }
        FeatureKind::ConstructionPlane { plane: PlaneRef::AlongPath { path, t } } => {
            let mut d = start("construct.plane.along_path")?;
            let (item, sketch) = match path.as_ref() {
                solidcraft_engine::doc::construct::GeoRef::SketchCurve { sketch, curve } => {
                    (crate::selection::curve_pick(curve, *sketch, s.active_sketch), Some(*sketch))
                }
                solidcraft_engine::doc::construct::GeoRef::Edge { at, name } => {
                    // Preserve the saved named reference until a replacement is picked.
                    d.extra.insert("path".into(), json!({"type":"edge","at":pt3(*at),"name":name}));
                    (Sel::Edge { body: String::new(), index: 0, point: *at }, None)
                }
                _ => return None,
            };
            d.kind = Kind::PathPlane { t: t.to_string(), sketch };
            if let Some(input) = d.inputs.first_mut() {
                input.items = vec![item];
            }
            d
        }
        FeatureKind::ConstructionPlane { plane: PlaneRef::AxisPoint { .. } } => {
            let mut world = f.kind.clone();
            world.transform_geometry(&feature_frame(s, f.component));
            let FeatureKind::ConstructionPlane { plane: PlaneRef::AxisPoint { axis, point } } = world else { return None };
            let mut d = start("construct.plane.axis_point")?;
            for (input, reference) in d.inputs.iter_mut().zip([&axis, &point]) {
                // Keep the exact saved references when no new pick has replaced them.
                // Fixed points also need a visible pick to keep the shared form complete.
                input.items = vec![match reference.as_ref() {
                    solidcraft_engine::doc::construct::GeoRef::World { axis } => Sel::Axis { name: axis.clone() },
                    solidcraft_engine::doc::construct::GeoRef::Axis { name } => Sel::Axis { name: name.to_string() },
                    solidcraft_engine::doc::construct::GeoRef::ConstructPoint { name } => Sel::ConstructionPoint { name: name.to_string() },
                    solidcraft_engine::doc::construct::GeoRef::SketchPoint { sketch, point } => Sel::SketchPoint { id: format!("{sketch}:{point}") },
                    solidcraft_engine::doc::construct::GeoRef::SketchCurve { sketch, curve } => {
                        Sel::SketchCurve { id: curve.clone(), sketch: Some(*sketch) }
                    }
                    solidcraft_engine::doc::construct::GeoRef::Vertex { at, .. } => Sel::Vertex { body: String::new(), point: *at },
                    solidcraft_engine::doc::construct::GeoRef::Point { p } => Sel::Vertex { body: String::new(), point: *p },
                    solidcraft_engine::doc::construct::GeoRef::Origin => Sel::Vertex { body: String::new(), point: Vec3::ZERO },
                    other => model_axis_sel(s, other)?,
                }];
            }
            d.extra.insert("axis".into(), serde_json::to_value(axis).ok()?);
            d.extra.insert("point".into(), serde_json::to_value(point).ok()?);
            d
        }
        FeatureKind::ConstructionPlane { plane: PlaneRef::Midplane { a, b } } => {
            let mut d = start("construct.plane.midplane")?;
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = geo_sel(s, a).into_iter().collect();
            }
            if let Some(inp) = d.inputs.get_mut(1) {
                inp.items = geo_sel(s, b).into_iter().collect();
            }
            d
        }
        FeatureKind::ConstructionPlane { plane: PlaneRef::AtAngle { base, axis_origin, axis_dir, line, .. } } => {
            let mut d = start("construct.plane.at_angle")?;
            d.kind = Kind::simple_restore(&f.kind)?;
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = plane_sel(s, base).into_iter().collect();
            }
            let pick = match line {
                Some(g) => model_axis_sel(s, g),
                None => world_axis(*axis_dir).filter(|_| axis_origin.len() < 1e-9).map(|name| Sel::Axis { name }),
            };
            match (pick, line) {
                (Some(pick), _) => {
                    if let Some(inp) = d.inputs.get_mut(1) {
                        inp.items = vec![pick];
                    }
                }
                (None, Some(g)) => {
                    d.extra.insert("axis".into(), serde_json::to_value(g).unwrap_or(Value::Null));
                }
                (None, None) => {
                    d.extra.insert("axis".into(), json!({"origin": pt3(*axis_origin), "dir": pt3(*axis_dir)}));
                }
            }
            d
        }
        FeatureKind::Split { body, plane, tool, sketch, line } => {
            let mut d = start("solid.split_body")?;
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = vec![Sel::Body { name: body.to_string() }];
            }
            if let (Some(k), Some(id)) = (sketch, line) {
                d.extra.insert("tool_sketch".into(), json!(k.0));
                if let Some(inp) = d.inputs.get_mut(1) {
                    inp.items = vec![crate::selection::curve_pick(id, k.0, s.active_sketch)];
                }
            } else if let Some(inp) = d.inputs.get_mut(1) {
                inp.items = match tool {
                    Some(t) => face_sel(s, t.point.0).into_iter().collect(),
                    None => plane_sel(s, plane).into_iter().collect(),
                };
            }
            d
        }
        FeatureKind::Scale { bodies, origin, factors, .. } => {
            let mut d = start("solid.scale")?;
            d.kind = Kind::simple_restore(&f.kind)?;
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = bodies.iter().map(|n| Sel::Body { name: n.to_string() }).collect();
            }
            d.extra.insert("origin".into(), pt3(origin.0));
            if let Some(f) = factors {
                d.extra.insert("factors".into(), json!(f));
            }
            d
        }
        FeatureKind::OffsetFace { faces, body, .. } => {
            let mut d = start("solid.offset_face")?;
            d.kind = Kind::simple_restore(&f.kind)?;
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = faces.iter().filter_map(|p| face_sel(s, p.0)).collect();
            }
            if let Some(b) = body {
                d.extra.insert("body".into(), json!(b));
            }
            d
        }
        FeatureKind::Thread { face, designation, length, modeled, handedness } => {
            let mut d = start("solid.thread")?;
            if *modeled {
                d.extra.insert("modeled".into(), json!(true));
            }
            d.kind = Kind::Thread {
                designation: designation.clone().unwrap_or_default(),
                length: length.as_deref().map(|e| shown_expr(e, ValueKind::Length)).unwrap_or_default(),
                handedness: *handedness,
                suggested: None,
                of: Vec::new(),
            };
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = face_sel(s, face.0).into_iter().collect();
            }
            d.sync_thread(s);
            d
        }
        FeatureKind::BoundingSolid { bodies, .. } => {
            let mut d = start("solid.bounding_solid")?;
            d.kind = Kind::simple_restore(&f.kind)?;
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = bodies.iter().map(|n| Sel::Body { name: n.to_string() }).collect();
            }
            d
        }
        FeatureKind::Pipe { path_sketch, path, targets, .. } => {
            let mut d = start("solid.pipe")?;
            d.kind = Kind::simple_restore(&f.kind)?;
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = path.iter().map(|id| crate::selection::curve_pick(id, path_sketch.0, s.active_sketch)).collect();
            }
            if !targets.is_empty() {
                d.extra.insert("targets".into(), json!(targets));
            }
            d
        }
        FeatureKind::Emboss { sketch, profiles, targets, face, .. } => {
            let mut d = start("solid.emboss")?;
            d.kind = Kind::simple_restore(&f.kind)?;
            let items = st.sketch(sketch.0).map(|ss| profile_indices(ss, profiles)).unwrap_or_default();
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = items.into_iter().map(|index| Sel::Profile { sketch: sketch.0, index }).collect();
            }
            if !targets.is_empty() {
                d.extra.insert("targets".into(), json!(targets));
            }
            if let Some(face) = face
                && let Some(input) = d.inputs.get_mut(1)
            {
                input.items = vec![emboss_face_sel(s, f.component, face, f.face_names.first()?)?];
            }
            d
        }
        FeatureKind::Rib { sketch, curves, thickness, depth, flip, web, normal, .. } => {
            let mut d = start(if *web { "solid.web" } else { "solid.rib" })?;
            d.kind = Kind::Rib {
                web: *web,
                thickness: shown_expr(thickness, ValueKind::Length),
                depth: depth.as_deref().map(|e| shown_expr(e, ValueKind::Length)).unwrap_or_default(),
                flip: *flip,
                normal: *normal,
            };
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = curves.iter().map(|id| crate::selection::curve_pick(id, sketch.0, s.active_sketch)).collect();
            }
            d
        }
        FeatureKind::ReplaceFace { faces, target, body } => {
            let mut d = start("solid.replace_face")?;
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = faces.iter().filter_map(|p| face_sel(s, p.0)).collect();
            }
            if let Some(inp) = d.inputs.get_mut(1) {
                inp.items = plane_sel(s, target).into_iter().collect();
            }
            if let Some(b) = body {
                d.extra.insert("body".into(), json!(b));
            }
            d
        }
        FeatureKind::DeleteFace { faces, body, heal } => {
            let mut d = start("face.delete")?;
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = faces.iter().filter_map(|p| face_sel(s, p.0)).collect();
            }
            if let Some(b) = body {
                d.extra.insert("body".into(), json!(b));
            }
            d.extra.insert("heal".into(), json!(heal));
            d
        }
        FeatureKind::Remove { bodies } => {
            let mut d = start("solid.remove")?;
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = bodies.iter().map(|n| Sel::Body { name: n.to_string() }).collect();
            }
            d
        }
        FeatureKind::Move { bodies, translate, rotate_axis, angle, copy } => {
            let mut d = start("solid.move")?;
            // Move stores distances in the first body's frame, which can differ from the
            // feature's component. Refill the world distances that the command accepts.
            let frame = bodies
                .first()
                .and_then(|body| {
                    s.placed_document()
                        .body_frames_with_source(&s.model.state())
                        .into_iter()
                        .find(|(world, own, ..)| world == &**body || own == &**body)
                        .map(|(.., frame)| frame)
                })
                .unwrap_or_else(|| feature_frame(s, f.component));
            let mut world_translate = translate.clone();
            if !solidcraft_engine::doc::is_identity(&frame) {
                solidcraft_engine::doc::map_vector_expr(&frame, &mut world_translate);
            }
            crate::dialogs_move::for_feature(
                &s.doc,
                &mut d,
                &solidcraft_engine::doc::plain(bodies),
                &world_translate,
                rotate_axis.map(Vec3::from),
                angle.as_ref(),
                *copy,
            );
            d
        }
        other => {
            let (kind, inputs, extra) = crate::dialogs_sheet::for_feature(app, other)
                .or_else(|| crate::dialogs_plastic::for_feature(app, other))
                .or_else(|| crate::dialogs_frame::for_feature(app, other))
                .or_else(|| crate::dialogs_surface::for_feature(app, other, &feature_frame(s, f.component)))?;
            let mut d = Dialog::new(kind, inputs);
            d.extra = extra;
            d
        }
    };
    if crate::length_input::unit(&s.doc) != "mm" {
        for value in crate::length_input::fields(&s.doc, &mut d.kind) {
            *value = crate::length_input::stored(&s.doc, value);
        }
        crate::length_input::extra(&mut d.extra, |e| crate::length_input::stored(&s.doc, e));
    }
    if let FeatureKind::Extrude { sketch, .. } = &f.kind
        && let Some(FeatureKind::Sketch { plane, .. }) = s.doc.feature(sketch.0).map(|f| &f.kind)
    {
        let mut plane = plane;
        // Plane nesting comes from design files: bound the walk just like other reference walks.
        for _ in 0..64 {
            match plane {
                PlaneRef::Offset { base, .. } | PlaneRef::AtAngle { base, .. } | PlaneRef::Flipped { base, .. } => plane = base,
                PlaneRef::Face { anchor: Some(anchor), .. } => {
                    let model = s.model.state();
                    if let Some(body) = model.body(&anchor.target.body) {
                        let owner = s.doc.model_body_component(body);
                        let name = s.doc.world_body_name(&model, &body.name, owner, &anchor.path);
                        if st.body(&name).is_some() && !s.body_visible(&name) {
                            d.edit_source_bodies.push(name);
                        }
                    }
                    break;
                }
                _ => break,
            }
        }
    }
    d.editing = Some((id, marker));
    d.active = 0;
    d.error = None;
    Some(d)
}

#[cfg(test)]
mod param_refusal_tests {
    use super::*;
    use solidcraft_engine::Session;

    #[test]
    fn a_refused_parameter_change_shows_why() {
        // A triangle whose sides are parameters: a side longer than the other two together
        // has no solution, so the change is refused and the table shows the reason.
        let mut s = Session::default();
        s.execute("sketch.create", &json!({"plane": "XY", "name": "Tri"})).unwrap();
        s.execute("sketch.line", &json!({"points": [[0, 0], [40, 0], [20, 30]], "closed": true})).unwrap();
        for l in ["l1", "l2", "l3"] {
            s.execute("sketch.dimension", &json!({"entities": [l]})).unwrap();
        }
        s.execute("sketch.finish", &json!({})).unwrap();
        let name = s.doc.params.iter().find(|p| p.model).map(|p| p.name.clone()).unwrap();
        let before = s.doc.param(&name).map(|p| p.expr.clone());
        let mut app = SolveApp::new(s, Default::default());
        let why = app.run("parameters.change", json!({"name":name,"expression":"500 mm"})).unwrap_err();
        assert!(why.contains("Tri") && why.contains("conflict"), "{why}");
        assert_eq!(app.session.doc.param(&name).map(|p| p.expr.clone()), before);
        assert!(app.status.as_ref().is_some_and(|(m, _, error)| *error && m.contains("Tri")), "{:?}", app.status);
        assert!(app.run("parameters.change", json!({"name":name,"expression":"41 mm"})).is_ok());
    }
}

#[cfg(test)]
mod refused_ok_tests {
    //! A refused OK's error lasts only as long as the inputs it was about (#641).

    use super::*;
    use crate::scenario::Harness;

    fn steps(h: &mut Harness, steps: &[Value]) {
        for s in steps {
            h.step(s).unwrap();
        }
    }

    #[test]
    fn selected_plane_axis_and_angle_build_a_preview() {
        let app = SolveApp::new(Session::default(), Default::default());
        let mut dialog = Dialog::for_command(&app, "construct.plane.at_angle").expect("the command has a dialog");
        dialog.inputs[0].items = vec![Sel::Plane { name: "XY".into() }];
        dialog.inputs[1].items = vec![Sel::Axis { name: "X".into() }];
        if let Kind::AnglePlane { angle } = &mut dialog.kind {
            *angle = "30 deg".into();
        }

        assert!(dialog.previews());
        let commands = preview_commands(&app, &dialog).expect("the selected inputs form a command");
        assert_eq!(commands.first().map(|(_, params)| params["angle"].as_str()), Some(Some("30 deg")));
        let preview = crate::preview::compute(
            &app.session,
            &commands,
            crate::preview::Colors {
                body: [1; 4],
                added: [2; 4],
                new_body: [3; 4],
                removed: [4; 4],
                edge: [5; 4],
                ghost: false,
                tint_new: true,
                cut_tint: true,
            },
        )
        .expect("the angled plane previews");

        assert!(!preview.scene.xray.is_empty(), "the preview contains the translucent construction plane");
    }

    /// A box, and a hole dialog with a position picked on its top.
    fn hole_dialog() -> Harness {
        let mut h = Harness::new();
        steps(
            &mut h,
            &[
                json!({"call": "engine.execute", "params": {"command": "solid.box", "params": {"length": 50, "width": 30, "height": 20}}}),
                json!({"call": "ui.view", "params": {"view": "iso"}}),
                json!({"start": "solid.hole"}),
                json!({"click": {"world": [25, 15, 20]}}),
            ],
        );
        h
    }

    /// A sketch, a second sketch after it, and `command` made from the first; the edit dialog
    /// of that feature open, so the history marker sits on the second sketch.
    fn edit_dialog(command: &str, params: Value) -> (Harness, u64) {
        let mut h = Harness::new();
        let mut sketches = Vec::new();
        for plane in ["XY", "XZ"] {
            let created = h.app.run("sketch.create", json!({"plane": plane})).unwrap();
            sketches.push(created["sketch"].as_u64().unwrap());
            h.app.run("sketch.rectangle.two_point", json!({"p0": [2, 0], "p1": [4, 3]})).unwrap();
            h.app.run("sketch.finish", json!({})).unwrap();
        }
        let mut params = params;
        params["sketch"] = json!(sketches[0]);
        h.app.run(command, params).unwrap();
        let feature = h.app.session.doc.features.last().unwrap().id;
        assert!(!solidcraft_engine::view::sketch_shown(&h.app.session, sketches[0]));
        h.app.edit_feature(feature);
        h.frames(1);
        assert_eq!(h.app.session.feature_edit_sketch, Some(sketches[0]), "{command}");
        assert!(solidcraft_engine::view::sketch_shown(&h.app.session, sketches[0]), "{command}");
        (h, sketches[0])
    }

    #[test]
    fn editing_shows_the_sketch_until_the_dialog_closes() {
        let closes: [(&str, fn(&mut Harness)); 5] = [
            ("OK", |h| steps(h, &[json!({"key": "Enter"})])),
            ("Esc", |h| steps(h, &[json!({"key": "Escape"})])),
            ("Cancel", |h| cancel(&mut h.app)),
            ("another tool", |h| steps(h, &[json!({"start": "solid.box"})])),
            ("new design", |h| {
                h.app.run("file.new", json!({})).unwrap();
            }),
        ];
        for (command, params) in [("solid.extrude", json!({"distance": 5})), ("solid.revolve", json!({"axis": "y"}))] {
            for (how, close) in closes {
                let (mut h, sketch) = edit_dialog(command, params.clone());
                close(&mut h);
                h.frames(1);
                assert!(h.app.dialog.as_ref().is_none_or(|d| d.editing.is_none()), "{command} {how}");
                assert_eq!(h.app.session.feature_edit_sketch, None, "{command} {how}");
                // A new design has no such sketch; in the edited one it is hidden again.
                if h.app.session.doc.feature(sketch).is_some() {
                    assert!(!solidcraft_engine::view::sketch_shown(&h.app.session, sketch), "{command} {how}");
                }
            }
        }
    }

    /// One Escape closes an edit dialog opened from its History row, also when its value names a parameter
    /// and the field lists the longer names (`T` lists `tan(`, #9084).
    #[test]
    fn one_escape_closes_an_edit_dialog_whose_value_names_a_parameter() {
        for name in ["Plate", "Corners", "Hole"] {
            let mut h = Harness::new();
            for (command, params) in [
                ("parameters.add", json!({"name": "T", "expression": "3 mm"})),
                ("parameters.add", json!({"name": "R", "expression": "5 mm"})),
                ("sketch.create", json!({"plane": "XY"})),
                ("sketch.rectangle.two_point", json!({"p0": [-40, -25], "p1": [40, 25]})),
                ("sketch.finish", json!({})),
                ("solid.extrude", json!({"distance": "T", "name": "Plate"})),
                ("solid.fillet", json!({"edges": [[40, 25, 1.5], [-40, 25, 1.5]], "radius": "R", "name": "Corners"})),
                ("solid.hole", json!({"position": [0, 0, 3], "diameter": 3.4, "name": "Hole"})),
            ] {
                h.app.run(command, params).unwrap();
            }
            h.frames(2);
            steps(&mut h, &[json!({"click": {"handle": format!("timeline:{name}")}, "double": true}), json!({"frames": 30})]);
            assert!(h.app.dialog.as_ref().is_some_and(|d| d.editing.is_some()), "{name}: the edit dialog opened");
            if name != "Hole" {
                assert!(h.ctx.memory(|m| m.focused()).is_some(), "{name}: its value field has the keyboard");
                assert!(crate::expression_complete::popup_state(&h.ctx).is_some(), "{name}: the field lists longer names");
            }
            steps(&mut h, &[json!({"key": "Escape"}), json!({"frames": 2})]);
            assert!(h.app.dialog.as_ref().is_none_or(|d| d.editing.is_none()), "{name}: one Escape closes the dialog");
        }
    }

    fn set_depth(h: &mut Harness, depth: &str) {
        steps(h, &[json!({"click": {"handle": "dialog:Depth"}}), json!({"key": "A", "cmd": true}), json!({"text": depth})]);
    }

    #[test]
    fn recessed_drill_point_controls_and_payload_follow_the_bottom_choice() {
        for recessed in [1, 2] {
            let mut h = hole_dialog();
            if let Kind::Hole { kind, opts, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
                *kind = recessed;
                opts.angled = true;
                opts.tip_angle = "118 deg".into();
            }
            h.frames(2);
            assert!(crate::scenario::rect_of("dialog:widget:Drill Point").is_some());
            assert!(crate::scenario::rect_of("dialog:widget:Tip Angle").is_some());
            let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
            assert_eq!(commands[0].1["tip_angle"], "118 deg");
            if let Kind::Hole { opts, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
                opts.angled = false;
            }
            h.frames(2);
            assert!(crate::scenario::rect_of("dialog:widget:Tip Angle").is_none());
            assert!(apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap()[0].1.get("tip_angle").is_none());
            if let Kind::Hole { opts, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
                opts.all = true;
            }
            h.frames(2);
            assert!(crate::scenario::rect_of("dialog:widget:Drill Point").is_none());
        }
    }

    /// What the dialog shows in red once the live preview has finished: its own error, else the
    /// preview's.
    fn shown(h: &mut Harness) -> Option<String> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while h.app.preview.busy {
            assert!(std::time::Instant::now() < deadline, "the preview did not finish");
            h.frames(1);
        }
        h.app.dialog.as_ref().and_then(|d| d.error.clone()).or_else(|| h.app.preview.error.clone())
    }

    fn refused(h: &mut Harness) {
        set_depth(h, "-5");
        steps(h, &[json!({"key": "Enter"})]);
        assert_eq!(h.app.dialog.as_ref().and_then(|d| d.error.as_deref()), Some("hole depth must be positive"));
    }

    #[test]
    fn a_refusal_is_shown_without_the_engine_prefix() {
        assert_eq!(
            super::shown_error("invalid parameters for `sheet.flange`: there is no sheet metal body: start one with Flange > Base"),
            "there is no sheet metal body: start one with Flange > Base"
        );
        assert_eq!(super::shown_error("hole depth must be positive"), "hole depth must be positive");
    }

    #[test]
    fn the_error_stays_while_the_inputs_do() {
        let mut h = hole_dialog();
        refused(&mut h);
        h.frames(10);
        assert_eq!(shown(&mut h).as_deref(), Some("hole depth must be positive"));
    }

    #[test]
    fn a_value_fixed_in_the_dialog_clears_it() {
        let mut h = hole_dialog();
        refused(&mut h);
        set_depth(&mut h, "5");
        h.frames(2);
        assert_eq!(shown(&mut h), None);
    }

    /// The canvas value box and the arrows change the dialog before `show` runs in a frame.
    #[test]
    fn a_value_fixed_outside_the_dialog_clears_it() {
        let mut h = hole_dialog();
        refused(&mut h);
        if let Some(Kind::Hole { depth, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            *depth = "5 mm".into();
        }
        h.frames(2);
        assert_eq!(shown(&mut h), None);
    }

    #[test]
    fn a_refusal_for_a_missing_pick_clears_once_it_is_picked() {
        let mut h = Harness::new();
        steps(
            &mut h,
            &[
                json!({"call": "engine.execute", "params": {"command": "solid.box", "params": {"length": 50, "width": 30, "height": 20}}}),
                json!({"call": "ui.view", "params": {"view": "iso"}}),
                json!({"start": "solid.hole"}),
                json!({"key": "Enter"}),
            ],
        );
        let error = h.app.dialog.as_ref().and_then(|d| d.error.clone());
        assert!(error.as_deref().is_some_and(|e| e.contains("first")), "{error:?}");
        steps(&mut h, &[json!({"click": {"world": [25, 15, 20]}})]);
        h.frames(2);
        assert_eq!(shown(&mut h), None);
    }

    /// A parameter the depth names changes the design, not the dialog's commands.
    #[test]
    fn a_parameter_the_inputs_use_clears_it() {
        let mut h = hole_dialog();
        let add = json!({"command": "parameters.add", "params": {"name": "dep", "expression": "-5 mm"}});
        steps(&mut h, &[json!({"call": "engine.execute", "params": add})]);
        set_depth(&mut h, "dep");
        steps(&mut h, &[json!({"key": "Enter"})]);
        assert!(h.app.dialog.as_ref().and_then(|d| d.error.as_deref()).is_some_and(|e| e.contains("depth")));
        let change = json!({"command": "parameters.change", "params": {"name": "dep", "expression": "5 mm"}});
        steps(&mut h, &[json!({"call": "engine.execute", "params": change})]);
        h.frames(2);
        assert_eq!(shown(&mut h), None);
    }

    /// Picks changed some other way than a click (a box selection, a cleared chip) are seen too,
    /// and the dialog then says what the inputs lack now.
    #[test]
    fn a_pick_changed_outside_the_dialog_shows_what_is_missing_now() {
        let mut h = hole_dialog();
        refused(&mut h);
        if let Some(input) = h.app.dialog.as_mut().and_then(|d| d.inputs.first_mut()) {
            input.items.clear();
        }
        h.frames(2);
        let error = h.app.dialog.as_ref().and_then(|d| d.error.clone());
        assert!(error.as_deref().is_some_and(|e| e.contains("first")), "{error:?}");
    }

    /// Editing an existing hole: the refusal clears, and OK then edits that hole.
    #[test]
    fn editing_a_hole_clears_it_and_keeps_the_feature() {
        let mut h = Harness::new();
        let hole = json!({"command": "solid.hole", "params": {"position": [25, 15, 20], "diameter": "5 mm", "depth": "10 mm", "name": "Bore"}});
        steps(
            &mut h,
            &[
                json!({"call": "engine.execute", "params": {"command": "solid.box", "params": {"length": 50, "width": 30, "height": 20}}}),
                json!({"call": "engine.execute", "params": hole}),
                json!({"call": "ui.view", "params": {"view": "iso"}}),
                json!({"call": "ui.editFeature", "params": {"feature": "Bore"}}),
            ],
        );
        let features = h.app.session.doc.features.len();
        refused(&mut h);
        assert!(h.app.dialog.as_ref().is_some_and(|d| d.editing.is_some()));
        set_depth(&mut h, "5");
        h.frames(2);
        assert_eq!(shown(&mut h), None);
        steps(&mut h, &[json!({"key": "Enter"})]);
        assert!(h.app.dialog.is_none());
        assert_eq!(h.app.session.doc.features.len(), features);
        let bore = h.app.session.execute("timeline.inspect", &json!({"feature": "Bore"})).unwrap();
        assert!(bore.to_string().contains("\"depth\":\"5\""), "{bore}");
    }
}

/// Whether two picks are the same face of the same body (wherever on it they were made).
fn same_face(a: &Sel, b: &Sel) -> bool {
    matches!((a, b), (Sel::Face { body: ba, index: ia, .. }, Sel::Face { body: bb, index: ib, .. }) if ba == bb && ia == ib)
}

/// Where a face was picked.
fn face_point(x: &Sel) -> Option<Value> {
    if let Sel::Face { point, .. } = x { Some(pt(*point)) } else { None }
}

#[cfg(test)]
mod awaiting_selection_tests {
    use super::{awaiting_selection, missing_pick, reveal_refusal};

    #[test]
    fn missing_selection_messages_are_guidance_and_real_failures_are_not() {
        for guidance in [
            "Select the curves to offset first",
            "Select the mirror line",
            "select the corner to cut first",
            "select the corners to round first",
            "select corner points, or the two lines of one corner",
        ] {
            assert!(awaiting_selection(guidance), "{guidance}");
        }
        for failure in ["Offset distance must be positive", "the sketch has no curves", "Selection is not on this sketch", "", "sel"] {
            assert!(!awaiting_selection(failure), "{failure}");
            assert!(!missing_pick(failure), "{failure}");
        }
        assert!(missing_pick("pick an axis") && missing_pick("select a plane first") && !missing_pick("pickle"));
    }

    /// Plane at Angle's OK with nothing picked says what to pick, without the red error and its
    /// Create Issue button that made people file it as a bug (#3292, #3773).
    #[test]
    fn a_missing_pick_is_guidance_without_an_issue_button() {
        let mut h = crate::scenario::Harness::new();
        h.step(&serde_json::json!({"start":"construct.plane.at_angle"})).unwrap();
        h.frames(2);
        h.step(&serde_json::json!({"key":"Enter"})).unwrap();
        h.frames(2);
        let d = h.app.dialog.as_ref().expect("the dialog stays open");
        assert_eq!(d.error.as_deref(), Some("select a plane first"));
        assert!(crate::scenario::handle_at("dialog:create_issue").is_none(), "no Create Issue for a missing pick");
    }

    /// A long refusal sits under the fold of a dialog its fields fill, so only its first line
    /// showed, or none, and no Create Issue (#8454). The dialog scrolls it into view once: a
    /// refusal on OK at once, a preview's once no field is typed in.
    #[test]
    fn a_long_refusal_scrolls_into_view_with_its_issue_button() {
        let reason = "boolean Cut: the operation failed: no result; the operation failed: not supported yet: the exact fallback for \
                      where the bodies meet (faces that touch, or cross at a shallow angle) takes planar faces only, and the target \
                      has 24 curved faces (cylinder, torus) and the tool has 8 curved faces (cylinder); "
            .repeat(3);
        let mut h = crate::scenario::Harness::new();
        h.step(&serde_json::json!({"start":"solid.extrude"})).unwrap();
        h.frames(2);
        let field = crate::scenario::rect_of("dialog:control:Distance").expect("the Distance field").center();
        h.step(&serde_json::json!({"click": [field.x, field.y]})).unwrap();
        if let Some(d) = h.app.dialog.as_mut() {
            d.error = Some(reason.clone());
        }
        // The scroll is animated.
        h.frames(30);
        let viewport = crate::scenario::rect_of("dialog:body:viewport").expect("the dialog's fields");
        let issue = crate::scenario::handle_at("dialog:create_issue").expect("Create Issue");
        let error = crate::scenario::rect_of("dialog:report:error").expect("the refusal");
        assert!(viewport.contains(issue) && viewport.contains_rect(error), "{error:?} and Create Issue at {issue:?} in {viewport:?}");
        assert!(reveal_refusal(true, true, None, &reason), "a refusal on OK, while a field is typed in");
        assert!(!reveal_refusal(false, true, None, &reason), "a preview's error waits while a field is typed in");
        assert!(reveal_refusal(false, false, None, &reason), "and shows once no field is");
        assert!(!reveal_refusal(true, false, Some(&reason), &reason), "once: the user may scroll away from it");
    }

    /// A missing-pick hint reads in the user's language: the dialogs return it as English text,
    /// and the dialog translates it as it shows it.
    #[test]
    fn missing_pick_hints_read_in_the_users_language() {
        for (command, english) in [
            ("solid.move", "select bodies first"),
            ("sheet.flange", "select a profile, edges or sketch lines first"),
            ("plastic.lip", "select the rim face first"),
        ] {
            let mut h = crate::scenario::Harness::new();
            h.step(&serde_json::json!({"start":command})).unwrap();
            h.frames(2);
            h.step(&serde_json::json!({"key":"Enter"})).unwrap();
            h.frames(2);
            assert_eq!(h.app.dialog.as_ref().and_then(|d| d.error.as_deref()), Some(english), "{command}");
            let _japanese = crate::i18n::scope("ja");
            let shown = super::shown_error(english);
            assert!(shown != english && !shown.is_ascii(), "{command} shows {shown:?}");
        }
    }
}

#[cfg(test)]
mod extrude_mixed_pick_tests {
    //! An extrude of a profile and a body face picked together (#3926).

    use super::*;
    use crate::scenario::Harness;

    /// A 20 x 20 x 10 box and a sketch rectangle at its top covering half of it. With the profile
    /// and the top face under it picked, OK extrudes the face (the whole top) and the profile.
    #[test]
    fn a_face_under_a_picked_profile_is_still_extruded() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length":20,"width":20,"height":10,"corner":[0,0,0]})).unwrap();
        h.app.run("sketch.create", json!({"plane":{"origin":[0,0,10],"x_dir":[1,0,0],"y_dir":[0,1,0]}})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[10,20]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("select.clear", json!({})).unwrap();
        // The new sketch's profile is taken at once.
        h.step(&json!({"start":"solid.extrude"})).unwrap();
        h.frames(2);
        let st = h.app.session.world_state();
        let box_body = &st.bodies[0];
        let top = (0..64)
            .find(|i| box_body.body.planar_face(*i).is_some_and(|p| p.normal().z > 0.99 && (p.origin.z - 10.0).abs() < 1e-9))
            .expect("the box's top face");
        let face = Sel::Face { body: box_body.name.clone(), index: top, point: Vec3::new(5.0, 10.0, 10.0) };
        let Some(d) = h.app.dialog.as_mut() else { panic!("the extrude dialog") };
        assert_eq!(d.inputs[0].items.len(), 1, "the profile is picked");
        d.inputs[0].items.push(face);
        if let Kind::Extrude { distance, operation, auto_op, .. } = &mut d.kind {
            *distance = "10 mm".into();
            *operation = 1;
            *auto_op = false;
        }
        h.frames(2);
        h.step(&json!({"key":"Enter"})).unwrap();
        assert!(h.app.dialog.is_none(), "OK: {:?}", h.app.dialog.as_ref().and_then(|d| d.error.clone()));
        let st = h.app.session.world_state();
        let volume: f64 = st.bodies.iter().map(|b| solidcraft_engine::doc::kernel::measure(&b.body).unwrap().volume).sum();
        assert!((volume - 8000.0).abs() < 1e-6, "the box grown to 20 mm: {volume}");
    }
}

#[cfg(test)]
mod shell_body_tests {
    //! The Shell dialog says whose face it removes (#4555).

    use super::*;
    use crate::scenario::Harness;

    /// Two boxes stacked flush: the upper one's bottom face lies in the lower one's top. Picked
    /// from below with the lower box hidden, the face is the upper box's, and the dialog says so.
    #[test]
    fn a_face_flush_with_another_body_is_shelled_on_its_own_body() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length":20,"width":20,"height":20,"corner":[0,0,0]})).unwrap();
        h.app.run("solid.box", json!({"length":20,"width":20,"height":10,"corner":[0,0,20],"operation":"new"})).unwrap();
        let st = h.app.session.world_state();
        let (lower, upper) = (st.bodies[0].name.to_string(), st.bodies[1].name.to_string());
        h.app.run("browser.visibility", json!({"visible":false,"items":[lower]})).unwrap();
        h.step(&json!({"call":"ui.view","params":{"view":"bottom"}})).unwrap();
        h.settle();
        h.step(&json!({"start":"solid.shell"})).unwrap();
        h.step(&json!({"click":{"world":[10,10,20]}})).unwrap();
        h.frames(2);
        let d = h.app.dialog.as_ref().expect("the shell dialog");
        let commands = dialog_commands(&h.app, d).unwrap();
        let (cmd, p) = commands.first().unwrap();
        assert_eq!(cmd, "solid.shell");
        assert_eq!(p["body"], json!(upper), "{p}");
    }
}

#[cfg(test)]
mod shell_thickness_tests {
    //! Shell with its Thickness box emptied (#960).

    use super::*;
    use crate::scenario::Harness;

    #[test]
    fn an_empty_thickness_makes_no_command() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length":20,"width":20,"height":10,"corner":[0,0,0]})).unwrap();
        h.step(&json!({"call":"ui.view","params":{"view":"top"}})).unwrap();
        h.settle();
        h.step(&json!({"start":"solid.shell"})).unwrap();
        h.step(&json!({"click":{"world":[10,10,10]}})).unwrap();
        h.frames(2);
        let d = h.app.dialog.as_ref().expect("the shell dialog");
        assert_eq!(d.inputs[0].items.len(), 1, "the top face is picked");
        assert!(dialog_commands(&h.app, d).is_ok(), "2 mm by default");
        if let Some(Dialog { kind: Kind::Shell { thickness }, .. }) = h.app.dialog.as_mut() {
            thickness.clear();
        }
        let d = h.app.dialog.as_ref().unwrap();
        assert_eq!(dialog_commands(&h.app, d).err().as_deref(), Some("enter the thickness"));
    }
}

#[cfg(test)]
mod construct_ask_tests {
    //! The toolbar CONSTRUCT commands ask for their picks (#204): none runs with empty
    //! params, and a pre-selected face fills the pick so Create just works.

    use super::*;
    use crate::scenario::Harness;

    const CONSTRUCT_PICK_COMMANDS: &[&str] = &[
        "construct.axis.through_cylinder",
        "construct.axis.normal_to_face",
        "construct.axis.two_planes",
        "construct.axis.two_points",
        "construct.axis.edge",
        "construct.point.vertex",
        "construct.point.two_edges",
        "construct.point.three_planes",
        "construct.point.center",
        "construct.point.edge_plane",
        "construct.point.along_path",
        "construct.plane.axis_point",
        "construct.plane.two_edges",
        "construct.plane.three_points",
        "construct.plane.tangent",
        "construct.plane.perpendicular",
    ];

    /// A 20 mm cylinder, 20 mm tall, at the origin: the #204 repro design.
    fn cylinder(h: &mut Harness) {
        h.step(&json!({"call": "engine.execute", "params": {"command": "solid.cylinder", "params": {"diameter": 20, "height": 20}}})).unwrap();
        h.step(&json!({"call": "ui.view", "params": {"view": "iso"}})).unwrap();
    }

    fn status_error(app: &SolveApp) -> Option<String> {
        app.status.as_ref().filter(|(_, _, error)| *error).map(|(message, ..)| message.clone())
    }

    #[test]
    fn every_construct_command_opens_a_dialog_instead_of_running_empty() {
        for id in CONSTRUCT_PICK_COMMANDS {
            let mut h = Harness::new();
            cylinder(&mut h);
            h.step(&json!({"start": id})).unwrap();
            h.frames(2);
            assert!(h.app.dialog.is_some(), "{id} must open a dialog, not run with empty params");
            assert!(status_error(&h.app).is_none(), "{id}: {:?}", status_error(&h.app));
        }
    }

    #[test]
    fn axis_through_cylinder_uses_the_preselected_face() {
        let mut h = Harness::new();
        cylinder(&mut h);
        h.step(&json!({"click": {"world": [10, 0, 10]}})).unwrap(); // the outer cylindrical face
        h.step(&json!({"start": "construct.axis.through_cylinder"})).unwrap();
        h.frames(2);
        let d = h.app.dialog.as_ref().expect("the dialog opens");
        assert_eq!(d.inputs[0].items.len(), 1, "the pre-selected face fills the Face input");
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        assert!(h.app.dialog.is_none(), "Create closes the dialog: {:?}", status_error(&h.app));
        assert!(status_error(&h.app).is_none(), "{:?}", status_error(&h.app));
        let axes = solidcraft_engine::view::construction_axes(&h.app.session);
        assert!(axes.iter().any(|(_, _, _, dir)| dir.z.abs() > 1.0 - 1e-6), "an axis along the cylinder: {axes:?}");
    }

    #[test]
    fn tangent_plane_uses_the_preselected_face() {
        let mut h = Harness::new();
        cylinder(&mut h);
        h.step(&json!({"click": {"world": [10, 0, 10]}})).unwrap(); // the outer cylindrical face
        h.step(&json!({"start": "construct.plane.tangent"})).unwrap();
        h.frames(2);
        let d = h.app.dialog.as_ref().expect("the dialog opens");
        assert_eq!(d.inputs[0].items.len(), 1, "the pre-selected face fills the Face input");
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        assert!(h.app.dialog.is_none(), "Create closes the dialog: {:?}", status_error(&h.app));
        assert!(status_error(&h.app).is_none(), "{:?}", status_error(&h.app));
        assert!(!solidcraft_engine::view::construction_planes(&h.app.session).is_empty(), "the tangent plane is created");
    }
}

#[cfg(test)]
mod preselected_sketch_line_tests {
    //! #8927: a click on a body edge that a shown sketch's line lies on picks the sketch line;
    //! Fillet and Chamfer, which take edges and no sketch curves, take it as that edge.
    use super::*;
    use crate::scenario::Harness;

    /// A 40 × 30 × 10 box made from a sketch on its top face, with that sketch shown.
    fn box_under_its_sketch(h: &mut Harness) {
        for (command, params) in [
            ("sketch.create", json!({"plane": "XY", "offset": 10})),
            ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]})),
            ("sketch.finish", json!({})),
            ("solid.extrude", json!({"distance": -10})),
        ] {
            h.step(&json!({"call": "engine.execute", "params": {"command": command, "params": params}})).unwrap();
        }
        h.step(&json!({"call": "ui.set", "params": {"shownSketches": [1]}})).unwrap();
        h.step(&json!({"call": "ui.view", "params": {"view": "iso"}})).unwrap();
        h.frames(2);
    }

    #[test]
    fn a_preselected_sketch_line_on_an_edge_fills_the_edges_input() {
        for (command, kind) in [("solid.fillet", "FilletFeature"), ("solid.chamfer", "ChamferFeature")] {
            let mut h = Harness::new();
            box_under_its_sketch(&mut h);
            h.step(&json!({"click": {"world": [20, 0, 10]}})).unwrap(); // the top front edge
            h.frames(2);
            assert!(
                matches!(h.app.session.selection.as_slice(), [Sel::SketchCurve { .. } | Sel::ExternalSketchCurve { .. }]),
                "the click picks the sketch line: {:?}",
                h.app.session.selection
            );
            h.step(&json!({"start": command})).unwrap();
            h.frames(2);
            let d = h.app.dialog.as_ref().expect("the dialog opens");
            assert!(
                matches!(d.inputs[0].items.first(), Some(Sel::Edge { point, .. }) if point.dist(Vec3::new(20.0, 0.0, 10.0)) < 1e-6),
                "{command}: the line under the selection is its edge: {:?}",
                d.inputs[0].items
            );
            h.step(&json!({"key": "Enter"})).unwrap();
            h.frames(2);
            let made = h.app.session.doc.features.iter().filter(|f| format!("{:?}", f.kind).starts_with(kind.trim_end_matches("Feature"))).count();
            assert_eq!(made, 1, "{command} runs on the edge");
        }
    }
}
