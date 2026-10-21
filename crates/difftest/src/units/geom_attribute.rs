//! Runner for unit `geom.attribute`: attributes, their user lists and propagation.
//!
//! C++: `solidcore_ffi::units::geom_attribute` (upstream `SmAttribute` and derived
//! classes, driven through `SmAObject` attribute lists on bare `SmRegion` objects).
//! Rust: `solidcore::geom::attribute`.
//!
//! Each case is a *script* over `n_objects` attributed objects and a table of
//! attribute handles (every attribute the script creates, every `MakeCopy` result
//! and every placeholder `ReadIndexedAttributesFromDB` creates, in that order). Ops
//! are JSON arrays `["name", args...]`; creation data rides inline as nested arrays
//! (`["new_vector3d", id, behavior, [x, y, z]]`). Attribute references: `r >= 0` is
//! handle `r`; `r < 0` is entry `k % 1024` of object `k / 1024`'s list
//! (`k = -r - 1`). Both sides report per op a status and its outputs, then the final
//! state: which handles are live, every reachable attribute (live handles first,
//! then object lists in order) with class, type code, id, behaviour, users and data,
//! every object's attribute list, the placeholder table and the stream state.
//!
//! Statuses: `0` ok, `-1` the reference does not resolve, `-2` skipped because the
//! op would be undefined behaviour upstream, else the upstream status. The skip rules
//! are evaluated by each side on its own state:
//! - Safety invariant: the objects listing an attribute are a sub-multiset of its
//!   users (every op below keeps it). Under it, `RemoveAttribute` and
//!   `ClearAttributes` never delete an attribute another object still lists.
//! - `remove_user` / `from_clear` (direct `RemoveUser` / `fromClearAttributes`):
//!   skipped while any object lists the attribute.
//! - `destroy` (`delete`): skipped unless the attribute has no users, or is standalone
//!   and its users equal its listings (else `~SmAttribute` deletes twice or leaves a
//!   dangling list entry).
//! - `merge` on a merge-region attribute: skipped when an `SM_AI_OTHER_REGION_ID`
//!   attribute of another class is found, or both inputs have one and the result does
//!   not (upstream casts / dereferences NULL).
//! - `propagate` on a colour attribute when both inputs have a colour: skipped unless
//!   all three colour attributes are vector attributes.
//! - merge-region ops on other classes, `set_vector` on non-vector attributes,
//!   `mr_set_regions` with an index valid only for the "this" list: skipped.
//! - `read_indexed`: skipped unless the stream is good and (ASCII) has a
//!   non-whitespace byte left, or (binary) holds a whole record with count and
//!   indices `<= 2^20` (upstream reads uninitialised values otherwise).
//!
//! C++ does not expose deletion, so the shim *predicts* which handles die from the
//! state before each deleting op (the Rust side reports its store's real liveness).

use serde_json::Value;

use solidcore::geom::attribute::{
    type_code, Attribute, AttributeHandle, AttributeStore, AttributeValue, Behavior, ObjectKey,
    TagData,
};
use solidcore::topology::database_io::{DatabaseReader, FileType, InputStream};
use solidcore::{SolidError, Vec3};
use solidcore_ffi::units::geom_attribute::{self as ffi, op, Stream};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::output::Topology;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `geom.attribute` runner.
pub struct Runner;

const ST_INVALID: i64 = -1;
const ST_SKIP: i64 = -2;
const REF_STRIDE: i64 = 1024;
const READ_CAP: u64 = 1 << 20;

const KIND_PLAIN: i64 = 0;
const KIND_GENERIC: i64 = 1;
const KIND_LONG: i64 = 2;
const KIND_POINTER: i64 = 3;
const KIND_POINTER_LIST: i64 = 4;
const KIND_VECTOR3D: i64 = 5;
const KIND_MERGE_REGION: i64 = 6;
const KIND_TAG: i64 = 7;
const KIND_COLOR: i64 = 8;

const QUERY_TYPES: [i64; 11] = [
    type_code::OBJECT,
    type_code::ATTRIBUTE,
    type_code::LONG,
    type_code::VECTOR3D,
    type_code::GENERIC,
    type_code::POINTER,
    type_code::POINTER_LIST,
    type_code::MERGE_REGION,
    type_code::TAG,
    type_code::COLOR,
    12000,
];

const OPS: &[(&str, i64)] = &[
    ("new_plain", op::NEW_PLAIN),
    ("new_generic", op::NEW_GENERIC),
    ("new_long", op::NEW_LONG),
    ("new_pointer", op::NEW_POINTER),
    ("new_pointer_list", op::NEW_POINTER_LIST),
    ("new_vector3d", op::NEW_VECTOR3D),
    ("new_color", op::NEW_COLOR),
    ("new_tag", op::NEW_TAG),
    ("new_merge_region", op::NEW_MERGE_REGION),
    ("make_copy", op::MAKE_COPY),
    ("add_attribute", op::ADD_ATTRIBUTE),
    ("remove_attribute", op::REMOVE_ATTRIBUTE),
    ("clear_attributes", op::CLEAR_ATTRIBUTES),
    ("split", op::SPLIT),
    ("copy", op::COPY),
    ("merge", op::MERGE),
    ("coincident", op::COINCIDENT),
    ("edit", op::EDIT),
    ("destruction", op::DESTRUCTION),
    ("propagate", op::PROPAGATE),
    ("add_user", op::ADD_USER),
    ("remove_user", op::REMOVE_USER),
    ("get_users", op::GET_USERS),
    ("query", op::QUERY),
    ("mr_query", op::MR_QUERY),
    ("mr_set_regions", op::MR_SET_REGIONS),
    ("mr_set_operation", op::MR_SET_OPERATION),
    ("set_vector", op::SET_VECTOR),
    ("destroy", op::DESTROY),
    ("read_indexed", op::READ_INDEXED),
    ("find", op::FIND),
    ("from_clear", op::FROM_CLEAR),
    ("set_is_void", op::SET_IS_VOID),
    ("make_infinite", op::MAKE_INFINITE),
];

// ---- script decoding ------------------------------------------------------------

/// A decoded case: op records plus the payload streams they consume.
struct Script {
    n_objects: i64,
    ops: Vec<[i64; 7]>,
    payload_i: Vec<i64>,
    payload_d: Vec<f64>,
    stream: Option<(Vec<u8>, i32, bool)>,
}

fn op_code(name: &str) -> Result<i64, HarnessError> {
    OPS.iter()
        .find(|(n, _)| *n == name)
        .map(|&(_, c)| c)
        .ok_or_else(|| HarnessError::new(format!("unknown op `{name}`")))
}

