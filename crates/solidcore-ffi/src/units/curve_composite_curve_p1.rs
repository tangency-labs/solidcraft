//! FFI wrappers for unit `curve.composite_curve_p1`.
//!
//! Shim: `harness/cpp/shim/curve_composite_curve_p1.cpp` (`sc_curve_composite_curve_p1_eval`).
//! Upstream: `source/SMLib/src/SmCompositeCurve.cpp`, `source/SMLib/inc/SmCompositeCurve.h`.
//!
//! A curve block is `0 dim oob n p m cpts(x y z w) knots` (an `SmBSplineCurve`), `1 dim
//! P1[3] D1[3] P2[3] D2[3]` (an `SmHermiteCurve`) or `2 3 P[3] V[3] d0 d1 scale io` (a
//! bounded `SmLine`), as in unit `curve.curve_p6`. A composite block is `dim n curve*n
//! closed has_senses sense*n has_gaps gap*n` (`SmCompositeCurve(dim, curves, closed,
//! senses?, gaps?)`). A curve record `curve` is `kind a b P[3]*7 [n+1 Pw[4]*(n+1) m+1
//! U*(m+1)]` (kind `0` B-spline with its NURB, `2` line, `1` other; the natural interval
//! and the points at 7 even parameters). A segment list `segs` is `n closed (transition
//! sense gap_start gap_end owns)*n`. Every output starts with the status (`0` =
//! `SM_SUCCESS`, else the `SmStatus`); the rest (on success):
//!
//! | [`Op`]      | upstream                                    | parameters                                 | outputs |
//! |-------------|---------------------------------------------|--------------------------------------------|---------|
//! | `Info`      | constructor, queries                        | composite `nt t*nt`                         | `segs a b dim degree nknots ncps analytic bounded repeated type eval_status seg_null canon_status cdim cclosed ncurves sense*ncurves kind*nt` |
//! | `Knots`     | `GetKnots`                                  | composite                                   | `nk k*nk nm m*nm` |
//! | `BBox`      | `CalculateBoundingBox`                      | composite `a b pseudo polar expand`         | `min[3] max[3] [pseudo] [polar]` |
//! | `Eq`        | `operator==`                                | composite composite                         | `a==b b==a a==a seg0==seg0` (`-1` without segments) |
//! | `Copy`      | `Copy`                                      | composite                                   | `copy==orig type segs curve*n` |
//! | `Transform` | `Transform`                                 | composite `O[3] X[3] Y[3] has_scale S[3]`   | `curve*n` |
//! | `Approx`    | `ApproximateCurve`                          | composite `tol`                             | `gap made [curve]` |
//! | `Orient`    | `ComputeProjectedLoopOrientation`           | `n curve*n sense*n N[3] want_bounds`        | `orient [umin vmin umax vmax]` |
//! | `Build`     | `BuildCompositesFromCurves`                 | `n curve*n same_tol line_dist`              | `nc (dim segs curve*nseg)*nc unused` |
//! | `Corner`    | `sm_CreateCorner` (shim copy)               | `V[3] P1[3] V1[3] P2[3] V2[3] type dist tol` | `(made [curve])*3` |
//! | `Classify`  | `sm_CurveClassify` (shim copy)              | `curve dist tol n curve*n has_orig [curve]` | `keep` |
//! | `Breaks`    | `sm_TrimFirstBreak` / `sm_TrimLastBreak` / `sm_InsertSortedBreaks` (shim copies) | `which n v*n x` | `n v*n` (always) |
//! | `Types`     | `SmOffsetMapAttribute`, `SmCompositeCurveRegion`, `SmCompositeCurveSegment` | `id corner comp curve other err nt t*nt` | `type corner curve other orig_null id region_type seg_type (attr region seg)*nt` |
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour or runaway
//! work: well-formed clamped B-splines, finite bounded Hermite data and lines (as in unit
//! `curve.curve_p6`), at most [`MAX_SEGMENTS`] curves; `Knots` of at most one segment
//! (more read past the knot array); finite increasing `BBox` intervals (the segments'
//! boxes clamp the shifted intervals to their ranges); `Copy` without Hermite segments (their copy fails, leaving a `NULL`
//! segment); `Approx` of 3D B-spline and non-degenerate Hermite segments whose ends
//! connect in order (else the join fails and upstream dereferences `NULL`); `Build` of
//! B-splines and lines only (`SmBSplineCurve*`); gaps for a closed composite only with
//! curves.

use crate::{call_with_buffer, FfiError};

/// Largest control point index `n`.
pub const MAX_INDEX: usize = 64;
/// Largest degree.
pub const MAX_DEGREE: usize = 9;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1e6;
/// Largest number of curves in a composite or curve list.
pub const MAX_SEGMENTS: usize = 8;
/// Largest type code asked about.
pub const MAX_TYPE: usize = 100_000;

