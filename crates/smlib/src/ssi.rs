//! Surface/surface intersection (`capi/include/smlib.h`, "Surface/surface intersection"): two
//! surfaces described in plain numbers, intersected by one of SMLib's SSI entry points, each
//! intersection curve exactly as SMLib holds it: schema 4 B-splines (`solidcraft_geom::brep`).
//!
//! The constructors take exactly the fields of an SSI corpus case (`tests/ssi-corpus`), angles in
//! degrees. Each surface's UV domain is SMLib's natural one ([`Surface::domain`]): (u, v) itself
//! for planes, the parameterisation of SMLib's internal NURBS form for the other analytic kinds.

use solidcraft_geom::brep as b;

use std::sync::OnceLock;

use crate::{Error, Result, error, ffi, init, route};

/// A surface with its UV domain: the description it was made from, from which solidcore makes its
/// own ([`crate::core::split`]), and the C side's, made when the C++ kernel needs it.
pub struct Surface {
    handle: OnceLock<Result<ffi::ssi::SurfaceHandle>>,
    /// solidcore's, made when it is first needed.
    core: OnceLock<Result<crate::core::split::Tool>>,
    spec: Spec,
}

/// What a [`Surface`] was made from: its constructor's arguments, checked as the C API checks them
/// ([`Spec::check`]).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Spec {
    Plane { origin: [f64; 3], xaxis: [f64; 3], yaxis: [f64; 3], u: [f64; 2], v: [f64; 2] },
    Cylinder { origin: [f64; 3], xaxis: [f64; 3], yaxis: [f64; 3], radius: f64, angle_deg: [f64; 2], height: [f64; 2] },
    Cone { apex: [f64; 3], xaxis: [f64; 3], yaxis: [f64; 3], half_angle_deg: f64, height: [f64; 2] },
    Sphere { center: [f64; 3], xaxis: [f64; 3], yaxis: [f64; 3], radius: f64, lon_deg: [f64; 2], lat_deg: [f64; 2] },
    Torus { center: [f64; 3], xaxis: [f64; 3], yaxis: [f64; 3], major_radius: f64, minor_radius: f64, major_deg: [f64; 2], minor_deg: [f64; 2] },
    Nurbs { degree: [usize; 2], nu: usize, nv: usize, knots_u: Vec<f64>, knots_v: Vec<f64>, points: Vec<[f64; 3]>, weights: Option<Vec<f64>> },
}

impl std::fmt::Debug for Surface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("smlib::ssi::Surface")
    }
}

fn int(n: usize) -> Result<i32> {
    i32::try_from(n).map_err(|_| Error::Argument)
}

/// The largest coordinate, degree and control net the C API takes (`SMLIB_SURFACE_MAX_*`).
const MAX_COORD: f64 = 1e8;
const MAX_DEGREE: usize = 25;
const MAX_POLES: usize = 1_000_000;

fn bounded(v: &[f64]) -> bool {
    v.iter().all(|x| x.is_finite() && x.abs() <= MAX_COORD)
}

