//! Restrict a tensor surface to a straight line in its parameter plane.
use super::*;
use crate::prelude::*;

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
        let mut remaining = usize::MAX;
        self.parameter_line_bounded(a, b, &mut remaining)
    }

    /// Exact restriction with a shared work budget. Charge the knot scan,
    /// actual parameter crossings, and tensor basis products used by this line.
    pub fn parameter_line_bounded(&self, a: [f64; 2], b: [f64; 2], remaining: &mut usize) -> SplineResult<(Vec<f64>, Vec<HPoint>)> {
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
        let knot_count = self.knots[0].len().saturating_add(self.knots[1].len());
        charge(remaining, knot_count)?;
        let mut crossings = Vec::new();
        for d in 0..2 {
            if delta[d] == 0.0 {
                continue;
            }
            for &knot in &self.knots[d] {
                if knot > a[d].min(b[d]) && knot < a[d].max(b[d]) {
                    let t = (knot - a[d]) / delta[d];
                    if !(0.0 < t && t < 1.0) {
                        return invalid("a knot crossing rounds to a parameter-line endpoint");
                    }
                    crossings.push((t, d, knot));
                }
            }
        }
        let n = crossings.len().saturating_add(2);
        charge(remaining, n.saturating_mul(2 * (1 + n.ilog2() as usize)))?;
        crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut cuts = vec![(0.0, a)];
        let mut axes = [false; 2];
        for (t, d, knot) in crossings {
            let last = cuts.last_mut().ok_or_else(|| SplineError::Invalid("missing parameter-line endpoint".into()))?;
            if last.0 != t {
                cuts.push((t, [a[0] + delta[0] * t, a[1] + delta[1] * t]));
                axes = [false; 2];
            }
            let (_, uv) = cuts.last_mut().ok_or_else(|| SplineError::Invalid("missing parameter-line knot".into()))?;
            if axes[d] && uv[d] != knot {
                return invalid("distinct knot crossings round to the same parameter");
            }
            uv[d] = knot;
            axes[d] = true;
        }
        cuts.push((1.0, b));
        let work = cuts
            .len()
            .saturating_mul(self.degree[0] + 1)
            .saturating_mul(self.degree[1] + 1)
            .saturating_mul(degrees[0] + 1)
            .saturating_mul(degrees[1] + 1);
        if cuts.len() > 257 || work > 2_000_000 {
            return Err(SplineError::TooLarge("parameter-line extraction budget exceeded".into()));
        }
        charge(remaining, work)?;
        // Accumulate controls in a local frame so translation does not affect
        // the continuity test at the joins between extracted spans.
        let origin = self.eval(a[0], a[1])?;
        let mut knots = vec![0.0; degree + 1];
        let mut ctrl = Vec::<HPoint>::new();
        let mut previous_scale: f64 = 0.0;
        for (piece, interval) in cuts.windows(2).enumerate() {
            let ((_, from), (hi, to)) = (interval[0], interval[1]);
            let mut spans = [0; 2];
            let mut ends = [[0.0; 2]; 2];
            for d in 0..2 {
                ends[d] = [from[d], to[d]];
                spans[d] = find_span(&self.knots[d], self.degree[d], self.count[d], from[d].min(to[d]))
                    .ok_or_else(|| SplineError::Invalid("parameter-line span is outside the surface".into()))?;
                if self.knots[d].get(spans[d] + 1).is_none_or(|&end| end < from[d].max(to[d])) {
                    return invalid("a knot crossing could not be resolved within the parameter line");
                }
            }
            let u = coefficients(self, 0, spans[0], ends[0][0], ends[0][1], degrees[0])?;
            let v = coefficients(self, 1, spans[1], ends[1][0], ends[1][1], degrees[1])?;
            let mut points = vec![HPoint::ZERO; degree + 1];
            for (j, vc) in v.iter().enumerate() {
                for (i, uc) in u.iter().enumerate() {
                    let point = self.point(spans[0] - self.degree[0] + i, spans[1] - self.degree[1] + j);
                    let point = point - HPoint { x: origin.x * point.w, y: origin.y * point.w, z: origin.z * point.w, w: 0.0 };
                    for (vj, &cv) in vc.iter().enumerate() {
                        for (ui, &cu) in uc.iter().enumerate() {
                            let factor = binomial(degrees[0], ui) * binomial(degrees[1], vj) / binomial(degree, ui + vj);
                            let slot = points.get_mut(ui + vj).ok_or_else(|| SplineError::Invalid("invalid parameter-line coefficient".into()))?;
                            *slot += point * (cu * cv * factor);
                        }
                    }
                }
            }
            let start =
                points.first().and_then(|p| p.project()).ok_or_else(|| SplineError::Invalid("parameter-line endpoint weights vanish".into()))?;
            let scale = points.iter().filter_map(|p| p.project()).map(|p| p.dist(start)).fold(0.0, f64::max);
            if let (Some(previous), Some(first)) = (ctrl.last(), points.first()) {
                let (Some(p), Some(q)) = (previous.project(), first.project()) else {
                    return invalid("parameter-line endpoint weights vanish");
                };
                if p.dist(q) > 1e-9 * previous_scale.max(scale) {
                    return Err(SplineError::Unsupported("parameter line crosses a discontinuous surface".into()));
                }
                let scale = previous.w / first.w;
                for point in &mut points {
                    *point = *point * scale;
                }
            }
            previous_scale = scale;
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
        for point in &mut ctrl {
            *point += HPoint { x: origin.x * point.w, y: origin.y * point.w, z: origin.z * point.w, w: 0.0 };
            if !point.is_finite() {
                return invalid("parameter-line controls must be finite");
            }
        }
        Ok((knots, ctrl))
    }
}

fn charge(remaining: &mut usize, work: usize) -> SplineResult<()> {
    *remaining =
        remaining.checked_sub(work).ok_or_else(|| SplineError::TooLarge("parameter-line extraction exceeded its shared work budget".into()))?;
    Ok(())
}
