//! Surface patches described in plain numbers, and the intersection of two of them read back as
//! plain numbers: the surface/surface intersection a caller without the kernel's types runs.
//!
//! A [`PatchSpec`] names a plane, a cylinder, a cone, a sphere, a torus or a NURBS net by the
//! numbers SMLib's C API constructors take (`smlib_ssi.cpp`'s `construct`), angles in degrees.
//! [`Patch::new`] makes the kernel's surface from it as those constructors make it, over its
//! natural UV domain; [`intersect`] runs `SmSurface::GlobalSurfaceIntersect` or
//! `SmAdvSurfaceIntersector` on two patches, and [`read`] reads each curve as the C API hands it
//! out ([`PatchCurve`]).
//!
//! Upstream: the SolidCraft fork's `capi/src/smlib_ssi.cpp` (`construct`, `nurbs`, `start`,
//! `smlib_ssi`), on `SmSurface::GlobalSurfaceIntersect` and `SmAdvSurfaceIntersector`.

use crate::math::det;
use std::borrow::Cow;

use crate::curve::curve::Curve;
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent2d::Extent2d;
use crate::geom::vector2d::{Point2, Vec2};
use crate::geom::vector3d::{Point3, Vec3};
use crate::intersect::surface_intersector::p5::do_intersection;
use crate::kernel::Kernel;
use crate::nurbs::b_spline_curve::p1::KnotType;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::p1::{
    BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData,
};
use crate::nurbs::math::{CPoint, NOW};
use crate::surface::cone::p1::Cone;
use crate::surface::plane::p1::Plane;
use crate::surface::sphere::{Sphere, SphereOptions};
use crate::surface::surface::p2::{SurfaceIntersection, TsectCurveType};
use crate::surface::surface::Surface;
use crate::surface::torus::{Torus, TorusOptions};
use crate::{Result, SolidError};

/// A surface patch in plain numbers: the arguments of SMLib's C API surface constructors, angles
/// in degrees. The axes need not be unit or square; each constructor takes them as the C API's
/// does.
#[derive(Clone, Debug, PartialEq)]
pub enum PatchSpec {
    /// `origin + u*x + v*y`, for u in `u` and v in `v` (`x`, `y` the axes made unit).
    Plane {
        /// The plane's origin, where (u, v) is (0, 0).
        origin: [f64; 3],
        /// The first axis: u runs along it, angles count from it.
        xaxis: [f64; 3],
        /// The second axis: with the first it makes the frame.
        yaxis: [f64; 3],
        /// The range of u, along the first axis made unit.
        u: [f64; 2],
        /// The range of v, along the second axis made unit.
        v: [f64; 2],
    },
    /// A cylinder about the axis `xaxis × yaxis` through `origin`, over the turn `angle_deg` and
    /// the heights `height` along the axis.
    Cylinder {
        /// A point on the axis, where the heights count from.
        origin: [f64; 3],
        /// The first axis: u runs along it, angles count from it.
        xaxis: [f64; 3],
        /// The second axis: with the first it makes the frame.
        yaxis: [f64; 3],
        /// The radius.
        radius: f64,
        /// The turn about the axis, in degrees.
        angle_deg: [f64; 2],
        /// The heights along the axis.
        height: [f64; 2],
    },
    /// A cone with its apex at `apex`, widening along `xaxis × yaxis` at `half_angle_deg`, over the
    /// heights `height` from the apex.
    Cone {
        /// The cone's apex.
        apex: [f64; 3],
        /// The first axis: u runs along it, angles count from it.
        xaxis: [f64; 3],
        /// The second axis: with the first it makes the frame.
        yaxis: [f64; 3],
        /// The angle between the axis and the side, in degrees.
        half_angle_deg: f64,
        /// The heights along the axis.
        height: [f64; 2],
    },
    /// A sphere over the longitudes `lon_deg` and latitudes `lat_deg`.
    Sphere {
        /// The centre.
        center: [f64; 3],
        /// The first axis: u runs along it, angles count from it.
        xaxis: [f64; 3],
        /// The second axis: with the first it makes the frame.
        yaxis: [f64; 3],
        /// The radius.
        radius: f64,
        /// The longitudes, in degrees.
        lon_deg: [f64; 2],
        /// The latitudes, in degrees.
        lat_deg: [f64; 2],
    },
    /// A torus over the turns `major_deg` about its axis and `minor_deg` about its tube.
    Torus {
        /// The centre.
        center: [f64; 3],
        /// The first axis: u runs along it, angles count from it.
        xaxis: [f64; 3],
        /// The second axis: with the first it makes the frame.
        yaxis: [f64; 3],
        /// The radius of the tube's centre circle.
        major_radius: f64,
        /// The tube's radius.
        minor_radius: f64,
        /// The turn about the axis, in degrees.
        major_deg: [f64; 2],
        /// The turn about the tube, in degrees, from its outer equator.
        minor_deg: [f64; 2],
    },
    /// A NURBS net of `nu` rows of `nv` points (`points[i * nv + j]`), of degree `degree` in u and
    /// v, with its knot vectors and, for a rational net, a weight per point.
    Nurbs {
        /// The degrees in u and v.
        degree: [usize; 2],
        /// The rows of the net (along u).
        nu: usize,
        /// The points in each row (along v).
        nv: usize,
        /// The knot vector in u: `nu + degree[0] + 1` values.
        knots_u: Vec<f64>,
        /// The knot vector in v: `nv + degree[1] + 1` values.
        knots_v: Vec<f64>,
        /// The control points, row by row.
        points: Vec<[f64; 3]>,
        /// A weight per point, for a rational net.
        weights: Option<Vec<f64>>,
    },
}

