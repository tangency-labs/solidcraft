//! Sweep paths through 3D sketch curves ([`PointPath`]): drawn 3D splines, which the kernel gets
//! as their fit points and so exactly ([`solidcraft_geom::catmull_rom`]); curves it holds
//! exactly as path segments (sketch curves, body edges: [`edge_path`]); and polylines (curves
//! that reach it only as points: projected onto a face, intersection and iso curves). SMLib
//! follows the path's segments ([`PointPath::segs`]), each free-form span exactly as a
//! [`PathSeg::Bezier`] ([`SpaceBezier`]); [`PointPath::points`] is its polyline, for what takes
//! points (rails, curve networks).

use solidcraft_geom::brep;
use solidcraft_geom::bspline::{HPoint, bezier_point, segment};
use solidcraft_geom::catmull_rom;
use solidcraft_geom::{MAX_BEZIER_CTRL, MAX_BEZIER_DEGREE, Plane, Region2, Vec3};

use crate::body::Body;
use crate::{KernelError, PathSeg, Result, SweepControl};

/// One curve of a [`PointPath`], in the path's direction of travel.
#[derive(Clone, Debug, PartialEq)]
pub enum PathPiece {
    /// Points joined by straight lines.
    Polyline(Vec<Vec3>),
    /// A drawn 3D spline: the centripetal Catmull–Rom spline through `fit`, from its last fit
    /// point back to its first when `reversed`.
    Spline { fit: Vec<Vec3>, reversed: bool },
    /// A curve the kernel holds exactly as path segments end to end: a sketch curve
    /// ([`PathSeg::of_sketch`]) or a body edge ([`edge_path`]).
    Segs(Vec<PathSeg>),
}

impl PathPiece {
    /// The piece's points in its direction of travel: the polyline, or the spline's samples
    /// ([`catmull_rom::samples`]).
    pub fn points(&self) -> Vec<Vec3> {
        match self {
            PathPiece::Polyline(pts) => pts.clone(),
            PathPiece::Spline { fit, reversed } => {
                let mut pts = catmull_rom::samples(fit);
                if *reversed {
                    pts.reverse();
                }
                pts
            }
            PathPiece::Segs(segs) => {
                let mut pts: Vec<Vec3> = segs.first().map(PathSeg::start).into_iter().collect();
                for s in segs {
                    pts.extend(s.points_after_start(1e-2));
                }
                pts
            }
        }
    }

    /// The piece the other way round.
    pub fn reversed(self) -> PathPiece {
        match self {
            PathPiece::Polyline(mut pts) => {
                pts.reverse();
                PathPiece::Polyline(pts)
            }
            PathPiece::Spline { fit, reversed } => PathPiece::Spline { fit, reversed: !reversed },
            PathPiece::Segs(segs) => PathPiece::Segs(segs.iter().rev().map(PathSeg::reversed).collect()),
        }
    }

    /// The piece's exact segments in the direction of travel: a spline of three or more fit
    /// points as its spans ([`PathSeg::Bezier`]), a curve held exactly as its segments; `None`
    /// for a polyline (and a spline of one or two points, a point or a line).
    pub(crate) fn spans(&self) -> Result<Option<Vec<PathSeg>>> {
        let (fit, reversed) = match self {
            PathPiece::Spline { fit, reversed } => (fit, reversed),
            PathPiece::Segs(segs) => return Ok(Some(segs.clone())),
            PathPiece::Polyline(_) => return Ok(None),
        };
        let spans = catmull_rom::beziers(fit).ok_or_else(|| KernelError::Invalid("the 3D sketch spline is not finite".into()))?;
        if spans.is_empty() {
            return Ok(None);
        }
        let seg = |p: [Vec3; 4]| {
            SpaceBezier::cubic(p).map(PathSeg::Bezier).ok_or_else(|| KernelError::Invalid("the 3D sketch spline is not finite".into()))
        };
        Ok(Some(if *reversed {
            spans.into_iter().rev().map(|[a, b, c, d]| seg([d, c, b, a])).collect::<Result<_>>()?
        } else {
            spans.into_iter().map(seg).collect::<Result<_>>()?
        }))
    }
}

