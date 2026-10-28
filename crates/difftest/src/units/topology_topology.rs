//! Runner for unit `topology.topology`: scripts over a pool of bare `SmTopology`
//! objects in one fresh context (flags, user values, marks under
//! `SmNewMarkAndLock`, `Notify` propagation and its pass-through to the context
//! callbacks, `GetTolerance`, `IsConnectedTo`, destruction) and
//! `SmTrackTopologyChanges` trackers of an empty B-rep (start / stop / is-tracking /
//! lists / reset / destruction, also with the system callback slot occupied).
//! `TRACK_LISTS` compares the rm, add and chg lists and the captured notify events;
//! `SET_KIND` gives a pool object a stand-in topology type code so the tracker's
//! `IsKindOf` classification is exercised (the object keeps `SmTopology` storage and
//! the final state dump asks the base class explicitly).
//!
//! C++: `solidcore_ffi::units::topology_topology` (upstream `SmTopology.cpp`,
//! `SmTrackTopologyChanges.cpp`). Rust: `solidcore::topology::topology`.
//!
//! Inputs: `n_objs` (pool size, 1..=32), `user_cb` (record the user notify
//! callback), `kind_ts` (type codes asked of `IsKindOf` at the end) and `ops`
//! (8-wide records, see `solidcore_ffi::units::topology_topology::op`). Output `i`:
//! per op `[status, n, values...]`, then the final state of every object, the
//! context's marks and locks, the system callback and the type system; `d`: the
//! tolerances asked for.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::context::{Context, MarkLock, MarkType, NotifyCallback};
use solidcore::geom::object::{NotifyOperation, ObjectRef};
use solidcore::topology::topology::{self as topo, Topology, TrackTopologyChanges};
use solidcore::BrepId;
use solidcore_ffi::units::topology_topology::{self as ffi, op, MAX_OBJECTS};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 600;

/// The op referred to a destroyed object or tracker, needed a lock when none was
/// held, or would destroy a tracker whose callback stays installed.
const ST_SKIP: i64 = -2;

/// Most trackers per script.
const MAX_TRACKERS: usize = 16;

/// Mark type codes (`SmMarkType`).
const MARK_TYPES: [i64; 7] = [0, 1, 2, 4, 8, 16, 31];

/// `topology.topology` runner.
pub struct Runner;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    n_objs: i64,
    #[serde(default)]
    user_cb: bool,
    #[serde(default)]
    kind_ts: Vec<i64>,
    ops: Vec<[i64; 8]>,
}

fn harness(e: impl std::fmt::Display) -> HarnessError {
    HarnessError::new(e.to_string())
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let raw = ffi::run(i.n_objs, i.user_cb, &i.kind_ts, &i.ops).map_err(harness)?;
    Ok(Output::new().ints("i", raw.ints).values("d", raw.dbls))
}

// ---- Rust side ---------------------------------------------------------------------

/// Records `Execute` calls like the shim's `Recorder`.
#[derive(Debug)]
struct Recorder {
    tag: i64,
    out: Arc<Mutex<Vec<i64>>>,
}

fn pool_index(r: Option<ObjectRef>) -> i64 {
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
                pool_index(data1),
                pool_index(data2),
                pool_index(data3),
            ]);
        }
        // the status is ignored by Notify (the shim returns SM_ERR too)
        Err(solidcore::SolidError::InvalidInput {
            operation: "difftest::topology_topology::Recorder",
            reason: "ignored status",
        })
    }

    fn as_any(&self) -> Option<&dyn core::any::Any> {
        Some(self)
    }
}

/// The handle of pool object `k` with its stand-in type code (see `SET_KIND`).
fn obj_ref(k: usize, kind: i64) -> ObjectRef {
    ObjectRef {
        type_code: kind,
        key: ObjectKey(k as u64),
    }
}

/// A captured list as `length, keys...` (the shim's object indices; `-1` null).
fn put_keys(out: &mut Vec<i64>, keys: &[ObjectKey]) {
    out.push(keys.len() as i64);
    out.extend(keys.iter().map(|k| k.0 as i64));
}

/// One captured event reference (the shim's object index; `-1` null).
fn put_event_ref(out: &mut Vec<i64>, object: Option<ObjectRef>) {
    out.push(object.map_or(-1, |o| o.key.0 as i64));
}

fn mark_type(m: i64) -> Result<MarkType, HarnessError> {
    i32::try_from(m)
        .ok()
        .and_then(MarkType::from_code)
        .ok_or_else(|| harness(format!("mark type {m}")))
}

