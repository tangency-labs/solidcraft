//! Runner for unit `blend.fillet_corner_p3`: the fillet corner classification
//! (`SmFilletCorner::Create`) and the corner operations (`CalcCornerGeom`, the virtual
//! `CreateCornerPatch`, `AdjustTopology` and `AdjustTangentCorner`).
//!
//! C++: `solidcore_ffi::units::blend_fillet_corner_p3` (upstream
//! `source/SMLib/src/SmFilletCorner.cpp`). Rust: `solidcore::blend::fillet_corner::p3`.
//!
//! Two kinds of case:
//!
//! - **`create`**: a real B-rep (unit `topology.edgeuse_p1`'s B-rep block: upstream
//!   `.smb` files, among them the fillet test files of `TestFiles/pt_TestFiles/Fillet`,
//!   wire edges, faces), a start edge use (selector), more filleted edge uses (selectors
//!   into the edge uses that start at the start edge use's vertex) and a fillet solver per
//!   filleted edge use (approximation and tangency tolerances, cross-section kind). The
//!   C++ side builds the B-rep, the Rust side imports the shim's export. Compared: whether
//!   a corner was made, its type, all edge uses around the vertex (export indices),
//!   tolerances, flags, setback and member counts, and the closed Nx1 corner's `m_bG1Case`.
//! - **`corner`**: `blend.fillet_geom_p1`'s pseudo-B-rep model (built the same way on both
//!   sides), a corner of a given derived class on it (fillet vertices, fillet edges, edge
//!   uses of the model's target wire edges, flags, tolerances, intersection points of
//!   vertex uses with their user surfaces) and one operation. Compared: the status, the
//!   corner's members and the model's whole export.
//!
//! Deferred on the Rust side (unported callees, `Output::deferred`): `Create` reaching
//! `sm_TestG1Continuity` / `CheckNx1Degeneracy` (`blend.fillet_corner_p1`) or
//! `sm_FindConcaveEdgeuses` (`blend.fillet_corner_p2`); every patch builder past its state
//! checks (`MakeAnalyticCornerSurface`, `CreateEndPatch`, `SmSurface::DropCurve`); the
//! Nx2 adjustments where they change the topology (`MakeFilletEdge`,
//! `SplitFilletGeomAtSideEdge` once a vertex drops inside a rail), the big-radius test of blending corners and the `NxN` /
//! closed Nx1 overrides. The shim does not call upstream on those paths (`-3`; a Rust
//! side that is not deferred there fails the case) and refuses (`-2`, a rejected case)
//! the inputs that make upstream dereference null or free the harness's objects. A
//! `create` case whose sector convexity test runs on geometry the import holds only
//! approximately (analytic surfaces and curves) is deferred like
//! `topology.edgeuse_p2`'s (`inexact_geometry`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::blend::fillet_corner::p3::{
    CornerEnv, FilletCorner, FilletCornerKind, FilletExecutiveData,
};
use solidcore::blend::fillet_geom::p1::{FilletCornerType, FilletSolverId};
use solidcore::blend::fillet_solver::p1::{FilletSolver, FilletSurfaceGenerator};
use solidcore::intersect::surface_intersector::p1::SrfIndex;
use solidcore::kernel::Kernel;
use solidcore::math::EFF_ZERO_RAD;
use solidcore::surface::surface::Surface;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::{Brep, EdgeuseId, VertexId, VertexuseId};
use solidcore::SolidError;
use solidcore_ffi::units::blend_fillet_corner_p3::{self as ffi, Op};
use solidcore_ffi::units::topology_edgeuse_p1 as ffi1;

use super::blend_fillet_geom_p1::{
    gen_model, push_model, status, Crv, EdgeIn, EuIn, GeomIn, Model, RustModel,
    Srf, VertexIn,
};
use super::topology_edgeuse_p1::{
    brep_block_of, gen_brep, import_with_types, path_of, sel, BrepIn, ImportTypes, FILES,
};
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::flag;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 400;

/// "Upstream would dereference null": the shim refused the input.
const REFUSED: f64 = -2.0;
/// The shim did not run upstream (the Rust side defers).
const NOT_RUN: f64 = -3.0;

/// The runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs --------------------------------------------------------------------------

/// A filleted edge use's solver: tolerances and cross-section kind (`0` base, `1`
/// linear, `2` circular, `3` blend curve).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SolverIn {
    approx: f64,
    tang: f64,
    #[serde(default)]
    fsg: u8,
}

/// An edge use of the model's target wire edge `e`: `0` its primary edge use, `1` the
/// mate.
type EuSel = [usize; 2];

/// An intersection point set on the vertex use of pseudo edge `e`'s primary edge use
/// (`which` 0) or mate (1): curve parameter, UV positions, user surfaces (target faces,
/// `-1` none).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TsectIn {
    e: usize,
    which: usize,
    t: f64,
    uv0: [f64; 2],
    uv1: [f64; 2],
    s0: i64,
    s1: i64,
}

