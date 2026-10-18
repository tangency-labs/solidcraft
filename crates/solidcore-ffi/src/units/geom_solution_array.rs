//! FFI wrappers for unit `geom.solution_array`.
//!
//! Shim: `harness/cpp/shim/geom_solution_array.cpp`.
//! Upstream: `source/SMLib/src/SmSolutionArray.cpp` (`SmSolutionEnd`, `SmSolution`,
//! `SmSolutionArray`).
//!
//! | Rust    | shim                                                          |
//! |---------|---------------------------------------------------------------|
//! | [`run`] | `sc_geom_solution_array_ints`, `sc_geom_solution_array_floats` |
//!
//! [`run`] executes a small program against one upstream solution array; the
//! encoding is documented in the shim. Its static shape (op codes, operand
//! ranges, solution fields) is validated here; checks that need the array's
//! run-time state are made by the shim (`SC_ERR_ARG`).

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_geom_solution_array_ints(
            prog: *const f64,
            n: i64,
            out: *mut i64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_geom_solution_array_floats(
            prog: *const f64,
            n: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Doubles per encoded solution.
pub const SOLUTION_LEN: usize = 35;

/// Number of object kinds (codes `0..KINDS`).
pub const KINDS: i64 = 15;

/// Objects per kind (ids `0..OBJECTS_PER_KIND`).
pub const OBJECTS_PER_KIND: i64 = 100;

/// Largest tree node id + 1.
pub const MAX_NODES: i64 = 4096;

/// Largest array size a program may request (capacity, `SetSize`, insert index).
pub const MAX_SIZE: f64 = 4096.0;

/// Largest insert count / appended array length.
pub const MAX_COUNT: f64 = 64.0;

/// Largest plain index operand (indices the shim checks or upstream bounds).
const MAX_INDEX: f64 = 1.0e6;

fn at(s: &[f64], i: usize) -> Result<f64, FfiError> {
    s.get(i).copied().ok_or_else(|| bad("truncated program"))
}

fn int_in(v: f64, lo: f64, hi: f64) -> bool {
    v.fract() == 0.0 && v >= lo && v <= hi
}

/// Fixed operand count of `op` (before embedded solutions), or `None`.
pub fn op_fixed(op: i64) -> Option<usize> {
    Some(match op {
        2 | 10 | 12 | 13 | 21 | 28 => 0,
        1 | 3 | 7 | 8 | 9 | 11 | 20 | 25 | 26 | 27 => 1,
        4 | 5 | 6 | 22 | 23 => 2,
        24 => 3,
        _ => return None,
    })
}

fn check_object(v: f64) -> Result<(), FfiError> {
    if v == -1.0 || int_in(v, 0.0, (KINDS * OBJECTS_PER_KIND - 1) as f64) {
        Ok(())
    } else {
        Err(bad("object code"))
    }
}

fn check_solution(s: &[f64]) -> Result<(), FfiError> {
    if s.len() != SOLUTION_LEN {
        return Err(bad("truncated solution"));
    }
    if !int_in(at(s, 0)?, 0.0, 4.0) || !int_in(at(s, 1)?, 0.0, 10.0) || !int_in(at(s, 2)?, 0.0, 5.0)
    {
        return Err(bad("solution type or counts"));
    }
    for k in 25..30 {
        check_object(at(s, k)?)?;
    }
    for k in 30..35 {
        let v = at(s, k)?;
        if v != -1.0 && !int_in(v, 0.0, (MAX_NODES - 1) as f64) {
            return Err(bad("node id"));
        }
    }
    Ok(())
}

fn validate(prog: &[f64]) -> Result<(), FfiError> {
    if prog.is_empty() {
        return Err(bad("empty program"));
    }
    let mut i = 0usize;
    while i < prog.len() {
        let op = at(prog, i)?;
        if !int_in(op, 1.0, 28.0) {
            return Err(bad("op"));
        }
        let op = op as i64;
        let fixed = op_fixed(op).ok_or_else(|| bad("op"))?;
        let a = prog
            .get(i + 1..i + 1 + fixed)
            .ok_or_else(|| bad("truncated program"))?;
        let flag = |j: usize| -> Result<(), FfiError> {
            if int_in(at(a, j)?, 0.0, 1.0) {
                Ok(())
            } else {
                Err(bad("flag"))
            }
        };
        let index = |j: usize, hi: f64| -> Result<(), FfiError> {
            if int_in(at(a, j)?, 0.0, hi) {
                Ok(())
            } else {
                Err(bad("index operand"))
            }
        };
        let nsols = match op {
            1 | 7 => {
                index(0, MAX_SIZE)?;
                0
            }
            3 => {
                index(0, 4.0)?;
                1
            }
            4 => {
                index(0, MAX_SIZE)?;
                index(1, MAX_COUNT)?;
                1
            }
            5 | 6 => {
                index(0, MAX_INDEX)?;
                index(1, MAX_INDEX)?;
                0
            }
            8 | 25 | 26 | 27 => {
                index(0, MAX_INDEX)?;
                1
            }
            9 => {
                index(0, MAX_INDEX)?;
                0
            }
            11 => {
                index(0, MAX_COUNT)?;
                at(a, 0)? as usize
            }
            20 => {
                flag(0)?;
                2
            }
            21 => 2,
            22 => {
                check_object(at(a, 0)?)?;
                flag(1)?;
                1
            }
            23 => {
                index(0, MAX_INDEX)?;
                flag(1)?;
                1
            }
            24 => {
                check_object(at(a, 0)?)?;
                flag(1)?;
                flag(2)?;
                1
            }
            2 | 28 => 1,
            _ => 0,
        };
        let start = i + 1 + fixed;
        for k in 0..nsols {
            let s = prog
                .get(start + k * SOLUTION_LEN..start + (k + 1) * SOLUTION_LEN)
                .ok_or_else(|| bad("truncated program"))?;
            check_solution(s)?;
        }
        i = start + nsols * SOLUTION_LEN;
    }
    Ok(())
}

/// Runs a solution-array program; returns its integer and float output streams.
pub fn run(prog: &[f64]) -> Result<(Vec<i64>, Vec<f64>), FfiError> {
    validate(prog)?;
    let n = prog.len() as i64;
    let ints = call_with_buffer(256, |buf: &mut [i64], len| {
        // SAFETY: prog valid for n reads (validated above); buf valid for buf.len()
        // writes; len valid.
        unsafe {
            sys::sc_geom_solution_array_ints(
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
            sys::sc_geom_solution_array_floats(
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
