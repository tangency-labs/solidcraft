//! FFI wrappers for unit `nurbs.b_spline_curve_n_lib`.
//!
//! Shim: `harness/cpp/shim/nurbs_b_spline_curve_n_lib.cpp`
//! (`sc_nurbs_b_spline_curve_n_lib_eval`). Upstream: `source/SMLib/src/SmBSplineCurveNLib.cpp`.
//!
//! Curve ops start with a curve block `dim oob n p m cpts(x y z w) knots` (`this`, built
//! with `SmBSplineCurve(gw_CURVE*, lDim, FALSE)` and `SetOutOfBoundsEnabled(oob)`);
//! static ops do not. A frame is `O[3] X[3] Y[3]` (`SmAxis2Placement::SetCanonical`;
//! its failure status is the op status). A point list is `count` then `3 count`
//! numbers. The op's parameters:
//!
//! | [`Op`]        | upstream                         | parameters                                   |
//! |---------------|----------------------------------|----------------------------------------------|
//! | `Arc`         | `ApproximateArc` (static)        | `dim has_nc nc` frame `r a0 a1 tol`          |
//! | `Rational`    | `ApproximateRationalCurve`       | `tol`                                        |
//! | `Helix`       | `CreateHelixSegment` (static)    | frame `h r0 r1 turns right tol`              |
//! | `ExtParam`    | `CreateExtendedCurve` (parameter)| `t cont precise`                             |
//! | `ExtDist`     | `CreateExtendedCurve` (distance) | `d end cont`                                 |
//! | `InterpCrv`   | `CreateInterpolatingCurve` (static) | `par dim deg` points vectors `has_more` more `ignore` |
//! | `Dampen`      | `DampenKnotSpacing`              | `factor mult`                                |
//! | `DegRed`      | `DegreeReduction`                | `tol max`                                    |
//! | `InsOne`      | `InsertOneKnot`                  | `knot times`                                 |
//! | `Through`     | `SetIntersectionCurveThroughPoint` (no UV curves) | `knot P[3] has_t T[3] pct`  |
//! | `Even`        | `EquallySpacedPoints`            | `t0 t1 n tol`                                |
//! | `InsKnots`    | `InsertKnots`                    | `nk K[nk]`                                   |
//! | `InterpPts`   | `InterpolatePoints` (static)     | points `has_params np U[np] deg has_ts Ts[3] has_te Te[3] closed par` |
//! | `Compat`      | `MakeCurvesCompatible` (static)  | `count` curve blocks, `tol`                  |
//! | `Deform`      | `DeformToEndPointTargets`        | `has_s S[3] has_e E[3] has_st St[3] st_type has_et Et[3] et_type` |
//! | `Reparam`     | `ReparametrizeWithArcLength`     |                                              |
//! | `Scale`       | `Scale(double)`                  | `f`                                          |
//! | `ScaleKnots`  | `ScaleKnotVector`                | `a b`                                        |
//! | `Sync`        | `SyncronizeKnotsOfCurves` (static) | `count` curve blocks, `tol`                |
//! | `Trim`        | `Trim`                           | `a b mode` (`0` TRUE, `1` FALSE, `2` UNSURE) |
//!
//! `cont` is an `SmContinuityType` (`0..=13`), `end` `1` (start) or `2` (end), `par`
//! `0..=2` (`SM_CP_*` / `SM_IT_*`), the tangent types `0..=2` (`SM_IV_*`). The output is
//! the op status (`0` = `SM_SUCCESS`, else the `SmStatus`), the op's outputs, then for
//! curve ops the state of `this`. Ops creating a curve output, on success, a flag and
//! (when set) the new curve's state; `Through` outputs `modified new_knot` on success;
//! `Even` `count`, the points and the parameters; `Trim` the interval after the call
//! (always); `Compat` / `Sync` the state of every curve (always). A state is `dim oob
//! borrowed knot_type form has_nurb [n p m cpts knots]`.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour, crashes or
//! endless loops: well-formed clamped curves (`m = n+p+1`, `1 <= p <= n`, `p <= 17`,
//! finite data, consistent dimension and rationality, positive weights, bounded
//! Euclidean points, non-decreasing knots with end multiplicity exactly `p+1` and
//! interior multiplicity at most `p`), finite bounded parameters, and per op the
//! restrictions documented on [`validate_op`].

