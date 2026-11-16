//! Certified checks of the curve/curve contacts that `BSplineCurve::trim_gap` closes a
//! gap at (tests only): speed bounds read from a B-spline's own control points, and a
//! Lipschitz test that two curves stay within a tolerance along a parameter path. No
//! acceptance here is decided by samples.

use solidcore::curve::curve::p2::Curve;
use solidcore::curve::curve::p8::CurveIntersectOptions;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::geom_utility::p2::ContinuityType;
use solidcore::geom::solution_array::{ObjectKind, SolutionEnd, SolutionObject, SolutionType};
use solidcore::kernel::Kernel;
use solidcore::math::{min2, EFF_ZERO};
use solidcore::nurbs::b_spline_curve::BSplineCurve;

/// The gap curve handle `trim_gap` names its solutions with (index and natural
/// interval), so the intersection merges and refines exactly as inside `trim_gap`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct GapCurve(u8, Extent1d);

impl SolutionObject for GapCurve {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Curve
    }

    fn natural_interval(&self) -> Option<Extent1d> {
        Some(self.1)
    }
}

/// The intersection `trim_gap` runs: its two extended curves, the tolerance and the
/// contacts of its solutions.
pub(crate) struct GapIntersection {
    /// The first curve, extended past the end at the gap.
    pub(crate) c0: Curve,
    /// The second curve, extended past the end at the gap.
    pub(crate) c1: Curve,
    /// Speed bounds of `c0` and `c1` over their whole domains.
    pub(crate) speed: [f64; 2],
    /// `trim_gap`'s intersection tolerance (five times the gap).
    pub(crate) tol: f64,
    /// The contacts `[t0, t1]` of the solutions in record order: each point, and both
    /// ends of each range.
    pub(crate) contacts: Vec<[f64; 2]>,
}

/// The intersection `trim_gap(this, other, _, d_min)` runs, replayed step for step from
/// `trim_gap`'s own code (closest ends, 3% G1 extensions, `5 * gap` tolerance); `None`
/// where `trim_gap` returns before intersecting.
pub(crate) fn gap_intersection(
    k: &Kernel,
    this: &BSplineCurve,
    other: &BSplineCurve,
    d_min: f64,
) -> Option<GapIntersection> {
    let extent0 = this.natural_interval();
    let t0size = extent0.max() - extent0.min();
    let s0 = this.evaluate_point(extent0.min()).ok()?;
    let e0 = this.evaluate_point(extent0.max()).ok()?;
    let extent1 = other.natural_interval();
    let t1size = extent1.max() - extent1.min();
    let s1 = other.evaluate_point(extent1.min()).ok()?;
    let e1 = other.evaluate_point(extent1.max()).ok()?;
    let d_s0s1 = (s0 - s1).length_squared();
    let d_s0e1 = (s0 - e1).length_squared();
    let d_e0s1 = (e0 - s1).length_squared();
    let d_e0e1 = (e0 - e1).length_squared();
    let dmin = min2(min2(d_s0s1, d_s0e1), min2(d_e0s1, d_e0e1));
    if dmin > d_min * d_min || dmin < EFF_ZERO {
        return None;
    }
    let ext_case = if dmin >= d_s0s1 {
        1
    } else if dmin >= d_s0e1 {
        2
    } else if dmin >= d_e0s1 {
        3
    } else {
        4
    };
    let at_start0 = ext_case == 1 || ext_case == 2;
    let at_start1 = ext_case == 1 || ext_case == 3;
    let extend = |c: &BSplineCurve, t: f64| {
        c.create_extended_curve(t, ContinuityType::G1, true)
            .ok()
            .flatten()
    };
    let c0 = extend(
        this,
        if at_start0 {
            extent0.min() - 0.03 * t0size
        } else {
            extent0.max() + 0.03 * t0size
        },
    )?;
    let c1 = extend(
        other,
        if at_start1 {
            extent1.min() - 0.03 * t1size
        } else {
            extent1.max() + 0.03 * t1size
        },
    )?;
    let speed = [speed_bound(&c0), speed_bound(&c1)];
    let (c0, c1) = (Curve::from(c0), Curve::from(c1));
    let tol = dmin * 5.0;
    let opts = CurveIntersectOptions {
        skip_global_coincidence_check: false,
        handles: [
            Some(GapCurve(0, c0.natural_interval())),
            Some(GapCurve(1, c1.natural_interval())),
        ],
        analytic_handles: [
            Some(GapCurve(2, c0.natural_interval())),
            Some(GapCurve(3, c1.natural_interval())),
        ],
    };
    let sols = c0
        .global_curve_intersect(
            k,
            &c0.natural_interval(),
            &c1,
            &c1.natural_interval(),
            tol,
            &opts,
        )
        .ok()?;
    let pair = |e: &SolutionEnd| [e.param(0).unwrap(), e.param(1).unwrap()];
    let contacts = sols
        .as_slice()
        .iter()
        .flat_map(|s| {
            let range = s.solution_type == SolutionType::RangeOfValues;
            std::iter::once(pair(&s.start)).chain(range.then(|| pair(&s.end)))
        })
        .collect();
    Some(GapIntersection {
        c0,
        c1,
        speed,
        tol,
        contacts,
    })
}

