//! FFI wrappers for unit `geom.vector3d`.
//!
//! Shim: `harness/cpp/shim/geom_vector3d.cpp`.
//! Upstream: `source/SMLib/inc/SmVector3d.h`, `source/SMLib/src/SmVector3d.cpp`
//! (`SmVector3d`), `source/SMLib/src/SmMath.cpp` (`sm_sqrt`, `sm_cbrt`, `sm_acos`,
//! `sm_asin`).
//!
//! | Rust   | shim                  | upstream                           |
//! |--------|-----------------------|------------------------------------|
//! | [`op`] | `sc_geom_vector3d_op` | one `SmVector3d` method per [`Op`] |

use crate::{check, FfiError};

mod sys {
    extern "C" {
        pub fn sc_geom_vector3d_op(
            op: i32,
            v: *const f64,
            s: f64,
            idx: i32,
            flag: i32,
            out: *mut f64,
            out_n: *mut i32,
        ) -> i32;
    }
}

/// Most values one operation returns.
pub const MAX_OUT: usize = 10;

/// One `SmVector3d` operation on vectors `a`..`e`, scalar `s`, index `idx`, `flag`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Op {
    /// `a.LengthSquared()`.
    LengthSquared = 0,
    /// `a.Length()`.
    Length = 1,
    /// `a.Dot(b)`.
    Dot = 2,
    /// `a * b` (cross product).
    Cross = 3,
    /// `-a`.
    Neg = 4,
    /// `a + b`.
    Add = 5,
    /// `a - b`.
    Sub = 6,
    /// `a * s`.
    Mul = 7,
    /// `s * a`.
    LMul = 8,
    /// `a / s`.
    Div = 9,
    /// `a.Unitize()` (status or vector).
    Unitize = 10,
    /// `a.Unitize(FALSE)` with the status ignored.
    UnitizeOrZ = 11,
    /// `a.TripleProduct(b, c)`.
    TripleProduct = 12,
    /// `a.NormalInPlane(b)`.
    NormalInPlane = 13,
    /// `a.GetMaxDimension()`.
    MaxDimension = 14,
    /// `a.GetMinDimension()`.
    MinDimension = 15,
    /// `a.IsParallelTo(b, s, &anti)`: `[parallel, anti]`, `anti = -1` when untouched.
    Parallel = 16,
    /// `a.IsPerpendicularTo(b, s)`.
    Perpendicular = 17,
    /// `a.ProjectToPlane(b)`.
    ProjectToPlane = 18,
    /// `a.ProjectPointToPlane(b, c)`.
    ProjectPointToPlane = 19,
    /// `a.RotateProjectToPlane(b, c)`.
    RotateProjectToPlane = 20,
    /// `a.RotateProjectPointToPlane(b, c, d)`.
    RotateProjectPointToPlane = 21,
    /// `a.PerspectiveProjectToPlane(b, c, d, e)`.
    PerspectiveProjectToPlane = 22,
    /// `a.PerspectiveProjectPointToPlane(b, c, d)`.
    PerspectiveProjectPointToPlane = 23,
    /// `a.DistanceBetween(b)`.
    Distance = 24,
    /// `a.DistanceBetweenSquared(b)`.
    DistanceSquared = 25,
    /// `a.CloserThan(s, b)`.
    CloserThan = 26,
    /// `a.Multiply(b)`.
    Multiply = 27,
    /// `a.DerivativeOfCrossProduct(b, c, d)`.
    DerivativeOfCross = 28,
    /// `a.SolveTwoLinearEquations(b, out)` (status or `[x, y]`).
    SolveTwoLinear = 29,
    /// `a[idx]` (const; `idx` in `0..=5`).
    Index = 30,
    /// `a[idx] = s` (non-const; `idx` in `0..=2`).
    IndexSet = 31,
    /// `a == b`.
    Eq = 32,
    /// `a += b`.
    AddAssign = 33,
    /// `a -= b`.
    SubAssign = 34,
    /// `a *= s`.
    MulAssign = 35,
    /// `a /= s`.
    DivAssign = 36,
    /// `a.IsInsideSector(b, c, d)`.
    InsideSector = 37,
    /// `a.MakeUnitOrthoVectors(flag ? &b : NULL, x, y, z)` (status or `[x, y, z]`).
    UnitOrtho = 38,
    /// `a.RotateVecAboutAxis(b, s)`.
    RotateAboutAxis = 39,
    /// `a.UnitizedDerivative(b)`.
    UnitizedDerivative = 40,
    /// `a.UnitizedDerivative2(b, c, ..)`: `[status, d1, d2]`.
    UnitizedDerivative2 = 41,
    /// `a.UnitizedDerivative3(b, c, d, ..)`: `[status, d1, d2, d3]`.
    UnitizedDerivative3 = 42,
    /// `a.AngleBetween(b, ang)`.
    Angle = 43,
    /// `a.CCWAngleBetween(b, c, ang)`.
    CcwAngle = 44,
    /// `a.IsColinearWith(b, c, d)`.
    Colinear = 45,
    /// `a.IsInitialized()`.
    IsInitialized = 46,
    /// `a.IsUndef()`.
    IsUndef = 47,
    /// `a.IsZero(s)`.
    IsZero = 48,
    /// `a.Is2d()`.
    Is2d = 49,
    /// `SmVector3d({a.x, a.y, a.z, s}, true)`.
    FromNl = 50,
    /// `SmVector3d(a, b)` (`b - a`).
    Between = 51,
    /// `SmVector3d(SmVector2d(a.x, a.y))`.
    FromVec2 = 52,
    /// `SmVector3d(double*)`, copy constructor, `operator=`, `Copy`, `Set`, `Swap` chained.
    Copy = 53,
    /// `a.SetUninitialized()`.
    Uninitialized = 54,
    /// `sm_sqrt(s)`.
    Sqrt = 55,
    /// `sm_cbrt(s)`.
    Cbrt = 56,
    /// `sm_acos(s)`.
    Acos = 57,
    /// `sm_asin(s)`.
    Asin = 58,
}

/// Runs `op` in upstream C++ on vectors `v = [a, b, c, d, e]` and returns its result
/// values; a failed `SmStatus` comes back as `FfiError::Status(code)`.
pub fn op(op: Op, v: [[f64; 3]; 5], s: f64, idx: i32, flag: bool) -> Result<Vec<f64>, FfiError> {
    // Const operator[] returns 0 for large indices; the non-const one dereferences NULL.
    let idx_ok = match op {
        Op::Index => (0..=5).contains(&idx),
        Op::IndexSet => (0..=2).contains(&idx),
        _ => true,
    };
    if !idx_ok {
        return Err(FfiError::InvalidArgument(format!("index {idx}")));
    }
    let flat: Vec<f64> = v.iter().flatten().copied().collect();
    let mut out = [0.0f64; MAX_OUT];
    let mut n = 0i32;
    // SAFETY: `flat` is valid for 15 reads, `out` for MAX_OUT writes, `n` is a valid
    // i32; the shim writes at most MAX_OUT values and retains nothing.
    let st = unsafe {
        sys::sc_geom_vector3d_op(
            op as i32,
            flat.as_ptr(),
            s,
            idx,
            i32::from(flag),
            out.as_mut_ptr(),
            &raw mut n,
        )
    };
    check(st)?;
    let n = usize::try_from(n)
        .ok()
        .filter(|&n| n <= MAX_OUT)
        .ok_or_else(|| FfiError::InvalidArgument(format!("shim returned {n} values")))?;
    Ok(out.get(..n).unwrap_or_default().to_vec())
}
