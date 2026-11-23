//! A planned set as an assembly ([`super::Assembly`]): which body turns about which shaft, which
//! is held, and how their motions are tied, worked out per type from the placed bodies.
//!
//! Every member is one of the plan's bodies, in their order (`members[i].body == i`). Its axis is
//! its body's frame z (where the teeth's turn is measured, as in [`crate::mesh3d::motion`]),
//! except where turning it round makes the link a plain gear: a left-hand worm, and a face gear's
//! pinion (pointed at the shaft crossing, as a bevel's axes are at the apex).
//!
//! The links form a tree: each member is driven by one link at most (a planetary ring by planet 1
//! only, a differential's second sun by planet 1 only), as SolidCraft's motion links take them.
//!
//! What the bodies don't hold stays out: a planetary set has no carrier body, so its planets turn
//! on fixed shafts (the carrier held) and the ring turns; a cycloidal drive has no input shaft,
//! so its disc only turns on its own axis and its pin ring is held. An elliptical pair is linked
//! at its mean ratio (1:1): the true ratio changes through the turn.

use super::{Assembly, AssemblyIssue, AssemblyLink, AssemblyMember, Frame, Kind, Plan, Spec};
use crate::axes::Axis;

fn neg(d: [f64; 3]) -> [f64; 3] {
    [-d[0], -d[1], -d[2]]
}

fn turning(f: &Frame) -> Axis {
    Axis { origin: f.origin, dir: f.z }
}

/// A gear's size in a link: its tooth count, so the ratio is the exact tooth ratio.
fn teeth(z: u32) -> f64 {
    f64::from(z.max(1))
}

/// Whether two frames turn about the same line.
fn coaxial(a: &Frame, b: &Frame) -> bool {
    turning(a).coaxial(&turning(b))
}

