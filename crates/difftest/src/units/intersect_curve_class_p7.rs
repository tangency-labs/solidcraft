//! Runner for unit `intersect.curve_class_p7`: the part-7 members of `SmCurveClassification`
//! (boundary point classifications and mates, interval collapse, touches, vertices and edges
//! met, edge and face crossings, near misses, coincident-maybe and partial-coincidence
//! heuristics, face classification of an interval) and `sm_SetDoFilletingGeometry`.
//!
//! C++: `solidcore_ffi::units::intersect_curve_class_p7` (upstream
//! `source/SMLib/src/SmCurveClass.cpp`). Rust: `solidcore::intersect::curve_class`.
//!
//! A case is a script: a pool of `objects` (as in unit `intersect.curve_class_p1`: curves of
//! every kind the code branches on, planes, B-spline surfaces, points, vertex, edge and face
//! stand-ins), two classifications built from data (curve, surface 1, UV curve 1, tangency
//! flag, zone tolerance, fillet data, intervals with their three point classifications), and
//! `ops` on them. Both sides produce the flat record documented in the ffi module, reported
//! as the `out` values. A Rust error maps to its `SmStatus` (`InvalidInput` 1007); an
//! `Unported` error anywhere defers the case.
//!
//! Half the generated scripts add a real one-face B-rep (`face`, as in unit
//! `intersect.curve_class_p4`: `sc_one_face_brep.h` in the shim, `heal_face_props::rust_brep`
//! plus its down face use's side, [`hfp::add_down_side`], in Rust) whose face, loops, edges,
//! vertices and edge uses follow the objects in the pool; their classifications and targets
//! usually name its real topology, so the `SmVertex` / `SmEdge` connection tests, intervals,
//! curves, points and tolerances run the ported topology members through the
//! `ClassGeometry::topology_entity` hook.
//!
//! Without that B-rep, the vertex, edge and face stand-ins are B-rep records in the shim's
//! bare state (see unit `intersect.curve_class_p1`,
//! [`BareTopology`](super::intersect_curve_class_p1::BareTopology)), so the members those
//! records answer (zone tolerances, edge curve / interval, face surface) are compared too.
//!
//! Deferred cases: those members on the bare stand-ins (no B-rep behind them), the edits of
//! `sm_SetDoFilletingGeometry` (`SmVertex::SetPoint`, `SmEdge::SetCurve`), the face queries of
//! `ClassifyFaceInterval` (`topology.face_p4`, `query.face_p1`, `query.face_p2`), in-surface
//! gaps (the surface point solve, `surface.surface_p4`) and the 2D pair tolerance on a surface
//! (`surface.surface_p1`).
//! Generated scripts are filtered by the shim's run-time checks (scripts it rejects as
//! undefined upstream, or as topology walks over the bare stand-ins, are dropped).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use solidcore::curve::curve::p2::Curve;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::tol::zone_tol3d;
use solidcore::intersect::curve_class::{
    self as cc_mod, CurveClassification, CurveInterval, PartialCoincidence,
};
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::point_class::{ClassGeometry, ClassObjectKind, PointClassType};
use solidcore::surface::surface::Surface;
use solidcore::surface::volume::Volume;
use solidcore::topology::Brep;
use solidcore::EntityRef;
use solidcore::kernel::Kernel;
use solidcore_ffi::units::intersect_curve_class_p1::kind;
use solidcore_ffi::units::intersect_curve_class_p7::{self as ffi, op};

use super::intersect_curve_class_p1::{
    add_face, b, build_pool, curve_block, gen_bspline, gen_objects, gen_tol, id, must, obj_kind, of_kind,
    opt_id, put_curve, put_pc, rp, st, surface_block, BSplineIn, ConicIn, CurveIn, Fail, LineIn,
    ObjIn, Pool, H, NOW, SC_ERR_ARG, USE_DEFAULT,
};
use super::intersect_curve_class_p4::{gen_face, topology_kinds};
use crate::units::heal_face_props::{self as hfp, FaceIn};
use solidcore_ffi::units::heal_face_props::FaceSpec;
use crate::case::{CaseContext, CaseSource};
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1500;

/// `intersect.curve_class_p7` runner.
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
struct CcIn {
    curve: Option<usize>,
    s1: Option<usize>,
    uv1: Option<usize>,
    #[serde(default)]
    ignore: bool,
    tol: f64,
    /// `SetFilletData(&other, fillet)` when given.
    #[serde(default)]
    fillet: Option<bool>,
    ivls: Vec<IvlIn>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    PcByIndex { t: usize, j: usize },
    Mate { t: usize, j: usize },
    Collapse { t: usize, j: usize },
    Touches { t: usize, targets: Vec<usize> },
    FindVertex { t: usize, target: Option<usize> },
    FindEdge { t: usize, target: Option<usize>, flag: u64 },
    CrossedByEdge { t: usize },
    CrossingFace { t: usize, face: Option<usize> },
    NearMisses { t: usize, b: bool },
    CoinMaybe { t: usize, j: usize, own: bool, report: bool },
    ClassifyFace { t: usize, j: usize, do_point: bool, face: Option<usize>, old: Option<usize>, uv_ok: bool },
    Partial { t: usize, own: bool },
    Fillet { t: usize, j: usize, todo: u64 },
}

