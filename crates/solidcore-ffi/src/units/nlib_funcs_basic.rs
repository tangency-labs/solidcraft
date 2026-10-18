//! FFI wrappers for unit `nlib.funcs_basic`.
//!
//! Shim: `harness/cpp/shim/nlib_funcs_basic.cpp` (`sc_nlib_funcs_basic_eval`).
//! Upstream: `source/SMLib/src/NL_FuncsBasic.cpp`.
//!
//! | [`Op`]          | upstream                                                       |
//! |-----------------|----------------------------------------------------------------|
//! | `CrvEval`       | `N_CFuncEval`, `N_CFuncDerivs`                                 |
//! | `CrvInspect`    | `N_CrvIsFuncSized`, `N_CrvAreFuncArraysNULL`, `N_CFuncInitArrays`, `N_CFuncGetArraySizes`, `N_CFuncGetDegree`, `N_CFuncGetArrayAndKnotVector`, `N_CFuncGetKnotVector`, `N_CrvFuncCntrlValKnotVector`, `N_CFuncGetData` |
//! | `CrvModify`     | `N_CrvFuncCompact`, `N_CFuncSetSizeIndices`, `N_CrvFuncSetPtrs`, `N_CFuncSizeArrays` (also on a NULL function) |
//! | `SrfEval`       | `N_SrfFuncEvalPt`                                              |
//! | `SrfInspect`    | `N_SrfIsFuncSized`, `N_SrfAreFuncArraysNULL`, `N_SFuncInitArrays`, `N_SFuncGetArraySizes`, `N_SFuncGetArrayAndKnotVectors`, `N_SrfFuncCntrlVal`, `N_SFuncGetDegrees`, `N_SFuncGetKnotVectors`, `N_SFuncGetKnots` |
//! | `SrfModify`     | `N_SFuncSetSizeIndices`, `N_SFuncSizeArrays` (also on a NULL function) |
//!
//! Input encoding (flat doubles) and output records are described in the shim and in
//! `crates/difftest/src/units/nlib_funcs_basic.rs`. [`eval`] parses the whole input and
//! rejects anything that would make upstream index outside the storage it is given:
//! indices above the storage, set-size indices or replacement arrays that do not fit,
//! and, for the evaluators, knot vectors shorter than `2p + 2` knots, value storage
//! shorter than `m - p` entries (per direction), degrees above [`MAX_EVAL_DEGREE`],
//! derivative orders above [`MAX_DER`], invalid side flags and non-finite knots or
//! parameters.

use crate::{call_with_buffer, FfiError};

/// Largest storage index accepted by [`eval`].
pub const MAX_INDEX: usize = 4096;
/// Largest degree accepted outside the evaluators (upstream `NL_DEGREE` is a `short`).
pub const MAX_DEGREE_ARG: usize = 1000;
/// Largest degree accepted by the evaluators (keeps `N_BasisEval`'s stack arrays and
/// `N_BasisDerivs`' `int` factors in range).
pub const MAX_EVAL_DEGREE: usize = 12;
/// Largest derivative order accepted by the curve evaluator.
pub const MAX_DER: usize = 64;
/// Largest number of evaluation points per call.
pub const MAX_POINTS: usize = 4096;

/// A group of upstream calls run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// Curve function; then `der k`, `k` x `(u flag)`.
    CrvEval,
    /// Curve function; then `nf mk`.
    CrvInspect,
    /// Curve function; then `kind` and its parameters.
    CrvModify,
    /// Surface function; then `k`, `k` x `(u v ufl vfl)`.
    SrfEval,
    /// Surface function; then `nf mf rk sk`.
    SrfInspect,
    /// Surface function; then `kind n m p q r s`.
    SrfModify,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::CrvEval => 0,
            Op::CrvInspect => 1,
            Op::CrvModify => 2,
            Op::SrfEval => 3,
            Op::SrfInspect => 4,
            Op::SrfModify => 5,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_nlib_funcs_basic_eval(
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

/// Shape of one encoded curve function.
#[derive(Debug, Clone, Copy)]
struct CrvShape {
    big_n: usize,
    big_m: usize,
    n: usize,
    m: usize,
    p: usize,
    /// Knots in use, all finite.
    finite: bool,
}

/// Shape of one encoded surface function.
#[derive(Debug, Clone, Copy)]
struct SrfShape {
    big_n: usize,
    big_m: usize,
    big_r: usize,
    big_s: usize,
    p: usize,
    q: usize,
    r: usize,
    s: usize,
    finite: bool,
}

impl Reader<'_> {
    fn slice(&mut self, k: usize) -> Result<&[f64], FfiError> {
        let end = self.at.checked_add(k).ok_or_else(|| bad("overflow"))?;
        let s = self
            .input
            .get(self.at..end)
            .ok_or_else(|| bad("input too short"))?;
        self.at = end;
        Ok(s)
    }

    fn curve(&mut self) -> Result<CrvShape, FfiError> {
        let big_n = self.index(MAX_INDEX)?;
        let big_m = self.index(MAX_INDEX)?;
        let n = self.index(big_n)?;
        let m = self.index(big_m)?;
        let p = self.index(MAX_DEGREE_ARG)?;
        self.slice(big_n + 1)?;
        let knots = self.slice(big_m + 1)?;
        let finite = knots.iter().take(m + 1).all(|x| x.is_finite());
        Ok(CrvShape {
            big_n,
            big_m,
            n,
            m,
            p,
            finite,
        })
    }

    fn surface(&mut self) -> Result<SrfShape, FfiError> {
        let big_n = self.index(MAX_INDEX)?;
        let big_m = self.index(MAX_INDEX)?;
        let big_r = self.index(MAX_INDEX)?;
        let big_s = self.index(MAX_INDEX)?;
        self.index(big_n)?; // n
        self.index(big_m)?; // m
        let p = self.index(MAX_DEGREE_ARG)?;
        let q = self.index(MAX_DEGREE_ARG)?;
        let r = self.index(big_r)?;
        let s = self.index(big_s)?;
        let cells = (big_n + 1)
            .checked_mul(big_m + 1)
            .ok_or_else(|| bad("overflow"))?;
        self.slice(cells)?;
        let u = self.slice(big_r + 1)?;
        let mut finite = u.iter().take(r + 1).all(|x| x.is_finite());
        let v = self.slice(big_s + 1)?;
        finite &= v.iter().take(s + 1).all(|x| x.is_finite());
        Ok(SrfShape {
            big_n,
            big_m,
            big_r,
            big_s,
            p,
            q,
            r,
            s,
            finite,
        })
    }

    fn flag(&mut self) -> Result<(), FfiError> {
        let f = self.num()?;
        if f != 1.0 && f != 2.0 {
            return Err(bad(format!("side flag {f}")));
        }
        Ok(())
    }

    fn param(&mut self) -> Result<(), FfiError> {
        let u = self.num()?;
        if !u.is_finite() {
            return Err(bad(format!("parameter {u}")));
        }
        Ok(())
    }
}

