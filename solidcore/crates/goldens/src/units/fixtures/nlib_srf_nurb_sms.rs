//! Runner-local SMS decoding, shared with non-C++ regression tests.
//!
//! Deviations: reject unrepresentable counts and arithmetic overflow; preflight
//! the consumed numeric payload without allocation before growing storage.
//! Parsing remains lazy with respect to ignored trailing tokens.

use solidcore::nurbs::math::NOW;

use crate::HarnessError;

/// Decoded surface storage, with the v index fastest.
pub(super) struct ParsedSrf {
    pub(super) net: Vec<Vec<[f64; 4]>>,
    pub(super) p: usize,
    pub(super) q: usize,
    pub(super) u: Vec<f64>,
    pub(super) v: Vec<f64>,
}

/// Decodes the header, homogeneous or polynomial points, and u/v knots.
pub(super) fn parse_sms(text: &str, path: &str) -> Result<ParsedSrf, HarnessError> {
    let error = |message: &str| HarnessError::new(format!("{path}: {message}"));
    let parse = |token: &str| token.parse::<f64>().map_err(|e| error(&e.to_string()));
    let mut it = text.split_whitespace();
    let mut num = || parse(it.next().ok_or_else(|| error("truncated"))?);
    let int = |value: f64| -> Result<usize, HarnessError> {
        // usize::MAX as f64 rounds up on 64-bit hosts: use an exclusive bound.
        let upper = 2.0_f64.powi(usize::BITS as i32);
        if !value.is_finite() || value.fract() != 0.0 || value < 0.0 || value >= upper {
            return Err(error(&format!("bad count {value}")));
        }
        Ok(value as usize)
    };
    let n = int(num()?)?;
    let m = int(num()?)?;
    let p = int(num()?)?;
    let q = int(num()?)?;
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

    // Validate even a short prefix before reporting truncation, preserving the
    // original first numeric error. Never parse ignored trailing text or allocate
    // storage from declared sizes, including when the payload is incomplete.
    let mut available = 0;
    for token in it.clone().take(required) {
        parse(token)?;
        available += 1;
    }
    if available < required {
        return Err(error("truncated"));
    }
    let mut num = || parse(it.next().ok_or_else(|| error("truncated"))?);
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
    Ok(ParsedSrf { net, p, q, u, v })
}