/// The corner of a `corner` case.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)] // the corner's independent SmBoolean members
struct CornerIn {
    /// The derived class (order of [`FilletCornerKind`]).
    kind: usize,
    /// The filleted vertex: start (`v_sub` 0) or end vertex of target edge `v_edge`
    /// (`-1` none).
    v_edge: i64,
    #[serde(default)]
    v_sub: usize,
    solver: Option<Vec<EuSel>>,
    all: Option<Vec<EuSel>>,
    #[serde(default)]
    verts: Vec<usize>,
    #[serde(default)]
    edges: Vec<usize>,
    #[serde(default)]
    bevel: bool,
    #[serde(default)]
    blending: bool,
    #[serde(default)]
    chamfer: bool,
    #[serde(default)]
    degenerate: bool,
    #[serde(default = "yes")]
    ext: bool,
    #[serde(default)]
    adjusted: bool,
    #[serde(default)]
    setback: f64,
    tol: f64,
    tan: f64,
    #[serde(default)]
    tsects: Vec<TsectIn>,
    /// `SetOriginalFace` of pseudo edges: `[edge, target face]`.
    #[serde(default)]
    orig_faces: Vec<[usize; 2]>,
    /// Pseudo edges whose curve becomes a real `SmLine` through its end points.
    #[serde(default)]
    line_edges: Vec<usize>,
    /// Pseudo edges whose curve becomes a real `SmCircle` in a plane parallel to XY:
    /// `[edge, cx, cy, cz, radius, d0, d1]` (domain in degrees).
    #[serde(default)]
    arc_edges: Vec<[f64; 7]>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum CornerOp {
    CalcGeom,
    Patch,
    AdjustTopology,
    AdjustTangent,
}

impl CornerOp {
    const fn ffi(self) -> Op {
        match self {
            Self::CalcGeom => Op::CalcGeom,
            Self::Patch => Op::Patch,
            Self::AdjustTopology => Op::AdjustTopology,
            Self::AdjustTangent => Op::AdjustTangent,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "case", rename_all = "snake_case")]
enum Inputs {
    Create {
        brep: BrepIn,
        #[serde(default)]
        validate_only: bool,
        #[serde(default)]
        init_gaps: bool,
        /// The start edge use (selector over the export's edge uses).
        eu: i64,
        /// More filleted edge uses: selectors into the edge uses starting at the start
        /// edge use's vertex (duplicates dropped).
        #[serde(default)]
        more: Vec<i64>,
        /// Per filleted edge use (cycled).
        solvers: Vec<SolverIn>,
    },
    Corner {
        op: CornerOp,
        model: Model,
        corner: CornerIn,
    },
}

const KINDS: [FilletCornerKind; 11] = [
    FilletCornerKind::OneByOne,
    FilletCornerKind::TwoByTwo,
    FilletCornerKind::NByOneClosed,
    FilletCornerKind::NByOne,
    FilletCornerKind::Degenerate,
    FilletCornerKind::Open,
    FilletCornerKind::ThreeByTwoMixed,
    FilletCornerKind::NByTwo,
    FilletCornerKind::FourByThree,
    FilletCornerKind::ConvexNByN,
    FilletCornerKind::ConcaveNByN,
];

// ---- create ----------------------------------------------------------------------------

/// The B-rep of a `create` case on both sides: the shim's export, its Rust import, the
/// vertex and the filleted edge uses.
struct CreateSetup {
    path: String,
    block: Vec<f64>,
    brep: Brep,
    types: ImportTypes,
    vertex: VertexId,
    eus: Vec<EdgeuseId>,
}

fn eu_vertex(b: &Brep, eu: EdgeuseId) -> Option<VertexId> {
    let vu = b.edgeuse(eu).ok()?.vertexuse?;
    b.vertexuse(vu).ok()?.vertex
}

fn create_setup(
    brep: &BrepIn,
    validate_only: bool,
    init_gaps: bool,
    eu: i64,
    more: &[i64],
) -> Result<Result<CreateSetup, Output>, HarnessError> {
    let path = path_of(Some(brep))?;
    let block = brep_block_of(Some(brep), validate_only, init_gaps)?;
    let ex = crate::export_memo::export("topology_edgeuse_p1", &[&path], &block, || {
        ffi1::eval(ffi1::Op::Export, &path, &block)
    })
    .map_err(|e| harness(format!("c++ export: {e}")))?;
    let Some(c) = ffi1::counts(&ex) else {
        let st = ex.get(1).copied().unwrap_or(0.0);
        return Ok(Err(Output::err(format!("brep_build_status_{st}"))));
    };
    let n_eu = c.get(6).copied().unwrap_or(0);
    let s = sel(eu, n_eu);
    if s < 0 {
        return Ok(Err(Output::err("no_edge_use")));
    }
    let (b, types) = import_with_types(&ex)?;
    let start = EdgeuseId::from_index(u32::try_from(s).map_err(|_| harness("index"))?);
    let Some(vertex) = eu_vertex(&b, start) else {
        return Ok(Err(Output::err("start_edge_use_without_vertex")));
    };
    // the other edge uses starting at the vertex, in export order
    let ring: Vec<EdgeuseId> = b
        .edgeuses()
        .iter()
        .map(|(id, _)| id)
        .filter(|&id| id != start && eu_vertex(&b, id) == Some(vertex))
        .collect();
    let mut eus = vec![start];
    for &m in more {
        let r = sel(m, ring.len());
        if let Some(&id) = usize::try_from(r).ok().and_then(|i| ring.get(i)) {
            if !eus.contains(&id) {
                eus.push(id);
            }
        }
    }
    Ok(Ok(CreateSetup {
        path,
        block,
        brep: b,
        types,
        vertex,
        eus,
    }))
}

fn solver_of(solvers: &[SolverIn], i: usize) -> Result<&SolverIn, HarnessError> {
    if solvers.is_empty() {
        return Err(harness("no solver"));
    }
    solvers
        .get(i % solvers.len())
        .ok_or_else(|| harness("solver"))
}

/// The record of a corner made by `Create` (layout of the shim).
fn create_output(rec: &[f64]) -> Result<Output, HarnessError> {
    let first = rec.first().copied().ok_or_else(|| harness("empty record"))?;
    if first == REFUSED {
        return Ok(Output::rejected("upstream would dereference null"));
    }
    if first == NOT_RUN {
        return Ok(Output::err("not_run_by_cpp"));
    }
    let mut out = Output::new().int("corner", first as i64);
    if first == 0.0 {
        if rec.len() != 1 {
            return Err(harness("trailing values"));
        }
        return Ok(out);
    }
    let get = |i: usize| rec.get(i).copied().ok_or_else(|| harness("record too short"));
    out = out.int("type", get(1)? as i64);
    let n = get(2)?;
    let n = usize::try_from(n as i64).map_err(|_| harness("bad count"))?;
    let all: Vec<i64> = (0..n).map(|i| get(3 + i).map(|x| x as i64)).collect::<Result<_, _>>()?;
    out = out.ints("all_eus", all);
    let base = 3 + n;
    out = out.values("tols", vec![get(base)?, get(base + 1)?]);
    let flags: Vec<i64> = (0..7).map(|i| get(base + 2 + i).map(|x| x as i64)).collect::<Result<_, _>>()?;
    out = out.ints("flags", flags);
    out = out.values("setback", vec![get(base + 9)?]);
    let counts: Vec<i64> = (0..3).map(|i| get(base + 10 + i).map(|x| x as i64)).collect::<Result<_, _>>()?;
    out = out.ints("counts", counts);
    out = out.int("g1", get(base + 13)? as i64);
    if rec.len() != base + 14 {
        return Err(harness("trailing values"));
    }
    Ok(out)
}

fn create_cpp(
    brep: &BrepIn,
    validate_only: bool,
    init_gaps: bool,
    eu: i64,
    more: &[i64],
    solvers: &[SolverIn],
) -> Result<Output, HarnessError> {
    let s = match create_setup(brep, validate_only, init_gaps, eu, more)? {
        Ok(s) => s,
        Err(o) => return Ok(o),
    };
    let mut input = s.block.clone();
    input.push(f64::from(s.vertex.index()));
    input.push(s.eus.len() as f64);
    for (i, eu) in s.eus.iter().enumerate() {
        let sv = solver_of(solvers, i)?;
        input.extend([f64::from(eu.index()), sv.approx, sv.tang, f64::from(sv.fsg)]);
    }
    let out = ffi::create(&s.path, &input).map_err(|e| harness(format!("c++: {e}")))?;
    let body = out.get(1..).ok_or_else(|| harness("c++: empty output"))?;
    create_output(body)
}

fn fsg_of(kind: u8) -> FilletSurfaceGenerator {
    match kind {
        1 => FilletSurfaceGenerator::Linear,
        2 => FilletSurfaceGenerator::circular_default(),
        3 => FilletSurfaceGenerator::blend_curve_default(),
        _ => FilletSurfaceGenerator::Base,
    }
}

/// `SmBSplineSurface_TYPE`, `SmPlane_TYPE`; `SmBSplineCurve_TYPE`, `SmLine_TYPE` and
/// `SmCrvOnSurf_TYPE` (as `topology.edgeuse_p2`'s runner).
const EXACT_SURFACES: [i64; 2] = [15002, 15011];
const EXACT_CURVES: [i64; 3] = [14033, 14010, 14048];

/// The deferral of a sector convexity test over geometry the import holds only as its
/// B-spline part (see `topology.edgeuse_p2`'s runner).
fn inexact(b: &Brep, types: &ImportTypes, eus: &[EdgeuseId]) -> Option<Output> {
    let mut all = Vec::new();
    for &eu in eus {
        all.push(eu);
        if let Ok(Some(r)) = eu1::radial(b, eu) {
            all.push(r);
        }
    }
    for eu in all {
        let st = eu1::face(b, eu)
            .ok()
            .flatten()
            .and_then(|f| types.inexact_surface(f.index() as usize))
            .unwrap_or(-1);
        if st >= 0 && !EXACT_SURFACES.contains(&st) {
            let (unit, symbol) = match st {
                // the gap is the missing `SurfaceKind` variant (the `SmSurface` dispatch,
                // unit `surface.surface_p2`; #76), not the kinds' own units (all ported)
                15012 | 15013 => ("surface.surface_p2", "SurfaceKind::Cone (SmCone)"),
                15014 => ("surface.surface_p2", "SurfaceKind::Sphere (SmSphere)"),
                15015 => ("surface.surface_p2", "SurfaceKind::Torus (SmTorus)"),
                15030 => (
                "harness:analytic-geometry-export",
                "SmSurfOfExtrusion face surface not exported exactly",
            ),
                _ => ("topology.brep_p10", "SmBrep::MakeTopologyFromData"),
            };
            return Some(Output::deferred(unit, symbol));
        }
        let ct = eu1::edge(b, eu)
            .ok()
            .flatten()
            .and_then(|e| types.edge_curve_types.get(e.index() as usize).copied())
            .unwrap_or(-1);
        if ct >= 0 && !EXACT_CURVES.contains(&ct) {
            return Some(Output::deferred("topology.brep_p10", "SmBrep::MakeTopologyFromData"));
        }
    }
    None
}

fn create_rust(
    brep: &BrepIn,
    validate_only: bool,
    init_gaps: bool,
    eu: i64,
    more: &[i64],
    solvers: &[SolverIn],
) -> Result<Output, HarnessError> {
    let mut s = match create_setup(brep, validate_only, init_gaps, eu, more)? {
        Ok(s) => s,
        Err(o) => return Ok(o),
    };
    let k = Kernel::default();
    let mut exec = FilletExecutiveData::default();
    let mut table = Vec::new();
    for (i, &id) in s.eus.iter().enumerate() {
        let sv = solver_of(solvers, i)?;
        let mut fs = FilletSolver::new(sv.approx, EFF_ZERO_RAD, sv.tang);
        fs.fsg = Some(fsg_of(sv.fsg));
        exec.edgeuse_solver_map
            .insert(id, FilletSolverId(u32::try_from(i).map_err(|_| harness("index"))?));
        table.push(fs);
    }
    let r = FilletCorner::create(&k, &mut s.brep, s.vertex, &s.eus, &exec, &table);
    let c = match r {
        Ok(c) => c,
        Err(e) => {
            return Ok(Output::unported(&e).unwrap_or_else(|| Output::err(format!("rust_error_{}", status(&e)))))
        }
    };
    let Some(c) = c else {
        return Ok(Output::new().int("corner", 0));
    };
    let all = c.all_eus.clone().unwrap_or_default();
    // the convexity test ran (two filleted edge uses, three edges around the vertex)
    if s.eus.len() == 2 && all.len() == 3 {
        if let Some(o) = inexact(&s.brep, &s.types, &s.eus) {
            return Ok(o);
        }
    }
    let mut rec = vec![1.0, c.corner_type() as i64 as f64, all.len() as f64];
    rec.extend(all.iter().map(|e| f64::from(e.index())));
    rec.extend([c.approx_tol_3d, c.tangency_tol_radians]);
    rec.extend([
        flag(c.bevel),
        flag(c.blending),
        flag(c.chamfer),
        flag(c.degenerate),
        flag(c.calc_as_const_rad),
        flag(c.topology_adjusted),
        flag(c.extended_surface_patch),
    ]);
    rec.push(c.set_back_dist);
    rec.extend([c.vertices.len() as f64, c.edges.len() as f64, c.surfaces.len() as f64]);
    rec.push(if c.corner_type() == FilletCornerType::NByOneClosed {
        flag(c.g1_case)
    } else {
        -1.0
    });
    create_output(&rec)
}

// ---- corner ----------------------------------------------------------------------------

fn push_eus(o: &mut Vec<f64>, l: Option<&Vec<EuSel>>) {
    o.push(flag(l.is_some()));
    let l = l.map_or(&[][..], Vec::as_slice);
    o.push(l.len() as f64);
    for [e, w] in l {
        o.extend([*e as f64, *w as f64]);
    }
}

fn corner_input(model: &Model, c: &CornerIn) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    push_model(&mut o, model)?;
    o.extend([c.kind as f64, c.v_edge as f64, c.v_sub as f64]);
    push_eus(&mut o, c.solver.as_ref());
    push_eus(&mut o, c.all.as_ref());
    o.push(c.verts.len() as f64);
    o.extend(c.verts.iter().map(|&v| v as f64));
    o.push(c.edges.len() as f64);
    o.extend(c.edges.iter().map(|&e| e as f64));
    o.extend([
        flag(c.bevel),
        flag(c.blending),
        flag(c.chamfer),
        flag(c.degenerate),
        flag(c.ext),
        flag(c.adjusted),
    ]);
    o.extend([c.setback, c.tol, c.tan]);
    o.push(c.tsects.len() as f64);
    for t in &c.tsects {
        o.extend([t.e as f64, t.which as f64, t.t]);
        o.extend(t.uv0);
        o.extend(t.uv1);
        o.extend([t.s0 as f64, t.s1 as f64]);
    }
    o.push(c.orig_faces.len() as f64);
    for [e, f] in &c.orig_faces {
        o.extend([*e as f64, *f as f64]);
    }
    o.push(c.line_edges.len() as f64);
    o.extend(c.line_edges.iter().map(|&e| e as f64));
    o.push(c.arc_edges.len() as f64);
    for a in &c.arc_edges {
        o.extend(a);
    }
    Ok(o)
}

/// Splits a corner record: `status`, the corner members, the model export.
fn corner_output(rec: &[f64]) -> Result<Output, HarnessError> {
    let first = rec.first().copied().ok_or_else(|| harness("empty record"))?;
    if first == REFUSED {
        return Ok(Output::rejected("upstream would dereference null"));
    }
    if first == NOT_RUN {
        return Ok(Output::err("not_run_by_cpp"));
    }
    let get = |i: usize| rec.get(i).copied().ok_or_else(|| harness("record too short"));
    let mut at = 1;
    let nv = usize::try_from(get(at)? as i64).map_err(|_| harness("count"))?;
    let verts: Vec<f64> = (0..nv).map(|i| get(at + 1 + i)).collect::<Result<_, _>>()?;
    at += 1 + nv;
    let ne = usize::try_from(get(at)? as i64).map_err(|_| harness("count"))?;
    let edges: Vec<f64> = (0..ne).map(|i| get(at + 1 + i)).collect::<Result<_, _>>()?;
    at += 1 + ne;
    let flags: Vec<f64> = (0..7).map(|i| get(at + i)).collect::<Result<_, _>>()?;
    at += 7;
    let setback = get(at)?;
    let nsurf = get(at + 1)?;
    at += 2;
    let model = rec.get(at..).ok_or_else(|| harness("record too short"))?.to_vec();
    Ok(Output::new()
        .int("status", first as i64)
        .values("corner_verts", verts)
        .values("corner_edges", edges)
        .values("corner_flags", flags)
        .values("setback", vec![setback])
        .int("surfaces", nsurf as i64)
        .values("model", model))
}

fn corner_cpp(op: CornerOp, model: &Model, c: &CornerIn) -> Result<Output, HarnessError> {
    let input = corner_input(model, c)?;
    let flat = ffi::corner(op.ffi(), &input).map_err(|e| harness(e.to_string()))?;
    corner_output(&flat)
}

fn corner_rust(k: &Kernel, op: CornerOp, model: &Model, ci: &CornerIn) -> Result<Output, HarnessError> {
    let h = |e: SolidError| harness(e.to_string());
    let mut rm = RustModel::build(model)?;
    let kind = *KINDS.get(ci.kind).ok_or_else(|| harness("kind"))?;
    let target_eu = |rm: &RustModel, [e, w]: EuSel| -> Result<EdgeuseId, HarnessError> {
        let edge = *rm.target_edges.get(e).ok_or_else(|| harness("target edge"))?;
        let prim = eu1::primary_edgeuse(&rm.target, edge)
            .map_err(h)?
            .ok_or_else(|| harness("primary edge use"))?;
        if w == 1 {
            eu1::mate(&rm.target, prim).map_err(h)?.ok_or_else(|| harness("mate"))
        } else {
            Ok(prim)
        }
    };
    let list = |rm: &RustModel, l: Option<&Vec<EuSel>>| -> Result<Option<Vec<EdgeuseId>>, HarnessError> {
        l.map(|l| l.iter().map(|&s| target_eu(rm, s)).collect()).transpose()
    };
    let vertex = match usize::try_from(ci.v_edge) {
        Ok(e) => {
            let edge = *rm.target_edges.get(e).ok_or_else(|| harness("target edge"))?;
            let [vs, ve] = rm.target.edge(edge).map_err(h)?.vertices;
            Some(if ci.v_sub == 0 { vs } else { ve })
        }
        Err(_) => None,
    };
    let solver_eus = list(&rm, ci.solver.as_ref())?;
    let all_eus = list(&rm, ci.all.as_ref())?;
    let mut c = FilletCorner::new(kind, vertex, solver_eus, all_eus, ci.tol, ci.tan);
    for &v in &ci.verts {
        c.vertices.push(rm.vertex(v)?);
    }
    for &e in &ci.edges {
        c.edges.push(rm.edge(e)?);
    }
    c.bevel = ci.bevel;
    c.blending = ci.blending;
    c.chamfer = ci.chamfer;
    c.degenerate = ci.degenerate;
    c.extended_surface_patch = ci.ext;
    c.topology_adjusted = ci.adjusted;
    c.set_back_dist = ci.setback;

    let mut surfaces: BTreeMap<VertexuseId, [Option<Surface>; 2]> = BTreeMap::new();
    for t in &ci.tsects {
        let eu = rm.edgeuse(t.e, t.which)?;
        let vu = rm
            .fb
            .brep()
            .edgeuse(eu)
            .map_err(h)?
            .vertexuse
            .ok_or_else(|| harness("edge use without vertex use"))?;
        {
            let tp = rm.fb.fillet_vertexuse_mut(vu).map_err(h)?.tsect_pnt_mut();
            tp.curve_parameter = t.t;
            *tp.uv_pos_mut(SrfIndex::First) = solidcore::geom::vector2d::Point2::new(t.uv0[0], t.uv0[1]);
            *tp.uv_pos_mut(SrfIndex::Second) = solidcore::geom::vector2d::Point2::new(t.uv1[0], t.uv1[1]);
        }
        let srf = |rm: &RustModel, s: i64| -> Result<Option<Surface>, HarnessError> {
            match usize::try_from(s) {
                Ok(i) if s < 1000 => {
                    let f = *rm.target_faces.get(i).ok_or_else(|| harness("face"))?;
                    Ok(rm.target.face(f).map_err(h)?.surface.clone())
                }
                // a real SmPlane (shim: p3_user_srf)
                _ if s == 1000 || s == 1001 => {
                    let (x, y, sc) = if s == 1000 {
                        ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0])
                    } else {
                        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [2.0, 0.5])
                    };
                    let v = |a: [f64; 3]| solidcore::math::Vec3::new(a[0], a[1], a[2]);
                    let dom = solidcore::geom::extent2d::Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
                    Ok(Some(Surface::from(solidcore::surface::plane::Plane::new(
                        v([0.0, 0.0, 0.0]),
                        v(x),
                        v(y),
                        solidcore::geom::vector2d::Vec2::new(sc[0], sc[1]),
                        &dom,
                    ))))
                }
                _ => Ok(None),
            }
        };
        surfaces.insert(vu, [srf(&rm, t.s0)?, srf(&rm, t.s1)?]);
    }

    for &[e, f] in &ci.orig_faces {
        let eid = rm.edge(e)?;
        let face = *rm.target_faces.get(f).ok_or_else(|| harness("face"))?;
        rm.fb.fillet_edge_mut(eid).map_err(h)?.set_original_face(Some(face));
    }
    for &e in &ci.line_edges {
        let eid = rm.edge(e)?;
        let ends = rm
            .fb
            .brep()
            .edge(eid)
            .map_err(h)?
            .curve
            .as_ref()
            .ok_or_else(|| harness("line edge without curve"))?
            .ends(k)
            .map_err(h)?;
        let mut l = solidcore::curve::line::Line::from_points(ends.start, ends.end, 3);
        let _ = l.make_nurb();
        rm.fb.brep_mut().edge_mut(eid).map_err(h)?.curve = Some(solidcore::curve::curve::Curve::from(l));
    }
    for &[e, cx, cy, cz, r, d0, d1] in &ci.arc_edges {
        let eid = rm.edge(e as usize)?;
        let v = solidcore::math::Vec3::new;
        let circle = solidcore::curve::ellipse::Ellipse::new_circle(
            &rm.k,
            v(cx, cy, cz),
            v(1.0, 0.0, 0.0),
            v(0.0, 1.0, 0.0),
            solidcore::geom::extent1d::Extent1d::from_raw(d0, d1),
            r,
            &solidcore::curve::ellipse::EllipseOptions::default(),
        )
        .map_err(h)?;
        rm.fb.brep_mut().edge_mut(eid).map_err(h)?.curve = Some(solidcore::curve::curve::Curve::from(circle));
    }
    let r = {
        let mut env = CornerEnv {
            k: &rm.k,
            target: &mut rm.target,
            pseudo: &mut rm.fb,
            corner: None,
            tsect_surfaces: &surfaces,
            // the model's geometries have no solver (as the shim's)
            solvers: &mut [],
        };
        match op {
            CornerOp::CalcGeom => c.calc_corner_geom(k, &mut env),
            CornerOp::Patch => c.create_corner_patch(k, &mut env),
            CornerOp::AdjustTopology => c.adjust_topology(k, &mut env),
            CornerOp::AdjustTangent => c.adjust_tangent_corner(k, &mut env),
        }
    };
    let st = match r {
        Ok(()) => 0.0,
        Err(e) if e.is_unported() => {
            return Ok(Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string())))
        }
        Err(e) => status(&e),
    };
    let mut rec = vec![st, c.vertices.len() as f64];
    for &v in &c.vertices {
        rec.push(rm.vertex_index(Some(v)));
    }
    rec.push(c.edges.len() as f64);
    for &e in &c.edges {
        rec.push(rm.edge_index(Some(e)));
    }
    rec.extend([
        flag(c.bevel),
        flag(c.blending),
        flag(c.chamfer),
        flag(c.degenerate),
        flag(c.calc_as_const_rad),
        flag(c.topology_adjusted),
        flag(c.extended_surface_patch),
    ]);
    rec.push(c.set_back_dist);
    rec.push(c.surfaces.len() as f64);
    rm.put(&mut rec)?;
    corner_output(&rec)
}

