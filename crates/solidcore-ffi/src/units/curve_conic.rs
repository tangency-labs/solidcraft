//! FFI wrappers for unit `curve.conic`.
//!
//! Shim: `harness/cpp/shim/curve_conic.cpp`.
//! Upstream: `source/SMLib/inc/SmConic.h`, `source/SMLib/src/SmConic.cpp` (`SmConic`).
//!
//! | Rust   | shim                 | upstream                        |
//! |--------|----------------------|---------------------------------|
//! | [`op`] | `sc_curve_conic_op`  | one `SmConic` method per [`Op`] |

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_curve_conic_op(
            op: i32,
            ca: *const f64,
            ca_len: i64,
            cb: *const f64,
            cb_len: i64,
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

/// Most transforms a [`Built`] conic may apply.
pub const MAX_TRANSFORMS: usize = 16;

/// One `SmConic` operation on conic A (and B for [`Op::Eq`] and
/// [`Op::IntersectConic`]). Status-bearing operations return the `SmStatus` first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Op {
    /// Type code, `A`, `B`, origin `x`, `y`, `theta` (through `WriteToDB`),
    /// `GetType`, `GetDim`, `GetNaturalInterval` (2).
    State = 0,
    /// `Evaluate(t, n, flags & 2, .., flags & 1)`: status, `n + 1` vectors (array of
    /// fresh `SmVector3d`).
    Evaluate = 1,
    /// `EvaluatePoint(t)`: status, point.
    EvaluatePoint = 2,
    /// `ComputeImplicitEquation`: status, six coefficients.
    Implicit = 3,
    /// `IntersectImplicitConic(w)`: status, count, parameters. Not for unknown conics.
    IntersectImplicit = 4,
    /// `IntersectConic(.., B, .., t)`: status, count, `(t, t_other, distance)` per
    /// solution. Not for unknown conics.
    IntersectConic = 5,
    /// `InvertPointOnConic(w0..2)`: status, success flag, parameter if found.
    Invert = 6,
    /// `GetKnots`: status, count, knots, count, multiplicities.
    Knots = 7,
    /// `CalculateContinuities(.., t)`: status, minimum, count, continuities.
    Continuities = 8,
    /// `A == B` (`flags & 1`: `A == A`).
    Eq = 9,
    /// `IsKindOf(n)`.
    IsKindOf = 10,
    /// `GetType`, `GetTypeString` characters, `-1`, `GetClassString` characters.
    TypeStrings = 11,
    /// `SmConic::ReadFromDB` of `data`: status, then the [`Op::State`] values and the
    /// point and tangent at `t = 0` of the read conic.
    ReadDb = 12,
}

/// Conic description: `SmConic(conic_type, a, b)` (or `SmConic()` for
/// `conic_type = -1`), then `Transform2D(angle, (tx, ty))` per transform in order.
#[derive(Debug, Clone, Default)]
pub struct Built {
    /// `SmConicType` value `0..=4`, or `-1` for the empty constructor.
    pub conic_type: i32,
    /// Constant `a`.
    pub a: f64,
    /// Constant `b`.
    pub b: f64,
    /// `(angle, tx, ty)` transforms.
    pub transforms: Vec<[f64; 3]>,
}

impl Built {
    fn flat(&self) -> Result<Vec<f64>, FfiError> {
        if !(-1..=4).contains(&self.conic_type) {
            return Err(FfiError::InvalidArgument("conic type".into()));
        }
        if self.transforms.len() > MAX_TRANSFORMS {
            return Err(FfiError::InvalidArgument("too many transforms".into()));
        }
        let mut v = vec![
            f64::from(self.conic_type),
            self.a,
            self.b,
            self.transforms.len() as f64,
        ];
        v.extend(self.transforms.iter().flatten());
        Ok(v)
    }
}

/// Extra operands of an operation.
#[derive(Debug, Clone, Default)]
pub struct Operands {
    /// Parameter (tolerance for [`Op::IntersectConic`], angle tolerance for
    /// [`Op::Continuities`]).
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

/// Run `op` on conics `a`, `b` with operands `x`.
pub fn op(op: Op, a: &Built, b: &Built, x: &Operands) -> Result<Vec<f64>, FfiError> {
    let unknown = a.conic_type == -1 || a.conic_type == 4;
    match op {
        Op::Evaluate if !(0..=MAX_DERIVS).contains(&x.n) => {
            return Err(FfiError::InvalidArgument("derivative count".into()))
        }
        Op::IntersectImplicit | Op::IntersectConic if unknown => {
            return Err(FfiError::InvalidArgument("unknown conic".into()))
        }
        Op::ReadDb if x.file_type != 0 && x.file_type != 1 => {
            return Err(FfiError::InvalidArgument("file type".into()))
        }
        _ => {}
    }
    let ca = a.flat()?;
    let cb = b.flat()?;
    let data_len = i64::try_from(x.data.len())
        .map_err(|_| FfiError::InvalidArgument("data too long".into()))?;
    call_with_buffer(64, |buf: &mut [f64], len| {
        // SAFETY: `ca`, `cb` are valid for their lengths (checked by the shim
        // against their headers) and `x.w` for 6 reads; `data` for `data_len`
        // reads; `buf` for `buf.len()` writes; `len` is a valid i64; nothing is
        // retained.
        unsafe {
            sys::sc_curve_conic_op(
                op as i32,
                ca.as_ptr(),
                ca.len() as i64,
                cb.as_ptr(),
                cb.len() as i64,
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
