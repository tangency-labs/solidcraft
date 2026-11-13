//! Certified loop orientation from the exact rational pcurves.
//! Green's integral is enclosed by outward-rounded interval de Boor evaluation.
//! An inconclusive enclosure is subdivided, never used to choose an exterior.
use super::*;
use solidcore_record::brep::Curve2;

const MAX_WORK: usize = 262_144;
const MAX_DEPTH: usize = 16;
const MAX_KNOTS: usize = 65_536;

#[derive(Clone, Copy, Debug)]
struct Interval {
    lo: f64,
    hi: f64,
}
impl Interval {
    fn unknown() -> Self {
        Self { lo: f64::NEG_INFINITY, hi: f64::INFINITY }
    }
    fn finite(self) -> bool {
        self.lo.is_finite() && self.hi.is_finite()
    }
    fn rounded(lo: f64, hi: f64) -> Self {
        let interval = Self { lo: lo.next_down(), hi: hi.next_up() };
        if interval.finite() { interval } else { Self::unknown() }
    }
    fn point(x: f64) -> Self {
        if x.is_finite() { Self { lo: x, hi: x } } else { Self::unknown() }
    }
    fn add(self, b: Self) -> Self {
        if !self.finite() || !b.finite() {
            return Self::unknown();
        }
        Self::rounded(self.lo + b.lo, self.hi + b.hi)
    }
    fn neg(self) -> Self {
        Self { lo: -self.hi, hi: -self.lo }
    }
    fn sub(self, b: Self) -> Self {
        self.add(b.neg())
    }
    fn mul(self, b: Self) -> Self {
        if !self.finite() || !b.finite() {
            return Self::unknown();
        }
        let values = [self.lo * b.lo, self.lo * b.hi, self.hi * b.lo, self.hi * b.hi];
        if values.iter().any(|x| !x.is_finite()) {
            return Self::unknown();
        }
        Self::rounded(values.into_iter().fold(f64::INFINITY, f64::min), values.into_iter().fold(f64::NEG_INFINITY, f64::max))
    }
    fn div(self, b: Self) -> Option<Self> {
        if !self.finite() || !b.finite() || b.lo <= 0.0 {
            return None;
        }
        Some(self.mul(Self::rounded(1.0 / b.hi, 1.0 / b.lo)))
    }
}
type Point = [Interval; 3];

