//! What the Rust engines of `--ssi-corpus` share: a corpus surface built with
//! solidcore's own types (`rust_surface`), and an intersection in the port's form
//! written out sampled the way the C++ runner samples it (`write_intersection`). The
//! port (`rust`, needs the `cpp` feature for its parity checks) and `solidcore-ssi` (the
//! `solidcore-ssi` crate on the same surfaces, no C++) both use them.

use solidcore::curve::curve::Curve;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::CPoint;
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::cone::Cone;
use solidcore::surface::plane::Plane;
use solidcore::surface::sphere::{Sphere, SphereOptions};
use solidcore::surface::surface::p2::SurfaceIntersection;
use solidcore::surface::surface::Surface;
use solidcore::surface::torus::{Torus, TorusOptions};
use solidcore::SolidError;

use super::case::{SsiCase, SurfaceSpec};
use super::geom::P3;
use super::raw::{type_name, RawCurve, RawOutput, N_SAMPLES};
use super::EngineFailure;
use crate::Deferred;

fn deferred(unit: &str, upstream: &str) -> Deferred {
    Deferred {
        unit: unit.into(),
        symbol: upstream.into(),
    }
}

pub(crate) fn v3(p: P3) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// The output of a case deferred for `engine` until `unit` is ported.
pub(crate) fn deferred_output(
    case: &SsiCase,
    tol: Option<f64>,
    engine: &str,
    unit: &str,
    symbol: &str,
) -> RawOutput {
    RawOutput {
        case: case.id.clone(),
        engine: engine.into(),
        status: 0,
        approx_tol: tol,
        deferred: Some(Deferred {
            unit: unit.into(),
            symbol: symbol.into(),
        }),
        ..RawOutput::default()
    }
}

/// The result of making an analytic surface's NURB in place, as the C++ runner's
/// `GetNaturalUVDomain()` does (`GetOrCreateGwNurbPointer` through a const cast): an
/// unported step defers the case.
fn nurb_made(r: Result<(), SolidError>, what: &str) -> Result<Result<(), Deferred>, String> {
    match r {
        Ok(()) => Ok(Ok(())),
        Err(SolidError::Unported { upstream, unit }) => Ok(Err(Deferred {
            unit: unit.into(),
            symbol: upstream.into(),
        })),
        Err(e) => Err(format!("{what}: {e}")),
    }
}

/// The runner's `unit_cross` (same operation order): the unit vector along `a x b`.
fn unit_cross(a: Vec3, b: Vec3) -> Vec3 {
    let c = [
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    ];
    let l = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
    Vec3::new(c[0] / l, c[1] / l, c[2] / l)
}

