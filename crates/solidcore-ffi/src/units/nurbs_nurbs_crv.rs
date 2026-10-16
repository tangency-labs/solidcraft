//! FFI wrappers for unit `nurbs.nurbs_crv`.
//!
//! Shim: `harness/cpp/shim/nurbs_nurbs_crv.cpp` (`sc_nurbs_nurbs_crv_eval`).
//! Upstream: `source/SMLib/src/SmNurbsCrv.cpp`.
//!
//! | [`Op`]          | upstream                                   | input                       |
//! |-----------------|--------------------------------------------|-----------------------------|
//! | `Knots`         | `sm_GetKnots`                              | `nk U[nk] has_ivl a b`      |
//! | `Copy`          | `sm_AllocateAndCopyNurbCurve`, `sm_CopyNurbCurve` | curve                |
//! | `Fixup`         | `sm_FixupKnotVector`                       | `nk U[nk] tol`              |
//! | `Bez`           | `sm_CreateBezSegments`                     | curve                       |
//! | `Split`         | `sm_CrvSplit`                              | curve `u`                   |
//! | `Trim`          | `sm_CrvTrim`                               | curve `ul ur`               |
//! | `EPolygon`      | `sm_CrvGetEPolygon`                        | curve `k l`                 |
//! | `BasisDerivs`   | `sm_BasisDerivs` (curve knots and degree)  | curve `u side der`          |
//! | `RatDerivs`     | `sm_CrvRationalBasisDerivs`                | curve `u side der`          |
//! | `CrvDerivs`     | `sm_CrvDerivs`                             | curve `u side der`          |
//! | `BasisEval`     | `sm_BasisEval`                             | curve `u side`              |
//! | `CrvEval`       | `sm_CrvEval`                               | curve `u side`              |
//! | `RemoveKnots`   | `sm_CrvRemoveKnots` (in place)             | curve `tol`                 |
//! | `SpanMaxima`    | `sm_BasisFindAllSpanMaxima`                | curve `i tol`               |
//! | `BasisIEval`    | `sm_BasisIEval`                            | curve `i u side`            |
//! | `MaxErr`        | `sm_CrvRemoveKnotMaxErr`                   | curve `r s`                 |
//! | `BasisIDerivs`  | `sm_BasisIDerivs`                          | curve `i u side der`        |
//! | `GlobalMax`     | `sm_BasisFindGlobalMax`                    | curve `i tol`               |
//! | `Degenerate`    | `sm_IsNCrvDegenerate`                      | curve `tol`                 |
//! | `Frame`         | `sm_ExtractFrame` (of `SmAxis2Placement(O, X, Y)`) | `O[3] X[3] Y[3]`    |
//!
//! A curve is encoded `n p m`, `n+1` control points `x y z w`, `m+1` knots; `side` is
//! `1` (`NL_LEFT`) or `2` (`NL_RIGHT`). [`eval`] accepts only well-formed clamped
//! curves (`m = n+p+1`, `1 <= p <= n`, `p <= 17`, finite data, consistent dimension
//! and rationality, non-decreasing knots with end multiplicity exactly `p+1` and
//! interior multiplicity at most `p`) and parameters within the ranges below:
//! `BasisDerivs` / `RatDerivs` need `der <= 4`, `CrvDerivs` `der <= 4` or `6..=8`
//! (upstream overruns its arrays at `der == 5`), `MaxErr` an interior knot index `r`
//! with `U[r] != U[r+1]` and its multiplicity `s`. Indices `k`, `l`, `i` may exceed
//! their ranges by a little (upstream checks them first).

use crate::{call_with_buffer, FfiError};

/// Largest control point index accepted by [`eval`].
pub const MAX_INDEX: usize = 4096;
/// Largest degree accepted by [`eval`].
pub const MAX_DEGREE: usize = 17;
/// Largest knot count of the knot-only ops.
pub const MAX_KNOTS: usize = 1 << 16;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `sm_GetKnots`.
    Knots,
    /// `sm_AllocateAndCopyNurbCurve` then `sm_CopyNurbCurve`.
    Copy,
    /// `sm_FixupKnotVector`.
    Fixup,
    /// `sm_CreateBezSegments`.
    Bez,
    /// `sm_CrvSplit`.
    Split,
    /// `sm_CrvTrim`.
    Trim,
    /// `sm_CrvGetEPolygon`.
    EPolygon,
    /// `sm_BasisDerivs`.
    BasisDerivs,
    /// `sm_CrvRationalBasisDerivs`.
    RatDerivs,
    /// `sm_CrvDerivs`.
    CrvDerivs,
    /// `sm_BasisEval`.
    BasisEval,
    /// `sm_CrvEval`.
    CrvEval,
    /// `sm_CrvRemoveKnots`.
    RemoveKnots,
    /// `sm_BasisFindAllSpanMaxima`.
    SpanMaxima,
    /// `sm_BasisIEval`.
    BasisIEval,
    /// `sm_CrvRemoveKnotMaxErr`.
    MaxErr,
    /// `sm_BasisIDerivs`.
    BasisIDerivs,
    /// `sm_BasisFindGlobalMax`.
    GlobalMax,
    /// `sm_IsNCrvDegenerate`.
    Degenerate,
    /// `sm_ExtractFrame`.
    Frame,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::Knots => 0,
            Op::Copy => 1,
            Op::Fixup => 2,
            Op::Bez => 3,
            Op::Split => 4,
            Op::Trim => 5,
            Op::EPolygon => 6,
            Op::BasisDerivs => 7,
            Op::RatDerivs => 8,
            Op::CrvDerivs => 9,
            Op::BasisEval => 10,
            Op::CrvEval => 11,
            Op::RemoveKnots => 12,
            Op::SpanMaxima => 13,
            Op::BasisIEval => 14,
            Op::MaxErr => 15,
            Op::BasisIDerivs => 16,
            Op::GlobalMax => 17,
            Op::Degenerate => 18,
            Op::Frame => 19,
        }
    }

    fn has_curve(self) -> bool {
        !matches!(self, Op::Knots | Op::Fixup | Op::Frame)
    }
}

