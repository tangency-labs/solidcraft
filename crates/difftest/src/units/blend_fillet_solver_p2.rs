//! Runner for unit `blend.fillet_solver_p2`. C++: `solidcore_ffi::units::blend_fillet_solver_p2`.
//! Rust: `solidcore::blend::fillet_solver::p2`.
//!
//! The solves and the analytic fillet builders call the solver's virtuals
//! (`GetJacobianSize`, `LoadInitialValues`, `LoadJacobian`, `SetupOffsetValues`), which the
//! library's solvers inherit from units that are not ported. Both sides therefore run a
//! small rolling-ball solver: the shim's `P2FS` and its mirror [`TestSolver`] (offset
//! surface `rail` minus offset surface `1 - rail` as three equations in the two UV
//! points, plus `extra` linear unknowns), on the same offset surfaces of planes and
//! B-spline patches (blocks of unit `topology.edgeuse_p1`).
//!
//! Ops: `PointOnPlaneSolve` (`plane`: the rolling ball between a floor and a wall patch
//! cut by a plane across the edge, the plane adjusting on the second try; curved
//! patches; up to four user doubles), `RailEdgeuseIntersect` (`rail_eu`: the rail on a
//! face B-rep's surface meeting its side edges, either rail, missing offset surfaces
//! (`SM_ERR`), file B-reps; the side edge use's UV trim curve is built on demand),
//! `RailRailIntersect` (`rail_rail`: missing surfaces compare `SM_ERR`; with all four
//! surfaces the Rust side defers at `SmTemporaryRadiusChange`, unit
//! `blend.fillet_executive_p2`), the analytic builders `CalcCylinderFilletGeom`,
//! `CalcConeFilletGeom` and `CalcTorusFilletGeom` (`geom`: a circular fillet between a
//! floor and a wall patch along a straight wire edge, or between a floor and a tangent
//! wall along a rational circular arc edge (quarter to full circle); constant and
//! varying radius, extension before/after, swapped surfaces, a linear generator, the
//! wrong edge kind, jittered points; the C++ stores into a fresh `SmFilletGeom` with two
//! rails, the Rust side compares the built parts, [`FilletSolver::cylinder_fillet_geom_parts`]
//! etc.), `GetFilletCorner` above index 1 (`corner`) and `RecordFilletError`
//! (`record`: compares the recorded `SmFilletErrorInfo` fields across rail-0, WR-190
//! rail-1, dual-rail deduplication and empty-tsect modes).
//!
//! B-rep cases build the C++ B-rep, export it through unit `topology.edgeuse_p1` and
//! import the same records on the Rust side; entity selectors are resolved modulo the
//! export's counts.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::blend::fillet_custom_solver::{
    InitialValues, JacobianLoad, OffsetSurface, SurfaceId,
};
use solidcore::blend::fillet_executive::p3::ErrorInfo;
use solidcore::blend::fillet_geom::p1::{
    FilStatus, FilletBrep, FilletEdgeType, FilletEdgeuse, FilletErrorType, FilletGeom,
    FilletGeomId, FilletGeomType, FilletVertex, FilletVertexType, FilletVertexuse,
};
use solidcore::blend::fillet_solver::p1::{
    FilletExecutiveRef, FilletSolver, FilletSolverOps, FilletSurfaceGenerator,
    OffsetSurfaceSlot,
};
use solidcore::blend::fillet_solver::p2::SolverErrorObject;
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::curve::line::Line;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::{Extent1d, ExtentNd};
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::matrix::Matrix;
use solidcore::geom::tol::zone_tol3d;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::surface_intersector::p1::{
    IntersectionPointType, SrfIndex, TsectPnt, MAX_USER_DOUBLES,
};
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::nurbs::knots::Side;
use solidcore::nurbs::math::NOW;
use solidcore::surface::offset_surface::SS_UNKNOWN;
use solidcore::surface::surface::p1::evaluate_1st_derivatives;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p2::{make_edge_vertex_topology, make_shell_vertex_topology};
use solidcore::topology::{
    edge, vertex, Brep, EdgeId, Edgeuse, EdgeuseId, VertexId, Vertexuse, VertexuseOwner,
};
use solidcore::SolidError;
use solidcore_ffi::units::blend_fillet_solver_p2::{self as ffi, Op};
use solidcore_ffi::units::topology_edgeuse_p1 as ffi1;

use super::topology_edgeuse_p1::{
    brep_block_of, gen_surface, import, path_of, push_surface, sel, surface_of, BrepIn, BssIn,
    CrvIn, SrfIn, FILES,
};
use crate::{HarnessError, Output, Rng, UnitDiff};

/// The runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// The shim solver's parameters (`tol extra radius slope target gu gv`).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Cfg {
    tol: f64,
    extra: usize,
    radius: f64,
    slope: f64,
    target: f64,
    gu: f64,
    gv: f64,
}

/// Offset surface `i` of `base[i]` when `has[i]`, and the orientations.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Surfs {
    base: [SrfIn; 2],
    has: [bool; 2],
    off: [f64; 2],
    orient: [f64; 2],
}

/// A tsect point: both UV points and the curve parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Tsect {
    uv0: [f64; 2],
    uv1: [f64; 2],
    cparam: f64,
}

/// A filleted edge of a real analytic curve kind: an `SmLine` segment, or an `SmCircle`
/// about the z axis through `span` degrees (`CreateWireEdgeFromCurve`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum AnalyticEdge {
    Line { a: [f64; 3], b: [f64; 3] },
    Circle { radius: f64, span: f64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    Plane {
        cfg: Cfg,
        surfs: Surfs,
        orig: [f64; 3],
        normal: [f64; 3],
        dom1: [f64; 4],
        dom2: [f64; 4],
        uv1: [f64; 2],
        uv2: [f64; 2],
    },
    RailEu {
        cfg: Cfg,
        surfs: Surfs,
        brep: BrepIn,
        side: i64,
        frac: f64,
        rail: usize,
        ptype: u8,
    },
    RailRail {
        cfg: [Cfg; 2],
        surfs: [Surfs; 2],
        uv: [f64; 2],
        rail: usize,
        other: usize,
    },
    /// `which`: 0 cylinder, 1 cone, 2 torus; `fsg`: 0 linear, 1 circular. The filleted
    /// edge is `brep`'s, or `analytic` when given (then `brep` is ignored); `rails` types
    /// the rail end vertices `SM_FV_RAIL_X_EDGEUSE`, processed, at its points.
    Geom {
        which: u8,
        cfg: Cfg,
        surfs: Surfs,
        fsg: u8,
        before: bool,
        after: bool,
        factor: f64,
        brep: BrepIn,
        #[serde(default)]
        analytic: Option<AnalyticEdge>,
        #[serde(default)]
        rails: Option<[[f64; 3]; 4]>,
        eu: [i64; 2],
        tsects: Vec<Tsect>,
    },
    Corner {
        cfg: Cfg,
        index: u64,
    },
    Record {
        cfg: Cfg,
        brep: BrepIn,
        eu: i64,
    },
}

fn b(v: bool) -> f64 {
    if v {
        1.0
    } else {
        0.0
    }
}

fn push_cfg(o: &mut Vec<f64>, c: &Cfg) {
    o.extend([
        c.tol,
        c.extra as f64,
        c.radius,
        c.slope,
        c.target,
        c.gu,
        c.gv,
    ]);
}

fn push_surfs(o: &mut Vec<f64>, s: &Surfs) -> Result<(), HarnessError> {
    push_surface(o, &s.base[0])?;
    push_surface(o, &s.base[1])?;
    for (h, d) in s.has.iter().zip(&s.off) {
        o.push(b(*h));
        o.push(*d);
    }
    o.extend(s.orient);
    Ok(())
}

// ---- B-rep cases -------------------------------------------------------------------------

/// The case's B-rep: the C++ export's counts and the imported Rust B-rep.
struct BrepCase {
    path: String,
    block: Vec<f64>,
    counts: [usize; 10],
    brep: Brep,
}

/// The Rust side's case: the source read by the Rust reader or built by the Rust kernel
/// in the shim's traversal order ([`crate::own_reader::edgeuse_graph_brep`]), never
/// imported from the C++ export; `Err(output)` when the Rust build fails, as
/// [`brep_case`] reports the shim's failed build.
fn own_case(brep: &BrepIn) -> Result<Result<BrepCase, Output>, HarnessError> {
    let path = path_of(Some(brep))?;
    let block = brep_block_of(Some(brep), false, false)?;
    let brep = match crate::own_reader::edgeuse_graph_brep(&mut Kernel::default(), brep)? {
        Ok(b) => b,
        Err(e) => return Ok(Err(crate::own_builder::failed_output(&e))),
    };
    Ok(Ok(BrepCase {
        path,
        block,
        counts: crate::own_builder::counts(&brep),
        brep,
    }))
}

