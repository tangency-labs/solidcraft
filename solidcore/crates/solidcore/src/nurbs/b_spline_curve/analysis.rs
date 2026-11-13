//! Shortcuts for `SmBSplineCurve::AssertValid`'s two global property analyses (not
//! upstream, tangency-labs/solidcore#2568): the zero-speed search (tests 11/12,
//! `MINIMIZE_FIRST_DERIVATIVE`) and the kink search (tests 13/14,
//! `MINIMIZE_RADIUS_OF_CURVATURE`). Neither changes a result.
//!
//! - [`curve_bounds`]: lower bounds on a curve's speed and radius of curvature over its
//!   whole natural interval, from its Bezier segments' control points. Where a bound
//!   clears the test's tolerance, the analysis cannot report and is not run.
//! - [`AnalysisMemo`]: the analyses' results kept in the kernel by the curve's exact
//!   content, so an unchanged curve checked again is not solved again.
//!
//! Both are off when `Kernel::memoize_curve_caches` is off (the `curve-memo-off`
//! feature), which runs every analysis in full, to check that they change nothing.
//!
//! **The bounds.** Take a Bezier segment of degree `p` on `[a, b]` with homogeneous
//! control points `(H_i, w_i)`, every weight positive. Shifting the origin to its first
//! point `c0` gives `Q_i = H_i - w_i c0`, and with `t` the segment's own parameter the
//! curve is `C = c0 + Q / w`. Its derivative is `C' = N / w^2` with
//! `N = Q' w - Q w'`, a polynomial of degree `2p - 1` in Bernstein form (the product
//! of the Bernstein forms of degrees `p - 1` and `p`). Then:
//! - **speed:** where every coefficient of `N` lies on the positive side of a plane
//!   through the origin with unit normal `n` (`N_k . n >= m > 0`), `|C'| >= m / w_max^2`
//!   on the whole segment: no zero speed there. In the curve's own parameter it is
//!   divided by `b - a`.
//! - **radius of curvature:** `rho = |C'|^3 / |C' x C''| >= |C'|^2 / |C''|`, and
//!   `C'' = (N' w - 2 N w') / w^3`, so
//!   `|C''| <= (max |N'_k| w_max + 2 max |N_k| max |w'_i|) / w_min^3`. `rho` is the same
//!   in every parameter, so the segment's own `t` is used.
//! - **an evaluable radius:** the radius search's objective fails to evaluate where
//!   `|C'|` or `|C' x C''|` is zero to `SM_EFF_ZERO`, and a failed evaluation leaves the
//!   value 0, which the search can return as a solution (a straight piece reported as
//!   a cusp). So the radius bound counts only where no evaluation can fail: with
//!   `C' x C'' = (N x N') / w^4`, `N x N'` again in Bernstein form (degree `4p - 3`) and
//!   bounded from below by a plane as the speed, both bounds clear `SM_EFF_ZERO` by a
//!   margin far above the rounding of an evaluation ([`CurveBounds::evaluable`]).
//!
//! The bounds are sufficient, never necessary: a segment whose `N` crosses every
//! candidate plane (a turn of more than half a circle, a cusp, zero speed) gives no
//! bound, and the analysis runs as before. The curve's bound is the least of its
//! segments'. Bezier control points come from `N_CrvDecomposeBez`, so they are the
//! curve's own exactly where no knot is inserted and to rounding elsewhere; the callers
//! compare with a margin ([`BOUND_MARGIN`]) far above that.
//!
//! Deviations: not upstream. No recursion; the loops run over the segments and their
//! control points.

use std::collections::{BTreeMap, VecDeque};

use crate::geom::solution_array::SolutionEnd;
use crate::geom::vector3d::Vec3;
use crate::math::EFF_ZERO;
use crate::nurbs::b_spline_curve::p1::BSplineCurve;
use crate::nurbs::crv_basic::Curve;
use crate::nurbs::math::{CPoint, NOW, NOZ};

/// How far a bound must clear a test's tolerance before its analysis is skipped
/// (relative): far above the rounding of the Bezier decomposition and of the bound.
pub(crate) const BOUND_MARGIN: f64 = 1.0 + 1e-6;

