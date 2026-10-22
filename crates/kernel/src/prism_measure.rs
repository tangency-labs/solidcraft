//! Independent current-geometry intervals for uniform prisms, read off the body's schema 4
//! record. These are not mesh measurements or certificates of construction history.
use crate::prism_curve::{Run, segment};
use solidcraft_geom::brep::eval::CurveEval;
use solidcraft_geom::brep::{Brep, Curve, Loop, Shell, Surface};
use solidcraft_geom::{Plane, Ring, Seg2, Vec2, Vec3};

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

#[derive(Clone, Copy, Debug)]
struct Interval {
    lo: f64,
    hi: f64,
}

impl Interval {
    fn enclosing(value: f64, error: f64) -> Option<Self> {
        if !value.is_finite() || !error.is_finite() || error < 0. {
            return None;
        }
        // The explicit extra arithmetic allowance also covers subtraction and
        // accumulation; do not round an uncertainty inward at its endpoints.
        let padding = 16. * f64::EPSILON * (value.abs() + error + 1.);
        Some(Self { lo: value - error - padding, hi: value + error + padding })
    }

    fn add(self, other: Self) -> Option<Self> {
        Self::enclosing((self.lo + self.hi + other.lo + other.hi) * 0.5, (self.hi - self.lo + other.hi - other.lo) * 0.5)
    }
}

/// Signed Green integral of a fitted segment with the *whole-curve* spatial
/// uncertainty. Angular/longitudinal monotonicity is mandatory: a curve in a
/// thin corridor could otherwise retrace it arbitrarily often.
fn green_interval(segment: &Seg2, tolerance: f64) -> Option<Interval> {
    let (length, a, b) = (segment.length(), segment.start(), segment.end());
    let extra = match *segment {
        Seg2::Line { .. } => 0.,
        Seg2::Arc { center, radius, .. } => center.len() + radius,
        _ => return None,
    };
    let scale = length + a.len() + b.len() + extra;
    // A monotone line differs from its chord by a strip; a radial-monotone
    // arc contributes 1/2 integral r(theta)^2 dtheta. The endpoint wedges,
    // coordinate uncertainty, and angular endpoint uncertainty are included
    // by this conservative bound (rather than treating the fitted arc exact).
    let error = 8. * tolerance * scale + 64. * tolerance * tolerance + 1024. * f64::EPSILON * (1. + scale * scale);
    Interval::enclosing(segment.area_term(), error)
}

struct CapLoop {
    segments: Ring<Seg2>,
    area: Interval,
}

/// Cross-product cancellation belongs to the surface representation, not
/// just the later incidence dot product. Reject ill-conditioned plane bases.
/// The natural normal of the plane spanned by `u` and `v`, and a bound on its rounding error.
fn plane_normal(u: Vec3, v: Vec3) -> Option<(Vec3, Vec3)> {
    let cross = u.cross(v);
    let length = cross.len();
    if !length.is_finite() || length <= 1e-6 * u.len() * v.len() {
        return None;
    }
    let n = cross.normalized()?;
    let ce = Vec3::new((u.y * v.z).abs() + (u.z * v.y).abs(), (u.z * v.x).abs() + (u.x * v.z).abs(), (u.x * v.y).abs() + (u.y * v.x).abs())
        * (256. * f64::EPSILON);
    let ne = ce * (1. / length) + Vec3::new(n.x.abs(), n.y.abs(), n.z.abs()) * (ce.len() / length);
    (ne.x.is_finite() && ne.y.is_finite() && ne.z.is_finite()).then_some((n, ne))
}

/// A planar surface's origin and spanning vectors (as its record holds them).
fn plane_of(s: &Surface) -> Option<(Vec3, Vec3, Vec3)> {
    match s {
        Surface::Plane { origin, u_axis, v_axis } => Some((v3(*origin), v3(*u_axis), v3(*v_axis))),
        Surface::PlaneThrough { origin, u_point, v_point } => {
            let o = v3(*origin);
            Some((o, v3(*u_point) - o, v3(*v_point) - o))
        }
        _ => None,
    }
}

/// `v` (a point when `w` is 1, a direction when 0) mapped by the column-major affine `m`; `None`
/// where the map is not affine there.
fn placed(m: &[[f64; 4]; 4], v: Vec3, w: f64) -> Option<Vec3> {
    let c = |k: usize| m[0][k] * v.x + m[1][k] * v.y + m[2][k] * v.z + m[3][k] * w;
    (c(3) == w).then(|| Vec3::new(c(0), c(1), c(2)))
}

/// A revolution of a line: the line's ends, the axis origin and unit axis, the placement.
type Revolved<'a> = (Vec3, Vec3, Vec3, Vec3, &'a [[f64; 4]; 4]);

fn revolved_line(s: &Surface) -> Option<Revolved<'_>> {
    let Surface::PlacedRevolution { profile, origin, axis, matrix, .. } = s else { return None };
    let Curve::Line { start, end } = &**profile else { return None };
    Some((v3(*start), v3(*end), v3(*origin), v3(*axis).normalized()?, matrix))
}

/// A spline surface's control net, `(point, weight)` with `[i][j]` i along u, and its knots.
type Net<'a> = (Vec<Vec<(Vec3, f64)>>, &'a [f64], &'a [f64]);

fn net_of(s: &Surface) -> Option<Net<'_>> {
    match s {
        Surface::Spline { u_knots, v_knots, points } => {
            Some((points.iter().map(|r| r.iter().map(|p| (v3(*p), 1.)).collect()).collect(), u_knots, v_knots))
        }
        Surface::RationalSpline { u_knots, v_knots, homogeneous } => Some((
            homogeneous.iter().map(|r| r.iter().map(|p| (Vec3::new(p[0] / p[3], p[1] / p[3], p[2] / p[3]), p[3])).collect()).collect(),
            u_knots,
            v_knots,
        )),
        _ => None,
    }
}

