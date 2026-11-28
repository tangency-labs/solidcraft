//! Reducing a long B-spline edge to the number of control points the portable record holds.
//!
//! SMLib can hand back the curve of a short intersection edge sampled so finely that it has more
//! control points than the record accepts (`MAX_POINTS`). The published knot-removal algorithm
//! (Piegl and Tiller, *The NURBS Book*, A5.8) takes knots out of a spline one at a time and moves
//! the control points nearby; each removal is certified: the new curve differs from the one before
//! it nowhere by more than the distance the algorithm measures between the two control points it
//! compares (the basis functions sum to 1). The measured distances are added up, per stretch of
//! the parameter range, so the reduced curve is within [`Reduced::bound`] of the original
//! everywhere, by proof and not by sampling. A reduction that cannot reach the cap within its
//! budget is refused with that reason; the tolerance is never exceeded.

use super::{Res, unsupported};
use solidcore_record::brep as b;

/// The most control points a spline of the portable record holds (`brep::knots`).
pub(super) const MAX_POINTS: usize = 4096;

/// A reduced curve and the proved largest distance of it from the original.
pub(super) struct Reduced {
    pub curve: b::Curve,
    pub bound: f64,
}

/// A curve with at most [`MAX_POINTS`] control points within `budget` of `curve` everywhere: the
/// curve itself when it has few enough. The ends are not moved. Only B-splines are reduced.
pub(super) fn within_cap(curve: b::Curve, budget: f64) -> Res<Reduced> {
    reduce_to(curve, MAX_POINTS, budget)
}

/// [`within_cap`] with the cap given.
pub(super) fn reduce_to(curve: b::Curve, cap: usize, budget: f64) -> Res<Reduced> {
    match curve {
        b::Curve::Spline { knots, points } if points.len() > cap => {
            let (knots, points, bound) = reduce(knots, points, cap, budget)?;
            Ok(Reduced { curve: b::Curve::Spline { knots, points }, bound })
        }
        b::Curve::RationalSpline { knots, homogeneous } if homogeneous.len() > cap => {
            // The distance in homogeneous coordinates that bounds the distance of the points
            // (The NURBS Book, 5.4: wmin * tolerance / (1 + largest |P|)).
            let wmin = homogeneous.iter().map(|h| h[3]).fold(f64::INFINITY, f64::min);
            if !(wmin > 0.0 && wmin.is_finite()) {
                return Err(unsupported("a rational edge spline with a weight that is not positive"));
            }
            let pmax = homogeneous.iter().map(|h| (h[0] / h[3]).abs().max((h[1] / h[3]).abs()).max((h[2] / h[3]).abs())).fold(0.0, f64::max);
            let scale = wmin / (1.0 + pmax);
            let (knots, homogeneous, bound) = reduce(knots, homogeneous, cap, budget * scale)?;
            Ok(Reduced { curve: b::Curve::RationalSpline { knots, homogeneous }, bound: bound / scale })
        }
        curve => Ok(Reduced { curve, bound: 0.0 }),
    }
}

fn dist<const D: usize>(a: &[f64; D], b: &[f64; D]) -> f64 {
    a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum::<f64>().sqrt()
}

/// The curve's value at `t` (de Boor), none for a parameter outside the knots or a bad spline.
fn eval<const D: usize>(knots: &[f64], points: &[[f64; D]], degree: usize, t: f64) -> Option<[f64; D]> {
    let n = points.len();
    if knots.len() != n + degree + 1 || n <= degree {
        return None;
    }
    let (lo, hi) = (*knots.get(degree)?, *knots.get(n)?);
    let t = t.clamp(lo, hi);
    // The span: the last k in degree..n with knots[k] <= t, the final one for t at the end.
    let k = knots.partition_point(|u| *u <= t).saturating_sub(1).clamp(degree, n - 1);
    let mut d: Vec<[f64; D]> = points.get(k - degree..=k)?.to_vec();
    for r in 1..=degree {
        for j in (r..=degree).rev() {
            let (a, z) = (*knots.get(j + k - degree)?, *knots.get(j + 1 + k - r)?);
            let alpha = if z > a { (t - a) / (z - a) } else { 0.0 };
            let (p, q) = (*d.get(j - 1)?, *d.get(j)?);
            let mut v = [0.0; D];
            for c in 0..D {
                v[c] = (1.0 - alpha) * p[c] + alpha * q[c];
            }
            *d.get_mut(j)? = v;
        }
    }
    d.get(degree).copied()
}