/// How much of a computed derivative's (or cross product's) size its rounding can be,
/// at most, for [`CurveBounds::evaluable`]: far above the evaluation's actual rounding.
const EVAL_ROUNDING: f64 = 1e-10;

/// Lower bounds on a curve's speed (in its own parameter) and radius of curvature over
/// its natural interval ([`curve_bounds`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CurveBounds {
    /// `|C'|` is at least this everywhere.
    pub(crate) speed: f64,
    /// The radius of curvature is at least this everywhere (`f64::INFINITY` for a
    /// straight curve).
    pub(crate) radius: f64,
    /// Whether the radius search's objective (`|C'|^3 / |C' x C''|`) evaluates
    /// everywhere: `|C'|` and `|C' x C''|` are bounded away from `SM_EFF_ZERO`, so no
    /// evaluation fails and leaves a value 0. Only then does [`CurveBounds::radius`]
    /// bound what the search returns.
    pub(crate) evaluable: bool,
}

impl CurveBounds {
    /// Whether the radius search over the curve cannot return a value below `tol`.
    pub(crate) fn radius_clears(&self, tol: f64) -> bool {
        self.evaluable && self.radius >= tol * BOUND_MARGIN
    }
}

/// The bounds of [`CurveBounds`] for `cur`, or `None` where they cannot be given: a
/// weight that is not positive, a degree 0 segment, a segment whose derivative's
/// coefficients surround the origin, or a failed decomposition.
pub(crate) fn curve_bounds(cur: &Curve) -> Option<CurveBounds> {
    let segments = cur.decompose_bez().ok()?;
    let mut bounds = CurveBounds {
        speed: f64::INFINITY,
        radius: f64::INFINITY,
        evaluable: true,
    };
    if segments.is_empty() {
        return None;
    }
    for segment in &segments {
        let b = segment_bounds(segment)?;
        bounds.speed = bounds.speed.min(b.speed);
        bounds.radius = bounds.radius.min(b.radius);
        bounds.evaluable &= b.evaluable;
    }
    Some(bounds)
}

/// A control point's homogeneous coordinates and weight (`NOZ` is a 2D point, `NOW` a
/// non-rational one, whose coordinates are its point's).
fn homogeneous(c: CPoint) -> (Vec3, f64) {
    let z = if c.z == NOZ { 0.0 } else { c.z };
    if c.w == NOW {
        (Vec3::new(c.x, c.y, z), 1.0)
    } else {
        (Vec3::new(c.x, c.y, z), c.w)
    }
}

/// `n` choose `k` as a double (`0` for `k > n`).
fn binomial(n: usize, k: usize) -> f64 {
    if k > n {
        return 0.0;
    }
    let k = k.min(n - k);
    let mut r = 1.0;
    for i in 0..k {
        r = r * (n - i) as f64 / (i + 1) as f64;
    }
    r
}

/// Whether `v` is a positive, finite number.
fn positive(v: f64) -> bool {
    v > 0.0 && v.is_finite()
}

/// `f` of each consecutive pair of `items`.
fn pairs<T: Copy, R>(items: &[T], f: impl Fn(T, T) -> R) -> Vec<R> {
    items
        .windows(2)
        .filter_map(|s| match s {
            [a, b] => Some(f(*a, *b)),
            _ => None,
        })
        .collect()
}

/// The least of `c . n` over the coefficients `c`, for the best unit `n` among the
/// first, the last and the sum of the coefficients; not positive where none separates
/// them from the origin.
fn plane_bound(coefficients: &[Vec3]) -> f64 {
    let mut sum = Vec3::ZERO;
    for &c in coefficients {
        sum += c;
    }
    let ends = [coefficients.first(), coefficients.last()];
    let mut least = 0.0_f64;
    for cand in ends.into_iter().flatten().copied().chain([sum]) {
        let len = cand.length();
        if !positive(len) {
            continue;
        }
        let unit = cand / len;
        let m = coefficients
            .iter()
            .map(|c| c.dot(unit))
            .fold(f64::INFINITY, f64::min);
        least = least.max(m);
    }
    least
}

