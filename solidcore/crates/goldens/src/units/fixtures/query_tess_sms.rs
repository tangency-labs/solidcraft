//! Pure text reader for query.tess SMS fixtures; no geometry validation is performed.
//!
//! Counts are bounded by usize's exclusive upper limit. Payload checks are staged:
//! net truncation precedes U arithmetic, and U consumption precedes V arithmetic.

use solidcore::nurbs::math::NOW;

/// Surface data in the original homogeneous, v-fastest storage order.
#[derive(Debug)]
pub struct ParsedSurface {
    /// Control net, with rows in u.
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

fn number(it: &mut std::vec::IntoIter<f64>) -> Result<f64, String> {
    it.next().ok_or_else(|| "truncated".to_owned())
}

fn count(it: &mut std::vec::IntoIter<f64>) -> Result<usize, String> {
    let value = number(it)?;
    // usize::MAX rounds UP on 64-bit, so comparison with MAX as f64 is unsafe.
    let exclusive = 2.0_f64.powi(usize::BITS as i32);
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value >= exclusive {
        return Err(format!("bad count {value}"));
    }
    Ok(value as usize)
}

fn knots(it: &mut std::vec::IntoIter<f64>, last: usize, degree: usize) -> Result<Vec<f64>, String> {
    let len = last
        .checked_add(degree)
        .and_then(|n| n.checked_add(2))
        .ok_or_else(|| "count overflow".to_owned())?;
    if len > it.len() {
        return Err("truncated".to_owned());
    }
    let mut values = Vec::new();
    for _ in 0..len {
        values.push(number(it)?);
    }
    Ok(values)
}

/// Parse all numeric tokens eagerly, including ignored trailing tokens, before headers.
pub fn parse(text: &str) -> Result<ParsedSurface, String> {
    let mut values = Vec::new();
    for token in text.split_whitespace() {
        values.push(token.parse::<f64>().map_err(|e| e.to_string())?);
    }
    let mut it = values.into_iter();
    let n = count(&mut it)?;
    let m = count(&mut it)?;
    let p = count(&mut it)?;
    let q = count(&mut it)?;
    let rational = number(&mut it)? != 0.0;
    // Only preflight the net here: later knot arithmetic must not mask its EOF.
    let net_len = n
        .checked_add(1)
        .and_then(|rows| m.checked_add(1).and_then(|cols| rows.checked_mul(cols)))
        .and_then(|points| points.checked_mul(if rational { 4 } else { 3 }));
    if net_len.is_none_or(|len| len > it.len()) {
        return Err("truncated".to_owned());
    }
    let mut net = Vec::new();
    for _ in 0..=n {
        let mut row = Vec::new();
        for _ in 0..=m {
            let (x, y, z) = (number(&mut it)?, number(&mut it)?, number(&mut it)?);
            let w = if rational { number(&mut it)? } else { NOW };
            row.push([x, y, z, w]);
        }
        net.push(row);
    }
    let u = knots(&mut it, n, p)?;
    let v = knots(&mut it, m, q)?;
    Ok(ParsedSurface { net, p, q, u, v })
}
