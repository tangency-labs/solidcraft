//! Surface/surface intersection of two surface patches given in plain numbers.
//!
//! A patch ([`Patch`]) is a plane, a cylinder, a cone, a sphere, a torus or a NURBS net,
//! described by a [`PatchSpec`] (angles in degrees) and made into the kernel's surface over its
//! natural UV domain ([`Patch::domain`]): (u, v) itself for a plane, the parameterisation of the
//! kernel's NURBS form for the other analytic kinds. [`intersect`] runs one of the kernel's two
//! intersectors ([`Method`]) on two patches and gives each curve as the record's B-spline
//! ([`IntersectionCurve`]), with its curves in each patch's UV where the intersector returned
//! them, and the points where a touching curve starts ([`TouchPoint`]).
//!
//! The kernel makes the surfaces, intersects and reads the curves
//! ([`solidcore::intersect::patch`]); ops checks the inputs, runs them on the session and
//! converts the curves into the record's.
//!
//! ```
//! use solidcore_ops::surface_intersection::{intersect, Method, Patch, PatchSpec};
//!
//! let (x, y, z) = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
//! let plane = PatchSpec::Plane { origin: [0.0; 3], xaxis: x, yaxis: y, u: [-2.0, 2.0], v: [-2.0, 2.0] };
//! let ball = PatchSpec::Sphere {
//!     center: [0.0; 3],
//!     xaxis: x,
//!     yaxis: z,
//!     radius: 1.0,
//!     lon_deg: [0.0, 360.0],
//!     lat_deg: [-90.0, 90.0],
//! };
//! let (plane, ball) = (Patch::new(&plane)?, Patch::new(&ball)?);
//! assert_eq!(plane.domain(), [-2.0, 2.0, -2.0, 2.0]);
//! // a plane through the centre meets the sphere in its equator
//! let found = intersect(&plane, &ball, Method::Global, Some(1e-6))?;
//! assert!(!found.curves.is_empty());
//! # Ok::<(), solidcore_ops::Error>(())
//! ```

use solidcore::intersect::patch as kpatch;
use solidcore::surface::surface::p2::TsectCurveType;
use solidcore_record::brep::{Curve, Curve2};

use crate::error::INVALID_INPUT;
use crate::session::{run, Answer};
use crate::{Error, Result};

pub use solidcore::intersect::patch::{Method, PatchSpec, TouchPoint};

/// The reason of the [`Error::Refused`] [`Patch::point`] gives for a point that is not finite.
pub const POINT_NOT_FINITE: &str = "not a finite point";

/// What [`Error::Unsupported`] names for an intersection curve that is not a B-spline.
pub const CURVE_NOT_BSPLINE: &str = "an intersection curve that is not a B-spline";

/// A surface patch: the kernel's surface of a [`PatchSpec`] over its natural UV domain.
#[derive(Clone, Debug)]
pub struct Patch(kpatch::Patch);

impl Answer for kpatch::Patch {}
impl Answer for kpatch::PatchIntersection {}
impl Answer for solidcore::surface::surface::p2::SurfaceIntersection {}

fn finite(v: &[f64]) -> bool {
    v.iter().all(|x| x.is_finite())
}

