//! Exact rational sweep of a fitted isoparametric trim segment.
use super::*;
use crate::bspline::{Dir, HPoint, NurbsSurface};

pub(super) fn ruled_patch(skin: &NurbsSurface, [a, b]: [[f64; 2]; 2], delta: Vec3) -> Result<NurbsSurface> {
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

    // v=0 is traversed along increasing u by the wall's exterior wire.
    // Reverse the curve if the active region uses that same direction.
    if a[varying] < b[varying] {
        seam.reverse();
        knots.reverse();
        for knot in &mut knots {
            *knot = hi - (*knot - lo);
        }
    }
    let seam = seam.into_iter().map(|p| [p.x, p.y, p.z, p.w]).collect::<Vec<_>>();
    let controls = solidcore_ops::form_cage::ruled_strip(&seam, delta)
        .map_err(|e| bad(&e.to_string()))?
        .into_iter()
        .map(|[x, y, z, w]| HPoint { x, y, z, w })
        .collect();
    NurbsSurface::from_homogeneous([degree, 1], [knots, vec![0.0, 0.0, 1.0, 1.0]], controls).map_err(|e| bad(&e.to_string()))
}
