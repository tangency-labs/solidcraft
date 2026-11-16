//! Runner for unit `intersect.curve_class_p2`: the part-2 members of `SmCurveClassification`
//! (assignment, reset, interval insertion, merging and editing, boundary zone tolerance,
//! location and dead topology), `SmCurveInterval::GetMinUV` / `GetMaxUV` and the entry checks
//! of `sm_RefineCCIToSurfaces`.
//!
//! Ported from upstream `source/SMLib/src/SmCurveClass.cpp`; checked against its golden.
//! Rust: `solidcore::intersect::curve_class`.
//!
//! A case is a script: a pool of `objects` (as in unit `intersect.curve_class_p1`: curves of
//! every kind the code branches on, 2D curves, planes, B-spline surfaces, points, vertex, edge,
//! face, loop and other stand-ins), curves on B-spline surfaces appended to the pool, two
//! classifications built from data (curve, surfaces, UV curves, face, topology intersector and
//! other classification stand-ins, flags, target loops, zone tolerance, intervals with their
//! three point classifications), and `ops` on them. The Rust side produces a flat record (per
//! op: code, status, outputs, state), reported as the `out` values. A Rust error maps to its
//! `SmStatus` (`InvalidInput` 1007); an `Unported` error anywhere defers the case.
//!
//! Three in ten generated scripts add a real one-face B-rep (`face`, as in unit
//! `intersect.curve_class_p4`: `heal_face_props::rust_brep` plus its down face use's side) after the curves on surfaces in the pool; their
//! face, edge and vertex classifications (and most classification faces) then name its real
//! topology, so the zone tolerances, `HasDeadTopologyPointObject` (also against that B-rep)
//! and the face surface drop of `GetMinUV` / `GetMaxUV` run the ported topology through
//! `ClassGeometry::topology_entity`.
//!
//! Some classification faces intentionally stay on the base pool's bare face while that
//! B-rep is present; the bare face owns the same B-spline surface. [`P2Geometry`] falls back
//! to the bare record only when the real topology lookup misses, so the UV surface drop
//! answers from the stand-in surface rather than deferring it.
//!
//! Without that B-rep, the vertex, edge and face stand-ins are B-rep records in their
//! bare state (see unit `intersect.curve_class_p1`,
//! [`BareTopology`](super::intersect_curve_class_p1::BareTopology)), so the members those
//! records answer (zone tolerances, edge curve / interval, face surface) are recorded too.
//!
//! Deferred cases: members on the bare vertex and edge stand-ins (no B-rep behind them:
//! `topology.vertex`, `topology.edge`, `topology.brep_p1`). Generated scripts keep the input
//! contract upstream defines ([`keeps_contract`]: before each op its preconditions are
//! checked on the classifications the ops before it left); the generator redraws the others.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use solidcore::curve::crv_on_surf::CrvOnSurf;
use solidcore::curve::curve::p2::Curve;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::solution_array::{Solution, SolutionObject, SolutionType};
use solidcore::geom::tol::{are_same, zone_tol3d};
use solidcore::intersect::curve_class::{
    self as cc_mod, CurveClassification, CurveInterval, IntervalPosition, ResetOptions,
};
use solidcore::intersect::point_class::{
    ClassGeometry, ClassObjectKind, PointClassType, PointClassification,
};
use solidcore::kernel::Kernel;
use solidcore::math::{Point2, Vec3};
use solidcore::surface::surface::Surface;
use solidcore::surface::volume::Volume;
use solidcore::topology::Brep;
use solidcore::EntityRef;


use super::intersect_curve_class_p1::{
    refused,
    b, build_pool, gen_bspline, gen_objects, gen_tol, id, must, of_kind, put_pc, rp, st,
    BSplineIn, CurveIn, Fail, LineIn, ObjIn, Pool, PoolObj, H, NOW, NOZ, USE_DEFAULT,
};
use super::intersect_curve_class_p1::{
    add_face, kind, kind_of_class, ContractPool,
};
use super::intersect_curve_class_p4::{face_keeps_contract, gen_face, topology_kinds};
use crate::units::heal_face_props::{FaceIn, LoopsIn};
use crate::case::{CaseContext, CaseSource};
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The op codes of a script record.
mod op {
    pub const ASSIGN: usize = 0;
    pub const OBJ_TOL: usize = 1;
    pub const DEAD: usize = 2;
    pub const CRV_LOC: usize = 3;
    pub const INSERT: usize = 4;
    pub const RESET: usize = 5;
    pub const CONNECT: usize = 6;
    pub const UPDATE_PC: usize = 7;
    pub const EDIT_MIN: usize = 8;
    pub const EDIT_MAX: usize = 9;
    pub const MIN_UV: usize = 10;
    pub const MAX_UV: usize = 11;
    pub const REFINE: usize = 12;
}

