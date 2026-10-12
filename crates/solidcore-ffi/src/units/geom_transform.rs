//! FFI wrappers for unit `geom.transform`.
//!
//! Shim: `harness/cpp/shim/geom_transform.cpp`.
//! Upstream: `source/SMLib/inc/SmTransform.h`, `source/SMLib/src/SmTransform.cpp`
//! (`SmTransform`).
//!
//! | Rust   | shim                    | upstream                            |
//! |--------|-------------------------|-------------------------------------|
//! | [`op`] | `sc_geom_transform_op`  | one `SmTransform` method per [`Op`] |

use crate::{check, FfiError};

mod sys {
    extern "C" {
        pub fn sc_geom_transform_op(
            op: i32,
            kind_a: i32,
            va: *const f64,
            zf_a: i32,
            kind_b: i32,
            vb: *const f64,
            zf_b: i32,
            w: *const f64,
            flag: i32,
            n: i64,
            data: *const u8,
            data_len: i64,
            file_type: i32,
            out: *mut f64,
            out_n: *mut i32,
        ) -> i32;
    }
}

/// Most values one operation returns.
pub const MAX_OUT: usize = 1024;

/// How a transform is built from eight vectors `v0..v7` and the flag `zf`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Kind {
    /// `SmTransform()`.
    Identity = 0,
    /// `SmTransform(NULL, v0, v1, v2, zf ? &v3 : NULL)`.
    Canonical = 1,
    /// `SmTransform()` then `SetToXAxis(v1)`, `SetToYAxis(v2)`, `SetToZAxis(v3)`,
    /// `SetToDisp(v0)` (three-double forms).
    Setters = 2,
    /// `SmTransform().SetAxis2Placement(SmAxis2Placement(v0, v1, v2))`.
    Axis2 = 3,
    /// Copy constructor of [`Kind::Canonical`].
    Copy = 4,
    /// `Copy(ctx, pNew)` of [`Kind::Canonical`].
    CopyVirtual = 5,
    /// [`Kind::Canonical`], then `ConcatTransform(SmTransform(NULL, v4, v5, v6, zf ? &v7 : NULL))`.
    Concat = 6,
    /// [`Kind::Setters`], then `SetCanonical(v4, v5, v6, zf ? &v7 : NULL)`.
    Reset = 7,
}

/// One `SmTransform` operation on transform A (and B for the binary ones).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Op {
    /// Matrix (16), `GetToOrigin/X/Y/Z/Disp` (15), natural domain (6),
    /// `IsBoundedSimple`, `IsCurveInParamDomainSimple`, `HasDiscontinuitiesSimple(NULL)`,
    /// `GetType`.
    State = 0,
    /// `TransformVector(w0)` (3).
    TransformVector = 1,
    /// `InvTransformPoint(w0)`: status, then 3 on success.
    InvPoint = 2,
    /// `InvTransformVector(w0)`: status, then 3 on success.
    InvVector = 3,
    /// B matrix, `ConcatTransform(B)` status, A matrix.
    Concat = 4,
    /// `ScaleSimple(w0, flag ? &w1 : NULL)`: status, matrix.
    Scale = 5,
    /// `TransformSimple(SmAxis2Placement(w0, w1, w2), flag ? &w3 : NULL)`: status, matrix.
    TransformSimple = 6,
    /// `InvertSimple`: status, matrix.
    Invert = 7,
    /// `EvaluateSimple(w0, n)`: status, then `(n+1)^3` vectors on success.
    Evaluate = 8,
    /// `InvEvaluateGuessPointSimple(w0)`: status, then size and the point on success.
    Guess = 9,
    /// `GlobalPointSolveSimple(w0)` on an array pre-filled with `n` solutions:
    /// status, found, size, then 11 values per solution.
    GlobalSolve = 10,
    /// `LocalPointSolveSimple(w0, w1)`: status, then found and 9 solution values.
    LocalSolve = 11,
    /// B matrix, `A == B`.
    Eq = 12,
    /// `A == copy of A`.
    EqSelf = 13,
    /// `IsKindOf(n)`.
    IsKindOf = 14,
    /// `EvaluateBoundingBoxSimple` of the box `w0..w1`; `n` selects the outputs.
    Bbox = 15,
    /// `SmTransform::ReadFromDB` of `data`: status, matrix of the read transform.
    ReadDb = 16,
    /// `HasDiscontinuitiesSimple(&disc, flag)`.
    HasDisconts = 17,
    /// `GetType`, `GetTypeString` characters, `-1`, `GetClassString` characters.
    TypeStrings = 18,
    /// `Init`, matrix.
    Init = 19,
    /// `SetAxis2Placement(SmAxis2Placement(w0, w1, w2))`, matrix.
    SetA2p = 20,
    /// `SetCanonical(w0, w1, w2, flag ? &w3 : NULL)`, matrix.
    SetCanonical = 21,
}

