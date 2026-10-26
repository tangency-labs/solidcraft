//! Runner for unit `blend.fillet_corner_p2`: `sm_FindConcaveEdgeuses` and the corner
//! methods ported so far (`AdjustConcaveCornerTopology`, `UpdateSplitFace`).
//!
//! C++: `solidcore_ffi::units::blend_fillet_corner_p2` (its shim is included in
//! `blend_fillet_corner_p1.cpp`, which compiles `source/SMLib/src/SmFilletCorner.cpp`).
//! Rust: `solidcore::blend::fillet_corner::p2`.
//!
//! **Corner cases** (`corner`) use unit `blend.fillet_corner_p3`'s corner harness (its
//! model, corner and shim, `sc_blend_fillet_corner_p3_corner`): an Nx2-shaped model
//! (`gen_nx2_model` there) with a corner whose method is
//! - `adjust_concave`: the virtual `AdjustTopology` of an Nx2 corner, which with three
//!   corner edges is `AdjustConcaveCornerTopology` (also other kinds and edge counts,
//!   which take the other branches: the `NxN` kinds `SmFilletConvexNxNCorner::
//!   AdjustTopology`). It runs to the end; the shim's pre-check refuses where upstream
//!   dereferences null (a side face or surface missing for the extension, an edge use
//!   without edge or mate in the mates loop). The vertices an op makes are appended to
//!   the model's export in creation order on both sides;
//! - `analytic`: `MakeAnalyticCornerSurface` (its `SmBoolean`, a `SER` / `NER` status
//!   read as `1`), on corners built for it (`gen_analytic`): a sphere octant of exact
//!   rational quarter arcs (one arc may be its chord), chamfers of lines (a plane or a
//!   bilinear patch; a rail-rail arc on a target face is recomputed as the
//!   intersection), four-sided rail-rail corners of two coaxial arcs and two lines (the
//!   surface of revolution, or the variable-radius fillet's early returns), and edge
//!   counts, a missing curve or a cubic interpolation that return before a fit. The
//!   corner's surfaces are compared value by value. The shim's pre-check runs the fits
//!   (copies, deleted) to refuse exactly the inputs where upstream would dereference a
//!   missing fillet solver (the model's fillet geometries have none) or vertex, and
//!   does not run the sphere fit's fallback `MakeConstantRadiusFillet` (unit
//!   `blend.fillet_corner_p1`, deferred);
//! - `update_split_face`: `UpdateSplitFace(orig, split)` on target faces, some fillet
//!   vertices classified on (or with their other face) a target face and some fillet
//!   edges with an original face. A fillet edge on `orig` reaches
//!   `SmFilletEdge::ShouldSwapFaces` (`blend.fillet_geom_p6`); the shim refuses the
//!   edges it would dereference `NULL` on (no curve, or a corner without a vertex).
//!
//! Compared: the status, the corner's members, the model's whole export and the
//! corner's surfaces. 80 built-in corner cases and 120 `analytic` ones (fixed seeds) plus
//! generated ones.
//!
//! **Tangent-solver cases** (`p1_corner`): unit `blend.fillet_corner_p1`'s `solvers`
//! corner cases (its runner, model and shim, `sc_blend_fillet_corner_p1_corner`) on
//! closed Nx1 and Nx2 corners, which run this unit's `GetTangentFilletSolver` overrides
//! through the virtual call: 40 built-in, drawn from that runner's generator with a fixed
//! seed and kept when they are such cases with registered solvers; run and compared by
//! that runner unchanged. Most of them return `NULL` on both sides (no G1 pair), so 16
//! more are built from the first of them with two or more targets, its target curves,
//! filleted edge uses, solver registrations, query and tangency tolerance replaced so
//! that every branch returns: Nx2 on two collinear lines (G1) or two perpendicular ones,
//! each solver and a foreign one queried, and with only the first edge use registered;
//! closed Nx1 on a line (its end tangents are parallel: G1) or a quadratic whose end
//! tangents are 90 degrees apart, its solver and a foreign one queried, and with the
//! solver registered on another edge use.
//!
//! **Cases.** `find_concave` on a real B-rep (unit `topology.edgeuse_p1`'s B-rep block:
//! the box-like fillet test files of `TestFiles/pt_TestFiles/Fillet` that unit
//! `blend.fillet_corner_p1` uses, with convex and concave corners, and that unit's
//! generated B-reps). The C++ side builds the B-rep, the Rust side imports the shim's
//! export. A vertex is selected; the edge uses are either the corner walk
//! `SmFilletCorner::Create` makes (from an edge use starting at the vertex: its clockwise
//! edge use's radial, skipping edges already seen, until the walk closes), or a selection
//! of the edge uses starting at the vertex (in export order, repeats allowed), sometimes
//! with edge uses of elsewhere. Built-in: every fillet file at its first 12 vertices, as
//! the walk and as the export ring, and the wire network of `prog_test` `my_test_topology`
//! at 4 vertices (its wire edge uses' radials). Compared: the concave pair (export
//! indices), or a count of 0 when no pair is concave.
//!
//! The shim refuses (`-2`) when a pair the loop reaches has an edge use upstream would
//! dereference null; the Rust side returns [`SolidError::InvalidInput`] there (whatever
//! its reason: a missing radial, or a missing edge as unit `topology.edgeuse_p1` reports
//! it) and both report `refused`. No B-rep the harness builds reaches it (every edge use
//! has a radial and an edge); the Rust half is covered by a unit test in
//! `fillet_corner::p2`. A case whose evaluated geometry the import holds only as its
//! B-spline part is deferred as unit `blend.fillet_corner_p1`'s runner defers it.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::blend::fillet_corner::p2 as corner;
use solidcore::error::SolidError;
use solidcore::kernel::Kernel;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::{Brep, EdgeuseId, VertexId};
use solidcore_ffi::units::blend_fillet_corner_p2 as ffi;
use solidcore_ffi::units::blend_fillet_corner_p3::Op as CornerOp;
use solidcore_ffi::units::topology_edgeuse_p1 as ffi1;

