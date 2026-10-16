//! Runner for unit `intersect.surface_drop_lib_p2`: `SmDropPt::SetProperties`,
//! `SmDropPt::GetNextPoint`, `sm_AddToCurve` and `sm_FlushCrv`.
//!
//! C++: `solidcore_ffi::units::intersect_surface_drop_lib_p2` (upstream
//! `source/SMLib/src/SmSurfaceDropLib.cpp`, compiled into the shim) on real `SmPlane`,
//! `SmBSplineSurface` and `SmSurfOfRevolution` surfaces, `SmLine`, `SmCircle` and
//! `SmBSplineCurve` curves and the `SmOffsetSurface` `DropCurve` builds. Rust:
//! `solidcore::intersect::surface_drop_lib::p2`.
//!
//! A case is one op; both sides produce the record documented in the ffi module, compared
//! as the values `rec`. Inputs the shim treats as undefined upstream are [`ffi::REJECTED`]
//! on both sides (the Rust side applies the same predicate with its own evaluations). A
//! `chain` case is a long run of consecutive `sm_AddToCurve` spans (built the same way for
//! both sides) that reaches the 2000 point message and the 10000 point limit.
//!
//! Deferred cases: `SetProperties` drops that reach the offset surface evaluations
//! (`surface.offset_surface`: every non-degenerate curve tangent that is not rejected
//! first), `SmSurface::EvaluateGeometric` (`surface.surface_p3`) or a non-planar
//! `IsSingularity` (`surface.surface_p5`); every `GetNextPoint` past its early returns
//! (`SmSurface::LocalPointSolve`, `surface.surface_p5`); `sm_AddToCurve` dog-legs at a
//! pole of a non-planar surface (`IsSingularity`).
//!
//! At least [`MIN_GENERATED`] generated cases run.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::line::Line;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::surface_drop_lib::p1::{DropPt, SM_SUCCESS};
use solidcore::intersect::surface_drop_lib::p2::{add_to_curve, flush_curve, DropOffset};
use solidcore::kernel::Kernel;
use solidcore::math::{EFF_ZERO, UNDEF_DOUBLE};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::knots::Side;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surf_of_revolution::SurfOfRevolution;
use solidcore::surface::surface::Surface;
use solidcore::SolidError;
use solidcore_ffi::units::intersect_surface_drop_lib_p2 as ffi;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1500;

