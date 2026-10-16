//! Runner for unit `geom.context`: context marks and mark locks, the Boolean flag,
//! notify callbacks, the context model size estimate / zone tolerance and the
//! `SmTol` context overloads, `SmMassPropertyBehavior`, the `SM_API` context.
//!
//! C++: `solidcore_ffi::units::geom_context` (upstream `SmContext`,
//! `SmNewMarkAndLock`, `source/SMLib/{inc,src}/SmContext.*`).
//! Rust: `solidcore::geom::context`, `solidcore::geom::tol`.
//!
//! Every case is a script of [`Step`]s run on `contexts` (model size, ratio) with the
//! system tolerances set from `sys_model` / `sys_ratio` and `slots` mark-lock slots.
//! Output: `i` (integers: marks, locks, flags, mark types, context indices) and `f`
//! (floats: model sizes, ratios, tolerances), each step's results followed by a
//! snapshot of every context, and a final snapshot after the live locks are dropped
//! in slot order. Every run uses at least [`MIN_GENERATED`] generated scripts.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::context::{
    self as ctxmod, Context, MarkLock, MarkType, MassPropertyBehavior, NotifyCallback,
};
use solidcore::geom::tol::{self, SmallTopoType};
use solidcore::{Kernel, KernelOptions};
use solidcore_ffi::units::geom_context::{self as ffi, op, Item, Step};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated scripts per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `geom.context` runner.
pub struct Runner;

/// One script step as written in fixtures (absent fields are 0).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(default)]
struct StepIn {
    op: i64,
    slot: i64,
    ctx: i64,
    ctx2: i64,
    m: i64,
    flag: i64,
    x: [f64; 4],
}

impl From<StepIn> for Step {
    fn from(s: StepIn) -> Self {
        Self {
            op: s.op,
            slot: s.slot,
            ctx: s.ctx,
            ctx2: s.ctx2,
            m: s.m,
            flag: s.flag,
            x: s.x,
        }
    }
}

fn default_size() -> f64 {
    tol::USE_DEFAULT
}

fn default_contexts() -> Vec<[f64; 2]> {
    vec![[tol::USE_DEFAULT, tol::USE_DEFAULT]]
}

fn default_slots() -> usize {
    4
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default = "default_size")]
    sys_model: f64,
    #[serde(default = "default_size")]
    sys_ratio: f64,
    #[serde(default = "default_contexts")]
    contexts: Vec<[f64; 2]>,
    #[serde(default = "default_slots")]
    slots: usize,
    #[serde(default)]
    steps: Vec<StepIn>,
}

impl Inputs {
    fn steps(&self) -> Vec<Step> {
        self.steps.iter().map(|&s| Step::from(s)).collect()
    }
}

fn output(items: &[Item]) -> Output {
    let mut ints = Vec::new();
    let mut floats = Vec::new();
    for it in items {
        match *it {
            Item::Int(v) => ints.push(v),
            Item::Float(v) => floats.push(v),
        }
    }
    Output::new().ints("i", ints).values("f", floats)
}

fn harness(e: impl std::fmt::Display) -> HarnessError {
    HarnessError::new(e.to_string())
}

// ---- C++ side ----------------------------------------------------------------------

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let contexts: Vec<(f64, f64)> = i.contexts.iter().map(|c| (c[0], c[1])).collect();
    let items =
        ffi::script(i.sys_model, i.sys_ratio, &contexts, i.slots, &i.steps()).map_err(harness)?;
    Ok(output(&items))
}

// ---- Rust side ---------------------------------------------------------------------