/// Builds the case's B-rep on both sides; `Err(output)` when it cannot be built.
fn brep_case(brep: &BrepIn) -> Result<Result<BrepCase, Output>, HarnessError> {
    let path = path_of(Some(brep))?;
    let block = brep_block_of(Some(brep), false, false)?;
    let ex = ffi1::eval(ffi1::Op::Export, &path, &block)
        .map_err(|e| harness(format!("c++ export: {e}")))?;
    let Some(counts) = ffi1::counts(&ex) else {
        let st = ex.get(1).copied().unwrap_or(0.0);
        return Ok(Err(Output::err(format!("brep_build_status_{st}"))));
    };
    let brep = import(&ex)?;
    Ok(Ok(BrepCase {
        path,
        block,
        counts,
        brep,
    }))
}

impl Inputs {
    fn brep(&self) -> Option<&BrepIn> {
        match self {
            Inputs::Geom {
                analytic: Some(_), ..
            } => None,
            Inputs::RailEu { brep, .. }
            | Inputs::Geom { brep, .. }
            | Inputs::Record { brep, .. } => Some(brep),
            _ => None,
        }
    }

    fn op(&self) -> Op {
        match self {
            Inputs::Plane { .. } => Op::Plane,
            Inputs::RailEu { .. } => Op::RailEu,
            Inputs::RailRail { .. } => Op::RailRail,
            Inputs::Geom { .. } => Op::Geom,
            Inputs::Corner { .. } | Inputs::Record { .. } => Op::Misc,
        }
    }
}

/// The edge uses of a wire edge (`CreateWireEdgeFromCurve`): the primary and its mate.
const WIRE_EDGEUSES: usize = 2;

/// The edge use counts of the case's export (`[n_eu]`).
fn n_eu(c: &BrepCase) -> usize {
    c.counts[6]
}

fn cpp_input(i: &Inputs, c: Option<&BrepCase>) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    let block = || -> Result<&BrepCase, HarnessError> { c.ok_or_else(|| harness("no B-rep")) };
    match i {
        Inputs::Plane {
            cfg,
            surfs,
            orig,
            normal,
            dom1,
            dom2,
            uv1,
            uv2,
        } => {
            push_cfg(&mut o, cfg);
            push_surfs(&mut o, surfs)?;
            o.extend(orig);
            o.extend(normal);
            o.extend(dom1);
            o.extend(dom2);
            o.extend(uv1);
            o.extend(uv2);
        }
        Inputs::RailEu {
            cfg,
            surfs,
            side,
            frac,
            rail,
            ptype,
            ..
        } => {
            let c = block()?;
            push_cfg(&mut o, cfg);
            push_surfs(&mut o, surfs)?;
            o.extend_from_slice(&c.block);
            o.extend([
                sel(*side, n_eu(c)) as f64,
                *frac,
                *rail as f64,
                f64::from(*ptype),
            ]);
        }
        Inputs::RailRail {
            cfg,
            surfs,
            uv,
            rail,
            other,
        } => {
            for (c, s) in cfg.iter().zip(surfs) {
                push_cfg(&mut o, c);
                push_surfs(&mut o, s)?;
            }
            o.extend(uv);
            o.extend([*rail as f64, *other as f64]);
        }
        Inputs::Geom {
            which,
            cfg,
            surfs,
            fsg,
            before,
            after,
            factor,
            eu,
            tsects,
            analytic,
            rails,
            ..
        } => {
            o.push(f64::from(*which));
            push_cfg(&mut o, cfg);
            push_surfs(&mut o, surfs)?;
            o.extend([f64::from(*fsg), b(*before), b(*after), *factor]);
            match rails {
                Some(r) => {
                    o.push(1.0);
                    o.extend(r.iter().flatten());
                }
                None => o.push(0.0),
            }
            let n_eus = match analytic {
                Some(AnalyticEdge::Line { a, b }) => {
                    o.push(1.0);
                    o.extend(a);
                    o.extend(b);
                    WIRE_EDGEUSES
                }
                Some(AnalyticEdge::Circle { radius, span }) => {
                    o.push(2.0);
                    o.extend([*radius, *span]);
                    WIRE_EDGEUSES
                }
                None => {
                    let c = block()?;
                    o.push(0.0);
                    o.extend_from_slice(&c.block);
                    n_eu(c)
                }
            };
            o.push(sel(eu[0], n_eus) as f64);
            o.push(sel(eu[1], n_eus) as f64);
            o.push(tsects.len() as f64);
            for t in tsects {
                o.extend(t.uv0);
                o.extend(t.uv1);
                o.push(t.cparam);
            }
        }
        Inputs::Corner { cfg, index } => {
            o.push(0.0);
            push_cfg(&mut o, cfg);
            o.push(*index as f64);
        }
        Inputs::Record { cfg, eu, .. } => {
            let c = block()?;
            o.push(1.0);
            push_cfg(&mut o, cfg);
            o.extend_from_slice(&c.block);
            o.push(sel(*eu, n_eu(c)) as f64);
        }
    }
    Ok(o)
}

/// Rejects (on both sides) the solver sizes the FFI refuses: `extra` above
/// [`ffi::MAX_EXTRA`] and more than [`ffi::MAX_TSECTS`] tsect points (the Rust test
/// solver would allocate intervals of that size).
fn check_sizes(i: &Inputs) -> Result<(), HarnessError> {
    let cfgs: Vec<&Cfg> = match i {
        Inputs::Plane { cfg, .. }
        | Inputs::RailEu { cfg, .. }
        | Inputs::Geom { cfg, .. }
        | Inputs::Corner { cfg, .. }
        | Inputs::Record { cfg, .. } => vec![cfg],
        Inputs::RailRail { cfg, .. } => cfg.iter().collect(),
    };
    if cfgs.iter().any(|c| c.extra > ffi::MAX_EXTRA) {
        return Err(harness("extra above MAX_EXTRA"));
    }
    if let Inputs::Geom { tsects, .. } = i {
        if tsects.len() > ffi::MAX_TSECTS {
            return Err(harness("more tsect points than MAX_TSECTS"));
        }
    }
    Ok(())
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    check_sizes(i)?;
    let case = match i.brep() {
        Some(brep) => match brep_case(brep)? {
            Ok(c) => Some(c),
            Err(out) => return Ok(out),
        },
        None => None,
    };
    let path = case.as_ref().map(|c| c.path.clone()).unwrap_or_default();
    let input = cpp_input(i, case.as_ref())?;
    let flat = ffi::eval(i.op(), &path, &input).map_err(|e| harness(e.to_string()))?;
    Ok(Output::new().values("flat", flat))
}

// ---- Rust side ------------------------------------------------------------------------

fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

/// The shim's `P2FS` (see its header): `GetJacobianSize` `4 + extra`; `SetupOffsetValues`
/// sets every present offset surface to `(radius + slope * curve_parameter) *
/// orientation`; `LoadInitialValues` appends the other surface's UV guess (the first
/// surface's plus `(gu, gv)`, extents its base domain) and the `extra` unknowns;
/// `LoadJacobian` loads `S_rail(uv1) - S_other(uv2)` and `x[k] - (target + i)`.
struct TestSolver {
    base: FilletSolver,
    extra: usize,
    radius: f64,
    slope: f64,
    target: f64,
    gu: f64,
    gv: f64,
}

fn slot_mut(s: &mut FilletSolver, rail: usize) -> Result<&mut OffsetSurfaceSlot, SolidError> {
    s.surfaces
        .get_mut(rail)
        .and_then(Option::as_mut)
        .ok_or_else(|| SolidError::invalid_input("difftest::TestSolver", "no offset surface"))
}

fn set_ext(e: &mut ExtentNd, i: usize, v: Extent1d) -> Result<(), SolidError> {
    let slot = e
        .get_mut(i)
        .ok_or_else(|| SolidError::invalid_input("difftest::TestSolver", "extent index"))?;
    *slot = v;
    Ok(())
}

fn at(x: &[f64], i: usize) -> Result<f64, SolidError> {
    x.get(i)
        .copied()
        .ok_or_else(|| SolidError::invalid_input("difftest::TestSolver", "index"))
}