impl OpIn {
    fn code_t(&self) -> (usize, usize) {
        match *self {
            OpIn::PcByIndex { t, .. } => (op::PC_BY_INDEX, t),
            OpIn::Mate { t, .. } => (op::MATE, t),
            OpIn::Collapse { t, .. } => (op::COLLAPSE, t),
            OpIn::Touches { t, .. } => (op::TOUCHES, t),
            OpIn::FindVertex { t, .. } => (op::FIND_VERTEX, t),
            OpIn::FindEdge { t, .. } => (op::FIND_EDGE, t),
            OpIn::CrossedByEdge { t } => (op::CROSSED_BY_EDGE, t),
            OpIn::CrossingFace { t, .. } => (op::CROSSING_FACE, t),
            OpIn::NearMisses { t, .. } => (op::NEAR_MISSES, t),
            OpIn::CoinMaybe { t, .. } => (op::COIN_MAYBE, t),
            OpIn::ClassifyFace { t, .. } => (op::CLASSIFY_FACE, t),
            OpIn::Partial { t, .. } => (op::PARTIAL, t),
            OpIn::Fillet { t, .. } => (op::FILLET, t),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Script {
    objects: Vec<ObjIn>,
    /// A real one-face B-rep (as in unit `intersect.curve_class_p4`) whose topology follows
    /// the objects in the pool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    face: Option<FaceIn>,
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

fn cpp_input(s: &Script) -> Vec<f64> {
    let mut v = Vec::new();
    objects_block(&mut v, &s.objects);
    for c in &s.cc {
        v.extend([opt_id(c.curve), opt_id(c.s1), opt_id(c.uv1), b(c.ignore), c.tol]);
        v.push(c.fillet.map_or(-1.0, b));
        v.push(c.ivls.len() as f64);
        for ivl in &c.ivls {
            v.extend([ivl.a, ivl.b]);
            for pc in &ivl.pcs {
                v.extend([f64::from(pc.class), opt_id(pc.obj), pc.t, pc.gap]);
            }
        }
    }
    v.push(s.ops.len() as f64);
    for o in &s.ops {
        let (code, t) = o.code_t();
        v.extend([code as f64, t as f64]);
        match o {
            OpIn::PcByIndex { j, .. } | OpIn::Mate { j, .. } | OpIn::Collapse { j, .. } => {
                v.push(*j as f64);
            }
            OpIn::Touches { targets, .. } => {
                v.push(targets.len() as f64);
                v.extend(targets.iter().map(|&x| x as f64));
            }
            OpIn::FindVertex { target, .. } => v.push(opt_id(*target)),
            OpIn::FindEdge { target, flag, .. } => v.extend([opt_id(*target), *flag as f64]),
            OpIn::CrossedByEdge { .. } => {}
            OpIn::CrossingFace { face, .. } => v.push(opt_id(*face)),
            OpIn::NearMisses { b: x, .. } => v.push(b(*x)),
            OpIn::CoinMaybe { j, own, report, .. } => v.extend([*j as f64, b(*own), b(*report)]),
            OpIn::ClassifyFace {
                j, do_point, face, old, uv_ok, ..
            } => v.extend([*j as f64, b(*do_point), opt_id(*face), opt_id(*old), b(*uv_ok)]),
            OpIn::Partial { own, .. } => v.push(b(*own)),
            OpIn::Fillet { j, todo, .. } => v.extend([*j as f64, *todo as f64]),
        }
    }
    v
}

fn ffi_face(s: &Script) -> Option<FaceSpec> {
    s.face.as_ref().map(hfp::ffi_face)
}

fn cpp(s: &Script) -> Result<Output, HarnessError> {
    let out = ffi::run(ffi_face(s).as_ref(), &cpp_input(s)).map_err(|e| harness(format!("c++: {e}")))?;
    Ok(Output::new().values("out", out))
}

/// Whether the shim accepts the script (it rejects what is undefined upstream).
fn shim_accepts(s: &Script) -> bool {
    match ffi::run(ffi_face(s).as_ref(), &cpp_input(s)) {
        Ok(_) => true,
        Err(solidcore_ffi::FfiError::Status(c)) if c == SC_ERR_ARG => false,
        Err(_) => false,
    }
}

// ---- Rust side --------------------------------------------------------------------------

/// The pool as seen by the classification: [`Pool`], plus the bare stand-ins of a script
/// that also has a real B-rep. [`Pool`] resolves only the B-rep's handles then; the vertex,
/// edge and face objects of the script stay in the shim's bare state there as well (the
/// shim builds them the same way with or without the face), so they resolve to their
/// [`BareTopology`](super::intersect_curve_class_p1::BareTopology) records, a B-rep of their
/// own (connected to nothing in the face's).
struct Geo<'a>(&'a Pool);

impl Geo<'_> {
    /// The bare record of a script object when the pool also has a real B-rep.
    fn bare_entity(&self, o: H) -> Option<(&Brep, EntityRef)> {
        self.0.topo.as_ref()?;
        self.0.bare.entity(o.0)
    }
}

impl ClassGeometry<H> for Geo<'_> {
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
        self.0.classification_vertex_point(o).or_else(|| {
            self.0.topo.as_ref()?;
            self.0.bare.vertex_point(o.0)
        })
    }
    fn topology_zone_tol_3d(&self, o: H) -> Option<f64> {
        self.0.topology_zone_tol_3d(o).or_else(|| {
            self.0.topo.as_ref()?;
            self.0.bare.zone_tol_3d(o.0)
        })
    }
    fn topology_entity(&self, o: H) -> Option<(&Brep, EntityRef)> {
        self.0.topology_entity(o).or_else(|| self.bare_entity(o))
    }
    fn topology_handle(&self, e: EntityRef) -> Option<H> {
        self.0.topology_handle(e)
    }
    fn topology_handle_in(&self, brep: &Brep, e: EntityRef) -> Option<H> {
        if self.0.topo.is_some() && core::ptr::eq(brep, &raw const self.0.bare.brep) {
            return self.0.handles.get(self.0.bare.index_of(e)?).copied();
        }
        self.0.topology_handle(e)
    }
}

