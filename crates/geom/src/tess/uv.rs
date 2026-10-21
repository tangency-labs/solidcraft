//! Parameters of points on an [`EvalSurface`], on the evaluator's own surface form: a
//! surface's parameter box and periods, the parameters of a point on it (closed form on a plane,
//! damped Gauss–Newton elsewhere), unwrapping across a period, and its poles.

use super::EvalSurface;
use crate::Vec3;

/// The parameter box of a surface and its periods.
#[derive(Clone, Copy, Debug)]
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
        EvalSurface::Nurbs { degree, knots, count, .. } => {
            let r = |d: usize| Some((*knots[d].get(degree[d])?, *knots[d].get(count[d])?));
            Domain { range: [r(0), r(1)], period: [None, None] }
        }
        EvalSurface::Revolved { profile, swap, .. } => {
            let r = (|| Some((*profile.knots.get(profile.degree)?, *profile.knots.get(profile.ctrl.len())?)))();
            let (a, b) = ((r, None), (Some((0.0, tau)), Some(tau)));
            let (u, v) = if *swap { (b, a) } else { (a, b) };
            Domain { range: [u.0, v.0], period: [u.1, v.1] }
        }
    }
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

/// Parameters of `p` (a point on the surface within `accept`), from `hint` when that
/// converges, else from the best starts of a grid over the surface's domain.
pub fn project(s: &EvalSurface, dom: &Domain, p: Vec3, hint: Option<[f64; 2]>, accept: f64) -> Option<[f64; 2]> {
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
    let (ur, vr) = (dom.range[0]?, dom.range[1]?);
    const N: usize = 16;
    let mut starts: Vec<(f64, [f64; 2])> = Vec::with_capacity((N + 1) * (N + 1));
    for i in 0..=N {
        for j in 0..=N {
            let uv = [ur.0 + (ur.1 - ur.0) * i as f64 / N as f64, vr.0 + (vr.1 - vr.0) * j as f64 / N as f64];
            if let Some((q, _, _)) = s.derivs(uv[0], uv[1]) {
                starts.push(((q - p).len2(), uv));
            }
        }
    }
    starts.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, start) in starts.iter().take(4) {
        if let Some((uv, d)) = newton(s, dom, p, *start)
            && best.is_none_or(|(_, bd)| d < bd)
        {
            best = Some((uv, d));
        }
    }
    best.filter(|(_, d)| *d <= accept).map(|(uv, _)| uv)
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

    #[test]
    fn unwraps_across_the_period() {
        let tau = std::f64::consts::TAU;
        assert!((unwrap(0.1, tau - 0.1, Some(tau)) - (tau + 0.1)).abs() < 1e-12);
        assert_eq!(unwrap(0.1, 6.0, None), 0.1);
    }
}
