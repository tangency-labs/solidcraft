//! Exact straight parameter-line restriction of a positive rational tensor.
//!
//! The tensor-product Bernstein bases are multiplied in their original order.
//! Knot cuts, endpoint normalization and local-frame accumulation retain the
//! authoring operator's arithmetic and shared work accounting.
use crate::eval::pcurve::find_span;
use crate::eval::plan::basis::{binomial, local_basis, Side};
use crate::eval::vec::VecExt as _;
use crate::Vec3;

const MAX_SPLINE_DEGREE: usize = crate::eval::plan::MAX_SPLINE_DEGREE;
const MAX_KNOTS: usize = 1 << 14;
const MAX_SURFACE_POINTS: usize = 1 << 20;

/// Failure categories retained by the spline authoring adapter.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum Error {
    /// Malformed source or extraction parameters.
    #[error("{0}")]
    Invalid(String),
    /// The source cannot be represented by one supported continuous curve.
    #[error("{0}")]
    Unsupported(String),
    /// An explicit storage or shared work budget was exceeded.
    #[error("{0}")]
    TooLarge(String),
}
type Result<T> = std::result::Result<T, Error>;
fn invalid<T>(message: &str) -> Result<T> {
    Err(Error::Invalid(message.into()))
}
fn check_knots(knots: &[f64]) -> Result<()> {
    if knots.len() > MAX_KNOTS {
        return Err(Error::TooLarge(format!(
            "{} knots (at most {MAX_KNOTS})",
            knots.len()
        )));
    }
    if knots.iter().any(|value| !value.is_finite()) {
        return invalid("a knot is not a finite number");
    }
    if knots.windows(2).any(|w| matches!(w, [a,b] if b<a)) {
        return invalid("knots must not decrease");
    }
    Ok(())
}

/// Coefficients when a single local basis is split by one new knot.
pub fn split_basis(knots: &[f64], knot: f64) -> Option<(f64, f64)> {
    let degree = knots.len().checked_sub(2)?;
    let (first, second, before_last, last) = (
        *knots.first()?,
        *knots.get(1)?,
        *knots.get(degree)?,
        *knots.get(degree + 1)?,
    );
    if !(knot >= first && knot <= last) {
        return None;
    }
    Some((
        if knot >= before_last {
            1.0
        } else {
            (knot - first) / (before_last - first)
        },
        if knot <= second {
            1.0
        } else {
            (last - knot) / (last - second)
        },
    ))
}

/// Insert one local knot and retain the first and last original-size vectors.
fn split_knots<T: Copy + PartialOrd>(knots: &[T], knot: T) -> [Vec<T>; 2] {
    let at = knots
        .iter()
        .position(|value| *value > knot)
        .unwrap_or(knots.len());
    let merged: Vec<_> = knots
        .iter()
        .take(at)
        .copied()
        .chain(std::iter::once(knot))
        .chain(knots.iter().skip(at).copied())
        .collect();
    [
        merged
            .get(..knots.len())
            .map(<[T]>::to_vec)
            .unwrap_or_default(),
        merged.get(1..).map(<[T]>::to_vec).unwrap_or_default(),
    ]
}

/// Bernstein coefficients of one local B-spline basis on a knot-free interval.
pub fn basis_bezier(knots: &[f64], a: f64, b: f64) -> Result<Vec<f64>> {
    let Some(degree) = knots.len().checked_sub(2) else {
        return invalid("a basis function needs at least two knots");
    };
    if degree > MAX_SPLINE_DEGREE {
        return Err(Error::Unsupported(format!(
            "degree {degree} (at most {MAX_SPLINE_DEGREE})"
        )));
    }
    check_knots(knots)?;
    if !(a.is_finite() && b.is_finite() && a < b) {
        return invalid("a Bézier interval must be finite and non-empty");
    }
    if knots.iter().any(|&value| value > a && value < b) {
        return invalid("a knot lies inside the Bézier interval");
    }
    let mut output = vec![0.0; degree + 1];
    let mut work = vec![(knots.to_vec(), 1.0)];
    let mut steps = 0usize;
    while let Some((knots, coefficient)) = work.pop() {
        steps += 1;
        if steps > 1 << 12 {
            return Err(Error::TooLarge("Bézier extraction did not converge".into()));
        }
        let (Some(&lo), Some(&hi)) = (knots.first(), knots.last()) else {
            continue;
        };
        if hi <= a || lo >= b || coefficient == 0.0 {
            continue;
        }
        let insert = if knots.iter().any(|&value| value < a) {
            Some(a)
        } else if knots.iter().any(|&value| value > b) {
            Some(b)
        } else {
            None
        };
        if let Some(knot) = insert {
            let (first, second) = split_basis(&knots, knot)
                .ok_or_else(|| Error::Invalid("knot insertion outside the support".into()))?;
            let [left, right] = split_knots(&knots, knot);
            work.push((left, coefficient * first));
            work.push((right, coefficient * second));
        } else {
            let count = knots.iter().filter(|&&value| value == b).count();
            if let Some(slot) = count.checked_sub(1).and_then(|index| output.get_mut(index)) {
                *slot += coefficient;
            }
        }
    }
    Ok(output)
}

