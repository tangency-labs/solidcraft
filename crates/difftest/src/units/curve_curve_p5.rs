//! Runner for unit `curve.curve_p5`: `SmCurve` part 5 (chain clean-up, duplicate
//! removal, `EvaluateSTEPPoint`, `CacheMakeOrValidate`, `IsArc`, `IsLine`, `IsLinear`,
//! `IsDomainBoundary`, `IsPeriodic`, `Length`, `LocalPropertyAnalysis`), the file-static
//! helpers `sm_IsPointSolutionAcrossClosedSeamBoundary` / `sm_FindAdditionalCpGuess`
//! and the `Evaluate` members of `SmCCIntersectENFO`, `SmCCTangentIntENFO` and
//! `SmFindCCNormalENFO`.
//!
//! C++: `solidcore_ffi::units::curve_curve_p5` (upstream `source/SMLib/src/SmCurve.cpp`,
//! called through `SmCurve*` on B-spline, Hermite, line, ellipse and circle curves; the
//! file-static helpers and local classes are verbatim copies in the shim). Rust:
//! `solidcore::curve::curve::p5`.
//!
//! A case is one `op` with its curves (`{"bspline": {dim, oob, cpts, p, u}}` with
//! control points as stored `[x, y, z, w]`, `{"hermite": {dim, p1, d1, p2, d2}}`,
//! `{"arc": ..}` (a quadratic B-spline arc made by the Rust `CreateCircleSegment`,
//! bit-identical to upstream, handed to both sides as B-spline data), `{"line": {p, v,
//! domain, scale, inside_out}}`, `{"ellipse": {c, x, y, domain, radii, inside_out}}` or
//! `{"circle": {c, x, y, domain, radius, inside_out}}`) and parameters. Both sides
//! produce the flat record documented in the ffi module; the runner reports its first
//! entry as the `status` int (`0` = `SM_SUCCESS`, else the `SmStatus`; `1007` for a Rust
//! `InvalidInput`, `-1` for a Rust error upstream cannot produce) and the rest as the
//! `out` values.
//!
//! Curved-span lengths use the ported `mass.integrator`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p2::{Curve, CurvePropertyType};
use solidcore::curve::curve::p4::{CcIntersectEnfo, CcTangentIntEnfo, FindCcNormalEnfo};
use solidcore::curve::curve::p5::{
    crvs_cleanup, find_additional_cp_guess, is_point_solution_across_closed_seam_boundary,
    local_property_analysis, make_curve_cache, make_curve_cache_with_zone_tol,
    remove_duplicate_curves, validate_curve_cache,
};
use solidcore::curve::curve_cache::CurveCache;
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::a_object::{CacheStore, ObjectCacheType};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::global_solver::SolverOperation;
use solidcore::geom::local_solve_nd::EvalNFunctions;
use solidcore::geom::matrix::Matrix;
use solidcore::geom::solution_array::{Solution, SolutionType};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::math::EFF_ZERO_SQ;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::math::{NOW, NOZ};
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::topology::brep::p5::create_wire_edge_from_curve;
use solidcore::topology::{edge, Brep};
use solidcore::SolidError;
use solidcore_ffi::units::curve_curve_p5::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `curve.curve_p5` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BSplineIn {
    dim: u64,
    #[serde(default)]
    oob: bool,
    cpts: Vec<[f64; 4]>,
    p: usize,
    u: Vec<f64>,
    /// Knots written back over the curve's own NURB after construction, so the
    /// constructor's knot fix-up is undone (curve block kind `5`). Omitted when false,
    /// so existing case identities are unchanged.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    raw_knots: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HermiteIn {
    dim: u64,
    p1: [f64; 3],
    d1: [f64; 3],
    p2: [f64; 3],
    d2: [f64; 3],
}

/// A quadratic circular arc built by the Rust `CreateCircleSegment` and handed to both
/// sides as inline B-spline data.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ArcIn {
    center: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    radius: f64,
    a0: f64,
    a1: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LineIn {
    p: [f64; 3],
    v: [f64; 3],
    domain: [f64; 2],
    #[serde(default = "one")]
    scale: f64,
    #[serde(default)]
    inside_out: bool,
}

