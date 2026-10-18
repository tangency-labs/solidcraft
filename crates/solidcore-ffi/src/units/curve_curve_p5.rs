//! FFI wrappers for unit `curve.curve_p5`.
//!
//! Shim: `harness/cpp/shim/curve_curve_p5.cpp` (`sc_curve_curve_p5_eval`). Upstream:
//! `source/SMLib/src/SmCurve.cpp`, `source/SMLib/inc/SmCurve.h`.
//!
//! A curve block is `0 dim oob n p m cpts(x y z w) knots` (an `SmBSplineCurve` built
//! with `SmBSplineCurve(gw_CURVE*, dim, FALSE)` and `SetOutOfBoundsEnabled(oob)`),
//! `1 dim P1[3] D1[3] P2[3] D2[3]` (an `SmHermiteCurve`), `2 3 P[3] V[3] d0 d1 scale io`
//! (a bounded `SmLine(P, V, [d0, d1], scale, 3, ctx, NULL, io)`), `3 3 C[3] X[3] Y[3] d0 d1
//! rx ry io` (an `SmEllipse` without source NURB) or `4 3 C[3] X[3] Y[3] d0 d1 r io` (an
//! `SmCircle`). Every output starts with the status (`0` = `SM_SUCCESS`, else the
//! `SmStatus`); the rest:
//!
//! | [`Op`]         | upstream                                       | parameters                          | outputs |
//! |----------------|------------------------------------------------|-------------------------------------|---------|
//! | `Cleanup`      | `SmCurve::CrvsCleanup(.., TRUE)`               | `n curve*n tol` (B-splines)         | `removed nc (origin a b P0 Pmid P1)*nc` |
//! | `RemoveDup`    | `SmCurve::RemoveDuplicateCurves`               | `n (present [curve])*n`             | `nc origin*nc` (`-1` for `NULL`) |
//! | `StepPoint`    | `EvaluateSTEPPoint` (virtual)                  | curve `t`                           | `P` |
//! | `Seam`         | `sm_IsPointSolutionAcrossClosedSeamBoundary`   | curve `a b` curve `a b type s0 s1 this_first` | flag |
//! | `Cache`        | `CacheMakeOrValidate`                          | curve `type with_old`               | `reused nspans (a b)*` |
//! | `CpGuess`      | `sm_FindAdditionalCpGuess`                     | curve `a b ABC t`                   | `found guess` |
//! | `IsArc`        | `IsArc` (virtual)                              | curve `n tol`                       | `flag [O X Y r s e]` |
//! | `IsLine`       | `IsLine` (virtual)                             | curve `n tol`                       | `flag [P V]` |
//! | `IsLinear`     | `SmCurve::IsLinear`                            | curve `tol want_dev`                | `flag [P T] dev` |
//! | `Domain`       | `IsDomainBoundary` (virtual)                   | curve `umin vmin umax vmax tol`     | type |
//! | `Periodic`     | `IsPeriodic`                                   | curve `a b`                         | flag |
//! | `Length`       | `Length` (virtual)                             | curve `a b acc`                     | length |
//! | `Lpa`          | `SmCurve::LocalPropertyAnalysis`               | curve `a b prop guess has_val val nv V*nv` | `found [type t value]` |
//! | `Cci`          | `SmCCIntersectENFO::Evaluate`                  | curve curve `x0 x1 jac`             | `f[2] J[4] found size zero` (always) |
//! | `CcTan`        | `SmCCTangentIntENFO::Evaluate`                 | curve curve `x0 x1 jac`             | `f[2] J[4] found` (always) |
//! | `Normal`       | `SmFindCCNormalENFO::Evaluate`, repeated       | curve curve `sqtol sqdist sop nv V*nv k (x0 x1 jac)*k` | `(status f[2] J[4] found)*k` |
//!
//! `with_old` first builds the curve's cache and passes it as the old cache. `this_first`
//! puts the first curve in the solution's first object slot. The `Normal` op keeps one
//! evaluator across its calls (its iteration count and step-back state carry over).
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour or runaway
//! work: well-formed clamped B-splines (`m = n+p+1`, `1 <= p <= n`, `p <= 9`, finite
//! bounded data, positive weights, end multiplicity `p+1`, interior at most `p`) of
//! dimension 2 or 3, finite bounded Hermite data, lines, ellipses and circles with
//! non-zero, non-parallel axes and positive scales / radii, parameters within a few
//! curve lengths of the curve's range, intervals with `a <= b`, B-splines only in
//! `Cleanup` (upstream casts to `SmBSplineCurve`), and option vectors / values present
//! wherever upstream reads them without a `NULL` check.

