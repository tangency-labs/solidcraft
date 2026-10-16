//! Runner for unit `surface.cone_p1`: `SmCone` / `SmCylinder` part 1 (construction
//! through both `SmCone::CreateCanonical` overloads, `SmCylinder::CreateCanonical` or
//! the constructors with `MakeNurb`; the state queries; STEP and NURB evaluation; the
//! parameter conversions; iso curves and iso circles; STEP inversion; positive points;
//! `AdjustSTEPUVDomain`; `Transform`; `operator==`; `Copy`) and the file-static
//! `sm_TotalCoincidenceChecker`.
//!
//! C++: `solidcore_ffi::units::surface_cone_p1` (upstream `source/SMLib/src/SmCone.cpp`,
//! `source/SMLib/src/SmCylinder.cpp`). Rust: `solidcore::surface::cone::p1`.
//!
//! A case is a `cone` (construction, placement, `oob`) plus `op` and its parameters,
//! or the static `coincidence` op on two curves. Both sides produce the same flat
//! record (layout in the FFI module docs), split into `build` (the construction
//! status), `status` (the op status) and `out` (the rest). Statuses are raw `SmStatus`
//! values (`1000` = `SM_SUCCESS`); `-1` on the Rust side for an error upstream cannot
//! produce; `-1002` (`SC_ERR_ARG`) where the shim refuses an op upstream would crash
//! on (the Rust side applies the same test).
//!
//! Deferred cases: `AdjustSTEPUVDomain` within the apex
//! (`SmSurfOfRevolution::AdjustSTEPUVDomain`, `surface.surf_of_revolution_p2`) and
//! non-identity `Transform` with a valid scale (`SmSurfOfRevolution::Transform`,
//! `surface.surf_of_revolution_p3`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::line::Line;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::geom_utility::p1::line_closest_point;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::math::EFF_ZERO_SQ;
use solidcore::nurbs::b_spline_surface::SurfParamType;
use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::knots::Side;
use solidcore::surface::cone::{total_coincidence_checker, Cone};
use solidcore::surface::surface::p2::{
    B_SPLINE_SURFACE_TYPE, CONE_TYPE, CYLINDER_TYPE, SURF_OF_REVOLUTION_TYPE, TYPE,
};
use solidcore::SolidError;
use solidcore_ffi::units::surface_cone_p1 as ffi;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1500;

/// `SM_SUCCESS`.
const SUCCESS: f64 = 1000.0;
/// `SM_ERR`.
const SM_ERR: f64 = 1001.0;
/// `SM_ERR_INVALID_INPUT` (what [`SolidError::InvalidInput`] stands for).
const SM_ERR_INVALID_INPUT: f64 = 1007.0;
/// `SC_ERR_ARG` (`harness/cpp/shim/sc_shim.h`): the shim refused the op.
const SC_ERR_ARG: f64 = -1002.0;

/// `surface.cone_p1` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ModeIn {
    CanonicalSemi {
        bot: f64,
        semi: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        height: Option<f64>,
        #[serde(default)]
        swap: bool,
        #[serde(default)]
        inside: bool,
    },
    Canonical {
        bot: f64,
        top: f64,
        height: f64,
        #[serde(default)]
        swap: bool,
        #[serde(default)]
        inside: bool,
    },
    CylCanonical {
        radius: f64,
    },
    ConeUnbounded {
        base: f64,
        semi: f64,
        #[serde(default)]
        swap: bool,
        #[serde(default)]
        inside: bool,
    },
    ConeBounded {
        bot: f64,
        top: f64,
        start: f64,
        end: f64,
        height: f64,
        #[serde(default)]
        swap: bool,
        #[serde(default)]
        inside: bool,
        #[serde(default)]
        make_nurb_gen: bool,
    },
    CylBounded {
        radius: f64,
        start: f64,
        end: f64,
        height: f64,
        #[serde(default)]
        swap: bool,
        #[serde(default)]
        inside: bool,
        #[serde(default)]
        make_nurb_gen: bool,
    },
    CylUnbounded {
        radius: f64,
        #[serde(default)]
        swap: bool,
        #[serde(default)]
        inside: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConeIn {
    mode: ModeIn,
    origin: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    #[serde(default)]
    oob: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CurveIn {
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Build {},
    EvalStep {
        u: f64,
        v: f64,
        hu: usize,
        hv: usize,
        #[serde(default)]
        point: bool,
    },
    Eval {
        u: f64,
        v: f64,
        hu: usize,
        hv: usize,
        #[serde(default = "yes")]
        u_left: bool,
        #[serde(default = "yes")]
        v_left: bool,
        #[serde(default)]
        upper: bool,
        #[serde(default = "yes")]
        non_zero: bool,
    },
    EvalPoint {
        u: f64,
        v: f64,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dom: Option<[f64; 4]>,
        #[serde(default)]
        uv: bool,
    },
    IsoCircle {
        p: [f64; 3],
        tol: f64,
    },
    Inversion {
        p: [f64; 3],
        tol: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        guess: Option<[f64; 2]>,
    },
    Positive {
        p: [f64; 3],
    },
    Adjust {
        dom: [f64; 4],
    },
    Transform {
        origin: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scale: Option<[f64; 3]>,
    },
    Eq {
        other: Box<ConeIn>,
    },
    Copy {},
    Coincidence {
        a: CurveIn,
        a_dom: [f64; 2],
        b: CurveIn,
        b_dom: [f64; 2],
        tol: f64,
    },
}

const fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cone: Option<ConeIn>,
    #[serde(flatten)]
    op: OpIn,
}

// ---- C++ side ---------------------------------------------------------------------

fn ffi_mode(m: &ModeIn) -> ffi::Mode {
    match *m {
        ModeIn::CanonicalSemi {
            bot,
            semi,
            height,
            swap,
            inside,
        } => ffi::Mode::CanonicalSemi {
            bot,
            semi,
            height,
            swap,
            inside,
        },
        ModeIn::Canonical {
            bot,
            top,
            height,
            swap,
            inside,
        } => ffi::Mode::Canonical {
            bot,
            top,
            height,
            swap,
            inside,
        },
        ModeIn::CylCanonical { radius } => ffi::Mode::CylCanonical { radius },
        ModeIn::ConeUnbounded {
            base,
            semi,
            swap,
            inside,
        } => ffi::Mode::ConeUnbounded {
            base,
            semi,
            swap,
            inside,
        },
        ModeIn::ConeBounded {
            bot,
            top,
            start,
            end,
            height,
            swap,
            inside,
            make_nurb_gen,
        } => ffi::Mode::ConeBounded {
            bot,
            top,
            start,
            end,
            height,
            swap,
            inside,
            make_nurb_gen,
        },
        ModeIn::CylBounded {
            radius,
            start,
            end,
            height,
            swap,
            inside,
            make_nurb_gen,
        } => ffi::Mode::CylBounded {
            radius,
            start,
            end,
            height,
            swap,
            inside,
            make_nurb_gen,
        },
        ModeIn::CylUnbounded {
            radius,
            swap,
            inside,
        } => ffi::Mode::CylUnbounded {
            radius,
            swap,
            inside,
        },
    }
}

fn ffi_cone(c: &ConeIn) -> ffi::Build {
    ffi::Build {
        mode: ffi_mode(&c.mode),
        origin: c.origin,
        x: c.x,
        y: c.y,
        oob: c.oob,
    }
}

fn ffi_curve(c: &CurveIn) -> ffi::CurveIn {
    match c.clone() {
        CurveIn::Line { a, b } => ffi::CurveIn::Line { a, b },
        CurveIn::Circle { c, x, y, d, r } => ffi::CurveIn::Circle { c, x, y, d, r },
        CurveIn::Ellipse { c, x, y, d, rx, ry } => ffi::CurveIn::Ellipse { c, x, y, d, rx, ry },
    }
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> { cpp_with_roundtrip(i, None, false) }

fn cpp_with_roundtrip(i: &Inputs, roundtrip: Option<bool>, subdomain: bool) -> Result<Output, HarnessError> {
    let flat = if let OpIn::Coincidence {
        a,
        a_dom,
        b,
        b_dom,
        tol,
    } = &i.op
    {
        ffi::coincidence(&ffi_curve(a), *a_dom, &ffi_curve(b), *b_dom, *tol)
    } else {
        let c = i.cone.as_ref().ok_or_else(|| harness("missing cone"))?;
        let op = match &i.op {
            OpIn::Build {} => ffi::Op::Build,
            OpIn::EvalStep {
                u,
                v,
                hu,
                hv,
                point,
            } => ffi::Op::EvalStep {
                u: *u,
                v: *v,
                hu: *hu,
                hv: *hv,
                point: *point,
            },
            OpIn::Eval {
                u,
                v,
                hu,
                hv,
                u_left,
                v_left,
                upper,
                non_zero,
            } => ffi::Op::Eval {
                u: *u,
                v: *v,
                hu: *hu,
                hv: *hv,
                u_left: *u_left,
                v_left: *v_left,
                upper: *upper,
                non_zero: *non_zero,
            },
            OpIn::EvalPoint { u, v } => ffi::Op::EvalPoint { u: *u, v: *v },
            OpIn::ToStep { u, v } => ffi::Op::ToStep { u: *u, v: *v },
            OpIn::ToNurbs { u, v } => ffi::Op::ToNurbs { u: *u, v: *v },
            OpIn::Iso {
                param,
                iso,
                dom,
                uv,
            } => ffi::Op::Iso {
                param: *param,
                iso: *iso,
                dom: *dom,
                uv: *uv,
            },
            OpIn::IsoCircle { p, tol } => ffi::Op::IsoCircle { p: *p, tol: *tol },
            OpIn::Inversion { p, tol, guess } => ffi::Op::Inversion {
                p: *p,
                tol: *tol,
                guess: *guess,
            },
            OpIn::Positive { p } => ffi::Op::Positive { p: *p },
            OpIn::Adjust { dom } => ffi::Op::Adjust { dom: *dom },
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
                other: Box::new(ffi_cone(other)),
            },
            OpIn::Copy {} => ffi::Op::Copy,
            OpIn::Coincidence { .. } => return Err(harness("unreachable")),
        };
        if subdomain { ffi::inversion_subdomain(&ffi_cone(c), &op) }
        else if let Some(binary) = roundtrip { ffi::roundtrip_op(&ffi_cone(c), &op, binary) }
        else { ffi::cone_op(&ffi_cone(c), &op) }
    }
    .map_err(|e| harness(e.to_string()))?;
    to_output(i, &flat)
}

/// Splits a record into `build`, `status` and `out`.
fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut out = Output::new();
    let mut rest = flat;
    if !matches!(i.op, OpIn::Coincidence { .. }) {
        let (b, r) = rest
            .split_first()
            .ok_or_else(|| harness("empty record"))?;
        out = out.values("build", vec![*b]);
        rest = r;
        if *b != SUCCESS {
            if !rest.is_empty() {
                return Err(harness("record continues after a failed build"));
            }
            return Ok(out);
        }
    }
    if !matches!(i.op, OpIn::Build {}) {
        let (s, r) = rest
            .split_first()
            .ok_or_else(|| harness("record has no op status"))?;
        out = out.values("status", vec![*s]);
        rest = r;
    }
    Ok(out.values("out", rest.to_vec()))
}

