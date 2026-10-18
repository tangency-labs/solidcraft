//! FFI wrappers for unit `surface.volume_p1`.
//!
//! Shim: `harness/cpp/shim/surface_volume_p1.cpp` (`sc_surface_volume_p1_eval`).
//! Upstream: `source/SMLib/src/SmVolume.cpp`, `source/SMLib/inc/SmVolume.h`.
//!
//! Volume ops start with a volume block: `n` maps (`1..=MAX_MAPS`), each
//! `kind [T] has_orient [O]` with kind `0` a plain `SmVolume`, `1` an `SmTransform`
//! built `SmTransform(ctx, origin, X, Y, &Z)` from the twelve numbers `T`, `2` a
//! default `SmTwistVolume`, `3` a default `SmUnbendVolume`; `O` (twelve numbers, same
//! form) becomes the orient map through `SetOrientMap(new SmTransform, 2)`. Map `i > 0`
//! is appended with `AddNextMap(map, 2)`. Static ops have no block. Every output starts
//! with the status (`0` = `SM_SUCCESS`, else the `SmStatus`); the rest:
//!
//! | [`Op`]           | upstream                                    | parameters          | outputs |
//! |------------------|---------------------------------------------|---------------------|---------|
//! | `Info`           | `GetType`, `GetTypeString`, `GetClassString`, `IsKindOf`, `GetLastMap`, `GetOrient*` | `nt T[nt]` | `type str -1 str -1 kind[nt] last O X Y Z` state |
//! | `Bounded`        | `IsBoundedSimple`, `IsBounded`, `GetNaturalParamDomain` |          | `simple bounded min max` |
//! | `Eval`, `Map`    | `Evaluate`, `Map`                           | `P n l0 l1 l2 nz comp` | `(n+1)^3` vectors (on success) |
//! | `MapPoint`       | `MapPoint`                                  | `P comp`            | point |
//! | `Orient`         | `Orient(P, n, TRUE x3, buf, TRUE, TRUE)`    | `P n`               | `(n+1)^3` vectors |
//! | `OrientPoint`    | `OrientPoint`                               | `P`                 | point |
//! | `OrientDir`      | `OrientDirectionalDerivs`                   | `P D n`             | `n+1` vectors |
//! | `OrientVector`   | `OrientVector`                              | `P D`               | vector |
//! | `EvalSimple`     | `EvaluateSimple(P, n, TRUE x3, buf)`        | `P n`               | `(n+1)^3` vectors |
//! | `EvalPointSimple`| `EvaluatePointSimple`                       | `P`                 | point |
//! | `Eval1stSimple`  | `Evaluate1stDerivativesSimple`              | `P l0 l1 l2 nz`     | `P Du Dv Dw` |
//! | `InvGuess`       | `InvEvaluateGuessPointSimple`               | `P`                 | `n` points |
//! | `SimplePreds`    | `IsPointInParamDomainSimple`, `IsLineInParamDomainSimple`, `IsCurveInParamDomainSimple(SmLine(A, B))` | `P A B` | three flags |
//! | `PtInInSpace`    | `IsPointInInSpaceDomain`                    | `P comp`            | flag, parameter point |
//! | `LineInParam`    | `IsLineInParamDomain`                       | `A B comp`          | flag |
//! | `CurveInParam`, `CurveInInSpace` | `IsCurveInParamDomain`, `IsCurveInInSpaceDomain` of `SmLine(A, B, 3)` | `A B comp` | flag |
//! | `Transform`, `TransformSimple` | `Transform`, `TransformSimple` of `SmAxis2Placement(O, X, Y)` | `O X Y has_scale S` | state |
//! | `AddNext`        | `AddNextMap(other, copy ? 1 : 2)`           | volume block, `copy`| state |
//! | `Copy`           | `Copy(ctx, p, simple)`                      | `simple`            | state of the copy (on success) |
//! | `Equal`          | `operator==`                                | volume block, `self`| flag |
//! | `MakeSimple`     | `MakeMapSimple`                             |                     | state |
//! | `Refresh`        | `RefreshInvOrientMap`                       |                     | state |
//! | `TrimBox`        | `TrimParamBoundingBoxSimple(SmExtent3d(A, B), trim = SmExtent3d(C, D))` | `A B C D` | `trim` |
//! | `HasDisc`        | `HasDiscontinuitiesSimple` of a `SetUnaligned` set | | flag, discontinuities |
//! | `PvEval`         | `SmFindPVIntersectENFO(P, *v, sq).Evaluate` | `P sq X jac`        | `found F[3] [J[9]]` (on success) |
//! | `Lift` (static)  | `smvol_Lift*Derivative`                     | `form order` inputs | lifted vectors |
//! | `Concat` (static)| `smvol_ConcatEvals`                         | `n A B`             | `(n+1)^3` vectors |
//! | `Disconts` (static) | `SmDiscontinuities3d()`, `SetUnaligned`, `Init` |               | three discontinuity sets |
//!
//! A state is `n` then per map `type [matrix(16) if a transform] has_orient [O(16)]
//! has_inv [I(16)]`; a discontinuity set `has min` and per direction `unaligned min
//! n_params n_conts`. The lift forms are `0` curve (`C[order]`), `1` surface
//! (`S1[2] [S2[3]] [S3[4]]`), `2` volume (`F1[3] [F2[6]] [F3[10]]`), followed by the
//! compounding volume's `D1[3] [D2[6]] [D3[10]]` up to `order`.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: finite
//! values, derivative orders that fit the shim's arrays (`n <= 5`, `Concat` `n <= 3`),
//! `Refresh` only on a volume with an orient map (upstream calls `MakeMapSimple`
//! through a null pointer otherwise), and no line or curve domain check on a chain
//! with a (default-constructed) unbend volume, which crashes upstream.

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