const NOZ: f64 = f64::MAX;
const NOW: f64 = -f64::MAX;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// Constructor and queries.
    Info,
    /// `GetKnots`.
    Knots,
    /// `CalculateBoundingBox`.
    BBox,
    /// `operator==`.
    Eq,
    /// `Copy`.
    Copy,
    /// `Transform`.
    Transform,
    /// `ApproximateCurve`.
    Approx,
    /// `ComputeProjectedLoopOrientation`.
    Orient,
    /// `BuildCompositesFromCurves`.
    Build,
    /// `sm_CreateCorner`.
    Corner,
    /// `sm_CurveClassify`.
    Classify,
    /// The sorted break helpers.
    Breaks,
    /// Type queries of the helper classes.
    Types,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Op::Info => 0,
            Op::Knots => 1,
            Op::BBox => 2,
            Op::Eq => 3,
            Op::Copy => 4,
            Op::Transform => 5,
            Op::Approx => 6,
            Op::Orient => 7,
            Op::Build => 8,
            Op::Corner => 9,
            Op::Classify => 10,
            Op::Breaks => 11,
            Op::Types => 12,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_curve_composite_curve_p1_eval(
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

impl Reader<'_> {
    fn nonzero3(&mut self) -> Result<[f64; 3], FfiError> {
        let v = self.vec3()?;
        if len_sq(v) < 1e-12 {
            return Err(bad("zero vector"));
        }
        Ok(v)
    }
}

fn len_sq(v: [f64; 3]) -> f64 {
    v.iter().map(|c| c * c).sum::<f64>()
}

/// What validation learns about a curve block.
#[derive(Debug, Clone, Copy)]
struct CurveInfo {
    /// `0` B-spline, `1` Hermite, `2` line.
    kind: usize,
    dim: usize,
    /// Euclidean end points (B-spline end control points, Hermite `P1`/`P2`; unknown
    /// for a line).
    start: [f64; 3],
    end: [f64; 3],
}

fn euclid([x, y, z, w]: [f64; 4]) -> [f64; 3] {
    let z = if z == NOZ { 0.0 } else { z };
    if w == NOW {
        [x, y, z]
    } else {
        [x / w, y / w, z / w]
    }
}

/// A B-spline `n p m cpts knots`: its range and end points.
fn bspline(rd: &mut Reader<'_>) -> Result<(f64, f64, [f64; 3], [f64; 3]), FfiError> {
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
    let first = cpts.first().copied().ok_or_else(|| bad("empty curve"))?;
    let last = cpts.last().copied().ok_or_else(|| bad("empty curve"))?;
    Ok((a, b, euclid(first), euclid(last)))
}

/// A line `P V d0 d1 scale io`.
fn line(rd: &mut Reader<'_>) -> Result<(), FfiError> {
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
    Ok(())
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
            let (_, _, start, end) = bspline(rd)?;
            Ok(CurveInfo {
                kind,
                dim,
                start,
                end,
            })
        }
        1 => {
            let p1 = rd.vec3()?;
            rd.vec3()?;
            let p2 = rd.vec3()?;
            rd.vec3()?;
            Ok(CurveInfo {
                kind,
                dim,
                start: p1,
                end: p2,
            })
        }
        _ => {
            if dim != 3 {
                return Err(bad("line block: dimension must be 3"));
            }
            line(rd)?;
            Ok(CurveInfo {
                kind,
                dim,
                start: [f64::NAN; 3],
                end: [f64::NAN; 3],
            })
        }
    }
}

/// A composite block (see the module docs): its curves.
fn composite_block(rd: &mut Reader<'_>) -> Result<Vec<CurveInfo>, FfiError> {
    let dim = rd.index(3)?;
    if dim < 2 {
        return Err(bad("composite: dimension must be 2 or 3"));
    }
    let n = rd.index(MAX_SEGMENTS)?;
    let mut curves = Vec::with_capacity(n);
    for _ in 0..n {
        curves.push(curve_block(rd)?);
    }
    let closed = rd.flag()?;
    if rd.flag()? {
        for _ in 0..n {
            rd.flag()?;
        }
    }
    if rd.flag()? {
        if closed && n == 0 {
            return Err(bad("composite: gaps of a closed composite without curves"));
        }
        for _ in 0..n {
            rd.finite()?;
        }
    }
    Ok(curves)
}

/// A positive finite tolerance.
fn tol(rd: &mut Reader<'_>) -> Result<f64, FfiError> {
    let v = rd.finite()?;
    if v <= 0.0 {
        return Err(bad("tolerance not positive"));
    }
    Ok(v)
}

fn type_list(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let nt = rd.index(32)?;
    for _ in 0..nt {
        rd.index(MAX_TYPE)?;
    }
    Ok(())
}

