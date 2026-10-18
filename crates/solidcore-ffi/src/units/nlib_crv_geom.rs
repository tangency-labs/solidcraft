//! FFI wrappers for unit `nlib.crv_geom`.
//!
//! Shim: `harness/cpp/shim/nlib_crv_geom.cpp` (`sc_nlib_crv_geom_eval`).
//! Upstream: `source/SMLib/src/NL_CrvGeom.cpp`.
//!
//! | [`Op`]      | upstream                   | parameters after the curve |
//! |-------------|----------------------------|----------------------------|
//! | `Closest`   | `N_CrvClosestPt`           | `P[3] u0 top toc`          |
//! | `ArcLength` | `N_CrvArcLength`           | `ul ur tol flg`            |
//! | `Polyline`  | `N_ApproxCrvWithPolyline`  | `tol tflg rflg`            |
//!
//! `flg` / `tflg` are `1` (`NL_ABSOLUTE`) or `2` (`NL_RELATIVE`), `rflg` `1`
//! (`NL_POINTS`), `2` (`NL_PARAMETERS`) or `3` (`NL_BOTH`). A curve is encoded `n p m`,
//! `n+1` control points `x y z w`, `m+1` knots. [`eval`] accepts only well-formed
//! clamped curves (`m = n+p+1`, `1 <= p <= n`, `p <= NL_MAXDEG`, finite data,
//! consistent dimension and rationality, non-decreasing knots with end multiplicity
//! exactly `p+1` and interior multiplicity at most `p`) and finite parameters within
//! the ranges below; everything else is rejected before the call.
//!
//! - `ArcLength`: `ul` and `ur` inside the knot range (`N_CrvExtractCrvSeg` reads
//!   outside its arrays otherwise).
//! - `Polyline`: upstream subdivides forever when the tolerance is below the
//!   resolution of the curve, so the (scaled) tolerance must be large enough:
//!   absolute `tol >= POLY_MIN_ABS_TOL * s`, where `s` is the largest absolute
//!   Euclidean control point coordinate; relative `tol >= POLY_MIN_REL_TOL` on a curve
//!   whose end points are at least `POLY_MIN_CHORD * s` apart (the arc length, which
//!   scales the tolerance, is at least that chord). Degree-1 curves take any finite
//!   tolerance (upstream does not use it).

use crate::validate::{bad, euclid};
use crate::{call_with_buffer, FfiError};

/// Largest control point index accepted by [`eval`].
pub const MAX_INDEX: usize = 4096;
/// Largest degree accepted by [`eval`] (upstream `NL_MAXDEG`).
pub const MAX_DEGREE: usize = 17;
/// Smallest absolute polyline tolerance, relative to the curve's coordinate size.
pub const POLY_MIN_ABS_TOL: f64 = 1e-7;
/// Smallest relative polyline tolerance.
pub const POLY_MIN_REL_TOL: f64 = 1e-6;
/// Smallest end-point distance (relative to the coordinate size) of a curve given a
/// relative polyline tolerance.
pub const POLY_MIN_CHORD: f64 = 1e-3;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `N_CrvClosestPt`.
    Closest,
    /// `N_CrvArcLength`.
    ArcLength,
    /// `N_ApproxCrvWithPolyline`.
    Polyline,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::Closest => 0,
            Op::ArcLength => 1,
            Op::Polyline => 2,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_nlib_crv_geom_eval(
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

/// A validated curve: Euclidean end points, coordinate size and knot range.
struct CurveInfo {
    first: [f64; 3],
    last: [f64; 3],
    size: f64,
    range: (f64, f64),
    degree: usize,
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
    let (pts, _) = cps.as_chunks::<4>();
    let [_, _, z0, w0] = *pts.first().ok_or_else(|| bad("empty curve"))?;
    let mut size = 0.0f64;
    for &[x, y, z, w] in pts {
        if (z == f64::MAX) != (z0 == f64::MAX) || (w == -f64::MAX) != (w0 == -f64::MAX) {
            return Err(bad("curve: mixed dimension or rationality"));
        }
        if !x.is_finite() || !y.is_finite() || !z.is_finite() || !w.is_finite() {
            return Err(bad("curve: non-finite control point"));
        }
        if w != -f64::MAX && w <= 0.0 {
            return Err(bad("curve: non-positive weight"));
        }
        for c in euclid([x, y, z, w]) {
            if !c.is_finite() {
                return Err(bad("curve: non-finite Euclidean point"));
            }
            size = size.max(c.abs());
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
    let first = euclid(*pts.first().ok_or_else(|| bad("empty curve"))?);
    let last = euclid(*pts.last().ok_or_else(|| bad("empty curve"))?);
    Ok(CurveInfo {
        first,
        last,
        size,
        range: (a, b),
        degree: p,
    })
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    let info = curve(&mut rd)?;
    match op {
        Op::Closest => {
            for _ in 0..6 {
                rd.finite()?;
            }
        }
        Op::ArcLength => {
            let ul = rd.finite()?;
            let ur = rd.finite()?;
            let (a, b) = info.range;
            if ul < a || ul > b || ur < a || ur > b {
                return Err(bad("arc length: parameters outside the knot range"));
            }
            rd.finite()?;
            rd.choice(&[1.0, 2.0])?;
        }
        Op::Polyline => {
            let tol = rd.finite()?;
            let tflg = rd.choice(&[1.0, 2.0])?;
            rd.choice(&[1.0, 2.0, 3.0])?;
            if info.degree == 1 {
                // Upstream returns the control polygon without using the tolerance.
            } else if tflg == 1.0 {
                if tol < POLY_MIN_ABS_TOL * info.size || tol <= 0.0 {
                    return Err(bad("polyline: absolute tolerance too small"));
                }
            } else {
                let [x0, y0, z0] = info.first;
                let [x1, y1, z1] = info.last;
                let chord =
                    ((x1 - x0) * (x1 - x0) + (y1 - y0) * (y1 - y0) + (z1 - z0) * (z1 - z0)).sqrt();
                if tol < POLY_MIN_REL_TOL
                    || chord.is_nan()
                    || chord < POLY_MIN_CHORD * info.size
                    || chord <= 0.0
                {
                    return Err(bad("polyline: relative tolerance too small for the curve"));
                }
            }
        }
    }
    rd.done()
}

/// Runs `op` on `input` (curve, then the op's parameters); returns the shim's output
/// record (status, then outputs).
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the
        // shim reads for `op` (validated above: a well-formed clamped curve, parameters
        // inside the knot range and flags upstream handles); buf is valid for
        // buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_nlib_crv_geom_eval(
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