/// The kernel `SmTol::SetModelSizeEstimate(model)` +
/// `SmTol::SetLargeSmallSizeRatio(ratio)` would leave behind.
fn kernel(model: f64, ratio: f64) -> Kernel {
    let model = if model == tol::USE_DEFAULT {
        tol::MODEL_SIZE_ESTIMATE
    } else {
        model
    };
    let ratio = if ratio == tol::USE_DEFAULT {
        tol::LARGE_SMALL_SIZE_RATIO
    } else {
        ratio
    };
    let zone = tol::size_estimate_to_zone_tol_3d(model);
    let mut o = KernelOptions::default();
    o.tolerances.model_size_estimate = model;
    o.tolerances.zone_tol_3d = zone;
    o.tolerances.xsect_tol_3d = zone * 2.0;
    o.tolerances.approx_tol_3d = zone / 2.0;
    o.tolerances.large_small_size_ratio = ratio;
    Kernel::new(&o)
}

/// The callback object installed by `CALLBACK` steps.
#[derive(Debug)]
struct Notify;

impl NotifyCallback for Notify {
    fn execute(
        &mut self,
        _operation: solidcore::geom::object::NotifyOperation,
        _caller: solidcore::geom::object::ObjectRef,
        _data1: Option<solidcore::geom::object::ObjectRef>,
        _data2: Option<solidcore::geom::object::ObjectRef>,
        _data3: Option<solidcore::geom::object::ObjectRef>,
    ) -> solidcore::Result<()> {
        Ok(())
    }
}

const MARKS: [MarkType; 7] = [
    MarkType::NoMark,
    MarkType::Mark,
    MarkType::Mark2,
    MarkType::Mark3,
    MarkType::MarkIo,
    MarkType::MarkAssert,
    MarkType::AllMarks,
];

struct Sim {
    k: Kernel,
    ctxs: Vec<Context>,
    slots: Vec<Option<MarkLock>>,
    out: Vec<Item>,
    api: Option<Context>,
}

fn idx(v: i64) -> Result<usize, HarnessError> {
    usize::try_from(v).map_err(|_| harness(format!("index {v}")))
}

fn mark(v: i64) -> Result<MarkType, HarnessError> {
    i32::try_from(v)
        .ok()
        .and_then(MarkType::from_code)
        .ok_or_else(|| harness(format!("mark type {v}")))
}

impl Sim {
    fn int(&mut self, v: i64) {
        self.out.push(Item::Int(v));
    }

    fn float(&mut self, v: f64) {
        self.out.push(Item::Float(v));
    }

    fn ctx(&mut self, i: i64) -> Result<&mut Context, HarnessError> {
        let i = idx(i)?;
        self.ctxs.get_mut(i).ok_or_else(|| harness("context index"))
    }

    fn opt_ctx(&self, i: i64) -> Option<&Context> {
        usize::try_from(i).ok().and_then(|i| self.ctxs.get(i))
    }

    fn slot(&mut self, s: i64) -> Result<&mut Option<MarkLock>, HarnessError> {
        let s = idx(s)?;
        self.slots.get_mut(s).ok_or_else(|| harness("slot index"))
    }

    /// `delete slot`: runs the lock's destructor.
    fn drop_slot(&mut self, s: i64) -> Result<(), HarnessError> {
        if let Some(mut l) = self.slot(s)?.take() {
            l.release(&mut self.ctxs).map_err(harness)?;
        }
        Ok(())
    }

    fn snapshot(&mut self) {
        let mut v = Vec::new();
        for c in &self.ctxs {
            for m in MARKS {
                v.push(Item::Int(c.current_mark(m) as i64));
                v.push(Item::Int(i64::from(c.is_mark_locked(m))));
            }
            v.push(Item::Int(i64::from(c.doing_boolean())));
            v.push(Item::Int(i64::from(c.sys_notify_callback().is_some())));
            v.push(Item::Int(i64::from(c.user_notify_callback().is_some())));
            v.push(Item::Float(c.stored_model_size_estimate()));
            v.push(Item::Float(c.stored_large_small_size_ratio()));
            v.push(Item::Float(c.this_zone_tol_3d()));
        }
        self.out.extend(v);
    }

