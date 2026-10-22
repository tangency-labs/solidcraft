//! Runner for unit `geom.object`: the `SmObject` / `SmShape` type system and
//! `AssertValid`, `SmObject::Notify` through the context callbacks,
//! `SmTemporaryChangeValue<TYPE>` scripts and `SmShape` construction.
//!
//! C++: `solidcore_ffi::units::geom_object` (upstream `SmObject.h`, `SmObject.cpp`,
//! `SmShape.h`, `SmShape.cpp`). Rust: `solidcore::geom::object`.
//!
//! Case kinds (`inputs.kind`):
//! - `types`: `IsKindOf` of an object and of an (empty) shape for each code in `ts`,
//!   then both `GetType`s, `AssertValid` and both type strings (as byte codes).
//! - `notify`: a script of `Notify` calls on a pool of objects whose context is the
//!   shared context (with the step's callbacks installed) or null; output is every
//!   `Execute` the callbacks received.
//! - `tcv`: a script of nested `SmTemporaryChangeValue` guards on one variable of
//!   type double (`ty` 0), `SmBoolean` (1) or `ULONG` (2); output is the variable
//!   after every step and after each remaining guard is destroyed.
//! - `shape`: `SmShape` from a B-rep (none, the `SmApiCreateBox` solid, or an ASCII
//!   `.smb` file, both built by the shared topology fixture; the Rust side imports the
//!   C++ export) and a selection of the whole B-rep / faces / edges / vertices, by index
//!   into the fixture walk's lists (the import's arena order).

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use solidcore::geom::attribute::ObjectKey;
use solidcore::geom::context::{Context, NotifyCallback};
use solidcore::geom::object::{
    self as obj, BrepStore, NoBreps, NotifyOperation, ObjectRef, Shape, ShapeMember, TemporaryChangeValue,
};
use solidcore::geom::tree::Tree;
use solidcore::kernel::Kernel;
use solidcore::topology::Brep;
use solidcore::{BrepId, EdgeId, FaceId, SolidError, VertexId};
use solidcore_ffi::units::geom_object::{
    self as ffi, tcv_op, NotifyStep, ShapeSelection, ShapeSource, MAX_NOTIFY_OP, NUM_OBJECTS,
};

use super::query_face_p1::{everything, inexact, model};
use super::topology_edgeuse_p1::fixture_input;
use crate::case::CaseContext;
use crate::json::num;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 600;

/// `geom.object` runner.
pub struct Runner;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct NotifyIn {
    #[serde(default)]
    has_context: bool,
    #[serde(default)]
    user: bool,
    #[serde(default)]
    sys: bool,
    op: i64,
    caller: i64,
    #[serde(default = "no_data")]
    data: [i64; 3],
}