/// The valid clamped spline [`reduce`] works on: knots ordered and finite, `degree + 1` equal at
/// each end, the points finite.
fn valid<const D: usize>(knots: &[f64], points: &[[f64; D]], degree: usize) -> bool {
    let n = points.len();
    degree >= 1
        && n > degree
        && knots.len() == n + degree + 1
        && knots.iter().all(|k| k.is_finite())
        && knots.windows(2).all(|w| w[0] <= w[1])
        && points.iter().all(|p| p.iter().all(|c| c.is_finite()))
        && knots[0] == knots[degree]
        && knots[n] == knots[n + degree]
        && knots[degree] < knots[n]
}

/// Remove one copy of the interior knot `u` if the curve stays within `tol` of itself (A5.8 for one
/// removal). The measured distance of the two control points compared, or none when it is not
/// removable. `lo..hi` are the knot indices whose spans the removal changes.
fn remove_once<const D: usize>(knots: &mut Vec<f64>, points: &mut Vec<[f64; D]>, degree: usize, u: f64, tol: f64) -> Option<f64> {
    let n = points.len();
    let ord = degree + 1;
    // r: the last index of u; s: its multiplicity.
    let r = knots.partition_point(|k| *k <= u).checked_sub(1)?;
    if knots.get(r).copied()? != u {
        return None;
    }
    let s = knots[..=r].iter().rev().take_while(|k| **k == u).count();
    if r < ord || r >= n || s > degree {
        return None;
    }
    let (first, last) = (r - degree, r - s);
    let off = first - 1;
    let mut temp: Vec<[f64; D]> = vec![[0.0; D]; last + 2 - off + 1];
    *temp.get_mut(0)? = *points.get(off)?;
    *temp.get_mut(last + 1 - off)? = *points.get(last + 1)?;
    let (mut i, mut j, mut ii, mut jj) = (first, last, 1usize, last - off);
    while j > i {
        let (di, dj) = (knots.get(i + ord)? - knots.get(i)?, knots.get(j + ord)? - knots.get(j)?);
        if !(di > 0.0 && dj > 0.0) {
            return None;
        }
        let alfi = (u - knots.get(i)?) / di;
        let alfj = (u - knots.get(j)?) / dj;
        if !(alfi > 0.0 && alfj < 1.0) {
            return None;
        }
        let (pi, pj) = (*points.get(i)?, *points.get(j)?);
        let (ti, tj) = (*temp.get(ii - 1)?, *temp.get(jj + 1)?);
        let (mut vi, mut vj) = ([0.0; D], [0.0; D]);
        for c in 0..D {
            vi[c] = (pi[c] - (1.0 - alfi) * ti[c]) / alfi;
            vj[c] = (pj[c] - alfj * tj[c]) / (1.0 - alfj);
        }
        *temp.get_mut(ii)? = vi;
        *temp.get_mut(jj)? = vj;
        i += 1;
        ii += 1;
        j -= 1;
        jj -= 1;
    }
    let measured = if j < i {
        dist(temp.get(ii - 1)?, temp.get(jj + 1)?)
    } else {
        let di = knots.get(i + ord)? - knots.get(i)?;
        if !(di > 0.0) {
            return None;
        }
        let alfi = (u - knots.get(i)?) / di;
        let (a, z) = (*temp.get(ii + 1)?, *temp.get(ii - 1)?);
        let mut mix = [0.0; D];
        for c in 0..D {
            mix[c] = alfi * a[c] + (1.0 - alfi) * z[c];
        }
        dist(points.get(i)?, &mix)
    };
    if !(measured <= tol) {
        return None;
    }
    // Take the new control points, drop the knot and the control point it leaves unneeded.
    let (mut i, mut j) = (first, last);
    while j > i {
        *points.get_mut(i)? = *temp.get(i - off)?;
        *points.get_mut(j)? = *temp.get(j - off)?;
        i += 1;
        j -= 1;
    }
    knots.remove(r);
    points.remove((2 * r - s - degree) / 2);
    Some(measured)
}

