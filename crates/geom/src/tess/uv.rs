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
use crate::Vec3;

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
        EvalSurface::Plane { .. } => Domain { range: [None, None], period: [None, None] },
        EvalSurface::Nurbs { degree, knots, count, closed: shut, .. } => {
            let r = |d: usize| Some((*knots[d].get(degree[d])?, *knots[d].get(count[d])?));
            let range = [r(0), r(1)];
            let period = [0, 1].map(|d| range[d].filter(|_| shut[d]).map(|(a, b)| b - a).filter(|p| *p > 0.0));
            Domain { range, period }
        }
        EvalSurface::Revolved { profile, swap, .. } => {
            let r = (|| Some((*profile.knots.get(profile.degree)?, *profile.knots.get(profile.ctrl.len())?)))();
            // A profile closed on itself (a whole circle: a torus as another kernel writes it)
            // is periodic along the profile too.
            let along = r.filter(|_| closed_curve(profile)).map(|(a, b)| b - a).filter(|p| *p > 0.0);
            let (a, b) = ((r, along), (Some((0.0, tau)), Some(tau)));
            let (u, v) = if *swap { (b, a) } else { (a, b) };
            Domain { range: [u.0, v.0], period: [u.1, v.1] }
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
    let Some(extent) = net_extent(&c.ctrl) else { return false };
    c.ctrl.len() >= 2 && clamped(c.degree, c.ctrl.len(), &c.knots) && same_point(c.ctrl.first(), c.ctrl.last(), extent)
}

/// Whether the tensor spline closes on itself along each direction: its knots clamped at both
/// ends of that direction (so its boundary rows are its edges) and its first and last control
/// rows the same points with the same weights. Control point `(i, j)` is `ctrl[j·count[0] + i]`.
pub fn closed(degree: [usize; 2], knots: &[Vec<f64>; 2], count: [usize; 2], ctrl: &[[f64; 4]]) -> [bool; 2] {
    let [nu, nv] = count;
    if nu.checked_mul(nv) != Some(ctrl.len()) || nu < 2 || nv < 2 {
        return [false; 2];
    }
    let Some(extent) = net_extent(ctrl) else { return [false; 2] };
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
    let (mut lo, mut hi) =
        (Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY));
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

/// Knots of degree `p` over `n` control points, clamped at both ends.
fn clamped(p: usize, n: usize, k: &[f64]) -> bool {
    let ends = |r: std::ops::RangeInclusive<usize>| k.get(r).is_some_and(|s| s.windows(2).all(|w| w.first() == w.get(1)));
    k.len() == n + p + 1 && ends(0..=p) && ends(n..=n + p)
}

/// `x` moved by whole periods `b − a` into `[a, b]` when it lies outside.
pub(crate) fn wrap_into(x: f64, (a, b): (f64, f64)) -> f64 {
    let period = b - a;
    if (a..=b).contains(&x) || !(period > 0.0) {
        return x;
    }
    let w = x - ((x - a) / period).floor() * period;
    w.clamp(a, b)
}

impl Domain {
    pub fn clamp(&self, uv: [f64; 2]) -> [f64; 2] {
        let mut out = uv;
        for (d, x) in out.iter_mut().enumerate() {
            if let (Some((a, b)), None) = (self.range[d], self.period[d]) {
                *x = x.clamp(a.min(b), a.max(b));
            }
        }
        out
    }
}

