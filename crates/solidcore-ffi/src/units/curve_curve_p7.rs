//! FFI wrappers for unit `curve.curve_p7`.
//!
//! Shim: `harness/cpp/shim/curve_curve_p7.cpp` (`sc_curve_curve_p7_eval`). Upstream:
//! `source/SMLib/src/SmCurve.cpp` (`SmCCIGlobalSolver::LocalSolve`, verbatim in the shim,
//! and `SmCurve::GlobalPropertyAnalysis`).
//!
//! A curve block is `0 dim oob n p m cpts(x y z w) knots` (an `SmBSplineCurve`) or
//! `1 dim P1[3] D1[3] P2[3] D2[3]` (an `SmHermiteCurve`); lines (`2 ...`) are rejected (the
//! solvers decompose curves through the curve cache). `vecs` is `nv V[nv]` (`NULL` for
//! `nv = 0`). A `cache` is `0` (the curve's object cache, `SmCacheMgr::GetOrCreateObjectCache`)
//! or `1 a b full chord angle offaxis minseg aux max3d ratio` (an `SmCurveCache` built and
//! tessellated by the shim; not `aux` without `full`, whose pruned leaves hold parent data
//! that `LocalSolve` would read as a span); each pushes its status first.
//!
//! | [`Op`]     | upstream                                        | parameters | outputs |
//! |------------|-------------------------------------------------|------------|---------|
//! | `CciSolve` | `SmCCIGlobalSolver::SolveIt`                     | `c1 self [c2] cache1 [cache2] a1 b1 a2 b2 sop tol proj ptype` vecs | `[status nsol sol*]` |
//! | `CciLocal` | `SmCCIGlobalSolver::LocalSolve` on one leaf pair | as `CciSolve`, then `leaf1 leaf2` | `nleaf1 nleaf2 [status [needs nsol sol*]]` |
//! | `Property` | `SmCurve::GlobalPropertyAnalysis`                | curve `a b prop has_val val` vecs `tol comb` | `status [nsol sol* npt P[3]* nvec V[3]*]` |
//!
//! A solution `sol` is `type nvar nobj o0 o1 n0 n1 sval s[P] eval e[P]` with
//! `P = max(nvar, 2)`; objects are `0` (first curve), `1` (second), `2` (another), `-1`
//! (`NULL`); nodes are pre-order indices in the first / second tree (`-1` = `NULL`). The
//! `CciLocal` leaves are the span nodes in pre-order, taken modulo their count.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: well-formed
//! clamped B-splines and finite Hermite data (as unit `curve.curve_p3`), increasing
//! intervals within the curves' reach, positive tolerances, the operations
//! `SM_SO_INTERSECT`, `SM_SO_INTERSECTION_TEST` (no projection) and the projected
//! intersections (`SM_SO_PROJECTED_INTERSECT` with a parallel projection and one unit
//! vector, `SM_SO_ROTATED_PROJECTED_INTERSECT` with a rotation and three vectors, the
//! second and third unit, `SM_SO_PERSPECTIVE_INTERSECT` with a perspective and three
//! vectors, the second unit), and for `Property` a value and three vectors (the solvers
//! read them without `NULL` checks).

use crate::{call_with_buffer, FfiError};

/// Largest control point index `n`.
pub const MAX_INDEX: usize = 128;
/// Largest degree.
pub const MAX_DEGREE: usize = 9;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1e6;
/// Parameters may lie this many interval lengths outside the curve's range.
pub const MAX_REL_DIST: f64 = 2.0;
/// Largest `lMinimumNumberOfSegments` of the cache.
pub const MAX_SEGMENTS: usize = 64;
/// Smallest `dMinimumParametricRatio` of the cache.
pub const MIN_RATIO: f64 = 1e-4;