/// The knots and points of the spline taken down to `cap` points, and the proved largest distance
/// from the original. Removes the cheapest knots first, at growing allowances, so the error is
/// spread; no stretch of the parameter range collects more than `budget`.
fn reduce<const D: usize>(mut knots: Vec<f64>, mut points: Vec<[f64; D]>, cap: usize, budget: f64) -> Res<(Vec<f64>, Vec<[f64; D]>, f64)> {
    let degree = knots.len().saturating_sub(points.len() + 1);
    if !valid(&knots, &points, degree) {
        return Err(unsupported("an edge spline whose knots are not those of a clamped B-spline"));
    }
    let before = points.len();
    let n = points.len();
    // The original's distinct knots: the stretches the error is added up over (none at the ends).
    let mut breaks: Vec<f64> = knots.clone();
    breaks.dedup();
    let stretches = breaks.len().saturating_sub(1);
    let mut used = vec![0.0f64; stretches];
    let interior: Vec<f64> = breaks.iter().copied().filter(|u| *u > knots[degree] && *u < knots[n]).collect();
    let stretch_of = |t: f64, breaks: &[f64]| breaks.partition_point(|b| *b <= t).saturating_sub(1).min(stretches.saturating_sub(1));
    let original = (knots.clone(), points.clone());
    let mut allowance = budget / 64.0;
    'levels: loop {
        loop {
            let mut removed = 0usize;
            for &u in &interior {
                while points.len() > cap {
                    let r = match knots.partition_point(|k| *k <= u).checked_sub(1) {
                        Some(r) => r,
                        None => break,
                    };
                    // The stretches the removal changes: the spans of the control points it moves.
                    let lo = knots.get(r.saturating_sub(degree + 1)).copied().unwrap_or(u);
                    let hi = knots.get((r + degree + 2).min(knots.len() - 1)).copied().unwrap_or(u);
                    let (a, z) = (stretch_of(lo, &breaks), stretch_of(hi, &breaks));
                    // No change reaches an end stretch: the ends of the edge stay where they are.
                    let room = if a == 0 || z + 1 >= stretches {
                        0.0
                    } else {
                        (a..=z).map(|k| budget - used.get(k).copied().unwrap_or(budget)).fold(f64::INFINITY, f64::min)
                    };
                    let tol = allowance.min(room);
                    if !(tol > 0.0) {
                        break;
                    }
                    match remove_once(&mut knots, &mut points, degree, u, tol) {
                        Some(measured) => {
                            for k in a..=z {
                                if let Some(x) = used.get_mut(k) {
                                    *x += measured;
                                }
                            }
                            removed += 1;
                        }
                        None => break,
                    }
                }
            }
            if points.len() <= cap || removed == 0 {
                break;
            }
        }
        if points.len() <= cap {
            break 'levels;
        }
        if allowance >= budget {
            break;
        }
        allowance = (allowance * 4.0).min(budget);
    }
    if points.len() > cap {
        return Err(unsupported(format!(
            "an edge spline of {before} control points cannot be reduced to the {cap} the record holds within {budget:e} of it (reduced to {})",
            points.len()
        )));
    }
    let bound = used.iter().copied().fold(0.0, f64::max);
    // The proof is the bound; sampling checks the implementation of it.
    let (ok, op) = (&original.0, &original.1);
    for w in breaks.windows(2) {
        for q in 0..=4 {
            let t = w[0] + (w[1] - w[0]) * f64::from(q) / 4.0;
            let (x, y) = (eval(ok, op, degree, t), eval(&knots, &points, degree, t));
            match (x, y) {
                (Some(x), Some(y)) if dist(&x, &y) <= bound * (1.0 + 1e-6) + 1e-12 => {}
                _ => return Err(unsupported("a reduced edge spline that differs from the original by more than its proved bound")),
            }
        }
    }
    Ok((knots, points, bound))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clamped cubic spline through `m` samples of a curve, as a chain of cubic Beziers (knots of
    /// multiplicity 3), the way a fine trace is held: each piece is a Hermite fit of its samples.
    fn bezier_chain(m: usize, f: impl Fn(f64) -> [f64; 3], df: impl Fn(f64) -> [f64; 3]) -> (Vec<f64>, Vec<[f64; 3]>) {
        let h = 1.0 / (m - 1) as f64;
        let mut knots = vec![0.0; 4];
        let mut points = Vec::new();
        for k in 0..m - 1 {
            let (t0, t1) = (k as f64 * h, (k + 1) as f64 * h);
            let (p0, p1, d0, d1) = (f(t0), f(t1), df(t0), df(t1));
            if k == 0 {
                points.push(p0);
            }
            points.push([p0[0] + d0[0] * h / 3.0, p0[1] + d0[1] * h / 3.0, p0[2] + d0[2] * h / 3.0]);
            points.push([p1[0] - d1[0] * h / 3.0, p1[1] - d1[1] * h / 3.0, p1[2] - d1[2] * h / 3.0]);
            points.push(p1);
            if k + 1 < m - 1 {
                knots.extend([t1; 3]);
            }
        }
        knots.extend([1.0; 4]);
        (knots, points)
    }

    fn helix(t: f64) -> [f64; 3] {
        let a = 6.0 * std::f64::consts::PI * t;
        [3.0 * a.cos(), 3.0 * a.sin(), 20.0 * t]
    }
    fn dhelix(t: f64) -> [f64; 3] {
        let a = 6.0 * std::f64::consts::PI * t;
        let w = 6.0 * std::f64::consts::PI;
        [-3.0 * w * a.sin(), 3.0 * w * a.cos(), 20.0]
    }

    fn deviation(a: &(Vec<f64>, Vec<[f64; 3]>), b: &(Vec<f64>, Vec<[f64; 3]>)) -> f64 {
        (0..=20000)
            .map(|i| {
                let t = f64::from(i) / 20000.0;
                match (eval(&a.0, &a.1, 3, t), eval(&b.0, &b.1, 3, t)) {
                    (Some(x), Some(y)) => dist(&x, &y),
                    _ => f64::INFINITY,
                }
            })
            .fold(0.0, f64::max)
    }

    #[test]
    fn a_long_helix_spline_comes_down_to_the_cap_within_its_budget() {
        // 2800 Bezier pieces: 8401 control points, past the 4096 the record takes.
        let (knots, points) = bezier_chain(2801, helix, dhelix);
        assert!(points.len() > 8000);
        // The record refuses it as it is: this is what an export of such an edge ran into.
        let raw = b::Curve::Spline { knots: knots.clone(), points: points.clone() };
        assert!(b::check_curve(&raw, &mut b::Budget::input()).is_err());
        let budget = 1e-4;
        let reduced = reduce_to(b::Curve::Spline { knots: knots.clone(), points: points.clone() }, MAX_POINTS, budget);
        let Ok(Reduced { curve: b::Curve::Spline { knots: k, points: p }, bound }) = reduced else { panic!("the helix must reduce") };
        assert!(p.len() <= MAX_POINTS, "{} points", p.len());
        assert!(b::check_curve(&b::Curve::Spline { knots: k.clone(), points: p.clone() }, &mut b::Budget::input()).is_ok());
        assert!(bound <= budget, "proved bound {bound}");
        let measured = deviation(&(knots, points), &(k, p));
        assert!(measured <= bound + 1e-12, "measured {measured} past the proved bound {bound}");
    }

    #[test]
    fn a_spline_within_the_cap_is_left_alone() {
        let (knots, points) = bezier_chain(100, helix, dhelix);
        let Ok(r) = reduce_to(b::Curve::Spline { knots: knots.clone(), points: points.clone() }, MAX_POINTS, 1e-6) else { panic!("refused") };
        assert_eq!(r.bound, 0.0);
        assert!(matches!(r.curve, b::Curve::Spline { knots: k, points: p } if k == knots && p == points));
    }

    #[test]
    fn a_reduction_the_budget_does_not_allow_is_refused_with_its_reason() {
        // A jagged polyline-like spline carries information in every point: nothing can go.
        let m = 300;
        let h = 1.0 / m as f64;
        let mut knots = vec![0.0; 2];
        let mut points = Vec::new();
        for k in 0..=m {
            points.push([k as f64 * h, if k % 2 == 0 { 0.0 } else { 1.0 }, 0.0]);
            if k > 0 && k < m {
                knots.push(k as f64 * h);
            }
        }
        knots.extend([1.0; 2]);
        let Err(e) = reduce_to(b::Curve::Spline { knots, points }, 100, 1e-6) else { panic!("must be refused") };
        assert!(e.to_string().contains("cannot be reduced"), "{e}");
    }

    #[test]
    fn a_rational_circle_trace_reduces_within_its_budget() {
        // The unit circle as many rational pieces with weights 1 (a rational spline of unit weights).
        let (knots, points) = bezier_chain(
            2400,
            |t| [(std::f64::consts::TAU * t).cos(), (std::f64::consts::TAU * t).sin(), 0.0],
            |t| {
                let w = std::f64::consts::TAU;
                [-w * (w * t).sin(), w * (w * t).cos(), 0.0]
            },
        );
        let homogeneous: Vec<[f64; 4]> = points.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect();
        let Ok(r) = reduce_to(b::Curve::RationalSpline { knots, homogeneous }, MAX_POINTS, 1e-5) else { panic!("must reduce") };
        let b::Curve::RationalSpline { homogeneous: h, .. } = r.curve else { panic!("stays rational") };
        assert!(h.len() <= MAX_POINTS);
        assert!(r.bound <= 1e-5);
    }

    #[test]
    fn hostile_splines_are_errors_not_panics() {
        let bad = [
            (vec![0.0, 0.0, 1.0, 1.0], vec![[0.0; 3]; 3]),
            (vec![f64::NAN; 8], vec![[0.0; 3]; 4]),
            (vec![1.0, 0.0, 0.0, 0.0, 1.0, 1.0], vec![[0.0; 3]; 2]),
            (vec![0.0; 5], vec![[0.0; 3]; 2]),
        ];
        for (knots, points) in bad {
            let _ = reduce_to(b::Curve::Spline { knots, points }, 1, 1e-6);
        }
    }
}
