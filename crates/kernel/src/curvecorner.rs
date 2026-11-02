//! Corners where rolling-ball blends meet on faces of any surface (the arc wall of an extruded
//! profile and the edges round it): what [`crate::blend_corner`] does for straight edges between
//! planes, with each blend fitted as in the parent module.
//!
//! Each blended edge's strip (its rolling-ball sections, fitted) runs a little past both ends.
//! Every end vertex has three faces and three edges. There:
//! - one blended edge: the strip ends where the third face cuts it, between where its contacts
//!   cross the other two edges;
//! - two: the strips cut each other along a crease, from where their contacts cross on the face
//!   they share to the third edge. When one strip's contact
//!   reaches further along the third edge, the other's ends on the crease alone and the first
//!   runs on beyond it to end on the other's third face;
//! - three (one radius): each strip ends at its section through the ball touching all three
//!   faces, and a piece of that ball's sphere fills the corner.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};

use solidcraft_geom::Vec3;
use truck_modeling::{self as mt, builder};

use super::{
    Guide, MAX_SAMPLES, SAMPLES, Section, fit_sections, in_proportion, knife_convex, knife_start, nearest_on, project, section_fits, solve_from,
    unsupported,
};
use crate::blend::{too_large, vtx};
use crate::body::{Solid, from_p3, p3, v3};
use crate::bridge::Frame;
use crate::record::Rebuild;
use crate::{KernelError, Result};

fn fail(m: &str) -> KernelError {
    KernelError::Failed(format!("blend corner: {m}"))
}

/// One blended edge's strip: the fitted rolling-ball surface (u across, from the contact with
/// side `fa` at 0 to side `fb`'s at 1; v along the edge from its front, running on past both
/// ends), its contacts (in v) and its sections.
struct Strip {
    edge: mt::Edge,
    fa: usize,
    fb: usize,
    surface: mt::Surface,
    contacts: [mt::Curve; 2],
    /// v of the sections at the edge's front and back vertex.
    ends: [f64; 2],
    /// +1 when the surface's normal points out of the material, −1 when into it.
    flip: f64,
    /// −1 for a convex edge (the ball inside the material), +1 for a concave one.
    sigma: f64,
    /// Sections with their v.
    sections: Vec<(f64, Section)>,
}

impl Strip {
    fn row(&self, f: usize) -> Result<usize> {
        if f == self.fa {
            Ok(0)
        } else if f == self.fb {
            Ok(1)
        } else {
            Err(fail("a face beside a blend"))
        }
    }

    fn contact(&self, f: usize) -> Result<&mt::Curve> {
        self.contacts.get(self.row(f)?).ok_or_else(|| fail("contact"))
    }

    /// u of the contact with face `f`.
    fn u(&self, f: usize) -> Result<f64> {
        self.row(f).map(|i| i as f64)
    }

    fn at(&self, u: f64, v: f64) -> Vec3 {
        use mt::ParametricSurface;
        from_p3(self.surface.subs(u, v))
    }

    /// The edge's own stretch of v.
    fn own(&self) -> (f64, f64) {
        let [a, b] = self.ends;
        (a.min(b), a.max(b))
    }

    /// v of the edge's end at `v`, and +1 when the strip runs on past it toward larger v.
    fn end_at(&self, v: &mt::Vertex) -> (f64, f64) {
        let [a, b] = self.ends;
        if self.edge.front() == v { (a, -1.0) } else { (b, 1.0) }
    }

    /// The strip's nearest point to `x` (u and v within it): from the section whose ball is
    /// nearest, by Gauss-Newton steps kept within the strip.
    fn nearest(&self, x: Vec3) -> Option<(f64, f64)> {
        use mt::ParametricSurface;
        let (v0, s0) = self.sections.iter().min_by(|a, b| a.1.c.dist(x).total_cmp(&b.1.c.dist(x)))?;
        let (a, b, w) = (s0.q1 - s0.c, s0.q2 - s0.c, x - s0.c);
        let angle = |p: Vec3, q: Vec3| (p.dot(q) / (p.len() * q.len()).max(1e-300)).clamp(-1.0, 1.0).acos();
        let total = angle(a, b);
        let mut uv = ((angle(a, w) / total.max(1e-12)).clamp(0.0, 1.0), *v0);
        let dist = |uv: (f64, f64)| from_p3(self.surface.subs(uv.0, uv.1)).dist(x);
        let mut d = dist(uv);
        for _ in 0..50 {
            let p = from_p3(self.surface.subs(uv.0, uv.1));
            let (su, sv) = (self.surface.uder(uv.0, uv.1), self.surface.vder(uv.0, uv.1));
            let (su, sv) = (Vec3::new(su.x, su.y, su.z), Vec3::new(sv.x, sv.y, sv.z));
            let r = x - p;
            let (a11, a12, a22) = (su.dot(su), su.dot(sv), sv.dot(sv));
            let (b1, b2) = (su.dot(r), sv.dot(r));
            let det = a11 * a22 - a12 * a12;
            if !(det.abs() > 1e-300) {
                break;
            }
            let (mut du, mut dv) = ((b1 * a22 - b2 * a12) / det, (a11 * b2 - a12 * b1) / det);
            let mut moved = false;
            for _ in 0..30 {
                let next = ((uv.0 + du).clamp(0.0, 1.0), (uv.1 + dv).clamp(0.0, 1.0));
                let dn = dist(next);
                if dn <= d {
                    moved = (next.0 - uv.0).abs() + (next.1 - uv.1).abs() > 1e-15;
                    uv = next;
                    d = dn;
                    break;
                }
                du *= 0.5;
                dv *= 0.5;
            }
            if !moved {
                break;
            }
        }
        Some(uv)
    }
}

/// Signed distance from a surface, positive out of the material; each search starts where the
/// last ended (the points asked about come in small steps). A strip's is measured to its
/// nearest point within it: past a contact, to the contact along the face there.
enum Away<'a> {
    Face(Box<(mt::Surface, Cell<Option<(f64, f64)>>)>),
    Strip(&'a Strip),
}

impl<'a> Away<'a> {
    fn face(f: &mt::Face) -> Self {
        Away::Face(Box::new((f.oriented_surface(), Cell::new(None))))
    }

    fn strip(s: &'a Strip) -> Self {
        Away::Strip(s)
    }

    fn at(&self, x: Vec3) -> Option<f64> {
        use mt::{ParametricSurface, ParametricSurface3D, SearchNearestParameter};
        match self {
            Away::Face(face) if matches!(face.0, mt::Surface::Plane(_)) => nearest_on(&face.0, &face.1, x).map(|(q, n, _)| (x - q).dot(n)),
            Away::Face(face) => {
                let (surface, last) = (&face.0, &face.1);
                // The nearest of the search from the last answer, a fresh one and either kept
                // within the surface's parameters: past them a conic's patch runs on round to
                // its far side, where a search can settle.
                let (ru, rv) = surface.try_range_tuple();
                let ((u0, u1), (v0, v1)) = (ru.unwrap_or((f64::NEG_INFINITY, f64::INFINITY)), rv.unwrap_or((f64::NEG_INFINITY, f64::INFINITY)));
                let inside = |uv: (f64, f64)| uv.0 >= u0 - 1e-9 && uv.0 <= u1 + 1e-9 && uv.1 >= v0 - 1e-9 && uv.1 <= v1 + 1e-9;
                let warm = last.get().and_then(|h| surface.search_nearest_parameter(p3(x), Some(h), 20));
                let mut found: Vec<(f64, f64)> = warm.into_iter().collect();
                if !warm.is_some_and(inside) {
                    found.extend(surface.search_nearest_parameter(p3(x), None, 100));
                }
                let clamped: Vec<(f64, f64)> = found.iter().map(|(u, v)| (u.clamp(u0, u1), v.clamp(v0, v1))).collect();
                found.extend(clamped);
                let d = |uv: &(f64, f64)| from_p3(surface.subs(uv.0, uv.1)).dist(x);
                let uv = found.into_iter().filter(|uv| d(uv).is_finite()).min_by(|a, b| d(a).total_cmp(&d(b)))?;
                last.set(Some(uv));
                let q = from_p3(surface.subs(uv.0, uv.1));
                let n = surface.normal(uv.0, uv.1);
                Some((x - q).dot(Vec3::new(n.x, n.y, n.z).normalized()?))
            }
            Away::Strip(s) => {
                let (u, v) = s.nearest(x)?;
                let q = from_p3(s.surface.subs(u, v));
                let n = s.surface.normal(u, v);
                Some((x - q).dot(Vec3::new(n.x, n.y, n.z).normalized()?) * s.flip)
            }
        }
    }
}

