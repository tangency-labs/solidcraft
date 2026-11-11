//! Pure SMS fixture decoding for `nlib.srf_adv_p3`.
//!
//! Deviations: reject counts at the exclusive usize bound before casting; check
//! all size arithmetic and actual numeric payload availability before growing
//! geometry incrementally. No geometry-validity or degree restrictions are added.

use crate::HarnessError;
use solidcore::nurbs::math::NOW;

pub(super) struct ParsedSurface {
    pub net: Vec<Vec<[f64; 4]>>,
    pub p: usize,
    pub q: usize,
    pub u: Vec<f64>,
    pub v: Vec<f64>,
}

pub(super) fn parse_sms(text: &str, path: &str) -> Result<ParsedSurface, HarnessError> {
    // Parse the entire text first: even an ignored malformed tail must win.
    let all = text
        .split_whitespace()
        .map(|t| {
            t.parse::<f64>()
                .map_err(|e| HarnessError::new(format!("{path}: {e}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut it = all.into_iter();
    let next = |it: &mut std::vec::IntoIter<f64>| {
        it.next()
            .ok_or_else(|| HarnessError::new(format!("{path}: truncated")))
    };
    let count = |v: f64| {
        // Unlike usize::MAX as f64, this bound is exact on both 32 and 64 bits.
        let exclusive = 2.0_f64.powi(usize::BITS as i32);
        if !v.is_finite() || v.fract() != 0.0 || v < 0.0 || v >= exclusive {
            Err(HarnessError::new(format!("{path}: bad count {v}")))
        } else {
            Ok(v as usize)
        }
    };
    let n = count(next(&mut it)?)?;
    let m = count(next(&mut it)?)?;
    let p = count(next(&mut it)?)?;
    let q = count(next(&mut it)?)?;
    let rational = next(&mut it)? != 0.0;
    let overflow = || HarnessError::new(format!("{path}: count overflow"));
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
    if payload > it.len() {
        return Err(HarnessError::new(format!("{path}: truncated")));
    }
    let mut net = Vec::new();
    for _ in 0..rows {
        let mut row = Vec::new();
        for _ in 0..cols {
            let (x, y, z) = (next(&mut it)?, next(&mut it)?, next(&mut it)?);
            let w = if rational { next(&mut it)? } else { NOW };
            row.push([x, y, z, w]);
        }
        net.push(row);
    }
    let mut u = Vec::new();
    for _ in 0..u_count {
        u.push(next(&mut it)?);
    }
    let mut v = Vec::new();
    for _ in 0..v_count {
        v.push(next(&mut it)?);
    }
    Ok(ParsedSurface { net, p, q, u, v })
}
