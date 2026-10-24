//! Runner for unit `heal.stitch_p1`. C++: `solidcore_ffi::units::heal_stitch_p1`.
//! Rust: `solidcore::heal::stitch::p1`.
//!
//! The C++ side builds a real `SmBrep` per call: an upstream `.smb` file, a wire edge,
//! a face or the `prog_test` wire network (the B-rep blocks of `topology.edgeuse_p1`),
//! unstitched sheets (one face per surface in one B-rep; generated as grids of
//! bilinear or biquadratic patches whose shared corners and edges are pulled apart by
//! a gap), or `SmApiCreateBox` (the upstream tests' box). The Rust side imports the
//! export of unit `topology.brep_p1` (the graph of `topology.edgeuse_p1` plus list
//! orders and vertex use lists) into a [`Brep`]. Every op's C++ output after the build
//! status is compared with the Rust side's as one float array (`out`): statuses (`0`,
//! an `SmStatus` code), traversal indices, flags and values; `-2` (the shim's guard on
//! input upstream would dereference null or free twice) is an error on both sides.
//!
//! Selectors are taken modulo the entity count; an op needing an entity the B-rep
//! lacks is rejected (nothing runs on either side). Edge parameters are given normalised to the
//! edge's interval and split points as an offset from the edge, resolved on the
//! imported B-rep (identically for both sides).
//!
//! **Inexact geometry.** The export carries B-spline curves, `SmLine`s, B-spline
//! surfaces, `SmPlane`s and `SmSurfOfRevolution`s exactly; other analytic kinds arrive
//! as their B-spline part,
//! so the ops evaluating geometry defer on B-reps holding them (as `topology.brep_p1`).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use solidcore::heal::stitch::p1 as st;
use solidcore::kernel::Kernel;
use solidcore::math::Point3;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::face::p2 as fp2;
use solidcore::topology::{edge, vertex, Brep, EdgeId, FaceId, VertexId};
use solidcore::{EntityRef, SolidError};
use solidcore_ffi::units::heal_stitch_p1::{self as ffi, Op, TY_EDGE, TY_FACE, TY_VERTEX};
use solidcore_ffi::units::topology_brep_p1::counts;

use super::topology_edgeuse_p1::{
    brep_block_of, flag, gen_brep, path_of, push_surface, sel, BrepIn, ImportTypes, SrfIn,
};
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The difftest runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// "Upstream would dereference null or free twice" in a status slot.
pub(crate) const NULL_DEREF: f64 = -2.0;

/// `SM_ERR_INVALID_INPUT`.
const SM_ERR_INVALID_INPUT: f64 = 1007.0;

/// `SmBSplineSurface_TYPE`, `SmPlane_TYPE`; `SmBSplineCurve_TYPE`, `SmLine_TYPE`.
const EXACT_SURFACES: [i64; 2] = [15002, 15011];
const EXACT_CURVES: [i64; 2] = [14033, 14010];

/// The deferral label of B-reps whose analytic geometry the shared export flattens to
/// B-spline parts (a harness limitation, as in `topology.brep_p1`).
const HARNESS_ANALYTIC_IMPORT: &str = "harness:analytic-geometry-export";

/// Largest `SmApi*` selector (`which` of [`OpIn::Api`]).
const MAX_API: u8 = 6;

