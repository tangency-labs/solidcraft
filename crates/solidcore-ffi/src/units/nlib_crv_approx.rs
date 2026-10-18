//! FFI wrappers for unit `nlib.crv_approx`.
//!
//! Shim: `harness/cpp/shim/nlib_crv_approx.cpp` (`sc_nlib_crv_approx_eval`).
//! Upstream: `source/SMLib/src/NL_CrvApprox.cpp`.
//!
//! | [`Op`]        | upstream                     | input                                     |
//! |---------------|------------------------------|-------------------------------------------|
//! | `NumCpts`     | `N_CalcNumCPtsToApproxArc`   | `tol ang deg mode`                        |
//! | `CircArcData` | `N_ApproxCircArcWithCrvData` | `cen[3] X[3] Y[3] rad as ae Ps[3] Pe[3] deg nc` |
//! | `NonRat`      | `N_ApproxNurbsWithNonRatCrv` | curve, `tol deg par tan`                  |
//!
//! `mode` of `NumCpts`: `0` = `sfn` NULL with `flg = NL_NO`, `1` = `sfn` NULL with
//! `flg = NL_YES`, `2` = `sfn` built by a first call (`tol = 1`, `ang = 360`, same
//! degree, `NL_YES`) and passed in. `par` is `1..=4` (`NL_UNIFORM`, `NL_CHORDLENGTH`,
//! `NL_CENTRIPETAL`, `NL_INHERITED`), `tan` `0` or `1`. A curve is encoded `n p m`,
//! `n+1` control points `x y z w`, `m+1` knots.
//!
//! The output record is the status (`0` ok, else the `NL_*_ERR` number, `1000` for a
//! bare error flag) and on success: `nc` (`NumCpts`) or the curve (`n p m`, control
//! points, knots).
//!
//! [`eval`] accepts only:
//! - finite numbers everywhere, integer counts and flags in range;
//! - `NumCpts`: any finite `tol` and `ang`, `deg <= MAX_DEGREE`;
//! - `CircArcData`: `|values| <= MAX_COORD`, `deg <= MAX_DEGREE`, `nc <= MAX_NC`;
//!   for inputs past the routine's own checks, axes that are not nearly parallel
//!   (`|X.Y| <= MAX_AXIS_COS |X||Y|`) and a sweep of at least `MIN_SWEEP` degrees,
//!   so the end tangents are non-zero (degree 2 projects onto them and uses the
//!   projection even when it fails; degrees 3 and 4 with few control points estimate
//!   a zero tangent from four points they do not have); and for degree 2 with
//!   `nc == 2` end tangents
//!   that are clearly not parallel (upstream then returns success with an
//!   uninitialised middle control point);
//! - `NonRat`: a well-formed clamped curve (`m = n+p+1`, `1 <= p <= n <= MAX_INDEX`,
//!   consistent dimension and rationality, positive weights, coordinates and weights
//!   within `MAX_COORD`, non-decreasing knots with end multiplicity exactly `p+1` and
//!   interior multiplicity at most `p`), `MIN_TOL <= tol <= 1` (so every Bezier
//!   segment gets at least `deg` samples) or `tol == 0` (rejected upstream), `1 <= deg <= MAX_DEGREE`.

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