struct Surface<'a, C, F> {
    degree: [usize; 2],
    knots: &'a [Vec<f64>; 2],
    count: [usize; 2],
    controls: &'a [C],
    components: F,
}
impl<C, F: Fn(&C) -> [f64; 4]> Surface<'_, C, F> {
    fn domain(&self) -> Result<[[f64; 2]; 2]> {
        let mut domain = [[0.0; 2]; 2];
        for (((out, knots), degree), count) in domain
            .iter_mut()
            .zip(self.knots)
            .zip(self.degree)
            .zip(self.count)
        {
            *out = [
                *knots
                    .get(degree)
                    .ok_or_else(|| Error::Invalid("missing parameter-line domain".into()))?,
                *knots
                    .get(count)
                    .ok_or_else(|| Error::Invalid("missing parameter-line domain".into()))?,
            ];
        }
        Ok(domain)
    }
    fn point(&self, u: usize, v: usize) -> Result<[f64; 4]> {
        if u >= self.count[0] || v >= self.count[1] {
            return invalid("missing parameter-line control");
        }
        self.controls
            .get(v * self.count[0] + u)
            .map(&self.components)
            .ok_or_else(|| Error::Invalid("missing parameter-line control".into()))
    }
    fn coefficients(
        &self,
        d: usize,
        span: usize,
        a: f64,
        b: f64,
        polynomial_degree: usize,
    ) -> Result<Vec<Vec<f64>>> {
        let degree = *self
            .degree
            .get(d)
            .ok_or_else(|| Error::Invalid("invalid parameter-line axis".into()))?;
        let knots = self
            .knots
            .get(d)
            .ok_or_else(|| Error::Invalid("invalid parameter-line axis".into()))?;
        let first = span
            .checked_sub(degree)
            .ok_or_else(|| Error::Invalid("invalid parameter-line span".into()))?;
        let domain = *self
            .domain()?
            .get(d)
            .ok_or_else(|| Error::Invalid("invalid parameter-line axis".into()))?;
        (first..=span)
            .map(|i| {
                let local = knots
                    .get(i..i + degree + 2)
                    .ok_or_else(|| Error::Invalid("missing parameter-line basis".into()))?;
                if a == b {
                    let side = if a >= domain[1] {
                        Side::Left
                    } else {
                        Side::Right
                    };
                    return Ok(vec![
                        *local_basis(local, a, side, 0).first().ok_or_else(
                            || Error::Invalid("missing parameter-line basis".into())
                        )?;
                        polynomial_degree + 1
                    ]);
                }
                let mut values = basis_bezier(local, a.min(b), a.max(b))?;
                if b < a {
                    values.reverse();
                }
                Ok(values)
            })
            .collect()
    }
}

fn project([x, y, z, w]: [f64; 4]) -> Option<Vec3> {
    let point = Vec3::new(x / w, y / w, z / w);
    (w.abs() > f64::MIN_POSITIVE && point.is_finite()).then_some(point)
}
fn charge(remaining: &mut usize, work: usize) -> Result<()> {
    *remaining = remaining.checked_sub(work).ok_or_else(|| {
        Error::TooLarge("parameter-line extraction exceeded its shared work budget".into())
    })?;
    Ok(())
}

