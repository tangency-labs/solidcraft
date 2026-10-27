//! FFI wrappers for unit `surface.surface_cache_p2`.
//!
//! Shim: `harness/cpp/shim/surface_surface_cache_p2.cpp` (`sc_surface_surface_cache_p2_eval`).
//! Upstream: `source/SMLib/src/SmSurfaceCache.cpp`.
//!
//! Notation: `B` is a B-spline surface block `oob form n m p q r s cpts(x y z w) u v`
//! (`SmBSplineSurface(gw_SURFACE*, FALSE, NULL)`, control points with the v index fastest), `N`
//! a bare NURB `n m p q r s cpts u v`, `S` a surface `kind ..` (`0 B`: B-spline; `1 o x y sx sy
//! D`: bounded `SmPlane(o, x, y, (sx, sy), D)`; `2 B`: `SmCurveBoundedSurface` with implicit
//! outer boundary and no boundary curves over the B-spline; `3 B` / `4 B`: `0` / `2` with
//! the B-spline's knots kept as given), `V` a vector `x y z`, `D` a domain
//! `umin vmin umax vmax`, `K` alternate cache user data and `C` a curve as in
//! [`super::surface_surface_cache_p1`], `O` the cache options `chord angtol_rad aspect
//! max_side min_side min_ratio start_fast_angle_deg`.
//!
//! | [`Op`]          | upstream                                         | parameters              |
//! |-----------------|--------------------------------------------------|-------------------------|
//! | `Nth`           | `SmPatchBoundaryBoundingPlanes::GetNthBoundingPlane` (k calls on one object) | `S D k index..` |
//! | `Add`           | `AddCurvesToCache`, then `GetCopyOfCurves` and `MatchCache` of every cache | `S na` x (`type approx angle K n3 C.. nuv C..`) `nq` x (`type approx angle K`) |
//! | `BezBlock`      | `sm_CreateBezierPatchBlock`                      | `B`                     |
//! | `ApproxBlock`   | `sm_CreateApproximateBezierPatchBlock`           | `S`                     |
//! | `Test`          | `TestAgainstTolerances` on a hand-built leaf     | `S D O aux children chsat angsat D [aux 3: N lo0 hi0 lo1 hi1 lo2 hi2]` |
//! | `SubdivBlock`   | `SubdivideByBlock` on a hand-built `SM_AD_AUX_DATA` root | `S O D nosurf`   |
//!
//! The output is the op status (`0` = `SM_SUCCESS`, else the `SmStatus`) and the op's
//! outputs (see the shim). [`eval`] accepts only what upstream handles without undefined
//! behaviour or unbounded work: well-formed clamped surfaces and curves, planes with
//! independent axes and moderate scales, domains inside the surface domain, bounding plane
//! indices `0..=3`, valid enumerators, bounded counts, finite values; the `Test` node is an
//! `SM_AD_BEZIER_SURFACE` (with its Bezier patch: upstream reads it unchecked),
//! `SM_AD_BEZIER_SURFACE_HEAD` or `SM_AD_AUX_DATA` node, the latter only over a B-spline
//! (upstream casts `mBA_pSurface` to `SmBSplineSurface` and reads its NURB, which a plane
//! does not have yet); `SubdivBlock` runs over B-splines and planes (upstream casts the
//! surface) unless the node has no surface, with tolerances that keep the tessellation
//! splits bounded.

use super::surface_surface_cache_p1::{bad, bspline_block, curves, key, surface, Reader, Srf};
use crate::{call_with_buffer, FfiError};

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `GetNthBoundingPlane`.
    Nth,
    /// `AddCurvesToCache`.
    Add,
    /// `sm_CreateBezierPatchBlock`.
    BezBlock,
    /// `sm_CreateApproximateBezierPatchBlock`.
    ApproxBlock,
    /// `TestAgainstTolerances`.
    Test,
    /// `SubdivideByBlock`.
    SubdivBlock,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Op::Nth => 0,
            Op::Add => 1,
            Op::BezBlock => 2,
            Op::ApproxBlock => 3,
            Op::Test => 4,
            Op::SubdivBlock => 5,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_surface_surface_cache_p2_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// The surface kinds of an `S` block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    BSpline,
    Plane,
    CurveBounded,
}

/// Squared length of a vector.
fn len2(v: [f64; 3]) -> f64 {
    v[0] * v[0] + v[1] * v[1] + v[2] * v[2]
}

