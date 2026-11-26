//! Trim and Break on free-form curves. Split the exact spans at the boundaries of the selected
//! piece. The remaining intervals of a spline each form one editable control spline.

use serde_json::{Value, json};
use solidcore_record::brep::{Curve as RecordCurve, Frame};
use solidcore_record::{Plane, Seg2, Vec2, Vec3};
use solidcraft_geom::prelude::*;
use solidcraft_sketch::{ConstraintKind, CurveKind, Nurbs, Shape, Sketch};

use super::sketch_modify::{Boundary, boundary_points, pin, support};
use super::sketch_rational;
use crate::Result;
use crate::params::bad;

/// An imported weighted spline may have no polynomial Bézier or rational quadratic
/// spans. Its own NURBS record still supports exact projection, crossing and subdivision.
#[derive(Clone, Debug)]
enum Geometry {
    Spans(Vec<Seg2>),
    Rational(RecordCurve),
}

impl Geometry {
    fn new(sk: &Sketch, ci: usize, cmd: &str) -> Result<Self> {
        if let Some(segs) = sk.exact_segs(ci) {
            return Ok(Self::Spans(segs));
        }
        if sk.curves.get(ci).is_some_and(|curve| curve.kind.is_freeform()) {
            return sketch_rational::record(sk, ci).map(Self::Rational).ok_or_else(|| bad(cmd, "invalid free-form curve"));
        }
        Ok(Self::Spans(sk.segs(ci)))
    }

    fn limit(&self) -> f64 {
        match self {
            Self::Spans(segs) => segs.len() as f64,
            Self::Rational(_) => 1.0,
        }
    }

    fn closed(&self, sk: &Sketch, ci: usize) -> bool {
        match self {
            Self::Spans(segs) => segs.first().zip(segs.last()).is_some_and(|(a, b)| a.start().dist(b.end()) < 1e-9),
            Self::Rational(_) => {
                sk.curves.get(ci).and_then(|c| c.kind.ends()).and_then(|(a, b)| sk.point(a).zip(sk.point(b))).is_some_and(|(a, b)| a.dist(b) < 1e-9)
            }
        }
    }

    fn project(&self, at: Vec2, cmd: &str) -> Result<(f64, Vec2)> {
        match self {
            Self::Spans(segs) => segs
                .iter()
                .enumerate()
                .map(|(i, seg)| {
                    let t = seg.closest_param(at);
                    (i as f64 + t, seg.point_at(t))
                })
                .min_by(|a, b| a.1.dist(at).total_cmp(&b.1.dist(at)))
                .ok_or_else(|| bad(cmd, "the curve has no shape")),
            Self::Rational(curve) => {
                let hit = solidcraft_kernel::curves::closest_point(curve, [at.x, at.y, 0.0], 1e-9)
                    .map_err(|e| bad(cmd, e.to_string()))?
                    .ok_or_else(|| bad(cmd, "cannot locate point on spline"))?;
                Ok((hit.parameter, Vec2::new(hit.point[0], hit.point[1])))
            }
        }
    }

    fn path(&self, a: f64, b: f64, cmd: &str) -> Result<Vec<Vec2>> {
        match self {
            Self::Spans(segs) => Ok(span(segs, a, b).iter().flat_map(|s| s.polyline(1e-3)).collect()),
            Self::Rational(curve) => sketch_rational::preview(curve, [a, b], cmd),
        }
    }

    fn records(&self, cmd: &str) -> Result<Vec<RecordCurve>> {
        match self {
            Self::Spans(segs) => segs.iter().map(|seg| recorded(seg, cmd)).collect(),
            Self::Rational(curve) => Ok(vec![curve.clone()]),
        }
    }
}

/// Crossings of exact free-form spans. The record keeps each Bézier or rational conic bounded
/// to its own knot domain, so crossings on another span's continuation are excluded.
fn recorded(seg: &Seg2, cmd: &str) -> Result<solidcore_record::brep::Curve> {
    let curve =
        solidcraft_kernel::services::sweep::Curve::from_seg2(seg, &solidcore_record::Plane::XY).ok_or_else(|| bad(cmd, "invalid curve span"))?;
    curve.to_record().map(|record| record.curve).map_err(|error| bad(cmd, error.to_string()))
}

