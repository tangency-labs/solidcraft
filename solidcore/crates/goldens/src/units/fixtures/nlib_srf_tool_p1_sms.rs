//! Allocation-checked SMS decoding shared by the runner and non-C++ regression tests.

use crate::HarnessError;
use solidcore::nurbs::math::NOW;

pub(super) struct SmsSurface {
    pub(super) net: Vec<Vec<[f64; 4]>>,
    pub(super) p: usize,
    pub(super) q: usize,
    pub(super) u: Vec<f64>,
    pub(super) v: Vec<f64>,
}

pub(super) fn decode(text: &str, path: &str) -> Result<SmsSurface, HarnessError> {
    let error = |message: &str| HarnessError::new(format!("{path}: {message}"));
    let number = |tokens: &mut std::str::SplitWhitespace<'_>| {
        tokens
            .next()
            .ok_or_else(|| error("truncated"))?
            .parse::<f64>()
            .map_err(|e| error(&e.to_string()))
    };
    let count = |value: f64| {
        // usize::MAX rounds up to 2^BITS as f64 on 64-bit targets. Use the
        // exclusive power-of-two bound, not that rounded maximum, before casting.
        let exclusive = 2.0_f64.powi(usize::BITS as i32);
        if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value >= exclusive {
            return Err(error(&format!("bad count {value}")));
        }
        Ok(value as usize)
    };
    let mut tokens = text.split_whitespace();
    let n = count(number(&mut tokens)?)?;
    let m = count(number(&mut tokens)?)?;
    let p = count(number(&mut tokens)?)?;
    let q = count(number(&mut tokens)?)?;
    let rational = number(&mut tokens)? != 0.0;
    let overflow = || error("count overflow");
    let rows = n.checked_add(1).ok_or_else(overflow)?;
    let cols = m.checked_add(1).ok_or_else(overflow)?;
    let u_len = n
        .checked_add(p)
        .and_then(|v| v.checked_add(2))
        .ok_or_else(overflow)?;
    let v_len = m
        .checked_add(q)
        .and_then(|v| v.checked_add(2))
        .ok_or_else(overflow)?;
    let payload = rows
        .checked_mul(cols)
        .and_then(|v| v.checked_mul(if rational { 4 } else { 3 }))
        .and_then(|v| v.checked_add(u_len))
        .and_then(|v| v.checked_add(v_len))
        .ok_or_else(overflow)?;

    // No header-sized allocation until the entire consumed payload is present.
    // Parse the available prefix even on truncation, preserving the old lazy
    // parser's first numeric error. Never inspect ignored trailing tokens.
    let mut preflight = tokens.clone();
    for _ in 0..payload {
        number(&mut preflight)?;
    }

    let mut net = Vec::new();
    for _ in 0..rows {
        let mut row = Vec::new();
        for _ in 0..cols {
            let (x, y, z) = (
                number(&mut tokens)?,
                number(&mut tokens)?,
                number(&mut tokens)?,
            );
            let w = if rational { number(&mut tokens)? } else { NOW };
            row.push([x, y, z, w]);
        }
        net.push(row);
    }
    let mut u = Vec::new();
    for _ in 0..u_len {
        u.push(number(&mut tokens)?);
    }
    let mut v = Vec::new();
    for _ in 0..v_len {
        v.push(number(&mut tokens)?);
    }
    Ok(SmsSurface { net, p, q, u, v })
}