use super::blend_fillet_corner_p1 as p1;
use super::blend_fillet_corner_p3::{
    corner_cpp_op, corner_rust_status, edge_in, gen_nx2_model, line, plane_srf, vin, CornerIn,
    EuSel,
};
use super::blend_fillet_geom_p1::{Crv, Model};

use super::blend_fillet_corner_p1::{
    eu_vertex, gen_topo_brep, inexact, rust_err, Topo, CORNER_FILES,
};
use super::topology_edgeuse_p1::{brep_block_of, harness, import_with_types, path_of, sel, BrepIn};
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Registered runner.
pub struct Runner;

/// The upper bound of a corner walk (`SmFilletCorner::Create` stops when it closes).
const MAX_WALK: usize = 64;

/// Built-in corner cases (from [`CORNER_SEED`]).
const CORNER_CASES: usize = 80;
/// Built-in `MakeAnalyticCornerSurface` cases (from [`ANALYTIC_SEED`]).
const ANALYTIC_CASES: usize = 120;
/// The seed of the built-in `MakeAnalyticCornerSurface` cases.
const ANALYTIC_SEED: u64 = 0x00a7_a1c5;

/// The seed of the built-in corner cases.
const CORNER_SEED: u64 = 0x00c0_47e2;

/// Built-in tangent-solver cases (from [`TANGENT_SEED`]).
const TANGENT_CASES: usize = 40;

/// The seed of the built-in tangent-solver cases.
const TANGENT_SEED: u64 = 0x7a46_e275;

/// Draws to find [`TANGENT_CASES`] (generous: about 1 in 70 draws qualifies).
const TANGENT_DRAWS: usize = 200_000;

/// A degree-1 target curve from `a` to `b` (the shape of `blend.fillet_geom_p1`'s
/// `Crv`).
fn line_target(a: [f64; 3], b: [f64; 3]) -> Value {
    serde_json::json!({
        "dim": 3,
        "cpts": [[a[0], a[1], a[2], 1.0], [b[0], b[1], b[2], 1.0]],
        "p": 1,
        "u": [0.0, 0.0, 1.0, 1.0],
    })
}

