//! FFI wrappers for unit `nlib.srf_adv_p1`.
//!
//! Shim: `harness/cpp/shim/nlib_srf_adv_p1.cpp` (`sc_nlib_srf_adv_p1_eval`).
//! Upstream: `source/SMLib/src/NL_SrfAdv.cpp`.
//!
//! | [`Op`]          | upstream                           | input                                   |
//! |-----------------|------------------------------------|-----------------------------------------|
//! | `ExtendByDist`  | `N_SrfExtendByDist`                | surface, `dist dflg end cont inplace`   |
//! | `MaxDeriv`      | `N_SrfOffsetGetMaxDeriv`           | surface, `d`                            |
//! | `Unbounded`     | `N_SrfEvalPtDerivsUnbounded`       | surface, `cflg ndr cnt`, `cnt` `(u, v)` |
//! | `SkinSpine`     | `N_CreateSkinSpine`                | `k`, curves `0..=k`, spine, params `0..=k`, `cmp align z0x z0y z0z subdiv dflag deg dir` |
//! | `VectorField`   | `N_CrossBoundaryDerivsVectorField` | curve, vector field curve, `bndy`       |
//!
//! A surface is encoded `n m p q`, `(n+1)(m+1)` control points `x y z w` (row-major),
//! `n+p+2` u knots, `m+q+2` v knots; a curve `n p`, `n+1` control points, `n+p+2`
//! knots. [`eval`] accepts only well-formed clamped surfaces and curves (degrees
//! `1..=NL_MAXDEG`, at least degree + 1 control points, finite data, non-decreasing
//! knots with end multiplicity exactly `p+1` and interior multiplicity at most `p`;
//! surface weights all `NOW` or all positive)
//! and rejects every argument that would make upstream index outside its arrays or
//! loop without end:
//!
//! - `ExtendByDist`: `dflg` `1..=2`, `end` `1..=2`, `cont` one of `2, 3, 4, 6`; with
//!   `cont` `2` or `3` a distance of at least a parameter step that
//!   is not lost to rounding (see `g2_extension_ok`).
//! - `Unbounded`: `cflg` one of `2, 3, 4, 6`; `ndr` `0..=`[`MAX_NDR`]; `1..=`[`MAX_EVALS`]
//!   evaluations; each parameter either within [`MAX_REACH`] domain lengths of the
//!   domain (bounding the periodic wrap loops) or beyond `1e12` in magnitude
//!   (upstream's immediate error exit).
//! - `SkinSpine`: `1 <= k <= MAX_CURVES - 1`; `1 <= deg <= NL_MAXDEG`; `dflag`
//!   `1..=3`; `dir` `1..=3`; `subdiv` `0..=`[`MAX_SUBDIV`]; spine parameters inside
//!   the spine's domain; without `cmp` the profiles must already share degree,
//!   control point count and knots (upstream copies `cur[0]`'s count from every
//!   profile).
//! - `VectorField`: `bndy` `1..=4`. Two valid curves with equal knot vectors have
//!   equal degrees and control point counts, so upstream never reads past the
//!   vector field's control points.

use crate::validate::{bad, euclid, NOW};
use crate::{call_with_buffer, FfiError};

/// Largest highest net index (`n`, `m`) of a surface accepted by [`eval`].
pub const MAX_INDEX: usize = 256;
/// Largest highest control point index of a curve accepted by [`eval`].
pub const MAX_CRV_INDEX: usize = 4096;
/// Upstream `NL_MAXDEG`.
pub const MAX_DEGREE: usize = 17;
/// Largest number of profiles of `SkinSpine`.
pub const MAX_CURVES: usize = 32;
/// Largest derivative order of `Unbounded`.
pub const MAX_NDR: usize = 4;
/// Largest number of evaluations of one `Unbounded` call.
pub const MAX_EVALS: usize = 64;
/// Largest distance of an `Unbounded` parameter from the domain, in domain lengths.
pub const MAX_REACH: f64 = 16.0;
/// Largest subdivision level of `SkinSpine`.
pub const MAX_SUBDIV: usize = 16;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `N_SrfExtendByDist`.
    ExtendByDist,
    /// `N_SrfOffsetGetMaxDeriv`.
    MaxDeriv,
    /// `N_SrfEvalPtDerivsUnbounded` (a sequence of calls sharing `esur`).
    Unbounded,
    /// `N_CreateSkinSpine`.
    SkinSpine,
    /// `N_CrossBoundaryDerivsVectorField`.
    VectorField,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::ExtendByDist => 0,
            Op::MaxDeriv => 1,
            Op::Unbounded => 2,
            Op::SkinSpine => 3,
            Op::VectorField => 4,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_nlib_srf_adv_p1_eval(
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
            return Err(bad(format!("bad index/count/flag {v}")));
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

/// A validated surface: `(p, q, u knots, v knots)`.
type SrfShape<'a> = (usize, usize, &'a [f64], &'a [f64]);

fn surface<'a>(rd: &mut Reader<'a>) -> Result<SrfShape<'a>, FfiError> {
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
    Ok((p, q, u, v))
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