/// The Bernstein coefficients (degree `a.len() + b.len() - 2`) of `f(A, B)` for the
/// polynomials with coefficients `a` and `b`, `f` bilinear (a product or a cross
/// product); `None` for an empty input.
fn bernstein_product(a: &[Vec3], b: &[Vec3], f: impl Fn(Vec3, Vec3) -> Vec3) -> Option<Vec<Vec3>> {
    let da = a.len().checked_sub(1)?;
    let db = b.len().checked_sub(1)?;
    let mut out = vec![Vec3::ZERO; da + db + 1];
    for (i, &x) in a.iter().enumerate() {
        for (j, &y) in b.iter().enumerate() {
            let coef = binomial(da, i) * binomial(db, j) / binomial(da + db, i + j);
            let slot = out.get_mut(i + j)?;
            *slot += f(x, y) * coef;
        }
    }
    Some(out)
}

/// The bounds of one Bezier segment (see the module docs).
fn segment_bounds(segment: &Curve) -> Option<CurveBounds> {
    let p = segment.degree();
    let knots = segment.knots();
    let span = knots.last()? - knots.first()?;
    if p == 0 || !positive(span) {
        return None;
    }
    let points: Vec<(Vec3, f64)> = segment.cpts().iter().map(|&c| homogeneous(c)).collect();
    if points.len() != p + 1 || points.iter().any(|&(_, w)| !positive(w)) {
        return None;
    }
    let (h0, w0) = *points.first()?;
    let c0 = h0 / w0;
    let q: Vec<Vec3> = points.iter().map(|&(h, w)| h - c0 * w).collect();
    let w: Vec<f64> = points.iter().map(|&(_, w)| w).collect();
    let pf = p as f64;
    let dq: Vec<Vec3> = pairs(&q, |a, b| (b - a) * pf);
    let dw: Vec<f64> = pairs(&w, |a, b| (b - a) * pf);

    // N = Q' w - Q w', degree 2p - 1
    let deg_n = 2 * p - 1;
    let mut n_coef = vec![Vec3::ZERO; deg_n + 1];
    for (i, (&a, &b)) in dq.iter().zip(&dw).enumerate() {
        for (j, (&qj, &wj)) in q.iter().zip(&w).enumerate() {
            let coef = binomial(p - 1, i) * binomial(p, j) / binomial(deg_n, i + j);
            let slot = n_coef.get_mut(i + j)?;
            *slot += (a * wj - qj * b) * coef;
        }
    }

    // speed: a plane through the origin with every coefficient on its positive side
    let least = plane_bound(&n_coef);
    if !positive(least) {
        return None;
    }
    let w_min = w.iter().copied().fold(f64::INFINITY, f64::min);
    let w_max = w.iter().copied().fold(0.0_f64, f64::max);
    let speed_t = least / (w_max * w_max);

    // radius: |C''| <= (max |N'| w_max + 2 max |N| max |w'|) / w_min^3
    let n_max = n_coef.iter().map(|c| c.length()).fold(0.0_f64, f64::max);
    let dn: Vec<Vec3> = pairs(&n_coef, |a, b| (b - a) * deg_n as f64);
    let dn_max = dn.iter().map(|c| c.length()).fold(0.0_f64, f64::max);
    let dw_max = dw.iter().map(|d| d.abs()).fold(0.0_f64, f64::max);
    let accel = (dn_max * w_max + 2.0 * n_max * dw_max) / (w_min * w_min * w_min);
    let radius = if accel > 0.0 {
        speed_t * speed_t / accel
    } else {
        f64::INFINITY
    };

    // evaluable: |C'| and |C' x C''| = |N x N'| / w^4 clear SM_EFF_ZERO by more than
    // an evaluation's rounding, in the curve's own parameter
    let cross = bernstein_product(&n_coef, &dn, Vec3::cross)?;
    let cross_least = plane_bound(&cross) / (w_max * w_max * w_max * w_max);
    let speed_max = n_max / (w_min * w_min) / span;
    let accel_max = accel / (span * span);
    let speed = speed_t / span;
    let evaluable = speed > 2.0 * EFF_ZERO + EVAL_ROUNDING * speed_max
        && cross_least / (span * span * span)
            > 2.0 * EFF_ZERO + EVAL_ROUNDING * speed_max * accel_max;

    let bounds = CurveBounds {
        speed,
        radius,
        evaluable,
    };
    (positive(bounds.speed) && !bounds.radius.is_nan()).then_some(bounds)
}

