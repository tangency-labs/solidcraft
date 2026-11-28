//! A body's steps rearranged to be built with fewer booleans on its teeth, the same solid: for
//! the builders (the plan keeps its steps as planned, each one of the generator's own).
//!
//! A cylindrical gear is planned as its teeth, then the solids joined to them (hub, shafts),
//! then the cuts (lightening holes, bearing pocket, bore, keyway, chamfers, shaft ends, hub
//! hardware), each a boolean on the whole toothed body. Since every join comes before every
//! cut, the body is the union of its solids less the union of its cuts, and
//! `(T ∪ B) − C = (T − C) ∪ (B − C)`:
//!
//! - a cut that is a straight prism through the whole height of each solid it meets, its
//!   outlines clear inside that solid's region, is holes in those regions ([`Fused::teeth`],
//!   [`Fused::blank`]): no boolean at all (a bore, a keyway, through lightening holes);
//! - the plain solids (round, straight prisms: hub, shafts) are a blank of their own with every
//!   cut that doesn't reach the teeth (hub chamfer, set screws, shaft ends), joined to the teeth
//!   in one union;
//! - what is left (tooth chamfers, a blind pocket, a bore chamfer on the teeth's face) is cut
//!   from the joined body as planned ([`Fused::after`]).
//!
//! Twisted teeth (helical, herringbone) take no holes: a hole swept with their twist would turn
//! with it. A body whose steps don't keep that order, or whose teeth aren't prisms or a loft,
//! is left as planned ([`fused`] is `None`).

use super::{Body, Op, Step};
use crate::geom::{Outline, Seg, V2};

/// [`fused`]'s answer: a body's steps as three lists, built one after another.
#[derive(Clone, Debug, PartialEq)]
pub struct Fused {
    /// The teeth (the first step new, the rest joined to it), with the through holes in their
    /// regions.
    pub teeth: Vec<Step>,
    /// The plain solids (the first new, the rest joined) with their holes, then the cuts that
    /// meet only them: a body of its own, joined to the teeth. Empty: none.
    pub blank: Vec<Step>,
    /// The cuts on the teeth joined to the blank, in the plan's order.
    pub after: Vec<Step>,
}

/// How near (mm) a hole's outline may come to a solid's outlines and still be taken as clear
/// inside it: the sampling's error both sides and a margin.
const CLEAR: f64 = 0.02;
/// The sampling of outlines for the tests here (mm).
const SAMPLE: f64 = 0.005;
const EPS: f64 = 1e-9;

/// `body`'s steps for building with fewer booleans on its teeth, the same solid (see the
/// module's notes); `None` when there is nothing to gain or it can't be shown the same.
pub fn fused(body: &Body) -> Option<Fused> {
    let steps = &body.steps;
    if steps.first()?.op() != Op::New || plain(&steps[0]) {
        return None;
    }
    if steps[1..].iter().any(|s| !matches!(s.op(), Op::Join | Op::Cut)) {
        return None;
    }
    // The solids in order, each cut with what was cut before the next solid (a compound's
    // layers, each with its own bore): a cut reaches only the solids made before it.
    let mut teeth: Vec<Step> = Vec::new();
    let mut blank: Vec<Step> = Vec::new();
    // The cuts left, each with how many solids were made before it.
    let mut left: Vec<(Step, usize)> = Vec::new();
    let mut holed = 0;
    for s in steps {
        if s.op() != Op::Cut {
            if plain(s) {
                blank.push(s.clone())
            } else {
                teeth.push(s.clone())
            }
            continue;
        }
        if let Some(holes) = through_holes(s, &teeth, &blank) {
            for (k, hs) in holes {
                let at = if k < teeth.len() { &mut teeth[k] } else { &mut blank[k - teeth.len()] };
                if let Step::Prism { region, .. } = at {
                    region.holes.extend(hs);
                }
            }
            holed += 1;
        } else {
            left.push((s.clone(), teeth.len() + blank.len()));
        }
    }
    let reach: Vec<Bounds> = teeth.iter().map(bounds).collect::<Option<_>>()?;
    // Each solid's place in the plan, to tell which came after a cut.
    let order: Vec<(bool, usize)> = {
        let (mut t, mut b) = (0, 0);
        steps
            .iter()
            .filter(|s| s.op() != Op::Cut)
            .map(|s| {
                if plain(s) {
                    b += 1;
                    (false, b - 1)
                } else {
                    t += 1;
                    (true, t - 1)
                }
            })
            .collect()
    };
    let mut blank_cuts: Vec<Step> = Vec::new();
    let mut after: Vec<Step> = Vec::new();
    for (c, made) in left {
        // A cut made before a solid reaches it now: only when it can't meet it.
        let cb = bounds(&c);
        for &(tooth, k) in &order[made..] {
            let later = if tooth { bounds(&teeth[k]) } else { bounds(&blank[k]) };
            if !cb.zip(later).is_some_and(|(a, b)| a.apart(&b)) {
                return None;
            }
        }
        let clear = cb.is_some_and(|b| reach.iter().all(|t| b.apart(t)));
        if clear && !blank.is_empty() {
            blank_cuts.push(c);
        } else {
            after.push(c);
        }
    }
    if holed == 0 && blank_cuts.is_empty() && blank.len() < 2 {
        return None;
    }
    for (k, s) in teeth.iter_mut().enumerate() {
        *s.op_mut() = if k == 0 { Op::New } else { Op::Join };
    }
    // The blank's first is new, the rest joined.
    for (k, s) in blank.iter_mut().enumerate() {
        *s.op_mut() = if k == 0 { Op::New } else { Op::Join };
    }
    blank.extend(blank_cuts);
    Some(Fused { teeth, blank, after })
}