/// The assembly of the planned set `p` (its bodies placed where they go).
pub(crate) fn fill(s: &Spec, p: &mut Plan) {
    if s.kind == Kind::Differential {
        // Planned with the set (crate::diff): its members follow the placed bodies.
        crate::diff::place_assembly(p);
        return;
    }
    // A set read from a sketch or a selection (its mesh graph and clusters): one assembly over
    // the whole graph.
    if s.only == 0
        && let Some((a, issues)) = from_members(p)
    {
        p.assembly = a;
        p.assembly_issues = issues;
        return;
    }
    let bodies = &p.bodies;
    let n = bodies.len();
    let mut members: Vec<AssemblyMember> =
        bodies.iter().enumerate().map(|(i, b)| AssemblyMember { body: i, axis: turning(&b.frame), grounded: false, carrier: None }).collect();
    let mut links = Vec::new();
    let gear = |a: usize, b: usize, ra: f64, rb: f64, internal: bool| AssemblyLink::Gear { a, b, ra, rb, internal, reverse: false };
    // One member of a set built alone: it turns, linked to nothing.
    let whole = s.only == 0;
    if let Some(t) = &s.train {
        // One member per gear; every mesh and shared shaft of the train is an edge of its graph,
        // made a tree driven by gear 1 (a compound cluster rigid on its first gear's shaft), and
        // a loop whose ratios don't agree is reported per member.
        if whole && n == t.gears.len() {
            let (a, issues) = Assembly::from_graph(members, &super::train::assembly_edges(t), 0);
            p.assembly = a;
            p.assembly_issues = issues;
        } else {
            p.assembly = Assembly { members, links };
        }
        return;
    }
    match s.kind {
        Kind::Rack => {
            // It slides along its length (its frame's x).
            for (m, b) in members.iter_mut().zip(bodies) {
                m.axis = Axis { origin: b.frame.origin, dir: b.frame.x };
            }
        }
        Kind::Pair if whole && n == 2 => links.push(gear(0, 1, teeth(s.teeth), teeth(s.teeth2), false)),
        Kind::Cycloidal if whole && n == 2 => {
            links.push(gear(0, 1, teeth(s.teeth), teeth(s.teeth2), false));
        }
        Kind::Planetary if whole && n >= 2 => {
            // Sun first, the planets, then the ring on the sun's axis. The carrier is held.
            let sun = &bodies[0].frame;
            let ring = (1..n).find(|i| coaxial(sun, &bodies[*i].frame));
            let (rs, rp) = (teeth(s.teeth), teeth(s.teeth2));
            let zr = if s.teeth3 > 0 { s.teeth3 } else { s.teeth + 2 * s.teeth2 };
            let planets: Vec<usize> = (1..n).filter(|i| Some(*i) != ring).collect();
            for &k in &planets {
                links.push(gear(0, k, rs, rp, false));
            }
            if let (Some(r), Some(&first)) = (ring, planets.first()) {
                links.push(gear(first, r, rp, teeth(zr), true));
            }
        }
        Kind::Bevel | Kind::SpiralBevel | Kind::Hypoid | Kind::CrossedHelical if whole && n == 2 => {
            // Turned about the frames' z (a bevel's both point at the apex), as mesh3d checks them.
            let (ratio, _, _) = crate::mesh3d::motion(s);
            let (ra, rb) = (teeth(s.teeth), teeth(s.teeth2));
            links.push(gear(0, 1, ra, rb, ratio > 0.0));
        }
        Kind::Worm if whole && n == 2 => {
            // The wheel first, then the worm. A right-hand worm turned forwards carries the wheel
            // back (mesh3d::motion): the worm's axis is its frame's z for a right hand, turned
            // round for a left hand, so the wheel turns -starts / teeth either way.
            if s.helix < 0.0 {
                members[1].axis.dir = neg(members[1].axis.dir);
            }
            links.push(AssemblyLink::Worm { worm: 1, wheel: 0, starts: s.starts.max(1), wheel_teeth: s.teeth.max(1) });
        }
        Kind::CycloidalDrive => {
            // The disc, then the pin ring: the ring is held, the disc turns on its own axis.
            for m in &mut members {
                m.grounded = if whole { n == 2 && m.body == 1 } else { s.only == 2 };
            }
        }
        Kind::Sprocket | Kind::Pulley if whole && n == 2 => {
            // A chain or belt: both wheels turn the same way, by their tooth counts.
            links.push(gear(0, 1, f64::from(s.teeth), f64::from(s.teeth2), true));
        }
        Kind::Spline if whole && n == 2 => links.push(AssemblyLink::Rigid { a: 0, b: 1 }),
        Kind::FaceGear if whole && n == 2 => {
            // The face gear, then its pinion, whose axis is turned to point at the shaft crossing
            // (as a bevel's): then the pinion drives the face gear as a plain external gear.
            members[1].axis.dir = neg(members[1].axis.dir);
            links.push(gear(1, 0, f64::from(s.teeth), f64::from(s.teeth2), false));
        }
        Kind::Elliptical if whole && n == 2 => {
            // Linked at the mean ratio: the true one changes through the turn.
            links.push(gear(0, 1, f64::from(s.teeth), f64::from(s.teeth), false));
        }
        Kind::BevelPlanetary if whole => {
            // Sun, planets, then a second sun or a ring: every planet driven by the sun, the far
            // member by planet 1, at mesh3d's ratios (every axis points at the apex; the carrier
            // is held).
            let index = |name: &str| bodies.iter().position(|b| b.name == name);
            let mut driven = vec![false; n];
            for m in &p.meshes3d {
                let Some([ba, bb]) = &m.bodies else { continue };
                let (Some(a), Some(b)) = (index(&ba.name), index(&bb.name)) else { continue };
                if driven[b] || m.ratio == 0.0 || !m.ratio.is_finite() {
                    continue;
                }
                driven[b] = true;
                let ra = f64::from(if a == 0 { s.teeth } else { s.teeth2 });
                links.push(gear(a, b, ra, ra / m.ratio.abs(), m.ratio > 0.0));
            }
            // The planets turn on the carrier's pins (when it is built).
            if let Some(c) = bodies.iter().position(|b| b.name.contains(super::bevel_planet::CARRIER)) {
                let sun = &bodies[0].frame;
                for (i, m) in members.iter_mut().enumerate() {
                    if i != 0 && i != c && !coaxial(sun, &bodies[i].frame) {
                        m.carrier = Some(c);
                    }
                }
            }
        }
        _ => {}
    }
    p.assembly = Assembly { members, links };
}