/// Exact intersections of a free-form curve and an analytic sketch curve.
fn support_hits(geo: &Geometry, shape: &Shape, infinite: bool, budget: &mut usize, cmd: &str) -> Result<Vec<Vec2>> {
    let mut out = Vec::new();
    match geo {
        Geometry::Spans(segs) => {
            for seg in segs {
                out.extend(support::hits_for(cmd, shape, seg, budget)?);
            }
        }
        Geometry::Rational(curve) => match *shape {
            Shape::Line { a, b } => {
                let d = (b - a).normalized().ok_or_else(|| bad(cmd, "zero-length cutting line"))?;
                let plane = Plane { origin: Vec3::new(a.x, a.y, 0.0), x: Vec3::new(d.x, d.y, 0.0), y: Vec3::Z };
                let hits = solidcraft_kernel::curves::plane_crossings(curve, [0.0, 1.0], &plane).map_err(|e| bad(cmd, e.to_string()))?;
                out.extend(hits.into_iter().map(|hit| a + d * hit.x));
            }
            Shape::Round { c, r, .. } => {
                let circle =
                    RecordCurve::Circle { frame: Frame { origin: [c.x, c.y, 0.0], x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 1.0, 0.0] }, radius: r };
                out.extend(record_crossings(curve, &circle, cmd)?);
            }
        },
    }
    out.retain(|q| infinite || shape.dist(*q) < 1e-6);
    Ok(out)
}

fn record_crossings(a: &RecordCurve, b: &RecordCurve, cmd: &str) -> Result<Vec<Vec2>> {
    let hits = solidcraft_kernel::curves::crossings(a, b, None, 1e-7).map_err(|e| bad(cmd, e.to_string()))?;
    Ok(hits.into_iter().map(|hit| Vec2::new(hit.on_a[0], hit.on_a[1])).collect())
}

/// Where other curves meet the target. Intersect the same supports in either target order.
fn cuts(sk: &Sketch, ci: usize, geo: &Geometry, cmd: &str) -> Result<Vec<Boundary>> {
    let mut out = Vec::new();
    let target = sk.curves.get(ci).ok_or_else(|| bad(cmd, "curve"))?;
    let shape = sk.shape(ci);
    let (major, minor) = sk.ellipse_axis_lines(ci).unwrap_or((None, None));
    let own_axes = [major.map(|(l, _)| l), minor.map(|(l, ..)| l)];
    let mut budget = 1_000_000usize;
    let mut records = None;
    for (oi, curve) in sk.curves.iter().enumerate() {
        if oi == ci
            || (shape.is_some() && !curve.kind.is_freeform())
            || own_axes.contains(&Some(oi))
            || curve.spline_guide_owner() == Some(target.id.as_str())
        {
            continue;
        }
        let mut found = if let Some(support) = &shape {
            support_hits(&Geometry::new(sk, oi, cmd)?, support, target.infinite, &mut budget, cmd)?
        } else if let Some(support) = sk.shape(oi) {
            support_hits(geo, &support, curve.infinite, &mut budget, cmd)?
        } else {
            if records.is_none() {
                records = Some(geo.records(cmd)?);
            }
            let other = Geometry::new(sk, oi, cmd)?.records(cmd)?;
            let mut hits = Vec::new();
            for a in records.iter().flatten() {
                for b in &other {
                    budget = budget.checked_sub(1).ok_or_else(|| bad(cmd, "intersection work limit exceeded"))?;
                    hits.extend(record_crossings(a, b, cmd)?);
                }
            }
            hits
        };
        if let Some((a, b)) = curve.kind.ends() {
            found.extend([a, b].into_iter().filter_map(|p| sk.point(p)));
        }
        for position in found {
            let (parameter, q) = geo.project(position, cmd)?;
            if q.dist(position) < 1e-6 && !out.iter().any(|c: &Boundary| c.position.dist(position) < 1e-6) {
                out.push(Boundary { parameter, position: q, point: None, cutter: Some(oi) });
            }
        }
    }
    Ok(out)
}

