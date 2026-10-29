//! Runner for unit `surface.sphere`: `SmSphere` (construction through `CreateCanonical`
//! or the constructor and `MakeNurb`, `operator==`, `Copy`, `CreateOffsetSurface`, the
//! STEP/NURB conversions, STEP evaluation, the normal, iso curves,
//! `RebuildSTEPFromNURBParameters`, `Transform`, `IsClosed*`, `STEPInversion`,
//! `CreateTrimmedCircleSegments`, the intersections with planes, spheres and cones,
//! `GlobalSurfaceIntersect`, `IsNurbSurfaceSphere`, and the inherited
//! `SmSurfOfRevolution::TrimWithDomain`).
//!
//! C++: `solidcore_ffi::units::surface_sphere` (upstream `source/SMLib/src/SmSphere.cpp`).
//! Rust: `solidcore::surface::sphere`.
//!
//! A case is `sphere` (placement, radius, build mode and options) plus `op` and its
//! parameters. Both sides produce the same flat record (layout in the FFI module docs),
//! split into `build` (the construction status), `status` (the op status) and `out` (the
//! rest). Statuses are `0` (`SM_SUCCESS`) or the `SmStatus`; `-1` on the Rust side for an
//! error upstream cannot produce.
//!
//! Deferred cases: those reaching `SmSurface::GlobalSurfaceIntersect` on a sphere
//! (`surface.sphere`: the sphere is not yet a `SurfaceKind`) and callees in units that are
//! not ported.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface::{IsoCurveOptions, SurfParamType};
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::surface::cone::Cone;
use solidcore::surface::plane::Plane;
use solidcore::surface::sphere::{Sphere, SphereOptions};
use solidcore::surface::surface::p2::{
    Surface, SurfaceIntersection, B_SPLINE_SURFACE_TYPE, SPHERE_TYPE, SURF_OF_REVOLUTION_TYPE,
    TYPE,
};
use solidcore::SolidError;
use solidcore_ffi::units::surface_sphere as ffi;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 800;

