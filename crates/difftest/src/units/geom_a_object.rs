//! Runner for unit `geom.a_object`: attributed objects, their notification and
//! caches, the stand-alone-with-graphics base and the `NLI_Math.h` helpers.
//!
//! C++: `solidcore_ffi::units::geom_a_object` (upstream `SmAObject`, `SmCacheMgr`,
//! `SmCacheCheckOutIn`, `SmSAGObject`, `NI_*_specialized`). Rust:
//! `solidcore::geom::a_object`.
//!
//! Three case kinds (`inputs.kind`):
//! - `"types"`: `IsKindOf` of both classes for a list of type codes, the type codes,
//!   `AssertValid` and the four type / class strings.
//! - `"nli"`: `NI_CPtToPtEuclid_specialized` and `NI_VectorBlendCPt_specialized` on
//!   one input set (compared bit for bit through the 1e-9 rule).
//! - `"script"`: a script over objects (`objects`: 0 a bare `SmRegion`, 1 a plain
//!   `SmAObject`, 2 an empty `SmBrep`; more are added by `copy_construct`),
//!   attributes and cache objects, with recording notify callbacks
//!   (`user_cb`, `sys_cb`). Ops are `["name", ints..., [x, y, z]?]`.
//!
//! Attribute references: `r >= 0` is a *fresh* handle (created by a `new_*` op and
//! not yet added to any object; `add` consumes it), `r < 0` is entry `k % 1024` of
//! object `k / 1024`'s list (`k = -r - 1`). With only these ops, the objects listing
//! an attribute always equal its users (as multisets), so no list ever holds a
//! deleted attribute; fresh handles are never deleted.
//!
//! Statuses: `0` ok, `-1` the reference does not resolve, `-2` skipped because
//! upstream would hit undefined behaviour; both sides decide skips on their own state:
//! - `notify`: a handler would dereference a NULL data object (`split_in_brep`
//!   without `d2`/`d3`, `coincident`/`copy` without `d1`, `split` without `d1`/`d2`,
//!   the merges without `d1`/`d2`); a merge whose sources carry an
//!   `SM_AI_OTHER_REGION_ID` attribute (the merge-region handler casts / dereferences
//!   unchecked); region propagation on a region whose `SM_AI_OTHER_REGION_ID`
//!   attribute is missing or of another class, with more than one "other" region
//!   (upstream indexes its stale second list), with the caller itself as the other
//!   region, with a duplicate entry in its list (read after delete), or with a
//!   non-vector `SM_AI_COLOR` attribute on the regions (mistyped colour merge).
//! - cache ops on a deleted cache (`check_out`, `check_in`, `soft_delete`,
//!   `guard_open`, `guard_close` of a guard whose cache died, `delete_objects_cache`
//!   of an object whose cache died); `copy_construct` past 64 objects.
//!
//! Output per op: status and outputs (`notify`: the callback records, 7 values
//! each: tag, operation, caller, caller type, data1..3); then every object (type,
//! attribute list as canonical indices, cache index), every reachable attribute
//! (object lists in order, then the fresh handles: class, id, behaviour, users,
//! long value / merge-region data, vector doubles), the fresh handles' canonical
//! indices, every cache (alive, use count, deleted flag) and the open guard count.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use solidcore::geom::a_object::{
    self, AObject, CacheCheckOutIn, CacheId, CacheMaker, CacheStore, ObjectCacheType,
};
use solidcore::geom::attribute::{
    ids, Attribute, AttributeHandle, AttributeStore, AttributeValue, Behavior, ObjectKey,
};
use solidcore::geom::context::{Context, NotifyCallback};
use solidcore::geom::object::{NotifyOperation, ObjectRef};
use solidcore::nurbs::math::CPoint;
use solidcore::{SolidError, Vec3};
use solidcore_ffi::units::geom_a_object::{self as ffi, op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::output::Topology;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `geom.a_object` runner.
pub struct Runner;

const ST_INVALID: i64 = -1;
const ST_SKIP: i64 = -2;
const REF_STRIDE: i64 = 1024;
const MAX_OBJECTS: usize = 64;

const O_REGION: i64 = 0;
const O_AOBJECT: i64 = 1;
const O_BREP: i64 = 2;

const K_PLAIN: i64 = 0;
const K_LONG: i64 = 1;
const K_VECTOR3D: i64 = 2;
const K_COLOR: i64 = 3;
const K_MERGE_REGION: i64 = 4;
const K_OTHER: i64 = 5;

const OPS: &[(&str, i64)] = &[
    ("new_plain", op::NEW_PLAIN),
    ("new_long", op::NEW_LONG),
    ("new_vector3d", op::NEW_VECTOR3D),
    ("new_color", op::NEW_COLOR),
    ("new_merge_region", op::NEW_MERGE_REGION),
    ("mr_set_operation", op::MR_SET_OPERATION),
    ("add", op::ADD),
    ("remove", op::REMOVE),
    ("clear", op::CLEAR),
    ("find", op::FIND),
    ("notify", op::NOTIFY),
    ("copy_construct", op::COPY_CONSTRUCT),
    ("assert_valid", op::ASSERT_VALID),
    ("new_cache", op::NEW_CACHE),
    ("set_cache", op::SET_CACHE),
    ("get_cache", op::GET_CACHE),
    ("check_out", op::CHECK_OUT),
    ("check_in", op::CHECK_IN),
    ("soft_delete", op::SOFT_DELETE),
    ("delete_objects_cache", op::DELETE_OBJECTS_CACHE),
    ("get_or_create", op::GET_OR_CREATE),
    ("guard_open", op::GUARD_OPEN),
    ("guard_close", op::GUARD_CLOSE),
    ("sag_make", op::SAG_MAKE),
    ("sag_notify", op::SAG_NOTIFY),
];

fn h(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

fn as_int(v: &Value) -> Result<i64, HarnessError> {
    v.as_i64()
        .or_else(|| v.as_u64().map(|u| u as i64))
        .ok_or_else(|| h(format!("expected integer, got {v}")))
}

fn f64s<const N: usize>(v: Option<&Value>, what: &str) -> Result<[f64; N], HarnessError> {
    let a = v
        .and_then(Value::as_array)
        .ok_or_else(|| h(format!("{what}: expected {N} numbers")))?;
    if a.len() != N {
        return Err(h(format!("{what}: expected {N} numbers")));
    }
    let mut out = [0.0; N];
    for (o, x) in out.iter_mut().zip(a) {
        *o = x.as_f64().ok_or_else(|| h(format!("{what}: not a number")))?;
    }
    Ok(out)
}

// ---- types ------------------------------------------------------------------------

fn types_rust(ts: &[i64]) -> Vec<i64> {
    let mut res = Vec::new();
    for &t in ts {
        res.push(i64::from(a_object::is_kind_of(t)));
        res.push(i64::from(a_object::sag_is_kind_of(t)));
    }
    res.extend([a_object::TYPE, a_object::TYPE, a_object::SAG_TYPE, a_object::SAG_TYPE]);
    res.push(i64::from(a_object::assert_valid(false)));
    res.push(i64::from(a_object::assert_valid(true)));
    for s in [
        a_object::TYPE_STRING,
        a_object::CLASS_STRING,
        a_object::SAG_TYPE_STRING,
        a_object::SAG_CLASS_STRING,
    ] {
        res.push(s.len() as i64);
        res.extend(s.bytes().map(i64::from));
    }
    res
}

fn type_list(v: &Value) -> Result<Vec<i64>, HarnessError> {
    v.get("ts")
        .and_then(Value::as_array)
        .ok_or_else(|| h("ts"))?
        .iter()
        .map(as_int)
        .collect()
}

// ---- nli --------------------------------------------------------------------------

struct Nli {
    noz: bool,
    now: bool,
    pw: [f64; 4],
    alpha: f64,
    aw: [f64; 4],
    bw: [f64; 4],
}

fn nli_inputs(v: &Value) -> Result<Nli, HarnessError> {
    let flag = |k: &str| {
        v.get(k)
            .and_then(Value::as_bool)
            .ok_or_else(|| h(format!("{k}: expected bool")))
    };
    Ok(Nli {
        noz: flag("noz")?,
        now: flag("now")?,
        pw: f64s(v.get("pw"), "pw")?,
        alpha: v
            .get("alpha")
            .and_then(Value::as_f64)
            .ok_or_else(|| h("alpha"))?,
        aw: f64s(v.get("aw"), "aw")?,
        bw: f64s(v.get("bw"), "bw")?,
    })
}

fn nli_output(r: [f64; 7]) -> Output {
    Output::new()
        .values("euclid", r.get(..3).unwrap_or_default().to_vec())
        .values("blend", r.get(3..).unwrap_or_default().to_vec())
        .ints("euclid_bits", r.iter().take(3).map(|x| x.to_bits() as i64).collect::<Vec<_>>())
        .ints("blend_bits", r.iter().skip(3).map(|x| x.to_bits() as i64).collect::<Vec<_>>())
}

fn cpt(a: [f64; 4]) -> CPoint {
    CPoint::new(a[0], a[1], a[2], a[3])
}

fn nli_rust(n: &Nli) -> [f64; 7] {
    let p: Vec3 = a_object::cpt_to_pt_euclid_specialized(n.noz, n.now, cpt(n.pw));
    let b = a_object::vector_blend_cpt_specialized(n.noz, n.now, n.alpha, cpt(n.aw), cpt(n.bw));
    [p.x, p.y, p.z, b.x, b.y, b.z, b.w]
}

// ---- scripts ----------------------------------------------------------------------

struct Script {
    objects: Vec<i64>,
    user_cb: bool,
    sys_cb: bool,
    ops: Vec<[i64; 8]>,
    payload_d: Vec<f64>,
}

fn op_code(name: &str) -> Result<i64, HarnessError> {
    OPS.iter()
        .find(|(n, _)| *n == name)
        .map(|&(_, c)| c)
        .ok_or_else(|| h(format!("unknown op `{name}`")))
}

fn decode(v: &Value) -> Result<Script, HarnessError> {
    let objects = v
        .get("objects")
        .and_then(Value::as_array)
        .ok_or_else(|| h("objects"))?
        .iter()
        .map(as_int)
        .collect::<Result<Vec<_>, _>>()?;
    let flag = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
    let mut s = Script {
        objects,
        user_cb: flag("user_cb"),
        sys_cb: flag("sys_cb"),
        ops: Vec::new(),
        payload_d: Vec::new(),
    };
    let raw = v
        .get("ops")
        .and_then(Value::as_array)
        .ok_or_else(|| h("ops"))?;
    for o in raw {
        let items = o
            .as_array()
            .ok_or_else(|| h(format!("op {o} is not an array")))?;
        let name = items
            .first()
            .and_then(Value::as_str)
            .ok_or_else(|| h(format!("op {o} has no name")))?;
        let code = op_code(name)?;
        let mut rec = [code, 0, 0, 0, 0, 0, 0, 0];
        let mut slot = 1;
        for x in items.iter().skip(1) {
            if x.is_array() {
                let d: [f64; 3] = f64s(Some(x), name)?;
                s.payload_d.extend(d);
            } else {
                let r = rec
                    .get_mut(slot)
                    .ok_or_else(|| h(format!("op {o}: too many arguments")))?;
                *r = as_int(x)?;
                slot += 1;
            }
        }
        s.ops.push(rec);
    }
    Ok(s)
}

/// Parses the int / double streams (the shim's or the Rust side's identical ones).
fn to_output(n_ops: usize, ints: &[i64], dbls: &[f64]) -> Result<Output, HarnessError> {
    let mut it = ints.iter().copied();
    let mut next = || it.next().ok_or_else(|| h("truncated output"));
    let take = |n: i64, next: &mut dyn FnMut() -> Result<i64, HarnessError>| {
        (0..n.max(0)).map(|_| next()).collect::<Result<Vec<i64>, _>>()
    };
    let mut status = Vec::new();
    let mut outs = Vec::new();
    for _ in 0..n_ops {
        status.push(next()?);
        let n = next()?;
        outs.push(take(n, &mut next)?);
    }
    let n_obj = next()?;
    let mut objects = Vec::new();
    for _ in 0..n_obj {
        let mut rec = vec![next()?];
        let n = next()?;
        rec.push(n);
        rec.extend(take(n, &mut next)?);
        rec.push(next()?);
        objects.push(rec);
    }
    let n_attrs = next()?;
    let mut attrs = Vec::new();
    let mut total_d = 0;
    for _ in 0..n_attrs {
        let mut rec = take(3, &mut next)?;
        let n_users = next()?;
        rec.push(n_users);
        rec.extend(take(n_users, &mut next)?);
        let n_vals = next()?;
        rec.push(n_vals);
        rec.extend(take(n_vals, &mut next)?);
        let n_d = next()?;
        rec.push(n_d);
        total_d += n_d;
        attrs.push(rec);
    }
    let n_handles = next()?;
    let handles = take(n_handles, &mut next)?;
    let n_caches = next()?;
    let caches = (0..n_caches)
        .map(|_| take(3, &mut next))
        .collect::<Result<Vec<_>, _>>()?;
    let guards = next()?;
    if it.next().is_some() {
        return Err(h("trailing output"));
    }
    if total_d as usize != dbls.len() {
        return Err(h("double stream length mismatch"));
    }
    let bits: Vec<i64> = dbls.iter().map(|d| d.to_bits() as i64).collect();
    Ok(Output::new()
        .ints("status", status)
        .topology(
            Topology::new()
                .adjacency("op_outputs", outs)
                .adjacency("objects", objects)
                .adjacency("attributes", attrs)
                .adjacency("caches", caches),
        )
        .ints("handles", handles)
        .int("open_guards", guards)
        .ints("attr_dbl_bits", bits)
        .values("attr_dbls", dbls.to_vec()))
}

fn run_cpp_script(v: &Value) -> Result<Output, HarnessError> {
    let s = decode(v)?;
    match ffi::run(&s.objects, s.user_cb, s.sys_cb, &s.ops, &s.payload_d) {
        Ok(raw) => to_output(s.ops.len(), &raw.ints, &raw.dbls),
        Err(e @ FfiError::InvalidArgument(_)) => Err(h(format!("ffi rejected: {e}"))),
        Err(e) => Err(h(format!("shim failed: {e}"))),
    }
}

// ---- Rust side --------------------------------------------------------------------

#[derive(Debug)]
struct Recorder {
    tag: i64,
    out: Arc<Mutex<Vec<i64>>>,
}

fn idx(r: Option<ObjectRef>) -> i64 {
    r.map_or(-1, |r| r.key.0 as i64)
}

impl NotifyCallback for Recorder {
    fn execute(
        &mut self,
        operation: NotifyOperation,
        caller: ObjectRef,
        data1: Option<ObjectRef>,
        data2: Option<ObjectRef>,
        data3: Option<ObjectRef>,
    ) -> solidcore::Result<()> {
        if let Ok(mut v) = self.out.lock() {
            v.extend([
                self.tag,
                i64::from(operation.code()),
                caller.key.0 as i64,
                caller.type_code,
                idx(data1),
                idx(data2),
                idx(data3),
            ]);
        }
        // ignored by Notify
        Err(SolidError::Upstream {
            operation: "recorder",
            code: solidcore::error::SM_ERR,
            entity: None,
        })
    }
}

struct Obj {
    kind: i64,
    r: ObjectRef,
    a: AObject,
}

struct RustRun {
    store: AttributeStore,
    objs: Vec<Obj>,
    handles: Vec<AttributeHandle>,
    consumed: Vec<bool>,
    caches: CacheStore<()>,
    cache_ids: Vec<CacheId>,
    guards: Vec<CacheCheckOutIn>,
    ctx: Context,
    rec: Arc<Mutex<Vec<i64>>>,
    pay_d: std::vec::IntoIter<f64>,
}

enum Step {
    Status(i64),
    Deferred(Output),
}

fn type_code_of(kind: i64) -> i64 {
    match kind {
        O_REGION => a_object::REGION_TYPE,
        O_AOBJECT => a_object::TYPE,
        _ => a_object::BREP_TYPE,
    }
}

fn kind_of(a: &Attribute) -> i64 {
    match a.value() {
        AttributeValue::None => K_PLAIN,
        AttributeValue::Long(_) => K_LONG,
        AttributeValue::Vector3d(_) => K_VECTOR3D,
        AttributeValue::Color(_) => K_COLOR,
        AttributeValue::MergeRegion(_) => K_MERGE_REGION,
        _ => K_OTHER,
    }
}

fn behavior(code: i64) -> Result<Behavior, HarnessError> {
    Behavior::from_code(code as i32).map_err(|e| h(e.to_string()))
}

fn notify_op(code: i64) -> Result<NotifyOperation, HarnessError> {
    NotifyOperation::from_code(code as i32).ok_or_else(|| h(format!("notify op {code}")))
}

fn cache_type(code: i64) -> Result<ObjectCacheType, HarnessError> {
    ObjectCacheType::from_code(code as i32).ok_or_else(|| h(format!("cache type {code}")))
}

impl RustRun {
    fn key(i: usize) -> ObjectKey {
        ObjectKey(i as u64)
    }

    fn obj(&self, i: i64) -> Result<&Obj, HarnessError> {
        usize::try_from(i)
            .ok()
            .and_then(|i| self.objs.get(i))
            .ok_or_else(|| h(format!("object {i}")))
    }

    fn data(&self, i: i64) -> Result<Option<ObjectRef>, HarnessError> {
        if i < 0 {
            return Ok(None);
        }
        Ok(Some(self.obj(i)?.r))
    }

    fn attr(&self, a: AttributeHandle) -> Result<&Attribute, HarnessError> {
        self.store.get(a).ok_or_else(|| h("dead attribute reached"))
    }

    fn list(&self, o: usize) -> Vec<AttributeHandle> {
        self.store.attributes(Self::key(o))
    }

    fn resolve(&self, r: i64) -> Option<AttributeHandle> {
        if r >= 0 {
            let i = usize::try_from(r).ok()?;
            if *self.consumed.get(i)? {
                return None;
            }
            return self.handles.get(i).copied();
        }
        let k = -r - 1;
        let (o, i) = (k / REF_STRIDE, k % REF_STRIDE);
        let o = usize::try_from(o).ok()?;
        if o >= self.objs.len() {
            return None;
        }
        self.list(o).get(usize::try_from(i).ok()?).copied()
    }

    fn cache_arg(&self, c: i64) -> Result<Option<CacheId>, HarnessError> {
        if c == -1 {
            return Ok(None);
        }
        usize::try_from(c)
            .ok()
            .and_then(|i| self.cache_ids.get(i))
            .copied()
            .map(Some)
            .ok_or_else(|| h(format!("cache {c}")))
    }

    fn new_brep_cache(&mut self) -> solidcore::Result<CacheId> {
        use solidcore::topology::brep_cache::{arena_identity, make_brep_cache, CacheSource};
        let brep = solidcore::topology::Brep::new();
        let _cache = make_brep_cache(&solidcore::kernel::Kernel::default(), CacheSource::Brep(&brep), &arena_identity)?;
        // This runner compares cache lifetime metadata only; geometry is covered
        // by topology.brep_cache. Fresh native caches are not in cache_ids either.
        self.caches.insert(())
    }

    fn cache_index(&self, c: Option<CacheId>) -> i64 {
        match c {
            None => -1,
            Some(c) => self
                .cache_ids
                .iter()
                .position(|&x| x == c)
                .map_or(-2, |p| p as i64),
        }
    }

    fn has_id(&self, o: usize, id: u64) -> Result<bool, HarnessError> {
        for a in self.list(o) {
            if self.attr(a)?.attribute_id() == id {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn notify_is_safe(&self, op: NotifyOperation, caller: i64, d: [i64; 3]) -> Result<bool, HarnessError> {
        use NotifyOperation as N;
        let [d1, d2, d3] = d;
        Ok(match op {
            N::SplitInBrep => d2 >= 0 && d3 >= 0,
            N::MergeInBrep | N::Merge => {
                if d1 < 0 || d2 < 0 {
                    return Ok(false);
                }
                !self.has_id(d1 as usize, ids::OTHER_REGION_ID)?
                    && !self.has_id(d2 as usize, ids::OTHER_REGION_ID)?
            }
            N::Coincident | N::Copy => d1 >= 0,
            N::Split => d1 >= 0 && d2 >= 0,
            N::RegPropagation => {
                let c = self.obj(caller)?;
                if c.kind != O_REGION {
                    return Ok(true);
                }
                let Some(m) = self.store.find_attribute(c.r.key, ids::OTHER_REGION_ID) else {
                    return Ok(false);
                };
                let Some(mr) = self.attr(m)?.merge_region_data() else {
                    return Ok(false);
                };
                let others = mr.other_regions().to_vec();
                if others.len() > 1 {
                    return Ok(false);
                }
                let l = self.list(caller as usize);
                let mut sorted = l.clone();
                sorted.sort();
                sorted.dedup();
                if sorted.len() != l.len() {
                    return Ok(false);
                }
                let mut regions = vec![caller as usize];
                for o in others {
                    let oi = o.0 as usize;
                    if oi >= self.objs.len() || oi == caller as usize {
                        return Ok(false);
                    }
                    regions.push(oi);
                }
                for o in regions {
                    for a in self.list(o) {
                        let at = self.attr(a)?;
                        if at.attribute_id() == ids::COLOR
                            && !matches!(kind_of(at), K_VECTOR3D | K_COLOR)
                        {
                            return Ok(false);
                        }
                    }
                }
                true
            }
            _ => true,
        })
    }

    fn records(&self) -> Vec<i64> {
        self.rec
            .lock()
            .map(|mut v| std::mem::take(&mut *v))
            .unwrap_or_default()
    }

    fn live(&self, c: Option<CacheId>) -> bool {
        c.is_none_or(|c| self.caches.is_live(c))
    }

    fn run_op(&mut self, a: [i64; 8], out: &mut Vec<i64>) -> Result<Step, HarnessError> {
        let [code, a1, a2, a3, a4, a5, ..] = a;
        let fail = |e: SolidError| -> Result<Step, HarnessError> {
            match Output::unported(&e) {
                Some(d) => Ok(Step::Deferred(d)),
                None => Err(h(format!("rust error: {e}"))),
            }
        };
        macro_rules! tryr {
            ($e:expr) => {
                match $e {
                    Ok(v) => v,
                    Err(e) => return fail(e),
                }
            };
        }
        match code {
            op::NEW_PLAIN | op::NEW_LONG | op::NEW_VECTOR3D | op::NEW_COLOR | op::NEW_MERGE_REGION => {
                let at = match code {
                    op::NEW_PLAIN => Attribute::new(a1 as u64, behavior(a2)?),
                    op::NEW_LONG => Attribute::long(a1 as u64, a2, behavior(a3)?),
                    op::NEW_VECTOR3D | op::NEW_COLOR => {
                        let mut v = [0.0; 3];
                        for x in &mut v {
                            *x = self.pay_d.next().ok_or_else(|| h("payload"))?;
                        }
                        let v = Vec3::new(v[0], v[1], v[2]);
                        if code == op::NEW_COLOR {
                            Attribute::color(a1 as u64, v, behavior(a2)?)
                        } else {
                            Attribute::vector3d(a1 as u64, v, behavior(a2)?)
                        }
                    }
                    _ => {
                        if self.obj(a1)?.kind != O_REGION || self.obj(a3)?.kind != O_REGION {
                            return Err(h("merge-region attribute on a non-region"));
                        }
                        Attribute::merge_region(ObjectKey(a1 as u64), a2, ObjectKey(a3 as u64), a4)
                    }
                };
                let hd = tryr!(self.store.insert(at));
                self.handles.push(hd);
                self.consumed.push(false);
                out.push(self.handles.len() as i64 - 1);
                Ok(Step::Status(0))
            }
            op::MR_SET_OPERATION => {
                let Some(at) = self.resolve(a1) else {
                    return Ok(Step::Status(ST_INVALID));
                };
                let Some(d) = self.store.get_mut(at).and_then(Attribute::merge_region_data_mut) else {
                    return Ok(Step::Status(ST_SKIP));
                };
                d.set_operation(a2 as i32);
                Ok(Step::Status(0))
            }
            op::ADD | op::REMOVE => {
                let key = self.obj(a1)?.r.key;
                let Some(at) = self.resolve(a2) else {
                    return Ok(Step::Status(ST_INVALID));
                };
                if code == op::ADD {
                    if let Some(c) = usize::try_from(a2).ok().and_then(|i| self.consumed.get_mut(i)) {
                        *c = true;
                    }
                    tryr!(self.store.add_attribute(key, at));
                } else {
                    tryr!(self.store.remove_attribute(key, at, a3 != 0));
                }
                Ok(Step::Status(0))
            }
            op::CLEAR => {
                let key = self.obj(a1)?.r.key;
                tryr!(self.store.clear_attributes(key));
                Ok(Step::Status(0))
            }
            op::FIND => {
                let key = self.obj(a1)?.r.key;
                let f = self.store.find_attribute(key, a2 as u64);
                let l = self.store.attributes(key);
                out.push(f.and_then(|f| l.iter().position(|&x| x == f)).map_or(-1, |p| p as i64));
                Ok(Step::Status(0))
            }
            op::NOTIFY | op::SAG_NOTIFY => {
                let opn = notify_op(a1)?;
                let this = self.obj(a2)?.r;
                for d in [a3, a4, a5] {
                    if d < -1 || d >= self.objs.len() as i64 {
                        return Err(h("notify data index"));
                    }
                }
                let (d1, d2, d3) = (self.data(a3)?, self.data(a4)?, self.data(a5)?);
                if code == op::SAG_NOTIFY {
                    use NotifyOperation as N;
                    if self.obj(a2)?.kind != O_BREP
                        || d1.is_some()
                        || d2.is_some()
                        || d3.is_some()
                        || !matches!(opn, N::PreEdit | N::PostEdit | N::ChangeOwner | N::Unknown)
                    {
                        return Err(h("sag_notify arguments"));
                    }
                    // SmTopology::Notify touches no record for these operations.
                    let mut records = std::collections::BTreeMap::<
                        ObjectKey,
                        solidcore::topology::topology::Topology,
                    >::new();
                    tryr!(a_object::sag_notify(
                        &mut records,
                        &mut self.store,
                        Some(&mut self.ctx),
                        opn,
                        this,
                        d1,
                        d2,
                        d3
                    ));
                } else {
                    if !self.notify_is_safe(opn, a2, [a3, a4, a5])? {
                        return Ok(Step::Status(ST_SKIP));
                    }
                    tryr!(a_object::notify(
                        &mut self.store,
                        Some(&mut self.ctx),
                        opn,
                        this,
                        d1,
                        d2,
                        d3
                    ));
                }
                let rec = self.records();
                out.push(rec.len() as i64 / 7);
                out.extend(rec);
                Ok(Step::Status(0))
            }
            op::COPY_CONSTRUCT => {
                let src = self.obj(a1)?.r.key;
                if self.objs.len() >= MAX_OBJECTS {
                    return Ok(Step::Status(ST_SKIP));
                }
                let new = Self::key(self.objs.len());
                self.objs.push(Obj {
                    kind: O_AOBJECT,
                    r: ObjectRef {
                        type_code: a_object::TYPE,
                        key: new,
                    },
                    a: AObject::new(),
                });
                tryr!(a_object::copy_construct(&mut self.store, src, new));
                out.push(self.objs.len() as i64 - 1);
                Ok(Step::Status(0))
            }
            op::ASSERT_VALID => {
                self.obj(a1)?;
                out.push(i64::from(a_object::assert_valid(a2 != 0)));
                Ok(Step::Status(0))
            }
            op::NEW_CACHE => {
                let c = tryr!(self.caches.insert(()));
                self.cache_ids.push(c);
                out.push(self.cache_ids.len() as i64 - 1);
                Ok(Step::Status(0))
            }
            op::SET_CACHE => {
                let c = self.cache_arg(a2)?;
                self.obj(a1)?;
                if let Some(o) = self.objs.get_mut(a1 as usize) {
                    o.a.set_cache_obj(c);
                }
                Ok(Step::Status(0))
            }
            op::GET_CACHE => {
                let ct = cache_type(a2)?;
                let o = if a1 == -1 { None } else { Some(self.obj(a1)?.a) };
                out.push(self.cache_index(a_object::get_object_cache(ct, o.as_ref())));
                Ok(Step::Status(0))
            }
            op::CHECK_OUT | op::CHECK_IN | op::SOFT_DELETE | op::GUARD_OPEN => {
                let c = self.cache_arg(a1)?;
                if !self.live(c) {
                    return Ok(Step::Status(ST_SKIP));
                }
                match code {
                    op::CHECK_OUT => tryr!(self.caches.check_out_cache(c)),
                    op::CHECK_IN => tryr!(self.caches.check_in_cache(c)),
                    op::SOFT_DELETE => {
                        let c = c.ok_or_else(|| h("soft_delete of NULL"))?;
                        tryr!(self.caches.soft_delete(c));
                    }
                    _ => {
                        let g = tryr!(CacheCheckOutIn::new(&mut self.caches, c));
                        self.guards.push(g);
                    }
                }
                Ok(Step::Status(0))
            }
            op::GUARD_CLOSE => {
                let Some(g) = self.guards.pop() else {
                    return Ok(Step::Status(ST_SKIP));
                };
                if !self.live(g.cache()) {
                    return Ok(Step::Status(ST_SKIP));
                }
                tryr!(g.release(&mut self.caches));
                Ok(Step::Status(0))
            }
            op::DELETE_OBJECTS_CACHE => {
                cache_type(a3)?;
                let cur = self.obj(a1)?.a.cache_obj();
                if !self.live(cur) {
                    return Ok(Step::Status(ST_SKIP));
                }
                let mut o = self.obj(a1)?.a;
                tryr!(a_object::delete_objects_cache(&mut self.caches, &mut o));
                if let Some(x) = self.objs.get_mut(a1 as usize) {
                    x.a = o;
                }
                Ok(Step::Status(0))
            }
            op::GET_OR_CREATE => {
                let ct = cache_type(a2)?;
                let o = self.obj(a1)?;
                let maker = if o.kind == O_BREP {
                    CacheMaker::SagObject
                } else {
                    CacheMaker::AObject
                };
                let needs_build = o.kind == O_BREP && o.a.cache_obj().is_none();
                let c = if needs_build {
                    let c = tryr!(self.new_brep_cache());
                    if let Some(o) = self.objs.get_mut(a1 as usize) {
                        o.a.set_cache_obj(Some(c));
                    }
                    Some(c)
                } else {
                    tryr!(a_object::get_or_create_object_cache(
                        ct,
                        Some(&self.ctx),
                        maker,
                        o.r.type_code,
                        &o.a
                    ))
                };
                out.push(self.cache_index(c));
                Ok(Step::Status(0))
            }
            op::SAG_MAKE => {
                let ct = cache_type(a4)?;
                if self.obj(a1)?.kind != O_BREP {
                    return Err(h("sag_make on a non-B-rep"));
                }
                let target = self.obj(a2)?.r.type_code;
                let old = self.cache_arg(a3)?;
                let made = if target == a_object::BREP_TYPE && old.is_none() {
                    self.new_brep_cache().map(Some)
                } else {
                    a_object::sag_cache_make_or_validate(ct, target, old)
                };
                match made {
                    Ok(c) => {
                        out.push(self.cache_index(c));
                        Ok(Step::Status(0))
                    }
                    Err(SolidError::Upstream { code, .. }) => {
                        out.push(-1);
                        Ok(Step::Status(i64::from(code)))
                    }
                    Err(e) => fail(e),
                }
            }
            _ => Err(h(format!("op code {code}"))),
        }
    }

    fn dump(&self, ints: &mut Vec<i64>, dbls: &mut Vec<f64>) -> Result<(), HarnessError> {
        let mut canon: Vec<AttributeHandle> = Vec::new();
        let see = |canon: &mut Vec<AttributeHandle>, a: AttributeHandle| {
            if !canon.contains(&a) {
                canon.push(a);
            }
        };
        let lists: Vec<Vec<AttributeHandle>> = (0..self.objs.len()).map(|o| self.list(o)).collect();
        for l in &lists {
            for &a in l {
                see(&mut canon, a);
            }
        }
        for (hd, c) in self.handles.iter().zip(&self.consumed) {
            if !c {
                see(&mut canon, *hd);
            }
        }
        let index = |a: AttributeHandle| canon.iter().position(|&x| x == a).map_or(-1, |p| p as i64);
        ints.push(self.objs.len() as i64);
        for (o, l) in self.objs.iter().zip(&lists) {
            ints.push(o.r.type_code);
            ints.push(l.len() as i64);
            ints.extend(l.iter().map(|&a| index(a)));
            ints.push(self.cache_index(o.a.cache_obj()));
        }
        ints.push(canon.len() as i64);
        for &a in &canon {
            let at = self.attr(a)?;
            ints.push(kind_of(at));
            ints.push(at.attribute_id() as i64);
            ints.push(i64::from(at.behavior().code()));
            let users = self
                .store
                .users(a)
                .map_err(|e| h(e.to_string()))?;
            ints.push(users.len() as i64);
            ints.extend(users.iter().map(|u| u.0 as i64));
            let mut v = Vec::new();
            let mut d = Vec::new();
            match at.value() {
                AttributeValue::Long(x) => v.push(*x),
                AttributeValue::Vector3d(x) | AttributeValue::Color(x) => d.extend([x.x, x.y, x.z]),
                AttributeValue::MergeRegion(m) => {
                    v.push(i64::from(m.operation()));
                    v.push(m.this_regions().len() as i64);
                    v.extend(m.this_regions().iter().map(|r| r.0 as i64));
                    v.extend(m.this_region_types().iter().copied());
                    v.push(m.other_regions().len() as i64);
                    v.extend(m.other_regions().iter().map(|r| r.0 as i64));
                    v.extend(m.other_types.iter().copied());
                }
                _ => {}
            }
            ints.push(v.len() as i64);
            ints.extend(v);
            ints.push(d.len() as i64);
            dbls.extend(d);
        }
        ints.push(self.handles.len() as i64);
        for (hd, c) in self.handles.iter().zip(&self.consumed) {
            ints.push(if *c { -1 } else { index(*hd) });
        }
        ints.push(self.cache_ids.len() as i64);
        for &c in &self.cache_ids {
            match self.caches.get(c) {
                Some(x) => ints.extend([1, x.use_count() as i64, i64::from(x.is_deleted())]),
                None => ints.extend([0, -1, -1]),
            }
        }
        ints.push(self.guards.len() as i64);
        Ok(())
    }
}

fn run_rust_script(v: &Value) -> Result<Output, HarnessError> {
    let s = decode(v)?;
    if s.objects.len() > MAX_OBJECTS || s.objects.iter().any(|k| !(0..=2).contains(k)) {
        return Err(h("object kinds"));
    }
    let rec = Arc::new(Mutex::new(Vec::new()));
    let mut ctx = Context::default();
    if s.user_cb {
        ctx.set_user_notify_callback(Some(Box::new(Recorder {
            tag: 1,
            out: Arc::clone(&rec),
        })));
    }
    if s.sys_cb {
        ctx.set_sys_notify_callback(Some(Box::new(Recorder {
            tag: 2,
            out: Arc::clone(&rec),
        })));
    }
    let objs = s
        .objects
        .iter()
        .enumerate()
        .map(|(i, &k)| Obj {
            kind: k,
            r: ObjectRef {
                type_code: type_code_of(k),
                key: ObjectKey(i as u64),
            },
            a: AObject::new(),
        })
        .collect();
    let mut run = RustRun {
        store: AttributeStore::new(),
        objs,
        handles: Vec::new(),
        consumed: Vec::new(),
        caches: CacheStore::new(),
        cache_ids: Vec::new(),
        guards: Vec::new(),
        ctx,
        rec,
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
    to_output(s.ops.len(), &ints, &dbls)
}

// ---- generator --------------------------------------------------------------------

const PLAIN_IDS: &[i64] = &[5, 6, 106, 116, 10000];
const VECTOR_IDS: &[i64] = &[100, 100, 7, 120];

struct Gen<'a> {
    rng: &'a mut Rng,
    n_obj: i64,
    regions: Vec<i64>,
    breps: Vec<i64>,
    n_handles: i64,
    n_caches: i64,
    ops: Vec<Value>,
}

impl Gen<'_> {
    fn pick(&mut self, v: &[i64]) -> i64 {
        self.rng.choose(v).copied().unwrap_or(0)
    }

    fn obj(&mut self) -> i64 {
        self.rng.range_i64(0, self.n_obj - 1)
    }

    fn data(&mut self) -> i64 {
        if self.rng.chance(0.15) {
            -1
        } else {
            self.obj()
        }
    }

    fn beh(&mut self) -> i64 {
        self.rng.range_i64(0, 4)
    }

    fn reference(&mut self) -> i64 {
        if self.n_handles > 0 && self.rng.chance(0.5) {
            self.rng.range_i64(0, self.n_handles - 1)
        } else {
            let o = self.obj();
            let i = self.rng.range_i64(0, 3);
            -(o * REF_STRIDE + i) - 1
        }
    }

    fn cache(&mut self) -> i64 {
        if self.n_caches == 0 || self.rng.chance(0.1) {
            -1
        } else {
            self.rng.range_i64(0, self.n_caches - 1)
        }
    }

    fn vec3(&mut self) -> Value {
        let p = self.rng.point3(-4.0, 4.0);
        json!([p[0], p[1], p[2]])
    }

    fn push(&mut self, v: Value) {
        self.ops.push(v);
    }

    fn create(&mut self) {
        match self.rng.below(10) {
            0..=2 => {
                let (id, b) = (self.pick(PLAIN_IDS), self.beh());
                self.push(json!(["new_plain", id, b]));
            }
            3..=4 => {
                let (id, val, b) = (self.pick(PLAIN_IDS), self.rng.range_i64(-50, 50), self.beh());
                self.push(json!(["new_long", id, val, b]));
            }
            5..=6 => {
                let (id, b, v) = (self.pick(VECTOR_IDS), self.beh(), self.vec3());
                self.push(json!(["new_vector3d", id, b, v]));
            }
            7..=8 => {
                let (id, b, v) = (self.pick(VECTOR_IDS), self.beh(), self.vec3());
                self.push(json!(["new_color", id, b, v]));
            }
            _ => {
                if self.regions.len() < 2 {
                    self.push(json!(["new_plain", 5, 0]));
                } else {
                    let r = self.regions.clone();
                    let a = self.pick(&r);
                    let b = self.pick(&r);
                    let (ta, tb) = (self.rng.range_i64(0, 3), self.rng.range_i64(0, 3));
                    self.push(json!(["new_merge_region", a, ta, b, tb]));
                }
            }
        }
        self.n_handles += 1;
    }

    fn notify(&mut self) {
        let opn = self.rng.range_i64(0, 16);
        let caller = if opn == 14 && !self.regions.is_empty() && self.rng.chance(0.8) {
            let r = self.regions.clone();
            self.pick(&r)
        } else {
            self.obj()
        };
        let mut d = [self.data(), self.data(), self.data()];
        // mostly keep the data the handlers need
        if self.rng.chance(0.9) {
            for x in &mut d {
                if *x < 0 {
                    *x = self.obj();
                }
            }
        }
        // children / targets often coincide with the caller (reused objects)
        if self.rng.chance(0.15) {
            d[0] = caller;
        }
        self.push(json!(["notify", opn, caller, d[0], d[1], d[2]]));
    }

    fn step(&mut self) {
        match self.rng.below(100) {
            0..=14 => self.create(),
            15..=29 => {
                let (o, r) = (self.obj(), self.reference());
                self.push(json!(["add", o, r]));
            }
            30..=33 => {
                let (o, r, d) = (self.obj(), self.reference(), i64::from(self.rng.chance(0.3)));
                self.push(json!(["remove", o, r, d]));
            }
            34..=35 => {
                let o = self.obj();
                self.push(json!(["clear", o]));
            }
            36..=37 => {
                let (o, id) = (self.obj(), self.pick(&[5, 100, 106, 114, 7]));
                self.push(json!(["find", o, id]));
            }
            38..=61 => self.notify(),
            62..=64 => {
                let src = self.obj();
                self.push(json!(["copy_construct", src]));
                self.n_obj += 1;
            }
            65..=67 => {
                let r = self.reference();
                let opn = self.rng.range_i64(0, 10);
                self.push(json!(["mr_set_operation", r, opn]));
            }
            68 => {
                let (o, l) = (self.obj(), i64::from(self.rng.chance(0.5)));
                self.push(json!(["assert_valid", o, l]));
            }
            69..=72 => {
                self.push(json!(["new_cache"]));
                self.n_caches += 1;
            }
            73..=76 => {
                let (o, c) = (self.obj(), self.cache());
                self.push(json!(["set_cache", o, c]));
            }
            77..=78 => {
                let o = if self.rng.chance(0.1) { -1 } else { self.obj() };
                let t = self.rng.range_i64(0, 3);
                self.push(json!(["get_cache", o, t]));
            }
            79..=88 => {
                let names = ["check_out", "check_in", "soft_delete", "guard_open", "guard_close"];
                let n = *self.rng.choose(&names).unwrap_or(&"check_out");
                if n == "guard_close" {
                    self.push(json!(["guard_close"]));
                } else {
                    let mut c = self.cache();
                    if n == "soft_delete" && c < 0 {
                        if self.n_caches == 0 {
                            return;
                        }
                        c = self.rng.range_i64(0, self.n_caches - 1);
                    }
                    self.push(json!([n, c]));
                }
            }
            89..=92 => {
                let (o, v, t) = (self.obj(), self.rng.range_i64(0, 1), self.rng.range_i64(0, 3));
                self.push(json!(["delete_objects_cache", o, v, t]));
            }
            93..=95 => {
                let (o, t) = (self.obj(), self.rng.range_i64(0, 3));
                // a B-rep without a cache would build an (unported) SmBrepCache
                if self.breps.contains(&o) && self.rng.chance(0.7) && self.n_caches > 0 {
                    let c = self.rng.range_i64(0, self.n_caches - 1);
                    self.push(json!(["set_cache", o, c]));
                }
                self.push(json!(["get_or_create", o, t]));
            }
            _ => {
                if self.breps.is_empty() {
                    return;
                }
                let b = self.breps.clone();
                let this = self.pick(&b);
                let target = self.obj();
                let old = if self.n_caches > 0 && self.rng.chance(0.8) {
                    self.rng.range_i64(0, self.n_caches - 1)
                } else {
                    -1
                };
                let t = self.rng.range_i64(0, 3);
                self.push(json!(["sag_make", this, target, old, t]));
            }
        }
    }
}

/// A region-propagation scenario: regions with a merge-region attribute (this
/// region, one other region), propagated colour attributes on either side and other
/// attributes, a Boolean operation, then `SM_NO_REG_PROPAGATION` on the result region.
fn reg_prop_case(rng: &mut Rng) -> Value {
    let n = 2 + rng.below(3) as i64;
    let objects: Vec<i64> = (0..n).map(|i| if i < 2 || rng.chance(0.7) { O_REGION } else { O_AOBJECT }).collect();
    let mut ops: Vec<Value> = Vec::new();
    let mut handles = 0i64;
    let mut new_handle = |ops: &mut Vec<Value>, v: Value| {
        ops.push(v);
        handles += 1;
        handles - 1
    };
    let (this, other) = if rng.chance(0.8) { (0, 1) } else { (1, 0) };
    if rng.chance(0.9) {
        let t = rng.range_i64(0, 3);
        let mr = new_handle(&mut ops, json!(["new_merge_region", this, rng.range_i64(0, 3), other, t]));
        let opn = rng.range_i64(0, 10);
        ops.push(json!(["mr_set_operation", mr, opn]));
        ops.push(json!(["add", this, mr]));
    }
    for _ in 0..rng.below(5) {
        let target = if rng.chance(0.5) { this } else { other };
        let b = rng.range_i64(0, 4);
        let p = rng.point3(0.0, 1.0);
        let v = match rng.below(4) {
            0 => json!(["new_color", 100, b, [p[0], p[1], p[2]]]),
            1 => json!(["new_vector3d", 7, b, [p[0], p[1], p[2]]]),
            2 => json!(["new_long", *rng.choose(&[5, 6, 106]).unwrap_or(&5), rng.range_i64(-9, 9), b]),
            _ => json!(["new_plain", *rng.choose(&[5, 116]).unwrap_or(&5), b]),
        };
        let hd = new_handle(&mut ops, v);
        ops.push(json!(["add", target, hd]));
    }
    let caller = if rng.chance(0.9) { this } else { rng.range_i64(0, n - 1) };
    ops.push(json!(["notify", 14, caller, -1, -1, -1]));
    if rng.chance(0.3) {
        ops.push(json!(["notify", 14, caller, -1, -1, -1]));
    }
    json!({"kind": "script", "objects": objects, "user_cb": rng.chance(0.5), "sys_cb": rng.chance(0.5), "ops": ops})
}

fn generate_case(rng: &mut Rng) -> Value {
    match rng.below(20) {
        4 => reg_prop_case(rng),
        0 => {
            let n = 1 + rng.below(12) as usize;
            let pool = [0, 11001, 12060, 16050, 16011, 16012, 12000, 12040, 16000, 18001, -1, 999_999];
            let ts: Vec<i64> = (0..n)
                .map(|_| {
                    if rng.chance(0.7) {
                        *rng.choose(&pool).unwrap_or(&0)
                    } else {
                        rng.range_i64(10_000, 20_000)
                    }
                })
                .collect();
            json!({"kind": "types", "ts": ts})
        }
        1..=3 => {
            let c4 = |rng: &mut Rng| {
                let p = rng.point3(-10.0, 10.0);
                let w = if rng.chance(0.1) {
                    rng.uniform(-2.0, 2.0)
                } else {
                    rng.uniform(0.1, 3.0)
                };
                json!([p[0], p[1], p[2], w])
            };
            let (pw, aw, bw) = (c4(rng), c4(rng), c4(rng));
            json!({"kind": "nli", "noz": rng.chance(0.5), "now": rng.chance(0.5),
                   "pw": pw, "alpha": rng.uniform(-3.0, 3.0), "aw": aw, "bw": bw})
        }
        _ => {
            let n = 2 + rng.below(6) as i64;
            let objects: Vec<i64> = (0..n)
                .map(|_| match rng.below(10) {
                    0..=5 => O_REGION,
                    6..=8 => O_AOBJECT,
                    _ => O_BREP,
                })
                .collect();
            let regions = (0..n).filter(|&i| objects.get(i as usize) == Some(&O_REGION)).collect();
            let breps = (0..n).filter(|&i| objects.get(i as usize) == Some(&O_BREP)).collect();
            let user_cb = rng.chance(0.6);
            let sys_cb = rng.chance(0.4);
            let n_steps = 4 + rng.below(36);
            let mut g = Gen {
                rng,
                n_obj: n,
                regions,
                breps,
                n_handles: 0,
                n_caches: 0,
                ops: Vec::new(),
            };
            for _ in 0..n_steps {
                g.step();
            }
            json!({"kind": "script", "objects": objects, "user_cb": user_cb, "sys_cb": sys_cb, "ops": g.ops})
        }
    }
}

fn kind_str(v: &Value) -> Result<&str, HarnessError> {
    v.get("kind").and_then(Value::as_str).ok_or_else(|| h("kind"))
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "geom.a_object"
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
        let v = &case.inputs;
        match kind_str(v)? {
            "types" => {
                let ts = type_list(v)?;
                let r = ffi::types(&ts).map_err(|e| h(e.to_string()))?;
                Ok(Output::new().ints("types", r))
            }
            "nli" => {
                let n = nli_inputs(v)?;
                let r = ffi::nli(n.noz, n.now, n.pw, n.alpha, n.aw, n.bw).map_err(|e| h(e.to_string()))?;
                Ok(nli_output(r))
            }
            "script" => run_cpp_script(v),
            k => Err(h(format!("unknown case kind {k}"))),
        }
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let v = &case.inputs;
        match kind_str(v)? {
            "types" => Ok(Output::new().ints("types", types_rust(&type_list(v)?))),
            "nli" => Ok(nli_output(nli_rust(&nli_inputs(v)?))),
            "script" => run_rust_script(v),
            k => Err(h(format!("unknown case kind {k}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every op runs (status 0) in a good share of generated scripts.
    #[test]
    fn generated_scripts_exercise_every_op() {
        let mut ok = vec![0usize; OPS.len()];
        let mut deferred = 0;
        let mut scripts = 0;
        for seed in 0..3000u64 {
            let mut rng = Rng::new(seed);
            let v = generate_case(&mut rng);
            if v["kind"] != "script" {
                continue;
            }
            scripts += 1;
            let s = decode(&v).unwrap();
            let out = run_rust_script(&v).unwrap();
            if out.deferred.is_some() {
                deferred += 1;
                continue;
            }
            for (rec, st) in s.ops.iter().zip(&out.ints["status"]) {
                if *st == 0 {
                    ok[rec[0] as usize] += 1;
                }
            }
        }
        for (i, (name, _)) in OPS.iter().enumerate() {
            println!("{name:22} ok {:6}", ok[i]);
            if *name != "sag_notify" {
                assert!(ok[i] >= 20, "op {name} ran only {} times", ok[i]);
            }
        }
        println!("deferred {deferred} of {scripts}");
        assert!(deferred * 4 < scripts);
    }
}
