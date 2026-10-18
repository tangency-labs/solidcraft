//! FFI wrappers for unit `nlib.crv_adv_p2`.
//!
//! Shim: `harness/cpp/shim/nlib_crv_adv_p2.cpp` (`sc_nlib_crv_adv_p2_eval`).
//! Upstream: `source/SMLib/src/NL_CrvAdv.cpp`.
//!
//! | [`Op`]              | upstream                            | input after the curve            |
//! |---------------------|-------------------------------------|----------------------------------|
//! | `ExtendDist`        | `N_CrvExtendByDist`                 | `dist flg end cont inplace`      |
//! | `ExtendParam`       | `N_CrvExtendByParamDist`            | `par end cont inplace`           |
//! | `OffsetMax2nd`      | `N_CrvOffsetGetMax2ndDeriv`         | `d N[3]`                         |
//! | `EvalUnbounded`     | `N_CrvEvalUnboundedPtsAndDerivs`    | `cont k`, then `k` x `u ndr`     |
//! | `RemoveKnotsDeriv`  | `N_CrvRemoveKnotsDerivConstraints`  | `tol whr der inplace`            |
//! | `RefineToKnots`     | `N_CrvRefineToKnotVector`           | `r+1`, `X[0..=r]`                |
//! | `SumDiff`           | `N_CrvSumDiffCrv`                   | second curve, `opr`              |
//! | `NonRatDeriv`       | `N_CrvNonRatGetFirstDeriv`          | `d`                              |
//! | `RatDeriv`          | `N_CrvRatGetFirstDeriv`             | (none)                           |
//! | `ConstMul`          | `N_ConstantMultiplyCrv`             | `alpha`                          |
//! | `LinearMul`         | `N_LinearMultiplyCrv`               | `s0 s1`                          |
//! | `ConstMul4d`        | `N_ConstantMultiplyCrv4d`           | `alpha`                          |
//! | `ReparamRat`        | `N_CrvReparamRat`                   | `alf bet gam del inplace`        |
//! | `ReparamWeights`    | `N_CrvReparamWeights`               | `w c d`                          |
//! | `ReparamMultKnots`  | `N_SrfReparamMultKnots`             | `tol inplace`                    |
//! | `ReparamEndWeights` | `N_SrfReparamWeights`               | `wt0 wtn`                        |
//!
//! Flags: `flg` is `1` (`NL_ABSOLUTE`) or `2` (`NL_RELATIVE`); `end` `1` (`NL_START`) or
//! `2` (`NL_END`); `cont` `2` (`NL_G1`), `3` (`NL_G2`) or `4` (`NL_CMAX`); `whr` `0..=4`
//! (`NL_NO`, `NL_START`, `NL_END`, `NL_BOTH`, `4` = an unknown flag); `opr` `1`
//! (`NL_PLUS`) or `2` (`NL_MINUS`); `inplace` `0` or `1`. A curve is encoded `n p m`,
//! `n+1` control points `x y z w`, `m+1` knots. [`eval`] accepts only well-formed
//! clamped curves (`m = n+p+1`, `1 <= p <= n`, `p <= NL_MAXDEG`, finite data,
//! consistent dimension and rationality, positive weights, non-decreasing knots with
//! end multiplicity exactly `p+1` and interior multiplicity at most `p`) and finite
//! parameters within the ranges below; everything else is rejected before the call.
//!
//! - Extensions (also the steps of `EvalUnbounded` with `NL_G1`/`NL_G2`): the end
//!   tangents fall back to the third control point from the end when the end pair
//!   cannot be normalised, so a curve with `n < 2` must have normalisable end pairs.
//! - `ExtendDist` / `ExtendParam`: the restrictions of `nlib_crv_adv_p1` (positive,
//!   bounded extensions that extend; no in-place `NL_G2` parameter extension that
//!   recurses, see `plan/findings.md`).
//! - `EvalUnbounded`: `1..=MAX_EVALS` evaluations, `ndr <= MAX_DER`, `u` at most
//!   `MAX_REL_DIST` parameter ranges outside, a parameter range of at least
//!   `MIN_REL_RANGE` times the knot magnitude (the periodic shift of a G1-closed curve
//!   loops forever upstream when it no longer changes `u`).
//! - `RefineToKnots`: `1..=MAX_KNOTS` non-decreasing knots (an empty list and knots
//!   on or outside the ends fail before any indexing and are accepted); inserted inside
//!   the range, no knot may reach a multiplicity above the degree.
//! - `SumDiff` with a rational curve: equal end knots, and knots of the two curves
//!   equal or more than `NEAR_KNOT_REL` parameter ranges apart (upstream corrupts the
//!   heap otherwise, see `plan/findings.md`).
//! - `NonRatDeriv`: `d <= p + 1`. `RemoveKnotsDeriv`: `der <= MAX_DER_CONSTRAINT`.
//! - `ReparamMultKnots`: `MIN_ARC_TOL <= tol <= 1`. `ReparamEndWeights`: non-zero end
//!   weights.

