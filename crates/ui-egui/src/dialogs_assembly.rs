//! Assembly dialogs: New Component, Joint, As-built Joint, Joint Origin, Rigid Group, Drive
//! Joints, Motion Link, Gear, Rack and Pinion, Screw, Cam (`dialogs_coupling`) and Interference. Joint origins are picked by hovering geometry: a face
//! snaps to its centre (or a round face to the centre of its nearer end), an edge to its middle
//! (a round edge to its centre), a vertex to itself; the snap shows as a glyph with its z axis
//! before the click. Joints and drives preview on the placed (world) model: the moving
//! component shows where it goes, and with Animate the joint plays its motion.

use std::collections::HashSet;

use crate::ui_kit::BackingTooltipExt as _;
use crate::ui_kit::type_face::RichTextExt as _;
use crate::ui_kit::{choice_row, expr_row, text_row};
use egui::{Color32, Pos2, RichText, Stroke, vec2};
use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_engine::Sel;
use solidcraft_engine::Session;
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::doc::joints::{Coupling, JointKind, JointOrigin, Size, Snap};
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::geom::{Aabb3, Mesh};

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind, combo, field, form_row};
use crate::preview::{Built, Colors};
use crate::selection::{
    AXES, BODIES, CONSTRUCTION_POINTS, EDGES, FACES, ORIGIN, PLANES, POINTS, PROFILES, SelInput, SelectionExt as _, VERTICES, items as sel_items,
    point_value as pt, polyline_mid,
};
use solidcraft_theme::Tokens;

use crate::viewport::Proj;

#[path = "dialogs_trace.rs"]
mod trace;
pub(crate) use trace::stop_text;
pub use trace::{TraceForm, picked_point};

/// Joint types in the order the Type box lists them (command names).
pub const TYPES: [&str; 13] = [
    "rigid",
    "revolute",
    "slider",
    "cylindrical",
    "pin_slot",
    "planar",
    "ball",
    "parallel",
    "perpendicular",
    "angle",
    "point_on_plane",
    "point_on_line",
    "line_on_plane",
];
const TYPE_LABELS: [&str; 13] = [
    "Rigid",
    "Revolute",
    "Slider",
    "Cylindrical",
    "Pin-slot",
    "Planar",
    "Ball",
    "Parallel",
    "Perpendicular",
    "Angle",
    "Point on plane",
    "Point on line",
    "Line on plane",
];
/// A joint type's label, as the Type box lists it ("Pin-slot", "Point on plane").
pub(crate) fn kind_label(kind: JointKind) -> &'static str {
    TYPES.iter().position(|t| JointKind::parse(t) == Some(kind)).and_then(|i| TYPE_LABELS.get(i)).copied().unwrap_or("")
}

/// A joint as the joint lists name it: its name, and its type in the user's language.
pub(crate) fn joint_choice(name: &str, kind: JointKind) -> String {
    format!("{name} ({})", solidcraft_ui_text::tr_label(kind_label(kind)))
}

/// What a joint origin input accepts: snaps on faces, edges and vertices, the world origin, a
/// sketch point (so a joint can be placed on the design origin or a hole centre, like Fusion's),
/// and the design's planes, axes and construction points.
pub const SNAPS: crate::selection::Accept =
    FACES | EDGES | VERTICES | ORIGIN | POINTS | PLANES | AXES | CONSTRUCTION_POINTS | crate::selection::SNAP_POINTS;
/// Commands whose effect is where occurrences sit (previewed on the placed model).
const MOVES: &[&str] = &[
    "joint.create",
    "joint.as_built",
    "joint.drive",
    "joint.edit",
    "joint.limits",
    "joint.dynamics",
    "joint.rigid_group",
    "joint.gear",
    "joint.rack_pinion",
    "joint.screw",
    "joint.cam",
    "assembly.constraint.create",
    "assembly.constraint.edit",
    "occurrence.align",
    "parts.insert",
    "parts.fastener",
];

#[derive(Clone, Debug)]
pub enum Asm {
    FromBodies,
    NewComponent {
        name: String,
        activate: bool,
    },
    Joint(Box<JointForm>),
    JointOrigin {
        name: String,
        hover: Hover,
    },
    RigidGroup,
    /// `focus`: the first value takes the keyboard (its text selected) on the next frame.
    Drive {
        joint: usize,
        values: Vec<String>,
        focus: bool,
    },
    MotionLink {
        a: usize,
        b: usize,
        ia: usize,
        ib: usize,
        ratio: String,
        offset: String,
        /// Editing motion link this instead of adding a new one (#6825).
        editing: Option<usize>,
    },
    Coupling(Box<crate::dialogs_coupling::CouplingForm>),
    Cam(Box<crate::dialogs_coupling::cam::CamForm>),
    Constrain(Box<crate::dialogs_constrain::ConstrainForm>),
    Interference {
        result: Option<Value>,
        overlaps: Vec<Mesh>,
        of: Vec<Sel>,
        /// Pairs closer than this (mm, as typed) are listed too.
        clearance: String,
        /// Compute's check under way (its reply lands here).
        check: Option<Check>,
    },
    /// Motion Trace: a picked point's path, previewed; OK keeps it as a 3D sketch spline.
    Trace(Box<TraceForm>),
}

/// Interference's check off the window thread: where it leaves its reply (the check's and the
/// shared volumes' meshes), the design and the picks it checked, and when it started.
#[derive(Clone, Debug)]
pub struct Check {
    reply: std::sync::Arc<std::sync::Mutex<Option<(Value, Vec<Mesh>)>>>,
    design: std::sync::Weak<solidcraft_engine::doc::Document>,
    of: Vec<Sel>,
    started: f64,
}

/// The snap under the cursor, remembered for the selection it was found for.
pub type Hover = Option<(Sel, Option<SnapPt>)>;

/// A search off the window thread: where it leaves its reply, and what it searched.
#[derive(Clone, Debug)]
pub struct Search {
    reply: std::sync::Arc<std::sync::Mutex<Option<Result<Value, String>>>>,
    searched: Searched,
}

/// The design and the joint (the commands OK would run for it) a search was for.
#[derive(Clone, Debug)]
pub struct Searched {
    design: std::sync::Weak<solidcraft_engine::doc::Document>,
    joint: Vec<(String, Value)>,
}

impl Searched {
    fn now(app: &SolveApp, f: &JointForm, inputs: &[SelInput]) -> Result<Searched, String> {
        Ok(Searched { design: std::sync::Arc::downgrade(&app.session.doc), joint: edit_commands(app, f, inputs)? })
    }

    /// Whether the design and the dialog's joint are still the ones searched.
    fn current(&self, app: &SolveApp, f: &JointForm, inputs: &[SelInput]) -> bool {
        self.design.upgrade().is_some_and(|d| std::sync::Arc::ptr_eq(&d, &app.session.doc))
            && edit_commands(app, f, inputs).is_ok_and(|now| now == self.joint)
    }
}

/// The Joint and As-built Joint dialogs.
#[derive(Clone, Debug)]
pub struct JointForm {
    pub name: String,
    pub as_built: bool,
    /// Editing this joint (its origins stay; type, alignment and limits change).
    pub editing: Option<u64>,
    pub kind: usize,
    /// Edit Joint: the sides as the dialog opened on them. A side still that pick is not sent,
    /// so it keeps its origin exactly.
    pub sides: [Option<Sel>; 2],
    /// Flip B relative to the mate (faces apart instead of together).
    pub flip: bool,
    pub angle: String,
    /// Edit Joint: the angle as the dialog opened on it. Sent only when changed, so a side moved
    /// without it turns B as joint.create would.
    pub angle_shown: String,
    pub offset: [String; 3],
    /// Per joint value: limited, minimum, maximum.
    pub limits: Vec<(bool, String, String)>,
    /// Find Limits: the search under way (`joint.find_limits`'s reply lands here).
    pub search: Option<Search>,
    /// What Find Limits found for each value (a line under its rows).
    pub found: Vec<String>,
    /// What the last search said about them all: parts already in each other, or why it failed.
    pub found_note: Option<String>,
    /// The design and the joint (as the dialog had it) the found limits are for.
    pub searched: Option<Searched>,
    /// Edit Joint Limits: open the Limits section when the dialog first draws.
    pub show_limits: bool,
    /// Per joint value: effort, velocity, damping, friction, as typed (empty: not set). Shown
    /// when editing: a joint being created has no id to set them on yet.
    pub dynamics: Vec<[String; 4]>,
    pub animate: bool,
    /// Animation phase, 0…1.
    pub phase: f64,
    pub hover: Hover,
}

impl JointForm {
    fn new(as_built: bool, doc: &solidcraft_engine::doc::Document) -> JointForm {
        JointForm {
            name: String::new(),
            as_built,
            editing: None,
            kind: 0,
            sides: [None, None],
            flip: false,
            angle: "0 deg".into(),
            angle_shown: "0 deg".into(),
            offset: std::array::from_fn(|_| crate::length_input::default(doc, "0")),
            limits: Vec::new(),
            search: None,
            found: Vec::new(),
            found_note: None,
            searched: None,
            show_limits: false,
            dynamics: Vec::new(),
            animate: false,
            phase: 0.0,
            hover: None,
        }
    }

    pub(crate) fn joint_kind(&self) -> JointKind {
        TYPES.get(self.kind).and_then(|t| JointKind::parse(t)).unwrap_or(JointKind::Rigid)
    }

    /// Limits rows sized to the joint's values.
    fn fit_limits(&mut self, doc: &solidcraft_engine::doc::Document) {
        let k = self.joint_kind();
        let n = k.dofs().len();
        while self.limits.len() < n {
            let i = self.limits.len();
            self.limits.push(if k.is_angle(i) {
                (false, "-90 deg".into(), "90 deg".into())
            } else {
                (false, crate::length_input::default(doc, "0"), crate::length_input::default(doc, "20"))
            });
        }
        self.limits.truncate(n);
        self.dynamics.resize_with(n, Default::default);
    }
}

impl Asm {
    pub fn title(&self) -> &'static str {
        match self {
            Asm::FromBodies => "cmd.component_from_bodies",
            Asm::NewComponent { .. } => "NEW COMPONENT",
            Asm::Joint(j) if j.editing.is_some() => "EDIT JOINT",
            Asm::Joint(j) if j.as_built => "AS-BUILT JOINT",
            Asm::Joint(_) => "JOINT",
            Asm::JointOrigin { .. } => "JOINT ORIGIN",
            Asm::RigidGroup => "RIGID GROUP",
            Asm::Drive { .. } => "DRIVE JOINTS",
            Asm::MotionLink { .. } => "MOTION LINK",
            Asm::Coupling(f) => match f.command {
                "joint.gear" => "GEAR",
                "joint.rack_pinion" => "RACK AND PINION",
                _ => "SCREW",
            },
            Asm::Cam(_) => "CAM",
            Asm::Constrain(_) => "CONSTRAIN",
            Asm::Interference { .. } => "INTERFERENCE",
            Asm::Trace(_) => "MOTION TRACE",
        }
    }

    pub fn previews(&self) -> bool {
        matches!(self, Asm::Joint(_) | Asm::Drive { .. } | Asm::RigidGroup | Asm::Coupling(_) | Asm::Cam(_) | Asm::Constrain(_))
    }
}

/// The dialog for an assembly command: its state and selection inputs.
pub fn start(app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    let s = &app.session;
    let (asm, inputs) = match id {
        "component.from_bodies" => (Asm::FromBodies, vec![SelInput::new("Bodies", BODIES, true)]),
        "component.create" => (Asm::NewComponent { name: String::new(), activate: true }, vec![SelInput::new("Bodies (optional)", BODIES, true)]),
        "joint.create" => (Asm::Joint(Box::new(JointForm::new(false, &app.session.doc))), side_inputs()),
        "joint.as_built" => (
            Asm::Joint(Box::new(JointForm::new(true, &app.session.doc))),
            vec![SelInput::new("Components", BODIES, true), SelInput::new("Position", SNAPS, false)],
        ),
        "joint.origin" => (Asm::JointOrigin { name: String::new(), hover: None }, vec![SelInput::new("Snap", SNAPS, false)]),
        "joint.rigid_group" => (Asm::RigidGroup, vec![SelInput::new("Components", BODIES, true)]),
        "joint.drive" => {
            let joint = picked_joint(app, true);
            (Asm::Drive { joint, values: current_values(s, joint), focus: true }, vec![])
        }
        "motion.trace" => (Asm::Trace(Box::new(trace::form(s, picked_joint(app, false)))), vec![SelInput::new("Point", SNAPS, false)]),
        "joint.motion_link" => {
            let b = usize::from(movable(s).len() > 1);
            (Asm::MotionLink { a: 0, b, ia: 0, ib: 0, ratio: "1".into(), offset: "0".into(), editing: None }, vec![])
        }
        "joint.gear" | "joint.rack_pinion" | "joint.screw" => crate::dialogs_coupling::start(app, id)?,
        "joint.cam" => crate::dialogs_coupling::cam::start(app),
        "assembly.constraint.create" => crate::dialogs_constrain::start(app)?,
        "inspect.interference" => (
            Asm::Interference { result: None, overlaps: Vec::new(), of: Vec::new(), clearance: "0".into(), check: None },
            vec![SelInput::new("Bodies (all if none)", BODIES, true)],
        ),
        _ => return None,
    };
    Some((Kind::Assembly(asm), inputs))
}

/// What an open dialog starts on when the control channel's `ui.start` passes `params`. Drive
/// Joints takes `joint` (id or name) and `values` (numbers: degrees or millimetres; text: an
/// expression), as `joint.drive` does. Nothing is driven until OK.
pub fn preset(s: &Session, k: &mut Asm, p: &Value) -> Result<(), String> {
    let Asm::Drive { joint, values, .. } = k else { return Err(format!("{} takes no params yet", k.title())) };
    let o = p.as_object().ok_or("`params` must be an object")?;
    if let Some(key) = o.keys().find(|k| !matches!(k.as_str(), "joint" | "values")) {
        return Err(format!("unknown param `{key}` (Drive Joints takes joint, values)"));
    }
    if let Some(v) = o.get("joint") {
        let key = match v {
            Value::Number(n) => n.to_string(),
            Value::String(x) => x.clone(),
            _ => return Err("`joint` must be an id or a name".into()),
        };
        let i = s.doc.assembly.joints.iter().position(|j| j.id.to_string() == key || j.name == key).ok_or_else(|| format!("no joint `{key}`"))?;
        if !movable(s).contains(&i) {
            return Err(format!("joint `{key}` is rigid: it has nothing to drive"));
        }
        *joint = i;
        *values = current_values(s, i);
    }
    if let Some(v) = o.get("values") {
        let j = s.doc.assembly.joints.get(*joint).ok_or("no joint that moves yet")?;
        let list = v.as_array().ok_or("`values` must be a list")?;
        if list.len() > values.len() {
            return Err(format!("`{}` takes {} value(s)", j.name, values.len()));
        }
        for (i, (slot, v)) in values.iter_mut().zip(list).enumerate() {
            *slot = match v {
                Value::Number(n) => {
                    let x = n.as_f64().filter(|x| x.is_finite() && x.abs() < 1e9).ok_or("values must be finite numbers")?;
                    num(x, if j.kind.is_angle(i) { "deg" } else { "mm" })
                }
                Value::String(t) if t.len() <= 1000 => t.clone(),
                _ => return Err("values must be numbers or expressions".into()),
            };
        }
    }
    Ok(())
}

/// The joint picked in the browser, else the first that moves (an index into the design's
/// joints). With `rigid`, a picked rigid joint comes before the first that moves: Drive Joints
/// opened on one says why it can't drive it (#6981).
fn picked_joint(app: &SolveApp, rigid: bool) -> usize {
    let s = &app.session;
    let list = movable(s);
    let picked: Vec<usize> = app.tree.picked_joints.iter().filter_map(|id| s.doc.assembly.joints.iter().position(|j| j.id == *id)).collect();
    let moving = picked.iter().copied().find(|i| list.contains(i));
    moving.or(picked.first().copied().filter(|_| rigid)).or(list.first().copied()).unwrap_or(0)
}

/// Indices (into the design's joints) of joints that move (not rigid).
fn movable(s: &Session) -> Vec<usize> {
    s.doc.assembly.joints.iter().enumerate().filter(|(_, j)| j.kind != JointKind::Rigid).map(|(i, _)| i).collect()
}

/// A joint's values as the Drive dialog shows them (degrees, millimetres).
fn current_values(s: &Session, index: usize) -> Vec<String> {
    let Some(j) = s.doc.assembly.joints.get(index) else { return Vec::new() };
    (0..j.kind.dofs().len())
        .map(|i| {
            let v = j.values.get(i).copied().unwrap_or(0.0);
            if j.kind.is_angle(i) { num(v.to_degrees(), "deg") } else { crate::length_input::model(&s.doc, v) }
        })
        .collect()
}

/// A value with its unit, without trailing zeros ("12 mm", "37.5 deg").
pub(crate) fn num(v: f64, unit: &str) -> String {
    format!("{} {unit}", crate::dialogs_coupling::trim(v))
}

/// The two side inputs of the Joint dialog (Joint and Edit Joint): the picks of each origin.
fn side_inputs() -> Vec<SelInput> {
    vec![SelInput::new("Component 1", SNAPS, false), SelInput::new("Component 2", SNAPS, false)]
}

/// The pick a joint side shows in the side inputs: the face, edge, plane, or axis of its own
/// occurrence that the side's snap names (the piece nearest its pick, when the name has
/// several), else the face or edge nearest the pick. A point or frame side names no face, so it
/// shows its component.
pub(crate) fn side_pick(app: &SolveApp, o: &JointOrigin) -> Option<Sel> {
    let s = &app.session;
    let (pick, face, edge) = match &o.snap {
        Snap::FaceCenter { pick, face, .. } | Snap::CircleCenter { pick, face, .. } => (*pick, face.as_deref(), None),
        Snap::Edge { pick, edge, .. } => (*pick, None, edge.as_deref()),
        Snap::Plane { plane, .. } => {
            let name = match plane.as_ref() {
                solidcraft_engine::doc::PlaneRef::Origin { name } => Some(name.clone()),
                solidcraft_engine::doc::PlaneRef::Datum { datum } => Some(datum.key()),
                solidcraft_engine::doc::PlaneRef::Construction { feature } => {
                    let id = feature.0;
                    s.doc
                        .feature(id)
                        .map(|f| f.name.clone())
                        .or_else(|| s.model.state().construct.iter().find(|c| c.feature == id).map(|c| c.name.clone()))
                }
                _ => None,
            }?;
            if matches!(plane.as_ref(), solidcraft_engine::doc::PlaneRef::Datum { .. }) {
                return Some(Sel::Plane { name });
            }
            let name = if o.occurrence == 0 {
                name
            } else {
                let occ_name = s.doc.occurrences.iter().find(|occ| occ.id == o.occurrence).map(|occ| occ.name.as_str()).unwrap_or("");
                if occ_name.is_empty() { name } else { format!("{occ_name}/{name}") }
            };
            return Some(Sel::Plane { name });
        }
        Snap::Axis { axis, .. } => {
            let name = match axis.as_ref() {
                solidcraft_engine::doc::joints::AxisRef::Name(name) => Some(name.clone()),
                solidcraft_engine::doc::joints::AxisRef::Target(solidcraft_engine::doc::joints::AxisTarget::Origin { axis }) => Some(axis.clone()),
                solidcraft_engine::doc::joints::AxisRef::Target(solidcraft_engine::doc::joints::AxisTarget::Construction { name }) => {
                    Some(name.clone())
                }
                solidcraft_engine::doc::joints::AxisRef::Target(solidcraft_engine::doc::joints::AxisTarget::Datum { datum }) => Some(datum.key()),
                _ => None,
            }?;
            if matches!(axis.as_ref(), solidcraft_engine::doc::joints::AxisRef::Target(solidcraft_engine::doc::joints::AxisTarget::Datum { .. })) {
                return Some(Sel::Axis { name });
            }
            let name = if o.occurrence == 0 {
                name
            } else {
                let occ_name = s.doc.occurrences.iter().find(|occ| occ.id == o.occurrence).map(|occ| occ.name.as_str()).unwrap_or("");
                if occ_name.is_empty() { name } else { format!("{occ_name}/{name}") }
            };
            return Some(Sel::Axis { name });
        }
        Snap::Point { .. } | Snap::Frame { .. } => return Some(Sel::Occurrence { path: origin_placement(s, o)? }),
    };
    // The pick is in its part's coordinates; the placed bodies are in the world.
    let path = origin_placement(s, o)?;
    let (_, at) = s.doc.path_placement(&path)?;
    let p = solidcraft_engine::doc::apply_point(&at, pick);
    let st = crate::open_meshing::session_world(s);
    let own: Vec<&solidcraft_engine::doc::ModelBody> = st.bodies.iter().filter(|b| placement_of_body(s, &b.name).as_ref() == Some(&path)).collect();
    let named = match (face, edge) {
        (Some(name), _) => own.iter().filter_map(|b| named_face(b, name, p)).min_by(|a, b| a.0.total_cmp(&b.0)),
        (None, Some(name)) => own.iter().filter_map(|b| named_edge(b, name, p)).min_by(|a, b| a.0.total_cmp(&b.0)),
        (None, None) => None,
    };
    named.map(|(_, sel)| sel).or_else(|| nearest_pick(&own, p, matches!(o.snap, Snap::Edge { .. })))
}

