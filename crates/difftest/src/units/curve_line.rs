//! Runner for unit `curve.line`: `SmLine`.
//!
//! C++: `solidcore_ffi::units::curve_line` (upstream `source/SMLib/src/SmLine.cpp` on
//! `SmLine` objects). Rust: `solidcore::curve::line`.
//!
//! A case is a line construction (`make`) and a program of operations (`ops`) run on
//! that line in order; both sides produce the record documented in the ffi module
//! (statuses `0` = `SM_SUCCESS`, else the `SmStatus`; `1007` for a Rust
//! `InvalidInput`, `-1` for a Rust error upstream cannot produce), reported as the
//! values `rec`. A case whose Rust side reaches an unported unit is deferred as a
//! whole.
//!
//! Deferred cases: `IntersectWithEllipse` reaching `SmCurve::GlobalPointSolve`
//! (`curve.curve_p1`) and `AssertValid` below `SM_LEVEL_GIVEN`
//! (`SmCurve::AssertValid`, `curve.curve_p4`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::line::{BoundedLineOptions, Line, TessellationRequest};
use solidcore::geom::assert_array::{AssertArray, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::global_solver::SolverOperation;
use solidcore::geom::solution_array::Solution;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BoundingBoxRequest, BoundingBoxes};
use solidcore::nurbs::b_spline_curve_n_lib::{InValueType, TrimMode};
use solidcore::nurbs::math::CPoint;
use solidcore::topology::database_io::{DatabaseReader, FileType};
use solidcore::SolidError;
use solidcore_ffi::units::curve_line::{self as ffi};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::v3;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `curve.line` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// `NL_NOW`.
const NOW: f64 = -f64::MAX;

// ---- inputs -------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BSplineIn {
    dim: u32,
    p: usize,
    /// `[x, y, z, w]` as stored; `w` = `null` for a non-rational point.
    cpts: Vec<[Option<f64>; 4]>,
    knots: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BaseDbIn {
    n: i64,
    p: i16,
    rat: i16,
    dim: i16,
    #[serde(default = "one")]
    kt: u64,
    #[serde(default = "six")]
    form: u64,
    vals: Vec<f64>,
}

fn one() -> u64 {
    1
}

