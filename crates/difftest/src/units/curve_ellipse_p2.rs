//! Runner for unit `curve.ellipse_p2`: `SmEllipse` / `SmPolarConversion`, part 2.
//!
//! C++: `solidcore_ffi::units::curve_ellipse_p2` (upstream `source/SMLib/src/SmEllipse.cpp`
//! on `SmEllipse` objects). Rust: `solidcore::curve::ellipse`.
//!
//! A case is an ellipse construction (`ellipse`, optionally with a second ellipse
//! whose NURB is passed as `pOptNurb`) and a program of operations (`ops`) run on it in
//! order; both sides produce the record documented in the ffi module (statuses `0` =
//! `SM_SUCCESS`, else the `SmStatus`; `1007` for a Rust `InvalidInput`, `-1` for a Rust
//! error upstream cannot produce), reported as the values `rec`. A case whose Rust side
//! reaches an unported unit is deferred as a whole.
//!
//! Deferred cases: intersections that reach `SmCurve::GlobalPointSolve`
//! (`curve.curve_p1`) or `SmCurve::GlobalCoincidenceChecker` (`curve.curve_p6`), 2D
//! ellipses (`SmEllipse::ConvertTo2D`, `curve.ellipse_p1`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::ellipse::{Ellipse, EllipseOptions, PolarConversion};
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::global_solver::{SolutionRequested, SolverOperation};
use solidcore::geom::solution_array::Solution;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::SolidError;
use solidcore_ffi::units::curve_ellipse_p2::{self as ffi};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `curve.ellipse_p2` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -------------------------------------------------------------------------