/// The checks ops makes of a description before the kernel sees it: every number finite, and a
/// NURBS net of degree at least 1 each way with more points than its degree each way, `nu * nv`
/// points, a weight per point when it has weights, and `n + p + 1` knots each way.
fn checked(spec: &PatchSpec) -> bool {
    match spec {
        PatchSpec::Plane {
            origin,
            xaxis,
            yaxis,
            u,
            v,
        } => finite(origin) && finite(xaxis) && finite(yaxis) && finite(u) && finite(v),
        PatchSpec::Cylinder {
            origin,
            xaxis,
            yaxis,
            radius,
            angle_deg,
            height,
        } => {
            finite(origin)
                && finite(xaxis)
                && finite(yaxis)
                && radius.is_finite()
                && finite(angle_deg)
                && finite(height)
        }
        PatchSpec::Cone {
            apex,
            xaxis,
            yaxis,
            half_angle_deg,
            height,
        } => {
            finite(apex)
                && finite(xaxis)
                && finite(yaxis)
                && half_angle_deg.is_finite()
                && finite(height)
        }
        PatchSpec::Sphere {
            center,
            xaxis,
            yaxis,
            radius,
            lon_deg,
            lat_deg,
        } => {
            finite(center)
                && finite(xaxis)
                && finite(yaxis)
                && radius.is_finite()
                && finite(lon_deg)
                && finite(lat_deg)
        }
        PatchSpec::Torus {
            center,
            xaxis,
            yaxis,
            major_radius,
            minor_radius,
            major_deg,
            minor_deg,
        } => {
            finite(center)
                && finite(xaxis)
                && finite(yaxis)
                && major_radius.is_finite()
                && minor_radius.is_finite()
                && finite(major_deg)
                && finite(minor_deg)
        }
        PatchSpec::Nurbs {
            degree: [p, q],
            nu,
            nv,
            knots_u,
            knots_v,
            points,
            weights,
        } => {
            *p >= 1
                && *q >= 1
                && nu > p
                && nv > q
                && nu.checked_mul(*nv) == Some(points.len())
                && nu.checked_add(p + 1) == Some(knots_u.len())
                && nv.checked_add(q + 1) == Some(knots_v.len())
                && weights
                    .as_ref()
                    .is_none_or(|w| w.len() == points.len() && finite(w))
                && finite(knots_u)
                && finite(knots_v)
                && points.iter().all(|c| finite(c))
        }
    }
}

impl Patch {
    /// The surface `spec` describes, over its natural UV domain.
    ///
    /// # Errors
    /// [`Error::Argument`] for a description that fails [`PatchSpec`]'s checks (a number not
    /// finite; a NURBS net whose degrees, point, weight and knot counts do not agree);
    /// [`Error::Status`] [`INVALID_INPUT`] for a surface whose natural domain is not finite or is
    /// empty; the kernel's error making the surface.
    pub fn new(spec: &PatchSpec) -> Result<Patch> {
        if !checked(spec) {
            return Err(Error::Argument);
        }
        let made = run("surface_intersection::Patch::new", |k| {
            Ok(kpatch::Patch::new(k, spec)?.ok_or(Error::Status(INVALID_INPUT)))
        })??;
        Ok(Patch(made))
    }

    /// The UV domain the intersection runs within, as `[u0, u1, v0, v1]`.
    #[must_use]
    pub fn domain(&self) -> [f64; 4] {
        self.0.uv_box()
    }

    /// The surface's point at (u, v).
    ///
    /// # Errors
    /// [`Error::Argument`] for u or v not finite; [`Error::Refused`] with [`POINT_NOT_FINITE`]
    /// for a point that is not finite; the kernel's error evaluating it.
    pub fn point(&self, u: f64, v: f64) -> Result<[f64; 3]> {
        if !u.is_finite() || !v.is_finite() {
            return Err(Error::Argument);
        }
        let p = run("surface_intersection::Patch::point", |k| {
            self.0.point(k, u, v)
        })?;
        if p.iter().all(|x| x.is_finite()) {
            Ok(p)
        } else {
            Err(Error::Refused {
                operation: "surface_intersection::Patch::point",
                reason: POINT_NOT_FINITE,
            })
        }
    }
}

/// The kind of an intersection curve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveType {
    /// The intersector could not tell.
    Unknown,
    /// Degenerate: one point where the surfaces touch.
    Touching,
    /// Transversal.
    Crossing,
    /// The surfaces are tangent along it.
    Tangent,
    /// Where the surfaces butt up against each other.
    Coincident,
    /// A small angle of intersection.
    NearTangent,
    /// Bounds a region where the surfaces coincide.
    RegionBoundary,
}

