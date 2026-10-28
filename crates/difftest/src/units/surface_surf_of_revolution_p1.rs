//! Runner for unit `surface.surf_of_revolution_p1`: `SmSurfOfRevolution` part 1
//! (construction through `CreateCanonical` or the constructor with `MakeNurb`,
//! `operator==`, iso curves, directrices, generators, the NURB to STEP conversion,
//! STEP evaluation, the plane intersection's special cases, the start-plane transform,
//! the small queries) and `sm_TestForIsoCircles`.
//!
//! C++: `solidcore_ffi::units::surface_surf_of_revolution_p1` (upstream
//! `source/SMLib/src/SmSurfOfRevolution.cpp`). Rust:
//! `solidcore::surface::surf_of_revolution::p1`.
//!
//! A case is `surf` (generator, construction, `oob`, `swap`) plus `op` and its
//! parameters, or the static `iso_circles` op on a B-spline surface. Both sides
//! produce the same flat record (layout in the FFI module docs), split into `build`
//! (the construction status), `status` (the op status) and `out` (the rest). Statuses
//! are `0` (`SM_SUCCESS`) or the `SmStatus`; `-1` on the Rust side for an error
//! upstream cannot produce.
//!
//! Deferred cases: `ConvertUVFromNURBSToSTEP` without exact polar conversion
//! (`SmSTEPSurface`, `surface.surface_p1`) and `IntersectWithPlane` of a planar
//! generator (the plane's pseudo box needs `SmSurface::GetFace`, `surface.surface_p1`;
//! the rest of the body `SmPlane::TrimCurveToPlaneDomain`, `surface.plane_p2`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::hyperbola::Hyperbola;
use solidcore::curve::line::Line;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tol::approx_tol3d;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surf_of_revolution::{test_for_iso_circles, SurfOfRevolution};
use solidcore::surface::surface::p2::{B_SPLINE_SURFACE_TYPE, SURF_OF_REVOLUTION_TYPE, TYPE};
use solidcore::SolidError;
use solidcore_ffi::units::surface_surf_of_revolution_p1 as ffi;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 600;

/// `surface.surf_of_revolution_p1` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

/// An inline 3D curve: control points as stored (`w = NL_NOW` when not rational,
/// written as `null`), degree, knots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct InlineCrv {
    pub(crate) cpts: Vec<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) weights: Option<Vec<f64>>,
    pub(crate) p: usize,
    pub(crate) u: Vec<f64>,
}