struct Rational {
    degree: usize,
    knots: Vec<f64>,
    controls: Vec<Point>,
    derivative: Vec<Point>,
    interval: [f64; 2],
    forward: bool,
}
impl Rational {
    fn new(curve: &Curve2, interval: [f64; 2], forward: bool, origin: [f64; 2]) -> Result<Self> {
        let (knots, raw) = match curve {
            Curve2::Spline { knots, points } => (knots, points.iter().map(|p| [p[0], p[1], 1.0]).collect::<Vec<_>>()),
            Curve2::RationalSpline { knots, homogeneous } => (knots, homogeneous.clone()),
        };
        let degree = knots
            .len()
            .checked_sub(raw.len() + 1)
            .filter(|p| *p > 0 && *p <= solidcraft_geom::bspline::MAX_SPLINE_DEGREE)
            .ok_or_else(|| invalid("invalid trim pcurve degree"))?;
        let controls: Vec<_> = raw
            .into_iter()
            .map(|p| {
                let w = Interval::point(p[2]);
                [Interval::point(p[0]).sub(Interval::point(origin[0]).mul(w)), Interval::point(p[1]).sub(Interval::point(origin[1]).mul(w)), w]
            })
            .collect();
        let derivative = controls
            .windows(2)
            .enumerate()
            .map(|(i, p)| {
                let left = *knots.get(i + 1).ok_or_else(|| invalid("trim derivative knots"))?;
                let right = *knots.get(i + degree + 1).ok_or_else(|| invalid("trim derivative knots"))?;
                if right == left {
                    return Ok([Interval::point(0.0); 3]);
                }
                let scale = Interval::point(degree as f64)
                    .div(Interval::point(right).sub(Interval::point(left)))
                    .ok_or_else(|| invalid("trim derivative knot interval"))?;
                Ok(std::array::from_fn(|d| p[1][d].sub(p[0][d]).mul(scale)))
            })
            .collect::<Result<Vec<_>>>()?;
        if controls.iter().chain(&derivative).flatten().any(|value| !value.finite()) {
            return Err(invalid("trim orientation arithmetic exceeds finite bounds"));
        }
        Ok(Self { degree, knots: knots.clone(), controls, derivative, interval, forward })
    }
    fn value(degree: usize, knots: &[f64], controls: &[Point], range: [f64; 2]) -> Option<Point> {
        let mid = range[0] * 0.5 + range[1] * 0.5;
        if range[0] < range[1] && mid == range[1] {
            return None;
        }
        let span = solidcraft_geom::bspline::find_span(knots, degree, controls.len(), mid)?;
        let mut row = controls.get(span.checked_sub(degree)?..=span)?.to_vec();
        let t = Interval { lo: range[0], hi: range[1] };
        for r in 1..=degree {
            for j in (r..=degree).rev() {
                let i = span + j - degree;
                let a = *knots.get(i)?;
                let b = *knots.get(i + degree - r + 1)?;
                let alpha = if b == a { Interval::point(0.0) } else { t.sub(Interval::point(a)).div(Interval::point(b).sub(Interval::point(a)))? };
                let one = Interval::point(1.0).sub(alpha);
                let left = *row.get(j - 1)?;
                let right = *row.get(j)?;
                *row.get_mut(j)? = std::array::from_fn(|d| left[d].mul(one).add(right[d].mul(alpha)));
            }
        }
        row.last().copied()
    }
    fn point(&self, t: f64) -> Option<[Interval; 2]> {
        let p = Self::value(self.degree, &self.knots, &self.controls, [t, t])?;
        Some([p[0].div(p[2])?, p[1].div(p[2])?])
    }
    fn integral(&self, range: [f64; 2]) -> Option<Interval> {
        let p = Self::value(self.degree, &self.knots, &self.controls, range)?;
        let derivative_knots = self.knots.get(1..self.knots.len().checked_sub(1)?)?;
        let d = Self::value(self.degree - 1, derivative_knots, &self.derivative, range)?;
        let numerator = p[0].mul(d[1]).sub(p[1].mul(d[0]));
        let integral = numerator.div(p[2].mul(p[2]))?.mul(Interval::point(range[1]).sub(Interval::point(range[0])));
        Some(if self.forward { integral } else { integral.neg() })
    }
    fn ranges(&self) -> Vec<[f64; 2]> {
        let [a, b] = self.interval;
        let mut points = vec![a];
        points.extend(self.knots.iter().copied().filter(|&t| t > a && t < b));
        points.push(b);
        points.sort_by(f64::total_cmp);
        points.dedup();
        points.windows(2).map(|p| [p[0], p[1]]).collect()
    }
}

