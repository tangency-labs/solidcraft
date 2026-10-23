//! Runner for unit `curve.composite_curve_p2`: `SmCompositeCurve` part 2
//! (`CreateOffsetsOfManyCurves`, `CreateTrimmedOffsets`, `IntersectAndTrimOffsets`, the
//! bow-tie check and repair, `MakeCompositeNurb`, `ReadFromDB` of composites and their
//! segments, `SmOffsetMapAttribute::MakeCopy` and the segment constructor).
//!
//! C++: `solidcore_ffi::units::curve_composite_curve_p2` (upstream
//! `source/SMLib/src/SmCompositeCurve.cpp`; the file-static `sm_CheckBowTies` through a
//! verbatim shim copy). Rust: `solidcore::curve::composite_curve::p2`.
//!
//! A case is one `op` with its curves (`{"bspline": {dim, oob, cpts, p, u}}` with control
//! points as stored `[x, y, z, w]`, `{"hermite": ..}`, `{"circle": ..}` (a quadratic
//! B-spline arc), `{"segment": {a, b}}` (`CreateLineSegment(3, a, b)`), `{"canon": {p,
//! pts, mults, knots}}` (`CreateCanonical(3, p, pts, ..)`, as upstream's `my_create_nurb`),
//! `{"line": ..}` (a bounded `SmLine`) or `{"offset": {base, n, dist}}` (an `SmOffsetCurve`
//! of `base` over its natural interval); arcs, segments and canonical curves are built by
//! the Rust B-spline constructors, bit-identical to upstream, and handed to both sides as
//! inline B-spline data), composites (`{dim, curves, closed, senses?, gaps?}`) and
//! parameters. Both sides produce the flat record documented in the ffi module; the
//! runner reports its first entry as the `status` int (`0` = `SM_SUCCESS`, else the
//! `SmStatus`; `1007` for a Rust `InvalidInput`, `-1` for a Rust error upstream cannot
//! produce) and the rest as the `out` values. Generated `read_db` cases carry the bytes
//! upstream's `WriteToDB` makes of a random composite.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::composite_curve::p1::{
    CompositeCurve, CompositeCurveSegment, OffsetCornerType, OffsetMapAttribute,
    OFFSET_MAP_ATTRIBUTE_TYPE,
};
use solidcore::curve::composite_curve::p2::{
    check_bow_ties, fix_bow_tie, OffsetDirection, OffsetParams, OffsetPiece,
};
use solidcore::curve::curve::p2::{Curve, CurveKind};
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::curve::offset_curve::OffsetCurve;
use solidcore::geom::attribute::{ids, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::object::ObjectRef;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use solidcore::nurbs::math::NOW;
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::topology::database_io::{DatabaseReader, FileType};
use solidcore::SolidError;
use solidcore_ffi::units::curve_composite_curve_p2::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 600;

/// `curve.composite_curve_p2` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

const UNDEF: f64 = 1.23456e-61;

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

/// A 3D B-spline built by the Rust `CreateCanonical` (unspecified form and knot type).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CanonIn {
    p: usize,
    pts: Vec<[f64; 3]>,
    mults: Vec<usize>,
    knots: Vec<f64>,
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
struct OffsetIn {
    base: Box<CurveIn>,
    n: [f64; 3],
    dist: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CurveIn {
    Bspline(BSplineIn),
    Hermite(HermiteIn),
    Circle(CircleIn),
    Segment(SegmentIn),
    Canon(CanonIn),
    Line(LineIn),
    Offset(OffsetIn),
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct ParamsIn {
    xtol: f64,
    atol: f64,
    normal: [f64; 3],
    corner: u8,
    trim: bool,
    dist: f64,
    #[serde(default)]
    match_pz: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct AttrIn {
    #[serde(default = "yes")]
    has: bool,
    #[serde(default)]
    corner: bool,
    #[serde(default)]
    left: bool,
    #[serde(default)]
    comp: u64,
    curve: u64,
    #[serde(default)]
    other: u64,
    /// Index of the originator in `origs`, `-1` for none.
    orig: i64,
    #[serde(default)]
    err: f64,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Offsets {
        comps: Vec<CompIn>,
        dirs: Vec<u64>,
        params: ParamsIn,
    },
    Trimmed {
        comp: CompIn,
        direction: u8,
        params: ParamsIn,
    },
    Intersect {
        raws: Vec<CurveIn>,
        attrs: Vec<AttrIn>,
        dist: f64,
        tol: f64,
        origs: Vec<CurveIn>,
    },
    CheckBow {
        offs: Vec<CurveIn>,
        dist: f64,
        tol: f64,
        origs: Vec<CurveIn>,
    },
    FixBow {
        curve: CurveIn,
        points: Vec<[f64; 3]>,
        dist: f64,
        breaks: Vec<f64>,
    },
    Nurb {
        comp: CompIn,
    },
    ReadDb {
        dim: u64,
        binary: bool,
        version: u64,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        bytes: Option<Vec<u8>>,
        /// Generation only: the composite whose upstream `WriteToDB` bytes the case reads.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        src: Option<CompIn>,
    },
    Misc {
        id: u64,
        corner: bool,
        left: bool,
        comp: u64,
        curve: u64,
        other: u64,
        err: f64,
        seg: CurveIn,
        sense: bool,
        g0: f64,
        g1: f64,
    },
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

/// `c` with an arc, segment or canonical curve replaced by its inline B-spline.
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
        CurveIn::Canon(c) => {
            let pts: Vec<Vec3> = c.pts.iter().map(|p| v3(*p)).collect();
            let bs = BSplineCurve::create_canonical(&CanonicalData {
                dim: 3,
                degree: c.p,
                control_points: &pts,
                form: BSplineCurveForm::Unspecified,
                knot_multiplicities: &c.mults,
                knots: &c.knots,
                knot_type: KnotType::Unspecified,
                weights: None,
                trim: None,
            })
            .map_err(|e| harness(format!("canonical: {e}")))?;
            bspline_in(&bs)
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
        CurveIn::Offset(o) => {
            v.extend([3.0, 3.0]);
            curve_block(v, &o.base)?;
            v.extend_from_slice(&o.n);
            v.push(o.dist);
        }
        CurveIn::Circle(_) | CurveIn::Segment(_) | CurveIn::Canon(_) => {
            return Err(harness("unresolved curve"))
        }
    }
    Ok(())
}

fn curve_list(v: &mut Vec<f64>, cs: &[CurveIn]) -> Result<(), HarnessError> {
    v.push(cs.len() as f64);
    for c in cs {
        curve_block(v, c)?;
    }
    Ok(())
}

fn comp_block(v: &mut Vec<f64>, c: &CompIn) -> Result<(), HarnessError> {
    v.push(c.dim as f64);
    curve_list(v, &c.curves)?;
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

fn params_block(v: &mut Vec<f64>, p: &ParamsIn) {
    v.extend([p.xtol, p.atol]);
    v.extend_from_slice(&p.normal);
    v.extend([f64::from(p.corner), b(p.trim), p.dist, b(p.match_pz)]);
}

fn db_bytes(text: Option<&String>, bytes: Option<&Vec<u8>>) -> Result<Vec<u8>, HarnessError> {
    match (text, bytes) {
        (Some(t), None) => Ok(t.clone().into_bytes()),
        (None, Some(b)) => Ok(b.clone()),
        _ => Err(harness("read_db needs exactly one of text, bytes")),
    }
}

fn cpp_input(op: &OpIn) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut v = Vec::new();
    let code = match op {
        OpIn::Offsets { comps, dirs, params } => {
            v.push(comps.len() as f64);
            for c in comps {
                comp_block(&mut v, c)?;
            }
            if dirs.len() != comps.len() {
                return Err(harness("offsets: one direction per composite"));
            }
            v.extend(dirs.iter().map(|d| *d as f64));
            params_block(&mut v, params);
            Op::Offsets
        }
        OpIn::Trimmed {
            comp,
            direction,
            params,
        } => {
            comp_block(&mut v, comp)?;
            v.push(f64::from(*direction));
            params_block(&mut v, params);
            Op::Trimmed
        }
        OpIn::Intersect {
            raws,
            attrs,
            dist,
            tol,
            origs,
        } => {
            if attrs.len() != raws.len() {
                return Err(harness("intersect: one attribute per raw curve"));
            }
            curve_list(&mut v, raws)?;
            for a in attrs {
                v.extend([
                    b(a.has),
                    b(a.corner),
                    b(a.left),
                    a.comp as f64,
                    a.curve as f64,
                    a.other as f64,
                    a.orig as f64,
                    a.err,
                ]);
            }
            v.extend([*dist, *tol]);
            curve_list(&mut v, origs)?;
            Op::Intersect
        }
        OpIn::CheckBow {
            offs,
            dist,
            tol,
            origs,
        } => {
            curve_list(&mut v, offs)?;
            v.extend([*dist, *tol]);
            curve_list(&mut v, origs)?;
            Op::CheckBow
        }
        OpIn::FixBow {
            curve,
            points,
            dist,
            breaks,
        } => {
            curve_block(&mut v, curve)?;
            v.push(points.len() as f64);
            for p in points {
                v.extend_from_slice(p);
            }
            v.extend([*dist, breaks.len() as f64]);
            v.extend_from_slice(breaks);
            Op::FixBow
        }
        OpIn::Nurb { comp } => {
            comp_block(&mut v, comp)?;
            Op::Nurb
        }
        OpIn::ReadDb {
            dim,
            binary,
            version,
            text,
            bytes,
            ..
        } => {
            let data = db_bytes(text.as_ref(), bytes.as_ref())?;
            v.extend([*dim as f64, b(*binary), *version as f64, data.len() as f64]);
            v.extend(data.iter().map(|&x| f64::from(x)));
            Op::ReadDb
        }
        OpIn::Misc {
            id,
            corner,
            left,
            comp,
            curve,
            other,
            err,
            seg,
            sense,
            g0,
            g1,
        } => {
            v.extend([
                *id as f64,
                b(*corner),
                b(*left),
                *comp as f64,
                *curve as f64,
                *other as f64,
                *err,
            ]);
            curve_block(&mut v, seg)?;
            v.extend([b(*sense), *g0, *g1]);
            Op::Misc
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

/// Upstream's `WriteToDB` bytes of `comp`.
fn write_db(comp: &CompIn, binary: bool, version: u64) -> Option<Vec<u8>> {
    let mut v = Vec::new();
    comp_block(&mut v, comp).ok()?;
    v.extend([b(binary), version as f64]);
    let out = ffi::eval(Op::WriteDb, &v).ok()?;
    if out.first().copied() != Some(0.0) {
        return None;
    }
    Some(out.get(1..)?.iter().map(|&x| x as u8).collect())
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
        CurveIn::Offset(o) => {
            let base = build(&o.base)?;
            let ivl = base.natural_interval();
            Ok(Curve::from(OffsetCurve::new(
                3,
                base,
                ivl,
                v3(o.n),
                o.dist,
                false,
            ).map_err(|e| HarnessError::new(e.to_string()))?))
        }
        CurveIn::Circle(_) | CurveIn::Segment(_) | CurveIn::Canon(_) => {
            Err(harness("unresolved curve"))
        }
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

fn ftry<T>(r: solidcore::Result<T>) -> Result<T, Fail> {
    r.map_err(Fail::from)
}

/// The curve record (see the ffi module docs).
fn put_curve(k: &Kernel, o: &mut Vec<f64>, c: &Curve) -> Result<(), Fail> {
    o.push(c.type_code() as f64);
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

fn put_composite(k: &Kernel, o: &mut Vec<f64>, c: &CompositeCurve) -> Result<(), Fail> {
    o.extend([c.dim() as f64, c.num_segments() as f64, b(c.is_closed())]);
    for s in c.segments() {
        o.extend([
            f64::from(s.transition as i32),
            b(s.same_sense),
            s.gap_start,
            s.gap_end,
            b(s.owns_curve),
        ]);
        put_curve(k, o, &s.curve)?;
    }
    Ok(())
}

/// A piece record; `indexed`: the originator as its index (else `0` for "not null").
fn put_piece(k: &Kernel, o: &mut Vec<f64>, p: &OffsetPiece, indexed: bool) -> Result<(), Fail> {
    o.push(b(p.attribute.is_some()));
    if let Some(a) = &p.attribute {
        o.extend([
            b(a.corner_offset),
            b(a.left_hand_offset),
            a.composite_index as f64,
            a.curve as f64,
            a.other_curve as f64,
        ]);
        o.push(match a.originator {
            None => -1.0,
            Some(r) if indexed => r.key.0 as f64,
            Some(_) => 0.0,
        });
        o.push(a.approx_err);
    }
    put_curve(k, o, &p.curve)
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

fn params(p: &ParamsIn) -> Result<OffsetParams, HarnessError> {
    let corner = match p.corner {
        0 => OffsetCornerType::LinearExtension,
        1 => OffsetCornerType::FilletCorner,
        2 => OffsetCornerType::LinearChamfer,
        _ => return Err(harness("bad corner type")),
    };
    Ok(OffsetParams {
        xsect_tol_3d: p.xtol,
        approx_tol_3d: p.atol,
        plane_normal: v3(p.normal),
        corner,
        trim_results: p.trim,
        offset_distance: p.dist,
        match_parameterization: p.match_pz,
    })
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

/// The status of `r` for the ops that output their partial results anyway.
fn status_of(r: solidcore::Result<()>) -> Result<f64, Fail> {
    match r {
        Ok(()) => Ok(0.0),
        Err(e) if e.is_unported() => Err(e.into()),
        Err(e) => Ok(status(&e)),
    }
}

#[allow(clippy::too_many_lines)]
fn rust(op: &OpIn) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    Ok(match op {
        OpIn::Offsets { comps, dirs, params: pin } => {
            let p = params(pin)?;
            let mut built = Vec::with_capacity(comps.len());
            for c in comps {
                built.push(composite(c)?);
            }
            run_body(|o| {
                let built: Vec<CompositeCurve> = built.into_iter().map(ftry).collect::<Result<_, _>>()?;
                let refs: Vec<&CompositeCurve> = built.iter().collect();
                let r = ftry(CompositeCurve::create_offsets_of_many_curves(&k, &refs, dirs, &p))?;
                o.extend([0.0, r.offsets.len() as f64]);
                for piece in &r.offsets {
                    put_piece(&k, o, piece, false)?;
                }
                o.push(r.max_gaps.len() as f64);
                o.extend_from_slice(&r.max_gaps);
                Ok(())
            })
        }
        OpIn::Trimmed {
            comp,
            direction,
            params: pin,
        } => {
            let p = params(pin)?;
            let dir = match direction {
                0 => OffsetDirection::RightHandSide,
                1 => OffsetDirection::LeftHandSide,
                2 => OffsetDirection::BothSides,
                _ => OffsetDirection::Unknown,
            };
            let c = composite(comp)?;
            run_body(|o| {
                let c = ftry(c)?;
                let r = ftry(c.create_trimmed_offsets(&k, dir, &p))?;
                o.extend([0.0, r.len() as f64]);
                for piece in &r {
                    put_piece(&k, o, piece, false)?;
                }
                Ok(())
            })
        }
        OpIn::Intersect {
            raws,
            attrs,
            dist,
            tol,
            origs,
        } => {
            let curves = build_all(raws)?;
            let orig_curves = build_all(origs)?;
            let mut pieces = Vec::with_capacity(curves.len());
            for (curve, a) in curves.into_iter().zip(attrs) {
                let originator = match usize::try_from(a.orig) {
                    Ok(i) => Some(ObjectRef {
                        type_code: orig_curves
                            .get(i)
                            .ok_or_else(|| harness("originator out of range"))?
                            .type_code(),
                        key: ObjectKey(i as u64),
                    }),
                    Err(_) => None,
                };
                pieces.push(OffsetPiece {
                    curve,
                    attribute: a.has.then(|| {
                        OffsetMapAttribute::new(
                            ids::OFFSETMAP,
                            a.corner,
                            a.left,
                            a.comp,
                            a.curve,
                            a.other,
                            originator,
                            a.err,
                        )
                    }),
                });
            }
            run_body(|o| {
                let orig_refs: Vec<&Curve> = orig_curves.iter().collect();
                let out = ftry(CompositeCurve::intersect_and_trim_offsets(
                    &k,
                    &mut pieces,
                    *dist,
                    *tol,
                    &orig_refs,
                ))?;
                o.extend([0.0, out.len() as f64]);
                for piece in &out {
                    put_piece(&k, o, piece, true)?;
                }
                o.push(pieces.len() as f64);
                for piece in &pieces {
                    put_curve(&k, o, &piece.curve)?;
                }
                Ok(())
            })
        }
        OpIn::CheckBow {
            offs,
            dist,
            tol,
            origs,
        } => {
            let offs = build_all(offs)?;
            let origs = build_all(origs)?;
            run_body(|o| {
                let off_refs: Vec<&Curve> = offs.iter().collect();
                let orig_refs: Vec<&Curve> = origs.iter().collect();
                let mut pts = Vec::new();
                let s = status_of(check_bow_ties(
                    &k, &off_refs, *dist, *tol, &orig_refs, &mut pts,
                ))?;
                o.extend([s, pts.len() as f64]);
                for p in pts {
                    pv(o, p);
                }
                Ok(())
            })
        }
        OpIn::FixBow {
            curve,
            points,
            dist,
            breaks,
        } => {
            let c = build(curve)?;
            run_body(|o| {
                let pts: Vec<Vec3> = points.iter().map(|p| v3(*p)).collect();
                let mut br = breaks.clone();
                let s = status_of(fix_bow_tie(&k, &c, &pts, *dist, &mut br))?;
                o.extend([s, br.len() as f64]);
                o.extend_from_slice(&br);
                Ok(())
            })
        }
        OpIn::Nurb { comp } => {
            let c = composite(comp)?;
            run_body(|o| {
                let mut c = ftry(c)?;
                let j = ftry(c.make_composite_nurb())?;
                o.push(0.0);
                put_curve(&k, o, &Curve::from(j))?;
                put_composite(&k, o, &c)
            })
        }
        OpIn::ReadDb {
            dim,
            binary,
            version,
            text,
            bytes,
            ..
        } => {
            let data = db_bytes(text.as_ref(), bytes.as_ref())?;
            run_body(|o| {
                let mut db = DatabaseReader::new();
                let ft = if *binary {
                    FileType::Binary
                } else {
                    FileType::Ascii
                };
                db.open_for_read(data, ft);
                let c = ftry(CompositeCurve::read_from_db(&k, &mut db, *dim, *version))?;
                o.push(0.0);
                put_composite(&k, o, &c)
            })
        }
        OpIn::Misc {
            id,
            corner,
            left,
            comp,
            curve,
            other,
            err,
            seg,
            sense,
            g0,
            g1,
        } => {
            let c = build(seg)?;
            run_body(|o| {
                let a = OffsetMapAttribute::new(*id, *corner, *left, *comp, *curve, *other, None, *err);
                let cp = a.make_copy();
                o.push(0.0);
                o.extend([
                    cp.attribute_id as f64,
                    b(cp.corner_offset),
                    b(cp.left_hand_offset),
                    cp.composite_index as f64,
                    cp.curve as f64,
                    cp.other_curve as f64,
                    b(cp.originator.is_none()),
                    cp.approx_err,
                    OFFSET_MAP_ATTRIBUTE_TYPE as f64,
                ]);
                let s = CompositeCurveSegment::new(c, *sense, *g0, *g1);
                o.extend([
                    f64::from(s.transition as i32),
                    b(s.same_sense),
                    s.gap_start,
                    s.gap_end,
                    b(s.owns_curve),
                ]);
                put_curve(&k, o, &s.curve)
            })
        }
    })
}

// ---- generation -------------------------------------------------------------------

fn rp(rng: &mut Rng, s: f64) -> [f64; 3] {
    rng.point3(-s, s)
}

fn rp2(rng: &mut Rng, s: f64) -> [f64; 3] {
    [rng.uniform(-s, s), rng.uniform(-s, s), 0.0]
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

/// A planar (z = 0) non-rational B-spline from `a` to `b` with interior points near the
/// chord.
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
                add(lerp(a, bb, t), rp2(rng, bulge))
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
        CurveIn::Line(l) => {
            // the same segment from the other end
            let len = l.domain[1] - l.domain[0];
            let end = add(l.p, scale3(l.v, l.domain[1] / l.scale));
            let start = add(l.p, scale3(l.v, l.domain[0] / l.scale));
            CurveIn::Line(LineIn {
                p: end,
                v: scale3(sub(start, end), 1.0 / len),
                domain: [0.0, 1.0],
                scale: 1.0,
                inside_out: false,
            })
        }
        other => other,
    }
}

/// A planar curve from `a` to `bb`: a segment, a bounded line or a B-spline.
fn edge(rng: &mut Rng, a: [f64; 3], bb: [f64; 3], bend: f64) -> CurveIn {
    match rng.below(10) {
        0..=5 => CurveIn::Segment(SegmentIn { a, b: bb }),
        6 => CurveIn::Line(LineIn {
            p: a,
            v: sub(bb, a),
            domain: [0.0, 1.0],
            scale: 1.0,
            inside_out: false,
        }),
        _ => CurveIn::Bspline(bspline_between(rng, a, bb, bend)),
    }
}

/// Vertices of a star-shaped planar polygon about `o` (counter-clockwise).
fn polygon(rng: &mut Rng, o: [f64; 3], k: usize, r: f64) -> Vec<[f64; 3]> {
    (0..k)
        .map(|i| {
            let ang = std::f64::consts::TAU * (i as f64 + rng.uniform(-0.25, 0.25)) / k as f64;
            let rr = r * rng.uniform(0.6, 1.0);
            add(o, [rr * ang.cos(), rr * ang.sin(), 0.0])
        })
        .collect()
}

/// A planar composite: a polygon or polyline of segments, lines and B-splines (some
/// stored reversed with sense `false`), or a full circle.
fn gen_comp(rng: &mut Rng, o: [f64; 3], r: f64) -> CompIn {
    if rng.chance(0.15) {
        let a0 = if rng.chance(0.5) { 0.0 } else { rng.uniform(-90.0, 90.0) };
        let y = if rng.chance(0.5) {
            [0.0, 1.0, 0.0]
        } else {
            [0.0, -1.0, 0.0]
        };
        return CompIn {
            dim: 3,
            curves: vec![CurveIn::Circle(CircleIn {
                center: o,
                x: [1.0, 0.0, 0.0],
                y,
                radius: r * rng.uniform(0.3, 1.0),
                a0,
                a1: a0 + 360.0,
            })],
            closed: rng.chance(0.8),
            senses: None,
            gaps: None,
        };
    }
    let closed = rng.chance(0.7);
    let k = if closed {
        3 + rng.below(4) as usize
    } else {
        2 + rng.below(4) as usize
    };
    let mut pts = polygon(rng, o, k, r);
    if rng.chance(0.5) {
        pts.reverse();
    }
    if closed {
        if let Some(&f) = pts.first() {
            pts.push(f);
        }
    }
    let bend = r * *rng.choose(&[0.0, 0.05, 0.15]).unwrap_or(&0.05);
    let mut curves = Vec::new();
    let mut senses = Vec::new();
    for w in pts.windows(2) {
        let (Some(&a), Some(&bb)) = (w.first(), w.last()) else {
            continue;
        };
        let c = edge(rng, a, bb, bend);
        if rng.chance(0.25) {
            curves.push(reversed(&c));
            senses.push(false);
        } else {
            curves.push(c);
            senses.push(true);
        }
    }
    let any_reversed = senses.iter().any(|s| !s);
    CompIn {
        dim: 3,
        curves,
        closed: closed && rng.chance(0.85),
        senses: (any_reversed || rng.chance(0.2)).then_some(senses),
        gaps: None,
    }
}

fn gen_params(rng: &mut Rng, r: f64) -> ParamsIn {
    let normal = match rng.below(10) {
        0 => [0.0, 0.0, -1.0],
        1 => [0.0, 0.0, 2.0],
        _ => [0.0, 0.0, 1.0],
    };
    let dist = r * rng.uniform(0.02, 0.6) * if rng.chance(0.25) { -1.0 } else { 1.0 };
    ParamsIn {
        xtol: *rng.choose(&[1e-3, 1e-3, 1e-4, 1e-2]).unwrap_or(&1e-3),
        atol: *rng.choose(&[2.5e-3, 1e-3, 1e-4]).unwrap_or(&1e-3),
        normal,
        corner: rng.below(2) as u8,
        trim: rng.chance(0.75),
        dist,
        match_pz: rng.chance(0.15),
    }
}

fn gen_offsets(rng: &mut Rng) -> OpIn {
    let r = rng.uniform(1.0, 5.0);
    let o = rp2(rng, 5.0);
    let nc = 1 + rng.below(3) as usize;
    let mut comps = vec![gen_comp(rng, o, r)];
    for _ in 1..nc {
        // a smaller composite inside, or one beside
        if rng.chance(0.6) {
            comps.push(gen_comp(rng, o, r * 0.3));
        } else {
            comps.push(gen_comp(rng, add(o, [2.5 * r, 0.0, 0.0]), r * 0.8));
        }
    }
    let dirs = comps
        .iter()
        .map(|_| {
            if rng.chance(0.03) {
                *rng.choose(&[0, 4]).unwrap_or(&0)
            } else {
                1 + rng.below(3)
            }
        })
        .collect();
    OpIn::Offsets {
        comps,
        dirs,
        params: gen_params(rng, r),
    }
}

fn gen_trimmed(rng: &mut Rng) -> OpIn {
    let r = rng.uniform(1.0, 5.0);
    let o = rp2(rng, 5.0);
    OpIn::Trimmed {
        comp: gen_comp(rng, o, r),
        direction: rng.below(4) as u8,
        params: gen_params(rng, r),
    }
}

/// The resolved curves of a random open or closed planar chain.
fn gen_chain(rng: &mut Rng, r: f64) -> Vec<CurveIn> {
    let o = rp2(rng, 3.0);
    let c = gen_comp(rng, o, r);
    let senses = c.senses.clone();
    c.curves
        .iter()
        .enumerate()
        .map(|(i, cu)| {
            let same = senses
                .as_ref()
                .and_then(|s| s.get(i).copied())
                .unwrap_or(true);
            if same {
                cu.clone()
            } else {
                reversed(cu)
            }
        })
        .collect()
}

fn gen_intersect(rng: &mut Rng) -> OpIn {
    let r = rng.uniform(1.0, 5.0);
    let origs = gen_chain(rng, r);
    let dist = r * rng.uniform(0.05, 0.6);
    let both = rng.chance(0.3);
    let left = rng.chance(0.5);
    let n = [0.0, 0.0, 1.0];
    let mut raws = Vec::new();
    let mut attrs = Vec::new();
    for (i, c) in origs.iter().enumerate() {
        for side in [left, !left].iter().take(if both { 2 } else { 1 }) {
            raws.push(CurveIn::Offset(OffsetIn {
                base: Box::new(c.clone()),
                n,
                dist: if *side { -dist } else { dist },
            }));
            attrs.push(AttrIn {
                has: rng.chance(0.9),
                corner: false,
                left: *side,
                comp: 0,
                curve: i as u64,
                other: 0,
                orig: if rng.chance(0.85) { i as i64 } else { -1 },
                err: 0.0,
            });
        }
    }
    OpIn::Intersect {
        raws,
        attrs,
        dist,
        tol: *rng.choose(&[1e-3, 1e-4, 1e-2]).unwrap_or(&1e-3),
        origs,
    }
}

fn gen_check_bow(rng: &mut Rng) -> OpIn {
    let r = rng.uniform(1.0, 5.0);
    let origs = gen_chain(rng, r);
    let dist = r * rng.uniform(0.05, 1.5);
    let sign = if rng.chance(0.5) { 1.0 } else { -1.0 };
    let mut offs: Vec<CurveIn> = origs
        .iter()
        .map(|c| {
            CurveIn::Offset(OffsetIn {
                base: Box::new(c.clone()),
                n: [0.0, 0.0, 1.0],
                dist: sign * dist,
            })
        })
        .collect();
    let mut origs = origs;
    match rng.below(20) {
        0 => {
            offs.pop();
        }
        1 => origs.clear(),
        2 => offs.clear(),
        _ => {}
    }
    OpIn::CheckBow {
        offs,
        dist,
        tol: *rng.choose(&[1e-3, 1e-4, 1e-2]).unwrap_or(&1e-3),
        origs,
    }
}

fn gen_fix_bow(rng: &mut Rng) -> OpIn {
    let r = rng.uniform(1.0, 5.0);
    let a = rp2(rng, 3.0);
    let bb = add(a, rp2(rng, r));
    let base = edge(rng, a, bb, r * 0.2);
    let curve = if rng.chance(0.4) {
        CurveIn::Offset(OffsetIn {
            base: Box::new(base),
            n: [0.0, 0.0, 1.0],
            dist: r * rng.uniform(-0.3, 0.3),
        })
    } else {
        base
    };
    let np = rng.below(4) as usize;
    let points = (0..np)
        .map(|_| add(lerp(a, bb, rng.uniform(-0.3, 1.3)), rp2(rng, r * 0.3)))
        .collect();
    let mut breaks: Vec<f64> = (0..rng.below(4)).map(|_| rng.uniform(-1.0, 2.0)).collect();
    breaks.sort_by(f64::total_cmp);
    OpIn::FixBow {
        curve,
        points,
        dist: r * rng.uniform(0.05, 0.8),
        breaks,
    }
}

fn gen_nurb(rng: &mut Rng) -> OpIn {
    let r = rng.uniform(0.5, 5.0);
    let o = rp2(rng, 5.0);
    let mut comp = gen_comp(rng, o, r);
    match rng.below(10) {
        0 => {
            // disconnected
            if let Some(c) = comp.curves.first_mut() {
                *c = reversed(c);
            }
        }
        1 => {
            // a 2D B-spline among 3D ones
            if let Some(CurveIn::Segment(s)) = comp.curves.first().cloned() {
                if let Ok(CurveIn::Bspline(mut bs)) = resolve(&CurveIn::Segment(s)) {
                    bs.dim = 2;
                    for c in &mut bs.cpts {
                        c[2] = f64::MAX;
                    }
                    if let Some(c) = comp.curves.first_mut() {
                        *c = CurveIn::Bspline(bs);
                    }
                }
            }
        }
        _ => {}
    }
    OpIn::Nurb { comp }
}

fn gen_db_curve(rng: &mut Rng) -> CurveIn {
    match rng.below(10) {
        0 => CurveIn::Hermite(HermiteIn {
            dim: 3,
            p1: rp(rng, 5.0),
            d1: rp(rng, 5.0),
            p2: rp(rng, 5.0),
            d2: rp(rng, 5.0),
        }),
        1 => CurveIn::Line(LineIn {
            p: rp(rng, 5.0),
            v: rp(rng, 2.0),
            domain: [rng.uniform(-2.0, 0.0), rng.uniform(0.5, 3.0)],
            scale: *rng.choose(&[1.0, 2.0]).unwrap_or(&1.0),
            inside_out: rng.chance(0.3),
        }),
        2 | 3 => CurveIn::Circle(CircleIn {
            center: rp(rng, 5.0),
            x: [1.0, 0.0, 0.0],
            y: [0.0, 1.0, 0.0],
            radius: rng.uniform(0.5, 4.0),
            a0: 0.0,
            a1: rng.uniform(30.0, 360.0),
        }),
        _ => {
            let a = rp(rng, 5.0);
            let e = rp(rng, 5.0);
            CurveIn::Bspline(bspline_between(rng, a, e, 1.0))
        }
    }
}

fn gen_read_db(rng: &mut Rng) -> OpIn {
    let n = rng.below(5) as usize;
    let curves: Vec<CurveIn> = (0..n).map(|_| gen_db_curve(rng)).collect();
    let closed = rng.chance(0.5);
    let senses = rng
        .chance(0.6)
        .then(|| (0..n).map(|_| rng.chance(0.7)).collect());
    let gaps = (rng.chance(0.6) && !(closed && n == 0)).then(|| {
        (0..n)
            .map(|_| *rng.choose(&[0.0, 1e-9, 1e-4, 0.01, 0.5]).unwrap_or(&0.0))
            .collect()
    });
    OpIn::ReadDb {
        dim: if rng.chance(0.1) { 2 } else { 3 },
        binary: rng.chance(0.5),
        version: *rng.choose(&[30, 42]).unwrap_or(&42),
        text: None,
        bytes: None,
        src: Some(CompIn {
            dim: 3,
            curves,
            closed,
            senses,
            gaps,
        }),
    }
}

fn gen_misc(rng: &mut Rng) -> OpIn {
    OpIn::Misc {
        id: *rng.choose(&[102, 7, 0]).unwrap_or(&102),
        corner: rng.chance(0.5),
        left: rng.chance(0.5),
        comp: rng.below(10),
        curve: rng.below(10),
        other: rng.below(10),
        err: rng.uniform(0.0, 1.0),
        seg: gen_db_curve(rng),
        sense: rng.chance(0.5),
        g0: rng.uniform(0.0, 1.0),
        g1: rng.uniform(0.0, 1.0),
    }
}

fn gen_case(rng: &mut Rng) -> OpIn {
    match rng.below(100) {
        0..=29 => gen_offsets(rng),
        30..=44 => gen_trimmed(rng),
        45..=59 => gen_intersect(rng),
        60..=67 => gen_check_bow(rng),
        68..=75 => gen_fix_bow(rng),
        76..=84 => gen_nurb(rng),
        85..=96 => gen_read_db(rng),
        _ => gen_misc(rng),
    }
}

/// Fills in a generated `read_db` case's bytes from upstream's writer.
fn finish(op: OpIn) -> Option<OpIn> {
    match op {
        OpIn::ReadDb {
            dim,
            binary,
            version,
            src: Some(src),
            ..
        } => {
            let data = write_db(&src, binary, version)?;
            let (text, bytes) = if binary {
                (None, Some(data))
            } else {
                (Some(String::from_utf8(data).ok()?), None)
            };
            Some(OpIn::ReadDb {
                dim,
                binary,
                version,
                text,
                bytes,
                src: None,
            })
        }
        other => Some(other),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "curve.composite_curve_p2"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let Some(op) = finish(gen_case(rng)) else {
                continue;
            };
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
        let op = case.parse()?;
        // Run only what the FFI accepts, as the C++ side: its guards refuse inputs
        // upstream (and the faithful port) never finish on (fuzz Rust-side hangs).
        let (code, input) = cpp_input(&op)?;
        ffi::validate(code, &input).map_err(|e| HarnessError::new(e.to_string()))?;
        rust(&op)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every op is generated, and the Rust side gets past the setup of most.
    #[test]
    fn generated_cases_are_diverse() {
        use std::collections::BTreeMap;
        let mut rng = Rng::new(7);
        let mut stats: BTreeMap<String, [usize; 4]> = BTreeMap::new();
        let mut why: BTreeMap<String, usize> = BTreeMap::new();
        for _ in 0..400 {
            let op = gen_case(&mut rng);
            let name = serde_json::to_value(&op).unwrap()["op"].as_str().unwrap().to_string();
            if matches!(op, OpIn::ReadDb { .. }) {
                continue;
            }
            let Ok((code, input)) = cpp_input(&op) else {
                continue;
            };
            if ffi::validate(code, &input).is_err() {
                continue;
            }
            let out = rust(&op);
            let e = stats.entry(name.clone()).or_default();
            e[0] += 1;
            match out {
                Err(_) => e[3] += 1,
                Ok(o) if o.deferred.is_some() => {
                    e[3] += 1;
                    *why.entry(format!("{name}: {:?}", o.deferred)).or_default() += 1;
                }
                Ok(o) if o.ints.get("status").is_some_and(|s| s == &vec![0]) => e[1] += 1,
                Ok(o) => {
                    e[2] += 1;
                    *why.entry(format!("{name}: status {:?}", o.ints.get("status"))).or_default() += 1;
                }
            }
        }
        for (k, v) in &stats {
            eprintln!("{k}: total {} ok {} err {} deferred/harness {}", v[0], v[1], v[2], v[3]);
        }
        for (k, v) in &why {
            eprintln!("  {v} x {k}");
        }
        assert!(stats.len() >= 7);
    }

    /// Generated database reads mostly succeed on both sides.
    #[test]
    fn generated_reads_succeed() {
        let mut rng = Rng::new(5);
        let mut ok = 0;
        for _ in 0..40 {
            let Some(op) = finish(gen_read_db(&mut rng)) else {
                continue;
            };
            let r = rust(&op).unwrap();
            let c = cpp(&op).unwrap();
            assert_eq!(r.ints.get("status"), c.ints.get("status"));
            if r.ints.get("status").is_some_and(|s| s == &vec![0]) {
                ok += 1;
            }
        }
        assert!(ok > 30, "{ok}");
    }

    /// Generated cases are well-formed for the shim.
    #[test]
    fn generated_cases_validate() {
        let mut rng = Rng::new(11);
        let mut ok = 0;
        for _ in 0..300 {
            let op = gen_case(&mut rng);
            if matches!(op, OpIn::ReadDb { .. }) {
                continue;
            }
            let Ok((code, input)) = cpp_input(&op) else {
                continue;
            };
            if ffi::validate(code, &input).is_ok() {
                ok += 1;
            }
        }
        assert!(ok > 200, "{ok}");
    }
}
