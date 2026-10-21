//! Runner for unit `blend.fillet_standard_solver_p2`: `SmConstantRadiusFS::LoadInitialValues`
//! (on an `SmConstantRadiusFS` and an `SmSurfaceSurfaceFS`), the
//! `SmConstantRadiusAssistedFS` `GuessPointOnPlaneSolve` (with `SmGuessSphereSolveENFO`)
//! and `PointOnPlaneSolve`, the `SmSurfaceSurfaceFS` constructor and `CalcFilletGeom`'s
//! first exit, and the `IsKindOf` of the assisted, surface/surface and variable-radius
//! solvers.
//!
//! C++: `solidcore_ffi::units::blend_fillet_standard_solver_p2` (upstream
//! `source/SMLib/src/SmFilletStandardSolver.cpp`). Rust:
//! `solidcore::blend::fillet_standard_solver::p2`.
//!
//! A case is one op with its inputs. Both sides produce the same flat record (layout in
//! the FFI module docs), compared as `status` (`0` = `SM_SUCCESS`, else the `SmStatus`:
//! `1001` `SM_ERR`, `1007` `SM_ERR_INVALID_INPUT`; `-1` on the Rust side for an error
//! upstream cannot produce) and `out` (the rest).
//!
//! The solvers are built over real `SmPlane`s and B-spline surfaces (open patches,
//! cylinder-like closed ones, ones with a pole) as the shim builds them; the offset
//! surfaces are the ported `SmOffsetSurface` with upstream's default `SM_SS_UNKNOWN`.
//! `LoadInitialValues` on an `SmConstantRadiusFS` and `PointOnPlaneSolve` reach the base
//! `SmFilletSolver::SetupOffsetExtension` / `SmFilletSphereSolveENFO::Evaluate` (unit
//! `blend.fillet_solver_p1`) and are deferred; on an `SmSurfaceSurfaceFS` they use its
//! own (ported) extension and compare real values. The plane solves run on two planes
//! meeting at an edge (rolling-ball corner, randomly placed, with the guesses near or far
//! from the answer) and on random surfaces.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::blend::fillet_custom_solver::{OffsetSurface, SurfaceId};
use solidcore::blend::fillet_standard_solver::p1::{
    constant_radius_from_edgeuse, ConstantDistanceFs, ConstantRadiusFs, CurveBasedFs, FilletLaw,
    FilletLawKind, SurfaceSurfaceFs, VariableRadiusFs, CONSTANT_RADIUS_ASSISTED_FS_TYPE,
};
use solidcore::blend::fillet_standard_solver::p2::{
    constant_radius_assisted_is_kind_of, point_on_plane_solve,
};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::{Extent1d, ExtentNd};
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::surface_intersector::p1::{SrfIndex, TsectPnt};
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::p1::NurbCircleParam;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::knots::Side;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::offset_surface::SS_UNKNOWN;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::{Brep, EdgeuseId};
use solidcore::SolidError;
use solidcore_ffi::units::blend_fillet_standard_solver_p2::{self as ffi, Op};
use solidcore_ffi::units::topology_edgeuse_p1 as ffi1;

use super::topology_edgeuse_p1::{
    brep_block_of, gen_brep, import_with_types, path_of, sel, BrepIn, FILES,
};
use super::topology_edgeuse_p2::{inexact_geometry, put_eu_state, Setup};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 400;

/// `blend.fillet_standard_solver_p2` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

fn he(e: &SolidError) -> HarnessError {
    harness(e.to_string())
}

const DEG: f64 = std::f64::consts::PI / 180.0;

/// `CalcFilletGeom` not run by the shim (past its parallel-normal exit).
const NOT_RUN: f64 = -3.0;

// ---- inputs -----------------------------------------------------------------------

/// A surface: a B-spline (control net as stored, rows in u) or a plane.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SrfIn {
    Bspline {
        net: Vec<Vec<[f64; 4]>>,
        p: usize,
        q: usize,
        u: Vec<f64>,
        v: Vec<f64>,
    },
    Plane {
        o: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        scale: [f64; 2],
        /// `[u0, v0, u1, v1]`.
        domain: [f64; 4],
    },
}

