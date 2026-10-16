//! FFI wrappers for unit `geom.pseudo_box`.
//!
//! Shim: `harness/cpp/shim/geom_pseudo_box.cpp`.
//! Upstream: `source/SMLib/src/SmPseudoBox.cpp`, `source/SMLib/inc/SmPseudoBox.h`
//! (`SmPseudoBox`), plus `SmExtent3d(const SmPseudoBox&)` and
//! `SmTol::GetScaledZero(const SmPseudoBox&)`.
//!
//! | Rust    | shim                     | upstream                          |
//! |---------|--------------------------|-----------------------------------|
//! | [`run`] | `sc_geom_pseudo_box_run` | `SmPseudoBox` methods, per [`Op`] |
//!
//! A program is a list of [`Cmd`]s on [`REGISTERS`] box registers (all start as
//! `SmPseudoBox()`); [`run`] validates and encodes it, runs it in C++ and returns the
//! concatenated outputs (layouts in the shim's op table).

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_geom_pseudo_box_run(
            prog: *const f64,
            n_prog: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Number of box registers.
pub const REGISTERS: usize = 4;

/// Shim op codes (the table in `harness/cpp/shim/geom_pseudo_box.cpp`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Op {
    /// `Init()`.
    Init = 0,
    /// `InitBasis()`.
    InitBasis = 1,
    /// `reg = SmPseudoBox(SmExtent3d(6))`.
    FromExtent = 2,
    /// `reg = SmExtent3d(6)` (`operator=`).
    AssignExtent = 3,
    /// `SetBasis(b0, b1, b2)`; output status.
    SetBasis = 4,
    /// `SetIntervals(SmExtent1d(a, b) x3)`.
    SetIntervals = 5,
    /// `SetInterval(idx, SmExtent1d(a, b))`; floats `idx, a, b`.
    SetInterval = 6,
    /// `SetUnbounded()`.
    SetUnbounded = 7,
    /// `AddPoint3d(p)`.
    AddPoint = 8,
    /// `ExpandAbsolute(d)`.
    Expand = 9,
    /// Basis (9) and intervals (6).
    Dump = 10,
    /// `ContainsPoint3d(p, tol)`; output flag.
    Contains = 11,
    /// `Evaluate(x, y, z)`; output 3.
    Evaluate = 12,
    /// `InvertAbsolute(p)`; output status [, 3].
    InvertAbsolute = 13,
    /// `reg[r].Union(reg[s], basis, reg[t])`.
    Union = 14,
    /// `DistanceToPoint(p)`; output 1.
    Distance = 15,
    /// `IntersectLine(p, v)`; output status [, n, t0, t1].
    IntersectLine = 16,
    /// `HasNegativeVolume()`; output flag.
    NegativeVolume = 17,
    /// `GetMaxDimension`, `GetXYArea`, `GetVolume`; output 3.
    Measures = 18,
    /// `IsPointSized(tol)`; output flag, centre 3, deviation.
    PointSized = 19,
    /// `IsLinear(tol)`; output flag, point 3, tangent 3, deviation.
    Linear = 20,
    /// `IsPlanar(tol)`; output flag, point 3, normal 3, deviation.
    Planar = 21,
    /// `AreDisjoint(reg[s], has ? v : NULL)`; floats `has, v`; output flag.
    AreDisjoint = 22,
    /// `AreProjectDisjoint(reg[s], type, pt, vec, aux)`; output flag.
    ProjectDisjoint = 23,
    /// `IsContainedBy(reg[s], tol)`; output flag.
    ContainedBy = 24,
    /// `ClassifyPoint3d(p, tol)`; output 3 enum values.
    Classify = 25,
    /// `CalcPlanes`; output 6 points then 6 vectors.
    Planes = 26,
    /// `IsBounded(types)`; output flag, 3 enum values.
    Bounded = 27,
    /// `IsAxisAligned(map)`; output flag [, 3].
    AxisAligned = 28,
    /// `AreAligned(reg[s], map)`; output flag [, 3].
    AreAligned = 29,
    /// `GetCorners`; output 24.
    Corners = 30,
    /// `GetEdges`; output 72.
    Edges = 31,
    /// `AssertDefined()`; output flag.
    AssertDefined = 32,
    /// `reg[r].ProjectToPlane(reg[t], type, pt, vec, aux)`; output status.
    Project = 33,
    /// `reg[t] = reg[r]` (registers `t, r`).
    Assign = 34,
    /// `reg = SmPseudoBox(b0, b1, b2, SmExtent1d(a, b) x3)`.
    FromBasisIntervals = 35,
    /// `SmExtent3d(reg)` (`Circumscribe`); output min 3, max 3.
    Circumscribe = 36,
    /// `SmTol::GetScaledZero(reg)`; output 1.
    ScaledZero = 37,
}