use crate::validate::{bad, euclid, NOW, NOZ};
use crate::{call_with_buffer, FfiError};

/// Largest control point index `n`.
pub const MAX_INDEX: usize = 512;
/// Largest degree.
pub const MAX_DEGREE: usize = 9;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1e11;
/// Parameters may lie this many interval lengths outside the curve's range.
pub const MAX_REL_DIST: f64 = 4.0;
/// Most curves of `Cleanup` / `RemoveDup`.
pub const MAX_CURVES: usize = 16;
/// Most samples of `IsArc` / `IsLine`.
pub const MAX_SAMPLES: usize = 401;
/// Most evaluations of `Normal`.
pub const MAX_CALLS: usize = 16;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Cleanup,
    RemoveDup,
    StepPoint,
    Seam,
    Cache,
    CpGuess,
    IsArc,
    IsLine,
    IsLinear,
    Domain,
    Periodic,
    Length,
    Lpa,
    Cci,
    CcTan,
    Normal,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Op::Cleanup => 0,
            Op::RemoveDup => 1,
            Op::StepPoint => 2,
            Op::Seam => 3,
            Op::Cache => 4,
            Op::CpGuess => 5,
            Op::IsArc => 6,
            Op::IsLine => 7,
            Op::IsLinear => 8,
            Op::Domain => 9,
            Op::Periodic => 10,
            Op::Length => 11,
            Op::Lpa => 12,
            Op::Cci => 13,
            Op::CcTan => 14,
            Op::Normal => 15,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_curve_curve_p5_eval(
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

/// The parameter range of a validated curve block and its kind.
#[derive(Debug, Clone, Copy)]
struct CurveInfo {
    kind: usize,
    a: f64,
    b: f64,
}

/// An ellipse or circle placement and domain `C X Y d0 d1`.
fn conic_frame(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    rd.vec3()?;
    let x = rd.nonzero3()?;
    let y = rd.nonzero3()?;
    let cross = [
        x[1] * y[2] - x[2] * y[1],
        x[2] * y[0] - x[0] * y[2],
        x[0] * y[1] - x[1] * y[0],
    ];
    if cross.iter().map(|c| c * c).sum::<f64>() < 1e-12 {
        return Err(bad("ellipse: parallel axes"));
    }
    let d0 = rd.finite()?;
    let d1 = rd.finite()?;
    if d0.abs() > 720.0 || d1.abs() > 720.0 {
        return Err(bad("ellipse: domain end beyond 720 degrees"));
    }
    if !(1e-4..=360.0).contains(&(d1 - d0)) {
        return Err(bad("ellipse: domain length outside [1e-4, 360]"));
    }
    Ok(())
}

fn radius(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let r = rd.finite()?;
    if !(1e-6..=1e6).contains(&r) {
        return Err(bad("ellipse: radius outside [1e-6, 1e6]"));
    }
    Ok(())
}

/// A curve block (see the module docs).
fn curve_block(rd: &mut Reader<'_>) -> Result<CurveInfo, FfiError> {
    let kind = rd.index(4)?;
    let dim = rd.index(3)?;
    if dim < 2 {
        return Err(bad("curve block: dimension must be 2 or 3"));
    }
    // analytic curves evaluate anywhere: only bound their parameters
    let wide = CurveInfo {
        kind,
        a: -MAX_COORD / 8.0,
        b: MAX_COORD / 8.0,
    };
    match kind {
        0 => {
            rd.flag()?;
            let (a, b) = bspline(rd)?;
            Ok(CurveInfo { kind, a, b })
        }
        1 => {
            for _ in 0..4 {
                rd.vec3()?;
            }
            Ok(CurveInfo {
                kind,
                a: 0.0,
                b: 1.0,
            })
        }
        2 => {
            if dim != 3 {
                return Err(bad("line block: dimension must be 3"));
            }
            line(rd)?;
            Ok(wide)
        }
        _ => {
            if dim != 3 {
                return Err(bad("ellipse block: dimension must be 3"));
            }
            conic_frame(rd)?;
            radius(rd)?;
            if kind == 3 {
                radius(rd)?;
            }
            rd.flag()?;
            Ok(CurveInfo {
                kind,
                a: -1e3,
                b: 1e3,
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

/// An interval `a b` of parameters with `a <= b`.
fn interval(rd: &mut Reader<'_>, c: &CurveInfo) -> Result<(), FfiError> {
    let a = param(rd, c)?;
    let b = param(rd, c)?;
    if a > b {
        return Err(bad("interval with a > b"));
    }
    Ok(())
}

/// Option vectors a property reads (when the vector array is not `NULL`).
fn property_vectors(prop: usize) -> usize {
    match prop {
        // perpendicular, parallel, silhouette vector / point, plane intersection,
        // distance to point, point inversion
        0 | 4 | 5 | 6 | 16 | 23 | 25 => 1,
        // angles: LocalPropertyAnalysis reads a third vector
        17..=19 => 3,
        // projected point, cylinder, tangent to circle, maxima, tangent through
        20..=22 | 24 | 26..=28 => 2,
        _ => 0,
    }
}

fn validate_op(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match op {
        Op::Cleanup => {
            let n = rd.index(MAX_CURVES)?;
            for _ in 0..n {
                if curve_block(rd)?.kind != 0 {
                    return Err(bad("cleanup: B-spline curves only"));
                }
            }
            if rd.finite()? < 0.0 {
                return Err(bad("negative tolerance"));
            }
        }
        Op::RemoveDup => {
            let n = rd.index(MAX_CURVES)?;
            for _ in 0..n {
                if rd.flag()? {
                    curve_block(rd)?;
                }
            }
        }
        Op::StepPoint => {
            let c = curve_block(rd)?;
            param(rd, &c)?;
        }
        Op::Seam => {
            let c1 = curve_block(rd)?;
            interval(rd, &c1)?;
            let c2 = curve_block(rd)?;
            interval(rd, &c2)?;
            rd.index(5)?;
            param(rd, &c1)?;
            param(rd, &c2)?;
            rd.flag()?;
        }
        Op::Cache => {
            curve_block(rd)?;
            rd.index(3)?;
            rd.flag()?;
        }
        Op::CpGuess => {
            let c = curve_block(rd)?;
            interval(rd, &c)?;
            rd.vec3()?;
            param(rd, &c)?;
        }
        Op::IsArc | Op::IsLine => {
            curve_block(rd)?;
            rd.index(MAX_SAMPLES)?;
            if rd.finite()? < 0.0 {
                return Err(bad("negative tolerance"));
            }
        }
        Op::IsLinear => {
            curve_block(rd)?;
            if rd.finite()? < 0.0 {
                return Err(bad("negative tolerance"));
            }
            rd.flag()?;
        }
        Op::Domain => {
            curve_block(rd)?;
            for _ in 0..4 {
                rd.finite()?;
            }
            rd.finite()?;
        }
        Op::Periodic => {
            let c = curve_block(rd)?;
            interval(rd, &c)?;
        }
        Op::Length => {
            let c = curve_block(rd)?;
            interval(rd, &c)?;
            if rd.finite()? <= 0.0 {
                return Err(bad("accuracy must be positive"));
            }
        }
        Op::Lpa => {
            let c = curve_block(rd)?;
            interval(rd, &c)?;
            let prop = rd.index(28)?;
            param(rd, &c)?;
            let has_val = rd.flag()?;
            rd.finite()?;
            let nv = rd.index(3)?;
            for _ in 0..nv {
                rd.vec3()?;
            }
            let need = property_vectors(prop);
            // projected point and the angles read their vectors without a NULL check
            if (nv > 0 || matches!(prop, 17..=21)) && nv < need {
                return Err(bad("property needs more option vectors"));
            }
            // values read without a NULL check (distance, cylinder radius)
            if matches!(prop, 22 | 23) && nv > 0 && !has_val {
                return Err(bad("property needs a value"));
            }
        }
        Op::Cci | Op::CcTan => {
            let c1 = curve_block(rd)?;
            let c2 = curve_block(rd)?;
            param(rd, &c1)?;
            param(rd, &c2)?;
            rd.flag()?;
        }
        Op::Normal => {
            let c1 = curve_block(rd)?;
            let c2 = curve_block(rd)?;
            rd.finite()?;
            rd.finite()?;
            let sop = rd.index(24)?;
            let nv = rd.index(3)?;
            for _ in 0..nv {
                rd.vec3()?;
            }
            // directed operations read two vectors once given; signed pivot always
            if (matches!(sop, 11..=13) && nv > 0 && nv < 2) || (sop == 23 && nv < 2) {
                return Err(bad("operation needs more option vectors"));
            }
            let k = rd.index(MAX_CALLS)?;
            for _ in 0..k {
                param(rd, &c1)?;
                param(rd, &c2)?;
                rd.flag()?;
            }
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
        // and flag the shim passes to upstream, B-splines where upstream casts to
        // SmBSplineCurve, option vectors wherever upstream dereferences them); buf is
        // valid for buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_curve_curve_p5_eval(
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