/// A whole positive control-net bound for the wall's projected variation.
/// A sampled normal or a vertex-only test is deliberately insufficient.
fn wall_error(surface: &Surface, d: Vec3, height: f64, work: &mut usize) -> Option<f64> {
    if let Some((_, u, v)) = plane_of(surface) {
        let (n, ne) = plane_normal(u, v)?;
        let terms = (n.x * d.x).abs() + (n.y * d.y).abs() + (n.z * d.z).abs();
        let error = ne.x * d.x.abs() + ne.y * d.y.abs() + ne.z * d.z.abs() + 1024. * f64::EPSILON * terms;
        return Some((n.dot(d).abs() + error) * height * 2.);
    }
    if let Surface::Cylinder { frame, .. } = surface {
        let a = v3(frame.x_axis).cross(v3(frame.y_axis)).normalized()?;
        return Some(a.cross(d).len() * height * 4.);
    }
    if let Some((start, end, _, a, matrix)) = revolved_line(surface) {
        let m = (end - start).normalized()?;
        let pa = placed(matrix, a, 0.)?.normalized()?;
        return Some((a.cross(m).len() + pa.cross(d).len()) * height * 4.);
    }
    let (points, _, _) = net_of(surface)?;
    let (rows, cols) = (points.len(), points.first()?.len());
    let count = rows.checked_mul(cols)?;
    if rows < 2 || cols < 2 || count > 16_384 || !points.iter().all(|r| r.len() == cols) {
        return None;
    }
    *work = work.checked_sub(count.checked_mul(4)?)?;
    if points.iter().flatten().any(|(p, w)| !p.is_finite() || !w.is_finite() || *w <= 0.) {
        return None;
    }
    let max_w = points.iter().flatten().map(|p| p.1).fold(0., f64::max);
    let min_w = points.iter().flatten().map(|p| p.1 / max_w).fold(1., f64::min);
    if min_w < 1e-6 {
        return None;
    }
    let magnitude = points.iter().flatten().map(|p| p.0.len()).fold(1., f64::max);
    let mut best = f64::INFINITY;
    for transpose in [false, true] {
        let at = |i: usize, j: usize| if transpose { points.get(j)?.get(i).copied() } else { points.get(i)?.get(j).copied() };
        let (groups, length) = if transpose { (cols, rows) } else { (rows, cols) };
        let mut variation: f64 = 0.;
        let mut rank_error: f64 = 0.;
        let mut monotone = true;
        let mut axial_sign = 0.;
        for i in 0..groups {
            let (base, bw) = at(i, 0)?;
            let mut previous = base.dot(d);
            for j in 0..length {
                let (point, w) = at(i, j)?;
                let step = point.dot(d) - previous;
                previous = point.dot(d);
                if step.abs() > 2048. * f64::EPSILON * magnitude {
                    if axial_sign * step < 0. {
                        monotone = false;
                    }
                    axial_sign = step.signum();
                }
                variation = variation.max((point - base).cross(d).len());
                let lhs = w / max_w * (at(0, 0)?.1 / max_w);
                let rhs = bw / max_w * (at(0, j)?.1 / max_w);
                rank_error = rank_error.max((lhs - rhs).abs());
            }
            if (at(i, length - 1)?.0 - base).dot(d).abs() < height * 0.5 {
                monotone = false;
            }
        }
        if !monotone || axial_sign == 0. {
            continue;
        }
        // Positive weights turn the control hull into a positional bound.
        // Rank-one weight error perturbs the normalized convex coefficients;
        // the positive minimum weight prevents an unbounded denominator.
        let error = 2. * variation + 8. * magnitude * rank_error / min_w.powi(2) + 2048. * f64::EPSILON * magnitude / min_w.powi(2);
        best = best.min(error);
    }
    best.is_finite().then_some(best)
}

fn segment_same(a: &Seg2, b: &Seg2, tolerance: f64) -> bool {
    match (*a, *b) {
        (Seg2::Line { a, b }, Seg2::Line { a: c, b: d }) => {
            (a.dist(c) <= tolerance && b.dist(d) <= tolerance) || (a.dist(d) <= tolerance && b.dist(c) <= tolerance)
        }
        (Seg2::Arc { center, radius, start, sweep }, Seg2::Arc { center: c, radius: r, start: t, sweep: s }) => {
            center.dist(c) <= tolerance
                && (radius - r).abs() <= tolerance
                && (sweep.abs() - s.abs()).abs() * radius <= tolerance
                && ((Vec2::new(start.cos(), start.sin()) * radius).dist(Vec2::new(t.cos(), t.sin()) * r) <= tolerance
                    || (Vec2::new(start.cos(), start.sin()) * radius).dist(Vec2::new((t + s).cos(), (t + s).sin()) * r) <= tolerance)
        }
        _ => false,
    }
}

