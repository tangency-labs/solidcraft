//! Runner for unit `heal.stitch_p2`. C++: `solidcore_ffi::units::heal_stitch_p2`.
//! Rust: `solidcore::heal::stitch::p2`.
//!
//! The B-reps are those of unit `heal.stitch_p1` (an upstream `.smb` file or another
//! B-rep block of `topology.edgeuse_p1`, unstitched sheets, or `SmApiCreateBox`), built
//! by the C++ side and, for the Rust side, by the Rust kernel (`heal.stitch_p1`'s
//! `own_brep`: the Rust `.smb` reader and constructors, renumbered into the shim's
//! traversal order, so no C++ export reaches it). Every op's C++ output after the build status is
//! compared with the Rust side's as one float array (`out`): statuses, traversal
//! indices (`-3` for an entity the op created), flags, values and the B-rep digest
//! after the op. `-2` (upstream would dereference null) and `-3` (upstream would
//! never return) in a status slot are guards, an error on both sides.
//!
//! Selectors are taken modulo the entity count (`-1` is a null list entry); an op
//! needing a vertex the B-rep lacks is rejected (nothing runs on either side). Both
//! sides resolve the selectors on the Rust build.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use solidcore::heal::stitch::p1 as st1;
use solidcore::heal::stitch::p2 as st;
use solidcore::kernel::Kernel;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::{vertex, ArenaId, Brep, EdgeId, VertexId};
use solidcore::SolidError;
use solidcore_ffi::units::heal_stitch_p2::{self as ffi, Op};
use solidcore_ffi::units::topology_brep_p1::counts;

use super::topology_edgeuse_p1::{
    brep_block_of, flag, gen_brep, path_of, push_surface, sel, BrepIn, SrfIn,
};
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The difftest runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// "Upstream would dereference null or free twice" in a status slot.
const NULL_DEREF: f64 = -2.0;

/// "Upstream would never return" in a status slot.
const NO_RETURN: f64 = -3.0;

/// An entity the op created (not in the original export).
const CREATED: f64 = -3.0;

/// `SM_ERR_INVALID_INPUT`.
const SM_ERR_INVALID_INPUT: f64 = 1007.0;