impl FilletSolverOps for TestSolver {
    fn solver(&self) -> &FilletSolver {
        &self.base
    }

    fn solver_mut(&mut self) -> &mut FilletSolver {
        &mut self.base
    }

    fn jacobian_size(&self) -> Result<usize, SolidError> {
        Ok(4 + self.extra)
    }

    fn setup_offset_values(
        &mut self,
        tsect: &TsectPnt,
        offset_dist: Option<&mut f64>,
    ) -> Result<(), SolidError> {
        let r = self.radius + self.slope * tsect.curve_parameter;
        for i in 0..2 {
            let o = self.base.orientation(i)?;
            if let Some(slot) = self.base.surfaces.get_mut(i).and_then(Option::as_mut) {
                slot.surface.set_offset_distance(r * o);
            }
        }
        if let Some(d) = offset_dist {
            *d = r;
        }
        Ok(())
    }

    fn load_initial_values(
        &mut self,
        _k: &Kernel,
        _brep: Option<&mut Brep>,
        do_surf2_calcs: bool,
        rail_index: usize,
        _surface1: &mut OffsetSurface,
        surface1_index: usize,
        intervals: &mut ExtentNd,
        periodicities: &mut Vec<bool>,
        guess_t: &mut Vec<f64>,
    ) -> Result<Option<InitialValues>, SolidError> {
        let mut out = None;
        if do_surf2_calcs {
            let other = 1 - rail_index.min(1);
            let dom = slot_mut(&mut self.base, other)?
                .surface
                .base_surface()
                // (runner helper without a kernel in scope)
                .natural_uv_domain_if_built();
            let idx2 = guess_t.len();
            set_ext(
                intervals,
                idx2,
                Extent1d::from_min_max(dom.min().x, dom.max().x),
            )?;
            set_ext(
                intervals,
                idx2 + 1,
                Extent1d::from_min_max(dom.min().y, dom.max().y),
            )?;
            periodicities.push(false);
            periodicities.push(false);
            let u = at(guess_t, surface1_index)? + self.gu;
            let v = at(guess_t, surface1_index + 1)? + self.gv;
            guess_t.push(u);
            guess_t.push(v);
            out = Some(InitialValues {
                surface2_rail: other,
                surface2_index: idx2,
            });
        }
        for i in 0..self.extra {
            set_ext(
                intervals,
                guess_t.len(),
                Extent1d::from_min_max(-100.0, 100.0),
            )?;
            periodicities.push(false);
            guess_t.push(0.5 * i as f64);
        }
        Ok(out)
    }

    fn load_jacobian(
        &mut self,
        k: &Kernel,
        num_equations: &mut usize,
        num_parameters: &mut usize,
        load: &mut JacobianLoad<'_>,
        f: &mut [f64],
        mut jacobian: Option<&mut Matrix>,
    ) -> Result<bool, SolidError> {
        let rail = load.rail_index.min(1);
        let other = 1 - rail;
        load.surface2 = Some(slot_mut(&mut self.base, other)?.surface.base_id);
        load.surf2_offset = *num_parameters;
        let (o1, o2) = (load.surf1_offset, load.surf2_offset);
        let x = load.x;
        let uv_a = Point2::new(at(x, o1)?, at(x, o1 + 1)?);
        let uv_b = Point2::new(at(x, o2)?, at(x, o2 + 1)?);
        let (pa, dua, dva) = slot_mut(&mut self.base, rail)?
            .surface
            .surface
            .with_evaluator(|e| evaluate_1st_derivatives(k, e, uv_a, Side::Left, Side::Left))?;
        let (pb, dub, dvb) = slot_mut(&mut self.base, other)?
            .surface
            .surface
            .with_evaluator(|e| evaluate_1st_derivatives(k, e, uv_b, Side::Left, Side::Left))?;
        let mut scale = pa.x.abs();
        if pa.y.abs() > scale {
            scale = pa.y.abs();
        }
        if pa.z.abs() > scale {
            scale = pa.z.abs();
        }
        let tol = self.base.conversion_tol * (1.0 + scale);
        let mut found = true;
        // per row: [S_a, dS_a/du, dS_a/dv, S_b, dS_b/du, dS_b/dv] components
        let rows = [
            [pa.x, dua.x, dva.x, pb.x, dub.x, dvb.x],
            [pa.y, dua.y, dva.y, pb.y, dub.y, dvb.y],
            [pa.z, dua.z, dva.z, pb.z, dub.z, dvb.z],
        ];
        let ne = *num_equations;
        for (c, [sa, ua, wa, sb, ub, wb]) in rows.into_iter().enumerate() {
            let v = sa - sb;
            *f.get_mut(ne + c)
                .ok_or_else(|| SolidError::invalid_input("difftest::TestSolver", "rF"))? = v;
            if v.abs() >= tol {
                found = false;
            }
            if let Some(j) = jacobian.as_deref_mut() {
                j.set_at(ne + c, o1, ua)?;
                j.set_at(ne + c, o1 + 1, wa)?;
                j.set_at(ne + c, o2, -ub)?;
                j.set_at(ne + c, o2 + 1, -wb)?;
            }
        }
        *num_equations += 3;
        *num_parameters += 2;
        for i in 0..self.extra {
            let (ne, np) = (*num_equations, *num_parameters);
            let v = at(x, np)? - (self.target + i as f64);
            *f.get_mut(ne)
                .ok_or_else(|| SolidError::invalid_input("difftest::TestSolver", "rF"))? = v;
            if v.abs() >= tol {
                found = false;
            }
            if let Some(j) = jacobian.as_deref_mut() {
                j.set_at(ne, np, 1.0)?;
            }
            *num_equations += 1;
            *num_parameters += 1;
        }
        Ok(found)
    }
}

/// The solver of `cfg` with the offset surfaces of `s` (distinct bases are distinct
/// objects).
fn solver_of(cfg: &Cfg, s: &Surfs) -> Result<TestSolver, HarnessError> {
    let k = Kernel::default();
    let err = |e: SolidError| harness(e.to_string());
    let mut fs = FilletSolver::new(cfg.tol, 0.1, 0.01);
    for (i, ((has, off), base)) in s.has.iter().zip(&s.off).zip(&s.base).enumerate() {
        if *has {
            let base = surface_of(base)?;
            let surface = OffsetSurface::new(&k, *off, base, SurfaceId::Free(i as u32), SS_UNKNOWN)
                .map_err(err)?;
            fs.set_surface(
                i,
                Some(OffsetSurfaceSlot {
                    key: i as u64,
                    surface,
                }),
            )
            .map_err(err)?;
        }
    }
    fs.orientations = s.orient;
    Ok(TestSolver {
        base: fs,
        extra: cfg.extra,
        radius: cfg.radius,
        slope: cfg.slope,
        target: cfg.target,
        gu: cfg.gu,
        gv: cfg.gv,
    })
}

fn put_offsets(o: &mut Vec<f64>, fs: &TestSolver) {
    for i in 0..2 {
        o.push(
            fs.base
                .surfaces
                .get(i)
                .and_then(Option::as_ref)
                .map_or(0.0, |s| s.surface.offset_distance()),
        );
    }
}

fn put_tsect(o: &mut Vec<f64>, t: &TsectPnt, n_user: usize) {
    for s in [SrfIndex::First, SrfIndex::Second] {
        let uv = t.uv_pos(s);
        o.extend([uv.x, uv.y]);
    }
    o.extend(t.user_doubles.iter().take(n_user.min(MAX_USER_DOUBLES)));
}

/// `put_crv` of the shim: `1 t0 t1 P(t0) P(mid) P(t1)`, `-1` for none.
fn put_crv(k: &Kernel, o: &mut Vec<f64>, c: Option<&Curve>) {
    let Some(c) = c else {
        o.push(-1.0);
        return;
    };
    o.push(1.0);
    let ivl = c.natural_interval();
    o.extend([ivl.min(), ivl.max()]);
    for t in [ivl.min(), ivl.mid(), ivl.max()] {
        let p = c.evaluate_point(k, t).unwrap_or(Vec3::UNDEF);
        o.extend([p.x, p.y, p.z]);
    }
}