/// Most intervals of a classification.
const MAX_IVLS: usize = 16;
/// Most target loops of a classification.
const MAX_LOOPS: usize = 4;
/// Most curves on surfaces.
const MAX_COS: usize = 4;

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
    /// `brep`: an empty B-rep; `live` (with a face): the face's B-rep instead.
    Dead {
        t: usize,
        brep: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        live: bool,
    },
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
    /// A real one-face B-rep (as in unit `intersect.curve_class_p4`) whose topology follows
    /// the curves on surfaces in the pool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    face: Option<FaceIn>,
    /// Curves on surfaces `(uv curve, B-spline surface)`, appended to the pool.
    #[serde(default)]
    cos: Vec<[usize; 2]>,
    cc: [CcIn; 2],
    ops: Vec<OpIn>,
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

        let c = must(CrvOnSurf::new(k, uv_curve, surface.clone(), None, 0))?;
        let i = pool.objs.len();
        pool.objs.push(PoolObj::Curve(Curve::from(c)));
        pool.handles.push(H(i, ClassObjectKind::Curve));
        pool.kinds.push(kind::CURVE);
    }
    Ok(pool)
}

/// The pool with bare topology records as a fallback. [`Pool::topology_entity`] resolves
/// either the real one-face B-rep or the bare stand-ins, never both; a classification face
/// can stay on a bare face while that B-rep is present. This view keeps every other
/// [`ClassGeometry`] method delegated unchanged and resolves such a handle through the
/// bare record only when the pool's real topology lookup misses.
struct P2Geometry<'a>(&'a Pool);

