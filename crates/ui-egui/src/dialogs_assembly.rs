//! Assembly dialogs: New Component, Joint, As-built Joint, Joint Origin, Rigid Group, Drive
//! Joints, Motion Link, Gear, Rack and Pinion, Screw, Cam (`dialogs_coupling`) and Interference. Joint origins are picked by hovering geometry: a face
//! snaps to its centre (or a round face to the centre of its nearer end), an edge to its middle
//! (a round edge to its centre), a vertex to itself; the snap shows as a glyph with its z axis
//! before the click. Joints and drives preview on the placed (world) model: the moving
//! component shows where it goes, and with Animate the joint plays its motion.

use crate::fonts::RichTextExt as _;
use crate::ui_kit::{choice_row, expr_row, text_row};
use egui::{Color32, Pos2, RichText, Stroke, vec2};
use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::Session;
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::doc::joints::{JointKind, Snap, XRef};
use solidcraft_engine::geom::{Aabb3, Mesh, Vec3};

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind, combo, field, form_row};
use crate::preview::{Built, Colors};
use crate::selection::{AXES, BODIES, EDGES, FACES, PLANES, SelInput, SelectionExt as _, VERTICES, items as sel_items, point_value as pt, polyline_mid};
use crate::theme::Tokens;

use crate::viewport::Proj;

pub(crate) const ALIGNMENTS: crate::selection::Accept = FACES | EDGES | AXES | PLANES;

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
/// What a joint origin input accepts (snaps on faces, edges and vertices).
pub const SNAPS: u16 = FACES | EDGES | VERTICES;
/// Commands whose effect is where occurrences sit (previewed on the placed model).
const MOVES: [&str; 12] = [
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
    /// Flip B relative to the mate (faces apart instead of together).
    pub flip: bool,
    pub angle: String,
    pub offset: [String; 3],
    /// Per joint value: limited, minimum, maximum.
    pub limits: Vec<(bool, String, String)>,
    pub animate: bool,
    /// Animation phase, 0…1.
    pub phase: f64,
    pub hover: Hover,
    /// Saved secondary references while editing; picked replacements live in the inputs.
    pub alignment: [Option<Value>; 2],
    pub reverse_alignment: [bool; 2],
    /// UI row -> backend A/B side. References and inversion remain backend-indexed.
    pub alignment_order: [usize; 2],
    pub alignment_order_ready: bool,
    pub alignment_origin_problem: Option<&'static str>,
    pub alignment_origins: Option<[u64; 2]>,
    pub alignment_revision: u64,
    /// Explicit semantics for circular picks; old saved axes retain Axis mode.
    pub circle_center: [bool; 2],
}

impl JointForm {
    fn new(as_built: bool) -> JointForm {
        JointForm {
            as_built,
            editing: None,
            kind: 0,
            flip: false,
            angle: "0 deg".into(),
            offset: ["0 mm".into(), "0 mm".into(), "0 mm".into()],
            limits: Vec::new(),
            animate: false,
            phase: 0.0,
            hover: None,
            alignment: [None, None],
            reverse_alignment: [false; 2],
            alignment_order: [0, 1],
            alignment_order_ready: false,
            alignment_origin_problem: None,
            alignment_origins: None,
            alignment_revision: 0,
            circle_center: [true; 2],
        }
    }

    fn joint_kind(&self) -> JointKind {
        TYPES.get(self.kind).and_then(|t| JointKind::parse(t)).unwrap_or(JointKind::Rigid)
    }