/// A surface over its natural UV domain, made from a [`PatchSpec`].
#[derive(Clone, Debug)]
pub struct Patch {
    /// The kernel's surface.
    pub surface: Surface,
    /// Its natural UV domain: finite and not empty.
    pub domain: Extent2d,
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// The C API runner's `unit_cross`.
fn unit_cross(a: Vec3, b: Vec3) -> Vec3 {
    let c = a.cross(b);
    let l = c.length();
    Vec3::new(c.x / l, c.y / l, c.z / l)
}

/// `[k0, k0, k1, ...]` as unique values and their multiplicities (`unique_knots`).
fn unique_knots(k: &[f64]) -> (Vec<f64>, Vec<usize>) {
    let (mut values, mut mults): (Vec<f64>, Vec<usize>) = (Vec::new(), Vec::new());
    for &x in k {
        match (values.last(), mults.last_mut()) {
            (Some(&last), Some(m)) if last == x => *m += 1,
            _ => {
                values.push(x);
                mults.push(1);
            }
        }
    }
    (values, mults)
}

/// `clamped`: the first and the last knot each p + 1 times.
fn clamped(k: &[f64], p: usize) -> bool {
    match (
        k.first(),
        k.get(p),
        k.last(),
        k.len().checked_sub(1 + p).and_then(|i| k.get(i)),
    ) {
        (Some(a), Some(b), Some(c), Some(d)) => a == b && c == d,
        _ => false,
    }
}

impl Patch {
    /// The surface of `spec` as `smlib_ssi.cpp`'s constructors make it, over its natural UV
    /// domain (`construct`); `None` when that domain is not finite or is empty
    /// (`SM_ERR_INVALID_INPUT` there). The caller checks the numbers first: this makes what the
    /// kernel's constructors make of them.
    ///
    /// # Errors
    /// [`SolidError::InvalidInput`] for a NURBS net with fewer than `nu * nv` points; the
    /// kernel's error from a surface constructor or the domain.
    pub fn new(k: &Kernel, spec: &PatchSpec) -> Result<Option<Patch>> {
        let surface = match spec {
            PatchSpec::Plane {
                origin,
                xaxis,
                yaxis,
                u,
                v,
            } => {
                let dom = Extent2d::from_bounds(u[0], v[0], u[1], v[1]);
                Surface::from(Plane::new(
                    v3(*origin),
                    v3(*xaxis),
                    v3(*yaxis),
                    Vec2::new(1.0, 1.0),
                    &dom,
                ))
            }
            PatchSpec::Cylinder {
                origin,
                xaxis,
                yaxis,
                radius,
                angle_deg,
                height,
            } => {
                let (x, y) = (v3(*xaxis), v3(*yaxis));
                let o = v3(*origin) + unit_cross(x, y) * height[0];
                Surface::from(Cone::new_cylinder_bounded(
                    o,
                    x,
                    y,
                    *radius,
                    angle_deg[0],
                    angle_deg[1],
                    height[1] - height[0],
                    false,
                    false,
                    false,
                ))
            }
            PatchSpec::Cone {
                apex,
                xaxis,
                yaxis,
                half_angle_deg,
                height,
            } => {
                let (x, y) = (v3(*xaxis), v3(*yaxis));
                let (h0, h1) = (height[0], height[1]);
                let t = det::tan(half_angle_deg.to_radians());
                let o = v3(*apex) + unit_cross(x, y) * h0;
                let pl = Axis2Placement::new(o, x, y);
                Surface::from(Cone::create_canonical(
                    k,
                    &pl,
                    h0 * t,
                    h1 * t,
                    h1 - h0,
                    false,
                    false,
                )?)
            }
            PatchSpec::Sphere {
                center,
                xaxis,
                yaxis,
                radius,
                lon_deg,
                lat_deg,
            } => {
                let anal = Extent2d::from_bounds(lon_deg[0], lat_deg[0], lon_deg[1], lat_deg[1]);
                let opts = SphereOptions {
                    swap_uv: false,
                    inside_out: false,
                    circle_nurb: None,
                };
                Surface::from(Sphere::new(
                    k,
                    v3(*center),
                    v3(*xaxis),
                    v3(*yaxis),
                    anal,
                    *radius,
                    opts,
                )?)
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
                let anal =
                    Extent2d::from_bounds(major_deg[0], minor_deg[0], major_deg[1], minor_deg[1]);
                Surface::from(Torus::new(
                    k,
                    v3(*center),
                    v3(*xaxis),
                    v3(*yaxis),
                    anal,
                    *major_radius,
                    *minor_radius,
                    TorusOptions::default(),
                )?)
            }
            PatchSpec::Nurbs {
                degree,
                nu,
                nv,
                knots_u,
                knots_v,
                points,
                weights,
            } => {
                let [p, q] = *degree;
                let w = |i: usize| {
                    weights
                        .as_ref()
                        .and_then(|w| w.get(i).copied())
                        .unwrap_or(1.0)
                };
                if !clamped(knots_u, p) || !clamped(knots_v, q) {
                    // SMLib's component constructor clamps the surface to its domain.
                    let (ku, mu) = unique_knots(knots_u);
                    let (kv, mv) = unique_knots(knots_v);
                    let wts: Vec<f64> = (0..points.len()).map(w).collect();
                    let pts: Vec<Point3> = points.iter().map(|p| v3(*p)).collect();
                    let data = CanonicalSurfaceData {
                        u_degree: p,
                        v_degree: q,
                        control_points: &pts,
                        form: BSplineSurfaceForm::Unspecified,
                        u_multiplicities: &mu,
                        v_multiplicities: &mv,
                        u_knots: &ku,
                        v_knots: &kv,
                        knot_type: KnotType::Unspecified,
                        weights: Some(&wts),
                        uv_domain: None,
                    };
                    Surface::from(BSplineSurface::create_canonical(&data)?)
                } else {
                    // The runner's way: the homogeneous net filled directly.
                    let mut pw = Vec::new();
                    for i in 0..*nu {
                        let mut row = Vec::new();
                        for j in 0..*nv {
                            let at = i.checked_mul(*nv).and_then(|r| r.checked_add(j));
                            let (Some(pt), Some(at)) = (at.and_then(|at| points.get(at)), at)
                            else {
                                return Err(SolidError::InvalidInput {
                                    operation: "Patch::new",
                                    reason: "a NURBS net with fewer than nu * nv points",
                                });
                            };
                            let wt = w(at);
                            row.push(CPoint::new(pt[0] * wt, pt[1] * wt, pt[2] * wt, wt));
                        }
                        pw.push(row);
                    }
                    let (n, m) = (nu.saturating_sub(1), nv.saturating_sub(1));
                    let (r, s) = (
                        knots_u.len().saturating_sub(1),
                        knots_v.len().saturating_sub(1),
                    );
                    Surface::from(BSplineSurface::from_data(
                        n, m, p, q, r, s, &pw, knots_u, knots_v,
                    )?)
                }
            }
        };
        let domain = surface.natural_uv_domain_with_nurb(k)?;
        let (lo, hi) = (domain.min(), domain.max());
        if !(lo.x.is_finite()
            && lo.y.is_finite()
            && hi.x.is_finite()
            && hi.y.is_finite()
            && lo.x < hi.x
            && lo.y < hi.y)
        {
            return Ok(None);
        }
        Ok(Some(Patch { surface, domain }))
    }

