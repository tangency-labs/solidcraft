//! Lazy SMS surface decoding shared by the runner and non-C++ regression tests.

use crate::HarnessError;
use solidcore::nurbs::math::NOW;

/// Raw SMS data; geometry validation remains the surface constructor's job.
#[derive(Debug)]
pub struct SmsSurface {
    /// Homogeneous control net, v index fastest.
    pub net: Vec<Vec<[f64; 4]>>,
    /// U degree.
    pub p: usize,
    /// V degree.
    pub q: usize,
    /// U knots.
    pub u: Vec<f64>,
    /// V knots.
    pub v: Vec<f64>,
}

fn number(it: &mut std::str::SplitWhitespace<'_>, path: &str) -> Result<f64, HarnessError> {
    it.next()
        .ok_or_else(|| HarnessError::new(format!("{path}: truncated")))?
        .parse::<f64>()
        .map_err(|e| HarnessError::new(format!("{path}: {e}")))
}

fn count(it: &mut std::str::SplitWhitespace<'_>, path: &str) -> Result<usize, HarnessError> {
    let value = number(it, path)?;
    // usize::MAX rounds up on 64-bit hosts: the upper bound must be exclusive.
    if !value.is_finite()
        || value.fract() != 0.0
        || value < 0.0
        || value >= 2.0_f64.powi(usize::BITS as i32)
    {
        return Err(HarnessError::new(format!("{path}: bad count {value}")));
    }
    Ok(value as usize)
}

/// Decode only the consumed prefix, ignoring even malformed trailing tokens.
pub fn parse_sms(text: &str, path: &str) -> Result<SmsSurface, HarnessError> {
    let mut it = text.split_whitespace();
    let n = count(&mut it, path)?;
    let m = count(&mut it, path)?;
    let p = count(&mut it, path)?;
    let q = count(&mut it, path)?;
    let rational = number(&mut it, path)? != 0.0;
    let overflow = || HarnessError::new(format!("{path}: SMS size overflow"));
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
    let required = rows
        .checked_mul(cols)
        .and_then(|v| v.checked_mul(if rational { 4 } else { 3 }))
        .and_then(|v| v.checked_add(u_count))
        .and_then(|v| v.checked_add(v_count))
        .ok_or_else(overflow)?;

    // Count without allocating or parsing. On short input, preserve the first
    // available bad float ahead of truncation, still before allocating output.
    if it.clone().count() < required {
        while it.clone().next().is_some() {
            number(&mut it, path)?;
        }
        return Err(HarnessError::new(format!("{path}: truncated")));
    }

    let mut net = Vec::new();
    for _ in 0..rows {
        let mut row = Vec::new();
        for _ in 0..cols {
            let x = number(&mut it, path)?;
            let y = number(&mut it, path)?;
            let z = number(&mut it, path)?;
            let w = if rational {
                number(&mut it, path)?
            } else {
                NOW
            };
            row.push([x, y, z, w]);
        }
        net.push(row);
    }
    let mut u = Vec::new();
    for _ in 0..u_count {
        u.push(number(&mut it, path)?);
    }
    let mut v = Vec::new();
    for _ in 0..v_count {
        v.push(number(&mut it, path)?);
    }
    Ok(SmsSurface { net, p, q, u, v })
}
