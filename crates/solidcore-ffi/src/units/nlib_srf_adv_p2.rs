//! FFI wrappers for unit `nlib.srf_adv_p2`.
//!
//! Shim: `harness/cpp/shim/nlib_srf_adv_p2.cpp` (`sc_nlib_srf_adv_p2_eval`).
//! Upstream: `source/SMLib/src/NL_SrfAdv.cpp`.
//!
//! | [`Op`]          | upstream                                  |
//! |-----------------|-------------------------------------------|
//! | `ExtendByDist`  | `ST_surxsdr` (core of `N_SrfExtendByDist`) |
//! | `NormalDeriv`   | `N_SrfEvalPtNormalDeriv`                  |
//!
//! Input encoding (flat doubles) and output records are described in the shim.
//! [`eval`] parses the whole input and rejects anything that could make upstream
//! index outside its arrays: the surface must have degrees `1..=NL_MAXDEG`,
//! `r = n + p + 1`, `s = m + q + 1` and valid clamped knot vectors
//! (`N_KnotVectorIsValid`: end multiplicities `p + 1`, non-decreasing, interior
//! multiplicities at most `p`), weights all `NOW` or all positive; every coordinate
//! and parameter must be finite. The
//! extension takes `dist dflg end cont depth inplace` with `dflg` in `1..=2`, `end`
//! in `1..=2`, `cont` one of `2, 3, 4, 6` (with `2` or `3`, a non-zero distance whose
//! parameter step is not lost to rounding, see `g2_extension_ok`) and
//! `depth` in `0..=11`; the normal
//! derivatives take `u v ufl vfl` with side flags in `1..=2`.

use crate::validate::{bad, euclid, NOW};
use crate::{call_with_buffer, FfiError};

/// Largest highest index (`n`, `m`) accepted by [`eval`].
pub const MAX_INDEX: usize = 256;
/// Upstream `NL_MAXDEG`.
pub const MAX_DEGREE: usize = 17;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `ST_surxsdr`; then `dist dflg end cont depth inplace`.
    ExtendByDist,
    /// `N_SrfEvalPtNormalDeriv`; then `u v ufl vfl`.
    NormalDeriv,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::ExtendByDist => 0,
            Op::NormalDeriv => 1,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_nlib_srf_adv_p2_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

crate::reader::unit_reader!();

impl Reader<'_> {
    fn num(&mut self) -> Result<f64, FfiError> {
        let v = self
            .input
            .get(self.at)
            .copied()
            .ok_or_else(|| bad("input too short"))?;
        self.at += 1;
        if !v.is_finite() {
            return Err(bad("non-finite input"));
        }
        Ok(v)
    }

    fn index(&mut self, lo: usize, hi: usize) -> Result<usize, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < lo as f64 || v > hi as f64 {
            return Err(bad(format!("bad index/count {v}")));
        }
        Ok(v as usize)
    }
}

/// `N_KnotVectorIsValid` for degree `p` (end multiplicities exactly `p + 1`,
/// non-decreasing, interior multiplicities at most `p`).
fn valid_knots(u: &[f64], p: usize) -> bool {
    let (Some(&first), Some(&last)) = (u.first(), u.last()) else {
        return false;
    };
    if u.len() < 2 * p + 2 || first >= last || u.windows(2).any(|w| w.first() > w.get(1)) {
        return false;
    }
    // Runs of equal knots: the ends have exactly p + 1, interior ones at most p.
    let mut runs: Vec<(f64, usize)> = Vec::new();
    for &x in u {
        match runs.last_mut() {
            Some((y, k)) if *y == x => *k += 1,
            _ => runs.push((x, 1)),
        }
    }
    runs.iter().all(|&(x, k)| {
        if x == first || x == last {
            k == p + 1
        } else {
            k <= p
        }
    })
}

/// Surface `(n, m, p, q)` after validation.
fn surface(rd: &mut Reader<'_>) -> Result<[usize; 4], FfiError> {
    let n = rd.index(1, MAX_INDEX)?;
    let m = rd.index(1, MAX_INDEX)?;
    let p = rd.index(1, MAX_DEGREE)?;
    let q = rd.index(1, MAX_DEGREE)?;
    if p > n || q > m {
        return Err(bad("degree above the highest net index"));
    }
    let net = rd.slice(4 * (n + 1) * (m + 1))?;
    // Weights: all NOW (non-rational) or all positive. Anything else is no valid
    // surface; a zero weight makes Euclidean points infinite, and the extension then
    // reads past its scratch rows when a reflection plane cannot be normalised.
    let ws: Vec<f64> = net.chunks(4).filter_map(|c| c.get(3).copied()).collect();
    if !(ws.iter().all(|&w| w == NOW) || ws.iter().all(|&w| w > 0.0)) {
        return Err(bad("mixed or non-positive weights"));
    }
    let u = rd.slice(n + p + 2)?;
    let v = rd.slice(m + q + 2)?;
    if !valid_knots(u, p) || !valid_knots(v, q) {
        return Err(bad("invalid knot vector"));
    }
    Ok([n, m, p, q])
}

