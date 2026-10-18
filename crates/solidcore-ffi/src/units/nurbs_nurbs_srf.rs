//! FFI wrappers for unit `nurbs.nurbs_srf`.
//!
//! Shim: `harness/cpp/shim/nurbs_nurbs_srf.cpp` (`sc_nurbs_nurbs_srf_eval`).
//! Upstream: `source/SMLib/src/SmNurbsSrf.cpp`.
//!
//! | [`Op`]           | upstream                                   |
//! |------------------|--------------------------------------------|
//! | `NetConstants`   | `sm_ComputeNetConstants`                   |
//! | `SurfConstants`  | `sm_ComputeSurfConstants` (through an `SmBSplineSurface` borrowing the surface) |
//! | `Allocate`       | `sm_AllocateNurbSurface`                   |
//! | `Copy`           | `sm_AllocateAndCopyNurbSurface`            |
//! | `Decompose`      | `sm_DecomposeSrf`                          |
//! | `Split`          | `sm_SplitSrf`                              |
//! | `Degenerate`     | `sm_IsNSrfDegenerate`                      |
//!
//! Input encoding (flat doubles) and output records are described in the shim.
//! [`eval`] parses the whole input and rejects anything that could make upstream
//! index outside its arrays: the surface must have `1 <= p <= n`, `1 <= q <= m`,
//! degrees up to `NL_MAXDEG`, and valid clamped knot vectors (`N_KnotVectorIsValid`);
//! every number must be finite; flags must be `0`/`1`, directions `1..=3`; a surface
//! sampling domain must lie inside the knot ranges.

use crate::{call_with_buffer, FfiError};

/// Largest highest index (`n`, `m`) accepted by [`eval`].
pub const MAX_INDEX: usize = 256;
/// Upstream `NL_MAXDEG`.
pub const MAX_DEGREE: usize = 17;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `sm_ComputeNetConstants`; then `zone_tol`, 5 flags, `use_surface`, `ul vl uh vh`.
    NetConstants,
    /// `sm_ComputeSurfConstants`; then `ul vl uh vh`, 5 flags.
    SurfConstants,
    /// `sm_AllocateNurbSurface`; input is only `n m p q r s` (no surface).
    Allocate,
    /// `sm_AllocateAndCopyNurbSurface`.
    Copy,
    /// `sm_DecomposeSrf`.
    Decompose,
    /// `sm_SplitSrf`; then `t dir`.
    Split,
    /// `sm_IsNSrfDegenerate`; then `tol`.
    Degenerate,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::NetConstants => 0,
            Op::SurfConstants => 1,
            Op::Allocate => 2,
            Op::Copy => 3,
            Op::Decompose => 4,
            Op::Split => 5,
            Op::Degenerate => 6,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_nurbs_nurbs_srf_eval(
            op: i32,
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

/// Validates a surface; returns its `(u, v)` knots.
fn surface<'a>(rd: &mut Reader<'a>) -> Result<(&'a [f64], &'a [f64]), FfiError> {
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
    Ok((u, v))
}

/// Reads `ul vl uh vh` and checks it lies inside the knot ranges.
fn domain(rd: &mut Reader<'_>, u: &[f64], v: &[f64]) -> Result<(), FfiError> {
    let &[ul, vl, uh, vh] = rd.slice(4)? else {
        return Err(bad("input too short"));
    };
    let inside = |lo: f64, hi: f64, k: &[f64]| match (k.first(), k.last()) {
        (Some(&a), Some(&b)) => a <= lo && lo <= hi && hi <= b,
        _ => false,
    };
    if !inside(ul, uh, u) || !inside(vl, vh, v) {
        return Err(bad("sampling domain outside the knot ranges"));
    }
    Ok(())
}

fn flags(rd: &mut Reader<'_>, k: usize) -> Result<(), FfiError> {
    for _ in 0..k {
        rd.index(0, 1)?;
    }
    Ok(())
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    if op == Op::Allocate {
        for _ in 0..2 {
            rd.index(0, MAX_INDEX)?;
        }
        for _ in 0..2 {
            rd.index(0, MAX_DEGREE)?;
        }
        for _ in 0..2 {
            rd.index(0, 2 * MAX_INDEX)?;
        }
        return rd.done();
    }
    let (u, v) = surface(&mut rd)?;
    match op {
        Op::NetConstants => {
            rd.num()?;
            flags(&mut rd, 6)?;
            domain(&mut rd, u, v)?;
        }
        Op::SurfConstants => {
            domain(&mut rd, u, v)?;
            flags(&mut rd, 5)?;
        }
        Op::Allocate | Op::Copy | Op::Decompose => {}
        Op::Split => {
            rd.num()?;
            rd.index(1, 3)?;
        }
        Op::Degenerate => {
            rd.num()?;
        }
    }
    rd.done()
}

/// Runs `op` on `input`; returns the shim's output record.
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the
        // shim reads for `op` (validated above: the surface shape, its knot vectors,
        // every flag, index and sampling domain upstream uses); buf is valid for
        // buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_nurbs_nurbs_srf_eval(
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

#[cfg(test)]
mod tests {
    use super::valid_knots;

    #[test]
    fn knot_validation() {
        assert!(valid_knots(&[0.0, 0.0, 1.0, 1.0], 1));
        assert!(!valid_knots(
            &[0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0],
            2
        ));
        assert!(!valid_knots(&[0.0, 0.5, 0.2, 1.0], 1));
    }
}
