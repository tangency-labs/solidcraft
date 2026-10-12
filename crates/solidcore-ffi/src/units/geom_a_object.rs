//! FFI wrappers for unit `geom.a_object`.
//!
//! Shim: `harness/cpp/shim/geom_a_object.cpp`.
//! Upstream: `source/SMLib/src/SmAObject.cpp`, `source/SMLib/src/SmCacheMgr.cpp`,
//! `source/SMLib/src/SmSAGObject.cpp`, `source/SMLib/inc/NLI_Math.h`.
//!
//! | Rust      | shim                     | upstream                                        |
//! |-----------|--------------------------|-------------------------------------------------|
//! | [`types`] | `sc_geom_a_object_types` | `IsKindOf`, `GetType`, strings, `AssertValid`   |
//! | [`nli`]   | `sc_geom_a_object_nli`   | `NI_CPtToPtEuclid_specialized`, `NI_VectorBlendCPt_specialized` |
//! | [`run`]   | `sc_geom_a_object_run`   | a script of notify / attribute / cache calls    |

use crate::{call_with_buffer, check, FfiError, SC_ERR_BUFFER};

mod sys {
    extern "C" {
        pub fn sc_geom_a_object_types(
            ts: *const i64,
            n: i64,
            out: *mut i64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_geom_a_object_nli(
            noz: i32,
            now: i32,
            pw: *const f64,
            alpha: f64,
            aw: *const f64,
            bw: *const f64,
            out: *mut f64,
        ) -> i32;
        pub fn sc_geom_a_object_run(
            kinds: *const i64,
            n_kinds: i64,
            user_cb: i32,
            sys_cb: i32,
            ops: *const i64,
            n_ops: i64,
            payload_d: *const f64,
            n_payload_d: i64,
            ints: *mut i64,
            ints_cap: i64,
            ints_len: *mut i64,
            dbls: *mut f64,
            dbls_cap: i64,
            dbls_len: *mut i64,
        ) -> i32;
    }
}

/// Script op codes (must match the shim's `enum Op`). Arguments are `a..g` of the
/// 8-wide record `[op, a, b, c, d, e, f, g]`; `ref` is an attribute reference.
pub mod op {
    /// `new SmAttribute` `[id, beh]`.
    pub const NEW_PLAIN: i64 = 0;
    /// `new SmLongAttribute` `[id, value, beh]`.
    pub const NEW_LONG: i64 = 1;
    /// `new SmVector3dAttribute` `[id, beh]` (3 payload doubles).
    pub const NEW_VECTOR3D: i64 = 2;
    /// `new SmColorAttribute` `[id, beh]` (3 payload doubles).
    pub const NEW_COLOR: i64 = 3;
    /// `new SmMergeRegionAttribute` `[this_obj, this_type, other_obj, other_type]`.
    pub const NEW_MERGE_REGION: i64 = 4;
    /// `SetOperation` `[ref, operation]`.
    pub const MR_SET_OPERATION: i64 = 5;
    /// `AddAttribute` `[obj, ref]`.
    pub const ADD: i64 = 6;
    /// `RemoveAttribute` `[obj, ref, do_not_delete]`.
    pub const REMOVE: i64 = 7;
    /// `ClearAttributes` `[obj]`.
    pub const CLEAR: i64 = 8;
    /// `FindAttribute` `[obj, id]`.
    pub const FIND: i64 = 9;
    /// `SmAObject::Notify` `[op, caller, d1, d2, d3]` (data `-1` = NULL).
    pub const NOTIFY: i64 = 10;
    /// `new SmAObject(src)` `[src]`.
    pub const COPY_CONSTRUCT: i64 = 11;
    /// `SmAObject::AssertValid` `[obj, level_given]`.
    pub const ASSERT_VALID: i64 = 12;
    /// `new ShimCache` `[]`.
    pub const NEW_CACHE: i64 = 13;
    /// `SetCacheObj` `[obj, cache]` (`-1` = NULL).
    pub const SET_CACHE: i64 = 14;
    /// `SmCacheMgr::GetObjectCache` `[obj, type]` (`obj -1` = NULL).
    pub const GET_CACHE: i64 = 15;
    /// `SmCacheMgr::CheckOutCache` `[cache]`.
    pub const CHECK_OUT: i64 = 16;
    /// `SmCacheMgr::CheckInCache` `[cache]`.
    pub const CHECK_IN: i64 = 17;
    /// `SmCacheMgr::SoftDelete` `[cache]`.
    pub const SOFT_DELETE: i64 = 18;
    /// `SmCacheMgr::DeleteObjectsCache` `[obj, overload, type]`.
    pub const DELETE_OBJECTS_CACHE: i64 = 19;
    /// `SmCacheMgr::GetOrCreateObjectCache` `[obj, type]`.
    pub const GET_OR_CREATE: i64 = 20;
    /// `SmCacheCheckOutIn(cache)` `[cache]`.
    pub const GUARD_OPEN: i64 = 21;
    /// `~SmCacheCheckOutIn` of the newest guard `[]`.
    pub const GUARD_CLOSE: i64 = 22;
    /// `SmSAGObject::CacheMakeOrValidate` `[this_brep, target, old_cache, type]`.
    pub const SAG_MAKE: i64 = 23;
    /// `SmSAGObject::Notify` `[op, caller_brep, -1, -1, -1]`.
    pub const SAG_NOTIFY: i64 = 24;
}

/// Most objects a script may hold (initial plus copy-constructed).
pub const MAX_OBJECTS: usize = 64;

/// [`types`] output: per queried type `(SmAObject::IsKindOf, SmSAGObject::IsKindOf)`,
/// then the type codes, `AssertValid` results and strings (layout in the shim).
pub fn types(ts: &[i64]) -> Result<Vec<i64>, FfiError> {
    call_with_buffer(256 + 2 * ts.len(), |buf, len| {
        // SAFETY: ts valid for ts.len() reads; buf valid for buf.len() writes; len valid.
        unsafe {
            sys::sc_geom_a_object_types(
                ts.as_ptr(),
                ts.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// `NI_CPtToPtEuclid_specialized(noz, now, pw)` (3 values), then
/// `NI_VectorBlendCPt_specialized(noz, now, alpha, aw, bw)` (4 values).
pub fn nli(
    noz: bool,
    now: bool,
    pw: [f64; 4],
    alpha: f64,
    aw: [f64; 4],
    bw: [f64; 4],
) -> Result<[f64; 7], FfiError> {
    let mut out = [0.0; 7];
    // SAFETY: the three inputs hold 4 doubles each and `out` 7, as the shim reads/writes.
    let st = unsafe {
        sys::sc_geom_a_object_nli(
            i32::from(noz),
            i32::from(now),
            pw.as_ptr(),
            alpha,
            aw.as_ptr(),
            bw.as_ptr(),
            out.as_mut_ptr(),
        )
    };
    check(st)?;
    Ok(out)
}

/// Raw result of [`run`]: the shim's int and double output streams.
#[derive(Debug, Clone, PartialEq)]
pub struct RawOutput {
    /// Integer stream.
    pub ints: Vec<i64>,
    /// Double stream.
    pub dbls: Vec<f64>,
}

fn validate(kinds: &[i64], ops: &[[i64; 8]], payload_d: &[f64]) -> Result<(), FfiError> {
    let bad = |m: String| Err(FfiError::InvalidArgument(m));
    if kinds.len() > MAX_OBJECTS || kinds.iter().any(|k| !(0..=2).contains(k)) {
        return bad("object kinds".into());
    }
    let behavior_ok = |b: i64| (0..=4).contains(&b);
    let cache_type_ok = |t: i64| (0..=3).contains(&t);
    let mut n_vec = 0usize;
    for o in ops {
        let [code, a, b, c, d, ..] = *o;
        let ok = match code {
            op::NEW_PLAIN => behavior_ok(b),
            op::NEW_LONG => behavior_ok(c),
            op::NEW_VECTOR3D | op::NEW_COLOR => {
                n_vec += 1;
                behavior_ok(b)
            }
            op::MR_SET_OPERATION => (0..=10).contains(&b),
            op::NOTIFY | op::SAG_NOTIFY => (0..=16).contains(&a),
            op::GET_CACHE | op::GET_OR_CREATE => cache_type_ok(b),
            op::DELETE_OBJECTS_CACHE => cache_type_ok(c) && (0..=1).contains(&b),
            op::SAG_MAKE => cache_type_ok(d),
            op::NEW_MERGE_REGION
            | op::ADD
            | op::REMOVE
            | op::CLEAR
            | op::FIND
            | op::COPY_CONSTRUCT
            | op::ASSERT_VALID
            | op::NEW_CACHE
            | op::SET_CACHE
            | op::CHECK_OUT
            | op::CHECK_IN
            | op::SOFT_DELETE
            | op::GUARD_OPEN
            | op::GUARD_CLOSE => true,
            _ => false,
        };
        if !ok {
            return bad(format!("op record {o:?}"));
        }
    }
    if payload_d.len() != 3 * n_vec {
        return bad("vector payload length".into());
    }
    Ok(())
}

/// Runs a script of 8-wide op records against fresh objects of the given kinds
/// (0 region, 1 `SmAObject`, 2 `SmBrep`), with the recording notify callbacks
/// installed as asked.
pub fn run(
    kinds: &[i64],
    user_cb: bool,
    sys_cb: bool,
    ops: &[[i64; 8]],
    payload_d: &[f64],
) -> Result<RawOutput, FfiError> {
    validate(kinds, ops, payload_d)?;
    let flat: Vec<i64> = ops.iter().flatten().copied().collect();
    let mut ints_cap = 4096 + 64 * ops.len();
    let mut dbls_cap = 1024 + 4 * payload_d.len();
    for _ in 0..2 {
        let mut ints = vec![0i64; ints_cap];
        let mut dbls = vec![0f64; dbls_cap];
        let (mut ints_len, mut dbls_len) = (0i64, 0i64);
        // SAFETY: `kinds`, `flat` (8*ops.len()) and `payload_d` are valid for their
        // lengths; `ints`/`dbls` have the capacities passed (the shim checks them
        // before writing); the length out-params are valid. Kinds, op codes,
        // behaviours, enum values and the payload length were validated above; the
        // shim checks object, attribute and cache indices itself. Nothing is retained.
        let st = unsafe {
            sys::sc_geom_a_object_run(
                kinds.as_ptr(),
                kinds.len() as i64,
                i32::from(user_cb),
                i32::from(sys_cb),
                flat.as_ptr(),
                ops.len() as i64,
                payload_d.as_ptr(),
                payload_d.len() as i64,
                ints.as_mut_ptr(),
                ints.len() as i64,
                &raw mut ints_len,
                dbls.as_mut_ptr(),
                dbls.len() as i64,
                &raw mut dbls_len,
            )
        };
        let ni = usize::try_from(ints_len).unwrap_or(0);
        let nd = usize::try_from(dbls_len).unwrap_or(0);
        if st == SC_ERR_BUFFER {
            ints_cap = ni.max(ints_cap);
            dbls_cap = nd.max(dbls_cap);
            continue;
        }
        check(st)?;
        ints.truncate(ni);
        dbls.truncate(nd);
        return Ok(RawOutput { ints, dbls });
    }
    Err(FfiError::Status(SC_ERR_BUFFER))
}