struct State {
    ctxs: Vec<Context>,
    recs: solidcore::topology::topology::NotifyRecords,
    n: usize,
    /// Per object: its stand-in type code (`SET_KIND`; `SmTopology_TYPE` initially).
    /// The change tracker classifies notification objects by this code only.
    kinds: Vec<i64>,
    store: AttributeStore,
    locks: Vec<MarkLock>,
    trackers: Vec<Option<TrackTopologyChanges>>,
    rec: Arc<Mutex<Vec<i64>>>,
    dbls: Vec<f64>,
}

enum Step {
    Out(Vec<i64>),
    Skip,
}

impl State {
    fn ctx(&mut self) -> Result<&mut Context, HarnessError> {
        self.ctxs.first_mut().ok_or_else(|| harness("no context"))
    }

    /// `None` for null (`-1` when allowed); `Err(None)` a destroyed object.
    fn obj(&self, o: i64, allow_null: bool) -> Result<Result<Option<usize>, ()>, HarnessError> {
        if o == -1 && allow_null {
            return Ok(Ok(None));
        }
        if o < 0 || o as usize >= self.n {
            return Err(harness(format!("object {o}")));
        }
        let k = o as usize;
        Ok(if self.recs.contains_key(&ObjectKey(k as u64)) {
            Ok(Some(k))
        } else {
            Err(())
        })
    }

    fn rec_mut(&mut self, k: usize) -> Result<&mut Topology, HarnessError> {
        self.recs
            .get_mut(&ObjectKey(k as u64))
            .ok_or_else(|| harness("record"))
    }

    fn rec(&self, k: usize) -> Result<&Topology, HarnessError> {
        self.recs
            .get(&ObjectKey(k as u64))
            .ok_or_else(|| harness("record"))
    }

    fn ctx_ref(&self) -> Result<&Context, HarnessError> {
        self.ctxs.first().ok_or_else(|| harness("no context"))
    }

    fn user_ptr_index(v: u64) -> i64 {
        if v == 0 {
            -1
        } else {
            v as i64 - 1
        }
    }

