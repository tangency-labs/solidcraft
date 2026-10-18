//! FFI wrappers for unit `curve.curve_cache`.
//!
//! Shim: `harness/cpp/shim/curve_curve_cache.cpp` (`sc_curve_curve_cache_eval`). Upstream:
//! `source/SMLib/src/SmCurveCache.cpp`, `source/SMLib/inc/SmCurveCache.h`.
//!
//! Input (every op): a curve block, `0 dim n p m cpts(x y z w) knots` (an
//! `SmBSplineCurve(gw_CURVE*, dim, FALSE)`) or `1 dim P1[3] D1[3] P2[3] D2[3]` (an
//! `SmHermiteCurve`); then the cache interval `a b` and the `SmCurveCache` constructor
//! arguments `full chord angle offaxis minseg aux max3d ratio`. Output: the status (`0` =
//! `SM_SUCCESS`, else the `SmStatus`) and, on success:
//!
//! | [`Op`]       | upstream                          | outputs |
//! |--------------|-----------------------------------|---------|
//! | `Tessellate` | `Tessellate`, `GetSpanCount`, `GetAt`, `GetParentAt`, `GetContinuitiesArray`, `GetTree`, `SmBezierAux1d::GetEnds`, `SmBezierSpan::GetPseudoBox` | `ns` spans `aux pbox`; `nc` continuities; `nn` nodes (pre-order from the top) `aux geom parent c1 c2 min[3] max[3] dkind [didx [aux]]` |
//! | `Analytic`   | `GetAnalyticCurve`                | `0` none, `1` the curve itself, `2` a new curve |
//! | `Type`       | `GetType`, `IsKindOf`             | `type is_cache is_object is_curve` |
//!
//! `aux` is `left right c0 c1 len chsq turn ivl_min ivl_max start[3] end[3]`; `pbox` the
//! three basis vectors and three intervals; node indices are pre-order positions (`-1`
//! none); `dkind` is `0` no data, `1` leaf span `didx`, `2` parent data `didx` followed by
//! its `aux`, `3` other.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour or runaway
//! work: well-formed clamped B-splines (`m = n+p+1`, `1 <= p <= n`, `p <= 9`, finite
//! bounded data, positive weights, end multiplicity `p+1`, interior at most `p`) of
//! dimension 2 or 3, finite bounded Hermite data, a cache interval within a few curve
//! lengths of the curve's range, non-negative tolerances, at most [`MAX_SEGMENTS`]
//! minimum segments and a parametric ratio in `[`[`MIN_RATIO`]`, 1]` (which bounds the
//! number of spans).

use crate::{call_with_buffer, FfiError};

/// Largest control point index `n`.
pub const MAX_INDEX: usize = 128;
/// Largest degree.
pub const MAX_DEGREE: usize = 9;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1e6;
/// The cache interval may reach this many curve ranges outside the curve's.
pub const MAX_REL_DIST: f64 = 2.0;
/// Largest `lMinimumNumberOfSegments`.
pub const MAX_SEGMENTS: usize = 64;
/// Smallest `dMinimumParametricRatio`.
pub const MIN_RATIO: f64 = 1e-4;

const NOZ: f64 = f64::MAX;
const NOW: f64 = -f64::MAX;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `Tessellate` and the decomposition read back.
    Tessellate,
    /// `GetAnalyticCurve`.
    Analytic,
    /// `GetType`, `IsKindOf`.
    Type,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Op::Tessellate => 0,
            Op::Analytic => 1,
            Op::Type => 2,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_curve_curve_cache_eval(
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

/// A curve block; its parameter range.
fn curve_block(rd: &mut Reader<'_>) -> Result<(f64, f64), FfiError> {
    let kind = rd.index(1)?;
    let dim = rd.index(3)?;
    if dim < 2 {
        return Err(bad("curve block: dimension must be 2 or 3"));
    }
    if kind == 0 {
        bspline(rd)
    } else {
        for _ in 0..4 {
            rd.vec3()?;
        }
        Ok((0.0, 1.0))
    }
}

/// A non-negative finite tolerance.
fn tol(rd: &mut Reader<'_>) -> Result<f64, FfiError> {
    let v = rd.finite()?;
    if v < 0.0 {
        return Err(bad("negative tolerance"));
    }
    Ok(v)
}

/// Checks `input` for `op` (see the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] describing the first problem.
pub fn validate(_op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    let (a, b) = curve_block(&mut rd)?;
    let far = MAX_REL_DIST * (b - a);
    for _ in 0..2 {
        let t = rd.finite()?;
        if t < a - far || t > b + far {
            return Err(bad("cache interval too far outside the curve"));
        }
    }
    rd.flag()?; // full
    tol(&mut rd)?; // chord
    tol(&mut rd)?; // angle
    tol(&mut rd)?; // off axis
    rd.index(MAX_SEGMENTS)?; // min segments
    rd.flag()?; // aux
    tol(&mut rd)?; // max 3d
    let ratio = rd.finite()?;
    if !(MIN_RATIO..=1.0).contains(&ratio) {
        return Err(bad("parametric ratio out of range"));
    }
    rd.done()
}

/// Runs `op` on the shim (see the module docs for the layouts).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] when [`validate`] rejects the input;
/// [`FfiError::Status`] for a shim failure.
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads (validated above: a well-formed curve block, the interval, the
        // constructor arguments); buf is valid for buf.len() writes and the shim
        // honours the capacity; len is valid.
        unsafe {
            sys::sc_curve_curve_cache_eval(
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
