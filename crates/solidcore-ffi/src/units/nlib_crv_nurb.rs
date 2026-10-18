//! FFI wrappers for unit `nlib.crv_nurb`.
//!
//! Shim: `harness/cpp/shim/nlib_crv_nurb.cpp` (`sc_nlib_crv_nurb_eval`).
//! Upstream: `source/SMLib/src/NL_CrvNurb.cpp`.
//!
//! | [`Op`]         | upstream                        | parameters after the curve |
//! |----------------|---------------------------------|----------------------------|
//! | `Greville`     | `N_CrvGetGrevilleAbscissa`      | `deg idx` (curve knots)    |
//! | `Curvature`    | `N_CrvEvalCurvature`            | `u side cfl`               |
//! | `Eval`         | `N_CrvEval`                     | `u side`                   |
//! | `BBox`         | `N_CrvGetBBox`                  | (none)                     |
//! | `Derivs`       | `N_CrvDerivs`                   | `u side der`               |
//! | `Tangent`      | `N_CrvEvalTangent`              | `u side`                   |
//! | `Frenet`       | `N_CrvEvalFrenetFrame`          | `u side`                   |
//! | `Transform`    | `N_CrvTransform`                | `M[16]` (row major)        |
//! | `MinMax`       | `N_CrvGetMinMaxWeightsAndPts`   | (none)                     |
//! | `Line`         | `N_CrvLineFromPtAndVector`      | `P[3] V[3]` (no curve)     |
//! | `Scale`        | `N_CrvScale`                    | `C[3] f[3]`                |
//! | `ScaleCPts`    | `N_CrvScaleCPts`                | `f`                        |
//! | `Translate`    | `N_CrvTranslate`                | `T[3]`                     |
//! | `Project`      | `N_CrvProjectOntoPlane`         | `O[3] N[3] E[3] prj inplace` |
//! | `Reverse`      | `N_CrvReverse`                  | `inplace`                  |
//! | `BBoxDiag`     | `N_CrvGetBBoxMaxDiagDist`       | (none)                     |
//! | `MaxPos`       | `N_CrvGetMaxPosVector`          | (none)                     |
//! | `LinearSegs`   | `N_CrvGetLinearSegs`            | `tol`                      |
//! | `DegenSegs`    | `N_CrvGetDegenSegs`             | `tol`                      |
//! | `Reparam`      | `N_CrvReparam`                  | `ul ur`                    |
//! | `Type`         | `N_CrvGetType`                  | `tol`                      |
//! | `CurvDeriv`    | `N_CrvGetCurvatureDeriv`        | `u side`                   |
//! | `Closed`       | `N_CrvIsClosedContinuity`       | `n tol0 tol1 tol2`         |
//!
//! `side` is `1` (`NL_LEFT`) or `2` (`NL_RIGHT`), `prj` `1` (`NL_PARALLEL`) or `2`
//! (`NL_PERSPECTIVE`), flags `0` or `1`. A curve is encoded `n p m`, `n+1` control
//! points `x y z w`, `m+1` knots. [`eval`] accepts only well-formed clamped curves
//! (`m = n+p+1`, `1 <= p <= n`, `p <= NL_MAXDEG`, finite data, consistent dimension and
//! rationality, non-decreasing knots with end multiplicity exactly `p+1` and interior
//! multiplicity at most `p`) and finite parameters within the ranges below; everything
//! else is rejected before the call.

use crate::{call_with_buffer, FfiError};

