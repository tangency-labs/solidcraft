//! MJCF, MuJoCo's own model format, of a design's components and joints (#20786, docs/mjcf.md).
//! It shares the URDF export's tree walk ([`crate::urdf`]): its links, inertials, collision shapes
//! and mesh files. What MJCF adds over URDF: several joints in one body, so a joint with several
//! values needs no in-between links; a `ball` joint; no limit where a joint has none; and
//! `<equality>` constraints, so a joint that closes a loop is kept rather than left out.
//!
//! Every body is written where the design places it now, and each hinge and slide's `ref` is its
//! current value, so MuJoCo's initial configuration (`qpos0`) is the design as it is and a loop's
//! constraint holds there. A body with several joints keeps the URDF chain's motion: joint `k`'s
//! anchor and axis are those of the chain's step `k` seen from the body frame at the current
//! values, `T_k⁻¹` with `T_k` the motion of steps `k..n`.

use std::collections::{BTreeMap, BTreeSet};

use solidcore_record::Vec3;
use solidcraft_doc::joints::{self, JointKind};
use solidcraft_doc::{Document, IDENTITY, Mat, ModelState, apply_point, mat_inverse, mat_mul};

use crate::Result;
use crate::urdf::{
    CollisionGeom, Link, Names, Note, Step, UrdfJoint, UrdfModel, UrdfOptions, attr, num, resolve_mimics, rotation, sanitise, translation, triple,
    walk,
};

/// The distance between the two points a hinge that closes a loop is held at (mm): two
/// `connect` constraints on its axis leave it the one turn a hinge has.
const LOOP_HINGE_SPAN_MM: f64 = 10.0;
/// A coupling's constant below this (m or rad) is rounding: the design holds the link exactly.
const COUPLING_NOISE: f64 = 1e-12;

/// The MJCF, its mesh files (name next to the `.xml`, bytes) and what was approximated.
#[derive(Clone, Debug)]
pub struct MjcfPackage {
    pub xml: String,
    pub meshes: Vec<(String, Vec<u8>)>,
    /// What the MJCF holds: bodies (not counting the world), joints and equality constraints.
    pub bodies: usize,
    pub joints: usize,
    pub equalities: usize,
    pub warnings: Vec<String>,
}

/// Build the MJCF of a design: `doc` with its occurrences placed (pending moves applied), `st`
/// with the bodies in their components' own frames, painted. `stem` names the model and
/// prefixes the mesh files.
pub fn mjcf(doc: &Document, st: &ModelState, stem: &str) -> Result<MjcfPackage> {
    let active = doc.active_assembly();
    let doc = &*active;
    let mut walked = walk(doc, st, stem, &UrdfOptions::default())?;
    let values = joints::joint_values(&doc.assembly);
    let all = doc.joints_with_rigid_groups();
    let mut balls = BTreeSet::new();
    let mut limited = BTreeSet::new();
    for j in all.iter() {
        if j.kind == JointKind::Ball {
            balls.insert(j.id);
        }
        let finite = |l: &Option<(f64, f64)>| l.is_some_and(|(lo, hi)| lo.is_finite() && hi.is_finite());
        limited.extend(j.limits.iter().enumerate().filter(|(_, l)| finite(l)).map(|(dof, _)| (j.id, dof)));
    }
    // URDF's stand-in for an unlimited slide is not one of the design's limits.
    for j in &mut walked.model.joints {
        j.limit = j.limit.filter(|_| j.source.is_some_and(|s| limited.contains(&s)));
    }
    let mut warnings = Vec::new();
    resolve_mimics(&mut walked.model, doc, "MJCF", &mut warnings);
    let mut children: BTreeMap<usize, Vec<&UrdfJoint>> = BTreeMap::new();
    for j in &walked.model.joints {
        children.entry(j.parent).or_default().push(j);
    }
    let value = |j: &UrdfJoint| j.source.and_then(|(id, k)| values.get(&id)?.get(k).copied()).unwrap_or(0.0);
    let mut w = Writer {
        model: &walked.model,
        frames: walked.model.link_frames(value),
        children,
        value,
        balls,
        names: body_names(&walked.model),
        constraint_names: Names::default(),
        equality: String::new(),
        warnings,
        xml: String::new(),
        bodies: 0,
        joints: 0,
        equalities: 0,
    };
    for note in &walked.notes {
        w.note(note);
    }
    w.xml += "<?xml version=\"1.0\"?>\n<!-- Exported by SolidCraft; units: m, kg, rad -->\n";
    w.xml += &format!("<mujoco model=\"{}\">\n  <compiler angle=\"radian\" autolimits=\"true\"/>\n  <asset>\n", attr(&sanitise(stem, false)));
    for (file, _) in &walked.meshes {
        w.xml += &format!("    <mesh name=\"{}\" file=\"{}\"/>\n", attr(mesh_name(file)), attr(file));
    }
    w.xml += "  </asset>\n  <worldbody>\n";
    if let Some(base) = walked.model.links.first() {
        w.geoms(base, 4);
    }
    w.hang(0, 4);
    w.xml += "  </worldbody>\n";
    w.couplings();
    if !w.equality.is_empty() {
        w.xml += &format!("  <equality>\n{}  </equality>\n", w.equality);
    }
    w.xml += "</mujoco>\n";
    let Writer { xml, bodies, joints, equalities, warnings, .. } = w;
    Ok(MjcfPackage { xml, meshes: walked.meshes, bodies, joints, equalities, warnings })
}

