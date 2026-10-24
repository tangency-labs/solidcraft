//! Fillets beyond a constant radius: a radius that varies along the edge (from one end to the
//! other) and chord-length fillets (the width across the blend given instead of the radius).
//!
//! Variable radius: a straight, convex or concave edge between two planar faces whose ends each
//! touch one more planar face square to the edge (the configuration of `blend`). Every section
//! square to the edge is a circular arc tangent to both faces, its radius running linearly from
//! one end to the other; the arcs' control points move linearly, so the blend is exactly a
//! rational B-spline of degree 2 across and 1 along. The radius may run down to zero at one end:
//! there the blend closes to the edge's end point, a three-sided face, and the end face is kept.

use std::collections::HashMap;

use solidcraft_geom::Vec3;
use truck_modeling::{self as mt, builder};

use crate::blend::{plane_normal, rebuild_face, too_large, unsupported, vtx};
use crate::body::{Body, Solid, p3};
use crate::{KernelError, Result, guard};

/// The radius of a chord-length fillet between faces whose in-face directions (away from the
/// edge) make the angle `phi`: the arc spans π − φ, so the chord is 2 r cos(φ/2).
fn chord_radius(chord: f64, phi: f64) -> f64 {
    chord / (2.0 * (phi / 2.0).cos())
}

/// The two planar faces at an edge, the in-face directions away from it (`t1`, `t2`), the
/// first face's normal, the angle between the directions and the side of the material.
struct EdgeFrame {
    i1: usize,
    i2: usize,
    n1: Vec3,
    t1: Vec3,
    t2: Vec3,
    phi: f64,
    side: f64,
}

fn edge_frame(faces: &[&mt::Face], edge: &mt::Edge) -> Result<EdgeFrame> {
    let on_edge: Vec<usize> = (0..faces.len()).filter(|i| faces.get(*i).is_some_and(|f| f.edge_iter().any(|e| e.id() == edge.id()))).collect();
    let [i1, i2] = on_edge[..] else { return Err(unsupported("the edge is not between two faces")) };
    let (Some(f1), Some(f2)) = (faces.get(i1), faces.get(i2)) else { return Err(KernelError::Failed("faces".into())) };
    let (Some(n1), Some(n2)) = (plane_normal(f1), plane_normal(f2)) else { return Err(unsupported("both faces next to the edge must be planar")) };
    let into = |f: &mt::Face, n: Vec3| -> Option<Vec3> {
        let e = f.boundary_iters().into_iter().flatten().find(|e| e.id() == edge.id())?;
        let dir = (vtx(e.back()) - vtx(e.front())).normalized()?;
        n.cross(dir).normalized()
    };
    let (Some(t1), Some(t2)) = (into(f1, n1), into(f2, n2)) else { return Err(KernelError::Failed("blend: edge direction".into())) };
    let phi = t1.dot(t2).clamp(-1.0, 1.0).acos();
    let convex = n1.dot(t2) < -1e-9 && n2.dot(t1) < -1e-9;
    let concave = n1.dot(t2) > 1e-9 && n2.dot(t1) > 1e-9;
    if !(phi > 1e-3 && phi < std::f64::consts::PI - 1e-3) || !(convex || concave) {
        return Err(unsupported("the faces at the edge are tangent or the edge is ambiguous"));
    }
    Ok(EdgeFrame { i1, i2, n1, t1, t2, phi, side: if convex { -1.0 } else { 1.0 } })
}

/// Fillet one straight edge with radius `r0` at its end nearest `start` and `r1` at the other.
pub fn fillet_variable(body: &Body, edge_at: Vec3, start: Vec3, r0: f64, r1: f64) -> Result<Body> {
    fillet_variable_points(body, edge_at, start, &[(0.0, r0), (1.0, r1)], false)
}

/// A fillet radius along an edge: a clamped B-spline of the position along it (0 at one end, 1
/// at the other), given by its degree, knots and control radii. The radius is a smooth curve
/// shaped by its control radii, each pulling it toward itself near its Greville position.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RadiusSpline {
    pub degree: usize,
    pub knots: Vec<f64>,
    pub radii: Vec<f64>,
}

impl RadiusSpline {
    /// Through (position, radius) points sorted by position from 0 to 1: straight between them,
    /// or `smooth` (cubic pieces with Catmull-Rom slopes, C¹ through every point).
    pub fn through(pts: &[(f64, f64)], smooth: bool) -> RadiusSpline {
        if !smooth || pts.len() < 3 {
            let mut knots = vec![0.0];
            knots.extend(pts.iter().map(|p| p.0));
            knots.push(1.0);
            return RadiusSpline { degree: 1, knots, radii: pts.iter().map(|p| p.1).collect() };
        }
        let n = pts.len();
        let at = |i: usize| pts.get(i).copied().unwrap_or((0.0, 0.0));
        let slope = |i: usize| {
            let (a, b) = (at(i.saturating_sub(1)), at((i + 1).min(n - 1)));
            if b.0 > a.0 { (b.1 - a.1) / (b.0 - a.0) } else { 0.0 }
        };
        let mut knots = vec![0.0; 4];
        let mut radii = vec![at(0).1];
        for i in 0..n - 1 {
            let ((t0, r0), (t1, r1)) = (at(i), at(i + 1));
            let h = t1 - t0;
            radii.extend([r0 + slope(i) * h / 3.0, r1 - slope(i + 1) * h / 3.0, r1]);
            knots.extend(if i + 2 == n { vec![1.0; 4] } else { vec![t1; 3] });
        }
        RadiusSpline { degree: 3, knots, radii }
    }

    /// Greville abscissae: where each control radius sits along the edge.
    pub fn greville(&self) -> Vec<f64> {
        (0..self.radii.len()).map(|j| self.knots.iter().skip(j + 1).take(self.degree).sum::<f64>() / self.degree as f64).collect()
    }

    /// The radius at position `t`.
    pub fn eval(&self, t: f64) -> f64 {
        let kv = mt::KnotVec::from(self.knots.clone());
        let b = kv.try_bspline_basis_functions(self.degree, t.clamp(0.0, 1.0)).unwrap_or_default();
        b.iter().zip(&self.radii).map(|(x, r)| x * r).sum()
    }

    /// The same radius with its positions taken from the other end.
    pub fn reversed(&self) -> RadiusSpline {
        RadiusSpline {
            degree: self.degree,
            knots: self.knots.iter().rev().map(|k| 1.0 - k).collect(),
            radii: self.radii.iter().rev().copied().collect(),
        }
    }

