//! How a [`Plan`] becomes solids, for every builder of it (the engine's timeline features and the
//! design's own Gears feature, `FeatureKind::Gears`): which bodies are one rigid part, the
//! tries a step gets when the kernel refuses it, and the herringbone half that is a mirror.
//!
//! A rigid member is what turns as one: a gear on its own, or a compound cluster (gears sharing a
//! shaft, [`Plan::clusters`]) ([`members`]). Its assembly
//! is the plan's with each cluster's members made one ([`assembly`]): one revolute per shaft and
//! no rigid links between gears that are one body.

use super::{Assembly, AssemblyLink, AssemblyMember, Body, Op, Plan, Step};
use crate::geom::{Region, V2};

/// One rigid member of a planned set: the plan's bodies (indices into [`Plan::bodies`]) that are
/// one part, unioned in order. A train plans a cluster's gears and its shaft as one body
/// already; a plan that keeps them apart has them joined here.
#[derive(Clone, Debug, PartialEq)]
pub struct Rigid {
    pub name: String,
    pub bodies: Vec<usize>,
}

/// The plan's rigid members, in the order of their first bodies: every body that is no
/// cluster's its own; a cluster's bodies one member.
pub fn members(p: &Plan) -> Vec<Rigid> {
    let cluster_of = |b: usize| p.members.iter().find(|m| m.body == Some(b)).and_then(|m| m.cluster);
    let mut done = vec![false; p.bodies.len()];
    let mut out = Vec::new();
    for b in 0..p.bodies.len() {
        if done[b] {
            continue;
        }
        let bodies: Vec<usize> = match cluster_of(b).and_then(|c| p.clusters.get(c)) {
            Some(list) => {
                let mut v: Vec<usize> = list.iter().filter_map(|m| p.members.get(*m).and_then(|m| m.body)).filter(|k| *k < p.bodies.len()).collect();
                v.sort_unstable();
                v.dedup();
                if !v.contains(&b) {
                    v.insert(0, b);
                }
                v
            }
            None => vec![b],
        };
        for k in &bodies {
            done[*k] = true;
        }
        let name = bodies.iter().map(|k| p.bodies[*k].name.as_str()).collect::<Vec<_>>().join(" + ");
        out.push(Rigid { name, bodies });
    }
    out
}

/// The plan's assembly with each rigid member's members made one: a member's `body` is its rigid
/// member's index in `rigid`, links inside one rigid member are dropped (they are one body now)
/// and every other link and carrier points at the member standing for its rigid member (the
/// first of them). Turns and ratios are the plan's: a cluster turns as one.
pub fn assembly(p: &Plan, rigid: &[Rigid]) -> Assembly {
    let rigid_of = |body: usize| rigid.iter().position(|r| r.bodies.contains(&body));
    let a = &p.assembly;
    // Old member -> new member.
    let mut map: Vec<Option<usize>> = vec![None; a.members.len()];
    let mut members: Vec<AssemblyMember> = Vec::new();
    let mut stands: Vec<usize> = Vec::new();
    for (i, m) in a.members.iter().enumerate() {
        let Some(r) = rigid_of(m.body) else { continue };
        match stands.iter().position(|x| *x == r) {
            Some(k) => {
                map[i] = Some(k);
                // A cluster held anywhere is held.
                members[k].grounded |= m.grounded;
            }
            None => {
                map[i] = Some(members.len());
                stands.push(r);
                members.push(AssemblyMember { body: r, ..m.clone() });
            }
        }
    }
    for m in &mut members {
        m.carrier = m.carrier.and_then(|c| map.get(c).copied().flatten());
    }
    let to = |i: usize| map.get(i).copied().flatten();
    let links = a
        .links
        .iter()
        .filter_map(|l| {
            let (x, y) = l.members();
            let (nx, ny) = (to(x)?, to(y)?);
            if nx == ny {
                return None;
            }
            Some(match l.clone() {
                AssemblyLink::Gear { ra, rb, internal, reverse, .. } => AssemblyLink::Gear { a: nx, b: ny, ra, rb, internal, reverse },
                AssemblyLink::RackPinion { pitch_radius, .. } => AssemblyLink::RackPinion { pinion: nx, rack: ny, pitch_radius },
                AssemblyLink::Worm { starts, wheel_teeth, .. } => AssemblyLink::Worm { worm: nx, wheel: ny, starts, wheel_teeth },
                AssemblyLink::Rigid { .. } => AssemblyLink::Rigid { a: nx, b: ny },
            })
        })
        .collect();
    // A carrier inside its own member is none.
    for (k, m) in members.iter_mut().enumerate() {
        if m.carrier == Some(k) {
            m.carrier = None;
        }
    }
    Assembly { members, links }
}