fn six() -> u64 {
    6
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LineDbIn {
    point: [f64; 3],
    vector: [f64; 3],
    domain: [f64; 2],
    scale: f64,
    inside_out: i32,
    base: BaseDbIn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
enum MakeIn {
    Default,
    Points {
        a: [f64; 3],
        b: [f64; 3],
        #[serde(default = "three")]
        dim: u32,
    },
    Infinite {
        p: [f64; 3],
        v: [f64; 3],
        #[serde(default = "three")]
        dim: u32,
        #[serde(default)]
        inside_out: bool,
    },
    Bounded {
        p: [f64; 3],
        v: [f64; 3],
        domain: [f64; 2],
        #[serde(default = "unit_scale")]
        scale: f64,
        #[serde(default = "three")]
        dim: u32,
        #[serde(default)]
        inside_out: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        nurb: Option<BSplineIn>,
    },
    Segment {
        #[serde(default = "three")]
        dim: u32,
        a: [f64; 3],
        b: [f64; 3],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        interval: Option<[f64; 2]>,
    },
    Canonical {
        p: [f64; 3],
        v: [f64; 3],
    },
    Read {
        #[serde(default = "three")]
        dim: u32,
        version: u32,
        binary: bool,
        db: LineDbIn,
    },
}

fn three() -> u32 {
    3
}

fn unit_scale() -> f64 {
    1.0
}

/// An interval operand: `"nat"`, `"step"` or `[a, b]`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(untagged)]
enum IvlIn {
    Named(IvlName),
    Explicit([f64; 2]),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum IvlName {
    Nat,
    Step,
}

const NAT: IvlIn = IvlIn::Named(IvlName::Nat);
const STEP: IvlIn = IvlIn::Named(IvlName::Step);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "op")]
enum OpIn {
    State,
    Evaluate {
        t: f64,
        n: usize,
    },
    EvaluateStep {
        t: f64,
        n: usize,
    },
    EvaluatePoint {
        t: f64,
    },
    ToStep {
        t: f64,
    },
    ToNurbs {
        t: f64,
    },
    IsLine,
    Queries,
    IsDegenerate {
        tol: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ivl: Option<IvlIn>,
    },
    DomainBoundary {
        dom: [f64; 4],
        tol: f64,
    },
    ApproxLength {
        ivl: IvlIn,
        turn: bool,
    },
    Length {
        ivl: IvlIn,
    },
    Ends,
    #[serde(rename = "bbox")]
    BBox {
        ivl: IvlIn,
        flags: u8,
    },
    #[serde(rename = "tight_bbox")]
    TightBBox {
        ivl: IvlIn,
    },
    Tessellate {
        ivl: IvlIn,
        nseg: u32,
        flags: u8,
    },
    Drop {
        ivl: IvlIn,
        sop: u8,
        p: [f64; 3],
        tol: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<f64>,
    },
    Gps {
        ivl: IvlIn,
        sop: u8,
        p: [f64; 3],
        tol: f64,
    },
    StepInversion {
        p: [f64; 3],
    },
    IntersectLine {
        ivl: IvlIn,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        other: Option<Box<MakeIn>>,
        other_ivl: IvlIn,
        tol: f64,
    },
    IntersectEllipse {
        ivl: IvlIn,
        frame: [[f64; 3]; 3],
        domain: [f64; 2],
        radii: [f64; 2],
        tol: f64,
    },
    Join {
        end_this: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        other_line: Option<Box<MakeIn>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        other_bspline: Option<BSplineIn>,
        end_other: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tol: Option<f64>,
    },
    Adjust {
        ivl: IvlIn,
    },
    #[serde(rename = "to_2d")]
    To2d,
    #[serde(rename = "to_3d")]
    To3d,
    Offset {
        n: [f64; 3],
        d: f64,
    },
    Refresh,
    Reverse {
        ivl: IvlIn,
    },
    Transform {
        frame: [[f64; 3]; 3],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scale: Option<[f64; 3]>,
    },
    Deform {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        start: Option<[f64; 3]>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        end: Option<[f64; 3]>,
        want_new: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        start_tan: Option<[f64; 3]>,
        start_type: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        end_tan: Option<[f64; 3]>,
        end_type: u8,
    },
    Trim {
        ivl: IvlIn,
        mode: u8,
    },
    EditParam {
        ivl: IvlIn,
    },
    Eq {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        other: Option<Box<MakeIn>>,
    },
    Copy,
    Analytic {
        ivl: IvlIn,
    },
    Assert {
        given: bool,
        with_list: bool,
    },
    KindOf {
        t: u32,
    },
    Canonical,
    SetScale {
        s: f64,
    },
    SetDomain {
        a: f64,
        b: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    make: MakeIn,
    ops: Vec<OpIn>,
}

// ---- C++ side ---------------------------------------------------------------------

fn ffi_bspline(c: &BSplineIn) -> ffi::BSplineData {
    ffi::BSplineData {
        dim: c.dim,
        p: c.p,
        cpts: c
            .cpts
            .iter()
            .map(|q| {
                [
                    q[0].unwrap_or(0.0),
                    q[1].unwrap_or(0.0),
                    q[2].unwrap_or(0.0),
                    q[3].unwrap_or(NOW),
                ]
            })
            .collect(),
        knots: c.knots.clone(),
    }
}

fn ffi_db(d: &LineDbIn) -> ffi::LineDb {
    ffi::LineDb {
        point: d.point,
        vector: d.vector,
        domain: d.domain,
        scale: d.scale,
        inside_out: d.inside_out,
        base: ffi::BaseDb {
            n: d.base.n,
            p: d.base.p,
            rat: d.base.rat,
            dim: d.base.dim,
            kt: d.base.kt,
            form: d.base.form,
            vals: d.base.vals.clone(),
        },
    }
}

fn ffi_make(m: &MakeIn) -> ffi::Make {
    match m {
        MakeIn::Default => ffi::Make::Default,
        MakeIn::Points { a, b, dim } => ffi::Make::Points {
            a: *a,
            b: *b,
            dim: *dim,
        },
        MakeIn::Infinite {
            p,
            v,
            dim,
            inside_out,
        } => ffi::Make::Infinite {
            p: *p,
            v: *v,
            dim: *dim,
            inside_out: *inside_out,
        },
        MakeIn::Bounded {
            p,
            v,
            domain,
            scale,
            dim,
            inside_out,
            nurb,
        } => ffi::Make::Bounded {
            p: *p,
            v: *v,
            domain: *domain,
            scale: *scale,
            dim: *dim,
            inside_out: *inside_out,
            nurb: nurb.as_ref().map(ffi_bspline),
        },
        MakeIn::Segment { dim, a, b, interval } => ffi::Make::Segment {
            dim: *dim,
            a: *a,
            b: *b,
            interval: *interval,
        },
        MakeIn::Canonical { p, v } => ffi::Make::Canonical { p: *p, v: *v },
        MakeIn::Read {
            dim,
            version,
            binary,
            db,
        } => ffi::Make::Read {
            dim: *dim,
            version: *version,
            binary: *binary,
            db: ffi_db(db),
        },
    }
}

fn ffi_ivl(i: IvlIn) -> ffi::Ivl {
    match i {
        IvlIn::Named(IvlName::Nat) => ffi::Ivl::Natural,
        IvlIn::Named(IvlName::Step) => ffi::Ivl::Step,
        IvlIn::Explicit([a, b]) => ffi::Ivl::Explicit(a, b),
    }
}

fn ffi_op(o: &OpIn) -> Result<ffi::Op, HarnessError> {
    use ffi::Op as F;
    Ok(match o {
        OpIn::State => F::State,
        OpIn::Evaluate { t, n } => F::Evaluate(*t, *n),
        OpIn::EvaluateStep { t, n } => F::EvaluateStep(*t, *n),
        OpIn::EvaluatePoint { t } => F::EvaluatePoint(*t),
        OpIn::ToStep { t } => F::ToStep(*t),
        OpIn::ToNurbs { t } => F::ToNurbs(*t),
        OpIn::IsLine => F::IsLine,
        OpIn::Queries => F::Queries,
        OpIn::IsDegenerate { tol, ivl } => F::IsDegenerate(*tol, ivl.map(ffi_ivl)),
        OpIn::DomainBoundary { dom, tol } => F::DomainBoundary(*dom, *tol),
        OpIn::ApproxLength { ivl, turn } => F::ApproxLength(ffi_ivl(*ivl), *turn),
        OpIn::Length { ivl } => F::Length(ffi_ivl(*ivl)),
        OpIn::Ends => F::Ends,
        OpIn::BBox { ivl, flags } => F::BBox(ffi_ivl(*ivl), *flags),
        OpIn::TightBBox { ivl } => F::TightBBox(ffi_ivl(*ivl)),
        OpIn::Tessellate { ivl, nseg, flags } => F::Tessellate(ffi_ivl(*ivl), *nseg, *flags),
        OpIn::Drop {
            ivl,
            sop,
            p,
            tol,
            target,
        } => F::Drop {
            ivl: ffi_ivl(*ivl),
            op: *sop,
            p: *p,
            tol: *tol,
            target: *target,
        },
        OpIn::Gps { ivl, sop, p, tol } => F::Gps {
            ivl: ffi_ivl(*ivl),
            op: *sop,
            p: *p,
            tol: *tol,
        },
        OpIn::StepInversion { p } => F::StepInversion(*p),
        OpIn::IntersectLine {
            ivl,
            other,
            other_ivl,
            tol,
        } => F::IntersectLine {
            ivl: ffi_ivl(*ivl),
            other: other.as_ref().map(|m| Box::new(ffi_make(m))),
            other_ivl: ffi_ivl(*other_ivl),
            tol: *tol,
        },
        OpIn::IntersectEllipse {
            ivl,
            frame,
            domain,
            radii,
            tol,
        } => F::IntersectEllipse {
            ivl: ffi_ivl(*ivl),
            frame: *frame,
            domain: *domain,
            radii: *radii,
            tol: *tol,
        },
        OpIn::Join {
            end_this,
            other_line,
            other_bspline,
            end_other,
            tol,
        } => F::Join {
            end_this: *end_this,
            other: match (other_line, other_bspline) {
                (Some(m), None) => Ok(Box::new(ffi_make(m))),
                (None, Some(b)) => Err(ffi_bspline(b)),
                _ => return Err(harness("join needs exactly one other curve")),
            },
            end_other: *end_other,
            tol: *tol,
        },
        OpIn::Adjust { ivl } => F::Adjust(ffi_ivl(*ivl)),
        OpIn::To2d => F::To2d,
        OpIn::To3d => F::To3d,
        OpIn::Offset { n, d } => F::Offset(*n, *d),
        OpIn::Refresh => F::Refresh,
        OpIn::Reverse { ivl } => F::Reverse(ffi_ivl(*ivl)),
        OpIn::Transform { frame, scale } => F::Transform(*frame, *scale),
        OpIn::Deform {
            start,
            end,
            want_new,
            start_tan,
            start_type,
            end_tan,
            end_type,
        } => F::Deform {
            start: *start,
            end: *end,
            want_new: *want_new,
            start_tan: (*start_tan, *start_type),
            end_tan: (*end_tan, *end_type),
        },
        OpIn::Trim { ivl, mode } => F::Trim(ffi_ivl(*ivl), *mode),
        OpIn::EditParam { ivl } => F::EditParam(ffi_ivl(*ivl)),
        OpIn::Eq { other } => F::Eq(other.as_ref().map(|m| Box::new(ffi_make(m)))),
        OpIn::Copy => F::Copy,
        OpIn::Analytic { ivl } => F::Analytic(ffi_ivl(*ivl)),
        OpIn::Assert { given, with_list } => F::Assert {
            given: *given,
            with_list: *with_list,
        },
        OpIn::KindOf { t } => F::KindOf(*t),
        OpIn::Canonical => F::Canonical,
        OpIn::SetScale { s } => F::SetScale(*s),
        OpIn::SetDomain { a, b } => F::SetDomain(*a, *b),
    })
}

fn ffi_program(i: &Inputs) -> Result<(ffi::Make, Vec<ffi::Op>), HarnessError> {
    let ops = i.ops.iter().map(ffi_op).collect::<Result<Vec<_>, _>>()?;
    Ok((ffi_make(&i.make), ops))
}

fn to_output(rec: Vec<f64>) -> Output {
    Output::new().values("rec", rec)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (m, ops) = ffi_program(i)?;
    match ffi::run(&m, &ops) {
        Ok(rec) => Ok(to_output(rec)),
        Err(e) => Err(harness(e.to_string())),
    }
}

// ---- Rust side --------------------------------------------------------------------

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
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

/// A failed Rust operation: the whole case is deferred, or the op has a status.
enum Fail {
    Deferred(Box<Output>),
    Harness(HarnessError),
}

impl From<HarnessError> for Fail {
    fn from(e: HarnessError) -> Self {
        Fail::Harness(e)
    }
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

fn put_nurb(o: &mut Vec<f64>, c: &BSplineCurve) {
    o.extend([f64::from(c.knot_type() as i32), f64::from(c.form() as i32)]);
    match c.gw_nurb() {
        None => o.push(0.0),
        Some(cur) => {
            o.extend([1.0, cur.n() as f64, cur.degree() as f64, cur.m() as f64]);
            for p in cur.cpts() {
                o.extend([p.x, p.y, p.z, p.w]);
            }
            o.extend_from_slice(cur.knots());
        }
    }
}

fn put_line(o: &mut Vec<f64>, l: &Line) {
    o.extend([l.type_code() as f64, l.dim() as f64]);
    put_vec(o, l.line_point());
    put_vec(o, l.line_vector());
    put_ivl(o, &l.step_interval());
    o.extend([l.line_scale(), b(l.inside_out())]);
    put_nurb(o, l.base());
    put_ivl(o, &l.natural_interval());
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
            id(s.objects[1]),
            s.start.solution_value,
            s.start.parameters[0],
            s.start.parameters[1],
            s.end.solution_value,
            s.end.parameters[0],
            s.end.parameters[1],
        ]);
    }
}

