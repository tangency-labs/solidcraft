//! FFI wrapper for unit `nlib.srf_adv_p5`.
//!
//! Shim: `harness/cpp/shim/nlib_srf_adv_p5.cpp` (`sc_nlib_srf_adv_p5_skin`).
//! Upstream: `source/SMLib/src/NL_SrfAdv.cpp` (`N_CreateSkinSrfBoundaryContinuity`).
//!
//! The input record is `k`, the curves `0..=k`, `hasDer` (then for each of the two
//! derivative surfaces a presence flag and the surface), `cmp cnt0 cnt1 bnd0 bnd1`,
//! `hasRail` (then for each rail a presence flag and the curve), `deg ES dir`,
//! `hasTs` (then `k + 1` parameters), `hasKns` (then the knot count and the knots).
//! A surface is encoded `n m p q`, `(n+1)(m+1)` control points `x y z w` (row-major),
//! `n+p+2` u knots, `m+q+2` v knots; a curve `n p`, `n+1` control points, `n+p+2`
//! knots.
//!
//! [`skin`] accepts only finite input and well-formed clamped surfaces and curves
//! (degrees `1..=NL_MAXDEG`, more control points than the degree, non-decreasing knots
//! with end multiplicity exactly `p+1` and interior multiplicity at most `p`), and
//! rejects every argument that would make upstream index outside its arrays or read
//! uninitialised data:
//!
//! - `1 <= k < MAX_CURVES`; `deg` in `1..=MAX_SKIN_DEGREE`, `ES >= 0`; `dir` in `1..=3`,
//!   and `1..=2` with derivative surfaces; `cnt` in `1..=2`, `bnd` in `1..=4`.
//! - With `cmp` (the data is declared compatible) every cross section has the same
//!   degree, control point count and knots, and so does the cross-boundary direction of
//!   each derivative surface (after upstream re-expresses it for its boundary).
//! - A rail array holds at least one rail; two rails share degree, count and knots.
//! - `ts` (when given) is strictly increasing; `kns` (when given) is a valid clamped knot
//!   vector for the skinning degree (the rail's with rails, else `deg`) spanning the
//!   skinning parameters (`ts`, or `[0, 1]` for the computed chord-length ones).
//! - Cross sections and rails have degree at most `MAX_SKIN_DEGREE` (upstream may
//!   elevate them by one).

use crate::{call_with_buffer, FfiError};

/// Largest highest net index (`n`, `m`) of a surface accepted by [`skin`].
pub const MAX_INDEX: usize = 256;
/// Largest highest control point index of a curve accepted by [`skin`].
pub const MAX_CRV_INDEX: usize = 256;
/// Upstream `NL_MAXDEG`.
pub const MAX_DEGREE: usize = 17;
/// Largest degree of a cross section, rail or the skinning direction.
pub const MAX_SKIN_DEGREE: usize = 8;
/// Largest number of cross sections.
pub const MAX_CURVES: usize = 32;
/// Largest knot count of `kns`.
pub const MAX_KNOTS: usize = 256;

mod sys {
    extern "C" {
        pub fn sc_nlib_srf_adv_p5_skin(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

fn bad(msg: impl Into<String>) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

/// Sequential reader over the input.
struct Reader<'a> {
    input: &'a [f64],
    at: usize,
}

impl<'a> Reader<'a> {
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

    fn slice(&mut self, k: usize) -> Result<&'a [f64], FfiError> {
        let end = self.at.checked_add(k).ok_or_else(|| bad("overflow"))?;
        let s = self
            .input
            .get(self.at..end)
            .ok_or_else(|| bad("input too short"))?;
        if s.iter().any(|x| !x.is_finite()) {
            return Err(bad("non-finite input"));
        }
        self.at = end;
        Ok(s)
    }

    fn done(&self) -> Result<(), FfiError> {
        if self.at != self.input.len() {
            return Err(bad("trailing input"));
        }
        Ok(())
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

/// A validated surface: `(n, m, p, q, u knots, v knots)`.
type SrfShape<'a> = (usize, usize, usize, usize, &'a [f64], &'a [f64]);

fn surface<'a>(rd: &mut Reader<'a>) -> Result<SrfShape<'a>, FfiError> {
    let n = rd.index(1, MAX_INDEX)?;
    let m = rd.index(1, MAX_INDEX)?;
    let p = rd.index(1, MAX_DEGREE)?;
    let q = rd.index(1, MAX_DEGREE)?;
    if p > n || q > m {
        return Err(bad("degree above the highest net index"));
    }
    rd.slice(4 * (n + 1) * (m + 1))?;
    let u = rd.slice(n + p + 2)?;
    let v = rd.slice(m + q + 2)?;
    if !valid_knots(u, p) || !valid_knots(v, q) {
        return Err(bad("invalid knot vector"));
    }
    Ok((n, m, p, q, u, v))
}

/// A validated curve: `(n, p, knots)`.
fn curve<'a>(rd: &mut Reader<'a>) -> Result<(usize, usize, &'a [f64]), FfiError> {
    let n = rd.index(1, MAX_CRV_INDEX)?;
    let p = rd.index(1, MAX_DEGREE)?;
    if p > n {
        return Err(bad("degree above the highest control point index"));
    }
    rd.slice(4 * (n + 1))?;
    let u = rd.slice(n + p + 2)?;
    if !valid_knots(u, p) {
        return Err(bad("invalid knot vector"));
    }
    Ok((n, p, u))
}