fn one() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EllipseIn {
    c: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    domain: [f64; 2],
    radii: [f64; 2],
    #[serde(default)]
    inside_out: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CircleIn {
    c: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    domain: [f64; 2],
    radius: f64,
    #[serde(default)]
    inside_out: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CurveIn {
    Bspline(BSplineIn),
    Hermite(HermiteIn),
    Arc(ArcIn),
    Line(LineIn),
    Ellipse(EllipseIn),
    Circle(CircleIn),
}

/// One `SmFindCCNormalENFO::Evaluate` call.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CallIn {
    x: [f64; 2],
    jac: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Cleanup {
        curves: Vec<CurveIn>,
        tol: f64,
    },
    RemoveDup {
        curves: Vec<Option<CurveIn>>,
    },
    StepPoint {
        curve: CurveIn,
        t: f64,
    },
    Seam {
        c1: CurveIn,
        ivl1: [f64; 2],
        c2: CurveIn,
        ivl2: [f64; 2],
        sol_type: i32,
        s: [f64; 2],
        this_first: bool,
    },
    Cache {
        curve: CurveIn,
        cache_type: i32,
        with_old: bool,
    },
    CpGuess {
        curve: CurveIn,
        ivl: [f64; 2],
        abc: [f64; 3],
        t: f64,
    },
    IsArc {
        curve: CurveIn,
        n: u64,
        tol: f64,
    },
    IsLine {
        curve: CurveIn,
        n: u64,
        tol: f64,
    },
    IsLinear {
        curve: CurveIn,
        tol: f64,
        want_dev: bool,
    },
    Domain {
        curve: CurveIn,
        domain: [f64; 4],
        tol: f64,
    },
    Periodic {
        curve: CurveIn,
        ivl: [f64; 2],
    },
    Length {
        curve: CurveIn,
        ivl: [f64; 2],
        acc: f64,
    },
    Lpa {
        curve: CurveIn,
        ivl: [f64; 2],
        prop: i32,
        guess: f64,
        val: Option<f64>,
        vecs: Vec<[f64; 3]>,
    },
    Cci {
        c1: CurveIn,
        c2: CurveIn,
        x: [f64; 2],
        jac: bool,
    },
    CcTan {
        c1: CurveIn,
        c2: CurveIn,
        x: [f64; 2],
        jac: bool,
    },
    Normal {
        c1: CurveIn,
        c2: CurveIn,
        sqtol: f64,
        sqdist: f64,
        sop: i32,
        vecs: Vec<[f64; 3]>,
        calls: Vec<CallIn>,
    },
    /// The cache of the curve of a wire edge whose tolerance is `tol`.
    EdgeCache {
        curve: CurveIn,
        tol: f64,
    },
}

// ---- C++ input --------------------------------------------------------------------

/// `c` with an arc replaced by its inline B-spline.
fn resolve(c: &CurveIn) -> Result<CurveIn, HarnessError> {
    let CurveIn::Arc(ci) = c else {
        return Ok(c.clone());
    };
    let frame = Axis2Placement::new(v3(ci.center), v3(ci.x), v3(ci.y));
    let arc = BSplineCurve::create_circle_segment(
        3,
        &frame,
        ci.radius,
        ci.a0,
        ci.a1,
        solidcore::nurbs::b_spline_curve::NurbCircleParam::Quadratic,
    )
    .map_err(|e| harness(format!("arc: {e}")))?;
    let cur = arc.gw_nurb().ok_or_else(|| harness("arc without NURB"))?;
    Ok(CurveIn::Bspline(BSplineIn {
        dim: 3,
        oob: false,
        cpts: cur.cpts().iter().map(|p| [p.x, p.y, p.z, p.w]).collect(),
        p: cur.degree(),
        u: cur.knots().to_vec(),
        raw_knots: false,
    }))
}

fn b(x: bool) -> f64 {
    f64::from(u8::from(x))
}

fn push_line(v: &mut Vec<f64>, l: &LineIn) {
    v.extend_from_slice(&l.p);
    v.extend_from_slice(&l.v);
    v.extend([l.domain[0], l.domain[1], l.scale, b(l.inside_out)]);
}

fn curve_block(v: &mut Vec<f64>, c: &CurveIn) -> Result<(), HarnessError> {
    match &resolve(c)? {
        CurveIn::Bspline(bs) => {
            let n = bs.cpts.len().saturating_sub(1);
            let m = bs.u.len().saturating_sub(1);
            let kind = if bs.raw_knots { 5.0 } else { 0.0 };
            v.extend([kind, bs.dim as f64, b(bs.oob)]);
            v.extend([n as f64, bs.p as f64, m as f64]);
            for c in &bs.cpts {
                v.extend_from_slice(c);
            }
            v.extend_from_slice(&bs.u);
        }
        CurveIn::Hermite(h) => {
            v.extend([1.0, h.dim as f64]);
            for p in [h.p1, h.d1, h.p2, h.d2] {
                v.extend_from_slice(&p);
            }
        }
        CurveIn::Line(l) => {
            v.extend([2.0, 3.0]);
            push_line(v, l);
        }
        CurveIn::Ellipse(e) => {
            v.extend([3.0, 3.0]);
            v.extend_from_slice(&e.c);
            v.extend_from_slice(&e.x);
            v.extend_from_slice(&e.y);
            v.extend([e.domain[0], e.domain[1], e.radii[0], e.radii[1], b(e.inside_out)]);
        }
        CurveIn::Circle(e) => {
            v.extend([4.0, 3.0]);
            v.extend_from_slice(&e.c);
            v.extend_from_slice(&e.x);
            v.extend_from_slice(&e.y);
            v.extend([e.domain[0], e.domain[1], e.radius, b(e.inside_out)]);
        }
        CurveIn::Arc(_) => return Err(harness("unresolved arc")),
    }
    Ok(())
}

fn op_of(op: &OpIn) -> Op {
    match op {
        OpIn::Cleanup { .. } => Op::Cleanup,
        OpIn::RemoveDup { .. } => Op::RemoveDup,
        OpIn::StepPoint { .. } => Op::StepPoint,
        OpIn::Seam { .. } => Op::Seam,
        OpIn::Cache { .. } => Op::Cache,
        OpIn::CpGuess { .. } => Op::CpGuess,
        OpIn::IsArc { .. } => Op::IsArc,
        OpIn::IsLine { .. } => Op::IsLine,
        OpIn::IsLinear { .. } => Op::IsLinear,
        OpIn::Domain { .. } => Op::Domain,
        OpIn::Periodic { .. } => Op::Periodic,
        OpIn::Length { .. } => Op::Length,
        OpIn::Lpa { .. } => Op::Lpa,
        OpIn::Cci { .. } => Op::Cci,
        OpIn::CcTan { .. } => Op::CcTan,
        OpIn::Normal { .. } => Op::Normal,
        OpIn::EdgeCache { .. } => Op::EdgeCache,
    }
}

#[allow(clippy::too_many_lines)]
fn cpp_input(op: &OpIn) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut v = Vec::new();
    match op {
        OpIn::Cleanup { curves, tol } => {
            v.push(curves.len() as f64);
            for c in curves {
                curve_block(&mut v, c)?;
            }
            v.push(*tol);
        }
        OpIn::RemoveDup { curves } => {
            v.push(curves.len() as f64);
            for c in curves {
                v.push(b(c.is_some()));
                if let Some(c) = c {
                    curve_block(&mut v, c)?;
                }
            }
        }
        OpIn::StepPoint { curve, t } => {
            curve_block(&mut v, curve)?;
            v.push(*t);
        }
        OpIn::Seam {
            c1,
            ivl1,
            c2,
            ivl2,
            sol_type,
            s,
            this_first,
        } => {
            curve_block(&mut v, c1)?;
            v.extend_from_slice(ivl1);
            curve_block(&mut v, c2)?;
            v.extend_from_slice(ivl2);
            v.extend([f64::from(*sol_type), s[0], s[1], b(*this_first)]);
        }
        OpIn::Cache {
            curve,
            cache_type,
            with_old,
        } => {
            curve_block(&mut v, curve)?;
            v.extend([f64::from(*cache_type), b(*with_old)]);
        }
        OpIn::CpGuess { curve, ivl, abc, t } => {
            curve_block(&mut v, curve)?;
            v.extend_from_slice(ivl);
            v.extend_from_slice(abc);
            v.push(*t);
        }
        OpIn::IsArc { curve, n, tol } | OpIn::IsLine { curve, n, tol } => {
            curve_block(&mut v, curve)?;
            v.extend([*n as f64, *tol]);
        }
        OpIn::IsLinear {
            curve,
            tol,
            want_dev,
        } => {
            curve_block(&mut v, curve)?;
            v.extend([*tol, b(*want_dev)]);
        }
        OpIn::Domain { curve, domain, tol } => {
            curve_block(&mut v, curve)?;
            v.extend_from_slice(domain);
            v.push(*tol);
        }
        OpIn::Periodic { curve, ivl } => {
            curve_block(&mut v, curve)?;
            v.extend_from_slice(ivl);
        }
        OpIn::Length { curve, ivl, acc } => {
            curve_block(&mut v, curve)?;
            v.extend_from_slice(ivl);
            v.push(*acc);
        }
        OpIn::Lpa {
            curve,
            ivl,
            prop,
            guess,
            val,
            vecs,
        } => {
            curve_block(&mut v, curve)?;
            v.extend_from_slice(ivl);
            v.extend([f64::from(*prop), *guess, b(val.is_some()), val.unwrap_or(0.0)]);
            v.push(vecs.len() as f64);
            for x in vecs {
                v.extend_from_slice(x);
            }
        }
        OpIn::Cci { c1, c2, x, jac } | OpIn::CcTan { c1, c2, x, jac } => {
            curve_block(&mut v, c1)?;
            curve_block(&mut v, c2)?;
            v.extend([x[0], x[1], b(*jac)]);
        }
        OpIn::Normal {
            c1,
            c2,
            sqtol,
            sqdist,
            sop,
            vecs,
            calls,
        } => {
            curve_block(&mut v, c1)?;
            curve_block(&mut v, c2)?;
            v.extend([*sqtol, *sqdist, f64::from(*sop)]);
            v.push(vecs.len() as f64);
            for x in vecs {
                v.extend_from_slice(x);
            }
            v.push(calls.len() as f64);
            for c in calls {
                v.extend([c.x[0], c.x[1], b(c.jac)]);
            }
        }
        OpIn::EdgeCache { curve, tol } => {
            curve_block(&mut v, curve)?;
            v.push(*tol);
        }
    }
    Ok((op_of(op), v))
}

fn to_output(flat: &[f64]) -> Output {
    let status = flat.first().copied().unwrap_or(f64::NAN);
    let rest = flat.get(1..).unwrap_or(&[]).to_vec();
    Output::new()
        .ints("status", vec![status as i64])
        .values("out", rest)
}

