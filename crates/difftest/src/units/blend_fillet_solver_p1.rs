//! Runner for unit `blend.fillet_solver_p1`. C++: `solidcore_ffi::units::blend_fillet_solver_p1`.
//! Rust: `solidcore::blend::fillet_solver::p1`.
//!
//! Both sides build the same solver: the base `SmFilletSolver` (the shim derives a
//! class that only names itself, so every virtual is the base version, except the
//! deterministic `LoadJacobian` and `SetupOffsetValues` the shim switches on, mirrored by
//! [`ShimSolver`]) with the same offset surfaces (offsets of `SmPlane`s or
//! `SmBSplineSurface`s, blocks of unit `topology.edgeuse_p1`). Ops: the constructor,
//! getters and an offset-bookkeeping script (`state`), `SetupOffsetExtension` over
//! planes, open, pinched (a collapsed boundary at umin/umax/vmin/vmax, one or two sides)
//! and periodic (rational circle x line, either direction) B-spline surfaces at extreme
//! scales and extension factors (`extension`), the sphere-solve and rail-UV-curve function
//! objects with and without Jacobians and loading solvers (`sphere`, `rail_uv`; a
//! loading sphere solve evaluates the offset surfaces, unit `surface.offset_surface`),
//! `ComputeSurfaceValues` (fails in the base `SetupOffsetValues`, else evaluates the
//! offset surfaces), `ComputeUVVectors` (including zero and parallel tangents),
//! `SplitFilletGeom` with empty intervals, `FinishConstruction` (deferred with an offset
//! surface to copy: `SetOffsetSurface`, unit `blend.fillet_geom_p1`), the fillet
//! surface generators (`fsg`), `SmTol::GetAngTolRad(const SmFilletSolver*)` (`tol_ang`)
//! and the base `LoadInitialValues` (`initial_values`).
//!
//! The file-static helpers and `.cpp`-local classes run on the copy of
//! `SmFilletSolver.cpp` compiled by the static shim: `sm_FindNextGoodInterval`
//! (`find_next`), `sm_CheckEdgeCurveDomains` (`check_domains`), `sm_TestAnalyticSurface`
//! (`analytic`: planes and planar B-splines compare, the cone test of other surfaces
//! defers; `analytic_rev`: real `SmCone` (compares, the `SmCone_TYPE` branch),
//! bounded `SmCylinder` and `SmTorus` (built on both sides, defer at the cone test) and
//! `SmSphere` (defers: no Rust type)), `SmRailRailIntersectENFO` (`rail_rail`, two loading solvers), the
//! `SmFindClippedRadiusEFO` constructor and `Evaluate` (`clipped`: offsets of planes and
//! B-spline patches of either base, `SM_SP_U` and `SM_SP_V`, both `SM_ERR` exits; `F`,
//! `F'`, `found`, the caller's `sUV` and `m_dGuessT` compare, see `gen_clipped`),
//! `sm_TestSideEdgeConvexity` (`convexity`: an edge without edge uses, edges of upstream
//! test files through `IsConvexRadialSector`), `sm_TrimRail` (`trim_rail`, on a B-rep
//! edge; the zone tolerance each trim reads and the 3D curve's knots before the trim
//! compare too), `sm_TestManifoldTangentRollover` (`rollover`, on a
//! B-rep edge) and the solver's B-rep queries (`queries`: `GetVertex`,
//! `FindIndexOfRailOnFace`, `FindIndexOfRailXSideEdgeuse` with edge uses of upstream test
//! files, face and wire B-reps). B-rep cases build the C++ B-rep, export it through unit
//! `topology.edgeuse_p1` and import the same records on the Rust side; entity selectors
//! are resolved modulo the export's counts. `-2` marks a call both sides skip because
//! upstream dereferences `NULL` there.

#[path = "blend_fillet_solver_p1/split_rail.rs"]
mod split_rail;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::blend::fillet_custom_solver::{JacobianLoad, OffsetSurface, SurfaceId};
use solidcore::blend::fillet_geom::p1::{FilletBrep, FilletSolverId};
use solidcore::blend::fillet_solver::p1::{
    check_edge_curve_domains, compute_uv_vectors, find_next_good_interval, test_analytic_surface,
    test_manifold_tangent_rollover, test_side_edge_convexity, tol_ang_tol_rad, trim_rail,
    AnalyticTestSurface, FilStatus, FilletSolver, FilletSolverOffsetsData, FilletSolverOps,
    FilletSolverType, FilletSphereSolveEnfo, FilletSurfaceGenerator, FindClippedRadiusEfo,
    OffsetSurfaceSlot, RailRailIntersectEnfo, RailUvCurveIntersectEnfo,
};
use solidcore::curve::curve::Curve;
use solidcore::curve::line::Line;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::{Extent1d, ExtentNd};
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::matrix::Matrix;
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::curve_class::p1::CurveClassification;
use solidcore::intersect::point_class::{
    ClassGeometry, ClassObject, ClassObjectKind, PointClassType,
};
use solidcore::intersect::surface_intersector::p1::TsectPnt;
use solidcore::kernel::Kernel;
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use solidcore::nurbs::math::NOW;
use solidcore::surface::cone::Cone;
use solidcore::surface::offset_surface::SS_UNKNOWN;
use solidcore::surface::sphere::Sphere;
use solidcore::surface::surface::Surface;
use solidcore::surface::torus::{Torus, TorusOptions};
use solidcore::surface::volume::Volume;
use solidcore::topology::edgeuse::p1 as eu;
use solidcore::topology::{Brep, Edge, EdgeId, EdgeuseId, FaceId, Vertex};
use solidcore::{EntityRef, SolidError};
use solidcore_ffi::units::blend_fillet_solver_p1::{self as ffi, Op, StaticOp};
use solidcore_ffi::units::topology_edgeuse_p1 as ffi1;

use super::topology_edgeuse_p1::{
    brep_block_of, bspline_of, domain, gen_brep, gen_curve, gen_surface, import, path_of,
    push_curve, push_surface, sel, surface_of, BrepIn, BssIn, CrvIn, SrfIn, FILES,
};
use crate::{HarnessError, Output, Rng, UnitDiff};

/// The runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Arrays {
    x: Vec<f64>,
    f: Vec<f64>,
    jac: Option<[usize; 2]>,
}

/// Offset surfaces of one base installed into the solver (`surf has0 off0 has1 off1`).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Surfs {
    base: SrfIn,
    has: [bool; 2],
    off: [f64; 2],
}

impl Default for Surfs {
    fn default() -> Self {
        Self {
            base: SrfIn::Plane {
                o: [0.0, 0.0, 0.0],
                n: [0.0, 0.0, 1.0],
            },
            has: [false, false],
            off: [0.0, 0.0],
        }
    }
}

/// The plane `z = 0`.
fn z_plane() -> SrfIn {
    SrfIn::Plane {
        o: [0.0, 0.0, 0.0],
        n: [0.0, 0.0, 1.0],
    }
}

fn z_planes() -> [SrfIn; 2] {
    [z_plane(), z_plane()]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    State {
        tols: [f64; 3],
        base: [SrfIn; 2],
        has: [bool; 2],
        off: [f64; 2],
        orient: [f64; 2],
        cmds: Vec<[f64; 3]>,
    },
    Extension {
        ext: f64,
        surface: SrfIn,
        dom: [f64; 4],
    },
    Sphere {
        c: [f64; 3],
        r: f64,
        adjusts: bool,
        #[serde(default)]
        load: bool,
        #[serde(default)]
        found: bool,
        #[serde(default)]
        surfs: Surfs,
        arrays: Arrays,
    },
    RailUv {
        rail: usize,
        #[serde(default)]
        load: bool,
        #[serde(default)]
        found: bool,
        curve: CrvIn,
        /// An `SmLine` UV curve (start, end, dimension 2 or 3) used instead of `curve`:
        /// the shim builds the real `SmLine`, whose virtual `Evaluate` is analytic.
        #[serde(default)]
        line: Option<[f64; 7]>,
        ivl: [f64; 2],
        arrays: Arrays,
    },
    SurfValues {
        #[serde(default)]
        setup: bool,
        #[serde(default)]
        surfs: Surfs,
        uv: [f64; 4],
    },
    UvVectors {
        vecs: [[f64; 3]; 5],
        uvd: [f64; 4],
    },
    Split,
    /// Direct real rail-copy and owner-notification operation.
    SplitRail,
    Finish {
        surface: SrfIn,
        has: [bool; 2],
        off: f64,
    },
    Fsg {
        kind: u8,
        args: [f64; 4],
        /// Kind 1 (linear) only: the two rail curves `CreateSurface` is given (empty:
        /// no `CreateSurface` call).
        #[serde(default)]
        rails: Vec<CrvIn>,
    },
    TolAng {
        has: bool,
        ang: f64,
    },
    InitialValues {
        do2: u32,
        rail: usize,
        idx2: u32,
    },
    FindNext {
        start: usize,
        cls: Vec<u32>,
    },
    CheckDomains {
        brep: BrepIn,
        edge: i64,
        before: bool,
        after: bool,
    },
    Analytic {
        surface: SrfIn,
    },
    /// `sm_TestAnalyticSurface` on a real analytic surface: `kind` 0 a canonical cone
    /// (`bot top height`), 1 a bounded cylinder (`radius start end height`, then
    /// `MakeNurb`), 2 a canonical sphere (`radius`), 3 a canonical torus (`major
    /// minor`), placed at `origin` with axes `x`, `y`.
    AnalyticRev {
        kind: u8,
        origin: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        p: [f64; 4],
    },
    RailRail {
        rail: usize,
        other: usize,
        load: [bool; 2],
        found: [bool; 2],
        arrays: Arrays,
    },
    Clipped {
        idx: usize,
        dist: f64,
        tol: f64,
        has: [bool; 2],
        off: [f64; 2],
        orient: [f64; 2],
        doms: [f64; 8],
        param: u8,
        iso: f64,
        uv: [f64; 4],
        /// Bases of offset surfaces 0 and 1 (default: the `z = 0` plane for both).
        #[serde(default = "z_planes")]
        bases: [SrfIn; 2],
        /// `Evaluate(radius, ..)` after the constructor, when given.
        #[serde(default)]
        radius: Option<f64>,
    },
    Convexity {
        brep: BrepIn,
        /// `-1`: a fresh edge without edge uses.
        edge: i64,
        tangential: bool,
    },
    TrimRail {
        brep: BrepIn,
        edge: i64,
        /// Fractions of the edge interval.
        t: [f64; 2],
    },
    Queries {
        brep: BrepIn,
        eu: [i64; 2],
        face: i64,
        side: i64,
    },
    /// `sm_TestManifoldTangentRollover` on an edge with tolerance `tol` (radians).
    Rollover {
        brep: BrepIn,
        edge: i64,
        tol: f64,
    },
}

impl Inputs {
    fn ffi(&self) -> Option<Op> {
        Some(match self {
            Inputs::State { .. } => Op::State,
            Inputs::Extension { .. } => Op::Extension,
            Inputs::Sphere { .. } => Op::Sphere,
            Inputs::RailUv { .. } => Op::RailUv,
            Inputs::SurfValues { .. } => Op::SurfValues,
            Inputs::UvVectors { .. } => Op::UvVectors,
            Inputs::Split => Op::Split,
            Inputs::Finish { .. } => Op::Finish,
            Inputs::Fsg { .. } => Op::Fsg,
            Inputs::TolAng { .. } | Inputs::InitialValues { .. } => Op::Misc,
            _ => return None,
        })
    }

