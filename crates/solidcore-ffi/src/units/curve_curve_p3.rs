//! FFI wrappers for unit `curve.curve_p3`.
//!
//! Shim: `harness/cpp/shim/curve_curve_p3.cpp` (`sc_curve_curve_p3_eval`). Upstream:
//! `source/SMLib/src/SmCurve.cpp`, `source/SMLib/inc/SmCurve.h`.
//!
//! A curve block is `0 dim oob n p m cpts(x y z w) knots` (an `SmBSplineCurve` built
//! with `SmBSplineCurve(gw_CURVE*, dim, FALSE)` and `SetOutOfBoundsEnabled(oob)`),
//! `1 dim P1[3] D1[3] P2[3] D2[3]` (an `SmHermiteCurve`) or `2 3 P[3] V[3] d0 d1 scale io`
//! (a bounded `SmLine(P, V, [d0, d1], scale, 3, ctx, NULL, io)`). `vecs` is `nv V[nv]`
//! (`NULL` for `nv = 0`). Every output starts with the status (`0` = `SM_SUCCESS`, else
//! the `SmStatus`); the rest:
//!
//! | [`Op`]          | upstream                                   | parameters                         | outputs |
//! |-----------------|--------------------------------------------|------------------------------------|---------|
//! | `Normal`        | `SmCurve::ComputeCrvsNormal`               | `n curve*n`                        | `N[3]` (on success) |
//! | `FixGaps`       | `SmCurve::CrvsFixGaps`                     | `n bspline*n tol`                  | `fixed nc (origin ncp Pw[4 ncp])*nc` |
//! | `ArcLength`     | `FindParameterAtArcLength`                 | curve `start len`                  | `end past` (also with `SM_ERR_NOT_CONVERGING`) |
//! | `PcExtrema`     | `SmFindPCExtremaEFO::Evaluate`             | curve `P a b sqtol sop nt t[nt]`   | `(status f fp found)*nt` |
//! | `LineFit`       | `SmBestLineFitENFO::Evaluate`              | curve curve `x0 x1 jac`            | `f[2] J[4] found size scaled_zero` (always) |
//! | `PointSolve`    | `LocalPointSolve`                          | curve `a b sop P has_dt dt has_tg tg` vecs `guess` | `found` sol |
//! | `CurveSolve`    | `LocalCurveSolve`                          | curve `a1 b1` curve `a2 b2 sop tol has_tg tg` vecs `g1 g2` | `found` sol |
//! | `Intersect`     | `LocalCurveIntersect`                      | curve `a1 b1` curve `a2 b2 tol g1 g2` | `found [my other dev]` |
//! | `PropertyLocal` | `SmCurvePropertyGS::LocalSolve`            | curve cache solver `leaf`          | `nleaf [status needs nsol sol*]` |
//! | `IsLinear`      | `SmCurve::IsLinear`                        | curve `tol want`                   | `linear [P[3] T[3]] dev` |
//!
//! A solution `sol` is `type nvar nobj o0 o1 value s0 s1 [evalue e0 e1]` (the end for a
//! range); objects are `0` (this curve), `1` (the other), `2` (another object), `-1`
//! (`NULL`). For `PropertyLocal` the curve (B-spline or Hermite) is cached by
//! `SmCurveCache(curve, [a, b], full, chord, angle, offaxis, minseg, aux, max3d, ratio)`
//! (`cache` = `a b full chord angle offaxis minseg aux max3d ratio`, then
//! `Tessellate`, whose status is the first output); `solver` is `i0 i1 prop has_val val`
//! vecs `sop tol best` (an `SmCurvePropertyGS` over the cache tree and `[i0, i1]` with
//! that operation, tolerance and best answer); `LocalSolve` runs on the leaf
//! `leaf % nleaf` of the pre-order leaf (span) list.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour or runaway
//! work: well-formed clamped B-splines (`m = n+p+1`, `1 <= p <= n`, `p <= 9`, finite
//! bounded data, positive weights, end multiplicity `p+1`, interior at most `p`) of
//! dimension 2 or 3, finite bounded Hermite data, lines with non-zero vectors and
//! positive scales, parameters within a few curve lengths of the curve's range,
//! increasing intervals, option vectors and values wherever upstream dereferences them
//! without a `NULL` check, only B-splines for `FixGaps` (upstream casts), a backwards
//! arc length only from the first knot on (upstream reads before it otherwise), and cache
//! options as in unit `curve.curve_cache`.