/// The solver block: `cls` 0 `SmConstantRadiusFS`, 1 `SmSurfaceSurfaceFS`, 2
/// `SmConstantRadiusAssistedFS`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SolverIn {
    cls: usize,
    a: SrfIn,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    b: Option<SrfIn>,
    r: [f64; 2],
    o: [bool; 2],
    ext: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlaneIn {
    orig: [f64; 3],
    normal: [f64; 3],
    d1: [f64; 4],
    d2: [f64; 4],
    uv1: [f64; 2],
    uv2: [f64; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    CrInit {
        solver: SolverIn,
        do2: bool,
        rail: usize,
        sel1: usize,
        s1_index: usize,
        intervals: Vec<[f64; 2]>,
        per: Vec<bool>,
        guess: Vec<f64>,
    },
    Guess {
        solver: SolverIn,
        plane: PlaneIn,
    },
    Plane {
        solver: SolverIn,
        plane: PlaneIn,
    },
    Ss {
        a: SrfIn,
        b: SrfIn,
        r: [f64; 2],
        o: [bool; 2],
        g: [[f64; 2]; 2],
        calc: bool,
        kinds: Vec<i64>,
    },
    Types {
        kinds: Vec<i64>,
    },
    /// The `SmConstantDistanceFS` edge-use constructor on a `topology.edgeuse_p1` B-rep.
    CdEu {
        brep: BrepIn,
        /// Edge use selector (modulo the edge use count).
        eu: i64,
        tol: f64,
        ang: f64,
        tan: f64,
        dist: f64,
        /// UV fractions of the offset surfaces' natural domains where they are evaluated.
        fu: f64,
        fv: f64,
    },
    /// `SmConstantRadiusFS::LoadInitialValues` with edge uses: the `SmConstantRadiusFS`
    /// edge-use constructor on a `topology.edgeuse_p1` B-rep, then `LoadInitialValues`
    /// from `*GetSurface(rail)`.
    CrEu {
        brep: BrepIn,
        /// Edge use selector (modulo the edge use count).
        eu: i64,
        tol: f64,
        ang: f64,
        tan: f64,
        r: f64,
        rail: usize,
        /// UV fractions of the rail offset's natural domain: the guess.
        fu: f64,
        fv: f64,
        /// `m_dSurfaceExtensionFactor`.
        ext: f64,
    },
}

impl Inputs {
    /// The shim op (`None` for the edge-use constructor, which has its own entry point).
    fn ffi(&self) -> Option<Op> {
        Some(match self {
            Inputs::CrInit { .. } => Op::CrInit,
            Inputs::Guess { .. } => Op::Guess,
            Inputs::Plane { .. } => Op::Plane,
            Inputs::Ss { .. } => Op::Ss,
            Inputs::Types { .. } => Op::Types,
            Inputs::CdEu { .. } | Inputs::CrEu { .. } => return None,
        })
    }
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

// ---- C++ side -----------------------------------------------------------------------

fn shape(net: &[Vec<[f64; 4]>]) -> Result<(usize, usize), HarnessError> {
    let n = net
        .len()
        .checked_sub(1)
        .ok_or_else(|| harness("empty net"))?;
    let cols = net.first().map_or(0, Vec::len);
    let m = cols
        .checked_sub(1)
        .ok_or_else(|| harness("empty net row"))?;
    if net.iter().any(|r| r.len() != cols) {
        return Err(harness("net rows differ in length"));
    }
    Ok((n, m))
}

fn push_srf(o: &mut Vec<f64>, s: &SrfIn) -> Result<(), HarnessError> {
    match s {
        SrfIn::Bspline { net, p, q, u, v } => {
            let (n, m) = shape(net)?;
            o.push(0.0);
            o.extend(
                [
                    n,
                    m,
                    *p,
                    *q,
                    u.len().saturating_sub(1),
                    v.len().saturating_sub(1),
                ]
                .map(|x| x as f64),
            );
            for row in net {
                for pt in row {
                    o.extend_from_slice(pt);
                }
            }
            o.extend_from_slice(u);
            o.extend_from_slice(v);
        }
        SrfIn::Plane {
            o: org,
            x,
            y,
            scale,
            domain,
        } => {
            o.push(1.0);
            o.extend_from_slice(org);
            o.extend_from_slice(x);
            o.extend_from_slice(y);
            o.extend_from_slice(scale);
            o.extend_from_slice(domain);
        }
    }
    Ok(())
}

fn push_solver(o: &mut Vec<f64>, s: &SolverIn) -> Result<(), HarnessError> {
    o.push(s.cls as f64);
    o.push(flag(s.b.is_none()));
    push_srf(o, &s.a)?;
    if let Some(b) = &s.b {
        push_srf(o, b)?;
    }
    o.extend([s.r[0], s.r[1], flag(s.o[0]), flag(s.o[1]), s.ext]);
    Ok(())
}

fn push_plane(o: &mut Vec<f64>, p: &PlaneIn) {
    o.extend_from_slice(&p.orig);
    o.extend_from_slice(&p.normal);
    o.extend_from_slice(&p.d1);
    o.extend_from_slice(&p.d2);
    o.extend_from_slice(&p.uv1);
    o.extend_from_slice(&p.uv2);
}

fn push_kinds(o: &mut Vec<f64>, kinds: &[i64]) {
    o.push(kinds.len() as f64);
    o.extend(kinds.iter().map(|&k| k as f64));
}

fn cpp_input(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    match i {
        Inputs::CrInit {
            solver,
            do2,
            rail,
            sel1,
            s1_index,
            intervals,
            per,
            guess,
        } => {
            push_solver(&mut o, solver)?;
            o.extend([flag(*do2), *rail as f64, *sel1 as f64, *s1_index as f64]);
            o.push(intervals.len() as f64);
            for iv in intervals {
                o.extend_from_slice(iv);
            }
            o.push(per.len() as f64);
            o.extend(per.iter().map(|&b| flag(b)));
            o.push(guess.len() as f64);
            o.extend_from_slice(guess);
        }
        Inputs::Guess { solver, plane } | Inputs::Plane { solver, plane } => {
            push_solver(&mut o, solver)?;
            push_plane(&mut o, plane);
        }
        Inputs::Ss {
            a,
            b,
            r,
            o: orient,
            g,
            calc,
            kinds,
        } => {
            push_srf(&mut o, a)?;
            push_srf(&mut o, b)?;
            o.extend([r[0], r[1], flag(orient[0]), flag(orient[1])]);
            o.extend(g.iter().flatten());
            o.push(flag(*calc));
            push_kinds(&mut o, kinds);
        }
        Inputs::Types { kinds } => push_kinds(&mut o, kinds),
        Inputs::CdEu { .. } | Inputs::CrEu { .. } => {
            return Err(harness("the edge-use op has its own input"))
        }
    }
    Ok(o)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    if matches!(i, Inputs::CrEu { .. }) {
        return cpp_cr_eu(i);
    }
    let Some(op) = i.ffi() else {
        return cpp_edgeuse(i);
    };
    let input = cpp_input(i)?;
    let flat = ffi::eval(op, &input).map_err(|e| harness(e.to_string()))?;
    Ok(to_output(&flat))
}

// ---- the edge-use constructor ------------------------------------------------------

/// The case's B-rep: the C++ export, the imported Rust B-rep and the selected edge
/// use; `Err(output)` when the B-rep cannot be built or has no edge use (as unit
/// `topology.edgeuse_p3`'s runner).
fn eu_setup(brep: &BrepIn, eu: i64) -> Result<Result<Setup, Output>, HarnessError> {
    let path = path_of(Some(brep))?;
    let block = brep_block_of(Some(brep), false, false)?;
    let ex = ffi1::eval(ffi1::Op::Export, &path, &block)
        .map_err(|e| harness(format!("c++ export: {e}")))?;
    let Some(c) = ffi1::counts(&ex) else {
        let st = ex.get(1).copied().unwrap_or(0.0);
        return Ok(Err(Output::err(format!("brep_build_status_{st}"))));
    };
    let n_eu = c.get(6).copied().unwrap_or(0);
    let s = sel(eu, n_eu);
    if s < 0 {
        return Ok(Err(Output::err("no_entity_of_that_kind")));
    }
    let (brep, types) = import_with_types(&ex)?;
    let eu = EdgeuseId::from_index(u32::try_from(s).map_err(|_| harness("index"))?);
    Ok(Ok(Setup {
        path,
        block,
        brep,
        eu,
        types,
    }))
}

/// The output of the constructor's record after the build status, as both sides build
/// it: `-2` (upstream would dereference null) is an error output.
fn eu_output(body: &[f64]) -> Output {
    let first = body.first().copied().unwrap_or(f64::NAN);
    if first == -2.0 {
        return Output::err("null_deref");
    }
    Output::new()
        .int("status", first as i64)
        .values("out", body.get(1..).unwrap_or(&[]).to_vec())
}

fn cpp_edgeuse(i: &Inputs) -> Result<Output, HarnessError> {
    let Inputs::CdEu {
        brep,
        eu,
        tol,
        ang,
        tan,
        dist,
        fu,
        fv,
    } = i
    else {
        return Err(harness("not the edge-use op"));
    };
    let s = match eu_setup(brep, *eu)? {
        Ok(s) => s,
        Err(o) => return Ok(o),
    };
    let mut input = s.block.clone();
    // mode 0: the SmConstantDistanceFS constructor
    input.extend([
        0.0,
        f64::from(s.eu.index()),
        *tol,
        *ang,
        *tan,
        *dist,
        *fu,
        *fv,
    ]);
    let out = ffi::eval_edgeuse(&s.path, &input).map_err(|e| harness(format!("c++: {e}")))?;
    let body = out.get(1..).ok_or_else(|| harness("c++: empty output"))?;
    Ok(eu_output(body))
}

/// Whether the edge use has the edge's primary edge use (`GetMate` / `GetRadial`).
fn has_prim(b: &Brep, eu: EdgeuseId) -> Result<bool, SolidError> {
    if eu1::is_shell_edgeuse(b, eu)? {
        return Ok(true);
    }
    Ok(match eu1::edge(b, eu)? {
        Some(e) => eu1::primary_edgeuse(b, e)?.is_some(),
        None => false,
    })
}

/// The shim's `eu_safe`: every pointer `EvaluateBinormal` and the constructor
/// dereference for a loop edge use.
fn eu_safe(b: &Brep, eu: Option<EdgeuseId>) -> Result<bool, SolidError> {
    let Some(eu) = eu else {
        return Ok(false);
    };
    let Some(e) = eu1::edge(b, eu)? else {
        return Ok(false);
    };
    let has_surface = match eu1::face(b, eu)? {
        Some(f) => b.face(f)?.surface.is_some(),
        None => false,
    };
    if !eu1::is_loop_edgeuse(b, eu)?
        || b.edge(e)?.curve.is_none()
        || eu1::faceuse(b, eu)?.is_none()
        || eu1::loopuse(b, eu)?.is_none()
        || !has_surface
        || eu1::vertexuse(b, eu)?.is_none()
        || !has_prim(b, eu)?
    {
        return Ok(false);
    }
    let Some(prim) = eu1::primary_edgeuse(b, e)? else {
        return Ok(false);
    };
    let Some(pm) = eu1::mate(b, prim)? else {
        return Ok(false);
    };
    if eu1::vertexuse(b, prim)?.is_none() || eu1::vertexuse(b, pm)?.is_none() {
        return Ok(false);
    }
    if let Some(mate) = eu1::mate(b, eu)? {
        if eu1::vertexuse(b, mate)?.is_none() {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The Rust record body of the constructor; `Err` for an error that is not an
/// upstream status.
#[allow(clippy::too_many_arguments)]
fn rust_edgeuse_body(
    b: &mut Brep,
    eu: EdgeuseId,
    tol: f64,
    ang: f64,
    tan: f64,
    dist: f64,
    fu: f64,
    fv: f64,
) -> Result<Vec<f64>, SolidError> {
    let k = Kernel::default();
    let radial = if has_prim(b, eu)? {
        eu1::radial(b, eu)?
    } else {
        None
    };
    if !eu_safe(b, Some(eu))? || !eu_safe(b, radial)? {
        return Ok(vec![-2.0]);
    }
    let mut v = Vec::new();
    let fs = ConstantDistanceFs::from_edgeuse(&k, b, tol, ang, tan, dist, eu)?;
    v.push(0.0);
    let built = fs.solver.edgeuses[0].is_some();
    v.push(flag(built));
    v.extend(fs.solver.orientations);
    v.push(fs.distance);
    if built {
        for i in 0..2 {
            let e = fs.solver.edgeuses.get(i).copied().flatten();
            v.push(e.map_or(-1.0, |e| f64::from(e.index())));
            let s = fs
                .solver
                .surface(i)?
                .map(|slot| &slot.surface)
                .ok_or_else(|| SolidError::invalid_input("runner", "no offset surface"))?;
            v.push(s.offset_distance());
            let face = match e {
                Some(e) => eu1::face(b, e)?,
                None => None,
            };
            v.push(face.map_or(-1.0, |f| f64::from(f.index())));
            v.push(flag(face.is_some_and(|f| s.base_id == SurfaceId::Face(f))));
            let d = s.surface.natural_uv_domain();
            let uv = Point2::new(
                d.min().x + fu * (d.max().x - d.min().x),
                d.min().y + fv * (d.max().y - d.min().y),
            );
            match s.surface.evaluate_point(uv) {
                Ok(p) => v.extend([0.0, p.x, p.y, p.z]),
                Err(e) if e.is_unported() => return Err(e),
                Err(e) => {
                    let st = match e {
                        SolidError::Upstream { code, .. } => f64::from(code),
                        _ => return Err(e),
                    };
                    v.extend([st, 0.0, 0.0, 0.0]);
                }
            }
        }
    }
    put_eu_state(&mut v, b, Some(eu))?;
    put_eu_state(&mut v, b, radial)?;
    Ok(v)
}

fn rust_edgeuse(i: &Inputs) -> Result<Output, HarnessError> {
    let Inputs::CdEu {
        brep,
        eu,
        tol,
        ang,
        tan,
        dist,
        fu,
        fv,
    } = i
    else {
        return Err(harness("not the edge-use op"));
    };
    let mut s = match eu_setup(brep, *eu)? {
        Ok(s) => s,
        Err(o) => return Ok(o),
    };
    if let Some(o) = inexact_geometry(&s) {
        return Ok(o);
    }
    // the radial's face and edge too
    if has_prim(&s.brep, s.eu).map_err(|e| he(&e))? {
        if let Some(radial) = eu1::radial(&s.brep, s.eu).map_err(|e| he(&e))? {
            let probe = Setup {
                path: String::new(),
                block: Vec::new(),
                brep: s.brep.clone(),
                eu: radial,
                types: s.types.clone(),
            };
            if let Some(o) = inexact_geometry(&probe) {
                return Ok(o);
            }
        }
    }
    match rust_edgeuse_body(&mut s.brep, s.eu, *tol, *ang, *tan, *dist, *fu, *fv) {
        Ok(body) => Ok(eu_output(&body)),
        Err(e) => {
            if let Some(o) = Output::unported(&e) {
                return Ok(o);
            }
            match e {
                SolidError::InvalidInput { .. } => Ok(Output::err("null_deref")),
                e => Err(harness(format!("rust: {e}"))),
            }
        }
    }
}

// ---- LoadInitialValues with edge uses ---------------------------------------------------

fn cpp_cr_eu(i: &Inputs) -> Result<Output, HarnessError> {
    let Inputs::CrEu {
        brep,
        eu,
        tol,
        ang,
        tan,
        r,
        rail,
        fu,
        fv,
        ext,
    } = i
    else {
        return Err(harness("not the edge-use initial-values op"));
    };
    let s = match eu_setup(brep, *eu)? {
        Ok(s) => s,
        Err(o) => return Ok(o),
    };
    let mut input = s.block.clone();
    // mode 1: SmConstantRadiusFS::LoadInitialValues with edge uses
    input.extend([
        1.0,
        f64::from(s.eu.index()),
        *tol,
        *ang,
        *tan,
        *r,
        *rail as f64,
        *fu,
        *fv,
        *ext,
    ]);
    let out = ffi::eval_edgeuse(&s.path, &input).map_err(|e| harness(format!("c++: {e}")))?;
    let body = out.get(1..).ok_or_else(|| harness("c++: empty output"))?;
    Ok(eu_output(body))
}

/// The Rust record body of the edge-use `LoadInitialValues` op (the shim's mode 1).
#[allow(clippy::too_many_arguments)]
fn rust_cr_eu_body(
    b: &mut Brep,
    eu: EdgeuseId,
    tol: f64,
    ang: f64,
    tan: f64,
    r: f64,
    rail: usize,
    fu: f64,
    fv: f64,
    ext: f64,
) -> Result<Vec<f64>, SolidError> {
    let k = Kernel::default();
    let radial = if has_prim(b, eu)? {
        eu1::radial(b, eu)?
    } else {
        None
    };
    if !eu_safe(b, Some(eu))? || !eu_safe(b, radial)? {
        return Ok(vec![-2.0]);
    }
    let mut fs = match constant_radius_from_edgeuse(&k, b, tol, ang, tan, r, eu, None) {
        Ok(fs) => fs,
        Err(e) if e.is_unported() => return Err(e),
        // the tangent-surface exits (SM_ERR only signalled upstream)
        Err(SolidError::Upstream { code, .. }) => return Ok(vec![f64::from(code)]),
        Err(e) => return Err(e),
    };
    fs.solver.surface_extension_factor = ext;
    let mut v = vec![0.0];
    let mut s1 = fs.surface(rail)?.clone();
    let d = s1.surface.natural_uv_domain();
    let mut ivls = ExtentNd::new(4);
    for (n, iv) in [
        Extent1d::from_min_max(d.min().x, d.max().x),
        Extent1d::from_min_max(d.min().y, d.max().y),
    ]
    .into_iter()
    .enumerate()
    {
        if let Some(slot) = ivls.get_mut(n) {
            *slot = iv;
        }
    }
    let mut per = vec![false, false];
    let mut guess = vec![
        d.min().x + fu * (d.max().x - d.min().x),
        d.min().y + fv * (d.max().y - d.min().y),
    ];
    // (crSurface1 is *GetSurface(rail), evaluated inside its domain: it makes no
    // extension the copy would have to hand back)
    let res = fs.load_initial_values(
        &k,
        Some(&*b),
        true,
        rail,
        &mut s1,
        0,
        &mut ivls,
        &mut per,
        &mut guess,
    );
    match &res {
        Ok(Some(iv)) => v.extend([0.0, iv.surface2_rail as f64, iv.surface2_index as f64]),
        Ok(None) => v.extend([0.0, -1.0, 999.0]),
        Err(e) if e.is_unported() => return Err(e.clone()),
        // (upstream sets rpSurface2 first and leaves the index unchanged)
        Err(e) => v.extend([status(e), (1 - rail) as f64, 999.0]),
    }
    if res.is_ok() {
        for n in 2..4 {
            let iv = ivls
                .get(n)
                .ok_or_else(|| SolidError::invalid_input("runner", "interval"))?;
            v.extend([iv.min(), iv.max()]);
        }
    }
    v.push(per.len() as f64);
    v.extend(per.iter().map(|&p| flag(p)));
    v.push(guess.len() as f64);
    v.extend_from_slice(&guess);
    v.extend(offsets(&fs));
    Ok(v)
}

fn rust_cr_eu(i: &Inputs) -> Result<Output, HarnessError> {
    let Inputs::CrEu {
        brep,
        eu,
        tol,
        ang,
        tan,
        r,
        rail,
        fu,
        fv,
        ext,
    } = i
    else {
        return Err(harness("not the edge-use initial-values op"));
    };
    let mut s = match eu_setup(brep, *eu)? {
        Ok(s) => s,
        Err(o) => return Ok(o),
    };
    if let Some(o) = inexact_geometry(&s) {
        return Ok(o);
    }
    // the radial's face and edge too
    if has_prim(&s.brep, s.eu).map_err(|e| he(&e))? {
        if let Some(radial) = eu1::radial(&s.brep, s.eu).map_err(|e| he(&e))? {
            let probe = Setup {
                path: String::new(),
                block: Vec::new(),
                brep: s.brep.clone(),
                eu: radial,
                types: s.types.clone(),
            };
            if let Some(o) = inexact_geometry(&probe) {
                return Ok(o);
            }
        }
    }
    match rust_cr_eu_body(
        &mut s.brep,
        s.eu,
        *tol,
        *ang,
        *tan,
        *r,
        *rail,
        *fu,
        *fv,
        *ext,
    ) {
        Ok(body) => Ok(eu_output(&body)),
        Err(e) => {
            if let Some(o) = Output::unported(&e) {
                return Ok(o);
            }
            Err(harness(format!("rust: {e}")))
        }
    }
}

fn gen_cr_eu(rng: &mut Rng) -> Inputs {
    let Inputs::CdEu {
        brep,
        eu,
        tol,
        ang,
        tan,
        dist,
        fu,
        fv,
    } = gen_cd_eu(rng)
    else {
        return Inputs::Types { kinds: Vec::new() };
    };
    Inputs::CrEu {
        brep,
        eu,
        tol,
        ang,
        tan,
        r: dist,
        rail: usize::from(rng.chance(0.5)),
        fu,
        fv,
        ext: *rng.choose(&[0.5, 0.1, 0.0, 1.0, 2.0]).unwrap_or(&0.5),
    }
}

fn gen_cd_eu(rng: &mut Rng) -> Inputs {
    // Mostly the upstream solid B-reps (their edges have radial face uses); see the
    // `blend.fillet_standard_solver_p1` runner's `gen_cr_eu`.
    let brep = match rng.below(10) {
        0..=5 => BrepIn::File {
            file: "box_with_large_fillet.smb".to_string(),
            rebuild: rng.chance(0.25),
        },
        6..=8 => BrepIn::File {
            file: (*rng.choose(&FILES).unwrap_or(&"Mouse.smb")).to_string(),
            rebuild: rng.chance(0.25),
        },
        _ => gen_brep(rng),
    };
    Inputs::CdEu {
        brep,
        eu: rng.below(1 << 20) as i64,
        tol: *rng.choose(&[1e-6, 1e-3, 0.0, 1e-9]).unwrap_or(&1e-6),
        ang: 30.0 * DEG,
        tan: *rng.choose(&[2.0 * DEG, 0.0, 0.1]).unwrap_or(&0.0),
        dist: *rng
            .choose(&[0.1, 0.5, 1.0, 2.5, -0.3, 0.0, 1e-7, 50.0])
            .unwrap_or(&0.5),
        fu: *rng.choose(&[0.0, 0.25, 0.5, 1.0, 0.9]).unwrap_or(&0.5),
        fv: *rng.choose(&[0.0, 0.4, 0.5, 1.0, 0.1]).unwrap_or(&0.5),
    }
}

/// Splits a flat record into `status` and `out`.
fn to_output(flat: &[f64]) -> Output {
    let (status, rest) = flat
        .split_first()
        .map_or((f64::NAN, &[][..]), |(s, r)| (*s, r));
    Output::new()
        .values("status", vec![status])
        .values("out", rest.to_vec())
}

// ---- Rust side ----------------------------------------------------------------------

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn surface_of(s: &SrfIn) -> Result<Surface, HarnessError> {
    Ok(match s {
        SrfIn::Bspline { net, p, q, u, v } => {
            let net = net
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
                        .collect()
                })
                .collect();
            let nurb = NurbSurface::new(net, *p, *q, u.clone(), v.clone())
                .map_err(|e| harness(format!("surface input: {e}")))?;
            Surface::from(BSplineSurface::from_nurb(nurb, false))
        }
        SrfIn::Plane {
            o,
            x,
            y,
            scale,
            domain,
        } => Surface::from(Plane::new(
            v3(*o),
            v3(*x),
            v3(*y),
            Vec2::new(scale[0], scale[1]),
            // (`SmExtent2d(u0, v0, u1, v1)`: stored as given, as the shim builds it)
            &Extent2d::from_bounds(domain[0], domain[1], domain[2], domain[3]),
        )),
    })
}

/// The `SmStatus` of a Rust error; `-1` for one upstream cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

/// A record or the deferral of an unported callee.
type Rec = Result<Vec<f64>, Box<Output>>;

fn fail(e: &SolidError) -> Result<f64, Box<Output>> {
    match Output::unported(e) {
        Some(out) => Err(Box::new(out)),
        None => Ok(status(e)),
    }
}

/// `new SmOffsetSurface(d, surface)` (upstream's default `SM_SS_UNKNOWN`).
fn offset_of(k: &Kernel, d: f64, s: Surface, id: SurfaceId) -> Result<OffsetSurface, HarnessError> {
    OffsetSurface::new(k, d, s, id, SS_UNKNOWN).map_err(|e| he(&e))
}

/// The solver of a block: the class's struct (the surface/surface solver's base is the
/// constant-radius data) and the base surfaces A and B with their identities.
enum Solver {
    Cr(ConstantRadiusFs),
    Ss(SurfaceSurfaceFs),
}

impl Solver {
    fn cr(&self) -> &ConstantRadiusFs {
        match self {
            Solver::Cr(c) => c,
            Solver::Ss(s) => &s.base,
        }
    }
}

struct BuiltSolver {
    solver: Solver,
    a: Surface,
}

fn solver_of(k: &Kernel, s: &SolverIn) -> Result<BuiltSolver, HarnessError> {
    let a = surface_of(&s.a)?;
    let (b, b_id) = match &s.b {
        Some(b) => (surface_of(b)?, SurfaceId::Free(1)),
        None => (a.clone(), SurfaceId::Free(0)),
    };
    let surfaces = [(a.clone(), SurfaceId::Free(0)), (b, b_id)];
    let solver = match s.cls {
        0 => {
            let mut c = ConstantRadiusFs::from_surfaces(
                k,
                1.0e-6,
                30.0 * DEG,
                2.0 * DEG,
                s.r,
                surfaces,
                s.o,
            )
            .map_err(|e| he(&e))?;
            c.solver.surface_extension_factor = s.ext;
            Solver::Cr(c)
        }
        1 => {
            let mut c = SurfaceSurfaceFs::new(
                k,
                1.0e-6,
                30.0 * DEG,
                2.0 * DEG,
                s.r,
                surfaces,
                s.o,
                [Point2::new(0.5, 0.5), Point2::new(0.5, 0.5)],
            )
            .map_err(|e| he(&e))?;
            c.base.solver.surface_extension_factor = s.ext;
            Solver::Ss(c)
        }
        _ => {
            let d = s.o.map(|b| if b { 1.0 } else { -1.0 });
            let [(sa, ia), (sb, ib)] = surfaces;
            let mut s0 = offset_of(k, s.r[0] * d[0], sa, ia)?;
            let mut s1 = offset_of(k, s.r[1] * d[1], sb, ib)?;
            s0.set_allow_extension(true);
            s1.set_allow_extension(true);
            let mut c = ConstantRadiusFs::from_parts(
                1.0e-6,
                30.0 * DEG,
                2.0 * DEG,
                s.r,
                [Some(s0), Some(s1)],
                d,
                [None, None],
            );
            c.solver.surface_extension_factor = s.ext;
            Solver::Cr(c)
        }
    };
    Ok(BuiltSolver { solver, a })
}

/// The shim's `put_offsets`: per rail `offset umin vmin umax vmax has_extension`
/// (`GetNaturalUVDomain`, `GetExtendedBaseSurface() != NULL`) after the op.
fn offsets(fs: &ConstantRadiusFs) -> Vec<f64> {
    offset_state(|i| fs.surface(i).ok())
}

fn offset_state<'a>(rail: impl Fn(usize) -> Option<&'a OffsetSurface>) -> Vec<f64> {
    let mut o = Vec::new();
    for i in 0..2 {
        match rail(i) {
            Some(s) => {
                o.push(s.offset_distance());
                let d = s.surface.natural_uv_domain();
                o.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
                o.push(flag(s.surface.with_extended_base_surface(|e| e.is_some())));
            }
            None => o.extend([f64::NAN; 6]),
        }
    }
    o
}

/// The surface of a selector (`1` `GetSurface(0)`, `2` `GetSurface(1)`, `3` another offset
/// of A, `4` an offset of a copy of A), as the caller's copy.
fn select(k: &Kernel, b: &BuiltSolver, r0: f64, sel: usize) -> Result<OffsetSurface, HarnessError> {
    Ok(match sel {
        1 | 2 => b.solver.cr().surface(sel - 1).map_err(|e| he(&e))?.clone(),
        3 => offset_of(k, r0, b.a.clone(), SurfaceId::Free(0))?,
        4 => offset_of(k, r0, b.a.clone(), SurfaceId::Free(2))?,
        _ => return Err(harness("init: NULL first surface")),
    })
}

fn extent2(d: [f64; 4]) -> Extent2d {
    Extent2d::from_bounds(d[0], d[1], d[2], d[3])
}

fn p2(v: [f64; 2]) -> Point2 {
    Point2::new(v[0], v[1])
}

fn push_tsect_uvs(o: &mut Vec<f64>, t: &TsectPnt) {
    for srf in [SrfIndex::First, SrfIndex::Second] {
        let uv = t.uv_pos(srf);
        o.extend([uv.x, uv.y]);
    }
}

fn push_v(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

/// `GetType` and the `IsKindOf` of each code.
fn push_types(o: &mut Vec<f64>, code: i64, is_kind_of: impl Fn(i64) -> bool, kinds: &[i64]) {
    o.push(code as f64);
    o.extend(kinds.iter().map(|&k| flag(is_kind_of(k))));
}

#[allow(clippy::too_many_lines)]
fn rust_record(i: &Inputs) -> Result<Rec, HarnessError> {
    let k = Kernel::default();
    let mut o = Vec::new();
    match i {
        Inputs::CrInit {
            solver,
            do2,
            rail,
            sel1,
            s1_index,
            intervals,
            per,
            guess,
        } => {
            let mut b = solver_of(&k, solver)?;
            let mut s1 = select(&k, &b, solver.r[0], *sel1)?;
            let mut ivls = ExtentNd::new(intervals.len());
            for (n, iv) in intervals.iter().enumerate() {
                *ivls.get_mut(n).ok_or_else(|| harness("interval index"))? =
                    Extent1d::from_min_max(iv[0], iv[1]);
            }
            let mut per = per.clone();
            let mut guess = guess.clone();
            let res = match &mut b.solver {
                Solver::Cr(c) => c.load_initial_values(
                    &k, None, *do2, *rail, &mut s1, *s1_index, &mut ivls, &mut per, &mut guess,
                ),
                Solver::Ss(c) => c.load_initial_values(
                    &k, None, *do2, *rail, &mut s1, *s1_index, &mut ivls, &mut per, &mut guess,
                ),
            };
            // (crSurface1 is *GetSurface(rail) when sel1 names the solver's own rail: C++
            // evaluates the solver's offset itself, so the extension the guess outside
            // its domain makes stays on it)
            if *sel1 == 1 + *rail {
                let cr = match &mut b.solver {
                    Solver::Cr(c) => c,
                    Solver::Ss(c) => &mut c.base,
                };
                cr.take_surface_extension(*rail, &s1).map_err(|e| he(&e))?;
            }
            match &res {
                Ok(Some(v)) => o.extend([0.0, v.surface2_rail as f64, v.surface2_index as f64]),
                Ok(None) => o.extend([0.0, -1.0, 999.0]),
                Err(e) => match fail(e) {
                    // (upstream sets rpSrf2Offset first and leaves the index unchanged)
                    Ok(s) => o.extend([s, 1usize.wrapping_sub(*rail) as f64, 999.0]),
                    Err(out) => return Ok(Err(out)),
                },
            }
            for n in 0..intervals.len() {
                let iv = ivls.get(n).ok_or_else(|| harness("interval index"))?;
                o.extend([iv.min(), iv.max()]);
            }
            o.push(per.len() as f64);
            o.extend(per.iter().map(|&b| flag(b)));
            o.push(guess.len() as f64);
            o.extend_from_slice(&guess);
            o.extend(offsets(b.solver.cr()));
        }
        Inputs::Guess { solver, plane } | Inputs::Plane { solver, plane } => {
            let b = solver_of(&k, solver)?;
            let Solver::Cr(mut fs) = b.solver else {
                return Err(harness("plane solve: needs the assisted solver"));
            };
            let guess = matches!(i, Inputs::Guess { .. });
            let (d1, d2) = (extent2(plane.d1), extent2(plane.d2));
            let mut t = TsectPnt::new();
            let res = if guess {
                fs.guess_point_on_plane_solve(
                    v3(plane.orig),
                    v3(plane.normal),
                    &d1,
                    &d2,
                    p2(plane.uv1),
                    p2(plane.uv2),
                    &mut t,
                )
            } else {
                point_on_plane_solve(
                    &mut fs,
                    &k,
                    None,
                    v3(plane.orig),
                    v3(plane.normal),
                    &d1,
                    &d2,
                    p2(plane.uv1),
                    p2(plane.uv2),
                    &mut t,
                )
            };
            match res {
                Ok(false) => o.extend([0.0, 0.0]),
                Ok(true) => {
                    o.extend([0.0, 1.0]);
                    push_tsect_uvs(&mut o, &t);
                    if !guess {
                        for srf in [SrfIndex::First, SrfIndex::Second] {
                            push_v(&mut o, t.srf_pos(srf));
                            push_v(&mut o, t.srf_du(srf));
                            push_v(&mut o, t.srf_dv(srf));
                            push_v(&mut o, t.srf_norm(srf));
                        }
                        push_v(&mut o, t.crv_pos());
                    }
                }
                Err(e) => match fail(&e) {
                    Ok(s) => o.push(s),
                    Err(out) => return Ok(Err(out)),
                },
            }
            o.extend(offsets(&fs));
        }
        Inputs::Ss {
            a,
            b,
            r,
            o: orient,
            g,
            calc,
            kinds,
        } => {
            let surfaces = [
                (surface_of(a)?, SurfaceId::Free(0)),
                (surface_of(b)?, SurfaceId::Free(1)),
            ];
            let mut fs = SurfaceSurfaceFs::new(
                &k,
                1.0e-6,
                30.0 * DEG,
                2.0 * DEG,
                *r,
                surfaces,
                *orient,
                [p2(g[0]), p2(g[1])],
            )
            .map_err(|e| he(&e))?;
            let mut cs = 0.0;
            if *calc {
                // the shim's pre-check of CalcFilletGeom's parallel-normal exit
                let mut normal = |i: usize, uv: Point2| -> Option<Vec3> {
                    fs.base
                        .solver
                        .surfaces
                        .get_mut(i)
                        .and_then(Option::as_mut)
                        .and_then(|s| {
                            s.surface
                                .surface
                                .evaluate_normal_extending(uv, Side::Left, Side::Left)
                                .ok()
                        })
                };
                let n1 = normal(0, p2(g[0]));
                let n2 = normal(1, p2(g[1]));
                cs = match (n1, n2) {
                    (Some(n1), Some(n2))
                        if n1.cross(n2).length_squared() < solidcore::math::EFF_ZERO_SQ =>
                    {
                        match fs.calc_fillet_geom(&k) {
                            Ok(()) => 0.0,
                            Err(e) => match fail(&e) {
                                Ok(s) => s,
                                Err(out) => return Ok(Err(out)),
                            },
                        }
                    }
                    _ => NOT_RUN,
                };
            }
            o.push(cs);
            for i in 0..2 {
                o.push(fs.base.fillet_radius(i).map_err(|e| he(&e))?);
            }
            o.extend(fs.base.solver.orientations);
            o.extend(offsets(&fs.base));
            for g in fs.uv_guesses {
                o.extend([g.x, g.y]);
            }
            o.push(f64::from(fs.trim_type as i32));
            o.push(f64::from(fs.solver_type() as i32));
            push_types(&mut o, fs.type_code(), |t| fs.is_kind_of(t), kinds);
        }
        Inputs::Types { kinds } => {
            let plane = |y: Vec3| {
                Surface::from(Plane::new(
                    Vec3::new(0.0, 0.0, 0.0),
                    Vec3::new(1.0, 0.0, 0.0),
                    y,
                    Vec2::new(1.0, 1.0),
                    &Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
                ))
            };
            let ss = SurfaceSurfaceFs::new(
                &k,
                1.0e-6,
                30.0 * DEG,
                2.0 * DEG,
                [0.1, 0.1],
                [
                    (plane(Vec3::new(0.0, 1.0, 0.0)), SurfaceId::Free(0)),
                    (plane(Vec3::new(0.0, 0.0, 1.0)), SurfaceId::Free(1)),
                ],
                [true, true],
                [Point2::new(0.5, 0.5), Point2::new(0.5, 0.5)],
            )
            .map_err(|e| he(&e))?;
            // (the variable-radius constructor needs the unported fillet law: its type
            // queries do not read the solver's data)
            let vr = VariableRadiusFs {
                curve_based: CurveBasedFs::new(1.0e-6, 30.0 * DEG, 2.0 * DEG),
                law: FilletLaw::new(FilletLawKind::Linear {
                    start_radius: 0.1,
                    end_radius: 0.1,
                }),
                law_orientation: false,
                law_interval: Extent1d::from_min_max(0.0, 1.0),
            };
            o.push(0.0);
            push_types(
                &mut o,
                CONSTANT_RADIUS_ASSISTED_FS_TYPE,
                constant_radius_assisted_is_kind_of,
                kinds,
            );
            push_types(&mut o, ss.type_code(), |t| ss.is_kind_of(t), kinds);
            push_types(&mut o, vr.type_code(), |t| vr.is_kind_of(t), kinds);
        }
        Inputs::CdEu { .. } | Inputs::CrEu { .. } => {
            return Err(harness("the edge-use op runs apart"))
        }
    }
    Ok(Ok(o))
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    if matches!(i, Inputs::CrEu { .. }) {
        return rust_cr_eu(i);
    }
    if i.ffi().is_none() {
        return rust_edgeuse(i);
    }
    Ok(match rust_record(i)? {
        Ok(flat) => to_output(&flat),
        Err(out) => *out,
    })
}

// ---- generators ---------------------------------------------------------------------

fn unit_vec(rng: &mut Rng) -> Vec3 {
    loop {
        let v = Vec3::new(
            rng.uniform(-1.0, 1.0),
            rng.uniform(-1.0, 1.0),
            rng.uniform(-1.0, 1.0),
        );
        let l = v.length();
        if l > 0.2 && l <= 1.0 {
            return v / l;
        }
    }
}

/// A unit vector perpendicular to `d`.
fn perp(d: Vec3, rng: &mut Rng) -> Vec3 {
    loop {
        let w = unit_vec(rng);
        let p = w - d * w.dot(d);
        let l = p.length();
        if l > 0.2 {
            return p / l;
        }
    }
}

/// Clamped uniform knots for `n + 1` control points of degree `p` over `[a, b]`.
fn uniform_knots(n: usize, p: usize, a: f64, b: f64) -> Vec<f64> {
    let spans = n + 1 - p;
    let mut u = vec![a; p + 1];
    for i in 1..spans {
        u.push(a + (b - a) * i as f64 / spans as f64);
    }
    u.extend(std::iter::repeat_n(b, p + 1));
    u
}

fn arr(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn gen_bspline_surface(rng: &mut Rng, z0: f64) -> SrfIn {
    let p = 1 + rng.below(3) as usize;
    let q = 1 + rng.below(3) as usize;
    let n = p + rng.below(2) as usize;
    let m = q + rng.below(2) as usize;
    let net = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    [
                        4.0 * i as f64 / n as f64,
                        4.0 * j as f64 / m as f64,
                        z0 + rng.uniform(-0.3, 0.3),
                        NOW,
                    ]
                })
                .collect()
        })
        .collect();
    SrfIn::Bspline {
        net,
        p,
        q,
        u: uniform_knots(n, p, 0.0, 1.0),
        v: uniform_knots(m, q, 0.0, 1.0),
    }
}

