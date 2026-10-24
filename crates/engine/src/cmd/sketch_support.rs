//! Bounded roots on exact curve pieces against an infinite line or complete circle.
//! Rational pieces are intersected in homogeneous coordinates, without sampled chords.
use solidcraft_geom::{Seg2, Vec2};
use solidcraft_sketch::{Shape, intersections};

use crate::{Result, params::bad};

/// Degrees the exact extend intersection handles: a round support squares the piece's degree in
/// homogeneous form, and both the root finder and its `evaluate` buffer hold at most 15
/// coefficients, so 2·7 + 1 fits.
const MAX_EXACT_EXTEND_DEGREE: u8 = 7;

fn choose(n: usize, k: usize) -> f64 {
    (0..k.min(n - k)).fold(1.0, |value, i| value * (n - i) as f64 / (i + 1) as f64)
}
fn product(a: &[f64], b: &[f64]) -> Vec<f64> {
    let (n, m) = (a.len() - 1, b.len() - 1);
    (0..=n + m)
        .map(|k| (k.saturating_sub(m)..=k.min(n)).map(|i| a[i] * b[k - i] * choose(n, i) * choose(m, k - i) / choose(n + m, k)).sum())
        .collect()
}
fn evaluate(c: &[f64], t: f64) -> f64 {
    let mut values = [0.0; 15];
    for (slot, value) in values.iter_mut().zip(c) {
        *slot = *value;
    }
    for n in (1..c.len()).rev() {
        for i in 0..n {
            values[i] = values[i] * (1.0 - t) + values[i + 1] * t;
        }
    }
    values[0]
}

/// Derivative roots partition a polynomial into monotone intervals. Testing its critical
/// points retains repeated/tangent roots that a sign-change-only search would miss.
fn roots(c: &[f64], budget: &mut usize) -> Result<Vec<f64>> {
    if c.is_empty() || c.len() > 15 || c.iter().any(|v| !v.is_finite()) {
        return Err(bad("sketch.extend", "invalid intersection polynomial"));
    }
    let scale = c.iter().map(|v| v.abs()).fold(0.0_f64, f64::max);
    if scale < 1e-28 {
        return Ok(Vec::new());
    }
    let eps = scale * 2e-13;
    if c.len() == 1 {
        return Ok(Vec::new());
    }
    let d: Vec<f64> = c.windows(2).map(|w| (w[1] - w[0]) * (c.len() - 1) as f64).collect();
    let mut knots = roots(&d, budget)?;
    knots.extend([0.0, 1.0]);
    knots.sort_by(f64::total_cmp);
    knots.dedup_by(|a, b| (*a - *b).abs() < 1e-13);
    let mut found: Vec<f64> = knots.iter().copied().filter(|t| evaluate(c, *t).abs() <= eps).collect();
    for pair in knots.windows(2) {
        let (mut lo, mut hi) = (pair[0], pair[1]);
        let (mut fl, fh) = (evaluate(c, lo), evaluate(c, hi));
        if fl.abs() <= eps || fh.abs() <= eps || fl.signum() == fh.signum() {
            continue;
        }
        for _ in 0..64 {
            if *budget == 0 {
                return Err(bad("sketch.extend", "intersection work limit exceeded"));
            }
            *budget -= 1;
            let mid = (lo + hi) * 0.5;
            let f = evaluate(c, mid);
            if hi - lo < 2e-14 {
                break;
            }
            if f.signum() == fl.signum() {
                lo = mid;
                fl = f;
            } else {
                hi = mid;
            }
        }
        found.push((lo + hi) * 0.5);
    }
    found.sort_by(f64::total_cmp);
    found.dedup_by(|a, b| (*a - *b).abs() < 1e-10);
    Ok(found)
}

