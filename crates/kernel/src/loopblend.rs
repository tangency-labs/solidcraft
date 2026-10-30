//! Blending a whole boundary loop of a planar face whose neighbours are walls standing
//! perpendicular to it (planes, and cylinders with axes along the face normal): a pocket floor,
//! the top outline of an extruded plate. Lines and arcs can meet tangentially or at mitred sharp
//! corners. The face shrinks by the radius, the walls are trimmed by it, and each loop edge
//! gets a blend face: a cylinder (or plane, for a chamfer) along a line, a torus (or cone) along
//! an arc, all meeting along the profile curves at the joints. Sharp joints at perpendicular
//! curved walls follow the intersections of the exact offsets, fitted within tolerance.
//! A chamfer may also set back by different distances on the face and up the walls (two
//! distances, or a distance and an angle: [`loop_chamfer_sides`]).
//! Free-form edges in the face plane (the outline of an extruded spline) take part too when
//! their walls stand square to the face: their blend surface is fitted within tolerance through
//! the edge's offset on the face and its copy on the wall, and sharp corners between them
//! follow the crossing of the offsets, found numerically.

use std::collections::HashMap;
use std::rc::Rc;

use solidcraft_geom::Vec3;
use truck_modeling::{self as mt, builder};

use crate::blend::{ChamferSide, too_large};
use crate::body::{Solid, from_p3, p3, v3};
use crate::record::Rebuild;
use crate::{KernelError, Result};

fn unsupported(msg: &str) -> KernelError {
    KernelError::Failed(format!("not supported yet: {msg}"))
}

#[derive(Clone, Debug)]
enum Seg {
    Line {
        a: Vec3,
        b: Vec3,
    },
    /// Arc from `a` to `b` about `c` turning by `angle` about `axis` (right hand, angle > 0).
    Arc {
        a: Vec3,
        c: Vec3,
        axis: Vec3,
        angle: f64,
        mid: Vec3,
    },
    /// A free-form curve in the face plane from `a` to `b`.
    Curve {
        a: Vec3,
        b: Vec3,
        mid: Vec3,
        span: Rc<Span>,
    },
}

/// A loop edge's oriented curve by a parameter from 0 to 1, running on beyond its ends as the
/// parabola of its end point, tangent and curvature (where an offset reaches past a sharp corner;
/// smooth enough there to fit through).
#[derive(Debug)]
struct Span {
    curve: mt::Curve,
    t0: f64,
    t1: f64,
}

impl Span {
    /// The point at `u` and the derivative there.
    fn at(&self, u: f64) -> (Vec3, Vec3) {
        use mt::ParametricCurve;
        let inside = u.clamp(0.0, 1.0);
        let t = self.t0 + (self.t1 - self.t0) * inside;
        let scale = self.t1 - self.t0;
        let (d, d2) = (self.curve.der(t), self.curve.der2(t));
        let (d, d2) = (Vec3::new(d.x, d.y, d.z) * scale, Vec3::new(d2.x, d2.y, d2.z) * (scale * scale));
        let past = u - inside;
        (from_p3(self.curve.subs(t)) + d * past + d2 * (past * past / 2.0), d + d2 * past)
    }
    /// The parameter of the curve's point nearest `p`.
    fn param_of(&self, p: Vec3) -> f64 {
        use mt::SearchNearestParameter;
        let (a, b) = (self.at(0.0).0, self.at(1.0).0);
        let near = (a.dist(b) * 1e-9).max(1e-12);
        if p.dist(a) <= near {
            return 0.0;
        }
        if p.dist(b) <= near {
            return 1.0;
        }
        let mid = (self.t0 + self.t1) / 2.0;
        let t = self.curve.search_nearest_parameter(p3(p), Some(mid), 100).unwrap_or(mid);
        ((t - self.t0) / (self.t1 - self.t0)).clamp(0.0, 1.0)
    }
}

impl Seg {
    fn start(&self) -> Vec3 {
        match *self {
            Seg::Line { a, .. } | Seg::Arc { a, .. } | Seg::Curve { a, .. } => a,
        }
    }
    fn end(&self) -> Vec3 {
        match *self {
            Seg::Line { b, .. } | Seg::Curve { b, .. } => b,
            Seg::Arc { a, c, axis, angle, .. } => turn(a, c, axis, angle),
        }
    }
    /// Unit tangent (direction of travel) at a point of the segment.
    fn tangent(&self, p: Vec3) -> Option<Vec3> {
        match self {
            Seg::Line { a, b } => (*b - *a).normalized(),
            Seg::Arc { c, axis, .. } => axis.cross(p - *c).normalized(),
            Seg::Curve { span, .. } => span.at(span.param_of(p)).1.normalized(),
        }
    }
    /// The point at parameter `u` (0 at the start, 1 at the end, beyond them along the line, the
    /// circle or the end tangents).
    fn point(&self, u: f64) -> Vec3 {
        match self {
            Seg::Line { a, b } => *a + (*b - *a) * u,
            Seg::Arc { a, c, axis, angle, .. } => turn(*a, *c, *axis, angle * u),
            Seg::Curve { span, .. } => span.at(u).0,
        }
    }
    /// Unit tangent at parameter `u`.
    fn tangent_at(&self, u: f64) -> Option<Vec3> {
        match self {
            Seg::Curve { span, .. } => span.at(u).1.normalized(),
            _ => self.tangent(self.point(u)),
        }
    }
    fn mid(&self) -> Vec3 {
        match *self {
            Seg::Line { a, b } => (a + b) * 0.5,
            Seg::Arc { mid, .. } | Seg::Curve { mid, .. } => mid,
        }
    }
    fn free(&self) -> bool {
        matches!(self, Seg::Curve { .. })
    }
}

/// Intersection of two inward offsets nearest their shared corner, in the face plane.
fn offset_joint(prev: &Seg, cur: &Seg, n: Vec3, d: f64) -> Option<Vec3> {
    if prev.free() || cur.free() {
        return offset_joint_free(prev, cur, n, d).map(|x| x.0);
    }
    let p = cur.start();
    let line_circle = |line: &Seg, arc: &Seg| {
        let Seg::Arc { a, c, .. } = *arc else { return None };
        let tangent = line.tangent(p)?;
        let origin = p + n.cross(tangent) * d;
        let radius = a.dist(c) + n.cross(arc.tangent(p)?).dot((p - c).normalized()?) * d;
        let foot = origin + tangent * (c - origin).dot(tangent);
        let height2 = radius * radius - foot.dist(c).powi(2);
        if !(radius > 0.0 && height2 >= 0.0) {
            return None;
        }
        let (a, b) = (foot + tangent * height2.sqrt(), foot - tangent * height2.sqrt());
        Some(if a.dist(p) < b.dist(p) { a } else { b })
    };
    match (prev, cur) {
        (Seg::Line { .. }, Seg::Arc { .. }) => line_circle(prev, cur),
        (Seg::Arc { .. }, Seg::Line { .. }) => line_circle(cur, prev),
        (Seg::Arc { a: a0, c: c0, .. }, Seg::Arc { a: a1, c: c1, .. }) => {
            let delta = *c1 - *c0;
            let distance = delta.len();
            let axis = delta.normalized()?;
            let r0 = a0.dist(*c0) + n.cross(prev.tangent(p)?).dot((p - *c0).normalized()?) * d;
            let r1 = a1.dist(*c1) + n.cross(cur.tangent(p)?).dot((p - *c1).normalized()?) * d;
            let along = (r0 * r0 - r1 * r1 + distance * distance) / (2.0 * distance);
            let height2 = r0 * r0 - along * along;
            if !(r0 > 0.0 && r1 > 0.0 && height2 >= 0.0) {
                return None;
            }
            let foot = *c0 + axis * along;
            let across = n.cross(axis) * height2.sqrt();
            let (a, b) = (foot + across, foot - across);
            Some(if a.dist(p) < b.dist(p) { a } else { b })
        }
        _ => None,
    }
}