fn as_int(v: &Value) -> Result<i64, HarnessError> {
    v.as_i64()
        .or_else(|| v.as_u64().map(|u| u as i64))
        .ok_or_else(|| HarnessError::new(format!("expected integer, got {v}")))
}

fn ints_of(v: Option<&Value>) -> Result<Vec<i64>, HarnessError> {
    match v {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(a)) => a.iter().map(as_int).collect(),
        Some(other) => Err(HarnessError::new(format!("expected int list, got {other}"))),
    }
}

fn dbls_of(v: Option<&Value>) -> Result<Vec<f64>, HarnessError> {
    match v {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| {
                x.as_f64()
                    .ok_or_else(|| HarnessError::new(format!("expected number, got {x}")))
            })
            .collect(),
        Some(other) => Err(HarnessError::new(format!(
            "expected number list, got {other}"
        ))),
    }
}

fn decode(v: &Value) -> Result<Script, HarnessError> {
    let n_objects = v
        .get("n_objects")
        .map(as_int)
        .transpose()?
        .ok_or_else(|| HarnessError::new("n_objects"))?;
    let raw_ops = v
        .get("ops")
        .and_then(Value::as_array)
        .ok_or_else(|| HarnessError::new("ops"))?;
    let mut s = Script {
        n_objects,
        ops: Vec::new(),
        payload_i: Vec::new(),
        payload_d: Vec::new(),
        stream: None,
    };
    for o in raw_ops {
        let items = o
            .as_array()
            .ok_or_else(|| HarnessError::new(format!("op {o} is not an array")))?;
        let name = items
            .first()
            .and_then(Value::as_str)
            .ok_or_else(|| HarnessError::new(format!("op {o} has no name")))?;
        let code = op_code(name)?;
        let mut rec = [code, 0, 0, 0, 0, 0, 0];
        let scalars: Vec<&Value> = items
            .iter()
            .skip(1)
            .filter(|x| !x.is_array() && !x.is_null())
            .collect();
        let lists: Vec<&Value> = items
            .iter()
            .skip(1)
            .filter(|x| x.is_array() || x.is_null())
            .collect();
        for (slot, x) in rec.iter_mut().skip(1).zip(&scalars) {
            *slot = as_int(x)?;
        }
        match code {
            op::NEW_GENERIC => {
                let l = ints_of(lists.first().copied())?;
                let d = dbls_of(lists.get(1).copied())?;
                let c = ints_of(lists.get(2).copied())?;
                rec[3] = l.len() as i64;
                rec[4] = d.len() as i64;
                rec[5] = c.len() as i64;
                s.payload_i.extend(l);
                s.payload_d.extend(d);
                s.payload_i.extend(c);
            }
            op::NEW_POINTER_LIST => {
                let first = lists.first().copied();
                let l = ints_of(first)?;
                rec[3] = l.len() as i64;
                rec[4] = i64::from(matches!(first, None | Some(Value::Null)));
                s.payload_i.extend(l);
            }
            op::NEW_VECTOR3D | op::NEW_COLOR | op::SET_VECTOR => {
                let d = dbls_of(lists.first().copied())?;
                if d.len() != 3 {
                    return Err(HarnessError::new("vector payload needs 3 numbers"));
                }
                s.payload_d.extend(d);
            }
            op::NEW_TAG => {
                let l = ints_of(lists.first().copied())?;
                if l.len() != 8 {
                    return Err(HarnessError::new("tag payload needs 8 integers"));
                }
                s.payload_i.extend(l);
            }
            _ => {}
        }
        s.ops.push(rec);
    }
    if let Some(st) = v.get("stream").filter(|x| !x.is_null()) {
        let data = if let Some(t) = st.get("text").and_then(Value::as_str) {
            t.as_bytes().to_vec()
        } else {
            ints_of(st.get("data"))?
                .into_iter()
                .map(|b| b as u8)
                .collect()
        };
        let ft = st.get("file_type").map(as_int).transpose()?.unwrap_or(0) as i32;
        let swap = st
            .get("byte_swap")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        s.stream = Some((data, ft, swap));
    }
    Ok(s)
}

// ---- the shared output layout -----------------------------------------------------

