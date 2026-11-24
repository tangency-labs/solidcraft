//! Certified checks of the curve/curve contacts that `BSplineCurve::trim_gap` closes a
//! gap at (tests only). The intersection is `trim_gap`'s own step
//! (`BSplineCurve::trim_gap_intersection`), and the paths are certified by the library's
//! `curves_stay_within` (a Lipschitz bound from the curves' own data). No acceptance here
//! is decided by samples.

use solidcore::curve::curve::p2::Curve;
use solidcore::curve::curve::p8::curves_stay_within;
use solidcore::geom::solution_array::SolutionType;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, GapIntersection as Step};

/// The intersection `trim_gap` runs: its two extended curves, the tolerance and the
/// contacts of its solutions.
pub(crate) struct GapIntersection {
    /// The first curve, extended past the end at the gap.
    pub(crate) c0: Curve,
    /// The second curve, extended past the end at the gap.
    pub(crate) c1: Curve,
    /// `trim_gap`'s intersection tolerance.
    pub(crate) tol: f64,
    /// The contacts `[t0, t1]` of the solutions in record order: each point, and both
    /// ends of each range.
    pub(crate) contacts: Vec<[f64; 2]>,
}

/// The intersection `trim_gap(this, other, _, d_min)` runs
/// (`BSplineCurve::trim_gap_intersection`, the step `trim_gap` itself calls); `None`
/// where `trim_gap` returns before using an intersection (no gap, gap too large, a
/// failed step, an error).
pub(crate) fn gap_intersection(
    k: &Kernel,
    this: &BSplineCurve,
    other: &BSplineCurve,
    d_min: f64,
) -> Option<GapIntersection> {
    let Step::Intersected(g) = this.trim_gap_intersection(k, other, d_min).ok()? else {
        return None;
    };
    let pair = |e: &solidcore::geom::solution_array::SolutionEnd| {
        [e.param(0).unwrap(), e.param(1).unwrap()]
    };
    let contacts = g
        .solutions
        .iter()
        .flat_map(|(ty, start, end)| {
            let range = *ty == SolutionType::RangeOfValues;
            std::iter::once(pair(start)).chain(range.then(|| pair(end)))
        })
        .collect();
    let [c0, c1] = g.curves;
    Some(GapIntersection {
        c0: Curve::from(c0),
        c1: Curve::from(c1),
        tol: g.tolerance,
        contacts,
    })
}

/// Certifies that the gap intersection `g` changed only by naming contacts at the
/// curves' ends: every point and range end now reported is a contact (both parameters
/// in their domains and the curve points within the tolerance, plus a rounding
/// margin; a range's interior is not certified here), and every contact reported
/// before (`old`) is still reported or joined to one now reported by a path over which
/// the curves stay within the tolerance (`curves_stay_within`). Returns whether the
/// contacts changed.
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
        // a path of no length: the node's own certified distance bound
        assert!(
            curves_stay_within(k, &g.c0, &g.c1, [t0, t1], [t0, t1], g.tol).unwrap(),
            "{name}: ({t0}, {t1}) not certified within {}",
            g.tol
        );
    }
    for &s in old {
        let joined = now.contains(&s)
            || now
                .iter()
                .any(|&e| curves_stay_within(k, &g.c0, &g.c1, s, e, g.tol).unwrap());
        assert!(joined, "{name}: old contact {s:?} joins none of {now:?}");
    }
    now.as_slice() != old
}