/// A sweep path through 3D sketch curves: its pieces end to end, each starting where the one
/// before it ended.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PointPath {
    pub pieces: Vec<PathPiece>,
}

impl PointPath {
    /// A path of one polyline.
    pub fn polyline(pts: Vec<Vec3>) -> PointPath {
        PointPath { pieces: vec![PathPiece::Polyline(pts)] }
    }

    /// The path as one polyline: the first piece's points, then each next piece's after its
    /// first (where the one before ended).
    pub fn points(&self) -> Vec<Vec3> {
        let mut chain: Vec<Vec3> = Vec::new();
        for p in &self.pieces {
            let pts = p.points();
            if chain.is_empty() {
                chain = pts;
            } else {
                chain.extend(pts.into_iter().skip(1));
            }
        }
        chain
    }

    /// The path as segments: each spline span exactly ([`PathSeg::Bezier`]), each polyline as the
    /// lines between its points, starting where the piece before it ended (as [`Self::points`]
    /// chains them).
    pub(crate) fn segs(&self) -> Result<Vec<PathSeg>> {
        let mut out: Vec<PathSeg> = Vec::new();
        for piece in &self.pieces {
            if let Some(spans) = piece.spans()? {
                out.extend(spans);
                continue;
            }
            let mut pts = piece.points();
            if let (Some(end), Some(first)) = (out.last().map(PathSeg::end), pts.first_mut()) {
                *first = end;
            }
            out.extend(pts.windows(2).filter(|w| w[0].dist(w[1]) > 1e-9).map(|w| PathSeg::Line { a: w[0], b: w[1] }));
        }
        Ok(out)
    }
}

/// Sweep a planar region along a path through 3D sketch curves: its splines exactly, its
/// polylines as lines (through SMLib).
pub fn sweep_point_path_with(plane: &Plane, region: &Region2, path: &PointPath, ctl: &SweepControl) -> Result<Body> {
    ctl.check()?;
    crate::sweep_path::smlib_sweep_points(plane, region, path, ctl)
}

/// Most segments one edge's curve may give a path.
const MAX_EDGE_SEGS: usize = 1000;

/// Edge `edge` of `body` (its index in the body's edge order) as the path segments that are its
/// curve exactly, from its start vertex to its end: a line, an arc, rational quadratic spans of
/// an ellipse, the lines of a polyline, the Bézier spans of a B-spline between its knots. An
/// intersection curve, which the record holds only as its surfaces and a leader near it, is
/// refused.
pub fn edge_path(body: &Body, edge: usize) -> Result<Vec<PathSeg>> {
    let record = crate::native::brep_record(body)?;
    let e = record.edges.get(edge).ok_or_else(|| KernelError::Invalid(format!("the body has no edge {edge}")))?;
    curve_path(e.parameters(), e.interval)
}

/// The sketch segments `segs` on `plane` projected along `direction` (pointing from them toward
/// the face) onto the face of `body` at `face_at`, as the record curves SMLib's projection makes
/// (one per piece of the projection that stays on the face), each on its interval.
pub fn project_onto_face(
    body: &Body,
    face_at: Vec3,
    plane: &Plane,
    segs: &[solidcraft_geom::Seg2],
    direction: Vec3,
) -> Result<Vec<(brep::Curve, [f64; 2])>> {
    if !(face_at.is_finite() && direction.is_finite() && direction.len() > 1e-12) || segs.is_empty() || segs.len() > 4096 {
        return Err(KernelError::Invalid("curve projection: a point, a direction and 1…4096 curves".into()));
    }
    crate::smlib_sweeps::project_onto_face(body, face_at, plane, segs, direction)
}

