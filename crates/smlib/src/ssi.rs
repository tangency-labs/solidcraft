//! Surface/surface intersection (`capi/include/smlib.h`, "Surface/surface intersection"): two
//! surfaces described in plain numbers, intersected by one of SMLib's SSI entry points, each
//! intersection curve sampled uniformly in its parameters.
//!
//! The constructors take exactly the fields of an SSI corpus case (`tests/ssi-corpus`), angles in
//! degrees. Each surface's UV domain is SMLib's natural one ([`Surface::domain`]): (u, v) itself
//! for planes, the parameterisation of SMLib's internal NURBS form for the other analytic kinds.

use crate::{Error, Result, error, ffi, init};

/// `SMLIB_SSI_MAX_SAMPLES`: most samples per curve.
pub const MAX_SAMPLES: usize = 100_000;

/// A surface with its UV domain, owned on the C side.
pub struct Surface(ffi::ssi::SurfaceHandle);

impl std::fmt::Debug for Surface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("smlib::ssi::Surface")
    }
}

fn int(n: usize) -> Result<i32> {
    i32::try_from(n).map_err(|_| Error::Argument)
}

impl Surface {
    /// `origin + u*x + v*y` (x, y the axes made unit), for u in `u`, v in `v`.
    pub fn plane(origin: [f64; 3], xaxis: [f64; 3], yaxis: [f64; 3], u: [f64; 2], v: [f64; 2]) -> Result<Surface> {
        init()?;
        ffi::ssi::plane(&origin, &xaxis, &yaxis, &u, &v).map(Surface).map_err(error)
    }

    /// A cylinder of `radius` about the axis through `origin` along `xaxis × yaxis`, angle
    /// `angle_deg` from `xaxis` towards `yaxis`, for heights `height` measured from `origin`.
    pub fn cylinder(origin: [f64; 3], xaxis: [f64; 3], yaxis: [f64; 3], radius: f64, angle_deg: [f64; 2], height: [f64; 2]) -> Result<Surface> {
        init()?;
        ffi::ssi::cylinder(&origin, &xaxis, &yaxis, radius, &angle_deg, &height).map(Surface).map_err(error)
    }

    /// A full cone with its apex at `apex` along `xaxis × yaxis`, radius `h * tan(half_angle_deg)`
    /// at distance h along the axis, for h in `height` (from 0: through the apex).
    pub fn cone(apex: [f64; 3], xaxis: [f64; 3], yaxis: [f64; 3], half_angle_deg: f64, height: [f64; 2]) -> Result<Surface> {
        init()?;
        ffi::ssi::cone(&apex, &xaxis, &yaxis, half_angle_deg, &height).map(Surface).map_err(error)
    }

    /// A sphere of `radius` at `center`, over longitudes `lon_deg` and latitudes `lat_deg`.
    pub fn sphere(center: [f64; 3], xaxis: [f64; 3], yaxis: [f64; 3], radius: f64, lon_deg: [f64; 2], lat_deg: [f64; 2]) -> Result<Surface> {
        init()?;
        ffi::ssi::sphere(&center, &xaxis, &yaxis, radius, &lon_deg, &lat_deg).map(Surface).map_err(error)
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
        init()?;
        ffi::ssi::torus(&center, &xaxis, &yaxis, major_radius, minor_radius, &major_deg, &minor_deg).map(Surface).map_err(error)
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
        init()?;
        let flat: Vec<f64> = points.iter().flatten().copied().collect();
        ffi::ssi::nurbs([int(degree[0])?, int(degree[1])?], int(nu)?, int(nv)?, knots_u, knots_v, &flat, weights).map(Surface).map_err(error)
    }

    /// The UV domain the intersection runs within, as `[u0, u1, v0, v1]`.
    pub fn domain(&self) -> Result<[f64; 4]> {
        ffi::ssi::domain(&self.0).map_err(error)
    }

    /// The surface's point at (u, v).
    pub fn evaluate(&self, u: f64, v: f64) -> Result<[f64; 3]> {
        ffi::ssi::evaluate(&self.0, u, v).map_err(error)
    }
}

/// Which of SMLib's intersectors runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// `SmSurface::GlobalSurfaceIntersect`: analytic special cases, else [`Method::Advanced`].
    Global,
    /// `SmAdvSurfaceIntersector`, the general NURBS path.
    Advanced,
    /// `SmSurfaceIntersector`, the basic intersector.
    Basic,
}

