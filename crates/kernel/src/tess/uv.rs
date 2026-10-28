//! Parameters of points on a surface, on the evaluator's own surface form (so the parameters
//! the plan stores are the ones the evaluators use), and the tolerance-driven divisions of a
//! face's parameter box.

use solidcraft_geom::tess::EvalSurface;
pub(crate) use solidcraft_geom::tess::uv::{Domain, degenerate_within, domain, unwrap};

use super::size::Rules;

/// Division of `range` along direction `d` so that between neighbouring lines the surface sags
/// at most the tolerance from its chord, the chord is at most `h` long and the normal turns
/// within the rules' facet angle, checked along iso-lines across `across`; at most `max_lines`
/// lines, ends included.
pub(crate) fn divide(s: &EvalSurface, d: usize, range: (f64, f64), across: (f64, f64), rules: &Rules, h: f64, max_lines: usize) -> Vec<f64> {
    let (a0, a1) = range;
    if !(a1 > a0) {
        return vec![a0, a1];
    }
    let flat = limits(s, d, across, rules, h);
    // Start from quarter turns at most, so a closed iso-line is not taken for a flat one (a
    // plane's parameters are lengths and its lines are never closed).
    let initial = if matches!(s, EvalSurface::Plane { .. }) { 1 } else { ((a1 - a0) / std::f64::consts::FRAC_PI_2).ceil().clamp(1.0, 64.0) as u32 };
    // Both ends exactly (a0 + (a1 − a0) · 1 may round away from a1): the end lines are the
    // face's own parameter box, which the boundary nodes sit on.
    let starts: Vec<f64> = (0..=initial)
        .map(|i| {
            let t = f64::from(i) / f64::from(initial);
            a0 * (1.0 - t) + a1 * t
        })
        .collect();
    split(&starts, &flat, max_lines)
}

/// `lines` (increasing, ends included) with every interval that fails the same limits as
/// [`divide`] split until it passes: lines taken from a face's edge nodes meet the edges node for
/// node, but nothing else checks how far the surface turns between them inside the face.
pub(crate) fn refine(s: &EvalSurface, d: usize, lines: &[f64], across: (f64, f64), rules: &Rules, h: f64, max_lines: usize) -> Vec<f64> {
    let flat = limits(s, d, across, rules, h);
    split(lines, &flat, max_lines)
}

/// Depth-first bisection of the intervals between `starts` until `flat` holds for each.
fn split(starts: &[f64], flat: &dyn Fn(f64, f64) -> bool, max_lines: usize) -> Vec<f64> {
    let Some(&first) = starts.first() else { return Vec::new() };
    let mut out = vec![first];
    let mut stack: Vec<(f64, f64, u32)> = starts.windows(2).rev().map(|w| (w[0], w[1], 0)).collect();
    while let Some((a, b, depth)) = stack.pop() {
        if depth >= 20 || out.len() + stack.len() >= max_lines.max(2) || flat(a, b) {
            out.push(b);
        } else {
            let m = 0.5 * (a + b);
            stack.push((m, b, depth + 1));
            stack.push((a, m, depth + 1));
        }
    }
    out
}

/// Whether the surface between the parameter points `a` and `b` is within the tolerance (at
/// their middle), the side length `h` and the facet angle: no point is needed between them.
pub(crate) fn fine_between(s: &EvalSurface, rules: &Rules, h: f64, a: [f64; 2], b: [f64; 2]) -> bool {
    let at = |q: [f64; 2]| s.derivs(q[0], q[1]);
    let (Some((pa, _, _)), Some((pb, _, _))) = (at(a), at(b)) else { return true };
    if pa.dist(pb) > h * (1.0 + 1e-9) {
        return false;
    }
    let mid = [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1])];
    let sag = at(mid).map_or(0.0, |(m, _, _)| m.dist((pa + pb) * 0.5));
    if sag > rules.tol {
        return false;
    }
    // At a pole the normal is undefined: take it from just inside the segment.
    let normal = |q: [f64; 2], toward: [f64; 2]| {
        let n = |q: [f64; 2]| at(q).and_then(|(_, su, sv)| su.cross(sv).normalized());
        n(q).or_else(|| n([q[0] + 1e-4 * (toward[0] - q[0]), q[1] + 1e-4 * (toward[1] - q[1])]))
    };
    match (rules.turn, normal(a, b), normal(b, a)) {
        (Some(t), Some(na), Some(nb)) => !t.exceeds(na.dot(nb).clamp(-1.0, 1.0).acos(), sag),
        _ => true,
    }
}