// ---- Rust side ----------------------------------------------------------------------

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

/// The `SmStatus` of a Rust error (`InvalidInput` is `SM_ERR_INVALID_INPUT`); `-1` for
/// one upstream cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::Upstream { code, .. } => f64::from(*code),
        SolidError::InvalidInput { .. } => SM_ERR_INVALID_INPUT,
        _ => -1.0,
    }
}

/// Builds the cone; `Ok(Err(e))` when the construction fails.
fn rust_cone(k: &Kernel, c: &ConeIn) -> solidcore::Result<Cone> {
    let (o, x, y) = (v3(c.origin), v3(c.x), v3(c.y));
    let a2p = Axis2Placement::new(o, x, y);
    let mut cone = match c.mode {
        ModeIn::CanonicalSemi {
            bot,
            semi,
            height,
            swap,
            inside,
        } => Cone::create_canonical_with_semi_angle(k, &a2p, bot, semi, height, swap, inside)?,
        ModeIn::Canonical {
            bot,
            top,
            height,
            swap,
            inside,
        } => Cone::create_canonical(k, &a2p, bot, top, height, swap, inside)?,
        ModeIn::CylCanonical { radius } => Cone::create_canonical_cylinder(k, &a2p, radius)?,
        ModeIn::ConeUnbounded {
            base,
            semi,
            swap,
            inside,
        } => {
            let mut c = Cone::new_unbounded(k, o, x, y, base, semi, swap, inside)?;
            c.make_nurb(k)?;
            c
        }
        ModeIn::ConeBounded {
            bot,
            top,
            start,
            end,
            height,
            swap,
            inside,
            make_nurb_gen,
        } => {
            let mut c = Cone::new_bounded(
                o,
                x,
                y,
                bot,
                top,
                start,
                end,
                height,
                swap,
                inside,
                make_nurb_gen,
            );
            c.make_nurb(k)?;
            c
        }
        ModeIn::CylBounded {
            radius,
            start,
            end,
            height,
            swap,
            inside,
            make_nurb_gen,
        } => {
            let mut c = Cone::new_cylinder_bounded(
                o,
                x,
                y,
                radius,
                start,
                end,
                height,
                swap,
                inside,
                make_nurb_gen,
            );
            c.make_nurb(k)?;
            c
        }
        ModeIn::CylUnbounded {
            radius,
            swap,
            inside,
        } => {
            let mut c = Cone::new_cylinder_unbounded(k, o, x, y, radius, swap, inside)?;
            c.make_nurb(k)?;
            c
        }
    };
    cone.set_out_of_bounds_enabled(c.oob);
    Ok(cone)
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

fn put_curve(o: &mut Vec<f64>, c: Option<&Curve>) {
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

/// `len chars...` of a string (`GetTypeString` / `GetClassString`).
fn put_str(o: &mut Vec<f64>, t: &str) {
    o.push(t.len() as f64);
    o.extend(t.bytes().map(f64::from));
}

fn put_state(k: &Kernel, o: &mut Vec<f64>, s: &Cone) -> solidcore::Result<()> {
    let rev = s.rev();
    o.push(s.type_code() as f64);
    put_str(o, s.type_string());
    put_str(o, s.class_string());
    o.extend([
        flag(s.is_kind_of(CYLINDER_TYPE)?),
        flag(s.is_kind_of(CONE_TYPE)?),
        flag(s.is_kind_of(SURF_OF_REVOLUTION_TYPE)?),
        flag(s.is_kind_of(B_SPLINE_SURFACE_TYPE)?),
        flag(s.is_kind_of(TYPE)?),
        flag(s.is_cylinder()),
        s.base_radius(),
        s.semi_angle_deg(),
        flag(s.inside_out()),
        flag(rev.swap_uv()),
        flag(rev.planar_generator()),
        flag(rev.make_nurb_gen_curve()),
        flag(rev.is_bounded()?),
    ]);
    let pos = rev.position();
    push_v(o, pos.origin());
    push_v(o, pos.x_axis());
    push_v(o, pos.y_axis());
    let d = rev.step_uv_domain();
    o.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
    o.extend([
        s.bot_radius(),
        s.top_radius(),
        s.bot_height(),
        s.top_height(),
        s.radius(1.25),
    ]);
    let (le, lp) = s.step_linear_param_extent();
    o.extend([le.min(), le.max(), f64::from(lp as u8)]);
    let mx = s.max_analytic_domain(k)?;
    o.extend([mx.min().x, mx.min().y, mx.max().x, mx.max().y]);
    let apex = s.apex(k)?;
    o.push(if apex.is_cylinder { SM_ERR } else { SUCCESS });
    o.push(flag(apex.is_cylinder));
    push_v(o, apex.point);
    o.push(apex.param);
    let can = s.canonical();
    push_v(o, can.origin.origin());
    o.extend([
        can.radius,
        can.semi_angle_deg,
        flag(can.swap_uv),
        flag(can.inside_out),
    ]);
    // GetHeight's failed evaluations leave initialized SmPoint3d sentinels.
    // Compare that defined result even when the polar curve is missing.
    o.extend([1.0, s.height(k)?]);
    let pc = rev.polar_converter();
    o.push(flag(rev.have_polar_conversion()));
    let angles = pc.angles();
    o.push(angles.len() as f64);
    if let (Some(a), Some(b)) = (angles.first(), angles.last()) {
        o.extend([*a, *b]);
    }
    let polar_curve = rev.polar_curve().map(|e| Curve::from(e.clone()));
    put_curve(o, polar_curve.as_ref());
    put_curve(o, rev.gen_curve());
    let sur = rev.base().gw_nurb();
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

fn side(left: bool) -> Side {
    if left {
        Side::Left
    } else {
        Side::Right
    }
}

/// The shim's test before `CreateIsoCircleFromPoint`: the STEP inversion it makes
/// leaves the parameters unset, or the NURB conversion of those parameters fails and
/// leaves the NURB parameter it then uses unset (always without `swap_uv`; with it,
/// when the polar converter wrote nothing).
fn iso_circle_inversion_fails(k: &Kernel, s: &Cone, p: Vec3, tol: f64) -> solidcore::Result<bool> {
    let pos = s.rev().position();
    let origin = pos.origin();
    let z_axis = pos.z_axis();
    let height_ivl = s.rev().step_uv_domain().v_interval();
    let param = line_closest_point(origin, z_axis, p);
    if !height_ivl.contains_value(param, tol) {
        return Ok(false);
    }
    let radius = s.radius(param);
    let center = origin + param * z_axis;
    let mut vec = p - center;
    if vec.length_squared() < 2.0 * EFF_ZERO_SQ {
        vec = pos.x_axis();
    }
    let vec = vec.unitize_or_z();
    let point_on_surface = center + radius * vec;
    let inv = match s.step_inversion(k, point_on_surface, tol, None) {
        Ok(inv) => inv,
        Err(e) if e.is_unported() => return Err(e),
        Err(_) => return Ok(true),
    };
    match s.convert_uv_from_step_to_nurbs(k, inv.uv) {
        Ok(_) => Ok(false),
        Err(e) if e.is_unported() => Err(e),
        Err(_) if !s.rev().swap_uv() => Ok(true),
        Err(_) => {
            let Some(circle) = s.rev().polar_curve() else {
                return Ok(true);
            };
            Ok(s.rev()
                .polar_converter()
                .convert_to_nurbs_parameter_or_written(k, circle, inv.uv.x, 0.0)?
                .is_none())
        }
    }
}

/// Runs the op on the Rust cone, appending its status and outputs.
fn rust_op(k: &Kernel, s: &Cone, op: &OpIn, o: &mut Vec<f64>) -> Result<solidcore::Result<()>, HarnessError> {
    let r: solidcore::Result<()> = match op {
        OpIn::Build {} => put_state(k, o, s),
        OpIn::EvalStep {
            u,
            v,
            hu,
            hv,
            point,
        } => {
            let uv = Point2::new(*u, *v);
            if *point {
                s.evaluate_step_point(uv).map(|p| {
                    o.push(SUCCESS);
                    push_v(o, p);
                })
            } else {
                s.evaluate_step(uv, *hu, *hv).map(|d| {
                    o.push(SUCCESS);
                    for v in d {
                        push_v(o, v);
                    }
                })
            }
        }
        OpIn::Eval {
            u,
            v,
            hu,
            hv,
            u_left,
            v_left,
            upper,
            non_zero,
        } => {
            let flags = EvaluateFlags {
                u_side: side(*u_left),
                v_side: side(*v_left),
                only_upper_half: *upper,
                non_zero_tangents: *non_zero,
            };
            s.evaluate(k, Point2::new(*u, *v), *hu, *hv, flags).map(|d| {
                o.push(SUCCESS);
                for v in d {
                    push_v(o, v);
                }
            })
        }
        OpIn::EvalPoint { u, v } => s.evaluate_point(k, Point2::new(*u, *v)).map(|p| {
            o.push(SUCCESS);
            push_v(o, p);
        }),
        OpIn::ToStep { u, v } => s.convert_uv_from_nurbs_to_step(k, Point2::new(*u, *v)).map(|p| {
            o.extend([SUCCESS, p.x, p.y]);
        }),
        OpIn::ToNurbs { u, v } => {
            if s.rev().polar_curve().is_none() {
                o.push(SC_ERR_ARG);
                Ok(())
            } else {
                s.convert_uv_from_step_to_nurbs(k, Point2::new(*u, *v)).map(|p| {
                    o.extend([SUCCESS, p.x, p.y]);
                })
            }
        }
        OpIn::Iso { param, iso, dom, uv } => {
            let d = dom.map(|[a, b, c, e]| Extent2d::from_bounds(a, b, c, e));
            s.create_iso_parametric_curve(k, param_of(*param), *iso, d.as_ref(), *uv)
                .map(|c| {
                    o.extend([SUCCESS, c.max_gap_3d]);
                    put_curve(o, Some(&c.curve));
                    put_curve(o, c.uv_iso_curve.as_ref());
                })
        }
        OpIn::IsoCircle { p, tol } => {
            if s.rev().polar_curve().is_none() {
                o.push(SC_ERR_ARG);
                Ok(())
            } else {
                match iso_circle_inversion_fails(k, s, v3(*p), *tol) {
                    Err(e) => Err(e),
                    Ok(true) => {
                        o.push(SC_ERR_ARG);
                        Ok(())
                    }
                    Ok(false) => s.create_iso_circle_from_point(k, v3(*p), *tol).map(|c| {
                        o.push(SUCCESS);
                        put_curve(o, c.as_ref());
                    }),
                }
            }
        }
        OpIn::Inversion { p, tol, guess } => s
            .step_inversion(k, v3(*p), *tol, guess.map(|[a, b]| Point2::new(a, b)))
            .map(|inv| {
                o.push(if inv.inside { SUCCESS } else { SM_ERR });
                o.extend([inv.uv.x, inv.uv.y, f64::from(inv.location as u8)]);
            }),
        OpIn::Positive { p } => s.positive_point(k, v3(*p)).map(|pp| {
            o.extend([SUCCESS, flag(pp.negated)]);
            push_v(o, pp.point);
            o.push(pp.apex_param);
        }),
        OpIn::Adjust { dom } => {
            let [a, b, c, e] = *dom;
            let mut t = s.clone();
            t.adjust_step_uv_domain(&Kernel::default(), &Extent2d::from_bounds(a, b, c, e))
                .and_then(|()| {
                    o.push(SUCCESS);
                    put_state(k, o, &t)
                })
        }
        OpIn::Transform {
            origin,
            x,
            y,
            scale,
        } => {
            let a2p = Axis2Placement::new(v3(*origin), v3(*x), v3(*y));
            let mut t = s.clone();
            t.transform(k, &a2p, scale.map(v3)).and_then(|()| {
                o.push(SUCCESS);
                put_state(k, o, &t)
            })
        }
        OpIn::Eq { other } => {
            match rust_cone(k, other) {
                Err(e) => {
                    if e.is_unported() {
                        return Ok(Err(e));
                    }
                    o.push(status(&e));
                }
                Ok(oc) => {
                    o.extend([SUCCESS, flag(s.approx_eq(&oc)), flag(oc.approx_eq(s))]);
                }
            }
            Ok(())
        }
        OpIn::Copy {} => {
            let c = s.clone();
            o.extend([SUCCESS, flag(c.approx_eq(s))]);
            put_state(k, o, &c)
        }
        OpIn::Coincidence { .. } => return Err(harness("not a cone op")),
    };
    Ok(r)
}

fn rust_curve(k: &Kernel, c: &CurveIn) -> Result<Curve, HarnessError> {
    Ok(match c {
        CurveIn::Line { a, b } => {
            let mut l = Line::from_points(v3(*a), v3(*b), 3);
            l.make_nurb().map_err(|e| harness(format!("line nurb: {e}")))?;
            Curve::from(l)
        }
        CurveIn::Circle { c, x, y, d, r } => {
            let [d0, d1] = *d;
            Curve::from(
                Ellipse::new_circle(
                    k,
                    v3(*c),
                    v3(*x),
                    v3(*y),
                    Extent1d::from_min_max(d0, d1),
                    *r,
                    &EllipseOptions::default(),
                )
                .map_err(|e| harness(format!("circle: {e}")))?,
            )
        }
        CurveIn::Ellipse { c, x, y, d, rx, ry } => {
            let [d0, d1] = *d;
            Curve::from(
                Ellipse::new(
                    k,
                    v3(*c),
                    v3(*x),
                    v3(*y),
                    Extent1d::from_min_max(d0, d1),
                    *rx,
                    *ry,
                    &EllipseOptions::default(),
                )
                .map_err(|e| harness(format!("ellipse: {e}")))?,
            )
        }
    })
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    if let OpIn::Coincidence {
        a,
        a_dom,
        b,
        b_dom,
        tol,
    } = &i.op
    {
        let (ca, cb) = (rust_curve(&k, a)?, rust_curve(&k, b)?);
        let [a0, a1] = *a_dom;
        let [b0, b1] = *b_dom;
        let r = total_coincidence_checker(
            &k,
            &ca,
            &Extent1d::from_min_max(a0, a1),
            &cb,
            &Extent1d::from_min_max(b0, b1),
            *tol,
        );
        let flat = match r {
            Ok(c) => vec![
                SUCCESS,
                flag(c.coincident),
                c.reversed.map_or(2.0, flag),
            ],
            Err(e) => {
                if let Some(out) = Output::unported(&e) {
                    return Ok(out);
                }
                // upstream's flags keep their initial FALSE / UNSURE
                vec![status(&e), 0.0, 2.0]
            }
        };
        return to_output(i, &flat);
    }
    let c = i.cone.as_ref().ok_or_else(|| harness("missing cone"))?;
    let mut o = Vec::new();
    match rust_cone(&k, c) {
        Err(e) => {
            if let Some(out) = Output::unported(&e) {
                return Ok(out);
            }
            o.push(status(&e));
        }
        Ok(cone) => {
            o.push(SUCCESS);
            let at = o.len();
            if let Err(e) = rust_op(&k, &cone, &i.op, &mut o)? {
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

// Issue #76 integration coverage reuses the exact constructor inputs and native guards.
// The numerical implementation remains cone_p1; these calls traverse Surface instead.
pub(super) fn common_cases(ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
    let mut cases = Runner.cases(ctx)?;
    cases.retain(|c| matches!(c.inputs.get("op").and_then(Value::as_str),
        Some("eval" | "eval_point" | "eval_step" | "to_step" | "to_nurbs" | "iso" | "inversion" | "adjust" | "transform" | "copy")));
    let mut numerical = Vec::new();
    for case in cases {
        let input: Inputs = case.parse()?;
        let native = cpp(&input)?;
        if native.values.get("status").is_some_and(|v| v == &[SC_ERR_ARG]) { continue; }
        numerical.push(case);
    }
    let mut cases = numerical;
    for c in &mut cases {
        c.unit = "surface.surface_p2".into();
        c.name = format!("common_cone_{}", c.name);
        c.inputs = serde_json::json!({ "common_cone": c.inputs });
    }
    let subdomains = cases.iter()
        .filter(|c| c.inputs.get("common_cone").and_then(|v| v.get("op")).and_then(Value::as_str) == Some("inversion"))
        .cloned().map(|mut c| {
            c.name.push_str("_subdomain");
            c.inputs.as_object_mut().ok_or_else(|| harness("common input object"))?
                .insert("common_subdomain".into(), Value::Bool(true));
            Ok(c)
        }).collect::<Result<Vec<_>, HarnessError>>()?;
    cases.extend(subdomains);
    let mut database_cases = Vec::new();
    for (class, mode) in [
        ("cone", serde_json::json!({"kind":"canonical", "bot":2.0,"top":4.0,"height":3.0})),
        ("cylinder", serde_json::json!({"kind":"cyl_canonical", "radius":2.0})),
    ] {
        for binary in [false, true] {
            for op in [serde_json::json!({"op":"eval_point","u":0.25,"v":0.5}),
                serde_json::json!({"op":"eval_step","u":90.0,"v":1.0,"hu":1,"hv":1,"point":true}),
                serde_json::json!({"op":"to_step","u":0.25,"v":0.5}),
                serde_json::json!({"op":"copy"})] {
                let mut value = op;
                value.as_object_mut().ok_or_else(|| harness("database operation object"))?.insert("cone".into(), serde_json::json!({"mode":mode,"origin":[0.0,0.0,0.0],"x":[1.0,0.0,0.0],"y":[0.0,1.0,0.0]}));
                let i: Inputs = serde_json::from_value(value.clone()).map_err(|e| harness(e.to_string()))?;
                let data = ffi::serialize_cone(&ffi_cone(i.cone.as_ref().ok_or_else(|| harness("missing cone"))?), binary)
                    .map_err(|e| harness(e.to_string()))?;
                if data.first() != Some(&SUCCESS) || data.get(1) != Some(&SUCCESS) {
                    return Err(harness("database fixture constructor/writer failed"));
                }
                let bytes: Vec<u8> = data.into_iter().skip(2).map(|v| v as u8).collect();
                database_cases.push(Case { unit:"surface.surface_p2".into(),
                    name:format!("common_db_{class}_{binary}_{}", database_cases.len()),
                    source: crate::case::CaseSource::Generated { seed: 76 },
                    inputs:serde_json::json!({"common_cone":value,"common_db":{"binary":binary,"bytes":bytes}}), });
            }
        }
    }
    for class in ["cone", "cylinder"] {
        for swap in [false, true] {
            for inside in [false, true] {
                for u in [0.0, 0.25, 0.5, 1.0] {
                    let mode = if class == "cone" { serde_json::json!({"kind":"canonical", "bot":2.0,"top":4.0,"height":3.0,"swap":swap,"inside":inside}) }
                        else { serde_json::json!({"kind":"cyl_bounded", "radius":2.0,"start":0.0,"end":360.0,"height":3.0,"swap":swap,"inside":inside}) };
                    database_cases.push(Case {unit:"surface.surface_p2".into(),
                        name:format!("common_lazy_{class}_{swap}_{inside}_{u}"),
                        source:crate::case::CaseSource::Generated {seed:76},
                        inputs:serde_json::json!({"common_lazy":true,"common_cone":{"op":"eval_point","u":u,"v":0.5,
                            "cone":{"mode":mode,"origin":[0.0,0.0,0.0],"x":[1.0,0.0,0.0],"y":[0.0,1.0,0.0]}}}), });
                }
            }
        }
    }
    let mut queries = Vec::new();
    for case in &database_cases {
        if !case.name.starts_with("common_lazy_") || !case.name.ends_with("_0.25") { continue; }
        for subdomain in [false, true] {
            let mut case = case.clone();
            case.name = format!("{}_queries_{subdomain}", case.name);
            case.inputs.as_object_mut().ok_or_else(|| harness("query input object"))?.remove("common_lazy");
            case.inputs.as_object_mut().ok_or_else(|| harness("query input object"))?.insert("common_queries".into(), Value::Bool(true));
            case.inputs.as_object_mut().ok_or_else(|| harness("query input object"))?.insert("common_subdomain".into(), Value::Bool(subdomain));
            queries.push(case);
        }
    }
    database_cases.extend(queries);
    cases.extend(database_cases);
    Ok(cases)
}

pub(super) fn common_cpp(value: Value, database: Option<&Value>, lazy: bool, subdomain: bool, queries: bool) -> Result<Output, HarnessError> {
    let i: Inputs = serde_json::from_value(value).map_err(|e| harness(e.to_string()))?;
    if queries {
        let build = ffi_cone(i.cone.as_ref().ok_or_else(|| harness("missing cone"))?);
        return to_output(&i, &ffi::common_queries(&build, subdomain).map_err(|e| harness(e.to_string()))?);
    }
    if lazy {
        let OpIn::EvalPoint {u, v} = i.op else { return Err(harness("lazy parameters")); };
        let build = ffi_cone(i.cone.as_ref().ok_or_else(|| harness("missing cone"))?);
        return to_output(&i, &ffi::lazy_cone(&build, [u, v]).map_err(|e| harness(e.to_string()))?);
    }
    let mut output = cpp_with_roundtrip(&i, database.and_then(|v| v.get("binary")).and_then(Value::as_bool), subdomain)?;
    // Surface's fallible inversion API exposes UV/location only on success. Direct
    // cone_p1 cases separately compare the native outputs retained on failure.
    if matches!(i.op, OpIn::Inversion { .. }) && output.values.get("status").is_some_and(|v| v != &[SUCCESS]) {
        output.values.insert("out".into(), Vec::new());
    }
    Ok(output)
}

pub(super) fn common_rust(value: Value, database: Option<&Value>, lazy: bool, subdomain: bool, queries: bool) -> Result<Output, HarnessError> {
    use solidcore::surface::surface::{Surface, SurfaceKind};
    let i: Inputs = serde_json::from_value(value).map_err(|e| harness(e.to_string()))?;
    let k = Kernel::default();
    let cone = i.cone.as_ref().ok_or_else(|| harness("missing cone"))?;
    let mut out = vec![SUCCESS];
    let built = if let Some(database) = database {
        use solidcore::topology::database_io::{DatabaseReader, FileType};
        let bytes: Vec<u8> = serde_json::from_value(database.get("bytes").ok_or_else(|| harness("database bytes"))?.clone()).map_err(|e| harness(e.to_string()))?;
        let mut reader = DatabaseReader::new();
        reader.open_for_read(bytes, if database.get("binary").and_then(Value::as_bool).unwrap_or(false) { FileType::Binary } else { FileType::Ascii });
        let ty = if matches!(cone.mode, ModeIn::CylCanonical { .. }) { CYLINDER_TYPE } else { CONE_TYPE };
        Surface::read_from_db(&k, ty, &mut reader, 30)
    } else { rust_cone(&k, cone).map(Surface::from) };
    let mut surface = match built {
        Ok(c) => c,
        Err(e) => return to_output(&i, &[status(&e)]),
    };
    if queries {
        let natural = surface.natural_uv_domain();
        let low = natural.evaluate(0.2, 0.2); let high = natural.evaluate(0.8, 0.8);
        let domain = if subdomain { Extent2d::from_bounds(low.x, low.y, high.x, high.y) } else { natural };
        let result = (|| -> solidcore::Result<()> {
            out.push(SUCCESS);
            let tolerance = k.tolerances.zone_tol_3d / 10.0;
            out.push(flag(surface.is_degenerate_point(&k, tolerance, Some(&domain))?));
            out.push(flag(surface.is_degenerate_curve(&k, solidcore::math::EFF_ZERO_DEG, Some(&domain), tolerance)?));
            match surface.evaluate_normal(&k, domain.mid(), Side::Left, Side::Left) {
                Ok(n) => { out.push(SUCCESS); push_v(&mut out, n); }
                Err(e) if e.is_unported() => return Err(e),
                Err(e) => out.push(status(&e)),
            }
            match surface.calculate_bounding_box(&k, &domain, [true,false,false], false) {
                Ok(b) => { out.push(SUCCESS); let b = b.normal_box.ok_or_else(|| SolidError::invalid_input("common_queries", "missing box"))?;
                    push_v(&mut out, b.min()); push_v(&mut out, b.max()); }
                Err(e) if e.is_unported() => return Err(e),
                Err(e) => out.push(status(&e)),
            }
            Ok(())
        })();
        if let Err(e) = result { return Ok(Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string()))); }
        return to_output(&i, &out);
    }
    if lazy {
        use solidcore::surface::surface::p2::SurfaceEval;
        let OpIn::EvalPoint {u, v} = i.op else { return Err(harness("lazy parameters")); };
        let mut eval = SurfaceEval::with_uv(&k, Some(&surface), u, v);
        out.push(SUCCESS);
        for p in [eval.pos(), eval.su(), eval.sv(), eval.suu(), eval.suv(), eval.svv()] {
            push_v(&mut out, p.map_err(|e| harness(e.to_string()))?);
        }
        return to_output(&i, &out);
    }
    let uv = |u, v| Point2::new(u, v);
    let r = match &i.op {
        OpIn::Eval { u, v, hu, hv, u_left, v_left, upper, non_zero } => {
            surface.evaluate(&k, uv(*u, *v), *hu, *hv, EvaluateFlags {
                u_side: side(*u_left), v_side: side(*v_left), only_upper_half: *upper,
                non_zero_tangents: *non_zero,
            }).map(|d| { out.push(SUCCESS); for p in d.into_iter().flatten() { push_v(&mut out, p); } })
        }
        OpIn::EvalPoint { u, v } => surface.evaluate_point(&k, uv(*u, *v))
            .map(|p| { out.push(SUCCESS); push_v(&mut out, p); }),
        OpIn::EvalStep { u, v, hu, hv, point } => {
            if *point { surface.evaluate_step_point(&k, uv(*u, *v))
                .map(|p| { out.push(SUCCESS); push_v(&mut out, p); }) }
            else { surface.evaluate_step(&k, uv(*u, *v), *hu, *hv, EvaluateFlags::default())
                .map(|d| { out.push(SUCCESS); for p in d.into_iter().flatten() { push_v(&mut out, p); } }) }
        }
        OpIn::ToStep { u, v } => surface.convert_uv_from_nurbs_to_step(&k, uv(*u, *v))
            .map(|p| out.extend([SUCCESS, p.x, p.y])),
        OpIn::ToNurbs { u, v } => {
            let no_polar = matches!(surface.kind(), SurfaceKind::Cone(c) if c.rev().polar_curve().is_none());
            if no_polar { out.push(SC_ERR_ARG); Ok(()) }
            else { surface.convert_uv_from_step_to_nurbs(&k, uv(*u, *v))
                .map(|p| out.extend([SUCCESS, p.x, p.y])) }
        }
        OpIn::Iso { param, iso, dom, uv } => {
            let domain = dom.map(|[a,b,c,d]| Extent2d::from_bounds(a,b,c,d));
            surface.create_iso_parametric_curve(&k, param_of(*param), *iso, 0.0, domain.as_ref(), *uv)
                .map(|c| { out.extend([SUCCESS, c.max_gap_3d]); put_curve(&mut out, Some(&c.curve)); put_curve(&mut out, c.uv_iso_curve.as_ref()); })
        }
        OpIn::Inversion { p, tol, guess } => {
            let natural = surface.step_uv_domain();
            let domain = if subdomain { { let low = natural.evaluate(0.2, 0.2); let high = natural.evaluate(0.8, 0.8); Extent2d::from_bounds(low.x,low.y,high.x,high.y) } } else { natural };
            surface.step_inversion(&k, &domain, v3(*p), *tol, guess.map(|[u,v]| Point2::new(u,v)))
                .map(|(uv,location)| out.extend([SUCCESS,uv.x,uv.y,f64::from(location as u8)]))
        }
        OpIn::Copy {} => surface.copy().and_then(|copy| {
            out.extend([SUCCESS, flag(copy.approx_eq(&k, &surface)?)]);
            match copy.kind() { SurfaceKind::Cone(c) => put_state(&k, &mut out, c),
                _ => Err(SolidError::invalid_input("difftest::common_cone", "copy erased cone class")) }
        }),
        // Edits are still exercised through the common API. Failed dependencies remain deferrals.
        OpIn::Adjust { dom } => surface.adjust_step_uv_domain(&k, &Extent2d::from_bounds(dom[0], dom[1], dom[2], dom[3]))
            .and_then(|()| { out.push(SUCCESS); match surface.kind() { SurfaceKind::Cone(c) => put_state(&k, &mut out, c), _ => unreachable!() } }),
        OpIn::Transform { origin, x, y, scale } => surface.transform(&k,
            &Axis2Placement::new(v3(*origin), v3(*x), v3(*y)), scale.map(v3))
            .and_then(|()| { out.push(SUCCESS); match surface.kind() { SurfaceKind::Cone(c) => put_state(&k, &mut out, c), _ => unreachable!() } }),
        _ => return Err(harness("unsupported common surface operation")),
    };
    if let Err(e) = r {
        if let Some(out) = Output::unported(&e) { return Ok(out); }
        out.truncate(1); out.push(status(&e));
    }
    to_output(&i, &out)
}

// ---- generation -----------------------------------------------------------------------

fn unit_vec(rng: &mut Rng) -> [f64; 3] {
    loop {
        let [x, y, z] = rng.point3(-1.0, 1.0);
        let l = (x * x + y * y + z * z).sqrt();
        if l > 0.1 && l <= 1.0 {
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

fn scale3(a: [f64; 3], s: f64) -> [f64; 3] {
    let [x, y, z] = a;
    [x * s, y * s, z * s]
}

fn add3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    [ax + bx, ay + by, az + bz]
}

/// A placement: mostly orthonormal, sometimes canonical, sometimes with skewed or
/// non-unit axes.
fn frame(rng: &mut Rng, scale: f64, degenerate: bool) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let origin = scale3(rng.point3(-10.0, 10.0), scale);
    if rng.chance(0.15) {
        return (origin, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    }
    if degenerate && rng.chance(0.02) {
        // parallel or zero axes (upstream builds degenerate surfaces)
        let x = unit_vec(rng);
        return if rng.chance(0.5) {
            (origin, x, scale3(x, 2.0))
        } else {
            (origin, [0.0; 3], x)
        };
    }
    let x = unit_vec(rng);
    let mut y = normalize(cross3(cross3(x, unit_vec(rng)), x));
    if !y.iter().all(|v| v.is_finite()) {
        y = normalize(cross3(x, [0.3, 0.5, 0.8]));
    }
    if rng.chance(0.1) {
        // skewed / non-unit axes
        let x2 = scale3(x, rng.uniform(0.5, 2.0));
        let y2 = add3(scale3(y, rng.uniform(0.5, 2.0)), scale3(x, rng.uniform(-0.3, 0.3)));
        return (origin, x2, y2);
    }
    (origin, x, y)
}

fn pick_scale(rng: &mut Rng) -> f64 {
    let r = rng.unit();
    if r < 0.7 {
        1.0
    } else if r < 0.8 {
        1e-3
    } else if r < 0.9 {
        1e3
    } else if r < 0.95 {
        1e5
    } else {
        1e-6
    }
}

fn radius(rng: &mut Rng, scale: f64) -> f64 {
    let r = rng.unit();
    if r < 0.08 {
        0.0
    } else if r < 0.1 {
        1e-13 * scale
    } else if r < 0.12 {
        -rng.uniform(0.1, 2.0) * scale
    } else {
        rng.uniform(0.05, 5.0) * scale
    }
}

fn semi(rng: &mut Rng) -> f64 {
    let r = rng.unit();
    if r < 0.12 {
        0.0
    } else if r < 0.16 {
        1e-13
    } else if r < 0.2 {
        5e-11
    } else if r < 0.25 {
        -rng.uniform(1.0, 60.0)
    } else if r < 0.28 {
        rng.uniform(85.0, 89.99)
    } else if r < 0.3 {
        // outside upstream's asserted range
        rng.uniform(90.0, 200.0) * if rng.chance(0.5) { 1.0 } else { -1.0 }
    } else {
        rng.uniform(1.0, 70.0)
    }
}

fn angles(rng: &mut Rng) -> (f64, f64) {
    let r = rng.unit();
    if r < 0.5 {
        (0.0, 360.0)
    } else if r < 0.6 {
        (-180.0, 180.0)
    } else if r < 0.63 {
        // reversed, empty or over-long sweeps
        match rng.below(3) {
            0 => (90.0, 10.0),
            1 => (30.0, 30.0),
            _ => (-400.0, 400.0),
        }
    } else {
        let a = rng.uniform(-300.0, 300.0);
        let len = rng.uniform(5.0, 360.0);
        let b = (a + len).min(360.0);
        if b <= a {
            (0.0, 90.0)
        } else {
            (a, b)
        }
    }
}

fn gen_cone(rng: &mut Rng) -> ConeIn {
    let scale = pick_scale(rng);
    let (origin, x, y) = frame(rng, scale, true);
    let swap = rng.chance(0.3);
    let inside = rng.chance(0.3);
    let h = |rng: &mut Rng| {
        if rng.chance(0.05) {
            1e-9 * scale
        } else if rng.chance(0.03) {
            if rng.chance(0.5) {
                0.0
            } else {
                -rng.uniform(0.1, 3.0) * scale
            }
        } else {
            rng.uniform(0.1, 10.0) * scale
        }
    };
    let mode = match rng.below(7) {
        0 => {
            let hh = if rng.chance(0.5) { Some(h(rng)) } else { None };
            ModeIn::CanonicalSemi {
                bot: radius(rng, scale).abs(),
                semi: semi(rng).abs(),
                height: hh,
                swap,
                inside,
            }
        }
        1 => {
            let unbounded = rng.chance(0.25);
            ModeIn::Canonical {
                bot: radius(rng, scale),
                top: if unbounded {
                    semi(rng)
                } else {
                    radius(rng, scale)
                },
                height: if unbounded {
                    if rng.chance(0.5) {
                        0.0
                    } else {
                        -rng.uniform(0.1, 3.0)
                    }
                } else {
                    h(rng)
                },
                swap,
                inside,
            }
        }
        2 => ModeIn::CylCanonical {
            radius: radius(rng, scale),
        },
        3 => ModeIn::ConeUnbounded {
            base: radius(rng, scale),
            semi: semi(rng),
            swap,
            inside,
        },
        4 => {
            let (start, end) = angles(rng);
            ModeIn::ConeBounded {
                bot: radius(rng, scale),
                top: radius(rng, scale),
                start,
                end,
                height: h(rng),
                swap,
                inside,
                make_nurb_gen: rng.chance(0.3),
            }
        }
        5 => {
            let (start, end) = angles(rng);
            ModeIn::CylBounded {
                radius: radius(rng, scale),
                start,
                end,
                height: h(rng),
                swap,
                inside,
                make_nurb_gen: rng.chance(0.3),
            }
        }
        _ => ModeIn::CylUnbounded {
            radius: radius(rng, scale),
            swap,
            inside,
        },
    };
    ConeIn {
        mode,
        origin,
        x,
        y,
        oob: rng.chance(0.15),
    }
}

/// A STEP parameter pair: inside the STEP domain mostly, sometimes on its boundary or
/// outside.
fn step_uv(rng: &mut Rng, d: &Extent2d) -> [f64; 2] {
    let (u0, u1) = (d.min().x, d.max().x);
    let (v0, v1) = (d.min().y.max(-1e4), d.max().y.min(1e4));
    let r = rng.unit();
    if r < 0.1 {
        [u0, v0]
    } else if r < 0.2 {
        [u1, v1]
    } else if r < 0.3 {
        [rng.uniform(u0 - 50.0, u1 + 50.0), rng.uniform(v0 - 1.0, v1 + 1.0)]
    } else {
        [rng.uniform(u0, u1), rng.uniform(v0, v1)]
    }
}

/// A NURB parameter pair in (or near) `d`.
fn nurb_uv(rng: &mut Rng, d: &Extent2d) -> [f64; 2] {
    let (u0, u1) = (d.min().x, d.max().x);
    let (v0, v1) = (d.min().y, d.max().y);
    let r = rng.unit();
    if r < 0.1 {
        [u0, v0]
    } else if r < 0.2 {
        [u1, v1]
    } else if r < 0.25 {
        let du = (u1 - u0) * 0.1;
        let dv = (v1 - v0) * 0.1;
        [rng.uniform(u0 - du, u1 + du), rng.uniform(v0 - dv, v1 + dv)]
    } else {
        [rng.uniform(u0, u1), rng.uniform(v0, v1)]
    }
}

fn gen_curve_pair(rng: &mut Rng) -> OpIn {
    let c = rng.point3(-5.0, 5.0);
    let x = unit_vec(rng);
    let y = normalize(cross3(cross3(x, unit_vec(rng)), x));
    let tol = if rng.chance(0.5) { 1e-6 } else { 1e-3 };
    match rng.below(5) {
        0 => {
            // lines: same, reversed, shifted, or different
            let a = rng.point3(-5.0, 5.0);
            let b = rng.point3(-5.0, 5.0);
            let eps = if rng.chance(0.5) { tol * 0.1 } else { tol * 10.0 };
            let (a2, b2) = match rng.below(4) {
                0 => (a, b),
                1 => (b, a),
                2 => (add3(a, [eps, 0.0, 0.0]), add3(b, [0.0, eps, 0.0])),
                _ => (rng.point3(-5.0, 5.0), b),
            };
            OpIn::Coincidence {
                a: CurveIn::Line { a, b },
                a_dom: [0.0, 1.0],
                b: CurveIn::Line { a: a2, b: b2 },
                b_dom: if rng.chance(0.2) { [0.0, 0.5] } else { [0.0, 1.0] },
                tol,
            }
        }
        1 | 2 => {
            let r = rng.uniform(0.5, 4.0);
            let full = rng.chance(0.4);
            let d = if full {
                [0.0, 360.0]
            } else {
                let a = rng.uniform(-180.0, 90.0);
                [a, a + rng.uniform(20.0, 200.0)]
            };
            let (y2, d2, r2, c2) = match rng.below(5) {
                0 => (y, d, r, c),
                1 => (scale3(y, -1.0), [-d[1], -d[0]], r, c),
                2 => (y, d, r * (1.0 + 1e-7), c),
                3 => {
                    let sh = if d[1] + 10.0 <= 360.0 { 10.0 } else { -10.0 };
                    (y, [d[0] + sh, d[1] + sh], r, c)
                }
                _ => (y, d, r, add3(c, [0.0, 0.0, 1e-4])),
            };
            OpIn::Coincidence {
                a: CurveIn::Circle { c, x, y, d, r },
                a_dom: [d[0], d[1]],
                b: CurveIn::Circle {
                    c: c2,
                    x,
                    y: y2,
                    d: d2,
                    r: r2,
                },
                b_dom: [d2[0], d2[1]],
                tol,
            }
        }
        3 => {
            let d = [0.0, 180.0];
            OpIn::Coincidence {
                a: CurveIn::Ellipse {
                    c,
                    x,
                    y,
                    d,
                    rx: 2.0,
                    ry: 1.0,
                },
                a_dom: d,
                b: CurveIn::Ellipse {
                    c,
                    x,
                    y,
                    d,
                    rx: 2.0,
                    ry: 1.0 + 1e-3,
                },
                b_dom: d,
                tol,
            }
        }
        _ => {
            let d = [0.0, 90.0];
            OpIn::Coincidence {
                a: CurveIn::Circle { c, x, y, d, r: 1.0 },
                a_dom: d,
                b: CurveIn::Line {
                    a: c,
                    b: add3(c, x),
                },
                b_dom: [0.0, 1.0],
                tol,
            }
        }
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    if rng.chance(0.06) {
        return Inputs {
            cone: None,
            op: gen_curve_pair(rng),
        };
    }
    let c = gen_cone(rng);
    let k = Kernel::default();
    let built = rust_cone(&k, &c).ok();
    let (step_dom, nurb_dom, scale) = match &built {
        Some(s) => (
            s.rev().step_uv_domain(),
            s.rev().base().natural_uv_domain(),
            1.0 + s.rev().position().origin().max_dimension() + s.bot_radius().abs(),
        ),
        None => (
            Extent2d::from_bounds(0.0, 0.0, 360.0, 1.0),
            Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
            1.0,
        ),
    };
    let surface_point = |rng: &mut Rng| -> [f64; 3] {
        let Some(s) = &built else {
            return rng.point3(-5.0, 5.0);
        };
        let [u, v] = step_uv(rng, &step_dom);
        let p = s.evaluate_step_point(Point2::new(u, v)).unwrap_or(Vec3::ZERO);
        let jitter = if rng.chance(0.3) {
            scale3(rng.point3(-1.0, 1.0), scale * 1e-3)
        } else {
            [0.0, 0.0, 0.0]
        };
        add3([p.x, p.y, p.z], jitter)
    };
    // (0 and 14 build)
    let op = match rng.below(15) {
        1 => {
            let [u, v] = step_uv(rng, &step_dom);
            let point = rng.chance(0.2);
            let (hu, hv) = if rng.chance(0.75) {
                let k = rng.below(4) as usize;
                (k, k)
            } else {
                (rng.below(5) as usize, rng.below(5) as usize)
            };
            OpIn::EvalStep {
                u,
                v,
                hu,
                hv,
                point,
            }
        }
        2 => {
            let [u, v] = nurb_uv(rng, &nurb_dom);
            let (hu, hv) = if rng.chance(0.7) {
                let k = rng.below(4) as usize;
                (k, k)
            } else {
                (rng.below(3) as usize, rng.below(3) as usize)
            };
            OpIn::Eval {
                u,
                v,
                hu,
                hv,
                u_left: rng.chance(0.7),
                v_left: rng.chance(0.7),
                upper: rng.chance(0.2) && hu == hv,
                non_zero: rng.chance(0.7),
            }
        }
        3 => {
            let [u, v] = nurb_uv(rng, &nurb_dom);
            OpIn::EvalPoint { u, v }
        }
        4 => {
            let [u, v] = nurb_uv(rng, &nurb_dom);
            OpIn::ToStep { u, v }
        }
        5 => {
            let [u, v] = step_uv(rng, &step_dom);
            OpIn::ToNurbs { u, v }
        }
        6 => {
            let param = u8::from(rng.chance(0.5));
            let (lo, hi) = if param == 0 {
                (nurb_dom.min().x, nurb_dom.max().x)
            } else {
                (nurb_dom.min().y, nurb_dom.max().y)
            };
            let r = rng.unit();
            let iso = if r < 0.15 {
                lo
            } else if r < 0.3 {
                hi
            } else {
                rng.uniform(lo, hi)
            };
            let dom = if rng.chance(0.4) {
                let [a, b] = nurb_uv(rng, &nurb_dom);
                let [c2, d] = nurb_uv(rng, &nurb_dom);
                Some([a.min(c2), b.min(d), a.max(c2), b.max(d)])
            } else {
                None
            };
            OpIn::Iso {
                param,
                iso,
                dom,
                uv: rng.chance(0.3),
            }
        }
        7 => {
            let p = if rng.chance(0.15) {
                let o = built
                    .as_ref()
                    .map_or(Vec3::ZERO, |s| s.rev().position().origin());
                [o.x, o.y, o.z]
            } else {
                surface_point(rng)
            };
            OpIn::IsoCircle {
                p,
                tol: if rng.chance(0.2) { 1e-9 * scale } else { 1e-6 * scale },
            }
        }
        8 => {
            let p = if rng.chance(0.1) {
                let o = built
                    .as_ref()
                    .and_then(|s| s.apex(&k).ok())
                    .map_or(Vec3::ZERO, |a| a.point);
                [o.x, o.y, o.z]
            } else {
                surface_point(rng)
            };
            let guess = if rng.chance(0.4) {
                Some(step_uv(rng, &step_dom))
            } else {
                None
            };
            OpIn::Inversion {
                p,
                tol: if rng.chance(0.2) { 1e-9 * scale } else { 1e-6 * scale },
                guess,
            }
        }
        9 => OpIn::Positive {
            p: if rng.chance(0.5) {
                surface_point(rng)
            } else {
                scale3(rng.point3(-20.0, 20.0), scale)
            },
        },
        10 => {
            // (a reversed sweep makes upstream loop forever: the FFI rejects it)
            let (a, b) = angles(rng);
            let (a, b) = (a.min(b), a.max(b));
            let v0 = rng.uniform(-5.0, 5.0) * scale;
            let v1 = v0 + rng.uniform(0.1, 10.0) * scale;
            OpIn::Adjust { dom: [a, v0, b, v1] }
        }
        11 => {
            let r = rng.unit();
            let (o, x, y) = if r < 0.3 {
                ([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])
            } else {
                frame(rng, 1.0, false)
            };
            let scale = match rng.below(4) {
                0 => None,
                1 => Some([1.0, 1.0, 1.0]),
                2 => {
                    let s = rng.uniform(0.2, 5.0);
                    Some([s, s, s])
                }
                _ => Some([1.0, 2.0, 1.0]),
            };
            OpIn::Transform {
                origin: o,
                x,
                y,
                scale,
            }
        }
        12 => {
            let other = if rng.chance(0.5) {
                c.clone()
            } else {
                let tweak = rng.chance(0.5);
                if tweak {
                    let mut o = c.clone();
                    if let ModeIn::ConeBounded { top, .. } = &mut o.mode {
                        *top += 1e-14;
                    } else if let ModeIn::CylUnbounded { radius, .. } = &mut o.mode {
                        *radius *= 1.5;
                    } else {
                        o.origin = add3(o.origin, [1e-3, 0.0, 0.0]);
                    }
                    o
                } else {
                    gen_cone(rng)
                }
            };
            OpIn::Eq {
                other: Box::new(other),
            }
        }
        13 => OpIn::Copy {},
        _ => OpIn::Build {},
    };
    Inputs { cone: Some(c), op }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.cone_p1"
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

    fn op_name(op: &OpIn) -> &'static str {
        match op {
            OpIn::Build {} => "build",
            OpIn::EvalStep { .. } => "eval_step",
            OpIn::Eval { .. } => "eval",
            OpIn::EvalPoint { .. } => "eval_point",
            OpIn::ToStep { .. } => "to_step",
            OpIn::ToNurbs { .. } => "to_nurbs",
            OpIn::Iso { .. } => "iso",
            OpIn::IsoCircle { .. } => "iso_circle",
            OpIn::Inversion { .. } => "inversion",
            OpIn::Positive { .. } => "positive",
            OpIn::Adjust { .. } => "adjust",
            OpIn::Transform { .. } => "transform",
            OpIn::Eq { .. } => "eq",
            OpIn::Copy {} => "copy",
            OpIn::Coincidence { .. } => "coincidence",
        }
    }

    /// The generators reach a successful construction and a successful run of every op
    /// on the Rust side.
    #[test]
    fn generated_cases_cover_every_op() {
        let mut tally: BTreeMap<(String, String), usize> = BTreeMap::new();
        for seed in 0..1500u64 {
            let mut rng = Rng::new(0x5eed_0000 + seed);
            let i = gen_case(&mut rng);
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
            "eval",
            "eval_point",
            "to_step",
            "to_nurbs",
            "iso",
            "iso_circle",
            "inversion",
            "positive",
            "eq",
            "copy",
        ] {
            let ok = tally.iter().any(|((o, k), _)| {
                o == op
                    && (k == "build:Some(1000.0) status:Some(1000.0)"
                        || k == "build:Some(1000.0) status:None")
            });
            assert!(ok, "{op} never succeeds");
        }
        let ok = tally
            .iter()
            .any(|((o, k), _)| o == "coincidence" && k == "build:None status:Some(1000.0)");
        assert!(ok, "coincidence never succeeds");
    }

    /// Most fixture cases compare real values (a successful construction and op).
    #[test]
    fn fixtures_mostly_compare_values() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/surface.cone_p1/cases.json");
        let text = std::fs::read_to_string(path).expect("fixtures");
        let v: Value = serde_json::from_str(&text).expect("json");
        let cases = v["cases"].as_array().expect("cases");
        let mut ok = 0;
        for c in cases {
            let i: Inputs = serde_json::from_value(c["inputs"].clone()).expect("inputs");
            let out = rust(&i).expect("harness");
            let b = out.values.get("build").and_then(|v| v.first()).copied();
            let st = out.values.get("status").and_then(|v| v.first()).copied();
            eprintln!(
                "{} build:{b:?} status:{st:?} deferred:{}",
                c["name"],
                out.deferred.is_some()
            );
            if out.deferred.is_none()
                && matches!(b, Some(1000.0) | None)
                && matches!(st, Some(1000.0) | None)
            {
                ok += 1;
            }
        }
        assert!(ok * 10 >= cases.len() * 6, "{ok} of {}", cases.len());
    }
}
