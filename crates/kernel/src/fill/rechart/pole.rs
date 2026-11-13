//! Exact tensor Bernstein composition with the Duffy map of a triangle:
//! P(r,t) = A + r ((1-t)(B-A) + t(C-A)). The side r=0 is
//! explicitly collapsed to A. Positive Bernstein products retain rational
//! weights and avoid a power-basis solve near a cusp.
use solidcraft_geom::bspline::{HPoint, MAX_SPLINE_DEGREE, NurbsSurface};

use crate::{KernelError, Result};

fn invalid(message: &str) -> KernelError {
    KernelError::Invalid(format!("subdivision pole chart: {message}"))
}

/// Multiply a tensor Bernstein polynomial of degree (d,d) by one bilinear
/// factor. NURBS Book Bernstein-product coefficients are binomial ratios;
/// with a linear factor they reduce to the two ratios below.
fn multiply(input: &[f64], degree: usize, factor: [f64; 4]) -> Result<Vec<f64>> {
    let width = degree + 1;
    let next = width + 1;
    let mut output = vec![0.0; next * next];
    for j in 0..width {
        for i in 0..width {
            let value = *input.get(i + width * j).ok_or_else(|| invalid("incomplete Bernstein coefficient grid"))?;
            for b in 0..2 {
                for a in 0..2 {
                    let along_r = if a == 0 { width - i } else { i + 1 } as f64 / width as f64;
                    let along_t = if b == 0 { width - j } else { j + 1 } as f64 / width as f64;
                    let factor = *factor.get(a + 2 * b).ok_or_else(|| invalid("incomplete bilinear factor"))?;
                    *output.get_mut(i + a + next * (j + b)).ok_or_else(|| invalid("Bernstein product index"))? += value * factor * along_r * along_t;
                }
            }
        }
    }
    Ok(output)
}

fn binomial(n: usize, k: usize) -> f64 {
    (0..k).fold(1.0, |value, i| value * (n - i) as f64 / (i + 1) as f64)
}

/// One triangle of a clamped rational Bézier tensor, corners numbered around
/// its unit square [00,10,11,01], with corners[0] at the explicit pole.
pub(super) fn triangle(source: &NurbsSurface, corners: [usize; 3]) -> Result<NurbsSurface> {
    let [p, q] = source.degree();
    let degree = p + q;
    let domain = source.domain();
    let clamped = source.degree().into_iter().enumerate().all(|(d, degree)| {
        source.knots()[d].iter().take(degree + 1).all(|&k| k == domain[d][0])
            && source.knots()[d].iter().rev().take(degree + 1).all(|&k| k == domain[d][1])
    });
    if degree > MAX_SPLINE_DEGREE || source.count() != [p + 1, q + 1] || !clamped {
        return Err(invalid("a pole chart needs one bounded Bezier tensor"));
    }
    let positions = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let a = *positions.get(corners[0]).ok_or_else(|| invalid("triangle corner"))?;
    let b = *positions.get(corners[1]).ok_or_else(|| invalid("triangle corner"))?;
    let c = *positions.get(corners[2]).ok_or_else(|| invalid("triangle corner"))?;
    if (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]) <= 0.0 {
        return Err(invalid("triangle orientation must follow the source"));
    }
    let u = [a[0], b[0], a[0], c[0]];
    let v = [a[1], b[1], a[1], c[1]];
    let width = degree + 1;
    let mut controls = vec![HPoint::ZERO; width * width];
    for j in 0..=q {
        for i in 0..=p {
            let mut coefficient = vec![1.0];
            let mut current = 0;
            for (power, count, factor) in [(i, p, u), (j, q, v)] {
                for k in 0..count {
                    let factor = if k < power { factor } else { factor.map(|x| 1.0 - x) };
                    coefficient = multiply(&coefficient, current, factor)?;
                    current += 1;
                }
            }
            let point = source.point(i, j) * (binomial(p, i) * binomial(q, j));
            for (output, weight) in controls.iter_mut().zip(coefficient) {
                *output += point * weight;
            }
        }
    }
    // This identity is exact: r=0 is A at every t. Keep the homogeneous row
    // bit-identical so the record represents a pole, including rational input.
    let pole = source.point(if a[0] == 0.0 { 0 } else { p }, if a[1] == 0.0 { 0 } else { q });
    for j in 0..width {
        *controls.get_mut(width * j).ok_or_else(|| invalid("pole row index"))? = pole;
    }
    let knots = std::iter::repeat_n(0.0, width).chain(std::iter::repeat_n(1.0, width)).collect::<Vec<_>>();
    NurbsSurface::from_homogeneous([degree, degree], [knots.clone(), knots], controls).map_err(|e| invalid(&e.to_string()))
}
