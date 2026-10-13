//! Curve geometry: lines, conics, B-splines, offset/projected/iso/composite curves,
//! as a concrete `enum` with evaluation, derivatives, closest point and length.
//!
//! Upstream: `SmCurve`, `SmLine`, `SmCircle`, `SmEllipse`, `SmParabola`,
//! `SmHyperbola`, `SmBSplineCurve`, `SmOffsetCurve`, `SmCompositeCurve`, `SmIsoCurve`,
//! `SmProjectedCurve`, `SmCurveClass` in `source/SMLib`, and `SM_API/SmApiCurves`.

pub mod conic;
pub mod crv_in_volume;
pub mod crv_on_surf;
#[allow(clippy::module_inception)]
pub mod curve;
pub mod curve_cache;
pub mod ellipse;
pub mod hermite_curve;
pub mod hyperbola;
pub mod iso_curve;
pub mod line;
pub mod offset_curve;
pub mod parabola;
pub mod projected_curve;
pub mod tangent_field;
