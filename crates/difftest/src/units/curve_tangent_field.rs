//! Runner for unit `curve.tangent_field`. C++: `solidcore_ffi::units::curve_tangent_field`.
//! Rust: `solidcore::curve::tangent_field`.
//!
//! Every case runs one op of `SmTangentField` (type queries, `Evaluate`, `EvaluatePoint`,
//! `operator==`, `Create`, `ReadFromDB`) on fields built from a UV curve (B-spline or
//! Hermite), a bilinear B-spline surface and start/end tangents. The surface is only
//! reached through unported `SmSurface` members on the Rust side (those cases are
//! deferred), so the Rust side stands in a plain struct holding its corners. Binary
//! streams are never truncated inside the tangent-field members (a short read is
//! undefined upstream, see `plan/findings.md`).
//!
//! `write_read` cases (their own seed stream, named `wrNNNN`) build a real field (data
//! constructor or `Create`, B-spline UV curve, the bilinear surface) on each side, write
//! it with `WriteType` + `WriteToDB` (ASCII or binary, versions 30 and 33), read each
//! side's own bytes back through `SmCurve::ReadFromDB`, and compare the bytes and, on
//! the curve read, the type queries, `operator==` with the original, `EvaluatePoint`
//! and `Evaluate` (1 and 2 derivatives) at sample parameters, `CalculateContinuities`
//! (status, count and end entries; the interior entries are indeterminate upstream,
//! UB-263), `Copy` (a plain `SmCrvOnSurf`) and its evaluation, and the bytes written
//! again. Optionally `GetKnots` caches the breaks first (the record then holds them).
//!
//! Every run uses at least [`MIN_GENERATED`] generated inputs (more with `--seeds`) and
//! [`WRITE_READ_CASES`] `write_read` cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::Curve;
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::curve::tangent_field::{
    self as tf, CurveEnd, TangentDirType, TangentField, TangentFieldCurve,
};
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::surface::surface::Surface;
use solidcore::topology::database_io::{DatabaseReader, DatabaseWriter, FileType};
use solidcore::topology::geometry_write::write_curve;
use solidcore::SolidError;
use solidcore_ffi::units::curve_tangent_field::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Minimum number of generated cases per run.
pub const MIN_GENERATED: usize = 600;

/// The runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg.into())
}

