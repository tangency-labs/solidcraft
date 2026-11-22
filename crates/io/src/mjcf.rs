//! MJCF, MuJoCo's own model format, of a design's components and joints (#20786, docs/mjcf.md).
//! It shares the URDF export's tree walk ([`crate::urdf`]): its links, inertials and mesh files.
//! What MJCF adds over URDF: several joints in one body, so a joint with several values needs no
//! in-between links; a `ball` joint; no limit where a joint has none; and `<equality>`
//! constraints, so a joint that closes a loop is kept rather than left out.
//!
//! Every body is written where the design places it now, and each hinge and slide's `ref` is its
//! current value, so MuJoCo's initial configuration (`qpos0`) is the design as it is and a loop's
//! constraint holds there. A body with several joints keeps the URDF chain's motion: joint `k`'s
//! anchor and axis are those of the chain's step `k` seen from the body frame at the current
//! values, `T_k⁻¹` with `T_k` the motion of steps `k..n`.

use std::collections::BTreeMap;

use solidcore_record::Vec3;
use solidcraft_doc::joints::{self, JointKind};
use solidcraft_doc::{Document, IDENTITY, Mat, ModelState, apply_point, mat_inverse, mat_mul};

use crate::Result;
use crate::urdf::{Link, Note, Step, UrdfJoint, UrdfModel, attr, num, rotation, sanitise, translation, triple, walk};

/// The distance between the two points a hinge that closes a loop is held at (mm): two
/// `connect` constraints on its axis leave it the one turn a hinge has.
const LOOP_HINGE_SPAN_MM: f64 = 10.0;

/// The MJCF, its mesh files (name next to the `.xml`, bytes) and what was approximated.
#[derive(Clone, Debug)]
pub struct MjcfPackage {
    pub xml: String,
    pub meshes: Vec<(String, Vec<u8>)>,
    pub model: UrdfModel,
    pub warnings: Vec<String>,
}

/// Build the MJCF of a design: `doc` with its occurrences placed (pending moves applied), `st`
/// with the bodies in their components' own frames, painted. `stem` names the model and
/// prefixes the mesh files.
pub fn mjcf(doc: &Document, st: &ModelState, stem: &str) -> Result<MjcfPackage> {
    let walked = walk(doc, st, stem, &[])?;
    let values = joints::joint_values(&doc.assembly);
    let value = |j: &UrdfJoint| j.source.and_then(|(id, k)| values.get(&id)?.get(k).copied()).unwrap_or(0.0);
    let all = doc.joints_with_rigid_groups();
    let source: BTreeMap<u64, (JointKind, bool)> = all.iter().map(|j| (j.id, (j.kind, j.limits.iter().any(Option::is_some)))).collect();
    let mut warnings = Vec::new();
    let mut equality = String::new();
    let model = &walked.model;
    let body_name = |i: usize| if i == 0 { "world".to_string() } else { model.links.get(i).map(|l| l.name.clone()).unwrap_or_default() };
    for note in &walked.notes {
        match note {
            Note::Text(t) => warnings.push(t.clone()),
            Note::Unlimited { .. } => {}
            Note::Relation { joint, kind } => {
                warnings.push(format!("joint `{joint}` is a {kind:?} relation, which MJCF has no joint for: written fixed as it is"))
            }
            Note::JointedCopy { link } => {
                warnings.push(format!("`{link}` is a copy of a jointed occurrence: fixed where it is (the MJCF moves only the first placement)"))
            }
            Note::Loop { joint, kind, a, b, frame } => {
                let (body1, body2) = (attr(&body_name(*a)), attr(&body_name(*b)));
                let name = attr(&sanitise(joint, false));
                let at = |z: f64| triple(apply_point(frame, Vec3::new(0.0, 0.0, z)) * 1e-3);
                match kind {
                    JointKind::Rigid => equality += &format!("    <weld name=\"{name}\" body1=\"{body1}\" body2=\"{body2}\"/>\n"),
                    JointKind::Ball => {
                        equality += &format!("    <connect name=\"{name}\" body1=\"{body1}\" body2=\"{body2}\" anchor=\"{}\"/>\n", at(0.0))
                    }
                    JointKind::Revolute => {
                        for (k, z) in [(1, 0.0), (2, LOOP_HINGE_SPAN_MM)] {
                            equality += &format!("    <connect name=\"{name}_{k}\" body1=\"{body1}\" body2=\"{body2}\" anchor=\"{}\"/>\n", at(z));
                        }
                    }
                    _ => warnings
                        .push(format!("joint `{joint}` closes a loop as a {kind:?} joint, which an MJCF equality constraint can't hold: left out")),
                }
            }
        }
    }
    let frames = model.link_frames(value);
    let mut children: BTreeMap<usize, Vec<&UrdfJoint>> = BTreeMap::new();
    for j in &model.joints {
        children.entry(j.parent).or_default().push(j);
    }
    let mut w = Writer { model, frames: &frames, children: &children, value: &value, source: &source, warnings: &mut warnings, xml: String::new() };
    let name = sanitise(stem, false);
    w.xml += "<?xml version=\"1.0\"?>\n<!-- Exported by SolidCraft; units: m, kg, rad -->\n";
    w.xml += &format!("<mujoco model=\"{}\">\n  <compiler angle=\"radian\" autolimits=\"true\"/>\n", attr(&name));
    w.xml += "  <asset>\n";
    for (file, _) in walked.meshes.iter().filter(|(f, _)| f.ends_with(".stl")) {
        w.xml += &format!("    <mesh name=\"{}\" file=\"{}\"/>\n", attr(mesh_name(file)), attr(file));
    }
    w.xml += "  </asset>\n  <worldbody>\n";
    if let Some(base) = model.links.first() {
        w.geoms(base, 4);
    }
    w.bodies(0, 4);
    w.xml += "  </worldbody>\n";
    if !equality.is_empty() {
        w.xml += &format!("  <equality>\n{equality}  </equality>\n");
    }
    w.xml += "</mujoco>\n";
    let xml = std::mem::take(&mut w.xml);
    let meshes = walked.meshes.into_iter().filter(|(f, _)| f.ends_with(".stl")).collect();
    Ok(MjcfPackage { xml, meshes, model: walked.model, warnings })
}