/// Unit direction along a curve at `t`.
fn tangent(c: &mt::Curve, t: f64) -> Option<Vec3> {
    use mt::ParametricCurve;
    let d = c.der(t);
    Vec3::new(d.x, d.y, d.z).normalized()
}

/// Nearest parameter on `c` to `x`, started from the nearest of a few samples.
/// The radius of edge `e` seen along the unit `axis`: its points, moved along the axis into one
/// plane, all on one circle (within `tol`).
fn radius_along(e: &mt::Edge, axis: Vec3, tol: f64) -> Option<f64> {
    use mt::{BoundedCurve, ParametricCurve};
    let curve = e.curve();
    let (t0, t1) = curve.range_tuple();
    let at = |f: f64| {
        let p = from_p3(curve.subs(t0 + (t1 - t0) * f));
        p - axis * axis.dot(p)
    };
    let (a, u, v) = (at(0.0), at(0.5) - at(0.0), at(1.0) - at(0.0));
    let w = u.cross(v);
    if w.len2() < 1e-24 || w.normalized()?.dot(axis).abs() < 1.0 - 1e-6 {
        return None;
    }
    let o = a + (v * u.len2() - u * v.len2()).cross(w) * (1.0 / (2.0 * w.len2()));
    let rho = o.dist(a);
    (0..=16).map(|i| at(i as f64 / 16.0)).all(|p| (p.dist(o) - rho).abs() <= tol).then_some(rho)
}

/// The parameter of `c`'s point nearest `x`: the nearest of 65 samples, refined where that
/// comes nearer.
pub(crate) fn param_on(c: &mt::Curve, x: Vec3) -> Option<f64> {
    use mt::{BoundedCurve, ParametricCurve, SearchNearestParameter};
    let (lo, hi) = c.range_tuple();
    let d = |t: f64| from_p3(c.subs(t)).dist(x);
    let hint = (0..=64).map(|i| lo + (hi - lo) * i as f64 / 64.0).min_by(|a, b| d(*a).total_cmp(&d(*b)))?;
    let t = c.search_nearest_parameter(p3(x), hint, 100).unwrap_or(hint).clamp(lo, hi);
    Some(if d(t) <= d(hint) { t } else { hint })
}

/// Where curve `a` crosses curve `b` on face `f`, both lying in it: its parameter on `a` and
/// the point. Of several, the one within `own` (a range of `a`'s parameter) nearest `near`.
fn cross(a: &mt::Curve, b: &mt::Curve, f: &mt::Face, near: Vec3, own: (f64, f64), tol: f64) -> Option<(f64, Vec3)> {
    use mt::{BoundedCurve, ParametricCurve, SearchNearestParameter};
    let (a0, a1) = a.range_tuple();
    let (b0, b1) = b.range_tuple();
    let surface = f.oriented_surface();
    let last = Cell::new(None);
    let hint = Cell::new(None::<f64>);
    // Signed distance across `b` in the face, and how far `b`'s nearest point is.
    // `b` sampled once (an intersection curve is slow to evaluate).
    let along_b: Vec<(f64, Vec3)> = (0..=64).map(|i| b0 + (b1 - b0) * i as f64 / 64.0).map(|t| (t, from_p3(b.subs(t)))).collect();
    let lateral = |s: f64| -> Option<(f64, f64)> {
        let x = from_p3(a.subs(s));
        // From the last answer when that is still nearest, else from the nearest sample of `b`
        // (a search from the far end of a short arc can stay there).
        let d = |t: f64| from_p3(b.subs(t)).dist(x);
        let warm = hint.get().map(|h| b.search_nearest_parameter(p3(x), h, 50).unwrap_or(h).clamp(b0, b1));
        let (start, ds) = along_b.iter().map(|(t, p)| (*t, p.dist(x))).min_by(|p, q| p.1.total_cmp(&q.1))?;
        let t = match warm {
            Some(w) if d(w) <= ds => w,
            _ => {
                let cold = b.search_nearest_parameter(p3(x), start, 50).unwrap_or(start).clamp(b0, b1);
                match warm {
                    Some(w) if d(w) <= d(cold).min(ds) => w,
                    _ if d(cold) <= ds => cold,
                    _ => start,
                }
            }
        };
        hint.set(Some(t));
        let y = from_p3(b.subs(t));
        let (_, n, _) = nearest_on(&surface, &last, y)?;
        let m = n.cross(tangent(b, t)?).normalized()?;
        Some(((x - y).dot(m), x.dist(y)))
    };
    let steps = 256;
    let samples: Vec<(f64, Option<(f64, f64)>)> = (0..=steps)
        .map(|i| {
            let s = a0 + (a1 - a0) * i as f64 / steps as f64;
            (s, lateral(s))
        })
        .collect();
    // Only where `a` comes within a sample's step of `b` (elsewhere the side flips only round
    // an end of `b`).
    let step = samples.windows(2).map(|w| from_p3(a.subs(w[1].0)).dist(from_p3(a.subs(w[0].0)))).fold(0.0, f64::max);
    let close = |d: f64| d <= 2.0 * step + tol;
    let mut found: Vec<(f64, Vec3)> = Vec::new();
    for w in samples.windows(2) {
        let [(s0, Some((h0, d0))), (s1, Some((h1, d1)))] = w else { continue };
        if h1.abs() <= tol && *d1 <= tol {
            found.push((*s1, from_p3(a.subs(*s1))));
            continue;
        }
        if (*h0 < 0.0) == (*h1 < 0.0) || !(close(*d0) || close(*d1)) {
            continue;
        }
        let (mut lo, mut hi, glo) = (*s0, *s1, *h0);
        hint.set(None);
        for _ in 0..60 {
            let m = (lo + hi) * 0.5;
            let Some((hm, _)) = lateral(m) else { break };
            if (hm < 0.0) == (glo < 0.0) {
                lo = m;
            } else {
                hi = m;
            }
        }
        let s = (lo + hi) * 0.5;
        hint.set(None);
        if lateral(s).is_some_and(|(_, d)| d <= tol) {
            found.push((s, from_p3(a.subs(s))));
        }
    }
    if let Some((s, Some((h, d)))) = samples.first()
        && h.abs() <= tol
        && *d <= tol
    {
        found.push((*s, from_p3(a.subs(*s))));
    }
    // Touching without crossing: through an end of `b` (an edge the blend uses up to its far
    // vertex), where the side `b` is measured from turns over. Where the distance comes down
    // to nothing.
    for w in samples.windows(3) {
        let [(s0, Some((_, d0))), (_, Some((_, d1))), (s2, Some((_, d2)))] = w else { continue };
        if !(d1 <= d0 && d1 <= d2 && close(*d1)) {
            continue;
        }
        let gap = |s: f64| {
            hint.set(None);
            lateral(s).map_or(f64::INFINITY, |x| x.1)
        };
        let (mut lo, mut hi) = (*s0, *s2);
        for _ in 0..80 {
            let (x, y) = (lo + (hi - lo) * 0.381_966, lo + (hi - lo) * 0.618_034);
            if gap(x) < gap(y) {
                hi = y;
            } else {
                lo = x;
            }
        }
        let s = (lo + hi) * 0.5;
        if gap(s) <= tol && !found.iter().any(|f| (f.0 - s).abs() <= (a1 - a0) * 1e-6) {
            found.push((s, from_p3(a.subs(s))));
        }
    }
    let slack = (a1 - a0) * 1e-6;
    let outside = |s: f64| s < own.0 - slack || s > own.1 + slack;
    found.into_iter().min_by(|x, y| outside(x.0).cmp(&outside(y.0)).then(x.1.dist(near).total_cmp(&y.1.dist(near))))
}