/// The distance from `p` to the face `f` of `m`, measured to its triangles.
fn face_dist(m: &Mesh, f: usize, p: Vec3) -> Option<f64> {
    m.triangles
        .iter()
        .zip(&m.tri_face)
        .filter(|(_, tf)| **tf as usize == f)
        .filter_map(|(t, _)| m.tri(t))
        .map(|[a, c, e]| crate::preview::point_triangle_dist(p, a, c, e))
        .reduce(f64::min)
}

/// The distance from `p` to the mesh edge `i` of `m`.
fn edge_dist(m: &Mesh, i: usize, p: Vec3) -> Option<f64> {
    m.edges.get(i)?.windows(2).map(|w| p.dist_to_segment(w[0], w[1])).reduce(f64::min)
}

/// The face of `b` the name `name` names, the piece nearest `p` when it has several.
fn named_face(b: &solidcraft_engine::doc::ModelBody, name: &str, p: Vec3) -> Option<(f64, Sel)> {
    let m = b.mesh();
    let names = solidcraft_engine::doc::naming::face_names(b);
    names
        .iter()
        .enumerate()
        .filter(|(_, n)| n.as_str() == name)
        .filter_map(|(f, _)| face_dist(&m, f, p).map(|d| (d, Sel::Face { body: b.name.clone(), index: f, point: p })))
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

/// The edge of `b` the name `name` names (a split edge: the piece nearest `p`).
fn named_edge(b: &solidcraft_engine::doc::ModelBody, name: &str, p: Vec3) -> Option<(f64, Sel)> {
    let m = b.mesh();
    let names = solidcraft_engine::doc::naming::edge_names(b);
    let index = match names.iter().position(|n| n == name) {
        Some(i) => i,
        None => {
            let at = solidcraft_engine::doc::naming::find_edge(b, name, p).ok()?;
            (0..m.edges.len())
                .min_by(|x, y| edge_dist(&m, *x, at).unwrap_or(f64::INFINITY).total_cmp(&edge_dist(&m, *y, at).unwrap_or(f64::INFINITY)))?
        }
    };
    let d = edge_dist(&m, index, p)?;
    Some((d, Sel::Edge { body: b.name.clone(), index, point: p }))
}

/// The face (or edge) of the bodies `own` nearest `p`.
fn nearest_pick(own: &[&solidcraft_engine::doc::ModelBody], p: Vec3, edges: bool) -> Option<Sel> {
    let mut best: Option<(f64, Sel)> = None;
    for b in own {
        let m = b.mesh();
        if edges {
            for i in 0..m.edges.len() {
                let Some(d) = edge_dist(&m, i, p) else { continue };
                if best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                    best = Some((d, Sel::Edge { body: b.name.clone(), index: i, point: p }));
                }
            }
        } else {
            for (t, f) in m.triangles.iter().zip(&m.tri_face) {
                let Some([a, c, e]) = m.tri(t) else { continue };
                let d = crate::preview::point_triangle_dist(p, a, c, e);
                if best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                    best = Some((d, Sel::Face { body: b.name.clone(), index: *f as usize, point: p }));
                }
            }
        }
    }
    best.map(|(_, sel)| sel)
}

/// Default name for a new joint based on its type (e.g. "Planar Joint1", "Revolute Joint1").
pub(crate) fn default_joint_name(doc: &solidcraft_engine::doc::Document, kind: JointKind) -> String {
    let prefix = format!("{} Joint", kind_label(kind));
    let names: std::collections::BTreeSet<_> = doc.assembly.joints.iter().map(|j| j.name.as_str()).collect();
    let number = (1..=doc.assembly.joints.len().saturating_add(1)).find(|n| !names.contains(format!("{prefix}{n}").as_str())).unwrap_or(1);
    format!("{prefix}{number}")
}

/// The Edit Joint dialog for a joint (by id). Its two sides show as the Joint dialog's do, each
/// pre-filled with the pick it sits on.
pub fn edit_joint(app: &SolveApp, id: u64) -> Option<Dialog> {
    let j = app.session.doc.assembly.joints.iter().find(|j| j.id == id)?;
    let mut f = JointForm::new(false, &app.session.doc);
    f.name = j.name.clone();
    f.editing = Some(id);
    f.sides = [side_pick(app, &j.a), side_pick(app, &j.b)];
    let mut inputs = side_inputs();
    for (input, pick) in inputs.iter_mut().zip(&f.sides) {
        input.items = pick.iter().cloned().collect();
    }
    f.kind = TYPES.iter().position(|t| JointKind::parse(t) == Some(j.kind)).unwrap_or(0);
    f.flip = j.flip;
    // An offset or angle typed as an expression shows (and is sent back) as that expression, so
    // the joint keeps following its parameters.
    f.angle = j.exprs.angle.clone().unwrap_or_else(|| num(j.angle.to_degrees(), "deg"));
    f.angle_shown = f.angle.clone();
    f.offset[2] = j.exprs.offset.clone().unwrap_or_else(|| crate::length_input::model(&app.session.doc, j.offset));
    f.fit_limits(&app.session.doc);
    for (i, l) in f.limits.iter_mut().enumerate() {
        if let Some(Some((lo, hi))) = j.limits.get(i) {
            let fmt = |v: f64| if j.kind.is_angle(i) { num(v.to_degrees(), "deg") } else { crate::length_input::model(&app.session.doc, v) };
            *l = (true, fmt(*lo), fmt(*hi));
        }
    }
    for (i, row) in f.dynamics.iter_mut().enumerate() {
        if let Some(Some(d)) = j.dynamics.get(i) {
            let fmt = |v: Option<f64>| v.map(|x| x.to_string()).unwrap_or_default();
            *row = [fmt(d.effort), fmt(d.velocity), fmt(d.damping), fmt(d.friction)];
        }
    }
    Some(Dialog::new(Kind::Assembly(Asm::Joint(Box::new(f))), inputs))
}

/// Edit Joint with its Limits section open (the browser's Edit Joint Limits).
pub fn edit_joint_limits(app: &SolveApp, id: u64) -> Option<Dialog> {
    let mut d = edit_joint(app, id)?;
    if let Kind::Assembly(Asm::Joint(f)) = &mut d.kind {
        f.show_limits = true;
    }
    Some(d)
}

/// The Edit dialog for a motion link (by its index, as `joint.list` numbers them): the gear,
/// rack, screw or cam dialog for a coupled link, the Motion Link one otherwise (#6825).
pub fn edit_link(app: &SolveApp, index: usize) -> Option<Dialog> {
    let s = &app.session;
    let l = s.doc.assembly.links.get(index)?;
    // A round-face pick is not kept in the form; leaving the size out of the box while editing
    // makes the engine keep the old one, the picked face included.
    let size_text = |sz: &Size| match sz {
        Size::Length(v) => crate::length_input::model(&s.doc, *v),
        Size::Expr(e) => e.clone(),
        Size::Circle(_) => String::new(),
    };
    let is_angle = |id: u64, i: usize| s.doc.assembly.joints.iter().find(|j| j.id == id).is_some_and(|j| j.kind.is_angle(i));
    if l.coupling.is_none() {
        // A plain link as `joint.list` shows it: ratio per degree where a turns, offset in b's units.
        let unit = |angle: bool| if angle { 1f64.to_radians() } else { 1.0 };
        let scale = unit(is_angle(l.b, l.ib)) / unit(is_angle(l.a, l.ia));
        let pos = |id: u64| s.doc.assembly.joints.iter().position(|j| j.id == id);
        let (Some(a), Some(b)) = (pos(l.a), pos(l.b)) else { return None };
        let offset = if is_angle(l.b, l.ib) { num(l.offset.to_degrees(), "deg") } else { crate::length_input::model(&s.doc, l.offset) };
        let ratio = crate::dialogs_coupling::trim(l.ratio / scale);
        return Some(Dialog::new(Kind::Assembly(Asm::MotionLink { a, b, ia: l.ia, ib: l.ib, ratio, offset, editing: Some(index) }), vec![]));
    }
    let (kind, inputs) = match l.coupling.as_ref()? {
        Coupling::Gear { ra, rb, internal, reverse } => (
            Asm::Coupling(Box::new(crate::dialogs_coupling::CouplingForm {
                command: "joint.gear",
                a: l.a,
                b: l.b,
                sizes: [size_text(ra), size_text(rb)],
                internal: *internal,
                reverse: *reverse,
                picked: [None, None],
                animate: false,
                phase: 0.0,
                editing: Some(index),
            })),
            vec![SelInput::new("Gear 1 (optional)", FACES | EDGES, false), SelInput::new("Gear 2 (optional)", FACES | EDGES, false)],
        ),
        Coupling::RackPinion { radius, reverse } => (
            Asm::Coupling(Box::new(crate::dialogs_coupling::CouplingForm {
                command: "joint.rack_pinion",
                a: l.a,
                b: l.b,
                sizes: [size_text(radius), String::new()],
                internal: false,
                reverse: *reverse,
                picked: [None, None],
                animate: false,
                phase: 0.0,
                editing: Some(index),
            })),
            vec![SelInput::new("Pinion (optional)", FACES | EDGES, false)],
        ),
        Coupling::Screw { pitch, reverse } => (
            Asm::Coupling(Box::new(crate::dialogs_coupling::CouplingForm {
                command: "joint.screw",
                a: l.a,
                b: l.b,
                sizes: [size_text(pitch), String::new()],
                internal: false,
                reverse: *reverse,
                picked: [None, None],
                animate: false,
                phase: 0.0,
                editing: Some(index),
            })),
            vec![],
        ),
        Coupling::Cam(c) => {
            use solidcraft_engine::doc::joints::cam::Follower;
            let follower = match &c.follower {
                Follower::Roller { .. } => 1,
                Follower::Flat => 2,
                Follower::Knife => 0,
            };
            let radius = match &c.follower {
                Follower::Roller { radius } => size_text(radius),
                _ => crate::dialogs::length_default(s, "5"),
            };
            (
                Asm::Cam(Box::new(crate::dialogs_coupling::cam::CamForm {
                    a: l.a,
                    b: l.b,
                    editing: Some(index),
                    follower,
                    radius,
                    flip: c.reverse,
                    picked: None,
                    animate: false,
                    phase: 0.0,
                })),
                vec![
                    SelInput::new("Profile", PROFILES | EDGES, true),
                    SelInput::new("Roller (optional)", FACES | EDGES, false),
                    SelInput::new("Tip (optional)", FACES | EDGES | VERTICES, false),
                ],
            )
        }
    };
    Some(Dialog::new(Kind::Assembly(kind), inputs))
}

/// Edit Joint with its motion playing (the browser's Animate Joint).
pub fn animate_joint(app: &SolveApp, id: u64) -> Option<Dialog> {
    let mut d = edit_joint(app, id)?;
    if let Kind::Assembly(Asm::Joint(f)) = &mut d.kind {
        f.animate = true;
    }
    Some(d)
}

/// Frame the two components a joint connects (the browser's Find in Window).
pub fn find_joint(app: &mut SolveApp, id: u64) {
    let s = &app.session;
    let Some(j) = s.doc.assembly.joints.iter().find(|j| j.id == id) else { return };
    let bb = occurrence_bounds(s, &[j.a.occurrence, j.b.occurrence]);
    if bb.is_empty() {
        return;
    }
    let mut cam = app.cam;
    cam.fit(&solidcraft_engine::render::view_bounds(&bb));
    app.animate_to(cam);
}

/// The box around the bodies of occurrences (and of the occurrences inside them), where they
/// are placed; occurrence 0 is the root, holding the bodies outside every occurrence.
pub fn occurrence_bounds(s: &Session, occs: &[u64]) -> Aabb3 {
    let model = s.model.state();
    let world = crate::open_meshing::session_world(s);
    let mut bb = Aabb3::default();
    for (name, _, path, _) in s.doc.body_frames(&model) {
        let on = occs.iter().any(|o| if *o == 0 { path.is_empty() } else { path.contains(o) });
        if on && let Some(b) = world.body(&name) {
            bb = bb.union(&b.mesh().bounds());
        }
    }
    bb
}

/// The Drive Joints dialog on a joint (by id).
pub fn drive_joint(app: &SolveApp, id: u64) -> Option<Dialog> {
    let joint = app.session.doc.assembly.joints.iter().position(|j| j.id == id)?;
    Some(Dialog::new(Kind::Assembly(Asm::Drive { joint, values: current_values(&app.session, joint), focus: true }), vec![]))
}

// ---- snaps ----

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SnapKind {
    FaceCenter,
    CircleCenter,
    EdgeMid,
    Vertex,
}

/// A joint origin snap (world): where it sits, its z axis, and the command's origin parameter.
#[derive(Clone, Debug)]
pub struct SnapPt {
    pub at: Vec3,
    pub z: Vec3,
    pub kind: SnapKind,
    pub param: Value,
}

/// The occurrence placing a world body (0: the root design), the instance its name says.
pub(crate) fn occurrence_of_body(s: &Session, body: &str) -> Option<u64> {
    placement_of_body(s, body).map(|path| path.last().copied().unwrap_or(0))
}

/// The placement of a world body: the occurrence ids from the root (empty: the root design).
/// The world name tells the instance, a copy of a sub-assembly placed more than once included.
pub(crate) fn placement_of_body(s: &Session, body: &str) -> Option<Vec<u64>> {
    let st = crate::open_meshing::session_world(s);
    let b = st.body(body)?;
    if s.doc.model_body_component(b) == 0 {
        return Some(Vec::new());
    }
    let model = s.model.state();
    let (_, _, _, path, _) = s.doc.body_frames_with_source(&model).into_iter().find(|(world, ..)| world == body)?;
    (!path.is_empty()).then_some(path)
}

/// The placement a joint origin's part is at: its occurrence's first placement, then the
/// occurrences it is inside.
fn origin_placement(s: &Session, o: &JointOrigin) -> Option<Vec<u64>> {
    if o.occurrence == 0 {
        return Some(Vec::new());
    }
    let occurrence = s.doc.occurrences.iter().find(|p| p.id == o.occurrence)?;
    let mut path = s.doc.first_path(occurrence.parent);
    path.push(occurrence.id);
    path.extend(&o.inside);
    s.doc.path_placement(&path)?;
    Some(path)
}

/// Centre and unit normal of the circle through a polyline's points, if it lies on one.
pub fn circle_of(pts: &[Vec3]) -> Option<(Vec3, Vec3)> {
    let n = pts.len();
    if n < 4 {
        return None;
    }
    let (a, b, c) = (*pts.first()?, *pts.get(n / 3)?, *pts.get(2 * n / 3)?);
    let (ab, ac) = (b - a, c - a);
    let nn = ab.cross(ac);
    let l2 = nn.len2();
    if l2 < 1e-18 {
        return None;
    }
    let center = a + (nn.cross(ab) * ac.len2() + ac.cross(nn) * ab.len2()) / (2.0 * l2);
    let r = center.dist(a);
    if !r.is_finite() || r > 1e6 {
        return None;
    }
    let tol = (r * 2e-3).max(1e-6);
    let normal = nn.normalized()?;
    pts.iter().all(|p| (p.dist(center) - r).abs() < tol && (*p - center).dot(normal).abs() < tol).then_some((center, normal))
}

/// Axis of a straight edge. Its largest component points positive so reversing the
/// tessellation does not reverse a joint's drive direction.
fn straight_edge_axis(pts: &[Vec3]) -> Option<Vec3> {
    let (a, b) = (*pts.first()?, *pts.last()?);
    let dir = (b - a).normalized()?;
    let tol = 1e-6 * a.dist(b).max(1.0);
    if !pts.iter().all(|p| (*p - a).cross(dir).len() < tol) {
        return None;
    }
    let leading = [dir.x, dir.y, dir.z].into_iter().max_by(|a, b| a.abs().total_cmp(&b.abs()))?;
    Some(if leading < 0.0 { -dir } else { dir })
}

/// Normal of the mesh face `f` near `p`.
pub(crate) fn face_normal_near(m: &Mesh, f: u32, p: Vec3) -> Option<Vec3> {
    m.triangles
        .iter()
        .zip(&m.tri_face)
        .filter(|(_, tf)| **tf == f)
        .filter_map(|(t, _)| m.tri(t))
        .min_by(|a, b| ((a[0] + a[1] + a[2]) / 3.0).dist(p).total_cmp(&((b[0] + b[1] + b[2]) / 3.0).dist(p)))
        .and_then(|[a, b, c]| (b - a).cross(c - a).normalized())
}

/// The hover label tag: "Plane" or "Axis" when picking planes or axes, else the snap kind's name.
pub(crate) fn snap_tag(s: &SnapPt) -> &'static str {
    if s.param.get("plane").is_some() {
        solidcraft_ui_text::tr("ui.plane")
    } else if s.param.get("axis").is_some() {
        solidcraft_ui_text::tr("ui.axis")
    } else {
        snap_name(s.kind)
    }
}