// ---- generators --------------------------------------------------------------------------

/// Upstream's fillet test files (`TestFiles/pt_TestFiles/Fillet`) whose corners are
/// classified, plus `topology.edgeuse_p1`'s files.
const FILLET_FILES: [&str; 12] = [
    "Fillet/box1x1x2.smb",
    "Fillet/brick.smb",
    "Fillet/corner_box.smb",
    "Fillet/l_shaped_box.smb",
    "Fillet/6_sided_corner.smb",
    "Fillet/box_w_slot.smb",
    "Fillet/nurb_box.smb",
    "Fillet/filleted_box.smb",
    "Fillet/cyl_on_box.smb",
    "Fillet/box_on_box.smb",
    "Fillet/open_shell.smb",
    "Fillet/rollover_corner.smb",
];

fn gen_solver(rng: &mut Rng, linear: bool) -> SolverIn {
    let approx = match rng.below(6) {
        0 => 1e-3,
        1 => 1e-5,
        2 => 1e-6,
        3 => 0.0,
        _ => 10f64.powf(rng.uniform(-9.0, -1.0)),
    };
    let tang = match rng.below(5) {
        0 => 0.01,
        1 => 0.0,
        _ => rng.uniform(1e-4, 0.5),
    };
    SolverIn {
        approx,
        tang,
        fsg: if linear { 1 } else { rng.below(4) as u8 },
    }
}

