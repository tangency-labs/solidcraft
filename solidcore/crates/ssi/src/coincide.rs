//! Coincident patches: one lies on the analytic surface of the other, so where
//! they overlap they share a region, and that region's boundary is their
//! intersection (ssi-corpus class 6).
//!
//! Lying on the surface is read off the composed field's coefficients
//! (`field::relate`). The boundary is made of the first patch's edges where they
//! fall inside the second and the second's edges where they fall strictly
//! inside the first, joined into loops. Inside is decided on parameters: each
//! edge point is inverted onto the other patch (`ImplicitCAM`'s certified closest
//! point finds the Bézier piece, then Newton runs on that piece, past its
//! boundary if it must, where the exact surface goes on), and its parameters
//! are compared with the patch's domain. No distance decides anything.

use crate::field::{relate_field, Field, Relation};
use crate::intersect::{CurvePoint, Param};
use crate::invert::Side;
use crate::patch::Patch;
use crate::piece::{dot, norm, sub, Piece, V3};
use crate::Error;

/// Sampling of an edge: the longest chord and the largest sag between chord and curve.
pub(crate) struct Spacing {
    pub max_step: f64,
    pub max_sag: f64,
}

/// Parameter slack for "on the boundary": 1e-9 of the domain.
const SLACK: f64 = 1e-9;

/// If one patch lies on the other's implicit surface (`fa`, `fb`), the boundary
/// of the region they share, as arcs oriented counterclockwise in the first
/// patch's parameters (empty when they lie on one surface without
/// overlapping); None when neither lies on the other's surface.
/// `pa` and `pb` are the patches' Bézier pieces.
#[allow(clippy::too_many_arguments)]
pub(crate) fn boundary(
    a: &Patch,
    ia: usize,
    fa: Option<Field>,
    pa: &[Piece],
    b: &Patch,
    ib: usize,
    fb: Option<Field>,
    pb: &[Piece],
    spacing: &Spacing,
) -> Result<Option<Vec<Vec<CurvePoint>>>, Error> {
    // A piece's corners and middle off the surface prove it not on it, before
    // any composition (a degree-8 form's with a free-form piece, only to say
    // no, was 8 ms of a pair that is not coincident, ssi-corpus y23_fig1b).
    let off = |piece: &Piece, field: &Field| {
        let b = piece.bounds();
        let size = norm(sub([b[3], b[4], b[5]], [b[0], b[1], b[2]]));
        [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0), (0.5, 0.5)]
            .iter()
            .any(|&(s, t)| {
                let p = piece.evaluate(s, t).point;
                norm(sub(p, field.foot(p))) > 1e-6 * size
            })
    };
    let on = |pieces: &[Piece], other: Option<Field>| {
        other.is_some_and(|field| {
            pieces
                .iter()
                .all(|piece| !off(piece, &field) && relate_field(piece, &field) == Relation::On)
        })
    };
    if !(on(pa, fb) || on(pb, fa)) {
        return Ok(None);
    }
    let (sa, sb) = (
        Side::with_pieces(a, ia, pa.to_vec())?,
        Side::with_pieces(b, ib, pb.to_vec())?,
    );
    let mut arcs = Vec::new();
    // The first patch's edges where they lie inside the second or on its
    // boundary; the second's where they lie strictly inside the first, so an
    // edge the two share is taken once.
    for e in 0..4 {
        arcs.extend(clip_edge(&sa, e, &sb, -SLACK, spacing, false));
    }
    let mut from_b = Vec::new();
    for e in 0..4 {
        from_b.extend(clip_edge(&sb, e, &sa, SLACK, spacing, true));
    }
    // The second patch's counterclockwise is the first's where their normals agree.
    let agree = arcs
        .iter()
        .chain(&from_b)
        .flatten()
        .next()
        .is_none_or(|q: &CurvePoint| {
            let (_, na) = sa.at(q.a.u, q.a.v);
            let (_, nb) = sb.at(q.b.u, q.b.v);
            dot(na, nb) >= 0.0
        });
    if !agree {
        for arc in &mut from_b {
            arc.reverse();
        }
    }
    arcs.extend(from_b);
    Ok(Some(arcs))
}