/// Most maps in a volume chain.
pub const MAX_MAPS: usize = 8;
/// Largest derivative order the shim's evaluation arrays hold.
pub const MAX_N: usize = 5;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1e8;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Info,
    Bounded,
    Eval,
    Map,
    MapPoint,
    Orient,
    OrientPoint,
    OrientDir,
    OrientVector,
    EvalSimple,
    EvalPointSimple,
    Eval1stSimple,
    InvGuess,
    SimplePreds,
    PtInInSpace,
    LineInParam,
    CurveInParam,
    CurveInInSpace,
    Transform,
    AddNext,
    Copy,
    Equal,
    MakeSimple,
    Refresh,
    TrimBox,
    HasDisc,
    PvEval,
    TransformSimple,
    Lift,
    Concat,
    Disconts,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Op::Info => 0,
            Op::Bounded => 1,
            Op::Eval => 2,
            Op::Map => 3,
            Op::MapPoint => 4,
            Op::Orient => 5,
            Op::OrientPoint => 6,
            Op::OrientDir => 7,
            Op::OrientVector => 8,
            Op::EvalSimple => 9,
            Op::EvalPointSimple => 10,
            Op::Eval1stSimple => 11,
            Op::InvGuess => 12,
            Op::SimplePreds => 13,
            Op::PtInInSpace => 14,
            Op::LineInParam => 15,
            Op::CurveInParam => 16,
            Op::CurveInInSpace => 17,
            Op::Transform => 18,
            Op::AddNext => 19,
            Op::Copy => 20,
            Op::Equal => 21,
            Op::MakeSimple => 22,
            Op::Refresh => 23,
            Op::TrimBox => 24,
            Op::HasDisc => 25,
            Op::PvEval => 26,
            Op::TransformSimple => 27,
            Op::Lift => 40,
            Op::Concat => 41,
            Op::Disconts => 42,
        }
    }

    /// Whether the op takes no volume block.
    #[must_use]
    pub fn is_static(self) -> bool {
        self.code() >= 40
    }
}