fn gen_plane(rng: &mut Rng) -> SrfIn {
    let x = unit_vec(rng);
    let y = perp(x, rng);
    SrfIn::Plane {
        o: rng.point3(-2.0, 2.0),
        x: arr(x),
        y: arr(y),
        scale: [rng.uniform(0.5, 3.0), rng.uniform(0.5, 3.0)],
        domain: [0.0, 0.0, rng.uniform(0.5, 2.0), rng.uniform(0.5, 2.0)],
    }
}

/// A cylinder-like B-spline surface: a full rational circle extruded (periodic in u).
fn gen_closed_surface(rng: &mut Rng) -> Option<SrfIn> {
    let r = rng.uniform(0.5, 3.0);
    let h = rng.uniform(0.5, 4.0);
    let circle = BSplineCurve::create_circle_segment(
        3,
        &Axis2Placement::default(),
        r,
        0.0,
        360.0,
        NurbCircleParam::Quadratic,
    )
    .ok()?;
    let cur = circle.gw_nurb()?;
    let pts: Vec<[f64; 4]> = cur.cpts().iter().map(|p| [p.x, p.y, p.z, p.w]).collect();
    let net = pts
        .iter()
        .map(|a| {
            let mut b = *a;
            // weighted coordinates when rational
            b[2] += if a[3] == NOW { h } else { h * a[3] };
            vec![*a, b]
        })
        .collect();
    Some(SrfIn::Bspline {
        net,
        p: cur.degree(),
        q: 1,
        u: cur.knots().to_vec(),
        v: vec![0.0, 0.0, 1.0, 1.0],
    })
}