/// Builds a corpus surface with solidcore's types, or names the unit that must land first.
pub(crate) fn rust_surface(
    k: &Kernel,
    spec: &SurfaceSpec,
) -> Result<Result<Surface, Deferred>, String> {
    match spec {
        SurfaceSpec::Plane {
            origin,
            xaxis,
            yaxis,
            u,
            v,
        } => {
            let dom = Extent2d::from_bounds(u[0], v[0], u[1], v[1]);
            let plane = Plane::new(
                v3(*origin),
                v3(*xaxis),
                v3(*yaxis),
                Vec2::new(1.0, 1.0),
                &dom,
            );
            // The C++ runner's `GetNaturalUVDomain()` makes the plane's NURB in place
            // (`GetOrCreateGwNurbPointer` through a const cast) and keeps it; the port's
            // query does the same, so ask it here to start GlobalSurfaceIntersect from the
            // same state (`IntersectWithPlane`'s parallel branch tests for the NURB).
            let _ = plane.natural_uv_domain();
            Ok(Ok(Surface::from(plane)))
        }
        SurfaceSpec::Nurbs {
            degree,
            knots_u,
            knots_v,
            nu,
            nv,
            points,
            weights,
        } => {
            if *nu == 0 || *nv == 0 || points.len() != nu * nv || weights.len() != nu * nv {
                return Err("nurbs: bad sizes".into());
            }
            if knots_u.len() != nu + degree[0] + 1 || knots_v.len() != nv + degree[1] + 1 {
                return Err("nurbs: bad knots".into());
            }
            // NLib control points are homogeneous: Cartesian = (x/w, y/w, z/w)
            let net: Vec<Vec<CPoint>> = points
                .chunks(*nv)
                .zip(weights.chunks(*nv))
                .map(|(row, w)| {
                    row.iter()
                        .zip(w)
                        .map(|(p, &w)| CPoint::new(p[0] * w, p[1] * w, p[2] * w, w))
                        .collect()
                })
                .collect();
            let sur = NurbSurface::new(net, degree[0], degree[1], knots_u.clone(), knots_v.clone())
                .map_err(|e| format!("nurbs: {e}"))?;
            Ok(Ok(Surface::from(BSplineSurface::from_nurb(sur, false))))
        }
        SurfaceSpec::Cylinder {
            origin,
            xaxis,
            yaxis,
            radius,
            angle,
            height,
        } => {
            let z = unit_cross(v3(*xaxis), v3(*yaxis));
            let o = v3(*origin) + z * height[0];
            let mut cyl = Cone::new_cylinder_bounded(
                o,
                v3(*xaxis),
                v3(*yaxis),
                *radius,
                angle[0],
                angle[1],
                height[1] - height[0],
                false,
                false,
                false,
            );
            Ok(nurb_made(cyl.make_nurb(k), "cylinder")?.map(|()| Surface::from(cyl)))
        }
        SurfaceSpec::Cone {
            apex,
            xaxis,
            yaxis,
            half_angle_deg,
            height,
        } => {
            // apex-based description: radius(h) = h * tan(half_angle), h in [h0, h1]
            // along z, full 360 degree rotation
            let z = unit_cross(v3(*xaxis), v3(*yaxis));
            let t = (half_angle_deg * std::f64::consts::PI / 180.0).tan();
            let o = v3(*apex) + z * height[0];
            let pl = Axis2Placement::new(o, v3(*xaxis), v3(*yaxis));
            match Cone::create_canonical(
                k,
                &pl,
                height[0] * t,
                height[1] * t,
                height[1] - height[0],
                false,
                false,
            ) {
                Ok(cone) => Ok(Ok(Surface::from(cone))),
                Err(SolidError::Unported { upstream, unit }) => Ok(Err(Deferred {
                    unit: unit.into(),
                    symbol: upstream.into(),
                })),
                Err(e) => Err(format!("cone: {e}")),
            }
        }
        SurfaceSpec::Sphere {
            center,
            xaxis,
            yaxis,
            radius,
            lon,
            lat,
        } => {
            let anal = Extent2d::from_bounds(lon[0], lat[0], lon[1], lat[1]);
            let mut sphere = match Sphere::new(
                k,
                v3(*center),
                v3(*xaxis),
                v3(*yaxis),
                anal,
                *radius,
                SphereOptions::default(),
            ) {
                Ok(s) => s,
                Err(SolidError::Unported { upstream, unit }) => {
                    return Ok(Err(Deferred {
                        unit: unit.into(),
                        symbol: upstream.into(),
                    }))
                }
                Err(e) => return Err(format!("sphere: {e}")),
            };
            Ok(nurb_made(sphere.make_nurb(k), "sphere")?.map(|()| Surface::from(sphere)))
        }
        SurfaceSpec::Torus {
            center,
            xaxis,
            yaxis,
            major_radius,
            minor_radius,
            major,
            minor,
        } => {
            let anal = Extent2d::from_bounds(major[0], minor[0], major[1], minor[1]);
            let mut torus = match Torus::new(
                k,
                v3(*center),
                v3(*xaxis),
                v3(*yaxis),
                anal,
                *major_radius,
                *minor_radius,
                TorusOptions::default(),
            ) {
                Ok(s) => s,
                Err(SolidError::Unported { upstream, unit }) => {
                    return Ok(Err(Deferred {
                        unit: unit.into(),
                        symbol: upstream.into(),
                    }))
                }
                Err(e) => return Err(format!("torus: {e}")),
            };
            Ok(nurb_made(torus.make_nurb(k), "torus")?.map(|()| Surface::from(torus)))
        }
    }
}