fn put_chars(o: &mut Vec<f64>, s: &str) {
    o.extend(s.bytes().map(f64::from));
}

fn rust_bspline(c: &BSplineIn) -> Result<BSplineCurve, HarnessError> {
    let d = ffi_bspline(c);
    let pw: Vec<CPoint> = d
        .cpts
        .iter()
        .map(|q| CPoint {
            x: q[0],
            y: q[1],
            z: q[2],
            w: q[3],
        })
        .collect();
    let n = pw.len().checked_sub(1).ok_or_else(|| harness("empty B-spline"))?;
    let m = d.knots.len().checked_sub(1).ok_or_else(|| harness("no knots"))?;
    BSplineCurve::from_data(u64::from(d.dim), n, m, d.p, &pw, &d.knots)
        .map_err(|e| harness(format!("B-spline: {e}")))
}

/// Builds a line; `Err(status)` when the construction fails.
fn rust_make(m: &MakeIn) -> Result<Result<Line, f64>, Fail> {
    Ok(match m {
        MakeIn::Default => Ok(Line::default()),
        MakeIn::Points { a, b, dim } => Ok(Line::from_points(v3(*a), v3(*b), u64::from(*dim))),
        MakeIn::Infinite {
            p,
            v,
            dim,
            inside_out,
        } => Ok(Line::infinite(v3(*p), v3(*v), u64::from(*dim), *inside_out)),
        MakeIn::Bounded {
            p,
            v,
            domain,
            scale,
            dim,
            inside_out,
            nurb,
        } => {
            let nurb = nurb.as_ref().map(rust_bspline).transpose()?;
            Ok(Line::bounded(
                v3(*p),
                v3(*v),
                &Extent1d::from_min_max(domain[0], domain[1]),
                &BoundedLineOptions {
                    scale: *scale,
                    dim: u64::from(*dim),
                    nurb: nurb.as_ref(),
                    inside_out: *inside_out,
                },
            ))
        }
        MakeIn::Segment { dim, a, b, interval } => {
            let ivl = interval.map(|[x, y]| Extent1d::from_min_max(x, y));
            let mut o = Vec::new();
            match st(
                &mut o,
                Line::create_line_segment(u64::from(*dim), v3(*a), v3(*b), ivl.as_ref()),
            )? {
                Some(l) => Ok(l),
                None => Err(o.first().copied().unwrap_or(-1.0)),
            }
        }
        MakeIn::Canonical { p, v } => Ok(Line::create_canonical(v3(*p), v3(*v))),
        MakeIn::Read {
            dim,
            version,
            binary,
            db,
        } => {
            let bytes = ffi::line_db_bytes(&ffi_db(db), *binary, u64::from(*version))
                .map_err(|e| harness(e.to_string()))?;
            let mut reader = DatabaseReader::new();
            reader.open_for_read(
                bytes,
                if *binary {
                    FileType::Binary
                } else {
                    FileType::Ascii
                },
            );
            let mut o = Vec::new();
            match st(
                &mut o,
                Line::read_from_db(&mut reader, u64::from(*dim), u64::from(*version)),
            )? {
                Some(l) => Ok(l),
                None => Err(o.first().copied().unwrap_or(-1.0)),
            }
        }
    })
}