    /// The UV domain as `[u0, u1, v0, v1]`.
    #[must_use]
    pub fn uv_box(&self) -> [f64; 4] {
        let (lo, hi) = (self.domain.min(), self.domain.max());
        [lo.x, hi.x, lo.y, hi.y]
    }

    /// The surface's point at (u, v), which may be outside the domain.
    ///
    /// # Errors
    /// The kernel's error evaluating it.
    pub fn point(&self, k: &Kernel, u: f64, v: f64) -> Result<[f64; 3]> {
        let p = self.surface.evaluate_point(k, Point2::new(u, v))?;
        Ok([p.x, p.y, p.z])
    }
}

/// Which of SMLib's intersectors runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// `SmSurface::GlobalSurfaceIntersect`: analytic special cases, else [`Method::Advanced`].
    Global,
    /// `SmAdvSurfaceIntersector`, the general NURBS path.
    Advanced,
}

/// A (rational) B-spline as SMLib's C API hands one out: its control points made cartesian
/// (each coordinate divided by its weight), `dim` reals each (3 in space, 2 in UV), a weight
/// per point (1 for a polynomial one) and its knots.
#[derive(Clone, Debug, PartialEq)]
pub struct CartesianNurbs {
    /// The cartesian control points, `dim` reals each.
    pub points: Vec<f64>,
    /// A weight per control point.
    pub weights: Vec<f64>,
    /// The knot vector.
    pub knots: Vec<f64>,
}

