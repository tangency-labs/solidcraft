//! FFI wrappers for unit `nlib.crv_tool_p2`.
//!
//! Shim: `harness/cpp/shim/nlib_crv_tool_p2.cpp` (`sc_nlib_crv_tool_p2_eval`).
//! Upstream: `source/SMLib/src/NL_CrvTool.cpp`.
//!
//! | [`Op`]               | upstream                        | input                              |
//! |----------------------|---------------------------------|------------------------------------|
//! | `RemoveKnots`        | `N_CrvRemoveKnots`              | curve, `tol inplace`               |
//! | `ReduceDegreeOnce`   | `N_CrvReduceDegreeOnce`         | curve, `tol inplace`               |
//! | `ReduceDegree`       | `N_CrvReduceDegree`             | curve, `tol inplace`               |
//! | `MakeCompatible`     | `N_CrvsMakeCompatible`          | `count`, curves, `0` (unused)      |
//! | `MakeCompatibleTol`  | `N_CrvsMakeCompatibleKnotTol`   | `count`, curves, `knot_tol`        |
//! | `MakeCompatibleAdj`  | `N_CrvsMakeCompatibleAdjKnots`  | `count`, curves, `tol`             |
//! | `RemoveDegenSegs`    | `N_CrvRemoveDegenSegs`          | curve, `tol inplace`               |
//!
//! A curve is encoded `n p m`, `n+1` control points `x y z w`, `m+1` knots. [`eval`]
//! accepts only well-formed clamped curves (`m = n+p+1`, `1 <= p <= n`, finite data,
//! consistent dimension and rationality, non-decreasing knots with end multiplicity
//! exactly `p+1` and interior multiplicity at most `p`) and finite tolerances (positive
//! where upstream would otherwise loop forever); everything else is rejected before the
//! call.

use crate::{call_with_buffer, FfiError};

/// Largest control point index accepted by [`eval`].
pub const MAX_INDEX: usize = 4096;
/// Largest degree accepted by [`eval`] (upstream `NL_MAXDEG`).
pub const MAX_DEGREE: usize = 17;
/// Largest number of curves of a curve-set op.
pub const MAX_CURVES: usize = 8;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `N_CrvRemoveKnots`.
    RemoveKnots,
    /// `N_CrvReduceDegreeOnce`.
    ReduceDegreeOnce,
    /// `N_CrvReduceDegree`.
    ReduceDegree,
    /// `N_CrvsMakeCompatible`.
    MakeCompatible,
    /// `N_CrvsMakeCompatibleKnotTol`.
    MakeCompatibleTol,
    /// `N_CrvsMakeCompatibleAdjKnots`.
    MakeCompatibleAdj,
    /// `N_CrvRemoveDegenSegs`.
    RemoveDegenSegs,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::RemoveKnots => 0,
            Op::ReduceDegreeOnce => 1,
            Op::ReduceDegree => 2,
            Op::MakeCompatible => 3,
            Op::MakeCompatibleTol => 4,
            Op::MakeCompatibleAdj => 5,
            Op::RemoveDegenSegs => 6,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_nlib_crv_tool_p2_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

fn bad(msg: impl Into<String>) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

crate::reader::unit_reader!();

impl Reader<'_> {
    fn finite(&mut self) -> Result<f64, FfiError> {
        let v = self.num()?;
        if !v.is_finite() {
            return Err(bad("non-finite parameter"));
        }
        Ok(v)
    }

    fn positive(&mut self) -> Result<f64, FfiError> {
        let v = self.finite()?;
        if v <= 0.0 {
            return Err(bad("tolerance must be positive"));
        }
        Ok(v)
    }

    fn flag(&mut self) -> Result<(), FfiError> {
        self.index(1).map(|_| ())
    }
}

/// Validates one curve.
fn curve(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let n = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || m != n + p + 1 {
        return Err(bad("curve: need 1 <= p <= n and m = n + p + 1"));
    }
    let cps = rd.take(4 * (n + 1))?;
    let knots = rd.take(m + 1)?;
    let (pts, _) = cps.as_chunks::<4>();
    let [_, _, z0, w0] = *pts.first().ok_or_else(|| bad("empty curve"))?;
    for &[x, y, z, w] in pts {
        if (z == f64::MAX) != (z0 == f64::MAX) || (w == -f64::MAX) != (w0 == -f64::MAX) {
            return Err(bad("curve: mixed dimension or rationality"));
        }
        if !x.is_finite() || !y.is_finite() || !z.is_finite() || !w.is_finite() {
            return Err(bad("curve: non-finite control point"));
        }
    }
    if knots.iter().any(|k| !k.is_finite()) || knots.windows(2).any(|w| w.first() > w.last()) {
        return Err(bad("curve: knots not non-decreasing"));
    }
    let (Some(&a), Some(&b)) = (knots.first(), knots.last()) else {
        return Err(bad("empty knots"));
    };
    let short = || bad("curve: knots too short");
    let head = knots.get(..=p).ok_or_else(short)?;
    let tail = knots.get(m - p..).ok_or_else(short)?;
    let inner = knots.get(p + 1..m - p).ok_or_else(short)?;
    if a >= b || head.iter().any(|&k| k != a) || tail.iter().any(|&k| k != b) {
        return Err(bad("curve: knots not clamped"));
    }
    if inner.iter().any(|&k| k == a || k == b) {
        return Err(bad("curve: end multiplicity above p + 1"));
    }
    let mut run = 1;
    for w in inner.windows(2) {
        run = if w.first() == w.last() { run + 1 } else { 1 };
        if run > p {
            return Err(bad("curve: interior multiplicity above the degree"));
        }
    }
    Ok(())
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    match op {
        Op::RemoveKnots | Op::ReduceDegreeOnce | Op::ReduceDegree => {
            curve(&mut rd)?;
            rd.finite()?;
            rd.flag()?;
        }
        Op::RemoveDegenSegs => {
            curve(&mut rd)?;
            rd.positive()?;
            rd.flag()?;
        }
        Op::MakeCompatible | Op::MakeCompatibleTol | Op::MakeCompatibleAdj => {
            let cnt = rd.index(MAX_CURVES)?;
            if cnt == 0 {
                return Err(bad("curve set: needs at least one curve"));
            }
            for _ in 0..cnt {
                curve(&mut rd)?;
            }
            if op == Op::MakeCompatibleAdj {
                rd.positive()?;
            } else {
                rd.finite()?;
            }
        }
    }
    rd.done()
}

/// Runs `op` on `input` (curve(s), then the op's parameters); returns the shim's output
/// record (status, then outputs).
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(1024, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the
        // shim reads for `op` (validated above); buf is valid for buf.len() writes and
        // the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_nlib_crv_tool_p2_eval(
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