use crate::validate::{bad, euclid};
use crate::{call_with_buffer, FfiError};

/// Largest control point index accepted by [`eval`].
pub const MAX_INDEX: usize = 4096;
/// Largest degree accepted by [`eval`] (upstream `NL_MAXDEG`).
pub const MAX_DEGREE: usize = 17;
/// Largest derivative order of `EvalUnbounded`.
pub const MAX_DER: usize = 8;
/// Largest `der` of `RemoveKnotsDeriv`.
pub const MAX_DER_CONSTRAINT: usize = 64;
/// Largest number of evaluations of one `EvalUnbounded` call.
pub const MAX_EVALS: usize = 32;
/// Largest number of knots of `RefineToKnots`.
pub const MAX_KNOTS: usize = 256;
/// Largest extension (or out-of-range parameter), relative to the curve size.
pub const MAX_REL_DIST: f64 = 64.0;
/// Smallest `NL_G2` extension by distance, relative to the curve size.
pub const MIN_G2_FRACTION: f64 = 1e-3;
/// `ST_curxcrr` splits `NL_G2` extensions longer than this many parameter ranges
/// into two recursive steps.
pub const G2_SPLIT: f64 = 0.9213;
/// Smallest parameter range of `EvalUnbounded` relative to the knot magnitude.
pub const MIN_REL_RANGE: f64 = 1e-6;
/// Interior knots of a rational `SumDiff` must be equal or at least this many
/// parameter ranges apart.
pub const NEAR_KNOT_REL: f64 = 1e-9;
/// Smallest arc length tolerance of `ReparamMultKnots`.
pub const MIN_ARC_TOL: f64 = 1e-10;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `N_CrvExtendByDist`.
    ExtendDist,
    /// `N_CrvExtendByParamDist`.
    ExtendParam,
    /// `N_CrvOffsetGetMax2ndDeriv`.
    OffsetMax2nd,
    /// `N_CrvEvalUnboundedPtsAndDerivs` (a sequence of calls sharing `ecur`/`gflg`).
    EvalUnbounded,
    /// `N_CrvRemoveKnotsDerivConstraints`.
    RemoveKnotsDeriv,
    /// `N_CrvRefineToKnotVector`.
    RefineToKnots,
    /// `N_CrvSumDiffCrv`.
    SumDiff,
    /// `N_CrvNonRatGetFirstDeriv`.
    NonRatDeriv,
    /// `N_CrvRatGetFirstDeriv`.
    RatDeriv,
    /// `N_ConstantMultiplyCrv`.
    ConstMul,
    /// `N_LinearMultiplyCrv`.
    LinearMul,
    /// `N_ConstantMultiplyCrv4d`.
    ConstMul4d,
    /// `N_CrvReparamRat`.
    ReparamRat,
    /// `N_CrvReparamWeights`.
    ReparamWeights,
    /// `N_SrfReparamMultKnots`.
    ReparamMultKnots,
    /// `N_SrfReparamWeights`.
    ReparamEndWeights,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::ExtendDist => 0,
            Op::ExtendParam => 1,
            Op::OffsetMax2nd => 2,
            Op::EvalUnbounded => 3,
            Op::RemoveKnotsDeriv => 4,
            Op::RefineToKnots => 5,
            Op::SumDiff => 6,
            Op::NonRatDeriv => 7,
            Op::RatDeriv => 8,
            Op::ConstMul => 9,
            Op::LinearMul => 10,
            Op::ConstMul4d => 11,
            Op::ReparamRat => 12,
            Op::ReparamWeights => 13,
            Op::ReparamMultKnots => 14,
            Op::ReparamEndWeights => 15,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_nlib_crv_adv_p2_eval(
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
    fn finite(&mut self) -> Result<f64, FfiError> {
        let v = self.num()?;
        if !v.is_finite() {
            return Err(bad("non-finite parameter"));
        }
        Ok(v)
    }
}

/// A validated curve: Euclidean control points, degree, knots.
struct CurveInfo {
    pts: Vec<[f64; 3]>,
    p: usize,
    knots: Vec<f64>,
    rational: bool,
}

impl CurveInfo {
    fn range(&self) -> (f64, f64) {
        (
            self.knots.first().copied().unwrap_or(0.0),
            self.knots.last().copied().unwrap_or(0.0),
        )
    }

