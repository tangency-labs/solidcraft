//! Surface/surface intersection (`capi/include/smlib.h`, "Surface/surface intersection"): two
//! surfaces described in plain numbers, intersected by one of SMLib's SSI entry points, each
//! intersection curve exactly as SMLib holds it: schema 4 B-splines (`solidcraft_geom::brep`).
//!
//! The constructors take exactly the fields of an SSI corpus case (`tests/ssi-corpus`), angles in
//! degrees. Each surface's UV domain is SMLib's natural one ([`Surface::domain`]): (u, v) itself
//! for planes, the parameterisation of SMLib's internal NURBS form for the other analytic kinds.

use solidcore_record::brep as b;

use std::sync::OnceLock;

use crate::{Error, Result};

/// A surface with its UV domain: the description it was made from, from which solidcore makes its
/// own ([`crate::core::ssi`]).
pub struct Surface {
    /// solidcore's, made when it is first needed.
    core: OnceLock<Result<crate::core::ssi::Tool>>,
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

/// The largest coordinate, degree and control net a surface may have (`SMLIB_SURFACE_MAX_*`).
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
    /// refused before solidcore sees it.
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
}

impl Surface {
    /// solidcore's surface, made the first time it is needed.
    fn core(&self) -> Result<&crate::core::ssi::Tool> {
        self.core.get_or_init(|| crate::stop::unstoppable(|| crate::core::ssi::surface(&self.spec))).as_ref().map_err(Clone::clone)
    }

    pub(crate) fn spec(&self) -> &Spec {
        &self.spec
    }

    /// A surface from its checked description.
    fn made(spec: Spec) -> Result<Surface> {
        spec.check()?;
        Ok(Surface { core: OnceLock::new(), spec })
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
    /// normal, except a cone that narrows along its axis: that one is built on the mirrored frame,
    /// where u and v both run reversed (the normal is unchanged). A spline is taken over its whole
    /// knot domain. A cone only as a whole turn; a revolution not at all ([`Error::Unsupported`]).
    pub fn from_record(surface: &solidcore_record::brep::Surface, domain: [[f64; 2]; 2]) -> Result<Surface> {
        use solidcore_record::brep::Surface as S;
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
                if !(t != 0.0 && t.is_finite()) || !(u[1] - u[0] >= std::f64::consts::TAU * (1.0 - 1e-12)) {
                    return Err(Error::Unsupported);
                }
                // SMLib's cone widens along its axis. A record cone with a negative half angle
                // narrows along the frame's z (docs/native-brep.md), so it is built on the mirrored
                // frame (y negated: x × -y = -z), along which it widens, with its heights mirrored
                // (as the kernel's `surf_of` reads such a cone).
                let (y_axis, t, v) = if t > 0.0 { (frame.y_axis, t, v) } else { (frame.y_axis.map(|c| -c), -t, [-v[1], -v[0]]) };
                // The apex lies where the radius is 0; heights run from it along the axis.
                let at = -radius_at_origin / t;
                let z = cross(frame.x_axis, y_axis);
                let apex = [0, 1, 2].map(|k| frame.origin[k] + z[k] * at);
                Surface::cone(apex, frame.x_axis, y_axis, t.atan().to_degrees(), [(v[0] - at).max(0.0), v[1] - at])
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
            // A ball patch's label has no (u, v): its face's NURBS form is the surface.
            S::PlacedRevolution { .. } | S::BallPatch { .. } | S::Blend { .. } | S::Sweep { .. } => Err(Error::Unsupported),
        }
    }

    /// The UV domain the intersection runs within, as `[u0, u1, v0, v1]`.
    pub fn domain(&self) -> Result<[f64; 4]> {
        self.core().map(crate::core::ssi::domain)
    }

