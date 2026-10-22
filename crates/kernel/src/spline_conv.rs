//! geom's B-spline curves and surfaces as schema 4 records, weights kept (homogeneous control
//! points carried over as they are). A whole tensor surface as a one-face sheet is
//! `surfaces::tensor_sheet`.

use solidcraft_geom::brep::{Curve, Surface};
use solidcraft_geom::bspline::{HPoint, NurbsSurface};

/// A homogeneous control point as the record's `[x*w, y*w, z*w, w]`.
pub(crate) fn h4(h: HPoint) -> [f64; 4] {
    [h.x, h.y, h.z, h.w]
}

/// A NURBS curve of knots and homogeneous control points.
pub(crate) fn nurbs_curve(knots: &[f64], ctrl: &[HPoint]) -> Curve {
    Curve::RationalSpline { knots: knots.to_vec(), homogeneous: ctrl.iter().copied().map(h4).collect() }
}

/// A NURBS surface of knots and homogeneous control points (`rows[i][j]`: i along u).
pub(crate) fn nurbs_surface(u_knots: &[f64], v_knots: &[f64], rows: Vec<Vec<[f64; 4]>>) -> Surface {
    Surface::RationalSpline { u_knots: u_knots.to_vec(), v_knots: v_knots.to_vec(), homogeneous: rows }
}

/// geom's tensor surface as a record surface.
pub(crate) fn surface(s: &NurbsSurface) -> Surface {
    let [nu, nv] = s.count();
    let [ku, kv] = s.knots();
    nurbs_surface(ku, kv, (0..nu).map(|i| (0..nv).map(|j| h4(s.point(i, j))).collect()).collect())
}
