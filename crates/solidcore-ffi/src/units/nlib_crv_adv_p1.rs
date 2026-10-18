//! FFI wrappers for unit `nlib.crv_adv_p1`.
//!
//! Shim: `harness/cpp/shim/nlib_crv_adv_p1.cpp` (`sc_nlib_crv_adv_p1_eval`).
//! Upstream: `source/SMLib/src/NL_CrvAdv.cpp`.
//!
//! | [`Op`]           | upstream                                   | parameters after the curve |
//! |------------------|--------------------------------------------|----------------------------|
//! | `EvalPt`         | `N_CrvEvalPt`                              | `u flg`                    |
//! | `DerivsAtKnot`   | `N_CrvDerivsAtKnot`                        | `u flg der`                |
//! | `EvenSpaced`     | `N_CrvEvalEvenSpacedPts`                   | `u0 u1 n tol`              |
//! | `Max2ndDeriv`    | `N_CrvGetMax2ndDeriv`                      | (none)                     |
//! | `PlanarOffset`   | `N_CrvPlanarOffsetGetDeriv`                | `u d N[3] flg`             |
//! | `ExtendDist`     | `N_CrvExtendByDist` (`ST_curxcdr`)         | `dist flg end cont inplace` |
//! | `ExtendParam`    | `N_CrvExtendByParamDist` (`ST_curxcrr`)    | `par end cont inplace`     |
//!
//! `flg` of the evaluations is `1` (`NL_LEFT`) or `2` (`NL_RIGHT`); `flg` of
//! `ExtendDist` is `1` (`NL_ABSOLUTE`) or `2` (`NL_RELATIVE`); `end` is `1`
//! (`NL_START`) or `2` (`NL_END`); `cont` is `2` (`NL_G1`), `3` (`NL_G2`) or `4`
//! (`NL_CMAX`); `inplace` is `0` or `1`. A curve is encoded `n p m`, `n+1` control
//! points `x y z w`, `m+1` knots. [`eval`] accepts only well-formed clamped curves
//! (`m = n+p+1`, `1 <= p <= n`, `p <= NL_MAXDEG`, finite data, consistent dimension
//! and rationality, positive weights, non-decreasing knots with end multiplicity
//! exactly `p+1` and interior multiplicity at most `p`) and finite parameters within
//! the ranges below; everything else is rejected before the call.
//!
//! - `DerivsAtKnot`: `der <= MAX_DER`.
//! - `EvenSpaced`: `n <= MAX_POINTS`.
//! - Extensions: the end tangents fall back to the third control point from the end
//!   when the end pair cannot be normalised, so a curve with `n < 2` must have a
//!   normalisable end pair (upstream reads past its arrays otherwise).
//! - `ExtendDist`: `0 < dist`, at most `MAX_REL_DIST` (relative) or `MAX_REL_DIST`
//!   times the control polygon length (absolute). With `NL_G2` the parameter search
//!   takes about `arc length / extension` steps and never ends when the step is lost
//!   to rounding, so the extension must be at least `MIN_G2_FRACTION` of the curve
//!   (relative `dist`, or absolute `dist` against the control polygon length).
//! - `ExtendParam`: `par` strictly outside the knot range on the extended side
//!   (upstream mirrors and continues spans of non-positive length otherwise), at most
//!   `MAX_REL_DIST` parameter ranges away. In place (`inplace = 1`) with `NL_G2`,
//!   `par` must not trigger the two-step recursion (more than `G2_SPLIT` parameter
//!   ranges away): `ST_curxcrr` then exits before saving the input curve and frees an
//!   uninitialised curve (`N_FreeCrv(&curA, SP)`, see `plan/findings.md`).

use crate::validate::{bad, euclid};
use crate::{call_with_buffer, FfiError};

