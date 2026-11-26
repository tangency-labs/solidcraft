//! Clamped global interpolation using averaged knots and a pivoted linear solve.
//! The same operator fits curve coordinates and tensor rows. See Piegl and Tiller,
//! The NURBS Book, algorithm A9.1.

use crate::eval::plan::{basis_ders, span_of, MAX_SPLINE_DEGREE};
use crate::{Result, SolidError, Vec3};

fn invalid(reason: &'static str) -> SolidError {
    SolidError::InvalidInput {
        operation: "spline interpolation",
        reason,
    }
}

/// Knot vector and fitted spline controls, in parameter order.
pub struct Interpolated<const N: usize> {
    /// Clamped knot vector.
    pub knots: Vec<f64>,
    /// Interpolating controls.
    pub controls: Vec<[f64; N]>,
}

/// Interpolate finite samples at strictly increasing parameters. The dense
/// solve is limited to 1,024 stations and at most 4,096 values per station.
pub fn curve<const N: usize>(
    parameters: &[f64],
    points: &[[f64; N]],
    degree: usize,
) -> Result<Interpolated<N>> {
    let system = System::new(parameters, degree)?;
    let controls = system.solve(points)?;
    Ok(Interpolated {
        knots: system.knots,
        controls,
    })
}

/// Cubic interpolation of a complete square grid with the same knots on both
/// axes. Samples are V-fast; the returned controls are U-fast.
pub fn cubic_tensor(samples: &[Vec3], parameters: &[f64]) -> Result<Interpolated<3>> {
    let n = parameters.len();
    if !(4..=65).contains(&n) || samples.len() != n * n {
        return Err(invalid(
            "a cubic tensor fit requires a complete bounded grid",
        ));
    }
    let system = System::new(parameters, 3)?;
    let mut first = vec![[0.0; 3]; n * n];
    let mut row = vec![[0.0; 3]; n];
    for v in 0..n {
        for (u, dst) in row.iter_mut().enumerate() {
            *dst = samples
                .get(u * n + v)
                .ok_or_else(|| invalid("missing interpolation sample"))?
                .to_array();
        }
        for (u, point) in system.solve(&row)?.into_iter().enumerate() {
            *first
                .get_mut(u * n + v)
                .ok_or_else(|| invalid("missing fitted row"))? = point;
        }
    }
    let mut controls = vec![[0.0; 3]; n * n];
    for (u, source) in first.chunks_exact(n).enumerate() {
        for (v, point) in system.solve(source)?.into_iter().enumerate() {
            *controls
                .get_mut(v * n + u)
                .ok_or_else(|| invalid("missing fitted column"))? = point;
        }
    }
    Ok(Interpolated {
        knots: system.knots,
        controls,
    })
}

struct System {
    knots: Vec<f64>,
    rows: Vec<Vec<f64>>,
}

impl System {
    fn new(parameters: &[f64], degree: usize) -> Result<Self> {
        let n = parameters.len();
        if !(1..=MAX_SPLINE_DEGREE).contains(&degree)
            || n <= degree
            || n > 1024
            || parameters.iter().any(|t| !t.is_finite())
            || parameters
                .windows(2)
                .any(|pair| matches!(pair, [a, b] if a >= b))
        {
            return Err(invalid(
                "the parameters must be finite and increasing with a bounded spline degree and count",
            ));
        }
        let first = *parameters
            .first()
            .ok_or_else(|| invalid("missing first interpolation parameter"))?;
        let last = *parameters
            .last()
            .ok_or_else(|| invalid("missing last interpolation parameter"))?;
        let mut knots = vec![first; degree + 1];
        for window in parameters.windows(degree).skip(1).take(n - degree - 1) {
            knots.push(window.iter().sum::<f64>() / degree as f64);
        }
        knots.extend(std::iter::repeat_n(last, degree + 1));
        if knots.iter().any(|v| !v.is_finite()) {
            return Err(invalid("averaging interpolation knots overflowed"));
        }
        let mut rows = vec![vec![0.0; n]; n];
        for (row, &t) in rows.iter_mut().zip(parameters) {
            let span = span_of(&knots, degree, n, t);
            let [basis, _] = basis_ders(&knots, span, degree, t)
                .ok_or_else(|| invalid("cannot evaluate interpolation basis"))?;
            for (x, value) in row
                .iter_mut()
                .skip(span.saturating_sub(degree))
                .zip(basis.into_iter().take(degree + 1))
            {
                *x = value;
            }
        }
        Ok(Self { knots, rows })
    }