/// The snap a picked item stands for (world coordinates, as the viewport shows them).
pub fn snap_of(s: &Session, sel: &Sel) -> Option<SnapPt> {
    let model_st = s.model.state();
    let st = crate::open_meshing::session_world(s);
    let (body, mut snap) = match sel {
        Sel::Plane { name } => {
            // A datum side round-trips through its key: the pick stands for that placement's datum.
            if let Some(datum) = solidcraft_engine::doc::construct::DatumRef::parse_key(name) {
                let occ = datum.path.last().copied().unwrap_or(0);
                let t = if occ == 0 { solidcraft_engine::doc::IDENTITY } else { solidcraft_engine::doc::joints::occurrence_world(&s.doc, occ) };
                let (at, z) = match datum.item.geom() {
                    solidcraft_engine::doc::construct::ConstructGeom::Plane(p) => (
                        solidcraft_engine::doc::apply_point(&t, p.origin),
                        solidcraft_engine::doc::apply_vector(&t, p.normal()).normalized().unwrap_or(Vec3::Z),
                    ),
                    _ => return None,
                };
                return Some(SnapPt { at, z, kind: SnapKind::FaceCenter, param: json!({"occurrence": occ, "plane": name}) });
            }
            let (occ, plane_name) = if let Some((prefix, plane_name)) = name.rsplit_once('/') {
                let occ = s
                    .doc
                    .occurrences
                    .iter()
                    .find(|o| o.name == prefix || o.id.to_string() == prefix)
                    .map(|o| o.id)
                    .or_else(|| s.doc.find_component(prefix).and_then(|c| s.doc.occurrence_of(c)).map(|o| o.id))
                    .unwrap_or(0);
                (occ, plane_name)
            } else {
                let occ = model_st
                    .construct
                    .iter()
                    .find(|c| c.name == *name)
                    .and_then(|c| {
                        let comp = s.doc.feature(c.feature).map(|f| f.component).unwrap_or(0);
                        if comp == 0 { Some(0) } else { s.doc.occurrence_of(comp).map(|o| o.id) }
                    })
                    .unwrap_or(0);
                (occ, name.as_str())
            };
            let upper = plane_name.to_ascii_uppercase();
            let (local_origin, local_normal) = if matches!(upper.as_str(), "XY" | "XZ" | "YZ") {
                let p = solidcore_record::Plane::named(&upper)?;
                (p.origin, p.normal().normalized().unwrap_or(Vec3::Z))
            } else {
                let selected_component = if occ == 0 { 0 } else { s.doc.occurrences.iter().find(|o| o.id == occ).map(|o| o.component).unwrap_or(0) };
                let c = model_st.construct.iter().find(|c| {
                    c.name == plane_name
                        && s.doc.feature(c.feature).map(|f| f.component) == Some(selected_component)
                        && matches!(&c.geom, solidcraft_engine::doc::construct::ConstructGeom::Plane(_))
                })?;
                match &c.geom {
                    solidcraft_engine::doc::construct::ConstructGeom::Plane(p) => (p.origin, p.normal().normalized().unwrap_or(Vec3::Z)),
                    _ => return None,
                }
            };
            let t = if occ == 0 { solidcraft_engine::doc::IDENTITY } else { solidcraft_engine::doc::joints::occurrence_world(&s.doc, occ) };
            let at = solidcraft_engine::doc::apply_point(&t, local_origin);
            let z = solidcraft_engine::doc::apply_vector(&t, local_normal).normalized().unwrap_or(Vec3::Z);
            ("", SnapPt { at, z, kind: SnapKind::FaceCenter, param: json!({"occurrence": occ, "plane": plane_name}) })
        }
        Sel::Axis { name } => {
            if let Some(datum) = solidcraft_engine::doc::construct::DatumRef::parse_key(name) {
                let occ = datum.path.last().copied().unwrap_or(0);
                let t = if occ == 0 { solidcraft_engine::doc::IDENTITY } else { solidcraft_engine::doc::joints::occurrence_world(&s.doc, occ) };
                let (at, z) = match datum.item.geom() {
                    solidcraft_engine::doc::construct::ConstructGeom::Axis { origin, dir } => (
                        solidcraft_engine::doc::apply_point(&t, origin),
                        solidcraft_engine::doc::apply_vector(&t, dir).normalized().unwrap_or(Vec3::Z),
                    ),
                    _ => return None,
                };
                return Some(SnapPt { at, z, kind: SnapKind::EdgeMid, param: json!({"occurrence": occ, "axis": name}) });
            }
            let (occ, axis_name) = if let Some((prefix, axis_name)) = name.rsplit_once('/') {
                let occ = s
                    .doc
                    .occurrences
                    .iter()
                    .find(|o| o.name == prefix || o.id.to_string() == prefix)
                    .map(|o| o.id)
                    .or_else(|| s.doc.find_component(prefix).and_then(|c| s.doc.occurrence_of(c)).map(|o| o.id))
                    .unwrap_or(0);
                (occ, axis_name)
            } else {
                let occ = model_st
                    .construct
                    .iter()
                    .find(|c| c.name == *name)
                    .and_then(|c| {
                        let comp = s.doc.feature(c.feature).map(|f| f.component).unwrap_or(0);
                        if comp == 0 { Some(0) } else { s.doc.occurrence_of(comp).map(|o| o.id) }
                    })
                    .unwrap_or(0);
                (occ, name.as_str())
            };
            let upper = axis_name.to_ascii_uppercase();
            let (local_origin, local_dir) = if matches!(upper.as_str(), "X" | "Y" | "Z") {
                let d = match upper.as_str() {
                    "X" => Vec3::X,
                    "Y" => Vec3::Y,
                    _ => Vec3::Z,
                };
                (Vec3::ZERO, d)
            } else {
                let c = model_st.construct.iter().find(|c| c.name == axis_name)?;
                match &c.geom {
                    solidcraft_engine::doc::construct::ConstructGeom::Axis { origin, dir } => (*origin, dir.normalized().unwrap_or(Vec3::Z)),
                    _ => return None,
                }
            };
            let t = if occ == 0 { solidcraft_engine::doc::IDENTITY } else { solidcraft_engine::doc::joints::occurrence_world(&s.doc, occ) };
            let at = solidcraft_engine::doc::apply_point(&t, local_origin);
            let z = solidcraft_engine::doc::apply_vector(&t, local_dir).normalized().unwrap_or(Vec3::Z);
            ("", SnapPt { at, z, kind: SnapKind::EdgeMid, param: json!({"occurrence": occ, "axis": axis_name}) })
        }
        Sel::ConstructionPoint { name } => {
            let c = model_st.construct.iter().find(|c| c.name == *name)?;
            let p = match &c.geom {
                solidcraft_engine::doc::construct::ConstructGeom::Point(p) => *p,
                _ => return None,
            };
            let comp = s.doc.feature(c.feature).map(|f| f.component).unwrap_or(0);
            let occ = if comp == 0 { 0 } else { s.doc.occurrence_of(comp).map(|o| o.id).unwrap_or(0) };
            let t = if occ == 0 { solidcraft_engine::doc::IDENTITY } else { solidcraft_engine::doc::joints::occurrence_world(&s.doc, occ) };
            let at = solidcraft_engine::doc::apply_point(&t, p);
            let z = solidcraft_engine::doc::apply_vector(&t, Vec3::Z).normalized().unwrap_or(Vec3::Z);
            ("", SnapPt { at, z, kind: SnapKind::Vertex, param: json!({"occurrence": occ, "point": pt(at), "z": pt(z)}) })
        }
        Sel::Face { body, index, point } => {
            let b = st.body(body)?;
            let faces = b.joint_faces().ok()?;
            let f = faces.iter().find(|f| f.index == *index)?;
            match f.plane_normal {
                Some(n) => (body.as_str(), SnapPt { at: f.centroid, z: n, kind: SnapKind::FaceCenter, param: json!({"face": pt(*point)}) }),
                None => {
                    let c = solidcraft_engine::kernel::cylinder_face_at(&b.body, *point)?;
                    let sv = (*point - c.axis_point).dot(c.axis);
                    let (at, dir) = if (sv - c.start).abs() <= (c.end - sv).abs() { (c.start, -1.0) } else { (c.end, 1.0) };
                    (
                        body.as_str(),
                        SnapPt {
                            at: c.axis_point + c.axis * at,
                            z: c.axis * dir,
                            kind: SnapKind::CircleCenter,
                            param: json!({"circle": pt(*point)}),
                        },
                    )
                }
            }
        }
        Sel::Edge { body, index, point } => {
            let b = st.body(body)?;
            let m = b.mesh();
            let e = m.edges.get(*index)?;
            match circle_of(e) {
                Some((c, n)) => {
                    // The axis points out of the material: away from the faces' side.
                    let faces = m.edge_faces.get(*index).cloned().unwrap_or_default();
                    let out = faces.iter().filter_map(|f| face_normal_near(&m, *f, *point)).fold(Vec3::ZERO, |a, x| a + x);
                    let z = if out.dot(n) < 0.0 { -n } else { n };
                    // Its round end, kept by name so it follows the hole or boss as the part
                    // changes (a point where it has none).
                    (body.as_str(), SnapPt { at: c, z, kind: SnapKind::CircleCenter, param: json!({"circle": pt(c), "z": pt(z)}) })
                }
                None => {
                    let mid = polyline_mid(e);
                    let z = straight_edge_axis(e)
                        .or_else(|| m.edge_faces.get(*index).and_then(|fs| fs.first()).and_then(|f| face_normal_near(&m, *f, mid)))
                        .unwrap_or(Vec3::Z);
                    // Its middle, kept by the edge's name.
                    (body.as_str(), SnapPt { at: mid, z, kind: SnapKind::EdgeMid, param: json!({"edge": pt(mid), "z": pt(z)}) })
                }
            }
        }
        Sel::Vertex { body, point } => {
            // The corner of the edges meeting there, kept by their names, z along the part's +Z
            // as a point's is.
            let z = occurrence_of_body(s, body)
                .map_or(Vec3::Z, |o| solidcraft_engine::doc::apply_vector(&solidcraft_engine::doc::joints::occurrence_world(&s.doc, o), Vec3::Z));
            (
                body.as_str(),
                SnapPt { at: *point, z: Vec3::Z, kind: SnapKind::Vertex, param: json!({"edge": pt(*point), "corner": pt(*point), "z": pt(z)}) },
            )
        }
        Sel::SketchPoint { id } => {
            // A sketch point's world position, its plane's normal as the joint's z, kept by its
            // sketch and id so it follows the sketch as the part changes. No body names it, so the
            // engine finds the occurrence from the world point (its own body).
            let (sketch, point) = match id.split_once(':') {
                Some((sketch, point)) => (sketch.parse().ok()?, point),
                None => (s.active_sketch?, id.as_str()),
            };
            let ss = st.sketch(sketch)?;
            let at = ss.plane.to_world(ss.sketch.point(ss.sketch.resolve_point(point)?)?);
            let z = ss.plane.normal();
            ("", SnapPt { at, z, kind: SnapKind::Vertex, param: json!({"point": pt(at), "z": pt(z)}) })
        }
        _ => return None,
    };
    // The placement picked on, so a part in a sub-assembly placed more than once is found on the
    // copy it was picked on (#11426).
    if let (Some(path), Value::Object(m)) = (placement_of_body(s, body), &mut snap.param) {
        m.insert("occurrence".into(), json!(path.last().copied().unwrap_or(0)));
        if !path.is_empty() {
            m.insert("path".into(), json!(path));
        }
    }
    Some(snap)
}

/// A pick in a joint origin input, given the woken snap points (#11227, #11476):
/// - an edge of the woken flat face is the face's point on it (a straight edge's middle, a
///   hole's centre), so it lies on the face as the face's other points do;
/// - a woken point that is its item's own snap (a hole's centre, an edge's middle, a face's
///   centre) stands for that item, or for a face of its body crossed on the way to its centre:
///   the joint keeps it by name, as a pick of the item itself would. Any other woken point (a
///   corner, an arc's end, a cylinder's far end) stays a point.
///
/// Only the woken item's own body counts: another body's point or edge there is that body's.
/// `snaps`: whether the dialog's active input takes joint snaps ([`takes_snaps`]). The caller
/// says so, because the overlay draws with the dialog held apart from `app`.
pub(crate) fn woken_pick(app: &SolveApp, snaps: bool, sel: Sel) -> Sel {
    use solidcraft_engine::snap::SnapKind as Kind;
    let woken = &app.viewport.woken;
    if !snaps {
        return sel;
    }
    if let (Sel::Edge { body, .. }, Some(Sel::Face { body: face_body, .. })) = (&sel, woken.item.as_ref())
        && body == face_body
        && let Some(own) = picked_snap(app, &sel)
        && let Some(p) =
            woken.points.iter().find(|p| matches!(p.kind, Kind::Middle | Kind::Centre) && p.at.dist(own.at) <= 1e-4 * (1.0 + own.at.len()))
    {
        return Sel::Vertex { body: body.clone(), point: p.at };
    }
    let Sel::Vertex { body, point } = &sel else { return sel };
    let tol = 1e-4 * (1.0 + point.len());
    let of_body = |i: &&Sel| matches!(i, Sel::Edge { body: b, .. } | Sel::Face { body: b, .. } if b == body);
    if !woken.points.iter().any(|p| p.at.dist(*point) <= tol) {
        return sel;
    }
    // The rim's item, or a face of its body crossed on the way to its centre.
    [&woken.item, &woken.face]
        .into_iter()
        .flatten()
        .filter(of_body)
        .find(|item| picked_snap(app, item).is_some_and(|s| s.at.dist(*point) <= tol))
        .cloned()
        .unwrap_or(sel)
}

/// Whether a dialog's active input takes joint snaps (a joint's origin, a trace's point).
pub(crate) fn takes_snaps(d: &Dialog) -> bool {
    d.active_input().is_some_and(|i| i.accept == SNAPS)
}

/// Each pick's snap, measured once for the world state, design and open sketch it was measured in.
#[derive(Default)]
pub(crate) struct SnapMemo(std::sync::Mutex<Snaps>);

#[derive(Default)]
struct Snaps {
    world: std::sync::Weak<solidcraft_engine::doc::ModelState>,
    doc: std::sync::Weak<solidcraft_engine::doc::Document>,
    active_sketch: Option<u64>,
    /// Least recently used first.
    snaps: Vec<(Sel, Option<SnapPt>)>,
}

/// Snaps kept: the picks, and the hovered items since the last one picked.
const SNAPS_KEPT: usize = 16;

/// [`snap_of`] from the app's memo while the world it was measured in stands.
pub(crate) fn picked_snap(app: &SolveApp, sel: &Sel) -> Option<SnapPt> {
    let s = &app.session;
    let Ok(mut memo) = app.snaps.0.lock() else { return snap_of(s, sel) };
    let world = std::sync::Arc::downgrade(&crate::open_meshing::session_world(s));
    let doc = std::sync::Arc::downgrade(&s.doc);
    if !(memo.world.ptr_eq(&world) && memo.doc.ptr_eq(&doc) && memo.active_sketch == s.active_sketch) {
        *memo = Snaps { world, doc, active_sketch: s.active_sketch, snaps: Vec::new() };
    }
    if let Some(i) = memo.snaps.iter().position(|(x, _)| x == sel) {
        let hit = memo.snaps.remove(i);
        let snap = hit.1.clone();
        memo.snaps.push(hit);
        return snap;
    }
    let snap = snap_of(s, sel);
    if memo.snaps.len() >= SNAPS_KEPT {
        memo.snaps.remove(0);
    }
    memo.snaps.push((sel.clone(), snap.clone()));
    snap
}

/// Where a woken point picked in a joint origin input came from, when a hovered flat face
/// offered it (#11476): one of the face's corners, edge middles or hole centres, with the face,
/// its outward normal and the edge. `None` for anything else, the face's own centre included
/// (that one is picked as the face, [`woken_pick`]). `snaps`: the active input takes joint
/// origins (the caller knows: the overlay holds the dialog apart from `app`).
pub(crate) fn face_point_source(
    app: &SolveApp,
    snaps: bool,
    sel: &Sel,
) -> Option<(solidcraft_engine::snap::SnapKind, solidcraft_engine::snap::SnapSource)> {
    use solidcraft_engine::snap::SnapKind as Kind;
    let Sel::Vertex { point, .. } = sel else { return None };
    let item = app.viewport.woken.item.as_ref().filter(|i| snaps && matches!(i, Sel::Face { .. }))?;
    let tol = 1e-4 * (1.0 + point.len());
    solidcraft_engine::snap::snap_points_from(&app.session, item)
        .into_iter()
        .find(|(p, from)| p.at.dist(*point) <= tol && matches!(p.kind, Kind::End | Kind::Middle | Kind::Centre) && from.normal.is_some())
        .map(|(p, from)| (p.kind, from))
}

/// A face's corner, edge middle or hole centre as a joint origin that lies on the face: z along
/// the face's outward normal, the face, edge and corner kept by name (`joint.create`'s `edge`
/// with `on` and no `z`; a hole's `circle`).
fn face_tied(s: &Session, sel: &Sel, kind: solidcraft_engine::snap::SnapKind, from: &solidcraft_engine::snap::SnapSource) -> Option<SnapPt> {
    use solidcraft_engine::snap::SnapKind as Kind;
    let Sel::Vertex { body, point } = sel else { return None };
    let (face, normal) = (from.face?, from.normal?);
    let (kind, mut param) = match kind {
        Kind::End => (SnapKind::Vertex, json!({"edge": pt(from.edge?), "corner": pt(*point), "on": pt(face)})),
        Kind::Middle => (SnapKind::EdgeMid, json!({"edge": pt(from.edge?), "on": pt(face)})),
        Kind::Centre => (SnapKind::CircleCenter, json!({"circle": pt(*point), "z": pt(normal)})),
        _ => return None,
    };
    if let (Some(occ), Value::Object(m)) = (occurrence_of_body(s, body), &mut param) {
        m.insert("occurrence".into(), json!(occ));
    }
    Some(SnapPt { at: *point, z: normal, kind, param })
}

/// The snap an input's pick stands for: tied to the face it came from when a hovered face
/// offered it ([`SelInput::from_face`]), else the pick's own snap ([`picked_snap`]).
pub fn snap_in(app: &SolveApp, input: &SelInput, sel: &Sel) -> Option<SnapPt> {
    input
        .from_face
        .iter()
        .find(|(x, ..)| x == sel)
        .and_then(|(_, kind, from)| face_tied(&app.session, sel, *kind, from))
        .or_else(|| picked_snap(app, sel))
}

/// The snap under the pointer in a joint origin input: tied to the hovered face when the face
/// offered it, else the pick's own snap ([`picked_snap`]).
fn snap_hovered(app: &SolveApp, sel: &Sel) -> Option<SnapPt> {
    face_point_source(app, true, sel).and_then(|(kind, from)| face_tied(&app.session, sel, kind, &from)).or_else(|| picked_snap(app, sel))
}

/// A snap moved across its own plane by (x, y) (the joint's offset), as an explicit frame.
fn shifted(s: &SnapPt, x: f64, y: f64) -> Value {
    let ax = s.z.any_perp();
    let ay = s.z.cross(ax);
    let mut p = json!({"frame": {"origin": pt(s.at + ax * x + ay * y), "z": pt(s.z), "x": pt(ax)}});
    if let Some(path) = s.param.get("path") {
        p["path"] = path.clone();
    }
    if let Some(o) = s.param.get("occurrence") {
        p["occurrence"] = o.clone();
    }
    p
}

// ---- rows ----

