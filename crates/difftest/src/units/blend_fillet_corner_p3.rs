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
//!   corner's members, the model's whole export (the vertices an operation makes are
//!   appended in creation order; the corner itself, `this`, is corner id
//!   `THIS_CORNER`) and the corner's surfaces (`put_corner_surfaces`).
//!
//! Deferred on the Rust side (unported callees, `Output::deferred`; `Create` itself runs:
//! `sm_TestG1Continuity` and `CheckNx1Degeneracy` are `blend.fillet_corner_p1`'s,
//! `sm_FindConcaveEdgeuses` `blend.fillet_corner_p2`'s, the geometry they evaluate
//! pre-deferred like the convexity test's): the patch builders where they reach
//! `MakeFaceBrep` (unit `blend.fillet_corner_p1`; also behind `CreateEndPatch`) or the
//! sphere fit's fallback `MakeConstantRadiusFillet`, and the big-radius test of blending
//! corners. The shim does not call upstream on those paths (`-3`; a Rust side that is
//! not deferred there fails the case) and refuses (`-2`, a rejected case) the inputs that
//! make upstream dereference null or free the harness's objects. To tell a default patch
//! that reaches `MakeFaceBrep` from one that returns a status before it, the shim runs
//! `MakeAnalyticCornerSurface`'s fits and the Coons patch's `CreateCornerBlend` on
//! copies first. Its `SER` / `NER` exits (read as `TRUE`) add no surface, so the
//! `MakeFaceBrep` after them copies the edges' curves only when the corner already had one
//! surface (a planar corner's exit leaves the edge it was recomputing without a curve).
//! Every refused case was checked by running the C++ natively with the pre-check bypassed,
//! one process per case: each crashed in upstream (`SIGSEGV` in `SmFilletCorner::Create`,
//! `MakeAnalyticCornerSurface`, `AdjustTopology`, `AdjustConcaveCornerTopology`,
//! `AdjustTangentCorner`) except the tangent split's call through a solver-less geometry
//! (`GetFilletSolver()->SplitFilletGeomAtSideEdge`: a member call on a null pointer,
//! undefined, though the non-virtual callee returns a status before it reads `this`). The model's fillet geometries have a solver (a base solver with the
//! generator of a cross-section kind, `geom_solvers`) only in generated patch and geometry
//! cases; elsewhere none, and the paths that ask a geometry for its solver (the `NxN`
//! candidate search past its first cross section, the closed Nx1 and Nx2 tangent splits,
//! the analytic corner's fits on a solver-less geometry) are refused. A `create` case whose sector convexity test runs on geometry
//! the import holds only approximately (analytic surfaces and curves) is deferred like
//! `topology.edgeuse_p2`'s (`inexact_geometry`).


use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::blend::fillet_corner::p3::{
    CornerEnv, CornerFilletBrep, ExecutiveErrorObject, FilletCorner, FilletCornerKind,
    FilletExecutiveData,
};
use solidcore::blend::fillet_custom_solver::{OffsetSurface, SurfaceId};
use solidcore::blend::fillet_executive::p3::ErrorInfo;
use solidcore::blend::fillet_geom::p1::{FilletCornerId, FilletCornerType, FilletSolverId};
use solidcore::blend::fillet_solver::p1::{FilletSolver, FilletSurfaceGenerator, OffsetSurfaceSlot};
use solidcore::intersect::surface_intersector::p1::SrfIndex;
use solidcore::kernel::Kernel;
use solidcore::math::EFF_ZERO_RAD;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::{Brep, EdgeuseId, FaceId, VertexId};
use solidcore::SolidError;
use solidcore_ffi::units::blend_fillet_corner_p3::{self as ffi, Op};
use solidcore_ffi::units::topology_edgeuse_p1 as ffi1;

use super::blend_fillet_geom_p1::{
    gen_model, push_model, status, Crv, EdgeIn, EuIn, GeomIn, Model, RustModel, Srf, VertexIn,
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
/// The corner id the shim exports the corner under test (`this`) as.
const THIS_CORNER: u32 = 1_000_000;

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
pub(super) type EuSel = [usize; 2];

/// An intersection point set on the vertex use of pseudo edge `e`'s primary edge use
/// (`which` 0) or mate (1): curve parameter, UV positions, user surfaces (target faces,
/// `-1` none).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct TsectIn {
    pub(super) e: usize,
    pub(super) which: usize,
    pub(super) t: f64,
    pub(super) uv0: [f64; 2],
    pub(super) uv1: [f64; 2],
    pub(super) s0: i64,
    pub(super) s1: i64,
}