fn mesh_name(file: &str) -> &str {
    file.strip_suffix(".stl").unwrap_or(file)
}

/// A link that is a step inside one joint's chain, not a placement.
fn in_between(model: &UrdfModel, i: usize) -> bool {
    i != 0 && model.links.get(i).is_some_and(|l| l.path.is_empty())
}

fn step_motion(j: &UrdfJoint, q: f64) -> Mat {
    match j.step {
        Step::Fixed => IDENTITY,
        Step::Revolute => rotation(j.axis, q),
        Step::Prismatic => translation(j.axis * q),
    }
}

/// `pos` (m) and `quat` (w x y z) of a frame.
fn pos_quat(m: &Mat) -> String {
    let r = |row: usize, col: usize| m.get(col).and_then(|c| c.get(row)).copied().unwrap_or(0.0);
    let (m00, m11, m22) = (r(0, 0), r(1, 1), r(2, 2));
    let trace = m00 + m11 + m22;
    // Shepperd: divide by the largest of the four, so no case loses precision.
    let q = if trace > m00.max(m11).max(m22) {
        let s = (1.0 + trace).sqrt() * 2.0;
        [0.25 * s, (r(2, 1) - r(1, 2)) / s, (r(0, 2) - r(2, 0)) / s, (r(1, 0) - r(0, 1)) / s]
    } else if m00 >= m11 && m00 >= m22 {
        let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        [(r(2, 1) - r(1, 2)) / s, 0.25 * s, (r(0, 1) + r(1, 0)) / s, (r(0, 2) + r(2, 0)) / s]
    } else if m11 >= m22 {
        let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        [(r(0, 2) - r(2, 0)) / s, (r(0, 1) + r(1, 0)) / s, 0.25 * s, (r(1, 2) + r(2, 1)) / s]
    } else {
        let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        [(r(1, 0) - r(0, 1)) / s, (r(0, 2) + r(2, 0)) / s, (r(1, 2) + r(2, 1)) / s, 0.25 * s]
    };
    let p = apply_point(m, Vec3::ZERO) * 1e-3;
    format!("pos=\"{}\" quat=\"{} {} {} {}\"", triple(p), num(q[0]), num(q[1]), num(q[2]), num(q[3]))
}

struct Writer<'a, V: Fn(&UrdfJoint) -> f64> {
    model: &'a UrdfModel,
    /// Every link's world frame at the design's current joint values (mm).
    frames: &'a [Mat],
    children: &'a BTreeMap<usize, Vec<&'a UrdfJoint>>,
    value: &'a V,
    source: &'a BTreeMap<u64, (JointKind, bool)>,
    warnings: &'a mut Vec<String>,
    xml: String,
}