/// Largest control point index accepted by [`eval`].
pub const MAX_INDEX: usize = 4096;
/// Largest degree accepted by [`eval`] (upstream `NL_MAXDEG`).
pub const MAX_DEGREE: usize = 17;
/// Largest derivative order accepted by [`eval`] (upstream rejects above 4 itself).
pub const MAX_DER: usize = 8;
/// Largest Greville degree / index argument accepted by [`eval`].
pub const MAX_GREVILLE: usize = 1 << 20;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `N_CrvGetGrevilleAbscissa`.
    Greville,
    /// `N_CrvEvalCurvature`.
    Curvature,
    /// `N_CrvEval`.
    Eval,
    /// `N_CrvGetBBox`.
    BBox,
    /// `N_CrvDerivs`.
    Derivs,
    /// `N_CrvEvalTangent`.
    Tangent,
    /// `N_CrvEvalFrenetFrame`.
    Frenet,
    /// `N_CrvTransform`.
    Transform,
    /// `N_CrvGetMinMaxWeightsAndPts`.
    MinMax,
    /// `N_CrvLineFromPtAndVector`.
    Line,
    /// `N_CrvScale`.
    Scale,
    /// `N_CrvScaleCPts`.
    ScaleCPts,
    /// `N_CrvTranslate`.
    Translate,
    /// `N_CrvProjectOntoPlane`.
    Project,
    /// `N_CrvReverse`.
    Reverse,
    /// `N_CrvGetBBoxMaxDiagDist`.
    BBoxDiag,
    /// `N_CrvGetMaxPosVector`.
    MaxPos,
    /// `N_CrvGetLinearSegs`.
    LinearSegs,
    /// `N_CrvGetDegenSegs`.
    DegenSegs,
    /// `N_CrvReparam`.
    Reparam,
    /// `N_CrvGetType`.
    Type,
    /// `N_CrvGetCurvatureDeriv`.
    CurvDeriv,
    /// `N_CrvIsClosedContinuity`.
    Closed,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::Greville => 0,
            Op::Curvature => 1,
            Op::Eval => 2,
            Op::BBox => 3,
            Op::Derivs => 4,
            Op::Tangent => 5,
            Op::Frenet => 6,
            Op::Transform => 7,
            Op::MinMax => 8,
            Op::Line => 9,
            Op::Scale => 10,
            Op::ScaleCPts => 11,
            Op::Translate => 12,
            Op::Project => 13,
            Op::Reverse => 14,
            Op::BBoxDiag => 15,
            Op::MaxPos => 16,
            Op::LinearSegs => 17,
            Op::DegenSegs => 18,
            Op::Reparam => 19,
            Op::Type => 20,
            Op::CurvDeriv => 21,
            Op::Closed => 22,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_nlib_crv_nurb_eval(
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

    fn flag(&mut self) -> Result<(), FfiError> {
        self.index(1).map(|_| ())
    }
}

/// Reads and validates a curve.
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

impl Reader<'_> {
    fn finites(&mut self, k: usize) -> Result<(), FfiError> {
        for _ in 0..k {
            self.finite()?;
        }
        Ok(())
    }

    fn side(&mut self) -> Result<(), FfiError> {
        let v = self.num()?;
        if v != 1.0 && v != 2.0 {
            return Err(bad(format!("bad side/projection flag {v}")));
        }
        Ok(())
    }
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    if op != Op::Line {
        curve(&mut rd)?;
    }
    match op {
        Op::Greville => {
            rd.index(MAX_GREVILLE)?;
            rd.index(MAX_GREVILLE)?;
        }
        Op::Curvature => {
            rd.finite()?;
            rd.side()?;
            rd.flag()?;
        }
        Op::Eval | Op::Tangent | Op::Frenet | Op::CurvDeriv => {
            rd.finite()?;
            rd.side()?;
        }
        Op::Derivs => {
            rd.finite()?;
            rd.side()?;
            rd.index(MAX_DER)?;
        }
        Op::BBox | Op::MinMax | Op::BBoxDiag | Op::MaxPos => {}
        Op::Transform => rd.finites(16)?,
        Op::Line | Op::Scale => rd.finites(6)?,
        Op::ScaleCPts | Op::LinearSegs | Op::DegenSegs | Op::Type => rd.finite().map(|_| ())?,
        Op::Translate => rd.finites(3)?,
        Op::Project => {
            rd.finites(9)?;
            rd.side()?;
            rd.flag()?;
        }
        Op::Reverse => rd.flag()?,
        Op::Reparam => rd.finites(2)?,
        Op::Closed => {
            rd.index(3)?;
            rd.finites(3)?;
        }
    }
    rd.done()
}

/// Runs `op` on `input` (curve unless [`Op::Line`], then the op's parameters);
/// returns the shim's output record (status, then outputs on success).
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(1024, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the
        // shim reads for `op` (validated above: a well-formed clamped curve and every
        // index, count and flag the shim passes to upstream); buf is valid for
        // buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_nlib_crv_nurb_eval(
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