/// The handle the fillet data of a classification names (its partner).
const OTHER_CC: H = H(usize::MAX, ClassObjectKind::Other);

fn build_cc(k: &Kernel, pool: &Pool, c: &CcIn) -> Result<CurveClassification<H>, Fail> {
    let mut cc = CurveClassification::new(k);
    cc.set_curve(pool.handle(c.curve)?, false);
    cc.set_surface1(pool.handle(c.s1)?);
    cc.set_uv_curve1(pool.handle(c.uv1)?);
    cc.set_ignore_tangency(c.ignore);
    cc.set_src_zone_tol_3d(zone_tol3d(k, c.tol));
    if let Some(f) = c.fillet {
        cc.set_fillet_data(Some(OTHER_CC), f);
    }
    must(cc.set_size(k, c.ivls.len()))?;
    for (i, ivl) in c.ivls.iter().enumerate() {
        let dst = must(cc.at_mut(i))?;
        dst.set_interval(&Extent1d::from_min_max(ivl.a, ivl.b));
        for (w, pc) in ivl.pcs.iter().enumerate() {
            let class = PointClassType::from_code(pc.class).ok_or_else(|| harness("bad class"))?;
            let h = pool.handle(pc.obj)?;
            let p = match w {
                0 => &mut dst.start,
                1 => &mut dst.mid,
                _ => &mut dst.end,
            };
            p.set_class_object(class, h);
            p.set_t_param(pc.t, false);
            p.set_gap_3d(pc.gap);
        }
    }
    Ok(cc)
}

fn put_ivl(o: &mut Vec<f64>, ivl: &CurveInterval<H>) {
    o.extend([ivl.min(), ivl.max()]);
    put_pc(o, &ivl.start);
    put_pc(o, &ivl.mid);
    put_pc(o, &ivl.end);
}

fn put_state(o: &mut Vec<f64>, cc: &CurveClassification<H>) {
    o.push(cc.size() as f64);
    for ivl in cc.intervals() {
        put_ivl(o, ivl);
    }
}