fn gen_create(rng: &mut Rng) -> Inputs {
    let brep = if rng.chance(0.7) {
        BrepIn::File {
            file: (*rng.choose(&FILLET_FILES).unwrap_or(&"Fillet/brick.smb")).to_string(),
            rebuild: rng.chance(0.2),
        }
    } else if rng.chance(0.5) {
        BrepIn::File {
            file: (*rng.choose(&FILES).unwrap_or(&"Mouse.smb")).to_string(),
            rebuild: rng.chance(0.2),
        }
    } else {
        gen_brep(rng)
    };
    let n_more = match rng.below(10) {
        0..=1 => 0,
        2..=5 => 1,
        6..=7 => 2,
        _ => rng.below(5) as usize,
    };
    let more = (0..n_more).map(|_| rng.below(1 << 16) as i64).collect();
    let linear = rng.chance(0.35);
    let n_solvers = 1 + rng.below(4) as usize;
    let solvers = (0..n_solvers).map(|_| gen_solver(rng, linear)).collect();
    Inputs::Create {
        brep,
        validate_only: rng.chance(0.05),
        init_gaps: rng.chance(0.1),
        eu: rng.below(1 << 16) as i64,
        more,
        solvers,
    }
}

fn vin(point: [f64; 3], vtype: i64) -> VertexIn {
    VertexIn {
        point,
        vtype,
        status: 0,
        corner: -1,
        pc_class: 100,
        pc_kind: 0,
        pc_index: -1,
        pc_sub: 0,
        pc_face: -1,
        t_param: 0.0,
        uv: [0.0, 0.0],
        mates: [-1, -1],
    }
}