/// Does the wall (`surface` over the face's `domain`) carry the cap boundary `cap` (an edge of a
/// cap at level `level`, read in `plane`) between the cap levels `levels`?
fn wall_supports_cap(
    surface: &Surface,
    domain: &[[f64; 2]; 2],
    cap: &[Seg2],
    level: f64,
    plane: &Plane,
    levels: (f64, f64),
    tol: f64,
    work: &mut usize,
) -> Option<bool> {
    if let Some((origin, u, v)) = plane_of(surface) {
        let (n, ne) = plane_normal(u, v)?;
        for s in cap {
            let Seg2::Line { a, b } = *s else { return Some(false) };
            for q in [a, b] {
                let point = plane.to_world(q) + plane.normal() * level;
                let delta = point - origin;
                let error =
                    1024. * f64::EPSILON * (1. + point.len() + origin.len()) + delta.x.abs() * ne.x + delta.y.abs() * ne.y + delta.z.abs() * ne.z;
                if delta.dot(n).abs() + error > tol * 8. {
                    return Some(false);
                }
            }
        }
        return Some(true);
    }
    let on_circle = |center: Vec2, radius: f64| {
        cap.iter().all(|s| matches!(*s, Seg2::Arc { center: c, radius: r, .. } if c.dist(center) + (r - radius).abs() <= tol * 8.))
    };
    let spans = |ha: f64, hb: f64| !(ha.min(hb) > levels.0 + tol * 8. || ha.max(hb) < levels.1 - tol * 8.);
    if let Surface::Cylinder { frame, radius } = surface {
        let (o, x, y) = (v3(frame.origin), v3(frame.x_axis), v3(frame.y_axis));
        let z = x.cross(y);
        // The face's heights along the axis: the rest of the cylinder carries no cap.
        let [v0, v1] = domain[1];
        if !spans(plane.height(o + z * v0), plane.height(o + z * v1)) {
            return Some(false);
        }
        if x.dot(plane.normal()).abs() * radius > tol || y.dot(plane.normal()).abs() * radius > tol {
            return None;
        }
        return Some(on_circle(plane.to_local(o), *radius));
    }
    if let Some((start, end, o, axis, matrix)) = revolved_line(surface) {
        let delta = start - o;
        let radial = (delta - axis * delta.dot(axis)).len();
        let u = axis.any_perp();
        let v = axis.cross(u);
        let center = plane.to_local(placed(matrix, o, 1.)?);
        if !spans(plane.height(placed(matrix, start, 1.)?), plane.height(placed(matrix, end, 1.)?)) {
            return Some(false);
        }
        let (pu, pv) = (placed(matrix, u, 0.)?, placed(matrix, v, 0.)?);
        let radius = pu.len() * radial;
        if (pu.len() - pv.len()).abs() * radial > tol
            || pu.dot(pv).abs() * radial > tol
            || pu.dot(plane.normal()).abs() * radial > tol
            || pv.dot(plane.normal()).abs() * radial > tol
        {
            return None;
        }
        return Some(on_circle(center, radius));
    }
    let (points, u_knots, v_knots) = net_of(surface)?;
    let (rows, cols) = (points.len(), points.first()?.len());
    *work = work.checked_sub(rows.checked_mul(cols)?.checked_mul(4)?)?;
    let at = |i: usize, j: usize| -> Option<(Vec3, f64)> { points.get(i)?.get(j).copied() };
    for transpose in [false, true] {
        let point = |i, j| if transpose { at(j, i) } else { at(i, j) };
        let (groups, length) = if transpose { (cols, rows) } else { (rows, cols) };
        // The first measurement subset uses a degree-one axial ruling. Its
        // positive rational interpolation is monotone and reaches both caps;
        // projected incidence alone would miss an axially displaced support.
        if !(2..=4096).contains(&groups) || length != 2 {
            continue;
        }
        let axial_knots = if transpose { u_knots } else { v_knots };
        if axial_knots.len() != 4
            || axial_knots.first() != axial_knots.get(1)
            || axial_knots.get(2) != axial_knots.last()
            || axial_knots.get(1)? >= axial_knots.get(2)?
        {
            continue;
        }
        let mut projected = Vec::new();
        let mut valid = true;
        let mut increasing = None;
        for i in 0..groups {
            let (base, w) = point(i, 0)?;
            let h0 = plane.height(base);
            let h1 = plane.height(point(i, 1)?.0);
            if !spans(h0, h1) || increasing.is_some_and(|s| s != (h1 > h0)) {
                valid = false;
                break;
            }
            increasing = Some(h1 > h0);
            let p = plane.to_local(base);
            for j in 0..length {
                if plane.to_local(point(i, j)?.0).dist(p) > tol * 8. {
                    valid = false;
                    break;
                }
            }
            let q = plane.to_world(p);
            projected.push([q.x * w, q.y * w, q.z * w, w]);
        }
        if !valid {
            continue;
        }
        let knots = if transpose { v_knots } else { u_knots };
        if knots.len() > 8192 {
            return None;
        }
        *work = work.checked_sub(knots.len())?;
        let degree = knots.len().checked_sub(projected.len() + 1)?;
        let interval = [*knots.get(degree)?, *knots.get(projected.len())?];
        if !(interval[0] < interval[1]) {
            continue;
        }
        let support = Curve::RationalSpline { knots: knots.to_vec(), homogeneous: projected };
        let eval = CurveEval::new(&support).ok()?;
        let ends = [eval.point(interval[0])?, eval.point(interval[1])?];
        let Some(fit) = segment(&Run { curve: &support, interval, forward: true, ends }, plane, tol, work) else {
            continue;
        };
        let fit_loop = CapLoop { segments: Ring::from(fit.clone()), area: Interval::enclosing(0., 0.)? };
        if let Some((center, radius)) = circle(&fit_loop, tol * 8.)
            && on_circle(center, radius)
        {
            return Some(true);
        }
        let same = loops_same(
            &CapLoop { segments: Ring::from(cap.to_vec()), area: Interval::enclosing(0., 0.)? },
            &CapLoop { segments: Ring::from(fit), area: Interval::enclosing(0., 0.)? },
            tol * 8.,
            work,
        )?;
        if same {
            return Some(true);
        }
    }
    Some(false)
}

