//! FFI wrappers for unit `geom.grid`.
//!
//! Shim: `harness/cpp/shim/geom_grid.cpp`.
//! Upstream: `source/SMLib/src/SmGrid.cpp` (`SmGrid`, `SmVoxel`, `SmGridElement`,
//! `SmHierarchyElement`).
//!
//! | Rust      | shim                                       |
//! |-----------|--------------------------------------------|
//! | [`run`]   | `sc_geom_grid_ints`, `sc_geom_grid_floats` |
//!
//! Runs a small program against one upstream grid; the encoding is documented
//! in the shim and validated here before any call.

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_geom_grid_ints(
            prog: *const f64,
            n: i64,
            out: *mut i64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_geom_grid_floats(
            prog: *const f64,
            n: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Largest object id + 1 (size of the shim's fake object array).
pub const MAX_OBJECTS: i64 = 4096;

/// Largest voxel count per axis accepted.
pub const MAX_AXIS: i64 = 16;

/// Largest absolute coordinate accepted (keeps every box size and voxel
/// address computation finite).
pub const MAX_COORD: f64 = 1e12;

/// Number of operands of op `op`, or `None` for an unknown op.
#[must_use]
pub fn op_size(op: i64) -> Option<usize> {
    Some(match op {
        0 | 2 => 9,
        1 => 8,
        3 | 6 | 10 => 6,
        4 | 5 => 3,
        7 | 9 => 1,
        8 => 0,
        _ => return None,
    })
}

fn int_in(v: f64, lo: i64, hi: i64) -> bool {
    v.fract() == 0.0 && v >= lo as f64 && v <= hi as f64
}

fn coords_ok(a: &[f64]) -> bool {
    a.iter().all(|v| v.is_finite() && v.abs() <= MAX_COORD)
}

fn validate(prog: &[f64]) -> Result<(), FfiError> {
    let ctor = prog.get(0..10).ok_or_else(|| bad("missing constructor"))?;
    if ctor.first() != Some(&0.0) {
        return Err(bad("constructor must be first"));
    }
    let bbox = ctor.get(1..7).ok_or_else(|| bad("constructor"))?;
    if !coords_ok(bbox) {
        return Err(bad("grid box"));
    }
    for k in 0..3 {
        let (lo, hi) = (bbox.get(k).copied(), bbox.get(k + 3).copied());
        match (lo, hi) {
            // A grid with a zero-size (or inverted) axis makes GetVoxelAddress
            // convert NaN or a negative value to an index: undefined behaviour.
            (Some(l), Some(h)) if h - l > 0.0 && (h - l) * 1e-12 > 0.0 => {}
            _ => return Err(bad("grid box must have positive size")),
        }
    }
    for k in 7..10 {
        if !int_in(ctor.get(k).copied().unwrap_or(0.0), 1, MAX_AXIS) {
            return Err(bad("voxel counts"));
        }
    }
    let mut hier = 0i64;
    let mut i = 10usize;
    while i < prog.len() {
        let op = prog
            .get(i)
            .copied()
            .ok_or_else(|| bad("truncated program"))?;
        if !int_in(op, 1, 10) {
            return Err(bad("op"));
        }
        let op = op as i64;
        let sz = op_size(op).ok_or_else(|| bad("op"))?;
        let a = prog
            .get(i + 1..i + 1 + sz)
            .ok_or_else(|| bad("truncated program"))?;
        let at = |j: usize| a.get(j).copied().unwrap_or(f64::NAN);
        let ok = match op {
            1 => {
                (at(0) == 1.0 || at(0) == 3.0)
                    && int_in(at(1), 0, MAX_OBJECTS - 1)
                    && coords_ok(a.get(2..8).unwrap_or(&[f64::NAN]))
            }
            2 => {
                hier += 1;
                (at(0) == 0.0 || at(0) == 1.0)
                    && int_in(at(1), 0, MAX_OBJECTS - 1)
                    && coords_ok(a.get(2..9).unwrap_or(&[f64::NAN]))
            }
            3 | 4 | 6 | 10 => coords_ok(a),
            5 => a.iter().all(|&v| int_in(v, -1000, 1000)),
            7 => !at(0).is_nan(),
            9 => int_in(at(0), 0, hier - 1),
            _ => true,
        };
        if !ok {
            return Err(bad("operands"));
        }
        i += 1 + sz;
    }
    Ok(())
}

/// Runs a grid program; returns its integer and float output streams.
pub fn run(prog: &[f64]) -> Result<(Vec<i64>, Vec<f64>), FfiError> {
    validate(prog)?;
    let n = prog.len() as i64;
    let ints = call_with_buffer(256, |buf: &mut [i64], len| {
        // SAFETY: prog valid for n reads; buf valid for buf.len() writes; len valid.
        unsafe {
            sys::sc_geom_grid_ints(
                prog.as_ptr(),
                n,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })?;
    let floats = call_with_buffer(256, |buf: &mut [f64], len| {
        // SAFETY: as above.
        unsafe {
            sys::sc_geom_grid_floats(
                prog.as_ptr(),
                n,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })?;
    Ok((ints, floats))
}
