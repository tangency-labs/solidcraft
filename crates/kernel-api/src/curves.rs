//! Curves and fitting ([`Curves`]): free curves queried (nearest a point or a ray, crossings, a
//! plane's section, an arc through three points, a joined pair, an imported cubic's standing),
//! fit and control splines checked and fitted, pcurves evaluated and two point lists matched. The
//! values are the record's (`solidcore_record::brep`, `solidcore_record::curve_query`,
//! `solidcore_record::projection`), so the application holds no kernel type.

use solidcore_record::brep::{Curve, Curve2};
use solidcore_record::curve_query::{Crossing, CubicPiece, CurvePoint, CurveRayPoint, FitSplineBounds, PlaneSection};
use solidcore_record::projection::PlanarSpline;
use solidcore_record::{Plane, Vec2};

use crate::Backend;

/// A curve query or fit the kernel refuses, in its words: input not finite or past the
/// coordinate bound, a curve the record's constructors refuse, a spline it cannot fit.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct CurveRefused(pub String);

/// The curves and fitting of one kernel ([`crate::OpKind::Curves`]).
pub trait Curves: Backend {
    /// Where each piece (a curve over its interval) crosses `plane`, in the plane's coordinates.
    fn piece_plane_crossings(&self, pieces: &[(&Curve, [f64; 2])], plane: &Plane) -> Result<Vec<Vec2>, CurveRefused>;
    /// Where `curve` over `interval` meets `plane`: lying in it, or its isolated points and parts.
    fn plane_section(&self, curve: &Curve, interval: [f64; 2], plane: &Plane) -> Result<PlaneSection, CurveRefused>;
    /// The point of `curve` nearest `p`, within `tolerance`; `None` where the solve finds none.
    fn closest_point(&self, curve: &Curve, p: [f64; 3], tolerance: f64) -> Result<Option<CurvePoint>, CurveRefused>;
    /// The isolated points where `curve` over `interval` crosses `plane`.
    fn plane_crossings(&self, curve: &Curve, interval: [f64; 2], plane: &Plane) -> Result<Vec<Vec2>, CurveRefused>;
    /// Where `a` and `b` cross within `tolerance`, in space or as seen `along` a direction.
    fn crossings(&self, a: &Curve, b: &Curve, along: Option<[f64; 3]>, tolerance: f64) -> Result<Vec<Crossing>, CurveRefused>;
    /// `a` followed by `b` as one curve, their meeting ends within `tolerance`.
    fn join_curves(&self, a: &Curve, b: &Curve, tolerance: f64) -> Result<Curve, CurveRefused>;
    /// The cubic Bézier through `points` as pieces a model keeps, each with its standing.
    fn cubic_pieces(&self, points: [[f64; 3]; 4]) -> Result<Vec<CubicPiece>, CurveRefused>;
    /// The arc from `points[0]` through `points[1]` to `points[2]`.
    fn arc_through(&self, points: [[f64; 3]; 3]) -> Result<Curve, CurveRefused>;
    /// The point of `curve` nearest the ray from `origin` along `direction`, within `tolerance`.
    fn closest_to_ray(&self, curve: &Curve, origin: [f64; 3], direction: [f64; 3], tolerance: f64) -> Result<Option<CurveRayPoint>, CurveRefused>;
    /// What a fit spline of cubic `pieces` certifies against the points `near` its fit points.
    fn fit_spline_bounds(&self, pieces: &[[[f64; 3]; 4]], near: &[(usize, [f64; 3])]) -> Result<FitSplineBounds, CurveRefused>;
    /// Splines through `p` (closed when `periodic`), each at most `limit` points.
    fn fit_splines(&self, p: &[Vec2], periodic: bool, limit: usize) -> Result<Vec<PlanarSpline>, CurveRefused>;
    /// Whether `n` control points of `degree` with explicit clamped `knots` and positive `weights`
    /// make a spline, at most `limit` points.
    fn check_control(&self, n: usize, degree: usize, knots: &[f64], weights: &[f64], limit: usize) -> Result<(), CurveRefused>;
    /// Control points with their weights as homogeneous points, normalised by the largest weight
    /// without changing the curve.
    fn homogeneous_controls(&self, p: &[Vec2], weights: &[f64]) -> Result<Vec<[f64; 4]>, CurveRefused>;
    /// A spline's knots and weights for its reversed curve, its knot spacings kept exactly.
    fn reversed_metadata(&self, knots: &[f64], weights: &[f64]) -> (Vec<f64>, Vec<f64>);
    /// The point or certified complete line a control spline's image is, to `roundoff`; `None`
    /// for a planar curve.
    fn linear_image(&self, p: &[Vec2], degree: usize, knots: &[f64], weights: &[f64], roundoff: f64) -> Result<Option<[Vec2; 2]>, CurveRefused>;
    /// Whether `p` closes, with distinct samples and regular chords, as a cyclic fit needs.
    fn check_periodic(&self, p: &[Vec2]) -> Result<(), CurveRefused>;
    /// The cubic Bézier pieces of the spline through `p` (closed when `periodic`).
    fn fit_beziers(&self, p: &[Vec2], periodic: bool) -> Result<Vec<[Vec2; 4]>, CurveRefused>;
    /// The natural fit's Hermite pieces through `p`, keeping its chord-length parameters.
    fn natural_spans(&self, p: &[Vec2]) -> Result<Vec<(Vec2, Vec2, Vec2, Vec2)>, CurveRefused>;
    /// Whether `new` are `old` in order, each within `within`, spending at most `work` steps. False
    /// includes an ambiguous match and exhausted work; it never means the two are equal.
    fn ordered_nearby_points(&self, old: &[Vec2], new: &[Vec2], within: f64, work: usize) -> Result<bool, CurveRefused>;
    /// The point of pcurve `c` at `t`; `None` outside its interval or for a malformed record.
    fn pcurve_eval(&self, c: &Curve2, t: f64) -> Option<[f64; 2]>;
}
