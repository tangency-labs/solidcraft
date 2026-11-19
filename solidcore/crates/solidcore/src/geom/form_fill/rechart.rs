//! Exact Bernstein composition of a rational tensor with an oriented Duffy triangle.
//! The collapsed side retains a bit-identical homogeneous pole column.
use crate::{Result, SolidError};

const OP: &str = "geom::form_fill::rechart";

/// A bounded rational tensor in U-varying-fastest order.
pub struct Tensor {
    /// Tensor degrees.
    pub degree: [usize; 2],
    /// Clamped knots in both directions.
    pub knots: [Vec<f64>; 2],
    /// Positive homogeneous controls.
    pub controls: Vec<[f64; 4]>,
}

fn bad(reason: &'static str) -> SolidError {
    SolidError::InvalidInput {
        operation: OP,
        reason,
    }
}

fn filled<T: Clone>(count: usize, value: T) -> Result<Vec<T>> {
    if count > 289 {
        return Err(bad("Duffy coefficient budget"));
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(count)
        .map_err(|_| bad("Duffy coefficient allocation"))?;
    output.resize(count, value);
    Ok(output)
}

fn multiply(input: &[f64], degree: usize, factor: [f64; 4]) -> Result<Vec<f64>> {
    let width = degree + 1;
    let next = width + 1;
    let mut output = filled(next * next, 0.)?;
    for j in 0..width {
        for i in 0..width {
            let value = *input
                .get(i + width * j)
                .ok_or_else(|| bad("incomplete Bernstein grid"))?;
            for b in 0..2 {
                for a in 0..2 {
                    let along_r = if a == 0 { width - i } else { i + 1 } as f64 / width as f64;
                    let along_t = if b == 0 { width - j } else { j + 1 } as f64 / width as f64;
                    let factor = *factor
                        .get(a + 2 * b)
                        .ok_or_else(|| bad("bilinear factor"))?;
                    *output
                        .get_mut(i + a + next * (j + b))
                        .ok_or_else(|| bad("Bernstein index"))? +=
                        value * factor * along_r * along_t;
                }
            }
        }
    }
    Ok(output)
}

fn binomial(n: usize, k: usize) -> f64 {
    (0..k).fold(1., |value, i| value * (n - i) as f64 / (i + 1) as f64)
}

/// Native chart parameters of three corners numbered around [00,10,11,01].
pub fn triangle_parameters(domain: [[f64; 2]; 2], corners: [usize; 3]) -> Result<[[f64; 2]; 3]> {
    if domain
        .iter()
        .any(|&[a, b]| !a.is_finite() || !b.is_finite() || a >= b)
    {
        return Err(bad("triangle domain"));
    }
    let positions = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]];
    let mut output = [[0.; 2]; 3];
    for (out, corner) in output.iter_mut().zip(corners) {
        let [u, v] = *positions
            .get(corner)
            .ok_or_else(|| bad("triangle corner"))?;
        *out = [
            domain[0][0] + (domain[0][1] - domain[0][0]) * u,
            domain[1][0] + (domain[1][1] - domain[1][0]) * v,
        ];
        if !out.iter().all(|x| x.is_finite()) {
            return Err(bad("triangle parameter overflow"));
        }
    }
    Ok(output)
}