fn ivl_of(l: &Line, i: IvlIn) -> Extent1d {
    match i {
        IvlIn::Named(IvlName::Nat) => l.natural_interval(),
        IvlIn::Named(IvlName::Step) => l.step_interval(),
        IvlIn::Explicit([a, c]) => Extent1d::from_min_max(a, c),
    }
}

fn sop(code: u8) -> Result<SolverOperation, HarnessError> {
    SolverOperation::from_code(i32::from(code)).ok_or_else(|| harness("solver operation"))
}

fn in_value(code: u8) -> Result<InValueType, HarnessError> {
    Ok(match code {
        0 => InValueType::Specified,
        1 => InValueType::Same,
        2 => InValueType::Unconstrained,
        _ => return Err(harness("in value type")),
    })
}

fn other_line(m: &MakeIn) -> Result<Line, Fail> {
    rust_make(m)?.map_err(|_| Fail::Harness(harness("other line construction failed")))
}

#[allow(clippy::too_many_lines)]
fn rust_op(op: &OpIn, a: &mut Line, o: &mut Vec<f64>) -> Result<(), Fail> {
    match op {
        OpIn::State => {
            o.push(0.0);
            put_line(o, a);
        }
        OpIn::Evaluate { t, n } => {
            o.push(0.0);
            for v in a.evaluate(*t, *n) {
                put_vec(o, v);
            }
        }
        OpIn::EvaluateStep { t, n } => {
            o.push(0.0);
            for v in a.evaluate_step(*t, *n) {
                put_vec(o, v);
            }
        }
        OpIn::EvaluatePoint { t } => {
            o.push(0.0);
            put_vec(o, a.evaluate_point(*t));
        }
        OpIn::ToStep { t } => o.extend([0.0, a.convert_t_from_nurbs_to_step(*t)]),
        OpIn::ToNurbs { t } => o.extend([0.0, a.convert_t_from_step_to_nurbs(*t)]),
        OpIn::IsLine => {
            let (p, v) = a.is_line();
            o.extend([0.0, 1.0]);
            put_vec(o, p);
            put_vec(o, v);
        }
        OpIn::Queries => {
            let nat = a.natural_interval();
            o.extend([
                0.0,
                b(a.is_bounded()),
                b(a.is_analytic()),
                b(a.is_closed(&nat, 0.0)),
                b(a.is_arc(5, 0.0)),
                a.degree() as f64,
                a.number_natural_knots() as f64,
            ]);
            put_ivl(o, &a.max_analytic_domain());
            put_ivl(o, &a.step_interval());
            o.push(a.type_code() as f64);
            put_chars(o, a.type_string());
            o.push(-1.0);
            put_chars(o, a.class_string());
        }
        OpIn::IsDegenerate { tol, ivl } => {
            let i = ivl.map(|i| ivl_of(a, i));
            o.extend([0.0, b(a.is_degenerate(*tol, i.as_ref()))]);
        }
        OpIn::DomainBoundary { dom, tol } => {
            let d = Extent2d::from_points(Point2::new(dom[0], dom[1]), Point2::new(dom[2], dom[3]));
            o.extend([0.0, f64::from(a.is_domain_boundary(&d, *tol) as i32)]);
        }
        OpIn::ApproxLength { ivl, turn } => {
            let (len, ang) = a.approximate_length(&ivl_of(a, *ivl), *turn);
            o.extend([0.0, len, ang.unwrap_or(-7.0)]);
        }
        OpIn::Length { ivl } => o.extend([0.0, a.length(&ivl_of(a, *ivl))]),
        OpIn::Ends => {
            if let Some(e) = st(o, a.ends())? {
                put_vec(o, e.start);
                put_vec(o, e.end);
                o.extend([e.start_weight, e.end_weight]);
            }
        }
        OpIn::BBox { ivl, flags } => {
            let boxes = a.calculate_bounding_box(
                &ivl_of(a, *ivl),
                BoundingBoxRequest {
                    normal: flags & 1 != 0,
                    pseudo: flags & 2 != 0,
                    polar: flags & 4 != 0,
                },
            );
            o.push(0.0);
            put_boxes(o, &boxes);
        }
        OpIn::TightBBox { ivl } => {
            let boxes = BoundingBoxes {
                normal: a.calculate_tight_bounding_box(&ivl_of(a, *ivl)),
                ..BoundingBoxes::default()
            };
            o.push(0.0);
            put_boxes(o, &boxes);
        }
        OpIn::Tessellate { ivl, nseg, flags } => {
            let t = a.tessellate(
                &ivl_of(a, *ivl),
                u64::from(*nseg),
                TessellationRequest {
                    parameters: flags & 1 != 0,
                    points: flags & 2 != 0,
                    tangents: flags & 4 != 0,
                },
            );
            o.push(0.0);
            if flags & 1 != 0 {
                o.push(t.parameters.len() as f64);
                o.extend_from_slice(&t.parameters);
            }
            if flags & 2 != 0 {
                o.push(t.points.len() as f64);
                for p in &t.points {
                    put_vec(o, *p);
                }
            }
            if flags & 4 != 0 {
                o.push(t.tangents.len() as f64);
                for p in &t.tangents {
                    put_vec(o, *p);
                }
            }
        }
        OpIn::Drop {
            ivl,
            sop: s,
            p,
            tol,
            target,
        } => {
            let r = a.drop_point_fast(&ivl_of(a, *ivl), sop(*s)?, v3(*p), *tol, *target, Some(0i64));
            if let Some(sols) = st(o, r)? {
                put_sols(o, &sols);
            }
        }
        OpIn::Gps {
            ivl,
            sop: s,
            p,
            tol,
        } => {
            let sols = a.global_point_solve_step(&ivl_of(a, *ivl), sop(*s)?, v3(*p), *tol, Some(0i64));
            o.push(0.0);
            put_sols(o, &sols);
        }
        OpIn::StepInversion { p } => {
            let (t, loc) = a.step_inversion(v3(*p));
            o.extend([0.0, t, f64::from(loc as i32)]);
        }
        OpIn::IntersectLine {
            ivl,
            other,
            other_ivl,
            tol,
        } => {
            let i = ivl_of(a, *ivl);
            let r = if let Some(m) = other {
                let bl = other_line(m)?;
                let oi = ivl_of(&bl, *other_ivl);
                a.intersect_with_line(&i, &bl, &oi, *tol, Some(0i64), Some(1i64))
            } else {
                let aa: &Line = a;
                let oi = ivl_of(aa, *other_ivl);
                aa.intersect_with_line(&i, aa, &oi, *tol, Some(0i64), Some(1i64))
            };
            if let Some(r) = st(o, r)? {
                o.push(b(r.needs_more_intersections));
                put_sols(o, &r.solutions);
            }
        }
        OpIn::IntersectEllipse {
            ivl,
            frame,
            domain,
            radii,
            tol,
        } => {
            let i = ivl_of(a, *ivl);
            let k = Kernel::default();
            let e = Ellipse::new(
                &k,
                v3(frame[0]),
                v3(frame[1]),
                v3(frame[2]),
                Extent1d::from_min_max(domain[0], domain[1]),
                radii[0],
                radii[1],
                &EllipseOptions::default(),
            );
            let e = e.map_err(|err| match Output::unported(&err) {
                Some(d) => Fail::Deferred(Box::new(d)),
                None => Fail::Harness(harness(err.to_string())),
            })?;
            let ei = e.base().natural_interval();
            let r = a.intersect_with_ellipse::<i64>(&Kernel::default(), &i, &e, &ei, *tol, Some(0), Some(1));
            if let Some(r) = st(o, r)? {
                o.push(b(r.needs_more_intersections));
                put_sols(o, &r.solutions);
            }
        }
        OpIn::Join {
            end_this,
            other_line: ol,
            other_bspline,
            end_other,
            tol,
        } => {
            let other = match (ol, other_bspline) {
                (Some(m), None) => Curve::from(other_line(m)?),
                (None, Some(c)) => Curve::from(rust_bspline(c)?),
                _ => return Err(Fail::Harness(harness("join needs one other curve"))),
            };
            let r = a.join_with(u64::from(*end_this), &other, u64::from(*end_other), *tol);
            st(o, r)?;
        }
        OpIn::Adjust { ivl } => {
            let i = ivl_of(a, *ivl);
            st(o, a.adjust_step_interval(&i))?;
        }
        OpIn::To2d => {
            st(o, a.convert_to_2d())?;
        }
        OpIn::To3d => {
            st(o, a.convert_to_3d())?;
        }
        OpIn::Offset { n, d } => {
            if let Some((l, gap)) = st(o, a.create_simple_offset(v3(*n), *d))? {
                o.push(gap);
                put_line(o, &l);
            }
        }
        OpIn::Refresh => {
            st(o, a.refresh_analytics())?;
        }
        OpIn::Reverse { ivl } => {
            let i = ivl_of(a, *ivl);
            let n = a.reverse_parameterization(&i);
            o.push(0.0);
            put_ivl(o, &n);
        }
        OpIn::Transform { frame, scale } => {
            let f = Axis2Placement::new(v3(frame[0]), v3(frame[1]), v3(frame[2]));
            st(o, a.transform(&f, scale.map(v3)))?;
        }
        OpIn::Deform {
            start,
            end,
            want_new,
            start_tan,
            start_type,
            end_tan,
            end_type,
        } => {
            let r = a.deform_to_end_point_targets(
                start.map(v3),
                end.map(v3),
                *want_new,
                start_tan.map(v3),
                in_value(*start_type)?,
                end_tan.map(v3),
                in_value(*end_type)?,
            );
            if let Some(c) = st(o, r)? {
                o.push(b(c.is_some()));
                if let Some(c) = c {
                    o.extend([c.type_code() as f64, c.dim() as f64]);
                    put_nurb(o, &c);
                }
            }
        }
        OpIn::Trim { ivl, mode } => {
            let i = ivl_of(a, *ivl);
            let m = match mode {
                0 => TrimMode::Notify,
                1 => TrimMode::Silent,
                _ => TrimMode::IntervalOnly,
            };
            st(o, a.trim(&i, m))?;
        }
        OpIn::EditParam { ivl } => {
            let i = ivl_of(a, *ivl);
            st(o, a.edit_parameterization(&i))?;
        }
        OpIn::Eq { other } => {
            let eq = match other {
                Some(m) => a.approx_eq(&other_line(m)?),
                None => a.approx_eq(a),
            };
            o.extend([0.0, b(eq)]);
        }
        OpIn::Copy => {
            o.push(0.0);
            put_line(o, &a.copy());
        }
        OpIn::Analytic { ivl } => {
            let i = ivl_of(a, *ivl);
            if let Some(c) = st(o, a.create_analytic_curve(&i))? {
                o.push(b(c.is_some()));
                if let Some(c) = c {
                    put_line(o, &c);
                }
            }
        }
        OpIn::Assert { given, with_list } => {
            let k = Kernel::default();
            let mut list = AssertArray::new();
            let r = a.assert_valid(&k, with_list.then_some(&mut list), *given, ObjectKey(1));
            if let Some(ok) = st(o, r)? {
                o.extend([b(ok), list.len() as f64]);
                for r in list.reports() {
                    o.extend([r.test_index as f64, r.tol3d, r.value]);
                }
            }
        }
        OpIn::KindOf { t } => o.extend([0.0, b(a.is_kind_of(i64::from(*t)))]),
        OpIn::Canonical => {
            let c = a.canonical();
            let u = a.canonical_unit();
            o.push(0.0);
            put_vec(o, c.point);
            put_vec(o, c.vector);
            put_ivl(o, &c.domain);
            o.push(b(c.inside_out));
            put_vec(o, u.point);
            put_vec(o, u.vector);
            o.push(u.scale);
            put_ivl(o, &u.domain);
            o.push(b(u.inside_out));
        }
        OpIn::SetScale { s } => {
            a.set_line_scale(*s);
            o.push(0.0);
        }
        OpIn::SetDomain { a: x, b: y } => {
            a.set_analytic_domain(Extent1d::from_min_max(*x, *y));
            o.push(0.0);
        }
    }
    Ok(())
}