// ---- inputs -----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SheetFace {
    pub(crate) surface: SrfIn,
    pub(crate) dom: [f64; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum StitchBrep {
    /// A B-rep block of `topology.edgeuse_p1`.
    Brep {
        brep: BrepIn,
        #[serde(default)]
        validate_only: bool,
        #[serde(default)]
        init_gaps: bool,
    },
    /// One face per surface in one B-rep.
    Sheets { faces: Vec<SheetFace> },
    /// `SmApiCreateBox(p, size[0], size[1], size[2])`.
    Box { p: [f64; 3], size: [f64; 3] },
}

/// A target: `ty` 0 vertex, 1 edge, 2 face; `sel` a selector.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Target {
    ty: u8,
    sel: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Export,
    Select {
        a: Target,
        b: Target,
        dist: f64,
    },
    Interior {
        edge: i64,
        s: f64,
        tol: f64,
    },
    Split {
        edge: i64,
        s: f64,
        offset: [f64; 3],
        tol: f64,
    },
    VPairs {
        tol: f64,
        #[serde(default)]
        ignore: bool,
        l1: Vec<i64>,
        l2: Vec<i64>,
    },
    Edges {
        tol: f64,
        manifold: bool,
        #[serde(default)]
        fast: bool,
        #[serde(default)]
        ignore: bool,
        #[serde(default)]
        nesting: bool,
        verts: Vec<i64>,
    },
    Simple {
        api: bool,
        tol: f64,
        keep: Vec<i64>,
        del: Vec<i64>,
    },
    Unify {
        api: bool,
        tol: f64,
        face: i64,
        n: u64,
    },
    /// `which`: 0 `SmApiStitch(brep)`, 1 `SmApiStitch(brep, face)`, 2
    /// `SmApiStitchIntoSolid`, 3 `SmApiStitchIntoShell`, 4 `SmApiUnifyNormals`, 5
    /// `SmApiSimpleFaceStitch` (empty lists), 6 `SmApiStitchAdvanced`.
    Api {
        which: u8,
        #[serde(default)]
        null_brep: bool,
        /// selector, `-1` null
        #[serde(default)]
        face: i64,
        #[serde(default)]
        n: u64,
        #[serde(default)]
        well_formed: bool,
        #[serde(default)]
        ratio: f64,
        #[serde(default)]
        tol: f64,
        #[serde(default)]
        flags: [bool; 4],
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    brep: StitchBrep,
    op: OpIn,
}

pub(crate) fn block_of(b: &StitchBrep) -> Result<Vec<f64>, HarnessError> {
    let mut v = Vec::new();
    match b {
        StitchBrep::Brep {
            brep,
            validate_only,
            init_gaps,
        } => {
            v.push(0.0);
            v.extend(brep_block_of(Some(brep), *validate_only, *init_gaps)?);
        }
        StitchBrep::Sheets { faces } => {
            v.push(1.0);
            v.push(faces.len() as f64);
            for f in faces {
                push_surface(&mut v, &f.surface)?;
                v.extend(f.dom);
            }
        }
        StitchBrep::Box { p, size } => {
            v.push(2.0);
            v.extend(p);
            v.extend(size);
        }
    }
    Ok(v)
}

pub(crate) fn path(b: &StitchBrep) -> Result<String, HarnessError> {
    match b {
        StitchBrep::Brep { brep, .. } => path_of(Some(brep)),
        _ => Ok(String::new()),
    }
}

/// The C++ export of the case's B-rep.
pub(crate) fn export(b: &StitchBrep) -> Result<Vec<f64>, HarnessError> {
    let (path, block) = (path(b)?, block_of(b)?);
    crate::export_memo::export("heal_stitch_p1", &[&path], &block, || {
        ffi::eval(Op::Export, &path, &block)
    })
    .map_err(|e| harness(format!("c++ export: {e}")))
}

pub(crate) fn build_failure(export: &[f64]) -> Output {
    Output::err(format!(
        "brep_build_status_{}",
        export.get(1).copied().unwrap_or(0.0)
    ))
}

/// An op naming an entity kind the B-rep lacks (e.g. a face op on a wire B-rep): no code
/// runs on either side, so the case is rejected (not counted as a pass).
pub(crate) fn no_entity() -> Output {
    Output::rejected("no entity of the op's kind in this B-rep: nothing runs on either side")
}

// ---- resolution ---------------------------------------------------------------------------

/// An op with its selectors resolved and its parameters made concrete on the B-rep.
#[derive(Debug, Clone)]
enum Resolved {
    Export,
    Select {
        a: EntityRef,
        b: EntityRef,
        dist: f64,
    },
    Interior {
        edge: EdgeId,
        t: f64,
        tol: f64,
    },
    Split {
        edge: EdgeId,
        p: Point3,
        tol: f64,
    },
    VPairs {
        tol: f64,
        ignore: bool,
        l1: Vec<VertexId>,
        l2: Vec<VertexId>,
    },
    Edges {
        tol: f64,
        flags: [bool; 4],
        verts: Vec<VertexId>,
    },
    Simple {
        api: bool,
        tol: f64,
        keep: Vec<FaceId>,
        del: Vec<FaceId>,
    },
    Unify {
        api: bool,
        tol: f64,
        face: FaceId,
        n: u64,
    },
    Api {
        which: u8,
        null_brep: bool,
        face: Option<FaceId>,
        n: u64,
        well_formed: bool,
        ratio: f64,
        tol: f64,
        flags: [bool; 4],
    },
}

pub(crate) fn pick(s: i64, n: usize) -> Option<u32> {
    u32::try_from(sel(s, n)).ok()
}

fn target(t: Target, b: &Brep) -> Option<EntityRef> {
    match t.ty {
        0 => pick(t.sel, b.vertices().len()).map(|i| EntityRef::Vertex(VertexId::from_index(i))),
        1 => pick(t.sel, b.edges().len()).map(|i| EntityRef::Edge(EdgeId::from_index(i))),
        2 => pick(t.sel, b.faces().len()).map(|i| EntityRef::Face(FaceId::from_index(i))),
        _ => None,
    }
}

/// Distinct vertex ids of the selectors (repeats dropped), skipping those in `not`.
fn vertex_list(s: &[i64], b: &Brep, not: &[VertexId]) -> Vec<VertexId> {
    let mut v = Vec::new();
    for &x in s {
        if let Some(i) = pick(x, b.vertices().len()) {
            let id = VertexId::from_index(i);
            if !v.contains(&id) && !not.contains(&id) {
                v.push(id);
            }
        }
    }
    v
}

fn face_list(s: &[i64], b: &Brep) -> Vec<FaceId> {
    s.iter()
        .filter_map(|&x| pick(x, b.faces().len()).map(FaceId::from_index))
        .collect()
}

/// The parameter at normalised position `s` of the edge's interval.
fn param(b: &Brep, e: EdgeId, s: f64) -> Result<f64, HarnessError> {
    let ivl = edge::interval(b, e).map_err(|e| harness(e.to_string()))?;
    Ok(ivl.min() + (ivl.max() - ivl.min()) * s)
}

fn resolve(k: &Kernel, op: &OpIn, b: &Brep) -> Result<Option<Resolved>, HarnessError> {
    let ne = b.edges().len();
    Ok(Some(match op {
        OpIn::Export => Resolved::Export,
        OpIn::Select { a, b: tb, dist } => {
            let (Some(a), Some(c)) = (target(*a, b), target(*tb, b)) else {
                return Ok(None);
            };
            Resolved::Select {
                a,
                b: c,
                dist: *dist,
            }
        }
        OpIn::Interior { edge, s, tol } => {
            let Some(e) = pick(*edge, ne).map(EdgeId::from_index) else {
                return Ok(None);
            };
            Resolved::Interior {
                edge: e,
                t: param(b, e, *s)?,
                tol: *tol,
            }
        }
        OpIn::Split {
            edge,
            s,
            offset,
            tol,
        } => {
            let Some(e) = pick(*edge, ne).map(EdgeId::from_index) else {
                return Ok(None);
            };
            let t = param(b, e, *s)?;
            let c = edge::curve(b, e)
                .map_err(|e| harness(e.to_string()))?
                .ok_or_else(|| harness("edge without curve"))?;
            let on = c
                .evaluate_point(k, t)
                .map_err(|e| harness(format!("split point: {e}")))?;
            let p = Point3::new(on.x + offset[0], on.y + offset[1], on.z + offset[2]);
            Resolved::Split {
                edge: e,
                p,
                tol: *tol,
            }
        }
        OpIn::VPairs {
            tol,
            ignore,
            l1,
            l2,
        } => {
            if b.vertices().is_empty() {
                return Ok(None);
            }
            let a = vertex_list(l1, b, &[]);
            let c = vertex_list(l2, b, &a);
            Resolved::VPairs {
                tol: *tol,
                ignore: *ignore,
                l1: a,
                l2: c,
            }
        }
        OpIn::Edges {
            tol,
            manifold,
            fast,
            ignore,
            nesting,
            verts,
        } => {
            if b.vertices().is_empty() {
                return Ok(None);
            }
            let v = verts
                .iter()
                .filter_map(|&x| pick(x, b.vertices().len()).map(VertexId::from_index))
                .collect();
            Resolved::Edges {
                tol: *tol,
                flags: [*manifold, *fast, *ignore, *nesting],
                verts: v,
            }
        }
        OpIn::Simple {
            api,
            tol,
            keep,
            del,
        } => {
            if b.faces().is_empty() {
                return Ok(None);
            }
            Resolved::Simple {
                api: *api,
                tol: *tol,
                keep: face_list(keep, b),
                del: face_list(del, b),
            }
        }
        OpIn::Unify { api, tol, face, n } => {
            let Some(f) = pick(*face, b.faces().len()).map(FaceId::from_index) else {
                return Ok(None);
            };
            Resolved::Unify {
                api: *api,
                tol: *tol,
                face: f,
                n: (*n).min(64),
            }
        }
        OpIn::Api {
            which,
            null_brep,
            face,
            n,
            well_formed,
            ratio,
            tol,
            flags,
        } => {
            if *which > MAX_API {
                return Err(harness(format!("unknown SmApi selector {which}")));
            }
            // SmApiStitch(brep, face) crashes upstream for any face with a real B-rep
            // (SmMerge(ctx, brep, NULL) dereferences the null other B-rep, UB-167): only
            // the null face is defined there (the FFI refuses the rest)
            let f = if *face < 0 || (*which == 1 && !*null_brep) {
                None
            } else {
                pick(*face, b.faces().len()).map(FaceId::from_index)
            };
            Resolved::Api {
                which: *which,
                null_brep: *null_brep,
                face: f,
                n: (*n).min(64),
                well_formed: *well_formed,
                ratio: *ratio,
                tol: *tol,
                flags: *flags,
            }
        }
    }))
}

fn idx<I: solidcore::topology::ArenaId>(id: I) -> f64 {
    f64::from(id.index())
}

fn ty_code(e: EntityRef) -> (f64, f64) {
    match e {
        EntityRef::Vertex(v) => (TY_VERTEX as f64, idx(v)),
        EntityRef::Edge(x) => (TY_EDGE as f64, idx(x)),
        EntityRef::Face(f) => (TY_FACE as f64, idx(f)),
        _ => (-1.0, -1.0),
    }
}

fn push_ids<I: solidcore::topology::ArenaId + Copy>(v: &mut Vec<f64>, ids: &[I]) {
    v.push(ids.len() as f64);
    v.extend(ids.iter().map(|&i| idx(i)));
}

fn args(op: &Resolved) -> (Op, Vec<f64>) {
    let mut v = Vec::new();
    let code = match op {
        Resolved::Export => Op::Export,
        Resolved::Select { a, b, dist } => {
            let (t1, i1) = ty_code(*a);
            let (t2, i2) = ty_code(*b);
            v.extend([t1, i1, t2, i2, *dist]);
            Op::Select
        }
        Resolved::Interior { edge, t, tol } => {
            v.extend([idx(*edge), *t, *tol]);
            Op::Interior
        }
        Resolved::Split { edge, p, tol } => {
            v.extend([idx(*edge), p.x, p.y, p.z, *tol]);
            Op::Split
        }
        Resolved::VPairs {
            tol,
            ignore,
            l1,
            l2,
        } => {
            v.extend([*tol, flag(*ignore)]);
            push_ids(&mut v, l1);
            push_ids(&mut v, l2);
            Op::VPairs
        }
        Resolved::Edges { tol, flags, verts } => {
            v.push(*tol);
            v.extend(flags.iter().map(|&f| flag(f)));
            push_ids(&mut v, verts);
            Op::Edges
        }
        Resolved::Simple {
            api,
            tol,
            keep,
            del,
        } => {
            v.extend([flag(*api), *tol]);
            push_ids(&mut v, keep);
            push_ids(&mut v, del);
            Op::Simple
        }
        Resolved::Unify { api, tol, face, n } => {
            v.extend([flag(*api), *tol, idx(*face), *n as f64]);
            Op::Unify
        }
        Resolved::Api {
            which,
            null_brep,
            face,
            n,
            well_formed,
            ratio,
            tol,
            flags,
        } => {
            v.extend([f64::from(*which), flag(*null_brep)]);
            let face1 = face.map_or(0.0, |f| idx(f) + 1.0);
            match which {
                1 => v.push(face1),
                3 => v.extend([flag(*well_formed), *ratio]),
                4 => v.extend([face1, *n as f64]),
                6 => {
                    v.push(*tol);
                    v.extend(flags.iter().map(|&f| flag(f)));
                }
                _ => {}
            }
            Op::Api
        }
    };
    (code, v)
}

/// The deferral of an op evaluating geometry on a B-rep with inexact geometry.
pub(crate) fn inexact(types: &ImportTypes) -> Option<Output> {
    if let Some(t) = (0..types.face_surface_types.len())
        .filter_map(|f| types.inexact_surface(f))
        .find(|t| !EXACT_SURFACES.contains(t))
    {
        return Some(Output::deferred(
            HARNESS_ANALYTIC_IMPORT,
            format!(
                "{} face surface exported as its B-spline part",
                super::topology_edgeuse_p1::surface_class_name(t)
            ),
        ));
    }
    if (0..types.edge_curve_types.len())
        .filter_map(|e| types.inexact_curve(e))
        .any(|t| t >= 0 && !EXACT_CURVES.contains(&t))
    {
        return Some(Output::deferred(
            HARNESS_ANALYTIC_IMPORT,
            "analytic edge curve exported as its B-spline part",
        ));
    }
    None
}

/// Whether the op evaluates curves or surfaces (and so defers on inexact geometry).
fn evaluates_geometry(op: &Resolved) -> bool {
    match op {
        Resolved::Export | Resolved::VPairs { .. } => false,
        Resolved::Api {
            null_brep, which, ..
        } => !*null_brep && *which != 5,
        _ => true,
    }
}

// ---- C++ side ---------------------------------------------------------------------------

fn guarded_output(o: Vec<f64>) -> Output {
    if o.first() == Some(&NULL_DEREF) {
        Output::err("null_dereference_guard")
    } else {
        Output::new().values("out", o)
    }
}

fn run_cpp(k: &Kernel, i: &Inputs) -> Result<Output, HarnessError> {
    let ex = export(&i.brep)?;
    let Some((_, graph_at)) = counts(&ex) else {
        return Ok(build_failure(&ex));
    };
    let (b, types) = super::topology_brep_p1::import_raw(&ex, graph_at)?;
    let Some(op) = resolve(k, &i.op, &b)? else {
        return Ok(no_entity());
    };
    if evaluates_geometry(&op) {
        if let Some(d) = inexact(&types) {
            return Ok(d);
        }
    }
    let (code, a) = args(&op);
    let mut input = block_of(&i.brep)?;
    input.extend(a);
    let out = ffi::eval(code, &path(&i.brep)?, &input).map_err(|e| harness(format!("c++: {e}")))?;
    let body = out.get(1..).ok_or_else(|| harness("c++: empty output"))?;
    Ok(guarded_output(body.to_vec()))
}

// ---- Rust side --------------------------------------------------------------------------

/// The `SmStatus` of a Rust error: an upstream code as is; `InvalidInput` raised by the
/// port itself marks a null dereference upstream (`-2`), from the API entry points or a
/// callee it is `SM_ERR_INVALID_INPUT`.
fn status_of(e: &SolidError) -> f64 {
    match e {
        SolidError::Upstream { code, .. } => f64::from(*code),
        SolidError::InvalidInput { operation, .. } if *operation == "heal::stitch::p1" => {
            NULL_DEREF
        }
        SolidError::InvalidInput { .. } => SM_ERR_INVALID_INPUT,
        _ => -9.0,
    }
}

/// `Ok(values)` on success; `Err` propagates `Unported`, any other error becomes its
/// status.
fn status_or<T>(
    r: solidcore::Result<T>,
    f: impl FnOnce(T) -> Vec<f64>,
) -> solidcore::Result<Vec<f64>> {
    match r {
        Ok(v) => {
            let mut o = vec![0.0];
            o.extend(f(v));
            Ok(o)
        }
        Err(e) if e.is_unported() => Err(e),
        Err(e) => Ok(vec![status_of(&e)]),
    }
}

fn ids_out<I: solidcore::topology::ArenaId + Copy>(ids: &[I]) -> Vec<f64> {
    let mut v = Vec::new();
    push_ids(&mut v, ids);
    v
}

fn opt_ids_out(ids: &[Option<VertexId>]) -> Vec<f64> {
    let mut v = vec![ids.len() as f64];
    v.extend(ids.iter().map(|i| i.map_or(-1.0, idx)));
    v
}

/// The B-rep after a mutating op (as the shim's `put_digest`).
pub(crate) fn digest(b: &Brep, o: &mut Vec<f64>) -> solidcore::Result<()> {
    let fs = bp1::faces(b, None)?;
    let es = bp1::edges(b, None)?;
    let vs = bp1::vertices(b, None)?;
    o.extend([fs.len() as f64, es.len() as f64, vs.len() as f64]);
    for v in vs {
        let p = vertex::point(b, v)?;
        o.extend([p.x, p.y, p.z]);
    }
    Ok(())
}

#[allow(clippy::too_many_lines)] // one arm per op
fn run_rust_op(k: &mut Kernel, b: &mut Brep, op: &Resolved) -> solidcore::Result<Vec<f64>> {
    let mut cb = st::DefaultStitchCallback::new();
    match op {
        Resolved::Export => Err(SolidError::invalid_input(
            "difftest",
            "export has no Rust op",
        )),
        Resolved::Select { a, b: c, dist } => {
            if matches!((a, c), (EntityRef::Edge(_), EntityRef::Edge(_))) {
                for f in b.faces().ids().collect::<Vec<_>>() {
                    if fp2::surface(b, f)?.is_none() {
                        return Ok(vec![NULL_DEREF]);
                    }
                }
            }
            status_or(
                st::select_topology_to_keep(&mut cb, k, b, *a, *c, *dist),
                |r| vec![flag(r.keep_first), r.max_distance],
            )
        }
        Resolved::Interior { edge, t, tol } => {
            status_or(st::check_interior_edge(k, b, *edge, *t, *tol), |r| {
                vec![flag(r)]
            })
        }
        Resolved::Split { edge, p, tol } => match st::test_and_split_edge(k, b, *edge, *p, *tol) {
            Ok(r) => {
                let mut o = vec![
                    0.0,
                    f64::from(r.reason as u8),
                    r.dist_to_edge,
                    flag(r.new_vertex.is_some()),
                ];
                if r.new_vertex.is_some() {
                    digest(b, &mut o)?;
                }
                Ok(o)
            }
            Err(e) if e.is_unported() => Err(e),
            Err(e) => Ok(vec![status_of(&e)]),
        },
        Resolved::VPairs {
            tol,
            ignore,
            l1,
            l2,
        } => {
            let mut s = st::Stitch::new(&mut cb, *tol, true, true);
            s.ignore_problems = *ignore;
            let mut a: Vec<Option<VertexId>> = l1.iter().copied().map(Some).collect();
            let mut c: Vec<Option<VertexId>> = l2.iter().copied().map(Some).collect();
            match s.stitch_vertex_pairs(k, b, &mut a, &mut c) {
                Ok(r) => {
                    let mut o = vec![0.0, r.max_vertex_gap, r.min_unstitched_vert_gap];
                    o.extend(ids_out(&r.processed));
                    o.extend(ids_out(&r.deleted));
                    o.extend(opt_ids_out(&a));
                    o.extend(opt_ids_out(&c));
                    if !r.processed.is_empty() {
                        digest(b, &mut o)?;
                    }
                    Ok(o)
                }
                Err(e) if e.is_unported() => Err(e),
                Err(e) => Ok(vec![status_of(&e)]),
            }
        }
        Resolved::Edges { tol, flags, verts } => {
            let [manifold, fast, ignore, nesting] = *flags;
            let mut s = st::Stitch::new(&mut cb, *tol, nesting, true);
            s.making_manifold_solid = manifold;
            s.fast_edge_compare = fast;
            s.ignore_problems = ignore;
            let v: Vec<Option<VertexId>> = verts.iter().copied().map(Some).collect();
            match s.stitch_edges_of_vertices(k, b, &v) {
                Ok(r) => {
                    let mut o = vec![0.0, r.max_edge_gap, r.min_unstitched_edge_gap];
                    o.extend(ids_out(&r.glued));
                    o.extend(ids_out(&r.deleted));
                    if !r.glued.is_empty() {
                        digest(b, &mut o)?;
                    }
                    Ok(o)
                }
                Err(e) if e.is_unported() => Err(e),
                Err(e) => Ok(vec![status_of(&e)]),
            }
        }
        Resolved::Simple {
            api,
            tol,
            keep,
            del,
        } => {
            let r = if *api {
                st::api_simple_face_stitch(k, Some(b), keep, del)
            } else {
                st::Stitch::new(&mut cb, *tol, true, true).do_simple_face_stitch(k, b, keep, del)
            };
            match r {
                Ok(r) => {
                    let mut o = vec![
                        0.0,
                        r.max_vertex_gap,
                        r.max_edge_gap,
                        r.stitched_edges as f64,
                    ];
                    digest(b, &mut o)?;
                    Ok(o)
                }
                Err(e) if e.is_unported() => Err(e),
                Err(e) => Ok(vec![status_of(&e)]),
            }
        }
        Resolved::Unify { api, tol, face, n } => {
            let r = if *api {
                st::api_unify_normals(k, Some(b), Some(*face), *n)
            } else {
                st::Stitch::new(&mut cb, *tol, true, true).unify_normals(k, b, *face, *n)
            };
            match r {
                Ok(flipped) => {
                    let mut o = vec![0.0];
                    o.extend(ids_out(&flipped));
                    for f in &flipped {
                        let d = fp2::uv_domain(b, *f)?;
                        o.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
                    }
                    Ok(o)
                }
                Err(e) if e.is_unported() => Err(e),
                Err(e) => Ok(vec![status_of(&e)]),
            }
        }
        Resolved::Api {
            which,
            null_brep,
            face,
            n,
            well_formed,
            ratio,
            tol,
            flags,
        } => {
            let brep = if *null_brep { None } else { Some(b) };
            match which {
                0 => status_or(st::api_stitch(k, brep), |()| Vec::new()),
                1 => status_or(st::api_stitch_face(k, brep, *face), |()| Vec::new()),
                2 => status_or(st::api_stitch_into_solid(k, brep), |r| {
                    vec![
                        flag(r.produces_a_solid),
                        r.stitched_edges as f64,
                        r.max_vertex_gap,
                        r.max_edge_gap,
                    ]
                }),
                3 => status_or(
                    st::api_stitch_into_shell(k, brep, *well_formed, *ratio),
                    |r| vec![r.stitched_edges as f64, r.max_vertex_gap, r.max_edge_gap],
                ),
                4 => status_or(st::api_unify_normals(k, brep, *face, *n), |f| ids_out(&f)),
                5 => status_or(st::api_simple_face_stitch(k, brep, &[], &[]), |_| {
                    Vec::new()
                }),
                _ => {
                    let [squeeze_small_edges, split_edges_with_vertices, making_manifold_solid, remove_laminar_slivers] =
                        *flags;
                    let options = st::AdvancedStitchOptions {
                        squeeze_small_edges,
                        split_edges_with_vertices,
                        making_manifold_solid,
                        remove_laminar_slivers,
                    };
                    status_or(st::api_stitch_advanced(k, brep, *tol, options), |r| {
                        vec![
                            r.stitched_edges as f64,
                            r.lamina_edges as f64,
                            r.max_vertex_gap,
                            r.max_edge_gap,
                        ]
                    })
                }
            }
        }
    }
}

fn run_rust(k: &Kernel, i: &Inputs) -> Result<Output, HarnessError> {
    let ex = export(&i.brep)?;
    let Some((_, graph_at)) = counts(&ex) else {
        return Ok(build_failure(&ex));
    };
    let (mut b, types) = super::topology_brep_p1::import(&ex, graph_at)?;
    let Some(op) = resolve(k, &i.op, &b)? else {
        return Ok(no_entity());
    };
    if evaluates_geometry(&op) {
        if let Some(d) = inexact(&types) {
            return Ok(d);
        }
    }
    if matches!(op, Resolved::Export) {
        return Ok(Output::new().values("out", Vec::new()));
    }
    let mut k = Kernel::default();
    match run_rust_op(&mut k, &mut b, &op) {
        Ok(o) => Ok(guarded_output(o)),
        Err(e) => match Output::unported(&e) {
            Some(o) => Ok(o),
            None => Err(harness(format!("rust: {e}"))),
        },
    }
}

// ---- generated cases --------------------------------------------------------------------

/// `w = 1` marker of a non-rational control point (`topology.edgeuse_p1`'s `NOW`).
const NOW: f64 = 1.0;

/// A grid of `nx * ny` patches of degree `p` (1 or 2) over cells of size `h`, each face
/// pulled apart from its neighbours by its own random offsets of size `gap` at every
/// control point (0 for some faces), some faces reversed in u.
pub(crate) fn gen_sheets(rng: &mut Rng) -> StitchBrep {
    let h = *rng.choose(&[1e-3, 1.0, 1.0, 1.0, 1e3]).unwrap_or(&1.0);
    let nx = 1 + rng.below(3) as usize;
    let ny = 1 + rng.below(if nx == 3 { 2 } else { 3 }) as usize;
    let p = 1 + rng.below(2) as usize;
    let gaps = [0.0, 0.0, 1e-9, 1e-7, 1e-5, 1e-4, 1e-3, 1e-2, 0.1];
    // base control net over the whole grid
    let (gx, gy) = (nx * p + 1, ny * p + 1);
    let mut base = vec![vec![[0.0; 3]; gy]; gx];
    let bump = rng.uniform(0.0, 0.5);
    for (i, col) in base.iter_mut().enumerate() {
        for (j, pt) in col.iter_mut().enumerate() {
            let (x, y) = (i as f64 / p as f64, j as f64 / p as f64);
            *pt = [x * h, y * h, bump * h * rng.uniform(-0.5, 0.5)];
        }
    }
    let mut faces = Vec::new();
    for cx in 0..nx {
        for cy in 0..ny {
            let gap = *rng.choose(&gaps).unwrap_or(&0.0) * h;
            let reverse = rng.chance(0.25);
            let mut net: Vec<Vec<[f64; 4]>> = Vec::new();
            for a in 0..=p {
                let mut row = Vec::new();
                for c in 0..=p {
                    let q = base
                        .get(cx * p + a)
                        .and_then(|col| col.get(cy * p + c))
                        .copied()
                        .unwrap_or([0.0; 3]);
                    let d = rng.point3(-1.0, 1.0);
                    row.push([q[0] + gap * d[0], q[1] + gap * d[1], q[2] + gap * d[2], NOW]);
                }
                net.push(row);
            }
            if reverse {
                net.reverse();
            }
            let mut knots = vec![0.0; p + 1];
            knots.extend(vec![1.0; p + 1]);
            let surface: SrfIn = match serde_json::from_value(json!({"bspline": {
                "net": net, "p": p, "q": p, "u": knots, "v": knots,
            }})) {
                Ok(s) => s,
                Err(_) => continue,
            };
            faces.push(SheetFace {
                surface,
                dom: [0.0, 0.0, 1.0, 1.0],
            });
        }
    }
    StitchBrep::Sheets { faces }
}

pub(crate) fn gen_tol(rng: &mut Rng) -> f64 {
    *rng.choose(&[0.0, 1e-9, 1e-6, 1e-5, 1e-4, 1e-3, 1e-2, 0.1, 1.0, 10.0])
        .unwrap_or(&1e-3)
}

pub(crate) fn gen_sels(rng: &mut Rng, max: u64) -> Vec<i64> {
    let n = rng.below(max + 1);
    (0..n).map(|_| rng.below(1 << 20) as i64).collect()
}

fn gen_op(rng: &mut Rng) -> OpIn {
    let s = |rng: &mut Rng| rng.below(1 << 20) as i64;
    let ss = [
        -0.1, 0.0, 1e-13, 1e-3, 0.01, 0.3, 0.5, 0.97, 0.999, 1.0, 1.1,
    ];
    match rng.below(20) {
        0..=3 => {
            let ty = u8::from(rng.chance(0.5));
            let ty2 = if rng.chance(0.9) {
                ty
            } else {
                rng.below(3) as u8
            };
            OpIn::Select {
                a: Target { ty, sel: s(rng) },
                b: Target {
                    ty: ty2,
                    sel: s(rng),
                },
                dist: *rng
                    .choose(&[0.0, 1e-6, 5e-5, 1e-4, 1e-3, 1e-2, 0.1, 1.0])
                    .unwrap_or(&1e-3),
            }
        }
        4 | 5 => OpIn::Interior {
            edge: s(rng),
            s: *rng.choose(&ss).unwrap_or(&0.5),
            tol: gen_tol(rng),
        },
        6 | 7 => {
            let m = *rng.choose(&[0.0, 1e-7, 1e-4, 1e-2, 0.3]).unwrap_or(&0.0);
            let d = rng.point3(-1.0, 1.0);
            OpIn::Split {
                edge: s(rng),
                s: *rng.choose(&ss).unwrap_or(&0.5),
                offset: [d[0] * m, d[1] * m, d[2] * m],
                tol: gen_tol(rng),
            }
        }
        8..=11 => OpIn::VPairs {
            tol: gen_tol(rng),
            ignore: rng.chance(0.2),
            l1: gen_sels(rng, 12),
            l2: gen_sels(rng, 12),
        },
        12..=14 => OpIn::Edges {
            tol: gen_tol(rng),
            manifold: rng.chance(0.7),
            fast: rng.chance(0.3),
            ignore: rng.chance(0.2),
            nesting: rng.chance(0.5),
            verts: gen_sels(rng, 8),
        },
        15 => OpIn::Simple {
            api: rng.chance(0.3),
            tol: gen_tol(rng),
            keep: gen_sels(rng, 3),
            del: gen_sels(rng, 3),
        },
        16 | 17 => OpIn::Unify {
            api: rng.chance(0.3),
            tol: gen_tol(rng),
            face: s(rng),
            n: *rng.choose(&[0, 1, 2, 3, 10]).unwrap_or(&3),
        },
        _ => OpIn::Api {
            which: rng.below(u64::from(MAX_API) + 1) as u8,
            null_brep: rng.chance(0.3),
            face: if rng.chance(0.2) { -1 } else { s(rng) },
            n: *rng.choose(&[0, 3, 10]).unwrap_or(&3),
            well_formed: rng.chance(0.5),
            ratio: *rng.choose(&[0.0, 1.0, 10.0, 100.0]).unwrap_or(&10.0),
            tol: gen_tol(rng),
            flags: [
                rng.chance(0.5),
                rng.chance(0.5),
                rng.chance(0.5),
                rng.chance(0.5),
            ],
        },
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "heal.stitch_p1"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let k = &Kernel::default();
        run_cpp(k, &case.parse::<Inputs>()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let k = &Kernel::default();
        run_rust(k, &case.parse::<Inputs>()?)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let brep = match rng.below(10) {
            0..=6 => gen_sheets(rng),
            7 => StitchBrep::Box {
                p: rng.point3(-5.0, 5.0),
                size: [
                    rng.uniform(0.5, 20.0),
                    rng.uniform(0.5, 20.0),
                    rng.uniform(0.5, 20.0),
                ],
            },
            _ => StitchBrep::Brep {
                brep: gen_brep(rng),
                validate_only: false,
                init_gaps: false,
            },
        };
        serde_json::to_value(Inputs {
            brep,
            op: gen_op(rng),
        })
        .ok()
    }
}
