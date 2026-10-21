//! Runner for unit `curve.projected_curve`: `SmProjectedCurve`, a parallel,
//! perspective or rotation projection of a B-spline or Hermite base curve.
//!
//! C++: `solidcore_ffi::units::curve_projected_curve` (upstream
//! `source/SMLib/src/SmProjectedCurve.cpp`, called through `SmCurve*` where the method
//! is virtual). Rust: `solidcore::curve::projected_curve` (through
//! `solidcore::curve::curve::Curve` for the same methods).
//!
//! A case is a projected curve `pc` (`curve`: `{"bspline": {dim, oob, cpts, p, u}}`
//! with control points as stored `[x, y, z, w]`, or `{"hermite": {dim, p1, d1, p2,
//! d2}}`; `ctor` 0 = constructor, 1 = `SetCanonical`; optional `point`, `vec`, `aux`;
//! `ptype` the `SmProjectionType`; `owns` the ownership `SmBoolean`) and an `op`.
//! `read_db` has no `pc`: its stream is written by the runner (both sides read the
//! same bytes) from the projection data and a base curve, in ASCII or binary, for a
//! database version. Both sides produce the flat record documented in the ffi
//! module; the runner reports its first entry as the `status` int (`0` =
//! `SM_SUCCESS`, else the `SmStatus`; `1007` for a Rust `InvalidInput`, `-1` for a
//! Rust error upstream cannot produce) and the rest as the `out` values.
//!
//! `Copy` reports whether the copy shares the base curve (upstream: the pointer is
//! shared exactly when it is not owned; the port always copies it, so the Rust side
//! reports "not owned").

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p2::{Curve, CurveKind};
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::curve::projected_curve::{OwnsCurve, ProjectedCurve};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, ProjectionType};
use solidcore::nurbs::b_spline_curve_n_lib::TrimMode;
use solidcore::nurbs::math::{NOW, NOZ};
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::topology::database_io::{DatabaseReader, FileType};
use solidcore::SolidError;
use solidcore_ffi::units::curve_projected_curve::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2500;

/// `curve.projected_curve` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// `SmCurve_TYPE`, `SmProjectedCurve_TYPE`, `SmBSplineCurve_TYPE`,
/// `SmOffsetCurve_TYPE`, two base types and an unrelated one (`IsKindOf` probes).
const KINDS: [i64; 7] = [14001, 14037, 14033, 14036, 12060, 12000, 1];

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CurveIn {
    Bspline(BSplineIn),
    Hermite(HermiteIn),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PcIn {
    curve: CurveIn,
    #[serde(default)]
    ctor: u8,
    #[serde(default)]
    point: Option<[f64; 3]>,
    #[serde(default)]
    vec: Option<[f64; 3]>,
    #[serde(default)]
    aux: Option<[f64; 3]>,
    ptype: u8,
    #[serde(default)]
    owns: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DbIn {
    version: u64,
    binary: bool,
    point: [f64; 3],
    vec: [f64; 3],
    aux: [f64; 3],
    ptype: u64,
    /// Type code written for the base curve (default: its own).
    #[serde(default)]
    base_type: Option<i64>,
    curve: CurveIn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    State,
    MakeOrtho,
    Eval {
        t: f64,
        n: usize,
        left: bool,
        nonzero: bool,
    },
    EvalPoint {
        t: f64,
    },
    Knots {
        ivl: Option<[f64; 2]>,
    },
    Continuities {
        angle: f64,
    },
    EditParam {
        a: f64,
        b: f64,
    },
    Reverse {
        a: f64,
        b: f64,
    },
    Trim {
        a: f64,
        b: f64,
        mode: u8,
    },
    Transform {
        o: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        scale: Option<[f64; 3]>,
    },
    Copy,
    Equal {
        other: Option<PcIn>,
    },
    Ends,
    ReadDb(DbIn),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default)]
    pc: Option<PcIn>,
    #[serde(flatten)]
    op: OpIn,
}