// ---- the B-reps of heal.stitch_p1 (copied from its runner) ---------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SheetFace {
    surface: SrfIn,
    dom: [f64; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum StitchBrep {
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

fn block_of(b: &StitchBrep) -> Result<Vec<f64>, HarnessError> {
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

fn path(b: &StitchBrep) -> Result<String, HarnessError> {
    match b {
        StitchBrep::Brep { brep, .. } => path_of(Some(brep)),
        _ => Ok(String::new()),
    }
}

/// The C++ export of the B-rep.
fn export(b: &StitchBrep) -> Result<Vec<f64>, HarnessError> {
    let (path, block) = (path(b)?, block_of(b)?);
    crate::export_memo::export("heal_stitch_p2", &[&path], &block, || {
        ffi::eval(Op::Export, &path, &block)
    })
    .map_err(|e| harness(format!("c++ export: {e}")))
}

fn build_failure(export: &[f64]) -> Output {
    Output::err(format!(
        "brep_build_status_{}",
        export.get(1).copied().unwrap_or(0.0)
    ))
}

/// An op needing a vertex the B-rep lacks: no code runs on either side, so the case is
/// rejected (not counted as a pass), as in `heal.stitch_p1`.
fn no_entity() -> Output {
    Output::rejected("no vertex in this B-rep: nothing runs on either side")
}

fn pick(s: i64, n: usize) -> Option<u32> {
    u32::try_from(sel(s, n)).ok()
}

// ---- inputs -----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
#[allow(clippy::struct_excessive_bools)] // upstream's option flags
enum OpIn {
    /// `DoStitching`; candidate lists are selectors (`None` = the null list).
    Do {
        tol: f64,
        #[serde(default = "yes")]
        nesting: bool,
        #[serde(default)]
        ignore: bool,
        #[serde(default)]
        fast: bool,
        #[serde(default)]
        squeeze: bool,
        #[serde(default = "yes")]
        split: bool,
        #[serde(default = "yes")]
        manifold: bool,
        #[serde(default)]
        slivers: bool,
        #[serde(default)]
        want_vgap: bool,
        #[serde(default)]
        want_egap: bool,
        #[serde(default)]
        cand_vertices: Option<Vec<i64>>,
        #[serde(default)]
        cand_edges: Option<Vec<i64>>,
    },
    /// `StitchIntoSolid`.
    Solid,
    /// `StitchIntoShell`.
    Shell { well_formed: bool, max_tol: f64 },
    /// The one-list `StitchVertexPairs` (selectors; repeats dropped, `-1` null).
    VList {
        tol: f64,
        #[serde(default = "yes")]
        manifold: bool,
        #[serde(default)]
        squeeze: bool,
        list: Vec<i64>,
    },
    /// `SplitEdgesWithVertices` (selectors, `-1` null).
    Split {
        tol: f64,
        #[serde(default = "yes")]
        manifold: bool,
        #[serde(default)]
        ignore: bool,
        verts: Vec<i64>,
        edges: Vec<i64>,
        /// Run on a B-rep with `m_bEditingEnabled` set, as upstream's callers
        /// (`SmMerge`, `SmBrep::SqueezeFace`) do; without it `MakeVertexSplitEdge`
        /// refuses every split.
        #[serde(default)]
        editing: bool,
    },
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    brep: StitchBrep,
    op: OpIn,
}

// ---- resolution ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Resolved {
    Do {
        tol: f64,
        /// `nesting ignore fast squeeze split manifold slivers want_vgap want_egap`
        flags: [bool; 9],
        cand_vertices: Option<Vec<VertexId>>,
        cand_edges: Option<Vec<EdgeId>>,
    },
    Solid,
    Shell {
        well_formed: bool,
        max_tol: f64,
    },
    VList {
        tol: f64,
        manifold: bool,
        squeeze: bool,
        list: Vec<Option<VertexId>>,
    },
    Split {
        tol: f64,
        manifold: bool,
        ignore: bool,
        verts: Vec<Option<VertexId>>,
        edges: Vec<Option<EdgeId>>,
        editing: bool,
    },
}

fn opt_vertex(s: i64, b: &Brep) -> Option<VertexId> {
    if s == -1 {
        return None;
    }
    pick(s, b.vertices().len()).map(VertexId::from_index)
}

fn opt_edge(s: i64, b: &Brep) -> Option<EdgeId> {
    if s == -1 {
        return None;
    }
    pick(s, b.edges().len()).map(EdgeId::from_index)
}

fn resolve(op: &OpIn, b: &Brep) -> Option<Resolved> {
    Some(match op {
        OpIn::Do {
            tol,
            nesting,
            ignore,
            fast,
            squeeze,
            split,
            manifold,
            slivers,
            want_vgap,
            want_egap,
            cand_vertices,
            cand_edges,
        } => Resolved::Do {
            tol: *tol,
            flags: [
                *nesting, *ignore, *fast, *squeeze, *split, *manifold, *slivers, *want_vgap,
                *want_egap,
            ],
            cand_vertices: cand_vertices.as_ref().map(|s| {
                s.iter()
                    .filter_map(|&x| pick(x, b.vertices().len()).map(VertexId::from_index))
                    .collect()
            }),
            cand_edges: cand_edges.as_ref().map(|s| {
                s.iter()
                    .filter_map(|&x| pick(x, b.edges().len()).map(EdgeId::from_index))
                    .collect()
            }),
        },
        OpIn::Solid => Resolved::Solid,
        OpIn::Shell {
            well_formed,
            max_tol,
        } => Resolved::Shell {
            well_formed: *well_formed,
            max_tol: *max_tol,
        },
        OpIn::VList {
            tol,
            manifold,
            squeeze,
            list,
        } => {
            if b.vertices().is_empty() {
                return None;
            }
            let mut v: Vec<Option<VertexId>> = Vec::new();
            for &x in list {
                let id = opt_vertex(x, b);
                if id.is_none() || !v.contains(&id) {
                    v.push(id);
                }
            }
            Resolved::VList {
                tol: *tol,
                manifold: *manifold,
                squeeze: *squeeze,
                list: v,
            }
        }
        OpIn::Split {
            tol,
            manifold,
            ignore,
            verts,
            edges,
            editing,
        } => {
            if b.vertices().is_empty() {
                return None;
            }
            Resolved::Split {
                tol: *tol,
                manifold: *manifold,
                ignore: *ignore,
                verts: verts.iter().map(|&x| opt_vertex(x, b)).collect(),
                edges: edges.iter().map(|&x| opt_edge(x, b)).collect(),
                editing: *editing,
            }
        }
    })
}

fn idx<I: ArenaId>(id: I) -> f64 {
    f64::from(id.index())
}

fn push_opt<I: ArenaId + Copy>(v: &mut Vec<f64>, ids: &[Option<I>]) {
    v.push(ids.len() as f64);
    v.extend(ids.iter().map(|i| i.map_or(-1.0, idx)));
}

fn push_all<I: ArenaId + Copy>(v: &mut Vec<f64>, ids: &[I]) {
    v.push(ids.len() as f64);
    v.extend(ids.iter().map(|&i| idx(i)));
}

fn args(op: &Resolved) -> (Op, Vec<f64>) {
    let mut v = Vec::new();
    let code = match op {
        Resolved::Do {
            tol,
            flags,
            cand_vertices,
            cand_edges,
        } => {
            v.push(*tol);
            v.extend(flags.iter().map(|&f| flag(f)));
            v.push(flag(cand_vertices.is_some()));
            push_all(&mut v, cand_vertices.as_deref().unwrap_or_default());
            v.push(flag(cand_edges.is_some()));
            push_all(&mut v, cand_edges.as_deref().unwrap_or_default());
            Op::Do
        }
        Resolved::Solid => Op::Solid,
        Resolved::Shell {
            well_formed,
            max_tol,
        } => {
            v.extend([flag(*well_formed), *max_tol]);
            Op::Shell
        }
        Resolved::VList {
            tol,
            manifold,
            squeeze,
            list,
        } => {
            v.extend([*tol, flag(*manifold), flag(*squeeze)]);
            push_opt(&mut v, list);
            Op::VList
        }
        Resolved::Split {
            tol,
            manifold,
            ignore,
            verts,
            edges,
            editing,
        } => {
            v.extend([*tol, flag(*manifold), flag(*ignore)]);
            push_opt(&mut v, verts);
            push_opt(&mut v, edges);
            v.push(flag(*editing));
            Op::Split
        }
    };
    (code, v)
}

// ---- C++ side ---------------------------------------------------------------------------

fn guarded_output(o: Vec<f64>) -> Output {
    match o.first() {
        Some(&s) if s == NULL_DEREF => Output::err("null_dereference_guard"),
        Some(&s) if s == NO_RETURN => Output::err("no_return_guard"),
        _ => Output::new().values("out", o),
    }
}

/// The C++ side's resolved op, or the early output: resolved on the Rust build
/// ([`prepare_rust`]), so both sides get the same op, and on the import of the C++
/// export only when the Rust build fails.
fn prepare_cpp(i: &Inputs) -> Result<Result<Resolved, Output>, HarnessError> {
    let ex = export(&i.brep)?;
    let Some((_, graph_at)) = counts(&ex) else {
        return Ok(Err(build_failure(&ex)));
    };
    if let Ok((_, op)) = prepare_rust(&mut Kernel::default(), i)? {
        return Ok(Ok(op));
    }
    let (b, _) = super::topology_brep_p1::import_raw(&ex, graph_at)?;
    Ok(resolve(&i.op, &b).ok_or_else(no_entity))
}

/// The Rust side's own B-rep of the case (`heal.stitch_p1`'s [`own_brep`], built by the
/// Rust kernel: no C++ export reaches it) and its resolved op, or the early output.
///
/// [`own_brep`]: super::heal_stitch_p1::own_brep
fn prepare_rust(k: &mut Kernel, i: &Inputs) -> Result<Result<(Brep, Resolved), Output>, HarnessError> {
    let src: super::heal_stitch_p1::StitchBrep = serde_json::to_value(&i.brep)
        .and_then(serde_json::from_value)
        .map_err(|e| harness(format!("B-rep block: {e}")))?;
    let b = match super::heal_stitch_p1::own_brep(k, &src)? {
        Ok(b) => b,
        Err(o) => return Ok(Err(o)),
    };
    let Some(op) = resolve(&i.op, &b) else {
        return Ok(Err(no_entity()));
    };
    Ok(Ok((b, op)))
}

fn run_cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let op = match prepare_cpp(i)? {
        Ok(r) => r,
        Err(o) => return Ok(o),
    };
    let (code, a) = args(&op);
    let mut input = block_of(&i.brep)?;
    input.extend(a);
    let p = path(&i.brep)?;
    let call = || ffi::eval(code, &p, &input);
    // A file B-rep's op runs in a fresh process: after a stitch of an `.smb` B-rep, the
    // next read of a file in the same process exports differently (state the shim's
    // context keeps), so a case's C++ result would depend on the cases before it.
    let out = if p.is_empty() {
        call()
    } else {
        solidcore_ffi::isolate::fresh_process(call)
    }
    .map_err(|e| harness(format!("c++: {e}")))?;
    let body = out.get(1..).ok_or_else(|| harness("c++: empty output"))?;
    Ok(guarded_output(body.to_vec()))
}

// ---- Rust side --------------------------------------------------------------------------

/// The `SmStatus` of a Rust error: an upstream code as is; `InvalidInput` raised by the
/// stitch itself marks a null dereference upstream (`-2`), from a callee it is
/// `SM_ERR_INVALID_INPUT`.
fn status_of(e: &SolidError) -> f64 {
    match e {
        SolidError::Upstream { code, .. } => f64::from(*code),
        SolidError::InvalidInput { operation, .. }
            if *operation == "heal::stitch::p1" || *operation == "heal::stitch::p2" =>
        {
            NULL_DEREF
        }
        SolidError::InvalidInput { .. } => SM_ERR_INVALID_INPUT,
        _ => -9.0,
    }
}

/// Entity ids of the original B-rep by index; ids the op created are [`CREATED`].
struct Ids {
    n_vertices: usize,
    n_edges: usize,
}

impl Ids {
    fn vertex(&self, v: Option<VertexId>) -> f64 {
        match v {
            None => -1.0,
            Some(v) if (v.index() as usize) < self.n_vertices => idx(v),
            Some(_) => CREATED,
        }
    }

    fn edge(&self, e: Option<EdgeId>) -> f64 {
        match e {
            None => -1.0,
            Some(e) if (e.index() as usize) < self.n_edges => idx(e),
            Some(_) => CREATED,
        }
    }

    fn vertices(&self, o: &mut Vec<f64>, l: impl ExactSizeIterator<Item = Option<VertexId>>) {
        o.push(l.len() as f64);
        for v in l {
            o.push(self.vertex(v));
        }
    }

    fn edges(&self, o: &mut Vec<f64>, l: impl ExactSizeIterator<Item = Option<EdgeId>>) {
        o.push(l.len() as f64);
        for e in l {
            o.push(self.edge(e));
        }
    }
}

/// The B-rep after the op (as the shim's `put_digest`): the counts, each vertex's point
/// and tolerance, then `heal.stitch_p1`'s topology digest.
fn digest(k: &mut Kernel, b: &Brep, o: &mut Vec<f64>) -> solidcore::Result<()> {
    let fs = bp1::faces(b, None)?;
    let es = bp1::edges(b, None)?;
    let vs = bp1::vertices(b, None)?;
    o.extend([fs.len() as f64, es.len() as f64, vs.len() as f64]);
    for v in vs {
        let p = vertex::point(b, v)?;
        o.extend([p.x, p.y, p.z, vertex::tolerance(b, v)?]);
    }
    super::heal_stitch_p1::topology_digest(k, b, o)
}

fn run_rust_op(k: &mut Kernel, b: &mut Brep, op: &Resolved) -> solidcore::Result<Vec<f64>> {
    let ids = Ids {
        n_vertices: b.vertices().len(),
        n_edges: b.edges().len(),
    };
    let mut cb = st1::DefaultStitchCallback::new();
    let failed = |e: SolidError| -> solidcore::Result<Vec<f64>> {
        if e.is_unported() {
            Err(e)
        } else {
            Ok(vec![status_of(&e)])
        }
    };
    match op {
        Resolved::Do {
            tol,
            flags,
            cand_vertices,
            cand_edges,
        } => {
            let [nesting, ignore, fast, squeeze, split, manifold, slivers, want_v, want_e] = *flags;
            let mut s = st1::Stitch::new(&mut cb, *tol, nesting, true);
            s.ignore_problems = ignore;
            s.fast_edge_compare = fast;
            s.squeeze_small_edges = squeeze;
            s.split_edges_with_vertices = split;
            s.making_manifold_solid = manifold;
            s.remove_laminar_slivers = slivers;
            let gaps = st::MinGapRequest {
                vertex: want_v,
                edge: want_e,
            };
            match s.do_stitching(k, b, cand_vertices.as_deref(), cand_edges.as_deref(), gaps) {
                Ok(r) => {
                    let mut o = vec![
                        0.0,
                        r.stitched_edges as f64,
                        r.lamina_edges as f64,
                        r.max_vertex_gap,
                        r.max_edge_gap,
                    ];
                    o.extend(r.min_unstitched_vert_gap);
                    o.extend(r.min_unstitched_edge_gap);
                    digest(k, b, &mut o)?;
                    Ok(o)
                }
                Err(e) => failed(e),
            }
        }
        Resolved::Solid => {
            // the shim's guard: the doubling from a tolerance at or below zero cannot
            // end (HANG-24); same test on the same box diagonal (`tol < dist / 200`
            // excludes a NaN tolerance, so `!(tol > 0)` is `tol <= 0` here)
            let (tol, dist) = match st::solid_stitch_start(k, b) {
                Ok(start) => start,
                Err(e) => return failed(e),
            };
            if tol < dist / 200.0 && tol <= 0.0 {
                return Ok(vec![NO_RETURN]);
            }
            match st::stitch_into_solid(k, b) {
                Ok(r) => {
                    let mut o = vec![
                        0.0,
                        flag(r.produces_a_solid),
                        r.stitched_edges as f64,
                        r.max_vertex_gap,
                        r.max_edge_gap,
                    ];
                    digest(k, b, &mut o)?;
                    Ok(o)
                }
                Err(e) => failed(e),
            }
        }
        Resolved::Shell {
            well_formed,
            max_tol,
        } => {
            // the shim's guard: the doubling from max_tol / 100 cannot end
            if *max_tol / 100.0 == 0.0 && 0.0 < *max_tol {
                return Ok(vec![NO_RETURN]);
            }
            match st::stitch_into_shell(k, b, *well_formed, *max_tol) {
                Ok(r) => {
                    let mut o = vec![
                        0.0,
                        r.stitched_edges as f64,
                        r.max_vertex_gap,
                        r.max_edge_gap,
                    ];
                    digest(k, b, &mut o)?;
                    Ok(o)
                }
                Err(e) => failed(e),
            }
        }
        Resolved::VList {
            tol,
            manifold,
            squeeze,
            list,
        } => {
            let mut s = st1::Stitch::new(&mut cb, *tol, true, true);
            s.making_manifold_solid = *manifold;
            s.squeeze_small_edges = *squeeze;
            let mut l = list.clone();
            match s.stitch_vertex_list(k, b, &mut l) {
                Ok(r) => {
                    let mut o = vec![0.0, r.max_vertex_gap, r.min_unstitched_vert_gap];
                    ids.vertices(&mut o, r.surviving.iter().map(|&v| Some(v)));
                    ids.vertices(&mut o, r.deleted.iter().map(|&v| Some(v)));
                    ids.edges(&mut o, r.deleted_edges.iter().map(|&e| Some(e)));
                    ids.vertices(&mut o, l.iter().copied());
                    if !r.deleted.is_empty() {
                        digest(k, b, &mut o)?;
                    }
                    Ok(o)
                }
                Err(e) => failed(e),
            }
        }
        Resolved::Split {
            tol,
            manifold,
            ignore,
            verts,
            edges,
            editing,
        } => {
            if *editing {
                b.set_editing_enabled(true);
            }
            let mut s = st1::Stitch::new(&mut cb, *tol, true, true);
            s.making_manifold_solid = *manifold;
            s.ignore_problems = *ignore;
            let mut v = verts.clone();
            let mut e = edges.clone();
            match s.split_edges_with_vertices(k, b, &mut v, &mut e) {
                Ok(r) => {
                    let mut o = vec![0.0, r.max_vertex_gap, r.min_unstitched_vert_gap];
                    ids.vertices(&mut o, r.processed.iter().map(|&x| Some(x)));
                    ids.edges(&mut o, r.split.iter().map(|&x| Some(x)));
                    ids.edges(&mut o, r.new_edges.iter().map(|&x| Some(x)));
                    ids.vertices(&mut o, v.iter().copied());
                    ids.edges(&mut o, e.iter().copied());
                    if !r.processed.is_empty() || !r.new_edges.is_empty() {
                        digest(k, b, &mut o)?;
                    }
                    Ok(o)
                }
                Err(e) => failed(e),
            }
        }
    }
}

fn run_rust(i: &Inputs) -> Result<Output, HarnessError> {
    // one context for the build and the op, as the shim's
    let mut k = Kernel::default();
    let (mut b, op) = match prepare_rust(&mut k, i)? {
        Ok(r) => r,
        Err(o) => return Ok(o),
    };
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
/// control point (0 for some faces), some faces reversed in u (as `heal.stitch_p1`).
fn gen_sheets(rng: &mut Rng) -> StitchBrep {
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

fn gen_tol(rng: &mut Rng) -> f64 {
    *rng.choose(&[0.0, 1e-9, 1e-6, 1e-5, 1e-4, 1e-3, 1e-2, 0.1, 1.0, 10.0])
        .unwrap_or(&1e-3)
}

fn gen_sels(rng: &mut Rng, max: u64) -> Vec<i64> {
    let n = rng.below(max + 1);
    (0..n).map(|_| rng.below(1 << 20) as i64).collect()
}

/// Selectors with some `-1` (null) entries.
fn gen_opt_sels(rng: &mut Rng, max: u64) -> Vec<i64> {
    let mut v = gen_sels(rng, max);
    for x in &mut v {
        if rng.chance(0.1) {
            *x = -1;
        }
    }
    v
}

fn gen_op(rng: &mut Rng) -> OpIn {
    match rng.below(20) {
        0..=4 => OpIn::Do {
            tol: gen_tol(rng),
            nesting: rng.chance(0.5),
            ignore: rng.chance(0.3),
            fast: rng.chance(0.3),
            squeeze: rng.chance(0.3),
            split: rng.chance(0.6),
            manifold: rng.chance(0.7),
            slivers: rng.chance(0.2),
            want_vgap: rng.chance(0.5),
            want_egap: rng.chance(0.5),
            cand_vertices: rng.chance(0.3).then(|| gen_sels(rng, 8)),
            cand_edges: rng.chance(0.3).then(|| gen_sels(rng, 8)),
        },
        5 => OpIn::Solid,
        6..=7 => OpIn::Shell {
            well_formed: rng.chance(0.5),
            max_tol: *rng
                .choose(&[-1.0, 0.0, 0.0, 1e-6, 1e-3, 0.1, 1.0])
                .unwrap_or(&0.0),
        },
        8..=13 => OpIn::VList {
            tol: gen_tol(rng),
            manifold: rng.chance(0.6),
            squeeze: rng.chance(0.4),
            list: gen_opt_sels(rng, 16),
        },
        _ => OpIn::Split {
            tol: gen_tol(rng),
            manifold: rng.chance(0.6),
            ignore: rng.chance(0.3),
            verts: gen_opt_sels(rng, 8),
            edges: gen_opt_sels(rng, 12),
            editing: false,
        },
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "heal.stitch_p2"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        run_cpp(&case.parse::<Inputs>()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        run_rust(&case.parse::<Inputs>()?)
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