fn put_boxes(o: &mut Vec<f64>, boxes: &BoundingBoxes) {
    if let Some(nb) = &boxes.normal {
        put_vec(o, nb.min());
        put_vec(o, nb.max());
    }
    if let Some(pb) = &boxes.pseudo {
        for v in pb.basis() {
            put_vec(o, *v);
        }
        for e in pb.intervals() {
            put_ivl(o, e);
        }
    }
    if let Some(xb) = &boxes.polar {
        o.push(f64::from(xb.state().code()));
        for v in xb.basis() {
            put_vec(o, *v);
        }
        let d = xb.domain();
        o.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
    }
}

fn op_code(op: &OpIn) -> Result<f64, HarnessError> {
    let prog = ffi::encode(&ffi::Make::Default, &[ffi_op(op)?]).map_err(|e| harness(e.to_string()))?;
    // `0` (default line), `1` (op count), then the op code
    prog.get(2).copied().ok_or_else(|| harness("empty op"))
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let mut o = Vec::new();
    let made = match rust_make(&i.make) {
        Ok(m) => m,
        Err(Fail::Deferred(d)) => return Ok(*d),
        Err(Fail::Harness(e)) => return Err(e),
    };
    let mut a = match made {
        Ok(l) => {
            o.push(0.0);
            l
        }
        Err(s) => {
            o.push(s);
            return Ok(to_output(o));
        }
    };
    for op in &i.ops {
        o.push(op_code(op)?);
        match rust_op(op, &mut a, &mut o) {
            Ok(()) => {}
            Err(Fail::Deferred(d)) => return Ok(*d),
            Err(Fail::Harness(e)) => return Err(e),
        }
    }
    put_line(&mut o, &a);
    Ok(to_output(o))
}