const fn no_data() -> [i64; 3] {
    [-1, -1, -1]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SourceIn {
    None,
    Box([f64; 6]),
    /// Path and whether the file is ASCII.
    File(String, bool),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Inputs {
    Types {
        ts: Vec<i64>,
    },
    Notify {
        steps: Vec<NotifyIn>,
    },
    /// Floats are JSON numbers or `"NaN"` / `"inf"` / `"-inf"`.
    Tcv {
        ty: i32,
        init: Value,
        steps: Vec<(i64, Value)>,
    },
    Shape {
        source: SourceIn,
        #[serde(default)]
        whole_brep: bool,
        #[serde(default)]
        faces: Vec<i64>,
        #[serde(default)]
        edges: Vec<i64>,
        #[serde(default)]
        vertices: Vec<i64>,
    },
}

fn harness(e: impl std::fmt::Display) -> HarnessError {
    HarnessError::new(e.to_string())
}

/// A JSON number or `"NaN"` / `"inf"` / `"-inf"`.
fn float(v: &Value) -> Result<f64, HarnessError> {
    match v {
        Value::Number(n) => n.as_f64().ok_or_else(|| harness(format!("bad number {n}"))),
        Value::String(s) if s == "NaN" => Ok(f64::NAN),
        Value::String(s) if s == "inf" => Ok(f64::INFINITY),
        Value::String(s) if s == "-inf" => Ok(f64::NEG_INFINITY),
        other => Err(harness(format!("bad float {other}"))),
    }
}

fn tcv_inputs(
    init: &Value,
    steps: &[(i64, Value)],
) -> Result<(f64, Vec<(i64, f64)>), HarnessError> {
    let s = steps
        .iter()
        .map(|(op, x)| Ok((*op, float(x)?)))
        .collect::<Result<Vec<_>, HarnessError>>()?;
    Ok((float(init)?, s))
}

fn rust_err(e: &SolidError) -> Output {
    Output::unported(e).unwrap_or_else(|| Output::err(format!("rust_error: {e}")))
}

/// Gives the import the B-rep's face, edge and vertex list orders (`GetFaces` /
/// `GetEdges` / `GetVertices`, which order the members of a whole B-rep); the walk
/// numbers faces and vertices in another order.
fn set_list_orders(b: &mut Brep, path: &str, block: &[f64]) -> Result<(), HarnessError> {
    let orders = ffi::list_orders(path, block).map_err(|e| harness(format!("c++ list orders: {e}")))?;
    let mut at = 0;
    let mut next = || -> Result<Vec<u32>, HarnessError> {
        let n = *orders.get(at).ok_or_else(|| harness("short list orders"))? as usize;
        let list = orders
            .get(at + 1..at + 1 + n)
            .ok_or_else(|| harness("short list orders"))?
            .iter()
            .map(|&i| i as u32)
            .collect();
        at += 1 + n;
        Ok(list)
    };
    let (faces, edges, vertices) = (next()?, next()?, next()?);
    let bad = |e: SolidError| harness(format!("list order: {e}"));
    b.set_face_list(faces.into_iter().map(FaceId::from_index).collect()).map_err(bad)?;
    b.set_edge_list(edges.into_iter().map(EdgeId::from_index).collect()).map_err(bad)?;
    b.set_vertex_list(vertices.into_iter().map(VertexId::from_index).collect()).map_err(bad)?;
    Ok(())
}

/// The shared topology fixture of a shape's B-rep (`None` without one).
fn fixture_brep(source: &SourceIn) -> Result<Option<Value>, HarnessError> {
    Ok(match source {
        SourceIn::None => None,
        SourceIn::Box([x, y, z, l, w, h]) => Some(json!({
            "kind": "box",
            "origin": [x, y, z],
            "size": [l, w, h],
        })),
        SourceIn::File(path, true) => Some(json!({"kind": "file", "file": path, "rebuild": false})),
        SourceIn::File(_, false) => return Err(harness("the shared fixture reads ASCII files only")),
    })
}

// ---- C++ side ----------------------------------------------------------------------

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    match i {
        Inputs::Types { ts } => Ok(Output::new().ints("i", ffi::types(ts).map_err(harness)?)),
        Inputs::Notify { steps } => {
            let s: Vec<NotifyStep> = steps
                .iter()
                .map(|n| NotifyStep {
                    has_context: n.has_context,
                    user_callback: n.user,
                    sys_callback: n.sys,
                    op: n.op,
                    caller: n.caller,
                    data: n.data,
                })
                .collect();
            Ok(Output::new().ints("i", ffi::notify(&s).map_err(harness)?))
        }
        Inputs::Tcv { ty, init, steps } => {
            let (init, steps) = tcv_inputs(init, steps)?;
            Ok(Output::new().values("v", ffi::tcv(*ty, init, &steps).map_err(harness)?))
        }
        Inputs::Shape {
            source,
            whole_brep,
            faces,
            edges,
            vertices,
        } => {
            let src = match fixture_brep(source)? {
                None => ShapeSource::None,
                Some(brep) => {
                    let (path, block) = fixture_input(brep)?;
                    ShapeSource::Fixture { path, block }
                }
            };
            let sel = ShapeSelection {
                whole_brep: *whole_brep,
                faces: faces.clone(),
                edges: edges.clone(),
                vertices: vertices.clone(),
            };
            Ok(Output::new().values("v", ffi::shape(&src, &sel).map_err(harness)?))
        }
    }
}

// ---- Rust side ---------------------------------------------------------------------

fn type_string_codes(out: &mut Vec<i64>, s: &str) {
    out.push(s.len() as i64);
    out.extend(s.bytes().map(i64::from));
}

fn rust_types(ts: &[i64]) -> Result<Vec<i64>, SolidError> {
    let shape = Shape::new(&mut Kernel::default(), &NoBreps, &[], &[], &[], &[])?;
    let mut o = Vec::new();
    for &t in ts {
        o.push(i64::from(obj::is_kind_of(t)));
        o.push(i64::from(shape.is_kind_of(t)));
    }
    o.push(obj::OBJECT_TYPE);
    o.push(shape.type_code());
    o.push(i64::from(obj::assert_valid()));
    type_string_codes(&mut o, obj::OBJECT_TYPE_STRING);
    type_string_codes(&mut o, shape.type_string());
    Ok(o)
}

/// Records `Execute` calls like the shim's `ScRecorder`.
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
        Err(SolidError::InvalidInput {
            operation: "difftest::geom_object::Recorder",
            reason: "ignored status",
        })
    }
}

