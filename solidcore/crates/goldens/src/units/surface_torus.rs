//! Runner for unit `surface.torus`: `SmTorus` (construction through `CreateCanonical`,
//! `operator==`, `Copy`, `CreateOffsetSurface`, the STEP/NURB conversions, STEP
//! evaluation, iso curves, `RebuildSTEPFromNURBParameters`, `Transform`, and the
//! inherited `SmSurfOfRevolution::TrimWithDomain`, `IntersectWithPlane` and
//! `GlobalSurfaceIntersect` with a plane; `IsNurbSurfaceTorus` on its NURB, fixtures
//! only).
//!
//! Ported from upstream `source/SMLib/src/SmTorus.cpp`; checked against its golden.
//! Rust: `solidcore::surface::torus`.
//!
//! A case is `torus` (placement, radii, flags, optional domain and generator circle)
//! plus `op` and its parameters. The Rust side produces a flat record, split into `build` (the construction status), `status` (the
//! op status) and `out` (the rest). Statuses are `0` (`SM_SUCCESS`) or the
//! `SmStatus`; `-1` for an error upstream cannot produce.
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
use solidcore::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::surface::surface::p2::{
    B_SPLINE_SURFACE_TYPE, SURF_OF_REVOLUTION_TYPE, TORUS_TYPE, TYPE,
};
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::p2::{Surface, SurfaceIntersection};
use solidcore::surface::torus::{Torus, TorusOptions};
use solidcore::SolidError;

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
        e if solidcore::error::nurbs::upstream_status(e).is_some() => {
            f64::from(solidcore::error::nurbs::upstream_status(e).unwrap_or(-1))
        }
        _ => -1.0,
    }
}

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
            t.create_iso_parametric_curve(k, param_of(*param), *iso, dom.as_ref(), false)
                .map(|c| {
                    o.extend([0.0, c.max_gap_3d]);
                    put_curve(o, Some(&c.curve));
                })
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

/// The `is_nurb` op: a copy of the torus's NURB, scaled and changed, as a plain B-spline
/// surface owning it, given to `IsNurbSurfaceTorus`.
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

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}