/// The corner of a `corner` case.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)] // the corner's independent SmBoolean members
pub(super) struct CornerIn {
    /// The derived class (order of [`FilletCornerKind`]).
    pub(super) kind: usize,
    /// The filleted vertex: start (`v_sub` 0) or end vertex of target edge `v_edge`
    /// (`-1` none).
    pub(super) v_edge: i64,
    #[serde(default)]
    pub(super) v_sub: usize,
    pub(super) solver: Option<Vec<EuSel>>,
    pub(super) all: Option<Vec<EuSel>>,
    #[serde(default)]
    pub(super) verts: Vec<usize>,
    #[serde(default)]
    pub(super) edges: Vec<usize>,
    #[serde(default)]
    pub(super) bevel: bool,
    #[serde(default)]
    pub(super) blending: bool,
    #[serde(default)]
    pub(super) chamfer: bool,
    #[serde(default)]
    pub(super) degenerate: bool,
    #[serde(default = "yes")]
    pub(super) ext: bool,
    #[serde(default)]
    pub(super) adjusted: bool,
    #[serde(default)]
    pub(super) setback: f64,
    pub(super) tol: f64,
    pub(super) tan: f64,
    #[serde(default)]
    pub(super) tsects: Vec<TsectIn>,
    /// `SetOriginalFace` of pseudo edges: `[edge, target face]`.
    #[serde(default)]
    pub(super) orig_faces: Vec<[usize; 2]>,
    /// Pseudo edges whose curve becomes a real `SmLine` through its end points.
    #[serde(default)]
    pub(super) line_edges: Vec<usize>,
    /// Pseudo edges whose curve becomes a real `SmCircle` in a plane parallel to XY:
    /// `[edge, cx, cy, cz, radius, d0, d1]` (domain in degrees).
    #[serde(default)]
    pub(super) arc_edges: Vec<[f64; 7]>,
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
struct BigRadCornerIn {
    kind: usize,
    v_edge: i64,
    #[serde(default)]
    v_sub: usize,
    #[serde(default)]
    adjusted: bool,
    tol: f64,
    tan: f64,
    #[serde(default)]
    edges: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BigRadRailsIn {
    f0: usize,
    eu0: usize,
    d0: f64,
    f1: usize,
    eu1: usize,
    d1: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BigRadSolverIn {
    tol: f64,
    ang: f64,
    tang: f64,
    #[serde(default)]
    rails: Option<BigRadRailsIn>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BigRadIn {
    model: Model,
    fg: usize,
    corners: Vec<BigRadCornerIn>,
    fillet_corners: Vec<usize>,
    solvers: Vec<BigRadSolverIn>,
    geom_solvers: Vec<i64>,
    fillet_solvers: Vec<usize>,
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
        /// A solver (and its cross-section kind) per model geometry, `None` for a
        /// geometry without one; empty: no geometry has a solver.
        #[serde(default)]
        geom_solvers: Vec<Option<SolverIn>>,
    },
    BigRad(BigRadIn),
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
    let first = rec
        .first()
        .copied()
        .ok_or_else(|| harness("empty record"))?;
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
    let get = |i: usize| {
        rec.get(i)
            .copied()
            .ok_or_else(|| harness("record too short"))
    };
    out = out.int("type", get(1)? as i64);
    let n = get(2)?;
    let n = usize::try_from(n as i64).map_err(|_| harness("bad count"))?;
    let all: Vec<i64> = (0..n)
        .map(|i| get(3 + i).map(|x| x as i64))
        .collect::<Result<_, _>>()?;
    out = out.ints("all_eus", all);
    let base = 3 + n;
    out = out.values("tols", vec![get(base)?, get(base + 1)?]);
    let flags: Vec<i64> = (0..7)
        .map(|i| get(base + 2 + i).map(|x| x as i64))
        .collect::<Result<_, _>>()?;
    out = out.ints("flags", flags);
    out = out.values("setback", vec![get(base + 9)?]);
    let counts: Vec<i64> = (0..3)
        .map(|i| get(base + 10 + i).map(|x| x as i64))
        .collect::<Result<_, _>>()?;
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
            .and_then(|e| types.inexact_curve(e.index() as usize))
            .unwrap_or(-1);
        if ct >= 0 && !EXACT_CURVES.contains(&ct) {
            return Some(Output::deferred(
                "topology.brep_p10",
                "SmBrep::MakeTopologyFromData",
            ));
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
    // the geometry `sm_TestG1Continuity` / `SmFilletNx1Corner::CheckNx1Degeneracy` (unit
    // `blend.fillet_corner_p1`, wired) evaluate before the corner is classified: the
    // filleted edges' curves (G1 test) or the side edge uses next to the single filleted
    // edge use (the Nx1 degeneracy test)
    if let Some(&eu0) = s.eus.first() {
        let b = &s.brep;
        let edges_at_vertex = {
            let mut edges = Vec::new();
            for (id, _) in b.edgeuses().iter() {
                if eu_vertex(b, id) == Some(s.vertex) {
                    if let Ok(Some(e)) = eu1::edge(b, id) {
                        if !edges.contains(&e) {
                            edges.push(e);
                        }
                    }
                }
            }
            edges.len()
        };
        let closed = eu1::edge(b, eu0)
            .ok()
            .flatten()
            .and_then(|e| solidcore::topology::edge::is_closed(b, e).ok())
            .unwrap_or(false);
        let evaluated: Vec<EdgeuseId> = match (s.eus.len(), closed) {
            (1, true) if edges_at_vertex == 1 => vec![eu0],
            (1, false) => {
                let mut v = vec![eu0];
                if let Ok(Some(s1)) = eu1::cw_edgeuse(b, eu0) {
                    v.push(s1);
                    let s2 = eu1::radial(b, eu0)
                        .ok()
                        .flatten()
                        .and_then(|r| eu1::ccw_edgeuse(b, r).ok().flatten());
                    match s2 {
                        Some(s2) if s2 == s1 => v.extend(eu1::mate(b, s1).ok().flatten()),
                        Some(s2) => v.push(s2),
                        None => {}
                    }
                }
                v
            }
            (2, _) if edges_at_vertex == 2 => s.eus.clone(),
            _ => Vec::new(),
        };
        if !evaluated.is_empty() {
            if let Some(o) = inexact(&s.brep, &s.types, &evaluated) {
                return Ok(o);
            }
        }
    }
    let mut exec = FilletExecutiveData::default();
    let mut table = Vec::new();
    for (i, &id) in s.eus.iter().enumerate() {
        let sv = solver_of(solvers, i)?;
        let mut fs = FilletSolver::new(sv.approx, EFF_ZERO_RAD, sv.tang);
        fs.fsg = Some(fsg_of(sv.fsg));
        exec.edgeuse_solver_map.insert(
            id,
            FilletSolverId(u32::try_from(i).map_err(|_| harness("index"))?),
        );
        table.push(fs);
    }
    let r = FilletCorner::create(&k, &mut s.brep, s.vertex, &s.eus, &exec, &table);
    let c = match r {
        Ok(c) => c,
        Err(e) => {
            return Ok(Output::unported(&e)
                .unwrap_or_else(|| Output::err(format!("rust_error_{}", status(&e)))))
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
    rec.extend([
        c.vertices.len() as f64,
        c.edges.len() as f64,
        c.surfaces.len() as f64,
    ]);
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

/// The solver block of a corner input: the distinct solvers (`approx tang fsg`), then, when
/// there are any, per model geometry its solver index (`-1` none).
fn push_geom_solvers(o: &mut Vec<f64>, model: &Model, gs: &[Option<SolverIn>]) {
    let used: Vec<&SolverIn> = gs.iter().take(model.geoms.len()).flatten().collect();
    o.push(used.len() as f64);
    if used.is_empty() {
        return;
    }
    for sv in &used {
        o.extend([sv.approx, sv.tang, f64::from(sv.fsg)]);
    }
    let mut next = 0.0;
    for g in 0..model.geoms.len() {
        if matches!(gs.get(g), Some(Some(_))) {
            o.push(next);
            next += 1.0;
        } else {
            o.push(-1.0);
        }
    }
}

fn corner_input_with(
    model: &Model,
    c: &CornerIn,
    gs: &[Option<SolverIn>],
) -> Result<Vec<f64>, HarnessError> {
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
    push_geom_solvers(&mut o, model, gs);
    Ok(o)
}

/// Splits a corner record: `status`, the corner members, the model export.
fn corner_output(rec: &[f64]) -> Result<Output, HarnessError> {
    let first = rec
        .first()
        .copied()
        .ok_or_else(|| harness("empty record"))?;
    if first == REFUSED {
        return Ok(Output::rejected("upstream would dereference null"));
    }
    if first == NOT_RUN {
        return Ok(Output::err("not_run_by_cpp"));
    }
    let get = |i: usize| {
        rec.get(i)
            .copied()
            .ok_or_else(|| harness("record too short"))
    };
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
    let model = rec
        .get(at..)
        .ok_or_else(|| harness("record too short"))?
        .to_vec();
    Ok(Output::new()
        .int("status", first as i64)
        .values("corner_verts", verts)
        .values("corner_edges", edges)
        .values("corner_flags", flags)
        .values("setback", vec![setback])
        .int("surfaces", nsurf as i64)
        .values("model", model))
}

fn corner_cpp(
    op: CornerOp,
    model: &Model,
    c: &CornerIn,
    gs: &[Option<SolverIn>],
) -> Result<Output, HarnessError> {
    corner_cpp_gs(op.ffi(), &[], model, c, gs)
}

/// The C++ side of a corner case running `op`, whose input is the corner's followed by
/// `tail` (unit `blend.fillet_corner_p2`'s operations).
pub(super) fn corner_cpp_op(
    op: Op,
    tail: &[f64],
    model: &Model,
    c: &CornerIn,
) -> Result<Output, HarnessError> {
    corner_cpp_gs(op, tail, model, c, &[])
}

/// [`corner_cpp_op`] with solvers on the model's geometries.
fn corner_cpp_gs(
    op: Op,
    tail: &[f64],
    model: &Model,
    c: &CornerIn,
    gs: &[Option<SolverIn>],
) -> Result<Output, HarnessError> {
    let mut input = corner_input_with(model, c, gs)?;
    input.extend_from_slice(tail);
    let flat = match ffi::corner(op, &input) {
        Ok(flat) => flat,
        // the shim's UB-166 refusal of a surface whose constructed v knots crash the
        // surface cache (only for the ops that build one)
        Err(solidcore_ffi::FfiError::Status(solidcore_ffi::SC_ERR_ARG))
            if op == Op::UpdateSplitFace =>
        {
            return Ok(Output::rejected("shim_rejected_args (UB-166)"));
        }
        Err(e) => return Err(harness(e.to_string())),
    };
    corner_output(&flat)
}

fn corner_rust(
    k: &Kernel,
    op: CornerOp,
    model: &Model,
    ci: &CornerIn,
    gs: &[Option<SolverIn>],
) -> Result<Output, HarnessError> {
    corner_rust_status_gs(k, model, ci, gs, |c, env, _| {
        match op {
            CornerOp::CalcGeom => c.calc_corner_geom(env),
            CornerOp::Patch => c.create_corner_patch(env),
            CornerOp::AdjustTopology => c.adjust_topology(env),
            CornerOp::AdjustTangent => c.adjust_tangent_corner(env),
        }
        .map(|()| 0.0)
    })
}

/// The Rust side of a corner case running `run` on the corner (with the model's target
/// faces, by index); `run` returns the record's status on success.
pub(super) fn corner_rust_status(
    k: &Kernel,
    model: &Model,
    ci: &CornerIn,
    run: impl FnOnce(&mut FilletCorner, &mut CornerEnv<'_>, &[FaceId]) -> solidcore::Result<f64>,
) -> Result<Output, HarnessError> {
    corner_rust_status_gs(k, model, ci, &[], run)
}

/// [`corner_rust_status`] with solvers on the model's geometries (as the shim's).
fn corner_rust_status_gs(
    k: &Kernel,
    model: &Model,
    ci: &CornerIn,
    gs: &[Option<SolverIn>],
    run: impl FnOnce(&mut FilletCorner, &mut CornerEnv<'_>, &[FaceId]) -> solidcore::Result<f64>,
) -> Result<Output, HarnessError> {
    let h = |e: SolidError| harness(e.to_string());
    let mut rm = RustModel::build(model)?;
    // a solver per geometry that has one (the shim's P4Solver with the cross-section
    // kind's generator), numbered in geometry order
    let mut solvers: Vec<FilletSolver> = Vec::new();
    for (g, sv) in gs.iter().enumerate().take(model.geoms.len()) {
        let Some(sv) = sv else { continue };
        let mut fs = FilletSolver::new(sv.approx, EFF_ZERO_RAD, sv.tang);
        fs.fsg = Some(fsg_of(sv.fsg));
        let gid = rm.geom(g)?;
        rm.fb.geom_mut(gid).map_err(h)?.solver =
            Some(FilletSolverId(u32::try_from(solvers.len()).map_err(|_| harness("index"))?));
        solvers.push(fs);
    }
    let kind = *KINDS.get(ci.kind).ok_or_else(|| harness("kind"))?;
    let target_eu = |rm: &RustModel, [e, w]: EuSel| -> Result<EdgeuseId, HarnessError> {
        let edge = *rm
            .target_edges
            .get(e)
            .ok_or_else(|| harness("target edge"))?;
        let prim = eu1::primary_edgeuse(&rm.target, edge)
            .map_err(h)?
            .ok_or_else(|| harness("primary edge use"))?;
        if w == 1 {
            eu1::mate(&rm.target, prim)
                .map_err(h)?
                .ok_or_else(|| harness("mate"))
        } else {
            Ok(prim)
        }
    };
    let list =
        |rm: &RustModel, l: Option<&Vec<EuSel>>| -> Result<Option<Vec<EdgeuseId>>, HarnessError> {
            l.map(|l| l.iter().map(|&s| target_eu(rm, s)).collect())
                .transpose()
        };
    let vertex = match usize::try_from(ci.v_edge) {
        Ok(e) => {
            let edge = *rm
                .target_edges
                .get(e)
                .ok_or_else(|| harness("target edge"))?;
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
            *tp.uv_pos_mut(SrfIndex::First) =
                solidcore::geom::vector2d::Point2::new(t.uv0[0], t.uv0[1]);
            *tp.uv_pos_mut(SrfIndex::Second) =
                solidcore::geom::vector2d::Point2::new(t.uv1[0], t.uv1[1]);
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
        let user = |s: Option<Surface>| {
            s.map(|s| {
                solidcore::intersect::surface_intersector::p1::UserSurface(std::sync::Arc::new(s))
            })
        };
        let user_pointer = [user(srf(&rm, t.s0)?), user(srf(&rm, t.s1)?)];
        rm.fb
            .fillet_vertexuse_mut(vu)
            .map_err(h)?
            .tsect_pnt_mut()
            .user_pointer = user_pointer;
    }

    for &[e, f] in &ci.orig_faces {
        let eid = rm.edge(e)?;
        let face = *rm.target_faces.get(f).ok_or_else(|| harness("face"))?;
        rm.fb
            .fillet_edge_mut(eid)
            .map_err(h)?
            .set_original_face(Some(face));
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
        rm.fb.brep_mut().edge_mut(eid).map_err(h)?.curve =
            Some(solidcore::curve::curve::Curve::from(l));
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
        rm.fb.brep_mut().edge_mut(eid).map_err(h)?.curve =
            Some(solidcore::curve::curve::Curve::from(circle));
    }
    let faces = rm.target_faces.clone();
    // m_pFilletBrep: `new (ctx) SmBrep()` (its constructor makes the infinite region)
    let mut fillet_brep = Brep::new_with_infinite_region().map_err(h)?;
    let mut exec = FilletExecutiveData::default();
    let r = {
        let mut records = solidcore::topology::topology::NotifyRecords::new();
        let mut attributes = solidcore::geom::attribute::AttributeStore::default();
        let mut env = CornerEnv {
            k: &mut rm.k,
            target: &mut rm.target,
            pseudo: &mut rm.fb,
            exec: &mut exec,
            // the shim exports `this` as this id
            corner: Some(FilletCornerId(THIS_CORNER)),
            // the model's geometries have a solver only where the case gives one
            solvers: &mut solvers,
            fillet: Some(fillet_env(&mut fillet_brep, &mut records, &mut attributes)),
        };
        run(&mut c, &mut env, &faces)
    };
    let st = match r {
        Ok(v) => v,
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
    put_corner_surfaces(&mut rec, &c.surfaces)?;
    put_fillet_brep(&mut rec, &fillet_brep)?;
    put_error_info(&mut rec, &exec.error, c.filleted_vertex());
    corner_output(&rec)
}

/// Distinct identities for the face makers' notifications in the fillet B-rep.
fn fillet_key_of(e: solidcore::EntityRef) -> solidcore::geom::object::ObjectRef {
    use solidcore::EntityRef as E;
    let (type_code, key) = match e {
        E::Face(f) => (solidcore::topology::face::p2::TYPE, 1 + u64::from(f.index())),
        E::Edge(x) => (solidcore::topology::edge::TYPE, 1 << 40 | u64::from(x.index())),
        E::Edgeuse(x) => (16005, 2 << 40 | u64::from(x.index())),
        _ => (0, 3 << 40),
    };
    solidcore::geom::object::ObjectRef {
        type_code,
        key: solidcore::geom::attribute::ObjectKey(key),
    }
}

/// The corner's [`CornerFilletBrep`] over `brep` (the executive's `m_pFilletBrep`).
pub(super) fn fillet_env<'a>(
    brep: &'a mut Brep,
    records: &'a mut solidcore::topology::topology::NotifyRecords,
    attributes: &'a mut solidcore::geom::attribute::AttributeStore,
) -> CornerFilletBrep<'a> {
    CornerFilletBrep {
        brep,
        notify: solidcore::topology::edge::NotifyEnv {
            records,
            attributes,
            context: None,
            key_of: &fillet_key_of,
        },
    }
}

/// The executive's fillet B-rep as the shim's `p3_put_fillet_brep` writes it: per face
/// (`GetFaces` order) its surface, its face use's orientation and per loop and edge use
/// (CCW) the edge use's orientation, vertex point, edge interval, edge curve and UV trim
/// curve.
pub(super) fn put_fillet_brep(o: &mut Vec<f64>, b: &Brep) -> Result<(), HarnessError> {
    use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
    let h = |e: SolidError| harness(e.to_string());
    let code = |o: OrientType| match o {
        OrientType::Unknown => 0.0,
        OrientType::Same => 1.0,
        OrientType::Opposite => 2.0,
    };
    let faces = solidcore::topology::brep::p1::faces(b, None).map_err(h)?;
    o.push(faces.len() as f64);
    for f in faces {
        let face = b.face(f).map_err(h)?;
        put_corner_surface(o, face.surface.as_ref())?;
        o.push(match face.faceuse {
            Some(fu) => code(b.faceuse(fu).map_err(h)?.orientation),
            None => -1.0,
        });
        let loops = solidcore::topology::face::p2::loops(b, f, false, None).map_err(h)?;
        o.push(loops.len() as f64);
        for l in loops {
            let eus = solidcore::topology::loopuse::loop_edgeuses(b, l).map_err(h)?;
            o.push(eus.len() as f64);
            for eu in eus {
                let rec = b.edgeuse(eu).map_err(h)?;
                o.push(code(rec.orientation));
                let p = match rec.vertexuse {
                    Some(vu) => match b.vertexuse(vu).map_err(h)?.vertex {
                        Some(v) => b.vertex(v).map_err(h)?.point,
                        None => solidcore::math::Point3::new(0.0, 0.0, 0.0),
                    },
                    None => solidcore::math::Point3::new(0.0, 0.0, 0.0),
                };
                o.extend([p.x, p.y, p.z]);
                let edge = match rec.edge {
                    Some(e) => Some(b.edge(e).map_err(h)?),
                    None => None,
                };
                o.extend(edge.map_or([0.0, 0.0], |e| [e.interval.min(), e.interval.max()]));
                super::blend_fillet_geom_p1::dump_curve(o, edge.and_then(|e| e.curve.as_ref()));
                super::blend_fillet_geom_p1::dump_curve(o, rec.uv_trim_curve.as_ref());
            }
        }
    }
    Ok(())
}

/// Executive error info as `p3_put_error_info` writes it.
pub(super) fn put_error_info(
    o: &mut Vec<f64>,
    info: &ErrorInfo<ExecutiveErrorObject>,
    corner_vert: Option<VertexId>,
) {
    o.push(f64::from(info.code));
    o.push(info.corner_type as i64 as f64);
    let vtag = match info.vertex {
        None => -1.0,
        Some(ExecutiveErrorObject::TargetVertex(v)) if Some(v) == corner_vert => 1.0,
        Some(_) => 0.0,
    };
    o.push(vtag);
    let nb = info.entity_name.as_bytes();
    o.push(nb.len() as f64);
    for &b in nb {
        o.push(f64::from(b));
    }
    let cb = info.comment.as_bytes();
    o.push(cb.len() as f64);
    for &b in cb {
        o.push(f64::from(b));
    }
}

fn big_rad_input(b: &BigRadIn) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    push_model(&mut o, &b.model)?;
    o.push(b.fg as f64);
    o.push(b.corners.len() as f64);
    for c in &b.corners {
        o.extend([
            c.kind as f64,
            c.v_edge as f64,
            c.v_sub as f64,
            flag(c.adjusted),
            c.tol,
            c.tan,
            c.edges.len() as f64,
        ]);
        for &e in &c.edges {
            o.push(e as f64);
        }
    }
    o.push(b.fillet_corners.len() as f64);
    for &ci in &b.fillet_corners {
        o.push(ci as f64);
    }
    o.push(b.solvers.len() as f64);
    for s in &b.solvers {
        o.extend([s.tol, s.ang, s.tang, flag(s.rails.is_some())]);
        if let Some(r) = &s.rails {
            o.extend([
                r.f0 as f64,
                r.eu0 as f64,
                r.d0,
                r.f1 as f64,
                r.eu1 as f64,
                r.d1,
            ]);
        }
        o.push(0.0); // ngo = 0 so M.geoms matches rm.geoms
    }
    for &si in &b.geom_solvers {
        o.push(si as f64);
    }
    o.push(b.fillet_solvers.len() as f64);
    for &si in &b.fillet_solvers {
        o.push(si as f64);
    }
    Ok(o)
}

fn big_rad_output(rec: &[f64]) -> Result<Output, HarnessError> {
    let first = rec
        .first()
        .copied()
        .ok_or_else(|| harness("empty big_rad record"))?;
    if first == REFUSED {
        return Ok(Output::rejected("upstream would dereference null"));
    }
    let converted = rec
        .get(1)
        .copied()
        .ok_or_else(|| harness("short big_rad record"))?;
    let tail = rec.get(2..).unwrap_or_default().to_vec();
    Ok(Output::new()
        .int("status", first as i64)
        .int("converted", converted as i64)
        .values("state", tail))
}

fn big_rad_cpp(b: &BigRadIn) -> Result<Output, HarnessError> {
    let input = big_rad_input(b)?;
    let flat = ffi::corner(Op::BigRad, &input).map_err(|e| harness(e.to_string()))?;
    big_rad_output(&flat)
}

fn big_rad_rust(k: &Kernel, b: &BigRadIn) -> Result<Output, HarnessError> {
    ffi::validate_corner_op(Op::BigRad, &big_rad_input(b)?)
        .map_err(|e| harness(e.to_string()))?;
    let h = |e: SolidError| harness(e.to_string());
    let mut rm = RustModel::build(&b.model)?;
    let fg = rm.geom(b.fg)?;
    let mut exec = FilletExecutiveData::default();
    for (ci, c) in b.corners.iter().enumerate() {
        let kind = *KINDS.get(c.kind).ok_or_else(|| harness("kind"))?;
        let vert = match usize::try_from(c.v_edge) {
            Ok(e) => {
                let edge = *rm
                    .target_edges
                    .get(e)
                    .ok_or_else(|| harness("target edge"))?;
                let [vs, ve] = rm.target.edge(edge).map_err(h)?.vertices;
                Some(if c.v_sub == 0 { vs } else { ve })
            }
            Err(_) => None,
        };
        let mut fc = FilletCorner::new(kind, vert, None, None, c.tol, c.tan);
        fc.topology_adjusted = c.adjusted;
        for &ei in &c.edges {
            fc.edges.push(rm.edge(ei)?);
        }
        exec.corners.insert(FilletCornerId(ci as u32), fc);
    }
    for &ci in &b.fillet_corners {
        exec.fillet_corners.push(FilletCornerId(ci as u32));
    }
    let mut solvers = Vec::new();
    for (si, s) in b.solvers.iter().enumerate() {
        let mut fs = FilletSolver::new(s.tol, s.ang, s.tang);
        if let Some(r) = &s.rails {
            let face_eu = |rm: &RustModel, fi: usize, eui: usize| -> Result<(FaceId, EdgeuseId), HarnessError> {
                let f = *rm.target_faces.get(fi).ok_or_else(|| harness("face"))?;
                let loops = solidcore::topology::face::p2::loops(&rm.target, f, false, None).map_err(h)?;
                let l = *loops.first().ok_or_else(|| harness("loop"))?;
                let eus = solidcore::topology::loopuse::loop_edgeuses(&rm.target, l).map_err(h)?;
                let eu = *eus.get(eui).ok_or_else(|| harness("eu"))?;
                Ok((f, eu))
            };
            let (f0, eu0) = face_eu(&rm, r.f0, r.eu0)?;
            let (f1, eu1_id) = face_eu(&rm, r.f1, r.eu1)?;
            let s0_base = rm.target.face(f0).map_err(h)?.surface.clone().ok_or_else(|| harness("s0"))?;
            let s1_base = rm.target.face(f1).map_err(h)?.surface.clone().ok_or_else(|| harness("s1"))?;
            let mut os0 = OffsetSurface::new(
                &rm.k,
                r.d0,
                s0_base,
                SurfaceId::Face(f0),
                solidcore::surface::offset_surface::SS_UNKNOWN,
            )
            .map_err(h)?;
            let mut os1 = OffsetSurface::new(
                &rm.k,
                r.d1,
                s1_base,
                SurfaceId::Face(f1),
                solidcore::surface::offset_surface::SS_UNKNOWN,
            )
            .map_err(h)?;
            os0.set_allow_extension(true);
            os1.set_allow_extension(true);
            fs.edgeuses = [Some(eu0), Some(eu1_id)];
            fs.set_surface(
                0,
                Some(OffsetSurfaceSlot {
                    key: (si * 2 + 1) as u64,
                    surface: os0,
                }),
            )
            .map_err(h)?;
            fs.set_surface(
                1,
                Some(OffsetSurfaceSlot {
                    key: (si * 2 + 2) as u64,
                    surface: os1,
                }),
            )
            .map_err(h)?;
        }
        solvers.push(fs);
    }
    for (gi, &si) in b.geom_solvers.iter().enumerate() {
        let gid = rm.geom(gi)?;
        rm.fb.geom_mut(gid).map_err(h)?.solver =
            usize::try_from(si).ok().map(|i| FilletSolverId(i as u32));
    }
    for &si in &b.fillet_solvers {
        exec.fillet_solvers.push(FilletSolverId(si as u32));
    }

    let res = exec.test_and_convert_big_rad_fillet(
        k,
        &mut rm.target,
        &mut rm.fb,
        &mut solvers,
        fg,
    );
    let (st, converted) = match res {
        Ok(conv) => (0.0, conv),
        Err(e) if e.is_unported() => {
            return Ok(Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string())))
        }
        Err(e) => (status(&e), false),
    };
    let mut rec = vec![st, flag(converted)];
    if st == 0.0 {
        rec.push(exec.fillet_corners.len() as f64);
        for &cid in &exec.fillet_corners {
            rec.push(f64::from(cid.0));
        }
        rec.push(exec.fillet_solvers.len() as f64);
        for &sid in &exec.fillet_solvers {
            rec.push(f64::from(sid.0));
        }
        if converted {
            let new_id = FilletCornerId(b.corners.len() as u32);
            let nc = exec.corners.get(&new_id).ok_or_else(|| harness("new corner"))?;
            rec.push(nc.corner_type() as i64 as f64);
            rec.push(nc.vertices.len() as f64);
            for &v in &nc.vertices {
                rec.push(rm.vertex_index(Some(v)));
            }
            rec.push(nc.edges.len() as f64);
            for &e in &nc.edges {
                rec.push(rm.edge_index(Some(e)));
            }
            rec.push(nc.approx_tol_3d);
            rec.push(nc.tangency_tol_radians);
            rec.push((rm.target_edges.len() * 2) as f64);
            for &te in &rm.target_edges {
                let [vs, ve] = rm.target.edge(te).map_err(h)?.vertices;
                for tv in [vs, ve] {
                    rec.push(
                        exec.vertex_corner_map
                            .get(&tv)
                            .map_or(-1.0, |c| f64::from(c.0)),
                    );
                }
            }
        }
        rm.put(&mut rec)?;
    }
    big_rad_output(&rec)
}

/// Curated `TestAndConvertBigRadFillet` cases exercising full corner conversion
/// (`squeeze = 0` & `1`, converging `CalcRailIntRail` on 3 orthogonal planes, edge
/// reversal, corner-edge relation updates, index swap in `m_vFilletCorners`) as well as
/// non-converting early-outs and error statuses.
fn big_rad_cases() -> Result<Vec<Case>, HarnessError> {
    let mk_vert = |point: [f64; 3],
                   vtype: i32,
                   status: i32,
                   corner: i64,
                   face: i64,
                   mates: [i64; 2]| VertexIn {
        point,
        vtype: i64::from(vtype),
        status: i64::from(status),
        corner,
        pc_class: if face >= 0 { 102 } else { 100 },
        pc_kind: if face >= 0 { 3 } else { 0 },
        pc_index: face,
        pc_sub: 0,
        pc_face: face,
        t_param: 0.0,
        uv: [0.2, 0.2],
        mates,
    };
    let mk_edge = |v0: usize, v1: usize, etype: i32, corner: i64, g0: i64, g1: i64| EdgeIn {
        vs: v0,
        ve: v1,
        etype: i64::from(etype),
        status: 1, // SM_FIL_PROCESSED
        corner,
        curve: Some(line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0])),
        interval: Some([0.0, 1.0]),
        fbe: [-1, -1],
        eus: [
            EuIn {
                uv_curve: None,
                geom: g0,
                vu_uv: [0.2, 0.2],
            },
            EuIn {
                uv_curve: None,
                geom: g1,
                vu_uv: [0.2, 0.2],
            },
        ],
    };

    // Base converting setup:
    // Target faces: f0 = XY plane z=0, f1 = XZ plane y=0, f2 = YZ plane x=0.
    // fg = geom 0 between corner 0 (Nx2, target vert (0,0)) and corner 1 (Nx2, target vert (0,1)).
    // Rail 0: v0 (1, 0.2, 0) -> v1 (0, 0.2, 0)  (dir = (-1, 0, 0)), MATE vertices on f0 (z=0), mates v4 & v5.
    // Rail 1: v2 (0.2, 0, 0.2) -> v3 (0.8, 0, 0.2) (dir = (+0.6, 0, 0), dot < 0!).
    // Extra rails into v0 (geom 1, solver 1 on f0 & f1) and v1 (geom 2, solver 2 on f0 & f2) so that
    // after squeezing rail 0 (moving v1's uses onto v0), v0 has geoms [1, 2] and CalcRailIntRail
    // converges on f0 at (0.2, 0.2, 0.0)!
    let base_case = |swap_mates_to_rail1: bool, reverse_cs: bool, swap_fc_order: bool| -> BigRadIn {
        let (vt0, vt1) = if swap_mates_to_rail1 {
            (4, 10) // RailXRail on rail 0, Mate on rail 1 -> squeeze = 1!
        } else {
            (10, 4) // Mate on rail 0, RailXRail on rail 1 -> squeeze = 0!
        };
        let mut vertices = vec![
            mk_vert([1.0, 0.2, 0.0], vt0, 1, 0, 0, [4, -1]), // v0: start of rail 0 (corner 0)
            mk_vert([0.0, 0.2, 0.0], vt0, 1, 1, 0, [-1, -1]), // v1: end of rail 0 (corner 1)
            mk_vert([0.2, 0.0, 0.2], vt1, 1, 0, 0, [4, -1]), // v2: start of rail 1 (corner 0)
            mk_vert([0.8, 0.0, 0.2], vt1, 1, 1, 0, [-1, -1]), // v3: end of rail 1 (corner 1)
            mk_vert([0.2, 0.0, 0.3], 10, 1, 0, 1, [0, 2]),   // v4: mate on f1
            mk_vert([0.0, 0.3, 0.2], 10, 1, 0, 2, [-1, -1]), // v5: extra vertex for rail of geom 1/2
        ];
        if swap_mates_to_rail1 {
            if let Some(v0) = vertices.get_mut(0) {
                v0.mates = [-1, -1];
            }
            if let Some(v2) = vertices.get_mut(2) {
                v2.mates = [4, -1];
            }
        }
        // Edges:
        // e0: rail 0 of geom 0 (v0 -> v1)
        // e1: rail 1 of geom 0 (v2 -> v3)
        // e2: cross-section of corner 1 (end corner): connects v3 & v1
        // e3: cross-section of corner 0 (start corner): connects v0 & v2
        // e4: rail of geom 1 connected to squeezed start vertex (v0 or v2) -> v5
        // e5: rail of geom 2 connected to squeezed end vertex (v1 or v3) -> v5
        let sq_sv = if swap_mates_to_rail1 { 2 } else { 0 };
        let sq_ev = if swap_mates_to_rail1 { 3 } else { 1 };
        let ot_ev = if swap_mates_to_rail1 { 1 } else { 3 };
        let ot_sv = if swap_mates_to_rail1 { 0 } else { 2 };
        let (cs_end_a, cs_end_b) = if reverse_cs {
            (sq_ev, ot_ev) // GetVertex() != ot_ev -> triggers SM_REVERSE_FILLET_EDGE!
        } else {
            (ot_ev, sq_ev)
        };
        let (cs_start_a, cs_start_b) = if reverse_cs {
            (ot_sv, sq_sv) // GetVertex() != sq_sv -> triggers SM_REVERSE_FILLET_EDGE!
        } else {
            (sq_sv, ot_sv)
        };
        let edges = vec![
            mk_edge(0, 1, 9, -1, 0, -1),                     // e0: SM_FE_RAIL (9) of geom 0
            mk_edge(2, 3, 9, -1, 0, -1),                     // e1: SM_FE_RAIL (9) of geom 0
            mk_edge(cs_end_a, cs_end_b, 1, 1, 0, 2),         // e2: SM_FE_CROSS_SECTION (1) of corner 1 (prim has fg=0 -> modified to mate's fg=2!)
            mk_edge(cs_start_a, cs_start_b, 1, 0, 1, 0),     // e3: SM_FE_CROSS_SECTION (1) of corner 0 (prim has fg=1, mate cleared!)
            mk_edge(sq_sv, 5, 9, -1, 1, -1),                 // e4: SM_FE_RAIL of geom 1
            mk_edge(sq_ev, 5, 9, -1, 2, -1),                 // e5: SM_FE_RAIL of geom 2
        ];
        let model = Model {
            targets: vec![line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0])],
            faces: vec![
                plane_srf([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]), // f0: z=0 (normal +Z)
                plane_srf([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]), // f1: y=0 (normal -Y)
                plane_srf([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]), // f2: x=0 (normal +X)
            ],
            fillet_brep: Vec::new(),
            vertices,
            edges,
            geoms: vec![
                GeomIn {
                    gtype: 1,
                    status: 0,
                    rails: [0, 1],
                    other1: Vec::new(),
                    other2: Vec::new(),
                    center: None,
                    surface: None,
                    sides: Vec::new(),
                },
                GeomIn {
                    gtype: 1,
                    status: 0,
                    rails: [4, -1],
                    other1: Vec::new(),
                    other2: Vec::new(),
                    center: None,
                    surface: None,
                    sides: Vec::new(),
                },
                GeomIn {
                    gtype: 1,
                    status: 0,
                    rails: [5, -1],
                    other1: Vec::new(),
                    other2: Vec::new(),
                    center: None,
                    surface: None,
                    sides: Vec::new(),
                },
            ],
        };
        BigRadIn {
            model,
            fg: 0,
            corners: vec![
                BigRadCornerIn {
                    kind: 7, // NByTwo
                    v_edge: 0,
                    v_sub: 0,
                    adjusted: false,
                    tol: 1.0e-5,
                    tan: 1.0e-2,
                    edges: vec![3],
                },
                BigRadCornerIn {
                    kind: 7, // NByTwo
                    v_edge: 0,
                    v_sub: 1,
                    adjusted: false,
                    tol: 1.0e-5,
                    tan: 1.0e-2,
                    edges: vec![2],
                },
            ],
            fillet_corners: if swap_fc_order { vec![1, 0] } else { vec![0, 1] },
            solvers: vec![
                BigRadSolverIn {
                    tol: 1.0e-5,
                    ang: 1.0e-3,
                    tang: 1.0e-2,
                    rails: None,
                },
                BigRadSolverIn {
                    tol: 1.0e-5,
                    ang: 1.0e-3,
                    tang: 1.0e-2,
                    rails: Some(BigRadRailsIn {
                        f0: 0,
                        eu0: 0,
                        d0: 0.2,
                        f1: 1,
                        eu1: 0,
                        d1: -0.2,
                    }),
                },
                BigRadSolverIn {
                    tol: 1.0e-5,
                    ang: 1.0e-3,
                    tang: 1.0e-2,
                    rails: Some(BigRadRailsIn {
                        f0: 0,
                        eu0: 3,
                        d0: 0.2,
                        f1: 2,
                        eu1: 0,
                        d1: 0.2,
                    }),
                },
            ],
            geom_solvers: vec![0, 1, 2],
            fillet_solvers: vec![0, 1, 2],
        }
    };

    let mut raw: Vec<(&str, BigRadIn)> = vec![
        ("big_rad_convert_squeeze0", base_case(false, false, false)),
        ("big_rad_convert_squeeze0_rev_swap", base_case(false, true, true)),
        ("big_rad_convert_squeeze1", base_case(true, true, false)),
    ];

    // Non-converting: topology_adjusted == true
    {
        let mut c = base_case(false, false, false);
        if let Some(c0) = c.corners.get_mut(0) {
            c0.adjusted = true;
        }
        raw.push(("big_rad_skip_adjusted", c));
    }
    // Non-converting: same rail direction (dot > 0)
    {
        let mut c = base_case(false, false, false);
        if let Some(v2) = c.model.vertices.get_mut(2) {
            v2.point = [0.8, 0.0, 0.2];
        }
        if let Some(v3) = c.model.vertices.get_mut(3) {
            v3.point = [0.2, 0.0, 0.2];
        }
        raw.push(("big_rad_skip_same_dir", c));
    }
    // Non-converting: non-Nx2 corner type
    {
        let mut c = base_case(false, false, false);
        if let Some(c1) = c.corners.get_mut(1) {
            c1.kind = 9; // ConvexNByN
        }
        raw.push(("big_rad_skip_corner_type", c));
    }
    // Error: neither rail has two SM_FV_MATE vertices -> SM_ERR
    {
        let mut c = base_case(false, false, false);
        if let Some(v0) = c.model.vertices.get_mut(0) {
            v0.vtype = 4;
        }
        if let Some(v2) = c.model.vertices.get_mut(2) {
            v2.vtype = 4;
        }
        raw.push(("big_rad_err_no_mate_rail", c));
    }
    // Error: start corner has NULL GetFilletCorner -> SM_ERR_NULL_POINTER
    {
        let mut c = base_case(false, false, false);
        if let Some(v0) = c.model.vertices.get_mut(0) {
            v0.corner = -1;
        }
        raw.push(("big_rad_err_null_corner", c));
    }

    raw.into_iter()
        .map(|(name, b)| {
            let v = serde_json::to_value(Inputs::BigRad(b))
                .map_err(|e| harness(e.to_string()))?;
            Ok(Case {
                unit: "blend.fillet_corner_p3".into(),
                name: name.into(),
                source: crate::case::CaseSource::Inline,
                inputs: v,
            })
        })
        .collect()
}

/// The corner's surfaces as the shim's `p3_put_surfaces` writes them: per surface its kind
/// (`1` plane, `2` surface of revolution, cone, sphere or torus, `0` any other) and the
/// B-spline record of the `SmBSplineSurface` it derives from upstream.
fn put_corner_surfaces(o: &mut Vec<f64>, surfaces: &[Surface]) -> Result<(), HarnessError> {
    o.push(surfaces.len() as f64);
    for s in surfaces {
        put_corner_surface(o, Some(s))?;
    }
    Ok(())
}

/// One surface of [`put_corner_surfaces`] (the shim's `p3_put_surface`; `None` for a
/// null surface).
pub(super) fn put_corner_surface(o: &mut Vec<f64>, s: Option<&Surface>) -> Result<(), HarnessError> {
    use solidcore::nurbs::b_spline_surface::BSplineSurface;
    let (kind, bs): (f64, Option<BSplineSurface>) = match s.map(Surface::kind) {
        Some(SurfaceKind::Plane(p)) => (1.0, Some(p.base().clone())),
        Some(SurfaceKind::SurfOfRevolution(r)) => (2.0, Some(r.base().clone())),
        Some(SurfaceKind::Cone(c)) => (2.0, Some(c.clone().base_mut().clone())),
        Some(SurfaceKind::Sphere(sp)) => (2.0, Some(sp.base().base().clone())),
        Some(SurfaceKind::Torus(t)) => (2.0, Some(t.base().base().clone())),
        Some(SurfaceKind::SurfOfExtrusion(e)) => (0.0, Some(e.base().clone())),
        Some(SurfaceKind::BSpline(b)) => (0.0, Some(b.clone())),
        _ => (0.0, None),
    };
    o.push(kind);
    super::blend_fillet_geom_p1::dump_bspline_surface(o, bs.as_ref())
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

pub(super) fn vin(point: [f64; 3], vtype: i64) -> VertexIn {
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

pub(super) fn line(a: [f64; 3], b: [f64; 3]) -> Crv {
    Crv {
        dim: 3,
        cpts: vec![[a[0], a[1], a[2], 1.0], [b[0], b[1], b[2], 1.0]],
        p: 1,
        u: vec![0.0, 0.0, 1.0, 1.0],
    }
}

pub(super) fn plane_srf(o: [f64; 3], du: [f64; 3], dv: [f64; 3]) -> Srf {
    let p = |a: f64, b: f64| {
        [
            o[0] + a * du[0] + b * dv[0],
            o[1] + a * du[1] + b * dv[1],
            o[2] + a * du[2] + b * dv[2],
            1.0,
        ]
    };
    Srf {
        net: vec![
            vec![p(0.0, 0.0), p(0.0, 1.0)],
            vec![p(1.0, 0.0), p(1.0, 1.0)],
        ],
        p: 1,
        q: 1,
        u: vec![0.0, 0.0, 1.0, 1.0],
        v: vec![0.0, 0.0, 1.0, 1.0],
    }
}

pub(super) fn edge_in(vs: usize, ve: usize, etype: i64, curve: Option<Crv>, g: [i64; 2]) -> EdgeIn {
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
pub(super) fn gen_nx2_model(rng: &mut Rng, scale: f64) -> (Model, Vec<TsectIn>) {
    let s = scale;
    let tangent = rng.chance(0.6);
    let d2 = if tangent {
        [
            -1.0,
            rng.uniform(-1e-3, 1e-3) * f64::from(u8::from(rng.chance(0.5))),
            0.0,
        ]
    } else {
        [rng.uniform(-1.0, 0.5), 1.0, rng.uniform(-0.5, 0.5)]
    };
    let targets = vec![
        line([0.0, 0.0, 0.0], [0.0, 0.0, s]), // side edge
        line([0.0, 0.0, 0.0], [s, 0.0, 0.0]), // filleted edge 1
        line([0.0, 0.0, 0.0], [d2[0] * s, d2[1] * s, d2[2] * s]), // filleted edge 2
        line([0.0, 0.0, 0.0], [0.0, -s, 0.0]), // fourth edge
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
        edge_in(
            0,
            3,
            *rng.choose(&[4i64, 1, 5]).unwrap_or(&4),
            None,
            [-1, -1],
        ),
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
    let face = |rng: &mut Rng| {
        if rng.chance(0.95) {
            rng.below(2) as i64
        } else {
            -1
        }
    };
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
    let gap = if rng.chance(0.7) {
        0.0
    } else {
        s * rng.uniform(1e-6, 0.5)
    };
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
        faces: vec![plane_srf(
            [-s, -s, 0.0],
            [3.0 * s, 0.0, 0.0],
            [0.0, 3.0 * s, 0.0],
        )],
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
    let lerp = |t: f64| {
        [
            a[0] + t * (b[0] - a[0]),
            a[1] + t * (b[1] - a[1]),
            a[2] + t * (b[2] - a[2]),
        ]
    };
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

/// Solvers for the generated corner models' geometries: none for most cases (the
/// solver-less guards stay covered), else one per geometry (a few geometries without).
fn gen_geom_solvers(rng: &mut Rng, op: CornerOp, n: usize) -> Vec<Option<SolverIn>> {
    // the patch and the geometry (the operations that read the geometries' solvers
    // through MakeAnalyticCornerSurface); the adjustments' solver uses stay refused
    if !matches!(op, CornerOp::Patch | CornerOp::CalcGeom) || !rng.chance(0.8) {
        return Vec::new();
    }
    let linear = rng.chance(0.3);
    (0..n)
        .map(|_| {
            let sv = gen_solver(rng, linear);
            if rng.chance(0.1) {
                None
            } else {
                Some(sv)
            }
        })
        .collect()
}

/// A corner case. Its geometries' solvers come from a generator of their own, seeded from
/// the case's tolerances, so that the shared stream (and every other case) is the one
/// generated without them.
fn gen_corner(rng: &mut Rng) -> Inputs {
    let mut i = gen_corner_model(rng);
    if let Inputs::Corner {
        op,
        model,
        corner,
        geom_solvers,
    } = &mut i
    {
        let mut own = Rng::new(corner.tol.to_bits() ^ corner.tan.to_bits().rotate_left(17));
        *geom_solvers = gen_geom_solvers(&mut own, *op, model.geoms.len());
    }
    i
}

fn gen_corner_model(rng: &mut Rng) -> Inputs {
    let scale = *rng
        .choose(&[1.0, 1e-3, 1e3, 10.0, 0.1, 1e-6, 1e5])
        .unwrap_or(&1.0);
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
            let n_all = if rng.chance(0.8) {
                4
            } else {
                rng.below(6) as usize
            };
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
            let kind = if rng.chance(0.85) {
                7
            } else {
                rng.below(11) as usize
            };
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
                geom_solvers: Vec::new(),
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
            let kind = if rng.chance(0.8) {
                6
            } else {
                rng.below(11) as usize
            };
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
                    orig_faces: if rng.chance(0.85) {
                        vec![[3, 0]]
                    } else {
                        Vec::new()
                    },
                    line_edges: Vec::new(),
                    arc_edges: Vec::new(),
                },
                geom_solvers: Vec::new(),
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
                    return if rng.chance(0.5) {
                        None
                    } else {
                        Some(Vec::new())
                    };
                }
                Some(
                    (0..k)
                        .map(|_| eu(rng.below(nt as u64) as usize, rng.below(2) as usize))
                        .collect(),
                )
            };
            let k1 = rng.below(4) as usize;
            let k2 = rng.below(6) as usize;
            Inputs::Corner {
                op,
                corner: CornerIn {
                    kind: rng.below(11) as usize,
                    v_edge: if nt > 0 && rng.chance(0.7) {
                        rng.below(nt as u64) as i64
                    } else {
                        -1
                    },
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
                    setback: if rng.chance(0.2) {
                        rng.uniform(0.0, 1.0)
                    } else {
                        0.0
                    },
                    tol,
                    tan,
                    tsects: Vec::new(),
                    orig_faces: Vec::new(),
                    line_edges: Vec::new(),
                    arc_edges: Vec::new(),
                },
                model,
                geom_solvers: Vec::new(),
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
        Inputs::Corner {
            op,
            model,
            corner,
            geom_solvers,
        } => corner_cpp(*op, model, corner, geom_solvers),
        Inputs::BigRad(b) => big_rad_cpp(b),
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
        Inputs::Corner {
            op,
            model,
            corner,
            geom_solvers,
        } => {
            ffi::validate_corner(&corner_input_with(model, corner, geom_solvers)?)
                .map_err(|e| harness(e.to_string()))?;
            corner_rust(k, *op, model, corner, geom_solvers)
        }
        Inputs::BigRad(b) => big_rad_rust(k, b),
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
                Inputs::Corner {
                    op,
                    model,
                    corner,
                    geom_solvers,
                } => corner_input_with(model, corner, geom_solvers)
                    .is_ok_and(|v| ffi::corner(op.ffi(), &v).is_ok()),
                Inputs::Create { .. } | Inputs::BigRad(_) => true,
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
        let mut out = default_cases(self, &ctx)?;
        out.extend(big_rad_cases()?);
        Ok(out)
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