#[allow(clippy::too_many_lines)]
fn rust_run(s: &Script) -> Result<Vec<f64>, Fail> {
    let k = Kernel::default();
    let mut pool = build_pool(&s.objects)?;
    if let Some(face) = &s.face {
        add_face(&k, &mut pool, face)?;
    }
    let mut ccs = [build_cc(&k, &pool, &s.cc[0])?, build_cc(&k, &pool, &s.cc[1])?];
    let geo = Geo(&pool);
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
            OpIn::PcByIndex { j, .. } => {
                let (pc, prm) = must(cc.point_class_by_index(*j))?;
                o.extend([0.0, prm]);
                put_pc(&mut o, pc);
            }
            OpIn::Mate { j, .. } => {
                let mate = must(cc.point_class_mate_by_index(&k, &geo, *j))?;
                let at = mate.map_or(-1.0, |m| {
                    cc.intervals()
                        .iter()
                        .position(|ivl| std::ptr::eq(&raw const ivl.end, m))
                        .map_or(-1.0, |p| p as f64)
                });
                o.extend([0.0, at]);
            }
            OpIn::Collapse { j, .. } => {
                let r = cc.collapse_interval(&k, &geo, *j);
                o.push(st(r)?.err().unwrap_or(0.0));
            }
            OpIn::Touches { targets, .. } => {
                let hs = targets
                    .iter()
                    .map(|&i| pool.handle(Some(i)).map(|h| h.ok_or_else(|| harness("no target"))))
                    .collect::<Result<Result<Vec<H>, _>, _>>()??;
                let touches = must(cc.point_classifies_connected_to(&geo, &hs))?;
                o.extend([0.0, touches.len() as f64]);
                for tch in &touches {
                    o.extend([
                        id(tch.connected_to),
                        f64::from(tch.loop_dir.code()),
                        tch.loop_index as f64,
                        tch.param,
                        tch.pt_classify_index as f64,
                        // the touch's classification: the first start, else the end of
                        // the interval before its boundary
                        if tch.pt_classify_index == 0 {
                            0.0
                        } else {
                            3.0 * (tch.pt_classify_index - 1) as f64 + 2.0
                        },
                    ]);
                }
            }
            OpIn::FindVertex { target, .. } => {
                let r = must(cc.find_vertex_objects(&geo, pool.handle(*target)?))?;
                o.extend([0.0, r.vertices.len() as f64]);
                o.extend(r.vertices.iter().map(|h| h.0 as f64));
                o.push(r.params.len() as f64);
                o.extend(r.params.iter().copied());
            }
            OpIn::FindEdge { target, flag, .. } => {
                let r = must(cc.find_edge_objects(&geo, pool.handle(*target)?, *flag))?;
                o.extend([0.0, r.len() as f64]);
                o.extend(r.iter().map(|h| h.0 as f64));
            }
            OpIn::CrossedByEdge { .. } => {
                let r = must(cc.is_crossed_by_edge(&geo))?;
                o.extend([0.0, b(r)]);
            }
            OpIn::CrossingFace { face, .. } => {
                let r = cc.is_crossing_face(pool.handle(*face)?);
                o.extend([0.0, b(r)]);
            }
            OpIn::NearMisses { b: x, .. } => {
                let r = must(cc.has_near_misses(&k, &geo, *x))?;
                o.extend([0.0, b(r)]);
            }
            OpIn::CoinMaybe { j, own, report, .. } => {
                let partner = if *own { cc.clone() } else { other.clone() };
                let (verdict, pos) = must(cc.are_coincident_maybe(&k, &geo, &partner, *j, *report))?;
                o.extend([0.0, f64::from(verdict.code()), f64::from(pos.code())]);
            }
            OpIn::ClassifyFace {
                j, do_point, face, old, uv_ok, ..
            } => {
                let f = pool.handle(*face)?.ok_or_else(|| harness("no face"))?;
                let r = cc.classify_face_interval(&k, &geo, *j, *do_point, f, pool.handle(*old)?, *uv_ok, None);
                o.push(st(r)?.err().unwrap_or(0.0));
            }
            OpIn::Partial { own, .. } => {
                let partner = if *own { cc.clone() } else { other.clone() };
                match st(cc.test_for_partial_coincident_edges(&k, &geo, &partner))? {
                    Ok(PartialCoincidence {
                        max_partial_coin_gap,
                        new_curve_to_test,
                    }) => {
                        o.extend([0.0, max_partial_coin_gap, b(new_curve_to_test.is_some())]);
                        if let Some(c) = new_curve_to_test {
                            put_curve(&k, &mut o, &Curve::from(c));
                        }
                    }
                    Err(code) => o.extend([code, 0.0, 0.0]),
                }
            }
            OpIn::Fillet { j, todo, .. } => {
                let r = cc_mod::set_do_filleting_geometry(&k, &geo, *j, cc, Some(&other), *todo);
                o.push(st(r)?.err().unwrap_or(0.0));
            }
        }
        put_state(&mut o, cc);
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

/// The kind of every pool object: the script objects, then the face's topology.
fn pool_kinds(objects: &[ObjIn], face: Option<&FaceIn>) -> Vec<usize> {
    let mut v: Vec<usize> = objects.iter().map(obj_kind).collect();
    if let Some(f) = face {
        v.extend(topology_kinds(f));
    }
    v
}

/// Pool indices of the objects of kinds `ks`; with a face, usually only its real
/// topology of those kinds when it has any.
fn pick_kinds(rng: &mut Rng, kinds: &[usize], n_obj: usize, ks: &[usize]) -> Vec<usize> {
    let all: Vec<usize> = kinds
        .iter()
        .enumerate()
        .filter(|(_, k)| ks.contains(k))
        .map(|(i, _)| i)
        .collect();
    let real: Vec<usize> = all.iter().copied().filter(|&i| i >= n_obj).collect();
    if !real.is_empty() && rng.chance(0.85) {
        real
    } else {
        all
    }
}

/// Pool indices of every object of the kinds `ks`.
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

/// A closed (periodic-looking) clamped cubic over `[0, 1]`.
fn closed_bspline(rng: &mut Rng, scale: f64) -> BSplineIn {
    let c = rp(rng, scale);
    let r = scale * rng.uniform(0.2, 1.0);
    let pts = [[1.0, 0.0], [1.0, 1.0], [-1.0, 1.0], [-1.0, -1.0], [1.0, -1.0], [1.0, 0.0]];
    BSplineIn {
        dim: 3,
        oob: false,
        cpts: pts
            .iter()
            .map(|q| [c[0] + r * q[0], c[1] + r * q[1], c[2], NOW])
            .collect(),
        p: 3,
        u: vec![0.0, 0.0, 0.0, 0.0, 0.4, 0.7, 1.0, 1.0, 1.0, 1.0],
    }
}

