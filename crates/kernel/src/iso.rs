//! Isoparametric curves of a face: the curves of its surface along which one parameter is held
//! (u = const or v = const), each exactly as a record curve, cut to the face by its trims (the
//! pcurves of its loops in (u, v)). One routine clips an iso line to a face ([`FaceTrims::clip`]);
//! the curve through a point ([`iso_curves`], projected into sketches) and the analysis grid
//! across a face ([`iso_grid`], drawn by Isocurve Analysis) both use it.
//!
//! Seams, poles and periods need nothing of their own: a face's loops and pcurves lie in one lift
//! of its surface's (u, v) inside its box ([`brep::Face::domain`]), a seam edge is in the loop
//! twice (once on each side of the box), and a pole is a degenerate edge or a vertex loop on the
//! box's side, where the outer loop's polygon closes. Every iso line of a closed surface is
//! therefore one line across the box, cut where it crosses the loops.

use solidcraft_geom::Vec3;
use solidcraft_geom::brep;
use solidcraft_geom::brep::eval::SurfaceEval;

use crate::body::Body;
use crate::point_path::{MAX_CURVE_POINTS, curve_path};
use crate::{KernelError, Result};

/// Samples per knot span (and at least this many per pcurve) where the iso curves look for the
/// crossings of a face's loops and classify the face's inside.
const ISO_SAMPLES: usize = 64;

/// Most iso curves each way [`iso_grid`] draws across one face.
pub const MAX_ISO_COUNT: usize = 100;

/// One piece of an isoparametric curve of a face, inside the face.
#[derive(Clone, Debug, PartialEq)]
pub struct IsoLine {
    /// The curve runs along the surface's first parameter (u runs, v is held), or its second.
    pub along_u: bool,
    /// The parameter held: v when `along_u`, else u.
    pub fixed: f64,
    /// The exact record curve on `interval` of its own parameter.
    pub curve: brep::Curve,
    pub interval: [f64; 2],
    /// Its points from the start of `interval` to its end, at the chord tolerance asked for.
    pub points: Vec<Vec3>,
}

/// A face's surface and its loops in (u, v), read once for any number of iso lines.
struct FaceTrims<'a> {
    surface: &'a brep::Surface,
    eval: SurfaceEval,
    domain: [[f64; 2]; 2],
    /// Each edge loop's pcurves, each with its samples ([`pcurve_samples`]) and whether the loop
    /// runs along it (from its interval's start to its end).
    loops: Vec<Vec<(&'a brep::Curve2, Vec<(f64, [f64; 2])>, bool)>>,
    /// Each edge loop as a closed polygon in (u, v), for telling the face's inside.
    polygons: Vec<Vec<[f64; 2]>>,
}

