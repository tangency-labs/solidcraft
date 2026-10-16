//! Runner for unit `surface.surf_of_extrusion_p1`: `SmSurfOfExtrusion` part 1
//! (construction through `CreateCanonical` or either constructor with `MakeNurb`,
//! `operator==`, the copy, `AdjustSTEPUVDomain`, `UpdateAnalyticalDomain`,
//! `Reparameterize`, `Transform`, the STEP / NURB conversions, STEP evaluation,
//! `STEPInversion`, `CreateDirectrixFromPoint`, `DropPointFast`, the small queries)
//! and the file-static `sm_IntersectGenerators`.
//!
//! C++: `solidcore_ffi::units::surface_surf_of_extrusion_p1` (upstream
//! `source/SMLib/src/SmSurfOfExtrusion.cpp`). Rust:
//! `solidcore::surface::surf_of_extrusion::p1`.
//!
//! A case is `surf` (generator, construction, `oob`, `swap`) plus `op` and its
//! parameters. Both sides produce the same flat record (layout in the FFI module
//! docs), split into `build` (the construction status), `status` (the op status) and
//! `out` (the rest). Statuses are `0` (`SM_SUCCESS`) or the `SmStatus`; `-1` on the
//! Rust side for an error upstream cannot produce.
//!
//! Deferred cases: `STEPInversion` of a generator not perpendicular to the extrusion
//! (`SmBSplineSurface::STEPInversion`'s solves, `surface.surface_p1`) and
//! `sm_IntersectGenerators` of parallel extrusions (`SmCurve::GlobalCurveSolve`,
//! `curve.curve_p9`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::hyperbola::Hyperbola;
use solidcore::curve::parabola::Parabola;
use solidcore::curve::line::Line;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::global_solver::{SolutionRequested, SolverOperation};
use solidcore::geom::solution_array::{ObjectKind, SolutionObject};
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::surface::surf_of_extrusion::p1::intersect_generators;
use solidcore::surface::surf_of_extrusion::SurfOfExtrusion;
use solidcore::surface::surface::p2::{B_SPLINE_SURFACE_TYPE, SURF_OF_EXTRUSION_TYPE, TYPE};
use solidcore::SolidError;
use solidcore_ffi::units::surface_surf_of_extrusion_p1 as ffi;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 800;

/// `surface.surf_of_extrusion_p1` runner.
pub struct Runner;

pub(crate) fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

/// An inline 3D curve: Euclidean control points, optional weights, degree, knots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct InlineCrv {
    pub(crate) cpts: Vec<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) weights: Option<Vec<f64>>,
    pub(crate) p: usize,
    pub(crate) u: Vec<f64>,
}

