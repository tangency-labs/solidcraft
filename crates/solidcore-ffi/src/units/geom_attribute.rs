//! FFI wrappers for unit `geom.attribute`.
//!
//! Shim: `harness/cpp/shim/geom_attribute.cpp`.
//! Upstream: `source/SMLib/src/SmAttribute.cpp` (`SmAttribute` and derived classes,
//! `SmAttributeData::ReadIndexedAttributesFromDB`) driven through the `SmAObject`
//! attribute-list calls.
//!
//! | Rust    | shim                    | upstream                                     |
//! |---------|-------------------------|----------------------------------------------|
//! | [`run`] | `sc_geom_attribute_run` | a script of attribute / attribute-list calls |

use crate::{check, FfiError, SC_ERR_BUFFER};

mod sys {
    extern "C" {
        pub fn sc_geom_attribute_run(
            n_objects: i64,
            ops: *const i64,
            n_ops: i64,
            payload_i: *const i64,
            n_payload_i: i64,
            payload_d: *const f64,
            n_payload_d: i64,
            data: *const u8,
            data_len: i64,
            file_type: i32,
            byte_swap: i32,
            ints: *mut i64,
            ints_cap: i64,
            ints_len: *mut i64,
            dbls: *mut f64,
            dbls_cap: i64,
            dbls_len: *mut i64,
        ) -> i32;
    }
}

/// Script op codes (must match the shim's `enum Op`). Arguments in brackets are
/// `a..f` of the 7-wide op record `[op, a, b, c, d, e, f]`; `ref` is an attribute
/// reference (see the shim header).
pub mod op {
    /// `new SmAttribute(id, beh)` `[id, beh]`.
    pub const NEW_PLAIN: i64 = 0;
    /// `new SmGenericAttribute` `[id, beh, n_longs, n_doubles, n_chars]` (payload).
    pub const NEW_GENERIC: i64 = 1;
    /// `new SmLongAttribute` `[id, value, beh]`.
    pub const NEW_LONG: i64 = 2;
    /// `new SmPointerAttribute` `[id, pointer, beh]`.
    pub const NEW_POINTER: i64 = 3;
    /// `new SmPointerListAttribute` `[id, beh, n, null_list]` (payload).
    pub const NEW_POINTER_LIST: i64 = 4;
    /// `new SmVector3dAttribute` `[id, beh]` (3 payload doubles).
    pub const NEW_VECTOR3D: i64 = 5;
    /// `new SmColorAttribute` `[id, beh]` (3 payload doubles).
    pub const NEW_COLOR: i64 = 6;
    /// `new SmTagAttribute` `[id, beh]` (8 payload longs).
    pub const NEW_TAG: i64 = 7;
    /// `new SmMergeRegionAttribute` `[this_obj, this_type, other_obj, other_type]`.
    pub const NEW_MERGE_REGION: i64 = 8;
    /// `MakeCopy` `[ref]`.
    pub const MAKE_COPY: i64 = 9;
    /// `obj->AddAttribute` `[obj, ref]`.
    pub const ADD_ATTRIBUTE: i64 = 10;
    /// `obj->RemoveAttribute` `[obj, ref, do_not_delete]`.
    pub const REMOVE_ATTRIBUTE: i64 = 11;
    /// `obj->ClearAttributes` `[obj]`.
    pub const CLEAR_ATTRIBUTES: i64 = 12;
    /// `Split` `[ref, split, child1, child2]`.
    pub const SPLIT: i64 = 13;
    /// `Copy` `[ref, from, to]`.
    pub const COPY: i64 = 14;
    /// `Merge` `[ref, orig1, orig2, merged]`.
    pub const MERGE: i64 = 15;
    /// `Coincident` `[ref, from, to]`.
    pub const COINCIDENT: i64 = 16;
    /// `Edit` `[ref, obj]`.
    pub const EDIT: i64 = 17;
    /// `Destruction` `[ref, obj]`.
    pub const DESTRUCTION: i64 = 18;
    /// `PropagateRegionsThroughBooleans` `[ref, orig1, orig2, merged]`.
    pub const PROPAGATE: i64 = 19;
    /// `AddUser` `[ref, obj]`.
    pub const ADD_USER: i64 = 20;
    /// `RemoveUser` `[ref, obj, do_not_delete]`.
    pub const REMOVE_USER: i64 = 21;
    /// `GetUsers` `[ref]`.
    pub const GET_USERS: i64 = 22;
    /// `GetType`, `IsKindOf`, `IsPropagatedThroughBooleans`, id, behaviour,
    /// `GetTypeString` `[ref]`.
    pub const QUERY: i64 = 23;
    /// Merge-region predicates and accessors `[ref, solid_flag]`.
    pub const MR_QUERY: i64 = 24;
    /// `SetRegions` `[ref, index, this_obj, this_type, other_obj, other_type]`.
    pub const MR_SET_REGIONS: i64 = 25;
    /// `SetOperation` `[ref, operation]`.
    pub const MR_SET_OPERATION: i64 = 26;
    /// `SmVector3dAttribute::SetValue` `[ref]` (3 payload doubles).
    pub const SET_VECTOR: i64 = 27;
    /// `delete` `[ref]`.
    pub const DESTROY: i64 = 28;
    /// `SmAttributeData::ReadIndexedAttributesFromDB` `[]`.
    pub const READ_INDEXED: i64 = 29;
    /// `obj->FindAttribute` `[obj, id]` (position in the object's list or -1).
    pub const FIND: i64 = 30;
    /// `fromClearAttributes` `[ref, obj]`.
    pub const FROM_CLEAR: i64 = 31;
}

