//! FFI wrappers for unit `nlib.srf_nurb`.
//!
//! Shim: `harness/cpp/shim/nlib_srf_nurb.cpp` (`sc_nlib_srf_nurb_eval`).
//! Upstream: `source/SMLib/src/NL_SrfNurb.cpp`.
//!
//! | [`Op`]            | upstream                        | parameters after the surface |
//! |-------------------|---------------------------------|------------------------------|
//! | `BasisDerivs`     | `N_SrfBasisDerivs`              | `u v ufl vfl mfl udr vdr`    |
//! | `EvalPt`          | `N_SrfEvalPt`                   | `u v ufl vfl`                |
//! | `Derivs`          | `N_SrfDerivs`                   | `u v ufl vfl mfl udr vdr`    |
//! | `BBox`            | `N_SrfGetBBox`                  |                              |
//! | `Transform`       | `N_SrfTransform`                | `M[16]` (row major)          |
//! | `MinMaxWeightPos` | `N_SrfMinMaxWeightPosVectors`   |                              |
//! | `NormalFast`      | `N_SrfEvalPtPtDerivNormalFast`  | `u v ufl vfl`                |
//! | `Translate`       | `N_SrfTranslate`                | `T`                          |
//! | `Rotate`          | `N_SrfRotateAtPt`               | `P V al`                     |
//! | `Reverse`         | `N_SrfReverse`                  | `dir inplace`                |
//! | `Type`            | `N_SrfType`                     | `tol`                        |
//! | `Reparam`         | `N_SrfReparam`                  | `us ue vs ve`                |
//! | `ClosedSmooth`    | `N_SrfIsClosedSmooth`           | `n t0 t1 dflg`               |
//! | `NormalPole`      | `N_SrfEvalPtPtDerivNormalPole`  | `u v ufl vfl`                |
//! | `NormalAtPole`    | `N_SrfEvalPtNormalAtPole`       | `u v ufl vfl`                |
//! | `ConeElliptic`    | `N_ConeEllipticEnds`            | `rf rr h tyf tyr txf txr` (no surface) |
//!
//! Output records are described in the shim. [`eval`] parses the whole input and
//! rejects anything that could make upstream index outside its arrays: the surface
//! must have degrees `1..=NL_MAXDEG`, `p <= n`, `q <= m` and valid clamped knot
//! vectors (`N_KnotVectorIsValid`); every value must be finite; side flags must be
//! `NL_LEFT` / `NL_RIGHT`, `mfl` `0` / `1`, derivative orders at most
//! [`MAX_DERIV_ORDER`] (above `NL_MAXDER` upstream fails first), direction flags
//! `1..=3`, the closure level `0..=2`.

use crate::{call_with_buffer, FfiError};

