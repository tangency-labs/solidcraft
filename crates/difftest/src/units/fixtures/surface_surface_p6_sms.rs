//! Pure SMS reader shared by the surface P6 runner and non-C++ tests.

use solidcore::nurbs::math::NOW;

use crate::HarnessError;

pub(crate) struct ParsedSurface {
    pub(crate) net: Vec<Vec<[f64; 4]>>,
    pub(crate) p: usize,
    pub(crate) q: usize,
    pub(crate) u: Vec<f64>,
    pub(crate) v: Vec<f64>,
}

fn count(path: &str, value: f64) -> Result<usize, HarnessError> {
    // usize::MAX rounds up to 2^BITS on 64-bit targets: compare exclusively.
    let upper = 2.0_f64.powi(usize::BITS as i32);
    if !value.is_finite() || value.fract() != 0.0 || value < 0.0 || value >= upper {
        return Err(HarnessError::new(format!("{path}: bad count {value}")));
    }
    Ok(value as usize)
}

pub(crate) fn knot_count(
    last: usize,
    degree: usize,
    axis: &str,
    path: &str,
) -> Result<usize, HarnessError> {
    last.checked_add(degree)
        .and_then(|sum| sum.checked_add(2))
        .ok_or_else(|| HarnessError::new(format!("{path}: {axis} knot count overflow")))
}

pub(crate) fn parse_sms(text: &str, path: &str) -> Result<ParsedSurface, HarnessError> {
    // Deliberately eager: even a malformed ignored tail precedes header errors.
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
    // Grow only after actual reads, never from declared sizes or iterator hints.
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
    // Preserve net-before-U-before-V error precedence, including arithmetic.
    let mut u = Vec::new();
    for _ in 0..knot_count(n, p, "u", path)? {
        u.push(num()?);
    }
    let mut v = Vec::new();
    for _ in 0..knot_count(m, q, "v", path)? {
        v.push(num()?);
    }
    Ok(ParsedSurface { net, p, q, u, v })
}
