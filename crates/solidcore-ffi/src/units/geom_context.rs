//! FFI wrappers for unit `geom.context`.
//!
//! Shim: `harness/cpp/shim/geom_context.cpp`.
//! Upstream: `source/SMLib/inc/SmContext.h`, `source/SMLib/src/SmContext.cpp`
//! (`SmContext`, `SmNewMarkAndLock`), the `SmContext` overloads of `SmTol`,
//! `SmMassPropertyBehavior` (`SmTopoTypes.h`) and `SmApiCreateContext` /
//! `SmApiGetOrCreateContext` (`SmApiGeneral.cpp`).
//!
//! | Rust       | shim                     | upstream                                   |
//! |------------|--------------------------|--------------------------------------------|
//! | [`script`] | `sc_geom_context_script` | a script of `SmContext` / `SmNewMarkAndLock` calls |

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_geom_context_script(
            sys_model: f64,
            sys_ratio: f64,
            ctx_params: *const f64,
            n_ctx: i64,
            ops: *const i64,
            xs: *const f64,
            n_ops: i64,
            n_slots: i64,
            out: *mut i64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Script operation codes (field use in brackets; see the shim for details).
pub mod op {
    /// `GetCurrentMark(m)` [ctx, m].
    pub const CUR_MARK: i64 = 0;
    /// `IsMarkLocked(m)` [ctx, m].
    pub const IS_LOCKED: i64 = 1;
    /// `slot = new SmNewMarkAndLock(ctx, mask m)` [slot, ctx, m].
    pub const LOCK_NEW: i64 = 2;
    /// `slot = new SmNewMarkAndLock(ctx, ctx2)` [slot, ctx, ctx2].
    pub const LOCK_PAIR: i64 = 3;
    /// `slot = new SmNewMarkAndLock()` [slot].
    pub const LOCK_DEFAULT: i64 = 4;
    /// `slot->NewMark()` when bound [slot].
    pub const LOCK_NEWMARK: i64 = 5;
    /// `slot->SetContext(ctx, m)` [slot, ctx, m].
    pub const LOCK_SETCTX: i64 = 6;
    /// `slot->SetContext(ctx, ctx2)` [slot, ctx, ctx2].
    pub const LOCK_SETCTX_PAIR: i64 = 7;
    /// `delete slot` [slot].
    pub const LOCK_DROP: i64 = 8;
    /// `GetMarkType*`, `GetContext*` [slot].
    pub const LOCK_INFO: i64 = 9;
    /// `SetDoingBoolean(m)` or `GetDoingBooleanRef() = m` [ctx, m, flag].
    pub const DOING_BOOL: i64 = 10;
    /// `GetThisModelSizeEstimate()`, `SmTol::GetModelSizeEstimate(ctx)` [ctx].
    pub const GET_MODEL: i64 = 11;
    /// `SetThisModelSizeEstimate(x0)` or `SmTol::SetModelSizeEstimate(ctx, x0)` [ctx, flag].
    pub const SET_MODEL: i64 = 12;
    /// `SetThisLargeSmallSizeRatio(x0)` [ctx].
    pub const SET_RATIO: i64 = 13;
    /// `SmTol::GetZoneTol3d/GetApproxTol3d/GetXSectTol3d(ctx, m)`, `CalcFaceZoneTol3d(ctx)` [ctx|-1, m].
    pub const TOL_CTX: i64 = 14;
    /// `SmTol::CalcEdgeZoneTol3d(x0, x1, flag ? &x2 : NULL, ctx)` [ctx|-1, flag].
    pub const CALC_EDGE: i64 = 15;
    /// `SmTol::CalcVertexZoneTol3d(x0, x1, flag ? &x2 : NULL, ctx)` [ctx|-1, flag].
    pub const CALC_VERTEX: i64 = 16;
    /// `SetSysNotifyCallback` (flag 0) / `SetUserNotifyCallback` (flag 1), non-null if m [ctx, m, flag].
    pub const CALLBACK: i64 = 17;
    /// `SmMassPropertyBehavior(x0..x3, m, flag)`, or the default one if slot != 0.
    pub const MASS: i64 = 18;
    /// `SmApiCreateContext(); SmApiGetOrCreateContext()`.
    pub const API: i64 = 19;
    /// One past the last code.
    pub const COUNT: i64 = 20;
}

/// The `SmMarkType` values.
pub const MARK_TYPES: [i64; 7] = [0, 1, 2, 4, 8, 16, 31];

/// Maximum number of contexts in a script.
pub const MAX_CONTEXTS: usize = 8;
/// Maximum number of lock slots in a script.
pub const MAX_SLOTS: usize = 16;

/// One script operation.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Step {
    /// Operation code ([`op`]).
    pub op: i64,
    /// Lock slot (or the "default" switch of [`op::MASS`]).
    pub slot: i64,
    /// Context index (`-1` = null where allowed).
    pub ctx: i64,
    /// Second context index.
    pub ctx2: i64,
    /// Mark type / mask / small-topology type / boolean / count operand.
    pub m: i64,
    /// Variant switch.
    pub flag: i64,
    /// Float operands.
    pub x: [f64; 4],
}