    fn step(&mut self, s: &Step) -> Result<(), HarnessError> {
        let x = s.x;
        match s.op {
            op::CUR_MARK => {
                let m = mark(s.m)?;
                let v = self.ctx(s.ctx)?.current_mark(m);
                self.int(v as i64);
            }
            op::IS_LOCKED => {
                let m = mark(s.m)?;
                let v = self.ctx(s.ctx)?.is_mark_locked(m);
                self.int(i64::from(v));
            }
            op::LOCK_NEW => {
                self.drop_slot(s.slot)?;
                let l = MarkLock::new(&mut self.ctxs, idx(s.ctx)?, s.m as i32).map_err(harness)?;
                *self.slot(s.slot)? = Some(l);
                self.int(i64::from(l.mark_type().code()));
            }
            op::LOCK_PAIR => {
                self.drop_slot(s.slot)?;
                let l = MarkLock::new_pair(&mut self.ctxs, idx(s.ctx)?, idx(s.ctx2)?)
                    .map_err(harness)?;
                *self.slot(s.slot)? = Some(l);
                self.int(i64::from(l.mark_type1().code()));
                self.int(i64::from(l.mark_type2().code()));
            }
            op::LOCK_DEFAULT => {
                self.drop_slot(s.slot)?;
                *self.slot(s.slot)? = Some(MarkLock::default());
            }
            op::LOCK_NEWMARK => match *self.slot(s.slot)? {
                Some(l) if l.context().is_some() => {
                    l.new_mark(&mut self.ctxs).map_err(harness)?;
                    self.int(1);
                }
                _ => self.int(-1),
            },
            op::LOCK_SETCTX | op::LOCK_SETCTX_PAIR => {
                let mut l = self.slot(s.slot)?.unwrap_or_else(MarkLock::unbound);
                let m = if s.op == op::LOCK_SETCTX {
                    l.set_context(&mut self.ctxs, idx(s.ctx)?, s.m as i32)
                } else {
                    l.set_context_pair(&mut self.ctxs, idx(s.ctx)?, idx(s.ctx2)?)
                }
                .map_err(harness)?;
                *self.slot(s.slot)? = Some(l);
                self.int(i64::from(m.code()));
            }
            op::LOCK_DROP => self.drop_slot(s.slot)?,
            op::LOCK_INFO => match *self.slot(s.slot)? {
                None => self.int(-2),
                Some(l) => {
                    let ix = |c: Option<usize>| c.map_or(-1, |c| c as i64);
                    self.int(i64::from(l.mark_type().code()));
                    self.int(i64::from(l.mark_type1().code()));
                    self.int(i64::from(l.mark_type2().code()));
                    self.int(ix(l.context()));
                    self.int(ix(l.context2()));
                }
            },
            op::DOING_BOOL => {
                let c = self.ctx(s.ctx)?;
                if s.flag != 0 {
                    *c.doing_boolean_mut() = s.m != 0;
                } else {
                    c.set_doing_boolean(s.m != 0);
                }
                let v = c.doing_boolean();
                self.int(i64::from(v));
            }
            op::GET_MODEL => {
                let c = self
                    .opt_ctx(s.ctx)
                    .ok_or_else(|| harness("context index"))?;
                let a = c.this_model_size_estimate(&self.k);
                let b = tol::context_model_size_estimate(&self.k, c);
                self.float(a);
                self.float(b);
            }
            op::SET_MODEL => {
                let c = self.ctx(s.ctx)?;
                let z = if s.flag != 0 {
                    tol::set_context_model_size_estimate(c, x[0])
                } else {
                    c.set_this_model_size_estimate(x[0])
                };
                self.float(z);
            }
            op::SET_RATIO => self.ctx(s.ctx)?.set_this_large_small_size_ratio(x[0]),
            op::TOL_CTX => {
                let small = i32::try_from(s.m)
                    .ok()
                    .and_then(SmallTopoType::from_code)
                    .ok_or_else(|| harness("small topology type"))?;
                let c = self.opt_ctx(s.ctx);
                let k = &self.k;
                let v = [
                    tol::context_zone_tol_3d(k, c, small),
                    tol::context_approx_tol_3d(k, c, small),
                    tol::context_xsect_tol_3d(k, c, small),
                    tol::calc_face_zone_tol_3d(k, c),
                ];
                for f in v {
                    self.float(f);
                }
            }
            op::CALC_EDGE | op::CALC_VERTEX => {
                let c = self.opt_ctx(s.ctx);
                let k = &self.k;
                let max = (s.flag != 0).then(|| tol::zone_tol3d(k, x[2]));
                let v = if s.op == op::CALC_EDGE {
                    tol::calc_edge_zone_tol_3d(k, x[0], x[1], max, c)
                } else {
                    tol::calc_vertex_zone_tol_3d(k, x[0], x[1], max, c)
                };
                self.float(v);
            }
            op::CALLBACK => {
                let c = self.ctx(s.ctx)?;
                let cb: Option<Box<dyn NotifyCallback>> = if s.m != 0 {
                    Some(Box::new(Notify))
                } else {
                    None
                };
                if s.flag != 0 {
                    c.set_user_notify_callback(cb);
                } else {
                    c.set_sys_notify_callback(cb);
                }
                let a = c.sys_notify_callback().is_some();
                let b = c.user_notify_callback().is_some();
                self.int(i64::from(a));
                self.int(i64::from(b));
            }
            op::MASS => {
                let r = if s.slot != 0 {
                    MassPropertyBehavior::default()
                } else {
                    MassPropertyBehavior::new(x[0], x[1], x[2], x[3], s.m as u64, s.flag as u64)
                };
                self.float(r.density);
                self.float(r.face_thickness);
                self.float(r.edge_cross_section_radius);
                self.float(r.vertex_sphere_radius);
                self.int(r.face_area_behavior as i64);
                self.int(r.face_volume_behavior as i64);
            }
            op::API => {
                ctxmod::create_context(&mut self.api);
                let p: *const Context = ctxmod::get_or_create_context(&mut self.api);
                let q: *const Context = ctxmod::get_or_create_context(&mut self.api);
                self.int(1);
                self.int(i64::from(std::ptr::eq(p, q)));
            }
            other => return Err(harness(format!("op {other}"))),
        }
        Ok(())
    }
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let steps = i.steps();
    // reject what the C++ wrapper rejects, so both sides fail alike
    ffi::validate(i.contexts.len(), i.slots, &steps).map_err(harness)?;
    let mut sim = Sim {
        k: kernel(i.sys_model, i.sys_ratio),
        ctxs: i
            .contexts
            .iter()
            .map(|c| Context::new(c[0], c[1]))
            .collect(),
        slots: vec![None; i.slots],
        out: Vec::new(),
        api: None,
    };
    for s in &steps {
        sim.step(s)?;
        sim.snapshot();
    }
    for s in 0..i.slots {
        sim.drop_slot(s as i64)?;
    }
    sim.snapshot();
    Ok(output(&sim.out))
}