    /// The same radius with one more control radius: the knot `u` inserted (Boehm), which leaves
    /// the curve as it was. None for `u` at or beyond an end, on an existing knot, or a spline
    /// already at 100 control radii.
    pub fn insert(&self, u: f64) -> Option<RadiusSpline> {
        let (n, p) = (self.radii.len(), self.degree);
        if !(u > 1e-6 && u < 1.0 - 1e-6) || n >= 100 || self.knots.iter().any(|k| (k - u).abs() < 1e-6) {
            return None;
        }
        // The span holding u: knots[k] <= u < knots[k + 1], p <= k < n.
        let k = (p..n).rev().find(|i| self.knots.get(*i).is_some_and(|x| *x <= u))?;
        let mut radii = Vec::with_capacity(n + 1);
        radii.extend_from_slice(self.radii.get(..=k - p)?);
        for i in k - p + 1..=k {
            let (lo, hi) = (*self.knots.get(i)?, *self.knots.get(i + p)?);
            let a = (u - lo) / (hi - lo);
            radii.push((1.0 - a) * self.radii.get(i - 1)? + a * self.radii.get(i)?);
        }
        radii.extend_from_slice(self.radii.get(k..)?);
        let mut knots = self.knots.clone();
        knots.insert(k + 1, u);
        Some(RadiusSpline { degree: self.degree, knots, radii })
    }

    /// The same spline without control radius `j` and the inner knot nearest its position: the
    /// curve changes around it, the ends stay. None for an end radius or a spline with no inner
    /// knot to remove.
    pub fn remove(&self, j: usize) -> Option<RadiusSpline> {
        let (n, p) = (self.radii.len(), self.degree);
        if j == 0 || j + 1 >= n || n <= p + 1 {
            return None;
        }
        let at = *self.greville().get(j)?;
        let inner = p + 1..n;
        let drop = inner.min_by(|a, b| (self.knots[*a] - at).abs().total_cmp(&(self.knots[*b] - at).abs()))?;
        let mut knots = self.knots.clone();
        knots.remove(drop);
        let mut radii = self.radii.clone();
        radii.remove(j);
        Some(RadiusSpline { degree: self.degree, knots, radii })
    }

    /// Refused unless it is a clamped B-spline from 0 to 1 of degree 1 to 3 with 2 to 100 control
    /// radii, positive along the edge and zero at most at one end.
    fn check(&self) -> Result<()> {
        let (n, deg) = (self.radii.len(), self.degree);
        if !(1..=3).contains(&deg) || !(2..=100).contains(&n) || n <= deg || self.knots.len() != n + deg + 1 {
            return Err(KernelError::Invalid("a radius spline has degree 1…3, 2…100 control radii and as many knots as both and one more".into()));
        }
        let clamped = self.knots.iter().take(deg + 1).all(|k| *k == 0.0) && self.knots.iter().skip(n).all(|k| *k == 1.0);
        if !clamped || !self.knots.iter().all(|k| k.is_finite()) || self.knots.windows(2).any(|w| w[1] < w[0]) {
            return Err(KernelError::Invalid("radius spline knots rise from 0 to 1, repeated degree + 1 times at each end".into()));
        }
        if self.radii.iter().any(|r| !(r.is_finite() && (*r == 0.0 || *r > 1e-6) && *r < 1e6)) {
            return Err(KernelError::Invalid("fillet radii must be positive (zero only at an end)".into()));
        }
        let zeros: Vec<usize> = (0..n).filter(|i| self.radii.get(*i) == Some(&0.0)).collect();
        if !matches!(zeros[..], [] | [0]) && zeros[..] != [n - 1] {
            return Err(KernelError::Invalid("the radius may be zero at one end of the edge only".into()));
        }
        // Positive inside the edge (a zero end is given exactly, as a control radius).
        if (1..200).any(|k| self.eval(k as f64 / 200.0) <= 1e-6) {
            return Err(KernelError::Invalid("the radius must stay positive along the edge".into()));
        }
        Ok(())
    }
}

/// Fillet one straight edge with a radius that varies along it: (position, radius) points with
/// positions 0 (the end nearest `start`) to 1 (the other end) and any between, the radius
/// straight between them or `smooth`. One end's radius may be zero.
pub fn fillet_variable_points(body: &Body, edge_at: Vec3, start: Vec3, points: &[(f64, f64)], smooth: bool) -> Result<Body> {
    if points.len() < 2 || points.len() > 100 {
        return Err(KernelError::Invalid("a variable radius needs 2…100 points".into()));
    }
    if points.iter().any(|(t, _)| !(t.is_finite() && (0.0..=1.0).contains(t))) {
        return Err(KernelError::Invalid("radius positions run from 0 to 1 along the edge".into()));
    }
    let mut pts = points.to_vec();
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    pts.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-9);
    if pts.first().map(|p| p.0) != Some(0.0) || pts.last().map(|p| p.0) != Some(1.0) || pts.len() < 2 {
        return Err(KernelError::Invalid("give the radius at both ends of the edge".into()));
    }
    fillet_variable_spline(body, edge_at, start, &RadiusSpline::through(&pts, smooth))
}

/// Fillet one straight edge with the radius `spline` along it, its positions from the end
/// nearest `start`. One end's radius may be zero. A curved edge, or one between curved faces, is
/// rolled by a ball of the law's radius instead ([`curved_variable`]).
pub fn fillet_variable_spline(body: &Body, edge_at: Vec3, start: Vec3, spline: &RadiusSpline) -> Result<Body> {
    body.require_brep("fillet")?;
    spline.check()?;
    let edge = crate::surfaces::edge_near(body, edge_at).ok_or_else(|| KernelError::Invalid(format!("no edge at {edge_at:?}")))?;
    guard("variable fillet", || {
        let Ok(s) = EdgeSetup::new(body, &edge) else {
            return curved_variable(body, &edge, start, spline);
        };
        // Positions from the end nearest `start` (the edge keeps its own direction).
        let law = if s.p1.dist(start) < s.p0.dist(start) { spline.reversed() } else { spline.clone() };
        build(body, &s, &edge, &law)
    })
}

/// The radius law along a curved edge: a ball whose radius is the law's at each station rolls
/// along the edge's smooth chain (the same sections, contacts and trims as the straight-edge case,
/// sized by the law there). A law that has run to zero at an end closes the blend to the edge's
/// end point, as the straight-edge fillet does.
fn curved_variable(body: &Body, edge: &mt::Edge, start: Vec3, law: &RadiusSpline) -> Result<Body> {
    // Positions from the end nearest `start` (the chain runs along the edge's own direction).
    let (p0, p1) = (vtx(edge.absolute_front()), vtx(edge.absolute_back()));
    let law = if p1.dist(start) < p0.dist(start) { law.reversed() } else { law.clone() };
    let chain = crate::blend::smooth_chain(&body.solid, edge).unwrap_or_else(|| vec![edge.id()]);
    let (made, rec) = guard("variable fillet", || crate::curveblend::curve_blend_law(&body.solid, &chain, body.size(), &law, true))?;
    crate::record::rebuilt(made, &[&body.solid], &rec)
}