/// Where curve `c` crosses `side`'s surface: its parameter and the point; of several, the one
/// nearest `near`.
fn reach(c: &mt::Curve, side: &Away, near: Vec3, tol: f64) -> Option<(f64, Vec3)> {
    use mt::{BoundedCurve, ParametricCurve};
    let (lo, hi) = c.range_tuple();
    let g = |s: f64| side.at(from_p3(c.subs(s))).filter(|x| x.is_finite());
    let steps = 256;
    let samples: Vec<(f64, Option<f64>)> = (0..=steps)
        .map(|i| {
            let s = lo + (hi - lo) * i as f64 / steps as f64;
            (s, g(s))
        })
        .collect();
    let mut found: Vec<(f64, Vec3)> = Vec::new();
    for w in samples.windows(2) {
        let [(s0, Some(g0)), (s1, Some(g1))] = w else { continue };
        if (*g0 < 0.0) == (*g1 < 0.0) {
            continue;
        }
        let (mut a, mut b, ga) = (*s0, *s1, *g0);
        for _ in 0..60 {
            let m = (a + b) * 0.5;
            let Some(gm) = g(m) else { break };
            if (gm < 0.0) == (ga < 0.0) {
                a = m;
            } else {
                b = m;
            }
        }
        let s = (a + b) * 0.5;
        if g(s).is_some_and(|x| x.abs() <= tol) {
            found.push((s, from_p3(c.subs(s))));
        }
    }
    found.into_iter().min_by(|x, y| x.1.dist(near).total_cmp(&y.1.dist(near)))
}

/// The curve on strip `s` from `a` to `b` (each u, v and point) where `side` changes sign (it is
/// negative in the material kept): for each u between, the v where it does nearest a guess
/// between the ends, stepping toward `out` (+1: larger v) while inside. Interpolated by chord
/// length through u spaced closer toward the ends; between its points it stays on the strip and
/// on the surface.
fn trace(s: &Strip, a: (f64, f64, Vec3), b: (f64, f64, Vec3), side: &Away, out: f64, tolerance: f64) -> Result<mt::Curve> {
    use mt::ParametricCurve;
    let beyond = || too_large("the blend is larger than the neighbouring faces");
    let (ua, va, pa) = a;
    let (ub, vb, pb) = b;
    let f = |u: f64, v: f64| side.at(s.at(u, v)).unwrap_or(f64::NAN);
    let root = |frac: f64| -> Option<(f64, f64)> {
        let u = ua + (ub - ua) * frac;
        let guess = (va + (vb - va) * frac).clamp(0.0, 1.0);
        // Steps in proportion to the distance from the nearer end, where the curve is known: a
        // face tangent to a contact there (a knife-edge tip) crosses the strip only within a
        // window that narrows toward it.
        let step = (vb - va).abs().max(1e-3) * frac.min(1.0 - frac).clamp(1e-6, 0.5) / 8.0;
        let g = f(u, guess);
        if !g.is_finite() {
            return None;
        }
        let dir = if g < 0.0 { out } else { -out };
        let mut prev = (guess, g);
        for i in 1..=4096 {
            let v = (guess + dir * step * i as f64).clamp(0.0, 1.0);
            let cur = (v, f(u, v));
            if !cur.1.is_finite() {
                return None;
            }
            if (prev.1 < 0.0) != (cur.1 < 0.0) {
                let (mut x, mut y) = (prev, cur);
                for _ in 0..60 {
                    let m = (x.0 + y.0) * 0.5;
                    let fm = f(u, m);
                    if (fm < 0.0) == (x.1 < 0.0) {
                        x = (m, fm);
                    } else {
                        y = (m, fm);
                    }
                }
                return Some((u, (x.0 + y.0) * 0.5));
            }
            if v <= 0.0 || v >= 1.0 {
                return None;
            }
            prev = cur;
        }
        None
    };
    let across = |t: f64| (1.0 - (std::f64::consts::PI * t).cos()) * 0.5;
    let mut m = 8;
    loop {
        let mut pts = vec![pa];
        for i in 1..m {
            let (u, v) = root(across(i as f64 / m as f64)).ok_or_else(beyond)?;
            pts.push(s.at(u, v));
        }
        pts.push(pb);
        let (curve, params) = crate::build::interpolate_cubic_chordal(&pts).ok_or_else(|| fail("end curve"))?;
        let mut accurate = true;
        for (i, w) in params.windows(2).enumerate() {
            let (Some(t0), Some(t1)) = (w.first(), w.get(1)) else { continue };
            root(across((i as f64 + 0.5) / m as f64)).ok_or_else(beyond)?;
            let x = from_p3(curve.subs((t0 + t1) * 0.5));
            let on = s.nearest(x).map(|(a, b)| s.at(a, b).dist(x));
            let off = side.at(x).map(f64::abs);
            if !(on.is_some_and(|d| d <= tolerance) && off.is_some_and(|d| d <= tolerance)) {
                accurate = false;
                break;
            }
        }
        if accurate {
            return Ok(mt::Curve::BSplineCurve(curve));
        }
        if m >= 256 {
            return Err(fail("could not fit where the blends meet"));
        }
        m *= 2;
    }
}

/// Convex (−1) or concave (+1): from the edge's direction as its first face runs it and the
/// faces' outward normals at its middle. At a knife edge (normals opposite), also the
/// direction both faces run on from it.
fn convexity(faces: &[mt::Face], e: &mt::Edge, fa: usize, fb: usize, r: f64) -> Result<(f64, Option<Vec3>)> {
    use mt::{BoundedCurve, ParametricCurve};
    let (Some(f1), Some(f2)) = (faces.get(fa), faces.get(fb)) else { return Err(fail("face")) };
    let used = f1.boundary_iters().into_iter().flatten().find(|x| x.id() == e.id()).ok_or_else(|| fail("edge use"))?;
    let c = used.oriented_curve();
    let (a, b) = c.range_tuple();
    let pm = from_p3(c.subs((a + b) * 0.5));
    let t = tangent(&c, (a + b) * 0.5).ok_or_else(|| fail("tangent"))?;
    let (Some(n1), Some(n2)) = (project(f1, pm, None).map(|x| x.1), project(f2, pm, None).map(|x| x.1)) else { return Err(fail("normals")) };
    if n1.dot(n2) > 1.0 - 1e-6 {
        return Err(unsupported("blending a smooth edge"));
    }
    if n1.dot(n2) < -1.0 + 1e-6 {
        let into = n1.cross(t).normalized().ok_or_else(|| fail("normals"))?;
        return match knife_convex(f1, f2, pm, into, r) {
            Some(true) => Ok((-1.0, Some(into))),
            _ => Err(unsupported("blending a knife edge inside the material (a crack)")),
        };
    }
    Ok((if t.dot(n1.cross(n2)) > 0.0 { -1.0 } else { 1.0 }, None))
}

