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

use solidcore_record::Vec3;
use solidcore_record::brep;
use solidcraft_geom::brep::eval::SurfaceEval;
use solidcraft_geom::prelude::*;

use crate::body::Body;
use crate::point_path::{MAX_CURVE_POINTS, curve_path};
use crate::{KernelError, Result};

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
    /// The surface parameter that runs along the curve (u when `along_u`, else v), from the
    /// start of `interval` to its end.
    pub span: [f64; 2],
    /// Its points from the start of `interval` to its end, at the chord tolerance asked for.
    pub points: Vec<Vec3>,
}

/// A face's surface and its loops in (u, v), read once for any number of iso lines.
struct FaceTrims<'a> {
    surface: &'a brep::Surface,
    eval: SurfaceEval,
    domain: [[f64; 2]; 2],
    spans: Vec<TrimSpan<'a>>,
}

type Station = (f64, [f64; 2]);

/// A pcurve between successive coordinate turns or knots. A gap between consecutive
/// edge uses (e.g. on a pole) is the straight UV segment closing the loop there.
struct TrimSpan<'a> {
    curve: Option<&'a brep::Curve2>,
    start: Station,
    end: Station,
}

impl TrimSpan<'_> {
    /// A full boundary span also belongs to the closed face. Test the active controls:
    /// a tangent point alone has no length even if two evaluations round to the same value.
    fn lies_on(&self, held: usize, fixed: f64) -> bool {
        if self.start.1[held] != fixed || self.end.1[held] != fixed {
            return false;
        }
        let Some(curve) = self.curve else { return true };
        let (knots, count) = match curve {
            brep::Curve2::Spline { knots, points } => (knots, points.len()),
            brep::Curve2::RationalSpline { knots, homogeneous } => (knots, homogeneous.len()),
        };
        let Some(degree) = knots.len().checked_sub(count + 1) else { return false };
        let end = knots.partition_point(|t| *t <= self.start.0.min(self.end.0));
        let Some(start) = end.checked_sub(degree + 1) else { return false };
        match curve {
            brep::Curve2::Spline { points, .. } => points.get(start..end).is_some_and(|ps| ps.iter().all(|p| p[held] == fixed)),
            brep::Curve2::RationalSpline { homogeneous, .. } => {
                homogeneous.get(start..end).is_some_and(|ps| ps.iter().all(|p| p[held] / p[2] == fixed))
            }
        }
    }

    fn crossing(&self, held: usize, fixed: f64) -> Result<Option<f64>> {
        let ((mut a, p), (mut b, q)) = (self.start, self.end);
        let along = 1 - held;
        // Half-open so a shared vertex is counted once on a crossing and twice or
        // zero times on a touch. Keep multiplicities until inside spans are paired.
        if (p[held] > fixed) == (q[held] > fixed) {
            return Ok(None);
        }
        if p[held] == fixed {
            return Ok(Some(p[along]));
        }
        if q[held] == fixed {
            return Ok(Some(q[along]));
        }
        let Some(curve) = self.curve else {
            let f = (fixed - p[held]) / (q[held] - p[held]);
            return Ok(Some(p[along] + f * (q[along] - p[along])));
        };
        let eval = |t| solidcore_ops::pcurve::eval(curve, t).ok_or_else(|| KernelError::Invalid("iso curve: invalid trim curve".into()));
        for _ in 0..80 {
            let m = 0.5 * a + 0.5 * b;
            if m == a || m == b {
                break;
            }
            let point = eval(m)?;
            if point[held] == fixed {
                return Ok(Some(point[along]));
            }
            if (point[held] > fixed) == (p[held] > fixed) {
                a = m;
            } else {
                b = m;
            }
        }
        Ok(Some(eval(0.5 * a + 0.5 * b)?[along]))
    }
}