impl<'a> FaceTrims<'a> {
    fn new(body: &'a Body, face: usize) -> Result<FaceTrims<'a>> {
        let f = body.record().faces.get(face).ok_or_else(|| KernelError::Invalid(format!("the body has no face {face}")))?;
        let surface = f.parameters();
        let eval = SurfaceEval::new(surface).map_err(|e| KernelError::Failed(format!("iso curve: {e}")))?;
        let loops: Vec<Vec<(&brep::Curve2, Vec<(f64, [f64; 2])>, bool)>> = f
            .loops
            .iter()
            .filter_map(|l| match l {
                brep::Loop::Edges { edges } => Some(edges.iter().map(|e| (&e.pcurve, pcurve_samples(&e.pcurve), e.forward)).collect()),
                // A pole: the outer loop's polygon closes along the box's side there.
                brep::Loop::Vertex { .. } => None,
            })
            .collect();
        let polygons = loops
            .iter()
            .map(|l| {
                // In the loop's order: a pcurve runs along its edge's interval whichever way the
                // loop does, so one the loop runs against is walked backwards.
                let mut poly: Vec<[f64; 2]> = Vec::new();
                for (_, samples, forward) in l {
                    let pts = samples.iter().map(|(_, q)| *q);
                    if *forward { poly.extend(pts) } else { poly.extend(pts.rev()) }
                }
                // Closed: a loop whose pcurves leave a side of the box out (a sphere's poles, as
                // vertex loops) has that side as its closing edge.
                if let Some(first) = poly.first().copied() {
                    poly.push(first);
                }
                poly
            })
            .collect();
        Ok(FaceTrims { surface, eval, domain: f.domain, loops, polygons })
    }

    /// The face's own extent in (u, v): the box around its loops, within the surface's box.
    fn extent(&self) -> [[f64; 2]; 2] {
        let mut b = [[f64::INFINITY, f64::NEG_INFINITY]; 2];
        for q in self.polygons.iter().flatten() {
            for (k, x) in q.iter().enumerate() {
                b[k] = [b[k][0].min(*x), b[k][1].max(*x)];
            }
        }
        let [[u0, u1], [v0, v1]] = self.domain;
        [[b[0][0].max(u0), b[0][1].min(u1)], [b[1][0].max(v0), b[1][1].min(v1)]]
    }

    /// Whether `q` is inside the face: inside its outer loop and outside its holes.
    fn inside(&self, q: [f64; 2]) -> bool {
        self.polygons.iter().filter(|l| crosses_right(q, l)).count() % 2 == 1
    }

    /// The iso line of the surface at `fixed` (v when `along_u`, else u) across the face's box,
    /// cut to the face: the pieces inside it, each as its exact record curve on its interval, in
    /// the order the line runs (none when the line misses the face).
    fn clip(&self, along_u: bool, fixed: f64) -> Result<Vec<(brep::Curve, [f64; 2])>> {
        let [[u0, u1], [v0, v1]] = self.domain;
        let (lo, hi) = if along_u { (u0, u1) } else { (v0, v1) };
        let at_t = |t: f64| if along_u { [t, fixed] } else { [fixed, t] };
        // Where the line crosses the loops' pcurves (along it), each crossing solved on the
        // pcurve itself, with the box's ends.
        let across = |q: [f64; 2]| if along_u { q[1] - fixed } else { q[0] - fixed };
        let along = |q: [f64; 2]| if along_u { q[0] } else { q[1] };
        let mut cuts = vec![lo, hi];
        for (c, samples, _) in self.loops.iter().flatten() {
            for w in samples.windows(2) {
                let ((ta, qa), (tb, qb)) = (w[0], w[1]);
                let (ka, kb) = (across(qa), across(qb));
                if (ka < 0.0) == (kb < 0.0) || ka == kb {
                    continue;
                }
                let (mut x, mut y) = (ta, tb);
                for _ in 0..80 {
                    let m = 0.5 * (x + y);
                    match c.eval(m).map(across) {
                        Some(k) if (k < 0.0) == (ka < 0.0) => x = m,
                        Some(_) => y = m,
                        None => break,
                    }
                }
                if let Some(t) = c.eval(0.5 * (x + y)).map(along).filter(|t| *t > lo && *t < hi) {
                    cuts.push(t);
                }
            }
        }
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|a, b| (*a - *b).abs() <= 1e-9 * (1.0 + b.abs()));
        let mut out = Vec::new();
        // Neighbouring inside pieces are one piece.
        let mut run: Option<(f64, f64)> = None;
        for w in cuts.windows(2) {
            let (a, b) = (w[0], w[1]);
            if !(b - a > 1e-12) {
                continue;
            }
            if self.inside(at_t(0.5 * (a + b))) {
                run = Some(run.map_or((a, b), |(s, _)| (s, b)));
            } else if let Some(r) = run.take() {
                out.push(iso_record(self.surface, along_u, fixed, r)?);
            }
        }
        if let Some(r) = run {
            out.push(iso_record(self.surface, along_u, fixed, r)?);
        }
        Ok(out)
    }
}

/// The isoparametric curve of face `face` of `body` through the point `at` on it, the pieces of
/// it inside the face, each as its exact record curve on its interval: along the surface's first
/// parameter (`along_u`; around a cylinder, cone, sphere or torus) or its second (along them).
/// On a B-spline surface the curve is the surface's row at the point's other parameter (each
/// control point the column through it evaluated there, exactly); on an analytic surface its
/// line or circle. The pieces are where the curve crosses the face's loops (their pcurves).
pub fn iso_curves(body: &Body, face: usize, at: Vec3, along_u: bool) -> Result<Vec<(brep::Curve, [f64; 2])>> {
    let trims = FaceTrims::new(body, face)?;
    let [[u0, u1], [v0, v1]] = trims.domain;
    let size = body.size().max(1e-9);
    let mid = [0.5 * (u0 + u1), 0.5 * (v0 + v1)];
    let [mut pu, mut pv] = trims
        .eval
        .project(at, Some(mid), size * 1e-3 + 1e-6)
        .ok_or_else(|| KernelError::Invalid("iso curve: the point is not on the face".into()))?;
    // Into the face's box, by whole periods.
    let period = trims.eval.domain().period;
    for (x, (lo, hi), p) in [(&mut pu, (u0, u1), period[0]), (&mut pv, (v0, v1), period[1])] {
        if let Some(p) = p.filter(|p| *p > 0.0) {
            *x = lo + (*x - lo).rem_euclid(p);
            if *x > hi + 1e-12 {
                *x -= p;
            }
        }
    }
    let out = trims.clip(along_u, if along_u { pv } else { pu })?;
    if out.is_empty() {
        return Err(KernelError::Invalid("the iso curve runs outside the face".into()));
    }
    Ok(out)
}