/// The runs of one edge of `own` whose points invert inside `other` (margin
/// above `shrink`), each a list of curve points, ends found by bisection on the
/// margin. `swapped` when `own` is the second patch, so the parameters go into
/// the right sides of each curve point.
fn clip_edge(
    own: &Side,
    e: usize,
    other: &Side,
    shrink: f64,
    spacing: &Spacing,
    swapped: bool,
) -> Vec<Vec<CurvePoint>> {
    let point_at = |t: f64| {
        let (u, v) = own.edge(e, t);
        (own.at(u, v).0, (u, v))
    };
    // A collapsed edge (a pole) is a point, not an arc.
    let (p0, p1, pm) = (point_at(0.0).0, point_at(1.0).0, point_at(0.5).0);
    let size = norm(sub(p1, p0)).max(norm(sub(pm, p0)));
    if !(size > 1e-12) {
        return Vec::new();
    }
    let ts = sample(&|t| point_at(t).0, spacing);
    // A point and its margin inside `other`, the boundary where it is (no
    // shrink). Inverted from the last point's parameters (each from the
    // certified closest point, two overlapping spheres took 0.23 s, ssi-corpus
    // c6_spheres_overlap), and afresh where that lands outside a closed
    // patch, whose seam a warm start runs past.
    let hint = std::cell::Cell::new(None);
    let record_from = |t: f64, warm: bool| -> Option<(CurvePoint, f64)> {
        let (p, own_uv) = point_at(t);
        let mut other_uv = other.invert(p, if warm { hint.get() } else { None })?;
        if hint.get().is_some()
            && other.closed.iter().any(|&c| c)
            && other.margin(other_uv, 0.0) < 0.0
        {
            if let Some(fresh) = other.invert(p, None) {
                if other.margin(fresh, 0.0) > other.margin(other_uv, 0.0) {
                    other_uv = fresh;
                }
            }
        }
        if warm {
            hint.set(Some(other_uv));
        }
        let mine = Param {
            patch: own.index,
            u: own_uv.0,
            v: own_uv.1,
        };
        let theirs = Param {
            patch: other.index,
            u: other_uv.0,
            v: other_uv.1,
        };
        let (a, b) = if swapped {
            (theirs, mine)
        } else {
            (mine, theirs)
        };
        Some((CurvePoint { p, a, b }, other.margin(other_uv, 0.0)))
    };
    let record = |t: f64| record_from(t, true);
    let fresh = |t: f64| record_from(t, false);
    let raw = |t: f64| fresh(t).map_or(f64::NEG_INFINITY, |(_, m)| m);
    let inside = |t: f64| raw(t) - shrink >= 0.0;
    // Where the run changes between t0 and t1, the change itself: on the
    // boundary exactly when the two ends lie on its two sides, else where the
    // shrunk boundary is crossed (an edge lying along the boundary leaving it).
    let crossing = |mut lo: f64, mut hi: f64| {
        let lo_in = inside(lo);
        let exact = (raw(lo) >= 0.0) != (raw(hi) >= 0.0);
        let side = |t: f64| if exact { raw(t) >= 0.0 } else { inside(t) };
        let lo_side = side(lo);
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if side(mid) == lo_side {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        if lo_in {
            lo
        } else {
            hi
        }
    };
    let mut runs = Vec::new();
    let mut run: Vec<CurvePoint> = Vec::new();
    let mut previous: Option<(f64, bool)> = None;
    for &t in &ts {
        let here = record(t);
        let is_in = here.as_ref().is_some_and(|(_, m)| *m - shrink >= 0.0);
        if let Some((t_prev, was_in)) = previous {
            if was_in != is_in {
                let tc = crossing(t_prev, t);
                if let Some((q, _)) = record(tc) {
                    run.push(q);
                }
                if was_in {
                    runs.push(std::mem::take(&mut run));
                }
            }
        }
        if is_in {
            if let Some((q, _)) = here {
                run.push(q);
            }
        }
        previous = Some((t, is_in));
    }
    if run.len() > 1 {
        runs.push(run);
    }
    runs.retain(|r| r.len() > 1);
    runs
}

/// Edge parameters from 0 to 1 at which the curve is sampled: halved until
/// every chord is shorter than the step and sags less than the sag.
pub(crate) fn sample(curve: &dyn Fn(f64) -> V3, spacing: &Spacing) -> Vec<f64> {
    sample_points(&|t| (curve(t), ()), spacing)
        .into_iter()
        .map(|(t, _, ())| t)
        .collect()
}

/// As `sample`, keeping each sample's point and whatever else the curve gave
/// with it, so nothing is computed twice.
pub(crate) fn sample_points<T: Copy>(
    curve: &dyn Fn(f64) -> (V3, T),
    spacing: &Spacing,
) -> Vec<(f64, V3, T)> {
    let first = curve(0.0);
    let mut out = vec![(0.0, first.0, first.1)];
    // Each interval carries its ends' points, so every point is computed once.
    let mut stack = vec![(0.0, first, 1.0, curve(1.0), 0)];
    while let Some((t0, a, t1, b, depth)) = stack.pop() {
        let tm = 0.5 * (t0 + t1);
        let m = curve(tm);
        let chord = norm(sub(b.0, a.0));
        let sag = norm(sub(
            m.0,
            [
                0.5 * (a.0[0] + b.0[0]),
                0.5 * (a.0[1] + b.0[1]),
                0.5 * (a.0[2] + b.0[2]),
            ],
        ));
        // At most 2^16 intervals: a curve whose sag will not shrink is not smooth
        // there, and halving it further only spends time (to 2^30 it was a hang).
        // At least 16, to find a turn the ends do not show, but not on an arc
        // shorter than the sag (ends of a loop meet: the first halving is always
        // made): those were 17 points in 3e-6 beside a crossing (ssi-corpus
        // c4_cone_apex_plane), and most of the curve's points.
        let short = depth > 0 && chord <= spacing.max_sag && sag <= spacing.max_sag;
        if (depth < 4 && !short)
            || (depth < 16 && (chord > spacing.max_step || sag > spacing.max_sag))
        {
            // Right half first on the stack, so halves come off left to right.
            stack.push((tm, m, t1, b, depth + 1));
            stack.push((t0, a, tm, m, depth + 1));
        } else {
            out.push((t1, b.0, b.1));
        }
    }
    out
}
