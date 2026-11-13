//! A bounded polygon of a positive-weight spline. Each subcurve lies in
//! its Euclidean Bézier control hull, and distance to a chord is convex.
use crate::eval::ray_tensor::{halve_curve, Control, Interval};
use crate::Vec3;

const MAX_DEGREE: usize = 16;
const MAX_POINTS: usize = 32_768;
const MAX_WORK: usize = 8_000_000;
const MAX_DEPTH: usize = 20;

/// Approximate a trimmed rational spline by a path with a complete-curve
/// chord-deviation bound. The output includes both ends and knot crossings.
pub fn chord_samples(
    degree: usize,
    knots: &[f64],
    controls: &[[f64; 4]],
    range: [f64; 2],
    tolerance: f64,
    limit: usize,
) -> Option<Vec<Vec3>> {
    if !(1..=MAX_DEGREE).contains(&degree)
        || !(degree + 1..=MAX_POINTS).contains(&controls.len())
        || knots.len() != controls.len() + degree + 1
        || !(2..=MAX_POINTS).contains(&limit)
        || !tolerance.is_finite()
        || tolerance <= 0.0
        || knots.iter().any(|x| !x.is_finite())
        || knots
            .windows(2)
            .any(|window| matches!(window, [a, b] if a > b))
        || controls
            .iter()
            .any(|&[x, y, z, w]| w <= 0.0 || [x, y, z, w].iter().any(|v| !v.is_finite()))
    {
        return None;
    }
    let reversed = range[0] > range[1];
    let [start, end] = if reversed {
        [range[1], range[0]]
    } else {
        range
    };
    if !(*knots.get(degree)? <= start && start < end && end <= *knots.get(controls.len())?) {
        return None;
    }
    if knots.chunk_by(|a, b| a == b).any(|run| {
        run.len() > degree && run.first().is_some_and(|&knot| start < knot && knot < end)
    }) {
        return None;
    }
    let tolerance_squared = Interval::point(tolerance)
        .mul(Interval::point(tolerance))
        .lo;
    let mut output = Vec::new();
    let mut work = MAX_WORK;
    for i in degree..controls.len() {
        let a = start.max(*knots.get(i)?);
        let b = end.min(*knots.get(i + 1)?);
        if a >= b {
            continue;
        }
        // Keep a baseline of eight stations on the trimmed edge, including
        // straight sections, so the path builder has the customary density.
        let stations = (1..8).map(|k| start + (end - start) * (f64::from(k) / 8.0));
        let boundaries = stations
            .filter(|t| a < *t && *t < b)
            .chain(std::iter::once(b));
        let span = Span {
            controls: controls.get(i - degree..=i)?,
            knots,
            index: i,
            degree,
        };
        let mut lo = a;
        for hi in boundaries {
            if hi <= lo {
                continue;
            }
            let control = span.bezier(lo, hi, &mut work)?;
            let begin = euclidean(*control.first()?)?;
            if output.is_empty() {
                output.push(point(begin)?);
            }
            let mut stack = vec![(control, 0)];
            while let Some((control, depth)) = stack.pop() {
                let a = *output.last()?;
                let b = point(euclidean(*control.last()?)?)?;
                work = work.checked_sub(control.len())?;
                if control
                    .iter()
                    .all(|&h| euclidean(h).is_some_and(|p| near_chord(p, a, b, tolerance_squared)))
                {
                    if output.len() == limit {
                        return None;
                    }
                    output.push(b);
                } else {
                    if depth == MAX_DEPTH || output.len() + stack.len() + 2 > limit {
                        return None;
                    }
                    let [left, right] = halve_curve(&control, &mut work).ok()?;
                    stack.push((right, depth + 1));
                    stack.push((left, depth + 1));
                }
            }
            lo = hi;
        }
    }
    if reversed {
        output.reverse();
    }
    (output.len() >= 2).then_some(output)
}

struct Span<'a> {
    controls: &'a [[f64; 4]],
    knots: &'a [f64],
    index: usize,
    degree: usize,
}

