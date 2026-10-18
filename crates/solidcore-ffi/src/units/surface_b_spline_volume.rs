//! FFI wrappers for unit `surface.b_spline_volume`.
//!
//! Shim: `harness/cpp/shim/surface_b_spline_volume.cpp` (`sc_surface_b_spline_volume_eval`).
//! Upstream: `source/SMLib/src/SmBSplineVolume.cpp`, `source/SMLib/inc/SmBSplineVolume.h`.
//!
//! Every op starts with a volume block: `l m n p q r` (highest control point indices
//! and degrees), the knots `U` (`l+p+2` values), `V` (`m+q+2`), `W` (`n+r+2`), the
//! control points `x y z w` with the `w` index fastest, then `v`, then `u`;
//! `has_orient [O(12)]` and `n_next` followed by `T(12)` per next map. The record is
//! filled into `sm_AllocateNurbVolume` and becomes `SmBSplineVolume(ctx, rec)`; `O`
//! (`origin X Y Z`) becomes the orient map through `SetOrientMap(new SmTransform, 2)`
//! and each `T` an `SmTransform` appended with `AddNextMap(map, 2)`. Every output
//! starts with the status (`0` = `SM_SUCCESS`, else the `SmStatus`); the rest:
//!
//! | [`Op`]       | upstream                                          | parameters        | outputs |
//! |--------------|---------------------------------------------------|-------------------|---------|
//! | `Info`       | `GetType`, `IsKindOf`, `IsBoundedSimple`, `IsBounded`, `GetDegree` (all 9 `SmVolumeParamType`s), `GetNaturalParamDomain`, `GetKnots` (all 9, then `U V W` in `SmExtent1d(a, b)`) | `nt T[nt] a b` | `type kind[nt] bs b deg[9] min max knots[12] state` |
//! | `Copy`       | `Copy(ctx, p, simple)`                            | `simple`          | state of the copy |
//! | `Equal`      | `operator==` with itself (`0`), its copy (`1`) or another volume block (`2`) | `mode [block]` | flag |
//! | `Eval`       | `EvaluateSimple`, `Evaluate`, `MapPoint`          | `P n comp`        | three statuses |
//! | `BBox`       | `EvaluateBoundingBoxSimple(SmExtent3d(A, B), NULL, box?, pb?)` | `A B want_box want_pb` | `min max` (box), `basis[3] ivl[3]` (pseudo box) |
//! | `Iso`        | `EvaluateIsoParametricCurveSimple`                | `kind p1 p2`      | curve made |
//! | `InvGuess`   | `InvEvaluateGuessPointSimple`                     | `P`               | `n` points |
//! | `Transform`  | `TransformSimple` / `Transform` of `SmAxis2Placement(O, X, Y)` | `O X Y has_scale S simple` | state |
//! | `Preds`      | `IsPointInParamDomainSimple(P)`, `IsLineInParamDomainSimple(A, B)`, `TrimParamBoundingBoxSimple(SmExtent3d(A, B), trim = SmExtent3d(C, D))` | `P A B C D` | flags, status, `trim` |
//!
//! A knot list is `status n knots[n] n mults[n]`. A state is `n` then per map
//! `type [matrix(16) | record] has_orient [O(16)] has_inv [I(16)]`, a record being the
//! knots `U V W` and the control points `x y z w`.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: finite
//! values, at most [`MAX_CP`] control points and degree [`MAX_DEGREE`] per direction,
//! derivative orders `n <= 3` (the shim's array), `SmVolumeParamsType` codes `0..=8`,
//! and at most [`MAX_NEXT`] next maps.

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

/// Most control points per direction.
pub const MAX_CP: usize = 8;
/// Highest degree per direction.
pub const MAX_DEGREE: usize = 5;
/// Most next maps.
pub const MAX_NEXT: usize = 3;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1e8;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Info,
    Copy,
    Equal,
    Eval,
    BBox,
    Iso,
    InvGuess,
    Transform,
    Preds,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Op::Info => 0,
            Op::Copy => 1,
            Op::Equal => 2,
            Op::Eval => 3,
            Op::BBox => 4,
            Op::Iso => 5,
            Op::InvGuess => 6,
            Op::Transform => 7,
            Op::Preds => 8,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_surface_b_spline_volume_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