/// An `S` block; returns its kind and natural domain.
fn surface_spec(rd: &mut Reader<'_>) -> Result<(Kind, Srf), FfiError> {
    // 3 / 4: 0 / 2 with the B-spline's knots kept as given
    match rd.index(4)? {
        1 => {
            rd.vec3()?;
            let x = rd.vec3()?;
            let y = rd.vec3()?;
            let (lx, ly) = (len2(x), len2(y));
            if !(1e-6..=1e6).contains(&lx) || !(1e-6..=1e6).contains(&ly) {
                return Err(bad("plane: axis length out of range"));
            }
            let c = [
                x[1] * y[2] - x[2] * y[1],
                x[2] * y[0] - x[0] * y[2],
                x[0] * y[1] - x[1] * y[0],
            ];
            if len2(c) < 1e-6 * lx * ly {
                return Err(bad("plane: axes (nearly) parallel"));
            }
            for _ in 0..2 {
                let s = rd.finite()?;
                if !(1e-3..=1e3).contains(&s) {
                    return Err(bad("plane: scale out of range"));
                }
            }
            let d = rd.domain()?;
            if d[2] - d[0] < 1e-6 || d[3] - d[1] < 1e-6 || d.iter().any(|v| v.abs() > 1e4) {
                return Err(bad("plane: domain size out of range"));
            }
            Ok((
                Kind::Plane,
                Srf {
                    dom: d,
                    u_distinct: 2,
                    v_distinct: 2,
                },
            ))
        }
        k => {
            let srf = bspline_block(rd)?;
            Ok((
                if k == 0 || k == 3 {
                    Kind::BSpline
                } else {
                    Kind::CurveBounded
                },
                srf,
            ))
        }
    }
}

/// The cache options `O`; returns `(chord, angtol_rad, max_side)`.
fn options(rd: &mut Reader<'_>) -> Result<(f64, f64, f64), FfiError> {
    let v = rd.finites(7)?;
    let get = |i: usize| v.get(i).copied().unwrap_or(0.0);
    Ok((get(0), get(1), get(3)))
}

fn validate_op(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match op {
        Op::Nth => {
            let (_, srf) = surface_spec(rd)?;
            rd.domain_in(&srf, false)?;
            let k = rd.index(8)?;
            for _ in 0..k {
                rd.index(3)?;
            }
        }
        Op::Add => {
            surface_spec(rd)?;
            let na = rd.index(8)?;
            for _ in 0..na {
                rd.index(1)?;
                rd.finite()?;
                rd.finite()?;
                key(rd)?;
                curves(rd)?;
                curves(rd)?;
            }
            let nq = rd.index(8)?;
            for _ in 0..nq {
                rd.index(1)?;
                rd.finite()?;
                rd.finite()?;
                key(rd)?;
            }
        }
        Op::BezBlock => {
            bspline_block(rd)?;
        }
        Op::ApproxBlock => {
            surface_spec(rd)?;
        }
        Op::Test => {
            let (kind, srf) = surface_spec(rd)?;
            rd.domain()?;
            options(rd)?;
            let aux = rd.index(6)?;
            if !matches!(aux, 3 | 4 | 6) {
                return Err(bad("test: aux type must be 3, 4 or 6"));
            }
            if aux == 6 && kind != Kind::BSpline {
                return Err(bad("test: SM_AD_AUX_DATA nodes need a B-spline surface"));
            }
            rd.flag()?;
            rd.flag()?;
            rd.flag()?;
            rd.domain_in(&srf, true)?;
            if aux == 3 {
                surface(rd)?;
                for _ in 0..3 {
                    let lo = rd.finite()?;
                    let hi = rd.finite()?;
                    if lo > hi {
                        return Err(bad("test: pseudo box interval not ordered"));
                    }
                }
            }
        }
        Op::SubdivBlock => {
            let (kind, srf) = surface_spec(rd)?;
            let (chord, ang, max_side) = options(rd)?;
            rd.domain_in(&srf, false)?;
            let nosurf = rd.flag()?;
            if !nosurf && kind == Kind::CurveBounded {
                return Err(bad("subdivide by block: the surface must be a B-spline"));
            }
            // keep the tessellation splits bounded
            if chord < 0.0 || (chord > 0.0 && chord < 1e-3) {
                return Err(bad("subdivide by block: chord tolerance too small"));
            }
            if ang < 0.0 || (ang > 0.0 && ang < 0.05) {
                return Err(bad("subdivide by block: angle tolerance too small"));
            }
            if max_side < 0.0 || (max_side > 0.0 && max_side < 0.05) {
                return Err(bad("subdivide by block: max side length too small"));
            }
        }
    }
    Ok(())
}

/// Checks `input` for `op` (layouts in the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
    validate_op(op, &mut rd)?;
    rd.done()
}

/// Runs `op` on `input` (layouts in the module docs); returns the shim's output record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see; [`FfiError::Status`]
/// for a shim failure.
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(65536, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated above: well-formed clamped surfaces and curves, planes
        // with independent axes, domains inside the surface domain, bounding plane indices
        // 0..=3, every count and enumerator the shim passes to upstream, node kinds whose
        // data upstream reads safely); buf is valid for buf.len() writes and the shim
        // honours the capacity; len is valid.
        unsafe {
            sys::sc_surface_surface_cache_p2_eval(
                op.code(),
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
