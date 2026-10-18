//! FFI wrappers for unit `curve.curve_p1`.
//!
//! Shim: `harness/cpp/shim/curve_curve_p1.cpp` (`sc_curve_curve_p1_eval`). Upstream:
//! `source/SMLib/src/SmCurve.cpp`, `source/SMLib/inc/SmCurve.h`.
//!
//! A curve block is `0 dim oob n p m cpts(x y z w) knots` (an `SmBSplineCurve` built
//! with `SmBSplineCurve(gw_CURVE*, dim, FALSE)` and `SetOutOfBoundsEnabled(oob)`),
//! `1 dim P1[3] D1[3] P2[3] D2[3]` (an `SmHermiteCurve`) or `2 3 P[3] V[3] d0 d1 scale io`
//! (a bounded `SmLine(P, V, [d0, d1], scale, 3, ctx, NULL, io)`). `vecs` is `nv V[nv]`
//! (`NULL` for `nv = 0`). Every output starts with the status (`0` = `SM_SUCCESS`, else
//! the `SmStatus`); the rest:
//!
//! | [`Op`]      | upstream                                 | parameters                                  | outputs |
//! |-------------|------------------------------------------|---------------------------------------------|---------|
//! | `Gps`       | `GlobalPointSolve`                       | curve `a b sop P tol has_tg tg` vecs `req`  | `n sol*n` (on success) |
//! | `Approx`    | `ApproximateCurve`                       | curve `nb breaks[nb] tol analytics match copy` | `written tol` curve (on success) |
//! | `Bisect`    | `TessellateByBisection`                  | curve `a b chord angle max3d`               | `nt np nd t[nt] P[3 np] D[3 nd]` (on success) |
//! | `Tess`      | `Tessellate` (virtual)                   | curve `a b chord angle minseg`              | as `Bisect` |
//! | `Classify`  | `ClassifyIntersectingRangeSolution`      | curve curve `type v0 s0 s1 v1 e0 e1`        | `degenerate written [v p0 p1]` (on success) |
//! | `Span`      | `sm_TestSpanAccuracy` (static)           | curve `lt LP[3] LD[3] ct CP[3] CD[3] tol match` | `pass dist` (on success) |
//! | `Refine`    | `sm_RefineCCSolutions` (static)          | curve curve `n (type nvar o0 o1 v0 s0 s1 v1 e0 e1)*n` | `n (v0 s0 s1 v1 e0 e1)*n` (always) |
//!
//! A `Gps` solution is `type nvar nobj o0 value s0 [evalue e0]` (the end for a range);
//! objects are `0` (the curve), `2` (another object, the cache's analytic curve), `-1`
//! (`NULL`). A result curve is `type dim has_nurb [n p m cpts(x y z w) knots]`. `Refine`
//! objects are stored `+1`: `0` = `NULL`, `1` = the first curve, `2` = the second.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour or runaway
//! work: well-formed clamped B-splines (`m = n+p+1`, `1 <= p <= n`, `p <= 9`, finite
//! bounded data, positive weights, end multiplicity `p+1`, interior at most `p`) of
//! dimension 2 or 3, finite bounded Hermite data, lines with non-zero vectors and
//! positive scales, parameters within a few curve lengths of the curve's range,
//! increasing intervals, the operations `GlobalPointSolve` asserts (with the option
//! vectors they read without a `NULL` check), requests `SM_SR_SINGLE` / `SM_SR_ALL`,
//! at least two non-decreasing breaks for `ApproximateCurve` (tolerance at least
//! [`MIN_TOL`]), tessellation tolerances that are zero or at least [`MIN_CHORD`] /
//! [`MIN_ANGLE`] degrees / [`MIN_DIST`].

use crate::{call_with_buffer, FfiError};

/// Largest control point index `n`.
pub const MAX_INDEX: usize = 128;
/// Largest degree.
pub const MAX_DEGREE: usize = 9;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1e6;
/// Parameters may lie this many interval lengths outside the curve's range.
pub const MAX_REL_DIST: f64 = 2.0;
/// Most breaks of `Approx`.
pub const MAX_BREAKS: usize = 8;
/// Most solutions of `Refine`.
pub const MAX_SOLUTIONS: usize = 8;
/// Largest `lMinimumNumberOfSegments` of `Tess`.
pub const MAX_SEGMENTS: usize = 64;
/// Smallest approximation tolerance of `Approx`.
pub const MIN_TOL: f64 = 1e-9;
/// Smallest non-zero chord height ratio of the tessellations.
pub const MIN_CHORD: f64 = 1e-4;
/// Smallest non-zero angle tolerance (degrees) of the tessellations.
pub const MIN_ANGLE: f64 = 0.5;
/// Smallest non-zero maximum segment length of `Bisect`.
pub const MIN_DIST: f64 = 1e-3;

