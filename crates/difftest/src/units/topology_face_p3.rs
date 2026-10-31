//! Runner for unit `topology.face_p3`. C++: `solidcore_ffi::units::topology_face_p3`.
//! Rust: `solidcore::topology::face::p3`.
//!
//! The C++ side builds a real `SmBrep` per call from the B-rep blocks of the
//! `topology.vertexuse` runner (an upstream `.smb` test file, optionally with rebuilt UV
//! trim curves; a wire edge; a face from a B-spline surface or an `SmPlane`, optionally
//! with an inner vertex loop; the wire network of `prog_test` `my_test_topology`). The
//! Rust side builds the same B-rep itself ([`own_brep`]: the Rust kernel's builders, or
//! the Rust `.smb` reader, renumbered into the shim's traversal order) and runs the
//! ported functions on it, so both sides hold the native geometry and no C++ state reaches
//! the Rust side; results are compared as flags, statuses, tolerances, edge use / vertex
//! use states, the full UV trim curves `CreateUVTrimCurves` leaves
//! ([`crate::util::trim_curve`]) and sampled curve points (see the FFI module docs for
//! each op's output).
//!
//! `own_reader_fillet.json` runs `CreateUVTrimCurves` on every face of the `Fillet/`
//! files of the fillet executive sweeps (e.g. `sphere_diff_3cylinders.smb`, whose face
//! 7 trim of edge 30 diverged before #1215), as read and with rebuilt UV trim curves,
//! each side reading the file with its own reader.
//!
//! Face, loop and other-face selectors are taken modulo the B-rep's counts, so
//! generated cases need no counts; a B-rep without faces gives `no face` on both sides.
//!
//! **Touching faces.** The inline `touching_*` cases ([`touching_cases`]) build one B-rep
//! of several `CreateFaceFromSurface` faces (block kind 10: planes and canonical
//! cylinders, spheres and tori, `CreateCanonical` on both sides) that meet at single points
//! (a sphere on a plane, sphere on sphere, sphere on a cylinder, a torus on a plane, at
//! interior, seam, pole and boundary points, with gaps up to and beyond the tolerance)
//! and compare `FaceIntersect`'s touching points (`Point3DClassify` on both faces of the
//! one B-rep) and curves with the C++.
//!
//! **Hermite edge-loop faces.** The inline `hermite_*` cases ([`hermite_degenerate_cases`])
//! reach `IsDegenerate`'s edge-pair coincidence check: a one-face B-rep (block kind 14)
//! whose single edge loop is an explicit list of curves, each edge with its own tolerance,
//! built natively with `SmBrepConstructor` and on the Rust side from the same records.
//! `SmHermiteCurve` is a curve kind without a NURB, so its curve cache measures the
//! distance of its cubic spans and the tree the coincidence drops build depends on the
//! edge's tolerance (a B-spline span is exact and skips that test). The cases pair a
//! sliver face (its second edge retraces or nearly retraces the first at a mid-point
//! separation around the tolerances), a retraced four-edge loop (pair (0, 1) coincident
//! marks both edges, then the remaining pairs are skipped as both-known-coincident), a
//! four-edge loop whose six pairs are all compared and none coincide, and an `SmLine`
//! sliver as the exact-span control; the edge tolerances vary per edge (1e-10 to 1,
//! separately, and the `SmLine` control at 1e-10 against 1) and the optional
//! `IsDegenerate` zone tolerance is `None` and explicit. `skip_quick` is forced `true`
//! where the coincidence check is under test; the `*_quick` controls run the quick point
//! check instead. The native `SmBrepConstructor` vertex sharing is not replicated on the
//! Rust side (its loop uses, face-use mates and edge-use pairs are).


use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p2::CurveKind;
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

use super::topology_vertexuse::{self as shared, BrepIn, SrfIn};
use crate::runner::default_cases;
use crate::util::trim_curve::put_trim_curve;
use crate::{Case, CaseContext, CaseSource, HarnessError, Output, Rng, UnitDiff};

/// The difftest runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// Mirrors `new SmBSplineCurve(*c)` in the shim: the native base-class copy slices an
/// analytic `SmBSplineCurve` subclass, then applies the B-spline copy constructor's
/// knot fixup. Do not preserve the dynamic curve kind here.
fn native_bspline_copy(curve: &SmCurve) -> Option<SmCurve> {
    let base = match curve.kind() {
        CurveKind::BSpline(bspline) => bspline,
        CurveKind::Line(line) => line.base(),
        CurveKind::Ellipse(ellipse) => ellipse.base(),
        CurveKind::Hyperbola(hyperbola) => hyperbola.base(),
        CurveKind::Parabola(parabola) => parabola.base(),
        _ => return None,
    };
    Some(SmCurve::from(base.copy()))
}

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

/// A canonical analytic surface of a kind-10 face (the shim's `CreateCanonical` kinds).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Shape {
    /// `SmCylinder`, radius `scale * a`.
    Cylinder,
    /// `SmSphere`, radius `scale * a`.
    Sphere,
    /// `SmTorus`, major radius `scale * a`, minor `scale * b`.
    Torus,
}

/// One face of a kind-10 B-rep block.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "face", rename_all = "snake_case")]
enum FaceIn {
    /// A surface block over `dom`, as a `Face` B-rep's.
    Surface { surface: SrfIn, dom: [f64; 4] },
    /// A canonical analytic surface at `origin` (Z axis along z, x or y for `axis` 0, 1,
    /// 2) over the fractions `from`..`to` of its natural domain (an unbounded direction
    /// clamped to `[0, scale * c]`).
    Analytic {
        shape: Shape,
        scale: f64,
        a: f64,
        #[serde(default)]
        b: f64,
        #[serde(default)]
        c: f64,
        origin: [f64; 3],
        #[serde(default)]
        axis: u8,
        from: [f64; 2],
        to: [f64; 2],
    },
}

/// A B-rep of several faces (block kind 10), only for this runner.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum FacesBrep {
    Faces { faces: Vec<FaceIn> },
}