fn eu_in(geom: i64) -> EuIn {
    EuIn {
        uv_curve: None,
        geom,
        vu_uv: [0.0, 0.0],
    }
}

fn line(a: [f64; 3], b: [f64; 3]) -> Crv {
    Crv {
        dim: 3,
        cpts: vec![[a[0], a[1], a[2], 1.0], [b[0], b[1], b[2], 1.0]],
        p: 1,
        u: vec![0.0, 0.0, 1.0, 1.0],
    }
}

fn plane_srf(o: [f64; 3], du: [f64; 3], dv: [f64; 3]) -> Srf {
    let p = |a: f64, b: f64| {
        [
            o[0] + a * du[0] + b * dv[0],
            o[1] + a * du[1] + b * dv[1],
            o[2] + a * du[2] + b * dv[2],
            1.0,
        ]
    };
    Srf {
        net: vec![vec![p(0.0, 0.0), p(0.0, 1.0)], vec![p(1.0, 0.0), p(1.0, 1.0)]],
        p: 1,
        q: 1,
        u: vec![0.0, 0.0, 1.0, 1.0],
        v: vec![0.0, 0.0, 1.0, 1.0],
    }
}

fn edge_in(vs: usize, ve: usize, etype: i64, curve: Option<Crv>, g: [i64; 2]) -> EdgeIn {
    EdgeIn {
        vs,
        ve,
        etype,
        status: 0,
        corner: -1,
        curve,
        interval: None,
        fbe: [-1, -1],
        eus: [eu_in(g[0]), eu_in(g[1])],
    }
}