/// One output item.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Item {
    /// An integer result.
    Int(i64),
    /// A floating-point result.
    Float(f64),
}

/// Checks a script the way the shim does (anything it would reject or crash on).
pub fn validate(n_ctx: usize, n_slots: usize, steps: &[Step]) -> Result<(), FfiError> {
    let bad = |m: &str| Err(FfiError::InvalidArgument(m.into()));
    if !(1..=MAX_CONTEXTS).contains(&n_ctx) {
        return bad("context count");
    }
    if n_slots > MAX_SLOTS {
        return bad("slot count");
    }
    let n_ctx = n_ctx as i64;
    let n_slots = n_slots as i64;
    for s in steps {
        let o = s.op;
        if !(0..op::COUNT).contains(&o) {
            return bad("op code");
        }
        let need_slot = (op::LOCK_NEW..=op::LOCK_INFO).contains(&o);
        let null_ok = matches!(o, op::TOL_CTX | op::CALC_EDGE | op::CALC_VERTEX);
        let no_ctx = matches!(
            o,
            op::LOCK_DEFAULT
                | op::LOCK_NEWMARK
                | op::LOCK_DROP
                | op::LOCK_INFO
                | op::MASS
                | op::API
        );
        if need_slot && !(0..n_slots).contains(&s.slot) {
            return bad("slot index");
        }
        let ctx_ok = (0..n_ctx).contains(&s.ctx) || (null_ok && s.ctx == -1);
        if !no_ctx && !ctx_ok {
            return bad("context index");
        }
        if matches!(o, op::LOCK_PAIR | op::LOCK_SETCTX_PAIR) && !(0..n_ctx).contains(&s.ctx2) {
            return bad("second context index");
        }
        if matches!(o, op::CUR_MARK | op::IS_LOCKED | op::LOCK_SETCTX) && !MARK_TYPES.contains(&s.m)
        {
            return bad("mark type");
        }
        if o == op::DOING_BOOL && !(0..=1).contains(&s.m) {
            return bad("boolean");
        }
        if o == op::TOL_CTX && !(0..=4).contains(&s.m) {
            return bad("small topology type");
        }
        if o == op::MASS && (s.m < 0 || s.flag < 0) {
            return bad("behavior");
        }
    }
    Ok(())
}

/// Runs `steps` in upstream C++ on `contexts` (model size, ratio) with the system
/// tolerances set from (`sys_model`, `sys_ratio`) and reset afterwards. Returns per
/// step its results and a snapshot of every context, then a final snapshot after the
/// live locks are destroyed in slot order.
pub fn script(
    sys_model: f64,
    sys_ratio: f64,
    contexts: &[(f64, f64)],
    n_slots: usize,
    steps: &[Step],
) -> Result<Vec<Item>, FfiError> {
    validate(contexts.len(), n_slots, steps)?;
    let params: Vec<f64> = contexts.iter().flat_map(|&(a, b)| [a, b]).collect();
    let ops: Vec<i64> = steps
        .iter()
        .flat_map(|s| [s.op, s.slot, s.ctx, s.ctx2, s.m, s.flag])
        .collect();
    let xs: Vec<f64> = steps.iter().flat_map(|s| s.x).collect();
    let raw = call_with_buffer(1024, |buf: &mut [i64], len| {
        // SAFETY: params/ops/xs hold 2*n_ctx, 6*n_ops and 4*n_ops values; the script
        // was validated above; buf is valid for buf.len() writes and len for one.
        unsafe {
            sys::sc_geom_context_script(
                sys_model,
                sys_ratio,
                params.as_ptr(),
                contexts.len() as i64,
                ops.as_ptr(),
                xs.as_ptr(),
                steps.len() as i64,
                n_slots as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })?;
    raw.chunks(2)
        .map(|c| match c {
            [0, v] => Ok(Item::Int(*v)),
            [1, v] => Ok(Item::Float(f64::from_bits(*v as u64))),
            _ => Err(FfiError::InvalidArgument("malformed shim output".into())),
        })
        .collect()
}
