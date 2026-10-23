//! Runner for unit `intersect.surface_drop_lib_p3`: `SmSurface::DropCurve` (the base
//! body).
//!
//! C++: `solidcore_ffi::units::intersect_surface_drop_lib_p3` (the library's
//! `SmSurface::DropCurve`, called non-virtually) on real `SmPlane`, `SmBSplineSurface`
//! and `SmSurfOfRevolution` surfaces (spheres and cones with poles, cylinders and tori
//! with seams, a closed rational B-spline cylinder, a B-spline with a collapsed edge)
//! and `SmLine`, `SmCircle` and `SmBSplineCurve` curves, with or without an
//! `SmDropCurveFail` record. Rust: `solidcore::intersect::surface_drop_lib::p3`
//! ([`Surface::drop_curve_base`]) on the same geometry (built by the
//! `intersect.surface_drop_lib_p2` runner's builders).
//!
//! A case is one drop; both sides produce the record documented in the ffi module,
//! compared as the values `rec`.
//!
//! Deferred cases: drops that finish on a surface that is not a B-spline surface need
//! the curve on that surface for the final gap measure (`curve.crv_on_surf`, dispatch
//! gap). Everything else compares: the early exits (degenerate or uncontained domains,
//! a first drop off the surface), degenerate (point) curves anywhere on the surface
//! (interior, boundaries, seams, poles: the pole line), curves along the surface
//! normal, and the traced curves on B-spline surfaces (the offset surface tangent drops,
//! the span subdivision stack, `AddToCurve`, the `CrvOnSurf` gap).
//!
//! At least [`MIN_GENERATED`] generated cases run.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::surface_drop_lib::p1::{DropFailRecord, SM_SUCCESS};
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::SurfParamType;
use solidcore::nurbs::math::NOW;
use solidcore::surface::surface::p2::DropCurveFail;
use solidcore::surface::surface::p4::NoObject;
use solidcore::surface::surface::Surface;
use solidcore_ffi::units::intersect_surface_drop_lib_p3 as ffi;

use super::intersect_surface_drop_lib_p2::{
    add3, arr3, cross3, ext, ffi_curve, ffi_surface, gen_curve_near, gen_domain, gen_knots,
    gen_scale, gen_surface, norm3, pick, put_curve, rust_curve, rust_surface, scale3, status,
    surface_with_domain, unit_dir, uv_in, CurveIn, SurfIn,
};
use crate::case::{CaseContext, CaseSource};
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1500;

/// `intersect.surface_drop_lib_p3` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    surface: SurfIn,
    curve: CurveIn,
    domain: [f64; 4],
    interval: [f64; 2],
    tol: f64,
    keep_all: bool,
    want_record: bool,
}

fn ffi_drop(i: &Inputs) -> ffi::Drop {
    ffi::Drop {
        surface: ffi_surface(&i.surface),
        curve: ffi_curve(&i.curve),
        domain: i.domain,
        interval: i.interval,
        tol: i.tol,
        keep_all: i.keep_all,
        want_record: i.want_record,
    }
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    match ffi::run(&ffi_drop(i)) {
        Ok(rec) => Ok(Output::new().values("rec", rec)),
        Err(e) => Err(harness(format!("C++: {e}"))),
    }
}

// ---- Rust side ----------------------------------------------------------------------

