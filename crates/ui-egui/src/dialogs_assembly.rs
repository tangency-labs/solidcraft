//! Assembly dialogs: New Component, Joint, As-built Joint, Joint Origin, Rigid Group, Drive
//! Joints, Motion Link, Gear, Rack and Pinion, Screw, Cam (`dialogs_coupling`) and Interference. Joint origins are picked by hovering geometry: a face
//! snaps to its centre (or a round face to the centre of its nearer end), an edge to its middle
//! (a round edge to its centre), a vertex to itself; the snap shows as a glyph with its z axis
//! before the click. Joints and drives preview on the placed (world) model: the moving
//! component shows where it goes, and with Animate the joint plays its motion.

use std::collections::HashSet;

use crate::fonts::RichTextExt as _;
use crate::ui_kit::{choice_row, expr_row, text_row};
use egui::{Color32, Pos2, RichText, Stroke, vec2};
use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::Session;
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::doc::joints::{JointKind, JointOrigin, Snap};
use solidcraft_engine::geom::{Aabb3, Mesh, Vec3};

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind, combo, field, form_row};
use crate::preview::{Built, Colors};
use crate::selection::{BODIES, EDGES, FACES, SelInput, SelectionExt as _, VERTICES, items as sel_items, point_value as pt, polyline_mid};
use crate::theme::Tokens;

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
    format!("{name} ({})", crate::i18n::tr_label(kind_label(kind)))
}

/// What a joint origin input accepts (snaps on faces, edges and vertices).
pub const SNAPS: u16 = FACES | EDGES | VERTICES;
/// Commands whose effect is where occurrences sit (previewed on the placed model).
const MOVES: &[&str] = &[
    "joint.create",
    "joint.as_built",
    "joint.drive",
    "joint.edit",
    "joint.limits",
    "joint.rigid_group",
    "joint.gear",
    "joint.rack_pinion",
    "joint.screw",
    "joint.cam",
    "occurrence.align",
    "parts.insert",
    "parts.fastener",
];

#[derive(Clone, Debug)]
pub enum Asm {
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
    },
    Coupling(Box<crate::dialogs_coupling::CouplingForm>),
    Cam(Box<crate::dialogs_coupling::cam::CamForm>),
    Interference {
        result: Option<Value>,
        overlaps: Vec<Mesh>,
        of: Vec<Sel>,
        /// Pairs closer than this (mm, as typed) are listed too.
        clearance: String,
    },
    /// Motion Trace: a picked point's path, previewed; OK keeps it as a 3D sketch spline.
    Trace(Box<TraceForm>),
}

/// The snap under the cursor, remembered for the selection it was found for.
pub type Hover = Option<(Sel, Option<SnapPt>)>;

/// The Joint and As-built Joint dialogs.
#[derive(Clone, Debug)]
pub struct JointForm {
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
    pub animate: bool,
    /// Animation phase, 0…1.
    pub phase: f64,
    pub hover: Hover,
}

impl JointForm {
    fn new(as_built: bool, doc: &solidcraft_engine::doc::Document) -> JointForm {
        JointForm {
            as_built,
            editing: None,
            kind: 0,
            sides: [None, None],
            flip: false,
            angle: "0 deg".into(),
            angle_shown: "0 deg".into(),
            offset: std::array::from_fn(|_| crate::length_input::default(doc, "0")),
            limits: Vec::new(),
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
    }
}

impl Asm {
    pub fn title(&self) -> &'static str {
        match self {
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
            Asm::Interference { .. } => "INTERFERENCE",
            Asm::Trace(_) => "MOTION TRACE",
        }
    }

    pub fn previews(&self) -> bool {
        matches!(self, Asm::Joint(_) | Asm::Drive { .. } | Asm::RigidGroup | Asm::Coupling(_) | Asm::Cam(_))
    }
}