/// The dialog's own rows (after the selection inputs); true when Enter was pressed in a value.
pub fn rows(app: &mut SolveApp, ui: &mut egui::Ui, k: &mut Asm, inputs: &mut [SelInput], active: &mut usize) -> bool {
    let t = Tokens::get();
    let mut enter = false;
    let problem = joint_problem(app, k, inputs);
    match k {
        Asm::FromBodies => {
            ui.add(egui::Label::new(app.tr("ui.components_from_bodies_hint")).wrap());
            if crate::ui_kit::button(ui, app.tr("ui.select_all"), crate::ui_kit::Button::Normal).clicked()
                && let Some(input) = inputs.first_mut()
            {
                // Transfers take definition names, rather than the names of repeated placements.
                input.items = app.session.model.state().bodies.iter().map(|body| Sel::Body { name: body.name.clone() }).collect();
                *active = 0;
            }
        }
        Asm::NewComponent { name, activate } => {
            enter |= text_row(ui, "Name", name, solidcraft_ui_text::tr_label("Component1")).0;
            crate::ui_kit::options(ui, "component_options", &mut [("Activate", activate)]);
        }
        Asm::Joint(f) => {
            // As-built: two components picked, the position comes next.
            if f.as_built && *active == 0 && inputs.first().is_some_and(|i| i.items.len() >= 2) {
                *active = 1;
            }
            // Picks that make no joint say why, under them, and offer the way out (#1239).
            if let Some((why, bodies)) = problem {
                ui.vertical(|ui| {
                    crate::ui_kit::status_line(ui, &why, crate::ui_kit::StatusTone::Warning);
                    if !bodies.is_empty()
                        && crate::ui_kit::button(ui, solidcraft_ui_text::tr("cmd.component_from_bodies"), crate::ui_kit::Button::Normal)
                            .backing_on_hover_text(solidcraft_ui_text::tr("joint.from_bodies_hint"))
                            .clicked()
                    {
                        // A refusal shows on the status line; the picks stay either way.
                        let _ = app.run("component.from_bodies", json!({ "bodies": bodies }));
                    }
                });
            }
            form_row(ui, "Motion", |ui| {
                ui.label("");
            });
            let was = f.kind;
            choice_row(ui, "Type", "jt_type", &mut f.kind, &TYPE_LABELS);
            if f.kind != was {
                // What Find Limits found was for the other type's values.
                (f.search, f.found_note, f.searched) = (None, None, None);
                f.found.clear();
            }
            let hint = default_joint_name(&app.session.doc, f.joint_kind());
            enter |= text_row(ui, "Name", &mut f.name, &hint).0;
            f.fit_limits(&app.session.doc);
            if !f.as_built {
                enter |= expr_row(ui, "Angle", &mut f.angle);
                crate::ui_kit::section(ui, "joint_position", "Position", false, |ui| {
                    for (l, v) in ["Offset X", "Offset Y", "Offset Z"].iter().zip(f.offset.iter_mut()) {
                        if f.editing.is_some() && *l != "Offset Z" {
                            continue;
                        }
                        enter |= expr_row(ui, l, v);
                    }
                });
                crate::ui_kit::options(ui, "joint_options", &mut [("Flip", &mut f.flip)]);
            }
            let kind = f.joint_kind();
            if kind != JointKind::Rigid {
                form_row(ui, "Animate", |ui| {
                    let label = solidcraft_ui_text::tr(if f.animate { "ui.joint_stop" } else { "ui.joint_play" });
                    if crate::ui_kit::button(ui, label, crate::ui_kit::Button::Normal)
                        .backing_on_hover_text(solidcraft_ui_text::tr_label("Play the joint's motion in the view"))
                        .clicked()
                    {
                        f.animate = !f.animate;
                    }
                });
                if std::mem::take(&mut f.show_limits) {
                    let id = ui.make_persistent_id("joint_limits");
                    ui.ctx().data_mut(|d| d.insert_persisted(id, true));
                }
                crate::ui_kit::section(ui, "joint_limits", "Limits", false, |ui| {
                    let found = f.found.clone();
                    for (i, (on, lo, hi)) in f.limits.iter_mut().enumerate() {
                        let name = kind.dofs().get(i).copied().unwrap_or("value");
                        form_row(ui, &format!("{} limits", capital(name)), |ui| {
                            crate::ui_kit::checkbox(ui, "", on);
                        });
                        if *on {
                            enter |= expr_row(ui, "  Minimum", lo);
                            enter |= expr_row(ui, "  Maximum", hi);
                        }
                        if let Some(note) = found.get(i).filter(|n| !n.is_empty()) {
                            ui.vertical(|ui| {
                                for line in note.lines() {
                                    ui.add(egui::Label::new(RichText::new(line).size(t.font_small).color(t.text_dim)).wrap());
                                }
                            });
                        }
                    }
                    if let Some(id) = f.editing {
                        find_limits_row(app, ui, f, id, inputs);
                    }
                });
                if f.editing.is_some() {
                    crate::ui_kit::section(ui, "joint_dynamics", "Dynamics", false, |ui| {
                        for (i, row) in f.dynamics.iter_mut().enumerate() {
                            let angle = kind.is_angle(i);
                            let name = kind.dofs().get(i).copied().unwrap_or("value");
                            crate::ui_kit::text(ui, &capital(name), crate::ui_kit::Text::Note(&format!("dynamics_{i}")));
                            let units = if angle { ["N m", "rad/s", "N m s/rad", "N m"] } else { ["N", "m/s", "N s/m", "N"] };
                            for ((label, value), unit) in ["Effort", "Velocity", "Damping", "Friction"].into_iter().zip(row.iter_mut()).zip(units) {
                                enter |= text_row(ui, label, value, unit).0;
                            }
                        }
                    });
                }
            } else {
                f.animate = false;
            }
            if f.animate {
                let now = ui.input(|i| i.time);
                // 48 steps over 3 s: each one a preview.
                f.phase = ((now / 3.0).fract() * 48.0).floor() / 48.0;
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
            }
        }
        Asm::JointOrigin { name, .. } => {
            enter |= text_row(ui, "Name", name, solidcraft_ui_text::tr_label("Joint Origin1")).0;
        }
        Asm::RigidGroup => {}
        Asm::Drive { joint, values, focus } => {
            let s = &app.session;
            let list = movable(s);
            // A rigid joint holds its parts still: say so, and offer Edit Joint to change its
            // type (#6981). Nothing is driven (no preview, OK stays off) until one that moves is chosen.
            let rigid = s.doc.assembly.joints.get(*joint).filter(|j| j.kind == JointKind::Rigid).map(|j| (j.id, j.name.clone()));
            if let Some((id, name)) = &rigid {
                let mut edit = false;
                ui.vertical(|ui| {
                    let why = solidcraft_ui_text::format("drive.rigid", &[("joint", name.as_str())]);
                    crate::ui_kit::status_line(ui, &why, crate::ui_kit::StatusTone::Warning);
                    let r = crate::ui_kit::button(ui, solidcraft_ui_text::tr_label("Edit Joint"), crate::ui_kit::Button::Normal);
                    crate::scenario::publish_handle("drive:edit_joint", r.rect.center());
                    edit = r.clicked();
                });
                if edit && let Some(Kind::Assembly(e)) = edit_joint(app, *id).map(|d| d.kind) {
                    *k = e;
                    return false;
                }
            }
            if list.is_empty() {
                if rigid.is_none() {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(solidcraft_ui_text::tr_label("no joint that moves yet")).color(t.text_dim));
                    });
                }
                return false;
            }
            let labels: Vec<String> = list.iter().filter_map(|i| s.doc.assembly.joints.get(*i)).map(|j| joint_choice(&j.name, j.kind)).collect();
            let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
            // On a rigid joint the box shows none of the joints that move until one is chosen.
            let mut pos = list.iter().position(|i| i == joint).unwrap_or(list.len());
            choice_row(ui, "Joint", "drv_joint", &mut pos, &refs);
            if let Some(&pick) = list.get(pos)
                && (pick != *joint || values.is_empty())
            {
                *joint = pick;
                *values = current_values(s, pick);
            }
            let Some(j) = s.doc.assembly.joints.get(*joint).filter(|j| j.kind != JointKind::Rigid).cloned() else { return false };
            for (i, v) in values.iter_mut().enumerate() {
                let angle = j.kind.is_angle(i);
                let unit_kind = if angle { ValueKind::Angle } else { ValueKind::Length };
                let name = j.kind.dofs().get(i).copied().unwrap_or("value");
                form_row(ui, &capital(name), |ui| {
                    if i == 0 {
                        // Typing replaces the value at once, as in the other dialogs' value boxes.
                        let id = egui::Id::unique("sc_drive_value");
                        let r = crate::ui_kit::cell_editor(ui, id, v, "", false);
                        crate::params_dialog::complete(ui, &r, v);
                        if std::mem::take(focus) {
                            r.request_focus();
                            let n = v.chars().count();
                            let mut st = egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
                            st.cursor.set_char_range(Some(egui::text_selection::CCursorRange::two(
                                egui::text::CCursor::new(0),
                                egui::text::CCursor::new(n),
                            )));
                            st.store(ui.ctx(), id);
                        }
                        enter |= r.lost_focus() && ui.input(|x| x.key_pressed(egui::Key::Enter));
                    } else {
                        enter |= field(ui, v);
                    }
                });
                // A slider over the limits (or a turn, or ±100 mm).
                let (lo, hi) = match j.limits.get(i).copied().flatten() {
                    Some((a, b)) => (a.min(b), a.max(b)),
                    None if angle => (-std::f64::consts::PI, std::f64::consts::PI),
                    None => (-100.0, 100.0),
                };
                let cur = crate::length_input::eval(&s.doc, v, unit_kind).ok().filter(|x| x.is_finite()).unwrap_or(0.0);
                let (mut shown, scale) = if angle { (cur.to_degrees(), 1f64.to_degrees()) } else { (cur, 1.0) };
                ui.vertical(|ui| {
                    let r = crate::ui_kit::slider(
                        ui,
                        &mut shown,
                        lo * scale..=hi * scale,
                        crate::ui_kit::Slider { show_value: false, ..Default::default() },
                    );
                    crate::scenario::publish_handle(&format!("drive_slider:{i}"), r.rect.center());
                    if r.changed() {
                        *v = if angle { format!("{shown:.1} deg") } else { crate::length_input::model(&s.doc, shown) };
                    }
                });
                // The slider's range: the joint's limits, or the default span when it has none.
                let limited = j.limits.get(i).copied().flatten().is_some();
                let fmt = |x: f64| {
                    if !angle {
                        return crate::prefs::show_mm(app, x, 1);
                    }
                    let (x, unit) = (x.to_degrees(), "°");
                    if x.fract().abs() < 1e-6 { format!("{x:.0}{unit}") } else { format!("{x:.1}{unit}") }
                };
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(format!("{} … {}{}", fmt(lo), fmt(hi), if limited { "  (limits)" } else { "  (no limits)" }))
                            .size(solidcraft_theme::Tokens::get().font_small)
                            .color(if limited { t.text } else { t.text_dim }),
                    );
                });
            }
            // The preview drove the joint: where it stopped short, and why (a closed loop or a
            // contact set), so the values asked for are not taken for where it is.
            if let Some(why) = app.preview.stopped.as_ref().and_then(drive_stop) {
                ui.vertical(|ui| {
                    ui.label(RichText::new(why).color(t.warning));
                });
            }
            // The moved parts' old place, drawn as a ghost; remembered between uses (#5905).
            form_row(ui, "Show ghost", |ui| {
                crate::ui_kit::checkbox(ui, "", &mut app.preferences.drive_joints_ghost);
            });
        }
        Asm::MotionLink { a, b, ia, ib, ratio, offset, editing: _ } => {
            let s = &app.session;
            let joints = &s.doc.assembly.joints;
            if joints.len() < 2 {
                crate::ui_kit::status_line(ui, "needs two joints", crate::ui_kit::StatusTone::Dim);
                return false;
            }
            let labels: Vec<&str> = joints.iter().map(|j| j.name.as_str()).collect();
            for (l, sel, idx, id) in [("Joint 1", &mut *a, &mut *ia, "ml_a"), ("Joint 2", &mut *b, &mut *ib, "ml_b")] {
                choice_row(ui, l, id, sel, &labels);
                let dofs = joints.get(*sel).map(|j| j.kind.dofs()).unwrap_or(&[]);
                if dofs.len() > 1 {
                    form_row(ui, "  Value", |ui| {
                        let caps: Vec<String> = dofs.iter().map(|d| capital(d)).collect();
                        let refs: Vec<&str> = caps.iter().map(String::as_str).collect();
                        combo(ui, &format!("{id}_i"), &refs, idx);
                    });
                } else {
                    *idx = 0;
                }
            }
            enter |= expr_row(ui, "Ratio", ratio);
            enter |= expr_row(ui, "Offset", offset);
        }
        Asm::Coupling(f) => enter |= crate::dialogs_coupling::rows(app, ui, f, inputs),
        Asm::Cam(f) => enter |= crate::dialogs_coupling::cam::rows(app, ui, f, inputs),
        Asm::Constrain(f) => enter |= crate::dialogs_constrain::rows(app, ui, f, inputs, active),
        Asm::Interference { result, overlaps, of, clearance, check } => {
            let picked: Vec<Sel> = inputs.first().map(|i| i.items.clone()).unwrap_or_default();
            if let Some(c) = check.take() {
                match c.reply.lock().map(|mut r| r.take()).map_err(|_| ()) {
                    Ok(None) => {
                        *check = Some(c);
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                    }
                    // Done: shown unless the design changed meanwhile (a script, the control
                    // channel, undo). A poisoned lock (its thread panicked) leaves nothing.
                    Ok(Some((r, o))) => {
                        if c.design.upgrade().is_some_and(|d| std::sync::Arc::ptr_eq(&d, &app.session.doc)) {
                            (*result, *overlaps, *of) = (Some(r), o, c.of);
                        }
                        ui.ctx().request_repaint();
                    }
                    Err(()) => {}
                }
            }
            enter |= expr_row(ui, "Clearance", clearance);
            // While it checks: the seconds so far, and Cancel where Compute was.
            if let Some(started) = check.as_ref().map(|c| c.started) {
                let t = Tokens::get();
                form_row(ui, "", |ui| {
                    ui.horizontal(|ui| {
                        let seconds = ((crate::now_ms() - started) / 1000.0).floor().max(0.0);
                        ui.add(egui::Spinner::new().size(t.font_small));
                        ui.label(
                            RichText::new(solidcraft_ui_text::format("dialog.computing", &[("seconds", &format!("{seconds:.0}"))])).color(t.text_dim),
                        );
                    });
                });
            }
            form_row(ui, "", |ui| {
                if check.is_some() {
                    if crate::ui_kit::button(ui, solidcraft_ui_text::tr("ui.cancel"), crate::ui_kit::Button::Normal).clicked() {
                        // Its thread finishes the check; the reply is left unread.
                        *check = None;
                    }
                    return;
                }
                let stale = result.is_some() && *of != picked;
                if crate::ui_kit::button(
                    ui,
                    solidcraft_ui_text::tr(if stale { "dialog.compute_again" } else { "dialog.compute" }),
                    crate::ui_kit::Button::Normal,
                )
                .clicked()
                {
                    (*result, *overlaps) = (None, Vec::new());
                    *check = Some(interference(app, &picked, clearance));
                    ui.ctx().request_repaint();
                }
            });
            if check.is_none()
                && let Some(r) = result.as_ref()
            {
                interference_rows(app, ui, r);
            }
        }
        Asm::Trace(f) => enter |= trace::rows(app, ui, f, inputs),
    }
    enter
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// The Limits section's Find Limits row: the button, or the search under way with Cancel; and
/// what the last search said about all the values.
fn find_limits_row(app: &mut SolveApp, ui: &mut egui::Ui, f: &mut JointForm, id: u64, inputs: &[SelInput]) {
    let t = Tokens::get();
    if let Some(search) = &f.search {
        // A poisoned lock: the search's thread panicked.
        let reply =
            search.reply.lock().map(|mut r| r.take()).unwrap_or_else(|_| Some(Err(solidcraft_ui_text::tr("joint.find_limits_failed").to_string())));
        match reply {
            Some(reply) => {
                // Found for a design changed since (by a script, the control channel, undo), or
                // a joint the dialog has changed since (its sides, flip, angle or offset): dropped.
                let searched = search.searched.clone();
                f.search = None;
                if searched.current(app, f, inputs) {
                    f.searched = Some(searched);
                    fill_limits(&app.session.doc, f, reply);
                } else {
                    f.found_note = Some(solidcraft_ui_text::tr("joint.find_limits_stale").to_string());
                }
                // The rows above were drawn before this: once more, with what was found.
                ui.ctx().request_repaint();
            }
            None => ui.ctx().request_repaint_after(std::time::Duration::from_millis(100)),
        }
    }
    // What it found stops applying once the joint or the design changes: its notes go, and the
    // dialog says so (the values filled in stay, to keep or edit).
    if f.search.is_none()
        && let Some(searched) = &f.searched
        && !searched.current(app, f, inputs)
    {
        f.searched = None;
        f.found.clear();
        f.found_note = Some(solidcraft_ui_text::tr("joint.find_limits_stale").to_string());
    }
    form_row(ui, "", |ui| {
        if f.search.is_some() {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(t.font_small));
                ui.label(RichText::new(solidcraft_ui_text::tr("joint.finding_limits")).color(t.text_dim));
                if crate::ui_kit::button(ui, solidcraft_ui_text::tr("ui.cancel"), crate::ui_kit::Button::Normal).clicked() {
                    // Its thread finishes within its work budget; the reply is left unread.
                    f.search = None;
                }
            });
        } else if crate::ui_kit::button(ui, solidcraft_ui_text::tr("joint.find_limits"), crate::ui_kit::Button::Normal)
            .backing_on_hover_text(solidcraft_ui_text::tr("joint.find_limits_hint"))
            .clicked()
        {
            find_limits(app, f, id, inputs);
        }
    });
    if let Some(note) = &f.found_note {
        ui.vertical(|ui| {
            ui.add(egui::Label::new(RichText::new(note).color(t.warning)).wrap());
        });
    }
}

/// The commands OK would run for the joint as the dialog has it (its sides as picked in
/// `inputs`), but its limits (what Find Limits fills in).
fn edit_commands(app: &SolveApp, f: &JointForm, inputs: &[SelInput]) -> Result<Vec<(String, Value)>, String> {
    let edit = Dialog::new(Kind::Assembly(Asm::Joint(Box::new(f.clone()))), inputs.to_vec());
    Ok(crate::dialogs::apply_commands(app, &edit)?.into_iter().filter(|(id, _)| id != "joint.limits").collect())
}

/// Find Limits: on a copy of the design, the joint as the dialog has it now (its type and
/// alignment, as OK would leave them), then `joint.find_limits` on it. Off the window thread
/// when the app runs commands there, else at once.
fn find_limits(app: &SolveApp, f: &mut JointForm, id: u64, inputs: &[SelInput]) {
    (f.found_note, f.search, f.searched) = (None, None, None);
    f.found.clear();
    let searched = match Searched::now(app, f, inputs) {
        Ok(s) => s,
        Err(e) => {
            f.found_note = Some(e);
            return;
        }
    };
    // A joint OK would refuse (an expression that doesn't read) says so at once, before any search.
    let mut probe = app.session.scratch();
    if let Some(e) = searched.joint.iter().find_map(|(id, p)| probe.execute(id, p).err()) {
        f.found_note = Some(e.to_string());
        return;
    }
    let mut commands = searched.joint.clone();
    commands.push(("joint.find_limits".into(), json!({ "joint": id })));
    // The search's reply, or the first command that failed.
    fn reply<E: std::fmt::Display>(replies: Vec<Result<Value, E>>) -> Result<Value, String> {
        let mut last = Err(String::new());
        for r in replies {
            last = Ok(r.map_err(|e| e.to_string())?);
        }
        last
    }
    #[cfg(not(target_arch = "wasm32"))]
    if app.apply_off_thread.is_some() {
        let search = Search { reply: Default::default(), searched: searched.clone() };
        let (ahead, out, list) = (app.session.ahead(), search.reply.clone(), commands.clone());
        let spawned = std::thread::Builder::new().name("find limits".into()).spawn(move || {
            let r = reply(ahead.replies(&list));
            if let Ok(mut slot) = out.lock() {
                *slot = Some(r);
            }
        });
        if spawned.is_ok() {
            f.search = Some(search);
            return;
        }
    }
    f.searched = Some(searched);
    fill_limits(&app.session.doc, f, reply(app.session.ahead().replies(&commands)));
}

/// Fill the limits rows from `joint.find_limits`'s reply: each side found, rounded inward (a
/// tenth of a degree, a hundredth of the design's length unit), and its row ticked once both
/// are; a side with nothing to meet keeps what it had. A line under each row says what stopped
/// each side.
fn fill_limits(doc: &solidcraft_engine::doc::Document, f: &mut JointForm, reply: Result<Value, String>) {
    let r = match reply {
        Ok(r) => r,
        Err(e) => {
            (f.found_note, f.searched) = (Some(e), None);
            return;
        }
    };
    let kind = f.joint_kind();
    let unit = crate::length_input::unit(doc).to_string();
    let scale = solidcraft_engine::doc::expr::unit_info(&unit).map(|(_, scale)| scale).filter(|s| *s > 0.0).unwrap_or(1.0);
    f.found = vec![String::new(); f.limits.len()];
    for v in r["values"].as_array().into_iter().flatten() {
        let Some(i) = v["index"].as_u64().and_then(|i| usize::try_from(i).ok()) else { continue };
        let (Some(row), Some(note)) = (f.limits.get_mut(i), f.found.get_mut(i)) else { continue };
        let angle = kind.is_angle(i);
        // A side as the dialog shows it: rounded toward the joint's value, so it stays clear; as
        // found where rounding would pass the joint's value (a stop nearer than the rounding).
        let (min, max) = (v["min"].as_f64().filter(|x| x.is_finite()), v["max"].as_f64().filter(|x| x.is_finite()));
        let now = v["value"].as_f64().filter(|x| x.is_finite());
        let step = if angle { 0.1 } else { 0.01 * scale };
        let lo = min.map(|x| Some((x / step - 1e-6).ceil() * step).filter(|r| now.is_none_or(|c| *r <= c)).unwrap_or(x));
        let hi = max.map(|x| Some((x / step + 1e-6).floor() * step).filter(|r| now.is_none_or(|c| *r >= c)).unwrap_or(x));
        let (lo, hi) = if lo.zip(hi).is_some_and(|(lo, hi)| lo > hi) { (min, max) } else { (lo, hi) };
        let shown = |x: f64| {
            if angle { (num(x, "deg"), format!("{}°", crate::dialogs_coupling::trim(x))) } else { (num(x / scale, &unit), num(x / scale, &unit)) }
        };
        let (min, max) = (lo.map(shown), hi.map(shown));
        let mut lines = Vec::new();
        for (label, found, by) in [("Minimum", &min, &v["min_by"]), ("Maximum", &max, &v["max_by"])] {
            let which = solidcraft_ui_text::tr_label(label);
            let value = found.as_ref().map(|(_, shown)| shown.as_str()).unwrap_or_default();
            lines.push(match by.as_str() {
                Some("loop") => solidcraft_ui_text::format("joint.limit_loop", &[("side", which), ("value", value)]),
                Some("limit") => solidcraft_ui_text::format("joint.limit_limit", &[("side", which), ("value", value)]),
                Some("turn") => solidcraft_ui_text::format("joint.limit_turn", &[("side", which), ("value", value)]),
                Some(_) => {
                    // Down to a tenth, so it never claims more than was searched.
                    let range = (v["range"].as_f64().filter(|x| x.is_finite()).unwrap_or(0.0) / scale * 10.0).floor() / 10.0;
                    solidcraft_ui_text::format("joint.limit_range", &[("side", which), ("range", &num(range, &unit))])
                }
                None => {
                    let (a, b) = (by["a"].as_str().unwrap_or_default(), by["b"].as_str().unwrap_or_default());
                    solidcraft_ui_text::format("joint.limit_contact", &[("side", which), ("value", value), ("a", a), ("b", b)])
                }
            });
        }
        *note = lines.join("\n");
        // A side with nothing to meet has no limit to fill in: the row keeps what it had.
        row.0 |= min.is_some() && max.is_some();
        if let Some((lo, _)) = min {
            row.1 = lo;
        }
        if let Some((hi, _)) = max {
            row.2 = hi;
        }
    }
    let overlapping: Vec<String> = r["overlapping_at_start"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|o| format!("{} / {}", o["a"].as_str().unwrap_or_default(), o["b"].as_str().unwrap_or_default()))
        .collect();
    if !overlapping.is_empty() {
        f.found_note = Some(solidcraft_ui_text::format("joint.limits_overlapping", &[("pairs", &overlapping.join(", "))]));
    }
}

