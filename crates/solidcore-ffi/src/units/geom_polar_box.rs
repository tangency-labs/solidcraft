//! FFI wrappers for unit `geom.polar_box`.
//!
//! Shim: `harness/cpp/shim/geom_polar_box.cpp`.
//! Upstream: `source/SMLib/src/SmPolarBox.cpp`, `source/SMLib/inc/SmPolarBox.h`
//! (`SmPolarBox`).
//!
//! | Rust    | shim                    | upstream                               |
//! |---------|-------------------------|----------------------------------------|
//! | [`run`] | `sc_geom_polar_box_run` | `SmPolarBox` methods, per [`Cmd`]      |
//!
//! A program is a list of [`Cmd`]s on [`REGISTERS`] box registers (all start as
//! `SmPolarBox()`); [`run`] encodes it, runs it in C++ and returns the concatenated
//! outputs (layouts on each [`Cmd`] variant and in the shim).

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_geom_polar_box_run(
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

/// Upstream `SM_ERR_INVALID_INPUT`, written by [`Cmd::BoundaryVectors`],
/// [`Cmd::Union`] and [`Cmd::UnionSelf`] for their error cases.
pub const SM_ERR_INVALID_INPUT: f64 = 1007.0;

/// One polar-box command. `r`, `s`, `t` are register indices below [`REGISTERS`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cmd {
    /// `reg[r].ReSet()`.
    Reset {
        /// Register.
        r: usize,
    },
    /// `reg[r] = SmPolarBox(v)`.
    FromVector {
        /// Register.
        r: usize,
        /// Initial vector.
        v: [f64; 3],
    },
    /// `reg[r].AddVector3d(v, b2.as_ref())`.
    Add {
        /// Register.
        r: usize,
        /// Vector to add.
        v: [f64; 3],
        /// Optional `pOptBasis2`.
        b2: Option<[f64; 3]>,
    },
    /// Output: state, basis 0..2 (9 values), domain `[min.x, min.y, max.x, max.y]`.
    Dump {
        /// Register.
        r: usize,
    },
    /// `ComputePolarCoord(v)`; output 2 values.
    PolarCoord {
        /// Register.
        r: usize,
        /// Vector.
        v: [f64; 3],
    },
    /// `GetPolarArea()`; output 1 value.
    Area {
        /// Register.
        r: usize,
    },
    /// `EvaluateNormalized(a, b)`; output 3 values.
    Evaluate {
        /// Register.
        r: usize,
        /// Normalised first angle.
        a: f64,
        /// Normalised second angle.
        b: f64,
    },
    /// `GetBoundaryVectors`; output status, then (if 0) count and vectors.
    BoundaryVectors {
        /// Register.
        r: usize,
    },
    /// `IsDisjointWithCone(v, ang)`; output flag.
    DisjointCone {
        /// Register.
        r: usize,
        /// Cone axis.
        v: [f64; 3],
        /// Cone half-angle (radians).
        ang: f64,
    },
    /// `IsPerpendicularDisjointWithCone(v, ang)`; output flag.
    PerpDisjointCone {
        /// Register.
        r: usize,
        /// Cone reference vector.
        v: [f64; 3],
        /// Cone half-angle (radians).
        ang: f64,
    },
    /// `reg[r].AreDisjoint(reg[s])`; output flag.
    AreDisjoint {
        /// Register.
        r: usize,
        /// Other register.
        s: usize,
    },
    /// `reg[r].Cross(reg[s], x); reg[t] = x` (fresh `x`); output status.
    Cross {
        /// Register.
        r: usize,
        /// Other register.
        s: usize,
        /// Result register.
        t: usize,
    },
    /// `reg[r].Union(reg[s], x); reg[t] = x` (fresh `x`); output status
    /// ([`SM_ERR_INVALID_INPUT`] for upstream's undefined-behaviour case, skipped).
    Union {
        /// Register.
        r: usize,
        /// Other register.
        s: usize,
        /// Result register.
        t: usize,
    },
    /// `reg[r].Union(reg[s], reg[r])` with `r != s`; output status (as [`Cmd::Union`]).
    UnionSelf {
        /// Register (also the result).
        r: usize,
        /// Other register.
        s: usize,
    },
    /// `HasPerpendicularToVector(v)`; output flag.
    HasPerpendicular {
        /// Register.
        r: usize,
        /// Test vector.
        v: [f64; 3],
    },
    /// `ExpandAbsolute(d)`.
    Expand {
        /// Register.
        r: usize,
        /// Expansion angle (radians).
        d: f64,
    },
    /// `reg[t] = reg[r]` (`operator=`).
    Assign {
        /// Destination register.
        t: usize,
        /// Source register.
        r: usize,
    },
}