/// Most objects a script may use.
pub const MAX_OBJECTS: i64 = 64;

/// The stream `READ_INDEXED` reads from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stream<'a> {
    /// The data.
    pub data: &'a [u8],
    /// `SmFileType` (0 ASCII, 1 binary, 2 byte-swapped, 3 unknown).
    pub file_type: i32,
    /// `SetByteSwap` value.
    pub byte_swap: bool,
}

/// Raw result of [`run`]: the shim's int and double output streams (layout in the
/// shim header).
#[derive(Debug, Clone, PartialEq)]
pub struct RawOutput {
    /// Integer stream.
    pub ints: Vec<i64>,
    /// Double stream.
    pub dbls: Vec<f64>,
}

fn behavior_ok(b: i64) -> bool {
    (0..=4).contains(&b)
}

/// Rejects ASCII data whose `>> ULONG` extractions could exceed the shim's read cap:
/// any `-` (negation wraps modulo 2^64) or a digit run longer than 6.
fn ascii_ok(data: &[u8]) -> bool {
    let mut run = 0;
    for &c in data {
        if c == b'-' {
            return false;
        }
        if c.is_ascii_digit() {
            run += 1;
            if run > 6 {
                return false;
            }
        } else {
            run = 0;
        }
    }
    true
}

/// Runs a script of 7-wide op records against `n_objects` fresh objects.
pub fn run(
    n_objects: i64,
    ops: &[[i64; 7]],
    payload_i: &[i64],
    payload_d: &[f64],
    stream: Option<&Stream<'_>>,
) -> Result<RawOutput, FfiError> {
    if !(0..=MAX_OBJECTS).contains(&n_objects) {
        return Err(FfiError::InvalidArgument(format!("n_objects {n_objects}")));
    }
    let n_pi = payload_i.len() as i64;
    let n_pd = payload_d.len() as i64;
    for o in ops {
        let [code, _, b, c, d, e, _] = *o;
        let beh = match code {
            op::NEW_PLAIN
            | op::NEW_GENERIC
            | op::NEW_POINTER_LIST
            | op::NEW_VECTOR3D
            | op::NEW_COLOR
            | op::NEW_TAG => Some(b),
            op::NEW_LONG | op::NEW_POINTER => Some(c),
            op::MAKE_COPY..=op::FROM_CLEAR | op::NEW_MERGE_REGION => None,
            _ => return Err(FfiError::InvalidArgument(format!("op {code}"))),
        };
        if beh.is_some_and(|x| !behavior_ok(x)) {
            return Err(FfiError::InvalidArgument(format!("behavior in op {code}")));
        }
        let counts_ok = match code {
            op::NEW_GENERIC => {
                (0..=n_pi).contains(&c) && (0..=n_pd).contains(&d) && (0..=n_pi).contains(&e)
            }
            op::NEW_POINTER_LIST => (0..=n_pi).contains(&c),
            _ => true,
        };
        if !counts_ok {
            return Err(FfiError::InvalidArgument(format!(
                "payload count in op {code}"
            )));
        }
    }
    let (data, file_type, byte_swap) = if let Some(s) = stream {
        if !(0..=3).contains(&s.file_type) {
            return Err(FfiError::InvalidArgument(format!(
                "file_type {}",
                s.file_type
            )));
        }
        if s.file_type == 0 && !ascii_ok(s.data) {
            return Err(FfiError::InvalidArgument("ascii data out of range".into()));
        }
        (s.data, s.file_type, i32::from(s.byte_swap))
    } else {
        if ops.iter().any(|o| o[0] == op::READ_INDEXED) {
            return Err(FfiError::InvalidArgument(
                "READ_INDEXED needs a stream".into(),
            ));
        }
        (&[][..], -1, 0)
    };
    let flat: Vec<i64> = ops.iter().flatten().copied().collect();
    let mut ints_cap = 4096 + 64 * ops.len() + 8 * payload_i.len() + 8 * data.len();
    let mut dbls_cap = 1024 + 8 * ops.len() + 2 * payload_d.len();
    for _ in 0..2 {
        let mut ints = vec![0i64; ints_cap];
        let mut dbls = vec![0f64; dbls_cap];
        let (mut ints_len, mut dbls_len) = (0i64, 0i64);
        // SAFETY: `flat` holds 7*ops.len() values; the payload and data slices are
        // valid for their lengths; `ints`/`dbls` have the capacities passed (the shim
        // checks them before writing); the length out-params are valid i64s. Op
        // codes, behaviours and payload counts were validated above, the shim checks
        // object indices and attribute references itself, and ASCII data cannot
        // produce counts above the shim's read cap. Nothing is retained.
        let st = unsafe {
            sys::sc_geom_attribute_run(
                n_objects,
                flat.as_ptr(),
                ops.len() as i64,
                payload_i.as_ptr(),
                n_pi,
                payload_d.as_ptr(),
                n_pd,
                data.as_ptr(),
                data.len() as i64,
                file_type,
                byte_swap,
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