fn pool_object(i: i64) -> ObjectRef {
    ObjectRef {
        type_code: obj::OBJECT_TYPE,
        key: ObjectKey(i as u64),
    }
}

fn rust_notify(steps: &[NotifyIn]) -> Result<Vec<i64>, HarnessError> {
    let rec = Arc::new(Mutex::new(Vec::new()));
    let mut ctx = Context::default();
    let mut res = Vec::new();
    for s in steps {
        if !(0..=MAX_NOTIFY_OP).contains(&s.op)
            || !(0..NUM_OBJECTS).contains(&s.caller)
            || s.data.iter().any(|d| !(-1..NUM_OBJECTS).contains(d))
        {
            return Err(harness("notify step"));
        }
        let op = NotifyOperation::from_code(s.op as i32).ok_or_else(|| harness("op"))?;
        ctx.set_user_notify_callback(s.user.then(|| {
            Box::new(Recorder {
                tag: 1,
                out: rec.clone(),
            }) as Box<dyn NotifyCallback>
        }));
        ctx.set_sys_notify_callback(s.sys.then(|| {
            Box::new(Recorder {
                tag: 2,
                out: rec.clone(),
            }) as Box<dyn NotifyCallback>
        }));
        rec.lock().map_err(harness)?.clear();
        let data = |k: i64| (k >= 0).then(|| pool_object(k));
        obj::notify(
            s.has_context.then_some(&mut ctx),
            op,
            pool_object(s.caller),
            data(s.data[0]),
            data(s.data[1]),
            data(s.data[2]),
        );
        let r = rec.lock().map_err(harness)?;
        res.push((r.len() / 7) as i64);
        res.extend(r.iter().copied());
    }
    Ok(res)
}

/// Conversions between script operands / outputs and the guarded type.
trait TcvValue: Copy + PartialEq {
    fn from_f64(x: f64) -> Self;
    fn to_f64(self) -> f64;
}

impl TcvValue for f64 {
    fn from_f64(x: f64) -> Self {
        x
    }
    fn to_f64(self) -> f64 {
        self
    }
}

impl TcvValue for i32 {
    fn from_f64(x: f64) -> Self {
        x as Self
    }
    fn to_f64(self) -> f64 {
        f64::from(self)
    }
}

impl TcvValue for u64 {
    fn from_f64(x: f64) -> Self {
        x as Self
    }
    fn to_f64(self) -> f64 {
        self as f64
    }
}