impl Cmd {
    fn registers(&self) -> Vec<usize> {
        match *self {
            Cmd::Reset { r }
            | Cmd::FromVector { r, .. }
            | Cmd::Add { r, .. }
            | Cmd::Dump { r }
            | Cmd::PolarCoord { r, .. }
            | Cmd::Area { r }
            | Cmd::Evaluate { r, .. }
            | Cmd::BoundaryVectors { r }
            | Cmd::DisjointCone { r, .. }
            | Cmd::PerpDisjointCone { r, .. }
            | Cmd::HasPerpendicular { r, .. }
            | Cmd::Expand { r, .. } => vec![r],
            Cmd::AreDisjoint { r, s } | Cmd::UnionSelf { r, s } => vec![r, s],
            Cmd::Assign { t, r } => vec![t, r],
            Cmd::Cross { r, s, t } | Cmd::Union { r, s, t } => vec![r, s, t],
        }
    }

    /// Appends the shim encoding (op code, then arguments).
    fn encode(&self, o: &mut Vec<f64>) {
        let reg = |i: usize| i as f64;
        match *self {
            Cmd::Reset { r } => o.extend([0.0, reg(r)]),
            Cmd::FromVector { r, v } => o.extend([1.0, reg(r), v[0], v[1], v[2]]),
            Cmd::Add { r, v, b2: None } => o.extend([2.0, reg(r), v[0], v[1], v[2]]),
            Cmd::Add { r, v, b2: Some(b) } => {
                o.extend([3.0, reg(r), v[0], v[1], v[2], b[0], b[1], b[2]]);
            }
            Cmd::Dump { r } => o.extend([4.0, reg(r)]),
            Cmd::PolarCoord { r, v } => o.extend([5.0, reg(r), v[0], v[1], v[2]]),
            Cmd::Area { r } => o.extend([6.0, reg(r)]),
            Cmd::Evaluate { r, a, b } => o.extend([7.0, reg(r), a, b]),
            Cmd::BoundaryVectors { r } => o.extend([8.0, reg(r)]),
            Cmd::DisjointCone { r, v, ang } => o.extend([9.0, reg(r), v[0], v[1], v[2], ang]),
            Cmd::PerpDisjointCone { r, v, ang } => {
                o.extend([10.0, reg(r), v[0], v[1], v[2], ang]);
            }
            Cmd::AreDisjoint { r, s } => o.extend([11.0, reg(r), reg(s)]),
            Cmd::Cross { r, s, t } => o.extend([12.0, reg(r), reg(s), reg(t)]),
            Cmd::Union { r, s, t } => o.extend([13.0, reg(r), reg(s), reg(t)]),
            Cmd::UnionSelf { r, s } => o.extend([14.0, reg(r), reg(s)]),
            Cmd::HasPerpendicular { r, v } => o.extend([15.0, reg(r), v[0], v[1], v[2]]),
            Cmd::Expand { r, d } => o.extend([16.0, reg(r), d]),
            Cmd::Assign { t, r } => o.extend([17.0, reg(t), reg(r)]),
        }
    }
}

/// Runs `prog` in upstream C++ and returns the concatenated outputs.
pub fn run(prog: &[Cmd]) -> Result<Vec<f64>, FfiError> {
    let mut flat = Vec::new();
    for (i, c) in prog.iter().enumerate() {
        if c.registers().iter().any(|&r| r >= REGISTERS) {
            return Err(FfiError::InvalidArgument(format!(
                "command {i}: register out of range"
            )));
        }
        if let Cmd::UnionSelf { r, s } = *c {
            if r == s {
                return Err(FfiError::InvalidArgument(format!(
                    "command {i}: union_self needs two registers"
                )));
            }
        }
        c.encode(&mut flat);
    }
    let n = i64::try_from(flat.len()).map_err(|_| FfiError::InvalidArgument("prog".into()))?;
    call_with_buffer(256, |buf: &mut [f64], len| {
        // SAFETY: `flat` is valid for `n` reads and is a well-formed program (checked
        // above and again in the shim); `buf` is valid for `buf.len()` writes; `len`
        // is a valid writable i64.
        unsafe {
            sys::sc_geom_polar_box_run(
                flat.as_ptr(),
                n,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
