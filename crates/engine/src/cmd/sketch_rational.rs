//! Sketch intervals of weighted splines that have no exact Bézier or conic representation.

use solidcore_record::{Vec2, brep::Curve};
use solidcraft_geom::bspline::{HPoint, segment};
use solidcraft_sketch::{CurveKind, Nurbs, Sketch};

use crate::Result;
use crate::params::bad;

/// Use a unit knot domain so a native query and a sketch interval use the same parameter.
pub(super) fn record(sk: &Sketch, ci: usize) -> Option<Curve> {
    let CurveKind::Spline { pts, control: true, degree, periodic: false, nurbs: Some(nurbs), .. } = &sk.curves.get(ci)?.kind else { return None };
    let points = pts.iter().map(|p| sk.point(*p)).collect::<Option<Vec<_>>>()?;
    nurbs.rational_record(&points, *degree)
}

/// Keep the actual boundary parameter, including one very near an interior knot.
fn subcurve(curve: &Curve, a: f64, b: f64, cmd: &str) -> Result<Curve> {
    let Curve::RationalSpline { knots, homogeneous } = curve else { return Err(bad(cmd, "expected a rational sketch spline")) };
    let degree = knots.len().checked_sub(homogeneous.len() + 1).ok_or_else(|| bad(cmd, "invalid spline degree"))?;
    let controls: Vec<_> = homogeneous.iter().map(|&[x, y, z, w]| HPoint { x, y, z, w }).collect();
    let (knots, points) = segment(degree, knots, &controls, a, b).map_err(|e| bad(cmd, e.to_string()))?;
    Ok(Curve::RationalSpline { knots, homogeneous: points.iter().map(|p| [p.x, p.y, p.z, p.w]).collect() })
}

/// A wrapped piece of a closed spline consists of the tail joined to the head at the old seam.
fn interval(curve: &Curve, [a, b]: [f64; 2], cmd: &str) -> Result<(Vec<Vec2>, u8, Nurbs)> {
    let piece = if b <= 1.0 {
        subcurve(curve, a, b, cmd)?
    } else if a == 1.0 {
        subcurve(curve, 0.0, b - 1.0, cmd)?
    } else {
        let tail = subcurve(curve, a, 1.0, cmd)?;
        let head = subcurve(curve, 0.0, b - 1.0, cmd)?;
        solidcore_ops::curve::join_curves(&tail, &head, 1e-7).map_err(|e| bad(cmd, e.to_string()))?
    };
    let Curve::RationalSpline { knots, homogeneous } = piece else { return Err(bad(cmd, "expected a rational sketch spline interval")) };
    let degree =
        knots.len().checked_sub(homogeneous.len() + 1).and_then(|d| u8::try_from(d).ok()).ok_or_else(|| bad(cmd, "invalid spline degree"))?;
    let (points, weights): (Vec<_>, Vec<_>) = homogeneous.into_iter().map(|[x, y, _, w]| (Vec2::new(x / w, y / w), w)).unzip();
    let nurbs = Nurbs { knots, weights };
    nurbs.check(points.len(), degree)?;
    Ok((points, degree, nurbs))
}

pub(super) fn preview(curve: &Curve, range: [f64; 2], cmd: &str) -> Result<Vec<Vec2>> {
    let (points, degree, nurbs) = interval(curve, range, cmd)?;
    Ok(nurbs.polyline(&points, degree))
}

pub(super) fn add_interval(
    sk: &mut Sketch,
    curve: &Curve,
    range: [f64; 2],
    start: Option<usize>,
    end: Option<usize>,
    construction: bool,
    cmd: &str,
) -> Result<usize> {
    let (points, degree, nurbs) = interval(curve, range, cmd)?;
    let mut pts = Vec::with_capacity(points.len());
    for (i, &p) in points.iter().enumerate() {
        let shared = if i == 0 {
            start
        } else if i + 1 == points.len() {
            end
        } else {
            None
        };
        pts.push(match shared {
            Some(point) => point,
            None => sk.add_point(p, None)?,
        });
    }
    let kind = CurveKind::Spline { pts, control: true, degree, periodic: false, nurbs: Some(nurbs), handles: Vec::new() };
    let ci = sk.add_curve(kind, None)?;
    if let Some(curve) = sk.curves.get_mut(ci) {
        curve.construction = construction;
    }
    Ok(ci)
}