/// Domain of one evaluator direction: degree `p`, knots in use `U[0..=m]`, value
/// storage `0..=big_n` (upstream reads `fu[spn-p..=spn]`, `spn <= m-p-1`).
fn eval_domain(p: usize, m: usize, big_n: usize, finite: bool) -> Result<(), FfiError> {
    if p > MAX_EVAL_DEGREE {
        return Err(bad(format!("degree {p} > {MAX_EVAL_DEGREE}")));
    }
    if m < 2 * p + 1 {
        return Err(bad(format!("{} knots for degree {p}", m + 1)));
    }
    if m - p - 1 > big_n {
        return Err(bad("value storage shorter than the knot vector needs"));
    }
    if !finite {
        return Err(bad("non-finite knot"));
    }
    Ok(())
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    match op {
        Op::CrvEval => {
            let sh = rd.curve()?;
            eval_domain(sh.p, sh.m, sh.big_n, sh.finite)?;
            rd.index(MAX_DER)?;
            let k = rd.index(MAX_POINTS)?;
            for _ in 0..k {
                rd.param()?;
                rd.flag()?;
            }
        }
        Op::CrvInspect => {
            rd.curve()?;
            rd.index(MAX_INDEX)?;
            rd.index(MAX_INDEX)?;
        }
        Op::CrvModify => {
            let sh = rd.curve()?;
            match rd.index(4)? {
                1 => {
                    rd.index(sh.big_n)?;
                    rd.index(MAX_DEGREE_ARG)?;
                    rd.index(sh.big_m)?;
                }
                2 => {
                    let lf = rd.index(MAX_INDEX + 1)?;
                    if lf < sh.n + 1 {
                        return Err(bad("set_ptrs: value array shorter than n + 1"));
                    }
                    rd.slice(lf)?;
                    let lu = rd.index(MAX_INDEX + 1)?;
                    if lu < sh.m + 1 {
                        return Err(bad("set_ptrs: knot array shorter than m + 1"));
                    }
                    rd.slice(lu)?;
                }
                3 | 4 => {
                    rd.index(MAX_INDEX)?;
                    rd.index(MAX_DEGREE_ARG)?;
                    rd.index(MAX_INDEX)?;
                }
                _ => {}
            }
        }
        Op::SrfEval => {
            let sh = rd.surface()?;
            eval_domain(sh.p, sh.r, sh.big_n, sh.finite)?;
            eval_domain(sh.q, sh.s, sh.big_m, sh.finite)?;
            let k = rd.index(MAX_POINTS)?;
            for _ in 0..k {
                rd.param()?;
                rd.param()?;
                rd.flag()?;
                rd.flag()?;
            }
        }
        Op::SrfInspect => {
            rd.surface()?;
            for _ in 0..4 {
                rd.index(MAX_INDEX)?;
            }
        }
        Op::SrfModify => {
            let sh = rd.surface()?;
            if rd.index(2)? == 0 {
                // New indices must stay inside the storage.
                rd.index(sh.big_n)?;
                rd.index(sh.big_m)?;
                rd.index(MAX_DEGREE_ARG)?;
                rd.index(MAX_DEGREE_ARG)?;
                rd.index(sh.big_r)?;
                rd.index(sh.big_s)?;
            } else {
                rd.index(MAX_INDEX)?;
                rd.index(MAX_INDEX)?;
                rd.index(MAX_DEGREE_ARG)?;
                rd.index(MAX_DEGREE_ARG)?;
                rd.index(MAX_INDEX)?;
                rd.index(MAX_INDEX)?;
            }
        }
    }
    rd.done()
}

/// Runs `op` on `input`; returns the shim's output record.
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(1024, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the
        // shim reads for `op` (validated above, including every index the shim
        // passes to upstream and the evaluator domains); buf is valid for buf.len()
        // writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_nlib_funcs_basic_eval(
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