use crate::{call_with_buffer, FfiError};

/// Largest control point index accepted by [`eval`].
pub const MAX_INDEX: usize = 2048;
/// Largest NURB degree accepted by [`eval`] (upstream `NL_MAXDEG`).
pub const MAX_DEGREE: usize = 17;
/// Largest magnitude of coordinates and parameters.
pub const MAX_COORD: f64 = 1e6;
/// Largest number of data points / knots / curves in an op.
pub const MAX_COUNT: usize = 256;
/// Largest number of curves of `Compat` / `Sync`.
pub const MAX_CURVES: usize = 8;
/// Largest extension (or out-of-range parameter), relative to the curve size.
pub const MAX_REL_DIST: f64 = 64.0;
/// Smallest `NL_G2` extension by distance, relative to the curve size.
pub const MIN_G2_FRACTION: f64 = 1e-3;
/// Smallest parameter range relative to the knot magnitude.
pub const MIN_REL_RANGE: f64 = 1e-6;
/// Largest number of span splits of `Dampen` (simulated before the call).
pub const MAX_DAMPEN_SPLITS: usize = 4000;
/// Largest number of turns of `Helix`.
pub const MAX_TURNS: f64 = 50.0;
/// Largest `n` of `Even`.
pub const MAX_EVEN: usize = 256;
/// Largest `nc` of `Arc`.
pub const MAX_NC: usize = 400;
/// Smallest tolerance of `Rational` (besides 0).
pub const MIN_RAT_TOL: f64 = 1e-7;
/// Largest degree / index of a `Rational` input curve (as `nlib.crv_approx`).
pub const MAX_RAT_DEGREE: usize = 9;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Arc,
    Rational,
    Helix,
    ExtParam,
    ExtDist,
    InterpCrv,
    Dampen,
    DegRed,
    InsOne,
    Through,
    Even,
    InsKnots,
    InterpPts,
    Compat,
    Deform,
    Reparam,
    Scale,
    ScaleKnots,
    Sync,
    Trim,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::Arc => 0,
            Op::Rational => 1,
            Op::Helix => 2,
            Op::ExtParam => 3,
            Op::ExtDist => 4,
            Op::InterpCrv => 5,
            Op::Dampen => 6,
            Op::DegRed => 7,
            Op::InsOne => 8,
            Op::Through => 9,
            Op::Even => 10,
            Op::InsKnots => 11,
            Op::InterpPts => 12,
            Op::Compat => 13,
            Op::Deform => 14,
            Op::Reparam => 15,
            Op::Scale => 16,
            Op::ScaleKnots => 17,
            Op::Sync => 18,
            Op::Trim => 19,
        }
    }

    /// Whether the op is static (no `this`).
    #[must_use]
    pub fn is_static(self) -> bool {
        matches!(
            self,
            Op::Arc | Op::Helix | Op::InterpCrv | Op::InterpPts | Op::Compat | Op::Sync
        )
    }
}

