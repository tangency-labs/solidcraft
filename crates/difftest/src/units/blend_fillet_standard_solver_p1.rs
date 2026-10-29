//! Runner for unit `blend.fillet_standard_solver_p1`: the circular and blend-curve
//! cross section `CreateSurfaceFromCurves`, `SmLinearCrossSectionFSG::CreateSurface`,
//! `SmCurveBasedFS::EvaluateBaseCurve`, `SmSurfaceSurfaceFS::SetupOffsetExtension`, the
//! `SmConstantRadiusFS` surface constructor, `SetupOffsetValues` and `LoadJacobian`, the
//! `SmConstantDistanceFS` `SetupOffsetValues`, `LoadJacobian` and `LoadInitialValues`
//! and the `SmVariableRadiusFS` queries and `SetupOffsetValues`.
//!
//! C++: `solidcore_ffi::units::blend_fillet_standard_solver_p1` (upstream
//! `source/SMLib/src/SmFilletStandardSolver.cpp`). Rust:
//! `solidcore::blend::fillet_standard_solver::p1`.
//!
//! A case is one op with its inputs. Both sides produce the same flat record (layout in
//! the FFI module docs), compared as `status` (`0` = `SM_SUCCESS`, else the `SmStatus`:
//! `1001` `SM_ERR`, `1007` `SM_ERR_INVALID_INPUT`; `-1` on the Rust side for an error
//! upstream cannot produce) and `out` (the rest). A C++ record starting with `-1` is an
//! input the shim's dry run rejects (upstream would read out of bounds or loop forever),
//! recorded as rejected: ruled or revolved circular sections needing the second pass,
//! blend-curve skins through point sections or with `G1` derivative surfaces.
//!
//! The generated generator cases build fillet geometry exactly: rails offset from a
//! centre line (line, circular arc, free-form) by constant vectors (equal radii: circular
//! sections), with radius differences below the tolerance, below four times it, below a
//! tenth (blend curves) and above (skipped sections), touching rails (point sections),
//! one or two knot rails, different knot counts, tolerances from 0 up, flags of every
//! generator option and scales from `1e-6` to `1e6`. The solver ops run on B-spline
//! surfaces and planes (real `SmPlane`s), B-spline, line (`SmLine`) and Hermite curves.
//! The offset surfaces are the ported `SmOffsetSurface` (`surface.offset_surface`) with
//! upstream's default `SM_SS_UNKNOWN` singularities on both sides. The variable-radius
//! `SetupOffsetValues` is deferred on the Rust side while the fillet laws
//! (`blend.fillet_executive_p1`) are unported. Every run has at least
//! [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::blend::fillet_custom_solver::{OffsetSurface, SurfaceId};
use solidcore::blend::fillet_geom::p1::{FilletBrep, FilletSolverId};
use solidcore::blend::fillet_solver::p1::{FilletSurfaceGenerator, OffsetSurfaceSlot};
use solidcore::blend::fillet_standard_solver::p1::{
    constant_radius_from_edgeuse, constant_radius_is_kind_of, BlendCurveCrossSectionFsg,
    BoundaryTrimmingType, CircularCrossSectionFsg, ConstantDistanceFs, ConstantRadiusFs,
    CurveBasedFs, FilletLaw, FilletLawKind, FilletSolver, InitialSurface, LinearCrossSectionFsg,
    RailSurface, SurfaceSurfaceFs, VariableRadiusFs, CONSTANT_RADIUS_FS_TYPE,
    CONSTANT_RADIUS_FS_TYPE_STRING,
};
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::curve::line::Line;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::{Extent1d, ExtentNd};
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::surface_intersector::p1::TsectPnt;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::p1::NurbCircleParam;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::offset_surface::SS_UNKNOWN;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::{Brep, EdgeuseId, FaceId};
use solidcore::SolidError;
use solidcore_ffi::units::blend_fillet_standard_solver_p1::{self as ffi, Op};

use super::topology_edgeuse_p1::{
    brep_block_of, gen_brep, path_of, sel, BrepIn, FILES,
};
use super::topology_edgeuse_p2::{inexact_geometry, put_eu_state, Setup};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 480;

/// `blend.fillet_standard_solver_p1` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

const DEG: f64 = std::f64::consts::PI / 180.0;

// ---- inputs -----------------------------------------------------------------------

/// An inline 3D B-spline curve: control points as stored (`[x, y, z, w]`), degree,
/// knots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct InlineCrv {
    pub(crate) cpts: Vec<[f64; 4]>,
    pub(crate) p: usize,
    pub(crate) u: Vec<f64>,
}

/// A curve of any kind the shim builds.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CurveIn {
    Bspline {
        #[serde(default)]
        oob: bool,
        c: InlineCrv,
    },
    Line {
        start: [f64; 3],
        end: [f64; 3],
    },
    Hermite {
        p1: [f64; 3],
        d1: [f64; 3],
        p2: [f64; 3],
        d2: [f64; 3],
    },
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Sides {
    s1: SrfIn,
    uv1: InlineCrv,
    s2: SrfIn,
    uv2: InlineCrv,
}

/// The constant-radius solver block.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CrIn {
    a: SrfIn,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    b: Option<SrfIn>,
    r: [f64; 2],
    o: [bool; 2],
}

/// The constant-distance solver block.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CdIn {
    a: SrfIn,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    b: Option<SrfIn>,
    d: [f64; 2],
    o: [f64; 2],
    dist: f64,
    ext: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum LawIn {
    Linear { r0: f64, r1: f64 },
    Bspline { c: InlineCrv, map: [f64; 2] },
}

/// The variable-radius solver block.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct VrIn {
    a: SrfIn,
    b: SrfIn,
    o: [f64; 2],
    center: CurveIn,
    law: LawIn,
    orient: bool,
    fsg: usize,
}

/// The `LoadJacobian` arguments.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct JacIn {
    rail: usize,
    ne: usize,
    np: usize,
    sel1: usize,
    off1: usize,
    sel2: usize,
    off2: usize,
    x: Vec<f64>,
    f: Vec<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    jac: Option<[usize; 2]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    Circ {
        approx: bool,
        tolpct: f64,
        mirror: bool,
        complement: bool,
        tol: f64,
        g1: bool,
        center: InlineCrv,
        rail1: InlineCrv,
        rail2: InlineCrv,
    },
    Blend {
        cont: usize,
        scale: f64,
        tol: f64,
        g1: bool,
        center: InlineCrv,
        rail1: InlineCrv,
        rail2: InlineCrv,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sides: Option<Box<Sides>>,
    },
    Linear {
        rail1: InlineCrv,
        rail2: InlineCrv,
    },
    Base {
        curve: CurveIn,
        calls: Vec<(f64, usize)>,
        kinds: Vec<i64>,
    },
    SsExt {
        ext: f64,
        surface: SrfIn,
        domain: [f64; 4],
    },
    Cr {
        solver: CrIn,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        set: Option<[f64; 2]>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        setup: Option<f64>,
        kinds: Vec<i64>,
    },
    CrJac {
        solver: CrIn,
        jac: JacIn,
    },
    CdSetup {
        solver: CdIn,
        user: f64,
        kinds: Vec<i64>,
    },
    CdJac {
        solver: CdIn,
        jac: JacIn,
    },
    CdInit {
        solver: CdIn,
        do2: bool,
        rail: usize,
        sel1: usize,
        s1_index: usize,
        s2_index: usize,
        intervals: Vec<[f64; 2]>,
        per: Vec<bool>,
        guess: Vec<f64>,
    },
    Vr {
        solver: VrIn,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        set: Option<[f64; 2]>,
    },
    VrSetup {
        solver: VrIn,
        user: f64,
    },
    /// The `SmConstantRadiusFS` edge-use constructor on a `topology.edgeuse_p1` B-rep.
    CrEu {
        brep: BrepIn,
        /// Edge use selector (modulo the edge use count).
        eu: i64,
        tol: f64,
        ang: f64,
        tan: f64,
        r: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        r2: Option<f64>,
        /// UV fractions of the offset surfaces' natural domains where they are evaluated.
        fu: f64,
        fv: f64,
    },
}

impl Inputs {
    /// The shim op (`None` for the edge-use constructor, which has its own entry point).
    fn ffi(&self) -> Option<Op> {
        Some(match self {
            Inputs::CrEu { .. } => return None,
            Inputs::Circ { .. } => Op::Circ,
            Inputs::Blend { .. } => Op::Blend,
            Inputs::Linear { .. } => Op::Linear,
            Inputs::Base { .. } => Op::Base,
            Inputs::SsExt { .. } => Op::SsExt,
            Inputs::Cr { .. } => Op::Cr,
            Inputs::CrJac { .. } => Op::CrJac,
            Inputs::CdSetup { .. } => Op::CdSetup,
            Inputs::CdJac { .. } => Op::CdJac,
            Inputs::CdInit { .. } => Op::CdInit,
            Inputs::Vr { .. } => Op::Vr,
            Inputs::VrSetup { .. } => Op::VrSetup,
        })
    }