/// [`offset_joint`] by Newton's method, for any two segments (a free-form one among them), with
/// the parameters of the crossing along `prev` and along `cur` (beyond 1 and below 0 past their
/// ends).
fn offset_joint_free(prev: &Seg, cur: &Seg, n: Vec3, d: f64) -> Option<(Vec3, f64, f64)> {
    let off = |s: &Seg, u: f64| Some(s.point(u) + n.cross(s.tangent_at(u)?) * d);
    let scale = prev.start().dist(prev.end()).max(cur.start().dist(cur.end())).max(d.abs()).max(1e-9);
    let (mut u0, mut u1) = (1.0f64, 0.0f64);
    let h = 1e-6;
    for _ in 0..60 {
        let (p0, p1) = (off(prev, u0)?, off(cur, u1)?);
        let f = p0 - p1;
        if f.len() <= scale * 1e-13 {
            break;
        }
        let j0 = (off(prev, u0 + h)? - off(prev, u0 - h)?) * (0.5 / h);
        let j1 = (off(cur, u1 + h)? - off(cur, u1 - h)?) * (-0.5 / h);
        let (a11, a12, a22) = (j0.dot(j0), j0.dot(j1), j1.dot(j1));
        let (b1, b2) = (-f.dot(j0), -f.dot(j1));
        let det = a11 * a22 - a12 * a12;
        // Parallel offsets never cross.
        if !(det.abs() > 1e-20 * a11 * a22) {
            return None;
        }
        u0 += ((b1 * a22 - b2 * a12) / det).clamp(-0.25, 0.25);
        u1 += ((a11 * b2 - a12 * b1) / det).clamp(-0.25, 0.25);
        if !(u0.is_finite() && u1.is_finite()) || !(-1.0..=3.0).contains(&u0) || !(-2.0..=2.0).contains(&u1) {
            return None;
        }
    }
    let (p0, p1) = (off(prev, u0)?, off(cur, u1)?);
    (p0.dist(p1) <= scale * 1e-9).then_some(((p0 + p1) * 0.5, u0, u1))
}

/// A free-form piece's blend: one surface through its offset on the face, its copy on the wall
/// and (for a round) the edge itself between them, and its two rails, the sides of the surface.
struct CurveFit {
    surface: mt::Surface,
    face: mt::Curve,
    wall: mt::Curve,
}

/// Whether `q` is on the surface of `g` within `tol`.
fn on_surface(g: &mt::Face, q: Vec3, tol: f64) -> bool {
    use mt::{ParametricSurface, SearchNearestParameter, SearchParameter};
    let s = g.oriented_surface();
    s.search_parameter(p3(q), None, 100)
        .or_else(|| s.search_nearest_parameter(p3(q), None, 100))
        .is_some_and(|(u, v)| from_p3(s.subs(u, v)).dist(q) <= tol)
}

/// [`CurveFit`] of a free-form piece set back by `sf` on the face and `sw` along its `wall`
/// (square to the face); `face_u`: where the face rail starts and ends (past the piece's ends,
/// or short of them, at a sharp corner). `up`: along the wall away from the edge.
#[allow(clippy::too_many_arguments)]
fn fit_free(s: &Seg, wall: &mt::Face, n: Vec3, up: Vec3, (sf, sw): (f64, f64), round: bool, face_u: (f64, f64), tol: f64) -> Result<CurveFit> {
    use mt::{Cut, ParametricCurve};
    let face_pt = |u: f64| s.tangent_at(u).map(|t| s.point(u) + n.cross(t) * sf);
    let wall_pt = |u: f64| s.point(u) + up * sw;
    // The wall stands square to the face all along, the edge's straight extrusion.
    for j in 0..=8 {
        let u = f64::from(j) / 8.0;
        let square = face_normal(wall, s.point(u)).is_some_and(|gn| gn.dot(n).abs() < 1e-6);
        if !square || !on_surface(wall, wall_pt(u), tol * 10.0) {
            return Err(unsupported("a free-form loop edge whose wall is not square to the face"));
        }
    }
    let snap = |u: f64, to: f64| if (u - to).abs() < 1e-6 { to } else { u };
    let (us, ue) = (snap(face_u.0, 0.0), snap(face_u.1, 1.0));
    if !(us < ue) {
        return Err(too_large("the blend is longer than a free-form edge"));
    }
    let (lo, hi) = (us.min(0.0) - 0.05, ue.max(1.0) + 0.05);
    let mut breaks = vec![lo, us, 0.0, 1.0, ue, hi];
    breaks.sort_by(f64::total_cmp);
    breaks.dedup();
    let span = hi - lo;
    let w_of = |u: f64| (u - lo) / span;
    for count in [8u32, 16, 32, 64, 128, 256] {
        let h = span / f64::from(count);
        let mut nodes = vec![lo];
        for win in breaks.windows(2) {
            let (&[a, b], true) = (win, true) else { continue };
            let parts = ((b - a) / h).ceil().clamp(1.0, 4096.0) as u32;
            nodes.extend((1..=parts).map(|j| if j == parts { b } else { a + (b - a) * f64::from(j) / f64::from(parts) }));
        }
        let w: Vec<f64> = nodes.iter().map(|u| w_of(*u)).collect();
        let fp: Vec<Vec3> = nodes.iter().map(|u| face_pt(*u)).collect::<Option<_>>().ok_or_else(|| unsupported("degenerate free-form edge"))?;
        // The offset on the face must run forward: past a bend tighter than the setback it folds.
        for (win, u) in fp.windows(2).zip(&nodes) {
            let (&[p, q], Some(t)) = (win, s.tangent_at(*u)) else { continue };
            if (q - p).dot(t) <= 0.0 {
                return Err(too_large("the blend is larger than a free-form edge's bend"));
            }
        }
        let wp: Vec<Vec3> = nodes.iter().map(|u| wall_pt(*u)).collect();
        let ep: Vec<Vec3> = nodes.iter().map(|u| s.point(*u)).collect();
        let fit = |pts: &[Vec3]| crate::build::interpolate_cubic_at(pts, &w);
        let (Some(fc), Some(wc)) = (fit(&fp), fit(&wp)) else { continue };
        let ec = if round {
            let Some(c) = fit(&ep) else { continue };
            Some(c)
        } else {
            None
        };
        let good = nodes.windows(2).all(|win| {
            let &[a, b] = win else { return false };
            let um = (a + b) / 2.0;
            let wm = w_of(um);
            let near = |c: &mt::BSplineCurve<mt::Point3>, want: Option<Vec3>| want.is_some_and(|x| from_p3(c.subs(wm)).dist(x) <= tol);
            near(&fc, face_pt(um)) && near(&wc, Some(wall_pt(um))) && ec.as_ref().is_none_or(|c| near(c, Some(s.point(um))))
        });
        if !good {
            continue;
        }
        let fail = |e: &dyn std::fmt::Display| KernelError::Failed(format!("free-form blend surface: {e}"));
        let row = |c: &mt::BSplineCurve<mt::Point3>, wt: f64| -> Vec<mt::Vector4> {
            c.control_points().iter().map(|p| mt::Vector4::new(p.x * wt, p.y * wt, p.z * wt, wt)).collect()
        };
        let vk = fc.knot_vec().clone();
        // A round's section is a quarter circle (the wall is square to the face): its control
        // polygon's corner is on the edge.
        let rows = match &ec {
            Some(ec) => vec![row(&fc, 1.0), row(ec, std::f64::consts::FRAC_1_SQRT_2), row(&wc, 1.0)],
            None => vec![row(&fc, 1.0), row(&wc, 1.0)],
        };
        let side_curve = |points: Option<&Vec<mt::Vector4>>, from: f64, to: f64| -> Result<mt::Curve> {
            let points = points.cloned().ok_or_else(|| fail(&"row"))?;
            let mut c = mt::NurbsCurve::new(mt::BSplineCurve::try_new(vk.clone(), points).map_err(|e| fail(&e))?);
            c = c.cut(from);
            let _ = c.cut(to);
            Ok(mt::Curve::NurbsCurve(c))
        };
        let face = side_curve(rows.first(), w_of(us), w_of(ue))?;
        let wall = side_curve(rows.last(), w_of(0.0), w_of(1.0))?;
        let across = mt::KnotVec::bezier_knot(rows.len() - 1);
        let bs = mt::BSplineSurface::try_new((across, vk), rows).map_err(|e| fail(&e))?;
        return Ok(CurveFit { surface: mt::Surface::NurbsSurface(mt::NurbsSurface::new(bs)), face, wall });
    }
    Err(unsupported("could not fit the blend along a free-form edge within tolerance"))
}

