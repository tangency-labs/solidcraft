//! Runner-local SMS decoding, also included by non-C++ regression tests.
//!
//! Deviations: validate counts, check arithmetic and available numeric tokens
//! before constructing the net, and grow storage only as points/knots are read.

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

/// Decodes `n m p q`, a rational flag, points, and u/v knots.
pub(super) fn parse_sms(text: &str, path: &str) -> Result<ParsedSrf, HarnessError> {
    let error = |message: &str| HarnessError::new(format!("{path}: {message}"));
    // Preserve eager float parsing, including trailing tokens. This allocation
    // depends only on actual input, never on header-declared dimensions.
    let all = text
        .split_whitespace()
        .map(|t| t.parse::<f64>().map_err(|e| error(&e.to_string())))
        .collect::<Result<Vec<_>, _>>()?;
    let available = all.len().saturating_sub(5);
    let mut it = all.into_iter();
    let mut num = || it.next().ok_or_else(|| error("truncated"));
    let int = |value: f64, name: &str| -> Result<usize, HarnessError> {
        // usize::MAX rounds up on 64-bit hosts; use the exclusive power-of-two bound.
        let upper = 2.0_f64.powi(usize::BITS as i32);
        if !value.is_finite() || value.fract() != 0.0 || value < 0.0 || value >= upper {
            return Err(error(&format!("bad {name} count {value}")));
        }
        Ok(value as usize)
    };
    let n = int(num()?, "n")?;
    let m = int(num()?, "m")?;
    let p = int(num()?, "p")?;
    let q = int(num()?, "q")?;
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
        return Err(error(&format!(
            "truncated: need {required} data tokens, have {available}"
        )));
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
    Ok(ParsedSrf { net, p, q, u, v })
}