/// The face `face` of `body` through `at` cut along `along` by the plane through `at` that holds
/// the face's normal there (its exact normal from the record) and `along`: SMLib's section curves
/// on that face, as record curves on their intervals.
pub fn face_section_along(body: &Body, face: usize, at: Vec3, along: Vec3) -> Result<Vec<(brep::Curve, [f64; 2])>> {
    use solidcraft_geom::brep::eval::SurfaceEval;
    let f = body.record().faces.get(face).ok_or_else(|| KernelError::Invalid(format!("the body has no face {face}")))?;
    let eval = SurfaceEval::new(f.parameters()).map_err(|e| KernelError::Failed(format!("face section: {e}")))?;
    let [[u0, u1], [v0, v1]] = f.domain;
    let size = body.size().max(1e-9);
    let uv = eval
        .project(at, Some([0.5 * (u0 + u1), 0.5 * (v0 + v1)]), size * 1e-3 + 1e-6)
        .ok_or_else(|| KernelError::Invalid("face section: the point is not on the face".into()))?;
    let (_, du, dv) = eval.derivs(uv[0], uv[1]).ok_or_else(|| KernelError::Failed("face section: the surface does not evaluate there".into()))?;
    let normal = du
        .cross(dv)
        .cross(along)
        .normalized()
        .ok_or_else(|| KernelError::Invalid("face section: the direction runs along the face's normal".into()))?;
    crate::smlib_sweeps::face_section(body, at, at, normal)
}

/// Samples per knot span (and at least this many per pcurve) where the iso curves look for the
/// crossings of a face's loops and classify the face's inside.
const ISO_SAMPLES: usize = 64;

/// The isoparametric curve of face `face` of `body` through the point `at` on it, the pieces of
/// it inside the face, each as its exact record curve on its interval: along the surface's first
/// parameter (`along_u`; around a cylinder, cone, sphere or torus) or its second (along them).
/// On a B-spline surface the curve is the surface's row at the point's other parameter (each
/// control point the column through it evaluated there, exactly); on an analytic surface its
/// line or circle. The pieces are where the curve crosses the face's loops (their pcurves).
pub fn iso_curves(body: &Body, face: usize, at: Vec3, along_u: bool) -> Result<Vec<(brep::Curve, [f64; 2])>> {
    use solidcraft_geom::brep::eval::SurfaceEval;
    let record = body.record();
    let f = record.faces.get(face).ok_or_else(|| KernelError::Invalid(format!("the body has no face {face}")))?;
    let eval = SurfaceEval::new(f.parameters()).map_err(|e| KernelError::Failed(format!("iso curve: {e}")))?;
    let [[u0, u1], [v0, v1]] = f.domain;
    let size = body.size().max(1e-9);
    let mid = [0.5 * (u0 + u1), 0.5 * (v0 + v1)];
    let [mut pu, mut pv] =
        eval.project(at, Some(mid), size * 1e-3 + 1e-6).ok_or_else(|| KernelError::Invalid("iso curve: the point is not on the face".into()))?;
    // Into the face's box, by whole periods.
    let period = eval.domain().period;
    for (x, (lo, hi), p) in [(&mut pu, (u0, u1), period[0]), (&mut pv, (v0, v1), period[1])] {
        if let Some(p) = p.filter(|p| *p > 0.0) {
            *x = lo + (*x - lo).rem_euclid(p);
            if *x > hi + 1e-12 {
                *x -= p;
            }
        }
    }
    // The iso line in (u, v): `fixed` held, the other running over the box.
    let (fixed, (lo, hi)) = if along_u { (pv, (u0, u1)) } else { (pu, (v0, v1)) };
    let at_t = |t: f64| if along_u { [t, fixed] } else { [fixed, t] };
    let pcurves: Vec<Vec<&brep::Curve2>> = f
        .loops
        .iter()
        .filter_map(|l| match l {
            brep::Loop::Edges { edges } => Some(edges.iter().map(|e| &e.pcurve).collect()),
            brep::Loop::Vertex { .. } => None,
        })
        .collect();
    // Where the line crosses the loops' pcurves (along it), each crossing solved on the pcurve
    // itself, with the box's ends.
    let across = |q: [f64; 2]| if along_u { q[1] - fixed } else { q[0] - fixed };
    let along = |q: [f64; 2]| if along_u { q[0] } else { q[1] };
    let mut cuts = vec![lo, hi];
    for c in pcurves.iter().flatten() {
        let samples = pcurve_samples(c);
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
    let loops: Vec<Vec<[f64; 2]>> = pcurves.iter().map(|l| l.iter().flat_map(|c| pcurve_samples(c)).map(|(_, q)| q).collect()).collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() <= 1e-9 * (1.0 + b.abs()));
    let inside = |q: [f64; 2]| loops.iter().filter(|l| crosses_right(q, l)).count() % 2 == 1;
    let mut out = Vec::new();
    // Neighbouring inside pieces are one piece.
    let mut run: Option<(f64, f64)> = None;
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        if !(b - a > 1e-12) {
            continue;
        }
        if inside(at_t(0.5 * (a + b))) {
            run = Some(run.map_or((a, b), |(s, _)| (s, b)));
        } else if let Some(r) = run.take() {
            out.push(iso_record(f.parameters(), along_u, fixed, r)?);
        }
    }
    if let Some(r) = run {
        out.push(iso_record(f.parameters(), along_u, fixed, r)?);
    }
    if out.is_empty() {
        return Err(KernelError::Invalid("the iso curve runs outside the face".into()));
    }
    Ok(out)
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