/// A straight edge between two planar faces as the variable blend sees it: the shell it is in
/// and that shell's faces, the frame at the edge, and its ends (front first).
struct EdgeSetup<'a> {
    si: usize,
    faces: Vec<&'a mt::Face>,
    fr: EdgeFrame,
    edge: mt::EdgeID,
    v0: mt::Vertex,
    v1: mt::Vertex,
    p0: Vec3,
    p1: Vec3,
    d: Vec3,
    tol: f64,
}

impl<'a> EdgeSetup<'a> {
    fn new(body: &'a Body, edge: &mt::Edge) -> Result<EdgeSetup<'a>> {
        if !matches!(edge.curve(), mt::Curve::Line(_)) {
            return Err(unsupported("only straight edges take a variable radius"));
        }
        let (si, shell) = body
            .solid
            .boundaries()
            .iter()
            .enumerate()
            .find(|(_, sh)| sh.edge_iter().any(|e| e.id() == edge.id()))
            .ok_or_else(|| KernelError::Failed("edge not found".into()))?;
        let faces: Vec<&mt::Face> = shell.face_iter().collect();
        let fr = edge_frame(&faces, edge)?;
        let (v0, v1) = (edge.absolute_front().clone(), edge.absolute_back().clone());
        let (p0, p1) = (vtx(&v0), vtx(&v1));
        let d = (p1 - p0).normalized().ok_or_else(|| KernelError::Failed("zero-length edge".into()))?;
        Ok(EdgeSetup { si, faces, fr, edge: edge.id(), v0, v1, p0, p1, d, tol: (body.size() * 1e-7).max(1e-9) })
    }

    /// How far a blend of radius `r` reaches into each face from the edge.
    fn back(&self, r: f64) -> f64 {
        r / (self.fr.phi / 2.0).tan()
    }

    /// Whether the band a blend of radius `r` takes from the faces at position `u` (strictly
    /// between the ends) stays inside both, clear of their other edges and holes.
    fn band_fits(&self, u: f64, r: f64) -> bool {
        let p = self.p0 + (self.p1 - self.p0) * u;
        let s = self.back(r);
        [(self.fr.i1, self.fr.t1), (self.fr.i2, self.fr.t2)]
            .into_iter()
            .all(|(i, t)| self.faces.get(i).is_some_and(|f| crate::blend::extension_fits(f, p, p + t * s, self.tol)))
    }

    /// The largest radius up to `cap` whose band fits at position `u`.
    fn room(&self, u: f64, cap: f64) -> f64 {
        if self.band_fits(u, cap) {
            return cap;
        }
        let (mut lo, mut hi) = (0.0, cap);
        while hi - lo > self.tol {
            let mid = (lo + hi) / 2.0;
            if self.band_fits(u, mid) { lo = mid } else { hi = mid }
        }
        lo
    }

    /// The room at the end `v` (`out` pointing away from the edge there), as a radius: zero
    /// where a face closes to a point (its next side runs back along the edge), the reach of the
    /// sides along the faces where they are square to the edge, None where a side leans outward.
    fn end_room(&self, v: &mt::Vertex, out: Vec3) -> Option<f64> {
        let mut room = f64::INFINITY;
        for (i, t) in [(self.fr.i1, self.fr.t1), (self.fr.i2, self.fr.t2)] {
            let side = self.faces.get(i)?.edge_iter().find(|e| e.id() != self.edge && (e.front() == v || e.back() == v))?;
            let dir = leaving(&side, v, self.tol)?;
            let lean = dir.dot(out);
            if lean < -1e-9 {
                return Some(0.0);
            }
            if lean > 1e-9 || !matches!(side.curve(), mt::Curve::Line(_)) {
                return None;
            }
            let other = if side.front() == v { side.back() } else { side.front() };
            room = room.min((vtx(other) - vtx(v)).dot(t) * (self.fr.phi / 2.0).tan());
        }
        Some(room)
    }
}

/// The direction `edge` leaves its end `v` in (along its first piece within `tol`).
fn leaving(edge: &mt::Edge, v: &mt::Vertex, tol: f64) -> Option<Vec3> {
    use mt::{BoundedCurve, ParameterDivision1D};
    let curve = edge.curve();
    let pts = curve.parameter_division(curve.range_tuple(), tol.max(1e-6)).1;
    let p = vtx(v);
    let near = if crate::body::from_p3(*pts.first()?).dist(p) <= crate::body::from_p3(*pts.last()?).dist(p) {
        pts.get(1)?
    } else {
        pts.get(pts.len().checked_sub(2)?)?
    };
    (crate::body::from_p3(*near) - p).normalized()
}

/// Build the blend of the edge of `s` with the radius `law` (its positions from the edge's front).
fn build(body: &Body, s: &EdgeSetup, edge: &mt::Edge, law: &RadiusSpline) -> Result<Body> {
    let (faces, fr, tol) = (&s.faces, &s.fr, s.tol);
    let (v0, v1, p0, p1, d) = (&s.v0, &s.v1, s.p0, s.p1, s.d);
    // The end radii are the first and last control radii (exact: a zero end closes the blend).
    let (ra, rb) = (law.radii.first().copied().unwrap_or(0.0), law.radii.last().copied().unwrap_or(0.0));
    let back = |r: f64| s.back(r);
    let arc_mid = |p: Vec3, sb: f64, r: f64| {
        let c = p + fr.t1 * sb + fr.n1 * (r * fr.side);
        c + (p - c).normalized().unwrap_or(-fr.n1) * r
    };
    // Each end: the blend's corners on the two faces and, with a radius, the arc between them,
    // the end face and its edges along the two faces. At a zero radius the blend closes to the
    // end vertex, and the faces around it stay as they are.
    struct Arc {
        v: mt::Vertex,
        g: usize,
        e1: mt::Edge,
        e2: mt::Edge,
        edge: mt::Edge,
    }
    struct End {
        a: mt::Vertex,
        b: mt::Vertex,
        arc: Option<Arc>,
    }
    let mut ends = Vec::new();
    for (v, p, r) in [(v0, p0, ra), (v1, p1, rb)] {
        if r == 0.0 {
            ends.push(End { a: v.clone(), b: v.clone(), arc: None });
            continue;
        }
        let at_v: Vec<usize> = (0..faces.len()).filter(|i| faces.get(*i).is_some_and(|f| f.vertex_iter().any(|x| &x == v))).collect();
        let others: Vec<usize> = at_v.iter().copied().filter(|i| *i != fr.i1 && *i != fr.i2).collect();
        let [g] = others[..] else { return Err(unsupported("each end of the edge must touch exactly one more face")) };
        let gf = faces.get(g).ok_or_else(|| KernelError::Failed("face".into()))?;
        let ng = plane_normal(gf).ok_or_else(|| unsupported("the faces at the edge ends must be planar"))?;
        if ng.dot(d).abs() < 1.0 - 1e-9 {
            return Err(unsupported("the faces at the edge ends must be perpendicular to the edge"));
        }
        let shared = |fi: usize| -> Result<mt::Edge> {
            let f = faces.get(fi).ok_or_else(|| KernelError::Failed("face".into()))?;
            f.edge_iter()
                .find(|e| e.id() != edge.id() && (e.front() == v || e.back() == v) && gf.edge_iter().any(|x| x.id() == e.id()))
                .ok_or_else(|| unsupported("unexpected corner topology"))
        };
        let (e1, e2) = (shared(fr.i1)?, shared(fr.i2)?);
        for (e, t) in [(&e1, fr.t1), (&e2, fr.t2)] {
            if !matches!(e.curve(), mt::Curve::Line(_)) {
                return Err(unsupported("the edges at the corners must be straight"));
            }
            let other = if e.front() == v { vtx(e.back()) } else { vtx(e.front()) };
            let along = (other - p).dot(t);
            if (other - p - t * along).len() > tol * 100.0 + 1e-7 {
                return Err(unsupported("the faces at the edge ends must be perpendicular to the edge"));
            }
            if along <= back(r) + tol {
                return Err(too_large("the blend is larger than the neighbouring faces"));
            }
        }
        let sb = back(r);
        let (a, b) = (builder::vertex(p3(p + fr.t1 * sb)), builder::vertex(p3(p + fr.t2 * sb)));
        let arc = builder::circle_arc(&a, &b, p3(arc_mid(p, sb, r)));
        ends.push(End { a, b, arc: Some(Arc { v: v.clone(), g, e1, e2, edge: arc }) });
    }
    let [end0, end1] = &ends[..] else { return Err(KernelError::Failed("ends".into())) };
    if let (Some(x), Some(y)) = (&end0.arc, &end1.arc)
        && x.g == y.g
    {
        return Err(unsupported("both ends touch the same face"));
    }
    // Along the edge the band the blend takes from each face stays inside it.
    const STATIONS: usize = 256;
    if (1..STATIONS).any(|k| !s.band_fits(k as f64 / STATIONS as f64, law.eval(k as f64 / STATIONS as f64))) {
        return Err(too_large("the blend is larger than the neighbouring faces"));
    }
    // Contact curves: the edge moved along each face by the setback of the radius there.
    let g = law.greville();
    let contact = |t: Vec3, a: &mt::Vertex, b: &mt::Vertex| -> Result<mt::Edge> {
        if law.degree == 1 && law.radii.len() == 2 {
            return Ok(builder::line(a, b));
        }
        let ctrl: Vec<mt::Point3> = g.iter().zip(&law.radii).map(|(gj, rj)| p3(p0 + (p1 - p0) * *gj + t * back(*rj))).collect();
        let c = mt::BSplineCurve::try_new(mt::KnotVec::from(law.knots.clone()), ctrl).map_err(|e| KernelError::Failed(format!("fillet: {e}")))?;
        mt::Edge::try_new(a, b, mt::Curve::BSplineCurve(c)).map_err(|e| KernelError::Failed(format!("fillet: {e}")))
    };
    let la = contact(fr.t1, &end0.a, &end1.a)?;
    let lb = contact(fr.t2, &end0.b, &end1.b)?;
    let arcs = || [end0, end1].into_iter().filter_map(|end| end.arc.as_ref().map(|arc| (end, arc)));
    let mut subst: HashMap<mt::EdgeID, mt::Edge> = HashMap::new();
    for (end, arc) in arcs() {
        for (e, nv) in [(&arc.e1, &end.a), (&arc.e2, &end.b)] {
            let (f, bk) = (e.absolute_front(), e.absolute_back());
            let ne = if f == &arc.v { builder::line(nv, bk) } else { builder::line(f, nv) };
            subst.insert(e.id(), ne);
        }
    }
    // The side edges at the ends are pieces of themselves; the contacts and end arcs are new.
    let mut rec = crate::record::Rebuild::default();
    rec.pieces(&subst);
    let mut new_faces: Vec<mt::Face> = Vec::with_capacity(faces.len() + 1);
    for (i, f) in faces.iter().enumerate() {
        let nf = if i == fr.i1 {
            let mut m = subst.clone();
            m.insert(edge.id(), la.clone());
            rebuild_face(f, &m, &[])?
        } else if i == fr.i2 {
            let mut m = subst.clone();
            m.insert(edge.id(), lb.clone());
            rebuild_face(f, &m, &[])?
        } else if let Some((_, arc)) = arcs().find(|(_, arc)| arc.g == i) {
            rebuild_face(f, &subst, std::slice::from_ref(&arc.edge))?
        } else {
            (*f).clone()
        };
        rec.face(&nf, f);
        new_faces.push(nf);
    }
    // The blend surface: rational quadratic arcs (tangent points and the edge point, middle
    // weight sin(φ/2)) at each end, joined along the radius spline.
    let w = (fr.phi / 2.0).sin();
    let h = |p: Vec3, wt: f64| mt::Vector4::new(p.x * wt, p.y * wt, p.z * wt, wt);
    let row = |t: Vec3, wt: f64, off: bool| -> Vec<mt::Vector4> {
        g.iter().zip(&law.radii).map(|(gj, rj)| h(p0 + (p1 - p0) * *gj + if off { t * back(*rj) } else { Vec3::ZERO }, wt)).collect()
    };
    let ctrl = vec![row(fr.t1, 1.0, true), row(fr.t1, w, false), row(fr.t2, 1.0, true)];
    let bsp = mt::BSplineSurface::new((mt::KnotVec::bezier_knot(2), mt::KnotVec::from(law.knots.clone())), ctrl);
    let mut surf = mt::Surface::NurbsSurface(mt::NurbsSurface::new(bsp));
    // Outward: away from the arc centre on a convex edge, toward it on a concave one (at an
    // end with a radius).
    let (p, r) = if end0.arc.is_none() { (p1, rb) } else { (p0, ra) };
    let (c, mid) = (p + fr.t1 * back(r) + fr.n1 * (r * fr.side), arc_mid(p, back(r), r));
    let outward = (mid - c) * (-fr.side);
    {
        use mt::{ParametricSurface3D, SearchParameter};
        if let Some((u, v)) = surf.search_parameter(p3(mid), None, 100) {
            let nn = surf.normal(u, v);
            if Vec3::new(nn.x, nn.y, nn.z).dot(outward) < 0.0 {
                surf = mt::Invertible::inverse(&surf);
            }
        }
    }
    // The blend face uses each new edge opposite to its neighbour; a closed end has no arc.
    let arc_of = |end: &End| end.arc.as_ref().map(|a| a.edge.clone());
    let f1_forward = faces.get(fr.i1).is_some_and(|f| f.boundaries().iter().any(|w| w.edge_iter().any(|e| e.id() == edge.id() && e.front() == v0)));
    let wire: mt::Wire = if f1_forward {
        [arc_of(end0), Some(lb.clone()), arc_of(end1).map(|a| a.inverse()), Some(la.inverse())].into_iter().flatten().collect()
    } else {
        [Some(la.clone()), arc_of(end1), Some(lb.inverse()), arc_of(end0).map(|a| a.inverse())].into_iter().flatten().collect()
    };
    let blend = mt::Face::try_new(vec![wire], surf).map_err(|e| KernelError::Failed(format!("fillet face: {e}")))?;
    rec.blend(&blend, [edge.id()]);
    new_faces.push(blend);
    let solid: &Solid = &body.solid;
    let mut shells = solid.boundaries().clone();
    if let Some(sh) = shells.get_mut(s.si) {
        *sh = new_faces.into();
    }
    let out = Solid::try_new(shells).map_err(|e| KernelError::Failed(format!("variable fillet produced an invalid solid: {e}")))?;
    crate::record::rebuilt(out, &[solid], &rec)
}

/// How much of the room along an edge a taper takes: its band stays within this fraction of each
/// face's width, so the faces keep a strip beside the round, narrowing to nothing only where a
/// face closes to a point.
const TAPER_SHARE: f64 = 0.9;
/// Spans of the cubic radius spline a taper builds.
const TAPER_SPANS: usize = 8;

/// The radius a taper gives one edge: its positions from `start`.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Taper {
    pub start: Vec3,
    pub spline: RadiusSpline,
}