/// A generator.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum GenIn {
    BSpline(InlineCrv),
    Line {
        a: [f64; 3],
        b: [f64; 3],
    },
    Circle {
        c: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        d: [f64; 2],
        r: f64,
    },
    Ellipse {
        c: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        d: [f64; 2],
        rx: f64,
        ry: f64,
    },
    Hyperbola {
        c: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        d: [f64; 2],
        a: f64,
        b: f64,
    },
    Parabola {
        c: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        d: [f64; 2],
        f: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub(crate) enum BuildIn {
    Canonical {
        vec: [f64; 3],
    },
    Ctor {
        origin: [f64; 3],
        vec: [f64; 3],
        domain: [f64; 4],
    },
    Infinite {
        vec: [f64; 3],
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SurfIn {
    pub(crate) gen: GenIn,
    pub(crate) build: BuildIn,
    #[serde(default)]
    pub(crate) oob: bool,
    #[serde(default)]
    pub(crate) swap: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Build {},
    EvalStep {
        u: f64,
        v: f64,
        k: usize,
        #[serde(default)]
        point: bool,
    },
    ToNurbs {
        u: f64,
        v: f64,
    },
    ToStep {
        u: f64,
        v: f64,
    },
    Adjust {
        dom: [f64; 4],
    },
    Update {
        dom: [f64; 4],
    },
    Reparam {
        dom: [f64; 4],
    },
    Transform {
        origin: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        #[serde(default)]
        scale: Option<[f64; 3]>,
    },
    Directrix {
        p: [f64; 3],
        tol: f64,
    },
    DropFast {
        dom: [f64; 4],
        sop: u8,
        p: [f64; 3],
        #[serde(default)]
        vec: Option<[f64; 3]>,
        tol: f64,
        #[serde(default)]
        target: Option<f64>,
        requested: u8,
    },
    StepInv {
        dom: [f64; 4],
        p: [f64; 3],
        tol: f64,
        #[serde(default)]
        guess: Option<[f64; 2]>,
    },
    Eq {
        other: Box<SurfIn>,
    },
    Copy {},
    Generators {
        other: GenIn,
        vec: [f64; 3],
        ivl: [f64; 2],
        tol: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    surf: SurfIn,
    #[serde(flatten)]
    op: OpIn,
}

// ---- C++ side -------------------------------------------------------------------------

pub(crate) fn cpts4(c: &InlineCrv) -> Result<Vec<[f64; 4]>, HarnessError> {
    match &c.weights {
        None => Ok(c.cpts.iter().map(|&[x, y, z]| [x, y, z, NOW]).collect()),
        Some(w) => {
            if w.len() != c.cpts.len() {
                return Err(harness("weights: wrong count"));
            }
            Ok(c.cpts
                .iter()
                .zip(w)
                .map(|(&[x, y, z], &w)| [x * w, y * w, z * w, w])
                .collect())
        }
    }
}

pub(crate) fn ffi_gen(g: &GenIn) -> Result<ffi::Gen, HarnessError> {
    Ok(match g {
        GenIn::BSpline(c) => ffi::Gen::BSpline(ffi::BSplineData {
            cpts: cpts4(c)?,
            p: c.p,
            knots: c.u.clone(),
        }),
        GenIn::Line { a, b } => ffi::Gen::Line { a: *a, b: *b },
        GenIn::Circle { c, x, y, d, r } => ffi::Gen::Circle {
            c: *c,
            x: *x,
            y: *y,
            d: *d,
            r: *r,
        },
        GenIn::Ellipse { c, x, y, d, rx, ry } => ffi::Gen::Ellipse {
            c: *c,
            x: *x,
            y: *y,
            d: *d,
            rx: *rx,
            ry: *ry,
        },
        GenIn::Hyperbola { c, x, y, d, a, b } => ffi::Gen::Hyperbola {
            c: *c,
            x: *x,
            y: *y,
            d: *d,
            a: *a,
            b: *b,
        },
        GenIn::Parabola { c, x, y, d, f } => ffi::Gen::Parabola {
            c: *c,
            x: *x,
            y: *y,
            d: *d,
            f: *f,
        },
    })
}

pub(crate) fn ffi_surf(s: &SurfIn) -> Result<ffi::Surf, HarnessError> {
    Ok(ffi::Surf {
        gen: ffi_gen(&s.gen)?,
        build: match &s.build {
            BuildIn::Canonical { vec } => ffi::Build::Canonical { vec: *vec },
            BuildIn::Ctor {
                origin,
                vec,
                domain,
            } => ffi::Build::Ctor {
                origin: *origin,
                vec: *vec,
                domain: *domain,
            },
            BuildIn::Infinite { vec } => ffi::Build::Infinite { vec: *vec },
        },
        oob: s.oob,
        swap: s.swap,
    })
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let op = match &i.op {
        OpIn::Build {} => ffi::Op::Build,
        OpIn::EvalStep { u, v, k, point } => ffi::Op::EvalStep {
            u: *u,
            v: *v,
            k: *k,
            point: *point,
        },
        OpIn::ToNurbs { u, v } => ffi::Op::ToNurbs { u: *u, v: *v },
        OpIn::ToStep { u, v } => ffi::Op::ToStep { u: *u, v: *v },
        OpIn::Adjust { dom } => ffi::Op::Adjust { dom: *dom },
        OpIn::Update { dom } => ffi::Op::Update { dom: *dom },
        OpIn::Reparam { dom } => ffi::Op::Reparam { dom: *dom },
        OpIn::Transform {
            origin,
            x,
            y,
            scale,
        } => ffi::Op::Transform {
            origin: *origin,
            x: *x,
            y: *y,
            scale: *scale,
        },
        OpIn::Directrix { p, tol } => ffi::Op::Directrix { p: *p, tol: *tol },
        OpIn::DropFast {
            dom,
            sop,
            p,
            vec,
            tol,
            target,
            requested,
        } => ffi::Op::DropFast {
            dom: *dom,
            op: *sop,
            p: *p,
            vec: *vec,
            tol: *tol,
            target: *target,
            requested: *requested,
        },
        OpIn::StepInv { dom, p, tol, guess } => ffi::Op::StepInv {
            dom: *dom,
            p: *p,
            tol: *tol,
            guess: *guess,
        },
        OpIn::Eq { other } => ffi::Op::Eq {
            other: Box::new(ffi_surf(other)?),
        },
        OpIn::Copy {} => ffi::Op::Copy,
        OpIn::Generators {
            other,
            vec,
            ivl,
            tol,
        } => ffi::Op::Generators {
            other: ffi_gen(other)?,
            vec: *vec,
            ivl: *ivl,
            tol: *tol,
        },
    };
    let flat = ffi::surface_op(&ffi_surf(&i.surf)?, &op).map_err(|e| harness(e.to_string()))?;
    to_output(i, &flat)
}

/// Splits a record into `build`, `status` and `out`.
fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let (b, rest) = flat
        .split_first()
        .ok_or_else(|| harness("empty record"))?;
    let out = Output::new().values("build", vec![*b]);
    if *b != 0.0 {
        if !rest.is_empty() {
            return Err(harness("record continues after a failed build"));
        }
        return Ok(out);
    }
    if matches!(i.op, OpIn::Build {}) {
        return Ok(out.values("out", rest.to_vec()));
    }
    let (s, rest) = rest
        .split_first()
        .ok_or_else(|| harness("record has no op status"))?;
    Ok(out.values("status", vec![*s]).values("out", rest.to_vec()))
}

// ---- Rust side ------------------------------------------------------------------------

pub(crate) fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

pub(crate) fn v3(p: [f64; 3]) -> Vec3 {
    let [x, y, z] = p;
    Vec3::new(x, y, z)
}

pub(crate) fn push_v(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

pub(crate) fn push_dom(o: &mut Vec<f64>, d: &Extent2d) {
    o.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
}

pub(crate) fn dom4(d: [f64; 4]) -> Extent2d {
    let [a, b, c, e] = d;
    Extent2d::from_bounds(a, b, c, e)
}

/// The `SmStatus` of a Rust error; `-1` for one upstream cannot return.
pub(crate) fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

/// A handle naming the surface in `DropPointFast`'s solutions.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SurfHandle;

impl SolutionObject for SurfHandle {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Surface
    }
}

pub(crate) fn rust_gen(k: &Kernel, g: &GenIn) -> Result<Curve, HarnessError> {
    Ok(match g {
        GenIn::BSpline(c) => {
            let pw = cpts4(c)?
                .into_iter()
                .map(|[x, y, z, w]| CPoint::new(x, y, z, w))
                .collect();
            let cur = NurbCurve::new(pw, c.p, c.u.clone())
                .map_err(|e| harness(format!("curve input: {e}")))?;
            Curve::from(BSplineCurve::from_nurb(cur, 3, false))
        }
        GenIn::Line { a, b } => {
            let mut l = Line::from_points(v3(*a), v3(*b), 3);
            l.make_nurb().map_err(|e| harness(format!("line nurb: {e}")))?;
            Curve::from(l)
        }
        GenIn::Circle { c, x, y, d, r } => {
            let [d0, d1] = *d;
            let e = Ellipse::new_circle(
                k,
                v3(*c),
                v3(*x),
                v3(*y),
                Extent1d::from_min_max(d0, d1),
                *r,
                &EllipseOptions::default(),
            )
            .map_err(|e| harness(format!("circle: {e}")))?;
            Curve::from(e)
        }
        GenIn::Ellipse { c, x, y, d, rx, ry } => {
            let [d0, d1] = *d;
            let e = Ellipse::new(
                k,
                v3(*c),
                v3(*x),
                v3(*y),
                Extent1d::from_min_max(d0, d1),
                *rx,
                *ry,
                &EllipseOptions::default(),
            )
            .map_err(|e| harness(format!("ellipse: {e}")))?;
            Curve::from(e)
        }
        GenIn::Hyperbola { c, x, y, d, a, b } => {
            let [d0, d1] = *d;
            Curve::from(Hyperbola::new(
                v3(*c),
                v3(*x),
                v3(*y),
                Extent1d::from_min_max(d0, d1),
                *a,
                *b,
                3,
            ))
        }
        GenIn::Parabola { c, x, y, d, f } => {
            let [d0, d1] = *d;
            Curve::from(Parabola::new(
                v3(*c),
                v3(*x),
                v3(*y),
                Extent1d::from_min_max(d0, d1),
                *f,
                3,
            ))
        }
    })
}

/// Builds the surface; `Ok(Err(e))` when the construction fails.
pub(crate) fn rust_surf(
    k: &Kernel,
    s: &SurfIn,
) -> Result<solidcore::Result<SurfOfExtrusion>, HarnessError> {
    let gen = rust_gen(k, &s.gen)?;
    let built = match &s.build {
        BuildIn::Canonical { vec } => SurfOfExtrusion::create_canonical(k, gen, v3(*vec)),
        BuildIn::Ctor {
            origin,
            vec,
            domain,
        } => SurfOfExtrusion::new(k, gen, v3(*origin), v3(*vec), dom4(*domain), s.swap)
            .and_then(|mut r| r.make_nurb(k).map(|()| r)),
        BuildIn::Infinite { vec } => SurfOfExtrusion::new_infinite(k, gen, v3(*vec), s.swap)
            .and_then(|mut r| r.make_nurb(k).map(|()| r)),
    };
    Ok(built.map(|mut r| {
        r.set_out_of_bounds_enabled(s.oob);
        r
    }))
}

fn step_interval(c: &Curve) -> Extent1d {
    match c.kind() {
        CurveKind::Ellipse(e) => e.step_interval(),
        CurveKind::Line(l) => l.step_interval(),
        CurveKind::Hyperbola(h) => h.step_interval(),
        CurveKind::Parabola(p) => p.step_interval(),
        _ => c.natural_interval(),
    }
}

pub(crate) fn nurb_of(c: &Curve) -> Option<&NurbCurve> {
    match c.kind() {
        CurveKind::BSpline(b) => b.gw_nurb(),
        CurveKind::Ellipse(e) => e.base().gw_nurb(),
        CurveKind::Line(l) => l.base().gw_nurb(),
        CurveKind::Hyperbola(h) => h.base().gw_nurb(),
        CurveKind::Parabola(p) => p.base().gw_nurb(),
        _ => None,
    }
}

pub(crate) fn put_curve(o: &mut Vec<f64>, c: Option<&Curve>) {
    o.push(flag(c.is_some()));
    let Some(c) = c else { return };
    o.push(c.type_code() as f64);
    let ivl = step_interval(c);
    o.extend([ivl.min(), ivl.max()]);
    let cur = nurb_of(c);
    o.push(flag(cur.is_some()));
    let Some(cur) = cur else { return };
    o.push(cur.degree() as f64);
    o.push(cur.cpts().len() as f64);
    for p in cur.cpts() {
        o.extend([p.x, p.y, p.z, p.w]);
    }
    o.push(cur.knots().len() as f64);
    o.extend_from_slice(cur.knots());
}

pub(crate) fn put_state(k: &Kernel, o: &mut Vec<f64>, s: &SurfOfExtrusion) -> solidcore::Result<()> {
    o.extend([
        flag(s.is_kind_of(SURF_OF_EXTRUSION_TYPE)?),
        flag(s.is_kind_of(B_SPLINE_SURFACE_TYPE)?),
        flag(s.is_kind_of(TYPE)?),
        s.type_code() as f64,
        flag(s.swap_uv()),
        flag(s.is_out_of_bounds_enabled()),
        f64::from(s.orientation() as u8),
    ]);
    push_v(o, s.origin());
    push_v(o, s.extrusion_vector());
    push_v(o, s.curve_point());
    push_v(o, s.curve_vec());
    push_dom(o, &s.step_uv_domain());
    let (ld, lp) = s.step_linear_param_extent();
    o.extend([ld.min(), ld.max(), f64::from(lp as u8)]);
    let (gd, gp) = s.gen_dir_param_extent();
    o.extend([gd.min(), gd.max(), f64::from(gp as u8)]);
    push_dom(o, &s.max_analytic_domain(k)?);
    let (gc, ev) = s.canonical();
    o.push(flag(gc.is_some()));
    push_v(o, ev);
    put_curve(o, s.gen_curve());
    let sur = s.base().gw_nurb();
    o.push(flag(sur.is_some()));
    let Some(sur) = sur else { return Ok(()) };
    let ix = sur.indices();
    o.extend([ix.p, ix.q, ix.n + 1, ix.m + 1].map(|x| x as f64));
    for row in sur.rows() {
        for p in row {
            o.extend([p.x, p.y, p.z, p.w]);
        }
    }
    o.push(sur.u_knots().len() as f64);
    o.extend_from_slice(sur.u_knots());
    o.push(sur.v_knots().len() as f64);
    o.extend_from_slice(sur.v_knots());
    Ok(())
}

fn solver_op(op: u8) -> SolverOperation {
    match op {
        3 => SolverOperation::Minimize,
        4 => SolverOperation::Maximize,
        5 => SolverOperation::Normalize,
        _ => SolverOperation::Intersect,
    }
}

/// Status then state of an edited surface.
fn edited(k: &Kernel, o: &mut Vec<f64>, s: &SurfOfExtrusion) -> solidcore::Result<()> {
    o.push(0.0);
    put_state(k, o, s)
}

/// Runs the op on the Rust surface, appending its status and outputs.
fn rust_op(
    k: &Kernel,
    s: &mut SurfOfExtrusion,
    op: &OpIn,
    o: &mut Vec<f64>,
) -> Result<solidcore::Result<()>, HarnessError> {
    let r: solidcore::Result<()> = match op {
        OpIn::Build {} => put_state(k, o, s),
        OpIn::EvalStep { u, v, k: kk, point } => {
            let uv = Point2::new(*u, *v);
            if *point {
                s.evaluate_step_point(k, uv).map(|p| {
                    o.push(0.0);
                    push_v(o, p);
                })
            } else {
                s.evaluate_step(k, uv, *kk, *kk).map(|d| {
                    o.push(0.0);
                    for v in d {
                        push_v(o, v);
                    }
                })
            }
        }
        OpIn::ToNurbs { u, v } => s
            .convert_uv_from_step_to_nurbs(k, Point2::new(*u, *v))
            .map(|p| o.extend([0.0, p.x, p.y])),
        OpIn::ToStep { u, v } => s
            .convert_uv_from_nurbs_to_step(k, Point2::new(*u, *v))
            .map(|p| o.extend([0.0, p.x, p.y])),
        OpIn::Adjust { dom } => s
            .adjust_step_uv_domain(k, &dom4(*dom))
            .and_then(|()| edited(k, o, s)),
        OpIn::Update { dom } => s
            .update_analytical_domain(k, &dom4(*dom))
            .and_then(|()| edited(k, o, s)),
        OpIn::Reparam { dom } => s
            .reparameterize(k, &dom4(*dom))
            .and_then(|()| edited(k, o, s)),
        OpIn::Transform {
            origin,
            x,
            y,
            scale,
        } => {
            let rf = Axis2Placement::new(v3(*origin), v3(*x), v3(*y));
            s.transform(k, &rf, scale.map(v3))
                .and_then(|()| edited(k, o, s))
        }
        OpIn::Directrix { p, tol } => s.create_directrix_from_point(k, v3(*p), *tol).map(|c| {
            o.push(0.0);
            put_curve(o, Some(&Curve::from(c)));
        }),
        OpIn::DropFast {
            dom,
            sop,
            p,
            vec,
            tol,
            target,
            requested,
        } => {
            let req = if *requested == 0 {
                SolutionRequested::Single
            } else {
                SolutionRequested::All
            };
            s.drop_point_fast(
                k,
                &dom4(*dom),
                solver_op(*sop),
                v3(*p),
                vec.map(v3),
                *tol,
                *target,
                req,
                Some(SurfHandle),
            )
            .map(|sols| {
                o.extend([0.0, sols.len() as f64]);
                for r in sols.as_slice() {
                    o.extend([
                        f64::from(r.solution_type as u8),
                        r.num_variables as f64,
                        r.num_objects as f64,
                        flag(r.objects[0] == Some(SurfHandle)),
                        r.start.solution_value,
                        r.start.parameters[0],
                        r.start.parameters[1],
                    ]);
                }
            })
        }
        OpIn::StepInv { dom, p, tol, guess } => s
            .step_inversion(
                k,
                &dom4(*dom),
                v3(*p),
                *tol,
                guess.map(|[x, y]| Point2::new(x, y)),
            )
            .map(|(uv, loc)| o.extend([0.0, uv.x, uv.y, f64::from(loc as u8)])),
        OpIn::Eq { other } => match rust_surf(k, other)? {
            Err(e) if e.is_unported() => Err(e),
            Err(e) => {
                o.push(status(&e));
                Ok(())
            }
            Ok(other) => s.approx_eq(k, &other).and_then(|a| {
                let b = other.approx_eq(k, s)?;
                o.extend([0.0, flag(a), flag(b)]);
                Ok(())
            }),
        },
        OpIn::Copy {} => s.copy().and_then(|c| {
            let eq = s.approx_eq(k, &c)?;
            o.extend([0.0, flag(eq)]);
            put_state(k, o, &c)
        }),
        OpIn::Generators {
            other,
            vec,
            ivl,
            tol,
        } => {
            let other = rust_gen(k, other)?;
            let [i0, i1] = *ivl;
            let mut curves = Vec::new();
            let mut types = Vec::new();
            intersect_generators(
                k,
                s,
                *tol,
                &other,
                v3(*vec),
                &Extent1d::from_min_max(i0, i1),
                Some(&mut curves),
                Some(&mut types),
            )
            .map(|r| {
                o.extend([
                    0.0,
                    flag(r.needs_more_intersections),
                    curves.len() as f64,
                ]);
                for c in &curves {
                    put_curve(o, Some(c));
                }
                o.push(types.len() as f64);
                o.extend(types.iter().map(|t| f64::from(*t as u8)));
            })
        }
    };
    Ok(r)
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let mut o = Vec::new();
    match rust_surf(&k, &i.surf)? {
        Err(e) => {
            if let Some(out) = Output::unported(&e) {
                return Ok(out);
            }
            o.push(status(&e));
        }
        Ok(mut surf) => {
            o.push(0.0);
            let at = o.len();
            if let Err(e) = rust_op(&k, &mut surf, &i.op, &mut o)? {
                if let Some(out) = Output::unported(&e) {
                    return Ok(out);
                }
                o.truncate(at);
                o.push(status(&e));
            }
        }
    }
    to_output(i, &o)
}

// ---- generation -----------------------------------------------------------------------

pub(crate) fn unit_vec(rng: &mut Rng) -> [f64; 3] {
    loop {
        let [x, y, z] = rng.point3(-1.0, 1.0);
        let l = (x * x + y * y + z * z).sqrt();
        if l > 0.2 && l <= 1.0 {
            return [x / l, y / l, z / l];
        }
    }
}

pub(crate) fn add(a: [f64; 3], b: [f64; 3], s: f64) -> [f64; 3] {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    [ax + s * bx, ay + s * by, az + s * bz]
}

pub(crate) fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    let [x, y, z] = a;
    [s * x, s * y, s * z]
}

pub(crate) fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx]
}