/// Smallest parameter step of the `NL_G2` search, in units of `f64::EPSILON` times the
/// largest knot magnitude (see `g2_extension_ok`).
pub const MIN_G2_STEP_EPS: f64 = 1024.0;

/// The validated surface at the start of `input` (`n m p q`, the net, the u and v
/// knots): a bound on the control polygon length of every iso-curve running in u
/// (`udir`) or v, and the knots of that direction. The bound is the sum, over
/// consecutive net rows (columns), of the largest distance between corresponding
/// Euclidean points; it is infinite when a distance is not finite.
fn iso_polygon_bound(input: &[f64], udir: bool) -> Result<(f64, &[f64]), FfiError> {
    let head = |k: usize| {
        input
            .get(k)
            .map(|&x| x as usize)
            .ok_or_else(|| bad("input too short"))
    };
    let (n, m, p, q) = (head(0)?, head(1)?, head(2)?, head(3)?);
    let pt = |i: usize, j: usize| -> Result<[f64; 3], FfiError> {
        let at = 4 + 4 * (i * (m + 1) + j);
        match input.get(at..at + 4) {
            Some(&[x, y, z, w]) => Ok(euclid([x, y, z, w])),
            _ => Err(bad("input too short")),
        }
    };
    let u0 = 4 + 4 * (n + 1) * (m + 1);
    let v0 = u0 + n + p + 2;
    let knots = if udir {
        input.get(u0..v0)
    } else {
        input.get(v0..v0 + m + q + 2)
    }
    .ok_or_else(|| bad("input too short"))?;
    let dist = |a: [f64; 3], b: [f64; 3]| {
        let [dx, dy, dz] = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        (dx * dx + dy * dy + dz * dz).sqrt()
    };
    let (rows, cols) = if udir { (n, m) } else { (m, n) };
    let mut total = 0.0;
    for a in 0..rows {
        let mut longest: f64 = 0.0;
        for b in 0..=cols {
            let (p0, p1) = if udir {
                (pt(a, b)?, pt(a + 1, b)?)
            } else {
                (pt(b, a)?, pt(b, a + 1)?)
            };
            let d = dist(p0, p1);
            if !d.is_finite() {
                return Ok((f64::INFINITY, knots));
            }
            longest = longest.max(d);
        }
        total += longest;
    }
    Ok((total, knots))
}

/// `ST_surxsdr` extends sampled iso-curves with `ST_curxcdr` `NL_G2` for `cont`
/// `NL_G1` / `NL_G2`. Its parameter search steps by `du = range * dist / d1` (`d1` the
/// iso-curve's arc length) and never ends when that step is lost to rounding (zero
/// or tiny `dist`, a huge or overflowing iso-curve length; HANG-6). So `dist` must
/// be non-zero and the step
/// estimated with [`iso_polygon_bound`] in place of `d1` (a lower bound of `du` for
/// non-rational surfaces) at least [`MIN_G2_STEP_EPS`] `f64::EPSILON`s of the largest
/// knot magnitude.
fn g2_extension_ok(input: &[f64], dist: f64, udir: bool, cont: usize) -> Result<(), FfiError> {
    if cont != 2 && cont != 3 {
        return Ok(());
    }
    let (bound, knots) = iso_polygon_bound(input, udir)?;
    let (Some(&first), Some(&last)) = (knots.first(), knots.last()) else {
        return Err(bad("empty knot vector"));
    };
    let step = (last - first) * (dist / bound);
    let floor = MIN_G2_STEP_EPS * f64::EPSILON * first.abs().max(last.abs());
    if !(step.abs() >= floor && dist != 0.0) {
        return Err(bad(
            "extend by distance: G1/G2 step lost to rounding (distance too short)",
        ));
    }
    Ok(())
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    surface(&mut rd)?;
    match op {
        Op::ExtendByDist => {
            let dist = rd.num()?;
            let udir = rd.index(1, 2)? == 1;
            rd.index(1, 2)?;
            let cont = rd.index(2, 6)?;
            if cont == 5 {
                return Err(bad("bad continuity flag"));
            }
            rd.index(0, 11)?;
            rd.index(0, 1)?;
            g2_extension_ok(input, dist, udir, cont)?;
        }
        Op::NormalDeriv => {
            rd.num()?;
            rd.num()?;
            rd.index(1, 2)?;
            rd.index(1, 2)?;
        }
    }
    rd.done()
}

/// Runs `op` on `input`; returns the shim's output record.
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(8192, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the
        // shim reads for `op` (validated above, including the surface shape, its knot
        // vectors and every flag); buf is valid for buf.len() writes and the shim
        // honours the capacity; len is valid.
        unsafe {
            sys::sc_nlib_srf_adv_p2_eval(
                op.code(),
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