pub(super) fn positive_loop(face: &b::Face, body: &Body) -> Result<usize> {
    let edge_loops: Vec<_> = face.loops.iter().enumerate().filter_map(|(i, l)| matches!(l, Loop::Edges { .. }).then_some(i)).collect();
    // A valid record has one exterior. Vertex loops are collapsed interior boundaries.
    if let [only] = edge_loops.as_slice() {
        return Ok(*only);
    }
    let knots = face
        .loops
        .iter()
        .filter_map(|l| match l {
            Loop::Edges { edges } => Some(edges),
            _ => None,
        })
        .flatten()
        .map(|u| match &u.pcurve {
            Curve2::Spline { knots, .. } | Curve2::RationalSpline { knots, .. } => knots.len(),
        })
        .fold(0usize, usize::saturating_add);
    if knots > MAX_KNOTS {
        return Err(KernelError::TooLarge("trim orientation exceeds 65536 pcurve knots".into()));
    }
    let mut outer = None;
    let mut remaining = MAX_WORK;
    for (i, wire) in face.loops.iter().enumerate() {
        let Loop::Edges { edges } = wire else { continue };
        let mut raw = Vec::with_capacity(edges.len());
        for use_ in edges {
            let edge = body.record().edges.get(use_.edge as usize).ok_or_else(|| invalid("missing trim orientation edge"))?;
            raw.push((&use_.pcurve, edge.interval, use_.forward));
        }
        let first = raw.first().ok_or_else(|| invalid("empty face trim"))?;
        let origin = solidcraft_geom::brep::pcurve::eval(first.0, first.1[0]).ok_or_else(|| invalid("trim start has no parameters"))?;
        let curves = raw.into_iter().map(|(c, r, f)| Rational::new(c, r, f, origin)).collect::<Result<Vec<_>>>()?;
        let mut connectors = Interval::point(0.0);
        for (previous, next) in curves.iter().zip(curves.iter().cycle().skip(1)).take(curves.len()) {
            let end = previous.point(previous.interval[usize::from(previous.forward)]).ok_or_else(|| invalid("trim end cannot be enclosed"))?;
            let start = next.point(next.interval[usize::from(!next.forward)]).ok_or_else(|| invalid("trim start cannot be enclosed"))?;
            // Periodic/pole joins close through the parameter-plane connector.
            connectors = connectors.add(end[0].mul(start[1]).sub(end[1].mul(start[0])));
        }
        let mut pieces: Vec<_> = curves.iter().enumerate().flat_map(|(c, curve)| curve.ranges().into_iter().map(move |r| (c, r))).collect();
        let mut sign = None;
        for _ in 0..=MAX_DEPTH {
            if pieces.len() > remaining {
                break;
            }
            remaining -= pieces.len();
            let mut area = Some(connectors);
            for &(c, range) in &pieces {
                area = area.zip(curves.get(c).and_then(|curve| curve.integral(range))).map(|(a, b)| a.add(b));
            }
            if let Some(area) = area {
                if area.lo > 0.0 {
                    sign = Some(true);
                    break;
                }
                if area.hi < 0.0 {
                    sign = Some(false);
                    break;
                }
            }
            let mut next = Vec::with_capacity(pieces.len().saturating_mul(2));
            for (c, [a, b]) in pieces {
                let mid = a * 0.5 + b * 0.5;
                if !(mid > a && mid < b) {
                    return Err(invalid("trim orientation reaches parameter precision limit"));
                }
                next.extend([(c, [a, mid]), (c, [mid, b])]);
            }
            pieces = next;
        }
        match sign {
            Some(true) if outer.is_none() => outer = Some(i),
            Some(true) => return Err(invalid("face has multiple positively oriented exterior trims")),
            Some(false) => {}
            None => return Err(KernelError::TooLarge("exact trim orientation could not be certified within its work budget".into())),
        }
    }
    outer.ok_or_else(|| invalid("face has no positively oriented exterior trim"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overflow_and_nan_remain_unbounded_and_large_finite_knots_refuse_qualification() {
        let overflow = Interval::point(f64::MAX).add(Interval::point(f64::MAX));
        for result in [overflow.mul(Interval::point(0.0)), overflow.sub(overflow), Interval::point(f64::NAN).mul(Interval::point(1.0))] {
            assert_eq!(result.lo, f64::NEG_INFINITY);
            assert_eq!(result.hi, f64::INFINITY);
        }
        let a = 1e200_f64;
        let b = a.next_up();
        let curve = Curve2::Spline { knots: vec![a, a, b, b], points: vec![[-1e308, -1e308], [1e308, 1e308]] };
        assert!(Rational::new(&curve, [a, b], true, [0.0, 0.0]).is_err());
    }
}