/// Start Compute's check (overlaps, pairs closer than the clearance, each body's nearest
/// neighbour) and find the overlapping volumes (world placement) to show in red. Off the window
/// thread, on a kernel thread, when the app runs commands there: a slow pair froze the window
/// until the app was closed (#11483). Else at once.
fn interference(app: &SolveApp, picked: &[Sel], clearance: &str) -> Check {
    let check =
        Check { reply: Default::default(), design: std::sync::Arc::downgrade(&app.session.doc), of: picked.to_vec(), started: crate::now_ms() };
    let names: Vec<String> = picked.body_names();
    let (scale, _) = crate::prefs::unit_scale(&app.session.doc.units);
    let reply = match crate::prefs::parse_length_mm(clearance, scale) {
        Some(v) if v >= 0.0 => {
            let mut p = json!({ "clearance": v, "nearest": 1 });
            if !names.is_empty() {
                p["bodies"] = json!(names);
            }
            #[cfg(not(target_arch = "wasm32"))]
            if app.apply_off_thread.is_some() {
                let (ahead, world, out, params) =
                    (app.session.ahead(), crate::open_meshing::session_world(&app.session), check.reply.clone(), p.clone());
                let spawned = solidcraft_engine::kernel::kernel_thread("interference").spawn(move || {
                    let r = check_now(ahead, &world, params);
                    if let Ok(mut slot) = out.lock() {
                        *slot = Some(r);
                    }
                });
                if spawned.is_ok() {
                    return check;
                }
            }
            check_now(app.session.ahead(), &crate::open_meshing::session_world(&app.session), p)
        }
        _ => (json!({ "error": solidcraft_ui_text::tr("dialog.clearance_invalid") }), Vec::new()),
    };
    if let Ok(mut slot) = check.reply.lock() {
        *slot = Some(reply);
    }
    check
}

/// `inspect.clearance` with `p` on a copy of the session, and the volume each pair it finds
/// overlapping shares ([`overlap_meshes`]).
fn check_now(ahead: solidcraft_engine::Ahead, world: &solidcraft_engine::doc::ModelState, p: Value) -> (Value, Vec<Mesh>) {
    // One reply for the one command.
    match ahead.replies(&[("inspect.clearance".to_string(), p)]).pop() {
        Some(Ok(r)) => {
            let meshes = overlap_meshes(world, &r);
            (r, meshes)
        }
        Some(Err(e)) => (json!({ "error": e.to_string() }), Vec::new()),
        None => (json!({}), Vec::new()),
    }
}

/// The volume each pair in an `inspect.clearance` reply's `overlaps` shares, meshed in world
/// placement. Each pair runs on a kernel worker with the kernel's stack, whatever thread asks
/// (the window thread's is 1 MiB on Windows).
fn overlap_meshes(st: &solidcraft_engine::doc::ModelState, r: &Value) -> Vec<Mesh> {
    use solidcraft_engine::kernel;
    let pairs: Vec<_> =
        r["overlaps"].as_array().into_iter().flatten().filter_map(|hit| Some((st.body(hit["a"].as_str()?)?, st.body(hit["b"].as_str()?)?))).collect();
    kernel::map_with_local_pools(&pairs, usize::MAX, |&(a, b)| {
        let Some(o) = kernel::boolean(&a.body, &b.body, kernel::BoolOp::Intersect)? else { return Ok(None) };
        o.mesh(&kernel::MeshTolerance::chord((o.size() * 2e-3).max(1e-3))).map(Some)
    })
    .into_iter()
    .filter_map(|m| m.ok().flatten())
    .collect()
}

fn interference_rows(app: &SolveApp, ui: &mut egui::Ui, r: &Value) {
    let t = Tokens::get();
    if let Some(e) = r.get("error").and_then(Value::as_str) {
        crate::ui_kit::status_line(ui, e, crate::ui_kit::StatusTone::Error);
        return;
    }
    let hits = r["overlaps"].as_array().cloned().unwrap_or_default();
    form_row(ui, "Checked", |ui| {
        ui.label(solidcraft_ui_text::format("dialog.bodies_count", &[("count", &r["bodies"].as_u64().unwrap_or(0).to_string())]));
    });
    let unchecked = r["unchecked"].as_array().cloned().unwrap_or_default();
    let near = r["near"].as_array().cloned().unwrap_or_default();
    let pair = |h: &Value| format!("{} · {}", h["a"].as_str().unwrap_or(""), h["b"].as_str().unwrap_or(""));
    if hits.is_empty() && unchecked.is_empty() {
        crate::ui_kit::status_line(ui, "no interference", crate::ui_kit::StatusTone::Dim);
    }
    // Pairs closer than the clearance, nearest first.
    if !near.is_empty() {
        form_row(ui, "Too close", |ui| {
            ui.label(RichText::new(solidcraft_ui_text::tr("ui.distance")).semibold().color(solidcraft_theme::Tokens::get().text));
        });
    }
    for h in &near {
        form_row(ui, &pair(h), |ui| {
            ui.label(RichText::new(crate::prefs::show_mm(app, h["distance_mm"].as_f64().unwrap_or(0.0), 1)).color(t.warning));
        });
    }
    // Each body's nearest neighbour.
    let nearest = r["nearest"].as_object().cloned().unwrap_or_default();
    if !nearest.is_empty() {
        form_row(ui, "Nearest", |ui| {
            ui.label(RichText::new(solidcraft_ui_text::tr("ui.distance")).semibold().color(solidcraft_theme::Tokens::get().text));
        });
    }
    for (body, list) in &nearest {
        let Some(n) = list.get(0) else { continue };
        form_row(ui, &format!("{body} → {}", n["body"].as_str().unwrap_or("")), |ui| {
            ui.label(crate::prefs::show_mm(app, n["distance_mm"].as_f64().unwrap_or(0.0), 1));
        });
    }
    if hits.is_empty() && unchecked.is_empty() {
        return;
    }
    if !hits.is_empty() {
        form_row(ui, "Interferes", |ui| {
            ui.label(RichText::new(solidcraft_ui_text::tr_label("Volume")).semibold().color(solidcraft_theme::Tokens::get().text));
        });
    }
    for h in hits {
        form_row(ui, &format!("{} · {}", h["a"].as_str().unwrap_or(""), h["b"].as_str().unwrap_or("")), |ui| {
            ui.label(RichText::new(crate::prefs::show_mm(app, h["volume_mm3"].as_f64().unwrap_or(0.0), 3)).color(t.error));
        });
    }
    // Pairs the check could not work out: not clear, not known to interfere.
    for u in unchecked {
        form_row(ui, &format!("{} · {}", u["a"].as_str().unwrap_or(""), u["b"].as_str().unwrap_or("")), |ui| {
            ui.label(RichText::new(solidcraft_ui_text::tr("dialog.could_not_be_checked")).color(t.warning))
                .backing_on_hover_text(u["reason"].as_str().unwrap_or(""));
        });
    }
}

// ---- commands ----

/// The body a pick is on.
pub(crate) fn body_of(sel: &Sel) -> Option<&str> {
    match sel {
        Sel::Face { body, .. } | Sel::Edge { body, .. } | Sel::Vertex { body, .. } => Some(body),
        Sel::Body { name } => Some(name),
        _ => None,
    }
}

/// Why the Joint dialog's picks, all in, make no joint: without it the preview and OK just go
/// quiet, and no option seems to do anything (#1239). With it, the bodies Create Components from
/// Bodies would part (none when that would not help). The usual case is every pick on one
/// component: a STEP file with several parts often brings them as bodies of one component.
fn joint_problem(app: &SolveApp, k: &Asm, inputs: &[SelInput]) -> Option<(String, Vec<String>)> {
    let Asm::Joint(f) = k else { return None };
    let picks: Vec<&Sel> =
        if f.as_built { sel_items(inputs, 0).iter().collect() } else { (0..2).filter_map(|i| sel_items(inputs, i).first()).collect() };
    if f.editing.is_some() || picks.len() < 2 {
        return None;
    }
    let mut bodies: Vec<String> = Vec::new();
    for b in picks.iter().filter_map(|p| body_of(p)) {
        if !bodies.iter().any(|x| x == b) {
            bodies.push(b.to_string());
        }
    }
    // The occurrence a joint at each body would hold: a part in a later copy of a sub-assembly is
    // held through that copy, so picks on the same part in two copies are two sides (#11426).
    let doc = &app.session.doc;
    let occs: Vec<Option<u64>> =
        bodies.iter().map(|b| placement_of_body(&app.session, b).map(|path| solidcraft_engine::doc::joints::origin_anchor(doc, &path).0)).collect();
    // Only every pick naming a body makes the one-component case a problem: a datum plane or axis
    // side names its placement's origin, not a body, so a datum of one part against a face of
    // another is still a two-part joint.
    if picks.iter().all(|p| body_of(p).is_some()) && occs.first().is_some_and(|first| first.is_some() && occs.iter().all(|o| o == first)) {
        return Some(match bodies.as_slice() {
            [body] => (solidcraft_ui_text::format("joint.one_body", &[("body", body.as_str())]), Vec::new()),
            _ => (solidcraft_ui_text::format("joint.one_component", &[("bodies", &bodies.join(", "))]), bodies),
        });
    }
    commands(app, k, inputs).err().map(|e| (e, Vec::new()))
}

/// The occurrences of picked bodies (deduplicated, in pick order).
fn occurrences(s: &Session, items: &[Sel]) -> Vec<u64> {
    let mut out = Vec::new();
    for x in items {
        if let Sel::Body { name } = x
            && let Some(o) = occurrence_of_body(s, name)
            && !out.contains(&o)
        {
            out.push(o);
        }
    }
    out
}

/// The commands the dialog's OK runs.
pub fn commands(app: &SolveApp, k: &Asm, inputs: &[SelInput]) -> Result<Vec<(String, Value)>, String> {
    let s = &app.session;
    let snap = |i: usize, what: &str| -> Result<SnapPt, String> {
        inputs.get(i).and_then(|inp| inp.items.first().and_then(|x| snap_in(app, inp, x))).ok_or_else(|| format!("select {what} first"))
    };
    let eval = |e: &str, kind: ValueKind| -> Result<f64, String> {
        if e.trim().is_empty() { Ok(0.0) } else { crate::length_input::eval(&s.doc, e, kind).map_err(|e| e.to_string()) }
    };
    let cmd = |id: &str, p: Value| vec![(id.to_string(), p)];
    Ok(match k {
        Asm::FromBodies => {
            let bodies = sel_items(inputs, 0).body_names();
            if bodies.is_empty() {
                return Err(solidcraft_ui_text::tr("ui.components_from_bodies_required").into());
            }
            cmd("component.from_bodies", json!({ "bodies": bodies }))
        }
        Asm::NewComponent { name, activate } => {
            let bodies: Vec<String> = sel_items(inputs, 0).body_names();
            if bodies.is_empty() {
                let mut p = json!({ "activate": activate });
                if !name.trim().is_empty() {
                    p["name"] = json!(name.trim());
                }
                cmd("component.create", p)
            } else {
                cmd("component.from_bodies", json!({ "bodies": bodies }))
            }
        }
        Asm::Joint(f) => {
            let kind = TYPES.get(f.kind).copied().unwrap_or("rigid");
            let jk = f.joint_kind();
            let custom_name = f.name.trim();
            let limits: Vec<Value> =
                f.limits.iter().take(jk.dofs().len()).map(|(on, lo, hi)| if *on { json!([lo, hi]) } else { Value::Null }).collect();
            if let Some(id) = f.editing {
                let mut p = json!({"joint": id, "type": kind, "flip": f.flip, "offset": f.offset[2]});
                if !custom_name.is_empty() {
                    p["name"] = json!(custom_name);
                }
                // The angle only when the user changed it: a side moved without it turns B as joint.create would.
                if f.angle != f.angle_shown {
                    p["angle"] = json!(f.angle);
                }
                // A side picked again moves the joint's attachment there; one left as it was keeps its origin.
                for (i, key) in ["a", "b"].into_iter().enumerate() {
                    let picked = sel_items(inputs, i).first();
                    if picked != f.sides[i].as_ref() {
                        let what = if i == 0 { "component 1's joint origin" } else { "component 2's joint origin" };
                        p[key] = snap(i, what)?.param;
                    }
                }
                let mut out = cmd("joint.edit", p);
                for (i, l) in limits.iter().enumerate() {
                    let p = match l.as_array() {
                        Some(a) => json!({"joint": id, "index": i, "min": a.first(), "max": a.get(1)}),
                        None => json!({"joint": id, "index": i}),
                    };
                    out.push(("joint.limits".into(), p));
                }
                let saved = app.session.doc.assembly.joints.iter().find(|j| j.id == id).map(|j| j.dynamics.as_slice()).unwrap_or_default();
                for (i, row) in f.dynamics.iter().enumerate().take(jk.dofs().len()) {
                    if row.iter().all(|x| x.trim().is_empty()) && !saved.get(i).is_some_and(Option::is_some) {
                        continue;
                    }
                    let mut p = json!({"joint": id, "index": i});
                    for (key, text) in ["effort", "velocity", "damping", "friction"].into_iter().zip(row) {
                        if !text.trim().is_empty() {
                            let x: f64 = text.trim().parse().map_err(|_| format!("{}: {text}", crate::i18n::tr_label("Not a number")))?;
                            p[key] = json!(x);
                        }
                    }
                    out.push(("joint.dynamics".into(), p));
                }
                return Ok(out);
            }
            if f.as_built {
                let occs = occurrences(s, sel_items(inputs, 0));
                if occs.len() != 2 {
                    return Err("select two components".into());
                }
                let mut p = json!({"type": kind, "a": occs.first(), "b": occs.get(1), "limits": limits});
                if !custom_name.is_empty() {
                    p["name"] = json!(custom_name);
                }
                if let Some(x) = inputs.get(1).and_then(|inp| inp.items.first().and_then(|x| snap_in(app, inp, x))) {
                    p["at"] = x.param;
                }
                return Ok(cmd("joint.as_built", p));
            }
            let (a, b) = (snap(0, "component 1's joint origin")?, snap(1, "component 2's joint origin")?);
            let placed = |x: &SnapPt| (x.param.get("occurrence").cloned(), x.param.get("path").cloned());
            if a.param.get("occurrence").is_some() && placed(&a) == placed(&b) {
                return Err("the two origins are on the same component".into());
            }
            let (x, y) = (eval(&f.offset[0], ValueKind::Length)?, eval(&f.offset[1], ValueKind::Length)?);
            let a_param = if x.abs() > 1e-12 || y.abs() > 1e-12 { shifted(&a, x, y) } else { a.param };
            // Picked origins mate by default (the engine's default: B's z opposite A's, faces
            // meet face to face, a pin goes into its hole); the dialog's Flip is the command's
            // `flip`, which turns B the other way.
            let mut p = json!({"type": kind, "a": a_param, "b": b.param, "flip": f.flip, "offset": f.offset[2], "angle": f.angle, "limits": limits});
            if !custom_name.is_empty() {
                p["name"] = json!(custom_name);
            }
            cmd("joint.create", p)
        }
        Asm::JointOrigin { name, .. } => {
            let mut p = snap(0, "a snap")?.param;
            if !name.trim().is_empty() {
                p["name"] = json!(name.trim());
            }
            cmd("joint.origin", p)
        }
        Asm::RigidGroup => {
            let items = sel_items(inputs, 0);
            let occs = occurrences(s, items);
            if occs.len() < 2 {
                if occs.len() == 1 && items.len() > 1 && items.iter().all(|x| matches!(x, Sel::Body { .. })) {
                    return Err(solidcraft_ui_text::format(
                        "joint.rigid_group_one_component",
                        &[("count", &items.len().to_string()), ("command", solidcraft_ui_text::tr("cmd.component_from_bodies"))],
                    ));
                }
                return Err(solidcraft_ui_text::tr("joint.rigid_group_select_components").into());
            }
            cmd("joint.rigid_group", json!({ "occurrences": occs }))
        }
        Asm::Drive { joint, values, .. } => {
            let j = s.doc.assembly.joints.get(*joint).ok_or("no joint that moves yet")?;
            // A rigid joint has nothing to drive: the dialog says why and offers Edit Joint (#6981).
            if j.kind == JointKind::Rigid {
                return Err(solidcraft_ui_text::format("drive.rigid", &[("joint", &j.name)]));
            }
            cmd("joint.drive", json!({"joint": j.id, "values": values}))
        }
        Asm::MotionLink { a, b, ia, ib, ratio, offset, editing } => {
            let js = &s.doc.assembly.joints;
            let (Some(ja), Some(jb)) = (js.get(*a), js.get(*b)) else { return Err("needs two joints".into()) };
            // The offset as typed: the engine reads it in Joint 2's units (deg or mm), and the
            // ratio in Joint 2's units per Joint 1's.
            let ratio = eval(ratio, ValueKind::Unitless)?;
            let mut p = json!({"a": ja.id, "b": jb.id, "ia": ia, "ib": ib, "ratio": ratio, "offset": offset.trim()});
            if let Some(index) = editing {
                p["link"] = json!(index);
            }
            cmd("joint.motion_link", p)
        }
        Asm::Coupling(f) => return crate::dialogs_coupling::commands(app, f, inputs),
        Asm::Cam(f) => return crate::dialogs_coupling::cam::commands(app, f, inputs),
        Asm::Constrain(f) => return crate::dialogs_constrain::commands(app, f, inputs),
        // The check runs from its Compute button; OK only closes.
        Asm::Interference { .. } => Vec::new(),
        Asm::Trace(f) => cmd("motion.trace.keep", trace::params(s, f, inputs)?),
    })
}

