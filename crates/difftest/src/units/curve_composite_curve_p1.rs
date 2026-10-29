//! Runner for unit `curve.composite_curve_p1`: `SmCompositeCurve` part 1 (construction,
//! type queries, `GetCurveSegment`, `GetCanonical`, `GetNaturalInterval`, `GetKnots`,
//! `CalculateBoundingBox`, `operator==`, `Copy`, `Transform`, `ApproximateCurve`,
//! `ComputeProjectedLoopOrientation`, `BuildCompositesFromCurves`, the file-static corner
//! builder, classifier and break helpers, and the type queries of the segment, region
//! and offset map attribute classes).
//!
//! C++: `solidcore_ffi::units::curve_composite_curve_p1` (upstream
//! `source/SMLib/src/SmCompositeCurve.cpp`; the file-static helpers through verbatim shim
//! copies). Rust: `solidcore::curve::composite_curve::p1`.
//!
//! A case is one `op` with its curves (`{"bspline": {dim, oob, cpts, p, u}}` with control
//! points as stored `[x, y, z, w]`, `{"hermite": {dim, p1, d1, p2, d2}}`, `{"circle":
//! ..}` (a quadratic B-spline arc), `{"segment": {a, b}}` (`CreateLineSegment(3, a, b)`)
//! or `{"line": {p, v, domain, scale, inside_out}}`; arcs and segments are built by the
//! Rust B-spline constructors, bit-identical to upstream, and handed to both sides as
//! inline B-spline data), composites (`{dim, curves, closed, senses?, gaps?}`) and
//! parameters. Both sides produce the flat record documented in the ffi module; the
//! runner reports its first entry as the `status` int (`0` = `SM_SUCCESS`, else the
//! `SmStatus`; `1007` for a Rust `InvalidInput`, `-1` for a Rust error upstream cannot
//! produce) and the rest as the `out` values.
//!
//! Deferred cases (unit `curve.curve_p5`): `ApproximateCurve`'s `AssertValid` of the
//! joined curve once it reaches `GlobalPropertyAnalysis` (`LocalPropertyAnalysis`), and
//! corner chamfers and classifications whose intersections or point solves reach the
//! unported curve/curve or analytic solvers.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::composite_curve::p1::{
    self as cc, create_corner, curve_classify, insert_sorted_breaks, region_is_kind_of,
    trim_first_break, trim_last_break, CompositeCurve, CompositeCurveSegment, OffsetCornerType,
    OffsetMapAttribute, REGION_TYPE, SEGMENT_TYPE,
};
use solidcore::curve::curve::p2::{Curve, CurveKind};
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BoundingBoxRequest};
use solidcore::nurbs::math::{NOW, NOZ};
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::SolidError;
use solidcore_ffi::units::curve_composite_curve_p1::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `curve.composite_curve_p1` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

const UNDEF: f64 = 1.23456e-61;

/// Type codes asked about by `info` and `types` cases.
const TYPES: [i64; 12] = [
    11001, 12060, 12070, 14001, 14010, 14032, 14033, 14034, 14038, 14040, 14200, 16016,
];

// ---- inputs -----------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BSplineIn {
    dim: u64,
    #[serde(default)]
    oob: bool,
    cpts: Vec<[f64; 4]>,
    p: usize,
    u: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HermiteIn {
    dim: u64,
    p1: [f64; 3],
    d1: [f64; 3],
    p2: [f64; 3],
    d2: [f64; 3],
}

/// A quadratic circular arc built by the Rust `CreateCircleSegment`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CircleIn {
    center: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    radius: f64,
    a0: f64,
    a1: f64,
}

/// A line segment built by the Rust `CreateLineSegment(3, a, b)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SegmentIn {
    a: [f64; 3],
    b: [f64; 3],
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
#[serde(rename_all = "snake_case")]
enum CurveIn {
    Bspline(BSplineIn),
    Hermite(HermiteIn),
    Circle(CircleIn),
    Segment(SegmentIn),
    Line(LineIn),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CompIn {
    #[serde(default = "three")]
    dim: u64,
    curves: Vec<CurveIn>,
    #[serde(default)]
    closed: bool,
    #[serde(default)]
    senses: Option<Vec<bool>>,
    #[serde(default)]
    gaps: Option<Vec<f64>>,
}

fn three() -> u64 {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Info {
        comp: CompIn,
        #[serde(default)]
        types: Vec<i64>,
    },
    Knots {
        comp: CompIn,
    },
    BBox {
        comp: CompIn,
        ivl: [f64; 2],
        pseudo: bool,
        polar: bool,
        #[serde(default = "yes")]
        expand: bool,
    },
    Eq {
        a: CompIn,
        b: CompIn,
    },
    Copy {
        comp: CompIn,
    },
    Transform {
        comp: CompIn,
        origin: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        scale: Option<[f64; 3]>,
    },
    Approx {
        comp: CompIn,
        tol: f64,
    },
    Orient {
        curves: Vec<CurveIn>,
        senses: Vec<bool>,
        normal: [f64; 3],
        bounds: bool,
    },
    Build {
        curves: Vec<CurveIn>,
        same: f64,
        line: f64,
    },
    Corner {
        vertex: [f64; 3],
        p1: [f64; 3],
        v1: [f64; 3],
        p2: [f64; 3],
        v2: [f64; 3],
        corner: u8,
        dist: f64,
        tol: f64,
    },
    Classify {
        curve: CurveIn,
        dist: f64,
        tol: f64,
        origs: Vec<CurveIn>,
        orig: Option<CurveIn>,
    },
    Breaks {
        which: u8,
        v: Vec<f64>,
        x: f64,
    },
    Types {
        id: u64,
        corner: bool,
        comp_index: u64,
        curve: u64,
        other: u64,
        err: f64,
        types: Vec<i64>,
    },
}

fn yes() -> bool {
    true
}

// ---- C++ input --------------------------------------------------------------------

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::from_array(p)
}