/// The sign of member `b`'s turn per turn of member `a` across a mesh between them, from where
/// their shafts lie: parallel shafts turn the other way across an outside mesh and the same way
/// inside a ring (each about its own axis direction); shafts that meet (bevels) turn the other
/// way when both axes point at the apex, as rolling cones do, and the same way when one points
/// away; shafts that cross without meeting are taken as a pair that turns the other way.
fn mesh_sign(a: &Axis, b: &Axis, internal: bool) -> f64 {
    let dot = |u: [f64; 3], v: [f64; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
    let par = dot(a.dir, b.dir);
    if par.abs() > 1.0 - 1e-9 {
        let own = if internal { 1.0 } else { -1.0 };
        return own * par.signum();
    }
    // Where the shafts come closest (the apex when they meet).
    let w = [a.origin[0] - b.origin[0], a.origin[1] - b.origin[1], a.origin[2] - b.origin[2]];
    let (bb, d, e) = (par, dot(a.dir, w), dot(b.dir, w));
    let den = 1.0 - bb * bb;
    let (s, t) = ((bb * e - d) / den, (e - bb * d) / den);
    // `s` along a from its origin, `t` along b from its: positive when the axis points at the apex.
    let toward = |x: f64| if x >= 0.0 { 1.0 } else { -1.0 };
    -toward(s) * toward(t)
}

/// The assembly of a set read from a sketch or a selection ([`Plan::members`]): a gear link per
/// mesh of its graph (teeth for the ratio, the sign from where the shafts lie: [`mesh_sign`]),
/// the gears of each cluster rigid on its first, made a tree from member 1 with the loops that
/// don't agree reported. `None` when the plan has no such graph (or its gears aren't all built).
pub(crate) fn from_members(p: &Plan) -> Option<(Assembly, Vec<AssemblyIssue>)> {
    if p.members.is_empty() || p.members.iter().all(|m| m.meshes_with.is_empty() && m.cluster.is_none()) {
        return None;
    }
    let body: Vec<usize> = p.members.iter().map(|m| m.body).collect::<Option<Vec<_>>>()?;
    if body.iter().any(|b| *b >= p.bodies.len()) {
        return None;
    }
    let members: Vec<AssemblyMember> =
        p.bodies.iter().enumerate().map(|(i, b)| AssemblyMember { body: i, axis: turning(&b.frame), grounded: false, carrier: None }).collect();
    let mut edges = Vec::new();
    for (i, m) in p.members.iter().enumerate() {
        for &j in m.meshes_with.iter().filter(|j| **j > i && **j < p.members.len()) {
            let n = &p.members[j];
            let (a, b) = (body[i], body[j]);
            let internal = m.ring != n.ring;
            let sign = mesh_sign(&members[a].axis, &members[b].axis, internal);
            let (ra, rb) = (f64::from(m.teeth.max(1)), f64::from(n.teeth.max(1)));
            // `joint.gear`'s rule: b turns −ra/rb, the same way when internal.
            edges.push(AssemblyLink::Gear { a, b, ra, rb, internal: sign > 0.0, reverse: false });
        }
    }
    for c in &p.clusters {
        if let Some((&first, rest)) = c.split_first() {
            for &k in rest {
                if first < body.len() && k < body.len() {
                    edges.push(AssemblyLink::Rigid { a: body[first], b: body[k] });
                }
            }
        }
    }
    Some(Assembly::from_graph(members, &edges, 0))
}
