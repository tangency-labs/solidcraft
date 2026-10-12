//! FFI wrappers for unit `geom.vector2d`.
//!
//! Shim: `harness/cpp/shim/geom_vector2d.cpp`.
//! Upstream: `source/SMLib/inc/SmVector2d.h`, `source/SMLib/src/SmVector2d.cpp` (`SmVector2d`).
//!
//! | Rust   | shim                  | upstream                         |
//! |--------|-----------------------|----------------------------------|
//! | [`op`] | `sc_geom_vector2d_op` | one `SmVector2d` method per [`Op`] |

use crate::{check, FfiError};

mod sys {
    extern "C" {
        pub fn sc_geom_vector2d_op(
            op: i32,
            a: *const f64,
            b: *const f64,
            s: f64,
            idx: i32,
            out: *mut f64,
            out_n: *mut i32,
        ) -> i32;
    }
}

/// One `SmVector2d` operation on operands `a`, `b` (vectors), `s` (scalar), `idx` (index).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Op {
    /// `a.LengthSquared()`.
    LengthSquared = 0,
    /// `a.Length()`.
    Length = 1,
    /// `a.Dot(b)`.
    Dot = 2,
    /// `-a`.
    Neg = 3,
    /// `a + b`.
    Add = 4,
    /// `a - b`.
    Sub = 5,
    /// `a * s`.
    Mul = 6,
    /// `s * a`.
    LMul = 7,
    /// `a / s`.
    Div = 8,
    /// `a.Unitize()`.
    Unitize = 9,
    /// `a.DistanceBetween(b)`.
    Distance = 10,
    /// `a.DistanceBetweenSquared(b)`.
    DistanceSquared = 11,
    /// `a.CloserThan(s, b)` (1 or 0).
    CloserThan = 12,
    /// `a.GetMaxDimension()`.
    MaxDimension = 13,
    /// `a.SwapXY()`.
    SwapXy = 14,
    /// `a.Rotate(s)`.
    Rotate = 15,
    /// `a == b` (1 or 0).
    Eq = 16,
    /// `a != b` (1 or 0).
    Ne = 17,
    /// `a[idx]` (const).
    Index = 18,
    /// `a[idx] = s` (non-const).
    IndexSet = 19,
    /// `a += b`.
    AddAssign = 20,
    /// `a -= b`.
    SubAssign = 21,
    /// `a *= s`.
    MulAssign = 22,
    /// `a.AngleBetween(b, ang)`.
    Angle = 23,
    /// `a.CCWAngleBetween(b, ang)`.
    CcwAngle = 24,
    /// `a.IsZero(s)` (1 or 0).
    IsZero = 25,
    /// `a.IsInitialized()` (1 or 0).
    IsInitialized = 26,
    /// `v2 = SmVector3d(a.x, a.y, s)`.
    FromVec3 = 27,
    /// `a.SetUninitialized()`.
    Uninitialized = 28,
    /// `SmVector2d(double*)`, copy constructor, `operator=`, `Copy`, `Set` chained.
    Copy = 29,
}

/// Runs `op` in upstream C++ and returns its 1 or 2 result values; a failed
/// `SmStatus` comes back as `FfiError::Status(code)`.
pub fn op(op: Op, a: [f64; 2], b: [f64; 2], s: f64, idx: i32) -> Result<Vec<f64>, FfiError> {
    // Non-const operator[] dereferences NULL for other indices.
    if matches!(op, Op::Index | Op::IndexSet) && idx != 0 && idx != 1 {
        return Err(FfiError::InvalidArgument(format!("index {idx}")));
    }
    let mut out = [0.0f64; 2];
    let mut n = 0i32;
    // SAFETY: `a` and `b` are valid for 2 reads, `out` for 2 writes, `n` is a valid
    // i32; the shim writes at most 2 values and retains nothing.
    let st = unsafe {
        sys::sc_geom_vector2d_op(
            op as i32,
            a.as_ptr(),
            b.as_ptr(),
            s,
            idx,
            out.as_mut_ptr(),
            &raw mut n,
        )
    };
    check(st)?;
    let n = usize::try_from(n)
        .ok()
        .filter(|&n| n <= 2)
        .ok_or_else(|| FfiError::InvalidArgument(format!("shim returned {n} values")))?;
    Ok(out.get(..n).unwrap_or_default().to_vec())
}