/// Runs steps at one nesting level: `guard` is this level's guard. Returns at the
/// level's `CLOSE` (true) or the end of the script (false). Nested guards borrow
/// the value from this guard.
fn tcv_level<T: TcvValue>(
    guard: &mut TemporaryChangeValue<'_, T>,
    steps: &[(i64, f64)],
    pos: &mut usize,
    out: &mut Vec<f64>,
) -> Result<bool, HarnessError> {
    let read = |g: &TemporaryChangeValue<'_, T>| g.get().map(|v| v.to_f64());
    while let Some(&(op, x)) = steps.get(*pos) {
        *pos += 1;
        let x = T::from_f64(x);
        match op {
            tcv_op::OPEN | tcv_op::OPEN_SET => {
                let target = guard.get_mut().ok_or_else(|| harness("no value"))?;
                let mut inner = if op == tcv_op::OPEN {
                    TemporaryChangeValue::new(target, x)
                } else {
                    let mut g = TemporaryChangeValue::empty();
                    g.set(target, x);
                    g
                };
                out.push(read(&inner).ok_or_else(|| harness("no value"))?);
                let closed = tcv_level(&mut inner, steps, pos, out)?;
                drop(inner);
                out.push(read(guard).ok_or_else(|| harness("no value"))?);
                if !closed {
                    return Ok(false);
                }
                continue;
            }
            tcv_op::CLOSE => return Ok(true),
            tcv_op::CLEAR => guard.clear(),
            tcv_op::USE_ORIG => guard.use_orig_value(),
            tcv_op::USE_TEMP => guard.use_temp_value(),
            tcv_op::SET_NEW => guard.set_new_value(x),
            tcv_op::WRITE => *guard.get_mut().ok_or_else(|| harness("no value"))? = x,
            _ => return Err(harness("op")),
        }
        out.push(read(guard).ok_or_else(|| harness("no value"))?);
    }
    Ok(false)
}

/// The top level of a script: the variable itself, no guard.
fn tcv_run<T: TcvValue>(init: T, steps: &[(i64, f64)]) -> Result<Vec<f64>, HarnessError> {
    let mut var = init;
    let mut out = vec![var.to_f64()];
    let mut pos = 0usize;
    while let Some(&(op, x)) = steps.get(pos) {
        pos += 1;
        let x = T::from_f64(x);
        match op {
            tcv_op::OPEN | tcv_op::OPEN_SET => {
                let mut g = if op == tcv_op::OPEN {
                    TemporaryChangeValue::new(&mut var, x)
                } else {
                    let mut g = TemporaryChangeValue::empty();
                    g.set(&mut var, x);
                    g
                };
                out.push(
                    g.get()
                        .map(|v| v.to_f64())
                        .ok_or_else(|| harness("no value"))?,
                );
                let closed = tcv_level(&mut g, steps, &mut pos, &mut out)?;
                drop(g);
                out.push(var.to_f64());
                if !closed {
                    break;
                }
            }
            tcv_op::WRITE => {
                var = x;
                out.push(var.to_f64());
            }
            _ => return Err(harness("op without guard")),
        }
    }
    Ok(out)
}

fn rust_tcv(ty: i32, init: f64, steps: &[(i64, f64)]) -> Result<Vec<f64>, HarnessError> {
    match ty {
        0 => tcv_run(init, steps),
        1 => tcv_run(i32::from_f64(init), steps),
        2 => tcv_run(u64::from_f64(init), steps),
        _ => Err(harness("type")),
    }
}

fn put_tree<O: Copy + PartialEq>(o: &mut Vec<f64>, t: &Tree<O>) -> Result<(), SolidError> {
    o.push(t.nodes().len() as f64);
    o.push(t.object_list()?.len() as f64);
    match t.top_node() {
        Some(top) => {
            o.push(1.0);
            let b = t.node(top)?.bbox;
            o.extend([
                b.min().x,
                b.min().y,
                b.min().z,
                b.max().x,
                b.max().y,
                b.max().z,
            ]);
        }
        None => o.push(0.0),
    }
    Ok(())
}