/// A curve of an edge-loop face (block kind 14, `support/edge_loop_face_fixture.h`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "curve", rename_all = "snake_case")]
enum LoopCurveIn {
    /// `SmLine(a, b)`: an exact-span curve.
    Line { a: [f64; 3], b: [f64; 3] },
    /// `SmHermiteCurve(p1, d1, p2, d2, 3)`: a curve kind without a NURB, whose curve
    /// cache measures the distance of its cubic spans, so its tree depends on the edge's
    /// tolerance.
    Hermite {
        p1: [f64; 3],
        d1: [f64; 3],
        p2: [f64; 3],
        d2: [f64; 3],
    },
}

/// One edge of an edge-loop face: its curve, its interval and its own tolerance
/// (`SmEdge::SetTolerance(tol, FALSE, FALSE)`).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LoopEdgeIn {
    curve: LoopCurveIn,
    t: [f64; 2],
    tol: f64,
}

/// A one-face B-rep with an edge loop of explicit curves and per-edge tolerances (block
/// kind 14): an `SmPlane` face over `dom` whose one loop is `edges` in order, built with
/// `SmBrepConstructor` natively and from the same records on the Rust side. Unlike the
/// shared kinds this source keeps two near-coincident edge curves apart (the constructor
/// does not merge them, unlike `MakeFaceWithCurves`), which `IsDegenerate`'s edge-pair
/// coincidence check needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EdgeLoopFaceIn {
    /// The plane's origin.
    o: [f64; 3],
    /// The plane's normal.
    n: [f64; 3],
    /// The face's UV domain `u0 v0 u1 v1`.
    dom: [f64; 4],
    /// `SmFace::SetTolerance(tol, FALSE, FALSE)`.
    face_tol: f64,
    edges: Vec<LoopEdgeIn>,
}