/// Damped Gauss–Newton (Levenberg–Marquardt) for the parameters nearest to `p`, from `start`.
/// Returns the parameters and their distance to `p`.
fn newton(s: &EvalSurface, dom: &Domain, p: Vec3, start: [f64; 2]) -> Option<([f64; 2], f64)> {
    let mut uv = dom.clamp(start);
    let (mut q, mut su, mut sv) = s.derivs(uv[0], uv[1])?;
    let mut dist = q.dist(p);
    let mut lambda = 1e-6;
    for _ in 0..60 {
        let r = p - q;
        let (a, b, c) = (su.len2(), su.dot(sv), sv.len2());
        let (g0, g1) = (su.dot(r), sv.dot(r));
        let scale = a + c;
        if !(scale > 0.0) || !scale.is_finite() {
            break;
        }
        let (a2, c2) = (a + lambda * scale, c + lambda * scale);
        let det = a2 * c2 - b * b;
        if !(det.abs() > 0.0) {
            break;
        }
        let step = [(c2 * g0 - b * g1) / det, (a2 * g1 - b * g0) / det];
        let next = dom.clamp([uv[0] + step[0], uv[1] + step[1]]);
        let Some((nq, nsu, nsv)) = s.derivs(next[0], next[1]) else { break };
        let nd = nq.dist(p);
        if nd <= dist {
            let moved = (next[0] - uv[0]).abs() + (next[1] - uv[1]).abs();
            (uv, q, su, sv, dist) = (next, nq, nsu, nsv, nd);
            lambda = (lambda * 0.1).max(1e-12);
            if moved <= 1e-15 * (1.0 + uv[0].abs() + uv[1].abs()) {
                break;
            }
        } else {
            lambda *= 10.0;
            if lambda > 1e8 {
                break;
            }
        }
    }
    Some((uv, dist))
}

/// The grid of surface points [`project`] searches from when it has no hint that converges: up to
/// [`MAX_STARTS`] evaluations of the surface, the same for every point, so a caller projecting
/// many points onto one surface (a face's pcurves, its boundary) keeps one `Starts` for it and
/// the grid is evaluated once, on first use ([`project_with`]).
#[derive(Clone, Debug, Default)]
pub struct Starts(std::sync::OnceLock<Vec<(Vec3, [f64; 2])>>);

/// A cache of the surface's own points: any two are alike.
impl PartialEq for Starts {
    fn eq(&self, _: &Starts) -> bool {
        true
    }
}

impl Starts {
    /// The grid over `dom` ([`starts_along`] in each direction, thinned to [`MAX_STARTS`]):
    /// each start's point and parameters. Empty where a direction is unbounded.
    fn of(&self, s: &EvalSurface, dom: &Domain) -> &[(Vec3, [f64; 2])] {
        self.0.get_or_init(|| {
            let (Some(ru), Some(rv)) = (dom.range[0], dom.range[1]) else { return Vec::new() };
            let (mut us, mut vs) = (starts_along(s, 0, ru), starts_along(s, 1, rv));
            // Thin the longer list (every other start, its ends kept) until the grid fits.
            while us.len().saturating_mul(vs.len()) > MAX_STARTS {
                let longer = if us.len() >= vs.len() { &mut us } else { &mut vs };
                let last = longer.last().copied();
                *longer = longer.iter().step_by(2).copied().chain(last).collect();
                longer.dedup();
            }
            let mut grid = Vec::with_capacity(us.len() * vs.len());
            for &u in &us {
                for &v in &vs {
                    if let Some((q, _, _)) = s.derivs(u, v) {
                        grid.push((q, [u, v]));
                    }
                }
            }
            grid
        })
    }
}

/// Parameters of `p` (a point on the surface within `accept`), from `hint` when that
/// converges, else from the best starts of a grid over the surface's domain, as fine as its
/// knot spans ([`starts_along`]).
pub fn project(s: &EvalSurface, dom: &Domain, p: Vec3, hint: Option<[f64; 2]>, accept: f64) -> Option<[f64; 2]> {
    project_with(s, dom, &Starts::default(), p, hint, accept)
}