mod sys {
    extern "C" {
        pub fn sc_nurbs_b_spline_curve_n_lib_eval(
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
    /// A point list: `count`, then `3 count` bounded values.
    fn points(&mut self, max: usize) -> Result<Vec<[f64; 3]>, FfiError> {
        let k = self.index(max)?;
        (0..k).map(|_| self.vec3()).collect()
    }
}

/// A validated curve: Euclidean control points, degree, knots.
#[derive(Debug, Clone)]
struct CurveInfo {
    pts: Vec<[f64; 3]>,
    p: usize,
    knots: Vec<f64>,
    rational: bool,
}

impl CurveInfo {
    fn range(&self) -> (f64, f64) {
        (
            self.knots.first().copied().unwrap_or(0.0),
            self.knots.last().copied().unwrap_or(0.0),
        )
    }

    fn polygon_length(&self) -> f64 {
        self.pts
            .iter()
            .zip(self.pts.iter().skip(1))
            .map(|(&a, &b)| dist(a, b))
            .sum()
    }

    fn pair_ok(&self, i: usize, j: usize) -> bool {
        match (self.pts.get(i), self.pts.get(j)) {
            (Some(&a), Some(&b)) => {
                let d = dist(a, b);
                d > 1e-150 && d < 1e150
            }
            _ => false,
        }
    }

    /// The end-tangent fallbacks of the extensions read the third point from the end.
    fn tangent_ok(&self, start: bool) -> Result<(), FfiError> {
        let n = self.pts.len() - 1;
        if n >= 2 {
            return Ok(());
        }
        let ok = if start {
            self.pair_ok(0, 1)
        } else {
            self.pair_ok(n, n - 1)
        };
        if ok {
            Ok(())
        } else {
            Err(bad(
                "extension: end pair not normalisable and no third point",
            ))
        }
    }

    /// The unique knots.
    fn unique_knots(&self) -> Vec<f64> {
        let mut u: Vec<f64> = Vec::new();
        for &k in &self.knots {
            if u.last().is_none_or(|&l| (l - k).abs() > 1e-12) {
                u.push(k);
            }
        }
        u
    }
}

fn dist([ax, ay, az]: [f64; 3], [bx, by, bz]: [f64; 3]) -> f64 {
    let (x, y, z) = (ax - bx, ay - by, az - bz);
    (x * x + y * y + z * z).sqrt()
}

fn euclid([x, y, z, w]: [f64; 4]) -> [f64; 3] {
    let z = if z == f64::MAX { 0.0 } else { z };
    if w == -f64::MAX {
        [x, y, z]
    } else {
        [x / w, y / w, z / w]
    }
}

/// Reads and validates a curve `n p m cpts knots`.
fn curve(rd: &mut Reader<'_>) -> Result<CurveInfo, FfiError> {
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
        if (z == f64::MAX) != (z0 == f64::MAX) || (w == -f64::MAX) != (w0 == -f64::MAX) {
            return Err(bad("curve: mixed dimension or rationality"));
        }
        if !x.is_finite() || !y.is_finite() || !z.is_finite() || !w.is_finite() {
            return Err(bad("curve: non-finite control point"));
        }
        if w != -f64::MAX && w <= 0.0 {
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
    if b - a < MIN_REL_RANGE * a.abs().max(b.abs()) {
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
        pts,
        p,
        knots: knots.to_vec(),
        rational: w0 != -f64::MAX,
    })
}

/// A curve block `dim oob curve`.
fn curve_block(rd: &mut Reader<'_>) -> Result<CurveInfo, FfiError> {
    rd.index(8)?;
    rd.flag()?;
    curve(rd)
}

/// Data points for the NLib fits: consecutive points distinct.
fn distinct(pts: &[[f64; 3]]) -> Result<(), FfiError> {
    if pts.windows(2).any(|w| matches!(w, [a, b] if a == b)) {
        return Err(bad("points: repeated consecutive point"));
    }
    Ok(())
}

/// `p!/(p-k)!` (upstream's `int` derivative factor in `N_BasisDerivs`) fits an `i32`.
fn check_factor(p: usize, k: usize) -> Result<(), FfiError> {
    let mut f: i64 = 1;
    for r in 0..k.min(p) {
        f *= (p - r) as i64;
        if f > i64::from(i32::MAX) {
            return Err(bad("derivative factor overflows upstream's int"));
        }
    }
    Ok(())
}

/// Validates the knots `x` of a refinement against the curve: inside the range, no
/// knot may reach a multiplicity above the degree.
fn refine_knots(info: &CurveInfo, x: &[f64]) -> Result<(), FfiError> {
    if x.windows(2).any(|w| w.first() > w.last()) {
        return Err(bad("refine: knots not non-decreasing"));
    }
    let (a, b) = info.range();
    let (Some(&first), Some(&last)) = (x.first(), x.last()) else {
        return Ok(());
    };
    if first <= a || last >= b {
        return Ok(()); // Rejected before any indexing.
    }
    for &k in x {
        let mult =
            info.knots.iter().filter(|&&u| u == k).count() + x.iter().filter(|&&u| u == k).count();
        if mult > info.p {
            return Err(bad("refine: multiplicity above the degree"));
        }
    }
    Ok(())
}

/// Simulates `DampenKnotSpacing`'s splitting of the unique knots; `None` when it does
/// not finish within [`MAX_DAMPEN_SPLITS`] splits (upstream loops forever).
fn dampen_splits(unique: &[f64], factor: f64) -> Option<usize> {
    let mut kv = unique.to_vec();
    let mut splits = 0usize;
    let mut done = false;
    while !done {
        done = true;
        let mut i = 0usize;
        while i + 1 < kv.len() {
            let at = |j: usize| kv.get(j).copied().unwrap_or(f64::NAN);
            let span = at(i + 1) - at(i);
            let mut split = false;
            if i > 0 && span / (at(i) - at(i - 1)) > factor {
                split = true;
            }
            if i + 2 < kv.len() && span / (at(i + 2) - at(i + 1)) > factor {
                split = true;
            }
            if split {
                splits += 1;
                if splits > MAX_DAMPEN_SPLITS {
                    return None;
                }
                done = false;
                let mid = (at(i + 1) + at(i)) / 2.0;
                kv.insert(i + 1, mid);
            }
            i += 1;
        }
    }
    Some(splits)
}

const CONT_G1: usize = 3;
const CONT_G1_G2: usize = 5;
const CONT_CINFINITY: usize = 13;

/// Validates one op's parameters (after `this` for curve ops):
///
/// - `Arc`: `nc <= MAX_NC`; with the routine's own checks passed (`r > 0`, distinct
///   angles), a sweep of at least `1e-6` degrees (non-zero end tangents).
/// - `Rational`: a rational input needs degree and index within `nlib.crv_approx`'s
///   limits and `MIN_RAT_TOL <= tol <= 1` (or `tol == 0`).
/// - `Helix`: no input that passes `CreateHelixSegment`'s checks but hits
///   `N_CrvApproxSpiral`'s early input errors (upstream crashes, see
///   `plan/findings.md`); at most `MAX_TURNS` turns, `tol >= 1e-7`.
/// - `ExtParam` / `ExtDist`: bounded, strict extensions; end tangents that can be
///   normalised for `NL_G1`/`NL_G2`; `NL_G2` distances of at least `MIN_G2_FRACTION`
///   curve sizes.
/// - `InterpCrv` / `InterpPts`: `1..=MAX_COUNT` distinct consecutive points, degree at
///   least 1, strictly increasing parameters (one per point), enough points for the
///   local interpolant of a closed fit without tangents.
/// - `Dampen`: a factor whose splitting finishes within `MAX_DAMPEN_SPLITS` splits,
///   `mult <= p`.
/// - `InsKnots`: refinement knots that keep every multiplicity at most the degree.
/// - `Compat` / `Sync`: `1..=MAX_CURVES` curves, `tol >= 0` (`> 0` for `Sync`).
/// - `Trim`: `b - a` at least `1e-6` (relative to the knot magnitude).
/// - `Through`: a parameter within one range of the curve, `0 <= pct <= 0.5`.
fn validate_op(op: Op, rd: &mut Reader<'_>, this: Option<&CurveInfo>) -> Result<(), FfiError> {
    let this_or = || this.ok_or_else(|| bad("op needs a curve"));
    match op {
        Op::Arc => {
            rd.index(8)?;
            let has_nc = rd.flag()?;
            let nc = rd.index(MAX_NC + 1)?;
            if has_nc && nc > MAX_NC {
                return Err(bad("arc: too many control points"));
            }
            rd.vec3()?;
            rd.vec3()?;
            rd.vec3()?;
            let r = rd.finite()?;
            let a0 = rd.finite()?;
            let a1 = rd.finite()?;
            rd.finite()?;
            if r > 0.0 && a0 != a1 {
                let mut ae = a1;
                if (ae - a0 - 90.0).abs() < 1.0e-10 {
                    ae = a0 + 90.0;
                }
                if ae < a0 {
                    ae += 360.0;
                }
                if ae - a0 < 1e-6 {
                    return Err(bad("arc: sweep too small"));
                }
            }
        }
        Op::Rational => {
            let c = this_or()?;
            let tol = rd.finite()?;
            if c.rational {
                if c.p > MAX_RAT_DEGREE || c.pts.len() > 2049 {
                    return Err(bad("rational: curve too large"));
                }
                if tol != 0.0 && !(MIN_RAT_TOL..=1.0).contains(&tol) {
                    return Err(bad("rational: tol out of range"));
                }
            }
        }
        Op::Helix => {
            rd.vec3()?;
            rd.vec3()?;
            rd.vec3()?;
            let h = rd.finite()?;
            let r0 = rd.finite()?;
            let r1 = rd.finite()?;
            let turns = rd.finite()?;
            rd.flag()?;
            let tol = rd.finite()?;
            let passes = h >= -1e-12 && r0 >= 0.0 && r1 >= 0.0 && turns >= 1e-12;
            if passes {
                let mtol = 1.0e-7;
                let q0 = if r0 < mtol { 0.0 } else { r0 };
                let q1 = if r1 < mtol { 0.0 } else { r1 };
                if h < mtol || (q0 + q1) < mtol || turns < mtol {
                    return Err(bad(
                        "helix: N_CrvApproxSpiral early input error crashes upstream",
                    ));
                }
                if turns > MAX_TURNS || tol < 1e-7 {
                    return Err(bad("helix: too many turns or too small a tolerance"));
                }
            }
        }
        Op::ExtParam => {
            let c = this_or()?;
            let t = rd.finite()?;
            let cont = rd.index(13)?;
            let precise = rd.flag()?;
            if [CONT_G1, CONT_G1_G2, CONT_CINFINITY].contains(&cont) {
                let (a, b) = c.range();
                let mut t = t;
                if !precise {
                    if t < a {
                        t -= b - a;
                    } else if t > b {
                        t += b - a;
                    }
                }
                if t < a - MAX_REL_DIST * (b - a) || t > b + MAX_REL_DIST * (b - a) {
                    return Err(bad("extend: parameter too far out"));
                }
                if cont != CONT_CINFINITY {
                    c.tangent_ok(true)?;
                    c.tangent_ok(false)?;
                }
            }
        }
        Op::ExtDist => {
            let c = this_or()?;
            let d = rd.finite()?;
            let end = rd.index(2)?;
            if end == 0 {
                return Err(bad("extend: bad end flag"));
            }
            let cont = rd.index(13)?;
            if [CONT_G1, CONT_G1_G2, CONT_CINFINITY].contains(&cont) {
                let size = c.polygon_length();
                if !(d > 0.0 && d <= MAX_REL_DIST * size) {
                    return Err(bad("extend by distance: distance out of range"));
                }
                if cont == CONT_G1_G2 && d < MIN_G2_FRACTION * size {
                    return Err(bad("extend by distance: G2 extension too short"));
                }
                if cont != CONT_CINFINITY {
                    c.tangent_ok(end == 1)?;
                }
            }
        }
        Op::InterpCrv => {
            rd.index(2)?;
            rd.index(8)?;
            let deg = rd.index(MAX_DEGREE)?;
            let pts = rd.points(MAX_COUNT)?;
            let vecs = rd.points(MAX_COUNT)?;
            rd.flag()?;
            let more = rd.points(8)?;
            rd.flag()?;
            if pts.is_empty() || deg == 0 {
                return Err(bad("interpolate: needs points and a degree"));
            }
            distinct(&pts)?;
            check_factor(deg.max(4), vecs.len().max(more.len()).min(MAX_DEGREE))?;
        }
        Op::Dampen => {
            let c = this_or()?;
            let factor = rd.finite()?;
            let mult = rd.index(MAX_DEGREE)?;
            if mult > c.p {
                return Err(bad("dampen: multiplicity above the degree"));
            }
            if dampen_splits(&c.unique_knots(), factor).is_none() {
                return Err(bad(
                    "dampen: splitting does not finish (upstream loops forever)",
                ));
            }
        }
        Op::DegRed => {
            this_or()?;
            rd.finite()?;
            rd.flag()?;
        }
        Op::InsOne => {
            this_or()?;
            rd.finite()?;
            rd.index(64)?;
        }
        Op::Through => {
            let c = this_or()?;
            let knot = rd.finite()?;
            rd.vec3()?;
            rd.flag()?;
            rd.vec3()?;
            let pct = rd.finite()?;
            let (a, b) = c.range();
            if knot < a - (b - a) || knot > b + (b - a) || !(0.0..=0.5).contains(&pct) {
                return Err(bad("through point: parameter or span percent out of range"));
            }
        }
        Op::Even => {
            this_or()?;
            rd.finite()?;
            rd.finite()?;
            rd.index(MAX_EVEN)?;
            if rd.finite()? < 0.0 {
                return Err(bad("even points: negative tolerance"));
            }
        }
        Op::InsKnots => {
            let c = this_or()?;
            let nk = rd.index(MAX_COUNT)?;
            let mut x = Vec::with_capacity(nk);
            for _ in 0..nk {
                x.push(rd.finite()?);
            }
            refine_knots(c, &x)?;
        }
        Op::InterpPts => {
            let pts = rd.points(MAX_COUNT)?;
            let has_params = rd.flag()?;
            let np = rd.index(MAX_COUNT)?;
            let mut prm = Vec::with_capacity(np);
            for _ in 0..np {
                prm.push(rd.finite()?);
            }
            let deg = rd.index(MAX_DEGREE)?;
            rd.flag()?;
            rd.vec3()?;
            rd.flag()?;
            rd.vec3()?;
            let closed = rd.flag()?;
            rd.index(2)?;
            if pts.is_empty() || deg == 0 {
                return Err(bad("interpolate: needs points and a degree"));
            }
            distinct(&pts)?;
            check_factor(deg, 1)?;
            if has_params && !closed {
                if np != pts.len() {
                    return Err(bad("interpolate: one parameter per point"));
                }
                if prm.windows(2).any(|w| matches!(w, [a, b] if a >= b)) {
                    return Err(bad("interpolate: parameters not increasing"));
                }
            }
            if closed {
                if pts.len() < 2 {
                    return Err(bad("closed interpolation: needs two points"));
                }
                let n = (pts.len() - 1) as i64;
                let pi = deg as i64;
                let nm = n / 2;
                let is = nm.max(n - pi - 1);
                let ie = nm.min(pi + 1);
                let nl = n - is + ie;
                if pi > nl {
                    return Err(bad("closed interpolation: local interpolant degree"));
                }
            }
        }
        Op::Compat | Op::Sync => {
            let cnt = rd.index(MAX_CURVES)?;
            if cnt == 0 {
                return Err(bad("curve set: needs a curve"));
            }
            for _ in 0..cnt {
                curve_block(rd)?;
            }
            let tol = rd.finite()?;
            if tol < 0.0 || (op == Op::Sync && tol == 0.0) {
                return Err(bad("curve set: bad tolerance"));
            }
        }
        Op::Deform => {
            this_or()?;
            for _ in 0..2 {
                rd.flag()?;
                rd.vec3()?;
            }
            for _ in 0..2 {
                rd.flag()?;
                rd.vec3()?;
                rd.index(2)?;
            }
        }
        Op::Reparam => {
            this_or()?;
        }
        Op::Scale => {
            this_or()?;
            rd.finite()?;
        }
        Op::ScaleKnots => {
            this_or()?;
            rd.finite()?;
            rd.finite()?;
        }
        Op::Trim => {
            let c = this_or()?;
            let x = rd.finite()?;
            let y = rd.finite()?;
            rd.index(2)?;
            let (lo, hi) = (x.min(y), x.max(y));
            let (a, b) = c.range();
            if hi - lo < 1e-6 * (1.0 + a.abs().max(b.abs())) {
                return Err(bad("trim: interval too short"));
            }
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
    let this = if op.is_static() {
        None
    } else {
        Some(curve_block(&mut rd)?)
    };
    validate_op(op, &mut rd, this.as_ref())?;
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
            sys::sc_nurbs_b_spline_curve_n_lib_eval(
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
