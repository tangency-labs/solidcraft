//! FFI wrappers for unit `curve.parabola`.
//!
//! Shim: `harness/cpp/shim/curve_parabola.cpp`.
//! Upstream: `source/SMLib/inc/SmParabola.h`, `source/SMLib/src/SmParabola.cpp`
//! (`SmParabola`).
//!
//! | Rust   | shim                    | upstream                         |
//! |--------|-------------------------|----------------------------------|
//! | [`op`] | `sc_curve_parabola_op`  | one `SmParabola` method per [`Op`] |
//!
//! Input layout: two curve blocks ([`Curve::block`], [`CURVE_BLOCK`] doubles each),
//! then the op's arguments (see [`Op`]). Output: the op status (`0` =
//! `SM_SUCCESS`) followed by the op's values; a curve *state* is `type dim oob
//! borrowed knot_type form O(3) X(3) Y(3) F step_min step_max has_nurb [n p m
//! cpts(x y z w)… knots…]`.

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_curve_parabola_op(
            op: i32,
            input: *const f64,
            n_in: i64,
            data: *const u8,
            data_len: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Doubles per curve block.
pub const CURVE_BLOCK: usize = 14;

/// Highest derivative count [`Op::EvalStep`] accepts.
pub const MAX_DERIVS: i64 = 16;

/// One operation on curve A (and B for [`Op::Eq`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Op {
    /// State of A.
    State = 0,
    /// args `self_flag`: `A == A` or `A == B`.
    Eq = 1,
    /// args `min max`: `AdjustSTEPInterval(SmExtent1d(min, max))`, state.
    Adjust = 2,
    /// args `P(3) classify`: `STEPInversion`: parameter, location (`-1` unasked).
    Invert = 3,
    /// `Copy`: state of the copy, `copy == A`.
    Copy = 4,
    /// args `t n`: `EvaluateSTEP`: `n + 1` vectors on success.
    EvalStep = 5,
    /// args `min max op P(3) tol has_target target requested`:
    /// `GlobalPointSolveSTEP`: count, `(type param value)` per solution, state.
    GpsStep = 6,
    /// args `type`: `IsBounded IsAnalytic IsArc STEPInterval(2) MaxAnalyticDomain(2)
    /// IsKindOf(type) GetType`, type string characters, `-1`, class string characters.
    Queries = 7,
    /// `MakeNurb`, state.
    MakeNurb = 8,
    /// args `min max`: `ReverseParameterization`: new interval, state after
    /// `GetOrCreateGwNurbPointer`.
    Reverse = 9,
    /// args `O(3) X(3) Y(3) has_scale S(3)`: `Transform`, state (or `-2` and the
    /// `SetCanonical` status of a bad frame).
    Transform = 10,
    /// args `file_type dim version via_curve`, `data` = stream: `ReadFromDB`, state
    /// after `GetOrCreateGwNurbPointer`.
    ReadDb = 11,
    /// args `P(3)`: through `SmCurve*`: `STEPInversion` status and parameter,
    /// `IsAnalytic`, `IsBounded`, `GetType`.
    Virtuals = 12,
}

/// A curve block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Curve {
    /// `0`: the constructor; `1`: `CreateCanonical` (domain and dimension ignored).
    pub kind: i32,
    /// Centre / origin.
    pub center: [f64; 3],
    /// X axis.
    pub x_axis: [f64; 3],
    /// Y axis.
    pub y_axis: [f64; 3],
    /// STEP domain (`SmExtent1d(min, max)`).
    pub domain: [f64; 2],
    /// Focal distance.
    pub focal_dist: f64,
    /// Image dimension.
    pub dim: u32,
}

impl Curve {
    /// The block's doubles.
    #[must_use]
    pub fn block(&self) -> Vec<f64> {
        let mut v = Vec::with_capacity(CURVE_BLOCK);
        v.push(f64::from(self.kind));
        v.extend(self.center);
        v.extend(self.x_axis);
        v.extend(self.y_axis);
        v.extend(self.domain);
        v.push(self.focal_dist);
        v.push(f64::from(self.dim));
        v
    }
}

/// Run `op` on curves `a`, `b` with arguments `args` and stream bytes `data`.
pub fn op(op: Op, a: &Curve, b: &Curve, args: &[f64], data: &[u8]) -> Result<Vec<f64>, FfiError> {
    if !matches!(a.kind, 0 | 1) || !matches!(b.kind, 0 | 1) {
        return Err(FfiError::InvalidArgument("curve kind".into()));
    }
    let mut input = a.block();
    input.extend(b.block());
    input.extend_from_slice(args);
    let n_in = i64::try_from(input.len()).map_err(|_| FfiError::InvalidArgument("input".into()))?;
    let data_len =
        i64::try_from(data.len()).map_err(|_| FfiError::InvalidArgument("data too long".into()))?;
    call_with_buffer(256, |buf: &mut [f64], len| {
        // SAFETY: `input` is valid for `n_in` reads and `data` for `data_len`; `buf`
        // for `buf.len()` writes; `len` is a valid i64; nothing is retained.
        unsafe {
            sys::sc_curve_parabola_op(
                op as i32,
                input.as_ptr(),
                n_in,
                data.as_ptr(),
                data_len,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