/// Parses the shim's int/double streams (or the Rust side's identical ones) into
/// named outputs.
fn to_output(
    n_objects: i64,
    n_ops: usize,
    ints: &[i64],
    dbls: &[f64],
) -> Result<Output, HarnessError> {
    let mut it = ints.iter().copied();
    let mut next = || {
        it.next()
            .ok_or_else(|| HarnessError::new("truncated output"))
    };
    let take = |n: i64, next: &mut dyn FnMut() -> Result<i64, HarnessError>| {
        (0..n.max(0))
            .map(|_| next())
            .collect::<Result<Vec<i64>, _>>()
    };
    let mut status = Vec::new();
    let mut outs = Vec::new();
    for _ in 0..n_ops {
        status.push(next()?);
        let n = next()?;
        outs.push(take(n, &mut next)?);
    }
    let n_handles = next()?;
    let alive = take(n_handles, &mut next)?;
    let canon = take(n_handles, &mut next)?;
    let n_attrs = next()?;
    let mut attrs = Vec::new();
    let mut dbl_counts = Vec::new();
    for _ in 0..n_attrs {
        let mut rec = take(4, &mut next)?;
        let n_users = next()?;
        rec.push(n_users);
        rec.extend(take(n_users, &mut next)?);
        let n_vals = next()?;
        rec.push(n_vals);
        rec.extend(take(n_vals, &mut next)?);
        let n_d = next()?;
        rec.push(n_d);
        dbl_counts.push(n_d);
        attrs.push(rec);
    }
    let lists = (0..n_objects)
        .map(|_| {
            let n = next()?;
            take(n, &mut next)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let n_all = next()?;
    let all = take(n_all, &mut next)?;
    let stream = take(2, &mut next)?;
    let total_d: i64 = dbl_counts.iter().sum();
    if total_d as usize != dbls.len() {
        return Err(HarnessError::new("double stream length mismatch"));
    }
    let bits: Vec<i64> = dbls.iter().map(|d| d.to_bits() as i64).collect();
    Ok(Output::new()
        .ints("status", status)
        .topology(
            Topology::new()
                .adjacency("op_outputs", outs)
                .adjacency("attributes", attrs)
                .adjacency("object_lists", lists),
        )
        .ints("handle_alive", alive)
        .ints("handle_canon", canon)
        .ints("placeholders", all)
        .ints("stream", stream)
        .ints("attr_dbl_bits", bits)
        .values("attr_dbls", dbls.to_vec()))
}

// ---- C++ side -------------------------------------------------------------------

fn run_cpp_case(v: &Value) -> Result<Output, HarnessError> {
    let s = decode(v)?;
    let stream = s.stream.as_ref().map(|(d, ft, swap)| Stream {
        data: d,
        file_type: *ft,
        byte_swap: *swap,
    });
    match ffi::run(
        s.n_objects,
        &s.ops,
        &s.payload_i,
        &s.payload_d,
        stream.as_ref(),
    ) {
        Ok(raw) => to_output(s.n_objects, s.ops.len(), &raw.ints, &raw.dbls),
        // the shim checks object indices and op operands itself (SC_ERR_ARG): a
        // guard rejection like the wrapper's, as the Rust side's harness errors
        Err(e @ (FfiError::InvalidArgument(_) | FfiError::Status(solidcore_ffi::SC_ERR_ARG))) => {
            Err(HarnessError::new(format!("ffi rejected: {e}")))
        }
        Err(e) => Ok(Output::err(e.to_string())),
    }
}

// ---- Rust side ------------------------------------------------------------------

struct RustRun {
    store: AttributeStore,
    regions: crate::fixtures::regions::RegionScene,
    n_objects: i64,
    handles: Vec<AttributeHandle>,
    all: Vec<Option<AttributeHandle>>,
    reader: Option<DatabaseReader>,
    data: Vec<u8>,
    pay_i: std::vec::IntoIter<i64>,
    pay_d: std::vec::IntoIter<f64>,
}

/// Outcome of one op: a status, or the case is deferred.
enum Step {
    Status(i64),
    Deferred(Output),
}

fn kind(a: &Attribute) -> i64 {
    match a.value() {
        AttributeValue::None
        | AttributeValue::PolyToEuVu { .. }
        | AttributeValue::PolyEdgeToVertexUse { .. }
        | AttributeValue::DecimateVertex(_)
        | AttributeValue::QuadricVertex(_)
        | AttributeValue::QuadricEdge(_) => KIND_PLAIN,
        AttributeValue::Generic { .. } => KIND_GENERIC,
        AttributeValue::Long(_) => KIND_LONG,
        AttributeValue::Pointer(_) => KIND_POINTER,
        AttributeValue::PointerList(_) => KIND_POINTER_LIST,
        AttributeValue::Vector3d(_) => KIND_VECTOR3D,
        AttributeValue::MergeRegion(_) => KIND_MERGE_REGION,
        AttributeValue::Tag(_) => KIND_TAG,
        AttributeValue::Color(_) => KIND_COLOR,
    }
}

fn behavior(code: i64) -> Result<Behavior, HarnessError> {
    Behavior::from_code(code as i32).map_err(|e| HarnessError::new(e.to_string()))
}

fn harness(e: &SolidError) -> HarnessError {
    HarnessError::new(format!("unexpected Rust error: {e}"))
}

fn is_c_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

impl RustRun {
    fn obj(&self, i: i64) -> Result<ObjectKey, HarnessError> {
        if (0..self.n_objects).contains(&i) {
            Ok(ObjectKey(i as u64))
        } else {
            Err(HarnessError::new(format!("object {i} out of range")))
        }
    }

    fn attr(&self, h: AttributeHandle) -> Result<&Attribute, HarnessError> {
        self.store
            .get(h)
            .ok_or_else(|| HarnessError::new("dead attribute reached"))
    }

    fn resolve(&self, r: i64) -> Option<AttributeHandle> {
        if r >= 0 {
            let h = *self.handles.get(usize::try_from(r).ok()?)?;
            return self.store.is_live(h).then_some(h);
        }
        let k = -(r + 1);
        let (o, i) = (k / REF_STRIDE, k % REF_STRIDE);
        if !(0..self.n_objects).contains(&o) {
            return None;
        }
        self.store
            .attributes(ObjectKey(o as u64))
            .get(usize::try_from(i).ok()?)
            .copied()
    }

    fn listings(&self, h: AttributeHandle) -> Vec<ObjectKey> {
        let mut v = Vec::new();
        for o in 0..self.n_objects {
            let key = ObjectKey(o as u64);
            for x in self.store.attributes(key) {
                if x == h {
                    v.push(key);
                }
            }
        }
        v
    }

    fn add_handle(&mut self, a: Attribute, out: &mut Vec<i64>) -> Result<(), HarnessError> {
        let h = self.store.insert(a).map_err(|e| harness(&e))?;
        self.handles.push(h);
        out.push(self.handles.len() as i64 - 1);
        Ok(())
    }

    fn next_i(&mut self) -> Result<i64, HarnessError> {
        self.pay_i
            .next()
            .ok_or_else(|| HarnessError::new("payload exhausted"))
    }

    fn next_d(&mut self) -> Result<f64, HarnessError> {
        self.pay_d
            .next()
            .ok_or_else(|| HarnessError::new("payload exhausted"))
    }

    fn next_vec(&mut self) -> Result<Vec3, HarnessError> {
        Ok(Vec3::new(self.next_d()?, self.next_d()?, self.next_d()?))
    }

    fn read_is_safe(&self) -> bool {
        let Some(r) = &self.reader else {
            return false;
        };
        let Some(s) = r.in_stream() else {
            return false;
        };
        if !s.good() {
            return false;
        }
        let pos = s.position();
        let rest = self.data.get(pos..).unwrap_or(&[]);
        if r.file_type() == FileType::Ascii {
            return rest.iter().any(|&c| !is_c_space(c));
        }
        let load = |at: usize| -> Option<u64> {
            let b: [u8; 8] = rest.get(at..at + 8)?.try_into().ok()?;
            let v = u64::from_le_bytes(b);
            Some(if r.byte_swap() { v.swap_bytes() } else { v })
        };
        let Some(n) = load(0) else {
            return false;
        };
        if n > READ_CAP || (rest.len() - 8) / 8 < n as usize {
            return false;
        }
        (0..n as usize).all(|i| load(8 + 8 * i).is_some_and(|x| x <= READ_CAP))
    }

    /// Maps a library result to a status (Unported defers the case).
    fn status(res: solidcore::Result<()>) -> Result<Step, HarnessError> {
        match res {
            Ok(()) => Ok(Step::Status(0)),
            Err(e) => {
                if let Some(d) = Output::unported(&e) {
                    return Ok(Step::Deferred(d));
                }
                match e {
                    SolidError::Upstream { code, .. } => Ok(Step::Status(i64::from(code))),
                    other => Err(harness(&other)),
                }
            }
        }
    }

    fn run_op(&mut self, a: [i64; 7], out: &mut Vec<i64>) -> Result<Step, HarnessError> {
        let code = a[0];
        match code {
            op::NEW_PLAIN => {
                self.add_handle(Attribute::new(a[1] as u64, behavior(a[2])?), out)?;
                return Ok(Step::Status(0));
            }
            op::NEW_GENERIC => {
                let l = (0..a[3])
                    .map(|_| self.next_i())
                    .collect::<Result<Vec<_>, _>>()?;
                let d = (0..a[4])
                    .map(|_| self.next_d())
                    .collect::<Result<Vec<_>, _>>()?;
                let c = (0..a[5])
                    .map(|_| self.next_i().map(|x| x as u8))
                    .collect::<Result<Vec<_>, _>>()?;
                self.add_handle(
                    Attribute::generic(a[1] as u64, behavior(a[2])?, &l, &d, &c),
                    out,
                )?;
                return Ok(Step::Status(0));
            }
            op::NEW_LONG => {
                self.add_handle(Attribute::long(a[1] as u64, a[2], behavior(a[3])?), out)?;
                return Ok(Step::Status(0));
            }
            op::NEW_POINTER => {
                self.add_handle(
                    Attribute::pointer(a[1] as u64, a[2] as u64, behavior(a[3])?),
                    out,
                )?;
                return Ok(Step::Status(0));
            }
            op::NEW_POINTER_LIST => {
                let l = (0..a[3])
                    .map(|_| self.next_i().map(|x| x as u64))
                    .collect::<Result<Vec<_>, _>>()?;
                let list = if a[4] != 0 { None } else { Some(&l[..]) };
                self.add_handle(
                    Attribute::pointer_list(a[1] as u64, list, behavior(a[2])?),
                    out,
                )?;
                return Ok(Step::Status(0));
            }
            op::NEW_VECTOR3D | op::NEW_COLOR => {
                let v = self.next_vec()?;
                let b = behavior(a[2])?;
                let at = if code == op::NEW_COLOR {
                    Attribute::color(a[1] as u64, v, b)
                } else {
                    Attribute::vector3d(a[1] as u64, v, b)
                };
                self.add_handle(at, out)?;
                return Ok(Step::Status(0));
            }
            op::NEW_TAG => {
                let mut t = [0i64; 8];
                for x in &mut t {
                    *x = self.next_i()?;
                }
                self.add_handle(
                    Attribute::tag(a[1] as u64, TagData::from_array(t), behavior(a[2])?),
                    out,
                )?;
                return Ok(Step::Status(0));
            }
            op::SET_IS_VOID => {
                let o = self.obj(a[1])?;
                let value = i32::try_from(a[2])
                    .ok()
                    .filter(|v| (0..=2).contains(v))
                    .ok_or_else(|| HarnessError::new("void state"))?;
                self.regions.set_is_void(o, value).map_err(|e| harness(&e))?;
                return Ok(Step::Status(0));
            }
            op::MAKE_INFINITE => {
                let o = self.obj(a[1])?;
                // Attributes made earlier would keep naming the replaced object.
                if !self.handles.is_empty() || !self.all.is_empty() {
                    return Ok(Step::Status(ST_SKIP));
                }
                self.regions.make_infinite(o).map_err(|e| harness(&e))?;
                return Ok(Step::Status(0));
            }
            op::NEW_MERGE_REGION => {
                let (r1, r2) = (self.obj(a[1])?, self.obj(a[3])?);
                self.add_handle(
                    Attribute::merge_region(r1, a[2] as u64 as i64, r2, a[4] as u64 as i64),
                    out,
                )?;
                return Ok(Step::Status(0));
            }
            op::READ_INDEXED => {
                if !self.read_is_safe() {
                    return Ok(Step::Status(ST_SKIP));
                }
                let before = self.all.clone();
                let reader = self
                    .reader
                    .as_mut()
                    .ok_or_else(|| HarnessError::new("no stream"))?;
                let res = self
                    .store
                    .read_indexed_attributes_from_db(&mut self.all, reader);
                for (i, slot) in self.all.iter().enumerate() {
                    if let Some(h) = slot {
                        if before.get(i).copied().flatten().is_none() {
                            self.handles.push(*h);
                        }
                    }
                }
                return match res {
                    Ok(idx) => {
                        out.extend(idx.into_iter().map(|x| x as i64));
                        Ok(Step::Status(0))
                    }
                    Err(e) => Self::status(Err(e)),
                };
            }
            op::FIND => {
                let o = self.obj(a[1])?;
                let f = self.store.find_attribute(o, a[2] as u64);
                let list = self.store.attributes(o);
                out.push(
                    f.and_then(|f| list.iter().position(|&x| x == f))
                        .map_or(-1, |p| p as i64),
                );
                return Ok(Step::Status(0));
            }
            op::CLEAR_ATTRIBUTES => {
                let o = self.obj(a[1])?;
                return Self::status(self.store.clear_attributes(o));
            }
            op::ADD_ATTRIBUTE | op::REMOVE_ATTRIBUTE => {
                let o = self.obj(a[1])?;
                let Some(h) = self.resolve(a[2]) else {
                    return Ok(Step::Status(ST_INVALID));
                };
                return if code == op::ADD_ATTRIBUTE {
                    Self::status(self.store.add_attribute(o, h))
                } else {
                    Self::status(self.store.remove_attribute(o, h, a[3] != 0).map(|_| ()))
                };
            }
            _ => {}
        }

        let Some(h) = self.resolve(a[1]) else {
            return Ok(Step::Status(ST_INVALID));
        };
        match code {
            op::MAKE_COPY => {
                let c = self.attr(h)?.make_copy();
                self.add_handle(c, out)?;
                Ok(Step::Status(0))
            }
            op::SPLIT => {
                let (s, c1, c2) = (self.obj(a[2])?, self.obj(a[3])?, self.obj(a[4])?);
                Self::status(self.store.split(h, s, c1, c2))
            }
            op::COPY => {
                let (f, t) = (self.obj(a[2])?, self.obj(a[3])?);
                Self::status(self.store.copy(h, f, t))
            }
            op::MERGE => {
                let (o1, o2, m) = (self.obj(a[2])?, self.obj(a[3])?, self.obj(a[4])?);
                if kind(self.attr(h)?) == KIND_MERGE_REGION {
                    let find = |o| self.store.find_attribute(o, 114);
                    let f = [find(m), find(o1), find(o2)];
                    for x in f.iter().flatten() {
                        if kind(self.attr(*x)?) != KIND_MERGE_REGION {
                            return Ok(Step::Status(ST_SKIP));
                        }
                    }
                    if f[1].is_some() && f[2].is_some() && f[0].is_none() {
                        return Ok(Step::Status(ST_SKIP));
                    }
                }
                Self::status(self.store.merge_with_regions(h, o1, o2, m, &self.regions))
            }
            op::COINCIDENT => {
                let (f, t) = (self.obj(a[2])?, self.obj(a[3])?);
                Self::status(self.store.coincident(h, f, t))
            }
            op::EDIT => {
                let o = self.obj(a[2])?;
                Self::status(self.store.edit(h, o))
            }
            op::DESTRUCTION => {
                let o = self.obj(a[2])?;
                Self::status(self.store.destruction(h, o))
            }
            op::PROPAGATE => {
                let (o1, o2, m) = (self.obj(a[2])?, self.obj(a[3])?, self.obj(a[4])?);
                if kind(self.attr(h)?) == KIND_COLOR {
                    let find = |o| self.store.find_attribute(o, 100);
                    let is_vec = |x: Option<AttributeHandle>| -> Result<bool, HarnessError> {
                        Ok(match x {
                            Some(x) => matches!(kind(self.attr(x)?), KIND_VECTOR3D | KIND_COLOR),
                            None => false,
                        })
                    };
                    let (c1, c2, cm) = (find(o1), find(o2), find(m));
                    if c1.is_some() && c2.is_some() && !(is_vec(c1)? && is_vec(c2)? && is_vec(cm)?)
                    {
                        return Ok(Step::Status(ST_SKIP));
                    }
                }
                Self::status(self.store.propagate_regions_through_booleans(h, o1, o2, m))
            }
            op::ADD_USER => {
                let o = self.obj(a[2])?;
                Self::status(self.store.add_user(h, o))
            }
            op::REMOVE_USER | op::FROM_CLEAR => {
                let o = self.obj(a[2])?;
                if !self.listings(h).is_empty() {
                    return Ok(Step::Status(ST_SKIP));
                }
                if code == op::FROM_CLEAR {
                    Self::status(self.store.from_clear_attributes(h, o).map(|_| ()))
                } else {
                    Self::status(self.store.remove_user(h, o, a[3] != 0).map(|_| ()))
                }
            }
            op::GET_USERS => {
                let u = self.store.users(h).map_err(|e| harness(&e))?;
                out.extend(u.into_iter().map(|k| k.0 as i64));
                Ok(Step::Status(0))
            }
            op::QUERY => {
                let at = self.attr(h)?;
                out.push(at.type_code());
                let mut mask = 0i64;
                for (i, t) in QUERY_TYPES.iter().enumerate() {
                    if at.is_kind_of(*t) {
                        mask |= 1 << i;
                    }
                }
                out.push(mask);
                out.push(i64::from(at.is_propagated_through_booleans()));
                out.push(at.attribute_id() as i64);
                out.push(i64::from(at.behavior().code()));
                out.extend(at.type_string().bytes().map(i64::from));
                Ok(Step::Status(0))
            }
            op::MR_QUERY => {
                let at = self.attr(h)?;
                let Some(d) = at.merge_region_data() else {
                    return Ok(Step::Status(ST_SKIP));
                };
                let prop = self
                    .store
                    .has_any_other_propagated_attribs(h)
                    .map_err(|e| harness(&e))?;
                out.push(i64::from(d.has_this_infinite_region()));
                out.push(i64::from(d.has_other_infinite_region()));
                out.push(i64::from(d.has_any_solid(a[2] as u64)));
                out.push(i64::from(d.operation()));
                out.push(i64::from(prop));
                out.push(d.this_regions().len() as i64);
                out.push(d.this_region_types().len() as i64);
                out.push(d.other_regions().len() as i64);
                if let Some(r) = d.this_region(0) {
                    out.push(r.0 as i64);
                }
                Ok(Step::Status(0))
            }
            op::MR_SET_REGIONS => {
                // as the shim: the kind check comes before the object check
                if self
                    .store
                    .get(h)
                    .and_then(Attribute::merge_region_data)
                    .is_none()
                {
                    return Ok(Step::Status(ST_SKIP));
                }
                let (r1, r2) = (self.obj(a[3])?, self.obj(a[5])?);
                let Some(d) = self
                    .store
                    .get_mut(h)
                    .and_then(Attribute::merge_region_data_mut)
                else {
                    return Ok(Step::Status(ST_SKIP));
                };
                let idx = a[2] as u64;
                let n_this = d.this_regions().len() as u64;
                let others = [
                    d.this_types.len(),
                    d.other_regions.len(),
                    d.other_types.len(),
                ];
                if idx < n_this && others.iter().any(|&n| idx >= n as u64) {
                    return Ok(Step::Status(ST_SKIP));
                }
                let index = usize::try_from(idx).unwrap_or(usize::MAX);
                Self::status(d.set_regions(index, r1, a[4] as u64 as i64, r2, a[6] as u64 as i64))
            }
            op::MR_SET_OPERATION => {
                let Some(d) = self
                    .store
                    .get_mut(h)
                    .and_then(Attribute::merge_region_data_mut)
                else {
                    return Ok(Step::Status(ST_SKIP));
                };
                d.set_operation(a[2] as i32);
                Ok(Step::Status(0))
            }
            op::SET_VECTOR => {
                let v = self.next_vec()?;
                let at = self
                    .store
                    .get_mut(h)
                    .ok_or_else(|| HarnessError::new("dead"))?;
                if at.vector3d_value().is_none() {
                    return Ok(Step::Status(ST_SKIP));
                }
                Self::status(at.set_vector3d_value(v))
            }
            op::DESTROY => {
                let users = self.attr(h)?.users().to_vec();
                if !users.is_empty() {
                    if !self.attr(h)?.behavior().is_standalone() {
                        return Ok(Step::Status(ST_SKIP));
                    }
                    let mut u = users;
                    let mut l = self.listings(h);
                    u.sort_unstable();
                    l.sort_unstable();
                    if u != l {
                        return Ok(Step::Status(ST_SKIP));
                    }
                }
                Self::status(self.store.destroy(h))
            }
            other => Err(HarnessError::new(format!("op {other}"))),
        }
    }

    fn dump(&self, ints: &mut Vec<i64>, dbls: &mut Vec<f64>) -> Result<(), HarnessError> {
        let mut canon: Vec<AttributeHandle> = Vec::new();
        let index_of = |canon: &Vec<AttributeHandle>, h| canon.iter().position(|&x| x == h);
        for &h in &self.handles {
            if self.store.is_live(h) && index_of(&canon, h).is_none() {
                canon.push(h);
            }
        }
        let lists: Vec<Vec<AttributeHandle>> = (0..self.n_objects)
            .map(|o| self.store.attributes(ObjectKey(o as u64)))
            .collect();
        for l in &lists {
            for &h in l {
                if index_of(&canon, h).is_none() {
                    canon.push(h);
                }
            }
        }
        let ci = |h| index_of(&canon, h).map_or(-1, |i| i as i64);
        ints.push(self.handles.len() as i64);
        for &h in &self.handles {
            ints.push(i64::from(self.store.is_live(h)));
        }
        for &h in &self.handles {
            ints.push(if self.store.is_live(h) { ci(h) } else { -1 });
        }
        ints.push(canon.len() as i64);
        for &h in &canon {
            let a = self.attr(h)?;
            ints.push(kind(a));
            ints.push(a.type_code());
            ints.push(a.attribute_id() as i64);
            ints.push(i64::from(a.behavior().code()));
            ints.push(a.users().len() as i64);
            ints.extend(a.users().iter().map(|k| k.0 as i64));
            let mut v: Vec<i64> = Vec::new();
            let mut d: Vec<f64> = Vec::new();
            match a.value() {
                AttributeValue::None
                | AttributeValue::PolyToEuVu { .. }
                | AttributeValue::PolyEdgeToVertexUse { .. }
                | AttributeValue::DecimateVertex(_)
                | AttributeValue::QuadricVertex(_)
                | AttributeValue::QuadricEdge(_) => {}
                AttributeValue::Generic {
                    longs,
                    doubles,
                    chars,
                } => {
                    v.push(longs.len() as i64);
                    v.extend(longs);
                    v.push(chars.len() as i64);
                    v.extend(chars.iter().map(|&c| i64::from(c)));
                    d.extend(doubles);
                }
                AttributeValue::Long(x) => v.push(*x),
                AttributeValue::Pointer(p) => v.push(*p as i64),
                AttributeValue::PointerList(l) => {
                    v.push(l.len() as i64);
                    v.extend(l.iter().map(|&p| p as i64));
                }
                AttributeValue::Vector3d(x) | AttributeValue::Color(x) => d.extend([x.x, x.y, x.z]),
                AttributeValue::MergeRegion(m) => {
                    v.push(i64::from(m.operation));
                    v.push(m.this_regions.len() as i64);
                    v.extend(m.this_regions.iter().map(|k| k.0 as i64));
                    v.push(m.this_types.len() as i64);
                    v.extend(&m.this_types);
                    v.push(m.other_regions.len() as i64);
                    v.extend(m.other_regions.iter().map(|k| k.0 as i64));
                    v.push(m.other_types.len() as i64);
                    v.extend(&m.other_types);
                }
                AttributeValue::Tag(t) => v.extend(t.to_array()),
            }
            ints.push(v.len() as i64);
            ints.extend(v);
            ints.push(d.len() as i64);
            dbls.extend(d);
        }
        for l in &lists {
            ints.push(l.len() as i64);
            ints.extend(l.iter().map(|&h| ci(h)));
        }
        ints.push(self.all.len() as i64);
        for slot in &self.all {
            ints.push(
                slot.and_then(|h| self.handles.iter().position(|&x| x == h))
                    .map_or(-1, |i| i as i64),
            );
        }
        match self.reader.as_ref().and_then(DatabaseReader::in_stream) {
            Some(s) => {
                ints.push(i64::from(s.state().bits()));
                ints.push(s.position() as i64);
            }
            None => ints.extend([-1, -1]),
        }
        Ok(())
    }
}

fn run_rust_case(v: &Value) -> Result<Output, HarnessError> {
    let s = decode(v)?;
    let (reader, data) = match &s.stream {
        Some((d, ft, swap)) => {
            let ft = FileType::from_code(*ft).map_err(|e| HarnessError::new(e.to_string()))?;
            let mut r = DatabaseReader::with_stream(ft, InputStream::new(d.clone()));
            r.set_byte_swap(*swap);
            (Some(r), d.clone())
        }
        None => (None, Vec::new()),
    };
    let regions = crate::fixtures::regions::RegionScene::orphan_objects(
        (0..s.n_objects).map(|i| ObjectKey(i as u64)),
    )
    .map_err(|e| HarnessError::new(e.to_string()))?;
    let mut run = RustRun {
        store: AttributeStore::new(),
        regions,
        n_objects: s.n_objects,
        handles: Vec::new(),
        all: Vec::new(),
        reader,
        data,
        pay_i: s.payload_i.clone().into_iter(),
        pay_d: s.payload_d.clone().into_iter(),
    };
    let mut ints = Vec::new();
    let mut dbls = Vec::new();
    for rec in &s.ops {
        let mut out = Vec::new();
        match run.run_op(*rec, &mut out)? {
            Step::Status(st) => {
                ints.push(st);
                ints.push(out.len() as i64);
                ints.extend(out);
            }
            Step::Deferred(d) => return Ok(d),
        }
    }
    run.dump(&mut ints, &mut dbls)?;
    to_output(s.n_objects, s.ops.len(), &ints, &dbls)
}

// ---- generator ------------------------------------------------------------------

const IDS: &[i64] = &[100, 100, 106, 114, 114, 5, 6, 116, 10000];

struct Gen<'a> {
    rng: &'a mut Rng,
    n_obj: i64,
    n_handles: i64,
    allow_mr: bool,
    /// Handles created as merge-region / vector attributes (targets of the ops that
    /// need that class).
    mr_handles: Vec<i64>,
    vec_handles: Vec<i64>,
    ops: Vec<Value>,
}

impl Gen<'_> {
    fn obj(&mut self) -> i64 {
        self.rng.below(self.n_obj as u64) as i64
    }

    fn beh(&mut self) -> i64 {
        self.rng.below(5) as i64
    }

    fn id(&mut self) -> i64 {
        *self.rng.choose(IDS).unwrap_or(&5)
    }

    fn reference(&mut self) -> i64 {
        if self.n_handles > 0 && self.rng.chance(0.6) {
            self.rng.below(self.n_handles as u64 + 1) as i64
        } else {
            let o = self.obj();
            let i = self.rng.below(3) as i64;
            -(o * REF_STRIDE + i) - 1
        }
    }

    /// A reference, usually to one of `pool` when it is not empty.
    fn reference_in(&mut self, pool: &[i64]) -> i64 {
        if !pool.is_empty() && self.rng.chance(0.8) {
            *self.rng.choose(pool).unwrap_or(&0)
        } else {
            self.reference()
        }
    }

    fn small(&mut self) -> f64 {
        // a few exactly representable values plus arbitrary ones
        if self.rng.chance(0.5) {
            (self.rng.range_i64(-8, 8) as f64) / 4.0
        } else {
            self.rng.uniform(-10.0, 10.0)
        }
    }

    fn push(&mut self, v: Value) {
        self.ops.push(v);
    }

    fn create(&mut self) {
        let id = self.id();
        let beh = self.beh();
        let r = self.rng.below(100);
        let v = if id == 100 && r < 70 {
            let c = [self.small(), self.small(), self.small()];
            if self.rng.chance(0.5) {
                serde_json::json!(["new_color", id, beh, c])
            } else {
                serde_json::json!(["new_vector3d", id, beh, c])
            }
        } else if id == 114 && r < 70 && self.allow_mr {
            let (a, b) = (self.obj(), self.obj());
            let (ta, tb) = (self.rng.below(3), self.rng.below(3));
            serde_json::json!(["new_merge_region", a, ta, b, tb])
        } else {
            match self.rng.below(if self.allow_mr { 9 } else { 8 }) {
                0 => serde_json::json!(["new_plain", id, beh]),
                1 => {
                    let nl = self.rng.below(4);
                    let nd = self.rng.below(4);
                    let nc = self.rng.below(6);
                    let l: Vec<i64> = (0..nl).map(|_| self.rng.range_i64(-1000, 1000)).collect();
                    let d: Vec<f64> = (0..nd).map(|_| self.small()).collect();
                    let c: Vec<i64> = (0..nc).map(|_| self.rng.below(256) as i64).collect();
                    serde_json::json!(["new_generic", id, beh, l, d, c])
                }
                2 => {
                    let v = self.rng.range_i64(-1_000_000, 1_000_000);
                    serde_json::json!(["new_long", id, v, beh])
                }
                3 => {
                    let p = self.rng.below(1 << 40) as i64;
                    serde_json::json!(["new_pointer", id, p, beh])
                }
                4 => {
                    if self.rng.chance(0.2) {
                        serde_json::json!(["new_pointer_list", id, beh, Value::Null])
                    } else {
                        let n = self.rng.below(4);
                        let l: Vec<i64> = (0..n).map(|_| self.rng.below(1 << 40) as i64).collect();
                        serde_json::json!(["new_pointer_list", id, beh, l])
                    }
                }
                5 => {
                    let c = [self.small(), self.small(), self.small()];
                    serde_json::json!(["new_vector3d", id, beh, c])
                }
                6 => {
                    let c = [self.small(), self.small(), self.small()];
                    serde_json::json!(["new_color", id, beh, c])
                }
                7 => {
                    let t: Vec<i64> = (0..8).map(|_| self.rng.range_i64(-3, 5)).collect();
                    serde_json::json!(["new_tag", id, beh, t])
                }
                _ => {
                    let (a, b) = (self.obj(), self.obj());
                    let (ta, tb) = (self.rng.below(3), self.rng.below(3));
                    serde_json::json!(["new_merge_region", a, ta, b, tb])
                }
            }
        };
        match v.get(0).and_then(Value::as_str) {
            Some("new_merge_region") => self.mr_handles.push(self.n_handles),
            Some("new_vector3d" | "new_color") => self.vec_handles.push(self.n_handles),
            _ => {}
        }
        self.n_handles += 1;
        self.push(v);
    }

    fn step(&mut self, has_stream: bool) {
        let r = self.rng.below(100);
        let v = match r {
            0..=21 => {
                self.create();
                return;
            }
            22..=39 => {
                let (o, a) = (self.obj(), self.reference());
                serde_json::json!(["add_attribute", o, a])
            }
            40..=44 => {
                let (a, s, c1, c2) = (self.reference(), self.obj(), self.obj(), self.obj());
                serde_json::json!(["split", a, s, c1, c2])
            }
            45..=48 => {
                let (a, f, t) = (self.reference(), self.obj(), self.obj());
                serde_json::json!(["copy", a, f, t])
            }
            49..=52 => {
                let (a, o1, o2, m) = (self.reference(), self.obj(), self.obj(), self.obj());
                serde_json::json!(["merge", a, o1, o2, m])
            }
            53..=55 => {
                let (a, f, t) = (self.reference(), self.obj(), self.obj());
                serde_json::json!(["coincident", a, f, t])
            }
            56..=60 => {
                let (a, o1, o2, m) = (self.reference(), self.obj(), self.obj(), self.obj());
                serde_json::json!(["propagate", a, o1, o2, m])
            }
            61..=66 => {
                let (o, a, d) = (self.obj(), self.reference(), self.rng.below(2));
                serde_json::json!(["remove_attribute", o, a, d])
            }
            67..=68 => {
                let o = self.obj();
                serde_json::json!(["clear_attributes", o])
            }
            69..=71 => {
                let (a, o) = (self.reference(), self.obj());
                serde_json::json!(["add_user", a, o])
            }
            72..=73 => {
                let (a, o, d) = (self.reference(), self.obj(), self.rng.below(2));
                serde_json::json!(["remove_user", a, o, d])
            }
            74 => {
                let (a, o) = (self.reference(), self.obj());
                serde_json::json!(["from_clear", a, o])
            }
            75..=77 => {
                let a = self.reference();
                serde_json::json!(["get_users", a])
            }
            78..=80 => {
                let a = self.reference();
                serde_json::json!(["query", a])
            }
            81..=82 => {
                let pool = self.mr_handles.clone();
                let (a, f) = (self.reference_in(&pool), self.rng.below(4));
                serde_json::json!(["mr_query", a, f])
            }
            83 => {
                let pool = self.mr_handles.clone();
                let a = self.reference_in(&pool);
                let i = if self.rng.chance(0.7) {
                    0
                } else {
                    self.rng.range_i64(-1, 3)
                };
                let (r1, t1, r2, t2) =
                    (self.obj(), self.rng.below(3), self.obj(), self.rng.below(3));
                serde_json::json!(["mr_set_regions", a, i, r1, t1, r2, t2])
            }
            84 => {
                let pool = self.mr_handles.clone();
                let (a, o) = (self.reference_in(&pool), self.rng.below(10));
                serde_json::json!(["mr_set_operation", a, o])
            }
            85..=86 => {
                let pool = self.vec_handles.clone();
                let a = self.reference_in(&pool);
                let c = [self.small(), self.small(), self.small()];
                serde_json::json!(["set_vector", a, c])
            }
            87..=88 => {
                let a = self.reference();
                serde_json::json!(["destroy", a])
            }
            89..=91 => {
                let (o, id) = (self.obj(), self.id());
                serde_json::json!(["find", o, id])
            }
            92..=93 => {
                let a = self.reference();
                serde_json::json!(["make_copy", a])
            }
            94..=95 => {
                let (a, o) = (self.reference(), self.obj());
                if self.rng.chance(0.5) {
                    serde_json::json!(["edit", a, o])
                } else {
                    serde_json::json!(["destruction", a, o])
                }
            }
            _ => {
                if has_stream {
                    serde_json::json!(["read_indexed"])
                } else {
                    let o = self.obj();
                    serde_json::json!(["clear_attributes", o])
                }
            }
        };
        if matches!(v.get(0).and_then(Value::as_str), Some("make_copy")) {
            self.n_handles += 1;
        }
        self.push(v);
    }
}

