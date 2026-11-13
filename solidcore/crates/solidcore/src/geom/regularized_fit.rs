//! Bounded damped least-squares fitting in the smaller of row and column space.
//! Both systems use the same positive-definite Cholesky factorization.

use crate::{Result, SolidError};

fn invalid(reason: &'static str) -> SolidError {
    SolidError::InvalidInput {
        operation: "regularized fit",
        reason,
    }
}

/// Minimize `|A x - b|² + damping |x|²` for a dense, finite row-major system.
///
/// Takes up to 2,304 rows and 768 columns. A positive diagonal damping permits
/// overlapping constraints and returns the minimum-change regularized fit.
pub fn solve(
    coefficients: &[Vec<f64>],
    rhs: &[f64],
    columns: usize,
    damping: f64,
) -> Result<Vec<f64>> {
    if !(1..=768).contains(&columns)
        || !(1..=2304).contains(&rhs.len())
        || rhs.len() != coefficients.len()
        || !damping.is_finite()
        || damping <= 0.0
        || rhs.iter().any(|v| !v.is_finite())
        || coefficients
            .iter()
            .any(|row| row.len() != columns || row.iter().any(|v| !v.is_finite()))
    {
        return Err(invalid("invalid bounded constraint system"));
    }
    let column_space = columns < rhs.len();
    let transposed;
    let gram = if column_space {
        transposed = (0..columns)
            .map(|i| {
                coefficients
                    .iter()
                    .map(|row| {
                        row.get(i)
                            .copied()
                            .ok_or_else(|| invalid("missing constraint column"))
                    })
                    .collect()
            })
            .collect::<Result<Vec<Vec<f64>>>>()?;
        &transposed
    } else {
        coefficients
    };
    let mut lower: Vec<Vec<f64>> = Vec::new();
    for (i, row) in gram.iter().enumerate() {
        let mut entries = Vec::new();
        for (j, other) in gram.iter().enumerate().take(i + 1) {
            let mut value = dot(row, other);
            if i == j {
                value += damping;
                value -= dot(&entries, &entries);
                if !value.is_finite() || value <= 1e-18 {
                    return Err(invalid("constraints are numerically ill-conditioned"));
                }
                entries.push(value.sqrt());
            } else {
                let previous = lower.get(j).ok_or_else(|| invalid("missing factor row"))?;
                let (&diagonal, prefix) = previous
                    .split_last()
                    .ok_or_else(|| invalid("missing factor diagonal"))?;
                value -= dot(&entries, prefix);
                entries.push(value / diagonal);
            }
        }
        lower.push(entries);
    }
    let mut solution = if column_space {
        gram.iter().map(|column| dot(column, rhs)).collect()
    } else {
        rhs.to_vec()
    };
    for (i, row) in lower.iter().enumerate() {
        let (&diagonal, prefix) = row
            .split_last()
            .ok_or_else(|| invalid("missing factor diagonal"))?;
        let mut value = *solution
            .get(i)
            .ok_or_else(|| invalid("missing constraint right-hand side"))?;
        for (&a, &b) in prefix.iter().zip(&solution) {
            value -= a * b;
        }
        *solution
            .get_mut(i)
            .ok_or_else(|| invalid("missing fit value"))? = value / diagonal;
    }
    for (i, row) in lower.iter().enumerate().rev() {
        let diagonal = *row
            .last()
            .ok_or_else(|| invalid("missing factor diagonal"))?;
        let mut value = *solution
            .get(i)
            .ok_or_else(|| invalid("missing fit value"))?;
        for (other, &x) in lower.iter().zip(&solution).skip(i + 1) {
            value -= other
                .get(i)
                .ok_or_else(|| invalid("missing factor column"))?
                * x;
        }
        *solution
            .get_mut(i)
            .ok_or_else(|| invalid("missing fit value"))? = value / diagonal;
    }
    let result = if column_space {
        solution
    } else {
        let mut result = vec![0.0; columns];
        for (row, value) in coefficients.iter().zip(solution) {
            for (out, a) in result.iter_mut().zip(row) {
                *out += a * value;
            }
        }
        result
    };
    if result.iter().any(|v| !v.is_finite()) {
        return Err(invalid("fit produced non-finite changes"));
    }
    Ok(result)
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn underdetermined_fit_shares_change_between_identical_controls() {
        let solution = solve(&[vec![1.0, 1.0]], &[6.0], 2, 1.0).unwrap();
        assert!((solution[0] - 2.0).abs() < 1e-14 && solution[0] == solution[1]);
    }

    #[test]
    fn overdetermined_fit_minimizes_the_regularized_error() {
        let solution = solve(&[vec![1.0], vec![1.0]], &[2.0, 4.0], 1, 1.0).unwrap();
        assert!((solution[0] - 2.0).abs() < 1e-14);
        assert!(solve(&[vec![1.0]], &[f64::NAN], 1, 1.0).is_err());
        assert!(solve(&[vec![1.0]], &[1.0], 2, 1.0).is_err());
    }
}