/// The most results an [`AnalysisMemo`] keeps; the oldest goes first.
const MEMO_CAPACITY: usize = 4096;

/// The largest curve (control points) an [`AnalysisMemo`] keeps a result for.
const MEMO_MAX_POINTS: usize = 4096;

/// A global property analysis's result: whether it returned `SM_SUCCESS`, and its
/// solutions' start ends.
pub(crate) type AnalysisResult = (bool, Vec<SolutionEnd>);

/// The results of `AssertValid`'s global property analyses, by the exact content of
/// what they depend on ([`analysis_key`]): an unchanged curve checked again gets the
/// result it got before. The key is the content bit for bit, never an address, so a
/// hit gives exactly what the analysis would compute; at most [`MEMO_CAPACITY`]
/// results are kept, the oldest dropped first.
#[derive(Debug, Default)]
pub struct AnalysisMemo {
    results: BTreeMap<Vec<u64>, AnalysisResult>,
    order: VecDeque<Vec<u64>>,
}

impl AnalysisMemo {
    /// The result kept for `key`.
    pub(crate) fn get(&self, key: &[u64]) -> Option<AnalysisResult> {
        self.results.get(key).cloned()
    }

    /// Keeps `result` for `key`, dropping the oldest result past [`MEMO_CAPACITY`].
    pub(crate) fn insert(&mut self, key: Vec<u64>, result: AnalysisResult) {
        if self.results.contains_key(&key) {
            return;
        }
        while self.order.len() >= MEMO_CAPACITY {
            match self.order.pop_front() {
                Some(old) => {
                    self.results.remove(&old);
                }
                None => break,
            }
        }
        self.order.push_back(key.clone());
        self.results.insert(key, result);
    }
}