/// Whether the ray from `q` along +u crosses the closed polyline `l` an odd number of times.
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

/// Most control points a record curve given to [`curve_path`] may have.
const MAX_CURVE_POINTS: usize = 100_000;

/// The record curve `c` on `[t0, t1]` as path segments ([`edge_path`]); the numbers are checked
/// first (they may come from a design file).
pub fn curve_path(c: &brep::Curve, [t0, t1]: [f64; 2]) -> Result<Vec<PathSeg>> {
    let bad = |what: &str| KernelError::Invalid(format!("the curve's {what} does not evaluate"));
    if !(t0.is_finite() && t1.is_finite() && t0 < t1 && t0.abs() < 1e9 && t1.abs() < 1e9) {
        return Err(bad("interval"));
    }
    let v = |q: [f64; 3]| Vec3::new(q[0], q[1], q[2]);
    let frame_ok = |f: &brep::Frame| {
        let (x, y) = (v(f.x_axis), v(f.y_axis));
        v(f.origin).is_finite() && (x.len() - 1.0).abs() < 1e-6 && (y.len() - 1.0).abs() < 1e-6 && x.dot(y).abs() < 1e-6
    };
    let finite = |pts: &[[f64; 3]]| pts.len() <= MAX_CURVE_POINTS && pts.iter().all(|q| v(*q).is_finite());
    match c {
        brep::Curve::Line { start, end } if !finite(&[*start, *end]) => return Err(bad("line")),
        brep::Curve::Circle { frame, radius } if !(frame_ok(frame) && radius.is_finite() && *radius > 0.0) => return Err(bad("circle")),
        brep::Curve::Ellipse { frame, radii } if !(frame_ok(frame) && radii.iter().all(|r| r.is_finite() && *r > 0.0)) => return Err(bad("ellipse")),
        brep::Curve::Polyline { points } | brep::Curve::Spline { points, .. } if !finite(points) => return Err(bad("points")),
        brep::Curve::RationalSpline { homogeneous, .. } if homogeneous.len() > MAX_CURVE_POINTS => return Err(bad("points")),
        _ => {}
    }
    let out = match c {
        brep::Curve::Line { start, end } => {
            let (s, e) = (v(*start), v(*end));
            vec![PathSeg::Line { a: s + (e - s) * t0, b: s + (e - s) * t1 }]
        }
        brep::Curve::Circle { frame, radius } => {
            let (o, x, y) = (v(frame.origin), v(frame.x_axis), v(frame.y_axis));
            vec![PathSeg::Arc { a: o + (x * t0.cos() + y * t0.sin()) * *radius, center: o, axis: x.cross(y), angle: t1 - t0 }]
        }
        brep::Curve::Ellipse { frame, radii } => {
            // Each span of at most a quarter turn the affine image of a circular arc's rational
            // quadratic: ends on the ellipse, the middle point at the tangents' meeting, weight
            // cos(half the span).
            let (o, x, y) = (v(frame.origin), v(frame.x_axis) * radii[0], v(frame.y_axis) * radii[1]);
            let at = |t: f64, r: f64| o + (x * t.cos() + y * t.sin()) * r;
            let m = ((t1 - t0) / std::f64::consts::FRAC_PI_2 - 1e-9).ceil().max(1.0) as usize;
            if m > MAX_EDGE_SEGS {
                return Err(bad("ellipse"));
            }
            let d = (t1 - t0) / m as f64;
            (0..m)
                .map(|i| {
                    let a = t0 + d * i as f64;
                    let w = (d / 2.0).cos();
                    let ctrl =
                        [HPoint::weighted(at(a, 1.0), 1.0), HPoint::weighted(at(a + d / 2.0, 1.0 / w), w), HPoint::weighted(at(a + d, 1.0), 1.0)];
                    SpaceBezier::new(&ctrl).map(PathSeg::Bezier).ok_or_else(|| bad("ellipse"))
                })
                .collect::<Result<_>>()?
        }
        brep::Curve::Polyline { points } => {
            let at = |t: f64| -> Option<Vec3> {
                let i = (t.floor().max(0.0) as usize).min(points.len().saturating_sub(2));
                let (a, b) = (v(*points.get(i)?), v(*points.get(i + 1)?));
                Some(a + (b - a) * (t - i as f64))
            };
            let mut ts = vec![t0];
            ts.extend((t0.floor() as i64 + 1..=t1.ceil() as i64 - 1).map(|k| k as f64).filter(|k| *k > t0 && *k < t1));
            ts.push(t1);
            if ts.len() > MAX_EDGE_SEGS {
                return Err(bad("polyline"));
            }
            ts.windows(2).map(|w| Some(PathSeg::Line { a: at(w[0])?, b: at(w[1])? })).collect::<Option<_>>().ok_or_else(|| bad("polyline"))?
        }
        brep::Curve::Spline { knots, points } => {
            let ctrl: Vec<HPoint> = points.iter().map(|q| HPoint::weighted(v(*q), 1.0)).collect();
            spline_spans(knots, &ctrl, t0, t1)?
        }
        brep::Curve::RationalSpline { knots, homogeneous } => {
            let ctrl: Vec<HPoint> = homogeneous.iter().map(|q| HPoint { x: q[0], y: q[1], z: q[2], w: q[3] }).collect();
            spline_spans(knots, &ctrl, t0, t1)?
        }
        brep::Curve::SurfaceIntersection { .. } => {
            return Err(KernelError::Failed("not supported yet: a path along an intersection curve (the record holds no exact curve for it)".into()));
        }
    };
    Ok(out)
}