pub(crate) fn normalize(a: [f64; 3]) -> [f64; 3] {
    let [x, y, z] = a;
    let l = (x * x + y * y + z * z).sqrt();
    [x / l, y / l, z / l]
}

/// A clamped knot vector for `n + 1` control points of degree `p` on `[a, b]`.
pub(crate) fn knots(rng: &mut Rng, n: usize, p: usize, a: f64, b: f64) -> Vec<f64> {
    let inner = n - p;
    let mut ts: Vec<f64> = (0..inner)
        .map(|i| (i as f64 + 1.0) / (inner as f64 + 1.0))
        .collect();
    if rng.chance(0.4) {
        for t in &mut ts {
            *t += rng.uniform(-0.2, 0.2) / (inner as f64 + 1.0);
        }
    }
    let mut u = vec![a; p + 1];
    u.extend(ts.iter().map(|t| a + t * (b - a)));
    u.extend(vec![b; p + 1]);
    u
}

/// A frame: origin, unit X, unit Y, unit Z (the generator plane normal).
pub(crate) type Frame = ([f64; 3], [f64; 3], [f64; 3], [f64; 3]);

pub(crate) fn frame(rng: &mut Rng, size: f64) -> Frame {
    let origin = if rng.chance(0.3) {
        [0.0, 0.0, 0.0]
    } else {
        rng.point3(-5.0 * size, 5.0 * size)
    };
    let (x, y, z) = match rng.below(3) {
        0 => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        1 => ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        _ => {
            let z = unit_vec(rng);
            let t = unit_vec(rng);
            let y = normalize(cross3(z, t));
            let x = cross3(y, z);
            (x, y, z)
        }
    };
    (origin, x, y, z)
}

