//! Pure SMS fixture decoder for the B-spline surface NLib p2 runner.
//!
//! Retains eager lexical parsing and stored homogeneous coordinates. Header sizes
//! are checked against actual numeric payload before any decoded output grows;
//! geometric validity remains the downstream surface constructor's responsibility.

use crate::HarnessError;
use solidcore::nurbs::math::NOW;

#[derive(Debug)]
pub(super) struct Decoded {
    pub net: Vec<Vec<[f64; 4]>>,
    pub p: usize,
    pub q: usize,
    pub u: Vec<f64>,
    pub v: Vec<f64>,
}

pub(super) fn decode(text: &str, path: &str) -> Result<Decoded, HarnessError> {
    // Parse even unused tails before inspecting headers, as the original reader did.
    let values = text
        .split_whitespace()
        .map(|t| {
            t.parse::<f64>()
                .map_err(|e| HarnessError::new(format!("{path}: {e}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut it = values.into_iter();
    let truncated = || HarnessError::new(format!("{path}: truncated"));
    let overflow = || HarnessError::new(format!("{path}: count overflow"));
    let count = |v: f64| {
        // usize::MAX as f64 rounds up on 64-bit: the boundary must be exclusive.
        if !v.is_finite() || v.fract() != 0.0 || v < 0.0 || v >= 2.0_f64.powi(usize::BITS as i32) {
            Err(HarnessError::new(format!("{path}: bad count {v}")))
        } else {
            Ok(v as usize)
        }
    };
    let n = count(it.next().ok_or_else(truncated)?)?;
    let m = count(it.next().ok_or_else(truncated)?)?;
    let p = count(it.next().ok_or_else(truncated)?)?;
    let q = count(it.next().ok_or_else(truncated)?)?;
    let rat = it.next().ok_or_else(truncated)? != 0.0;
    let rows = n.checked_add(1).ok_or_else(overflow)?;
    let cols = m.checked_add(1).ok_or_else(overflow)?;
    let points = rows.checked_mul(cols).ok_or_else(overflow)?;
    let u_len = n
        .checked_add(p)
        .and_then(|v| v.checked_add(2))
        .ok_or_else(overflow)?;
    let v_len = m
        .checked_add(q)
        .and_then(|v| v.checked_add(2))
        .ok_or_else(overflow)?;
    let coordinates = points
        .checked_mul(3 + usize::from(rat))
        .ok_or_else(overflow)?;
    let payload = coordinates
        .checked_add(u_len)
        .and_then(|v| v.checked_add(v_len))
        .ok_or_else(overflow)?;
    if payload > it.len() {
        return Err(truncated());
    }
    let mut num = || it.next().ok_or_else(truncated);
    let mut net = Vec::new();
    for _ in 0..rows {
        let mut row = Vec::new();
        for _ in 0..cols {
            row.push([num()?, num()?, num()?, if rat { num()? } else { NOW }]);
        }
        net.push(row);
    }
    let mut u = Vec::new();
    for _ in 0..u_len {
        u.push(num()?);
    }
    let mut v = Vec::new();
    for _ in 0..v_len {
        v.push(num()?);
    }
    Ok(Decoded { net, p, q, u, v })
}
