//! Runner for unit `intersect.curve_class_p2`: the part-2 members of `SmCurveClassification`
//! (assignment, reset, interval insertion, merging and editing, boundary zone tolerance,
//! location and dead topology), `SmCurveInterval::GetMinUV` / `GetMaxUV` and the entry checks
//! of `sm_RefineCCIToSurfaces`.
//!
//! C++: `solidcore_ffi::units::intersect_curve_class_p2` (upstream
//! `source/SMLib/src/SmCurveClass.cpp`). Rust: `solidcore::intersect::curve_class`.
//!
//! A case is a script: a pool of `objects` (as in unit `intersect.curve_class_p1`: curves of
//! every kind the code branches on, 2D curves, planes, B-spline surfaces, points, vertex, edge,
//! face, loop and other stand-ins), curves on B-spline surfaces appended to the pool, two
//! classifications built from data (curve, surfaces, UV curves, face, topology intersector and
//! other classification stand-ins, flags, target loops, zone tolerance, intervals with their
//! three point classifications), and `ops` on them. Both sides produce the flat record
//! documented in the ffi module, reported as the `out` values. A Rust error maps to its
//! `SmStatus` (`InvalidInput` 1007); an `Unported` error anywhere defers the case.
//!
//! Deferred cases: the topology zone tolerances of point classifications (vertex, edge and
//! face objects, through `intersect.point_class`), the live-topology test of
//! `HasDeadTopologyPointObject` (`topology.brep_p1`) and the face surface of `GetMinUV` /
//! `GetMaxUV` for a 3D curve (`topology.face_p2`). Generated scripts are filtered by the
//! shim's run-time checks (scripts it rejects as undefined upstream, or as topology walks over
//! the bare stand-ins, are dropped).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use solidcore::curve::crv_on_surf::CrvOnSurf;
use solidcore::curve::curve::p2::Curve;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::solution_array::{Solution, SolutionObject, SolutionType};
use solidcore::geom::tol::zone_tol3d;
use solidcore::intersect::curve_class::{
    self as cc_mod, CurveClassification, CurveInterval, IntervalPosition, ResetOptions,
};
use solidcore::intersect::point_class::{ClassObjectKind, PointClassType, PointClassification};
use solidcore::kernel::Kernel;
use solidcore::math::{Point2, Vec3};
use solidcore::surface::surface::SurfaceKind;
use solidcore_ffi::units::intersect_curve_class_p1::kind;
use solidcore_ffi::units::intersect_curve_class_p2::{self as ffi, op};

use super::intersect_curve_class_p1::{
    b, build_pool, curve_block, gen_bspline, gen_objects, gen_tol, id, must, of_kind, opt_id,
    put_pc, rp, st, surface_block, BSplineIn, CurveIn, Fail, LineIn, ObjIn, Pool, PoolObj, H,
    NOW, NOZ, SC_ERR_ARG, USE_DEFAULT,
};
use crate::case::{CaseContext, CaseSource};
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1500;

/// `intersect.curve_class_p2` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------------

/// A point classification: `SetClassObject(class, obj)`, `SetTParam(t)`, `SetGap3d(gap)`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct PcIn {
    class: i32,
    obj: Option<usize>,
    t: f64,
    gap: f64,
}

/// An interval `[a, b]` and its start, mid and end classifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct IvlIn {
    a: f64,
    b: f64,
    pcs: [PcIn; 3],
}

/// A classification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
struct CcIn {
    curve: Option<usize>,
    #[serde(default)]
    s1: Option<usize>,
    #[serde(default)]
    uv1: Option<usize>,
    #[serde(default)]
    s2: Option<usize>,
    #[serde(default)]
    uv2: Option<usize>,
    #[serde(default)]
    face: Option<usize>,
    #[serde(default)]
    ti: Option<usize>,
    #[serde(default)]
    other: Option<usize>,
    #[serde(default)]
    fillet: bool,
    #[serde(default = "yes")]
    current: bool,
    #[serde(default)]
    ignore: bool,
    #[serde(default = "yes")]
    ok_tol: bool,
    #[serde(default)]
    tgt_only: bool,
    #[serde(default)]
    loops: Vec<usize>,
    tol: f64,
    ivls: Vec<IvlIn>,
}

fn yes() -> bool {
    true
}

/// The arguments of `ReSet`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
struct ResetIn {
    curve: Option<usize>,
    ivl: Option<[f64; 2]>,
    uv: Option<usize>,
    tol: f64,
    owned: bool,
    current: bool,
    s1: Option<usize>,
    s2: Option<usize>,
    uv2: Option<usize>,
    ti: Option<usize>,
}

/// A solution of `sm_RefineCCIToSurfaces`: `type s0 s1 e0 e1 value`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct SolIn {
    ty: usize,
    s: [f64; 2],
    e: [f64; 2],
    value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Assign { t: usize },
    ObjTol { t: usize, j: usize },
    Dead { t: usize, brep: bool },
    CrvLoc { t: usize, j: usize },
    Insert { t: usize, j: usize, ivl: IvlIn },
    Reset { t: usize, r: ResetIn },
    Connect { t: usize, j: usize, end: usize, allow: bool },
    UpdatePc { t: usize, j: usize, tol: f64, pc: PcIn },
    EditMin { t: usize, j: usize, param: f64, collapse: bool },
    EditMax { t: usize, j: usize, param: f64, collapse: bool },
    MinUv { t: usize, j: usize, uv: [f64; 2] },
    MaxUv { t: usize, j: usize, uv: [f64; 2] },
    Refine { t: usize, sol: SolIn, edge: Option<usize>, sss: [f64; 3], count: u64 },
}