/// A curved-wall joint is generally not planar. Fit its exact offset slices rather than
/// projecting a circular section onto a tangent mitre and revolving that distorted section.
/// `(r, rw)`: the round's radius (`rw` unused), or a chamfer's setbacks on the face and up the wall.
fn offset_profile(prev: &Seg, cur: &Seg, n: Vec3, side: f64, (r, rw): (f64, f64), round: bool, tol: f64) -> Result<mt::Curve> {
    use mt::ParametricCurve;
    let point = |u: f64| {
        let (d, h) = if round {
            let (sin, cos) = (u * std::f64::consts::FRAC_PI_2).sin_cos();
            (r * (1.0 - sin), r * (1.0 - cos))
        } else {
            (r * (1.0 - u), rw * u)
        };
        offset_joint(prev, cur, n, d).map(|p| p + n * (side * h))
    };
    for count in [8, 16, 32, 64, 128] {
        let pts = (0..=count)
            .map(|i| point(f64::from(i) / f64::from(count)))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| too_large("the blend's offset walls do not meet"))?;
        let curve = crate::build::interpolate_cubic(&pts).ok_or_else(|| unsupported("curved corner profile"))?;
        if (0..count).all(|i| {
            let u = (f64::from(i) + 0.5) / f64::from(count);
            point(u).is_some_and(|p| p.dist(from_p3(curve.subs(u))) <= tol)
        }) {
            return Ok(mt::Curve::BSplineCurve(curve));
        }
    }
    Err(unsupported("could not fit the curved corner within tolerance"))
}

/// The exact shape of an oriented loop edge of the face with normal `n` (a line, or an arc found
/// from samples, else a free-form curve in the face plane).
fn seg_of(e: &mt::Edge, n: Vec3, tol: f64) -> Option<Seg> {
    use mt::{BoundedCurve, ParametricCurve};
    let (a, b) = (from_p3(e.front().point()), from_p3(e.back().point()));
    let c = e.oriented_curve();
    let (t0, t1) = c.range_tuple();
    let pts: Vec<Vec3> = (0..=16).map(|k| from_p3(c.subs(t0 + (t1 - t0) * k as f64 / 16.0))).collect();
    let closed = a.dist(b) < tol;
    if !closed && pts.iter().all(|p| p.dist_to_segment(a, b) < tol) {
        return Some(Seg::Line { a, b });
    }
    let m = *pts.get(8)?;
    if let Some(arc) = arc_of(a, b, m, &pts, closed, tol) {
        return Some(arc);
    }
    if closed || !pts.iter().all(|p| (*p - a).dot(n).abs() < tol) || matches!(c, mt::Curve::Line(_)) {
        return None;
    }
    Some(Seg::Curve { a, b, mid: m, span: Rc::new(Span { curve: c, t0, t1 }) })
}

/// The arc through an edge's samples `pts` (from `a` to `b`, `m` halfway), if it is one.
fn arc_of(a: Vec3, b: Vec3, m: Vec3, pts: &[Vec3], closed: bool, tol: f64) -> Option<Seg> {
    // A closed edge (a full circle): fit through points a third of the way round.
    let (b_fit, m_fit) = if closed { (*pts.get(5)?, *pts.get(11)?) } else { (b, m) };
    let (u, v) = (b_fit - a, m_fit - a);
    let w = u.cross(v);
    let d = 2.0 * w.len2();
    if d < 1e-300 {
        return None;
    }
    let centre = a + (v.cross(w) * u.len2() + w.cross(u) * v.len2()) * (1.0 / d);
    let r = centre.dist(a);
    if !pts.iter().all(|p| (p.dist(centre) - r).abs() < tol) {
        return None;
    }
    // Turning direction: the travel at a is axis × (a − c).
    let t_a = (*pts.get(1)? - a).normalized()?;
    let mut axis = w.normalized()?;
    if axis.cross(a - centre).dot(t_a) < 0.0 {
        axis = -axis;
    }
    let (ra, rb) = ((a - centre).normalized()?, (b - centre).normalized()?);
    let mut angle = ra.dot(rb).clamp(-1.0, 1.0).acos();
    if ra.cross(rb).dot(axis) < 0.0 {
        angle = std::f64::consts::TAU - angle;
    }
    if closed {
        angle = std::f64::consts::TAU;
    }
    Some(Seg::Arc { a, c: centre, axis, angle, mid: m })
}

/// Whether an edge is a straight segment: a line, or a curve whose points all lie on the chord
/// (the contact lines of a rolling-ball blend between parallel walls are such curves).
fn is_straight(e: &mt::Edge, tol: f64) -> bool {
    use mt::{BoundedCurve, ParametricCurve};
    if matches!(e.curve(), mt::Curve::Line(_)) {
        return true;
    }
    let (a, b) = (from_p3(e.absolute_front().point()), from_p3(e.absolute_back().point()));
    if a.dist(b) < tol {
        return false;
    }
    let c = e.curve();
    let (t0, t1) = c.range_tuple();
    (0..=16).all(|k| from_p3(c.subs(t0 + (t1 - t0) * k as f64 / 16.0)).dist_to_segment(a, b) < tol)
}

/// `p` turned about the line (c, axis) by `ang`.
fn turn(p: Vec3, c: Vec3, axis: Vec3, ang: f64) -> Vec3 {
    let k = axis.normalized().unwrap_or(Vec3::Z);
    let v = p - c;
    let (s, co) = ang.sin_cos();
    c + v * co + k.cross(v) * s + k * (k.dot(v) * (1.0 - co))
}

/// Split full circles in two halves (an edge needs two ends).
fn pieces(segs: &[Seg]) -> Vec<(usize, Seg)> {
    let mut out = Vec::new();
    for (i, s) in segs.iter().enumerate() {
        match *s {
            Seg::Arc { a, c, axis, angle, .. } if angle > std::f64::consts::TAU - 1e-9 => {
                let h = std::f64::consts::PI;
                let a2 = turn(a, c, axis, h);
                out.push((i, Seg::Arc { a, c, axis, angle: h, mid: turn(a, c, axis, h / 2.0) }));
                out.push((i, Seg::Arc { a: a2, c, axis, angle: h, mid: turn(a, c, axis, 1.5 * h) }));
            }
            _ => out.push((i, s.clone())),
        }
    }
    out
}

fn plane_normal(f: &mt::Face) -> Option<Vec3> {
    match f.oriented_surface() {
        mt::Surface::Plane(p) => {
            let n = p.normal();
            Vec3::new(n.x, n.y, n.z).normalized()
        }
        _ => None,
    }
}

/// Outward normal of a face at a point on it.
fn face_normal(f: &mt::Face, p: Vec3) -> Option<Vec3> {
    use mt::{ParametricSurface3D, SearchNearestParameter, SearchParameter};
    let s = f.oriented_surface();
    let (u, v) = s.search_parameter(p3(p), None, 100).or_else(|| s.search_nearest_parameter(p3(p), None, 100))?;
    let n = s.normal(u, v);
    Vec3::new(n.x, n.y, n.z).normalized()
}

/// Blend `edges` (by id) if they form a boundary loop or neighbouring run of a planar face with
/// perpendicular walls, with what it built from `solid` (each blend face in place of its loop
/// edge). `None` when the edges are not such a loop.
pub(crate) fn loop_blend(solid: &Solid, edge_ids: &[mt::EdgeID], size: f64, r: f64, round: bool) -> Option<Result<(Solid, Rebuild)>> {
    loop_blend_sized(solid, edge_ids, size, Sizing::Equal { r, round })
}