fn bspline_in(c: &BSplineCurve) -> Result<CurveIn, HarnessError> {
    let cur = c.gw_nurb().ok_or_else(|| harness("curve without NURB"))?;
    Ok(CurveIn::Bspline(BSplineIn {
        dim: c.dim(),
        oob: false,
        cpts: cur.cpts().iter().map(|p| [p.x, p.y, p.z, p.w]).collect(),
        p: cur.degree(),
        u: cur.knots().to_vec(),
    }))
}

/// `c` with an arc or segment replaced by its inline B-spline.
fn resolve(c: &CurveIn) -> Result<CurveIn, HarnessError> {
    match c {
        CurveIn::Circle(ci) => {
            let frame = Axis2Placement::new(v3(ci.center), v3(ci.x), v3(ci.y));
            let arc = BSplineCurve::create_circle_segment(
                3,
                &frame,
                ci.radius,
                ci.a0,
                ci.a1,
                solidcore::nurbs::b_spline_curve::NurbCircleParam::Quadratic,
            )
            .map_err(|e| harness(format!("circle: {e}")))?;
            bspline_in(&arc)
        }
        CurveIn::Segment(s) => {
            let l = BSplineCurve::create_line_segment(3, v3(s.a), v3(s.b))
                .map_err(|e| harness(format!("segment: {e}")))?;
            bspline_in(&l)
        }
        _ => Ok(c.clone()),
    }
}

fn b(x: bool) -> f64 {
    f64::from(u8::from(x))
}

fn curve_block(v: &mut Vec<f64>, c: &CurveIn) -> Result<(), HarnessError> {
    match &resolve(c)? {
        CurveIn::Bspline(bs) => {
            let n = bs.cpts.len().saturating_sub(1);
            let m = bs.u.len().saturating_sub(1);
            v.extend([0.0, bs.dim as f64, b(bs.oob)]);
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
            v.extend_from_slice(&l.p);
            v.extend_from_slice(&l.v);
            v.extend([l.domain[0], l.domain[1], l.scale, b(l.inside_out)]);
        }
        CurveIn::Circle(_) | CurveIn::Segment(_) => return Err(harness("unresolved curve")),
    }
    Ok(())
}

fn comp_block(v: &mut Vec<f64>, c: &CompIn) -> Result<(), HarnessError> {
    v.extend([c.dim as f64, c.curves.len() as f64]);
    for cu in &c.curves {
        curve_block(v, cu)?;
    }
    v.push(b(c.closed));
    // The shim reads one sense and one gap per curve: upstream ignores entries past
    // the curve count and reads past a shorter array (undefined).
    let n = c.curves.len();
    match &c.senses {
        Some(s) => {
            let s = s.get(..n).ok_or_else(|| harness("fewer senses than curves"))?;
            v.push(1.0);
            v.extend(s.iter().map(|x| b(*x)));
        }
        None => v.push(0.0),
    }
    match &c.gaps {
        Some(g) => {
            let g = g.get(..n).ok_or_else(|| harness("fewer gaps than curves"))?;
            v.push(1.0);
            v.extend_from_slice(g);
        }
        None => v.push(0.0),
    }
    Ok(())
}

fn type_list(v: &mut Vec<f64>, t: &[i64]) {
    v.push(t.len() as f64);
    v.extend(t.iter().map(|x| *x as f64));
}

fn cpp_input(op: &OpIn) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut v = Vec::new();
    let code = match op {
        OpIn::Info { comp, types } => {
            comp_block(&mut v, comp)?;
            type_list(&mut v, types);
            Op::Info
        }
        OpIn::Knots { comp } => {
            comp_block(&mut v, comp)?;
            Op::Knots
        }
        OpIn::BBox {
            comp,
            ivl,
            pseudo,
            polar,
            expand,
        } => {
            comp_block(&mut v, comp)?;
            v.extend([ivl[0], ivl[1], b(*pseudo), b(*polar), b(*expand)]);
            Op::BBox
        }
        OpIn::Eq { a, b: bb } => {
            comp_block(&mut v, a)?;
            comp_block(&mut v, bb)?;
            Op::Eq
        }
        OpIn::Copy { comp } => {
            comp_block(&mut v, comp)?;
            Op::Copy
        }
        OpIn::Transform {
            comp,
            origin,
            x,
            y,
            scale,
        } => {
            comp_block(&mut v, comp)?;
            v.extend_from_slice(origin);
            v.extend_from_slice(x);
            v.extend_from_slice(y);
            v.push(b(scale.is_some()));
            v.extend_from_slice(&scale.unwrap_or([1.0; 3]));
            Op::Transform
        }
        OpIn::Approx { comp, tol } => {
            comp_block(&mut v, comp)?;
            v.push(*tol);
            Op::Approx
        }
        OpIn::Orient {
            curves,
            senses,
            normal,
            bounds,
        } => {
            v.push(curves.len() as f64);
            for c in curves {
                curve_block(&mut v, c)?;
            }
            v.extend(senses.iter().map(|x| b(*x)));
            v.extend_from_slice(normal);
            v.push(b(*bounds));
            Op::Orient
        }
        OpIn::Build { curves, same, line } => {
            v.push(curves.len() as f64);
            for c in curves {
                curve_block(&mut v, c)?;
            }
            v.extend([*same, *line]);
            Op::Build
        }
        OpIn::Corner {
            vertex,
            p1,
            v1,
            p2,
            v2,
            corner,
            dist,
            tol,
        } => {
            for p in [vertex, p1, v1, p2, v2] {
                v.extend_from_slice(p);
            }
            v.extend([f64::from(*corner), *dist, *tol]);
            Op::Corner
        }
        OpIn::Classify {
            curve,
            dist,
            tol,
            origs,
            orig,
        } => {
            curve_block(&mut v, curve)?;
            v.extend([*dist, *tol, origs.len() as f64]);
            for c in origs {
                curve_block(&mut v, c)?;
            }
            v.push(b(orig.is_some()));
            if let Some(o) = orig {
                curve_block(&mut v, o)?;
            }
            Op::Classify
        }
        OpIn::Breaks { which, v: vals, x } => {
            v.extend([f64::from(*which), vals.len() as f64]);
            v.extend_from_slice(vals);
            v.push(*x);
            Op::Breaks
        }
        OpIn::Types {
            id,
            corner,
            comp_index,
            curve,
            other,
            err,
            types,
        } => {
            v.extend([
                *id as f64,
                b(*corner),
                *comp_index as f64,
                *curve as f64,
                *other as f64,
                *err,
            ]);
            type_list(&mut v, types);
            Op::Types
        }
    };
    Ok((code, v))
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

