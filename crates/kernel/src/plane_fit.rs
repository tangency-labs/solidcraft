//! Planes fitted to points around a loop (Newell's method), for planar faces of polygons read
//! from files and for patches filling a planar loop of edges, and points sampled along record
//! edges (shared with the fill of a loop that is not planar).

use solidcraft_geom::Vec3;
use solidcraft_geom::brep::eval::CurveEval;
use solidcraft_geom::brep::{Brep, Curve, Edge, Surface};

use crate::Result;

/// The plane through points round a loop: its normal by Newell's method (turning with the
/// loop, the plane's natural normal), through their centroid; `None` for fewer than three points
/// or no area.
pub(crate) fn plane_through(pts: &[Vec3]) -> Option<Surface> {
    if pts.len() < 3 {
        return None;
    }
    let z = loop_area(pts).normalized()?;
    let o = pts.iter().fold(Vec3::ZERO, |a, p| a + *p) / pts.len() as f64;
    let a = if z.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let x = (a - z * a.dot(z)).normalized()?;
    let y = z.cross(x);
    Some(Surface::Plane { origin: [o.x, o.y, o.z], u_axis: [x.x, x.y, x.z], v_axis: [y.x, y.y, y.z] })
}

/// The area vector of a closed polygon (Newell's method): normal to the loop, as long as the area
/// it encloses, pointing the way the loop runs round by the right-hand rule.
pub(crate) fn loop_area(points: &[Vec3]) -> Vec3 {
    let mut n = Vec3::ZERO;
    for (i, p) in points.iter().enumerate() {
        let Some(q) = points.get((i + 1) % points.len()) else { continue };
        n += Vec3::new((p.y - q.y) * (p.z + q.z), (p.z - q.z) * (p.x + q.x), (p.x - q.x) * (p.y + q.y));
    }
    n * 0.5
}

/// Points along a record edge, in the direction a loop runs along it, at `s ∈ [0, 1]` of its
/// interval: the record curve's own points (an intersection curve traced as the record holds it).
pub(crate) struct EdgeSampler {
    curve: CurveEval,
    range: (f64, f64),
}

impl EdgeSampler {
    /// The sampler of `edge` run from its start (`forward`) or from its end.
    pub(crate) fn new(edge: &Edge, forward: bool) -> Option<EdgeSampler> {
        let [t0, t1] = edge.interval;
        let curve = CurveEval::new(&edge.curve).ok()?;
        (t0.is_finite() && t1.is_finite() && t0 < t1).then_some(EdgeSampler { curve, range: if forward { (t0, t1) } else { (t1, t0) } })
    }

    pub(crate) fn at(&mut self, s: f64) -> Option<Vec3> {
        let (t0, t1) = self.range;
        let t = if s >= 1.0 { t1 } else { t0 + (t1 - t0) * s };
        self.curve.point(t).filter(|p| p.is_finite())
    }
}

/// The plane of the closed loop `chain` of edges of `b` (each with the way the loop runs along
/// it) when its edges are certainly in one plane, `None` otherwise (also when an edge cannot be
/// sampled). The plane is fitted to points along the edges ([`plane_through`], its natural normal
/// turning with the loop); the edges are certified within `tol` of it from their records: a line
/// by its ends, a circle or ellipse by its frame, a polyline, B-spline or NURBS curve with
/// positive weights by its points (the curve lies in their convex hull), an intersection curve
/// only when one of its two surfaces is a plane that the loop's points lie on (the curve lies in
/// that plane). Samples alone are never trusted: a spline can bulge out of the plane between them.
pub(crate) fn planar_face(b: &Brep, chain: &[(u32, bool)], tol: f64) -> Result<Option<Surface>> {
    const SAMPLES: usize = 16;
    let edges: Vec<&Edge> = match chain.iter().map(|(e, _)| b.edges.get(*e as usize)).collect::<Option<_>>() {
        Some(e) => e,
        None => return Ok(None),
    };
    let mut pts = Vec::with_capacity(chain.len() * SAMPLES);
    for (e, (_, forward)) in edges.iter().zip(chain) {
        let Some(mut sampler) = EdgeSampler::new(e, *forward) else { return Ok(None) };
        for k in 0..SAMPLES {
            let Some(p) = sampler.at(k as f64 / SAMPLES as f64) else { return Ok(None) };
            pts.push(p);
        }
    }
    let Some(plane) = plane_through(&pts) else { return Ok(None) };
    let Some((o, n)) = plane_frame(&plane) else { return Ok(None) };
    let on = |p: Vec3, o: Vec3, n: Vec3| (p - o).dot(n).abs() <= tol;
    let on_plane = |p: Vec3| on(p, o, n);
    if !pts.iter().all(|p| on_plane(*p)) {
        return Ok(None);
    }
    for e in &edges {
        let certified = match &e.curve {
            Curve::Line { start, end } => on_plane(Vec3::from(*start)) && on_plane(Vec3::from(*end)),
            Curve::Circle { frame, .. } | Curve::Ellipse { frame, .. } => {
                let z = Vec3::from(frame.x_axis).cross(Vec3::from(frame.y_axis));
                on_plane(Vec3::from(frame.origin)) && z.cross(n).len() <= 1e-9
            }
            Curve::Polyline { points } | Curve::Spline { points, .. } => points.iter().all(|p| on_plane(Vec3::from(*p))),
            Curve::RationalSpline { homogeneous, .. } => {
                homogeneous.iter().all(|h| h[3] > 0.0 && on_plane(Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3])))
            }
            // A support plane holds the loop when the loop's points lie on it.
            Curve::SurfaceIntersection { surfaces, .. } => {
                surfaces.iter().filter_map(|s| plane_frame(s)).any(|(qo, qn)| pts.iter().all(|p| on(*p, qo, qn)))
            }
        };
        if !certified {
            return Ok(None);
        }
    }
    Ok(Some(plane))
}

/// A plane record's origin and unit normal (`None` for any other surface).
fn plane_frame(s: &Surface) -> Option<(Vec3, Vec3)> {
    match s {
        Surface::Plane { origin, u_axis, v_axis } => Some((Vec3::from(*origin), Vec3::from(*u_axis).cross(Vec3::from(*v_axis)).normalized()?)),
        Surface::PlaneThrough { origin, u_point, v_point } => {
            let o = Vec3::from(*origin);
            Some((o, (Vec3::from(*u_point) - o).cross(Vec3::from(*v_point) - o).normalized()?))
        }
        _ => None,
    }
}