/// A point of the frame's X/Y plane at `(a, b)`, out of plane by `off`.
pub(crate) fn in_frame(f: &Frame, a: f64, b: f64, off: f64) -> [f64; 3] {
    let (o, x, y, z) = f;
    add(add(add(*o, *x, a), *y, b), *z, off)
}

pub(crate) fn gen_generator(rng: &mut Rng, f: &Frame, planar: bool, size: f64) -> GenIn {
    let off = |rng: &mut Rng| {
        if planar {
            0.0
        } else {
            rng.uniform(-1.0, 1.0) * size
        }
    };
    match rng.below(14) {
        0..=6 => {
            let p = 1 + rng.below(3) as usize;
            let n = p + rng.below(5) as usize;
            let mut cpts = Vec::new();
            let mut a = rng.uniform(-2.0, 0.0);
            for _ in 0..=n {
                a += rng.uniform(0.2, 1.0);
                let b = rng.uniform(-2.0, 2.0);
                let o = off(rng);
                cpts.push(in_frame(f, a * size, b * size, o));
            }
            if p == 1 && n == 1 && rng.chance(0.5) {
                // a straight two point B-spline: linear
            }
            let weights = rng
                .chance(0.3)
                .then(|| (0..=n).map(|_| rng.uniform(0.5, 2.0)).collect());
            let (a, b) = if rng.chance(0.5) {
                (0.0, 1.0)
            } else {
                let a = rng.uniform(-3.0, 3.0);
                (a, a + rng.uniform(0.5, 4.0))
            };
            let u = knots(rng, n, p, a, b);
            GenIn::BSpline(InlineCrv {
                cpts,
                weights,
                p,
                u,
            })
        }
        7 | 8 => {
            let o1 = off(rng);
            GenIn::Line {
                a: in_frame(f, rng.uniform(-2.0, 0.0) * size, rng.uniform(-1.0, 1.0) * size, o1),
                b: in_frame(f, rng.uniform(0.5, 3.0) * size, rng.uniform(-1.0, 1.0) * size, o1),
            }
        }
        9 | 10 => {
            let (o, x, y, _) = f;
            let d0 = rng.uniform(-180.0, 90.0);
            let len = if rng.chance(0.25) { 360.0 } else { rng.uniform(30.0, 270.0) };
            GenIn::Circle {
                c: add(*o, *x, rng.uniform(-1.0, 1.0) * size),
                x: *x,
                y: *y,
                d: [d0, (d0 + len).min(d0 + 360.0).min(360.0)],
                r: rng.uniform(0.3, 2.0) * size,
            }
        }
        12 => {
            let (o, x, y, _) = f;
            let d0 = rng.uniform(-2.0, 1.0);
            GenIn::Hyperbola {
                c: *o,
                x: *x,
                y: *y,
                d: [d0, d0 + rng.uniform(0.3, 2.5)],
                a: rng.uniform(0.3, 2.0) * size,
                b: rng.uniform(0.3, 2.0) * size,
            }
        }
        13 => {
            let (o, x, y, _) = f;
            let d0 = rng.uniform(-3.0, 1.0);
            GenIn::Parabola {
                c: *o,
                x: *x,
                y: *y,
                d: [d0, d0 + rng.uniform(0.5, 4.0)],
                f: rng.uniform(0.2, 2.0) * size,
            }
        }
        _ => {
            let (o, x, y, _) = f;
            let d0 = rng.uniform(-90.0, 90.0);
            let len = rng.uniform(30.0, 270.0);
            GenIn::Ellipse {
                c: *o,
                x: *x,
                y: *y,
                d: [d0, (d0 + len).min(360.0)],
                rx: rng.uniform(0.5, 2.0) * size,
                ry: rng.uniform(0.2, 1.5) * size,
            }
        }
    }
}