fn gen_ascii_stream(rng: &mut Rng) -> Value {
    let mut text = String::new();
    let lines = 1 + rng.below(8);
    for _ in 0..lines {
        match rng.below(12) {
            0 => text.push_str("            0 \n"),
            1 => text.push_str("abc\n"),
            2 => {
                // a line longer than GOTO_NEXT_ATTLINE accepts (or just under)
                let n = 995 + rng.below(10) as usize;
                text.push_str("1 3 ");
                text.push_str(&"y".repeat(n));
                text.push('\n');
            }
            3 => text.push_str("2 4\n"), // count larger than the indices on the line
            4 => {
                text.push_str("1 2 ");
                text.push(char::from(0xFFu8));
                text.push('\n');
            }
            5 => text.push_str("+3 1 +2 0 trailing words\n"),
            6 => text.push_str("\t\n"),
            _ => {
                let n = rng.below(5);
                text.push_str("            ");
                text.push_str(&n.to_string());
                text.push(' ');
                for _ in 0..n {
                    text.push_str(&rng.below(12).to_string());
                    text.push(' ');
                }
                text.push('\n');
            }
        }
    }
    if rng.chance(0.3) {
        text.pop(); // last line without its newline
    }
    // `char::from(0xFF)` encodes as two UTF-8 bytes; send raw bytes instead.
    let bytes: Vec<u8> = text
        .chars()
        .map(|c| if c == '\u{FF}' { 0xFF } else { c as u8 })
        .collect();
    serde_json::json!({"data": bytes, "file_type": 0, "byte_swap": false})
}

