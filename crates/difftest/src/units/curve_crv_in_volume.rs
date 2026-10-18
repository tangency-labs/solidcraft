//! Runner for unit `curve.crv_in_volume`: `SmCrvInVolume`, a curve mapped through a
//! volume.
//!
//! C++: `solidcore_ffi::units::curve_crv_in_volume` (upstream
//! `source/SMLib/src/SmCrvInVolume.cpp` on objects built from a B-spline or Hermite base
//! curve and an identity `SmTransform` volume; the break cache is loaded directly).
//! Rust: `solidcore::curve::crv_in_volume` (through the [`Curve`] dispatch where the
//! method is virtual upstream).
//!
//! A case is an optional object (`civ`: `curve` as in `curve.curve_p2`, `in_param`,
//! `copy`, `owner`, optional `cache` `{need, breaks, conts}`) and an `op` with its
//! parameters. Both sides produce the flat record documented in the ffi module; the
//! runner reports its first entry as the `status` int (`0` = `SM_SUCCESS`, else the
//! `SmStatus`; `1007` for a Rust `InvalidInput`, `-1` for a Rust error upstream cannot
//! produce) and the rest as `out`.
//!
//! Deferred cases: what reaches the unported parts: the curve classification of
//! uncached breaks (`intersect.curve_class_*`) and the unported volume kinds. Every
//! run has at least [`MIN_GENERATED`]
//! generated cases; generated inputs the ffi wrapper rejects are redrawn.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::crv_in_volume::{continuity_from_code, CrvInVolume};
use solidcore::geom::transform::Transform;
use solidcore::nurbs::math::{NOW, NOZ};
use solidcore::surface::volume::{Volume, VolumeKind};
use solidcore::curve::curve::p2::{Curve, CurveKind};
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_curve_n_lib::TrimMode;
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::topology::database_io::{DatabaseReader, FileType, InputStream};
use solidcore::SolidError;
use solidcore_ffi::units::curve_crv_in_volume::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `curve.crv_in_volume` runner.
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
struct CacheIn {
    need: bool,
    breaks: Vec<f64>,
    conts: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CivIn {
    curve: CurveIn,
    #[serde(default)]
    in_param: bool,
    #[serde(default)]
    copy: u64,
    #[serde(default)]
    owner: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cache: Option<CacheIn>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Info {
        types: Vec<i64>,
    },
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
        want: bool,
    },
    Mults {
        knots: Vec<f64>,
    },
    Nnk,
    Conts {
        angle: f64,
    },
    Equal {
        other: Option<CivIn>,
    },
    Copy,
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
        mode: i64,
    },
    Transform {
        origin: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        scale: Option<[f64; 3]>,
    },
    ReadDb {
        version: i64,
        text: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    civ: Option<CivIn>,
    #[serde(flatten)]
    op: OpIn,
}

// ---- C++ input --------------------------------------------------------------------

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
                v.extend_from_slice(&p);
            }
        }
    }
}

fn civ_block(v: &mut Vec<f64>, c: &CivIn) {
    curve_block(v, &c.curve);
    v.extend([b(c.in_param), c.copy as f64, c.owner as f64]);
    match &c.cache {
        None => v.push(0.0),
        Some(k) => {
            v.extend([1.0, b(k.need), k.breaks.len() as f64]);
            v.extend_from_slice(&k.breaks);
            v.push(k.conts.len() as f64);
            v.extend(k.conts.iter().map(|&c| c as f64));
        }
    }
}

