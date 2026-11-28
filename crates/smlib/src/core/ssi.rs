//! Surface/surface intersection on solidcore (`smlib_ssi.cpp`'s `smlib_ssi`, `smlib_surface_domain`,
//! `smlib_surface_evaluate`): solidcore-ops' ([`solidcore_ops::surface_intersection`]), on the
//! patches a [`Spec`] describes, its results read as the capi reads them.

use solidcore_ops::surface_intersection as ops;

use super::call::ops_as;
use crate::ssi::{CurveType, Method, Spec, SsiCurve, SsiPoint, SsiResult};
use crate::{Error, Result};

/// A surface as solidcore holds it, over its natural UV domain (`smlib_surface`'s `s` and
/// `domain`).
pub(crate) type Tool = ops::Patch;

/// solidcore-ops' error as the capi's: a status the surface or the intersector answered with as
/// that status, a curve that is no B-spline as [`Error::Unsupported`], a point that is not
/// finite as before; a failure of the kernel's in its own words ([`super::call::error`]).
fn ssi_error(call: &'static str, e: solidcore_ops::Error, detail: Option<String>) -> Error {
    use solidcore_ops::Error as Ops;
    match (e, detail) {
        (Ops::Status(status), None) => Error::Status(status),
        (Ops::Unsupported(ops::CURVE_NOT_BSPLINE), None) => Error::Unsupported,
        (Ops::Refused { reason: ops::POINT_NOT_FINITE, .. }, None) => Error::Solidcore(format!("{call}: {}", ops::POINT_NOT_FINITE)),
        (e, detail) => super::call::error(call, e, detail),
    }
}

/// `spec` as solidcore-ops' description: the same numbers.
pub(crate) fn patch_spec(spec: &Spec) -> ops::PatchSpec {
    match spec.clone() {
        Spec::Plane { origin, xaxis, yaxis, u, v } => ops::PatchSpec::Plane { origin, xaxis, yaxis, u, v },
        Spec::Cylinder { origin, xaxis, yaxis, radius, angle_deg, height } => {
            ops::PatchSpec::Cylinder { origin, xaxis, yaxis, radius, angle_deg, height }
        }
        Spec::Cone { apex, xaxis, yaxis, half_angle_deg, height } => ops::PatchSpec::Cone { apex, xaxis, yaxis, half_angle_deg, height },
        Spec::Sphere { center, xaxis, yaxis, radius, lon_deg, lat_deg } => ops::PatchSpec::Sphere { center, xaxis, yaxis, radius, lon_deg, lat_deg },
        Spec::Torus { center, xaxis, yaxis, major_radius, minor_radius, major_deg, minor_deg } => {
            ops::PatchSpec::Torus { center, xaxis, yaxis, major_radius, minor_radius, major_deg, minor_deg }
        }
        Spec::Nurbs { degree, nu, nv, knots_u, knots_v, points, weights } => {
            ops::PatchSpec::Nurbs { degree, nu, nv, knots_u, knots_v, points, weights }
        }
    }
}

/// The surface of `spec` with its natural UV domain (`construct`).
pub(crate) fn surface(spec: &Spec) -> Result<Tool> {
    let spec = patch_spec(spec);
    ops_as("smlib_surface", &[], ssi_error, || ops::Patch::new(&spec))
}

/// `[u0, u1, v0, v1]`.
pub(crate) fn domain(s: &Tool) -> [f64; 4] {
    s.domain()
}

/// The surface's point at (u, v): `smlib_surface_evaluate`, a non-finite point being `SM_ERR`.
pub(crate) fn evaluate(s: &Tool, u: f64, v: f64) -> Result<[f64; 3]> {
    ops_as("smlib_surface_evaluate", &[], ssi_error, || s.point(u, v))
}

pub(crate) fn curve_type(t: Option<ops::CurveType>) -> CurveType {
    match t {
        None => CurveType::None,
        Some(ops::CurveType::Unknown) => CurveType::Unknown,
        Some(ops::CurveType::Touching) => CurveType::Touching,
        Some(ops::CurveType::Crossing) => CurveType::Crossing,
        Some(ops::CurveType::Tangent) => CurveType::Tangent,
        Some(ops::CurveType::Coincident) => CurveType::Coincident,
        Some(ops::CurveType::NearTangent) => CurveType::NearTangent,
        Some(ops::CurveType::RegionBoundary) => CurveType::RegionBoundary,
    }
}

/// `smlib_ssi`: `a` and `b` within their domains, at approximation tolerance `approx_tol` (<= 0:
/// the kernel's default).
pub(crate) fn intersect(a: &Tool, b: &Tool, method: Method, approx_tol: f64) -> Result<SsiResult> {
    if !approx_tol.is_finite() {
        return Err(Error::Argument);
    }
    let tol = (approx_tol > 0.0).then_some(approx_tol);
    let method = match method {
        Method::Global => ops::Method::Global,
        Method::Advanced => ops::Method::Advanced,
    };
    let found = ops_as("smlib_ssi", &[], ssi_error, || ops::intersect(a, b, method, tol))?;
    Ok(SsiResult {
        curves: found
            .curves
            .into_iter()
            .map(|c| SsiCurve {
                curve_type: curve_type(c.curve_type),
                deviation: c.deviation,
                interval: c.interval,
                curve: c.curve,
                uv_a: c.uv_a,
                uv_b: c.uv_b,
            })
            .collect(),
        points: found.points.into_iter().map(|p| SsiPoint { xyz: p.xyz, uv_a: p.uv_a, uv_b: p.uv_b }).collect(),
    })
}