fn cpp(op: &OpIn) -> Result<Output, HarnessError> {
    let (code, input) = cpp_input(op)?;
    match ffi::eval(code, &input) {
        Ok(flat) => Ok(to_output(&flat)),
        Err(e) => Err(harness(e.to_string())),
    }
}

// ---- Rust side --------------------------------------------------------------------

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::from_array(p)
}

fn build_line(l: &LineIn) -> Line {
    let opts = BoundedLineOptions {
        scale: l.scale,
        inside_out: l.inside_out,
        ..BoundedLineOptions::default()
    };
    Line::bounded(
        v3(l.p),
        v3(l.v),
        &Extent1d::from_min_max(l.domain[0], l.domain[1]),
        &opts,
    )
}

fn build_ellipse(k: &Kernel, e: &EllipseIn) -> solidcore::Result<Ellipse> {
    let opts = EllipseOptions {
        dim: 3,
        nurb: None,
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

fn build_circle(k: &Kernel, e: &CircleIn) -> solidcore::Result<Ellipse> {
    let opts = EllipseOptions {
        dim: 3,
        nurb: None,
        inside_out: e.inside_out,
    };
    Ellipse::new_circle(
        k,
        v3(e.c),
        v3(e.x),
        v3(e.y),
        Extent1d::from_min_max(e.domain[0], e.domain[1]),
        e.radius,
        &opts,
    )
}

fn build(k: &Kernel, c: &CurveIn) -> Result<Curve, HarnessError> {
    match &resolve(c)? {
        CurveIn::Bspline(bs) => {
            let n = bs
                .cpts
                .len()
                .checked_sub(1)
                .ok_or_else(|| harness("no points"))?;
            let m = bs.u.len().checked_sub(1).ok_or_else(|| harness("no knots"))?;
            let mut cur =
                allocate_nurb_curve(n, bs.p, m).map_err(|e| harness(format!("curve: {e}")))?;
            for (d, s) in cur.cpts_mut().iter_mut().zip(&bs.cpts) {
                d.x = s[0];
                d.y = s[1];
                d.z = s[2];
                d.w = s[3];
            }
            cur.knots_mut().copy_from_slice(&bs.u);
            let mut c = BSplineCurve::from_nurb(cur, bs.dim, false);
            if bs.raw_knots {
                // undo the constructor's fix-up, as the shim does
                c.get_or_create_gw_nurb()
                    .map_err(|e| harness(format!("curve: {e}")))?
                    .knots_mut()
                    .copy_from_slice(&bs.u);
            }
            c.set_out_of_bounds_enabled(bs.oob);
            Ok(Curve::from(c))
        }
        CurveIn::Hermite(h) => Ok(Curve::from(HermiteCurve::new(
            v3(h.p1),
            v3(h.d1),
            v3(h.p2),
            v3(h.d2),
            h.dim,
        ))),
        CurveIn::Line(l) => Ok(Curve::from(build_line(l))),
        CurveIn::Ellipse(e) => Ok(Curve::from(
            build_ellipse(k, e).map_err(|e| harness(format!("ellipse: {e}")))?,
        )),
        CurveIn::Circle(e) => Ok(Curve::from(
            build_circle(k, e).map_err(|e| harness(format!("circle: {e}")))?,
        )),
        CurveIn::Arc(_) => Err(harness("unresolved arc")),
    }
}

fn pv(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

/// The `SmStatus` of a Rust error; `1007` for `InvalidInput`, `-1` for one upstream
/// cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

/// A Rust failure: deferred for `Unported`, else the status record.
enum Fail {
    Deferred(Box<Output>),
    Status(f64),
}

impl From<SolidError> for Fail {
    fn from(e: SolidError) -> Self {
        match Output::unported(&e) {
            Some(o) => Fail::Deferred(Box::new(o)),
            None => Fail::Status(status(&e)),
        }
    }
}

/// Evaluates `r`: `Ok(v)`, or records the status / deferral.
macro_rules! tri {
    ($o:ident, $r:expr) => {
        match $r {
            Ok(v) => v,
            Err(e) => match Fail::from(e) {
                Fail::Deferred(d) => return Ok(*d),
                Fail::Status(s) => {
                    $o.push(s);
                    return Ok(to_output(&$o));
                }
            },
        }
    };
}

/// The status of `r` (`0` for `Ok`), or the deferral.
fn st_of<T>(r: &solidcore::Result<T>) -> Result<f64, Box<Output>> {
    match r {
        Ok(_) => Ok(0.0),
        Err(e) => match Output::unported(e) {
            Some(o) => Err(Box::new(o)),
            None => Ok(status(e)),
        },
    }
}

fn ivl_of(i: [f64; 2]) -> Extent1d {
    Extent1d::from_min_max(i[0], i[1])
}

/// The record of one ENFO evaluation: status, `f`, the Jacobian, found.
fn put_eval(
    o: &mut Vec<f64>,
    r: &solidcore::Result<bool>,
    f: [f64; 2],
    j: &Matrix,
) -> Result<(), Box<Output>> {
    o.push(st_of(r)?);
    o.extend(f);
    for (r, c) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
        o.push(j.at(r, c).unwrap_or(f64::NAN));
    }
    o.push(b(matches!(r, Ok(true))));
    Ok(())
}

fn put_points(k: &Kernel, o: &mut Vec<f64>, c: &Curve) {
    let nat = c.natural_interval();
    o.extend([nat.min(), nat.max()]);
    for t in [nat.min(), nat.evaluate(0.5), nat.max()] {
        pv(o, c.evaluate_point(k, t).unwrap_or(Vec3::UNDEF));
    }
}

#[allow(clippy::too_many_lines)]
fn rust(op: &OpIn) -> Result<Output, HarnessError> {
    let mut k = Kernel::default();
    let mut o: Vec<f64> = Vec::new();
    match op {
        OpIn::Cleanup { curves, tol } => {
            let mut cs: Vec<Curve> = curves
                .iter()
                .map(|c| build(&k, c))
                .collect::<Result<_, _>>()?;
            let r = tri!(o, crvs_cleanup(&k, &mut cs, *tol));
            o.extend([0.0, r.removed as f64, cs.len() as f64]);
            for (c, origin) in cs.iter().zip(&r.origin) {
                o.push(*origin as f64);
                put_points(&k, &mut o, c);
            }
        }
        OpIn::RemoveDup { curves } => {
            let mut cs: Vec<Option<Curve>> = curves
                .iter()
                .map(|c| c.as_ref().map(|c| build(&k, c)).transpose())
                .collect::<Result<_, _>>()?;
            let origin = tri!(o, remove_duplicate_curves(&k, &mut cs));
            o.extend([0.0, cs.len() as f64]);
            for (c, i) in cs.iter().zip(&origin) {
                o.push(if c.is_some() { *i as f64 } else { -1.0 });
            }
        }
        OpIn::StepPoint { curve, t } => {
            let c = build(&k, curve)?;
            let p = tri!(o, c.evaluate_step_point(&k, *t));
            o.push(0.0);
            pv(&mut o, p);
        }
        OpIn::Seam {
            c1,
            ivl1,
            c2,
            ivl2,
            sol_type,
            s,
            this_first,
        } => {
            let a = build(&k, c1)?;
            let bb = build(&k, c2)?;
            let mut sol: Solution<()> = Solution::new();
            sol.solution_type =
                SolutionType::from_code(*sol_type).ok_or_else(|| harness("solution type"))?;
            sol.num_variables = 2;
            sol.num_objects = 2;
            sol.start.parameters[0] = s[0];
            sol.start.parameters[1] = s[1];
            let r = tri!(
                o,
                is_point_solution_across_closed_seam_boundary(&k, 
                    &sol,
                    &a,
                    &ivl_of(*ivl1),
                    &bb,
                    &ivl_of(*ivl2),
                    *this_first,
                )
            );
            o.extend([0.0, b(r)]);
        }
        OpIn::Cache {
            curve,
            cache_type,
            with_old,
        } => {
            let c = build(&k, curve)?;
            let ct = ObjectCacheType::from_code(*cache_type)
                .ok_or_else(|| harness("cache type"))?;
            let mut store: CacheStore<CurveCache<()>> = CacheStore::new();
            let mut old_cache = None;
            let mut old = None;
            if *with_old {
                let cache = tri!(o, make_curve_cache::<()>(&k, &c, None));
                old = Some(
                    store
                        .insert(cache.clone())
                        .map_err(|e| harness(e.to_string()))?,
                );
                old_cache = Some(cache);
            }
            let validated = tri!(o, validate_curve_cache(ct, old));
            let (reused, cache) = match (validated, old_cache) {
                (Some(id), Some(cache)) => (Some(id) == old, cache),
                _ => (false, tri!(o, make_curve_cache::<()>(&k, &c, None))),
            };
            o.extend([0.0, b(reused), cache.span_count() as f64]);
            for i in 0..cache.span_count() {
                let span = tri!(o, cache.span(i));
                o.extend([span.aux.interval.min(), span.aux.interval.max()]);
            }
        }
        OpIn::EdgeCache { curve, tol } => {
            let c = build(&k, curve)?;
            let mut brep = tri!(o, Brep::new_with_infinite_region());
            let ivl = c.natural_interval();
            let e = tri!(o, create_wire_edge_from_curve(&mut k, &mut brep, c, ivl));
            tri!(o, edge::set_tolerance(&mut brep, e, *tol, false, false));
            let edge_curve = tri!(o, edge::curve(&brep, e)).ok_or_else(|| harness("edge curve"))?;
            // the cache is the edge curve's own: its tolerance is the edge's
            let zone_tol_3d = tri!(o, edge::tolerance(&brep, e));
            let cache = tri!(
                o,
                make_curve_cache_with_zone_tol::<()>(&k, edge_curve, None, Some(zone_tol_3d))
            );
            o.extend([0.0, cache.span_count() as f64]);
            for i in 0..cache.span_count() {
                let span = tri!(o, cache.span(i));
                o.extend([span.aux.interval.min(), span.aux.interval.max()]);
            }
        }
        OpIn::CpGuess { curve, ivl, abc, t } => {
            let c = build(&k, curve)?;
            let r = tri!(o, find_additional_cp_guess(&k, &c, &ivl_of(*ivl), v3(*abc), *t));
            o.extend([0.0, b(r.is_some()), r.unwrap_or(0.0)]);
        }
        OpIn::IsArc { curve, n, tol } => {
            // the FFI rejects more than `ffi::MAX_SAMPLES` samples; upstream (and the port)
            // would loop that many times, so a larger count is not a case for Rust either
            if *n > ffi::MAX_SAMPLES as u64 {
                return Err(HarnessError::new(format!("{n} samples")));
            }
            let c = build(&k, curve)?;
            let r = tri!(o, c.is_arc(&k, *n, *tol));
            o.extend([0.0, b(r.is_some())]);
            if let Some((f, r, s0, s1)) = r {
                pv(&mut o, f.origin());
                pv(&mut o, f.x_axis());
                pv(&mut o, f.y_axis());
                o.extend([r, s0, s1]);
            }
        }
        OpIn::IsLine { curve, n, tol } => {
            // as `IsArc`: the FFI bounds the sample count
            if *n > ffi::MAX_SAMPLES as u64 {
                return Err(HarnessError::new(format!("{n} samples")));
            }
            let c = build(&k, curve)?;
            let r = tri!(o, c.is_line(&k, *n, *tol));
            o.extend([0.0, b(r.is_some())]);
            if let Some((p, v)) = r {
                pv(&mut o, p);
                pv(&mut o, v);
            }
        }
        OpIn::IsLinear {
            curve,
            tol,
            want_dev,
        } => {
            let c = build(&k, curve)?;
            let r = tri!(o, c.is_linear(&k, *tol, *want_dev));
            o.extend([0.0, b(r.linear)]);
            if r.linear {
                pv(&mut o, r.line_point);
                pv(&mut o, r.line_tangent);
            }
            o.push(r.deviation);
        }
        OpIn::Domain { curve, domain, tol } => {
            let c = build(&k, curve)?;
            let d = Extent2d::from_bounds(domain[0], domain[1], domain[2], domain[3]);
            let r = tri!(o, c.is_domain_boundary(&k, &d, *tol));
            o.extend([0.0, f64::from(r as i32)]);
        }
        OpIn::Periodic { curve, ivl } => {
            let c = build(&k, curve)?;
            let r = tri!(o, c.is_periodic(&k, &ivl_of(*ivl)));
            o.extend([0.0, b(r)]);
        }
        OpIn::Length { curve, ivl, acc } => {
            let c = build(&k, curve)?;
            let r = tri!(o, c.length(&k, &ivl_of(*ivl), *acc));
            o.extend([0.0, r]);
        }
        OpIn::Lpa {
            curve,
            ivl,
            prop,
            guess,
            val,
            vecs,
        } => {
            let c = build(&k, curve)?;
            let prop =
                CurvePropertyType::from_code(*prop).ok_or_else(|| harness("property code"))?;
            let vecs: Vec<Vec3> = vecs.iter().map(|v| v3(*v)).collect();
            let mut sol: Solution<()> = Solution::new();
            let found = tri!(
                o,
                local_property_analysis(&k, &c, &ivl_of(*ivl), prop, *guess, *val, &vecs, &mut sol)
            );
            o.extend([0.0, b(found)]);
            if found {
                o.extend([
                    f64::from(sol.solution_type as i32),
                    sol.start.parameters[0],
                    sol.start.solution_value,
                ]);
            }
        }
        OpIn::Cci { c1, c2, x, jac } => {
            let a = build(&k, c1)?;
            let bb = build(&k, c2)?;
            let mut e = CcIntersectEnfo::new(&k, &a, &bb, EFF_ZERO_SQ);
            let mut f = [0.0; 2];
            let mut j = Matrix::new(2, 2);
            let r = e.evaluate_equations(&k, x, &mut f, jac.then_some(&mut j));
            if let Err(d) = put_eval(&mut o, &r, f, &j) {
                return Ok(*d);
            }
            o.extend([e.solution_size, e.scaled_zero]);
        }
        OpIn::CcTan { c1, c2, x, jac } => {
            let a = build(&k, c1)?;
            let bb = build(&k, c2)?;
            let mut e = CcTangentIntEnfo::new(&k, &a, &bb, EFF_ZERO_SQ);
            let mut f = [0.0; 2];
            let mut j = Matrix::new(2, 2);
            let r = e.evaluate(x, &mut f, jac.then_some(&mut j));
            if let Err(d) = put_eval(&mut o, &r, f, &j) {
                return Ok(*d);
            }
        }
        OpIn::Normal {
            c1,
            c2,
            sqtol,
            sqdist,
            sop,
            vecs,
            calls,
        } => {
            let a = build(&k, c1)?;
            let bb = build(&k, c2)?;
            let sop = SolverOperation::from_code(*sop).ok_or_else(|| harness("operation"))?;
            let vecs: Vec<Vec3> = vecs.iter().map(|v| v3(*v)).collect();
            let opt = if vecs.is_empty() {
                None
            } else {
                Some(vecs.as_slice())
            };
            let mut e = FindCcNormalEnfo::new(&k, &a, &bb, *sqtol, *sqdist, sop, opt);
            o.push(0.0);
            for call in calls {
                let mut f = [0.0; 2];
                let mut j = Matrix::new(2, 2);
                let r = e.evaluate_equations(&k, &call.x, &mut f, call.jac.then_some(&mut j));
                if let Err(d) = put_eval(&mut o, &r, f, &j) {
                    return Ok(*d);
                }
            }
        }
    }
    Ok(to_output(&o))
}

// ---- generators -------------------------------------------------------------------

fn rp(rng: &mut Rng, s: f64) -> [f64; 3] {
    rng.point3(-s, s)
}

/// A random clamped B-spline (sometimes rational, 2D or with repeated interior knots).
fn gen_bspline(rng: &mut Rng) -> BSplineIn {
    let p = 1 + rng.below(4) as usize;
    let n = p + rng.below(6) as usize;
    let rational = rng.chance(0.25);
    let two_d = rng.chance(0.15);
    let a = if rng.chance(0.5) {
        0.0
    } else {
        rng.uniform(-5.0, 5.0)
    };
    let len = if rng.chance(0.5) {
        1.0
    } else {
        rng.uniform(0.1, 10.0)
    };
    let mut u = vec![a; p + 1];
    let inner = n - p;
    let mut ks: Vec<f64> = (0..inner)
        .map(|_| a + len * rng.uniform(0.05, 0.95))
        .collect();
    ks.sort_by(f64::total_cmp);
    if inner >= 2 && p >= 2 && rng.chance(0.3) {
        if let Some(&v) = ks.first() {
            if let Some(x) = ks.get_mut(1) {
                *x = v;
            }
        }
    }
    u.extend(ks);
    u.extend(vec![a + len; p + 1]);
    let scale = if rng.chance(0.5) { 5.0 } else { 100.0 };
    let cpts = (0..=n)
        .map(|i| {
            let [x, y, z] = rp(rng, scale);
            let x = x + i as f64 * scale * 0.2;
            let w = if rational { rng.uniform(0.3, 3.0) } else { NOW };
            let z = if two_d { NOZ } else { z };
            if rational {
                [x * w, y * w, if two_d { z } else { z * w }, w]
            } else {
                [x, y, z, w]
            }
        })
        .collect();
    let dim = if two_d || rng.chance(0.1) { 2 } else { 3 };
    BSplineIn {
        dim,
        oob: rng.chance(0.1),
        cpts,
        p,
        u,
        raw_knots: false,
    }
}

/// A non-rational B-spline whose control points lie on the segment `p`..`q`
/// (degree `deg`, evenly spaced: a constant speed line; else a line of varying speed).
fn gen_linear_bspline(rng: &mut Rng, p: [f64; 3], q: [f64; 3], deg: usize, even: bool) -> BSplineIn {
    let n = deg + rng.below(3) as usize;
    let mut fr: Vec<f64> = (0..=n).map(|i| i as f64 / n as f64).collect();
    if !even {
        for f in fr.iter_mut().skip(1).take(n.saturating_sub(1)) {
            *f = rng.uniform(0.05, 0.95);
        }
        fr.sort_by(f64::total_cmp);
    }
    let cpts = fr
        .iter()
        .map(|&t| {
            [
                p[0] + t * (q[0] - p[0]),
                p[1] + t * (q[1] - p[1]),
                p[2] + t * (q[2] - p[2]),
                NOW,
            ]
        })
        .collect();
    let mut u = vec![0.0; deg + 1];
    let inner = n - deg;
    for i in 1..=inner {
        u.push(i as f64 / (inner + 1) as f64);
    }
    u.extend(vec![1.0; deg + 1]);
    BSplineIn {
        dim: 3,
        oob: false,
        cpts,
        p: deg,
        u,
        raw_knots: false,
    }
}

fn gen_hermite(rng: &mut Rng) -> HermiteIn {
    let s = if rng.chance(0.5) { 3.0 } else { 50.0 };
    let mut d1 = rp(rng, s);
    if rng.chance(0.1) {
        d1 = [0.0; 3];
    }
    let p1 = rp(rng, s);
    let mut p2 = rp(rng, s);
    if rng.chance(0.1) {
        p2 = p1;
    }
    let mut h = HermiteIn {
        dim: if rng.chance(0.2) { 2 } else { 3 },
        p1,
        d1,
        p2,
        d2: if rng.chance(0.08) { [0.0; 3] } else { rp(rng, s) },
    };
    if h.dim == 2 {
        for v in [&mut h.p1, &mut h.d1, &mut h.p2, &mut h.d2] {
            v[2] = 0.0;
        }
    }
    h
}

fn gen_line(rng: &mut Rng) -> LineIn {
    let d0 = rng.uniform(-5.0, 5.0);
    LineIn {
        p: rp(rng, 10.0),
        v: rp(rng, 3.0),
        domain: [d0, d0 + rng.uniform(0.5, 10.0)],
        scale: *rng.choose(&[1.0, 1.0, 2.0, 0.5]).unwrap_or(&1.0),
        inside_out: rng.chance(0.3),
    }
}

fn gen_arc(rng: &mut Rng, full: bool) -> CurveIn {
    let a0 = rng.uniform(-90.0, 90.0);
    let c = CurveIn::Arc(ArcIn {
        center: rp(rng, 5.0),
        x: [1.0, 0.0, 0.0],
        y: [0.0, rng.uniform(0.5, 1.0), rng.uniform(-0.5, 0.5)],
        radius: rng.uniform(0.5, 10.0),
        a0,
        a1: if full {
            a0 + 360.0
        } else {
            a0 + rng.uniform(20.0, 359.0)
        },
    });
    resolve(&c).unwrap_or(c)
}

/// Orthogonal-ish placement axes.
fn gen_axes(rng: &mut Rng) -> ([f64; 3], [f64; 3]) {
    if rng.chance(0.5) {
        ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0])
    } else {
        let x = [1.0, rng.uniform(-0.5, 0.5), rng.uniform(-0.5, 0.5)];
        let y = [rng.uniform(-0.3, 0.3), 1.0, rng.uniform(-0.5, 0.5)];
        (x, y)
    }
}

