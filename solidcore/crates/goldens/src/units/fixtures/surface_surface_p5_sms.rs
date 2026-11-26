//! Private SMS fixture reader shared by the P5 runner and non-C++ tests.
//!
//! Deviations: reject unrepresentable counts and overflowing knot lengths rather
//! than saturating casts or overflowing arithmetic. Preserve eager float parsing
//! and the original net, U-knot, V-knot processing/error order.

use crate::HarnessError;
use solidcore::nurbs::math::NOW;

pub(crate) struct ParsedSurface {
    pub(crate) net: Vec<Vec<[f64; 4]>>,
    pub(crate) p: usize,
    pub(crate) q: usize,
    pub(crate) u: Vec<f64>,
    pub(crate) v: Vec<f64>,
}

pub(crate) fn read_sms(path: &str) -> Result<ParsedSurface, HarnessError> {
    let text =
        std::fs::read_to_string(path).map_err(|e| HarnessError::new(format!("{path}: {e}")))?;
    parse_sms(path, &text)
}

fn count(path: &str, value: f64) -> Result<usize, HarnessError> {
    // usize::MAX rounds up on 64-bit hosts. Use the exclusive power-of-two
    // bound instead, so a saturating cast can never accept that rounded value.
    let exclusive_bound = 2.0_f64.powi(usize::BITS as i32);
    if !value.is_finite() || value.fract() != 0.0 || value < 0.0 || value >= exclusive_bound {
        return Err(HarnessError::new(format!("{path}: bad count {value}")));
    }
    Ok(value as usize)
}

fn knot_count(path: &str, n: usize, degree: usize) -> Result<usize, HarnessError> {
    n.checked_add(degree)
        .and_then(|sum| sum.checked_add(2))
        .ok_or_else(|| HarnessError::new(format!("{path}: knot count overflow")))
}

pub(crate) fn parse_sms(path: &str, text: &str) -> Result<ParsedSurface, HarnessError> {
    // Even ignored tails must be lexically valid, before any header validation.
    let values = text
        .split_whitespace()
        .map(|token| {
            token
                .parse::<f64>()
                .map_err(|e| HarnessError::new(format!("{path}: {e}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut it = values.into_iter();
    let mut num = || {
        it.next()
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
    // Compute each length only at its original processing position. In
    // particular, V overflow must not mask missing U data (nor net truncation).
    let mut u = Vec::new();
    for _ in 0..knot_count(path, n, p)? {
        u.push(num()?);
    }
    let mut v = Vec::new();
    for _ in 0..knot_count(path, m, q)? {
        v.push(num()?);
    }
    Ok(ParsedSurface { net, p, q, u, v })
}
