//! Runner for unit `geom.solution_array`: `SmSolutionEnd`, `SmSolution` queries and
//! `SmSolutionArray` editing and sorted insertion.
//!
//! C++: `solidcore_ffi::units::geom_solution_array` (upstream
//! `source/SMLib/src/SmSolutionArray.cpp`). Rust: `solidcore::geom::solution_array`.
//!
//! A case is a program of ops run against one solution array on both sides (the
//! op list and the solution encoding are documented in the shim). Objects are
//! codes `kind*100 + id` (`-1` = null): curve, surface, vertex, edge and face
//! objects are real upstream objects, the others stand-ins that only answer
//! `IsKindOf`. The Rust handles reach the same geometry through the
//! [`SolutionObject`] hooks: the shim's line and plane, and its vertex, edge and face
//! as records of one B-rep in the state the shim builds them ([`BareObjects`]). Paths
//! that need a poly vertex or volume's geometry or an object's zone tolerance are
//! deferred on the Rust side.
//!
//! Outputs: `ints` (indices, statuses, classifications, solution types, counts,
//! object codes, node ids) and `floats` (solution values and parameters, points,
//! intervals, tolerances). Every run has at least [`MIN_GENERATED`] generated
//! cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::object::OBJECT_TYPE;
use solidcore::geom::solution_array::{
    ObjectKind, Solution, SolutionArray, SolutionEnd, SolutionObject, SolutionType, SortKey,
    MAX_PARAMS, MAX_TREES,
};
use solidcore::curve::curve::p2::Curve;
use solidcore::curve::line::Line;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tree::NodeId;
use solidcore::geom::vector2d::Point2;
use solidcore::math::{Point3, Vec3};
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::edge as topo_edge;
use solidcore::topology::face::p2 as topo_face;
use solidcore::topology::vertex as topo_vertex;
use solidcore::topology::{Brep, Edge, EdgeId, Face, FaceId, Vertex, VertexId};
use solidcore::SolidError;
use solidcore_ffi::units::geom_solution_array as ffi;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 800;

/// `SM_BIG_ULONG`: `AddSortedSolution`'s "rejected" index.
const BIG_ULONG: i64 = 99_999_999;

/// `SM_ERR`.
const SM_ERR: i64 = 1001;

/// `SmCurve_TYPE`.
const CURVE_TYPE: i64 = 14001;

/// `geom.solution_array` runner.
pub struct Runner;

/// An object code (`kind*100 + id`, see the shim).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Obj(i64);

impl SolutionObject for Obj {
    fn kind(&self) -> ObjectKind {
        match self.0 / 100 {
            0 => ObjectKind::Curve,
            1 => ObjectKind::Surface,
            2 => ObjectKind::Volume,
            3 => ObjectKind::Vertex,
            4 => ObjectKind::Edge,
            5 => ObjectKind::Face,
            6 => ObjectKind::PolyVertex,
            7 => ObjectKind::PolyEdge,
            8 => ObjectKind::PolyLoop,
            9 => ObjectKind::PolyFace,
            10 => ObjectKind::CPolyFace,
            11 => ObjectKind::PolyShell,
            12 => ObjectKind::PolyRegion,
            13 => ObjectKind::PolyBrep,
            _ => ObjectKind::Other,
        }
    }

    fn natural_interval(&self) -> Option<Extent1d> {
        (self.kind() == ObjectKind::Curve).then(|| shim_line().natural_interval())
    }

    fn natural_uv_domain(&self) -> Option<Extent2d> {
        // (runner helper without a kernel in scope)
        (self.kind() == ObjectKind::Surface).then(|| shim_plane().natural_uv_domain_if_built())
    }

    fn vertex_point(&self) -> Option<Point3> {
        let b = BareObjects::new().ok()?;
        (self.kind() == ObjectKind::Vertex)
            .then(|| topo_vertex::point(&b.brep, b.vertex).ok())
            .flatten()
    }

    fn edge_curve(&self) -> Option<Self> {
        (self.kind() == ObjectKind::Edge).then_some(Obj(EDGE_CURVE))
    }

    fn edge_interval(&self) -> Option<Extent1d> {
        let b = BareObjects::new().ok()?;
        (self.kind() == ObjectKind::Edge)
            .then(|| topo_edge::interval(&b.brep, b.edge).ok())
            .flatten()
    }

