//! FFI wrappers for unit `nurbs.b_spline_curve_p2`.
//!
//! Shim: `harness/cpp/shim/nurbs_b_spline_curve_p2.cpp`
//! (`sc_nurbs_b_spline_curve_p2_eval`). Upstream: `source/SMLib/src/SmBSplineCurve.cpp`.
//!
//! Curve ops start with a curve block `has_nurb dim oob [n p m cpts(x y z w) knots]`
//! (`this`: with a NURB built with `SmBSplineCurve(gw_CURVE*, lDim, FALSE)`, else with
//! `SmBSplineCurve(lDim)`; then `SetOutOfBoundsEnabled(oob)`); static ops do not. The
//! op's parameters:
//!
//! | [`Op`]         | upstream                         | parameters                                   |
//! |----------------|----------------------------------|----------------------------------------------|
//! | `Intersector`  | `IsIntersectorApprox`            |                                              |
//! | `BBox`         | `CalculateBoundingBox`           | `a b normal pseudo polar`                    |
//! | `Analytic`     | `CreateAnalyticCurve`            | `a b`                                        |
//! | `Join`         | `CreateByJoining` (static)       | `k` curve blocks, `has_senses S[k]`          |
//! | `Point`        | `CreatePointCurve` (static)      | `P[3]`                                       |
//! | `Offset`       | `CreateSimpleOffset`             | `tol N[3] d match`                           |
//! | `Repeated`     | `HasRepeatedEndControlPoints`    | `tol`                                        |
//! | `Canonical`    | `GetCanonical`                   |                                              |
//! | `IsLine`       | `IsLine`                         | `ns tol`                                     |
//! | `C0`           | `IsPiecewiseC0`                  |                                              |
//! | `Degenerate`   | `IsDegenerate`                   | `tol has_ivl a b`                            |
//! | `Expert`       | `GetControlPointsExpert`         | `form start end stride`                      |
//! | `MaxDomain`    | `GetMaxAnalyticDomain`           |                                              |
//! | `Unique`       | `GetNumberOfUniqueKnots`         |                                              |
//! | `Polygon`      | `GetPolygon`                     | `a b`                                        |
//! | `Match`        | `MatchWeights`                   | curve block (the other curve)                |
//! | `Elevate`      | `DegreeElevate`                  | `deg`                                        |
//! | `Reverse`      | `ReverseParameterization`        | `a b`                                        |
//! | `SetCanonical` | `SetCanonical`                   | `dim deg ncp P[3 ncp] form nk K[nk] M[nk] kt has_w [nw W[nw]] has_trim a b` |
//! | `SetCp`        | `SetControlPoint`                | `form index P[3] w`                          |
//! | `Adjust`       | `AdjustSTEPInterval`             | `a b`                                        |
//! | `ReadDb`       | `ReadFromDB` (static)            | `dim file_type version nb B[nb]`             |
//!
//! Intervals `a b` are `SmExtent1d(a, b)` (swapped when `a > b`). The output is the op
//! status (`0` = `SM_SUCCESS`, else the `SmStatus`; `0` for the predicates), the op's
//! outputs on success, then for curve ops the state of `this`:
//!
//! - `Intersector`, `Repeated`, `C0`, `Degenerate`: the flag; `IsLine` the flag and,
//!   when set, `P[3] V[3]`; `Unique` the count; `MaxDomain`, `Reverse` the interval.
//! - `BBox`: the normal box `min[3] max[3]`, the pseudo box `basis[9] intervals[6]`,
//!   the polar box `state basis[9] umin vmin umax vmax`, each when asked for.
//! - `Analytic`: whether a curve was made. `Join`, `Point`, `ReadDb`: whether a curve
//!   was made and its state; `Join` then (always) the state of every segment. `Offset`:
//!   `gap`, whether a curve was made and its state.
//! - `Canonical`: `dim deg form kt ncp P[3 ncp] nk K[nk] M[nk] nw W[nw]`.
//! - `Expert`: `len` and `len = stride * count` values (zero where not written).
//! - `Polygon`: `count P[3 count]`.
//!
//! A state is `dim oob borrowed knot_type form has_nurb [n p m cpts knots]`.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour, crashes or
//! runaway work: well-formed clamped curves (`m = n+p+1`, `1 <= p <= n`, `p <= 17`,
//! finite data, consistent dimension and rationality, positive weights, bounded
//! Euclidean points, non-decreasing knots with end multiplicity exactly `p+1` and
//! interior multiplicity at most `p`), dimensions 1 to 8, a curve without NURB only for
//! the op that handles one (`Repeated`), and per op the restrictions
//! documented on [`validate_op`].

use crate::validate::{bad, euclid, NOW, NOZ};
use crate::{call_with_buffer, FfiError};