/// A sketch segment on `plane` as the record curve that is it, on its interval, running the
/// segment's way: a line, a circle (an arc's; clockwise ones on the frame turned over), a cubic
/// or Bézier as a spline of its degree, a conic as a rational quadratic. `None` for a segment
/// with non-finite numbers.
pub fn sketch_record_curve(plane: &Plane, s: &solidcraft_geom::Seg2) -> Option<(brep::Curve, [f64; 2])> {
    use solidcraft_geom::Seg2;
    let w = |p: solidcraft_geom::Vec2| {
        let q = plane.to_world(p);
        [q.x, q.y, q.z]
    };
    let d = |v: Vec3| [v.x, v.y, v.z];
    let bezier = |pts: &[solidcraft_geom::Vec2]| {
        let n = pts.len().checked_sub(1)?;
        let knots = std::iter::repeat_n(0.0, n + 1).chain(std::iter::repeat_n(1.0, n + 1)).collect();
        Some((brep::Curve::Spline { knots, points: pts.iter().map(|p| w(*p)).collect() }, [0.0, 1.0]))
    };
    let out = match *s {
        Seg2::Line { a, b } => (brep::Curve::Line { start: w(a), end: w(b) }, [0.0, 1.0]),
        Seg2::Arc { center, radius, start, sweep } => {
            let (y, t0) = if sweep >= 0.0 { (plane.y, start) } else { (plane.y * -1.0, -start) };
            let frame = brep::Frame { origin: w(center), x_axis: d(plane.x), y_axis: d(y) };
            (brep::Curve::Circle { frame, radius }, [t0, t0 + sweep.abs()])
        }
        Seg2::Cubic { p0, p1, p2, p3 } => bezier(&[p0, p1, p2, p3])?,
        Seg2::Bezier { n, p } => bezier(p.get(..=usize::from(n))?)?,
        Seg2::Conic { a, apex, b, w: wt } => {
            let h = |p: solidcraft_geom::Vec2, k: f64| {
                let q = w(p);
                [q[0] * k, q[1] * k, q[2] * k, k]
            };
            (
                brep::Curve::RationalSpline { knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], homogeneous: vec![h(a, 1.0), h(apex, wt), h(b, 1.0)] },
                [0.0, 1.0],
            )
        }
    };
    let finite = |q: &[f64]| q.iter().all(|x| x.is_finite());
    let ok = match &out.0 {
        brep::Curve::Line { start, end } => finite(start) && finite(end),
        brep::Curve::Circle { frame, radius } => finite(&frame.origin) && radius.is_finite() && *radius > 0.0,
        brep::Curve::Spline { points, .. } => points.iter().all(|q| finite(q)),
        brep::Curve::RationalSpline { homogeneous, .. } => homogeneous.iter().all(|q| finite(q) && q[3] > 0.0),
        _ => false,
    };
    (ok && out.1.iter().all(|t| t.is_finite())).then_some(out)
}