// ---- generated cases --------------------------------------------------------------

fn small(rng: &mut Rng) -> f64 {
    // coordinates on a coarse grid sometimes, so coincidences happen
    if rng.chance(0.3) {
        rng.range_i64(-4, 4) as f64
    } else {
        rng.uniform(-10.0, 10.0)
    }
}

fn pt(rng: &mut Rng) -> [f64; 3] {
    [small(rng), small(rng), small(rng)]
}

fn dir(rng: &mut Rng) -> [f64; 3] {
    match rng.below(10) {
        0 => [0.0, 0.0, 0.0],
        1 => [1.0, 0.0, 0.0],
        2 => [0.0, 0.0, 1.0],
        _ => pt(rng),
    }
}

fn gen_bspline(rng: &mut Rng, a: [f64; 3], c: [f64; 3]) -> BSplineIn {
    // a degree-1 (sometimes degree-2) B-spline from a to c, or a wavy cubic
    match rng.below(3) {
        0 => BSplineIn {
            dim: 3,
            p: 1,
            cpts: vec![
                [Some(a[0]), Some(a[1]), Some(a[2]), None],
                [Some(c[0]), Some(c[1]), Some(c[2]), None],
            ],
            knots: vec![0.0, 0.0, 1.0, 1.0],
        },
        1 => {
            let m = [(a[0] + c[0]) / 2.0, (a[1] + c[1]) / 2.0, (a[2] + c[2]) / 2.0];
            BSplineIn {
                dim: 3,
                p: 2,
                cpts: vec![
                    [Some(a[0]), Some(a[1]), Some(a[2]), None],
                    [Some(m[0]), Some(m[1]), Some(m[2]), None],
                    [Some(c[0]), Some(c[1]), Some(c[2]), None],
                ],
                knots: vec![0.0, 0.0, 0.0, 2.0, 2.0, 2.0],
            }
        }
        _ => {
            let q = pt(rng);
            BSplineIn {
                dim: 3,
                p: 2,
                cpts: vec![
                    [Some(a[0]), Some(a[1]), Some(a[2]), None],
                    [Some(q[0]), Some(q[1]), Some(q[2]), None],
                    [Some(c[0]), Some(c[1]), Some(c[2]), None],
                ],
                knots: vec![1.0, 1.0, 1.0, 3.0, 3.0, 3.0],
            }
        }
    }
}

fn gen_domain(rng: &mut Rng) -> [f64; 2] {
    match rng.below(6) {
        0 => [0.0, 1.0],
        1 => [2.0, 7.0],
        2 => [-3.0, 3.0],
        3 => [1.0, 1.0],
        _ => {
            let x = rng.uniform(-5.0, 5.0);
            [x, x + rng.uniform(0.0, 8.0)]
        }
    }
}