/// [`loop_blend`] as a chamfer of `d1` on its first face and the second side by `side`. The
/// first face is the one facing up most (the planar face or its wall, decided as for a straight
/// edge, see [`crate::chamfer_sides`]), the other one with `flip`.
pub(crate) fn loop_chamfer_sides(
    solid: &Solid,
    edge_ids: &[mt::EdgeID],
    size: f64,
    d1: f64,
    side: ChamferSide,
    flip: bool,
) -> Option<Result<(Solid, Rebuild)>> {
    loop_blend_sized(solid, edge_ids, size, Sizing::Sides { d1, side, flip })
}

/// How large a loop blend is.
#[derive(Clone, Copy, Debug)]
enum Sizing {
    /// A round of radius `r` (`round`), or an equal-distance chamfer of `r`.
    Equal { r: f64, round: bool },
    /// A chamfer of `d1` on its first face, the second side by `side` ([`loop_chamfer_sides`]).
    Sides { d1: f64, side: ChamferSide, flip: bool },
}

fn loop_blend_sized(solid: &Solid, edge_ids: &[mt::EdgeID], size: f64, sizing: Sizing) -> Option<Result<(Solid, Rebuild)>> {
    match crate::guard("loop blend", || Ok(loop_blend_inner(solid, edge_ids, size, sizing))) {
        Ok(result) => result,
        Err(error) => Some(Err(error)),
    }
}

