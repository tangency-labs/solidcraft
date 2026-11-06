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
    // Lexically validate the whole file, including ignored numeric tails, before
    // interpreting headers in their original n, m, p, q, rational order.
    let values = text
        .split_whitespace()
        .map(|token| token.parse::<f64>().map_err(|e| error(&e.to_string())))
        .collect::<Result<Vec<_>, _>>()?;
    let mut numbers = values.into_iter();
    let mut number = || numbers.next().ok_or_else(|| error("truncated"));
    let count = |value: f64| {
        // usize::MAX rounds up on 64-bit targets: test the exclusive bound
        // before casting, rather than allowing a saturating conversion.
        let exclusive = 2.0_f64.powi(usize::BITS as i32);
        if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value >= exclusive {
            return Err(error(&format!("bad count {value}")));
        }
        Ok(value as usize)
    };
    let n = count(number()?)?;
    let m = count(number()?)?;
    let p = count(number()?)?;
    let q = count(number()?)?;
    let rational = number()? != 0.0;
    let overflow = || error("count overflow");
    let rows = n.checked_add(1).ok_or_else(overflow)?;
    let cols = m.checked_add(1).ok_or_else(overflow)?;
    let u_knots = n
        .checked_add(p)
        .and_then(|v| v.checked_add(2))
        .ok_or_else(overflow)?;
    let v_knots = m
        .checked_add(q)
        .and_then(|v| v.checked_add(2))
        .ok_or_else(overflow)?;
    let stride = 3_usize
        .checked_add(usize::from(rational))
        .ok_or_else(overflow)?;
    let payload = rows
        .checked_mul(cols)
        .and_then(|v| v.checked_mul(stride))
        .and_then(|v| v.checked_add(u_knots))
        .and_then(|v| v.checked_add(v_knots))
        .ok_or_else(overflow)?;
    // Actual numeric availability, not the header, bounds all output growth.
    if numbers.len() < payload {
        return Err(error("truncated"));
    }
    let mut number = || numbers.next().ok_or_else(|| error("truncated"));
    let mut net = Vec::new();
    for _ in 0..rows {
        let mut row = Vec::new();
        for _ in 0..cols {
            let (x, y, z) = (number()?, number()?, number()?);
            let w = if rational { number()? } else { NOW };
            row.push([x, y, z, w]);
        }
        net.push(row);
    }
    let mut u = Vec::new();
    for _ in 0..u_knots {
        u.push(number()?);
    }
    let mut v = Vec::new();
    for _ in 0..v_knots {
        v.push(number()?);
    }
    Ok(SmsSurface { net, p, q, u, v })
}
