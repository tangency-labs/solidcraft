//! A record curve's proved distance from a face: the kernel's curve/surface gap
//! ([`solidcore::query::curve_surface_gap`]) on a record spline, so applications name no
//! kernel type for it. Only the conversion and the call: the bound is the kernel's.

use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::topology::{Brep, FaceId};
use solidcore::Kernel;
use solidcore_record::brep::Curve as Record;

/// A record spline or rational spline read as the kernel's curve.
pub struct GapCurve(BSplineCurve);

impl GapCurve {
    /// `curve` as the kernel's curve; `None` for one that is not a spline or rational spline, or
    /// whose knots and points do not make a curve of degree 1 or more.
    #[must_use]
    pub fn new(curve: &Record) -> Option<GapCurve> {
        let (knots, pw): (&[f64], Vec<CPoint>) = match curve {
            Record::Spline { knots, points } => (
                knots,
                points
                    .iter()
                    .map(|&[x, y, z]| CPoint::new(x, y, z, NOW))
                    .collect(),
            ),
            Record::RationalSpline { knots, homogeneous } => (
                knots,
                homogeneous
                    .iter()
                    .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
                    .collect(),
            ),
            _ => return None,
        };
        let degree = knots.len().checked_sub(pw.len() + 1).filter(|d| *d >= 1)?;
        let nurb = NurbCurve::new(pw, degree, knots.to_vec()).ok()?;
        Some(GapCurve(BSplineCurve::from_nurb(nurb, 3, false)))
    }
}

/// A proved upper bound on the distance from `curve` over `interval` to the surface of face `face`
/// of `body`; `None` when the kernel does not enclose the gap. `floor` is the scale below which a
/// tighter bound changes nothing.
///
/// # Errors
/// The kernel's: a dangling face, its unported and cancelled statuses.
pub fn curve_face_gap_bound(
    k: &Kernel,
    body: &Brep,
    face: FaceId,
    curve: &GapCurve,
    interval: [f64; 2],
    floor: f64,
) -> solidcore::Result<Option<f64>> {
    solidcore::query::curve_surface_gap::curve_face_gap_bound(
        k, body, face, &curve.0, interval, floor,
    )
}