fn circle(lp: &CapLoop, tolerance: f64) -> Option<(Vec2, f64)> {
    let Seg2::Arc { center, radius, sweep: first, .. } = *lp.segments.first()? else { return None };
    let mut sweep = 0.;
    for s in &lp.segments {
        let Seg2::Arc { center: c, radius: r, sweep: a, .. } = *s else { return None };
        if c.dist(center) > tolerance || (r - radius).abs() > tolerance || a * first <= 0. {
            return None;
        }
        sweep += a;
    }
    ((sweep.abs() - std::f64::consts::TAU).abs() * radius <= tolerance).then_some((center, radius))
}

fn loops_same(a: &CapLoop, b: &CapLoop, tolerance: f64, work: &mut usize) -> Option<bool> {
    *work = work.checked_sub(a.segments.len().checked_add(b.segments.len())?)?;
    if (a.area.lo > 0.) != (b.area.lo > 0.) {
        return Some(false);
    }
    if let (Some((ca, ra)), Some((cb, rb))) = (circle(a, tolerance), circle(b, tolerance)) {
        return Some(ca.dist(cb) + (ra - rb).abs() <= tolerance);
    }
    if a.segments.len() != b.segments.len() {
        return Some(false);
    }
    *work = work.checked_sub(a.segments.len().checked_mul(b.segments.len())?)?;
    let mut used = vec![false; b.segments.len()];
    for s in &a.segments {
        let mut match_index = None;
        for (i, t) in b.segments.iter().enumerate() {
            if !used.get(i).copied()? && segment_same(s, t, tolerance) {
                if match_index.is_some() {
                    return None;
                }
                match_index = Some(i);
            }
        }
        let Some(i) = match_index else { return Some(false) };
        *used.get_mut(i)? = true;
    }
    Some(true)
}

struct Certificate {
    loops: Vec<CapLoop>,
    volume: Interval,
    lo: f64,
    hi: f64,
}

/// One closed face shell within the budgets: each face's outward side (whether it is its
/// surface's natural normal).
fn preflight(record: &Brep) -> Option<Vec<bool>> {
    let [Shell::Faces { faces, .. }] = record.shells.as_slice() else { return None };
    if record.faces.len() > 16_384 || faces.len() != record.faces.len() {
        return None;
    }
    let mut edges = 65_536usize;
    for face in &record.faces {
        for lp in &face.loops {
            if let Loop::Edges { edges: uses } = lp {
                edges = edges.checked_sub(uses.len())?;
            }
        }
    }
    let mut outward = vec![None; record.faces.len()];
    for f in faces {
        let slot = outward.get_mut(f.face as usize)?;
        if slot.is_some() {
            return None;
        }
        *slot = Some(f.forward);
    }
    outward.into_iter().collect()
}

/// Face `face`'s edges, each once per use.
fn face_edges(record: &Brep, face: usize) -> impl Iterator<Item = usize> + '_ {
    record
        .faces
        .get(face)
        .into_iter()
        .flat_map(|f| &f.loops)
        .flat_map(|lp| match lp {
            Loop::Edges { edges } => edges.as_slice(),
            Loop::Vertex { .. } => &[],
        })
        .map(|e| e.edge as usize)
}

/// The points of face `face`'s vertices (once per edge use).
fn face_points(record: &Brep, face: usize) -> impl Iterator<Item = Vec3> + '_ {
    face_edges(record, face)
        .filter_map(|e| record.edges.get(e))
        .flat_map(|e| e.vertices)
        .filter_map(|v| record.vertices.get(v as usize))
        .map(|v| v3(v.point))
}

/// Loop `l` of face `face` as it runs about the face's outward normal (`outward`: the natural
/// one), each edge with its number.
fn loop_runs(record: &Brep, face: usize, l: usize, outward: bool) -> Option<Vec<(usize, Run<'_>)>> {
    let Loop::Edges { edges } = record.faces.get(face)?.loops.get(l)? else { return None };
    let mut runs =
        edges.iter().map(|e| Some((e.edge as usize, Run::of(record, e.edge as usize, e.forward == outward)?))).collect::<Option<Vec<_>>>()?;
    if !outward {
        runs.reverse();
    }
    Some(runs)
}