/// `Approx`: 3D B-spline / non-degenerate Hermite segments connected in order.
fn check_approx(curves: &[CurveInfo]) -> Result<(), FfiError> {
    if curves.is_empty() {
        return Err(bad("approx: no segments (the join fails)"));
    }
    for c in curves {
        if c.kind == 2 || c.dim != 3 {
            return Err(bad("approx: only 3D B-spline and Hermite segments"));
        }
        let d = [
            c.end[0] - c.start[0],
            c.end[1] - c.start[1],
            c.end[2] - c.start[2],
        ];
        if c.kind == 1 && len_sq(d) < 1e-6 {
            return Err(bad("approx: degenerate Hermite segment"));
        }
    }
    for w in curves.windows(2) {
        let (Some(p), Some(q)) = (w.first(), w.last()) else {
            continue;
        };
        let size = p
            .end
            .iter()
            .chain(&q.start)
            .fold(1.0f64, |m, v| m.max(v.abs()));
        let d = [
            q.start[0] - p.end[0],
            q.start[1] - p.end[1],
            q.start[2] - p.end[2],
        ];
        if len_sq(d).sqrt() > 1e-9 * size {
            return Err(bad("approx: segments not connected (the join fails)"));
        }
    }
    Ok(())
}

fn validate_op(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match op {
        Op::Info => {
            composite_block(rd)?;
            type_list(rd)?;
        }
        Op::Knots => {
            if composite_block(rd)?.len() > 1 {
                return Err(bad(
                    "knots: more than one segment reads past the knot array",
                ));
            }
        }
        Op::BBox => {
            composite_block(rd)?;
            let a = rd.finite()?;
            let b = rd.finite()?;
            if a > b {
                return Err(bad("bbox: interval not increasing"));
            }
            rd.flag()?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::Eq => {
            composite_block(rd)?;
            composite_block(rd)?;
        }
        Op::Copy => {
            if composite_block(rd)?.iter().any(|c| c.kind == 1) {
                return Err(bad("copy: a Hermite segment cannot be copied"));
            }
        }
        Op::Transform => {
            composite_block(rd)?;
            rd.vec3()?;
            let x = rd.nonzero3()?;
            let y = rd.nonzero3()?;
            let cross = [
                x[1] * y[2] - x[2] * y[1],
                x[2] * y[0] - x[0] * y[2],
                x[0] * y[1] - x[1] * y[0],
            ];
            if len_sq(cross) < 1e-6 * len_sq(x) * len_sq(y) {
                return Err(bad("transform: parallel axes"));
            }
            rd.flag()?;
            let s = rd.vec3()?;
            if s.iter().any(|c| c.abs() < 0.1 || c.abs() > 10.0) {
                return Err(bad("transform: scale out of range"));
            }
        }
        Op::Approx => {
            let curves = composite_block(rd)?;
            check_approx(&curves)?;
            let t = tol(rd)?;
            if !(1e-6..=1.0).contains(&t) {
                return Err(bad("approx: tolerance out of range"));
            }
        }
        Op::Orient => {
            let n = rd.index(MAX_SEGMENTS)?;
            if n == 0 {
                return Err(bad("orient: no curves (empty polygon)"));
            }
            for _ in 0..n {
                curve_block(rd)?;
            }
            for _ in 0..n {
                rd.flag()?;
            }
            rd.nonzero3()?;
            rd.flag()?;
        }
        Op::Build => {
            let n = rd.index(MAX_SEGMENTS)?;
            for _ in 0..n {
                if curve_block(rd)?.kind == 1 {
                    return Err(bad("build: curves must be SmBSplineCurves"));
                }
            }
            let same = tol(rd)?;
            let line = rd.finite()?;
            if !(line == 0.0 || line > same) {
                return Err(bad("build: line distance"));
            }
        }
        Op::Corner => {
            for _ in 0..5 {
                rd.vec3()?;
            }
            rd.index(2)?;
            rd.finite()?;
            tol(rd)?;
        }
        Op::Classify => {
            curve_block(rd)?;
            tol(rd)?;
            tol(rd)?;
            let n = rd.index(4)?;
            for _ in 0..n {
                curve_block(rd)?;
            }
            if rd.flag()? {
                curve_block(rd)?;
            }
        }
        Op::Breaks => {
            rd.index(2)?;
            let n = rd.index(64)?;
            for _ in 0..n {
                rd.finite()?;
            }
            rd.finite()?;
        }
        Op::Types => {
            rd.index(MAX_TYPE)?;
            rd.flag()?;
            rd.index(MAX_TYPE)?;
            rd.index(MAX_TYPE)?;
            rd.index(MAX_TYPE)?;
            rd.finite()?;
            type_list(rd)?;
        }
    }
    Ok(())
}

/// Checks that `input` is a well-formed record for `op` (see the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] describing the first problem.
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
        // reads for `op` (validated above: well-formed curve and composite blocks, every
        // index, count and flag the shim passes to upstream); buf is valid for buf.len()
        // writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_curve_composite_curve_p1_eval(
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