fn gen_domain(rng: &mut Rng, full: bool) -> [f64; 2] {
    let d0 = *rng.choose(&[0.0, 0.0, -90.0, 30.0]).unwrap_or(&0.0);
    if full {
        [d0, d0 + 360.0]
    } else {
        [d0, d0 + rng.uniform(10.0, 359.0)]
    }
}

fn gen_ellipse(rng: &mut Rng, full: bool) -> CurveIn {
    let (x, y) = gen_axes(rng);
    let r = rng.uniform(0.5, 10.0);
    CurveIn::Ellipse(EllipseIn {
        c: rp(rng, 5.0),
        x,
        y,
        domain: gen_domain(rng, full),
        radii: [r, r * rng.uniform(0.2, 0.9)],
        inside_out: rng.chance(0.2),
    })
}

fn gen_circle(rng: &mut Rng, full: bool) -> CurveIn {
    let (x, y) = gen_axes(rng);
    CurveIn::Circle(CircleIn {
        c: rp(rng, 5.0),
        x,
        y,
        domain: gen_domain(rng, full),
        radius: rng.uniform(0.5, 10.0),
        inside_out: rng.chance(0.2),
    })
}

fn gen_curve(rng: &mut Rng) -> CurveIn {
    match rng.below(24) {
        0..=1 => {
            let full = rng.chance(0.3);
            gen_arc(rng, full)
        },
        2..=4 => CurveIn::Line(gen_line(rng)),
        5..=12 => CurveIn::Bspline(gen_bspline(rng)),
        13..=14 => {
            let full = rng.chance(0.3);
            gen_ellipse(rng, full)
        },
        15..=16 => {
            let full = rng.chance(0.3);
            gen_circle(rng, full)
        },
        _ => CurveIn::Hermite(gen_hermite(rng)),
    }
}