/// The radius spline that tapers `radius` on the straight edge at `edge_at` to the room along
/// it: `radius` where that fits, following the room down where it doesn't, to zero at an end
/// where a face closes to a point. Its control radii are the target min(radius, share × room)
/// at their Greville positions (Schoenberg's variation-diminishing spline), which stays under the
/// target wherever the room is concave along the edge (a convex face); the blend checks the rest.
pub fn taper(body: &Body, edge_at: Vec3, radius: f64) -> Result<Taper> {
    body.require_brep("fillet")?;
    if !(radius.is_finite() && radius > 1e-6 && radius < 1e6) {
        return Err(KernelError::Invalid("fillet radii must be positive".into()));
    }
    let edge = crate::surfaces::edge_near(body, edge_at).ok_or_else(|| KernelError::Invalid(format!("no edge at {edge_at:?}")))?;
    guard("taper", || {
        let s = EdgeSetup::new(body, &edge)?;
        let leans = || unsupported("a taper needs each end of the edge square to it or closing to a point");
        let room0 = s.end_room(&s.v0, -s.d).ok_or_else(leans)?;
        let room1 = s.end_room(&s.v1, s.d).ok_or_else(leans)?;
        if room0 == 0.0 && room1 == 0.0 {
            return Err(too_large("the faces close to a point at both ends of the edge"));
        }
        let mut knots = vec![0.0; 4];
        knots.extend((1..TAPER_SPANS).map(|k| k as f64 / TAPER_SPANS as f64));
        knots.extend([1.0; 4]);
        let mut law = RadiusSpline { degree: 3, knots, radii: vec![0.0; TAPER_SPANS + 3] };
        let g = law.greville();
        let last = g.len() - 1;
        law.radii = g
            .iter()
            .enumerate()
            .map(|(j, u)| match j {
                0 => (TAPER_SHARE * room0).min(radius),
                j if j == last => (TAPER_SHARE * room1).min(radius),
                _ => TAPER_SHARE * s.room(*u, radius / TAPER_SHARE),
            })
            .collect();
        if law.radii.iter().take(last).skip(1).any(|r| *r <= 1e-6) {
            return Err(too_large("the room for the blend runs out inside the edge"));
        }
        law.check()?;
        Ok(Taper { start: s.p0, spline: law })
    })
}

