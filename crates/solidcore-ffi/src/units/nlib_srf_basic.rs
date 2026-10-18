//! FFI wrappers for unit `nlib.srf_basic`.
//!
//! Shim: `harness/cpp/shim/nlib_srf_basic.cpp` (`sc_nlib_srf_basic_eval`).
//! Upstream: `source/SMLib/src/NL_SrfBasic.cpp`, `source/SMLib/src/NL_SrfGeom.cpp`.
//!
//! | [`Op`]           | upstream                                                     |
//! |------------------|--------------------------------------------------------------|
//! | `Inspect`        | `N_SrfCountsAreValid`, `N_SrfWeightsAreValid`, `N_SrfIsValid`, `N_SrfIsSized`, `N_IsSrfRat`, `N_SrfIsClosed`, `N_SrfAreWeightsEqual`, `N_SrfGetParameterBounds`, `N_SrfGetArraySizes`, `N_SrfGetDegrees`, `N_SrfGetAverageLen`, `N_SrfIsFlatCheap` |
//! | `Enet`           | `N_SrfGetENet`                                               |
//! | `Modify`         | `N_SrfCompress`, `N_SwapUV`, `N_SrfNonRatToRat`, `N_SrfMakeNonRat`, `N_SrfPruneRat`, `N_SrfReparamToInterval`, `N_SrfSetSizeIndices`, `N_SrfInitArrays` + `N_SrfAreArraysNULL` + `N_SrfCopy`, `N_SrfSizeArrays` (dumped with `N_SrfGetCPtsDegreesAndKnots`) |
//! | `Equal`          | `N_SrfsAreEqual`                                             |
//! | `RatCompatible`  | `N_MakeSrfsRatCompatible`                                    |
//!
//! Input encoding (flat doubles) and output records are described in the shim and in
//! `crates/difftest/src/units/nlib_srf_basic.rs`. [`eval`] parses the whole input and
//! rejects anything that would make upstream index outside the storage it is given
//! (indices above the storage, too-short knot vectors for a rescale, set-size indices
//! past the storage, negative or non-integral counts).

use crate::{call_with_buffer, FfiError};

/// Largest storage index (`N`, `M`, `R`, `S`) accepted by [`eval`].
pub const MAX_INDEX: usize = 4096;
/// Largest degree accepted by [`eval`] (upstream `NL_DEGREE` is a `short`).
pub const MAX_DEGREE_ARG: usize = 1000;

/// A group of upstream calls run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// Checks and measures of one surface; then `eps np mp rk sk`.
    Inspect,
    /// `N_SrfGetENet`; then `ku lu kv lv`.
    Enet,
    /// A modification; then `kind` and its parameters.
    Modify,
    /// `N_SrfsAreEqual` on two surfaces; then `tol3d tol1d`.
    Equal,
    /// `N_MakeSrfsRatCompatible`: `k`, then `k` surfaces.
    RatCompatible,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::Inspect => 0,
            Op::Enet => 1,
            Op::Modify => 2,
            Op::Equal => 3,
            Op::RatCompatible => 4,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_nlib_srf_basic_eval(
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

/// Shape of one encoded surface.
#[derive(Debug, Clone, Copy)]
struct Shape {
    p: usize,
    q: usize,
    r: usize,
    s: usize,
    big_n: usize,
    big_m: usize,
    big_r: usize,
    big_s: usize,
}

impl Reader<'_> {
    fn surface(&mut self) -> Result<Shape, FfiError> {
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
        self.skip(4 * (big_n + 1) * (big_m + 1) + (big_r + 1) + (big_s + 1))?;
        Ok(Shape {
            p,
            q,
            r,
            s,
            big_n,
            big_m,
            big_r,
            big_s,
        })
    }
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    match op {
        Op::Inspect => {
            rd.surface()?;
            rd.num()?; // eps
            for _ in 0..4 {
                rd.index(MAX_INDEX)?;
            }
        }
        Op::Enet => {
            rd.surface()?;
            for _ in 0..4 {
                rd.index(MAX_INDEX)?;
            }
        }
        Op::Modify => {
            let sh = rd.surface()?;
            let kind = rd.index(9)?;
            match kind {
                5 => {
                    for _ in 0..4 {
                        rd.num()?;
                    }
                    let dir = rd.index(3)?;
                    if dir == 0 {
                        return Err(bad("reparam: dir must be 1..=3"));
                    }
                    if (dir & 1 != 0 && sh.r < sh.p) || (dir & 2 != 0 && sh.s < sh.q) {
                        return Err(bad("reparam: knot vector shorter than the degree"));
                    }
                }
                6 => {
                    // New indices must stay inside the storage.
                    rd.index(sh.big_n)?;
                    rd.index(sh.big_m)?;
                    rd.index(MAX_DEGREE_ARG)?;
                    rd.index(MAX_DEGREE_ARG)?;
                    rd.index(sh.big_r)?;
                    rd.index(sh.big_s)?;
                }
                8 => {
                    rd.surface()?;
                }
                9 => {
                    rd.index(MAX_INDEX)?;
                    rd.index(MAX_INDEX)?;
                    rd.index(MAX_DEGREE_ARG)?;
                    rd.index(MAX_DEGREE_ARG)?;
                    rd.index(MAX_INDEX)?;
                    rd.index(MAX_INDEX)?;
                }
                _ => {}
            }
        }
        Op::Equal => {
            rd.surface()?;
            rd.surface()?;
            rd.num()?;
            rd.num()?;
        }
        Op::RatCompatible => {
            let k = rd.index(64)?;
            for _ in 0..k {
                rd.surface()?;
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
        // passes to upstream); buf is valid for buf.len() writes and the shim honours
        // the capacity; len is valid.
        unsafe {
            sys::sc_nlib_srf_basic_eval(
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