fn certificate(record: &Brep, body_size: f64, plane: &Plane, tol: f64) -> Option<Certificate> {
    let outward = preflight(record)?;
    let d = plane.normal();
    let mut caps = Vec::new();
    let mut walls = Vec::new();
    let mut edge_uses = 65_536usize;
    for (fi, face) in record.faces.iter().enumerate() {
        for _ in face_edges(record, fi) {
            edge_uses = edge_uses.checked_sub(1)?;
        }
        let out = *outward.get(fi)?;
        if let Some((_, u, v)) = plane_of(&face.surface) {
            let n = u.cross(v).normalized()? * if out { 1. } else { -1. };
            if n.dot(d).abs() > 1. - 1e-12 {
                if n.cross(d).len() * body_size > tol {
                    return None;
                }
                // A planar support's parameter origin may be arbitrarily far
                // from its trimmed face. Use actual boundary geometry only.
                let level = plane.height(face_points(record, fi).next()?);
                caps.push((fi, level, n.dot(d) > 0.));
                if caps.len() > 2 {
                    return None;
                }
                continue;
            }
        }
        walls.push(fi);
    }
    if caps.len() != 2 {
        return None;
    }
    let top = *caps.iter().find(|c| c.2)?;
    let bottom = *caps.iter().find(|c| !c.2)?;
    let height = top.1 - bottom.1;
    if height <= tol || !height.is_finite() {
        return None;
    }
    for cap in [top, bottom] {
        let (origin, u, v) = plane_of(&record.faces.get(cap.0)?.surface)?;
        let (n, ne) = plane_normal(u, v)?;
        for point in face_points(record, cap.0) {
            let delta = point - origin;
            let terms = (delta.x * n.x).abs() + (delta.y * n.y).abs() + (delta.z * n.z).abs();
            let error = 1024. * f64::EPSILON * terms + delta.x.abs() * ne.x + delta.y.abs() * ne.y + delta.z.abs() * ne.z;
            if delta.dot(n).abs() + error > tol * 2. || (plane.height(point) - cap.1).abs() > tol * 2. {
                return None;
            }
        }
    }
    let mut work = 2_000_000;
    let cap_loops = |cap: usize, work: &mut usize| -> Option<Vec<CapLoop>> {
        let out = *outward.get(cap)?;
        (0..record.faces.get(cap)?.loops.len()).map(|l| cap_loop(&loop_runs(record, cap, l, out)?, plane, tol, work)).collect()
    };
    let loops = cap_loops(top.0, &mut work)?;
    let lower = cap_loops(bottom.0, &mut work)?;
    if loops.len() != lower.len() || loops.is_empty() || loops.len() > 8192 {
        return None;
    }
    let mut pair_work = 16_000_000;
    for a in &loops {
        // Bottom cap has the opposite material orientation.
        let reverse = CapLoop { segments: a.segments.clone(), area: Interval { lo: -a.area.hi, hi: -a.area.lo } };
        let mut matches = 0usize;
        for b in &lower {
            if loops_same(&reverse, b, tol * 8., &mut pair_work)? {
                matches += 1;
            }
        }
        if matches != 1 {
            return None;
        }
    }
    let mut controls = 262_144;
    // Include cap fitting, support fitting and the <=8tol support-to-cap
    // comparison, not just the control-net's projected ruling variation.
    let mut corridor = tol * 16.;
    let top_edges: std::collections::BTreeSet<usize> = face_edges(record, top.0).collect();
    let bottom_edges: std::collections::BTreeSet<usize> = face_edges(record, bottom.0).collect();
    let cap_edges = face_edges(record, top.0).count().checked_add(face_edges(record, bottom.0).count())?;
    pair_work = pair_work.checked_sub((65_536 - edge_uses).checked_mul(cap_edges)?)?;
    for &wall in &walls {
        let face = record.faces.get(wall)?;
        let variation = wall_error(&face.surface, d, height, &mut controls)?;
        if variation > tol * 8. {
            return None;
        }
        corridor = corridor.max(tol * 16. + variation);
        if corridor > tol * 24. {
            return None;
        }
        // Every wall must end on these actual cap wires. This is topology
        // correspondence, not nearest geometry or stale face provenance.
        let mut upper = 0usize;
        let mut lower = 0usize;
        for edge in face_edges(record, wall) {
            let run = Run::of(record, edge, true)?;
            let (on_top, on_bottom) = (top_edges.contains(&edge), bottom_edges.contains(&edge));
            if on_top || on_bottom {
                let cap = segment(&run, plane, tol, &mut work)?;
                if !wall_supports_cap(&face.surface, &face.domain, &cap, plane.height(run.ends[0]), plane, (bottom.1, top.1), tol, &mut work)? {
                    return None;
                }
                if on_top {
                    upper += 1;
                } else {
                    lower += 1;
                }
                continue;
            }
            if !matches!(run.curve, Curve::Line { .. }) {
                return None;
            }
            let eval = CurveEval::new(run.curve).ok()?;
            let (p, q) = (eval.point(run.interval[0])?, eval.point(run.interval[1])?);
            if p.dist(run.ends[0]) > tol || q.dist(run.ends[1]) > tol {
                return None;
            }
            let delta = q - p;
            if delta.cross(d).len() > corridor || delta.dot(d).abs() < height - tol * 8. {
                return None;
            }
        }
        if upper == 0 || upper != lower {
            return None;
        }
    }
    let mut area = Interval::enclosing(0., 0.)?;
    let mut length = 0.;
    let mut count = 0usize;
    for lp in &loops {
        area = area.add(lp.area)?;
        for edge in &lp.segments {
            length += edge.length();
            count = count.checked_add(1)?;
        }
    }
    if count > 8192 || area.lo <= 0. {
        return None;
    }
    separated_material(&loops, corridor * 16., &mut pair_work)?;
    // Each boundary's monotone radial/linear corridor contributes at most a
    // two-sided strip plus four endpoint disks. Unknown intersecting or
    // touching corridors will be refused by the separation check below.
    let strip = 2. * length * corridor + 4. * std::f64::consts::PI * count as f64 * corridor * corridor;
    // Each cap's vertices are within 2tol and its whole rational edge
    // control heights within another tol; include both cap corridors.
    let h_error = tol * 8.;
    if area.lo <= strip || height <= h_error {
        return None;
    }
    let lo = (area.lo - strip) * (height - h_error);
    let hi = (area.hi + strip) * (height + h_error);
    let volume = Interval::enclosing((lo + hi) * 0.5, (hi - lo) * 0.5)?;
    if volume.lo <= 0. || volume.hi - volume.lo > 0.001 {
        return None;
    }
    Some(Certificate { loops, volume, lo: bottom.1, hi: top.1 })
}

/// Independently prove equality of the supported current uniform prisms.
/// `None` means unproved, including budget exhaustion; it never grants a copy.
pub fn certified_prism_equivalent(a: &crate::Body, b: &crate::Body) -> crate::Result<Option<bool>> {
    crate::guard("certified prism comparison", || {
        if a.is_mesh() || b.is_mesh() || !a.size().is_finite() || !b.size().is_finite() {
            return Ok(None);
        }
        let (ra, rb) = (crate::native::brep_record(a)?, crate::native::brep_record(b)?);
        Ok(equivalent(&ra, a.size(), &rb, b.size()))
    })
}