fn gen_db(rng: &mut Rng) -> (LineDbIn, u32) {
    let a = pt(rng);
    let c = pt(rng);
    let dim2 = rng.chance(0.25);
    let rat = rng.chance(0.2);
    let mut vals = Vec::new();
    for p in [a, c] {
        let w = if rat { rng.uniform(0.5, 2.0) } else { 1.0 };
        vals.extend([p[0] * if rat { w } else { 1.0 }, p[1] * if rat { w } else { 1.0 }]);
        if !dim2 {
            vals.push(p[2] * if rat { w } else { 1.0 });
        }
        if rat {
            vals.push(w);
        }
    }
    let d = gen_domain(rng);
    vals.extend([d[0], d[0], d[1], d[1]]);
    let v = dir(rng);
    let version = if rng.chance(0.5) { 30 } else { 33 };
    (
        LineDbIn {
            point: a,
            vector: v,
            domain: if rng.chance(0.1) { [d[1], d[0] - 1.0] } else { d },
            scale: rng.uniform(0.1, 4.0),
            inside_out: i32::from(rng.chance(0.3)),
            base: BaseDbIn {
                n: 1,
                p: 1,
                rat: i16::from(rat),
                dim: if dim2 { 2 } else { 3 },
                kt: rng.below(5),
                form: rng.below(7),
                vals,
            },
        },
        version,
    )
}

fn gen_make(rng: &mut Rng, depth: u32) -> MakeIn {
    let dim = if rng.chance(0.15) { 2 } else { 3 };
    // nested lines (other operands) use constructions that cannot fail
    match rng.below(if depth == 0 { 14 } else { 9 }) {
        0 => MakeIn::Default,
        1 | 2 => {
            let a = pt(rng);
            let c = if rng.chance(0.1) { a } else { pt(rng) };
            MakeIn::Points { a, b: c, dim }
        }
        3 | 4 => {
            let mut v = dir(rng);
            if v == [0.0, 0.0, 0.0] {
                v = [0.0, 1.0, 0.0];
            }
            MakeIn::Infinite {
                p: pt(rng),
                v,
                dim,
                inside_out: rng.chance(0.4),
            }
        }
        5..=8 => {
            let p = pt(rng);
            let v = dir(rng);
            let domain = gen_domain(rng);
            let scale = if rng.chance(0.2) { 1.0 } else { rng.uniform(0.1, 5.0) };
            let inside_out = rng.chance(0.4);
            let nurb = if rng.chance(0.25) {
                let e = pt(rng);
                Some(gen_bspline(rng, p, e))
            } else {
                None
            };
            MakeIn::Bounded {
                p,
                v,
                domain,
                scale,
                dim,
                inside_out,
                nurb,
            }
        }
        9 | 10 => {
            let a = pt(rng);
            let c = if rng.chance(0.1) { a } else { pt(rng) };
            MakeIn::Segment {
                dim,
                a,
                b: c,
                interval: if rng.chance(0.5) {
                    Some(gen_domain(rng))
                } else {
                    None
                },
            }
        }
        11 => {
            let mut v = dir(rng);
            if v == [0.0, 0.0, 0.0] {
                v = [1.0, 1.0, 0.0];
            }
            MakeIn::Canonical { p: pt(rng), v }
        }
        _ => {
            let (db, version) = gen_db(rng);
            MakeIn::Read {
                dim: if rng.chance(0.3) { 2 } else { 3 },
                version,
                binary: rng.chance(0.6),
                db,
            }
        }
    }
}

fn gen_ivl(rng: &mut Rng) -> IvlIn {
    match rng.below(5) {
        0 | 1 => NAT,
        2 => STEP,
        _ => {
            let a = rng.uniform(-4.0, 8.0);
            IvlIn::Explicit([a, a + rng.uniform(0.0, 6.0)])
        }
    }
}

fn gen_param(rng: &mut Rng) -> f64 {
    if rng.chance(0.3) {
        rng.range_i64(-2, 8) as f64
    } else {
        rng.uniform(-4.0, 9.0)
    }
}