/// A constant fillet of `edges` at `radius`; where that is too large, each edge it doesn't fit
/// on alone gets a [`taper`], and the rest the constant radius. The result and the taper of each
/// edge (in `edges`' order; None where the radius is constant).
pub fn fillet_tapered(body: &Body, edges: &[Vec3], radius: f64) -> Result<(Body, Vec<Option<Taper>>)> {
    crate::memo::scope(|| fillet_tapered_in_scope(body, edges, radius))
}

fn fillet_tapered_in_scope(body: &Body, edges: &[Vec3], radius: f64) -> Result<(Body, Vec<Option<Taper>>)> {
    let why = match crate::fillet(body, edges, radius) {
        Err(KernelError::TooLarge(why)) => why,
        other => return other.map(|b| (b, vec![None; edges.len()])),
    };
    let mut tapers = vec![None; edges.len()];
    let mut constant = Vec::new();
    for (p, slot) in edges.iter().zip(tapers.iter_mut()) {
        match crate::fillet(body, &[*p], radius) {
            Ok(_) => constant.push(*p),
            Err(KernelError::TooLarge(_)) => *slot = Some(taper(body, *p, radius)?),
            Err(e) => return Err(e),
        }
    }
    // Each edge fits alone but not together: nothing to taper.
    if tapers.iter().all(Option::is_none) {
        return Err(KernelError::TooLarge(why));
    }
    // Each step's result carries its history over `[body]`.
    let counts = crate::record::counts(&[body]);
    let mut cur = crate::boolean_record::operand(body, 0, &counts);
    for (p, t) in edges.iter().zip(&tapers) {
        if let Some(t) = t {
            cur = crate::record::after(fillet_variable_spline(&cur, *p, t.start, &t.spline)?, &cur, &counts);
        }
    }
    if !constant.is_empty() {
        cur = crate::record::after(crate::fillet(&cur, &constant, radius)?, &cur, &counts);
    }
    Ok((cur, tapers))
}

