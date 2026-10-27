//! Runner for unit `curve.offset_curve`: implicit offset curves (`SmOffsetCurve`).
//!
//! C++: `solidcore_ffi::units::curve_offset_curve` (upstream
//! `source/SMLib/src/SmOffsetCurve.cpp`, called through `SmCurve*`). Rust:
//! `solidcore::curve::offset_curve` through [`Curve`].
//!
//! A case is a `base` curve (`{"bspline": {dim, oob, cpts, p, u}}` with control points
//! as stored `[x, y, z, w]`, or `{"hermite": {dim, p1, d1, p2, d2}}`), the offset
//! `spec` (`{dim, a, b, normal, dist, owns, nrev}`: built, then reversed `nrev` times),
//! an optional `outer` spec (an offset of that offset) and an `op`. `read_db` cases
//! carry the database bytes (made by upstream `WriteToDB` when the case is generated)
//! instead. Both sides produce the flat record documented in the ffi module; the
//! runner reports its first entry as the `status` int (`0` = `SM_SUCCESS`, else the
//! `SmStatus`; `1007` for a Rust `InvalidInput`, `-1` for a Rust error upstream cannot
//! produce) and the rest as the `out` values.
//!
//! `clone_ctor` is upstream's copy constructor, not the virtual `Copy`: the original is
//! built, moved into a [`Curve`] and cloned. Its output is `this==copy copy==this`
//! before the state and probes, so a borrowed base (shared upstream) and an owned one
//! (copied) are compared directly. The constructor is undefined for an owned Hermite
//! base (upstream's base `Copy` fails, then both objects delete it); such cases are
//! rejected before either side runs. The empty object (no base, `UNSURE` ownership) is
//! undefined too but the protocol cannot express it.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p2::Curve;
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::curve::offset_curve::OffsetCurve;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_curve_n_lib::TrimMode;
use solidcore::nurbs::math::NOW;
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::topology::database_io::{DatabaseReader, FileType};
use solidcore::SolidError;
use solidcore_ffi::units::curve_offset_curve::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `curve.offset_curve` runner.
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
    /// Knots written back over the curve's own NURB after construction, undoing the
    /// constructor's knot fix-up (base block kind `2`). Omitted when false, so
    /// existing case identities are unchanged.
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum BaseIn {
    Bspline(BSplineIn),
    Hermite(HermiteIn),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SpecIn {
    #[serde(default = "three")]
    dim: u64,
    a: f64,
    b: f64,
    normal: [f64; 3],
    dist: f64,
    #[serde(default)]
    owns: bool,
    #[serde(default)]
    nrev: usize,
}

const fn three() -> u64 {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    State,
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
        mults: bool,
    },
    Continuities {
        angle: f64,
    },
    Reverse {
        a: f64,
        b: f64,
        probes: Vec<f64>,
    },
    Trim {
        a: f64,
        b: f64,
        /// 0 `TRUE`, 1 `FALSE`, 2 `UNSURE`.
        mode: u8,
        probes: Vec<f64>,
    },
    Eq {
        #[serde(default)]
        this: bool,
        #[serde(default)]
        copy: bool,
        other: Option<SpecIn>,
    },
    Copy {
        probes: Vec<f64>,
    },
    /// The copy constructor (`new (ctx) SmOffsetCurve(*original)`), not `Copy`.
    CloneCtor {
        probes: Vec<f64>,
    },
    IsKindOf {
        #[serde(rename = "type")]
        type_code: i64,
    },
    ReadDb {
        binary: bool,
        version: u64,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        bytes: Option<Vec<u8>>,
        probes: Vec<f64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default)]
    base: Option<BaseIn>,
    #[serde(default)]
    spec: Option<SpecIn>,
    #[serde(default)]
    outer: Option<SpecIn>,
    #[serde(flatten)]
    op: OpIn,
}

fn b(f: bool) -> f64 {
    if f {
        1.0
    } else {
        0.0
    }
}

fn push3(v: &mut Vec<f64>, p: [f64; 3]) {
    v.extend(p);
}