/// A model shaped like an Nx2 corner: target wire edges (the side edge first, then the
/// two filleted edges, tangent or not, and a fourth), two target faces, a
/// `SM_FV_RAIL_X_EDGEUSE` vertex classified to the side edge's edge use with two rails
/// (one per fillet geometry) leaving it, rails' other ends, other corner vertices and
/// edges between them.
fn gen_nx2_model(rng: &mut Rng, scale: f64) -> (Model, Vec<TsectIn>) {
    let s = scale;
    let tangent = rng.chance(0.6);
    let d2 = if tangent {
        [-1.0, rng.uniform(-1e-3, 1e-3) * f64::from(u8::from(rng.chance(0.5))), 0.0]
    } else {
        [rng.uniform(-1.0, 0.5), 1.0, rng.uniform(-0.5, 0.5)]
    };
    let targets = vec![
        line([0.0, 0.0, 0.0], [0.0, 0.0, s]),           // side edge
        line([0.0, 0.0, 0.0], [s, 0.0, 0.0]),           // filleted edge 1
        line([0.0, 0.0, 0.0], [d2[0] * s, d2[1] * s, d2[2] * s]), // filleted edge 2
        line([0.0, 0.0, 0.0], [0.0, -s, 0.0]),          // fourth edge
    ];
    let faces = vec![
        plane_srf([0.0, 0.0, 0.0], [s, 0.0, 0.0], [0.0, 0.0, s]),
        plane_srf([0.0, 0.0, 0.0], [0.0, s, 0.0], [0.0, 0.0, s]),
    ];
    let r = s * rng.uniform(0.05, 0.3);
    let mut v0 = vin([0.0, 0.0, r], 9);
    v0.pc_class = 109; // SM_PC_EDGEUSE
    v0.pc_kind = 2;
    v0.pc_index = 0;
    let vertices = vec![
        v0,
        vin([s, 0.0, r], 7),
        vin([-s, 0.0, r], 7),
        vin([0.0, r, 0.0], *rng.choose(&[1i64, 8, 9, 4]).unwrap_or(&1)),
        vin([0.0, -r, 0.0], *rng.choose(&[1i64, 9, 13]).unwrap_or(&1)),
    ];
    let rail1 = line([0.0, 0.0, r], [s, 0.0, r]);
    let rail2 = line([0.0, 0.0, r], [-s, 0.0, r]);
    let other1 = line([0.0, r, 0.0], [s, r, 0.0]);
    let other2 = line([0.0, -r, 0.0], [-s, -r, 0.0]);
    let curve_or_none = |rng: &mut Rng, c: Crv| if rng.chance(0.9) { Some(c) } else { None };
    let edges = vec![
        edge_in(0, 1, 9, curve_or_none(rng, rail1), [0, 0]),
        edge_in(0, 2, 9, curve_or_none(rng, rail2), [1, 1]),
        edge_in(3, 1, 9, Some(other1), [0, 0]),
        edge_in(4, 2, 9, Some(other2), [1, 1]),
        edge_in(0, 3, *rng.choose(&[4i64, 1, 5]).unwrap_or(&4), None, [-1, -1]),
        edge_in(3, 4, 4, None, [-1, -1]),
    ];
    let geoms = vec![
        GeomIn {
            gtype: 0,
            status: 0,
            center: None,
            surface: None,
            rails: [0, 2],
            other1: Vec::new(),
            other2: Vec::new(),
            sides: Vec::new(),
        },
        GeomIn {
            gtype: 0,
            status: 0,
            center: None,
            surface: None,
            rails: [1, 3],
            other1: Vec::new(),
            other2: Vec::new(),
            sides: Vec::new(),
        },
    ];
    let model = Model {
        targets,
        faces,
        fillet_brep: Vec::new(),
        vertices,
        geoms,
        edges,
    };
    // intersection points of the rails' vertex uses at vertex 0: the side edge parameters
    // and the user surfaces' UVs (equal, close or apart)
    // side edge parameters: inside its domain, at its ends or past them (upstream
    // ignores the evaluation status)
    let t1 = match rng.below(6) {
        0 => 0.0,
        1 => 1.0,
        2 => rng.uniform(-0.5, 1.5),
        _ => rng.uniform(0.0, 1.0),
    };
    let t2 = match rng.below(3) {
        0 => t1,
        1 => t1 + rng.uniform(-1e-9, 1e-9),
        _ => rng.uniform(0.0, 1.0),
    };
    let uv = [rng.uniform(0.0, 1.0), rng.uniform(0.0, 1.0)];
    let uv_b = [rng.uniform(0.0, 1.0), rng.uniform(0.0, 1.0)];
    let near = |rng: &mut Rng, p: [f64; 2]| match rng.below(3) {
        0 => p,
        1 => [p[0] + rng.uniform(-1e-10, 1e-10), p[1]],
        _ => [rng.uniform(-0.2, 1.2), rng.uniform(-0.2, 1.2)],
    };
    let face = |rng: &mut Rng| if rng.chance(0.95) { rng.below(2) as i64 } else { -1 };
    let (fa, fb) = (face(rng), face(rng));
    let tsects = vec![
        TsectIn {
            e: 0,
            which: 0,
            t: t1,
            uv0: uv,
            uv1: uv_b,
            s0: fa,
            s1: fb,
        },
        TsectIn {
            e: 1,
            which: 0,
            t: t2,
            uv0: near(rng, uv),
            uv1: near(rng, uv_b),
            s0: if rng.chance(0.9) { fa } else { face(rng) },
            s1: if rng.chance(0.9) { fb } else { face(rng) },
        },
    ];
    (model, tsects)
}