/// A quadratic target curve whose end tangents are 90 degrees apart.
fn bent_target() -> Value {
    serde_json::json!({
        "dim": 3,
        "cpts": [[0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 0.0, 1.0], [2.0, 0.0, 0.0, 1.0]],
        "p": 2,
        "u": [0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
    })
}

/// `base` (a `blend.fillet_corner_p1` `solvers` case with two or more targets) on a
/// corner of `kind` at the start of target 0, filleted edge uses `eus` (primary edge
/// uses of targets), targets 0 and 1 replaced by `t0`, `t1`, solvers registered on
/// `regs` (targets) and solver `query` asked about (`regs.len()`: an unregistered one).
fn tangent_variant(
    base: &Value,
    kind: u64,
    t0: Value,
    t1: Value,
    eus: &[usize],
    regs: &[usize],
    query: usize,
) -> Option<Value> {
    let mut v = base.clone();
    let targets = v.get_mut("model")?.get_mut("targets")?.as_array_mut()?;
    *targets.get_mut(0)? = t0;
    *targets.get_mut(1)? = t1;
    let corner = v.get_mut("corner")?.as_object_mut()?;
    corner.insert("kind".into(), kind.into());
    corner.insert("v_edge".into(), 0.into());
    corner.insert("v_sub".into(), 0.into());
    corner.insert("tan".into(), 1e-2.into());
    corner.insert(
        "solver".into(),
        eus.iter().map(|&e| serde_json::json!([e, 0])).collect(),
    );
    let regs: Vec<Value> = regs
        .iter()
        .map(|&e| serde_json::json!({"e": e, "which": 0, "approx": 1e-3, "tang": 1e-2}))
        .collect();
    let obj = v.as_object_mut()?;
    obj.insert("solvers".into(), regs.into());
    obj.insert("query".into(), query.into());
    Some(v)
}

/// The built-in tangent-solver cases drawn from unit `blend.fillet_corner_p1`'s
/// generator ([`TANGENT_SEED`]), and the first of them with two or more targets (the
/// base of [`tangent_variants`]).
fn tangent_cases() -> (Vec<Value>, Option<Value>) {
    let mut rng = Rng::new(TANGENT_SEED);
    let mut drawn = Vec::new();
    let mut base = None;
    for _ in 0..TANGENT_DRAWS {
        if drawn.len() == TANGENT_CASES {
            break;
        }
        let Some(inner) = p1::Runner.generate(&mut rng) else {
            continue;
        };
        if !is_tangent_case(&inner) {
            continue;
        }
        let two_targets = inner
            .get("model")
            .and_then(|m| m.get("targets"))
            .and_then(Value::as_array)
            .is_some_and(|t| t.len() >= 2);
        if base.is_none() && two_targets {
            base = Some(inner.clone());
        }
        drawn.push(inner);
    }
    (drawn, base)
}

/// The 16 targeted tangent-solver variants of `base` (named).
fn tangent_variants(base: &Value) -> Vec<(String, Value)> {
    let x = line_target([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
    let collinear = line_target([0.0, 0.0, 0.0], [-2.0, 0.0, 0.0]);
    let perpendicular = line_target([0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let mut out = Vec::new();
    let mut add = |name: String, v: Option<Value>| {
        if let Some(v) = v {
            out.push((name, v));
        }
    };
    for (g1, t1) in [("g1", &collinear), ("not_g1", &perpendicular)] {
        for q in 0..3 {
            add(
                format!("nx2_{g1}_query{q}"),
                tangent_variant(base, 7, x.clone(), t1.clone(), &[0, 1], &[0, 1], q),
            );
        }
        for q in 0..2 {
            add(
                format!("nx2_{g1}_one_registered_query{q}"),
                tangent_variant(base, 7, x.clone(), t1.clone(), &[0, 1], &[0], q),
            );
        }
    }
    for (g1, t0) in [("g1", &x), ("not_g1", &bent_target())] {
        for q in 0..2 {
            add(
                format!("nx1_closed_{g1}_query{q}"),
                tangent_variant(base, 2, t0.clone(), perpendicular.clone(), &[0], &[0], q),
            );
        }
        add(
            format!("nx1_closed_{g1}_other_registered"),
            tangent_variant(base, 2, t0.clone(), perpendicular.clone(), &[0], &[1], 0),
        );
    }
    out
}

/// The corner method of a `corner` case.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum CornerMethod {
    /// The virtual `AdjustTopology` (`AdjustConcaveCornerTopology` for an Nx2 corner with
    /// three corner edges).
    AdjustConcave,
    /// `SmFilletCorner::UpdateSplitFace(orig, split)`.
    UpdateSplitFace,
    /// `SmFilletCorner::MakeAnalyticCornerSurface`.
    Analytic,
}

/// One case.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    /// `sm_FindConcaveEdgeuses(v, all, ..)`. `walk`: `all` is the corner walk from the
    /// edge use `eus[0]` selects; else each of `eus` selects an edge use starting at the
    /// vertex (a negative one, `-1 - s`, any edge use of the B-rep), and no `eus` is all
    /// of them in export order.
    FindConcave {
        brep: BrepIn,
        v: i64,
        eus: Vec<i64>,
        walk: bool,
    },
    /// A case of unit `blend.fillet_corner_p1`'s runner (its JSON inputs), run by it.
    P1Corner { inner: Value },
    /// A corner method on unit `blend.fillet_corner_p3`'s corner harness; `split` are
    /// the target faces of `update_split_face`.
    Corner {
        method: CornerMethod,
        model: Box<Model>,
        corner: CornerIn,
        #[serde(default)]
        split: [usize; 2],
    },
}

/// The corner walk of `SmFilletCorner::Create` from `start`: the clockwise edge use's
/// radial, skipping an edge use whose edge the step started on or one already seen,
/// until it returns to `start`; `None` when a link is missing or it does not close.
fn corner_walk(b: &Brep, start: EdgeuseId) -> Option<Vec<EdgeuseId>> {
    let edge = |eu| eu1::edge(b, eu).ok().flatten();
    let mut all = vec![start];
    let mut eu = start;
    for _ in 0..MAX_WALK {
        let e0 = eu;
        let e1 = eu1::cw_edgeuse(b, e0).ok().flatten()?;
        eu = eu1::radial(b, e1).ok().flatten()?;
        if eu == start {
            return Some(all);
        }
        if edge(e0) == edge(e1) {
            continue;
        }
        if all.iter().any(|&p| edge(p) == edge(eu)) {
            continue;
        }
        all.push(eu);
    }
    None
}

/// Builds and imports the B-rep and resolves the vertex and edge uses: `args` is
/// `v eu..`. `Err(output)` when the case ends before the op, the same on both sides.
fn topo(i: &Inputs) -> Result<Result<Topo, Output>, HarnessError> {
    let Inputs::FindConcave { brep, v, eus, walk } = i else {
        return Err(harness("not a find_concave case"));
    };
    let path = path_of(Some(brep))?;
    let block = brep_block_of(Some(brep), false, false)?;
    let ex = ffi1::eval(ffi1::Op::Export, &path, &block)
        .map_err(|e| harness(format!("c++ export: {e}")))?;
    let Some(c) = ffi1::counts(&ex) else {
        let st = ex.get(1).copied().unwrap_or(0.0);
        return Ok(Err(Output::err(format!("brep_build_status_{st}"))));
    };
    let (n_eu, n_v) = (c[6], c[9]);
    let (b, types) = import_with_types(&ex)?;
    let vv = sel(*v, n_v);
    if vv < 0 {
        return Ok(Err(Output::err("no_vertex")));
    }
    let vid = VertexId::from_index(vv as u32);
    // the edge uses starting at the vertex (export order)
    let ring: Vec<EdgeuseId> = b
        .edgeuses()
        .iter()
        .map(|(id, _)| id)
        .filter(|&id| eu_vertex(&b, id) == Some(vid))
        .collect();
    if ring.is_empty() {
        return Ok(Err(Output::err("no_edge_use")));
    }
    let from_ring = |s: i64| ring.get(sel(s, ring.len()).max(0) as usize).copied();
    let chosen: Vec<EdgeuseId> = if *walk {
        let start = eus.first().copied().unwrap_or(0).rem_euclid(1 << 30);
        let Some(start) = from_ring(start) else {
            return Err(harness("ring selection"));
        };
        match corner_walk(&b, start) {
            Some(w) => w,
            None => return Ok(Err(Output::err("walk_open"))),
        }
    } else if eus.is_empty() {
        ring.clone()
    } else {
        eus.iter()
            .filter_map(|&s| {
                if s < 0 {
                    let r = sel(-1 - s, n_eu);
                    (r >= 0).then(|| EdgeuseId::from_index(r as u32))
                } else {
                    from_ring(s)
                }
            })
            .collect()
    };
    if chosen.is_empty() {
        return Ok(Err(Output::err("no_edge_use")));
    }
    let mut args = vec![vv];
    args.extend(chosen.iter().map(|e| i64::from(e.index())));
    Ok(Ok(Topo {
        path,
        block,
        brep: b,
        types,
        args,
    }))
}

fn corner_cpp(
    method: CornerMethod,
    model: &Model,
    c: &CornerIn,
    split: [usize; 2],
) -> Result<Output, HarnessError> {
    match method {
        CornerMethod::AdjustConcave => corner_cpp_op(CornerOp::AdjustTopology, &[], model, c),
        CornerMethod::UpdateSplitFace => corner_cpp_op(
            CornerOp::UpdateSplitFace,
            &[split[0] as f64, split[1] as f64],
            model,
            c,
        ),
        CornerMethod::Analytic => corner_cpp_op(CornerOp::AnalyticSurface, &[], model, c),
    }
}

fn corner_rust(
    k: &Kernel,
    method: CornerMethod,
    model: &Model,
    c: &CornerIn,
    split: [usize; 2],
) -> Result<Output, HarnessError> {
    corner_rust_status(k, model, c, |c, env, faces| match method {
        CornerMethod::AdjustConcave => c.adjust_topology(env).map(|()| 0.0),
        CornerMethod::UpdateSplitFace => {
            let (Some(&orig), Some(&new)) = (faces.get(split[0]), faces.get(split[1])) else {
                return Err(SolidError::invalid_input("difftest", "split face index"));
            };
            c.update_split_face(env, orig, new, std::slice::from_ref(c))
                .map(|()| 0.0)
        }
        CornerMethod::Analytic => c
            .make_analytic_corner_surface(env)
            .map(|made| if made { 1.0 } else { 0.0 }),
    })
}

/// `inner` as a case of unit `blend.fillet_corner_p1`.
fn p1_case(inner: &Value) -> Case {
    Case {
        unit: "blend.fillet_corner_p1".to_string(),
        name: "p1_corner".to_string(),
        source: crate::CaseSource::Fixture {
            path: "built-in: blend.fillet_corner_p2 cases".to_string(),
            index: 0,
            upstream_ref: None,
        },
        inputs: inner.clone(),
    }
}

/// Whether a `blend.fillet_corner_p1` case is a `solvers` corner case on a closed Nx1
/// (kind 2) or Nx2 (kind 7) corner with registered solvers.
fn is_tangent_case(v: &Value) -> bool {
    v.get("op").and_then(Value::as_str) == Some("corner")
        && v.get("method").and_then(Value::as_str) == Some("solvers")
        && matches!(
            v.get("corner")
                .and_then(|c| c.get("kind"))
                .and_then(Value::as_u64),
            Some(2 | 7)
        )
        && v.get("solvers")
            .and_then(Value::as_array)
            .is_some_and(|a| !a.is_empty())
}

fn run_cpp(i: &Inputs) -> Result<Output, HarnessError> {
    if let Inputs::P1Corner { inner } = i {
        return p1::Runner.run_cpp(&p1_case(inner));
    }
    if let Inputs::Corner {
        method,
        model,
        corner,
        split,
    } = i
    {
        return corner_cpp(*method, model, corner, *split);
    }
    let t = match topo(i)? {
        Ok(t) => t,
        Err(o) => return Ok(o),
    };
    let mut v = t.block.clone();
    let (&vv, eus) = t
        .args
        .split_first()
        .ok_or_else(|| harness("missing vertex"))?;
    v.push(vv as f64);
    v.push(eus.len() as f64);
    v.extend(eus.iter().map(|&a| a as f64));
    let out = ffi::find_concave(&t.path, &v).map_err(|e| harness(format!("c++: {e}")))?;
    let Some((&head, rest)) = out.split_first() else {
        return Err(harness("empty output"));
    };
    if head != 0.0 {
        let st = rest.first().copied().unwrap_or(0.0);
        return Ok(Output::err(format!("brep_build_status_{st}")));
    }
    if rest.first().copied() == Some(-2.0) {
        return Ok(Output::err("refused"));
    }
    Ok(Output::new().values("concave", rest.to_vec()))
}

fn run_rust(i: &Inputs) -> Result<Output, HarnessError> {
    if let Inputs::P1Corner { inner } = i {
        return p1::Runner.run_rust(&p1_case(inner));
    }
    if let Inputs::Corner {
        method,
        model,
        corner,
        split,
    } = i
    {
        return corner_rust(&Kernel::default(), *method, model, corner, *split);
    }
    let mut t = match topo(i)? {
        Ok(t) => t,
        Err(o) => return Ok(o),
    };
    let (&vv, eus) = t
        .args
        .split_first()
        .ok_or_else(|| harness("missing vertex"))?;
    let v = VertexId::from_index(u32::try_from(vv).map_err(|_| harness("index"))?);
    let eus = eus
        .iter()
        .map(|&a| u32::try_from(a).map(EdgeuseId::from_index))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| harness("index"))?;
    if let Some(o) = inexact(&t, &[], &eus) {
        return Ok(o);
    }
    let k = Kernel::default();
    match corner::find_concave_edgeuses(&k, &mut t.brep, v, &eus) {
        Ok(c) => {
            let mut o = vec![c.len() as f64];
            o.extend(c.iter().map(|e| f64::from(e.index())));
            Ok(Output::new().values("concave", o))
        }
        // every InvalidInput of the op is a null dereference upstream (the empty list is
        // screened above): a missing radial (`NULL_DEREF`) or, from unit
        // `topology.edgeuse_p1`, a missing edge; the shim refuses both
        Err(SolidError::InvalidInput { .. }) => Ok(Output::err("refused")),
        Err(e) => Ok(rust_err(&e)),
    }
}

fn gen_find_concave(rng: &mut Rng) -> Inputs {
    let brep = gen_topo_brep(rng);
    let walk = rng.chance(0.5);
    let n = 1 + rng.below(6) as usize;
    let eus = (0..n)
        .map(|_| {
            if !walk && rng.chance(0.1) {
                -1 - rng.range_i64(0, 1 << 20)
            } else {
                rng.range_i64(0, 1 << 20)
            }
        })
        .collect();
    Inputs::FindConcave {
        brep,
        v: rng.range_i64(0, 1 << 20),
        eus,
        walk,
    }
}

/// Seed of the face choice of the `corner_adjust_concave_face_*` cases (a generator of
/// its own, so the cases of [`CORNER_SEED`] stay as they were).
const SIDE_FACE_SEED: u64 = 0x5de_f4ce;
/// The loop edge use of a natural plane face along the z axis (the side edge).
const SIDE_FACE_EU: i64 = 3;

/// `i` with its side edge use on a target face's loop (the face's edge along the z axis,
/// as the wire side edge), so the extension finds a side face; `None` for other cases.
fn with_side_face(i: Inputs, rng: &mut Rng) -> Option<Inputs> {
    let Inputs::Corner {
        method: CornerMethod::AdjustConcave,
        mut model,
        corner,
        split,
    } = i
    else {
        return None;
    };
    let v0 = model.vertices.first_mut()?;
    v0.pc_kind = 5;
    v0.pc_index = rng.below(2) as i64;
    v0.pc_sub = SIDE_FACE_EU;
    Some(Inputs::Corner {
        method: CornerMethod::AdjustConcave,
        model,
        corner,
        split,
    })
}

/// A corner case on an Nx2-shaped model (`gen_nx2_model` of unit
/// `blend.fillet_corner_p3`): its target faces are `0` (plane x-z) and `1` (plane y-z),
/// its corner edges `4 5` (plus `2`, a rail, for the three-edge concave case), its
/// fillet vertices `0` (rails meet the side edge), `3`, `4`.
fn gen_corner(rng: &mut Rng) -> Inputs {
    let scale = *rng.choose(&[1.0, 1e-3, 1e3, 0.1, 1e5]).unwrap_or(&1.0);
    let tol = match rng.below(4) {
        0 => 1e-3 * scale,
        1 => 1e-6 * scale,
        2 => 0.0,
        _ => scale * 10f64.powf(rng.uniform(-9.0, -2.0)),
    };
    let (mut model, tsects) = gen_nx2_model(rng, scale);
    let eu = |e: usize, w: usize| -> EuSel { [e, w] };
    let n_all = if rng.chance(0.85) {
        4
    } else {
        rng.below(6) as usize
    };
    let all: Vec<EuSel> = (0..n_all).map(|i| eu(i % 4, usize::from(i >= 4))).collect();
    let solver = Some(vec![eu(1, 0), eu(2, rng.below(2) as usize)]);
    let mut verts: Vec<usize> = vec![0, 3, 4];
    if rng.chance(0.2) {
        verts.reverse();
    }
    if rng.chance(0.1) {
        verts = vec![3, 4];
    }
    let method = if rng.chance(0.5) {
        CornerMethod::AdjustConcave
    } else {
        CornerMethod::UpdateSplitFace
    };
    let (kind, edges, mut orig_faces, split) = match method {
        // (gen_analytic makes the analytic cases)
        CornerMethod::AdjustConcave | CornerMethod::Analytic => {
            let kind = if rng.chance(0.9) {
                7
            } else {
                rng.below(11) as usize
            };
            let edges = if rng.chance(0.85) {
                vec![4, 5, 2]
            } else {
                vec![4, 5]
            };
            (kind, edges, Vec::new(), [0, 1])
        }
        CornerMethod::UpdateSplitFace => {
            // fillet vertices classified on a target face (or with it as their other face)
            for v in model.vertices.iter_mut().skip(1) {
                if rng.chance(0.6) {
                    v.pc_class = 102; // SM_PC_FACE
                    v.pc_kind = 3;
                    v.pc_index = rng.below(2) as i64;
                }
                if rng.chance(0.3) {
                    v.pc_face = rng.below(2) as i64;
                }
            }
            let orig = rng.below(2) as usize;
            let split = [orig, if rng.chance(0.9) { 1 - orig } else { orig }];
            let mut orig_faces = Vec::new();
            for e in [4usize, 5] {
                if rng.chance(0.3) {
                    orig_faces.push([e, rng.below(2) as usize]);
                }
            }
            (rng.below(11) as usize, vec![4, 5], orig_faces, split)
        }
    };
    orig_faces.dedup_by_key(|p| p[0]);
    let blending = rng.chance(0.25);
    Inputs::Corner {
        method,
        model: Box::new(model),
        corner: CornerIn {
            kind,
            v_edge: if rng.chance(0.95) { 1 } else { -1 },
            v_sub: 0,
            solver,
            all: if rng.chance(0.95) { Some(all) } else { None },
            verts,
            edges,
            bevel: !blending,
            blending,
            chamfer: false,
            degenerate: false,
            ext: rng.chance(0.8),
            adjusted: false,
            setback: 0.0,
            tol,
            tan: rng.uniform(1e-4, 0.2),
            tsects,
            orig_faces,
            line_edges: Vec::new(),
            arc_edges: Vec::new(),
        },
        split,
    }
}

/// A rational quadratic arc (exact circle) from `a` to `b` about `c`, at most a half turn
/// short of `PI`: its middle control point is where the end tangents meet, with weight
/// `cos(angle / 2)` (stored weighted, as the shim's `Pw`).
fn arc(c: [f64; 3], a: [f64; 3], b: [f64; 3]) -> Crv {
    let sub = |p: [f64; 3], q: [f64; 3]| [p[0] - q[0], p[1] - q[1], p[2] - q[2]];
    let (u, v) = (sub(a, c), sub(b, c));
    let dot = u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
    let len = (u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt();
    let cos_full = (dot / (len * len)).clamp(-1.0, 1.0);
    let w = ((1.0 + cos_full) / 2.0).sqrt(); // cos(angle / 2)
    // the tangents meet at c + (u + v) / (1 + cos(angle))
    let f = 1.0 / (1.0 + cos_full);
    let m = [
        c[0] + (u[0] + v[0]) * f,
        c[1] + (u[1] + v[1]) * f,
        c[2] + (u[2] + v[2]) * f,
    ];
    Crv {
        dim: 3,
        cpts: vec![
            [a[0], a[1], a[2], 1.0],
            [m[0] * w, m[1] * w, m[2] * w, w],
            [b[0], b[1], b[2], 1.0],
        ],
        p: 2,
        u: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
    }
}

/// The fillet edges of the closed loop through `p` (edge `i` from `p[i]` to the next
/// point), with the type and curve `make` gives for edge `i` from `a` to `b`.
fn ring_edges(
    p: &[[f64; 3]],
    mut make: impl FnMut(usize, [f64; 3], [f64; 3]) -> (i64, Option<Crv>),
) -> Vec<super::blend_fillet_geom_p1::EdgeIn> {
    let n = p.len();
    p.iter()
        .zip(p.iter().cycle().skip(1))
        .enumerate()
        .map(|(i, (&a, &b))| {
            let (etype, crv) = make(i, a, b);
            edge_in(i, (i + 1) % n, etype, crv, [-1, -1])
        })
        .collect()
}

/// A corner for `MakeAnalyticCornerSurface`: fillet edges without fillet geometries (the
/// model's have no solver) whose curves admit (or almost admit) the analytic fits.
/// - a sphere octant: three exact quarter arcs, cross sections or fillet-fillet edges (the
///   sphere through the arcs); one arc may be replaced by its chord (no sphere: the
///   constant-radius fillet, deferred);
/// - a chamfer: three or four straight edges (a plane / a bilinear patch through their
///   start points), or a triangle with a rail-rail interpolating arc on a target face (the
///   arc is recomputed as the plane's intersection with the face);
/// - a four-sided rail-rail corner: two arcs about a common axis and two lines (the
///   surface of revolution, else the variable-radius fillet, which returns before it needs
///   a solver);
/// - degenerate counts (two or five edges), an edge without curve (`SER`: `TRUE`) and a
///   cubic rail-rail interpolation (`FALSE`).
fn gen_analytic(rng: &mut Rng) -> Inputs {
    let scale = *rng.choose(&[1.0, 1e-3, 1e3, 0.1]).unwrap_or(&1.0);
    let r = scale * rng.uniform(0.2, 2.0);
    let c = [
        rng.uniform(-1.0, 1.0) * scale,
        rng.uniform(-1.0, 1.0) * scale,
        rng.uniform(-1.0, 1.0) * scale,
    ];
    let at = |d: [f64; 3]| [c[0] + d[0] * r, c[1] + d[1] * r, c[2] + d[2] * r];
    let tol = match rng.below(3) {
        0 => 1e-3 * scale,
        1 => 1e-6 * scale,
        _ => scale * 10f64.powf(rng.uniform(-9.0, -2.0)),
    };
    let targets = vec![
        line([0.0, 0.0, 0.0], [0.0, 0.0, scale]),
        line([0.0, 0.0, 0.0], [scale, 0.0, 0.0]),
    ];
    let mut chamfer = false;
    let mut orig_faces = Vec::new();
    let shape = rng.below(6);
    let (points, edges, faces): (Vec<[f64; 3]>, Vec<_>, Vec<_>) = match shape {
        // sphere octant
        0 | 1 => {
            let p = [at([1.0, 0.0, 0.0]), at([0.0, 1.0, 0.0]), at([0.0, 0.0, 1.0])];
            let etype = |rng: &mut Rng| *rng.choose(&[1i64, 4, 5]).unwrap_or(&1);
            let chord = if shape == 1 { rng.below(3) as usize } else { 3 };
            let mut edges = ring_edges(&p, |i, a, b| {
                let crv = if i == chord { line(a, b) } else { arc(c, a, b) };
                (etype(rng), Some(crv))
            });
            if rng.chance(0.2) {
                edges.reverse();
            }
            (p.to_vec(), edges, vec![plane_srf(c, [r, 0.0, 0.0], [0.0, r, 0.0])])
        }
        // chamfer: a triangle or a quadrilateral of lines
        2 => {
            chamfer = true;
            let n: u32 = if rng.chance(0.5) { 3 } else { 4 };
            let mut p: Vec<[f64; 3]> = (0..n)
                .map(|i| {
                    let a = std::f64::consts::TAU * f64::from(i) / f64::from(n);
                    at([a.cos(), a.sin(), rng.uniform(-0.3, 0.3)])
                })
                .collect();
            if rng.chance(0.2) {
                p.reverse();
            }
            let edges = ring_edges(&p, |_, a, b| (1, Some(line(a, b))));
            (p, edges, vec![plane_srf(c, [r, 0.0, 0.0], [0.0, r, 0.0])])
        }
        // chamfer: a triangle whose first edge is a rail-rail interpolating arc on target
        // face 0 (the plane z = c.z); the others lines
        3 => {
            chamfer = true;
            let [p0, p1, p2] = [at([1.0, 0.0, 0.0]), at([0.0, 1.0, 0.0]), at([0.3, 0.3, 1.0])];
            let edges = vec![
                edge_in(0, 1, 11, Some(arc(c, p0, p1)), [-1, -1]),
                edge_in(1, 2, 1, Some(line(p1, p2)), [-1, -1]),
                edge_in(2, 0, 1, Some(line(p2, p0)), [-1, -1]),
            ];
            let p = vec![p0, p1, p2];
            if rng.chance(0.85) {
                orig_faces.push([0, 0]);
            }
            (p, edges, vec![plane_srf(c, [2.0 * r, 0.0, 0.0], [0.0, 2.0 * r, 0.0])])
        }
        // four-sided: arcs about the z axis through c at two heights, and two lines; the
        // first arc is the rail-rail interpolation
        4 => {
            let h = r * rng.uniform(0.2, 2.0);
            let r2 = if rng.chance(0.5) { 1.0 } else { rng.uniform(0.5, 1.5) };
            let [c0, c1, c2] = c;
            let [p0, p1, p2, p3] = [
                at([1.0, 0.0, 0.0]),
                at([0.0, 1.0, 0.0]),
                [c0, c1 + r * r2, c2 + h],
                [c0 + r * r2, c1, c2 + h],
            ];
            let top = [c0, c1, c2 + h];
            let cross = *rng.choose(&[1i64, 1, 4]).unwrap_or(&1);
            let edges = vec![
                edge_in(0, 1, 11, Some(arc(c, p0, p1)), [-1, -1]),
                edge_in(1, 2, cross, Some(line(p1, p2)), [-1, -1]),
                edge_in(2, 3, 1, Some(arc(top, p2, p3)), [-1, -1]),
                edge_in(3, 0, cross, Some(line(p3, p0)), [-1, -1]),
            ];
            let p = vec![p0, p1, p2, p3];
            (p, edges, vec![plane_srf(c, [r, 0.0, 0.0], [0.0, r, 0.0])])
        }
        // degenerate: two or five edges, a missing curve, a cubic rail-rail interpolation
        _ => {
            let n = *rng.choose(&[2usize, 3, 5]).unwrap_or(&3);
            let p: Vec<[f64; 3]> = (0..n)
                .map(|i| {
                    let a = std::f64::consts::TAU * (i as f64) / (n as f64);
                    at([a.cos(), a.sin(), 0.0])
                })
                .collect();
            let mut edges = ring_edges(&p, |_, a, b| (1, Some(line(a, b))));
            if let Some(e) = edges.first_mut() {
                match rng.below(3) {
                    0 => e.curve = None,
                    1 => {
                        e.etype = 11;
                        e.status = 6; // SM_FE_CUBIC_RAIL_RAIL_INTERPOLATE
                    }
                    _ => {}
                }
            }
            (p, edges, vec![plane_srf(c, [r, 0.0, 0.0], [0.0, r, 0.0])])
        }
    };
    let n_edges = edges.len();
    // real SmLine / SmCircle curves where the edges are straight / arcs in the XY plane
    // about c (the analytic code dispatches IsLine, IsArc on them): the circle is the
    // quarter from the x to the y direction
    let real = rng.chance(0.5);
    let line_edges: Vec<usize> = if real {
        edges
            .iter()
            .enumerate()
            .filter(|(_, e)| e.curve.as_ref().is_some_and(|c| c.p == 1))
            .map(|(i, _)| i)
            .collect()
    } else {
        Vec::new()
    };
    let arc_from_x = |e: &super::blend_fillet_geom_p1::EdgeIn| {
        e.curve.as_ref().is_some_and(|c| c.p == 2) && e.vs == 0 && e.ve == 1 && (shape <= 1 || shape == 4)
    };
    let arc_edges: Vec<[f64; 7]> = if real {
        edges
            .iter()
            .enumerate()
            .filter(|(_, e)| arc_from_x(e))
            .map(|(i, _)| [i as f64, c[0], c[1], c[2], r, 0.0, 90.0])
            .collect()
    } else {
        Vec::new()
    };
    let model = Model {
        targets,
        faces,
        fillet_brep: Vec::new(),
        vertices: points.iter().map(|&p| vin(p, 8)).collect(),
        geoms: Vec::new(),
        edges,
    };
    Inputs::Corner {
        method: CornerMethod::Analytic,
        model: Box::new(model),
        corner: CornerIn {
            kind: rng.below(11) as usize,
            v_edge: -1,
            v_sub: 0,
            solver: None,
            all: None,
            verts: (0..points.len()).collect(),
            edges: (0..n_edges).collect(),
            bevel: false,
            blending: false,
            chamfer,
            degenerate: false,
            ext: rng.chance(0.5),
            adjusted: false,
            setback: 0.0,
            tol,
            tan: rng.uniform(1e-4, 0.2),
            tsects: Vec::new(),
            orig_faces,
            line_edges,
            arc_edges,
        },
        split: [0, 0],
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "blend.fillet_corner_p2"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        run_cpp(&case.parse::<Inputs>()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        run_rust(&case.parse::<Inputs>()?)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let i = match rng.below(3) {
            0 => gen_find_concave(rng),
            1 => gen_corner(rng),
            _ => gen_analytic(rng),
        };
        serde_json::to_value(i).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let mut v = default_cases(self, ctx)?;
        let push = |v: &mut Vec<Case>, name: String, upstream_ref: &str, i: Inputs| {
            let index = v.len();
            v.push(Case {
                unit: "blend.fillet_corner_p2".to_string(),
                name,
                source: crate::CaseSource::Fixture {
                    path: "built-in: blend.fillet_corner_p2 cases".to_string(),
                    index,
                    upstream_ref: Some(upstream_ref.to_string()),
                },
                inputs: serde_json::to_value(i).map_err(|e| harness(e.to_string()))?,
            });
            Ok::<(), HarnessError>(())
        };
        for file in CORNER_FILES {
            for vertex in 0..12 {
                for walk in [true, false] {
                    let i = Inputs::FindConcave {
                        brep: BrepIn::File {
                            file: file.to_string(),
                            rebuild: false,
                        },
                        v: vertex,
                        // the walk from the first edge use of the ring, or the whole ring
                        // in export order
                        eus: if walk { vec![0] } else { Vec::new() },
                        walk,
                    };
                    let name = format!(
                        "find_concave_{}_v{vertex}_{}",
                        file.trim_start_matches("Fillet/").trim_end_matches(".smb"),
                        if walk { "walk" } else { "ring" }
                    );
                    push(&mut v, name, "TestFiles/pt_TestFiles/Fillet", i)?;
                }
            }
        }
        // Wire edge uses (prog_test my_test_topology's network): their radial is the shell
        // edge use's next one, a branch the fillet files' loop edge uses never take.
        for vertex in 0..4 {
            let i = Inputs::FindConcave {
                brep: BrepIn::Network,
                v: vertex,
                eus: Vec::new(),
                walk: false,
            };
            let name = format!("find_concave_network_v{vertex}_ring");
            push(&mut v, name, "tests/prog_test (my_test_topology)", i)?;
        }
        let mut rng = Rng::new(CORNER_SEED);
        for n in 0..CORNER_CASES {
            let i = gen_corner(&mut rng);
            let Inputs::Corner { method, .. } = &i else {
                return Err(harness("corner case"));
            };
            let name = match method {
                CornerMethod::AdjustConcave => format!("corner_adjust_concave_{n:02}"),
                CornerMethod::UpdateSplitFace => format!("corner_update_split_face_{n:02}"),
                CornerMethod::Analytic => format!("corner_analytic_{n:02}"),
            };
            push(
                &mut v,
                name,
                "SmFilletCorner.cpp (Nx2-shaped corner model)",
                i,
            )?;
        }
        let mut rng = Rng::new(ANALYTIC_SEED);
        for n in 0..ANALYTIC_CASES {
            let name = format!("corner_analytic_{n:02}");
            let src = "SmFilletCorner.cpp:3379 (MakeAnalyticCornerSurface on arc/line corners)";
            push(&mut v, name, src, gen_analytic(&mut rng))?;
        }
        // the `corner` cases again (same stream) with the side edge use on a face
        let mut rng = Rng::new(CORNER_SEED);
        for n in 0..CORNER_CASES {
            let i = gen_corner(&mut rng);
            let mut face_rng = Rng::new(SIDE_FACE_SEED.wrapping_add(n as u64));
            if let Some(i) = with_side_face(i, &mut face_rng) {
                let name = format!("corner_adjust_concave_face_{n:02}");
                let src = "SmFilletCorner.cpp:7140 (Nx2 corner, side edge use on a face)";
                push(&mut v, name, src, i)?;
            }
        }
        let (drawn, base) = tangent_cases();
        for (n, inner) in drawn.into_iter().enumerate() {
            let name = format!("p1_corner_tangent_solver_{n:02}");
            let src = "SmFilletCorner.cpp (blend.fillet_corner_p1 corner model)";
            push(&mut v, name, src, Inputs::P1Corner { inner })?;
        }
        let base = base.ok_or_else(|| harness("no tangent-solver base case"))?;
        for (name, inner) in tangent_variants(&base) {
            let name = format!("p1_corner_tangent_{name}");
            let src = "SmFilletCorner.cpp:5141, :6869 (targeted G1 cases)";
            push(&mut v, name, src, Inputs::P1Corner { inner })?;
        }
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The targeted variants reach every return of both overrides (Rust side; the
    /// difftest compares the C++ side): the solver's registration index, `-1` for NULL.
    #[test]
    fn tangent_variants_return_solvers() {
        let (_, base) = tangent_cases();
        let base = base.unwrap();
        let expect = [
            ("nx2_g1_query0", 1.0),
            ("nx2_g1_query1", 0.0),
            ("nx2_g1_query2", -1.0),
            ("nx2_g1_one_registered_query0", -1.0),
            ("nx2_g1_one_registered_query1", -1.0),
            ("nx2_not_g1_query0", -1.0),
            ("nx2_not_g1_query1", -1.0),
            ("nx2_not_g1_query2", -1.0),
            ("nx2_not_g1_one_registered_query0", -1.0),
            ("nx2_not_g1_one_registered_query1", -1.0),
            ("nx1_closed_g1_query0", 0.0),
            ("nx1_closed_g1_query1", -1.0),
            ("nx1_closed_g1_other_registered", -1.0),
            ("nx1_closed_not_g1_query0", -1.0),
            ("nx1_closed_not_g1_query1", -1.0),
            ("nx1_closed_not_g1_other_registered", -1.0),
        ];
        let variants = tangent_variants(&base);
        assert_eq!(variants.len(), expect.len());
        for ((name, inner), (want_name, want)) in variants.into_iter().zip(expect) {
            assert_eq!(name, want_name);
            let o = run_rust(&Inputs::P1Corner { inner }).unwrap();
            let got = o.values.get("solvers").and_then(|v| v.first().copied());
            assert_eq!(got, Some(want), "{name}: {o:?}");
        }
    }

    #[test]
    fn inputs_round_trip() {
        let mut rng = Rng::new(7);
        for _ in 0..100 {
            let v = Runner.generate(&mut rng).unwrap();
            let back: Inputs =
                serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("{e}: {v}"));
            assert_eq!(serde_json::to_value(back).unwrap(), v);
        }
    }
}