/// Chord-length fillet: each edge rounded so that the blend is `chord` wide across, from contact
/// to contact at every station. A straight edge between planar faces is built exactly, the radius
/// `chord / (2 cos(φ/2))` from its face angle and edges sharing that angle blended together; a
/// curved edge, or one between curved faces (or a chain of selected edges running on into one),
/// is rolled by a ball whose radius gives the same width, which follows the face angle along it.
pub fn fillet_chord(body: &Body, edges: &[Vec3], chord: f64) -> Result<Body> {
    body.require_brep("fillet")?;
    if !(chord.is_finite() && chord > 1e-6 && chord < 1e6) {
        return Err(KernelError::Invalid("the chord length must be positive".into()));
    }
    if edges.is_empty() || edges.len() > 1000 {
        return Err(KernelError::Invalid("select 1…1000 edges".into()));
    }
    let found: Vec<mt::Edge> = edges
        .iter()
        .map(|p| crate::surfaces::edge_near(body, *p).ok_or_else(|| KernelError::Invalid(format!("no edge at {p:?}"))))
        .collect::<Result<_>>()?;
    // The exact radius of a straight edge between planar faces; None where the rolling ball has to
    // take the edge (a curved edge, or one between curved faces).
    let exact_radius = |e: &mt::Edge| -> Option<f64> {
        if !matches!(e.curve(), mt::Curve::Line(_)) {
            return None;
        }
        let shell = body.solid.boundaries().iter().find(|sh| sh.edge_iter().any(|x| x.id() == e.id()))?;
        let faces: Vec<&mt::Face> = shell.face_iter().collect();
        Some(chord_radius(chord, edge_frame(&faces, e).ok()?.phi))
    };
    // A straight edge running on smoothly into a selected curved one goes with it: the ball rolls
    // along the chain, so blending the parts apart would leave a step at their joint.
    let meets = |a: &mt::Edge, b: &mt::Edge| [a.front(), a.back()].into_iter().any(|v| v == b.front() || v == b.back());
    let mixed = |e: &mt::Edge| found.iter().any(|f| f.id() != e.id() && exact_radius(f).is_none() && meets(e, f));
    // Radius per edge from its face angle; the rest roll a ball along their chain.
    let mut groups: Vec<(f64, Vec<Vec3>)> = Vec::new();
    let mut rolled: Vec<Vec<Vec3>> = Vec::new();
    let mut done: Vec<mt::EdgeID> = Vec::new();
    for (p, e) in edges.iter().zip(&found) {
        if let Some(r) = exact_radius(e).filter(|_| !mixed(e)) {
            match groups.iter_mut().find(|(gr, _)| (gr - r).abs() <= 1e-9 * r) {
                Some((_, v)) => v.push(*p),
                None => groups.push((r, vec![*p])),
            }
            continue;
        }
        if done.contains(&e.id()) {
            continue;
        }
        // The whole smooth chain (as the constant-radius path rolls it): a ball cannot stop
        // where the edge runs on, so the chain's other edges are blended with it.
        let chain = crate::blend::smooth_chain(&body.solid, e).unwrap_or_else(|| vec![e.id()]);
        done.extend(chain.iter().copied());
        rolled.push(edges.iter().zip(&found).filter(|(_, f)| chain.contains(&f.id())).map(|(p, _)| *p).collect());
    }
    // Each step's result carries its history over `[body]`.
    let counts = crate::record::counts(&[body]);
    let mut cur = crate::boolean_record::operand(body, 0, &counts);
    for (r, pts) in groups {
        cur = crate::record::after(crate::fillet(&cur, &pts, r)?, &cur, &counts);
    }
    for picks in rolled {
        // The chain as the current body has it: the blends before it rebuilt the faces it runs
        // through, and a rebuilt face carries its edges again.
        let edges: Vec<mt::Edge> = picks.iter().filter_map(|p| crate::surfaces::edge_near(&cur, *p)).collect();
        let Some(first) = edges.first() else { continue };
        let keep = |x: &mt::Edge| edges.iter().any(|e| e.id() == x.id());
        let chain = crate::blend::smooth_chain_within(&cur.solid, first, &keep).unwrap_or_else(|| vec![first.id()]);
        let (made, rec) = crate::guard("chord fillet", || crate::curveblend::curve_blend_chord(&cur.solid, &chain, cur.size(), chord, true))?;
        let step = crate::record::rebuilt(made, &[&cur.solid], &rec)?;
        cur = crate::record::after(step, &cur, &counts);
    }
    Ok(cur)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{box_solid, measure};

    fn rel(a: f64, b: f64) -> f64 {
        (a - b).abs() / b.abs().max(1e-9)
    }

    /// Inserting a knot keeps the curve and adds one control radius; removing a control radius
    /// takes out one inner knot and keeps the ends.
    #[test]
    fn radius_spline_knot_insertion_and_removal() {
        let law =
            RadiusSpline { degree: 3, knots: vec![0.0, 0.0, 0.0, 0.0, 0.25, 0.6, 1.0, 1.0, 1.0, 1.0], radii: vec![2.0, 5.0, 1.5, 4.0, 1.0, 3.0] };
        for u in [0.1, 0.25 + 1e-3, 0.4, 0.9] {
            let more = law.insert(u).unwrap();
            assert_eq!((more.radii.len(), more.knots.len()), (7, 11));
            assert!(more.check().is_ok());
            let worst = (0..=400).map(|i| f64::from(i) / 400.0).map(|t| (more.eval(t) - law.eval(t)).abs()).fold(0.0, f64::max);
            assert!(worst < 1e-12, "insert {u}: off by {worst}");
            // Removing the control radius just added drops a knot again.
            let j = more.greville().iter().enumerate().min_by(|a, b| (a.1 - u).abs().total_cmp(&(b.1 - u).abs())).unwrap().0;
            let less = more.remove(j).unwrap();
            assert_eq!((less.radii.len(), less.knots.len()), (6, 10));
            assert!(less.check().is_ok());
        }
        // Not at an end, on a knot, or outside.
        assert!(law.insert(0.0).is_none() && law.insert(0.6).is_none() && law.insert(1.2).is_none());
        // Ends stay, and a single cubic span has no inner knot to give up.
        assert!(law.remove(0).is_none() && law.remove(5).is_none());
        let bare = RadiusSpline { degree: 3, knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], radii: vec![2.0, 3.0, 3.0, 2.0] };
        assert!(bare.remove(1).is_none());
        let less = law.remove(2).unwrap();
        assert_eq!((less.eval(0.0), less.eval(1.0)), (2.0, 3.0));
    }

    /// The material a variable fillet removes from a right-angled edge of length L, radius
    /// running r0 → r1: ∫ (1 − π/4) r(t)² dt = (1 − π/4) L (r0² + r0 r1 + r1²) / 3.
    #[test]
    fn variable_radius_on_a_box_edge() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        // The edge along y at x = 10, z = 30; radius 2 at y = 0, 5 at y = 20.
        let f = fillet_variable(&b, Vec3::new(10.0, 10.0, 30.0), Vec3::new(10.0, 0.0, 30.0), 2.0, 5.0).unwrap();
        let removed = (1.0 - std::f64::consts::FRAC_PI_4) * 20.0 * (4.0 + 10.0 + 25.0) / 3.0;
        let v = measure(&f).unwrap().volume;
        assert!(rel(v, 6000.0 - removed) < 1e-4, "{v} vs {}", 6000.0 - removed);
        // Started from the other end: the same solid.
        let h = fillet_variable(&b, Vec3::new(10.0, 10.0, 30.0), Vec3::new(10.0, 20.0, 30.0), 5.0, 2.0).unwrap();
        assert!(rel(measure(&h).unwrap().volume, v) < 1e-9);
        // Equal radii give the constant fillet.
        let g = fillet_variable(&b, Vec3::new(10.0, 10.0, 30.0), Vec3::new(10.0, 0.0, 30.0), 3.0, 3.0).unwrap();
        let k = crate::fillet(&b, &[Vec3::new(10.0, 10.0, 30.0)], 3.0).unwrap();
        assert!(rel(measure(&g).unwrap().volume, measure(&k).unwrap().volume) < 1e-6);
        // Too big for the faces: refused.
        assert!(fillet_variable(&b, Vec3::new(10.0, 10.0, 30.0), Vec3::new(10.0, 0.0, 30.0), 2.0, 50.0).is_err());
    }

    /// A radius through a middle point: straight pieces (exact: each piece removes
    /// (1 − π/4) L (a² + ab + b²) / 3), and smooth (the removed volume integrates the law).
    #[test]
    fn variable_radius_through_a_middle_point() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        let (at, start) = (Vec3::new(10.0, 10.0, 30.0), Vec3::new(10.0, 0.0, 30.0));
        let k = 1.0 - std::f64::consts::FRAC_PI_4;
        let pts = [(0.0, 2.0), (0.5, 4.0), (1.0, 2.0)];
        let f = fillet_variable_points(&b, at, start, &pts, false).unwrap();
        let removed = 2.0 * k * 10.0 * (4.0 + 8.0 + 16.0) / 3.0;
        let v = measure(&f).unwrap().volume;
        assert!(rel(v, 6000.0 - removed) < 1e-4, "{v} vs {}", 6000.0 - removed);
        assert_eq!(f.unmeshed_faces(), 0);
        let s = fillet_variable_points(&b, at, start, &pts, true).unwrap();
        let law = RadiusSpline::through(&pts, true);
        let n = 20000;
        let integral: f64 = (0..n).map(|i| law.eval((i as f64 + 0.5) / n as f64).powi(2)).sum::<f64>() / n as f64 * 20.0;
        assert!((law.eval(0.5) - 4.0).abs() < 1e-12 && (law.eval(0.0) - 2.0).abs() < 1e-12);
        let v = measure(&s).unwrap().volume;
        assert!(rel(v, 6000.0 - k * integral) < 1e-4, "{v} vs {}", 6000.0 - k * integral);
        // Unsorted, from the other end: the same.
        let r = fillet_variable_points(&b, at, Vec3::new(10.0, 20.0, 30.0), &[(1.0, 2.0), (0.0, 2.0), (0.5, 4.0)], true).unwrap();
        assert!(rel(measure(&r).unwrap().volume, v) < 1e-9);
        // A middle radius too big for the faces, missing ends, bad positions: refused.
        assert!(fillet_variable_points(&b, at, start, &[(0.0, 2.0), (0.5, 40.0), (1.0, 2.0)], false).is_err());
        assert!(fillet_variable_points(&b, at, start, &[(0.0, 2.0), (0.5, 4.0)], false).is_err());
        assert!(fillet_variable_points(&b, at, start, &[(0.0, 2.0), (f64::NAN, 4.0), (1.0, 2.0)], false).is_err());
        assert!(fillet_variable_points(&b, at, start, &[(0.0, 2.0), (1.0, -1.0)], false).is_err());
    }

    /// The radius running down to zero at one end: the blend closes to the corner, a three-sided
    /// face, and the end face there is untouched. Removed (1 − π/4) L r0² / 3 when linear.
    #[test]
    fn variable_radius_down_to_zero_at_one_end() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        let (at, start, end) = (Vec3::new(10.0, 10.0, 30.0), Vec3::new(10.0, 0.0, 30.0), Vec3::new(10.0, 20.0, 30.0));
        let k = 1.0 - std::f64::consts::FRAC_PI_4;
        for (r0, r1) in [(4.0, 0.0), (0.0, 4.0)] {
            let f = fillet_variable(&b, at, start, r0, r1).unwrap();
            let v = measure(&f).unwrap().volume;
            assert!(rel(v, 6000.0 - k * 20.0 * 16.0 / 3.0) < 1e-4, "{r0} → {r1}: {v}");
            assert_eq!(f.unmeshed_faces(), 0);
            let faces: Vec<&mt::Face> = f.solid.boundaries().iter().flat_map(|sh| sh.face_iter()).collect();
            assert_eq!(faces.len(), 7);
            // The blend is three-sided; the closed end's corner is still a box corner.
            assert!(faces.iter().any(|fc| fc.edge_iter().count() == 3));
            let corner = if r1 == 0.0 { end } else { start };
            assert!(faces.iter().flat_map(|fc| fc.vertex_iter()).any(|x| vtx(&x).dist(corner) < 1e-9));
        }
        // Through a middle point, smooth, closing at the far end.
        let pts = [(0.0, 3.0), (0.5, 4.0), (1.0, 0.0)];
        let s = fillet_variable_points(&b, at, start, &pts, true).unwrap();
        let law = RadiusSpline::through(&pts, true);
        let n = 20000;
        let integral: f64 = (0..n).map(|i| law.eval((i as f64 + 0.5) / n as f64).powi(2)).sum::<f64>() / n as f64 * 20.0;
        let v = measure(&s).unwrap().volume;
        assert!(rel(v, 6000.0 - k * integral) < 1e-4, "{v} vs {}", 6000.0 - k * integral);
        // Zero at both ends, zero in the middle, a smooth radius dipping below zero, or a radius
        // too small to build: refused.
        assert!(fillet_variable(&b, at, start, 0.0, 0.0).is_err());
        assert!(fillet_variable_points(&b, at, start, &[(0.0, 2.0), (0.5, 0.0), (1.0, 2.0)], false).is_err());
        let dip = [(0.0, 0.0), (0.2, 0.1), (0.4, 6.0), (1.0, 6.0)];
        assert!((1..200).any(|i| RadiusSpline::through(&dip, true).eval(i as f64 / 200.0) <= 1e-6));
        assert!(fillet_variable_points(&b, at, start, &dip, true).is_err());
        assert!(fillet_variable(&b, at, start, 4.0, 1e-9).is_err());
    }

    /// A prism of the polygon `pts` (in xy) from z = 0 to `h`.
    fn prism(pts: &[(f64, f64)], h: f64) -> Body {
        use solidcraft_geom::{Loop2, Plane, Region2, Vec2};
        let outer = Loop2::polygon(&pts.iter().map(|(x, y)| Vec2::new(*x, *y)).collect::<Vec<_>>());
        crate::extrude(&Plane::XY, &[Region2 { outer, holes: vec![] }], 0.0, h).unwrap().pop().unwrap()
    }

    /// The material a right-angled blend with the radius `law` removes along an edge of length L:
    /// ∫ (1 − π/4) r² over the edge.
    fn removed(law: &RadiusSpline, len: f64) -> f64 {
        let n = 20000;
        (1.0 - std::f64::consts::FRAC_PI_4) * (0..n).map(|i| law.eval((i as f64 + 0.5) / n as f64).powi(2)).sum::<f64>() / n as f64 * len
    }

    /// The volume of a fine tessellation: at a blend closed to a point `measure` takes a
    /// coarser mesh of the blend, a few mm³ off on these parts.
    fn fine_volume(b: &Body) -> f64 {
        b.tessellate_with(5e-4, true).unwrap().measure().volume
    }

    /// A taper on the top front edge of a wedge, whose top face narrows to a point, and of a
    /// trapezoid, whose top face narrows to 2 mm: the radius asked for where it fits, below
    /// 0.9 of the room everywhere, down to zero at the wedge's tip and to 1.8 mm at the
    /// trapezoid's narrow end; the volume integrates the spline.
    #[test]
    fn taper_follows_the_room_along_the_edge() {
        let (at, start) = (Vec3::new(20.0, 0.0, 15.0), Vec3::new(0.0, 0.0, 15.0));
        let wedge = prism(&[(0.0, 0.0), (40.0, 0.0), (0.0, 20.0)], 15.0);
        assert!(matches!(crate::fillet(&wedge, &[at], 16.0), Err(KernelError::TooLarge(_))));
        let t = taper(&wedge, at, 16.0).unwrap();
        let law = if t.start.dist(start) < 1e-9 { t.spline.clone() } else { t.spline.reversed() };
        // The room: the top face is 20 (1 − u) wide, the front 15 high.
        for i in 0..=100 {
            let u = i as f64 / 100.0;
            let r = law.eval(u);
            assert!(r <= 0.9 * (20.0 * (1.0 - u)).min(15.0) + 1e-9 && r <= 16.0, "{u}: {r}");
        }
        assert_eq!(law.eval(1.0), 0.0);
        assert!((law.eval(0.0) - 13.5).abs() < 1e-9);
        let (f, tapers) = fillet_tapered(&wedge, &[at], 16.0).unwrap();
        assert_eq!(tapers, vec![Some(t)]);
        let v = fine_volume(&f);
        assert!(rel(v, 6000.0 - removed(&law, 40.0)) < 1e-4, "{v} vs {}", 6000.0 - removed(&law, 40.0));
        assert_eq!(f.unmeshed_faces(), 0);
        let faces: Vec<&mt::Face> = f.solid.boundaries().iter().flat_map(|sh| sh.face_iter()).collect();
        assert!(faces.iter().any(|fc| fc.edge_iter().count() == 3));

        let trap = prism(&[(0.0, 0.0), (40.0, 0.0), (40.0, 2.0), (0.0, 20.0)], 15.0);
        assert!(matches!(crate::fillet(&trap, &[at], 3.0), Err(KernelError::TooLarge(_))));
        let (f, tapers) = fillet_tapered(&trap, &[at], 3.0).unwrap();
        let t = tapers.into_iter().flatten().next().unwrap();
        let law = if t.start.dist(start) < 1e-9 { t.spline } else { t.spline.reversed() };
        assert!((law.eval(0.0) - 3.0).abs() < 1e-9 && (law.eval(1.0) - 1.8).abs() < 1e-9);
        let v = fine_volume(&f);
        assert!(rel(v, 6600.0 - removed(&law, 40.0)) < 1e-4, "{v} vs {}", 6600.0 - removed(&law, 40.0));
        // A radius that fits stays constant.
        let (g, tapers) = fillet_tapered(&trap, &[at], 1.0).unwrap();
        assert_eq!(tapers, vec![None]);
        assert!(rel(measure(&g).unwrap().volume, measure(&crate::fillet(&trap, &[at], 1.0).unwrap()).unwrap().volume) < 1e-12);
    }

    /// A radius spline given directly: a cubic with its control radii pulling the radius up in
    /// the middle; the same from the other end; refused when it isn't clamped or dips to zero.
    #[test]
    fn variable_radius_from_a_spline() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        let (at, start) = (Vec3::new(10.0, 10.0, 30.0), Vec3::new(10.0, 0.0, 30.0));
        let law = RadiusSpline { degree: 3, knots: vec![0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0], radii: vec![2.0, 2.0, 6.0, 3.0, 0.0] };
        let f = fillet_variable_spline(&b, at, start, &law).unwrap();
        let v = fine_volume(&f);
        assert!(rel(v, 6000.0 - removed(&law, 20.0)) < 1e-4, "{v} vs {}", 6000.0 - removed(&law, 20.0));
        let r = fillet_variable_spline(&b, at, Vec3::new(10.0, 20.0, 30.0), &law.reversed()).unwrap();
        assert!(rel(fine_volume(&r), v) < 1e-9);
        let unclamped = RadiusSpline { knots: vec![0.0, 0.0, 0.0, 0.1, 0.5, 1.0, 1.0, 1.0, 1.0], ..law.clone() };
        assert!(fillet_variable_spline(&b, at, start, &unclamped).is_err());
        let dips = RadiusSpline { radii: vec![2.0, 0.0, 0.0, 3.0, 2.0], ..law.clone() };
        assert!(fillet_variable_spline(&b, at, start, &dips).is_err());
        // A middle radius beyond the faces: too large.
        let big = RadiusSpline { radii: vec![2.0, 2.0, 40.0, 3.0, 2.0], ..law };
        assert!(matches!(fillet_variable_spline(&b, at, start, &big), Err(KernelError::TooLarge(_))));
    }

    #[test]
    fn chord_length_on_a_right_angle() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        // A right angle: chord c gives radius c / √2.
        let f = fillet_chord(&b, &[Vec3::new(10.0, 10.0, 30.0)], 4.0).unwrap();
        let k = crate::fillet(&b, &[Vec3::new(10.0, 10.0, 30.0)], 4.0 / 2f64.sqrt()).unwrap();
        assert!(rel(measure(&f).unwrap().volume, measure(&k).unwrap().volume) < 1e-9);
    }
}

#[cfg(test)]
#[path = "taper_mesh_tests.rs"]
mod taper_mesh_tests;
