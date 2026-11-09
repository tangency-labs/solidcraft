//! Parameters of points on an [`EvalSurface`], on the evaluator's own surface form: a
//! surface's parameter box and periods, the parameters of a point on it (closed form on a plane,
//! damped Gauss–Newton elsewhere), unwrapping across a period, and its poles.
//!
//! A spline that closes on itself along a direction (its first and last control rows coincide,
//! weights included, and its knots are clamped there: a NURBS cylinder, a fillet ring round a
//! hole's rim) is periodic along it, one active knot interval per period: the record stays as it
//! is, and parameters outside the interval are evaluated a whole number of periods back inside
//! it ([`EvalSurface::derivs`]). So is a revolution along a profile closed on itself (a torus's
//! whole circle).

use super::EvalSurface;
use crate::vec::VecExt as _;
use solidcore_record::Vec3;

/// The parameter box of a surface and its periods.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Domain {
    /// Bounded ranges (`None`: unbounded, as a plane's).
    pub range: [Option<(f64, f64)>; 2],
    pub period: [Option<f64>; 2],
}

/// The parameter box and periods of `s`.
pub fn domain(s: &EvalSurface) -> Domain {
    let tau = std::f64::consts::TAU;
    match s {
        EvalSurface::Plane { .. } => Domain {
            range: [None, None],
            period: [None, None],
        },
        EvalSurface::Nurbs {
            degree,
            knots,
            count,
            closed: shut,
            ..
        } => {
            let r = |d: usize| Some((*knots[d].get(degree[d])?, *knots[d].get(count[d])?));
            let range = [r(0), r(1)];
            let period = [0, 1].map(|d| {
                range[d]
                    .filter(|_| shut[d])
                    .map(|(a, b)| b - a)
                    .filter(|p| *p > 0.0)
            });
            Domain { range, period }
        }
        EvalSurface::Revolved { profile, swap, .. } => {
            let r = (|| {
                Some((
                    *profile.knots.get(profile.degree)?,
                    *profile.knots.get(profile.ctrl.len())?,
                ))
            })();
            // A profile closed on itself (a whole circle: a torus as another kernel writes it)
            // is periodic along the profile too.
            let along = r
                .filter(|_| closed_curve(profile))
                .map(|(a, b)| b - a)
                .filter(|p| *p > 0.0);
            let (a, b) = ((r, along), (Some((0.0, tau)), Some(tau)));
            let (u, v) = if *swap { (b, a) } else { (a, b) };
            Domain {
                range: [u.0, v.0],
                period: [u.1, v.1],
            }
        }
    }
}

/// Relative closeness of a spline's first and last control rows that makes it closed: control
/// points within this fraction of the control net's extent, weights within this fraction of
/// their size. Rows written as one (a producer's exact copy) pass; a surface whose ends merely
/// come near each other does not.
const CLOSED: f64 = 1e-9;

/// Whether a spline curve closes on itself: its knots clamped at both ends and its first and
/// last control points the same, weights included ([`closed`] for a curve).
pub fn closed_curve(c: &super::EvalCurve) -> bool {
    let Some(extent) = net_extent(&c.ctrl) else {
        return false;
    };
    c.ctrl.len() >= 2
        && clamped(c.degree, c.ctrl.len(), &c.knots)
        && same_point(c.ctrl.first(), c.ctrl.last(), extent)
}

/// Whether the tensor spline closes on itself along each direction: its knots clamped at both
/// ends of that direction (so its boundary rows are its edges) and its first and last control
/// rows the same points with the same weights. Control point `(i, j)` is `ctrl[j·count[0] + i]`.
pub fn closed(
    degree: [usize; 2],
    knots: &[Vec<f64>; 2],
    count: [usize; 2],
    ctrl: &[[f64; 4]],
) -> [bool; 2] {
    let [nu, nv] = count;
    if nu.checked_mul(nv) != Some(ctrl.len()) || nu < 2 || nv < 2 {
        return [false; 2];
    }
    let Some(extent) = net_extent(ctrl) else {
        return [false; 2];
    };
    let same = |a, b| same_point(ctrl.get(a), ctrl.get(b), extent);
    let u = clamped(degree[0], nu, &knots[0]) && (0..nv).all(|j| same(j * nu, j * nu + nu - 1));
    let v = clamped(degree[1], nv, &knots[1]) && (0..nu).all(|i| same(i, (nv - 1) * nu + i));
    [u, v]
}