// ---- database streams -------------------------------------------------------------

fn bspline_text(b: &BSplineIn) -> String {
    let n = b.cpts.len().saturating_sub(1);
    let rat = b.cpts.first().is_some_and(|c| c[3] != NOW);
    let two_d = b.cpts.first().is_some_and(|c| c[2] == NOZ);
    let mut s = format!(
        "{}\n{}\n{}\n{}\n1\n6\n",
        n,
        b.p,
        i32::from(rat),
        if two_d { 2 } else { 3 }
    );
    for c in &b.cpts {
        let mut vals = vec![c[0], c[1]];
        if !two_d {
            vals.push(c[2]);
        }
        if rat {
            vals.push(c[3]);
        }
        let t: Vec<String> = vals.iter().map(|x| format!("{x:.17e}")).collect();
        s.push_str(&t.join(" "));
        s.push('\n');
    }
    for k in &b.u {
        let _ = writeln!(s, "{k:.17e}");
    }
    s
}

fn bspline_bin(o: &mut Vec<u8>, b: &BSplineIn) {
    let n = b.cpts.len().saturating_sub(1) as i64;
    let rat = b.cpts.first().is_some_and(|c| c[3] != NOW);
    let two_d = b.cpts.first().is_some_and(|c| c[2] == NOZ);
    o.extend(n.to_ne_bytes());
    o.extend((b.p as i16).to_ne_bytes());
    o.extend(i16::from(rat).to_ne_bytes());
    o.extend((if two_d { 2i16 } else { 3 }).to_ne_bytes());
    o.extend(1i64.to_ne_bytes());
    o.extend(6i64.to_ne_bytes());
    for c in &b.cpts {
        let mut vals = vec![c[0], c[1]];
        if !two_d {
            vals.push(c[2]);
        }
        if rat {
            vals.push(c[3]);
        }
        for v in vals {
            o.extend(v.to_ne_bytes());
        }
    }
    for k in &b.u {
        o.extend(k.to_ne_bytes());
    }
}

/// `SmHermiteCurve::WriteToDB` data: P1 D1 P2 D2 and the power basis C D.
fn hermite_vectors(h: &HermiteIn) -> [Vec3; 6] {
    let c = HermiteCurve::new(v3(h.p1), v3(h.d1), v3(h.p2), v3(h.d2), 3);
    let (pc, pd) = c.power_basis();
    [v3(h.p1), v3(h.d1), v3(h.p2), v3(h.d2), pc, pd]
}

/// The stream `SmProjectedCurve::WriteToDB` writes for `d` (the base curve as written
/// by its own writer).
fn db_bytes(d: &DbIn) -> Vec<u8> {
    let with_aux = d.version > 34;
    let (own_type, dim) = match &d.curve {
        CurveIn::Bspline(b) => (14033, b.dim),
        CurveIn::Hermite(h) => (14034, h.dim),
    };
    let base_type = d.base_type.unwrap_or(own_type);
    if d.binary {
        let mut o = Vec::new();
        let mut vs = vec![d.point, d.vec];
        if with_aux {
            vs.push(d.aux);
        }
        for v in vs {
            for x in v {
                o.extend(x.to_ne_bytes());
            }
        }
        o.extend(d.ptype.to_ne_bytes());
        o.extend(base_type.to_ne_bytes());
        o.extend(dim.to_ne_bytes());
        match &d.curve {
            CurveIn::Bspline(b) => bspline_bin(&mut o, b),
            CurveIn::Hermite(h) => {
                for v in hermite_vectors(h) {
                    for x in [v.x, v.y, v.z] {
                        o.extend(x.to_ne_bytes());
                    }
                }
            }
        }
        o
    } else {
        let mut s = String::new();
        let mut vs = vec![d.point, d.vec];
        if with_aux {
            vs.push(d.aux);
        }
        for v in vs {
            let _ = writeln!(s, "{:.17e} {:.17e} {:.17e} SmProjectedCurve data", v[0], v[1], v[2]);
        }
        let _ = writeln!(s, "{} SmProjectedCurve Projection Type", d.ptype);
        s.push_str(" ProjectedCurve->ProjectedCurve \n");
        let _ = writeln!(s, "Object Type {base_type}");
        let _ = writeln!(s, "Curve Dim {dim}");
        match &d.curve {
            CurveIn::Bspline(b) => s.push_str(&bspline_text(b)),
            CurveIn::Hermite(h) => {
                for v in hermite_vectors(h) {
                    let _ = writeln!(s, "{:.17e} {:.17e} {:.17e}", v.x, v.y, v.z);
                }
            }
        }
        s.into_bytes()
    }
}