fn rust_shape(
    brep: Option<&Brep>,
    whole_brep: bool,
    faces: &[i64],
    edges: &[i64],
    vertices: &[i64],
) -> Result<Vec<f64>, SolidError> {
    let b = BrepId::from_index(0);
    let pair;
    let store: &dyn BrepStore = match brep {
        Some(brep) => {
            pair = (b, brep);
            &pair
        }
        None => &NoBreps,
    };
    let breps: Vec<BrepId> = if whole_brep { vec![b] } else { Vec::new() };
    let f: Vec<_> = faces
        .iter()
        .map(|&i| (b, FaceId::from_index(i as u32)))
        .collect();
    let e: Vec<_> = edges
        .iter()
        .map(|&i| (b, EdgeId::from_index(i as u32)))
        .collect();
    let v: Vec<_> = vertices
        .iter()
        .map(|&i| (b, VertexId::from_index(i as u32)))
        .collect();
    let shape = Shape::new(&mut Kernel::default(), store, &breps, &f, &e, &v)?;
    let mut o = vec![
        shape.faces().len() as f64,
        shape.edges().len() as f64,
        shape.vertices().len() as f64,
    ];
    // walk indices of the members (the import's arena ids)
    o.extend(shape.faces().iter().map(|k| f64::from(k.1.index())));
    o.extend(shape.edges().iter().map(|k| f64::from(k.1.index())));
    o.extend(shape.vertices().iter().map(|k| f64::from(k.1.index())));
    put_tree(&mut o, shape.vertex_tree())?;
    put_tree(&mut o, shape.curve_tree())?;
    put_tree(&mut o, shape.surface_tree())?;
    // IsInShape of every face, edge and vertex of the B-rep
    let flag = |member| if shape.is_in_shape(member) { 1.0 } else { 0.0 };
    if let Some(brep) = brep {
        o.extend(brep.faces().ids().map(|f| flag(ShapeMember::Face((b, f)))));
        o.extend(brep.edges().ids().map(|e| flag(ShapeMember::Edge((b, e)))));
        o.extend(brep.vertices().ids().map(|v| flag(ShapeMember::Vertex((b, v)))));
    }
    Ok(o)
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    match i {
        Inputs::Types { ts } => Ok(match rust_types(ts) {
            Ok(v) => Output::new().ints("i", v),
            Err(e) => rust_err(&e),
        }),
        Inputs::Notify { steps } => Ok(Output::new().ints("i", rust_notify(steps)?)),
        Inputs::Tcv { ty, init, steps } => {
            let (init, steps) = tcv_inputs(init, steps)?;
            Ok(Output::new().values("v", rust_tcv(*ty, init, &steps)?))
        }
        Inputs::Shape {
            source,
            whole_brep,
            faces,
            edges,
            vertices,
        } => {
            let empty = !*whole_brep && faces.is_empty() && edges.is_empty() && vertices.is_empty();
            let model = match fixture_brep(source)? {
                None if !empty => return Err(harness("selection without B-rep")),
                None => None,
                Some(brep) => {
                    let mut m = model(&brep)?;
                    set_list_orders(&mut m.brep, &m.path, &m.block)?;
                    let (all_faces, all_edges) = everything(&m.brep);
                    if let Some(o) = inexact(&m, &all_faces, &all_edges) {
                        return Ok(o);
                    }
                    Some(m)
                }
            };
            let brep = model.as_ref().map(|m| &m.brep);
            Ok(match rust_shape(brep, *whole_brep, faces, edges, vertices) {
                Ok(v) => Output::new().values("v", v),
                Err(e) => rust_err(&e),
            })
        }
    }
}

// ---- generation --------------------------------------------------------------------

fn gen_types(rng: &mut Rng) -> Inputs {
    const CODES: [i64; 10] = [
        11001, 16500, 11000, 11002, 16000, 12150, 0, -1, 16501, 22001,
    ];
    let n = rng.range_i64(1, 12);
    let ts = (0..n)
        .map(|_| {
            if rng.chance(0.7) {
                CODES
                    .get(rng.below(CODES.len() as u64) as usize)
                    .copied()
                    .unwrap_or(0)
            } else {
                rng.range_i64(-100_000, 100_000)
            }
        })
        .collect();
    Inputs::Types { ts }
}