#[allow(clippy::too_many_lines)]
fn gen_op(rng: &mut Rng) -> OpIn {
    match rng.below(40) {
        0 => OpIn::State,
        1 | 2 => OpIn::Evaluate {
            t: gen_param(rng),
            n: rng.below(4) as usize,
        },
        3 => OpIn::EvaluateStep {
            t: gen_param(rng),
            n: rng.below(4) as usize,
        },
        4 => OpIn::EvaluatePoint { t: gen_param(rng) },
        5 => OpIn::ToStep { t: gen_param(rng) },
        6 => OpIn::ToNurbs { t: gen_param(rng) },
        7 => OpIn::IsLine,
        8 => OpIn::Queries,
        9 => OpIn::IsDegenerate {
            tol: *rng.choose(&[0.0, 1e-6, 0.5, 3.0]).unwrap_or(&0.0),
            ivl: if rng.chance(0.5) { Some(gen_ivl(rng)) } else { None },
        },
        10 => {
            let u = rng.range_i64(-3, 3) as f64;
            let v = rng.range_i64(-3, 3) as f64;
            OpIn::DomainBoundary {
                dom: [u, v, u + rng.range_i64(0, 8) as f64, v + rng.range_i64(0, 8) as f64],
                tol: *rng.choose(&[1e-10, 1e-3, 0.5]).unwrap_or(&1e-10),
            }
        }
        11 => OpIn::ApproxLength {
            ivl: gen_ivl(rng),
            turn: rng.chance(0.5),
        },
        12 => OpIn::Length { ivl: gen_ivl(rng) },
        13 => OpIn::Ends,
        14 => OpIn::BBox {
            ivl: gen_ivl(rng),
            flags: 1 + rng.below(7) as u8,
        },
        15 => OpIn::TightBBox { ivl: gen_ivl(rng) },
        16 => OpIn::Tessellate {
            ivl: gen_ivl(rng),
            nseg: rng.below(6) as u32,
            flags: rng.below(8) as u8,
        },
        17 | 18 => OpIn::Drop {
            ivl: gen_ivl(rng),
            sop: *rng.choose(&[0u8, 3, 3, 4, 5]).unwrap_or(&3),
            p: pt(rng),
            tol: *rng.choose(&[1e-6, 0.5, 5.0, 20.0]).unwrap_or(&1e-6),
            target: if rng.chance(0.3) { Some(rng.uniform(0.0, 10.0)) } else { None },
        },
        19 | 20 => OpIn::Gps {
            ivl: gen_ivl(rng),
            sop: *rng.choose(&[0u8, 3, 4, 5]).unwrap_or(&3),
            p: pt(rng),
            tol: *rng.choose(&[1e-6, 0.5, 5.0, 20.0]).unwrap_or(&1e-6),
        },
        21 => OpIn::StepInversion { p: pt(rng) },
        22 | 23 => OpIn::IntersectLine {
            ivl: gen_ivl(rng),
            other: if rng.chance(0.1) {
                None
            } else {
                Some(Box::new(gen_make(rng, 1)))
            },
            other_ivl: gen_ivl(rng),
            tol: *rng.choose(&[1e-6, 1e-3, 0.5]).unwrap_or(&1e-6),
        },
        24 => {
            let a = pt(rng);
            OpIn::Join {
                end_this: rng.below(2) as u8,
                other_line: if rng.chance(0.6) {
                    Some(Box::new(gen_make(rng, 1)))
                } else {
                    None
                },
                other_bspline: None,
                end_other: rng.below(2) as u8,
                tol: if rng.chance(0.5) { Some(*rng.choose(&[1e-7, 1e-3, 1.0]).unwrap_or(&1e-7)) } else { None },
            }
            .with_bspline_fallback(rng, a)
        }
        25 => OpIn::Adjust { ivl: gen_ivl(rng) },
        26 => OpIn::To2d,
        27 => OpIn::To3d,
        28 => OpIn::Offset {
            n: dir(rng),
            d: rng.uniform(-3.0, 3.0),
        },
        29 => OpIn::Refresh,
        30 => OpIn::Reverse { ivl: gen_ivl(rng) },
        31 => {
            let x = dir(rng);
            let y = dir(rng);
            OpIn::Transform {
                frame: [pt(rng), x, y],
                scale: if rng.chance(0.4) {
                    Some([rng.uniform(0.2, 3.0), rng.uniform(0.2, 3.0), rng.uniform(0.2, 3.0)])
                } else {
                    None
                },
            }
        }
        32 => OpIn::Deform {
            start: if rng.chance(0.6) { Some(pt(rng)) } else { None },
            end: if rng.chance(0.6) { Some(pt(rng)) } else { None },
            want_new: rng.chance(0.7),
            start_tan: if rng.chance(0.5) { Some(dir(rng)) } else { None },
            start_type: if rng.chance(0.6) { 2 } else { rng.below(3) as u8 },
            end_tan: if rng.chance(0.5) { Some(dir(rng)) } else { None },
            end_type: if rng.chance(0.6) { 2 } else { rng.below(3) as u8 },
        },
        33 => OpIn::Trim {
            ivl: gen_ivl(rng),
            mode: rng.below(3) as u8,
        },
        34 => OpIn::EditParam { ivl: gen_ivl(rng) },
        35 => OpIn::Eq {
            other: if rng.chance(0.3) {
                None
            } else {
                Some(Box::new(gen_make(rng, 1)))
            },
        },
        36 => OpIn::Copy,
        37 => OpIn::Analytic { ivl: gen_ivl(rng) },
        38 => OpIn::KindOf {
            t: *rng
                .choose(&[14010u32, 14033, 14001, 14022, 1, 12000, 0])
                .unwrap_or(&14010),
        },
        _ => {
            match rng.below(4) {
                0 | 1 => OpIn::Canonical,
                2 => OpIn::SetScale {
                    s: rng.uniform(0.1, 3.0),
                },
                _ => {
                    let a = rng.uniform(-3.0, 3.0);
                    OpIn::SetDomain {
                        a,
                        b: a + rng.uniform(0.0, 4.0),
                    }
                }
            }
        }
    }
}

impl OpIn {
    /// A join without other line gets a B-spline from `a`.
    fn with_bspline_fallback(self, rng: &mut Rng, a: [f64; 3]) -> Self {
        match self {
            OpIn::Join {
                end_this,
                other_line: None,
                end_other,
                tol,
                ..
            } => {
                let c = pt(rng);
                OpIn::Join {
                    end_this,
                    other_line: None,
                    other_bspline: Some(gen_bspline(rng, a, c)),
                    end_other,
                    tol,
                }
            }
            other => other,
        }
    }
}

/// Whether `m` builds a line whose NURB has a non-empty knot interval and distinct
/// end points.
fn regular(m: &MakeIn) -> bool {
    let nonzero = |v: &[f64; 3]| v.iter().any(|&x| x != 0.0);
    match m {
        MakeIn::Points { a, b, .. } => a != b,
        MakeIn::Segment { a, b, interval, .. } => {
            a != b && interval.is_none_or(|[x, y]| (y - x).abs() > 1e-3)
        }
        MakeIn::Infinite { v, .. } | MakeIn::Canonical { v, .. } => nonzero(v),
        MakeIn::Bounded {
            v, domain, nurb, ..
        } => nonzero(v) && domain[1] > domain[0] + 1e-3 && nurb.is_none(),
        MakeIn::Default | MakeIn::Read { .. } => false,
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let make = gen_make(rng, 0);
    let n = 1 + rng.below(5) as usize;
    let mut ops: Vec<OpIn> = (0..n).map(|_| gen_op(rng)).collect();
    // `SmBSplineCurve::DeformToEndPointTargets` indexes past its arrays when the
    // degree elevation of a degenerate NURB fails (see plan/findings.md): only the first
    // op on a regular line may deform into a new B-spline
    for (k, op) in ops.iter_mut().enumerate() {
        if let OpIn::Deform {
            start_type,
            end_type,
            ..
        } = op
        {
            if k > 0 || !regular(&make) {
                *start_type = 2;
                *end_type = 2;
            }
        }
    }
    if rng.chance(0.1) {
        ops.push(OpIn::Assert {
            given: true,
            with_list: rng.chance(0.7),
        });
    }
    Inputs { make, ops }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "curve.line"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let i = gen_case(rng);
            if ffi_program(&i).is_ok_and(|(m, ops)| ffi::encode(&m, &ops).is_ok())
            {
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