/// [`project`] with the grid kept in `starts`, which must have been used with this surface and
/// domain only.
pub fn project_with(s: &EvalSurface, dom: &Domain, starts: &Starts, p: Vec3, hint: Option<[f64; 2]>, accept: f64) -> Option<[f64; 2]> {
    if let EvalSurface::Plane { origin, du, dv } = s {
        let r = p - *origin;
        let (a, b, c) = (du.len2(), du.dot(*dv), dv.len2());
        let det = a * c - b * b;
        if !(det.abs() > 0.0) {
            return None;
        }
        let (g0, g1) = (du.dot(r), dv.dot(r));
        let uv = [(c * g0 - b * g1) / det, (a * g1 - b * g0) / det];
        return (uv[0].is_finite() && uv[1].is_finite()).then_some(uv);
    }
    let mut best: Option<([f64; 2], f64)> = None;
    if let Some(h) = hint
        && let Some((uv, d)) = newton(s, dom, p, h)
    {
        if d <= accept {
            return Some(uv);
        }
        best = Some((uv, d));
    }
    if dom.range.iter().any(Option::is_none) {
        return None;
    }
    let grid = starts.of(s, dom);
    // The nearest starts first (in grid order where equally near), until one converges within
    // `accept`: where turns of a surface pass close to each other (a thread's root band a tenth
    // of a millimetre from the next turn's), the nearest few can all lie on the wrong turn.
    let mut near: Vec<(f64, usize)> = grid.iter().enumerate().map(|(i, (q, _))| ((*q - p).len2(), i)).collect();
    let by_distance = |a: &(f64, usize), b: &(f64, usize)| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1));
    if near.len() > NEWTON_STARTS {
        near.select_nth_unstable_by(NEWTON_STARTS, by_distance);
        near.truncate(NEWTON_STARTS);
    }
    near.sort_by(by_distance);
    for start in near.iter().filter_map(|(_, i)| grid.get(*i).map(|(_, uv)| uv)) {
        if let Some((uv, d)) = newton(s, dom, p, *start)
            && best.is_none_or(|(_, bd)| d < bd)
        {
            best = Some((uv, d));
        }
        if accept.is_finite() && best.is_some_and(|(_, d)| d <= accept) {
            break;
        }
    }
    best.filter(|(_, d)| *d <= accept).map(|(uv, _)| uv)
}

/// Evenly spaced starts along each direction of [`project`]'s search of the whole surface.
const EVEN_STARTS: usize = 16;
/// Starts per knot span of a direction where the surface has its own knots.
const SPAN_STARTS: usize = 2;
/// Most points of the surface the search measures.
const MAX_STARTS: usize = 1 << 14;
/// Most of them it runs Gauss–Newton from.
const NEWTON_STARTS: usize = 32;

/// The parameters along direction `d` (over `range`) that [`project`] starts from when it has no
/// hint: even steps across the range, and where the surface has its own knots along `d` (a
/// spline's, a revolution's profile's) each span's start and points inside it, so a surface that
/// winds several turns along `d` (a thread's flank, one turn every few spans) is searched on
/// every turn and a point is never taken to the turn beside its own.
fn starts_along(s: &EvalSurface, d: usize, (a, b): (f64, f64)) -> Vec<f64> {
    let knots = match s {
        EvalSurface::Plane { .. } => None,
        EvalSurface::Nurbs { knots, .. } => knots.get(d),
        EvalSurface::Revolved { profile, swap, .. } => (usize::from(*swap) == d).then_some(&profile.knots),
    };
    let (lo, hi) = (a.min(b), a.max(b));
    let mut out: Vec<f64> = (0..=EVEN_STARTS).map(|k| a + (b - a) * k as f64 / EVEN_STARTS as f64).collect();
    if let Some(knots) = knots {
        let inside = knots.iter().copied().filter(|k| *k > lo && *k < hi);
        let mut ends: Vec<f64> = std::iter::once(lo).chain(inside).chain(std::iter::once(hi)).collect();
        ends.dedup();
        for w in ends.windows(2).take(MAX_STARTS) {
            let (Some(&k0), Some(&k1)) = (w.first(), w.get(1)) else { continue };
            out.extend((0..SPAN_STARTS).map(|j| k0 + (k1 - k0) * (j as f64 + 0.5) / SPAN_STARTS as f64));
            out.push(k0);
        }
    }
    out.retain(|x| x.is_finite());
    out.sort_by(f64::total_cmp);
    out.dedup();
    out
}