impl<'a> FaceTrims<'a> {
    fn new(body: &'a Body, face: usize) -> Result<FaceTrims<'a>> {
        let f = body.record().faces.get(face).ok_or_else(|| KernelError::Invalid(format!("the body has no face {face}")))?;
        let surface = f.parameters();
        let eval = SurfaceEval::new(surface).map_err(|e| KernelError::Failed(format!("iso curve: {e}")))?;
        let mut spans = Vec::new();
        for wire in &f.loops {
            let brep::Loop::Edges { edges } = wire else { continue };
            let (mut first, mut last) = (None, None);
            for edge in edges {
                let curve = &edge.pcurve;
                let mut stations = solidcore_ops::pcurve::monotone_breaks(curve).map_err(|e| KernelError::Failed(format!("iso curve: {e}")))?;
                if !edge.forward {
                    stations.reverse();
                }
                let mut previous = None;
                for t in stations {
                    let uv = solidcore_ops::pcurve::eval(curve, t).ok_or_else(|| KernelError::Invalid("iso curve: invalid trim curve".into()))?;
                    let end = (t, uv);
                    if let Some(start) = previous {
                        spans.push(TrimSpan { curve: Some(curve), start, end });
                    } else if let Some(start) = last.filter(|start| *start != uv) {
                        spans.push(TrimSpan { curve: None, start: (0.0, start), end });
                    }
                    first.get_or_insert(uv);
                    previous = Some(end);
                    last = Some(uv);
                }
            }
            if let Some((p, q)) = last.zip(first).filter(|(p, q)| p != q) {
                spans.push(TrimSpan { curve: None, start: (0.0, p), end: (1.0, q) });
            }
        }
        Ok(FaceTrims { surface, eval, domain: f.domain, spans })
    }

    /// The face's own extent in (u, v), including the trims' interior turns.
    fn extent(&self) -> [[f64; 2]; 2] {
        let mut bounds = [[f64::INFINITY, f64::NEG_INFINITY]; 2];
        for span in &self.spans {
            for q in [span.start.1, span.end.1] {
                for (b, x) in bounds.iter_mut().zip(q) {
                    *b = [b[0].min(x), b[1].max(x)];
                }
            }
        }
        std::array::from_fn(|i| [bounds[i][0].max(self.domain[i][0]), bounds[i][1].min(self.domain[i][1])])
    }

    /// Pair crossings of all loops along the same line, starting outside at minus infinity.
    fn clip(&self, along_u: bool, fixed: f64) -> Result<Vec<(brep::Curve, [f64; 2], [f64; 2])>> {
        let held = usize::from(along_u);
        let [lo, hi] = self.domain[1 - held];
        let along = 1 - held;
        let mut cuts = Vec::new();
        let mut intervals = Vec::new();
        for span in &self.spans {
            if let Some(at) = span.crossing(held, fixed)? {
                cuts.push(at);
            } else if span.lies_on(held, fixed) {
                let (a, b) = (span.start.1[along], span.end.1[along]);
                intervals.push((a.min(b), a.max(b)));
            }
        }
        cuts.sort_by(f64::total_cmp);
        let (pairs, leftover) = cuts.as_chunks::<2>();
        if !leftover.is_empty() {
            return Err(KernelError::Invalid("iso curve: trim loop has an unpaired crossing".into()));
        }
        intervals.extend(pairs.iter().map(|&[a, b]| (a, b)));
        intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut ranges: Vec<(f64, f64)> = Vec::new();
        for (a, b) in intervals {
            let (a, b) = (a.max(lo), b.min(hi));
            if a < b {
                // The two pcurves at one STEP vertex can evaluate its UV a few ulps apart.
                // Join such adjacent pieces after pairing so a double crossing keeps its parity.
                if let Some(last) = ranges.last_mut().filter(|last| a - last.1 <= 8.0 * f64::EPSILON * a.abs().max(last.1.abs())) {
                    last.1 = last.1.max(b);
                } else {
                    ranges.push((a, b));
                }
            }
        }
        ranges
            .into_iter()
            .map(|(a, b)| {
                let (curve, interval) = iso_record(self.surface, along_u, fixed, (a, b))?;
                Ok((curve, interval, [a, b]))
            })
            .collect()
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
    let out: Vec<(brep::Curve, [f64; 2])> = trims.clip(along_u, if along_u { pv } else { pu })?.into_iter().map(|(c, i, _)| (c, i)).collect();
    if out.is_empty() {
        return Err(KernelError::Invalid("the iso curve runs outside the face".into()));
    }
    Ok(out)
}

/// The isoparametric curves of face `face` of `body` for Isocurve Analysis: `count`
/// (1…[`MAX_ISO_COUNT`]) curves each way, evenly spaced across the face, the held parameter at
/// `lo + k · (hi − lo) / (count + 1)` for `k = 1…count` over the face's extent in (u, v), so
/// neither side of the extent (the face's boundary) is drawn as a curve. Faces of the body on the
/// very same surface (a face a boss splits in two) are spaced together, across the extent of all
/// of them, so their curves are the same lines, straight across where they meet. Each curve is
/// cut to the face ([`FaceTrims::clip`]), its points at chord tolerance `chord`. Along u first,
/// then v.
pub fn iso_grid(body: &Body, face: usize, count: usize, chord: f64) -> Result<Vec<IsoLine>> {
    if !(1..=MAX_ISO_COUNT).contains(&count) {
        return Err(KernelError::Invalid(format!("iso curves: 1…{MAX_ISO_COUNT} each way")));
    }
    if !(chord.is_finite() && chord > 0.0) {
        return Err(KernelError::Invalid("iso curves: the chord tolerance must be positive".into()));
    }
    let trims = FaceTrims::new(body, face)?;
    let [[u0, u1], [v0, v1]] = shared_extent(body, face, &trims)?;
    let mut out = Vec::new();
    for along_u in [true, false] {
        let (lo, hi) = if along_u { (v0, v1) } else { (u0, u1) };
        if !(hi > lo) {
            continue;
        }
        for k in 1..=count {
            let fixed = lo + (hi - lo) * k as f64 / (count + 1) as f64;
            for (curve, interval, span) in trims.clip(along_u, fixed)? {
                let segs = curve_path(&curve, interval)?;
                let mut points: Vec<Vec3> = segs.first().map(crate::PathSeg::start).into_iter().collect();
                for s in &segs {
                    points.extend(s.points_after_start(chord));
                }
                out.push(IsoLine { along_u, fixed, curve, interval, span, points });
            }
        }
    }
    Ok(out)
}

/// The extent in (u, v) of face `face` together with every other face of `body` on the same
/// surface record (the same surface and parameters, so one (u, v) for all of them): the box round
/// all their extents ([`FaceTrims::extent`]).
fn shared_extent(body: &Body, face: usize, trims: &FaceTrims) -> Result<[[f64; 2]; 2]> {
    let mut b = trims.extent();
    for (i, other) in body.record().faces.iter().enumerate() {
        if i == face || other.parameters() != trims.surface {
            continue;
        }
        let [[a0, a1], [c0, c1]] = FaceTrims::new(body, i)?.extent();
        b = [[b[0][0].min(a0), b[0][1].max(a1)], [b[1][0].min(c0), b[1][1].max(c1)]];
    }
    Ok(b)
}

/// The iso curve of `surface` at `fixed` (the other parameter) as a record curve on `[a, b]` of
/// its own parameter: the surface's own line or circle, or its B-spline row.
fn iso_record(surface: &brep::Surface, along_u: bool, fixed: f64, (a, b): (f64, f64)) -> Result<(brep::Curve, [f64; 2])> {
    use solidcore_record::brep::{Curve, Frame, Surface};
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
        Surface::Torus { frame: f, major_radius, minor_radius, tube_start } => {
            let (o, x, y, z) = frame(f);
            if along_u {
                let w = fixed + tube_start;
                circle(o + z * (minor_radius * w.sin()), x, y, major_radius + minor_radius * w.cos())
            } else {
                // The tube circle from its start, so its angle is v.
                let dir = x * fixed.cos() + y * fixed.sin();
                let (s, c) = tube_start.sin_cos();
                circle(o + dir * *major_radius, dir * c + z * s, z * c - dir * s, *minor_radius)
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
        // A label: the face's (u, v) are its NURBS form's, whose rows are its iso curves.
        Surface::BallPatch { .. } => return Err(KernelError::Invalid("an iso curve of a ball patch's label (take its NURBS form's)".into())),
        Surface::Blend { .. } => return Err(KernelError::Invalid("an iso curve of a blend's label (take its NURBS form's)".into())),
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