const NOZ: f64 = f64::MAX;
const NOW: f64 = -f64::MAX;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    CciSolve,
    CciLocal,
    Property,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Op::CciSolve => 0,
            Op::CciLocal => 1,
            Op::Property => 2,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_curve_curve_p7_eval(
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

    /// A finite value of magnitude at most [`MAX_COORD`].
    fn finite(&mut self) -> Result<f64, FfiError> {
        let v = self.num()?;
        if !v.is_finite() || v.abs() > MAX_COORD {
            return Err(bad(format!("value {v} not finite or too large")));
        }
        Ok(v)
    }

    fn vec3(&mut self) -> Result<[f64; 3], FfiError> {
        Ok([self.finite()?, self.finite()?, self.finite()?])
    }

    fn nonzero3(&mut self) -> Result<[f64; 3], FfiError> {
        let v = self.vec3()?;
        if v.iter().map(|c| c * c).sum::<f64>() < 1e-12 {
            return Err(bad("zero vector"));
        }
        Ok(v)
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

/// A curve block the cache accepts (no lines).
fn cached_curve(rd: &mut Reader<'_>) -> Result<CurveInfo, FfiError> {
    if peek_kind(rd)? == 2.0 {
        return Err(bad("the solvers take B-splines and Hermite curves only"));
    }
    curve_block(rd)
}

/// A cache block (see the module docs).
fn cache(rd: &mut Reader<'_>, c: &CurveInfo) -> Result<(), FfiError> {
    if !rd.flag()? {
        return Ok(());
    }
    interval(rd, c)?; // cache interval
    let full = rd.flag()?;
    tol(rd)?; // chord
    tol(rd)?; // angle
    tol(rd)?; // off axis
    rd.index(MAX_SEGMENTS)?; // min segments
    let aux = rd.flag()?;
    if aux && !full {
        // parents passing the tests become leaves holding SmBezierAux1d data, which
        // LocalSolve casts to SmBezierSpan (reading past the object)
        return Err(bad(
            "a pruned tree (aux data without full tree) has non-span leaves",
        ));
    }
    tol(rd)?; // max 3d
    let ratio = rd.finite()?;
    if !(MIN_RATIO..=1.0).contains(&ratio) {
        return Err(bad("parametric ratio out of range"));
    }
    Ok(())
}

/// A positive finite tolerance.
fn pos_tol(rd: &mut Reader<'_>) -> Result<f64, FfiError> {
    let v = tol(rd)?;
    if v <= 0.0 {
        return Err(bad("tolerance not positive"));
    }
    Ok(v)
}

/// `nv V[nv]` with exactly `need` vectors, those flagged in `unit` of unit length.
fn proj_vecs(rd: &mut Reader<'_>, need: usize, unit: &[usize]) -> Result<(), FfiError> {
    let nv = rd.index(3)?;
    if nv != need {
        return Err(bad("projection needs a different number of vectors"));
    }
    for i in 0..nv {
        let v = rd.vec3()?;
        let len = v.iter().map(|c| c * c).sum::<f64>().sqrt();
        if unit.contains(&i) && (len - 1.0).abs() > 1e-9 {
            return Err(bad("projection vector not of unit length"));
        }
    }
    Ok(())
}

fn validate_op(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match op {
        Op::CciSolve | Op::CciLocal => {
            let c1 = cached_curve(rd)?;
            let same = rd.flag()?;
            let c2 = if same { c1 } else { cached_curve(rd)? };
            cache(rd, &c1)?;
            if !same {
                cache(rd, &c2)?;
            }
            interval(rd, &c1)?;
            interval(rd, &c2)?;
            let sop = rd.index(24)?;
            pos_tol(rd)?;
            let proj = rd.flag()?;
            let ptype = rd.index(2)?;
            match (sop, proj, ptype) {
                (0 | 2, false, 0) => {
                    if rd.index(3)? != 0 {
                        return Err(bad("plain intersections take no vectors"));
                    }
                }
                // parallel: plane unit normal
                (16, true, 0) => proj_vecs(rd, 1, &[0])?,
                // rotation: axis point, unit axis, unit x axis
                (17, true, 2) => proj_vecs(rd, 3, &[1, 2])?,
                // perspective: plane point, unit normal, eye
                (18, true, 1) => proj_vecs(rd, 3, &[1])?,
                _ => return Err(bad("operation / projection not accepted")),
            }
            if op == Op::CciLocal {
                rd.index(1 << 20)?;
                rd.index(1 << 20)?;
            }
        }
        Op::Property => {
            let c = cached_curve(rd)?;
            interval(rd, &c)?;
            rd.index(28)?; // property
                           // the property solvers read values and vectors without NULL checks
            if !rd.flag()? {
                return Err(bad("property needs a value"));
            }
            rd.finite()?;
            vecs(rd, 3)?;
            pos_tol(rd)?;
            rd.flag()?; // comb
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
    let mut rd = Reader { input, at: 0 };
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
            sys::sc_curve_curve_p7_eval(
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