fn op_of(op: &OpIn) -> Op {
    match op {
        OpIn::Info { .. } => Op::Info,
        OpIn::Eval { .. } => Op::Eval,
        OpIn::EvalPoint { .. } => Op::EvalPoint,
        OpIn::Knots { .. } => Op::Knots,
        OpIn::Mults { .. } => Op::Mults,
        OpIn::Nnk => Op::Nnk,
        OpIn::Conts { .. } => Op::Conts,
        OpIn::Equal { .. } => Op::Equal,
        OpIn::Copy => Op::Copy,
        OpIn::EditParam { .. } => Op::EditParam,
        OpIn::Reverse { .. } => Op::Reverse,
        OpIn::Trim { .. } => Op::Trim,
        OpIn::Transform { .. } => Op::Transform,
        OpIn::ReadDb { .. } => Op::ReadDb,
    }
}

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let op = op_of(&i.op);
    let mut v = Vec::new();
    if !op.is_static() {
        let c = i.civ.as_ref().ok_or_else(|| harness("object op without civ"))?;
        civ_block(&mut v, c);
    }
    match &i.op {
        OpIn::Info { types } => {
            v.push(types.len() as f64);
            v.extend(types.iter().map(|&t| t as f64));
        }
        OpIn::Eval {
            t,
            n,
            left,
            nonzero,
        } => v.extend([*t, *n as f64, b(*left), b(*nonzero)]),
        OpIn::EvalPoint { t } => v.push(*t),
        OpIn::Knots { ivl, want } => {
            let [x, y] = ivl.unwrap_or([0.0, 0.0]);
            v.extend([b(ivl.is_some()), x, y, b(*want)]);
        }
        OpIn::Mults { knots } => {
            v.push(knots.len() as f64);
            v.extend_from_slice(knots);
        }
        OpIn::Nnk | OpIn::Copy => {}
        OpIn::Conts { angle } => v.push(*angle),
        OpIn::Equal { other } => match other {
            None => v.push(1.0),
            Some(o) => {
                v.push(0.0);
                civ_block(&mut v, o);
            }
        },
        OpIn::EditParam { a, b: bb } | OpIn::Reverse { a, b: bb } => v.extend([*a, *bb]),
        OpIn::Trim { a, b: bb, mode } => v.extend([*a, *bb, *mode as f64]),
        OpIn::Transform {
            origin,
            x,
            y,
            scale,
        } => {
            v.extend_from_slice(origin);
            v.extend_from_slice(x);
            v.extend_from_slice(y);
            v.push(b(scale.is_some()));
            v.extend_from_slice(&scale.unwrap_or([1.0, 1.0, 1.0]));
        }
        OpIn::ReadDb { version, text } => {
            v.push(*version as f64);
            v.push(text.len() as f64);
            v.extend(text.bytes().map(f64::from));
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
    }
}

fn build(c: &CivIn) -> Result<CrvInVolume, HarnessError> {
    let mut civ = CrvInVolume::new(
        build_curve(&c.curve)?,
        c.in_param,
        Volume::new(VolumeKind::Transform(Transform::IDENTITY)),
        c.copy,
        c.owner,
    );
    if let Some(k) = &c.cache {
        let conts = k
            .conts
            .iter()
            .map(|&x| continuity_from_code(x).ok_or_else(|| harness("bad continuity code")))
            .collect::<Result<Vec<_>, _>>()?;
        civ.set_breaks_cache(k.need, k.breaks.clone(), conts);
    }
    Ok(civ)
}

fn civ_of(c: &Curve) -> Result<&CrvInVolume, HarnessError> {
    match c.kind() {
        CurveKind::CrvInVolume(x) => Ok(x),
        _ => Err(harness("not a curve in volume")),
    }
}

fn pv(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn put_curve(o: &mut Vec<f64>, c: &Curve) {
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
        // other kinds never are the base curve here
        _ => o.push(-1.0),
    }
}

fn put_cache(o: &mut Vec<f64>, c: &CrvInVolume) {
    o.push(b(c.need_breaks()));
    o.push(c.breaks().len() as f64);
    o.extend_from_slice(c.breaks());
    o.push(c.conts().len() as f64);
    o.extend(c.conts().iter().map(|&k| f64::from(k as i32)));
}