/// The key of an analysis of `curve` over `[lo, hi]` for `property` with minimum
/// separation `min_sep`, under `settings` (the kernel's tolerances and fixes, as the
/// caller encodes them): every input of the analysis, bit for bit. `None` for a curve
/// past [`MEMO_MAX_POINTS`] or without a NURB.
pub(crate) fn analysis_key(
    curve: &BSplineCurve,
    lo: f64,
    hi: f64,
    property: u64,
    min_sep: f64,
    settings: &[u64],
) -> Option<Vec<u64>> {
    let cur = curve.nurb.as_ref()?;
    if cur.cpts().len() > MEMO_MAX_POINTS {
        return None;
    }
    let mut key = vec![
        property,
        lo.to_bits(),
        hi.to_bits(),
        min_sep.to_bits(),
        curve.dim,
        u64::from(curve.out_of_bounds_enabled),
        u64::try_from(cur.degree()).ok()?,
        u64::try_from(cur.cpts().len()).ok()?,
        u64::try_from(cur.knots().len()).ok()?,
    ];
    for c in cur.cpts() {
        key.extend([c.x.to_bits(), c.y.to_bits(), c.z.to_bits(), c.w.to_bits()]);
    }
    key.extend(cur.knots().iter().map(|v| v.to_bits()));
    // (form and knot type by their debug names, as bytes)
    for b in format!("{:?}/{:?}", curve.form, curve.knot_type).bytes() {
        key.push(u64::from(b));
    }
    key.extend_from_slice(settings);
    Some(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nurbs::math::CPoint;

    fn curve(degree: usize, points: &[CPoint], knots: &[f64]) -> Curve {
        Curve::new(points.to_vec(), degree, knots.to_vec()).unwrap()
    }

    /// A quarter circle of radius 2 (rational quadratic, weights 1, sqrt(1/2), 1): speed
    /// and radius bounded, the radius bound at most the true radius 2 (sound), and the
    /// radius search's objective evaluable everywhere.
    ///
    /// The bound is loose (about 0.19 here, a tenth of the radius): it divides the least
    /// speed by the greatest `|C''|` over the span, each taken from control-point hulls
    /// and weight extremes at different places, so it only clears tolerances well below
    /// the radius. The check's radius tolerances are four orders of magnitude below the
    /// curve's size, so that is enough to skip the search.
    #[test]
    fn a_quarter_circle_is_bounded() {
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let c = curve(
            2,
            &[
                CPoint::new(2.0, 0.0, 0.0, 1.0),
                CPoint::new(2.0 * s, 2.0 * s, 0.0, s),
                CPoint::new(0.0, 2.0, 0.0, 1.0),
            ],
            &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        );
        let b = curve_bounds(&c).unwrap();
        assert!(
            b.speed > 0.0 && b.radius > 0.0 && b.radius <= 2.0 + 1e-12,
            "{b:?}"
        );
        assert!(
            b.evaluable && b.radius_clears(0.1) && !b.radius_clears(2.0),
            "{b:?}"
        );
    }

    /// A quadratic that turns back on itself (its first and last control points
    /// coincide, the middle one off them): zero speed inside; no bound.
    #[test]
    fn a_turn_back_has_no_bound() {
        let c = curve(
            2,
            &[
                CPoint::new(0.0, 0.0, 0.0, NOW),
                CPoint::new(1.0, 0.0, 0.0, NOW),
                CPoint::new(0.0, 0.0, 0.0, NOW),
            ],
            &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        );
        assert_eq!(curve_bounds(&c), None);
    }

    /// A straight non-rational segment: speed its length, radius infinite.
    #[test]
    fn a_line_is_straight() {
        let c = curve(
            1,
            &[
                CPoint::new(0.0, 0.0, 0.0, NOW),
                CPoint::new(3.0, 4.0, 0.0, NOW),
            ],
            &[0.0, 0.0, 2.0, 2.0],
        );
        let b = curve_bounds(&c).unwrap();
        assert!((b.speed - 2.5).abs() < 1e-12, "{b:?}");
        assert_eq!(b.radius, f64::INFINITY);
        assert!(!b.evaluable && !b.radius_clears(1e-3), "{b:?}");
    }

    /// The first span of the mapped UV trim of #2568's case: a straight 2D cubic with unit
    /// weights and unevenly spaced control points. Its radius is infinite, but the radius
    /// search's objective fails on it (`C' x C''` is zero) and the search returns 0, so
    /// the bound must not clear any tolerance.
    #[test]
    fn a_straight_cubic_does_not_clear_the_radius_test() {
        let y = 2.8284271247461916;
        let h = 0.033986983156039985;
        let c = curve(
            3,
            &[
                CPoint::new(5.560586550410673e-15, y, NOZ, 1.0),
                CPoint::new(0.38469966347659956, y, NOZ, 1.0),
                CPoint::new(0.7712034706794334, y, NOZ, 1.0),
                CPoint::new(1.1595370454427245, y, NOZ, 1.0),
            ],
            &[0.0, 0.0, 0.0, 0.0, h, h, h, h],
        );
        let b = curve_bounds(&c).unwrap();
        assert!(b.speed > 0.0 && b.radius > 1.0, "{b:?}");
        assert!(!b.evaluable && !b.radius_clears(3.46e-3), "{b:?}");
    }

    #[test]
    fn the_memo_keeps_the_newest() {
        let mut memo = AnalysisMemo::default();
        for i in 0..(MEMO_CAPACITY as u64 + 2) {
            memo.insert(vec![i], (true, Vec::new()));
        }
        assert!(memo.get(&[0]).is_none() && memo.get(&[1]).is_none());
        assert!(memo.get(&[2]).is_some());
        assert_eq!(memo.results.len(), MEMO_CAPACITY);
    }
}