/// `x` moved by whole periods to lie within half a period of `near`.
pub fn unwrap(x: f64, near: f64, period: Option<f64>) -> f64 {
    match period {
        Some(p) if p > 0.0 => x - ((x - near) / p).round() * p,
        _ => x,
    }
}

/// Is the surface degenerate along direction `d` at `uv` (a pole: moving along `d` does not
/// move the point)?
pub fn degenerate(s: &EvalSurface, uv: [f64; 2], d: usize) -> bool {
    let Some((_, su, sv)) = s.derivs(uv[0], uv[1]) else { return false };
    let (this, other) = if d == 0 { (su, sv) } else { (sv, su) };
    this.len() <= 1e-9 * other.len().max(1e-12)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tess::EvalCurve;

    fn cylinder() -> EvalSurface {
        EvalSurface::Revolved {
            profile: EvalCurve { degree: 1, knots: vec![0.0, 0.0, 1.0, 1.0], ctrl: vec![[3.0, 0.0, 0.0, 1.0], [3.0, 0.0, 4.0, 1.0]] },
            origin: Vec3::ZERO,
            axis: Vec3::Z,
            transform: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]],
            swap: false,
        }
    }

    #[test]
    fn projects_points_back_to_their_parameters() {
        let s = cylinder();
        let dom = domain(&s);
        for &(u, v) in &[(0.1, 0.2), (0.5, 3.0), (0.9, 6.0), (0.0, 1.0)] {
            let (p, _) = s.eval(u, v);
            let Some(uv) = project(&s, &dom, p, None, 1e-9) else { panic!("projects") };
            let (q, _) = s.eval(uv[0], uv[1]);
            assert!(q.dist(p) < 1e-9);
            assert!((uv[0] - u).abs() < 1e-9);
        }
        // Off the surface by more than `accept`: no parameters.
        assert!(project(&s, &dom, Vec3::new(5.0, 0.0, 1.0), None, 1e-3).is_none());
    }

    /// A quadratic spline round a square in u (first and last columns the same), straight
    /// up in v, `last` moving its last u column.
    fn ring(last: [f64; 4], u_knots: Vec<f64>) -> EvalSurface {
        let around =
            [[2.0, 0.0, 0.0, 1.0], [2.0, 2.0, 0.0, 1.0], [-2.0, 2.0, 0.0, 1.0], [-2.0, -2.0, 0.0, 1.0], [2.0, -2.0, 0.0, 1.0], [2.0, 0.0, 0.0, 1.0]];
        let mut ctrl = Vec::new();
        for z in [0.0, 3.0] {
            for (i, c) in around.iter().enumerate() {
                let c = if i == around.len() - 1 { [c[0] + last[0], c[1] + last[1], c[2] + last[2], c[3] + last[3]] } else { *c };
                ctrl.push([c[0] * c[3], c[1] * c[3], (c[2] + z) * c[3], c[3]]);
            }
        }
        EvalSurface::nurbs([2, 1], [u_knots, vec![0.0, 0.0, 1.0, 1.0]], [6, 2], ctrl)
    }

    #[test]
    fn a_spline_closed_on_itself_is_periodic_over_its_knot_interval() {
        let clamped = || vec![0.0, 0.0, 0.0, 0.25, 0.5, 0.75, 1.0, 1.0, 1.0];
        let s = ring([0.0; 4], clamped());
        let EvalSurface::Nurbs { degree, knots, count, ctrl, closed: kept } = &s else { panic!("a spline") };
        assert_eq!(closed(*degree, knots, *count, ctrl), [true, false]);
        assert_eq!(*kept, [true, false], "the constructor keeps what the test finds");
        assert_eq!(domain(&s).period, [Some(1.0), None]);
        // A period away is the same point; inside the interval nothing moves.
        for (u, inside) in [(1.25, 0.25), (-0.25, 0.75), (3.6, 0.6)] {
            let (Some(a), Some(b)) = (s.derivs(u, 0.4), s.derivs(inside, 0.4)) else { panic!("evaluates") };
            assert!(a.0.dist(b.0) < 1e-12 && a.1.dist(b.1) < 1e-12, "{u}: {a:?} {b:?}");
        }
        assert_eq!(s.wrap([1.0, 0.5]), [1.0, 0.5]);
        assert_eq!(s.wrap([1.25, 2.0]), [0.25, 2.0]);
        // Projection finds a point near the seam from either side.
        let p = s.derivs(0.02, 0.5).map(|d| d.0).unwrap_or(Vec3::ZERO);
        let dom = domain(&s);
        let Some(q) = project(&s, &dom, p, Some([0.98, 0.5]), 1e-9) else { panic!("projects") };
        assert!((unwrap(q[0], 0.98, dom.period[0]) - 1.02).abs() < 1e-9, "{q:?}");
        // Ends that only come near each other, a weight that differs, or unclamped knots: open,
        // and evaluated past the interval as before (the end span continued).
        for (what, open) in [
            ("ends apart", ring([0.0, 1e-6, 0.0, 0.0], clamped())),
            ("weights differ", ring([0.0, 0.0, 0.0, 1e-6], clamped())),
            ("unclamped", ring([0.0; 4], vec![-0.5, -0.25, 0.0, 0.25, 0.5, 0.75, 1.0, 1.25, 1.5])),
        ] {
            assert_eq!(domain(&open).period, [None, None], "{what}");
            assert_eq!(open.wrap([1.25, 0.5]), [1.25, 0.5], "{what}");
        }
    }

    /// A revolution of a whole circle (a torus, the profile a rational quadratic closed on
    /// itself) is periodic along the profile too, over its knot interval; a cylinder's line
    /// profile is not.
    #[test]
    fn a_revolution_of_a_closed_profile_is_periodic_along_it() {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        // The circle of radius 1 about (3, 0, 0) in the xz plane, four quarter arcs.
        let pts = [[4.0, 0.0], [4.0, 1.0], [3.0, 1.0], [2.0, 1.0], [2.0, 0.0], [2.0, -1.0], [3.0, -1.0], [4.0, -1.0], [4.0, 0.0]];
        let ctrl: Vec<[f64; 4]> = pts
            .iter()
            .enumerate()
            .map(|(i, [x, z])| {
                let wt = if i % 2 == 1 { w } else { 1.0 };
                [x * wt, 0.0, z * wt, wt]
            })
            .collect();
        let knots = vec![0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0];
        for swap in [false, true] {
            let s = EvalSurface::Revolved {
                profile: EvalCurve { degree: 2, knots: knots.clone(), ctrl: ctrl.clone() },
                origin: Vec3::ZERO,
                axis: Vec3::Z,
                transform: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]],
                swap,
            };
            let (along, around) = if swap { (1, 0) } else { (0, 1) };
            let dom = domain(&s);
            assert_eq!((dom.period[along], dom.period[around]), (Some(1.0), Some(std::f64::consts::TAU)), "swap {swap}");
            let at = |t: f64, a: f64| if swap { [a, t] } else { [t, a] };
            let wrapped = s.wrap(at(1.3, 0.4));
            assert!((wrapped[along] - 0.3).abs() < 1e-12 && wrapped[around] == 0.4, "swap {swap}: {wrapped:?}");
            assert_eq!(s.wrap(at(0.3, 7.0)), at(0.3, 7.0), "the angle is left alone");
            let (Some(p), Some(q)) = (s.derivs(at(1.3, 0.4)[0], at(1.3, 0.4)[1]), s.derivs(at(0.3, 0.4)[0], at(0.3, 0.4)[1])) else {
                panic!("evaluates")
            };
            assert!(p.0.dist(q.0) < 1e-12, "swap {swap}: {p:?} {q:?}");
        }
        assert_eq!(domain(&cylinder()).period, [None, Some(std::f64::consts::TAU)]);
    }

    #[test]
    fn unwraps_across_the_period() {
        let tau = std::f64::consts::TAU;
        assert!((unwrap(0.1, tau - 0.1, Some(tau)) - (tau + 0.1)).abs() < 1e-12);
        assert_eq!(unwrap(0.1, 6.0, None), 0.1);
    }
}