    fn step(&mut self, a: &[i64; 8]) -> Result<Step, HarnessError> {
        macro_rules! live {
            ($o:expr, $null:expr) => {
                match self.obj($o, $null)? {
                    Ok(v) => v,
                    Err(()) => return Ok(Step::Skip),
                }
            };
        }
        macro_rules! need {
            ($v:expr) => {
                $v.ok_or_else(|| harness("null object"))?
            };
        }
        let mut out = Vec::new();
        match a[0] {
            op::SET_FLAGS => {
                let o = need!(live!(a[1], false));
                self.rec_mut(o)?.set_all_flags(a[2] as u64);
            }
            op::SET_KIND => {
                let o = need!(live!(a[1], false));
                let kind = a[2];
                if !matches!(kind, topo::TYPE | 16002 | 16005 | 16016 | 16021) {
                    return Err(harness("set kind type"));
                }
                if let Some(k) = self.kinds.get_mut(o) {
                    *k = kind;
                }
            }
            op::GET_FLAGS => {
                let o = need!(live!(a[1], false));
                out.push(self.rec(o)?.all_flags() as i64);
            }
            op::SET_UIDX1 | op::SET_UIDX2 => {
                let o = need!(live!(a[1], false));
                let r = self.rec_mut(o)?;
                if a[0] == op::SET_UIDX1 {
                    r.set_user_index1(a[2] as u64);
                } else {
                    r.set_user_index2(a[2] as u64);
                }
            }
            op::SET_UPTR => {
                let o = need!(live!(a[1], false));
                if a[2] < -1 || a[2] >= self.n as i64 {
                    return Err(harness("user pointer target"));
                }
                self.rec_mut(o)?.set_user_ptr1((a[2] + 1) as u64);
            }
            op::GET_USER => {
                let o = need!(live!(a[1], false));
                let r = self.rec(o)?;
                out.extend([
                    r.user_index1() as i64,
                    r.user_index2() as i64,
                    Self::user_ptr_index(r.user_ptr1()),
                ]);
            }
            op::MARK | op::GET_MARK | op::IS_MARKED => {
                let o = need!(live!(a[1], false));
                let m = mark_type(a[2])?;
                if a[0] == op::MARK {
                    let ctx = self.ctxs.first();
                    let r = self
                        .recs
                        .get_mut(&ObjectKey(o as u64))
                        .ok_or_else(|| harness("record"))?;
                    r.mark(ctx, m);
                } else if a[0] == op::GET_MARK {
                    out.push(self.rec(o)?.get_mark(m) as i64);
                } else {
                    let marked = self.rec(o)?.is_marked(self.ctx_ref()?, m);
                    out.push(i64::from(marked));
                }
            }
            op::LOCK => {
                if !(0..=31).contains(&a[1]) {
                    return Err(harness("lock mask"));
                }
                let l = MarkLock::new(&mut self.ctxs, 0, a[1] as i32).map_err(harness)?;
                out.push(i64::from(l.mark_type().code()));
                self.locks.push(l);
            }
            op::LOCK_NEW_MARK => {
                let Some(l) = self.locks.last() else {
                    return Ok(Step::Skip);
                };
                l.new_mark(&mut self.ctxs).map_err(harness)?;
            }
            op::UNLOCK => {
                let Some(mut l) = self.locks.pop() else {
                    return Ok(Step::Skip);
                };
                l.release(&mut self.ctxs).map_err(harness)?;
            }
            op::NOTIFY => {
                let opn = i32::try_from(a[1])
                    .ok()
                    .and_then(NotifyOperation::from_code)
                    .ok_or_else(|| harness("notify operation"))?;
                let kind_of = |k: usize| self.kinds.get(k).copied().unwrap_or(topo::TYPE);
                let t = need!(live!(a[2], false));
                let caller = obj_ref(t, kind_of(t));
                let d1 = live!(a[3], true).map(|k| obj_ref(k, kind_of(k)));
                let d2 = live!(a[4], true).map(|k| obj_ref(k, kind_of(k)));
                let d3 = live!(a[5], true).map(|k| obj_ref(k, kind_of(k)));
                self.rec.lock().map_err(harness)?.clear();
                let ctx = self.ctxs.first_mut();
                if let Err(e) = topo::notify(
                    &mut self.recs,
                    &mut self.store,
                    ctx,
                    opn,
                    caller,
                    d1,
                    d2,
                    d3,
                ) {
                    return Err(harness(format!("rust notify failed: {e}")));
                }
                let r = self.rec.lock().map_err(harness)?;
                out.push((r.len() / 7) as i64);
                out.extend(r.iter().copied());
            }
            op::TOLERANCE => {
                let o = need!(live!(a[1], false));
                let tol = self.rec(o)?.tolerance();
                self.dbls.push(tol);
            }
            op::CONNECTED => {
                let o = need!(live!(a[1], false));
                let p = live!(a[2], true).map(|k| ObjectKey(k as u64));
                out.push(i64::from(self.rec(o)?.is_connected_to(p)));
            }
            op::TRACK_NEW => {
                if self.trackers.len() >= MAX_TRACKERS {
                    return Ok(Step::Skip);
                }
                let key = self.trackers.len() as u64;
                let brep = (a[2] != 0).then(|| BrepId::from_index(0));
                let t = TrackTopologyChanges::new(brep, key, a[1] != 0, Some(self.ctx()?));
                self.trackers.push(Some(t));
                out.push(key as i64);
            }
            op::TRACK_START
            | op::TRACK_STOP
            | op::TRACK_IS
            | op::TRACK_LISTS
            | op::TRACK_RESET
            | op::TRACK_DROP => {
                if a[1] < 0 || a[1] as usize >= self.trackers.len() {
                    return Err(harness("tracker index"));
                }
                let ti = a[1] as usize;
                let Some(mut t) = self.trackers.get_mut(ti).and_then(Option::take) else {
                    return Ok(Step::Skip);
                };
                // destroying a tracker whose callback would stay installed leaves a
                // dangling callback upstream: skipped on both sides
                if a[0] == op::TRACK_DROP
                    && t.is_tracking(Some(self.ctx_ref()?))
                    && t.last_notify_callback()
                {
                    if let Some(slot) = self.trackers.get_mut(ti) {
                        *slot = Some(t);
                    }
                    return Ok(Step::Skip);
                }
                let mut keep = true;
                match a[0] {
                    op::TRACK_START => t.start_tracking(Some(self.ctx()?)),
                    op::TRACK_STOP => t.stop_tracking(Some(self.ctx()?)),
                    op::TRACK_IS => out.push(i64::from(t.is_tracking(Some(self.ctx_ref()?)))),
                    op::TRACK_LISTS => {
                        let lists = t.lists();
                        put_keys(&mut out, &lists.rm_list);
                        put_keys(&mut out, &lists.add_list);
                        put_keys(&mut out, &lists.chg_list);
                        out.push(lists.notify_events.len() as i64);
                        for ev in &lists.notify_events {
                            out.push(i64::from(ev.operation.code()));
                            put_event_ref(&mut out, ev.caller);
                            put_event_ref(&mut out, ev.data1);
                            put_event_ref(&mut out, ev.data2);
                            put_event_ref(&mut out, ev.data3);
                        }
                    }
                    op::TRACK_RESET => t.reset_arrays(),
                    _ => keep = false,
                }
                if keep {
                    if let Some(slot) = self.trackers.get_mut(ti) {
                        *slot = Some(t);
                    }
                } else {
                    t.finish(Some(self.ctx()?));
                }
            }
            op::SYS_FOREIGN => {
                let cb = (a[1] != 0).then(|| {
                    Box::new(Recorder {
                        tag: 2,
                        out: self.rec.clone(),
                    }) as Box<dyn NotifyCallback>
                });
                self.ctx()?.set_sys_notify_callback(cb);
            }
            op::DESTROY => {
                let o = need!(live!(a[1], false));
                solidcore::topology::proto_topology::p1::destroy_topology(
                    &mut self.recs,
                    ObjectKey(o as u64),
                )
                .map_err(harness)?;
                self.recs
                    .remove(&ObjectKey(o as u64))
                    .ok_or_else(|| harness("record"))?;
            }
            other => return Err(harness(format!("op {other}"))),
        }
        Ok(Step::Out(out))
    }

