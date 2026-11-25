//! Standalone decoder for the P7 runner's SMS text input.
//!
//! Counts are rounded through f64 as before, but must fit the exclusive usize
//! bound. Knot additions are checked at their original processing positions:
//! net first, then u sizing/consumption, then v sizing/consumption. These formerly
//! overflowing sizes now return errors. Geometry validation remains downstream.

/// Text payload, before the adapter supplies the B-spline form and oob defaults.
#[derive(Debug, PartialEq)]
pub(crate) struct Parsed {
    pub(crate) net: Vec<Vec<[f64; 4]>>,
    pub(crate) p: usize,
    pub(crate) q: usize,
    pub(crate) u: Vec<f64>,
    pub(crate) v: Vec<f64>,
}

fn count(path: &str, value: f64) -> Result<usize, String> {
    // Unlike usize::MAX as f64, this is exclusive even when MAX rounds upward.
    let exclusive = 2.0_f64.powi(usize::BITS as i32);
    if !value.is_finite() || value.fract() != 0.0 || value < 0.0 || value >= exclusive {
        return Err(format!("{path}: bad count {value}"));
    }
    Ok(value as usize)
}

/// Checked left-to-right knot sizing, shared with the focused arithmetic tests.
pub(crate) fn knot_count(path: &str, n: usize, p: usize) -> Result<usize, String> {
    n.checked_add(p)
        .and_then(|sum| sum.checked_add(2))
        .ok_or_else(|| format!("{path}: knot count overflow"))
}

/// Decode all floats eagerly, retaining path-qualified errors and numeric tails.
/// `now` is the production NOW sentinel for nonrational control points.
pub(crate) fn parse(path: &str, text: &str, now: f64) -> Result<Parsed, String> {
    let values = text
        .split_whitespace()
        .map(|token| token.parse::<f64>().map_err(|e| format!("{path}: {e}")))
        .collect::<Result<Vec<_>, _>>()?;
    let mut it = values.into_iter();
    let mut num = || it.next().ok_or_else(|| format!("{path}: truncated"));
    let n = count(path, num()?)?;
    let m = count(path, num()?)?;
    let p = count(path, num()?)?;
    let q = count(path, num()?)?;
    let rational = num()? != 0.0;
    let mut net = Vec::new();
    for _ in 0..=n {
        let mut row = Vec::new();
        for _ in 0..=m {
            let (x, y, z) = (num()?, num()?, num()?);
            let w = if rational { num()? } else { now };
            row.push([x, y, z, w]);
        }
        net.push(row);
    }
    let mut u = Vec::new();
    for _ in 0..knot_count(path, n, p)? {
        u.push(num()?);
    }
    let mut v = Vec::new();
    for _ in 0..knot_count(path, m, q)? {
        v.push(num()?);
    }
    Ok(Parsed { net, p, q, u, v })
}
