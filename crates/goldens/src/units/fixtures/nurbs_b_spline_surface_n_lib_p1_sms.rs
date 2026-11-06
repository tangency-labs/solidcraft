//! Pure SMS fixture decoder shared by the runner and non-C++ regression tests.

use crate::HarnessError;
use solidcore::nurbs::math::NOW;

pub(super) struct ParsedSrf {
    pub net: Vec<Vec<[f64; 4]>>,
    pub p: usize,
    pub q: usize,
    pub u: Vec<f64>,
    pub v: Vec<f64>,
}

fn error(path: &str, message: impl std::fmt::Display) -> HarnessError {
    HarnessError::new(format!("{path}: {message}"))
}

fn count(value: f64, path: &str) -> Result<usize, HarnessError> {
    if !value.is_finite()
        || value < 0.0
        || value.fract() != 0.0
        || value >= 2.0_f64.powi(usize::BITS as i32)
    {
        return Err(error(path, format!("bad count {value}")));
    }
    Ok(value as usize)
}

pub(super) fn decode(text: &str, path: &str) -> Result<ParsedSrf, HarnessError> {
    // Eager lexical validation deliberately includes even the ignored numeric tail.
    let values = text
        .split_whitespace()
        .map(|token| token.parse::<f64>().map_err(|e| error(path, e)))
        .collect::<Result<Vec<_>, _>>()?;
    let mut values = values.into_iter();
    let mut num = || values.next().ok_or_else(|| error(path, "truncated"));
    let n = count(num()?, path)?;
    let m = count(num()?, path)?;
    let p = count(num()?, path)?;
    let q = count(num()?, path)?;
    let rational = num()? != 0.0;
    let overflow = || error(path, "count overflow");
    let rows = n.checked_add(1).ok_or_else(overflow)?;
    let cols = m.checked_add(1).ok_or_else(overflow)?;
    let u_count = n
        .checked_add(p)
        .and_then(|v| v.checked_add(2))
        .ok_or_else(overflow)?;
    let v_count = m
        .checked_add(q)
        .and_then(|v| v.checked_add(2))
        .ok_or_else(overflow)?;
    let payload = rows
        .checked_mul(cols)
        .and_then(|v| v.checked_mul(if rational { 4 } else { 3 }))
        .and_then(|v| v.checked_add(u_count))
        .and_then(|v| v.checked_add(v_count))
        .ok_or_else(overflow)?;
    if payload > values.len() {
        return Err(error(path, "truncated"));
    }
    let mut num = || values.next().ok_or_else(|| error(path, "truncated"));
    // Never reserve from declared counts, even after the numeric payload preflight.
    let mut net = Vec::new();
    for _ in 0..rows {
        let mut row = Vec::new();
        for _ in 0..cols {
            let (x, y, z) = (num()?, num()?, num()?);
            let w = if rational { num()? } else { NOW };
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