    fn static_op(&self) -> Option<StaticOp> {
        Some(match self {
            Inputs::FindNext { .. } => StaticOp::FindNext,
            Inputs::CheckDomains { .. } => StaticOp::CheckDomains,
            Inputs::Analytic { .. } => StaticOp::Analytic,
            Inputs::AnalyticRev { .. } => StaticOp::AnalyticRev,
            Inputs::RailRail { .. } => StaticOp::RailRail,
            Inputs::Clipped { .. } => StaticOp::Clipped,
            Inputs::Convexity { .. } => StaticOp::Convexity,
            Inputs::TrimRail { .. } => StaticOp::TrimRail,
            Inputs::Queries { .. } => StaticOp::Queries,
            Inputs::Rollover { .. } => StaticOp::Rollover,
            Inputs::SplitRail => StaticOp::SplitRail,
            _ => return None,
        })
    }

    fn brep(&self) -> Option<&BrepIn> {
        match self {
            Inputs::CheckDomains { brep, .. }
            | Inputs::Convexity { brep, .. }
            | Inputs::TrimRail { brep, .. }
            | Inputs::Queries { brep, .. }
            | Inputs::Rollover { brep, .. } => Some(brep),
            _ => None,
        }
    }
}

/// The Jacobian of `a` (rows and columns at most `MAX_ARRAY`, as the FFI accepts).
fn jacobian_of(a: &Arrays) -> Result<Option<Matrix>, HarnessError> {
    match a.jac {
        Some([r, c]) if r > ffi::MAX_ARRAY || c > ffi::MAX_ARRAY => {
            Err(harness("Jacobian larger than MAX_ARRAY"))
        }
        jac => Ok(jac.map(|[r, c]| Matrix::new(r, c))),
    }
}

fn push_arrays(o: &mut Vec<f64>, a: &Arrays) {
    o.push(a.x.len() as f64);
    o.extend_from_slice(&a.x);
    o.push(a.f.len() as f64);
    o.extend_from_slice(&a.f);
    match a.jac {
        Some([r, c]) => o.extend_from_slice(&[1.0, r as f64, c as f64]),
        None => o.extend_from_slice(&[0.0, 0.0, 0.0]),
    }
}

fn push_surfs(o: &mut Vec<f64>, s: &Surfs) -> Result<(), HarnessError> {
    push_surface(o, &s.base)?;
    for (h, d) in s.has.iter().zip(&s.off) {
        o.push(b(*h));
        o.push(*d);
    }
    Ok(())
}

// ---- B-rep cases -------------------------------------------------------------------------

/// The case's B-rep: its counts and B-rep (the C++ side's from the export, the Rust
/// side's from [`own_case`]).
struct BrepCase {
    path: String,
    block: Vec<f64>,
    counts: [usize; 10],
    brep: Brep,
}