mod sys {
    extern "C" {
        pub fn sc_surface_volume_p1_eval(
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

    /// A volume block; returns whether its head map has an orient map and whether
    /// any map is an unbend volume.
    fn volume(&mut self) -> Result<(bool, bool), FfiError> {
        let n = self.index(MAX_MAPS)?;
        if n == 0 {
            return Err(bad("volume without maps"));
        }
        let mut head_oriented = false;
        let mut unbend = false;
        for i in 0..n {
            match self.index(3)? {
                1 => self.vecs(4)?,
                3 => unbend = true,
                _ => {}
            }
            let oriented = self.flag()?;
            if oriented {
                self.vecs(4)?;
            }
            if i == 0 {
                head_oriented = oriented;
            }
        }
        Ok((head_oriented, unbend))
    }
}

/// Checks that `input` is a well-formed record for `op` (see the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] naming the first problem.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut r = Reader::new(input);
    let (oriented, unbend) = if op.is_static() {
        (false, false)
    } else {
        r.volume()?
    };
    match op {
        Op::Info => {
            let nt = r.index(64)?;
            for _ in 0..nt {
                r.num()?;
            }
        }
        Op::Bounded | Op::MakeSimple | Op::HasDisc | Op::Disconts => {}
        Op::Refresh => {
            if !oriented {
                return Err(bad("RefreshInvOrientMap needs an orient map"));
            }
        }
        Op::Eval | Op::Map => {
            r.vecs(1)?;
            r.index(MAX_N)?;
            for _ in 0..5 {
                r.flag()?;
            }
        }
        Op::MapPoint | Op::PtInInSpace => {
            r.vecs(1)?;
            r.flag()?;
        }
        Op::Orient | Op::EvalSimple => {
            r.vecs(1)?;
            r.index(MAX_N)?;
        }
        Op::OrientPoint | Op::EvalPointSimple | Op::InvGuess => r.vecs(1)?,
        Op::OrientDir => {
            r.vecs(2)?;
            r.index(MAX_N)?;
        }
        Op::OrientVector => r.vecs(2)?,
        Op::Eval1stSimple => {
            r.vecs(1)?;
            for _ in 0..4 {
                r.flag()?;
            }
        }
        Op::SimplePreds => r.vecs(3)?,
        Op::LineInParam | Op::CurveInParam | Op::CurveInInSpace => {
            if unbend {
                return Err(bad(
                    "curve domain checks of a default SmUnbendVolume crash upstream",
                ));
            }
            r.vecs(2)?;
            r.flag()?;
        }
        Op::Transform | Op::TransformSimple => {
            r.vecs(3)?;
            r.flag()?;
            r.vecs(1)?;
        }
        Op::AddNext | Op::Equal => {
            r.volume()?;
            r.flag()?;
        }
        Op::Copy => {
            r.flag()?;
        }
        Op::TrimBox => r.vecs(4)?,
        Op::PvEval => {
            r.vecs(1)?;
            r.num()?;
            r.vecs(1)?;
            r.flag()?;
        }
        Op::Lift => {
            let form = r.index(2)?;
            let order = r.index(3)?;
            if order == 0 {
                return Err(bad("lift order 0"));
            }
            let first: [usize; 3] = match form {
                0 => [1, 1, 1],
                1 => [2, 3, 4],
                _ => [3, 6, 10],
            };
            let n_first: usize = if form == 0 {
                order
            } else {
                first.iter().take(order).sum()
            };
            let n_second: usize = [3, 6, 10].iter().take(order).sum();
            r.vecs(n_first + n_second)?;
        }
        Op::Concat => {
            let n = r.index(3)?;
            let size = (n + 1) * (n + 1) * (n + 1);
            r.vecs(2 * size)?;
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
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated above: finite values, volume blocks of 1..=8 maps
        // with known kinds, derivative orders within the shim's arrays, an orient map
        // for `Refresh`); buf is valid for buf.len() writes and the shim honours the
        // capacity; len is valid.
        unsafe {
            sys::sc_surface_volume_p1_eval(
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