    /// The surface's point at (u, v).
    pub fn evaluate(&self, u: f64, v: f64) -> Result<[f64; 3]> {
        crate::core::ssi::evaluate(self.core()?, u, v)
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

/// Intersects `a` and `b` within their domains with `method` at approximation tolerance
/// `approx_tol` (<= 0: SMLib's default, 1/1000 of the surfaces' size). An intersector failure is
/// SMLib's status; a curve SMLib does not hold as a B-spline is [`Error::Unsupported`].
pub fn intersect(a: &Surface, b: &Surface, method: Method, approx_tol: f64) -> Result<SsiResult> {
    crate::core::ssi::intersect(a.core()?, b.core()?, method, approx_tol)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Refused here, before solidcore sees the surface.
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

    /// #13917: a record cone with a negative half angle narrows along its frame's z axis
    /// (docs/native-brep.md); it converts like a widening one, and every point of the surface
    /// over its domain has the record's radius at its height.
    #[test]
    fn a_record_cone_narrowing_along_its_axis_converts() {
        use solidcore_record::brep::{Frame, Surface as S};
        let frame = Frame { origin: [1.0, 2.0, 3.0], x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 1.0, 0.0] };
        for half in [0.2f64, -0.2] {
            let rec = S::Cone { frame: frame.clone(), radius_at_origin: 5.0, half_angle: half };
            let s = Surface::from_record(&rec, [[0.0, std::f64::consts::TAU], [-4.0, 6.0]]).unwrap_or_else(|e| panic!("half {half}: {e}"));
            let [u0, u1, v0, v1] = s.domain().unwrap();
            let (mut lo, mut hi) = (f64::MAX, f64::MIN);
            for i in 0..=8 {
                for j in 0..=8 {
                    let p = s.evaluate(u0 + (u1 - u0) * f64::from(i) / 8.0, v0 + (v1 - v0) * f64::from(j) / 8.0).unwrap();
                    let h = p[2] - 3.0;
                    let r = ((p[0] - 1.0).powi(2) + (p[1] - 2.0).powi(2)).sqrt();
                    assert!((r - (5.0 + h * half.tan())).abs() < 1e-9, "half {half}: ({},{},{}) r {r} at h {h}", p[0], p[1], p[2]);
                    lo = lo.min(h);
                    hi = hi.max(h);
                }
            }
            assert!((lo + 4.0).abs() < 1e-9 && (hi - 6.0).abs() < 1e-9, "half {half}: heights {lo}..{hi}");
        }
    }

    /// A plane's domain and points are its own parameters; a plane through a sphere's centre meets
    /// it in one great circle, by either method.
    #[test]
    fn a_plane_through_a_spheres_centre_is_a_great_circle() {
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
                let eval = solidcraft_geom::brep::eval::CurveEval::new(&c.curve).unwrap();
                let [t0, t1] = c.interval;
                for k in 0..=100 {
                    let p = eval.point(t0 + (t1 - t0) * f64::from(k) / 100.0).unwrap();
                    assert!((p.x.hypot(p.y) - 1.0).abs() < 1e-5 && p.z.abs() < 1e-5, "{method:?}: {p:?}");
                }
            }
        }
    }

    /// The adapter is solidcore-ops' surface/surface intersection with nothing changed: each
    /// pair's domains, points and intersections through [`Surface`] and [`intersect`] are bit
    /// for bit what [`solidcore_ops::surface_intersection`] gives for the same numbers, by both
    /// methods, at a given tolerance and at the default one.
    #[test]
    fn routed_intersections_are_solidcore_ops() {
        use solidcore_ops::surface_intersection as ops;
        let (x, y, z) = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
        let bits = |v: &[f64]| v.iter().map(|f| f.to_bits()).collect::<Vec<_>>();
        let net: Vec<[f64; 3]> = (0..16).map(|i| [f64::from(i / 4) - 1.5, f64::from(i % 4) - 1.5, 0.2 * f64::from((i * 7) % 5) - 0.4]).collect();
        let weights: Vec<f64> = (0..16).map(|i| 1.0 + 0.1 * f64::from(i % 3)).collect();
        let knots = [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
        let specs = [
            Spec::Plane { origin: [0.0, 0.0, 0.1], xaxis: x, yaxis: [0.0, 2.0, 0.0], u: [-2.0, 2.0], v: [-3.0, 3.0] },
            Spec::Cylinder { origin: [0.2, 0.0, -1.0], xaxis: x, yaxis: y, radius: 0.8, angle_deg: [0.0, 360.0], height: [-1.0, 2.0] },
            Spec::Cone { apex: [0.0, 0.0, -2.0], xaxis: x, yaxis: y, half_angle_deg: 30.0, height: [0.5, 4.0] },
            Spec::Sphere { center: [0.1, 0.2, 0.3], xaxis: x, yaxis: z, radius: 1.0, lon_deg: [0.0, 360.0], lat_deg: [-90.0, 90.0] },
            Spec::Torus {
                center: [0.0; 3],
                xaxis: x,
                yaxis: y,
                major_radius: 1.5,
                minor_radius: 0.4,
                major_deg: [0.0, 360.0],
                minor_deg: [0.0, 360.0],
            },
            Spec::Nurbs { degree: [3, 3], nu: 4, nv: 4, knots_u: knots.to_vec(), knots_v: knots.to_vec(), points: net.clone(), weights: None },
            Spec::Nurbs { degree: [3, 3], nu: 4, nv: 4, knots_u: knots.to_vec(), knots_v: knots.to_vec(), points: net, weights: Some(weights) },
        ];
        let made: Vec<(Surface, ops::Patch)> =
            specs.iter().map(|spec| (Surface::made(spec.clone()).unwrap(), ops::Patch::new(&crate::core::ssi::patch_spec(spec)).unwrap())).collect();
        for (routed, direct) in &made {
            let d = routed.domain().unwrap();
            assert_eq!(bits(&d), bits(&direct.domain()));
            for (fu, fv) in [(0.0, 0.0), (0.3, 0.7), (1.0, 1.0), (1.25, -0.5)] {
                let (u, v) = (d[0] + (d[1] - d[0]) * fu, d[2] + (d[3] - d[2]) * fv);
                assert_eq!(bits(&routed.evaluate(u, v).unwrap()), bits(&direct.point(u, v).unwrap()));
            }
        }
        let mut compared = 0;
        for (i, (ra, da)) in made.iter().enumerate() {
            for (rb, db) in made.iter().skip(i + 1) {
                for (method, m) in [(Method::Global, ops::Method::Global), (Method::Advanced, ops::Method::Advanced)] {
                    for tol in [1e-6, 0.0] {
                        let routed = intersect(ra, rb, method, tol);
                        let direct = ops::intersect(da, db, m, (tol > 0.0).then_some(tol));
                        let (routed, direct) = match (routed, direct) {
                            (Ok(r), Ok(d)) => (r, d),
                            (Err(_), Err(_)) => continue,
                            (r, d) => panic!("one of them failed: routed {:?}, direct {:?}", r.err(), d.err()),
                        };
                        assert_eq!(routed.curves.len(), direct.curves.len());
                        for (r, d) in routed.curves.iter().zip(&direct.curves) {
                            assert_eq!(r.curve_type, crate::core::ssi::curve_type(d.curve_type));
                            assert_eq!(bits(&[r.deviation]), bits(&[d.deviation]));
                            assert_eq!(bits(&r.interval), bits(&d.interval));
                            assert_eq!(format!("{:?}", r.curve), format!("{:?}", d.curve));
                            assert_eq!(format!("{:?}", r.uv_a), format!("{:?}", d.uv_a));
                            assert_eq!(format!("{:?}", r.uv_b), format!("{:?}", d.uv_b));
                        }
                        assert_eq!(routed.points.len(), direct.points.len());
                        for (r, d) in routed.points.iter().zip(&direct.points) {
                            assert_eq!([bits(&r.xyz), bits(&r.uv_a), bits(&r.uv_b)], [bits(&d.xyz), bits(&d.uv_a), bits(&d.uv_b)]);
                        }
                        compared += 1;
                    }
                }
            }
        }
        // every pair, by both methods at both tolerances: 7 patches make 21 pairs
        assert_eq!(compared, 21 * 2 * 2, "an intersection failed both ways");
    }
}