/// The pool: the base pool of unit `intersect.curve_class_p1` plus closed curves, a 2D
/// curve, more vertices and edges (on B-spline and other curves) and faces.
fn gen_objects_p7(rng: &mut Rng) -> Vec<ObjIn> {
    let mut objs = gen_objects(rng);
    let scale = *rng.choose(&[1.0, 1.0, 1e-4, 1e4]).unwrap_or(&1.0);
    objs.push(ObjIn::Curve(CurveIn::Bspline(closed_bspline(rng, scale))));
    objs.push(ObjIn::Curve(CurveIn::Conic(ConicIn {
        center: rp(rng, 5.0),
        x: [1.0, 0.0, 0.0],
        y: [0.0, 1.0, 0.0],
        domain: [0.0, 360.0],
        rx: rng.uniform(0.5, 5.0),
        ry: None,
        inside_out: false,
    })));
    let mut two_d = gen_bspline(rng);
    two_d.dim = 2;
    for c in &mut two_d.cpts {
        c[2] = f64::MAX;
    }
    objs.push(ObjIn::Curve(CurveIn::Bspline(two_d)));
    objs.push(ObjIn::Curve(CurveIn::Line(LineIn {
        p: [0.0; 3],
        v: [scale, 0.0, 0.0],
        domain: [0.0, 1.0],
        scale: 1.0,
        inside_out: false,
    })));
    let curves = of_kind(&objs, kind::CURVE);
    let bsplines: Vec<usize> = objs
        .iter()
        .enumerate()
        .filter(|(_, o)| matches!(o, ObjIn::Curve(CurveIn::Bspline(_))))
        .map(|(i, _)| i)
        .collect();
    objs.push(ObjIn::Vertex);
    for _ in 0..2 {
        let c = if rng.chance(0.7) {
            rng.choose(&bsplines).copied()
        } else {
            rng.choose(&curves).copied()
        };
        objs.push(ObjIn::Edge(c));
    }
    let bsurf = of_kind(&objs, kind::BSURF).first().copied();
    objs.push(ObjIn::Face(bsurf));
    objs
}

fn gen_pc(rng: &mut Rng, kinds: &[usize], n_obj: usize, topo: f64) -> PcIn {
    let choices: &[(i32, &[usize])] = &[
        (100, &[]),
        (102, &[kind::FACE]),
        (103, &[kind::EDGE]),
        (104, &[kind::VERTEX]),
        (105, &[kind::POINT]),
        (106, &[kind::CURVE]),
        (107, &[kind::PLANE, kind::BSURF]),
        (110, &[]),
    ];
    let pick = if topo < 0.0 {
        // geometry only (what CollapseInterval can take the zone tolerances of)
        rng.choose(&[(100, &[][..]), (106, &[kind::CURVE][..]), (107, &[kind::PLANE, kind::BSURF][..]), (110, &[][..])])
            .copied()
    } else if rng.chance(topo) {
        rng.choose(choices.get(1..4).unwrap_or(&[])).copied()
    } else {
        rng.choose(choices).copied()
    };
    let (class, ks) = pick.unwrap_or((100, &[]));
    let obj = if ks.is_empty() {
        None
    } else {
        let v = pick_kinds(rng, kinds, n_obj, ks);
        maybe_of(rng, &v, 0.05)
    };
    let gap = match rng.below(7) {
        0 => 0.0,
        1 => 1e-13,
        2 => 1e-9,
        3 => 1e-6,
        4 => 1e-3,
        5 => rng.uniform(0.0, 1e-2),
        _ => rng.uniform(0.0, 1e-8),
    };
    PcIn {
        class,
        obj,
        t: rng.uniform(-1.0, 2.0),
        gap,
    }
}