// ---- generation --------------------------------------------------------------------

/// A model size estimate or ratio: the default sentinel, table boundaries, or
/// log-uniform values over many decades (sometimes negative).
fn size(rng: &mut Rng) -> f64 {
    const EDGES: [f64; 9] = [0.00005, 0.005, 0.5, 500.0, 50000.0, 5e6, 50.0, 1000.0, 1.0];
    let r = rng.unit();
    if r < 0.35 {
        tol::USE_DEFAULT
    } else if r < 0.6 {
        rng.choose(&EDGES).copied().unwrap_or(50.0)
    } else {
        let v = 10f64.powf(rng.uniform(-6.0, 8.0));
        if rng.chance(0.1) {
            -v
        } else {
            v
        }
    }
}

fn gen_step(rng: &mut Rng, n_ctx: i64, n_slots: i64) -> StepIn {
    const WEIGHTS: [(i64, u64); 20] = [
        (op::CUR_MARK, 3),
        (op::IS_LOCKED, 2),
        (op::LOCK_NEW, 8),
        (op::LOCK_PAIR, 4),
        (op::LOCK_DEFAULT, 1),
        (op::LOCK_NEWMARK, 4),
        (op::LOCK_SETCTX, 3),
        (op::LOCK_SETCTX_PAIR, 2),
        (op::LOCK_DROP, 6),
        (op::LOCK_INFO, 2),
        (op::DOING_BOOL, 2),
        (op::GET_MODEL, 2),
        (op::SET_MODEL, 2),
        (op::SET_RATIO, 1),
        (op::TOL_CTX, 2),
        (op::CALC_EDGE, 1),
        (op::CALC_VERTEX, 1),
        (op::CALLBACK, 1),
        (op::MASS, 1),
        (op::API, 1),
    ];
    let total: u64 = WEIGHTS.iter().map(|w| w.1).sum();
    let mut r = rng.below(total);
    let mut code = op::CUR_MARK;
    for (c, w) in WEIGHTS {
        if r < w {
            code = c;
            break;
        }
        r -= w;
    }
    let mut s = StepIn {
        op: code,
        slot: rng.range_i64(0, n_slots - 1),
        ctx: rng.range_i64(0, n_ctx - 1),
        ctx2: rng.range_i64(0, n_ctx - 1),
        ..StepIn::default()
    };
    let mark_type = |rng: &mut Rng| rng.choose(&ffi::MARK_TYPES).copied().unwrap_or(0);
    match code {
        op::CUR_MARK | op::IS_LOCKED | op::LOCK_SETCTX => s.m = mark_type(rng),
        op::LOCK_NEW => {
            s.m = if rng.chance(0.6) {
                ctxmod::ALL_MARKS.into()
            } else if rng.chance(0.1) {
                rng.range_i64(-40, 100)
            } else {
                rng.range_i64(0, 31)
            }
        }
        op::DOING_BOOL | op::CALLBACK => {
            s.m = rng.range_i64(0, 1);
            s.flag = rng.range_i64(0, 1);
        }
        op::SET_MODEL => {
            s.flag = rng.range_i64(0, 1);
            s.x[0] = size(rng);
        }
        op::SET_RATIO => s.x[0] = size(rng),
        op::TOL_CTX | op::CALC_EDGE | op::CALC_VERTEX => {
            if rng.chance(0.25) {
                s.ctx = -1;
            }
            s.m = rng.range_i64(0, 4);
            s.flag = rng.range_i64(0, 1);
            s.x = [
                rng.uniform(0.0, 1e-3),
                rng.uniform(0.0, 1e-3),
                rng.uniform(-1e-4, 1e-3),
                0.0,
            ];
        }
        op::MASS => {
            s.slot = i64::from(rng.chance(0.2));
            s.m = rng.range_i64(0, 2);
            s.flag = rng.range_i64(0, 1);
            s.x = [
                rng.uniform(0.0, 10.0),
                rng.uniform(0.0, 1.0),
                rng.uniform(0.0, 1.0),
                rng.uniform(0.0, 1.0),
            ];
        }
        _ => {}
    }
    s
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "geom.context"
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
        let n_ctx = rng.range_i64(1, 3);
        let n_slots = rng.range_i64(1, 6);
        let n_steps = rng.range_i64(1, 40);
        let contexts = (0..n_ctx).map(|_| [size(rng), size(rng)]).collect();
        let steps = (0..n_steps)
            .map(|_| gen_step(rng, n_ctx, n_slots))
            .collect();
        serde_json::to_value(Inputs {
            sys_model: size(rng),
            sys_ratio: size(rng),
            contexts,
            slots: n_slots as usize,
            steps,
        })
        .ok()
    }
}