// ---- inputs ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BsplineIn {
    dim: u64,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CurveIn {
    Bspline(BsplineIn),
    Hermite(HermiteIn),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FieldIn {
    curve: CurveIn,
    #[serde(default = "interp")]
    dir: u64,
    start: [f64; 3],
    end: [f64; 3],
    /// `[umin, vmin, umax, vmax]`; `None` = the surface's natural domain.
    #[serde(default)]
    domain: Option<[f64; 4]>,
}

fn interp() -> u64 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EndIn {
    curve: CurveIn,
    param: f64,
    #[serde(default)]
    opposite: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CreateIn {
    tol: f64,
    base3d: CurveIn,
    base_uv: CurveIn,
    start: EndIn,
    end: EndIn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReadIn {
    /// `0` ASCII, `1` binary.
    file_type: u64,
    dim: u64,
    version: u64,
    /// Bytes, or `text` for ASCII.
    #[serde(default)]
    bytes: Vec<u8>,
    #[serde(default)]
    text: Option<String>,
    /// Binary: `dir` then `vals` written as native bytes before `bytes`.
    #[serde(default)]
    dir: Option<u64>,
    #[serde(default)]
    vals: Vec<f64>,
}

impl ReadIn {
    fn data(&self) -> Vec<u8> {
        let mut d = Vec::new();
        if let Some(t) = &self.text {
            d.extend_from_slice(t.as_bytes());
        }
        if let Some(k) = self.dir {
            d.extend_from_slice(&k.to_ne_bytes());
        }
        for v in &self.vals {
            d.extend_from_slice(&v.to_ne_bytes());
        }
        d.extend_from_slice(&self.bytes);
        d
    }
}

/// `write_read`: the field (`field`, or `create` when `create` is set) written and read
/// back through `SmCurve::ReadFromDB`, then queried.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct WriteReadIn {
    /// `0` ASCII, `1` binary.
    file_type: u64,
    version: u64,
    /// `GetKnots` before writing (caches the breaks the record then holds).
    #[serde(default)]
    cache_breaks: bool,
    /// `CalculateContinuities`' angle tolerance.
    angle_tol: f64,
    /// Evaluation parameters.
    params: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    op: String,
    /// Corners `P00 P10 P01 P11` of the bilinear surface (default: unit square, z = 0).
    #[serde(default)]
    surface: Option<[[f64; 3]; 4]>,
    #[serde(default)]
    field: Option<FieldIn>,
    #[serde(default)]
    t: f64,
    #[serde(default)]
    n: usize,
    /// `equal`: bit 0 same surface object, bit 1 B shares A's UV curve, bit 2 A == A.
    #[serde(default)]
    flags: u64,
    #[serde(default)]
    surface_b: Option<[[f64; 3]; 4]>,
    #[serde(default)]
    field_b: Option<FieldIn>,
    #[serde(default)]
    create: Option<CreateIn>,
    #[serde(default)]
    read: Option<ReadIn>,
    #[serde(default)]
    write_read: Option<WriteReadIn>,
}

const UNIT_SQUARE: [[f64; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [1.0, 1.0, 0.0],
];

fn op_of(s: &str) -> Result<Op, HarnessError> {
    Ok(match s {
        "state" => Op::State,
        "eval" => Op::Eval,
        "eval_point" => Op::EvalPoint,
        "equal" => Op::Equal,
        "create" => Op::Create,
        "read_db" => Op::ReadDb,
        "write_read" => Op::WriteRead,
        _ => return Err(harness(format!("unknown op {s}"))),
    })
}

// ---- C++ side -------------------------------------------------------------------------

fn push_curve(v: &mut Vec<f64>, c: &CurveIn) -> Result<(), HarnessError> {
    match c {
        CurveIn::Bspline(b) => {
            let n = b.cpts.len().checked_sub(1).ok_or_else(|| harness("no points"))?;
            let m = b.u.len().checked_sub(1).ok_or_else(|| harness("no knots"))?;
            v.extend([0.0, b.dim as f64, n as f64, b.p as f64, m as f64]);
            for p in &b.cpts {
                v.extend_from_slice(p);
            }
            v.extend_from_slice(&b.u);
        }
        CurveIn::Hermite(h) => {
            v.extend([1.0, h.dim as f64]);
            for p in [h.p1, h.d1, h.p2, h.d2] {
                v.extend_from_slice(&p);
            }
        }
    }
    Ok(())
}

fn push_surface(v: &mut Vec<f64>, s: &[[f64; 3]; 4]) {
    for p in s {
        v.extend_from_slice(p);
    }
}

fn push_field(v: &mut Vec<f64>, f: &FieldIn) -> Result<(), HarnessError> {
    push_curve(v, &f.curve)?;
    v.push(f.dir as f64);
    v.extend_from_slice(&f.start);
    v.extend_from_slice(&f.end);
    match f.domain {
        Some(d) => {
            v.push(1.0);
            v.extend_from_slice(&d);
        }
        None => v.extend([0.0, 0.0, 0.0, 1.0, 1.0]),
    }
    Ok(())
}

fn push_create(v: &mut Vec<f64>, c: &CreateIn) -> Result<(), HarnessError> {
    v.push(c.tol);
    push_curve(v, &c.base3d)?;
    push_curve(v, &c.base_uv)?;
    for e in [&c.start, &c.end] {
        push_curve(v, &e.curve)?;
        v.push(e.param);
        v.push(if e.opposite { 1.0 } else { 0.0 });
    }
    Ok(())
}

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let op = op_of(&i.op)?;
    let mut v = Vec::new();
    let srf = i.surface.unwrap_or(UNIT_SQUARE);
    let field = || i.field.as_ref().ok_or_else(|| harness("missing field"));
    match op {
        Op::ReadDb => {
            let r = i.read.as_ref().ok_or_else(|| harness("missing read"))?;
            let d = r.data();
            v.extend([r.file_type as f64, r.dim as f64, r.version as f64, d.len() as f64]);
            v.extend(d.iter().map(|&b| f64::from(b)));
        }
        Op::Create => {
            let c = i.create.as_ref().ok_or_else(|| harness("missing create"))?;
            push_surface(&mut v, &srf);
            push_create(&mut v, c)?;
        }
        Op::WriteRead => {
            let w = i
                .write_read
                .as_ref()
                .ok_or_else(|| harness("missing write_read"))?;
            push_surface(&mut v, &srf);
            if let Some(c) = &i.create {
                v.push(1.0);
                push_create(&mut v, c)?;
            } else {
                v.push(0.0);
                push_field(&mut v, field()?)?;
            }
            v.extend([
                w.file_type as f64,
                w.version as f64,
                if w.cache_breaks { 1.0 } else { 0.0 },
                w.angle_tol,
                w.params.len() as f64,
            ]);
            v.extend_from_slice(&w.params);
        }
        Op::State | Op::Eval | Op::EvalPoint => {
            push_surface(&mut v, &srf);
            push_field(&mut v, field()?)?;
            if op != Op::State {
                v.push(i.t);
            }
            if op == Op::Eval {
                v.push(i.n as f64);
            }
        }
        Op::Equal => {
            push_surface(&mut v, &srf);
            push_field(&mut v, field()?)?;
            v.push(i.flags as f64);
            if i.flags & 1 == 0 {
                push_surface(&mut v, &i.surface_b.unwrap_or(srf));
            }
            push_field(
                &mut v,
                i.field_b.as_ref().ok_or_else(|| harness("missing field_b"))?,
            )?;
        }
    }
    Ok((op, v))
}

fn to_output(flat: &[f64]) -> Output {
    let status = flat.first().copied().unwrap_or(f64::NAN);
    let rest = flat.get(1..).unwrap_or(&[]).to_vec();
    Output::new()
        .ints("status", vec![status as i64])
        .values("out", rest)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (op, input) = cpp_input(i)?;
    match ffi::eval(op, &input) {
        Ok(flat) => Ok(to_output(&flat)),
        Err(e) => Err(harness(e.to_string())),
    }
}

// ---- Rust side --------------------------------------------------------------------------

/// The surface the shim builds from four corners (`read_surface`): a bilinear
/// `SmBSplineSurface` on `[0, 1]^2` with unit weights, corners in the order
/// `(u0,v0) (u1,v0) (u0,v1) (u1,v1)`.
fn stand_in(c: [[f64; 3]; 4]) -> Result<BSplineSurface, HarnessError> {
    let [c00, c10, c01, c11] = c.map(|[x, y, z]| CPoint::new(x, y, z, 1.0));
    let net = vec![vec![c00, c01], vec![c10, c11]];
    let kn = vec![0.0, 0.0, 1.0, 1.0];
    let sur = NlSurface::new(net, 1, 1, kn.clone(), kn)
        .map_err(|e| harness(format!("surface: {e}")))?;
    Ok(BSplineSurface::from_nurb(sur, false))
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::from_array(p)
}

fn build(c: &CurveIn) -> Result<Curve, HarnessError> {
    match c {
        CurveIn::Bspline(b) => {
            let n = b.cpts.len().checked_sub(1).ok_or_else(|| harness("no points"))?;
            let m = b.u.len().checked_sub(1).ok_or_else(|| harness("no knots"))?;
            let mut cur =
                allocate_nurb_curve(n, b.p, m).map_err(|e| harness(format!("curve: {e}")))?;
            for (d, s) in cur.cpts_mut().iter_mut().zip(&b.cpts) {
                d.x = s[0];
                d.y = s[1];
                d.z = s[2];
                d.w = s[3];
            }
            cur.knots_mut().copy_from_slice(&b.u);
            Ok(Curve::from(BSplineCurve::from_nurb(cur, b.dim, false)))
        }
        CurveIn::Hermite(h) => Ok(Curve::from(HermiteCurve::new(
            v3(h.p1),
            v3(h.d1),
            v3(h.p2),
            v3(h.d2),
            h.dim,
        ))),
    }
}

fn dir_type(k: u64) -> Result<TangentDirType, HarnessError> {
    Ok(match k {
        0 => TangentDirType::Constant,
        1 => TangentDirType::Interpolate,
        2 => TangentDirType::Perpendicular,
        _ => return Err(harness("bad dir type")),
    })
}

fn domain(d: Option<[f64; 4]>) -> Result<Option<Extent2d>, HarnessError> {
    d.map(|[u0, v0, u1, v1]| {
        let mut e = Extent2d::new();
        e.set_min_max(Point2::new(u0, v0), Point2::new(u1, v1))
            .map_err(|e| harness(format!("domain: {e}")))?;
        Ok(e)
    })
    .transpose()
}

/// The `SmStatus` of a Rust error; `1007` for `InvalidInput`, `-1` for one upstream
/// cannot return.
fn status(e: &SolidError) -> i64 {
    match e {
        SolidError::InvalidInput { .. } => 1007,
        SolidError::Upstream { code, .. } => i64::from(*code),
        _ => -1,
    }
}

fn fail(e: &SolidError) -> Output {
    Output::unported(e).unwrap_or_else(|| to_output(&[status(e) as f64]))
}

fn push3(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

const KINDS: [i64; 8] = [14001, 14048, 14050, 14033, 14034, 14010, 14000, 0];

fn push_string(o: &mut Vec<f64>, s: &str) {
    o.push(s.len() as f64);
    o.extend(s.bytes().map(f64::from));
}

fn rust(k: &solidcore::kernel::Kernel, i: &Inputs) -> Result<Output, HarnessError> {
    let op = op_of(&i.op)?;
    let srf = stand_in(i.surface.unwrap_or(UNIT_SQUARE))?;
    if op == Op::ReadDb {
        let r = i.read.as_ref().ok_or_else(|| harness("missing read"))?;
        let ft = if r.file_type == 0 {
            FileType::Ascii
        } else {
            FileType::Binary
        };
        let mut db = DatabaseReader::new();
        db.open_for_read(r.data(), ft);
        return Ok(match tf::read_from_db(&solidcore::kernel::Kernel::default(), &mut db, r.dim, r.version) {
            Ok(_) => to_output(&[0.0]),
            Err(e) => fail(&e),
        });
    }
    if op == Op::WriteRead {
        return rust_write_read(k, i, srf);
    }
    if op == Op::Create {
        let c = i.create.as_ref().ok_or_else(|| harness("missing create"))?;
        let base3d = build(&c.base3d)?;
        let base_uv = build(&c.base_uv)?;
        let sc = build(&c.start.curve)?;
        let ec = build(&c.end.curve)?;
        let start = CurveEnd {
            curve: &sc,
            param: c.start.param,
            opposite: c.start.opposite,
        };
        let end = CurveEnd {
            curve: &ec,
            param: c.end.param,
            opposite: c.end.opposite,
        };
        return Ok(
            match TangentField::create(k, c.tol, &base3d, &base_uv, &srf, start, end) {
                Ok(f) => {
                    let ivl = f.natural_interval();
                    let mut o = vec![0.0];
                    let a = f.evaluate_point(k, ivl.min());
                    let b = f.evaluate_point(k, ivl.max());
                    match (a, b) {
                        (Ok(a), Ok(b)) => {
                            push3(&mut o, a);
                            push3(&mut o, b);
                            to_output(&o)
                        }
                        (Err(e), _) | (_, Err(e)) => fail(&e),
                    }
                }
                Err(e) => fail(&e),
            },
        );
    }

    let fa = i.field.as_ref().ok_or_else(|| harness("missing field"))?;
    let uv_a = build(&fa.curve)?;
    let a = match TangentField::new(
        &uv_a,
        &srf,
        dir_type(fa.dir)?,
        v3(fa.start),
        v3(fa.end),
        domain(fa.domain)?,
    ) {
        Ok(a) => a,
        Err(e) => return Ok(fail(&e)),
    };
    let mut o = vec![0.0];
    match op {
        Op::State => {
            o.push(a.type_code() as f64);
            for k in KINDS {
                o.push(if a.is_kind_of(k) { 1.0 } else { 0.0 });
            }
            let ivl = a.natural_interval();
            o.extend([ivl.min(), ivl.max()]);
            push_string(&mut o, a.type_string());
            push_string(&mut o, a.class_string());
        }
        Op::Eval => match a.evaluate(k, i.t, i.n) {
            Ok(v) => {
                for p in v {
                    push3(&mut o, p);
                }
            }
            Err(e) => return Ok(fail(&e)),
        },
        Op::EvalPoint => match a.evaluate_point(k, i.t) {
            Ok(p) => push3(&mut o, p),
            Err(e) => return Ok(fail(&e)),
        },
        Op::Equal => {
            let fb = i.field_b.as_ref().ok_or_else(|| harness("missing field_b"))?;
            let srf_b = stand_in(i.surface_b.or(i.surface).unwrap_or(UNIT_SQUARE))?;
            let srf_b_ref = if i.flags & 1 != 0 { &srf } else { &srf_b };
            let uv_b_own = build(&fb.curve)?;
            let uv_b = if i.flags & 2 != 0 { &uv_a } else { &uv_b_own };
            let b = match TangentField::new(
                uv_b,
                srf_b_ref,
                dir_type(fb.dir)?,
                v3(fb.start),
                v3(fb.end),
                domain(fb.domain)?,
            ) {
                Ok(b) => b,
                Err(e) => return Ok(fail(&e)),
            };
            let eq = if i.flags & 4 != 0 {
                a.approx_eq(k, &a)
            } else {
                a.approx_eq(k, &b)
            };
            match eq {
                Ok(eq) => o.push(if eq { 1.0 } else { 0.0 }),
                Err(e) => return Ok(fail(&e)),
            }
        }
        Op::Create | Op::ReadDb | Op::WriteRead => {}
    }
    Ok(to_output(&o))
}

/// An error of a step whose status is compared: its `SmStatus`, or the deferral of an
/// unported step.
fn step_status(e: &SolidError) -> Result<f64, Box<Output>> {
    match Output::unported(e) {
        Some(d) => Err(Box::new(d)),
        None => Ok(status(e) as f64),
    }
}

fn push_bytes(o: &mut Vec<f64>, b: &[u8]) {
    o.push(b.len() as f64);
    o.extend(b.iter().map(|&x| f64::from(x)));
}

/// `WriteType` + `WriteToDB` of `c` (as the shim's `write_curve`).
fn write_record(c: &Curve, ft: FileType, version: u64) -> solidcore::Result<Vec<u8>> {
    let mut w = DatabaseWriter::open_for_write(ft);
    w.write_type(c.type_code(), Some(c.dim()));
    write_curve(c, &mut w, version)?;
    Ok(w.into_bytes())
}

fn rust_write_read(
    k: &solidcore::kernel::Kernel,
    i: &Inputs,
    srf: BSplineSurface,
) -> Result<Output, HarnessError> {
    match write_read_outputs(k, i, srf)? {
        Ok(o) => Ok(to_output(&o)),
        Err(deferred) => Ok(*deferred),
    }
}

fn write_read_outputs(
    k: &solidcore::kernel::Kernel,
    i: &Inputs,
    srf: BSplineSurface,
) -> Result<Result<Vec<f64>, Box<Output>>, HarnessError> {
    let w = i
        .write_read
        .as_ref()
        .ok_or_else(|| harness("missing write_read"))?;
    let ft = if w.file_type == 0 {
        FileType::Ascii
    } else {
        FileType::Binary
    };
    let srf = Surface::from(srf);
    let made = if let Some(c) = &i.create {
        let base3d = build(&c.base3d)?;
        let base_uv = build(&c.base_uv)?;
        let sc = build(&c.start.curve)?;
        let ec = build(&c.end.curve)?;
        let start = CurveEnd {
            curve: &sc,
            param: c.start.param,
            opposite: c.start.opposite,
        };
        let end = CurveEnd {
            curve: &ec,
            param: c.end.param,
            opposite: c.end.opposite,
        };
        TangentFieldCurve::create(k, c.tol, &base3d, base_uv, srf, start, end)
    } else {
        let f = i.field.as_ref().ok_or_else(|| harness("missing field"))?;
        TangentFieldCurve::new(
            k,
            build(&f.curve)?,
            srf,
            dir_type(f.dir)?,
            v3(f.start),
            v3(f.end),
            domain(f.domain)?,
        )
    };
    let mut o = Vec::new();
    let tf = match made {
        Ok(tf) => Curve::from(tf),
        Err(e) => {
            match step_status(&e) {
                Ok(s) => o.push(s),
                Err(d) => return Ok(Err(d)),
            }
            return Ok(Ok(o));
        }
    };
    o.push(0.0);
    macro_rules! status_of {
        ($r:expr) => {
            match $r {
                Ok(v) => {
                    o.push(0.0);
                    Some(v)
                }
                Err(e) => match step_status(&e) {
                    Ok(s) => {
                        o.push(s);
                        None
                    }
                    Err(d) => return Ok(Err(d)),
                },
            }
        };
    }
    if w.cache_breaks {
        if let Some((knots, _)) = status_of!(tf.knots(k, None)) {
            o.push(knots.len() as f64);
            o.extend(knots);
        }
    }
    let Some(bytes) = status_of!(write_record(&tf, ft, w.version)) else {
        return Ok(Ok(o));
    };
    push_bytes(&mut o, &bytes);
    let mut db = DatabaseReader::new();
    db.open_for_read(bytes, ft);
    let read = db.read_type(true).and_then(|h| {
        let (Some(t), Some(d)) = (h.type_code, h.dim) else {
            return Err(SolidError::invalid_input("difftest", "type header"));
        };
        Curve::read_from_db(k, t, &mut db, d, w.version)
    });
    let Some(c) = status_of!(read) else {
        return Ok(Ok(o));
    };
    o.push(c.type_code() as f64);
    for kind in KINDS {
        o.push(if c.is_kind_of(kind) { 1.0 } else { 0.0 });
    }
    let ivl = c.natural_interval();
    o.extend([ivl.min(), ivl.max()]);
    let eq = match c.approx_eq(k, &tf) {
        Ok(eq) => eq,
        Err(e) => return Ok(Err(Box::new(fail(&e)))),
    };
    o.push(if eq { 1.0 } else { 0.0 });
    for &t in &w.params {
        if let Some(p) = status_of!(c.evaluate_point(k, t)) {
            push3(&mut o, p);
        }
        if let Some(pd) = status_of!(c.evaluate(k, t, 1, true, true)) {
            for j in 0..2 {
                push3(&mut o, pd.get(j).copied().unwrap_or(Vec3::UNDEF));
            }
        }
        status_of!(c.evaluate(k, t, 2, true, true));
    }
    if let Some((min, conts)) = status_of!(c.calculate_continuities(k, w.angle_tol)) {
        // the interior entries and the minimum are indeterminate upstream (UB-263)
        o.push(conts.len() as f64);
        if let (Some(&first), Some(&last)) = (conts.first(), conts.last()) {
            o.extend([f64::from(first as i32), f64::from(last as i32)]);
        }
        if conts.len() < 3 {
            o.push(f64::from(min as i32));
        }
    }
    if let Some(cp) = status_of!(c.copy(k)) {
        o.push(cp.type_code() as f64);
        for &t in &w.params {
            if let Some(p) = status_of!(cp.evaluate_point(k, t)) {
                push3(&mut o, p);
            }
        }
    }
    if let Some(again) = status_of!(write_record(&c, ft, w.version)) {
        push_bytes(&mut o, &again);
    }
    Ok(Ok(o))
}

// ---- generation -------------------------------------------------------------------------

fn gen_bspline(rng: &mut Rng, dim: u64, a: f64, b: f64) -> CurveIn {
    let p = 1 + rng.below(3) as usize;
    let n = p + rng.below(4) as usize;
    let m = n + p + 1;
    let mut inner: Vec<f64> = (0..m + 1 - 2 * (p + 1))
        .map(|_| a + (b - a) * rng.uniform(0.05, 0.95))
        .collect();
    inner.sort_by(f64::total_cmp);
    let mut u = vec![a; p + 1];
    u.extend(inner);
    u.extend(vec![b; p + 1]);
    let cpts = (0..=n)
        .map(|_| {
            let [x, y, z] = rng.point3(-2.0, 2.0);
            if dim == 2 {
                [x, y, NOZ, NOW]
            } else {
                [x, y, z, NOW]
            }
        })
        .collect();
    CurveIn::Bspline(BsplineIn { dim, cpts, p, u })
}

fn gen_hermite(rng: &mut Rng, dim: u64) -> CurveIn {
    let mut v = || {
        let [x, y, z] = rng.point3(-2.0, 2.0);
        [x, y, if dim == 2 { 0.0 } else { z }]
    };
    CurveIn::Hermite(HermiteIn {
        dim,
        p1: v(),
        d1: v(),
        p2: v(),
        d2: v(),
    })
}

fn range_of(c: &CurveIn) -> (f64, f64) {
    match c {
        CurveIn::Bspline(b) => (
            b.u.first().copied().unwrap_or(0.0),
            b.u.last().copied().unwrap_or(1.0),
        ),
        CurveIn::Hermite(_) => (0.0, 1.0),
    }
}

fn gen_uv(rng: &mut Rng) -> CurveIn {
    if rng.chance(0.4) {
        gen_hermite(rng, 2)
    } else {
        let a = if rng.chance(0.5) { 0.0 } else { rng.uniform(-5.0, 5.0) };
        let len = if rng.chance(0.5) { 1.0 } else { rng.uniform(0.5, 10.0) };
        gen_bspline(rng, 2, a, a + len)
    }
}

fn gen_vec(rng: &mut Rng) -> [f64; 3] {
    match rng.below(6) {
        0 => [0.0, 0.0, 0.0],
        1 => rng.point3(-1e3, 1e3),
        _ => rng.point3(-3.0, 3.0),
    }
}

fn gen_field(rng: &mut Rng) -> FieldIn {
    let domain = if rng.chance(0.8) {
        let u0 = rng.uniform(-1.0, 0.5);
        let v0 = rng.uniform(-1.0, 0.5);
        Some([u0, v0, u0 + rng.uniform(0.0, 2.0), v0 + rng.uniform(0.0, 2.0)])
    } else {
        None
    };
    FieldIn {
        curve: gen_uv(rng),
        dir: if rng.chance(0.8) { 1 } else { rng.below(3) },
        start: gen_vec(rng),
        end: gen_vec(rng),
        domain,
    }
}

/// A parameter near the end bands of `[a, b]` (the `1e-8 + EFF_ZERO` thresholds),
/// at the ends, outside, or inside.
fn gen_t(rng: &mut Rng, a: f64, b: f64) -> f64 {
    let k = rng.below(30) as f64 - 5.0;
    match rng.below(8) {
        0 => a,
        1 => b,
        2 => a + k * 1e-9,
        3 => b - k * 1e-9,
        4 => a - rng.uniform(0.0, 0.5) * (b - a),
        5 => b + rng.uniform(0.0, 0.5) * (b - a),
        6 => a + 1e-8 + rng.uniform(-2e-12, 2e-12),
        _ => a + (b - a) * rng.unit(),
    }
}

fn gen_surface(rng: &mut Rng) -> [[f64; 3]; 4] {
    if rng.chance(0.5) {
        UNIT_SQUARE
    } else {
        [
            rng.point3(-2.0, 2.0),
            rng.point3(-2.0, 2.0),
            rng.point3(-2.0, 2.0),
            rng.point3(-2.0, 2.0),
        ]
    }
}

/// A variant of `f` for equality tests.
fn variant(rng: &mut Rng, f: &FieldIn) -> FieldIn {
    let mut g = f.clone();
    match rng.below(7) {
        0 => {}
        1 => {
            // below / above the scaled zero of the start tangent
            let s = 1e-12 * (1.0 + g.start.iter().fold(0.0_f64, |m, x| m.max(x.abs())));
            let f = if rng.chance(0.5) { 0.3 } else { 3.0 };
            if let Some(x) = g.start.first_mut() {
                *x += s * f;
            }
        }
        2 => {
            if let Some(x) = g.end.get_mut(2) {
                *x += rng.uniform(-1e-3, 1e-3);
            }
        }
        3 => g.dir = (g.dir + 1) % 3,
        4 => {
            if let Some(d) = g.domain.as_mut() {
                if let Some(x) = d.get_mut(2) {
                    *x += if rng.chance(0.5) { 1e-14 } else { 1e-6 };
                }
            }
        }
        5 => g.curve = gen_uv(rng),
        _ => g.domain = None,
    }
    g
}

/// A 3D curve through `p` at parameter 0 (and a random end).
fn curve_from(rng: &mut Rng, p: [f64; 3]) -> CurveIn {
    let mut d = || rng.point3(-2.0, 2.0);
    CurveIn::Hermite(HermiteIn {
        dim: 3,
        p1: p,
        d1: d(),
        p2: d(),
        d2: d(),
    })
}

fn offset(rng: &mut Rng, p: [f64; 3], dist: f64) -> [f64; 3] {
    let [x, y, z] = rng.point3(-1.0, 1.0);
    let l = (x * x + y * y + z * z).sqrt().max(1e-3);
    [
        p[0] + x / l * dist,
        p[1] + y / l * dist,
        p[2] + z / l * dist,
    ]
}

fn gen_create(rng: &mut Rng) -> CreateIn {
    let tol = if rng.chance(0.5) {
        2.0 * 1e-3
    } else {
        rng.uniform(1e-7, 1e-2)
    };
    let (p0, p1, base3d) = {
        let h = HermiteIn {
            dim: 3,
            p1: rng.point3(-2.0, 2.0),
            d1: rng.point3(-2.0, 2.0),
            p2: rng.point3(-2.0, 2.0),
            d2: rng.point3(-2.0, 2.0),
        };
        (h.p1, h.p2, CurveIn::Hermite(h))
    };
    let factors = [0.0, 0.0, 0.5, 0.999, 1.001, 2.0, 1e3];
    let end_in = |rng: &mut Rng, p: [f64; 3]| {
        let f = *rng.choose(&factors).unwrap_or(&0.0);
        let q = offset(rng, p, f * tol);
        EndIn {
            curve: curve_from(rng, q),
            param: 0.0,
            opposite: rng.chance(0.5),
        }
    };
    let start = end_in(rng, p0);
    let end = end_in(rng, p1);
    let base_uv = gen_bspline(rng, 2, 0.0, 1.0);
    CreateIn {
        tol,
        base3d,
        base_uv,
        start,
        end,
    }
}

fn generate(rng: &mut Rng) -> Inputs {
    let mut i = Inputs {
        op: String::new(),
        surface: Some(gen_surface(rng)),
        field: None,
        t: 0.0,
        n: 0,
        flags: 0,
        surface_b: None,
        field_b: None,
        create: None,
        read: None,
        write_read: None,
    };
    match rng.below(10) {
        0 => {
            i.op = "state".into();
            i.field = Some(gen_field(rng));
        }
        1..=3 => {
            i.op = "eval_point".into();
            let f = gen_field(rng);
            let (a, b) = range_of(&f.curve);
            i.t = gen_t(rng, a, b);
            i.field = Some(f);
        }
        4..=6 => {
            i.op = "eval".into();
            let f = gen_field(rng);
            let (a, b) = range_of(&f.curve);
            i.t = gen_t(rng, a, b);
            i.n = if rng.chance(0.15) {
                2 + rng.below(2) as usize
            } else {
                rng.below(2) as usize
            };
            i.field = Some(f);
        }
        7 | 8 => {
            i.op = "equal".into();
            let f = gen_field(rng);
            i.field_b = Some(variant(rng, &f));
            i.field = Some(f);
            i.flags = if rng.chance(0.8) {
                1 | (rng.below(2) << 1)
            } else {
                rng.below(8)
            };
            if i.flags & 1 == 0 {
                i.surface_b = i.surface;
            }
        }
        _ => {
            i.op = "create".into();
            i.create = Some(gen_create(rng));
        }
    }
    i
}

/// Generated `write_read` cases (their own seed stream, `<unit>:write_read`).
pub const WRITE_READ_CASES: usize = 240;

/// A `write_read` case: a field built from data (B-spline UV curves: the Rust writer has
/// no `SmHermiteCurve::WriteToDB`) or with `Create`, written in either format.
fn generate_write_read(rng: &mut Rng) -> Option<Value> {
    let surface = Some(gen_surface(rng));
    let (field, create, (a, b)) = if rng.chance(0.25) {
        let c = gen_create(rng);
        let r = range_of(&c.base_uv);
        (None, Some(c), r)
    } else {
        let mut f = gen_field(rng);
        let a = if rng.chance(0.5) { 0.0 } else { rng.uniform(-5.0, 5.0) };
        let len = match rng.below(6) {
            0 => 1e-3,
            1 => 1e3,
            _ if rng.chance(0.5) => 1.0,
            _ => rng.uniform(0.5, 10.0),
        };
        f.curve = gen_bspline(rng, 2, a, a + len);
        if rng.chance(0.1) {
            // huge and tiny tangents
            let s = if rng.chance(0.5) { 1e-9 } else { 1e2 };
            f.start = f.start.map(|x| x * s);
            f.end = f.end.map(|x| x * s);
        }
        let r = range_of(&f.curve);
        (Some(f), None, r)
    };
    let np = 1 + rng.below(4) as usize;
    let params = (0..np).map(|_| gen_t(rng, a, b)).collect();
    let i = Inputs {
        op: "write_read".into(),
        surface,
        field,
        t: 0.0,
        n: 0,
        flags: 0,
        surface_b: None,
        field_b: None,
        create,
        read: None,
        write_read: Some(WriteReadIn {
            file_type: u64::from(!rng.chance(0.3)),
            version: if rng.chance(0.3) { 33 } else { 30 },
            cache_breaks: rng.chance(0.3),
            angle_tol: if rng.chance(0.5) {
                0.01
            } else {
                rng.uniform(1e-6, 0.5)
            },
            params,
        }),
    };
    serde_json::to_value(i).ok()
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "curve.tangent_field"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(generate(rng)).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let main = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        let mut v = default_cases(self, &main)?;
        // write/read round trips: their own seed stream, so the other generated cases
        // are unchanged; named `wrNNNN`.
        let wr = CaseContext {
            seeds: ctx.seeds.max(WRITE_READ_CASES),
            ..ctx.clone()
        };
        for mut c in wr.generated_cases("curve.tangent_field:write_read", generate_write_read) {
            c.name = c.name.replacen("gen", "wr", 1);
            c.unit = self.id().to_string();
            v.push(c);
        }
        Ok(v)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let k = &solidcore::Kernel::default();
        rust(k, &case.parse()?)
    }
}