/// A plain solid: a straight prism of round outlines about the axis (a hub, a shaft).
fn plain(s: &Step) -> bool {
    match s {
        Step::Prism { region, twist, .. } => {
            twist.abs() <= 1e-12
                && region.holes.is_empty()
                && !region.outer.is_empty()
                && region.outer.iter().all(|o| matches!(&o.segs[..], [Seg::Circle(c, r)] if c.len() <= EPS && *r > 0.0))
        }
        _ => false,
    }
}

/// Where a step reaches, in its body's frame: heights along the axis and distances from it.
#[derive(Clone, Copy, Debug)]
struct Bounds {
    z: (f64, f64),
    r: (f64, f64),
}

impl Bounds {
    /// Whether nothing of the two can meet (they may touch).
    fn apart(&self, o: &Bounds) -> bool {
        self.z.1 <= o.z.0 + EPS || self.z.0 >= o.z.1 - EPS || self.r.0 >= o.r.1 + EPS || o.r.0 >= self.r.1 + EPS
    }
}

/// The nearest an outline comes to the origin (0 when it holds it) and the farthest, sampled.
fn radii(o: &Outline) -> (f64, f64) {
    let p = o.polyline(SAMPLE);
    let far = p.iter().map(|q| q.len()).fold(0.0, f64::max);
    if inside(&p, V2::ZERO) {
        return (0.0, far + SAMPLE);
    }
    let near = p.windows(2).map(|w| point_segment(V2::ZERO, w[0], w[1])).fold(f64::INFINITY, f64::min);
    ((near - SAMPLE).max(0.0), far + SAMPLE)
}

/// A step's [`Bounds`], when they can be said.
fn bounds(s: &Step) -> Option<Bounds> {
    let sorted = |a: f64, b: f64| (a.min(b), a.max(b));
    match s {
        Step::Prism { region, z0, height, .. } => {
            let rs: Vec<(f64, f64)> = region.outer.iter().map(radii).collect();
            let r = (rs.iter().map(|x| x.0).fold(f64::INFINITY, f64::min), rs.iter().map(|x| x.1).fold(0.0, f64::max));
            (!rs.is_empty()).then_some(Bounds { z: sorted(*z0, z0 + height), r })
        }
        Step::Loft { sections, .. } => {
            let z = sections.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), (_, h)| (lo.min(*h), hi.max(*h)));
            let far = sections.iter().map(|(o, _)| radii(o).1).fold(0.0, f64::max);
            (!sections.is_empty()).then_some(Bounds { z, r: (0.0, far) })
        }
        Step::Revolve { profile, .. } => {
            let z = profile.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), q| (lo.min(q.y), hi.max(q.y)));
            let r = profile.iter().fold((f64::INFINITY, 0.0f64), |(lo, hi), q| (lo.min(q.x.abs()), hi.max(q.x.abs())));
            (!profile.is_empty()).then_some(Bounds { z, r })
        }
        Step::Placed { frame, step } => {
            let inner = bounds(step)?;
            let axial = frame.origin[0].abs() <= EPS
                && frame.origin[1].abs() <= EPS
                && frame.z[0].abs() <= EPS
                && frame.z[1].abs() <= EPS
                && (frame.z[2].abs() - 1.0).abs() <= EPS;
            if axial {
                let (a, b) = (frame.origin[2] + frame.z[2] * inner.z.0, frame.origin[2] + frame.z[2] * inner.z.1);
                return Some(Bounds { z: sorted(a, b), r: inner.r });
            }
            // Elsewhere: the box round it, moved.
            let m = inner.r.1;
            let mut z = (f64::INFINITY, f64::NEG_INFINITY);
            let mut far: f64 = 0.0;
            for x in [-m, m] {
                for y in [-m, m] {
                    for h in [inner.z.0, inner.z.1] {
                        let w = frame.world([x, y, h]);
                        z = (z.0.min(w[2]), z.1.max(w[2]));
                        far = far.max(w[0].hypot(w[1]));
                    }
                }
            }
            Some(Bounds { z, r: (0.0, far) })
        }
        Step::Taper { .. } | Step::RingTaper { .. } | Step::Coil { .. } | Step::Group { .. } => None,
    }
}