fn cartesian(c: &[f64; 4]) -> Option<Vec3> {
    (c[3] != 0.0).then(|| Vec3::new(c[0] / c[3], c[1] / c[3], c[2] / c[3]))
}

/// The diagonal of a control net's box; `None` when a weight vanishes or the net is a point.
fn net_extent(ctrl: &[[f64; 4]]) -> Option<f64> {
    let (mut lo, mut hi) = (
        Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
        Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
    );
    for c in ctrl {
        let p = cartesian(c)?;
        lo = Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
        hi = Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
    }
    let extent = hi.dist(lo);
    (extent.is_finite() && extent > 0.0).then_some(extent)
}

/// Two control points the same within [`CLOSED`] of the net's extent, weights included.
fn same_point(a: Option<&[f64; 4]>, b: Option<&[f64; 4]>, extent: f64) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => {
            matches!((cartesian(a), cartesian(b)), (Some(p), Some(q)) if p.dist(q) <= CLOSED * extent)
                && (a[3] - b[3]).abs() <= CLOSED * a[3].abs().max(b[3].abs())
        }
        _ => false,
    }
}

/// Knots of degree `p` over `n` control points, clamped at both ends: the spline starts at its
/// first control point and ends at its last, its interval `[k[p], k[n]]` ending on knots of
/// multiplicity `p` within it. The outermost knots (`k[0]`, `k[n + p]`) take no part in the
/// spline over its interval, so they need not repeat: a periodic quadratic circle written with
/// knot multiplicities `1, 2, …, 2, 1` (as OpenCascade writes one) is clamped.
fn clamped(p: usize, n: usize, k: &[f64]) -> bool {
    let ends = |r: std::ops::RangeInclusive<usize>| {
        k.get(r)
            .is_some_and(|s| s.windows(2).all(|w| w.first() == w.get(1)))
    };
    p >= 1 && k.len() == n + p + 1 && ends(1..=p) && ends(n..=n + p - 1)
}

/// How far past an end of the interval, relative to its size, a parameter is still that end:
/// rounding, not a period away.
const WRAP_SLACK: f64 = 1e-12;

/// `x` moved by whole periods `b − a` into `[a, b]` when it lies outside. A parameter a rounding
/// past an end (a pcurve ending on the seam one ulp beyond it) is that end, not moved a period
/// to the other one: a plan's node there would put its triangles across the whole period.
pub(crate) fn wrap_into(x: f64, (a, b): (f64, f64)) -> f64 {
    let period = b - a;
    if (a..=b).contains(&x) || !(period > 0.0) {
        return x;
    }
    let slack = WRAP_SLACK * period.max(a.abs()).max(b.abs());
    if x > b && x - b <= slack {
        return b;
    }
    if x < a && a - x <= slack {
        return a;
    }
    let w = x - ((x - a) / period).floor() * period;
    w.clamp(a, b)
}

/// `x` moved by whole periods to lie within half a period of `near`.
pub fn unwrap(x: f64, near: f64, period: Option<f64>) -> f64 {
    match period {
        Some(p) if p > 0.0 => x - ((x - near) / p).round() * p,
        _ => x,
    }
}

/// [`degenerate`], or within `reach` mm of a pole (see [`collapsed`]): a boundary point on a
/// patch side collapsed to within a tolerance, not to a point, where its parameter along `d` is
/// as free as at a pole.
pub fn degenerate_within(
    s: &EvalSurface,
    dom: &Domain,
    uv: [f64; 2],
    d: usize,
    reach: f64,
) -> bool {
    let Some((_, su, sv)) = s.derivs(uv[0], uv[1]) else {
        return false;
    };
    let speed = [su.len(), sv.len()];
    collapsed(speed, dom, reach)
        .get(d)
        .copied()
        .unwrap_or(false)
}

