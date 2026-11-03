//! Safe wrappers for `surface.surface_p11`; shim: `surface_surface_p11.cpp`.
//! Upstream: `source/SMLib/src/SmSurface.cpp` (`SmSurface::LocalSurfaceIntersect`).
//!
//! Each surface is a non-rational biquadratic Bezier patch (kind 0, a 3x3 net over
//! `[0,1]^2`), an analytic `SmPlane` (kind 1: origin `net[0]`, x axis `net[1]`, y axis
//! `net[2]`, unit scales), a canonical analytic `SmTorus` (kind 2: centre `net[0]`,
//! sweep start `net[1]`, 90 degree direction `net[2]`, `(major, minor)` radii
//! `net[3][0..2]` over the full analytic STEP domain `[0, 360]^2` degrees, its NURB
//! domain being the case's own `[0, 1]^2`; `net[3][2]` and the remaining net values are
//! unused) or a canonical surface of revolution of a line generator (kind 3: axis point
//! `net[0]`, axis direction `net[1]`, line start `net[2]`, line end `net[3]`, swap flag
//! `net[4][0]`; the line's STEP interval is `[0, 1]`, so the surface's NURB domain is
//! `[0, 1]` along the generator and the sweep circle's arc length along the
//! revolution, and the reverse with `bSwapUV`). Every value must be finite; plane axes must be non-zero and not
//! parallel (`SmAxis2Placement::SetCanonical` fails on them and leaves the axes
//! undefined); a torus needs both radii above `SM_EFF_ZERO` (`SmTorus::CreateCanonical`
//! refuses others); a revolution needs a line generator of non-zero length and a
//! rotation frame (`CreateCanonical` returns `SM_ERR` for a generator on the axis).
//! Torus placement axes and the revolution axis are left to `SetCanonical`'s and
//! `Unitize`'s defined handling: non-unit and non-perpendicular (including parallel)
//! axes are normalized by `Unitize` and `MakeUnitOrthoVectors`, and an axis whose
//! squared length is below `SM_EFF_ZERO_SQ` is stored as `(0,0,1)` by `Unitize` rather
//! than left undefined, so the placement constructor still builds a frame. Those guards
//! therefore apply no axis length or parallelism threshold, unlike the plane guard.
//! Domains must not be inverted. No counts cross the FFI except the control point count
//! of [`lift_curve`]'s inline UV curve, checked against its arrays.

use std::ffi::CString;

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
        pub fn sc_surface_surface_p11_lift(
            surf_path: *const std::ffi::c_char,
            kind: i32,
            xyz: *const f64,
            has_domain: i32,
            domain: *const f64,
            curve_path: *const std::ffi::c_char,
            degree: i32,
            ncp: i64,
            cpts: *const f64,
            knots: *const f64,
            has_interval: i32,
            interval: *const f64,
            tol: f64,
            skip_checks: i32,
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
    let xyz: Vec<f64> = a
        .net
        .iter()
        .chain(b.net.iter())
        .flatten()
        .copied()
        .collect();
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
    if !(0..=3).contains(&s.kind)
        || s.net.iter().flatten().any(|v| !v.is_finite())
        || s.domain.iter().any(|v| !v.is_finite())
        || s.domain[0] > s.domain[2]
        || s.domain[1] > s.domain[3]
        || (s.kind == 1 && !plane_ok(&s.net))
        || (s.kind == 2 && !torus_ok(&s.net))
        || (s.kind == 3 && !revolution_ok(&s.net))
    {
        return Err(FfiError::InvalidArgument("unsafe surface p11 input".into()));
    }
    Ok(())
}

/// One surface of a case: kind 0 (Bezier patch), 1 (plane), 2 (canonical torus) or 3
/// (canonical surface of revolution of a line generator), and its 3x3 net.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceInput {
    /// 0: biquadratic Bezier patch; 1: plane (origin, x axis, y axis in `net[0..3]`);
    /// 2: canonical torus (centre, sweep start, 90 degree direction in `net[0..3]`,
    /// `(major, minor)` radii in `net[3][0..2]`); 3: canonical surface of revolution
    /// (axis point, axis direction, line start, line end in `net[0]`..`net[3]`,
    /// `net[4][0]` non-zero for `bSwapUV`).
    pub kind: i32,
    /// The control net (row major, `u` rows).
    pub net: [[f64; 3]; 9],
    /// `[umin, vmin, umax, vmax]`: the intersection domain (the torus's and the
    /// revolution's NURB domain).
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

/// `SM_EFF_ZERO` (`crates/solidcore/src/math.rs`; this crate does not depend on the
/// kernel). `SmTorus::CreateCanonical` refuses a radius that is not above it.
const EFF_ZERO: f64 = 1.0e-12;

