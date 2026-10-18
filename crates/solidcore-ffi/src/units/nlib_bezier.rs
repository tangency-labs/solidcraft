//! FFI wrappers for unit `nlib.bezier`.
//!
//! Shim: `harness/cpp/shim/nlib_bezier.cpp` (`sc_nlib_bezier_eval`).
//! Upstream: `source/SMLib/src/NL_Bezier.cpp`.
//!
//! | [`Op`]            | upstream                              |
//! |-------------------|---------------------------------------|
//! | `ElevMatrix`      | `N_BezGetDegreeElevationMatrix`       |
//! | `ProdMatrix`      | `N_BezFuncMultiplyBezCrvMatrix`       |
//! | `Elevate`         | `N_BezElevateDegree`                  |
//! | `FuncFunc`        | `N_BezFuncMultiplyBezFunc`            |
//! | `FuncCrv`         | `N_BezFuncMultiplyBezCrv`             |
//! | `FuncCrv4d`       | `N_BezFuncMultiplyBezCrv4D`           |
//! | `SrfElevate`      | `N_BezSrfElevateDegree`               |
//! | `OneBasis`        | `N_BezEvalOneBasis`                   |
//! | `ReduceCoefs`     | `N_BezDegreeReduceCoefs`              |
//! | `Reduce`          | `N_BezReduceDegree`                   |
//! | `Split`           | `N_BezSplit`                          |
//! | `ArcLength`       | `N_BezCalcArcLength`                  |
//! | `Length`          | `N_BezCalcLength`                     |
//! | `CrvExtend`       | `N_BezCrvExtend`                      |
//! | `SrfExtend`       | `N_BezSrfExtend4D`                    |
//!
//! Input encodings (flat doubles) are listed per op in the shim. [`eval`] parses the
//! whole input and rejects anything that would make upstream read outside its arrays
//! (degrees past the `NL_PascalTri` table, index ranges past the output buffers,
//! too-short control point arrays, out-of-range flags).

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

/// Largest `p + t` / `p + q` accepted where upstream reads `NL_PascalTri[18][18]`.
pub const MAX_PASCAL: usize = 17;
/// Largest degree accepted elsewhere.
pub const MAX_DEGREE_ARG: usize = 1000;
/// Largest net / surface storage index accepted.
pub const MAX_INDEX: usize = 256;

/// An upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `p t`.
    ElevMatrix,
    /// `p q`.
    ProdMatrix,
    /// `p t f l pre`, `Pw[p+1]`.
    Elevate,
    /// `p q s e pre`, `f[p+1]`, `g[q+1]`.
    FuncFunc,
    /// `p q s e pre`, `f[p+1]`, `Pw[q+1]`.
    FuncCrv,
    /// As `FuncCrv`.
    FuncCrv4d,
    /// `r t dir f l roc pre rows cols`, `net[rows][cols]`.
    SrfElevate,
    /// `p i u`.
    OneBasis,
    /// `p`.
    ReduceCoefs,
    /// `p`, `Pw[p+1]`.
    Reduce,
    /// `p u`, `Pw[p+1]`.
    Split,
    /// `p eps len`, `Pw[p+1]`.
    ArcLength,
    /// `p tol`, `Pw[p+1]`.
    Length,
    /// `p whr rev`, `Pw[p+1]`.
    CrvExtend,
    /// `dir whr rev in_place`, surface.
    SrfExtend,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::ElevMatrix => 0,
            Op::ProdMatrix => 1,
            Op::Elevate => 2,
            Op::FuncFunc => 3,
            Op::FuncCrv => 4,
            Op::FuncCrv4d => 5,
            Op::SrfElevate => 6,
            Op::OneBasis => 7,
            Op::ReduceCoefs => 8,
            Op::Reduce => 9,
            Op::Split => 10,
            Op::ArcLength => 11,
            Op::Length => 12,
            Op::CrvExtend => 13,
            Op::SrfExtend => 14,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_nlib_bezier_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

crate::reader::unit_reader!();

impl Reader<'_> {
    fn index(&mut self, max: usize) -> Result<usize, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < 0.0 || v > max as f64 {
            return Err(bad(format!("bad index/count {v} (max {max})")));
        }
        Ok(v as usize)
    }

    fn flag(&mut self, allowed: &[usize]) -> Result<usize, FfiError> {
        let v = self.index(16)?;
        if !allowed.contains(&v) {
            return Err(bad(format!("bad flag {v}")));
        }
        Ok(v)
    }

    fn cpts(&mut self, k: usize) -> Result<(), FfiError> {
        self.skip(4 * k)
    }
}