// ---- C++ input --------------------------------------------------------------------

fn push3(v: &mut Vec<f64>, p: [f64; 3]) {
    v.extend_from_slice(&p);
}

fn b(x: bool) -> f64 {
    f64::from(u8::from(x))
}

fn curve_block(v: &mut Vec<f64>, c: &CurveIn) {
    match c {
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
                push3(v, p);
            }
        }
    }
}

fn pc_block(v: &mut Vec<f64>, pc: &PcIn) {
    curve_block(v, &pc.curve);
    v.push(f64::from(pc.ctor));
    for x in [pc.point, pc.vec, pc.aux] {
        v.push(b(x.is_some()));
        push3(v, x.unwrap_or([0.0; 3]));
    }
    v.extend([f64::from(pc.ptype), f64::from(pc.owns)]);
}

fn op_of(op: &OpIn) -> Op {
    match op {
        OpIn::State => Op::State,
        OpIn::MakeOrtho => Op::MakeOrtho,
        OpIn::Eval { .. } => Op::Eval,
        OpIn::EvalPoint { .. } => Op::EvalPoint,
        OpIn::Knots { .. } => Op::Knots,
        OpIn::Continuities { .. } => Op::Continuities,
        OpIn::EditParam { .. } => Op::EditParam,
        OpIn::Reverse { .. } => Op::Reverse,
        OpIn::Trim { .. } => Op::Trim,
        OpIn::Transform { .. } => Op::Transform,
        OpIn::Copy => Op::Copy,
        OpIn::Equal { .. } => Op::Equal,
        OpIn::Ends => Op::Ends,
        OpIn::ReadDb(_) => Op::ReadDb,
    }
}

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let op = op_of(&i.op);
    let mut v = Vec::new();
    if let OpIn::ReadDb(d) = &i.op {
        // `(SmProjectionType)lValue` beyond the enum's range of values (0..=3) is
        // undefined behaviour upstream (the port reads it as `SM_PT_UNKNOWN`).
        if d.ptype > 3 {
            return Err(harness(
                "projection type outside SmProjectionType (undefined upstream)",
            ));
        }
        let bytes = db_bytes(d);
        v.extend([d.version as f64, b(d.binary), bytes.len() as f64]);
        v.extend(bytes.iter().map(|&x| f64::from(x)));
        return Ok((op, v));
    }
    let pc = i.pc.as_ref().ok_or_else(|| harness("curve op without pc"))?;
    pc_block(&mut v, pc);
    match &i.op {
        OpIn::State | OpIn::MakeOrtho | OpIn::Copy | OpIn::Ends | OpIn::ReadDb(_) => {}
        OpIn::Eval {
            t,
            n,
            left,
            nonzero,
        } => v.extend([*t, *n as f64, b(*left), b(*nonzero)]),
        OpIn::EvalPoint { t } => v.push(*t),
        OpIn::Knots { ivl } => {
            let [a, bb] = ivl.unwrap_or([0.0, 0.0]);
            v.extend([b(ivl.is_some()), a, bb]);
        }
        OpIn::Continuities { angle } => v.push(*angle),
        OpIn::EditParam { a, b: bb } | OpIn::Reverse { a, b: bb } => v.extend([*a, *bb]),
        OpIn::Trim { a, b: bb, mode } => v.extend([*a, *bb, f64::from(*mode)]),
        OpIn::Transform { o, x, y, scale } => {
            push3(&mut v, *o);
            push3(&mut v, *x);
            push3(&mut v, *y);
            v.push(b(scale.is_some()));
            push3(&mut v, scale.unwrap_or([0.0; 3]));
        }
        OpIn::Equal { other } => {
            v.push(b(other.is_none()));
            if let Some(o) = other {
                pc_block(&mut v, o);
            }
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

// ---- Rust side --------------------------------------------------------------------

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::from_array(p)
}

fn build_curve(c: &CurveIn) -> Result<Curve, HarnessError> {
    match c {
        CurveIn::Bspline(bs) => {
            let n = bs.cpts.len().checked_sub(1).ok_or_else(|| harness("no points"))?;
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
    }
}

fn ptype_of(t: u64) -> ProjectionType {
    match t {
        0 => ProjectionType::Parallel,
        1 => ProjectionType::Perspective,
        2 => ProjectionType::Rotation,
        _ => ProjectionType::Unknown,
    }
}

fn owns_of(o: u8) -> OwnsCurve {
    match o {
        1 => OwnsCurve::Yes,
        2 => OwnsCurve::Unsure,
        _ => OwnsCurve::No,
    }
}

/// The projected curve of `pc` and the status of its construction.
fn build(pc: &PcIn) -> Result<(ProjectedCurve, f64), HarnessError> {
    let base = build_curve(&pc.curve)?;
    let ptype = ptype_of(u64::from(pc.ptype));
    let owns = owns_of(pc.owns);
    if pc.ctor == 0 {
        Ok((
            ProjectedCurve::new(
                base,
                pc.point.map(v3),
                pc.vec.map(v3),
                pc.aux.map(v3),
                ptype,
                owns,
            ),
            0.0,
        ))
    } else {
        // an empty curve (`SmProjectedCurve()`), then SetCanonical
        let mut c = ProjectedCurve::new(
            base.clone(),
            None,
            None,
            None,
            ProjectionType::Unknown,
            OwnsCurve::No,
        );
        let undef = Vec3::UNDEF;
        let st = match c.set_canonical(
            base,
            pc.point.map_or(undef, v3),
            pc.vec.map_or(undef, v3),
            pc.aux.map_or(undef, v3),
            ptype,
            owns,
        ) {
            Ok(()) => 0.0,
            Err(e) => status(&e),
        };
        Ok((c, st))
    }
}

fn pv(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn base_state(o: &mut Vec<f64>, c: &Curve) {
    match c.kind() {
        CurveKind::BSpline(bs) => {
            o.extend([0.0, bs.dim() as f64]);
            match bs.gw_nurb() {
                None => o.push(-1.0),
                Some(cur) => {
                    o.extend([cur.n() as f64, cur.degree() as f64, cur.m() as f64]);
                    for p in cur.cpts() {
                        o.extend([p.x, p.y, p.z, p.w]);
                    }
                    o.extend_from_slice(cur.knots());
                }
            }
        }
        CurveKind::Hermite(h) => {
            o.extend([1.0, h.dim() as f64]);
            let a = h.evaluate(0.0, 1, false);
            let e = h.evaluate(1.0, 1, false);
            for v in a.iter().take(2).chain(e.iter().take(2)) {
                pv(o, *v);
            }
        }
        _ => o.push(-1.0),
    }
}

fn state(o: &mut Vec<f64>, c: &ProjectedCurve) {
    pv(o, c.proj_point());
    pv(o, c.proj_vec());
    pv(o, c.aux_data());
    o.push(f64::from(c.proj_type() as i32));
    o.push(f64::from(c.owns_curve() as i32));
    base_state(o, c.curve());
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

/// Evaluates `r`: `Ok(v)`, or records the status (or returns the deferral).
macro_rules! tri {
    ($o:ident, $r:expr) => {
        match $r {
            Ok(v) => v,
            Err(e) => {
                if let Some(d) = Output::unported(&e) {
                    return Ok(d);
                }
                $o.push(status(&e));
                return Ok(to_output(&$o));
            }
        }
    };
}

/// The status of `r` (`0` on success), or the deferral.
fn st_of(r: solidcore::Result<()>) -> Result<f64, Box<Output>> {
    match r {
        Ok(()) => Ok(0.0),
        Err(e) => match Output::unported(&e) {
            Some(d) => Err(Box::new(d)),
            None => Ok(status(&e)),
        },
    }
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let mut o: Vec<f64> = Vec::new();
    if let OpIn::ReadDb(d) = &i.op {
        let mut db = DatabaseReader::new();
        db.open_for_read(
            db_bytes(d),
            if d.binary {
                FileType::Binary
            } else {
                FileType::Ascii
            },
        );
        let c = tri!(o, ProjectedCurve::read_from_db(&Kernel::default(), &mut db, d.version));
        o.push(0.0);
        state(&mut o, &c);
        o.push(c.type_code() as f64);
        return Ok(to_output(&o));
    }
    let pc_in = i.pc.as_ref().ok_or_else(|| harness("curve op without pc"))?;
    let (mut pc, ctor_status) = build(pc_in)?;
    let k = Kernel::default();
    match &i.op {
        OpIn::ReadDb(_) => {}
        OpIn::State => {
            // through the curve object, as upstream's virtual calls
            let c = Curve::from(pc.clone());
            let ivl = c.natural_interval();
            o.push(ctor_status);
            state(&mut o, &pc);
            o.push(c.type_code() as f64);
            o.push(c.dim() as f64);
            let deg = tri!(o, c.degree());
            let nnk = tri!(o, c.number_natural_knots());
            let ncp = tri!(o, c.number_control_points());
            let rational = tri!(o, c.is_rational());
            o.extend([deg as f64, nnk as f64, ncp as f64, ivl.min(), ivl.max()]);
            o.extend([b(c.is_analytic()), b(pc.is_bounded()), b(rational)]);
            for t in KINDS {
                o.push(b(c.is_kind_of(t)));
            }
        }
        OpIn::MakeOrtho => {
            let s = match st_of(pc.make_ortho_normal()) {
                Ok(s) => s,
                Err(d) => return Ok(*d),
            };
            o.push(s);
            state(&mut o, &pc);
        }
        OpIn::Eval {
            t,
            n,
            left,
            nonzero,
        } => {
            let c = Curve::from(pc);
            let v = tri!(o, c.evaluate(&k, *t, *n, *left, *nonzero));
            o.push(0.0);
            for x in v {
                pv(&mut o, x);
            }
        }
        OpIn::EvalPoint { t } => {
            let c = Curve::from(pc);
            let p = tri!(o, c.evaluate_point(&k, *t));
            o.push(0.0);
            pv(&mut o, p);
        }
        OpIn::Knots { ivl } => {
            let e = ivl.map(|[a, bb]| Extent1d::from_min_max(a, bb));
            let c = Curve::from(pc);
            let (kn, m) = tri!(o, c.knots(e.as_ref()));
            o.push(0.0);
            o.push(kn.len() as f64);
            o.extend_from_slice(&kn);
            o.push(m.len() as f64);
            o.extend(m.iter().map(|&x| x as f64));
        }
        OpIn::Continuities { angle } => {
            let c = Curve::from(pc);
            let (mn, all) = tri!(o, c.calculate_continuities(&k, *angle));
            o.push(0.0);
            o.push(f64::from(mn as i32));
            o.push(all.len() as f64);
            o.extend(all.iter().map(|&x| f64::from(x as i32)));
        }
        OpIn::EditParam { a, b: bb } => {
            let mut c = Curve::from(pc);
            let s = match st_of(c.edit_parameterization(&Kernel::default(), &Extent1d::from_min_max(*a, *bb))) {
                Ok(s) => s,
                Err(d) => return Ok(*d),
            };
            o.push(s);
            projected_state(&mut o, &c)?;
        }
        OpIn::Reverse { a, b: bb } => {
            let mut c = Curve::from(pc);
            match c.reverse_parameterization(&Kernel::default(), &Extent1d::from_min_max(*a, *bb)) {
                Ok(nv) => o.extend([0.0, nv.min(), nv.max()]),
                Err(e) => {
                    if let Some(d) = Output::unported(&e) {
                        return Ok(d);
                    }
                    o.push(status(&e));
                }
            }
            projected_state(&mut o, &c)?;
        }
        OpIn::Trim { a, b: bb, mode } => {
            let mut c = Curve::from(pc);
            let mut ivl = Extent1d::from_min_max(*a, *bb);
            let m = match mode {
                0 => TrimMode::Notify,
                1 => TrimMode::Silent,
                _ => TrimMode::IntervalOnly,
            };
            let s = match st_of(c.trim(&k, &mut ivl, m)) {
                Ok(s) => s,
                Err(d) => return Ok(*d),
            };
            o.extend([s, ivl.min(), ivl.max()]);
            projected_state(&mut o, &c)?;
        }
        OpIn::Transform {
            o: org,
            x,
            y,
            scale,
        } => {
            let ax = Axis2Placement::new(v3(*org), v3(*x), v3(*y));
            let mut c = Curve::from(pc);
            let s = match st_of(c.transform(&Kernel::default(), &ax, scale.map(v3))) {
                Ok(s) => s,
                Err(d) => return Ok(*d),
            };
            o.push(s);
            projected_state(&mut o, &c)?;
        }
        OpIn::Copy => {
            let c = Curve::from(pc.clone());
            let nc = tri!(o, c.copy(&Kernel::default()));
            o.push(0.0);
            projected_state(&mut o, &nc)?;
            o.push(b(pc.owns_curve() == OwnsCurve::No));
            let eq = tri!(o, nc.approx_eq(&k, &c));
            o.push(b(eq));
        }
        OpIn::Equal { other } => {
            let c = Curve::from(pc.clone());
            o.push(0.0);
            match other {
                None => {
                    let eq = tri!(o, c.approx_eq(&k, &c));
                    o.push(b(eq));
                }
                Some(oi) => {
                    let (p2, _) = build(oi)?;
                    let eq = tri!(o, c.approx_eq(&k, &Curve::from(p2)));
                    o.push(b(eq));
                }
            }
        }
        OpIn::Ends => {
            let c = Curve::from(pc);
            let e = tri!(o, c.ends(&k, ));
            o.push(0.0);
            pv(&mut o, e.start);
            pv(&mut o, e.end);
            o.extend([e.start_weight, e.end_weight]);
        }
    }
    Ok(to_output(&o))
}

fn projected_state(o: &mut Vec<f64>, c: &Curve) -> Result<(), HarnessError> {
    match c.kind() {
        CurveKind::Projected(p) => {
            state(o, p);
            Ok(())
        }
        _ => Err(harness("not a projected curve")),
    }
}

// ---- generation -------------------------------------------------------------------

fn rp(rng: &mut Rng, s: f64) -> [f64; 3] {
    rng.point3(-s, s)
}

/// A random clamped B-spline (sometimes rational, 2D or with a repeated interior knot).
fn gen_bspline(rng: &mut Rng) -> BSplineIn {
    let p = 1 + rng.below(4) as usize;
    let n = p + rng.below(6) as usize;
    let rational = rng.chance(0.25);
    let two_d = rng.chance(0.1);
    let a = if rng.chance(0.5) { 0.0 } else { rng.uniform(-5.0, 5.0) };
    let len = if rng.chance(0.5) { 1.0 } else { rng.uniform(0.1, 10.0) };
    let mut u = vec![a; p + 1];
    let inner = n - p;
    let mut ks: Vec<f64> = (0..inner).map(|_| a + len * rng.uniform(0.05, 0.95)).collect();
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
        oob: rng.chance(0.15),
        cpts,
        p,
        u,
    }
}

fn gen_hermite(rng: &mut Rng) -> HermiteIn {
    let s = if rng.chance(0.5) { 3.0 } else { 50.0 };
    let mut d1 = rp(rng, s);
    if rng.chance(0.1) {
        d1 = [0.0; 3];
    }
    HermiteIn {
        dim: 3,
        p1: rp(rng, s),
        d1,
        p2: rp(rng, s),
        d2: rp(rng, s),
    }
}

fn gen_curve(rng: &mut Rng) -> CurveIn {
    if rng.chance(0.7) {
        CurveIn::Bspline(gen_bspline(rng))
    } else {
        CurveIn::Hermite(gen_hermite(rng))
    }
}

fn range_of(c: &CurveIn) -> (f64, f64) {
    match c {
        CurveIn::Bspline(bs) => (
            bs.u.first().copied().unwrap_or(0.0),
            bs.u.last().copied().unwrap_or(1.0),
        ),
        CurveIn::Hermite(_) => (0.0, 1.0),
    }
}

/// A parameter of `c`: interior, at a knot, at an end, or slightly outside.
fn gen_t(rng: &mut Rng, c: &CurveIn) -> f64 {
    let (a, bb) = range_of(c);
    match rng.below(10) {
        0 => a,
        1 => bb,
        2 => {
            if let CurveIn::Bspline(bs) = c {
                *rng.choose(&bs.u).unwrap_or(&a)
            } else {
                0.5
            }
        }
        3 => a + (bb - a) * rng.uniform(-0.3, 1.3),
        _ => a + (bb - a) * rng.unit(),
    }
}

fn gen_ivl(rng: &mut Rng, c: &CurveIn) -> (f64, f64) {
    let (a, bb) = range_of(c);
    if rng.chance(0.3) {
        return (a, bb);
    }
    let x = a + (bb - a) * rng.uniform(-0.2, 1.2);
    let y = a + (bb - a) * rng.uniform(-0.2, 1.2);
    (x.min(y), x.max(y))
}

/// A projection vector: unit, arbitrary, tiny (counts as zero) or exactly zero.
fn gen_vec(rng: &mut Rng) -> [f64; 3] {
    match rng.below(8) {
        0 => [0.0; 3],
        1 => {
            let [x, y, z] = rp(rng, 1.0);
            [x * 1e-13, y * 1e-13, z * 1e-13]
        }
        2 => *rng
            .choose(&[[0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]])
            .unwrap_or(&[0.0, 0.0, 1.0]),
        3 | 4 => {
            let v = v3(rp(rng, 1.0)).unitize_or_z();
            [v.x, v.y, v.z]
        }
        _ => rp(rng, 3.0),
    }
}

fn gen_pc(rng: &mut Rng) -> PcIn {
    let curve = gen_curve(rng);
    let ptype = if rng.chance(0.05) { 3 } else { rng.below(3) as u8 };
    let point = if rng.chance(0.9) { Some(rp(rng, 10.0)) } else { None };
    let vec = if rng.chance(0.92) { Some(gen_vec(rng)) } else { None };
    let aux = if rng.chance(0.85) {
        Some(if ptype == 1 {
            // an eye point off the plane, usually
            let [x, y, z] = rp(rng, 20.0);
            [x, y, z + 30.0]
        } else {
            gen_vec(rng)
        })
    } else {
        None
    };
    let owns = match rng.below(10) {
        0..=3 => 0,
        4..=6 => 1,
        _ => 2,
    };
    PcIn {
        curve,
        ctor: u8::from(rng.chance(0.3)),
        point,
        vec,
        aux,
        ptype,
        owns,
    }
}

/// A unit-ish frame for `Transform` (identity sometimes).
fn gen_transform(rng: &mut Rng) -> OpIn {
    if rng.chance(0.15) {
        return OpIn::Transform {
            o: [0.0; 3],
            x: [1.0, 0.0, 0.0],
            y: [0.0, 1.0, 0.0],
            scale: if rng.chance(0.5) {
                Some([1.0, 1.0, 1.0])
            } else {
                None
            },
        };
    }
    let ang = rng.uniform(0.0, 6.0);
    let (s, c) = ang.sin_cos();
    let scale = if rng.chance(0.4) {
        if rng.chance(0.5) {
            let f = rng.uniform(0.5, 2.0);
            Some([f, f, f])
        } else {
            Some([rng.uniform(0.5, 2.0), rng.uniform(0.5, 2.0), rng.uniform(0.5, 2.0)])
        }
    } else {
        None
    };
    OpIn::Transform {
        o: rp(rng, 5.0),
        x: [c, s, 0.0],
        y: [-s, c, 0.0],
        scale,
    }
}

fn gen_db(rng: &mut Rng) -> DbIn {
    let mut curve = gen_curve(rng);
    if let CurveIn::Bspline(bs) = &mut curve {
        // the B-spline reader keeps the written rationality and dimension
        bs.oob = false;
    }
    DbIn {
        version: if rng.chance(0.3) { 34 } else { 36 },
        binary: rng.chance(0.5),
        point: rp(rng, 10.0),
        vec: gen_vec(rng),
        aux: gen_vec(rng),
        // 0..=3 only: casting a larger value to `SmProjectionType` is undefined
        ptype: rng.below(4),
        base_type: if rng.chance(0.05) { Some(14099) } else { None },
        curve,
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    if rng.chance(0.08) {
        return Inputs {
            pc: None,
            op: OpIn::ReadDb(gen_db(rng)),
        };
    }
    let pc = gen_pc(rng);
    let c = pc.curve.clone();
    let op = match rng.below(15) {
        0 => OpIn::State,
        1 => OpIn::MakeOrtho,
        2..=4 => OpIn::Eval {
            t: gen_t(rng, &c),
            n: rng.below(4) as usize,
            left: rng.chance(0.5),
            nonzero: rng.chance(0.5),
        },
        5 => OpIn::EvalPoint { t: gen_t(rng, &c) },
        6 => OpIn::Knots {
            ivl: if rng.chance(0.5) {
                let (a, bb) = gen_ivl(rng, &c);
                Some([a, bb])
            } else {
                None
            },
        },
        7 => OpIn::Continuities {
            angle: rng.uniform(0.01, 0.2),
        },
        8 => {
            let (a, bb) = gen_ivl(rng, &c);
            OpIn::EditParam { a, b: bb }
        }
        9 => {
            let (a, bb) = range_of(&c);
            OpIn::Reverse { a, b: bb }
        }
        10 => {
            let (a, bb) = gen_ivl(rng, &c);
            OpIn::Trim {
                a,
                b: bb,
                mode: rng.below(3) as u8,
            }
        }
        11 => gen_transform(rng),
        12 => OpIn::Copy,
        13 => OpIn::Equal {
            other: if rng.chance(0.3) {
                None
            } else if rng.chance(0.5) {
                Some(pc.clone())
            } else {
                let mut o = pc.clone();
                match rng.below(4) {
                    0 => o.ptype = (o.ptype + 1) % 3,
                    1 => o.point = Some(rp(rng, 10.0)),
                    2 => o = gen_pc(rng),
                    _ => {
                        if let Some(p) = &mut o.point {
                            p[0] += 1e-14;
                        }
                    }
                }
                Some(o)
            },
        },
        _ => OpIn::Ends,
    };
    Inputs { pc: Some(pc), op }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "curve.projected_curve"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let i = gen_case(rng);
            let Ok((op, input)) = cpp_input(&i) else {
                continue;
            };
            if ffi::validate(op, &input).is_ok() {
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