use crate::validate::{bad, euclid, NOW, NOZ};
use crate::{call_with_buffer, FfiError};

/// Largest control point index `n`.
pub const MAX_INDEX: usize = 128;
/// Largest degree.
pub const MAX_DEGREE: usize = 9;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1e6;
/// Parameters may lie this many interval lengths outside the curve's range.
pub const MAX_REL_DIST: f64 = 2.0;
/// Most curves of `Normal` and `FixGaps`.
pub const MAX_CURVES: usize = 12;
/// Most parameters of `PcExtrema`.
pub const MAX_PARAMS: usize = 8;
/// Largest `lMinimumNumberOfSegments` of the cache.
pub const MAX_SEGMENTS: usize = 64;
/// Smallest `dMinimumParametricRatio` of the cache.
pub const MIN_RATIO: f64 = 1e-4;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Normal,
    FixGaps,
    ArcLength,
    PcExtrema,
    LineFit,
    PointSolve,
    CurveSolve,
    Intersect,
    PropertyLocal,
    IsLinear,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Op::Normal => 0,
            Op::FixGaps => 1,
            Op::ArcLength => 2,
            Op::PcExtrema => 3,
            Op::LineFit => 4,
            Op::PointSolve => 5,
            Op::CurveSolve => 6,
            Op::Intersect => 7,
            Op::PropertyLocal => 8,
            Op::IsLinear => 9,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_curve_curve_p3_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

crate::reader::unit_reader!(MAX_COORD);

/// The parameter range of a validated curve block.
#[derive(Debug, Clone, Copy)]
struct CurveInfo {
    a: f64,
    b: f64,
}

