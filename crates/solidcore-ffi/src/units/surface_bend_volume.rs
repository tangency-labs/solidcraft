//! FFI wrappers for unit `surface.bend_volume`.
//!
//! Shim: `harness/cpp/shim/surface_bend_volume.cpp` (`sc_surface_bend_volume_eval`).
//! Upstream: `source/SMLib/src/SmBendVolume.cpp`, `source/SMLib/inc/SmBendVolume.h`.
//!
//! Every op but `ReadDb` starts with a bend block: `kind [C A M d t] has_next [T]`
//! with kind `0` the default `SmBendVolume(&ctx)` and `1` the component constructor
//! `SmBendVolume(ctx, C, A, M, d, t)` (center, axis, mid direction, neutral distance,
//! singularity tolerance); `T` (twelve numbers `O X Y Z`) becomes the next map
//! `SmTransform(ctx, O, X, Y, &Z)` appended with `AddNextMap(t, 2)`. Every output
//! starts with the status (`0` = `SM_SUCCESS`, else the `SmStatus`); the rest:
//!
//! | [`Op`]       | upstream                                          | parameters        | outputs |
//! |--------------|---------------------------------------------------|-------------------|---------|
//! | `Info`       | `GetType*`, `GetClassString`, `IsKindOf`, `GetBend*`, `GetNaturalParamDomain`, `IsBoundedSimple`, `GetLegalMinU` | `nt T[nt]` | state, `type str -1 str -1 kind[nt] C A M B domain(6) bounded min_u` |
//! | `Eval`       | `Evaluate(P, n, TRUE x3, buf, TRUE, TRUE, comp)`  | `P n comp`        | `(n+1)^3` vectors (on success) |
//! | `EvalSimple` | `EvaluateSimple(P, n, TRUE x3, buf)`              | `P n`             | `(n+1)^3` vectors (on success) |
//! | `BBox`       | `EvaluateBoundingBoxSimple(SmExtent3d(A, B), NULL, box?, pseudo?)` of a fresh box / pseudo box | `A B want_box want_pseudo` | `box(6)` if wanted, `basis(9) ivls(6)` if wanted |
//! | `Trim`       | `TrimParamBoundingBoxSimple(SmExtent3d(A, B), trim)` of a fresh box | `A B` | `box(6)` |
//! | `InvGuess`   | `InvEvaluateGuessPointSimple`                     | `P`               | `n` points |
//! | `Global`     | `GlobalPointSolveSimple` of a fresh array         | `P has_dom [A B]` | `found n` solutions (on success) |
//! | `Local`      | `LocalPointSolveSimple` of a fresh `SmSolution`   | `P G has_dom [A B]` | `found` solution (on success) |
//! | `Preds`      | `IsPointInParamDomainSimple(P)`, `IsLineInParamDomainSimple(A, B)`, `IsBoundedSimple` | `P A B` | three flags |
//! | `CurveIn`    | `IsCurveInParamDomainSimple(SmLine(A, B, 3))`     | `A B`             | flag |
//! | `Copy`       | `Copy(ctx, p, simple)`                            | `simple`          | state of the copy |
//! | `Equal`      | `operator==`                                      | bend block, `self` | flag |
//! | `ReadDb`     | `SmBendVolume::ReadFromDB` of an ASCII stream (version 30) | `n bytes[n]` (no block) | state (on a new volume) |
//!
//! A state is `type neutral tol has_orient [O(16)] has_inv [I(16)] has_next [type]`;
//! a solution `type nvars value p0 p1 p2 nobjects is_this`; domains `A B` are the raw
//! corners of `SmExtent3d(A.x, A.y, A.z, B.x, B.y, B.z)`.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: finite
//! values within [`MAX_COORD`], derivative orders that fit the shim's arrays (`n <= 5`;
//! upstream rejects `n > 3` before writing), and for `ReadDb` byte values forming a text that
//! `surface_volume_p2::check_text` accepts (the base read has null and
//! uninitialised cases).

use crate::units::surface_volume_p2;
use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

/// Largest derivative order the shim's evaluation arrays hold.
pub const MAX_N: usize = 5;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1e8;
/// Longest database stream.
pub const MAX_BYTES: usize = 4096;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Info,
    Eval,
    EvalSimple,
    BBox,
    Trim,
    InvGuess,
    Global,
    Local,
    Preds,
    CurveIn,
    Copy,
    Equal,
    ReadDb,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Op::Info => 0,
            Op::Eval => 1,
            Op::EvalSimple => 2,
            Op::BBox => 3,
            Op::Trim => 4,
            Op::InvGuess => 5,
            Op::Global => 6,
            Op::Local => 7,
            Op::Preds => 8,
            Op::CurveIn => 9,
            Op::Copy => 10,
            Op::Equal => 11,
            Op::ReadDb => 12,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_surface_bend_volume_eval(
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

    fn vecs(&mut self, n: usize) -> Result<Vec<f64>, FfiError> {
        (0..3 * n).map(|_| self.num()).collect()
    }

    fn bend(&mut self) -> Result<(), FfiError> {
        if self.index(1)? == 1 {
            self.vecs(3)?;
            self.num()?;
            self.num()?;
        }
        if self.flag()? {
            self.vecs(4)?;
        }
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
    if op != Op::ReadDb {
        r.bend()?;
    }
    match op {
        Op::Info => {
            let nt = r.index(64)?;
            for _ in 0..nt {
                r.index(1 << 30)?;
            }
        }
        Op::Eval => {
            r.vecs(1)?;
            r.index(MAX_N)?;
            r.flag()?;
        }
        Op::EvalSimple => {
            r.vecs(1)?;
            r.index(MAX_N)?;
        }
        Op::BBox => {
            r.vecs(2)?;
            r.flag()?;
            r.flag()?;
        }
        Op::Trim | Op::CurveIn => {
            r.vecs(2)?;
        }
        Op::InvGuess => {
            r.vecs(1)?;
        }
        Op::Global | Op::Local => {
            r.vecs(if op == Op::Local { 2 } else { 1 })?;
            if r.flag()? {
                r.vecs(2)?;
            }
        }
        Op::Preds => {
            r.vecs(3)?;
        }
        Op::Copy => {
            r.flag()?;
        }
        Op::Equal => {
            r.bend()?;
            r.flag()?;
        }
        Op::ReadDb => {
            let n = r.index(MAX_BYTES)?;
            let mut data = Vec::new();
            for _ in 0..n {
                data.push(r.index(255)? as u8);
            }
            // the base read that follows (`SmVolume::ReadFromDB`) has null and
            // uninitialised cases: the strict text grammar of `surface.volume_p2`
            surface_volume_p2::check_text(surface_volume_p2::BEND_VOLUME_TYPE, &data)?;
        }
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
    call_with_buffer(1024, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated above: finite values, a bend block of a known
        // kind, derivative orders within the shim's arrays, byte values for
        // streams); buf is valid for buf.len() writes and the shim
        // honours the capacity; len is valid.
        unsafe {
            sys::sc_surface_bend_volume_eval(
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