crate::reader::unit_reader!(MAX_COORD);

impl Reader<'_> {
    fn num(&mut self) -> Result<f64, FfiError> {
        let v = self
            .input
            .get(self.at)
            .copied()
            .ok_or_else(|| bad("input too short"))?;
        self.at += 1;
        if !v.is_finite() || v.abs() > MAX_COORD {
            return Err(bad("value not finite or too large"));
        }
        Ok(v)
    }

    fn index(&mut self, max: usize) -> Result<usize, FfiError> {
        let v = self.num()?;
        if v < 0.0 || v.fract() != 0.0 || v > max as f64 {
            return Err(bad(format!("index {v} outside 0..={max}")));
        }
        Ok(v as usize)
    }

    fn flag(&mut self) -> Result<bool, FfiError> {
        Ok(self.num()? != 0.0)
    }

    fn vecs(&mut self, n: usize) -> Result<(), FfiError> {
        for _ in 0..3 * n {
            self.num()?;
        }
        Ok(())
    }

    /// A volume block.
    fn volume(&mut self) -> Result<(), FfiError> {
        let mut hi = [0usize; 3];
        for h in &mut hi {
            *h = self.index(MAX_CP - 1)?;
        }
        let mut deg = [0usize; 3];
        for d in &mut deg {
            *d = self.index(MAX_DEGREE)?;
        }
        for (h, d) in hi.iter().zip(deg) {
            // knots U[0..=h+d+1], non-decreasing
            let mut prev = f64::NEG_INFINITY;
            for _ in 0..h + d + 2 {
                let k = self.num()?;
                if k < prev {
                    return Err(bad("knots must be non-decreasing"));
                }
                prev = k;
            }
        }
        let count = (hi[0] + 1) * (hi[1] + 1) * (hi[2] + 1);
        for _ in 0..count {
            for _ in 0..4 {
                self.num()?;
            }
        }
        if self.flag()? {
            self.vecs(4)?;
        }
        let n_next = self.index(MAX_NEXT)?;
        self.vecs(4 * n_next)?;
        Ok(())
    }
}

/// Checks that `input` is a well-formed record for `op` (see the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] naming the first problem.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut r = Reader::new(input);
    r.volume()?;
    match op {
        Op::Info => {
            let nt = r.index(64)?;
            for _ in 0..nt {
                r.num()?;
            }
            r.num()?;
            r.num()?;
        }
        Op::Copy => {
            r.flag()?;
        }
        Op::Equal => {
            if r.index(2)? == 2 {
                r.volume()?;
            }
        }
        Op::Eval => {
            r.vecs(1)?;
            r.index(3)?;
            r.flag()?;
        }
        Op::BBox => {
            r.vecs(2)?;
            r.flag()?;
            r.flag()?;
        }
        Op::Iso => {
            r.index(8)?;
            r.num()?;
            r.num()?;
        }
        Op::InvGuess => r.vecs(1)?,
        Op::Transform => {
            r.vecs(3)?;
            r.flag()?;
            r.vecs(1)?;
            r.flag()?;
        }
        Op::Preds => r.vecs(5)?,
    }
    if r.at != input.len() {
        return Err(bad(format!(
            "input has {} values, op reads {}",
            input.len(),
            r.at
        )));
    }
    Ok(())
}

/// Runs `op` of the shim on `input` (layouts in the module docs) and returns its
/// outputs, starting with the status.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input [`eval`] rejects; the shim's status.
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated above: finite values, record sizes within the
        // limits with knot counts matching the control points and degrees, at most
        // MAX_NEXT next maps, derivative orders within the shim's array); buf is valid
        // for buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_surface_b_spline_volume_eval(
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