/// The preview's commands: OK's, with the joint's values playing its motion while animating.
pub fn animate(app: &SolveApp, k: &Asm, cmds: &mut Vec<(String, Value)>) {
    if let Asm::Coupling(f) = k {
        crate::dialogs_coupling::animate(&app.session, f, cmds);
    }
    if let Asm::Cam(f) = k {
        crate::dialogs_coupling::cam::animate(&app.session, f, cmds);
    }
    let Asm::Joint(f) = k else { return };
    if !f.animate {
        return;
    }
    let kind = f.joint_kind();
    let tau = std::f64::consts::TAU;
    let wave = (f.phase * tau).sin();
    let values: Vec<String> = (0..kind.dofs().len())
        .map(|i| {
            let limited = f.limits.get(i).filter(|l| l.0);
            let angle = kind.is_angle(i);
            let vk = if angle { ValueKind::Angle } else { ValueKind::Length };
            match limited.and_then(|(_, lo, hi)| {
                Some((crate::length_input::eval(&app.session.doc, lo, vk).ok()?, crate::length_input::eval(&app.session.doc, hi, vk).ok()?))
            }) {
                // Back and forth between the limits.
                Some((lo, hi)) => {
                    let v = lo + (hi - lo) * (0.5 - 0.5 * (f.phase * tau).cos());
                    if angle { format!("{} deg", v.to_degrees()) } else { format!("{v} mm") }
                }
                None if angle && i == 0 => format!("{} deg", f.phase * 360.0),
                None if angle => format!("{} deg", 30.0 * wave),
                None => format!("{} mm", 20.0 * wave),
            }
        })
        .collect();
    let drive = match f.editing {
        Some(id) => Some(id),
        None => match cmds.iter_mut().find(|(c, _)| c == "joint.create") {
            Some((_, p)) => {
                p["values"] = json!(values);
                None
            }
            // As-built joints start at zero: drive the new one after it is made.
            None => app.session.doc.next_item_id().ok(),
        },
    };
    if let Some(id) = drive {
        cmds.push(("joint.drive".into(), json!({"joint": id, "values": values})));
    }
}

/// Joint and drive previews: the commands run on a scratch copy and the placed model before and
/// after is compared, so the components that move show where they go (`None`: not such
/// commands).
pub fn world_preview(s: &Session, cmds: &[(String, Value)], colors: Colors) -> Option<Result<Built, String>> {
    // Any feature, when components are placed away from their frames: the preview must show
    // the world the viewport shows.
    let moves = cmds.iter().all(|(c, _)| MOVES.contains(&c.as_str()));
    if cmds.is_empty() || !(moves || solidcraft_engine::frames::placed(s)) {
        return None;
    }
    let preview = match s.preview_visible_world(cmds) {
        Ok(preview) => preview,
        Err(error) => return Some(Err(error.to_string())),
    };
    let stopped = cmds
        .iter()
        .zip(&preview.replies)
        .find(|((c, _), r)| {
            c == "joint.drive" && (r["stopped_by_loop"].is_object() || r["stopped_by_contact"].is_object() || r["unconnected"].is_string())
        })
        .map(|(_, r)| r.clone());
    // A constraint's reply: how free its part is with the constraint the preview shows.
    let placed = cmds.iter().zip(&preview.replies).find(|((c, _), _)| c.starts_with("assembly.constraint.")).map(|(_, r)| r.clone());
    let (before, after) = (preview.before, preview.after);
    // A moved component looks like itself (no new surface), with its old place as the ghost.
    let colors = Colors { added: colors.body, new_body: colors.body, ..colors };
    let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crate::preview::build(&before, &after, colors)));
    Some(match built {
        Ok(mut b) => {
            if !colors.ghost {
                b.scene.ghost.clear();
            }
            b.bounds = after.bodies.iter().fold(Aabb3::EMPTY, |acc, x| acc.union(&x.mesh().bounds()));
            b.stopped = stopped;
            b.placed = placed;
            Ok(b)
        }
        Err(_) => Err("the preview failed".into()),
    })
}

/// What a drive that stopped short of the asked values says: where the joint got to and why
/// (`None`: it went all the way). `reply` is `joint.drive`'s result. A drive whose joint is
/// joined to nothing grounded moves nothing, and says which part that is.
pub fn drive_stop(reply: &Value) -> Option<String> {
    if let Some(part) = reply.get("unconnected").and_then(Value::as_str) {
        return Some(solidcraft_ui_text::format("drive.unconnected", &[("part", part)]));
    }
    // The values come as joint.list gives them: degrees and millimetres.
    let at: Vec<String> = reply
        .get("values")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| Some((v.get("value")?.as_f64()?, v.get("unit")?.as_str()?)))
        .map(|(v, unit)| if unit == "deg" { num((v * 10.0).round() / 10.0, "deg") } else { num((v * 100.0).round() / 100.0, "mm") })
        .collect();
    let at = at.join(", ");
    let name = |v: &Value| v.as_str().unwrap_or_default().to_string();
    if let Some(l) = reply.get("stopped_by_loop").filter(|v| v.is_object()) {
        return Some(solidcraft_ui_text::format("drive.stopped_by_loop", &[("value", &at), ("joint", &name(&l["joint"]))]));
    }
    let c = reply.get("stopped_by_contact").filter(|v| v.is_object())?;
    Some(solidcraft_ui_text::format("drive.stopped_by_contact", &[("value", &at), ("a", &name(&c["a"])), ("b", &name(&c["b"]))]))
}

// ---- overlay ----

/// Snap glyphs under the cursor and at the picked origins; the overlap of an interference.
pub fn overlay(app: &SolveApp, painter: &egui::Painter, proj: &Proj, d: &mut Dialog) {
    let t = Tokens::get();
    let Kind::Assembly(k) = &mut d.kind else { return };
    if let Asm::Interference { overlaps, .. } = k {
        // Front faces only, shaded by how they face the view.
        let back = proj.cam.back();
        for m in overlaps.iter() {
            let mut em = egui::Mesh::default();
            for tri in &m.triangles {
                let Some(ps) = m.tri(tri) else { continue };
                let Some(n) = (ps[1] - ps[0]).cross(ps[2] - ps[0]).normalized() else { continue };
                let facing = n.dot(back);
                if facing <= 0.0 {
                    continue;
                }
                let k = 0.55 + 0.45 * facing;
                let shade = |c: u8| (f64::from(c) * k) as u8;
                let ic = t.interference;
                let fill = Color32::from_rgba_unmultiplied(shade(ic.r()), shade(ic.g()), shade(ic.b()), 210);
                let sp: Vec<Pos2> = ps.iter().filter_map(|p| proj.to_screen(*p)).collect();
                if sp.len() == 3 {
                    let base = em.vertices.len() as u32;
                    for p in sp {
                        em.colored_vertex(p, fill);
                    }
                    em.add_triangle(base, base + 1, base + 2);
                }
            }
            painter.add(egui::Shape::mesh(em));
        }
        return;
    }
    if let Asm::Trace(f) = k {
        trace::overlay(painter, proj, f);
    }
    // A joint and a constraint name their two picks; a joint origin and a trace have one.
    let two = matches!(k, Asm::Joint(_) | Asm::Constrain(_));
    let hover = match k {
        Asm::Joint(f) => Some(&mut f.hover),
        Asm::JointOrigin { hover, .. } => Some(hover),
        Asm::Trace(f) => Some(&mut f.hover),
        Asm::Constrain(f) => Some(&mut f.hover),
        _ => None,
    };
    let Some(hover) = hover else { return };
    // The picked origins: Component 1 in the accent colour, Component 2 in orange.
    let picked: Vec<(SnapPt, Color32)> = d
        .inputs
        .iter()
        .enumerate()
        .filter(|(_, i)| i.accept == SNAPS)
        .filter_map(|(n, i)| i.items.first().and_then(|x| snap_in(app, i, x)).map(|s| (s, if n == 0 { t.accent } else { t.warning })))
        .collect();
    for (n, (s, c)) in picked.iter().enumerate() {
        let tag = format!("Component {}", n + 1);
        glyph(painter, proj, s, *c, app.cam.half_height(), two.then_some(tag.as_str()));
    }
    // The snap under the cursor (found again only when the hovered item changes).
    let accepting = d.inputs.get(d.active).is_some_and(|i| i.accept == SNAPS);
    let under = app.viewport.hover.as_ref().and_then(crate::viewport::hit_sel).filter(|_| accepting).map(|s| woken_pick(app, accepting, s));
    match under {
        Some(sel) => {
            let fresh = !matches!(hover, Some((s, _)) if *s == sel);
            if fresh {
                *hover = Some((sel.clone(), snap_hovered(app, &sel)));
            }
            if let Some((_, Some(s))) = hover.as_ref() {
                glyph(painter, proj, s, t.hover_profile_edge, app.cam.half_height(), Some(snap_tag(s)));
            }
        }
        None => *hover = None,
    }
}

/// A snap: the plane square to its z as a disc about the point, its kind's marker at the point and
/// the z axis as an arrow.
fn glyph(painter: &egui::Painter, proj: &Proj, s: &SnapPt, col: Color32, half_height: f64, tag: Option<&str>) {
    let Some(c) = proj.to_screen(s.at) else { return };
    let len = half_height * 2.0 * 0.06;
    plane_disc(painter, proj, s.at, s.z, len * 0.5, col);
    crate::icons::markers::snap(painter, c, proj.to_screen(s.at + s.z * len), s.kind, col, tag);
}

/// The plane a joint origin's z stands square to, as a circle of radius `r` about its point, so
/// the face it lies on reads at a glance however the arrow is foreshortened (#11476).
fn plane_disc(painter: &egui::Painter, proj: &Proj, at: Vec3, z: Vec3, r: f64, col: Color32) {
    const SIDES: usize = 48;
    let Some(z) = z.normalized() else { return };
    let (ax, ay) = (z.any_perp(), z.cross(z.any_perp()));
    let rim: Vec<Pos2> = (0..SIDES)
        .filter_map(|i| {
            let a = std::f64::consts::TAU * i as f64 / SIDES as f64;
            proj.to_screen(at + (ax * a.cos() + ay * a.sin()) * r)
        })
        .collect();
    if rim.len() < SIDES {
        return;
    }
    let halo = Tokens::get().icons.marker_halo;
    painter.add(egui::Shape::convex_polygon(rim.clone(), col.gamma_multiply(56.0 / 255.0), Stroke::NONE));
    painter.add(egui::Shape::closed_line(rim.clone(), Stroke::new(3.5, halo)));
    painter.add(egui::Shape::closed_line(rim, Stroke::new(1.5, col)));
}

/// What a snap is called (the hover label).
pub(crate) fn snap_name(k: SnapKind) -> &'static str {
    match k {
        SnapKind::FaceCenter => "Face center",
        SnapKind::CircleCenter => "Circle center",
        SnapKind::EdgeMid => "Edge midpoint",
        SnapKind::Vertex => "Vertex",
    }
}

// ---- timeline and browser ----

/// A joint's menu (timeline and browser): Edit, Drive, Suppress, Delete.
fn joint_menu(
    app: &SolveApp,
    ui: &mut egui::Ui,
    (id, kind, suppressed): (u64, JointKind, bool),
    open: &mut Option<Dialog>,
    run: &mut Option<(&'static str, Value)>,
) {
    if crate::ui_kit::button(ui, solidcraft_ui_text::tr_label("Edit Joint"), crate::ui_kit::Button::Normal).clicked() {
        *open = edit_joint(app, id);
        ui.close();
    }
    if kind != JointKind::Rigid && crate::ui_kit::button(ui, solidcraft_ui_text::tr_label("Drive Joint"), crate::ui_kit::Button::Normal).clicked() {
        *open = drive_joint(app, id);
        ui.close();
    }
    if crate::ui_kit::button(ui, solidcraft_ui_text::tr_label(if suppressed { "Unsuppress" } else { "Suppress" }), crate::ui_kit::Button::Normal)
        .clicked()
    {
        *run = Some(("joint.edit", json!({"joint": id, "suppressed": !suppressed})));
        ui.close();
    }
    ui.separator();
    if crate::ui_kit::button(ui, solidcraft_ui_text::tr_label("Delete"), crate::ui_kit::Button::Normal).clicked() {
        *run = Some(("selection.delete", json!({ "joints": [id] })));
        ui.close();
    }
}

/// Membership is indexed once so a collapsed folder with hundreds of joints stays cheap.
fn timeline_joint_items(app: &SolveApp, ids: Option<&[u64]>) -> Vec<(u64, String, JointKind, bool)> {
    let members: std::collections::BTreeSet<u64> = match ids {
        Some(ids) => ids.iter().copied().collect(),
        None => app.session.doc.timeline_groups.iter().flat_map(|group| group.joints.iter().copied()).collect(),
    };
    app.session
        .doc
        .assembly
        .joints
        .iter()
        .filter(|joint| members.contains(&joint.id) == ids.is_some())
        .map(|joint| (joint.id, joint.name.clone(), joint.kind, joint.suppressed))
        .collect()
}

/// More joints than this (an imported assembly makes one per placed part) start grouped in the
/// timeline as one button with their count, from the second joint on; clicking it shows each joint.
const JOINT_GROUP_MIN: usize = 1;
/// The browser-state key that remembers the timeline's joints were shown one by one.
const JOINTS_KEY: &str = "timeline_joints";
/// The History key that remembers a folder's joints were shown one by one, before its id.
pub(crate) const FOLDER_JOINTS_KEY: &str = "timeline_joints:";

/// Which joints a row of joint chips in the History bar shows: those no feature folder holds
/// (after the features), or a folder's (an import's rigid group, see `timeline_folders`).
#[derive(Clone, Copy)]
pub(crate) enum JointRun<'a> {
    Loose,
    Folder { id: u64, name: &'a str },
}

impl JointRun<'_> {
    /// The key that remembers the joints were shown one by one: in the browser's state for the
    /// joints after the history, in the design's History choices for a folder's.
    fn key(self) -> String {
        match self {
            Self::Loose => JOINTS_KEY.to_string(),
            Self::Folder { id, .. } => format!("{FOLDER_JOINTS_KEY}{id}"),
        }
    }

    /// The scenario handle of their grouped chip.
    fn handle(self) -> String {
        match self {
            Self::Loose => "timeline:joints".to_string(),
            Self::Folder { name, .. } => format!("timeline-folder-joints:{name}"),
        }
    }
}

/// Whether `count` joints group as one button: from the second joint, once the preference is on
/// (Preferences > Layout, "Group joints in the History"; off by default, which shows each joint).
fn grouped(app: &SolveApp, count: usize) -> bool {
    count > JOINT_GROUP_MIN && app.preferences.history_group_joints
}

fn joints_open(app: &SolveApp, count: usize, run: JointRun) -> bool {
    !grouped(app, count)
        || match run {
            JointRun::Loose => crate::browser::is_open(app, &run.key(), false),
            // Folder ids are numbered in each design: its choice stays with the design, as the
            // folder's own fold does.
            JointRun::Folder { .. } => crate::history_folds::is_open(app, &run.key(), false),
        }
}

fn toggle_joints(app: &mut SolveApp, run: JointRun) {
    match run {
        JointRun::Loose => crate::browser::toggle(app, &run.key(), false),
        JointRun::Folder { .. } => {
            let open = crate::history_folds::is_open(app, &run.key(), false);
            crate::history_folds::set_open(app, &run.key(), !open);
        }
    }
}

/// The button that stands for `count` grouped joints, its top left at `at` (see
/// `HistoryGroupChip`). Returns its width and whether it was clicked.
fn joint_group_button(
    ui: &mut egui::Ui,
    p: &egui::Painter,
    at: egui::Pos2,
    count: usize,
    open: bool,
    accent: solidcraft_theme::TabAccent,
    run: JointRun,
) -> (f32, bool) {
    let chip = crate::ui_kit::HistoryGroupChip {
        icon: "joint",
        name: "",
        count,
        open,
        selected: false,
        hovered: false,
        dim: false,
        error: false,
        warning: false,
        accent,
    };
    let br = egui::Rect::from_min_size(at, chip.size(p));
    let resp = ui.interact(br, ui.scope_id().with(("joint_group", run.key())), egui::Sense::click());
    let handle = run.handle();
    crate::scenario::publish_handle(&handle, br.center());
    crate::scenario::publish_target(&format!("{handle}_arrow"), crate::ui_kit::HistoryGroupChip::well(br));
    let pointer = resp.hover_pos();
    let on_arrow = crate::ui_kit::HistoryGroupChip::on_arrow(br, pointer);
    crate::scenario::publish_count(&format!("{handle}_arrow_hot"), f64::from(u8::from(on_arrow)));
    chip.paint(p, br, pointer);
    let tip = if open { "click to group them" } else { "click to show each one" };
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).backing_on_hover_text(format!("{count} joints: {tip}"));
    (br.width(), resp.clicked())
}

/// Joints after the features in the timeline, except those a feature folder holds (`held`):
/// double-click edits, the menu drives, suppresses or deletes. Returns the x after them.
pub fn timeline_joints(app: &mut SolveApp, ui: &mut egui::Ui, p: &egui::Painter, x: f32, r: egui::Rect, held: &HashSet<u64>) -> f32 {
    let loose: Vec<u64> = app.session.doc.assembly.joints.iter().map(|j| j.id).filter(|id| !held.contains(id)).collect();
    if loose.is_empty() {
        return x;
    }
    joint_chips(app, ui, p, x + 8.0, r, &loose, JointRun::Loose)
}