// ---- lazy NURB (`surface.surface_p2` `common_const_make` cases) ------------------------

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

    /// The 92 cases whose golden rows moved when the torus flags `torus_major_radius_from_origin`
    /// (WR-501), `torus_signed_gen_curve_interval` (WR-408) and
    /// `torus_iso_circle_axis_from_midpoint` (WR-504) became the only path.
    const WR408_501_504_MOVED: [&str; 92] = [
        "edge_swap_inside_offset", "edge_swap_inside_rebuild",
        "gen0047", "gen0355", "gen0358", "gen0393", "gen0470", "gen0482", "gen0489",
        "horn_half_minor_iso_from_axis_0.0", "horn_half_minor_iso_from_axis_0.5",
        "issue1433_torus_far_below_s0_i0", "issue1433_torus_far_below_s0_i1",
        "issue1433_torus_far_below_s1_i0", "issue1433_torus_far_below_s1_i1",
        "issue1433_torus_far_full_s0_i0", "issue1433_torus_far_full_s0_i1",
        "issue1433_torus_far_full_s1_i0", "issue1433_torus_far_full_s1_i1",
        "issue1433_torus_far_inner_s0_i0", "issue1433_torus_far_inner_s0_i1",
        "issue1433_torus_far_inner_s1_i0", "issue1433_torus_far_inner_s1_i1",
        "issue1433_torus_far_offseam_s0_i0", "issue1433_torus_far_offseam_s0_i1",
        "issue1433_torus_far_offseam_s1_i0", "issue1433_torus_far_offseam_s1_i1",
        "issue1433_torus_far_past180_s0_i0", "issue1433_torus_far_past180_s0_i1",
        "issue1433_torus_far_past180_s1_i0", "issue1433_torus_far_past180_s1_i1",
        "issue1433_torus_far_quarter_s0_i1", "issue1433_torus_far_quarter_s1_i1",
        "issue1433_torus_net_1_1_-1", "issue1433_torus_perturb_corner_1e-12",
        "issue1433_torus_scaled_0.0001_swap", "issue1433_torus_scaled_0.01_swap",
        "issue1433_torus_scaled_100000_swap", "issue1433_torus_scaled_100_swap",
        "issue1433_torus_thin_tube",
        "issue1433_torus_tilted_below_s0_i0", "issue1433_torus_tilted_below_s0_i1",
        "issue1433_torus_tilted_below_s1_i0", "issue1433_torus_tilted_below_s1_i1",
        "issue1433_torus_tilted_inner_s0_i0", "issue1433_torus_tilted_inner_s0_i1",
        "issue1433_torus_tilted_inner_s1_i0", "issue1433_torus_tilted_inner_s1_i1",
        "issue1433_torus_tilted_offseam_s0_i0", "issue1433_torus_tilted_offseam_s0_i1",
        "issue1433_torus_tilted_offseam_s1_i0", "issue1433_torus_tilted_offseam_s1_i1",
        "issue1433_torus_tilted_past180_s0_i0", "issue1433_torus_tilted_past180_s0_i1",
        "issue1433_torus_tilted_past180_s1_i0", "issue1433_torus_tilted_past180_s1_i1",
        "issue1433_torus_tilted_quarter_s0_i1", "issue1433_torus_tilted_quarter_s1_i1",
        "issue1433_torus_tolscale_1000",
        "issue1433_torus_z_below_s0_i0", "issue1433_torus_z_below_s0_i1",
        "issue1433_torus_z_below_s1_i0", "issue1433_torus_z_below_s1_i1",
        "issue1433_torus_z_inner_s0_i0", "issue1433_torus_z_inner_s0_i1",
        "issue1433_torus_z_inner_s1_i0", "issue1433_torus_z_inner_s1_i1",
        "issue1433_torus_z_offseam_s0_i0", "issue1433_torus_z_offseam_s0_i1",
        "issue1433_torus_z_offseam_s1_i0", "issue1433_torus_z_offseam_s1_i1",
        "issue1433_torus_z_past180_s0_i0", "issue1433_torus_z_past180_s0_i1",
        "issue1433_torus_z_past180_s1_i0", "issue1433_torus_z_past180_s1_i1",
        "issue1433_torus_z_quarter_s0_i1", "issue1433_torus_z_quarter_s1_i1",
        "issue1490_r20_4_s1_i0_sw0_is_nurb", "issue1490_r20_4_s1_i0_sw1_is_nurb",
        "issue1490_r20_4_s1_i1_sw0_is_nurb", "issue1490_r20_4_s1_i1_sw1_is_nurb",
        "issue1490_r20_4_s1e3_i0_sw0_is_nurb", "issue1490_r20_4_s1e3_i0_sw1_is_nurb",
        "issue1490_r20_4_s1e3_i1_sw0_is_nurb", "issue1490_r20_4_s1e3_i1_sw1_is_nurb",
        "issue1490_r20_4_s1em3_i0_sw0_is_nurb", "issue1490_r20_4_s1em3_i0_sw1_is_nurb",
        "issue1490_r20_4_s1em3_i1_sw0_is_nurb", "issue1490_r20_4_s1em3_i1_sw1_is_nurb",
        "v_neg_io_swap_rebuild", "v_neg_plain_rebuild", "v_zero_io_swap_rebuild",
    ];

    /// The STEP domain `[u0, v0, u1, v1]` of a constructed torus (`None`: the whole turn
    /// both ways).
    fn input_lengths(t: &TorusIn) -> (f64, f64) {
        t.dom.map_or((360.0, 360.0), |[u0, v0, u1, v1]| (u1 - u0, v1 - v0))
    }

    /// The largest residual seen per check, as a fraction of its allowance.
    #[derive(Default)]
    struct Residuals(BTreeMap<&'static str, f64>);

    impl Residuals {
        /// Asserts `residual <= allowance` (NaN fails) and records the fraction used.
        fn check(&mut self, key: &'static str, name: &str, residual: f64, allowance: f64) {
            assert!(residual <= allowance, "{name}: {key}: residual {residual:e} over {allowance:e}");
            let used = residual / allowance;
            let e = self.0.entry(key).or_insert(0.0);
            if used > *e {
                *e = used;
            }
        }
    }

    type NurbSurf = solidcore::nurbs::srf_basic::Surface;
    use solidcore::surface::implicit::{lies_on, Implicit};

    /// Control point `(s, g)` of `net` (Euclidean, weight), `s` along the sweep and `g`
    /// along the tube: the NURB's u is the sweep unless `swap`.
    fn net_point(net: &NurbSurf, swap: bool, s: usize, g: usize) -> (Vec3, f64) {
        let c = if swap { net.cpt(g, s) } else { net.cpt(s, g) }.expect("control point");
        (c.to_euclid(), c.w)
    }

    /// `knots` are degree-2 Bezier knots for `ncp` control points: three end copies and
    /// every interior knot twice, so each `(P2k, P2k+1, P2k+2)` is its own rational Bezier
    /// arc joined C0 at `P2k`.
    fn bezier_knots(knots: &[f64], ncp: usize) -> bool {
        let n = knots.len();
        ncp % 2 == 1
            && n == ncp + 3
            && knots[0] == knots[1]
            && knots[1] == knots[2]
            && knots[n - 1] == knots[n - 2]
            && knots[n - 2] == knots[n - 3]
            && (3..n - 3).step_by(2).all(|t| knots[t] == knots[t + 1])
    }

    /// Asserts that the rational quadratic Bezier arcs `pts` (Euclidean, weight) are
    /// exactly arcs of the circle of centre `c`, unit plane normal `nrm` and radius `rho`:
    /// each end on the circle, every point in its plane, the middle point at
    /// `rho / cos(theta / 2)` on the bisector of the ends, and `w1^2 = w0 w2 cos^2(theta / 2)`
    /// (the conditions for a rational quadratic Bezier to be that circular arc), every arc
    /// turning the same way about `nrm` (no arc runs back). Returns the arcs' total angle
    /// in degrees.
    #[allow(clippy::too_many_arguments)]
    fn assert_arcs(
        res: &mut Residuals,
        name: &str,
        pts: &[(Vec3, f64)],
        c: Vec3,
        nrm: Vec3,
        rho: f64,
        tol: f64,
    ) -> f64 {
        assert!(rho > 0.0, "{name}: arc radius {rho}");
        let mut total = 0.0;
        let mut sense = 0.0;
        for seg in pts.windows(3).step_by(2) {
            let ((p0, w0), (p1, w1), (p2, w2)) = (seg[0], seg[1], seg[2]);
            let (a, m, b) = (p0 - c, p1 - c, p2 - c);
            res.check("net: arc end on its circle", name, (a.length() - rho).abs(), tol);
            res.check("net: arc end on its circle", name, (b.length() - rho).abs(), tol);
            for v in [a, m, b] {
                res.check("net: control point in the arc's plane", name, v.dot(nrm).abs(), tol);
            }
            let signed = a.cross(b).dot(nrm).atan2(a.dot(b));
            if sense == 0.0 {
                sense = signed.signum();
            }
            assert!(signed * sense > 0.0, "{name}: an arc turns back ({signed} rad)");
            let theta = signed.abs();
            let half = (theta / 2.0).cos();
            assert!(half > 0.0, "{name}: arc of {theta} rad");
            res.check("net: middle point at rho / cos(theta / 2)", name, (m.length() - rho / half).abs(), tol / half);
            let bis = a + b;
            res.check("net: middle point on the bisector", name, m.cross(bis / bis.length()).length(), tol / half);
            res.check("net: arc weights w1^2 = w0 w2 cos^2", name, (w1 * w1 - w0 * w2 * half * half).abs() / (w0 * w2), 1e-12);
            total += theta.to_degrees();
        }
        total
    }

    /// Asserts that `form` lies on the torus of radii `major` and `minor` about the axis
    /// through `origin` along `axis`, and spans `lengths` of it. That it lies on the torus is
    /// certified by the kernel's [`lies_on`](solidcore::surface::implicit::lies_on): every
    /// Bernstein coefficient of the torus's implicit equation on every Bezier patch is within
    /// its `gamma_K` rounding, nothing sampled. The extent is pinned by the net (NURB u the
    /// sweep unless `swap`): degree-2 Bezier knots both ways; the tube column at the sweep
    /// start is arcs of the minor circle (centre `R` out from the axis in its meridian
    /// plane, radius `r`) turning one way over `lengths.1`; every sweep row is arcs about
    /// the axis of its column-0 point turning one way over `lengths.0`, turned as the first
    /// row, with tensor-product weights. Only column 0 is checked directly; the other
    /// columns follow from the rows turned alike and the tensor weights. These net checks
    /// are against chosen allowances.
    #[allow(clippy::too_many_arguments)]
    fn assert_net_is_torus(
        res: &mut Residuals,
        name: &str,
        form: &BSplineSurface,
        swap: bool,
        major: f64,
        minor: f64,
        lengths: (f64, f64),
        origin: Vec3,
        axis: Vec3,
        tol: f64,
        ang: f64,
    ) {
        let a = axis / axis.length();
        let torus = Implicit::Torus { centre: origin, axis: a, major, minor };
        let on = lies_on(form, &torus);
        assert!(matches!(on, Ok(true)), "{name}: not certified on the torus: {on:?}");
        let net = form.gw_nurb().expect("nurb");
        let ix = net.indices();
        let (nu, nv) = (ix.n + 1, ix.m + 1);
        assert!(bezier_knots(net.u_knots(), nu) && bezier_knots(net.v_knots(), nv), "{name}: knots");
        let (ns, ng) = if swap { (nv, nu) } else { (nu, nv) };
        let p = |s: usize, g: usize| net_point(net, swap, s, g);
        let radial = |x: Vec3| {
            let d = x - origin;
            d - d.dot(a) * a
        };
        // the generator: the tube column at the sweep start
        // its meridian line, from its control point farthest from the axis; a tube that
        // crosses the axis has points on both sides, so the side of the tube's centre is
        // the one that puts the first point at distance r (every point is then checked)
        let r0 = (0..ng)
            .map(|g| radial(p(0, g).0))
            .fold(Vec3::new(0.0, 0.0, 0.0), |m, d| if d.length() > m.length() { d } else { m });
        let line = r0 / r0.length();
        let off_circle = |dir: Vec3| ((p(0, 0).0 - (origin + major * dir)).length() - minor).abs();
        let dir0 = if off_circle(-line) < off_circle(line) { -line } else { line };
        let column: Vec<_> = (0..ng).map(|g| p(0, g)).collect();
        let tube = assert_arcs(res, name, &column, origin + major * dir0, a.cross(dir0), minor, tol);
        res.check("net: tube angle = STEP v length", name, (tube - lengths.1).abs(), ang);
        // every sweep row: the arc about the axis of its column point
        let w00 = p(0, 0).1;
        for g in 0..ng {
            let (q, wq) = p(0, g);
            let rho = radial(q).length();
            let row: Vec<_> = (0..ns).map(|s| p(s, g)).collect();
            let sweep = assert_arcs(res, name, &row, origin + (q - origin).dot(a) * a, a, rho, tol);
            res.check("net: sweep angle = STEP u length", name, (sweep - lengths.0).abs(), ang);
            for s in 0..ns {
                let (ps, w) = p(s, g);
                let ws0 = p(s, 0).1;
                res.check("net: tensor-product weights", name, (w * w00 - ws0 * wq).abs() / (w * w00), 1e-12);
                if s % 2 == 0 {
                    // the row's start lies on the meridian, on the side of `dir0` or across
                    // the axis; each arc end is turned as the generator's is
                    let side = if radial(q).dot(dir0) < 0.0 { -1.0 } else { 1.0 };
                    let d = radial(ps);
                    let rot = radial(p(s, 0).0);
                    let side0 = if radial(p(0, 0).0).dot(dir0) < 0.0 { -1.0 } else { 1.0 };
                    res.check("net: rows turned alike", name, (side * d / d.length() - side0 * rot / rot.length()).length(), 1e-9);
                }
            }
        }
    }

    /// The analytic data of `t` against the torus it must be. `exact` (NURB u the sweep
    /// unless `swap`) is first certified on that torus over `lengths` ([`assert_net_is_torus`]);
    /// `t` must then have radii `major` and `minor`, STEP interval lengths `lengths`
    /// (degrees), the axis through `origin` along `axis` (either sense), and pass through
    /// `test` at the NURB domain's four corners and its centre, each at a STEP parameter
    /// inside its domain; the corners fix the patch's placement on the torus (its u0 and
    /// v0). `slack` is a certified bound on how far `test` lies off `exact` (a perturbed
    /// control point; 0 otherwise). It bounds `test` against `exact`, not against `t`, so a
    /// perturbed row's checks of `t` (point and parameter) are MEASURED against allowances
    /// widened by it.
    #[allow(clippy::too_many_arguments)]
    fn assert_is_torus(
        res: &mut Residuals,
        name: &str,
        k: &Kernel,
        t: &Torus,
        test: &BSplineSurface,
        exact: &BSplineSurface,
        swap: bool,
        major: f64,
        minor: f64,
        lengths: (f64, f64),
        origin: Vec3,
        axis: Vec3,
        slack: f64,
    ) {
        // chosen allowances (not derived): 1e-9 of the model size for points, 1e-9 relative
        // for radii, 1e-9 degrees for angles; the test prints the largest fraction of each
        // that the 92 rows use, and every move it pins is far larger (the PR lists each)
        let size = origin.length() + major + minor;
        let tol = 1e-9 * size;
        assert_net_is_torus(res, name, exact, swap, major, minor, lengths, origin, axis, tol, 1e-9);
        // a point moved by `slack` turns the tube or the sweep by at most slack over the
        // smaller circle radius; the parameter checks of a perturbed row use these widened
        // allowances as measured, not derived, bounds
        let ang = if slack > 0.0 {
            let den = minor.min((major - minor).abs());
            assert!(den > 0.0, "{name}: no angle bound for R = r");
            1e-9 + (slack / den).to_degrees()
        } else {
            1e-9
        };
        res.check("torus: major radius", name, (t.major_radius() - major).abs(), 1e-9 * major + slack);
        res.check("torus: minor radius", name, (t.minor_radius() - minor).abs(), 1e-9 * minor + slack);
        let d = t.base().step_uv_domain();
        let (du, dv) = (d.u_interval(), d.v_interval());
        res.check("torus: STEP u length", name, (du.length() - lengths.0).abs(), ang);
        res.check("torus: STEP v length", name, (dv.length() - lengths.1).abs(), ang);
        let pos = t.base().position();
        res.check("torus: origin", name, (pos.origin() - origin).length(), tol + slack);
        res.check("torus: axis", name, pos.z_axis().cross(axis / axis.length()).length(), 1e-9);
        let nd = test.natural_uv_domain();
        for (a, b) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0), (0.5, 0.5)] {
            let uv = nd.evaluate(a, b);
            let p = test.evaluate_point(uv).expect("nurb point");
            let step = t.convert_uv_from_nurbs_to_step(k, uv).expect("to step");
            let outside = |x: f64, lo: f64, hi: f64| (lo - x).max(x - hi).max(0.0);
            res.check("torus: STEP parameter in the domain", name, outside(step.x, du.min(), du.max()), ang);
            res.check("torus: STEP parameter in the domain", name, outside(step.y, dv.min(), dv.max()), ang);
            let q = t.evaluate_step_point(step).expect("step point");
            let key = if slack > 0.0 { "torus: through the NURB (perturbed rows, MEASURED)" } else { "torus: through the NURB" };
            res.check(key, name, (p - q).length(), tol + slack);
        }
    }

    /// How far the offset of `t` by `dist` (`CreateOffsetSurface`) is from the source moved
    /// along its normal: the distance (closed form) from the offset torus of the source
    /// NURB point plus `dist` times its unit normal `Su x Sv / |Su x Sv|`, at each point of
    /// a 5 x 5 grid inside the NURB domain (off the axis of a horn torus), and the rounding
    /// allowance for it. A geometric check of the offset's sense, independent of the
    /// formula that sets its minor radius. Points where the tube has crossed the axis
    /// (`R + r cos v <= 0`, a spindle torus's folded part, where the sweep runs backwards
    /// and so does the normal) are left out: there the offset along the normal is not the
    /// minor-radius change. `None` when no point is left.
    fn offset_off_normal(name: &str, k: &Kernel, t: &Torus, dist: f64, origin: Vec3, axis: Vec3) -> Option<(Vec<f64>, f64)> {
        let off = t
            .create_offset_surface(k, dist, 1e-6)
            .expect("offset")
            .unwrap_or_else(|| panic!("{name}: no offset"));
        let a_hat = axis / axis.length();
        let (big, small) = (off.major_radius(), off.minor_radius());
        // the distance from x to the offset torus, the revolution of its minor circle: in the
        // meridian plane through x, the nearer of the two circles (the far one matters for a
        // spindle torus, whose tube crosses the axis)
        let off_torus = |x: Vec3| {
            let v = x - origin;
            let h = v.dot(a_hat);
            let rho = (v - h * a_hat).length();
            let near = ((rho - big).powi(2) + h * h).sqrt() - small;
            let far = ((rho + big).powi(2) + h * h).sqrt() - small;
            near.abs().min(far.abs())
        };
        let src = t.base().base();
        let nd = src.natural_uv_domain();
        let tol = 1e-9 * (origin.length() + t.major_radius() + t.minor_radius() + dist.abs());
        let mut per_point = Vec::new();
        let grid = [0.1, 0.3, 0.5, 0.7, 0.9];
        for (a, b) in grid.iter().flat_map(|a| grid.iter().map(move |b| (*a, *b))) {
            let uv = nd.evaluate(a, b);
            let v = t.convert_uv_from_nurbs_to_step(k, uv).expect("to step").y;
            if t.major_radius() + t.minor_radius() * v.to_radians().cos() <= 1e-6 * t.major_radius() {
                continue;
            }
            let d = src.evaluate_simple(uv, 1, 1, false, false, false).expect("derivatives");
            let n = d[1][0].cross(d[0][1]);
            let moved = d[0][0] + dist * (n / n.length());
            let off_by = off_torus(moved);
            assert!(off_by.is_finite(), "{name}: offset check at ({a}, {b})");
            per_point.push(off_by);
        }
        (!per_point.is_empty()).then_some((per_point, tol))
    }

    /// The rows whose torus is inside out (50 of the 92), offset to the wrong side
    /// (tangency-labs/solidcraft#17091: `CreateOffsetSurface` reverses the offset for a swap
    /// but not for inside out). The fix must remove this list.
    const INSIDE_OUT_ROWS: [&str; 50] = [
        "edge_swap_inside_offset",
        "edge_swap_inside_rebuild",
        "gen0358",
        "gen0393",
        "gen0482",
        "gen0489",
        "issue1433_torus_far_below_s0_i1",
        "issue1433_torus_far_below_s1_i1",
        "issue1433_torus_far_full_s0_i1",
        "issue1433_torus_far_full_s1_i1",
        "issue1433_torus_far_inner_s0_i1",
        "issue1433_torus_far_inner_s1_i1",
        "issue1433_torus_far_offseam_s0_i1",
        "issue1433_torus_far_offseam_s1_i1",
        "issue1433_torus_far_past180_s0_i1",
        "issue1433_torus_far_past180_s1_i1",
        "issue1433_torus_far_quarter_s0_i1",
        "issue1433_torus_far_quarter_s1_i1",
        "issue1433_torus_scaled_0.0001_swap",
        "issue1433_torus_scaled_0.01_swap",
        "issue1433_torus_scaled_100000_swap",
        "issue1433_torus_scaled_100_swap",
        "issue1433_torus_tilted_below_s0_i1",
        "issue1433_torus_tilted_below_s1_i1",
        "issue1433_torus_tilted_inner_s0_i1",
        "issue1433_torus_tilted_inner_s1_i1",
        "issue1433_torus_tilted_offseam_s0_i1",
        "issue1433_torus_tilted_offseam_s1_i1",
        "issue1433_torus_tilted_past180_s0_i1",
        "issue1433_torus_tilted_past180_s1_i1",
        "issue1433_torus_tilted_quarter_s0_i1",
        "issue1433_torus_tilted_quarter_s1_i1",
        "issue1433_torus_z_below_s0_i1",
        "issue1433_torus_z_below_s1_i1",
        "issue1433_torus_z_inner_s0_i1",
        "issue1433_torus_z_inner_s1_i1",
        "issue1433_torus_z_offseam_s0_i1",
        "issue1433_torus_z_offseam_s1_i1",
        "issue1433_torus_z_past180_s0_i1",
        "issue1433_torus_z_past180_s1_i1",
        "issue1433_torus_z_quarter_s0_i1",
        "issue1433_torus_z_quarter_s1_i1",
        "issue1490_r20_4_s1_i1_sw0_is_nurb",
        "issue1490_r20_4_s1_i1_sw1_is_nurb",
        "issue1490_r20_4_s1e3_i1_sw0_is_nurb",
        "issue1490_r20_4_s1e3_i1_sw1_is_nurb",
        "issue1490_r20_4_s1em3_i1_sw0_is_nurb",
        "issue1490_r20_4_s1em3_i1_sw1_is_nurb",
        "v_neg_io_swap_rebuild",
        "v_zero_io_swap_rebuild",
    ];

    /// Every golden row that moved with the WR-408/501/504 retirement holds the exact torus
    /// (the recogniser and the rebuild) or the exact minor circle (the horn iso curve).
    #[test]
    fn wr408_501_504_moved_rows_are_the_exact_torus() {
        let ctx = CaseContext {
            root: crate::case::repo_root(),
            seeds: MIN_GENERATED,
        };
        let cases = Runner.cases(&ctx).expect("cases");
        let k = Kernel::default();
        let mut res = Residuals::default();
        let mut seen = 0;
        let mut excluded = Vec::new();
        let mut folded = Vec::new();
        for case in &cases {
            if !WR408_501_504_MOVED.contains(&case.name.as_str()) {
                continue;
            }
            seen += 1;
            let name = case.name.as_str();
            let i: Inputs = case.parse().expect("inputs");
            let t = rust_torus(&k, &i.torus).expect("harness").expect("torus builds");
            let (major, minor) = (i.torus.major, i.torus.minor);
            let origin = v3(i.torus.origin);
            let axis = v3(i.torus.x).cross(v3(i.torus.y));
            let lengths = input_lengths(&i.torus);
            // every row's torus offset by a quarter of its minor radius lies along its normal,
            // except the inside-out tori (#17091), which must stay on the wrong side by
            // exactly 2d until the fix flips this (the torus's own flag: a case given an
            // explicit generator takes it from that circle, not from the input's flag)
            let d = 0.25 * minor;
            // every grid point is checked on its own, so a row only partly on the wrong side
            // fails both branches
            if let Some((per_point, off_tol)) = offset_off_normal(name, &k, &t, d, origin, axis) {
                if t.inside_out() {
                    excluded.push(name);
                    for off_by in per_point {
                        res.check("offset: inside-out row, every point off by 2d (#17091)", name, (off_by - 2.0 * d).abs(), off_tol);
                    }
                } else {
                    for off_by in per_point {
                        res.check("offset: source point moved along its normal is on the offset", name, off_by, off_tol);
                    }
                }
            } else {
                folded.push(name);
            }
            match &i.op {
                OpIn::IsNurb { scale, net_scale, perturb } => {
                    // the moved is_nurb rows are unscaled or mirrored nets (an isometry
                    // about the world origin), some with one control point moved by at most
                    // 1e-9
                    assert!(net_scale.iter().all(|s| s.abs() == 1.0), "{name}: net {net_scale:?}");
                    assert!(perturb.iter().all(|p| p.d.iter().all(|d| d.abs() <= 1e-9)), "{name}");
                    let m = Vec3::new(net_scale[0], net_scale[1], net_scale[2]);
                    let mo = Vec3::new(origin.x * m.x, origin.y * m.y, origin.z * m.z);
                    let ma = Vec3::new(axis.x * m.x, axis.y * m.y, axis.z * m.z);
                    let mut exact = t.base().base().gw_nurb().cloned().expect("nurb");
                    let ix = exact.indices();
                    for a in 0..=ix.n {
                        for b in 0..=ix.m {
                            let p = exact.cpt_mut(a, b).expect("cpt");
                            p.x *= m.x;
                            p.y *= m.y;
                            p.z *= m.z;
                        }
                    }
                    let mut nurb = exact.clone();
                    for pt in perturb {
                        if let Ok(p) = nurb.cpt_mut(pt.i, pt.j) {
                            p.x += pt.d[0];
                            p.y += pt.d[1];
                            p.z += pt.d[2];
                        }
                    }
                    // moving one control point by d moves the rational surface by at most
                    // |d| max(1, w_max) / w_min (its weighted basis over the weighted sum,
                    // whose basis sums to 1), whether the coordinates are homogeneous or not
                    let ws: Vec<f64> = nurb.rows().flat_map(|r| r.iter().map(|p| p.w)).collect();
                    let (w_min, w_max) = ws.iter().fold((f64::INFINITY, 1.0_f64), |(a, b), w| (a.min(*w), b.max(*w)));
                    assert!(w_min > 0.0, "{name}: weights {ws:?}");
                    let moved: f64 = perturb.iter().map(|p| v3(p.d).length()).sum();
                    let slack = moved * w_max / w_min;
                    let test = BSplineSurface::from_nurb(nurb, false);
                    let exact = BSplineSurface::from_nurb(exact, false);
                    let found = Torus::is_nurb_surface_torus(&k, &test, *scale)
                        .expect("no error")
                        .unwrap_or_else(|| panic!("{name}: not found"));
                    assert_eq!(found.inside_out(), i.torus.inside_out != (m.x * m.y * m.z < 0.0), "{name}: inside out");
                    assert_is_torus(&mut res, name, &k, &found, &test, &exact, i.torus.swap, major, minor, lengths, mo, ma, slack);
                }
                OpIn::Rebuild {} => {
                    let mut r = t.clone();
                    r.rebuild_step_from_nurb_parameters(&k).unwrap_or_else(|e| panic!("{name}: {e}"));
                    assert_eq!(r.inside_out(), i.torus.inside_out, "{name}: inside out");
                    assert_is_torus(&mut res, name, &k, &r, t.base().base(), t.base().base(), i.torus.swap, major, minor, lengths, origin, axis, 0.0);
                }
                OpIn::Offset { dist, tol, rebuild } => {
                    assert!(*rebuild, "{name}: only rebuilt offsets move");
                    let off = t
                        .create_offset_surface(&k, *dist, *tol)
                        .expect("offset")
                        .unwrap_or_else(|| panic!("{name}: no offset"));
                    // the offset torus as `CreateOffsetSurface` makes it: minor r + d, d negated
                    // when u and v are swapped (not when inside out). This row is swapped and
                    // inside out, so that is the wrong side (#17091, checked above); this
                    // records what the kernel builds, and the rebuild must give it back
                    let off_minor = minor + if i.torus.swap { -dist } else { *dist };
                    res.check("offset: minor radius", name, (off.minor_radius() - off_minor).abs(), 1e-12 * off_minor);
                    res.check("offset: major radius", name, (off.major_radius() - major).abs(), 1e-12 * major);
                    let od = off.base().step_uv_domain();
                    res.check("offset: STEP u length", name, (od.u_interval().length() - lengths.0).abs(), 1e-9);
                    res.check("offset: STEP v length", name, (od.v_interval().length() - lengths.1).abs(), 1e-9);
                    let opos = off.base().position();
                    res.check("offset: origin", name, (opos.origin() - origin).length(), 1e-9 * (origin.length() + major + off_minor));
                    res.check("offset: axis", name, opos.z_axis().cross(axis / axis.length()).length(), 1e-9);
                    // and the rebuild gives that torus back
                    let mut r = off.clone();
                    r.rebuild_step_from_nurb_parameters(&k).unwrap_or_else(|e| panic!("{name}: {e}"));
                    assert_eq!(r.inside_out(), off.inside_out(), "{name}: inside out");
                    assert_is_torus(&mut res, name, &k, &r, off.base().base(), off.base().base(), off.base().swap_uv(), major, off_minor, lengths, origin, axis, 0.0);
                }
                OpIn::Iso { param, iso, dom } => {
                    assert!(dom.is_none(), "{name}: only whole-domain iso curves move");
                    let c = t
                        .create_iso_parametric_curve(&k, param_of(*param), *iso, None, false)
                        .unwrap_or_else(|e| panic!("{name}: {e}"));
                    let CurveKind::Ellipse(e) = c.curve.kind() else {
                        panic!("{name}: not a circle");
                    };
                    let x = e.position().x_axis();
                    let ptol = 1e-9 * (origin.length() + major + minor);
                    // the minor circle: unit x axis in the torus's xy plane, centre R along
                    // it from the origin, radius r, over the torus's v interval
                    res.check("iso: unit x axis", name, (x.length() - 1.0).abs(), 1e-12);
                    res.check("iso: x axis in the torus's xy plane", name, x.dot(axis).abs(), 1e-12);
                    let centre = origin + major * x;
                    res.check("iso: centre", name, (e.position().origin() - centre).length(), 1e-12 * (origin.length() + major + minor));
                    res.check("iso: radius", name, (e.x_radius() - minor).abs(), 1e-12 * minor);
                    let ivl = e.step_interval();
                    res.check("iso: interval length", name, (ivl.length() - lengths.1).abs(), 1e-9);
                    // its analytic ends are the iso curve's NURB ends, and the NURB's middle
                    // lies on the circle
                    let nurb = e.base();
                    let ni = nurb.natural_interval();
                    let ends = [ni.min(), ni.max()].map(|s| nurb.evaluate_point(s).expect("nurb point"));
                    let step_ends = [ivl.min(), ivl.max()].map(|s| e.evaluate_step_point(s).expect("circle point"));
                    let gap = |a: [Vec3; 2], b: [Vec3; 2]| (a[0] - b[0]).length().max((a[1] - b[1]).length());
                    let ends_gap = gap(ends, step_ends).min(gap(ends, [step_ends[1], step_ends[0]]));
                    res.check("iso: NURB ends are the circle's", name, ends_gap, ptol);
                    let mid = nurb.evaluate_point(ni.evaluate(0.5)).expect("nurb point");
                    let normal = x.cross(e.position().y_axis());
                    res.check("iso: NURB middle on the circle", name, ((mid - centre).length() - minor).abs(), ptol);
                    res.check("iso: NURB middle in the circle's plane", name, (mid - centre).dot(normal).abs(), ptol);
                }
                other => panic!("{name}: unexpected op {}", op_name(other)),
            }
        }
        assert_eq!(seen, WR408_501_504_MOVED.len(), "every moved case is checked");
        excluded.sort_unstable();
        eprintln!("offset check: {} inside-out rows off by 2d at every point, {} rows wholly folded: {folded:?}", excluded.len(), folded.len());
        assert_eq!(excluded, INSIDE_OUT_ROWS.to_vec(), "the rows #17091 excludes");
        assert_eq!(folded, ["gen0355"], "the rows with every grid point on the folded part");
        for (key, used) in &res.0 {
            eprintln!("largest residual / allowance: {used:.3e}  {key}");
        }
    }

    /// The 2 cases whose golden rows moved with the spindle minor iso circle fix: u iso
    /// curves of spindle tori (r > R) whose tube interval starts on the folded part.
    const SPINDLE_ISO_MOVED: [&str; 2] = ["gen0410", "gen0456"];

    /// Every golden row the spindle minor iso circle fix moves holds the exact minor circle
    /// at the iso: the meridian circle (centre R along a unit x axis normal to the torus's
    /// axis, radius r, in the plane of x and the axis, so wholly on the torus), its NURB
    /// exactly arcs of that circle turning the torus's v length, and its ends the torus
    /// NURB's points at the ends of the iso line. The ends tell it from the opposite
    /// meridian (centre -R x), which the old rows held.
    #[test]
    fn spindle_iso_moved_rows_are_the_minor_circle_at_the_iso() {
        let ctx = CaseContext {
            root: crate::case::repo_root(),
            seeds: MIN_GENERATED,
        };
        let cases = Runner.cases(&ctx).expect("cases");
        let k = Kernel::default();
        let mut res = Residuals::default();
        let mut seen = 0;
        for case in &cases {
            if !SPINDLE_ISO_MOVED.contains(&case.name.as_str()) {
                continue;
            }
            seen += 1;
            let name = case.name.as_str();
            let i: Inputs = case.parse().expect("inputs");
            let OpIn::Iso { param, iso, dom } = &i.op else {
                panic!("{name}: not an iso row");
            };
            assert!(*param == 0 && !i.torus.swap && dom.is_none(), "{name}: a whole u iso line");
            let (major, minor) = (i.torus.major, i.torus.minor);
            assert!(minor > major, "{name}: a spindle torus");
            let origin = v3(i.torus.origin);
            let axis = v3(i.torus.x).cross(v3(i.torus.y));
            let axis = axis / axis.length();
            let lengths = input_lengths(&i.torus);
            let t = rust_torus(&k, &i.torus).expect("harness").expect("torus builds");
            let c = t
                .create_iso_parametric_curve(&k, param_of(*param), *iso, None, false)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            let CurveKind::Ellipse(e) = c.curve.kind() else {
                panic!("{name}: not a circle");
            };
            let tol = 1e-9 * (origin.length() + major + minor);
            let x = e.position().x_axis();
            res.check("iso: unit x axis", name, (x.length() - 1.0).abs(), 1e-12);
            res.check("iso: x axis normal to the torus's axis", name, x.dot(axis).abs(), 1e-12);
            let centre = origin + major * x;
            res.check("iso: centre", name, (e.position().origin() - centre).length(), tol);
            res.check("iso: x radius", name, (e.x_radius() - minor).abs(), 1e-12 * minor);
            res.check("iso: y radius", name, (e.y_radius() - minor).abs(), 1e-12 * minor);
            let normal = x.cross(e.position().y_axis());
            res.check("iso: plane holds the torus's axis", name, normal.dot(axis).abs(), 1e-12);
            let net = e.base().gw_nurb().expect("nurb");
            let pts: Vec<_> = net.cpts().iter().map(|p| (p.to_euclid(), p.w)).collect();
            let turn = assert_arcs(&mut res, name, &pts, centre, normal, minor, tol);
            res.check("iso: arcs turn the v length", name, (turn - lengths.1).abs(), 1e-9);
            let surf = t.base().base();
            let vi = surf.natural_uv_domain().v_interval();
            let line_ends = [vi.min(), vi.max()]
                .map(|v| surf.evaluate_point(Vec2::new(*iso, v)).expect("surface point"));
            let first = pts.first().expect("control points").0;
            let last = pts.last().expect("control points").0;
            let gap = (first - line_ends[0]).length().max((last - line_ends[1]).length());
            let swapped = (first - line_ends[1]).length().max((last - line_ends[0]).length());
            res.check("iso: ends on the torus's iso line", name, gap.min(swapped), tol);
            // the check tells the old rows' circle apart: an iso line end lies off the
            // opposite meridian circle by far more than the allowance
            let opposite = origin - major * x;
            let off = line_ends.map(|p| ((p - opposite).length() - minor).abs());
            assert!(off[0].max(off[1]) > 1e6 * tol, "{name}: opposite meridian not told apart: {off:?}");
        }
        assert_eq!(seen, SPINDLE_ISO_MOVED.len(), "every moved case is checked");
        for (key, used) in &res.0 {
            eprintln!("largest residual / allowance: {used:.3e}  {key}");
        }
    }
}