/// `surface.sphere` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CircleIn {
    c: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    d: [f64; 2],
    r: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SphereIn {
    /// `0` = `CreateCanonical`, `1` = constructor and `MakeNurb`.
    #[serde(default)]
    mode: u8,
    origin: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    radius: f64,
    #[serde(default)]
    swap: bool,
    #[serde(default)]
    inside_out: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dom: Option<[f64; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    circle: Option<CircleIn>,
}

/// A NURB domain argument.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum DomIn {
    Natural,
    Explicit { d: [f64; 4] },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PlaneIn {
    Bounded {
        origin: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        scale: [f64; 2],
        dom: [f64; 4],
    },
    Infinite {
        origin: [f64; 3],
        normal: [f64; 3],
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConeIn {
    origin: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    r0: f64,
    r1: f64,
    h: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Args {
    dom: DomIn,
    other_dom: DomIn,
    #[serde(default)]
    tol: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PerturbIn {
    i: usize,
    j: usize,
    d: [f64; 3],
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
    ToStep {
        u: f64,
        v: f64,
    },
    ToNurbs {
        u: f64,
        v: f64,
    },
    Normal {
        u: f64,
        v: f64,
    },
    Iso {
        param: u8,
        iso: f64,
        #[serde(default)]
        dom: Option<[f64; 4]>,
    },
    Offset {
        dist: f64,
        tol: f64,
        #[serde(default)]
        rebuild: bool,
    },
    Rebuild {},
    Transform {
        origin: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        #[serde(default)]
        scale: Option<[f64; 3]>,
    },
    Eq {
        other: SphereIn,
    },
    Copy {},
    Closed {
        param: u8,
        dom: DomIn,
    },
    StepInv {
        #[serde(default)]
        dom: Option<[f64; 4]>,
        point: [f64; 3],
        tol: f64,
        #[serde(default)]
        guess: Option<[f64; 2]>,
    },
    Trim {
        c: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        d: [f64; 2],
        tol: f64,
    },
    IsectPlane {
        plane: PlaneIn,
        args: Args,
    },
    IsectSphere {
        other: SphereIn,
        args: Args,
    },
    IsectCone {
        cone: ConeIn,
        args: Args,
    },
    Global {
        plane: PlaneIn,
        args: Args,
    },
    IsNurb {
        scale: f64,
        net_scale: [f64; 3],
        #[serde(default)]
        perturb: Vec<PerturbIn>,
    },
    TrimDomain {
        dom: [f64; 4],
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    sphere: SphereIn,
    #[serde(flatten)]
    op: OpIn,
}

// ---- C++ side -------------------------------------------------------------------------

fn ffi_sphere(s: &SphereIn) -> ffi::SphereIn {
    ffi::SphereIn {
        mode: if s.mode == 0 {
            ffi::Mode::Canonical
        } else {
            ffi::Mode::Ctor
        },
        origin: s.origin,
        x: s.x,
        y: s.y,
        radius: s.radius,
        swap: s.swap,
        inside_out: s.inside_out,
        dom: s.dom,
        circle: s.circle.as_ref().map(|c| ffi::CircleIn {
            c: c.c,
            x: c.x,
            y: c.y,
            d: c.d,
            r: c.r,
        }),
    }
}

fn ffi_dom(d: DomIn) -> ffi::Dom {
    match d {
        DomIn::Natural => ffi::Dom::Natural,
        DomIn::Explicit { d } => ffi::Dom::Explicit(d),
    }
}

fn ffi_args(a: &Args) -> ffi::IsectArgs {
    ffi::IsectArgs {
        dom: ffi_dom(a.dom),
        other_dom: ffi_dom(a.other_dom),
        tol: a.tol,
    }
}

fn ffi_plane(p: &PlaneIn) -> ffi::Plane {
    match p {
        PlaneIn::Bounded {
            origin,
            x,
            y,
            scale,
            dom,
        } => ffi::Plane::Bounded {
            origin: *origin,
            x: *x,
            y: *y,
            scale: *scale,
            dom: *dom,
        },
        PlaneIn::Infinite { origin, normal } => ffi::Plane::Infinite {
            origin: *origin,
            normal: *normal,
        },
    }
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
        OpIn::ToStep { u, v } => ffi::Op::ToStep { u: *u, v: *v },
        OpIn::ToNurbs { u, v } => ffi::Op::ToNurbs { u: *u, v: *v },
        OpIn::Normal { u, v } => ffi::Op::Normal { u: *u, v: *v },
        OpIn::Iso { param, iso, dom } => ffi::Op::Iso {
            param: *param,
            iso: *iso,
            dom: *dom,
        },
        OpIn::Offset { dist, tol, rebuild } => ffi::Op::Offset {
            dist: *dist,
            tol: *tol,
            rebuild: *rebuild,
        },
        OpIn::Rebuild {} => ffi::Op::Rebuild,
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
        OpIn::Eq { other } => ffi::Op::Eq {
            other: Box::new(ffi_sphere(other)),
        },
        OpIn::Copy {} => ffi::Op::Copy,
        OpIn::Closed { param, dom } => ffi::Op::Closed {
            param: *param,
            dom: ffi_dom(*dom),
        },
        OpIn::StepInv {
            dom,
            point,
            tol,
            guess,
        } => ffi::Op::StepInv {
            dom: *dom,
            point: *point,
            tol: *tol,
            guess: *guess,
        },
        OpIn::Trim { c, x, y, d, tol } => ffi::Op::Trim {
            c: *c,
            x: *x,
            y: *y,
            d: *d,
            tol: *tol,
        },
        OpIn::IsectPlane { plane, args } => ffi::Op::IsectPlane {
            plane: ffi_plane(plane),
            args: ffi_args(args),
        },
        OpIn::IsectSphere { other, args } => ffi::Op::IsectSphere {
            other: Box::new(ffi_sphere(other)),
            args: ffi_args(args),
        },
        OpIn::IsectCone { cone, args } => ffi::Op::IsectCone {
            cone: ffi::ConeIn {
                origin: cone.origin,
                x: cone.x,
                y: cone.y,
                r0: cone.r0,
                r1: cone.r1,
                h: cone.h,
            },
            args: ffi_args(args),
        },
        OpIn::Global { plane, args } => ffi::Op::Global {
            plane: ffi_plane(plane),
            args: ffi_args(args),
        },
        OpIn::IsNurb {
            scale,
            net_scale,
            perturb,
        } => ffi::Op::IsNurb {
            scale: *scale,
            net_scale: *net_scale,
            perturb: perturb
                .iter()
                .map(|p| ffi::Perturb {
                    i: p.i,
                    j: p.j,
                    d: p.d,
                })
                .collect(),
        },
        OpIn::TrimDomain { dom } => ffi::Op::TrimDomain { dom: *dom },
    };
    let flat = ffi::sphere_op(&ffi_sphere(&i.sphere), &op).map_err(|e| harness(e.to_string()))?;
    to_output(i, &flat)
}

/// Splits a record into `build`, `status` and `out`.
fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let (b, rest) = flat
        .split_first()
        .ok_or_else(|| harness("empty record"))?;
    let mut out = Output::new().values("build", vec![*b]);
    if *b != 0.0 {
        if !rest.is_empty() {
            return Err(harness("record continues after a failed build"));
        }
        return Ok(out);
    }
    let mut rest = rest;
    // (the other operand's construction status of an intersection is the op's first
    // value; `Closed` and `IsNurb` have no status)
    if !matches!(
        i.op,
        OpIn::Build {} | OpIn::Closed { .. } | OpIn::IsNurb { .. }
    ) {
        let (s, r) = rest
            .split_first()
            .ok_or_else(|| harness("record has no op status"))?;
        out = out.values("status", vec![*s]);
        rest = r;
    }
    Ok(out.values("out", rest.to_vec()))
}

// ---- Rust side ------------------------------------------------------------------------

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

fn v3(p: [f64; 3]) -> Vec3 {
    let [x, y, z] = p;
    Vec3::new(x, y, z)
}

fn push_v(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

/// The `SmStatus` of a Rust error; `-1` for one upstream cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::Upstream { code, .. } => f64::from(*code),
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007.0,
        _ => -1.0,
    }
}

/// `SC_ERR_ARG` (`harness/cpp/shim/sc_shim.h`): the shim rejected the arguments.
const SC_ERR_ARG: f64 = -1002.0;

/// Builds a sphere; `Ok(Err(e))` when the construction fails.
fn rust_sphere(k: &Kernel, s: &SphereIn) -> Result<solidcore::Result<Sphere>, HarnessError> {
    rust_sphere_made(k, s, true)
}

/// [`rust_sphere`]; `make` false leaves the constructor-mode sphere without its NURB.
fn rust_sphere_made(k: &Kernel, s: &SphereIn, make: bool) -> Result<solidcore::Result<Sphere>, HarnessError> {
    let frame = Axis2Placement::new(v3(s.origin), v3(s.x), v3(s.y));
    if s.mode == 0 {
        return Ok(Sphere::create_canonical(k, &frame, s.radius));
    }
    let circle = match &s.circle {
        None => None,
        Some(c) => {
            let [d0, d1] = c.d;
            let e = Ellipse::new_circle(
                k,
                v3(c.c),
                v3(c.x),
                v3(c.y),
                Extent1d::from_min_max(d0, d1),
                c.r,
                &EllipseOptions::default(),
            )
            .map_err(|e| harness(format!("circle: {e}")))?;
            Some(Curve::from(e))
        }
    };
    let dom = s
        .dom
        .map_or(Extent2d::from_bounds(0.0, -90.0, 360.0, 90.0), |[a, b, c, d]| {
            Extent2d::from_bounds(a, b, c, d)
        });
    let built = Sphere::new(
        k,
        v3(s.origin),
        v3(s.x),
        v3(s.y),
        dom,
        s.radius,
        SphereOptions {
            swap_uv: s.swap,
            inside_out: s.inside_out,
            circle_nurb: circle,
        },
    );
    Ok(built.and_then(|mut sp| {
        if !make {
            return Ok(sp);
        }
        sp.make_nurb(k)?;
        if sp.base().base().gw_nurb().is_none() {
            return Err(SolidError::sm_err("difftest::surface_sphere"));
        }
        Ok(sp)
    }))
}

fn nurb_of(c: &Curve) -> Option<&NurbCurve> {
    match c.kind() {
        CurveKind::BSpline(b) => b.gw_nurb(),
        CurveKind::Ellipse(e) => e.base().gw_nurb(),
        CurveKind::Line(l) => l.base().gw_nurb(),
        CurveKind::Hyperbola(h) => h.base().gw_nurb(),
        CurveKind::Parabola(p) => p.base().gw_nurb(),
        _ => None,
    }
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

fn put_curve(o: &mut Vec<f64>, c: Option<&Curve>) {
    o.push(flag(c.is_some()));
    let Some(c) = c else { return };
    o.push(c.type_code() as f64);
    let ivl = step_interval(c);
    o.extend([ivl.min(), ivl.max()]);
    let cur = nurb_of(c);
    o.push(flag(cur.is_some()));
    if let Some(cur) = cur {
        o.push(cur.degree() as f64);
        o.push(cur.cpts().len() as f64);
        for p in cur.cpts() {
            o.extend([p.x, p.y, p.z, p.w]);
        }
        o.push(cur.knots().len() as f64);
        o.extend_from_slice(cur.knots());
    }
    match c.kind() {
        CurveKind::Ellipse(e) => {
            o.push(1.0);
            let p = e.position();
            push_v(o, p.origin());
            push_v(o, p.x_axis());
            push_v(o, p.y_axis());
            o.extend([e.x_radius(), e.y_radius(), flag(e.inside_out())]);
        }
        _ => o.push(0.0),
    }
}

fn put_state(o: &mut Vec<f64>, t: &Sphere) -> solidcore::Result<()> {
    let s = t.base();
    o.extend([
        flag(t.is_kind_of(SPHERE_TYPE)?),
        flag(t.is_kind_of(SURF_OF_REVOLUTION_TYPE)?),
        flag(t.is_kind_of(B_SPLINE_SURFACE_TYPE)?),
        flag(t.is_kind_of(TYPE)?),
        t.type_code() as f64,
        flag(s.swap_uv()),
        flag(s.planar_generator()),
        flag(t.inside_out()),
        t.radius(),
    ]);
    let pos = s.position();
    push_v(o, pos.origin());
    push_v(o, pos.x_axis());
    push_v(o, pos.y_axis());
    let d = t.step_uv_domain();
    o.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
    let mx = t.max_analytic_domain();
    o.extend([mx.min().x, mx.min().y, mx.max().x, mx.max().y]);
    let pc = s.polar_converter();
    o.push(flag(s.have_polar_conversion()));
    let angles = pc.angles();
    o.push(angles.len() as f64);
    if let (Some(a), Some(b)) = (angles.first(), angles.last()) {
        o.extend([*a, *b]);
    }
    let polar_curve = s.polar_curve().map(|e| Curve::from(e.clone()));
    put_curve(o, polar_curve.as_ref());
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

fn param_of(p: u8) -> SurfParamType {
    if p == 0 {
        SurfParamType::U
    } else {
        SurfParamType::V
    }
}

fn dom_of(d: DomIn, natural: Extent2d) -> Extent2d {
    match d {
        DomIn::Natural => natural,
        DomIn::Explicit { d: [a, b, c, e] } => Extent2d::from_bounds(a, b, c, e),
    }
}

fn natural(s: &Sphere) -> Extent2d {
    s.base().base().natural_uv_domain()
}

fn rust_plane(p: &PlaneIn) -> Plane {
    match p {
        PlaneIn::Bounded {
            origin,
            x,
            y,
            scale,
            dom,
        } => Plane::new(
            v3(*origin),
            v3(*x),
            v3(*y),
            Vec2::new(scale[0], scale[1]),
            &Extent2d::from_bounds(dom[0], dom[1], dom[2], dom[3]),
        ),
        PlaneIn::Infinite { origin, normal } => Plane::from_normal(v3(*origin), v3(*normal)),
    }
}

/// `more ncurves curves ntypes types ndevs devs nuv1 nuv2 nnull1 nnull2`.
fn put_intersection(o: &mut Vec<f64>, more: bool, out: &SurfaceIntersection) {
    o.push(flag(more));
    o.push(out.curves_3d.len() as f64);
    for c in &out.curves_3d {
        put_curve(o, Some(c));
    }
    o.push(out.curve_types.len() as f64);
    o.extend(out.curve_types.iter().map(|t| f64::from(*t as i32)));
    o.push(out.deviations.len() as f64);
    o.extend_from_slice(&out.deviations);
    o.push(out.surface1_uv_curves.len() as f64);
    o.push(out.surface2_uv_curves.len() as f64);
    o.push(out.surface1_uv_curves.iter().filter(|c| c.is_none()).count() as f64);
    o.push(out.surface2_uv_curves.iter().filter(|c| c.is_none()).count() as f64);
}

/// What a Rust op run produced: the record continues (`Ok`), or the op failed with an
/// error whose status ends the record (`Err`). Ops that write the state after a
/// failure push it themselves.
type OpRun = solidcore::Result<()>;

/// An intersection's record after the other operand's status: the op's status then, on
/// success, the intersection.
fn push_isect(o: &mut Vec<f64>, r: solidcore::Result<(bool, SurfaceIntersection)>) -> OpRun {
    match r {
        Ok((more, out)) => {
            o.push(0.0);
            put_intersection(o, more, &out);
            Ok(())
        }
        Err(e) if e.is_unported() => Err(e),
        Err(e) => {
            o.push(status(&e));
            Ok(())
        }
    }
}

/// Runs the op on the Rust sphere, appending its status and outputs. `Err` from the
/// harness only; `Ok(Err(e))` when the op failed before writing its status.
#[allow(clippy::too_many_lines)] // one arm per op
fn rust_op(
    k: &Kernel,
    t: &mut Sphere,
    op: &OpIn,
    o: &mut Vec<f64>,
) -> Result<OpRun, HarnessError> {
    let r: OpRun = match op {
        OpIn::Build {} => put_state(o, t),
        OpIn::EvalStep { u, v, k: kk, point } => {
            let uv = Point2::new(*u, *v);
            if *point {
                t.evaluate_step_point(uv).map(|p| {
                    o.push(0.0);
                    push_v(o, p);
                })
            } else {
                t.evaluate_step(uv, *kk, *kk).map(|d| {
                    o.push(0.0);
                    for v in d {
                        push_v(o, v);
                    }
                })
            }
        }
        OpIn::ToStep { u, v } => t
            .convert_uv_from_nurbs_to_step(k, Point2::new(*u, *v))
            .map(|p| o.extend([0.0, p.x, p.y])),
        OpIn::ToNurbs { u, v } => t
            .convert_uv_from_step_to_nurbs(k, Point2::new(*u, *v))
            .map(|p| o.extend([0.0, p.x, p.y])),
        OpIn::Normal { u, v } => t.evaluate_normal(Point2::new(*u, *v)).map(|n| {
            o.push(0.0);
            push_v(o, n);
        }),
        OpIn::Iso { param, iso, dom } => {
            let dom = dom.map(|[a, b, c, d]| Extent2d::from_bounds(a, b, c, d));
            // the shim's guard: the base extraction must succeed
            let probe = t.base().base().create_iso_parametric_curve(
                k,
                param_of(*param),
                *iso,
                IsoCurveOptions {
                    domain: dom,
                    uv_iso_curve: false,
                },
            );
            match probe {
                Err(e) if e.is_unported() => Err(e),
                Err(_) => {
                    o.push(SC_ERR_ARG);
                    Ok(())
                }
                Ok(_) => t
                    .create_iso_parametric_curve(k, param_of(*param), *iso, dom.as_ref(), false)
                    .map(|c| {
                        o.extend([0.0, c.max_gap_3d]);
                        put_curve(o, Some(&c.curve));
                    }),
            }
        }
        OpIn::Offset { dist, tol, rebuild } => {
            t.create_offset_surface(k, *dist, *tol).and_then(|off| {
                o.push(0.0);
                o.push(flag(off.is_some()));
                if let Some(mut off) = off {
                    if *rebuild {
                        match off.rebuild_step_from_nurb_parameters(k) {
                            Err(e) if e.is_unported() => return Err(e),
                            Err(e) => o.push(status(&e)),
                            Ok(()) => o.push(0.0),
                        }
                    }
                    put_state(o, &off)?;
                }
                Ok(())
            })
        }
        OpIn::Rebuild {} => match t.rebuild_step_from_nurb_parameters(k) {
            Err(e) if e.is_unported() => Err(e),
            r => {
                o.push(r.map_or_else(|e| status(&e), |()| 0.0));
                put_state(o, t)
            }
        },
        OpIn::Transform {
            origin,
            x,
            y,
            scale,
        } => {
            let a2p = Axis2Placement::new(v3(*origin), v3(*x), v3(*y));
            match t.transform(k, &a2p, scale.map(v3)) {
                Err(e) if e.is_unported() => Err(e),
                r => {
                    o.push(r.map_or_else(|e| status(&e), |()| 0.0));
                    put_state(o, t)
                }
            }
        }
        OpIn::Eq { other } => match rust_sphere(k, other)? {
            Err(e) if e.is_unported() => Err(e),
            Err(e) => {
                o.push(status(&e));
                Ok(())
            }
            Ok(other) => {
                o.extend([0.0, flag(t.approx_eq(&other)), flag(other.approx_eq(t))]);
                Ok(())
            }
        },
        OpIn::Copy {} => {
            let c = t.copy();
            o.push(0.0);
            put_state(o, &c).map(|()| o.push(flag(t.approx_eq(&c))))
        }
        OpIn::Closed { param, dom } => {
            let p = match param {
                0 => SurfParamType::U,
                1 => SurfParamType::V,
                _ => SurfParamType::Both,
            };
            let (closed, cont) = t.is_closed(&dom_of(*dom, natural(t)), p);
            o.extend([
                flag(closed),
                f64::from(cont as i32),
                flag(t.is_closed_u()),
                flag(t.is_closed_v()),
            ]);
            Ok(())
        }
        OpIn::StepInv {
            dom,
            point,
            tol,
            guess,
        } => {
            let d = dom.map_or_else(
                || t.step_uv_domain(),
                |[a, b, c, e]| Extent2d::from_bounds(a, b, c, e),
            );
            let inv = t.step_inversion(&d, v3(*point), *tol, guess.map(|[a, b]| Point2::new(a, b)));
            o.extend([
                if inv.on_surface { 0.0 } else { 1001.0 },
                inv.uv.x,
                inv.uv.y,
                f64::from(inv.location as i32),
            ]);
            Ok(())
        }
        OpIn::Trim { c, x, y, d, tol } => t
            .create_trimmed_circle_segments(
                k,
                v3(*c),
                v3(*x),
                v3(*y),
                &Extent1d::from_min_max(d[0], d[1]),
                *tol,
            )
            .map(|curves| {
                o.push(0.0);
                o.push(curves.len() as f64);
                for c in &curves {
                    put_curve(o, Some(c));
                }
            }),
        OpIn::IsectPlane { plane, args } => {
            o.push(0.0);
            let p = rust_plane(plane);
            let nat = natural(t);
            let pnat = p.natural_uv_domain();
            push_isect(
                o,
                t.intersect_with_plane(
                    k,
                    &dom_of(args.dom, nat),
                    &p,
                    &dom_of(args.other_dom, pnat),
                    args.tol,
                ),
            )
        }
        OpIn::Global { plane, args } => {
            o.push(0.0);
            let p = rust_plane(plane);
            let nat = natural(t);
            let pnat = p.natural_uv_domain();
            let other = Surface::from(p);
            match t.global_surface_intersect(
                k,
                &dom_of(args.dom, nat),
                &other,
                &dom_of(args.other_dom, pnat),
                [true, true],
                args.tol,
                None,
            ) {
                Ok(out) => {
                    o.push(0.0);
                    // (`more` is not reported by the general entry point)
                    put_intersection(o, false, &out);
                    Ok(())
                }
                Err(e) if e.is_unported() => Err(e),
                Err(e) => {
                    o.push(status(&e));
                    Ok(())
                }
            }
        }
        OpIn::IsectSphere { other, args } => match rust_sphere(k, other)? {
            Err(e) if e.is_unported() => Err(e),
            Err(e) => {
                // the other sphere's construction status replaces the first value
                o.push(status(&e));
                Ok(())
            }
            Ok(other) => {
                o.push(0.0);
                let nat = natural(t);
                let onat = natural(&other);
                push_isect(
                    o,
                    t.intersect_with_sphere(
                        k,
                        &dom_of(args.dom, nat),
                        &other,
                        &dom_of(args.other_dom, onat),
                        args.tol,
                    ),
                )
            }
        },
        OpIn::IsectCone { cone, args } => {
            let frame = Axis2Placement::new(v3(cone.origin), v3(cone.x), v3(cone.y));
            match Cone::create_canonical(k, &frame, cone.r0, cone.r1, cone.h, false, false) {
                Err(e) if e.is_unported() => Err(e),
                Err(e) => {
                    o.push(status(&e));
                    Ok(())
                }
                Ok(c) => {
                    o.push(0.0);
                    let nat = natural(t);
                    let cnat = c.rev().base().natural_uv_domain();
                    push_isect(
                        o,
                        t.intersect_with_cone(
                            k,
                            &dom_of(args.dom, nat),
                            &c,
                            &dom_of(args.other_dom, cnat),
                            args.tol,
                        ),
                    )
                }
            }
        }
        OpIn::IsNurb {
            scale,
            net_scale,
            perturb,
        } => {
            let mut test = t.base().base().clone();
            let edited = test.get_or_create_gw_nurb().and_then(|nurb| {
                let ix = nurb.indices();
                for i in 0..=ix.n {
                    for j in 0..=ix.m {
                        let p = nurb.cpt_mut(i, j)?;
                        p.x *= net_scale[0];
                        p.y *= net_scale[1];
                        p.z *= net_scale[2];
                    }
                }
                for pt in perturb {
                    if pt.i > ix.n || pt.j > ix.m {
                        continue;
                    }
                    let p = nurb.cpt_mut(pt.i, pt.j)?;
                    p.x += pt.d[0];
                    p.y += pt.d[1];
                    p.z += pt.d[2];
                }
                Ok(())
            });
            match edited.and_then(|()| Sphere::is_nurb_surface_sphere(k, &test, *scale)) {
                Err(e) if e.is_unported() => Err(e),
                // upstream returns the status of a failed SER call as a truthy result
                Err(e) => {
                    o.extend([status(&e).max(1.0), 0.0]);
                    Ok(())
                }
                Ok(None) => {
                    o.extend([0.0, 0.0]);
                    Ok(())
                }
                Ok(Some(found)) => {
                    o.extend([1.0, 1.0]);
                    put_state(o, &found)
                }
            }
        }
        OpIn::TrimDomain { dom } => {
            let [a, b, c, d] = *dom;
            // (upstream writes the domain back; the inherited body never changes it)
            match t.trim_with_domain(k, &Extent2d::from_bounds(a, b, c, d)) {
                Err(e) if e.is_unported() => Err(e),
                r => {
                    o.push(r.map_or_else(|e| status(&e), |()| 0.0));
                    o.extend_from_slice(dom);
                    put_state(o, t)
                }
            }
        }
    };
    Ok(r)
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let mut o = Vec::new();
    match rust_sphere(&k, &i.sphere)? {
        Err(e) => {
            if let Some(out) = Output::unported(&e) {
                return Ok(out);
            }
            o.push(status(&e));
        }
        Ok(mut t) => {
            o.push(0.0);
            let at = o.len();
            if let Err(e) = rust_op(&k, &mut t, &i.op, &mut o)? {
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

fn unit_vec(rng: &mut Rng) -> [f64; 3] {
    loop {
        let [x, y, z] = rng.point3(-1.0, 1.0);
        let l = (x * x + y * y + z * z).sqrt();
        if l > 0.2 && l <= 1.0 {
            return [x / l, y / l, z / l];
        }
    }
}

fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx]
}

fn normalize(a: [f64; 3]) -> [f64; 3] {
    let [x, y, z] = a;
    let l = (x * x + y * y + z * z).sqrt();
    [x / l, y / l, z / l]
}

fn add(a: [f64; 3], b: [f64; 3], s: f64) -> [f64; 3] {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    [ax + s * bx, ay + s * by, az + s * bz]
}

/// A unit frame (X, Y); retried until both axes pass the FFI's unit check.
fn frame(rng: &mut Rng) -> ([f64; 3], [f64; 3]) {
    match rng.below(3) {
        0 => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        1 => ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        _ => loop {
            let z = unit_vec(rng);
            let t = unit_vec(rng);
            let y = normalize(cross3(z, t));
            let x = normalize(cross3(y, z));
            let unit = |v: [f64; 3]| {
                let [a, b, c] = v;
                ((a * a + b * b + c * c) - 1.0).abs() <= 1e-12
            };
            if unit(x) && unit(y) {
                return (x, y);
            }
        },
    }
}

fn gen_sphere(rng: &mut Rng) -> SphereIn {
    let origin = if rng.chance(0.3) {
        [0.0, 0.0, 0.0]
    } else {
        rng.point3(-5.0, 5.0)
    };
    let (x, y) = frame(rng);
    let mode = u8::from(rng.chance(0.55));
    let radius = match rng.below(14) {
        0 => *rng.choose(&[0.0, 1e-13, -1e-13]).unwrap_or(&0.0),
        1 if mode == 0 => -rng.uniform(0.5, 6.0),
        _ => rng.uniform(0.5, 6.0),
    };
    // (the constructor needs a positive radius: the FFI rejects the others)
    let mode = if radius <= 1e-6 { 0 } else { mode };
    let lat = |rng: &mut Rng| -> [f64; 2] {
        match rng.below(4) {
            0 | 1 => [-90.0, 90.0],
            2 => {
                let a = rng.uniform(-90.0, 30.0);
                [a, a + rng.uniform(5.0, 60.0)]
            }
            _ => [-90.0, rng.uniform(-60.0, 90.0)],
        }
    };
    let lon = |rng: &mut Rng| -> [f64; 2] {
        match rng.below(4) {
            0 | 1 => [0.0, 360.0],
            2 => {
                let a = rng.uniform(-360.0, 0.0);
                [a, a + rng.uniform(10.0, 360.0)]
            }
            _ => {
                let a = rng.uniform(0.0, 200.0);
                [a, (a + rng.uniform(10.0, 160.0)).min(360.0)]
            }
        }
    };
    let dom = (mode == 1 && rng.chance(0.6)).then(|| {
        let [u0, u1] = lon(rng);
        let [v0, v1] = lat(rng);
        [u0, v0, u1, v1]
    });
    // an explicit generator: the meridian circle the constructor would make over the V
    // interval (or a little off it)
    let circle = (mode == 1 && rng.chance(0.15) && radius > 1e-6).then(|| {
        let [_, v0, _, v1] = dom.unwrap_or([0.0, -90.0, 360.0, 90.0]);
        let d = if rng.chance(0.7) {
            [v0, v1]
        } else {
            [v0, (v1 - rng.uniform(1.0, 5.0)).max(v0 + 1.0)]
        };
        CircleIn {
            c: origin,
            x,
            y: cross3(x, y),
            d,
            r: radius,
        }
    });
    SphereIn {
        mode,
        origin,
        x,
        y,
        radius,
        swap: mode == 1 && rng.chance(0.4),
        inside_out: mode == 1 && rng.chance(0.4),
        dom,
        circle,
    }
}

/// A sub-box of `d` or `d` itself.
fn some_dom(rng: &mut Rng, d: Extent2d) -> DomIn {
    if rng.chance(0.6) {
        DomIn::Natural
    } else {
        let a = d.evaluate(rng.uniform(0.0, 0.4), rng.uniform(0.0, 0.4));
        let b = d.evaluate(rng.uniform(0.6, 1.0), rng.uniform(0.6, 1.0));
        DomIn::Explicit {
            d: [a.x, a.y, b.x, b.y],
        }
    }
}

fn gen_args(rng: &mut Rng, nd: Extent2d, other: Extent2d) -> Args {
    Args {
        dom: some_dom(rng, nd),
        other_dom: some_dom(rng, other),
        tol: rng.choose(&[None, None, Some(1e-6), Some(1e-3), Some(0.05)]).copied().flatten(),
    }
}

#[allow(clippy::too_many_lines)] // one arm per op
fn gen_case(rng: &mut Rng) -> Option<Inputs> {
    let k = Kernel::default();
    let sphere = gen_sphere(rng);
    // the Rust sphere sizes the parameters (both sides get the same numbers)
    let built = rust_sphere(&k, &sphere).ok()?.ok();
    let nd = built
        .as_ref()
        .map_or(Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0), natural);
    let sd = built
        .as_ref()
        .map_or(Extent2d::from_bounds(0.0, -90.0, 360.0, 90.0), |b| {
            b.step_uv_domain()
        });
    let pick = |rng: &mut Rng, lo: f64, hi: f64| -> f64 {
        match rng.below(10) {
            0 => lo,
            1 => hi,
            2 => lo - rng.uniform(0.0, 0.5) * (hi - lo),
            _ => lo + rng.unit() * (hi - lo),
        }
    };
    let r = sphere.radius.abs().max(0.5);
    let op = match rng.below(24) {
        0 => OpIn::Build {},
        1..=3 => OpIn::EvalStep {
            u: pick(rng, sd.min().x, sd.max().x),
            v: pick(rng, sd.min().y, sd.max().y),
            k: rng.below(4) as usize,
            point: rng.chance(0.2),
        },
        4 => OpIn::ToStep {
            u: pick(rng, nd.min().x, nd.max().x),
            v: pick(rng, nd.min().y, nd.max().y),
        },
        5 => OpIn::ToNurbs {
            u: pick(rng, sd.min().x, sd.max().x),
            v: pick(rng, sd.min().y, sd.max().y),
        },
        6 => OpIn::Normal {
            u: pick(rng, nd.min().x, nd.max().x),
            v: pick(rng, nd.min().y, nd.max().y),
        },
        7 | 8 => {
            let param = rng.below(2) as u8;
            let (lo, hi) = if param == 0 {
                (nd.min().x, nd.max().x)
            } else {
                (nd.min().y, nd.max().y)
            };
            let dom = rng.chance(0.35).then(|| {
                let a = nd.evaluate(rng.uniform(0.0, 0.4), rng.uniform(0.0, 0.4));
                let b = nd.evaluate(rng.uniform(0.6, 1.0), rng.uniform(0.6, 1.0));
                [a.x, a.y, b.x, b.y]
            });
            OpIn::Iso {
                param,
                iso: pick(rng, lo, hi),
                dom,
            }
        }
        9 => OpIn::Offset {
            dist: rng.uniform(-1.5, 1.5) * r,
            tol: *rng.choose(&[0.0, 1e-6, 1e-3, 0.1])?,
            rebuild: rng.chance(0.4),
        },
        10 => OpIn::Rebuild {},
        11 => {
            let identity = rng.chance(0.4);
            let (x, y) = if identity {
                ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0])
            } else {
                frame(rng)
            };
            let scale = match rng.below(4) {
                0 => None,
                1 => Some([1.0, 1.0, 1.0]),
                2 => {
                    let s = rng.uniform(0.5, 2.0);
                    Some([s, s, s])
                }
                _ => Some([
                    rng.uniform(0.5, 2.0),
                    rng.uniform(0.5, 2.0),
                    *rng.choose(&[1.0, -1.0, 0.0])?,
                ]),
            };
            OpIn::Transform {
                origin: if identity {
                    [0.0; 3]
                } else {
                    rng.point3(-2.0, 2.0)
                },
                x,
                y,
                scale,
            }
        }
        12 => {
            let mut other = sphere.clone();
            match rng.below(5) {
                0 => {}
                1 => other.swap = !other.swap,
                2 => other.radius += 1e-3,
                3 => other.inside_out = !other.inside_out,
                _ => other.origin = add(other.origin, [1e-3, 0.0, 0.0], 1.0),
            }
            OpIn::Eq { other }
        }
        // (a sub-draw keeps the other ops' generated cases unchanged)
        13 => {
            if rng.below(4) == 0 {
                OpIn::Copy {}
            } else {
                OpIn::TrimDomain {
                    dom: gen_trim_domain(rng, nd)?,
                }
            }
        }
        14 => OpIn::Closed {
            param: rng.below(3) as u8,
            dom: some_dom(rng, nd),
        },
        15 => {
            // a point on (or near) the sphere
            let dir = unit_vec(rng);
            let dist = match rng.below(6) {
                0 => 0.0,
                1 => sphere.radius.abs() * rng.uniform(0.9, 1.1),
                _ => sphere.radius.abs(),
            };
            let pole = rng.chance(0.15);
            let d = if pole { [0.0, 0.0, 1.0] } else { dir };
            OpIn::StepInv {
                dom: rng.chance(0.3).then(|| {
                    let a = sd.evaluate(rng.uniform(0.0, 0.4), rng.uniform(0.0, 0.4));
                    let b = sd.evaluate(rng.uniform(0.6, 1.0), rng.uniform(0.6, 1.0));
                    [a.x, a.y, b.x, b.y]
                }),
                point: add(sphere.origin, d, dist),
                tol: *rng.choose(&[0.0, 1e-9, 1e-6, 0.01])?,
                guess: rng
                    .chance(0.3)
                    .then(|| [rng.uniform(0.0, 360.0), rng.uniform(-90.0, 90.0)]),
            }
        }
        16 | 17 => {
            // a circle on the sphere: a plane section at height h along n
            let (x, y) = frame(rng);
            let n = cross3(x, y);
            let h = match rng.below(6) {
                0 => sphere.radius.abs(),
                1 => 0.0,
                _ => rng.uniform(-0.99, 0.99) * sphere.radius.abs(),
            };
            let c = add(sphere.origin, n, h);
            let ivl = match rng.below(3) {
                0 => [0.0, 360.0],
                1 => {
                    let a = rng.uniform(-360.0, 0.0);
                    [a, a + rng.uniform(20.0, 360.0)]
                }
                _ => {
                    let a = rng.uniform(0.0, 200.0);
                    [a, (a + rng.uniform(10.0, 160.0)).min(360.0)]
                }
            };
            OpIn::Trim {
                c,
                x,
                y,
                d: ivl,
                tol: *rng.choose(&[0.0, 1e-9, 1e-6, 1e-3])?,
            }
        }
        18 | 19 => {
            let (x, y) = frame(rng);
            let n = cross3(x, y);
            let h = match rng.below(8) {
                0 => sphere.radius.abs(),
                1 => -sphere.radius.abs(),
                2 => 1.5 * sphere.radius.abs(),
                _ => rng.uniform(-1.05, 1.05) * sphere.radius.abs(),
            };
            let origin = add(sphere.origin, n, h);
            let plane = if rng.chance(0.5) {
                PlaneIn::Infinite { origin, normal: n }
            } else {
                let a = rng.uniform(0.3, 2.0) * r;
                let b = rng.uniform(0.3, 2.0) * r;
                PlaneIn::Bounded {
                    origin: add(origin, x, rng.uniform(-0.5, 0.5) * r),
                    x,
                    y,
                    scale: [1.0, 1.0],
                    dom: [-a, -b, a, b],
                }
            };
            let pnat = rust_plane(&plane).natural_uv_domain();
            let args = gen_args(rng, nd, pnat);
            if rng.chance(0.25) {
                OpIn::Global { plane, args }
            } else {
                OpIn::IsectPlane { plane, args }
            }
        }
        20 | 21 => {
            let (x, y) = frame(rng);
            let r2 = rng.uniform(0.5, 6.0);
            let dist = match rng.below(8) {
                0 => 0.0,
                1 => (sphere.radius.abs() - r2).abs(),
                2 => sphere.radius.abs() + r2,
                3 => 2.0 * (sphere.radius.abs() + r2),
                _ => rng.uniform(
                    (sphere.radius.abs() - r2).abs() * 0.9,
                    (sphere.radius.abs() + r2) * 1.1,
                ),
            };
            let origin = add(sphere.origin, unit_vec(rng), dist);
            let swap = rng.chance(0.3);
            let inside_out = rng.chance(0.3);
            let other = if rng.chance(0.5) {
                SphereIn {
                    mode: 0,
                    origin,
                    x,
                    y,
                    radius: r2,
                    swap: false,
                    inside_out: false,
                    dom: None,
                    circle: None,
                }
            } else {
                let u0 = rng.uniform(-90.0, 30.0);
                SphereIn {
                    mode: 1,
                    origin,
                    x,
                    y,
                    radius: r2,
                    swap,
                    inside_out,
                    dom: rng
                        .chance(0.7)
                        .then(|| [u0, -90.0, u0 + rng.uniform(100.0, 360.0), 90.0]),
                    circle: None,
                }
            };
            let onat = rust_sphere(&k, &other)
                .ok()?
                .ok()
                .map_or(Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0), |b| natural(&b));
            OpIn::IsectSphere {
                other,
                args: gen_args(rng, nd, onat),
            }
        }
        22 => {
            // a cone whose axis passes through the sphere centre (or nearly)
            let (x, y) = frame(rng);
            let axis = cross3(x, y);
            let r0 = rng.uniform(0.1, 2.0) * r;
            let r1 = if rng.chance(0.2) {
                r0
            } else {
                rng.uniform(0.0, 2.0) * r
            };
            let h = rng.uniform(0.5, 3.0) * r;
            let off = rng.uniform(-0.6, 0.3) * h;
            let mut origin = add(sphere.origin, axis, off);
            if rng.chance(0.2) {
                origin = add(origin, x, rng.uniform(0.1, 1.0) * r);
            }
            let cone = ConeIn {
                origin,
                x,
                y,
                r0,
                r1,
                h,
            };
            let frame = Axis2Placement::new(v3(origin), v3(x), v3(y));
            let cnat = Cone::create_canonical(&k, &frame, r0, r1, h, false, false)
                .ok()
                .map_or(Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0), |c| {
                    c.rev().base().natural_uv_domain()
                });
            OpIn::IsectCone {
                cone,
                args: gen_args(rng, nd, cnat),
            }
        }
        _ => {
            let n = rng.below(3) as usize;
            let scale = *rng.choose(&[1.0, 1.0, 0.1, 10.0, 1000.0])?;
            let net_scale = match rng.below(5) {
                0 => {
                    let s = rng.uniform(0.5, 2.0);
                    [s, s, s]
                }
                1 => [rng.uniform(0.5, 2.0), 1.0, 1.0],
                _ => [1.0, 1.0, 1.0],
            };
            OpIn::IsNurb {
                scale,
                net_scale,
                perturb: (0..n)
                    .map(|_| PerturbIn {
                        i: rng.below(7) as usize,
                        j: rng.below(7) as usize,
                        d: *rng
                            .choose(&[[1e-3, 0.0, 0.0], [0.0, 1e-9, 0.0], [0.1, 0.1, 0.0]])
                            .unwrap_or(&[0.0; 3]),
                    })
                    .collect(),
            }
        }
    };
    Some(Inputs { sphere, op })
}

/// A `TrimWithDomain` domain for the NURB domain `nd`: the whole domain, one sticking
/// out of it, one direction trimmed, a thin strip, one within `SM_EFF_ZERO_PARAM` of
/// the edge, or a sub-domain.
fn gen_trim_domain(rng: &mut Rng, nd: Extent2d) -> Option<[f64; 4]> {
    let (lo, hi) = (nd.min(), nd.max());
    let (du, dv) = (hi.x - lo.x, hi.y - lo.y);
    Some(match rng.below(8) {
        0 => [lo.x, lo.y, hi.x, hi.y],
        1 => {
            let e = rng.uniform(1e-6, 0.3);
            if rng.chance(0.5) {
                [lo.x - e * du, lo.y, hi.x, hi.y]
            } else {
                [lo.x, lo.y, hi.x, hi.y + e * dv]
            }
        }
        2 => {
            let a = rng.uniform(0.0, 0.45);
            let b = rng.uniform(0.55, 1.0);
            if rng.chance(0.5) {
                [lo.x + a * du, lo.y, lo.x + b * du, hi.y]
            } else {
                [lo.x, lo.y + a * dv, hi.x, lo.y + b * dv]
            }
        }
        3 => {
            let a = rng.uniform(0.0, 0.9);
            let w = *rng.choose(&[1e-3, 1e-2, 0.05])?;
            [lo.x + a * du, lo.y, lo.x + (a + w) * du, hi.y]
        }
        4 => [lo.x - 1e-13, lo.y, hi.x, hi.y - 0.25 * dv],
        _ => {
            let a = nd.evaluate(rng.uniform(0.0, 0.45), rng.uniform(0.0, 0.45));
            let b = nd.evaluate(rng.uniform(0.55, 1.0), rng.uniform(0.55, 1.0));
            [a.x, a.y, b.x, b.y]
        }
    })
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.sphere"
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

// ---- lazy NURB (`surface.surface_p2` `common_const_make` cases) ------------------------

/// A constructor-mode sphere of a `common_const_make` case.
fn const_make_input(value: Value) -> Result<SphereIn, HarnessError> {
    let s: SphereIn = serde_json::from_value(value).map_err(|e| harness(e.to_string()))?;
    if s.mode == 0 {
        return Err(harness("const_make: constructor mode required"));
    }
    Ok(s)
}

/// The native side of a sphere `common_const_make` case.
pub(super) fn const_make_cpp(value: Value) -> Result<Output, HarnessError> {
    let s = const_make_input(value)?;
    let flat = ffi::const_make(&ffi_sphere(&s)).map_err(|e| harness(e.to_string()))?;
    super::surface_cone_p1::const_make_nurb_cpp(&flat)
}

/// The Rust side: the sphere as its constructor leaves it (no NURB), then the shared
/// const-query, `Copy`, `==` and `Transform` sequence.
pub(super) fn const_make_rust(value: Value) -> Result<Output, HarnessError> {
    let s = const_make_input(value)?;
    let k = Kernel::default();
    let built = rust_sphere_made(&k, &s, false)?.map(Surface::from);
    super::surface_cone_p1::const_make_nurb_rust(&k, built)
}

/// `common_const_make` cases for the sphere: the constructor's sphere with no NURB, in
/// the swap and inside-out flags, partial domains, caps ending at a pole and an explicit
/// generator.
pub(super) fn const_make_cases() -> Vec<Case> {
    type Shape = (&'static str, Option<[f64; 4]>, Option<CircleIn>);
    let mut cases = Vec::new();
    let frame = |origin: [f64; 3]| (origin, [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let shapes: [Shape; 7] = [
        ("full", None, None),
        ("part", Some([30.0, -60.0, 200.0, 45.0]), None),
        ("north_cap", Some([0.0, 60.0, 360.0, 90.0]), None),
        ("south_cap", Some([0.0, -90.0, 360.0, -60.0]), None),
        ("hemi", Some([-90.0, 0.0, 90.0, 90.0]), None),
        ("wrap", Some([-180.0, -90.0, 180.0, 90.0]), None),
        ("gen", None, Some(CircleIn { c: [0.5, -1.0, 2.0], x: [0.0, 0.0, 1.0], y: [0.0, 1.0, 0.0], d: [-90.0, 90.0], r: 2.0 })),
    ];
    for swap in [false, true] {
        for inside_out in [false, true] {
            for (name, dom, circle) in &shapes {
                let (origin, x, y) = frame([0.5, -1.0, 2.0]);
                let sphere = SphereIn { mode: 1, origin, x, y, radius: 2.0, swap, inside_out, dom: *dom, circle: circle.clone() };
                let Ok(sphere) = serde_json::to_value(&sphere) else { continue };
                cases.push(Case {
                    unit: "surface.surface_p2".into(),
                    name: format!("common_const_make_sphere_{name}_{swap}_{inside_out}"),
                    source: crate::case::CaseSource::Generated { seed: 76 },
                    inputs: serde_json::json!({"common_const_make": true, "common_sphere": sphere}),
                });
            }
        }
    }
    cases
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn op_name(op: &OpIn) -> &'static str {
        match op {
            OpIn::Build {} => "build",
            OpIn::EvalStep { .. } => "eval_step",
            OpIn::ToStep { .. } => "to_step",
            OpIn::ToNurbs { .. } => "to_nurbs",
            OpIn::Normal { .. } => "normal",
            OpIn::Iso { .. } => "iso",
            OpIn::Offset { .. } => "offset",
            OpIn::Rebuild {} => "rebuild",
            OpIn::Transform { .. } => "transform",
            OpIn::Eq { .. } => "eq",
            OpIn::Copy {} => "copy",
            OpIn::Closed { .. } => "closed",
            OpIn::StepInv { .. } => "step_inv",
            OpIn::Trim { .. } => "trim",
            OpIn::IsectPlane { .. } => "isect_plane",
            OpIn::IsectSphere { .. } => "isect_sphere",
            OpIn::IsectCone { .. } => "isect_cone",
            OpIn::Global { .. } => "global",
            OpIn::IsNurb { .. } => "is_nurb",
            OpIn::TrimDomain { .. } => "trim_domain",
        }
    }

    /// Prints the generated case of `SPHERE_SEED` and both sides' records:
    /// `SPHERE_SEED=<seed> cargo test --profile difftest -p difftest --features cpp --lib
    /// sphere_dump_case -- --ignored --nocapture`.
    #[test]
    #[ignore = "debugging aid"]
    fn sphere_dump_case() {
        let seed: u64 = std::env::var("SPHERE_SEED")
            .expect("SPHERE_SEED")
            .parse()
            .expect("seed");
        let mut rng = Rng::new(seed);
        let v = Runner.generate(&mut rng).expect("case");
        eprintln!("{v}");
        let i: Inputs = serde_json::from_value(v).expect("inputs");
        eprintln!("rust: {:?}", rust(&i).expect("rust").values);
        eprintln!("cpp: {:?}", cpp(&i).expect("cpp").values);
    }

    /// The generators reach a successful construction and a successful run of every op
    /// on the Rust side.
    #[test]
    fn generated_cases_cover_every_op() {
        let mut tally: BTreeMap<(String, String), usize> = BTreeMap::new();
        for seed in 0..1500u64 {
            let mut rng = Rng::new(0x7370_0000 + seed);
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
            "to_step",
            "to_nurbs",
            "normal",
            "iso",
            "offset",
            "rebuild",
            "transform",
            "eq",
            "copy",
            "closed",
            "step_inv",
            "trim",
            "isect_plane",
            "isect_sphere",
            "isect_cone",
            "is_nurb",
            "trim_domain",
        ] {
            let ok = tally.iter().any(|((o, k), _)| {
                o == op
                    && (k == "build:Some(0.0) status:Some(0.0)"
                        || k == "build:Some(0.0) status:None")
            });
            assert!(ok, "{op} never succeeds");
        }
    }
}