    fn dump(&self, kind_ts: &[i64], o: &mut Vec<i64>) -> Result<(), HarnessError> {
        let key_index = |k: Option<ObjectKey>| k.map_or(-1, |k| k.0 as i64);
        for k in 0..self.n {
            let Some(r) = self.recs.get(&ObjectKey(k as u64)) else {
                o.push(0);
                continue;
            };
            o.push(1);
            o.push(r.all_flags() as i64);
            for m in [1, 2, 4, 8, 16] {
                o.push(r.get_mark(mark_type(m)?) as i64);
            }
            o.push(r.user_index1() as i64);
            o.push(r.user_index2() as i64);
            o.push(Self::user_ptr_index(r.user_ptr1()));
            o.push(key_index(r.owner()));
            o.push(key_index(r.next()));
            o.push(key_index(r.last()));
        }
        let ctx = self.ctx_ref()?;
        for m in MARK_TYPES {
            o.push(ctx.current_mark(mark_type(m)?) as i64);
        }
        for m in [1, 2, 4, 8, 16] {
            o.push(i64::from(ctx.is_mark_locked(mark_type(m)?)));
        }
        let foreign = ctx
            .sys_notify_callback()
            .and_then(NotifyCallback::as_any)
            .and_then(|a| a.downcast_ref::<Recorder>())
            .is_some_and(|r| r.tag == 2);
        o.push(i64::from(foreign));
        if !self.recs.is_empty() {
            o.push(topo::TYPE);
            o.extend(kind_ts.iter().map(|&t| i64::from(topo::is_kind_of(t))));
            for s in [topo::TYPE_STRING, topo::CLASS_STRING] {
                o.push(s.len() as i64);
                o.extend(s.bytes().map(i64::from));
            }
        }
        Ok(())
    }
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    if !(1..=MAX_OBJECTS).contains(&i.n_objs) {
        return Err(harness("n_objs"));
    }
    let n = i.n_objs as usize;
    let rec = Arc::new(Mutex::new(Vec::new()));
    let mut ctx = Context::default();
    if i.user_cb {
        ctx.set_user_notify_callback(Some(Box::new(Recorder {
            tag: 1,
            out: rec.clone(),
        })));
    }
    let mut s = State {
        ctxs: vec![ctx],
        recs: (0..n)
            .map(|k| (ObjectKey(k as u64), Topology::new()))
            .collect(),
        n,
        kinds: vec![topo::TYPE; n],
        store: AttributeStore::new(),
        locks: Vec::new(),
        trackers: Vec::new(),
        rec,
        dbls: Vec::new(),
    };
    let mut ints = Vec::new();
    for a in &i.ops {
        match s.step(a)? {
            Step::Out(out) => {
                ints.push(0);
                ints.push(out.len() as i64);
                ints.extend(out);
            }
            Step::Skip => {
                ints.extend([ST_SKIP, 0]);
            }
        }
    }
    s.dump(&i.kind_ts, &mut ints)?;
    Ok(Output::new().ints("i", ints).values("d", s.dbls))
}