/// Each link's body name: the link's, unless it is MuJoCo's own `world`.
fn body_names(model: &UrdfModel) -> Vec<String> {
    let mut names = Names::default();
    let world = names.take("world");
    for l in model.links.iter().filter(|l| l.name != world) {
        names.take(&l.name);
    }
    let renamed = model.links.iter().enumerate().map(|(i, l)| match i {
        0 => world.clone(),
        _ if l.name == world => names.take(&l.name),
        _ => l.name.clone(),
    });
    renamed.collect()
}

fn mesh_name(file: &str) -> &str {
    file.strip_suffix(".stl").unwrap_or(file)
}

/// A link that is a step inside one joint's chain, not a placement.
fn in_between(l: &Link) -> bool {
    l.path.is_empty() && l.inertial.is_some()
}

/// An origin flagged as a frame, not a placement.
fn is_frame(l: &Link) -> bool {
    l.path.is_empty() && l.inertial.is_none()
}

fn step_motion(j: &UrdfJoint, q: f64) -> Mat {
    match j.step {
        Step::Fixed => IDENTITY,
        Step::Revolute => rotation(j.axis, q),
        Step::Prismatic => translation(j.axis * q),
    }
}

/// Stored units (mm, rad) to MJCF's (m, rad).
fn scale(j: &UrdfJoint) -> f64 {
    if j.step == Step::Prismatic { 1e-3 } else { 1.0 }
}

/// A frame as `pos` (m) and `xyaxes`, its x and y axes.
fn pose(m: &Mat) -> String {
    let axis = |col: usize| m.get(col).map(|c| triple(Vec3::new(c[0], c[1], c[2]))).unwrap_or_default();
    format!("pos=\"{}\" xyaxes=\"{} {}\"", triple(apply_point(m, Vec3::ZERO) * 1e-3), axis(0), axis(1))
}

struct Writer<'a, V: Fn(&UrdfJoint) -> f64> {
    model: &'a UrdfModel,
    /// Every link's world frame at the design's current joint values (mm).
    frames: Vec<Mat>,
    children: BTreeMap<usize, Vec<&'a UrdfJoint>>,
    value: V,
    /// The design's ball joints.
    balls: BTreeSet<u64>,
    /// Each link's body name.
    names: Vec<String>,
    constraint_names: Names,
    equality: String,
    warnings: Vec<String>,
    xml: String,
    bodies: usize,
    joints: usize,
    equalities: usize,
}