impl Op {
    /// Register operands and float arguments of the op.
    #[must_use]
    pub const fn arity(self) -> (usize, usize) {
        match self {
            Op::Init
            | Op::InitBasis
            | Op::SetUnbounded
            | Op::Dump
            | Op::NegativeVolume
            | Op::Measures
            | Op::Planes
            | Op::Bounded
            | Op::AxisAligned
            | Op::Corners
            | Op::Edges
            | Op::AssertDefined
            | Op::Circumscribe
            | Op::ScaledZero => (1, 0),
            Op::FromExtent | Op::AssignExtent | Op::SetIntervals | Op::IntersectLine => (1, 6),
            Op::SetBasis => (1, 9),
            Op::SetInterval | Op::AddPoint | Op::Evaluate | Op::InvertAbsolute | Op::Distance => {
                (1, 3)
            }
            Op::Expand | Op::PointSized | Op::Linear | Op::Planar => (1, 1),
            Op::Contains | Op::Classify => (1, 4),
            Op::Union => (3, 9),
            Op::AreDisjoint => (2, 4),
            Op::ProjectDisjoint | Op::Project => (2, 10),
            Op::ContainedBy => (2, 1),
            Op::AreAligned | Op::Assign => (2, 0),
            Op::FromBasisIntervals => (1, 15),
        }
    }
}

/// One command: an op, its register operands and its float arguments.
#[derive(Debug, Clone, PartialEq)]
pub struct Cmd {
    /// The op.
    pub op: Op,
    /// Register operands (each below [`REGISTERS`]).
    pub regs: Vec<usize>,
    /// Float arguments.
    pub args: Vec<f64>,
}

fn validate(i: usize, c: &Cmd) -> Result<(), FfiError> {
    let bad = |m: &str| {
        Err(FfiError::InvalidArgument(format!(
            "command {i} ({:?}): {m}",
            c.op
        )))
    };
    let (n_regs, n_args) = c.op.arity();
    if c.regs.len() != n_regs || c.args.len() != n_args {
        return bad("wrong operand count");
    }
    if c.regs.iter().any(|&r| r >= REGISTERS) {
        return bad("register out of range");
    }
    let first = c.args.first().copied().unwrap_or(0.0);
    match c.op {
        Op::SetInterval if ![0.0, 1.0, 2.0].contains(&first) => bad("interval index"),
        Op::ProjectDisjoint | Op::Project if ![0.0, 1.0, 2.0].contains(&first) => {
            bad("projection type")
        }
        Op::AreDisjoint if ![0.0, 1.0].contains(&first) => bad("has-projection flag"),
        _ => Ok(()),
    }
}

/// Runs `prog` in upstream C++ and returns the concatenated outputs.
pub fn run(prog: &[Cmd]) -> Result<Vec<f64>, FfiError> {
    let mut flat = Vec::new();
    for (i, c) in prog.iter().enumerate() {
        validate(i, c)?;
        flat.push(f64::from(c.op as u8));
        flat.extend(c.regs.iter().map(|&r| r as f64));
        flat.extend(&c.args);
    }
    let n = i64::try_from(flat.len()).map_err(|_| FfiError::InvalidArgument("prog".into()))?;
    call_with_buffer(256, |buf: &mut [f64], len| {
        // SAFETY: `flat` is valid for `n` reads and is a well-formed program (checked
        // above and again in the shim); `buf` is valid for `buf.len()` writes; `len`
        // is a valid writable i64.
        unsafe {
            sys::sc_geom_pseudo_box_run(
                flat.as_ptr(),
                n,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