/// The isoparametric curves of face `face` of `body` for Isocurve Analysis, the held parameter
/// on a ladder of the face's surface ([`iso_ladder`]) rather than on the face's own extent, so
/// faces on one surface (a face split by a boss, the pieces of a split cylinder) show the same
/// lines, straight across where they meet. `count` (1…[`MAX_ISO_COUNT`]) sets the density: a
/// bounded parameter's range in `count + 1` steps, a linear one in steps of the body's size over
/// `count + 1`, rounded to 1, 2 or 5 times a power of ten. The rungs strictly inside the face's
/// extent are cut to the face ([`FaceTrims::clip`]), their points at chord tolerance `chord`.
/// Along u first, then v.
pub fn iso_grid(body: &Body, face: usize, count: usize, chord: f64) -> Result<Vec<IsoLine>> {
    if !(1..=MAX_ISO_COUNT).contains(&count) {
        return Err(KernelError::Invalid(format!("iso curves: 1…{MAX_ISO_COUNT} each way")));
    }
    if !(chord.is_finite() && chord > 0.0) {
        return Err(KernelError::Invalid("iso curves: the chord tolerance must be positive".into()));
    }
    let trims = FaceTrims::new(body, face)?;
    let spacing = round_spacing(body.size() / (count + 1) as f64)
        .ok_or_else(|| KernelError::Invalid("iso curves: the body has no size to space them by".into()))?;
    let [[u0, u1], [v0, v1]] = trims.extent();
    let mut out = Vec::new();
    for along_u in [true, false] {
        let (lo, hi) = if along_u { (v0, v1) } else { (u0, u1) };
        if !(hi > lo) {
            continue;
        }
        let Some((start, step)) = iso_ladder(trims.surface, &trims.eval, !along_u, count, spacing) else { continue };
        // Rungs on the extent's sides are its boundary, not curves across it.
        let eps = step * 1e-6;
        let first = ((lo + eps - start) / step).ceil();
        let last = ((hi - eps - start) / step).floor();
        if !(first.is_finite() && last.is_finite()) {
            continue;
        }
        // The ladder is spaced so that a face holds at most about `count + 1` rungs; the cap
        // only guards a surface whose parameters run far faster than the body's size.
        let rungs = (last - first + 1.0).clamp(0.0, (4 * MAX_ISO_COUNT) as f64) as usize;
        for k in 0..rungs {
            let fixed = start + (first + k as f64) * step;
            for (curve, interval) in trims.clip(along_u, fixed)? {
                let segs = curve_path(&curve, interval)?;
                let mut points: Vec<Vec3> = segs.first().map(crate::PathSeg::start).into_iter().collect();
                for s in &segs {
                    points.extend(s.points_after_start(chord));
                }
                out.push(IsoLine { along_u, fixed, curve, interval, points });
            }
        }
    }
    Ok(out)
}