/// The B-spline (homogeneous `ctrl` on `knots`) on `[t0, t1]` as its Bézier spans, one per knot
/// span: each the piece of the spline between two neighbouring knots ([`segment`]), exactly.
fn spline_spans(knots: &[f64], ctrl: &[HPoint], t0: f64, t1: f64) -> Result<Vec<PathSeg>> {
    let bad = || KernelError::Invalid("the edge's B-spline does not evaluate".into());
    let p = knots.len().checked_sub(ctrl.len() + 1).ok_or_else(bad)?;
    if !(1..=MAX_BEZIER_DEGREE).contains(&p) {
        return Err(KernelError::Failed(format!("not supported yet: a path along a B-spline of degree {p}")));
    }
    let mut cuts = vec![t0];
    cuts.extend(knots.iter().copied().filter(|k| *k > t0 && *k < t1));
    cuts.push(t1);
    cuts.dedup();
    if cuts.len() > MAX_EDGE_SEGS {
        return Err(KernelError::Failed("not supported yet: a path along an edge of more than 1000 spans".into()));
    }
    cuts.windows(2)
        .map(|w| {
            let (_, piece) = segment(p, knots, ctrl, w[0], w[1]).map_err(|e| KernelError::Invalid(format!("the edge's B-spline: {e}")))?;
            SpaceBezier::new(&piece).map(PathSeg::Bezier).ok_or_else(bad)
        })
        .collect()
}