    fn polygon_length(&self) -> f64 {
        self.pts
            .iter()
            .zip(self.pts.iter().skip(1))
            .map(|(&a, &b)| dist(a, b))
            .sum()
    }

    /// Whether `pts[i] - pts[j]` is safely normalisable.
    fn pair_ok(&self, i: usize, j: usize) -> bool {
        match (self.pts.get(i), self.pts.get(j)) {
            (Some(&a), Some(&b)) => {
                let d = dist(a, b);
                d > 1e-150 && d < 1e150
            }
            _ => false,
        }
    }

    /// The end-tangent fallbacks of the extensions read the third point from the end.
    fn tangent_ok(&self, start: bool) -> Result<(), FfiError> {
        let n = self.pts.len() - 1;
        if n >= 2 {
            return Ok(());
        }
        let ok = if start {
            self.pair_ok(0, 1)
        } else {
            self.pair_ok(n, n - 1)
        };
        if ok {
            Ok(())
        } else {
            Err(bad(
                "extension: end pair not normalisable and no third point",
            ))
        }
    }
}

fn dist([ax, ay, az]: [f64; 3], [bx, by, bz]: [f64; 3]) -> f64 {
    let (x, y, z) = (ax - bx, ay - by, az - bz);
    (x * x + y * y + z * z).sqrt()
}

