//! Restrict a tensor surface to a straight line in its parameter plane.
use super::*;

fn snap(surface: &NurbsSurface, d: usize, value: f64) -> f64 {
    surface.knots[d].iter().copied().find(|k| (value - k).abs() <= 64.0 * f64::EPSILON * (1.0 + value.abs().max(k.abs()))).unwrap_or(value)
}
fn coefficients(surface: &NurbsSurface, d: usize, span: usize, a: f64, b: f64, polynomial_degree: usize) -> SplineResult<Vec<Vec<f64>>> {
    let degree = surface.degree[d];
    let first = span.checked_sub(degree).ok_or_else(|| SplineError::Invalid("invalid parameter-line span".into()))?;
    (first..=span)
        .map(|i| {
            let local = surface.knots[d].get(i..i + degree + 2).ok_or_else(|| SplineError::Invalid("missing parameter-line basis".into()))?;
            if a == b {
                let side = if a >= surface.domain()[d][1] { Side::Left } else { Side::Right };
                return Ok(vec![local_basis(local, a, side, 0)[0]; polynomial_degree + 1]);
            }
            let mut values = basis_bezier(local, a.min(b), a.max(b))?;
            if b < a {
                values.reverse();
            }
            Ok(values)
        })
        .collect()
}

impl NurbsSurface {
    /// Exact rational curve `S(a + t*(b-a))`, 0 <= t <= 1. Each knot crossing
    /// starts a Bezier piece. Products of the two Bernstein bases have degree
    /// `p+q` (an isoparametric direction contributes degree zero). The pieces
    /// share endpoints and form one clamped curve; unsupported degrees and
    /// discontinuities are refused instead of sampled or fitted.
    pub fn parameter_line(&self, a: [f64; 2], b: [f64; 2]) -> SplineResult<(Vec<f64>, Vec<HPoint>)> {
        let domain = self.domain();
        if a == b
            || (0..2).any(|d| {
                !a[d].is_finite() || !b[d].is_finite() || a[d] < domain[d][0] || a[d] > domain[d][1] || b[d] < domain[d][0] || b[d] > domain[d][1]
            })
        {
            return invalid("a parameter line needs distinct finite endpoints inside the surface domain");
        }
        let delta = [b[0] - a[0], b[1] - a[1]];
        let degrees = [if delta[0] == 0.0 { 0 } else { self.degree[0] }, if delta[1] == 0.0 { 0 } else { self.degree[1] }];
        let degree = degrees[0] + degrees[1];
        if degree > MAX_SPLINE_DEGREE {
            return Err(SplineError::Unsupported("parameter-line degree exceeds the curve evaluator".into()));
        }
        let mut cuts = vec![0.0, 1.0];
        for d in 0..2 {
            if delta[d] == 0.0 {
                continue;
            }
            cuts.extend(self.knots[d].iter().filter_map(|k| {
                let t = (k - a[d]) / delta[d];
                (t > 0.0 && t < 1.0).then_some(t)
            }));
        }
        cuts.sort_by(f64::total_cmp);
        // Distinct crossings can be almost coincident in the two directions.
        // Merging them would leave an interior knot inside a polynomial piece.
        cuts.dedup();
        let work = cuts
            .len()
            .saturating_mul(self.degree[0] + 1)
            .saturating_mul(self.degree[1] + 1)
            .saturating_mul(degrees[0] + 1)
            .saturating_mul(degrees[1] + 1);
        if cuts.len() > 257 || work > 2_000_000 {
            return Err(SplineError::TooLarge("parameter-line extraction budget exceeded".into()));
        }
        let mut knots = vec![0.0; degree + 1];
        let mut ctrl = Vec::<HPoint>::new();
        for (piece, interval) in cuts.windows(2).enumerate() {
            let [lo, hi] = [interval[0], interval[1]];
            let mut spans = [0; 2];
            let mut ends = [[0.0; 2]; 2];
            for d in 0..2 {
                ends[d] = [snap(self, d, a[d] + delta[d] * lo), snap(self, d, a[d] + delta[d] * hi)];
                spans[d] = find_span(&self.knots[d], self.degree[d], self.count[d], (ends[d][0] + ends[d][1]) * 0.5)
                    .ok_or_else(|| SplineError::Invalid("parameter-line span is outside the surface".into()))?;
            }
            let u = coefficients(self, 0, spans[0], ends[0][0], ends[0][1], degrees[0])?;
            let v = coefficients(self, 1, spans[1], ends[1][0], ends[1][1], degrees[1])?;
            let mut points = vec![HPoint::ZERO; degree + 1];
            for (j, vc) in v.iter().enumerate() {
                for (i, uc) in u.iter().enumerate() {
                    let point = self.point(spans[0] - self.degree[0] + i, spans[1] - self.degree[1] + j);
                    for (vj, &cv) in vc.iter().enumerate() {
                        for (ui, &cu) in uc.iter().enumerate() {
                            let factor = binomial(degrees[0], ui) * binomial(degrees[1], vj) / binomial(degree, ui + vj);
                            let slot = points.get_mut(ui + vj).ok_or_else(|| SplineError::Invalid("invalid parameter-line coefficient".into()))?;
                            *slot += point * (cu * cv * factor);
                        }
                    }
                }
            }
            if let (Some(previous), Some(first)) = (ctrl.last(), points.first()) {
                let (Some(p), Some(q)) = (previous.project(), first.project()) else {
                    return invalid("parameter-line endpoint weights vanish");
                };
                if p.dist(q) > 1e-9 * (1.0 + p.len().max(q.len())) {
                    return Err(SplineError::Unsupported("parameter line crosses a discontinuous surface".into()));
                }
                let scale = previous.w / first.w;
                for point in &mut points {
                    *point = *point * scale;
                }
            }
            if points.iter().any(|p| !p.is_finite() || p.w <= 0.0) {
                return invalid("parameter-line weights must remain positive and finite");
            }
            ctrl.extend(points.into_iter().skip(usize::from(piece > 0)));
            if piece + 2 == cuts.len() {
                knots.extend(std::iter::repeat_n(1.0, degree + 1));
            } else {
                knots.extend(std::iter::repeat_n(hi, degree));
            }
        }
        Ok((knots, ctrl))
    }
}
