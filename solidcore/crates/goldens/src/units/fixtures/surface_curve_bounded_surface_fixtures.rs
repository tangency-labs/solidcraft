//! Runner-local SMS/SMC decoding, included by non-C++ regression tests.
//!
//! Deviations: validate counts, check arithmetic and available numeric tokens
//! before constructing geometry storage, and grow it only as values are decoded.

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

/// Decoded curve storage, with coordinates and weights stored verbatim.
pub(super) struct ParsedCrv {
    pub(super) dim: u64,
    pub(super) cpts: Vec<[f64; 4]>,
    pub(super) p: usize,
    pub(super) u: Vec<f64>,
}

fn error(path: &str, message: impl std::fmt::Display) -> HarnessError {
    HarnessError::new(format!("{path}: {message}"))
}

fn numbers(text: &str, path: &str) -> Result<Vec<f64>, HarnessError> {
    // Preserve eager numeric parsing, including otherwise-unused trailing tokens.
    // This allocation depends only on actual input, never declared dimensions.
    text.split_whitespace()
        .map(|t| t.parse::<f64>().map_err(|e| error(path, e)))
        .collect()
}

fn int(path: &str, value: f64, name: &str) -> Result<usize, HarnessError> {
    // usize::MAX rounds up to 2^BITS as f64 on 64-bit hosts. The exclusive
    // power-of-two bound rejects that unrepresentable value without saturation.
    let upper = 2.0_f64.powi(usize::BITS as i32);
    if !value.is_finite() || value.fract() != 0.0 || value < 0.0 || value >= upper {
        return Err(error(path, format!("bad {name} count {value}")));
    }
    Ok(value as usize)
}

fn ensure_available(path: &str, required: usize, available: usize) -> Result<(), HarnessError> {
    if required > available {
        return Err(error(
            path,
            format!("truncated: need {required} data tokens, have {available}"),
        ));
    }
    Ok(())
}

/// Decodes `n m p q`, a rational flag, points, and u/v knots.
pub(super) fn parse_sms(text: &str, path: &str) -> Result<ParsedSrf, HarnessError> {
    let all = numbers(text, path)?;
    let available = all.len().saturating_sub(5);
    let mut it = all.into_iter();
    let mut num = || it.next().ok_or_else(|| error(path, "truncated"));
    let n = int(path, num()?, "n")?;
    let m = int(path, num()?, "m")?;
    let p = int(path, num()?, "p")?;
    let q = int(path, num()?, "q")?;
    let rat = num()? != 0.0;
    let rows = n
        .checked_add(1)
        .ok_or_else(|| error(path, "row count overflow"))?;
    let cols = m
        .checked_add(1)
        .ok_or_else(|| error(path, "column count overflow"))?;
    let u_count = n
        .checked_add(p)
        .and_then(|v| v.checked_add(2))
        .ok_or_else(|| error(path, "u knot count overflow"))?;
    let v_count = m
        .checked_add(q)
        .and_then(|v| v.checked_add(2))
        .ok_or_else(|| error(path, "v knot count overflow"))?;
    let points = rows
        .checked_mul(cols)
        .ok_or_else(|| error(path, "point count overflow"))?;
    let required = points
        .checked_mul(if rat { 4 } else { 3 })
        .and_then(|v| v.checked_add(u_count))
        .and_then(|v| v.checked_add(v_count))
        .ok_or_else(|| error(path, "required token count overflow"))?;
    ensure_available(path, required, available)?;
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

/// Decodes `n p`, a rational flag, dimension, points, and knots (version <= 32).
pub(super) fn parse_smc(text: &str, path: &str) -> Result<ParsedCrv, HarnessError> {
    let all = numbers(text, path)?;
    let available = all.len().saturating_sub(4);
    let mut it = all.into_iter();
    let mut num = || it.next().ok_or_else(|| error(path, "truncated"));
    let n = int(path, num()?, "n")?;
    let p = int(path, num()?, "p")?;
    let rat = num()? != 0.0;
    let dim = int(path, num()?, "dimension")? as u64;
    let points = n
        .checked_add(1)
        .ok_or_else(|| error(path, "point count overflow"))?;
    let u_count = n
        .checked_add(p)
        .and_then(|v| v.checked_add(2))
        .ok_or_else(|| error(path, "u knot count overflow"))?;
    // Preserve the original dimension branch: exactly 2 omits z; all others read z.
    let width = (if dim == 2 { 2_usize } else { 3_usize })
        .checked_add(usize::from(rat))
        .ok_or_else(|| error(path, "coordinate width overflow"))?;
    let required = points
        .checked_mul(width)
        .and_then(|v| v.checked_add(u_count))
        .ok_or_else(|| error(path, "required token count overflow"))?;
    ensure_available(path, required, available)?;
    let mut cpts = Vec::new();
    for _ in 0..points {
        let x = num()?;
        let y = num()?;
        let z = if dim == 2 { 0.0 } else { num()? };
        let w = if rat { num()? } else { NOW };
        cpts.push([x, y, z, w]);
    }
    let mut u = Vec::new();
    for _ in 0..u_count {
        u.push(num()?);
    }
    Ok(ParsedCrv { dim, cpts, p, u })
}