fn gen_notify(rng: &mut Rng) -> Inputs {
    let n = rng.range_i64(1, 20);
    let steps = (0..n)
        .map(|_| NotifyIn {
            has_context: rng.chance(0.75),
            user: rng.chance(0.6),
            sys: rng.chance(0.6),
            op: rng.range_i64(0, MAX_NOTIFY_OP),
            caller: rng.range_i64(0, NUM_OBJECTS - 1),
            data: [
                rng.range_i64(-1, NUM_OBJECTS - 1),
                rng.range_i64(-1, NUM_OBJECTS - 1),
                rng.range_i64(-1, NUM_OBJECTS - 1),
            ],
        })
        .collect();
    Inputs::Notify { steps }
}

fn gen_operand(rng: &mut Rng, ty: i32) -> f64 {
    if ty == 0 {
        match rng.below(10) {
            0 => f64::NAN,
            1 => 0.0,
            2 => -0.0,
            3 => 5.0,
            4 => f64::INFINITY,
            _ => rng.uniform(-100.0, 100.0),
        }
    } else if ty == 1 {
        rng.range_i64(0, 2) as f64
    } else {
        rng.range_i64(0, 6) as f64
    }
}

fn gen_tcv(rng: &mut Rng) -> Inputs {
    let ty = rng.range_i64(0, 2) as i32;
    let init = gen_operand(rng, ty);
    let n = rng.range_i64(1, 30);
    let mut depth = 0;
    let mut steps = Vec::new();
    for _ in 0..n {
        let x = gen_operand(rng, ty);
        let op = if depth == 0 {
            if rng.chance(0.8) {
                if rng.chance(0.7) {
                    tcv_op::OPEN
                } else {
                    tcv_op::OPEN_SET
                }
            } else {
                tcv_op::WRITE
            }
        } else {
            rng.range_i64(tcv_op::OPEN, tcv_op::WRITE)
        };
        match op {
            tcv_op::OPEN | tcv_op::OPEN_SET => depth += 1,
            tcv_op::CLOSE => depth -= 1,
            _ => {}
        }
        steps.push((op, num(x)));
    }
    Inputs::Tcv {
        ty,
        init: num(init),
        steps,
    }
}

fn gen_shape(rng: &mut Rng) -> Inputs {
    if rng.chance(0.3) {
        return Inputs::Shape {
            source: SourceIn::None,
            whole_brep: false,
            faces: Vec::new(),
            edges: Vec::new(),
            vertices: Vec::new(),
        };
    }
    let o = rng.point3(-10.0, 10.0);
    let bx = [
        o[0],
        o[1],
        o[2],
        rng.uniform(0.1, 5.0),
        rng.uniform(0.1, 5.0),
        rng.uniform(0.1, 5.0),
    ];
    let pick = |rng: &mut Rng, n: i64, p: f64| -> Vec<i64> {
        if rng.chance(p) {
            (0..rng.range_i64(1, 4))
                .map(|_| rng.range_i64(0, n - 1))
                .collect()
        } else {
            Vec::new()
        }
    };
    let whole_brep = rng.chance(0.3);
    let mut faces = pick(rng, 6, 0.4);
    let edges = pick(rng, 12, 0.4);
    let vertices = pick(rng, 8, 0.4);
    if !whole_brep && faces.is_empty() && edges.is_empty() && vertices.is_empty() {
        faces.push(rng.range_i64(0, 5));
    }
    Inputs::Shape {
        source: SourceIn::Box(bx),
        whole_brep,
        faces,
        edges,
        vertices,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "geom.object"
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
        let i = match rng.below(20) {
            0..=2 => gen_types(rng),
            3..=8 => gen_notify(rng),
            9..=18 => gen_tcv(rng),
            _ => gen_shape(rng),
        };
        serde_json::to_value(i).ok()
    }
}