fn three() -> u64 {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EllipseJ {
    c: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    domain: [f64; 2],
    radii: [f64; 2],
    #[serde(default = "three")]
    dim: u64,
    #[serde(default)]
    inside_out: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    nurb_from: Option<Box<EllipseJ>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct LineJ {
    p: [f64; 3],
    v: [f64; 3],
    domain: [f64; 2],
    scale: f64,
}

fn full() -> [f64; 2] {
    [0.0, 1.0]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "op")]
enum OpJ {
    State {
        #[serde(default)]
        types: Vec<i64>,
    },
    EvalStep {
        t: f64,
        nd: u32,
    },
    EvalStepPoint {
        t: f64,
    },
    StepInv {
        p: [f64; 3],
        #[serde(default)]
        classify: bool,
        #[serde(default)]
        zone_tol: f64,
    },
    ToNurbs {
        t: f64,
    },
    PolarToNurbs {
        t: f64,
        #[serde(default)]
        zone_tol: f64,
    },
    AngleToNurb {
        args: [f64; 5],
    },
    IsDegenerate {
        tol: f64,
    },
    StartPlane {
        p: [f64; 3],
        tol: f64,
        #[serde(default)]
        snap: bool,
    },
    Drop {
        #[serde(default = "full")]
        ivl: [f64; 2],
        sop: i32,
        p: [f64; 3],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        in_vec: Option<[f64; 3]>,
        tol: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<f64>,
        #[serde(default)]
        req: i32,
    },
    IntersectEllipse {
        #[serde(default = "full")]
        ivl: [f64; 2],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        other: Option<EllipseJ>,
        #[serde(default = "full")]
        other_ivl: [f64; 2],
        tol: f64,
    },
    IntersectLine {
        #[serde(default = "full")]
        ivl: [f64; 2],
        line: LineJ,
        #[serde(default = "full")]
        line_ivl: [f64; 2],
        tol: f64,
    },
    Transform {
        origin: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scale: Option<[f64; 3]>,
    },
    #[serde(rename = "to_3d")]
    To3d,
    SetUp,
    Eq {
        other: EllipseJ,
    },
    SetStepInterval {
        domain: [f64; 2],
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    ellipse: EllipseJ,
    #[serde(default)]
    ops: Vec<OpJ>,
}

// ---- C++ side -----------------------------------------------------------------------

fn ffi_ellipse(e: &EllipseJ) -> ffi::EllipseIn {
    ffi::EllipseIn {
        center: e.c,
        x_axis: e.x,
        y_axis: e.y,
        domain: e.domain,
        radii: e.radii,
        dim: e.dim,
        inside_out: e.inside_out,
        nurb_from: e.nurb_from.as_ref().map(|s| Box::new(ffi_ellipse(s))),
    }
}

fn ffi_line(l: &LineJ) -> ffi::LineIn {
    ffi::LineIn {
        point: l.p,
        vector: l.v,
        domain: l.domain,
        scale: l.scale,
    }
}

fn ffi_op(op: &OpJ) -> ffi::Op {
    use ffi::Op as F;
    match op {
        OpJ::State { types } => F::State {
            types: types.clone(),
        },
        OpJ::EvalStep { t, nd } => F::EvalStep { t: *t, nd: *nd },
        OpJ::EvalStepPoint { t } => F::EvalStepPoint { t: *t },
        OpJ::StepInv {
            p,
            classify,
            zone_tol,
        } => F::StepInv {
            p: *p,
            classify: *classify,
            zone_tol: *zone_tol,
        },
        OpJ::ToNurbs { t } => F::ToNurbs { t: *t },
        OpJ::PolarToNurbs { t, zone_tol } => F::PolarToNurbs {
            t: *t,
            zone_tol: *zone_tol,
        },
        OpJ::AngleToNurb { args } => F::AngleToNurb { args: *args },
        OpJ::IsDegenerate { tol } => F::IsDegenerate { tol: *tol },
        OpJ::StartPlane { p, tol, snap } => F::StartPlane {
            p: *p,
            tol: *tol,
            snap: *snap,
        },
        OpJ::Drop {
            ivl,
            sop,
            p,
            in_vec,
            tol,
            target,
            req,
        } => F::Drop {
            ivl: *ivl,
            op: *sop,
            p: *p,
            in_vec: *in_vec,
            tol: *tol,
            target: *target,
            req: *req,
        },
        OpJ::IntersectEllipse {
            ivl,
            other,
            other_ivl,
            tol,
        } => F::IntersectEllipse {
            ivl: *ivl,
            other: other.as_ref().map(ffi_ellipse),
            other_ivl: *other_ivl,
            tol: *tol,
        },
        OpJ::IntersectLine {
            ivl,
            line,
            line_ivl,
            tol,
        } => F::IntersectLine {
            ivl: *ivl,
            line: ffi_line(line),
            line_ivl: *line_ivl,
            tol: *tol,
        },
        OpJ::Transform {
            origin,
            x,
            y,
            scale,
        } => F::Transform {
            origin: *origin,
            x: *x,
            y: *y,
            scale: *scale,
        },
        OpJ::To3d => F::To3d,
        OpJ::SetUp => F::SetUp,
        OpJ::Eq { other } => F::Eq {
            other: ffi_ellipse(other),
        },
        OpJ::SetStepInterval { domain } => F::SetStepInterval { domain: *domain },
    }
}

fn ffi_program(i: &Inputs) -> (ffi::EllipseIn, Vec<ffi::Op>) {
    (ffi_ellipse(&i.ellipse), i.ops.iter().map(ffi_op).collect())
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (a, ops) = ffi_program(i);
    match ffi::run(&a, &ops) {
        Ok(rec) => Ok(Output::new().values("rec", rec)),
        Err(e) => Err(harness(e.to_string())),
    }
}

// ---- Rust side --------------------------------------------------------------------

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::from_array(p)
}

fn b(x: bool) -> f64 {
    if x {
        1.0
    } else {
        0.0
    }
}

/// The `SmStatus` of a Rust error; `1007` for `InvalidInput`, `-1` for one upstream
/// cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

/// A failed Rust operation: the whole case is deferred, or a harness error.
enum Fail {
    Deferred(Box<Output>),
    Harness(HarnessError),
}

/// `Ok(v)`, an op status (pushed, `None`), or a deferral.
fn st<T>(o: &mut Vec<f64>, r: solidcore::Result<T>) -> Result<Option<T>, Fail> {
    match r {
        Ok(v) => {
            o.push(0.0);
            Ok(Some(v))
        }
        Err(e) => {
            if let Some(d) = Output::unported(&e) {
                return Err(Fail::Deferred(Box::new(d)));
            }
            o.push(status(&e));
            Ok(None)
        }
    }
}

fn put_vec(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn put_ivl(o: &mut Vec<f64>, e: &Extent1d) {
    o.extend([e.min(), e.max()]);
}

fn put_chars(o: &mut Vec<f64>, s: &str) {
    o.extend(s.bytes().map(f64::from));
    o.push(-1.0);
}

fn put_sols(o: &mut Vec<f64>, sols: &[Solution<i64>]) {
    o.push(sols.len() as f64);
    for s in sols {
        let id = |x: Option<i64>| x.map_or(-1.0, |v| v as f64);
        o.extend([
            f64::from(s.solution_type as i32),
            s.num_variables as f64,
            s.num_objects as f64,
            id(s.objects[0]),
            if s.num_objects > 1 {
                id(s.objects[1])
            } else {
                -1.0
            },
            s.start.solution_value,
            s.start.parameters[0],
            if s.num_variables > 1 {
                s.start.parameters[1]
            } else {
                0.0
            },
        ]);
    }
}

fn put_state(o: &mut Vec<f64>, e: &Ellipse) {
    let pos = e.position();
    put_vec(o, pos.origin());
    put_vec(o, pos.x_axis());
    put_vec(o, pos.y_axis());
    put_ivl(o, &e.step_interval());
    o.extend([
        e.x_radius(),
        e.y_radius(),
        b(e.inside_out()),
        e.base().dim() as f64,
        f64::from(e.base().form() as i32),
    ]);
    match e.base().gw_nurb() {
        None => o.push(0.0),
        Some(cur) => {
            o.extend([1.0, cur.n() as f64, cur.degree() as f64, cur.m() as f64]);
            for p in cur.cpts() {
                o.extend([p.x, p.y, p.z, p.w]);
            }
            o.extend_from_slice(cur.knots());
        }
    }
    let pc = e.polar_converter();
    o.extend([b(pc.is_polar_conversion_possible()), b(pc.has_curve())]);
    o.push(pc.natural_knots().len() as f64);
    o.extend_from_slice(pc.natural_knots());
    o.push(pc.angles().len() as f64);
    o.extend_from_slice(pc.angles());
    o.push(pc.exact_conversion().len() as f64);
    o.extend(pc.exact_conversion().iter().map(|&x| b(x)));
}

/// The ellipse of `e`; `Err` only for an unported callee.
fn build(k: &Kernel, e: &EllipseJ) -> solidcore::Result<Ellipse> {
    let src = match &e.nurb_from {
        Some(s) => Some(build(k, s)?),
        None => None,
    };
    let opts = EllipseOptions {
        dim: e.dim,
        nurb: src.as_ref().map(Ellipse::base),
        inside_out: e.inside_out,
    };
    Ellipse::new(
        k,
        v3(e.c),
        v3(e.x),
        v3(e.y),
        Extent1d::from_min_max(e.domain[0], e.domain[1]),
        e.radii[0],
        e.radii[1],
        &opts,
    )
}

fn sub_ivl(c: &BSplineCurve, f: [f64; 2]) -> Extent1d {
    let nat = c.natural_interval();
    Extent1d::from_min_max(nat.evaluate(f[0]), nat.evaluate(f[1]))
}

fn line_ivl(l: &Line, f: [f64; 2]) -> Extent1d {
    let nat = l.natural_interval();
    Extent1d::from_min_max(nat.evaluate(f[0]), nat.evaluate(f[1]))
}

fn run_op(k: &Kernel, a: &mut Ellipse, op: &OpJ, o: &mut Vec<f64>) -> Result<(), Fail> {
    match op {
        OpJ::State { types } => {
            o.push(0.0);
            o.extend([
                a.type_code() as f64,
                Ellipse::class_type() as f64,
                a.degree() as f64,
                a.start_angle_deg(),
                a.end_angle_deg(),
                b(a.is_analytic()),
                b(a.is_circle()),
            ]);
            put_ivl(o, &a.max_analytic_domain());
            o.push(f64::from(a.is_domain_boundary() as i32));
            let c = a.canonical();
            o.push(0.0);
            put_vec(o, c.position.origin());
            put_vec(o, c.position.x_axis());
            put_vec(o, c.position.y_axis());
            o.extend([c.radius_x, c.radius_y]);
            put_ivl(o, &c.domain);
            o.extend([b(c.inside_out), b(a.inside_out())]);
            let pc = a.polar_converter();
            o.push(pc.class_type() as f64);
            put_chars(o, pc.class_string());
            put_chars(o, a.type_string());
            put_chars(o, a.class_string());
            for &t in types {
                o.push(b(Ellipse::is_kind_of(t)));
            }
        }
        OpJ::EvalStep { t, nd } => {
            if let Some(v) = st(o, a.evaluate_step(*t, *nd as usize))? {
                for p in v {
                    put_vec(o, p);
                }
            }
        }
        OpJ::EvalStepPoint { t } => {
            if let Some(p) = st(o, a.evaluate_step_point(*t))? {
                put_vec(o, p);
            }
        }
        OpJ::StepInv {
            p,
            classify,
            zone_tol,
        } => {
            let (t, loc) = a.step_inversion(k, v3(*p), *classify, *zone_tol);
            o.extend([0.0, t]);
            if let Some(l) = loc {
                o.push(f64::from(l as i32));
            }
        }
        OpJ::ToNurbs { t } => {
            if let Some(r) = st(o, a.convert_t_from_step_to_nurbs(k, *t))? {
                o.push(r);
            }
        }
        OpJ::PolarToNurbs { t, zone_tol } => {
            let r = a
                .polar_converter()
                .convert_to_nurbs_parameter(k, a, *t, *zone_tol);
            if let Some((r, exact)) = st(o, r)? {
                o.extend([r, b(exact)]);
            }
        }
        OpJ::AngleToNurb { args } => {
            let [s0, s1, a0, a1, ang] = *args;
            if let Some(r) = st(
                o,
                PolarConversion::angle_param_to_nurb_param(s0, s1, a0, a1, ang),
            )? {
                o.push(r);
            }
        }
        OpJ::IsDegenerate { tol } => {
            o.extend([0.0, b(a.is_degenerate(*tol))]);
        }
        OpJ::StartPlane { p, tol, snap } => {
            if let Some(sp) = st(o, a.transform_point_to_start_plane(v3(*p), *tol, *snap))? {
                put_vec(o, sp.point);
                o.push(sp.num_angles as f64);
                o.extend_from_slice(&sp.angles_deg);
                o.push(b(sp.inside));
            }
        }
        OpJ::Drop {
            ivl,
            sop,
            p,
            in_vec,
            tol,
            target,
            req,
        } => {
            let i = sub_ivl(a.base(), *ivl);
            let sop = usize::try_from(*sop)
                .ok()
                .and_then(|s| SolverOperation::ALL.get(s).copied())
                .ok_or_else(|| Fail::Harness(harness("bad solver operation")))?;
            let req = if *req == 0 {
                SolutionRequested::Single
            } else {
                SolutionRequested::All
            };
            let r = a.drop_point_fast::<i64>(
                k, &i,
                sop,
                v3(*p),
                in_vec.map(v3),
                *tol,
                *target,
                req,
                Some(0),
            );
            if let Some(sols) = st(o, r)? {
                put_sols(o, &sols);
            }
        }
        OpJ::IntersectEllipse {
            ivl,
            other,
            other_ivl,
            tol,
        } => {
            let i = sub_ivl(a.base(), *ivl);
            let r = match other {
                None => {
                    let oi = sub_ivl(a.base(), *other_ivl);
                    a.intersect_with_ellipse::<i64>(&Kernel::default(), &i, a, &oi, *tol, Some(0), Some(0))
                }
                Some(e) => {
                    let bb = build(k, e).map_err(|e| match Output::unported(&e) {
                        Some(d) => Fail::Deferred(Box::new(d)),
                        None => Fail::Harness(harness(e.to_string())),
                    })?;
                    if bb.base().gw_nurb().is_none() {
                        o.push(-2.0);
                        return Ok(());
                    }
                    let oi = sub_ivl(bb.base(), *other_ivl);
                    a.intersect_with_ellipse::<i64>(&Kernel::default(), &i, &bb, &oi, *tol, Some(0), Some(1))
                }
            };
            if let Some(r) = st(o, r)? {
                o.push(b(r.needs_more_intersections));
                put_sols(o, &r.solutions);
            }
        }
        OpJ::IntersectLine {
            ivl,
            line,
            line_ivl: li,
            tol,
        } => {
            let i = sub_ivl(a.base(), *ivl);
            let opts = BoundedLineOptions {
                scale: line.scale,
                ..BoundedLineOptions::default()
            };
            let l = Line::bounded(
                v3(line.p),
                v3(line.v),
                &Extent1d::from_min_max(line.domain[0], line.domain[1]),
                &opts,
            );
            let oi = line_ivl(&l, *li);
            let r = a.intersect_with_line::<i64>(&Kernel::default(), &i, &l, &oi, *tol, Some(0), Some(1));
            if let Some(r) = st(o, r)? {
                o.push(b(r.needs_more_intersections));
                put_sols(o, &r.solutions);
            }
        }
        OpJ::Transform {
            origin,
            x,
            y,
            scale,
        } => {
            let mv = Axis2Placement::new(v3(*origin), v3(*x), v3(*y));
            st(o, a.transform(k, &mv, scale.map(v3)))?;
        }
        OpJ::To3d => {
            a.convert_to_3d();
            o.push(0.0);
        }
        OpJ::SetUp => {
            st(o, a.set_up_polar_conversion(k))?;
        }
        OpJ::Eq { other } => {
            let bb = build(k, other).map_err(|e| match Output::unported(&e) {
                Some(d) => Fail::Deferred(Box::new(d)),
                None => Fail::Harness(harness(e.to_string())),
            })?;
            o.extend([0.0, b(a.approx_eq(&bb))]);
        }
        OpJ::SetStepInterval { domain } => {
            a.set_step_interval(Extent1d::from_min_max(domain[0], domain[1]));
            o.push(0.0);
        }
    }
    Ok(())
}

fn op_code(op: &OpJ) -> f64 {
    match op {
        OpJ::State { .. } => 1.0,
        OpJ::EvalStep { .. } => 2.0,
        OpJ::EvalStepPoint { .. } => 3.0,
        OpJ::StepInv { .. } => 4.0,
        OpJ::ToNurbs { .. } => 5.0,
        OpJ::PolarToNurbs { .. } => 6.0,
        OpJ::AngleToNurb { .. } => 7.0,
        OpJ::IsDegenerate { .. } => 8.0,
        OpJ::StartPlane { .. } => 9.0,
        OpJ::Drop { .. } => 10.0,
        OpJ::IntersectEllipse { .. } => 11.0,
        OpJ::IntersectLine { .. } => 12.0,
        OpJ::Transform { .. } => 13.0,
        OpJ::To3d => 14.0,
        OpJ::SetUp => 15.0,
        OpJ::Eq { .. } => 16.0,
        OpJ::SetStepInterval { .. } => 17.0,
    }
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let mut a = match build(&k, &i.ellipse) {
        Ok(a) => a,
        Err(e) => {
            return match Output::unported(&e) {
                Some(d) => Ok(d),
                None => Err(harness(e.to_string())),
            }
        }
    };
    let mut o = Vec::new();
    if a.base().gw_nurb().is_some() {
        for op in &i.ops {
            o.push(op_code(op));
            match run_op(&k, &mut a, op, &mut o) {
                Ok(()) => {}
                Err(Fail::Deferred(d)) => return Ok(*d),
                Err(Fail::Harness(h)) => return Err(h),
            }
        }
    }
    put_state(&mut o, &a);
    Ok(Output::new().values("rec", o))
}

// ---- generation -------------------------------------------------------------------

fn round(x: f64) -> f64 {
    (x * 1e6).round() / 1e6
}

fn gen_dir(rng: &mut Rng) -> [f64; 3] {
    loop {
        let v = rng.point3(-1.0, 1.0);
        if v.iter().map(|x| x * x).sum::<f64>() > 0.05 {
            return v.map(round);
        }
    }
}

fn gen_domain(rng: &mut Rng) -> [f64; 2] {
    match rng.below(8) {
        0 => [0.0, 360.0],
        1 => [-180.0, 180.0],
        2 => [0.0, 90.0],
        3 => [*rng.choose(&[0.0, 1e-5, 2e-4, 179.9999, 180.0, 359.9999]).unwrap_or(&0.0), 270.0],
        4 => {
            let a = round(rng.uniform(-360.0, 0.0));
            [a, a + 360.0]
        }
        5 => [0.0, *rng.choose(&[180.0, 180.0002, 359.9998, 1e-3, 45.0]).unwrap_or(&90.0)],
        _ => {
            let a = round(rng.uniform(-360.0, 300.0));
            let l = round(rng.uniform(0.01, 360.0));
            [a, (a + l).min(360.0)]
        }
    }
}

fn gen_ellipse(rng: &mut Rng, allow_nurb: bool) -> EllipseJ {
    let r1 = round(rng.uniform(0.05, 5.0));
    let r2 = if rng.chance(0.6) {
        r1
    } else {
        round(rng.uniform(0.05, 5.0))
    };
    let x = gen_dir(rng);
    // mostly orthogonal axes; sometimes skewed (SetCanonical orthonormalises)
    let y = if rng.chance(0.7) {
        let xv = v3(x);
        let mut t = v3(gen_dir(rng)).cross(xv);
        if t.length() < 0.1 {
            t = Vec3::new(-x[1], x[0], 0.3);
        }
        [round(t.x), round(t.y), round(t.z)]
    } else {
        gen_dir(rng)
    };
    let mut e = EllipseJ {
        c: rng.point3(-5.0, 5.0).map(round),
        x,
        y,
        domain: gen_domain(rng),
        radii: [r1, r2],
        dim: if rng.chance(0.03) { 2 } else { 3 },
        inside_out: rng.chance(0.3),
        nurb_from: None,
    };
    if allow_nurb && rng.chance(0.1) {
        let mut src = e.clone();
        if rng.chance(0.5) {
            src.domain = gen_domain(rng);
        }
        e.nurb_from = Some(Box::new(src));
    }
    e
}

fn near_point(rng: &mut Rng, e: &EllipseJ) -> [f64; 3] {
    let k = Kernel::default();
    let base = build(&k, &EllipseJ {
        nurb_from: None,
        dim: 3,
        ..e.clone()
    })
    .ok()
    .and_then(|el| {
        let t = rng.uniform(e.domain[0] - 30.0, e.domain[1] + 30.0);
        el.evaluate_step_point(t).ok()
    })
    .unwrap_or(Vec3::from_array(e.c));
    let s = *rng.choose(&[0.0, 1e-9, 1e-6, 1e-3, 0.5, 3.0]).unwrap_or(&0.0);
    let d = rng.point3(-1.0, 1.0);
    let mut p = [base.x + s * d[0], base.y + s * d[1], base.z + s * d[2]];
    if rng.chance(0.08) {
        // on the axis
        let n = v3(e.x).cross(v3(e.y));
        let n = n / n.length();
        let h = rng.uniform(-2.0, 2.0);
        p = [e.c[0] + h * n.x, e.c[1] + h * n.y, e.c[2] + h * n.z];
    }
    p
}

fn gen_frac(rng: &mut Rng) -> [f64; 2] {
    if rng.chance(0.6) {
        return [0.0, 1.0];
    }
    let a = round(rng.unit());
    let b = round(rng.unit());
    if a <= b {
        [a, b]
    } else {
        [b, a]
    }
}

fn gen_op(rng: &mut Rng, e: &EllipseJ) -> OpJ {
    match rng.below(17) {
        0 => OpJ::State {
            types: vec![14022, 14021, 14033, 14001, 12060, 14010, 0],
        },
        1 => OpJ::EvalStep {
            t: round(rng.uniform(-720.0, 720.0)),
            nd: rng.below(5) as u32,
        },
        2 => OpJ::EvalStepPoint {
            t: round(rng.uniform(e.domain[0] - 10.0, e.domain[1] + 10.0)),
        },
        3 => OpJ::StepInv {
            p: near_point(rng, e),
            classify: rng.chance(0.5),
            zone_tol: *rng.choose(&[0.0, 0.0, 1e-3, 1e-7]).unwrap_or(&0.0),
        },
        4 | 5 => {
            let mut t = round(rng.uniform(e.domain[0] - 5.0, e.domain[1] + 5.0));
            if rng.chance(0.3) {
                t = *rng.choose(&e.domain).unwrap_or(&0.0);
            }
            if rng.chance(0.5) {
                OpJ::ToNurbs { t }
            } else {
                OpJ::PolarToNurbs {
                    t,
                    zone_tol: *rng.choose(&[0.0, 1e-5]).unwrap_or(&0.0),
                }
            }
        }
        6 => {
            let a0 = round(rng.uniform(-360.0, 360.0));
            let a1 = a0 + round(rng.uniform(-120.0, 120.0));
            let ang = if rng.chance(0.8) {
                a0 + (a1 - a0) * rng.unit()
            } else {
                round(rng.uniform(-400.0, 400.0))
            };
            OpJ::AngleToNurb {
                args: [0.0, round(rng.uniform(0.1, 3.0)), a0, a1, ang],
            }
        }
        7 => OpJ::IsDegenerate {
            tol: *rng.choose(&[0.0, 1e-6, 1e-3, 0.1, 10.0]).unwrap_or(&0.0),
        },
        8 => OpJ::StartPlane {
            p: near_point(rng, e),
            tol: *rng.choose(&[0.0, 1e-6, 1e-3]).unwrap_or(&0.0),
            snap: rng.chance(0.5),
        },
        9 | 10 => OpJ::Drop {
            ivl: gen_frac(rng),
            sop: *rng.choose(&[3, 3, 0, 4]).unwrap_or(&3),
            p: near_point(rng, e),
            in_vec: rng.chance(0.3).then(|| gen_dir(rng)),
            tol: *rng.choose(&[0.0, 1e-6, 1e-3, 1.0]).unwrap_or(&0.0),
            target: rng.chance(0.3).then(|| round(rng.uniform(0.0, 2.0))),
            req: rng.below(2) as i32,
        },
        11 => {
            let other = if rng.chance(0.1) {
                None
            } else {
                let mut o = gen_ellipse(rng, false);
                o.dim = 3;
                if rng.chance(0.5) {
                    // same plane, nearby
                    o.x = e.x;
                    o.y = e.y;
                    o.c = [
                        e.c[0] + round(rng.uniform(-3.0, 3.0)),
                        e.c[1] + round(rng.uniform(-3.0, 3.0)),
                        e.c[2],
                    ];
                }
                Some(o)
            };
            OpJ::IntersectEllipse {
                ivl: gen_frac(rng),
                other,
                other_ivl: gen_frac(rng),
                tol: *rng.choose(&[1e-6, 1e-3, 0.1]).unwrap_or(&1e-6),
            }
        }
        12 => {
            let p = near_point(rng, e);
            OpJ::IntersectLine {
                ivl: gen_frac(rng),
                line: LineJ {
                    p,
                    v: if rng.chance(0.3) { e.x } else { gen_dir(rng) },
                    domain: [round(rng.uniform(-5.0, 0.0)), round(rng.uniform(0.0, 5.0)) + 0.1],
                    scale: *rng.choose(&[1.0, 0.5, 2.0]).unwrap_or(&1.0),
                },
                line_ivl: gen_frac(rng),
                tol: *rng.choose(&[1e-6, 1e-3, 0.1]).unwrap_or(&1e-6),
            }
        }
        13 => OpJ::Transform {
            origin: rng.point3(-3.0, 3.0).map(round),
            x: gen_dir(rng),
            y: gen_dir(rng),
            scale: match rng.below(4) {
                0 => None,
                1 => {
                    let s = round(rng.uniform(0.1, 3.0));
                    Some([s, s, s])
                }
                2 => Some([1.0, 1.0, 1.0]),
                _ => Some([1.0, 2.0, 1.0]),
            },
        },
        14 => {
            if rng.chance(0.5) {
                OpJ::To3d
            } else {
                OpJ::SetUp
            }
        }
        15 => OpJ::Eq {
            other: if rng.chance(0.5) {
                EllipseJ {
                    nurb_from: None,
                    ..e.clone()
                }
            } else {
                gen_ellipse(rng, false)
            },
        },
        _ => OpJ::SetStepInterval {
            domain: gen_domain(rng),
        },
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let ellipse = gen_ellipse(rng, true);
    let n = 1 + rng.below(4) as usize;
    let ops = (0..n).map(|_| gen_op(rng, &ellipse)).collect();
    Inputs { ellipse, ops }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "curve.ellipse_p2"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let i = gen_case(rng);
            let (a, ops) = ffi_program(&i);
            if ffi::encode(&a, &ops).is_ok() {
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