fn base_block(v: &mut Vec<f64>, c: &BaseIn) -> Result<(), HarnessError> {
    match c {
        BaseIn::Bspline(bs) => {
            let n = bs
                .cpts
                .len()
                .checked_sub(1)
                .ok_or_else(|| harness("no points"))?;
            let m =
                bs.u.len()
                    .checked_sub(1)
                    .ok_or_else(|| harness("no knots"))?;
            let kind = if bs.raw_knots { 2.0 } else { 0.0 };
            v.extend([
                kind,
                bs.dim as f64,
                b(bs.oob),
                n as f64,
                bs.p as f64,
                m as f64,
            ]);
            for p in &bs.cpts {
                v.extend(p);
            }
            v.extend_from_slice(&bs.u);
        }
        BaseIn::Hermite(h) => {
            v.extend([1.0, h.dim as f64]);
            for p in [h.p1, h.d1, h.p2, h.d2] {
                push3(v, p);
            }
        }
    }
    Ok(())
}

fn spec_block(v: &mut Vec<f64>, s: &SpecIn) {
    v.extend([s.dim as f64, s.a, s.b]);
    push3(v, s.normal);
    v.extend([s.dist, b(s.owns), s.nrev as f64]);
}

fn probe_block(v: &mut Vec<f64>, probes: &[f64]) {
    v.push(probes.len() as f64);
    v.extend_from_slice(probes);
}

fn db_bytes(text: Option<&String>, bytes: Option<&Vec<u8>>) -> Result<Vec<u8>, HarnessError> {
    match (text, bytes) {
        (Some(t), None) => Ok(t.clone().into_bytes()),
        (None, Some(b)) => Ok(b.clone()),
        _ => Err(harness("read_db needs exactly one of text, bytes")),
    }
}

fn op_of(op: &OpIn) -> Op {
    match op {
        OpIn::State => Op::State,
        OpIn::Eval { .. } => Op::Eval,
        OpIn::EvalPoint { .. } => Op::EvalPoint,
        OpIn::Knots { .. } => Op::Knots,
        OpIn::Continuities { .. } => Op::Continuities,
        OpIn::Reverse { .. } => Op::Reverse,
        OpIn::Trim { .. } => Op::Trim,
        OpIn::Eq { .. } => Op::Eq,
        OpIn::Copy { .. } => Op::Copy,
        OpIn::CloneCtor { .. } => Op::CloneCtor,
        OpIn::IsKindOf { .. } => Op::IsKindOf,
        OpIn::ReadDb { .. } => Op::ReadDb,
    }
}