/// Along each direction, whether a point whose parameter speeds (mm per unit) are `speed` is a
/// pole, or within `reach` mm of one: moving along the direction there not at all (to 1e-9 of
/// the other's speed), or across half its period (or its whole range) by no more than `reach`.
pub fn collapsed(speed: [f64; 2], dom: &Domain, reach: f64) -> [bool; 2] {
    let [a, b] = speed;
    let mut out = [a <= 1e-9 * b.max(1e-12), b <= 1e-9 * a.max(1e-12)];
    for (d, flag) in out.iter_mut().enumerate() {
        let span = match (dom.period[d], dom.range[d]) {
            (Some(period), _) => 0.5 * period,
            (None, Some((lo, hi))) => (hi - lo).abs(),
            (None, None) => continue,
        };
        *flag |= speed[d] * span <= reach;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fast::EvalCurve;

    fn cylinder() -> EvalSurface {
        EvalSurface::Revolved {
            profile: EvalCurve {
                degree: 1,
                knots: vec![0.0, 0.0, 1.0, 1.0],
                ctrl: vec![[3.0, 0.0, 0.0, 1.0], [3.0, 0.0, 4.0, 1.0]],
            },
            origin: Vec3::ZERO,
            axis: Vec3::Z,
            transform: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            swap: false,
        }
    }

    /// A revolution of a whole circle (a torus, the profile a rational quadratic closed on
    /// itself) is periodic along the profile too, over its knot interval; a cylinder's line
    /// profile is not.
    #[test]
    fn a_revolution_of_a_closed_profile_is_periodic_along_it() {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        // The circle of radius 1 about (3, 0, 0) in the xz plane, four quarter arcs.
        let pts = [
            [4.0, 0.0],
            [4.0, 1.0],
            [3.0, 1.0],
            [2.0, 1.0],
            [2.0, 0.0],
            [2.0, -1.0],
            [3.0, -1.0],
            [4.0, -1.0],
            [4.0, 0.0],
        ];
        let ctrl: Vec<[f64; 4]> = pts
            .iter()
            .enumerate()
            .map(|(i, [x, z])| {
                let wt = if i % 2 == 1 { w } else { 1.0 };
                [x * wt, 0.0, z * wt, wt]
            })
            .collect();
        let knots = vec![
            0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0,
        ];
        for swap in [false, true] {
            let s = EvalSurface::Revolved {
                profile: EvalCurve {
                    degree: 2,
                    knots: knots.clone(),
                    ctrl: ctrl.clone(),
                },
                origin: Vec3::ZERO,
                axis: Vec3::Z,
                transform: [
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                ],
                swap,
            };
            let (along, around) = if swap { (1, 0) } else { (0, 1) };
            let dom = domain(&s);
            assert_eq!(
                (dom.period[along], dom.period[around]),
                (Some(1.0), Some(std::f64::consts::TAU)),
                "swap {swap}"
            );
            let at = |t: f64, a: f64| if swap { [a, t] } else { [t, a] };
            let wrapped = s.wrap(at(1.3, 0.4));
            assert!(
                (wrapped[along] - 0.3).abs() < 1e-12 && wrapped[around] == 0.4,
                "swap {swap}: {wrapped:?}"
            );
            assert_eq!(
                s.wrap(at(0.3, 7.0)),
                at(0.3, 7.0),
                "the angle is left alone"
            );
            let (Some(p), Some(q)) = (
                s.derivs(at(1.3, 0.4)[0], at(1.3, 0.4)[1]),
                s.derivs(at(0.3, 0.4)[0], at(0.3, 0.4)[1]),
            ) else {
                panic!("evaluates")
            };
            assert!(p.0.dist(q.0) < 1e-12, "swap {swap}: {p:?} {q:?}");
        }
        assert_eq!(
            domain(&cylinder()).period,
            [None, Some(std::f64::consts::TAU)]
        );
    }

    #[test]
    fn unwraps_across_the_period() {
        let tau = std::f64::consts::TAU;
        assert!((unwrap(0.1, tau - 0.1, Some(tau)) - (tau + 0.1)).abs() < 1e-12);
        assert_eq!(unwrap(0.1, 6.0, None), 0.1);
    }
}
