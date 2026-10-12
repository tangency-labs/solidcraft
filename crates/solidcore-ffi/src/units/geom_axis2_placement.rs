//! FFI wrappers for unit `geom.axis2_placement`.
//!
//! Shim: `harness/cpp/shim/geom_axis2_placement.cpp`.
//! Upstream: `source/SMLib/inc/SmAxis2Placement.h`, `source/SMLib/src/SmAxis2Placement.cpp`
//! (`SmAxis2Placement`).
//!
//! | Rust   | shim                          | upstream                                 |
//! |--------|-------------------------------|------------------------------------------|
//! | [`op`] | `sc_geom_axis2_placement_op`  | one `SmAxis2Placement` method per [`Op`] |

use crate::{check, FfiError};

mod sys {
    extern "C" {
        pub fn sc_geom_axis2_placement_op(
            op: i32,
            kind_a: i32,
            va: *const f64,
            da: f64,
            kind_b: i32,
            vb: *const f64,
            db: f64,
            w: *const f64,
            s: f64,
            t: f64,
            out: *mut f64,
            out_n: *mut i32,
        ) -> i32;
    }
}

/// Most values one operation returns.
pub const MAX_OUT: usize = 40;

/// How a placement is built from six vectors `v0..v5` and a scalar `d`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Kind {
    /// `SmAxis2Placement()`.
    Identity = 0,
    /// `SmAxis2Placement(v0, v1, v2)` (orient constructor).
    Orient = 1,
    /// `SmAxis2Placement().SetCanonical(v0, v1, v2)` (status reported).
    Canonical = 2,
    /// `SmAxis2Placement().SetSTEPCanonical(v0, v1, v2)` (status reported).
    Step = 3,
    /// `SmAxis2Placement(v0, d)` (plane constructor).
    Plane = 4,
    /// `SmAxis2Placement(v0, v1, v2, v3, v4, v5)` (from/to constructor).
    FromTo = 5,
    /// Copy constructor of `SmAxis2Placement(v0, v1, v2)`.
    Copy = 6,
    /// `SmAxis2Placement(v0, v1, v2)` then `RotateAboutAxisAtPoint(d, v3, v4)`.
    Rotated = 7,
}

/// One `SmAxis2Placement` operation on placement A (and B for the binary ones).
///
/// Every result starts with `[status_a, A origin, A x, A y, A z]` (13 values); binary
/// operations then give B the same way, followed by the operation's values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Op {
    /// `GetOrigin`, `GetXAxis`, `GetYAxis`, then `GetCanonical` (18 values).
    State = 0,
    /// `GetMatrix` (16, row-major `padMat[i][j]`).
    Matrix = 1,
    /// `GetMirrorMatrix` (16).
    Mirror = 2,
    /// `Translate(w0)`, then the state (12).
    Translate = 3,
    /// `RotateAboutAxis(s, w0)`, then the state.
    Rotate = 4,
    /// `RotateAboutAxisAtPoint(s, w0, w1)`, then the state.
    RotateAt = 5,
    /// `TransformPoint(w0)` (3).
    TransformPoint = 6,
    /// `TransformVector(w0)` (3).
    TransformVector = 7,
    /// `A.TransformAxis2Placement(B)` (B, then the result's state).
    TransformA2p = 8,
    /// `Invert` (the result's state).
    Invert = 9,
    /// `A == B` (B, then 0/1).
    Eq = 10,
    /// `A == A` (0/1).
    EqSelf = 11,
    /// `IsIdentity(s)` (0/1).
    IsIdentity = 12,
    /// `AreCoaxial(B, SmApproxTol3d(s), t)` (B, then 0/1).
    Coaxial = 13,
    /// `RotateAboutAxis(s, w0)`, `Translate(w1)`, `RotateAboutAxis(t, w2)`, then the state.
    Chain = 14,
    /// `Init`, then the state.
    Init = 15,
    /// `A.TransformAxis2Placement(A, A)` (aliased output), then the state.
    TransformSelf = 16,
}

/// Placement description: kind, six vectors, scalar.
#[derive(Debug, Clone, Copy)]
pub struct Placement {
    /// Construction.
    pub kind: Kind,
    /// Vectors `v0..v5`.
    pub v: [[f64; 3]; 6],
    /// Scalar `d`.
    pub d: f64,
}

/// Run `op` on placements `a`, `b`, vectors `w` and scalars `s`, `t`.
pub fn op(
    op: Op,
    a: &Placement,
    b: &Placement,
    w: [[f64; 3]; 3],
    s: f64,
    t: f64,
) -> Result<Vec<f64>, FfiError> {
    let va: Vec<f64> = a.v.iter().flatten().copied().collect();
    let vb: Vec<f64> = b.v.iter().flatten().copied().collect();
    let wf: Vec<f64> = w.iter().flatten().copied().collect();
    let mut out = [0.0f64; MAX_OUT];
    let mut n = 0i32;
    // SAFETY: `va`, `vb` are valid for 18 reads and `wf` for 9; `out` for MAX_OUT
    // writes (the shim writes at most 38); `n` is a valid i32; nothing is retained.
    let st = unsafe {
        sys::sc_geom_axis2_placement_op(
            op as i32,
            a.kind as i32,
            va.as_ptr(),
            a.d,
            b.kind as i32,
            vb.as_ptr(),
            b.d,
            wf.as_ptr(),
            s,
            t,
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