/// A B-spline surface with a collapsed boundary row (a pole).
fn gen_singular_surface(rng: &mut Rng) -> SrfIn {
    let SrfIn::Bspline {
        mut net,
        p,
        q,
        u,
        v,
    } = gen_bspline_surface(rng, 0.0)
    else {
        return gen_plane(rng);
    };
    let apex = [2.0, 2.0, 2.0, NOW];
    if rng.chance(0.5) {
        if let Some(row) = net.first_mut() {
            for pt in row.iter_mut() {
                *pt = apex;
            }
        }
    } else {
        for row in &mut net {
            if let Some(pt) = row.last_mut() {
                *pt = apex;
            }
        }
    }
    SrfIn::Bspline { net, p, q, u, v }
}

fn gen_surface(rng: &mut Rng) -> SrfIn {
    match rng.below(8) {
        0 | 1 => gen_plane(rng),
        2 => gen_closed_surface(rng).unwrap_or_else(|| gen_plane(rng)),
        3 => gen_singular_surface(rng),
        _ => {
            let z0 = rng.uniform(0.5, 2.0);
            gen_bspline_surface(rng, z0)
        }
    }
}

/// Two planes meeting along an edge in direction `x` at angle `phi` (a randomly placed
/// rolling-ball corner): A spans `x, y`, B spans `x, z'` with `z'` at `phi` from `y`;
/// returns the planes, the origin and the edge direction.
fn gen_corner(rng: &mut Rng) -> (SrfIn, SrfIn, Vec3, Vec3) {
    let x = unit_vec(rng);
    let y = perp(x, rng);
    let z = x.cross(y);
    let phi = rng.uniform(40.0, 140.0) * DEG;
    let w = y * phi.cos() + z * phi.sin();
    let o = Vec3::new(
        rng.uniform(-2.0, 2.0),
        rng.uniform(-2.0, 2.0),
        rng.uniform(-2.0, 2.0),
    );
    let s = if rng.chance(0.2) {
        rng.uniform(0.5, 2.0)
    } else {
        1.0
    };
    let a = SrfIn::Plane {
        o: arr(o),
        x: arr(x),
        y: arr(y),
        scale: [s, s],
        domain: [0.0, 0.0, 1.0, 1.0],
    };
    let b = SrfIn::Plane {
        o: arr(o),
        x: arr(x),
        y: arr(w),
        scale: [s, s],
        domain: [0.0, 0.0, 1.0, 1.0],
    };
    (a, b, o, x)
}