    /// Limits rows sized to the joint's values.
    fn fit_limits(&mut self) {
        let k = self.joint_kind();
        let n = k.dofs().len();
        while self.limits.len() < n {
            let i = self.limits.len();
            self.limits.push(if k.is_angle(i) { (false, "-90 deg".into(), "90 deg".into()) } else { (false, "0 mm".into(), "20 mm".into()) });
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
        "joint.create" => (
            Asm::Joint(Box::new(JointForm::new(false))),
            vec![
                SelInput::new("Component 1", SNAPS, false),
                SelInput::new("Component 2", SNAPS, false),
                SelInput::new("Alignment 1 (optional)", ALIGNMENTS, false),
                SelInput::new("Alignment 2 (optional)", ALIGNMENTS, false),
            ],
        ),
        "joint.as_built" => {
            (Asm::Joint(Box::new(JointForm::new(true))), vec![SelInput::new("Components", BODIES, true), SelInput::new("Position", SNAPS, false)])
        }
        "joint.origin" => (Asm::JointOrigin { name: String::new(), hover: None }, vec![SelInput::new("Snap", SNAPS, false)]),
        "joint.rigid_group" => (Asm::RigidGroup, vec![SelInput::new("Components", BODIES, true)]),
        "joint.drive" => {
            let joint = picked_joint(app);
            (Asm::Drive { joint, values: current_values(s, joint), focus: true }, vec![])
        }
        "motion.trace" => (Asm::Trace(Box::new(trace::form(s, picked_joint(app)))), vec![SelInput::new("Point", SNAPS, false)]),
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

/// The joint picked in the browser, else the first that moves (an index into the design's joints).
fn picked_joint(app: &SolveApp) -> usize {
    let s = &app.session;
    let list = movable(s);
    let picked =
        app.tree.picked_joints.iter().find_map(|id| list.iter().copied().find(|i| s.doc.assembly.joints.get(*i).is_some_and(|j| j.id == *id)));
    picked.or(list.first().copied()).unwrap_or(0)
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
            if j.kind.is_angle(i) { num(v.to_degrees(), "deg") } else { num(v, "mm") }
        })
        .collect()
}

/// A value with its unit, without trailing zeros ("12 mm", "37.5 deg").
pub(crate) fn num(v: f64, unit: &str) -> String {
    format!("{} {unit}", crate::dialogs_coupling::trim(v))
}

/// The Edit Joint dialog for a joint (by id).
pub fn edit_joint(app: &SolveApp, id: u64) -> Option<Dialog> {
    let j = app.session.doc.assembly.joints.iter().find(|j| j.id == id)?;
    let mut f = JointForm::new(false);
    f.editing = Some(id);
    f.kind = TYPES.iter().position(|t| JointKind::parse(t) == Some(j.kind)).unwrap_or(0);
    f.flip = j.flip;
    for (i, origin) in [&j.a, &j.b].into_iter().enumerate() {
        if let Snap::FaceCenter { x: Some(XRef::Planar { planar_face, reverse }), .. }
        | Snap::CircleCenter { x: Some(XRef::Planar { planar_face, reverse }), .. } = &origin.snap
        {
            f.alignment[i] = Some(json!({"face": planar_face, "occurrence": origin.occurrence}));
            f.reverse_alignment[i] = *reverse;
        } else if let Snap::FaceCenter { x: Some(XRef::Reference { reference, reverse }), .. }
        | Snap::CircleCenter { x: Some(XRef::Reference { reference, reverse }), .. } = &origin.snap
        {
            f.alignment[i] = Some(json!({"reference":reference,"occurrence":origin.occurrence}));
            f.reverse_alignment[i] = *reverse;
            f.circle_center[i] = matches!(reference, solidcraft_engine::doc::joints::AlignmentRef::CircleCenter(_));
        }
    }
    // An offset or angle typed as an expression shows (and is sent back) as that expression, so
    // the joint keeps following its parameters.
    f.angle = j.exprs.angle.clone().unwrap_or_else(|| num(j.angle.to_degrees(), "deg"));
    f.offset[2] = j.exprs.offset.clone().unwrap_or_else(|| num(j.offset, "mm"));
    f.fit_limits();
    for (i, l) in f.limits.iter_mut().enumerate() {
        if let Some(Some((lo, hi))) = j.limits.get(i) {
            let fmt = |v: f64| if j.kind.is_angle(i) { num(v.to_degrees(), "deg") } else { num(v, "mm") };
            *l = (true, fmt(*lo), fmt(*hi));
        }
    }
    let mut dialog = Dialog::new(
        Kind::Assembly(Asm::Joint(Box::new(f))),
        vec![SelInput::new("Alignment 1 (optional)", ALIGNMENTS, false), SelInput::new("Alignment 2 (optional)", ALIGNMENTS, false)],
    );
    resolve_alignment_order(&app.session, &mut dialog, None);
    Some(dialog)
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
                    // The displayed center must also be a persistent round origin.
                    // A unique cylinder and end face identify the picked rim without
                    // depending on its edge-history name or a static point/vector.
                    let param = alignment_pick(s, sel, true)
                        .ok()
                        .filter(|p| {
                            let model = s.model.state();
                            ["name", "end"].into_iter().all(|key| {
                                p["reference"][key]
                                    .as_str()
                                    .is_some_and(|name| solidcraft_engine::doc::joints::named_face_spot(&s.doc, &model, None, name).is_ok())
                            })
                        })
                        .map(|p| json!({"circle":p["reference"]["name"],"end":p["reference"]["end"]}))
                        .unwrap_or_else(|| json!({"point": pt(c), "z": pt(z)}));
                    (body, SnapPt { at: c, z, kind: SnapKind::CircleCenter, param })
                }
                None => {
                    let mid = polyline_mid(e);
                    let z = m.edge_faces.get(*index).and_then(|fs| fs.first()).and_then(|f| face_normal_near(&m, *f, mid)).unwrap_or(Vec3::Z);
                    (body, SnapPt { at: mid, z, kind: SnapKind::EdgeMid, param: json!({"point": pt(mid), "z": pt(z)}) })
                }
            }
        }
        Sel::Vertex { body, point } => (body, SnapPt { at: *point, z: Vec3::Z, kind: SnapKind::Vertex, param: json!({"point": pt(*point)}) }),
        _ => return None,
    };
    if let (Some(occ), Value::Object(m)) = (occurrence_of_body(s, body), &mut snap.param) {
        m.insert("occurrence".into(), json!(occ));
    }
    Some(snap)
}