/// How far along its axis a trimming blank (a bevel's back and front cones) is tried: the kernel
/// sometimes refuses the intersection where the cones graze a root fillet, and takes it a hair
/// further along.
pub const NUDGES: [f64; 4] = [0.0, 0.0137, -0.0213, 0.0391];

/// Whether a step trims a body made already (a revolved blank cut or intersected): tried at each
/// of [`NUDGES`].
pub fn trims(step: &Step) -> bool {
    matches!(step, Step::Revolve { op, .. } if *op != Op::New)
}

/// What a step does to its body.
pub fn op_of(step: &Step) -> Op {
    step.op()
}

/// The tries a step gets, in order, each with the blank's move along the axis: a trimming blank
/// at each of [`NUDGES`]; a loft of a few sections (up to 12) through all of them, then every
/// other one, then its two ends (a coarser twist beats no gear). A loft of many sections (a
/// hypoid's) is tried once: its sections are as many as the kernel needs to line them up tooth to
/// tooth.
pub fn attempts(step: &Step) -> Vec<(Step, f64)> {
    let mut out: Vec<(Step, f64)> = if trims(step) { NUDGES.iter().map(|n| (step.clone(), *n)).collect() } else { vec![(step.clone(), 0.0)] };
    if let Step::Loft { sections, op } = step
        && sections.len() <= 12
    {
        let n = sections.len();
        if n > 3 {
            let mut every_other: Vec<usize> = (0..n).step_by(2).collect();
            if every_other.last() != Some(&(n - 1)) {
                every_other.push(n - 1);
            }
            out.push((Step::Loft { sections: every_other.iter().map(|k| sections[*k].clone()).collect(), op: *op }, 0.0));
        }
        if n > 2 {
            out.push((Step::Loft { sections: vec![sections[0].clone(), sections[n - 1].clone()], op: *op }, 0.0));
        }
    }
    out
}

/// A revolved profile moved `nudge` along the axis.
pub fn nudged(profile: &[V2], nudge: f64) -> Vec<V2> {
    profile.iter().map(|q| V2::new(q.x, q.y + nudge)).collect()
}

/// A herringbone's second half, when step `i` of `body` is it: the first step's twisted prism
/// turned back the other way above it, which is that prism mirrored in the plane between them
/// (each point of the upper half at height `d` above it lies where the lower half's point `d`
/// below it does, turned the same). Mirrored and joined, it is the same solid for one sweep and a
/// mirror in place of two sweeps. The plane's height, in the body's frame.
pub fn mirrored_half(body: &Body, i: usize, step: &Step) -> Option<f64> {
    let (
        Some(Step::Prism { region: r0, z0: a, height: h, twist: t, op: Op::New }),
        Some(Step::Prism { region: r1, z0: b, height: h1, twist: t1, op: Op::Join }),
    ) = (body.steps.first(), body.steps.get(1))
    else {
        return None;
    };
    let turned = Region { outer: r0.outer.iter().map(|o| o.rotated(*t)).collect(), holes: r0.holes.iter().map(|o| o.rotated(*t)).collect() };
    let same = i == 1
        && matches!(step, Step::Prism { .. })
        && t.abs() > 1e-12
        && *t1 == -*t
        && h1 == h
        && (b - (a + h)).abs() <= 1e-9 * (1.0 + h.abs())
        && *r1 == turned;
    same.then_some(*b)
}

/// A region as the kernel takes it: each outer outline (by index) with the holes (by index) that
/// lie inside it. A hole is the outer outline's whose sampled outline holds the hole's start, the
/// smallest such when outlines nest; a hole inside none goes with the first.
pub fn region_parts(r: &Region) -> Vec<(usize, Vec<usize>)> {
    let rings: Vec<Vec<V2>> = r.outer.iter().map(|o| o.polyline(0.05)).collect();
    let inside = |ring: &[V2], q: V2| {
        let mut c = false;
        for w in ring.windows(2) {
            let (a, b) = (w[0], w[1]);
            if (a.y > q.y) != (b.y > q.y) && q.x < a.x + (q.y - a.y) * (b.x - a.x) / (b.y - a.y) {
                c = !c;
            }
        }
        c
    };
    let mut parts: Vec<(usize, Vec<usize>)> = (0..r.outer.len()).map(|k| (k, Vec::new())).collect();
    for (h, hole) in r.holes.iter().enumerate() {
        let Some(q) = hole.segs.first().map(|s| s.start()) else { continue };
        let home = (0..rings.len())
            .filter(|k| inside(&rings[*k], q))
            .min_by(|a, b| r.outer[*a].area().abs().total_cmp(&r.outer[*b].area().abs()))
            .unwrap_or(0);
        if let Some(p) = parts.get_mut(home) {
            p.1.push(h);
        }
    }
    parts
}
