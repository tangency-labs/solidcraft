//! FFI wrappers for unit `geom.object`.
//!
//! Shim: `harness/cpp/shim/geom_object.cpp`.
//! Upstream: `source/SMLib/inc/SmObject.h`, `source/SMLib/src/SmObject.cpp`
//! (`SmObject`, `SmTemporaryChangeValue`), `source/SMLib/inc/SmShape.h`,
//! `source/SMLib/src/SmShape.cpp` (`SmShape`).
//!
//! | Rust       | shim                    | upstream                                         |
//! |------------|-------------------------|--------------------------------------------------|
//! | [`types`]  | `sc_geom_object_types`  | `SmObject` / `SmShape` `IsKindOf`, `GetType`, `GetTypeString`, `AssertValid` |
//! | [`notify`] | `sc_geom_object_notify` | a script of `SmObject::Notify` calls             |
//! | [`tcv`]    | `sc_geom_object_tcv`    | a script of `SmTemporaryChangeValue<TYPE>` calls |
//! | [`shape`]  | `sc_geom_object_shape`  | `SmShape::SmShape` and its accessors             |
//! | [`list_orders`] | `sc_geom_object_list_orders` | `SmBrep::GetFaces`, `GetEdges`, `GetVertices` of the fixture B-rep |

use std::ffi::CString;

