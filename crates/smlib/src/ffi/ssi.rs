//! The surface/surface intersection block of `capi/include/smlib.h` (`smlib_surface_*`,
//! `smlib_ssi*`), declared by hand. Part of the FFI boundary, like its parent: called only
//! through [`crate::ssi`].
#![allow(unsafe_code)]

/// `smlib_surface`, opaque.
#[repr(C)]
pub struct RawSurface {
    _private: [u8; 0],
}

/// `smlib_ssi_result`, opaque.
#[repr(C)]
pub struct RawResult {
    _private: [u8; 0],
}

#[cfg(feature = "native")]
unsafe extern "C" {
    fn smlib_surface_plane(origin: *const f64, xaxis: *const f64, yaxis: *const f64, u: *const f64, v: *const f64, out: *mut *mut RawSurface) -> i32;
    fn smlib_surface_cylinder(
        origin: *const f64,
        xaxis: *const f64,
        yaxis: *const f64,
        radius: f64,
        angle_deg: *const f64,
        height: *const f64,
        out: *mut *mut RawSurface,
    ) -> i32;
    fn smlib_surface_cone(
        apex: *const f64,
        xaxis: *const f64,
        yaxis: *const f64,
        half_angle_deg: f64,
        height: *const f64,
        out: *mut *mut RawSurface,
    ) -> i32;
    fn smlib_surface_sphere(
        center: *const f64,
        xaxis: *const f64,
        yaxis: *const f64,
        radius: f64,
        lon_deg: *const f64,
        lat_deg: *const f64,
        out: *mut *mut RawSurface,
    ) -> i32;
    fn smlib_surface_torus(
        center: *const f64,
        xaxis: *const f64,
        yaxis: *const f64,
        major_radius: f64,
        minor_radius: f64,
        major_deg: *const f64,
        minor_deg: *const f64,
        out: *mut *mut RawSurface,
    ) -> i32;
    fn smlib_surface_nurbs(
        degree_u: i32,
        degree_v: i32,
        nu: i32,
        nv: i32,
        knots_u: *const f64,
        n_knots_u: i64,
        knots_v: *const f64,
        n_knots_v: i64,
        points: *const f64,
        weights: *const f64,
        out: *mut *mut RawSurface,
    ) -> i32;
    fn smlib_surface_free(s: *mut RawSurface);
    fn smlib_surface_domain(s: *const RawSurface, out: *mut f64) -> i32;
    fn smlib_surface_evaluate(s: *const RawSurface, u: f64, v: f64, out: *mut f64) -> i32;
    fn smlib_ssi(a: *const RawSurface, b: *const RawSurface, method: i32, approx_tol: f64, out: *mut *mut RawResult) -> i32;
    fn smlib_ssi_result_free(r: *mut RawResult);
    fn smlib_ssi_curve_count(r: *const RawResult, out: *mut i32) -> i32;
    fn smlib_ssi_curve_info(r: *const RawResult, i: i32, ty: *mut i32, flags: *mut i32, deviation: *mut f64, interval: *mut f64) -> i32;
    fn smlib_ssi_curve_nurbs(
        r: *const RawResult,
        i: i32,
        which: i32,
        degree: *mut i32,
        n_points: *mut i64,
        n_knots: *mut i64,
        cap_points: i64,
        points: *mut f64,
        weights: *mut f64,
        cap_knots: i64,
        knots: *mut f64,
    ) -> i32;
    fn smlib_ssi_point_count(r: *const RawResult, out: *mut i32) -> i32;
    fn smlib_ssi_point(r: *const RawResult, i: i32, xyz: *mut f64, uv_a: *mut f64, uv_b: *mut f64) -> i32;
}

/// An owned `smlib_surface`, freed once on drop.
#[cfg_attr(not(feature = "native"), allow(dead_code))]
pub struct SurfaceHandle(std::ptr::NonNull<RawSurface>);

/// An owned `smlib_ssi_result`, freed once on drop.
#[cfg_attr(not(feature = "native"), allow(dead_code))]
pub struct ResultHandle(std::ptr::NonNull<RawResult>);

// Only ever used through the C API, which serialises every call.
unsafe impl Send for SurfaceHandle {}
unsafe impl Sync for SurfaceHandle {}
unsafe impl Send for ResultHandle {}
unsafe impl Sync for ResultHandle {}

