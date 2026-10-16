//! Safe wrappers for `surface.surface_p11`; shim: `surface_surface_p11.cpp`.
//! Upstream: `source/SMLib/src/SmSurface.cpp` (`SmSurface::LocalSurfaceIntersect`).
//!
//! Each surface is a non-rational biquadratic Bezier patch (kind 0, a 3x3 net over
//! `[0,1]^2`) or an analytic `SmPlane` (kind 1: origin `net[0]`, x axis `net[1]`, y axis
//! `net[2]`, unit scales). Every value must be finite; plane axes must be non-zero and
//! not parallel (`SmAxis2Placement::SetCanonical` fails on them and leaves the axes
//! undefined); domains must not be inverted. No counts cross the FFI.
use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_surface_surface_p11_lsi(
            kinds: *const i32,
            xyz: *const f64,
            domains: *const f64,
            guesses: *const f64,
            tol: f64,
            has_normal: i32,
            plane_normal: *const f64,
            has_previous: i32,
            previous_uv: *const f64,
            boundary_testing: i32,
            output: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_surface_surface_p11_isb(
            kinds: *const i32,
            xyz: *const f64,
            domains: *const f64,
            tol: f64,
            output: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// `a.IntersectSurfaceBoundaries(ctx, a.domain, b, b.domain, tol, ..)`: `[status, n,
/// per curve: deviation, type, interval min, max, 5 points]`.
///
/// # Errors
/// Invalid data is rejected before C++ (as [`local_surface_intersect`], plus a
/// positive tolerance: `SmApproxTol3d` treats other values as its default);
/// shim failures return `FfiError::Status`.
pub fn intersect_surface_boundaries(
    a: &SurfaceInput,
    b: &SurfaceInput,
    tol: f64,
) -> Result<Vec<f64>, FfiError> {
    for s in [a, b] {
        check_surface(s)?;
    }
    if !(tol.is_finite() && tol > 0.0) {
        return Err(FfiError::InvalidArgument("unsafe surface p11 input".into()));
    }
    let kinds = [a.kind, b.kind];
    let xyz: Vec<f64> = a.net.iter().chain(b.net.iter()).flatten().copied().collect();
    let domains: Vec<f64> = a.domain.iter().chain(b.domain.iter()).copied().collect();
    call_with_buffer(64, |buffer, len| {
        // SAFETY: fixed-length inputs (2 kinds, 54 net values, 8 domain values) checked
        // above; the shim copies output only within the buffer capacity.
        unsafe {
            sys::sc_surface_surface_p11_isb(
                kinds.as_ptr(),
                xyz.as_ptr(),
                domains.as_ptr(),
                tol,
                buffer.as_mut_ptr(),
                buffer.len() as i64,
                &raw mut *len,
            )
        }
    })
}

fn check_surface(s: &SurfaceInput) -> Result<(), FfiError> {
    if !(0..=1).contains(&s.kind)
        || s.net.iter().flatten().any(|v| !v.is_finite())
        || s.domain.iter().any(|v| !v.is_finite())
        || s.domain[0] > s.domain[2]
        || s.domain[1] > s.domain[3]
        || (s.kind == 1 && !plane_ok(&s.net))
    {
        return Err(FfiError::InvalidArgument("unsafe surface p11 input".into()));
    }
    Ok(())
}

/// One surface of a case: kind 0 (Bezier patch) or 1 (plane), and its 3x3 net.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceInput {
    /// 0: biquadratic Bezier patch; 1: plane (origin, x axis, y axis in `net[0..3]`).
    pub kind: i32,
    /// The control net (row major, `u` rows).
    pub net: [[f64; 3]; 9],
    /// `[umin, vmin, umax, vmax]`: the intersection domain.
    pub domain: [f64; 4],
}

fn plane_ok(net: &[[f64; 3]; 9]) -> bool {
    let [x, y] = [net[1], net[2]];
    let cross = [
        x[1] * y[2] - x[2] * y[1],
        x[2] * y[0] - x[0] * y[2],
        x[0] * y[1] - x[1] * y[0],
    ];
    let len = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let (lx, ly, lc) = (len(x), len(y), len(cross));
    lx > 1e-9 && ly > 1e-9 && lc > 1e-6 * lx * ly && lx.is_finite() && ly.is_finite()
}

/// `a.LocalSurfaceIntersect(a.domain, b, b.domain, tol, guess_a, guess_b, normal,
/// previous, boundary_testing)`: `[status, found, u, v, u2, v2, deviation]` (the
/// solution only when found).
///
/// # Errors
/// Invalid data is rejected before C++; shim failures return `FfiError::Status`.
pub fn local_surface_intersect(
    a: &SurfaceInput,
    b: &SurfaceInput,
    tol: f64,
    guesses: [f64; 4],
    plane_normal: Option<[f64; 3]>,
    previous_uv: Option<[f64; 2]>,
    boundary_testing: bool,
) -> Result<Vec<f64>, FfiError> {
    let invalid = || FfiError::InvalidArgument("unsafe surface p11 input".into());
    for s in [a, b] {
        check_surface(s)?;
    }
    if !tol.is_finite()
        || guesses.iter().any(|v| !v.is_finite())
        || plane_normal.is_some_and(|n| n.iter().any(|v| !v.is_finite()))
        || previous_uv.is_some_and(|p| p.iter().any(|v| !v.is_finite()))
    {
        return Err(invalid());
    }
    let kinds = [a.kind, b.kind];
    let xyz: Vec<f64> = a.net.iter().chain(b.net.iter()).flatten().copied().collect();
    let domains: Vec<f64> = a.domain.iter().chain(b.domain.iter()).copied().collect();
    let normal = plane_normal.unwrap_or([0.0; 3]);
    let previous = previous_uv.unwrap_or([0.0; 2]);
    call_with_buffer(16, |buffer, len| {
        // SAFETY: fixed-length inputs (2 kinds, 54 net values, 8 domain values, 4
        // guesses, 3 normal and 2 previous values) checked above; the shim copies
        // output only within the buffer capacity.
        unsafe {
            sys::sc_surface_surface_p11_lsi(
                kinds.as_ptr(),
                xyz.as_ptr(),
                domains.as_ptr(),
                guesses.as_ptr(),
                tol,
                i32::from(plane_normal.is_some()),
                normal.as_ptr(),
                i32::from(previous_uv.is_some()),
                previous.as_ptr(),
                i32::from(boundary_testing),
                buffer.as_mut_ptr(),
                buffer.len() as i64,
                &raw mut *len,
            )
        }
    })
}
