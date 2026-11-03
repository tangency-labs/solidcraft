//! Trim and Break on free-form curves (ellipses, splines, conics). The curve is cut into exact
//! pieces (cubic Béziers become single-span control splines, conic arcs conics), so the result
//! stays exact; the pieces replace the original curve.

use serde_json::{Value, json};
use solidcraft_geom::{Seg2, Vec2};
use solidcraft_sketch::{ConstraintKind, CurveKind, Sketch};

use super::sketch::add_c;
use crate::Result;
use crate::params::bad;

/// Exact pieces of a free-form curve (polyline lines for the few kinds without them).
fn pieces(sk: &Sketch, ci: usize) -> Vec<Seg2> {
    sk.exact_segs(ci).unwrap_or_else(|| sk.segs(ci))
}

/// Position along the pieces (piece index + local parameter) closest to `p`, and the distance.
fn locate(segs: &[Seg2], p: Vec2) -> (f64, f64) {
    segs.iter()
        .enumerate()
        .map(|(i, s)| {
            let t = s.closest_param(p);
            (i as f64 + t, s.point_at(t).dist(p))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap_or((0.0, f64::INFINITY))
}

fn point_at(segs: &[Seg2], u: f64) -> Vec2 {
    let n = segs.len();
    let i = (u.floor() as usize).min(n.saturating_sub(1));
    segs.get(i).map(|s| s.point_at((u - i as f64).clamp(0.0, 1.0))).unwrap_or_default()
}

/// Closest point on curve `ci` (exact pieces, or the analytic shape).
fn project(sk: &Sketch, ci: usize, p: Vec2) -> Vec2 {
    if sk.curves.get(ci).is_some_and(|c| c.kind.is_freeform()) {
        let segs = pieces(sk, ci);
        let (u, _) = locate(&segs, p);
        return point_at(&segs, u);
    }
    sk.shape(ci).map(|s| s.project(p)).unwrap_or(p)
}

/// Where other curves meet free-form curve `ci`: (position along it, the point, the other curve).
fn cuts(sk: &Sketch, ci: usize, segs: &[Seg2], only_freeform: bool) -> Vec<(f64, Vec2, usize)> {
    let mine = sk.polyline(ci);
    let tol = 1e-6;
    let mut out: Vec<(f64, Vec2, usize)> = Vec::new();
    let seg_x = |a: Vec2, b: Vec2, c: Vec2, d: Vec2| -> Option<Vec2> {
        let (r, s) = (b - a, d - c);
        let den = r.cross(s);
        if den.abs() < 1e-15 {
            return None;
        }
        let (t, u) = ((c - a).cross(s) / den, (c - a).cross(r) / den);
        ((-1e-9..=1.0 + 1e-9).contains(&t) && (-1e-9..=1.0 + 1e-9).contains(&u)).then(|| a + r * t)
    };
    for (oi, curve) in sk.curves.iter().enumerate() {
        if oi == ci || (only_freeform && !curve.kind.is_freeform()) {
            continue;
        }
        let other = sk.polyline(oi);
        let mut found: Vec<Vec2> = Vec::new();
        for w in mine.windows(2) {
            for v in other.windows(2) {
                if let Some(x) = seg_x(w[0], w[1], v[0], v[1]) {
                    found.push(x);
                }
            }
        }
        // End points of the other curve lying on this one.
        if let Some((a, b)) = sk.curves.get(oi).and_then(|c| c.kind.ends()) {
            for q in [a, b].iter().filter_map(|q| sk.point(*q)) {
                if locate(segs, q).1 < 1e-6 {
                    found.push(q);
                }
            }
        }
        for x in found {
            // Refine onto both exact curves by alternating projection.
            let mut q = x;
            for _ in 0..40 {
                let n = project(sk, ci, project(sk, oi, q));
                if n.dist(q) < 1e-13 {
                    q = n;
                    break;
                }
                q = n;
            }
            let (u, d) = locate(segs, q);
            if d < tol && project(sk, oi, q).dist(q) < tol && !out.iter().any(|(_, y, _)| y.dist(q) < tol * 10.0) {
                out.push((u, q, oi));
            }
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

/// Crossings for an analytic target with free-form cutters. Analytic-to-analytic crossings
/// remain in the shape intersection routine; this supplies the missing mixed pairs.
pub(super) fn analytic_cut_points(sk: &Sketch, ci: usize) -> Vec<(Vec2, usize)> {
    if !sk.curves.iter().any(|c| c.kind.is_freeform()) {
        return Vec::new();
    }
    cuts(sk, ci, &pieces(sk, ci), true).into_iter().map(|(_, q, oi)| (q, oi)).collect()
}

/// Exact sub-pieces between positions `ua` < `ub` (positions may run past the end on closed
/// curves; they wrap).
fn span(segs: &[Seg2], ua: f64, ub: f64) -> Vec<Seg2> {
    let n = segs.len() as f64;
    let mut out = Vec::new();
    let mut u = ua;
    while u < ub - 1e-12 && out.len() < 10_000 {
        let i = u.floor();
        let next = (i + 1.0).min(ub);
        let k = (i.rem_euclid(n)) as usize;
        let Some(s) = segs.get(k).copied() else { break };
        let (t0, t1) = (u - i, next - i);
        let mut piece = s;
        if t1 < 1.0 - 1e-12 {
            piece = piece.split_at(t1).0;
        }
        if t0 > 1e-12 {
            // Split at t0 measured on the (possibly shortened) piece.
            let tt = if t1 < 1.0 - 1e-12 { piece.closest_param(s.point_at(t0)) } else { t0 };
            piece = piece.split_at(tt).1;
        }
        out.push(piece);
        u = next;
    }
    out
}

/// Add pieces as curves, chained from point `start` (an existing point index or a new one) to
/// `end`. Returns the new curve indices.
fn add_pieces(sk: &mut Sketch, segs: &[Seg2], start: Option<usize>, end: Option<usize>, construction: bool) -> Result<Vec<usize>> {
    let mut made = Vec::new();
    let n = segs.len();
    let mut prev = match start {
        Some(i) => i,
        None => match segs.first() {
            Some(s) => sk.add_point(s.start(), None)?,
            None => return Ok(made),
        },
    };
    for (k, s) in segs.iter().enumerate() {
        let to = match (k + 1 == n, end) {
            (true, Some(e)) => e,
            _ => sk.add_point(s.end(), None)?,
        };
        let kind = match *s {
            Seg2::Line { .. } => CurveKind::Line { a: prev, b: to },
            Seg2::Cubic { p1, p2, .. } => {
                let (c1, c2) = (sk.add_point(p1, None)?, sk.add_point(p2, None)?);
                CurveKind::Spline { pts: vec![prev, c1, c2, to], control: true, degree: 3, periodic: false, nurbs: None }
            }
            Seg2::Conic { apex, w, .. } => {
                let x = sk.add_point(apex, None)?;
                CurveKind::Conic { a: prev, b: to, apex: x, rho: w / (1.0 + w) }
            }
            Seg2::Bezier { n, p } => {
                let k = (n as usize).clamp(1, solidcraft_geom::MAX_BEZIER_DEGREE);
                let mut pts = vec![prev];
                for q in p.iter().take(k).skip(1) {
                    pts.push(sk.add_point(*q, None)?);
                }
                pts.push(to);
                CurveKind::Spline { pts, control: true, degree: n, periodic: false, nurbs: None }
            }
            Seg2::Arc { .. } => return Err(bad("trim", "unexpected arc piece")),
        };
        let c = sk.add_curve(kind, None)?;
        if let Some(cu) = sk.curves.get_mut(c) {
            cu.construction = construction;
        }
        made.push(c);
        prev = to;
    }
    Ok(made)
}

/// Trim (`remove`) or break a free-form curve at the crossings around `at`.
pub(super) fn trim_or_break(sk: &mut Sketch, ci: usize, at: Vec2, remove: bool, cmd: &str) -> Result<Value> {
    modify(sk, ci, at, remove, None, cmd)
}

/// Break at explicit positions, projected onto the exact curve. Closed curves can be opened
/// at one point; open curves require interior positions.
pub(super) fn break_at_points(sk: &mut Sketch, ci: usize, points: &[Vec2], cmd: &str) -> Result<Value> {
    modify(sk, ci, Vec2::ZERO, false, Some(points), cmd)
}

fn modify(sk: &mut Sketch, ci: usize, at: Vec2, remove: bool, points: Option<&[Vec2]>, cmd: &str) -> Result<Value> {
    let segs = pieces(sk, ci);
    if segs.is_empty() {
        return Err(bad(cmd, "the curve has no shape"));
    }
    let n = segs.len() as f64;
    let closed = segs.first().zip(segs.last()).is_some_and(|(a, b)| a.start().dist(b.end()) < 1e-9);
    let cs: Vec<(f64, Vec2, Option<usize>)> = if let Some(points) = points {
        let mut positions: Vec<f64> = points.iter().map(|q| locate(&segs, *q).0).collect();
        if positions.iter().any(|u| !u.is_finite() || (!closed && (*u <= 1e-9 || *u >= n - 1e-9))) {
            return Err(bad(cmd, "break points must project inside the curve, away from its ends"));
        }
        if closed {
            for u in &mut positions {
                if *u >= n - 1e-9 {
                    *u = 0.0;
                }
            }
        }
        positions.sort_by(f64::total_cmp);
        positions.dedup_by(|a, b| (*a - *b).abs() <= 1e-9);
        positions.into_iter().map(|u| (u, point_at(&segs, u), None)).collect()
    } else {
        cuts(sk, ci, &segs, false).into_iter().filter(|c| closed || (c.0 > 1e-9 && c.0 < n - 1e-9)).map(|(u, q, c)| (u, q, Some(c))).collect()
    };
    let (uq, _) = locate(&segs, at);
    let ends = sk.curves.get(ci).and_then(|c| c.kind.ends());
    let construction = sk.curves.get(ci).is_some_and(|c| c.construction);
    // Intervals between consecutive cuts (closed curves wrap round).
    let mut bounds: Vec<(f64, Option<usize>)> = cs.iter().map(|c| (c.0, c.2)).collect();
    if bounds.is_empty() {
        if remove {
            sk.remove_curves(&[ci]);
            return Ok(json!({"deleted": true}));
        }
        return Err(bad(cmd, "nothing to break at there"));
    }
    let intervals: Vec<(f64, f64, Option<usize>, Option<usize>)> = if closed {
        if bounds.len() < 2 && remove {
            return Err(bad(cmd, "a closed curve needs two crossings to trim"));
        }
        let m = bounds.len();
        (0..m)
            .filter_map(|i| {
                let (a, ca) = *bounds.get(i)?;
                let (b, cb) = *bounds.get((i + 1) % m)?;
                let b = if b <= a + 1e-12 { b + n } else { b };
                Some((a, b, ca, cb))
            })
            .collect()
    } else {
        bounds.insert(0, (0.0, None));
        bounds.push((n, None));
        bounds.windows(2).map(|w| (w[0].0, w[1].0, w[0].1, w[1].1)).collect()
    };
    let inside = |(a, b, _, _): &(f64, f64, Option<usize>, Option<usize>)| {
        let u = if closed && uq < *a { uq + n } else { uq };
        u >= *a && u <= *b
    };
    let gone = if remove { intervals.iter().position(inside) } else { None };
    // Points at the cuts are shared by neighbouring intervals.
    let mut cut_pt: Vec<(f64, usize)> = Vec::new();
    for (u, q, _) in &cs {
        cut_pt.push((*u, sk.add_point(*q, None)?));
    }
    let point_for =
        |u: f64, cut_pt: &[(f64, usize)]| cut_pt.iter().find(|(x, _)| (x - u.rem_euclid(n)).abs() < 1e-9 || (x - u).abs() < 1e-9).map(|p| p.1);
    let mut made = Vec::new();
    for (k, iv) in intervals.iter().enumerate() {
        if Some(k) == gone {
            continue;
        }
        let (a, b, ..) = *iv;
        let start = if !closed && a <= 1e-12 { ends.map(|e| e.0) } else { point_for(a, &cut_pt) };
        let end = if !closed && b >= n - 1e-12 { ends.map(|e| e.1) } else { point_for(b, &cut_pt) };
        let sub = span(&segs, a, b);
        made.extend(add_pieces(sk, &sub, start, end, construction)?);
    }
    // Cut points stay on the curves that cut there.
    for ((_, _, oi), (_, pi)) in cs.iter().zip(&cut_pt) {
        let used = sk.curves.iter().any(|c| c.kind.uses(*pi));
        if used && let Some(oi) = oi {
            add_c(sk, ConstraintKind::PointOnCurve { p: *pi, c: *oi })?;
        }
    }
    let ids: Vec<String> = made.iter().filter_map(|c| sk.curves.get(*c).map(|c| c.id.clone())).collect();
    // The removed piece, for previews (`sketch.trim.path`).
    let removed: Vec<[f64; 2]> = gone
        .and_then(|k| intervals.get(k))
        .map(|(a, b, ..)| span(&segs, *a, *b).iter().flat_map(|s| s.polyline(1e-3)).map(|q| [q.x, q.y]).collect())
        .unwrap_or_default();
    sk.remove_curves(&[ci]);
    let unused: Vec<usize> = cut_pt.iter().map(|x| x.1).filter(|pi| !sk.curves.iter().any(|c| c.kind.uses(*pi))).collect();
    sk.remove_points(&unused);
    Ok(json!({"curves": ids, "removed": removed}))
}