fn gen_cc(rng: &mut Rng, objs: &[ObjIn], kinds: &[usize]) -> CcIn {
    let curves = of_kind(objs, kind::CURVE);
    let surfs = of_kinds(objs, &[kind::PLANE, kind::BSURF]);
    let n = rng.below(6) as usize;
    // parameters: ascending over [0, 1], with repeats (degenerate intervals)
    let mut ps: Vec<f64> = (0..=n)
        .map(|_| match rng.below(5) {
            0 => 0.5,
            1 => 0.0,
            _ => rng.unit(),
        })
        .collect();
    ps.sort_by(f64::total_cmp);
    // nearly coincident boundaries: degenerate at some tolerances only
    if n >= 2 && rng.chance(0.3) {
        let i = 1 + rng.below(n as u64 - 1) as usize;
        let d = *rng.choose(&[1e-13, 1e-9, 1e-7, 1e-5, 1e-3]).unwrap_or(&1e-9);
        let base = ps.get(i).copied().unwrap_or(0.5);
        if let Some(x) = ps.get_mut(i + 1) {
            *x = base + d;
        }
        ps.sort_by(f64::total_cmp);
    }
    if rng.chance(0.3) {
        if let Some(f) = ps.first_mut() {
            *f = 0.0;
        }
        if let Some(l) = ps.last_mut() {
            *l = 1.0;
        }
    }
    let topo = *rng.choose(&[-1.0, 0.0, 0.2, 0.6]).unwrap_or(&0.2);
    let mut ivls: Vec<IvlIn> = Vec::new();
    for w in ps.windows(2) {
        let (Some(&a), Some(&bb)) = (w.first(), w.get(1)) else {
            continue;
        };
        let n_obj = objs.len();
        let mut pcs = [
            gen_pc(rng, kinds, n_obj, topo),
            gen_pc(rng, kinds, n_obj, topo),
            gen_pc(rng, kinds, n_obj, topo),
        ];
        // shared boundaries usually classify alike
        if let Some(prev) = ivls.last() {
            if rng.chance(0.8) {
                let [_, _, end] = prev.pcs;
                pcs = [end, pcs[1], pcs[2]];
            }
        }
        ivls.push(IvlIn { a, b: bb, pcs });
    }
    CcIn {
        curve: maybe_of(rng, &curves, 0.05),
        s1: maybe_of(rng, &surfs, 0.6),
        uv1: maybe_of(rng, &curves, 0.7),
        ignore: rng.chance(0.3),
        tol: gen_tol(rng),
        fillet: if rng.chance(0.4) { Some(rng.chance(0.8)) } else { None },
        ivls,
    }
}

fn gen_op(rng: &mut Rng, kinds: &[usize], n_obj: usize, ccs: &[CcIn; 2]) -> OpIn {
    let t = rng.below(2) as usize;
    let ivls: &[IvlIn] = ccs.get(t).map_or(&[], |c| c.ivls.as_slice());
    let size = ivls.len();
    let j = rng.below(size as u64 + 1) as usize;
    let vertices = pick_kinds(rng, kinds, n_obj, &[kind::VERTEX]);
    let edges = pick_kinds(rng, kinds, n_obj, &[kind::EDGE]);
    let faces = pick_kinds(rng, kinds, n_obj, &[kind::FACE]);
    match rng.below(26) {
        0..=1 => OpIn::PcByIndex { t, j },
        2..=3 => OpIn::Mate { t, j },
        4..=6 => {
            // usually a degenerate (or tiny) interval
            let short: Vec<usize> = ivls
                .iter()
                .enumerate()
                .filter(|(_, v)| v.b - v.a < 1e-3)
                .map(|(i, _)| i)
                .collect();
            let j = match rng.choose(&short) {
                Some(&i) if rng.chance(0.8) => i,
                _ => rng.below(size as u64 + 1) as usize,
            };
            OpIn::Collapse { t, j }
        }
        7..=8 => {
            let n = if rng.chance(0.6) { 0 } else { 1 + rng.below(2) as usize };
            let pool: Vec<usize> = if rng.chance(0.5) { vertices } else { edges };
            let targets = (0..n).filter_map(|_| rng.choose(&pool).copied()).collect();
            OpIn::Touches { t, targets }
        }
        9..=10 => OpIn::FindVertex { t, target: maybe_of(rng, &vertices, 0.7) },
        11..=12 => OpIn::FindEdge { t, target: maybe_of(rng, &edges, 0.7), flag: rng.below(4) },
        13 => OpIn::CrossedByEdge { t },
        14 => OpIn::CrossingFace { t, face: maybe_of(rng, &faces, 0.4) },
        15..=16 => OpIn::NearMisses { t, b: rng.chance(0.5) },
        17..=19 => OpIn::CoinMaybe { t, j, own: rng.chance(0.4), report: rng.chance(0.2) },
        20..=21 => OpIn::ClassifyFace {
            t,
            j,
            do_point: rng.chance(0.2),
            face: rng.choose(&faces).copied(),
            old: maybe_of(rng, &faces, 0.5),
            uv_ok: rng.chance(0.7),
        },
        22..=23 => OpIn::Partial { t, own: rng.chance(0.4) },
        _ => OpIn::Fillet { t, j, todo: rng.below(8) },
    }
}

fn gen_script(rng: &mut Rng) -> Script {
    let objects = gen_objects_p7(rng);
    let face = if rng.chance(0.5) { Some(gen_face(rng)) } else { None };
    let kinds = pool_kinds(&objects, face.as_ref());
    let cc = [gen_cc(rng, &objects, &kinds), gen_cc(rng, &objects, &kinds)];
    let n_ops = 1 + rng.below(5) as usize;
    let ops = (0..n_ops)
        .map(|_| gen_op(rng, &kinds, objects.len(), &cc))
        .collect();
    Script { objects, face, cc, ops }
}

// ---- fixtures built in code ---------------------------------------------------------------