impl<V: Fn(&UrdfJoint) -> f64> Writer<'_, V> {
    fn note(&mut self, note: &Note) {
        match note {
            Note::Text(t) => self.warnings.push(t.clone()),
            Note::Unlimited { .. } => {}
            Note::Relation { joint, kind } => {
                self.warnings.push(format!("joint `{joint}` is a {kind:?} relation, which MJCF has no joint for: written fixed as it is"))
            }
            Note::JointedCopy { link } => {
                self.warnings.push(format!("`{link}` is a copy of a jointed occurrence: fixed where it is (the MJCF moves only the first placement)"))
            }
            Note::Loop { joint, kind, a, b, frame } => {
                let body = |i: usize| self.names.get(i).map(|n| attr(n)).unwrap_or_default();
                let bodies = format!("body1=\"{}\" body2=\"{}\"", body(*a), body(*b));
                let connect = |z: f64| format!("{bodies} anchor=\"{}\"", triple(apply_point(frame, Vec3::new(0.0, 0.0, z)) * 1e-3));
                match kind {
                    JointKind::Rigid => self.constraint("weld", joint, &bodies),
                    JointKind::Ball => self.constraint("connect", joint, &connect(0.0)),
                    JointKind::Revolute => {
                        let (near, far) = (connect(0.0), connect(LOOP_HINGE_SPAN_MM));
                        self.constraint("connect", &format!("{joint}_1"), &near);
                        self.constraint("connect", &format!("{joint}_2"), &far);
                    }
                    _ => self
                        .warnings
                        .push(format!("joint `{joint}` closes a loop as a {kind:?} joint, which an MJCF equality constraint can't hold: left out")),
                }
            }
        }
    }

    fn constraint(&mut self, element: &str, name: &str, attributes: &str) {
        let name = self.constraint_names.take(name);
        self.equality += &format!("    <{element} name=\"{}\" {attributes}/>\n", attr(&name));
        self.equalities += 1;
    }

    fn is_ball(&self, j: &UrdfJoint) -> bool {
        j.source.is_some_and(|(id, _)| self.balls.contains(&id))
    }

    /// Motion links, and joints whose limits leave them one value. A `joint` constraint holds
    /// `y - y₀ = a₀ + a₁ (x - x₀)`, where `x₀` and `y₀` are the joints' `ref`. MuJoCo sets how
    /// stiff it is without `a₁`: below 1 (a rack's 0.005 m per radian) the constraint is so soft
    /// that the joints come apart, so it is written the other way round.
    fn couplings(&mut self) {
        let model = self.model;
        for f in &model.joints {
            let y0 = (self.value)(f) * scale(f);
            if let Some((lo, _)) = f.limit.filter(|(lo, hi)| lo == hi && !self.is_ball(f)) {
                let held = format!("joint1=\"{}\" polycoef=\"{} 0 0 0 0\"", attr(&f.name), num(lo * scale(f) - y0));
                self.constraint("joint", &format!("{}_limit", f.name), &held);
            }
            let Some((m, d)) = f.mimic.and_then(|m| Some((m, model.joints.get(m.joint)?))) else { continue };
            if self.is_ball(f) || self.is_ball(d) {
                self.warnings.push(format!("the motion link to `{}` is left out: an MJCF ball joint has no values to couple", f.name));
                continue;
            }
            let a0 = m.multiplier * (self.value)(d) * scale(d) + m.offset - y0;
            let a0 = if a0.abs() < COUPLING_NOISE { 0.0 } else { a0 };
            let a1 = m.multiplier;
            let (y, x, a0, a1) = if a1 != 0.0 && a1.abs() < 1.0 { (d, f, -a0 / a1, 1.0 / a1) } else { (f, d, a0, a1) };
            let coupled = format!("joint1=\"{}\" joint2=\"{}\" polycoef=\"{} {} 0 0 0\"", attr(&y.name), attr(&x.name), num(a0), num(a1));
            self.constraint("joint", &f.name, &coupled);
        }
    }

    /// What hangs on link `parent`: its frames, and bodies with the steps of the joint each is on.
    fn hang(&mut self, parent: usize, indent: usize) {
        let pad = " ".repeat(indent);
        for first in self.children.get(&parent).cloned().unwrap_or_default() {
            if let Some(l) = self.model.links.get(first.child).filter(|l| is_frame(l)) {
                self.xml += &format!("{pad}<site name=\"{}\" {}/>\n", attr(&l.name), pose(&first.origin));
                continue;
            }
            // A chain's in-between links fold into the link at its end.
            let mut chain = vec![first];
            let mut child = first.child;
            while self.model.links.get(child).is_some_and(in_between) {
                let next = match self.children.get(&child).map(Vec::as_slice) {
                    Some([next]) if next.origin == IDENTITY => *next,
                    _ => break,
                };
                chain.push(next);
                child = next.child;
            }
            self.body(parent, &chain, child, indent);
        }
    }

    fn body(&mut self, parent: usize, chain: &[&UrdfJoint], child: usize, indent: usize) {
        let (Some(link), Some(name)) = (self.model.links.get(child), self.names.get(child)) else { return };
        let (Some(pf), Some(cf)) = (self.frames.get(parent), self.frames.get(child)) else { return };
        let Some(rel) = mat_inverse(pf).map(|inv| mat_mul(&inv, cf)) else { return };
        let pad = " ".repeat(indent);
        self.xml += &format!("{pad}<body name=\"{}\" {}>\n", attr(name), pose(&rel));
        self.bodies += 1;
        if let Some(i) = &link.inertial {
            let t = &i.inertia;
            self.xml += &format!(
                "{pad}  <inertial pos=\"{}\" mass=\"{}\" fullinertia=\"{} {} {} {} {} {}\"/>\n",
                triple(i.com * 1e-3),
                num(i.mass),
                num(t[0][0]),
                num(t[1][1]),
                num(t[2][2]),
                num(t[0][1]),
                num(t[0][2]),
                num(t[1][2])
            );
        }
        self.joints(chain, &pad);
        self.geoms(link, indent + 2);
        self.hang(child, indent + 2);
        self.xml += &format!("{pad}</body>\n");
    }

    /// The chain's moving steps as joints of one body written at the current values.
    fn joints(&mut self, chain: &[&UrdfJoint], pad: &str) {
        let moving: Vec<&UrdfJoint> = chain.iter().copied().filter(|j| j.step != Step::Fixed).collect();
        let Some(last) = moving.last() else { return };
        self.joints += if self.is_ball(last) { 1 } else { moving.len() };
        if self.is_ball(last) {
            if moving.iter().any(|j| j.limit.is_some()) {
                self.warnings.push(format!("ball joint `{}`: its limits are not written (an MJCF ball joint limits one cone angle)", last.name));
            }
            let dynamics = passive(last);
            if moving.iter().any(|j| passive(j) != dynamics) {
                self.warnings.push(format!(
                    "ball joint `{}`: its damping and friction are those of its last value (an MJCF ball joint has one of each)",
                    last.name
                ));
            }
            self.xml += &format!("{pad}  <joint name=\"{}\" type=\"ball\"{dynamics}/>\n", attr(&last.name));
            return;
        }
        // T_k: steps k..n at their current values; joint k sits where step k does, seen from the body.
        let mut tail = IDENTITY;
        let mut placed = Vec::new();
        for j in moving.iter().rev() {
            tail = mat_mul(&step_motion(j, (self.value)(j)), &tail);
            let Some(inv) = mat_inverse(&tail) else { return };
            placed.push((*j, inv));
        }
        for (j, inv) in placed.into_iter().rev() {
            let pos = apply_point(&inv, Vec3::ZERO);
            let axis = apply_point(&inv, j.axis) - pos;
            let kind = if j.step == Step::Prismatic { "slide" } else { "hinge" };
            // MuJoCo takes no range of one value: `couplings` holds the joint there.
            let range = match j.limit.filter(|(lo, hi)| lo < hi) {
                Some((lo, hi)) => format!(" range=\"{} {}\"", num(lo * scale(j)), num(hi * scale(j))),
                None => String::new(),
            };
            self.xml += &format!(
                "{pad}  <joint name=\"{}\" type=\"{kind}\" pos=\"{}\" axis=\"{}\" ref=\"{}\"{range}{}/>\n",
                attr(&j.name),
                triple(pos * 1e-3),
                triple(axis),
                num((self.value)(j) * scale(j)),
                passive(j)
            );
        }
    }

    /// A link's mesh, drawn and colliding as one geom, or drawn by one and colliding by others.
    fn geoms(&mut self, link: &Link, indent: usize) {
        let pad = " ".repeat(indent);
        let at = pose(&link.mesh_origin);
        let rgba = link.color.map(|[r, g, b, a]| format!(" rgba=\"{} {} {} {}\"", num(r as f64), num(g as f64), num(b as f64), num(a as f64)));
        let rgba = rgba.unwrap_or_default();
        let as_drawn = match (&link.visual, link.collision.as_slice()) {
            (Some(v), [CollisionGeom::Mesh { file, .. }]) => v == file,
            _ => false,
        };
        if let Some(v) = &link.visual {
            let drawn_only = if as_drawn { "" } else { " contype=\"0\" conaffinity=\"0\"" };
            self.xml += &format!("{pad}<geom type=\"mesh\" mesh=\"{}\" {at}{rgba}{drawn_only}/>\n", attr(mesh_name(v)));
        }
        for g in link.collision.iter().filter(|_| !as_drawn) {
            let shape = match g {
                CollisionGeom::Mesh { file, origin } => format!("type=\"mesh\" mesh=\"{}\" {}", attr(mesh_name(file)), pose(origin)),
                CollisionGeom::Box { origin, size } => format!("type=\"box\" size=\"{}\" {}", triple(*size * 0.5e-3), pose(origin)),
                CollisionGeom::Cylinder { origin, radius, length } => {
                    format!("type=\"cylinder\" size=\"{} {}\" {}", num(radius * 1e-3), num(length * 0.5e-3), pose(origin))
                }
                CollisionGeom::Sphere { origin, radius } => format!("type=\"sphere\" size=\"{}\" {}", num(radius * 1e-3), pose(origin)),
            };
            self.xml += &format!("{pad}<geom {shape} group=\"3\"/>\n");
        }
    }
}

/// A joint's saved damping and dry friction as attributes.
fn passive(j: &UrdfJoint) -> String {
    let written = |name: &str, x: Option<f64>| x.map(|x| format!(" {name}=\"{}\"", num(x))).unwrap_or_default();
    written("damping", j.dynamics.damping) + &written("frictionloss", j.dynamics.friction)
}
