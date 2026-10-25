//! FFI wrappers for unit `nlib.crv_basic`.
//!
//! Shim: `harness/cpp/shim/nlib_crv_basic.cpp` (`sc_nlib_crv_basic_eval`).
//! Upstream: `source/SMLib/src/NL_CrvBasic.cpp`, `source/SMLib/src/NL_SrfSymbol.cpp`.
//!
//! | [`Op`]           | upstream                                                     |
//! |------------------|--------------------------------------------------------------|
//! | `Inspect`        | `N_CrvIs3d`, `N_CrvWeightsAreValid`, `N_CrvIsSized`, `N_IsCrvRat`, `N_CrvIsClosed`, `N_CrvIsDegen`, `N_CrvIsLine`, `N_CrvAreWeightsEqual`, `N_Crv4dIsDegen`, `N_CrvIsInZ0Plane`, `N_CrvGetParamBounds`, `N_CrvGetArraySizes`, `N_CrvGetDegree`, `N_CrvIsPlanar` |
//! | `Epolygon`       | `N_CrvGetEPolygon`                                           |
//! | `Modify`         | `N_CrvCompress`, `N_Crv3dTo2d`, `N_Crv2dTo3d`, `N_CrvRatToNonRat`, `N_CrvNonRatToRat`, `N_CrvMakeNonRat`, `N_CrvPruneRat`, `N_CrvReparamToInterval`, `N_CrvSetSizeIndices`, `N_CrvInitArrays` + `N_CrvAreArraysNULL` + `N_CrvCopy`, `N_CrvSizeArrays` (dumped with `N_CrvGetCPtsDegreeAndKnots`) |
//! | `Equal`          | `N_CrvsAreEqual`                                             |
//! | `Compatible`     | `N_CrvsAreCombatible`                                        |
//! | `RatCompatible`  | `N_CrvsMakeRatCompatible`                                    |
//! | `Coincident`     | `N_CrvsAreCoincident`                                        |
//! | `Conic`          | `N_CrvGetConicData`                                          |
//! | `CrossBoundary`  | `N_CrossBoundDerivCrvNonRatSrf`                              |
//! | `NumDenom`       | `N_CrvGetNumAndDenom`, `N_CrvGetDenomCrvFunc`, `N_CreateCrvFromNumAndDenom` |
//!
//! Input encoding (flat doubles) and output records are described in the shim and in
//! `crates/difftest/src/units/nlib_crv_basic.rs`. [`eval`] parses the whole input and
//! rejects anything that would make upstream index outside the storage it is given
//! (indices above the storage, too-short knot vectors for a rescale or a
//! cross-boundary derivative, set-size indices past the storage, negative or
//! non-integral counts), loop forever (huge or non-finite conic angles), or hand
//! `N_CrvsMakeCompatible` a curve that is not a clamped, well-formed B-spline.

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

/// Largest storage index (`N`, `K`, surface `N`, `M`, `R`, `S`) accepted by [`eval`].
pub const MAX_INDEX: usize = 4096;
/// Largest degree accepted by [`eval`] (upstream `NL_DEGREE` is a `short`).
pub const MAX_DEGREE_ARG: usize = 1000;
/// Largest absolute conic angle accepted by [`eval`] (degrees).
pub const MAX_CONIC_ANGLE: f64 = 1.0e6;

