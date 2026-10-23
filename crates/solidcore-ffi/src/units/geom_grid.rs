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
//!
//! Opcodes 11–15 are an opt-in, unit-local extension: define an affine degree-1
//! tensor-product B-spline, add its real hierarchy, expand that hierarchy, add a
//! cache-owned leaf by ordinal, and observe the caches/grid. Opcodes 1–10 retain
//! their original opaque-object semantics, even when an explicit surface has the
//! same numeric label. The bounded affine family avoids unbounded subdivision of
//! arbitrary Bezier control nets; it is not a restriction on the legacy corpus.

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
        1 | 12 | 14 => 8,
        3 | 6 | 10 => 6,
        4 | 5 => 3,
        7 | 9 | 13 => 1,
        8 | 15 => 0,
        11 => 16,
        _ => return None,
    })
}

fn int_in(v: f64, lo: i64, hi: i64) -> bool {
    v.fract() == 0.0 && v >= lo as f64 && v <= hi as f64
}

fn coords_ok(a: &[f64]) -> bool {
    a.iter().all(|v| v.is_finite() && v.abs() <= MAX_COORD)
}

/// Validates a grid program without executing native code.
///
/// Explicit surfaces are finite, well-conditioned affine patches with at most
/// three spans per direction. A selected leaf is checked again against the actual
/// native cache before its node is passed to `SmGrid::AddElement`.
pub fn validate(prog: &[f64]) -> Result<(), FfiError> {
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
    let mut surfaces = Vec::new();
    let mut real_hier = Vec::new();
    let mut i = 10usize;
    while i < prog.len() {
        let op = prog
            .get(i)
            .copied()
            .ok_or_else(|| bad("truncated program"))?;
        if !int_in(op, 1, 15) {
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
                real_hier.push(false);
                (at(0) == 0.0 || at(0) == 1.0)
                    && int_in(at(1), 0, MAX_OBJECTS - 1)
                    && coords_ok(a.get(2..9).unwrap_or(&[f64::NAN]))
            }
            3 | 4 | 6 | 10 => coords_ok(a),
            5 => a.iter().all(|&v| int_in(v, -1000, 1000)),
            7 => !at(0).is_nan(),
            9 => int_in(at(0), 0, hier - 1),
            11 => {
                let id = at(0);
                let valid = surfaces.len() < 4
                    && int_in(id, 0, MAX_OBJECTS - 1)
                    && !surfaces.contains(&(id as i64))
                    && affine_surface_ok(a);
                if valid {
                    surfaces.push(id as i64);
                }
                valid
            }
            12 => {
                hier += 1;
                real_hier.push(true);
                int_in(at(0), 0, MAX_OBJECTS - 1)
                    && surfaces.contains(&(at(0) as i64))
                    && at(1).is_finite()
                    && (0.0..=1.0).contains(&at(1))
                    && box_ok(a.get(2..8).unwrap_or_default())
            }
            13 => int_in(at(0), 0, hier - 1) && real_hier.get(at(0) as usize) == Some(&true),
            14 => {
                int_in(at(0), 0, MAX_OBJECTS - 1)
                    && surfaces.contains(&(at(0) as i64))
                    && int_in(at(1), 0, 255)
                    && box_ok(a.get(2..8).unwrap_or_default())
            }
            15 => !surfaces.is_empty(),
            _ => true,
        };
        if !ok {
            return Err(bad("operands"));
        }
        i += 1 + sz;
    }
    Ok(())
}

fn box_ok(a: &[f64]) -> bool {
    a.len() == 6
        && coords_ok(a)
        && (0..3).all(|k| a.get(k).zip(a.get(k + 3)).is_some_and(|(l, h)| l <= h))
}

fn affine_surface_ok(a: &[f64]) -> bool {
    let at = |i| a.get(i).copied().unwrap_or(f64::NAN);
    if !a
        .get(1..10)
        .is_some_and(|v| v.iter().all(|x| x.is_finite() && x.abs() <= 1024.0))
        || !(10..14).all(|i| at(i).is_finite() && at(i).abs() <= 8.0)
        || ![at(12) - at(10), at(13) - at(11)]
            .iter()
            .all(|d| (0.125..=16.0).contains(d))
        || !int_in(at(14), 1, 3)
        || !int_in(at(15), 1, 3)
    {
        return false;
    }
    let u = [at(4), at(5), at(6)];
    let v = [at(7), at(8), at(9)];
    let uu = u.iter().map(|x| x * x).sum::<f64>();
    let vv = v.iter().map(|x| x * x).sum::<f64>();
    let [ux, uy, uz] = u;
    let [vx, vy, vz] = v;
    let cross = [uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx];
    (0.015625..=4096.0).contains(&uu)
        && (0.015625..=4096.0).contains(&vv)
        && cross.iter().map(|x| x * x).sum::<f64>() >= 0.01 * uu * vv
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