/// Same centre marker as a primary snap, but the arrow is the persisted cylinder axis,
/// not the outward end normal used when mating primary origins.
fn alignment_snap_of(s: &Session, sel: &Sel, center_frame: Option<(Vec3, Vec3)>) -> Option<SnapPt> {
    let mut snap = snap_of(s, sel)?;
    if snap.kind == SnapKind::CircleCenter
        && let Some((origin, z)) = center_frame
    {
        let v = snap.at - origin;
        let z = z.normalized()?;
        snap.z = (v - z * v.dot(z)).normalized()?;
        return Some(snap);
    }
    if snap.kind == SnapKind::CircleCenter {
        let st = s.world_state();
        let cylinders = match sel {
            Sel::Edge { body, index, .. } => {
                let b = st.body(body)?;
                let mesh = b.mesh();
                mesh.edge_faces
                    .get(*index)?
                    .iter()
                    .filter_map(|f| solidcraft_engine::kernel::cylinder_face(&b.body, &mesh, *f as usize))
                    .collect::<Vec<_>>()
            }
            Sel::Face { body, index, .. } => {
                let b = st.body(body)?;
                solidcraft_engine::kernel::cylinder_face(&b.body, &b.mesh(), *index).into_iter().collect()
            }
            _ => Vec::new(),
        };
        if let [c] = cylinders.as_slice() {
            snap.z = *c.axis;
        }
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
                    ui.add(egui::Label::new(RichText::new(why).color(t.warning)).wrap());
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
            f.fit_limits();
            if !f.as_built {
                for (i, label) in ["Invert alignment 1", "Invert alignment 2"].into_iter().enumerate() {
                    let side = f.alignment_order[i];
                    let selected = f.alignment[side].is_some() || !sel_items(inputs, i + if f.editing.is_some() { 0 } else { 2 }).is_empty();
                    form_row(ui, label, |ui| {
                        let response = ui.add_enabled(selected, egui::Checkbox::new(&mut f.reverse_alignment[side], ""));
                        crate::scenario::publish_handle(&format!("dialog:invert-alignment:{}", i + 1), response.rect.center());
                    });
                }
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
                    let label = if f.animate { "■ Stop" } else { "▶ Play" };
                    if crate::ui_kit::button(ui, label, crate::ui_kit::Button::Normal).on_hover_text("Play the joint's motion in the view").clicked()
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
            if list.is_empty() {
                ui.vertical(|ui| {
                    ui.label(RichText::new(crate::i18n::tr_label("no joint that moves yet")).color(t.text_dim));
                });
                return false;
            }
            let labels: Vec<String> =
                list.iter().filter_map(|i| s.doc.assembly.joints.get(*i)).map(|j| format!("{} ({:?})", j.name, j.kind)).collect();
            let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
            let mut pos = list.iter().position(|i| i == joint).unwrap_or(0);
            choice_row(ui, "Joint", "drv_joint", &mut pos, &refs);
            let pick = list.get(pos).copied().unwrap_or(0);
            if pick != *joint || values.is_empty() {
                *joint = pick;
                *values = current_values(s, pick);
            }
            let Some(j) = s.doc.assembly.joints.get(*joint).cloned() else { return false };
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
                let cur = s.doc.eval(v, unit_kind).ok().filter(|x| x.is_finite()).unwrap_or(0.0);
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
                        *v = if angle { format!("{shown:.1} deg") } else { format!("{shown:.2} mm") };
                    }
                });
                // The slider's range: the joint's limits, or the default span when it has none.
                let limited = j.limits.get(i).copied().flatten().is_some();
                let fmt = |x: f64| {
                    let (x, unit) = if angle { (x.to_degrees(), "°") } else { (x, " mm") };
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
        }
        Asm::MotionLink { a, b, ia, ib, ratio, offset } => {
            let s = &app.session;
            let joints = &s.doc.assembly.joints;
            if joints.len() < 2 {
                form_row(ui, "", |ui| {
                    ui.label(RichText::new(crate::i18n::tr_label("needs two joints")).color(t.text_dim));
                });
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
        form_row(ui, "", |ui| {
            ui.label(RichText::new(e).color(t.error));
        });
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
        form_row(ui, "Result", |ui| {
            ui.label(RichText::new(crate::i18n::tr_label("no interference")).color(t.text_dim));
        });
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

fn sel_items(inputs: &[SelInput], i: usize) -> &[Sel] {
    inputs.get(i).map(|x| x.items.as_slice()).unwrap_or(&[])
}

pub(crate) fn alignment_side(k: &Kind, input: usize) -> Option<usize> {
    let Kind::Assembly(Asm::Joint(f)) = k else { return None };
    if f.as_built {
        return None;
    }
    let side = input.checked_sub(if f.editing.is_some() { 0 } else { 2 })?;
    f.alignment_order.get(side).copied()
}

pub(crate) fn alignment_dialog(d: &Dialog) -> bool {
    matches!(&d.kind, Kind::Assembly(Asm::Joint(f)) if !f.as_built)
}

fn alignment_occurrences(s: &Session, f: &JointForm, inputs: &[SelInput]) -> Option<[u64; 2]> {
    if let Some(id) = f.editing {
        let j = s.doc.assembly.joints.iter().find(|j| j.id == id)?;
        return Some([j.a.occurrence, j.b.occurrence]);
    }
    let mut out = [0; 2];
    for (side, item) in out.iter_mut().enumerate() {
        let picked = inputs.get(side)?.items.first()?;
        *item = alignment_occurrence(s, picked)?;
        if snap_of(s, picked)?.param["occurrence"].as_u64() != Some(*item) {
            return None;
        }
    }
    Some(out)
}

/// Prefer measured motion of the base solution. Explicit grounding (including rigidly
/// grounded chains) also identifies the stationary side when an edited joint already sits
/// in its solved pose. Never assume that the first picked origin is the moving component.
pub(crate) fn resolve_alignment_order(s: &Session, d: &mut Dialog, interaction: Option<&crate::preview::JointInteraction>) {
    let Kind::Assembly(Asm::Joint(f)) = &mut d.kind else { return };
    if f.as_built {
        return;
    }
    let supported = if let Some(id) = f.editing {
        s.doc
            .assembly
            .joints
            .iter()
            .find(|j| j.id == id)
            .is_none_or(|j| [&j.a.snap, &j.b.snap].into_iter().all(|snap| matches!(snap, Snap::FaceCenter { .. } | Snap::CircleCenter { .. })))
    } else {
        d.inputs
            .iter()
            .take(2)
            .filter_map(|i| i.items.first())
            .filter_map(|sel| snap_of(s, sel))
            .all(|snap| snap.param.get("face").is_some() || snap.param.get("circle").is_some())
    };
    f.alignment_origin_problem = (!supported)
        .then_some("Secondary alignment needs face origins or uniquely named cylindrical hole rims; repick the primary origins to use alignment");
    if !supported {
        f.alignment_order_ready = false;
        return;
    }
    let Some(origins) = alignment_occurrences(s, f, &d.inputs) else {
        f.alignment_order_ready = false;
        return;
    };
    let same = f.alignment_origins == Some(origins) && f.alignment_revision == s.revision;
    if same && f.alignment_order_ready {
        return;
    }
    let interaction = interaction.filter(|view| view.source_revision == s.revision).filter(|view| {
        let joint =
            if let Some(id) = f.editing { view.document.assembly.joints.iter().find(|j| j.id == id) } else { view.document.assembly.joints.last() };
        joint.is_some_and(|j| [j.a.occurrence, j.b.occurrence] == origins)
    });
    let mut fixed = std::collections::BTreeSet::from([0]);
    fixed.extend(s.doc.occurrences.iter().filter(|o| o.grounded).map(|o| o.id));
    if let Some(view) = interaction {
        fixed.extend(view.fixed_occurrences.iter().copied());
    }
    loop {
        let size = fixed.len();
        for o in &s.doc.occurrences {
            if o.parent != 0 && fixed.contains(&o.parent) {
                fixed.insert(o.id);
            }
        }
        for j in s.doc.assembly.joints.iter().filter(|j| !j.suppressed && Some(j.id) != f.editing && j.kind == JointKind::Rigid) {
            if fixed.contains(&j.a.occurrence) {
                fixed.insert(j.b.occurrence);
            }
            if fixed.contains(&j.b.occurrence) {
                fixed.insert(j.a.occurrence);
            }
        }
        if size == fixed.len() {
            break;
        }
    }
    let anchored = origins.map(|o| fixed.contains(&o));
    let order = match anchored {
        [true, false] => Some([1, 0]),
        [false, true] => Some([0, 1]),
        _ => interaction.and_then(|view| {
            let j = if let Some(id) = f.editing {
                view.document.assembly.joints.iter().find(|j| j.id == id)
            } else {
                view.document.assembly.joints.last()
            }?;
            if [j.a.occurrence, j.b.occurrence] != origins {
                return None;
            }
            let raw = s.model.state();
            let frames = s.doc.body_frames_with_source(&raw);
            let moved = origins.map(|o| {
                frames.iter().any(|(body, _, _, path, _)| {
                    path.first().copied().unwrap_or(0) == o && view.to_document.get(body).is_some_and(|m| !solidcraft_engine::doc::is_identity(m))
                })
            });
            match moved {
                [true, false] => Some([0, 1]),
                [false, true] => Some([1, 0]),
                _ => None,
            }
        }),
    };
    let Some(order) = order else {
        f.alignment_order_ready = false;
        return;
    };
    let base = if f.editing.is_some() { 0 } else { 2 };
    if order != f.alignment_order && d.inputs.len() > base + 1 {
        let old_first = std::mem::take(&mut d.inputs[base].items);
        d.inputs[base].items = std::mem::replace(&mut d.inputs[base + 1].items, old_first);
    }
    f.alignment_order = order;
    f.alignment_order_ready = true;
    f.alignment_origins = Some(origins);
    f.alignment_revision = s.revision;
}

pub(crate) fn update_alignment_order(app: &mut SolveApp) {
    let Some(mut d) = app.dialog.take() else { return };
    resolve_alignment_order(&app.session, &mut d, app.preview.interaction.as_ref());
    app.dialog = Some(d);
}

pub(crate) fn alignment_input_available(d: &Dialog, index: usize) -> bool {
    if alignment_side(&d.kind, index).is_none() {
        return true;
    }
    let Kind::Assembly(Asm::Joint(f)) = &d.kind else { return false };
    if !f.alignment_order_ready {
        return false;
    }
    let base = if f.editing.is_some() { 0 } else { 2 };
    index == base || f.alignment[f.alignment_order[0]].is_some() || !sel_items(&d.inputs, base).is_empty()
}

pub(crate) fn alignment_hover_commands(app: &SolveApp, d: &Dialog) -> Option<Vec<(String, Value)>> {
    let side = alignment_side(&d.kind, d.active)?;
    // Highlight and snap markers are always available, but the first reference must be
    // clicked before a counterpart can transiently change the assembled pose.
    let Kind::Assembly(Asm::Joint(f)) = &d.kind else { return None };
    let other = 1 - side;
    let row = f.alignment_order.iter().position(|s| *s == other)? + if f.editing.is_some() { 0 } else { 2 };
    if f.alignment[other].is_none() && sel_items(&d.inputs, row).is_empty() {
        return None;
    }
    let hit = app.viewport.hover.as_ref()?;
    let sel = d.candidate(&app.session, hit)?;
    let mut candidate = d.clone();
    candidate.inputs.get_mut(d.active)?.items = vec![sel];
    crate::dialogs::apply_commands(app, &candidate).ok()
}

pub(crate) fn stored_alignment(k: &Kind, input: usize) -> Option<Value> {
    let side = alignment_side(k, input)?;
    let Kind::Assembly(Asm::Joint(f)) = k else { return None };
    f.alignment.get(side)?.clone()
}

pub(crate) fn clear_alignment(k: &mut Kind, input: usize) {
    let Some(side) = alignment_side(k, input) else { return };
    if let Kind::Assembly(Asm::Joint(f)) = k {
        f.alignment[side] = None;
        f.reverse_alignment[side] = false;
    }
}

/// Exact authored path, independent of the solved preview's body placement.
fn alignment_occurrence(s: &Session, sel: &Sel) -> Option<u64> {
    match sel {
        Sel::Face { body, .. } | Sel::Edge { body, .. } => {
            let model = s.model.state();
            let frames = s.doc.body_frames_with_source(&model);
            let (_, _, _, path, _) = frames.iter().find(|(name, ..)| name == body)?;
            (path.len() <= 1).then(|| path.first().copied().unwrap_or(0))
        }
        Sel::ExternalSketchCurve { sketch, .. } => feature_occurrence(s, *sketch),
        Sel::Axis { name } | Sel::Plane { name } => {
            if matches!(name.as_str(), "X" | "Y" | "Z" | "XY" | "XZ" | "YZ") {
                return Some(0);
            }
            feature_occurrence(s, s.model.state().construct.iter().find(|c| c.name == *name)?.feature)
        }
        _ => None,
    }
}

fn feature_occurrence(s: &Session, feature: u64) -> Option<u64> {
    let component = s.doc.feature(feature)?.component;
    if component == 0 {
        return Some(0);
    }
    let mut occurrences = s.doc.occurrences.iter().filter(|o| o.component == component);
    let first = occurrences.next()?;
    (first.parent == 0 && occurrences.next().is_none()).then_some(first.id)
}

/// Reject a wrong component before hover, Select Other or a click can replace an input.
/// The engine independently validates persisted references and programmatic commands.
pub(crate) fn alignment_accepts(s: &Session, d: &Dialog, sel: &Sel) -> bool {
    let Some(side) = alignment_side(&d.kind, d.active) else { return true };
    if !alignment_input_available(d, d.active) {
        return false;
    }
    let Kind::Assembly(Asm::Joint(f)) = &d.kind else { return false };
    let expected = if let Some(id) = f.editing {
        s.doc.assembly.joints.iter().find(|j| j.id == id).map(|j| if side == 0 { j.a.occurrence } else { j.b.occurrence })
    } else {
        d.inputs.get(side).and_then(|i| i.items.first()).and_then(|primary| {
            let path = alignment_occurrence(s, primary)?;
            (snap_of(s, primary)?.param["occurrence"].as_u64() == Some(path)).then_some(path)
        })
    };
    expected.is_some() && alignment_occurrence(s, sel) == expected && alignment_projection_valid(s, d, sel, side)
}

fn primary_alignment_frame(s: &Session, d: &Dialog, side: usize) -> Option<(Vec3, Vec3)> {
    let Kind::Assembly(Asm::Joint(f)) = &d.kind else { return None };
    if let Some(id) = f.editing {
        let j = s.doc.assembly.joints.iter().find(|j| j.id == id)?;
        let mut origin = if side == 0 { j.a.clone() } else { j.b.clone() };
        if let Snap::FaceCenter { x, .. } | Snap::CircleCenter { x, .. } = &mut origin.snap {
            *x = None;
        }
        let frame = solidcraft_engine::doc::joints::resolve_origin(&s.doc, &s.model.state(), &origin).ok()?;
        let placement =
            if origin.occurrence == 0 { solidcraft_engine::doc::IDENTITY } else { s.occurrence_placement(&[origin.occurrence]).ok()?.world };
        return Some((
            solidcraft_engine::doc::apply_point(&placement, Vec3::new(frame[3][0], frame[3][1], frame[3][2])),
            solidcraft_engine::doc::apply_vector(&placement, Vec3::new(frame[2][0], frame[2][1], frame[2][2])),
        ));
    }
    let snap = snap_of(s, d.inputs.get(side)?.items.first()?)?;
    Some((snap.at, snap.z))
}

/// Cheap geometry validation before a candidate is highlighted or committed; the engine
/// still independently validates every persisted identity and projection.
fn alignment_projection_valid(s: &Session, d: &Dialog, sel: &Sel, side: usize) -> bool {
    let Kind::Assembly(Asm::Joint(f)) = &d.kind else { return false };
    let Some((origin, z)) = primary_alignment_frame(s, d, side) else { return false };
    let st = s.world_state();
    let vector = (|| -> Option<Vec3> {
        match sel {
            Sel::Edge { body, index, .. } => {
                alignment_pick(s, sel, f.circle_center[side]).ok()?;
                let b = st.body(body)?;
                let mesh = b.mesh();
                let points = mesh.edges.get(*index)?;
                if let Some((center, normal)) = circle_of(points) {
                    if f.circle_center[side] {
                        Some(center - origin)
                    } else {
                        let cylinders: Vec<_> = mesh
                            .edge_faces
                            .get(*index)?
                            .iter()
                            .filter_map(|i| solidcraft_engine::kernel::cylinder_face(&b.body, &mesh, *i as usize))
                            .collect();
                        match cylinders.as_slice() {
                            [c] => Some(*c.axis),
                            [] => Some(normal),
                            _ => None,
                        }
                    }
                } else {
                    let (a, b) = (*points.first()?, *points.last()?);
                    let dir = b - a;
                    let tol = (dir.len() * 1e-6).max(1e-7);
                    points.iter().all(|p| p.dist_to_segment(a, b) <= tol).then_some(dir)
                }
            }
            Sel::Face { body, index, .. } => {
                alignment_pick(s, sel, f.circle_center[side]).ok()?;
                let b = st.body(body)?;
                match solidcraft_engine::kernel::face_surface(&b.body, *index)? {
                    solidcraft_engine::kernel::FaceSurface::Plane { normal, .. } => Some(*normal),
                    _ => solidcraft_engine::kernel::cylinder_face(&b.body, &b.mesh(), *index).map(|c| *c.axis),
                }
            }
            Sel::Axis { name } | Sel::Plane { name } => {
                if let Some(dir) = match name.as_str() {
                    "X" | "YZ" => Some(Vec3::X),
                    "Y" => Some(Vec3::Y),
                    "Z" | "XY" => Some(Vec3::Z),
                    "XZ" => Some(-Vec3::Y),
                    _ => None,
                } {
                    return Some(dir);
                }
                let model = s.model.state();
                let dir = match &model.construct.iter().find(|c| c.name == *name)?.geom {
                    solidcraft_engine::doc::construct::ConstructGeom::Axis { dir, .. } => Some(*dir),
                    solidcraft_engine::doc::construct::ConstructGeom::Plane(p) => Some(p.normal()),
                    _ => None,
                }?;
                let occurrence = alignment_occurrence(s, sel)?;
                if occurrence == 0 {
                    Some(dir)
                } else {
                    Some(solidcraft_engine::doc::apply_vector(&s.occurrence_placement(&[occurrence]).ok()?.world, dir))
                }
            }
            Sel::ExternalSketchCurve { sketch, id } => {
                let ss = st.sketch(*sketch)?;
                let index = ss.sketch.curve_index(id)?;
                let points = ss.sketch.polyline(index);
                Some(ss.plane.to_world(*points.last()?) - ss.plane.to_world(*points.first()?))
            }
            _ => None,
        }
    })();
    match (vector, z.normalized()) {
        (Some(v), Some(z)) => (v - z * v.dot(z)).len() > 1e-8,
        _ => false,
    }
}

/// Keep the picked occurrence explicit, including repeated definitions. Nested paths have no
/// independent joint frames yet and are refused instead of aliasing to a primary instance.
fn alignment_pick(s: &Session, sel: &Sel, center: bool) -> Result<Value, String> {
    let model = s.model.state();
    let occurrence = |component| -> Result<u64, String> {
        if component == 0 {
            return Ok(0);
        }
        let occurrences: Vec<_> = s.doc.occurrences.iter().filter(|o| o.component == component).collect();
        match occurrences.as_slice() {
            [o] if o.parent == 0 => Ok(o.id),
            _ => Err("alignment construction or sketch occurrence is ambiguous or nested".into()),
        }
    };
    let feature_ref = |feature: u64, reference: Value| -> Result<Value, String> {
        let owner = s.doc.feature(feature).ok_or("alignment feature no longer exists")?.component;
        Ok(json!({"reference":reference,"occurrence":occurrence(owner)?}))
    };
    match sel {
        Sel::Face { body, index, .. } | Sel::Edge { body, index, .. } => {
            let frames = s.doc.body_frames_with_source(&model);
            let (_, source, _, path, _) = frames.iter().find(|(name, ..)| name == body).ok_or("alignment occurrence no longer exists")?;
            if path.len() > 1 {
                return Err("secondary alignment on nested occurrence paths is not supported yet".into());
            }
            let b = model.body(source).ok_or("alignment body no longer exists")?;
            let names = if matches!(sel, Sel::Edge { .. }) {
                solidcraft_engine::doc::naming::edge_names(b)
            } else {
                solidcraft_engine::doc::naming::face_names(b).to_vec()
            };
            let name = names.get(*index).cloned().filter(|n| !n.is_empty()).ok_or("alignment geometry has no persistent name")?;
            let mut value = if matches!(sel, Sel::Edge { .. }) {
                let mesh = b.mesh();
                if mesh.edges.get(*index).and_then(|e| circle_of(e)).is_some() {
                    // A hole rim may be split into arcs or named by inexact edge history.
                    // Its adjacent cylinder supplies the same persistent axis as a primary
                    // circle-centre origin. Never choose between several cylindrical faces.
                    let cylinders: Vec<_> = mesh
                        .edge_faces
                        .get(*index)
                        .into_iter()
                        .flatten()
                        .filter(|f| solidcraft_engine::kernel::cylinder_face(&b.body, &mesh, **f as usize).is_some())
                        .collect();
                    match cylinders.as_slice() {
                        [face] => {
                            let names = solidcraft_engine::doc::naming::face_names(b);
                            let name =
                                names.get(**face as usize).filter(|n| !n.is_empty()).ok_or("circle axis has no persistent cylindrical face")?;
                            if center {
                                let ends: Vec<_> = mesh.edge_faces.get(*index).into_iter().flatten().filter(|f| *f != *face).collect();
                                let [end] = ends.as_slice() else {
                                    return Err("circle center needs one persistent rim; select a circular rim with a unique end face".into());
                                };
                                let end = names.get(**end as usize).filter(|n| !n.is_empty()).ok_or("circle rim has no persistent end face")?;
                                json!({"reference":{"type":"circle_center","name":name,"end":end}})
                            } else {
                                json!({"reference":{"type":"cylinder","name":name}})
                            }
                        }
                        [] if !center => json!({"reference":{"type":"edge","name":name}}),
                        [] => return Err("circle center needs a named cylindrical rim; use Axis for a standalone circular edge".into()),
                        _ => return Err("circle axis is ambiguous; select its cylindrical face or a construction axis".into()),
                    }
                } else {
                    json!({"reference":{"type":"edge","name":name}})
                }
            } else {
                let faces = b.body.faces((b.body.size() * 2e-3).max(1e-3)).map_err(|e| e.to_string())?;
                if faces.get(*index).is_some_and(|f| f.plane_normal.is_some()) {
                    json!({"face":name})
                } else if solidcraft_engine::kernel::cylinder_face(&b.body, &b.mesh(), *index).is_some() {
                    if center {
                        return Err("select a circular rim for Center mode, or choose Axis for a cylindrical face".into());
                    }
                    json!({"reference":{"type":"cylinder","name":name}})
                } else {
                    return Err("alignment face must be planar or cylindrical".into());
                }
            };
            value["occurrence"] = json!(path.first().copied().unwrap_or(0));
            Ok(value)
        }
        Sel::ExternalSketchCurve { sketch, id } => feature_ref(*sketch, json!({"type":"sketch_line","sketch":sketch,"curve":id})),
        Sel::Axis { name } | Sel::Plane { name } => {
            let axis = matches!(sel, Sel::Axis { .. });
            let origin = if axis { matches!(name.as_str(), "X" | "Y" | "Z") } else { matches!(name.as_str(), "XY" | "XZ" | "YZ") };
            if origin {
                let reference = if axis { json!({"type":"origin_axis","axis":name}) } else { json!({"type":"origin_plane","plane":name}) };
                // The viewport's origin widget is the root frame, even while a part is active.
                return Ok(json!({"reference":reference,"occurrence":0}));
            }
            let construction = model.construct.iter().find(|c| c.name == *name).ok_or("alignment construction no longer exists")?;
            feature_ref(construction.feature, json!({"type":"construction","feature":construction.feature}))
        }
        _ => Err("select a planar face, cylindrical axis, straight/circular edge, sketch line, axis or plane".into()),
    }
}

fn alignment_params(s: &Session, f: &JointForm, inputs: &[SelInput]) -> Result<[Option<Value>; 2], String> {
    let mut out = [None, None];
    if f.as_built {
        return Ok(out);
    }
    for (side, slot) in out.iter_mut().enumerate() {
        let row = f.alignment_order.iter().position(|s| *s == side).ok_or("invalid alignment order")?;
        let picked = sel_items(inputs, row + if f.editing.is_some() { 0 } else { 2 }).first();
        *slot = match picked {
            Some(sel) => Some(alignment_pick(s, sel, f.circle_center[side])?),
            None => f.alignment[side].clone(),
        };
        if let Some(value) = slot {
            if picked.is_none() {
                match (value["reference"]["type"].as_str(), f.circle_center[side]) {
                    (Some("circle_center"), false) => {
                        value["reference"] = json!({"type":"cylinder","name":value["reference"]["name"]});
                    }
                    (Some("cylinder"), true) => return Err("pick a circular rim to change this saved axis into a Center reference".into()),
                    _ => {}
                }
            }
            value["reverse"] = json!(f.reverse_alignment[side]);
        }
    }
    Ok(out)
}

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
        if e.trim().is_empty() { Ok(0.0) } else { s.doc.eval(e, kind).map_err(|e| e.to_string()) }
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
            let alignment = alignment_params(s, f, inputs)?;
            let kind = TYPES.get(f.kind).copied().unwrap_or("rigid");
            let jk = f.joint_kind();
            let limits: Vec<Value> =
                f.limits.iter().take(jk.dofs().len()).map(|(on, lo, hi)| if *on { json!([lo, hi]) } else { Value::Null }).collect();
            if let Some(id) = f.editing {
                let mut p = json!({"joint": id, "type": kind, "flip": f.flip, "offset": f.offset[2], "angle": f.angle});
                let old = s.doc.assembly.joints.iter().find(|j| j.id == id).ok_or("joint no longer exists")?;
                for (i, (key, origin)) in [("a_alignment", &old.a), ("b_alignment", &old.b)].into_iter().enumerate() {
                    if alignment[i].is_some()
                        || matches!(
                            &origin.snap,
                            Snap::FaceCenter { x: Some(XRef::Planar { .. } | XRef::Reference { .. }), .. }
                                | Snap::CircleCenter { x: Some(XRef::Planar { .. } | XRef::Reference { .. }), .. }
                        )
                    {
                        p[key] = alignment[i].clone().unwrap_or(Value::Null);
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
            for (side, origin) in [&a, &b].into_iter().enumerate() {
                if alignment.iter().any(Option::is_some) {
                    let picked = sel_items(inputs, side).first().ok_or("select the primary face origin first")?;
                    if alignment_occurrence(s, picked).map(|o| json!(o)) != Some(origin.param["occurrence"].clone()) {
                        return Err("this primary origin aliases another instance; select an unambiguous occurrence before adding alignment".into());
                    }
                }
            }
            if a.param.get("occurrence").is_some() && a.param.get("occurrence") == b.param.get("occurrence") {
                return Err("the two origins are on the same component".into());
            }
            let (x, y) = (eval(&f.offset[0], ValueKind::Length)?, eval(&f.offset[1], ValueKind::Length)?);
            if alignment.iter().any(Option::is_some) && (x.abs() > 1e-12 || y.abs() > 1e-12) {
                return Err("secondary alignment needs a persistent face origin; set Offset X and Offset Y to zero first".into());
            }
            let a_param = if x.abs() > 1e-12 || y.abs() > 1e-12 { shifted(&a, x, y) } else { a.param };
            // Picked origins mate by default (the engine's default: B's z opposite A's, faces
            // meet face to face, a pin goes into its hole); the dialog's Flip is the command's
            // `flip`, which turns B the other way.
            let mut p = json!({"type": kind, "a": a_param, "b": b.param, "flip": f.flip, "offset": f.offset[2], "angle": f.angle, "limits": limits});
            for (i, key) in ["a_alignment", "b_alignment"].into_iter().enumerate() {
                if let Some(value) = &alignment[i] {
                    p[key] = value.clone();
                }
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
            let occs = occurrences(s, sel_items(inputs, 0));
            if occs.len() < 2 {
                return Err("select two or more components".into());
            }
            cmd("joint.rigid_group", json!({ "occurrences": occs }))
        }
        Asm::Drive { joint, values, .. } => {
            let j = s.doc.assembly.joints.get(*joint).ok_or("no joint that moves yet")?;
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
            match limited.and_then(|(_, lo, hi)| Some((app.session.doc.eval(lo, vk).ok()?, app.session.doc.eval(hi, vk).ok()?))) {
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
    let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if cmds.iter().all(|(id, _)| id.starts_with("joint.")) {
            crate::preview::build_motion(&before, &after, colors)
        } else {
            crate::preview::build(&before, &after, colors)
        }
    }));
    Some(match built {
        Ok(mut b) => {
            if cmds.iter().any(|(id, _)| matches!(id.as_str(), "joint.create" | "joint.edit")) {
                let raw = s.model.state();
                let before_frames = s.doc.body_frames_with_source(&raw);
                let mut to_document = std::collections::HashMap::new();
                for (name, _, _, _, frame) in preview.document.body_frames_with_source(&raw) {
                    if let Some((_, _, _, _, before)) = before_frames.iter().find(|(old, ..)| old == &name)
                        && let Some(inverse) = solidcraft_engine::doc::mat_inverse(&frame)
                    {
                        to_document.insert(name, solidcraft_engine::doc::mat_mul(before, &inverse));
                    }
                }
                let fixed_occurrences = cmds
                    .iter()
                    .zip(&preview.replies)
                    .find(|((id, _), _)| matches!(id.as_str(), "joint.create" | "joint.edit"))
                    .and_then(|(_, reply)| reply["fixed"].as_array())
                    .map(|items| items.iter().filter_map(Value::as_u64).collect())
                    .unwrap_or_default();
                b.interaction = Some(crate::preview::JointInteraction {
                    source_revision: s.revision,
                    fixed_occurrences,
                    document: preview.document.clone(),
                    state: after.clone(),
                    to_document,
                });
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
    let aligning = alignment_side(&d.kind, d.active).is_some();
    let center_frame = alignment_side(&d.kind, d.active).and_then(|side| {
        let Kind::Assembly(Asm::Joint(f)) = &d.kind else { return None };
        f.circle_center[side].then(|| primary_alignment_frame(&app.session, d, side)).flatten()
    });
    let accepting = d.inputs.get(d.active).is_some_and(|i| i.accept == SNAPS) || aligning;
    let under = app.viewport.hover.as_ref().and_then(|h| d.candidate(&app.session, h)).filter(|_| accepting);
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
        .filter_map(|(n, i)| {
            i.items.first().and_then(|x| snap_of(&app.session, x).map(|s| (placed_snap(app, x, s), if n == 0 { t.accent } else { t.warning })))
        })
        .collect();
    for (n, (s, c)) in picked.iter().enumerate() {
        let tag = format!("Component {}", n + 1);
        glyph(painter, proj, s, *c, app.cam.half_height(), two.then_some(tag.as_str()));
    }
    // The snap under the cursor (found again only when the hovered item changes).
    match under {
        Some(sel) => {
            let fresh = aligning || !matches!(hover, Some((s, _)) if *s == sel);
            if fresh {
                let mut source = sel.clone();
                if app.preview.draw_active()
                    && let Some(interaction) = &app.preview.interaction
                {
                    match &mut source {
                        Sel::Face { body, point, .. } | Sel::Edge { body, point, .. } | Sel::Vertex { body, point } => {
                            if let Some(frame) = interaction.to_document.get(body) {
                                *point = solidcraft_engine::doc::apply_point(frame, *point);
                            }
                        }
                        _ => {}
                    }
                }
                let snap = if aligning { alignment_snap_of(&app.session, &source, center_frame) } else { snap_of(&app.session, &source) };
                *hover = Some((sel.clone(), snap.map(|s| placed_snap(app, &source, s))));
            }
            if let Some((_, Some(s))) = hover.as_ref() {
                let label = if aligning && s.kind == SnapKind::CircleCenter {
                    if center_frame.is_some() { "Circle center alignment" } else { "Circle axis" }
                } else {
                    snap_name(s.kind)
                };
                glyph(painter, proj, s, t.hover_profile_edge, app.cam.half_height(), Some(label));
            }
        }
        None => *hover = None,
    }
}

fn placed_snap(app: &SolveApp, sel: &Sel, mut snap: SnapPt) -> SnapPt {
    if app.preview.draw_active()
        && let Some(interaction) = &app.preview.interaction
        && let Some(body) = body_of(sel)
        && let Some(frame) = interaction.to_document.get(body).and_then(solidcraft_engine::doc::mat_inverse)
    {
        snap.at = solidcraft_engine::doc::apply_point(&frame, snap.at);
        snap.z = solidcraft_engine::doc::apply_vector(&frame, snap.z);
    }
    snap
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

/// Joints after the features in the timeline: double-click edits, the menu drives, suppresses
/// or deletes. Returns the x after them.
pub fn timeline_joints(app: &mut SolveApp, ui: &mut egui::Ui, p: &egui::Painter, mut x: f32, r: egui::Rect) -> f32 {
    let t = Tokens::get();
    let joints: Vec<(u64, String, JointKind, bool)> =
        app.session.doc.assembly.joints.iter().map(|j| (j.id, j.name.clone(), j.kind, j.suppressed)).collect();
    if joints.is_empty() {
        return x;
    }
    x += 8.0;
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
    let t = Tokens::get();
    let joints: Vec<(u64, String, JointKind, bool)> =
        app.session.doc.assembly.joints.iter().map(|joint| (joint.id, joint.name.clone(), joint.kind, joint.suppressed)).collect();
    if joints.is_empty() {
        return;
    }
    ui.add_space(4.0);
    ui.label(egui::RichText::new("JOINTS").size(crate::theme::Tokens::get().font_micro).semibold().color(t.text_dim));
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
