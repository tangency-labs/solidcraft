//! Runner for unit `intersect.surface_drop_lib_p1`: the drop point record `SmDropPt`
//! (constructor, `operator=`, `SetUninitialized`, `IsGoodTangentDrop`,
//! `IsMovingAlongBoundary`, `TestSpanAccuracy`, `CheckSeamJump`, `FixBoundaryTangents`),
//! the span stack (`sm_PushPoint`, `sm_PopStack`, `sm_StackLook`), `sm_FixBoundaryCurves`,
//! `sm_RemoveDuplicateDropPointSolutions`, `SmSurface::DropIsoCurve` and
//! `SmSurface::DropAndTrimCurve`.
//!
//! C++: `solidcore_ffi::units::intersect_surface_drop_lib_p1` (upstream
//! `source/SMLib/src/SmSurfaceDropLib.cpp`, compiled into the shim) on real `SmPlane`,
//! `SmBSplineSurface` and `SmSurfOfRevolution` surfaces and `SmLine`, `SmCircle` and
//! `SmBSplineCurve` curves. Rust: `solidcore::intersect::surface_drop_lib::p1`.
//!
//! A case is one op; both sides produce the record documented in the ffi module, compared
//! as the values `rec`. Inputs the shim treats as undefined upstream are [`ffi::REJECTED`]
//! on both sides (the Rust side applies the same predicate with its own evaluations).
//!
//! Deferred cases: `TestSpanAccuracy` spans that reach `smsurf_IsPointOnNormal`
//! Curves and surfaces may be upstream `.dat` files (database version 30), read on the
//! C++ side by `SmBSplineCurve::ReadFromFile` / `SmBSplineSurface::ReadFromFile` and on
//! the Rust side by the same steps (`read_file_db`, then `ReadFromDB`); a `Trim` case
//! without a domain or interval uses the natural ones, as upstream's `prog_test` does.
//!
//! Deferred cases: `TestSpanAccuracy` spans that reach `smsurf_IsPointOnNormal`
//! (`surface.surface_p3`), every `DropAndTrimCurve` (`SmSurfaceDropCurve::DoTrace`,
//! `intersect.surface_drop_curve`), and surface evaluations of kinds whose evaluators are
//! not ported yet.
//!
//! At least [`MIN_GENERATED`] generated cases run.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p2::BSPLINE_CURVE_TYPE;
use solidcore::curve::curve::Curve;
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
use solidcore_ffi::units::intersect_surface_drop_lib_p1 as ffi;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1500;

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
        CurveIn::File { file } => ffi::CurveIn::File { path: file.clone() },
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
        SurfIn::File { file } => ffi::SurfaceIn::File { path: file.clone() },
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

