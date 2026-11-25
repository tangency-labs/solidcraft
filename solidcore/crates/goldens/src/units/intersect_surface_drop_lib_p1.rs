//! Runner for unit `intersect.surface_drop_lib_p1`: the drop point record `SmDropPt`
//! (constructor, `operator=`, `SetUninitialized`, `IsGoodTangentDrop`,
//! `IsMovingAlongBoundary`, `TestSpanAccuracy`, `CheckSeamJump`, `FixBoundaryTangents`),
//! the span stack (`sm_PushPoint`, `sm_PopStack`, `sm_StackLook`), `sm_FixBoundaryCurves`,
//! `sm_RemoveDuplicateDropPointSolutions`, `SmSurface::DropIsoCurve` and
//! `SmSurface::DropAndTrimCurve`.
//!
//! Ported from upstream `source/SMLib/src/SmSurfaceDropLib.cpp` (on `SmPlane`,
//! `SmBSplineSurface` and `SmSurfOfRevolution` surfaces and `SmLine`, `SmCircle` and
//! `SmBSplineCurve` curves); checked against its golden.
//! Rust: `solidcore::intersect::surface_drop_lib::p1`.
//!
//! A case is one op; the Rust side produces a flat record, reported as the values `rec`.
//! Inputs undefined upstream are [`REJECTED`] (the runner applies the predicate with the
//! kernel's evaluations).
//!
//! Deferred cases: `TestSpanAccuracy` spans that reach `smsurf_IsPointOnNormal`
//! (`surface.surface_p3`), `DropAndTrimCurve`s whose trace reaches unported code
//! (an `SmCrvOnSurf` over a plane or surface of revolution, `curve.crv_on_surf`;
//! non-plane `SmSurface::DropCurve`, `intersect.surface_drop_lib_p3`; the classification
//! `ReSet` / `ConnectIntervals` stubs, `intersect.curve_class_p2`), and surface
//! evaluations of kinds whose evaluators are not ported yet.
//!
//! Curves and surfaces may be upstream `.dat` files (database version 30), read as
//! `SmBSplineCurve::ReadFromFile` / `SmBSplineSurface::ReadFromFile` read them
//! (`read_file_db`, then `ReadFromDB`); a `Trim` case
//! without a domain or interval uses the natural ones, as upstream's `prog_test` does.
//!
//! At least [`MIN_GENERATED`] generated cases run, then [`TRIM_CASES`] fixed-seed
//! `DropAndTrimCurve` cases (`trim0000` ..).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p2::BSPLINE_CURVE_TYPE;
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::curve::line::Line;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::solution_array::{Solution, SolutionArray};
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::surface_drop_lib::p1::{
    fix_boundary_curves, pop_stack, push_point, remove_duplicate_drop_point_solutions,
    stack_look, DropPt,
};
use solidcore::kernel::Kernel;
use solidcore::math::{EFF_ZERO, UNDEF_DOUBLE};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surf_of_revolution::SurfOfRevolution;
use solidcore::surface::surface::p2::B_SPLINE_SURFACE_TYPE;
use solidcore::surface::surface::Surface;
use solidcore::topology::database_io::{DatabaseReader, FileType};
use solidcore::topology::ObjectHandle;
use solidcore::SolidError;

use crate::case::{CaseContext, CaseSource};
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The record of an op undefined upstream (not run).
const REJECTED: f64 = -2.0;
/// The record of an op whose surface of revolution could not be made.
const NO_SURFACE: f64 = -3.0;
/// Values per drop point.
const PT_LEN: usize = 38;

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1500;

/// `DropAndTrimCurve` cases appended after the generated ones ([`trim_cases`]).
pub const TRIM_CASES: u64 = 400;

/// First seed of the [`trim_cases`] (seed `TRIM_SEED + i` for case `trim{i:04}`).
const TRIM_SEED: u64 = 0x5d1b_7a1c_0000_0000;

/// `intersect.surface_drop_lib_p1` runner.
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
    /// An upstream `.dat` file read as `SmBSplineCurve::ReadFromFile` does.
    File {
        file: String,
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
    /// An upstream `.dat` file read as `SmBSplineSurface::ReadFromFile` does.
    File {
        file: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum StackIn {
    Push { t: f64, uv: [f64; 2], duv: [f64; 2] },
    Pop,
    Look,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    Ctor {
        t: f64,
        degen: u8,
    },
    Assign {
        pt: Vec<f64>,
    },
    Uninit {
        pt: Vec<f64>,
    },
    GoodTangent {
        pt: Vec<f64>,
        on_pole: u8,
    },
    Moving {
        surface: SurfIn,
        pt: Vec<f64>,
        domain: [f64; 4],
        tol: [f64; 2],
        ang: f64,
        test: u8,
    },
    Span {
        surface: SurfIn,
        curve: CurveIn,
        domain: [f64; 4],
        start: Vec<f64>,
        end: Vec<f64>,
        tol: f64,
        uv_tol: [f64; 2],
        refine: bool,
    },
    Seam {
        surface: SurfIn,
        closure: u8,
        tol: f64,
        start: Vec<f64>,
        end: Vec<f64>,
    },
    FixTangents {
        domain: [f64; 4],
        start: Vec<f64>,
        end: Vec<f64>,
        uv_tol: [f64; 2],
        tol: f64,
    },
    Stack {
        ops: Vec<StackIn>,
    },
    FixCurves {
        domain: [f64; 4],
        uv_tol: [f64; 2],
        poly: Vec<[f64; 3]>,
    },
    Dups {
        uv_tol: [f64; 2],
        uvs: Vec<[f64; 2]>,
    },
    Iso {
        surface: SurfIn,
        curve: CurveIn,
        domain: [f64; 4],
        interval: [f64; 2],
        start_uv: [f64; 2],
        tol: f64,
    },
    Trim {
        surface: SurfIn,
        curve: CurveIn,
        /// `None`: the surface's natural UV domain.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        domain: Option<[f64; 4]>,
        /// `None`: the curve's natural interval.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        interval: Option<[f64; 2]>,
        tol: f64,
        keep_all: bool,
        on_surface: bool,
    },
}

// ---- Rust side ----------------------------------------------------------------------

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn p2(p: [f64; 2]) -> Point2 {
    Vec2::new(p[0], p[1])
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
        CurveIn::File { file } => {
            let mut db = read_file_db(file, b'C')?;
            Curve::read_from_db(k, BSPLINE_CURVE_TYPE, &mut db, 3, 30)
                .map_err(|e| harness(format!("{file}: {e}")))
        }
    }
}

/// The ASCII database stream `SmCurve::ReadFromFile` / `SmSurface::ReadFromFile` read
/// `path` from, for a file without an `Object` header (database version 30): a leading
/// line feed is skipped, and a first line starting with `header` (`C` / `S`) too.
fn read_file_db(path: &str, header: u8) -> Result<DatabaseReader, HarnessError> {
    let bytes = std::fs::read(path).map_err(|e| harness(format!("{path}: {e}")))?;
    let mut rest: &[u8] = &bytes;
    if let Some((b'\n', tail)) = rest.split_first() {
        rest = tail;
    }
    match rest.first() {
        Some(b'O') => return Err(harness(format!("{path}: versioned file not supported"))),
        Some(&c) if c == header => {
            let at = rest.iter().position(|&b| b == b'\n').map_or(rest.len(), |p| p + 1);
            rest = rest.get(at..).unwrap_or(&[]);
        }
        _ => {}
    }
    let mut db = DatabaseReader::new();
    db.open_for_read(rest.to_vec(), FileType::Ascii);
    Ok(db)
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
        } => Some(Surface::from(Plane::new(
            v3(*o),
            v3(*x),
            v3(*y),
            Vec2::new(scale[0], scale[1]),
            &ext(*domain),
        ))),
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
        SurfIn::File { file } => {
            let mut db = read_file_db(file, b'S')?;
            Some(
                Surface::read_from_db(k, B_SPLINE_SURFACE_TYPE, &mut db, 30)
                    .map_err(|e| harness(format!("{file}: {e}")))?,
            )
        }
    })
}