// ---- generated cases ---------------------------------------------------------------

fn gen_value(rng: &mut Rng) -> i64 {
    match rng.below(4) {
        0 => rng.range_i64(0, 15),
        1 => 1i64 << rng.range_i64(0, 62),
        2 => rng.next_u64() as i64,
        _ => -rng.range_i64(1, 1000),
    }
}

fn gen_inputs(rng: &mut Rng) -> Inputs {
    let n_objs = rng.range_i64(1, 6);
    let n_ops = rng.range_i64(1, 40);
    let mut ops = Vec::new();
    let mut n_trackers = 0i64;
    let obj = |rng: &mut Rng| rng.range_i64(0, n_objs - 1);
    let data = |rng: &mut Rng| {
        if rng.chance(0.25) {
            -1
        } else {
            rng.range_i64(0, n_objs - 1)
        }
    };
    for _ in 0..n_ops {
        let pick = rng.below(100);
        let rec: [i64; 8] = match pick {
            0..=7 => [op::SET_FLAGS, obj(rng), gen_value(rng), 0, 0, 0, 0, 0],
            8..=10 => [op::GET_FLAGS, obj(rng), 0, 0, 0, 0, 0, 0],
            11..=14 => {
                let o = if rng.chance(0.5) {
                    op::SET_UIDX1
                } else {
                    op::SET_UIDX2
                };
                [o, obj(rng), gen_value(rng), 0, 0, 0, 0, 0]
            }
            15..=17 => [op::SET_UPTR, obj(rng), data(rng), 0, 0, 0, 0, 0],
            18..=20 => [op::GET_USER, obj(rng), 0, 0, 0, 0, 0, 0],
            21..=30 => {
                let o = *rng
                    .choose(&[op::MARK, op::GET_MARK, op::IS_MARKED])
                    .unwrap_or(&op::MARK);
                let m = *rng.choose(&MARK_TYPES).unwrap_or(&1);
                [o, obj(rng), m, 0, 0, 0, 0, 0]
            }
            31..=36 => {
                let mask = if rng.chance(0.5) {
                    31
                } else {
                    rng.range_i64(0, 31)
                };
                [op::LOCK, mask, 0, 0, 0, 0, 0, 0]
            }
            37..=40 => [op::LOCK_NEW_MARK, 0, 0, 0, 0, 0, 0, 0],
            41..=44 => [op::UNLOCK, 0, 0, 0, 0, 0, 0, 0],
            45..=69 => [
                op::NOTIFY,
                rng.range_i64(0, 16),
                obj(rng),
                data(rng),
                data(rng),
                data(rng),
                0,
                0,
            ],
            70..=71 => [op::TOLERANCE, obj(rng), 0, 0, 0, 0, 0, 0],
            72..=73 => [op::CONNECTED, obj(rng), data(rng), 0, 0, 0, 0, 0],
            74..=78 if n_trackers < MAX_TRACKERS as i64 => {
                n_trackers += 1;
                [
                    op::TRACK_NEW,
                    i64::from(rng.chance(0.5)),
                    i64::from(rng.chance(0.85)),
                    0,
                    0,
                    0,
                    0,
                    0,
                ]
            }
            79..=92 if n_trackers > 0 => {
                let o = *rng
                    .choose(&[
                        op::TRACK_START,
                        op::TRACK_STOP,
                        op::TRACK_IS,
                        op::TRACK_IS,
                        op::TRACK_LISTS,
                        op::TRACK_RESET,
                        op::TRACK_DROP,
                    ])
                    .unwrap_or(&op::TRACK_IS);
                [o, rng.range_i64(0, n_trackers - 1), 0, 0, 0, 0, 0, 0]
            }
            93..=95 => [
                op::SYS_FOREIGN,
                i64::from(rng.chance(0.5)),
                0,
                0,
                0,
                0,
                0,
                0,
            ],
            96..=97 => [op::DESTROY, obj(rng), 0, 0, 0, 0, 0, 0],
            _ => [op::IS_MARKED, obj(rng), 1, 0, 0, 0, 0, 0],
        };
        ops.push(rec);
    }
    let kind_ts = (0..rng.range_i64(0, 4))
        .map(|_| {
            *rng.choose(&[16001, 12060, 11001, 16050, 16000, 16002, 0, -1])
                .unwrap_or(&0)
        })
        .collect();
    Inputs {
        n_objs,
        user_cb: rng.chance(0.7),
        kind_ts,
        ops,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.topology"
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_inputs(rng)).ok()
    }
}
