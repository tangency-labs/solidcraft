//! FFI wrappers for unit `curve.curve_p6`.
//!
//! Shim: `harness/cpp/shim/curve_curve_p6.cpp` (`sc_curve_curve_p6_eval`). Upstream:
//! `source/SMLib/src/SmCurve.cpp`, `source/SMLib/inc/SmCurve.h`.
//!
//! A curve block is `0 dim oob n p m cpts(x y z w) knots` (an `SmBSplineCurve` built with
//! `SmBSplineCurve(gw_CURVE*, dim, FALSE)` and `SetOutOfBoundsEnabled(oob)`), `1 dim P1[3]
//! D1[3] P2[3] D2[3]` (an `SmHermiteCurve`) or `2 3 P[3] V[3] d0 d1 scale io` (a bounded
//! `SmLine(P, V, [d0, d1], scale, 3, ctx, NULL, io)`), as in unit `curve.curve_p3`. Every
//! output starts with the status (`0` = `SM_SUCCESS`, else the `SmStatus`); the rest (on
//! success):
//!
//! | [`Op`]      | upstream                    | parameters                                   | outputs |
//! |-------------|-----------------------------|----------------------------------------------|---------|
//! | `BBox`      | `CalculateBoundingBox`      | curve `a b normal pseudo polar expand`        | `[min[3] max[3]] [basis[9] ivl[6]] [state basis[9] dom[4]]` |
//! | `IsPlanar`  | `IsPlanar`                  | curve `tol`                                  | `planar P[3] N[3] dev` |
//! | `DropPoint` | `DropPoint`                 | curve `a b P[3] has_v V[3] tol has_g g sop`  | `ok param dist` |
//! | `GpsStep`   | `GlobalPointSolveSTEP`      | curve `a b sop P[3] tol has_tg tg req`       | `n sol*n` |
//! | `MaxDist`   | `CurveMaxDistanceBetween`   | curve `a b same [curve] s e nsamp has_max max` | `dist t1 t2 has_dir dir` |
//! | `Simple`    | `SimpleCoincidenceChecker`  | curve `a b` curve `a2 b2 tol pb1 pb2`        | `n sol*n` |
//! | `Global`    | `GlobalCoincidenceChecker`  | curve `a b` curve `a2 b2 tol snap`           | `more n sol*n` |
//! | `Deform`    | `DeformToEndPointTargets`   | curve `has_s S[3] has_e E[3] want has_st St[3] st_type has_et Et[3] et_type` | `has_new` state |
//!
//! A solution `sol` is `type nvar nobj o0 o1 value s0 s1 [evalue e0 e1]` (the end for a
//! range; `s1`/`e1` are `0` for a one-variable solution); objects are `0` (this curve),
//! `1` (the other), `2` (another object), `-1` (`NULL`). `same` passes this curve itself as
//! the other one (no second block). `pb1` / `pb2` give `SimpleCoincidenceChecker` the
//! pseudo box from `CalculateBoundingBox` of the respective curve over its interval (a
//! failed box is the op status). `has_dir` is `0` when `pbOptIncorrectDir` was left
//! untouched. The curve `state` after `Deform` (the new curve when there is one, else
//! the edited curve) is `kind a b P[3]*7 [n+1 Pw[4(n+1)] m+1 U[m+1]]`: kind `0`
//! (`SmBSplineCurve_TYPE`, with its NURB), `2` (line) or `1` (other), its natural interval
//! and 7 evenly spaced points.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour or runaway
//! work: well-formed clamped B-splines (`m = n+p+1`, `1 <= p <= n`, `p <= 9`, finite
//! bounded data, positive weights, end multiplicity `p+1`, interior at most `p`) of
//! dimension 2 or 3, finite bounded Hermite data, lines with non-zero vectors and
//! positive scales, parameters within a few curve lengths of the curve's range,
//! increasing intervals, positive tolerances, `DropPoint` operations below 8,
//! `GlobalPointSolveSTEP` operations intersect / minimize / maximize / normalize,
//! `CurveMaxDistanceBetween` sample counts other than `1` (upstream divides by
//! `nsamp - 1`) and at most [`MAX_SAMPLES`], tangent types `0..=2`.

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
/// Largest sample count of `CurveMaxDistanceBetween`.
pub const MAX_SAMPLES: usize = 64;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    BBox,
    IsPlanar,
    DropPoint,
    GpsStep,
    MaxDist,
    Simple,
    Global,
    Deform,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Op::BBox => 0,
            Op::IsPlanar => 1,
            Op::DropPoint => 2,
            Op::GpsStep => 3,
            Op::MaxDist => 4,
            Op::Simple => 5,
            Op::Global => 6,
            Op::Deform => 7,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_curve_curve_p6_eval(
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

/// An increasing interval `a b` of parameters of `c`.
fn interval(rd: &mut Reader<'_>, c: &CurveInfo) -> Result<(f64, f64), FfiError> {
    let a = param(rd, c)?;
    let b = param(rd, c)?;
    if a >= b {
        return Err(bad("interval not increasing"));
    }
    Ok((a, b))
}

/// A positive finite tolerance.
fn tol(rd: &mut Reader<'_>) -> Result<f64, FfiError> {
    let v = rd.finite()?;
    if v <= 0.0 {
        return Err(bad("tolerance not positive"));
    }
    Ok(v)
}

fn validate_op(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match op {
        Op::BBox => {
            let c = curve_block(rd)?;
            interval(rd, &c)?;
            for _ in 0..4 {
                rd.flag()?;
            }
        }
        Op::IsPlanar => {
            curve_block(rd)?;
            tol(rd)?;
        }
        Op::DropPoint => {
            let c = curve_block(rd)?;
            interval(rd, &c)?;
            rd.vec3()?;
            rd.flag()?;
            rd.vec3()?;
            tol(rd)?;
            rd.flag()?;
            param(rd, &c)?;
            rd.index(7)?;
        }
        Op::GpsStep => {
            let c = curve_block(rd)?;
            interval(rd, &c)?;
            let sop = rd.index(5)?;
            if !matches!(sop, 0 | 3..=5) {
                return Err(bad("operation not accepted"));
            }
            rd.vec3()?;
            tol(rd)?;
            rd.flag()?;
            rd.finite()?;
            rd.index(1)?;
        }
        Op::MaxDist => {
            let c = curve_block(rd)?;
            interval(rd, &c)?;
            let same = rd.flag()?;
            let c2 = if same { c } else { curve_block(rd)? };
            param(rd, &c2)?;
            param(rd, &c2)?;
            let n = rd.index(MAX_SAMPLES)?;
            if n == 1 {
                return Err(bad("one sample divides by zero upstream"));
            }
            rd.flag()?;
            tol(rd)?;
        }
        Op::Simple => {
            let c = curve_block(rd)?;
            interval(rd, &c)?;
            let c2 = curve_block(rd)?;
            interval(rd, &c2)?;
            tol(rd)?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::Global => {
            let c = curve_block(rd)?;
            interval(rd, &c)?;
            let c2 = curve_block(rd)?;
            interval(rd, &c2)?;
            tol(rd)?;
            rd.flag()?;
        }
        Op::Deform => {
            curve_block(rd)?;
            for _ in 0..2 {
                rd.flag()?;
                rd.vec3()?;
            }
            rd.flag()?;
            for _ in 0..2 {
                rd.flag()?;
                rd.vec3()?;
                rd.index(2)?;
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
        // and flag the shim passes to upstream); buf is valid for buf.len() writes and the
        // shim honours the capacity; len is valid.
        unsafe {
            sys::sc_curve_curve_p6_eval(
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