pub(super) fn analytic_cut_points(sk: &Sketch, ci: usize, cmd: &str) -> Result<Vec<(Vec2, usize)>> {
    if !sk.curves.iter().any(|c| c.kind.is_freeform()) {
        return Ok(Vec::new());
    }
    Ok(cuts(sk, ci, &Geometry::new(sk, ci, cmd)?, cmd)?.into_iter().filter_map(|c| Some((c.position, c.cutter?))).collect())
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
                CurveKind::Spline { pts: vec![prev, c1, c2, to], control: true, degree: 3, periodic: false, nurbs: None, handles: Vec::new() }
            }
            Seg2::Conic { apex, w, .. } => {
                let x = sk.add_point(apex, None)?;
                CurveKind::Conic { a: prev, b: to, apex: x, rho: w / (1.0 + w) }
            }
            Seg2::Bezier { n, p } => {
                let k = (n as usize).clamp(1, solidcore_record::MAX_BEZIER_DEGREE);
                let mut pts = vec![prev];
                for q in p.iter().take(k).skip(1) {
                    pts.push(sk.add_point(*q, None)?);
                }
                pts.push(to);
                CurveKind::Spline { pts, control: true, degree: n, periodic: false, nurbs: None, handles: Vec::new() }
            }
            Seg2::Arc { .. } => return Err(bad("sketch.trim", "unexpected arc piece")),
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

fn trim_cuts(sk: &Sketch, ci: usize, geo: &Geometry, closed: bool, cmd: &str) -> Result<Vec<Boundary>> {
    let n = geo.limit();
    let mut bounds = cuts(sk, ci, geo, cmd)?;
    for (point, at) in boundary_points(sk, ci) {
        let (parameter, q) = geo.project(at, cmd)?;
        if q.dist(at) <= 1e-6 {
            Boundary::add_point(&mut bounds, parameter, at, point);
        }
    }
    bounds.retain(|cut| closed || (cut.parameter > 1e-9 && cut.parameter < n - 1e-9));
    bounds.sort_by(|a, b| a.parameter.total_cmp(&b.parameter));
    Ok(bounds)
}

/// The two cuts enclosing the pick, and the interval between them in path order.
fn picked(cuts: &[Boundary], u: f64, closed: bool, n: f64) -> ([Option<Boundary>; 2], (f64, f64)) {
    let pair = Boundary::around(cuts, u, closed);
    let a = pair[0].map_or(0.0, |c| c.parameter);
    let b = pair[1].map_or(n, |c| c.parameter);
    (pair, (a, if closed && b <= a { b + n } else { b }))
}

/// Store each selected interval of a spline as one editable spline. Successive Bézier spans
/// share their end control point. A knot of multiplicity degree joins their control polygons
/// without changing the geometry, including the weights of rational quadratic spans.
fn add_spline(sk: &mut Sketch, segs: &[Seg2], start: Option<usize>, end: Option<usize>, construction: bool, cmd: &str) -> Result<usize> {
    let first = segs.first().ok_or_else(|| bad(cmd, "empty spline piece"))?;
    let mut pts = vec![match start {
        Some(p) => p,
        None => sk.add_point(first.start(), None)?,
    }];
    let mut weights = vec![1.0];
    let mut knots = vec![0.0];
    let mut degree = 0;
    for (k, seg) in segs.iter().enumerate() {
        let (inner, w): (Vec<Vec2>, f64) = match *seg {
            Seg2::Line { .. } => (Vec::new(), 1.0),
            Seg2::Conic { apex, w, .. } => (vec![apex], w),
            Seg2::Cubic { p1, p2, .. } => (vec![p1, p2], 1.0),
            Seg2::Bezier { n, ref p } => (p.get(1..usize::from(n)).ok_or_else(|| bad(cmd, "invalid spline degree"))?.to_vec(), 1.0),
            Seg2::Arc { .. } => return Err(bad(cmd, "unexpected arc in a spline")),
        };
        if k > 0 && degree != inner.len() + 1 {
            return Err(bad(cmd, "inconsistent spline degrees"));
        }
        degree = inner.len() + 1;
        for q in inner {
            pts.push(sk.add_point(q, None)?);
            weights.push(w);
        }
        let to = if k + 1 == segs.len() { end } else { None };
        pts.push(match to {
            Some(p) => p,
            None => sk.add_point(seg.end(), None)?,
        });
        weights.push(1.0);
        knots.extend(std::iter::repeat_n(k as f64, degree));
    }
    knots.extend(std::iter::repeat_n(segs.len() as f64, degree + 1));
    if weights.iter().all(|w| *w == 1.0) {
        weights.clear();
    }
    let degree = degree as u8;
    let nurbs = Nurbs { knots, weights };
    nurbs.check(pts.len(), degree)?;
    let nurbs = (segs.len() > 1 || !nurbs.weights.is_empty()).then_some(nurbs);
    let c = sk.add_curve(CurveKind::Spline { pts, degree, control: true, periodic: false, handles: Vec::new(), nurbs }, None)?;
    if let Some(curve) = sk.curves.get_mut(c) {
        curve.construction = construction;
    }
    Ok(c)
}