const NOZ: f64 = f64::MAX;
const NOW: f64 = -f64::MAX;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Gps,
    Approx,
    Bisect,
    Tess,
    Classify,
    Span,
    Refine,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Op::Gps => 0,
            Op::Approx => 1,
            Op::Bisect => 2,
            Op::Tess => 3,
            Op::Classify => 4,
            Op::Span => 5,
            Op::Refine => 6,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_curve_curve_p1_eval(
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

crate::reader::unit_reader!(MAX_COORD);

/// The parameter range of a validated curve block.
#[derive(Debug, Clone, Copy)]
struct CurveInfo {
    a: f64,
    b: f64,
}

fn euclid([x, y, z, w]: [f64; 4]) -> [f64; 3] {
    let z = if z == NOZ { 0.0 } else { z };
    if w == NOW {
        [x, y, z]
    } else {
        [x / w, y / w, z / w]
    }
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

/// Option vectors `GlobalPointSolve` reads without a `NULL` check for `sop` (`None`:
/// operation not accepted).
fn gps_vectors(sop: usize) -> Option<usize> {
    match sop {
        // intersect, minimize, maximize, normalize, at distance
        0 | 3..=6 => Some(0),
        // ray fire, 3D signed directed minimize, projected min / max, tangent through point
        8 | 9 | 14 | 15 | 24 => Some(1),
        // signed directed / directed min / max
        11..=13 => Some(2),
        _ => None,
    }
}

/// A tessellation tolerance: zero or at least `min`.
fn tess_tol(rd: &mut Reader<'_>, min: f64) -> Result<(), FfiError> {
    let v = tol(rd)?;
    if v != 0.0 && v < min {
        return Err(bad("tessellation tolerance too small"));
    }
    Ok(())
}

/// A range solution of two curves: `v0 s0 s1 v1 e0 e1` (values finite, parameters
/// within the curves' ranges).
fn two_curve_ends(rd: &mut Reader<'_>, c1: &CurveInfo, c2: &CurveInfo) -> Result<(), FfiError> {
    for _ in 0..2 {
        rd.finite()?;
        param(rd, c1)?;
        param(rd, c2)?;
    }
    Ok(())
}

fn validate_op(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match op {
        Op::Gps => {
            let c = curve_block(rd)?;
            interval(rd, &c)?;
            let sop = rd.index(24)?;
            let need = gps_vectors(sop)
                .ok_or_else(|| bad("operation not accepted by GlobalPointSolve"))?;
            rd.vec3()?;
            tol(rd)?;
            rd.flag()?;
            rd.finite()?;
            vecs(rd, need)?;
            rd.index(1)?;
        }
        Op::Approx => {
            let c = curve_block(rd)?;
            let nb = rd.index(MAX_BREAKS)?;
            if nb < 2 {
                return Err(bad("ApproximateCurve needs two breaks"));
            }
            let mut last = f64::NEG_INFINITY;
            for _ in 0..nb {
                let t = param(rd, &c)?;
                if t < last {
                    return Err(bad("breaks not non-decreasing"));
                }
                last = t;
            }
            if tol(rd)? < MIN_TOL {
                return Err(bad("approximation tolerance too small"));
            }
            rd.flag()?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::Bisect | Op::Tess => {
            let c = curve_block(rd)?;
            interval(rd, &c)?;
            tess_tol(rd, MIN_CHORD)?;
            tess_tol(rd, MIN_ANGLE)?;
            if op == Op::Bisect {
                tess_tol(rd, MIN_DIST)?;
            } else {
                rd.index(MAX_SEGMENTS)?;
            }
        }
        Op::Classify => {
            let c1 = curve_block(rd)?;
            let c2 = curve_block(rd)?;
            let ty = rd.index(2)?;
            if ty == 0 {
                return Err(bad("solution type must be single or range"));
            }
            two_curve_ends(rd, &c1, &c2)?;
        }
        Op::Span => {
            let c = curve_block(rd)?;
            param(rd, &c)?;
            rd.vec3()?;
            rd.vec3()?;
            param(rd, &c)?;
            rd.vec3()?;
            rd.vec3()?;
            tol(rd)?;
            rd.flag()?;
        }
        Op::Refine => {
            let c1 = curve_block(rd)?;
            let c2 = curve_block(rd)?;
            let n = rd.index(MAX_SOLUTIONS)?;
            for _ in 0..n {
                let ty = rd.index(2)?;
                if ty == 0 {
                    return Err(bad("solution type must be single or range"));
                }
                rd.index(3)?; // variables
                rd.index(2)?; // object 0 (+1)
                rd.index(2)?; // object 1 (+1)
                two_curve_ends(rd, &c1, &c2)?;
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
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated above: well-formed curve blocks, every index, count
        // and flag the shim passes to upstream, option vectors wherever upstream
        // dereferences them); buf is valid for buf.len() writes and the shim honours the
        // capacity; len is valid.
        unsafe {
            sys::sc_curve_curve_p1_eval(
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
