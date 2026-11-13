//! Exact rational sweep of a fitted isoparametric trim segment.
use super::*;
use crate::bspline::{Dir, HPoint, NurbsSurface};

pub(super) fn ruled_patch(skin: &NurbsSurface, [a, b]: [[f64; 2]; 2], delta: Vec3) -> Result<NurbsSurface> {
    let direction = delta.normalized().filter(|_| delta.len() >= 1e-9).ok_or_else(|| bad("trim extrusion requires finite nonzero displacement"))?;
    let varying = match (a[0] == b[0], a[1] == b[1]) {
        (false, true) => 0,
        (true, false) => 1,
        _ => return Err(bad("trim extrusion requires an isoparametric segment")),
    };
    let dir = if varying == 0 { Dir::S } else { Dir::T };
    let [lo, hi] = [a[varying].min(b[varying]), a[varying].max(b[varying])];
    let (mut knots, mut seam) = skin.iso_curve_segment(dir, a[1 - varying], [lo, hi]).map_err(|e| bad(&e.to_string()))?;
    let degree = skin.degree()[varying];
    if knots.get(degree + 1..seam.len()).is_none_or(|interior| interior.chunk_by(|x, y| x == y).any(|k| k.len() >= degree)) {
        return Err(bad("trim extrusion requires a smooth seam spline"));
    }

    // Certify a regular, non-self-intersecting ruled strip by monotone
    // transverse projected controls. Positive rational weights preserve
    // monotonicity for this scalar B-spline coordinate.
    let point = |p: &HPoint| p.project().ok_or_else(|| bad("invalid trim spline weight"));
    let projected = seam.iter().map(point).collect::<Result<Vec<_>>>()?;
    let chord = *projected.last().ok_or_else(|| bad("empty trim spline"))? - *projected.first().ok_or_else(|| bad("empty trim spline"))?;
    let axis = (chord - direction * chord.dot(direction)).normalized().ok_or_else(|| bad("trim extrusion runs along the seam"))?;
    if projected.windows(2).any(|p| (p[1] - p[0]).dot(axis) <= 1e-12) {
        return Err(bad("trim extrusion needs a regular transverse seam"));
    }

    // v=0 is traversed along increasing u by the wall's exterior wire.
    // Reverse the curve if the active region uses that same direction.
    if a[varying] < b[varying] {
        seam.reverse();
        knots.reverse();
        for knot in &mut knots {
            *knot = hi - (*knot - lo);
        }
    }
    let mut controls = seam.clone();
    for p in seam {
        let q = point(&p)? + delta;
        if !q.is_finite() || q.x.abs().max(q.y.abs()).max(q.z.abs()) > 100_000.0 {
            return Err(bad("trim extrusion coordinates must remain within 100000 mm"));
        }
        let xyz = p.xyz() + delta * p.w;
        controls.push(HPoint { x: xyz.x, y: xyz.y, z: xyz.z, w: p.w });
    }
    NurbsSurface::from_homogeneous([degree, 1], [knots, vec![0.0, 0.0, 1.0, 1.0]], controls).map_err(|e| bad(&e.to_string()))
}