/// A model shaped like a 3x2 mixed-convexity corner: four corner edges, one
/// `SM_FE_ON_EDGE` from an `SM_FV_ON_VERTEX` corner vertex and one `SM_FE_ON_EXTEND_EDGE`
/// (B-spline curves meeting or not), an original face on the latter.
fn gen_3x2_model(rng: &mut Rng, scale: f64) -> Model {
    let s = scale;
    let mut verts = vec![
        vin([0.0, 0.0, 0.0], *rng.choose(&[4i64, 4, 4, 9]).unwrap_or(&4)),
        vin([s, 0.0, 0.0], 9),
        vin([s, s, 0.0], 1),
        vin([0.0, s, 0.0], 9),
    ];
    if rng.chance(0.2) {
        verts.push(vin([0.5 * s, 0.5 * s, 0.0], 1));
    }
    let gap = if rng.chance(0.7) { 0.0 } else { s * rng.uniform(1e-6, 0.5) };
    let c_on = gen_join_curve(rng, [0.0, 0.0, 0.0], [s, 0.0, 0.0]);
    let c_ext = gen_join_curve(rng, [-s, 0.0, 0.0], [gap, 0.0, 0.0]);
    let on_type = 6;
    let ext_type = 7;
    let mut edges = vec![
        edge_in(0, 1, on_type, Some(c_on), [-1, -1]),
        edge_in(1, 2, 9, Some(line([s, 0.0, 0.0], [s, s, 0.0])), [-1, -1]),
        edge_in(2, 3, 4, Some(line([s, s, 0.0], [0.0, s, 0.0])), [-1, -1]),
        edge_in(3, 0, ext_type, Some(c_ext), [-1, -1]),
    ];
    if rng.chance(0.15) {
        // the corner vertex starts the extended edge too
        edges.swap(0, 0);
        if let Some(e) = edges.get_mut(3) {
            e.vs = 0;
            e.ve = 3;
        }
    }
    if rng.chance(0.1) {
        if let Some(e) = edges.get_mut(3) {
            e.curve = None;
        }
    }
    Model {
        targets: vec![line([0.0, 0.0, 0.0], [s, 0.0, 0.0])],
        faces: vec![plane_srf([-s, -s, 0.0], [3.0 * s, 0.0, 0.0], [0.0, 3.0 * s, 0.0])],
        fillet_brep: Vec::new(),
        vertices: verts,
        geoms: Vec::new(),
        edges,
    }
}

/// A cubic or linear B-spline from `a` to `b`, sometimes bent.
fn gen_join_curve(rng: &mut Rng, a: [f64; 3], b: [f64; 3]) -> Crv {
    if rng.chance(0.4) {
        return line(a, b);
    }
    let lerp = |t: f64| [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1]), a[2] + t * (b[2] - a[2])];
    let bend = (b[0] - a[0]).abs().max(1e-3) * rng.uniform(-0.3, 0.3);
    let p1 = lerp(1.0 / 3.0);
    let p2 = lerp(2.0 / 3.0);
    Crv {
        dim: 3,
        cpts: vec![
            [a[0], a[1], a[2], 1.0],
            [p1[0], p1[1] + bend, p1[2], 1.0],
            [p2[0], p2[1] - bend, p2[2], 1.0],
            [b[0], b[1], b[2], 1.0],
        ],
        p: 3,
        u: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
    }
}