/// Reads and validates a curve.
fn curve(rd: &mut Reader<'_>) -> Result<CurveInfo, FfiError> {
    let n = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || m != n + p + 1 {
        return Err(bad("curve: need 1 <= p <= n and m = n + p + 1"));
    }
    let cps = rd.take(4 * (n + 1))?;
    let knots = rd.take(m + 1)?;
    let (cpts, _) = cps.as_chunks::<4>();
    let [_, _, z0, w0] = *cpts.first().ok_or_else(|| bad("empty curve"))?;
    let mut pts = Vec::with_capacity(cpts.len());
    for &[x, y, z, w] in cpts {
        if (z == f64::MAX) != (z0 == f64::MAX) || (w == -f64::MAX) != (w0 == -f64::MAX) {
            return Err(bad("curve: mixed dimension or rationality"));
        }
        if !x.is_finite() || !y.is_finite() || !z.is_finite() || !w.is_finite() {
            return Err(bad("curve: non-finite control point"));
        }
        if w != -f64::MAX && w <= 0.0 {
            return Err(bad("curve: non-positive weight"));
        }
        let e = euclid([x, y, z, w]);
        if e.iter().any(|c| !c.is_finite() || c.abs() > 1e100) {
            return Err(bad("curve: Euclidean point out of range"));
        }
        pts.push(e);
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
    Ok(CurveInfo {
        pts,
        p,
        knots: knots.to_vec(),
        rational: w0 != -f64::MAX,
    })
}

/// Validates the knots `x` of `RefineToKnots` against the curve.
fn refine_knots(info: &CurveInfo, x: &[f64]) -> Result<(), FfiError> {
    if x.iter().any(|k| !k.is_finite()) || x.windows(2).any(|w| w.first() > w.last()) {
        return Err(bad("refine: knots not non-decreasing"));
    }
    let (a, b) = info.range();
    let (Some(&first), Some(&last)) = (x.first(), x.last()) else {
        return Ok(()); // r = -1 fails first.
    };
    if first <= a || last >= b {
        return Ok(()); // Rejected before any indexing.
    }
    for &k in x {
        let mult =
            info.knots.iter().filter(|&&u| u == k).count() + x.iter().filter(|&&u| u == k).count();
        if mult > info.p {
            return Err(bad("refine: multiplicity above the degree"));
        }
    }
    Ok(())
}

/// The rational branch of `N_CrvSumDiffCrv` adds the control points of two products
/// (`D_q N_p`, `D_p N_q`) over the count of the second; when their knot vectors come
/// out differently (rescaled parameter ranges, interior knots closer than `NL_PTOL`
/// but not equal) it reads and writes past the first (heap corruption, see
/// `plan/findings.md`). Rational sums need equal end knots and interior knots that
/// are equal or clearly apart.
fn rational_sum_ok(a: &CurveInfo, b: &CurveInfo) -> Result<(), FfiError> {
    if a.range() != b.range() {
        return Err(bad("rational sum: parameter ranges differ"));
    }
    let (lo, hi) = a.range();
    let sep = NEAR_KNOT_REL * (hi - lo);
    for &x in &a.knots {
        for &y in &b.knots {
            if x != y && (x - y).abs() <= sep {
                return Err(bad("rational sum: nearly coincident knots"));
            }
        }
    }
    Ok(())
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    let info = curve(&mut rd)?;
    let flag01 = [0.0, 1.0];
    let ends = [1.0, 2.0];
    let conts = [2.0, 3.0, 4.0];
    match op {
        Op::ExtendDist => {
            let d = rd.finite()?;
            let relative = rd.choice(&[1.0, 2.0])? == 2.0;
            let start = rd.choice(&ends)? == 1.0;
            let cont = rd.choice(&conts)?;
            rd.choice(&flag01)?;
            let size = if relative { 1.0 } else { info.polygon_length() };
            if !(d > 0.0 && d <= MAX_REL_DIST * size) {
                return Err(bad("extend by distance: distance out of range"));
            }
            if cont == 3.0 && d < MIN_G2_FRACTION * size {
                return Err(bad("extend by distance: G2 extension too short"));
            }
            if cont != 4.0 {
                info.tangent_ok(start)?;
            }
        }
        Op::ExtendParam => {
            let par = rd.finite()?;
            let start = rd.choice(&ends)? == 1.0;
            let cont = rd.choice(&conts)?;
            let inplace = rd.choice(&flag01)? == 1.0;
            let (a, b) = info.range();
            let out = if start { a - par } else { par - b };
            if !(out > 0.0 && out <= MAX_REL_DIST * (b - a)) {
                return Err(bad("extend by parameter: par not outside the extended end"));
            }
            if inplace && cont == 3.0 && out > G2_SPLIT * (b - a) {
                return Err(bad(
                    "extend by parameter: in-place G2 recursion frees garbage",
                ));
            }
            if cont != 4.0 {
                info.tangent_ok(start)?;
            }
        }
        Op::OffsetMax2nd => {
            for _ in 0..4 {
                rd.finite()?;
            }
        }
        Op::EvalUnbounded => {
            let cont = rd.choice(&conts)?;
            let k = rd.index(MAX_EVALS)?;
            if k == 0 {
                return Err(bad("unbounded eval: no evaluation"));
            }
            let (a, b) = info.range();
            if b - a < MIN_REL_RANGE * a.abs().max(b.abs()) {
                return Err(bad("unbounded eval: parameter range too small"));
            }
            for _ in 0..k {
                let u = rd.finite()?;
                rd.index(MAX_DER)?;
                if u < a - MAX_REL_DIST * (b - a) || u > b + MAX_REL_DIST * (b - a) {
                    return Err(bad("unbounded eval: parameter too far out"));
                }
            }
            if cont != 4.0 {
                info.tangent_ok(true)?;
                info.tangent_ok(false)?;
            }
        }
        Op::RemoveKnotsDeriv => {
            rd.finite()?;
            rd.choice(&[0.0, 1.0, 2.0, 3.0, 4.0])?;
            rd.index(MAX_DER_CONSTRAINT)?;
            rd.choice(&flag01)?;
        }
        Op::RefineToKnots => {
            let cnt = rd.index(MAX_KNOTS)?;
            let x = rd.take(cnt)?;
            refine_knots(&info, x)?;
        }
        Op::SumDiff => {
            let other = curve(&mut rd)?;
            rd.choice(&[1.0, 2.0])?;
            if info.rational || other.rational {
                rational_sum_ok(&info, &other)?;
            }
        }
        Op::NonRatDeriv => {
            rd.index(info.p + 1)?;
        }
        Op::RatDeriv => {}
        Op::ConstMul | Op::ConstMul4d => {
            rd.finite()?;
        }
        Op::LinearMul => {
            rd.finite()?;
            rd.finite()?;
        }
        Op::ReparamRat => {
            for _ in 0..4 {
                rd.finite()?;
            }
            rd.choice(&flag01)?;
        }
        Op::ReparamWeights => {
            for _ in 0..3 {
                rd.finite()?;
            }
        }
        Op::ReparamMultKnots => {
            let tol = rd.finite()?;
            if !(MIN_ARC_TOL..=1.0).contains(&tol) {
                return Err(bad("reparam mult knots: tolerance out of range"));
            }
            rd.choice(&flag01)?;
        }
        Op::ReparamEndWeights => {
            for _ in 0..2 {
                if rd.finite()? == 0.0 {
                    return Err(bad("reparam end weights: zero weight"));
                }
            }
        }
    }
    rd.done()
}

/// Runs `op` on `input` (curve, then the op's parameters); returns the shim's output
/// record (status, then outputs).
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the
        // shim reads for `op` (validated above: well-formed clamped curves, finite
        // parameters in the ranges upstream handles, known flags); buf is valid for
        // buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_nlib_crv_adv_p2_eval(
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