/// `put_srf` of the shim: `1 u0 v0 u1 v1` and the 3 x 3 grid of points.
fn put_srf(k: &Kernel, o: &mut Vec<f64>, s: &Surface) {
    o.push(1.0);
    // (runner helper that does not return Result)
    let d = s.natural_uv_domain_if_built();
    o.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
    for a in [0.0, 0.5, 1.0] {
        for bb in [0.0, 0.5, 1.0] {
            let p = s
                .evaluate_point(k, d.evaluate(a, bb))
                .unwrap_or(Vec3::UNDEF);
            o.extend([p.x, p.y, p.z]);
        }
    }
}

/// The shim's output after a `Calc*FilletGeom`: the fillet geometry `g`'s surface, its
/// rails' curves (and, for the cone and torus, their intervals) and its centre line.
fn put_stored(
    k: &Kernel,
    o: &mut Vec<f64>,
    which: u8,
    fb: &FilletBrep,
    g: FilletGeomId,
) -> Result<(), SolidError> {
    let geom = fb.geom(g)?;
    let surface = geom
        .fillet_surface(None)?
        .ok_or_else(|| SolidError::invalid_input("difftest", "no fillet surface"))?;
    put_srf(k, o, surface);
    let rails = [
        geom.rail(0)
            .ok_or_else(|| SolidError::invalid_input("difftest", "no rail"))?,
        geom.rail(1)
            .ok_or_else(|| SolidError::invalid_input("difftest", "no rail"))?,
    ];
    for rail in rails {
        put_crv(k, o, edge::curve(fb.brep(), rail)?);
    }
    if which != 0 {
        for rail in rails {
            let ivl = edge::interval(fb.brep(), rail)?;
            o.extend([ivl.min(), ivl.max()]);
        }
    }
    put_crv(
        k,
        o,
        geom.center_line_curve().cloned().map(Curve::from).as_ref(),
    );
    Ok(())
}

/// Two edge uses (primary `SM_OT_SAME` at `vs`, mate `SM_OT_OPPOSITE` at `ve`) on `e`
/// with vertex uses, as `CreateWireEdgeFromCurve` / `SmFilletGeom::MakeRailEdge` link
/// them.
fn link_uses(
    b: &mut Brep,
    e: EdgeId,
    vs: VertexId,
    ve: VertexId,
) -> Result<[(EdgeuseId, solidcore::topology::VertexuseId); 2], HarnessError> {
    let h = |e: SolidError| harness(e.to_string());
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
    Ok([(eu1, vu1), (eu2, vu2)])
}

/// A wire edge of the analytic curve of `spec`, built as the shim's
/// `SmBrep::CreateWireEdgeFromCurve(curve, curve->GetNaturalInterval())` on a fresh
/// `SmBrep()` builds it: `MakeShellVertex` at the start point (a vertex shell of the
/// infinite region, the B-rep's tolerance), `MakeWireEdgeVertex` (`MakeEdgeVertexTopology`:
/// the vertex shell becomes the wire shell of the two edge uses and a new vertex takes the
/// end point; the edge's then the vertex's `SetTolerance(GetTolerance())`, the interval,
/// `SetOrientation(SM_OT_SAME)` (the first edge use `SM_OT_SAME`, its mate
/// `SM_OT_OPPOSITE`), the curve, `RefreshTolerance(FALSE)` of both vertices), then
/// `GlueVertices(start, end)` when the ends are closer than the B-rep's tolerance (a
/// closed curve): the end vertex's use moves to the start vertex, whose tolerance grows
/// to the distance plus the end vertex's. The Rust arena keeps the glued-away end
/// vertex's record (B-rep records cannot be erased yet); nothing references it.
fn analytic_brep(k: &Kernel, spec: &AnalyticEdge) -> Result<Brep, HarnessError> {
    let h = |e: SolidError| harness(e.to_string());
    let curve = match spec {
        AnalyticEdge::Line { a, b } => Curve::from(
            Line::create_line_segment(3, Vec3::from_array(*a), Vec3::from_array(*b), None)
                .map_err(h)?,
        ),
        AnalyticEdge::Circle { radius, span } => Curve::from(
            Ellipse::create_canonical_circle(
                k,
                &Axis2Placement::default(),
                *radius,
                Some(&Extent1d::from_min_max(0.0, *span)),
                None,
            )
            .map_err(h)?,
        ),
    };
    let ivl = curve.natural_interval();
    let ps = curve.evaluate_point(k, ivl.min()).map_err(h)?;
    let pe = curve.evaluate_point(k, ivl.max()).map_err(h)?;
    let mut b = Brep::new_with_infinite_region().map_err(h)?;
    let tol = b.tolerance();

    // MakeShellVertex(NULL, sStartPt, ..)
    let (_, vs) = make_shell_vertex_topology(&mut b, None).map_err(h)?;
    b.vertex_mut(vs).map_err(h)?.point = ps;
    vertex::set_tolerance(&mut b, vs, tol, true, true).map_err(h)?;

    // MakeWireEdgeVertex(NULL, pNewV1, pCurve, crInterval, SM_OT_SAME, sEndPt, ..)
    let (e, ve) = make_edge_vertex_topology(&mut b, None, vs).map_err(h)?;
    b.vertex_mut(ve).map_err(h)?.point = pe;
    edge::set_tolerance(&mut b, e, tol, true, true).map_err(h)?;
    vertex::set_tolerance(&mut b, ve, tol, true, true).map_err(h)?;
    b.edge_mut(e).map_err(h)?.interval = ivl;
    let eus = b.edge(e).map_err(h)?.edgeuses.clone();
    let [eu1, eu2] = eus.as_slice() else {
        return Err(harness("wire edge without two edge uses"));
    };
    b.edgeuse_mut(*eu1).map_err(h)?.orientation = OrientType::Same;
    b.edgeuse_mut(*eu2).map_err(h)?.orientation = OrientType::Opposite;
    b.edge_mut(e).map_err(h)?.curve = Some(curve);
    vertex::refresh_tolerance(&mut b, vs, false).map_err(h)?;
    vertex::refresh_tolerance(&mut b, ve, false).map_err(h)?;

    // glue vertices for short edges (GlueVertices(pNewV1, pNewV2))
    let dist = ps.distance(pe);
    if dist < tol {
        let radius = dist + b.vertex(ve).map_err(h)?.tolerance;
        if radius > b.vertex(vs).map_err(h)?.tolerance {
            let radius = zone_tol3d(k, radius);
            vertex::set_tolerance(&mut b, vs, radius, true, true).map_err(h)?;
        }
        let vus = std::mem::take(&mut b.vertex_mut(ve).map_err(h)?.vertexuses);
        for vu in vus {
            b.vertexuse_mut(vu).map_err(h)?.vertex = Some(vs);
            b.vertex_mut(vs).map_err(h)?.vertexuses.push(vu);
        }
        b.edge_mut(e).map_err(h)?.vertices = [vs, vs];
    }
    Ok(b)
}

/// The shim's pseudo B-rep: a fresh `SmFilletGeom` (type default; the copies of the
/// solver's offset surfaces it makes are not read by the builders) with two rails made as
/// `MakeRailEdge` does (a rail edge of fillet vertices of unknown type, unprocessed, each
/// rail end with an edge use and a vertex use); `rails` types the rail ends
/// `SM_FV_RAIL_X_EDGEUSE`, processed, at its points. The geometry is geometry 0.
fn fillet_geom_model(
    k: &Kernel,
    rails: Option<&[[f64; 3]; 4]>,
) -> Result<(FilletBrep, FilletGeomId), HarnessError> {
    let h = |e: SolidError| harness(e.to_string());
    let mut fb = FilletBrep::new();
    let geom = FilletGeom::new(k, None, FilletGeomType::Default).map_err(h)?;
    let g = fb.add_geom(geom);
    for i in 0..2 {
        let mut ends = [None, None];
        for slot in &mut ends {
            let v = fb.brep_mut().add_vertex(Vec3::UNDEF).map_err(h)?;
            fb.insert_fillet_vertex(v, FilletVertex::new(k, None));
            *slot = Some(v);
        }
        let [Some(vs), Some(ve)] = ends else {
            return Err(harness("rail vertices"));
        };
        let e = fb.new_fillet_edge(vs, ve, None).map_err(h)?;
        fb.fillet_edge_mut(e)
            .map_err(h)?
            .set_fillet_edge_type(FilletEdgeType::Rail);
        for (eu, vu) in link_uses(fb.brep_mut(), e, vs, ve)? {
            fb.insert_fillet_edgeuse(eu, FilletEdgeuse::default());
            fb.insert_fillet_vertexuse(vu, FilletVertexuse::new());
        }
        fb.geom_mut(g).map_err(h)?.set_rail(i, Some(e));
        if let Some(pts) = rails {
            let ends = pts
                .get(2 * i..2 * i + 2)
                .ok_or_else(|| harness("rail point index"))?;
            for (v, p) in [vs, ve].into_iter().zip(ends) {
                let fv = fb.fillet_vertex_mut(v).map_err(h)?;
                fv.set_fillet_vertex_type(FilletVertexType::RailXEdgeuse);
                fv.set_status(FilStatus::Processed);
                fb.brep_mut().vertex_mut(v).map_err(h)?.point = Vec3::from_array(*p);
            }
        }
    }
    Ok((fb, g))
}

