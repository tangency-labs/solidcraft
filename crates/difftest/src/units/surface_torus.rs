//! Runner for unit `surface.torus`: `SmTorus` (construction through `CreateCanonical`,
//! `operator==`, `Copy`, `CreateOffsetSurface`, the STEP/NURB conversions, STEP
//! evaluation, iso curves, `RebuildSTEPFromNURBParameters`, `Transform`, and the
//! inherited `SmSurfOfRevolution::TrimWithDomain`, `IntersectWithPlane` and
//! `GlobalSurfaceIntersect` with a plane; `IsNurbSurfaceTorus` on its NURB, fixtures
//! only).
//!
//! C++: `solidcore_ffi::units::surface_torus` (upstream `source/SMLib/src/SmTorus.cpp`).
//! Rust: `solidcore::surface::torus`.
//!
//! A case is `torus` (placement, radii, flags, optional domain and generator circle)
//! plus `op` and its parameters. Both sides produce the same flat record (layout in
//! the FFI module docs), split into `build` (the construction status), `status` (the
//! op status) and `out` (the rest). Statuses are `0` (`SM_SUCCESS`) or the
//! `SmStatus`; `-1` on the Rust side for an error upstream cannot produce.
//!
//! Deferred cases: `Transform` other than the identity or a rejected scale
//! (`SmSurfOfRevolution::Transform`, `surface.surf_of_revolution_p3`); a plane
//! intersection reaching the general intersector (`GlobalSurfaceIntersect`).

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
use solidcore::nurbs::b_spline_surface::{BSplineSurface, IsoCurveOptions, SurfParamType};
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::surface::surface::p2::{
    B_SPLINE_SURFACE_TYPE, SURF_OF_REVOLUTION_TYPE, TORUS_TYPE, TYPE,
};
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::p2::{Surface, SurfaceIntersection};
use solidcore::surface::torus::{Torus, TorusOptions};
use solidcore::SolidError;
use solidcore_ffi::units::surface_torus as ffi;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 500;

/// `surface.torus` runner.
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
struct TorusIn {
    origin: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    major: f64,
    minor: f64,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Args {
    dom: DomIn,
    other_dom: DomIn,
    #[serde(default)]
    tol: Option<f64>,
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
        other: TorusIn,
    },
    Copy {},
    IsectPlane {
        plane: PlaneIn,
        args: Args,
    },
    Global {
        plane: PlaneIn,
        args: Args,
    },
    TrimDomain {
        dom: [f64; 4],
    },
    /// `IsNurbSurfaceTorus` on the torus's NURB, scaled per axis and with control point
    /// changes (fixtures only: the generator stream is unchanged).
    IsNurb {
        scale: f64,
        #[serde(default = "unit_scale")]
        net_scale: [f64; 3],
        #[serde(default)]
        perturb: Vec<PerturbIn>,
    },
}

/// A control point change `(i, j) += d`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct PerturbIn {
    i: usize,
    j: usize,
    d: [f64; 3],
}

const fn unit_scale() -> [f64; 3] {
    [1.0, 1.0, 1.0]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    torus: TorusIn,
    #[serde(flatten)]
    op: OpIn,
}

// ---- C++ side -------------------------------------------------------------------------