fn loop_blend_inner(solid: &Solid, edge_ids: &[mt::EdgeID], size: f64, sizing: Sizing) -> Option<Result<(Solid, Rebuild)>> {
    #[cfg(test)]
    assert!(crate::guarded(), "loop selection must run inside the kernel guard");
    let tol = (size * 1e-6).max(1e-7);
    for (si, shell) in solid.boundaries().iter().enumerate() {
        let faces: Vec<&mt::Face> = shell.face_iter().collect();
        for (fi, f) in faces.iter().enumerate() {
            let Some(n) = plane_normal(f) else { continue };
            for (wi, w) in f.boundaries().iter().enumerate() {
                let ids: Vec<mt::EdgeID> = w.edge_iter().map(|e| e.id()).collect();
                if !edge_ids.iter().all(|e| ids.contains(e)) {
                    continue;
                }
                if ids.len() == edge_ids.len() {
                    return Some(build(solid, si, &faces, fi, wi, n, size, tol, sizing, None));
                }
                // A run of neighbouring edges of the loop (its ends meet unblended edges).
                let m = ids.len();
                let inside = |i: usize| ids.get(i % m).is_some_and(|x| edge_ids.contains(x));
                let starts: Vec<usize> = (0..m).filter(|i| inside(*i) && !inside(*i + m - 1)).collect();
                if let [s0] = starts[..]
                    && (0..edge_ids.len()).all(|j| inside(s0 + j))
                {
                    return Some(build(solid, si, &faces, fi, wi, n, size, tol, sizing, Some((s0, edge_ids.len()))));
                }
            }
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn build(
    solid: &Solid,
    si: usize,
    faces: &[&mt::Face],
    fi: usize,
    wi: usize,
    n: Vec3,
    size: f64,
    tol: f64,
    sizing: Sizing,
    run: Option<(usize, usize)>,
) -> Result<(Solid, Rebuild)> {
    #[cfg(test)]
    assert!(crate::guarded(), "loop construction must run inside the kernel guard");
    // `r`: the round's radius or the equal chamfer's size; an unequal chamfer's first distance
    // (its setbacks come from `frame`).
    let (r, round, sides) = match sizing {
        Sizing::Equal { r, round } => (r, round, None),
        Sizing::Sides { d1, side, flip } => (d1, false, Some((side, flip))),
    };
    let f = faces.get(fi).ok_or_else(|| KernelError::Failed("face".into()))?;
    let wire = f.boundaries().get(wi).cloned().ok_or_else(|| KernelError::Failed("loop".into()))?;
    let all_edges: Vec<mt::Edge> = wire.edge_iter().cloned().collect();
    // The blended edges in loop order: the whole loop, or a run of it (open).
    let open = run.is_some();
    let edges: Vec<mt::Edge> = match run {
        None => all_edges,
        Some((s0, len)) => (0..len).filter_map(|j| all_edges.get((s0 + j) % all_edges.len().max(1)).cloned()).collect(),
    };
    let edge_segs: Vec<Seg> = edges
        .iter()
        .map(|e| seg_of(e, n, (size * 1e-5).max(1e-6)))
        .collect::<Option<_>>()
        .ok_or_else(|| unsupported("loop edges must be lines or arcs"))?;
    // Pieces: the loop's edges, full circles in halves; each knows its edge.
    let split = pieces(&edge_segs);
    let owner: Vec<usize> = split.iter().map(|x| x.0).collect();
    let segs: Vec<Seg> = split.iter().map(|x| x.1.clone()).collect();
    let k = segs.len();
    // Joints: k round a whole loop; k + 1 along a run (its two ends included).
    let nj = if open { k + 1 } else { k };
    let nxt = |i: usize| if open { i + 1 } else { (i + 1) % k };
    // Joints: smooth, or sharp where the walls meet in one straight seam (the blends
    // then meet in a mitre). A loop can therefore mix lines and arcs at its corners.
    let mut mitre = vec![false; nj];
    for i in 0..k {
        if open && i == 0 {
            continue;
        }
        let (Some(prev), Some(cur)) = (segs.get((i + k - 1) % k), segs.get(i)) else { continue };
        let p = cur.start();
        let (Some(ta), Some(tb)) = (prev.tangent(p), cur.tangent(p)) else { return Err(unsupported("degenerate loop edge")) };
        if ta.dot(tb) < 1.0 - 1e-6 {
            if ta.dot(tb) < -1.0 + 1e-3 {
                return Err(unsupported("blending a loop with sharp corners next to arcs"));
            }
            if let Some(x) = mitre.get_mut(i) {
                *x = true;
            }
        }
    }
    // Walls: the other face of each edge, perpendicular to the face; all on one side.
    let mut walls: Vec<usize> = Vec::with_capacity(k);
    let mut side = 0.0;
    for (oi, s) in owner.iter().zip(&segs) {
        let e = edges.get(*oi).ok_or_else(|| KernelError::Failed("edge".into()))?;
        let wf = faces
            .iter()
            .position(|g| !std::ptr::eq(*g, *f) && g.edge_iter().any(|x| x.id() == e.id()))
            .ok_or_else(|| unsupported("an edge with one face"))?;
        let mid = s.mid();
        let g = faces.get(wf).ok_or_else(|| KernelError::Failed("wall".into()))?;
        let gn = face_normal(g, mid).ok_or_else(|| unsupported("wall normal"))?;
        if gn.dot(n).abs() > 1.0 - 1e-6 {
            return Err(unsupported("a wall tangent to the face"));
        }
        // Into the face from the loop: left of travel.
        let m = n.cross(s.tangent(mid).ok_or_else(|| unsupported("tangent"))?);
        let sd = if gn.dot(m) > 0.0 { 1.0 } else { -1.0 };
        if side != 0.0 && sd != side {
            return Err(unsupported("a loop that is partly convex and partly concave"));
        }
        side = sd;
        walls.push(wf);
    }
    // A run ends in the cross-section through its end vertex: the face there must hold it (a
    // plane square to the run). One at an angle cuts the blend's two contacts at different
    // places, which the rolling-ball blend handles.
    if open {
        let ends = [
            (edges.first().map(|e| e.front().clone()), segs.first().map(|s| (s.start(), s.clone()))),
            (edges.last().map(|e| e.back().clone()), segs.last().map(|s| (s.end(), s.clone()))),
        ];
        for (v, seg) in ends {
            let (Some(v), Some((p, s))) = (v, seg) else { return Err(KernelError::Failed("run end".into())) };
            let t = s.tangent(p).ok_or_else(|| unsupported("degenerate loop edge"))?;
            let square = faces
                .iter()
                .enumerate()
                .filter(|(gi, g)| *gi != fi && !walls.contains(gi) && g.vertex_iter().any(|x| x == v))
                .all(|(_, g)| plane_normal(g).is_some_and(|gn| gn.dot(t).abs() > 1.0 - 1e-6));
            if !square {
                return Err(unsupported("a run of edges ending at a face at an angle"));
            }
        }
    }
    // Cross-section at a loop point: into the face (m) and up the wall (t, away from the edge,
    // up a concave wall and down a convex one). The ball of radius r touches both at the
    // setback s = r / tan(φ/2), φ the angle between m and t; its centre is on the bisector.
    let into = |s: &Seg, p: Vec3| -> Vec3 { s.tangent(p).map(|t| n.cross(t)).unwrap_or(Vec3::ZERO) };
    let up = |i: usize, p: Vec3| -> Option<Vec3> {
        let s = segs.get(i)?;
        let g = faces.get(*walls.get(i)?)?;
        let gn = face_normal(g, p)?;
        let t = s.tangent(p)?.cross(gn).normalized()?;
        Some(if t.dot(n * side) < 0.0 { -t } else { t })
    };
    // An unequal chamfer: whether its first side is the planar face (else the walls). As for a
    // straight edge, that is the one facing up most (a curved wall by its mean normal along each
    // edge), the other with `flip`; it must be the same all round the loop.
    let face_first = match sides {
        None => true,
        Some((_, flip)) => {
            use mt::{BoundedCurve, ParametricCurve};
            let up = Vec3::new(1e-6, 1e-3, 1.0);
            let face_up = n.dot(up);
            let mut first: Option<bool> = None;
            for (pi, oi) in owner.iter().enumerate() {
                if pi > 0 && owner.get(pi - 1) == Some(oi) {
                    continue;
                }
                let (Some(e), Some(g)) = (edges.get(*oi), walls.get(pi).and_then(|w| faces.get(*w))) else {
                    return Err(KernelError::Failed("loop edge".into()));
                };
                let c = e.curve();
                let (t0, t1) = c.range_tuple();
                let ups: Vec<f64> =
                    (0..=8).filter_map(|j| face_normal(g, from_p3(c.subs(t0 + (t1 - t0) * f64::from(j) / 8.0))).map(|gn| gn.dot(up))).collect();
                if ups.is_empty() {
                    return Err(unsupported("wall normal"));
                }
                let wall_up = ups.iter().sum::<f64>() / ups.len() as f64;
                let mine = (face_up >= wall_up) != flip;
                if first.is_some_and(|x| x != mine) {
                    return Err(unsupported(
                        "an unequal chamfer round a loop whose first face changes along it (the face faces up more than some walls and less than others)",
                    ));
                }
                first = Some(mine);
            }
            first.unwrap_or(true)
        }
    };
    // Into the face (m), up the wall (t), the setbacks along each and the distance from the edge
    // to a round's centre.
    let frame = |i: usize, p: Vec3| -> Option<(Vec3, Vec3, f64, f64, f64)> {
        let s = segs.get(i)?;
        let (m, t) = (into(s, p), up(i, p)?);
        let phi = m.dot(t).clamp(-1.0, 1.0).acos();
        if !(phi > 1e-3 && phi < std::f64::consts::PI - 1e-3) {
            return None;
        }
        let (sf, sw) = match sides {
            None => {
                let sb = if round { r / (phi / 2.0).tan() } else { r };
                (sb, sb)
            }
            Some((cs, _)) => {
                let d2 = match cs {
                    ChamferSide::Distance(d) => d,
                    // The triangle edge–A–B: angle φ at the edge, `a` at A (on the first side).
                    ChamferSide::Angle(a) => {
                        let den = (phi + a).sin();
                        if den <= 1e-9 {
                            return None;
                        }
                        r * a.sin() / den
                    }
                };
                if face_first { (r, d2) } else { (d2, r) }
            }
        };
        Some((m, t, sf, sw, r / (phi / 2.0).sin()))
    };
    if let Some((ChamferSide::Angle(a), _)) = sides {
        for (i, s) in segs.iter().enumerate() {
            let p = s.start();
            if let Some(t) = up(i, p)
                && (into(s, p).dot(t).clamp(-1.0, 1.0).acos() + a).sin() <= 1e-9
            {
                return Err(KernelError::Invalid("that chamfer angle runs past the other face".into()));
            }
        }
    }
    // The straight seam between the walls at a sharp joint (direction away from the face).
    let seam_dir = |i: usize, p: Vec3| -> Option<Vec3> {
        let o = owner.get(i)?;
        let v = edges.get(*o)?.front().clone();
        let loop_ids: Vec<mt::EdgeID> = edges.iter().map(|e| e.id()).collect();
        let mut dirs = Vec::new();
        for g in faces {
            for e in g.edge_iter() {
                if loop_ids.contains(&e.id()) || !(*e.front() == v || *e.back() == v) {
                    continue;
                }
                let other = if *e.front() == v { e.back().clone() } else { e.front().clone() };
                if let Some(d) = (from_p3(other.point()) - p).normalized()
                    && !dirs.iter().any(|x: &Vec3| x.dot(d) > 1.0 - 1e-9)
                {
                    dirs.push(d);
                }
            }
        }
        match dirs[..] {
            [d] => Some(d),
            _ => None,
        }
    };
    let mut pa = Vec::with_capacity(nj);
    let mut pb = Vec::with_capacity(nj);
    let mut centres = Vec::with_capacity(nj);
    let mut curved_joint = vec![false; nj];
    // Where each free-form piece's face rail starts and ends along it (off its ends at a sharp corner).
    let mut face_u = vec![(0.0, 1.0); k];
    // Joint i: the start of piece i (the end of the last piece for a run's far end).
    let joint_point = |i: usize| if i < k { segs.get(i).map(|s| s.start()) } else { segs.last().map(|s| s.end()) };
    for i in 0..nj {
        let p = joint_point(i).unwrap_or_default();
        let (m, t, sb, sbw, dc) = frame(i.min(k - 1), p).ok_or_else(|| unsupported("the face and wall meet at an angle the blend can't take"))?;
        centres.push(p + (m + t).normalized().unwrap_or(m) * dc);
        if !mitre.get(i).copied().unwrap_or(false) {
            pa.push(p + m * sb);
            pb.push(p + t * sbw);
            continue;
        }
        // Sharp joint: the face rails meet where the two offset lines cross; the wall rails
        // meet on the seam.
        let ip = (i + k - 1) % k;
        let (m0, t0, sb0, sbw0, _) = frame(ip, p).ok_or_else(|| unsupported("the face and wall meet at an angle the blend can't take"))?;
        let (Some(d0), Some(d1)) = (segs.get(ip).and_then(|x| x.tangent(p)), segs.get(i).and_then(|x| x.tangent(p))) else {
            return Err(unsupported("degenerate loop edge"));
        };
        let x = d0.cross(d1);
        let lam = ((m * sb - m0 * sb0).cross(d1)).dot(x) / x.len2();
        let curved = |x: Option<&Seg>| matches!(x, Some(Seg::Arc { .. } | Seg::Curve { .. }));
        let free = segs.get(ip).is_some_and(Seg::free) || segs.get(i).is_some_and(Seg::free);
        let exact = (curved(segs.get(ip)) || curved(segs.get(i))) && t.dot(n * side) > 1.0 - 1e-9 && t0.dot(n * side) > 1.0 - 1e-9;
        if exact {
            let (Some(prev), Some(cur)) = (segs.get(ip), segs.get(i)) else { return Err(unsupported("corner segments")) };
            let d = if sides.is_some() { sb } else { r };
            if free {
                let (q, u0, u1) = offset_joint_free(prev, cur, n, d).ok_or_else(|| too_large("the blend's offset walls do not meet"))?;
                if let Some(x) = face_u.get_mut(ip) {
                    x.1 = u0;
                }
                if let Some(x) = face_u.get_mut(i) {
                    x.0 = u1;
                }
                pa.push(q);
            } else {
                pa.push(offset_joint(prev, cur, n, d).ok_or_else(|| too_large("the blend's offset walls do not meet"))?);
            }
            if let Some(slot) = curved_joint.get_mut(i) {
                *slot = true;
            }
        } else if free {
            return Err(unsupported("a sharp corner between a free-form edge and a sloping wall"));
        } else {
            pa.push(p + m0 * sb0 + d0 * lam);
        }
        let e = seam_dir(i, p).ok_or_else(|| unsupported("a sharp corner without one straight seam"))?;
        let (h0, h1) = (e.dot(t0), e.dot(t));
        if h0 < 1e-6 || h1 < 1e-6 || (sbw0 / h0 - sbw / h1).abs() > tol * 10.0 {
            return Err(unsupported("a sharp corner whose walls meet the blend differently"));
        }
        pb.push(p + e * (sbw / h1));
    }
    // Sharp joints between straight rim edges can extend beyond the old corner.
    // Equal wall slopes reach this path instead of the corner builder, and must
    // also reject an extension through the outer wall or another hole.
    for (i, q) in pa.iter().enumerate() {
        // Smooth arc transitions and open-run ends retain their existing checks.
        if !mitre.get(i).copied().unwrap_or(false) {
            continue;
        }
        let p = joint_point(i).ok_or_else(|| unsupported("degenerate loop joint"))?;
        if !crate::blend::extension_fits(f, p, *q, size * 1e-7) {
            // A narrow corner sends the mitre far into the face: say which corner.
            return Err(too_large(&format!(
                "the blend is larger than the neighbouring faces at the sharp corner near ({:.2}, {:.2}, {:.2})",
                p.x, p.y, p.z
            )));
        }
    }
    // The offset arcs must not turn inside out; an arc of exactly the blend's radius shrinks
    // to a point (a rounded corner rounded again: the blend there is a sphere).
    let mut collapsed = vec![false; k.max(nj)];
    for (i, s) in segs.iter().enumerate() {
        if let Seg::Arc { a, c, .. } = *s {
            let toward = into(s, a).dot(c - a) > 0.0;
            // An unequal chamfer's offset on the face; it never ends in a point (a cone's apex).
            if sides.is_some() {
                let reach = frame(i, a).map(|x| x.2).ok_or_else(|| unsupported("the face and wall meet at an angle the blend can't take"))?;
                if toward && a.dist(c) <= reach + tol * 10.0 {
                    return Err(too_large("the chamfer is larger than a corner radius"));
                }
                continue;
            }
            if toward && (a.dist(c) - r).abs() <= tol * 10.0 && !mitre.get(i).copied().unwrap_or(true) && !mitre.get(nxt(i)).copied().unwrap_or(true)
            {
                if let Some(x) = collapsed.get_mut(i) {
                    *x = true;
                }
            } else if toward && a.dist(c) <= r + tol {
                return Err(too_large("the blend is larger than a corner radius"));
            }
        }
    }
    if collapsed.iter().all(|x| *x) {
        return Err(too_large("the blend is larger than a corner radius"));
    }
    // Free-form pieces: their blend surfaces and rails.
    let mut curve_fits: Vec<Option<CurveFit>> = Vec::with_capacity(k);
    for (i, s) in segs.iter().enumerate() {
        if !s.free() {
            curve_fits.push(None);
            continue;
        }
        let (_, t, sf, sw, _) = frame(i, s.start()).ok_or_else(|| unsupported("the face and wall meet at an angle the blend can't take"))?;
        if t.dot(n * side) < 1.0 - 1e-9 {
            return Err(unsupported("a free-form loop edge whose wall is not square to the face"));
        }
        let wall = walls.get(i).and_then(|w| faces.get(*w)).ok_or_else(|| KernelError::Failed("wall".into()))?;
        let fit = fit_free(s, wall, n, t, (sf, sw), round, face_u.get(i).copied().unwrap_or((0.0, 1.0)), tol)?;
        curve_fits.push(Some(fit));
    }
    let mut va: Vec<mt::Vertex> = pa.iter().map(|p| builder::vertex(p3(*p))).collect();
    // A collapsed piece starts and ends at one vertex.
    for i in 0..k {
        if collapsed.get(i).copied().unwrap_or(false)
            && let Some(v) = va.get(i).cloned()
            && let Some(slot) = va.get_mut(nxt(i))
        {
            *slot = v;
        }
    }
    // Model vertices at the joints (the second half of a circle starts at none).
    let mut joint_vertex: Vec<Option<mt::Vertex>> = (0..k)
        .map(|i| if i == 0 || owner.get(i) != owner.get(i - 1) { owner.get(i).and_then(|o| edges.get(*o)).map(|e| e.front().clone()) } else { None })
        .collect();
    if open {
        joint_vertex.push(edges.last().map(|e| e.back().clone()));
    }
    let loop_ids: Vec<mt::EdgeID> = edges.iter().map(|e| e.id()).collect();
    let on_face = |e: &mt::Edge| f.edge_iter().any(|x| x.id() == e.id());
    // A blend as tall as its walls uses them up: each wall seam ends exactly where the
    // wall-side rail would, at a vertex of the walls' far boundary, which becomes the rail.
    let wall_end: Vec<Option<mt::Vertex>> = joint_vertex
        .iter()
        .zip(&pb)
        .map(|(v, target)| {
            let v = v.as_ref()?;
            faces.iter().flat_map(|g| g.edge_iter()).find_map(|e| {
                if loop_ids.contains(&e.id()) || on_face(&e) || !(e.front() == v || e.back() == v) || !matches!(e.curve(), mt::Curve::Line(_)) {
                    return None;
                }
                let other = if e.front() == v { e.back().clone() } else { e.front().clone() };
                (from_p3(other.point()).dist(*target) <= tol * 10.0).then_some(other)
            })
        })
        .collect();
    let used_up = wall_end.iter().filter(|x| x.is_some()).count();
    if used_up > 0 && (used_up != nj || open) {
        return Err(unsupported("the blend uses up only part of the walls"));
    }
    let walls_used_up = used_up > 0;
    let vb: Vec<mt::Vertex> = match walls_used_up {
        true => wall_end.into_iter().flatten().collect(),
        false => pb.iter().map(|p| builder::vertex(p3(*p))).collect(),
    };
    let centre = |i: usize| centres.get(i).copied().unwrap_or_default();
    let profile_mid = |i: usize| {
        let (Some(p), c) = (joint_point(i), centre(i)) else { return Vec3::ZERO };
        c + (p - c).normalized().unwrap_or(n) * r
    };
    // Profiles at the joints (face side → wall side).
    let mut profiles: Vec<mt::Edge> = Vec::with_capacity(nj);
    for i in 0..nj {
        let (Some(a), Some(b)) = (va.get(i), vb.get(i)) else { return Err(KernelError::Failed("joint".into())) };
        if curved_joint.get(i).copied().unwrap_or(false) {
            let (Some(prev), Some(cur)) = (segs.get((i + k - 1) % k), segs.get(i)) else { return Err(unsupported("corner segments")) };
            let setbacks = match sides {
                None => (r, r),
                Some(_) => joint_point(i).and_then(|p| frame(i.min(k - 1), p)).map(|x| (x.2, x.3)).ok_or_else(|| unsupported("frame"))?,
            };
            let curve = offset_profile(prev, cur, n, side, setbacks, round, (setbacks.0.min(setbacks.1) * 1e-7).max(1e-9))?;
            profiles.push(mt::Edge::try_new(a, b, curve).map_err(|e| KernelError::Failed(format!("curved corner: {e}")))?);
            continue;
        }
        if !round {
            profiles.push(builder::line(a, b));
            continue;
        }
        if !mitre.get(i).copied().unwrap_or(false) {
            profiles.push(builder::circle_arc(a, b, p3(profile_mid(i))));
            continue;
        }
        // Mitre: the cross-section circle of this edge's blend where it passes the face-side
        // point, slid along the edge onto the mitre plane (an ellipse arc, on both blends).
        let p = segs.get(i).map(|x| x.start()).unwrap_or_default();
        let d1 = segs.get(i).and_then(|x| x.tangent(p)).ok_or_else(|| unsupported("tangent"))?;
        let (pa_i, pb_i) = (from_p3(a.point()), from_p3(b.point()));
        let lam = (pa_i - p).dot(d1);
        let (_, t, sb, _, _) = frame(i, p).ok_or_else(|| unsupported("frame"))?;
        let cc = centre(i) + d1 * lam;
        let q = p + t * sb + d1 * lam;
        let mid = cc + ((pa_i - cc) + (q - cc)).normalized().unwrap_or(t) * r;
        // q projects onto the wall rail at pb_i; fitting through pb_i before projection
        // would distort the circular cross-section at mixed line/arc corners.
        let arc = builder::circle_arc(&builder::vertex(p3(pa_i)), &builder::vertex(p3(q)), p3(mid));
        let nb = (pa_i - p).cross(pb_i - p).normalized().ok_or_else(|| unsupported("mitre plane"))?;
        let den = d1.dot(nb);
        if den.abs() < 1e-9 {
            return Err(unsupported("mitre plane along the edge"));
        }
        // x ↦ x − d1 ((x − p)·nb) / (d1·nb), column-major.
        let col = |j: usize| {
            let ej = [Vec3::X, Vec3::Y, Vec3::Z][j];
            ej - d1 * (ej.dot(nb) / den)
        };
        let (c0, c1, c2) = (col(0), col(1), col(2));
        let tr = d1 * (p.dot(nb) / den);
        let mat = mt::Matrix4::new(c0.x, c0.y, c0.z, 0.0, c1.x, c1.y, c1.z, 0.0, c2.x, c2.y, c2.z, 0.0, tr.x, tr.y, tr.z, 1.0);
        let mut curve = arc.curve();
        mt::Transformed::transform_by(&mut curve, mat);
        profiles.push(mt::Edge::new(a, b, curve));
    }
    let rail = |vs: &[mt::Vertex], i: usize, offset: &dyn Fn(Vec3, &Seg) -> Vec3| -> Option<mt::Edge> {
        let s = segs.get(i)?;
        if std::ptr::eq(vs, va.as_slice()) && collapsed.get(i).copied().unwrap_or(false) {
            return None;
        }
        let (a, b) = (vs.get(i)?, vs.get(nxt(i))?);
        Some(match s {
            Seg::Line { .. } => builder::line(a, b),
            Seg::Arc { mid, .. } => builder::circle_arc(a, b, p3(offset(*mid, s))),
            Seg::Curve { .. } => {
                let fit = curve_fits.get(i)?.as_ref()?;
                let c = if std::ptr::eq(vs, va.as_slice()) { &fit.face } else { &fit.wall };
                mt::Edge::try_new(a, b, c.clone()).ok()?
            }
        })
    };
    let piece_of = |s: &Seg| segs.iter().position(|x| std::ptr::eq(x, s)).unwrap_or(0);
    let face_off = |p: Vec3, s: &Seg| {
        let i = piece_of(s);
        frame(i, p).map(|(m, _, sb, _, _)| p + m * sb).unwrap_or(p)
    };
    let wall_off = |p: Vec3, s: &Seg| {
        let i = piece_of(s);
        frame(i, p).map(|(_, t, _, sbw, _)| p + t * sbw).unwrap_or(p)
    };
    let face_rails: Vec<Option<mt::Edge>> = (0..k).map(|i| rail(&va, i, &face_off)).collect();
    if face_rails.iter().zip(&collapsed).any(|(r, c)| r.is_none() && !c) {
        return Err(KernelError::Failed("rail".into()));
    }
    let wall_rails: Vec<mt::Edge> = if walls_used_up {
        // Each piece's wall rail is the wall's far edge between the joints, where the offset
        // rail lies (checked at the piece's middle).
        (0..k)
            .map(|i| -> Result<mt::Edge> {
                let (Some(a), Some(b), Some(s), Some(wf)) = (vb.get(i), vb.get(nxt(i)), segs.get(i), walls.get(i)) else {
                    return Err(KernelError::Failed("rail".into()));
                };
                let mid = s.mid();
                let want = wall_off(mid, s);
                let g = faces.get(*wf).ok_or_else(|| KernelError::Failed("wall".into()))?;
                g.edge_iter()
                    .find(|e| (e.front() == a && e.back() == b) || (e.front() == b && e.back() == a))
                    .filter(|e| crate::blend::edge_mid(e).is_some_and(|m| m.dist(want) <= tol * 10.0))
                    .map(|e| if e.front() == a { e.clone() } else { e.inverse() })
                    .ok_or_else(|| unsupported("the blend uses up walls whose far edge is not where the blend ends"))
            })
            .collect::<Result<_>>()?
    } else {
        (0..k).map(|i| rail(&vb, i, &wall_off)).collect::<Option<_>>().ok_or_else(|| KernelError::Failed("rail".into()))?
    };
    // Seams between neighbouring walls at the joints get shorter.
    // At a run's ends the face's next loop edges stop at the face-side point.
    let mut seam_subst: HashMap<mt::EdgeID, mt::Edge> = HashMap::new();
    let mut seam_ends: HashMap<mt::EdgeID, (mt::Edge, Option<mt::Vertex>, Option<mt::Vertex>)> = HashMap::new();
    for (i, v) in joint_vertex.iter().enumerate() {
        let Some(v) = v else { continue };
        for g in faces {
            for e in g.edge_iter() {
                if loop_ids.contains(&e.id()) || !(e.front() == v || e.back() == v) {
                    continue;
                }
                // Once per edge and joint (an edge between both ends of a run is met twice).
                if seam_ends.get(&e.id()).is_some_and(|x: &(mt::Edge, Option<mt::Vertex>, Option<mt::Vertex>)| {
                    (x.0.absolute_front() == v && x.1.is_some()) || (x.0.absolute_back() == v && x.2.is_some())
                }) {
                    continue;
                }
                let (fa, fb) = (e.absolute_front().clone(), e.absolute_back().clone());
                let other = if &fa == v { &fb } else { &fa };
                if !is_straight(&e, (size * 1e-5).max(1e-6)) {
                    return Err(unsupported("the wall seams must be straight"));
                }
                let face_side = on_face(&e);
                // A used-up wall goes, seams and all.
                if walls_used_up && !face_side {
                    continue;
                }
                let target = if face_side { pa.get(i) } else { pb.get(i) }.copied().unwrap_or_default();
                let along = (from_p3(other.point()) - from_p3(v.point())).dot(target - from_p3(v.point()));
                let reach = (target - from_p3(v.point())).len2();
                if along <= reach + tol {
                    return Err(too_large(if face_side { "the blend is longer than the next edge" } else { "the blend is taller than the walls" }));
                }
                let nb = if face_side { va.get(i) } else { vb.get(i) }.ok_or_else(|| KernelError::Failed("joint".into()))?;
                let slot = seam_ends.entry(e.id()).or_insert_with(|| (e.absolute_clone(), None, None));
                if &fa == v {
                    slot.1 = Some(nb.clone());
                } else {
                    slot.2 = Some(nb.clone());
                }
            }
        }
    }
    for (id, (e, a, b)) in seam_ends {
        let a = a.unwrap_or_else(|| e.absolute_front().clone());
        let b = b.unwrap_or_else(|| e.absolute_back().clone());
        if from_p3(a.point()).dist(from_p3(b.point())) < tol * 10.0 {
            return Err(too_large("the blend is longer than the next edge"));
        }
        seam_subst.insert(id, builder::line(&a, &b));
    }
    // Rebuild faces.
    let idx_of = |id: mt::EdgeID| loop_ids.iter().position(|x| *x == id);
    let rebuild = |g: &mt::Face, rails: &[Option<mt::Edge>]| -> Result<mt::Face> {
        let mut wires = Vec::new();
        for w in g.absolute_boundaries() {
            let mut es = Vec::new();
            for e in w.edge_iter() {
                if let Some(i) = idx_of(e.id()) {
                    // Rails run in loop order; an edge used the other way takes them reversed.
                    let mine: Vec<&mt::Edge> = owner.iter().zip(rails).filter(|(o, _)| **o == i).filter_map(|(_, r)| r.as_ref()).collect();
                    let same = edges.get(i).is_some_and(|le| le.orientation() == e.orientation());
                    if same {
                        es.extend(mine.into_iter().cloned());
                    } else {
                        es.extend(mine.into_iter().rev().map(|r| r.inverse()));
                    }
                    continue;
                }
                let ne = if let Some(s) = seam_subst.get(&e.id()) {
                    if e.front() == e.absolute_front() { s.clone() } else { s.inverse() }
                } else {
                    e.clone()
                };
                es.push(ne);
            }
            // A run's end: the face's edge and the wall's seam now stop at the two ends of the
            // end profile, which closes the gap between them.
            if open {
                let mut fixed: Vec<mt::Edge> = Vec::with_capacity(es.len() + 2);
                let m = es.len();
                for j in 0..m {
                    let (Some(cur), Some(next)) = (es.get(j), es.get((j + 1) % m)) else { continue };
                    fixed.push(cur.clone());
                    if cur.back() == next.front() {
                        continue;
                    }
                    for ji in [0, nj - 1] {
                        let (Some(a), Some(b), Some(pr)) = (va.get(ji), vb.get(ji), profiles.get(ji)) else { continue };
                        if cur.back() == a && next.front() == b {
                            fixed.push(pr.clone());
                        } else if cur.back() == b && next.front() == a {
                            fixed.push(pr.inverse());
                        }
                    }
                }
                es = fixed;
            }
            wires.push(mt::Wire::from(es));
        }
        crate::heal::absolute_face(g, wires).ok_or_else(|| KernelError::Failed("loop blend: a face could not be rebuilt".into()))
    };
    let mut out: Vec<mt::Face> = Vec::new();
    let mut rec = Rebuild::default();
    rec.pieces(&seam_subst);
    for (gi, g) in faces.iter().enumerate() {
        let rebuilt = if gi == fi {
            rebuild(g, &face_rails)?
        } else if walls.contains(&gi) {
            if walls_used_up {
                continue;
            }
            let wr: Vec<Option<mt::Edge>> = wall_rails.iter().cloned().map(Some).collect();
            rebuild(g, &wr)?
        } else if g.edge_iter().any(|e| seam_subst.contains_key(&e.id())) {
            rebuild(g, &[])?
        } else {
            (*g).clone()
        };
        rec.face(&rebuilt, g);
        out.push(rebuilt);
    }
    // Blend faces.
    use mt::{ParametricSurface3D, SearchNearestParameter};
    for i in 0..k {
        let (Some(s), Some(pr), Some(pr_next), Some(fr), Some(wr)) =
            (segs.get(i), profiles.get(i), profiles.get(nxt(i)), face_rails.get(i), wall_rails.get(i))
        else {
            continue;
        };
        let swept: mt::Shell = match *s {
            Seg::Curve { .. } => Default::default(),
            Seg::Line { a, b } => {
                // The cross-section at the start, swept along the whole edge and beyond (the
                // face may reach past the edge's ends at a mitre).
                let d = (b - a).normalized().ok_or_else(|| unsupported("degenerate loop edge"))?;
                let (_, t, sb, sbw, _) = frame(i, a).ok_or_else(|| unsupported("frame"))?;
                let m = into(s, a);
                let ext = size;
                let back = d * -ext;
                let (fa, wa) = (builder::vertex(p3(a + m * sb + back)), builder::vertex(p3(a + t * sbw + back)));
                let base = if round { builder::circle_arc(&fa, &wa, p3(profile_mid(i) + back)) } else { builder::line(&fa, &wa) };
                vec![builder::tsweep(&base, v3(b - a + d * (2.0 * ext)))].into()
            }
            Seg::Arc { .. } if collapsed.get(i).copied().unwrap_or(false) => {
                // A sphere about the profile's centre, its poles well away from the corner
                // patch (a pole inside a face's corner confuses the meshing).
                let c = centre(i);
                let corners = [from_p3(pr.front().point()), from_p3(pr.back().point()), from_p3(pr_next.back().point())];
                let g = corners.iter().fold(Vec3::ZERO, |acc, q| acc + (*q - c).normalized().unwrap_or(Vec3::ZERO));
                let g = g.normalized().ok_or_else(|| unsupported("sphere corner"))?;
                let w = g.any_perp();
                let (top, eq, bottom) = (builder::vertex(p3(c + w * r)), p3(c + g * r), builder::vertex(p3(c - w * r)));
                let meridian = builder::circle_arc(&top, &bottom, eq);
                builder::rsweep(&meridian, p3(c), v3(w), mt::Rad(std::f64::consts::TAU))
            }
            Seg::Arc { c, axis, angle, .. }
                if !curved_joint.get(i).copied().unwrap_or(false) && !curved_joint.get(nxt(i)).copied().unwrap_or(false) =>
            {
                builder::rsweep(pr, p3(c), v3(axis), mt::Rad(angle))
            }
            Seg::Arc { a, c, axis, angle, .. } => {
                // Sweep a radial section, not the shared corner curve: the latter can be
                // nonplanar and would change the round's radius all along this edge.
                let (_, t, sb, sbw, _) = frame(i, a).ok_or_else(|| unsupported("frame"))?;
                let (fa, wa) = (builder::vertex(p3(a + into(s, a) * sb)), builder::vertex(p3(a + t * sbw)));
                let base = if round { builder::circle_arc(&fa, &wa, p3(profile_mid(i))) } else { builder::line(&fa, &wa) };
                builder::rsweep(&base, p3(c), v3(axis), mt::Rad(angle))
            }
        };
        let fitted = curve_fits.get(i).and_then(|x| x.as_ref()).map(|x| x.surface.clone());
        let mut surf =
            fitted.or_else(|| swept.face_iter().next().map(|x| x.oriented_surface())).ok_or_else(|| KernelError::Failed("blend surface".into()))?;
        // Outward: toward the profile centre on a concave loop, away from it on a convex one.
        // An unequal chamfer's profile has no centre: its middle, away from the edge on a concave
        // loop (where the chamfer fills) and toward it on a convex one.
        let (c, mid) = (centre(i), profile_mid(i));
        let (mid, outward) = match (sides, joint_point(i)) {
            (Some(_), Some(p)) => {
                let m = (from_p3(pr.front().point()) + from_p3(pr.back().point())) * 0.5;
                (m, (m - p) * side)
            }
            _ => (mid, (c - mid) * side),
        };
        if let Some((u, v)) = surf.search_nearest_parameter(p3(mid), None, 100) {
            let nn = surf.normal(u, v);
            if Vec3::new(nn.x, nn.y, nn.z).dot(outward) < 0.0 {
                surf = mt::Invertible::inverse(&surf);
            }
        }
        let w: mt::Wire = match fr {
            Some(fr) => vec![fr.inverse(), pr.clone(), wr.clone(), pr_next.inverse()].into(),
            None => vec![pr.clone(), wr.clone(), pr_next.inverse()].into(),
        };
        let face = mt::Face::try_new(vec![w.clone()], surf.clone()).map_err(|e| KernelError::Failed(format!("blend face: {e}")))?;
        // In place of the loop edge this piece runs along (a full circle's two halves both).
        rec.blend(&face, owner.get(i).and_then(|o| edges.get(*o)).map(|e| e.id()));
        out.push(face);
    }
    let mut shells = solid.boundaries().clone();
    if let Some(sh) = shells.get_mut(si) {
        *sh = out.into();
    }
    let solid = Solid::try_new(shells).map_err(|e| KernelError::Failed(format!("loop blend produced an invalid solid: {e}")))?;
    Ok((solid, rec))
}