/// The joints `ids` as chips from `x` on, grouped into one chip (open to show each) when there
/// are many. Returns the x after them.
pub(crate) fn joint_chips(app: &mut SolveApp, ui: &mut egui::Ui, p: &egui::Painter, mut x: f32, r: egui::Rect, ids: &[u64], set: JointRun) -> f32 {
    let t = Tokens::get();
    let wanted: HashSet<u64> = ids.iter().copied().collect();
    // The design held, not its joints copied: this runs at every frame, for every joint.
    let doc = app.session.doc.clone();
    let joints: Vec<&solidcraft_engine::doc::joints::Joint> = doc.assembly.joints.iter().filter(|j| wanted.contains(&j.id)).collect();
    if joints.is_empty() {
        return x;
    }
    if grouped(app, joints.len()) {
        let open = joints_open(app, joints.len(), set);
        let accent = crate::workspace::tab_accent(&t, &app.ui.tab);
        let (width, clicked) = joint_group_button(ui, p, egui::pos2(x, r.top() + 6.0), joints.len(), open, accent, set);
        if clicked {
            toggle_joints(app, set);
        }
        x += width + 4.0;
        if !open {
            return x;
        }
    }
    let mut open: Option<Dialog> = None;
    let mut run: Option<(&'static str, Value)> = None;
    // Only the chips in the strip's view are made: a large assembly's hundreds of joints were
    // each hit-tested, painted and given a tooltip and a menu at every frame, the History bar's
    // scroll area showing a few dozen. A chip out of view keeps its place and its handle.
    let shown = ui.clip_rect();
    for j in joints {
        let br = egui::Rect::from_min_size(egui::pos2(x, r.top() + 6.0), vec2(28.0, 28.0));
        crate::scenario::publish_handle(&format!("timeline-joint:{}", j.name), br.center());
        if !br.intersects(shown) {
            x += 32.0;
            continue;
        }
        let (id, name, kind, suppressed) = (j.id, j.name.as_str(), j.kind, j.suppressed);
        let resp = ui.interact(br, ui.scope_id().with(("joint", id)), egui::Sense::click());
        let bg = if app.tree.picked_joints.contains(&id) {
            t.accent_soft
        } else if resp.hovered() {
            t.hover
        } else {
            t.timeline_item
        };
        p.rect(br, t.radius_sm.saturating_sub(1), bg, t.control_border_stroke(t.border), egui::StrokeKind::Inside);
        crate::icons::paint_icon(p, br.shrink(4.0), "joint", f32::from((if suppressed { t.border } else { t.icon }).a()) / 255.0);
        if suppressed {
            p.line_segment([br.left_bottom() + vec2(4.0, -4.0), br.right_top() + vec2(-4.0, 4.0)], Stroke::new(1.5, t.text_dim));
        }
        let resp = resp.backing_on_hover_text(format!("{name}  ({kind:?} joint)\nDouble-click to edit · right-click for more"));
        if resp.double_clicked() {
            open = edit_joint(app, id);
        } else if resp.clicked() {
            pick_joint(app, id, ui.input(|i| i.modifiers.command || i.modifiers.ctrl));
        }
        if resp.secondary_clicked() && !app.tree.picked_joints.contains(&id) {
            pick_joint(app, id, false);
        }
        crate::ui_kit::Menu::context(&resp, |ui| joint_menu(app, ui, (id, kind, suppressed), &mut open, &mut run));
        x += 32.0;
    }
    if let Some((c, prm)) = run {
        if c == "selection.delete" {
            crate::delete::request(app, prm);
        } else {
            let _ = app.run(c, prm);
        }
    }
    if let Some(d) = open {
        app.tool = None;
        app.dialog = Some(d);
    }
    x
}

/// Named joint rows for the vertical feature-history layout.
pub fn timeline_joints_vertical(app: &mut SolveApp, ui: &mut egui::Ui) {
    timeline_joint_rows(app, ui, None);
}

pub(crate) fn timeline_joint_rows(app: &mut SolveApp, ui: &mut egui::Ui, ids: Option<&[u64]>) {
    let t = Tokens::get();
    let joints = timeline_joint_items(app, ids);
    if joints.is_empty() {
        return;
    }
    if ids.is_none() {
        ui.add_space(4.0);
        if grouped(app, joints.len()) {
            let shown = joints_open(app, joints.len(), JointRun::Loose);
            let header = format!("JOINTS ({})", joints.len());
            let clicked = ui
                .horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(t.form_section_icon_size, t.form_section_icon_size), egui::Sense::hover());
                    let fold = crate::ui_kit::disclosure_button_at(ui, rect, shown, &header);
                    let response = ui.add(
                        egui::Label::new(egui::RichText::new(&header).size(solidcraft_theme::Tokens::get().font_micro).semibold().color(t.text_dim))
                            .sense(egui::Sense::click()),
                    );
                    crate::scenario::publish_handle("timeline:joints", response.rect.center());
                    fold.clicked() || response.clicked()
                })
                .inner;
            if clicked {
                toggle_joints(app, JointRun::Loose);
            }
            if !shown {
                return;
            }
        } else {
            ui.label(egui::RichText::new("JOINTS").size(solidcraft_theme::Tokens::get().font_micro).semibold().color(t.text_dim));
        }
    }
    let mut open = None;
    let mut run = None;
    for (id, name, kind, suppressed) in joints {
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), egui::Sense::click());
        crate::scenario::publish_handle(&format!("timeline-joint:{name}"), rect.center());
        // Keep the scroll extent and row IDs, but only lay out icons, text and menus for
        // joints in the clipped viewport (an imported assembly can have thousands).
        if !ui.is_rect_visible(rect) {
            continue;
        }
        if app.tree.picked_joints.contains(&id) {
            ui.painter().rect_filled(rect, t.radius_sm.saturating_sub(1), t.accent_soft);
        } else if response.hovered() {
            ui.painter().rect_filled(rect, t.radius_sm.saturating_sub(1), t.hover);
        }
        let icon = egui::Rect::from_min_size(egui::pos2(rect.left() + 4.0, rect.top() + 4.0), vec2(22.0, 22.0));
        crate::icons::paint_icon(ui.painter(), icon, "joint", f32::from((if suppressed { t.border } else { t.icon }).a()) / 255.0);
        if suppressed {
            ui.painter().line_segment([icon.left_bottom(), icon.right_top()], Stroke::new(1.5, t.text_dim));
        }
        ui.painter().text(
            egui::pos2(icon.right() + 7.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            &name,
            egui::FontId::proportional(solidcraft_theme::Tokens::get().font_body),
            if suppressed { t.text_dim } else { t.text },
        );
        let response = response.backing_on_hover_text(format!("{name}  ({kind:?} joint)\nDouble-click to edit · right-click for more"));
        if response.double_clicked() {
            open = edit_joint(app, id);
        } else if response.clicked() {
            pick_joint(app, id, ui.input(|i| i.modifiers.command || i.modifiers.ctrl));
        }
        if response.secondary_clicked() && !app.tree.picked_joints.contains(&id) {
            pick_joint(app, id, false);
        }
        crate::ui_kit::Menu::context(&response, |ui| joint_menu(app, ui, (id, kind, suppressed), &mut open, &mut run));
    }
    if let Some((command, params)) = run {
        if command == "selection.delete" {
            crate::delete::request(app, params);
        } else {
            let _ = app.run(command, params);
        }
    }
    if let Some(dialog) = open {
        app.tool = None;
        app.dialog = Some(dialog);
    }
}

/// The Joints folder at the end of the Browser: a row per joint (double-click edits, the menu
/// edits, drives, suppresses or deletes). Returns the joint whose row is under the pointer.
pub fn browser_joints(app: &mut SolveApp, rows: &mut crate::ui_kit::browser::Rows<'_>, depth: usize) -> Option<u64> {
    use crate::browser::{Row, Spec, is_open, node_row, toggle};
    let joints: Vec<(u64, String, JointKind, bool)> =
        app.session.doc.assembly.joints.iter().map(|j| (j.id, j.name.clone(), j.kind, j.suppressed)).collect();
    app.tree.picked_joints.retain(|p| joints.iter().any(|j| j.0 == *p));
    if joints.is_empty() {
        return None;
    }
    // Drawn like every other browser node (its fold, eye and rows publish `fold:Joints`,
    // `eye:Joints`, `row:<name>`). The folder's eye is its own: hiding it hides every joint's
    // glyph without changing each joint's eye.
    let key = "c0/joints";
    let start_open = crate::browser::rows_start_open(app);
    let open = is_open(app, key, start_open);
    let folder_shown = !app.session.folder_hidden(key);
    let r = node_row(
        rows,
        Spec { key, ..Default::default() },
        Row { depth, fold: Some(open), eye: Some(folder_shown), icon: "folder", label: "Joints", dim: !folder_shown, ..Default::default() },
    );
    if r.eye {
        crate::browser::toggle_folder(app, key);
    } else if r.fold || r.clicked {
        toggle(app, key, start_open);
    }
    if !open {
        return None;
    }
    let mut dialog: Option<Dialog> = None;
    let mut hovered = None;
    for (id, name, kind, suppressed) in joints {
        let visible = app.session.joint_visible(id);
        let selected = app.tree.picked_joints.contains(&id);
        let joint_key = format!("joint:{id}");
        let r = node_row(
            rows,
            Spec { key: &joint_key, ..Default::default() },
            Row {
                depth: depth + 1,
                eye: Some(visible),
                icon: "joint",
                label: &name,
                name: true,
                rename: true,
                selected,
                dim: suppressed || !visible || !folder_shown,
                ..Default::default()
            },
        );
        if let Some(resp) = &r.resp {
            resp.clone().backing_on_hover_text(format!("{kind:?} joint"));
        }
        if r.hovered {
            hovered = Some(id);
        }
        if r.eye {
            let _ = app.run("browser.visibility", json!({ "joints": [id], "visible": !visible }));
        // The slower second click on the only selected joint renames it and keeps it selected.
        } else if r.clicked && !(r.slow_rename && app.tree.picked_joints == [id]) {
            pick_joint(app, id, rows.ui().input(|i| i.modifiers.command || i.modifiers.ctrl));
        }
        if r.slow_rename {
            crate::browser::start_inline_rename(app, crate::context_menu::RenameWhat::Joint(id), r.name_at());
        } else if r.double {
            dialog = edit_joint(app, id);
        }
        crate::browser::inline_rename(app, rows.ui(), r.name_at(), &name);
        // The same menus as the rest of the browser.
        if let Some(at) = r.secondary {
            if !app.tree.picked_joints.contains(&id) {
                pick_joint(app, id, false);
            }
            crate::context_menu::open_for(app, at, crate::context_menu::Target::Joint { id });
        }
    }
    // Then a row per motion link (Motion Link, Gear, Rack and Pinion, Screw and Cam): the links
    // were only ever in `joint.list`, so they could not be seen, changed or deleted here (#6825).
    let links = link_rows(&app.session);
    let n = app.session.doc.assembly.links.len();
    app.tree.picked_links.retain(|i| *i < n);
    for (index, label, icon, tip) in links {
        let selected = app.tree.picked_links.contains(&index);
        let link_key = format!("link:{index}");
        let r = node_row(
            rows,
            Spec { key: &link_key, ..Default::default() },
            Row { depth: depth + 1, icon, label: &label, name: true, selected, dim: !folder_shown, ..Default::default() },
        );
        if let (Some(resp), Some(tip)) = (&r.resp, &tip) {
            resp.clone().backing_on_hover_text(tip.as_str());
        }
        if r.clicked {
            pick_link(app, index, rows.ui().input(|i| i.modifiers.command || i.modifiers.ctrl));
        }
        if r.double {
            dialog = edit_link(app, index);
        }
        if let Some(at) = r.secondary {
            if !app.tree.picked_links.contains(&index) {
                pick_link(app, index, false);
            }
            crate::context_menu::open_for(app, at, crate::context_menu::Target::Link { index });
        }
    }
    if let Some(d) = dialog {
        app.tool = None;
        app.dialog = Some(d);
    }
    hovered
}

/// A motion link's kind: the word its Browser row starts with and the icon it shows.
fn coupling_kind(c: Option<&solidcraft_engine::doc::joints::Coupling>) -> (&'static str, &'static str) {
    use solidcraft_engine::doc::joints::Coupling as C;
    match c {
        Some(C::Gear { .. }) => ("Gear", "gear"),
        Some(C::RackPinion { .. }) => ("Rack and Pinion", "rack_pinion"),
        Some(C::Screw { .. }) => ("Screw", "screw"),
        Some(C::Cam(_)) => ("Cam", "cam"),
        None => ("Motion Link", "motion_link"),
    }
}

/// The Browser's motion-link rows: the link's index, its label, its icon and what hovering its
/// row says.
fn link_rows(s: &Session) -> Vec<(usize, String, &'static str, Option<String>)> {
    let name = |id: u64| s.doc.assembly.joints.iter().find(|j| j.id == id).map(|j| j.name.clone()).unwrap_or_default();
    s.doc
        .assembly
        .links
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let (word, icon) = coupling_kind(l.coupling.as_ref());
            let label = format!("{}: {} → {}", solidcraft_ui_text::tr_label(word), name(l.a), name(l.b));
            (i, label, icon, link_tip(s, l))
        })
        .collect()
}

/// A link row's tooltip: a cam's travel over a turn of the cam, which its row else has no room
/// for (`joint.list` says the same). The other links say all they have to in their label.
fn link_tip(s: &Session, l: &solidcraft_engine::doc::joints::MotionLink) -> Option<String> {
    let joint = |id: u64| s.doc.assembly.joints.iter().find(|j| j.id == id);
    if !matches!(l.coupling, Some(solidcraft_engine::doc::joints::Coupling::Cam(_))) {
        return None;
    }
    let angle = joint(l.b).is_some_and(|j| j.kind.is_angle(l.ib));
    let unit = if angle { "deg" } else { "mm" };
    let show = |v: f64| if angle { v.to_degrees() } else { v };
    let (mut min, mut max) = (f64::NAN, f64::NAN);
    for v in &l.lift {
        if v.is_finite() {
            if !min.is_finite() || *v < min {
                min = *v;
            }
            if !max.is_finite() || *v > max {
                max = *v;
            }
        }
    }
    if !min.is_finite() || !max.is_finite() {
        return Some(solidcraft_ui_text::tr("browser.link.no_profile").to_string());
    }
    let (min, max) = (show(min), show(max));
    Some(format!(
        "{} {}…{} {unit} ({} {})",
        solidcraft_ui_text::tr("browser.link.lift"),
        crate::dialogs_coupling::trim(min),
        crate::dialogs_coupling::trim(max),
        solidcraft_ui_text::tr("browser.link.rise"),
        crate::dialogs_coupling::trim(max - min)
    ))
}

/// A click on a motion link's row: it becomes the pick (or, with Ctrl, toggles in or out of it),
/// and the two joints it ties draw highlighted with it (#6825).
pub fn pick_link(app: &mut SolveApp, index: usize, add: bool) {
    app.tree.picked_timeline_groups.clear();
    app.tree.timeline_anchor = None;
    if !add {
        app.tree.picked_joints.clear();
        app.tree.picked_canvases.clear();
        app.tree.picked_components.clear();
        app.tree.picked_links.clear();
        let _ = app.run("select.clear", json!({}));
    }
    if app.tree.picked_links.contains(&index) {
        app.tree.picked_links.retain(|l| *l != index);
        app.tree.picked_joints.clear();
    } else {
        app.tree.picked_links.push(index);
        app.tree.picked_links.sort_unstable();
        if let Some(l) = app.session.doc.assembly.links.get(index) {
            let (a, b) = (l.a, l.b);
            for id in [a, b] {
                if !app.tree.picked_joints.contains(&id) {
                    app.tree.picked_joints.push(id);
                }
            }
        }
    }
}

/// A click on a joint's browser row: it becomes the pick (or, with Ctrl, toggles in or out of
/// it). Picked joints draw highlighted in the viewport, even hidden ones.
pub fn pick_joint(app: &mut SolveApp, id: u64, add: bool) {
    app.tree.picked_timeline_groups.clear();
    app.tree.timeline_anchor = None;
    if !add {
        let was = app.tree.picked_joints == [id];
        app.tree.picked_joints.clear();
        app.tree.picked_canvases.clear();
        app.tree.picked_components.clear();
        let _ = app.run("select.clear", json!({}));
        if !was {
            app.tree.picked_joints.push(id);
        }
    } else if app.tree.picked_joints.contains(&id) {
        app.tree.picked_joints.retain(|j| *j != id);
    } else {
        app.tree.picked_joints.push(id);
    }
}

/// A placement's Joints folder (#9130): the design's joints that attach this placement (it is
/// one of the two they join), each beside the placement it joins this one to. The rows are the
/// same joints as the root Joints folder's and act the same: a click picks the joint, a
/// double-click edits it, the eye shows or hides its glyph and the menu is the joint's menu.
/// The folder starts closed and has no eye of its own (the root folder's eye hides every joint
/// glyph); it isn't drawn for a placement no joint attaches. `key` is the placement's row key.
/// Returns the joint whose row is under the pointer.
pub fn component_joints(app: &mut SolveApp, rows: &mut crate::ui_kit::browser::Rows<'_>, path: &[u64], key: &str, depth: usize) -> Option<u64> {
    use crate::browser::{Row, Spec, is_open, node_row, toggle};
    let joints = placement_joints(&app.session.doc, path);
    if joints.is_empty() {
        return None;
    }
    let key = format!("{key}/joints");
    let open = is_open(app, &key, false);
    let r = node_row(
        rows,
        Spec { key: &key, ..Default::default() },
        Row { depth, fold: Some(open), icon: "folder", label: "Joints", ..Default::default() },
    );
    if r.fold || r.clicked {
        toggle(app, &key, false);
    }
    if !open {
        return None;
    }
    // Hiding the root folder hides every glyph: these rows fade with it, as its own rows do.
    let folder_shown = !app.session.folder_hidden("c0/joints");
    let mut dialog: Option<Dialog> = None;
    let mut hovered = None;
    for j in joints.iter().cloned() {
        let id = j.id;
        let visible = app.session.joint_visible(id);
        let selected = app.tree.picked_joints.contains(&id);
        let joint_key = format!("{key}/joint:{id}");
        let r = node_row(
            rows,
            Spec { key: &joint_key, ..Default::default() },
            Row {
                depth: depth + 1,
                eye: Some(visible),
                icon: "joint",
                label: &j.name,
                secondary: &j.other,
                name: true,
                selected,
                dim: j.suppressed || !visible || !folder_shown,
                ..Default::default()
            },
        );
        if r.hovered {
            hovered = Some(id);
        }
        if let Some(resp) = &r.resp {
            resp.clone().backing_on_hover_text(format!("{:?} joint", j.kind));
        }
        if r.eye {
            let _ = app.run("browser.visibility", json!({ "joints": [id], "visible": !visible }));
        } else if r.clicked {
            pick_joint(app, id, rows.ui().input(|i| i.modifiers.command || i.modifiers.ctrl));
        }
        if r.double {
            dialog = edit_joint(app, id);
        }
        if let Some(at) = r.secondary {
            if !app.tree.picked_joints.contains(&id) {
                pick_joint(app, id, false);
            }
            crate::context_menu::open_for(app, at, crate::context_menu::Target::Joint { id });
        }
    }
    if let Some(d) = dialog {
        app.tool = None;
        app.dialog = Some(d);
    }
    hovered
}

/// A joint as a placement's Joints folder lists it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlacementJoint {
    pub id: u64,
    pub name: String,
    pub kind: JointKind,
    pub suppressed: bool,
    /// The name of the placement the joint joins this one to (empty when there is none).
    pub other: String,
}

/// The joints that attach the placement at `path` (its last occurrence is one of the two a
/// joint joins), in the design's joint order.
/// Made once per design version for every placement (see [`placement_joints`]): the Browser asks
/// for each expanded placement's joints at every frame, and each ask went through every joint of
/// the design, naming the other placement by a search of every occurrence.
pub(crate) fn placement_joints(doc: &std::sync::Arc<solidcraft_engine::doc::Document>, path: &[u64]) -> std::sync::Arc<[PlacementJoint]> {
    let Some(&occ) = path.last() else { return std::sync::Arc::from([]) };
    PLACEMENT_JOINTS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if !cache.as_ref().is_some_and(|(held, _)| std::sync::Arc::ptr_eq(held, doc)) {
            *cache = Some((doc.clone(), placement_joint_index(doc)));
        }
        cache.as_ref().and_then(|(_, by)| by.get(&occ).cloned()).unwrap_or_else(|| std::sync::Arc::from([]))
    })
}

thread_local! {
    /// [`placement_joint_index`] of the document it holds.
    #[allow(clippy::type_complexity)]
    static PLACEMENT_JOINTS: std::cell::RefCell<Option<(std::sync::Arc<solidcraft_engine::doc::Document>, std::collections::HashMap<u64, std::sync::Arc<[PlacementJoint]>>)>> =
        const { std::cell::RefCell::new(None) };
}

/// Each occurrence's joints, in the design's joint order, in one pass over the joints.
fn placement_joint_index(doc: &solidcraft_engine::doc::Document) -> std::collections::HashMap<u64, std::sync::Arc<[PlacementJoint]>> {
    let names: std::collections::HashMap<u64, &str> = doc.occurrences.iter().rev().map(|o| (o.id, o.name.as_str())).collect();
    let name_of = |o: u64| names.get(&o).map(|n| (*n).to_string()).unwrap_or_default();
    let mut by: std::collections::HashMap<u64, Vec<PlacementJoint>> = std::collections::HashMap::new();
    for j in &doc.assembly.joints {
        let joint = |other: u64| PlacementJoint { id: j.id, name: j.name.clone(), kind: j.kind, suppressed: j.suppressed, other: name_of(other) };
        by.entry(j.a.occurrence).or_default().push(joint(j.b.occurrence));
        if j.b.occurrence != j.a.occurrence {
            by.entry(j.b.occurrence).or_default().push(joint(j.a.occurrence));
        }
    }
    by.into_iter().map(|(o, list)| (o, list.into())).collect()
}

#[cfg(test)]
#[path = "dialogs_assembly_tests.rs"]
mod tests;

thread_local! {
    /// Named joint origins in the world, for the design state they were found in.
    static ORIGINS: std::cell::RefCell<Option<(String, Vec<(String, solidcraft_engine::doc::Mat)>)>> = const { std::cell::RefCell::new(None) };
}

/// Named joint origins in the world: on their occurrence's component, so they move with it
/// (joint drives, free moves not captured yet, Capture Position).
pub fn origins_world(s: &Session) -> Vec<(String, solidcraft_engine::doc::Mat)> {
    if s.doc.assembly.origins.is_empty() {
        return Vec::new();
    }
    let key = format!("{}:{:?}", s.revision, s.pending_moves);
    if let Some(v) = ORIGINS.with(|o| o.borrow().as_ref().filter(|(k, _)| *k == key).map(|(_, v)| v.clone())) {
        return v;
    }
    let doc: std::borrow::Cow<solidcraft_engine::doc::Document> = if s.pending_moves.is_empty() {
        std::borrow::Cow::Borrowed(&*s.doc)
    } else {
        let mut d = (*s.doc).clone();
        for o in &mut d.occurrences {
            if let Some(m) = s.pending_moves.get(&o.id) {
                o.transform = *m;
            }
        }
        std::borrow::Cow::Owned(d)
    };
    let st = s.model.state();
    let v: Vec<_> = doc.assembly.origins.iter().filter_map(|o| doc.origin_world(&st, &o.origin).ok().map(|m| (o.name.clone(), m))).collect();
    ORIGINS.with(|o| *o.borrow_mut() = Some((key, v.clone())));
    v
}