fn pascal(sum: usize) -> Result<(), FfiError> {
    if sum > MAX_PASCAL {
        return Err(bad(format!("degree sum {sum} past NL_PascalTri")));
    }
    Ok(())
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    match op {
        Op::ElevMatrix | Op::ProdMatrix => {
            let p = rd.index(MAX_PASCAL)?;
            let t = rd.index(MAX_PASCAL)?;
            pascal(p + t)?;
        }
        Op::Elevate => {
            let p = rd.index(MAX_PASCAL)?;
            let t = rd.index(MAX_PASCAL)?;
            pascal(p + t)?;
            rd.index(MAX_INDEX)?; // f
            rd.index(p + t)?; // l: rows of the matrix
            rd.flag(&[0, 1])?;
            rd.cpts(p + 1)?;
        }
        Op::FuncFunc | Op::FuncCrv | Op::FuncCrv4d => {
            let p = rd.index(MAX_PASCAL)?;
            let q = rd.index(MAX_PASCAL)?;
            pascal(p + q)?;
            rd.index(MAX_INDEX)?; // s
            rd.index(MAX_INDEX)?; // e (> p+q is NL_INP_ERR before any access)
            rd.flag(&[0, 1])?;
            rd.skip(p + 1)?;
            if op == Op::FuncFunc {
                rd.skip(q + 1)?;
            } else {
                rd.cpts(q + 1)?;
            }
        }
        Op::SrfElevate => {
            let r = rd.index(MAX_PASCAL)?;
            let t = rd.index(MAX_PASCAL)?;
            pascal(r + t)?;
            let dir = rd.flag(&[1, 2, 3])?;
            rd.index(MAX_INDEX)?; // f
            rd.index(r + t)?; // l: rows of the matrix
            let roc = rd.index(MAX_INDEX)?;
            rd.flag(&[0, 1])?;
            let rows = rd.index(MAX_INDEX)?;
            let cols = rd.index(MAX_INDEX)?;
            if rows == 0 || cols == 0 {
                return Err(bad("empty net"));
            }
            match dir {
                1 if rows <= r || cols <= roc => return Err(bad("u: net too small")),
                2 if cols <= r || rows <= roc => return Err(bad("v: net too small")),
                _ => {}
            }
            rd.cpts(rows * cols)?;
        }
        Op::OneBasis => {
            let p = rd.index(MAX_DEGREE_ARG)?;
            rd.index(p + 8)?;
            rd.num()?;
        }
        Op::ReduceCoefs => {
            rd.index(MAX_DEGREE_ARG)?;
        }
        Op::Reduce => {
            let p = rd.index(MAX_DEGREE_ARG)?;
            if p < 2 {
                return Err(bad("reduce: degree < 2 reads unset coefficients"));
            }
            rd.cpts(p + 1)?;
        }
        Op::ArcLength => {
            let p = rd.index(MAX_DEGREE_ARG)?;
            rd.num()?;
            rd.num()?;
            rd.cpts(p + 1)?;
        }
        Op::Split | Op::Length => {
            let p = rd.index(MAX_DEGREE_ARG)?;
            rd.num()?;
            rd.cpts(p + 1)?;
        }
        Op::CrvExtend => {
            let p = rd.index(MAX_PASCAL)?;
            rd.flag(&[1, 2])?;
            rd.flag(&[0, 1])?;
            rd.cpts(p + 1)?;
        }
        Op::SrfExtend => {
            rd.flag(&[1, 2])?;
            rd.flag(&[1, 2])?;
            rd.flag(&[0, 1])?;
            rd.flag(&[0, 1])?;
            let big_n = rd.index(MAX_INDEX)?;
            let big_m = rd.index(MAX_INDEX)?;
            let big_r = rd.index(MAX_INDEX)?;
            let big_s = rd.index(MAX_INDEX)?;
            rd.index(big_n)?; // n
            rd.index(big_m)?; // m
            rd.index(MAX_PASCAL)?; // p
            rd.index(MAX_PASCAL)?; // q
            rd.index(big_r)?; // r
            rd.index(big_s)?; // s
            rd.cpts((big_n + 1) * (big_m + 1))?;
            rd.skip(big_r + 1 + big_s + 1)?;
        }
    }
    rd.done()
}

/// Runs `op` on `input`; returns the shim's output record (status first).
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(1024, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the
        // shim reads for `op` (validated above, including every degree and index the
        // shim passes to upstream); buf is valid for buf.len() writes and the shim
        // honours the capacity; len is valid.
        unsafe {
            sys::sc_nlib_bezier_eval(
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