fn cont_flag(rd: &mut Reader<'_>) -> Result<usize, FfiError> {
    let cont = rd.index(2, 6)?;
    if cont == 5 {
        return Err(bad("unsupported continuity flag"));
    }
    Ok(cont)
}

fn ends(k: &[f64]) -> Result<(f64, f64), FfiError> {
    match (k.first(), k.last()) {
        (Some(&a), Some(&b)) => Ok((a, b)),
        _ => Err(bad("empty knots")),
    }
}

fn check_reach(t: f64, k: &[f64]) -> Result<(), FfiError> {
    let (a, b) = ends(k)?;
    let len = b - a;
    if t.abs() > 1.0e12 || (t >= a - MAX_REACH * len && t <= b + MAX_REACH * len) {
        Ok(())
    } else {
        Err(bad(
            "unbounded evaluation parameter too far from the domain",
        ))
    }
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    match op {
        Op::ExtendByDist => {
            surface(&mut rd)?;
            let dist = rd.num()?;
            let udir = rd.index(1, 2)? == 1;
            rd.index(1, 2)?;
            let cont = cont_flag(&mut rd)?;
            rd.index(0, 1)?;
            g2_extension_ok(input, dist, udir, cont)?;
        }
        Op::MaxDeriv => {
            surface(&mut rd)?;
            rd.num()?;
        }
        Op::Unbounded => {
            let (_, _, uk, vk) = surface(&mut rd)?;
            cont_flag(&mut rd)?;
            rd.index(0, MAX_NDR)?;
            let cnt = rd.index(1, MAX_EVALS)?;
            for _ in 0..cnt {
                let u = rd.num()?;
                let v = rd.num()?;
                check_reach(u, uk)?;
                check_reach(v, vk)?;
            }
        }
        Op::SkinSpine => {
            let k = rd.index(1, MAX_CURVES - 1)?;
            let mut shapes = Vec::with_capacity(k + 1);
            for _ in 0..=k {
                shapes.push(curve(&mut rd)?);
            }
            let (_, _, sk) = curve(&mut rd)?;
            let (a, b) = ends(sk)?;
            let params = rd.slice(k + 1)?;
            if params.iter().any(|&t| t < a || t > b) {
                return Err(bad("spine parameter outside the spine's domain"));
            }
            let cmp = rd.index(0, 1)?;
            rd.index(0, 1)?;
            rd.slice(3)?;
            rd.index(0, MAX_SUBDIV)?;
            rd.index(1, 3)?;
            rd.index(1, MAX_DEGREE)?;
            rd.index(1, 3)?;
            if cmp == 0 {
                let first = shapes.first().ok_or_else(|| bad("no curves"))?;
                if shapes.iter().any(|s| s != first) {
                    return Err(bad("incompatible profiles without cmp"));
                }
            }
        }
        Op::VectorField => {
            curve(&mut rd)?;
            curve(&mut rd)?;
            rd.index(1, 4)?;
        }
    }
    rd.done()
}

/// Runs `op` on `input`; returns the shim's output record.
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the
        // shim reads for `op` (validated above, including surface and curve shapes,
        // knot vectors, flags and every parameter upstream uses for indexing or
        // looping); buf is valid for buf.len() writes and the shim honours the
        // capacity; len is valid.
        unsafe {
            sys::sc_nlib_srf_adv_p1_eval(
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