fn put_state(o: &mut Vec<f64>, c: &CrvInVolume) {
    o.push(c.owner_flag() as f64);
    o.push(b(c.in_param_space()));
    put_cache(o, c);
    put_curve(o, c.curve());
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

/// Evaluates `r`: `Ok(v)`, or returns the deferral / status record.
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

/// The status of a result whose failure still has outputs: `Ok(0)` / `Ok(code)`, or
/// `Err(deferred)`.
fn st_of<T>(r: &solidcore::Result<T>) -> Result<f64, Box<Output>> {
    match r {
        Ok(_) => Ok(0.0),
        Err(e) => match Output::unported(e) {
            Some(d) => Err(Box::new(d)),
            None => Ok(status(e)),
        },
    }
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let mut o: Vec<f64> = Vec::new();
    if let OpIn::ReadDb { version, text } = &i.op {
        let mut db = DatabaseReader::with_stream(
            FileType::Ascii,
            InputStream::new(text.as_bytes().to_vec()),
        );
        let c = match CrvInVolume::read_from_db(&Kernel::default(), &mut db, *version as u64) {
            Ok(c) => c,
            Err(e) => {
                if let Some(d) = Output::unported(&e) {
                    return Ok(d);
                }
                // status, not made
                o.extend([status(&e), 0.0]);
                return Ok(to_output(&o));
            }
        };
        o.extend([0.0, 1.0]);
        put_state(&mut o, &c);
        return Ok(to_output(&o));
    }
    let civ_in = i.civ.as_ref().ok_or_else(|| harness("object op without civ"))?;
    let mut civ = build(civ_in)?;
    let k = Kernel::default();
    match &i.op {
        OpIn::Info { types } => {
            let c = Curve::from(civ);
            let deg = tri!(o, c.degree());
            let ncp = tri!(o, c.number_control_points());
            let ivl = c.natural_interval();
            let x = civ_of(&c)?;
            o.push(0.0);
            o.extend([
                c.type_code() as f64,
                c.dim() as f64,
                deg as f64,
                ncp as f64,
                ivl.min(),
                ivl.max(),
                b(c.is_analytic()),
                b(c.is_bounded()),
                b(x.owns_curve()),
                b(x.owns_volume()),
            ]);
            o.extend(types.iter().map(|&t| b(c.is_kind_of(t))));
            put_state(&mut o, x);
        }
        OpIn::Eval {
            t,
            n,
            left,
            nonzero,
        } => {
            let c = Curve::from(civ);
            let v = tri!(o, c.evaluate(*t, *n, *left, *nonzero));
            o.push(0.0);
            for p in v {
                pv(&mut o, p);
            }
        }
        OpIn::EvalPoint { t } => {
            let c = Curve::from(civ);
            let p = tri!(o, c.evaluate_point(*t));
            o.push(0.0);
            pv(&mut o, p);
        }
        OpIn::Knots { ivl, want } => {
            let iv = ivl.map(|[x, y]| Extent1d::from_min_max(x, y));
            let (kn, m) = tri!(o, civ.knots(iv.as_ref()));
            o.push(0.0);
            o.push(kn.len() as f64);
            o.extend_from_slice(&kn);
            if *want {
                o.push(m.len() as f64);
                o.extend(m.iter().map(|&x| x as f64));
            }
            put_cache(&mut o, &civ);
        }
        OpIn::Mults { knots } => {
            let m = tri!(o, civ.generate_mults(knots));
            o.push(0.0);
            o.extend(m.iter().map(|&x| x as f64));
        }
        OpIn::Nnk => {
            let n = tri!(o, civ.number_natural_knots());
            o.extend([0.0, n as f64]);
            put_cache(&mut o, &civ);
        }
        OpIn::Conts { angle } => {
            let (mn, cs) = tri!(o, civ.calculate_continuities(*angle));
            o.extend([0.0, f64::from(mn as i32), cs.len() as f64]);
            o.extend(cs.iter().map(|&x| f64::from(x as i32)));
            put_cache(&mut o, &civ);
        }
        OpIn::Equal { other } => {
            let c = Curve::from(civ);
            let eq = match other {
                None => tri!(o, c.approx_eq(&c)),
                Some(oc) => {
                    let d = Curve::from(build(oc)?);
                    tri!(o, c.approx_eq(&d))
                }
            };
            o.extend([0.0, b(eq)]);
        }
        OpIn::Copy => {
            let c = Curve::from(civ);
            let d = tri!(o, c.copy(&Kernel::default()));
            o.push(0.0);
            put_state(&mut o, civ_of(&d)?);
        }
        OpIn::EditParam { a, b: bb } => {
            let r = civ.edit_parameterization(&Kernel::default(), &Extent1d::from_min_max(*a, *bb));
            o.push(match st_of(&r) {
                Ok(s) => s,
                Err(d) => return Ok(*d),
            });
            put_state(&mut o, &civ);
        }
        OpIn::Reverse { a, b: bb } => {
            let r = civ.reverse_parameterization(&Kernel::default(), &Extent1d::from_min_max(*a, *bb));
            o.push(match st_of(&r) {
                Ok(s) => s,
                Err(d) => return Ok(*d),
            });
            if let Ok(nv) = r {
                o.extend([nv.min(), nv.max()]);
            }
            put_state(&mut o, &civ);
        }
        OpIn::Trim { a, b: bb, mode } => {
            let mode = match mode {
                0 => TrimMode::Notify,
                1 => TrimMode::Silent,
                _ => TrimMode::IntervalOnly,
            };
            let mut ivl = Extent1d::from_min_max(*a, *bb);
            let r = civ.trim(&k, &mut ivl, mode);
            o.push(match st_of(&r) {
                Ok(s) => s,
                Err(d) => return Ok(*d),
            });
            o.extend([ivl.min(), ivl.max()]);
            put_state(&mut o, &civ);
        }
        OpIn::Transform {
            origin,
            x,
            y,
            scale,
        } => {
            let ax = Axis2Placement::new(v3(*origin), v3(*x), v3(*y));
            let r = civ.transform(&ax, scale.map(v3));
            o.push(match st_of(&r) {
                Ok(s) => s,
                Err(d) => return Ok(*d),
            });
            put_state(&mut o, &civ);
        }
        OpIn::ReadDb { .. } => return Err(harness("static op")),
    }
    Ok(to_output(&o))
}

// ---- generation -------------------------------------------------------------------

fn rp(rng: &mut Rng, s: f64) -> [f64; 3] {
    rng.point3(-s, s)
}

/// A random clamped B-spline (sometimes rational, 2D or with repeated interior knots).
fn gen_bspline(rng: &mut Rng) -> BSplineIn {
    let p = 1 + rng.below(4) as usize;
    let n = p + rng.below(6) as usize;
    let rational = rng.chance(0.25);
    let two_d = rng.chance(0.2);
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
    let dim = if two_d || rng.chance(0.15) { 2 } else { 3 };
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
    HermiteIn {
        dim: 3,
        p1: rp(rng, s),
        d1: rp(rng, s),
        p2: rp(rng, s),
        d2: rp(rng, s),
    }
}

fn gen_curve(rng: &mut Rng) -> CurveIn {
    if rng.chance(0.8) {
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

fn knots_of(c: &CurveIn) -> Vec<f64> {
    match c {
        CurveIn::Bspline(bs) => {
            let mut k = bs.u.clone();
            k.dedup();
            k
        }
        CurveIn::Hermite(_) => vec![0.0, 1.0],
    }
}

/// A parameter of `c`: interior, at a knot, at an end, or slightly outside.
fn gen_t(rng: &mut Rng, c: &CurveIn) -> f64 {
    let (a, bb) = range_of(c);
    match rng.below(10) {
        0 => a,
        1 => bb,
        2 => *rng.choose(&knots_of(c)).unwrap_or(&a),
        3 => a + (bb - a) * rng.uniform(-0.3, 1.3),
        _ => a + (bb - a) * rng.unit(),
    }
}

/// A random break cache: the curve knots plus a few extra breaks (sometimes mismatched
/// continuity counts, sometimes not current).
fn gen_cache(rng: &mut Rng, c: &CurveIn) -> CacheIn {
    let (a, bb) = range_of(c);
    let mut breaks = knots_of(c);
    for _ in 0..rng.below(4) {
        breaks.push(a + (bb - a) * rng.unit());
    }
    breaks.sort_by(f64::total_cmp);
    breaks.dedup();
    if rng.chance(0.15) {
        // drop the first or last break
        if rng.chance(0.5) {
            breaks.pop();
        } else if !breaks.is_empty() {
            breaks.remove(0);
        }
    }
    if rng.chance(0.08) {
        breaks.truncate(rng.below(3) as usize);
    }
    let mut nc = breaks.len();
    if rng.chance(0.1) {
        nc = rng.below(breaks.len() as u64 + 2) as usize;
    }
    let conts = (0..nc)
        .map(|_| {
            if rng.chance(0.05) {
                0
            } else {
                1 + rng.below(13) as i64
            }
        })
        .collect();
    CacheIn {
        need: rng.chance(0.1),
        breaks,
        conts,
    }
}

fn gen_civ(rng: &mut Rng) -> CivIn {
    let curve = gen_curve(rng);
    let cache = if rng.chance(0.8) {
        Some(gen_cache(rng, &curve))
    } else {
        None
    };
    CivIn {
        in_param: rng.chance(0.5),
        copy: rng.below(4),
        owner: if rng.chance(0.85) {
            rng.below(4)
        } else {
            rng.below(8)
        },
        cache,
        curve,
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let civ = gen_civ(rng);
    let c = &civ.curve;
    let (a, bb) = range_of(c);
    let op = match rng.below(26) {
        0 | 1 => OpIn::Info {
            types: vec![
                CrvInVolume::TYPE,
                14001,
                14033,
                14034,
                10001,
                rng.below(30000) as i64,
            ],
        },
        2 => OpIn::Eval {
            t: gen_t(rng, c),
            n: rng.below(6) as usize,
            left: rng.chance(0.5),
            nonzero: rng.chance(0.5),
        },
        3 => OpIn::EvalPoint { t: gen_t(rng, c) },
        4..=7 => {
            let ivl = if rng.chance(0.6) {
                let x = a + (bb - a) * rng.uniform(-0.2, 1.2);
                let y = a + (bb - a) * rng.uniform(-0.2, 1.2);
                Some([x.min(y), x.max(y)])
            } else {
                None
            };
            OpIn::Knots {
                ivl,
                want: rng.chance(0.7),
            }
        }
        8..=11 => {
            let mut knots: Vec<f64> = match &civ.cache {
                Some(k) if rng.chance(0.6) => k
                    .breaks
                    .iter()
                    .copied()
                    .filter(|_| rng.chance(0.7))
                    .collect(),
                _ => (0..rng.below(5)).map(|_| gen_t(rng, c)).collect(),
            };
            knots.sort_by(f64::total_cmp);
            if rng.chance(0.1) {
                knots.reverse();
            }
            OpIn::Mults { knots }
        }
        12 | 13 => OpIn::Nnk,
        14 | 15 => OpIn::Conts {
            angle: if rng.chance(0.5) { 0.5 } else { rng.uniform(0.0, 10.0) },
        },
        16 | 17 => OpIn::Equal {
            other: match rng.below(20) {
                0..=2 => None,
                3..=9 => {
                    let mut o = civ.clone();
                    o.in_param = !o.in_param;
                    Some(o)
                }
                10..=14 => Some(gen_civ(rng)),
                _ => Some(civ.clone()),
            },
        },
        18 | 19 => OpIn::Copy,
        20 => {
            let x = rng.uniform(-10.0, 10.0);
            OpIn::EditParam {
                a: x,
                b: x + rng.uniform(0.01, 20.0) * if rng.chance(0.1) { -1.0 } else { 1.0 },
            }
        }
        21 => OpIn::Reverse {
            a: a + (bb - a) * rng.uniform(0.0, 0.5),
            b: a + (bb - a) * rng.uniform(0.5, 1.0),
        },
        22 | 23 => {
            let x = a + (bb - a) * rng.uniform(-0.1, 0.6);
            let y = a + (bb - a) * rng.uniform(0.4, 1.1);
            OpIn::Trim {
                a: x,
                b: y,
                mode: rng.below(3) as i64,
            }
        }
        _ => {
            let identity = rng.chance(0.6);
            OpIn::Transform {
                origin: if identity { [0.0; 3] } else { rp(rng, 5.0) },
                x: [1.0, 0.0, 0.0],
                y: [0.0, 1.0, 0.0],
                scale: if rng.chance(0.5) {
                    None
                } else if identity && rng.chance(0.5) {
                    Some([1.0, 1.0, 1.0])
                } else {
                    Some([2.0, 2.0, 2.0])
                },
            }
        }
    };
    Inputs { civ: Some(civ), op }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "curve.crv_in_volume"
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
