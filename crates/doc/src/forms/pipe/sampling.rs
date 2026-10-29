//! Bounded path samples on the exact CAD edge, divided at its spline knots.
use super::*;
use solidcraft_geom::Vec3;

pub(super) fn curve(curve: &kernel::CurveGeometry, limit: usize) -> Result<Vec<Vec3>> {
    if limit < 2 {
        return Err(invalid("pipe path sampling budget exceeded"));
    }
    let point = |t| curve.at(t).map(|p| p.0).ok_or_else(|| invalid("the pipe path edge cannot be evaluated"));
    let mut cuts: Vec<_> = (0..=8).map(|i| i as f64 / 8.0).collect();
    let [a, b] = curve.range;
    cuts.extend(curve.curve.knots.iter().filter_map(|k| {
        let t = (k - a) / (b - a);
        (t > 0.0 && t < 1.0).then_some(t)
    }));
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    let mut points = vec![point(0.0)?];
    for pair in cuts.windows(2) {
        let (lo, hi) = (pair[0], pair[1]);
        let mut stack = vec![(lo, hi, point(lo)?, point(hi)?, 0usize)];
        while let Some((lo, hi, a, b, depth)) = stack.pop() {
            let mid = (lo + hi) * 0.5;
            let p = point(mid)?;
            if p.dist_to_segment(a, b) > 0.001 {
                if depth >= 20 {
                    return Err(invalid("pipe path sampling did not converge"));
                }
                stack.push((mid, hi, p, b, depth + 1));
                stack.push((lo, mid, a, p, depth + 1));
            } else {
                points.push(b);
            }
            if points.len() + stack.len() > limit {
                return Err(invalid("pipe path sampling budget exceeded"));
            }
        }
    }
    Ok(points)
}
