//! The planner's entry (design 6): the area and the plate as loops in the hinge frame, row by row
//! through spans, clean crossings and keep-out zones to the slots.
//!
//! The frame and the area stay inputs, not a sketch: a sheet-metal bend zone feeds the same
//! entry later (sheet-metal addendum).

use std::f64::consts::{FRAC_PI_2, PI};

use super::lattice::{FrameBox, Lattice};
use super::near::{sweeps, NearEdge};
use super::rows::plan_rows_zoned;
use super::spans::{clean_ends, row_spans, row_zone};
use super::{HingeSlot, LivingHingeError, LivingHingePlan, LivingHingeSpec, CHORD, GRAZE, ON_EDGE};
use crate::math::Vec2;
use crate::prism::profile::{Loop2, Region2, Seg2};

/// The box of `loops` in the frame; an arc counts its ends and the points where it is furthest
/// along `u` or `v`. Empty loops give an inverted box, which the lattice refuses.
pub fn frame_box(loops: &[Vec<NearEdge>]) -> FrameBox {
    let mut b = FrameBox {
        u_min: f64::INFINITY,
        u_max: f64::NEG_INFINITY,
        v_min: f64::INFINITY,
        v_max: f64::NEG_INFINITY,
    };
    let mut add = |p: (f64, f64)| {
        b.u_min = b.u_min.min(p.0);
        b.u_max = b.u_max.max(p.0);
        b.v_min = b.v_min.min(p.1);
        b.v_max = b.v_max.max(p.1);
    };
    for edge in loops.iter().flatten() {
        match *edge {
            NearEdge::Line(a, c) | NearEdge::Chord(a, c) => {
                add(a);
                add(c);
            }
            NearEdge::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                let at = |a: f64| (center.0 + radius * a.cos(), center.1 + radius * a.sin());
                add(at(start));
                add(at(start + sweep));
                for quarter in 0_i32..4 {
                    let a = f64::from(quarter) * FRAC_PI_2;
                    if sweeps(start, sweep, a) {
                        add(at(a));
                    }
                }
            }
        }
    }
    b
}

/// A loop of the plate's or the area's outline as near edges (design 6.4): lines and arcs as they
/// are, a conic or a Bézier as its chords ([`NearEdge::Chord`]), each within [`CHORD`] of the
/// curve. `None` when a curve cannot be followed that closely.
///
/// An arc's ends are worked out from its angles, so they can miss the neighbours' points by a
/// rounding: a line or chord end within [`ON_EDGE`] of the arc end it meets is moved onto it, so
/// both edges agree on which side of a row their common point lies.
pub fn near_loop(lp: &Loop2) -> Option<Vec<NearEdge>> {
    frame_loop(lp, 0.0)
}

/// The sketch point `p` in the hinge frame of `angle` (design 1a): `(p·û, p·v̂)` with
/// `û = (cos α, sin α)` along the slots and `v̂ = (−sin α, cos α)` across the rows. An angle of 0
/// leaves `p` as it is, bit for bit.
#[must_use]
pub fn frame_point(p: (f64, f64), angle: f64) -> (f64, f64) {
    let (s, c) = angle.sin_cos();
    (p.0 * c + p.1 * s, p.1 * c - p.0 * s)
}

/// [`near_loop`] of a sketch loop `lp` in the hinge frame of `angle` ([`frame_point`]): every
/// point turned into the frame, an arc's angles less `angle`, then the arc ends snapped.
pub fn frame_loop(lp: &Loop2, angle: f64) -> Option<Vec<NearEdge>> {
    let at = |p: Vec2| frame_point((p.x, p.y), angle);
    let mut out = Vec::with_capacity(lp.segs.len());
    for seg in &lp.segs {
        match *seg {
            Seg2::Line { a, b } => out.push(NearEdge::Line(at(a), at(b))),
            Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            } => out.push(NearEdge::Arc {
                center: at(center),
                radius,
                start: start - angle,
                sweep,
            }),
            Seg2::Cubic { .. } | Seg2::Conic { .. } | Seg2::Bezier { .. } => {
                let points = crate::cell_grid::flatten(seg, CHORD)?;
                out.extend(
                    points
                        .iter()
                        .zip(points.iter().skip(1))
                        .map(|(&p, &q)| NearEdge::Chord(at(p), at(q))),
                );
            }
        }
    }
    snap_arc_ends(&mut out);
    Some(out)
}