/// The curve blocks of a curve op.
fn curve_blocks(v: &mut Vec<f64>, i: &Inputs) -> Result<(), HarnessError> {
    let base = i
        .base
        .as_ref()
        .ok_or_else(|| harness("curve op without base"))?;
    let spec = i
        .spec
        .as_ref()
        .ok_or_else(|| harness("curve op without spec"))?;
    base_block(v, base)?;
    spec_block(v, spec);
    match &i.outer {
        None => v.push(0.0),
        Some(o) => {
            v.push(1.0);
            spec_block(v, o);
        }
    }
    Ok(())
}

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let op = op_of(&i.op);
    let mut v = Vec::new();
    if let OpIn::ReadDb {
        binary,
        version,
        text,
        bytes,
        probes,
    } = &i.op
    {
        let data = db_bytes(text.as_ref(), bytes.as_ref())?;
        v.extend([b(*binary), *version as f64, data.len() as f64]);
        v.extend(data.iter().map(|&c| f64::from(c)));
        probe_block(&mut v, probes);
        return Ok((op, v));
    }
    curve_blocks(&mut v, i)?;
    match &i.op {
        OpIn::State | OpIn::ReadDb { .. } => {}
        OpIn::Eval {
            t,
            n,
            left,
            nonzero,
        } => {
            v.extend([*t, *n as f64, b(*left), b(*nonzero)]);
        }
        OpIn::EvalPoint { t } => v.push(*t),
        OpIn::Knots { ivl, mults } => {
            let [a, bb] = ivl.unwrap_or([0.0, 0.0]);
            v.extend([b(ivl.is_some()), a, bb, b(*mults)]);
        }
        OpIn::Continuities { angle } => v.push(*angle),
        OpIn::Reverse { a, b: bb, probes } => {
            v.extend([*a, *bb]);
            probe_block(&mut v, probes);
        }
        OpIn::Trim {
            a,
            b: bb,
            mode,
            probes,
        } => {
            v.extend([*a, *bb, f64::from(*mode)]);
            probe_block(&mut v, probes);
        }
        OpIn::Eq { this, copy, other } => {
            v.extend([b(*this), b(*copy)]);
            if !*this && !*copy {
                let o = other.as_ref().ok_or_else(|| harness("eq without other"))?;
                spec_block(&mut v, o);
            }
        }
        OpIn::Copy { probes } | OpIn::CloneCtor { probes } => probe_block(&mut v, probes),
        OpIn::IsKindOf { type_code } => v.push(*type_code as f64),
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

fn build_base(c: &BaseIn) -> Result<Curve, HarnessError> {
    match c {
        BaseIn::Bspline(bs) => {
            let n = bs
                .cpts
                .len()
                .checked_sub(1)
                .ok_or_else(|| harness("no points"))?;
            let m =
                bs.u.len()
                    .checked_sub(1)
                    .ok_or_else(|| harness("no knots"))?;
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
        BaseIn::Hermite(h) => Ok(Curve::from(HermiteCurve::new(
            v3(h.p1),
            v3(h.d1),
            v3(h.p2),
            v3(h.d2),
            h.dim,
        ))),
    }
}

fn build_offset(base: Curve, s: &SpecIn) -> solidcore::Result<Curve> {
    let mut c = Curve::from(OffsetCurve::new(
        s.dim,
        base,
        Extent1d::from_min_max(s.a, s.b),
        v3(s.normal),
        s.dist,
        s.owns,
    )?);
    for _ in 0..s.nrev {
        let ivl = c.natural_interval();
        // (status ignored by the shim; the offset reversal cannot fail)
        let _ = c.reverse_parameterization(&Kernel::default(), &ivl);
    }
    Ok(c)
}

/// Most reversals the runner applies (`ffi::MAX_REVERSALS`, the guard's bound): the
/// `nrev` loop is runner code, so a fuzzed count must not make it run for hours.
const MAX_RUNNER_REVERSALS: usize = 4;

/// The base curve and the op's curve.
fn build(i: &Inputs) -> Result<(Curve, Curve), HarnessError> {
    let other = match &i.op {
        OpIn::Eq { other, .. } => other.as_ref(),
        _ => None,
    };
    for s in [i.spec.as_ref(), i.outer.as_ref(), other]
        .into_iter()
        .flatten()
    {
        if s.nrev > MAX_RUNNER_REVERSALS {
            return Err(harness("nrev above the runner's reversal bound"));
        }
    }
    // the curves are built again rather than cloned: a Rust clone would fix the base
    // curve's knots up, while upstream offsets the curve itself
    let base_in = i.base.as_ref().ok_or_else(|| harness("no base"))?;
    let spec = i.spec.as_ref().ok_or_else(|| harness("no spec"))?;
    let built =
        |base: Curve, spec: &SpecIn| build_offset(base, spec).map_err(|e| harness(e.to_string()));
    let inner = built(build_base(base_in)?, spec)?;
    Ok(match &i.outer {
        None => (build_base(base_in)?, inner),
        Some(o) => (built(build_base(base_in)?, spec)?, built(inner, o)?),
    })
}

/// The copy constructor's undefined domain (also rejected by the ffi wrapper): it copies
/// the base curve when the original owns it (the protocol's `owns` flag), and upstream's
/// `Copy` fails for a Hermite curve, leaving the copy sharing a base both objects then
/// delete. Without an outer offset the original owns the base itself; with one the outer
/// offset must own the inner offset it copies (a borrowed inner offset is shared, so a
/// borrowed Hermite chain stays defined).
fn clone_ctor_unsafe(i: &Inputs) -> bool {
    let hermite = matches!(&i.base, Some(BaseIn::Hermite(_)));
    let inner_owns = i.spec.as_ref().is_some_and(|s| s.owns);
    let outer_copies = i.outer.as_ref().is_none_or(|s| s.owns);
    hermite && inner_owns && outer_copies
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

fn pv(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn state(k: &Kernel, o: &mut Vec<f64>, c: &Curve) -> Result<(), SolidError> {
    o.extend([c.type_code() as f64, c.dim() as f64]);
    let solidcore::curve::curve::p2::CurveKind::Offset(off) = c.kind() else {
        o.push(-1.0);
        return Ok(());
    };
    pv(o, off.offset_normal());
    let ivl = off.natural_interval();
    o.extend([
        off.offset_distance(),
        ivl.min(),
        ivl.max(),
        f64::from(off.reversed_flag()),
    ]);
    match off.base_curve() {
        None => o.extend([-1.0, -1.0]),
        Some(bc) => {
            o.push(bc.type_code() as f64);
            o.push(off.number_natural_knots(k)? as f64);
        }
    }
    Ok(())
}

fn probes(k: &Kernel, o: &mut Vec<f64>, c: &Curve, ts: &[f64]) -> Result<(), SolidError> {
    for &t in ts {
        match c.evaluate_point(k, t) {
            Ok(p) => {
                o.push(0.0);
                pv(o, p);
            }
            Err(e @ SolidError::Unported { .. }) => return Err(e),
            Err(e) => o.push(status(&e)),
        }
        match c.evaluate(k, t, 3, true, true) {
            Ok(v) => {
                o.push(0.0);
                for x in v {
                    pv(o, x);
                }
            }
            Err(e @ SolidError::Unported { .. }) => return Err(e),
            Err(e) => o.push(status(&e)),
        }
    }
    Ok(())
}

/// Runs the op; `Err` is a Rust failure of the op itself (reported as its status).
fn rust_op(k: &Kernel, i: &Inputs, o: &mut Vec<f64>) -> Result<(), SolidError> {
    if let OpIn::ReadDb {
        binary,
        version,
        text,
        bytes,
        probes: ts,
    } = &i.op
    {
        let data =
            db_bytes(text.as_ref(), bytes.as_ref()).map_err(|_| SolidError::InvalidInput {
                operation: "difftest",
                reason: "bad read_db input",
            })?;
        let mut db = DatabaseReader::new();
        db.open_for_read(
            data,
            if *binary {
                FileType::Binary
            } else {
                FileType::Ascii
            },
        );
        let c = Curve::from(OffsetCurve::read_from_db(
            &Kernel::default(),
            &mut db,
            *version,
        )?);
        o.push(0.0);
        state(k, o, &c)?;
        return probes(k, o, &c, ts);
    }
    if matches!(&i.op, OpIn::CloneCtor { .. }) && clone_ctor_unsafe(i) {
        return Err(SolidError::InvalidInput {
            operation: "difftest",
            reason: "the native copy constructor is undefined for an owned Hermite base",
        });
    }
    let (under, mut c) = build(i).map_err(|_| SolidError::InvalidInput {
        operation: "difftest",
        reason: "bad curve input",
    })?;
    match &i.op {
        OpIn::State => {
            o.push(0.0);
            state(k, o, &c)?;
        }
        OpIn::Eval {
            t,
            n,
            left,
            nonzero,
        } => {
            let v = c.evaluate(k, *t, *n, *left, *nonzero)?;
            o.push(0.0);
            for x in v {
                pv(o, x);
            }
        }
        OpIn::EvalPoint { t } => {
            let p = c.evaluate_point(k, *t)?;
            o.push(0.0);
            pv(o, p);
        }
        OpIn::Knots { ivl, mults } => {
            let ivl = ivl.map(|[a, bb]| Extent1d::from_min_max(a, bb));
            let (k, m) = c.knots(k, ivl.as_ref())?;
            o.push(0.0);
            o.push(k.len() as f64);
            o.extend(&k);
            if *mults {
                o.push(m.len() as f64);
                o.extend(m.iter().map(|&x| x as f64));
            } else {
                o.push(0.0);
            }
        }
        OpIn::Continuities { angle } => {
            let (min, all) = c.calculate_continuities(k, *angle)?;
            o.push(0.0);
            o.push(f64::from(min as i32));
            o.push(all.len() as f64);
            o.extend(all.iter().map(|&x| f64::from(x as i32)));
        }
        OpIn::Reverse {
            a,
            b: bb,
            probes: ts,
        } => {
            let ivl =
                c.reverse_parameterization(&Kernel::default(), &Extent1d::from_min_max(*a, *bb))?;
            o.extend([0.0, ivl.min(), ivl.max()]);
            state(k, o, &c)?;
            probes(k, o, &c, ts)?;
        }
        OpIn::Trim {
            a,
            b: bb,
            mode,
            probes: ts,
        } => {
            let mut ivl = Extent1d::from_min_max(*a, *bb);
            let mode = match mode {
                0 => TrimMode::Notify,
                1 => TrimMode::Silent,
                _ => TrimMode::IntervalOnly,
            };
            let k = Kernel::default();
            let s = match c.trim(&k, &mut ivl, mode) {
                Ok(()) => 0.0,
                Err(e @ SolidError::Unported { .. }) => return Err(e),
                Err(e) => status(&e),
            };
            o.extend([s, ivl.min(), ivl.max()]);
            state(&k, o, &c)?;
            probes(&k, o, &c, ts)?;
        }
        OpIn::Eq { this, copy, other } => {
            o.push(0.0);
            if *this {
                o.push(b(c.approx_eq(k, &c)?));
            } else if *copy {
                let cp = c.copy(&Kernel::default())?;
                o.push(b(c.approx_eq(k, &cp)?));
                o.push(b(cp.approx_eq(k, &c)?));
            } else {
                let s = other.as_ref().ok_or(SolidError::InvalidInput {
                    operation: "difftest",
                    reason: "eq without other",
                })?;
                let mut s = s.clone();
                s.owns = false;
                let c2 = build_offset(under, &s)?;
                o.push(b(c.approx_eq(k, &c2)?));
                o.push(b(c2.approx_eq(k, &c)?));
            }
        }
        OpIn::Copy { probes: ts } => {
            let cp = c.copy(&Kernel::default())?;
            o.push(0.0);
            state(k, o, &cp)?;
            probes(k, o, &cp, ts)?;
        }
        OpIn::CloneCtor { probes: ts } => {
            // upstream's copy constructor: the original is moved into `Curve` and cloned
            let cp = c.clone();
            o.push(0.0);
            o.push(b(c.approx_eq(k, &cp)?));
            o.push(b(cp.approx_eq(k, &c)?));
            state(k, o, &cp)?;
            probes(k, o, &cp, ts)?;
        }
        OpIn::IsKindOf { type_code } => {
            o.push(0.0);
            o.push(b(c.is_kind_of(*type_code)));
        }
        OpIn::ReadDb { .. } => {}
    }
    Ok(())
}

fn rust(k: &Kernel, i: &Inputs) -> Result<Output, HarnessError> {
    let mut o = Vec::new();
    let Err(e) = rust_op(k, i, &mut o) else {
        return Ok(to_output(&o));
    };
    if let Some(d) = Output::unported(&e) {
        return Ok(d);
    }
    if matches!(
        e,
        SolidError::InvalidInput {
            operation: "difftest",
            ..
        }
    ) {
        return Err(harness(e.to_string()));
    }
    // the op failed: only its status (the shim writes nothing else then)
    Ok(to_output(&[status(&e)]))
}

// ---- generation -------------------------------------------------------------------

fn rp(rng: &mut Rng, scale: f64) -> [f64; 3] {
    [
        rng.uniform(-scale, scale),
        rng.uniform(-scale, scale),
        rng.uniform(-scale, scale),
    ]
}

/// A clamped B-spline: planar (in `z = 0`) or spatial, sometimes rational.
fn gen_bspline(rng: &mut Rng, planar: bool) -> BSplineIn {
    let p = 1 + rng.below(4) as usize;
    let n = p + rng.below(6) as usize;
    let rational = rng.chance(0.25);
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
    let scale = if rng.chance(0.5) { 5.0 } else { 50.0 };
    let degenerate_end = rng.chance(0.08);
    let mut cpts: Vec<[f64; 4]> = (0..=n)
        .map(|i| {
            let [x, y, z] = rp(rng, scale);
            let x = x + i as f64 * scale * 0.3;
            let z = if planar { 0.0 } else { z };
            let w = if rational { rng.uniform(0.3, 3.0) } else { NOW };
            if rational {
                [x * w, y * w, z * w, w]
            } else {
                [x, y, z, w]
            }
        })
        .collect();
    if degenerate_end && cpts.len() >= 2 {
        // a zero first derivative at the start (repeated first control point)
        if let Some(&first) = cpts.first() {
            if let Some(second) = cpts.get_mut(1) {
                let w0 = first[3];
                let w1 = second[3];
                if w0 == NOW {
                    *second = first;
                } else {
                    let s = w1 / w0;
                    *second = [first[0] * s, first[1] * s, first[2] * s, w1];
                }
            }
        }
    }
    BSplineIn {
        dim: 3,
        oob: rng.chance(0.15),
        cpts,
        p,
        u,
        raw_knots: false,
    }
}

fn gen_hermite(rng: &mut Rng, planar: bool) -> HermiteIn {
    let f = |rng: &mut Rng, s: f64| {
        let [x, y, z] = rp(rng, s);
        [x, y, if planar { 0.0 } else { z }]
    };
    let p1 = f(rng, 5.0);
    let mut d1 = f(rng, 10.0);
    let p2 = f(rng, 5.0);
    let d2 = f(rng, 10.0);
    if rng.chance(0.1) {
        d1 = [0.0, 0.0, 0.0];
    }
    HermiteIn {
        dim: 3,
        p1,
        d1,
        p2,
        d2,
    }
}

fn base_range(c: &BaseIn) -> (f64, f64) {
    match c {
        BaseIn::Bspline(bs) => (
            bs.u.first().copied().unwrap_or(0.0),
            bs.u.last().copied().unwrap_or(1.0),
        ),
        BaseIn::Hermite(_) => (0.0, 1.0),
    }
}

fn gen_normal(rng: &mut Rng, planar: bool) -> [f64; 3] {
    match rng.below(8) {
        0 => [0.0, 0.0, 0.0],
        1 | 2 if planar => [0.0, 0.0, if rng.chance(0.5) { 1.0 } else { -2.5 }],
        3 => [1.0, 0.0, 0.0],
        _ => rp(rng, 1.0),
    }
}

fn gen_spec(rng: &mut Rng, range: (f64, f64), planar: bool) -> SpecIn {
    let (a, bb) = range;
    let len = bb - a;
    let (sa, sb) = match rng.below(4) {
        0 => (a + 0.1 * len, b_(bb, len, 0.2)),
        1 => (a - 0.01 * len, bb),
        _ => (a, bb),
    };
    let dist = match rng.below(8) {
        0 => 0.0,
        1 => 1e-13,
        2 => -rng.uniform(0.01, 5.0),
        _ => rng.uniform(0.01, 5.0),
    };
    SpecIn {
        dim: if rng.chance(0.1) { 2 } else { 3 },
        a: sa,
        b: sb,
        normal: gen_normal(rng, planar),
        dist,
        owns: rng.chance(0.5),
        nrev: usize::from(rng.chance(0.3)),
    }
}

fn b_(bb: f64, len: f64, f: f64) -> f64 {
    bb - f * len
}

/// A parameter of the offset interval: interior, at an end, near an end, outside.
fn gen_t(rng: &mut Rng, s: &SpecIn) -> f64 {
    let (a, bb) = (s.a.min(s.b), s.a.max(s.b));
    let len = bb - a;
    match rng.below(10) {
        0 => a,
        1 => bb,
        2 => bb - 1e-7 * len,
        3 => a + 1e-9 * len,
        4 => bb + 0.05 * len,
        _ => rng.uniform(a, bb),
    }
}

fn gen_probes(rng: &mut Rng, s: &SpecIn) -> Vec<f64> {
    let k = 1 + rng.below(4) as usize;
    (0..k).map(|_| gen_t(rng, s)).collect()
}

/// Upstream's `WriteToDB` bytes of the case's curve.
fn write_db(i: &Inputs, binary: bool, version: u64) -> Option<Vec<u8>> {
    let mut v = Vec::new();
    curve_blocks(&mut v, i).ok()?;
    v.extend([b(binary), version as f64]);
    let out = ffi::eval(Op::WriteDb, &v).ok()?;
    if out.first().copied() != Some(0.0) {
        return None;
    }
    Some(out.get(1..)?.iter().map(|&x| x as u8).collect())
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let planar = rng.chance(0.6);
    let base = if rng.chance(0.7) {
        BaseIn::Bspline(gen_bspline(rng, planar))
    } else {
        BaseIn::Hermite(gen_hermite(rng, planar))
    };
    let range = base_range(&base);
    let spec = gen_spec(rng, range, planar);
    let outer = rng.chance(0.12).then(|| {
        let mut o = gen_spec(rng, (spec.a.min(spec.b), spec.a.max(spec.b)), planar);
        o.a = spec.a.min(spec.b);
        o.b = spec.a.max(spec.b);
        o
    });
    let top = outer.clone().unwrap_or_else(|| spec.clone());
    let op = match rng.below(24) {
        0 => OpIn::State,
        1..=8 => OpIn::Eval {
            t: gen_t(rng, &top),
            n: rng.below(6) as usize,
            left: rng.chance(0.5),
            nonzero: rng.chance(0.7),
        },
        9..=11 => OpIn::EvalPoint {
            t: gen_t(rng, &top),
        },
        12 => OpIn::Knots {
            ivl: rng.chance(0.5).then(|| {
                let x = gen_t(rng, &top);
                let y = gen_t(rng, &top);
                [x, y]
            }),
            mults: rng.chance(0.5),
        },
        13 => OpIn::Continuities {
            angle: rng.uniform(0.0, 10.0),
        },
        14 | 15 => OpIn::Reverse {
            a: gen_t(rng, &top),
            b: gen_t(rng, &top),
            probes: gen_probes(rng, &top),
        },
        16 | 17 => OpIn::Trim {
            a: gen_t(rng, &top),
            b: gen_t(rng, &top),
            mode: rng.below(3) as u8,
            probes: gen_probes(rng, &top),
        },
        18 => OpIn::Eq {
            this: rng.chance(0.2),
            copy: rng.chance(0.4),
            other: Some(if rng.chance(0.5) {
                top.clone()
            } else {
                gen_spec(rng, range, planar)
            }),
        },
        19 => OpIn::Copy {
            probes: gen_probes(rng, &top),
        },
        20 => OpIn::IsKindOf {
            type_code: *rng
                .choose(&[14036, 14001, 14034, 14033, 13001, 1, 0])
                .unwrap_or(&14036),
        },
        _ => {
            let binary = rng.chance(0.5);
            OpIn::ReadDb {
                binary,
                version: 42,
                text: None,
                bytes: None,
                probes: gen_probes(rng, &top),
            }
        }
    };
    Inputs {
        base: Some(base),
        spec: Some(spec),
        outer,
        op,
    }
}

/// Fills in a generated `read_db` case's bytes from upstream's writer and drops the
/// curve (the reader builds its own).
fn finish(mut i: Inputs) -> Option<Inputs> {
    if let OpIn::ReadDb {
        binary,
        version,
        text,
        bytes,
        ..
    } = &mut i.op
    {
        let data = {
            let probe = Inputs {
                base: i.base.clone(),
                spec: i.spec.clone(),
                outer: i.outer.clone(),
                op: OpIn::State,
            };
            write_db(&probe, *binary, *version)?
        };
        if *binary {
            *bytes = Some(data);
        } else {
            *text = Some(String::from_utf8(data).ok()?);
        }
        i.base = None;
        i.spec = None;
        i.outer = None;
    }
    Some(i)
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "curve.offset_curve"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let i = gen_case(rng);
            let Ok((op, input)) = cpp_input(&Inputs {
                op: if matches!(i.op, OpIn::ReadDb { .. }) {
                    OpIn::State
                } else {
                    i.op.clone()
                },
                ..i.clone()
            }) else {
                continue;
            };
            if ffi::validate(op, &input).is_err() {
                continue;
            }
            if let Some(i) = finish(i) {
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
        let k = &Kernel::default();
        rust(k, &case.parse()?)
    }
}
