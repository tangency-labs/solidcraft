//! Pure SMS/SMC fixture readers for `nlib.srf_adv_p1`.
//!
//! Deviations: reject unrepresentable counts and checked size overflow, and preflight
//! the numeric payload before building geometry. All tokens are parsed eagerly,
//! including ignored numeric tails, preserving float-error precedence.

use solidcore::nurbs::math::{NOW, NOZ};

#[derive(Debug, PartialEq)]
pub(crate) struct Surface {
    pub(crate) net: Vec<Vec<[f64; 4]>>,
    pub(crate) p: usize,
    pub(crate) q: usize,
    pub(crate) u: Vec<f64>,
    pub(crate) v: Vec<f64>,
}

#[derive(Debug, PartialEq)]
pub(crate) struct Curve {
    pub(crate) cpts: Vec<[f64; 4]>,
    pub(crate) p: usize,
    pub(crate) u: Vec<f64>,
}

struct Numbers<'a> {
    path: &'a str,
    values: std::vec::IntoIter<f64>,
}

impl<'a> Numbers<'a> {
    fn new(path: &'a str, source: &str) -> Result<Self, String> {
        let values = source
            .split_whitespace()
            .map(|t| t.parse::<f64>().map_err(|e| format!("{path}: {e}")))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            path,
            values: values.into_iter(),
        })
    }

    fn num(&mut self) -> Result<f64, String> {
        self.values
            .next()
            .ok_or_else(|| format!("{}: truncated", self.path))
    }

    fn count(&mut self) -> Result<usize, String> {
        let v = self.num()?;
        // usize::MAX rounds up on 64-bit targets; the upper bound is exclusive.
        if !v.is_finite() || v.fract() != 0.0 || v < 0.0 || v >= 2.0_f64.powi(usize::BITS as i32) {
            return Err(format!("{}: bad count {v}", self.path));
        }
        Ok(v as usize)
    }

    fn size(&self, size: Option<usize>) -> Result<usize, String> {
        size.ok_or_else(|| format!("{}: count overflow", self.path))
    }

    fn preflight(&self, size: usize) -> Result<(), String> {
        if size > self.values.len() {
            return Err(format!("{}: truncated", self.path));
        }
        Ok(())
    }

    fn knots(&mut self, count: usize) -> Result<Vec<f64>, String> {
        let mut knots = Vec::new();
        for _ in 0..count {
            knots.push(self.num()?);
        }
        Ok(knots)
    }
}

pub(crate) fn parse_sms(path: &str, source: &str) -> Result<Surface, String> {
    let mut nums = Numbers::new(path, source)?;
    let n = nums.count()?;
    let m = nums.count()?;
    let p = nums.count()?;
    let q = nums.count()?;
    let rat = nums.num()? != 0.0;
    let rows = nums.size(n.checked_add(1))?;
    let cols = nums.size(m.checked_add(1))?;
    let u_count = nums.size(n.checked_add(p).and_then(|v| v.checked_add(2)))?;
    let v_count = nums.size(m.checked_add(q).and_then(|v| v.checked_add(2)))?;
    let stride = nums.size(3_usize.checked_add(usize::from(rat)))?;
    let payload = nums.size(
        rows.checked_mul(cols)
            .and_then(|v| v.checked_mul(stride))
            .and_then(|v| v.checked_add(u_count))
            .and_then(|v| v.checked_add(v_count)),
    )?;
    nums.preflight(payload)?;
    let mut net = Vec::new();
    for _ in 0..rows {
        let mut row = Vec::new();
        for _ in 0..cols {
            let (x, y, z) = (nums.num()?, nums.num()?, nums.num()?);
            let w = if rat { nums.num()? } else { NOW };
            row.push([x, y, z, w]);
        }
        net.push(row);
    }
    let u = nums.knots(u_count)?;
    let v = nums.knots(v_count)?;
    Ok(Surface { net, p, q, u, v })
}

pub(crate) fn parse_smc(path: &str, source: &str) -> Result<Curve, String> {
    let mut nums = Numbers::new(path, source)?;
    let n = nums.count()?;
    let p = nums.count()?;
    let rat = nums.num()? != 0.0;
    let dim = nums.count()?;
    let points = nums.size(n.checked_add(1))?;
    let u_count = nums.size(n.checked_add(p).and_then(|v| v.checked_add(2)))?;
    let stride = nums.size(
        2_usize
            .checked_add(usize::from(dim == 3))
            .and_then(|v| v.checked_add(usize::from(rat))),
    )?;
    let payload = nums.size(
        points
            .checked_mul(stride)
            .and_then(|v| v.checked_add(u_count)),
    )?;
    nums.preflight(payload)?;
    let mut cpts = Vec::new();
    for _ in 0..points {
        let (x, y) = (nums.num()?, nums.num()?);
        let z = if dim == 3 { nums.num()? } else { NOZ };
        let w = if rat { nums.num()? } else { NOW };
        cpts.push([x, y, z, w]);
    }
    let u = nums.knots(u_count)?;
    Ok(Curve { cpts, p, u })
}