/// The case's B-rep: the shared `topology.vertexuse` kinds, several faces, or an edge-loop
/// face with per-edge tolerances.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum CaseBrep {
    Faces(FacesBrep),
    EdgeLoop(EdgeLoopFaceIn),
    Shared(BrepIn),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    brep: CaseBrep,
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
    let brep = match &i.brep {
        CaseBrep::Shared(b) => b,
        CaseBrep::Faces(FacesBrep::Faces { faces }) => {
            v.extend([10.0, faces.len() as f64]);
            for face in faces {
                push_face(&mut v, face)?;
            }
            v.push(flag(i.validate_only));
            v.push(flag(i.init_gaps));
            return Ok(v);
        }
        CaseBrep::EdgeLoop(f) => {
            v.push(14.0);
            v.extend(f.o);
            v.extend(f.n);
            v.extend(f.dom);
            v.push(f.face_tol);
            v.push(f.edges.len() as f64);
            for e in &f.edges {
                match &e.curve {
                    LoopCurveIn::Line { a, b } => {
                        v.push(1.0);
                        v.extend(a);
                        v.extend(b);
                    }
                    LoopCurveIn::Hermite { p1, d1, p2, d2 } => {
                        v.push(3.0);
                        for q in [p1, d1, p2, d2] {
                            v.extend(q);
                        }
                    }
                }
                v.extend(e.t);
                v.push(e.tol);
            }
            v.push(flag(i.validate_only));
            v.push(flag(i.init_gaps));
            return Ok(v);
        }
    };
    match brep {
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

/// One face of a kind-10 block.
fn push_face(v: &mut Vec<f64>, face: &FaceIn) -> Result<(), HarnessError> {
    match face {
        FaceIn::Surface { surface, dom } => {
            shared::push_surface(v, surface)?;
            v.extend(dom);
        }
        FaceIn::Analytic {
            shape,
            scale,
            a,
            b,
            c,
            origin,
            axis,
            from,
            to,
        } => {
            let kind = match shape {
                Shape::Cylinder => 10.0,
                Shape::Sphere => 11.0,
                Shape::Torus => 12.0,
            };
            v.extend([kind, *scale, *a, *b, *c]);
            v.extend(origin);
            v.push(f64::from(*axis));
            v.extend(from);
            v.extend(to);
        }
    }
    Ok(())
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
    let path = match &i.brep {
        CaseBrep::Shared(b) => shared::path_of(b)?,
        CaseBrep::Faces(_) | CaseBrep::EdgeLoop(_) => String::new(),
    };
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
    counts: [usize; 10],
    loop_of_face: impl Fn(usize) -> Option<usize>,
) -> Option<[f64; N_ARGS]> {
    let [_, _, _, n_f, _, n_l, _, _, _, _] = counts;
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
    // the Rust graph of the export is read only to resolve a face's first loop (the
    // `LoopOrient` default selector); an edge-loop face's Hermite curves are not in the
    // shared export, so no other op imports it
    let imported = if matches!(i.op, OpIn::LoopOrient { .. }) {
        Some(shared::import_raw(&b.export)?)
    } else {
        None
    };
    let Some(a) = args(&i.op, b.counts, |f| {
        imported.as_ref().and_then(|(rb, _)| first_loop(rb, f))
    }) else {
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

/// The Rust B-rep of the case and its entity counts, built by the Rust kernel as the
/// shim (`sc_topology_fixture.h`'s `build`) builds it, or the output of a failed build:
/// the shared kinds by [`super::topology_face_p1::own_brep`], a face with an inner vertex
/// loop (block kind 4) and the several-face B-rep (block kind 10) here. No C++ state
/// reaches it.
fn own_brep(i: &Inputs) -> Result<crate::own_builder::RustBrep<(Brep, [usize; 10])>, HarnessError> {
    use crate::own_builder::{build_status, counts, finish, in_new_brep, Built as Own, Conventions};
    let mut k = Kernel::default();
    let conv = &Conventions::TOPOLOGY_FIXTURE;
    let built = match (&i.brep, i.vertex_loop) {
        (CaseBrep::Shared(BrepIn::Face { surface, dom }), Some([fu, fv])) => {
            let s = surface_of(surface)?;
            let dom = *dom;
            let raw = in_new_brep(&mut k, |k, b| {
                // kind 4: the face of kind 2 plus an inner vertex loop at the surface
                // point whose parameters are the given fractions of the face domain
                let face = crate::own_builder::create_face(k, b, s, dom)?;
                let uv = solidcore::geom::vector2d::Point2::new(
                    dom[0] + fu * (dom[2] - dom[0]),
                    dom[1] + fv * (dom[3] - dom[1]),
                );
                let pt = solidcore::topology::face::p2::surface(b, face)?
                    .ok_or_else(|| SolidError::sm_err("difftest::face_p3::vertex_loop"))?
                    .evaluate_point(k, uv)?;
                let mut state = solidcore::topology::brep::p2::ArenaNotifyState::new(b);
                let mut env = state.env(None);
                solidcore::topology::brep::p9::make_vertex_loop(
                    b,
                    face,
                    pt,
                    OrientType::Opposite,
                    &mut env,
                )?;
                Ok(())
            })?;
            finish(&mut k, raw, i.validate_only, i.init_gaps, conv)?
        }
        (CaseBrep::Shared(src), _) => {
            super::topology_face_p1::own_built(src, i.validate_only, i.init_gaps)?
        }
        (CaseBrep::EdgeLoop(f), _) => {
            let raw = rust_edge_loop_face(&mut k, f);
            finish(&mut k, raw, i.validate_only, i.init_gaps, conv)?
        }
        (CaseBrep::Faces(FacesBrep::Faces { faces }), _) => {
            let mut surfaces = Vec::new();
            for face in faces {
                surfaces.push(match face {
                    FaceIn::Surface { surface, dom } => Ok((surface_of(surface)?, *dom)),
                    FaceIn::Analytic { .. } => Err(face),
                });
            }
            let raw = in_new_brep(&mut k, |k, b| {
                for s in surfaces {
                    let (s, dom) = match s {
                        Ok(x) => x,
                        Err(face) => analytic_face(k, face)?,
                    };
                    crate::own_builder::create_face(k, b, s, dom)?;
                }
                Ok(())
            })?;
            finish(&mut k, raw, i.validate_only, i.init_gaps, conv)?
        }
    };
    Ok(match built {
        Own::Brep(b) => {
            let c = counts(&b);
            Ok((*b, c))
        }
        Own::Failed(e) => Err(Output::unported(&e).unwrap_or_else(|| {
            Output::err(format!("brep_build_status_{}", build_status(&e)))
        })),
    })
}

/// The Rust B-rep of an edge-loop face source (block kind 14): the records the shim's
/// `SmBrepConstructor` realises (`support/edge_loop_face_fixture.h`), built on the Rust
/// side — the ported `SmBrep(ctx)` root (`new_brep`: the infinite region on the region
/// list, the context tolerance, the construction notification), its shell inserted into
/// that region as `StartShell` does, then one face on a real `Plane` over the given UV
/// domain, one loop with its two loop uses (the upward one and its mate), the given edge
/// curves in loop order with one edge per curve and its own interval and tolerance, one
/// edge use pair per edge (same then opposite orientation), and one vertex per edge start
/// (a closed loop's consecutive edges share their vertex). The loop use starts at the
/// first edge and its mate at that edge's mate, as `StartEdge` leaves them.
fn rust_edge_loop_face(k: &mut Kernel, f: &EdgeLoopFaceIn) -> Result<Brep, SolidError> {
    use solidcore::curve::hermite_curve::HermiteCurve;
    use solidcore::curve::line::Line;
    use solidcore::geom::vector2d::Point2;
    use solidcore::geom::vector3d::Vec3;
    use solidcore::surface::plane::Plane;
    use solidcore::surface::surface::Surface;
    use solidcore::topology::shell::ShellMembers;
    use solidcore::topology::{
        Edge, EdgeId, Edgeuse, EdgeuseId, EdgeuseOwner, Face, Faceuse, FaceuseId, Loop, LoopEdge,
        Loopuse, LoopuseId, LoopuseStart, ShellId, Vertex, VertexId, Vertexuse, VertexuseId,
        VertexuseOwner,
    };

    let n = f.edges.len();
    if n == 0 {
        return Err(SolidError::invalid_input(
            "difftest::face_p3::edge_loop_face",
            "no edges",
        ));
    }
    let index = |i: usize| u32::try_from(i).unwrap_or(u32::MAX);
    let mut curves = Vec::with_capacity(n);
    for e in &f.edges {
        curves.push(match &e.curve {
            LoopCurveIn::Line { a, b } => SmCurve::from(Line::from_points(
                Vec3::from_array(*a),
                Vec3::from_array(*b),
                3,
            )),
            LoopCurveIn::Hermite { p1, d1, p2, d2 } => SmCurve::from(HermiteCurve::new(
                Vec3::from_array(*p1),
                Vec3::from_array(*d1),
                Vec3::from_array(*p2),
                Vec3::from_array(*d2),
                3,
            )),
        });
    }
    // the vertices at the edges' interval starts (evaluated before the curves move into
    // the edge records)
    let mut starts = Vec::with_capacity(n);
    for (e, c) in f.edges.iter().zip(&curves) {
        starts.push(c.evaluate_point(k, e.t[0])?);
    }

    let uv_domain = Extent2d::from_points(
        Point2::new(f.dom[0], f.dom[1]),
        Point2::new(f.dom[2], f.dom[3]),
    );
    // `StartBrep`: the ported `SmBrep(ctx)` root -- infinite region on the B-rep's
    // region list, the context tolerance, the construction notification
    let mut b = solidcore::topology::brep::p3::new_brep(
        k,
        solidcore::geom::tol::USE_DEFAULT,
        solidcore::geom::tol::USE_DEFAULT,
    )?;
    let region = b
        .infinite_region()
        .ok_or_else(|| SolidError::null_pointer("difftest::face_p3::edge_loop_face"))?;
    b.add_shell_record(solidcore::topology::Shell {
        faces: vec![FaceId::from_index(0)],
        region: Some(region),
        // `StartShell`'s `SetShellType(SmFaceuse_TYPE)`
        shell_type: solidcore::topology::shell::ShellType::Faceuse,
        members: ShellMembers::Faceuses(vec![FaceuseId::from_index(0), FaceuseId::from_index(1)]),
    })?;
    // `StartShell`'s `m_pOuterRegion->PostInsert(m_pOuterShell)`
    b.region_mut(region)?.shells.push(ShellId::from_index(0));
    b.add_face_record(Face {
        loops: vec![LoopId::from_index(0)],
        surface: Some(Surface::from(Plane::from_normal(
            Vec3::from_array(f.o),
            Vec3::from_array(f.n),
        ))),
        faceuse: Some(FaceuseId::from_index(0)),
        uv_domain,
        // SmFace::SetTolerance(tol, FALSE, FALSE)
        tolerance: zone_tol3d(k, f.face_tol),
        ..Face::new(ShellId::from_index(0))
    })?;
    b.add_loop_record(Loop {
        face: FaceId::from_index(0),
        edges: (0..n)
            .map(|i| LoopEdge {
                edge: EdgeId::from_index(index(i)),
                forward: true,
            })
            .collect(),
        loopuse: Some(LoopuseId::from_index(0)),
    })?;
    // the upward face use and its mate (both in the shell, as `StartFace` inserts them)
    b.add_faceuse_record(Faceuse {
        shell: Some(ShellId::from_index(0)),
        face: Some(FaceId::from_index(0)),
        mate: Some(FaceuseId::from_index(1)),
        orientation: OrientType::Same,
        loopuses: vec![LoopuseId::from_index(0)],
        ..Faceuse::default()
    })?;
    b.add_faceuse_record(Faceuse {
        shell: Some(ShellId::from_index(0)),
        face: Some(FaceId::from_index(0)),
        mate: Some(FaceuseId::from_index(0)),
        orientation: OrientType::Opposite,
        loopuses: vec![LoopuseId::from_index(1)],
        ..Faceuse::default()
    })?;
    // the loop's two uses: the primary walks the edges' same-orientation edge uses from
    // the first edge, its mate the opposite-orientation ones from that edge's mate
    b.add_loopuse_record(Loopuse {
        faceuse: Some(FaceuseId::from_index(0)),
        loop_: Some(LoopId::from_index(0)),
        mate: Some(LoopuseId::from_index(1)),
        start: LoopuseStart::Edgeuse(Some(EdgeuseId::from_index(0))),
        orientation: OrientType::Same,
    })?;
    b.add_loopuse_record(Loopuse {
        faceuse: Some(FaceuseId::from_index(1)),
        loop_: Some(LoopId::from_index(0)),
        mate: Some(LoopuseId::from_index(0)),
        start: LoopuseStart::Edgeuse(Some(EdgeuseId::from_index(index(n)))),
        orientation: OrientType::Same,
    })?;
    // the brep tolerance the constructor gives its vertices (`m_pBrep->GetTolerance()`)
    let vertex_tol = b.tolerance();
    for (i, &point) in starts.iter().enumerate() {
        b.add_vertex_record(Vertex {
            point,
            tolerance: vertex_tol,
            vertexuses: vec![
                VertexuseId::from_index(index(i)),
                VertexuseId::from_index(index(n + (i + n - 1) % n)),
            ],
            poly_vertex: None,
        })?;
    }
    // edge uses: [same, opposite] per edge, the same ones walking the loop counter
    // clockwise and the opposite ones walking it the other way from their mate
    for i in 0..n {
        b.add_edgeuse_record(Edgeuse {
            edge: Some(EdgeId::from_index(index(i))),
            owner: EdgeuseOwner::Loopuse(Some(LoopuseId::from_index(0))),
            ccw: Some(EdgeuseId::from_index(index((i + 1) % n))),
            cw: Some(EdgeuseId::from_index(index((i + n - 1) % n))),
            vertexuse: Some(VertexuseId::from_index(index(i))),
            orientation: OrientType::Same,
            ..Edgeuse::default()
        })?;
    }
    for i in 0..n {
        b.add_edgeuse_record(Edgeuse {
            edge: Some(EdgeId::from_index(index(i))),
            owner: EdgeuseOwner::Loopuse(Some(LoopuseId::from_index(1))),
            ccw: Some(EdgeuseId::from_index(index(n + (i + n - 1) % n))),
            cw: Some(EdgeuseId::from_index(index(n + (i + 1) % n))),
            vertexuse: Some(VertexuseId::from_index(index(n + i))),
            orientation: OrientType::Opposite,
            ..Edgeuse::default()
        })?;
    }
    for i in 0..n {
        b.add_vertexuse_record(Vertexuse {
            vertex: Some(VertexId::from_index(index(i))),
            owner: VertexuseOwner::Edgeuse(Some(EdgeuseId::from_index(index(i)))),
            ..Vertexuse::default()
        })?;
    }
    for i in 0..n {
        b.add_vertexuse_record(Vertexuse {
            vertex: Some(VertexId::from_index(index((i + 1) % n))),
            owner: VertexuseOwner::Edgeuse(Some(EdgeuseId::from_index(index(n + i)))),
            ..Vertexuse::default()
        })?;
    }
    for ((e, curve), i) in f.edges.iter().zip(curves).zip(0..n) {
        b.add_edge_record(Edge {
            vertices: [
                VertexId::from_index(index(i)),
                VertexId::from_index(index((i + 1) % n)),
            ],
            edgeuses: vec![
                EdgeuseId::from_index(index(i)),
                EdgeuseId::from_index(index(n + i)),
            ],
            interval: solidcore::geom::extent1d::Extent1d::from_min_max(e.t[0], e.t[1]),
            // SmEdge::SetTolerance(tol, FALSE, FALSE)
            tolerance: zone_tol3d(k, e.tol),
            curve: Some(curve),
        })?;
    }
    Ok(b)
}

/// A `topology.vertexuse` surface block as the Rust surface the shim's `read_surface`
/// makes.
fn surface_of(s: &SrfIn) -> Result<solidcore::surface::surface::Surface, HarnessError> {
    let s: super::topology_edgeuse_p1::SrfIn = serde_json::to_value(s)
        .and_then(serde_json::from_value)
        .map_err(|e| harness(format!("surface block: {e}")))?;
    super::topology_edgeuse_p1::surface_of(&s)
}

/// The shim's `analytic_or_surface_face` for a canonical analytic face: the surface
/// (`CreateCanonical` on `SmAxis2Placement(o, x, y)`) and the face domain, the given
/// fractions of its natural UV domain (an unbounded direction clamped to
/// `[0, scale * c]`).
fn analytic_face(
    k: &Kernel,
    face: &FaceIn,
) -> Result<(solidcore::surface::surface::Surface, [f64; 4]), SolidError> {
    use solidcore::geom::axis2_placement::Axis2Placement;
    use solidcore::math::Vec3;
    use solidcore::surface::surface::Surface;
    let FaceIn::Analytic {
        shape,
        scale,
        a,
        b,
        c,
        origin,
        axis,
        from,
        to,
    } = face
    else {
        return Err(SolidError::invalid_input(
            "difftest::face_p3",
            "not an analytic face",
        ));
    };
    let x = if *axis == 1 {
        Vec3::new(0.0, 1.0, 0.0)
    } else {
        Vec3::new(1.0, 0.0, 0.0)
    };
    let y = match axis {
        0 => Vec3::new(0.0, 1.0, 0.0),
        1 => Vec3::new(0.0, 0.0, 1.0),
        _ => Vec3::new(0.0, 0.0, -1.0),
    };
    let o = Axis2Placement::new(Vec3::from_array(*origin), x, y);
    let s = match shape {
        Shape::Cylinder => Surface::from(
            solidcore::surface::cone::Cone::create_canonical_cylinder(k, &o, scale * a)?,
        ),
        Shape::Sphere => Surface::from(solidcore::surface::sphere::Sphere::create_canonical(
            k,
            &o,
            scale * a,
        )?),
        Shape::Torus => Surface::from(solidcore::surface::torus::Torus::create_canonical(
            k,
            &o,
            scale * a,
            scale * b,
            solidcore::surface::torus::TorusOptions::default(),
            None,
        )?),
    };
    let dom = s.natural_uv_domain_with_nurb(k)?;
    let (mut lo, mut hi) = (dom.min(), dom.max());
    if hi.x - lo.x > 1e5 {
        lo.x = 0.0;
        hi.x = scale * c;
    }
    if hi.y - lo.y > 1e5 {
        lo.y = 0.0;
        hi.y = scale * c;
    }
    let d0 = [
        lo.x + from[0] * (hi.x - lo.x),
        lo.y + from[1] * (hi.y - lo.y),
    ];
    let d1 = [lo.x + to[0] * (hi.x - lo.x), lo.y + to[1] * (hi.y - lo.y)];
    Ok((s, [d0[0], d0[1], d1[0], d1[1]]))
}

#[allow(clippy::too_many_lines)] // one arm per op
fn run_rust_case(i: &Inputs) -> Result<Output, HarnessError> {
    // an input the FFI refuses is a harness error on both sides (PR #502's convention):
    // the shim never builds it, so the Rust side must not either. Fuzz case fuzz00127
    // (`--fuzz 300 --seeds 24`) mutated a control point to `1e300`: the export refuses a
    // block value above `1e150` (HANG-29) for the C++ side, while this side built the
    // face for over 40 s. `validate_block` is the export's own block checks (pure Rust,
    // no shim call), on the path and block `build` sends.
    let path = match &i.brep {
        CaseBrep::Shared(b) => shared::path_of(b)?,
        CaseBrep::Faces(_) | CaseBrep::EdgeLoop(_) => String::new(),
    };
    fixture::validate_block(&path, &block(i)?).map_err(|e| harness(format!("export: {e}")))?;
    let (mut b, counts) = match own_brep(i)? {
        Ok(x) => x,
        Err(out) => return Ok(out),
    };
    let Some(a) = args(&i.op, counts, |f| first_loop(&b, f)) else {
        return Ok(Output::err("no face"));
    };
    let op = op_of(&i.op);
    let f = FaceId::from_index(a[0] as u32);
    let loop_orient = matches!(i.op, OpIn::LoopOrient { .. });
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
            let mut records = solidcore::topology::topology::NotifyRecords::new();
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
                        own.push(Some(native_bspline_copy(c).ok_or_else(|| {
                            harness("UV trim curve is not an SmBSplineCurve kind")
                        })?));
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
            for (_, eu) in b.edgeuses().iter() {
                value!(put_trim_curve(&k, &mut out, eu.uv_trim_curve.as_ref()));
            }
            // each UV trim curve's owner (the shim's `g.find(6, uv->GetOwner())`)
            for (eu, _) in b.edgeuses().iter() {
                out.push(crate::own_builder::uv_owner_index(&b, eu, None));
            }
        }
        OpIn::Intersect { .. } => {
            let other = FaceId::from_index(a[1] as u32);
            let options = p3::FaceIntersectOptions {
                want_points: a[5] != 0.0,
                coincidence: (a[4] >= 0.0).then_some(a[4] > 0.5),
                ..p3::FaceIntersectOptions::default()
            };
            match status!(p3::face_intersect(
                &k, &mut b, f, other, a[2], a[3], &options
            )) {
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

/// A sphere face of radius `r` at `origin` over the fractions `from`..`to` of its domain.
fn sphere(r: f64, origin: [f64; 3], axis: u8, from: [f64; 2], to: [f64; 2]) -> FaceIn {
    FaceIn::Analytic {
        shape: Shape::Sphere,
        scale: 1.0,
        a: r,
        b: 0.0,
        c: 0.0,
        origin,
        axis,
        from,
        to,
    }
}

/// A plane face through `o` with normal `n` over `dom` (plane parameters about `o`).
fn plane(o: [f64; 3], n: [f64; 3], dom: [f64; 4]) -> FaceIn {
    FaceIn::Surface {
        surface: SrfIn::Plane { o, n },
        dom,
    }
}

/// One edge of an edge-loop face case: `[0, 1]` of the curve and the edge's tolerance.
fn loop_edge(curve: LoopCurveIn, tol: f64) -> LoopEdgeIn {
    LoopEdgeIn {
        curve,
        t: [0.0, 1.0],
        tol,
    }
}

/// `SmHermiteCurve(p0, d0, p1, d1, 3)`: a cubic arc from `p0` to `p1`.
fn hermite(p0: [f64; 3], d0: [f64; 3], p1: [f64; 3], d1: [f64; 3]) -> LoopCurveIn {
    LoopCurveIn::Hermite {
        p1: p0,
        d1: d0,
        p2: p1,
        d2: d1,
    }
}

/// The two curves of a sliver face on `z = 0`: a Hermite arc from `(0, 0, 0)` to
/// `(s, 0, 0)` bending by `bend * s` in y, and the same path walked backwards with the
/// two y derivatives moved apart so the reversed curve's mid point shifts by `sep`
/// (cubic Hermite: `h10(0.5) = 0.125 = -h11(0.5)`, so `ds = 4 * sep` shifts it by
/// `0.25 * ds`). `sep == 0` is exactly the reversed path.
fn sliver_curves(s: f64, bend: f64, sep: f64) -> Vec<LoopCurveIn> {
    let p0 = [0.0, 0.0, 0.0];
    let p1 = [s, 0.0, 0.0];
    let ds = 4.0 * sep;
    vec![
        hermite(p0, [0.5 * s, bend * s, 0.0], p1, [0.5 * s, -bend * s, 0.0]),
        hermite(
            p1,
            [-0.5 * s, bend * s + ds, 0.0],
            p0,
            [-0.5 * s, -bend * s - ds, 0.0],
        ),
    ]
}

/// Four distinct Hermite arcs around a square of side `s`: each chord's end tangents
/// are a third of the chord plus a bend perpendicular to it, so no two edges are
/// coincident.
fn quad_curves(s: f64) -> Vec<LoopCurveIn> {
    let c = [[0.0, 0.0, 0.0], [s, 0.0, 0.0], [s, s, 0.0], [0.0, s, 0.0]];
    c.iter()
        .zip(c.iter().cycle().skip(1))
        .map(|(&a, &b)| {
            let v = [b[0] - a[0], b[1] - a[1], 0.0];
            let d0 = [v[0] / 3.0 + 0.15 * v[1], v[1] / 3.0 - 0.15 * v[0], 0.0];
            let d1 = [v[0] / 3.0 - 0.15 * v[1], v[1] / 3.0 + 0.15 * v[0], 0.0];
            hermite(a, d0, b, d1)
        })
        .collect()
}

/// The edge-loop face case of `curves` (plane `z = 0` over a domain enclosing them) with
/// the edges' tolerances and the `IsDegenerate` arguments.
fn hermite_case(
    curves: Vec<LoopCurveIn>,
    tols: &[f64],
    s: f64,
    skip_quick: bool,
    tol: Option<f64>,
) -> Inputs {
    let dom = [-1.0, -1.0, s + 1.0, s + 1.0];
    Inputs {
        brep: CaseBrep::EdgeLoop(EdgeLoopFaceIn {
            o: [0.0, 0.0, 0.0],
            n: [0.0, 0.0, 1.0],
            dom,
            face_tol: 1e-6,
            edges: curves
                .into_iter()
                .enumerate()
                .map(|(i, c)| loop_edge(c, tols.get(i).copied().unwrap_or(1e-3)))
                .collect(),
        }),
        validate_only: false,
        init_gaps: false,
        vertex_loop: None,
        op: OpIn::Degenerate {
            face: 0,
            skip_quick,
            tol,
        },
    }
}

/// The inline Hermite edge-loop cases (see the module docs): real faces with at least two
/// non-degenerate Hermite boundary edges, whose `IsDegenerate` runs the edge-pair
/// coincidence check (`skip_quick`) and, as controls, the quick point check.
fn hermite_degenerate_cases() -> Vec<(String, Value)> {
    let sliver = |s, sep| sliver_curves(s, 0.25, sep);
    let retraced_sliver = |s| {
        let mut c = sliver_curves(s, 0.25, 0.0);
        c.extend(sliver_curves(s, 0.25, 0.0));
        c
    };
    let cases: Vec<(&str, Inputs)> = vec![
        // edge 1 exactly retraces edge 0: every pair coincident, face degenerate
        (
            "hermite_sliver_exact_skip",
            hermite_case(sliver(1.0, 0.0), &[1e-3, 1e-3], 1.0, true, None),
        ),
        (
            "hermite_sliver_exact_asym_tol",
            hermite_case(sliver(1.0, 0.0), &[1e-10, 1.0], 1.0, true, None),
        ),
        (
            "hermite_sliver_exact_scale1e6",
            hermite_case(sliver(1e6, 0.0), &[1e-10, 1e-3], 1e6, true, None),
        ),
        // the optional zone tolerance replaces the edge (and coincidence) tolerances
        (
            "hermite_sliver_opt_tol_small",
            hermite_case(sliver(1.0, 0.0), &[1e-3, 1e-3], 1.0, true, Some(1e-6)),
        ),
        (
            "hermite_sliver_opt_tol_big",
            hermite_case(sliver(1.0, 0.0), &[1e-3, 1e-3], 1.0, true, Some(0.5)),
        ),
        // the quick check runs first: a zero-area face has no inside to step into
        (
            "hermite_sliver_quick",
            hermite_case(sliver(1.0, 0.0), &[1e-3, 1e-3], 1.0, false, None),
        ),
        // edge 1 is 2e-3 away from edge 0: coincident within the edges' tolerances ...
        (
            "hermite_sliver_near_coincident",
            hermite_case(sliver(1.0, 2e-3), &[1e-2, 1e-2], 1.0, true, None),
        ),
        // ... and apart with tighter ones, and one wide edge tolerance is enough
        (
            "hermite_sliver_near_separate",
            hermite_case(sliver(1.0, 2e-3), &[1e-5, 1e-5], 1.0, true, None),
        ),
        (
            "hermite_sliver_near_mixed",
            hermite_case(sliver(1.0, 2e-3), &[1e-5, 1e-2], 1.0, true, None),
        ),
        // four distinct edges: the six pairs are checked and none is coincident
        (
            "hermite_quad_skip",
            hermite_case(quad_curves(1.0), &[1e-3; 4], 1.0, true, None),
        ),
        (
            "hermite_quad_skip_scale1e3",
            hermite_case(quad_curves(1e3), &[1e-10, 1e-3, 1.0, 1e-7], 1e3, true, None),
        ),
        // the quick check classifies a point stepped into the face
        (
            "hermite_quad_quick",
            hermite_case(quad_curves(1.0), &[1e-3; 4], 1.0, false, None),
        ),
        // both pairs retrace the same path: once pair (0, 1) marks both edges, the
        // remaining pairs are skipped as "both known coincident"
        (
            "hermite_retraced_sliver_skip",
            hermite_case(retraced_sliver(1.0), &[1e-3; 4], 1.0, true, None),
        ),
        // an exact-span curve kind (SmLine) as the control: its cache tree does not
        // depend on the edge tolerance, and the pair is still coincident
        (
            "line_sliver_skip",
            hermite_case(
                vec![
                    LoopCurveIn::Line {
                        a: [0.0, 0.0, 0.0],
                        b: [1.0, 0.0, 0.0],
                    },
                    LoopCurveIn::Line {
                        a: [1.0, 0.0, 0.0],
                        b: [0.0, 0.0, 0.0],
                    },
                ],
                &[1e-10, 1.0],
                1.0,
                true,
                None,
            ),
        ),
    ];
    cases
        .into_iter()
        .filter_map(|(name, i)| serde_json::to_value(i).ok().map(|v| (name.to_string(), v)))
        .collect()
}

/// The inline touching-face cases (see the module docs): each B-rep's faces intersected
/// pairwise in both orders, with and without points.
fn touching_cases() -> Vec<(String, Value)> {
    const ALL: ([f64; 2], [f64; 2]) = ([0.0, 0.0], [1.0, 1.0]);
    let sq = |h: f64| [-h, -h, h, h];
    let full = |r: f64, o: [f64; 3]| sphere(r, o, 0, ALL.0, ALL.1);
    let breps: Vec<(&str, Vec<FaceIn>)> = vec![
        // a sphere resting on a plane (a box face) at an interior point of both
        (
            "sphere_plane",
            vec![
                full(1.0, [0.0; 3]),
                plane([0.0, 1.0, 0.0], [0.0, 1.0, 0.0], sq(0.5)),
            ],
        ),
        (
            "sphere_plane_flip",
            vec![
                full(1.0, [0.0; 3]),
                plane([0.0, 1.0, 0.0], [0.0, -1.0, 0.0], sq(0.5)),
            ],
        ),
        // the touch point outside the plane face, on its boundary
        (
            "sphere_plane_outside",
            vec![
                full(1.0, [0.0; 3]),
                plane([0.0, 1.0, 0.0], [0.0, 1.0, 0.0], [0.2, 0.2, 0.7, 0.7]),
            ],
        ),
        (
            "sphere_plane_on_edge",
            vec![
                full(1.0, [0.0; 3]),
                plane([0.0, 1.0, 0.0], [0.0, 1.0, 0.0], [0.0, -0.5, 0.5, 0.5]),
            ],
        ),
        (
            "sphere_plane_on_corner",
            vec![
                full(1.0, [0.0; 3]),
                plane([0.0, 1.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.5, 0.5]),
            ],
        ),
        // the touch point outside the sphere face (a longitude band away from it)
        (
            "sphere_band_plane",
            vec![
                sphere(1.0, [0.0; 3], 0, [0.0, 0.0], [0.2, 1.0]),
                plane([0.0, 1.0, 0.0], [0.0, 1.0, 0.0], sq(0.5)),
            ],
        ),
        // on the sphere's seam edge and at its pole
        (
            "sphere_seam_plane",
            vec![
                full(1.0, [0.0; 3]),
                plane([1.0, 0.0, 0.0], [1.0, 0.0, 0.0], sq(0.5)),
            ],
        ),
        (
            "sphere_pole_plane",
            vec![
                full(1.0, [0.0; 3]),
                plane([0.0, 0.0, 1.0], [0.0, 0.0, 1.0], sq(0.5)),
            ],
        ),
        // a sphere placed along another axis, off the origin
        (
            "sphere_axis_x_plane",
            vec![
                sphere(1.0, [2.0, -1.0, 3.0], 1, ALL.0, ALL.1),
                plane([2.0, 0.0, 3.0], [0.0, 1.0, 0.0], sq(0.5)),
            ],
        ),
        (
            "sphere_axis_y_plane",
            vec![
                sphere(1.0, [2.0, -1.0, 3.0], 2, ALL.0, ALL.1),
                plane([3.0, -1.0, 3.0], [1.0, 0.0, 0.0], sq(0.5)),
            ],
        ),
        // gaps below, near and beyond the tolerances
        (
            "sphere_plane_gap_1e-9",
            vec![
                full(1.0, [0.0; 3]),
                plane([0.0, 1.0 + 1e-9, 0.0], [0.0, 1.0, 0.0], sq(0.5)),
            ],
        ),
        (
            "sphere_plane_gap_1e-6",
            vec![
                full(1.0, [0.0; 3]),
                plane([0.0, 1.0 + 1e-6, 0.0], [0.0, 1.0, 0.0], sq(0.5)),
            ],
        ),
        (
            "sphere_plane_gap_1e-3",
            vec![
                full(1.0, [0.0; 3]),
                plane([0.0, 1.0 + 1e-3, 0.0], [0.0, 1.0, 0.0], sq(0.5)),
            ],
        ),
        (
            "sphere_plane_overlap_1e-9",
            vec![
                full(1.0, [0.0; 3]),
                plane([0.0, 1.0 - 1e-9, 0.0], [0.0, 1.0, 0.0], sq(0.5)),
            ],
        ),
        // tiny and huge
        (
            "sphere_plane_tiny",
            vec![
                full(1e-3, [0.0; 3]),
                plane([0.0, 1e-3, 0.0], [0.0, 1.0, 0.0], sq(5e-4)),
            ],
        ),
        (
            "sphere_plane_huge",
            vec![
                full(100.0, [0.0; 3]),
                plane([0.0, 100.0, 0.0], [0.0, 1.0, 0.0], sq(50.0)),
            ],
        ),
        // two spheres, and a sphere on a cylinder's side
        (
            "sphere_sphere",
            vec![full(1.0, [0.0; 3]), full(0.5, [0.0, 1.5, 0.0])],
        ),
        (
            "sphere_sphere_gap",
            vec![full(1.0, [0.0; 3]), full(0.5, [0.0, 1.5 + 1e-7, 0.0])],
        ),
        (
            "cylinder_sphere",
            vec![
                FaceIn::Analytic {
                    shape: Shape::Cylinder,
                    scale: 1.0,
                    a: 1.0,
                    b: 0.0,
                    c: 2.0,
                    origin: [0.0; 3],
                    axis: 0,
                    from: ALL.0,
                    to: ALL.1,
                },
                full(0.5, [0.0, 1.5, 1.0]),
            ],
        ),
        // a cylinder lying on a plane (a line, not a point) as a control
        (
            "cylinder_plane_line",
            vec![
                FaceIn::Analytic {
                    shape: Shape::Cylinder,
                    scale: 1.0,
                    a: 1.0,
                    b: 0.0,
                    c: 2.0,
                    origin: [0.0; 3],
                    axis: 0,
                    from: ALL.0,
                    to: ALL.1,
                },
                plane([0.0, 1.0, 1.0], [0.0, 1.0, 0.0], sq(0.5)),
            ],
        ),
        // a torus touching a plane at its outer equator, and on its top circle (a curve)
        (
            "torus_plane_point",
            vec![
                FaceIn::Analytic {
                    shape: Shape::Torus,
                    scale: 1.0,
                    a: 2.0,
                    b: 0.5,
                    c: 0.0,
                    origin: [0.0; 3],
                    axis: 0,
                    from: ALL.0,
                    to: ALL.1,
                },
                plane([0.0, 2.5, 0.0], [0.0, 1.0, 0.0], sq(0.5)),
            ],
        ),
        (
            "torus_plane_circle",
            vec![
                FaceIn::Analytic {
                    shape: Shape::Torus,
                    scale: 1.0,
                    a: 2.0,
                    b: 0.5,
                    c: 0.0,
                    origin: [0.0; 3],
                    axis: 0,
                    from: ALL.0,
                    to: ALL.1,
                },
                plane([0.0, 0.0, 0.5], [0.0, 0.0, 1.0], sq(3.0)),
            ],
        ),
    ];
    let mut out = Vec::new();
    for (name, faces) in breps {
        for (face, other) in [(0, 1), (1, 0)] {
            for (approx_tol, ang_tol_deg, want_points) in
                [(1e-6, 5.0, true), (1e-4, 20.0, true), (1e-6, 5.0, false)]
            {
                let i = Inputs {
                    brep: CaseBrep::Faces(FacesBrep::Faces {
                        faces: faces.clone(),
                    }),
                    validate_only: false,
                    init_gaps: false,
                    vertex_loop: None,
                    op: OpIn::Intersect {
                        face,
                        other,
                        approx_tol,
                        ang_tol_deg,
                        coincidence: None,
                        want_points,
                    },
                };
                let tag = format!(
                    "touching_{name}_{face}{other}_tol{approx_tol:e}{}",
                    if want_points { "" } else { "_nopts" }
                );
                if let Ok(v) = serde_json::to_value(i) {
                    out.push((tag, v));
                }
            }
        }
    }
    out
}

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

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let mut v = default_cases(self, ctx)?;
        for (name, inputs) in touching_cases() {
            v.push(Case {
                unit: self.id().into(),
                name,
                source: CaseSource::Inline,
                inputs,
            });
        }
        for (name, inputs) in hermite_degenerate_cases() {
            v.push(Case {
                unit: self.id().into(),
                name,
                source: CaseSource::Inline,
                inputs,
            });
        }
        Ok(v)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        run_rust_case(&case.parse()?)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let brep = shared::gen_brep(rng);
        let vertex_loop = (matches!(brep, BrepIn::Face { .. }) && rng.chance(0.4))
            .then(|| [rng.uniform(0.1, 0.9), rng.uniform(0.1, 0.9)]);
        let i = Inputs {
            brep: CaseBrep::Shared(brep),
            vertex_loop,
            validate_only: rng.chance(0.1),
            init_gaps: rng.chance(0.3),
            op: gen_op(rng),
        };
        serde_json::to_value(i).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore::curve::line::Line;
    use solidcore::geom::extent1d::Extent1d;
    use solidcore::kernel::Kernel;
    use solidcore::math::{Point3, Vec3};
    use solidcore::nurbs::b_spline_curve::BSplineCurve;
    use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;

    #[test]
    fn own_uv_trim_copy_slices_line_to_its_b_spline_base() {
        let k = Kernel::default();
        let mut nurb = allocate_nurb_curve(1, 1, 3).unwrap();
        for (point, x) in nurb.cpts_mut().iter_mut().zip([-5.0, 5.0]) {
            point.x = x;
            point.y = -1.753_545_387_010_300_2;
            point.z = 0.0;
            point.w = 1.0;
        }
        nurb.knots_mut().copy_from_slice(&[0.0, 0.0, 1.0, 1.0]);
        let line = SmCurve::from(Line::from_parts(
            BSplineCurve::from_nurb(nurb, 1, false),
            Point3::new(-5.0, -1.753_545_387_010_300_2, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Extent1d::from_min_max(0.0, 1.0),
            10.0,
            false,
        ));
        let native_copy = native_bspline_copy(&line).unwrap();
        let parameter = 1.0 / 7.0;

        assert!(matches!(native_copy.kind(), CurveKind::BSpline(_)));
        let native_x = native_copy.evaluate_point(&k, parameter).unwrap().x;
        let analytic_x = line.evaluate_point(&k, parameter).unwrap().x;
        assert_eq!(native_x, -5.0 * (1.0 - parameter) + 5.0 * parameter);
        assert_ne!(native_x, analytic_x);
    }
}