impl ClassGeometry<H> for P2Geometry<'_> {
    fn curve(&self, o: H) -> Option<&Curve> {
        self.0.curve(o)
    }
    fn surface(&self, o: H) -> Option<&Surface> {
        self.0.surface(o)
    }
    fn volume(&self, o: H) -> Option<&Volume> {
        self.0.volume(o)
    }
    fn point(&self, o: H) -> Option<Vec3> {
        self.0.point(o)
    }
    fn classification_vertex_point(&self, o: H) -> Option<Vec3> {
        self.0.classification_vertex_point(o)
    }
    fn topology_zone_tol_3d(&self, o: H) -> Option<f64> {
        self.0.topology_zone_tol_3d(o)
    }
    fn topology_entity(&self, o: H) -> Option<(&Brep, EntityRef)> {
        self.0.topology_entity(o).or_else(|| self.0.bare.entity(o.0))
    }
    fn topology_handle(&self, e: EntityRef) -> Option<H> {
        self.0.topology_handle(e)
    }
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
    let mut pool = build_all(&k, s)?;
    if let Some(face) = &s.face {
        add_face(&k, &mut pool, face)?;
    }
    let mut ccs = [build_cc(&k, &pool, &s.cc[0])?, build_cc(&k, &pool, &s.cc[1])?];
    let mut o: Vec<f64> = Vec::new();
    let view = ContractPool::with_extra_curves(&s.objects, s.cos.len(), s.face.as_ref());
    for x in &s.ops {
        if let Some(why) = undefined_upstream(&view, s.face.is_some(), &ccs, x) {
            return Err(Fail::Undefined(why));
        }
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
            OpIn::Dead { brep, live, .. } => {
                // an empty brep, as on the C++ side, or the face's
                let empty_brep = Brep::new();
                let target = if *live {
                    Some(&pool.topo.as_ref().ok_or_else(|| harness("no face"))?.brep)
                } else {
                    brep.then_some(&empty_brep)
                };
                let r = must(cc.has_dead_topology_point_object(&pool, target))?;
                o.extend([0.0, b(r)]);
            }
            OpIn::CrvLoc { j, .. } => {
                let (p, prm) = must(cc.classify_crv_loc_by_index(&k, &pool, *j))?;
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
                let geometry = P2Geometry(&pool);
                let r = if matches!(x, OpIn::MinUv { .. }) {
                    cc.interval_min_uv(&k, &geometry, *j, given)
                } else {
                    cc.interval_max_uv(&k, &geometry, *j, given)
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
        Err(Fail::Refused(s)) => Ok(refused(s)),
        Err(Fail::Undefined(why)) => Ok(Output::err(format!("undefined_upstream: {why}"))),
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
        3 => OpIn::Dead { t, brep: rng.chance(0.7), live: false },
        4..=5 => OpIn::CrvLoc { t, j },
        6..=7 => {
            let (lo, hi) = curve_range(objs, c.curve);
            let mut ivl = gen_ivls(rng, objs, 1, lo, hi, 0.1, false);
            let jj = if rng.chance(0.3) { size + rng.below(3) as usize } else { j };
            // (the fallback's gen_pc stays eager: it advances rng)
            let popped = ivl.pop();
            let fallback = IvlIn { a: 0.0, b: 1.0, pcs: [gen_pc(rng, objs, 0.0, false); 3] };
            OpIn::Insert {
                t,
                j: jj,
                ivl: popped.unwrap_or(fallback),
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
    let mut s = Script { objects, face: None, cos, cc, ops };
    if rng.chance(0.3) {
        add_face_topology(rng, &mut s);
    }
    s
}

/// Gives `s` a one-face B-rep (edge loops) and moves its face, edge and vertex point
/// classifications (and some classification faces) onto that B-rep's real topology, more of
/// them than the plain scripts have; `HasDeadTopologyPointObject` also tests against it.
fn add_face_topology(rng: &mut Rng, s: &mut Script) {
    let Some(face) = (0..8)
        .map(|_| gen_face(rng))
        .find(|f| matches!(f.loops, Some(LoopsIn::Edges { .. })))
    else {
        return;
    };
    let first = s.objects.len() + s.cos.len();
    let kinds = topology_kinds(&face);
    let real = |k: usize| -> Vec<usize> {
        kinds.iter().enumerate().filter(|(_, &x)| x == k).map(|(j, _)| first + j).collect()
    };
    let (faces, edges, vertices) = (real(kind::FACE), real(kind::EDGE), real(kind::VERTEX));
    let remap = |rng: &mut Rng, pc: &mut PcIn| {
        // geometry classifications sometimes become topology ones
        if matches!(pc.class, 100 | 106) && rng.chance(0.3) {
            pc.class = *rng.choose(&[102, 103, 104, 104]).unwrap_or(&104);
        }
        let v = match pc.class {
            102 => &faces,
            103 => &edges,
            104 => &vertices,
            _ => return,
        };
        pc.obj = rng.choose(v).copied();
    };
    for c in &mut s.cc {
        if rng.chance(0.5) {
            c.face = faces.first().copied();
        }
        for ivl in &mut c.ivls {
            for pc in &mut ivl.pcs {
                remap(rng, pc);
            }
        }
    }
    for o in &mut s.ops {
        match o {
            OpIn::Insert { ivl, .. } => {
                for pc in &mut ivl.pcs {
                    remap(rng, pc);
                }
            }
            OpIn::UpdatePc { pc, .. } => remap(rng, pc),
            OpIn::Dead { live, .. } => *live = rng.chance(0.6),
            _ => {}
        }
    }
    if rng.chance(0.5) {
        let t = rng.below(2) as usize;
        s.ops.push(OpIn::Dead { t, brep: true, live: rng.chance(0.7) });
    }
    s.face = Some(face);
}

// ---- input contract ---------------------------------------------------------------------

/// Whether a point classification `class obj` is one the shim's `read_pc` takes: a class
/// code it knows and an object of that class's kind (or none).
fn pc_keeps_contract(pool: &ContractPool, pc: &PcIn) -> bool {
    let Ok(want) = kind_of_class(pc.class) else { return false };
    pool.get(pc.obj, &[])
        && pool
            .kind(pc.obj)
            .is_none_or(|k| Some(k) == want || (want == Some(kind::BSURF) && k == kind::PLANE))
}

fn ivl_keeps_contract(pool: &ContractPool, ivl: &IvlIn) -> bool {
    ivl.pcs.iter().all(|pc| pc_keeps_contract(pool, pc))
}

/// The shim's `read_cc`: objects of the kinds a classification takes, at most
/// [`MAX_LOOPS`] target loops and [`MAX_IVLS`] intervals.
fn cc_keeps_contract(pool: &ContractPool, c: &CcIn) -> bool {
    let srf = [kind::PLANE, kind::BSURF];
    pool.get(c.curve, &[kind::CURVE])
        && pool.get(c.s1, &srf)
        && pool.get(c.uv1, &[kind::CURVE])
        && pool.get(c.s2, &srf)
        && pool.get(c.uv2, &[kind::CURVE])
        && pool.get(c.face, &[kind::FACE])
        && pool.get(c.ti, &[kind::OTHER])
        && pool.get(c.other, &[kind::OTHER])
        && c.loops.len() <= MAX_LOOPS
        && c.loops.iter().all(|l| pool.get(Some(*l), &[kind::LOOP]))
        && c.ivls.len() <= MAX_IVLS
        && c.ivls.iter().all(|i| ivl_keeps_contract(pool, i))
}

/// The input checks of `ReSet`: objects of the kinds it takes, and a new curve needs its
/// interval.
fn reset_ok(pool: &ContractPool, r: &ResetIn) -> bool {
    let srf = [kind::PLANE, kind::BSURF];
    pool.get(r.curve, &[kind::CURVE])
        && pool.get(r.uv, &[kind::CURVE])
        && pool.get(r.s1, &srf)
        && pool.get(r.s2, &srf)
        && pool.get(r.uv2, &[kind::CURVE])
        && pool.get(r.ti, &[kind::OTHER])
        && (r.curve.is_none() && r.uv.is_none() || r.ivl.is_some())
}

fn is_point(pc: &PointClassification<H>) -> bool {
    pc.object().is_some_and(|h| h.1 == ClassObjectKind::Point)
}

/// Why op `x` on the current classifications is undefined upstream: the run-time checks of
/// the reference's shim, evaluated on the state the ops before it left (a precondition of
/// `x`, never its result). `None` when it is defined.
fn undefined_upstream(
    pool: &ContractPool,
    has_face: bool,
    ccs: &[CurveClassification<H>; 2],
    x: &OpIn,
) -> Option<&'static str> {
    let (_, t) = x.code_t();
    let cc = ccs.get(t)?;
    let size = cc.size();
    let bad = |b: bool, why: &'static str| b.then_some(why);
    match x {
        OpIn::Assign { .. } => None,
        OpIn::ObjTol { j, .. } | OpIn::CrvLoc { j, .. } => {
            if size == 0 || *j > size {
                return Some("boundary index");
            }
            let (pc, _) = cc.point_class_by_index(*j).ok()?;
            if matches!(x, OpIn::ObjTol { .. }) {
                return bad(is_point(pc), "zone tolerance of a point object");
            }
            bad(!pc.sss_point_flag() && cc.curve().is_none(), "no classification curve")
        }
        OpIn::Dead { live, .. } => bad(*live && !has_face, "no B-rep"),
        OpIn::Insert { j, ivl, .. } => bad(
            !ivl_keeps_contract(pool, ivl) || *j > MAX_IVLS || (*j == 0 && size > 0),
            "insert",
        ),
        OpIn::Reset { r, .. } => bad(!reset_ok(pool, r), "reset"),
        OpIn::Connect { j, end, .. } => bad(*j > MAX_IVLS || *end > 4, "index"),
        OpIn::UpdatePc { j, pc, .. } => {
            if !pc_keeps_contract(pool, pc) || size == 0 || *j > size {
                return Some("boundary index");
            }
            // the mate of the first and last boundary needs the curve
            bad((*j == 0 || *j == size) && cc.curve().is_none(), "no classification curve")
        }
        OpIn::EditMin { j, param, .. } | OpIn::EditMax { j, param, .. } => {
            if *j > MAX_IVLS {
                return Some("index");
            }
            if *j >= size {
                return None;
            }
            // an edit within the allowed range evaluates the curve and takes the end
            // objects' zone tolerances
            let ivl = |i: usize| cc.at(i).ok().map(|v| v.interval);
            let (lo, hi) = if matches!(x, OpIn::EditMin { .. }) {
                let lo = if *j > 0 { ivl(*j - 1)?.min() } else { cc.interval().min() };
                (lo, ivl(*j)?.max())
            } else {
                let hi = if *j + 1 < size { ivl(*j + 1)?.max() } else { cc.interval().max() };
                (ivl(*j)?.min(), hi)
            };
            let ends_ok = (0..size).all(|i| cc.at(i).is_ok_and(|v| !is_point(&v.start) && !is_point(&v.end)));
            bad(
                Extent1d::from_min_max(lo, hi).contains_value(*param, 0.0) && (cc.curve().is_none() || !ends_ok),
                "edit",
            )
        }
        OpIn::MinUv { j, .. } | OpIn::MaxUv { j, .. } => bad(*j >= size || cc.curve().is_none(), "min/max UV"),
        OpIn::Refine { sol, edge, count, .. } => {
            if !pool.get(*edge, &[kind::EDGE]) || sol.ty > 4 || *count > 100 {
                return Some("refine input");
            }
            // past the entry checks the bare stand-in edge's topology is walked
            let point = sol.ty == 1 || are_same(sol.s[0], sol.e[0]) || are_same(sol.s[1], sol.e[1]);
            bad(
                *count <= 5 && point && cc.surface1().is_some() && cc.surface2().is_some(),
                "refinement past its entry checks",
            )
        }
    }
}

/// Whether a script keeps the input contract of the C++ reference wrapper: its face is
/// one upstream builds, its curves on surfaces are UV curves on B-spline surfaces, its
/// classifications are well-formed ([`cc_keeps_contract`]) and every op keeps
/// [`undefined_upstream`].
fn keeps_contract(s: &Script) -> bool {
    let pool = ContractPool::with_extra_curves(&s.objects, s.cos.len(), s.face.as_ref());
    let base = ContractPool::new(&s.objects, None);
    s.face.as_ref().is_none_or(face_keeps_contract)
        && s.cos.len() <= MAX_COS
        && s.cos.iter().all(|[uv, sf]| base.get(Some(*uv), &[kind::CURVE]) && base.get(Some(*sf), &[kind::BSURF]))
        && s.cc.iter().all(|c| cc_keeps_contract(&pool, c))
        && !matches!(rust_run(s), Err(Fail::Undefined(_)))
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
            OpIn::Dead { t: 0, brep: true, live: false },
            OpIn::Dead { t: 0, brep: false, live: false },
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
            json!(Script { objects: objs.clone(), face: None, cos: vec![], cc: [cc0.clone(), cc1.clone()], ops: uv3d }),
        ));
        out.push((
            format!("edge_ops_{name}"),
            json!(Script { objects: objs, face: None, cos: vec![], cc: [cc0, cc1], ops }),
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
        OpIn::Dead { t: 0, brep: true, live: false },
        OpIn::Reset { t: 0, r: ResetIn { curve: None, ivl: None, uv: None, tol: 1e-3, owned: false, current: false, s1: None, s2: None, uv2: None, ti: None } },
        OpIn::Connect { t: 0, j: 0, end: 1, allow: true },
        OpIn::EditMax { t: 0, j: 0, param: 0.5, collapse: true },
        OpIn::Assign { t: 1 },
        OpIn::Insert { t: 0, j: 0, ivl: IvlIn { a: 0.0, b: 1.0, pcs: [unknown(); 3] } },
        OpIn::Insert { t: 1, j: 2, ivl: IvlIn { a: 0.5, b: 1.0, pcs: [unknown(); 3] } },
    ];
    out.push((
        "edge_empty".into(),
        json!(Script { objects: objs, face: None, cos: vec![], cc: [plain_cc(Some(0), 1e-5, vec![]), plain_cc(None, 1e-5, vec![])], ops }),
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
            if keeps_contract(&s) {
                return serde_json::to_value(s).ok();
            }
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

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Most generated scripts are not deferred.
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

    /// A classification face may stay on the base pool's bare face while a real one-face
    /// B-rep is also present. `SmFace::GetSurface` answers from that stand-in's surface, so
    /// `GetMinUV` / `GetMaxUV` answer through the bare fallback instead of deferring on it.
    #[test]
    fn mixed_topology_cases_do_not_defer_on_the_face_surface() {
        let mut mixed = 0;
        for i in 0..MIN_GENERATED {
            let mut rng = Rng::new(CaseContext::seed_for("intersect.curve_class_p2", i));
            let Some(v) = Runner.generate(&mut rng) else { continue };
            let s: Script = serde_json::from_value(v).unwrap();
            let bare_face = s.objects.iter().any(|o| matches!(o, ObjIn::Face(Some(_))));
            let uv_op = s.ops.iter().any(|o| matches!(o, OpIn::MinUv { .. } | OpIn::MaxUv { .. }));
            if !bare_face || s.face.is_none() || !uv_op {
                continue;
            }
            mixed += 1;
            let out = rust(&s).unwrap_or_else(|e| panic!("gen{i}: rust: {e}"));
            assert!(
                out.deferred.as_ref().is_none_or(|d| d.symbol != "SmFace::GetSurface"),
                "gen{i}: {:?}",
                out.deferred
            );
        }
        assert!(mixed >= 17, "{mixed} mixed topology cases");
    }
}

#[cfg(test)]
mod dump {
    use super::*;

    /// Prints how often the comparable generated scripts reach each op and status (a
    /// debugging aid: `cargo test -p goldens --lib p2_coverage -- --ignored
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
                let sub = Script { objects: s.objects.clone(), face: s.face.clone(), cos: s.cos.clone(), cc: s.cc.clone(), ops: s.ops[..n].to_vec() };
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
