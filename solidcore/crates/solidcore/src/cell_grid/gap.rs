//! Exact distances between pieces of a boundary (lines and circular arcs) and between loops of
//! them, for the clearance of a sketched cell from its neighbours.
//!
//! The distance between two pieces is 0 when they meet. Otherwise it is reached either at an end
//! of one piece (its distance to the other piece, in closed form, [`Prim::distance`]) or where both
//! are interior and the segment between the two points is normal to both curves: a line's normal
//! through an arc's centre, or the line through two arcs' centres. Those few candidate points are
//! worked out in closed form, and the least distance between them and the ends is the answer. No
//! sampling, no flattening.

use crate::math::det;
use crate::math::Vec2;

use super::free;
use super::region::Prim;
use super::vec::Vec2Ext;

/// The direction of `v` as an angle.
fn angle_of(v: Vec2) -> f64 {
    det::atan2(v.y, v.x)
}

/// The point of `arc` (an arc piece) nearest the line `ab`, where that point is interior to the
/// arc and the line's normal through the centre reaches it: `centre +- r * n`, and how far each is
/// from the segment. `INFINITY` when neither lies on the arc.
fn line_arc(a: Vec2, b: Vec2, arc: &Prim) -> f64 {
    let Prim::Arc { c, r, .. } = *arc else {
        return f64::INFINITY;
    };
    let Some(d) = (b - a).normalized() else {
        return f64::INFINITY;
    };
    let line = Prim::line(a, b);
    let mut m = f64::INFINITY;
    for s in [1.0, -1.0] {
        let n = Vec2::new(-d.y * s, d.x * s);
        if arc.contains_angle(angle_of(n), 0.0) {
            m = m.min(line.distance(c + n * r));
        }
    }
    m
}

/// The least distance between two arc pieces that is reached with both interior: along the line
/// through the centres, and for concentric arcs wherever their directions overlap.
fn arc_arc(p: &Prim, q: &Prim) -> f64 {
    let (
        Prim::Arc {
            c: c1,
            r: r1,
            p0: a0,
            p1: a1,
            ..
        },
        Prim::Arc {
            c: c2,
            r: r2,
            p0: b0,
            p1: b1,
            ..
        },
    ) = (*p, *q)
    else {
        return f64::INFINITY;
    };
    let d = c2 - c1;
    let len = d.length();
    let mut m = f64::INFINITY;
    if len > 1e-12 {
        let u = d / len;
        for s in [1.0, -1.0] {
            for t in [1.0, -1.0] {
                if p.contains_angle(angle_of(u * s), 0.0) && q.contains_angle(angle_of(u * t), 0.0)
                {
                    let (x, y) = (c1 + u * (s * r1), c2 + u * (t * r2));
                    m = m.min(x.distance(y));
                }
            }
        }
    } else {
        // Concentric: every shared direction is `|r1 - r2|` apart, and the ranges share one
        // exactly when an end of either lies in the other.
        let shared = [a0, a1]
            .iter()
            .any(|x| q.contains_angle(angle_of(*x - c2), 0.0))
            || [b0, b1]
                .iter()
                .any(|x| p.contains_angle(angle_of(*x - c1), 0.0));
        if shared {
            m = (r1 - r2).abs();
        }
    }
    m
}

/// The distance between two pieces (0 where they meet).
pub(super) fn prim_distance(a: &Prim, b: &Prim) -> f64 {
    if !free::intersections(a, b).is_empty() {
        return 0.0;
    }
    let mut m = a
        .distance(b.start())
        .min(a.distance(b.end()))
        .min(b.distance(a.start()))
        .min(b.distance(a.end()));
    match (a, b) {
        (Prim::Line { a: p, b: q, .. }, Prim::Arc { .. }) => m = m.min(line_arc(*p, *q, b)),
        (Prim::Arc { .. }, Prim::Line { a: p, b: q, .. }) => m = m.min(line_arc(*p, *q, a)),
        (Prim::Arc { .. }, Prim::Arc { .. }) => m = m.min(arc_arc(a, b)),
        (Prim::Line { .. }, Prim::Line { .. }) => {}
    }
    m
}

/// The least distance between the boundaries of two loops of pieces (0 where they meet), `INFINITY`
/// when either has no pieces. A pair of pieces whose boxes are farther apart than the best
/// distance so far is skipped: the boxes' gap along an axis is at most the distance.
pub(super) fn boundary_distance(a: &[Prim], b: &[Prim]) -> f64 {
    let mut m = f64::INFINITY;
    for p in a {
        let pb = p.bounds();
        for q in b {
            if !pb.overlaps(&q.bounds(), m) {
                continue;
            }
            m = m.min(prim_distance(p, q));
            if m == 0.0 {
                return 0.0;
            }
        }
    }
    m
}