/// What Trim's hover keeps of a free-form curve while the sketch is unchanged: the cuts are the
/// costly part, and [`FreeformTrim::piece`] only looks the cursor up among them.
#[derive(Clone, Debug)]
pub(super) struct FreeformTrim {
    geo: Geometry,
    closed: bool,
    cuts: Vec<Boundary>,
    /// The curve as points where nothing cuts it, so a click deletes all of it.
    whole: Option<Vec<Vec2>>,
}

impl FreeformTrim {
    pub(super) fn new(sk: &Sketch, ci: usize) -> Option<Self> {
        let geo = Geometry::new(sk, ci, "sketch.trim").ok()?;
        if geo.limit() == 0.0 {
            return None;
        }
        let closed = geo.closed(sk, ci);
        let cuts = trim_cuts(sk, ci, &geo, closed, "sketch.trim").ok()?;
        let whole = (cuts.is_empty() || (closed && cuts.len() < 2)).then(|| sk.polyline(ci));
        Some(Self { geo, closed, cuts, whole })
    }

    /// The piece a click at `at` removes, the one `sketch.trim` reports.
    pub(super) fn piece(&self, at: Vec2) -> Option<Vec<Vec2>> {
        if let Some(whole) = &self.whole {
            return Some(whole.clone());
        }
        let (uq, _) = self.geo.project(at, "sketch.trim").ok()?;
        let (_, (a, b)) = picked(&self.cuts, uq, self.closed, self.geo.limit());
        self.geo.path(a, b, "sketch.trim").ok()
    }
}