fn put_vec(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

/// The record value of a `SmSurfParamType` (see the ffi module).
fn param_code(p: SurfParamType) -> f64 {
    match p {
        SurfParamType::U => 0.0,
        SurfParamType::V => 1.0,
        SurfParamType::Both => 2.0,
        SurfParamType::Neither => 3.0,
        SurfParamType::Unknown => 4.0,
        _ => 5.0,
    }
}

/// A status record value: `0` for `SM_SUCCESS` (1000), as the shim's `st`.
fn code(c: i32) -> f64 {
    if c == SM_SUCCESS {
        0.0
    } else {
        f64::from(c)
    }
}

fn put_curves(o: &mut Vec<f64>, curves: &[BSplineCurve]) -> Result<(), HarnessError> {
    o.push(curves.len() as f64);
    for c in curves {
        put_curve(o, c).map_err(|e| harness(format!("curve data: {e}")))?;
    }
    Ok(())
}

fn put_record(o: &mut Vec<f64>, f: &DropCurveFail<NoObject>) -> Result<(), HarnessError> {
    o.push(code(f.fail_status));
    o.push(f.from_call as f64);
    o.push(f64::from(f.create_path_type as u8));
    o.push(if f.has_context { 1.0 } else { 0.0 });
    let (dmin, dmax) = (f.uv_domain.min(), f.uv_domain.max());
    o.extend([dmin.x, dmin.y, dmax.x, dmax.y]);
    o.extend([f.interval.min(), f.interval.max()]);
    o.push(f.approx_tol_3d);
    o.push(f.max_drop_to_surf_arg);
    o.push(f.max_approx_dev);
    o.push(f64::from(f.keep_all_drop_curves));
    o.push(f64::from(f.degen_curve));
    o.push(f64::from(f.degen_surf_u));
    o.push(f64::from(f.degen_surf_v));
    o.push(f64::from(f.drop_domain_is_contained));
    o.push(f.max_drop_to_surf);
    o.push(f64::from(f.closed_u));
    o.push(f64::from(f.closed_v));
    o.push(param_code(f.srf_closure));
    o.push(f.singularities as f64);
    o.push(f.approx_singularities as f64);
    o.push(f.srf_pole_points.len() as f64);
    for p in &f.srf_pole_points {
        put_vec(o, *p);
    }
    o.push(f.srf_pole_normals.len() as f64);
    for p in &f.srf_pole_normals {
        put_vec(o, *p);
    }
    let sols = f.solutions.as_slice();
    o.push(sols.len() as f64);
    for s in sols {
        o.extend([s.start.parameters[0], s.start.parameters[1]]);
    }
    o.push(f.ctrl_poly.len() as f64);
    for p in &f.ctrl_poly {
        put_vec(o, *p);
    }
    o.push(f.knots.len() as f64);
    o.extend_from_slice(&f.knots);
    put_curves(o, &f.uv_curves)
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let Some(srf) = rust_surface(&k, &i.surface)? else {
        return Ok(Output::new().values("rec", vec![ffi::NO_SURFACE]));
    };
    let curve = rust_curve(&k, &i.curve)?;
    let mut fail: DropCurveFail<NoObject> = DropCurveFail::default();
    let record = i.want_record.then_some(DropFailRecord {
        fail: &mut fail,
        surface: None,
        curve: None,
    });
    let r = srf.drop_curve_base(
        &k,
        &ext(i.domain),
        &curve,
        &Extent1d::from_min_max(i.interval[0], i.interval[1]),
        i.tol,
        i.keep_all,
        record,
    );
    let mut o = Vec::new();
    match r {
        Ok(out) => {
            o.push(0.0);
            o.push(out.max_drop_to_surf);
            o.push(out.max_approx_dev);
            put_curves(&mut o, &out.uv_curves)?;
        }
        Err(e) => {
            if let Some(d) = Output::unported(&e) {
                return Ok(d);
            }
            o.extend([status(&e), 0.0, 0.0, 0.0]);
        }
    }
    if i.want_record {
        put_record(&mut o, &fail)?;
    }
    Ok(Output::new().values("rec", o))
}

// ---- generation ---------------------------------------------------------------------

/// A surface with poles: a sphere or a cone about the z axis (surfaces of revolution),
/// or a B-spline patch with its `u = 0` edge collapsed to a point.
fn gen_pole_surface(rng: &mut Rng, s: f64) -> SurfIn {
    match rng.below(3) {
        0 => {
            // sphere (perhaps a cap): circle about the axis
            let r = rng.uniform(0.5, 2.0) * s;
            let half = core::f64::consts::FRAC_PI_2;
            let d0 = if rng.chance(0.7) {
                -half
            } else {
                rng.uniform(-1.2, 0.0)
            };
            let d1 = if rng.chance(0.7) {
                half
            } else {
                rng.uniform(0.3, 1.2)
            };
            SurfIn::Revolution {
                gen: CurveIn::Circle {
                    c: [0.0, 0.0, 0.0],
                    x: [1.0, 0.0, 0.0],
                    y: [0.0, 0.0, 1.0],
                    d: [d0, d1],
                    r,
                },
                point: [0.0, 0.0, 0.0],
                axis: [0.0, 0.0, 1.0],
                swap: rng.chance(0.3),
            }
        }
        1 => {
            // cone with its apex on the axis
            let r = rng.uniform(0.5, 2.0) * s;
            let h = rng.uniform(0.5, 2.0) * s;
            let (a, b) = if rng.chance(0.5) {
                ([0.0, 0.0, 0.0], [r, 0.0, h])
            } else {
                ([r, 0.0, 0.0], [0.0, 0.0, h])
            };
            SurfIn::Revolution {
                gen: CurveIn::Line { a, b },
                point: [0.0, 0.0, 0.0],
                axis: [0.0, 0.0, 1.0],
                swap: rng.chance(0.3),
            }
        }
        _ => {
            // bilinear-ish patch with the u = 0 row collapsed
            let p = 1 + rng.below(2) as usize;
            let q = 1 + rng.below(2) as usize;
            let nu = p + 1 + rng.below(2) as usize;
            let nv = q + 1 + rng.below(2) as usize;
            let apex = [0.5 * s, 0.5 * s, rng.uniform(-0.2, 0.2) * s];
            let net = (0..nu)
                .map(|i| {
                    (0..nv)
                        .map(|j| {
                            if i == 0 {
                                [apex[0], apex[1], apex[2], NOW]
                            } else {
                                let x = i as f64 / (nu - 1) as f64;
                                let y = j as f64 / (nv - 1) as f64;
                                [x * s, y * s, rng.uniform(-0.1, 0.1) * s, NOW]
                            }
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
    }
}

/// A uv of the domain `d`: inside, on a boundary or a corner (seams and poles lie on
/// the natural boundaries).
fn gen_uv(rng: &mut Rng, d: [f64; 4]) -> [f64; 2] {
    let mut uv = uv_in(rng, d);
    match rng.below(8) {
        0 => uv[0] = d[0],
        1 => uv[0] = d[2],
        2 => uv[1] = d[1],
        3 => uv[1] = d[3],
        4 => uv = [d[0], d[1]],
        5 => uv = [d[2], d[3]],
        _ => {}
    }
    uv
}

/// A surface point and its unit normal and first derivatives.
type Frame = ([f64; 3], [f64; 3], [f64; 3], [f64; 3]);

/// Surface point, unit normal and first derivatives at `uv` (`None` when the surface
/// cannot evaluate there).
fn frame(k: &Kernel, srf: &Surface, uv: [f64; 2]) -> Option<Frame> {
    let (p, su, sv) = srf
        .evaluate_1st_derivatives(k, Point2::new(uv[0], uv[1]), true, true)
        .ok()?;
    let n = norm3(cross3(arr3(su), arr3(sv)));
    Some((arr3(p), n, arr3(su), arr3(sv)))
}

/// A point curve at `p`: a B-spline whose control points coincide (or nearly: an
/// `SmLine` between coincident points is a unit line, not a point); with its interval.
fn point_curve(rng: &mut Rng, p: [f64; 3], s: f64) -> (CurveIn, [f64; 2]) {
    let eps = pick(rng, &[0.0, 0.0, 1.0e-15, 1.0e-13, 1.0e-11], 0.0) * s.max(1.0);
    let n = 2 + rng.below(3) as usize;
    let deg = (n - 1).min(1 + rng.below(2) as usize);
    let cpts = (0..n)
        .map(|i| {
            let q = if i == 0 || rng.chance(0.5) {
                p
            } else {
                add3(p, scale3(unit_dir(rng), eps))
            };
            [q[0], q[1], q[2], NOW]
        })
        .collect();
    let t0 = pick(rng, &[0.0, -1.0, 2.0], 0.0);
    let t1 = t0 + pick(rng, &[1.0, 0.5, 10.0], 1.0);
    (
        CurveIn::Bspline {
            p: deg,
            cpts,
            knots: gen_knots(rng, n, deg, t0, t1),
        },
        [t0, t1],
    )
}

/// The point curve at `p` of the fixtures (degree 1, two equal control points).
fn point_at(p: [f64; 3]) -> CurveIn {
    CurveIn::Bspline {
        p: 1,
        cpts: vec![[p[0], p[1], p[2], NOW], [p[0], p[1], p[2], NOW]],
        knots: vec![0.0, 0.0, 1.0, 1.0],
    }
}

/// The curve's whole parameter range or a random part of it, an interval overhanging the
/// range, one entirely past its end, a zero-length or inverted one, or one just above the
/// ffi guard's length bound (HANG-38).
fn gen_interval(rng: &mut Rng, range: [f64; 2]) -> [f64; 2] {
    let w = range[1] - range[0];
    let a = range[0] + rng.uniform(0.0, 0.4) * w;
    let b = range[0] + rng.uniform(0.6, 1.0) * w;
    match rng.below(100) {
        0..=39 => range,
        40..=64 => [a, b],
        65..=74 => [range[0] - 0.1 * w, range[1] + 0.1 * w],
        75..=79 => [range[0] + rng.uniform(-0.1, 0.1) * w, range[1] + 0.1 * w],
        80..=82 => [range[1] + 0.1 * w, range[1] + 0.5 * w],
        83..=86 => [a, a],
        87..=91 => [b, a],
        92..=94 => [range[1], range[0]],
        _ => {
            // length 3x the guard's bound (16000 ulp of the larger end)
            let m = a.abs().max(1.0);
            let len = 3.0 * 16000.0 * (m.next_up() - m);
            [a, a + len]
        }
    }
}

fn curve_range(c: &CurveIn) -> [f64; 2] {
    match c {
        CurveIn::Line { .. } => [0.0, 1.0],
        CurveIn::Circle { d, .. } => *d,
        CurveIn::Bspline { knots, .. } => [
            knots.first().copied().unwrap_or(0.0),
            knots.last().copied().unwrap_or(1.0),
        ],
    }
}

fn gen_tol(rng: &mut Rng, s: f64) -> f64 {
    // SmApproxTol3d takes fabs and maps SM_USE_DEFAULT on entry (SmTol.cpp:109,
    // SmTol.h:167-168): negative and default tolerances must run as |tol| / zone/2.
    if rng.chance(0.05) {
        return -pick(rng, &[1.0e-12, 1.0e-4, 5.0e-2, 1.0, 1.0e3], 1.0e-4) * s;
    }
    if rng.chance(0.03) {
        return solidcore::geom::tol::USE_DEFAULT;
    }
    if rng.chance(0.06) {
        // zero, tiny and huge
        return pick(rng, &[0.0, 1.0e-12, 10.0], 0.0) * s;
    }
    pick(rng, &[1.0e-6, 1.0e-4, 1.0e-3, 1.0e-2, 1.0e-8], 1.0e-4) * s
}

/// One case.
fn gen_case(rng: &mut Rng, k: &Kernel) -> Option<Inputs> {
    let s = gen_scale(rng);
    let surface = if rng.chance(0.35) {
        gen_pole_surface(rng, s)
    } else {
        gen_surface(rng, s)
    };
    let (srf, nat) = surface_with_domain(k, &surface)?;
    let mut domain = gen_domain(rng, nat);
    let tol = gen_tol(rng, s);
    let kind = rng.below(100);
    let (curve, mut interval) = if kind < 45 {
        // a point curve on (or off) the surface
        let uv = gen_uv(rng, domain);
        let (p, n, _, _) = frame(k, &srf, uv)?;
        let off = pick(rng, &[0.0, 0.0, 0.0, 1.0e-9, 1.0e-6, 1.0e-3, 0.5], 0.0) * s;
        point_curve(rng, add3(p, scale3(n, off)), s)
    } else if kind < 65 {
        // a line along the surface normal, from the surface or off it
        let uv = gen_uv(rng, domain);
        let (p, n, _, _) = frame(k, &srf, uv)?;
        let len = pick(rng, &[0.1, 1.0, 1.0e-3], 1.0) * s;
        let start_off = pick(rng, &[0.0, 0.0, 1.0e-9, 1.0e-3, 0.5], 0.0) * s;
        let sign = if rng.chance(0.5) { 1.0 } else { -1.0 };
        let a = add3(p, scale3(n, sign * start_off));
        let b = add3(a, scale3(n, sign * len));
        (CurveIn::Line { a, b }, [0.0, 1.0])
    } else {
        // a curve near the surface (mostly deferred: the offset surface)
        let c = gen_curve_near(k, rng, &srf, domain, s)?;
        let r = curve_range(&c);
        (c, r)
    };
    if rng.chance(0.4) {
        interval = gen_interval(rng, interval);
    }

    // domain variations: uncontained, degenerate, inverted
    match rng.below(20) {
        0 => {
            let w = (nat[2] - nat[0]).abs().max(1.0e-3);
            domain = [nat[0] - 0.1 * w, nat[1], nat[2], nat[3]];
        }
        1 => domain = [nat[0], nat[1], nat[0], nat[3]],
        2 => domain = [nat[0], nat[1], nat[2], nat[1]],
        3 => {
            let e = 1.0e-9 * (1.0 + nat[0].abs());
            domain = [nat[0], nat[1], nat[2] + e, nat[3]];
        }
        4 => domain = [nat[2], nat[1], nat[0], nat[3]],
        5 => domain = [nat[0], nat[3], nat[2], nat[1]],
        _ => {}
    }

    Some(Inputs {
        surface,
        curve,
        domain,
        interval,
        tol,
        keep_all: rng.chance(0.7),
        want_record: rng.chance(0.8),
    })
}

fn gen_inputs(rng: &mut Rng) -> Inputs {
    let k = Kernel::default();
    loop {
        if let Some(i) = gen_case(rng, &k) {
            if ffi::encode(&ffi_drop(&i)).is_ok() {
                return i;
            }
        }
    }
}

// ---- fixtures -----------------------------------------------------------------------

/// The unit square plane.
fn square_plane() -> SurfIn {
    SurfIn::Plane {
        o: [0.0, 0.0, 0.0],
        x: [1.0, 0.0, 0.0],
        y: [0.0, 1.0, 0.0],
        scale: [1.0, 1.0],
        domain: [0.0, 0.0, 1.0, 1.0],
    }
}

/// A cone of base radius 1 and height 1 about the z axis, apex at `(0, 0, 1)`.
fn cone_to_apex() -> SurfIn {
    SurfIn::Revolution {
        gen: CurveIn::Line {
            a: [1.0, 0.0, 0.0],
            b: [0.0, 0.0, 1.0],
        },
        point: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        swap: false,
    }
}

/// A sphere of radius `r` about the z axis (poles at `v = -pi/2, pi/2`).
fn sphere(r: f64) -> SurfIn {
    let half = core::f64::consts::FRAC_PI_2;
    SurfIn::Revolution {
        gen: CurveIn::Circle {
            c: [0.0, 0.0, 0.0],
            x: [1.0, 0.0, 0.0],
            y: [0.0, 0.0, 1.0],
            d: [-half, half],
            r,
        },
        point: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        swap: false,
    }
}

/// A cylinder of radius 1 and height 2 about the z axis (seam at `u = 0`).
fn cylinder() -> SurfIn {
    SurfIn::Revolution {
        gen: CurveIn::Line {
            a: [1.0, 0.0, 0.0],
            b: [1.0, 0.0, 2.0],
        },
        point: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        swap: false,
    }
}

fn line(a: [f64; 3], b: [f64; 3]) -> CurveIn {
    CurveIn::Line { a, b }
}

fn fixture(surface: SurfIn, curve: CurveIn, domain: [f64; 4], interval: [f64; 2]) -> Inputs {
    Inputs {
        surface,
        curve,
        domain,
        interval,
        tol: 1.0e-4,
        keep_all: true,
        want_record: true,
    }
}

fn fixtures() -> Vec<(String, Inputs)> {
    let unit = [0.0, 0.0, 1.0, 1.0];
    let mut v = vec![
        // TestSmCreateRectangle shape: an edge of the unit square (deferred: offset)
        (
            "plane_edge".to_owned(),
            fixture(
                square_plane(),
                line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                unit,
                [0.0, 1.0],
            ),
        ),
        // a point curve inside the square
        (
            "plane_point".to_owned(),
            fixture(square_plane(), point_at([0.5, 0.25, 0.0]), unit, [0.0, 1.0]),
        ),
        // a point curve above the square (a good first drop along the normal, then no
        // next point: no curve)
        (
            "plane_point_above".to_owned(),
            fixture(square_plane(), point_at([0.5, 0.25, 0.5]), unit, [0.0, 1.0]),
        ),
        // negative and default tolerances run as |tol| / zone/2 (PR #136 review)
        (
            "plane_point_tol_neg1".to_owned(),
            Inputs {
                tol: -1.0,
                ..fixture(square_plane(), point_at([0.5, 0.5, 0.1]), unit, [0.0, 1.0])
            },
        ),
        (
            "plane_point_tol_neg_small".to_owned(),
            Inputs {
                tol: -1.0e-4,
                ..fixture(square_plane(), point_at([0.5, 0.5, 0.1]), unit, [0.0, 1.0])
            },
        ),
        (
            "plane_point_tol_pos1".to_owned(),
            Inputs {
                tol: 1.0,
                ..fixture(square_plane(), point_at([0.5, 0.5, 0.1]), unit, [0.0, 1.0])
            },
        ),
        (
            "plane_point_tol_use_default".to_owned(),
            Inputs {
                tol: solidcore::geom::tol::USE_DEFAULT,
                ..fixture(square_plane(), point_at([0.5, 0.5, 0.1]), unit, [0.0, 1.0])
            },
        ),
        // a line along the normal of the square
        (
            "plane_normal_line".to_owned(),
            fixture(
                square_plane(),
                line([0.5, 0.5, 0.0], [0.5, 0.5, 1.0]),
                unit,
                [0.0, 1.0],
            ),
        ),
        // a domain larger than the plane's
        (
            "plane_uncontained".to_owned(),
            fixture(
                square_plane(),
                line([0.5, 0.5, 0.0], [0.6, 0.5, 0.0]),
                [-0.5, 0.0, 1.0, 1.0],
                [0.0, 1.0],
            ),
        ),
        // a degenerate domain (natural domain used)
        (
            "plane_degenerate_domain".to_owned(),
            fixture(
                square_plane(),
                point_at([0.5, 0.5, 0.0]),
                [0.5, 0.0, 0.5, 1.0],
                [0.0, 1.0],
            ),
        ),
        // (a plane whose natural domain has no width or height is not compared: the
        // shared plane encoder of `intersect.surface_drop_lib_p2` rejects it, UB-68 in
        // `plan/upstream-issues.md`)
        // the apex of a cone (a surface pole) with a point curve there: the pole line
        // (a degenerate curve on a pole)
        (
            "cone_apex_pole_line".to_owned(),
            Inputs {
                surface: cone_to_apex(),
                curve: point_at([0.0, 0.0, 1.0]),
                domain: [0.0; 4],
                interval: [0.0, 1.0],
                tol: 1.0e-4,
                keep_all: true,
                want_record: true,
            },
        ),
        // lines along the axis through a sphere pole: the drop of the start lands on the
        // pole, so FindDegenParamForDirection runs (SM_CTC_BAD_FIND_DEGENPARAM_WARNING).
        // All of them defer on the offset surface today (the tangent is (anti-)parallel to
        // the normal, yet `SetProperties` still reaches `rOffset.DropVectors`), so they
        // only compare once surface.offset_surface (PR #62) lands.
        (
            "sphere_axis_line_n_up_from_pole".to_owned(),
            Inputs {
                surface: sphere(1.0),
                curve: line([0.0, 0.0, 1.0], [0.0, 0.0, 1.5]),
                domain: [0.0; 4],
                interval: [0.0, 1.0],
                tol: 1.0e-4,
                keep_all: true,
                want_record: true,
            },
        ),
        (
            "sphere_axis_line_n_down_from_pole".to_owned(),
            Inputs {
                surface: sphere(1.0),
                curve: line([0.0, 0.0, 1.0], [0.0, 0.0, 0.5]),
                domain: [0.0; 4],
                interval: [0.0, 1.0],
                tol: 1.0e-4,
                keep_all: true,
                want_record: true,
            },
        ),
        (
            "sphere_axis_line_n_up_from_above".to_owned(),
            Inputs {
                surface: sphere(1.0),
                curve: line([0.0, 0.0, 1.5], [0.0, 0.0, 2.5]),
                domain: [0.0; 4],
                interval: [0.0, 1.0],
                tol: 1.0e-4,
                keep_all: true,
                want_record: true,
            },
        ),
        (
            "sphere_axis_line_n_down_from_above".to_owned(),
            Inputs {
                surface: sphere(1.0),
                curve: line([0.0, 0.0, 1.5], [0.0, 0.0, 0.5]),
                domain: [0.0; 4],
                interval: [0.0, 1.0],
                tol: 1.0e-4,
                keep_all: true,
                want_record: true,
            },
        ),
        (
            "sphere_axis_line_s_down_from_pole".to_owned(),
            Inputs {
                surface: sphere(1.0),
                curve: line([0.0, 0.0, -1.0], [0.0, 0.0, -1.5]),
                domain: [0.0; 4],
                interval: [0.0, 1.0],
                tol: 1.0e-4,
                keep_all: true,
                want_record: true,
            },
        ),
        (
            "sphere_axis_line_s_up_from_pole".to_owned(),
            Inputs {
                surface: sphere(1.0),
                curve: line([0.0, 0.0, -1.0], [0.0, 0.0, -0.5]),
                domain: [0.0; 4],
                interval: [0.0, 1.0],
                tol: 1.0e-4,
                keep_all: true,
                want_record: true,
            },
        ),
        (
            "sphere_axis_line_s_down_from_below".to_owned(),
            Inputs {
                surface: sphere(1.0),
                curve: line([0.0, 0.0, -1.5], [0.0, 0.0, -2.5]),
                domain: [0.0; 4],
                interval: [0.0, 1.0],
                tol: 1.0e-4,
                keep_all: true,
                want_record: true,
            },
        ),
        (
            "sphere_axis_line_s_up_from_below".to_owned(),
            Inputs {
                surface: sphere(1.0),
                curve: line([0.0, 0.0, -1.5], [0.0, 0.0, -0.5]),
                domain: [0.0; 4],
                interval: [0.0, 1.0],
                tol: 1.0e-4,
                keep_all: true,
                want_record: true,
            },
        ),
        // a zero-length interval
        (
            "plane_zero_length_interval".to_owned(),
            fixture(
                square_plane(),
                line([0.5, 0.5, 0.0], [0.5, 0.5, 1.0]),
                unit,
                [0.5, 0.5],
            ),
        ),
    ];
    // point curves on the sphere's poles and seam, on the cylinder's seam (the four pole
    // ones end at FirstDropUncontainedFail; the pole line is reached by cone_apex_pole_line)
    for (name, srf, p) in [
        ("sphere_north_pole", sphere(1.0), [0.0, 0.0, 1.0]),
        ("sphere_south_pole", sphere(1.0), [0.0, 0.0, -1.0]),
        ("sphere_seam", sphere(1.0), [1.0, 0.0, 0.0]),
        ("sphere_big_pole", sphere(1.0e3), [0.0, 0.0, 1.0e3]),
        ("cylinder_seam", cylinder(), [1.0, 0.0, 1.0]),
        ("cylinder_corner", cylinder(), [1.0, 0.0, 0.0]),
    ] {
        let curve = point_at(p);
        v.push((
            name.to_owned(),
            Inputs {
                surface: srf,
                curve,
                domain: [0.0; 4],
                interval: [0.0, 1.0],
                tol: 1.0e-4,
                keep_all: true,
                want_record: true,
            },
        ));
    }
    // the natural domain of the fixtures above that used a placeholder
    let k = Kernel::default();
    for (_, i) in &mut v {
        if i.domain == [0.0; 4] {
            if let Some((_, nat)) = surface_with_domain(&k, &i.surface) {
                i.domain = nat;
            }
        }
    }
    // a line along the cylinder's seam (deferred: offset)
    v.push((
        "cylinder_seam_line".to_owned(),
        Inputs {
            surface: cylinder(),
            curve: line([1.0, 0.0, 0.0], [1.0, 0.0, 2.0]),
            domain: surface_with_domain(&k, &cylinder()).map_or([0.0, 0.0, 1.0, 1.0], |x| x.1),
            interval: [0.0, 1.0],
            tol: 1.0e-4,
            keep_all: false,
            want_record: true,
        },
    ));
    v
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "intersect.surface_drop_lib_p3"
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        let mut cases = Vec::new();
        for (name, i) in fixtures() {
            let v = serde_json::to_value(&i).map_err(|e| harness(format!("fixture: {e}")))?;
            cases.push(Case {
                unit: self.id().to_owned(),
                name: format!("fixture_{name}"),
                source: CaseSource::Inline,
                inputs: v,
            });
        }
        cases.extend(default_cases(self, &ctx)?);
        Ok(cases)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_inputs(rng)).ok()
    }
}