/// The strip of edge `e` between faces `fa` and `fb`, running on past each end as far as the
/// ball still fits, up to `reach`.
#[allow(clippy::too_many_arguments)]
fn strip(
    faces: &[mt::Face],
    e: &mt::Edge,
    (fa, fb): (usize, usize),
    r: f64,
    round: bool,
    (sigma, knife): (f64, Option<Vec3>),
    reach: f64,
    tol: f64,
    fit_tol: f64,
) -> Result<Strip> {
    use mt::{BoundedCurve, ParametricCurve, ParametricSurface3D, SearchNearestParameter};
    let (Some(f1), Some(f2)) = (faces.get(fa), faces.get(fb)) else { return Err(fail("face")) };
    let (o1, o2) = ([f1], [f2]);
    let curve = e.curve();
    let (t0, t1) = curve.range_tuple();
    let at = |s: f64| -> Option<(Vec3, Vec3)> {
        let t = t0 + (t1 - t0) * s;
        Some((from_p3(curve.subs(t)), tangent(&curve, t)?))
    };
    // At a knife edge each ball starts where the last one was, beside it (the first where the
    // faces are a diameter apart): the edge itself is no start.
    let offset: Cell<Option<Vec3>> = Cell::new(None);
    let ball = |p: Vec3, t: Vec3| -> Option<Section> {
        let start = match (knife, offset.get()) {
            (None, _) => None,
            (Some(_), Some(o)) => Some(p + o),
            (Some(into), None) => Some(knife_start(&o1, &o2, p, t, into, r)?),
        };
        let s = solve_from(&o1, &o2, p, t, r, sigma, 0.0, None, tol * 100.0, start)?;
        if knife.is_some() {
            offset.set(Some(s.c - p));
        }
        Some(s)
    };
    // Along the edge, then on along its curve past its ends (a line runs straight on, a conic's
    // arc round its circle) while the ball fits.
    let mut raw: Vec<Section> = Vec::new();
    let (p0, _) = at(0.0).ok_or_else(|| fail("tangent"))?;
    let n = 32;
    let mut along = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let (p, t) = at(i as f64 / n as f64).ok_or_else(|| fail("tangent"))?;
        along.push(ball(p, t).ok_or_else(|| unsupported("the ball doesn't fit along the edge"))?);
    }
    let length = (0..n).filter_map(|i| Some(at(i as f64 / n as f64)?.0.dist(at((i + 1) as f64 / n as f64)?.0))).sum::<f64>();
    if !(length.is_finite() && length > tol) {
        return Err(fail("edge length"));
    }
    let past = |sign: f64| -> Vec<Section> {
        let mut list = Vec::new();
        for k in 1..=16 {
            let s = (sign * reach * k as f64 / 16.0) / length;
            match at(if sign < 0.0 { s } else { 1.0 + s }).filter(|(p, t)| p.is_finite() && t.is_finite()).and_then(|(p, t)| ball(p, t)) {
                Some(x) => list.push(x),
                None => break,
            }
        }
        list
    };
    let after = past(1.0);
    offset.set(along.first().map(|s| s.c - p0));
    let before = past(-1.0);
    raw.extend(before.iter().rev().copied());
    let first = raw.len();
    raw.extend(along);
    let last = raw.len() - 1;
    raw.extend(after);
    let guide = Guide::new(raw)?;
    let total = guide.lengths.last().copied().ok_or_else(|| fail("spine"))?;
    let ends = [
        guide.lengths.get(first).copied().ok_or_else(|| fail("spine"))? / total,
        guide.lengths.get(last).copied().ok_or_else(|| fail("spine"))? / total,
    ];
    let uniform = |along: f64| -> Result<Section> {
        if along <= 0.0 {
            return guide.sections.first().copied().ok_or_else(|| fail("section"));
        }
        if along >= 1.0 {
            return guide.sections.last().copied().ok_or_else(|| fail("section"));
        }
        let (anchor, t) = guide.anchor(along).ok_or_else(|| unsupported("degenerate rolling-ball spine"))?;
        // This anchor is an interpolated ball centre, so keep it as the solve's
        // initial centre instead of applying another edge-to-centre offset.
        solve_from(&o1, &o2, anchor, t, r, sigma, 0.0, None, tol * 100.0, Some(anchor))
            .ok_or_else(|| unsupported("the ball doesn't fit along the resampled spine"))
    };
    let mut samples = SAMPLES;
    let (list, fit) = loop {
        let list: Vec<Section> = (0..samples).map(|j| uniform(j as f64 / (samples - 1) as f64)).collect::<Result<_>>()?;
        let fit = crate::guard("fit corner blend", || fit_sections(std::slice::from_ref(&list), 0, false, round))?;
        let mut accurate = true;
        'check: for j in 0..samples - 1 {
            for fraction in [0.25, 0.5, 0.75] {
                let along = (j as f64 + fraction) / (samples - 1) as f64;
                if !section_fits(&fit, &uniform(along)?, along, round, sigma, fit_tol) {
                    accurate = false;
                    break 'check;
                }
            }
        }
        if accurate {
            break (list, fit);
        }
        if samples >= MAX_SAMPLES {
            return Err(fail("could not fit the rolling-ball surface"));
        }
        samples = (2 * samples - 1).min(MAX_SAMPLES);
    };
    // Outward: toward the ball centre on a concave edge, away on a convex one.
    let mid = list.get(list.len() / 2).ok_or_else(|| fail("section"))?;
    let m = mid.c + ((mid.q1 - mid.c) + (mid.q2 - mid.c)).normalized().ok_or_else(|| fail("section"))? * r;
    let (u, v) = fit.surface.search_nearest_parameter(p3(m), Some((0.5, 0.5)), 100).ok_or_else(|| fail("orientation"))?;
    let nn = fit.surface.normal(u, v);
    let flip = if Vec3::new(nn.x, nn.y, nn.z).dot((mid.c - m) * sigma) < 0.0 { -1.0 } else { 1.0 };
    let sections = list.iter().enumerate().map(|(j, s)| (j as f64 / (samples - 1) as f64, *s)).collect();
    let [c1, c2] = fit.contacts;
    Ok(Strip { edge: e.clone(), fa, fb, surface: fit.surface, contacts: [c1, c2], ends, flip, sigma, sections })
}

/// The point a radius from all three faces (inside the material for a convex corner), from
/// `start`, and its contacts with them.
fn corner_ball(faces: [&mt::Face; 3], start: Vec3, r: f64, sigma: f64, tol: f64) -> Option<(Vec3, [Vec3; 3])> {
    let mut c = start;
    for _ in 0..100 {
        let mut rows = [[0.0; 3]; 3];
        let mut rhs = [0.0; 3];
        let mut qs = [Vec3::ZERO; 3];
        let mut worst: f64 = 0.0;
        for (((f, row), b), q) in faces.iter().zip(rows.iter_mut()).zip(rhs.iter_mut()).zip(qs.iter_mut()) {
            let (x, n, _) = project(f, c, None)?;
            let g = (c - x).dot(n) - sigma * r;
            *row = [n.x, n.y, n.z];
            *b = -g;
            *q = x;
            worst = worst.max(g.abs());
        }
        let d = crate::polyhedron::solve3_pub(rows, rhs)?;
        let dc = Vec3::new(d[0], d[1], d[2]);
        if worst < tol && dc.len() < tol {
            return Some((c, qs));
        }
        c += dc;
        if !c.is_finite() {
            return None;
        }
    }
    None
}

/// Flip a surface so that its normal near `at` points along `outward`.
fn orient(mut surf: mt::Surface, at: Vec3, outward: Vec3) -> mt::Surface {
    use mt::{ParametricSurface3D, SearchNearestParameter};
    if let Some((u, v)) = surf.search_nearest_parameter(p3(at), None, 100) {
        let n = surf.normal(u, v);
        if Vec3::new(n.x, n.y, n.z).dot(outward) < 0.0 {
            surf = mt::Invertible::inverse(&surf);
        }
    }
    surf
}

/// Where a blend's contact ends at a vertex where it ends alone: crossing the edge there (its
/// parameter and the point), or past that edge's far vertex `w`, crossing the next edge `g` of
/// its face there (`h`: the third edge at `w`, between the faces `f4` beyond and the vertex's
/// third face).
enum Reached {
    Edge(f64, Vec3),
    Past { t: f64, p: Vec3, w: mt::Vertex, g: mt::EdgeID, h: mt::EdgeID, f4: usize },
}

/// A face of the shell being rebuilt, a strip, or a corner's sphere.
#[derive(Clone, Copy, PartialEq)]
enum Owner {
    Face(usize),
    Strip(mt::EdgeID),
    Patch,
}

/// Edges from `from` to `to` through the pool edges of `owner`.
fn connect(from: &mt::Vertex, to: &mt::Vertex, pool: &[(mt::Edge, Vec<Owner>)], owner: Owner) -> Result<Vec<mt::Edge>> {
    let mut out = Vec::new();
    let mut used: HashSet<mt::EdgeID> = HashSet::new();
    let mut cur = from.clone();
    for _ in 0..=pool.len() {
        if &cur == to {
            return Ok(out);
        }
        let next = pool.iter().filter(|(e, o)| o.contains(&owner) && !used.contains(&e.id())).find_map(|(e, _)| {
            if e.front() == &cur {
                Some(e.clone())
            } else if e.back() == &cur {
                Some(e.inverse())
            } else {
                None
            }
        });
        let Some(e) = next else { break };
        used.insert(e.id());
        cur = e.back().clone();
        out.push(e);
    }
    Err(fail("a boundary does not close"))
}

/// The strip's own station nearest the corner ball `c`: the section where the strip stops against
/// that ball, where its direction of leave and its curvature are read.
fn corner_station(strip: &Strip, c: Vec3) -> Option<f64> {
    strip.sections.iter().min_by(|a, b| a.1.c.dist(c).total_cmp(&b.1.c.dist(c))).map(|x| x.0)
}