/// `intersect.surface_drop_lib_p2` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

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
    Bspline {
        p: usize,
        cpts: Vec<[f64; 4]>,
        knots: Vec<f64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SurfIn {
    Plane {
        o: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        scale: [f64; 2],
        domain: [f64; 4],
    },
    Bspline {
        p: usize,
        q: usize,
        net: Vec<Vec<[f64; 4]>>,
        u: Vec<f64>,
        v: Vec<f64>,
    },
    Revolution {
        gen: CurveIn,
        point: [f64; 3],
        axis: [f64; 3],
        swap: bool,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    Set {
        surface: SurfIn,
        pt: Vec<f64>,
        offset: f64,
        singularities: u32,
        tol: f64,
        domain: [f64; 4],
        adjust: bool,
    },
    Next {
        surface: SurfIn,
        curve: CurveIn,
        start: Vec<f64>,
        offset: f64,
        singularities: u32,
        poles: Option<Box<[[f64; 3]; 4]>>,
        domain: [f64; 4],
        closure: u8,
        tol: f64,
        uv_tol: [f64; 2],
        end_t: f64,
    },
    Add {
        surface: SurfIn,
        domain: [f64; 4],
        tol: f64,
        uv_tol: [f64; 2],
        pts: Vec<Vec<f64>>,
        spans: Vec<[usize; 2]>,
        flush: bool,
    },
    /// `n` consecutive spans on the unit square plane (see [`chain`]).
    Chain {
        n: usize,
        flush: bool,
    },
    Flush {
        domain: [f64; 4],
        uv_tol: [f64; 2],
        poly: Vec<[f64; 3]>,
        knots: Vec<f64>,
    },
}

// ---- C++ side -----------------------------------------------------------------------

fn ffi_curve(c: &CurveIn) -> ffi::CurveIn {
    match c {
        CurveIn::Line { a, b } => ffi::CurveIn::Line { a: *a, b: *b },
        CurveIn::Circle { c, x, y, d, r } => ffi::CurveIn::Circle {
            c: *c,
            x: *x,
            y: *y,
            d: *d,
            r: *r,
        },
        CurveIn::Bspline { p, cpts, knots } => ffi::CurveIn::BSpline {
            p: *p,
            cpts: cpts.clone(),
            knots: knots.clone(),
        },
    }
}

fn ffi_surface(s: &SurfIn) -> ffi::SurfaceIn {
    match s {
        SurfIn::Plane {
            o,
            x,
            y,
            scale,
            domain,
        } => ffi::SurfaceIn::Plane {
            o: *o,
            x: *x,
            y: *y,
            scale: *scale,
            domain: *domain,
        },
        SurfIn::Bspline { p, q, net, u, v } => ffi::SurfaceIn::BSpline {
            p: *p,
            q: *q,
            net: net.clone(),
            u: u.clone(),
            v: v.clone(),
        },
        SurfIn::Revolution {
            gen,
            point,
            axis,
            swap,
        } => ffi::SurfaceIn::Revolution {
            gen: ffi_curve(gen),
            point: *point,
            axis: *axis,
            swap: *swap,
        },
    }
}
/// The unit square plane of [`chain`].
fn square_plane() -> SurfIn {
    SurfIn::Plane {
        o: [0.0, 0.0, 0.0],
        x: [1.0, 0.0, 0.0],
        y: [0.0, 1.0, 0.0],
        scale: [1.0, 1.0],
        domain: [0.0, 0.0, 1.0, 1.0],
    }
}

/// A drop point record at `t`, `uv`, `duv` (the other members fixed).
fn chain_pt(t: f64, uv: [f64; 2], duv: [f64; 2]) -> Vec<f64> {
    let mut v = vec![t, 1.0];
    v.extend([0.0; 9]);
    v.extend(uv);
    v.extend(duv);
    v.extend([0.0; 9]);
    v.extend([1.0, 1.0, 0.0, 0.0, 90.0, 0.0, 1.0]);
    v.extend([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1000.0]);
    v
}

/// `n` consecutive spans zig-zagging over the unit square (`t = i`).
fn chain(n: usize, flush: bool) -> Inputs {
    let uv = |i: usize| {
        let row = (i / 40) % 40;
        let col = i % 40;
        let x = if row.is_multiple_of(2) { col } else { 39 - col };
        [0.0125 + 0.025 * x as f64, 0.0125 + 0.025 * row as f64]
    };
    let pts: Vec<Vec<f64>> = (0..=n)
        .map(|i| chain_pt(i as f64, uv(i), [0.01, 0.005]))
        .collect();
    Inputs::Add {
        surface: square_plane(),
        domain: [0.0, 0.0, 1.0, 1.0],
        tol: 1.0e-6,
        uv_tol: [1.0e-9, 1.0e-9],
        pts,
        spans: (0..n).map(|i| [i, i + 1]).collect(),
        flush,
    }
}

/// A chain case as its spans; other cases unchanged.
fn expand(i: Inputs) -> Inputs {
    match i {
        Inputs::Chain { n, flush } => chain(n, flush),
        other => other,
    }
}

fn ffi_op(i: &Inputs) -> Result<ffi::Op, HarnessError> {
    Ok(match i {
        Inputs::Set {
            surface,
            pt,
            offset,
            singularities,
            tol,
            domain,
            adjust,
        } => ffi::Op::Set {
            surface: ffi_surface(surface),
            pt: pt.clone(),
            offset: *offset,
            singularities: *singularities,
            tol: *tol,
            domain: *domain,
            adjust: *adjust,
        },
        Inputs::Next {
            surface,
            curve,
            start,
            offset,
            singularities,
            poles,
            domain,
            closure,
            tol,
            uv_tol,
            end_t,
        } => ffi::Op::Next {
            surface: ffi_surface(surface),
            curve: ffi_curve(curve),
            start: start.clone(),
            offset: *offset,
            singularities: *singularities,
            poles: poles.clone(),
            domain: *domain,
            closure: *closure,
            tol: *tol,
            uv_tol: *uv_tol,
            end_t: *end_t,
        },
        Inputs::Add {
            surface,
            domain,
            tol,
            uv_tol,
            pts,
            spans,
            flush,
        } => ffi::Op::Add {
            surface: ffi_surface(surface),
            domain: *domain,
            tol: *tol,
            uv_tol: *uv_tol,
            pts: pts.clone(),
            spans: spans.clone(),
            flush: *flush,
        },
        Inputs::Chain { .. } => return Err(harness("chain not expanded")),
        Inputs::Flush {
            domain,
            uv_tol,
            poly,
            knots,
        } => ffi::Op::Flush {
            domain: *domain,
            uv_tol: *uv_tol,
            poly: poly.clone(),
            knots: knots.clone(),
        },
    })
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    match ffi::run(&ffi_op(i)?) {
        Ok(rec) => Ok(Output::new().values("rec", rec)),
        Err(e) => Err(harness(format!("C++: {e}"))),
    }
}

// ---- Rust side ----------------------------------------------------------------------

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn ext(d: [f64; 4]) -> Extent2d {
    Extent2d::from_bounds(d[0], d[1], d[2], d[3])
}

fn b(x: bool) -> f64 {
    if x {
        1.0
    } else {
        0.0
    }
}

fn tri_in(x: u8) -> Option<bool> {
    match x {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

fn tri_out(x: Option<bool>) -> f64 {
    match x {
        Some(false) => 0.0,
        Some(true) => 1.0,
        None => 2.0,
    }
}

fn rust_circle(
    k: &Kernel,
    c: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    d: [f64; 2],
    r: f64,
) -> Result<Curve, HarnessError> {
    let e = Ellipse::new_circle(
        k,
        v3(c),
        v3(x),
        v3(y),
        Extent1d::from_min_max(d[0], d[1]),
        r,
        &EllipseOptions::default(),
    )
    .map_err(|e| harness(format!("circle: {e}")))?;
    Ok(Curve::from(e))
}

fn rust_curve(k: &Kernel, c: &CurveIn) -> Result<Curve, HarnessError> {
    match c {
        CurveIn::Line { a, b } => {
            let mut l = Line::from_points(v3(*a), v3(*b), 3);
            l.make_nurb()
                .map_err(|e| harness(format!("line nurb: {e}")))?;
            Ok(Curve::from(l))
        }
        CurveIn::Circle { c, x, y, d, r } => rust_circle(k, *c, *x, *y, *d, *r),
        CurveIn::Bspline { p, cpts, knots } => {
            let pw = cpts
                .iter()
                .map(|c| CPoint::new(c[0], c[1], c[2], c[3]))
                .collect();
            let cur = NurbCurve::new(pw, *p, knots.clone())
                .map_err(|e| harness(format!("curve: {e}")))?;
            Ok(Curve::from(BSplineCurve::from_nurb(cur, 3, false)))
        }
    }
}

/// The surface; `Ok(None)` when a surface of revolution cannot be made.
fn rust_surface(k: &Kernel, s: &SurfIn) -> Result<Option<Surface>, HarnessError> {
    Ok(match s {
        SurfIn::Plane {
            o,
            x,
            y,
            scale,
            domain,
        } => {
            let mut p = Plane::new(
                v3(*o),
                v3(*x),
                v3(*y),
                Vec2::new(scale[0], scale[1]),
                &ext(*domain),
            );
            // with its NURB, as the shim makes it (a read-only query of the Rust plane
            // never keeps one; upstream's would, changing later point solves)
            p.make_nurb().map_err(|e| harness(format!("plane NURB: {e}")))?;
            Some(Surface::from(p))
        }
        SurfIn::Bspline { p, q, net, u, v } => {
            let net = net
                .iter()
                .map(|r| r.iter().map(|c| CPoint::new(c[0], c[1], c[2], c[3])).collect())
                .collect();
            let sur = NurbSurface::new(net, *p, *q, u.clone(), v.clone())
                .map_err(|e| harness(format!("surface: {e}")))?;
            Some(Surface::from(BSplineSurface::from_nurb(sur, false)))
        }
        SurfIn::Revolution {
            gen,
            point,
            axis,
            swap,
        } => {
            let g = rust_curve(k, gen)?;
            SurfOfRevolution::create_canonical(k, g, v3(*point), v3(*axis), *swap)
                .ok()
                .map(Surface::from)
        }
    })
}

fn pt_in(v: &[f64]) -> Result<DropPt, HarnessError> {
    if v.len() != ffi::PT_LEN {
        return Err(harness("drop point length"));
    }
    let g = |i: usize| v.get(i).copied().unwrap_or(0.0);
    let t3 = |i: usize| Vec3::new(g(i), g(i + 1), g(i + 2));
    let tr = |i: usize| tri_in(g(i) as u8);
    let mut p = DropPt::new(g(0), tr(34));
    p.good_drop = tr(1);
    p.crv_pd = [t3(2), t3(5), t3(8)];
    p.uv_pd = [Point2::new(g(11), g(12)), Vec2::new(g(13), g(14))];
    p.pd_3d = [t3(15), t3(18)];
    p.srf_normal = t3(21);
    p.from_left_u = tr(24);
    p.from_left_v = tr(25);
    p.drop_to_surf = g(26);
    p.approx_dev = g(27);
    p.normal_to_crv_ang_deg = g(28);
    p.srf_to_crv_ang_deg = g(29);
    p.srf_to_crv_speed_ratio = g(30);
    p.on_boundary = tr(31);
    p.on_seam = tr(32);
    p.on_pole = tr(33);
    p.leaving_domain = tr(35);
    p.entering_domain = tr(36);
    p.drop_deriv_rtn = g(37) as i32;
    Ok(p)
}

fn put_vec(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn put_pt(o: &mut Vec<f64>, p: &DropPt) {
    o.push(p.t);
    o.push(tri_out(p.good_drop));
    for v in p.crv_pd {
        put_vec(o, v);
    }
    for v in p.uv_pd {
        o.extend([v.x, v.y]);
    }
    for v in p.pd_3d {
        put_vec(o, v);
    }
    put_vec(o, p.srf_normal);
    o.push(tri_out(p.from_left_u));
    o.push(tri_out(p.from_left_v));
    o.extend([
        p.drop_to_surf,
        p.approx_dev,
        p.normal_to_crv_ang_deg,
        p.srf_to_crv_ang_deg,
        p.srf_to_crv_speed_ratio,
    ]);
    for f in [
        p.on_boundary,
        p.on_seam,
        p.on_pole,
        p.degen_curve,
        p.leaving_domain,
        p.entering_domain,
    ] {
        o.push(tri_out(f));
    }
    o.push(f64::from(p.drop_deriv_rtn));
}

fn put_curve(o: &mut Vec<f64>, c: &BSplineCurve) -> solidcore::Result<()> {
    o.push(c.degree()? as f64);
    let cp = c.control_points()?;
    o.push(cp.len() as f64);
    for p in cp {
        o.extend([p.x, p.y, p.z, p.w]);
    }
    let (k, m) = c.knots(None)?;
    o.push(k.len() as f64);
    o.extend_from_slice(&k);
    o.extend(m.iter().map(|&x| x as f64));
    let ivl = c.natural_interval();
    o.extend([ivl.min(), ivl.max()]);
    Ok(())
}

/// The `SmStatus` of a Rust error.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => 1001.0,
    }
}

/// A failed Rust op: deferred, or the harness failed.
enum Fail {
    Deferred(Box<Output>),
    Harness(HarnessError),
}

impl From<HarnessError> for Fail {
    fn from(e: HarnessError) -> Self {
        Fail::Harness(e)
    }
}

/// `Ok(Ok(v))`, `Ok(Err(status))`, or a deferral.
fn st<T>(r: solidcore::Result<T>) -> Result<Result<T, f64>, Fail> {
    match r {
        Ok(v) => Ok(Ok(v)),
        Err(e) => {
            if let Some(d) = Output::unported(&e) {
                return Err(Fail::Deferred(Box::new(d)));
            }
            Ok(Err(status(&e)))
        }
    }
}
fn closure(c: u8) -> SurfParamType {
    match c {
        0 => SurfParamType::U,
        1 => SurfParamType::V,
        2 => SurfParamType::Both,
        _ => SurfParamType::Neither,
    }
}

/// A status record value: `0` for `SM_SUCCESS`.
fn code(c: i32) -> f64 {
    if c == SM_SUCCESS {
        0.0
    } else {
        f64::from(c)
    }
}

fn put_state(
    o: &mut Vec<f64>,
    poly: &[Vec3],
    knots: &[f64],
    curves: &[BSplineCurve],
) -> Result<(), Fail> {
    o.push(poly.len() as f64);
    for p in poly {
        put_vec(o, *p);
    }
    o.push(knots.len() as f64);
    o.extend_from_slice(knots);
    o.push(curves.len() as f64);
    for c in curves {
        st(put_curve(o, c))?.map_err(|_| harness("curve data"))?;
    }
    Ok(())
}

fn run_op(k: &Kernel, i: &Inputs, o: &mut Vec<f64>) -> Result<(), Fail> {
    match i {
        Inputs::Set {
            surface,
            pt,
            offset,
            singularities,
            tol,
            domain,
            adjust,
        } => {
            let Some(s) = rust_surface(k, surface)? else {
                o.push(ffi::NO_SURFACE);
                return Ok(());
            };
            let mut p = pt_in(pt)?;
            let mut off = DropOffset::new(*offset, *singularities);
            let r = p.set_properties(k, &s, &mut off, *singularities, *tol, &ext(*domain), *adjust);
            match st(r)? {
                Ok(c) => o.push(code(c)),
                Err(c) => o.push(c),
            }
            put_pt(o, &p);
            o.push(off.offset_distance);
        }
        Inputs::Next {
            surface,
            curve,
            start,
            offset,
            singularities,
            poles,
            domain,
            closure: cl,
            tol,
            uv_tol: _,
            end_t,
        } => {
            let Some(s) = rust_surface(k, surface)? else {
                o.push(ffi::NO_SURFACE);
                return Ok(());
            };
            let c = rust_curve(k, curve)?;
            let d = ext(*domain);
            let a = pt_in(start)?;
            // the shim's rejection: the start evaluation (status ignored upstream) fails
            let zu = EFF_ZERO * (1.0 + d.u_interval().length());
            let zv = EFF_ZERO * (1.0 + d.v_interval().length());
            if d.contains_point(a.uv_pd[0], zu.max(zv))
                && !(a.on_boundary != Some(false) && a.leaving_domain != Some(false))
                && st(s.evaluate_2nd_derivatives(
                    k, a.uv_pd[0],
                    a.from_left_u != Some(false),
                    a.from_left_v != Some(false),
                ))?
                .is_err()
            {
                o.push(ffi::REJECTED);
                return Ok(());
            }
            let pole_pts: Option<Vec<Vec3>> = poles.as_ref().map(|ps| ps.iter().map(|p| v3(*p)).collect());
            let mut off = DropOffset::new(*offset, *singularities);
            let mut end = DropPt::new(0.0, Some(false));
            let r = a.get_next_point(
                k,
                &s,
                &mut off,
                *singularities,
                pole_pts.as_deref(),
                &d,
                closure(*cl),
                &c,
                *tol,
                *end_t,
                &mut end,
            );
            match st(r)? {
                Ok(c) => o.push(code(c)),
                Err(c) => o.push(c),
            }
            put_pt(o, &end);
            o.push(off.offset_distance);
        }
        Inputs::Add {
            surface,
            domain,
            tol,
            uv_tol,
            pts,
            spans,
            flush,
        } => {
            let Some(s) = rust_surface(k, surface)? else {
                o.push(ffi::NO_SURFACE);
                return Ok(());
            };
            let d = ext(*domain);
            let pts = pts.iter().map(|p| pt_in(p)).collect::<Result<Vec<_>, _>>()?;
            let mut poly = Vec::new();
            let mut knots = Vec::new();
            let mut curves = Vec::new();
            let mut failed = false;
            for [ia, ib] in spans {
                let (Some(a), Some(e)) = (pts.get(*ia), pts.get(*ib)) else {
                    return Err(Fail::Harness(harness("span index")));
                };
                let mut ch = false;
                let r = add_to_curve(
                    k, &s, &d, a, e, &mut poly, &mut knots, *tol, uv_tol[0], uv_tol[1], &mut curves,
                    &mut ch,
                );
                match st(r)? {
                    Ok(()) => o.push(0.0),
                    Err(c) => {
                        o.push(c);
                        failed = true;
                    }
                }
                o.push(b(ch));
                if failed {
                    break;
                }
            }
            if *flush && !failed {
                let mut ch = false;
                let r = flush_curve(&d, &mut poly, &mut knots, uv_tol[0], uv_tol[1], &mut curves, &mut ch);
                match st(r)? {
                    Ok(()) => o.push(0.0),
                    Err(c) => o.push(c),
                }
                o.push(b(ch));
            }
            put_state(o, &poly, &knots, &curves)?;
        }
        Inputs::Chain { .. } => return Err(Fail::Harness(harness("chain not expanded"))),
        Inputs::Flush {
            domain,
            uv_tol,
            poly,
            knots,
        } => {
            let d = ext(*domain);
            let mut poly: Vec<Vec3> = poly.iter().map(|p| v3(*p)).collect();
            let mut knots = knots.clone();
            let mut curves = Vec::new();
            let mut ch = false;
            let r = flush_curve(&d, &mut poly, &mut knots, uv_tol[0], uv_tol[1], &mut curves, &mut ch);
            match st(r)? {
                Ok(()) => o.push(0.0),
                Err(c) => o.push(c),
            }
            o.push(b(ch));
            put_state(o, &poly, &knots, &curves)?;
        }
    }
    Ok(())
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let mut o = Vec::new();
    match run_op(&k, i, &mut o) {
        Ok(()) => Ok(Output::new().values("rec", o)),
        Err(Fail::Deferred(d)) => Ok(*d),
        Err(Fail::Harness(e)) => Err(e),
    }
}

// ---- generation ---------------------------------------------------------------------

fn pick<T: Copy>(rng: &mut Rng, xs: &[T], default: T) -> T {
    rng.choose(xs).copied().unwrap_or(default)
}

fn unit_dir(rng: &mut Rng) -> [f64; 3] {
    loop {
        let p = rng.point3(-1.0, 1.0);
        let l = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        if l > 0.1 && l <= 1.0 {
            return [p[0] / l, p[1] / l, p[2] / l];
        }
    }
}

fn scale3(p: [f64; 3], s: f64) -> [f64; 3] {
    [p[0] * s, p[1] * s, p[2] * s]
}

fn add3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm3(a: [f64; 3]) -> [f64; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    if l > 0.0 {
        scale3(a, 1.0 / l)
    } else {
        [0.0, 0.0, 1.0]
    }
}

fn arr3(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// A random flag value (`0`, `1`, rarely `2`).
fn rflag(rng: &mut Rng) -> f64 {
    if rng.chance(0.1) {
        2.0
    } else if rng.chance(0.5) {
        1.0
    } else {
        0.0
    }
}

/// A random drop point state (`PT_LEN` values).
fn gen_pt(rng: &mut Rng, uv: [f64; 2], duv: [f64; 2], crv: [[f64; 3]; 3], t: f64) -> Vec<f64> {
    let mut v = Vec::with_capacity(ffi::PT_LEN);
    v.push(t);
    v.push(rflag(rng));
    for c in crv {
        v.extend(c);
    }
    v.extend(uv);
    v.extend(duv);
    let undef = rng.chance(0.3);
    for _ in 0..6 {
        v.push(if undef { UNDEF_DOUBLE } else { rng.uniform(-5.0, 5.0) });
    }
    for _ in 0..3 {
        v.push(if undef { UNDEF_DOUBLE } else { rng.uniform(-1.0, 1.0) });
    }
    v.push(rflag(rng));
    v.push(rflag(rng));
    v.push(rng.uniform(0.0, 1.0));
    v.push(rng.uniform(0.0, 1.0));
    v.push(rng.uniform(0.0, 180.0));
    v.push(pick(rng, &[0.0, 1.0, 2.9, 3.0, 3.1, 10.0, 14.9, 15.0, 40.0], 1.0));
    v.push(pick(rng, &[0.1, 0.25, 0.26, 0.5, 0.51, 1.0, 1.99, 2.0, 3.99, 4.0, 9.0], 1.0));
    for _ in 0..6 {
        v.push(rflag(rng));
    }
    v.push(pick(rng, &[0.0, 1000.0, 1001.0, 1032.0, 99_999_999.0], 1000.0));
    v
}

fn gen_knots(rng: &mut Rng, n1: usize, p: usize, a: f64, b: f64) -> Vec<f64> {
    let interior = n1 - p - 1;
    let mut mid: Vec<f64> = (0..interior).map(|_| rng.uniform(a, b)).collect();
    mid.sort_by(f64::total_cmp);
    if interior > 0 && rng.chance(0.2) {
        // uniform interior knots
        mid = (1..=interior)
            .map(|i| a + (b - a) * i as f64 / (interior + 1) as f64)
            .collect();
    }
    let mut k = vec![a; p + 1];
    k.extend(mid);
    k.extend(vec![b; p + 1]);
    k
}

/// A plane through a random frame; its natural domain.
fn gen_plane(rng: &mut Rng, s: f64) -> SurfIn {
    let x = unit_dir(rng);
    let y = norm3(cross3(unit_dir(rng), x));
    let y = if rng.chance(0.2) {
        // non-orthogonal axes
        norm3(add3(y, scale3(x, 0.3)))
    } else {
        y
    };
    let u0 = rng.uniform(-2.0, 1.0) * s;
    let v0 = rng.uniform(-2.0, 1.0) * s;
    SurfIn::Plane {
        o: scale3(rng.point3(-1.0, 1.0), s),
        x,
        y,
        scale: [
            pick(rng, &[1.0, 0.5, 2.0, -1.0], 1.0),
            pick(rng, &[1.0, 0.25, 3.0], 1.0),
        ],
        domain: [u0, v0, u0 + rng.uniform(0.5, 3.0) * s, v0 + rng.uniform(0.5, 3.0) * s],
    }
}

/// A non-rational B-spline surface: a height field, a bilinear patch, or a rational
/// cylinder patch.
fn gen_bspline_surface(rng: &mut Rng, s: f64) -> SurfIn {
    let kind = rng.below(3);
    if kind == 1 {
        // bilinear patch
        let mut c = [[0.0; 3]; 4];
        for p in &mut c {
            *p = scale3(rng.point3(-1.0, 1.0), s);
        }
        let [c0, c1, c2, c3] = c;
        let w = |p: [f64; 3]| [p[0], p[1], p[2], NOW];
        let (a0, a1) = (rng.uniform(-1.0, 0.0), rng.uniform(0.5, 2.0));
        let (b0, b1) = (rng.uniform(-1.0, 0.0), rng.uniform(0.5, 2.0));
        return SurfIn::Bspline {
            p: 1,
            q: 1,
            net: vec![vec![w(c0), w(c1)], vec![w(c2), w(c3)]],
            u: vec![a0, a0, a1, a1],
            v: vec![b0, b0, b1, b1],
        };
    }
    if kind == 2 {
        // a closed (full) rational cylinder: degree 2 x 1, closed in u
        let r = rng.uniform(0.5, 2.0) * s;
        let h = rng.uniform(0.5, 2.0) * s;
        let w2 = core::f64::consts::FRAC_1_SQRT_2;
        let ring = [
            ([1.0, 0.0], 1.0),
            ([1.0, 1.0], w2),
            ([0.0, 1.0], 1.0),
            ([-1.0, 1.0], w2),
            ([-1.0, 0.0], 1.0),
            ([-1.0, -1.0], w2),
            ([0.0, -1.0], 1.0),
            ([1.0, -1.0], w2),
            ([1.0, 0.0], 1.0),
        ];
        let net = ring
            .iter()
            .map(|(xy, w)| {
                vec![
                    [xy[0] * r * w, xy[1] * r * w, 0.0, *w],
                    [xy[0] * r * w, xy[1] * r * w, h * w, *w],
                ]
            })
            .collect();
        return SurfIn::Bspline {
            p: 2,
            q: 1,
            net,
            u: vec![0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0],
            v: vec![0.0, 0.0, 1.0, 1.0],
        };
    }
    let p = 1 + rng.below(3) as usize;
    let q = 1 + rng.below(3) as usize;
    let nu = p + 1 + rng.below(3) as usize;
    let nv = q + 1 + rng.below(3) as usize;
    let amp = rng.uniform(0.0, 0.5);
    let net = (0..nu)
        .map(|i| {
            (0..nv)
                .map(|j| {
                    let x = i as f64 / (nu - 1) as f64;
                    let y = j as f64 / (nv - 1) as f64;
                    [x * s, y * s, rng.uniform(-amp, amp) * s, NOW]
                })
                .collect()
        })
        .collect();
    SurfIn::Bspline {
        p,
        q,
        net,
        u: gen_knots(rng, nu, p, 0.0, 1.0),
        v: gen_knots(rng, nv, q, 0.0, 1.0),
    }
}

/// A surface of revolution about the z axis: a line generator (cylinder or cone) or a
/// circle arc (torus or sphere patch).
fn gen_revolution(rng: &mut Rng, s: f64) -> SurfIn {
    let gen = if rng.chance(0.6) {
        let r0 = rng.uniform(0.5, 2.0) * s;
        let r1 = if rng.chance(0.5) {
            r0
        } else {
            rng.uniform(0.2, 2.0) * s
        };
        let z0 = rng.uniform(-1.0, 0.0) * s;
        let z1 = z0 + rng.uniform(0.5, 2.0) * s;
        CurveIn::Line {
            a: [r0, 0.0, z0],
            b: [r1, 0.0, z1],
        }
    } else {
        let big = rng.uniform(1.5, 3.0) * s;
        let small = rng.uniform(0.3, 1.0) * s;
        let d0 = rng.uniform(-1.5, 0.0);
        CurveIn::Circle {
            c: [big, 0.0, 0.0],
            x: [1.0, 0.0, 0.0],
            y: [0.0, 0.0, 1.0],
            d: [d0, d0 + rng.uniform(0.5, 3.0)],
            r: small,
        }
    };
    SurfIn::Revolution {
        gen,
        point: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        swap: rng.chance(0.3),
    }
}

fn gen_surface(rng: &mut Rng, s: f64) -> SurfIn {
    match rng.below(10) {
        0..=2 => gen_plane(rng, s),
        3..=5 => gen_bspline_surface(rng, s),
        _ => gen_revolution(rng, s),
    }
}

fn gen_scale(rng: &mut Rng) -> f64 {
    pick(rng, &[1.0, 1.0, 1.0, 1.0e-3, 1.0e3, 0.01, 100.0], 1.0)
}

/// The surface and its natural domain (`None` when it cannot be built).
fn surface_with_domain(k: &Kernel, s: &SurfIn) -> Option<(Surface, [f64; 4])> {
    let srf = rust_surface(k, s).ok()??;
    let d = srf.natural_uv_domain();
    Some((srf, [d.u_min(), d.v_min(), d.u_max(), d.v_max()]))
}

/// The natural domain or a random sub-domain.
fn gen_domain(rng: &mut Rng, nat: [f64; 4]) -> [f64; 4] {
    if rng.chance(0.6) {
        return nat;
    }
    let du = nat[2] - nat[0];
    let dv = nat[3] - nat[1];
    let a = rng.uniform(0.0, 0.4);
    let b2 = rng.uniform(0.6, 1.0);
    let c = rng.uniform(0.0, 0.4);
    let d = rng.uniform(0.6, 1.0);
    [
        nat[0] + a * du,
        nat[1] + c * dv,
        nat[0] + b2 * du,
        nat[1] + d * dv,
    ]
}

fn uv_in(rng: &mut Rng, d: [f64; 4]) -> [f64; 2] {
    [rng.uniform(d[0], d[2]), rng.uniform(d[1], d[3])]
}

/// A uv on a boundary of `d` (perhaps nudged by about `tol`), or inside.
fn uv_on_boundary(rng: &mut Rng, d: [f64; 4], tol: [f64; 2]) -> [f64; 2] {
    let mut uv = uv_in(rng, d);
    let nudge = |rng: &mut Rng, t: f64| match rng.below(4) {
        0 => 0.0,
        1 => t * 0.5,
        2 => -t * 0.5,
        _ => t * 2.0,
    };
    match rng.below(6) {
        0 => uv[0] = d[0] + nudge(rng, tol[0]),
        1 => uv[0] = d[2] + nudge(rng, tol[0]),
        2 => uv[1] = d[1] + nudge(rng, tol[1]),
        3 => uv[1] = d[3] + nudge(rng, tol[1]),
        4 => {
            uv[0] = d[0];
            uv[1] = d[3];
        }
        _ => {}
    }
    uv
}

fn gen_uv_tol(rng: &mut Rng, d: [f64; 4]) -> [f64; 2] {
    let f = pick(rng, &[1.0e-12, 1.0e-9, 1.0e-6, 1.0e-3, 0.0], 1.0e-9);
    [f * (1.0 + (d[2] - d[0]).abs()), f * (1.0 + (d[3] - d[1]).abs())]
}
/// A curve near the surface: through surface points (perhaps offset).
fn gen_curve_near(k: &Kernel, rng: &mut Rng, srf: &Surface, d: [f64; 4], s: f64) -> Option<CurveIn> {
    let n = 2 + rng.below(4) as usize;
    let off = pick(rng, &[0.0, 0.0, 1.0e-9, 1.0e-6, 1.0e-3, 0.1], 0.0) * s;
    let mut cpts = Vec::new();
    for _ in 0..n {
        let uv = uv_in(rng, d);
        let p = srf.evaluate_point(k, Point2::new(uv[0], uv[1])).ok()?;
        let p = add3(arr3(p), scale3(unit_dir(rng), off));
        cpts.push([p[0], p[1], p[2], NOW]);
    }
    let p = (n - 1).min(1 + rng.below(3) as usize);
    let t0 = pick(rng, &[0.0, -1.0, 2.0], 0.0);
    let t1 = t0 + pick(rng, &[1.0, 0.5, 10.0], 1.0);
    let knots = gen_knots(rng, n, p, t0, t1);
    if cpts.windows(2).all(|w| w.first() == w.last()) {
        return None;
    }
    Some(CurveIn::Bspline { p, cpts, knots })
}

/// `uv` a little outside `d` (by `frac` of its size) on a random side.
fn uv_outside(rng: &mut Rng, d: [f64; 4], frac: f64) -> [f64; 2] {
    let mut uv = uv_in(rng, d);
    let w = [d[2] - d[0], d[3] - d[1]];
    match rng.below(4) {
        0 => uv[0] = d[0] - frac * w[0],
        1 => uv[0] = d[2] + frac * w[0],
        2 => uv[1] = d[1] - frac * w[1],
        _ => uv[1] = d[3] + frac * w[1],
    }
    uv
}

fn gen_singularities(rng: &mut Rng) -> u32 {
    if rng.chance(0.75) {
        0
    } else {
        pick(rng, &[1, 2, 4, 8, 16, 5, 10, 15], 1)
    }
}

/// A curve point near the surface point `p` with normal `n` and tangents `su`, `sv`:
/// along the normal by a random distance, off it by a random deviation.
fn near_point(rng: &mut Rng, p: [f64; 3], n: [f64; 3], su: [f64; 3], s: f64) -> [f64; 3] {
    let along = pick(rng, &[0.0, 0.0, 1.0e-9, 1.0e-6, 0.1, 1.0, 10.0], 0.0)
        * s
        * if rng.chance(0.5) { 1.0 } else { -1.0 };
    let off = pick(rng, &[0.0, 0.0, 1.0e-12, 1.0e-8, 1.0e-6, 1.0e-3, 1.0], 0.0) * s;
    let side = norm3(su);
    add3(add3(p, scale3(n, along)), scale3(side, off))
}

/// A curve tangent: in the tangent plane, zero, along the normal, near it (within or
/// just past 3 degrees), or random.
fn tangent(rng: &mut Rng, su: [f64; 3], sv: [f64; 3], n: [f64; 3], s: f64) -> [f64; 3] {
    match rng.below(10) {
        0 => [0.0, 0.0, 0.0],
        1 => scale3(n, s),
        2 => {
            let a = pick(rng, &[0.5_f64, 2.9, 3.1, 10.0], 2.9).to_radians();
            add3(scale3(n, a.cos() * s), scale3(norm3(su), a.sin() * s))
        }
        3 => scale3(unit_dir(rng), s),
        4 => scale3(sv, pick(rng, &[1.0, -1.0], 1.0)),
        5 => scale3(su, pick(rng, &[1.0, -1.0], 1.0)),
        6 => [1.0e-12, 0.0, 0.0],
        _ => {
            let a = rng.uniform(-1.0, 1.0);
            let c = rng.uniform(-1.0, 1.0);
            add3(scale3(su, a), scale3(sv, c))
        }
    }
}

fn gen_set(rng: &mut Rng, k: &Kernel) -> Option<Inputs> {
    let s = gen_scale(rng);
    let surface = gen_surface(rng, s);
    let (srf, nat) = surface_with_domain(k, &surface)?;
    let domain = gen_domain(rng, nat);
    let utol = gen_uv_tol(rng, domain);
    let uv = match rng.below(20) {
        0..=9 => uv_in(rng, domain),
        10..=16 => uv_on_boundary(rng, domain, utol),
        17 => uv_outside(rng, domain, 1.0e-3),
        18 => [domain[0], domain[1]],
        _ => uv_on_boundary(rng, nat, utol),
    };
    let u = Point2::new(uv[0], uv[1]);
    let (p, su, sv) = srf.evaluate_1st_derivatives(k, u, true, true).ok()?;
    let n = srf.evaluate_normal(k, u, Side::Left, Side::Left).ok()?;
    let (p, su, sv, n) = (arr3(p), arr3(su), arr3(sv), arr3(n));
    let c0 = near_point(rng, p, n, su, s);
    let c1 = tangent(rng, su, sv, n, s);
    let c2 = scale3(rng.point3(-1.0, 1.0), s);
    let degen = match rng.below(20) {
        0..=8 => 1.0,
        9..=14 => 2.0,
        _ => 0.0,
    };
    let duv = if degen == 2.0 && rng.chance(0.7) {
        [0.0, 0.0]
    } else {
        [rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)]
    };
    let t = rng.unit();
    let mut pt = gen_pt(rng, uv, duv, [c0, c1, c2], t);
    if let Some(f) = pt.get_mut(34) {
        *f = degen;
    }
    Some(Inputs::Set {
        surface,
        pt,
        offset: pick(rng, &[1.0, 0.0, -2.0], 1.0),
        singularities: gen_singularities(rng),
        tol: pick(rng, &[1.0e-6, 1.0e-3, 1.0e-9, 0.1], 1.0e-6) * s,
        domain,
        adjust: rng.chance(0.5),
    })
}

fn gen_next(rng: &mut Rng, k: &Kernel) -> Option<Inputs> {
    let s = gen_scale(rng);
    let surface = gen_surface(rng, s);
    let (srf, nat) = surface_with_domain(k, &surface)?;
    let domain = gen_domain(rng, nat);
    let curve = if rng.chance(0.7) {
        gen_curve_near(k, rng, &srf, domain, s)?
    } else {
        CurveIn::Line {
            a: scale3(rng.point3(-1.0, 1.0), s),
            b: scale3(rng.point3(-1.0, 1.0), s),
        }
    };
    let c = rust_curve(k, &curve).ok()?;
    let ivl = c.natural_interval();
    let utol = gen_uv_tol(rng, domain);
    let kind = rng.below(3);
    let uv = match kind {
        0 => {
            let f = pick(rng, &[1.0e-3, 0.5, 1.0e-14], 1.0e-3);
            uv_outside(rng, domain, f)
        }
        1 => uv_on_boundary(rng, domain, utol),
        _ => uv_in(rng, domain),
    };
    let t0 = rng.uniform(ivl.min(), ivl.max());
    let crv = c.evaluate(k, t0, 2, true, true).ok()?;
    let g = |i: usize| crv.get(i).map_or([0.0; 3], |v| arr3(*v));
    let duv = [rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)];
    let mut start = gen_pt(rng, uv, duv, [g(0), g(1), g(2)], t0);
    if kind == 1 {
        // on the boundary: leaving (an early return) or not
        let leaving = pick(rng, &[0.0, 1.0, 2.0], 1.0);
        if let Some(f) = start.get_mut(31) {
            *f = pick(rng, &[1.0, 1.0, 2.0, 0.0], 1.0);
        }
        if let Some(f) = start.get_mut(35) {
            *f = leaving;
        }
    }
    let end_t = match rng.below(6) {
        0 => ivl.max() + pick(rng, &[0.5, 1.0e-3], 0.5) * (ivl.max() - ivl.min()),
        _ => rng.uniform(ivl.min(), ivl.max()),
    };
    let poles = if rng.chance(0.2) {
        let mut ps = [[0.0; 3]; 4];
        for p in &mut ps {
            *p = scale3(rng.point3(-1.0, 1.0), s);
        }
        Some(Box::new(ps))
    } else {
        None
    };
    Some(Inputs::Next {
        surface,
        curve,
        start,
        offset: 1.0,
        singularities: gen_singularities(rng),
        poles,
        domain,
        closure: pick(rng, &[0, 1, 2, 3, 3], 3),
        tol: pick(rng, &[1.0e-6, 1.0e-3, 1.0e-9], 1.0e-6) * s,
        uv_tol: utol,
        end_t,
    })
}

/// A run of spans over the domain: consecutive spans (sometimes with gaps in `t` or
/// `uv` around `SM_EFF_ZERO`, big jumps, zero length or reversed spans), points on and
/// near the boundary, pole flags.
fn gen_add(rng: &mut Rng, k: &Kernel) -> Option<Inputs> {
    let s = gen_scale(rng);
    let surface = if rng.chance(0.5) {
        gen_plane(rng, s)
    } else {
        gen_surface(rng, s)
    };
    let (_, nat) = surface_with_domain(k, &surface)?;
    let domain = gen_domain(rng, nat);
    let utol = gen_uv_tol(rng, domain);
    let w = [domain[2] - domain[0], domain[3] - domain[1]];
    let nspans = 1 + rng.below(8) as usize;
    let crv = [[0.0; 3]; 3];
    let mut pts: Vec<Vec<f64>> = Vec::new();
    let mut spans = Vec::new();
    let mut t = pick(rng, &[0.0, -1.0, 100.0], 0.0);
    let mut uv = if rng.chance(0.4) {
        uv_on_boundary(rng, domain, utol)
    } else {
        uv_in(rng, domain)
    };
    let pole = |rng: &mut Rng| if rng.chance(0.1) { pick(rng, &[1.0, 2.0], 1.0) } else { 0.0 };
    for _ in 0..nspans {
        let mag = pick(rng, &[0.0, 0.2, 1.0, 3.0], 1.0);
        let da = [rng.uniform(-mag, mag) * w[0], rng.uniform(-mag, mag) * w[1]];
        let db = [rng.uniform(-mag, mag) * w[0], rng.uniform(-mag, mag) * w[1]];
        let dt = match rng.below(12) {
            0 => 0.0,
            1 => 0.5e-10,
            2 => -0.5,
            _ => rng.uniform(0.1, 1.0) * pick(rng, &[1.0, 1.0e-3, 10.0], 1.0),
        };
        let next_uv = match rng.below(6) {
            0 => uv_on_boundary(rng, domain, utol),
            1 => {
                let f = pick(rng, &[1.0e-12, 0.1], 0.1);
                uv_outside(rng, domain, f)
            }
            _ => uv_in(rng, domain),
        };
        let mut a = gen_pt(rng, uv, da, crv, t);
        let mut e = gen_pt(rng, next_uv, db, crv, t + dt);
        for p in [&mut a, &mut e] {
            let f = pole(rng);
            if let Some(x) = p.get_mut(33) {
                *x = f;
            }
        }
        pts.push(a);
        pts.push(e);
        spans.push([pts.len() - 2, pts.len() - 1]);
        // the next start: continuous, or off by about SM_EFF_ZERO, or a jump
        t += dt;
        uv = next_uv;
        match rng.below(10) {
            0 => t += pick(rng, &[0.5e-10, 2.0e-10, -2.0e-10, 1.0], 1.0),
            1 => uv[0] += pick(rng, &[0.5e-10, 2.0e-10, 0.3 * w[0]], 2.0e-10),
            _ => {}
        }
        if rng.chance(0.1) {
            // repeat a span
            let last = pts.len() - 1;
            spans.push([last - 1, last]);
        }
    }
    Some(Inputs::Add {
        surface,
        domain,
        tol: pick(rng, &[1.0e-6, 1.0e-3], 1.0e-6) * s,
        uv_tol: utol,
        pts,
        spans,
        flush: rng.chance(0.8),
    })
}

fn gen_flush(rng: &mut Rng) -> Inputs {
    let s = gen_scale(rng);
    let d0 = [rng.uniform(-1.0, 0.0) * s, rng.uniform(-1.0, 0.0) * s];
    let domain = [
        d0[0],
        d0[1],
        d0[0] + rng.uniform(0.5, 2.0) * s,
        d0[1] + rng.uniform(0.5, 2.0) * s,
    ];
    let tol = gen_uv_tol(rng, domain);
    let nk = pick(rng, &[0, 2, 2, 3, 4, 6], 2);
    let mut knots: Vec<f64> = Vec::new();
    let mut t = rng.uniform(-1.0, 1.0);
    for _ in 0..nk {
        knots.push(t);
        t += pick(rng, &[1.0, 0.25, 1.0e-6, 3.0], 1.0);
    }
    let np = if nk == 0 { rng.below(4) as usize } else { 3 * nk - 2 };
    let poly = (0..np)
        .map(|_| {
            let mut p = [rng.uniform(domain[0], domain[2]), rng.uniform(domain[1], domain[3]), 0.0];
            match rng.below(5) {
                0 => p[0] = pick(rng, &[domain[0], domain[2], domain[0] + 0.5 * tol[0]], domain[0]),
                1 => p[1] = pick(rng, &[domain[1], domain[3], domain[3] - 0.5 * tol[1]], domain[1]),
                2 => p[0] = domain[0] - 0.1 * s,
                _ => {}
            }
            p
        })
        .collect();
    Inputs::Flush {
        domain,
        uv_tol: tol,
        poly,
        knots,
    }
}

fn gen_inputs(rng: &mut Rng) -> Inputs {
    let k = Kernel::default();
    loop {
        let got = match rng.below(100) {
            0..=44 => gen_set(rng, &k),
            45..=64 => gen_next(rng, &k),
            65..=89 => gen_add(rng, &k),
            90..=97 => Some(gen_flush(rng)),
            _ => Some(Inputs::Chain {
                n: pick(rng, &[10, 700, 3332, 3333], 10),
                flush: rng.chance(0.5),
            }),
        };
        if let Some(i) = got {
            let i2 = expand(i.clone());
            if ffi_op(&i2).is_ok_and(|op| ffi::encode(&op).is_ok()) {
                return i;
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "intersect.surface_drop_lib_p2"
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&expand(case.parse()?))
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&expand(case.parse()?))
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_inputs(rng)).ok()
    }
}

