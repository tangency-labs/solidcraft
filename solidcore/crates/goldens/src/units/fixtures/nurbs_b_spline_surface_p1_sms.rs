//! Pure, lazy numeric decoder for the p1 surface runner's SMS fixtures.
//!
//! Shape arithmetic and token availability are checked before output allocation.
//! Only consumed numbers are parsed; arbitrary trailing tokens are ignored.

use crate::HarnessError;
use solidcore::nurbs::math::NOW;

/// Stored (not dehomogenized) surface data, with the v index varying fastest.
#[derive(Debug)]
pub struct ParsedSms {
    /// Stored coordinates and weights, with rows in u.
    pub net: Vec<Vec<[f64; 4]>>,
    /// Degree in u, without geometry-policy restrictions.
    pub p: usize,
    /// Degree in v, without geometry-policy restrictions.
    pub q: usize,
    /// Knots in u.
    pub u: Vec<f64>,
    /// Knots in v.
    pub v: Vec<f64>,
}

/// Decode the consumed SMS header and payload, qualifying errors with `path`.
pub fn parse_sms(text: &str, path: &str) -> Result<ParsedSms, HarnessError> {
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
        // usize::MAX rounds up on 64-bit targets, so this bound is exclusive.
        let exclusive = 2.0_f64.powi(usize::BITS as i32);
        if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value >= exclusive {
            Err(error(&format!("bad count {value}")))
        } else {
            Ok(value as usize)
        }
    };
    let n = count(number(&mut tokens)?)?;
    let m = count(number(&mut tokens)?)?;
    let p = count(number(&mut tokens)?)?;
    let q = count(number(&mut tokens)?)?;
    let rational = number(&mut tokens)? != 0.0;
    let overflow = || error("SMS size overflow");
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
    let points = rows.checked_mul(cols).ok_or_else(overflow)?;
    let coordinates = points
        .checked_mul(if rational { 4 } else { 3 })
        .ok_or_else(overflow)?;
    let required = coordinates
        .checked_add(u_len)
        .and_then(|v| v.checked_add(v_len))
        .ok_or_else(overflow)?;
    if tokens.clone().count() < required {
        // Preserve the first consumed malformed number ahead of a later EOF,
        // without allocating any output for a short body.
        for token in tokens {
            token.parse::<f64>().map_err(|e| error(&e.to_string()))?;
        }
        return Err(error("truncated"));
    }
    let mut net = Vec::new();
    for _ in 0..rows {
        let mut row = Vec::new();
        for _ in 0..cols {
            let x = number(&mut tokens)?;
            let y = number(&mut tokens)?;
            let z = number(&mut tokens)?;
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
    Ok(ParsedSms { net, p, q, u, v })
}