/// One side of a G2 corner: the corner ball's own section arc of radius `r` about `c`, from its
/// contact `q_from` to its contact `q_to`, each station carrying the direction the corner patch
/// leaves the blend in (towards the corner's apex) and the blend's own curvature across it, read
/// from `strip` where the ball stops against it. A rolling ball's section is a circular arc about
/// its own centre wherever the edge runs, so this arc is the blend's section curve itself, and its
/// ends are the ball's contacts exactly — the sides close the loop whatever the fit's station
/// spacing is.
fn g2_side(strip: &Strip, c: Vec3, r: f64, q_from: Vec3, q_to: Vec3, apex: Vec3) -> Result<Vec<Frame>> {
    use mt::ParametricSurface;
    /// Stations across one section: enough for the patch's cubic cross-sections, few enough to
    /// keep the corner's own patch small.
    const STATIONS: usize = 9;
    let vec_of = |d: mt::Vector3| Vec3::new(d.x, d.y, d.z);
    let v = corner_station(strip, c).ok_or_else(|| fail("corner section"))?;
    let a = (q_from - c).normalized().ok_or_else(|| fail("corner contact"))?;
    let b = (q_to - c).normalized().ok_or_else(|| fail("corner contact"))?;
    let dot = a.dot(b).clamp(-1.0, 1.0);
    let w = (b - a * dot).normalized().ok_or_else(|| fail("corner contact"))?;
    let theta = dot.acos();
    // The blend's own direction of leave and curvature across the seam, at its corner station.
    let speed = vec_of(strip.surface.vder(0.5, v)).len();
    if !(speed > 1e-30) {
        return Err(unsupported("a corner section of no width"));
    }
    let leave = vec_of(strip.surface.vder(0.5, v)) * (1.0 / speed);
    let bend = vec_of(strip.surface.vvder(0.5, v)) * (1.0 / (speed * speed));
    if !(leave.is_finite() && bend.is_finite() && theta.is_finite() && r.is_finite()) {
        return Err(unsupported("a corner section that is not finite"));
    }
    let mut out = Vec::with_capacity(STATIONS);
    for k in 0..STATIONS {
        let t = theta * k as f64 / (STATIONS - 1) as f64;
        let p = c + (a * t.cos() + w * t.sin()) * r;
        // The patch leaves the blend towards the material the corner caps, so the sign is the one
        // towards the corner's apex (the blends' own v direction runs away from the vertex).
        let leave = if leave.dot(apex - p) < 0.0 { -leave } else { leave };
        if !(p.is_finite() && leave.is_finite() && bend.is_finite()) {
            return Err(unsupported("a corner section that is not finite"));
        }
        out.push(Frame { p, leave, bend, bend_change: Vec3::ZERO });
    }
    Ok(out)
}

/// Blend the edges `ids` of `solid` with radius `r` (a chamfer's distance), rebuilding the
/// corners at their ends; with what it built from `solid`.
pub(crate) fn corner_blend(solid: &Solid, ids: &[mt::EdgeID], size: f64, r: f64, round: bool) -> Result<(Solid, Rebuild)> {
    corner_blend_style(solid, ids, size, r, round, false)
}