/// When cut `c` is holes through the solids it meets (`teeth` then `blank`, by index in that
/// order): each such solid's index and the outlines that are holes in it. `None` when it isn't.
fn through_holes(c: &Step, teeth: &[Step], blank: &[Step]) -> Option<Vec<(usize, Vec<Outline>)>> {
    let Step::Prism { region: cut, z0, height, twist, .. } = c else { return None };
    if twist.abs() > 1e-12 || !cut.holes.is_empty() || cut.outer.is_empty() {
        return None;
    }
    let cz = (z0.min(z0 + height), z0.max(z0 + height));
    let cut_r: Vec<(f64, f64)> = cut.outer.iter().map(radii).collect();
    let mut out = Vec::new();
    for (k, s) in teeth.iter().chain(blank).enumerate() {
        let b = bounds(s)?;
        // Not at the same heights, or every outline outside its reach: it misses.
        if cz.1 <= b.z.0 + EPS || cz.0 >= b.z.1 - EPS || cut_r.iter().all(|r| r.0 >= b.r.1 + EPS) {
            continue;
        }
        let Step::Prism { region, twist: t, .. } = s else { return None };
        // Every outline one of its holes already (a bore cut again): nothing to take.
        if cut.outer.iter().all(|o| region.holes.contains(o)) {
            continue;
        }
        if t.abs() > 1e-12 || !(cz.0 <= b.z.0 + EPS && cz.1 >= b.z.1 - EPS) {
            return None;
        }
        let loops: Vec<Vec<V2>> = region.outer.iter().chain(&region.holes).map(|o| o.polyline(SAMPLE)).collect();
        let loop_r: Vec<(f64, f64)> = region.outer.iter().chain(&region.holes).map(radii).collect();
        let mut holes = Vec::new();
        for (o, r) in cut.outer.iter().zip(&cut_r) {
            let p = o.polyline(SAMPLE);
            // Clear of every outline of the solid, and holding none of them.
            for (l, lr) in loops.iter().zip(&loop_r) {
                let near = r.1 + CLEAR < lr.0 || r.0 > lr.1 + CLEAR;
                if !near && within(&p, l, CLEAR) {
                    return None;
                }
                if l.first().is_some_and(|q| inside(&p, *q)) {
                    return None;
                }
            }
            let q = p.first().copied()?;
            if loops.iter().filter(|l| inside(l, q)).count() % 2 == 1 {
                holes.push(o.clone());
            }
        }
        if !holes.is_empty() {
            out.push((k, holes));
        }
    }
    Some(out)
}

/// Whether `q` is inside the closed polyline `ring` (even–odd).
fn inside(ring: &[V2], q: V2) -> bool {
    let mut c = false;
    for w in ring.windows(2) {
        let (a, b) = (w[0], w[1]);
        if (a.y > q.y) != (b.y > q.y) && q.x < a.x + (q.y - a.y) * (b.x - a.x) / (b.y - a.y) {
            c = !c;
        }
    }
    c
}

fn point_segment(q: V2, a: V2, b: V2) -> f64 {
    let d = b - a;
    let l = d.dot(d);
    let t = if l > 0.0 { ((q - a).dot(d) / l).clamp(0.0, 1.0) } else { 0.0 };
    q.dist(a + d * t)
}

/// Whether two polylines come within `limit` of each other anywhere (crossing included).
fn within(p: &[V2], q: &[V2], limit: f64) -> bool {
    let bbox = |s: &[V2]| {
        s.iter().fold((f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY), |b, v| {
            (b.0.min(v.x), b.1.min(v.y), b.2.max(v.x), b.3.max(v.y))
        })
    };
    let (bp, bq) = (bbox(p), bbox(q));
    if (bq.0 - bp.2).max(bp.0 - bq.2).max(bq.1 - bp.3).max(bp.1 - bq.3) >= limit {
        return false;
    }
    let boxes: Vec<_> = q.windows(2).map(bbox).collect();
    p.windows(2).any(|a| {
        let ba = bbox(a);
        q.windows(2).zip(&boxes).any(|(b, bb)| {
            let gap = (bb.0 - ba.2).max(ba.0 - bb.2).max(bb.1 - ba.3).max(ba.1 - bb.3);
            gap < limit && segment_segment(a[0], a[1], b[0], b[1]) < limit
        })
    })
}

fn segment_segment(a: V2, b: V2, c: V2, d: V2) -> f64 {
    let side = |p: V2, q: V2, r: V2| (q - p).cross(r - p);
    let (d1, d2, d3, d4) = (side(c, d, a), side(c, d, b), side(a, b, c), side(a, b, d));
    if ((d1 > 0.0) != (d2 > 0.0)) && ((d3 > 0.0) != (d4 > 0.0)) {
        return 0.0;
    }
    point_segment(a, c, d).min(point_segment(b, c, d)).min(point_segment(c, a, b)).min(point_segment(d, a, b))
}