/// Where the iso lines of a surface fall in one of its parameters (`held_u`: u, else v):
/// `start + k * step` for whole k, fixed by the surface alone. A bounded parameter (an angle, a
/// B-spline's knot range) is its range in `count + 1` steps from its start. A linear one (a
/// plane's, a cylinder's or cone's along its axis) has rungs `spacing` apart in the world,
/// counted from the world origin along the parameter's gradient, so another record of the same
/// plane or axis (its own origin, its own face) gets the same lines. None for a parameter that is
/// neither.
fn iso_ladder(surface: &brep::Surface, eval: &SurfaceEval, held_u: bool, count: usize, spacing: f64) -> Option<(f64, f64)> {
    use solidcraft_geom::brep::Surface;
    let d = if held_u { 0 } else { 1 };
    if let Some((a, b)) = eval.domain().range.get(d).copied().flatten().filter(|(a, b)| b > a) {
        return Some((a, (b - a) / (count + 1) as f64));
    }
    let v = |q: [f64; 3]| Vec3::new(q[0], q[1], q[2]);
    // The parameter as (P - origin) · g, P a point of the surface.
    let (origin, g) = match surface {
        Surface::Plane { origin, u_axis, v_axis } => (v(*origin), plane_gradient(v(*u_axis), v(*v_axis), held_u)?),
        Surface::PlaneThrough { origin, u_point, v_point } => {
            let o = v(*origin);
            (o, plane_gradient(v(*u_point) - o, v(*v_point) - o, held_u)?)
        }
        // Along the axis: P = origin + z * v + (radial part, at right angles to z).
        Surface::Cylinder { frame, .. } | Surface::Cone { frame, .. } if !held_u => {
            let z = v(frame.x_axis).cross(v(frame.y_axis));
            let zz = z.dot(z);
            if !(zz > 1e-300) {
                return None;
            }
            (v(frame.origin), z * (1.0 / zz))
        }
        _ => return None,
    };
    let len = g.len();
    if !(len.is_finite() && len > 0.0) {
        return None;
    }
    // The parameter of the world plane P · g/|g| = k * spacing is k * spacing * |g| - origin · g.
    Some((-origin.dot(g), spacing * len))
}

/// The gradient of a plane's u (`held_u`) or v in the world: the dual of its axes in their plane.
fn plane_gradient(du: Vec3, dv: Vec3, held_u: bool) -> Option<Vec3> {
    let n = du.cross(dv);
    let nn = n.dot(n);
    if !(nn > 1e-300) {
        return None;
    }
    Some(if held_u { dv.cross(n) } else { n.cross(du) } * (1.0 / nn))
}

/// `x` rounded down to 1, 2 or 5 times a power of ten (None unless it is positive and finite).
fn round_spacing(x: f64) -> Option<f64> {
    if !(x.is_finite() && x > 0.0) {
        return None;
    }
    let p = 10f64.powf(x.log10().floor());
    let m = x / p;
    Some(
        p * if m >= 5.0 {
            5.0
        } else if m >= 2.0 {
            2.0
        } else {
            1.0
        },
    )
}

/// A pcurve's parameters and points at [`ISO_SAMPLES`] per knot span, in its direction.
fn pcurve_samples(c: &brep::Curve2) -> Vec<(f64, [f64; 2])> {
    let Some([a, b]) = c.interval() else { return Vec::new() };
    let spans = match c {
        brep::Curve2::Spline { knots, .. } | brep::Curve2::RationalSpline { knots, .. } => knots.windows(2).filter(|w| w[1] > w[0]).count().max(1),
    };
    let n = (spans * ISO_SAMPLES).min(1 << 16);
    (0..=n)
        .filter_map(|i| {
            let t = a + (b - a) * i as f64 / n as f64;
            c.eval(t).map(|q| (t, q))
        })
        .collect()
}

/// Whether the ray from `q` along +u crosses the closed polygon `l` an odd number of times.
fn crosses_right(q: [f64; 2], l: &[[f64; 2]]) -> bool {
    let mut odd = false;
    for w in l.windows(2) {
        let (a, b) = (w[0], w[1]);
        if (a[1] > q[1]) != (b[1] > q[1]) && q[0] < a[0] + (b[0] - a[0]) * (q[1] - a[1]) / (b[1] - a[1]) {
            odd = !odd;
        }
    }
    odd
}