/// [`certified_prism_equivalent`] of the records `a` and `b` of bodies of sizes `size_a` and
/// `size_b`.
fn equivalent(a: &Brep, size_a: f64, b: &Brep, size_b: f64) -> Option<bool> {
    let outward = preflight(a)?;
    preflight(b)?;
    let size = size_a.max(size_b).max(1.);
    let tol = size * 1e-10;
    let identity_tol = size * 1e-7;
    let origin = v3(a.vertices.first()?.point);
    let mut candidates = Vec::new();
    for (face, out) in a.faces.iter().zip(&outward) {
        if let Some((_, u, v)) = plane_of(&face.surface) {
            let n = u.cross(v).normalized()? * if *out { 1. } else { -1. };
            if !candidates.iter().any(|d: &Vec3| d.dot(n).abs() > 1. - 1e-12) {
                candidates.push(n);
                if candidates.len() == 8 {
                    break;
                }
            }
        }
    }
    for d in candidates {
        let plane = Plane::from_normal(origin, d)?;
        let Some(ca) = certificate(a, size_a, &plane, tol) else { continue };
        let Some(cb) = certificate(b, size_b, &plane, tol) else { continue };
        if (ca.lo - cb.lo).abs() > identity_tol || (ca.hi - cb.hi).abs() > identity_tol || ca.loops.len() != cb.loops.len() {
            return Some(false);
        }
        let mut work = 16_000_000;
        let mut used = vec![false; cb.loops.len()];
        for lp in &ca.loops {
            let mut matched = None;
            for (i, other) in cb.loops.iter().enumerate() {
                if !used.get(i).copied()? && loops_same(lp, other, identity_tol, &mut work)? {
                    if matched.is_some() {
                        return None;
                    }
                    matched = Some(i);
                }
            }
            let Some(i) = matched else { return Some(false) };
            *used.get_mut(i)? = true;
        }
        let delta = (ca.volume.hi - cb.volume.lo).abs().max((cb.volume.hi - ca.volume.lo).abs());
        return Some(delta <= 1e-5 * (1. + ca.volume.lo.abs()));
    }
    None
}

/// A cap's loop from its edges in order (each with its edge number), certified segment by
/// segment, with the interval of its signed area.
fn cap_loop(runs: &[(usize, Run)], plane: &Plane, tolerance: f64, work: &mut usize) -> Option<CapLoop> {
    let mut segments = Vec::new();
    let mut area = Interval::enclosing(0., 0.)?;
    let mut ids = std::collections::HashSet::new();
    for (id, run) in runs {
        if !ids.insert(*id) {
            return None;
        }
        let certified = segment(run, plane, tolerance, work)?;
        if segments.len().checked_add(certified.len())? > 8192 {
            return None;
        }
        for segment in &certified {
            area = area.add(green_interval(segment, tolerance)?)?;
        }
        segments.extend(certified);
    }
    // The edges run end to end round the loop.
    let closed = runs.iter().zip(runs.iter().cycle().skip(1)).all(|((_, a), (_, b))| a.ends[1].dist(b.ends[0]) <= 2. * tolerance);
    if segments.is_empty() || !closed {
        return None;
    }
    // Curve endpoints already agree with actual shared vertices in the
    // whole-span proof. Check the fitted representation's continuity too.
    let segments = Ring::new(segments);
    for (a, b) in segments.edges() {
        if a.end().dist(b.start()) > 2. * tolerance {
            return None;
        }
    }
    Some(CapLoop { segments, area })
}