fn gen_solver(rng: &mut Rng, cls: usize, corner: Option<(SrfIn, SrfIn)>) -> SolverIn {
    let (a, b) = if let Some((a, b)) = corner {
        (a, Some(b))
    } else {
        let a = gen_surface(rng);
        let b = if rng.chance(0.1) {
            None
        } else {
            Some(gen_surface(rng))
        };
        (a, b)
    };
    let r0 = rng.uniform(0.05, 0.5);
    let r1 = if rng.chance(0.7) {
        r0
    } else {
        rng.uniform(0.05, 0.5)
    };
    SolverIn {
        cls,
        a,
        b,
        r: [r0, r1],
        o: [rng.chance(0.5), rng.chance(0.5)],
        ext: *rng.choose(&[0.0, 0.1, 0.5, 1.0]).unwrap_or(&0.5),
    }
}

fn gen_cr_init(rng: &mut Rng) -> Inputs {
    let cls = usize::from(rng.chance(0.75));
    let corner = if rng.chance(0.4) {
        let (a, b, _, _) = gen_corner(rng);
        Some((a, b))
    } else {
        None
    };
    let solver = gen_solver(rng, cls, corner);
    let do2 = rng.chance(0.92);
    let rail = rng.below(2) as usize;
    let sel1 = if rng.chance(0.7) {
        1 + rail
    } else {
        1 + rng.below(4) as usize
    };
    let s1_index = if rng.chance(0.7) { 0 } else { 2 };
    let mut guess: Vec<f64> = (0..s1_index).map(|_| rng.uniform(0.0, 1.0)).collect();
    guess.push(rng.uniform(0.05, 0.95));
    guess.push(rng.uniform(0.05, 0.95));
    let dim = guess.len() + 2 + rng.below(2) as usize;
    let intervals = (0..dim)
        .map(|_| {
            let a = rng.uniform(-1.0, 0.5);
            [a, a + rng.uniform(0.0, 2.0)]
        })
        .collect();
    let per = (0..s1_index).map(|_| rng.chance(0.2)).collect();
    Inputs::CrInit {
        solver,
        do2,
        rail,
        sel1,
        s1_index,
        intervals,
        per,
        guess,
    }
}

