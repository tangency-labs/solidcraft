//! FFI wrappers for unit `nurbs.b_spline_curve_p3`.
//!
//! Shim: `harness/cpp/shim/nurbs_b_spline_curve_p3.cpp`
//! (`sc_nurbs_b_spline_curve_p3_eval`). Upstream: `source/SMLib/src/SmBSplineCurve.cpp`.
//!
//! Every op starts with a curve block `has_nurb dim oob [n p m cpts(x y z w) knots]`
//! (`this`: with a NURB built with `SmBSplineCurve(gw_CURVE*, lDim, FALSE)`, else with
//! `SmBSplineCurve(lDim)`; then `SetOutOfBoundsEnabled(oob)`). The op's parameters:
//!
//! | [`Op`]      | upstream                    | parameters                                   |
//! |-------------|-----------------------------|----------------------------------------------|
//! | `Scale`     | `CreateByScaleTransRot`     | `SV[3] EV[3] deg`                            |
//! | `Pole`      | `HasInternalPole`           | `tol`                                        |
//! | `Segment`   | `IsSegmentLine`             | `a b ns tol`                                 |
//! | `Cp`        | `GetControlPoint`           | `form index`                                 |
//! | `OnPlane`   | `IsOnPlane`                 | `P[3] N[3] tol has_ivl a b`                  |
//! | `Join`      | `JoinWith`                  | `this_end` curve block `other_end has_tol tol` |
//! | `Subdivide` | `SubdivideAtDiscontinuity`  | `continuity angle_tol`                       |
//! | `Gap`       | `TrimGap`                   | curve block `fix dmin`                       |
//! | `Assert`    | `AssertValid`               | `level_given`                                |
//!
//! Intervals `a b` are `SmExtent1d(a, b)` (swapped when `a > b`). The output is the op
//! status (`0` = `SM_SUCCESS`, else the `SmStatus`; `0` for the predicates), the op's
//! outputs on success, then the state of `this`:
//!
//! - `Scale`: whether a curve was made and its state. `Pole`: the flag.
//! - `Segment`: the flag and `P[3] V[3]` (NaN where upstream leaves them unset); a
//!   status passed through the `SmBoolean` is the record's status.
//! - `Cp`: `P[3] w`. `OnPlane`: the flag and the maximum distance (NaN when unset).
//! - `Join`: (always) the state of the other curve. `Gap`: the `int` result, then the
//!   state of the other curve.
//! - `Subdivide`: the count and the state of every piece.
//! - `Assert`: the flag, the report count and per report `test tol3d value u v`.
//!
//! A state is `dim oob borrowed knot_type form has_nurb [n p m cpts knots]`.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour, crashes or
//! runaway work: well-formed clamped curves (`m = n+p+1`, `1 <= p <= n`, `p <= 17`,
//! finite data, consistent dimension and rationality, positive weights, bounded
//! Euclidean points, non-decreasing knots with end multiplicity exactly `p+1` and
//! interior multiplicity at most `p`; for `Assert` also end multiplicity `p+2` and
//! interior multiplicity `p+1`), dimensions 1 to 8, a curve without NURB only for the
//! ops that handle one (`Pole`, `Assert`), and per op the restrictions documented on
//! [`validate_op`].

use crate::validate::{bad, euclid, NOW, NOZ};
use crate::{call_with_buffer, FfiError};

/// Largest control point index accepted by [`eval`].
pub const MAX_INDEX: usize = 2048;
/// Largest NURB degree accepted by [`eval`].
pub const MAX_DEGREE: usize = 17;
/// Largest magnitude of coordinates and parameters.
pub const MAX_COORD: f64 = 1e6;
/// Largest degree of a `Join` curve.
pub const MAX_JOIN_DEGREE: usize = 9;
/// Largest out-of-range parameter, relative to the curve's range.
pub const MAX_REL_DIST: f64 = 16.0;
/// Largest number of samples of `Segment`.
pub const MAX_SAMPLES: usize = 200;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Scale,
    Pole,
    Segment,
    Cp,
    OnPlane,
    Join,
    Subdivide,
    Gap,
    Assert,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::Scale => 0,
            Op::Pole => 1,
            Op::Segment => 2,
            Op::Cp => 3,
            Op::OnPlane => 4,
            Op::Join => 5,
            Op::Subdivide => 6,
            Op::Gap => 7,
            Op::Assert => 8,
        }
    }

    /// Whether the op handles a curve without NURB.
    #[must_use]
    pub fn handles_empty(self) -> bool {
        matches!(self, Op::Pole | Op::Assert)
    }
}