fn ffi_torus(t: &TorusIn) -> ffi::TorusIn {
    ffi::TorusIn {
        origin: t.origin,
        x: t.x,
        y: t.y,
        major: t.major,
        minor: t.minor,
        swap: t.swap,
        inside_out: t.inside_out,
        dom: t.dom,
        circle: t.circle.as_ref().map(|c| ffi::CircleIn {
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
            other: Box::new(ffi_torus(other)),
        },
        OpIn::Copy {} => ffi::Op::Copy,
        OpIn::IsectPlane { plane, args } => ffi::Op::IsectPlane {
            plane: ffi_plane(plane),
            args: ffi_args(args),
        },
        OpIn::Global { plane, args } => ffi::Op::Global {
            plane: ffi_plane(plane),
            args: ffi_args(args),
        },
        OpIn::TrimDomain { dom } => ffi::Op::TrimDomain { dom: *dom },
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
    };
    let flat = ffi::torus_op(&ffi_torus(&i.torus), &op).map_err(|e| harness(e.to_string()))?;
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
    if !matches!(i.op, OpIn::Build {}) {
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
        _ => -1.0,
    }
}

/// `SC_ERR_ARG` (`harness/cpp/shim/sc_shim.h`): the shim rejected the arguments.
const SC_ERR_ARG: f64 = -1002.0;

/// Builds the torus; `Ok(Err(e))` when the construction fails.
fn rust_torus(k: &Kernel, t: &TorusIn) -> Result<solidcore::Result<Torus>, HarnessError> {
    rust_torus_made(k, t, true)
}

/// [`rust_torus`]; `make` false is the constructor alone (no `MakeNurb`).
fn rust_torus_made(k: &Kernel, t: &TorusIn, make: bool) -> Result<solidcore::Result<Torus>, HarnessError> {
    let circle = match &t.circle {
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
    let frame = Axis2Placement::new(v3(t.origin), v3(t.x), v3(t.y));
    let dom = t.dom.map(|[a, b, c, d]| Extent2d::from_bounds(a, b, c, d));
    let opts = TorusOptions {
        swap_uv: t.swap,
        inside_out: t.inside_out,
        circle_nurb: circle,
    };
    if !make {
        return Ok(Torus::new(
            k,
            v3(t.origin),
            v3(t.x),
            v3(t.y),
            dom.unwrap_or(Extent2d::from_bounds(0.0, 0.0, 360.0, 360.0)),
            t.major,
            t.minor,
            opts,
        ));
    }
    Ok(Torus::create_canonical(
        k,
        &frame,
        t.major,
        t.minor,
        opts,
        dom.as_ref(),
    ))
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

fn put_state(o: &mut Vec<f64>, t: &Torus) -> solidcore::Result<()> {
    let s = t.base();
    o.extend([
        flag(t.is_kind_of(TORUS_TYPE)?),
        flag(t.is_kind_of(SURF_OF_REVOLUTION_TYPE)?),
        flag(t.is_kind_of(B_SPLINE_SURFACE_TYPE)?),
        flag(t.is_kind_of(TYPE)?),
        t.type_code() as f64,
        flag(s.swap_uv()),
        flag(s.planar_generator()),
        flag(t.inside_out()),
        t.major_radius(),
        t.minor_radius(),
    ]);
    let pos = s.position();
    push_v(o, pos.origin());
    push_v(o, pos.x_axis());
    push_v(o, pos.y_axis());
    let d = s.step_uv_domain();
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

/// Runs the op on the Rust torus, appending its status and outputs. `Err` from the
/// harness only; `Ok(Err(e))` when the op failed before writing its status.
fn rust_op(
    k: &Kernel,
    t: &mut Torus,
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
        OpIn::Eq { other } => match rust_torus(k, other)? {
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
        OpIn::IsectPlane { plane, args } => {
            let p = rust_plane(plane);
            let pnat = p.natural_uv_domain();
            match t.intersect_with_plane(k, &p, &dom_of(args.other_dom, pnat), args.tol) {
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
        OpIn::Global { plane, args } => {
            let p = rust_plane(plane);
            let nat = t.base().base().natural_uv_domain();
            let pnat = p.natural_uv_domain();
            let other = Surface::from(p);
            match Surface::from(t.copy()).global_surface_intersect(
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
        OpIn::IsNurb {
            scale,
            net_scale,
            perturb,
        } => match is_nurb(k, t, *scale, *net_scale, perturb) {
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
        },
    };
    Ok(r)
}

/// The shim's `OP_IS_NURB`: a copy of the torus's NURB, scaled and changed, as a plain
/// B-spline surface owning it, given to `IsNurbSurfaceTorus`.
fn is_nurb(
    k: &Kernel,
    t: &Torus,
    scale: f64,
    net_scale: [f64; 3],
    perturb: &[PerturbIn],
) -> solidcore::Result<Option<Torus>> {
    let mut nurb = t
        .base()
        .base()
        .gw_nurb()
        .cloned()
        .ok_or_else(|| SolidError::null_pointer("difftest::surface_torus::is_nurb"))?;
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
    let test = BSplineSurface::from_nurb(nurb, false);
    Torus::is_nurb_surface_torus(k, &test, scale)
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let mut o = Vec::new();
    match rust_torus(&k, &i.torus)? {
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

fn gen_torus(rng: &mut Rng) -> TorusIn {
    let origin = if rng.chance(0.3) {
        [0.0, 0.0, 0.0]
    } else {
        rng.point3(-5.0, 5.0)
    };
    let (x, y) = frame(rng);
    let major = match rng.below(10) {
        0 => *rng.choose(&[0.0, -1.0, 1e-13]).unwrap_or(&0.0),
        _ => rng.uniform(0.5, 6.0),
    };
    let minor = match rng.below(12) {
        0 => *rng.choose(&[0.0, -0.5]).unwrap_or(&0.0),
        // spindle torus (minor >= major)
        1 => major.abs() * rng.uniform(1.0, 1.5),
        _ => major.abs() * rng.uniform(0.05, 0.9),
    };
    let angular = |rng: &mut Rng| -> [f64; 2] {
        match rng.below(4) {
            0 => [0.0, 360.0],
            1 => {
                let a = rng.uniform(-360.0, 0.0);
                [a, a + rng.uniform(10.0, 360.0)]
            }
            _ => {
                let a = rng.uniform(0.0, 200.0);
                [a, (a + rng.uniform(10.0, 160.0)).min(360.0)]
            }
        }
    };
    let dom = rng.chance(0.5).then(|| {
        let [u0, u1] = angular(rng);
        let [v0, v1] = angular(rng);
        [u0, v0, u1, v1]
    });
    // an explicit generator: the minor circle the constructor would make, over the V
    // interval (or a little off it)
    let circle = (rng.chance(0.15) && minor > 1e-6).then(|| {
        let [_, v0, _, v1] = dom.unwrap_or([0.0, 0.0, 360.0, 360.0]);
        let d = if rng.chance(0.7) {
            [v0, v1]
        } else {
            [v0, (v1 - rng.uniform(1.0, 5.0)).max(v0 + 1.0)]
        };
        CircleIn {
            c: add(origin, x, major),
            x,
            y: cross3(x, y),
            d,
            r: minor,
        }
    });
    TorusIn {
        origin,
        x,
        y,
        major,
        minor,
        swap: rng.chance(0.4),
        inside_out: rng.chance(0.4),
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
        tol: rng
            .choose(&[None, None, Some(1e-6), Some(1e-3), Some(0.05)])
            .copied()
            .flatten(),
    }
}

/// A plane for the torus's plane intersection: through the axis (the generator
/// rotated into it), normal to the axis at a height through, tangent to or missing the
/// tube (the sweep iso circles), parallel to the axis off it or oblique (the general
/// intersector), bounded or infinite. `exact` keeps to the planes the special case
/// answers itself: through the axis, or normal to it cutting the tube transversally or
/// missing it.
fn gen_plane(rng: &mut Rng, t: &TorusIn, exact: bool) -> PlaneIn {
    let axis = normalize(cross3(t.x, t.y));
    let r = t.minor.abs();
    let big = t.major.abs() + r;
    if exact {
        let (origin, normal, px) = if rng.chance(0.5) {
            let a = rng.uniform(0.0, 360.0).to_radians();
            let dir = add(add([0.0; 3], t.x, a.cos()), t.y, a.sin());
            (t.origin, normalize(cross3(axis, dir)), axis)
        } else {
            let h = if rng.chance(0.2) {
                1.2 * r
            } else {
                rng.uniform(-0.9, 0.9) * r
            };
            (add(t.origin, axis, h), axis, t.x)
        };
        return finish_plane(rng, origin, normal, px, big);
    }
    let (origin, normal, px) = match rng.below(8) {
        // through the axis
        0..=2 => {
            let ang = *rng.choose(&[0.0, 30.0, 90.0, 135.0, 180.0, 270.0]).unwrap_or(&0.0)
                + if rng.chance(0.5) {
                    rng.uniform(0.0, 360.0)
                } else {
                    0.0
                };
            let a = ang.to_radians();
            let dir = add([0.0; 3], t.x, a.cos());
            let dir = add(dir, t.y, a.sin());
            let n = normalize(cross3(axis, dir));
            let shift = if rng.chance(0.2) {
                rng.uniform(-0.5, 0.5) * r
            } else {
                0.0
            };
            (add(add(t.origin, axis, rng.uniform(-1.0, 1.0) * r), n, shift), n, axis)
        }
        // normal to the axis
        3..=5 => {
            let h = match rng.below(7) {
                0 => r,
                1 => -r,
                2 => 0.0,
                3 => 1.2 * r,
                4 => r * (1.0 - 1e-9),
                _ => rng.uniform(-1.0, 1.0) * r,
            };
            let n = if rng.chance(0.5) {
                axis
            } else {
                [-axis[0], -axis[1], -axis[2]]
            };
            (add(t.origin, axis, h), n, t.x)
        }
        // oblique
        _ => {
            let n = normalize(add(axis, unit_vec(rng), rng.uniform(0.2, 1.0)));
            (add(t.origin, n, rng.uniform(-0.5, 0.5) * r), n, cross3(n, unit_vec(rng)))
        }
    };
    finish_plane(rng, origin, normal, px, big)
}

/// The infinite plane through `origin` normal to `normal`, or a bounded one with X axis
/// along `px` and half sizes up to `1.5 big`.
fn finish_plane(rng: &mut Rng, origin: [f64; 3], normal: [f64; 3], px: [f64; 3], big: f64) -> PlaneIn {
    if rng.chance(0.5) || len3(px) < 1e-3 {
        PlaneIn::Infinite { origin, normal }
    } else {
        let x = normalize(px);
        let y = normalize(cross3(normal, x));
        let a = rng.uniform(0.3, 1.5) * big;
        let b = rng.uniform(0.3, 1.5) * big;
        PlaneIn::Bounded {
            origin,
            x,
            y,
            scale: [1.0, 1.0],
            dom: [-a, -b, a, b],
        }
    }
}

fn len3(a: [f64; 3]) -> f64 {
    let [x, y, z] = a;
    (x * x + y * y + z * z).sqrt()
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

fn gen_case(rng: &mut Rng) -> Option<Inputs> {
    let k = Kernel::default();
    let torus = gen_torus(rng);
    // the Rust torus sizes the parameters (both sides get the same numbers)
    let built = rust_torus(&k, &torus).ok()?.ok();
    let nd = built
        .as_ref()
        .map_or(Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0), |b| {
            b.base().base().natural_uv_domain()
        });
    let sd = built
        .as_ref()
        .map_or(Extent2d::from_bounds(0.0, 0.0, 360.0, 360.0), |b| {
            b.base().step_uv_domain()
        });
    let pick = |rng: &mut Rng, lo: f64, hi: f64| -> f64 {
        match rng.below(10) {
            0 => lo,
            1 => hi,
            2 => lo - rng.uniform(0.0, 0.5) * (hi - lo),
            _ => lo + rng.unit() * (hi - lo),
        }
    };
    let op = match rng.below(13) {
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
        6 | 7 => {
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
        8 => OpIn::Offset {
            dist: rng.uniform(-1.5, 1.5) * torus.minor.abs(),
            tol: *rng.choose(&[0.0, 1e-6, 1e-3, 0.1])?,
            rebuild: rng.chance(0.4),
        },
        // (a sub-draw keeps the other ops' generated cases unchanged)
        9 => {
            if rng.below(3) == 0 {
                OpIn::Rebuild {}
            } else {
                // (`Global` takes only the planes its special case answers: the general
                // intersection behind it is not ported for a torus)
                let global = rng.chance(0.25);
                let plane = gen_plane(rng, &torus, global);
                let pnat = rust_plane(&plane).natural_uv_domain();
                let args = gen_args(rng, nd, pnat);
                if global {
                    OpIn::Global { plane, args }
                } else {
                    OpIn::IsectPlane { plane, args }
                }
            }
        }
        10 => {
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
        11 => {
            let mut other = torus.clone();
            match rng.below(5) {
                0 => {}
                1 => other.swap = !other.swap,
                2 => other.minor += 1e-3,
                3 => other.inside_out = !other.inside_out,
                _ => other.origin = add(other.origin, [1e-3, 0.0, 0.0], 1.0),
            }
            OpIn::Eq { other }
        }
        // (a sub-draw keeps the other ops' generated cases unchanged)
        _ => {
            if rng.below(4) == 0 {
                OpIn::Copy {}
            } else {
                OpIn::TrimDomain {
                    dom: gen_trim_domain(rng, nd)?,
                }
            }
        }
    };
    Some(Inputs { torus, op })
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.torus"
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

/// The native side of a torus `common_const_make` case.
pub(super) fn const_make_cpp(value: Value) -> Result<Output, HarnessError> {
    let t: TorusIn = serde_json::from_value(value).map_err(|e| harness(e.to_string()))?;
    let flat = ffi::const_make(&ffi_torus(&t)).map_err(|e| harness(e.to_string()))?;
    super::surface_cone_p1::const_make_nurb_cpp(&flat)
}

/// The Rust side: the torus as its constructor leaves it (no NURB), then the shared
/// const-query, `Copy`, `==` and `Transform` sequence.
pub(super) fn const_make_rust(value: Value) -> Result<Output, HarnessError> {
    let t: TorusIn = serde_json::from_value(value).map_err(|e| harness(e.to_string()))?;
    let k = Kernel::default();
    let built = rust_torus_made(&k, &t, false)?.map(Surface::from);
    super::surface_cone_p1::const_make_nurb_rust(&k, built)
}

/// `common_const_make` cases for the torus: swap and inside-out flags, partial domains,
/// a minor radius equal to and above the major radius (the inside-out, self-intersecting
/// shapes) and an explicit generator.
pub(super) fn const_make_cases() -> Vec<Case> {
    type Shape = (&'static str, f64, f64, Option<[f64; 4]>, Option<CircleIn>);
    let mut cases = Vec::new();
    let shapes: [Shape; 8] = [
        ("full", 3.0, 1.0, None, None),
        ("part", 3.0, 1.0, Some([30.0, 20.0, 250.0, 300.0]), None),
        ("minor_eq_major", 2.0, 2.0, None, None),
        ("minor_gt_major", 1.0, 2.5, None, None),
        ("minor_gt_major_part", 1.0, 2.5, Some([0.0, 100.0, 270.0, 260.0]), None),
        ("tiny_minor", 3.0, 1e-3, None, None),
        ("wide_minor_part", 0.5, 4.0, Some([0.0, 0.0, 180.0, 360.0]), None),
        ("gen", 3.0, 1.0, None, Some(CircleIn { c: [3.5, -1.0, 2.0], x: [1.0, 0.0, 0.0], y: [0.0, 0.0, 1.0], d: [0.0, 360.0], r: 1.0 })),
    ];
    for swap in [false, true] {
        for inside_out in [false, true] {
            for (name, major, minor, dom, circle) in &shapes {
                let torus = TorusIn { origin: [0.5, -1.0, 2.0], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0], major: *major, minor: *minor, swap, inside_out, dom: *dom, circle: circle.clone() };
                let Ok(torus) = serde_json::to_value(&torus) else { continue };
                cases.push(Case {
                    unit: "surface.surface_p2".into(),
                    name: format!("common_const_make_torus_{name}_{swap}_{inside_out}"),
                    source: crate::case::CaseSource::Generated { seed: 76 },
                    inputs: serde_json::json!({"common_const_make": true, "common_torus": torus}),
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
            OpIn::Iso { .. } => "iso",
            OpIn::Offset { .. } => "offset",
            OpIn::Rebuild {} => "rebuild",
            OpIn::Transform { .. } => "transform",
            OpIn::Eq { .. } => "eq",
            OpIn::Copy {} => "copy",
            OpIn::IsectPlane { .. } => "isect_plane",
            OpIn::Global { .. } => "global",
            OpIn::TrimDomain { .. } => "trim_domain",
            OpIn::IsNurb { .. } => "is_nurb",
        }
    }

    /// The generators reach a successful construction and a successful run of every op
    /// on the Rust side.
    #[test]
    fn generated_cases_cover_every_op() {
        let mut tally: BTreeMap<(String, String), usize> = BTreeMap::new();
        for seed in 0..500u64 {
            let mut rng = Rng::new(0x7075_0000 + seed);
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
            "iso",
            "offset",
            "rebuild",
            "transform",
            "eq",
            "copy",
            "isect_plane",
            "global",
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
