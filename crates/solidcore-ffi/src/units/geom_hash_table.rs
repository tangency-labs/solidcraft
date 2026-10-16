//! FFI wrappers for unit `geom.hash_table`.
//!
//! Shim: `harness/cpp/shim/geom_hash_table.cpp`.
//! Upstream: `source/SMLib/inc/SmHashTable.h` (`SmObjsInVoxels`, `SmHashTable`).
//!
//! | Rust          | shim                                                   |
//! |---------------|--------------------------------------------------------|
//! | [`run`]       | `sc_geom_hash_table_ints`, `sc_geom_hash_table_floats` |
//!
//! A program (flat `f64`, encoding documented in the shim) is validated here
//! before any call; the shim additionally rejects the upstream undefined
//! behaviour paths that need the model state.

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_geom_hash_table_ints(
            prog: *const f64,
            n: i64,
            out: *mut i64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_geom_hash_table_floats(
            prog: *const f64,
            n: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Largest item id + 1 (size of the shim's fake object array).
pub const MAX_ITEMS: i64 = 4096;

/// Largest voxel count per axis, voxel total and item estimate.
pub const MAX_COUNT: f64 = 1_048_576.0;

/// Fixed operand count of op `op` (op 4 is followed by its own `n` indices),
/// or `None` for an unknown op.
pub fn op_size(op: i64) -> Option<usize> {
    Some(match op {
        0 | 1 => 11,
        2 => 6,
        3 | 6 | 7 | 10 => 1,
        4 => 4,
        5 | 9 => 3,
        8 | 11 => 0,
        _ => return None,
    })
}

fn bad(m: &str) -> FfiError {
    FfiError::InvalidArgument(m.into())
}

fn int_in(v: f64, lo: f64, hi: f64) -> bool {
    v.fract() == 0.0 && v >= lo && v <= hi
}

fn at(a: &[f64], k: usize) -> f64 {
    a.get(k).copied().unwrap_or(f64::NAN)
}

fn item_ok(v: f64) -> bool {
    int_in(v, -1.0, (MAX_ITEMS - 1) as f64)
}

fn sizes_ok(a: &[f64]) -> bool {
    let (Some(&x), Some(&y), Some(&z), Some(&est)) = (a.first(), a.get(1), a.get(2), a.get(10))
    else {
        return false;
    };
    int_in(x, 0.0, MAX_COUNT)
        && int_in(y, 0.0, MAX_COUNT)
        && int_in(z, 0.0, MAX_COUNT)
        && x * y * z <= MAX_COUNT
        && int_in(est, 0.0, MAX_COUNT)
}

fn validate(prog: &[f64]) -> Result<(), FfiError> {
    let mut i = 0usize;
    while i < prog.len() {
        let op = prog
            .get(i)
            .copied()
            .ok_or_else(|| bad("truncated program"))?;
        if !int_in(op, 0.0, 11.0) {
            return Err(bad("op"));
        }
        let op = op as i64;
        let mut sz = op_size(op).ok_or_else(|| bad("op"))?;
        let a = prog
            .get(i + 1..i + 1 + sz)
            .ok_or_else(|| bad("truncated program"))?;
        if (op == 0) != (i == 0) {
            return Err(bad("constructor must be first and only"));
        }
        match op {
            0 | 1 if !sizes_ok(a) => return Err(bad("sizes")),
            2 if !a.first().copied().is_some_and(item_ok) => return Err(bad("add item")),
            3 | 7 if !a.first().copied().is_some_and(item_ok) => return Err(bad("item")),
            4 => {
                let (lim, mode, n) = (at(a, 0), at(a, 1), at(a, 3));
                if !(lim == -1.0 || int_in(lim, 0.0, 1e15))
                    || !int_in(mode, 0.0, 2.0)
                    || !int_in(n, 0.0, MAX_COUNT)
                {
                    return Err(bad("find operands"));
                }
                let idx = prog
                    .get(i + 1 + sz..i + 1 + sz + n as usize)
                    .ok_or_else(|| bad("truncated bucket list"))?;
                if idx.iter().any(|&b| !int_in(b, 0.0, MAX_COUNT)) {
                    return Err(bad("bucket index"));
                }
                sz += n as usize;
            }
            6 | 10 if !int_in(at(a, 0), 0.0, 1e15) => return Err(bad("voxel index")),
            9 if a.iter().any(|&v| !int_in(v, 0.0, 1e15)) => return Err(bad("voxel indices")),
            _ => {}
        }
        i += 1 + sz;
    }
    if prog.is_empty() {
        return Err(bad("empty program"));
    }
    Ok(())
}

/// Runs a voxel-model program; returns its integer and float output streams.
pub fn run(prog: &[f64]) -> Result<(Vec<i64>, Vec<f64>), FfiError> {
    validate(prog)?;
    let n = prog.len() as i64;
    let ints = call_with_buffer(256, |buf: &mut [i64], len| {
        // SAFETY: prog valid for n reads; buf valid for buf.len() writes; len valid.
        unsafe {
            sys::sc_geom_hash_table_ints(
                prog.as_ptr(),
                n,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })?;
    let floats = call_with_buffer(256, |buf: &mut [f64], len| {
        // SAFETY: prog valid for n reads; buf valid for buf.len() writes; len valid.
        unsafe {
            sys::sc_geom_hash_table_floats(
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