/// A group of upstream calls run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// Checks and measures of one curve; then `tol np mk`.
    Inspect,
    /// `N_CrvGetEPolygon`; then `k l`.
    Epolygon,
    /// A modification; then `kind` and its parameters.
    Modify,
    /// `N_CrvsAreEqual` on two curves; then `tol3d tol1d`.
    Equal,
    /// `N_CrvsAreCombatible`: `k >= 1`, then `k` curves.
    Compatible,
    /// `N_CrvsMakeRatCompatible`: `k`, then `k` curves.
    RatCompatible,
    /// `N_CrvsAreCoincident` on two curves; then `tol`.
    Coincident,
    /// `N_CrvGetConicData`: `ctp as ae`.
    Conic,
    /// `N_CrossBoundDerivCrvNonRatSrf`: surface, `side`, `has_dst`, optional curve.
    CrossBoundary,
    /// Numerator / denominator routines: curve, `mode`, `has_c`, optional curve,
    /// `has_f`, optional curve function.
    NumDenom,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::Inspect => 0,
            Op::Epolygon => 1,
            Op::Modify => 2,
            Op::Equal => 3,
            Op::Compatible => 4,
            Op::RatCompatible => 5,
            Op::Coincident => 6,
            Op::Conic => 7,
            Op::CrossBoundary => 8,
            Op::NumDenom => 9,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_nlib_crv_basic_eval(
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

/// Shape of one encoded curve (or curve function).
#[derive(Debug, Clone, Copy)]
struct CrvShape {
    big_n: usize,
    big_k: usize,
    n: usize,
    p: usize,
    m: usize,
    /// Offset of the first control point in the input.
    data: usize,
}

/// Shape of one encoded surface.
#[derive(Debug, Clone, Copy)]
struct SrfShape {
    n: usize,
    m: usize,
    p: usize,
    q: usize,
    r: usize,
    s: usize,
}

impl Reader<'_> {
    /// A curve (`width = 4` doubles per control point) or a curve function
    /// (`width = 1`).
    fn curve_like(&mut self, width: usize) -> Result<CrvShape, FfiError> {
        let big_n = self.index(MAX_INDEX)?;
        let big_k = self.index(MAX_INDEX)?;
        let n = self.index(big_n)?;
        let p = self.index(MAX_DEGREE_ARG)?;
        let m = self.index(big_k)?;
        let data = self.at;
        self.skip(width * (big_n + 1) + (big_k + 1))?;
        Ok(CrvShape {
            big_n,
            big_k,
            n,
            p,
            m,
            data,
        })
    }

    fn curve(&mut self) -> Result<CrvShape, FfiError> {
        self.curve_like(4)
    }

    fn surface(&mut self) -> Result<SrfShape, FfiError> {
        let big_n = self.index(MAX_INDEX)?;
        let big_m = self.index(MAX_INDEX)?;
        let big_r = self.index(MAX_INDEX)?;
        let big_s = self.index(MAX_INDEX)?;
        let n = self.index(big_n)?;
        let m = self.index(big_m)?;
        let p = self.index(MAX_DEGREE_ARG)?;
        let q = self.index(MAX_DEGREE_ARG)?;
        let r = self.index(big_r)?;
        let s = self.index(big_s)?;
        self.skip(4 * (big_n + 1) * (big_m + 1) + (big_r + 1) + (big_s + 1))?;
        Ok(SrfShape { n, m, p, q, r, s })
    }
}