fn pc(class: i32, obj: Option<usize>, t: f64, gap: f64) -> PcIn {
    PcIn { class, obj, t, gap }
}

fn unknown() -> PcIn {
    pc(100, None, 0.0, 0.0)
}

/// Edge cases: empty, huge- and tiny-scale classifications, boundary indices, degenerate
/// intervals at tolerance limits, every early return of the heuristics.
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
            ObjIn::Point([scale, 0.0, 0.0]),
            ObjIn::Face(Some(1)),
            ObjIn::Curve(CurveIn::Bspline(BSplineIn {
                dim: 3,
                oob: false,
                cpts: vec![
                    [scale, 0.0, 0.0, NOW],
                    [scale, scale, 0.0, NOW],
                    [-scale, scale, 0.0, NOW],
                    [-scale, -scale, 0.0, NOW],
                    [scale, -scale, 0.0, NOW],
                    [scale, 0.0, 0.0, NOW],
                ],
                p: 3,
                u: vec![0.0, 0.0, 0.0, 0.0, 0.4, 0.7, 1.0, 1.0, 1.0, 1.0],
            })),
        ];
        // a 1e-12-long interval is degenerate at any tolerance; 1e-6 only at the large ones
        let ivls = vec![
            IvlIn { a: 0.0, b: 0.25, pcs: [pc(106, Some(4), 0.0, 1e-9), pc(102, Some(3), 0.0, 0.0), pc(106, Some(0), 0.1, 0.0)] },
            IvlIn { a: 0.25, b: 0.25 + 1e-12, pcs: [pc(106, Some(0), 0.1, 0.0), unknown(), pc(107, Some(1), 0.2, 1e-7)] },
            IvlIn { a: 0.25 + 1e-12, b: 0.5, pcs: [pc(107, Some(1), 0.2, 1e-7), unknown(), unknown()] },
            IvlIn { a: 0.5, b: 0.5 + 1e-6, pcs: [unknown(), pc(102, Some(3), 0.0, 0.0), unknown()] },
            IvlIn { a: 0.5 + 1e-6, b: 1.0, pcs: [unknown(), pc(110, None, 0.0, 0.0), pc(105, Some(2), 1.0, 2e-3)] },
        ];
        let cc0 = CcIn { curve: Some(0), s1: None, uv1: None, ignore: false, tol: 1e-5 * scale, fillet: Some(true), ivls };
        let cc1 = CcIn {
            curve: Some(4),
            s1: Some(1),
            uv1: None,
            ignore: true,
            tol: USE_DEFAULT,
            fillet: Some(false),
            ivls: vec![IvlIn { a: 0.0, b: 1.0, pcs: [unknown(), unknown(), unknown()] }],
        };
        let ops = vec![
            OpIn::PcByIndex { t: 0, j: 0 },
            OpIn::PcByIndex { t: 0, j: 3 },
            OpIn::PcByIndex { t: 0, j: 5 },
            OpIn::Mate { t: 0, j: 0 },
            OpIn::Mate { t: 0, j: 2 },
            OpIn::Mate { t: 0, j: 5 },
            OpIn::Mate { t: 1, j: 0 },
            OpIn::Mate { t: 1, j: 1 },
            OpIn::Touches { t: 0, targets: vec![] },
            OpIn::FindVertex { t: 0, target: None },
            OpIn::FindEdge { t: 0, target: None, flag: 3 },
            OpIn::CrossedByEdge { t: 0 },
            OpIn::CrossingFace { t: 0, face: None },
            OpIn::CrossingFace { t: 0, face: Some(3) },
            OpIn::NearMisses { t: 0, b: false },
            OpIn::NearMisses { t: 1, b: true },
            OpIn::CoinMaybe { t: 0, j: 0, own: false, report: true },
            OpIn::ClassifyFace { t: 0, j: 0, do_point: true, face: Some(3), old: None, uv_ok: true },
            OpIn::ClassifyFace { t: 0, j: 2, do_point: false, face: Some(3), old: None, uv_ok: true },
            OpIn::ClassifyFace { t: 0, j: 3, do_point: false, face: Some(3), old: Some(3), uv_ok: false },
            OpIn::Partial { t: 0, own: true },
            OpIn::Partial { t: 1, own: true },
            OpIn::Fillet { t: 0, j: 0, todo: 7 },
            OpIn::Fillet { t: 1, j: 0, todo: 7 },
            OpIn::Collapse { t: 0, j: 0 },
            OpIn::Collapse { t: 0, j: 1 },
            OpIn::Collapse { t: 0, j: 2 },
            OpIn::Collapse { t: 0, j: 9 },
            OpIn::Collapse { t: 1, j: 0 },
            OpIn::PcByIndex { t: 0, j: 3 },
        ];
        out.push((format!("edge_heuristics_{name}"), json!(Script { objects: objs, face: None, cc: [cc0, cc1], ops })));
    }
    // empty classifications
    let objs = vec![ObjIn::Curve(CurveIn::Line(LineIn {
        p: [0.0; 3],
        v: [1.0, 0.0, 0.0],
        domain: [0.0, 1.0],
        scale: 1.0,
        inside_out: false,
    }))];
    let empty = || CcIn { curve: Some(0), s1: None, uv1: None, ignore: false, tol: 1e-5, fillet: None, ivls: vec![] };
    let ops = vec![
        OpIn::Mate { t: 0, j: 0 },
        OpIn::Collapse { t: 0, j: 0 },
        OpIn::CrossedByEdge { t: 0 },
        OpIn::CrossingFace { t: 0, face: None },
        OpIn::NearMisses { t: 0, b: true },
        OpIn::Partial { t: 0, own: false },
        OpIn::Fillet { t: 0, j: 3, todo: 1 },
    ];
    out.push(("edge_empty".into(), json!(Script { objects: objs, face: None, cc: [empty(), empty()], ops })));
    out
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "intersect.curve_class_p7"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let s = gen_script(rng);
            if ffi::validate(ffi_face(&s).as_ref(), &cpp_input(&s)).is_err() || !shim_accepts(&s) {
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

    /// Generated scripts are accepted by the ffi checks, and a good share of them is
    /// comparable now (not deferred).
    #[test]
    fn generated_scripts_are_partly_comparable() {
        let mut rng = Rng::new(7);
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
        assert!(deferred * 2 < total, "{deferred} of {total} deferred");
    }

    /// The usage fixtures reach the branches they are named after.
    #[test]
    fn usage_fixtures_reach_their_branches() {
        let v: Value =
            serde_json::from_str(include_str!("../../../../fixtures/intersect.curve_class_p7/usage.json")).unwrap();
        let script = |name: &str| -> Script {
            let c = v["cases"].as_array().unwrap().iter().find(|c| c["name"] == name).unwrap();
            serde_json::from_value(c["inputs"].clone()).unwrap()
        };
        // boundary 0 of the closed curve mates the end of the last interval
        let out = rust_run(&script("seam_mates_closed_curve")).ok().unwrap();
        assert_eq!(&out[..3], &[1.0, 0.0, 2.0]);
        // a zero-length interval collapses, a long one does not
        let out = rust_run(&script("collapse_snapped_interval_zero")).ok().unwrap();
        assert_eq!(&out[..3], &[2.0, 0.0, 2.0]);
        let out = rust_run(&script("collapse_snapped_interval_long")).ok().unwrap();
        assert_eq!(&out[..3], &[2.0, 1001.0, 3.0]);
    }

    #[test]
    fn edge_cases_are_accepted() {
        for (name, v) in edge_cases() {
            let s: Script = serde_json::from_value(v).unwrap();
            assert!(ffi::validate(ffi_face(&s).as_ref(), &cpp_input(&s)).is_ok(), "{name}");
            assert!(shim_accepts(&s), "{name}");
        }
    }
}