/// A curve that is closed (or nearly) over its natural interval.
fn gen_closed_curve(rng: &mut Rng) -> CurveIn {
    match rng.below(5) {
        0 => gen_arc(rng, true),
        1 => gen_circle(rng, true),
        2 => gen_ellipse(rng, true),
        3 => {
            let mut h = gen_hermite(rng);
            h.p2 = h.p1;
            if rng.chance(0.3) {
                h.p2[0] += *rng.choose(&[1e-13, 1e-9, 1e-4]).unwrap_or(&1e-9);
            }
            if rng.chance(0.3) {
                h.d2 = h.d1;
            }
            CurveIn::Hermite(h)
        }
        _ => {
            let mut bs = gen_bspline(rng);
            if let Some(f) = bs.cpts.first().copied() {
                if let Some(l) = bs.cpts.last_mut() {
                    *l = f;
                }
            }
            CurveIn::Bspline(bs)
        }
    }
}

/// The natural interval of `c` (the Rust curve's).
fn range_of(c: &CurveIn) -> (f64, f64) {
    let k = Kernel::default();
    match build(&k, c) {
        Ok(curve) => {
            let nat = curve.natural_interval();
            (nat.min(), nat.max())
        }
        Err(_) => (0.0, 1.0),
    }
}

/// A parameter of `c`: interior, at an end, or slightly outside.
fn gen_t(rng: &mut Rng, c: &CurveIn) -> f64 {
    let (a, bb) = range_of(c);
    match rng.below(10) {
        0 => a,
        1 => bb,
        2 => {
            if let CurveIn::Bspline(bs) = c {
                *rng.choose(&bs.u).unwrap_or(&a)
            } else {
                0.5 * (a + bb)
            }
        }
        3 => a + (bb - a) * rng.uniform(-0.3, 1.3),
        _ => a + (bb - a) * rng.unit(),
    }
}

