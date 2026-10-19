//! Runner for unit `blend.fillet_geom_p3`. C++: `solidcore_ffi::units::blend_fillet_geom_p3`.
//! Rust: `solidcore::blend::fillet_geom::p3`.
//!
//! Every case builds a target B-rep from a B-rep block of unit `topology.edgeuse_p1` (an
//! upstream `.smb` solid, a face or a wire edge; the Rust side imports the C++ export), the
//! fillet solvers (a base `SmFilletSolver` with edge uses of the target and offset surfaces
//! of inline bases; the shim's may let `SetupOffsetValues` succeed, mirrored by
//! [`ShimSolver`]) and a pseudo B-rep of fillet vertices, geometries and edges, then runs one
//! op on it and compares the op's status and the export of the whole pseudo B-rep (vertex
//! points, types, statuses and original UVs; edge curves, intervals, UV trim curves, edge
//! use geometries and vertex use intersection points; geometry statuses, side and
//! referenced edge uses).
//!
//! Selectors of target objects are taken modulo the export's counts. UV points given with a
//! `uv_face` selector are fractions of that target face's surface domain. Both sides get
//! the resolved values.
//!
//! The geometry generators make three kinds of model: a mate vertex across a rail from its
//! main vertex (classified to a target edge use) on a target face, for `CornerVert` /
//! `Mate`; two fillet surfaces meeting along a fillet/fillet edge whose processed end
//! points are dropped onto its 3D curve, for `CornerEdge` / `FilletIntFillet`; and a
//! fillet geometry with rails over a target face's edge, for `ReCalc` (which ends in the
//! unported fillet intersector unless a rail or point fails first).
//!
//! `SmFilletGeom::TopologySplit` is not compared: upstream reads the solver's
//! `SmFilletExecutive`, which the shim cannot build (unit `blend.fillet_executive_p1`);
//! its Rust port is covered by kernel tests.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::blend::fillet_custom_solver::{OffsetSurface, SurfaceId};
use solidcore::blend::fillet_geom::p1::{
    FilStatus, FilletBrep, FilletCornerId, FilletEdgeType, FilletEdgeuse, FilletGeomId,
    FilletGeomType, FilletSolverId, FilletVertex, FilletVertexType, FilletVertexuse,
};
use solidcore::blend::fillet_geom::p3::new_solver_fillet_geom;
use solidcore::blend::fillet_solver::p1::{
    FilletGeomRef, FilletSolver, FilletSolverOps, OffsetSurfaceSlot,
};
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::point_class::PointClassType;
use solidcore::intersect::surface_intersector::p1::{IntersectionPointType, SrfIndex, TsectPnt};
use solidcore::kernel::Kernel;
use solidcore::math::{Point3, BIG_DOUBLE};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::nurbs::math::{NOW, NOZ};
use solidcore::surface::offset_surface::SS_UNKNOWN;
use solidcore::topology::{
    Brep, EdgeId, Edgeuse, EdgeuseId, FaceId, VertexId, Vertexuse, VertexuseOwner,
};
use solidcore::{EntityRef, SolidError};
use solidcore_ffi::units::blend_fillet_geom_p3::{self as ffi, Op};
use solidcore_ffi::units::topology_edgeuse_p1 as ffi1;

use super::topology_edgeuse_p1::{
    brep_block_of, bspline_of, clamped_knots, gen_curve, import, path_of, push_curve, push_surface,
    sel, surface_of, BrepIn, BssIn, CrvIn, SrfIn, FILES,
};
use crate::{HarnessError, Output, Rng, UnitDiff};

/// The runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

fn h(e: impl std::fmt::Display) -> HarnessError {
    harness(e.to_string())
}

// ---- inputs -----------------------------------------------------------------------------

fn minus_one() -> i64 {
    -1
}

fn no_pair() -> [i64; 2] {
    [-1, -1]
}

fn pc_unknown() -> i64 {
    100
}

fn edge_unknown() -> i64 {
    14
}

fn ptype_unknown() -> i64 {
    1
}

/// An offset surface of a solver: `SmOffsetSurface(dist, base)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OffIn {
    base: SrfIn,
    dist: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SolverIn {
    tol: f64,
    ang: f64,
    #[serde(default)]
    tang: f64,
    /// Target edge-use selectors (`-1` none).
    #[serde(default = "no_pair")]
    eus: [i64; 2],
    #[serde(default)]
    surfs: [Option<OffIn>; 2],
    #[serde(default)]
    extend: [bool; 2],
    /// `SetupOffsetValues` succeeds without changing anything (the base fails).
    #[serde(default)]
    setup: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VertexIn {
    #[serde(default)]
    point: [f64; 3],
    #[serde(default)]
    vtype: i64,
    #[serde(default)]
    status: i64,
    #[serde(default = "minus_one")]
    corner: i64,
    #[serde(default = "pc_unknown")]
    pc_class: i64,
    /// `0` none, `1` target vertex, `2` target edge use, `3` target face.
    #[serde(default)]
    pc_kind: i64,
    /// The classification object's selector.
    #[serde(default)]
    pc_index: i64,
    /// A target face selector (`-1` none).
    #[serde(default = "minus_one")]
    pc_face: i64,
    #[serde(default)]
    t: f64,
    #[serde(default)]
    uv: [f64; 2],
    #[serde(default = "no_pair")]
    mates: [i64; 2],
}

/// A vertex use's intersection point (`SmTsectPnt`); with `uv_face` (a target face
/// selector) the UV positions are fractions of that face's surface domain.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TsectIn {
    #[serde(default = "ptype_unknown")]
    ptype: i64,
    #[serde(default)]
    uv: [[f64; 2]; 2],
    #[serde(default = "minus_one")]
    uv_face: i64,
    #[serde(default)]
    crv: [f64; 3],
    #[serde(default)]
    deriv: [f64; 3],
    #[serde(default)]
    cparam: f64,
}