    fn curve_point(&self, t: f64) -> Option<Point3> {
        (self.kind() == ObjectKind::Curve)
            .then(|| shim_line().evaluate_point(&solidcore::Kernel::default(), t).ok())
            .flatten()
    }

    fn face_surface(&self) -> Option<Self> {
        (self.kind() == ObjectKind::Face).then_some(Obj(FACE_SURFACE))
    }

    fn face_uv_domain(&self) -> Option<Extent2d> {
        let b = BareObjects::new().ok()?;
        (self.kind() == ObjectKind::Face)
            .then(|| topo_face::uv_domain(&b.brep, b.face).ok())
            .flatten()
    }

    fn surface_point(&self, u: f64, v: f64) -> Option<Point3> {
        (self.kind() == ObjectKind::Surface)
            .then(|| shim_plane()
                .evaluate_point(&solidcore::Kernel::default(), Point2::new(u, v))
                .ok())
            .flatten()
    }
}

/// The handle of the shim edges' curve (`the_line()`, a curve of its own that no object
/// code names; kind 0, an id the generator never draws).
const EDGE_CURVE: i64 = 99;
/// The handle of the shim faces' surface (`the_plane()`; kind 1, an id never drawn).
const FACE_SURFACE: i64 = 199;

/// The shim's line: `SmLine((0,0,0), (1,2,3), 3)` (every curve object and the edges'
/// curve).
fn shim_line() -> Curve {
    Curve::from(Line::from_points(
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 2.0, 3.0),
        3,
    ))
}

/// The shim's plane: `SmPlane((0,0,1), (0,0,1))` (every surface object and the faces'
/// surface).
fn shim_plane() -> Surface {
    Surface::from(Plane::from_normal(
        Point3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, 0.0, 1.0),
    ))
}

/// The shim's vertex, edge and face objects as records of one B-rep, in the state the
/// shim builds them (default-constructed `SmVertex` / `SmEdge` / `SmFace` with only
/// `m_pCurve` / `m_pSurface` set, never joined to a B-rep): a vertex at the origin with
/// tolerance `0` and no vertex uses, an edge on the line with the empty interval,
/// tolerance `0` and no edge uses, a face on the plane and nothing else (its shell is a
/// placeholder no list names). All objects of a kind are alike, so one record each.
struct BareObjects {
    brep: Brep,
    vertex: VertexId,
    edge: EdgeId,
    face: FaceId,
}

impl BareObjects {
    fn new() -> Result<Self, SolidError> {
        let mut brep = Brep::default();
        let vertex = brep.add_vertex_record(Vertex::default())?;
        let edge = brep.add_edge_record(Edge {
            // never read: an edge's vertices are those of its (absent) edge uses
            vertices: [VertexId::from_index(u32::MAX); 2],
            edgeuses: Vec::new(),
            interval: Extent1d::new(),
            tolerance: 0.0,
            curve: Some(shim_line()),
        })?;
        let shell = brep.add_shell()?;
        let mut f = Face::new(shell);
        f.surface = Some(shim_plane());
        let face = brep.add_face_record(f)?;
        Ok(Self {
            brep,
            vertex,
            edge,
            face,
        })
    }
}