fn pt_in(v: &[f64]) -> Result<DropPt, HarnessError> {
    if v.len() != PT_LEN {
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

/// `put_curve` of a traced UV curve (`SmBSplineCurve*` upstream, an `SmLine` with its
/// NURB made by `GetOrCreateGwNurbPointer`); `None` for a curve of no B-spline kind.
fn put_uv_curve(o: &mut Vec<f64>, c: &Curve) -> Option<solidcore::Result<()>> {
    Some(match c.kind() {
        CurveKind::BSpline(b) => put_curve(o, b),
        CurveKind::Line(l) => l.base_with_nurb().and_then(|b| put_curve(o, &b)),
        CurveKind::Ellipse(e) => put_curve(o, e.base()),
        CurveKind::Hyperbola(h) => put_curve(o, h.base()),
        CurveKind::Parabola(p) => put_curve(o, p.base()),
        _ => return None,
    })
}

/// The `SmStatus` of a Rust error.
fn status(e: &SolidError) -> f64 {
    crate::status::kind_slot(e, legacy_status(e))
}

/// [`status`] as recorded before kinds.
fn legacy_status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007.0,
        e if solidcore::error::upstream_status(e).is_some() => {
            f64::from(solidcore::error::upstream_status(e).unwrap_or(-1))
        }
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

fn test_bdry(c: u8) -> SurfParamType {
    match c {
        0 => SurfParamType::U,
        1 => SurfParamType::V,
        2 => SurfParamType::Unknown,
        3 => SurfParamType::Both,
        _ => SurfParamType::Neither,
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

/// The shim's `span_undefined`: a Hermite sample outside the domain whose point or
/// clamped point does not evaluate.
fn span_undefined(k: &Kernel, 
    s: &Surface,
    d: &Extent2d,
    a: &DropPt,
    e: &DropPt,
) -> Result<bool, Fail> {
    let ivl = Extent1d::from_min_max(a.t, e.t);
    let dt = e.t - a.t;
    if dt < ivl.length() / 1000.0 {
        return Ok(false);
    }
    let h = HermiteCurve::from_2d(a.uv_pd[0], a.uv_pd[1] * dt, e.uv_pd[0], e.uv_pd[1] * dt, 2);
    for ii in 0..3u32 {
        let nt = (f64::from(ii) + 1.0) / (3.0 + 1.0);
        let hp = h.evaluate_point(nt);
        let uv = Point2::new(hp.x, hp.y);
        if !d.contains_point(uv, EFF_ZERO)
            && (st(s.evaluate_point(k, uv))?.is_err()
                || st(s.evaluate_point(k, d.clamp_point(uv)))?.is_err())
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn run_op(k: &Kernel, i: &Inputs, o: &mut Vec<f64>) -> Result<(), Fail> {
    match i {
        Inputs::Ctor { t, degen } => put_pt(o, &DropPt::new(*t, tri_in(*degen))),
        Inputs::Assign { pt } => {
            let src = pt_in(pt)?;
            let mut dst = DropPt::new(1.0, Some(true));
            dst.assign(&src);
            let same = dst;
            dst.assign(&same);
            put_pt(o, &dst);
        }
        Inputs::Uninit { pt } => {
            let mut p = pt_in(pt)?;
            p.set_uninitialized();
            put_pt(o, &p);
        }
        Inputs::GoodTangent { pt, on_pole } => {
            let p = pt_in(pt)?;
            o.push(b(p.is_good_tangent_drop(tri_in(*on_pole))));
        }
        Inputs::Moving {
            surface,
            pt,
            domain,
            tol,
            ang,
            test,
        } => {
            let Some(s) = rust_surface(k, surface)? else {
                o.push(crate::status::state_slot(crate::status::RunnerState::NotReached, NO_SURFACE));
                return Ok(());
            };
            let p = pt_in(pt)?;
            match st(p.is_moving_along_boundary(
                k, &s,
                &ext(*domain),
                tol[0],
                tol[1],
                *ang,
                test_bdry(*test),
            ))? {
                Ok(poc) => o.push(f64::from(poc.code())),
                Err(kernel_status) => o.push(crate::status::legacy_or(REJECTED, || kernel_status)),
            }
        }
        Inputs::Span {
            surface,
            curve,
            domain,
            start,
            end,
            tol,
            uv_tol,
            refine,
        } => {
            let Some(s) = rust_surface(k, surface)? else {
                o.push(crate::status::state_slot(crate::status::RunnerState::NotReached, NO_SURFACE));
                return Ok(());
            };
            let c = rust_curve(k, curve)?;
            let d = ext(*domain);
            let a = pt_in(start)?;
            let e = pt_in(end)?;
            if span_undefined(k, &s, &d, &a, &e)? {
                o.push(crate::status::state_slot(crate::status::RunnerState::Skipped, REJECTED));
                return Ok(());
            }
            match st(a.test_span_accuracy(k, &s, &d, &c, &e, *tol, uv_tol[0], uv_tol[1], *refine))? {
                Ok(r) => o.extend([
                    0.0,
                    b(r.passes),
                    r.max_drop_to_surf,
                    r.max_drop_to_surf_param,
                    r.max_approx_dev,
                ]),
                Err(code) => o.push(code),
            }
        }
        Inputs::Seam {
            surface,
            closure: cl,
            tol,
            start,
            end,
        } => {
            let Some(s) = rust_surface(k, surface)? else {
                o.push(crate::status::state_slot(crate::status::RunnerState::NotReached, NO_SURFACE));
                return Ok(());
            };
            let a = pt_in(start)?;
            let mut e = pt_in(end)?;
            match st(a.check_seam_jump(k, &s, closure(*cl), *tol, &mut e))? {
                Ok(()) => o.push(crate::status::ok_slot(0.0)),
                Err(code) => o.push(code),
            }
            put_pt(o, &e);
        }
        Inputs::FixTangents {
            domain,
            start,
            end,
            uv_tol,
            tol,
        } => {
            let mut a = pt_in(start)?;
            let mut e = pt_in(end)?;
            let ch = a.fix_boundary_tangents(&ext(*domain), &mut e, uv_tol[0], uv_tol[1], *tol);
            o.push(0.0);
            o.push(b(ch));
            put_pt(o, &a);
            put_pt(o, &e);
        }
        Inputs::Stack { ops } => {
            let mut stack = Vec::new();
            for s in ops {
                let r = match s {
                    StackIn::Push { t, uv, duv } => {
                        push_point(*t, [p2(*uv), p2(*duv)], &mut stack);
                        continue;
                    }
                    StackIn::Pop => pop_stack(&mut stack),
                    StackIn::Look => stack_look(&stack),
                };
                match st(r)? {
                    Ok((t, uv)) => o.extend([crate::status::ok_slot(0.0), t, uv[0].x, uv[0].y, uv[1].x, uv[1].y]),
                    Err(code) => o.push(code),
                }
            }
            o.push(stack.len() as f64);
            o.extend_from_slice(&stack);
        }
        Inputs::FixCurves {
            domain,
            uv_tol,
            poly,
        } => {
            let mut pts: Vec<Vec3> = poly.iter().map(|p| v3(*p)).collect();
            match st(fix_boundary_curves(&mut pts, &ext(*domain), uv_tol[0], uv_tol[1]))? {
                Ok(ch) => {
                    o.push(b(ch));
                    for p in pts {
                        put_vec(o, p);
                    }
                }
                Err(kernel_status) => o.push(crate::status::legacy_or(REJECTED, || kernel_status)),
            }
        }
        Inputs::Dups { uv_tol, uvs } => {
            let mut sols: SolutionArray<ObjectHandle> = SolutionArray::new();
            for (n, uv) in uvs.iter().enumerate() {
                let mut s = Solution::new();
                s.start.parameters[0] = uv[0];
                s.start.parameters[1] = uv[1];
                s.start.solution_value = n as f64;
                sols.add(&s);
            }
            remove_duplicate_drop_point_solutions(&mut sols, uv_tol[0], uv_tol[1]);
            o.push(sols.len() as f64);
            for s in sols.as_slice() {
                o.extend([
                    s.start.solution_value,
                    s.start.parameters[0],
                    s.start.parameters[1],
                ]);
            }
        }
        Inputs::Iso {
            surface,
            curve,
            domain,
            interval,
            start_uv,
            tol,
        } => {
            let Some(s) = rust_surface(k, surface)? else {
                o.push(crate::status::state_slot(crate::status::RunnerState::NotReached, NO_SURFACE));
                return Ok(());
            };
            let c = rust_curve(k, curve)?;
            let r = s.drop_iso_curve(
                k, &ext(*domain),
                &c,
                &Extent1d::from_min_max(interval[0], interval[1]),
                p2(*start_uv),
                *tol,
            );
            match st(r)? {
                Ok(r) => {
                    o.extend([
                        0.0,
                        b(r.successful),
                        r.max_drop_to_surf,
                        r.max_approx_dev,
                        b(r.iso_uv_curve.is_some()),
                    ]);
                    if let Some(cv) = &r.iso_uv_curve {
                        st(put_curve(o, cv))?.map_err(|_| harness("iso curve data"))?;
                    }
                }
                Err(code) => o.push(code),
            }
        }
        Inputs::Trim {
            surface,
            curve,
            domain,
            interval,
            tol,
            keep_all,
            on_surface,
        } => {
            let Some(s) = rust_surface(k, surface)? else {
                o.push(crate::status::state_slot(crate::status::RunnerState::NotReached, NO_SURFACE));
                return Ok(());
            };
            let c = rust_curve(k, curve)?;
            let dom = match domain { Some(d) => ext(*d), None => s.natural_uv_domain_with_nurb(k).map_err(|e| HarnessError::new(e.to_string()))? };
            let ivl = interval.map_or_else(
                || c.natural_interval(),
                |i| Extent1d::from_min_max(i[0], i[1]),
            );
            let r = s.drop_and_trim_curve::<ObjectHandle>(
                k,
                &dom,
                &c,
                &ivl,
                *tol,
                *keep_all,
                *on_surface,
                None,
            );
            match st(r)? {
                Ok(r) => {
                    o.extend([
                        0.0,
                        r.max_drop_to_surf,
                        r.max_approx_dev,
                        r.uv_curves.len() as f64,
                    ]);
                    for cv in &r.uv_curves {
                        // the curve's GetType() (an SmLine or an SmBSplineCurve), then its data
                        o.push(cv.type_code() as f64);
                        let put = put_uv_curve(o, cv).ok_or_else(|| harness("trim curve kind"))?;
                        st(put)?.map_err(|_| harness("trim curve data"))?;
                    }
                }
                Err(code) => o.push(code),
            }
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

/// The surface of an `iso` or `trim` case as a one-face sheet over the op's UV domain (the
/// case's, or for a `trim` without one the surface's natural domain), labelled `surface`
/// (for `examples/case_smb.rs`). The dropped UV curves are open pieces, not loops, so the
/// sheet does not show them.
///
/// # Errors
///
/// The case's inputs are bad, its op has no surface (every op but `iso` and `trim`), or
/// its surface of revolution cannot be made.
pub fn result_breps(case: &Case) -> Result<super::surface_surface_p2::Drawn, HarnessError> {
    let cannot = |why: String| harness(format!("case {}: cannot be drawn: {why}", case.name));
    let mut k = Kernel::default();
    let (surface, domain) = match case.parse::<Inputs>()? {
        Inputs::Iso {
            surface, domain, ..
        } => (surface, Some(domain)),
        Inputs::Trim {
            surface, domain, ..
        } => (surface, domain),
        _ => return Err(cannot("the op has no surface (only iso and trim do)".into())),
    };
    let s = rust_surface(&k, &surface)?
        .ok_or_else(|| cannot("the surface of revolution cannot be made".into()))?;
    let dom = match domain {
        Some(d) => ext(d),
        None => s
            .natural_uv_domain_with_nurb(&k)
            .map_err(|e| cannot(format!("natural UV domain: {e}")))?,
    };
    let b = super::surface_surface_p2::sheet(&mut k, s, &dom)
        .map_err(|e| harness(format!("case {}: {e}", case.name)))?;
    Ok((k, vec![("surface", b)]))
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

fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
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
    let mut v = Vec::with_capacity(PT_LEN);
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
    // (runner helper that does not return Result)
    let d = srf.natural_uv_domain_if_built();
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

fn gen_moving(rng: &mut Rng, k: &Kernel) -> Option<Inputs> {
    let s = gen_scale(rng);
    let surface = gen_surface(rng, s);
    let (srf, nat) = surface_with_domain(k, &surface)?;
    let domain = gen_domain(rng, nat);
    let tol = gen_uv_tol(rng, domain);
    let uv = uv_on_boundary(rng, domain, tol);
    // a tangent in the surface tangent plane (or a random one)
    let (_, su, sv) = srf
        .evaluate_1st_derivatives(k, Point2::new(uv[0], uv[1]), true, true)
        .ok()?;
    let a = rng.uniform(-1.0, 1.0);
    let c = rng.uniform(-1.0, 1.0);
    let mut tan = arr3(su * a + sv * c);
    match rng.below(8) {
        0 => tan = scale3(unit_dir(rng), s),
        1 => tan = arr3(sv),
        2 => tan = arr3(-su),
        3 => tan = [0.0, 0.0, 0.0],
        _ => {}
    }
    let crv = [rng.point3(-1.0, 1.0), tan, rng.point3(-1.0, 1.0)];
    let t = rng.unit();
    let pt = gen_pt(rng, uv, [0.0, 0.0], crv, t);
    Some(Inputs::Moving {
        surface,
        pt,
        domain,
        tol,
        ang: pick(rng, &[5.0, 5.0, 1.0, 30.0, 0.0, 89.0], 5.0),
        test: rng.below(5) as u8,
    })
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

fn gen_span(rng: &mut Rng, k: &Kernel) -> Option<Inputs> {
    let s = gen_scale(rng);
    let surface = gen_surface(rng, s);
    let (srf, nat) = surface_with_domain(k, &surface)?;
    let domain = gen_domain(rng, nat);
    let curve = gen_curve_near(k, rng, &srf, domain, s)?;
    let c = rust_curve(k, &curve).ok()?;
    let ivl = c.natural_interval();
    let mut ta = rng.uniform(ivl.min(), ivl.max());
    let mut tb = rng.uniform(ivl.min(), ivl.max());
    if rng.chance(0.7) && tb < ta {
        core::mem::swap(&mut ta, &mut tb);
    }
    let w = [domain[2] - domain[0], domain[3] - domain[1]];
    let mut uva = uv_in(rng, domain);
    let mut uvb = uv_in(rng, domain);
    if rng.chance(0.4) {
        // push the span ends to / beyond the boundary
        uva[0] = domain[0] + pick(rng, &[0.0, -0.01, -0.3], 0.0) * w[0];
        uvb[0] = domain[0] + pick(rng, &[0.0, -1.0e-13, -0.5], 0.0) * w[0];
    }
    let mag = pick(rng, &[0.0, 1.0, 5.0, 50.0], 1.0);
    let da = [rng.uniform(-mag, mag) * w[0], rng.uniform(-mag, mag) * w[1]];
    let db = [rng.uniform(-mag, mag) * w[0], rng.uniform(-mag, mag) * w[1]];
    let crv = [[0.0; 3]; 3];
    Some(Inputs::Span {
        surface,
        curve,
        domain,
        start: gen_pt(rng, uva, da, crv, ta),
        end: gen_pt(rng, uvb, db, crv, tb),
        tol: pick(rng, &[1.0e-6, 1.0e-3, 1.0e-9, 0.1], 1.0e-6) * s,
        uv_tol: gen_uv_tol(rng, domain),
        refine: rng.chance(0.5),
    })
}

fn gen_seam(rng: &mut Rng, k: &Kernel) -> Option<Inputs> {
    let s = gen_scale(rng);
    let surface = if rng.chance(0.8) {
        gen_revolution(rng, s)
    } else {
        gen_bspline_surface(rng, s)
    };
    let (_, nat) = surface_with_domain(k, &surface)?;
    let w = [nat[2] - nat[0], nat[3] - nat[1]];
    // start near one side, end near the other (or across the middle)
    let near = |rng: &mut Rng, lo: f64, hi: f64, frac: f64| match rng.below(4) {
        0 => lo,
        1 => hi,
        2 => lo + frac * (hi - lo),
        _ => hi - frac * (hi - lo),
    };
    let fa = pick(rng, &[0.0, 1.0e-12, 1.0e-6, 0.01, 0.2], 0.01);
    let fb = pick(rng, &[0.0, 1.0e-12, 1.0e-6, 0.01, 0.2], 0.01);
    let uva = [near(rng, nat[0], nat[2], fa), rng.uniform(nat[1], nat[3])];
    let mut uvb = [near(rng, nat[0], nat[2], fb), rng.uniform(nat[1], nat[3])];
    let mut uva2 = uva;
    if rng.chance(0.4) {
        // the v direction too
        uva2[1] = near(rng, nat[1], nat[3], fa);
        uvb[1] = near(rng, nat[1], nat[3], fb);
    }
    if rng.chance(0.2) {
        uvb[0] = uva2[0] + 0.6 * w[0] * if rng.chance(0.5) { 1.0 } else { -1.0 };
    }
    let crv = [[0.0; 3]; 3];
    let mut start = gen_pt(rng, uva2, [0.0, 0.0], crv, 0.0);
    let end = gen_pt(rng, uvb, [0.0, 0.0], crv, 1.0);
    if rng.chance(0.8) {
        // from-left flags as SetProperties sets them (0 or 1)
        // (PT_LEN values, so both indices exist)
        if let Some(f) = start.get_mut(24) {
            *f = pick(rng, &[0.0, 1.0], 1.0);
        }
        if let Some(f) = start.get_mut(25) {
            *f = pick(rng, &[0.0, 1.0], 1.0);
        }
    }
    Some(Inputs::Seam {
        surface,
        closure: pick(rng, &[0, 0, 1, 2, 3], 0),
        tol: pick(rng, &[1.0e-6, 1.0e-3, 1.0e-9, 1.0, 0.0], 1.0e-6) * s,
        start,
        end,
    })
}

fn gen_fix_tangents(rng: &mut Rng) -> Inputs {
    let s = gen_scale(rng);
    let d0 = [rng.uniform(-1.0, 0.0) * s, rng.uniform(-1.0, 0.0) * s];
    let domain = [
        d0[0],
        d0[1],
        d0[0] + rng.uniform(0.5, 2.0) * s,
        d0[1] + rng.uniform(0.5, 2.0) * s,
    ];
    let tol = gen_uv_tol(rng, domain);
    let crv = [[0.0; 3]; 3];
    let mut ends = Vec::new();
    let side = rng.below(4);
    for _ in 0..2 {
        let mut uv = uv_in(rng, domain);
        if rng.chance(0.8) {
            let n = pick(rng, &[0.0, 0.5, -0.5, 2.0], 0.0);
            match side {
                0 => uv[0] = domain[0] + n * tol[0],
                1 => uv[0] = domain[2] + n * tol[0],
                2 => uv[1] = domain[1] + n * tol[1],
                _ => uv[1] = domain[3] + n * tol[1],
            }
        }
        let duv = [rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)];
        let t = rng.unit();
        ends.push(gen_pt(rng, uv, duv, crv, t));
    }
    let end = ends.pop().unwrap_or_default();
    let start = ends.pop().unwrap_or_default();
    Inputs::FixTangents {
        domain,
        start,
        end,
        uv_tol: tol,
        tol: 1.0e-6,
    }
}

fn gen_stack(rng: &mut Rng) -> Inputs {
    let n = rng.below(12) as usize;
    let ops = (0..n)
        .map(|_| match rng.below(4) {
            0 | 1 => StackIn::Push {
                t: rng.uniform(-10.0, 10.0),
                uv: [rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)],
                duv: [rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)],
            },
            2 => StackIn::Pop,
            _ => StackIn::Look,
        })
        .collect();
    Inputs::Stack { ops }
}

fn gen_fix_curves(rng: &mut Rng) -> Inputs {
    let s = gen_scale(rng);
    let domain = [0.0, -s, 2.0 * s, s];
    let tol = gen_uv_tol(rng, domain);
    let spans = rng.below(5) as usize;
    let mut n = 1 + 3 * spans;
    if rng.chance(0.1) {
        n = pick(rng, &[0, 1, 2, 3, 5, 6, 9], 6);
    }
    let poly = (0..n)
        .map(|_| {
            let mut p = [rng.uniform(-0.3, 2.3) * s, rng.uniform(-1.3, 1.3) * s, 0.0];
            match rng.below(5) {
                0 => p[0] = pick(rng, &[0.0, 2.0 * s, 0.5 * tol[0], 2.0 * s - 0.5 * tol[0]], 0.0),
                1 => p[1] = pick(rng, &[-s, s, -s + 0.5 * tol[1]], s),
                _ => {}
            }
            p
        })
        .collect();
    Inputs::FixCurves {
        domain,
        uv_tol: tol,
        poly,
    }
}

fn gen_dups(rng: &mut Rng) -> Inputs {
    let n = rng.below(10) as usize;
    let tol = pick(rng, &[1.0e-6, 1.0e-3, 0.1, 0.0], 1.0e-6);
    let mut uvs: Vec<[f64; 2]> = Vec::new();
    for _ in 0..n {
        let uv = if !uvs.is_empty() && rng.chance(0.5) {
            let base = pick(rng, &uvs, [0.0, 0.0]);
            let f = pick(rng, &[0.0, 0.5, 0.99, 1.0, 2.0], 0.5);
            [base[0] + f * tol, base[1] - f * tol]
        } else {
            [rng.uniform(0.0, 1.0), rng.uniform(0.0, 1.0)]
        };
        uvs.push(uv);
    }
    Inputs::Dups {
        uv_tol: [tol, tol * pick(rng, &[1.0, 0.5, 2.0], 1.0)],
        uvs,
    }
}

/// An iso curve candidate: a line through `P(uv)` along a parameter direction of a plane
/// or bilinear patch, a circle at the generator point of a surface of revolution, or a
/// random curve near the surface.
fn gen_iso(rng: &mut Rng, k: &Kernel) -> Option<Inputs> {
    let s = gen_scale(rng);
    let kind = rng.below(4);
    let surface = match kind {
        0 => gen_plane(rng, s),
        1 => gen_bspline_surface(rng, s),
        2 => gen_revolution(rng, s),
        _ => gen_surface(rng, s),
    };
    let (srf, nat) = surface_with_domain(k, &surface)?;
    let domain = if rng.chance(0.8) {
        nat
    } else {
        gen_domain(rng, nat)
    };
    let w = [domain[2] - domain[0], domain[3] - domain[1]];
    let start = if rng.chance(0.2) {
        [domain[0], domain[1]]
    } else {
        uv_in(rng, domain)
    };
    let dir = rng.below(4);
    let frac = pick(rng, &[0.3, 0.5, 1.0, 1.2], 0.5);
    let mut end = start;
    match dir {
        0 => end[0] += frac * w[0],
        1 => end[0] -= frac * w[0],
        2 => end[1] += frac * w[1],
        _ => end[1] -= frac * w[1],
    }
    let pa = arr3(srf.evaluate_point(k, Point2::new(start[0], start[1])).ok()?);
    let pb = arr3(srf.evaluate_point(k, Point2::new(end[0], end[1])).ok()?);
    let nrm = arr3(
        srf.evaluate_normal(
            k, Point2::new(start[0], start[1]),
            solidcore::nurbs::knots::Side::Left,
            solidcore::nurbs::knots::Side::Left,
        )
        .ok()?,
    );
    let off = scale3(nrm, pick(rng, &[0.0, 0.0, 1.0e-9, 1.0e-4, 0.1], 0.0) * s);
    let curve = match (kind, &surface) {
        (2, SurfIn::Revolution { .. }) if rng.chance(0.7) => {
            // the parallel circle through P(start)
            let r = (pa[0] * pa[0] + pa[1] * pa[1]).sqrt();
            if r < 1.0e-6 * s {
                return None;
            }
            let a0 = pa[1].atan2(pa[0]);
            let span = pick(rng, &[0.5, 1.0, 3.0, 6.0], 1.0);
            CurveIn::Circle {
                c: [0.0, 0.0, pa[2]],
                x: [1.0, 0.0, 0.0],
                y: [0.0, 1.0, 0.0],
                d: [a0, a0 + span],
                r,
            }
        }
        (_, _) if rng.chance(0.75) => {
            let a = add3(pa, off);
            let bb = add3(pb, off);
            if sub3(a, bb).iter().all(|c| c.abs() < 1.0e-12) {
                return None;
            }
            CurveIn::Line { a, b: bb }
        }
        _ => gen_curve_near(k, rng, &srf, domain, s)?,
    };
    let c = rust_curve(k, &curve).ok()?;
    let ivl = c.natural_interval();
    let interval = if rng.chance(0.8) {
        [ivl.min(), ivl.max()]
    } else {
        let l = ivl.max() - ivl.min();
        [ivl.min() + 0.1 * l, ivl.max() - pick(rng, &[0.0, 0.2], 0.0) * l]
    };
    Some(Inputs::Iso {
        surface,
        curve,
        domain,
        interval,
        start_uv: start,
        tol: pick(rng, &[1.0e-6, 1.0e-3, 1.0e-9, 0.0], 1.0e-6) * s,
    })
}

/// A `DropAndTrimCurve` case: a line across a plane, the curve of an iso drop case
/// (iso lines and parallel circles, some offset, on planes, B-spline surfaces and
/// surfaces of revolution), or a curve near a surface.
fn gen_trim(rng: &mut Rng, k: &Kernel) -> Option<Inputs> {
    match rng.below(10) {
        0..=1 => gen_trim_plane_line(rng, k),
        2..=3 => gen_trim_bspline_iso_line(rng, k),
        4..=7 => {
            let Some(Inputs::Iso {
                surface,
                curve,
                domain,
                interval,
                tol,
                ..
            }) = gen_iso(rng, k)
            else {
                return None;
            };
            Some(Inputs::Trim {
                surface,
                curve,
                domain: Some(domain),
                interval: Some(interval),
                tol: pick(rng, &[tol, 1.0e-6, 0.0, 1.0e-12], tol),
                keep_all: rng.chance(0.5),
                on_surface: rng.chance(0.5),
            })
        }
        _ => {
            let s = gen_scale(rng);
            let surface = gen_surface(rng, s);
            let (srf, nat) = surface_with_domain(k, &surface)?;
            let domain = gen_domain(rng, nat);
            let curve = gen_curve_near(k, rng, &srf, domain, s)?;
            let c = rust_curve(k, &curve).ok()?;
            let ivl = c.natural_interval();
            Some(Inputs::Trim {
                surface,
                curve,
                domain: Some(domain),
                interval: Some([ivl.min(), ivl.max()]),
                tol: pick(rng, &[1.0e-6, 1.0e-3, 0.0], 1.0e-6) * s,
                keep_all: rng.chance(0.5),
                on_surface: rng.chance(0.5),
            })
        }
    }
}

/// A line along an iso line of a B-spline surface that is linear across it (a
/// bilinear patch either way, a rational cylinder along v), over the whole domain or
/// part of it, possibly offset along the normal.
fn gen_trim_bspline_iso_line(rng: &mut Rng, k: &Kernel) -> Option<Inputs> {
    let s = gen_scale(rng);
    let surface = gen_bspline_surface(rng, s);
    let SurfIn::Bspline { p, q, .. } = &surface else {
        return None;
    };
    let along_u = *p == 1 && (*q != 1 || rng.chance(0.5));
    if !along_u && *q != 1 {
        return None;
    }
    let (srf, nat) = surface_with_domain(k, &surface)?;
    let domain = if rng.chance(0.7) {
        nat
    } else {
        gen_domain(rng, nat)
    };
    let (a, bb) = if rng.chance(0.6) {
        (0.0, 1.0)
    } else {
        (rng.uniform(0.0, 0.4), rng.uniform(0.6, 1.0))
    };
    let iso = rng.uniform(0.0, 1.0);
    let at = |f: f64| {
        if along_u {
            [
                domain[0] + f * (domain[2] - domain[0]),
                domain[1] + iso * (domain[3] - domain[1]),
            ]
        } else {
            [
                domain[0] + iso * (domain[2] - domain[0]),
                domain[1] + f * (domain[3] - domain[1]),
            ]
        }
    };
    let (ua, ub) = (at(a), at(bb));
    let pa = arr3(srf.evaluate_point(k, Point2::new(ua[0], ua[1])).ok()?);
    let pb = arr3(srf.evaluate_point(k, Point2::new(ub[0], ub[1])).ok()?);
    let nrm = arr3(
        srf.evaluate_normal(k, 
            Point2::new(ua[0], ua[1]),
            solidcore::nurbs::knots::Side::Left,
            solidcore::nurbs::knots::Side::Left,
        )
        .ok()?,
    );
    let off = scale3(nrm, pick(rng, &[0.0, 0.0, 1.0e-9, 1.0e-4], 0.0) * s);
    let (pa, pb) = (add3(pa, off), add3(pb, off));
    if sub3(pa, pb).iter().all(|c| c.abs() < 1.0e-12 * s) {
        return None;
    }
    Some(Inputs::Trim {
        surface,
        curve: CurveIn::Line { a: pa, b: pb },
        domain: Some(domain),
        interval: Some(if rng.chance(0.8) { [0.0, 1.0] } else { [0.1, 0.9] }),
        tol: pick(rng, &[1.0e-6, 1.0e-3, 0.0, 1.0e-12], 1.0e-6) * s,
        keep_all: rng.chance(0.5),
        on_surface: rng.chance(0.5),
    })
}

/// A line across the middle of a plane.
fn gen_trim_plane_line(rng: &mut Rng, k: &Kernel) -> Option<Inputs> {
    let s = 1.0;
    let surface = gen_plane(rng, s);
    let (srf, nat) = surface_with_domain(k, &surface)?;
    let a = [
        nat[0] + 0.25 * (nat[2] - nat[0]),
        nat[1] + 0.5 * (nat[3] - nat[1]),
    ];
    let bb = [
        nat[0] + 0.75 * (nat[2] - nat[0]),
        nat[1] + 0.5 * (nat[3] - nat[1]),
    ];
    let pa = arr3(srf.evaluate_point(k, Point2::new(a[0], a[1])).ok()?);
    let pb = arr3(srf.evaluate_point(k, Point2::new(bb[0], bb[1])).ok()?);
    Some(Inputs::Trim {
        surface,
        curve: CurveIn::Line { a: pa, b: pb },
        domain: Some(nat),
        interval: Some([0.0, 1.0]),
        tol: 1.0e-6,
        keep_all: rng.chance(0.5),
        on_surface: rng.chance(0.5),
    })
}

/// The fixed-seed `DropAndTrimCurve` cases ([`gen_trim`]), appended after the generated
/// cases so that adding them leaves every generated case's inputs unchanged. Case `i`
/// takes the first trim input of seed `TRIM_SEED + i`.
fn trim_cases(unit: &str) -> Vec<Case> {
    let k = Kernel::default();
    let mut out = Vec::new();
    for i in 0..TRIM_CASES {
        let seed = TRIM_SEED + i;
        let mut rng = Rng::new(seed);
        let got = (0..64).find_map(|_| gen_trim(&mut rng, &k));
        let Some(inputs) = got.and_then(|t| serde_json::to_value(t).ok()) else {
            continue;
        };
        out.push(Case {
            unit: unit.to_string(),
            name: format!("trim{i:04}"),
            source: CaseSource::Generated { seed },
            inputs,
        });
    }
    out
}

fn gen_inputs(rng: &mut Rng) -> Inputs {
    let k = Kernel::default();
    loop {
        let r = rng.below(100);
        let got = match r {
            0..=3 => Some(Inputs::Ctor {
                t: pick(rng, &[0.0, 1.0, -3.5, UNDEF_DOUBLE, 1.0e8], 0.0),
                degen: rng.below(3) as u8,
            }),
            4..=6 => {
                let uv = [rng.unit(), rng.unit()];
                let du = rng.unit();
                let pt = gen_pt(rng, uv, [du, 0.0], [[1.0; 3]; 3], 0.5);
                Some(Inputs::Assign { pt })
            }
            7..=9 => {
                let uv = [rng.unit(), rng.unit()];
                let pt = gen_pt(rng, uv, [0.0, 1.0], [[2.0; 3]; 3], 0.5);
                Some(Inputs::Uninit { pt })
            }
            10..=16 => {
                let pt = gen_pt(rng, [0.0, 0.0], [0.0, 0.0], [[0.0; 3]; 3], 0.0);
                Some(Inputs::GoodTangent {
                    pt,
                    on_pole: rng.below(3) as u8,
                })
            }
            17..=38 => gen_moving(rng, &k),
            39..=53 => gen_span(rng, &k),
            54..=68 => gen_seam(rng, &k),
            69..=74 => Some(gen_fix_tangents(rng)),
            75..=78 => Some(gen_stack(rng)),
            79..=83 => Some(gen_fix_curves(rng)),
            84..=87 => Some(gen_dups(rng)),
            88..=98 => gen_iso(rng, &k),
            _ => gen_trim_plane_line(rng, &k),
        };
        if let Some(i) = got {
            return i;
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "intersect.surface_drop_lib_p1"
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        let mut cases = default_cases(self, &ctx)?;
        cases.extend(trim_cases(self.id()));
        Ok(cases)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_inputs(rng)).ok()
    }
}


/// Certificates of the `DropAndTrimCurve` rows that WR-455's contact move changed
/// (tangency-labs/solidcraft#17404: `Curve::global_curve_intersect` now always moves a point
/// contact to a curve's end when the curves are certified to stay within tolerance of each
/// other over the stretch between). The drop intersects the curve with the domain's
/// boundary curves (`boundary_start_points`), breaks the curve at every contact except one
/// within `EFF_ZERO_SQRT / 100` of an interval end, and keeps each piece whose midpoint drops
/// into the domain. In both rows a contact main reported just inside the curve's interval
/// is now reported at its end, so main's break there, and the piece it cut off, are gone.
/// No acceptance below is decided by samples.
#[cfg(test)]
mod wr455_trim {
    use super::*;
    use solidcore::curve::curve::p8::{curves_stay_within, CurveIntersectOptions};
    use solidcore::geom::solution_array::SolutionType;
    use solidcore::intersect::surface_drop_curve::SurfaceDropCurve;
    use solidcore::intersect::surface_tracer::tracer_surface_cache;
    use solidcore::nurbs::math::gamma;
    use solidcore::surface::surface::p4::NoObject;
    use solidcore::surface::surface::SurfaceKind;

    /// A replayed `trim` case: the drop's breaks, its tolerance (the boundary intersections
    /// run at it: the curve is on the surface), the domain boundary curves it intersects
    /// the curve with, and the record the runner writes now.
    struct Replay {
        k: Kernel,
        surface: Surface,
        curve: Curve,
        interval: Extent1d,
        tol: f64,
        breaks: Vec<f64>,
        iso: Vec<Curve>,
        rec: Vec<f64>,
    }

    fn replay(name: &str) -> Replay {
        let case = crate::case::regression_case(&Runner, name);
        let inputs: Inputs = case.parse().unwrap();
        let Inputs::Trim {
            surface,
            curve,
            domain,
            interval,
            tol,
            on_surface,
            ..
        } = &inputs
        else {
            panic!("{name}: not a trim case");
        };
        assert!(*on_surface, "{name}: the drop intersects the boundaries");
        let k = Kernel::default();
        let s = rust_surface(&k, surface).unwrap().unwrap();
        let c = rust_curve(&k, curve).unwrap();
        let dom = ext(domain.unwrap());
        let [lo, hi] = interval.unwrap();
        let ivl = Extent1d::from_min_max(lo, hi);
        let (at, breaks, iso) = {
            let mut drop = SurfaceDropCurve::new(&k, &s, &dom, &c, &ivl).unwrap();
            drop.set_curve_is_on_surface(true);
            let mut cache = tracer_surface_cache::<NoObject>(&k, &drop.base).unwrap();
            drop.drop_trace(&k, &mut cache, (*tol != 0.0).then_some(*tol), None, None)
                .unwrap();
            let at = drop.base.approx_tol_3d;
            let iso = cache.iso_boundary_curves(&k, &dom, at / 3.0).unwrap().curves.to_vec();
            (at, drop.curve_breaks.clone().unwrap(), iso)
        };
        let rec = rust(&inputs).unwrap().values.get("rec").cloned().unwrap();
        Replay {
            k,
            surface: s,
            curve: c,
            interval: ivl,
            tol: at,
            breaks,
            iso,
            rec,
        }
    }

    /// The contacts `[t, s]` (curve, boundary curve `j`) the drop's boundary intersection
    /// reports: each point, and both ends of each range.
    fn contacts(r: &Replay, j: usize) -> Vec<[f64; 2]> {
        let iso = r.iso.get(j).unwrap();
        let sols = r
            .curve
            .global_curve_intersect::<NoObject>(
                &r.k,
                &r.interval,
                iso,
                &iso.natural_interval(),
                r.tol,
                &CurveIntersectOptions::default(),
            )
            .unwrap();
        let pair = |e: &solidcore::geom::solution_array::SolutionEnd| {
            [e.param(0).unwrap(), e.param(1).unwrap()]
        };
        sols.as_slice()
            .iter()
            .flat_map(|s| {
                let range = s.solution_type == SolutionType::RangeOfValues;
                std::iter::once(pair(&s.start)).chain(range.then(|| pair(&s.end)))
            })
            .collect()
    }

    /// Certifies that main's contact `old` with boundary curve `j` and the contact `new`
    /// reported now are one contact (PROVEN): each is within the tolerance (a path of no
    /// length), and the curve and the boundary curve stay within it along a path joining
    /// them (`curves_stay_within`: the library's certified bound, no samples). Returns a
    /// bound on the 3D length of the curve between them: its speed bound over the stretch
    /// times the parameter step, widened by `gamma(4)` for the difference and the product.
    fn same_contact(r: &Replay, name: &str, j: usize, old: [f64; 2], new: [f64; 2]) -> f64 {
        let iso = r.iso.get(j).unwrap();
        for p in [old, new] {
            assert!(
                curves_stay_within(&r.k, &r.curve, iso, p, p, r.tol).unwrap(),
                "{name}: {p:?} is not a contact"
            );
        }
        assert!(
            curves_stay_within(&r.k, &r.curve, iso, old, new, r.tol).unwrap(),
            "{name}: {old:?} and {new:?} are not joined within {}",
            r.tol
        );
        let (a, b) = (old[0].min(new[0]), old[0].max(new[0]));
        let speed = r.curve.speed_bound(&Extent1d::from_min_max(a, b)).unwrap();
        let length = speed * (b - a) * (1.0 + gamma(4));
        eprintln!("{name}: contacts {old:?} -> {new:?} joined; stretch length <= {length:e}");
        assert!(length.is_finite());
        length
    }

    /// The UV control points of the cubic B-spline piece record that starts at `at`
    /// (`14033, degree, count, (x, y, z, w) * count, knot count, knots, multiplicities,
    /// interval`), with its knots and multiplicities.
    fn cubic_piece(rec: &[f64], at: usize) -> (Vec<[f64; 2]>, Vec<f64>, Vec<f64>) {
        let head = rec.get(at..at + 3).unwrap();
        assert_eq!(head, [14033.0, 3.0, 7.0], "a cubic piece of 7 control points");
        let pts: Vec<[f64; 2]> =
            rec.get(at + 3..at + 31).unwrap().chunks(4).map(|c| [c[0], c[1]]).collect();
        let n = rec.get(at + 31).copied().unwrap();
        assert_eq!(n, 3.0, "three distinct knots");
        let knots = rec.get(at + 32..at + 35).unwrap().to_vec();
        let mults = rec.get(at + 35..at + 38).unwrap().to_vec();
        assert_eq!(mults, [4.0, 3.0, 4.0], "two cubic Bezier segments");
        assert_eq!(rec.len(), at + 40, "the last piece");
        (pts, knots, mults)
    }

    /// An upper bound on `|S_u|` and `|S_v|` of a rational B-spline net over its whole
    /// domain (PROVEN): for a fixed `v`, `S(., v)` is a rational curve of degree `p` whose
    /// weights `W_i(v) = sum_j N_j(v) w_ij` lie in `[w_min, w_max]` and whose points
    /// `Q_i(v) = sum_j N_j(v) w_ij P_ij / W_i(v)` lie in the net's convex hull (convex
    /// combinations), so, as for a rational curve (`crv_speed_bound`), `|S_u| <= 2 p (w_max /
    /// w_min) D / h_u`: `D` the net's diameter (at most its box's diagonal) and `h_u` the
    /// shortest non-empty u span (every degree-`p - 1` basis function that is not zero
    /// somewhere has a support holding one). Likewise in `v`. The net's coordinates are
    /// divided by their weights (one rounding), the box's sides and diagonal take four more,
    /// the quotients and products four more: `gamma(16)` covers them and is applied once.
    fn net_lipschitz(net: &NurbSurface) -> (f64, f64) {
        let w = |c: &CPoint| if c.w == NOW { 1.0 } else { c.w };
        let cpts: Vec<CPoint> = net.cpts().into_iter().flatten().collect();
        let (mut wmin, mut wmax) = (f64::MAX, 0.0_f64);
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for c in &cpts {
            let wc = w(c);
            assert!(wc > 0.0, "a positive weight");
            wmin = wmin.min(wc);
            wmax = wmax.max(wc);
            for (a, x) in [c.x, c.y, c.z].into_iter().enumerate() {
                lo[a] = lo[a].min(x / wc);
                hi[a] = hi[a].max(x / wc);
            }
        }
        let diag = (0..3).map(|a| (hi[a] - lo[a]).powi(2)).sum::<f64>().sqrt();
        let h_min = |knots: &[f64]| {
            knots
                .windows(2)
                .map(|w| w[1] - w[0])
                .filter(|h| *h > 0.0)
                .fold(f64::MAX, f64::min)
        };
        #[allow(clippy::cast_precision_loss)]
        let bound = |deg: usize, knots: &[f64]| {
            2.0 * deg as f64 * (wmax / wmin) * diag / h_min(knots) * (1.0 + gamma(16))
        };
        (bound(net.p(), net.u_knots()), bound(net.q(), net.v_knots()))
    }

    /// Main's record of `trim0041` (merge base ada55bdad).
    const MAIN_TRIM0041: &str = "[0.0, 1.540549292218044, 0.6477656531906973, 4.0, 14033.0, 1.0, 2.0, 0.8099930304431764,
0.04483711937998038, 1.7976931348623157e+308, -1.7976931348623157e+308, 0.8099927814150117,
0.04483711937998038, 1.7976931348623157e+308, -1.7976931348623157e+308, 2.0, 2.0,
2.0000062216107315, 2.0, 2.0, 2.0, 2.0000062216107315, 14033.0, 1.0, 2.0,
0.4163857732583449, 0.8565265566577224, 1.7976931348623157e+308, -1.7976931348623157e+308,
0.4163848152569086, 0.8565265566577224, 1.7976931348623157e+308, -1.7976931348623157e+308,
2.0, 9.415200699179575, 9.415211055639727, 2.0, 2.0, 9.415200699179575, 9.415211055639727,
14033.0, 1.0, 2.0, 0.13427082079868075, 0.9827014205224025, 1.7976931348623157e+308,
-1.7976931348623157e+308, 0.13427070751840312, 0.9827014205224025, 1.7976931348623157e+308,
-1.7976931348623157e+308, 2.0, 11.99999875353449, 12.0, 2.0, 2.0, 11.99999875353449, 12.0,
14033.0, 3.0, 19.0, 0.8099929454115713, 0.04483531703858344, 0.0, -1.7976931348623157e+308,
0.801547559302499, -0.13417498083167556, 0.0, -1.7976931348623157e+308, 0.7912656602113024,
0.0, 0.0, -1.7976931348623157e+308, 0.7641200119703179, 0.0, 0.0, -1.7976931348623157e+308,
0.7505471878498257, 0.0, 0.0, -1.7976931348623157e+308, 0.7315814184821634, 0.0, 0.0,
-1.7976931348623157e+308, 0.7063862627761869, 0.0, 0.0, -1.7976931348623157e+308,
0.6811911070702104, 0.0, 0.0, -1.7976931348623157e+308, 0.6482011341315796, 0.0, 0.0,
-1.7976931348623157e+308, 0.6135429702982758, 0.0, 0.0, -1.7976931348623157e+308,
0.5788848064649719, 0.0, 0.0, -1.7976931348623157e+308, 0.5448965961593462, 0.0, 0.0,
-1.7976931348623157e+308, 0.5184836967459221, 0.0, 0.0, -1.7976931348623157e+308,
0.4920707973324981, 0.0, 0.0, -1.7976931348623157e+308, 0.4734652462081046, 0.0, 0.0,
-1.7976931348623157e+308, 0.460545779257611, 0.0, 0.0, -1.7976931348623157e+308,
0.43470684535662374, 0.0, 0.0, -1.7976931348623157e+308, 0.4246082928232255,
0.9329150231641156, 0.0, -1.7976931348623157e+308, 0.4163857732583449, 0.8565265566580085,
0.0, -1.7976931348623157e+308, 7.0, 2.0000062216107315, 3.8538048410029426,
4.780704150699048, 5.707603460395154, 6.6345027700912595, 7.5614020797873644,
9.415200699179575, 4.0, 3.0, 3.0, 3.0, 3.0, 3.0, 4.0, 2.0000062216107315, 9.415200699179575]";

    /// Main's record of `trim0248` (merge base ada55bdad).
    const MAIN_TRIM0248: &str = "[0.0, 0.021791374733099694, 0.0009911724651202995, 5.0, 14033.0, 1.0, 2.0,
0.5188545896105052, 0.6958014875742085, 1.7976931348623157e+308, -1.7976931348623157e+308,
0.5188545292606881, 0.6958014875742085, 1.7976931348623157e+308, -1.7976931348623157e+308,
2.0, -1.0, -0.9999992203222092, 2.0, 2.0, -1.0, -0.9999992203222092, 14033.0, 1.0, 2.0,
0.26995605858697974, 0.645939964079408, 1.7976931348623157e+308, -1.7976931348623157e+308,
0.26995618739155886, 0.645939964079408, 1.7976931348623157e+308, -1.7976931348623157e+308,
2.0, 2.0878080649322626, 2.0878094872220574, 2.0, 2.0, 2.0878080649322626,
2.0878094872220574, 14033.0, 1.0, 2.0, 0.7209103359554261, 0.24423615400097023,
1.7976931348623157e+308, -1.7976931348623157e+308, 0.7209109213691748, 0.24423615400097023,
1.7976931348623157e+308, -1.7976931348623157e+308, 2.0, 6.616389580735179,
6.616425259842228, 2.0, 2.0, 6.616389580735179, 6.616425259842228, 14033.0, 1.0, 2.0,
0.7615156251703623, 0.2430591549246388, 1.7976931348623157e+308, -1.7976931348623157e+308,
0.7615161600081228, 0.2430591549246388, 1.7976931348623157e+308, -1.7976931348623157e+308,
2.0, 8.999968557309979, 9.0, 2.0, 2.0, 8.999968557309979, 9.0, 14033.0, 3.0, 7.0,
0.7209109173000509, 0.24423569724218072, 0.0, -1.7976931348623157e+308, 0.7273837130239268,
0.23915013258330606, 0.0, -1.7976931348623157e+308, 0.7340677675625935, 0.23627145971325667,
0.0, -1.7976931348623157e+308, 0.740908793128353, 0.23607000022232297, 0.0,
-1.7976931348623157e+308, 0.7477498186941125, 0.23586854073138927, 0.0,
-1.7976931348623157e+308, 0.7548052807751381, 0.2383449396356775, 0.0,
-1.7976931348623157e+308, 0.7615156251703772, 0.2430591549244301, 0.0,
-1.7976931348623157e+308, 3.0, 6.616425259842228, 7.808196908576104, 8.999968557309979, 4.0,
3.0, 4.0, 6.616425259842228, 8.999968557309979]";

    /// trim0041, a two-segment line on a torus' outer equator over `t in [2, 12]`: main's contact
    /// with a boundary curve of the domain at `t = 11.99999875` is now reported at the curve's end
    /// `t = 12`, so main's fourth piece, the line over `[11.99999875, 12]`, is gone. PROVEN: the
    /// two contacts are one (joined within the drop's tolerance 9.43e-3 along the boundary curve),
    /// so main's piece lies within the tolerance of the domain's boundary over its whole length,
    /// and it is at most 1.8e-6 long: a touch of the boundary at the curve's end, of no length at
    /// that tolerance, which the drop reports as no piece wherever the contact lies within
    /// `EFF_ZERO_SQRT / 100` of the end. Every other value of the record is main's, exactly
    /// (PROVEN). Class: equal at tolerance; main's piece is a point set the new row lacks, but one
    /// shorter than the tolerance and on the boundary within it.
    #[test]
    fn wr455_trim0041_end_touch_is_one_contact() {
        let r = replay("trim0041");
        assert!(matches!(r.surface.kind(), SurfaceKind::SurfOfRevolution(_)));
        assert_eq!(
            r.breaks,
            [2.0, 2.000_006_221_610_731_5, 9.415_200_699_179_575, 9.415_211_055_639_727, 12.0]
        );
        let new = contacts(&r, 3)
            .into_iter()
            .find(|c| c[0] == r.interval.max())
            .expect("a contact at the curve's end");
        // main's contact (its sliver piece starts there)
        let old = [11.999_998_753_534_49, 0.134_270_820_798_680_72];
        let length = same_contact(&r, "trim0041", 3, old, new);
        assert!(length <= 2e-6 && length < r.tol, "{length}");
        let main: Vec<f64> = serde_json::from_str(MAIN_TRIM0041).unwrap();
        // main's pieces: two lines, the sliver line over [old, 12], the B-spline
        let sliver = main.get(40..58).unwrap();
        assert_eq!(sliver.get(16..18).unwrap(), [old[0], 12.0]);
        let mut want = main.get(..40).unwrap().to_vec();
        want.extend_from_slice(main.get(58..).unwrap());
        want[3] = 3.0;
        assert_eq!(r.rec, want, "trim0041: only the sliver is gone");
    }

    /// trim0248, a four-segment line near a torus over `t in [-1, 9]`, tolerance 1e-3: main's
    /// contacts at `t = -0.99999922` and `t = 8.99996856` (with two of the domain's boundary
    /// curves) are now reported at the curve's ends. PROVEN, as for trim0041: each pair is one
    /// contact (joined within the tolerance along the boundary curve), and each stretch between is
    /// at most 1e-5 long (1% of the tolerance). At the start, main's sliver line over `[-1,
    /// -0.99999922]` is gone (a touch of the boundary at the end, of no length at the tolerance).
    /// At the end, main's B-spline over `[6.6164253, 8.99996856]` and its line over `[8.99996856,
    /// 9]` are now one B-spline over `[6.6164253, 9]`. PROVEN: both B-splines are two cubic Bezier
    /// segments on the same start, so at the same local parameter they differ by at most their
    /// largest control point difference (the Bernstein basis is non-negative and sums to one),
    /// main's line lies within its own extent of main's B-spline's end, and the surface's partials
    /// are bounded over its whole net (`net_lipschitz`): every point of either row's curves is
    /// within `delta` (2.33e-4, asserted below 2.5e-4) in 3D of a point of the other's, against the
    /// tolerance 1e-3. The two lines between are main's, exactly. The header's largest drop and
    /// deviation are sampled measures of the drop (not certified): 0.0217914 and 9.9117e-4, both
    /// within 3e-8 of main's. Class: equal at tolerance.
    #[test]
    fn wr455_trim0248_end_contacts_are_one_contact() {
        let r = replay("trim0248");
        assert_eq!(r.tol, 1e-3);
        assert_eq!(
            r.breaks,
            [
                -1.0,
                2.087_808_064_932_262_6,
                2.087_809_487_222_057_4,
                4.615_437_813_944_749,
                6.616_389_580_735_179,
                6.616_425_259_842_228,
                9.0
            ]
        );
        let start = contacts(&r, 3)
            .into_iter()
            .find(|c| c[0] == r.interval.min())
            .expect("a contact at the curve's start");
        let end = contacts(&r, 1)
            .into_iter()
            .find(|c| c[0] == r.interval.max())
            .expect("a contact at the curve's end");
        let old_start = [-0.999_999_220_322_209_2, 0.518_854_546_798_177_7];
        let old_end = [8.999_968_557_309_979, 0.761_515_625_170_362_4];
        for length in [
            same_contact(&r, "trim0248 start", 3, old_start, start),
            same_contact(&r, "trim0248 end", 1, old_end, end),
        ] {
            assert!(length <= 1e-5 && length < r.tol, "{length}");
        }

        let main: Vec<f64> = serde_json::from_str(MAIN_TRIM0248).unwrap();
        // main: sliver line [-1, old_start], two lines, the end line [old_end, 9], B-spline
        assert_eq!(main.get(16..18).unwrap(), [-1.0, old_start[0]]);
        assert_eq!(main.get(74..76).unwrap(), [old_end[0], 9.0]);
        assert_eq!(r.rec.get(..4).unwrap()[3], 3.0);
        assert_eq!(r.rec.get(4..40).unwrap(), main.get(22..58).unwrap(), "the two lines");
        for (now, was) in [(r.rec[1], main[1]), (r.rec[2], main[2])] {
            assert!((now - was).abs() <= 3e-8, "{now} vs {was}");
        }

        // the B-splines: same start, two cubic segments each
        let (new_pts, new_knots, _) = cubic_piece(&r.rec, 40);
        let (old_pts, old_knots, _) = cubic_piece(&main, 76);
        assert_eq!(new_knots.first(), old_knots.first());
        assert_eq!(new_knots.last().copied(), Some(9.0));
        assert_eq!(old_knots.last().copied(), Some(old_end[0]));
        let (mut du, mut dv) = (0.0_f64, 0.0_f64);
        for (p, q) in new_pts.iter().zip(&old_pts) {
            du = du.max((p[0] - q[0]).abs());
            dv = dv.max((p[1] - q[1]).abs());
        }
        // main's end line: from (a, y) to (b, y), against main's B-spline's end
        let line = main.get(58..76).unwrap();
        let (l0, l1) = ([line[3], line[4]], [line[7], line[8]]);
        let s_end = *old_pts.last().unwrap();
        let hu = du + (l1[0] - l0[0]).abs() + (l0[0] - s_end[0]).abs();
        let hv = dv + (l1[1] - l0[1]).abs() + (l0[1] - s_end[1]).abs();
        // each difference, sum and product rounds: gamma(8) covers the few of them
        let (hu, hv) = (hu * (1.0 + gamma(8)), hv * (1.0 + gamma(8)));

        // every UV point of either row lies in the box of these control points, which the
        // net's domain holds, so the segment between two matched points does too
        let SurfaceKind::SurfOfRevolution(rev) = r.surface.kind() else {
            panic!("a surface of revolution");
        };
        let net = rev.base().gw_nurb().expect("the revolution's NURB");
        let nat = r.surface.natural_uv_domain_with_nurb(&r.k).unwrap();
        let (u_knots, v_knots) = (net.u_knots(), net.v_knots());
        assert_eq!(
            [nat.u_min(), nat.u_max(), nat.v_min(), nat.v_max()],
            [
                u_knots[net.p()],
                u_knots[net.n() + 1],
                v_knots[net.q()],
                v_knots[net.m() + 1]
            ],
            "the surface's parameters are its net's"
        );
        for p in new_pts.iter().chain(&old_pts).chain([&l0, &l1]) {
            assert!(
                nat.u_min() <= p[0] && p[0] <= nat.u_max() && nat.v_min() <= p[1] && p[1] <= nat.v_max(),
                "{p:?} outside the net"
            );
        }
        let (lu, lv) = net_lipschitz(net);
        let delta = (lu * hu + lv * hv) * (1.0 + gamma(4));
        eprintln!(
            "trim0248: UV within ({hu:e}, {hv:e}); |S_u| <= {lu:e}, |S_v| <= {lv:e}; 3D within {delta:e} (tol {})",
            r.tol
        );
        assert!(delta <= 2.5e-4 && delta < r.tol, "{delta}");
    }
}
