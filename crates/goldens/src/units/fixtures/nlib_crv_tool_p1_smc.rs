//! Allocation-checked SMC decoding shared by the runner and non-C++ regression tests.

use crate::HarnessError;
use solidcore::nurbs::math::{NOW, NOZ};

pub(super) struct SmcCurve {
    pub(super) cpts: Vec<[f64; 4]>,
    pub(super) p: usize,
    pub(super) u: Vec<f64>,
}

pub(super) fn decode(text: &str, path: &str) -> Result<SmcCurve, HarnessError> {
    let error = |message: &str| HarnessError::new(format!("{path}: {message}"));
    // Preserve eager lexical validation of the entire file, even ignored tails,
    // before inspecting headers in their original n, p, rational, dim order.
    let values = text
        .split_whitespace()
        .map(|token| token.parse::<f64>().map_err(|e| error(&e.to_string())))
        .collect::<Result<Vec<_>, _>>()?;
    let mut numbers = values.into_iter();
    let mut number = || numbers.next().ok_or_else(|| error("truncated"));
    let count = |value: f64| {
        // usize::MAX rounds up on 64-bit targets; reject the exclusive 2^BITS
        // boundary before casting rather than accepting a saturating conversion.
        let exclusive = 2.0_f64.powi(usize::BITS as i32);
        if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value >= exclusive {
            return Err(error(&format!("bad count {value}")));
        }
        Ok(value as usize)
    };
    let n = count(number()?)?;
    let p = count(number()?)?;
    let rational = number()? != 0.0;
    let dim = count(number()?)?;
    let overflow = || error("count overflow");
    let points = n.checked_add(1).ok_or_else(overflow)?;
    let knots = n
        .checked_add(p)
        .and_then(|v| v.checked_add(2))
        .ok_or_else(overflow)?;
    let stride = 2_usize
        .checked_add(usize::from(dim == 3))
        .and_then(|v| v.checked_add(usize::from(rational)))
        .ok_or_else(overflow)?;
    let payload = points
        .checked_mul(stride)
        .and_then(|v| v.checked_add(knots))
        .ok_or_else(overflow)?;
    // Preflight numeric availability before any header-driven output growth.
    if numbers.len() < payload {
        return Err(error("truncated"));
    }
    let mut number = || numbers.next().ok_or_else(|| error("truncated"));
    let mut cpts = Vec::new();
    for _ in 0..points {
        let (x, y) = (number()?, number()?);
        let z = if dim == 3 { number()? } else { NOZ };
        let w = if rational { number()? } else { NOW };
        cpts.push([x, y, z, w]);
    }
    let mut u = Vec::new();
    for _ in 0..knots {
        u.push(number()?);
    }
    Ok(SmcCurve { cpts, p, u })
}