fn curve_type(t: TsectCurveType) -> CurveType {
    match t {
        TsectCurveType::Unknown => CurveType::Unknown,
        TsectCurveType::Touching => CurveType::Touching,
        TsectCurveType::Crossing => CurveType::Crossing,
        TsectCurveType::Tangent => CurveType::Tangent,
        TsectCurveType::Coincident => CurveType::Coincident,
        TsectCurveType::NearTangent => CurveType::NearTangent,
        TsectCurveType::RegionBoundary => CurveType::RegionBoundary,
    }
}

/// One intersection curve: its 3D curve and, where the intersector returned them, its curves in
/// each patch's UV, each the (rational) B-spline the kernel holds (its lines, circles and
/// ellipses are B-splines too; a traced curve one fitted within the approximation tolerance).
/// A curve is polynomial where every weight is 1, else rational, its homogeneous points the
/// kernel's cartesian points times their weights.
#[derive(Clone, Debug)]
pub struct IntersectionCurve {
    /// Its kind, where the intersector reported one.
    pub curve_type: Option<CurveType>,
    /// The intersector's deviation estimate (NaN: none).
    pub deviation: f64,
    /// The 3D curve's natural parameter interval.
    pub interval: [f64; 2],
    /// The 3D curve.
    pub curve: Curve,
    /// On the first patch, in its UV.
    pub uv_a: Option<Curve2>,
    /// On the second patch, in its UV.
    pub uv_b: Option<Curve2>,
}

/// What an intersection found.
#[derive(Clone, Debug, Default)]
pub struct Intersection {
    /// The intersection curves, in the intersector's order.
    pub curves: Vec<IntersectionCurve>,
    /// Where each touching curve starts, in the same order.
    pub points: Vec<TouchPoint>,
}

/// The 3D B-spline of `c`.
fn curve3(c: kpatch::CartesianNurbs) -> Curve {
    if c.is_polynomial() {
        Curve::Spline {
            points: c.points3(),
            knots: c.knots,
        }
    } else {
        Curve::RationalSpline {
            homogeneous: c.homogeneous3(),
            knots: c.knots,
        }
    }
}

/// The UV B-spline of `c`, as [`curve3`].
fn curve2(c: kpatch::CartesianNurbs) -> Curve2 {
    if c.is_polynomial() {
        Curve2::Spline {
            points: c.points2(),
            knots: c.knots,
        }
    } else {
        Curve2::RationalSpline {
            homogeneous: c.homogeneous2(),
            knots: c.knots,
        }
    }
}

/// `a` and `b` intersected within their domains by `method`, at the approximation tolerance
/// `tolerance` (`None`: the kernel's default, 1/1000 of the surfaces' size).
///
/// # Errors
/// [`Error::Argument`] for a tolerance not finite and positive; [`Error::Status`] with the
/// intersector's status where its failure stands for one; [`Error::Unsupported`] with
/// [`CURVE_NOT_BSPLINE`] for a 3D curve the kernel does not hold as a B-spline; the kernel's
/// error otherwise.
pub fn intersect(
    a: &Patch,
    b: &Patch,
    method: Method,
    tolerance: Option<f64>,
) -> Result<Intersection> {
    if tolerance.is_some_and(|t| !(t.is_finite() && t > 0.0)) {
        return Err(Error::Argument);
    }
    let read = run("surface_intersection::intersect", |k| {
        // a status is the intersector's answer; any other failure is the kernel's error
        let found = match kpatch::intersect(k, &a.0, &b.0, method, tolerance) {
            Ok(found) => found,
            Err(e) => {
                return match e.sm_status() {
                    Some(status) => Ok(Err(Error::Status(status))),
                    None => Err(e),
                }
            }
        };
        Ok(kpatch::read(k, &found)?.ok_or(Error::Unsupported(CURVE_NOT_BSPLINE)))
    })??;
    Ok(Intersection {
        curves: read
            .curves
            .into_iter()
            .map(|c| IntersectionCurve {
                curve_type: c.curve_type.map(curve_type),
                deviation: c.deviation,
                interval: c.interval,
                curve: curve3(c.curve),
                uv_a: c.uv_a.map(curve2),
                uv_b: c.uv_b.map(curve2),
            })
            .collect(),
        points: read.points,
    })
}
