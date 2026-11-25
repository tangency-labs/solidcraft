//! Pure decoder for offset-surface SMS fixtures.
//!
//! This preserves the runner's eager lexical pass, raw weighted points and
//! net → U → V error order. Counts have an exclusive usize boundary and knot
//! additions are checked at their original consumption positions. Storage grows
//! only after reads; there are no header-derived reservations or geometry caps.

use crate::HarnessError;
use solidcore::nurbs::math::NOW;

/// Raw SMS data, without downstream geometry validation or adapter defaults.
pub(crate) struct ParsedSrf {
    pub(crate) net: Vec<Vec<[f64; 4]>>,
    pub(crate) p: usize,
    pub(crate) q: usize,
    pub(crate) u: Vec<f64>,
    pub(crate) v: Vec<f64>,
}

fn count(path: &str, value: f64) -> Result<usize, HarnessError> {
    let exclusive_upper = 2.0_f64.powi(usize::BITS as i32);
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value >= exclusive_upper {
        return Err(HarnessError::new(format!("{path}: bad count {value}")));
    }
    Ok(value as usize)
}

/// Checked knot length, also callable by bounded arithmetic regression tests.
pub(crate) fn knot_count(
    index: usize,
    degree: usize,
    path: &str,
    axis: &str,
) -> Result<usize, HarnessError> {
    index
        .checked_add(degree)
        .and_then(|sum| sum.checked_add(2))
        .ok_or_else(|| HarnessError::new(format!("{path}: {axis} knot count overflow")))
}

/// Decode all numeric tokens eagerly; extra numeric values remain ignored.
pub(crate) fn parse_sms(text: &str, path: &str) -> Result<ParsedSrf, HarnessError> {
    let values = text
        .split_whitespace()
        .map(|token| {
            token
                .parse::<f64>()
                .map_err(|error| HarnessError::new(format!("{path}: {error}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut values = values.into_iter();
    let mut num = || {
        values
            .next()
            .ok_or_else(|| HarnessError::new(format!("{path}: truncated")))
    };
    let n = count(path, num()?)?;
    let m = count(path, num()?)?;
    let p = count(path, num()?)?;
    let q = count(path, num()?)?;
    let rational = num()? != 0.0;
    let mut net = Vec::new();
    for _ in 0..=n {
        let mut row = Vec::new();
        for _ in 0..=m {
            let (x, y, z) = (num()?, num()?, num()?);
            let w = if rational { num()? } else { NOW };
            row.push([x, y, z, w]);
        }
        net.push(row);
    }
    let mut u = Vec::new();
    for _ in 0..knot_count(n, p, path, "u")? {
        u.push(num()?);
    }
    let mut v = Vec::new();
    for _ in 0..knot_count(m, q, path, "v")? {
        v.push(num()?);
    }
    Ok(ParsedSrf { net, p, q, u, v })
}