/// Samples a curve as the runner's `sample_curve` (failed evaluations skipped); an
/// unported evaluation defers the case.
pub(crate) fn sample(k: &Kernel, c: &Curve, n: i32) -> Result<(Vec<Vec3>, [f64; 2]), SolidError> {
    let ivl = c.natural_interval();
    let (lo, hi) = (ivl.min(), ivl.max());
    let mut out = Vec::new();
    for i in 0..n {
        let t = lo + (hi - lo) * (f64::from(i) / f64::from(n - 1));
        match c.evaluate_point(k, t) {
            Ok(p) => out.push(p),
            Err(e @ SolidError::Unported { .. }) => return Err(e),
            Err(_) => {}
        }
    }
    Ok((out, [lo, hi]))
}

pub(crate) fn to_p3(v: Vec3) -> P3 {
    [v.x, v.y, v.z]
}

/// Writes the intersection `x` of `surfaces` into `out`, sampled as the C++ runner
/// samples it: each curve at [`N_SAMPLES`] parameters over its natural interval, each
/// UV curve the same, and each surface at its UV samples. `Ok(Err(_))` names the unit
/// an unported evaluation on the way needs.
pub(crate) fn write_intersection(
    k: &Kernel,
    x: &SurfaceIntersection,
    surfaces: [&Surface; 2],
    out: &mut RawOutput,
) -> Result<Result<(), Deferred>, EngineFailure> {
    for (i, c3) in x.curves_3d.iter().enumerate() {
        let ty = x.curve_types.get(i).map_or(-1, |t| *t as i64);
        let deviation = x.deviations.get(i).copied().unwrap_or(f64::NAN);
        let (pts, ivl) = match sample(k, c3, N_SAMPLES) {
            Ok(r) => r,
            Err(SolidError::Unported { upstream, unit }) => {
                return Ok(Err(deferred(unit, upstream)))
            }
            Err(e) => return Err(EngineFailure::Other(e.to_string())),
        };
        let mut curve = RawCurve {
            ty: type_name(ty).to_string(),
            deviation,
            interval: Some(ivl),
            class_type: Some(c3.type_code()),
            xyz: pts.into_iter().map(to_p3).collect(),
            ..RawCurve::default()
        };
        let uvs = [&x.surface1_uv_curves, &x.surface2_uv_curves];
        for (kk, (uvlist, surf)) in uvs.iter().zip(surfaces).enumerate() {
            let Some(Some(uvc)) = uvlist.get(i) else {
                continue;
            };
            let (uv, _) = match sample(k, uvc, N_SAMPLES) {
                Ok(r) => r,
                Err(SolidError::Unported { upstream, unit }) => {
                    return Ok(Err(deferred(unit, upstream)))
                }
                Err(e) => return Err(EngineFailure::Other(e.to_string())),
            };
            let mut at = Vec::with_capacity(uv.len());
            for p in &uv {
                match surf.evaluate_point(k, Point2::new(p.x, p.y)) {
                    Ok(q) => at.push(to_p3(q)),
                    Err(SolidError::Unported { upstream, unit }) => {
                        return Ok(Err(deferred(unit, upstream)))
                    }
                    Err(_) => at.push([f64::NAN; 3]),
                }
            }
            let uv2: Vec<[f64; 2]> = uv.iter().map(|p| [p.x, p.y]).collect();
            if kk == 0 {
                curve.uv1 = uv2;
                curve.s1_at_uv = at;
            } else {
                curve.uv2 = uv2;
                curve.s2_at_uv = at;
            }
        }
        out.curves.push(curve);
    }
    Ok(Ok(()))
}