/// The dialog for an assembly command: its state and selection inputs.
pub fn start(app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    let s = &app.session;
    let (asm, inputs) = match id {
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
            (Asm::MotionLink { a: 0, b, ia: 0, ib: 0, ratio: "1".into(), offset: "0".into() }, vec![])
        }
        "joint.gear" | "joint.rack_pinion" | "joint.screw" => crate::dialogs_coupling::start(app, id)?,
        "joint.cam" => crate::dialogs_coupling::cam::start(app),
        "inspect.interference" => (
            Asm::Interference { result: None, overlaps: Vec::new(), of: Vec::new(), clearance: "0".into() },
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

/// The pick a joint side shows in the side inputs: the face or edge of its own occurrence that
/// the side's snap names (the piece nearest its pick, when the name has several), else the face
/// or edge nearest the pick. A point or frame side names no face: None.
fn side_pick(app: &SolveApp, o: &JointOrigin) -> Option<Sel> {
    let s = &app.session;
    let (pick, face, edge) = match &o.snap {
        Snap::FaceCenter { pick, face, .. } | Snap::CircleCenter { pick, face, .. } => (*pick, face.as_deref(), None),
        Snap::Edge { pick, edge, .. } => (*pick, None, edge.as_deref()),
        Snap::Point { .. } | Snap::Frame { .. } => return None,
    };
    // The pick is in the component's coordinates; the placed bodies are in the world.
    let at =
        if o.occurrence == 0 { solidcraft_engine::doc::IDENTITY } else { solidcraft_engine::doc::joints::occurrence_world(&s.doc, o.occurrence) };
    let p = solidcraft_engine::doc::apply_point(&at, pick);
    let st = s.world_state();
    let own: Vec<&solidcraft_engine::doc::ModelBody> = st.bodies.iter().filter(|b| occurrence_of_body(s, &b.name) == Some(o.occurrence)).collect();
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
            let (at, _) = solidcraft_engine::doc::naming::find_edge(b, name, p).ok()?;
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

/// The Edit Joint dialog for a joint (by id). Its two sides show as the Joint dialog's do, each
/// pre-filled with the pick it sits on.
pub fn edit_joint(app: &SolveApp, id: u64) -> Option<Dialog> {
    let j = app.session.doc.assembly.joints.iter().find(|j| j.id == id)?;
    let mut f = JointForm::new(false, &app.session.doc);
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
    Some(Dialog::new(Kind::Assembly(Asm::Joint(Box::new(f))), inputs))
}

/// The Drive Joints dialog on a joint (by id).
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
    cam.fit(&bb);
    app.animate_to(cam);
}

/// The box around the bodies of occurrences (and of the occurrences inside them), where they
/// are placed; occurrence 0 is the root, holding the bodies outside every occurrence.
pub fn occurrence_bounds(s: &Session, occs: &[u64]) -> Aabb3 {
    let model = s.model.state();
    let world = s.world_state();
    let mut bb = Aabb3::default();
    for (name, _, path, _) in s.doc.body_frames(&model) {
        let on = occs.iter().any(|o| if *o == 0 { path.is_empty() } else { path.contains(o) });
        if on && let Some(b) = world.body(&name) {
            bb = bb.union(&b.mesh().bounds());
        }
    }
    bb
}

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
    // The world name tells the instance: a further occurrence of a component is not its first.
    let st = s.world_state();
    let b = st.body(body)?;
    if s.doc.model_body_component(b) == 0 {
        return Some(0);
    }
    let model = s.model.state();
    let (_, _, _, path, _) = s.doc.body_frames_with_source(&model).into_iter().find(|(world, ..)| world == body)?;
    path.last().copied()
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
fn face_normal_near(m: &Mesh, f: u32, p: Vec3) -> Option<Vec3> {
    m.triangles
        .iter()
        .zip(&m.tri_face)
        .filter(|(_, tf)| **tf == f)
        .filter_map(|(t, _)| m.tri(t))
        .min_by(|a, b| ((a[0] + a[1] + a[2]) / 3.0).dist(p).total_cmp(&((b[0] + b[1] + b[2]) / 3.0).dist(p)))
        .and_then(|[a, b, c]| (b - a).cross(c - a).normalized())
}

/// The snap a picked item stands for (world coordinates, as the viewport shows them).
pub fn snap_of(s: &Session, sel: &Sel) -> Option<SnapPt> {
    let st = s.world_state();
    let (body, mut snap) = match sel {
        Sel::Face { body, index, point } => {
            let b = st.body(body)?;
            let tol = (b.body.size() * 2e-3).max(1e-3);
            let f = b.body.faces(tol).ok()?.into_iter().find(|f| f.index == *index)?;
            match f.plane_normal {
                Some(n) => (body, SnapPt { at: f.centroid, z: n, kind: SnapKind::FaceCenter, param: json!({"face": pt(*point)}) }),
                None => {
                    let c = solidcraft_engine::kernel::cylinder_face_at(&b.body, *point)?;
                    let sv = (*point - c.axis_point).dot(c.axis);
                    let (at, dir) = if (sv - c.start).abs() <= (c.end - sv).abs() { (c.start, -1.0) } else { (c.end, 1.0) };
                    (
                        body,
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
                    (body, SnapPt { at: c, z, kind: SnapKind::CircleCenter, param: json!({"circle": pt(c), "z": pt(z)}) })
                }
                None => {
                    let mid = polyline_mid(e);
                    let z = straight_edge_axis(e)
                        .or_else(|| m.edge_faces.get(*index).and_then(|fs| fs.first()).and_then(|f| face_normal_near(&m, *f, mid)))
                        .unwrap_or(Vec3::Z);
                    // Its middle, kept by the edge's name.
                    (body, SnapPt { at: mid, z, kind: SnapKind::EdgeMid, param: json!({"edge": pt(mid), "z": pt(z)}) })
                }
            }
        }
        Sel::Vertex { body, point } => {
            // The corner of the edges meeting there, kept by their names, z along the part's +Z
            // as a point's is.
            let z = occurrence_of_body(s, body)
                .map_or(Vec3::Z, |o| solidcraft_engine::doc::apply_vector(&solidcraft_engine::doc::joints::occurrence_world(&s.doc, o), Vec3::Z));
            (body, SnapPt { at: *point, z: Vec3::Z, kind: SnapKind::Vertex, param: json!({"edge": pt(*point), "corner": pt(*point), "z": pt(z)}) })
        }
        _ => return None,
    };
    if let (Some(occ), Value::Object(m)) = (occurrence_of_body(s, body), &mut snap.param) {
        m.insert("occurrence".into(), json!(occ));
    }
    Some(snap)
}

/// A snap moved across its own plane by (x, y) (the joint's offset), as an explicit frame.
fn shifted(s: &SnapPt, x: f64, y: f64) -> Value {
    let ax = s.z.any_perp();
    let ay = s.z.cross(ax);
    let mut p = json!({"frame": {"origin": pt(s.at + ax * x + ay * y), "z": pt(s.z), "x": pt(ax)}});
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
        Asm::NewComponent { name, activate } => {
            enter |= text_row(ui, "Name", name, crate::i18n::tr_label("Component1")).0;
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
                        && crate::ui_kit::button(ui, crate::i18n::tr("cmd.component_from_bodies"), crate::ui_kit::Button::Normal)
                            .on_hover_text(crate::i18n::tr("joint.from_bodies_hint"))
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
            choice_row(ui, "Type", "jt_type", &mut f.kind, &TYPE_LABELS);
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
                    let label = crate::i18n::tr(if f.animate { "ui.joint_stop" } else { "ui.joint_play" });
                    if crate::ui_kit::button(ui, label, crate::ui_kit::Button::Normal)
                        .on_hover_text(crate::i18n::tr_label("Play the joint's motion in the view"))
                        .clicked()
                    {
                        f.animate = !f.animate;
                    }
                });
                crate::ui_kit::section(ui, "joint_limits", "Limits", false, |ui| {
                    for (i, (on, lo, hi)) in f.limits.iter_mut().enumerate() {
                        let name = kind.dofs().get(i).copied().unwrap_or("value");
                        form_row(ui, &format!("{} limits", capital(name)), |ui| {
                            crate::ui_kit::checkbox(ui, "", on);
                        });
                        if *on {
                            enter |= expr_row(ui, "  Minimum", lo);
                            enter |= expr_row(ui, "  Maximum", hi);
                        }
                    }
                });
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
            enter |= text_row(ui, "Name", name, crate::i18n::tr_label("Joint Origin1")).0;
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
                    let why = crate::i18n::format("drive.rigid", &[("joint", name.as_str())]);
                    crate::ui_kit::status_line(ui, &why, crate::ui_kit::StatusTone::Warning);
                    let r = crate::ui_kit::button(ui, crate::i18n::tr_label("Edit Joint"), crate::ui_kit::Button::Normal);
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
                        ui.label(RichText::new(crate::i18n::tr_label("no joint that moves yet")).color(t.text_dim));
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
                        let id = egui::Id::new("sc_drive_value");
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
                            .size(crate::theme::Tokens::get().font_small)
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
        Asm::MotionLink { a, b, ia, ib, ratio, offset } => {
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
        Asm::Interference { result, overlaps, of, clearance } => {
            let picked: Vec<Sel> = inputs.first().map(|i| i.items.clone()).unwrap_or_default();
            enter |= expr_row(ui, "Clearance", clearance);
            form_row(ui, "", |ui| {
                let stale = result.is_some() && *of != picked;
                if crate::ui_kit::button(
                    ui,
                    crate::i18n::tr(if stale { "dialog.compute_again" } else { "dialog.compute" }),
                    crate::ui_kit::Button::Normal,
                )
                .clicked()
                {
                    let (r, o) = interference(app, &picked, clearance);
                    *result = Some(r);
                    *overlaps = o;
                    *of = picked;
                }
            });
            if let Some(r) = result.as_ref() {
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

/// Run the check (overlaps, pairs closer than the clearance, each body's nearest neighbour) and
/// find the overlapping volumes (world placement) to show in red.
fn interference(app: &mut SolveApp, picked: &[Sel], clearance: &str) -> (Value, Vec<Mesh>) {
    let names: Vec<String> = picked.body_names();
    let (scale, _) = crate::prefs::unit_scale(&app.session.doc.units);
    let clearance = match crate::prefs::parse_length_mm(clearance, scale) {
        Some(v) if v >= 0.0 => v,
        _ => return (json!({ "error": crate::i18n::tr("dialog.clearance_invalid") }), Vec::new()),
    };
    let mut p = json!({ "clearance": clearance, "nearest": 1 });
    if !names.is_empty() {
        p["bodies"] = json!(names);
    }
    let r = match app.run("inspect.clearance", p) {
        Ok(v) => v,
        Err(e) => return (json!({ "error": e }), Vec::new()),
    };
    let st = app.session.world_state();
    let mut meshes = Vec::new();
    for hit in r["overlaps"].as_array().into_iter().flatten() {
        let (Some(a), Some(b)) = (hit["a"].as_str().and_then(|n| st.body(n)), hit["b"].as_str().and_then(|n| st.body(n))) else { continue };
        if let Ok(Some(o)) = solidcraft_engine::kernel::boolean(&a.body, &b.body, solidcraft_engine::kernel::BoolOp::Intersect)
            && let Ok(m) = o.tessellate((o.size() * 2e-3).max(1e-3))
        {
            meshes.push(m);
        }
    }
    (r, meshes)
}

fn interference_rows(app: &SolveApp, ui: &mut egui::Ui, r: &Value) {
    let t = Tokens::get();
    if let Some(e) = r.get("error").and_then(Value::as_str) {
        crate::ui_kit::status_line(ui, e, crate::ui_kit::StatusTone::Error);
        return;
    }
    let hits = r["overlaps"].as_array().cloned().unwrap_or_default();
    form_row(ui, "Checked", |ui| {
        ui.label(crate::i18n::format("dialog.bodies_count", &[("count", &r["bodies"].as_u64().unwrap_or(0).to_string())]));
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
            ui.label(RichText::new(crate::i18n::tr("ui.distance")).semibold().color(crate::theme::Tokens::get().text));
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
            ui.label(RichText::new(crate::i18n::tr("ui.distance")).semibold().color(crate::theme::Tokens::get().text));
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
            ui.label(RichText::new(crate::i18n::tr_label("Volume")).semibold().color(crate::theme::Tokens::get().text));
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
            ui.label(RichText::new(crate::i18n::tr("dialog.could_not_be_checked")).color(t.warning))
                .on_hover_text(u["reason"].as_str().unwrap_or(""));
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
    let occs: Vec<Option<u64>> = bodies.iter().map(|b| occurrence_of_body(&app.session, b)).collect();
    if occs.first().is_some_and(|first| first.is_some() && occs.iter().all(|o| o == first)) {
        return Some(match bodies.as_slice() {
            [body] => (crate::i18n::format("joint.one_body", &[("body", body.as_str())]), Vec::new()),
            _ => (crate::i18n::format("joint.one_component", &[("bodies", &bodies.join(", "))]), bodies),
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
        sel_items(inputs, i).first().and_then(|x| snap_of(s, x)).ok_or_else(|| format!("select {what} first"))
    };
    let eval = |e: &str, kind: ValueKind| -> Result<f64, String> {
        if e.trim().is_empty() { Ok(0.0) } else { crate::length_input::eval(&s.doc, e, kind).map_err(|e| e.to_string()) }
    };
    let cmd = |id: &str, p: Value| vec![(id.to_string(), p)];
    Ok(match k {
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
            let limits: Vec<Value> =
                f.limits.iter().take(jk.dofs().len()).map(|(on, lo, hi)| if *on { json!([lo, hi]) } else { Value::Null }).collect();
            if let Some(id) = f.editing {
                let mut p = json!({"joint": id, "type": kind, "flip": f.flip, "offset": f.offset[2]});
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
                return Ok(out);
            }
            if f.as_built {
                let occs = occurrences(s, sel_items(inputs, 0));
                if occs.len() != 2 {
                    return Err("select two components".into());
                }
                let mut p = json!({"type": kind, "a": occs.first(), "b": occs.get(1), "limits": limits});
                if let Some(x) = sel_items(inputs, 1).first().and_then(|x| snap_of(s, x)) {
                    p["at"] = x.param;
                }
                return Ok(cmd("joint.as_built", p));
            }
            let (a, b) = (snap(0, "component 1's joint origin")?, snap(1, "component 2's joint origin")?);
            if a.param.get("occurrence").is_some() && a.param.get("occurrence") == b.param.get("occurrence") {
                return Err("the two origins are on the same component".into());
            }
            let (x, y) = (eval(&f.offset[0], ValueKind::Length)?, eval(&f.offset[1], ValueKind::Length)?);
            let a_param = if x.abs() > 1e-12 || y.abs() > 1e-12 { shifted(&a, x, y) } else { a.param };
            // Picked origins mate by default (the engine's default: B's z opposite A's, faces
            // meet face to face, a pin goes into its hole); the dialog's Flip is the command's
            // `flip`, which turns B the other way.
            let p = json!({"type": kind, "a": a_param, "b": b.param, "flip": f.flip, "offset": f.offset[2], "angle": f.angle, "limits": limits});
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
                    return Err(crate::i18n::format(
                        "joint.rigid_group_one_component",
                        &[("count", &items.len().to_string()), ("command", crate::i18n::tr("cmd.component_from_bodies"))],
                    ));
                }
                return Err(crate::i18n::tr("joint.rigid_group_select_components").into());
            }
            cmd("joint.rigid_group", json!({ "occurrences": occs }))
        }
        Asm::Drive { joint, values, .. } => {
            let j = s.doc.assembly.joints.get(*joint).ok_or("no joint that moves yet")?;
            // A rigid joint has nothing to drive: the dialog says why and offers Edit Joint (#6981).
            if j.kind == JointKind::Rigid {
                return Err(crate::i18n::format("drive.rigid", &[("joint", &j.name)]));
            }
            cmd("joint.drive", json!({"joint": j.id, "values": values}))
        }
        Asm::MotionLink { a, b, ia, ib, ratio, offset } => {
            let js = &s.doc.assembly.joints;
            let (Some(ja), Some(jb)) = (js.get(*a), js.get(*b)) else { return Err("needs two joints".into()) };
            // The offset as typed: the engine reads it in Joint 2's units (deg or mm), and the
            // ratio in Joint 2's units per Joint 1's.
            let ratio = eval(ratio, ValueKind::Unitless)?;
            cmd("joint.motion_link", json!({"a": ja.id, "b": jb.id, "ia": ia, "ib": ib, "ratio": ratio, "offset": offset.trim()}))
        }
        Asm::Coupling(f) => return crate::dialogs_coupling::commands(app, f, inputs),
        Asm::Cam(f) => return crate::dialogs_coupling::cam::commands(app, f, inputs),
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
        return Some(crate::i18n::format("drive.unconnected", &[("part", part)]));
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
        return Some(crate::i18n::format("drive.stopped_by_loop", &[("value", &at), ("joint", &name(&l["joint"]))]));
    }
    let c = reply.get("stopped_by_contact").filter(|v| v.is_object())?;
    Some(crate::i18n::format("drive.stopped_by_contact", &[("value", &at), ("a", &name(&c["a"])), ("b", &name(&c["b"]))]))
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
    // A joint names its two picks; a joint origin and a trace have one.
    let two = matches!(k, Asm::Joint(_));
    let hover = match k {
        Asm::Joint(f) => Some(&mut f.hover),
        Asm::JointOrigin { hover, .. } => Some(hover),
        Asm::Trace(f) => Some(&mut f.hover),
        _ => None,
    };
    let Some(hover) = hover else { return };
    // The picked origins: Component 1 in the accent colour, Component 2 in orange.
    let picked: Vec<(SnapPt, Color32)> = d
        .inputs
        .iter()
        .enumerate()
        .filter(|(_, i)| i.accept == SNAPS)
        .filter_map(|(n, i)| i.items.first().and_then(|x| snap_of(&app.session, x)).map(|s| (s, if n == 0 { t.accent } else { t.warning })))
        .collect();
    for (n, (s, c)) in picked.iter().enumerate() {
        let tag = format!("Component {}", n + 1);
        glyph(painter, proj, s, *c, app.cam.half_height(), two.then_some(tag.as_str()));
    }
    // The snap under the cursor (found again only when the hovered item changes).
    let accepting = d.inputs.get(d.active).is_some_and(|i| i.accept == SNAPS);
    let under = app.viewport.hover.as_ref().and_then(crate::viewport::hit_sel).filter(|_| accepting);
    match under {
        Some(sel) => {
            let fresh = !matches!(hover, Some((s, _)) if *s == sel);
            if fresh {
                *hover = Some((sel.clone(), snap_of(&app.session, &sel)));
            }
            if let Some((_, Some(s))) = hover.as_ref() {
                glyph(painter, proj, s, t.hover_profile_edge, app.cam.half_height(), Some(snap_name(s.kind)));
            }
        }
        None => *hover = None,
    }
}

/// A snap: its kind's marker at the point and the z axis as an arrow.
fn glyph(painter: &egui::Painter, proj: &Proj, s: &SnapPt, col: Color32, half_height: f64, tag: Option<&str>) {
    let Some(c) = proj.to_screen(s.at) else { return };
    let len = half_height * 2.0 * 0.06;
    crate::icons::markers::snap(painter, c, proj.to_screen(s.at + s.z * len), s.kind, col, tag);
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
    if crate::ui_kit::button(ui, crate::i18n::tr_label("Edit Joint"), crate::ui_kit::Button::Normal).clicked() {
        *open = edit_joint(app, id);
        ui.close();
    }
    if kind != JointKind::Rigid && crate::ui_kit::button(ui, crate::i18n::tr_label("Drive Joint"), crate::ui_kit::Button::Normal).clicked() {
        *open = drive_joint(app, id);
        ui.close();
    }
    if crate::ui_kit::button(ui, crate::i18n::tr_label(if suppressed { "Unsuppress" } else { "Suppress" }), crate::ui_kit::Button::Normal).clicked() {
        *run = Some(("joint.edit", json!({"joint": id, "suppressed": !suppressed})));
        ui.close();
    }
    ui.separator();
    if crate::ui_kit::button(ui, crate::i18n::tr_label("Delete"), crate::ui_kit::Button::Normal).clicked() {
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

/// Which joints a row of joint chips in the History bar shows: those no feature folder holds
/// (after the features), or a folder's (an import's rigid group, see `timeline_folders`).
#[derive(Clone, Copy)]
pub(crate) enum JointRun<'a> {
    Loose,
    Folder { id: u64, name: &'a str },
}

impl JointRun<'_> {
    /// The browser-state key that remembers the joints were shown one by one.
    fn key(self) -> String {
        match self {
            Self::Loose => JOINTS_KEY.to_string(),
            Self::Folder { id, .. } => format!("{JOINTS_KEY}:{id}"),
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

fn joints_open(app: &SolveApp, count: usize, run: JointRun) -> bool {
    count <= JOINT_GROUP_MIN || crate::browser::is_open(app, &run.key(), false)
}

fn toggle_joints(app: &mut SolveApp, run: JointRun) {
    crate::browser::toggle(app, &run.key(), false);
}

/// The button that stands for `count` grouped joints, its top left at `at` (see
/// `HistoryGroupChip`). Returns its width and whether it was clicked.
fn joint_group_button(
    ui: &mut egui::Ui,
    p: &egui::Painter,
    at: egui::Pos2,
    count: usize,
    open: bool,
    accent: crate::theme::TabAccent,
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
    let resp = ui.interact(br, ui.id().with(("joint_group", run.key())), egui::Sense::click());
    let handle = run.handle();
    crate::scenario::publish_handle(&handle, br.center());
    crate::scenario::publish_target(&format!("{handle}_arrow"), crate::ui_kit::HistoryGroupChip::well(br));
    let pointer = resp.hover_pos();
    let on_arrow = crate::ui_kit::HistoryGroupChip::on_arrow(br, pointer);
    crate::scenario::publish_count(&format!("{handle}_arrow_hot"), f64::from(u8::from(on_arrow)));
    chip.paint(p, br, pointer);
    let tip = if open { "click to group them" } else { "click to show each one" };
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(format!("{count} joints: {tip}"));
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
    let joints: Vec<(u64, String, JointKind, bool)> =
        app.session.doc.assembly.joints.iter().filter(|j| wanted.contains(&j.id)).map(|j| (j.id, j.name.clone(), j.kind, j.suppressed)).collect();
    if joints.is_empty() {
        return x;
    }
    if joints.len() > JOINT_GROUP_MIN {
        let open = joints_open(app, joints.len(), set);
        let accent = crate::theme::TabAccent::for_tab(&t, &app.ui.tab);
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
    for (id, name, kind, suppressed) in joints {
        let br = egui::Rect::from_min_size(egui::pos2(x, r.top() + 6.0), vec2(28.0, 28.0));
        crate::scenario::publish_handle(&format!("timeline-joint:{name}"), br.center());
        let resp = ui.interact(br, ui.id().with(("joint", id)), egui::Sense::click());
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
        let resp = resp.on_hover_text(format!("{name}  ({kind:?} joint)\nDouble-click to edit · right-click for more"));
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
        if joints.len() > JOINT_GROUP_MIN {
            let shown = joints_open(app, joints.len(), JointRun::Loose);
            let header = format!("JOINTS ({})", joints.len());
            let clicked = ui
                .horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(t.form_section_icon_size, t.form_section_icon_size), egui::Sense::hover());
                    let fold = crate::ui_kit::disclosure_button_at(ui, rect, shown, &header);
                    let response = ui.add(
                        egui::Label::new(egui::RichText::new(&header).size(crate::theme::Tokens::get().font_micro).semibold().color(t.text_dim))
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
            ui.label(egui::RichText::new("JOINTS").size(crate::theme::Tokens::get().font_micro).semibold().color(t.text_dim));
        }
    }
    let mut open = None;
    let mut run = None;
    for (id, name, kind, suppressed) in joints {
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), egui::Sense::click());
        crate::scenario::publish_handle(&format!("timeline-joint:{name}"), rect.center());
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
            egui::FontId::proportional(crate::theme::Tokens::get().font_body),
            if suppressed { t.text_dim } else { t.text },
        );
        let response = response.on_hover_text(format!("{name}  ({kind:?} joint)\nDouble-click to edit · right-click for more"));
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
/// edits, drives, suppresses or deletes).
pub fn browser_joints(app: &mut SolveApp, ui: &mut egui::Ui, depth: usize) {
    use crate::browser::{Row, draw_row, is_open, toggle};
    let joints: Vec<(u64, String, JointKind, bool)> =
        app.session.doc.assembly.joints.iter().map(|j| (j.id, j.name.clone(), j.kind, j.suppressed)).collect();
    app.tree.picked_joints.retain(|p| joints.iter().any(|j| j.0 == *p));
    if joints.is_empty() {
        return;
    }
    // Drawn like every other browser node (its fold, eye and rows publish `fold:Joints`,
    // `eye:Joints`, `row:<name>`). The folder's eye is its own: hiding it hides every joint's
    // glyph without changing each joint's eye.
    let key = "c0/joints";
    let open = is_open(app, key, true);
    let folder_shown = !app.session.folder_hidden(key);
    let r = draw_row(
        ui,
        ui.id().with(key),
        &Row { depth, fold: Some(open), eye: Some(folder_shown), icon: "folder", label: "Joints", dim: !folder_shown, ..Default::default() },
    );
    if r.eye {
        crate::browser::toggle_folder(app, key);
    } else if r.fold || r.clicked {
        toggle(app, key, true);
    }
    if !open {
        return;
    }
    let mut dialog: Option<Dialog> = None;
    for (id, name, kind, suppressed) in joints {
        let visible = app.session.joint_visible(id);
        let selected = app.tree.picked_joints.contains(&id);
        let r = draw_row(
            ui,
            ui.id().with(("joint", id)),
            &Row {
                depth: depth + 1,
                eye: Some(visible),
                icon: "joint",
                label: &name,
                name: true,
                selected,
                dim: suppressed || !visible || !folder_shown,
                ..Default::default()
            },
        );
        if let Some(resp) = &r.resp {
            resp.clone().on_hover_text(format!("{kind:?} joint"));
        }
        if r.eye {
            let _ = app.run("browser.visibility", json!({ "joints": [id], "visible": !visible }));
        } else if r.clicked {
            pick_joint(app, id, ui.input(|i| i.modifiers.command || i.modifiers.ctrl));
        }
        if r.double {
            dialog = edit_joint(app, id);
        }
        // The same menus as the rest of the browser.
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
pub fn component_joints(app: &mut SolveApp, ui: &mut egui::Ui, path: &[u64], key: &str, depth: usize) {
    use crate::browser::{Row, draw_row, is_open, toggle};
    let joints = placement_joints(&app.session.doc, path);
    if joints.is_empty() {
        return;
    }
    let key = format!("{key}/joints");
    let open = is_open(app, &key, false);
    let r = draw_row(ui, ui.id().with(&key), &Row { depth, fold: Some(open), icon: "folder", label: "Joints", ..Default::default() });
    if r.fold || r.clicked {
        toggle(app, &key, false);
    }
    if !open {
        return;
    }
    // Hiding the root folder hides every glyph: these rows fade with it, as its own rows do.
    let folder_shown = !app.session.folder_hidden("c0/joints");
    let mut dialog: Option<Dialog> = None;
    for j in joints {
        let id = j.id;
        let visible = app.session.joint_visible(id);
        let selected = app.tree.picked_joints.contains(&id);
        let r = draw_row(
            ui,
            ui.id().with((&key, "joint", id)),
            &Row {
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
        if let Some(resp) = &r.resp {
            resp.clone().on_hover_text(format!("{:?} joint", j.kind));
        }
        if r.eye {
            let _ = app.run("browser.visibility", json!({ "joints": [id], "visible": !visible }));
        } else if r.clicked {
            pick_joint(app, id, ui.input(|i| i.modifiers.command || i.modifiers.ctrl));
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
pub(crate) fn placement_joints(doc: &solidcraft_engine::doc::Document, path: &[u64]) -> Vec<PlacementJoint> {
    let Some(&occ) = path.last() else { return Vec::new() };
    let name_of = |o: u64| doc.occurrences.iter().find(|x| x.id == o).map(|x| x.name.clone()).unwrap_or_default();
    doc.assembly
        .joints
        .iter()
        .filter_map(|j| {
            let other = if j.a.occurrence == occ {
                j.b.occurrence
            } else if j.b.occurrence == occ {
                j.a.occurrence
            } else {
                return None;
            };
            Some(PlacementJoint { id: j.id, name: j.name.clone(), kind: j.kind, suppressed: j.suppressed, other: name_of(other) })
        })
        .collect()
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

/// Named joint origins in the viewport: a small triad where each sits.
pub fn origins(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    let t = Tokens::get();
    let len = app.cam.half_height() * 2.0 * 0.04;
    for (_, m) in origins_world(&app.session) {
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
    app.session
        .doc
        .assembly
        .joints
        .iter()
        .filter_map(|j| {
            let picked = app.tree.picked_joints.contains(&j.id);
            (picked || app.session.joint_drawn(j.id)).then_some((j.id, picked))
        })
        .collect()
}

/// Joint glyphs in the viewport: a ring where the joint sits, its motion axis, and its limits
/// (ticks on the ring for a turn, along the axis for a slide). Glyphs are only drawn; they are
/// never picked in the viewport.
pub fn joint_glyphs(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    let drawn = drawn_joints(app);
    if drawn.is_empty() {
        return;
    }
    let t = Tokens::get();
    let frames = joint_frames_world(&app.session);
    let len = app.cam.half_height() * 2.0 * 0.05;
    let at = |m: &solidcraft_engine::doc::Mat, x: f64, y: f64, z: f64| solidcraft_engine::doc::apply_point(m, Vec3::new(x, y, z));
    for (id, picked) in drawn {
        let Some(j) = app.session.doc.assembly.joints.iter().find(|j| j.id == id) else { continue };
        let Some((_, m)) = frames.iter().find(|(f, _)| *f == id) else { continue };
        let col = if picked {
            t.sel_edge
        } else if j.suppressed {
            t.text_dim
        } else {
            t.accent
        };
        let width = if picked { 2.5 } else { 1.5 };
        let stroke = Stroke::new(width, col);
        let Some(c) = proj.to_screen(at(m, 0.0, 0.0, 0.0)) else { continue };
        // The motion axis (z), both ways.
        if let (Some(a), Some(b)) = (proj.to_screen(at(m, 0.0, 0.0, -len)), proj.to_screen(at(m, 0.0, 0.0, len))) {
            painter.line_segment([a, b], stroke);
        }
        // A ring about the axis.
        let r = len * 0.5;
        let ring: Vec<Pos2> = (0..=32)
            .filter_map(|k| {
                let a = k as f64 / 32.0 * std::f64::consts::TAU;
                proj.to_screen(at(m, r * a.cos(), r * a.sin(), 0.0))
            })
            .collect();
        if ring.len() > 1 {
            painter.add(egui::Shape::line(ring, stroke));
        }
        // Limits: ticks across the ring for a turn, across the axis (or slot) for a slide.
        for (i, lim) in j.limits.iter().enumerate() {
            let Some((lo, hi)) = lim else { continue };
            let name = j.kind.dofs().get(i).copied().unwrap_or("");
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
                if let (Some(p), Some(q)) = (proj.to_screen(at(m, p.0, p.1, p.2)), proj.to_screen(at(m, q.0, q.1, q.2))) {
                    painter.line_segment([p, q], Stroke::new(width, t.warning));
                }
            }
        }
        painter.circle(c, if picked { 4.5 } else { 3.5 }, t.panel, stroke);
    }
}

#[cfg(test)]
#[path = "joint_browser_tests.rs"]
mod joint_browser_tests;