impl SurfaceHandle {
    #[cfg_attr(not(feature = "native"), allow(dead_code))]
    pub(crate) fn raw(&self) -> *const RawSurface {
        self.0.as_ptr()
    }
}

impl Drop for SurfaceHandle {
    fn drop(&mut self) {
        #[cfg(feature = "native")]
        unsafe {
            smlib_surface_free(self.0.as_ptr())
        };
    }
}

impl Drop for ResultHandle {
    fn drop(&mut self) {
        #[cfg(feature = "native")]
        unsafe {
            smlib_ssi_result_free(self.0.as_ptr())
        };
    }
}

/// One curve's `smlib_ssi_curve_info`.
#[derive(Clone, Copy, Debug)]
pub struct CurveInfo {
    pub ty: i32,
    pub deviation: f64,
    pub interval: [f64; 2],
}

/// `smlib_ssi_curve_nurbs` `which`: the 3D curve, the UV curve on a, on b.
pub const CURVE_3D: i32 = 0;
pub const CURVE_UV_A: i32 = 1;
pub const CURVE_UV_B: i32 = 2;

/// One B-spline as `smlib_ssi_curve_nurbs` gives it: cartesian control points, `dim` values each,
/// one weight each, and every knot (as many as points + degree + 1).
#[derive(Clone, Debug, Default)]
pub struct Nurbs {
    pub points: Vec<f64>,
    pub weights: Vec<f64>,
    pub knots: Vec<f64>,
}

#[cfg(feature = "native")]
mod calls {
    use super::*;
    use crate::ffi::{ERR_ARG, ERR_EMPTY, OK};

    fn new_surface(f: impl FnOnce(*mut *mut RawSurface) -> i32) -> Result<SurfaceHandle, i32> {
        let mut out: *mut RawSurface = std::ptr::null_mut();
        match (f(&mut out), std::ptr::NonNull::new(out)) {
            (OK, Some(p)) => Ok(SurfaceHandle(p)),
            (OK, None) => Err(ERR_EMPTY),
            (s, _) => Err(s),
        }
    }

