//! Lazy SMS decoding shared by the surface-advanced runner and non-C++ tests.
//!
//! Deviations: counts and payload sizes are checked before incremental allocation.

use solidcore::nurbs::math::NOW;

use crate::HarnessError;

pub(super) struct SmsSurface {
    pub net: Vec<Vec<[f64; 4]>>,
    pub p: usize,
    pub q: usize,
    pub u: Vec<f64>,
    pub v: Vec<f64>,
}

pub(super) fn parse_sms(text: &str, path: &str) -> Result<SmsSurface, HarnessError> {
    let error = |message: &str| HarnessError::new(format!("{path}: {message}"));
    let mut tokens = text.split_whitespace();
    let number = |tokens: &mut std::str::SplitWhitespace<'_>| {
        tokens
            .next()
            .ok_or_else(|| error("truncated"))?
            .parse::<f64>()
            .map_err(|e| error(&e.to_string()))
    };
    let count = |value: f64| {
        // usize::MAX as f64 rounds up on 64-bit hosts: the limit is exclusive.
        if !value.is_finite()
            || value.fract() != 0.0
            || value < 0.0
            || value >= 2.0_f64.powi(usize::BITS as i32)
        {
            return Err(error(&format!("bad count {value}")));
        }
        Ok(value as usize)
    };
    let n = count(number(&mut tokens)?)?;
    let m = count(number(&mut tokens)?)?;
    let p = count(number(&mut tokens)?)?;
    let q = count(number(&mut tokens)?)?;
    let rat = number(&mut tokens)? != 0.0;
    let overflow = || error("SMS size overflow");
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
        .and_then(|v| v.checked_mul(if rat { 4 } else { 3 }))
        .and_then(|v| v.checked_add(u_count))
        .and_then(|v| v.checked_add(v_count))
        .ok_or_else(overflow)?;

    if tokens.clone().count() < payload {
        // Preserve the lazy reader's first numeric error before truncation, but
        // never grow a net from a short file declaring huge dimensions.
        while tokens.clone().next().is_some() {
            number(&mut tokens)?;
        }
        return Err(error("truncated"));
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
            let w = if rat { number(&mut tokens)? } else { NOW };
            row.push([x, y, z, w]);
        }
        net.push(row);
    }
    let mut u = Vec::new();
    for _ in 0..u_count {
        u.push(number(&mut tokens)?);
    }
    let mut v = Vec::new();
    for _ in 0..v_count {
        v.push(number(&mut tokens)?);
    }
    // Trailing tokens have always been ignored, even when they are not numbers.
    Ok(SmsSurface { net, p, q, u, v })
}