/// Largest control point index accepted by [`eval`].
pub const MAX_INDEX: usize = 2048;
/// Largest NURB degree accepted by [`eval`].
pub const MAX_DEGREE: usize = 17;
/// Largest magnitude of coordinates and parameters.
pub const MAX_COORD: f64 = 1e6;
/// Largest number of segments of `Join`.
pub const MAX_SEGMENTS: usize = 16;
/// Largest degree of a `Join` segment.
pub const MAX_JOIN_DEGREE: usize = 9;
/// Largest extension (or out-of-range parameter), relative to the curve's range.
pub const MAX_REL_DIST: f64 = 16.0;
/// Largest number of samples of `IsLine`.
pub const MAX_SAMPLES: usize = 200;
/// Largest stride of `Expert`.
pub const MAX_STRIDE: usize = 16;
/// Largest new degree of `Elevate`.
pub const MAX_NEW_DEGREE: usize = 24;
/// Largest control point / knot count of `ReadDb`.
pub const MAX_DB_INDEX: usize = 2048;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Intersector,
    BBox,
    Analytic,
    Join,
    Point,
    Offset,
    Repeated,
    Canonical,
    IsLine,
    C0,
    Degenerate,
    Expert,
    MaxDomain,
    Unique,
    Polygon,
    Match,
    Elevate,
    Reverse,
    SetCanonical,
    SetCp,
    Adjust,
    ReadDb,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::Intersector => 0,
            Op::BBox => 1,
            Op::Analytic => 2,
            Op::Join => 3,
            Op::Point => 4,
            Op::Offset => 5,
            Op::Repeated => 6,
            Op::Canonical => 7,
            Op::IsLine => 8,
            Op::C0 => 9,
            Op::Degenerate => 10,
            Op::Expert => 11,
            Op::MaxDomain => 12,
            Op::Unique => 13,
            Op::Polygon => 14,
            Op::Match => 15,
            Op::Elevate => 16,
            Op::Reverse => 17,
            Op::SetCanonical => 18,
            Op::SetCp => 19,
            Op::Adjust => 20,
            Op::ReadDb => 21,
        }
    }

    /// Whether the op is static (no `this`).
    #[must_use]
    pub fn is_static(self) -> bool {
        matches!(self, Op::Join | Op::Point | Op::ReadDb)
    }

    /// Whether the op handles a curve without NURB.
    #[must_use]
    pub fn handles_empty(self) -> bool {
        matches!(self, Op::Repeated)
    }
}