fn build(c: &CurveIn) -> Result<Curve, HarnessError> {
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
        CurveIn::Circle(_) | CurveIn::Segment(_) => Err(harness("unresolved curve")),
    }
}

fn build_all(cs: &[CurveIn]) -> Result<Vec<Curve>, HarnessError> {
    cs.iter().map(build).collect()
}

fn pv(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
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

/// Like [`tri!`] inside a helper returning `Result<_, Fail>`.
fn ftry<T>(r: solidcore::Result<T>) -> Result<T, Fail> {
    r.map_err(Fail::from)
}

/// The curve state record (see the ffi module docs).
fn put_curve(k: &Kernel, o: &mut Vec<f64>, c: &Curve) -> Result<(), Fail> {
    let kind = match c.kind() {
        CurveKind::BSpline(_) => 0.0,
        CurveKind::Line(_) => 2.0,
        _ => 1.0,
    };
    o.push(kind);
    let ivl = c.natural_interval();
    o.extend([ivl.min(), ivl.max()]);
    for i in 0..=6 {
        let p = match c.evaluate_point(k, ivl.evaluate(f64::from(i) / 6.0)) {
            Ok(p) => p,
            Err(e) if e.is_unported() => return Err(e.into()),
            Err(_) => Vec3::new(UNDEF, UNDEF, UNDEF),
        };
        pv(o, p);
    }
    if let CurveKind::BSpline(bs) = c.kind() {
        let nurb = bs.gw_nurb().ok_or(Fail::Status(-1.0))?;
        o.push(nurb.cpts().len() as f64);
        for p in nurb.cpts() {
            o.extend([p.x, p.y, p.z, p.w]);
        }
        o.push(nurb.knots().len() as f64);
        o.extend_from_slice(nurb.knots());
    }
    Ok(())
}

fn put_segments(o: &mut Vec<f64>, c: &CompositeCurve) {
    o.extend([c.num_segments() as f64, b(c.is_closed())]);
    for s in c.segments() {
        o.extend([
            f64::from(s.transition as i32),
            b(s.same_sense),
            s.gap_start,
            s.gap_end,
            b(s.owns_curve),
        ]);
    }
}

fn composite(c: &CompIn) -> Result<solidcore::Result<CompositeCurve>, HarnessError> {
    let curves = build_all(&c.curves)?;
    Ok(CompositeCurve::with_curves(
        c.dim,
        curves,
        c.closed,
        c.senses.as_deref(),
        c.gaps.as_deref(),
    ))
}

fn as_comp(c: &Curve) -> Result<&CompositeCurve, Fail> {
    match c.kind() {
        CurveKind::Composite(x) => Ok(x),
        _ => Err(Fail::Status(-1.0)),
    }
}

/// Runs `f`, turning a [`Fail`] into the output.
fn run_body(f: impl FnOnce(&mut Vec<f64>) -> Result<(), Fail>) -> Output {
    let mut o = Vec::new();
    match f(&mut o) {
        Ok(()) => to_output(&o),
        Err(Fail::Deferred(d)) => *d,
        Err(Fail::Status(s)) => {
            o.clear();
            o.push(s);
            to_output(&o)
        }
    }
}

#[allow(clippy::too_many_lines)]
fn rust(op: &OpIn) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let mut o: Vec<f64> = Vec::new();
    match op {
        OpIn::Info { comp, types } => {
            let c = Curve::from(tri!(o, composite(comp)?));
            return Ok(run_body(|o| {
                let x = as_comp(&c)?;
                o.push(0.0);
                put_segments(o, x);
                let ivl = c.natural_interval();
                o.extend([ivl.min(), ivl.max(), c.dim() as f64]);
                o.push(ftry(c.degree())? as f64);
                o.push(ftry(c.number_natural_knots(&k))? as f64);
                o.push(ftry(c.number_control_points())? as f64);
                o.push(b(c.is_analytic()));
                o.push(b(c.is_bounded()));
                o.push(b(ftry(c.has_repeated_end_control_points(&k, 1e-6))?));
                o.push(c.type_code() as f64);
                o.push(match c.evaluate_point(&k, ivl.min()) {
                    Ok(_) => 0.0,
                    Err(e) => status(&e),
                });
                o.push(b(x.segment(x.num_segments()).is_none()));
                let canon = x.canonical();
                o.extend([
                    0.0,
                    canon.dim as f64,
                    b(canon.closed),
                    canon.curves.len() as f64,
                ]);
                o.extend(canon.senses.iter().map(|s| b(*s)));
                o.extend(types.iter().map(|t| b(c.is_kind_of(*t))));
                Ok(())
            }));
        }
        OpIn::Knots { comp } => {
            let c = Curve::from(tri!(o, composite(comp)?));
            let (kn, m) = tri!(o, c.knots(&k, None));
            o.push(0.0);
            o.push(kn.len() as f64);
            o.extend_from_slice(&kn);
            o.push(m.len() as f64);
            o.extend(m.iter().map(|x| *x as f64));
        }
        OpIn::BBox {
            comp,
            ivl,
            pseudo,
            polar,
            expand,
        } => {
            let c = Curve::from(tri!(o, composite(comp)?));
            let req = BoundingBoxRequest {
                normal: true,
                pseudo: *pseudo,
                polar: *polar,
            };
            let ivl = Extent1d::from_min_max(ivl[0], ivl[1]);
            let bx = tri!(o, c.calculate_bounding_box(&k, &ivl, req, *expand));
            o.push(0.0);
            if let Some(nb) = bx.normal {
                o.extend((0..3).map(|i| nb.min_at(i)));
                o.extend((0..3).map(|i| nb.max_at(i)));
            }
            if let Some(pb) = bx.pseudo {
                for v in pb.basis() {
                    pv(&mut o, *v);
                }
                for iv in pb.intervals() {
                    o.extend([iv.min(), iv.max()]);
                }
            }
            if let Some(pl) = bx.polar {
                o.push(f64::from(pl.state().code()));
                for v in pl.basis() {
                    pv(&mut o, *v);
                }
                let d = pl.domain();
                o.extend([d.u_min(), d.v_min(), d.u_max(), d.v_max()]);
            }
        }
        OpIn::Eq { a, b: bb } => {
            let ca = Curve::from(tri!(o, composite(a)?));
            let cb = Curve::from(tri!(o, composite(bb)?));
            let ab = tri!(o, ca.approx_eq(&k, &cb));
            let ba = tri!(o, cb.approx_eq(&k, &ca));
            let aa = tri!(o, ca.approx_eq(&k, &ca));
            o.extend([0.0, b(ab), b(ba), b(aa)]);
            let (CurveKind::Composite(xa), CurveKind::Composite(xb)) = (ca.kind(), cb.kind())
            else {
                return Err(harness("not composites"));
            };
            match (xa.segment(0), xb.segment(0)) {
                (Some(sa), Some(sb)) => {
                    let e = tri!(o, CompositeCurveSegment::approx_eq(sa,&k,  sb));
                    o.push(b(e));
                }
                _ => o.push(-1.0),
            }
        }
        OpIn::Copy { comp } => {
            let c = Curve::from(tri!(o, composite(comp)?));
            let cp = tri!(o, c.copy(&k));
            let eq = tri!(o, cp.approx_eq(&k, &c));
            return Ok(run_body(|o| {
                o.extend([0.0, b(eq), cp.type_code() as f64]);
                let x = as_comp(&cp)?;
                put_segments(o, x);
                for s in x.segments() {
                    put_curve(&k, o, &s.curve)?;
                }
                Ok(())
            }));
        }
        OpIn::Transform {
            comp,
            origin,
            x,
            y,
            scale,
        } => {
            let mut c = Curve::from(tri!(o, composite(comp)?));
            let a2p = Axis2Placement::new(v3(*origin), v3(*x), v3(*y));
            tri!(o, c.transform(&k, &a2p, scale.map(v3)));
            return Ok(run_body(|o| {
                o.push(0.0);
                for s in as_comp(&c)?.segments() {
                    put_curve(&k, o, &s.curve)?;
                }
                Ok(())
            }));
        }
        OpIn::Approx { comp, tol } => {
            let c = tri!(o, composite(comp)?);
            let r = tri!(o, c.approximate_curve(&k, *tol));
            return Ok(run_body(|o| {
                o.extend([0.0, r.max_gap, b(r.curve.is_some())]);
                if let Some(bs) = r.curve {
                    put_curve(&k, o, &Curve::from(bs))?;
                }
                Ok(())
            }));
        }
        OpIn::Orient {
            curves,
            senses,
            normal,
            bounds,
        } => {
            let cs = build_all(curves)?;
            let mut bx = Extent2d::new();
            let r = tri!(
                o,
                CompositeCurve::compute_projected_loop_orientation(&k, 
                    &cs,
                    senses,
                    v3(*normal),
                    bounds.then_some(&mut bx)
                )
            );
            o.extend([0.0, f64::from(r as i32)]);
            if *bounds {
                o.extend([bx.u_min(), bx.v_min(), bx.u_max(), bx.v_max()]);
            }
        }
        OpIn::Build { curves, same, line } => {
            let cs = build_all(curves)?;
            let r = tri!(
                o,
                CompositeCurve::build_composites_from_curves(&k, cs, *same, *line)
            );
            return Ok(run_body(|o| {
                o.extend([0.0, r.composites.len() as f64]);
                for x in &r.composites {
                    o.push(x.dim() as f64);
                    put_segments(o, x);
                    for s in x.segments() {
                        put_curve(&k, o, &s.curve)?;
                    }
                }
                o.push(r.unused.len() as f64);
                Ok(())
            }));
        }
        OpIn::Corner {
            vertex,
            p1,
            v1,
            p2,
            v2,
            corner,
            dist,
            tol,
        } => {
            let ct = match corner {
                0 => OffsetCornerType::LinearExtension,
                1 => OffsetCornerType::FilletCorner,
                _ => OffsetCornerType::LinearChamfer,
            };
            let r = tri!(
                o,
                create_corner(
                    &k,
                    v3(*vertex),
                    v3(*p1),
                    v3(*v1),
                    v3(*p2),
                    v3(*v2),
                    ct,
                    *dist,
                    *tol
                )
            );
            return Ok(run_body(|o| {
                o.push(0.0);
                for c in [&r.curve1, &r.curve2, &r.curve3] {
                    o.push(b(c.is_some()));
                    if let Some(c) = c {
                        put_curve(&k, o, c)?;
                    }
                }
                Ok(())
            }));
        }
        OpIn::Classify {
            curve,
            dist,
            tol,
            origs,
            orig,
        } => {
            let t = build(curve)?;
            let os = build_all(origs)?;
            let refs: Vec<&Curve> = os.iter().collect();
            let og = orig.as_ref().map(build).transpose()?;
            let keep = tri!(
                o,
                curve_classify(&k, &t, *dist, *tol, &refs, og.as_ref())
            );
            o.extend([0.0, b(keep)]);
        }
        OpIn::Breaks { which, v, x } => {
            let mut v = v.clone();
            match which {
                0 => trim_first_break(&mut v, *x),
                1 => trim_last_break(&mut v, *x),
                _ => insert_sorted_breaks(&mut v, *x),
            }
            o.extend([0.0, v.len() as f64]);
            o.extend_from_slice(&v);
        }
        OpIn::Types {
            id,
            corner,
            comp_index,
            curve,
            other,
            err,
            types,
        } => {
            let a = OffsetMapAttribute::new(*id, *corner, false, *comp_index, *curve, *other, None, *err);
            o.extend([
                0.0,
                a.type_code() as f64,
                b(a.corner_offset()),
                a.curve() as f64,
                a.other_curve() as f64,
                b(a.orig_curve().is_none()),
                a.attribute_id as f64,
                REGION_TYPE as f64,
                SEGMENT_TYPE as f64,
            ]);
            for t in types {
                o.extend([
                    b(a.is_kind_of(*t)),
                    b(region_is_kind_of(*t)),
                    b(CompositeCurveSegment::is_kind_of(*t)),
                ]);
            }
            let _ = cc::TYPE;
        }
    }
    Ok(to_output(&o))
}