/// Initial material scope: a convex straight outer or a circle, with separated
/// circular holes. Wider profiles must acquire their own separation proof.
fn separated_material(loops: &[CapLoop], clearance: f64, work: &mut usize) -> Option<()> {
    let outer = loops.iter().find(|l| l.area.lo > 0.)?;
    if loops.iter().filter(|l| l.area.lo > 0.).count() != 1 {
        return None;
    }
    let outer_circle = circle(outer, clearance);
    if outer_circle.is_none() {
        *work = work.checked_sub(outer.segments.len().checked_mul(outer.segments.len())?)?;
        for s in &outer.segments {
            let Seg2::Line { a, b } = *s else { return None };
            let delta = b - a;
            if delta.len() <= clearance {
                return None;
            }
            for t in &outer.segments {
                if delta.cross(t.start() - a) < -clearance * delta.len() {
                    return None;
                }
            }
        }
        let mut turn = 0.;
        let mut angular_error = 0.;
        for (i, (s, next)) in outer.segments.edges().enumerate() {
            let Seg2::Line { a, b } = *s else {
                return None;
            };
            let x = b - a;
            let y = next.end() - next.start();
            turn += x.cross(y).atan2(x.dot(y));
            angular_error += clearance * 8. / x.len() + 256. * f64::EPSILON;
            for (j, t) in outer.segments.iter().enumerate() {
                if i != j && a.dist(t.start()) <= clearance {
                    return None;
                }
            }
        }
        if angular_error > 1e-3 || (turn - std::f64::consts::TAU).abs() > angular_error {
            return None;
        }
    }
    let mut holes = Vec::new();
    for lp in loops {
        if std::ptr::eq(lp, outer) {
            continue;
        }
        if lp.area.hi >= 0. {
            return None;
        }
        let (center, radius) = circle(lp, clearance)?;
        if let Some((c, r)) = outer_circle {
            if center.dist(c) + radius + clearance >= r {
                return None;
            }
        } else {
            *work = work.checked_sub(outer.segments.len())?;
            for s in &outer.segments {
                let Seg2::Line { a, b } = *s else { return None };
                if (b - a).cross(center - a) / (b - a).len() <= radius + clearance {
                    return None;
                }
            }
        }
        *work = work.checked_sub(holes.len())?;
        for (c, r) in &holes {
            if center.dist(*c) <= radius + r + clearance {
                return None;
            }
        }
        holes.push((center, radius));
    }
    Some(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn drilled(radius: f64, shift: f64) -> crate::Body {
        use solidcraft_geom::{Loop2, Region2};
        let region = Region2 {
            outer: Loop2::polygon(&[Vec2::ZERO, Vec2::new(20., 0.), Vec2::new(20., 20.), Vec2::new(0., 20.)]),
            holes: vec![Loop2::circle(Vec2::new(10. + shift, 10.), radius)],
        };
        crate::extrude(&Plane::XY, &[region], 0., 10.).unwrap().remove(0)
    }

    fn record(b: &crate::Body) -> Brep {
        crate::native::brep_record(b).unwrap()
    }

    fn cert(b: &crate::Body, tol: f64) -> Option<Certificate> {
        certificate(&record(b), b.size(), &Plane::XY, tol)
    }

    /// The first face of `r` not on a plane.
    fn curved(r: &Brep) -> usize {
        r.faces.iter().position(|f| plane_of(&f.surface).is_none()).unwrap()
    }

    #[test]
    fn actual_drilled_prism_and_step_reopen_are_certified_without_history() {
        let source = drilled(6., 0.);
        let before = crate::step_export(&[&source], "source").unwrap();
        let imported = crate::step_import(&before).unwrap();
        let copy = &imported.bodies[0].body;
        let exact = 4000. - 360. * std::f64::consts::PI;
        for body in [&source, copy] {
            let cap = cert(body, 3e-9).expect("actual complete current cap and walls");
            assert!(cap.volume.lo <= exact && cap.volume.hi >= exact, "{:?}, expected {exact}", cap.volume);
            assert!(cap.volume.hi - cap.volume.lo <= 0.001);
        }
        assert_eq!(certified_prism_equivalent(&source, copy).unwrap(), Some(true));
        let wider = drilled(6.0001, 0.);
        let a = cert(&source, 3e-9).unwrap();
        let b = cert(&wider, 3e-9).unwrap();
        assert!(a.volume.lo - b.volume.hi > 1e-5 * (1. + a.volume.hi));
        assert_eq!(certified_prism_equivalent(&source, &wider).unwrap(), Some(false));
        let shifted = drilled(6., 0.0001);
        let same_volume = cert(&shifted, 3e-9).unwrap();
        assert!(same_volume.volume.lo <= exact && same_volume.volume.hi >= exact);
        assert_eq!(certified_prism_equivalent(&source, &shifted).unwrap(), Some(false));
        assert_eq!(crate::step_export(&[&source], "source").unwrap(), before);
    }

    #[test]
    fn poorly_conditioned_plane_walls_do_not_get_certificates() {
        let surface = Surface::Plane { origin: [0., 0., 0.], u_axis: [1., 0., 0.], v_axis: [1., 0., 1e-9] };
        let line = Curve::Line { start: [0., 0., 0.], end: [1., 0., 0.] };
        let mut work = 2_000_000;
        let cap =
            segment(&Run { curve: &line, interval: [0., 1.], forward: true, ends: [Vec3::ZERO, Vec3::X] }, &Plane::XY, 1e-9, &mut work).unwrap();
        assert!(wall_error(&surface, Vec3::Z, 10., &mut work).is_none());
        assert!(wall_supports_cap(&surface, &[[0., 1.], [0., 1.]], &cap, 0., &Plane::XY, (0., 10.), 1e-9, &mut work).is_none());
        let normal = plane_normal(Vec3::X, Vec3::Z).unwrap();
        assert!(normal.0.dist(-Vec3::Y) < 1e-12);
        assert!(normal.1.len() > 0.);
    }

    #[test]
    fn rotated_current_boolean_prism_step_and_wrong_surface_incidence() {
        let stock = crate::box_solid(Vec3::ZERO, Vec3::new(20., 20., 10.)).unwrap();
        let cutter = crate::cylinder(Vec3::new(10., 10., 0.), Vec3::Z, 6., 10.).unwrap();
        let source = crate::boolean(&stock, &cutter, crate::BoolOp::Cut).unwrap().unwrap();
        let moved = crate::transform(&source, Vec3::new(30., 20., 10.), Vec3::ZERO, Vec3::new(1., 2., 3.), 0.7).unwrap();
        let saved = crate::step_export(&[&moved], "actual boolean").unwrap();
        let imported = crate::step_import(&saved).unwrap();
        assert_eq!(certified_prism_equivalent(&moved, &imported.bodies[0].body).unwrap(), Some(true));
        // Preserve every current cap/edge while replacing only the curved
        // wall support with radius five. Topology incidence alone must fail.
        let wrong = record(&drilled(5., 0.));
        let mut invalid = record(&source);
        let wall = curved(&invalid);
        invalid.faces[wall].surface = wrong.faces[curved(&wrong)].surface.clone();
        assert!(certificate(&invalid, source.size(), &Plane::XY, 3e-9).is_none());
        // The actual wall moved 1000 up: it carries no cap edge at the part's levels.
        let actual = record(&source);
        let wall = curved(&actual);
        let mut shifted = actual.faces[wall].surface.clone();
        match &mut shifted {
            Surface::Cylinder { frame, .. } => frame.origin[2] += 1000.,
            Surface::PlacedRevolution { matrix, .. } => matrix[3][2] += 1000.,
            Surface::Spline { points, .. } => points.iter_mut().flatten().for_each(|p| p[2] += 1000.),
            Surface::RationalSpline { homogeneous, .. } => homogeneous.iter_mut().flatten().for_each(|p| p[2] += 1000. * p[3]),
            other => panic!("unexpected wall {other:?}"),
        }
        let outward = preflight(&actual).unwrap();
        let top = (0..actual.faces.len())
            .find(|&f| {
                plane_of(&actual.faces[f].surface)
                    .is_some_and(|(_, u, v)| u.cross(v).normalized().unwrap().z * if outward[f] { 1. } else { -1. } > 0.9)
            })
            .unwrap();
        let arc = face_edges(&actual, top).find(|e| !matches!(actual.edges[*e].curve, Curve::Line { .. })).unwrap();
        let run = Run::of(&actual, arc, true).unwrap();
        let cap = segment(&run, &Plane::XY, 3e-9, &mut 2_000_000).unwrap();
        let domain = actual.faces[wall].domain;
        assert_ne!(
            wall_supports_cap(&shifted, &domain, &cap, Plane::XY.height(run.ends[0]), &Plane::XY, (0., 10.), 3e-9, &mut 2_000_000),
            Some(true)
        );
    }

    #[test]
    fn cap_support_origin_is_not_a_trimmed_face_height() {
        let source = drilled(6., 0.);
        let original = record(&source);
        let mut same = original.clone();
        for face in &mut same.faces {
            match &mut face.surface {
                Surface::Plane { origin, u_axis, v_axis } if v3(*u_axis).cross(v3(*v_axis)).normalized().unwrap().z.abs() > 0.9 => {
                    *origin = (v3(*origin) + v3(*u_axis) * 1e8).into();
                }
                Surface::PlaneThrough { origin, u_point, v_point } => {
                    let (o, u, v) = (v3(*origin), v3(*u_point), v3(*v_point));
                    if (u - o).cross(v - o).normalized().unwrap().z.abs() > 0.9 {
                        let shift = (u - o) * 1e8;
                        (*origin, *u_point, *v_point) = ((o + shift).into(), (u + shift).into(), (v + shift).into());
                    }
                }
                _ => {}
            }
        }
        assert_eq!(equivalent(&original, source.size(), &same, source.size()), Some(true));
        let last = original.faces.len() - 1;
        let outward = preflight(&original).unwrap()[last];
        let runs = loop_runs(&original, last, 0, outward).unwrap();
        assert!(
            loops_same(
                &cap_loop(&runs, &Plane::XY, 3e-9, &mut 2_000_000).unwrap(),
                &cap_loop(&runs, &Plane::XY, 3e-9, &mut 2_000_000).unwrap(),
                1e-6,
                &mut 0
            )
            .is_none()
        );
    }

    #[test]
    fn winding_retracing_and_uncertain_material_do_not_get_certificates() {
        let arc = Seg2::Arc { center: Vec2::ZERO, radius: 6., start: 0., sweep: std::f64::consts::TAU };
        let retraced =
            CapLoop { segments: Ring::from([arc, arc.reversed(), arc]), area: Interval::enclosing(36. * std::f64::consts::PI, 1e-8).unwrap() };
        assert!(circle(&retraced, 1e-6).is_none());
        let region = solidcraft_geom::Loop2::polygon(&[Vec2::ZERO, Vec2::new(20., 0.), Vec2::new(20., 20.), Vec2::new(0., 20.)]);
        let doubled = CapLoop { segments: region.segs.iter().chain(&region.segs).copied().collect(), area: Interval::enclosing(800., 1e-8).unwrap() };
        assert!(separated_material(&[doubled], 1e-6, &mut 16_000_000).is_none());
        let thin = crate::box_solid(Vec3::ZERO, Vec3::new(20., 20., 1e-5)).unwrap();
        assert!(cert(&thin, 3e-6).is_none());
    }

    #[test]
    fn current_rational_circle_green_interval_encloses_independent_area() {
        let r = 6.;
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let corner = |i: usize| {
            let a = i as f64 * std::f64::consts::FRAC_PI_2;
            Vec3::new(r * a.cos(), r * a.sin(), 0.)
        };
        // Four quarter circles as rational quadratic splines.
        let curves: Vec<Curve> = (0..4)
            .map(|i| {
                let (a, b) = (corner(i), corner(i + 1));
                let apex = a + b;
                Curve::RationalSpline {
                    knots: vec![0., 0., 0., 1., 1., 1.],
                    homogeneous: vec![[a.x, a.y, 0., 1.], [apex.x * w, apex.y * w, 0., w], [b.x, b.y, 0., 1.]],
                }
            })
            .collect();
        let runs: Vec<(usize, Run)> = curves
            .iter()
            .enumerate()
            .map(|(i, c)| (i, Run { curve: c, interval: [0., 1.], forward: true, ends: [corner(i), corner(i + 1)] }))
            .collect();
        let cap = cap_loop(&runs, &Plane::XY, 3e-9, &mut 2_000_000).unwrap();
        let exact = std::f64::consts::PI * r * r;
        assert!(cap.area.lo <= exact && cap.area.hi >= exact, "{:?}", cap.area);
        assert!((cap.area.hi - cap.area.lo) * 10. < 0.001);
        assert_eq!(cap.segments.len(), 4);
        let back: Vec<(usize, Run)> =
            runs.iter().rev().map(|(i, run)| (*i, Run { forward: false, ends: [run.ends[1], run.ends[0]], ..*run })).collect();
        let reversed = cap_loop(&back, &Plane::XY, 3e-9, &mut 2_000_000).unwrap();
        assert!(reversed.area.lo <= -exact && reversed.area.hi >= -exact);
    }
}