fn gen_corner(rng: &mut Rng) -> Inputs {
    let scale = *rng.choose(&[1.0, 1e-3, 1e3, 10.0, 0.1, 1e-6, 1e5]).unwrap_or(&1.0);
    let tol = match rng.below(5) {
        0 => 1e-3 * scale,
        1 => 1e-6 * scale,
        2 => 0.0,
        3 => 1e-12,
        _ => scale * 10f64.powf(rng.uniform(-9.0, -2.0)),
    };
    let tan = rng.uniform(1e-4, 0.2);
    let op = *rng
        .choose(&[
            CornerOp::AdjustTopology,
            CornerOp::AdjustTangent,
            CornerOp::Patch,
            CornerOp::CalcGeom,
        ])
        .unwrap_or(&CornerOp::Patch);
    let eu = |e: usize, w: usize| [e, w];
    match rng.below(10) {
        // Nx2 shaped: the adjustments
        0..=5 => {
            let (model, tsects) = gen_nx2_model(rng, scale);
            let n_all = if rng.chance(0.8) { 4 } else { rng.below(6) as usize };
            let all: Vec<EuSel> = (0..n_all).map(|i| eu(i % 4, usize::from(i >= 4))).collect();
            let solver = match rng.below(10) {
                0 => None,
                1 => Some(vec![eu(1, 0)]),
                _ => Some(vec![eu(1, 0), eu(2, rng.below(2) as usize)]),
            };
            let mut verts: Vec<usize> = vec![0, 3, 4];
            if rng.chance(0.2) {
                verts = vec![0];
            }
            if rng.chance(0.2) {
                verts.reverse();
            }
            let edges = match rng.below(4) {
                0 => vec![4, 5, 2],
                _ => vec![4, 5],
            };
            let kind = if rng.chance(0.85) { 7 } else { rng.below(11) as usize };
            let blending = rng.chance(0.25);
            Inputs::Corner {
                op,
                model,
                corner: CornerIn {
                    kind,
                    v_edge: if rng.chance(0.9) { 1 } else { -1 },
                    v_sub: 0,
                    solver,
                    all: if rng.chance(0.95) { Some(all) } else { None },
                    verts,
                    edges,
                    bevel: !blending,
                    blending,
                    chamfer: rng.chance(0.1),
                    degenerate: false,
                    ext: rng.chance(0.8),
                    adjusted: false,
                    setback: 0.0,
                    tol,
                    tan,
                    tsects,
                    orig_faces: Vec::new(),
                    line_edges: Vec::new(),
                    arc_edges: Vec::new(),
                },
            }
        }
        // 3x2 shaped: its patch
        6..=7 => {
            let model = gen_3x2_model(rng, scale);
            let nv = model.vertices.len();
            let mut verts: Vec<usize> = (0..nv).collect();
            if rng.chance(0.3) {
                verts.retain(|&v| v != 0);
            }
            let mut edges: Vec<usize> = vec![0, 1, 2, 3];
            if rng.chance(0.15) {
                edges.pop();
            }
            if rng.chance(0.15) {
                edges.swap(0, 2);
            }
            let kind = if rng.chance(0.8) { 6 } else { rng.below(11) as usize };
            Inputs::Corner {
                op: if rng.chance(0.7) { CornerOp::Patch } else { op },
                model,
                corner: CornerIn {
                    kind,
                    v_edge: 0,
                    v_sub: 0,
                    solver: Some(vec![eu(0, 0)]),
                    all: Some(vec![eu(0, 0), eu(0, 1)]),
                    verts,
                    edges,
                    bevel: rng.chance(0.3),
                    blending: false,
                    chamfer: false,
                    degenerate: false,
                    ext: rng.chance(0.7),
                    adjusted: false,
                    setback: 0.0,
                    tol,
                    tan,
                    tsects: Vec::new(),
                    orig_faces: if rng.chance(0.85) { vec![[3, 0]] } else { Vec::new() },
                    line_edges: Vec::new(),
                    arc_edges: Vec::new(),
                },
            }
        }
        // p1's random model: early outs and state checks of every kind
        _ => {
            let model = gen_model(rng);
            let nv = model.vertices.len();
            let ne = model.edges.len();
            let nt = model.targets.len();
            let pick = |rng: &mut Rng, n: usize, k: usize| -> Vec<usize> {
                if n == 0 {
                    return Vec::new();
                }
                (0..k).map(|_| rng.below(n as u64) as usize).collect()
            };
            let kv = rng.below(5) as usize;
            let verts = pick(rng, nv, kv);
            let ke = rng.below(7) as usize;
            let edges = pick(rng, ne, ke);
            let eus = |rng: &mut Rng, k: usize| -> Option<Vec<EuSel>> {
                if nt == 0 {
                    return if rng.chance(0.5) { None } else { Some(Vec::new()) };
                }
                Some((0..k).map(|_| eu(rng.below(nt as u64) as usize, rng.below(2) as usize)).collect())
            };
            let k1 = rng.below(4) as usize;
            let k2 = rng.below(6) as usize;
            Inputs::Corner {
                op,
                corner: CornerIn {
                    kind: rng.below(11) as usize,
                    v_edge: if nt > 0 && rng.chance(0.7) { rng.below(nt as u64) as i64 } else { -1 },
                    v_sub: 0,
                    solver: eus(rng, k1),
                    all: eus(rng, k2),
                    verts,
                    edges,
                    bevel: rng.chance(0.4),
                    blending: rng.chance(0.3),
                    chamfer: rng.chance(0.1),
                    degenerate: rng.chance(0.1),
                    ext: rng.chance(0.7),
                    adjusted: rng.chance(0.1),
                    setback: if rng.chance(0.2) { rng.uniform(0.0, 1.0) } else { 0.0 },
                    tol,
                    tan,
                    tsects: Vec::new(),
                    orig_faces: Vec::new(),
                    line_edges: Vec::new(),
                    arc_edges: Vec::new(),
                },
                model,
            }
        }
    }
}

// ---- runner ----------------------------------------------------------------------------

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    match i {
        Inputs::Create {
            brep,
            validate_only,
            init_gaps,
            eu,
            more,
            solvers,
        } => create_cpp(brep, *validate_only, *init_gaps, *eu, more, solvers),
        Inputs::Corner { op, model, corner } => corner_cpp(*op, model, corner),
    }
}

fn rust(k: &Kernel, i: &Inputs) -> Result<Output, HarnessError> {
    match i {
        Inputs::Create {
            brep,
            validate_only,
            init_gaps,
            eu,
            more,
            solvers,
        } => create_rust(brep, *validate_only, *init_gaps, *eu, more, solvers),
        Inputs::Corner { op, model, corner } => {
            ffi::validate_corner(&corner_input(model, corner)?).map_err(|e| harness(e.to_string()))?;
            corner_rust(k, *op, model, corner)
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "blend.fillet_corner_p3"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse::<Inputs>()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let k = &Kernel::default();
        rust(k, &case.parse::<Inputs>()?)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        // keep inputs the FFI accepts
        for _ in 0..32 {
            let inputs = if rng.chance(0.5) {
                gen_create(rng)
            } else {
                gen_corner(rng)
            };
            // keep corner inputs the shim builds (its model block rejects some tiny
            // geometry, as blend.fillet_geom_p1's does)
            let ok = match &inputs {
                Inputs::Corner { op, model, corner } => corner_input(model, corner)
                    .is_ok_and(|v| ffi::corner(op.ffi(), &v).is_ok()),
                Inputs::Create { .. } => true,
            };
            if ok {
                return serde_json::to_value(inputs).ok();
            }
        }
        None
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs_round_trip() {
        let mut rng = Rng::new(7);
        for _ in 0..20 {
            let i = gen_corner(&mut rng);
            let v = serde_json::to_value(&i).unwrap();
            let back: Inputs = serde_json::from_value(v).unwrap();
            assert!(matches!(back, Inputs::Corner { .. }));
            let c = gen_create(&mut rng);
            let v = serde_json::to_value(&c).unwrap();
            let _: Inputs = serde_json::from_value(v).unwrap();
        }
    }
}