/// A rational Bézier span in space (a [`PathSeg::Bezier`]): degree 1…[`MAX_BEZIER_DEGREE`],
/// homogeneous control points `(x·w, y·w, z·w, w)` with positive weights, on 0…1. A drawn 3D
/// spline's spans are cubics of weight one; an edge's NURBS curve and an ellipse are spans of
/// their degree, exactly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpaceBezier {
    n: u8,
    p: [HPoint; MAX_BEZIER_CTRL],
}

/// Pieces a span's length is integrated over (8-point Gauss–Legendre on each).
const LENGTH_PIECES: usize = 16;

/// Nodes and weights of the 8-point Gauss–Legendre rule on [-1, 1].
const GAUSS_8: [(f64, f64); 8] = [
    (-0.960_289_856_497_536_2, 0.101_228_536_290_376_26),
    (-0.796_666_477_413_626_7, 0.222_381_034_453_374_47),
    (-0.525_532_409_916_329, 0.313_706_645_877_887_3),
    (-0.183_434_642_495_649_8, 0.362_683_783_378_362),
    (0.183_434_642_495_649_8, 0.362_683_783_378_362),
    (0.525_532_409_916_329, 0.313_706_645_877_887_3),
    (0.796_666_477_413_626_7, 0.222_381_034_453_374_47),
    (0.960_289_856_497_536_2, 0.101_228_536_290_376_26),
];

impl SpaceBezier {
    /// The span over `ctrl` (2…[`MAX_BEZIER_CTRL`] finite homogeneous points, positive weights).
    pub fn new(ctrl: &[HPoint]) -> Option<SpaceBezier> {
        let n = u8::try_from(ctrl.len().checked_sub(1)?).ok()?;
        if !(1..=MAX_BEZIER_DEGREE as u8).contains(&n) || ctrl.iter().any(|q| !q.is_finite() || !(q.w > 0.0)) {
            return None;
        }
        let mut p = [HPoint::ZERO; MAX_BEZIER_CTRL];
        p.get_mut(..ctrl.len())?.copy_from_slice(ctrl);
        Some(SpaceBezier { n, p })
    }

    /// The cubic over `p` (weights one).
    pub fn cubic(p: [Vec3; 4]) -> Option<SpaceBezier> {
        SpaceBezier::new(&p.map(|q| HPoint::weighted(q, 1.0)))
    }

    pub fn degree(&self) -> usize {
        usize::from(self.n)
    }

    /// The homogeneous control points.
    pub fn ctrl(&self) -> &[HPoint] {
        self.p.get(..=self.degree()).unwrap_or(&[])
    }

    /// The point at `x` (0…1).
    pub fn point(&self, x: f64) -> Option<Vec3> {
        bezier_point(self.ctrl(), x).project()
    }

    pub fn start(&self) -> Vec3 {
        self.ctrl().first().and_then(|q| q.project()).unwrap_or_default()
    }

    pub fn end(&self) -> Vec3 {
        self.ctrl().last().and_then(|q| q.project()).unwrap_or_default()
    }

    /// The derivative at `x`: the quotient rule on the homogeneous curve and its hodograph.
    pub fn derivative(&self, x: f64) -> Option<Vec3> {
        let c = self.ctrl();
        let n = self.degree() as f64;
        let hodograph: Vec<HPoint> = c.windows(2).map(|w| (w[1] - w[0]) * n).collect();
        let (h, d) = (bezier_point(c, x), bezier_point(&hodograph, x));
        (h.w.abs() > f64::MIN_POSITIVE).then(|| (d.xyz() * h.w - h.xyz() * d.w) * (1.0 / (h.w * h.w))).filter(|v| v.is_finite())
    }

    /// The span the other way round.
    pub fn reversed(&self) -> SpaceBezier {
        let mut r = *self;
        if let Some(c) = r.p.get_mut(..=self.degree()) {
            c.reverse();
        }
        r
    }