// ---- generators -------------------------------------------------------------------

fn rp(rng: &mut Rng, s: f64) -> [f64; 3] {
    rng.point3(-s, s)
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale3(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn lerp(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    add(a, scale3(sub(b, a), t))
}

/// A random clamped B-spline (sometimes rational, 2D or with repeated interior knots).
fn gen_bspline(rng: &mut Rng) -> BSplineIn {
    let p = 1 + rng.below(4) as usize;
    let n = p + rng.below(5) as usize;
    let rational = rng.chance(0.25);
    let two_d = rng.chance(0.1);
    let a = if rng.chance(0.6) {
        0.0
    } else {
        rng.uniform(-3.0, 3.0)
    };
    let len = if rng.chance(0.5) {
        1.0
    } else {
        rng.uniform(0.2, 5.0)
    };
    let mut u = vec![a; p + 1];
    let inner = n - p;
    let mut ks: Vec<f64> = (0..inner)
        .map(|_| a + len * rng.uniform(0.05, 0.95))
        .collect();
    ks.sort_by(f64::total_cmp);
    u.extend(ks);
    u.extend(vec![a + len; p + 1]);
    let scale = if rng.chance(0.5) { 5.0 } else { 50.0 };
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
    BSplineIn {
        dim: if two_d { 2 } else { 3 },
        oob: rng.chance(0.1),
        cpts,
        p,
        u,
    }
}

/// A B-spline from `a` to `b` (non-rational, 3D) with interior points near the chord.
fn bspline_between(rng: &mut Rng, a: [f64; 3], bb: [f64; 3], bulge: f64) -> BSplineIn {
    let p = 1 + rng.below(3) as usize;
    let n = p + rng.below(3) as usize;
    let a0 = if rng.chance(0.6) { 0.0 } else { rng.uniform(-2.0, 2.0) };
    let len = if rng.chance(0.5) { 1.0 } else { rng.uniform(0.3, 4.0) };
    let mut u = vec![a0; p + 1];
    let mut ks: Vec<f64> = (0..n - p).map(|_| a0 + len * rng.uniform(0.1, 0.9)).collect();
    ks.sort_by(f64::total_cmp);
    u.extend(ks);
    u.extend(vec![a0 + len; p + 1]);
    let cpts = (0..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            let q = if i == 0 {
                a
            } else if i == n {
                bb
            } else {
                add(lerp(a, bb, t), rp(rng, bulge))
            };
            [q[0], q[1], q[2], NOW]
        })
        .collect();
    BSplineIn {
        dim: 3,
        oob: false,
        cpts,
        p,
        u,
    }
}

fn gen_hermite(rng: &mut Rng) -> HermiteIn {
    let s = if rng.chance(0.5) { 3.0 } else { 30.0 };
    HermiteIn {
        dim: 3,
        p1: rp(rng, s),
        d1: rp(rng, s),
        p2: rp(rng, s),
        d2: rp(rng, s),
    }
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

fn gen_circle(rng: &mut Rng) -> CurveIn {
    let a0 = rng.uniform(-90.0, 90.0);
    CurveIn::Circle(CircleIn {
        center: rp(rng, 5.0),
        x: [1.0, 0.0, 0.0],
        y: [0.0, rng.uniform(0.5, 1.0), rng.uniform(-0.5, 0.5)],
        radius: rng.uniform(0.5, 10.0),
        a0,
        a1: a0 + rng.uniform(20.0, 360.0),
    })
}

fn gen_curve(rng: &mut Rng, hermite: bool) -> CurveIn {
    match rng.below(20) {
        0..=2 => gen_circle(rng),
        3..=5 => CurveIn::Line(gen_line(rng)),
        6..=7 => CurveIn::Segment(SegmentIn {
            a: rp(rng, 10.0),
            b: rp(rng, 10.0),
        }),
        8..=15 => CurveIn::Bspline(gen_bspline(rng)),
        _ if hermite => CurveIn::Hermite(gen_hermite(rng)),
        _ => CurveIn::Bspline(gen_bspline(rng)),
    }
}

/// A composite of random (unconnected) curves.
fn gen_comp(rng: &mut Rng, max: u64, hermite: bool) -> CompIn {
    let n = rng.below(max + 1) as usize;
    let curves: Vec<CurveIn> = (0..n).map(|_| gen_curve(rng, hermite)).collect();
    let closed = rng.chance(0.5);
    let senses = rng
        .chance(0.6)
        .then(|| (0..n).map(|_| rng.chance(0.7)).collect());
    let gaps = (rng.chance(0.6) && !(closed && n == 0)).then(|| {
        (0..n)
            .map(|_| *rng.choose(&[0.0, 1e-9, 1e-4, 0.01, 0.5]).unwrap_or(&0.0))
            .collect()
    });
    CompIn {
        dim: if rng.chance(0.1) { 2 } else { 3 },
        curves,
        closed,
        senses,
        gaps,
    }
}

/// A connected chain of 3D B-splines and Hermite curves through `pts`.
fn gen_chain(rng: &mut Rng, pts: &[[f64; 3]], hermite: bool) -> Vec<CurveIn> {
    pts.windows(2)
        .filter_map(|w| {
            let (a, bb) = (*w.first()?, *w.last()?);
            let d = sub(bb, a);
            Some(if hermite && rng.chance(0.3) {
                CurveIn::Hermite(HermiteIn {
                    dim: 3,
                    p1: a,
                    d1: add(d, rp(rng, 1.0)),
                    p2: bb,
                    d2: add(d, rp(rng, 1.0)),
                })
            } else {
                let bulge = *rng.choose(&[0.0, 0.1, 1.0]).unwrap_or(&0.1);
                CurveIn::Bspline(bspline_between(rng, a, bb, bulge))
            })
        })
        .collect()
}

/// Points of a planar polygon (closed: the last equals the first).
fn gen_polygon(rng: &mut Rng, k: usize, closed: bool) -> (Vec<[f64; 3]>, [f64; 3]) {
    let o = rp(rng, 5.0);
    let (ex, ey) = match rng.below(3) {
        0 => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        1 => ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        _ => ([0.8, 0.6, 0.0], [0.0, 0.6, 0.8]),
    };
    let r = rng.uniform(0.5, 10.0);
    let squash = if rng.chance(0.15) { 1e-9 } else { rng.uniform(0.3, 1.0) };
    let mut pts: Vec<[f64; 3]> = (0..k)
        .map(|i| {
            let ang = std::f64::consts::TAU * (i as f64 + rng.uniform(-0.2, 0.2)) / k as f64;
            add(
                o,
                add(scale3(ex, r * ang.cos()), scale3(ey, squash * r * ang.sin())),
            )
        })
        .collect();
    if closed {
        if let Some(&f) = pts.first() {
            pts.push(f);
        }
    }
    let n = [
        ex[1] * ey[2] - ex[2] * ey[1],
        ex[2] * ey[0] - ex[0] * ey[2],
        ex[0] * ey[1] - ex[1] * ey[0],
    ];
    (pts, n)
}

/// `c` reversed (same point set, opposite direction).
fn reversed(c: &CurveIn) -> CurveIn {
    match resolve(c).unwrap_or_else(|_| c.clone()) {
        CurveIn::Bspline(mut bs) => {
            bs.cpts.reverse();
            let (a, e) = (
                bs.u.first().copied().unwrap_or(0.0),
                bs.u.last().copied().unwrap_or(1.0),
            );
            bs.u = bs.u.iter().rev().map(|k| a + e - k).collect();
            CurveIn::Bspline(bs)
        }
        CurveIn::Hermite(h) => CurveIn::Hermite(HermiteIn {
            dim: h.dim,
            p1: h.p2,
            d1: scale3(h.d2, -1.0),
            p2: h.p1,
            d2: scale3(h.d1, -1.0),
        }),
        other => other,
    }
}

fn gen_info(rng: &mut Rng) -> OpIn {
    let mut types: Vec<i64> = TYPES.to_vec();
    if rng.chance(0.5) {
        types.push(rng.below(20_000) as i64);
    }
    OpIn::Info {
        comp: gen_comp(rng, 5, true),
        types,
    }
}

fn gen_knots(rng: &mut Rng) -> OpIn {
    OpIn::Knots {
        comp: gen_comp(rng, 1, true),
    }
}

fn gen_bbox(rng: &mut Rng) -> OpIn {
    let comp = gen_comp(rng, 4, true);
    let total: f64 = comp
        .curves
        .iter()
        .map(|c| match resolve(c) {
            Ok(CurveIn::Bspline(bs)) => {
                bs.u.last().copied().unwrap_or(1.0) - bs.u.first().copied().unwrap_or(0.0)
            }
            _ => 1.0,
        })
        .sum::<f64>()
        .max(1.0);
    let ivl = if rng.chance(0.3) {
        [-1e3, 1e3]
    } else {
        let x = rng.uniform(-0.5, total + 0.5);
        let y = rng.uniform(-0.5, total + 0.5);
        [x.min(y), x.max(y)]
    };
    OpIn::BBox {
        comp,
        ivl,
        pseudo: rng.chance(0.6),
        polar: rng.chance(0.5),
        expand: rng.chance(0.7),
    }
}

fn perturbed(rng: &mut Rng, c: &CompIn) -> CompIn {
    let mut d = c.clone();
    match rng.below(8) {
        0 => d.closed = !d.closed,
        1 => {
            if let Some(s) = d.senses.as_mut().and_then(|s| s.first_mut()) {
                *s = !*s;
            } else {
                d.senses = Some(vec![false; d.curves.len()]);
            }
        }
        2 => {
            let eps = *rng.choose(&[1e-14, 1e-6, 0.1]).unwrap_or(&1e-6);
            if let Some(g) = d.gaps.as_mut().and_then(|g| g.first_mut()) {
                *g += eps;
            } else if !(d.closed && d.curves.is_empty()) {
                d.gaps = Some(vec![eps; d.curves.len()]);
            }
        }
        3 => {
            d.curves.pop();
            if let Some(s) = d.senses.as_mut() {
                s.pop();
            }
            if let Some(g) = d.gaps.as_mut() {
                g.pop();
            }
            if d.closed && d.curves.is_empty() {
                d.gaps = None;
            }
        }
        4 => d.dim = if d.dim == 3 { 2 } else { 3 },
        5 => {
            if let Some(c0) = d.curves.first_mut() {
                *c0 = gen_curve(rng, true);
            }
        }
        _ => {}
    }
    d
}

fn gen_eq(rng: &mut Rng) -> OpIn {
    let a = gen_comp(rng, 4, true);
    let b = perturbed(rng, &a);
    OpIn::Eq { a, b }
}

fn gen_copy(rng: &mut Rng) -> OpIn {
    OpIn::Copy {
        comp: gen_comp(rng, 4, false),
    }
}

fn gen_transform(rng: &mut Rng) -> OpIn {
    let comp = gen_comp(rng, 3, true);
    let (origin, x, y) = if rng.chance(0.2) {
        ([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])
    } else {
        (rp(rng, 5.0), rp(rng, 1.0), rp(rng, 1.0))
    };
    let scale = match rng.below(4) {
        0 => Some([1.0; 3]),
        1 => {
            let s = rng.uniform(0.2, 3.0);
            Some([s; 3])
        }
        2 => Some([rng.uniform(0.2, 3.0), rng.uniform(0.2, 3.0), rng.uniform(0.2, 3.0)]),
        _ => None,
    };
    OpIn::Transform {
        comp,
        origin,
        x,
        y,
        scale,
    }
}

fn gen_approx(rng: &mut Rng) -> OpIn {
    let k = 2 + rng.below(4) as usize;
    let closed = rng.chance(0.4);
    let (pts, _) = gen_polygon(rng, k, closed);
    let curves = gen_chain(rng, &pts, true);
    let n = curves.len();
    OpIn::Approx {
        comp: CompIn {
            dim: 3,
            curves,
            closed,
            senses: rng.chance(0.3).then(|| vec![true; n]),
            gaps: rng.chance(0.3).then(|| vec![0.0; n]),
        },
        tol: *rng.choose(&[1e-4, 1e-3, 1e-2]).unwrap_or(&1e-3),
    }
}

fn gen_orient(rng: &mut Rng) -> OpIn {
    let k = 2 + rng.below(4) as usize;
    let (pts, n) = gen_polygon(rng, k, true);
    let mut curves = gen_chain(rng, &pts, true);
    if rng.chance(0.3) {
        // plain line segments
        curves = pts
            .windows(2)
            .filter_map(|w| {
                Some(CurveIn::Segment(SegmentIn {
                    a: *w.first()?,
                    b: *w.last()?,
                }))
            })
            .collect();
    }
    if rng.chance(0.3) {
        curves.reverse();
        curves = curves.iter().map(reversed).collect();
    }
    let senses: Vec<bool> = curves.iter().map(|_| rng.chance(0.8)).collect();
    let curves = curves
        .iter()
        .zip(&senses)
        .map(|(c, s)| if *s { c.clone() } else { reversed(c) })
        .collect();
    let normal = match rng.below(4) {
        0 => scale3(n, -1.0),
        1 => rp(rng, 1.0),
        _ => n,
    };
    OpIn::Orient {
        curves,
        senses,
        normal,
        bounds: rng.chance(0.5),
    }
}

fn gen_build(rng: &mut Rng) -> OpIn {
    let same = *rng.choose(&[1e-6, 1e-3, 1e-2]).unwrap_or(&1e-3);
    let line = match rng.below(3) {
        0 => 0.0,
        1 => same * 5.0,
        _ => same * 100.0,
    };
    let mut curves = Vec::new();
    let chains = 1 + rng.below(2);
    for _ in 0..chains {
        let k = 1 + rng.below(4) as usize;
        let closed = rng.chance(0.5);
        let (pts, _) = gen_polygon(rng, k.max(2), closed);
        for w in pts.windows(2) {
            let (Some(&a), Some(&bb)) = (w.first(), w.last()) else {
                continue;
            };
            let jit = *rng.choose(&[0.0, 0.0, 0.3, 2.0, 20.0, 500.0]).unwrap_or(&0.0) * same;
            let a = add(a, rp(rng, jit));
            let c = if rng.chance(0.3) {
                CurveIn::Segment(SegmentIn { a, b: bb })
            } else if rng.chance(0.15) {
                CurveIn::Line(LineIn {
                    p: a,
                    v: sub(bb, a),
                    domain: [0.0, 1.0],
                    scale: 1.0,
                    inside_out: false,
                })
            } else {
                CurveIn::Bspline(bspline_between(rng, a, bb, 0.5))
            };
            curves.push(if rng.chance(0.3) { reversed(&c) } else { c });
        }
    }
    if rng.chance(0.2) {
        // a tiny curve
        let a = rp(rng, 5.0);
        curves.push(CurveIn::Segment(SegmentIn {
            a,
            b: add(a, [same * 0.1, 0.0, 0.0]),
        }));
    }
    // shuffle
    for i in (1..curves.len()).rev() {
        let j = rng.below(i as u64 + 1) as usize;
        curves.swap(i, j);
    }
    curves.truncate(8);
    OpIn::Build { curves, same, line }
}

fn gen_corner(rng: &mut Rng) -> OpIn {
    let vertex = rp(rng, 5.0);
    let ang1 = rng.uniform(0.0, std::f64::consts::TAU);
    let ang2 = ang1 + rng.uniform(0.2, 2.9) * if rng.chance(0.5) { 1.0 } else { -1.0 };
    let dir1 = [ang1.cos(), ang1.sin(), 0.0];
    let dir2 = [ang2.cos(), ang2.sin(), 0.0];
    let dist = rng.uniform(0.1, 2.0);
    // offsets of the two original lines (incoming along dir1, outgoing along dir2)
    let side = if rng.chance(0.5) { 1.0 } else { -1.0 };
    let n1 = [-dir1[1] * side, dir1[0] * side, 0.0];
    let n2 = [-dir2[1] * side, dir2[0] * side, 0.0];
    let mut p1 = add(vertex, scale3(n1, dist));
    let mut p2 = add(vertex, scale3(n2, dist));
    let mut v1 = dir1;
    let mut v2 = dir2;
    match rng.below(10) {
        0 => v1 = [0.0; 3],
        1 => v2 = [0.0; 3],
        2 => {
            p1 = rp(rng, 5.0);
            p2 = rp(rng, 5.0);
            v1 = rp(rng, 1.0);
            v2 = rp(rng, 1.0);
        }
        3 => v2 = v1,
        _ => {}
    }
    OpIn::Corner {
        vertex,
        p1,
        v1,
        p2,
        v2,
        corner: rng.below(3) as u8,
        dist,
        tol: *rng.choose(&[1e-6, 1e-4, 1e-3]).unwrap_or(&1e-4),
    }
}

fn gen_classify(rng: &mut Rng) -> OpIn {
    let curve = gen_curve(rng, true);
    let n = rng.below(4) as usize;
    let origs = (0..n).map(|_| gen_curve(rng, true)).collect();
    let orig = rng.chance(0.4).then(|| gen_curve(rng, true));
    OpIn::Classify {
        curve,
        dist: rng.uniform(0.01, 20.0),
        tol: *rng.choose(&[1e-6, 1e-4, 1e-3]).unwrap_or(&1e-4),
        origs,
        orig,
    }
}

fn gen_breaks(rng: &mut Rng) -> OpIn {
    let n = rng.below(7) as usize;
    let mut v: Vec<f64> = (0..n).map(|_| rng.uniform(-10.0, 10.0)).collect();
    v.sort_by(f64::total_cmp);
    let x = match rng.below(6) {
        0 => *rng.choose(&v).unwrap_or(&0.0),
        1 => rng.choose(&v).map_or(0.0, |y| y + 1e-15),
        2 => -20.0,
        3 => 20.0,
        _ => rng.uniform(-10.0, 10.0),
    };
    OpIn::Breaks {
        which: rng.below(3) as u8,
        v,
        x,
    }
}

fn gen_types(rng: &mut Rng) -> OpIn {
    let mut types: Vec<i64> = TYPES.to_vec();
    types.push(rng.below(20_000) as i64);
    OpIn::Types {
        id: *rng.choose(&[102, 7, 0]).unwrap_or(&102),
        corner: rng.chance(0.5),
        comp_index: rng.below(10),
        curve: rng.below(10),
        other: rng.below(10),
        err: rng.uniform(0.0, 1.0),
        types,
    }
}

fn gen_case(rng: &mut Rng) -> OpIn {
    match rng.below(100) {
        0..=9 => gen_info(rng),
        10..=14 => gen_knots(rng),
        15..=26 => gen_bbox(rng),
        27..=33 => gen_eq(rng),
        34..=39 => gen_copy(rng),
        40..=47 => gen_transform(rng),
        48..=57 => gen_approx(rng),
        58..=69 => gen_orient(rng),
        70..=81 => gen_build(rng),
        82..=89 => gen_corner(rng),
        90..=94 => gen_classify(rng),
        95..=97 => gen_breaks(rng),
        _ => gen_types(rng),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "curve.composite_curve_p1"
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
    use std::collections::BTreeMap;

    /// Generated cases cover every op.
    #[test]
    fn generated_cases_are_diverse() {
        let mut rng = Rng::new(7);
        let mut stats: BTreeMap<String, [usize; 4]> = BTreeMap::new();
        for _ in 0..2000 {
            let op = gen_case(&mut rng);
            let name = serde_json::to_value(&op).unwrap()["op"].as_str().unwrap().to_string();
            let Ok((code, input)) = cpp_input(&op) else {
                continue;
            };
            if ffi::validate(code, &input).is_err() {
                continue;
            }
            let out = rust(&op);
            let e = stats.entry(name).or_default();
            e[0] += 1;
            match out {
                Err(_) => e[3] += 1,
                Ok(o) if o.deferred.is_some() => e[3] += 1,
                Ok(o) if o.ints.get("status").is_some_and(|s| s == &vec![0]) => e[1] += 1,
                Ok(_) => e[2] += 1,
            }
        }
        for (k, v) in &stats {
            eprintln!("{k}: total {} ok {} err {} deferred/harness {}", v[0], v[1], v[2], v[3]);
        }
        assert_eq!(stats.len(), 13);
    }
}
