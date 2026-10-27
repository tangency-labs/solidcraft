//! FFI wrappers for unit `curve.hermite_curve`.
//!
//! Shim: `harness/cpp/shim/curve_hermite_curve.cpp`.
//! Upstream: `source/SMLib/inc/SmHermiteCurve.h`, `source/SMLib/src/SmHermiteCurve.cpp`
//! (`SmHermiteCurve`).
//!
//! | Rust   | shim                         | upstream                              |
//! |--------|------------------------------|---------------------------------------|
//! | [`op`] | `sc_curve_hermite_curve_op`  | one `SmHermiteCurve` method per [`Op`] |

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_curve_hermite_curve_op(
            op: i32,
            kind_a: i32,
            va: *const f64,
            dim_a: i64,
            kind_b: i32,
            vb: *const f64,
            dim_b: i64,
            t: f64,
            n: i64,
            flags: i32,
            w: *const f64,
            data: *const u8,
            data_len: i64,
            file_type: i32,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Highest derivative count [`Op::Evaluate`] accepts.
pub const MAX_DERIVS: i64 = 16;

/// How a curve is built from `v0..v3` (start point, start derivative, end point,
/// end derivative) and a dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Kind {
    /// The 3D constructor with the given dimension.
    ThreeD = 0,
    /// The 2D constructor (`SmPoint2d`/`SmVector2d` from the `x`, `y` of each vector)
    /// with the given dimension.
    TwoD = 1,
    /// The 3D constructor with its default dimension.
    ThreeDDefault = 2,
    /// The 2D constructor with its default dimension.
    TwoDDefault = 3,
}

/// One `SmHermiteCurve` operation on curve A (and B for [`Op::Eq`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Op {
    /// `GetType`, `GetDim`, `GetBezierPoints` (12), `GetNaturalInterval` (2), the 18
    /// member doubles (through `WriteToDB`).
    State = 0,
    /// `Evaluate(t, n, flags & 2, .., flags & 1)`: status, `n + 1` vectors (array
    /// pre-filled with zeros).
    Evaluate = 1,
    /// `EvaluatePoint(t)`: status, point.
    EvaluatePoint = 2,
    /// `CalculateBoundingBox`: status, then the requested boxes (see the shim).
    Bbox = 3,
    /// `ComputeChordHeight`.
    Chord = 4,
    /// `GetKnots`: status, count, knots, count, multiplicities.
    Knots = 5,
    /// `CalculateContinuities(.., t)`: status, minimum, count, continuities.
    Continuities = 6,
    /// `A == B` (`flags & 1`: `A == A`).
    Eq = 7,
    /// `IsKindOf(n)`.
    IsKindOf = 8,
    /// `GetType`, `GetTypeString` characters, `-1`, `GetClassString` characters.
    TypeStrings = 9,
    /// `SmHermiteCurve::ReadFromDB` of `data`: status, then type, dimension and the 18
    /// member doubles of the read curve.
    ReadDb = 10,
    /// `WriteType` + `WriteToDB` of A (`file_type`, version `n`), read back through
    /// `ReadType` + `SmCurve::ReadFromDB`: write status, byte count, bytes, read status,
    /// then type, dimension, the 18 member doubles, `== A`, `EvaluatePoint(t)` status
    /// and point, and the status, count and bytes of a second write of the read curve.
    /// Accepted for finite vectors and versions up to 1000 (a non-finite ASCII value is
    /// written as text the reader cannot extract).
    WriteRead = 11,
}

/// Curve description.
#[derive(Debug, Clone, Copy)]
pub struct Built {
    /// Construction.
    pub kind: Kind,
    /// Start point, start derivative, end point, end derivative.
    pub v: [[f64; 3]; 4],
    /// Image dimension (ignored by the default-dimension kinds).
    pub dim: u32,
}

/// Extra operands of an operation.
#[derive(Debug, Clone, Default)]
pub struct Operands {
    /// Parameter (or angle tolerance for [`Op::Continuities`]).
    pub t: f64,
    /// Derivative count or type code.
    pub n: i64,
    /// Per-operation flag bits.
    pub flags: i32,
    /// Extra values `w0..w5`.
    pub w: [f64; 6],
    /// Database bytes ([`Op::ReadDb`]).
    pub data: Vec<u8>,
    /// Database file type, 0 = ASCII, 1 = binary ([`Op::ReadDb`]).
    pub file_type: i32,
}

/// Run `op` on curves `a`, `b` with operands `x`.
pub fn op(op: Op, a: &Built, b: &Built, x: &Operands) -> Result<Vec<f64>, FfiError> {
    match op {
        Op::Evaluate if !(0..=MAX_DERIVS).contains(&x.n) => {
            return Err(FfiError::InvalidArgument("derivative count".into()))
        }
        Op::Bbox if !matches!(x.flags & 7, 1..=7) => {
            return Err(FfiError::InvalidArgument(
                "no bounding box requested".into(),
            ))
        }
        Op::ReadDb | Op::WriteRead if x.file_type != 0 && x.file_type != 1 => {
            return Err(FfiError::InvalidArgument("file type".into()))
        }
        Op::WriteRead
            if !(0..=1000).contains(&x.n)
                || !x.t.is_finite()
                || a.v.iter().flatten().any(|c| !c.is_finite()) =>
        {
            return Err(FfiError::InvalidArgument(
                "write_read: version or non-finite value".into(),
            ))
        }
        _ => {}
    }
    let va: Vec<f64> = a.v.iter().flatten().copied().collect();
    let vb: Vec<f64> = b.v.iter().flatten().copied().collect();
    let data_len = i64::try_from(x.data.len())
        .map_err(|_| FfiError::InvalidArgument("data too long".into()))?;
    call_with_buffer(64, |buf: &mut [f64], len| {
        // SAFETY: `va`, `vb` are valid for 12 reads and `x.w` for 6; `data` for
        // `data_len` reads; `buf` for `buf.len()` writes; `len` is a valid i64;
        // nothing is retained.
        unsafe {
            sys::sc_curve_hermite_curve_op(
                op as i32,
                a.kind as i32,
                va.as_ptr(),
                i64::from(a.dim),
                b.kind as i32,
                vb.as_ptr(),
                i64::from(b.dim),
                x.t,
                x.n,
                x.flags,
                x.w.as_ptr(),
                x.data.as_ptr(),
                data_len,
                x.file_type,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