mod sys {
    extern "C" {
        pub fn sc_nurbs_b_spline_curve_p3_eval(
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

/// A validated curve block.
#[derive(Debug, Clone)]
struct CurveInfo {
    dim: usize,
    /// Euclidean control points (empty without NURB).
    pts: Vec<[f64; 3]>,
    p: usize,
    knots: Vec<f64>,
    three_d: bool,
    has_nurb: bool,
}

impl CurveInfo {
    fn range(&self) -> (f64, f64) {
        (
            self.knots.first().copied().unwrap_or(0.0),
            self.knots.last().copied().unwrap_or(0.0),
        )
    }

    /// An end pair the extensions can take a tangent from (or a third point).
    fn extension_ok(&self) -> bool {
        let n = self.pts.len().saturating_sub(1);
        if n >= 2 {
            return true;
        }
        let far = |i: usize, j: usize| match (self.pts.get(i), self.pts.get(j)) {
            (Some(a), Some(b)) => {
                let d =
                    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
                d > 1e-150 && d < 1e150
            }
            _ => false,
        };
        far(0, 1) && far(n, n.wrapping_sub(1))
    }
}

/// Reads and validates a curve `n p m cpts knots`; `relaxed` also accepts end
/// multiplicity `p + 2` and interior multiplicity `p + 1`.
fn curve(rd: &mut Reader<'_>, dim: usize, relaxed: bool) -> Result<CurveInfo, FfiError> {
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
    let mut pts = Vec::with_capacity(cpts.len());
    for &[x, y, z, w] in cpts {
        if (z == NOZ) != (z0 == NOZ) || (w == NOW) != (w0 == NOW) {
            return Err(bad("curve: mixed dimension or rationality"));
        }
        if !x.is_finite() || !y.is_finite() || !z.is_finite() || !w.is_finite() {
            return Err(bad("curve: non-finite control point"));
        }
        if w != NOW && w <= 0.0 {
            return Err(bad("curve: non-positive weight"));
        }
        let e = euclid([x, y, z, w]);
        if e.iter().any(|c| !c.is_finite() || c.abs() > MAX_COORD) {
            return Err(bad("curve: Euclidean point out of range"));
        }
        pts.push(e);
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
    if b - a < 1e-6 * a.abs().max(b.abs()) {
        return Err(bad("curve: parameter range too small"));
    }
    let extra_end = usize::from(relaxed);
    let at_a = inner.iter().filter(|&&k| k == a).count();
    let at_b = inner.iter().filter(|&&k| k == b).count();
    if at_a > extra_end || at_b > extra_end {
        return Err(bad("curve: end multiplicity too high"));
    }
    let max_run = p + usize::from(relaxed);
    let mut run = 1;
    for w in inner.windows(2) {
        run = if w.first() == w.last() { run + 1 } else { 1 };
        if run > max_run {
            return Err(bad("curve: interior multiplicity too high"));
        }
    }
    Ok(CurveInfo {
        dim,
        pts,
        p,
        knots: knots.to_vec(),
        three_d: z0 != NOZ,
        has_nurb: true,
    })
}

/// A curve block `has_nurb dim oob [curve]`.
fn curve_block(rd: &mut Reader<'_>, relaxed: bool) -> Result<CurveInfo, FfiError> {
    let has = rd.flag()?;
    let dim = rd.index(8)?;
    if dim == 0 {
        return Err(bad("curve block: dimension 0"));
    }
    rd.flag()?;
    if has {
        curve(rd, dim, relaxed)
    } else {
        Ok(CurveInfo {
            dim,
            pts: Vec::new(),
            p: 0,
            knots: Vec::new(),
            three_d: false,
            has_nurb: false,
        })
    }
}

/// An interval `a b` within [`MAX_REL_DIST`] ranges of the curve's.
fn interval(rd: &mut Reader<'_>, c: &CurveInfo) -> Result<(f64, f64), FfiError> {
    let x = rd.finite()?;
    let y = rd.finite()?;
    let (lo, hi) = (x.min(y), x.max(y));
    let (a, b) = c.range();
    let far = MAX_REL_DIST * (b - a);
    if lo < a - far || hi > b + far {
        return Err(bad("interval too far outside the curve"));
    }
    Ok((lo, hi))
}

/// A curve `JoinWith` hands to `CreateByJoining` (as `CreateByJoining`'s segments in
/// `nurbs.b_spline_curve_p2`).
fn join_curve(c: &CurveInfo) -> Result<(), FfiError> {
    if !c.has_nurb {
        return Err(bad("join: curve without NURB"));
    }
    if !(2..=3).contains(&c.dim) || c.three_d != (c.dim == 3) {
        return Err(bad("join: dimension must be 2 or 3 and match the data"));
    }
    if c.p > MAX_JOIN_DEGREE {
        return Err(bad("join: degree too high"));
    }
    Ok(())
}

/// Validates one op's parameters (after `this`):
///
/// - Ops other than `Pole` and `Assert` need a NURB; only `Assert` takes the relaxed
///   knot vectors.
/// - `Scale`, `Cp`: dimension at least 2 (`GetControlPointsExpert` writes two values
///   per point). `Cp`: form `0..=2`.
/// - `Segment`, `OnPlane`: intervals within [`MAX_REL_DIST`] ranges of the curve's;
///   `Segment` at most [`MAX_SAMPLES`] samples.
/// - `Join`: both curves 2D or 3D (matching their data) of degree at most
///   [`MAX_JOIN_DEGREE`], the other with a NURB.
/// - `Subdivide`: a continuity `0..=13`.
/// - `Gap`: the other curve with a NURB, both with an end pair (or a third point)
///   the G1 extensions can take a tangent from, `fix` `0..=2`.
fn validate_op(op: Op, rd: &mut Reader<'_>, c: &CurveInfo) -> Result<(), FfiError> {
    if !c.has_nurb && !op.handles_empty() {
        return Err(bad("op needs a NURB"));
    }
    match op {
        Op::Scale => {
            rd.vec3()?;
            rd.vec3()?;
            rd.finite()?;
            if c.dim < 2 {
                return Err(bad("scale: dimension below 2"));
            }
        }
        Op::Pole => {
            rd.finite()?;
        }
        Op::Segment => {
            interval(rd, c)?;
            rd.index(MAX_SAMPLES)?;
            rd.finite()?;
        }
        Op::Cp => {
            rd.index(2)?;
            rd.index(MAX_INDEX + 4)?;
            if c.dim < 2 {
                return Err(bad("cp: dimension below 2"));
            }
        }
        Op::OnPlane => {
            rd.vec3()?;
            rd.vec3()?;
            rd.finite()?;
            if rd.flag()? {
                interval(rd, c)?;
            } else {
                rd.finite()?;
                rd.finite()?;
            }
        }
        Op::Join => {
            rd.flag()?;
            let other = curve_block(rd, false)?;
            rd.flag()?;
            rd.flag()?;
            rd.finite()?;
            join_curve(c)?;
            join_curve(&other)?;
        }
        Op::Subdivide => {
            rd.index(13)?;
            rd.finite()?;
        }
        Op::Gap => {
            let other = curve_block(rd, false)?;
            rd.index(2)?;
            rd.finite()?;
            if !other.has_nurb {
                return Err(bad("gap: other curve without NURB"));
            }
            if !c.extension_ok() || !other.extension_ok() {
                return Err(bad("gap: end pair not normalisable and no third point"));
            }
        }
        Op::Assert => {
            rd.flag()?;
        }
    }
    Ok(())
}

/// Checks `input` for `op` as [`eval`] does, without calling the shim (for case
/// generators that resample rejected inputs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input [`eval`] rejects.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    let this = curve_block(&mut rd, op == Op::Assert)?;
    validate_op(op, &mut rd, &this)?;
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
        // reads for `op` (validated above: curve blocks with well-formed clamped curves,
        // every index, count and flag the shim passes to upstream, and parameters in the
        // ranges upstream handles); buf is valid for buf.len() writes and the shim
        // honours the capacity; len is valid.
        unsafe {
            sys::sc_nurbs_b_spline_curve_p3_eval(
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