fn eu_id(i: i64) -> Option<EdgeuseId> {
    u32::try_from(i).ok().map(EdgeuseId::from_index)
}

fn point_type(code: u8) -> IntersectionPointType {
    match code {
        0 => IntersectionPointType::Undefined,
        2 => IntersectionPointType::Crossing,
        3 => IntersectionPointType::TangentPoint,
        4 => IntersectionPointType::TangentCurve,
        5 => IntersectionPointType::Singularity,
        6 => IntersectionPointType::Coincidence,
        7 => IntersectionPointType::Touching,
        _ => IntersectionPointType::Unknown,
    }
}

fn point_type_code(t: IntersectionPointType) -> f64 {
    match t {
        IntersectionPointType::Undefined => 0.0,
        IntersectionPointType::Unknown => 1.0,
        IntersectionPointType::Crossing => 2.0,
        IntersectionPointType::TangentPoint => 3.0,
        IntersectionPointType::TangentCurve => 4.0,
        IntersectionPointType::Singularity => 5.0,
        IntersectionPointType::Coincidence => 6.0,
        IntersectionPointType::Touching => 7.0,
    }
}

fn dom2(d: &[f64; 4]) -> Extent2d {
    Extent2d::from_points(Point2::new(d[0], d[1]), Point2::new(d[2], d[3]))
}