    fn solve<const N: usize>(&self, points: &[[f64; N]]) -> Result<Vec<[f64; N]>> {
        let n = self.rows.len();
        if points.len() != n
            || !(1..=4096).contains(&N)
            || n.saturating_mul(N) > 1_048_576
            || points.iter().flatten().any(|x| !x.is_finite())
        {
            return Err(invalid(
                "interpolation requires bounded finite data at each parameter",
            ));
        }
        let mut a = self.rows.clone();
        let mut q = points.to_vec();
        for col in 0..n {
            let pivot = a
                .iter()
                .enumerate()
                .skip(col)
                .filter_map(|(i, row)| Some((i, row.get(col)?.abs())))
                .max_by(|x, y| x.1.total_cmp(&y.1))
                .filter(|(_, size)| *size > 1e-14)
                .ok_or_else(|| invalid("singular interpolation system"))?
                .0;
            a.swap(col, pivot);
            q.swap(col, pivot);
            let row = a
                .get(col)
                .ok_or_else(|| invalid("missing pivot row"))?
                .clone();
            let rhs = *q.get(col).ok_or_else(|| invalid("missing pivot sample"))?;
            let divisor = *row
                .get(col)
                .ok_or_else(|| invalid("missing pivot coefficient"))?;
            for (target, sample) in a.iter_mut().zip(&mut q).skip(col + 1) {
                let factor = *target
                    .get(col)
                    .ok_or_else(|| invalid("missing interpolation coefficient"))?
                    / divisor;
                if factor != 0.0 {
                    for (x, p) in target.iter_mut().zip(&row).skip(col) {
                        *x -= factor * p;
                    }
                    for (x, p) in sample.iter_mut().zip(rhs) {
                        *x -= factor * p;
                    }
                }
            }
        }
        let mut controls = vec![[0.0; N]; n];
        for (r, (row, mut value)) in a.iter().zip(q).enumerate().rev() {
            for (coefficient, point) in row.iter().zip(&controls).skip(r + 1) {
                for (x, p) in value.iter_mut().zip(point) {
                    *x -= coefficient * p;
                }
            }
            let divisor = *row
                .get(r)
                .ok_or_else(|| invalid("missing interpolation diagonal"))?;
            *controls
                .get_mut(r)
                .ok_or_else(|| invalid("missing interpolated control"))? =
                value.map(|x| x / divisor);
        }
        if controls.iter().flatten().any(|x| !x.is_finite()) {
            return Err(invalid("interpolation did not give finite control points"));
        }
        Ok(controls)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cubic_tensor_reproduces_an_affine_patch_and_keeps_axis_order() {
        let parameter = [0.0, 0.25, 0.75, 1.0];
        let p = |u: f64, v: f64| Vec3::new(u, 2.0 * v, 3.0 * u - v);
        let samples: Vec<_> = parameter
            .into_iter()
            .flat_map(|u| parameter.map(|v| p(u, v)))
            .collect();
        let fit = cubic_tensor(&samples, &parameter).unwrap();
        assert_eq!(fit.knots, [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
        for (i, control) in fit.controls.into_iter().enumerate() {
            assert!(
                (Vec3::from(control) - p((i % 4) as f64 / 3.0, (i / 4) as f64 / 3.0)).length()
                    < 1e-12
            );
        }
        assert!(curve(&[0.0, 0.0, 1.0], &[[0.0]; 3], 2).is_err());
    }
}