/// An upper bound on the speed `|c'(t)|` of a clamped B-spline over its whole domain,
/// from its control points (no sampling):
///
/// * polynomial: `c'` is the B-spline of degree `p - 1` with control points
///   `p (P[i+1] - P[i]) / (u[i+p+1] - u[i+1])` (nonzero spans), so the largest bounds it;
/// * rational (positive weights): `|c'| <= 2 p (w_max / w_min) D / min_span`, `D` the
///   control polygon's diameter.
///
/// Widened by `1e-9` relative plus `1e-12` as a rounding margin (not a derived bound).
pub(crate) fn speed_bound(c: &BSplineCurve) -> f64 {
    let p = c.degree().unwrap();
    let (unique, mult) = c.knots(None).unwrap();
    let u: Vec<f64> = unique
        .iter()
        .zip(&mult)
        .flat_map(|(&k, &m)| std::iter::repeat_n(k, m))
        .collect();
    let (pts, weights) = c.control_polygon().unwrap();
    assert_eq!(u.len(), pts.len() + p + 1, "a clamped knot vector");
    let spans: Vec<(usize, f64)> = (0..pts.len() - 1)
        .map(|i| (i, u[i + p + 1] - u[i + 1]))
        .filter(|&(_, d)| d > 0.0)
        .collect();
    let pf = p as f64;
    let b = if weights.is_empty() {
        spans
            .iter()
            .map(|&(i, d)| pf * pts[i + 1].distance(pts[i]) / d)
            .fold(0.0, f64::max)
    } else {
        assert!(weights.iter().all(|&w| w > 0.0), "positive weights");
        let w_min = weights.iter().copied().fold(f64::MAX, f64::min);
        let w_max = weights.iter().copied().fold(0.0, f64::max);
        let diam = pts
            .iter()
            .flat_map(|a| pts.iter().map(move |b| a.distance(*b)))
            .fold(0.0, f64::max);
        let min_span = spans.iter().map(|s| s.1).fold(f64::MAX, f64::min);
        2.0 * pf * (w_max / w_min) * diam / min_span
    };
    b * (1.0 + 1e-9) + 1e-12
}

/// Whether `a(t0)` and `b(t1)` stay within `tol` of each other along the whole straight
/// parameter path from `s` to `e`, with `speed` bounding `|a'|` and `|b'|`. Certified:
/// with `f` the distance along the path and `lip` its Lipschitz bound, `f` on a step of
/// length `h` between grid values `f0` and `f1` is at most `(f0 + f1) / 2 + lip h / 2`;
/// the grid refines until every step's bound, plus a rounding margin of `1e-9` of the
/// coordinates, is within `tol`.
pub(crate) fn path_within(k: &Kernel, g: &GapIntersection, s: [f64; 2], e: [f64; 2]) -> bool {
    let lip = g.speed[0] * (e[0] - s[0]).abs() + g.speed[1] * (e[1] - s[1]).abs();
    let at = |l: f64| {
        let pa = g.c0.evaluate_point(k, s[0] + l * (e[0] - s[0])).unwrap();
        let pb = g.c1.evaluate_point(k, s[1] + l * (e[1] - s[1])).unwrap();
        (pa.distance(pb), 1e-9 * (1.0 + pa.length().max(pb.length())))
    };
    let mut n = 16_usize;
    while n <= 1 << 22 {
        let h = 1.0 / n as f64;
        let mut prev = at(0.0);
        let ok = (1..=n).all(|i| {
            let cur = at(i as f64 * h);
            let bound = 0.5 * (prev.0 + cur.0) + 0.5 * lip * h + prev.1.max(cur.1);
            prev = cur;
            bound <= g.tol
        });
        if ok {
            return true;
        }
        n *= 4;
    }
    false
}

/// Certifies that the gap intersection `g` changed only by naming contacts at the
/// curves' ends: every point and range end now reported is a contact (both parameters
/// in their domains and the curve points within the tolerance, plus a rounding
/// margin; a range's interior is not certified here), and every contact reported
/// before (`old`) is still reported or joined to one now reported by a path over which
/// the curves stay within the tolerance. Returns whether the contacts changed.
pub(crate) fn certify_gap_move(
    k: &Kernel,
    name: &str,
    g: &GapIntersection,
    old: &[[f64; 2]],
) -> bool {
    let (i0, i1) = (g.c0.natural_interval(), g.c1.natural_interval());
    let now = &g.contacts;
    for &[t0, t1] in now {
        assert!(
            i0.min() <= t0 && t0 <= i0.max() && i1.min() <= t1 && t1 <= i1.max(),
            "{name}: ({t0}, {t1}) outside the domains"
        );
        let pa = g.c0.evaluate_point(k, t0).unwrap();
        let pb = g.c1.evaluate_point(k, t1).unwrap();
        let margin = 1e-9 * (1.0 + pa.length().max(pb.length()));
        let d = pa.distance(pb);
        assert!(
            d + margin <= g.tol,
            "{name}: ({t0}, {t1}) apart {d} > {}",
            g.tol
        );
    }
    for &s in old {
        let joined = now.contains(&s) || now.iter().any(|&e| path_within(k, g, s, e));
        assert!(joined, "{name}: old contact {s:?} joins none of {now:?}");
    }
    now.as_slice() != old
}