impl Method {
    fn code(self) -> i32 {
        match self {
            Method::Global => 0,
            Method::Advanced => 1,
            Method::Basic => 2,
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

/// One intersection curve, sampled at the same count of parameters on the 3D curve and on each
/// UV curve. A value SMLib could not evaluate is NaN, and so is every UV value (with the surface
/// at it) of a UV curve the method did not return ([`SsiCurve::has_uv_a`]).
#[derive(Clone, Debug)]
pub struct SsiCurve {
    pub curve_type: CurveType,
    pub has_uv_a: bool,
    pub has_uv_b: bool,
    /// SMLib's deviation estimate (NaN: none).
    pub deviation: f64,
    /// The 3D curve's natural parameter interval, which `xyz` samples uniformly.
    pub interval: [f64; 2],
    pub xyz: Vec<[f64; 3]>,
    pub uv_a: Vec<[f64; 2]>,
    pub uv_b: Vec<[f64; 2]>,
    /// Surface a at `uv_a`, b at `uv_b`.
    pub a_at_uv: Vec<[f64; 3]>,
    pub b_at_uv: Vec<[f64; 3]>,
}

/// An isolated point: a [`CurveType::Touching`] curve's first sample.
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

fn triples(v: &[f64]) -> Vec<[f64; 3]> {
    v.as_chunks::<3>().0.to_vec()
}

fn pairs(v: &[f64]) -> Vec<[f64; 2]> {
    v.as_chunks::<2>().0.to_vec()
}

/// Intersects `a` and `b` within their domains with `method` at approximation tolerance
/// `approx_tol` (<= 0: SMLib's default, 1/1000 of the surfaces' size), sampling each curve at
/// `samples` parameters (2..=[`MAX_SAMPLES`]). An intersector failure is SMLib's status.
pub fn intersect(a: &Surface, b: &Surface, method: Method, approx_tol: f64, samples: usize) -> Result<SsiResult> {
    if !(2..=MAX_SAMPLES).contains(&samples) {
        return Err(Error::Argument);
    }
    let r = ffi::ssi::ssi(&a.0, &b.0, method.code(), approx_tol, int(samples)?).map_err(error)?;
    let n = ffi::ssi::curve_count(&r).map_err(error)?;
    let mut curves = Vec::new();
    for i in 0..n {
        let info = ffi::ssi::curve_info(&r, i).map_err(error)?;
        let count = usize::try_from(info.samples).map_err(|_| Error::Argument)?;
        let s = ffi::ssi::curve_samples(&r, i, count).map_err(error)?;
        curves.push(SsiCurve {
            curve_type: CurveType::from_code(info.ty),
            has_uv_a: info.flags & 1 != 0,
            has_uv_b: info.flags & 2 != 0,
            deviation: info.deviation,
            interval: info.interval,
            xyz: triples(&s.xyz),
            uv_a: pairs(&s.uv_a),
            uv_b: pairs(&s.uv_b),
            a_at_uv: triples(&s.a_at_uv),
            b_at_uv: triples(&s.b_at_uv),
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
        let z = [0.0, 0.0, 1.0];
        let (x, y) = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let plane = Surface::plane([0.0; 3], x, y, [-2.0, 2.0], [-2.0, 2.0]);
        let sphere = Surface::sphere([0.0; 3], x, z, 1.0, [0.0, 360.0], [-90.0, 90.0]);
        let (plane, sphere) = match (plane, sphere) {
            (Err(Error::NotBuilt), _) | (_, Err(Error::NotBuilt)) => return,
            (Ok(p), Ok(s)) => (p, s),
            (p, s) => panic!("{p:?} {s:?}"),
        };
        let r = intersect(&plane, &sphere, Method::Global, 1e-6, 101).expect("intersect");
        assert!(!r.curves.is_empty());
        for c in &r.curves {
            assert_eq!(c.xyz.len(), 101);
            for p in &c.xyz {
                assert!((p[0].hypot(p[1]) - 1.0).abs() < 1e-5 && p[2].abs() < 1e-5, "{p:?}");
            }
        }
        assert_eq!(intersect(&plane, &sphere, Method::Global, 1e-6, 1).err(), Some(Error::Argument));
    }

    #[test]
    fn bad_input_is_an_argument_error() {
        match Surface::plane([0.0; 3], [1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0], [0.0, 1.0]) {
            Err(Error::NotBuilt) => {}
            r => assert_eq!(r.err(), Some(Error::Argument)),
        }
        match Surface::nurbs([1, 1], 2, 2, &[0.0, 0.0, 1.0, 1.0], &[0.0, 0.0, 1.0, 1.0], &[[0.0; 3]; 3], None) {
            Err(Error::NotBuilt) => {}
            r => assert_eq!(r.err(), Some(Error::Argument)),
        }
    }
}