/// A plane solve on a rolling-ball corner (orientations towards the inside of the
/// corner for most cases), or on random surfaces.
fn gen_plane_solve(rng: &mut Rng, guess: bool) -> Inputs {
    let wide = [-1.0, -1.0, 2.0, 2.0];
    if rng.chance(0.75) {
        let (a, b, o, x) = gen_corner(rng);
        let mut solver = gen_solver(rng, 2, Some((a, b)));
        if rng.chance(0.8) {
            // A's normal x * y points into the corner, B's x * w points away from it
            solver.o = [true, false];
        }
        let t = rng.uniform(0.2, 0.8);
        let orig = o + x * t;
        let normal = if rng.chance(0.85) { x } else { unit_vec(rng) };
        let r = solver.r;
        let near = rng.chance(0.7);
        // (outside the base domain the allow-extension offsets extend their base)
        let outside = rng.chance(0.3);
        let jitter = |rng: &mut Rng, v: f64| {
            if near {
                v + rng.uniform(-0.05, 0.05)
            } else if outside {
                rng.uniform(-0.4, 1.4)
            } else {
                rng.uniform(0.0, 1.0)
            }
        };
        let uv1 = [jitter(rng, t), jitter(rng, r[1])];
        let uv2 = [jitter(rng, t), jitter(rng, r[0])];
        let d = if rng.chance(0.8) {
            wide
        } else {
            [0.0, 0.0, 1.0, 1.0]
        };
        let plane = PlaneIn {
            orig: arr(orig),
            normal: arr(normal),
            d1: d,
            d2: d,
            uv1,
            uv2,
        };
        return if guess {
            Inputs::Guess { solver, plane }
        } else {
            Inputs::Plane { solver, plane }
        };
    }
    let solver = gen_solver(rng, 2, None);
    // guesses outside the base domains in a third of the cases (the offsets extend)
    let (lo, hi) = if rng.chance(0.35) {
        (-0.4, 1.4)
    } else {
        (0.0, 1.0)
    };
    let plane = PlaneIn {
        orig: rng.point3(-2.0, 2.0),
        normal: arr(unit_vec(rng)),
        d1: wide,
        d2: wide,
        uv1: [rng.uniform(lo, hi), rng.uniform(lo, hi)],
        uv2: [rng.uniform(lo, hi), rng.uniform(lo, hi)],
    };
    if guess {
        Inputs::Guess { solver, plane }
    } else {
        Inputs::Plane { solver, plane }
    }
}