impl Default for TsectIn {
    fn default() -> Self {
        Self {
            ptype: 1,
            uv: [[0.0; 2]; 2],
            uv_face: -1,
            crv: [0.0; 3],
            deriv: [0.0; 3],
            cparam: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EuIn {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    uv_curve: Option<CrvIn>,
    #[serde(default = "minus_one")]
    geom: i64,
    #[serde(default)]
    vu_uv: [f64; 2],
    #[serde(default)]
    tsect: TsectIn,
}

impl Default for EuIn {
    fn default() -> Self {
        Self {
            uv_curve: None,
            geom: -1,
            vu_uv: [0.0; 2],
            tsect: TsectIn::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EdgeIn {
    vs: usize,
    ve: usize,
    #[serde(default = "edge_unknown")]
    etype: i64,
    #[serde(default)]
    status: i64,
    #[serde(default = "minus_one")]
    corner: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    curve: Option<CrvIn>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    interval: Option<[f64; 2]>,
    eus: [EuIn; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GeomIn {
    #[serde(default)]
    gtype: i64,
    #[serde(default)]
    status: i64,
    #[serde(default = "minus_one")]
    solver: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    surface: Option<SrfIn>,
    #[serde(default = "no_pair")]
    rails: [i64; 2],
    #[serde(default)]
    other1: Vec<usize>,
    #[serde(default)]
    other2: Vec<usize>,
    #[serde(default)]
    sides: Vec<[usize; 2]>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ModelIn {
    #[serde(default)]
    solvers: Vec<SolverIn>,
    #[serde(default)]
    vertices: Vec<VertexIn>,
    #[serde(default)]
    geoms: Vec<GeomIn>,
    #[serde(default)]
    edges: Vec<EdgeIn>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Export,
    CornerVert {
        v: usize,
    },
    Mate {
        v: usize,
    },
    CornerEdge {
        e: usize,
    },
    FilletIntFillet {
        e: usize,
    },
    ReCalc {
        g: usize,
        #[serde(default)]
        analytic: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dir: Option<[f64; 3]>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ref_curve: Option<CrvIn>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    brep: BrepIn,
    model: ModelIn,
    #[serde(flatten)]
    op: OpIn,
    /// A regression input the FFI or the shim must refuse (upstream crashes on it): the
    /// case is recorded as rejected, never run on the Rust side.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    expected_rejection: bool,
}

// ---- the target B-rep and resolution -------------------------------------------------------

/// The case's target B-rep: the C++ export's counts and the imported Rust B-rep.
struct Target {
    path: String,
    block: Vec<f64>,
    counts: [usize; 10],
    brep: Brep,
}

/// Builds the target on both sides; `Err(output)` when it cannot be built.
fn target(brep: &BrepIn) -> Result<Result<Target, Output>, HarnessError> {
    let path = path_of(Some(brep))?;
    let block = brep_block_of(Some(brep), false, false)?;
    let ex = ffi1::eval(ffi1::Op::Export, &path, &block)
        .map_err(|e| harness(format!("c++ export: {e}")))?;
    let Some(counts) = ffi1::counts(&ex) else {
        let st = ex.get(1).copied().unwrap_or(0.0);
        return Ok(Err(Output::new().values("flat", vec![1.0, st])));
    };
    let brep = import(&ex)?;
    Ok(Ok(Target {
        path,
        block,
        counts,
        brep,
    }))
}

impl Target {
    fn n_f(&self) -> usize {
        self.counts[3]
    }
    fn n_eu(&self) -> usize {
        self.counts[6]
    }
    fn n_v(&self) -> usize {
        self.counts[9]
    }

    fn face(&self, s: i64) -> Option<FaceId> {
        u32::try_from(sel(s, self.n_f()))
            .ok()
            .map(FaceId::from_index)
    }

    /// `(u0, v0, u1, v1)` of the face's surface domain.
    fn face_domain(&self, s: i64) -> Result<Option<[f64; 4]>, HarnessError> {
        let Some(f) = self.face(s) else {
            return Ok(None);
        };
        let Some(srf) = self.brep.face(f).map_err(h)?.surface.as_ref() else {
            return Ok(None);
        };
        let d = srf.natural_uv_domain();
        let (u, v) = (d.u_interval(), d.v_interval());
        Ok(Some([u.min(), v.min(), u.max(), v.max()]))
    }

    /// A resolved classification object `(kind, export index)`.
    fn pc(&self, kind: i64, s: i64) -> (i64, i64) {
        let i = match kind {
            1 => sel(s, self.n_v()),
            2 => sel(s, self.n_eu()),
            3 => sel(s, self.n_f()),
            _ => -1,
        };
        if i < 0 {
            (0, -1)
        } else {
            (kind, i)
        }
    }
}

/// The intersection point of a vertex use, resolved.
#[derive(Debug, Clone)]
struct Tsect {
    ptype: i64,
    uv: [[f64; 2]; 2],
    crv: [f64; 3],
    deriv: [f64; 3],
    cparam: f64,
}

fn resolve_tsect(t: &TsectIn, tg: &Target) -> Result<Tsect, HarnessError> {
    let mut uv = t.uv;
    if t.uv_face >= 0 {
        if let Some([u0, v0, u1, v1]) = tg.face_domain(t.uv_face)? {
            for p in &mut uv {
                *p = [u0 + (u1 - u0) * p[0], v0 + (v1 - v0) * p[1]];
            }
        }
    }
    Ok(Tsect {
        ptype: t.ptype,
        uv,
        crv: t.crv,
        deriv: t.deriv,
        cparam: t.cparam,
    })
}

// ---- C++ ---------------------------------------------------------------------------------

fn b(v: bool) -> f64 {
    if v {
        1.0
    } else {
        0.0
    }
}

/// The input after the B-rep block (layout in the FFI wrapper).
fn model_input(i: &Inputs, tg: &Target) -> Result<Vec<f64>, HarnessError> {
    let m = &i.model;
    let mut o = tg.block.clone();

    o.push(m.solvers.len() as f64);
    for s in &m.solvers {
        o.extend([s.tol, s.ang, s.tang]);
        for &e in &s.eus {
            o.push(sel(e, tg.n_eu()) as f64);
        }
        for off in &s.surfs {
            match off {
                Some(off) => {
                    o.push(1.0);
                    push_surface(&mut o, &off.base)?;
                    o.push(off.dist);
                }
                None => o.push(0.0),
            }
        }
        o.extend([b(s.extend[0]), b(s.extend[1]), b(s.setup)]);
    }

    o.push(m.vertices.len() as f64);
    for v in &m.vertices {
        o.extend(v.point);
        o.extend([
            v.vtype as f64,
            v.status as f64,
            v.corner as f64,
            v.pc_class as f64,
        ]);
        let (kind, idx) = tg.pc(v.pc_kind, v.pc_index);
        o.extend([kind as f64, idx as f64, sel(v.pc_face, tg.n_f()) as f64]);
        o.extend([v.t, v.uv[0], v.uv[1]]);
    }

    o.push(m.geoms.len() as f64);
    for g in &m.geoms {
        o.extend([g.gtype as f64, g.status as f64, g.solver as f64]);
        match &g.surface {
            Some(s) => {
                o.push(1.0);
                push_surface(&mut o, s)?;
            }
            None => o.push(0.0),
        }
    }

    o.push(m.edges.len() as f64);
    for e in &m.edges {
        o.extend([
            e.vs as f64,
            e.ve as f64,
            e.etype as f64,
            e.status as f64,
            e.corner as f64,
        ]);
        match &e.curve {
            Some(c) => {
                o.push(1.0);
                push_curve(&mut o, c)?;
            }
            None => o.push(0.0),
        }
        match e.interval {
            Some([a, c]) => o.extend([1.0, a, c]),
            None => o.push(0.0),
        }
        for eu in &e.eus {
            match &eu.uv_curve {
                Some(c) => {
                    o.push(1.0);
                    push_curve(&mut o, c)?;
                }
                None => o.push(0.0),
            }
            o.push(eu.geom as f64);
            o.extend(eu.vu_uv);
            let t = resolve_tsect(&eu.tsect, tg)?;
            o.push(t.ptype as f64);
            o.extend(t.uv[0]);
            o.extend(t.uv[1]);
            o.extend(t.crv);
            o.extend(t.deriv);
            o.push(t.cparam);
        }
    }

    for g in &m.geoms {
        o.extend([g.rails[0] as f64, g.rails[1] as f64]);
        o.push(g.other1.len() as f64);
        o.extend(g.other1.iter().map(|&x| x as f64));
        o.push(g.other2.len() as f64);
        o.extend(g.other2.iter().map(|&x| x as f64));
        o.push(g.sides.len() as f64);
        for &[e, w] in &g.sides {
            o.extend([e as f64, w as f64]);
        }
    }

    for v in &m.vertices {
        o.extend([v.mates[0] as f64, v.mates[1] as f64]);
    }

    match &i.op {
        OpIn::Export => {}
        OpIn::CornerVert { v } | OpIn::Mate { v } => o.push(*v as f64),
        OpIn::CornerEdge { e } | OpIn::FilletIntFillet { e } => o.push(*e as f64),
        OpIn::ReCalc {
            g,
            analytic,
            dir,
            ref_curve,
        } => {
            o.extend([*g as f64, b(*analytic), b(dir.is_some())]);
            o.extend(dir.unwrap_or([0.0; 3]));
            match ref_curve {
                Some(c) => {
                    o.push(1.0);
                    push_curve(&mut o, c)?;
                }
                None => o.push(0.0),
            }
        }
    }
    Ok(o)
}

fn op_of(op: &OpIn) -> Op {
    match op {
        OpIn::Export => Op::Export,
        OpIn::CornerVert { .. } => Op::CornerVert,
        OpIn::Mate { .. } => Op::Mate,
        OpIn::CornerEdge { .. } => Op::CornerEdge,
        OpIn::FilletIntFillet { .. } => Op::FilletIntFillet,
        OpIn::ReCalc { .. } => Op::ReCalc,
    }
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let tg = match target(&i.brep)? {
        Ok(t) => t,
        Err(out) => return Ok(out),
    };
    let input = model_input(i, &tg)?;
    let result = ffi::eval(op_of(&i.op), &tg.path, &input);
    if i.expected_rejection {
        // Only a flagged case may be recorded as rejected.
        return match result {
            Err(solidcore_ffi::FfiError::InvalidArgument(reason)) => Ok(Output::rejected(reason)),
            Err(solidcore_ffi::FfiError::Status(solidcore_ffi::SC_ERR_ARG)) => {
                Ok(Output::rejected("shim_rejected_model"))
            }
            Err(e) => Err(harness(format!(
                "expected-rejection fixture failed in another way: {e}"
            ))),
            Ok(_) => Err(harness("expected-rejection fixture was not refused")),
        };
    }
    let flat = result.map_err(|e| harness(format!("c++: {e}")))?;
    Ok(Output::new().values("flat", flat))
}

// ---- Rust --------------------------------------------------------------------------------

/// The shim's solver: a base [`FilletSolver`] whose `SetupOffsetValues` succeeds without
/// changing anything when `setup`.
struct ShimSolver {
    base: FilletSolver,
    setup: bool,
}

impl FilletSolverOps for ShimSolver {
    fn solver(&self) -> &FilletSolver {
        &self.base
    }

    fn solver_mut(&mut self) -> &mut FilletSolver {
        &mut self.base
    }

    fn setup_offset_values(
        &mut self,
        tsect: &TsectPnt,
        offset_dist: Option<&mut f64>,
    ) -> solidcore::Result<()> {
        if !self.setup {
            return self.base.setup_offset_values(tsect, offset_dist);
        }
        Ok(())
    }
}

/// The Rust-side model.
struct RustModel {
    k: Kernel,
    solvers: Vec<ShimSolver>,
    fb: FilletBrep,
    vertices: Vec<VertexId>,
    edges: Vec<EdgeId>,
    geoms: Vec<FilletGeomId>,
}

/// Two edge uses (primary `SM_OT_SAME` at `vs`, mate `SM_OT_OPPOSITE` at `ve`) on `e`, as
/// `MakeFilletEdge` links them.
fn link_uses(
    b: &mut Brep,
    e: EdgeId,
    vs: VertexId,
    ve: VertexId,
) -> Result<[EdgeuseId; 2], HarnessError> {
    let eu1 = b.add_edgeuse_record(Edgeuse::default()).map_err(h)?;
    let eu2 = b.add_edgeuse_record(Edgeuse::default()).map_err(h)?;
    let vu1 = b.add_vertexuse_record(Vertexuse::default()).map_err(h)?;
    let vu2 = b.add_vertexuse_record(Vertexuse::default()).map_err(h)?;
    for (eu, vu, v, o) in [
        (eu1, vu1, vs, OrientType::Same),
        (eu2, vu2, ve, OrientType::Opposite),
    ] {
        let r = b.edgeuse_mut(eu).map_err(h)?;
        r.edge = Some(e);
        r.orientation = o;
        r.vertexuse = Some(vu);
        let u = b.vertexuse_mut(vu).map_err(h)?;
        u.vertex = Some(v);
        u.owner = VertexuseOwner::Edgeuse(Some(eu));
        b.vertex_mut(v).map_err(h)?.vertexuses.push(vu);
    }
    b.edge_mut(e).map_err(h)?.edgeuses = vec![eu1, eu2];
    Ok([eu1, eu2])
}

fn opt(x: i64, n: usize) -> Option<usize> {
    usize::try_from(x).ok().filter(|&i| i < n)
}

impl RustModel {
    fn build(i: &Inputs, tg: &Target) -> Result<Self, HarnessError> {
        let m = &i.model;
        let k = Kernel::default();

        let mut solvers = Vec::new();
        for s in &m.solvers {
            let mut fs = FilletSolver::new(s.tol, s.ang, s.tang);
            for (slot, &e) in fs.edgeuses.iter_mut().zip(&s.eus) {
                *slot = u32::try_from(sel(e, tg.n_eu()))
                    .ok()
                    .map(EdgeuseId::from_index);
            }
            for (rail, off) in s.surfs.iter().enumerate() {
                if let Some(off) = off {
                    let base = surface_of(&off.base)?;
                    let surface = OffsetSurface::new(
                        &k,
                        off.dist,
                        base,
                        SurfaceId::Free(rail as u32),
                        SS_UNKNOWN,
                    )
                    .map_err(h)?;
                    fs.set_surface(
                        rail,
                        Some(OffsetSurfaceSlot {
                            key: rail as u64,
                            surface,
                        }),
                    )
                    .map_err(h)?;
                }
            }
            fs.extend_before = s.extend[0];
            fs.extend_after = s.extend[1];
            solvers.push(ShimSolver {
                base: fs,
                setup: s.setup,
            });
        }

        let mut fb = FilletBrep::new();
        let corner = |c: i64| u32::try_from(c).ok().map(FilletCornerId);
        let mut vertices = Vec::new();
        for v in &m.vertices {
            let id = fb
                .brep_mut()
                .add_vertex(Point3::new(v.point[0], v.point[1], v.point[2]))
                .map_err(h)?;
            let mut fv = FilletVertex::new(&k, corner(v.corner));
            fv.set_fillet_vertex_type(
                FilletVertexType::from_code(v.vtype).ok_or_else(|| harness("bad vertex type"))?,
            );
            fv.set_status(FilStatus::from_code(v.status).ok_or_else(|| harness("bad status"))?);
            let class = i32::try_from(v.pc_class)
                .ok()
                .and_then(PointClassType::from_code)
                .ok_or_else(|| harness("bad point class"))?;
            let (kind, idx) = tg.pc(v.pc_kind, v.pc_index);
            let idx = u32::try_from(idx).unwrap_or(0);
            let object = match kind {
                1 => Some(EntityRef::Vertex(VertexId::from_index(idx))),
                2 => Some(EntityRef::Edgeuse(EdgeuseId::from_index(idx))),
                3 => Some(EntityRef::Face(FaceId::from_index(idx))),
                _ => None,
            };
            fv.set_point_class(class, object);
            if let Some(f) = tg.face(v.pc_face) {
                fv.point_classification_mut()
                    .set_face(Some(EntityRef::Face(f)));
            }
            fv.set_original_t_param(v.t);
            fv.set_original_uv(Point2::new(v.uv[0], v.uv[1]));
            fb.insert_fillet_vertex(id, fv);
            vertices.push(id);
        }

        let mut geoms = Vec::new();
        for g in &m.geoms {
            let gtype =
                FilletGeomType::from_code(g.gtype).ok_or_else(|| harness("bad geom type"))?;
            let mut geom = match opt(g.solver, solvers.len()) {
                Some(si) => {
                    let fs = solvers.get(si).ok_or_else(|| harness("solver"))?;
                    new_solver_fillet_geom(&k, FilletSolverId(si as u32), &fs.base, gtype)
                        .map_err(h)?
                }
                None => solidcore::blend::fillet_geom::p1::FilletGeom::new(&k, None, gtype)
                    .map_err(h)?,
            };
            geom.status = FilStatus::from_code(g.status).ok_or_else(|| harness("bad status"))?;
            if let Some(s) = &g.surface {
                geom.set_fillet_surface(Some(surface_of(s)?));
            }
            geoms.push(fb.add_geom(geom));
        }

        let mut edges = Vec::new();
        for e in &m.edges {
            let vs = *vertices.get(e.vs).ok_or_else(|| harness("edge vertex"))?;
            let ve = *vertices.get(e.ve).ok_or_else(|| harness("edge vertex"))?;
            let id = fb.new_fillet_edge(vs, ve, corner(e.corner)).map_err(h)?;
            let eus = link_uses(fb.brep_mut(), id, vs, ve)?;
            {
                let fe = fb.fillet_edge_mut(id).map_err(h)?;
                fe.set_fillet_edge_type(
                    FilletEdgeType::from_code(e.etype).ok_or_else(|| harness("bad edge type"))?,
                );
                fe.set_status(FilStatus::from_code(e.status).ok_or_else(|| harness("bad status"))?);
            }
            {
                let edge = fb.brep_mut().edge_mut(id).map_err(h)?;
                if let Some(c) = &e.curve {
                    edge.curve = Some(Curve::from(bspline_of(c)?));
                }
                if let Some([a, c]) = e.interval {
                    edge.interval = Extent1d::from_min_max(a, c);
                }
            }
            for (eu, spec) in eus.iter().zip(&e.eus) {
                fb.insert_fillet_edgeuse(*eu, FilletEdgeuse::default());
                if let Some(c) = &spec.uv_curve {
                    fb.brep_mut().edgeuse_mut(*eu).map_err(h)?.uv_trim_curve =
                        Some(Curve::from(bspline_of(c)?));
                }
                if let Some(gi) = opt(spec.geom, geoms.len()) {
                    let g = *geoms.get(gi).ok_or_else(|| harness("edge use geom"))?;
                    fb.set_edgeuse_fillet_geom(*eu, Some(g)).map_err(h)?;
                }
                let vu = fb
                    .brep()
                    .edgeuse(*eu)
                    .map_err(h)?
                    .vertexuse
                    .ok_or_else(|| harness("edge use without vertex use"))?;
                let mut fvu = FilletVertexuse::new();
                fvu.set_uv_point(Point2::new(spec.vu_uv[0], spec.vu_uv[1]));
                let t = resolve_tsect(&spec.tsect, tg)?;
                let ts = fvu.tsect_pnt_mut();
                ts.point_type = i32::try_from(t.ptype)
                    .ok()
                    .and_then(IntersectionPointType::from_code)
                    .ok_or_else(|| harness("bad point type"))?;
                *ts.uv_pos_mut(SrfIndex::First) = Point2::new(t.uv[0][0], t.uv[0][1]);
                *ts.uv_pos_mut(SrfIndex::Second) = Point2::new(t.uv[1][0], t.uv[1][1]);
                *ts.crv_pos_mut() = Point3::new(t.crv[0], t.crv[1], t.crv[2]);
                *ts.crv_deriv_mut() = Vec3::new(t.deriv[0], t.deriv[1], t.deriv[2]);
                ts.curve_parameter = t.cparam;
                fb.insert_fillet_vertexuse(vu, fvu);
            }
            edges.push(id);
        }

        for (g, spec) in geoms.iter().zip(&m.geoms) {
            let eid = |x: usize| edges.get(x).copied().ok_or_else(|| harness("geom edge"));
            let mut sides = Vec::new();
            for &[e, w] in &spec.sides {
                let edge = eid(e)?;
                let eu = fb
                    .brep()
                    .edge(edge)
                    .map_err(h)?
                    .edgeuses
                    .get(w)
                    .copied()
                    .ok_or_else(|| harness("side edge use"))?;
                sides.push(eu);
            }
            let geom = fb.geom_mut(*g).map_err(h)?;
            for (r, &rail) in spec.rails.iter().enumerate() {
                geom.set_rail(r, opt(rail, edges.len()).map(eid).transpose()?);
            }
            for &x in &spec.other1 {
                geom.other_rails1.push(eid(x)?);
            }
            for &x in &spec.other2 {
                geom.other_rails2.push(eid(x)?);
            }
            for eu in sides {
                geom.add_side_fillet_edgeuse(eu);
            }
        }

        for (v, spec) in vertices.iter().zip(&m.vertices) {
            for (j, &mate) in spec.mates.iter().enumerate() {
                let mate = opt(mate, vertices.len()).and_then(|x| vertices.get(x).copied());
                fb.fillet_vertex_mut(*v).map_err(h)?.set_mate(j, mate);
            }
        }

        Ok(Self {
            k,
            solvers,
            fb,
            vertices,
            edges,
            geoms,
        })
    }

    fn put_eu(&self, o: &mut Vec<f64>, eu: EdgeuseId) {
        for (i, &e) in self.edges.iter().enumerate() {
            if let Ok(edge) = self.fb.brep().edge(e) {
                if let Some(w) = edge.edgeuses.iter().position(|&x| x == eu) {
                    o.extend([i as f64, w as f64]);
                    return;
                }
            }
        }
        o.extend([-1.0, -1.0]);
    }

    /// The export (mirrors the shim's).
    fn put(&self, o: &mut Vec<f64>) -> Result<(), HarnessError> {
        let b = self.fb.brep();
        o.push(self.vertices.len() as f64);
        for &v in &self.vertices {
            let fv = self.fb.fillet_vertex(v).map_err(h)?;
            let p = self.fb.vertex_point(v).map_err(h)?;
            o.extend([p.x, p.y, p.z]);
            o.push(fv.fillet_vertex_type() as i64 as f64);
            o.push(fv.status() as i64 as f64);
            let uv = fv.original_uv();
            o.extend([uv.x, uv.y]);
        }
        o.push(self.edges.len() as f64);
        for &e in &self.edges {
            let fe = self.fb.fillet_edge(e).map_err(h)?;
            let edge = b.edge(e).map_err(h)?;
            o.push(fe.fillet_edge_type() as i64 as f64);
            o.push(fe.status() as i64 as f64);
            dump_curve(o, edge.curve.as_ref());
            o.extend([edge.interval.min(), edge.interval.max()]);
            for &eu in edge.edgeuses.iter().take(2) {
                let r = b.edgeuse(eu).map_err(h)?;
                dump_curve(o, r.uv_trim_curve.as_ref());
                let g = self.fb.fillet_edgeuse(eu).map_err(h)?.fillet_geom();
                o.push(
                    g.and_then(|g| self.geoms.iter().position(|&x| x == g))
                        .map_or(-1.0, |i| i as f64),
                );
                let vu = r
                    .vertexuse
                    .ok_or_else(|| harness("edge use without vertex use"))?;
                let t = self.fb.fillet_vertexuse(vu).map_err(h)?.tsect_pnt();
                o.push(f64::from(t.point_type.code()));
                let (a, c) = (t.uv_pos(SrfIndex::First), t.uv_pos(SrfIndex::Second));
                o.extend([a.x, a.y, c.x, c.y]);
                let p = t.crv_pos();
                o.extend([p.x, p.y, p.z, t.curve_parameter]);
            }
        }
        o.push(self.geoms.len() as f64);
        for &g in &self.geoms {
            let geom = self.fb.geom(g).map_err(h)?;
            o.push(geom.status() as i64 as f64);
            o.push(geom.side_eus.len() as f64);
            for &eu in &geom.side_eus {
                self.put_eu(o, eu);
            }
            o.push(geom.ref_eus.len() as f64);
            for &eu in &geom.ref_eus {
                self.put_eu(o, eu);
            }
        }
        Ok(())
    }
}

fn dump_curve(o: &mut Vec<f64>, c: Option<&Curve>) {
    let bs = match c.map(Curve::kind) {
        Some(CurveKind::BSpline(bs)) => Some(bs),
        _ => None,
    };
    let Some(cur) = bs.and_then(BSplineCurve::gw_nurb) else {
        o.push(0.0);
        return;
    };
    o.push(1.0);
    o.extend([
        bs.map_or(3, BSplineCurve::dim) as f64,
        cur.n() as f64,
        cur.degree() as f64,
        cur.m() as f64,
    ]);
    for p in cur.cpts() {
        o.extend([p.x, p.y, p.z, p.w]);
    }
    o.extend_from_slice(cur.knots());
}

/// The `SmStatus` of a Rust error; `-1` for one upstream cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let tg = match target(&i.brep)? {
        Ok(t) => t,
        Err(out) => return Ok(out),
    };
    let mut m = RustModel::build(i, &tg)?;
    let k = std::mem::take(&mut m.k);
    let bases: Vec<FilletSolver> = m.solvers.iter().map(|s| s.base.clone()).collect();
    let res = match &i.op {
        OpIn::Export => Ok(()),
        OpIn::CornerVert { v } => {
            let v = *m.vertices.get(*v).ok_or_else(|| harness("vertex"))?;
            m.fb.calc_corner_vert_geom(&k, v, &tg.brep, &bases)
        }
        OpIn::Mate { v } => {
            let v = *m.vertices.get(*v).ok_or_else(|| harness("vertex"))?;
            m.fb.calc_mate_geom(&k, v, &tg.brep, &bases)
        }
        OpIn::CornerEdge { e } => {
            let e = *m.edges.get(*e).ok_or_else(|| harness("edge"))?;
            // the shim refuses the part 6 calculators (its corners carry no data)
            m.fb.calc_corner_edge_geom(&k, e, &tg.brep, &[], &bases)
        }
        OpIn::FilletIntFillet { e } => {
            let e = *m.edges.get(*e).ok_or_else(|| harness("edge"))?;
            let corner = m.fb.fillet_edge(e).map_err(h)?.fillet_corner();
            m.fb.calc_fillet_int_fillet(&k, e, corner, &bases)
        }
        OpIn::ReCalc {
            g,
            analytic,
            dir,
            ref_curve,
        } => {
            let g = *m.geoms.get(*g).ok_or_else(|| harness("geometry"))?;
            let si =
                m.fb.geom(g)
                    .map_err(h)?
                    .fillet_solver()
                    .ok_or_else(|| harness("geometry without solver"))?;
            let reference = ref_curve
                .as_ref()
                .map(bspline_of)
                .transpose()?
                .map(Curve::from);
            // the solver's own geometry (made by its constructor) is none of the model's
            let geometry_id = |_: FilletGeomRef| Ok(FilletGeomId(u32::MAX));
            let solver = m
                .solvers
                .get_mut(si.0 as usize)
                .ok_or_else(|| harness("solver"))?;
            m.fb.re_calc_fillet_geom(
                &k,
                g,
                solver,
                &tg.brep,
                *analytic,
                dir.map(|d| Vec3::new(d[0], d[1], d[2])),
                reference.as_ref(),
                &geometry_id,
            )
        }
    };
    if let Err(e) = &res {
        if let Some(out) = Output::unported(e) {
            return Ok(out);
        }
    }
    let mut o = vec![0.0, res.as_ref().err().map_or(0.0, status)];
    m.put(&mut o)?;
    Ok(Output::new().values("flat", o))
}

// ---- generators ----------------------------------------------------------------------------

/// Upstream test files with faces whose edge uses lie on several faces.
const SOLID_FILES: [&str; 6] = [
    "Mouse.smb",
    "Endcap.smb",
    "box_with_large_fillet.smb",
    "cone.smb",
    "torus.smb",
    "vase_rev.smb",
];

fn gen_bspline_surface(rng: &mut Rng, scale: f64, z: f64) -> SrfIn {
    let p = 1 + rng.below(3) as usize;
    let q = 1 + rng.below(3) as usize;
    let n = p + rng.below(3) as usize;
    let m = q + rng.below(3) as usize;
    let net = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    [
                        i as f64 * scale / n as f64,
                        j as f64 * scale / m as f64,
                        (z + rng.uniform(-0.2, 0.2)) * scale,
                        NOW,
                    ]
                })
                .collect()
        })
        .collect();
    SrfIn::Bspline(BssIn {
        oob: false,
        form: 0,
        net,
        p,
        q,
        u: clamped_knots(rng, p, n, 0.0, 1.0),
        v: clamped_knots(rng, q, m, 0.0, 1.0),
    })
}

/// A curve of dimension `dim` on `[a, b]` through roughly `[0, scale]` in x.
fn gen_curve_on(rng: &mut Rng, dim: u64, scale: f64, a: f64, b: f64) -> CrvIn {
    let p = 1 + rng.below(3) as usize;
    let n = p + rng.below(3) as usize;
    let cpts = (0..=n)
        .map(|i| {
            let x = i as f64 / n as f64 * scale;
            let y = rng.uniform(-0.3, 0.3) * scale;
            let z = if dim == 2 {
                NOZ
            } else {
                rng.uniform(-0.3, 0.3) * scale
            };
            [x, y, z, NOW]
        })
        .collect();
    CrvIn {
        dim,
        cpts,
        p,
        u: clamped_knots(rng, p, n, a, b),
    }
}

fn point_on(c: &CrvIn, t: f64) -> [f64; 3] {
    bspline_of(c)
        .ok()
        .and_then(|bc| bc.evaluate_point(t).ok())
        .map_or([0.0; 3], |p| [p.x, p.y, p.z])
}

fn rail_edge(vs: usize, ve: usize, geom: i64, tsect: [TsectIn; 2]) -> EdgeIn {
    let [t0, t1] = tsect;
    EdgeIn {
        vs,
        ve,
        etype: FilletEdgeType::Rail as i64,
        status: 0,
        corner: -1,
        curve: None,
        interval: None,
        eus: [
            EuIn {
                geom,
                tsect: t0,
                ..EuIn::default()
            },
            EuIn {
                geom,
                tsect: t1,
                ..EuIn::default()
            },
        ],
    }
}

fn vertex(vtype: FilletVertexType, status: FilStatus) -> VertexIn {
    VertexIn {
        point: [0.0; 3],
        vtype: vtype as i64,
        status: status as i64,
        corner: -1,
        pc_class: 100,
        pc_kind: 0,
        pc_index: 0,
        pc_face: -1,
        t: 0.0,
        uv: [0.0; 2],
        mates: [-1, -1],
    }
}

fn solid_brep(rng: &mut Rng) -> BrepIn {
    BrepIn::File {
        file: (*rng.choose(&SOLID_FILES).unwrap_or(&"Mouse.smb")).to_string(),
        rebuild: false,
    }
}

/// A mate vertex (and possibly a chain of mates) across a rail from its main vertex.
fn gen_mate(rng: &mut Rng) -> Inputs {
    let brep = if rng.chance(0.85) {
        solid_brep(rng)
    } else {
        BrepIn::File {
            file: (*rng.choose(&FILES).unwrap_or(&"Mouse.smb")).to_string(),
            rebuild: false,
        }
    };
    let solver = SolverIn {
        tol: 1e-6,
        ang: 0.01,
        tang: 0.01,
        eus: [rng.below(1000) as i64, rng.below(1000) as i64],
        surfs: [None, None],
        extend: [false, false],
        setup: false,
    };
    let face = rng.below(1000) as i64;

    // main vertex: processed (or computed: an unprocessed fillet-x-edgeuse stays so)
    let mut main = vertex(
        *rng.choose(&[
            FilletVertexType::RailEnd,
            FilletVertexType::RailXEdgeuse,
            FilletVertexType::FilletXEdgeuse,
        ])
        .unwrap_or(&FilletVertexType::RailEnd),
        FilStatus::Processed,
    );
    if main.vtype == FilletVertexType::FilletXEdgeuse as i64 && rng.chance(0.3) {
        main.status = FilStatus::Unprocessed as i64;
    }
    if rng.chance(0.92) {
        main.pc_kind = 2;
        main.pc_index = rng.below(1000) as i64;
        main.pc_class = 102;
    }

    // the mate: on a face, or (rarely) unclassified
    let mut mate = vertex(FilletVertexType::Mate, FilStatus::Unprocessed);
    if rng.chance(0.95) {
        mate.pc_kind = 3;
        mate.pc_index = face;
        mate.pc_class = 103;
    }
    mate.mates = [0, -1];
    mate.uv = [rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)];

    let other_a = vertex(FilletVertexType::RailEnd, FilStatus::Processed);
    let other_b = vertex(FilletVertexType::RailEnd, FilStatus::Processed);
    let mut vertices = vec![main, mate, other_a, other_b];

    // a fraction of the face's domain, sometimes outside it (an extended surface)
    let mut frac = || {
        if rng.chance(0.85) {
            [rng.uniform(0.05, 0.95), rng.uniform(0.05, 0.95)]
        } else {
            [rng.uniform(-0.2, 1.2), rng.uniform(-0.2, 1.2)]
        }
    };
    let main_tsect = TsectIn {
        uv: [frac(), frac()],
        uv_face: face,
        ..TsectIn::default()
    };
    let geom_of_mate = if rng.chance(0.93) { 0 } else { -1 };
    let mut edges = vec![
        rail_edge(0, 2, 0, [main_tsect, TsectIn::default()]),
        rail_edge(1, 3, geom_of_mate, [TsectIn::default(), TsectIn::default()]),
    ];
    if rng.chance(0.1) {
        // the mate's edge is not a rail: no fillet geometry (SM_ERR)
        if let Some(e) = edges.get_mut(1) {
            e.etype = FilletEdgeType::CrossSection as i64;
        }
    }
    let mut op = OpIn::CornerVert { v: 1 };
    if rng.chance(0.3) {
        op = OpIn::Mate { v: 1 };
    }
    if rng.chance(0.2) {
        // a second mate whose main vertex is the first mate
        let mut mate2 = vertex(FilletVertexType::Mate, FilStatus::Unprocessed);
        mate2.pc_kind = 3;
        mate2.pc_index = rng.below(1000) as i64;
        mate2.pc_class = 103;
        mate2.mates = [1, -1];
        vertices.push(mate2);
        vertices.push(vertex(FilletVertexType::RailEnd, FilStatus::Processed));
        // the first mate is the second's main vertex, cast to an edge use there and to a
        // face in its own evaluation: unclassified, it fails (status ignored) and the
        // second mate stays unprocessed
        if let Some(m1) = vertices.get_mut(1) {
            m1.pc_kind = 0;
        }
        edges.push(rail_edge(4, 5, 0, [TsectIn::default(), TsectIn::default()]));
        op = OpIn::CornerVert { v: 4 };
    }
    if rng.chance(0.08) {
        // already processed: nothing to do
        if let Some(v) = vertices.get_mut(1) {
            v.status = FilStatus::Processed as i64;
        }
    }
    Inputs {
        brep,
        model: ModelIn {
            solvers: vec![solver],
            vertices,
            geoms: vec![GeomIn {
                gtype: 0,
                status: 0,
                solver: 0,
                surface: None,
                rails: [0, 1],
                other1: vec![],
                other2: vec![],
                sides: vec![],
            }],
            edges,
        },
        op,
        expected_rejection: false,
    }
}

/// A vertex classified to a target vertex (`SM_FV_ON_VERTEX`), or another type the
/// dispatcher leaves alone.
fn gen_on_vertex(rng: &mut Rng) -> Inputs {
    let mut v = vertex(
        *rng.choose(&[
            FilletVertexType::OnVertex,
            FilletVertexType::OnVertex,
            FilletVertexType::OnVertex,
            FilletVertexType::FilletXEdgeuse,
            FilletVertexType::RailEnd,
            FilletVertexType::Unknown,
            FilletVertexType::CliffRailXEdge,
        ])
        .unwrap_or(&FilletVertexType::OnVertex),
        FilStatus::Unprocessed,
    );
    // (an on-vertex vertex without a target vertex dereferences NULL upstream)
    if v.vtype == FilletVertexType::OnVertex as i64 || rng.chance(0.9) {
        v.pc_kind = 1;
        v.pc_index = rng.below(1000) as i64;
        v.pc_class = 101;
    }
    if rng.chance(0.1) {
        v.status = FilStatus::Processed as i64;
    }
    Inputs {
        brep: solid_brep(rng),
        model: ModelIn {
            vertices: vec![v],
            ..ModelIn::default()
        },
        op: OpIn::CornerVert { v: 0 },
        expected_rejection: false,
    }
}

/// Two fillet surfaces meeting along a fillet/fillet edge.
fn gen_fif(rng: &mut Rng) -> Inputs {
    let scale = *rng.choose(&[1e-3, 1.0, 1.0, 1.0, 1e3]).unwrap_or(&1.0);
    let (a, bb) = if rng.chance(0.6) {
        (0.0, 1.0)
    } else {
        (rng.uniform(-3.0, 0.0), rng.uniform(1.0, 4.0))
    };
    let curve = gen_curve_on(rng, 3, scale, a, bb);
    let (mut ta, mut tb) = (
        a + (bb - a) * rng.uniform(0.0, 0.45),
        a + (bb - a) * rng.uniform(0.55, 1.0),
    );
    if rng.chance(0.1) {
        std::mem::swap(&mut ta, &mut tb); // SetMinMax fails: the natural interval stays
    }
    let noise = |rng: &mut Rng| {
        if rng.chance(0.5) {
            [0.0; 3]
        } else {
            rng.point3(-1.0, 1.0).map(|c| c * scale * 1e-4)
        }
    };
    let mut ps = point_on(&curve, ta);
    let mut pe = point_on(&curve, tb);
    let (na, nb) = (noise(rng), noise(rng));
    for (p, n) in ps.iter_mut().zip(na).chain(pe.iter_mut().zip(nb)) {
        *p += n;
    }
    let mut vs = vertex(FilletVertexType::FilletXFillet, FilStatus::Processed);
    vs.point = ps;
    let mut ve = vertex(FilletVertexType::FilletXFillet, FilStatus::Processed);
    ve.point = pe;
    match rng.below(20) {
        0 => {
            // neither end processed: SM_ERR
            vs.status = 0;
            ve.status = 0;
        }
        1 => vs.status = 0, // the vertex's own intersection (unported)
        2 => ve.status = 0,
        _ => {}
    }
    let uv_curve = |rng: &mut Rng| {
        // the 3D curve's parameter range, or (rarely) another one
        if rng.chance(0.9) {
            gen_curve_on(rng, 2, 1.0, a, bb)
        } else {
            gen_curve(rng, 2, 1.0)
        }
    };
    let solver = |rng: &mut Rng| SolverIn {
        tol: *rng.choose(&[1e-7, 1e-6, 1e-4, 1e-2]).unwrap_or(&1e-6) * scale,
        ang: rng.uniform(0.001, 0.1),
        tang: 0.01,
        eus: [-1, -1],
        surfs: [None, None],
        extend: [false, false],
        setup: false,
    };
    let solvers = vec![solver(rng), solver(rng)];
    let mut has_curve = true;
    if rng.chance(0.05) {
        has_curve = false; // the surface/surface intersection makes it (unported)
    }
    let edge = EdgeIn {
        vs: 0,
        ve: 1,
        etype: FilletEdgeType::FilletXFillet as i64,
        status: 0,
        // without a curve upstream reads the corner's context (UB-200; the shim refuses that)
        corner: if !has_curve || rng.chance(0.9) { 0 } else { -1 },
        curve: has_curve.then(|| curve.clone()),
        interval: Some([a, bb]),
        eus: [
            EuIn {
                uv_curve: Some(uv_curve(rng)),
                geom: 0,
                ..EuIn::default()
            },
            EuIn {
                uv_curve: Some(uv_curve(rng)),
                geom: 1,
                ..EuIn::default()
            },
        ],
    };
    let geom = |rng: &mut Rng, s: i64, z: f64| GeomIn {
        gtype: 0,
        status: 0,
        solver: s,
        surface: if rng.chance(0.97) {
            Some(gen_bspline_surface(rng, scale, z))
        } else {
            None
        },
        rails: [-1, -1],
        other1: vec![],
        other2: vec![],
        sides: vec![],
    };
    let geoms = vec![geom(rng, 0, 0.0), geom(rng, 1, 0.5)];
    let op = match rng.below(3) {
        0 => OpIn::FilletIntFillet { e: 0 },
        _ => OpIn::CornerEdge { e: 0 },
    };
    Inputs {
        brep: BrepIn::Network,
        model: ModelIn {
            solvers,
            vertices: vec![vs, ve],
            geoms,
            edges: vec![edge],
        },
        op,
        expected_rejection: false,
    }
}

/// Fillet edges of other types for the edge dispatcher.
fn gen_corner_edge(rng: &mut Rng) -> Inputs {
    let etype = *rng
        .choose(&[
            FilletEdgeType::Rail,
            FilletEdgeType::Precomputed,
            FilletEdgeType::BlendingRail,
            FilletEdgeType::CliffRail,
            FilletEdgeType::SplitFace,
            FilletEdgeType::Unknown,
        ])
        .unwrap_or(&FilletEdgeType::Rail);
    let edge = EdgeIn {
        vs: 0,
        ve: 1,
        etype: etype as i64,
        status: 0,
        corner: if rng.chance(0.85) { 3 } else { -1 },
        curve: None,
        interval: None,
        eus: [EuIn::default(), EuIn::default()],
    };
    Inputs {
        brep: BrepIn::Network,
        model: ModelIn {
            vertices: vec![
                vertex(FilletVertexType::RailEnd, FilStatus::Processed),
                vertex(FilletVertexType::RailEnd, FilStatus::Processed),
            ],
            edges: vec![edge],
            ..ModelIn::default()
        },
        op: OpIn::CornerEdge { e: 0 },
        expected_rejection: false,
    }
}

/// A fillet geometry on a solver over a target face's edge, for `ReCalcFilletGeom`.
/// Whether `ReCalcFilletGeom`'s sort loop would sort a rail-end point of the generated
/// rails (rail `jj` is edge `jj`): a processed vertex that is not a mate or an
/// extended-edge-use end, with a set uv (`UVPos(jj)` for the 2D test; `UVPos(0)` of an
/// unknown point for the 3D test).
fn recalc_sorts_a_point(gtype: FilletGeomType, vertices: &[VertexIn], edges: &[EdgeIn]) -> bool {
    let b3d = matches!(
        gtype,
        FilletGeomType::CornerFillet | FilletGeomType::TangentRollover
    );
    let unset = |uv: Option<&[f64; 2]>| uv.is_none_or(|p| p.contains(&-BIG_DOUBLE));
    edges.iter().take(2).enumerate().any(|(jj, e)| {
        [e.vs, e.ve].iter().zip(&e.eus).any(|(&v, eu)| {
            let Some(v) = vertices.get(v) else {
                return false;
            };
            if v.status != FilStatus::Processed as i64
                || v.vtype == FilletVertexType::Mate as i64
                || v.vtype == FilletVertexType::RailXExtendedEdgeuse as i64
            {
                return false;
            }
            let t = &eu.tsect;
            if !b3d {
                return !unset(t.uv.get(jj));
            }
            t.ptype != IntersectionPointType::Unknown as i64 || !unset(t.uv.first())
        })
    })
}

fn gen_recalc(rng: &mut Rng) -> Inputs {
    let brep = solid_brep(rng);
    let base = |rng: &mut Rng| {
        let z = rng.uniform(-0.5, 0.5);
        gen_bspline_surface(rng, 1.0, z)
    };
    let solver = SolverIn {
        tol: *rng.choose(&[1e-6, 1e-4]).unwrap_or(&1e-6),
        ang: 0.05,
        tang: 0.01,
        eus: [rng.below(1000) as i64, rng.below(1000) as i64],
        surfs: [
            Some(OffIn {
                base: base(rng),
                dist: rng.uniform(-0.2, 0.2),
            }),
            Some(OffIn {
                base: base(rng),
                dist: rng.uniform(-0.2, 0.2),
            }),
        ],
        extend: [rng.chance(0.5), rng.chance(0.5)],
        setup: rng.chance(0.7),
    };
    let gtype = *rng
        .choose(&[
            FilletGeomType::TangentRollover,
            FilletGeomType::TangentRollover,
            FilletGeomType::Default,
            FilletGeomType::CornerFillet,
        ])
        .unwrap_or(&FilletGeomType::TangentRollover);
    let tsect = |rng: &mut Rng| {
        let ptype = if rng.chance(0.6) { 1 } else { 2 };
        let mut uv = [
            [rng.uniform(0.0, 1.0), rng.uniform(0.0, 1.0)],
            [rng.uniform(0.0, 1.0), rng.uniform(0.0, 1.0)],
        ];
        // sometimes "unset" (-SM_BIG_DOUBLE), which the sort loop skips
        if rng.chance(0.1) {
            let k = rng.below(4) as usize;
            if let Some(c) = uv.get_mut(k / 2).and_then(|p| p.get_mut(k % 2)) {
                *c = -BIG_DOUBLE;
            }
        }
        TsectIn {
            ptype,
            uv,
            uv_face: -1,
            crv: rng.point3(-1.0, 1.0),
            deriv: rng.point3(-1.0, 1.0),
            cparam: 0.0,
        }
    };
    let status = |rng: &mut Rng| {
        if rng.chance(0.85) {
            FilStatus::Processed
        } else {
            FilStatus::Unprocessed
        }
    };
    let mut vertices = Vec::new();
    for _ in 0..4 {
        let vt = *rng
            .choose(&[
                FilletVertexType::RailEnd,
                FilletVertexType::RailEnd,
                FilletVertexType::RailXEdgeuse,
                FilletVertexType::Mate,
            ])
            .unwrap_or(&FilletVertexType::RailEnd);
        vertices.push(vertex(vt, status(rng)));
    }
    let edges = vec![
        rail_edge(0, 1, 0, [tsect(rng), tsect(rng)]),
        rail_edge(2, 3, 0, [tsect(rng), tsect(rng)]),
    ];
    // rail 1 missing: rail 0 is processed, then NULL_POINTER
    let rails = if rng.chance(0.6) { [0, -1] } else { [0, 1] };
    let ref_curve = (gtype == FilletGeomType::CornerFillet && rng.chance(0.8))
        .then(|| gen_curve_on(rng, 3, 1.0, 0.0, 1.0));
    let mut dir = rng.chance(0.5).then(|| rng.point3(-1.0, 1.0));
    // Without a march direction, when rail 0 has no curve (never here) and both rails
    // are there, upstream reads the first sorted point even when there is none (UB-199;
    // the shim refuses that model): give such a model a direction.
    if dir.is_none() && rails[1] >= 0 && !recalc_sorts_a_point(gtype, &vertices, &edges) {
        dir = Some(rng.point3(-1.0, 1.0));
    }
    Inputs {
        brep,
        model: ModelIn {
            solvers: vec![solver],
            vertices,
            geoms: vec![GeomIn {
                gtype: gtype as i64,
                status: 0,
                solver: 0,
                surface: None,
                rails,
                other1: vec![],
                other2: vec![],
                sides: vec![],
            }],
            edges,
        },
        op: OpIn::ReCalc {
            g: 0,
            // the analytic branch is skipped for a roll-over (the shim refuses others)
            analytic: gtype == FilletGeomType::TangentRollover && rng.chance(0.3),
            dir,
            ref_curve,
        },
        expected_rejection: false,
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    match rng.below(20) {
        0..=6 => gen_mate(rng),
        7..=8 => gen_on_vertex(rng),
        9..=14 => gen_fif(rng),
        15 => gen_corner_edge(rng),
        _ => gen_recalc(rng),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "blend.fillet_geom_p3"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_case(rng)).ok()
    }

    fn cases(&self, ctx: &crate::case::CaseContext) -> Result<Vec<crate::Case>, HarnessError> {
        let ctx = crate::case::CaseContext {
            seeds: ctx.seeds.max(300),
            ..ctx.clone()
        };
        crate::runner::default_cases(self, &ctx)
    }

    fn run_cpp(&self, case: &crate::Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &crate::Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}