/// Largest highest index (`n`, `m`) accepted by [`eval`].
pub const MAX_INDEX: usize = 256;
/// Upstream `NL_MAXDEG`.
pub const MAX_DEGREE: usize = 17;
/// Largest derivative order accepted by [`eval`] (`NL_MAXDER` is 4).
pub const MAX_DERIV_ORDER: usize = 6;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `N_SrfBasisDerivs`.
    BasisDerivs,
    /// `N_SrfEvalPt`.
    EvalPt,
    /// `N_SrfDerivs`.
    Derivs,
    /// `N_SrfGetBBox`.
    BBox,
    /// `N_SrfTransform`.
    Transform,
    /// `N_SrfMinMaxWeightPosVectors`.
    MinMaxWeightPos,
    /// `N_SrfEvalPtPtDerivNormalFast`.
    NormalFast,
    /// `N_SrfTranslate`.
    Translate,
    /// `N_SrfRotateAtPt`.
    Rotate,
    /// `N_SrfReverse`.
    Reverse,
    /// `N_SrfType`.
    Type,
    /// `N_SrfReparam`.
    Reparam,
    /// `N_SrfIsClosedSmooth`.
    ClosedSmooth,
    /// `N_SrfEvalPtPtDerivNormalPole`.
    NormalPole,
    /// `N_SrfEvalPtNormalAtPole`.
    NormalAtPole,
    /// `N_ConeEllipticEnds`.
    ConeElliptic,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::BasisDerivs => 0,
            Op::EvalPt => 1,
            Op::Derivs => 2,
            Op::BBox => 3,
            Op::Transform => 4,
            Op::MinMaxWeightPos => 5,
            Op::NormalFast => 6,
            Op::Translate => 7,
            Op::Rotate => 8,
            Op::Reverse => 9,
            Op::Type => 10,
            Op::Reparam => 11,
            Op::ClosedSmooth => 12,
            Op::NormalPole => 13,
            Op::NormalAtPole => 14,
            Op::ConeElliptic => 15,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_nlib_srf_nurb_eval(
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

    fn nums(&mut self, k: usize) -> Result<(), FfiError> {
        for _ in 0..k {
            self.num()?;
        }
        Ok(())
    }

    fn index(&mut self, lo: usize, hi: usize) -> Result<usize, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < lo as f64 || v > hi as f64 {
            return Err(bad(format!("bad index/flag {v}")));
        }
        Ok(v as usize)
    }

    /// `u v ufl vfl`.
    fn point_params(&mut self) -> Result<(), FfiError> {
        self.nums(2)?;
        self.index(1, 2)?;
        self.index(1, 2)?;
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

fn surface(rd: &mut Reader<'_>) -> Result<(), FfiError> {
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
    Ok(())
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    if op != Op::ConeElliptic {
        surface(&mut rd)?;
    }
    match op {
        Op::BasisDerivs | Op::Derivs => {
            rd.point_params()?;
            rd.index(0, 1)?;
            rd.index(0, MAX_DERIV_ORDER)?;
            rd.index(0, MAX_DERIV_ORDER)?;
        }
        Op::EvalPt | Op::NormalFast | Op::NormalPole | Op::NormalAtPole => rd.point_params()?,
        Op::BBox | Op::MinMaxWeightPos => {}
        Op::Transform => rd.nums(16)?,
        Op::Translate => rd.nums(3)?,
        Op::Rotate | Op::ConeElliptic => rd.nums(7)?,
        Op::Reverse => {
            rd.index(1, 3)?;
            rd.index(0, 1)?;
        }
        Op::Type => rd.nums(1)?,
        Op::Reparam => rd.nums(4)?,
        Op::ClosedSmooth => {
            rd.index(0, 2)?;
            rd.nums(2)?;
            rd.index(1, 3)?;
        }
    }
    rd.done()
}

/// Runs `op` on `input`; returns the shim's output record.
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the
        // shim reads for `op` (validated above, including the surface shape, its knot
        // vectors, every flag and derivative order upstream uses for indexing); buf
        // is valid for buf.len() writes and the shim honours the capacity; len is
        // valid.
        unsafe {
            sys::sc_nlib_srf_nurb_eval(
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
    use super::{valid_knots, validate, Op};

    #[test]
    fn knot_validation() {
        assert!(valid_knots(&[0.0, 0.0, 1.0, 1.0], 1));
        assert!(!valid_knots(&[0.0, 0.0, 0.0, 0.0], 1));
        assert!(!valid_knots(&[0.0, 0.5, 0.2, 1.0], 1));
    }

    #[test]
    fn input_validation() {
        let mut s = vec![1.0, 1.0, 1.0, 1.0];
        s.extend([0.0; 16]);
        s.extend([0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0]);
        assert!(validate(Op::BBox, &s).is_ok());
        let mut t = s.clone();
        t.extend([0.5, 0.5, 1.0, 3.0]);
        assert!(validate(Op::EvalPt, &t).is_err());
        assert!(validate(Op::ConeElliptic, &[1.0; 7]).is_ok());
    }
}