fn type_kinds(rng: &mut Rng) -> Vec<i64> {
    let mut v: Vec<i64> = (16030..16050).collect();
    v.extend([1, 2, 3, 16000, 16001]);
    v.push(rng.range_i64(0, 20000));
    v
}

fn gen_ss(rng: &mut Rng) -> Inputs {
    let parallel = rng.chance(0.3);
    let (a, b) = if parallel {
        // two parallel planes: CalcFilletGeom's parallel-normal exit
        let x = unit_vec(rng);
        let y = perp(x, rng);
        let n = x.cross(y);
        let o = Vec3::new(
            rng.uniform(-2.0, 2.0),
            rng.uniform(-2.0, 2.0),
            rng.uniform(-2.0, 2.0),
        );
        let plane = |o: Vec3| SrfIn::Plane {
            o: arr(o),
            x: arr(x),
            y: arr(y),
            scale: [1.0, 1.0],
            domain: [0.0, 0.0, 1.0, 1.0],
        };
        (plane(o), plane(o + n * rng.uniform(0.1, 2.0)))
    } else if rng.chance(0.5) {
        let (a, b, _, _) = gen_corner(rng);
        (a, b)
    } else {
        (gen_surface(rng), gen_surface(rng))
    };
    Inputs::Ss {
        a,
        b,
        r: [rng.uniform(0.05, 0.5), rng.uniform(0.05, 0.5)],
        o: [rng.chance(0.5), rng.chance(0.5)],
        g: [
            [rng.uniform(0.0, 1.0), rng.uniform(0.0, 1.0)],
            [rng.uniform(0.0, 1.0), rng.uniform(0.0, 1.0)],
        ],
        calc: rng.chance(0.8),
        kinds: type_kinds(rng),
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    match rng.below(25) {
        0..=7 => gen_cr_init(rng),
        8..=12 => gen_plane_solve(rng, true),
        13..=14 => gen_plane_solve(rng, false),
        15..=18 => gen_ss(rng),
        19..=20 => gen_cd_eu(rng),
        22..=24 => gen_cr_eu(rng),
        _ => Inputs::Types {
            kinds: type_kinds(rng),
        },
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "blend.fillet_standard_solver_p2"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_case(rng)).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// The generators reach every op, mostly with real values on the Rust side.
    #[test]
    fn generated_cases_cover_every_op() {
        let mut tally: BTreeMap<(String, String), usize> = BTreeMap::new();
        for seed in 0..300u64 {
            let mut rng = Rng::new(0x5eed_1000 + seed);
            let i = gen_case(&mut rng);
            let op = format!("{:?}", i.ffi());
            let out = rust(&i).expect("harness");
            let kind = if let Some(d) = &out.deferred {
                format!("deferred:{}", d.symbol)
            } else {
                let st = out.values.get("status").and_then(|v| v.first()).copied();
                let found = out.values.get("out").and_then(|v| v.first()).copied();
                format!(
                    "status:{} first:{}",
                    st.unwrap_or(f64::NAN),
                    found.unwrap_or(f64::NAN)
                )
            };
            *tally.entry((op, kind)).or_default() += 1;
        }
        for ((op, kind), n) in &tally {
            println!("{op:8} {kind:60} {n}");
        }
        let count = |op: &str, kind: &str| {
            tally
                .iter()
                .filter(|((o, k), _)| o == op && k.starts_with(kind))
                .map(|(_, n)| *n)
                .sum::<usize>()
        };
        assert!(
            count("CrInit", "status:0") > 40,
            "too few successful CrInit cases"
        );
        assert!(
            count("Guess", "status:0 first:1") > 20,
            "too few found Guess cases"
        );
    }
}