/// The two ends of an arc, `None` for a line or a chord.
fn arc_ends(edge: &NearEdge) -> Option<((f64, f64), (f64, f64))> {
    match *edge {
        NearEdge::Arc {
            center,
            radius,
            start,
            sweep,
        } => {
            let at = |a: f64| (center.0 + radius * a.cos(), center.1 + radius * a.sin());
            Some((at(start), at(start + sweep)))
        }
        NearEdge::Line(..) | NearEdge::Chord(..) => None,
    }
}

/// Moves the end of a line or chord (its last end when `last`) onto `to` when within [`ON_EDGE`].
fn snap_end(edge: &mut NearEdge, last: bool, to: (f64, f64)) {
    if let NearEdge::Line(a, b) | NearEdge::Chord(a, b) = edge {
        let p = if last { b } else { a };
        if (p.0 - to.0).hypot(p.1 - to.1) <= ON_EDGE {
            *p = to;
        }
    }
}

/// Snaps the neighbours of every arc of the closed loop `edges` onto the arc's ends.
fn snap_arc_ends(edges: &mut [NearEdge]) {
    let n = edges.len();
    for i in 0..n {
        let Some((first, last)) = edges.get(i).and_then(arc_ends) else {
            continue;
        };
        if let Some(prev) = edges.get_mut((i + n - 1) % n) {
            snap_end(prev, true, first);
        }
        if let Some(next) = edges.get_mut((i + 1) % n) {
            snap_end(next, false, last);
        }
    }
}

