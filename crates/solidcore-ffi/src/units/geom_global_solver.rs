//! FFI wrappers for unit `geom.global_solver`.
//!
//! Shim: `harness/cpp/shim/geom_global_solver.cpp`.
//! Upstream: `source/SMLib/src/SmGlobalSolver.cpp` (`SmGlobalSolver`).
//!
//! | Rust    | shim                                                          |
//! |---------|---------------------------------------------------------------|
//! | [`run`] | `sc_geom_global_solver_ints`, `sc_geom_global_solver_floats`  |
//!
//! [`run`] executes a small program (trees, a solver, solves, sorted insertions,
//! protected queries) against upstream; the encoding is documented in the shim.
//! Its static shape (op codes, enum ranges, counts, solution fields) is
//! validated here; checks that need run-time state (tree shapes, solver tree
//! counts) are made by the shim (`SC_ERR_ARG`).

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_geom_global_solver_ints(
            prog: *const f64,
            n: i64,
            out: *mut i64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_geom_global_solver_floats(
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

/// Doubles per encoded tree node (parent, child 1, child 2, aux type, box).
pub const NODE_LEN: usize = 10;

/// Most nodes per tree.
pub const MAX_TREE_NODES: usize = 256;

fn at(s: &[f64], i: usize) -> Result<f64, FfiError> {
    s.get(i).copied().ok_or_else(|| bad("truncated program"))
}

fn int_in(v: f64, lo: f64, hi: f64) -> bool {
    v.fract() == 0.0 && v >= lo && v <= hi
}

fn check_int(v: f64, lo: f64, hi: f64, what: &str) -> Result<(), FfiError> {
    if int_in(v, lo, hi) {
        Ok(())
    } else {
        Err(bad(what))
    }
}

fn check_object(v: f64) -> Result<(), FfiError> {
    if v == -1.0 || int_in(v, 0.0, 99.0) || int_in(v, 1400.0, 1499.0) {
        Ok(())
    } else {
        Err(bad("object code"))
    }
}

fn check_solution(s: &[f64]) -> Result<(), FfiError> {
    if s.len() != SOLUTION_LEN {
        return Err(bad("truncated solution"));
    }
    check_int(at(s, 0)?, 0.0, 4.0, "solution type")?;
    check_int(at(s, 1)?, 0.0, 10.0, "solution variable count")?;
    check_int(at(s, 2)?, 0.0, 5.0, "solution object count")?;
    for k in 25..30 {
        check_object(at(s, k)?)?;
    }
    for k in 30..35 {
        if at(s, k)? != -1.0 {
            return Err(bad("solution node (must be -1)"));
        }
    }
    Ok(())
}

/// Operand count of the op at `prog[i]`, checking its operands.
fn op_len(prog: &[f64], i: usize) -> Result<usize, FfiError> {
    let op = at(prog, i)?;
    let a = prog.get(i + 1..).unwrap_or(&[]);
    let need = |n: usize| -> Result<&[f64], FfiError> {
        a.get(..n).ok_or_else(|| bad("truncated program"))
    };
    check_int(op, 1.0, 10.0, "op")?;
    let len = match op as i64 {
        1 => {
            let nn = at(a, 0)?;
            check_int(nn, 1.0, MAX_TREE_NODES as f64, "tree node count")?;
            let len = 1 + NODE_LEN * nn as usize;
            let t = need(len)?;
            for k in 0..nn as usize {
                let d = t
                    .get(1 + NODE_LEN * k..1 + NODE_LEN * (k + 1))
                    .ok_or_else(|| bad("tree"))?;
                for j in 0..3 {
                    check_int(at(d, j)?, -1.0, nn - 1.0, "tree node link")?;
                }
                check_int(at(d, 3)?, 0.0, 8.0, "aux type")?;
            }
            len
        }
        2 => {
            let t = need(2)?;
            check_int(at(t, 0)?, 0.0, 3.0, "solver kind")?;
            check_int(at(t, 1)?, 0.0, 10.0, "variable count")?;
            2
        }
        3 => {
            let t = need(15)?;
            check_int(at(t, 0)?, 0.0, 24.0, "solver operation")?;
            check_int(at(t, 1)?, 0.0, 3.0, "solution requested")?;
            check_int(at(t, 4)?, 0.0, 3.0, "operation category")?;
            15
        }
        4 => {
            let t = need(13)?;
            check_int(at(t, 0)?, 0.0, 24.0, "solver operation")?;
            check_int(at(t, 1)?, 0.0, 3.0, "solution requested")?;
            13
        }
        5 => {
            let t = need(2 + SOLUTION_LEN)?;
            check_int(at(t, 0)?, 0.0, 4.0, "sort key")?;
            check_int(at(t, 1)?, 0.0, 1.0, "combine flag")?;
            check_solution(t.get(2..).ok_or_else(|| bad("solution"))?)?;
            2 + SOLUTION_LEN
        }
        6 | 10 => 0,
        7 | 8 => {
            let t = need(3)?;
            for j in 0..3 {
                check_int(at(t, j)?, -1.0, (MAX_TREE_NODES - 1) as f64, "branch node")?;
            }
            3
        }
        9 => {
            let t = need(2 * SOLUTION_LEN)?;
            check_solution(t.get(..SOLUTION_LEN).ok_or_else(|| bad("solution"))?)?;
            check_solution(t.get(SOLUTION_LEN..).ok_or_else(|| bad("solution"))?)?;
            2 * SOLUTION_LEN
        }
        _ => return Err(bad("op")),
    };
    Ok(len)
}

fn validate(prog: &[f64]) -> Result<(), FfiError> {
    if prog.is_empty() {
        return Err(bad("empty program"));
    }
    let mut i = 0usize;
    while i < prog.len() {
        i += 1 + op_len(prog, i)?;
    }
    Ok(())
}

/// Runs a global-solver program; returns its integer and float output streams.
pub fn run(prog: &[f64]) -> Result<(Vec<i64>, Vec<f64>), FfiError> {
    validate(prog)?;
    let n = prog.len() as i64;
    let ints = call_with_buffer(256, |buf: &mut [i64], len| {
        // SAFETY: prog valid for n reads (validated above); buf valid for buf.len()
        // writes; len valid.
        unsafe {
            sys::sc_geom_global_solver_ints(
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
            sys::sc_geom_global_solver_floats(
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