/// Transform description: kind, eight vectors, optional-Z flag.
#[derive(Debug, Clone, Copy)]
pub struct Built {
    /// Construction.
    pub kind: Kind,
    /// Vectors `v0..v7`.
    pub v: [[f64; 3]; 8],
    /// Pass the optional Z axes (`v3`, `v7`).
    pub zf: bool,
}

/// Extra operands of an operation.
#[derive(Debug, Clone, Default)]
pub struct Operands {
    /// Vectors `w0..w4`.
    pub w: [[f64; 3]; 5],
    /// Flag operand.
    pub flag: bool,
    /// Integer operand.
    pub n: i64,
    /// Database bytes ([`Op::ReadDb`]).
    pub data: Vec<u8>,
    /// Database file type, 0 = ASCII, 1 = binary ([`Op::ReadDb`]).
    pub file_type: i32,
}

/// Run `op` on transforms `a`, `b` with operands `x`.
pub fn op(op: Op, a: &Built, b: &Built, x: &Operands) -> Result<Vec<f64>, FfiError> {
    match op {
        Op::Evaluate if !(0..=7).contains(&x.n) => {
            return Err(FfiError::InvalidArgument("derivative order".into()))
        }
        Op::GlobalSolve if !(0..=4).contains(&x.n) => {
            return Err(FfiError::InvalidArgument("pre-filled solutions".into()))
        }
        Op::Bbox if !(0..=2).contains(&x.n) => {
            return Err(FfiError::InvalidArgument("bbox mode".into()))
        }
        Op::ReadDb if x.file_type != 0 && x.file_type != 1 => {
            return Err(FfiError::InvalidArgument("file type".into()))
        }
        _ => {}
    }
    let va: Vec<f64> = a.v.iter().flatten().copied().collect();
    let vb: Vec<f64> = b.v.iter().flatten().copied().collect();
    let wf: Vec<f64> = x.w.iter().flatten().copied().collect();
    let data_len = i64::try_from(x.data.len())
        .map_err(|_| FfiError::InvalidArgument("data too long".into()))?;
    let mut out = vec![0.0f64; MAX_OUT];
    let mut n = 0i32;
    // SAFETY: `va`, `vb` are valid for 24 reads and `wf` for 15; `data` for
    // `data_len` reads; `out` for MAX_OUT writes (the shim never writes more); `n` is
    // a valid i32; nothing is retained.
    let st = unsafe {
        sys::sc_geom_transform_op(
            op as i32,
            a.kind as i32,
            va.as_ptr(),
            i32::from(a.zf),
            b.kind as i32,
            vb.as_ptr(),
            i32::from(b.zf),
            wf.as_ptr(),
            i32::from(x.flag),
            x.n,
            x.data.as_ptr(),
            data_len,
            x.file_type,
            out.as_mut_ptr(),
            &raw mut n,
        )
    };
    check(st)?;
    let n = usize::try_from(n)
        .ok()
        .filter(|&n| n <= MAX_OUT)
        .ok_or_else(|| FfiError::InvalidArgument(format!("shim returned {n} values")))?;
    out.truncate(n);
    Ok(out)
}