    /// A label of the op for tallies.
    #[cfg(test)]
    fn label(&self) -> String {
        self.ffi()
            .map_or_else(|| "CrEu".to_string(), |op| format!("{op:?}"))
    }
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

/// The edge-use shims' `put_default_geom`: the constructor's default fillet geometry
/// (stored in `pseudo`) as `count solver_is_fs` and per rail `own` (its own offset copy,
/// installed by `FinishConstruction`) and with `own` `offset base_is_face_surface status
/// P[3]` at the UV fractions `(fu, fv)` of the copy's natural domain. `faces[i]` is rail
/// `i`'s face.
#[allow(clippy::too_many_arguments)]
pub(crate) fn put_default_geom(
    k: &Kernel,
    v: &mut Vec<f64>,
    fs: &FilletSolver,
    id: FilletSolverId,
    pseudo: &FilletBrep,
    faces: [Option<FaceId>; 2],
    fu: f64,
    fv: f64,
) -> Result<(), SolidError> {
    v.push(fs.fillet_geoms.len() as f64);
    let Some(&g) = fs.fillet_geoms.first() else {
        return Ok(());
    };
    let fg = pseudo.geom(g)?;
    v.push(flag(fg.fillet_solver() == Some(id)));
    for (own, face) in fg.off_surfs.iter().zip(faces) {
        let Some(s) = own else {
            v.push(0.0);
            continue;
        };
        v.push(1.0);
        v.push(s.offset_distance());
        v.push(flag(face.is_some_and(|f| s.base_id == SurfaceId::Face(f))));
        let d = s.surface.natural_uv_domain();
        let uv = Point2::new(
            d.min().x + fu * (d.max().x - d.min().x),
            d.min().y + fv * (d.max().y - d.min().y),
        );
        match s.surface.evaluate_point(k, uv) {
            Ok(p) => v.extend([0.0, p.x, p.y, p.z]),
            Err(e) if e.is_unported() => return Err(e),
            Err(SolidError::Upstream { code, .. }) => v.extend([f64::from(code), 0.0, 0.0, 0.0]),
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

// ---- C++ side -----------------------------------------------------------------------

pub(crate) fn push_crv(o: &mut Vec<f64>, c: &InlineCrv) {
    let n = c.cpts.len().saturating_sub(1);
    o.extend([n as f64, c.p as f64, c.u.len().saturating_sub(1) as f64]);
    for p in &c.cpts {
        o.extend_from_slice(p);
    }
    o.extend_from_slice(&c.u);
}

fn push_curve(o: &mut Vec<f64>, c: &CurveIn) {
    match c {
        CurveIn::Bspline { oob, c } => {
            o.extend([0.0, flag(*oob)]);
            push_crv(o, c);
        }
        CurveIn::Line { start, end } => {
            o.push(1.0);
            o.extend_from_slice(start);
            o.extend_from_slice(end);
        }
        CurveIn::Hermite { p1, d1, p2, d2 } => {
            o.push(2.0);
            for v in [p1, d1, p2, d2] {
                o.extend_from_slice(v);
            }
        }
    }
}

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

fn push_kinds(o: &mut Vec<f64>, kinds: &[i64]) {
    o.push(kinds.len() as f64);
    o.extend(kinds.iter().map(|&k| k as f64));
}

fn push_cr(o: &mut Vec<f64>, s: &CrIn) -> Result<(), HarnessError> {
    o.push(flag(s.b.is_none()));
    push_srf(o, &s.a)?;
    if let Some(b) = &s.b {
        push_srf(o, b)?;
    }
    o.extend([s.r[0], s.r[1], flag(s.o[0]), flag(s.o[1])]);
    Ok(())
}

fn push_cd(o: &mut Vec<f64>, s: &CdIn) -> Result<(), HarnessError> {
    o.push(flag(s.b.is_none()));
    push_srf(o, &s.a)?;
    if let Some(b) = &s.b {
        push_srf(o, b)?;
    }
    o.extend([s.d[0], s.d[1], s.o[0], s.o[1], s.dist, s.ext]);
    Ok(())
}

fn push_vr(o: &mut Vec<f64>, s: &VrIn) -> Result<(), HarnessError> {
    push_srf(o, &s.a)?;
    push_srf(o, &s.b)?;
    o.extend(s.o);
    push_curve(o, &s.center);
    match &s.law {
        LawIn::Linear { r0, r1 } => o.extend([0.0, *r0, *r1]),
        LawIn::Bspline { c, map } => {
            o.push(1.0);
            push_crv(o, c);
            o.extend(map);
        }
    }
    o.extend([flag(s.orient), s.fsg as f64]);
    Ok(())
}

fn push_jac(o: &mut Vec<f64>, j: &JacIn) {
    o.extend([j.rail, j.ne, j.np, j.sel1, j.off1, j.sel2, j.off2].map(|v| v as f64));
    o.push(j.x.len() as f64);
    o.extend_from_slice(&j.x);
    o.push(j.f.len() as f64);
    o.extend_from_slice(&j.f);
    let [rows, cols] = j.jac.unwrap_or([0, 0]);
    o.extend([flag(j.jac.is_some()), rows as f64, cols as f64]);
}

#[allow(clippy::too_many_lines)]
fn cpp_input(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    match i {
        Inputs::Circ {
            approx,
            tolpct,
            mirror,
            complement,
            tol,
            g1,
            center,
            rail1,
            rail2,
        } => {
            o.extend([
                flag(*approx),
                *tolpct,
                flag(*mirror),
                flag(*complement),
                *tol,
                flag(*g1),
            ]);
            push_crv(&mut o, center);
            push_crv(&mut o, rail1);
            push_crv(&mut o, rail2);
        }
        Inputs::Blend {
            cont,
            scale,
            tol,
            g1,
            center,
            rail1,
            rail2,
            sides,
        } => {
            o.extend([*cont as f64, *scale, *tol, flag(*g1)]);
            push_crv(&mut o, center);
            push_crv(&mut o, rail1);
            push_crv(&mut o, rail2);
            o.push(flag(sides.is_some()));
            if let Some(s) = sides {
                push_srf(&mut o, &s.s1)?;
                push_crv(&mut o, &s.uv1);
                push_srf(&mut o, &s.s2)?;
                push_crv(&mut o, &s.uv2);
            }
        }
        Inputs::Linear { rail1, rail2 } => {
            push_crv(&mut o, rail1);
            push_crv(&mut o, rail2);
        }
        Inputs::Base {
            curve,
            calls,
            kinds,
        } => {
            push_curve(&mut o, curve);
            o.push(calls.len() as f64);
            for (t, n) in calls {
                o.extend([*t, *n as f64]);
            }
            push_kinds(&mut o, kinds);
        }
        Inputs::SsExt {
            ext,
            surface,
            domain,
        } => {
            o.push(*ext);
            push_srf(&mut o, surface)?;
            o.extend_from_slice(domain);
            push_kinds(&mut o, &[]);
        }
        Inputs::Cr {
            solver,
            set,
            setup,
            kinds,
        } => {
            push_cr(&mut o, solver)?;
            let [d0, d1] = set.unwrap_or([0.0, 0.0]);
            o.extend([flag(set.is_some()), d0, d1]);
            o.extend([flag(setup.is_some()), setup.unwrap_or(0.0)]);
            push_kinds(&mut o, kinds);
        }
        Inputs::CrJac { solver, jac } => {
            push_cr(&mut o, solver)?;
            push_jac(&mut o, jac);
        }
        Inputs::CdSetup {
            solver,
            user,
            kinds,
        } => {
            push_cd(&mut o, solver)?;
            o.push(*user);
            push_kinds(&mut o, kinds);
        }
        Inputs::CdJac { solver, jac } => {
            push_cd(&mut o, solver)?;
            push_jac(&mut o, jac);
        }
        Inputs::CdInit {
            solver,
            do2,
            rail,
            sel1,
            s1_index,
            s2_index,
            intervals,
            per,
            guess,
        } => {
            push_cd(&mut o, solver)?;
            o.extend([
                flag(*do2),
                *rail as f64,
                *sel1 as f64,
                *s1_index as f64,
                *s2_index as f64,
            ]);
            o.push(intervals.len() as f64);
            for iv in intervals {
                o.extend_from_slice(iv);
            }
            o.push(per.len() as f64);
            o.extend(per.iter().map(|&b| flag(b)));
            o.push(guess.len() as f64);
            o.extend_from_slice(guess);
        }
        Inputs::Vr { solver, set } => {
            push_vr(&mut o, solver)?;
            let [e0, e1] = set.unwrap_or([0.0, 0.0]);
            o.extend([flag(set.is_some()), e0, e1]);
            push_kinds(&mut o, &[]);
        }
        Inputs::VrSetup { solver, user } => {
            push_vr(&mut o, solver)?;
            o.push(*user);
        }
        Inputs::CrEu { .. } => return Err(harness("the edge-use op has its own input")),
    }
    Ok(o)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let Some(op) = i.ffi() else {
        return cpp_edgeuse(i);
    };
    let input = cpp_input(i)?;
    let flat = ffi::eval(op, &input).map_err(|e| harness(e.to_string()))?;
    if flat.first().copied() == Some(-1.0) {
        return Ok(Output::rejected(
            "the shim's dry run: upstream would read out of bounds (ruled/revolved \
             circular sections with a second pass, or a blend-curve skin through a point \
             section) or loop forever",
        ));
    }
    Ok(to_output(&flat))
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

pub(crate) fn bcurve_of(c: &InlineCrv) -> Result<BSplineCurve, HarnessError> {
    let pw = c
        .cpts
        .iter()
        .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
        .collect();
    let cur =
        NurbCurve::new(pw, c.p, c.u.clone()).map_err(|e| harness(format!("curve input: {e}")))?;
    Ok(BSplineCurve::from_nurb(cur, 3, false))
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn curve_of(c: &CurveIn) -> Result<Curve, HarnessError> {
    Ok(match c {
        CurveIn::Bspline { oob, c } => {
            let mut b = bcurve_of(c)?;
            b.set_out_of_bounds_enabled(*oob);
            Curve::from(b)
        }
        CurveIn::Line { start, end } => Curve::from(Line::from_points(v3(*start), v3(*end), 3)),
        CurveIn::Hermite { p1, d1, p2, d2 } => {
            Curve::from(HermiteCurve::new(v3(*p1), v3(*d1), v3(*p2), v3(*d2), 3))
        }
    })
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
pub(crate) fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

pub(crate) fn dump_curve(o: &mut Vec<f64>, c: Option<&BSplineCurve>) {
    let Some(cur) = c.and_then(BSplineCurve::gw_nurb) else {
        o.push(0.0);
        return;
    };
    o.push(1.0);
    o.extend([
        cur.n() as f64,
        cur.degree() as f64,
        cur.knots().len().saturating_sub(1) as f64,
    ]);
    for p in cur.cpts() {
        o.extend([p.x, p.y, p.z, p.w]);
    }
    o.extend_from_slice(cur.knots());
}

fn dump_surface(o: &mut Vec<f64>, s: &BSplineSurface) {
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

/// `GetType`, `len chars` of `GetTypeString` and `IsKindOf` per code.
fn dump_types(
    o: &mut Vec<f64>,
    code: i64,
    string: &str,
    is_kind_of: impl Fn(i64) -> bool,
    kinds: &[i64],
) {
    o.push(code as f64);
    o.push(string.len() as f64);
    o.extend(string.bytes().map(f64::from));
    o.extend(kinds.iter().map(|&k| flag(is_kind_of(k))));
}

/// A record or the deferral of an unported callee.
type Rec = Result<Vec<f64>, Box<Output>>;

fn fail(e: &SolidError) -> Result<f64, Box<Output>> {
    match Output::unported(e) {
        Some(out) => Err(Box::new(out)),
        None => Ok(status(e)),
    }
}

fn rust_generator(i: &Inputs) -> Result<Rec, HarnessError> {
    let k = Kernel::default();
    let mut o = Vec::new();
    let (res, curves) = match i {
        Inputs::Circ {
            approx,
            tolpct,
            mirror,
            complement,
            tol,
            g1,
            center,
            rail1,
            rail2,
        } => {
            let fsg = CircularCrossSectionFsg {
                approximate_arcs: *approx,
                tol_percent_radius: *tolpct,
                mirror: *mirror,
                complement: *complement,
            };
            // the shim's centre line is a plain SmBSplineCurve
            let mut c = Curve::from(bcurve_of(center)?);
            let mut r1 = bcurve_of(rail1)?;
            let mut r2 = bcurve_of(rail2)?;
            let res = fsg.create_surface_from_curves(&k, &mut c, &mut r1, &mut r2, *tol, *g1);
            let CurveKind::BSpline(c) = c.into_kind() else {
                return Err(HarnessError::new("centre line changed kind"));
            };
            (res, [c, r1, r2])
        }
        Inputs::Blend {
            cont,
            scale,
            tol,
            g1,
            center,
            rail1,
            rail2,
            sides,
        } => {
            let fsg = BlendCurveCrossSectionFsg {
                continuity: *cont as u32,
                blend_scale: *scale,
                at_thirds: false,
            };
            let mut c = bcurve_of(center)?;
            let mut r1 = bcurve_of(rail1)?;
            let mut r2 = bcurve_of(rail2)?;
            let built = match sides {
                Some(s) => Some((
                    surface_of(&s.s1)?,
                    bcurve_of(&s.uv1)?,
                    surface_of(&s.s2)?,
                    bcurve_of(&s.uv2)?,
                )),
                None => None,
            };
            let (side1, side2) = match &built {
                Some((s1, uv1, s2, uv2)) => (
                    RailSurface {
                        surface: Some(s1),
                        uv_curve: Some(uv1),
                    },
                    RailSurface {
                        surface: Some(s2),
                        uv_curve: Some(uv2),
                    },
                ),
                None => (
                    RailSurface {
                        surface: None,
                        uv_curve: None,
                    },
                    RailSurface {
                        surface: None,
                        uv_curve: None,
                    },
                ),
            };
            let res = fsg
                .create_surface_from_curves(&k, &mut c, &mut r1, &mut r2, *tol, *g1, side1, side2);
            (res, [c, r1, r2])
        }
        _ => return Err(harness("not a generator op")),
    };
    match &res {
        Ok(_) => o.push(0.0),
        Err(e) => match fail(e) {
            Ok(s) => o.push(s),
            Err(out) => return Ok(Err(out)),
        },
    }
    for c in &curves {
        dump_curve(&mut o, Some(c));
    }
    if let Ok(s) = &res {
        dump_surface(&mut o, s);
    }
    Ok(Ok(o))
}

/// Placeholder surfaces of solvers whose surfaces the op never reads (the shim's
/// bilinear surfaces).
fn bilinear(z: bool) -> Result<Surface, HarnessError> {
    let p = |x: f64, y: f64, zz: f64| Vec3::new(x, y, zz);
    let s = if z {
        BSplineSurface::create_bilinear_surface(
            p(0.0, 0.0, 0.0),
            p(0.0, 0.0, 1.0),
            p(1.0, 0.0, 0.0),
            p(1.0, 0.0, 1.0),
        )
    } else {
        BSplineSurface::create_bilinear_surface(
            p(0.0, 0.0, 0.0),
            p(1.0, 0.0, 0.0),
            p(0.0, 1.0, 0.0),
            p(1.0, 1.0, 0.0),
        )
    }
    .map_err(|e| harness(e.to_string()))?;
    Ok(Surface::from(s))
}

fn he(e: &SolidError) -> HarnessError {
    harness(e.to_string())
}

/// The `LoadJacobian` matrix of a case, bounded as the FFI validator bounds it (a size
/// above `MAX_ARRAY` is refused there, so it is never allocated here).
fn jac_matrix(jac: &JacIn) -> Result<Option<solidcore::geom::matrix::Matrix>, HarnessError> {
    match jac.jac {
        Some([r, c]) if r > ffi::MAX_ARRAY || c > ffi::MAX_ARRAY => {
            Err(harness("jacobian: matrix size above the FFI bound"))
        }
        Some([r, c]) => Ok(Some(solidcore::geom::matrix::Matrix::new(r, c))),
        None => Ok(None),
    }
}

/// `new SmOffsetSurface(d, surface)` (upstream's default `SM_SS_UNKNOWN`).
fn offset_of(k: &Kernel, d: f64, s: Surface, id: SurfaceId) -> Result<OffsetSurface, HarnessError> {
    OffsetSurface::new(k, d, s, id, SS_UNKNOWN).map_err(|e| he(&e))
}

fn cr_of(k: &Kernel, s: &CrIn) -> Result<ConstantRadiusFs, HarnessError> {
    let a = surface_of(&s.a)?;
    let (b, b_id) = match &s.b {
        Some(b) => (surface_of(b)?, SurfaceId::Free(1)),
        None => (a.clone(), SurfaceId::Free(0)),
    };
    // (the constructor's fillet geometry is not compared here)
    ConstantRadiusFs::from_surfaces(
        k,
        &mut FilletBrep::new(),
        FilletSolverId(0),
        1.0e-6,
        30.0 * DEG,
        2.0 * DEG,
        s.r,
        [(a, SurfaceId::Free(0)), (b, b_id)],
        s.o,
    )
    .map_err(|e| he(&e))
}

fn cd_of(k: &Kernel, s: &CdIn) -> Result<ConstantDistanceFs, HarnessError> {
    let a = surface_of(&s.a)?;
    let (b, b_id) = match &s.b {
        Some(b) => (surface_of(b)?, SurfaceId::Free(1)),
        None => (a.clone(), SurfaceId::Free(0)),
    };
    let mut s0 = offset_of(k, s.d[0], a, SurfaceId::Free(0))?;
    let mut s1 = offset_of(k, s.d[1], b, b_id)?;
    s0.set_allow_extension(true);
    s1.set_allow_extension(true);
    let mut solver = FilletSolver::new(1.0e-6, 30.0 * DEG, 2.0 * DEG);
    solver.surfaces = slots(s0, s1);
    solver.orientations = s.o;
    solver.surface_extension_factor = s.ext;
    Ok(ConstantDistanceFs {
        solver,
        distance: s.dist,
    })
}

fn vr_of(k: &Kernel, s: &VrIn) -> Result<VariableRadiusFs, HarnessError> {
    let center = curve_of(&s.center)?;
    let law_interval = center.natural_interval();
    let mut curve_based = CurveBasedFs::new(1.0e-6, 30.0 * DEG, 2.0 * DEG);
    let a = surface_of(&s.a)?;
    let b = surface_of(&s.b)?;
    let mut s0 = offset_of(k, 0.0, a, SurfaceId::Free(0))?;
    let mut s1 = offset_of(k, 0.0, b, SurfaceId::Free(1))?;
    s0.set_allow_extension(true);
    s1.set_allow_extension(true);
    curve_based.solver.surfaces = slots(s0, s1);
    curve_based.solver.orientations = s.o;
    curve_based
        .solver
        .set_fillet_surface_generator(Some(match s.fsg {
            1 => FilletSurfaceGenerator::Linear,
            2 => FilletSurfaceGenerator::circular_default(),
            _ => FilletSurfaceGenerator::blend_curve_default(),
        }));
    curve_based.original = Some(center);
    let law = match &s.law {
        LawIn::Linear { r0, r1 } => FilletLaw::new(FilletLawKind::Linear {
            start_radius: *r0,
            end_radius: *r1,
        }),
        LawIn::Bspline { c, map } => {
            // SmBSplineFilletLaw(pCurve, crEdgeMap) (unit blend.fillet_executive_p2); the
            // maps here are never degenerate, so its [0, 1] fallback is not needed
            let mut law = FilletLaw::new(FilletLawKind::BSpline {
                law_curve: bcurve_of(c)?,
                extended_law: None,
            });
            law.edge_map = Extent1d::from_min_max(map[0], map[1]);
            law
        }
    };
    Ok(VariableRadiusFs {
        curve_based,
        law,
        law_orientation: s.orient,
        law_interval,
    })
}

fn tsect(user: f64) -> TsectPnt {
    let mut t = TsectPnt::new();
    if let Some(slot) = t.user_doubles.first_mut() {
        *slot = user;
    }
    t
}

/// The two rail surfaces of an `SmFilletSolver` base (distinct objects: keys 0 and 1).
fn slots(s0: OffsetSurface, s1: OffsetSurface) -> [Option<OffsetSurfaceSlot>; 2] {
    [
        Some(OffsetSurfaceSlot {
            key: 0,
            surface: s0,
        }),
        Some(OffsetSurfaceSlot {
            key: 1,
            surface: s1,
        }),
    ]
}

/// The offset distances of the rail surfaces of an `SmFilletSolver` base.
fn offsets(s: &[Option<OffsetSurfaceSlot>; 2]) -> [f64; 2] {
    slot_offsets(s)
}

/// The offset distances of the rail surfaces of an `SmFilletSolver` base.
fn slot_offsets(s: &[Option<OffsetSurfaceSlot>; 2]) -> [f64; 2] {
    [0, 1].map(|i| {
        s.get(i)
            .and_then(Option::as_ref)
            .map_or(f64::NAN, |slot| slot.surface.offset_distance())
    })
}

#[allow(clippy::too_many_lines)]
fn rust_record(i: &Inputs) -> Result<Rec, HarnessError> {
    let k = Kernel::default();
    let mut o = Vec::new();
    match i {
        Inputs::Circ { .. } | Inputs::Blend { .. } => return rust_generator(i),
        Inputs::Linear { rail1, rail2 } => {
            let r1 = bcurve_of(rail1)?;
            let r2 = bcurve_of(rail2)?;
            match LinearCrossSectionFsg.create_surface(&r1, &r2) {
                Ok(s) => {
                    o.push(0.0);
                    dump_surface(&mut o, &s);
                }
                Err(e) => match fail(&e) {
                    Ok(s) => o.push(s),
                    Err(out) => return Ok(Err(out)),
                },
            }
        }
        Inputs::Base {
            curve,
            calls,
            kinds,
        } => {
            let mut fs = CurveBasedFs::new(1.0e-6, 30.0 * DEG, 2.0 * DEG);
            fs.set_original_curve(Some(curve_of(curve)?));
            o.push(0.0);
            for &(t, n) in calls {
                // (bounded as the FFI validator bounds it: `Curve::evaluate` allocates
                // `n + 1` vectors)
                if n > ffi::MAX_DERIV {
                    return Err(harness("base: derivative count above the FFI bound"));
                }
                match fs.evaluate_base_curve(&k, t, n) {
                    Ok(pv) => {
                        o.push(0.0);
                        for v in pv {
                            o.extend([v.x, v.y, v.z]);
                        }
                    }
                    Err(e) => match fail(&e) {
                        Ok(s) => o.push(s),
                        Err(out) => return Ok(Err(out)),
                    },
                }
            }
            dump_curve(&mut o, fs.extended.as_ref());
            o.push(f64::from(fs.solver_type() as i32));
            dump_types(
                &mut o,
                fs.type_code(),
                fs.type_string(),
                |t| fs.is_kind_of(t),
                kinds,
            );
        }
        Inputs::SsExt {
            ext,
            surface,
            domain,
        } => {
            let mut base = ConstantRadiusFs::from_surfaces(
                &k,
                &mut FilletBrep::new(),
                FilletSolverId(0),
                1.0e-6,
                30.0 * DEG,
                2.0 * DEG,
                [1.0, 1.0],
                [
                    (bilinear(false)?, SurfaceId::Free(10)),
                    (bilinear(true)?, SurfaceId::Free(11)),
                ],
                [true, true],
            )
            .map_err(|e| he(&e))?;
            base.solver.surface_extension_factor = *ext;
            let fs = SurfaceSurfaceFs {
                base,
                uv_guesses: [Point2::new(0.5, 0.5), Point2::new(0.5, 0.5)],
                trim_type: BoundaryTrimmingType::None,
            };
            let s = surface_of(surface)?;
            // (`SmExtent2d(u0, v0, u1, v1)`: stored as given, as the shim builds it)
            let mut d = Extent2d::from_bounds(domain[0], domain[1], domain[2], domain[3]);
            match fs.setup_offset_extension(&k, &s, &mut d) {
                Ok(()) => o.push(0.0),
                // (no status upstream: a void function)
                Err(e) => match fail(&e) {
                    Ok(s) => o.push(s),
                    Err(out) => return Ok(Err(out)),
                },
            }
            o.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
            o.push(f64::from(fs.solver_type() as i32));
            dump_types(&mut o, fs.type_code(), fs.type_string(), |_| false, &[]);
        }
        Inputs::Cr {
            solver,
            set,
            setup,
            kinds,
        } => {
            let mut fs = cr_of(&k, solver)?;
            if let Some([d0, d1]) = set {
                for (s, d) in fs.solver.surfaces.iter_mut().zip([d0, d1]) {
                    if let Some(s) = s {
                        s.surface.set_offset_distance(*d);
                    }
                }
            }
            let mut dist = -999.0;
            let mut st = 0.0;
            if let Some(user) = setup {
                match fs.setup_offset_values(&tsect(*user)) {
                    Ok(d) => dist = d,
                    Err(e) => match fail(&e) {
                        Ok(s) => st = s,
                        Err(out) => return Ok(Err(out)),
                    },
                }
            }
            o.extend([st, dist]);
            let rad = |i| fs.fillet_radius(i).map_err(|e| harness(e.to_string()));
            o.extend([rad(0)?, rad(1)?]);
            o.extend(fs.solver.orientations);
            o.extend(slot_offsets(&fs.solver.surfaces));
            o.extend([fs.solver.conversion_tol, fs.solver.surface_extension_factor]);
            o.push(fs.jacobian_size() as f64);
            o.push(flag(fs.offset_radii_can_change()));
            o.push(f64::from(fs.constant_radius_solver_type() as i32));
            dump_types(
                &mut o,
                CONSTANT_RADIUS_FS_TYPE,
                CONSTANT_RADIUS_FS_TYPE_STRING,
                constant_radius_is_kind_of,
                kinds,
            );
        }
        Inputs::CrJac { solver, jac } => {
            let mut fs = cr_of(&k, solver)?;
            let ids = |sel: usize| match sel {
                1 | 3 => Some(SurfaceId::Free(0)),
                2 => Some(if solver.b.is_some() {
                    SurfaceId::Free(1)
                } else {
                    SurfaceId::Free(0)
                }),
                4 => Some(SurfaceId::Free(2)),
                _ => None,
            };
            let (mut ne, mut np) = (jac.ne, jac.np);
            let mut offs = [jac.off1, jac.off2];
            let mut f = jac.f.clone();
            let mut m = jac_matrix(jac)?;
            let res = fs.load_jacobian(&k, 
                &mut ne,
                &mut np,
                jac.rail,
                [ids(jac.sel1), ids(jac.sel2)],
                &mut offs,
                &jac.x,
                &mut f,
                m.as_mut(),
            );
            let found = match &res {
                Ok(b) => {
                    o.push(0.0);
                    *b
                }
                Err(e) => match fail(e) {
                    Ok(s) => {
                        o.push(s);
                        false
                    }
                    Err(out) => return Ok(Err(out)),
                },
            };
            o.extend([
                ne as f64,
                np as f64,
                offs[0] as f64,
                offs[1] as f64,
                flag(found),
            ]);
            o.extend_from_slice(&f);
            if let (Some(m), Some([rows, cols])) = (&m, jac.jac) {
                for r in 0..rows {
                    for c in 0..cols {
                        o.push(m.at(r, c).map_err(|e| harness(e.to_string()))?);
                    }
                }
            }
            o.extend(slot_offsets(&fs.solver.surfaces));
        }
        Inputs::CdSetup {
            solver,
            user,
            kinds,
        } => {
            let mut fs = cd_of(&k, solver)?;
            let (st, dist) = match fs.setup_offset_values(&tsect(*user)) {
                Ok(d) => (0.0, d),
                Err(e) => match fail(&e) {
                    Ok(s) => (s, -999.0),
                    Err(out) => return Ok(Err(out)),
                },
            };
            o.extend([st, dist]);
            o.extend(offsets(&fs.solver.surfaces));
            o.push(fs.distance());
            o.push(flag(fs.can_make_analytic()));
            o.push(fs.jacobian_size() as f64);
            o.push(f64::from(fs.solver_type() as i32));
            dump_types(
                &mut o,
                fs.type_code(),
                fs.type_string(),
                |t| fs.is_kind_of(t),
                kinds,
            );
        }
        Inputs::CdJac { solver, jac } => {
            let mut fs = cd_of(&k, solver)?;
            let ids = |sel: usize| match sel {
                1 | 3 => Some(SurfaceId::Free(0)),
                2 => Some(if solver.b.is_some() {
                    SurfaceId::Free(1)
                } else {
                    SurfaceId::Free(0)
                }),
                4 => Some(SurfaceId::Free(2)),
                _ => None,
            };
            let (mut ne, mut np) = (jac.ne, jac.np);
            let mut f = jac.f.clone();
            let mut m = jac_matrix(jac)?;
            let res = fs.load_jacobian(&k, 
                &mut ne,
                &mut np,
                jac.rail,
                [ids(jac.sel1), ids(jac.sel2)],
                [jac.off1, jac.off2],
                &jac.x,
                &mut f,
                m.as_mut(),
            );
            let found = match &res {
                Ok(b) => {
                    o.push(0.0);
                    *b
                }
                Err(e) => match fail(e) {
                    Ok(s) => {
                        o.push(s);
                        false
                    }
                    Err(out) => return Ok(Err(out)),
                },
            };
            o.extend([ne as f64, np as f64, flag(found)]);
            o.extend_from_slice(&f);
            if let (Some(m), Some([rows, cols])) = (&m, jac.jac) {
                for r in 0..rows {
                    for c in 0..cols {
                        o.push(m.at(r, c).map_err(|e| harness(e.to_string()))?);
                    }
                }
            }
            o.extend(offsets(&fs.solver.surfaces));
        }
        Inputs::CdInit {
            solver,
            do2,
            rail,
            sel1,
            s1_index,
            s2_index,
            intervals,
            per,
            guess,
        } => {
            let mut fs = cd_of(&k, solver)?;
            // (`GetSurface(0)` / `GetSurface(1)` are the solver's own surfaces, as the shim
            // passes them)
            let mut extra = match sel1 {
                1 | 2 => None,
                _ => Some(offset_of(
                    &k,
                    solver.d[0],
                    surface_of(&solver.a)?,
                    if *sel1 == 3 {
                        SurfaceId::Free(0)
                    } else {
                        SurfaceId::Free(2)
                    },
                )?),
            };
            let surface1 = match (sel1, extra.as_mut()) {
                (_, Some(s)) => InitialSurface::Other(s),
                (1, None) => InitialSurface::Rail(0),
                _ => InitialSurface::Rail(1),
            };
            let mut ivls = ExtentNd::new(intervals.len());
            for (n, [a, b]) in intervals.iter().enumerate() {
                *ivls.get_mut(n).ok_or_else(|| harness("interval index"))? =
                    Extent1d::from_min_max(*a, *b);
            }
            let mut per = per.clone();
            let mut guess = guess.clone();
            let res = fs.load_initial_values(
                &k, None, *do2, *rail, surface1, *s1_index, *s2_index, &mut ivls, &mut per,
                &mut guess,
            );
            match &res {
                Ok(v) => o.extend([
                    0.0,
                    v.surface2_rail.map_or(-1.0, |r| r as f64),
                    v.surface2_index as f64,
                ]),
                Err(e) => match fail(e) {
                    // (upstream leaves rpSurface2 NULL and the index unchanged on its errors)
                    Ok(s) => o.extend([s, -1.0, *s2_index as f64]),
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
            o.extend(offsets(&fs.solver.surfaces));
        }
        Inputs::Vr { solver, set } => {
            let mut fs = vr_of(&k, solver)?;
            if let Some([e0, e1]) = set {
                fs.set_fillet_law_interval(Extent1d::from_min_max(*e0, *e1));
            }
            let can = match fs.offset_radii_can_change() {
                Ok(b) => b,
                Err(e) => match fail(&e) {
                    Ok(s) => {
                        o.push(s);
                        return Ok(Ok(o));
                    }
                    Err(out) => return Ok(Err(out)),
                },
            };
            o.push(0.0);
            o.push(flag(can));
            o.push(flag(fs.fillet_law_orientation()));
            let ivl = fs.fillet_law_interval();
            o.extend([ivl.min(), ivl.max()]);
            o.push(1.0); // GetFilletLaw is the law given
            o.push(fs.jacobian_size() as f64);
            o.push(f64::from(fs.solver_type() as i32));
            dump_types(&mut o, fs.type_code(), fs.type_string(), |_| false, &[]);
        }
        Inputs::CrEu { .. } => return Err(harness("the edge-use op runs apart")),
        Inputs::VrSetup { solver, user } => {
            let mut fs = vr_of(&k, solver)?;
            match fs.setup_offset_values(&tsect(*user)) {
                Ok(d) => o.extend([0.0, d]),
                Err(e) => match fail(&e) {
                    Ok(s) => o.extend([s, -999.0]),
                    Err(out) => return Ok(Err(out)),
                },
            }
            o.extend(offsets(&fs.curve_based.solver.surfaces));
        }
    }
    Ok(Ok(o))
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    if i.ffi().is_none() {
        return rust_edgeuse(i);
    }
    Ok(match rust_record(i)? {
        Ok(flat) => to_output(&flat),
        Err(out) => *out,
    })
}

// ---- the edge-use constructor ------------------------------------------------------

/// The case's B-rep (the C++ side's imported from the shim's export, the Rust side's
/// read or built by the Rust kernel) and the selected edge use; `Err(output)` when the B-rep cannot be built or has no edge use (as unit
/// `topology.edgeuse_p3`'s runner).
fn eu_setup(brep: &BrepIn, eu: i64, own: bool) -> Result<Result<Setup, Output>, HarnessError> {
    let path = path_of(Some(brep))?;
    let block = brep_block_of(Some(brep), false, false)?;
    // with `own` (the Rust side) the B-rep is read or built by the Rust kernel, never
    // imported (`crate::own_reader::edgeuse_case`)
    let (c, brep, types) = match crate::own_reader::edgeuse_case(brep, false, false, own)? {
        Ok(x) => x,
        Err(o) => return Ok(Err(o)),
    };
    let n_eu = c.get(6).copied().unwrap_or(0);
    let s = sel(eu, n_eu);
    if s < 0 {
        return Ok(Err(Output::err("no_entity_of_that_kind")));
    }
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
    let Inputs::CrEu {
        brep,
        eu,
        tol,
        ang,
        tan,
        r,
        r2,
        fu,
        fv,
    } = i
    else {
        return Err(harness("not the edge-use op"));
    };
    let s = match eu_setup(brep, *eu, false)? {
        Ok(s) => s,
        Err(o) => return Ok(o),
    };
    let mut input = s.block.clone();
    input.extend([
        f64::from(s.eu.index()),
        *tol,
        *ang,
        *tan,
        *r,
        flag(r2.is_some()),
        r2.unwrap_or(0.0),
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
    r: f64,
    r2: Option<f64>,
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
    let mut pseudo = FilletBrep::new();
    let id = FilletSolverId(0);
    match constant_radius_from_edgeuse(&k, b, &mut pseudo, id, tol, ang, tan, r, eu, r2) {
        Ok(fs) => {
            v.push(0.0);
            for i in 0..2 {
                let e = fs.solver.edgeuses.get(i).copied().flatten();
                v.push(e.map_or(-1.0, |e| f64::from(e.index())));
                v.push(fs.fillet_radius(i)?);
                v.push(fs.solver.orientations.get(i).copied().unwrap_or(f64::NAN));
                let s = fs.surface(i)?;
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
                match s.surface.evaluate_point(&k, uv) {
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
            let mut faces = [None, None];
            for (face, e) in faces.iter_mut().zip(fs.solver.edgeuses) {
                if let Some(e) = e {
                    *face = eu1::face(b, e)?;
                }
            }
            put_default_geom(&k, &mut v, &fs.solver, id, &pseudo, faces, fu, fv)?;
        }
        Err(SolidError::Upstream { code, .. }) => v.push(f64::from(code)),
        Err(e) => return Err(e),
    }
    put_eu_state(&k, &mut v, b, Some(eu))?;
    put_eu_state(&k, &mut v, b, radial)?;
    Ok(v)
}

fn rust_edgeuse(i: &Inputs) -> Result<Output, HarnessError> {
    let Inputs::CrEu {
        brep,
        eu,
        tol,
        ang,
        tan,
        r,
        r2,
        fu,
        fv,
    } = i
    else {
        return Err(harness("not the edge-use op"));
    };
    let mut s = match eu_setup(brep, *eu, true)? {
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
    match rust_edgeuse_body(&mut s.brep, s.eu, *tol, *ang, *tan, *r, *r2, *fu, *fv) {
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
pub(crate) fn uniform_knots(n: usize, p: usize, a: f64, b: f64) -> Vec<f64> {
    let spans = n + 1 - p;
    let mut u = vec![a; p + 1];
    for i in 1..spans {
        u.push(a + (b - a) * i as f64 / spans as f64);
    }
    u.extend(std::iter::repeat_n(b, p + 1));
    u
}

fn crv_of(c: &BSplineCurve) -> Option<InlineCrv> {
    let cur = c.gw_nurb()?;
    Some(InlineCrv {
        cpts: cur.cpts().iter().map(|p| [p.x, p.y, p.z, p.w]).collect(),
        p: cur.degree(),
        u: cur.knots().to_vec(),
    })
}

fn translate(c: &InlineCrv, d: Vec3) -> InlineCrv {
    let mut out = c.clone();
    for p in &mut out.cpts {
        if p[3] == NOW {
            p[0] += d.x;
            p[1] += d.y;
            p[2] += d.z;
        } else {
            // weighted coordinates
            p[0] += d.x * p[3];
            p[1] += d.y * p[3];
            p[2] += d.z * p[3];
        }
    }
    out
}

fn scale_crv(c: &mut InlineCrv, s: f64) {
    for p in &mut c.cpts {
        p[0] *= s;
        p[1] *= s;
        p[2] *= s;
    }
}

/// A line / B-spline-line / free-form centre line with its interval: `(curve, kind)`
/// with kind 0 a degree 1 line, 1 a straight higher degree curve, 2 free-form.
fn gen_center(rng: &mut Rng) -> (InlineCrv, u32) {
    let len = rng.uniform(2.0, 8.0);
    let (a, b) = match rng.below(4) {
        0 => (0.0, len),
        1 => (rng.uniform(-3.0, 0.0), rng.uniform(1.0, 4.0)),
        _ => (0.0, 1.0),
    };
    let kind = match rng.below(10) {
        0..=2 => 0,
        3 => 1,
        _ => 2,
    };
    let (p, n) = match kind {
        0 => (1, 1 + rng.below(3) as usize),
        1 => (2 + rng.below(2) as usize, 3 + rng.below(3) as usize),
        _ => {
            let p = 2 + rng.below(2) as usize;
            (p, p + rng.below(4) as usize)
        }
    };
    let u = uniform_knots(n, p, a, b);
    let cpts = (0..=n)
        .map(|i| {
            // Greville abscissae: a linear parametrisation of the straight ones
            let g: f64 = u.iter().skip(i + 1).take(p).sum::<f64>() / p as f64;
            let x = len * (g - a) / (b - a);
            if kind == 2 {
                [x, rng.uniform(-0.8, 0.8), rng.uniform(-0.8, 0.8), NOW]
            } else {
                [x, 0.0, 0.0, NOW]
            }
        })
        .collect();
    (InlineCrv { cpts, p, u }, kind)
}

/// Centre line and rails of a fillet: constant offsets `r1 e_a`, `r2 (cos t e_a + sin t
/// e_b)` (plus variations), or coaxial arcs.
#[allow(clippy::type_complexity)]
fn gen_rails(rng: &mut Rng, blend: bool) -> (InlineCrv, InlineCrv, InlineCrv) {
    let r1 = rng.uniform(0.3, 2.0);
    let theta = rng.uniform(0.3, 2.6);
    let r2 = r1
        * match rng.below(12) {
            0..=5 => 1.0,
            6 => 1.0 + 1e-9,
            7 => 1.0 + 2e-7,
            8 | 9 => 1.0 + rng.uniform(0.01, 0.08),
            10 if !blend => 1.0 + rng.uniform(0.15, 0.4),
            _ => 1.0 + rng.uniform(1e-6, 1e-4),
        };
    if rng.chance(0.15) {
        // coaxial arcs (a revolve candidate)
        let c = Vec3::new(rng.uniform(-2.0, 2.0), rng.uniform(-2.0, 2.0), 0.0);
        let rho = rng.uniform(4.0, 8.0);
        let span = rng.uniform(30.0, 300.0);
        let x = unit_vec(rng);
        let y = perp(x, rng);
        let z = x.cross(y);
        // section plane: radial e_a, axial e_b
        let off = |r: f64, ang: f64| (r * ang.cos(), r * ang.sin());
        let (a1, h1) = off(r1, rng.uniform(-0.5, 0.5));
        let (a2, h2) = off(r2, rng.uniform(-0.5, 0.5) + theta);
        let arc = |rad: f64, h: f64| {
            BSplineCurve::create_circle_segment(
                3,
                &Axis2Placement::new(c + z * h, x, y),
                rad,
                0.0,
                span,
                NurbCircleParam::Quadratic,
            )
            .ok()
            .and_then(|a| crv_of(&a))
        };
        if let (Some(center), Some(rail1), Some(rail2)) =
            (arc(rho, 0.0), arc(rho + a1, h1), arc(rho + a2, h2))
        {
            return (center, rail1, rail2);
        }
    }
    let (center, kind) = gen_center(rng);
    // the frame of the sections: a direction perpendicular to the (mean) line
    let d = Vec3::new(1.0, 0.0, 0.0);
    let e_a = perp(d, rng);
    let e_b = d.cross(e_a);
    let off1 = e_a * r1;
    let off2 = (e_a * theta.cos() + e_b * theta.sin()) * r2;
    let rail1 = translate(&center, off1);
    let mut rail2 = translate(&center, off2);
    match rng.below(12) {
        // varying gap (some sections skipped or blended)
        0 if kind == 2 => {
            for q in &mut rail2.cpts {
                q[1] += rng.uniform(-0.05, 0.05) * r1;
                q[2] += rng.uniform(-0.05, 0.05) * r1;
            }
        }
        // rails meet at the start
        1 if !blend => {
            if let (Some(q2), Some(q1)) = (rail2.cpts.first_mut(), rail1.cpts.first()) {
                *q2 = *q1;
            }
        }
        // rail 2 with one more knot (synchronised for circular sections; an error for
        // blend curves)
        2 => {
            if let Some(c) = bcurve_of(&rail2).ok().and_then(|mut c| {
                let ivl = c.natural_interval();
                c.insert_one_knot(ivl.evaluate(rng.uniform(0.2, 0.8)), 1)
                    .ok()
                    .map(|()| c)
            }) {
                if let Some(c) = crv_of(&c) {
                    rail2 = c;
                }
            }
        }
        // rail 2 not parallel (lines)
        3 => {
            if let Some(last) = rail2.cpts.last_mut() {
                last[1] += 0.2 * r1;
            }
        }
        _ => {}
    }
    (center, rail1, rail2)
}

fn gen_scale(rng: &mut Rng) -> f64 {
    *rng.choose(&[1.0, 1.0, 1.0, 1.0, 1e-3, 1e3, 1e-6, 1e6])
        .unwrap_or(&1.0)
}

fn gen_tol(rng: &mut Rng) -> f64 {
    match rng.below(12) {
        0 => 0.0,
        1 => 1e-3,
        2 => 0.05,
        3 => 1e-9,
        _ => 1e-6,
    }
}

fn gen_circ(rng: &mut Rng) -> Inputs {
    let (mut center, mut rail1, mut rail2) = gen_rails(rng, false);
    let s = gen_scale(rng);
    for c in [&mut center, &mut rail1, &mut rail2] {
        scale_crv(c, s);
    }
    Inputs::Circ {
        approx: rng.chance(0.7),
        tolpct: *rng
            .choose(&[0.05, 0.05, 0.01, 0.2, 1e-7, 2.0])
            .unwrap_or(&0.05),
        mirror: rng.chance(0.2),
        complement: rng.chance(0.15),
        tol: gen_tol(rng) * s,
        g1: rng.chance(0.4),
        center,
        rail1,
        rail2,
    }
}

/// A bumpy B-spline surface over `[0, 1]^2` in the plane `z = z0` (scaled by 4).
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
        x: [x.x, x.y, x.z],
        y: [y.x, y.y, y.z],
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
    let c = crv_of(&circle)?;
    let top = translate(&c, Vec3::new(0.0, 0.0, h));
    let net = c
        .cpts
        .iter()
        .zip(&top.cpts)
        .map(|(a, b)| vec![*a, *b])
        .collect();
    Some(SrfIn::Bspline {
        net,
        p: c.p,
        q: 1,
        u: c.u,
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
        // u = umin is a point: singular in u
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
        _ => gen_bspline_surface(rng, 0.0),
    }
}

fn gen_blend(rng: &mut Rng) -> Inputs {
    let (mut center, mut rail1, mut rail2) = gen_rails(rng, true);
    let s = gen_scale(rng);
    for c in [&mut center, &mut rail1, &mut rail2] {
        scale_crv(c, s);
    }
    let cont = match rng.below(10) {
        0..=3 => 1,
        4..=6 => 2,
        7 | 8 => 3,
        _ => 4,
    };
    let sides = if rng.chance(0.6) {
        let ivl = rail1
            .u
            .first()
            .copied()
            .zip(rail1.u.last().copied())
            .unwrap_or((0.0, 1.0));
        let uv = |rng: &mut Rng| {
            let p = 1 + rng.below(3) as usize;
            let n = p + rng.below(2) as usize;
            InlineCrv {
                cpts: (0..=n)
                    .map(|_| [rng.uniform(0.1, 0.9), rng.uniform(0.1, 0.9), 0.0, NOW])
                    .collect(),
                p,
                u: uniform_knots(n, p, ivl.0, ivl.1),
            }
        };
        let s1 = if rng.chance(0.3) {
            gen_plane(rng)
        } else {
            gen_bspline_surface(rng, 0.0)
        };
        let uv1 = uv(rng);
        let s2 = if rng.chance(0.3) {
            gen_plane(rng)
        } else {
            gen_bspline_surface(rng, 1.0)
        };
        let uv2 = uv(rng);
        Some(Box::new(Sides { s1, uv1, s2, uv2 }))
    } else {
        None
    };
    Inputs::Blend {
        cont,
        scale: *rng.choose(&[1.5, 1.5, 1.0, 0.5, 2.5]).unwrap_or(&1.5),
        tol: gen_tol(rng) * s,
        // (a skin with the G1 derivative surfaces is undefined upstream and rejected)
        g1: rng.chance(0.25),
        center,
        rail1,
        rail2,
        sides,
    }
}

fn gen_linear(rng: &mut Rng) -> Inputs {
    let (_, rail1, mut rail2) = gen_rails(rng, false);
    if rng.chance(0.3) {
        let (c, _) = gen_center(rng);
        rail2 = translate(&c, Vec3::new(0.0, 1.0, 1.0));
    }
    Inputs::Linear { rail1, rail2 }
}

fn type_kinds(rng: &mut Rng) -> Vec<i64> {
    let mut k = vec![11001, 16037, 16042, 16043, 16044, 16045, 16046, 16038, 0];
    k.push(rng.range_i64(0, 20000));
    k
}

fn gen_base(rng: &mut Rng) -> Inputs {
    let curve = match rng.below(10) {
        0 | 1 => CurveIn::Line {
            start: rng.point3(-2.0, 2.0),
            end: rng.point3(-2.0, 2.0),
        },
        2 => CurveIn::Hermite {
            p1: rng.point3(-2.0, 2.0),
            d1: rng.point3(-2.0, 2.0),
            p2: rng.point3(-2.0, 2.0),
            d2: rng.point3(-2.0, 2.0),
        },
        _ => {
            let (c, _) = gen_center(rng);
            CurveIn::Bspline {
                oob: rng.chance(0.3),
                c,
            }
        }
    };
    let ivl = match &curve {
        CurveIn::Bspline { c, .. } => (
            c.u.first().copied().unwrap_or(0.0),
            c.u.last().copied().unwrap_or(1.0),
        ),
        _ => (0.0, 1.0),
    };
    let (a, b) = ivl;
    let len = b - a;
    // `SM_EFF_ZERO * (1 + |min| + |max|)`, the snapping distance
    let snap = 1e-12 * (1.0 + a.abs() + b.abs());
    let n_calls = 1 + rng.below(6) as usize;
    let calls = (0..n_calls)
        .map(|_| {
            let t = match rng.below(12) {
                0 | 1 => rng.uniform(a, b),
                2 => a,
                3 => b,
                4 => a - 0.3 * snap,
                5 => b + 0.3 * snap,
                6 => a - len * rng.uniform(0.001, 0.05),
                7 => b + len * rng.uniform(0.001, 0.05),
                8 => a - len * rng.uniform(0.2, 2.0),
                9 => b + len * rng.uniform(0.2, 2.0),
                10 => a - 3.0 * snap,
                _ => b + len * rng.uniform(0.05, 0.3),
            };
            (t, rng.below(4) as usize)
        })
        .collect();
    Inputs::Base {
        curve,
        calls,
        kinds: type_kinds(rng),
    }
}

fn gen_ss_ext(rng: &mut Rng) -> Inputs {
    let surface = gen_surface(rng);
    let natural = match &surface {
        SrfIn::Plane { domain, .. } => *domain,
        SrfIn::Bspline { .. } => [0.0, 0.0, 1.0, 1.0],
    };
    let domain = match rng.below(4) {
        0 | 1 => natural,
        2 => {
            let lerp = |a: f64, b: f64, t: f64| a + (b - a) * t;
            [
                lerp(natural[0], natural[2], rng.uniform(0.0, 0.4)),
                lerp(natural[1], natural[3], rng.uniform(0.0, 0.4)),
                lerp(natural[0], natural[2], rng.uniform(0.6, 1.0)),
                lerp(natural[1], natural[3], rng.uniform(0.6, 1.0)),
            ]
        }
        _ => [
            natural[0] - 0.2,
            natural[1] + 0.1,
            natural[2] + 0.3,
            natural[3] - 0.1,
        ],
    };
    Inputs::SsExt {
        ext: *rng.choose(&[0.5, 0.5, 0.0, 1.0, 0.25]).unwrap_or(&0.5),
        surface,
        domain,
    }
}

fn gen_cr(rng: &mut Rng) -> CrIn {
    let a = gen_surface(rng);
    let b = if rng.chance(0.2) {
        None
    } else {
        Some(gen_surface(rng))
    };
    let r0 = match rng.below(8) {
        0 => 0.0,
        1 => -rng.uniform(0.1, 2.0),
        _ => rng.uniform(0.1, 2.0),
    };
    let r1 = if rng.chance(0.4) {
        r0
    } else {
        rng.uniform(0.1, 2.0)
    };
    CrIn {
        a,
        b,
        r: [r0, r1],
        o: [rng.chance(0.5), rng.chance(0.5)],
    }
}

fn gen_cd(rng: &mut Rng) -> CdIn {
    let a = gen_bspline_surface(rng, 0.0);
    let b = if rng.chance(0.2) {
        None
    } else {
        Some(gen_surface(rng))
    };
    let pm = |rng: &mut Rng| if rng.chance(0.5) { 1.0 } else { -1.0 };
    CdIn {
        a,
        b,
        d: [rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)],
        o: [pm(rng), pm(rng)],
        dist: *rng.choose(&[1.0, 0.5, 2.0, 0.0, 1e-6, 1e3]).unwrap_or(&1.0),
        ext: *rng.choose(&[0.5, 0.5, 1.0, 0.0]).unwrap_or(&0.5),
    }
}

fn gen_jac(rng: &mut Rng, distance: bool) -> JacIn {
    let ne = rng.below(2) as usize;
    let np = rng.below(3) as usize;
    let nx = 12;
    let nf = ne + 4 + rng.below(2) as usize;
    let _ = distance;
    JacIn {
        rail: rng.below(2) as usize,
        ne,
        np,
        sel1: rng.below(5) as usize,
        off1: rng.below(6) as usize,
        sel2: rng.below(5) as usize,
        off2: rng.below(6) as usize,
        x: (0..nx).map(|_| rng.uniform(-0.1, 1.1)).collect(),
        f: (0..nf).map(|_| rng.uniform(-1.0, 1.0)).collect(),
        jac: if rng.chance(0.7) {
            Some([ne + 4, nx])
        } else {
            None
        },
    }
}

fn gen_cd_init(rng: &mut Rng) -> Inputs {
    let solver = gen_cd(rng);
    let do2 = rng.chance(0.25);
    let rail = rng.below(2) as usize;
    let sel1 = 1 + rng.below(4) as usize;
    let ng = 2 + rng.below(2) as usize;
    let dim = ng + 3 + rng.below(3) as usize;
    let intervals = (0..dim)
        .map(|_| [rng.uniform(-1.0, 0.0), rng.uniform(1.0, 2.0)])
        .collect();
    let np = rng.below(4) as usize;
    let per = (0..np).map(|_| rng.chance(0.5)).collect();
    let guess = (0..ng).map(|_| rng.uniform(0.05, 0.95)).collect();
    Inputs::CdInit {
        solver,
        do2,
        rail,
        sel1,
        s1_index: rng.below((ng - 1) as u64) as usize,
        s2_index: rng.below((dim - 2) as u64) as usize,
        intervals,
        per,
        guess,
    }
}

fn gen_vr(rng: &mut Rng) -> VrIn {
    let center = match gen_base(rng) {
        Inputs::Base { curve, .. } => curve,
        _ => CurveIn::Line {
            start: [0.0; 3],
            end: [1.0, 0.0, 0.0],
        },
    };
    let law = if rng.chance(0.7) {
        let r0 = rng.uniform(0.2, 2.0);
        let r1 = match rng.below(4) {
            0 => r0,
            // around the SM_EFF_ZERO (1e-12) test of OffsetRadiiCanChange
            1 => r0 + 5e-13,
            2 => r0 + 2e-12,
            _ => rng.uniform(0.2, 2.0),
        };
        LawIn::Linear { r0, r1 }
    } else {
        let p = 1 + rng.below(2) as usize;
        let n = p + rng.below(2) as usize;
        LawIn::Bspline {
            c: InlineCrv {
                cpts: (0..=n)
                    .map(|_| [rng.uniform(0.2, 2.0), 0.0, 0.0, NOW])
                    .collect(),
                p,
                u: uniform_knots(n, p, 0.0, 1.0),
            },
            map: [0.0, 1.0],
        }
    };
    let pm = |rng: &mut Rng| if rng.chance(0.5) { 1.0 } else { -1.0 };
    VrIn {
        a: gen_bspline_surface(rng, 0.0),
        b: gen_surface(rng),
        o: [pm(rng), pm(rng)],
        center,
        law,
        orient: rng.chance(0.5),
        fsg: 1 + rng.below(3) as usize,
    }
}

fn gen_cr_eu(rng: &mut Rng) -> Inputs {
    // Mostly the upstream solid B-reps (their edges have radial face uses). Only
    // box_with_large_fillet.smb has edges between exactly imported faces (planes and
    // B-splines) with UV trim curves: the other solids defer on their surface kinds or
    // on making a UV trim curve (SmSurface::DropCurve); the laminas compare the
    // tangent-surface exit.
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
    let r = *rng
        .choose(&[0.1, 0.5, 1.0, 2.5, -0.3, 0.0, 1e-7, 50.0])
        .unwrap_or(&0.5);
    Inputs::CrEu {
        brep,
        eu: rng.below(1 << 20) as i64,
        tol: *rng.choose(&[1e-6, 1e-3, 0.0, 1e-9]).unwrap_or(&1e-6),
        ang: 30.0 * DEG,
        tan: *rng.choose(&[2.0 * DEG, 0.0, 0.1]).unwrap_or(&0.0),
        r,
        r2: rng
            .chance(0.4)
            .then(|| *rng.choose(&[0.2, 1.5, -1.0, 0.0, r]).unwrap_or(&0.2)),
        fu: *rng.choose(&[0.0, 0.25, 0.5, 1.0, 0.9]).unwrap_or(&0.5),
        fv: *rng.choose(&[0.0, 0.4, 0.5, 1.0, 0.1]).unwrap_or(&0.5),
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    match rng.below(46) {
        40..=45 => gen_cr_eu(rng),
        0..=10 => gen_circ(rng),
        11..=17 => gen_blend(rng),
        18 | 19 => gen_linear(rng),
        20..=23 => gen_base(rng),
        24..=26 => gen_ss_ext(rng),
        27..=29 => {
            let solver = gen_cr(rng);
            Inputs::Cr {
                solver,
                set: rng
                    .chance(0.5)
                    .then(|| [rng.uniform(-2.0, 2.0), rng.uniform(-2.0, 2.0)]),
                setup: rng.chance(0.7).then(|| rng.uniform(-1.0, 3.0)),
                kinds: type_kinds(rng),
            }
        }
        30 => Inputs::CrJac {
            solver: gen_cr(rng),
            jac: gen_jac(rng, false),
        },
        31 | 32 => Inputs::CdSetup {
            solver: gen_cd(rng),
            user: *rng
                .choose(&[1.0, 0.0, -0.5, 1e-9, 1e6, 0.7])
                .unwrap_or(&1.0),
            kinds: type_kinds(rng),
        },
        33 => Inputs::CdJac {
            solver: gen_cd(rng),
            jac: gen_jac(rng, true),
        },
        34 | 35 => gen_cd_init(rng),
        36..=38 => {
            let solver = gen_vr(rng);
            Inputs::Vr {
                solver,
                set: rng
                    .chance(0.3)
                    .then(|| [rng.uniform(-1.0, 0.5), rng.uniform(0.6, 3.0)]),
            }
        }
        _ => Inputs::VrSetup {
            solver: gen_vr(rng),
            user: rng.uniform(-0.5, 1.5),
        },
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "blend.fillet_standard_solver_p1"
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
        for seed in 0..400u64 {
            let mut rng = Rng::new(0x5eed_0000 + seed);
            let i = gen_case(&mut rng);
            let op = i.label();
            let out = rust(&i).expect("harness");
            let kind = if let Some(d) = &out.deferred {
                format!("deferred:{}", d.symbol)
            } else if let Some(e) = &out.error {
                format!("error:{e}")
            } else {
                let st = out
                    .values
                    .get("status")
                    .and_then(|v| v.first())
                    .copied()
                    .or_else(|| {
                        out.ints
                            .get("status")
                            .and_then(|v| v.first())
                            .map(|&s| s as f64)
                    });
                format!("status:{}", st.unwrap_or(f64::NAN))
            };
            *tally.entry((op, kind)).or_default() += 1;
        }
        for ((op, kind), n) in &tally {
            println!("{op:10} {kind:60} {n}");
        }
        let ok = |op: &str| {
            tally
                .get(&(op.to_string(), "status:0".to_string()))
                .copied()
                .unwrap_or(0)
        };
        assert!(ok("Circ") > 40, "too few successful Circ cases");
        assert!(ok("Blend") > 20, "too few successful Blend cases");
        assert!(ok("CrEu") > 10, "too few successful CrEu cases");
    }

    /// A `cr_jac` case on two planes (`domain` of the first plane given).
    fn cr_jac_planes(domain: [f64; 4], jac: Option<[usize; 2]>) -> Inputs {
        let plane = |y: [f64; 3], domain: [f64; 4]| SrfIn::Plane {
            o: [0.0, 0.0, 0.0],
            x: [1.0, 0.0, 0.0],
            y,
            scale: [1.0, 1.0],
            domain,
        };
        Inputs::CrJac {
            solver: CrIn {
                a: plane([0.0, 1.0, 0.0], domain),
                b: Some(plane([0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0])),
                r: [0.5, 0.5],
                o: [false, false],
            },
            jac: JacIn {
                rail: 0,
                ne: 0,
                np: 1,
                sel1: 1,
                off1: 0,
                sel2: 2,
                off2: 2,
                x: vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 0.25, 0.75, 0.5],
                f: vec![0.1, -0.2, 0.3, -0.4],
                jac,
            },
        }
    }

    /// Sizes above the FFI bounds are refused before anything is allocated (review
    /// fuzz: a `2^31 x 12` Jacobian or `2^31` derivatives aborted the Rust side).
    #[test]
    fn oversized_jacobian_and_derivative_counts_are_refused() {
        let i = cr_jac_planes([0.0, 0.0, 1.0, 1.0], Some([1 << 31, 12]));
        assert!(rust(&i).is_err());
        let ok = cr_jac_planes([0.0, 0.0, 1.0, 1.0], Some([4, 12]));
        assert!(rust(&ok).is_ok());
        let base = Inputs::Base {
            curve: CurveIn::Line {
                start: [0.0, 0.0, 0.0],
                end: [1.0, 0.0, 0.0],
            },
            calls: vec![(0.5, 1 << 31)],
            kinds: vec![],
        };
        assert!(rust(&base).is_err());
    }

    /// A solver's plane with a reversed domain is refused (decreasing NURB knots: the
    /// C++ segfaults extending it when reversed by `1e10`); an ordered one is accepted.
    #[cfg(feature = "cpp")]
    #[test]
    fn reversed_plane_domain_of_a_solver_is_refused() {
        let bad = cr_jac_planes([0.0, 0.0, 1.0, -1e10], Some([4, 12]));
        let e = cpp(&bad).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains("reversed domain"), "{e}");
        assert!(cpp(&cr_jac_planes([0.0, 0.0, 1.0, 1e10], Some([4, 12]))).is_ok());
    }
}