fn modify(sk: &mut Sketch, ci: usize, at: Vec2, remove: bool, points: Option<&[Vec2]>, cmd: &str) -> Result<Value> {
    let geo = Geometry::new(sk, ci, cmd)?;
    if geo.limit() == 0.0 {
        return Err(bad(cmd, "the curve has no shape"));
    }
    let n = geo.limit();
    let closed = geo.closed(sk, ci);
    let mut removed_interval = None;
    let cs: Vec<Boundary> = if let Some(points) = points {
        let mut positions = Vec::new();
        for q in points {
            let (u, position) = geo.project(*q, cmd)?;
            if !u.is_finite() || (!closed && (u <= 1e-9 || u >= n - 1e-9)) {
                return Err(bad(cmd, "break points must project inside the curve, away from its ends"));
            }
            let parameter = if closed && u >= n - 1e-9 { 0.0 } else { u };
            positions.push(Boundary { parameter, position, point: None, cutter: None });
        }
        positions.sort_by(|a, b| a.parameter.total_cmp(&b.parameter));
        positions.dedup_by(|a, b| (a.parameter - b.parameter).abs() <= 1e-9);
        positions
    } else {
        let cuts = trim_cuts(sk, ci, &geo, closed, cmd)?;
        let (pair, interval) = picked(&cuts, geo.project(at, cmd)?.0, closed, n);
        removed_interval = remove.then_some(interval);
        let mut pair: Vec<_> = pair.into_iter().flatten().collect();
        pair.sort_by(|a, b| a.parameter.total_cmp(&b.parameter));
        pair
    };
    if cs.is_empty() {
        if remove {
            sk.remove_curves(&[ci]);
            return Ok(json!({"deleted": true}));
        }
        return Err(bad(cmd, "nothing to break at there"));
    }
    let curve = sk.curves.get(ci).ok_or_else(|| bad(cmd, "curve"))?;
    let ends = curve.kind.ends();
    let construction = curve.construction;
    let join_spans = points.is_none() && matches!(curve.kind, CurveKind::Spline { .. });
    let mut bounds = Vec::new();
    if !closed {
        bounds.push((0.0, ends.map(|e| e.0)));
    }
    for cut in &cs {
        let cutter_ends = cut.cutter.and_then(|i| sk.curves.get(i)?.kind.ends());
        let marker = cut.point.filter(|p| cutter_ends.is_none_or(|(a, b)| *p != a && *p != b));
        let p = match marker {
            Some(p) => p,
            None => sk.add_point(cut.position, None)?,
        };
        if let Some(c) = cut.cutter {
            pin(sk, p, c)?;
        }
        bounds.push((cut.parameter, Some(p)));
    }
    if closed {
        if let Some(&(u, p)) = bounds.first() {
            bounds.push((u + n, p));
        }
    } else {
        bounds.push((n, ends.map(|e| e.1)));
    }
    let mut made = Vec::new();
    for pair in bounds.windows(2) {
        let [(a, start), (b, end)] = pair else { continue };
        if removed_interval.is_some_and(|(from, _)| from == *a) {
            continue;
        }
        match &geo {
            Geometry::Rational(curve) => made.push(sketch_rational::add_interval(sk, curve, [*a, *b], *start, *end, construction, cmd)?),
            Geometry::Spans(segs) => {
                let sub = span(segs, *a, *b);
                if join_spans {
                    made.push(add_spline(sk, &sub, *start, *end, construction, cmd)?);
                } else {
                    made.extend(add_pieces(sk, &sub, *start, *end, construction)?);
                }
            }
        }
    }
    // The original spline goes away. A cut can now use the held point as its endpoint.
    // Keep the IDs of other point-on-curve constraints by moving them to their new pieces.
    let mut held: std::collections::BTreeMap<_, _> = boundary_points(sk, ci).collect();
    for p in cs.iter().filter_map(|cut| cut.point) {
        held.remove(&p);
    }
    for c in &made {
        if let Some((a, b)) = sk.curves.get(*c).and_then(|curve| curve.kind.ends()) {
            held.remove(&a);
            held.remove(&b);
        }
    }
    for c in &made {
        if held.is_empty() {
            break;
        }
        let piece = Geometry::new(sk, *c, cmd)?;
        let mut belongs = std::collections::BTreeSet::new();
        for (&p, &at) in &held {
            if piece.project(at, cmd)?.1.dist(at) <= 1e-6 {
                belongs.insert(p);
            }
        }
        for k in &mut sk.constraints {
            if let ConstraintKind::PointOnCurve { p, c: on } = &mut k.kind
                && *on == ci
                && belongs.contains(p)
            {
                *on = *c;
            }
        }
        held.retain(|p, _| !belongs.contains(p));
    }
    let ids: Vec<String> = made.iter().filter_map(|c| sk.curves.get(*c).map(|c| c.id.clone())).collect();
    // The removed piece, for previews (`sketch.trim.path`).
    let removed = removed_interval.map(|(a, b)| geo.path(a, b, cmd)).transpose()?.unwrap_or_default();
    let removed: Vec<[f64; 2]> = removed.into_iter().map(|q| [q.x, q.y]).collect();
    sk.remove_curves(&[ci]);
    Ok(json!({"curves": ids, "removed": removed}))
}