    /// The span carried by the affine map `f` of points (weights kept: an affine image of a
    /// rational Bézier is the Bézier of the images of its control points).
    pub fn mapped(&self, f: impl Fn(Vec3) -> Vec3) -> Option<SpaceBezier> {
        let ctrl: Vec<HPoint> = self.ctrl().iter().map(|q| q.project().map(|p| HPoint::weighted(f(p), q.w))).collect::<Option<_>>()?;
        SpaceBezier::new(&ctrl)
    }

    /// The control points as SMLib takes them: (x, y, z, weight).
    pub(crate) fn weighted_points(&self) -> Option<Vec<[f64; 4]>> {
        self.ctrl().iter().map(|q| q.project().map(|p| [p.x, p.y, p.z, q.w])).collect()
    }

    fn speed(&self, x: f64) -> Option<f64> {
        self.derivative(x).map(Vec3::len)
    }

    /// The length from `a` to `b` (8-point Gauss–Legendre).
    fn length_between(&self, a: f64, b: f64) -> Option<f64> {
        let (mid, half) = (0.5 * (a + b), 0.5 * (b - a));
        GAUSS_8.iter().try_fold(0.0, |sum, (x, w)| Some(sum + w * half * self.speed(mid + half * x)?))
    }

    /// The length before each of the [`LENGTH_PIECES`] pieces, and the whole length last.
    fn lengths(&self) -> Option<Vec<f64>> {
        let mut out = vec![0.0];
        for i in 0..LENGTH_PIECES {
            let (a, b) = (i as f64 / LENGTH_PIECES as f64, (i + 1) as f64 / LENGTH_PIECES as f64);
            out.push(out.last().copied().unwrap_or(0.0) + self.length_between(a, b)?);
        }
        out.last().is_some_and(|t| t.is_finite()).then_some(out)
    }

    /// Its length (`None` for a span that does not evaluate).
    pub fn length(&self) -> Option<f64> {
        self.lengths()?.last().copied()
    }

    /// The point `f` (0…1) of the way along the span by length, and the unit direction there.
    pub fn along(&self, f: f64) -> Option<(Vec3, Vec3)> {
        let table = self.lengths()?;
        let want = table.last()? * f.clamp(0.0, 1.0);
        let k = table.partition_point(|l| *l <= want).saturating_sub(1).min(LENGTH_PIECES - 1);
        let (a, before) = (k as f64 / LENGTH_PIECES as f64, *table.get(k)?);
        let (mut lo, mut hi) = (a, (k + 1) as f64 / LENGTH_PIECES as f64);
        for _ in 0..60 {
            let x = 0.5 * (lo + hi);
            if before + self.length_between(a, x)? < want {
                lo = x;
            } else {
                hi = x;
            }
        }
        let x = 0.5 * (lo + hi);
        Some((self.point(x)?, self.derivative(x)?.normalized()?))
    }

    /// Points along the span, without its start, every chord within `tol` of it (by the bound
    /// on the homogeneous second differences, as for a polynomial span of weight one; a rational
    /// span's homogeneous bound over its smallest weight).
    pub fn polyline(&self, tol: f64) -> Vec<Vec3> {
        let c = self.ctrl();
        let n = self.degree() as f64;
        let wmin = c.iter().map(|q| q.w).fold(f64::INFINITY, f64::min).max(1e-12);
        let bend = c.windows(3).map(|w| (w[0] - w[1] * 2.0 + w[2]).xyz().len()).fold(0.0, f64::max) * n * (n - 1.0).max(1.0) / wmin;
        let k = ((bend / (8.0 * tol.max(1e-9))).sqrt().ceil() as usize).clamp(1, 512);
        (1..=k).filter_map(|i| self.point(i as f64 / k as f64)).collect()
    }
}
