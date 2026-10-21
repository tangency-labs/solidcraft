//! Runner for unit `topology.face_p3`. C++: `solidcore_ffi::units::topology_face_p3`.
//! Rust: `solidcore::topology::face::p3`.
//!
//! The C++ side builds a real `SmBrep` per call from the B-rep blocks of the
//! `topology.vertexuse` runner (an upstream `.smb` test file, optionally with rebuilt UV
//! trim curves; a wire edge; a face from a B-spline surface or an `SmPlane`; the wire
//! network of `prog_test` `my_test_topology`). The Rust side imports the graph that
//! runner's shim exports (raw members, in traversal order; planes as planes, lines as
//! lines) with [`shared::import`] and runs the ported functions on it; results are
//! compared as flags, statuses, tolerances, edge use / vertex use states and sampled
//! curve points (see the FFI module docs for each op's output).
//!
//! **Inexact geometry.** The import holds an analytic `SmBSplineSurface` subclass other
//! than `SmPlane` (cylinder, cone, sphere, torus, surface of revolution or extrusion) and
//! an `SmBSplineCurve` subclass other than `SmLine` (ellipse, ...) only as its B-spline
//! part. A geometric op (`LoopOrient` and later) on a B-rep holding such a kind is
//! deferred to the unit that gives the kind its Rust representation: the `SurfaceKind`
//! dispatch of `surface.surface_p2` for `SmCone` / `SmCylinder` / `SmSphere` / `SmTorus`
//! (their own units are ported, but no `Surface` can hold them yet),
//! `surface.surf_of_extrusion_p2` for the extrusion, `topology.brep_p7`
//! (`SmBrep::ReadFromFile`) otherwise; the topological ops (`Lamina`, `SetTol`, `Notify`)
//! are compared on every B-rep.
//!
//! Face, loop and other-face selectors are taken modulo the B-rep's counts, so
//! generated cases need no counts; a B-rep without faces gives `no face` on both sides.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::Curve as SmCurve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::object::{NotifyOperation, ObjectRef};
use solidcore::geom::tol::zone_tol3d;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::face::p3;
use solidcore::topology::loopuse as lu1;
use solidcore::topology::topology_traverser;
use solidcore::topology::{Brep, FaceId, LoopId};
use solidcore::{EntityRef, SolidError};
use solidcore_ffi::units::topology_face_p3::{self as ffi, Op, N_ARGS};
use solidcore_ffi::units::topology_vertexuse::{self as fixture, Op as FixtureOp};

use super::topology_vertexuse::{self as shared, BrepIn};
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The difftest runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// `SmBSplineSurface_TYPE`, `SmPlane_TYPE`: imported exactly.
const EXACT_SURFACES: [i64; 2] = [15002, 15011];
/// `SmBSplineCurve_TYPE`, `SmLine_TYPE`: imported exactly.
const EXACT_CURVES: [i64; 2] = [14033, 14010];

/// "Upstream would dereference null" in an output slot.
const NULL_DEREF: f64 = -2.0;

/// The notifications of the `Notify` op, by index (as the shim's `NOTIFY_OPS`).
const NOTIFY_OPS: [NotifyOperation; ffi::NOTIFY_OPS] = [
    NotifyOperation::PreEdit,
    NotifyOperation::PostEdit,
    NotifyOperation::ChangeGeometry,
    NotifyOperation::AddToBrep,
    NotifyOperation::RmFromBrep,
];

// ---- inputs -----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    /// `which`: 0 `IsLamina`, 1 `HasLaminaEdge`, 2 `HasConsecutiveLaminaEdges`.
    Lamina {
        face: i64,
        which: u8,
    },
    SetTol {
        face: i64,
        tol: f64,
        only_if_larger: bool,
        cascade: bool,
    },
    /// `operation`: an index into [`NOTIFY_OPS`].
    Notify {
        face: i64,
        operation: u8,
    },
    /// `lp` selects the loop: `-1` (default) the first loop of the face, else any loop.
    LoopOrient {
        face: i64,
        #[serde(default = "minus_one")]
        lp: i64,
        want_bounds: bool,
        do_3d: bool,
    },
    Degenerate {
        face: i64,
        skip_quick: bool,
        tol: Option<f64>,
    },
    Composites {
        face: i64,
    },
    /// `own_curves`: pass copies of the face's UV trim curves, cleared first.
    UvTrim {
        face: i64,
        adjust: bool,
        #[serde(default)]
        own_curves: bool,
    },
    /// `coincidence`: `None` runs `CoincidenceCheck`.
    Intersect {
        face: i64,
        other: i64,
        approx_tol: f64,
        ang_tol_deg: f64,
        coincidence: Option<bool>,
        want_points: bool,
    },
}