/// Compose a single clamped rational Bezier tensor with an oriented Duffy map.
pub fn triangle(
    degree: [usize; 2],
    knots: [&[f64]; 2],
    controls: &[[f64; 4]],
    corners: [usize; 3],
) -> Result<Tensor> {
    let [p, q] = degree;
    let sum = p
        .checked_add(q)
        .filter(|&d| d <= 16 && p > 0 && q > 0)
        .ok_or_else(|| bad("bounded Bezier tensor degree"))?;
    if controls.len() != (p + 1) * (q + 1)
        || controls
            .iter()
            .any(|h| h.iter().any(|x| !x.is_finite()) || h[3] <= 0.)
    {
        return Err(bad("positive complete Bezier controls"));
    }
    for (degree, knots) in degree.into_iter().zip(knots) {
        let (a, b) = (
            *knots.first().ok_or_else(|| bad("Bezier knots"))?,
            *knots.last().ok_or_else(|| bad("Bezier knots"))?,
        );
        if knots.len() != 2 * (degree + 1)
            || !a.is_finite()
            || !b.is_finite()
            || a >= b
            || knots.iter().take(degree + 1).any(|&k| k != a)
            || knots.iter().skip(degree + 1).any(|&k| k != b)
        {
            return Err(bad("one clamped Bezier knot span required"));
        }
    }
    let [a, b, c] = triangle_parameters([[0., 1.], [0., 1.]], corners)?;
    if (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]) <= 0. {
        return Err(bad("triangle must follow the source orientation"));
    }
    let u = [a[0], b[0], a[0], c[0]];
    let v = [a[1], b[1], a[1], c[1]];
    let width = sum + 1;
    let mut output = filled(width * width, [0.; 4])?;
    for j in 0..=q {
        for i in 0..=p {
            let mut coefficient = vec![1.];
            let mut current = 0;
            for (power, count, factor) in [(i, p, u), (j, q, v)] {
                for k in 0..count {
                    coefficient = multiply(
                        &coefficient,
                        current,
                        if k < power {
                            factor
                        } else {
                            factor.map(|x| 1. - x)
                        },
                    )?;
                    current += 1;
                }
            }
            let point = controls
                .get(i + (p + 1) * j)
                .ok_or_else(|| bad("source control index"))?;
            let scale = binomial(p, i) * binomial(q, j);
            for (out, weight) in output.iter_mut().zip(coefficient) {
                for (out, point) in out.iter_mut().zip(point) {
                    *out += point * scale * weight;
                }
            }
        }
    }
    let pole_i = if a[0] == 0. { 0 } else { p };
    let pole_j = if a[1] == 0. { 0 } else { q };
    let pole = *controls
        .get(pole_i + (p + 1) * pole_j)
        .ok_or_else(|| bad("pole index"))?;
    for j in 0..width {
        *output
            .get_mut(width * j)
            .ok_or_else(|| bad("pole row index"))? = pole;
    }
    if output
        .iter()
        .any(|h| h.iter().any(|x| !x.is_finite()) || h[3] <= 0.)
    {
        return Err(bad("Duffy homogeneous overflow"));
    }
    let knots = std::iter::repeat_n(0., width)
        .chain(std::iter::repeat_n(1., width))
        .collect::<Vec<_>>();
    Ok(Tensor {
        degree: [sum, sum],
        knots: [knots.clone(), knots],
        controls: output,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::plan::nurbs_derivs;

    #[test]
    fn duffy_composition_preserves_rational_surface_and_explicit_pole() {
        let knots = [vec![0., 0., 0., 1., 1., 1.], vec![0., 0., 0., 1., 1., 1.]];
        let controls = (0..3)
            .flat_map(|j| {
                (0..3).map(move |i| {
                    let (u, v) = (f64::from(i) / 2., f64::from(j) / 2.);
                    let w = 1. + f64::from(i + j) / 8.;
                    [u * w, v * w, u * v * w, w]
                })
            })
            .collect::<Vec<_>>();
        let tensor = triangle([2, 2], [&knots[0], &knots[1]], &controls, [0, 1, 3]).unwrap();
        let degree = tensor.degree[0];
        let count = degree + 1;
        for j in 0..count {
            assert_eq!(tensor.controls[j * count], controls[0]);
        }
        for r in [0., 0.125, 0.5, 1.] {
            for t in [0., 0.2, 0.8, 1.] {
                let source = nurbs_derivs(
                    [2, 2],
                    &knots,
                    [3, 3],
                    &controls,
                    [r * (1. - t), r * t],
                    |h| *h,
                )
                .unwrap()
                .0;
                let actual = nurbs_derivs(
                    tensor.degree,
                    &tensor.knots,
                    [count, count],
                    &tensor.controls,
                    [r, t],
                    |h| *h,
                )
                .unwrap()
                .0;
                assert!(actual.distance(source) < 1e-12, "r={r},t={t}");
            }
        }
        assert!(triangle([2, 2], [&knots[0], &knots[1]], &controls, [0, 3, 1]).is_err());
        assert!(triangle(
            [usize::MAX, 2],
            [&knots[0], &knots[1]],
            &controls,
            [0, 1, 3]
        )
        .is_err());
    }
}