impl OpIn {
    fn code_t(&self) -> (usize, usize) {
        match *self {
            OpIn::Assign { t } => (op::ASSIGN, t),
            OpIn::ObjTol { t, .. } => (op::OBJ_TOL, t),
            OpIn::Dead { t, .. } => (op::DEAD, t),
            OpIn::CrvLoc { t, .. } => (op::CRV_LOC, t),
            OpIn::Insert { t, .. } => (op::INSERT, t),
            OpIn::Reset { t, .. } => (op::RESET, t),
            OpIn::Connect { t, .. } => (op::CONNECT, t),
            OpIn::UpdatePc { t, .. } => (op::UPDATE_PC, t),
            OpIn::EditMin { t, .. } => (op::EDIT_MIN, t),
            OpIn::EditMax { t, .. } => (op::EDIT_MAX, t),
            OpIn::MinUv { t, .. } => (op::MIN_UV, t),
            OpIn::MaxUv { t, .. } => (op::MAX_UV, t),
            OpIn::Refine { t, .. } => (op::REFINE, t),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Script {
    objects: Vec<ObjIn>,
    /// Curves on surfaces `(uv curve, B-spline surface)`, appended to the pool.
    #[serde(default)]
    cos: Vec<[usize; 2]>,
    cc: [CcIn; 2],
    ops: Vec<OpIn>,
}

// ---- C++ side ---------------------------------------------------------------------------

fn objects_block(v: &mut Vec<f64>, objects: &[ObjIn]) {
    v.push(objects.len() as f64);
    for o in objects {
        match o {
            ObjIn::Curve(c) => {
                v.push(kind::CURVE as f64);
                curve_block(v, c);
            }
            ObjIn::Plane { o, n } => {
                v.push(kind::PLANE as f64);
                v.extend_from_slice(o);
                v.extend_from_slice(n);
            }
            ObjIn::Bsurf(sf) => {
                v.push(kind::BSURF as f64);
                surface_block(v, sf);
            }
            ObjIn::Point(p) => {
                v.push(kind::POINT as f64);
                v.extend_from_slice(p);
            }
            ObjIn::Vertex => v.push(kind::VERTEX as f64),
            ObjIn::Edge(c) => v.extend([kind::EDGE as f64, opt_id(*c)]),
            ObjIn::Face(sf) => v.extend([kind::FACE as f64, opt_id(*sf)]),
            ObjIn::Loop => v.push(kind::LOOP as f64),
            ObjIn::Other => v.push(kind::OTHER as f64),
        }
    }
}

fn pc_block(v: &mut Vec<f64>, pc: &PcIn) {
    v.extend([f64::from(pc.class), opt_id(pc.obj), pc.t, pc.gap]);
}

fn ivl_block(v: &mut Vec<f64>, ivl: &IvlIn) {
    v.extend([ivl.a, ivl.b]);
    for pc in &ivl.pcs {
        pc_block(v, pc);
    }
}

fn cpp_input(s: &Script) -> Vec<f64> {
    let mut v = Vec::new();
    objects_block(&mut v, &s.objects);
    v.push(s.cos.len() as f64);
    for [uv, sf] in &s.cos {
        v.extend([*uv as f64, *sf as f64]);
    }
    for c in &s.cc {
        v.extend([
            opt_id(c.curve),
            opt_id(c.s1),
            opt_id(c.uv1),
            opt_id(c.s2),
            opt_id(c.uv2),
            opt_id(c.face),
            opt_id(c.ti),
            opt_id(c.other),
            b(c.fillet),
            b(c.current),
            b(c.ignore),
            b(c.ok_tol),
            b(c.tgt_only),
        ]);
        v.push(c.loops.len() as f64);
        v.extend(c.loops.iter().map(|&l| l as f64));
        v.push(c.tol);
        v.push(c.ivls.len() as f64);
        for ivl in &c.ivls {
            ivl_block(&mut v, ivl);
        }
    }
    v.push(s.ops.len() as f64);
    for o in &s.ops {
        let (code, t) = o.code_t();
        v.extend([code as f64, t as f64]);
        match o {
            OpIn::Assign { .. } => {}
            OpIn::ObjTol { j, .. } | OpIn::CrvLoc { j, .. } => v.push(*j as f64),
            OpIn::Dead { brep, .. } => v.push(b(*brep)),
            OpIn::Insert { j, ivl, .. } => {
                v.push(*j as f64);
                ivl_block(&mut v, ivl);
            }
            OpIn::Reset { r, .. } => {
                let [a, bb] = r.ivl.unwrap_or([0.0, 0.0]);
                v.extend([opt_id(r.curve), b(r.ivl.is_some()), a, bb, opt_id(r.uv), r.tol]);
                v.extend([b(r.owned), b(r.current)]);
                v.extend([opt_id(r.s1), opt_id(r.s2), opt_id(r.uv2), opt_id(r.ti)]);
            }
            OpIn::Connect { j, end, allow, .. } => v.extend([*j as f64, *end as f64, b(*allow)]),
            OpIn::UpdatePc { j, tol, pc, .. } => {
                v.extend([*j as f64, *tol]);
                pc_block(&mut v, pc);
            }
            OpIn::EditMin { j, param, collapse, .. } | OpIn::EditMax { j, param, collapse, .. } => {
                v.extend([*j as f64, *param, b(*collapse)]);
            }
            OpIn::MinUv { j, uv, .. } | OpIn::MaxUv { j, uv, .. } => v.extend([*j as f64, uv[0], uv[1]]),
            OpIn::Refine { sol, edge, sss, count, .. } => {
                v.extend([sol.ty as f64, sol.s[0], sol.s[1], sol.e[0], sol.e[1], sol.value]);
                v.push(opt_id(*edge));
                v.extend_from_slice(sss);
                v.push(*count as f64);
            }
        }
    }
    v
}

fn cpp(s: &Script) -> Result<Output, HarnessError> {
    let out = ffi::run(&cpp_input(s)).map_err(|e| harness(format!("c++: {e}")))?;
    Ok(Output::new().values("out", out))
}

/// Whether the shim accepts the script (it rejects what is undefined upstream).
fn shim_accepts(s: &Script) -> bool {
    match ffi::run(&cpp_input(s)) {
        Ok(_) => true,
        Err(solidcore_ffi::FfiError::Status(c)) if c == SC_ERR_ARG => false,
        Err(_) => false,
    }
}

// ---- Rust side --------------------------------------------------------------------------

/// A solution handle naming nothing.
#[derive(Debug, Clone, Copy, PartialEq)]
struct NoObj;

impl SolutionObject for NoObj {
    fn kind(&self) -> solidcore::geom::solution_array::ObjectKind {
        solidcore::geom::solution_array::ObjectKind::Curve
    }
}

/// The pool plus the curves on surfaces.
fn build_all(k: &Kernel, s: &Script) -> Result<Pool, Fail> {
    let mut pool = build_pool(&s.objects)?;
    for [uv, sf] in &s.cos {
        let uv_curve = pool.curve_at(Some(*uv))?.ok_or_else(|| harness("no uv curve"))?.clone();
        let surface = pool.surface_at(Some(*sf))?.ok_or_else(|| harness("no surface"))?;
        let SurfaceKind::BSpline(bs) = surface.kind() else {
            return Err(harness("curve on a surface that is not a B-spline").into());
        };
        let c = must(CrvOnSurf::new(k, uv_curve, bs.clone(), None, 0))?;
        let i = pool.objs.len();
        pool.objs.push(PoolObj::Curve(Curve::from(c)));
        pool.handles.push(H(i, ClassObjectKind::Curve));
        pool.kinds.push(kind::CURVE);
    }
    Ok(pool)
}

fn set_pc(pool: &Pool, p: &mut PointClassification<H>, pc: &PcIn) -> Result<(), Fail> {
    let class = PointClassType::from_code(pc.class).ok_or_else(|| harness("bad class"))?;
    p.set_class_object(class, pool.handle(pc.obj)?);
    p.set_t_param(pc.t, false);
    p.set_gap_3d(pc.gap);
    Ok(())
}

fn set_ivl(pool: &Pool, dst: &mut CurveInterval<H>, ivl: &IvlIn) -> Result<(), Fail> {
    dst.set_interval(&Extent1d::from_min_max(ivl.a, ivl.b));
    set_pc(pool, &mut dst.start, &ivl.pcs[0])?;
    set_pc(pool, &mut dst.mid, &ivl.pcs[1])?;
    set_pc(pool, &mut dst.end, &ivl.pcs[2])
}

fn build_cc(k: &Kernel, pool: &Pool, c: &CcIn) -> Result<CurveClassification<H>, Fail> {
    let mut cc = CurveClassification::new(k);
    cc.set_curve(pool.handle(c.curve)?, false);
    cc.set_uv_curve1(pool.handle(c.uv1)?);
    cc.set_intersect_data(
        pool.handle(c.s1)?,
        pool.handle(c.s2)?,
        pool.handle(c.uv2)?,
        c.current,
        pool.handle(c.ti)?,
    );
    cc.set_face(pool.handle(c.face)?);
    if c.other.is_some() || c.fillet {
        cc.set_fillet_data(pool.handle(c.other)?, c.fillet);
    }
    cc.set_ignore_tangency(c.ignore);
    cc.set_ok_to_set_tolerance(c.ok_tol);
    cc.set_tgt_loops_only(c.tgt_only);
    for &l in &c.loops {
        cc.add_tgt_loop(pool.handle(Some(l))?.ok_or_else(|| harness("no loop"))?);
    }
    cc.set_src_zone_tol_3d(zone_tol3d(k, c.tol));
    must(cc.set_size(k, c.ivls.len()))?;
    for (i, ivl) in c.ivls.iter().enumerate() {
        set_ivl(pool, must(cc.at_mut(i))?, ivl)?;
    }
    Ok(cc)
}

/// The full classification record (unit `intersect.curve_class_p1`).
fn put_cc(o: &mut Vec<f64>, k: &Kernel, cc: &CurveClassification<H>) {
    o.push(cc.size() as f64);
    let ivl = cc.interval();
    o.extend([ivl.min(), ivl.max()]);
    o.push(cc_mod::src_zone_tol_3d_of(k, Some(cc)));
    o.extend([
        b(cc.is_curve_owned()),
        b(cc.is_curve_current()),
        b(cc.ignore_tangency()),
        b(cc.doing_cross_hatch()),
        b(cc.doing_fillets()),
        b(cc.ok_to_set_tolerance()),
        b(cc.tgt_loops_only()),
    ]);
    o.extend([
        id(cc.curve()),
        id(cc.uv_curve1()),
        id(cc.uv_curve2()),
        id(cc.any_uv_curve1()),
        id(cc.any_uv_curve2()),
        id(cc.surface1()),
        id(cc.surface2()),
        id(cc.face()),
        id(cc.topology_intersector()),
        id(cc.other_classification()),
    ]);
    o.push(cc.tgt_loops().len() as f64);
    o.extend(cc.tgt_loops().iter().map(|h| h.0 as f64));
    for ivl in cc.intervals() {
        o.extend([ivl.min(), ivl.max()]);
        put_pc(o, &ivl.start);
        put_pc(o, &ivl.mid);
        put_pc(o, &ivl.end);
    }
}

fn position(code: usize) -> Result<IntervalPosition, HarnessError> {
    Ok(match code {
        0 => IntervalPosition::Uninit,
        1 => IntervalPosition::Start,
        2 => IntervalPosition::Inside,
        3 => IntervalPosition::End,
        4 => IntervalPosition::Outside,
        _ => return Err(harness("bad interval position")),
    })
}

#[allow(clippy::too_many_lines)]
fn rust_run(s: &Script) -> Result<Vec<f64>, Fail> {
    let k = Kernel::default();
    let pool = build_all(&k, s)?;
    let mut ccs = [build_cc(&k, &pool, &s.cc[0])?, build_cc(&k, &pool, &s.cc[1])?];
    let mut o: Vec<f64> = Vec::new();
    for x in &s.ops {
        let (code, t) = x.code_t();
        if t > 1 {
            return Err(harness("bad target").into());
        }
        let other = ccs.get(1 - t).cloned().ok_or_else(|| harness("bad target"))?;
        let cc = ccs.get_mut(t).ok_or_else(|| harness("bad target"))?;
        o.push(code as f64);
        match x {
            OpIn::Assign { .. } => {
                // the shim disowns the target's curve first (it may be a pool object)
                cc.set_curve(cc.curve(), false);
                must(cc.assign_from(&k, &pool, &other))?;
                o.push(0.0);
            }
            OpIn::ObjTol { j, .. } => {
                let tol = must(cc.point_obj_zone_tol_3d(&k, &pool, *j))?;
                o.extend([0.0, tol]);
            }
            OpIn::Dead { brep, .. } => {
                // an empty brep, as on the C++ side
                let empty_brep = solidcore::topology::Brep::new();
                let r = must(cc.has_dead_topology_point_object(brep.then_some(&empty_brep)))?;
                o.extend([0.0, b(r)]);
            }
            OpIn::CrvLoc { j, .. } => {
                let (p, prm) = must(cc.classify_crv_loc_by_index(&pool, *j))?;
                o.extend([0.0, prm, p.x, p.y, p.z]);
            }
            OpIn::Insert { j, ivl, .. } => {
                let mut new = CurveInterval::new(&k, Some(cc));
                set_ivl(&pool, &mut new, ivl)?;
                must(cc.insert_at(&k, *j, new))?;
                o.push(0.0);
            }
            OpIn::Reset { r, .. } => {
                let opts = ResetOptions {
                    curve: pool.handle(r.curve)?,
                    interval: r.ivl.map(|[a, bb]| Extent1d::from_min_max(a, bb)),
                    uv_curve: pool.handle(r.uv)?,
                    zone_tol_3d: zone_tol3d(&k, r.tol),
                    is_curve_owned: r.owned,
                    is_curve_current: r.current,
                    surface1: pool.handle(r.s1)?,
                    surface2: pool.handle(r.s2)?,
                    uv_curve2: pool.handle(r.uv2)?,
                    ti: pool.handle(r.ti)?,
                };
                must(cc.reset(&k, &opts))?;
                o.push(0.0);
            }
            OpIn::Connect { j, end, allow, .. } => {
                let r = cc.connect_intervals(*j, position(*end)?, *allow);
                o.push(st(r)?.err().unwrap_or(0.0));
            }
            OpIn::UpdatePc { j, tol, pc, .. } => {
                let mut p = PointClassification::new(zone_tol3d(&k, *tol));
                set_pc(&pool, &mut p, pc)?;
                let r = cc.update_point_class(&k, &pool, *j, &p);
                o.push(st(r)?.err().unwrap_or(0.0));
            }
            OpIn::EditMin { j, param, collapse, .. } => {
                let r = cc.edit_interval_min(&k, &pool, *j, *param, *collapse);
                o.push(st(r)?.err().unwrap_or(0.0));
            }
            OpIn::EditMax { j, param, collapse, .. } => {
                let r = cc.edit_interval_max(&k, &pool, *j, *param, *collapse);
                o.push(st(r)?.err().unwrap_or(0.0));
            }
            OpIn::MinUv { j, uv, .. } | OpIn::MaxUv { j, uv, .. } => {
                let given = Point2::new(uv[0], uv[1]);
                let r = if matches!(x, OpIn::MinUv { .. }) {
                    cc.interval_min_uv(&pool, *j, given)
                } else {
                    cc.interval_max_uv(&pool, *j, given)
                };
                match st(r)? {
                    Ok(p) => o.extend([0.0, p.x, p.y]),
                    Err(code) => o.extend([code, uv[0], uv[1]]),
                }
            }
            OpIn::Refine { sol, edge, sss, count, .. } => {
                let mut solution: Solution<NoObj> = Solution::new();
                solution.solution_type = SolutionType::from_code(sol.ty as i32).ok_or_else(|| harness("bad type"))?;
                solution.start.parameters[0] = sol.s[0];
                solution.start.parameters[1] = sol.s[1];
                solution.end.parameters[0] = sol.e[0];
                solution.end.parameters[1] = sol.e[1];
                solution.start.solution_value = sol.value;
                let r = must(cc_mod::refine_cci_to_surfaces(
                    cc,
                    &solution,
                    pool.handle(*edge)?,
                    Vec3::new(sss[0], sss[1], sss[2]),
                    *count,
                ))?;
                o.extend([0.0, b(r.sss_point_flag), r.sss_point.x, r.sss_point.y, r.sss_point.z]);
                o.extend([r.deviation, r.recursion_count as f64]);
                o.extend([f64::from(solution.solution_type.code()), sol.s[0], sol.s[1], sol.e[0], sol.e[1], sol.value]);
            }
        }
        put_cc(&mut o, &k, cc);
    }
    Ok(o)
}

fn rust(s: &Script) -> Result<Output, HarnessError> {
    match rust_run(s) {
        Ok(v) => Ok(Output::new().values("out", v)),
        Err(Fail::Deferred(d)) => Ok(*d),
        Err(Fail::Harness(e)) => Err(e),
    }
}

// ---- generation -------------------------------------------------------------------------

fn of_kinds(objs: &[ObjIn], ks: &[usize]) -> Vec<usize> {
    ks.iter().flat_map(|&k| of_kind(objs, k)).collect()
}

fn maybe_of(rng: &mut Rng, v: &[usize], p_none: f64) -> Option<usize> {
    if rng.chance(p_none) {
        None
    } else {
        rng.choose(v).copied()
    }
}

/// The pool: the base pool of unit `intersect.curve_class_p1` plus a closed curve, a 2D curve
/// (UV curve), a short line, an edge on a B-spline, a face on the B-spline surface and more
/// stand-ins.
fn gen_objects_p2(rng: &mut Rng) -> Vec<ObjIn> {
    let mut objs = gen_objects(rng);
    let scale = *rng.choose(&[1.0, 1.0, 1e-4, 1e4]).unwrap_or(&1.0);
    let c = rp(rng, scale);
    let r = scale * rng.uniform(0.2, 1.0);
    let pts = [[1.0, 0.0], [1.0, 1.0], [-1.0, 1.0], [-1.0, -1.0], [1.0, -1.0], [1.0, 0.0]];
    objs.push(ObjIn::Curve(CurveIn::Bspline(BSplineIn {
        dim: 3,
        oob: false,
        cpts: pts.iter().map(|q| [c[0] + r * q[0], c[1] + r * q[1], c[2], NOW]).collect(),
        p: 3,
        u: vec![0.0, 0.0, 0.0, 0.0, 0.4, 0.7, 1.0, 1.0, 1.0, 1.0],
    })));
    // a UV curve inside [0, 1]^2 (also a 2D classification curve)
    let mut two_d = gen_bspline(rng);
    two_d.dim = 2;
    let len = two_d.cpts.len().max(2) as f64;
    for (i, cp) in two_d.cpts.iter_mut().enumerate() {
        *cp = [0.1 + 0.8 * i as f64 / (len - 1.0), rng.uniform(0.1, 0.9), NOZ, NOW];
    }
    objs.push(ObjIn::Curve(CurveIn::Bspline(two_d)));
    objs.push(ObjIn::Curve(CurveIn::Line(LineIn {
        p: [0.0; 3],
        v: [scale, 0.0, 0.0],
        domain: [0.0, 1.0],
        scale: 1.0,
        inside_out: false,
    })));
    let bsplines: Vec<usize> = objs
        .iter()
        .enumerate()
        .filter(|(_, o)| matches!(o, ObjIn::Curve(CurveIn::Bspline(_))))
        .map(|(i, _)| i)
        .collect();
    objs.push(ObjIn::Edge(rng.choose(&bsplines).copied()));
    let bsurf = of_kind(&objs, kind::BSURF).first().copied();
    objs.push(ObjIn::Face(bsurf));
    objs.push(ObjIn::Other);
    objs
}

/// A point classification; `topo` is the chance of a topology object (deferred zone
/// tolerances), points (not `SmObject`s) only with `points`.
fn gen_pc(rng: &mut Rng, objs: &[ObjIn], topo: f64, points: bool) -> PcIn {
    let geometry: &[(i32, &[usize])] = &[
        (100, &[]),
        (106, &[kind::CURVE]),
        (106, &[kind::CURVE]),
        (107, &[kind::PLANE, kind::BSURF]),
        (110, &[]),
    ];
    let topology: &[(i32, &[usize])] = &[(102, &[kind::FACE]), (103, &[kind::EDGE]), (104, &[kind::VERTEX])];
    let (class, ks) = if rng.chance(topo) {
        rng.choose(topology).copied()
    } else if points && rng.chance(0.1) {
        Some((105, &[kind::POINT][..]))
    } else {
        rng.choose(geometry).copied()
    }
    .unwrap_or((100, &[]));
    let obj = if ks.is_empty() { None } else { maybe_of(rng, &of_kinds(objs, ks), 0.05) };
    let gap = match rng.below(6) {
        0 => 0.0,
        1 => 1e-13,
        2 => 1e-6,
        3 => 1e-3,
        _ => rng.uniform(0.0, 1e-2),
    };
    PcIn {
        class,
        obj,
        t: rng.uniform(-1.0, 2.0),
        gap,
    }
}

fn gen_ivls(rng: &mut Rng, objs: &[ObjIn], n: usize, lo: f64, hi: f64, topo: f64, points: bool) -> Vec<IvlIn> {
    let mut ps: Vec<f64> = (0..=n)
        .map(|_| match rng.below(6) {
            0 => lo + 0.5 * (hi - lo),
            1 => lo,
            _ => lo + (hi - lo) * rng.unit(),
        })
        .collect();
    ps.sort_by(f64::total_cmp);
    // nearly coincident boundaries: degenerate at some tolerances only
    if n >= 2 && rng.chance(0.4) {
        let i = rng.below(n as u64 - 1) as usize;
        let d = *rng.choose(&[0.0, 1e-13, 1e-9, 1e-7, 1e-5, 1e-3]).unwrap_or(&1e-9) * (hi - lo);
        let base = ps.get(i).copied().unwrap_or(lo);
        if let Some(x) = ps.get_mut(i + 1) {
            *x = base + d;
        }
        ps.sort_by(f64::total_cmp);
    }
    if rng.chance(0.5) {
        if let Some(f) = ps.first_mut() {
            *f = lo;
        }
        if let Some(l) = ps.last_mut() {
            *l = hi;
        }
    }
    let mut ivls: Vec<IvlIn> = Vec::new();
    for w in ps.windows(2) {
        let (Some(&a), Some(&bb)) = (w.first(), w.get(1)) else {
            continue;
        };
        let mut pcs = [
            gen_pc(rng, objs, topo, points),
            gen_pc(rng, objs, topo, points),
            gen_pc(rng, objs, topo, points),
        ];
        if let Some(prev) = ivls.last() {
            if rng.chance(0.8) {
                pcs[0] = prev.pcs[2];
            }
        }
        ivls.push(IvlIn { a, b: bb, pcs });
    }
    ivls
}

/// The natural interval of pool curve `c` as the generator sees it (for parameters).
fn curve_range(objs: &[ObjIn], c: Option<usize>) -> (f64, f64) {
    match c.and_then(|i| objs.get(i)) {
        Some(ObjIn::Curve(CurveIn::Bspline(bs))) => (
            bs.u.first().copied().unwrap_or(0.0),
            bs.u.last().copied().unwrap_or(1.0),
        ),
        Some(ObjIn::Curve(CurveIn::Line(l))) => (l.domain[0], l.domain[1]),
        _ => (0.0, 1.0),
    }
}

fn gen_cc(rng: &mut Rng, objs: &[ObjIn], n_cos: usize, topo: f64, points: bool) -> CcIn {
    let mut curves = of_kind(objs, kind::CURVE);
    curves.extend(objs.len()..objs.len() + n_cos);
    let surfs = of_kinds(objs, &[kind::PLANE, kind::BSURF]);
    let others = of_kind(objs, kind::OTHER);
    let loops = of_kind(objs, kind::LOOP);
    let faces = of_kind(objs, kind::FACE);
    let curve = if n_cos > 0 && rng.chance(0.3) {
        Some(objs.len() + rng.below(n_cos as u64) as usize)
    } else {
        maybe_of(rng, &curves, 0.05)
    };
    let (lo, hi) = if curve.is_some_and(|c| c >= objs.len()) {
        (0.0, 1.0)
    } else {
        curve_range(objs, curve)
    };
    let n = rng.below(6) as usize;
    CcIn {
        curve,
        s1: maybe_of(rng, &surfs, 0.4),
        uv1: maybe_of(rng, &curves, 0.6),
        s2: maybe_of(rng, &surfs, 0.4),
        uv2: maybe_of(rng, &curves, 0.6),
        face: maybe_of(rng, &faces, 0.5),
        ti: maybe_of(rng, &others, 0.6),
        other: maybe_of(rng, &others, 0.6),
        fillet: rng.chance(0.3),
        current: rng.chance(0.7),
        ignore: rng.chance(0.3),
        ok_tol: rng.chance(0.7),
        tgt_only: rng.chance(0.3),
        loops: (0..rng.below(3)).filter_map(|_| rng.choose(&loops).copied()).collect(),
        tol: gen_tol(rng),
        ivls: gen_ivls(rng, objs, n, lo, hi, topo, points),
    }
}

/// A parameter for an interval edit: the allowed range's ends, the neighbour boundary (a
/// zero-length interval), values just inside, and sometimes outside.
fn edit_param(rng: &mut Rng, lo: f64, hi: f64) -> f64 {
    let w = hi - lo;
    match rng.below(9) {
        0 => lo,
        1 => hi,
        2 => lo + 1e-12 * w.abs().max(1.0),
        3 => hi - 1e-9 * w.abs().max(1.0),
        4 => lo - 0.1 - w.abs(),
        5 => hi + 1e-9,
        _ => lo + w * rng.unit(),
    }
}

#[allow(clippy::too_many_lines)]
fn gen_op(rng: &mut Rng, objs: &[ObjIn], n_cos: usize, ccs: &[CcIn; 2]) -> OpIn {
    let t = rng.below(2) as usize;
    let [c0, c1] = ccs;
    let c = if t == 0 { c0 } else { c1 };
    let size = c.ivls.len();
    let j = rng.below(size as u64 + 1) as usize;
    let mut curves = of_kind(objs, kind::CURVE);
    curves.extend(objs.len()..objs.len() + n_cos);
    let surfs = of_kinds(objs, &[kind::PLANE, kind::BSURF]);
    let others = of_kind(objs, kind::OTHER);
    let edges = of_kind(objs, kind::EDGE);
    match rng.below(28) {
        0..=1 => OpIn::Assign { t },
        2 => OpIn::ObjTol { t, j },
        3 => OpIn::Dead { t, brep: rng.chance(0.7) },
        4..=5 => OpIn::CrvLoc { t, j },
        6..=7 => {
            let (lo, hi) = curve_range(objs, c.curve);
            let mut ivl = gen_ivls(rng, objs, 1, lo, hi, 0.1, false);
            let jj = if rng.chance(0.3) { size + rng.below(3) as usize } else { j };
            OpIn::Insert {
                t,
                j: jj,
                ivl: ivl.pop().unwrap_or(IvlIn { a: 0.0, b: 1.0, pcs: [gen_pc(rng, objs, 0.0, false); 3] }),
            }
        }
        8..=10 => {
            let fresh = rng.chance(0.6);
            let curve = if fresh && rng.chance(0.85) { maybe_of(rng, &curves, 0.0) } else { None };
            let uv = if fresh && (curve.is_none() || rng.chance(0.4)) { maybe_of(rng, &curves, 0.0) } else { None };
            let (lo, hi) = curve_range(objs, curve);
            let ivl = if fresh || rng.chance(0.3) {
                Some(if rng.chance(0.8) { [lo, hi] } else { [lo + 0.25 * (hi - lo), lo + 0.75 * (hi - lo)] })
            } else {
                None
            };
            OpIn::Reset {
                t,
                r: ResetIn {
                    curve,
                    ivl,
                    uv,
                    tol: match rng.below(5) {
                        0 => 0.0,
                        1 => -1e-4,
                        2 => USE_DEFAULT,
                        3 => 1e-5,
                        _ => gen_tol(rng),
                    },
                    owned: rng.chance(0.3),
                    current: rng.chance(0.7),
                    s1: maybe_of(rng, &surfs, 0.4),
                    s2: maybe_of(rng, &surfs, 0.4),
                    uv2: maybe_of(rng, &curves, 0.6),
                    ti: maybe_of(rng, &others, 0.6),
                },
            }
        }
        11..=13 => OpIn::Connect {
            t,
            j: if rng.chance(0.9) { rng.below(size.max(1) as u64) as usize } else { size + 1 },
            end: *rng.choose(&[1, 1, 1, 3, 3, 3, 0, 2, 4]).unwrap_or(&1),
            allow: rng.chance(0.5),
        },
        14..=15 => OpIn::UpdatePc {
            t,
            j,
            tol: gen_tol(rng),
            pc: gen_pc(rng, objs, 0.15, true),
        },
        16..=21 => {
            let jj = rng.below(size.max(1) as u64) as usize;
            let (lo, hi) = curve_range(objs, c.curve);
            let collapse = rng.chance(0.8);
            let get = |i: usize| c.ivls.get(i);
            if rng.chance(0.5) {
                let a = if jj > 0 { get(jj - 1).map_or(lo, |v| v.a) } else { lo };
                let bb = get(jj).map_or(hi, |v| v.b);
                OpIn::EditMin { t, j: jj, param: edit_param(rng, a, bb), collapse }
            } else {
                let a = get(jj).map_or(lo, |v| v.a);
                let bb = if jj + 1 < size { get(jj + 1).map_or(hi, |v| v.b) } else { hi };
                OpIn::EditMax { t, j: jj, param: edit_param(rng, bb, a), collapse }
            }
        }
        22..=24 => {
            let jj = rng.below(size.max(1) as u64) as usize;
            let uv = [rng.uniform(-1.0, 2.0), rng.uniform(-1.0, 2.0)];
            if rng.chance(0.5) {
                OpIn::MinUv { t, j: jj, uv }
            } else {
                OpIn::MaxUv { t, j: jj, uv }
            }
        }
        _ => {
            let s0 = rng.unit();
            let s1 = rng.unit();
            let (e0, e1) = match rng.below(4) {
                0 => (s0, rng.unit()),
                1 => (rng.unit(), s1),
                2 => (s0 + 1e-15, s1 + 0.5),
                _ => (rng.unit(), rng.unit()),
            };
            OpIn::Refine {
                t,
                sol: SolIn {
                    ty: *rng.choose(&[1, 2, 2, 2, 0, 3, 4]).unwrap_or(&2),
                    s: [s0, s1],
                    e: [e0, e1],
                    value: rng.uniform(0.0, 1e-3),
                },
                edge: maybe_of(rng, &edges, 0.3),
                sss: rp(rng, 10.0),
                count: *rng.choose(&[0, 3, 5, 6, 9]).unwrap_or(&0),
            }
        }
    }
}

fn gen_script(rng: &mut Rng) -> Script {
    let objects = gen_objects_p2(rng);
    let bsurfs = of_kind(&objects, kind::BSURF);
    let curves = of_kind(&objects, kind::CURVE);
    let n_cos = rng.below(3) as usize;
    let cos: Vec<[usize; 2]> = (0..n_cos)
        .filter_map(|_| {
            // the UV curve (the last 2D curve of the pool, usually)
            let uv = if rng.chance(0.8) { curves.get(curves.len().saturating_sub(2)).copied() } else { rng.choose(&curves).copied() };
            Some([uv?, rng.choose(&bsurfs).copied()?])
        })
        .collect();
    let n_cos = cos.len();
    // mostly comparable: few topology objects
    let topo = *rng.choose(&[0.0, 0.0, 0.05, 0.3]).unwrap_or(&0.0);
    let points = rng.chance(0.3);
    let cc = [gen_cc(rng, &objects, n_cos, topo, points), gen_cc(rng, &objects, n_cos, topo, points)];
    let n_ops = 1 + rng.below(5) as usize;
    let ops = (0..n_ops).map(|_| gen_op(rng, &objects, n_cos, &cc)).collect();
    Script { objects, cos, cc, ops }
}

// ---- fixtures built in code ---------------------------------------------------------------

fn pc(class: i32, obj: Option<usize>, t: f64, gap: f64) -> PcIn {
    PcIn { class, obj, t, gap }
}

fn unknown() -> PcIn {
    pc(100, None, 0.0, 0.0)
}

fn plain_cc(curve: Option<usize>, tol: f64, ivls: Vec<IvlIn>) -> CcIn {
    CcIn {
        curve,
        s1: None,
        uv1: None,
        s2: None,
        uv2: None,
        face: None,
        ti: None,
        other: None,
        fillet: false,
        current: true,
        ignore: false,
        ok_tol: true,
        tgt_only: false,
        loops: vec![],
        tol,
        ivls,
    }
}

/// Edge cases: huge- and tiny-scale classifications, empty ones, boundary indices, zero-length
/// edits at tolerance limits, both reset branches with zero and default tolerances, every early
/// return of the refinement.
#[allow(clippy::too_many_lines)]
fn edge_cases() -> Vec<(String, Value)> {
    let mut out = Vec::new();
    for (name, scale) in [("unit", 1.0), ("huge", 1e5), ("tiny", 1e-5)] {
        let objs = vec![
            ObjIn::Curve(CurveIn::Line(LineIn {
                p: [0.0; 3],
                v: [scale, 0.0, 0.0],
                domain: [0.0, 1.0],
                scale: 1.0,
                inside_out: false,
            })),
            ObjIn::Plane { o: [0.0; 3], n: [0.0, 0.0, 1.0] },
            ObjIn::Curve(CurveIn::Bspline(BSplineIn {
                dim: 2,
                oob: false,
                cpts: vec![[0.1, 0.2, NOZ, NOW], [0.5, 0.9, NOZ, NOW], [0.9, 0.4, NOZ, NOW]],
                p: 2,
                u: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            })),
            ObjIn::Face(Some(1)),
            ObjIn::Other,
            ObjIn::Loop,
            ObjIn::Edge(Some(0)),
        ];
        let ivls = vec![
            IvlIn { a: 0.0, b: 0.25, pcs: [pc(106, Some(0), 0.0, 1e-9), unknown(), pc(107, Some(1), 0.1, 0.0)] },
            IvlIn { a: 0.25, b: 0.25 + 1e-12, pcs: [pc(107, Some(1), 0.1, 0.0), unknown(), pc(106, Some(2), 0.2, 1e-7)] },
            IvlIn { a: 0.25 + 1e-12, b: 0.5, pcs: [pc(106, Some(2), 0.2, 1e-7), unknown(), unknown()] },
            IvlIn { a: 0.5, b: 0.5 + 1e-6, pcs: [unknown(), pc(110, None, 0.0, 0.0), unknown()] },
            IvlIn { a: 0.5 + 1e-6, b: 1.0, pcs: [unknown(), unknown(), pc(106, Some(0), 1.0, 2e-3)] },
        ];
        let mut cc0 = plain_cc(Some(0), 1e-5 * scale, ivls.clone());
        cc0.s1 = Some(1);
        cc0.face = Some(3);
        cc0.loops = vec![5];
        cc0.other = Some(4);
        cc0.fillet = true;
        let mut cc1 = plain_cc(Some(2), USE_DEFAULT, vec![IvlIn { a: 0.0, b: 1.0, pcs: [unknown(); 3] }]);
        cc1.s1 = Some(1);
        cc1.s2 = Some(1);
        let fresh = ResetIn {
            curve: Some(0),
            ivl: Some([0.0, 1.0]),
            uv: None,
            tol: 0.0,
            owned: true,
            current: false,
            s1: Some(1),
            s2: None,
            uv2: Some(2),
            ti: Some(4),
        };
        let keep = ResetIn { curve: None, ivl: None, uv: None, tol: USE_DEFAULT, owned: false, current: true, s1: None, s2: None, uv2: None, ti: None };
        let ivl = |a: f64, bb: f64| IvlIn { a, b: bb, pcs: [unknown(); 3] };
        let sol = |ty: usize, s: [f64; 2], e: [f64; 2]| SolIn { ty, s, e, value: 1e-4 };
        let ops = vec![
            OpIn::ObjTol { t: 0, j: 0 },
            OpIn::ObjTol { t: 0, j: 5 },
            OpIn::CrvLoc { t: 0, j: 0 },
            OpIn::CrvLoc { t: 0, j: 5 },
            OpIn::CrvLoc { t: 1, j: 1 },
            OpIn::Dead { t: 0, brep: true },
            OpIn::Dead { t: 0, brep: false },
            OpIn::MaxUv { t: 1, j: 0, uv: [0.5, 0.5] },
            OpIn::MinUv { t: 1, j: 0, uv: [0.5, 0.5] },
            OpIn::Refine { t: 0, sol: sol(2, [0.1, 0.2], [0.3, 0.4]), edge: None, sss: [1.0, 2.0, 3.0], count: 0 },
            OpIn::Refine { t: 0, sol: sol(1, [0.1, 0.2], [0.1, 0.2]), edge: None, sss: [1.0, 2.0, 3.0], count: 0 },
            OpIn::Refine { t: 1, sol: sol(1, [0.1, 0.2], [0.1, 0.2]), edge: Some(6), sss: [1.0, 2.0, 3.0], count: 6 },
            OpIn::Refine { t: 1, sol: sol(2, [0.1, 0.2], [0.1, 0.9]), edge: Some(6), sss: [1.0, 2.0, 3.0], count: 7 },
            OpIn::EditMin { t: 0, j: 2, param: 0.25, collapse: true },
            OpIn::EditMax { t: 0, j: 0, param: 0.25 + 1e-12, collapse: false },
            OpIn::EditMax { t: 0, j: 2, param: 0.5 + 1e-6, collapse: true },
            OpIn::EditMin { t: 0, j: 0, param: -1.0, collapse: true },
            OpIn::EditMin { t: 0, j: 9, param: 0.0, collapse: true },
            OpIn::Connect { t: 0, j: 0, end: 1, allow: false },
            OpIn::Connect { t: 0, j: 1, end: 3, allow: true },
            OpIn::Connect { t: 0, j: 7, end: 3, allow: true },
            OpIn::UpdatePc { t: 0, j: 1, tol: 1e-3, pc: pc(106, Some(0), 0.3, 1e-8) },
            OpIn::UpdatePc { t: 0, j: 0, tol: 1e-3, pc: pc(107, Some(1), 0.3, 1e-8) },
            OpIn::Insert { t: 0, j: 1, ivl: ivl(0.1, 0.2) },
            OpIn::Insert { t: 0, j: 9, ivl: ivl(0.9, 1.0) },
            OpIn::Assign { t: 1 },
            OpIn::Reset { t: 0, r: keep },
            OpIn::Insert { t: 0, j: 3, ivl: ivl(0.5, 0.6) },
            OpIn::Reset { t: 0, r: fresh },
            OpIn::Assign { t: 1 },
            OpIn::Insert { t: 1, j: 2, ivl: ivl(0.5, 0.6) },
            OpIn::Reset { t: 1, r: ResetIn { curve: None, uv: Some(2), owned: false, tol: -1e-6, ..fresh } },
        ];
        // a 3D curve on a face: the face's surface (deferred)
        let uv3d = vec![OpIn::MinUv { t: 0, j: 0, uv: [0.5, 0.5] }];
        out.push((
            format!("edge_uv_on_face_{name}"),
            json!(Script { objects: objs.clone(), cos: vec![], cc: [cc0.clone(), cc1.clone()], ops: uv3d }),
        ));
        out.push((
            format!("edge_ops_{name}"),
            json!(Script { objects: objs, cos: vec![], cc: [cc0, cc1], ops }),
        ));
    }
    // empty classifications (no array allocated)
    let objs = vec![ObjIn::Curve(CurveIn::Line(LineIn {
        p: [0.0; 3],
        v: [1.0, 0.0, 0.0],
        domain: [0.0, 1.0],
        scale: 1.0,
        inside_out: false,
    }))];
    let ops = vec![
        OpIn::Dead { t: 0, brep: true },
        OpIn::Reset { t: 0, r: ResetIn { curve: None, ivl: None, uv: None, tol: 1e-3, owned: false, current: false, s1: None, s2: None, uv2: None, ti: None } },
        OpIn::Connect { t: 0, j: 0, end: 1, allow: true },
        OpIn::EditMax { t: 0, j: 0, param: 0.5, collapse: true },
        OpIn::Assign { t: 1 },
        OpIn::Insert { t: 0, j: 0, ivl: IvlIn { a: 0.0, b: 1.0, pcs: [unknown(); 3] } },
        OpIn::Insert { t: 1, j: 2, ivl: IvlIn { a: 0.5, b: 1.0, pcs: [unknown(); 3] } },
    ];
    out.push((
        "edge_empty".into(),
        json!(Script { objects: objs, cos: vec![], cc: [plain_cc(Some(0), 1e-5, vec![]), plain_cc(None, 1e-5, vec![])], ops }),
    ));
    out
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "intersect.curve_class_p2"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let s = gen_script(rng);
            if ffi::validate(&cpp_input(&s)).is_err() || !shim_accepts(&s) {
                continue;
            }
            return serde_json::to_value(s).ok();
        }
        None
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        let mut cases = default_cases(self, &ctx)?;
        for (name, inputs) in edge_cases() {
            cases.push(Case {
                unit: self.id().into(),
                name,
                source: CaseSource::Inline,
                inputs,
            });
        }
        Ok(cases)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generated scripts are accepted by the ffi checks, and most of them are comparable now
    /// (not deferred).
    #[test]
    fn generated_scripts_are_mostly_comparable() {
        let mut rng = Rng::new(11);
        let (mut total, mut deferred) = (0, 0);
        for _ in 0..300 {
            let Some(v) = Runner.generate(&mut rng) else {
                continue;
            };
            let s: Script = serde_json::from_value(v).unwrap();
            total += 1;
            if matches!(rust_run(&s), Err(Fail::Deferred(_))) {
                deferred += 1;
            }
        }
        assert!(total > 250, "{total}");
        assert!(deferred * 3 < total, "{deferred} of {total} deferred");
    }

    #[test]
    fn edge_cases_are_accepted() {
        for (name, v) in edge_cases() {
            let s: Script = serde_json::from_value(v).unwrap();
            assert!(ffi::validate(&cpp_input(&s)).is_ok(), "{name}");
            assert!(shim_accepts(&s), "{name}");
        }
    }
}

#[cfg(test)]
mod dump {
    use super::*;

    /// Prints how often the comparable generated scripts reach each op and status (a
    /// debugging aid: `cargo test -p difftest --features cpp --lib p2_coverage -- --ignored
    /// --nocapture`).
    #[test]
    #[ignore = "debugging aid"]
    fn p2_coverage() {
        let mut stats: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
        for i in 0..MIN_GENERATED {
            let mut rng = Rng::new(CaseContext::seed_for("intersect.curve_class_p2", i));
            let Some(v) = Runner.generate(&mut rng) else { continue };
            let s: Script = serde_json::from_value(v).unwrap();
            if rust_run(&s).is_err() {
                *stats.entry("deferred".into()).or_default() += 1;
                continue;
            }
            // each op alone after the ones before it: the status and the size change
            let mut prev_len = 0;
            for n in 1..=s.ops.len() {
                let sub = Script { objects: s.objects.clone(), cos: s.cos.clone(), cc: s.cc.clone(), ops: s.ops[..n].to_vec() };
                let Ok(out) = rust_run(&sub) else { break };
                let code = out[prev_len] as usize;
                let status = out[prev_len + 1];
                *stats.entry(format!("{code:2} status={status}")).or_default() += 1;
                prev_len = out.len();
            }
        }
        for (k, v) in stats {
            println!("{v:6} {k}");
        }
    }
}