fn gen_step_ivl(g: &GenIn) -> [f64; 2] {
    match g {
        GenIn::BSpline(c) => [
            c.u.first().copied().unwrap_or(0.0),
            c.u.last().copied().unwrap_or(1.0),
        ],
        GenIn::Line { .. } => [0.0, 1.0],
        GenIn::Circle { d, .. }
        | GenIn::Ellipse { d, .. }
        | GenIn::Hyperbola { d, .. }
        | GenIn::Parabola { d, .. } => *d,
    }
}

fn gen_vec(rng: &mut Rng, f: &Frame) -> [f64; 3] {
    let (_, x, _, z) = f;
    let len = match rng.below(8) {
        0 => 1e-3,
        1 => 1e3,
        2 => 1.0,
        _ => rng.uniform(0.2, 5.0),
    };
    let dir = match rng.below(8) {
        // perpendicular to the generator plane (either sense)
        0..=2 => *z,
        3 => scale(*z, -1.0),
        // in the plane (coplanar)
        4 => *x,
        // nearly perpendicular
        5 => normalize(add(*z, *x, 1e-12)),
        _ => unit_vec(rng),
    };
    if rng.chance(0.02) {
        [0.0, 0.0, 0.0]
    } else {
        scale(dir, len)
    }
}

pub(crate) fn gen_surf(rng: &mut Rng) -> SurfIn {
    let size = match rng.below(10) {
        0 => 1e-3,
        1 => 1e3,
        _ => 1.0,
    };
    let f = frame(rng, size);
    let planar = rng.chance(0.75);
    let gen = gen_generator(rng, &f, planar, size);
    let vec = gen_vec(rng, &f);
    let build = match rng.below(10) {
        0..=3 => BuildIn::Canonical { vec },
        4..=7 => {
            let [u0, u1] = gen_step_ivl(&gen);
            let (u0, u1) = if rng.chance(0.2) {
                // a sub-interval: MakeNurb trims the generator
                let l = u1 - u0;
                (u0 + 0.1 * l, u1 - 0.2 * l)
            } else {
                (u0, u1)
            };
            let (v0, v1) = match rng.below(4) {
                0 => (0.0, 1.0),
                1 => (-1.0, 2.5),
                _ => {
                    let a = rng.uniform(-10.0, 10.0);
                    (a, a + rng.uniform(0.1, 10.0))
                }
            };
            let (o, ..) = f;
            BuildIn::Ctor {
                origin: o,
                vec,
                domain: [u0, v0, u1, v1],
            }
        }
        _ => BuildIn::Infinite { vec },
    };
    SurfIn {
        gen,
        build,
        oob: rng.chance(0.2),
        swap: rng.chance(0.4),
    }
}

