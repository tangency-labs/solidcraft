//! FFI wrappers for unit `nlib.srf_adv_p6`.
//!
//! Shim: `harness/cpp/shim/nlib_srf_adv_p6.cpp` (`sc_nlib_srf_adv_p6_sweep`,
//! `sc_nlib_srf_adv_p6_fill`). Upstream: `source/SMLib/src/NL_SrfAdv.cpp`
//! (`N_CreateSweepScale`, `N_FillNSidedHole`).
//!
//! A curve is encoded `n p`, `n+1` control points `x y z w`, `n+p+2` knots.
//!
//! [`sweep`] takes `curT curC Z0x Z0y Z0z hasZ [curZ] hasS [curS] tflg tc deg dir E`;
//! [`fill`] takes `k curves[0..=k] hasDer [has_i curve_i]... eps flg CIx CIy CIz NIx
//! NIy NIz`. Both accept only finite input and well-formed clamped curves (degrees
//! `1..=MAX_CRV_DEGREE`, more control points than the degree, non-decreasing knots
//! with end multiplicity exactly `p+1` and interior multiplicity at most `p`), and
//! reject arguments that would make upstream index outside its arrays:
//!
//! - sweep: `deg` in `1..=MAX_SWEEP_DEGREE`, `dir` in `1..=2`, `E > 0` and at least
//!   [`MIN_SWEEP_TOL_FRACTION`] of the trajectory's control polygon length: the
//!   trajectory is approximated by a polyline within `E / 2`, and
//!   `N_ApproxCrvWithPolyline` subdivides without end below floating-point
//!   resolution (HANG-5; a tiny `E`, or a trajectory with huge coordinates).
//! - fill: `1 <= k < MAX_SIDES` (upstream rejects `k < 2` before any access), `eps > 0`.

use crate::validate::{bad, euclid};
use crate::{call_with_buffer, FfiError};

/// Largest highest control point index of a curve.
pub const MAX_CRV_INDEX: usize = 64;
/// Largest degree of an input curve.
pub const MAX_CRV_DEGREE: usize = 6;
/// Largest sweep degree.
pub const MAX_SWEEP_DEGREE: usize = 5;
/// Smallest sweeping tolerance `E`, relative to the trajectory's control polygon
/// length (see the module docs).
pub const MIN_SWEEP_TOL_FRACTION: f64 = 1e-6;
/// Largest number of boundary curves of a hole.
pub const MAX_SIDES: usize = 12;

mod sys {
    extern "C" {
        pub fn sc_nlib_srf_adv_p6_sweep(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_nlib_srf_adv_p6_fill(
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
            return Err(bad(format!("bad index/count/flag {v}")));
        }
        Ok(v as usize)
    }

    fn flag(&mut self) -> Result<bool, FfiError> {
        Ok(self.index(0, 1)? == 1)
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

/// Validates a curve; returns the length of its (Euclidean) control polygon.
fn curve(rd: &mut Reader<'_>) -> Result<f64, FfiError> {
    let n = rd.index(1, MAX_CRV_INDEX)?;
    let p = rd.index(1, MAX_CRV_DEGREE)?;
    if p > n {
        return Err(bad("degree above the highest control point index"));
    }
    let pts = rd.slice(4 * (n + 1))?;
    // Weights: all NOW (non-rational) or all positive.
    let now = -f64::MAX;
    let ws: Vec<f64> = pts.chunks(4).filter_map(|c| c.get(3).copied()).collect();
    if !(ws.iter().all(|&w| w == now) || ws.iter().all(|&w| w > 0.0)) {
        return Err(bad("mixed or non-positive weights"));
    }
    let u = rd.slice(n + p + 2)?;
    if !valid_knots(u, p) {
        return Err(bad("invalid knot vector"));
    }
    let e: Vec<[f64; 3]> = pts
        .chunks(4)
        .filter_map(|c| match *c {
            [x, y, z, w] => Some(euclid([x, y, z, w])),
            _ => None,
        })
        .collect();
    Ok(e.windows(2)
        .map(|w| match *w {
            [[ax, ay, az], [bx, by, bz]] => {
                let (dx, dy, dz) = (bx - ax, by - ay, bz - az);
                (dx * dx + dy * dy + dz * dz).sqrt()
            }
            _ => 0.0,
        })
        .sum())
}

fn validate_sweep(input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    let traj_len = curve(&mut rd)?;
    curve(&mut rd)?;
    rd.slice(3)?;
    if rd.flag()? {
        curve(&mut rd)?;
    }
    if rd.flag()? {
        curve(&mut rd)?;
    }
    rd.flag()?;
    rd.num()?;
    rd.index(1, MAX_SWEEP_DEGREE)?;
    rd.index(1, 2)?;
    let e = rd.num()?;
    if e <= 0.0 {
        return Err(bad("sweeping tolerance must be positive"));
    }
    // (NaN `e` or length is rejected too)
    if e.partial_cmp(&(MIN_SWEEP_TOL_FRACTION * traj_len)).is_none_or(std::cmp::Ordering::is_lt) {
        return Err(bad("sweeping tolerance too small for the trajectory"));
    }
    rd.done()
}

fn validate_fill(input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    let k = rd.index(1, MAX_SIDES - 1)?;
    for _ in 0..=k {
        curve(&mut rd)?;
    }
    if rd.flag()? {
        for _ in 0..=k {
            if rd.flag()? {
                curve(&mut rd)?;
            }
        }
    }
    let eps = rd.num()?;
    if eps <= 0.0 {
        return Err(bad("angular tolerance must be positive"));
    }
    rd.flag()?;
    rd.slice(6)?;
    rd.done()
}

/// Runs `N_CreateSweepScale` on the encoded `input`; returns the shim's output record
/// (status, then the surface on success).
pub fn sweep(input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate_sweep(input)?;
    call_with_buffer(8192, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads (validated above: curve shapes, knot vectors, flags and degrees); buf
        // is valid for buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_nlib_srf_adv_p6_sweep(
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Runs `N_FillNSidedHole` on the encoded `input`; returns the shim's output record
/// (status, then the patches, the center and normal, and the curves after the call).
pub fn fill(input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate_fill(input)?;
    call_with_buffer(8192, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads (validated above: curve shapes, knot vectors, flags and k); buf is
        // valid for buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_nlib_srf_adv_p6_fill(
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