#[cfg(test)]
mod dump {
    use super::*;

    /// Prints how often the comparable generated scripts reach each op outcome (a debugging
    /// aid: `cargo test -p difftest --features cpp --lib p7_coverage -- --ignored --nocapture`).
    #[test]
    #[ignore = "debugging aid"]
    fn p7_coverage() {
        let mut stats: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
        for i in 0..MIN_GENERATED {
            let mut rng = Rng::new(CaseContext::seed_for("intersect.curve_class_p7", i));
            let Some(v) = Runner.generate(&mut rng) else { continue };
            let s: Script = serde_json::from_value(v).unwrap();
            let Ok(out) = rust_run(&s) else {
                *stats.entry("deferred".into()).or_default() += 1;
                continue;
            };
            // walk the record: op status first-output
            let mut at = 0;
            for x in &s.ops {
                let code = out[at] as usize;
                let status = out[at + 1];
                let first = out.get(at + 2).copied().unwrap_or(-9.0);
                let key = match x {
                    OpIn::Collapse { .. } | OpIn::ClassifyFace { .. } | OpIn::Fillet { .. } => {
                        format!("{code:2} status={status}")
                    }
                    OpIn::Partial { .. } => format!("{code:2} status={status} gap>0={}", first > 0.0),
                    OpIn::Mate { .. } => format!("{code:2} mate={}", first >= 0.0),
                    OpIn::PcByIndex { .. } => format!("{code:2}"),
                    OpIn::CoinMaybe { .. } => format!("{code:2} verdict={first} pos={}", out[at + 3]),
                    _ => format!("{code:2} first={first}"),
                };
                *stats.entry(key).or_default() += 1;
                // skip to the next op: re-run up to this op to find the length
                let n = s.ops.iter().position(|y| std::ptr::eq(y, x)).unwrap() + 1;
                let sub = Script { objects: s.objects.clone(), face: s.face.clone(), cc: s.cc.clone(), ops: s.ops[..n].to_vec() };
                at = rust_run(&sub).map_or(out.len(), |v| v.len());
            }
        }
        for (k, v) in stats {
            println!("{v:6} {k}");
        }
    }
}