fn ffi_op(i: &Inputs) -> ffi::Op {
    match i {
        Inputs::Ctor { t, degen } => ffi::Op::Ctor {
            t: *t,
            degen: *degen,
        },
        Inputs::Assign { pt } => ffi::Op::Assign(pt.clone()),
        Inputs::Uninit { pt } => ffi::Op::Uninit(pt.clone()),
        Inputs::GoodTangent { pt, on_pole } => ffi::Op::GoodTangent {
            pt: pt.clone(),
            on_pole: *on_pole,
        },
        Inputs::Moving {
            surface,
            pt,
            domain,
            tol,
            ang,
            test,
        } => ffi::Op::Moving {
            surface: ffi_surface(surface),
            pt: pt.clone(),
            domain: *domain,
            tol: *tol,
            ang: *ang,
            test: *test,
        },
        Inputs::Span {
            surface,
            curve,
            domain,
            start,
            end,
            tol,
            uv_tol,
            refine,
        } => ffi::Op::Span {
            surface: ffi_surface(surface),
            curve: ffi_curve(curve),
            domain: *domain,
            start: start.clone(),
            end: end.clone(),
            tol: *tol,
            uv_tol: *uv_tol,
            refine: *refine,
        },
        Inputs::Seam {
            surface,
            closure,
            tol,
            start,
            end,
        } => ffi::Op::Seam {
            surface: ffi_surface(surface),
            closure: *closure,
            tol: *tol,
            start: start.clone(),
            end: end.clone(),
        },
        Inputs::FixTangents {
            domain,
            start,
            end,
            uv_tol,
            tol,
        } => ffi::Op::FixTangents {
            domain: *domain,
            start: start.clone(),
            end: end.clone(),
            uv_tol: *uv_tol,
            tol: *tol,
        },
        Inputs::Stack { ops } => ffi::Op::Stack(
            ops.iter()
                .map(|s| match s {
                    StackIn::Push { t, uv, duv } => ffi::StackOp::Push {
                        t: *t,
                        uv: *uv,
                        duv: *duv,
                    },
                    StackIn::Pop => ffi::StackOp::Pop,
                    StackIn::Look => ffi::StackOp::Look,
                })
                .collect(),
        ),
        Inputs::FixCurves {
            domain,
            uv_tol,
            poly,
        } => ffi::Op::FixCurves {
            domain: *domain,
            uv_tol: *uv_tol,
            poly: poly.clone(),
        },
        Inputs::Dups { uv_tol, uvs } => ffi::Op::Dups {
            uv_tol: *uv_tol,
            uvs: uvs.clone(),
        },
        Inputs::Iso {
            surface,
            curve,
            domain,
            interval,
            start_uv,
            tol,
        } => ffi::Op::Iso {
            surface: ffi_surface(surface),
            curve: ffi_curve(curve),
            domain: *domain,
            interval: *interval,
            start_uv: *start_uv,
            tol: *tol,
        },
        Inputs::Trim {
            surface,
            curve,
            domain,
            interval,
            tol,
            keep_all,
            on_surface,
        } => ffi::Op::Trim {
            surface: ffi_surface(surface),
            curve: ffi_curve(curve),
            domain: *domain,
            interval: *interval,
            tol: *tol,
            keep_all: *keep_all,
            on_surface: *on_surface,
        },
    }
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    match ffi::run(&ffi_op(i)) {
        Ok(rec) => Ok(Output::new().values("rec", rec)),
        Err(e) => Err(harness(format!("C++: {e}"))),
    }
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
                o.push(ffi::NO_SURFACE);
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
                Err(_) => o.push(ffi::REJECTED),
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
                o.push(ffi::NO_SURFACE);
                return Ok(());
            };
            let c = rust_curve(k, curve)?;
            let d = ext(*domain);
            let a = pt_in(start)?;
            let e = pt_in(end)?;
            if span_undefined(k, &s, &d, &a, &e)? {
                o.push(ffi::REJECTED);
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
                o.push(ffi::NO_SURFACE);
                return Ok(());
            };
            let a = pt_in(start)?;
            let mut e = pt_in(end)?;
            match st(a.check_seam_jump(k, &s, closure(*cl), *tol, &mut e))? {
                Ok(()) => o.push(0.0),
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
                    Ok((t, uv)) => o.extend([0.0, t, uv[0].x, uv[0].y, uv[1].x, uv[1].y]),
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
                Err(_) => o.push(ffi::REJECTED),
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
                o.push(ffi::NO_SURFACE);
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
                o.push(ffi::NO_SURFACE);
                return Ok(());
            };
            let c = rust_curve(k, curve)?;
            let dom = domain.map_or_else(|| s.natural_uv_domain(), ext);
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
                        st(put_curve(o, cv))?.map_err(|_| harness("trim curve data"))?;
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

fn gen_trim(rng: &mut Rng, k: &Kernel) -> Option<Inputs> {
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
            _ => gen_trim(rng, &k),
        };
        if let Some(i) = got {
            if ffi::encode(&ffi_op(&i)).is_ok() {
                return i;
            }
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
        default_cases(self, &ctx)
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