fn gen_ivl(rng: &mut Rng, c: &CurveIn) -> [f64; 2] {
    let (a, bb) = range_of(c);
    if rng.chance(0.4) {
        return [a, bb];
    }
    let x = a + (bb - a) * rng.uniform(-0.2, 1.2);
    let y = if rng.chance(0.05) {
        x
    } else {
        a + (bb - a) * rng.uniform(-0.2, 1.2)
    };
    [x.min(y), x.max(y)]
}

fn gen_vecs(rng: &mut Rng, n: usize) -> Vec<[f64; 3]> {
    (0..n)
        .map(|_| {
            let v = rp(rng, 3.0);
            if rng.chance(0.3) {
                let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
                [v[0] / len, v[1] / len, v[2] / len]
            } else {
                v
            }
        })
        .collect()
}

/// A chain of B-spline segments through random points, some of them tiny (shorter
/// than the clean-up tolerance), and tiny closed loops whose chain neighbours are long:
/// upstream `TrimGap` crashes (SIGSEGV) closing the gap next to a closed curve, see
/// `plan/findings.md`.
fn gen_cleanup(rng: &mut Rng) -> OpIn {
    let tol = *rng.choose(&[1e-3, 1e-2, 0.1, 0.5]).unwrap_or(&0.1);
    let n = 2 + rng.below(6) as usize;
    let tiny: Vec<bool> = (0..n).map(|_| rng.chance(0.35)).collect();
    let mut pts = vec![rp(rng, 10.0)];
    for &t in &tiny {
        let last = *pts.last().unwrap_or(&[0.0; 3]);
        let step = if t {
            tol * rng.uniform(0.01, 0.6)
        } else {
            rng.uniform(1.0, 6.0)
        };
        let d = rp(rng, 1.0);
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-6);
        pts.push([
            last[0] + d[0] / len * step,
            last[1] + d[1] / len * step,
            last[2] + d[2] / len * step,
        ]);
    }
    let long_at = |i: Option<usize>| i.is_none_or(|i| tiny.get(i).is_none_or(|t| !t));
    let mut curves = Vec::with_capacity(n);
    let mut prev_loop = false;
    for (i, w) in pts.windows(2).enumerate() {
        let (Some(&p), Some(&q)) = (w.first(), w.get(1)) else {
            continue;
        };
        let neighbours_long = long_at(i.checked_sub(1)) && long_at(Some(i + 1));
        let is_tiny = tiny.get(i).copied().unwrap_or(false);
        if !is_tiny && !prev_loop && neighbours_long && rng.chance(0.25) {
            // a tiny closed loop at p, then the chain goes on from p
            let mut bs = gen_linear_bspline(rng, p, p, 2, true);
            if let Some(mid) = bs.cpts.get_mut(1) {
                mid[1] += 3.0 * tol;
            }
            curves.push(CurveIn::Bspline(bs));
            prev_loop = true;
            // keep the segment too (the loop's other neighbour is it)
        } else {
            prev_loop = false;
        }
        let deg = 1 + rng.below(3) as usize;
        let even = rng.chance(0.5);
        let mut bs = gen_linear_bspline(rng, p, q, deg, even);
        if deg > 1 && rng.chance(0.5) {
            let m = bs.cpts.len();
            if let Some(mid) = bs.cpts.get_mut(m / 2) {
                mid[2] += rng.uniform(-0.5, 0.5) * if is_tiny { tol } else { 1.0 };
            }
        }
        curves.push(CurveIn::Bspline(bs));
    }
    OpIn::Cleanup { curves, tol }
}

/// A list with copies, near copies and empty entries.
fn gen_remove_dup(rng: &mut Rng) -> OpIn {
    let n = rng.below(7) as usize;
    let mut curves: Vec<Option<CurveIn>> = Vec::new();
    for _ in 0..n {
        let r = rng.below(10);
        if r == 0 {
            curves.push(None);
        } else if r < 5 && !curves.is_empty() {
            let i = rng.below(curves.len() as u64) as usize;
            let mut c = curves.get(i).cloned().flatten();
            if let Some(CurveIn::Bspline(bs)) = c.as_mut() {
                if rng.chance(0.5) {
                    let eps = *rng.choose(&[1e-14, 1e-12, 1e-9, 1e-5]).unwrap_or(&1e-9);
                    if let Some(p) = bs.cpts.last_mut() {
                        p[0] += eps;
                    }
                }
            }
            curves.push(c.or_else(|| Some(gen_curve(rng))));
        } else {
            curves.push(Some(gen_curve(rng)));
        }
    }
    OpIn::RemoveDup { curves }
}