mod sys {
    extern "C" {
        pub fn sc_nurbs_b_spline_curve_p2_eval(
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
}

/// Reads and validates a curve `n p m cpts knots`.
fn curve(rd: &mut Reader<'_>, dim: usize) -> Result<CurveInfo, FfiError> {
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
fn curve_block(rd: &mut Reader<'_>) -> Result<CurveInfo, FfiError> {
    let has = rd.flag()?;
    let dim = rd.index(8)?;
    if dim == 0 {
        return Err(bad("curve block: dimension 0"));
    }
    rd.flag()?;
    if has {
        curve(rd, dim)
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

/// Canonical data `dim deg ncp P[3 ncp] form nk K[nk] M[nk] kt has_w [nw W[nw]]
/// has_trim a b` (as `CreateCanonical` of `nurbs.b_spline_curve_p1`).
fn canonical(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let dim = rd.index(8)?;
    let deg = rd.index(40)?;
    let ncp = rd.index(MAX_INDEX)?;
    rd.finites(3 * ncp)?;
    rd.index(6)?;
    let nk = rd.index(MAX_INDEX)?;
    rd.finites(nk)?;
    let mut total = 0usize;
    for _ in 0..nk {
        total += rd.index(64)?;
    }
    rd.index(4)?;
    let weights_ok = if rd.flag()? {
        let nw = rd.index(MAX_INDEX)?;
        rd.finites(nw)?;
        nw == ncp
    } else {
        true
    };
    rd.flag()?;
    rd.finites(2)?;
    let checked = (1..=3).contains(&dim) && (1..=32).contains(&deg) && weights_ok;
    if checked && (ncp == 0 || total < ncp + deg + 1) {
        return Err(bad("canonical: need ncp >= 1 and ncp + deg + 1 knots"));
    }
    Ok(())
}

/// The numbers of one line of an ASCII stream (at least `min`).
fn line_numbers(line: &str, min: usize) -> Result<Vec<f64>, FfiError> {
    let v: Vec<f64> = line
        .split_whitespace()
        .map(|t| t.parse::<f64>().map_err(|_| bad("read_db: not a number")))
        .collect::<Result<_, _>>()?;
    if v.len() < min {
        return Err(bad("read_db: too few values on a line"));
    }
    if v.iter().any(|x| !x.is_finite()) {
        return Err(bad("read_db: non-finite value"));
    }
    Ok(v)
}

fn int_of(v: f64, max: usize) -> Result<usize, FfiError> {
    if v.fract() != 0.0 || v < 0.0 || v > max as f64 {
        return Err(bad("read_db: bad size"));
    }
    Ok(v as usize)
}

/// A database stream holding every value `ReadFromDB` reads, with sizes in range.
fn check_db(bytes: &[u8], ascii: bool, version: usize) -> Result<(), FfiError> {
    if ascii {
        let text = core::str::from_utf8(bytes).map_err(|_| bad("read_db: not text"))?;
        let mut lines = text.split('\n');
        let mut next = |min: usize| -> Result<Vec<f64>, FfiError> {
            line_numbers(
                lines.next().ok_or_else(|| bad("read_db: too few lines"))?,
                min,
            )
        };
        let get = |l: &[f64]| l.first().copied().unwrap_or(-1.0);
        let n = int_of(get(&next(1)?), MAX_DB_INDEX)?;
        let p = int_of(get(&next(1)?), MAX_DEGREE)?;
        let rat = get(&next(1)?);
        let dim = get(&next(1)?);
        if rat.fract() != 0.0
            || rat.abs() > 32767.0
            || dim.fract() != 0.0
            || !(0.0..=8.0).contains(&dim)
        {
            return Err(bad("read_db: bad rational flag or dimension"));
        }
        if version > 32 {
            int_of(get(&next(1)?), 4)?;
            int_of(get(&next(1)?), 6)?;
        }
        let per = 2 + usize::from(dim != 2.0) + usize::from(rat != 0.0);
        for _ in 0..=n {
            next(per)?;
        }
        for _ in 0..n + p + 2 {
            next(1)?;
        }
        Ok(())
    } else {
        let rd8 = |at: usize| -> Result<i64, FfiError> {
            let b = bytes
                .get(at..at + 8)
                .ok_or_else(|| bad("read_db: short header"))?;
            let mut a = [0u8; 8];
            a.copy_from_slice(b);
            Ok(i64::from_ne_bytes(a))
        };
        let rd2 = |at: usize| -> Result<i16, FfiError> {
            let b = bytes
                .get(at..at + 2)
                .ok_or_else(|| bad("read_db: short header"))?;
            Ok(i16::from_ne_bytes([
                b.first().copied().unwrap_or(0),
                b.get(1).copied().unwrap_or(0),
            ]))
        };
        let size = |v: i64, max: usize| -> Result<usize, FfiError> {
            usize::try_from(v)
                .ok()
                .filter(|&x| x <= max)
                .ok_or_else(|| bad("read_db: bad size"))
        };
        let n = size(rd8(0)?, MAX_DB_INDEX)?;
        let p = size(i64::from(rd2(8)?), MAX_DEGREE)?;
        let rat = rd2(10)?;
        let dim = rd2(12)?;
        if !(0..=8).contains(&dim) {
            return Err(bad("read_db: bad dimension"));
        }
        let mut at = 14;
        if version > 32 {
            let kt = rd8(at)?;
            let form = rd8(at + 8)?;
            if !(0..=4).contains(&kt) || !(0..=6).contains(&form) {
                return Err(bad("read_db: knot type or form out of range"));
            }
            at += 16;
        }
        let per = 2 + usize::from(dim != 2) + usize::from(rat != 0);
        let doubles = (n + 1) * per + (n + p + 2);
        if bytes.len() < at + 8 * doubles {
            return Err(bad("read_db: short data"));
        }
        Ok(())
    }
}

/// Validates the parameters of a static op.
fn validate_static(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match op {
        Op::Join => {
            let k = rd.index(MAX_SEGMENTS)?;
            if k == 0 {
                return Err(bad("join: needs a segment"));
            }
            let mut segs = Vec::with_capacity(k);
            for _ in 0..k {
                let c = curve_block(rd)?;
                if !c.has_nurb {
                    return Err(bad("join: segment without NURB"));
                }
                if !(2..=3).contains(&c.dim) || c.three_d != (c.dim == 3) {
                    return Err(bad(
                        "join: segment dimension must be 2 or 3 and match the data",
                    ));
                }
                if c.p > MAX_JOIN_DEGREE {
                    return Err(bad("join: degree too high"));
                }
                segs.push(c);
            }
            rd.flag()?;
            for _ in 0..k {
                rd.flag()?;
            }
        }
        Op::Point => {
            rd.vec3()?;
        }
        Op::ReadDb => {
            rd.index(8)?;
            let ascii = !rd.flag()?;
            let version = rd.index(100)?;
            let nb = rd.index(1 << 20)?;
            let bytes: Vec<u8> = (0..nb)
                .map(|_| rd.index(255).map(|b| b as u8))
                .collect::<Result<_, _>>()?;
            check_db(&bytes, ascii, version)?;
        }
        _ => return Err(bad("not a static op")),
    }
    Ok(())
}

/// Validates one curve op's parameters (after `this`):
///
/// - Ops other than `Repeated` need a NURB.
/// - `BBox`, `Analytic`, `Reverse`, `Adjust`, `Degenerate`: intervals within
///   [`MAX_REL_DIST`] ranges of the curve's; `BBox` with out-of-bounds evaluation needs
///   its interval inside the curve's range; `Adjust` needs `b - a` of at least `1e-6`
///   (relative to the knot magnitude).
/// - `Offset`: a single-span 3D curve without interior knots and a tolerance in
///   `[1e-6, 1]` (the C++ side approximates an offset curve).
/// - `IsLine`: at most [`MAX_SAMPLES`] samples, a non-negative tolerance.
/// - `Expert`: form `0..=2`, stride at most [`MAX_STRIDE`]; a dimension below 2 only
///   when upstream's own checks fail first (it writes past the stride otherwise).
/// - `Polygon`: any finite interval. `Match`: another curve with a NURB.
/// - `Elevate`: at most [`MAX_NEW_DEGREE`]. `SetCanonical`: data as `CreateCanonical`.
/// - `SetCp`: form `0..=2`, finite point and weight.
fn validate_op(op: Op, rd: &mut Reader<'_>, c: &CurveInfo) -> Result<(), FfiError> {
    if !c.has_nurb && !op.handles_empty() {
        return Err(bad("op needs a NURB"));
    }
    match op {
        Op::Intersector | Op::Canonical | Op::C0 | Op::MaxDomain | Op::Unique => {}
        Op::BBox => {
            let (lo, hi) = interval(rd, c)?;
            rd.flag()?;
            rd.flag()?;
            rd.flag()?;
            // (a slack of a few ulps: the clamping upstream handles those)
            let (a, b) = c.range();
            if lo < a - 1e-12 * (1.0 + a.abs()) || hi > b + 1e-12 * (1.0 + b.abs()) {
                return Err(bad("bbox: interval outside the curve range"));
            }
        }
        Op::Analytic | Op::Polygon => {
            rd.finite()?;
            rd.finite()?;
        }
        Op::Offset => {
            let tol = rd.finite()?;
            rd.vec3()?;
            rd.finite()?;
            rd.flag()?;
            if c.pts.len() != c.p + 1 || !c.three_d {
                return Err(bad("offset: needs a 3D single-span curve"));
            }
            if !(1e-6..=1.0).contains(&tol) {
                return Err(bad("offset: tolerance out of range"));
            }
        }
        Op::Repeated => {
            rd.finite()?;
        }
        Op::IsLine => {
            rd.index(MAX_SAMPLES)?;
            if rd.finite()? < 0.0 {
                return Err(bad("is_line: negative tolerance"));
            }
        }
        Op::Degenerate => {
            rd.finite()?;
            rd.flag()?;
            interval(rd, c)?;
        }
        Op::Expert => {
            let form = rd.index(2)?;
            let start = rd.index(MAX_INDEX + 4)?;
            let end = rd.index(MAX_INDEX + 4)?;
            let stride = rd.index(MAX_STRIDE)?;
            let min_stride = c.dim + usize::from(form != 0);
            let passes = start <= end && end < c.pts.len() && stride >= min_stride;
            if passes && c.dim < 2 {
                return Err(bad("expert: dimension below 2 writes past the stride"));
            }
        }
        Op::Match => {
            let other = curve_block(rd)?;
            if !other.has_nurb {
                return Err(bad("match: other curve without NURB"));
            }
        }
        Op::Elevate => {
            rd.index(MAX_NEW_DEGREE)?;
        }
        Op::Reverse => {
            interval(rd, c)?;
        }
        Op::SetCanonical => canonical(rd)?,
        Op::SetCp => {
            rd.index(2)?;
            rd.index(MAX_INDEX + 4)?;
            rd.vec3()?;
            rd.finite()?;
        }
        Op::Adjust => {
            let (lo, hi) = interval(rd, c)?;
            let (a, b) = c.range();
            if hi - lo < 1e-6 * (1.0 + a.abs().max(b.abs())) {
                return Err(bad("adjust: interval too short"));
            }
        }
        Op::Join | Op::Point | Op::ReadDb => return Err(bad("static op")),
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
    if op.is_static() {
        validate_static(op, &mut rd)?;
    } else {
        let this = curve_block(&mut rd)?;
        validate_op(op, &mut rd, &this)?;
    }
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
            sys::sc_nurbs_b_spline_curve_p2_eval(
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