    pub fn plane(origin: &[f64; 3], xaxis: &[f64; 3], yaxis: &[f64; 3], u: &[f64; 2], v: &[f64; 2]) -> Result<SurfaceHandle, i32> {
        new_surface(|out| unsafe { smlib_surface_plane(origin.as_ptr(), xaxis.as_ptr(), yaxis.as_ptr(), u.as_ptr(), v.as_ptr(), out) })
    }
    pub fn cylinder(
        origin: &[f64; 3],
        xaxis: &[f64; 3],
        yaxis: &[f64; 3],
        radius: f64,
        angle_deg: &[f64; 2],
        height: &[f64; 2],
    ) -> Result<SurfaceHandle, i32> {
        new_surface(|out| unsafe {
            smlib_surface_cylinder(origin.as_ptr(), xaxis.as_ptr(), yaxis.as_ptr(), radius, angle_deg.as_ptr(), height.as_ptr(), out)
        })
    }
    pub fn cone(apex: &[f64; 3], xaxis: &[f64; 3], yaxis: &[f64; 3], half_angle_deg: f64, height: &[f64; 2]) -> Result<SurfaceHandle, i32> {
        new_surface(|out| unsafe { smlib_surface_cone(apex.as_ptr(), xaxis.as_ptr(), yaxis.as_ptr(), half_angle_deg, height.as_ptr(), out) })
    }
    pub fn sphere(
        center: &[f64; 3],
        xaxis: &[f64; 3],
        yaxis: &[f64; 3],
        radius: f64,
        lon_deg: &[f64; 2],
        lat_deg: &[f64; 2],
    ) -> Result<SurfaceHandle, i32> {
        new_surface(|out| unsafe {
            smlib_surface_sphere(center.as_ptr(), xaxis.as_ptr(), yaxis.as_ptr(), radius, lon_deg.as_ptr(), lat_deg.as_ptr(), out)
        })
    }
    pub fn torus(
        center: &[f64; 3],
        xaxis: &[f64; 3],
        yaxis: &[f64; 3],
        major_radius: f64,
        minor_radius: f64,
        major_deg: &[f64; 2],
        minor_deg: &[f64; 2],
    ) -> Result<SurfaceHandle, i32> {
        new_surface(|out| unsafe {
            smlib_surface_torus(
                center.as_ptr(),
                xaxis.as_ptr(),
                yaxis.as_ptr(),
                major_radius,
                minor_radius,
                major_deg.as_ptr(),
                minor_deg.as_ptr(),
                out,
            )
        })
    }
    /// `points` holds `nu * nv` points (3 values each), `weights` `nu * nv` or none; the C side
    /// checks every count against the degrees and sizes.
    pub fn nurbs(
        degree: [i32; 2],
        nu: i32,
        nv: i32,
        knots_u: &[f64],
        knots_v: &[f64],
        points: &[f64],
        weights: Option<&[f64]>,
    ) -> Result<SurfaceHandle, i32> {
        let n = i64::from(nu).checked_mul(i64::from(nv)).ok_or(ERR_ARG)?;
        let fits = |len: usize, per: i64| n.checked_mul(per).is_some_and(|need| i64::try_from(len).is_ok_and(|l| l == need));
        if !fits(points.len(), 3) || weights.is_some_and(|w| !fits(w.len(), 1)) {
            return Err(ERR_ARG);
        }
        let ku = i64::try_from(knots_u.len()).map_err(|_| ERR_ARG)?;
        let kv = i64::try_from(knots_v.len()).map_err(|_| ERR_ARG)?;
        let w = weights.map_or(std::ptr::null(), <[f64]>::as_ptr);
        new_surface(|out| unsafe {
            smlib_surface_nurbs(degree[0], degree[1], nu, nv, knots_u.as_ptr(), ku, knots_v.as_ptr(), kv, points.as_ptr(), w, out)
        })
    }
    pub fn domain(s: &SurfaceHandle) -> Result<[f64; 4], i32> {
        let mut d = [0.0; 4];
        match unsafe { smlib_surface_domain(s.0.as_ptr(), d.as_mut_ptr()) } {
            OK => Ok(d),
            st => Err(st),
        }
    }
    pub fn evaluate(s: &SurfaceHandle, u: f64, v: f64) -> Result<[f64; 3], i32> {
        let mut p = [0.0; 3];
        match unsafe { smlib_surface_evaluate(s.0.as_ptr(), u, v, p.as_mut_ptr()) } {
            OK => Ok(p),
            st => Err(st),
        }
    }
    pub fn ssi(a: &SurfaceHandle, b: &SurfaceHandle, method: i32, approx_tol: f64) -> Result<ResultHandle, i32> {
        let mut out: *mut RawResult = std::ptr::null_mut();
        match (unsafe { smlib_ssi(a.0.as_ptr(), b.0.as_ptr(), method, approx_tol, &mut out) }, std::ptr::NonNull::new(out)) {
            (OK, Some(p)) => Ok(ResultHandle(p)),
            (OK, None) => Err(ERR_EMPTY),
            (s, _) => Err(s),
        }
    }
    pub fn curve_count(r: &ResultHandle) -> Result<i32, i32> {
        let mut n = 0;
        match unsafe { smlib_ssi_curve_count(r.0.as_ptr(), &mut n) } {
            OK => Ok(n),
            s => Err(s),
        }
    }
    pub fn curve_info(r: &ResultHandle, i: i32) -> Result<CurveInfo, i32> {
        let mut c = CurveInfo { ty: 0, deviation: 0.0, interval: [0.0; 2] };
        match unsafe { smlib_ssi_curve_info(r.0.as_ptr(), i, &mut c.ty, std::ptr::null_mut(), &mut c.deviation, c.interval.as_mut_ptr()) } {
            OK => Ok(c),
            s => Err(s),
        }
    }
    /// Curve `i`'s `which` B-spline, `dim` coordinates per control point: the counts first, then
    /// the arrays.
    pub fn curve_nurbs(r: &ResultHandle, i: i32, which: i32, dim: usize) -> Result<Nurbs, i32> {
        let (mut degree, mut np, mut nk) = (0i32, 0i64, 0i64);
        let null = std::ptr::null_mut();
        let st = unsafe { smlib_ssi_curve_nurbs(r.0.as_ptr(), i, which, &mut degree, &mut np, &mut nk, 0, null, null, 0, null) };
        if st != OK {
            return Err(st);
        }
        let (n, k) = (usize::try_from(np).map_err(|_| ERR_ARG)?, usize::try_from(nk).map_err(|_| ERR_ARG)?);
        let degree = usize::try_from(degree).map_err(|_| ERR_ARG)?;
        if !(n > degree && k == n + degree + 1) {
            return Err(ERR_ARG);
        }
        let mut c = Nurbs { points: vec![0.0; n.checked_mul(dim).ok_or(ERR_ARG)?], weights: vec![0.0; n], knots: vec![0.0; k] };
        let st = unsafe {
            smlib_ssi_curve_nurbs(
                r.0.as_ptr(),
                i,
                which,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                np,
                c.points.as_mut_ptr(),
                c.weights.as_mut_ptr(),
                nk,
                c.knots.as_mut_ptr(),
            )
        };
        if st == OK { Ok(c) } else { Err(st) }
    }
    pub fn point_count(r: &ResultHandle) -> Result<i32, i32> {
        let mut n = 0;
        match unsafe { smlib_ssi_point_count(r.0.as_ptr(), &mut n) } {
            OK => Ok(n),
            s => Err(s),
        }
    }
    pub fn point(r: &ResultHandle, i: i32) -> Result<([f64; 3], [f64; 2], [f64; 2]), i32> {
        let (mut p, mut a, mut b) = ([0.0; 3], [0.0; 2], [0.0; 2]);
        match unsafe { smlib_ssi_point(r.0.as_ptr(), i, p.as_mut_ptr(), a.as_mut_ptr(), b.as_mut_ptr()) } {
            OK => Ok((p, a, b)),
            s => Err(s),
        }
    }
}