/// A 2D curve on or near a side of `domain`.
fn gen_domain_case(rng: &mut Rng) -> OpIn {
    let u0 = rng.uniform(-5.0, 0.0);
    let v0 = rng.uniform(-5.0, 0.0);
    let u1 = u0 + rng.uniform(0.5, 5.0);
    let v1 = v0 + rng.uniform(0.5, 5.0);
    let domain = [u0, v0, u1, v1];
    let tol = *rng.choose(&[0.0, 1e-12, 1e-9, 1e-6, 1e-3]).unwrap_or(&1e-9);
    let side = rng.below(5);
    let fix = |t: f64| -> [f64; 3] {
        match side {
            0 => [u0, v0 + t * (v1 - v0), 0.0],
            1 => [u1, v0 + t * (v1 - v0), 0.0],
            2 => [u0 + t * (u1 - u0), v0, 0.0],
            3 => [u0 + t * (u1 - u0), v1, 0.0],
            _ => [u0 + t * (u1 - u0), v0 + t * (v1 - v0), 0.0],
        }
    };
    let ta = rng.uniform(-0.2, 0.6);
    let tb = rng.uniform(0.4, 1.2);
    let (p, q) = (fix(ta), fix(tb));
    let curve = match rng.below(4) {
        0 => {
            let deg = 1 + rng.below(3) as usize;
            let even = rng.chance(0.5);
            let mut bs = gen_linear_bspline(rng, p, q, deg, even);
            if rng.chance(0.3) {
                let n = bs.cpts.len();
                if let Some(mid) = bs.cpts.get_mut(n / 2) {
                    let eps = *rng.choose(&[1e-13, 1e-8, 1e-2]).unwrap_or(&1e-8);
                    mid[0] += eps;
                    mid[1] += eps;
                }
            }
            bs.dim = 2;
            for c in &mut bs.cpts {
                c[2] = NOZ;
            }
            CurveIn::Bspline(bs)
        }
        1 => {
            let d = [q[0] - p[0], q[1] - p[1], 0.0];
            let bend = if rng.chance(0.5) { 0.0 } else { rng.uniform(-1.0, 1.0) };
            CurveIn::Hermite(HermiteIn {
                dim: 2,
                p1: p,
                d1: [d[0] + bend, d[1] - bend, 0.0],
                p2: q,
                d2: d,
            })
        }
        2 => CurveIn::Line(LineIn {
            p,
            v: [q[0] - p[0], q[1] - p[1], 0.0],
            domain: [0.0, 1.0],
            scale: 1.0,
            inside_out: rng.chance(0.3),
        }),
        _ => gen_curve(rng),
    };
    OpIn::Domain {
        curve,
        domain,
        tol,
    }
}

/// Properties with the option vectors and value they need.
fn gen_lpa(rng: &mut Rng) -> OpIn {
    let curve = gen_curve(rng);
    let prop = rng.below(29) as i32;
    let need = match prop {
        0 | 4 | 5 | 6 | 16 | 23 | 25 => 1,
        17..=19 => 3,
        20..=22 | 24 | 26..=28 => 2,
        _ => 0,
    };
    let nv = if need == 0 && rng.chance(0.5) {
        rng.below(3) as usize
    } else {
        need
    };
    let mut vecs = gen_vecs(rng, nv);
    // points near the curve for the point properties
    if matches!(prop, 6 | 23 | 25) {
        let k = Kernel::default();
        if let Ok(c) = build(&k, &curve) {
            let t = gen_t(rng, &curve);
            if let (Ok(p), Some(v)) = (c.evaluate_point(&k, t), vecs.first_mut()) {
                let d = rp(rng, 0.5);
                *v = [p.x + d[0], p.y + d[1], p.z + d[2]];
            }
        }
    }
    let val = if matches!(prop, 2 | 3 | 16 | 22 | 23 | 24) || rng.chance(0.3) {
        Some(rng.uniform(0.1, 5.0))
    } else {
        None
    };
    OpIn::Lpa {
        ivl: gen_ivl(rng, &curve),
        guess: gen_t(rng, &curve),
        curve,
        prop,
        val,
        vecs,
    }
}

/// Two curves and parameters near where they meet (crossing, tangent, apart).
fn gen_pair(rng: &mut Rng) -> (CurveIn, CurveIn) {
    match rng.below(4) {
        0 => {
            // a circle and a line tangent to it (or nearly)
            let r = rng.uniform(1.0, 5.0);
            let eps = *rng.choose(&[0.0, 0.0, 1e-9, 1e-4, 0.05]).unwrap_or(&0.0);
            let c = CurveIn::Circle(CircleIn {
                c: [0.0, 0.0, 0.0],
                x: [1.0, 0.0, 0.0],
                y: [0.0, 1.0, 0.0],
                domain: [0.0, 360.0],
                radius: r,
                inside_out: false,
            });
            let l = CurveIn::Line(LineIn {
                p: [-r, r + eps, 0.0],
                v: [2.0 * r, 0.0, 0.0],
                domain: [0.0, 1.0],
                scale: 1.0,
                inside_out: false,
            });
            (c, l)
        }
        1 => {
            // two crossing segments
            let p = rp(rng, 5.0);
            let q = rp(rng, 5.0);
            let m = [
                0.5 * (p[0] + q[0]),
                0.5 * (p[1] + q[1]),
                0.5 * (p[2] + q[2]),
            ];
            let d = rp(rng, 3.0);
            let deg = 1 + rng.below(3) as usize;
            let a = gen_linear_bspline(rng, p, q, deg, true);
            let bb = CurveIn::Hermite(HermiteIn {
                dim: 3,
                p1: [m[0] - d[0], m[1] - d[1], m[2] - d[2]],
                d1: [2.0 * d[0], 2.0 * d[1] + 0.5, 2.0 * d[2]],
                p2: [m[0] + d[0], m[1] + d[1], m[2] + d[2]],
                d2: [2.0 * d[0], 2.0 * d[1] - 0.5, 2.0 * d[2]],
            });
            (CurveIn::Bspline(a), bb)
        }
        2 => {
            // the same curve twice (coincident)
            let c = gen_curve(rng);
            (c.clone(), c)
        }
        _ => (gen_curve(rng), gen_curve(rng)),
    }
}

fn gen_x(rng: &mut Rng, a: &CurveIn, b: &CurveIn) -> [f64; 2] {
    let x0 = gen_t(rng, a);
    let x1 = if rng.chance(0.3) {
        // the same normalised position on the other curve
        let (a0, a1) = range_of(a);
        let (b0, b1) = range_of(b);
        let f = if a1 > a0 { (x0 - a0) / (a1 - a0) } else { 0.5 };
        b0 + f * (b1 - b0)
    } else {
        gen_t(rng, b)
    };
    [x0, x1]
}

fn gen_normal(rng: &mut Rng) -> OpIn {
    let (c1, c2) = gen_pair(rng);
    let sop = *rng
        .choose(&[0, 3, 4, 5, 6, 11, 12, 13, 14, 15, 19, 22, 23])
        .unwrap_or(&5);
    let nv = match sop {
        11..=13 | 23 => 2,
        14 | 15 | 22 => {
            if rng.chance(0.1) {
                0
            } else {
                1 + rng.below(2) as usize
            }
        }
        _ => rng.below(3) as usize,
    };
    let vecs = gen_vecs(rng, nv);
    let ncalls = 1 + rng.below(5) as usize;
    let mut calls = Vec::with_capacity(ncalls);
    let first = gen_x(rng, &c1, &c2);
    for i in 0..ncalls {
        let x = if i == 0 || rng.chance(0.4) {
            first
        } else {
            gen_x(rng, &c1, &c2)
        };
        calls.push(CallIn {
            x,
            jac: rng.chance(0.75),
        });
    }
    OpIn::Normal {
        c1,
        c2,
        sqtol: *rng.choose(&[1e-12, 1e-6, 1e-2, 1.0]).unwrap_or(&1e-6),
        sqdist: rng.uniform(0.0, 4.0),
        sop,
        vecs,
        calls,
    }
}