/// Two non-zero axes that are not (nearly) parallel.
fn axes(x: &[f64; 3], y: &[f64; 3]) -> bool {
    let len = |a: &[f64; 3]| (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    let c = [x[1] * y[2] - x[2] * y[1], x[2] * y[0] - x[0] * y[2], x[0] * y[1] - x[1] * y[0]];
    bounded(x) && bounded(y) && len(x) > 1e-12 && len(y) > 1e-12 && len(&c) > 1e-9 * len(x) * len(y)
}

/// An increasing interval inside `[lo, hi]` spanning at most `span`.
fn range(r: &[f64; 2], lo: f64, hi: f64, span: f64) -> bool {
    bounded(r) && r[0] < r[1] && r[0] >= lo && r[1] <= hi && r[1] - r[0] <= span
}

fn length(x: f64) -> bool {
    x.is_finite() && x > 1e-9 && x <= MAX_COORD
}

/// A knot vector of n + p + 1 non-decreasing values with a non-empty domain `[k[p], k[n]]`, no
/// interior knot repeated more than p times and no end knot more than p + 1.
fn knots(k: &[f64], n: usize, p: usize) -> bool {
    if k.len() != n + p + 1 || !bounded(k) || k.windows(2).any(|w| w[1] < w[0]) {
        return false;
    }
    let (Some(lo), Some(hi)) = (k.get(p).copied(), k.get(n).copied()) else { return false };
    let mut run = 1;
    for w in k.windows(2) {
        run = if w[1] == w[0] { run + 1 } else { 1 };
        if run > p + 1 || (run > p && w[1] > lo && w[1] < hi) {
            return false;
        }
    }
    lo < hi
}

impl Spec {
    /// The checks each of the C API's constructors makes of its arguments, so that a surface is
    /// refused alike on either kernel, and without the C++ kernel.
    fn check(&self) -> Result<()> {
        let whole = |r: &[f64; 2]| range(r, -MAX_COORD, MAX_COORD, 2.0 * MAX_COORD);
        let turn = |r: &[f64; 2]| range(r, -360.0, 360.0, 360.0);
        let ok = match self {
            Spec::Plane { origin, xaxis, yaxis, u, v } => bounded(origin) && axes(xaxis, yaxis) && whole(u) && whole(v),
            Spec::Cylinder { origin, xaxis, yaxis, radius, angle_deg, height } => {
                bounded(origin) && axes(xaxis, yaxis) && length(*radius) && turn(angle_deg) && range(height, -MAX_COORD, MAX_COORD, MAX_COORD)
            }
            Spec::Cone { apex, xaxis, yaxis, half_angle_deg, height } => {
                bounded(apex)
                    && axes(xaxis, yaxis)
                    && (0.01..=89.0).contains(half_angle_deg)
                    && range(height, 0.0, MAX_COORD, MAX_COORD)
                    && height[1] - height[0] > 1e-9
            }
            Spec::Sphere { center, xaxis, yaxis, radius, lon_deg, lat_deg } => {
                bounded(center) && axes(xaxis, yaxis) && length(*radius) && turn(lon_deg) && range(lat_deg, -90.0, 90.0, 180.0)
            }
            Spec::Torus { center, xaxis, yaxis, major_radius, minor_radius, major_deg, minor_deg } => {
                bounded(center)
                    && axes(xaxis, yaxis)
                    && length(*major_radius)
                    && length(*minor_radius)
                    && minor_radius < major_radius
                    && turn(major_deg)
                    && turn(minor_deg)
            }
            Spec::Nurbs { degree: [p, q], nu, nv, knots_u, knots_v, points, weights } => {
                (1..=MAX_DEGREE).contains(p)
                    && (1..=MAX_DEGREE).contains(q)
                    && *nu > *p
                    && *nv > *q
                    && nu.checked_mul(*nv).is_some_and(|n| n <= MAX_POLES && points.len() == n)
                    && knots(knots_u, *nu, *p)
                    && knots(knots_v, *nv, *q)
                    && points.iter().all(|c| bounded(c))
                    && weights.as_ref().is_none_or(|w| w.len() == points.len() && w.iter().all(|x| (1e-6..=1e6).contains(x)))
            }
        };
        if ok { Ok(()) } else { Err(Error::Argument) }
    }

    /// The C side's surface.
    fn on_cpp(&self) -> Result<ffi::ssi::SurfaceHandle> {
        init()?;
        match self {
            Spec::Plane { origin, xaxis, yaxis, u, v } => ffi::ssi::plane(origin, xaxis, yaxis, u, v),
            Spec::Cylinder { origin, xaxis, yaxis, radius, angle_deg, height } => {
                ffi::ssi::cylinder(origin, xaxis, yaxis, *radius, angle_deg, height)
            }
            Spec::Cone { apex, xaxis, yaxis, half_angle_deg, height } => ffi::ssi::cone(apex, xaxis, yaxis, *half_angle_deg, height),
            Spec::Sphere { center, xaxis, yaxis, radius, lon_deg, lat_deg } => ffi::ssi::sphere(center, xaxis, yaxis, *radius, lon_deg, lat_deg),
            Spec::Torus { center, xaxis, yaxis, major_radius, minor_radius, major_deg, minor_deg } => {
                ffi::ssi::torus(center, xaxis, yaxis, *major_radius, *minor_radius, major_deg, minor_deg)
            }
            Spec::Nurbs { degree, nu, nv, knots_u, knots_v, points, weights } => {
                let flat: Vec<f64> = points.iter().flatten().copied().collect();
                ffi::ssi::nurbs([int(degree[0])?, int(degree[1])?], int(*nu)?, int(*nv)?, knots_u, knots_v, &flat, weights.as_deref())
            }
        }
        .map_err(error)
    }
}

/// Whether intersections run on the C++ kernel ([`route::backend`]).
fn ssi_on_cpp() -> bool {
    route::backend() == route::Backend::Smlib
}

impl Surface {
    /// The C side's surface, made the first time the C++ kernel needs it.
    pub(crate) fn handle(&self) -> Result<&ffi::ssi::SurfaceHandle> {
        self.handle.get_or_init(|| self.spec.on_cpp()).as_ref().map_err(Clone::clone)
    }

    /// solidcore's surface, made the first time it is needed.
    fn core(&self) -> Result<&crate::core::split::Tool> {
        self.core.get_or_init(|| crate::core::ssi::surface(&self.spec)).as_ref().map_err(Clone::clone)
    }

    pub(crate) fn spec(&self) -> &Spec {
        &self.spec
    }

    /// A surface from its checked description. On the C++
    /// kernel, its side is made now, so that what it refuses is refused here.
    fn made(spec: Spec) -> Result<Surface> {
        spec.check()?;
        let s = Surface { handle: OnceLock::new(), core: OnceLock::new(), spec };
        if ssi_on_cpp() {
            s.handle()?;
        }
        Ok(s)
    }

    /// `origin + u*x + v*y` (x, y the axes made unit), for u in `u`, v in `v`.
    pub fn plane(origin: [f64; 3], xaxis: [f64; 3], yaxis: [f64; 3], u: [f64; 2], v: [f64; 2]) -> Result<Surface> {
        Surface::made(Spec::Plane { origin, xaxis, yaxis, u, v })
    }

    /// A cylinder of `radius` about the axis through `origin` along `xaxis × yaxis`, angle
    /// `angle_deg` from `xaxis` towards `yaxis`, for heights `height` measured from `origin`.
    pub fn cylinder(origin: [f64; 3], xaxis: [f64; 3], yaxis: [f64; 3], radius: f64, angle_deg: [f64; 2], height: [f64; 2]) -> Result<Surface> {
        Surface::made(Spec::Cylinder { origin, xaxis, yaxis, radius, angle_deg, height })
    }

    /// A full cone with its apex at `apex` along `xaxis × yaxis`, radius `h * tan(half_angle_deg)`
    /// at distance h along the axis, for h in `height` (from 0: through the apex).
    pub fn cone(apex: [f64; 3], xaxis: [f64; 3], yaxis: [f64; 3], half_angle_deg: f64, height: [f64; 2]) -> Result<Surface> {
        Surface::made(Spec::Cone { apex, xaxis, yaxis, half_angle_deg, height })
    }

    /// A sphere of `radius` at `center`, over longitudes `lon_deg` and latitudes `lat_deg`.
    pub fn sphere(center: [f64; 3], xaxis: [f64; 3], yaxis: [f64; 3], radius: f64, lon_deg: [f64; 2], lat_deg: [f64; 2]) -> Result<Surface> {
        Surface::made(Spec::Sphere { center, xaxis, yaxis, radius, lon_deg, lat_deg })
    }

    /// A ring torus at `center` about `xaxis × yaxis`, `major_deg` round the axis and `minor_deg`
    /// round the tube.
    pub fn torus(
        center: [f64; 3],
        xaxis: [f64; 3],
        yaxis: [f64; 3],
        major_radius: f64,
        minor_radius: f64,
        major_deg: [f64; 2],
        minor_deg: [f64; 2],
    ) -> Result<Surface> {
        Surface::made(Spec::Torus { center, xaxis, yaxis, major_radius, minor_radius, major_deg, minor_deg })
    }

    /// A (rational) B-spline surface: `nu × nv` control points, point (i, j) (i along u) at
    /// `points[i * nv + j]`; `weights` likewise, or `None` for all 1.
    pub fn nurbs(
        degree: [usize; 2],
        nu: usize,
        nv: usize,
        knots_u: &[f64],
        knots_v: &[f64],
        points: &[[f64; 3]],
        weights: Option<&[f64]>,
    ) -> Result<Surface> {
        Surface::made(Spec::Nurbs {
            degree,
            nu,
            nv,
            knots_u: knots_u.to_vec(),
            knots_v: knots_v.to_vec(),
            points: points.to_vec(),
            weights: weights.map(<[f64]>::to_vec),
        })
    }

    /// A schema 4 record surface over the parameter box `domain` (`[[u0, u1], [v0, v1]]`, the
    /// record's own parameters: radians for angles), with the record's parameterisation and
    /// normal. A spline is taken over its whole knot domain. A cone only as a whole turn, widening
    /// along its axis; a revolution not at all ([`Error::Unsupported`]).
    pub fn from_record(surface: &solidcraft_geom::brep::Surface, domain: [[f64; 2]; 2]) -> Result<Surface> {
        use solidcraft_geom::brep::Surface as S;
        let [u, v] = domain;
        let deg = |r: [f64; 2]| r.map(f64::to_degrees);
        let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        let len = |a: [f64; 3]| (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
        let cross = |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
        // SMLib's plane runs along its axes made unit: the record's parameters scale by their lengths.
        let plane = |origin: [f64; 3], du: [f64; 3], dv: [f64; 3]| {
            let (a, b) = (len(du), len(dv));
            Surface::plane(origin, du, dv, [u[0] * a, u[1] * a], [v[0] * b, v[1] * b])
        };
        match surface {
            S::Plane { origin, u_axis, v_axis } => plane(*origin, *u_axis, *v_axis),
            S::PlaneThrough { origin, u_point, v_point } => plane(*origin, sub(*u_point, *origin), sub(*v_point, *origin)),
            S::Cylinder { frame, radius } => Surface::cylinder(frame.origin, frame.x_axis, frame.y_axis, *radius, deg(u), v),
            S::Cone { frame, radius_at_origin, half_angle } => {
                let t = half_angle.tan();
                if !(t > 0.0) || !(u[1] - u[0] >= std::f64::consts::TAU * (1.0 - 1e-12)) {
                    return Err(Error::Unsupported);
                }
                // The apex lies where the radius is 0; heights run from it along the axis.
                let at = -radius_at_origin / t;
                let z = cross(frame.x_axis, frame.y_axis);
                let apex = [0, 1, 2].map(|k| frame.origin[k] + z[k] * at);
                Surface::cone(apex, frame.x_axis, frame.y_axis, half_angle.to_degrees(), [(v[0] - at).max(0.0), v[1] - at])
            }
            S::Sphere { frame, radius } => Surface::sphere(frame.origin, frame.x_axis, frame.y_axis, *radius, deg(u), deg(v)),
            S::Torus { frame, major_radius, minor_radius, tube_start } => {
                // SMLib's tube angle counts from the outer equator.
                Surface::torus(frame.origin, frame.x_axis, frame.y_axis, *major_radius, *minor_radius, deg(u), deg(v.map(|a| a + tube_start)))
            }
            S::Spline { u_knots, v_knots, points } => {
                let (nu, nv) = (points.len(), points.first().map_or(0, Vec::len));
                let flat: Vec<[f64; 3]> = points.iter().flatten().copied().collect();
                let degree = [u_knots.len().checked_sub(nu + 1).ok_or(Error::Argument)?, v_knots.len().checked_sub(nv + 1).ok_or(Error::Argument)?];
                Surface::nurbs(degree, nu, nv, u_knots, v_knots, &flat, None)
            }
            S::RationalSpline { u_knots, v_knots, homogeneous } => {
                let (nu, nv) = (homogeneous.len(), homogeneous.first().map_or(0, Vec::len));
                let all: Vec<[f64; 4]> = homogeneous.iter().flatten().copied().collect();
                let flat: Vec<[f64; 3]> = all.iter().map(|h| [h[0] / h[3], h[1] / h[3], h[2] / h[3]]).collect();
                let weights: Vec<f64> = all.iter().map(|h| h[3]).collect();
                let degree = [u_knots.len().checked_sub(nu + 1).ok_or(Error::Argument)?, v_knots.len().checked_sub(nv + 1).ok_or(Error::Argument)?];
                Surface::nurbs(degree, nu, nv, u_knots, v_knots, &flat, Some(&weights))
            }
            S::PlacedRevolution { .. } => Err(Error::Unsupported),
        }
    }

    /// The UV domain the intersection runs within, as `[u0, u1, v0, v1]`.
    pub fn domain(&self) -> Result<[f64; 4]> {
        if !ssi_on_cpp() {
            return self.core().map(crate::core::ssi::domain);
        }
        ffi::ssi::domain(self.handle()?).map_err(error)
    }

    /// The surface's point at (u, v).
    pub fn evaluate(&self, u: f64, v: f64) -> Result<[f64; 3]> {
        if !ssi_on_cpp() {
            return crate::core::ssi::evaluate(self.core()?, u, v);
        }
        ffi::ssi::evaluate(self.handle()?, u, v).map_err(error)
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

impl Method {
    fn code(self) -> i32 {
        match self {
            Method::Global => 0,
            Method::Advanced => 1,
        }
    }
}

/// A curve's type, SMLib's `SmTsectCurveType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveType {
    /// The method reported no type.
    None,
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
    /// A value this crate does not know.
    Other(i32),
}

impl CurveType {
    fn from_code(c: i32) -> CurveType {
        match c {
            -1 => CurveType::None,
            0 => CurveType::Unknown,
            1 => CurveType::Touching,
            2 => CurveType::Crossing,
            3 => CurveType::Tangent,
            4 => CurveType::Coincident,
            5 => CurveType::NearTangent,
            6 => CurveType::RegionBoundary,
            c => CurveType::Other(c),
        }
    }
}

/// One intersection curve as SMLib returned it: its 3D curve and, where the method returned
/// them, its curves in each surface's UV, each the (rational) B-spline SMLib holds (its lines,
/// circles and ellipses are B-splines too; an intersection curve one SMLib fitted within the
/// approximation tolerance).
#[derive(Clone, Debug)]
pub struct SsiCurve {
    pub curve_type: CurveType,
    /// SMLib's deviation estimate (NaN: none).
    pub deviation: f64,
    /// The 3D curve's natural parameter interval.
    pub interval: [f64; 2],
    pub curve: b::Curve,
    /// On surface a, in a's UV.
    pub uv_a: Option<b::Curve2>,
    /// On surface b, in b's UV.
    pub uv_b: Option<b::Curve2>,
}

/// An isolated point: where a [`CurveType::Touching`] curve starts.
#[derive(Clone, Copy, Debug)]
pub struct SsiPoint {
    pub xyz: [f64; 3],
    pub uv_a: [f64; 2],
    pub uv_b: [f64; 2],
}

/// What an intersection found.
#[derive(Clone, Debug, Default)]
pub struct SsiResult {
    pub curves: Vec<SsiCurve>,
    pub points: Vec<SsiPoint>,
}

/// The 3D B-spline: polynomial where every weight is 1.
pub(crate) fn curve3(c: ffi::ssi::Nurbs) -> b::Curve {
    let points = c.points.as_chunks::<3>().0;
    if c.weights.iter().all(|w| *w == 1.0) {
        b::Curve::Spline { knots: c.knots, points: points.to_vec() }
    } else {
        b::Curve::RationalSpline {
            knots: c.knots,
            homogeneous: points.iter().zip(&c.weights).map(|(p, w)| [p[0] * w, p[1] * w, p[2] * w, *w]).collect(),
        }
    }
}

/// A UV B-spline, as [`curve3`].
pub(crate) fn curve2(c: ffi::ssi::Nurbs) -> b::Curve2 {
    let points = c.points.as_chunks::<2>().0;
    if c.weights.iter().all(|w| *w == 1.0) {
        b::Curve2::Spline { knots: c.knots, points: points.to_vec() }
    } else {
        b::Curve2::RationalSpline { knots: c.knots, homogeneous: points.iter().zip(&c.weights).map(|(p, w)| [p[0] * w, p[1] * w, *w]).collect() }
    }
}

/// A UV curve the method may not have returned (`SMLIB_ERR_EMPTY`).
fn uv(r: &ffi::ssi::ResultHandle, i: i32, which: i32) -> Result<Option<b::Curve2>> {
    match ffi::ssi::curve_nurbs(r, i, which, 2) {
        Ok(c) => Ok(Some(curve2(c))),
        Err(ffi::ERR_EMPTY) => Ok(None),
        Err(s) => Err(error(s)),
    }
}

/// Intersects `a` and `b` within their domains with `method` at approximation tolerance
/// `approx_tol` (<= 0: SMLib's default, 1/1000 of the surfaces' size). An intersector failure is
/// SMLib's status; a curve SMLib does not hold as a B-spline is [`Error::Unsupported`].
pub fn intersect(a: &Surface, b: &Surface, method: Method, approx_tol: f64) -> Result<SsiResult> {
    if !ssi_on_cpp() {
        return crate::core::ssi::intersect(a.core()?, b.core()?, method, approx_tol);
    }
    let r = ffi::ssi::ssi(a.handle()?, b.handle()?, method.code(), approx_tol).map_err(error)?;
    let n = ffi::ssi::curve_count(&r).map_err(error)?;
    let mut curves = Vec::new();
    for i in 0..n {
        let info = ffi::ssi::curve_info(&r, i).map_err(error)?;
        curves.push(SsiCurve {
            curve_type: CurveType::from_code(info.ty),
            deviation: info.deviation,
            interval: info.interval,
            curve: curve3(ffi::ssi::curve_nurbs(&r, i, ffi::ssi::CURVE_3D, 3).map_err(error)?),
            uv_a: uv(&r, i, ffi::ssi::CURVE_UV_A)?,
            uv_b: uv(&r, i, ffi::ssi::CURVE_UV_B)?,
        });
    }
    let n = ffi::ssi::point_count(&r).map_err(error)?;
    let mut points = Vec::new();
    for i in 0..n {
        let (xyz, uv_a, uv_b) = ffi::ssi::point(&r, i).map_err(error)?;
        points.push(SsiPoint { xyz, uv_a, uv_b });
    }
    Ok(SsiResult { curves, points })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plane through a sphere's centre: one great circle.
    #[test]
    fn plane_through_sphere_is_a_circle() {
        #[cfg(feature = "native")]
        let _cpp = route::on_cpp();
        let z = [0.0, 0.0, 1.0];
        let (x, y) = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let plane = Surface::plane([0.0; 3], x, y, [-2.0, 2.0], [-2.0, 2.0]);
        let sphere = Surface::sphere([0.0; 3], x, z, 1.0, [0.0, 360.0], [-90.0, 90.0]);
        let (plane, sphere) = match (plane, sphere) {
            (Err(Error::NotBuilt), _) | (_, Err(Error::NotBuilt)) => return,
            (Ok(p), Ok(s)) => (p, s),
            (p, s) => panic!("{p:?} {s:?}"),
        };
        let r = intersect(&plane, &sphere, Method::Global, 1e-6).expect("intersect");
        assert!(!r.curves.is_empty());
        for c in &r.curves {
            let eval = b::eval::CurveEval::new(&c.curve).expect("a curve");
            let [t0, t1] = c.interval;
            for k in 0..=100 {
                let p = eval.point(t0 + (t1 - t0) * f64::from(k) / 100.0).expect("evaluates");
                assert!((p.x.hypot(p.y) - 1.0).abs() < 1e-5 && p.z.abs() < 1e-5, "{p:?}");
            }
        }
    }

    /// Refused here, before any kernel: alike on either, and with no C++ kernel.
    #[test]
    fn bad_input_is_an_argument_error() {
        let (x, y) = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let refused = [
            Surface::plane([0.0; 3], x, x, [0.0, 1.0], [0.0, 1.0]),
            Surface::plane([0.0; 3], x, y, [1.0, 0.0], [0.0, 1.0]),
            Surface::plane([f64::NAN; 3], x, y, [0.0, 1.0], [0.0, 1.0]),
            Surface::cylinder([0.0; 3], x, y, 0.0, [0.0, 360.0], [0.0, 1.0]),
            Surface::cone([0.0; 3], x, y, f64::NAN, [0.0, 1.0]),
            Surface::sphere([0.0; 3], x, y, 1.0, [0.0, 360.0], [-91.0, 90.0]),
            Surface::torus([0.0; 3], x, y, 1.0, 2.0, [0.0, 360.0], [0.0, 360.0]),
            Surface::nurbs([1, 1], 2, 2, &[0.0, 0.0, 1.0, 1.0], &[0.0, 0.0, 1.0, 1.0], &[[0.0; 3]; 3], None),
            Surface::nurbs([1, 1], 2, 2, &[0.0, 0.0, 1.0], &[0.0, 0.0, 1.0, 1.0], &[[0.0; 3]; 4], None),
            Surface::nurbs([1, 1], 2, 2, &[0.0, 0.0, 1.0, 1.0], &[0.0, 0.0, 1.0, 1.0], &[[0.0; 3]; 4], Some(&[0.0; 4])),
            Surface::nurbs([0, 1], 2, 2, &[0.0, 0.0, 1.0], &[0.0, 0.0, 1.0, 1.0], &[[0.0; 3]; 4], None),
        ];
        for (i, r) in refused.iter().enumerate() {
            assert_eq!(r.as_ref().err(), Some(&Error::Argument), "{i}");
        }
    }

    /// On solidcore a surface needs no C++ kernel, and answers by itself. The basic intersector is
    /// the one method solidcore has no public entry for, and it says so; nothing falls back.
    #[test]
    fn surfaces_on_solidcore_answer_without_the_cpp_kernel() {
        if ssi_on_cpp() {
            return;
        }
        let (x, y, z) = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
        let plane = Surface::plane([0.0; 3], x, y, [-2.0, 2.0], [-3.0, 3.0]).unwrap();
        assert_eq!(plane.domain().unwrap(), [-2.0, 2.0, -3.0, 3.0]);
        assert_eq!(plane.evaluate(1.0, -2.0).unwrap(), [1.0, -2.0, 0.0]);
        assert_eq!(plane.evaluate(f64::NAN, 0.0).unwrap_err(), Error::Argument);
        let sphere = Surface::sphere([0.0; 3], x, z, 1.0, [0.0, 360.0], [-90.0, 90.0]).unwrap();
        for method in [Method::Global, Method::Advanced] {
            let r = intersect(&plane, &sphere, method, 1e-6).unwrap();
            assert!(!r.curves.is_empty(), "{method:?}");
            for c in &r.curves {
                let eval = b::eval::CurveEval::new(&c.curve).unwrap();
                let [t0, t1] = c.interval;
                for k in 0..=100 {
                    let p = eval.point(t0 + (t1 - t0) * f64::from(k) / 100.0).unwrap();
                    assert!((p.x.hypot(p.y) - 1.0).abs() < 1e-5 && p.z.abs() < 1e-5, "{method:?}: {p:?}");
                }
            }
        }
    }
}