const VOLUME: i64 = 2;
const POLY_VERTEX: i64 = 6;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Sol {
    t: i64,
    nv: i64,
    no: i64,
    sv: f64,
    sp: [f64; MAX_PARAMS],
    ev: f64,
    ep: [f64; MAX_PARAMS],
    objs: [i64; MAX_TREES],
    nodes: [i64; MAX_TREES],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Op {
    New {
        cap: i64,
    },
    Add {
        s: Sol,
    },
    AddSorted {
        key: i64,
        s: Sol,
    },
    InsertAt {
        idx: i64,
        count: i64,
        s: Sol,
    },
    RemoveAt {
        idx: i64,
        count: i64,
    },
    Swap {
        i: i64,
        j: i64,
    },
    SetSize {
        n: i64,
    },
    SetAt {
        idx: i64,
        s: Sol,
    },
    GetAt {
        idx: i64,
    },
    Reset,
    Append {
        sols: Vec<Sol>,
    },
    Dump,
    Info,
    Classify {
        start: bool,
        a: Sol,
        b: Sol,
    },
    IsEqual {
        a: Sol,
        b: Sol,
    },
    GetIndex {
        obj: i64,
        flag: bool,
        s: Sol,
    },
    PointAt {
        idx: i64,
        end: bool,
        s: Sol,
    },
    PointOf {
        obj: i64,
        flag: bool,
        end: bool,
        s: Sol,
    },
    ObjIndexForParam {
        pidx: i64,
        s: Sol,
    },
    ParamInterval {
        pidx: i64,
        s: Sol,
    },
    ObjTol {
        idx: i64,
        s: Sol,
    },
    Copy {
        s: Sol,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    ops: Vec<Op>,
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

fn b(v: bool) -> f64 {
    if v {
        1.0
    } else {
        0.0
    }
}

fn enc_sol(v: &mut Vec<f64>, s: &Sol) {
    v.extend([s.t as f64, s.nv as f64, s.no as f64, s.sv]);
    v.extend(s.sp);
    v.push(s.ev);
    v.extend(s.ep);
    v.extend(s.objs.iter().map(|&o| o as f64));
    v.extend(s.nodes.iter().map(|&n| n as f64));
}

fn prog(ops: &[Op]) -> Vec<f64> {
    let mut v = Vec::new();
    for op in ops {
        match op {
            Op::New { cap } => v.extend([1.0, *cap as f64]),
            Op::Add { s } => {
                v.push(2.0);
                enc_sol(&mut v, s);
            }
            Op::AddSorted { key, s } => {
                v.extend([3.0, *key as f64]);
                enc_sol(&mut v, s);
            }
            Op::InsertAt { idx, count, s } => {
                v.extend([4.0, *idx as f64, *count as f64]);
                enc_sol(&mut v, s);
            }
            Op::RemoveAt { idx, count } => v.extend([5.0, *idx as f64, *count as f64]),
            Op::Swap { i, j } => v.extend([6.0, *i as f64, *j as f64]),
            Op::SetSize { n } => v.extend([7.0, *n as f64]),
            Op::SetAt { idx, s } => {
                v.extend([8.0, *idx as f64]);
                enc_sol(&mut v, s);
            }
            Op::GetAt { idx } => v.extend([9.0, *idx as f64]),
            Op::Reset => v.push(10.0),
            Op::Append { sols } => {
                v.extend([11.0, sols.len() as f64]);
                for s in sols {
                    enc_sol(&mut v, s);
                }
            }
            Op::Dump => v.push(12.0),
            Op::Info => v.push(13.0),
            Op::Classify { start, a, b: o } => {
                v.extend([20.0, b(*start)]);
                enc_sol(&mut v, a);
                enc_sol(&mut v, o);
            }
            Op::IsEqual { a, b: o } => {
                v.push(21.0);
                enc_sol(&mut v, a);
                enc_sol(&mut v, o);
            }
            Op::GetIndex { obj, flag, s } => {
                v.extend([22.0, *obj as f64, b(*flag)]);
                enc_sol(&mut v, s);
            }
            Op::PointAt { idx, end, s } => {
                v.extend([23.0, *idx as f64, b(*end)]);
                enc_sol(&mut v, s);
            }
            Op::PointOf { obj, flag, end, s } => {
                v.extend([24.0, *obj as f64, b(*flag), b(*end)]);
                enc_sol(&mut v, s);
            }
            Op::ObjIndexForParam { pidx, s } => {
                v.extend([25.0, *pidx as f64]);
                enc_sol(&mut v, s);
            }
            Op::ParamInterval { pidx, s } => {
                v.extend([26.0, *pidx as f64]);
                enc_sol(&mut v, s);
            }
            Op::ObjTol { idx, s } => {
                v.extend([27.0, *idx as f64]);
                enc_sol(&mut v, s);
            }
            Op::Copy { s } => {
                v.push(28.0);
                enc_sol(&mut v, s);
            }
        }
    }
    v
}

// ---- Rust side ------------------------------------------------------------

fn obj(code: i64) -> Option<Obj> {
    (code >= 0).then_some(Obj(code))
}

fn obj_code(o: Option<Obj>) -> i64 {
    o.map_or(-1, |o| o.0)
}

fn usz(v: i64) -> Result<usize, HarnessError> {
    usize::try_from(v).map_err(|_| harness("negative index"))
}

fn to_sol(s: &Sol) -> Result<Solution<Obj>, HarnessError> {
    let t = SolutionType::from_code(s.t as i32).ok_or_else(|| harness("solution type"))?;
    let mut r = Solution::new();
    r.solution_type = t;
    r.num_variables = usz(s.nv)?;
    r.num_objects = usz(s.no)?;
    r.start = SolutionEnd {
        solution_value: s.sv,
        parameters: s.sp,
    };
    r.end = SolutionEnd {
        solution_value: s.ev,
        parameters: s.ep,
    };
    for (slot, &code) in r.objects.iter_mut().zip(&s.objs) {
        *slot = obj(code);
    }
    for (slot, &id) in r.nodes.iter_mut().zip(&s.nodes) {
        *slot = (id >= 0).then_some(NodeId(id as usize));
    }
    Ok(r)
}

fn put_end(of: &mut Vec<f64>, e: &SolutionEnd) {
    of.push(e.solution_value);
    of.extend(e.parameters);
}

fn put_sol(oi: &mut Vec<i64>, of: &mut Vec<f64>, s: &Solution<Obj>) {
    oi.extend([
        i64::from(s.solution_type.code()),
        s.num_variables as i64,
        s.num_objects as i64,
    ]);
    oi.extend(s.objects.iter().map(|&o| obj_code(o)));
    oi.extend(s.nodes.iter().map(|n| n.map_or(-1, |n| n.0 as i64)));
    put_end(of, &s.start);
    put_end(of, &s.end);
}

fn put_v(of: &mut Vec<f64>, v: Vec3) {
    of.extend([v.x, v.y, v.z]);
}

enum RunError {
    Rust(SolidError),
    Harness(HarnessError),
}

impl From<SolidError> for RunError {
    fn from(e: SolidError) -> Self {
        Self::Rust(e)
    }
}

impl From<HarnessError> for RunError {
    fn from(e: HarnessError) -> Self {
        Self::Harness(e)
    }
}

/// Applies one op to `arr`, appending its outputs.
fn apply(
    arr: &mut SolutionArray<Obj>,
    op: &Op,
    oi: &mut Vec<i64>,
    of: &mut Vec<f64>,
) -> Result<(), RunError> {
    match op {
        Op::New { cap } => *arr = SolutionArray::try_with_capacity(usz(*cap)?)?,
        Op::Add { s } => oi.push(arr.add(&to_sol(s)?) as i64),
        Op::AddSorted { key, s } => {
            let key = SortKey::from_code(*key as i32).ok_or_else(|| harness("sort key"))?;
            let r = arr.add_sorted_solution(&to_sol(s)?, key)?;
            oi.push(r.map_or(BIG_ULONG, |i| i as i64));
        }
        Op::InsertAt { idx, count, s } => {
            arr.try_insert_at(usz(*idx)?, &to_sol(s)?, usz(*count)?)?;
        }
        Op::RemoveAt { idx, count } => arr.remove_at(usz(*idx)?, usz(*count)?),
        Op::Swap { i, j } => arr.swap(usz(*i)?, usz(*j)?)?,
        Op::SetSize { n } => arr.try_set_size(usz(*n)?)?,
        Op::SetAt { idx, s } => arr.set_at(usz(*idx)?, &to_sol(s)?)?,
        Op::GetAt { idx } => put_sol(oi, of, &arr.get_at(usz(*idx)?)?),
        Op::Reset => arr.reset(),
        Op::Append { sols } => {
            let mut src = SolutionArray::new();
            for s in sols {
                src.add(&to_sol(s)?);
            }
            oi.push(arr.append(&src) as i64);
        }
        Op::Dump => {
            oi.push(arr.len() as i64);
            for k in 0..arr.len() {
                let s = *arr.get(k).ok_or_else(|| harness("dump index"))?;
                put_sol(oi, of, &s);
            }
        }
        Op::Info => {
            oi.push(arr.type_code());
            oi.push(i64::from(arr.is_kind_of(SolutionArray::<Obj>::TYPE)));
            oi.push(i64::from(arr.is_kind_of(OBJECT_TYPE)));
            oi.push(i64::from(arr.is_kind_of(CURVE_TYPE)));
        }
        Op::Classify { start, a, b } => oi.push(i64::from(
            to_sol(a)?
                .classify_solution_end(&to_sol(b)?, *start)?
                .code(),
        )),
        Op::IsEqual { a, b } => oi.push(i64::from(to_sol(a)?.is_equal(&to_sol(b)?)?)),
        Op::GetIndex { obj: o, flag, s } => match to_sol(s)?.index_of(obj(*o), *flag)? {
            Some((i, p)) => oi.extend([0, i as i64, p as i64]),
            None => oi.push(SM_ERR),
        },
        Op::PointAt { idx, end, s } => {
            let r = to_sol(s)?.point(usz(*idx)?, *end)?;
            put_point(oi, of, r);
        }
        Op::PointOf {
            obj: o,
            flag,
            end,
            s,
        } => {
            let r = to_sol(s)?.point_of(obj(*o), *flag, *end)?;
            put_point(oi, of, r);
        }
        Op::ObjIndexForParam { pidx, s } => {
            match to_sol(s)?.object_index_for_parameter_index(usz(*pidx)?)? {
                Some((i, d)) => oi.extend([0, i as i64, d as i64]),
                None => oi.push(SM_ERR),
            }
        }
        Op::ParamInterval { pidx, s } => match to_sol(s)?.parameter_interval(usz(*pidx)?)? {
            Some(ivl) => {
                oi.push(0);
                of.extend([ivl.min(), ivl.max()]);
            }
            None => oi.push(SM_ERR),
        },
        Op::ObjTol { idx, s } => of.push(to_sol(s)?.object_tolerance(usz(*idx)?)?),
        Op::Copy { s } => {
            let src = to_sol(s)?;
            let mut c = src;
            put_sol(oi, of, &c);
            c.reset();
            put_sol(oi, of, &c);
            let mut e = src.start;
            for k in 0..MAX_PARAMS {
                of.push(e.param(k)?);
            }
            e.reset();
            put_end(of, &e);
        }
    }
    Ok(())
}

fn put_point(oi: &mut Vec<i64>, of: &mut Vec<f64>, r: Option<(Vec3, Vec3)>) {
    match r {
        Some((p, q)) => {
            oi.push(0);
            put_v(of, p);
            put_v(of, q);
        }
        None => oi.push(SM_ERR),
    }
}

fn run_rust(ops: &[Op]) -> Result<(Vec<i64>, Vec<f64>), RunError> {
    let mut arr = SolutionArray::new();
    let (mut oi, mut of) = (Vec::new(), Vec::new());
    for op in ops {
        apply(&mut arr, op, &mut oi, &mut of)?;
    }
    Ok((oi, of))
}

// ---- generation -----------------------------------------------------------

/// Object codes a case draws from.
struct Pool {
    codes: Vec<i64>,
}

const SAFE_KINDS: [i64; 10] = [3, 6, 7, 8, 9, 10, 11, 12, 13, 14];
const GEOM_KINDS: [i64; 4] = [0, 1, 4, 5];

impl Pool {
    fn new(rng: &mut Rng) -> Self {
        let geom = rng.chance(0.25);
        let n = rng.range_i64(1, 4);
        let mut codes = Vec::new();
        for _ in 0..n {
            let kind = if geom && rng.chance(0.5) {
                *rng.choose(&GEOM_KINDS).unwrap_or(&0)
            } else if rng.chance(0.08) {
                VOLUME
            } else {
                *rng.choose(&SAFE_KINDS).unwrap_or(&14)
            };
            codes.push(kind * 100 + rng.range_i64(0, 2));
        }
        Self { codes }
    }

    fn pick(&self, rng: &mut Rng, null_rate: f64) -> i64 {
        if rng.chance(null_rate) {
            -1
        } else {
            *rng.choose(&self.codes).unwrap_or(&-1)
        }
    }
}

fn kind(code: i64) -> i64 {
    if code < 0 {
        -1
    } else {
        code / 100
    }
}

/// A value from a small palette, often nudged by a tolerance-scale amount, so that
/// equality and ordering ties are common.
fn value(rng: &mut Rng) -> f64 {
    let base = *rng
        .choose(&[0.0, 1.0, 2.0, -1.0, 0.5, 0.25, 1e-3, 3.0, 1e5, -2.5])
        .unwrap_or(&0.0);
    let base = if rng.chance(0.15) {
        rng.uniform(-10.0, 10.0)
    } else {
        base
    };
    let nudge = *rng
        .choose(&[
            0.0, 0.0, 0.0, 1e-14, 1e-13, 5e-13, 2e-12, 1e-9, 5e-7, 1e-6, 2e-6, 1e-4, 1e-2,
        ])
        .unwrap_or(&0.0);
    if rng.chance(0.5) {
        base + nudge
    } else {
        base - nudge
    }
}

fn gen_sol(rng: &mut Rng, pool: &Pool) -> Sol {
    let t = match rng.below(10) {
        0..=4 => 1,
        5..=7 => 2,
        8 => 3,
        _ => *rng.choose(&[0, 4]).unwrap_or(&4),
    };
    let nv = if rng.chance(0.1) {
        rng.range_i64(0, 10)
    } else {
        rng.range_i64(0, 4)
    };
    let no = if rng.chance(0.1) {
        rng.range_i64(0, 5)
    } else {
        rng.range_i64(0, 3)
    };
    let mut sp = [0.0; MAX_PARAMS];
    let mut ep = [0.0; MAX_PARAMS];
    let used = if rng.chance(0.2) {
        MAX_PARAMS
    } else {
        nv as usize
    };
    for (a, b) in sp.iter_mut().zip(ep.iter_mut()).take(used) {
        *a = value(rng);
        *b = if rng.chance(0.3) { *a } else { value(rng) };
    }
    let sv = if rng.chance(0.05) { 1e20 } else { value(rng) };
    let ev = if rng.chance(0.4) { sv } else { value(rng) };
    let mut objs = [-1; MAX_TREES];
    for (k, o) in objs.iter_mut().enumerate() {
        if (k as i64) < no {
            *o = pool.pick(rng, 0.1);
        } else if rng.chance(0.1) {
            *o = pool.pick(rng, 0.0);
        }
    }
    let mut nodes = [-1; MAX_TREES];
    for n in &mut nodes {
        if rng.chance(0.2) {
            *n = rng.range_i64(0, 4095);
        }
    }
    Sol {
        t,
        nv,
        no,
        sv,
        sp,
        ev,
        ep,
        objs,
        nodes,
    }
}

/// A solution close to `s`: same counts and objects, some values nudged.
fn near(rng: &mut Rng, s: &Sol) -> Sol {
    let mut o = s.clone();
    let nudge = |rng: &mut Rng, v: f64| -> f64 {
        if rng.chance(0.5) {
            v
        } else {
            v + *rng
                .choose(&[
                    1e-14, -1e-13, 1e-12, 1e-9, -1e-7, 1e-6, -3e-6, 1e-4, 0.5, -1.0,
                ])
                .unwrap_or(&0.0)
        }
    };
    o.sv = nudge(rng, o.sv);
    o.ev = nudge(rng, o.ev);
    for (a, b) in o.sp.iter_mut().zip(o.ep.iter_mut()) {
        *a = nudge(rng, *a);
        *b = nudge(rng, *b);
    }
    if rng.chance(0.15) {
        o.t = *rng.choose(&[1, 2, 3]).unwrap_or(&1);
    }
    if rng.chance(0.05) {
        o.nv = rng.range_i64(0, 10);
    }
    o
}

fn without(s: &mut Sol, rng: &mut Rng, bad: &[i64]) {
    for o in &mut s.objs {
        if bad.contains(&kind(*o)) {
            *o = rng.choose(&SAFE_KINDS).unwrap_or(&14) * 100;
            if bad.contains(&kind(*o)) {
                *o = 1400;
            }
        }
    }
}

fn non_null(s: &mut Sol) {
    for (k, o) in s.objs.iter_mut().enumerate().take(s.no as usize) {
        if *o < 0 {
            *o = 1400 + k as i64;
        }
    }
}

/// Makes `s` safe for `GetParameterInterval` paths (`IsEqual`, `AddSortedSolution`):
/// no volumes and, when the first object has parameters, no null objects (upstream
/// would dereference them).
fn interval_safe(s: &mut Sol, rng: &mut Rng) {
    without(s, rng, &[VOLUME]);
    if s.objs
        .first()
        .is_some_and(|&o| GEOM_KINDS.contains(&kind(o)))
    {
        non_null(s);
    }
}

fn gen_array(rng: &mut Rng) -> Inputs {
    let pool = Pool::new(rng);
    let mut ops = Vec::new();
    let mut model: SolutionArray<Obj> = SolutionArray::new();
    let mut prev: Vec<Sol> = Vec::new();
    if rng.chance(0.3) {
        ops.push(Op::New {
            cap: rng.range_i64(0, 12),
        });
    }
    let n = rng.range_i64(4, 40);
    let sorted = rng.chance(0.6);
    let key = rng.range_i64(0, 4);
    for _ in 0..n {
        let mut s = if !prev.is_empty() && rng.chance(0.35) {
            let base = rng
                .choose(&prev)
                .cloned()
                .unwrap_or_else(|| gen_sol(rng, &pool));
            near(rng, &base)
        } else {
            gen_sol(rng, &pool)
        };
        interval_safe(&mut s, rng);
        prev.push(s.clone());
        let size = model.len() as i64;
        let op = if sorted && rng.chance(0.75) {
            Op::AddSorted {
                key: if rng.chance(0.8) {
                    key
                } else {
                    rng.range_i64(0, 4)
                },
                s,
            }
        } else {
            match rng.below(12) {
                0..=2 => Op::Add { s },
                3 => Op::InsertAt {
                    idx: rng.range_i64(0, size + 2),
                    count: rng.range_i64(0, 3),
                    s,
                },
                4 if size > 0 => Op::RemoveAt {
                    idx: rng.range_i64(0, size + 1),
                    count: rng.range_i64(0, 3),
                },
                5 if size > 0 => Op::Swap {
                    i: rng.range_i64(0, size - 1),
                    j: rng.range_i64(0, size - 1),
                },
                6 => Op::SetSize {
                    n: rng.range_i64(0, size + 3),
                },
                7 if size > 0 => Op::SetAt {
                    idx: rng.range_i64(0, size - 1),
                    s,
                },
                8 if size > 0 => Op::GetAt {
                    idx: rng.range_i64(0, size - 1),
                },
                9 if rng.chance(0.2) => Op::Reset,
                10 => {
                    let m = rng.range_i64(0, 3);
                    let sols = (0..m)
                        .map(|_| {
                            let mut t = gen_sol(rng, &pool);
                            interval_safe(&mut t, rng);
                            t
                        })
                        .collect();
                    Op::Append { sols }
                }
                11 => Op::Dump,
                _ => Op::AddSorted {
                    key: rng.range_i64(0, 4),
                    s,
                },
            }
        };
        let (mut oi, mut of) = (Vec::new(), Vec::new());
        if apply(&mut model, &op, &mut oi, &mut of).is_err() {
            // unported on the Rust side: the case is deferred; stop growing it
            ops.push(op);
            break;
        }
        ops.push(op);
    }
    ops.push(Op::Dump);
    ops.push(Op::Info);
    Inputs { ops }
}

/// A solution `a` and one whose start (and end) parameters sit at, between,
/// below or above `a`'s ends, per variable mostly in one consistent way.
fn classify_pair(rng: &mut Rng, mut a: Sol) -> (Sol, Sol) {
    if rng.chance(0.15) {
        let o = near(rng, &a);
        return (a, o);
    }
    if a.nv == 0 && rng.chance(0.9) {
        a.nv = rng.range_i64(1, 3);
    }
    if a.t == 2 || rng.chance(0.5) {
        // a well-formed range: start below end in every parameter
        for (lo, hi) in a.sp.iter_mut().zip(a.ep.iter_mut()) {
            if *lo > *hi {
                std::mem::swap(lo, hi);
            } else if *lo == *hi && rng.chance(0.7) {
                *hi += 1.0;
            }
        }
    }
    let mut o = a.clone();
    o.t = *rng.choose(&[1, 1, 2, 3]).unwrap_or(&1);
    let mode = rng.below(6);
    let place = |rng: &mut Rng, lo: f64, hi: f64| -> f64 {
        let m = if rng.chance(0.85) { mode } else { rng.below(6) };
        let tiny = *rng.choose(&[0.0, 1e-14, -1e-14, 1e-11]).unwrap_or(&0.0);
        match m {
            0 => lo + tiny,
            1 => hi + tiny,
            2 => 0.5 * (lo + hi),
            3 => lo - 1.0,
            4 => hi + 1.0,
            _ => value(rng),
        }
    };
    for ((os, oe), (&lo, &hi)) in
        o.sp.iter_mut()
            .zip(o.ep.iter_mut())
            .zip(a.sp.iter().zip(&a.ep))
    {
        *os = place(rng, lo, hi);
        *oe = place(rng, lo, hi);
    }
    let vs = [a.sv, a.sv, a.sv, a.ev, a.sv + 1e-7, a.sv + 1e-3];
    o.sv = *rng.choose(&vs).unwrap_or(&a.sv);
    o.ev = *rng.choose(&vs).unwrap_or(&a.ev);
    (a, o)
}

/// One of `s`'s used objects (60%) or a pool object / null.
fn pick_used(rng: &mut Rng, s: &Sol, pool: &Pool) -> i64 {
    let used = s.objs.get(..s.no as usize).unwrap_or(&[]);
    if !used.is_empty() && rng.chance(0.6) {
        *rng.choose(used).unwrap_or(&-1)
    } else {
        pool.pick(rng, 0.2)
    }
}

fn gen_queries(rng: &mut Rng) -> Inputs {
    let pool = Pool::new(rng);
    let mut ops = Vec::new();
    for _ in 0..rng.range_i64(1, 6) {
        let mut s = gen_sol(rng, &pool);
        let op = match rng.below(9) {
            0 => {
                let (a, o) = classify_pair(rng, s);
                Op::Classify {
                    start: rng.chance(0.5),
                    a,
                    b: o,
                }
            }
            1 => {
                interval_safe(&mut s, rng);
                let mut o = if rng.chance(0.85) {
                    near(rng, &s)
                } else {
                    gen_sol(rng, &pool)
                };
                interval_safe(&mut o, rng);
                Op::IsEqual { a: s, b: o }
            }
            2 => {
                non_null(&mut s);
                let obj = pick_used(rng, &s, &pool);
                Op::GetIndex {
                    obj,
                    flag: rng.chance(0.6),
                    s,
                }
            }
            3 => {
                non_null(&mut s);
                let idx = rng.range_i64(0, s.no + 1);
                if let Some(o) = s.objs.get_mut(idx as usize).filter(|_| idx < s.no) {
                    if kind(*o) == POLY_VERTEX {
                        *o = 700;
                    }
                }
                Op::PointAt {
                    idx,
                    end: rng.chance(0.5),
                    s,
                }
            }
            4 => {
                non_null(&mut s);
                without(&mut s, rng, &[POLY_VERTEX]);
                let obj = pick_used(rng, &s, &pool);
                Op::PointOf {
                    obj,
                    flag: rng.chance(0.6),
                    end: rng.chance(0.5),
                    s,
                }
            }
            5 => Op::ObjIndexForParam {
                pidx: rng.range_i64(0, s.nv + 1),
                s,
            },
            6 => {
                interval_safe(&mut s, rng);
                Op::ParamInterval {
                    pidx: rng.range_i64(0, s.nv + 1),
                    s,
                }
            }
            7 => {
                let idx = rng.range_i64(0, s.no + 1);
                if let Some(o) = s.objs.get_mut(idx as usize).filter(|_| idx < s.no) {
                    *o = -1;
                }
                Op::ObjTol { idx, s }
            }
            _ => Op::Copy { s },
        };
        ops.push(op);
    }
    Inputs { ops }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "geom.solution_array"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let (oi, of) = ffi::run(&prog(&i.ops)).map_err(|e| harness(e.to_string()))?;
        Ok(Output::new().ints("ints", oi).values("floats", of))
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        match run_rust(&i.ops) {
            Ok((oi, of)) => Ok(Output::new().ints("ints", oi).values("floats", of)),
            Err(RunError::Rust(e)) => {
                Ok(Output::unported(&e).unwrap_or_else(|| Output::err(format!("rust_error: {e}"))))
            }
            Err(RunError::Harness(e)) => Err(e),
        }
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let i = if rng.chance(0.5) {
            gen_array(rng)
        } else {
            gen_queries(rng)
        };
        serde_json::to_value(i).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }
}