/// The distance from `p` to `edge`.
fn distance(p: (f64, f64), edge: &NearEdge) -> f64 {
    match *edge {
        NearEdge::Line(a, b) | NearEdge::Chord(a, b) => {
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len2 = dx * dx + dy * dy;
            let t = if len2 > 0.0 {
                (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            (p.0 - a.0 - t * dx).hypot(p.1 - a.1 - t * dy)
        }
        NearEdge::Arc {
            center,
            radius,
            start,
            sweep,
        } => {
            let (dx, dy) = (p.0 - center.0, p.1 - center.1);
            if sweeps(start, sweep, dy.atan2(dx)) {
                (dx.hypot(dy) - radius).abs()
            } else {
                let at = |a: f64| (center.0 + radius * a.cos(), center.1 + radius * a.sin());
                let (e0, e1) = (at(start), at(start + sweep));
                (p.0 - e0.0)
                    .hypot(p.1 - e0.1)
                    .min((p.0 - e1.0).hypot(p.1 - e1.1))
            }
        }
    }
}

/// Three points of `edge`: its ends and its middle.
fn probes(edge: &NearEdge) -> [(f64, f64); 3] {
    match *edge {
        NearEdge::Line(a, b) | NearEdge::Chord(a, b) => {
            [a, ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0), b]
        }
        NearEdge::Arc {
            center,
            radius,
            start,
            sweep,
        } => {
            let at = |a: f64| (center.0 + radius * a.cos(), center.1 + radius * a.sin());
            [at(start), at(start + sweep / 2.0), at(start + sweep)]
        }
    }
}

/// The area's edges inside the plate, not on its boundary (design 6.2): an edge whose ends and
/// middle all lie within [`ON_EDGE`] of the plate's edges is the plate's edge (open), any other is
/// closed and keeps `r + m` from the slots.
pub fn closed_edges(field: &[Vec<NearEdge>], plate: &[Vec<NearEdge>]) -> Vec<NearEdge> {
    let on_plate = |p: (f64, f64)| plate.iter().flatten().any(|e| distance(p, e) <= ON_EDGE);
    field
        .iter()
        .flatten()
        .filter(|edge| !probes(edge).into_iter().all(&on_plate))
        .copied()
        .collect()
}

/// Plans the slots of the area `field` in the plate `plate`, both closed loops in the hinge frame
/// (a conic or a Bézier as its chords). `closed` are the area's edges that lie inside the plate
/// (not on its boundary): slots keep `r + m` from them; `None` takes [`closed_edges`]. The lattice is centred on the area's box,
/// or anchored at `anchor`. Each row: its spans (design 6.3), the open tags of crossings that are
/// not clean dropped (6.6 rule 1), its zone `Z_c ∪ Z_o` (6.5), then runners, pieces and short
/// pieces (6.6 to 6.9).
pub fn plan(
    field: &[Vec<NearEdge>],
    plate: &[Vec<NearEdge>],
    closed: Option<&[NearEdge]>,
    spec: &LivingHingeSpec,
    anchor: Option<(f64, f64)>,
) -> Result<LivingHingePlan, LivingHingeError> {
    spec.check()?;
    if plate.iter().all(Vec::is_empty) {
        return Err(LivingHingeError::Plate("it has no edges".into()));
    }
    let found;
    let closed = if let Some(c) = closed {
        c
    } else {
        found = closed_edges(field, plate);
        found.as_slice()
    };
    let b = frame_box(field);
    let lattice = match anchor {
        Some(a) => Lattice::anchored(spec, &b, a)?,
        None => Lattice::centred(spec, &b)?,
    };
    let rho = spec.edge_reach();
    let closed_reach = spec.radius() + spec.margin;
    Ok(plan_rows_zoned(
        spec,
        &lattice,
        |_, v| {
            let mut spans = row_spans(field, plate, v);
            clean_ends(&mut spans, plate, closed, v, rho, closed_reach);
            spans
        },
        |_, v| row_zone(plate, closed, v, rho, closed_reach),
    ))
}

/// A slot's region in the hinge frame for the extrude (design 7): two lines and two half-circle
/// arcs of radius `slot_width / 2`, counter-clockwise, around the centre line `[a0, a1]` on the
/// row `v` (for a run-out, past the plate's edge).
pub fn slot_region(slot: &HingeSlot, slot_width: f64) -> Region2 {
    let r = slot_width / 2.0;
    let (a0, a1, v) = (slot.a0, slot.a1, slot.v);
    Region2 {
        outer: Loop2 {
            segs: vec![
                Seg2::Line {
                    a: Vec2::new(a0, v - r),
                    b: Vec2::new(a1, v - r),
                },
                Seg2::Arc {
                    center: Vec2::new(a1, v),
                    radius: r,
                    start: -FRAC_PI_2,
                    sweep: PI,
                },
                Seg2::Line {
                    a: Vec2::new(a1, v + r),
                    b: Vec2::new(a0, v + r),
                },
                Seg2::Arc {
                    center: Vec2::new(a0, v),
                    radius: r,
                    start: FRAC_PI_2,
                    sweep: PI,
                },
            ],
        },
        holes: Vec::new(),
    }
}

/// An edge's carrier: the whole line through a line or chord, the whole circle of an arc.
#[derive(Clone, Copy)]
enum Carrier {
    Line((f64, f64), (f64, f64)),
    Circle((f64, f64), f64),
}

fn carrier(e: &NearEdge) -> Carrier {
    match *e {
        NearEdge::Line(a, b) | NearEdge::Chord(a, b) => Carrier::Line(a, b),
        NearEdge::Arc { center, radius, .. } => Carrier::Circle(center, radius),
    }
}

/// The `v` of the points where the lines through `a, b` and `p, d` and circles meet. Points of the
/// carriers outside the edges come too: an extra row costs a sample, never a wrong answer.
fn crossing_vs(x: &Carrier, y: &Carrier, out: &mut Vec<f64>) {
    let line_circle = |a: (f64, f64), b: (f64, f64), c: (f64, f64), r: f64, out: &mut Vec<f64>| {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let (fx, fy) = (a.0 - c.0, a.1 - c.1);
        let qa = dx * dx + dy * dy;
        if qa <= 0.0 {
            return;
        }
        let qb = 2.0 * (fx * dx + fy * dy);
        let qc = fx * fx + fy * fy - r * r;
        let disc = qb * qb - 4.0 * qa * qc;
        if disc < 0.0 {
            return;
        }
        let s = disc.sqrt();
        for t in [(-qb - s) / (2.0 * qa), (-qb + s) / (2.0 * qa)] {
            out.push(a.1 + t * dy);
        }
    };
    match (*x, *y) {
        (Carrier::Line(a, b), Carrier::Line(p, d)) => {
            let (r, s) = ((b.0 - a.0, b.1 - a.1), (d.0 - p.0, d.1 - p.1));
            let den = r.0 * s.1 - r.1 * s.0;
            if den.abs() > GRAZE {
                let t = ((p.0 - a.0) * s.1 - (p.1 - a.1) * s.0) / den;
                out.push(a.1 + t * r.1);
            }
        }
        (Carrier::Line(a, b), Carrier::Circle(c, r))
        | (Carrier::Circle(c, r), Carrier::Line(a, b)) => {
            line_circle(a, b, c, r, out);
        }
        (Carrier::Circle(c0, r0), Carrier::Circle(c1, r1)) => {
            let (dx, dy) = (c1.0 - c0.0, c1.1 - c0.1);
            let d2 = dx * dx + dy * dy;
            if d2 <= GRAZE {
                return;
            }
            // The radical line, then the circle against it.
            let k = (r0 * r0 - r1 * r1 + d2) / (2.0 * d2);
            let m = (c0.0 + k * dx, c0.1 + k * dy);
            line_circle(m, (m.0 - dy, m.1 + dx), c0, r0, out);
        }
    }
}

/// The rows where the slices of two regions can change their order: every edge end, every arc's
/// top and bottom, every crossing of an edge of `a` with one of `b`.
fn critical_vs(a: &[Vec<NearEdge>], b: &[Vec<NearEdge>]) -> Vec<f64> {
    let mut vs = Vec::new();
    for e in a.iter().chain(b).flatten() {
        match *e {
            NearEdge::Line(p, q) | NearEdge::Chord(p, q) => vs.extend([p.1, q.1]),
            NearEdge::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                vs.push(center.1 + radius * start.sin());
                vs.push(center.1 + radius * (start + sweep).sin());
                for t in [FRAC_PI_2, -FRAC_PI_2] {
                    if sweeps(start, sweep, t) {
                        vs.push(center.1 + radius * t.sin());
                    }
                }
            }
        }
    }
    for x in a.iter().flatten() {
        for y in b.iter().flatten() {
            crossing_vs(&carrier(x), &carrier(y), &mut vs);
        }
    }
    vs.retain(|v| v.is_finite());
    vs.sort_by(f64::total_cmp);
    vs.dedup();
    vs
}

/// Whether the area `field` and the plate's outline `plate` overlap (design 3.5): some row holds
/// a stretch inside both, longer than [`ON_EDGE`]. Touching along an edge or at a point is not
/// meeting. Exact: between two rows of [`critical_vs`] no slice changes order, so the slab's
/// middle row answers for it; slabs thinner than [`ON_EDGE`] count as touching.
pub fn meets(field: &[Vec<NearEdge>], plate: &[Vec<NearEdge>]) -> bool {
    critical_vs(field, plate).windows(2).any(|w| {
        let &[lo, hi] = w else { return false };
        hi - lo > ON_EDGE
            && row_spans(field, plate, (lo + hi) / 2.0)
                .iter()
                .any(|s| s.hi - s.lo > ON_EDGE)
    })
}