/// A canonical-torus payload: both radii strictly above `SM_EFF_ZERO`, the bound below
/// which `SmTorus::CreateCanonical` refuses a radius. The placement axes are not
/// constrained:
/// `SmAxis2Placement::SetCanonical` normalizes non-unit and non-perpendicular axes and
/// `Unitize` stores `(0,0,1)` for a zero-length axis, so a short or parallel axis still
/// yields a defined frame and rejecting it would reject an input the native constructor
/// defines. In particular the plane guard's length and parallelism thresholds do not
/// apply here.
fn torus_ok(net: &[[f64; 3]; 9]) -> bool {
    net[3][0] > EFF_ZERO && net[3][1] > EFF_ZERO
}

/// `SM_EFF_ZERO_SQ` (`crates/solidcore/src/math.rs`): the squared bound `Unitize` and
/// `IsParallelTo` compare against.
const EFF_ZERO_SQ: f64 = EFF_ZERO * EFF_ZERO;

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length_squared(v: [f64; 3]) -> f64 {
    dot(v, v)
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// `Unitize()` as `SmSurfOfRevolution::CreateCanonical` uses it (its status is ignored):
/// a vector below `SM_EFF_ZERO_SQ` becomes `(0,0,1)`.
fn unitize_or_z(v: [f64; 3]) -> [f64; 3] {
    let len_sq = length_squared(v);
    if len_sq < EFF_ZERO_SQ {
        return [0.0, 0.0, 1.0];
    }
    let len = len_sq.sqrt();
    [v[0] / len, v[1] / len, v[2] / len]
}

/// `IsParallelTo(other, tol_deg)`: false when either vector is not above
/// `SM_EFF_ZERO_SQ`; otherwise the smaller angle to the other's line is within
/// `tol_deg`.
fn parallel_within(v: [f64; 3], o: [f64; 3], tol_deg: f64) -> bool {
    let (v_sq, o_sq) = (length_squared(v), length_squared(o));
    if v_sq <= EFF_ZERO_SQ || o_sq <= EFF_ZERO_SQ {
        return false;
    }
    let cos = dot(v, o) / (v_sq.sqrt() * o_sq.sqrt());
    cos.clamp(-1.0, 1.0).abs().acos() < tol_deg.to_radians()
}

/// A canonical surface of revolution of a line generator (`CreateCanonical`): the line
/// needs a non-zero length (`SmLine::CreateLineSegment` unitizes it and fails
/// otherwise) and `CreateCanonical` needs a rotation frame, which it cannot build when
/// the generator lies on the axis (its `y_vec` cross product is zero and it returns
/// `SM_ERR`). The frame construction is repeated here exactly: the axis is unitized
/// with `Unitize`'s `(0,0,1)` fallback, the start point at the line's STEP interval
/// minimum is tried, then 45% of the interval when that point is on the axis or
/// parallel to it. A zero-length axis is defined (it becomes `(0,0,1)`), so it is not
/// rejected.
fn revolution_ok(net: &[[f64; 3]; 9]) -> bool {
    let step = [
        net[3][0] - net[2][0],
        net[3][1] - net[2][1],
        net[3][2] - net[2][2],
    ];
    if length_squared(step) < EFF_ZERO_SQ {
        return false;
    }
    let axis = unitize_or_z(net[1]);
    let mut x = [
        net[2][0] - net[0][0],
        net[2][1] - net[0][1],
        net[2][2] - net[0][2],
    ];
    if length_squared(x).sqrt() < EFF_ZERO || parallel_within(x, axis, 1.0) {
        // CreateCanonical calls SmLine::EvaluateSTEP, which evaluates the line as
        // start + t * unit_direction * scale. Keep that order at the frame bound.
        let scale = length_squared(step).sqrt();
        let unit = unitize_or_z(step);
        x = [
            net[2][0] + 0.45 * unit[0] * scale - net[0][0],
            net[2][1] + 0.45 * unit[1] * scale - net[0][1],
            net[2][2] + 0.45 * unit[2] * scale - net[0][2],
        ];
    }
    length_squared(cross(axis, x)) >= EFF_ZERO_SQ
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
    let xyz: Vec<f64> = a
        .net
        .iter()
        .chain(b.net.iter())
        .flatten()
        .copied()
        .collect();
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

/// The surface of [`lift_curve`].
#[derive(Debug, Clone, PartialEq)]
pub enum LiftSurface {
    /// `SmBSplineSurface::ReadFromFile(path)` (an upstream test file; its contents are
    /// trusted, not validated).
    File(String),
    /// A patch, plane, canonical torus or canonical surface of revolution as for
    /// [`local_surface_intersect`] (its `domain` is not used: [`lift_curve`] takes the
    /// domain separately).
    Inline(Box<SurfaceInput>),
}

/// The UV curve of [`lift_curve`].
#[derive(Debug, Clone, PartialEq)]
pub enum LiftUvCurve {
    /// `SmBSplineCurve::ReadFromFile(path)` (an upstream test file; trusted).
    File(String),
    /// A non-rational 2D B-spline: degree, control points `(u, v)` and knots.
    Inline {
        /// Degree, `1..=8`.
        degree: i64,
        /// Control points, `degree + 1 ..= 64` of them.
        cpts: Vec<[f64; 2]>,
        /// `cpts.len() + degree + 1` clamped non-decreasing knots, interior
        /// multiplicity at most `degree`.
        knots: Vec<f64>,
    },
}

/// Largest magnitude of an inline UV control point or knot.
pub const MAX_UV: f64 = 1e6;

fn path(p: &str) -> Result<CString, FfiError> {
    CString::new(p).map_err(|_| FfiError::InvalidArgument("path with NUL".into()))
}

/// Clamped, non-decreasing, finite knots with interior multiplicity at most `p`.
fn knots_ok(knots: &[f64], ncp: usize, p: usize) -> bool {
    let (Some(&k0), Some(&km)) = (knots.first(), knots.last()) else {
        return false;
    };
    if knots.len() != ncp + p + 1
        || knots.iter().any(|k| !(k.is_finite() && k.abs() <= MAX_UV))
        || knots.windows(2).any(|w| w.first() > w.last())
        || km <= k0
        || knots.iter().take(p + 1).any(|&k| k != k0)
        || knots.iter().rev().take(p + 1).any(|&k| k != km)
    {
        return false;
    }
    let mut run = 0usize;
    let mut last = f64::NAN;
    for &k in knots.iter().filter(|&&k| k != k0 && k != km) {
        run = if k == last { run + 1 } else { 1 };
        last = k;
        if run > p {
            return false;
        }
    }
    true
}

/// `surface.LiftCurve(ctx, domain, uv_curve, interval, tol, max_dist, curve,
/// skip_checks)` (`domain` / `interval` default to the natural ones): `[status,
/// max_dist, type, dim, degree, ncp, cpts (x y z w), nknots, knots, mults, interval
/// min, max]` (only the status when `LiftCurve` fails).
///
/// # Errors
/// Rejected before C++: an invalid inline surface (as [`local_surface_intersect`]), an
/// inline UV curve outside the limits of [`LiftUvCurve::Inline`] or with control points
/// that are not finite or larger than [`MAX_UV`], an inverted or non-finite domain, an
/// interval that is inverted, not finite or (for an inline curve) not inside the
/// curve's knot range, a tolerance that is not finite and positive (`SmApproxTol3d`
/// treats other values as its default). Shim failures (a file that does not read)
/// return `FfiError::Status`.
#[allow(clippy::too_many_arguments)]
pub fn lift_curve(
    surface: &LiftSurface,
    domain: Option<[f64; 4]>,
    uv_curve: &LiftUvCurve,
    interval: Option<[f64; 2]>,
    tol: f64,
    skip_continuity_checks: bool,
) -> Result<Vec<f64>, FfiError> {
    let invalid = || FfiError::InvalidArgument("unsafe surface p11 lift input".into());
    let (surf_path, kind, xyz) = match surface {
        LiftSurface::File(p) => (Some(path(p)?), 0, vec![0.0; 27]),
        LiftSurface::Inline(s) => {
            check_surface(&SurfaceInput {
                domain: [0.0, 0.0, 1.0, 1.0],
                ..**s
            })?;
            (None, s.kind, s.net.iter().flatten().copied().collect())
        }
    };
    if let Some(d) = domain {
        if d.iter().any(|v| !v.is_finite()) || d[0] > d[2] || d[1] > d[3] {
            return Err(invalid());
        }
    }
    if !(tol.is_finite() && tol > 0.0) {
        return Err(invalid());
    }
    let (curve_path, degree, cpts, knots) = match uv_curve {
        LiftUvCurve::File(p) => (Some(path(p)?), 0, Vec::new(), Vec::new()),
        LiftUvCurve::Inline {
            degree,
            cpts,
            knots,
        } => {
            let p = usize::try_from(*degree).map_err(|_| invalid())?;
            if !(1..=8).contains(&p)
                || !(p + 1..=64).contains(&cpts.len())
                || cpts
                    .iter()
                    .flatten()
                    .any(|v| !(v.is_finite() && v.abs() <= MAX_UV))
                || !knots_ok(knots, cpts.len(), p)
            {
                return Err(invalid());
            }
            if let Some(i) = interval {
                let (k0, km) = (
                    knots.first().copied().unwrap_or(f64::NAN),
                    knots.last().copied().unwrap_or(f64::NAN),
                );
                if !(k0 <= i[0] && i[1] <= km) {
                    return Err(invalid());
                }
            }
            (None, p as i32, cpts.iter().flatten().copied().collect(), knots.clone())
        }
    };
    if let Some(i) = interval {
        if i.iter().any(|v| !v.is_finite()) || i[0] > i[1] {
            return Err(invalid());
        }
    }
    let dom = domain.unwrap_or([0.0; 4]);
    let ivl = interval.unwrap_or([0.0; 2]);
    let ncp = (cpts.len() / 2) as i64;
    let null = std::ptr::null::<std::ffi::c_char>();
    call_with_buffer(1024, |buffer, len| {
        // SAFETY: paths are NUL-terminated C strings or NULL; `xyz` holds 27 values,
        // `dom` 4, `ivl` 2; an inline curve's `cpts` holds `2 * ncp` and `knots`
        // `ncp + degree + 1` values (checked above); the shim copies output only within
        // the buffer capacity.
        unsafe {
            sys::sc_surface_surface_p11_lift(
                surf_path.as_ref().map_or(null, |p| p.as_ptr()),
                kind,
                xyz.as_ptr(),
                i32::from(domain.is_some()),
                dom.as_ptr(),
                curve_path.as_ref().map_or(null, |p| p.as_ptr()),
                degree,
                ncp,
                cpts.as_ptr(),
                knots.as_ptr(),
                i32::from(interval.is_some()),
                ivl.as_ptr(),
                tol,
                i32::from(skip_continuity_checks),
                buffer.as_mut_ptr(),
                buffer.len() as i64,
                &raw mut *len,
            )
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A canonical torus (kind 2): centre and unit placement axes in `net[0..3]`,
    /// `(major, minor)` radii in `net[3][0..2]`.
    fn canonical_torus(major: f64, minor: f64) -> SurfaceInput {
        SurfaceInput {
            kind: 2,
            net: [
                [1.0, 2.0, 3.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [major, minor, 0.0],
                [0.0; 3],
                [0.0; 3],
                [0.0; 3],
                [0.0; 3],
                [0.0; 3],
            ],
            domain: [0.0, 0.0, 1.0, 1.0],
        }
    }

    fn accepted(s: &SurfaceInput) -> bool {
        check_surface(s).is_ok()
    }

    #[test]
    fn canonical_torus_guard_zero_x_axis_with_unit_y_axis_accepted() {
        let mut s = canonical_torus(2.0, 0.5);
        s.net[1] = [0.0; 3];
        assert!(accepted(&s));
    }

    #[test]
    fn canonical_torus_guard_tiny_x_axis_with_unit_y_axis_accepted() {
        let mut s = canonical_torus(2.0, 0.5);
        s.net[1] = [1e-13, 0.0, 0.0];
        assert!(accepted(&s));
    }

    #[test]
    fn canonical_torus_guard_non_perpendicular_unit_axes_accepted() {
        let mut s = canonical_torus(2.0, 0.5);
        s.net[1] = [1.0, 0.0, 0.0];
        s.net[2] = [0.6, 0.8, 0.0];
        assert!(accepted(&s));
    }

    #[test]
    fn canonical_torus_guard_parallel_unit_axes_accepted() {
        let mut s = canonical_torus(2.0, 0.5);
        s.net[1] = [1.0, 0.0, 0.0];
        s.net[2] = [1.0, 0.0, 0.0];
        assert!(accepted(&s));
    }

    #[test]
    fn canonical_torus_guard_non_positive_radii_rejected() {
        for (major, minor) in [(0.0, 0.5), (-1.0, 0.5), (2.0, 0.0), (2.0, -0.5), (-1.0, -1.0)] {
            assert!(!accepted(&canonical_torus(major, minor)));
        }
    }

    #[test]
    fn canonical_torus_guard_equal_eff_zero_radii_rejected() {
        for (major, minor) in [(EFF_ZERO, 2.0), (2.0, EFF_ZERO), (EFF_ZERO, EFF_ZERO)] {
            assert!(!accepted(&canonical_torus(major, minor)));
        }
    }

    #[test]
    fn canonical_torus_guard_nan_radius_rejected() {
        let mut s = canonical_torus(2.0, 0.5);
        s.net[3][1] = f64::NAN;
        assert!(!accepted(&s));
    }

    #[test]
    fn canonical_torus_guard_plane_zero_axis_rejected() {
        let mut s = canonical_torus(2.0, 0.5);
        s.kind = 1;
        s.net[1] = [0.0; 3];
        assert!(!accepted(&s));
    }

    #[test]
    fn canonical_torus_guard_plane_parallel_axes_rejected() {
        let mut s = canonical_torus(2.0, 0.5);
        s.kind = 1;
        s.net[1] = [1.0, 0.0, 0.0];
        s.net[2] = [1.0, 0.0, 0.0];
        assert!(!accepted(&s));
    }

    #[test]
    fn canonical_torus_guard_finite_bezier_patch_accepted() {
        let mut s = canonical_torus(2.0, 0.5);
        s.kind = 0;
        s.net = [[0.5; 3]; 9];
        assert!(accepted(&s));
    }

    /// A canonical surface of revolution of a line generator (kind 3): axis point
    /// `net[0]`, axis direction `net[1]`, line start `net[2]`, line end `net[3]`,
    /// `net[4][0]` the swap flag.
    fn canonical_revolution() -> SurfaceInput {
        SurfaceInput {
            kind: 3,
            net: [
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                [1.0, 0.0, 0.0],
                [1.0, 0.0, 2.0],
                [0.0; 3],
                [0.0; 3],
                [0.0; 3],
                [0.0; 3],
                [0.0; 3],
            ],
            domain: [0.5, 0.1, 5.7, 0.9],
        }
    }

    #[test]
    fn canonical_revolution_guard_cylinder_accepted() {
        assert!(accepted(&canonical_revolution()));
    }

    #[test]
    fn canonical_revolution_guard_zero_axis_accepted() {
        // `CreateCanonical` calls `Unitize()` and ignores its status; a zero axis
        // becomes (0,0,1), so the constructor still builds a defined frame.
        let mut s = canonical_revolution();
        s.net[1] = [0.0; 3];
        assert!(accepted(&s));
    }

    #[test]
    fn canonical_revolution_guard_non_unit_and_oblique_axis_accepted() {
        let mut s = canonical_revolution();
        s.net[1] = [0.1, 0.2, 5.0];
        assert!(accepted(&s));
    }

    #[test]
    fn canonical_revolution_guard_swap_flag_accepted() {
        let mut s = canonical_revolution();
        s.net[4][0] = 1.0;
        assert!(accepted(&s));
        s.net[4][0] = -2.5;
        assert!(accepted(&s));
    }

    #[test]
    fn canonical_revolution_guard_start_on_axis_with_oblique_generator_accepted() {
        // The start point lies on the axis, so `CreateCanonical` retries at 45% of
        // the generator's STEP interval, where the line is off the axis.
        let mut s = canonical_revolution();
        s.net[2] = [0.0, 0.0, 0.0];
        s.net[3] = [1.0, 0.0, 2.0];
        assert!(accepted(&s));
    }

    #[test]
    fn canonical_revolution_guard_retry_preserves_step_evaluation_order() {
        // CreateCanonical retries through SmLine::EvaluateSTEP: t * unit * scale,
        // not t * raw_step. Here the latter rounds to EFF_ZERO and accepts the
        // frame, while the pinned operation order remains one ulp below it.
        let mut s = canonical_revolution();
        s.net[2] = [0.0, 0.0, 0.0];
        s.net[3] = [2.222222222222222e-12, 0.0, 0.7];
        assert!(!accepted(&s));
    }

    #[test]
    fn canonical_revolution_guard_zero_length_generator_rejected() {
        // `SmLine::CreateLineSegment` unitizes the generator vector and fails on a
        // zero-length one, so the fixture would not construct.
        let mut s = canonical_revolution();
        s.net[3] = s.net[2];
        assert!(!accepted(&s));
    }

    #[test]
    fn canonical_revolution_guard_generator_on_axis_rejected() {
        // A generator coincident with the axis makes `CreateCanonical` return SM_ERR
        // before it builds a surface.
        let mut s = canonical_revolution();
        s.net[2] = [0.0, 0.0, 0.5];
        s.net[3] = [0.0, 0.0, 1.5];
        assert!(!accepted(&s));
    }

    #[test]
    fn canonical_revolution_guard_non_finite_generator_rejected() {
        let mut s = canonical_revolution();
        s.net[2][0] = f64::NAN;
        assert!(!accepted(&s));
    }

    #[test]
    fn canonical_revolution_guard_inverted_domain_rejected() {
        let mut s = canonical_revolution();
        s.domain = [5.7, 0.1, 0.5, 0.9];
        assert!(!accepted(&s));
    }
}