/// Largest control point index accepted by [`eval`].
pub const MAX_INDEX: usize = 4096;
/// Largest degree accepted by [`eval`] (upstream `NL_MAXDEG`).
pub const MAX_DEGREE: usize = 17;
/// Largest derivative order of `DerivsAtKnot`.
pub const MAX_DER: usize = 8;
/// Largest point count index of `EvenSpaced`.
pub const MAX_POINTS: usize = 4096;
/// Largest extension, relative to the curve size (see the module docs).
pub const MAX_REL_DIST: f64 = 64.0;
/// Smallest `NL_G2` extension by distance, relative to the curve size.
pub const MIN_G2_FRACTION: f64 = 1e-3;
/// `ST_curxcrr` splits `NL_G2` extensions longer than this many parameter ranges
/// into two recursive steps.
pub const G2_SPLIT: f64 = 0.9213;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `N_CrvEvalPt`.
    EvalPt,
    /// `N_CrvDerivsAtKnot`.
    DerivsAtKnot,
    /// `N_CrvEvalEvenSpacedPts`.
    EvenSpaced,
    /// `N_CrvGetMax2ndDeriv`.
    Max2ndDeriv,
    /// `N_CrvPlanarOffsetGetDeriv`.
    PlanarOffset,
    /// `N_CrvExtendByDist` (`ST_curxcdr` at depth 0).
    ExtendDist,
    /// `N_CrvExtendByParamDist` (`ST_curxcrr` at depth 0).
    ExtendParam,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::EvalPt => 0,
            Op::DerivsAtKnot => 1,
            Op::EvenSpaced => 2,
            Op::Max2ndDeriv => 3,
            Op::PlanarOffset => 4,
            Op::ExtendDist => 5,
            Op::ExtendParam => 6,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_nlib_crv_adv_p1_eval(
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

/// A validated curve: Euclidean control points and knot range.
struct CurveInfo {
    pts: Vec<[f64; 3]>,
    range: (f64, f64),
}

impl CurveInfo {
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
    Ok(CurveInfo { pts, range: (a, b) })
}

/// The end-tangent fallbacks of the extensions read the third point from the end.
fn tangent_ok(info: &CurveInfo, start: bool) -> Result<(), FfiError> {
    let n = info.pts.len() - 1;
    if n >= 2 {
        return Ok(());
    }
    let ok = if start {
        info.pair_ok(0, 1)
    } else {
        info.pair_ok(n, n - 1)
    };
    if ok {
        Ok(())
    } else {
        Err(bad(
            "extension: end pair not normalisable and no third point",
        ))
    }
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    let info = curve(&mut rd)?;
    let side = [1.0, 2.0];
    match op {
        Op::EvalPt => {
            rd.finite()?;
            rd.choice(&side)?;
        }
        Op::DerivsAtKnot => {
            rd.finite()?;
            rd.choice(&side)?;
            rd.index(MAX_DER)?;
        }
        Op::EvenSpaced => {
            rd.finite()?;
            rd.finite()?;
            rd.index(MAX_POINTS)?;
            rd.finite()?;
        }
        Op::Max2ndDeriv => {}
        Op::PlanarOffset => {
            for _ in 0..5 {
                rd.finite()?;
            }
            rd.choice(&side)?;
        }
        Op::ExtendDist => {
            let d = rd.finite()?;
            let relative = rd.choice(&[1.0, 2.0])? == 2.0;
            let start = rd.choice(&[1.0, 2.0])? == 1.0;
            let cont = rd.choice(&[2.0, 3.0, 4.0])?;
            rd.choice(&[0.0, 1.0])?;
            let size = if relative { 1.0 } else { info.polygon_length() };
            if !(d > 0.0 && d <= MAX_REL_DIST * size) {
                return Err(bad("extend by distance: distance out of range"));
            }
            if cont == 3.0 && d < MIN_G2_FRACTION * size {
                return Err(bad("extend by distance: G2 extension too short"));
            }
            if cont != 4.0 {
                tangent_ok(&info, start)?;
            }
        }
        Op::ExtendParam => {
            let par = rd.finite()?;
            let start = rd.choice(&[1.0, 2.0])? == 1.0;
            let cont = rd.choice(&[2.0, 3.0, 4.0])?;
            let inplace = rd.choice(&[0.0, 1.0])? == 1.0;
            let (a, b) = info.range;
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
                tangent_ok(&info, start)?;
            }
        }
    }
    rd.done()
}

/// Runs `op` on `input` (curve, then the op's parameters); returns the shim's output
/// record (status, then outputs on success).
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the
        // shim reads for `op` (validated above: a well-formed clamped curve, finite
        // parameters in the ranges upstream handles, known flags); buf is valid for
        // buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_nlib_crv_adv_p1_eval(
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