mod sys {
    extern "C" {
        pub fn sc_nurbs_nurbs_crv_eval(
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

/// Sequential reader over the input.
struct Reader<'a> {
    input: &'a [f64],
    at: usize,
}

impl<'a> Reader<'a> {
    fn num(&mut self) -> Result<f64, FfiError> {
        let v = self
            .input
            .get(self.at)
            .copied()
            .ok_or_else(|| bad("input too short"))?;
        self.at += 1;
        Ok(v)
    }

    fn finite(&mut self) -> Result<f64, FfiError> {
        let v = self.num()?;
        if !v.is_finite() {
            return Err(bad("non-finite parameter"));
        }
        Ok(v)
    }

    fn finites(&mut self, k: usize) -> Result<(), FfiError> {
        for _ in 0..k {
            self.finite()?;
        }
        Ok(())
    }

    fn index(&mut self, max: usize) -> Result<usize, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < 0.0 || v > max as f64 {
            return Err(bad(format!("bad index/count {v}")));
        }
        Ok(v as usize)
    }

    fn flag(&mut self) -> Result<bool, FfiError> {
        Ok(self.index(1)? == 1)
    }

    fn side(&mut self) -> Result<(), FfiError> {
        let v = self.num()?;
        if v != 1.0 && v != 2.0 {
            return Err(bad(format!("bad side flag {v}")));
        }
        Ok(())
    }

    fn take(&mut self, k: usize) -> Result<&'a [f64], FfiError> {
        let end = self.at.checked_add(k).ok_or_else(|| bad("overflow"))?;
        let s = self
            .input
            .get(self.at..end)
            .ok_or_else(|| bad("input too short"))?;
        self.at = end;
        Ok(s)
    }

    fn done(&self) -> Result<(), FfiError> {
        if self.at != self.input.len() {
            return Err(bad("trailing input"));
        }
        Ok(())
    }
}

/// A validated curve: `(n, p, knots)`.
struct Crv<'a> {
    n: usize,
    p: usize,
    knots: &'a [f64],
}

/// Reads and validates a curve.
fn curve<'a>(rd: &mut Reader<'a>) -> Result<Crv<'a>, FfiError> {
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
    Ok(Crv { n, p, knots })
}

/// A finite knot array `nk U[nk]` with `nk >= 1`.
fn knot_array(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let nk = rd.index(MAX_KNOTS)?;
    if nk == 0 {
        return Err(bad("empty knot array"));
    }
    rd.finites(nk)
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
    let c = if op.has_curve() {
        Some(curve(&mut rd)?)
    } else {
        None
    };
    let need = || bad("op needs a curve");
    match op {
        Op::Knots => {
            knot_array(&mut rd)?;
            rd.flag()?;
            rd.finites(2)?;
        }
        Op::Fixup => {
            knot_array(&mut rd)?;
            rd.finite()?;
        }
        Op::Copy | Op::Bez => {}
        Op::Split | Op::RemoveKnots | Op::Degenerate => {
            rd.finite()?;
        }
        Op::Trim => rd.finites(2)?,
        Op::EPolygon => {
            let c = c.ok_or_else(need)?;
            rd.index(c.n + 3)?;
            rd.index(c.n + 3)?;
        }
        Op::BasisDerivs | Op::RatDerivs => {
            rd.finite()?;
            rd.side()?;
            rd.index(4)?;
        }
        Op::CrvDerivs => {
            rd.finite()?;
            rd.side()?;
            if rd.index(8)? == 5 {
                return Err(bad("der == 5 overruns upstream's arrays"));
            }
        }
        Op::BasisEval | Op::CrvEval => {
            rd.finite()?;
            rd.side()?;
        }
        Op::SpanMaxima | Op::GlobalMax => {
            let c = c.ok_or_else(need)?;
            rd.index(c.n + 3)?;
            rd.finite()?;
        }
        Op::BasisIEval => {
            let c = c.ok_or_else(need)?;
            rd.index(c.n + 3)?;
            rd.finite()?;
            rd.side()?;
        }
        Op::BasisIDerivs => {
            let c = c.ok_or_else(need)?;
            rd.index(c.n + 3)?;
            rd.finite()?;
            rd.side()?;
            rd.index(8)?;
        }
        Op::MaxErr => {
            let c = c.ok_or_else(need)?;
            let r = rd.index(c.n)?;
            let s = rd.index(c.p)?;
            let at = |i: usize| c.knots.get(i).copied();
            if r < c.p + 1 || at(r) == at(r + 1) {
                return Err(bad(
                    "max_err: r must be an interior knot with U[r] != U[r+1]",
                ));
            }
            let mult = (0..=r).rev().take_while(|&k| at(k) == at(r)).count();
            if s != mult {
                return Err(bad("max_err: s must be the multiplicity of U[r]"));
            }
        }
        Op::Frame => rd.finites(9)?,
    }
    rd.done()
}

/// Runs `op` on `input` (layouts in the module docs); returns the shim's output record
/// (flag, then outputs).
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(1024, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the
        // shim reads for `op` (validated above: a well-formed clamped curve and every
        // index, count and flag the shim passes to upstream); buf is valid for
        // buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_nurbs_nurbs_crv_eval(
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