impl CartesianNurbs {
    /// Whether every weight is 1: a polynomial B-spline.
    #[must_use]
    pub fn is_polynomial(&self) -> bool {
        self.weights.iter().all(|w| *w == 1.0)
    }

    /// The control points of a 3D curve, 3 reals each.
    #[must_use]
    pub fn points3(&self) -> Vec<[f64; 3]> {
        self.points
            .chunks_exact(3)
            .filter_map(|p| <[f64; 3]>::try_from(p).ok())
            .collect()
    }

    /// The control points of a UV curve, 2 reals each.
    #[must_use]
    pub fn points2(&self) -> Vec<[f64; 2]> {
        self.points
            .chunks_exact(2)
            .filter_map(|p| <[f64; 2]>::try_from(p).ok())
            .collect()
    }

    /// The homogeneous control points of a 3D curve: each cartesian point times its weight, then
    /// the weight.
    #[must_use]
    pub fn homogeneous3(&self) -> Vec<[f64; 4]> {
        self.points3()
            .iter()
            .zip(&self.weights)
            .map(|(p, w)| [p[0] * w, p[1] * w, p[2] * w, *w])
            .collect()
    }

    /// The homogeneous control points of a UV curve, as [`CartesianNurbs::homogeneous3`].
    #[must_use]
    pub fn homogeneous2(&self) -> Vec<[f64; 3]> {
        self.points2()
            .iter()
            .zip(&self.weights)
            .map(|(p, w)| [p[0] * w, p[1] * w, *w])
            .collect()
    }
}

/// One intersection curve as the C API reads it.
#[derive(Clone, Debug)]
pub struct PatchCurve {
    /// The kind the method reported, if it reported one.
    pub curve_type: Option<TsectCurveType>,
    /// The method's deviation estimate (NaN: none).
    pub deviation: f64,
    /// The 3D curve's natural parameter interval.
    pub interval: [f64; 2],
    /// The 3D curve.
    pub curve: CartesianNurbs,
    /// Its curve in the first surface's UV, where the method returned one that is a B-spline.
    pub uv_a: Option<CartesianNurbs>,
    /// Its curve in the second surface's UV, the same way.
    pub uv_b: Option<CartesianNurbs>,
}

/// An isolated point: where a touching curve starts, in space and in each surface's UV (NaN
/// where there is no UV curve or it can't be evaluated).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TouchPoint {
    /// In space.
    pub xyz: [f64; 3],
    /// In the first surface's UV.
    pub uv_a: [f64; 2],
    /// In the second surface's UV.
    pub uv_b: [f64; 2],
}