/// A curve/plane crossing near tangency: `abc` nearly perpendicular to the tangent.
fn gen_cp_guess(rng: &mut Rng) -> OpIn {
    let curve = gen_curve(rng);
    let t = gen_t(rng, &curve);
    let mut abc = rp(rng, 1.0);
    if rng.chance(0.7) {
        let k = Kernel::default();
        if let Ok(c) = build(&k, &curve) {
            if let Ok(v) = c.evaluate(&k, t, 1, true, true) {
                if let Some(tan) = v.get(1) {
                    // the plane normal: perpendicular to the tangent, tilted a little
                    let side = tan.cross(v3(rp(rng, 1.0)));
                    let tilt = rng.uniform(-0.2, 0.2);
                    let n = side + *tan * tilt;
                    abc = [n.x, n.y, n.z];
                }
            }
        }
    }
    OpIn::CpGuess {
        ivl: gen_ivl(rng, &curve),
        curve,
        abc,
        t,
    }
}

#[allow(clippy::too_many_lines)]
fn gen_case(rng: &mut Rng) -> OpIn {
    match rng.below(32) {
        0..=1 => gen_cleanup(rng),
        2..=3 => gen_remove_dup(rng),
        4 => {
            let curve = gen_curve(rng);
            let t = match &curve {
                CurveIn::Ellipse(e) => e.domain[0] + (e.domain[1] - e.domain[0]) * rng.unit(),
                CurveIn::Circle(e) => e.domain[0] + (e.domain[1] - e.domain[0]) * rng.unit(),
                CurveIn::Line(l) => l.domain[0] + (l.domain[1] - l.domain[0]) * rng.unit(),
                _ => gen_t(rng, &curve),
            };
            OpIn::StepPoint { curve, t }
        }
        5..=6 => {
            let c1 = if rng.chance(0.8) {
                gen_closed_curve(rng)
            } else {
                gen_curve(rng)
            };
            let c2 = if rng.chance(0.4) {
                c1.clone()
            } else {
                gen_curve(rng)
            };
            let (a0, a1) = range_of(&c1);
            let s0 = match rng.below(4) {
                0 => a0,
                1 => a1,
                2 => a1 - 1e-12 * (a1 - a0),
                _ => gen_t(rng, &c1),
            };
            OpIn::Seam {
                ivl1: if rng.chance(0.8) {
                    [a0, a1]
                } else {
                    gen_ivl(rng, &c1)
                },
                ivl2: if rng.chance(0.8) {
                    let r = range_of(&c2);
                    [r.0, r.1]
                } else {
                    gen_ivl(rng, &c2)
                },
                s: [s0, gen_t(rng, &c2)],
                c1,
                c2,
                sol_type: if rng.chance(0.85) { 1 } else { 2 },
                this_first: rng.chance(0.7),
            }
        }
        7 => OpIn::Cache {
            curve: gen_curve(rng),
            cache_type: if rng.chance(0.8) {
                0
            } else {
                rng.below(4) as i32
            },
            with_old: rng.chance(0.3),
        },
        8..=9 => gen_cp_guess(rng),
        10..=12 => {
            let curve = match rng.below(4) {
                0 => {
            let full = rng.chance(0.3);
            gen_arc(rng, full)
        },
                1 => {
            let full = rng.chance(0.3);
            gen_circle(rng, full)
        },
                2 => {
            let full = rng.chance(0.3);
            gen_ellipse(rng, full)
        },
                _ => gen_curve(rng),
            };
            OpIn::IsArc {
                curve,
                n: *rng.choose(&[0, 1, 2, 3, 5, 20]).unwrap_or(&5),
                tol: *rng.choose(&[0.0, 1e-12, 1e-9, 1e-6, 1e-3]).unwrap_or(&1e-6),
            }
        }
        13..=14 => {
            let p = rp(rng, 5.0);
            let q = rp(rng, 5.0);
            let curve = match rng.below(3) {
                0 => {
                    let deg = 1 + rng.below(3) as usize;
                    let even = rng.chance(0.6);
                    CurveIn::Bspline(gen_linear_bspline(rng, p, q, deg, even))
                }
                1 => CurveIn::Line(gen_line(rng)),
                _ => gen_curve(rng),
            };
            if rng.chance(0.5) {
                OpIn::IsLine {
                    curve,
                    n: *rng.choose(&[0, 3, 5, 10]).unwrap_or(&5),
                    tol: *rng.choose(&[0.0, 1e-12, 1e-9, 1e-6, 1e-3]).unwrap_or(&1e-6),
                }
            } else {
                OpIn::IsLinear {
                    curve,
                    tol: *rng.choose(&[0.0, 1e-12, 1e-9, 1e-6, 1e-3]).unwrap_or(&1e-6),
                    want_dev: rng.chance(0.5),
                }
            }
        }
        15..=16 => gen_domain_case(rng),
        17..=18 => {
            let curve = if rng.chance(0.7) {
                gen_closed_curve(rng)
            } else {
                gen_curve(rng)
            };
            let ivl = if rng.chance(0.8) {
                let r = range_of(&curve);
                [r.0, r.1]
            } else {
                gen_ivl(rng, &curve)
            };
            OpIn::Periodic { curve, ivl }
        }
        19..=20 => {
            let p = rp(rng, 5.0);
            let q = rp(rng, 5.0);
            let curve = match rng.below(4) {
                0 => {
                    let even = rng.chance(0.5);
                    CurveIn::Bspline(gen_linear_bspline(rng, p, q, 1, even))
                }
                1 => {
                    let mut bs = gen_bspline(rng);
                    bs.p = 1;
                    let n = bs.cpts.len() - 1;
                    let mut u = vec![0.0, 0.0];
                    for i in 1..n {
                        u.push(i as f64 / n as f64);
                    }
                    u.extend([1.0, 1.0]);
                    bs.u = u;
                    CurveIn::Bspline(bs)
                }
                2 => CurveIn::Line(gen_line(rng)),
                _ => gen_curve(rng),
            };
            let ivl = gen_ivl(rng, &curve);
            OpIn::Length {
                curve,
                ivl,
                acc: *rng.choose(&[1e-3, 1e-6, 1e-8]).unwrap_or(&1e-6),
            }
        }
        21..=24 => gen_lpa(rng),
        25..=26 => {
            let (c1, c2) = gen_pair(rng);
            let x = gen_x(rng, &c1, &c2);
            OpIn::Cci {
                c1,
                c2,
                x,
                jac: rng.chance(0.8),
            }
        }
        27 => {
            let (c1, c2) = gen_pair(rng);
            let x = gen_x(rng, &c1, &c2);
            OpIn::CcTan {
                c1,
                c2,
                x,
                jac: rng.chance(0.8),
            }
        }
        _ => gen_normal(rng),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "curve.curve_p5"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let op = gen_case(rng);
            let Ok((code, input)) = cpp_input(&op) else {
                continue;
            };
            if ffi::validate(code, &input).is_err() {
                continue;
            }
            return serde_json::to_value(op).ok();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_cases_are_valid_and_diverse() {
        let mut rng = Rng::new(7);
        let mut ops = std::collections::BTreeSet::new();
        for _ in 0..400 {
            let v = Runner.generate(&mut rng).expect("case");
            let op: OpIn = serde_json::from_value(v).expect("parse");
            ops.insert(format!("{:?}", op_of(&op)));
        }
        assert!(ops.len() >= 14, "{ops:?}");
    }
}