impl Span<'_> {
    fn bezier(&self, a: f64, b: f64, work: &mut usize) -> Option<Vec<Control>> {
        let p = self.degree;
        *work = work.checked_sub((p + 1).pow(3))?;
        (0..=p)
            .map(|split| {
                // Ordered blossom arguments: p-split copies of a, then split copies of b.
                let mut values: Vec<_> = self
                    .controls
                    .iter()
                    .map(|c| c.map(Interval::point))
                    .collect();
                for level in 1..=p {
                    let t = if level <= p - split { a } else { b };
                    for i in (level..=p).rev() {
                        let k = self.index - p + i;
                        let lo = *self.knots.get(k)?;
                        let hi = *self.knots.get(k + p - level + 1)?;
                        if !(lo <= t && t <= hi && lo < hi) {
                            return None;
                        }
                        let alpha = Interval::point(t)
                            .sub(Interval::point(lo))
                            .div(Interval::point(hi).sub(Interval::point(lo)))
                            .intersect(Interval::UNIT)?;
                        let inverse = Interval::point(1.0).sub(alpha).intersect(Interval::UNIT)?;
                        let x = *values.get(i - 1)?;
                        let y = values.get_mut(i)?;
                        *y = std::array::from_fn(|axis| {
                            x[axis].mul(inverse).add(y[axis].mul(alpha))
                        });
                    }
                }
                values.last().copied()
            })
            .collect()
    }
}

fn euclidean([x, y, z, w]: Control) -> Option<[Interval; 3]> {
    if w.lo <= 0.0 {
        return None;
    }
    let p = [x, y, z].map(|coordinate| coordinate.div(w));
    p.iter().all(|value| value.is_finite()).then_some(p)
}

fn point(p: [Interval; 3]) -> Option<Vec3> {
    let p = p.map(Interval::midpoint);
    p.iter().all(|x| x.is_finite()).then(|| Vec3::from_array(p))
}

fn near_chord(p: [Interval; 3], a: Vec3, b: Vec3, tolerance_squared: f64) -> bool {
    let Some(approximate) = point(p) else {
        return false;
    };
    let direction = b - a;
    let length_squared = direction.length_squared();
    let t = if length_squared > 0.0 {
        ((approximate - a).dot(direction) / length_squared).clamp(0.0, 1.0)
    } else {
        0.0
    };
    if !t.is_finite() {
        return false;
    }
    // The floating calculation only proposes a point on the chord. Qualify
    // distance to that point with outward arithmetic, including both endpoints.
    let a = a.to_array().map(Interval::point);
    let b = b.to_array().map(Interval::point);
    let squared =
        p.into_iter()
            .zip(a.into_iter().zip(b))
            .fold(Interval::ZERO, |sum, (x, (a, b))| {
                let delta = x.sub(a.add(b.sub(a).scale(t)));
                let bound = Interval::point(delta.lo.abs().max(delta.hi.abs()));
                sum.add(bound.mul(bound))
            });
    squared.is_finite() && squared.hi <= tolerance_squared
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn a_cubic_with_midpoint_on_the_chord_still_splits_to_resolve_both_lobes() {
        let knots = [0., 0., 0., 0., 1., 1., 1., 1.];
        let controls = [
            [0., 0., 0., 1.],
            [0., 10., 0., 1.],
            [1., -10., 0., 1.],
            [1., 0., 0., 1.],
        ];
        let path = chord_samples(3, &knots, &controls, [0., 1.], 0.001, 2048).unwrap();
        assert!(path.len() > 9);
        assert!(path.iter().any(|p| p.y > 2.5) && path.iter().any(|p| p.y < -2.5));
        assert!(chord_samples(3, &knots, &controls, [0., 1.], 0.001, 9).is_none());
    }

    #[test]
    fn rational_line_samples_respect_trim_and_direction() {
        let knots = [0., 0., 1., 1.];
        let controls = [[0., 0., 0., 1.], [2., 0., 0., 2.]];
        let path = chord_samples(1, &knots, &controls, [0.75, 0.25], 0.001, 9).unwrap();
        assert_eq!(path.len(), 9);
        assert!((path.first().unwrap().x - 6.0 / 7.0).abs() < 1e-14);
        assert!((path.last().unwrap().x - 0.4).abs() < 1e-14);
        assert!(path.windows(2).all(|p| matches!(p, [a, b] if a.x > b.x)));
    }
}