/// Largest control point index of an input curve.
pub const MAX_INDEX: usize = 2048;
/// Largest degree (of the input curve and of the approximation).
pub const MAX_DEGREE: usize = 9;
/// Largest `nc` of `CircArcData`.
pub const MAX_NC: usize = 400;
/// Largest magnitude of coordinates, radii, angles and weights.
pub const MAX_COORD: f64 = 1e6;
/// Smallest `tol` of `NonRat`.
pub const MIN_TOL: f64 = 1e-7;
/// Largest `|cos|` of the angle between the axes of a degree 2 `CircArcData`.
pub const MAX_AXIS_COS: f64 = 0.99;
/// Smallest sweep (degrees) of a degree 2 `CircArcData`.
pub const MIN_SWEEP: f64 = 1e-6;
/// Smallest `1 - cos^2` of the angle between the end tangents of a degree 2
/// `CircArcData` with `nc == 2`.
pub const MIN_TANGENT_SIN2: f64 = 1e-6;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `N_CalcNumCPtsToApproxArc`.
    NumCpts,
    /// `N_ApproxCircArcWithCrvData`.
    CircArcData,
    /// `N_ApproxNurbsWithNonRatCrv`.
    NonRat,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::NumCpts => 0,
            Op::CircArcData => 1,
            Op::NonRat => 2,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_nlib_crv_approx_eval(
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

impl Reader<'_> {
    fn finite(&mut self) -> Result<f64, FfiError> {
        let v = self
            .input
            .get(self.at)
            .copied()
            .ok_or_else(|| bad("input too short"))?;
        self.at += 1;
        if !v.is_finite() {
            return Err(bad("non-finite input"));
        }
        Ok(v)
    }

    fn bounded(&mut self) -> Result<f64, FfiError> {
        let v = self.finite()?;
        if v.abs() > MAX_COORD {
            return Err(bad("value out of range"));
        }
        Ok(v)
    }

    fn vec3(&mut self) -> Result<[f64; 3], FfiError> {
        Ok([self.bounded()?, self.bounded()?, self.bounded()?])
    }

    fn index(&mut self, max: usize) -> Result<usize, FfiError> {
        let v = self.finite()?;
        if v.fract() != 0.0 || v < 0.0 || v > max as f64 {
            return Err(bad(format!("bad index/count {v}")));
        }
        Ok(v as usize)
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    let d = dot(a, a).sqrt();
    if d > 1e-150 {
        Some([a[0] / d, a[1] / d, a[2] / d])
    } else {
        None
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
    let (cpts, _) = cps.as_chunks::<4>();
    let [_, _, z0, w0] = *cpts.first().ok_or_else(|| bad("empty curve"))?;
    for &[x, y, z, w] in cpts {
        if (z == f64::MAX) != (z0 == f64::MAX) || (w == -f64::MAX) != (w0 == -f64::MAX) {
            return Err(bad("curve: mixed dimension or rationality"));
        }
        let zz = if z == f64::MAX { 0.0 } else { z };
        for c in [x, y, zz] {
            if !c.is_finite() || c.abs() > MAX_COORD {
                return Err(bad("curve: coordinate out of range"));
            }
        }
        if w != -f64::MAX && !(1.0 / MAX_COORD..=MAX_COORD).contains(&w) {
            return Err(bad("curve: weight out of range"));
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

/// The restrictions of `CircArcData` (non-zero end tangents; for `nc == 2`, given
/// only for degree 2, tangents that are not parallel).
fn arc_tangents_ok(x: [f64; 3], y: [f64; 3], as_: f64, ae: f64, nc: usize) -> Result<(), FfiError> {
    let (Some(xu), Some(yu)) = (unit(x), unit(y)) else {
        return Ok(()); // Rejected with NL_INP_ERR before any use.
    };
    if dot(xu, yu).abs() > MAX_AXIS_COS {
        return Err(bad("arc: axes nearly parallel"));
    }
    let mut ae = ae;
    let mut closed = false;
    if ae <= as_ {
        ae += 360.0;
    }
    if ae - as_ > 359.9999 {
        ae = as_ + 360.0;
        closed = true;
    }
    if ae - as_ < MIN_SWEEP {
        return Err(bad("arc: sweep too small"));
    }
    if nc == 2 {
        if closed {
            return Err(bad("arc: closed parabolic arc has parallel tangents"));
        }
        let (a, b) = (as_.to_radians(), ae.to_radians());
        let tan = |t: f64| {
            let (s, c) = t.sin_cos();
            [
                -s * xu[0] + c * yu[0],
                -s * xu[1] + c * yu[1],
                -s * xu[2] + c * yu[2],
            ]
        };
        let (Some(t0), Some(t1)) = (unit(tan(a)), unit(tan(b))) else {
            return Err(bad("arc: zero tangent"));
        };
        let c = dot(t0, t1);
        if 1.0 - c * c < MIN_TANGENT_SIN2 {
            return Err(bad("arc: parallel end tangents"));
        }
    }
    Ok(())
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    match op {
        Op::NumCpts => {
            rd.finite()?;
            rd.finite()?;
            rd.index(MAX_DEGREE)?;
            rd.index(2)?;
        }
        Op::CircArcData => {
            rd.vec3()?;
            let x = rd.vec3()?;
            let y = rd.vec3()?;
            let rad = rd.bounded()?;
            let as_ = rd.bounded()?;
            let ae = rd.bounded()?;
            rd.vec3()?;
            rd.vec3()?;
            let deg = rd.index(MAX_DEGREE)?;
            let nc = rd.index(MAX_NC)?;
            if (2..=4).contains(&deg) && nc >= deg && rad > 0.0 {
                arc_tangents_ok(x, y, as_, ae, if deg == 2 { nc } else { 0 })?;
            }
        }
        Op::NonRat => {
            curve(&mut rd)?;
            let tol = rd.finite()?;
            // tol == 0 fails the 1/tol check before any use.
            if tol != 0.0 && !(MIN_TOL..=1.0).contains(&tol) {
                return Err(bad("non-rational approximation: tol out of range"));
            }
            let deg = rd.index(MAX_DEGREE)?;
            if deg == 0 {
                return Err(bad("non-rational approximation: degree 0"));
            }
            let par = rd.index(4)?;
            if par == 0 {
                return Err(bad("non-rational approximation: bad par flag"));
            }
            rd.index(1)?;
        }
    }
    rd.done()
}

/// Runs `op` on `input`; returns the shim's output record (status, then outputs).
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the
        // shim reads for `op` (validated above: well-formed clamped curves, finite
        // parameters in the ranges upstream handles, known flags); buf is valid for
        // buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_nlib_crv_approx_eval(
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