fn gen_binary_stream(rng: &mut Rng) -> Value {
    let swap = rng.chance(0.3);
    let file_type = 1 + rng.below(3) as i64;
    let mut data = Vec::new();
    let enc = |v: u64, out: &mut Vec<u8>| {
        let v = if swap { v.swap_bytes() } else { v };
        out.extend_from_slice(&v.to_le_bytes());
    };
    for _ in 0..=rng.below(5) {
        let n = rng.below(4);
        enc(n, &mut data);
        for _ in 0..n {
            enc(rng.below(10), &mut data);
        }
    }
    if rng.chance(0.2) {
        // a truncated tail (skipped by both sides)
        data.extend_from_slice(&[1, 0, 0]);
    }
    serde_json::json!({"data": data, "file_type": file_type, "byte_swap": swap})
}

fn generate_case(rng: &mut Rng) -> Value {
    let n_obj = 2 + rng.below(4) as i64;
    let stream = match rng.below(4) {
        0 => Some(gen_ascii_stream(rng)),
        1 => Some(gen_binary_stream(rng)),
        _ => None,
    };
    // A Merge on a merge-region attribute defers the whole case (SmRegion::GetBrep is
    // unported), so only some cases create merge-region attributes at all.
    let allow_mr = rng.chance(0.3);
    let n_ops = 4 + rng.below(36) as usize;
    let mut g = Gen {
        rng,
        n_obj,
        n_handles: 0,
        allow_mr,
        mr_handles: Vec::new(),
        vec_handles: Vec::new(),
        ops: Vec::new(),
    };
    // Region states are set before any attribute exists (MAKE_INFINITE replaces the
    // object), so merge-region attributes meet infinite and void regions as well as
    // plain solid ones.
    if allow_mr {
        for _ in 0..=g.rng.below(2) {
            let o = g.obj();
            g.push(serde_json::json!(["make_infinite", o]));
        }
        for _ in 0..=g.rng.below(3) {
            let (o, v) = (g.obj(), g.rng.below(3) as i64);
            g.push(serde_json::json!(["set_is_void", o, v]));
        }
    }
    for _ in 0..2 + g.rng.below(3) {
        g.create();
    }
    for _ in 0..n_ops {
        if allow_mr && g.rng.chance(0.04) {
            let (o, v) = (g.obj(), g.rng.below(3) as i64);
            g.push(serde_json::json!(["set_is_void", o, v]));
        }
        g.step(stream.is_some());
    }
    serde_json::json!({"n_objects": n_obj, "ops": g.ops, "stream": stream})
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "geom.attribute"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        Some(generate_case(rng))
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        run_cpp_case(&case.inputs)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        run_rust_case(&case.inputs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every op runs (status 0) in a good share of generated scripts, so skips and
    /// unresolved references do not hide parts of the unit.
    #[test]
    fn generated_scripts_exercise_every_op() {
        let mut ok = [0usize; OPS.len()];
        let mut total = [0usize; OPS.len()];
        let mut deferred = 0;
        for seed in 0..2000u64 {
            let mut rng = Rng::new(seed);
            let v = generate_case(&mut rng);
            let s = decode(&v).unwrap();
            let out = run_rust_case(&v).unwrap();
            if out.deferred.is_some() {
                deferred += 1;
                continue;
            }
            for (rec, st) in s.ops.iter().zip(&out.ints["status"]) {
                let i = rec[0] as usize;
                total[i] += 1;
                if *st == 0 {
                    ok[i] += 1;
                }
            }
        }
        for (i, (name, _)) in OPS.iter().enumerate() {
            println!("{name:20} ok {:6} of {:6}", ok[i], total[i]);
            assert!(ok[i] >= 20, "op {name} ran only {} times", ok[i]);
        }
        println!("deferred {deferred}");
        assert!(deferred < 200);
    }
}