/// A curve's shape: `(n, p, knots)`.
type CrvShape<'a> = (usize, usize, &'a [f64]);

fn flag(rd: &mut Reader<'_>) -> Result<bool, FfiError> {
    Ok(rd.index(0, 1)? == 1)
}

/// The shape of the cross-boundary direction of a derivative surface built for `bnd`:
/// a strip across u (`NL_LEFT` / `NL_RIGHT`) runs along v, one across v along u.
/// Upstream re-expresses the surface for its target boundary (extending it across, or
/// transposing it), which keeps that direction.
fn cross_shape<'a>(s: &SrfShape<'a>, bnd: usize) -> CrvShape<'a> {
    let (n, m, p, q, u, v) = *s;
    if bnd <= 2 {
        (m, q, v)
    } else {
        (n, p, u)
    }
}

fn validate(input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
    let k = rd.index(1, MAX_CURVES - 1)?;
    let mut shapes: Vec<CrvShape<'_>> = Vec::with_capacity(k + 1);
    for _ in 0..=k {
        let s = curve(&mut rd)?;
        if s.1 > MAX_SKIN_DEGREE || s.0 > MAX_CRV_INDEX {
            return Err(bad("cross section degree or size too large"));
        }
        shapes.push(s);
    }
    let has_der = flag(&mut rd)?;
    let mut ders: Vec<Option<SrfShape<'_>>> = Vec::new();
    if has_der {
        for _ in 0..2 {
            if flag(&mut rd)? {
                ders.push(Some(surface(&mut rd)?));
            } else {
                ders.push(None);
            }
        }
    }
    let cmp = flag(&mut rd)?;
    rd.index(1, 2)?;
    rd.index(1, 2)?;
    let bnd = [rd.index(1, 4)?, rd.index(1, 4)?];
    let has_rail = flag(&mut rd)?;
    let mut rails: Vec<CrvShape<'_>> = Vec::new();
    if has_rail {
        for _ in 0..2 {
            if flag(&mut rd)? {
                let s = curve(&mut rd)?;
                if s.1 > MAX_SKIN_DEGREE || s.0 > MAX_CRV_INDEX {
                    return Err(bad("rail degree or size too large"));
                }
                rails.push(s);
            }
        }
        if rails.is_empty() {
            return Err(bad("rail array without rails"));
        }
        if rails.windows(2).any(|w| w.first() != w.get(1)) {
            return Err(bad("incompatible rails"));
        }
    }
    let deg = rd.index(1, MAX_SKIN_DEGREE)?;
    let es = rd.num()?;
    if es < 0.0 {
        return Err(bad("negative skinning tolerance"));
    }
    let dir = rd.index(1, 3)?;
    if has_der && dir == 3 {
        return Err(bad("derivative surfaces with NL_UVDIR"));
    }
    let ts = if flag(&mut rd)? {
        let t = rd.slice(k + 1)?;
        if t.windows(2).any(|w| w.first() >= w.get(1)) {
            return Err(bad("skinning parameters not increasing"));
        }
        Some(t)
    } else {
        None
    };
    if flag(&mut rd)? {
        let cnt = rd.index(2, MAX_KNOTS)?;
        let kn = rd.slice(cnt)?;
        let ps = rails.first().map_or(deg, |r| r.1);
        if !valid_knots(kn, ps) {
            return Err(bad("invalid kns"));
        }
        let (a, b) = match ts {
            Some(t) => (t.first().copied(), t.last().copied()),
            None => (Some(0.0), Some(1.0)),
        };
        if kn.first().copied() != a || kn.last().copied() != b {
            return Err(bad("kns does not span the skinning parameters"));
        }
    }
    if cmp {
        let first = *shapes.first().ok_or_else(|| bad("no curves"))?;
        if shapes.iter().any(|s| *s != first) {
            return Err(bad("incompatible cross sections with cmp"));
        }
        for (i, d) in ders.iter().enumerate() {
            if let Some(d) = d {
                let b = bnd.get(i).copied().unwrap_or(1);
                if cross_shape(d, b) != first {
                    return Err(bad(
                        "derivative surface incompatible with the cross sections",
                    ));
                }
            }
        }
    }
    rd.done()
}

/// Runs `N_CreateSkinSrfBoundaryContinuity` on the encoded `input`; returns the shim's
/// output record.
pub fn skin(input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(input)?;
    call_with_buffer(8192, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the
        // shim reads (validated above: shapes of every curve and surface, knot
        // vectors, flags, compatibility where upstream assumes it, and every value
        // upstream uses for indexing); buf is valid for buf.len() writes and the shim
        // honours the capacity; len is valid.
        unsafe {
            sys::sc_nlib_srf_adv_p5_skin(
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
