//! Pure runner-local SMS decoding, shared with non-C++ regression tests.
//!
//! Deviations: checked counts/arithmetic and actual numeric payload preflight before
//! incremental net/knot construction. Preserve eager float parsing (including tails)
//! before header validation; numeric tails remain ignored. No geometry validity caps.

use solidcore::nurbs::math::NOW;

use crate::HarnessError;

/// Decoded storage, independent of the C++-gated runner; v index is fastest.
pub(super) struct ParsedSurface {
    pub(super) net: Vec<Vec<[f64; 4]>>,
    pub(super) p: usize,
    pub(super) q: usize,
    pub(super) u: Vec<f64>,
    pub(super) v: Vec<f64>,
}

/// Parses all floats before interpreting `n m p q rat`, points, and u/v knots.
pub(super) fn parse_sms(text: &str, path: &str) -> Result<ParsedSurface, HarnessError> {
    let error = |message: &str| HarnessError::new(format!("{path}: {message}"));
    let values = text
        .split_whitespace()
        .map(|token| token.parse::<f64>().map_err(|e| error(&e.to_string())))
        .collect::<Result<Vec<_>, _>>()?;
    let available = values.len().saturating_sub(5);
    let mut it = values.into_iter();
    let mut num = || it.next().ok_or_else(|| error("truncated"));
    let count = |value: f64| {
        // usize::MAX rounds upward on 64-bit hosts. The upper bound is exclusive.
        let upper = 2.0_f64.powi(usize::BITS as i32);
        if !value.is_finite() || value.fract() != 0.0 || value < 0.0 || value >= upper {
            return Err(error(&format!("bad count {value}")));
        }
        Ok(value as usize)
    };
    let n = count(num()?)?;
    let m = count(num()?)?;
    let p = count(num()?)?;
    let q = count(num()?)?;
    let rat = num()? != 0.0;
    let rows = n
        .checked_add(1)
        .ok_or_else(|| error("row count overflow"))?;
    let cols = m
        .checked_add(1)
        .ok_or_else(|| error("column count overflow"))?;
    let u_count = n
        .checked_add(p)
        .and_then(|v| v.checked_add(2))
        .ok_or_else(|| error("u knot count overflow"))?;
    let v_count = m
        .checked_add(q)
        .and_then(|v| v.checked_add(2))
        .ok_or_else(|| error("v knot count overflow"))?;
    let required = rows
        .checked_mul(cols)
        .and_then(|v| v.checked_mul(if rat { 4 } else { 3 }))
        .and_then(|v| v.checked_add(u_count))
        .and_then(|v| v.checked_add(v_count))
        .ok_or_else(|| error("required token count overflow"))?;
    if required > available {
        return Err(error("truncated"));
    }
    let mut net = Vec::new();
    for _ in 0..rows {
        let mut row = Vec::new();
        for _ in 0..cols {
            let (x, y, z) = (num()?, num()?, num()?);
            let w = if rat { num()? } else { NOW };
            row.push([x, y, z, w]);
        }
        net.push(row);
    }
    let mut u = Vec::new();
    for _ in 0..u_count {
        u.push(num()?);
    }
    let mut v = Vec::new();
    for _ in 0..v_count {
        v.push(num()?);
    }
    Ok(ParsedSurface { net, p, q, u, v })
}