/// The generator.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum GenIn {
    BSpline(InlineCrv),
    Line { a: [f64; 3], b: [f64; 3] },
    Circle {
        c: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        d: [f64; 2],
        r: f64,
    },
    /// `SmHyperbola(c, x, y, [d0, d1], a, b, 3)` (fixtures only; the generator never
    /// draws one).
    Hyperbola {
        c: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        d: [f64; 2],
        a: f64,
        b: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub(crate) enum BuildIn {
    Canonical {
        point: [f64; 3],
        axis: [f64; 3],
    },
    Ctor {
        origin: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        domain: [f64; 4],
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

/// An inline surface: control net as stored (rows in u), degrees, knots.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InlineSrf {
    net: Vec<Vec<[f64; 4]>>,
    p: usize,
    q: usize,
    u: Vec<f64>,
    v: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum SrfIn {
    /// An upstream `.sms` file (absolute path after fixture expansion).
    File { sms: String },
    Inline(InlineSrf),
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
    Iso {
        param: u8,
        iso: f64,
        #[serde(default)]
        dom: Option<[f64; 4]>,
    },
    Directrix {
        t: f64,
    },
    Generator {
        angle: f64,
    },
    StartPlane {
        p: [f64; 3],
        tol: f64,
        #[serde(default)]
        snap: bool,
    },
    Eq {
        other: SurfIn,
    },
    Plane {
        origin: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        dom: [f64; 4],
        #[serde(default)]
        tol: Option<f64>,
    },
    IsoCircles {
        srf: SrfIn,
        param: u8,
        tol: f64,
        ang: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    surf: Option<SurfIn>,
    #[serde(flatten)]
    op: OpIn,
}

// ---- file reader ----------------------------------------------------------------------

fn numbers(path: &str) -> Result<Vec<f64>, HarnessError> {
    let text = std::fs::read_to_string(path).map_err(|e| harness(format!("{path}: {e}")))?;
    text.split_whitespace()
        .map(|t| t.parse::<f64>().map_err(|e| harness(format!("{path}: {e}"))))
        .collect()
}

fn int(path: &str, v: f64) -> Result<usize, HarnessError> {
    if v.fract() != 0.0 || !(0.0..=1e6).contains(&v) {
        return Err(harness(format!("{path}: bad count {v}")));
    }
    Ok(v as usize)
}

/// Reads an upstream `.sms` text file: `n m`, `p q`, rational flag, `(n+1)(m+1)`
/// points (`x y z`, or `wx wy wz w` when rational, v index fastest), `n+p+2` u knots,
/// `m+q+2` v knots.
fn read_sms(path: &str) -> Result<InlineSrf, HarnessError> {
    let v = numbers(path)?;
    let mut it = v.into_iter();
    let mut num = || it.next().ok_or_else(|| harness(format!("{path}: truncated")));
    let n = int(path, num()?)?;
    let m = int(path, num()?)?;
    let p = int(path, num()?)?;
    let q = int(path, num()?)?;
    let rat = num()? != 0.0;
    let mut net = Vec::new();
    for _ in 0..=n {
        let mut row = Vec::new();
        for _ in 0..=m {
            let (x, y, z) = (num()?, num()?, num()?);
            let w = if rat { num()? } else { NOW };
            row.push([x, y, z, w]);
        }
        net.push(row);
    }
    let u = (0..n + p + 2).map(|_| num()).collect::<Result<Vec<_>, _>>()?;
    let v = (0..m + q + 2).map(|_| num()).collect::<Result<Vec<_>, _>>()?;
    Ok(InlineSrf { net, p, q, u, v })
}

fn srf(s: &SrfIn) -> Result<InlineSrf, HarnessError> {
    match s {
        SrfIn::File { sms } => read_sms(sms),
        SrfIn::Inline(i) => Ok(i.clone()),
    }
}

// ---- C++ side -------------------------------------------------------------------------

fn cpts4(c: &InlineCrv) -> Result<Vec<[f64; 4]>, HarnessError> {
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

fn ffi_gen(g: &GenIn) -> Result<ffi::Gen, HarnessError> {
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
        GenIn::Hyperbola { c, x, y, d, a, b } => ffi::Gen::Hyperbola {
            c: *c,
            x: *x,
            y: *y,
            d: *d,
            a: *a,
            b: *b,
        },
    })
}

pub(crate) fn ffi_surf(s: &SurfIn) -> Result<ffi::Surf, HarnessError> {
    Ok(ffi::Surf {
        gen: ffi_gen(&s.gen)?,
        build: match &s.build {
            BuildIn::Canonical { point, axis } => ffi::Build::Canonical {
                point: *point,
                axis: *axis,
            },
            BuildIn::Ctor {
                origin,
                x,
                y,
                domain,
            } => ffi::Build::Ctor {
                origin: *origin,
                x: *x,
                y: *y,
                domain: *domain,
            },
        },
        oob: s.oob,
        swap: s.swap,
    })
}

fn ffi_srf(s: &InlineSrf) -> ffi::SurfaceData {
    ffi::SurfaceData {
        net: s.net.clone(),
        p: s.p,
        q: s.q,
        u: s.u.clone(),
        v: s.v.clone(),
    }
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let flat = if let OpIn::IsoCircles {
        srf: s,
        param,
        tol,
        ang,
    } = &i.op
    {
        ffi::iso_circles(&ffi_srf(&srf(s)?), *param, *tol, *ang)
    } else {
        let s = i.surf.as_ref().ok_or_else(|| harness("missing surf"))?;
        let op = match &i.op {
            OpIn::Build {} => ffi::Op::Build,
            OpIn::EvalStep { u, v, k, point } => ffi::Op::EvalStep {
                u: *u,
                v: *v,
                k: *k,
                point: *point,
            },
            OpIn::ToStep { u, v } => ffi::Op::ToStep { u: *u, v: *v },
            OpIn::Iso { param, iso, dom } => ffi::Op::Iso {
                param: *param,
                iso: *iso,
                dom: *dom,
            },
            OpIn::Directrix { t } => ffi::Op::Directrix { t: *t },
            OpIn::Generator { angle } => ffi::Op::Generator { angle: *angle },
            OpIn::StartPlane { p, tol, snap } => ffi::Op::StartPlane {
                p: *p,
                tol: *tol,
                snap: *snap,
            },
            OpIn::Eq { other } => ffi::Op::Eq {
                other: Box::new(ffi_surf(other)?),
            },
            OpIn::Plane {
                origin,
                x,
                y,
                dom,
                tol,
            } => ffi::Op::Plane {
                origin: *origin,
                x: *x,
                y: *y,
                dom: *dom,
                tol: *tol,
            },
            OpIn::IsoCircles { .. } => return Err(harness("unreachable")),
        };
        ffi::surface_op(&ffi_surf(s)?, &op)
    }
    .map_err(|e| harness(e.to_string()))?;
    to_output(i, &flat)
}

/// Splits a record into `build`, `status` and `out`.
fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut out = Output::new();
    let mut rest = flat;
    if !matches!(i.op, OpIn::IsoCircles { .. }) {
        let (b, r) = rest
            .split_first()
            .ok_or_else(|| harness("empty record"))?;
        out = out.values("build", vec![*b]);
        rest = r;
        if *b != 0.0 {
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

/// The `SmStatus` of a Rust error; `-1` for one upstream cannot return.
pub(crate) fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

fn rust_gen(k: &Kernel, g: &GenIn) -> Result<Curve, HarnessError> {
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
    })
}

/// Builds the surface; `Ok(Err(e))` when the construction fails.
pub(crate) fn rust_surf(
    k: &Kernel,
    s: &SurfIn,
) -> Result<solidcore::Result<SurfOfRevolution>, HarnessError> {
    rust_surf_made(k, s, true)
}

/// [`rust_surf`]; `make` false leaves a constructor-mode surface without its NURB.
fn rust_surf_made(
    k: &Kernel,
    s: &SurfIn,
    make: bool,
) -> Result<solidcore::Result<SurfOfRevolution>, HarnessError> {
    let gen = rust_gen(k, &s.gen)?;
    let built = match &s.build {
        BuildIn::Canonical { point, axis } => {
            SurfOfRevolution::create_canonical(k, gen, v3(*point), v3(*axis), s.swap)
        }
        BuildIn::Ctor {
            origin,
            x,
            y,
            domain,
        } => {
            let [a, b, c, d] = *domain;
            SurfOfRevolution::new(
                k,
                gen,
                v3(*origin),
                v3(*x),
                v3(*y),
                Extent2d::from_bounds(a, b, c, d),
                s.swap,
            )
            .and_then(|mut r| {
                if make {
                    r.make_nurb(k)?;
                }
                Ok(r)
            })
        }
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

pub(crate) fn put_state(k: &Kernel, o: &mut Vec<f64>, s: &SurfOfRevolution) -> solidcore::Result<()> {
    o.extend([
        flag(s.is_kind_of(SURF_OF_REVOLUTION_TYPE)?),
        flag(s.is_kind_of(B_SPLINE_SURFACE_TYPE)?),
        flag(s.is_kind_of(TYPE)?),
        flag(s.is_bounded()?),
        flag(s.swap_uv()),
        flag(s.planar_generator()),
    ]);
    let pos = s.position();
    push_v(o, pos.origin());
    push_v(o, pos.x_axis());
    push_v(o, pos.y_axis());
    let d = s.step_uv_domain();
    o.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
    o.extend([s.start_angle_deg(), s.end_angle_deg()]);
    let mx = s.max_analytic_domain(k)?;
    o.extend([mx.min().x, mx.min().y, mx.max().x, mx.max().y]);
    let (gd, gp) = s.gen_dir_param_extent();
    o.extend([gd.min(), gd.max(), f64::from(gp as u8)]);
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

/// `SC_ERR_ARG` (`harness/cpp/shim/sc_shim.h`): the shim rejected the arguments.
pub(crate) const SC_ERR_ARG: f64 = -1002.0;

pub(crate) fn param_of(p: u8) -> SurfParamType {
    if p == 0 {
        SurfParamType::U
    } else {
        SurfParamType::V
    }
}

/// Runs the op on the Rust surface, appending its status and outputs.
fn rust_op(
    k: &Kernel,
    s: &SurfOfRevolution,
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
        OpIn::ToStep { u, v } => s
            .convert_uv_from_nurbs_to_step(k, Point2::new(*u, *v))
            .map(|p| o.extend([0.0, p.x, p.y])),
        OpIn::Iso { param, iso, dom } => {
            let dom = dom.map(|[a, b, c, d]| Extent2d::from_bounds(a, b, c, d));
            s.create_iso_parametric_curve(k, param_of(*param), *iso, dom.as_ref())
                .map(|c| {
                    o.extend([0.0, c.max_gap_3d]);
                    put_curve(o, Some(&c.curve));
                })
        }
        OpIn::Directrix { t } => s.create_directrix_at_generator_param(k, *t).map(|d| {
            o.extend([0.0, d.radius]);
            push_v(o, d.axis_point);
            put_curve(o, Some(&d.directrix));
        }),
        OpIn::Generator { angle } => s.create_generator_from_angle(k, *angle).map(|c| {
            o.push(0.0);
            put_curve(o, Some(&c));
        }),
        OpIn::StartPlane { p, tol, snap } => s
            .transform_point_to_start_plane(v3(*p), *tol, *snap)
            .map(|r| {
                o.push(0.0);
                push_v(o, r.point);
                o.push(r.angles_deg.len() as f64);
                o.extend(r.angles_deg.iter().take(2));
                o.push(flag(r.inside));
            }),
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
        OpIn::Plane {
            origin,
            x,
            y,
            dom,
            tol,
        } => {
            // the shim: SmPlane::CreateCanonical(SmAxis2Placement(o, x, y)), SC_ERR_ARG
            // when it fails
            let frame = Axis2Placement::new(v3(*origin), v3(*x), v3(*y));
            let plane = match Plane::create_canonical(&frame) {
                Err(e) if e.is_unported() => return Ok(Err(e)),
                Err(_) => {
                    o.push(SC_ERR_ARG);
                    return Ok(Ok(()));
                }
                Ok(p) => p,
            };
            let [a, b, c, d] = *dom;
            // the shim passes `SmApproxTol3d(tol)`: |tol|, SM_USE_DEFAULT -> zone tol / 2
            let tol = tol.map(|t| approx_tol3d(k, t));
            s.intersect_with_plane(k, &plane, &Extent2d::from_bounds(a, b, c, d), tol)
                .map(|(more, x)| {
                    o.extend([0.0, flag(more), x.curves_3d.len() as f64]);
                    for c in &x.curves_3d {
                        put_curve(o, Some(c));
                    }
                    o.extend(x.curve_types.iter().map(|t| f64::from(*t as u8)));
                    o.extend_from_slice(&x.deviations);
                })
        }
        OpIn::IsoCircles { .. } => return Err(harness("unreachable")),
    };
    Ok(r)
}

fn rust_iso_circles(
    k: &Kernel,
    s: &InlineSrf,
    param: u8,
    tol: f64,
    ang: f64,
) -> Result<Vec<f64>, HarnessError> {
    let net = s
        .net
        .iter()
        .map(|r| r.iter().map(|&[x, y, z, w]| CPoint::new(x, y, z, w)).collect())
        .collect();
    let sur = NurbSurface::new(net, s.p, s.q, s.u.clone(), s.v.clone())
        .map_err(|e| harness(format!("surface input: {e}")))?;
    let bss = BSplineSurface::from_nurb(sur, false);
    let mut o = Vec::new();
    match test_for_iso_circles(k, &bss, param_of(param), tol, ang) {
        Err(e) if e.is_unported() => return Err(harness(format!("unported: {e}"))),
        Err(e) => o.push(status(&e)),
        Ok(r) => {
            o.push(0.0);
            o.extend(
                [
                    r.found_circles,
                    r.parallel_planes,
                    r.same_plane,
                    r.coaxial_centers,
                    r.common_axis,
                    r.planar_gen_curve,
                ]
                .map(flag),
            );
            let nan = f64::NAN;
            o.push(r.radius.unwrap_or(nan));
            let a = r.angles.map_or([nan, nan], |a| [a.min(), a.max()]);
            o.extend(a);
            let nanv = Vec3::new(nan, nan, nan);
            match r.reference_frame {
                Some(f) => {
                    push_v(&mut o, f.origin());
                    push_v(&mut o, f.x_axis());
                    push_v(&mut o, f.y_axis());
                }
                None => {
                    for _ in 0..3 {
                        push_v(&mut o, nanv);
                    }
                }
            }
            push_v(&mut o, r.common_line_point.unwrap_or(nanv));
            push_v(&mut o, r.common_line_vec.unwrap_or(nanv));
        }
    }
    Ok(o)
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    if let OpIn::IsoCircles {
        srf: s,
        param,
        tol,
        ang,
    } = &i.op
    {
        let flat = rust_iso_circles(&k, &srf(s)?, *param, *tol, *ang)?;
        return to_output(i, &flat);
    }
    let s = i.surf.as_ref().ok_or_else(|| harness("missing surf"))?;
    let mut o = Vec::new();
    match rust_surf(&k, s)? {
        Err(e) => {
            if let Some(out) = Output::unported(&e) {
                return Ok(out);
            }
            o.push(status(&e));
        }
        Ok(surf) => {
            o.push(0.0);
            let at = o.len();
            if let Err(e) = rust_op(&k, &surf, &i.op, &mut o)? {
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
fn knots(rng: &mut Rng, n: usize, p: usize, a: f64, b: f64) -> Vec<f64> {
    let inner = n - p;
    let mut ts: Vec<f64> = (0..inner)
        .map(|i| (i as f64 + 1.0) / (inner as f64 + 1.0))
        .collect();
    if rng.chance(0.4) {
        // jitter, keeping order
        for t in &mut ts {
            *t += rng.uniform(-0.2, 0.2) / (inner as f64 + 1.0);
        }
    }
    let mut u = vec![a; p + 1];
    u.extend(ts.iter().map(|t| a + t * (b - a)));
    u.extend(vec![b; p + 1]);
    u
}

/// A frame: origin, unit X, unit Y, unit Z (the axis).
pub(crate) type Frame = ([f64; 3], [f64; 3], [f64; 3], [f64; 3]);

pub(crate) fn frame(rng: &mut Rng) -> Frame {
    let origin = if rng.chance(0.3) {
        [0.0, 0.0, 0.0]
    } else {
        rng.point3(-5.0, 5.0)
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

/// A point at radius `r`, height `h` of the frame's X/Z half plane, out of plane by `off`.
pub(crate) fn in_frame(f: &Frame, r: f64, h: f64, off: f64) -> [f64; 3] {
    let (o, x, y, z) = f;
    add(add(add(*o, *x, r), *z, h), *y, off)
}

pub(crate) fn gen_generator(rng: &mut Rng, f: &Frame, planar: bool) -> GenIn {
    let off = |rng: &mut Rng| if planar { 0.0 } else { rng.uniform(-1.0, 1.0) };
    match rng.below(10) {
        0..=5 => {
            let p = 1 + rng.below(3) as usize;
            let n = p + rng.below(5) as usize;
            let mut cpts = Vec::new();
            let mut h = rng.uniform(-2.0, 0.0);
            for i in 0..=n {
                let mut r = rng.uniform(0.3, 3.0);
                if i == 0 && rng.chance(0.15) {
                    r = 0.0; // start on the axis
                }
                h += rng.uniform(0.2, 1.0);
                let o = off(rng);
                cpts.push(in_frame(f, r, h, o));
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
        6..=7 if rng.chance(0.35) => {
            // a line parallel (or anti-parallel) to the axis: a cylinder, whose sweep
            // circle CreateSurfOfRevolution reparametrises with arc length
            let r = rng.uniform(0.2, 3.0);
            let o = off(rng);
            let (h0, h1) = (rng.uniform(-2.0, 0.0), rng.uniform(0.5, 3.0));
            let (h0, h1) = if rng.chance(0.3) { (h1, h0) } else { (h0, h1) };
            GenIn::Line {
                a: in_frame(f, r, h0, o),
                b: in_frame(f, r, h1, o),
            }
        }
        6..=7 => {
            let o1 = off(rng);
            let o2 = off(rng);
            let r0 = if rng.chance(0.15) { 0.0 } else { rng.uniform(0.2, 3.0) };
            GenIn::Line {
                a: in_frame(f, r0, rng.uniform(-2.0, 0.0), o1),
                b: in_frame(f, rng.uniform(0.2, 3.0), rng.uniform(0.5, 3.0), o2),
            }
        }
        _ => {
            // a circle in the X/Z plane (or tilted), centred off the axis
            let (o, x, y, z) = f;
            let c = add(add(*o, *x, rng.uniform(1.5, 4.0)), *z, rng.uniform(-1.0, 1.0));
            let (cx, cy) = if planar {
                (*x, *z)
            } else {
                (normalize(add(*x, *y, 0.3)), *z)
            };
            let _ = y;
            let d0 = rng.uniform(-180.0, 90.0);
            let len = rng.uniform(30.0, 270.0);
            GenIn::Circle {
                c,
                x: cx,
                y: cy,
                d: [d0, (d0 + len).min(360.0)],
                r: rng.uniform(0.3, 1.2),
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
        GenIn::Circle { d, .. } | GenIn::Hyperbola { d, .. } => *d,
    }
}

pub(crate) fn gen_surf(rng: &mut Rng) -> SurfIn {
    let f = frame(rng);
    let planar = rng.chance(0.6);
    let gen = gen_generator(rng, &f, planar);
    let (o, x, y, z) = f;
    let build = if rng.chance(0.65) {
        let axis = if rng.chance(0.3) {
            add([0.0; 3], z, rng.uniform(0.5, 3.0))
        } else {
            z
        };
        BuildIn::Canonical { point: o, axis }
    } else {
        let [v0, v1] = gen_step_ivl(&gen);
        let (u0, u1) = match rng.below(4) {
            0 => (0.0, 360.0),
            1 => {
                let a = rng.uniform(-360.0, 0.0);
                (a, a + rng.uniform(10.0, 360.0))
            }
            _ => {
                let a = rng.uniform(0.0, 200.0);
                (a, (a + rng.uniform(10.0, 160.0)).min(360.0))
            }
        };
        BuildIn::Ctor {
            origin: o,
            x,
            y,
            domain: [u0, v0, u1, v1],
        }
    };
    SurfIn {
        gen,
        build,
        oob: rng.chance(0.2),
        swap: rng.chance(0.4),
    }
}

fn gen_iso_circles(rng: &mut Rng, k: &Kernel) -> Option<OpIn> {
    let srf = if rng.chance(0.75) {
        // a NURB surface of revolution
        let f = frame(rng);
        let planar = rng.chance(0.7);
        let GenIn::BSpline(c) = gen_generator(rng, &f, planar) else {
            return None;
        };
        let pw = cpts4(&c)
            .ok()?
            .into_iter()
            .map(|[x, y, z, w]| CPoint::new(x, y, z, w))
            .collect();
        let cur = NurbCurve::new(pw, c.p, c.u.clone()).ok()?;
        let gen = BSplineCurve::from_nurb(cur, 3, false);
        let (o, _, _, z) = f;
        let ang = if rng.chance(0.5) { 360.0 } else { rng.uniform(20.0, 359.0) };
        let s = BSplineSurface::create_surf_of_revolution(
            k,
            &gen,
            v3(o),
            v3(z),
            ang,
            rng.chance(0.4),
            None,
        )
        .ok()?;
        let sur = s.gw_nurb()?;
        let ix = sur.indices();
        let rows: Vec<Vec<[f64; 4]>> = sur
            .rows()
            .map(|r| r.iter().map(|p| [p.x, p.y, p.z, p.w]).collect())
            .collect();
        let _ = ix;
        InlineSrf {
            net: rows,
            p: ix.p,
            q: ix.q,
            u: sur.u_knots().to_vec(),
            v: sur.v_knots().to_vec(),
        }
    } else {
        // a random bicubic / bilinear patch
        let p = 1 + rng.below(3) as usize;
        let q = 1 + rng.below(3) as usize;
        let n = p + rng.below(3) as usize;
        let m = q + rng.below(3) as usize;
        let net = (0..=n)
            .map(|i| {
                (0..=m)
                    .map(|j| {
                        let [dx, dy, dz] = rng.point3(-0.3, 0.3);
                        [i as f64 + dx, j as f64 + dy, dz, NOW]
                    })
                    .collect()
            })
            .collect();
        InlineSrf {
            net,
            p,
            q,
            u: knots(rng, n, p, 0.0, 1.0),
            v: knots(rng, m, q, 0.0, 1.0),
        }
    };
    let _ = k;
    let tol = *rng.choose(&[1e-9, 1e-7, 1e-5, 1e-3])?;
    let ang = *rng.choose(&[1e-9, 1e-6, 1e-3, 0.1])?;
    Some(OpIn::IsoCircles {
        srf: SrfIn::Inline(srf),
        param: rng.below(2) as u8,
        tol,
        ang,
    })
}

fn gen_case(rng: &mut Rng) -> Option<Inputs> {
    let k = Kernel::default();
    if rng.chance(0.12) {
        return Some(Inputs {
            surf: None,
            op: gen_iso_circles(rng, &k)?,
        });
    }
    let s = gen_surf(rng);
    // the Rust surface sizes the parameters (both sides get the same numbers)
    let built = rust_surf(&k, &s).ok()?.ok();
    let nurb = built.as_ref().map(|b| b.base().natural_uv_domain());
    let step = built.as_ref().map(SurfOfRevolution::step_uv_domain);
    let pick = |rng: &mut Rng, lo: f64, hi: f64| -> f64 {
        match rng.below(10) {
            0 => lo,
            1 => hi,
            2 => lo - rng.uniform(0.0, 0.5) * (hi - lo),
            _ => lo + rng.unit() * (hi - lo),
        }
    };
    let nd = nurb.unwrap_or(Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0));
    let sd = step.unwrap_or(Extent2d::from_bounds(0.0, 0.0, 360.0, 1.0));
    let op = match rng.below(11) {
        0 => OpIn::Build {},
        1 | 2 => OpIn::EvalStep {
            u: pick(rng, sd.min().x, sd.max().x),
            v: pick(rng, sd.min().y, sd.max().y),
            k: rng.below(4) as usize,
            point: rng.chance(0.2),
        },
        3 => OpIn::ToStep {
            u: pick(rng, nd.min().x, nd.max().x),
            v: pick(rng, nd.min().y, nd.max().y),
        },
        4 | 5 => {
            let param = rng.below(2) as u8;
            let (lo, hi) = if param == 0 {
                (nd.min().x, nd.max().x)
            } else {
                (nd.min().y, nd.max().y)
            };
            let dom = rng.chance(0.3).then(|| {
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
        6 => {
            let g = built.as_ref().and_then(|b| b.gen_curve()).map_or(
                Extent1d::from_min_max(0.0, 1.0),
                Curve::natural_interval,
            );
            OpIn::Directrix {
                t: pick(rng, g.min(), g.max()),
            }
        }
        7 => OpIn::Generator {
            angle: if rng.chance(0.2) {
                0.0
            } else {
                rng.uniform(-400.0, 400.0)
            },
        },
        8 => {
            let p = match (&built, rng.below(3)) {
                (Some(b), 0 | 1) => {
                    let uv = Point2::new(
                        sd.min().x + rng.unit() * (sd.max().x - sd.min().x),
                        sd.min().y + rng.unit() * (sd.max().y - sd.min().y),
                    );
                    let p = b.evaluate_step_point(&k, uv).ok()?;
                    let [dx, dy, dz] = rng.point3(-1e-4, 1e-4);
                    [p.x + dx, p.y + dy, p.z + dz]
                }
                (Some(b), _) => {
                    let o = b.position().origin();
                    let z = b.position().z_axis();
                    let t = rng.uniform(-2.0, 2.0);
                    [o.x + t * z.x, o.y + t * z.y, o.z + t * z.z]
                }
                _ => rng.point3(-5.0, 5.0),
            };
            OpIn::StartPlane {
                p,
                tol: *rng.choose(&[1e-9, 1e-6, 1e-3, 0.1])?,
                snap: rng.chance(0.5),
            }
        }
        9 => {
            let mut other = s.clone();
            match rng.below(4) {
                0 => {}
                1 => other.swap = !other.swap,
                2 => {
                    if let BuildIn::Canonical { point, .. } = &mut other.build {
                        *point = add(*point, [1e-3, 0.0, 0.0], 1.0);
                    }
                }
                _ => {
                    if let GenIn::BSpline(c) = &mut other.gen {
                        if let Some(p) = c.cpts.last_mut() {
                            *p = add(*p, [0.0, 0.0, 1e-3], 1.0);
                        }
                    }
                }
            }
            OpIn::Eq { other }
        }
        _ => {
            let (o, z) = built
                .as_ref()
                .map_or(([0.0; 3], [0.0, 0.0, 1.0]), |b| {
                    let o = b.position().origin();
                    let z = b.position().z_axis();
                    ([o.x, o.y, o.z], [z.x, z.y, z.z])
                });
            let t = unit_vec(rng);
            let (x, y) = match rng.below(3) {
                // plane through the axis
                0 => (z, normalize(cross3(z, t))),
                // plane normal to the axis
                1 => {
                    let x = normalize(cross3(z, t));
                    (x, cross3(z, x))
                }
                _ => (unit_vec(rng), normalize(cross3(t, unit_vec(rng)))),
            };
            OpIn::Plane {
                origin: add(o, z, rng.uniform(-1.0, 1.0)),
                x,
                y,
                dom: [-10.0, -10.0, 10.0, 10.0],
                tol: rng.chance(0.5).then(|| rng.uniform(1e-6, 1e-3)),
            }
        }
    };
    Some(Inputs { surf: Some(s), op })
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.surf_of_revolution_p1"
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

/// The constructor-built surface of a `common_const_make` case.
fn const_make_input(value: Value) -> Result<SurfIn, HarnessError> {
    let s: SurfIn = serde_json::from_value(value).map_err(|e| harness(e.to_string()))?;
    if !matches!(s.build, BuildIn::Ctor { .. }) {
        return Err(harness("const_make: constructor build required"));
    }
    Ok(s)
}

/// The native side of a surface of revolution `common_const_make` case.
pub(super) fn const_make_cpp(value: Value) -> Result<Output, HarnessError> {
    let s = const_make_input(value)?;
    let flat = ffi::const_make(&ffi_surf(&s)?).map_err(|e| harness(e.to_string()))?;
    super::surface_cone_p1::const_make_nurb_cpp(&flat)
}

/// The Rust side: the surface as its constructor leaves it (no NURB), then the shared
/// const-query, `Copy`, `==` and `Transform` sequence.
pub(super) fn const_make_rust(value: Value) -> Result<Output, HarnessError> {
    let s = const_make_input(value)?;
    let k = Kernel::default();
    let built = rust_surf_made(&k, &s, false)?.map(solidcore::surface::surface::p2::Surface::from);
    super::surface_cone_p1::const_make_nurb_rust(&k, built)
}

/// `common_const_make` cases for the surface of revolution: lines, circles and a B-spline
/// generator, including ones that touch or cross the axis (an apex, a tangent circle), a
/// partial sweep and the swap flag.
pub(super) fn const_make_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    let o = [0.5, -1.0, 2.0];
    let at = |p: [f64; 3]| [p[0] + o[0], p[1] + o[1], p[2] + o[2]];
    let circle = |c: [f64; 3], d: [f64; 2], r: f64| GenIn::Circle { c: at(c), x: [1.0, 0.0, 0.0], y: [0.0, 0.0, 1.0], d, r };
    let line = |a: [f64; 3], b: [f64; 3]| GenIn::Line { a: at(a), b: at(b) };
    let arc = GenIn::BSpline(InlineCrv {
        cpts: vec![at([1.0, 0.0, 0.0]), at([2.0, 0.0, 1.0]), at([1.0, 0.0, 2.0])],
        weights: Some(vec![1.0, 0.7, 1.0]),
        p: 2,
        u: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
    });
    let shapes: Vec<(&str, GenIn, [f64; 4])> = vec![
        ("line_frustum", line([1.0, 0.0, 0.0], [2.0, 0.0, 2.0]), [0.0, 0.0, 360.0, 1.0]),
        ("line_cyl", line([1.5, 0.0, -1.0], [1.5, 0.0, 2.0]), [0.0, 0.0, 360.0, 1.0]),
        ("line_apex", line([0.0, 0.0, 0.0], [2.0, 0.0, 2.0]), [0.0, 0.0, 360.0, 1.0]),
        ("line_apex_end", line([2.0, 0.0, 0.0], [0.0, 0.0, 2.0]), [0.0, 0.0, 360.0, 1.0]),
        ("line_cross", line([-1.0, 0.0, -1.0], [2.0, 0.0, 2.0]), [0.0, 0.0, 360.0, 1.0]),
        ("line_part", line([1.0, 0.0, 0.0], [2.0, 0.0, 2.0]), [45.0, 0.2, 200.0, 0.9]),
        ("circle_tangent", circle([1.5, 0.0, 0.0], [0.0, 360.0], 1.5), [0.0, 0.0, 360.0, 360.0]),
        ("circle_cross", circle([1.5, 0.0, 0.0], [0.0, 360.0], 2.0), [0.0, 0.0, 360.0, 360.0]),
        ("circle_half", circle([1.0, 0.0, 0.0], [90.0, 270.0], 1.0), [0.0, 90.0, 360.0, 270.0]),
        ("circle_arc", circle([3.0, 0.0, 0.5], [30.0, 200.0], 1.0), [0.0, 30.0, 360.0, 200.0]),
        ("circle_part", circle([3.0, 0.0, 0.5], [30.0, 200.0], 1.0), [45.0, 30.0, 200.0, 200.0]),
        ("bspline_arc", arc, [0.0, 0.0, 360.0, 1.0]),
    ];
    for swap in [false, true] {
        for (name, gen, domain) in &shapes {
            let surf = SurfIn {
                gen: gen.clone(),
                build: BuildIn::Ctor { origin: o, x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0], domain: *domain },
                oob: false,
                swap,
            };
            let Ok(surf) = serde_json::to_value(&surf) else { continue };
            cases.push(Case {
                unit: "surface.surface_p2".into(),
                name: format!("common_const_make_revolution_{name}_{swap}"),
                source: crate::case::CaseSource::Generated { seed: 76 },
                inputs: serde_json::json!({"common_const_make": true, "common_revolution": surf}),
            });
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
            OpIn::Iso { .. } => "iso",
            OpIn::Directrix { .. } => "directrix",
            OpIn::Generator { .. } => "generator",
            OpIn::StartPlane { .. } => "start_plane",
            OpIn::Eq { .. } => "eq",
            OpIn::Plane { .. } => "plane",
            OpIn::IsoCircles { .. } => "iso_circles",
        }
    }

    /// The generators reach a successful construction and a successful run of every op
    /// on the Rust side.
    #[test]
    fn generated_cases_cover_every_op() {
        let mut tally: BTreeMap<(String, String), usize> = BTreeMap::new();
        for seed in 0..600u64 {
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
            "to_step",
            "iso",
            "directrix",
            "generator",
            "start_plane",
            "eq",
            "plane",
        ] {
            let ok = tally.iter().any(|((o, k), _)| {
                o == op && (k == "build:Some(0.0) status:Some(0.0)" || k == "build:Some(0.0) status:None")
            });
            assert!(ok, "{op} never succeeds");
        }
        let ok = tally
            .iter()
            .any(|((o, k), _)| o == "iso_circles" && k == "build:None status:Some(0.0)");
        assert!(ok, "iso_circles never succeeds");
    }
}