#[allow(clippy::too_many_lines)] // one case generator per op
fn gen_case(rng: &mut Rng) -> Option<Inputs> {
    let k = Kernel::default();
    let s = gen_surf(rng);
    // the Rust surface sizes the parameters (both sides get the same numbers)
    let built = rust_surf(&k, &s).ok()?.ok();
    let nurb = built.as_ref().map(|b| b.base().natural_uv_domain());
    let step = built.as_ref().map(SurfOfExtrusion::step_uv_domain);
    let pick = |rng: &mut Rng, lo: f64, hi: f64| -> f64 {
        match rng.below(10) {
            0 => lo,
            1 => hi,
            2 => lo - rng.uniform(0.0, 0.5) * (hi - lo),
            _ => lo + rng.unit() * (hi - lo),
        }
    };
    let nd = nurb.unwrap_or(Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0));
    let sd = step.unwrap_or(Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0));
    // a finite STEP V range to sample (the infinite domain is huge)
    let (sv0, sv1) = if sd.max().y - sd.min().y > 1e5 {
        (-3.0, 3.0)
    } else {
        (sd.min().y, sd.max().y)
    };
    let (nv0, nv1, nu0, nu1) = (nd.min().y, nd.max().y, nd.min().x, nd.max().x);
    // a point on the surface (or near it)
    let surf_point = |rng: &mut Rng, noise: f64| -> Option<[f64; 3]> {
        let b = built.as_ref()?;
        let uv = Point2::new(
            sd.min().x + rng.unit() * (sd.max().x - sd.min().x),
            sv0 + rng.unit() * (sv1 - sv0),
        );
        let p = b.evaluate_step_point(&k, uv).ok()?;
        let [dx, dy, dz] = rng.point3(-noise, noise);
        Some([p.x + dx, p.y + dy, p.z + dz])
    };
    // a sub-domain of a domain
    let sub = |rng: &mut Rng, lo: [f64; 2], hi: [f64; 2]| -> [f64; 4] {
        let [u0, v0] = lo;
        let [u1, v1] = hi;
        if rng.chance(0.25) {
            return [u0, v0, u1, v1];
        }
        let a = rng.uniform(0.0, 0.4);
        let b = rng.uniform(0.6, 1.0);
        let c = rng.uniform(0.0, 0.4);
        let d = rng.uniform(0.6, 1.0);
        [
            u0 + a * (u1 - u0),
            v0 + c * (v1 - v0),
            u0 + b * (u1 - u0),
            v0 + d * (v1 - v0),
        ]
    };
    let op = match rng.below(17) {
        0 => OpIn::Build {},
        1 | 2 => OpIn::EvalStep {
            u: pick(rng, sd.min().x, sd.max().x),
            v: pick(rng, sv0, sv1),
            k: if rng.chance(0.05) {
                6
            } else if matches!(s.gen, GenIn::BSpline(_)) {
                rng.below(5) as usize
            } else {
                rng.below(6) as usize
            },
            point: rng.chance(0.2),
        },
        3 => OpIn::ToNurbs {
            u: pick(rng, sd.min().x, sd.max().x),
            v: pick(rng, sv0, sv1),
        },
        4 => OpIn::ToStep {
            u: pick(rng, nu0, nu1),
            v: pick(rng, nv0, nv1),
        },
        5 => OpIn::Adjust {
            dom: sub(rng, [sd.min().x, sv0], [sd.max().x, sv1]),
        },
        6 => OpIn::Update {
            dom: sub(rng, [nu0, nv0], [nu1, nv1]),
        },
        7 => {
            let dom = if rng.chance(0.3) {
                let a = rng.uniform(-2.0, 2.0);
                let b = rng.uniform(-2.0, 2.0);
                [a, b, a + rng.uniform(0.5, 3.0), b + rng.uniform(0.5, 3.0)]
            } else {
                sub(rng, [nu0, nv0], [nu1, nv1])
            };
            OpIn::Reparam { dom }
        }
        8 => {
            let (origin, x, y) = match rng.below(4) {
                0 => ([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
                1 => (rng.point3(-3.0, 3.0), [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
                _ => {
                    let z = unit_vec(rng);
                    let t = unit_vec(rng);
                    let y = normalize(cross3(z, t));
                    (rng.point3(-3.0, 3.0), cross3(y, z), y)
                }
            };
            let scale = match rng.below(5) {
                0 => Some([2.0, 2.0, 2.0]),
                1 => Some([1.0, 1.0, 1.0]),
                2 => Some([0.5, 0.5, 2.0]),
                _ => None,
            };
            OpIn::Transform {
                origin,
                x,
                y,
                scale,
            }
        }
        9 | 10 => {
            let noise = if rng.chance(0.3) { 1e-6 } else { 0.0 };
            OpIn::Directrix {
                p: surf_point(rng, noise).unwrap_or_else(|| rng.point3(-3.0, 3.0)),
                tol: *rng.choose(&[1e-9, 1e-6, 1e-3])?,
            }
        }
        11 | 12 => {
            let p = if rng.chance(0.7) {
                surf_point(rng, 0.5)?
            } else {
                rng.point3(-5.0, 5.0)
            };
            let dom = if rng.chance(0.7) {
                [nu0, nv0, nu1, nv1]
            } else {
                sub(rng, [nu0, nv0], [nu1, nv1])
            };
            OpIn::DropFast {
                dom,
                sop: *rng.choose(&[3u8, 3, 0, 4, 5])?,
                p,
                vec: rng.chance(0.3).then(|| unit_vec(rng)),
                tol: *rng.choose(&[1e-9, 1e-6, 1e-3, 0.5])?,
                target: rng.chance(0.15).then(|| rng.uniform(0.0, 3.0)),
                requested: u8::from(rng.chance(0.7)),
            }
        }
        13 | 14 => {
            let noise = if rng.chance(0.3) { 1e-7 } else { 0.0 };
            let p = surf_point(rng, noise).unwrap_or_else(|| rng.point3(-3.0, 3.0));
            OpIn::StepInv {
                dom: [sd.min().x, sd.min().y, sd.max().x, sd.max().y],
                p,
                tol: *rng.choose(&[1e-6, 1e-4, 1e-2])?,
                guess: rng.chance(0.3).then(|| {
                    [
                        sd.min().x + rng.unit() * (sd.max().x - sd.min().x),
                        rng.uniform(sv0, sv1),
                    ]
                }),
            }
        }
        15 => {
            if rng.chance(0.5) {
                OpIn::Copy {}
            } else {
                let mut other = s.clone();
                match rng.below(4) {
                    0 => {}
                    1 => other.swap = !other.swap,
                    2 => {
                        other.build = match other.build {
                            BuildIn::Canonical { vec } => BuildIn::Canonical {
                                vec: add(vec, [1e-3, 0.0, 0.0], 1.0),
                            },
                            BuildIn::Infinite { vec } => BuildIn::Infinite {
                                vec: add(vec, [0.0, 1e-3, 0.0], 1.0),
                            },
                            BuildIn::Ctor {
                                origin,
                                vec,
                                domain,
                            } => BuildIn::Ctor {
                                origin: add(origin, [1e-3, 0.0, 0.0], 1.0),
                                vec,
                                domain,
                            },
                        };
                    }
                    _ => {
                        if let GenIn::BSpline(c) = &mut other.gen {
                            if let Some(p) = c.cpts.last_mut() {
                                *p = add(*p, [0.0, 0.0, 1e-3], 1.0);
                            }
                        }
                    }
                }
                OpIn::Eq {
                    other: Box::new(other),
                }
            }
        }
        _ => {
            // another sweep along a parallel (or not) vector
            let size = 1.0;
            let f = frame(rng, size);
            let other = gen_generator(rng, &f, true, size);
            let ev = built
                .as_ref()
                .map_or([0.0, 0.0, 1.0], |b| {
                    let e = b.extrusion_vector();
                    [e.x, e.y, e.z]
                });
            let vec = match rng.below(4) {
                0 => scale(ev, 2.0),
                1 => scale(ev, -0.5),
                2 => ev,
                _ => unit_vec(rng),
            };
            OpIn::Generators {
                other,
                vec,
                ivl: [rng.uniform(-2.0, 0.0), rng.uniform(0.5, 3.0)],
                tol: *rng.choose(&[1e-6, 1e-3])?,
            }
        }
    };
    Some(Inputs { surf: s, op })
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.surf_of_extrusion_p1"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..8 {
            if let Some(i) = gen_case(rng) {
                return serde_json::to_value(i).ok();
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

    fn op_name(op: &OpIn) -> &'static str {
        match op {
            OpIn::Build {} => "build",
            OpIn::EvalStep { .. } => "eval_step",
            OpIn::ToNurbs { .. } => "to_nurbs",
            OpIn::ToStep { .. } => "to_step",
            OpIn::Adjust { .. } => "adjust",
            OpIn::Update { .. } => "update",
            OpIn::Reparam { .. } => "reparam",
            OpIn::Transform { .. } => "transform",
            OpIn::Directrix { .. } => "directrix",
            OpIn::DropFast { .. } => "drop_fast",
            OpIn::StepInv { .. } => "step_inv",
            OpIn::Eq { .. } => "eq",
            OpIn::Copy {} => "copy",
            OpIn::Generators { .. } => "generators",
        }
    }

    /// The generators reach a successful construction and a successful run of every op
    /// on the Rust side.
    #[test]
    fn generated_cases_cover_every_op() {
        let mut tally: BTreeMap<(String, String), usize> = BTreeMap::new();
        for seed in 0..800u64 {
            let mut rng = Rng::new(0x5eed_0000 + seed);
            let Some(i) = gen_case(&mut rng) else {
                continue;
            };
            let out = rust(&i).expect("harness");
            let kind = if let Some(d) = &out.deferred {
                format!("deferred:{}", d.symbol)
            } else {
                let b = out.values.get("build").and_then(|v| v.first()).copied();
                let st = out.values.get("status").and_then(|v| v.first()).copied();
                format!("build:{b:?} status:{st:?}")
            };
            *tally.entry((op_name(&i.op).to_string(), kind)).or_default() += 1;
        }
        for (k, v) in &tally {
            eprintln!("{k:?} {v}");
        }
        for op in [
            "build",
            "eval_step",
            "to_nurbs",
            "to_step",
            "adjust",
            "update",
            "reparam",
            "transform",
            "directrix",
            "drop_fast",
            "step_inv",
            "eq",
            "copy",
            "generators",
        ] {
            let ok = tally.iter().any(|((o, k), _)| {
                o == op
                    && (k == "build:Some(0.0) status:Some(0.0)"
                        || k == "build:Some(0.0) status:None")
            });
            assert!(ok, "{op} never succeeds");
        }
    }

    /// The generated surfaces reach every generator orientation, and the inversions
    /// and drops reach their seam handling.
    #[test]
    fn generated_cases_cover_every_orientation() {
        let k = Kernel::default();
        let mut orient: BTreeMap<String, usize> = BTreeMap::new();
        let mut seams = 0usize;
        for seed in 0..800u64 {
            let mut rng = Rng::new(0x5eed_0000 + seed);
            let Some(i) = gen_case(&mut rng) else {
                continue;
            };
            if let Ok(Ok(s)) = rust_surf(&k, &i.surf) {
                *orient.entry(format!("{:?}", s.orientation())).or_default() += 1;
            }
            if let OpIn::StepInv { .. } = i.op {
                let out = rust(&i).expect("harness");
                if out.values.get("out").and_then(|v| v.get(2)).copied() == Some(1.0) {
                    seams += 1;
                }
            }
        }
        eprintln!("{orient:?} seams {seams}");
        for o in ["Linear", "Perpendicular", "Independent", "Coplanar", "NotPlanar"] {
            assert!(orient.contains_key(o), "{o} never built");
        }
    }

}