pub(super) fn hits(support: &Shape, seg: &Seg2, budget: &mut usize) -> Result<Vec<Vec2>> {
    if let Seg2::Line { a, b } = *seg {
        let other = Shape::Line { a, b };
        if let Shape::Line { a: pa, b: pb } = *support
            && (pb - pa).normalized().is_some_and(|d| d.cross(a - pa).abs() < 1e-9 && d.cross(b - pa).abs() < 1e-9)
        {
            // A collinear line is passed over (no hits) so extend can find the next target on
            // its way instead of failing.
            return Ok(Vec::new());
        }
        return Ok(intersections(support, &other)
            .into_iter()
            .filter(|p| {
                let d = b - a;
                let t = (*p - a).dot(d) / d.len2().max(1e-30);
                (-1e-10..=1.0 + 1e-10).contains(&t)
            })
            .collect());
    }
    if let Seg2::Arc { center, radius, start, sweep } = *seg {
        let other = Shape::Round { c: center, r: radius, start, sweep };
        if let Shape::Round { c, r, .. } = *support
            && c.dist(center) < 1e-9
            && (r - radius).abs() < 1e-9
        {
            // The same circle is passed over, so extend can find the next target.
            return Ok(Vec::new());
        }
        return Ok(intersections(support, &other)
            .into_iter()
            .filter(|p| {
                let t = (((*p - center).angle() - start) * sweep.signum()).rem_euclid(std::f64::consts::TAU);
                t <= sweep.abs() + 1e-10
            })
            .collect());
    }
    let (points, weights): (Vec<Vec2>, Vec<f64>) = match *seg {
        Seg2::Cubic { p0, p1, p2, p3 } => (vec![p0, p1, p2, p3], vec![1.0; 4]),
        Seg2::Conic { a, apex, b, w } if w.is_finite() && w > 0.0 => (vec![a, apex, b], vec![1.0, w, 1.0]),
        Seg2::Bezier { n, p } if (1..=MAX_EXACT_EXTEND_DEGREE).contains(&n) => (p[..=usize::from(n)].to_vec(), vec![1.0; usize::from(n) + 1]),
        _ => return Err(bad("sketch.extend", "invalid exact curve piece")),
    };
    if points.iter().any(|p| !p.is_finite()) {
        return Err(bad("sketch.extend", "nonfinite intersection geometry"));
    }
    let weight_scale = weights.iter().copied().fold(1.0_f64, f64::max);
    let weights: Vec<f64> = weights.iter().map(|w| w / weight_scale).collect();
    let origin = match *support {
        Shape::Line { a, .. } => a,
        Shape::Round { c, .. } => c,
    };
    let scale = points.iter().map(|p| p.dist(origin)).fold(1.0_f64, f64::max);
    let x: Vec<f64> = points.iter().zip(&weights).map(|(p, w)| (p.x - origin.x) * w / scale).collect();
    let y: Vec<f64> = points.iter().zip(&weights).map(|(p, w)| (p.y - origin.y) * w / scale).collect();
    let c = match *support {
        Shape::Line { a, b } => {
            let d = (b - a).normalized().ok_or_else(|| bad("sketch.extend", "zero-length support"))?;
            x.iter().zip(&y).map(|(x, y)| d.x * y - d.y * x).collect::<Vec<_>>()
        }
        Shape::Round { r, .. } => {
            let (xx, yy, dd) = (product(&x, &x), product(&y, &y), product(&weights, &weights));
            xx.iter().zip(&yy).zip(&dd).map(|((x, y), d)| x + y - (r / scale).powi(2) * d).collect()
        }
    };
    if c.iter().all(|v| v.abs() < 1e-28) {
        // A zero polynomial: the curve lies on the support (infinite line or circle). Pass it
        // over, so extend can find the next target.
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for t in roots(&c, budget)? {
        let p = seg.point_at(t);
        if !p.is_finite() || support.project(p).dist(p) > 1e-7 {
            return Err(bad("sketch.extend", "the intersection could not be resolved accurately"));
        }
        if !out.iter().any(|q: &Vec2| q.dist(p) < 1e-7) {
            out.push(p);
        }
    }
    Ok(out)
}

/// How far an infinite line (#1359) reaches each way from `near`, as a boundary: well past any
/// sketch geometry.
const ENDLESS: f64 = 1e7;

/// An infinite line's piece `seg` as a boundary: the line through it, as a segment reaching
/// [`ENDLESS`] each way from the point of the line nearest `near`. Other pieces are returned as
/// they are.
pub(super) fn endless(seg: &Seg2, near: Vec2) -> Seg2 {
    let Seg2::Line { a, b } = *seg else { return *seg };
    let Some(dir) = (b - a).normalized() else { return *seg };
    let mid = a + dir * (near - a).dot(dir);
    Seg2::Line { a: mid - dir * ENDLESS, b: mid + dir * ENDLESS }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_extend_negative_arc_target() {
        let support = Shape::Line { a: Vec2::new(-2.0, 0.5), b: Vec2::new(-1.5, 0.5) };
        let target = Seg2::Arc { center: Vec2::ZERO, radius: 1.0, start: std::f64::consts::PI, sweep: -std::f64::consts::FRAC_PI_2 };
        let found = hits(&support, &target, &mut 1000).unwrap();
        assert_eq!(found.len(), 1);
        assert!(found[0].dist(Vec2::new(-0.75_f64.sqrt(), 0.5)) < 1e-10);
    }

    #[test]
    fn exact_extend_degree_seven_retains_tangent_and_crossing() {
        // Degree elevation of quadratic y=(2t-1)^2 retains its double root at t=1/2.
        let mut y = vec![1.0, -1.0, 1.0];
        while y.len() < 8 {
            let n = y.len();
            let mut elevated = vec![y[0]];
            for i in 1..n {
                let a = i as f64 / n as f64;
                elevated.push(a * y[i - 1] + (1.0 - a) * y[i]);
            }
            elevated.push(y[n - 1]);
            y = elevated;
        }
        let mut points = [Vec2::ZERO; solidcraft_geom::MAX_BEZIER_CTRL];
        for (i, p) in points.iter_mut().take(8).enumerate() {
            *p = Vec2::new(10.0 + i as f64 * 20.0 / 7.0, y[i]);
        }
        let target = Seg2::Bezier { n: 7, p: points };
        let support = Shape::Line { a: Vec2::ZERO, b: Vec2::X };
        let found = hits(&support, &target, &mut 10000).unwrap();
        assert_eq!(found.len(), 1);
        assert!(found[0].dist(Vec2::new(20.0, 0.0)) < 1e-7);
        let crossing = Shape::Line { a: Vec2::new(0.0, 0.25), b: Vec2::new(1.0, 0.25) };
        let found = hits(&crossing, &target, &mut 10000).unwrap();
        assert_eq!(found.len(), 2);
        assert!(found[0].dist(Vec2::new(15.0, 0.25)) < 1e-7);
        assert!(found[1].dist(Vec2::new(25.0, 0.25)) < 1e-7);
        assert!(hits(&crossing, &target, &mut 0).is_err());
        let circle = Shape::Round { c: Vec2::new(20.0, -1.0), r: 1.0, start: 0.0, sweep: std::f64::consts::TAU };
        let found = hits(&circle, &target, &mut 10000).unwrap();
        assert_eq!(found.len(), 1, "degree fourteen tangent {found:?}");
        assert!(found[0].dist(Vec2::new(20.0, 0.0)) < 1e-7);
    }

    #[test]
    fn exact_extend_rational_quarter_hits_analytic_eight_six_point() {
        let target =
            Seg2::Conic { a: Vec2::new(10.0, 0.0), apex: Vec2::new(10.0, 10.0), b: Vec2::new(0.0, 10.0), w: std::f64::consts::FRAC_1_SQRT_2 };
        let support = Shape::Line { a: Vec2::new(0.0, 6.0), b: Vec2::new(1.0, 6.0) };
        let found = hits(&support, &target, &mut 10000).unwrap();
        assert_eq!(found.len(), 1);
        assert!(found[0].dist(Vec2::new(8.0, 6.0)) < 1e-7);
    }
}