/// Borrow an authoritative tensor and restrict it to one straight UV segment.
/// The result is a packed piecewise-Bezier rational curve on [0,1].
pub fn extract<C>(
    degree: [usize; 2],
    knots: &[Vec<f64>; 2],
    count: [usize; 2],
    controls: &[C],
    a: [f64; 2],
    b: [f64; 2],
    remaining: &mut usize,
    components: impl Fn(&C) -> [f64; 4],
) -> Result<(Vec<f64>, Vec<[f64; 4]>)> {
    if degree.iter().any(|&p| p == 0 || p > MAX_SPLINE_DEGREE)
        || count.iter().zip(degree).any(|(&n, p)| n <= p)
        || count[0].checked_mul(count[1]) != Some(controls.len())
        || controls.len() > MAX_SURFACE_POINTS
    {
        return invalid("invalid parameter-line tensor dimensions");
    }
    for ((knots, p), n) in knots.iter().zip(degree).zip(count) {
        if knots.len() != n + p + 1 {
            return invalid("invalid parameter-line tensor dimensions");
        }
    }
    let surface = Surface {
        degree,
        knots,
        count,
        controls,
        components,
    };
    let domain = surface.domain()?;
    if a == b
        || a.iter().zip(b).zip(domain).any(|((&a, b), [lo, hi])| {
            !a.is_finite() || !b.is_finite() || a < lo || a > hi || b < lo || b > hi
        })
    {
        return invalid(
            "a parameter line needs distinct finite endpoints inside the surface domain",
        );
    }
    let delta = [b[0] - a[0], b[1] - a[1]];
    let degrees = [
        if delta[0] == 0.0 { 0 } else { degree[0] },
        if delta[1] == 0.0 { 0 } else { degree[1] },
    ];
    let output_degree = degrees[0] + degrees[1];
    if output_degree > MAX_SPLINE_DEGREE {
        return Err(Error::Unsupported(
            "parameter-line degree exceeds the curve evaluator".into(),
        ));
    }
    charge(remaining, knots[0].len().saturating_add(knots[1].len()))?;
    for axis in knots {
        check_knots(axis)?;
    }
    let mut crossings = Vec::new();
    for (d, axis) in knots.iter().enumerate() {
        let dd = *delta
            .get(d)
            .ok_or_else(|| Error::Invalid("invalid parameter-line axis".into()))?;
        if dd == 0.0 {
            continue;
        }
        let ad = *a
            .get(d)
            .ok_or_else(|| Error::Invalid("invalid parameter-line axis".into()))?;
        let bd = *b
            .get(d)
            .ok_or_else(|| Error::Invalid("invalid parameter-line axis".into()))?;
        let p = *degree
            .get(d)
            .ok_or_else(|| Error::Invalid("invalid parameter-line axis".into()))?;
        for group in axis.chunk_by(|a, b| a == b) {
            let Some(&knot) = group.first() else { continue };
            if knot > ad.min(bd) && knot < ad.max(bd) {
                if group.len() > p {
                    let other = 1 - d;
                    let other_count = *count
                        .get(other)
                        .ok_or_else(|| Error::Invalid("invalid parameter-line axis".into()))?;
                    charge(remaining, other_count)?;
                    let next = axis
                        .partition_point(|&value| value <= knot)
                        .checked_sub(p + 1)
                        .ok_or_else(|| {
                            Error::Invalid("missing discontinuous boundary row".into())
                        })?;
                    let previous = next.checked_sub(1).ok_or_else(|| {
                        Error::Invalid("missing discontinuous boundary row".into())
                    })?;
                    for i in 0..other_count {
                        let (u, v) = if d == 0 {
                            ((previous, i), (next, i))
                        } else {
                            ((i, previous), (i, next))
                        };
                        if surface.point(u.0, u.1)? != surface.point(v.0, v.1)? {
                            return Err(Error::Unsupported(
                                "parameter line crosses a discontinuous surface".into(),
                            ));
                        }
                    }
                }
                let t = (knot - ad) / dd;
                if !(0.0 < t && t < 1.0) {
                    return invalid("a knot crossing rounds to a parameter-line endpoint");
                }
                crossings.push((t, d, knot));
            }
        }
    }
    let n = crossings.len().saturating_add(2);
    let log =
        n.checked_ilog2()
            .ok_or_else(|| Error::Invalid("missing parameter-line cuts".into()))? as usize;
    charge(remaining, n.saturating_mul(2 * (1 + log)))?;
    crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut cuts = vec![(0.0, a)];
    let mut axes = [false; 2];
    for (t, d, knot) in crossings {
        let last = cuts
            .last_mut()
            .ok_or_else(|| Error::Invalid("missing parameter-line endpoint".into()))?;
        if last.0 != t {
            cuts.push((t, [a[0] + delta[0] * t, a[1] + delta[1] * t]));
            axes = [false; 2];
        }
        let (_, uv) = cuts
            .last_mut()
            .ok_or_else(|| Error::Invalid("missing parameter-line knot".into()))?;
        let axis = axes
            .get_mut(d)
            .ok_or_else(|| Error::Invalid("invalid parameter-line axis".into()))?;
        let coordinate = uv
            .get_mut(d)
            .ok_or_else(|| Error::Invalid("invalid parameter-line axis".into()))?;
        if *axis && *coordinate != knot {
            return invalid("distinct knot crossings round to the same parameter");
        }
        *coordinate = knot;
        *axis = true;
    }
    cuts.push((1.0, b));
    let work = cuts
        .len()
        .saturating_mul(degree[0] + 1)
        .saturating_mul(degree[1] + 1)
        .saturating_mul(degrees[0] + 1)
        .saturating_mul(degrees[1] + 1);
    if cuts.len() > 257 || work > 2_000_000 {
        return Err(Error::TooLarge(
            "parameter-line extraction budget exceeded".into(),
        ));
    }
    charge(remaining, work)?;
    let origin = super::form_fill::rechart::chart::point(
        degree,
        knots,
        count,
        controls,
        a,
        &surface.components,
    )
    .ok_or_else(|| {
        Error::Invalid(format!(
            "the surface cannot be evaluated at ({}, {})",
            a[0], a[1]
        ))
    })?;
    let mut output_knots = vec![0.0; output_degree + 1];
    let mut output = Vec::<[f64; 4]>::new();
    let mut previous_scale: f64 = 0.0;
    for (piece, interval) in cuts.windows(2).enumerate() {
        let [(_, from), (hi, to)] = interval else {
            return invalid("missing parameter-line interval");
        };
        let mut spans = [0; 2];
        let mut ends = [[0.0; 2]; 2];
        for (d, axis) in knots.iter().enumerate() {
            let end = ends
                .get_mut(d)
                .ok_or_else(|| Error::Invalid("invalid parameter-line axis".into()))?;
            let span = spans
                .get_mut(d)
                .ok_or_else(|| Error::Invalid("invalid parameter-line axis".into()))?;
            let p = *degree
                .get(d)
                .ok_or_else(|| Error::Invalid("invalid parameter-line axis".into()))?;
            let n = *count
                .get(d)
                .ok_or_else(|| Error::Invalid("invalid parameter-line axis".into()))?;
            *end = [
                *from
                    .get(d)
                    .ok_or_else(|| Error::Invalid("invalid parameter-line axis".into()))?,
                *to.get(d)
                    .ok_or_else(|| Error::Invalid("invalid parameter-line axis".into()))?,
            ];
            *span = find_span(axis, p, n, end[0].min(end[1])).ok_or_else(|| {
                Error::Invalid("parameter-line span is outside the surface".into())
            })?;
            if axis
                .get(*span + 1)
                .is_none_or(|&last| last < end[0].max(end[1]))
            {
                return invalid("a knot crossing could not be resolved within the parameter line");
            }
        }
        let u = surface.coefficients(0, spans[0], ends[0][0], ends[0][1], degrees[0])?;
        let v = surface.coefficients(1, spans[1], ends[1][0], ends[1][1], degrees[1])?;
        let mut points = vec![[0.0; 4]; output_degree + 1];
        for (j, vc) in v.iter().enumerate() {
            for (i, uc) in u.iter().enumerate() {
                let [x, y, z, w] =
                    surface.point(spans[0] - degree[0] + i, spans[1] - degree[1] + j)?;
                let point = [x - origin.x * w, y - origin.y * w, z - origin.z * w, w];
                for (vj, &cv) in vc.iter().enumerate() {
                    for (ui, &cu) in uc.iter().enumerate() {
                        let factor = binomial(degrees[0], ui) * binomial(degrees[1], vj)
                            / binomial(output_degree, ui + vj);
                        let slot = points.get_mut(ui + vj).ok_or_else(|| {
                            Error::Invalid("invalid parameter-line coefficient".into())
                        })?;
                        for (slot, point) in slot.iter_mut().zip(point) {
                            *slot += point * (cu * cv * factor);
                        }
                    }
                }
            }
        }
        let start = points
            .first()
            .and_then(|p| project(*p))
            .ok_or_else(|| Error::Invalid("parameter-line endpoint weights vanish".into()))?;
        let scale = points
            .iter()
            .filter_map(|p| project(*p))
            .map(|p| p.dist(start))
            .fold(0.0, f64::max);
        if let (Some(previous), Some(first)) = (output.last(), points.first()) {
            let (Some(p), Some(q)) = (project(*previous), project(*first)) else {
                return invalid("parameter-line endpoint weights vanish");
            };
            if p.dist(q) > 1e-9 * previous_scale.max(scale) {
                return Err(Error::Unsupported(
                    "parameter line crosses a discontinuous surface".into(),
                ));
            }
            let scale = previous[3] / first[3];
            for point in &mut points {
                *point = point.map(|x| x * scale);
            }
        }
        previous_scale = scale;
        if points
            .iter()
            .any(|p| p.iter().any(|v| !v.is_finite()) || p[3] <= 0.0)
        {
            return invalid("parameter-line weights must remain positive and finite");
        }
        output.extend(points.into_iter().skip(usize::from(piece > 0)));
        if piece + 2 == cuts.len() {
            output_knots.extend(std::iter::repeat_n(1.0, output_degree + 1));
        } else {
            output_knots.extend(std::iter::repeat_n(*hi, output_degree));
        }
    }
    for point in &mut output {
        let [x, y, z, w] = *point;
        *point = [
            x + origin.x * w,
            y + origin.y * w,
            z + origin.z * w,
            w + 0.0,
        ];
        if point.iter().any(|v| !v.is_finite()) {
            return invalid("parameter-line controls must be finite");
        }
    }
    Ok((output_knots, output))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagonal_and_isoparametric_lines_have_the_analytic_controls_and_exact_work_debits() {
        let knots = [vec![0., 0., 1., 1.], vec![0., 0., 1., 1.]];
        let controls = [
            [0., 0., 0., 1.],
            [1., 0., 1., 1.],
            [0., 1., 1., 1.],
            [1., 1., 2., 1.],
        ];
        let mut work = 48;
        let (curve_knots, diagonal) = extract(
            [1, 1],
            &knots,
            [2, 2],
            &controls,
            [0., 0.],
            [1., 1.],
            &mut work,
            |h| *h,
        )
        .unwrap();
        assert_eq!(work, 0);
        assert_eq!(curve_knots, [0., 0., 0., 1., 1., 1.]);
        assert_eq!(
            diagonal,
            [[0., 0., 0., 1.], [0.5, 0.5, 1., 1.], [1., 1., 2., 1.]]
        );
        let (_, reverse) = extract(
            [1, 1],
            &knots,
            [2, 2],
            &controls,
            [1., 1.],
            [0., 0.],
            &mut 48,
            |h| *h,
        )
        .unwrap();
        assert_eq!(reverse, diagonal.into_iter().rev().collect::<Vec<_>>());
        let mut work = 32;
        let (curve_knots, iso) = extract(
            [1, 1],
            &knots,
            [2, 2],
            &controls,
            [0., 0.25],
            [1., 0.25],
            &mut work,
            |h| *h,
        )
        .unwrap();
        assert_eq!(work, 0);
        assert_eq!(curve_knots, [0., 0., 1., 1.]);
        assert_eq!(iso, [[0., 0.25, 0.25, 1.], [1., 0.25, 1.25, 1.]]);
    }

    #[test]
    fn rejects_hostile_dimensions_and_exhausted_work_before_extracting_controls() {
        let knots = [vec![0., 0., 1., 1.], vec![0., 0., 1., 1.]];
        let controls = [[0., 0., 0., 1.]; 4];
        let mut unlimited = usize::MAX;
        assert!(matches!(
            extract(
                [1, 1],
                &knots,
                [usize::MAX, 2],
                &controls,
                [0., 0.],
                [1., 1.],
                &mut unlimited,
                |h| *h
            ),
            Err(Error::Invalid(_))
        ));
        let mut work = 1;
        assert_eq!(
            extract(
                [1, 1],
                &knots,
                [2, 2],
                &controls,
                [0., 0.],
                [1., 1.],
                &mut work,
                |h| *h
            ),
            Err(Error::TooLarge(
                "parameter-line extraction exceeded its shared work budget".into()
            ))
        );
        assert_eq!(work, 1);
        assert!(extract(
            [1, 1],
            &knots,
            [2, 2],
            &controls,
            [0., 0.],
            [0., 0.],
            &mut unlimited,
            |h| *h
        )
        .is_err());
    }
}