/// A curve `N_CrvsMakeCompatible` can work on: storage exactly the indices in use,
/// `m = n + p + 1`, `1 <= p <= n`, clamped non-decreasing finite knots with interior
/// multiplicities at most `p`, finite control points, consistent dimension and
/// rationality, positive weights.
fn check_well_formed(input: &[f64], sh: &CrvShape) -> Result<(), FfiError> {
    let CrvShape {
        big_n,
        big_k,
        n,
        p,
        m,
        data,
    } = *sh;
    if big_n != n || big_k != m || m != n + p + 1 || p == 0 || p > n || p > 20 {
        return Err(bad("coincident: curve must be a plain clamped B-spline"));
    }
    let short = || bad("input too short");
    let cps = input.get(data..data + 4 * (n + 1)).ok_or_else(short)?;
    let knots = input
        .get(data + 4 * (n + 1)..data + 4 * (n + 1) + m + 1)
        .ok_or_else(short)?;
    let (pts, _) = cps.as_chunks::<4>();
    let [_, _, z0, w0] = *pts.first().ok_or_else(short)?;
    for &[x, y, z, w] in pts {
        if (z == f64::MAX) != (z0 == f64::MAX) || (w == -f64::MAX) != (w0 == -f64::MAX) {
            return Err(bad("coincident: mixed dimension or rationality"));
        }
        if !x.is_finite() || !y.is_finite() || !z.is_finite() || !w.is_finite() {
            return Err(bad("coincident: non-finite control point"));
        }
        // Upstream accepts both weight bounds; NaN remains rejected by the finite check above.
        if w != -f64::MAX && (w.is_nan() || w < 1e-3 || w > 100.0) {
            return Err(bad("coincident: bad weight"));
        }
    }
    if knots.iter().any(|k| !k.is_finite()) || knots.windows(2).any(|w| w.first() > w.last()) {
        return Err(bad("coincident: knots not non-decreasing"));
    }
    let (Some(&a), Some(&b)) = (knots.first(), knots.last()) else {
        return Err(short());
    };
    let head = knots.get(..=p).ok_or_else(short)?;
    let tail = knots.get(m - p..).ok_or_else(short)?;
    if a >= b || head.iter().any(|&k| k != a) || tail.iter().any(|&k| k != b) {
        return Err(bad("coincident: knots not clamped"));
    }
    let inner = knots.get(p + 1..m - p).ok_or_else(short)?;
    let mut run = 1;
    for w in inner.windows(2) {
        run = if w.first() == w.last() { run + 1 } else { 1 };
        if run > p {
            return Err(bad("coincident: interior multiplicity above the degree"));
        }
    }
    if inner.iter().any(|&k| k == a || k == b) {
        return Err(bad("coincident: interior knot at an end"));
    }
    Ok(())
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    match op {
        Op::Inspect => {
            rd.curve()?;
            rd.num()?; // tol
            rd.index(MAX_INDEX)?;
            rd.index(MAX_INDEX)?;
        }
        Op::Epolygon => {
            rd.curve()?;
            rd.index(MAX_INDEX)?;
            rd.index(MAX_INDEX)?;
        }
        Op::Modify => {
            let sh = rd.curve()?;
            let kind = rd.index(11)?;
            match kind {
                6 => {
                    rd.index(1)?;
                }
                7 => {
                    rd.num()?;
                    rd.num()?;
                    if sh.m < sh.p {
                        return Err(bad("reparam: knot vector shorter than the degree"));
                    }
                }
                8 => {
                    rd.index(sh.big_n)?;
                    rd.index(MAX_DEGREE_ARG)?;
                    rd.index(sh.big_k)?;
                }
                10 => {
                    rd.curve()?;
                }
                11 => {
                    rd.index(MAX_INDEX)?;
                    rd.index(MAX_DEGREE_ARG)?;
                    rd.index(MAX_INDEX)?;
                }
                _ => {}
            }
        }
        Op::Equal => {
            rd.curve()?;
            rd.curve()?;
            rd.num()?;
            rd.num()?;
        }
        Op::Compatible | Op::RatCompatible => {
            let k = rd.index(64)?;
            if op == Op::Compatible && k == 0 {
                return Err(bad("compatible: needs at least one curve"));
            }
            for _ in 0..k {
                rd.curve()?;
            }
        }
        Op::Coincident => {
            let a = rd.curve()?;
            check_well_formed(input, &a)?;
            let b = rd.curve()?;
            check_well_formed(input, &b)?;
            rd.num()?;
        }
        Op::Conic => {
            let ctp = rd.index(5)?;
            if !matches!(ctp, 2 | 4 | 5) {
                return Err(bad("conic: type must be 2, 4 or 5"));
            }
            for _ in 0..2 {
                let a = rd.num()?;
                if !a.is_finite() || a.abs() > MAX_CONIC_ANGLE {
                    return Err(bad("conic: angle out of range"));
                }
            }
        }
        Op::CrossBoundary => {
            let s = rd.surface()?;
            let side = rd.index(4)?;
            let ok = match side {
                1 => s.n >= 1 && s.p < s.r,
                2 => s.n >= 1 && s.n + s.p <= s.r,
                3 => s.m >= 1 && s.q < s.s,
                4 => s.m >= 1 && s.m + s.q <= s.s,
                _ => false,
            };
            if !ok {
                return Err(bad("cross boundary: bad side or surface too small"));
            }
            if rd.index(1)? == 1 {
                rd.curve()?;
            }
        }
        Op::NumDenom => {
            rd.curve()?;
            rd.index(2)?; // mode
            if rd.index(1)? == 1 {
                rd.curve()?;
            }
            if rd.index(1)? == 1 {
                rd.curve_like(1)?;
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
            sys::sc_nlib_crv_basic_eval(
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