/// [`corner_blend`]; with `g2`, the corners where three blends meet are G2 patches
/// ([`crate::corner_g2`]) instead of pieces of the rolling ball's sphere.
pub(crate) fn corner_blend_style(solid: &Solid, ids: &[mt::EdgeID], size: f64, r: f64, round: bool, g2: bool) -> Result<(Solid, Rebuild)> {
    if !(size.is_finite() && size > 0.0 && r.is_finite() && r > 0.0) {
        return Err(KernelError::Invalid("blend size and radius must be finite and positive".into()));
    }
    if ids.is_empty() || ids.len() > 64 {
        return Err(KernelError::Invalid("select 1…64 edges".into()));
    }
    let tol = (size * 1e-7).max(1e-9);
    let fit_tol = (size * 1e-5).clamp(1e-5, 0.001);
    let shells = solid.boundaries();
    let si = shells.iter().position(|sh| sh.edge_iter().any(|e| ids.contains(&e.id()))).ok_or_else(|| fail("edge not found"))?;
    let shell = shells.get(si).ok_or_else(|| fail("shell"))?;
    if ids.iter().any(|id| !shell.edge_iter().any(|e| e.id() == *id)) {
        return Err(unsupported("blends on separate lumps in one go"));
    }
    let faces: Vec<mt::Face> = shell.face_iter().cloned().collect();
    let edges: HashMap<mt::EdgeID, mt::Edge> = shell.edge_iter().map(|e| (e.id(), e.absolute_clone())).collect();
    let mut on_edge: HashMap<mt::EdgeID, Vec<usize>> = HashMap::new();
    let mut at_vertex: HashMap<mt::VertexID, (HashSet<mt::EdgeID>, Vec<usize>)> = HashMap::new();
    for (fi, f) in faces.iter().enumerate() {
        for e in f.edge_iter() {
            let list = on_edge.entry(e.id()).or_default();
            if !list.contains(&fi) {
                list.push(fi);
            }
            for v in [e.front(), e.back()] {
                let a = at_vertex.entry(v.id()).or_default();
                a.0.insert(e.id());
                if !a.1.contains(&fi) {
                    a.1.push(fi);
                }
            }
        }
    }
    let face_with = |a: mt::EdgeID, b: mt::EdgeID| -> Result<usize> {
        let fa = on_edge.get(&a).cloned().unwrap_or_default();
        let fb = on_edge.get(&b).cloned().unwrap_or_default();
        fa.into_iter().find(|f| fb.contains(f)).ok_or_else(|| fail("faces"))
    };
    let face = |fi: usize| faces.get(fi).ok_or_else(|| fail("face"));

    // The vertices at the blended edges' ends: three faces, three edges.
    let mut touched: HashMap<mt::VertexID, mt::Vertex> = HashMap::new();
    let mut sides: HashMap<mt::EdgeID, (usize, usize, f64, Option<Vec3>)> = HashMap::new();
    for id in ids {
        let e = edges.get(id).ok_or_else(|| fail("edge"))?;
        if e.front() == e.back() {
            return Err(unsupported("blending a closed edge at a corner"));
        }
        let fs = on_edge.get(id).cloned().unwrap_or_default();
        let [fa, fb] = fs[..] else { return Err(unsupported("the edge is not between two faces")) };
        let (sigma, knife) = convexity(&faces, e, fa, fb, r)?;
        sides.insert(*id, (fa, fb, sigma, knife));
        touched.insert(e.front().id(), e.front().clone());
        touched.insert(e.back().id(), e.back().clone());
    }
    for vid in touched.keys() {
        let (eids, fids) = at_vertex.get(vid).ok_or_else(|| fail("vertex"))?;
        if eids.len() != 3 || fids.len() != 3 {
            return Err(unsupported("blends ending where more than three edges meet"));
        }
        let chosen: Vec<&mt::EdgeID> = eids.iter().filter(|id| sides.contains_key(id)).collect();
        if chosen.len() >= 2 && chosen.iter().any(|id| sides.get(*id).is_none_or(|s| s.2 > 0.0)) {
            return Err(unsupported("concave blends meeting at a corner"));
        }
        if chosen.len() == 3 && !round {
            return Err(unsupported("three chamfers at one corner"));
        }
        // A blended edge running on smoothly into one that isn't: the blend follows it there
        // (a chain round the corner, blended elsewhere), unless it is an earlier round's arc of
        // this radius, which the blend ends in. Round a smaller one the ball can't.
        let v = touched.get(vid).ok_or_else(|| fail("vertex"))?;
        let leaves = |e: &mt::Edge| {
            use mt::BoundedCurve;
            let c = e.curve();
            let (t0, t1) = c.range_tuple();
            tangent(&c, if e.front() == v { t0 } else { t1 }).map(|d| if e.front() == v { d } else { -d })
        };
        for id in &chosen {
            let Some(d) = edges.get(*id).and_then(leaves) else { continue };
            for g in eids.iter().filter(|g| !sides.contains_key(*g)) {
                let Some(e) = edges.get(g) else { continue };
                if !leaves(e).is_some_and(|x| x.dot(d) < -1.0 + 1e-6) {
                    continue;
                }
                // The earlier round's end may also be where it meets a curved face: a circle only
                // seen along the round's axis, which its contact with the third face runs along.
                let third = eids.iter().find(|x| *x != *id && *x != g).and_then(|x| edges.get(x)).and_then(leaves);
                let seen = super::circle_of(e, tol * 1e3).map(|c| c.1).or_else(|| third.and_then(|a| radius_along(e, a, tol * 1e3)));
                match seen {
                    Some(rho) if (rho - r).abs() <= tol * 1e3 => {}
                    Some(rho) if rho < r => return Err(too_large("the blend is larger than a corner radius")),
                    _ => return Err(unsupported("a blend running on smoothly into an edge not blended")),
                }
            }
        }
    }

    crate::guard("blend corners", || {
        // The strips.
        let mut strips: HashMap<mt::EdgeID, Strip> = HashMap::new();
        for id in ids {
            let (Some(e), Some((fa, fb, sigma, knife))) = (edges.get(id), sides.get(id).copied()) else { return Err(fail("edge")) };
            strips.insert(*id, strip(&faces, e, (fa, fb), r, round, (sigma, knife), 8.0 * r, tol, fit_tol)?);
        }
        let strip_of = |id: &mt::EdgeID| strips.get(id).ok_or_else(|| fail("strip"));
        // New end points: (edge, vertex, the face of a blended edge's contact).
        let mut endpt: HashMap<(mt::EdgeID, mt::VertexID, Option<usize>), mt::Vertex> = HashMap::new();
        let mut pool: HashMap<mt::VertexID, Vec<(mt::Edge, Vec<Owner>)>> = HashMap::new();
        let mut patches: Vec<(mt::Face, Vec<mt::EdgeID>)> = Vec::new();
        // Edges a blend uses up to their far vertex (an earlier round's edge, when the radii
        // are equal): left out, the blend ends at that vertex.
        let mut consumed: HashSet<mt::EdgeID> = HashSet::new();
        // Vertices a blend takes in (an earlier round's corner past the end of its edge).
        let mut gone: HashMap<mt::VertexID, mt::Vertex> = HashMap::new();
        // In a fixed order (by position): the work and the result don't depend on hashing.
        let mut order_v: Vec<(&mt::VertexID, &mt::Vertex)> = touched.iter().collect();
        order_v.sort_by(|a, b| {
            let (p, q) = (vtx(a.1), vtx(b.1));
            p.x.total_cmp(&q.x).then(p.y.total_cmp(&q.y)).then(p.z.total_cmp(&q.z))
        });
        for (vid, vert) in order_v {
            let v = vtx(vert);
            let (eids, _) = at_vertex.get(vid).ok_or_else(|| fail("vertex"))?;
            let mut chosen: Vec<mt::EdgeID> = eids.iter().filter(|id| sides.contains_key(id)).copied().collect();
            let rest: Vec<mt::EdgeID> = eids.iter().filter(|id| !sides.contains_key(id)).copied().collect();
            chosen.sort_by_key(|id| ids.iter().position(|x| x == id));
            let curve_of = |id: &mt::EdgeID| edges.get(id).map(|e| e.curve()).ok_or_else(|| fail("edge"));
            // The new end on edge `id` at `p`: its far vertex when the blend reaches it.
            // A contact (`along`: the curves arriving there, and where) that runs into that
            // vertex along another of its edges would leave a face no wider than nothing: refused.
            let at_end = |id: &mt::EdgeID, p: Vec3, along: &[(&mt::Curve, f64, usize)]| -> Result<(mt::Vertex, bool)> {
                let far = edges.get(id).map(|e| if e.front().id() == *vid { e.back().clone() } else { e.front().clone() });
                let Some(w) = far.filter(|w| vtx(w).dist(p) <= fit_tol * 10.0 && !touched.contains_key(&w.id())) else {
                    return Ok((builder::vertex(p3(p)), false));
                };
                let others = at_vertex.get(&w.id()).map(|x| x.0.iter().filter(|g| *g != id).copied().collect::<Vec<_>>()).unwrap_or_default();
                for g in others {
                    use mt::BoundedCurve;
                    let Some(e) = edges.get(&g) else { continue };
                    let c = e.curve();
                    let (t0, t1) = c.range_tuple();
                    let leaves = tangent(&c, if e.front() == &w { t0 } else { t1 });
                    let of = on_edge.get(&g).cloned().unwrap_or_default();
                    if along
                        .iter()
                        .filter(|(_, _, f)| of.contains(f))
                        .filter_map(|(c, t, _)| tangent(c, *t))
                        .any(|d| leaves.is_some_and(|l| l.dot(d).abs() > 1.0 - 1e-6))
                    {
                        return Err(unsupported("a blend that runs along an edge where it ends"));
                    }
                }
                Ok((w, true))
            };
            let mut ending = |id: &mt::EdgeID, p: Vec3, along: &[(&mt::Curve, f64, usize)]| -> Result<mt::Vertex> {
                let (w, used) = at_end(id, p, along)?;
                if used {
                    consumed.insert(*id);
                }
                Ok(w)
            };
            let list = pool.entry(*vid).or_default();
            match (&chosen[..], &rest[..]) {
                ([e1], [e2, e3]) => {
                    let s1 = strip_of(e1)?;
                    let (f12, f31, f23) = (face_with(*e1, *e2)?, face_with(*e1, *e3)?, face_with(*e2, *e3)?);
                    let (_, out) = s1.end_at(vert);
                    let (c12, c31) = (s1.contact(f12)?, s1.contact(f31)?);
                    let none = || too_large("the blend is larger than the neighbouring faces");
                    // Each contact crosses its edge at the vertex, or runs past that edge's far
                    // end (the blend takes in that corner, an earlier round's) to the next edge of
                    // its face there.
                    let side = |ei: &mt::EdgeID, fi: usize| -> Result<Reached> {
                        let c = s1.contact(fi)?;
                        if let Some((t, p)) = cross(c, &curve_of(ei)?, face(fi)?, v, s1.own(), fit_tol) {
                            return Ok(Reached::Edge(t, p));
                        }
                        // Only an earlier round's corner: the third face runs on smoothly from a
                        // side at the vertex (else the blend would wrap round a corner of the part).
                        let normal = |f: usize| face(f).ok().and_then(|g| project(g, v, None)).map(|x| x.1);
                        let smooth = normal(f23).is_some_and(|n| [f12, f31].iter().any(|f| normal(*f).is_some_and(|m| m.dot(n) > 1.0 - 1e-6)));
                        if !smooth {
                            return Err(none());
                        }
                        let e = edges.get(ei).ok_or_else(|| fail("edge"))?;
                        let w = if e.front().id() == *vid { e.back().clone() } else { e.front().clone() };
                        let (ws, wf) = at_vertex.get(&w.id()).ok_or_else(|| fail("vertex"))?;
                        if touched.contains_key(&w.id()) || ws.len() != 3 || wf.len() != 3 || ws.iter().any(|x| sides.contains_key(x)) {
                            return Err(none());
                        }
                        let on = |f: usize| ws.iter().filter(|x| *x != ei).find(|x| on_edge.get(*x).is_some_and(|fs| fs.contains(&f))).copied();
                        let (Some(g), Some(h)) = (on(fi), on(f23)) else { return Err(none()) };
                        let f4 = face_with(g, h)?;
                        let (t, p) = cross(c, &curve_of(&g)?, face(fi)?, v, s1.own(), fit_tol).ok_or_else(none)?;
                        // Through that far vertex itself: the contact reaches the edge's end there
                        // (touching it where it runs in tangent to the contact, as at a knife-edge
                        // tip).
                        if p.dist(vtx(&w)) <= fit_tol * 10.0 {
                            return Ok(Reached::Edge(t, vtx(&w)));
                        }
                        Ok(Reached::Past { t, p, w, g, h, f4 })
                    };
                    match (side(e2, f12)?, side(e3, f31)?) {
                        (Reached::Edge(v2, p2), Reached::Edge(v3, p3_)) => {
                            let (a, b) = (ending(e2, p2, &[(c12, v2, f12)])?, ending(e3, p3_, &[(c31, v3, f31)])?);
                            endpt.insert((*e2, *vid, None), a.clone());
                            endpt.insert((*e1, *vid, Some(f12)), a.clone());
                            endpt.insert((*e3, *vid, None), b.clone());
                            endpt.insert((*e1, *vid, Some(f31)), b.clone());
                            let end = Away::face(face(f23)?);
                            let c = trace(s1, (s1.u(f12)?, v2, p2), (s1.u(f31)?, v3, p3_), &end, out, fit_tol)?;
                            list.push((mt::Edge::new(&a, &b, c), vec![Owner::Face(f23), Owner::Strip(*e1)]));
                        }
                        (r2, r3) => {
                            // The end runs from the edge reached across the third face to where
                            // it crosses the edge `h` from the corner taken in, then across the
                            // face beyond (`f4`) to the next edge `g`.
                            let ((ed, fd, td, pd), (ex, fx, past)) = match (r2, r3) {
                                (Reached::Edge(t, p), past @ Reached::Past { .. }) => ((e2, f12, t, p), (e3, f31, past)),
                                (past @ Reached::Past { .. }, Reached::Edge(t, p)) => ((e3, f31, t, p), (e2, f12, past)),
                                _ => return Err(unsupported("a blend running past both edges where it ends")),
                            };
                            let Reached::Past { t: tx, p: px, w, g, h, f4 } = past else { return Err(fail("corner")) };
                            let a = ending(ed, pd, &[(s1.contact(fd)?, td, fd)])?;
                            let b = builder::vertex(p3(px));
                            let (_, y) = reach(&curve_of(&h)?, &Away::strip(s1), vtx(&w), fit_tol).ok_or_else(none)?;
                            let (u_y, v_y) = s1.nearest(y).ok_or_else(none)?;
                            if s1.at(u_y, v_y).dist(y) > fit_tol * 10.0 {
                                return Err(none());
                            }
                            let yv = builder::vertex(p3(y));
                            consumed.insert(*ex);
                            gone.insert(w.id(), w.clone());
                            endpt.insert((*ed, *vid, None), a.clone());
                            endpt.insert((*e1, *vid, Some(fd)), a.clone());
                            endpt.insert((*e1, *vid, Some(fx)), b.clone());
                            endpt.insert((g, w.id(), None), b.clone());
                            endpt.insert((h, w.id(), None), yv.clone());
                            let first = trace(s1, (s1.u(fd)?, td, pd), (u_y, v_y, y), &Away::face(face(f23)?), out, fit_tol)?;
                            let second = trace(s1, (u_y, v_y, y), (s1.u(fx)?, tx, px), &Away::face(face(f4)?), out, fit_tol)?;
                            list.push((mt::Edge::new(&a, &yv, first), vec![Owner::Face(f23), Owner::Strip(*e1)]));
                            list.push((mt::Edge::new(&yv, &b, second), vec![Owner::Face(f4), Owner::Strip(*e1)]));
                        }
                    }
                }
                ([ea, eb], [e3]) => {
                    let (sa, sb) = (strip_of(ea)?, strip_of(eb)?);
                    let fab = face_with(*ea, *eb)?;
                    let (fa3, fb3) = (face_with(*ea, *e3)?, face_with(*eb, *e3)?);
                    let c3 = curve_of(e3)?;
                    let none = || too_large("the blend is larger than the neighbouring faces");
                    // Where the contacts cross on the shared face, and each reaches the third edge.
                    let (va_q, q) = cross(sa.contact(fab)?, sb.contact(fab)?, face(fab)?, v, sa.own(), fit_tol).ok_or_else(none)?;
                    let vb_q = param_on(sb.contact(fab)?, q).ok_or_else(none)?;
                    let (va_x, xa) = cross(sa.contact(fa3)?, &c3, face(fa3)?, v, sa.own(), fit_tol).ok_or_else(none)?;
                    let (vb_x, xb) = cross(sb.contact(fb3)?, &c3, face(fb3)?, v, sb.own(), fit_tol).ok_or_else(none)?;
                    let qv = builder::vertex(p3(q));
                    endpt.insert((*ea, *vid, Some(fab)), qv.clone());
                    endpt.insert((*eb, *vid, Some(fab)), qv.clone());
                    if xa.dist(xb) <= fit_tol * 10.0 {
                        // The crease runs to the third edge.
                        let x = (xa + xb) * 0.5;
                        let xv = ending(e3, x, &[(sa.contact(fa3)?, va_x, fa3), (sb.contact(fb3)?, vb_x, fb3)])?;
                        endpt.insert((*e3, *vid, None), xv.clone());
                        endpt.insert((*ea, *vid, Some(fa3)), xv.clone());
                        endpt.insert((*eb, *vid, Some(fb3)), xv.clone());
                        let (_, out) = sa.end_at(vert);
                        let c = trace(sa, (sa.u(fab)?, va_q, q), (sa.u(fa3)?, va_x, x), &Away::strip(sb), out, fit_tol)?;
                        list.push((mt::Edge::new(&qv, &xv, c), vec![Owner::Strip(*ea), Owner::Strip(*eb)]));
                    } else {
                        // The strip reaching further along the third edge (`big`) runs on past
                        // the crease to end on the other's third face.
                        let (big, small, ebig, esmall, fbig3, fsmall3, x, vbig_x) =
                            if xa.dist(v) > xb.dist(v) { (sa, sb, *ea, *eb, fa3, fb3, xa, va_x) } else { (sb, sa, *eb, *ea, fb3, fa3, xb, vb_x) };
                        let vs_q = if esmall == *ea { va_q } else { vb_q };
                        let (vs_y, y) = reach(small.contact(fsmall3)?, &Away::strip(big), v, fit_tol).ok_or_else(none)?;
                        let (u_y, v_y) = big.nearest(y).ok_or_else(none)?;
                        if big.at(u_y, v_y).dist(y) > fit_tol * 10.0 {
                            return Err(unsupported("blends this different at one corner"));
                        }
                        let xv = ending(e3, x, &[(big.contact(fbig3)?, vbig_x, fbig3)])?;
                        let yv = builder::vertex(p3(y));
                        endpt.insert((*e3, *vid, None), xv.clone());
                        endpt.insert((ebig, *vid, Some(fbig3)), xv.clone());
                        endpt.insert((esmall, *vid, Some(fsmall3)), yv.clone());
                        let (_, out_s) = small.end_at(vert);
                        let (_, out_b) = big.end_at(vert);
                        let crease = trace(small, (small.u(fab)?, vs_q, q), (small.u(fsmall3)?, vs_y, y), &Away::strip(big), out_s, fit_tol)?;
                        let section = trace(big, (u_y, v_y, y), (big.u(fbig3)?, vbig_x, x), &Away::face(face(fsmall3)?), out_b, fit_tol)?;
                        list.push((mt::Edge::new(&qv, &yv, crease), vec![Owner::Strip(*ea), Owner::Strip(*eb)]));
                        list.push((mt::Edge::new(&yv, &xv, section), vec![Owner::Face(fsmall3), Owner::Strip(ebig)]));
                    }
                }
                ([e1, e2, e3], []) => {
                    let s1 = strip_of(e1)?;
                    let (f12, f23, f31) = (face_with(*e1, *e2)?, face_with(*e2, *e3)?, face_with(*e3, *e1)?);
                    // The corner ball, from the first strip's section nearest the vertex.
                    let (v1, _) = s1.end_at(vert);
                    let start = s1
                        .sections
                        .iter()
                        .min_by(|a, b| (a.0 - v1).abs().total_cmp(&(b.0 - v1).abs()))
                        .map(|x| x.1.c)
                        .ok_or_else(|| fail("section"))?;
                    let (c, [q12, q23, q31]) = corner_ball([face(f12)?, face(f23)?, face(f31)?], start, r, s1.sigma, tol * 100.0)
                        .ok_or_else(|| unsupported("the ball doesn't fit in the corner"))?;
                    let (v12, v23, v31) = (builder::vertex(p3(q12)), builder::vertex(p3(q23)), builder::vertex(p3(q31)));
                    for (e, f, x) in [(e1, f12, &v12), (e2, f12, &v12), (e2, f23, &v23), (e3, f23, &v23), (e3, f31, &v31), (e1, f31, &v31)] {
                        endpt.insert((*e, *vid, Some(f)), x.clone());
                    }
                    let arc = |a: &mt::Vertex, b: &mt::Vertex| -> Result<mt::Edge> {
                        let m = c + ((vtx(a) - c) + (vtx(b) - c)).normalized().ok_or_else(|| fail("corner arc"))? * r;
                        Ok(builder::circle_arc(a, b, p3(m)))
                    };
                    let (a1, a2, a3) = (arc(&v12, &v31)?, arc(&v12, &v23)?, arc(&v23, &v31)?);
                    // A sphere with its poles and seam away from the patch.
                    let n = |f: usize, q: Vec3| face(f).ok().and_then(|g| project(g, q, None)).map(|x| x.1).unwrap_or(Vec3::Z);
                    let out = (n(f12, q12) + n(f23, q23) + n(f31, q31)).normalized().unwrap_or(Vec3::Z);
                    let side = out.any_perp();
                    let (np, sp, back) = (c + side * r, c - side * r, c - out * r);
                    if g2 {
                        // A G2 patch on the blends' own end sections, gathered at the ball's pole
                        // (the point the sphere's meridians are built through above). Each side is
                        // the strip's section through the ball's contact with the face it shares
                        // with the previous strip; the strips run on past their edges, so that
                        // station is found by projecting the contact point, not taken from the
                        // edge's own end.
                        let s2 = strip_of(e2)?;
                        let s3 = strip_of(e3)?;
                        // The cap's own pole: the ball's point nearest the corner (the sphere
                        // patch's `back` above is its opposite pole, where the meridian starts).
                        let apex = c + out * r;
                        // Side 1 runs from the ball's contact with the face it shares with e3
                        // round to the one it shares with e2, and so on round the loop: each
                        // side's last station is the next side's first, at that contact.
                        let sides = [g2_side(s1, c, r, q31, q12, apex)?, g2_side(s2, c, r, q12, q23, apex)?, g2_side(s3, c, r, q23, q31, apex)?];
                        let _patch = crate::corner_g2::corner_g2_patch(&sides, apex)?;
                        // Sewing the patch into the shell is not done yet: it brings its own
                        // boundary vertices where the strips' contacts end at the ball's contacts
                        // (`v12`/`v23`/`v31`), and a truck vertex is its allocation, not its point,
                        // so the rebuilt shell does not close there (rewrapping the section curves
                        // onto the contacts is the missing step). Refused rather than left open:
                        // the rolling ball fills the corner instead.
                        return Err(unsupported(
                            "a G2 corner sewn between these blends (the patch's section curves do not join the blends' contacts yet)",
                        ));
                    }
                    let meridian = builder::circle_arc(&builder::vertex(p3(np)), &builder::vertex(p3(sp)), p3(back));
                    let surf = mt::Surface::RevolutedCurve(mt::Processor::new(mt::RevolutedCurve::by_revolution(
                        meridian.oriented_curve(),
                        p3(c),
                        v3(side),
                    )));
                    let surf = orient(surf, c + out * r, out * -s1.sigma);
                    let wire: mt::Wire = vec![a2.clone(), a3.clone(), a1.inverse()].into();
                    let patch = mt::Face::try_new(vec![wire], surf).map_err(|e| fail(&format!("corner patch: {e}")))?;
                    patches.push((patch, vec![*e1, *e2, *e3]));
                    list.push((a1, vec![Owner::Strip(*e1), Owner::Patch]));
                    list.push((a2, vec![Owner::Strip(*e2), Owner::Patch]));
                    list.push((a3, vec![Owner::Strip(*e3), Owner::Patch]));
                }
                _ => return Err(unsupported("this corner")),
            }
        }

        // Rebuilt edges: contacts per (edge, face), shortened edges per edge.
        let end_of =
            |id: mt::EdgeID, v: &mt::Vertex, f: Option<usize>| -> mt::Vertex { endpt.get(&(id, v.id(), f)).cloned().unwrap_or_else(|| v.clone()) };
        let mut lines: HashMap<(mt::EdgeID, Option<usize>), mt::Edge> = HashMap::new();
        let mut line_of = |e: &mt::Edge, f: Option<usize>| -> Result<mt::Edge> {
            use mt::Cut;
            if let Some(x) = lines.get(&(e.id(), f)) {
                return Ok(x.clone());
            }
            let abs = e.absolute_clone();
            let (a, b) = (end_of(e.id(), abs.front(), f), end_of(e.id(), abs.back(), f));
            let x = match (f, strips.get(&e.id())) {
                (Some(fi), Some(s)) => {
                    // The contact between its new ends.
                    let c = s.contact(fi)?;
                    let none = || too_large("the blend is larger than the neighbouring faces");
                    let (ta, tb) = (param_on(c, vtx(&a)).ok_or_else(none)?, param_on(c, vtx(&b)).ok_or_else(none)?);
                    if tb - ta < 1e-9 {
                        return Err(too_large("the blend is larger than the neighbouring faces"));
                    }
                    let mut piece = c.clone();
                    let _ = piece.cut(tb);
                    let piece = piece.cut(ta);
                    mt::Edge::try_new(&a, &b, piece).map_err(|e| fail(&format!("contact: {e}")))?
                }
                _ => {
                    let orig = vtx(abs.back()) - vtx(abs.front());
                    if (vtx(&b) - vtx(&a)).dot(orig) <= size * 1e-9 * orig.len() {
                        return Err(too_large("the blend is larger than the neighbouring faces"));
                    }
                    if matches!(abs.curve(), mt::Curve::Line(_)) {
                        builder::line(&a, &b)
                    } else {
                        let moved = |n: &mt::Vertex, o: &mt::Vertex| (n != o).then(|| n.clone());
                        super::trim_curved_seam(&abs, moved(&a, abs.front()), moved(&b, abs.back()), size)?
                    }
                }
            };
            lines.insert((e.id(), f), x.clone());
            Ok(x)
        };
        let changed = |v: &mt::Vertex| touched.contains_key(&v.id()) || gone.contains_key(&v.id());
        let moved = |e: &mt::Edge| changed(e.front()) || changed(e.back());
        let everywhere: Vec<(mt::Edge, Vec<Owner>)> = pool.values().flatten().cloned().collect();
        let mut rec = Rebuild::default();
        let mut out: Vec<mt::Face> = Vec::with_capacity(faces.len() + strips.len() + patches.len());
        for (fi, f) in faces.iter().enumerate() {
            if !f.vertex_iter().any(|v| changed(&v)) {
                out.push(f.clone());
                continue;
            }
            let mut wires = Vec::new();
            for w in f.boundaries() {
                let olds: Vec<mt::Edge> = w.edge_iter().filter(|e| !consumed.contains(&e.id())).cloned().collect();
                let mut news = Vec::with_capacity(olds.len());
                for e in &olds {
                    let ne = if strips.contains_key(&e.id()) {
                        line_of(e, Some(fi))?
                    } else if moved(e) {
                        let piece = line_of(e, None)?;
                        rec.piece(&piece, e);
                        piece
                    } else {
                        news.push(e.clone());
                        continue;
                    };
                    news.push(if e.front() == e.absolute_front() { ne } else { ne.inverse() });
                }
                let n = news.len();
                let mut ws = Vec::new();
                for i in 0..n {
                    let (Some(a), Some(b)) = (news.get(i), news.get((i + 1) % n)) else { continue };
                    ws.push(a.clone());
                    if a.back() != b.front() {
                        ws.extend(connect(a.back(), b.front(), &everywhere, Owner::Face(fi))?);
                    }
                }
                wires.push(mt::Wire::from(ws));
            }
            let rebuilt = mt::Face::try_new(wires, f.oriented_surface()).map_err(|e| fail(&format!("face: {e}")))?;
            rec.face(&rebuilt, f);
            out.push(rebuilt);
        }
        // The strips: back along the first face's contact, round the front corner, along the
        // second's, round the back corner.
        for id in ids {
            let (Some(s), Some(e)) = (strips.get(id), edges.get(id)) else { continue };
            let (va, vb) = (e.front(), e.back());
            let a_forward = faces.get(s.fa).is_some_and(|f| f.boundaries().iter().any(|w| w.edge_iter().any(|x| x.id() == *id && x.front() == va)));
            let la = line_of(e, Some(s.fa))?;
            let lb = line_of(e, Some(s.fb))?;
            let (first, second) = if a_forward { (la.inverse(), lb) } else { (lb.inverse(), la) };
            let p1 = pool.get(&va.id()).map(|x| &x[..]).unwrap_or(&[]);
            let p2 = pool.get(&vb.id()).map(|x| &x[..]).unwrap_or(&[]);
            let mut w = vec![first.clone()];
            w.extend(connect(first.back(), second.front(), p1, Owner::Strip(*id))?);
            w.push(second.clone());
            w.extend(connect(second.back(), first.front(), p2, Owner::Strip(*id))?);
            let surface = if s.flip < 0.0 { mt::Invertible::inverse(&s.surface) } else { s.surface.clone() };
            let surface = in_proportion(&surface)?;
            let blend = mt::Face::try_new(vec![w.into()], surface).map_err(|e| fail(&format!("blend face: {e}")))?;
            rec.blend(&blend, [*id]);
            out.push(blend);
        }
        // Corner patches run each section opposite to its strip.
        for (mut p, corner) in patches {
            let uses = |f: &mt::Face, id: mt::EdgeID| {
                f.boundaries().iter().flat_map(|w| w.edge_iter().cloned().collect::<Vec<_>>()).find(|x| x.id() == id).map(|x| x.front().id())
            };
            let flip = p
                .boundaries()
                .first()
                .and_then(|w| w.edge_iter().next())
                .and_then(|pe| out.iter().find_map(|f| uses(f, pe.id())).map(|front| front == pe.front().id()));
            if flip == Some(true) {
                let surf = p.oriented_surface();
                let wires: Vec<mt::Wire> = p.boundaries().iter().map(|w| w.inverse()).collect();
                p = mt::Face::try_new(wires, surf).map_err(|e| fail(&format!("corner patch: {e}")))?;
            }
            rec.blend(&p, corner);
            out.push(p);
        }
        let mut shells = solid.boundaries().clone();
        if let Some(sh) = shells.get_mut(si) {
            *sh = out.into();
        }
        let res = Solid::try_new(shells).map_err(|e| fail(&format!("invalid solid: {e}")))?;
        Ok((res, rec))
    })
}

#[cfg(test)]
#[path = "curvecorner_tests.rs"]
mod tests;