/// A B-spline `n p m cpts knots`.
fn bspline(rd: &mut Reader<'_>) -> Result<(f64, f64), FfiError> {
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
    for &[x, y, z, w] in cpts {
        if (z == NOZ) != (z0 == NOZ) || (w == NOW) != (w0 == NOW) {
            return Err(bad("curve: mixed dimension or rationality"));
        }
        if !x.is_finite()
            || !y.is_finite()
            || (z != NOZ && !z.is_finite())
            || (w != NOW && !w.is_finite())
        {
            return Err(bad("curve: non-finite control point"));
        }
        if w != NOW && w <= 1e-3 {
            return Err(bad("curve: non-positive weight"));
        }
        if euclid([x, y, z, w])
            .iter()
            .any(|c| !c.is_finite() || c.abs() > MAX_COORD)
        {
            return Err(bad("curve: Euclidean point out of range"));
        }
    }
    if knots.iter().any(|k| !k.is_finite() || k.abs() > MAX_COORD)
        || knots.windows(2).any(|w| w.first() > w.last())
    {
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
    if b - a < 1e-6 * a.abs().max(b.abs()).max(1.0) {
        return Err(bad("curve: parameter range too small"));
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
    Ok((a, b))
}

/// A line `P V d0 d1 scale io`; its analytic range.
fn line(rd: &mut Reader<'_>) -> Result<(f64, f64), FfiError> {
    rd.vec3()?;
    rd.nonzero3()?;
    let d0 = rd.finite()?;
    let d1 = rd.finite()?;
    if (d1 - d0).abs() < 1e-6 {
        return Err(bad("line: domain too small"));
    }
    let scale = rd.finite()?;
    if scale <= 1e-6 {
        return Err(bad("line: scale not positive"));
    }
    rd.flag()?;
    Ok((d0.min(d1), d0.max(d1)))
}

/// A curve block (see the module docs).
fn curve_block(rd: &mut Reader<'_>) -> Result<CurveInfo, FfiError> {
    let kind = rd.index(2)?;
    let dim = rd.index(3)?;
    if dim < 2 {
        return Err(bad("curve block: dimension must be 2 or 3"));
    }
    match kind {
        0 => {
            rd.flag()?;
            let (a, b) = bspline(rd)?;
            Ok(CurveInfo { a, b })
        }
        1 => {
            for _ in 0..4 {
                rd.vec3()?;
            }
            Ok(CurveInfo { a: 0.0, b: 1.0 })
        }
        _ => {
            if dim != 3 {
                return Err(bad("line block: dimension must be 3"));
            }
            // a line evaluates anywhere: only bound its parameters
            line(rd)?;
            Ok(CurveInfo {
                a: -MAX_COORD / 8.0,
                b: MAX_COORD / 8.0,
            })
        }
    }
}

/// A parameter within [`MAX_REL_DIST`] ranges of the curve's.
fn param(rd: &mut Reader<'_>, c: &CurveInfo) -> Result<f64, FfiError> {
    let t = rd.finite()?;
    let far = MAX_REL_DIST * (c.b - c.a);
    if t < c.a - far || t > c.b + far {
        return Err(bad("parameter too far outside the curve"));
    }
    Ok(t)
}

/// The kind of a curve block about to be read (without consuming it).
fn peek_kind(rd: &Reader<'_>) -> Result<f64, FfiError> {
    rd.input
        .get(rd.at)
        .copied()
        .ok_or_else(|| bad("input too short"))
}

/// An increasing interval `a b` of parameters of `c`.
fn interval(rd: &mut Reader<'_>, c: &CurveInfo) -> Result<(f64, f64), FfiError> {
    let a = param(rd, c)?;
    let b = param(rd, c)?;
    if a >= b {
        return Err(bad("interval not increasing"));
    }
    Ok((a, b))
}

/// `nv V[nv]` with at least `need` vectors (`nv` at most 3, `0` only when `need` is 0).
fn vecs(rd: &mut Reader<'_>, need: usize) -> Result<usize, FfiError> {
    let nv = rd.index(3)?;
    for _ in 0..nv {
        rd.vec3()?;
    }
    if nv < need {
        return Err(bad("operation needs more option vectors"));
    }
    Ok(nv)
}

/// A non-negative finite tolerance.
fn tol(rd: &mut Reader<'_>) -> Result<f64, FfiError> {
    let v = rd.finite()?;
    if v < 0.0 {
        return Err(bad("negative tolerance"));
    }
    Ok(v)
}

/// Option vectors `LocalCurveSolve` dereferences for `sop` (`None`: operation not
/// accepted).
fn curve_solve_vectors(sop: usize) -> Option<usize> {
    match sop {
        // minimize, maximize, normalize, at distance
        3..=6 => Some(0),
        // 3D signed directed minimize, projected min / max
        9 | 14 | 15 => Some(1),
        // signed directed / directed min / max
        11..=13 => Some(2),
        // signed pivot minimize
        23 => Some(3),
        _ => None,
    }
}

fn validate_op(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match op {
        Op::Normal => {
            let n = rd.index(MAX_CURVES)?;
            if n == 0 {
                return Err(bad("no curves"));
            }
            for _ in 0..n {
                curve_block(rd)?;
            }
        }
        Op::FixGaps => {
            let n = rd.index(MAX_CURVES)?;
            for _ in 0..n {
                if peek_kind(rd)? != 0.0 {
                    return Err(bad("FixGaps takes B-splines only"));
                }
                curve_block(rd)?;
            }
            tol(rd)?;
        }
        Op::ArcLength => {
            let kind = peek_kind(rd)?;
            // a line's knots are its domain `[min(d0, d1), max(d0, d1)]`
            let line_min = match (rd.input.get(rd.at + 8), rd.input.get(rd.at + 9)) {
                (Some(&d0), Some(&d1)) if kind == 2.0 => Some(d0.min(d1)),
                _ => None,
            };
            let c = curve_block(rd)?;
            let first_knot = line_min.unwrap_or(c.a);
            let start = param(rd, &c)?;
            let len = rd.finite()?;
            if len < 0.0 && start < first_knot {
                return Err(bad("backwards arc length from below the first knot"));
            }
        }
        Op::PcExtrema => {
            let c = curve_block(rd)?;
            rd.vec3()?;
            interval(rd, &c)?;
            tol(rd)?;
            rd.index(24)?;
            let nt = rd.index(MAX_PARAMS)?;
            for _ in 0..nt {
                param(rd, &c)?;
            }
        }
        Op::LineFit => {
            let c1 = curve_block(rd)?;
            let c2 = curve_block(rd)?;
            param(rd, &c1)?;
            param(rd, &c2)?;
            rd.flag()?;
        }
        Op::PointSolve => {
            let c = curve_block(rd)?;
            interval(rd, &c)?;
            let sop = rd.index(24)?;
            if !matches!(sop, 0 | 3..=6 | 8 | 9) {
                return Err(bad("operation not accepted by LocalPointSolve"));
            }
            rd.vec3()?;
            let has_dt = rd.flag()?;
            tol(rd)?;
            let has_tg = rd.flag()?;
            rd.finite()?;
            if sop == 6 && !has_tg {
                return Err(bad("at distance needs the target distance"));
            }
            // the ray operations NULL-check both before reading them
            let need = usize::from(matches!(sop, 8 | 9) && has_dt);
            vecs(rd, need)?;
            param(rd, &c)?;
        }
        Op::CurveSolve => {
            let c1 = curve_block(rd)?;
            interval(rd, &c1)?;
            let c2 = curve_block(rd)?;
            interval(rd, &c2)?;
            let sop = rd.index(24)?;
            let need = curve_solve_vectors(sop)
                .ok_or_else(|| bad("operation not accepted by LocalCurveSolve"))?;
            tol(rd)?;
            rd.flag()?;
            rd.finite()?;
            vecs(rd, need)?;
            param(rd, &c1)?;
            param(rd, &c2)?;
        }
        Op::Intersect => {
            let c1 = curve_block(rd)?;
            interval(rd, &c1)?;
            let c2 = curve_block(rd)?;
            interval(rd, &c2)?;
            tol(rd)?;
            param(rd, &c1)?;
            param(rd, &c2)?;
        }
        Op::PropertyLocal => {
            if peek_kind(rd)? == 2.0 {
                return Err(bad("the cache takes B-splines and Hermite curves only"));
            }
            let c = curve_block(rd)?;
            interval(rd, &c)?; // cache interval
            rd.flag()?; // full
            tol(rd)?; // chord
            tol(rd)?; // angle
            tol(rd)?; // off axis
            rd.index(MAX_SEGMENTS)?; // min segments
            rd.flag()?; // aux
            tol(rd)?; // max 3d
            let ratio = rd.finite()?;
            if !(MIN_RATIO..=1.0).contains(&ratio) {
                return Err(bad("parametric ratio out of range"));
            }
            interval(rd, &c)?; // solver interval
            rd.index(28)?; // property
                           // the property analysis reads values and vectors without NULL checks
            if !rd.flag()? {
                return Err(bad("property needs a value"));
            }
            rd.finite()?;
            vecs(rd, 3)?;
            rd.index(24)?; // operation
            rd.finite()?; // tolerance
            rd.finite()?; // best answer
            rd.index(1 << 20)?; // leaf
        }
        Op::IsLinear => {
            curve_block(rd)?;
            tol(rd)?;
            rd.flag()?;
        }
    }
    Ok(())
}

/// Checks `input` for `op` (layouts in the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input [`eval`] rejects.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    validate_op(op, &mut rd)?;
    rd.done()
}

/// Runs `op` on `input` (layouts in the module docs); returns the shim's output record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see; [`FfiError::Status`]
/// for a shim failure.
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(1024, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated above: well-formed curve blocks, every index, count
        // and flag the shim passes to upstream, option vectors and values wherever
        // upstream dereferences them); buf is valid for buf.len() writes and the shim
        // honours the capacity; len is valid.
        unsafe {
            sys::sc_curve_curve_p3_eval(
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
