//! FFI wrappers for unit `topology.topology`.
//!
//! Shim: `harness/cpp/shim/topology_topology.cpp`.
//! Upstream: `source/SMLib/src/SmTopology.cpp`, `source/SMLib/inc/SmTopology.h`,
//! `source/SMLib/src/SmTrackTopologyChanges.cpp`,
//! `source/SMLib/inc/SmTrackTopologyChanges.h`.
//!
//! | Rust    | shim                       | upstream                                          |
//! |---------|----------------------------|---------------------------------------------------|
//! | [`run`] | `sc_topology_topology_run` | a script of `SmTopology` / `SmNewMarkAndLock` / `SmTrackTopologyChanges` calls |

use crate::{check, FfiError, SC_ERR_BUFFER};

mod sys {
    extern "C" {
        pub fn sc_topology_topology_run(
            n_objs: i64,
            user_cb: i32,
            kind_ts: *const i64,
            n_kind_ts: i64,
            ops: *const i64,
            n_ops: i64,
            ints: *mut i64,
            ints_cap: i64,
            ints_len: *mut i64,
            dbls: *mut f64,
            dbls_cap: i64,
            dbls_len: *mut i64,
        ) -> i32;
    }
}

/// Op codes of a script record `[op, a, b, c, d, e, f, g]`.
pub mod op {
    /// `[o, flags]`: `SetAllFlags`.
    pub const SET_FLAGS: i64 = 0;
    /// `[o]`: `GetAllFlags`.
    pub const GET_FLAGS: i64 = 1;
    /// `[o, v]`: `SetUserIndex1`.
    pub const SET_UIDX1: i64 = 2;
    /// `[o, v]`: `SetUserIndex2`.
    pub const SET_UIDX2: i64 = 3;
    /// `[o, p]`: `SetUserPtr1` (a pointer identifying object `p`, `-1` null).
    pub const SET_UPTR: i64 = 4;
    /// `[o]`: the user indices and pointer.
    pub const GET_USER: i64 = 5;
    /// `[o, mark type]`: `Mark`.
    pub const MARK: i64 = 6;
    /// `[o, mark type]`: `GetMark`.
    pub const GET_MARK: i64 = 7;
    /// `[o, mark type]`: `IsMarked`.
    pub const IS_MARKED: i64 = 8;
    /// `[mask]`: push `SmNewMarkAndLock(ctx, mask)`.
    pub const LOCK: i64 = 9;
    /// `NewMark` of the top lock.
    pub const LOCK_NEW_MARK: i64 = 10;
    /// Pop (destroy) the top lock.
    pub const UNLOCK: i64 = 11;
    /// `[operation, this, d1, d2, d3]`: `this->Notify(...)` (`-1` data are null).
    pub const NOTIFY: i64 = 12;
    /// `[o]`: `GetTolerance` (to the double output).
    pub const TOLERANCE: i64 = 13;
    /// `[o, p]`: `SmTopology::IsConnectedTo(p)`.
    pub const CONNECTED: i64 = 14;
    /// `[start_now, has_brep]`: a new `SmTrackTopologyChanges`.
    pub const TRACK_NEW: i64 = 15;
    /// `[t]`: `StartTracking`.
    pub const TRACK_START: i64 = 16;
    /// `[t]`: `StopTracking`.
    pub const TRACK_STOP: i64 = 17;
    /// `[t]`: `IsTracking`.
    pub const TRACK_IS: i64 = 18;
    /// `[t]`: `GetRmList`, `GetAddList`, `GetChgList`, `GetNotifyEvents`.
    pub const TRACK_LISTS: i64 = 19;
    /// `[t]`: `ReSetArrays`.
    pub const TRACK_RESET: i64 = 20;
    /// `[t]`: delete the tracker.
    pub const TRACK_DROP: i64 = 21;
    /// `[on]`: install (1) or clear (0) a foreign system notify callback.
    pub const SYS_FOREIGN: i64 = 22;
    /// `[o]`: delete the object.
    pub const DESTROY: i64 = 23;
    /// `[o, type]`: give the object a stand-in type code (`SmTopology` storage) so
    /// the change tracker's `IsKindOf` classification can be exercised: 16001
    /// `SmTopology`, 16002 `SmFace`, 16005 `SmEdgeuse`, 16016 `SmEdge`, 16021
    /// `SmVertex`.
    pub const SET_KIND: i64 = 24;
    /// Largest op code.
    pub const LAST: i64 = 24;
}

/// Largest object pool.
pub const MAX_OBJECTS: i64 = 32;

/// Raw script output: the integer and double streams.
#[derive(Debug, Clone, PartialEq)]
pub struct RawOutput {
    /// Per op `[status, n, values...]`, then the final state dump.
    pub ints: Vec<i64>,
    /// Tolerances, in op order.
    pub dbls: Vec<f64>,
}

fn validate(n_objs: i64, ops: &[[i64; 8]]) -> Result<(), FfiError> {
    if !(1..=MAX_OBJECTS).contains(&n_objs) {
        return Err(FfiError::InvalidArgument(format!("n_objs {n_objs}")));
    }
    for o in ops {
        if !(0..=op::LAST).contains(&o[0]) {
            return Err(FfiError::InvalidArgument(format!("op {}", o[0])));
        }
    }
    Ok(())
}

/// Runs a script over `n_objs` fresh `SmTopology` objects in a fresh context, with
/// the recording user notify callback when `user_cb`. `kind_ts` are the type codes
/// asked of `IsKindOf` in the final dump.
pub fn run(
    n_objs: i64,
    user_cb: bool,
    kind_ts: &[i64],
    ops: &[[i64; 8]],
) -> Result<RawOutput, FfiError> {
    validate(n_objs, ops)?;
    let flat: Vec<i64> = ops.iter().flatten().copied().collect();
    let mut ints_cap = 4096 + 64 * ops.len();
    let mut dbls_cap = 16 + ops.len();
    for _ in 0..2 {
        let mut ints = vec![0i64; ints_cap];
        let mut dbls = vec![0f64; dbls_cap];
        let (mut ints_len, mut dbls_len) = (0i64, 0i64);
        // SAFETY: `kind_ts` and `flat` (8*ops.len()) are valid for their lengths;
        // `ints`/`dbls` have the capacities passed (the shim checks them before
        // writing); the length out-params are valid. The pool size and op codes were
        // validated above; the shim checks object, tracker and enum arguments itself.
        // Nothing is retained.
        let st = unsafe {
            sys::sc_topology_topology_run(
                n_objs,
                i32::from(user_cb),
                kind_ts.as_ptr(),
                kind_ts.len() as i64,
                flat.as_ptr(),
                ops.len() as i64,
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