/// The same calls when SMLib is not built: each one fails with `NOT_BUILT`.
#[cfg(not(feature = "native"))]
mod calls {
    use super::*;
    use crate::ffi::NOT_BUILT;

    pub fn plane(_: &[f64; 3], _: &[f64; 3], _: &[f64; 3], _: &[f64; 2], _: &[f64; 2]) -> Result<SurfaceHandle, i32> {
        Err(NOT_BUILT)
    }
    pub fn cylinder(_: &[f64; 3], _: &[f64; 3], _: &[f64; 3], _: f64, _: &[f64; 2], _: &[f64; 2]) -> Result<SurfaceHandle, i32> {
        Err(NOT_BUILT)
    }
    pub fn cone(_: &[f64; 3], _: &[f64; 3], _: &[f64; 3], _: f64, _: &[f64; 2]) -> Result<SurfaceHandle, i32> {
        Err(NOT_BUILT)
    }
    pub fn sphere(_: &[f64; 3], _: &[f64; 3], _: &[f64; 3], _: f64, _: &[f64; 2], _: &[f64; 2]) -> Result<SurfaceHandle, i32> {
        Err(NOT_BUILT)
    }
    pub fn torus(_: &[f64; 3], _: &[f64; 3], _: &[f64; 3], _: f64, _: f64, _: &[f64; 2], _: &[f64; 2]) -> Result<SurfaceHandle, i32> {
        Err(NOT_BUILT)
    }
    pub fn nurbs(_: [i32; 2], _: i32, _: i32, _: &[f64], _: &[f64], _: &[f64], _: Option<&[f64]>) -> Result<SurfaceHandle, i32> {
        Err(NOT_BUILT)
    }
    pub fn domain(_: &SurfaceHandle) -> Result<[f64; 4], i32> {
        Err(NOT_BUILT)
    }
    pub fn evaluate(_: &SurfaceHandle, _: f64, _: f64) -> Result<[f64; 3], i32> {
        Err(NOT_BUILT)
    }
    pub fn ssi(_: &SurfaceHandle, _: &SurfaceHandle, _: i32, _: f64) -> Result<ResultHandle, i32> {
        Err(NOT_BUILT)
    }
    pub fn curve_count(_: &ResultHandle) -> Result<i32, i32> {
        Err(NOT_BUILT)
    }
    pub fn curve_info(_: &ResultHandle, _: i32) -> Result<CurveInfo, i32> {
        Err(NOT_BUILT)
    }
    pub fn curve_nurbs(_: &ResultHandle, _: i32, _: i32, _: usize) -> Result<Nurbs, i32> {
        Err(NOT_BUILT)
    }
    pub fn point_count(_: &ResultHandle) -> Result<i32, i32> {
        Err(NOT_BUILT)
    }
    pub fn point(_: &ResultHandle, _: i32) -> Result<([f64; 3], [f64; 2], [f64; 2]), i32> {
        Err(NOT_BUILT)
    }
}

pub use calls::*;