/// Is a named joint origin's triad drawn: its occurrence shown (with its parents), and the
/// Joints folder shown in the design and on that placement. Hidden hardware keeps no triad.
pub fn origin_drawn(s: &Session, name: &str) -> bool {
    let Some(o) = s.doc.assembly.origins.iter().find(|o| o.name == name) else { return false };
    if s.folder_hidden("c0/joints") {
        return false;
    }
    if o.origin.occurrence == 0 {
        return true;
    }
    let Some(occ) = s.doc.occurrences.iter().find(|x| x.id == o.origin.occurrence) else { return false };
    // The origin's own placement, not its component's first one (copies hide separately).
    let mut path = s.doc.first_path(occ.parent);
    path.push(occ.id);
    s.occurrence_visible(&path) && !s.folder_hidden(&format!("{}/joints", Session::folder_scope(&path)))
}

/// Named joint origins in the viewport: a small triad where each sits.
pub fn origins(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    let t = Tokens::get();
    let len = app.cam.half_height() * 2.0 * 0.04;
    for (name, m) in origins_world(&app.session) {
        if !origin_drawn(&app.session, &name) {
            continue;
        }
        let at = solidcraft_engine::doc::apply_point(&m, Vec3::ZERO);
        let Some(c) = proj.to_screen(at) else { continue };
        for (axis, col) in [(Vec3::X, t.axis_x), (Vec3::Y, t.axis_y), (Vec3::Z, t.axis_z)] {
            if let Some(tip) = proj.to_screen(at + solidcraft_engine::doc::apply_vector(&m, axis) * len) {
                painter.line_segment([c, tip], Stroke::new(2.0, col));
            }
        }
        painter.circle(c, 4.5, t.panel, Stroke::new(1.5, t.text));
    }
}

thread_local! {
    /// Joint frames in the world (joint id → A's frame), for the design state they were found in.
    static JOINT_FRAMES: std::cell::RefCell<Option<(String, Vec<(u64, solidcraft_engine::doc::Mat)>)>> = const { std::cell::RefCell::new(None) };
}

/// Each joint's frame in the world (where its glyph sits): the frame of its first origin, on
/// its occurrence as placed (including moves not captured yet).
pub fn joint_frames_world(s: &Session) -> Vec<(u64, solidcraft_engine::doc::Mat)> {
    if s.doc.assembly.joints.is_empty() {
        return Vec::new();
    }
    let key = format!("{}:{:?}", s.revision, s.pending_moves);
    if let Some(v) = JOINT_FRAMES.with(|o| o.borrow().as_ref().filter(|(k, _)| *k == key).map(|(_, v)| v.clone())) {
        return v;
    }
    let doc: std::borrow::Cow<solidcraft_engine::doc::Document> = if s.pending_moves.is_empty() {
        std::borrow::Cow::Borrowed(&*s.doc)
    } else {
        let mut d = (*s.doc).clone();
        for o in &mut d.occurrences {
            if let Some(m) = s.pending_moves.get(&o.id) {
                o.transform = *m;
            }
        }
        std::borrow::Cow::Owned(d)
    };
    let st = s.model.state();
    let v: Vec<_> = doc.assembly.joints.iter().filter_map(|j| doc.origin_world(&st, &j.a).ok().map(|m| (j.id, m))).collect();
    JOINT_FRAMES.with(|o| *o.borrow_mut() = Some((key, v.clone())));
    v
}

/// The joints whose glyph the viewport draws, and whether each is highlighted. A hidden joint
/// isn't drawn (it still constrains and moves the parts), except while it is picked in the
/// browser: then it shows highlighted until the pick goes away.
pub fn drawn_joints(app: &SolveApp) -> Vec<(u64, bool)> {
    // `Session::joint_drawn` for each joint, with the Joints folder looked up once: it was looked
    // up again for every joint of the design at every frame.
    let folder_shown = !app.session.folder_hidden("c0/joints");
    app.session
        .doc
        .assembly
        .joints
        .iter()
        .filter_map(|j| {
            let picked = app.tree.picked_joints.contains(&j.id);
            (picked || (folder_shown && app.session.joint_visible(j.id))).then_some((j.id, picked))
        })
        .collect()
}

thread_local! {
    /// The size (box diagonal) of each occurrence's bodies, for the design revision it was found in.
    static OCCURRENCE_SIZES: std::cell::RefCell<Option<(u64, std::collections::HashMap<u64, f64>)>> = const { std::cell::RefCell::new(None) };
}

/// The size (box diagonal, mm) of each occurrence's bodies, including those of the occurrences
/// inside it; occurrence 0 is the root's own bodies. Found once per design revision, in one
/// pass over the bodies, as [`occurrence_bounds`] would find it for each occurrence.
fn occurrence_sizes(s: &Session) -> std::collections::HashMap<u64, f64> {
    let rev = s.revision;
    if let Some(v) = OCCURRENCE_SIZES.with(|o| o.borrow().as_ref().filter(|(k, _)| *k == rev).map(|(_, v)| v.clone())) {
        return v;
    }
    let model = s.model.state();
    let Ok(world) = s.world_state() else { return std::collections::HashMap::new() };
    let mut boxes: std::collections::HashMap<u64, Aabb3> = std::collections::HashMap::new();
    for (name, _, path, _) in s.doc.body_frames(&model) {
        let Some(b) = world.body(&name) else { continue };
        let bb = b.mesh().bounds();
        let owners: Vec<u64> = if path.is_empty() { vec![0] } else { path.clone() };
        for o in owners {
            let e = boxes.entry(o).or_default();
            *e = e.union(&bb);
        }
    }
    let v: std::collections::HashMap<u64, f64> = boxes.into_iter().filter(|(_, b)| !b.is_empty()).map(|(o, b)| (o, b.diagonal())).collect();
    OCCURRENCE_SIZES.with(|o| *o.borrow_mut() = Some((rev, v.clone())));
    v
}

/// A joint glyph's axis half-length is at most this share of the view height ...
const GLYPH_VIEW_SHARE: f64 = 0.05;
/// ... and at most this share of the smaller joined part's size, so zoomed out the glyphs
/// shrink with the parts they join instead of covering them (#6404).
const GLYPH_PART_SHARE: f64 = 0.25;
/// The half size (screen points) of a rigid joint's square mark.
const RIGID_MARK: f32 = 3.5;

/// How one joint's glyph is drawn.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct JointGlyph {
    pub id: u64,
    pub picked: bool,
    /// A rigid joint has no motion: a small square, no axis or ring.
    pub rigid: bool,
    /// Half the motion axis (world mm); the ring's radius is half of it.
    pub len: f64,
    /// Lies on a glyph drawn before it (zoomed out, joints piled up): drawn faint.
    pub faded: bool,
    pub centre: Pos2,
}

/// The glyphs to draw, in drawing order: picked joints first, then moving joints, then rigid
/// ones. Each is sized to the view and to the parts it joins; one whose centre falls inside a
/// glyph planned before it is faded, unless it is picked. A badge style
/// (`joint_symbols`) reaches as far as its badge, the same wherever the joint sits.
pub(crate) fn joint_glyph_plan(app: &SolveApp, proj: &Proj) -> Vec<JointGlyph> {
    let drawn = drawn_joints(app);
    if drawn.is_empty() {
        return Vec::new();
    }
    let frames = joint_frames_world(&app.session);
    let sizes = occurrence_sizes(&app.session);
    let view_len = app.cam.half_height() * 2.0 * GLYPH_VIEW_SHARE;
    let badge = crate::joint_symbols::badge_reach(app.preferences.joint_symbols);
    let at = |m: &solidcraft_engine::doc::Mat, x: f64, y: f64, z: f64| solidcraft_engine::doc::apply_point(m, Vec3::new(x, y, z));
    let mut plan = Vec::new();
    for (id, picked) in drawn {
        let Some(j) = app.session.doc.assembly.joints.iter().find(|j| j.id == id) else { continue };
        let Some((_, m)) = frames.iter().find(|(f, _)| *f == id) else { continue };
        let Some(centre) = proj.to_screen(at(m, 0.0, 0.0, 0.0)) else { continue };
        let part = [j.a.occurrence, j.b.occurrence]
            .iter()
            .filter_map(|o| sizes.get(o).copied())
            .filter(|d| d.is_finite() && *d > 0.0)
            .fold(f64::INFINITY, f64::min);
        let len = view_len.min(part * GLYPH_PART_SHARE);
        plan.push((JointGlyph { id, picked, rigid: j.kind == JointKind::Rigid, len, faded: false, centre }, *m));
    }
    plan.sort_by_key(|(g, _)| (!g.picked, g.rigid));
    // Screen reach of each glyph: its badge, or its ring (or a rigid mark), for the overlap test.
    let reach = |g: &JointGlyph, m: &solidcraft_engine::doc::Mat| -> f32 {
        if let Some(radius) = badge {
            return radius;
        }
        if g.rigid {
            return RIGID_MARK;
        }
        let r = g.len * 0.5;
        [(r, 0.0, 0.0), (0.0, r, 0.0)]
            .iter()
            .filter_map(|p| proj.to_screen(at(m, p.0, p.1, p.2)))
            .map(|q| q.distance(g.centre))
            .fold(RIGID_MARK, f32::max)
    };
    let mut placed: Vec<(Pos2, f32)> = Vec::new();
    for (g, m) in &mut plan {
        let r = reach(g, m);
        g.faded = !g.picked && placed.iter().any(|(c, rc)| c.distance(g.centre) < rc.max(r));
        if !g.faded {
            placed.push((g.centre, r));
        }
    }
    plan.into_iter().map(|(g, _)| g).collect()
}

/// How near (screen points) the pointer must come to a glyph's centre for its tooltip.
const GLYPH_TIP_REACH: f32 = 8.0;

/// The joint whose glyph the pointer at `pos` is on: the nearest centre within reach, and of
/// glyphs on the same spot the one drawn on top (#6404).
pub(crate) fn joint_glyph_at(plan: &[JointGlyph], pos: Pos2) -> Option<u64> {
    plan.iter().map(|g| (g.id, g.centre.distance(pos))).filter(|(_, d)| *d <= GLYPH_TIP_REACH).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(id, _)| id)
}

/// A glyph's tooltip: the joint's name and type, e.g. "Glue (Rigid)", or the name alone when it says the type ("Rigid 7"), so the square, ring and
/// axis explain themselves (#6404).
pub(crate) fn joint_glyph_tip(app: &SolveApp, id: u64) -> Option<String> {
    let j = app.session.doc.assembly.joints.iter().find(|j| j.id == id)?;
    let kind = app.tr(kind_label(j.kind));
    // A name that already says the type ("Rigid 7") is enough on its own.
    Some(if j.name.to_lowercase().contains(&kind.to_lowercase()) { j.name.clone() } else { format!("{} ({kind})", j.name) })
}

/// Joint glyphs in the viewport, in the style Preferences picks (`joint_symbols`). The default,
/// [`RING_AND_AXIS`], draws a ring where a moving joint sits, its motion axis and its limits
/// (ticks on the ring for a turn, along the axis for a slide), and a small square for a rigid
/// joint ([`ring_shapes`]). The other styles draw a round badge with the pictogram of the joint's
/// type, the same size wherever it sits ([`crate::joint_symbols::badge_shapes`]): joints that
/// don't let their parts move draw quiet, a joint with a limit has a mark on its rim, and a picked
/// joint's badge is larger and filled.
/// Glyphs are never picked in the viewport; hovering one names the joint and its type.
pub fn joint_glyphs(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    let plan = joint_glyph_plan(app, proj);
    if plan.is_empty() {
        return;
    }
    let ctx = painter.ctx();
    let pointer = ctx
        .input(|i| if i.pointer.any_down() { None } else { i.pointer.hover_pos() })
        .filter(|p| painter.clip_rect().contains(*p) && !crate::shell::covered(app, *p));
    if let Some(p) = pointer
        && let Some(id) = joint_glyph_at(&plan, p)
        && let Some(tip) = joint_glyph_tip(app, id)
    {
        egui::containers::Tooltip::always_open(
            ctx.clone(),
            painter.layer_id(),
            egui::Id::unique(("joint_glyph_tip", id)),
            egui::PopupAnchor::Pointer,
        )
        .show(|ui| ui.label(tip));
    }
    let t = Tokens::get();
    let style = app.preferences.joint_symbols;
    let rings = style == RING_AND_AXIS;
    let bright = (app.preferences.highlight_moving_joints && !rings).then(|| crate::joint_symbols::bright(&app.session));
    let frames = joint_frames_world(&app.session);
    // Back to front, so the glyphs drawn first in the plan end up on top.
    for g in plan.iter().rev() {
        let Some(j) = app.session.doc.assembly.joints.iter().find(|j| j.id == g.id) else { continue };
        let Some((_, m)) = frames.iter().find(|(f, _)| *f == g.id) else { continue };
        crate::scenario::publish_handle(&format!("joint-glyph:{}", j.name), g.centre);
        let (stroke, rim) = glyph_strokes(app, g.id, g.picked, j.suppressed);
        let screen = |x: f64, y: f64, z: f64| proj.to_screen(solidcraft_engine::doc::apply_point(m, Vec3::new(x, y, z)));
        if rings {
            painter.extend(ring_shapes(&screen, j.kind, &j.limits, g, stroke, rim, &t));
            continue;
        }
        use crate::joint_symbols::Mood;
        // A joint lit by its pick or by its hovered browser row draws as a picked one does, in
        // the colour its core stroke has.
        let mood = if rim.is_some() {
            Mood::Picked
        } else if j.suppressed {
            Mood::Suppressed
        } else if bright.as_ref().is_some_and(|joints| !joints.contains(&g.id)) {
            Mood::Quiet
        } else {
            Mood::Bright
        };
        // x runs along the joint's travel as it looks from here.
        let travel = crate::joint_symbols::along(j.kind).and_then(|d| screen(d[0] * g.len, d[1] * g.len, d[2] * g.len)).map(|tip| tip - g.centre);
        let x = travel.filter(|d| d.length() > 4.0).map_or(vec2(1.0, 0.0), |d| if d.x < 0.0 { -d.normalized() } else { d.normalized() });
        let look = crate::joint_symbols::badge(style, j.kind, mood, stroke.color, &t);
        painter.extend(crate::joint_symbols::badge_shapes(g.centre, x, j.kind, &look, crate::joint_symbols::has_limit(j), g.faded, &t));
    }
}

/// A joint glyph's core stroke and, while it is picked or its browser row is hovered, the
/// lighter rim under it: the colours and widths a selected or hovered body's edges use.
pub(crate) fn glyph_strokes(app: &SolveApp, id: u64, picked: bool, suppressed: bool) -> (Stroke, Option<Stroke>) {
    lit_strokes(&Tokens::get(), picked, app.viewport.hover_joint == Some(id), suppressed)
}

/// [`glyph_strokes`] from a joint's state alone: picked, it takes the selection's core and rim,
/// hovered or not; hovered, the hovered edge's core and halo; unlit, a plain accent stroke, dim
/// while the joint is suppressed, and no rim.
pub(crate) fn lit_strokes(t: &Tokens, picked: bool, hovered: bool, suppressed: bool) -> (Stroke, Option<Stroke>) {
    let lit = if picked {
        Some((t.sel_edge, t.sel_edge_rim))
    } else if hovered {
        Some((t.hover_edge, t.hover_edge_halo))
    } else {
        None
    };
    match lit {
        Some((core, rim)) => (Stroke::new(2.5, core), Some(Stroke::new(2.5 + 3.0, rim))),
        None => (Stroke::new(1.5, if suppressed { t.text_dim } else { t.accent }), None),
    }
}

/// The `joint_symbols` style that draws a ring and axis in the model, as before badges; the
/// default.
pub const RING_AND_AXIS: u8 = crate::joint_symbols::RING_AND_AXIS;

/// The [`RING_AND_AXIS`] glyph of a joint of `kind` with these `limits`, `screen` placing a point
/// of the joint's frame: a ring where a moving joint sits, its motion axis and its limits (ticks
/// across the ring for a turn, across the axis or slot for a slide), or a small square for a rigid
/// joint, faint where the plan says it lies on another. `stroke` is its core ([`glyph_strokes`]);
/// `rim`, while the joint is lit, is drawn under the core.
pub(crate) fn ring_shapes(
    screen: &impl Fn(f64, f64, f64) -> Option<Pos2>,
    kind: JointKind,
    limits: &[Option<(f64, f64)>],
    g: &JointGlyph,
    stroke: Stroke,
    rim: Option<Stroke>,
    t: &Tokens,
) -> Vec<egui::Shape> {
    let mut out = Vec::new();
    // A glyph piled on another is drawn faint.
    let fade = |s: Stroke| if g.faded { Stroke::new(s.width, s.color.gamma_multiply(0.3)) } else { s };
    let (stroke, rim) = (fade(stroke), rim.map(fade));
    let width = stroke.width;
    let c = g.centre;
    let fill = if g.faded { t.panel.gamma_multiply(0.3) } else { t.panel };
    if g.rigid {
        let half = if g.picked { RIGID_MARK + 1.0 } else { RIGID_MARK };
        let square = egui::Rect::from_center_size(c, vec2(half * 2.0, half * 2.0));
        if let Some(rim) = rim {
            out.push(egui::Shape::rect_stroke(square, 0.0, rim, egui::StrokeKind::Middle));
        }
        out.push(egui::Shape::Rect(egui::epaint::RectShape::new(square, 0.0, fill, stroke, egui::StrokeKind::Middle)));
        return out;
    }
    let len = g.len;
    let r = len * 0.5;
    // The motion axis (z), both ways, and a ring about it: the rim first, then the core.
    let axis = match (screen(0.0, 0.0, -len), screen(0.0, 0.0, len)) {
        (Some(a), Some(b)) => Some([a, b]),
        _ => None,
    };
    for s in rim.into_iter().chain([stroke]) {
        if let Some(axis) = axis {
            out.push(egui::Shape::line_segment(axis, s));
        }
        out.extend(motion_ring(screen, r, s));
    }
    // Limits: ticks across the ring for a turn, across the axis (or slot) for a slide.
    let warning = if g.faded { t.warning.gamma_multiply(0.3) } else { t.warning };
    for (i, lim) in limits.iter().enumerate() {
        let Some((lo, hi)) = lim else { continue };
        let name = kind.dofs().get(i).copied().unwrap_or("");
        for v in [*lo, *hi] {
            if !v.is_finite() {
                continue;
            }
            let (p, q) = match name {
                "angle" => ((v.cos() * r * 0.7, v.sin() * r * 0.7, 0.0), (v.cos() * r * 1.3, v.sin() * r * 1.3, 0.0)),
                "distance" if v.abs() < 1e6 => ((-r * 0.3, 0.0, v), (r * 0.3, 0.0, v)),
                "slide" if v.abs() < 1e6 => ((v, -r * 0.3, 0.0), (v, r * 0.3, 0.0)),
                _ => continue,
            };
            if let (Some(p), Some(q)) = (screen(p.0, p.1, p.2), screen(q.0, q.1, q.2)) {
                out.push(egui::Shape::line_segment([p, q], Stroke::new(width, warning)));
            }
        }
    }
    if let Some(rim) = rim {
        out.push(egui::Shape::circle_stroke(c, 4.5, rim));
    }
    out.push(egui::Shape::Circle(egui::epaint::CircleShape { center: c, radius: if rim.is_some() { 4.5 } else { 3.5 }, fill, stroke }));
    out
}

/// A ring of radius `r` about a joint's axis.
fn motion_ring(screen: &impl Fn(f64, f64, f64) -> Option<Pos2>, r: f64, stroke: Stroke) -> Option<egui::Shape> {
    let ring: Vec<Pos2> = (0..=32)
        .filter_map(|k| {
            let a = k as f64 / 32.0 * std::f64::consts::TAU;
            screen(r * a.cos(), r * a.sin(), 0.0)
        })
        .collect();
    (ring.len() > 1).then(|| egui::Shape::line(ring, stroke))
}

#[cfg(test)]
#[path = "joint_glyph_tests.rs"]
mod joint_glyph_tests;

#[cfg(test)]
#[path = "joint_browser_tests.rs"]
mod joint_browser_tests;