/// The iso curve of `surface` at `fixed` (the other parameter) as a record curve on `[a, b]` of
/// its own parameter: the surface's own line or circle, or its B-spline row.
fn iso_record(surface: &brep::Surface, along_u: bool, fixed: f64, (a, b): (f64, f64)) -> Result<(brep::Curve, [f64; 2])> {
    use solidcraft_geom::brep::{Curve, Frame, Surface};
    let v = |q: [f64; 3]| Vec3::new(q[0], q[1], q[2]);
    let d = |q: Vec3| [q.x, q.y, q.z];
    let line = |p: Vec3, q: Vec3| (Curve::Line { start: d(p), end: d(q) }, [0.0, 1.0]);
    let circle = |o: Vec3, x: Vec3, y: Vec3, r: f64| (Curve::Circle { frame: Frame { origin: d(o), x_axis: d(x), y_axis: d(y) }, radius: r }, [a, b]);
    let frame = |f: &Frame| (v(f.origin), v(f.x_axis), v(f.y_axis), v(f.x_axis).cross(v(f.y_axis)));
    Ok(match surface {
        Surface::Plane { origin, u_axis, v_axis } => {
            let (o, x, y) = (v(*origin), v(*u_axis), v(*v_axis));
            let at = |t: f64| if along_u { o + x * t + y * fixed } else { o + x * fixed + y * t };
            line(at(a), at(b))
        }
        Surface::PlaneThrough { origin, u_point, v_point } => {
            let (o, x, y) = (v(*origin), v(*u_point) - v(*origin), v(*v_point) - v(*origin));
            let at = |t: f64| if along_u { o + x * t + y * fixed } else { o + x * fixed + y * t };
            line(at(a), at(b))
        }
        Surface::Cylinder { frame: f, radius } => {
            let (o, x, y, z) = frame(f);
            if along_u {
                circle(o + z * fixed, x, y, *radius)
            } else {
                let r = (x * fixed.cos() + y * fixed.sin()) * *radius;
                line(o + r + z * a, o + r + z * b)
            }
        }
        Surface::Cone { frame: f, radius_at_origin, half_angle } => {
            let (o, x, y, z) = frame(f);
            let r = |h: f64| radius_at_origin + h * half_angle.tan();
            if along_u {
                circle(o + z * fixed, x, y, r(fixed))
            } else {
                let dir = x * fixed.cos() + y * fixed.sin();
                line(o + dir * r(a) + z * a, o + dir * r(b) + z * b)
            }
        }
        Surface::Sphere { frame: f, radius } => {
            let (o, x, y, z) = frame(f);
            if along_u {
                circle(o + z * (radius * fixed.sin()), x, y, radius * fixed.cos())
            } else {
                circle(o, x * fixed.cos() + y * fixed.sin(), z, *radius)
            }
        }
        Surface::Torus { frame: f, major_radius, minor_radius } => {
            let (o, x, y, z) = frame(f);
            if along_u {
                circle(o + z * (minor_radius * fixed.sin()), x, y, major_radius + minor_radius * fixed.cos())
            } else {
                let dir = x * fixed.cos() + y * fixed.sin();
                circle(o + dir * *major_radius, dir, z, *minor_radius)
            }
        }
        Surface::Spline { u_knots, v_knots, points } => {
            let rows: Vec<Vec<[f64; 4]>> = points.iter().map(|r| r.iter().map(|q| [q[0], q[1], q[2], 1.0]).collect()).collect();
            spline_iso(u_knots, v_knots, &rows, along_u, fixed, a, b)?
        }
        Surface::RationalSpline { u_knots, v_knots, homogeneous } => spline_iso(u_knots, v_knots, homogeneous, along_u, fixed, a, b)?,
        Surface::PlacedRevolution { .. } => {
            return Err(KernelError::Failed("not supported yet: an iso curve of a revolved surface record".into()));
        }
    })
}

/// The row of a B-spline surface (homogeneous `net[i][j]`, i along u) at `fixed`: along u, each
/// control point the v-column through it evaluated at `fixed` (along v, the u-row), on `[a, b]`.
fn spline_iso(u_knots: &[f64], v_knots: &[f64], net: &[Vec<[f64; 4]>], along_u: bool, fixed: f64, a: f64, b: f64) -> Result<(brep::Curve, [f64; 2])> {
    use solidcraft_geom::bspline::CurveN;
    let bad = || KernelError::Invalid("iso curve: the surface's B-spline does not evaluate".into());
    let (nu, nv) = (net.len(), net.first().map_or(0, Vec::len));
    if nu * nv > MAX_CURVE_POINTS || net.iter().any(|r| r.len() != nv) {
        return Err(bad());
    }
    let (pu, pv) = (u_knots.len().checked_sub(nu + 1).ok_or_else(bad)?, v_knots.len().checked_sub(nv + 1).ok_or_else(bad)?);
    let at = |degree: usize, knots: &[f64], ctrl: Vec<[f64; 4]>| CurveN::<4> { degree, knots: knots.to_vec(), ctrl }.eval(fixed).map(|(p, _)| p);
    let (knots, homogeneous): (Vec<f64>, Vec<[f64; 4]>) = if along_u {
        (u_knots.to_vec(), net.iter().map(|row| at(pv, v_knots, row.clone())).collect::<Option<_>>().ok_or_else(bad)?)
    } else {
        (
            v_knots.to_vec(),
            (0..nv).map(|j| at(pu, u_knots, net.iter().filter_map(|r| r.get(j).copied()).collect())).collect::<Option<_>>().ok_or_else(bad)?,
        )
    };
    Ok((brep::Curve::RationalSpline { knots, homogeneous }, [a, b]))
}