/// Runs the case; `Err` is a status or a deferral (after the values pushed so far).
fn rust_run(i: &Inputs, o: &mut Vec<f64>) -> Result<Result<(), SolidError>, HarnessError> {
    let k = Kernel::default();
    Ok(match i {
        Inputs::Plane {
            cfg,
            surfs,
            orig,
            normal,
            dom1,
            dom2: d2,
            uv1,
            uv2,
        } => {
            let mut fs = solver_of(cfg, surfs)?;
            let mut t = TsectPnt::default();
            let r = FilletSolver::point_on_plane_solve(
                &mut fs,
                &k,
                None,
                Vec3::from_array(*orig),
                Vec3::from_array(*normal),
                &dom2(dom1),
                &dom2(d2),
                Point2::new(uv1[0], uv1[1]),
                Point2::new(uv2[0], uv2[1]),
                &mut t,
            );
            match r {
                Ok(found) => {
                    o.push(0.0);
                    put_offsets(o, &fs);
                    o.push(b(found));
                    if found {
                        put_tsect(o, &t, cfg.extra);
                    }
                    Ok(())
                }
                Err(e) => {
                    if let Some(_d) = Output::unported(&e) {
                        return Ok(Err(e));
                    }
                    o.push(status(&e));
                    put_offsets(o, &fs);
                    Ok(())
                }
            }
        }
        Inputs::RailEu {
            cfg,
            surfs,
            brep,
            side,
            frac,
            rail,
            ptype,
        } => {
            let Ok(mut c) = own_case(brep)? else {
                return Err(harness("B-rep failed on the Rust side only"));
            };
            o.push(0.0);
            let side = eu_id(sel(*side, n_eu(&c))).ok_or_else(|| harness("side edge use"))?;
            let mut fs = solver_of(cfg, surfs)?;
            let mut t = TsectPnt {
                point_type: point_type(*ptype),
                ..TsectPnt::default()
            };
            let e = solidcore::topology::edgeuse::p1::edge(&c.brep, side)
                .map_err(|e| harness(e.to_string()))?
                .ok_or_else(|| harness("side edge use without edge"))?;
            let ivl = c.brep.edge(e).map_err(|e| harness(e.to_string()))?.interval;
            let guess = ivl.min() + (ivl.max() - ivl.min()) * *frac;
            let r = FilletSolver::rail_edgeuse_intersect(
                &mut fs,
                &k,
                &mut c.brep,
                *rail,
                side,
                guess,
                &mut t,
            );
            match r {
                Ok(param) => {
                    o.push(0.0);
                    put_offsets(o, &fs);
                    o.push(b(param.is_some()));
                    if let Some(p) = param {
                        o.push(p);
                        put_tsect(o, &t, cfg.extra);
                        o.push(t.curve_parameter);
                        o.push(point_type_code(t.point_type));
                    }
                    Ok(())
                }
                Err(e) => {
                    if Output::unported(&e).is_some() {
                        return Ok(Err(e));
                    }
                    o.push(status(&e));
                    put_offsets(o, &fs);
                    Ok(())
                }
            }
        }
        Inputs::RailRail {
            cfg,
            surfs,
            uv,
            rail,
            other,
        } => {
            let mut fa = solver_of(&cfg[0], &surfs[0])?;
            let mut fb = solver_of(&cfg[1], &surfs[1])?;
            let mut ta = TsectPnt::default();
            let mut tb = TsectPnt::default();
            let r = FilletSolver::rail_rail_intersect(
                &mut fa,
                &mut fb,
                &k,
                None,
                Point2::new(uv[0], uv[1]),
                *rail,
                *other,
                &mut ta,
                &mut tb,
            );
            match r {
                Ok(found) => {
                    o.push(0.0);
                    put_offsets(o, &fa);
                    put_offsets(o, &fb);
                    o.push(b(found));
                    if found {
                        put_tsect(o, &ta, cfg[0].extra);
                        put_tsect(o, &tb, cfg[1].extra);
                    }
                    Ok(())
                }
                Err(e) => {
                    if Output::unported(&e).is_some() {
                        return Ok(Err(e));
                    }
                    o.push(status(&e));
                    put_offsets(o, &fa);
                    put_offsets(o, &fb);
                    Ok(())
                }
            }
        }
        Inputs::Geom {
            which,
            cfg,
            surfs,
            fsg,
            before,
            after,
            factor,
            brep,
            analytic,
            rails,
            eu,
            tsects,
        } => {
            let (target, n_eus) = if let Some(a) = analytic {
                (analytic_brep(&k, a)?, WIRE_EDGEUSES)
            } else {
                let Ok(c) = own_case(brep)? else {
                    return Err(harness("B-rep failed on the Rust side only"));
                };
                let n = n_eu(&c);
                (c.brep, n)
            };
            o.push(0.0);
            let mut fs = solver_of(cfg, surfs)?;
            fs.base.set_extension_factor(*factor);
            fs.base.extend_before = *before;
            fs.base.extend_after = *after;
            fs.base.fsg = Some(if *fsg == 0 {
                FilletSurfaceGenerator::Linear
            } else {
                FilletSurfaceGenerator::circular_default()
            });
            fs.base.edgeuses = [eu_id(sel(eu[0], n_eus)), eu_id(sel(eu[1], n_eus))];
            let pts: Vec<TsectPnt> = tsects
                .iter()
                .map(|t| {
                    let mut p = TsectPnt::default();
                    *p.uv_pos_mut(SrfIndex::First) = Point2::new(t.uv0[0], t.uv0[1]);
                    *p.uv_pos_mut(SrfIndex::Second) = Point2::new(t.uv1[0], t.uv1[1]);
                    p.curve_parameter = t.cparam;
                    p
                })
                .collect();
            // a fresh `SmFilletGeom` of the solver with two rails in a fresh pseudo B-rep
            let h = |e: SolidError| harness(e.to_string());
            let (mut fb, g) = fillet_geom_model(&k, rails.as_ref())?;
            let fg = Some(g);
            let r = match which {
                0 => {
                    FilletSolver::calc_cylinder_fillet_geom(&mut fs, &k, &target, &mut fb, &pts, fg)
                }
                1 => FilletSolver::calc_cone_fillet_geom(&mut fs, &k, &target, &mut fb, &pts, fg),
                _ => FilletSolver::calc_torus_fillet_geom(&mut fs, &k, &target, &mut fb, &pts, fg),
            };
            match r {
                Ok(()) => {
                    o.push(0.0);
                    put_stored(&k, o, *which, &fb, g).map_err(h)?;
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
        Inputs::Corner { cfg, index } => {
            let fs = solver_of(
                cfg,
                &Surfs {
                    base: [plane_z(), plane_z()],
                    has: [false, false],
                    off: [0.0, 0.0],
                    orient: [1.0, 1.0],
                },
            )?;
            let empty = Brep::new();
            let index = usize::try_from(*index).map_err(|_| harness("index"))?;
            match fs.base.fillet_corner(&empty, index, &solidcore::blend::fillet_geom::p3::NoCorners) {
                Ok(c) => {
                    o.push(b(c.is_none()));
                    o.push(fs.jacobian_size().unwrap_or(0) as f64);
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
        Inputs::Record { cfg, brep, eu } => {
            let Ok(c) = own_case(brep)? else {
                return Err(harness("B-rep failed on the Rust side only"));
            };
            o.push(0.0);
            let h = |e: SolidError| harness(e.to_string());
            let mut fs = solver_of(
                cfg,
                &Surfs {
                    base: [plane_z(), plane_z()],
                    has: [false, false],
                    off: [0.0, 0.0],
                    orient: [1.0, 1.0],
                },
            )?;
            let eu0_idx = sel(*eu, n_eu(&c));
            let eu0 = eu_id(eu0_idx).ok_or_else(|| harness("record edge use"))?;
            let filleted = solidcore::topology::edgeuse::p1::edge(&c.brep, eu0).map_err(h)?;
            fs.base.edgeuses = [Some(eu0), None];
            fs.base.status = if eu0_idx % 2 == 0 {
                FilStatus::SurfTracingFailure
            } else {
                FilStatus::Unprocessed
            };
            let err_code = if eu0_idx % 3 == 0 {
                FilletErrorType::EdgeProblem
            } else {
                FilletErrorType::BadInt
            };
            fs.base.executive = Some(FilletExecutiveRef);

            let (mut fb, g) = fillet_geom_model(&k, None)?;
            fs.base.fillet_geoms = vec![g];
            let rail0 = fb
                .geom(g)
                .map_err(h)?
                .rail(0)
                .ok_or_else(|| harness("rail 0"))?;
            let rail1 = fb
                .geom(g)
                .map_err(h)?
                .rail(1)
                .ok_or_else(|| harness("rail 1"))?;

            let mut pts = Vec::new();
            if cfg.target < -1.5 {
                let r0 = Curve::from(
                    solidcore::nurbs::b_spline_curve::BSplineCurve::create_line_segment(
                        3,
                        Vec3::ZERO,
                        Vec3::new(1.0, 0.0, 0.0),
                    )
                    .map_err(h)?,
                );
                fb.brep_mut().edge_mut(rail0).map_err(h)?.curve = Some(r0);
                let mut t = TsectPnt::default();
                *t.crv_pos_mut() = Vec3::new(cfg.target, cfg.radius, fs.base.approx_tol_3d);
                pts.push(t);
            } else if cfg.target < 0.0 {
                let r1 = Curve::from(
                    solidcore::nurbs::b_spline_curve::BSplineCurve::create_line_segment(
                        3,
                        Vec3::ZERO,
                        Vec3::new(0.0, 1.0, 0.0),
                    )
                    .map_err(h)?,
                );
                fb.brep_mut().edge_mut(rail1).map_err(h)?.curve = Some(r1);
                let mut plane = solidcore::surface::plane::Plane::from_normal(
                    Vec3::ZERO,
                    Vec3::new(0.0, 0.0, 1.0),
                );
                plane.make_nurb().map_err(h)?;
                fb.geom_mut(g)
                    .map_err(h)?
                    .set_fillet_surface(Some(Surface::from(plane)));
                let cl = solidcore::nurbs::b_spline_curve::BSplineCurve::create_line_segment(
                    3,
                    Vec3::ZERO,
                    Vec3::new(2.0, 0.0, 0.0),
                )
                .map_err(h)?;
                fb.geom_mut(g).map_err(h)?.center_line_curve = Some(Curve::from(cl));
                let mut t = TsectPnt::default();
                *t.crv_pos_mut() = Vec3::new(cfg.target, -cfg.radius, fs.base.approx_tol_3d);
                pts.push(t);
            } else if cfg.target < 1.5 {
                let r0 = Curve::from(
                    solidcore::nurbs::b_spline_curve::BSplineCurve::create_line_segment(
                        3,
                        Vec3::ZERO,
                        Vec3::new(1.0, 0.0, 0.0),
                    )
                    .map_err(h)?,
                );
                let r1 = Curve::from(
                    solidcore::nurbs::b_spline_curve::BSplineCurve::create_line_segment(
                        3,
                        Vec3::ZERO,
                        Vec3::new(0.0, 1.0, 0.0),
                    )
                    .map_err(h)?,
                );
                fb.brep_mut().edge_mut(rail0).map_err(h)?.curve = Some(r0);
                fb.brep_mut().edge_mut(rail1).map_err(h)?.curve = Some(r1);
                pts.push(TsectPnt::default());
            }

            let mut ei = ErrorInfo::default();
            match fs.base.record_fillet_error_in(
                &c.brep,
                &fb,
                err_code,
                &pts,
                Some("difftest"),
                Some(&mut ei),
            ) {
                Ok(()) => {
                    o.push(f64::from(ei.code));
                    o.push(f64::from(ei.status));
                    o.push(f64::from(ei.edge_type));
                    o.push(b(ei.edge == filleted.map(SolverErrorObject::FilletedEdge)));
                    o.push(ei.surfaces.len() as f64);
                    o.push(b(
                        ei.surfaces.as_slice() == [Some(SolverErrorObject::FilletSurface(g))]
                    ));
                    o.push(ei.curves.len() as f64);
                    o.push(b(
                        ei.curves.as_slice() == [Some(SolverErrorObject::CenterLineCurve(g))]
                    ));
                    o.push(ei.edges.len() as f64);
                    o.push(b(
                        ei.edges.as_slice() == [Some(SolverErrorObject::FilletEdge(rail0))]
                    ));
                    let loc = ei.location.unwrap_or(Vec3::UNDEF);
                    o.push(b(loc.is_initialized()));
                    if loc.is_initialized() {
                        o.extend([loc.x, loc.y, loc.z]);
                    }
                    o.push(ei.comment.len() as f64);
                    for byte in ei.comment.bytes() {
                        o.push(f64::from(byte));
                    }
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
    })
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    check_sizes(i)?;
    // the C++ side reports a B-rep that cannot be built
    if let Some(brep) = i.brep() {
        if let Err(out) = own_case(brep)? {
            return Ok(out);
        }
    }
    let mut o = Vec::new();
    Ok(match rust_run(i, &mut o)? {
        Ok(()) => Output::new().values("flat", o),
        Err(e) => Output::unported(&e).unwrap_or_else(|| {
            o.push(status(&e));
            Output::new().values("flat", o)
        }),
    })
}

// ---- generators ------------------------------------------------------------------------

/// The plane `z = 0` (a filler base for solvers without surfaces).
fn plane_z() -> SrfIn {
    SrfIn::Plane {
        o: [0.0, 0.0, 0.0],
        n: [0.0, 0.0, 1.0],
    }
}

/// A bilinear patch `P(u, v) = f(u, v)` over `[-l, l]^2` (knots `-l -l l l`).
fn patch(l: f64, f: impl Fn(f64, f64) -> [f64; 3]) -> SrfIn {
    let net = [-l, l]
        .iter()
        .map(|&u| {
            [-l, l]
                .iter()
                .map(|&v| {
                    let [x, y, z] = f(u, v);
                    [x, y, z, NOW]
                })
                .collect()
        })
        .collect();
    SrfIn::Bspline(BssIn {
        oob: false,
        form: 0,
        net,
        p: 1,
        q: 1,
        u: vec![-l, -l, l, l],
        v: vec![-l, -l, l, l],
    })
}

const L: f64 = 10.0;

/// The floor `(u, v, 0)` (normal `+z`).
fn floor() -> SrfIn {
    patch(L, |u, v| [u, v, 0.0])
}

/// The wall `(v, 0, u)` (normal `+y`).
fn wall_y() -> SrfIn {
    patch(L, |u, v| [v, 0.0, u])
}

/// The wall `(x, u, v)` (normal `+x`).
fn wall_x(x: f64) -> SrfIn {
    patch(L, move |u, v| [x, u, v])
}

/// A straight degree-1 curve from `a` to `b`.
fn line(a: [f64; 3], bb: [f64; 3]) -> CrvIn {
    CrvIn {
        dim: 3,
        cpts: vec![[a[0], a[1], a[2], NOW], [bb[0], bb[1], bb[2], NOW]],
        p: 1,
        u: vec![0.0, 0.0, 1.0, 1.0],
    }
}

/// A rational quadratic arc of radius `r` about the origin in `z = 0`, from angle 0
/// through `quarters` quarter circles (1..=4).
fn arc(r: f64, quarters: usize) -> CrvIn {
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let cs = [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0), (1.0, 0.0)];
    let mut cpts = Vec::new();
    let mut u = vec![0.0, 0.0, 0.0];
    for (q, pair) in cs.windows(2).take(quarters).enumerate() {
        let [(c0, s0), (c1, s1)] =
            [pair.first(), pair.last()].map(|p| p.copied().unwrap_or((1.0, 0.0)));
        if q == 0 {
            cpts.push([r * c0, r * s0, 0.0, 1.0]);
        }
        let (cx, cy) = (c0 + c1, s0 + s1);
        cpts.push([r * cx * w, r * cy * w, 0.0, w]);
        cpts.push([r * c1, r * s1, 0.0, 1.0]);
        let t = (q + 1) as f64 / quarters as f64;
        if q + 1 < quarters {
            u.extend([t, t]);
        }
    }
    u.extend([1.0, 1.0, 1.0]);
    CrvIn {
        dim: 3,
        cpts,
        p: 2,
        u,
    }
}

fn gen_cfg(rng: &mut Rng, extra_max: u64, radius: f64) -> Cfg {
    Cfg {
        tol: if rng.chance(0.8) {
            1.0e-6
        } else {
            rng.uniform(1e-8, 1e-3)
        },
        extra: rng.below(extra_max + 1) as usize,
        radius,
        slope: 0.0,
        target: rng.uniform(-3.0, 3.0),
        gu: 0.0,
        gv: 0.0,
    }
}

fn jitter(rng: &mut Rng, v: f64, s: f64) -> f64 {
    v + rng.uniform(-s, s)
}

/// The floor / `y = 0` wall surfaces with offsets `r`.
fn corner_surfs(r: f64) -> Surfs {
    Surfs {
        base: [floor(), wall_y()],
        has: [true, true],
        off: [r, r],
        orient: [1.0, 1.0],
    }
}

fn gen_plane(rng: &mut Rng) -> Inputs {
    let r = rng.uniform(0.2, 2.0);
    let x0 = rng.uniform(-5.0, 5.0);
    let mut surfs = corner_surfs(r);
    if rng.chance(0.2) {
        surfs.base = [gen_surface(rng), gen_surface(rng)];
    }
    let normal = if rng.chance(0.7) {
        [1.0, 0.0, 0.0]
    } else {
        [1.0, rng.uniform(-0.3, 0.3), rng.uniform(-0.3, 0.3)]
    };
    let s = if rng.chance(0.5) { 0.01 } else { 1.0 };
    let d = [-L, -L, L, L];
    Inputs::Plane {
        cfg: gen_cfg(rng, 4, r),
        surfs,
        orig: [x0, r, r],
        normal,
        dom1: d,
        dom2: d,
        uv1: [jitter(rng, x0, s), jitter(rng, r, s)],
        uv2: [jitter(rng, r, s), jitter(rng, x0, s)],
    }
}

fn gen_rail_eu(rng: &mut Rng) -> Inputs {
    let r = rng.uniform(0.2, 2.0);
    let mut surfs = corner_surfs(r);
    let brep = if rng.chance(0.15) {
        surfs.base = [gen_surface(rng), gen_surface(rng)];
        BrepIn::File {
            file: (*rng.choose(&FILES).unwrap_or(&"plane_square.smb")).to_string(),
            rebuild: false,
        }
    } else {
        let (a, bb) = (rng.uniform(-6.0, -1.0), rng.uniform(1.0, 6.0));
        let (c, d) = if rng.chance(0.8) {
            (rng.uniform(-3.0, r - 0.05), rng.uniform(r + 0.05, 5.0))
        } else {
            (rng.uniform(-3.0, 3.0), rng.uniform(3.5, 6.0))
        };
        // mostly a real SmPlane face (its UV trim curves come from the plane's own
        // projection), sometimes the floor patch
        let surface = if rng.chance(0.75) { plane_z() } else { floor() };
        surfs.base[0] = surface.clone();
        BrepIn::Face {
            surface,
            dom: [a, c, bb, d],
        }
    };
    if rng.chance(0.15) {
        if let Some(h) = surfs.has.get_mut(rng.below(2) as usize) {
            *h = false;
        }
    }
    let mut c = gen_cfg(rng, 5, r);
    if c.extra == 5 && rng.chance(0.7) {
        c.extra = rng.below(5) as usize;
    }
    Inputs::RailEu {
        cfg: c,
        surfs,
        brep,
        side: rng.below(16) as i64,
        frac: rng.uniform(0.0, 1.0),
        rail: usize::from(!rng.chance(0.8)),
        ptype: rng.below(8) as u8,
    }
}

fn gen_rail_rail(rng: &mut Rng) -> Inputs {
    let r = rng.uniform(0.2, 2.0);
    let mut surfs = [corner_surfs(r), corner_surfs(r)];
    surfs[1].base[1] = wall_x(rng.uniform(-3.0, 3.0));
    // a third of the cases miss surfaces (the `SM_ERR` exit); the others solve
    if rng.chance(0.33) {
        let k = rng.below(2) as usize;
        let i = rng.below(2) as usize;
        if let Some(h) = surfs.get_mut(k).and_then(|s| s.has.get_mut(i)) {
            *h = false;
        }
        for s in &mut surfs {
            for h in &mut s.has {
                if rng.chance(0.2) {
                    *h = false;
                }
            }
        }
    }
    Inputs::RailRail {
        cfg: [gen_cfg(rng, 2, r), gen_cfg(rng, 2, r)],
        surfs,
        uv: [rng.uniform(-3.0, 3.0), rng.uniform(-3.0, 3.0)],
        rail: rng.below(2) as usize,
        other: rng.below(2) as usize,
    }
}

fn tsect(uv0: [f64; 2], uv1: [f64; 2], cparam: f64) -> Tsect {
    Tsect { uv0, uv1, cparam }
}

/// A cylinder or cone case (`which` 0/1): the fillet between the floor and the `y = 0`
/// wall along the x axis.
fn gen_straight(rng: &mut Rng, which: u8) -> Inputs {
    let r = rng.uniform(0.2, 2.0);
    let (xs, xe) = (rng.uniform(-5.0, -1.0), rng.uniform(1.0, 5.0));
    let slope = if which == 1 || rng.chance(0.15) {
        if rng.chance(0.3) {
            0.0
        } else {
            rng.uniform(-0.5 * r, 0.5 * r)
        }
    } else {
        0.0
    };
    let r1 = r + slope;
    let mut surfs = corner_surfs(r);
    let mut ts = vec![tsect([xs, r], [r, xs], 0.0), tsect([xe, r1], [r1, xe], 1.0)];
    if rng.chance(0.2) {
        // the surfaces swapped: the arc runs the other way
        surfs.base = [wall_y(), floor()];
        for t in &mut ts {
            std::mem::swap(&mut t.uv0, &mut t.uv1);
        }
    }
    if rng.chance(0.2) {
        for t in &mut ts {
            for v in t.uv0.iter_mut().chain(t.uv1.iter_mut()) {
                *v = jitter(rng, *v, 0.05);
            }
        }
    }
    let curve = if rng.chance(0.1) {
        arc(2.0, 1)
    } else {
        line([xs - 0.5, 0.0, 0.0], [xe + 0.5, 0.0, 0.0])
    };
    // a real `SmLine` edge (`IsLine` of the line kind) in a fifth of the cases
    let analytic = rng.chance(0.2).then_some(AnalyticEdge::Line {
        a: [xs - 0.5, 0.0, 0.0],
        b: [xe + 0.5, 0.0, 0.0],
    });
    let mut c = gen_cfg(rng, 0, r);
    c.slope = slope;
    Inputs::Geom {
        which,
        cfg: c,
        surfs,
        fsg: u8::from(!rng.chance(0.1)),
        before: rng.chance(0.5),
        after: rng.chance(0.5),
        factor: if rng.chance(0.7) {
            0.5
        } else {
            rng.uniform(0.0, 1.0)
        },
        brep: BrepIn::Wire {
            curve,
            t0: 0.0,
            t1: 1.0,
        },
        analytic,
        rails: None,
        eu: [rng.below(2) as i64, rng.below(2) as i64],
        tsects: ts,
    }
}

/// A cylinder or cone case whose rail points are degenerate: both on the floor, a hair
/// apart (`|angle| < EFF_ZERO_SQRT`, or just above it), or the floor and a roof straight
/// across the centre line (`|angle| > pi - EFF_ZERO_SQRT`, or just inside it), with a
/// real `SmLine` or a B-spline edge.
fn gen_degenerate(rng: &mut Rng) -> Inputs {
    let r = rng.uniform(0.2, 2.0);
    let (xs, xe) = (rng.uniform(-5.0, -1.0), rng.uniform(1.0, 5.0));
    let opposed = rng.chance(0.5);
    let gap = *rng
        .choose(&[0.0, 1e-12, 1e-8, 1e-6, 1e-4, 1e-2])
        .unwrap_or(&0.0);
    let surfs = if opposed {
        Surfs {
            base: [floor(), patch(L, move |u, v| [u, v, 2.0 * r])],
            has: [true, true],
            off: [r, -r],
            orient: [1.0, -1.0],
        }
    } else {
        Surfs {
            base: [floor(), floor()],
            has: [true, true],
            off: [r, r],
            orient: [1.0, 1.0],
        }
    };
    // the second surface's point is `gap` aside the first's
    let ts = vec![
        tsect([xs, r], [xs + gap, r], 0.0),
        tsect([xe, r], [xe + gap, r], 1.0),
    ];
    let which = u8::from(rng.chance(0.5));
    let analytic = rng.chance(0.5).then_some(AnalyticEdge::Line {
        a: [xs - 0.5, 0.0, 0.0],
        b: [xe + 0.5, 0.0, 0.0],
    });
    Inputs::Geom {
        which,
        cfg: gen_cfg(rng, 0, r),
        surfs,
        fsg: 1,
        before: rng.chance(0.3),
        after: rng.chance(0.3),
        factor: 0.5,
        brep: BrepIn::Wire {
            curve: line([xs - 0.5, 0.0, 0.0], [xe + 0.5, 0.0, 0.0]),
            t0: 0.0,
            t1: 1.0,
        },
        analytic,
        rails: None,
        eu: [rng.below(2) as i64, rng.below(2) as i64],
        tsects: ts,
    }
}

/// A torus case: the inner fillet between the floor and the wall tangent to an arc edge
/// of radius `big` at angle 0.
fn gen_torus(rng: &mut Rng) -> Inputs {
    let big = rng.uniform(2.0, 5.0);
    let r = rng.uniform(0.2, 1.0);
    let rho = big - r;
    let quarters = 1 + rng.below(4) as usize;
    let span = quarters as f64 * std::f64::consts::FRAC_PI_2;
    // the end point at the start's angle: the span wraps to 360 degrees (with an open
    // edge, the seam of the torus moves); some cases have the arc centre on the axis (no
    // unique nearest point on the edge circle) or a vanishing radius
    let seam = rng.chance(0.1);
    let on_axis = rng.chance(0.05);
    let tiny = rng.chance(0.05);
    let phi = if seam {
        0.0
    } else {
        span * if rng.chance(0.5) {
            1.0
        } else {
            rng.uniform(0.3, 0.95)
        }
    };
    let slope = if rng.chance(0.1) { 0.2 * r } else { 0.0 };
    let surfs = Surfs {
        base: [floor(), wall_x(big)],
        has: [true, true],
        off: [r, -r],
        orient: [1.0, -1.0],
    };
    let mut ts = vec![
        tsect([rho, 0.0], [0.0, r], 0.0),
        tsect([rho * phi.cos(), rho * phi.sin()], [0.0, r], 1.0),
    ];
    if on_axis {
        if let Some(t) = ts.first_mut() {
            t.uv0 = [0.0, 0.0];
        }
    }
    let curve = if rng.chance(0.1) {
        line([0.0, 0.0, 0.0], [big, 0.0, 0.0])
    } else {
        arc(big, quarters)
    };
    // a real `SmCircle` (a full circle for four quarters: a closed edge) or `SmLine`
    // edge in a third of the cases
    let analytic = match rng.below(10) {
        0 => Some(AnalyticEdge::Line {
            a: [0.0, 0.0, 0.0],
            b: [big, 0.0, 0.0],
        }),
        1 | 2 => Some(AnalyticEdge::Circle {
            radius: big,
            span: 90.0 * quarters as f64,
        }),
        _ => None,
    };
    // rail ends typed RAIL_X_EDGEUSE, processed, around the start of the arc: the
    // before-the-edge-start check may set the extend-after flag
    let rails = rng.chance(0.3).then(|| {
        let mut p = [[0.0; 3]; 4];
        for q in &mut p {
            *q = [
                jitter(rng, big, 1.0),
                jitter(rng, 0.0, 1.0),
                jitter(rng, 0.0, 0.5),
            ];
        }
        p
    });
    let mut c = gen_cfg(rng, 0, r);
    c.slope = slope;
    if tiny {
        c.radius = 1.0e-12;
    }
    // the rail end checks only run for an edge not extended after
    let after = if rails.is_some() {
        rng.chance(0.2)
    } else {
        rng.chance(0.85)
    };
    Inputs::Geom {
        which: 2,
        cfg: c,
        surfs,
        fsg: u8::from(!rng.chance(0.1)),
        before: rng.chance(0.5),
        after,
        factor: 0.5,
        brep: BrepIn::Wire {
            curve,
            t0: 0.0,
            t1: 1.0,
        },
        analytic,
        rails,
        eu: if rng.chance(0.5) { [0, 1] } else { [1, 0] },
        tsects: ts,
    }
}

/// The op of a case, for its name: `rail_rail_solve` when all four offset surfaces are
/// present (the solve runs), `rail_rail_missing` for the `SM_ERR` exit; `cylinder`,
/// `cone`, `torus` for the geometry builders.
fn op_label(i: &Inputs) -> &'static str {
    match i {
        Inputs::Plane { .. } => "plane",
        Inputs::RailEu { .. } => "rail_eu",
        Inputs::RailRail { surfs, .. } => {
            if surfs.iter().all(|s| s.has.iter().all(|&h| h)) {
                "rail_rail_solve"
            } else {
                "rail_rail_missing"
            }
        }
        Inputs::Geom { which: 0, .. } => "cylinder",
        Inputs::Geom { which: 1, .. } => "cone",
        Inputs::Geom { .. } => "torus",
        Inputs::Corner { .. } => "corner",
        Inputs::Record { .. } => "record",
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    match rng.below(24) {
        0..=3 => gen_plane(rng),
        4..=8 => gen_rail_eu(rng),
        9..=12 => gen_rail_rail(rng),
        13..=15 => gen_straight(rng, 0),
        16 | 17 => gen_straight(rng, 1),
        18..=20 => gen_torus(rng),
        21 => gen_degenerate(rng),
        22 => Inputs::Corner {
            cfg: gen_cfg(rng, 2, 1.0),
            index: 2 + rng.below(50),
        },
        _ => Inputs::Record {
            cfg: gen_cfg(rng, 0, 1.0),
            brep: if rng.chance(0.5) {
                BrepIn::Wire {
                    curve: line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                    t0: 0.0,
                    t1: 1.0,
                }
            } else {
                BrepIn::Face {
                    surface: floor(),
                    dom: [-1.0, -1.0, 1.0, 1.0],
                }
            },
            eu: rng.below(8) as i64,
        },
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "blend.fillet_solver_p2"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_case(rng)).ok()
    }

    fn cases(&self, ctx: &crate::case::CaseContext) -> Result<Vec<crate::Case>, HarnessError> {
        let ctx = crate::case::CaseContext {
            seeds: ctx.seeds.max(300),
            ..ctx.clone()
        };
        let mut cases = crate::runner::default_cases(self, &ctx)?;
        // generated cases carry their op in the name (`gen0042_rail_rail_solve`), so the
        // report and `--filter` can count and select them per op
        for c in &mut cases {
            if matches!(c.source, crate::case::CaseSource::Generated { .. }) {
                if let Ok(i) = c.parse::<Inputs>() {
                    c.name = format!("{}_{}", c.name, op_label(&i));
                }
            }
        }
        Ok(cases)
    }

    fn run_cpp(&self, case: &crate::Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &crate::Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}