/// What an intersection of two patches found.
#[derive(Clone, Debug, Default)]
pub struct PatchIntersection {
    /// The curves, in the intersector's order.
    pub curves: Vec<PatchCurve>,
    /// Where each touching curve starts, in the same order.
    pub points: Vec<TouchPoint>,
}

/// The C API's `nurbs`: `c` as its B-spline, control points made cartesian in `dim`
/// coordinates; `None` for a curve that is not one, which is never approximated here.
fn cartesian(k: &Kernel, c: &Curve, dim: usize) -> Result<Option<CartesianNurbs>> {
    let Some(part): Option<Cow<'_, BSplineCurve>> = c.bspline_part(k)? else {
        return Ok(None);
    };
    let Some(n) = part.gw_nurb() else {
        return Ok(None);
    };
    let mut out = CartesianNurbs {
        points: Vec::new(),
        weights: Vec::new(),
        knots: n.knots().to_vec(),
    };
    for p in n.cpts() {
        let w = if p.w == NOW { 1.0 } else { p.w };
        let q = [p.x / w, p.y / w, p.z / w];
        out.points.extend_from_slice(q.get(..dim).unwrap_or(&q));
        out.weights.push(w);
    }
    Ok(Some(out))
}

/// The C API's `start`: where `c` starts, NaN where there is no curve or it can't be evaluated.
fn start(k: &Kernel, c: Option<&Curve>) -> [f64; 3] {
    c.and_then(|c| c.evaluate_point(k, c.natural_interval().min()).ok())
        .map_or([f64::NAN; 3], |p| [p.x, p.y, p.z])
}

/// The intersection of two patches as the intersector returns it, read as the C API reads it
/// ([`PatchCurve`]); `None` at the first 3D curve that is no B-spline, the one thing the result
/// can't hold.
///
/// # Errors
/// The kernel's error reading a curve.
pub fn read(k: &Kernel, r: &SurfaceIntersection) -> Result<Option<PatchIntersection>> {
    let mut out = PatchIntersection::default();
    for (i, c3) in r.curves_3d.iter().enumerate() {
        let (ua, ub) = (
            r.surface1_uv_curves.get(i).and_then(Option::as_ref),
            r.surface2_uv_curves.get(i).and_then(Option::as_ref),
        );
        let ty = r.curve_types.get(i).copied();
        let ivl = c3.natural_interval();
        let uv = |c: Option<&Curve>| -> Result<_> {
            Ok(c.map(|c| cartesian(k, c, 2)).transpose()?.flatten())
        };
        let deviation = r.deviations.get(i).copied().unwrap_or(f64::NAN);
        let Some(curve) = cartesian(k, c3, 3)? else {
            return Ok(None);
        };
        out.curves.push(PatchCurve {
            curve_type: ty,
            deviation,
            interval: [ivl.min(), ivl.max()],
            curve,
            uv_a: uv(ua)?,
            uv_b: uv(ub)?,
        });
        if ty == Some(TsectCurveType::Touching) {
            let (a, b) = (start(k, ua), start(k, ub));
            out.points.push(TouchPoint {
                xyz: start(k, Some(c3)),
                uv_a: [a[0], a[1]],
                uv_b: [b[0], b[1]],
            });
        }
    }
    Ok(Some(out))
}

/// `smlib_ssi`'s intersection: `a` and `b` within their domains by `method`, at approximation
/// tolerance `approx_tol` (`None`: the kernel's default), the surfaces' edges taken as curve
/// ends. [`read`] reads the result.
///
/// # Errors
/// The intersector's error, an SMLib status where it stands for one ([`SolidError::sm_status`]).
pub fn intersect(
    k: &Kernel,
    a: &Patch,
    b: &Patch,
    method: Method,
    approx_tol: Option<f64>,
) -> Result<SurfaceIntersection> {
    let edges = [true, true];
    match method {
        Method::Global => a
            .surface
            .global_surface_intersect(k, &a.domain, &b.surface, &b.domain, edges, approx_tol, None),
        Method::Advanced => do_intersection(
            k, &a.surface, &a.domain, &b.surface, &b.domain, edges, approx_tol, None,
        ),
    }
}