/// Builds the case's B-rep on both sides; `Err(output)` when it cannot be built.
fn brep_case(brep: &BrepIn) -> Result<Result<BrepCase, Output>, HarnessError> {
    let path = path_of(Some(brep))?;
    let block = brep_block_of(Some(brep), false, false)?;
    let ex = crate::export_memo::export("topology_edgeuse_p1", &[&path], &block, || {
        ffi1::eval(ffi1::Op::Export, &path, &block)
    })
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

/// The Rust side's B-rep case: the source read by the Rust reader or built by the Rust
/// kernel in the shim's traversal order ([`crate::own_reader::edgeuse_graph_brep`]),
/// never imported from the C++ export; `Err(output)` when the Rust build fails, as
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

/// The resolved selectors of a B-rep case: `[a, b, c, d]` export indices (`-1` none).
fn resolve(i: &Inputs, c: &BrepCase) -> [i64; 4] {
    let [_, _, _, n_f, _, _, n_eu, n_e, _, _] = c.counts;
    match i {
        Inputs::CheckDomains { edge, .. }
        | Inputs::TrimRail { edge, .. }
        | Inputs::Rollover { edge, .. } => [sel(*edge, n_e), -1, -1, -1],
        Inputs::Convexity { edge, .. } => {
            [if *edge < 0 { -1 } else { sel(*edge, n_e) }, -1, -1, -1]
        }
        Inputs::Queries { eu, face, side, .. } => [
            sel(eu[0], n_eu),
            sel(eu[1], n_eu),
            sel(*face, n_f),
            sel(*side, n_eu),
        ],
        _ => [-1; 4],
    }
}

/// The trim interval of a `trim_rail` case: fractions of the edge's interval.
fn trim_interval(c: &BrepCase, e: i64, t: [f64; 2]) -> Result<[f64; 2], HarnessError> {
    let id = EdgeId::from_index(u32::try_from(e).map_err(|_| harness("edge index"))?);
    let ivl = c
        .brep
        .edge(id)
        .map_err(|e| harness(e.to_string()))?
        .interval;
    let at = |f: f64| ivl.min() + (ivl.max() - ivl.min()) * f;
    Ok([at(t[0]), at(t[1])])
}

fn static_input(i: &Inputs, c: Option<&BrepCase>) -> Result<Vec<f64>, HarnessError> {
    let mut o = c.map(|c| c.block.clone()).unwrap_or_default();
    let r = c.map_or([-1; 4], |c| resolve(i, c));
    match i {
        Inputs::FindNext { start, cls } => {
            o.push(cls.len() as f64);
            o.push(*start as f64);
            o.extend(cls.iter().map(|&k| f64::from(k)));
        }
        Inputs::CheckDomains { before, after, .. } => {
            o.extend([r[0] as f64, b(*before), b(*after)]);
        }
        Inputs::Analytic { surface } => push_surface(&mut o, surface)?,
        Inputs::AnalyticRev {
            kind,
            origin,
            x,
            y,
            p,
        } => {
            o.push(f64::from(*kind));
            o.extend(origin.iter().chain(x).chain(y).chain(p));
        }
        Inputs::RailRail {
            rail,
            other,
            load,
            found,
            arrays,
        } => {
            o.extend([
                *rail as f64,
                *other as f64,
                b(load[0]),
                b(found[0]),
                b(load[1]),
                b(found[1]),
            ]);
            push_arrays(&mut o, arrays);
        }
        Inputs::Clipped {
            idx,
            dist,
            tol,
            has,
            off,
            orient,
            doms,
            param,
            iso,
            uv,
            bases,
            radius,
        } => {
            o.extend([*idx as f64, *dist, *tol]);
            push_surface(&mut o, &bases[0])?;
            push_surface(&mut o, &bases[1])?;
            o.extend([b(has[0]), off[0], b(has[1]), off[1]]);
            o.extend_from_slice(orient);
            o.extend_from_slice(doms);
            o.push(f64::from(*param));
            o.push(*iso);
            o.extend_from_slice(uv);
            o.extend([b(radius.is_some()), radius.unwrap_or(0.0)]);
        }
        Inputs::Convexity { tangential, .. } => o.extend([r[0] as f64, b(*tangential)]),
        Inputs::TrimRail { t, .. } => {
            let c = c.ok_or_else(|| harness("trim_rail without B-rep"))?;
            let [t0, t1] = trim_interval(c, r[0], *t)?;
            o.extend([r[0] as f64, t0, t1]);
        }
        Inputs::Queries { .. } => o.extend(r.map(|v| v as f64)),
        Inputs::Rollover { tol, .. } => o.extend([r[0] as f64, *tol]),
        _ => return Err(harness("not a static op")),
    }
    Ok(o)
}

fn cpp_input(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    match i {
        Inputs::State {
            tols,
            base,
            has,
            off,
            orient,
            cmds,
        } => {
            o.extend_from_slice(tols);
            push_surface(&mut o, &base[0])?;
            push_surface(&mut o, &base[1])?;
            for (h, d) in has.iter().zip(off) {
                o.push(b(*h));
                o.push(*d);
            }
            o.extend_from_slice(orient);
            o.push(cmds.len() as f64);
            for c in cmds {
                o.extend_from_slice(c);
            }
        }
        Inputs::Extension { ext, surface, dom } => {
            o.push(*ext);
            push_surface(&mut o, surface)?;
            o.extend_from_slice(dom);
        }
        Inputs::Sphere {
            c,
            r,
            adjusts,
            load,
            found,
            surfs,
            arrays,
        } => {
            o.extend_from_slice(c);
            o.push(*r);
            o.extend([b(*adjusts), b(*load), b(*found)]);
            push_surfs(&mut o, surfs)?;
            push_arrays(&mut o, arrays);
        }
        Inputs::RailUv {
            rail,
            load,
            found,
            curve,
            line,
            ivl,
            arrays,
        } => {
            o.push(*rail as f64);
            o.push(b(*load));
            o.push(b(*found));
            o.push(b(line.is_some()));
            if let Some(l) = line {
                o.extend_from_slice(l);
            }
            push_curve(&mut o, curve)?;
            o.extend_from_slice(ivl);
            push_arrays(&mut o, arrays);
        }
        Inputs::SurfValues { setup, surfs, uv } => {
            o.push(b(*setup));
            push_surfs(&mut o, surfs)?;
            o.extend_from_slice(uv);
        }
        Inputs::UvVectors { vecs, uvd } => {
            for v in vecs {
                o.extend_from_slice(v);
            }
            o.extend_from_slice(uvd);
        }
        Inputs::Split => o.push(0.0),
        Inputs::Finish { surface, has, off } => {
            push_surface(&mut o, surface)?;
            o.push(b(has[0]));
            o.push(b(has[1]));
            o.push(*off);
        }
        Inputs::Fsg { kind, args, rails } => {
            o.push(f64::from(*kind));
            o.extend_from_slice(args);
            if *kind == 1 {
                // (the shim reads the rail count of the linear kind, then the rails)
                o.push(rails.len() as f64);
                for r in rails {
                    push_curve(&mut o, r)?;
                }
            }
        }
        Inputs::TolAng { has, ang } => o.extend([0.0, b(*has), *ang]),
        Inputs::InitialValues { do2, rail, idx2 } => {
            o.extend([1.0, f64::from(*do2), *rail as f64, f64::from(*idx2)]);
        }
        _ => return Err(harness("not a main-shim op")),
    }
    Ok(o)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    if matches!(i, Inputs::SplitRail) {
        return split_rail::cpp();
    }
    if let Some(op) = i.static_op() {
        let case = match i.brep() {
            Some(brep) => match brep_case(brep)? {
                Ok(c) => Some(c),
                Err(out) => return Ok(out),
            },
            None => None,
        };
        let path = case.as_ref().map(|c| c.path.clone()).unwrap_or_default();
        let input = static_input(i, case.as_ref())?;
        let flat = ffi::eval_static(op, &path, &input).map_err(|e| harness(e.to_string()))?;
        return Ok(Output::new().values("flat", flat));
    }
    let op = i.ffi().ok_or_else(|| harness("no op"))?;
    let input = cpp_input(i)?;
    let flat = ffi::eval(op, &input).map_err(|e| harness(e.to_string()))?;
    Ok(Output::new().values("flat", flat))
}

// ---- Rust side ------------------------------------------------------------------------

fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

fn b(v: bool) -> f64 {
    if v {
        1.0
    } else {
        0.0
    }
}

fn fil_status(v: f64) -> FilStatus {
    match v as i64 {
        1 => FilStatus::Processed,
        2 => FilStatus::Failure,
        3 => FilStatus::SurfIntFailure,
        4 => FilStatus::EndPntDropFailure,
        5 => FilStatus::ToBeSqueezed,
        6 => FilStatus::CubicRailRailInterpolate,
        7 => FilStatus::NoIntRailEu,
        8 => FilStatus::SolverNotConverge,
        9 => FilStatus::ToBeDeleted,
        10 => FilStatus::SurfTracingFailure,
        11 => FilStatus::SelfIntHandlingFailure,
        12 => FilStatus::SurfSkinningFailure,
        _ => FilStatus::Unprocessed,
    }
}

fn point_class(code: u32) -> PointClassType {
    match code {
        101 => PointClassType::Region,
        102 => PointClassType::Face,
        103 => PointClassType::Edge,
        104 => PointClassType::Vertex,
        105 => PointClassType::Point,
        106 => PointClassType::Curve,
        107 => PointClassType::Surface,
        108 => PointClassType::Volume,
        109 => PointClassType::Edgeuse,
        110 => PointClassType::Nothing,
        _ => PointClassType::Unknown,
    }
}

fn surf_param(code: u8) -> SurfParamType {
    match code {
        0 => SurfParamType::U,
        1 => SurfParamType::V,
        3 => SurfParamType::Both,
        4 => SurfParamType::Neither,
        _ => SurfParamType::Unknown,
    }
}

fn put_state(o: &mut Vec<f64>, fs: &FilletSolver) -> Result<(), SolidError> {
    for i in 0..2 {
        let s = fs.surface(i)?;
        o.push(b(s.is_some()));
        o.push(s.map_or(-1.0, |s| s.key as f64));
        o.push(s.map_or(0.0, |s| s.surface.offset_distance()));
        o.push(fs.orientation(i)?);
        o.push(fs.saved_offsets.get(i).copied().unwrap_or(0.0));
    }
    o.push(fs.extension_factor());
    o.push(b(fs.reverse_trim()));
    o.push(f64::from(fs.status() as i32));
    Ok(())
}

fn rust_state(
    tols: &[f64; 3],
    base: &[SrfIn; 2],
    has: [bool; 2],
    off: &[f64; 2],
    orient: &[f64; 2],
    cmds: &[[f64; 3]],
    o: &mut Vec<f64>,
) -> Result<Result<(), SolidError>, HarnessError> {
    let bases = [surface_of(&base[0])?, surface_of(&base[1])?];
    let kern = Kernel::default();
    // the shim's solver owns the constructor's fillet geometry
    let mut pseudo = FilletBrep::new();
    let mut fs = match FilletSolver::new_in(
        &kern,
        &mut pseudo,
        FilletSolverId(0),
        tols[0],
        tols[1],
        tols[2],
    ) {
        Ok(fs) => fs,
        Err(e) => return Ok(Err(e)),
    };
    let mut next_key = 0u64;
    let mut make = |rail: usize, d: f64| -> Result<OffsetSurfaceSlot, SolidError> {
        let key = next_key;
        next_key += 1;
        let base = if rail == 0 { &bases[0] } else { &bases[1] };
        Ok(OffsetSurfaceSlot {
            key,
            surface: OffsetSurface::new(
                &kern,
                d,
                base.clone(),
                SurfaceId::Free(rail as u32),
                SS_UNKNOWN,
            )?,
        })
    };
    let r = (|| -> Result<(), SolidError> {
        for (k, (h, d)) in has.iter().zip(off).enumerate() {
            if *h {
                fs.set_surface(k, Some(make(k, *d)?))?;
            }
        }
        fs.orientations = *orient;
        o.extend_from_slice(&[
            fs.this_approx_tol_3d(),
            fs.this_ang_tol_rad(),
            fs.tangency_tolerance(),
            fs.conversion_tol(),
            fs.extension_factor(),
            b(fs.reverse_trim()),
            f64::from(fs.status() as i32),
            fs.jacobian_size()? as f64,
            b(fs.can_make_analytic()?),
            b(fs.offset_radii_can_change()?),
            fs.fillet_geoms().len() as f64,
            b(fs.first_fillet_geom()? == fs.last_fillet_geom()?),
            fs.type_code()? as f64,
            b(FilletSolverOps::is_kind_of(&fs, FilletSolver::TYPE)?),
            b(FilletSolverOps::is_kind_of(
                &fs,
                solidcore::geom::object::OBJECT_TYPE,
            )?),
            b(FilletSolverOps::is_kind_of(&fs, 16002)?),
            b(fs.edgeuse(0)?.is_none()),
            b(fs.vertex(&Brep::default(), 0)?.is_none()),
            b(fs.fillet_surface_generator().is_none()),
            b(fs.fillet_geom_for_surfaces().is_none()),
            b(fs.fillet_executive().is_none()),
        ]);
        let mut saved = FilletSolverOffsetsData::default();
        for c in cmds {
            match c[0] as i64 {
                0 => fs.set_surfaces_to_zero_offset()?,
                1 => fs.reload_surface_offsets()?,
                2 => saved = fs.offsets_data()?,
                3 => fs.set_offsets_data(&saved)?,
                4 => {
                    let rail = c[1] as usize;
                    fs.set_surface(rail, Some(make(rail, c[2])?))?;
                }
                5 => fs.set_extension_factor(c[1]),
                6 => fs.set_reverse_trim(c[1] != 0.0),
                7 => fs.set_status(fil_status(c[1])),
                _ => {}
            }
            put_state(o, &fs)?;
        }
        Ok(())
    })();
    Ok(r)
}

fn put_arrays(o: &mut Vec<f64>, f: &[f64], j: Option<&Matrix>) {
    o.extend_from_slice(f);
    if let Some(j) = j {
        for r in 0..j.num_rows() {
            for c in 0..j.num_columns() {
                o.push(j.at(r, c).unwrap_or(f64::NAN));
            }
        }
    }
}

/// A [`ShimSolver`] with the case's offset surfaces (keys 0 and 1).
fn shim_with(s: &Surfs, load: bool, found: bool, setup: bool) -> Result<ShimSolver, HarnessError> {
    let base = surface_of(&s.base)?;
    let kern = Kernel::default();
    let mut fs = ShimSolver {
        base: FilletSolver::new(1.0e-6, 0.1, 0.01),
        load,
        found,
        setup,
    };
    for (k, (h, d)) in s.has.iter().zip(&s.off).enumerate() {
        if *h {
            let surface =
                OffsetSurface::new(&kern, *d, base.clone(), SurfaceId::Free(0), SS_UNKNOWN)
                    .map_err(|e| harness(e.to_string()))?;
            let slot = OffsetSurfaceSlot {
                key: k as u64,
                surface,
            };
            fs.base
                .set_surface(k, Some(slot))
                .map_err(|e| harness(e.to_string()))?;
        }
    }
    Ok(fs)
}

/// The shim's `put_bss`: `has_nurb [n m p q r s cpts(x y z w) knots_u knots_v]`.
fn put_bss(o: &mut Vec<f64>, s: &BSplineSurface) {
    let Some(sur) = s.gw_nurb() else {
        o.push(0.0);
        return;
    };
    o.push(1.0);
    let ix = sur.indices();
    o.extend([ix.n, ix.m, ix.p, ix.q, ix.r, ix.s].map(|x| x as f64));
    for row in sur.rows() {
        for p in row {
            o.extend([p.x, p.y, p.z, p.w]);
        }
    }
    o.extend_from_slice(sur.u_knots());
    o.extend_from_slice(sur.v_knots());
}

/// `put_crv` of the static shim: `1 t0 t1 P(t0) P(mid) P(t1)`, `-1` for none.
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

/// `put_zone_tol_and_knots` of the static shim, before `sm_TrimRail`: the zone
/// tolerance each trim reads (`SmTol::GetZoneTol3d(curve)`: the rail's tolerance for
/// a curve of the rail or of its edge uses, the system zone tolerance without curve),
/// then the 3D curve's unique knots and multiplicities (`n knots mults`, `-1` without
/// curve or when `GetKnots` fails).
fn put_zone_tol_and_knots(
    k: &Kernel,
    o: &mut Vec<f64>,
    b: &Brep,
    e: EdgeId,
    prim: EdgeuseId,
    mate: EdgeuseId,
) -> Result<(), HarnessError> {
    let err = |e: SolidError| harness(e.to_string());
    let edge = b.edge(e).map_err(err)?;
    let zt = |has: bool| {
        if has {
            edge.tolerance
        } else {
            k.tolerances.zone_tol_3d
        }
    };
    o.push(zt(edge.curve.is_some()));
    o.push(zt(eu::uv_trim_curve(b, prim).map_err(err)?.0.is_some()));
    o.push(zt(eu::uv_trim_curve(b, mate).map_err(err)?.0.is_some()));
    match edge.curve.as_ref().map(|c| c.knots(k, None)) {
        Some(Ok((knots, mults))) => {
            o.push(knots.len() as f64);
            o.extend_from_slice(&knots);
            o.extend(mults.iter().map(|&m| m as f64));
        }
        _ => o.push(-1.0),
    }
    Ok(())
}

fn eid(i: i64) -> Result<u32, HarnessError> {
    u32::try_from(i).map_err(|_| harness("entity index"))
}

/// The Rust side of a B-rep case (after the leading `0`).
fn rust_brep(
    i: &Inputs,
    c: &mut BrepCase,
    o: &mut Vec<f64>,
) -> Result<Result<(), SolidError>, HarnessError> {
    let k = Kernel::default();
    let r = resolve(i, c);
    let res = match i {
        Inputs::CheckDomains { before, after, .. } => {
            let e = EdgeId::from_index(eid(r[0])?);
            match check_edge_curve_domains(&k, &c.brep, e, *before, *after) {
                Ok(copy) => {
                    o.push(b(copy.is_some()));
                    let edge_curve = c
                        .brep
                        .edge(e)
                        .map_err(|e| harness(e.to_string()))?
                        .curve
                        .as_ref();
                    put_crv(&k, o, copy.as_ref().or(edge_curve));
                    Ok(())
                }
                Err(SolidError::InvalidInput { .. }) => {
                    o.push(-2.0);
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
        Inputs::Convexity { tangential, .. } => {
            let e = if r[0] < 0 {
                let v = c
                    .brep
                    .add_vertex_record(Vertex::default())
                    .map_err(|e| harness(e.to_string()))?;
                let edge = Edge {
                    vertices: [v, v],
                    edgeuses: Vec::new(),
                    interval: Extent1d::default(),
                    tolerance: 0.0,
                    curve: None,
                };
                c.brep
                    .add_edge_record(edge)
                    .map_err(|e| harness(e.to_string()))?
            } else {
                EdgeId::from_index(eid(r[0])?)
            };
            test_side_edge_convexity(&k, &mut c.brep, e, *tangential).map(|v| o.push(b(v)))
        }
        Inputs::TrimRail { t, .. } => {
            let e = EdgeId::from_index(eid(r[0])?);
            let [t0, t1] = trim_interval(c, r[0], *t)?;
            let prim = eu::primary_edgeuse(&c.brep, e).map_err(|e| harness(e.to_string()))?;
            let mate = match prim {
                Some(p) => eu::mate(&c.brep, p).map_err(|e| harness(e.to_string()))?,
                None => None,
            };
            if let (Some(prim), Some(mate)) = (prim, mate) {
                put_zone_tol_and_knots(&k, o, &c.brep, e, prim, mate)?;
                let st = match trim_rail(&k, &mut c.brep, e, &Extent1d::from_min_max(t0, t1)) {
                    Ok(()) => 0.0,
                    Err(e @ SolidError::Unported { .. }) => return Ok(Err(e)),
                    Err(e) => status(&e),
                };
                o.push(st);
                let bb = &c.brep;
                let err = |e: SolidError| harness(e.to_string());
                put_crv(&k, o, bb.edge(e).map_err(err)?.curve.as_ref());
                put_crv(&k, o, eu::uv_trim_curve(bb, prim).map_err(err)?.0);
                put_crv(&k, o, eu::uv_trim_curve(bb, mate).map_err(err)?.0);
                Ok(())
            } else {
                o.push(-2.0);
                Ok(())
            }
        }
        Inputs::Queries { .. } => {
            let opt = |v: i64| -> Result<Option<u32>, HarnessError> {
                if v < 0 {
                    Ok(None)
                } else {
                    eid(v).map(Some)
                }
            };
            let mut fs = FilletSolver::new(1.0e-6, 0.1, 0.01);
            fs.edgeuses = [
                opt(r[0])?.map(EdgeuseId::from_index),
                opt(r[1])?.map(EdgeuseId::from_index),
            ];
            let face = opt(r[2])?.map(FaceId::from_index);
            let side = EdgeuseId::from_index(eid(r[3])?);
            let one = |res: Result<usize, SolidError>| match res {
                Ok(v) => Ok(v as f64),
                Err(SolidError::InvalidInput { .. }) => Ok(-2.0),
                Err(e) => Err(e),
            };
            (|| -> Result<(), SolidError> {
                for idx in 0..2 {
                    let v = fs.vertex(&c.brep, idx)?;
                    o.push(v.map_or(-1.0, |v| f64::from(v.index())));
                }
                o.push(one(fs.find_index_of_rail_on_face(&c.brep, face))?);
                o.push(one(fs.find_index_of_rail_x_side_edgeuse(&c.brep, side))?);
                Ok(())
            })()
        }
        Inputs::Rollover { tol, .. } => {
            let e = EdgeId::from_index(eid(r[0])?);
            match test_manifold_tangent_rollover(&k, &mut c.brep, e, *tol) {
                Ok((tangent, opposite)) => {
                    o.extend([b(tangent), b(opposite)]);
                    Ok(())
                }
                Err(SolidError::InvalidInput { .. }) => {
                    o.push(-2.0);
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
        _ => return Err(harness("not a B-rep op")),
    };
    Ok(res)
}

fn rust_static(i: &Inputs, o: &mut Vec<f64>) -> Result<Result<(), SolidError>, HarnessError> {
    let k = Kernel::default();
    let res = match i {
        Inputs::FindNext { start, cls } => {
            let mut cc: CurveClassification<H> = CurveClassification::new(&k);
            let r = (|| -> Result<(), SolidError> {
                cc.set_size(&k, cls.len())?;
                for (n, &code) in cls.iter().enumerate() {
                    cc.at_mut(n)?
                        .mid
                        .set_class_object(point_class(code), Some(H));
                }
                Ok(())
            })();
            if r.is_ok() {
                let found = find_next_good_interval(&cc, *start);
                o.extend([0.0, b(found.is_some()), found.map_or(999.0, |v| v as f64)]);
            }
            r
        }
        Inputs::Analytic { surface } => {
            let s = surface_of(surface)?;
            test_analytic_surface(&Kernel::default(), AnalyticTestSurface::Surface(&s))
                .map(|v| o.push(b(v)))
        }
        Inputs::AnalyticRev {
            kind,
            origin,
            x,
            y,
            p,
        } => {
            let k = Kernel::default();
            let v = |a: [f64; 3]| Vec3::new(a[0], a[1], a[2]);
            let a2p = Axis2Placement::new(v(*origin), v(*x), v(*y));
            let tested = |s: AnalyticTestSurface<'_>, o: &mut Vec<f64>| {
                o.push(0.0);
                test_analytic_surface(&k, s).map(|r| o.push(b(r)))
            };
            let built = match kind {
                0 => {
                    Cone::create_canonical(&k, &a2p, p[0], p[1], p[2], false, false).map(Rev::Cone)
                }
                1 => {
                    let mut c = Cone::new_cylinder_bounded(
                        v(*origin),
                        v(*x),
                        v(*y),
                        p[0],
                        p[1],
                        p[2],
                        p[3],
                        false,
                        false,
                        false,
                    );
                    c.make_nurb(&k).map(|()| Rev::Cone(c))
                }
                2 => Sphere::create_canonical(&k, &a2p, p[0]).map(Rev::Sphere),
                _ => Torus::create_canonical(&k, &a2p, p[0], p[1], TorusOptions::default(), None)
                    .map(Rev::Torus),
            };
            match built {
                Err(e @ SolidError::Unported { .. }) => Err(e),
                Err(e) => {
                    o.push(status(&e));
                    Ok(())
                }
                Ok(Rev::Cone(c)) => tested(AnalyticTestSurface::Cone(&c), o),
                Ok(Rev::Sphere(s)) => tested(AnalyticTestSurface::Sphere(&s), o),
                Ok(Rev::Torus(t)) => tested(AnalyticTestSurface::Torus(&t), o),
            }
        }
        Inputs::RailRail {
            rail,
            other,
            load,
            found,
            arrays,
        } => {
            let mut s1 = shim_with(&Surfs::default(), load[0], found[0], false)?;
            let mut s2 = shim_with(&Surfs::default(), load[1], found[1], false)?;
            let mut f = arrays.f.clone();
            let mut j = jacobian_of(arrays)?;
            let mut e = RailRailIntersectEnfo::new(*rail, &mut s1, *other, &mut s2);
            let res = e.evaluate(&k, &arrays.x, &mut f, j.as_mut());
            match &res {
                Ok(found) => o.extend_from_slice(&[0.0, b(*found)]),
                Err(e @ SolidError::Unported { .. }) => return Ok(Err(e.clone())),
                Err(e) => o.extend_from_slice(&[status(e), 0.0]),
            }
            put_arrays(o, &f, j.as_ref());
            Ok(())
        }
        Inputs::Clipped {
            idx,
            dist,
            tol,
            has,
            off,
            orient,
            doms,
            param,
            iso,
            uv,
            bases,
            radius,
        } => {
            let base = [surface_of(&bases[0])?, surface_of(&bases[1])?];
            let kern = Kernel::default();
            let mut fs = FilletSolver::new(*tol, 0.1, 0.01);
            let r = (|| -> Result<(), SolidError> {
                for (k, ((h, d), base)) in has.iter().zip(off).zip(&base).enumerate() {
                    if *h {
                        let s = OffsetSurface::new(
                            &kern,
                            *d,
                            base.clone(),
                            SurfaceId::Free(k as u32),
                            SS_UNKNOWN,
                        )?;
                        fs.set_surface(
                            k,
                            Some(OffsetSurfaceSlot {
                                key: k as u64,
                                surface: s,
                            }),
                        )?;
                    }
                }
                fs.orientations = *orient;
                let [a0, a1, a2, a3, b0, b1, b2, b3] = *doms;
                let ds = [
                    Extent2d::from_points(Point2::new(a0, a1), Point2::new(a2, a3)),
                    Extent2d::from_points(Point2::new(b0, b1), Point2::new(b2, b3)),
                ];
                let uvs = [Vec2::new(uv[0], uv[1]), Vec2::new(uv[2], uv[3])];
                let both = has[0] && has[1];
                let (fields, evaluated) = {
                    let mut e = FindClippedRadiusEfo::new(
                        &mut fs,
                        *idx,
                        &ds,
                        surf_param(*param),
                        *iso,
                        uvs,
                        *dist,
                    )?;
                    let (u, v) = (e.other_domain.u_interval(), e.other_domain.v_interval());
                    let fields = vec![
                        f64::from(e.surf_param as i32),
                        e.iso_param,
                        e.orientation,
                        e.other_orientation,
                        u.min(),
                        v.min(),
                        u.max(),
                        v.max(),
                        e.target_distance,
                        e.approx_tol_3d,
                        e.guess_t,
                        e.uv.x,
                        e.uv.y,
                        e.other_uv.x,
                        e.other_uv.y,
                    ];
                    // Evaluate: the status, the out-parameters (also those a failure
                    // leaves) and the caller's sUV[idx] / sUV[1 - idx] (m_rUV, m_rOtherUV)
                    let evaluated = match radius {
                        Some(_) if !both => Some(vec![-2.0]),
                        Some(r) => {
                            let st = match e.evaluate(&k, *r) {
                                Ok(_) => 0.0,
                                Err(err @ SolidError::Unported { .. }) => return Err(err),
                                Err(err) => status(&err),
                            };
                            Some(vec![
                                st,
                                e.last_f_of_t,
                                e.last_f_prime,
                                b(e.last_found),
                                e.uv.x,
                                e.uv.y,
                                e.other_uv.x,
                                e.other_uv.y,
                                e.guess_t,
                            ])
                        }
                        None => None,
                    };
                    (fields, evaluated)
                };
                let key = |s: Option<&OffsetSurfaceSlot>| s.map_or(-1.0, |s| s.key as f64);
                o.push(key(fs.surface(*idx)?));
                o.push(key(fs.surface(1 - *idx)?));
                o.extend(fields);
                if let Some(ev) = evaluated {
                    let done = ev.len() > 1;
                    o.extend(ev);
                    if done {
                        for k in 0..2 {
                            o.push(fs.surface(k)?.map_or(0.0, |s| s.surface.offset_distance()));
                        }
                    }
                }
                Ok(())
            })();
            r
        }
        _ => return Err(harness("not a static op")),
    };
    Ok(res)
}

fn rust(k: &Kernel, i: &Inputs) -> Result<Output, HarnessError> {
    if matches!(i, Inputs::SplitRail) {
        return split_rail::rust();
    }
    let mut o = Vec::new();
    if i.static_op().is_some() {
        let res = if let Some(brep) = i.brep() {
            let mut c = match own_case(brep)? {
                Ok(c) => c,
                Err(out) => return Ok(out),
            };
            o.push(0.0);
            rust_brep(i, &mut c, &mut o)?
        } else {
            rust_static(i, &mut o)?
        };
        return Ok(match res {
            Ok(()) => Output::new().values("flat", o),
            Err(e) => Output::unported(&e).unwrap_or_else(|| {
                o.push(status(&e));
                Output::new().values("flat", o)
            }),
        });
    }
    let res: Result<(), SolidError> = match i {
        Inputs::State {
            tols,
            base,
            has,
            off,
            orient,
            cmds,
        } => rust_state(tols, base, *has, off, orient, cmds, &mut o)?,
        Inputs::Extension { ext, surface, dom } => {
            let k = Kernel::default();
            let s = surface_of(surface)?;
            let mut fs = FilletSolver::new(1.0e-6, 0.1, 0.01);
            fs.set_extension_factor(*ext);
            let mut d =
                Extent2d::from_points(Point2::new(dom[0], dom[1]), Point2::new(dom[2], dom[3]));
            let r = fs.setup_offset_extension(&k, &s, &mut d);
            if r.is_ok() {
                let (u, v) = (d.u_interval(), d.v_interval());
                o.extend_from_slice(&[0.0, u.min(), v.min(), u.max(), v.max()]);
            }
            r
        }
        Inputs::Sphere {
            c,
            r,
            adjusts,
            load,
            found,
            surfs,
            arrays,
        } => {
            let mut fs = shim_with(surfs, *load, *found, false)?;
            let mut f = arrays.f.clone();
            let mut j = jacobian_of(arrays)?;
            let mut e = FilletSphereSolveEnfo::new(Vec3::new(c[0], c[1], c[2]), *r, &mut fs);
            e.set_plane_adjusts(*adjusts);
            let res = e.evaluate(k, &arrays.x, &mut f, j.as_mut());
            match &res {
                Ok(found) => o.extend_from_slice(&[0.0, b(*found)]),
                Err(e) => {
                    if let Some(out) = Output::unported(e) {
                        return Ok(out);
                    }
                    o.extend_from_slice(&[status(e), 0.0]);
                }
            }
            put_arrays(&mut o, &f, j.as_ref());
            Ok(())
        }
        Inputs::RailUv {
            rail,
            load,
            found,
            curve,
            line,
            ivl,
            arrays,
        } => {
            let crv = match line {
                Some(l) => Curve::from(Line::from_points(
                    Point3::new(l[0], l[1], l[2]),
                    Point3::new(l[3], l[4], l[5]),
                    l[6] as u64,
                )),
                None => Curve::from(bspline_of(curve)?),
            };
            let mut fs = ShimSolver {
                base: FilletSolver::new(1.0e-6, 0.1, 0.01),
                load: *load,
                found: *found,
                setup: false,
            };
            let mut f = arrays.f.clone();
            let mut j = jacobian_of(arrays)?;
            let mut e = RailUvCurveIntersectEnfo::new(
                &crv,
                Extent1d::from_min_max(ivl[0], ivl[1]),
                *rail,
                &mut fs,
            );
            let res = e.evaluate(k, &arrays.x, &mut f, j.as_mut());
            match &res {
                Ok(found) => o.extend_from_slice(&[0.0, b(*found)]),
                Err(e) => {
                    if let Some(out) = Output::unported(e) {
                        return Ok(out);
                    }
                    o.extend_from_slice(&[status(e), 0.0]);
                }
            }
            put_arrays(&mut o, &f, j.as_ref());
            Ok(())
        }
        Inputs::SurfValues { setup, surfs, uv } => {
            let mut fs = shim_with(surfs, false, false, *setup)?;
            let mut t = TsectPnt::new();
            let r = FilletSolver::compute_surface_values(
                k,
                &mut fs as &mut dyn FilletSolverOps,
                [Point2::new(uv[0], uv[1]), Point2::new(uv[2], uv[3])],
                &mut t,
            );
            if r.is_ok() {
                o.push(0.0);
                // as the shim: per surface P, du, dv, unit normal, UV; curve point, angle
                for (pv, uvc) in t.surface_pv.iter().zip(&t.uv_curve_pv) {
                    for p in [pv[0][0], pv[1][0], pv[0][1], pv[2][2]] {
                        o.extend_from_slice(&[p.x, p.y, p.z]);
                    }
                    let uv = uvc[0];
                    o.extend_from_slice(&[uv.x, uv.y]);
                }
                let c = t.curve_pv[0];
                o.extend_from_slice(&[c.x, c.y, c.z, t.tangent_plane_angle_rad]);
            }
            r
        }
        Inputs::UvVectors { vecs, uvd } => {
            let v = |a: [f64; 3]| Vec3::new(a[0], a[1], a[2]);
            let mut t = TsectPnt::new();
            let [du0, dv0, du1, dv1, cd] = *vecs;
            let [a0, b0, a1, b1] = *uvd;
            t.surface_pv[0][1][0] = v(du0);
            t.surface_pv[0][0][1] = v(dv0);
            t.surface_pv[1][1][0] = v(du1);
            t.surface_pv[1][0][1] = v(dv1);
            t.uv_curve_pv[0][1] = Vec2::new(a0, b0);
            t.uv_curve_pv[1][1] = Vec2::new(a1, b1);
            t.curve_pv[1] = v(cd);
            let r = compute_uv_vectors(&mut t);
            o.push(match &r {
                Ok(()) => 0.0,
                Err(e) => status(e),
            });
            for uvp in &t.uv_curve_pv {
                o.push(uvp[1].x);
                o.push(uvp[1].y);
            }
            match r {
                Err(e @ SolidError::Unported { .. }) => Err(e),
                _ => Ok(()),
            }
        }
        Inputs::Split => {
            let k = Kernel::default();
            let mut pseudo = FilletBrep::new();
            let cc: CurveClassification<EntityRef> = CurveClassification::new(&k);
            let r = (|| -> Result<usize, SolidError> {
                let mut fs =
                    FilletSolver::new_in(&k, &mut pseudo, FilletSolverId(0), 1.0e-6, 0.1, 0.01)?;
                let target = Brep::new();
                fs.split_fillet_geom(
                    &k,
                    &mut pseudo,
                    &NoGeometry,
                    0,
                    0,
                    &[],
                    &[],
                    &cc,
                    &cc,
                    [&target, &target],
                )?;
                Ok(fs.fillet_geoms().len())
            })();
            r.map(|n| o.extend_from_slice(&[0.0, n as f64]))
        }
        Inputs::Finish { surface, has, off } => {
            let base = surface_of(surface)?;
            let k = Kernel::default();
            let mut pseudo = FilletBrep::new();
            let r = (|| -> Result<(), SolidError> {
                // the constructor's fillet geometry, before the derived constructor's
                // SetSurface calls (as the shim's ShimFS)
                let mut fs =
                    FilletSolver::new_in(&k, &mut pseudo, FilletSolverId(0), 1.0e-6, 0.1, 0.01)?;
                if has[0] {
                    let s =
                        OffsetSurface::new(&k, *off, base.clone(), SurfaceId::Free(0), SS_UNKNOWN)?;
                    fs.set_surface(0, Some(OffsetSurfaceSlot { key: 0, surface: s }))?;
                }
                if has[1] {
                    let s = OffsetSurface::new(
                        &k,
                        -*off,
                        base.clone(),
                        SurfaceId::Free(0),
                        SS_UNKNOWN,
                    )?;
                    fs.set_surface(1, Some(OffsetSurfaceSlot { key: 1, surface: s }))?;
                }
                fs.finish_construction(&k, &mut pseudo)?;
                // every geometry now holds a copy of each set offset surface (the shim
                // compares only the status; checked here against the solver's)
                for g in fs.fillet_geoms() {
                    let geom = pseudo.geom(g)?;
                    for (rail, has_rail) in has.iter().enumerate() {
                        let own = geom.off_surfs.get(rail).and_then(Option::as_ref);
                        let ours = fs.surface(rail)?;
                        if own.map(|s| s.surface.offset_distance())
                            != ours.map(|s| s.surface.offset_distance())
                            || own.is_some() != *has_rail
                        {
                            return Err(SolidError::invalid_input(
                                "difftest::blend_fillet_solver_p1",
                                "FinishConstruction did not install the offset copies",
                            ));
                        }
                    }
                }
                Ok(())
            })();
            if r.is_ok() {
                o.push(0.0);
            }
            r
        }
        Inputs::Fsg { kind, args, rails } => {
            let [a, bb, c, d] = *args;
            let g = match kind {
                0 => FilletSurfaceGenerator::Base,
                1 => FilletSurfaceGenerator::Linear,
                2 => FilletSurfaceGenerator::circular_default(),
                3 => FilletSurfaceGenerator::circular(a != 0.0, bb, c != 0.0, d != 0.0),
                4 => FilletSurfaceGenerator::blend_curve_default(),
                // (the shim's `(ULONG)b`)
                _ => FilletSurfaceGenerator::blend_curve(a, bb as u32),
            };
            o.push(f64::from(g.fillet_surface_generator_type() as i32));
            match g {
                FilletSurfaceGenerator::Circular(c) => o.extend([
                    b(c.approximate_arcs),
                    c.tol_percent_radius,
                    b(c.mirror),
                    b(c.complement),
                ]),
                _ => o.extend([0.0; 4]),
            }
            o.push(g.blend_scale().unwrap_or(0.0));
            o.push(g.continuity().map_or(0.0, f64::from));
            if *kind == 0 {
                let k = Kernel::default();
                let mut r1 = bspline_of(&line_curve())?;
                let mut r2 = r1.clone();
                let none = solidcore::blend::fillet_standard_solver::p1::RailSurface {
                    surface: None,
                    uv_curve: None,
                };
                o.push(
                    match g.create_surface(
                        &k,
                        1.0e-6,
                        FilletSolverType::Unknown,
                        None,
                        &mut r1,
                        &mut r2,
                        &[],
                        none,
                        none,
                    ) {
                        Ok(_) => 0.0,
                        Err(e) => status(&e),
                    },
                );
            } else if let (1, [c1, c2]) = (*kind, rails.as_slice()) {
                // the linear one: the ruled surface of two real rails, `status` then the
                // surface block (as the shim's `put_bss`; it reads only the rails)
                let k = Kernel::default();
                let mut r1 = bspline_of(c1)?;
                let mut r2 = bspline_of(c2)?;
                let none = solidcore::blend::fillet_standard_solver::p1::RailSurface {
                    surface: None,
                    uv_curve: None,
                };
                match g.create_surface(
                    &k,
                    1.0e-6,
                    FilletSolverType::Unknown,
                    None,
                    &mut r1,
                    &mut r2,
                    &[],
                    none,
                    none,
                ) {
                    Ok(s) => {
                        o.push(0.0);
                        put_bss(&mut o, &s);
                    }
                    Err(e) => {
                        o.push(status(&e));
                        o.push(0.0);
                    }
                }
            } else {
                o.push(-1.0);
            }
            Ok(())
        }
        Inputs::TolAng { has, ang } => {
            let fs = FilletSolver::new(1.0e-6, *ang, 0.01);
            o.push(tol_ang_tol_rad(has.then_some(&fs)));
            Ok(())
        }
        Inputs::InitialValues { do2, rail, idx2 } => {
            let k = Kernel::default();
            let mut fs = FilletSolver::new(1.0e-6, 0.1, 0.01);
            let plane = surface_of(&Surfs::default().base)?;
            let mut s1 = OffsetSurface::new(&k, 0.0, plane, SurfaceId::Free(0), SS_UNKNOWN)
                .map_err(|e| harness(e.to_string()))?;
            let mut ext = ExtentNd::new(3);
            let mut per = Vec::new();
            let mut guess = Vec::new();
            let r = fs.load_initial_values(
                &k,
                None,
                *do2 != 0,
                *rail,
                &mut s1,
                0,
                &mut ext,
                &mut per,
                &mut guess,
            );
            match r {
                Ok(_) => o.push(0.0),
                Err(e @ SolidError::Unported { .. }) => {
                    return Ok(Output::unported(&e).unwrap_or_default())
                }
                Err(e) => o.push(status(&e)),
            }
            o.extend([
                f64::from(*do2),
                f64::from(*idx2),
                1.0,
                per.len() as f64,
                guess.len() as f64,
            ]);
            Ok(())
        }
        _ => return Err(harness("static op in the main path")),
    };
    match res {
        Ok(()) => Ok(Output::new().values("flat", o)),
        Err(e) => {
            if let Some(out) = Output::unported(&e) {
                return Ok(out);
            }
            if o.is_empty() || matches!(i, Inputs::State { .. }) {
                o.push(status(&e));
            }
            Ok(Output::new().values("flat", o))
        }
    }
}

/// A straight degree-1 curve (the rails handed to the base `CreateSurface`).
fn line_curve() -> CrvIn {
    CrvIn {
        dim: 3,
        cpts: vec![[0.0, 0.0, 0.0, NOW], [1.0, 0.0, 0.0, NOW]],
        p: 1,
        u: vec![0.0, 0.0, 1.0, 1.0],
    }
}

/// The shim's solver: the base, or with `load` the deterministic `LoadJacobian` of
/// `ShimFS` (row `ne` = `x[surf1_offset] - 0.5 x[0] + rail + 0.25 surf2_offset` (the
/// incoming value) and its partials, written only inside `f` and the Jacobian;
/// `surf2_offset = np`; `ne += 1`, `np += 2`), with `setup` a `SetupOffsetValues` that
/// succeeds.
struct ShimSolver {
    base: FilletSolver,
    load: bool,
    found: bool,
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
    ) -> Result<(), SolidError> {
        if !self.setup {
            return self.base.setup_offset_values(tsect, offset_dist);
        }
        Ok(())
    }

    fn load_jacobian(
        &mut self,
        k: &Kernel,
        num_equations: &mut usize,
        num_parameters: &mut usize,
        load: &mut JacobianLoad<'_>,
        f: &mut [f64],
        jacobian: Option<&mut Matrix>,
    ) -> Result<bool, SolidError> {
        if !self.load {
            return self
                .base
                .load_jacobian(k, num_equations, num_parameters, load, f, jacobian);
        }
        let ne = *num_equations;
        let o1 = load.surf1_offset;
        if let (Some(slot), Some(&xo), Some(&x0)) = (f.get_mut(ne), load.x.get(o1), load.x.first())
        {
            *slot = xo - 0.5 * x0 + load.rail_index as f64 + 0.25 * load.surf2_offset as f64;
        }
        if let Some(j) = jacobian {
            if ne < j.num_rows() {
                if o1 < j.num_columns() {
                    j.set_at(ne, o1, 1.0)?;
                }
                if j.num_columns() > 0 {
                    j.set_at(ne, 0, -0.5)?;
                }
            }
        }
        load.surf2_offset = *num_parameters;
        *num_equations += 1;
        *num_parameters += 2;
        Ok(self.found)
    }
}

/// A classification handle (the shim classifies against a plane object).
#[derive(Debug, Clone, Copy, PartialEq)]
struct H;

impl ClassObject for H {
    fn class_kind(&self) -> ClassObjectKind {
        ClassObjectKind::Other
    }
}

/// No geometry behind the classification handles (`SplitFilletGeom` with empty
/// intervals classifies nothing).
struct NoGeometry;

impl<O> ClassGeometry<O> for NoGeometry {
    fn curve(&self, _o: O) -> Option<&Curve> {
        None
    }
    fn surface(&self, _o: O) -> Option<&Surface> {
        None
    }
    fn volume(&self, _o: O) -> Option<&Volume> {
        None
    }
    fn point(&self, _o: O) -> Option<Point3> {
        None
    }
}

/// A built analytic surface of [`Inputs::AnalyticRev`].
enum Rev {
    Cone(Cone),
    Sphere(Sphere),
    Torus(Torus),
}

// ---- generators ------------------------------------------------------------------------

/// A degree-2 rational full circle of radius `r` swept along z (degree 1): a cylinder
/// periodic in u, or in v when `transposed`.
fn cylinder(r: f64, h: f64, transposed: bool) -> SrfIn {
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let ring: [[f64; 3]; 9] = [
        [1.0, 0.0, 1.0],
        [1.0, 1.0, w],
        [0.0, 1.0, 1.0],
        [-1.0, 1.0, w],
        [-1.0, 0.0, 1.0],
        [-1.0, -1.0, w],
        [0.0, -1.0, 1.0],
        [1.0, -1.0, w],
        [1.0, 0.0, 1.0],
    ];
    let circle_knots = vec![
        0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0,
    ];
    let line_knots = vec![0.0, 0.0, 1.0, 1.0];
    let pt = |&[x, y, wt]: &[f64; 3], z: f64| [x * r * wt, y * r * wt, z * wt, wt];
    if transposed {
        let net = [0.0, h]
            .iter()
            .map(|&z| ring.iter().map(|q| pt(q, z)).collect())
            .collect();
        return SrfIn::Bspline(BssIn {
            oob: false,
            form: 0,
            net,
            p: 1,
            q: 2,
            u: line_knots,
            v: circle_knots,
        });
    }
    let net = ring.iter().map(|q| vec![pt(q, 0.0), pt(q, h)]).collect();
    SrfIn::Bspline(BssIn {
        oob: false,
        form: 0,
        net,
        p: 2,
        q: 1,
        u: circle_knots,
        v: line_knots,
    })
}

/// A B-spline with one or two boundaries collapsed to a point (`sides`: bits umin,
/// umax, vmin, vmax).
fn pinched(rng: &mut Rng, sides: u8) -> SrfIn {
    let mut s = gen_surface(rng);
    while matches!(s, SrfIn::Plane { .. }) {
        s = gen_surface(rng);
    }
    if let SrfIn::Bspline(bs) = &mut s {
        let n = bs.net.len();
        let m = bs.net.first().map_or(0, Vec::len);
        let apex = |rng: &mut Rng| {
            let p = rng.point3(-1.0, 1.0);
            [p[0], p[1], p[2], NOW]
        };
        if sides & 1 != 0 {
            let a = apex(rng);
            if let Some(row) = bs.net.first_mut() {
                row.fill(a);
            }
        }
        if sides & 2 != 0 && n > 0 {
            let a = apex(rng);
            if let Some(row) = bs.net.last_mut() {
                row.fill(a);
            }
        }
        if sides & 4 != 0 && m > 0 {
            let a = apex(rng);
            bs.net
                .iter_mut()
                .filter_map(|row| row.first_mut())
                .for_each(|q| *q = a);
        }
        if sides & 8 != 0 && m > 0 {
            let a = apex(rng);
            bs.net
                .iter_mut()
                .filter_map(|row| row.last_mut())
                .for_each(|q| *q = a);
        }
        bs.oob = false;
    }
    s
}

fn gen_srf(rng: &mut Rng) -> SrfIn {
    if rng.chance(0.25) {
        let scale = *rng.choose(&[1e-4, 1.0, 1.0, 1e4]).unwrap_or(&1.0);
        return cylinder(
            scale * rng.uniform(0.5, 2.0),
            scale * rng.uniform(0.5, 3.0),
            rng.chance(0.5),
        );
    }
    if rng.chance(0.3) {
        let sides = *rng
            .choose(&[1u8, 2, 4, 8, 1 | 2, 4 | 8, 1 | 4, 2 | 8])
            .unwrap_or(&1);
        return pinched(rng, sides);
    }
    gen_surface(rng)
}

fn gen_arrays(
    rng: &mut Rng,
    min_x: usize,
    min_f: usize,
    min_rows: usize,
    min_cols: usize,
) -> Arrays {
    let nx = min_x + rng.below(4) as usize;
    let x = (0..nx).map(|_| rng.uniform(-1.0, 2.0)).collect();
    let jac = rng.chance(0.6).then(|| {
        let rows = min_rows + rng.below(4) as usize;
        let cols = min_cols + rng.below(4) as usize;
        [rows, cols]
    });
    let nf = jac.map_or(min_f, |[r, _]| r.max(min_f)) + rng.below(3) as usize;
    let f = (0..nf).map(|_| rng.uniform(-5.0, 5.0)).collect();
    Arrays { x, f, jac }
}

fn gen_surfs(rng: &mut Rng) -> Surfs {
    Surfs {
        base: gen_surface(rng),
        has: [rng.chance(0.8), rng.chance(0.8)],
        off: [rng.uniform(-2.0, 2.0), rng.uniform(-2.0, 2.0)],
    }
}

fn gen_state(rng: &mut Rng) -> Inputs {
    let n = rng.below(12) as usize;
    let cmds = (0..n)
        .map(|_| match rng.below(10) {
            0 | 1 => [0.0, 0.0, 0.0],
            2 => [1.0, 0.0, 0.0],
            3 | 4 => [2.0, 0.0, 0.0],
            5 | 6 => [3.0, 0.0, 0.0],
            7 => [4.0, rng.below(2) as f64, rng.uniform(-3.0, 3.0)],
            8 => [
                5.0 + rng.below(2) as f64,
                rng.uniform(0.0, 2.0).round(),
                0.0,
            ],
            _ => [7.0, rng.below(13) as f64, 0.0],
        })
        .collect();
    let o = |rng: &mut Rng| if rng.chance(0.5) { 1.0 } else { -1.0 };
    Inputs::State {
        tols: [
            rng.uniform(1e-8, 1e-3),
            rng.uniform(0.01, 0.5),
            rng.uniform(0.001, 0.1),
        ],
        base: [gen_surface(rng), gen_surface(rng)],
        has: [rng.chance(0.8), rng.chance(0.8)],
        off: [rng.uniform(-2.0, 2.0), rng.uniform(-2.0, 2.0)],
        orient: [o(rng), o(rng)],
        cmds,
    }
}

fn gen_extension(rng: &mut Rng) -> Inputs {
    let surface = gen_srf(rng);
    let [u0, v0, u1, v1] = match &surface {
        SrfIn::Plane { .. } => {
            let s = *rng.choose(&[1e-6, 1.0, 1e6]).unwrap_or(&1.0);
            let a = rng.uniform(-1.0, 1.0) * s;
            let c = rng.uniform(-1.0, 1.0) * s;
            [
                a,
                c,
                a + rng.uniform(0.0, 2.0) * s,
                c + rng.uniform(0.0, 2.0) * s,
            ]
        }
        s @ SrfIn::Bspline(_) => {
            let d = domain(s);
            if rng.chance(0.7) {
                d
            } else {
                // a sub-domain
                let t = |rng: &mut Rng, a: f64, b: f64| a + (b - a) * rng.uniform(0.0, 1.0);
                let (ua, ub) = (t(rng, d[0], d[2]), t(rng, d[0], d[2]));
                let (va, vb) = (t(rng, d[1], d[3]), t(rng, d[1], d[3]));
                [ua.min(ub), va.min(vb), ua.max(ub), va.max(vb)]
            }
        }
    };
    let ext = *rng
        .choose(&[0.5, 0.5, 0.0, 0.1, 2.0, -0.3, 1e-12, 1e-6, 1e6, 1e12])
        .unwrap_or(&0.5);
    Inputs::Extension {
        ext,
        surface,
        dom: [u0, v0, u1, v1],
    }
}

fn gen_sphere(rng: &mut Rng) -> Inputs {
    let adjusts = rng.chance(0.5);
    let load = rng.chance(0.6);
    let mut surfs = gen_surfs(rng);
    let mut arrays = if load {
        gen_arrays(rng, 2, 1, 1, 2)
    } else {
        gen_arrays(rng, 0, 0, 0, 0)
    };
    if load {
        // a loading solver dereferences its surfaces and reads crX (the FFI guard)
        surfs.has = [true, adjusts || rng.chance(0.5)];
        while arrays.x.len() < if adjusts { 4 } else { 2 } {
            arrays.x.push(rng.uniform(0.0, 1.0));
        }
    }
    Inputs::Sphere {
        c: rng.point3(-2.0, 2.0),
        r: rng.uniform(-2.0, 2.0),
        adjusts,
        load,
        found: rng.chance(0.5),
        surfs,
        arrays,
    }
}

/// The two rails of a linear `fsg` case (`SmLinearCrossSectionFSG::CreateSurface`):
/// real 3D B-spline curves, either a rail and a copy of it moved by an offset (same
/// degree, knots and domain: the ruled surface needs no reparametrisation), or two
/// independent curves (different degrees, knot vectors, domains, rational or not:
/// `CreateRuledSurface` makes them compatible, or fails).
fn gen_fsg_rails(rng: &mut Rng) -> Vec<CrvIn> {
    let scale = *rng.choose(&[1e-3, 1.0, 1.0, 1e3]).unwrap_or(&1.0);
    let first = gen_curve(rng, 3, scale);
    let second = if rng.chance(0.5) {
        let d = [
            rng.uniform(-1.0, 1.0) * scale,
            rng.uniform(-1.0, 1.0) * scale,
            rng.uniform(0.5, 2.0) * scale,
        ];
        let mut c = first.clone();
        for p in &mut c.cpts {
            let w = if p[3] == NOW { 1.0 } else { p[3] };
            p[0] += d[0] * w;
            p[1] += d[1] * w;
            p[2] += d[2] * w;
        }
        c
    } else {
        gen_curve(rng, 3, scale)
    };
    vec![first, second]
}

fn gen_rail_uv(rng: &mut Rng) -> Inputs {
    let scale = *rng.choose(&[1e-3, 1.0, 1e3]).unwrap_or(&1.0);
    let dim = 2 + rng.below(2);
    let curve = gen_curve(rng, dim, scale);
    let a = curve.u.first().copied().unwrap_or(0.0);
    let z = curve.u.last().copied().unwrap_or(1.0);
    let mut arrays = gen_arrays(rng, 3, 2, 2, 3);
    let load = rng.chance(0.7);
    if let Some(x0) = arrays.x.first_mut() {
        *x0 = a + (z - a) * rng.uniform(-0.2, 1.2);
    }
    // half the loading cases start on the curve's first point (a clamped
    // non-rational curve passes through it): the converged branch
    if let (true, Some([px, py, _, w])) = (load && rng.chance(0.5), curve.cpts.first().copied()) {
        if w == NOW {
            if let [x0, x1, x2, ..] = arrays.x.as_mut_slice() {
                *x0 = a;
                *x1 = px;
                *x2 = py;
            }
        }
    }
    // a third of the cases use an SmLine UV curve (natural interval [0, 1], including
    // degenerate zero-length lines), whose virtual Evaluate is analytic
    let line = rng.chance(0.35).then(|| {
        let mut p = [0.0; 7];
        for v in p.iter_mut().take(6) {
            *v = scale * rng.uniform(-2.0, 2.0);
        }
        if rng.chance(0.1) {
            p.copy_within(0..3, 3);
        }
        p[6] = dim as f64;
        p
    });
    if line.is_some() {
        if let Some(x0) = arrays.x.first_mut() {
            *x0 = rng.uniform(-0.2, 1.2);
        }
    }
    Inputs::RailUv {
        load,
        found: rng.chance(0.6),
        rail: rng.below(2) as usize,
        curve,
        line,
        ivl: if line.is_some() { [0.0, 1.0] } else { [a, z] },
        arrays,
    }
}

/// A B-rep for the static B-rep ops: mostly upstream test files.
fn gen_brep_case(rng: &mut Rng) -> BrepIn {
    if rng.chance(0.6) {
        return BrepIn::File {
            file: (*rng.choose(&FILES).unwrap_or(&"SmallBox.smp")).to_string(),
            rebuild: false,
        };
    }
    gen_brep(rng)
}

/// A cone, cylinder, sphere or torus at a random placement (axes possibly unnormalised
/// and not perpendicular), at scales from `1e-3` to `1e3`.
fn gen_analytic_rev(rng: &mut Rng) -> Inputs {
    let kind = rng.below(4) as u8;
    let scale = *rng.choose(&[1e-3, 1.0, 1.0, 1.0, 1e3]).unwrap_or(&1.0);
    let origin = rng.point3(-2.0, 2.0).map(|c| c * scale);
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let mut x = rng.point3(-1.0, 1.0);
    if dot(x, x) < 0.04 {
        x = [1.0, 0.0, 0.0];
    }
    let mut y = rng.point3(-1.0, 1.0);
    let c = [
        x[1] * y[2] - x[2] * y[1],
        x[2] * y[0] - x[0] * y[2],
        x[0] * y[1] - x[1] * y[0],
    ];
    if dot(y, y) < 0.04 || dot(c, c) < 0.01 * dot(x, x) * dot(y, y) {
        // far from parallel
        y = if x[0].abs() > 0.5 {
            [-x[1], x[0], 0.0]
        } else {
            [0.0, -x[2], x[1]]
        };
        if dot(y, y) < 0.04 {
            y = [x[2], 0.0, -x[0]];
        }
    }
    if rng.chance(0.5) {
        // perpendicular axes
        let d = dot(x, y) / dot(x, x);
        y = [y[0] - d * x[0], y[1] - d * x[1], y[2] - d * x[2]];
    }
    let r = |rng: &mut Rng| rng.uniform(0.1, 2.0) * scale;
    let p = match kind {
        // bot top height (equal radii: a cone of semi-angle 0, still SmCone_TYPE)
        0 => {
            let bot = r(rng);
            let top = if rng.chance(0.2) { bot } else { r(rng) };
            [bot, top, r(rng), 0.0]
        }
        // radius start end height
        1 => {
            let start = rng.uniform(-360.0, 0.0);
            [r(rng), start, start + rng.uniform(10.0, 360.0), r(rng)]
        }
        2 => [r(rng), 0.0, 0.0, 0.0],
        // major minor (a spindle torus when minor > major)
        _ => [r(rng), r(rng), 0.0, 0.0],
    };
    Inputs::AnalyticRev {
        kind,
        origin,
        x,
        y,
        p,
    }
}

/// `[this, other]` stored at `[idx, 1 - idx]`.
fn by_slot<T>(idx: usize, this: T, other: T) -> [T; 2] {
    if idx == 0 {
        [this, other]
    } else {
        [other, this]
    }
}

/// A B-spline patch of `gen_surface` (a bilinear one when it keeps making planes).
fn gen_patch(rng: &mut Rng) -> BssIn {
    for _ in 0..8 {
        if let SrfIn::Bspline(bs) = gen_surface(rng) {
            return bs;
        }
    }
    BssIn {
        oob: false,
        form: 0,
        net: vec![
            vec![[0.0, 0.0, 0.0, NOW], [0.0, 1.0, 0.1, NOW]],
            vec![[1.0, 0.0, -0.1, NOW], [1.0, 1.0, 0.0, NOW]],
        ],
        p: 1,
        q: 1,
        u: vec![0.0, 0.0, 1.0, 1.0],
        v: vec![0.0, 0.0, 1.0, 1.0],
    }
}

/// `SmFindClippedRadiusEFO` cases. Kinds: the constructor's members on offsets of the
/// `z = 0` plane (parallel offsets: `Evaluate` fails at the first `LocalCurveIntersect`,
/// or meets a coincident offset); the `z = 0` plane against a tilted plane through
/// the point the iso line `iso = 0` meets at `radius` (a wide other domain: both
/// intersections; a far one: the first fails; a point or quadrant at the solution: the
/// second, after the radius step, fails; the target distance the rail distance: found);
/// a B-spline patch against a tilted plane through its middle, either surface carrying
/// the iso curve, iso parameters inside and outside the patch's domain (its
/// out-of-bounds base evaluation). `SM_SP_U` and `SM_SP_V` throughout.
fn gen_clipped(rng: &mut Rng) -> Inputs {
    let o = |rng: &mut Rng| if rng.chance(0.5) { 1.0 } else { -1.0 };
    let idx = rng.below(2) as usize;
    let unit = |n: [f64; 3]| {
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        n.map(|c| c / l)
    };
    let tilted = |rng: &mut Rng| {
        [
            1.0 + rng.uniform(-0.3, 0.3),
            2.0 + rng.uniform(-0.3, 0.3),
            3.0 + rng.uniform(-0.3, 0.3),
        ]
    };
    let cat = |[a, b]: [[f64; 4]; 2]| [a[0], a[1], a[2], a[3], b[0], b[1], b[2], b[3]];
    let cat2 = |[a, b]: [[f64; 2]; 2]| [a[0], a[1], b[0], b[1]];
    let param = rng.below(2) as u8;
    match rng.below(10) {
        // offsets of the z = 0 plane, everything random
        0 | 1 => {
            let mut doms = [0.0; 8];
            for d in &mut doms {
                *d = rng.uniform(-2.0, 2.0);
            }
            Inputs::Clipped {
                idx,
                dist: rng.uniform(0.01, 3.0),
                tol: rng.uniform(1e-8, 1e-3),
                has: [rng.chance(0.8), rng.chance(0.8)],
                off: [rng.uniform(-2.0, 2.0), rng.uniform(-2.0, 2.0)],
                orient: [o(rng), o(rng)],
                doms,
                param: rng.below(5) as u8,
                iso: rng.uniform(-1.0, 2.0),
                uv: [
                    rng.uniform(-1.0, 2.0),
                    rng.uniform(-1.0, 2.0),
                    rng.uniform(-1.0, 2.0),
                    rng.uniform(-1.0, 2.0),
                ],
                bases: z_planes(),
                radius: rng.chance(0.8).then(|| rng.uniform(0.0, 2.0)),
            }
        }
        // the z = 0 plane against a tilted plane through the solution at `radius`
        2..=5 => {
            let r0 = *rng.choose(&[0.5, 1.0, 1.0, 2.0, 1e-3, 1e3]).unwrap_or(&1.0);
            let (oi, oo) = (o(rng), o(rng));
            let orient = by_slot(idx, oi, oo);
            let n = unit(tilted(rng));
            // the iso line iso = 0 of the offset plane z = r0 * oi passes through p0;
            // the other offset plane's (0, 0) is p0
            let p0 = [0.0, 0.0, r0 * oi];
            let origin = [
                p0[0] - r0 * oo * n[0],
                p0[1] - r0 * oo * n[1],
                p0[2] - r0 * oo * n[2],
            ];
            let w = 10.0 * r0.max(1.0);
            let other_dom = match rng.below(8) {
                0 | 1 => [-w, -w, w, w],
                2 => [5.0 * w, 5.0 * w, 6.0 * w, 6.0 * w],
                3 => [0.0, 0.0, 0.0, 0.0],
                4 => [0.0, 0.0, w, w],
                5 => [-w, 0.0, 0.0, w],
                6 => [0.0, -w, w, 0.0],
                _ => [-w, -w, 0.0, 0.0],
            };
            let doms = cat(by_slot(idx, [-w, -w, w, w], other_dom));
            // the rail distance at the solution: base points (0, 0) and the origin
            let rail =
                (origin[0] * origin[0] + origin[1] * origin[1] + origin[2] * origin[2]).sqrt();
            let bases = by_slot(idx, z_plane(), SrfIn::Plane { o: origin, n });
            let mut g = || rng.uniform(-0.5, 0.5) * r0;
            let uv = cat2([[g(), g()], [g(), g()]]);
            Inputs::Clipped {
                idx,
                dist: if rng.chance(0.3) {
                    rail
                } else {
                    rng.uniform(0.01, 3.0) * r0
                },
                tol: *rng.choose(&[1e-6, 1e-7, 1e-8]).unwrap_or(&1e-6),
                has: [true, true],
                off: [rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)],
                orient,
                doms,
                param,
                iso: 0.0,
                uv,
                bases,
                radius: Some(r0),
            }
        }
        // a B-spline patch against a tilted plane through its middle
        _ => {
            let mut patch = gen_patch(rng);
            patch.oob = rng.chance(0.3);
            let pts: Vec<[f64; 4]> = patch.net.iter().flatten().copied().collect();
            let count = pts.len().max(1) as f64;
            let mut c = [0.0; 3];
            let mut size: f64 = 0.0;
            for q in &pts {
                for (ck, qk) in c.iter_mut().zip(q) {
                    *ck += qk / count;
                }
                size = size.max(q[0].abs()).max(q[1].abs());
            }
            let size = size.max(1e-6);
            let n = [
                1.0 + rng.uniform(-0.3, 0.3),
                1.0 + rng.uniform(-0.3, 0.3),
                0.5 + rng.uniform(-0.3, 0.3),
            ];
            let plane = SrfIn::Plane { o: c, n };
            let patch_dom = domain(&SrfIn::Bspline(patch.clone()));
            let w = 10.0 * size;
            let plane_dom = [-w, -w, w, w];
            // which surface carries the iso curve
            let iso_on_patch = rng.chance(0.6);
            let (iso_srf, other_srf, iso_dom, other_dom) = if iso_on_patch {
                (SrfIn::Bspline(patch), plane, patch_dom, plane_dom)
            } else {
                (plane, SrfIn::Bspline(patch), plane_dom, patch_dom)
            };
            let iso = if iso_on_patch {
                let (a, b) = if param == 0 {
                    (patch_dom[0], patch_dom[2])
                } else {
                    (patch_dom[1], patch_dom[3])
                };
                let f = if rng.chance(0.2) {
                    *rng.choose(&[-0.2, -0.05, 1.05, 1.2]).unwrap_or(&1.1)
                } else {
                    rng.uniform(0.2, 0.8)
                };
                a + (b - a) * f
            } else {
                0.0
            };
            let bases = by_slot(idx, iso_srf, other_srf);
            let doms = cat(by_slot(idx, iso_dom, other_dom));
            // near the middle of the patch's domain, the plane's origin
            let guess = |d: [f64; 4], rng: &mut Rng| {
                [
                    d[0] + (d[2] - d[0]) * (0.5 + rng.uniform(-0.1, 0.1)),
                    d[1] + (d[3] - d[1]) * (0.5 + rng.uniform(-0.1, 0.1)),
                ]
            };
            let gi = if iso_on_patch {
                guess(iso_dom, rng)
            } else {
                [0.0, 0.0]
            };
            let go = if iso_on_patch {
                [0.0, 0.0]
            } else {
                guess(other_dom, rng)
            };
            let uv = cat2(by_slot(idx, gi, go));
            Inputs::Clipped {
                idx,
                dist: rng.uniform(0.01, 2.0) * size,
                tol: *rng.choose(&[1e-6, 1e-7, 1e-5]).unwrap_or(&1e-6),
                has: [true, true],
                off: [0.0, 0.0],
                orient: [o(rng), o(rng)],
                doms,
                param,
                iso,
                uv,
                bases,
                radius: Some(rng.uniform(0.01, 0.2) * size),
            }
        }
    }
}

fn gen_static(rng: &mut Rng) -> Inputs {
    let s = |rng: &mut Rng| rng.below(64) as i64;
    match rng.below(10) {
        0 => {
            let n = rng.below(6) as usize;
            let cls = (0..n)
                .map(|_| {
                    *rng.choose(&[100u32, 101, 102, 102, 103, 103, 104, 106, 109, 110])
                        .unwrap_or(&100)
                })
                .collect();
            Inputs::FindNext {
                start: rng.below(n as u64 + 2) as usize,
                cls,
            }
        }
        1 => Inputs::CheckDomains {
            brep: gen_brep(rng),
            edge: s(rng),
            before: rng.chance(0.5),
            after: rng.chance(0.5),
        },
        2 => {
            let mut surface = gen_surface(rng);
            if rng.chance(0.5) {
                if let SrfIn::Bspline(bs) = &mut surface {
                    // planar (z constant): the IsPlanar branch
                    let z = rng.uniform(-1.0, 1.0);
                    bs.net.iter_mut().flatten().for_each(|q| q[2] = z);
                }
            }
            if rng.chance(0.5) {
                gen_analytic_rev(rng)
            } else {
                Inputs::Analytic { surface }
            }
        }
        3 => {
            let mut arrays = gen_arrays(rng, 1, 2, 2, 1);
            if rng.chance(0.2) {
                arrays.x.clear();
            }
            Inputs::RailRail {
                rail: rng.below(2) as usize,
                other: rng.below(2) as usize,
                load: [rng.chance(0.8), rng.chance(0.8)],
                found: [rng.chance(0.6), rng.chance(0.6)],
                arrays,
            }
        }
        4 => gen_clipped(rng),
        5 => Inputs::Convexity {
            brep: BrepIn::File {
                file: (*rng.choose(&FILES).unwrap_or(&"SmallBox.smp")).to_string(),
                rebuild: false,
            },
            edge: if rng.chance(0.6) { -1 } else { s(rng) },
            tangential: rng.chance(0.5),
        },
        6 => {
            let a = rng.uniform(-0.1, 0.5);
            Inputs::TrimRail {
                brep: gen_brep_case(rng),
                edge: s(rng),
                t: [a, a + rng.uniform(0.05, 0.7)],
            }
        }
        // tolerances from tight to past a right angle (a negative cosine limit)
        8 => Inputs::Rollover {
            brep: gen_brep_case(rng),
            edge: s(rng),
            tol: *rng
                .choose(&[1e-6, 1e-3, 0.01, 0.1, 0.5, 1.0, 2.0])
                .unwrap_or(&0.01),
        },
        _ => Inputs::Queries {
            brep: gen_brep_case(rng),
            eu: [
                if rng.chance(0.1) { -1 } else { s(rng) },
                if rng.chance(0.2) { -1 } else { s(rng) },
            ],
            face: if rng.chance(0.1) { -1 } else { s(rng) },
            side: s(rng),
        },
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    match rng.below(40) {
        36..=39 => gen_clipped(rng),
        32..=35 => gen_analytic_rev(rng),
        0..=4 => gen_state(rng),
        5..=10 => gen_extension(rng),
        11 => gen_sphere(rng),
        12..=14 => gen_rail_uv(rng),
        15 => {
            let setup = rng.chance(0.5);
            let mut surfs = gen_surfs(rng);
            if setup {
                surfs.has = [true, true];
            }
            Inputs::SurfValues {
                setup,
                surfs,
                uv: [
                    rng.uniform(0.0, 1.0),
                    rng.uniform(0.0, 1.0),
                    rng.uniform(0.0, 1.0),
                    rng.uniform(0.0, 1.0),
                ],
            }
        }
        16 | 17 => {
            let mut vecs = [[0.0; 3]; 5];
            for v in &mut vecs {
                *v = rng.point3(-2.0, 2.0);
            }
            match rng.below(5) {
                0 => vecs[0] = [0.0, 0.0, 0.0],
                1 => vecs[1] = vecs[0].map(|c| c * 2.0),
                2 => vecs[2] = vecs[2].map(|c| c * 1e-5),
                _ => {}
            }
            Inputs::UvVectors {
                vecs,
                uvd: [
                    rng.uniform(-1.0, 1.0),
                    rng.uniform(-1.0, 1.0),
                    rng.uniform(-1.0, 1.0),
                    rng.uniform(-1.0, 1.0),
                ],
            }
        }
        18 => Inputs::Split,
        19 => Inputs::Finish {
            surface: gen_surface(rng),
            has: [rng.chance(0.5), rng.chance(0.5)],
            off: rng.uniform(-1.0, 1.0),
        },
        20 => {
            // half the cases are the linear kind (the one with a real `CreateSurface`)
            let kind = if rng.chance(0.5) {
                1
            } else {
                rng.below(6) as u8
            };
            let args = if kind == 5 {
                [rng.uniform(0.1, 3.0), rng.below(4) as f64, 0.0, 0.0]
            } else {
                [
                    b(rng.chance(0.5)),
                    rng.uniform(0.0, 0.2),
                    b(rng.chance(0.5)),
                    b(rng.chance(0.5)),
                ]
            };
            // the linear kind (1) is given two rails for `CreateSurface` in 3 of 4 cases
            let rails = if kind == 1 && rng.chance(0.75) {
                gen_fsg_rails(rng)
            } else {
                Vec::new()
            };
            Inputs::Fsg { kind, args, rails }
        }
        21 => {
            if rng.chance(0.5) {
                Inputs::TolAng {
                    has: rng.chance(0.7),
                    ang: rng.uniform(1e-6, 0.5),
                }
            } else {
                Inputs::InitialValues {
                    do2: rng.below(2) as u32,
                    rail: rng.below(2) as usize,
                    idx2: rng.below(5) as u32,
                }
            }
        }
        _ => gen_static(rng),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "blend.fillet_solver_p1"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_case(rng)).ok()
    }

    fn cases(&self, ctx: &crate::case::CaseContext) -> Result<Vec<crate::Case>, HarnessError> {
        let ctx = crate::case::CaseContext {
            seeds: ctx.seeds.max(320),
            ..ctx.clone()
        };
        crate::runner::default_cases(self, &ctx)
    }

    fn run_cpp(&self, case: &crate::Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &crate::Case) -> Result<Output, HarnessError> {
        let k = &Kernel::default();
        rust(k, &case.parse()?)
    }
}