use super::topology_edgeuse_p1 as graph;
use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_geom_object_types(
            ts: *const i64,
            n: i64,
            out: *mut i64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_geom_object_notify(
            ops: *const i64,
            n_ops: i64,
            out: *mut i64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_geom_object_tcv(
            ty: i32,
            init: f64,
            ops: *const i64,
            xs: *const f64,
            n_ops: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_geom_object_list_orders(
            path: *const std::ffi::c_char,
            block: *const f64,
            n_block: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_geom_object_shape(
            path: *const std::ffi::c_char,
            block: *const f64,
            n_block: i64,
            sel: *const i64,
            n_sel: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Number of objects in a notify script's object pool.
pub const NUM_OBJECTS: i64 = 6;

/// Largest `SmNotifyOperation` value (`SM_NO_UNKNOWN`).
pub const MAX_NOTIFY_OP: i64 = 16;

/// `IsKindOf` of `SmObject` and `SmShape` for each of `ts` (pairs), then
/// `SmObject::GetType`, `SmShape::GetType`, `SmObject::AssertValid` and the two
/// type strings as (length, bytes...).
pub fn types(ts: &[i64]) -> Result<Vec<i64>, FfiError> {
    call_with_buffer(2 * ts.len() + 64, |buf: &mut [i64], len| {
        // SAFETY: ts is valid for ts.len() reads; buf for buf.len() writes; len for one.
        unsafe {
            sys::sc_geom_object_types(
                ts.as_ptr(),
                ts.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// One `Notify` call: whether the caller has a context, whether that context has a
/// user / system callback, the operation, the caller and the data objects (pool
/// indices, `-1` = NULL).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotifyStep {
    /// The caller's context is the shared one (else NULL).
    pub has_context: bool,
    /// A user notify callback is installed.
    pub user_callback: bool,
    /// A system notify callback is installed.
    pub sys_callback: bool,
    /// `SmNotifyOperation` value.
    pub op: i64,
    /// Caller index.
    pub caller: i64,
    /// Data object indices (`-1` = NULL).
    pub data: [i64; 3],
}

/// Runs `SmObject::Notify` for each step. Output per step: the number of
/// `Execute` records, then each record (tag 1 = user, 2 = system, op, caller,
/// caller type, data1, data2, data3).
pub fn notify(steps: &[NotifyStep]) -> Result<Vec<i64>, FfiError> {
    for s in steps {
        if !(0..=MAX_NOTIFY_OP).contains(&s.op)
            || !(0..NUM_OBJECTS).contains(&s.caller)
            || s.data.iter().any(|d| !(-1..NUM_OBJECTS).contains(d))
        {
            return Err(FfiError::InvalidArgument("notify step".into()));
        }
    }
    let ops: Vec<i64> = steps
        .iter()
        .flat_map(|s| {
            [
                i64::from(s.has_context),
                i64::from(s.user_callback),
                i64::from(s.sys_callback),
                s.op,
                s.caller,
                s.data[0],
                s.data[1],
                s.data[2],
            ]
        })
        .collect();
    call_with_buffer(16 * steps.len() + 16, |buf: &mut [i64], len| {
        // SAFETY: ops holds 8 * steps.len() validated values; buf is valid for
        // buf.len() writes and len for one.
        unsafe {
            sys::sc_geom_object_notify(
                ops.as_ptr(),
                steps.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// `SmTemporaryChangeValue` script operation codes.
pub mod tcv_op {
    /// Push `SmTemporaryChangeValue(var, x)`.
    pub const OPEN: i64 = 0;
    /// Push `SmTemporaryChangeValue()` then `Set(var, x)`.
    pub const OPEN_SET: i64 = 1;
    /// Pop the innermost guard (destructor).
    pub const CLOSE: i64 = 2;
    /// `Clear()` on the innermost guard.
    pub const CLEAR: i64 = 3;
    /// `UseOrigValue()` on the innermost guard.
    pub const USE_ORIG: i64 = 4;
    /// `UseTempValue()` on the innermost guard.
    pub const USE_TEMP: i64 = 5;
    /// `SetNewValue(x)` on the innermost guard.
    pub const SET_NEW: i64 = 6;
    /// `var = x`.
    pub const WRITE: i64 = 7;
}

/// Whether `x` is a valid integer operand (`0 <= x <= 1e9`, integral).
fn int_operand(x: f64) -> bool {
    (0.0..=1e9).contains(&x) && x == (x as i64) as f64
}

/// Runs a `SmTemporaryChangeValue<TYPE>` script on one variable (`ty` 0 = double,
/// 1 = `SmBoolean`, 2 = `ULONG`; integer operands must be integral in `[0, 1e9]`).
/// Output: the variable after construction, after every op, and after each
/// remaining guard is destroyed.
pub fn tcv(ty: i32, init: f64, steps: &[(i64, f64)]) -> Result<Vec<f64>, FfiError> {
    if !(0..=2).contains(&ty) {
        return Err(FfiError::InvalidArgument("type".into()));
    }
    if ty != 0 && !int_operand(init) {
        return Err(FfiError::InvalidArgument("init".into()));
    }
    let mut depth = 0i64;
    for &(op, x) in steps {
        if !(tcv_op::OPEN..=tcv_op::WRITE).contains(&op) {
            return Err(FfiError::InvalidArgument("op".into()));
        }
        if ty != 0 && !int_operand(x) {
            return Err(FfiError::InvalidArgument("operand".into()));
        }
        match op {
            tcv_op::OPEN | tcv_op::OPEN_SET => depth += 1,
            tcv_op::WRITE => {}
            _ if depth == 0 => return Err(FfiError::InvalidArgument("no guard".into())),
            tcv_op::CLOSE => depth -= 1,
            _ => {}
        }
    }
    let ops: Vec<i64> = steps.iter().map(|s| s.0).collect();
    let xs: Vec<f64> = steps.iter().map(|s| s.1).collect();
    call_with_buffer(2 * steps.len() + 8, |buf: &mut [f64], len| {
        // SAFETY: ops/xs hold steps.len() validated values; buf is valid for
        // buf.len() writes and len for one.
        unsafe {
            sys::sc_geom_object_tcv(
                ty,
                init,
                ops.as_ptr(),
                xs.as_ptr(),
                steps.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Where a shape's B-rep comes from.
#[derive(Debug, Clone, PartialEq)]
pub enum ShapeSource {
    /// No B-rep (only the empty selection is valid).
    None,
    /// The B-rep of the shared topology fixture `path` / `block` (the `topology_edgeuse_p1`
    /// encoding).
    Fixture {
        /// The fixture's file path (empty for constructed B-reps).
        path: String,
        /// The fixture block.
        block: Vec<f64>,
    },
}

/// Which members of the B-rep go into the shape: the whole B-rep and indices into
/// the fixture walk's face, edge and vertex lists.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShapeSelection {
    /// Add the whole B-rep.
    pub whole_brep: bool,
    /// Face indices.
    pub faces: Vec<i64>,
    /// Edge indices.
    pub edges: Vec<i64>,
    /// Vertex indices.
    pub vertices: Vec<i64>,
}

/// Builds `SmShape` from the selection. Output: shape face/edge/vertex counts and
/// their B-rep list indices, each tree (node count, object count, has top, top
/// box), then `IsInShape` of every B-rep face, edge and vertex. Out-of-range
/// indices are rejected by the shim.
pub fn shape(source: &ShapeSource, sel: &ShapeSelection) -> Result<Vec<f64>, FfiError> {
    let empty =
        !sel.whole_brep && sel.faces.is_empty() && sel.edges.is_empty() && sel.vertices.is_empty();
    let (path, block): (&str, &[f64]) = match source {
        ShapeSource::None if empty => ("", &[]),
        ShapeSource::None => {
            return Err(FfiError::InvalidArgument("selection without B-rep".into()))
        }
        ShapeSource::Fixture { path, block } => {
            // the graph wrapper validates (and builds) the block first
            let export = graph::eval(graph::Op::Export, path, block)?;
            if graph::counts(&export).is_none() {
                return Err(FfiError::InvalidArgument(
                    "native fixture construction failed".into(),
                ));
            }
            (path, block)
        }
    };
    let path = CString::new(path).map_err(|_| FfiError::InvalidArgument("path".into()))?;
    let n_block = i64::try_from(block.len())
        .map_err(|_| FfiError::InvalidArgument("block too large".into()))?;
    let mut s = vec![
        i64::from(sel.whole_brep),
        sel.faces.len() as i64,
        sel.edges.len() as i64,
        sel.vertices.len() as i64,
    ];
    s.extend(&sel.faces);
    s.extend(&sel.edges);
    s.extend(&sel.vertices);
    call_with_buffer(512, |buf: &mut [f64], len| {
        // SAFETY: path is a NUL-terminated string and block holds n_block values (validated
        // by the graph wrapper), both living across the call; s holds s.len() values in
        // the documented layout (indices are range-checked by the shim); buf is valid for
        // buf.len() writes.
        unsafe {
            sys::sc_geom_object_shape(
                path.as_ptr(),
                block.as_ptr(),
                n_block,
                s.as_ptr(),
                s.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// The fixture B-rep's `GetFaces`, `GetEdges` and `GetVertices` orders as fixture walk
/// indices: `n, indices...` per kind.
pub fn list_orders(path: &str, block: &[f64]) -> Result<Vec<f64>, FfiError> {
    let export = graph::eval(graph::Op::Export, path, block)?;
    if graph::counts(&export).is_none() {
        return Err(FfiError::InvalidArgument(
            "native fixture construction failed".into(),
        ));
    }
    let path = CString::new(path).map_err(|_| FfiError::InvalidArgument("path".into()))?;
    let n_block = i64::try_from(block.len())
        .map_err(|_| FfiError::InvalidArgument("block too large".into()))?;
    call_with_buffer(256, |buf: &mut [f64], len| {
        // SAFETY: path is a NUL-terminated string and block holds n_block values (validated
        // by the graph wrapper), both living across the call; buf is valid for buf.len()
        // writes.
        unsafe {
            sys::sc_geom_object_list_orders(
                path.as_ptr(),
                block.as_ptr(),
                n_block,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