impl<V: Fn(&UrdfJoint) -> f64> Writer<'_, V> {
    /// The bodies that hang on link `parent`, each with the steps of the joint that moves it.
    fn bodies(&mut self, parent: usize, indent: usize) {
        let joints: Vec<&UrdfJoint> = self.children.get(&parent).cloned().unwrap_or_default();
        for first in joints {
            // A chain's in-between links fold into the link at its end.
            let mut chain = vec![first];
            let mut child = first.child;
            while in_between(self.model, child) {
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
        let Some(link) = self.model.links.get(child) else { return };
        let (Some(pf), Some(cf)) = (self.frames.get(parent), self.frames.get(child)) else { return };
        let Some(rel) = mat_inverse(pf).map(|inv| mat_mul(&inv, cf)) else { return };
        let pad = " ".repeat(indent);
        self.xml += &format!("{pad}<body name=\"{}\" {}>\n", attr(&link.name), pos_quat(&rel));
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
        self.bodies(child, indent + 2);
        self.xml += &format!("{pad}</body>\n");
    }

    /// The chain's moving steps as joints of one body written at the current values.
    fn joints(&mut self, chain: &[&UrdfJoint], pad: &str) {
        let moving: Vec<&UrdfJoint> = chain.iter().copied().filter(|j| j.step != Step::Fixed).collect();
        let Some(first) = moving.first() else { return };
        let ball = first.source.and_then(|(id, _)| self.source.get(&id).copied());
        if let Some((JointKind::Ball, limited)) = ball
            && moving.iter().all(|j| j.step == Step::Revolute && j.source.map(|s| s.0) == first.source.map(|s| s.0))
        {
            let name = chain.last().map(|j| j.name.as_str()).unwrap_or_default();
            if limited {
                self.warnings.push(format!("ball joint `{name}`: its limits are not written (an MJCF ball joint limits one cone angle)"));
            }
            self.xml += &format!("{pad}  <joint name=\"{}\" type=\"ball\"/>\n", attr(name));
            return;
        }
        for (k, j) in moving.iter().enumerate() {
            // T_k: steps k..n at their current values; joint k sits where step k does, seen from the body.
            let tail = moving[k..].iter().fold(IDENTITY, |m, s| mat_mul(&m, &step_motion(s, (self.value)(s))));
            let Some(inv) = mat_inverse(&tail) else { continue };
            let pos = apply_point(&inv, Vec3::ZERO) * 1e-3;
            let axis = apply_point(&inv, j.axis) - apply_point(&inv, Vec3::ZERO);
            let (kind, scale) = if j.step == Step::Prismatic { ("slide", 1e-3) } else { ("hinge", 1.0) };
            let range = match j.limit {
                Some((lo, hi)) => format!(" range=\"{} {}\"", num(lo * scale), num(hi * scale)),
                None => String::new(),
            };
            self.xml += &format!(
                "{pad}  <joint name=\"{}\" type=\"{kind}\" pos=\"{}\" axis=\"{}\" ref=\"{}\"{range}/>\n",
                attr(&j.name),
                triple(pos),
                triple(axis),
                num((self.value)(j) * scale)
            );
        }
    }

    /// A link's mesh: one geom that is drawn and collides, or a drawn one and a collision one.
    fn geoms(&mut self, link: &Link, indent: usize) {
        let pad = " ".repeat(indent);
        let at = pos_quat(&link.mesh_origin);
        let rgba = link.color.map(|[r, g, b, a]| format!(" rgba=\"{} {} {} {}\"", num(r as f64), num(g as f64), num(b as f64), num(a as f64)));
        let rgba = rgba.unwrap_or_default();
        match (&link.visual, &link.collision) {
            (Some(v), Some(c)) if v == c => {
                self.xml += &format!("{pad}<geom type=\"mesh\" mesh=\"{}\" {at}{rgba}/>\n", attr(mesh_name(v)));
            }
            (visual, collision) => {
                if let Some(v) = visual {
                    self.xml += &format!("{pad}<geom type=\"mesh\" mesh=\"{}\" {at}{rgba} contype=\"0\" conaffinity=\"0\"/>\n", attr(mesh_name(v)));
                }
                if let Some(c) = collision {
                    self.xml += &format!("{pad}<geom type=\"mesh\" mesh=\"{}\" {at} group=\"3\"/>\n", attr(mesh_name(c)));
                }
            }
        }
    }
}