fn minus_one() -> i64 {
    -1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    brep: BrepIn,
    #[serde(default)]
    validate_only: bool,
    #[serde(default)]
    init_gaps: bool,
    /// For a `Face` B-rep: parameter fractions of the face domain at which an inner
    /// vertex loop is added (`SmBrep::MakeVertexLoop`); block kind 4.
    #[serde(default)]
    vertex_loop: Option<[f64; 2]>,
    #[serde(flatten)]
    op: OpIn,
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

/// The B-rep block (as the `topology.vertexuse` runner's).
fn block(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut v = Vec::new();
    match &i.brep {
        BrepIn::File { rebuild, .. } => v.extend([0.0, flag(*rebuild)]),
        BrepIn::Wire { curve, t0, t1 } => {
            v.push(1.0);
            shared::push_curve(&mut v, curve)?;
            v.extend([*t0, *t1]);
        }
        BrepIn::Face { surface, dom } => {
            v.push(if i.vertex_loop.is_some() { 4.0 } else { 2.0 });
            shared::push_surface(&mut v, surface)?;
            v.extend(dom);
            if let Some(l) = i.vertex_loop {
                v.extend(l);
            }
        }
        BrepIn::Network => v.push(3.0),
    }
    v.push(flag(i.validate_only));
    v.push(flag(i.init_gaps));
    Ok(v)
}

/// A built B-rep: its export and counts.
struct Built {
    path: String,
    block: Vec<f64>,
    export: Vec<f64>,
    counts: [usize; 10],
}

/// Exports the case's B-rep; `Err(output)` when it could not be built.
fn build(i: &Inputs) -> Result<Result<Built, Output>, HarnessError> {
    let path = shared::path_of(&i.brep)?;
    let block = block(i)?;
    let export = crate::export_memo::export("topology_vertexuse", &[&path], &block, || {
        fixture::eval(FixtureOp::Export, &path, &block)
    })
    .map_err(|e| harness(format!("c++ export: {e}")))?;
    let Some(counts) = fixture::counts(&export) else {
        let status = export.get(1).copied().unwrap_or(0.0);
        return Ok(Err(Output::err(format!("brep_build_status_{status}"))));
    };
    Ok(Ok(Built {
        path,
        block,
        export,
        counts,
    }))
}

/// The face of a selector (`None` without faces).
fn face_sel(s: i64, n_f: usize) -> Option<usize> {
    let x = shared::sel(s.max(0), n_f);
    usize::try_from(x).ok()
}

/// The six shim arguments of the op, with selectors resolved; `None` when the B-rep has
/// no face (or no loop for `LoopOrient`).
fn args(
    op: &OpIn,
    b: &Built,
    loop_of_face: impl Fn(usize) -> Option<usize>,
) -> Option<[f64; N_ARGS]> {
    let [_, _, _, n_f, _, n_l, _, _, _, _] = b.counts;
    let f = |s: i64| face_sel(s, n_f).map(|x| x as f64);
    Some(match *op {
        OpIn::Lamina { face, which } => [f(face)?, f64::from(which.min(2)), 0.0, 0.0, 0.0, 0.0],
        OpIn::SetTol {
            face,
            tol,
            only_if_larger,
            cascade,
        } => [f(face)?, tol, flag(only_if_larger), flag(cascade), 0.0, 0.0],
        OpIn::Notify { face, operation } => [
            f(face)?,
            f64::from(operation % ffi::NOTIFY_OPS as u8),
            0.0,
            0.0,
            0.0,
            0.0,
        ],
        OpIn::LoopOrient {
            face,
            lp,
            want_bounds,
            do_3d,
        } => {
            let fi = face_sel(face, n_f)?;
            let l = if lp < 0 {
                loop_of_face(fi)?
            } else {
                usize::try_from(shared::sel(lp, n_l)).ok()?
            };
            [
                fi as f64,
                l as f64,
                flag(want_bounds),
                flag(do_3d),
                0.0,
                0.0,
            ]
        }
        OpIn::Degenerate {
            face,
            skip_quick,
            tol,
        } => [
            f(face)?,
            flag(skip_quick),
            flag(tol.is_some()),
            tol.unwrap_or(0.0),
            0.0,
            0.0,
        ],
        OpIn::Composites { face } => [f(face)?, 0.0, 0.0, 0.0, 0.0, 0.0],
        OpIn::UvTrim {
            face,
            adjust,
            own_curves,
        } => [f(face)?, flag(adjust), flag(own_curves), 0.0, 0.0, 0.0],
        OpIn::Intersect {
            face,
            other,
            approx_tol,
            ang_tol_deg,
            coincidence,
            want_points,
        } => [
            f(face)?,
            f(other)?,
            approx_tol,
            ang_tol_deg,
            coincidence.map_or(-1.0, flag),
            flag(want_points),
        ],
    })
}

fn op_of(op: &OpIn) -> Op {
    match op {
        OpIn::Lamina { .. } => Op::Lamina,
        OpIn::SetTol { .. } => Op::SetTol,
        OpIn::Notify { .. } => Op::Notify,
        OpIn::LoopOrient { .. } => Op::LoopOrient,
        OpIn::Degenerate { .. } => Op::Degenerate,
        OpIn::Composites { .. } => Op::Composites,
        OpIn::UvTrim { .. } => Op::UvTrim,
        OpIn::Intersect { .. } => Op::Intersect,
    }
}

/// The first loop of face `f` in the Rust import (the loop of its first face use's
/// first loop use), as the generated `LoopOrient` default.
fn first_loop(b: &Brep, f: usize) -> Option<usize> {
    let fu = b.face(FaceId::from_index(f as u32)).ok()?.faceuse?;
    let lu = *b.faceuse(fu).ok()?.loopuses.first()?;
    let l = b.loopuse(lu).ok()?.loop_?;
    Some(l.index() as usize)
}

// ---- C++ side ---------------------------------------------------------------------------

fn run_cpp_case(i: &Inputs) -> Result<Output, HarnessError> {
    let b = match build(i)? {
        Ok(b) => b,
        Err(out) => return Ok(out),
    };
    let (rb, _) = shared::import(&b.export)?;
    let Some(a) = args(&i.op, &b, |f| first_loop(&rb, f)) else {
        return Ok(Output::err("no face"));
    };
    let v =
        ffi::eval(op_of(&i.op), &b.path, &b.block, &a).map_err(|e| harness(format!("c++: {e}")))?;
    Ok(Output::new().values("out", v))
}

// ---- Rust side --------------------------------------------------------------------------

/// The identity each topology entity is notified under.
fn key_of(e: EntityRef) -> ObjectRef {
    let (type_code, kind, i) = match e {
        EntityRef::Brep(x) => (16011, 0u64, x.index()),
        EntityRef::Region(x) => (16012, 1, x.index()),
        EntityRef::Shell(x) => (16013, 2, x.index()),
        EntityRef::Faceuse(x) => (16022, 3, x.index()),
        EntityRef::Face(x) => (16002, 4, x.index()),
        EntityRef::Loopuse(x) => (16004, 5, x.index()),
        EntityRef::Loop(x) => (16003, 6, x.index()),
        EntityRef::Edgeuse(x) => (16005, 7, x.index()),
        EntityRef::Edge(x) => (16016, 8, x.index()),
        EntityRef::Vertexuse(x) => (16006, 9, x.index()),
        EntityRef::Vertex(x) => (16021, 10, x.index()),
    };
    ObjectRef {
        type_code,
        key: ObjectKey(1 + (kind << 32) + u64::from(i)),
    }
}

/// Tolerances of every face, edge and vertex.
fn put_tols(b: &Brep, out: &mut Vec<f64>) {
    out.extend(b.faces().iter().map(|(_, f)| f.tolerance));
    out.extend(b.edges().iter().map(|(_, e)| e.tolerance));
    out.extend(b.vertices().iter().map(|(_, v)| v.tolerance));
}

/// The edge use and vertex use states (see the FFI module docs).
fn put_use_states(b: &Brep, out: &mut Vec<f64>) {
    for (_, eu) in b.edgeuses().iter() {
        out.push(flag(eu.uv_trim_curve.is_some()));
        out.push(flag(eu.max_edge_face_gap.is_init()));
        out.push(flag(eu.edge_ccw_edge_gap.is_init()));
    }
    for (_, vu) in b.vertexuses().iter() {
        out.push(flag(vu.vertex_face_gap.is_init()));
        out.push(flag(vu.vertex_edge_gap.is_init()));
    }
}

/// A curve's samples (see the FFI module docs).
fn put_samples(c: &SmCurve, out: &mut Vec<f64>) -> Result<(), SolidError> {
    let ivl = c.natural_interval();
    out.extend([ivl.min(), ivl.max()]);
    for k in 0..=2u8 {
        match c.evaluate_point(&Kernel::default(), ivl.evaluate(f64::from(k) / 2.0)) {
            Ok(p) => out.extend([0.0, p.x, p.y, p.z]),
            Err(SolidError::Upstream { code, .. }) => out.extend([f64::from(code), 0.0, 0.0, 0.0]),
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Whether the face has a surface and every edge of its loops a curve (the shim's
/// `face_geometry_complete`).
fn face_geometry_complete(b: &Brep, f: FaceId) -> bool {
    let Ok(face) = b.face(f) else { return false };
    if face.surface.is_none() {
        return false;
    }
    match topology_traverser::face_edges(b, f) {
        Ok(es) => es
            .iter()
            .all(|&e| b.edge(e).is_ok_and(|e| e.curve.is_some())),
        Err(_) => false,
    }
}

/// Whether `ComputeLoopOrientation` of loop `l` on face `f` reads no missing geometry
/// (the shim's `loop_geometry_complete`): a vertex loop reads none; an edge loop needs
/// `f`'s surface and its edges' curves, and without `do_3d` the whole geometry of the
/// loop's own face (creating a UV trim curve builds that face's trimmed surface cache).
fn loop_geometry_complete(b: &Brep, f: FaceId, l: LoopId, do_3d: bool) -> bool {
    let Ok((lu, _)) = lu1::loop_loopuses(b, l) else {
        return false;
    };
    if lu1::is_vertex_loopuse(b, lu).unwrap_or(false) {
        return true;
    }
    if !b.face(f).is_ok_and(|face| face.surface.is_some()) {
        return false;
    }
    let Ok(eus) = lu1::edgeuses(b, lu, None, None) else {
        return false;
    };
    let curves = eus.iter().all(|&eu| {
        eu1::edge(b, eu)
            .ok()
            .flatten()
            .is_some_and(|e| b.edge(e).is_ok_and(|e| e.curve.is_some()))
    });
    if !curves {
        return false;
    }
    if do_3d {
        return true;
    }
    let loop_face = b
        .loopuse(lu)
        .ok()
        .and_then(|r| r.faceuse)
        .and_then(|fu| b.faceuse(fu).ok().and_then(|r| r.face));
    loop_face.is_some_and(|lf| face_geometry_complete(b, lf))
}

/// Whether `l` is a vertex loop (its first loop use is of vertex type): the one
/// `ComputeLoopOrientation` input that reads no geometry at all.
fn is_vertex_loop(b: &Brep, l: LoopId) -> bool {
    lu1::loop_loopuses(b, l)
        .and_then(|(lu, _)| lu1::is_vertex_loopuse(b, lu))
        .unwrap_or(false)
}

/// The deferral of a geometric op on a B-rep holding a kind the import has only as its
/// B-spline part (see the module docs).
fn inexact_geometry(types: &[f64]) -> Result<Option<Output>, HarnessError> {
    let bad = || harness("bad types export");
    let n_f = *types.get(1).ok_or_else(bad)? as usize;
    let surfaces = types.get(2..2 + n_f).ok_or_else(bad)?;
    let n_e = *types.get(2 + n_f).ok_or_else(bad)? as usize;
    let curves = types.get(3 + n_f..3 + n_f + n_e).ok_or_else(bad)?;
    for &t in surfaces {
        let t = t as i64;
        if t >= 0 && !EXACT_SURFACES.contains(&t) {
            // The analytic kinds' own units (`surface.cone_p1`, `surface.sphere`,
            // `surface.torus`) are ported; what is missing is their `SurfaceKind`
            // variant, i.e. the `SmSurface` dispatch of `surface.surface_p2` (issue
            // #76, the DISPATCH GAP of `Surface::read_from_db`).
            let (unit, symbol) = match t {
                15012 => ("surface.surface_p2", "SurfaceKind::Cone (SmCone dispatch)"),
                15013 => (
                    "surface.surface_p2",
                    "SurfaceKind::Cone (SmCylinder dispatch)",
                ),
                15014 => (
                    "surface.surface_p2",
                    "SurfaceKind::Sphere (SmSphere dispatch)",
                ),
                15015 => (
                    "surface.surface_p2",
                    "SurfaceKind::Torus (SmTorus dispatch)",
                ),
                15030 => (
                    "surface.surf_of_extrusion_p2",
                    "SurfaceKind::SurfOfExtrusion",
                ),
                _ => ("topology.brep_p7", "SmBrep::ReadFromFile"),
            };
            return Ok(Some(Output::deferred(unit, symbol)));
        }
    }
    if curves
        .iter()
        .any(|&t| t >= 0.0 && !EXACT_CURVES.contains(&(t as i64)))
    {
        return Ok(Some(Output::deferred(
            "topology.brep_p7",
            "SmBrep::ReadFromFile",
        )));
    }
    Ok(None)
}

/// Maps a Rust error of an op: `Unported` defers, an upstream status is returned for the
/// caller to output, anything else is a harness failure.
fn status_of(e: SolidError) -> Result<Result<i32, Output>, HarnessError> {
    if let Some(o) = Output::unported(&e) {
        return Ok(Err(o));
    }
    match e {
        SolidError::Upstream { code, .. } => Ok(Ok(code)),
        e => Err(harness(format!("rust: {e}"))),
    }
}

/// Unwraps an op result whose upstream function returns no status.
macro_rules! value {
    ($r:expr) => {
        match $r {
            Ok(v) => v,
            Err(e) => {
                if let Some(o) = Output::unported(&e) {
                    return Ok(o);
                }
                return Err(harness(format!("rust: {e}")));
            }
        }
    };
}

/// Unwraps an op result whose upstream function returns a status: `Ok(value)` or the
/// status code.
macro_rules! status {
    ($r:expr) => {
        match $r {
            Ok(v) => Ok(v),
            Err(e) => match status_of(e)? {
                Ok(code) => Err(code),
                Err(o) => return Ok(o),
            },
        }
    };
}

#[allow(clippy::too_many_lines)] // one arm per op
fn run_rust_case(i: &Inputs) -> Result<Output, HarnessError> {
    let built = match build(i)? {
        Ok(b) => b,
        Err(out) => return Ok(out),
    };
    let (mut b, _) = shared::import(&built.export)?;
    let Some(a) = args(&i.op, &built, |f| first_loop(&b, f)) else {
        return Ok(Output::err("no face"));
    };
    let op = op_of(&i.op);
    let f = FaceId::from_index(a[0] as u32);
    let loop_orient = matches!(i.op, OpIn::LoopOrient { .. });
    let vertex_loop = loop_orient && is_vertex_loop(&b, LoopId::from_index(a[1] as u32));
    if op as i32 >= Op::LoopOrient as i32 && !vertex_loop {
        let types = ffi::eval(Op::Types, &built.path, &built.block, &[])
            .map_err(|e| harness(format!("c++ types: {e}")))?;
        if let Some(o) = inexact_geometry(&types)? {
            return Ok(o);
        }
    }
    let k = Kernel::default();
    let mut out = vec![0.0];
    let complete = if loop_orient {
        loop_geometry_complete(&b, f, LoopId::from_index(a[1] as u32), a[3] != 0.0)
    } else {
        face_geometry_complete(&b, f)
            && (!matches!(i.op, OpIn::Intersect { .. })
                || face_geometry_complete(&b, FaceId::from_index(a[1] as u32)))
    };
    if op as i32 >= Op::LoopOrient as i32 && !complete {
        out.push(NULL_DEREF);
        return Ok(Output::new().values("out", out));
    }
    match &i.op {
        OpIn::Lamina { .. } => {
            let r = match a[1] as u8 {
                0 => p3::is_lamina(&b, f),
                1 => p3::has_lamina_edge(&b, f),
                _ => p3::has_consecutive_lamina_edges(&b, f),
            };
            out.push(flag(value!(r)));
        }
        OpIn::SetTol { .. } => {
            // `SetTolerance(SmZoneTol3d, ..)`: the shim's double converts implicitly
            // (USE_DEFAULT, then fabs)
            let tol = zone_tol3d(&k, a[1]);
            value!(p3::set_tolerance(&mut b, f, tol, a[2] != 0.0, a[3] != 0.0));
            put_tols(&b, &mut out);
        }
        OpIn::Notify { .. } => {
            let operation = *NOTIFY_OPS
                .get(a[1] as usize)
                .ok_or_else(|| harness("notify operation"))?;
            let mut records = BTreeMap::new();
            let mut attributes = AttributeStore::default();
            let mut env = solidcore::topology::edge::NotifyEnv {
                records: &mut records,
                attributes: &mut attributes,
                context: None,
                key_of: &key_of,
            };
            value!(p3::notify(&mut b, f, &mut env, None, operation, [None; 3]));
            let rect = b
                .face(f)
                .map_err(|e| harness(e.to_string()))?
                .rectangular_trim;
            out.push(f64::from(rect));
            put_use_states(&b, &mut out);
        }
        OpIn::LoopOrient { .. } => {
            let l = LoopId::from_index(a[1] as u32);
            let want_bounds = a[2] != 0.0;
            let mut bounds = Extent2d::new();
            let r = status!(p3::compute_loop_orientation(
                &k,
                &mut b,
                f,
                l,
                want_bounds.then_some(&mut bounds),
                a[3] != 0.0,
            ));
            let (code, orient) = match r {
                Ok(o) => (0, o),
                Err(code) => (code, OrientType::Unknown),
            };
            out.push(f64::from(code));
            out.push(match orient {
                OrientType::Unknown => 0.0,
                OrientType::Same => 1.0,
                OrientType::Opposite => 2.0,
            });
            if want_bounds {
                out.extend([
                    bounds.min().x,
                    bounds.min().y,
                    bounds.max().x,
                    bounds.max().y,
                ]);
            }
            put_use_states(&b, &mut out);
        }
        OpIn::Degenerate { .. } => {
            // the shim's `SmZoneTol3d tol = a[3]` (USE_DEFAULT, then fabs)
            let tol = (a[2] != 0.0).then_some(zone_tol3d(&k, a[3]));
            out.push(flag(value!(p3::is_degenerate(
                &k,
                &mut b,
                f,
                a[1] != 0.0,
                tol
            ))));
        }
        OpIn::Composites { .. } => match status!(p3::create_composites(&k, &mut b, f)) {
            Ok(comps) => {
                out.push(0.0);
                out.push(comps.len() as f64);
                for c in &comps {
                    out.push(c.num_segments() as f64);
                    let ivl = c.natural_interval();
                    out.extend([ivl.min(), ivl.max()]);
                    out.push(flag(c.is_closed()));
                    for k in 0..c.num_segments() {
                        let seg = c.segment(k).ok_or_else(|| harness("segment"))?;
                        out.push(f64::from(seg.transition as i32));
                        out.push(flag(seg.same_sense));
                        out.extend([seg.gap_start, seg.gap_end]);
                        value!(put_samples(seg.curve(), &mut out));
                    }
                }
            }
            Err(code) => out.extend([f64::from(code), 0.0]),
        },
        OpIn::UvTrim { .. } => {
            // a[2] != 0: the face's own UV trim curves (copies with their edge use's
            // orientation) are cleared and handed to the call (the shim's UV_TRIM).
            let mut own: Vec<Option<SmCurve>> = Vec::new();
            let mut orients = Vec::new();
            if a[2] != 0.0 {
                let mut face_eus = Vec::new();
                for (eu, _) in b.edgeuses().iter() {
                    if value!(eu1::is_loop_edgeuse(&b, eu)) && value!(eu1::face(&b, eu)) == Some(f)
                    {
                        face_eus.push(eu);
                    }
                }
                for &eu in &face_eus {
                    if let Some(c) = value!(eu1::uv_trim_curve_pointer(&b, eu)) {
                        own.push(Some(c.clone()));
                        orients.push(value!(eu1::orientation(&b, eu)));
                    }
                }
                for &eu in &face_eus {
                    value!(eu1::delete_uv_trim_curve(&mut b, eu));
                }
            }
            let opt = (a[2] != 0.0).then_some((&mut own[..], &orients[..]));
            match status!(p3::create_uv_trim_curves(&k, &mut b, f, a[1] != 0.0, opt)) {
                Ok(g) => {
                    out.push(0.0);
                    out.extend([
                        g.mean_curve_surface_gap,
                        g.max_curve_surface_gap,
                        g.mean_3d_vertex_gap,
                        g.max_3d_vertex_gap,
                        g.mean_ps_curve_curve_gap,
                        g.max_ps_curve_curve_gap,
                    ]);
                }
                Err(code) => {
                    out.push(f64::from(code));
                    out.extend([0.0; 6]);
                }
            }
            put_tols(&b, &mut out);
            put_use_states(&b, &mut out);
        }
        OpIn::Intersect { .. } => {
            let other = FaceId::from_index(a[1] as u32);
            let options = p3::FaceIntersectOptions {
                want_points: a[5] != 0.0,
                coincidence: (a[4] >= 0.0).then_some(a[4] > 0.5),
                ..p3::FaceIntersectOptions::default()
            };
            match status!(p3::face_intersect(&k, &b, f, other, a[2], a[3], &options)) {
                Ok(r) => {
                    out.push(0.0);
                    out.push(r.curves_3d.len() as f64);
                    for c in &r.curves_3d {
                        value!(put_samples(c, &mut out));
                    }
                    let points = r.points_3d.unwrap_or_default();
                    out.push(points.len() as f64);
                    for p in points {
                        out.extend([p.x, p.y, p.z]);
                    }
                }
                Err(code) => out.extend([f64::from(code), 0.0, 0.0]),
            }
        }
    }
    Ok(Output::new().values("out", out))
}

// ---- generation ---------------------------------------------------------------------------

fn gen_op(rng: &mut Rng) -> OpIn {
    let face = rng.below(1 << 20) as i64;
    let tol = |rng: &mut Rng| match rng.below(8) {
        0 => 0.0,
        1 => 1e-7,
        2 => 1e-3,
        3 => 1e-12,
        4 => rng.uniform(1e2, 1e3),
        5 => 1e3,
        _ => rng.uniform(0.0, 0.5),
    };
    match rng.below(10) {
        0 | 1 => OpIn::Lamina {
            face,
            which: rng.below(3) as u8,
        },
        2 => OpIn::SetTol {
            face,
            tol: tol(rng),
            only_if_larger: rng.chance(0.5),
            cascade: rng.chance(0.5),
        },
        3 => OpIn::Notify {
            face,
            operation: rng.below(ffi::NOTIFY_OPS as u64) as u8,
        },
        4 => OpIn::LoopOrient {
            face,
            lp: if rng.chance(0.7) {
                -1
            } else {
                rng.below(1 << 20) as i64
            },
            want_bounds: rng.chance(0.5),
            do_3d: rng.chance(0.7),
        },
        5 => OpIn::Degenerate {
            face,
            skip_quick: rng.chance(0.5),
            tol: rng.chance(0.4).then(|| tol(rng)),
        },
        6 => OpIn::Composites { face },
        7 => OpIn::UvTrim {
            face,
            adjust: rng.chance(0.5),
            own_curves: rng.chance(0.5),
        },
        _ => OpIn::Intersect {
            face,
            other: rng.below(1 << 20) as i64,
            approx_tol: *rng
                .choose(&[1e-12, 1e-8, 1e-6, 1e-4, 1e-3, 1e-2, 1e2, 1e3])
                .unwrap_or(&1e-4),
            ang_tol_deg: *rng.choose(&[0.0, 0.5, 5.0, 30.0]).unwrap_or(&5.0),
            coincidence: match rng.below(3) {
                0 => None,
                1 => Some(false),
                _ => Some(true),
            },
            want_points: rng.chance(0.5),
        },
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.face_p3"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        run_cpp_case(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        run_rust_case(&case.parse()?)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let brep = shared::gen_brep(rng);
        let vertex_loop = (matches!(brep, BrepIn::Face { .. }) && rng.chance(0.4))
            .then(|| [rng.uniform(0.1, 0.9), rng.uniform(0.1, 0.9)]);
        let i = Inputs {
            brep,
            vertex_loop,
            validate_only: rng.chance(0.1),
            init_gaps: rng.chance(0.3),
            op: gen_op(rng),
        };
        serde_json::to_value(i).ok()
    }
}