/// Whether the surface between the iso-lines at `a` and `b` (direction `d`, sampled along
/// iso-lines across `across`) is within the tolerance, the side length `h` and the facet angle.
fn limits<'a>(s: &'a EvalSurface, d: usize, across: (f64, f64), rules: &'a Rules, h: f64) -> impl Fn(f64, f64) -> bool + 'a {
    let tol = rules.tol;
    let uv = move |x: f64, y: f64| if d == 0 { (x, y) } else { (y, x) };
    let at = move |x: f64, y: f64| {
        let (u, v) = uv(x, y);
        s.derivs(u, v).map(|(p, _, _)| p)
    };
    let normal = move |x: f64, y: f64| {
        let (u, v) = uv(x, y);
        s.derivs(u, v).and_then(|(_, su, sv)| su.cross(sv).normalized())
    };
    const ACROSS: usize = 5;
    let ys: Vec<f64> = (0..ACROSS).map(|k| across.0 + (across.1 - across.0) * k as f64 / (ACROSS - 1) as f64).collect();
    move |a: f64, b: f64| {
        ys.iter().all(|&y| {
            let (Some(pa), Some(pb)) = (at(a, y), at(b, y)) else { return true };
            if pa.dist(pb) > h * (1.0 + 1e-9) {
                return false;
            }
            let sag = [0.25, 0.5, 0.75].iter().filter_map(|f| at(a + (b - a) * f, y)).map(|q| q.dist_to_segment(pa, pb)).fold(0.0, f64::max);
            if sag > tol {
                return false;
            }
            // At a pole the normal is undefined: take it from just inside the segment.
            let na = normal(a, y).or_else(|| normal(a + 1e-4 * (b - a), y));
            let nb = normal(b, y).or_else(|| normal(b - 1e-4 * (b - a), y));
            match (rules.turn, na, nb) {
                (Some(t), Some(na), Some(nb)) => !t.exceeds(na.dot(nb).clamp(-1.0, 1.0).acos(), sag),
                _ => true,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_geom::Vec3;
    use solidcraft_geom::tess::EvalCurve;

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
    fn divides_the_turn_but_not_the_straight_direction() {
        let s = cylinder();
        let turn =
            divide(&s, 1, (0.0, std::f64::consts::TAU), (0.0, 1.0), &Rules { tol: 0.01, max_len: f64::INFINITY, turn: None }, f64::INFINITY, 10_000);
        let along =
            divide(&s, 0, (0.0, 1.0), (0.0, std::f64::consts::TAU), &Rules { tol: 0.01, max_len: f64::INFINITY, turn: None }, f64::INFINITY, 10_000);
        assert_eq!(along, vec![0.0, 1.0]);
        // Chord sag r(1 − cos(θ/2)) ≤ 0.01 with r = 3 needs θ ≤ 0.163: at least 39 segments.
        assert!(turn.len() >= 40 && turn.len() < 80, "{}", turn.len());
        // With a longest side the straight direction is divided too: 4 mm in sides ≤ 0.5.
        let along = divide(&s, 0, (0.0, 1.0), (0.0, std::f64::consts::TAU), &Rules { tol: 0.01, max_len: f64::INFINITY, turn: None }, 0.5, 10_000);
        assert_eq!(along.len(), 9);
    }

    /// The first and last lines are the range's ends to the last bit: the boundary nodes sit on
    /// them, and a line a rounding inside would be taken for an interior one (a drill point's
    /// cone, where a0 + (a1 − a0) is not a1).
    #[test]
    fn divides_with_the_ends_exactly() {
        let (a0, a1) = (-1.010485317689877, 1.083909784703316);
        let lines = divide(&cylinder(), 1, (a0, a1), (0.0, 1.0), &Rules { tol: 0.01, max_len: f64::INFINITY, turn: None }, f64::INFINITY, 10_000);
        assert_eq!(lines.first().copied(), Some(a0));
        assert_eq!(lines.last().copied(), Some(a1));
    }
}
