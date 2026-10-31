//! Runner for unit `curve.conic`: rational parametric 2D conics.
//!
//! C++: `solidcore_ffi::units::curve_conic` (upstream `SmConic`,
//! `source/SMLib/inc/SmConic.h`, `source/SMLib/src/SmConic.cpp`).
//! Rust: `solidcore::curve::conic`.
//!
//! Every case builds conic `a` (and `b` for `eq` / `intersect_conic`) from a type
//! (`-1` = the empty constructor, `0..=4` = `SmConicType`), constants `a`, `b` and
//! a list of `Transform2D(angle, (tx, ty))` calls, then runs one operation `op`
//! with parameter `t`, integer `n`, flag bits `flags`, extra values `w` and (for
//! `read_db`) the database text `data` (`binary`: whitespace separated values, the
//! first written as a little-endian u64, the rest as doubles). The result `r` is
//! the operation's values with booleans as `1.0`/`0.0`; an operation whose
//! upstream status is not `SM_SUCCESS` is compared as the error `status_<code>`.
//! Every run uses at least [`MIN_GENERATED`] generated inputs (more with `--seeds`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::conic::{Conic, ConicType};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::vector2d::Vec2;
use solidcore::geom::vector3d::Vec3;
use solidcore::topology::database_io::{DatabaseReader, FileType};
use solidcore::SolidError;
use solidcore_ffi::units::curve_conic::{self as ffi, Built, Op, Operands};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::flag;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// Upstream `SM_SUCCESS`.
const SM_SUCCESS: f64 = 1000.0;

use solidcore::error::SM_ERR_INVALID_INPUT;

/// `curve.conic` runner.
pub struct Runner;

/// Operation names used in `inputs.op`, and whether the result starts with a
/// status.
const OPS: [(&str, Op, bool); 13] = [
    ("state", Op::State, false),
    ("evaluate", Op::Evaluate, true),
    ("evaluate_point", Op::EvaluatePoint, true),
    ("implicit", Op::Implicit, true),
    ("intersect_implicit", Op::IntersectImplicit, true),
    ("intersect_conic", Op::IntersectConic, true),
    ("invert", Op::Invert, true),
    ("knots", Op::Knots, true),
    ("continuities", Op::Continuities, true),
    ("eq", Op::Eq, false),
    ("is_kind_of", Op::IsKindOf, false),
    ("type_strings", Op::TypeStrings, false),
    ("read_db", Op::ReadDb, true),
];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ConicIn {
    #[serde(rename = "type", default)]
    conic_type: i32,
    #[serde(default)]
    a: f64,
    #[serde(default)]
    b: f64,
    #[serde(default)]
    transforms: Vec<[f64; 3]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    op: String,
    #[serde(default)]
    a: ConicIn,
    #[serde(default)]
    b: ConicIn,
    #[serde(default)]
    t: f64,
    #[serde(default)]
    n: i64,
    #[serde(default)]
    flags: i32,
    #[serde(default)]
    w: [f64; 6],
    #[serde(default)]
    data: String,
    #[serde(default)]
    binary: bool,
}

fn lookup(name: &str) -> Result<(Op, bool), HarnessError> {
    OPS.iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, op, st)| (*op, *st))
        .ok_or_else(|| HarnessError::new(format!("unknown op {name}")))
}

fn status_code(e: &SolidError) -> i64 {
    match e {
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => i64::from(SM_ERR_INVALID_INPUT),
        SolidError::Upstream { code, .. } => i64::from(*code),
        _ => -1,
    }
}

/// The database bytes of a case: `data` as text, or (`binary`) as whitespace
/// separated values, the first a little-endian u64, the others doubles.
fn db_bytes(i: &Inputs) -> Result<Vec<u8>, HarnessError> {
    if !i.binary {
        return Ok(i.data.clone().into_bytes());
    }
    let mut out = Vec::new();
    for (k, tok) in i.data.split_whitespace().enumerate() {
        if k == 0 {
            let v: u64 = tok
                .parse()
                .map_err(|_| HarnessError::new(format!("bad binary u64 {tok}")))?;
            out.extend(v.to_le_bytes());
        } else {
            let v: f64 = tok
                .parse()
                .map_err(|_| HarnessError::new(format!("bad binary double {tok}")))?;
            out.extend(v.to_le_bytes());
        }
    }
    Ok(out)
}

fn ffi_built(c: &ConicIn) -> Built {
    Built {
        conic_type: c.conic_type,
        a: c.a,
        b: c.b,
        transforms: c.transforms.clone(),
    }
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (op, has_status) = lookup(&i.op)?;
    let x = Operands {
        t: i.t,
        n: i.n,
        flags: i.flags,
        w: i.w,
        data: db_bytes(i)?,
        file_type: i32::from(i.binary),
    };
    match ffi::op(op, &ffi_built(&i.a), &ffi_built(&i.b), &x) {
        Ok(v) => {
            if has_status {
                if let Some(&s) = v.first() {
                    if s != SM_SUCCESS {
                        return Ok(Output::err(format!("status_{}", s as i64)));
                    }
                }
            }
            Ok(Output::new().values("r", v))
        }
        Err(FfiError::Status(s)) if s > 0 => Ok(Output::err(format!("status_{s}"))),
        Err(e) => Err(HarnessError::new(e.to_string())),
    }
}

/// Build a conic as the shim does.
fn build(c: &ConicIn) -> Result<Conic, HarnessError> {
    if !(-1..=4).contains(&c.conic_type) {
        return Err(HarnessError::new("conic type"));
    }
    let mut k = if c.conic_type < 0 {
        Conic::empty()
    } else {
        Conic::new(ConicType::from_code(i64::from(c.conic_type)), c.a, c.b)
    };
    for [ang, tx, ty] in &c.transforms {
        k.transform_2d(*ang, Vec2::new(*tx, *ty));
    }
    Ok(k)
}

fn put_v(out: &mut Vec<f64>, v: Vec3) {
    out.extend(v.to_array());
}

fn put_state(out: &mut Vec<f64>, k: &Conic) {
    out.push(k.conic_type().code() as f64);
    out.extend([k.a(), k.b(), k.origin().x, k.origin().y, k.theta_rad()]);
    out.push(k.type_code() as f64);
    out.push(k.dim() as f64);
    let ivl = k.natural_interval();
    out.extend([ivl.min(), ivl.max()]);
}

fn str_codes(out: &mut Vec<f64>, s: &str) {
    out.extend(s.bytes().map(f64::from));
}

fn rust_values(i: &Inputs) -> Result<solidcore::Result<Vec<f64>>, HarnessError> {
    let (op, _) = lookup(&i.op)?;
    let a = build(&i.a)?;
    let mut out = Vec::new();
    match op {
        Op::State => put_state(&mut out, &a),
        Op::Evaluate => {
            let n = usize::try_from(i.n).map_err(|_| HarnessError::new("negative n"))?;
            match a.evaluate(i.t, n, i.flags & 1 != 0) {
                Ok(v) => {
                    out.push(SM_SUCCESS);
                    for p in v {
                        put_v(&mut out, p);
                    }
                }
                Err(e) => return Ok(Err(e)),
            }
        }
        Op::EvaluatePoint => {
            out.push(SM_SUCCESS);
            put_v(&mut out, a.evaluate_point(i.t));
        }
        Op::Implicit => {
            out.push(SM_SUCCESS);
            out.extend(a.compute_implicit_equation());
        }
        Op::IntersectImplicit => {
            let [w0, w1, w2, w3, w4, w5] = i.w;
            match a.intersect_implicit_conic(&[w0, w1, w2, w3, w4, w5]) {
                Ok(r) => {
                    out.push(SM_SUCCESS);
                    out.push(r.len() as f64);
                    out.extend(r);
                }
                Err(e) => return Ok(Err(e)),
            }
        }
        Op::IntersectConic => {
            let b = build(&i.b)?;
            match a.intersect_conic(&b, i.t) {
                Ok(r) => {
                    out.push(SM_SUCCESS);
                    out.push(r.len() as f64);
                    for s in r {
                        out.extend([s.param, s.other_param, s.distance]);
                    }
                }
                Err(e) => return Ok(Err(e)),
            }
        }
        Op::Invert => {
            out.push(SM_SUCCESS);
            match a.invert_point_on_conic(Vec3::new(i.w[0], i.w[1], i.w[2])) {
                Some(p) => out.extend([1.0, p]),
                None => out.push(0.0),
            }
        }
        Op::Knots => {
            let ivl = Extent1d::from_min_max(i.w[0], i.w[1]);
            let (knots, mults) = a.knots((i.flags & 1 != 0).then_some(&ivl));
            out.push(SM_SUCCESS);
            out.push(knots.len() as f64);
            out.extend(knots);
            if i.flags & 2 != 0 {
                out.push(mults.len() as f64);
                out.extend(mults.iter().map(|&m| m as f64));
            } else {
                out.push(0.0);
            }
        }
        Op::Continuities => {
            let (min, conts) = a.calculate_continuities();
            out.push(SM_SUCCESS);
            out.push(f64::from(min as i32));
            out.push(conts.len() as f64);
            out.extend(conts.iter().map(|&c| f64::from(c as i32)));
        }
        Op::Eq => {
            if i.flags & 1 != 0 {
                out.push(flag(a.approx_eq(&a)));
            } else {
                let b = build(&i.b)?;
                out.push(flag(a.approx_eq(&b)));
            }
        }
        Op::IsKindOf => out.push(flag(a.is_kind_of(i.n))),
        Op::TypeStrings => {
            out.push(a.type_code() as f64);
            str_codes(&mut out, a.type_string());
            out.push(-1.0);
            str_codes(&mut out, a.class_string());
        }
        Op::ReadDb => {
            let ft = if i.binary {
                FileType::Binary
            } else {
                FileType::Ascii
            };
            let mut db = DatabaseReader::new();
            db.open_for_read(db_bytes(i)?, ft);
            match Conic::read_from_db(&mut db) {
                Ok(k) => {
                    out.push(SM_SUCCESS);
                    put_state(&mut out, &k);
                    match k.evaluate(0.0, 1, false) {
                        Ok(v) => {
                            for p in v {
                                put_v(&mut out, p);
                            }
                        }
                        Err(e) => return Ok(Err(e)),
                    }
                }
                Err(e) => return Ok(Err(e)),
            }
        }
    }
    Ok(Ok(out))
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    match rust_values(i)? {
        Ok(v) => Ok(Output::new().values("r", v)),
        Err(e) => Ok(Output::unported(&e)
            .unwrap_or_else(|| Output::err(format!("status_{}", status_code(&e))))),
    }
}

// ---- generation ---------------------------------------------------------------------

fn gen_conic(rng: &mut Rng, known: bool) -> ConicIn {
    let conic_type = if known {
        rng.range_i64(0, 3) as i32
    } else {
        *rng.choose(&[0, 0, 1, 1, 2, 3, 3, 4, -1]).unwrap_or(&0)
    };
    let scale = *rng.choose(&[1.0, 1.0, 1.0, 10.0, 0.1, 1e-3, 1e3]).unwrap_or(&1.0);
    let mut a = rng.uniform(0.2, 4.0) * scale;
    let mut b = rng.uniform(0.2, 4.0) * scale;
    if rng.chance(0.1) {
        a = -a;
    }
    if rng.chance(0.05) {
        b = -b;
    }
    if conic_type == 3 && rng.chance(0.5) {
        b = 0.0;
    }
    if conic_type == 0 && rng.chance(0.1) {
        b = a; // a circle
    }
    let nt = *rng.choose(&[0usize, 0, 1, 1, 2, 3]).unwrap_or(&0);
    let mut transforms = Vec::new();
    for _ in 0..nt {
        let ang = match rng.below(6) {
            0 => 0.0,
            1 => core::f64::consts::FRAC_PI_2,
            2 => core::f64::consts::PI,
            _ => rng.uniform(-3.5, 3.5),
        };
        let (tx, ty) = if rng.chance(0.3) {
            (0.0, 0.0)
        } else {
            (rng.uniform(-2.0, 2.0) * scale, rng.uniform(-2.0, 2.0) * scale)
        };
        transforms.push([ang, tx, ty]);
    }
    ConicIn {
        conic_type,
        a,
        b,
        transforms,
    }
}

fn gen_param(rng: &mut Rng, c: &ConicIn) -> f64 {
    match c.conic_type {
        0 => match rng.below(8) {
            0 => 1.0,
            1 => -1.0,
            2 => 3.0,
            3 => 0.0,
            4 => 1.0 + rng.uniform(-1e-12, 1e-12),
            _ => rng.uniform(-1.0, 3.0),
        },
        1 => match rng.below(6) {
            0 => 0.0,
            1 => 1.0 - 1e-6,
            2 => -1.0 + 1e-6,
            3 => rng.uniform(-1.5, 1.5),
            _ => rng.uniform(-0.999, 0.999),
        },
        _ => match rng.below(4) {
            0 => 0.0,
            1 => rng.uniform(-999.0, 999.0),
            _ => rng.uniform(-5.0, 5.0),
        },
    }
}

/// Upstream `IntersectConic` scenarios (`tests/prog_test/src/smCurveTest.cpp`),
/// shuffled: a random member pair of these shapes and transforms.
fn gen_intersect_pair(rng: &mut Rng) -> (ConicIn, ConicIn) {
    let a = gen_conic(rng, true);
    let mut b = gen_conic(rng, true);
    if rng.chance(0.2) {
        // concentric, rotated copy with swapped axes (4 intersections for ellipses)
        b = ConicIn {
            conic_type: a.conic_type,
            a: a.b,
            b: a.a,
            transforms: vec![[rng.uniform(-1.0, 1.0), 0.0, 0.0]],
        };
    }
    if rng.chance(0.05) {
        b = a.clone();
    }
    (a, b)
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let idx = rng.below(OPS.len() as u64) as usize;
    let (name, op, _) = OPS.get(idx).copied().unwrap_or(("state", Op::State, false));
    let mut a = gen_conic(rng, false);
    let mut b = gen_conic(rng, false);
    let mut t = gen_param(rng, &a);
    let mut flags = 0;
    let mut n = 0;
    let mut w = [0.0; 6];
    let mut data = String::new();
    let mut binary = false;
    match op {
        Op::Evaluate => {
            n = *rng.choose(&[0i64, 1, 1, 2, 2, 2, 3]).unwrap_or(&1);
            flags = rng.below(4) as i32;
        }
        Op::IntersectImplicit => {
            a = gen_conic(rng, true);
            if rng.chance(0.8) {
                let other = gen_conic(rng, true);
                if let Ok(k) = build(&other) {
                    w = k.compute_implicit_equation();
                }
            } else {
                for v in &mut w {
                    *v = rng.uniform(-2.0, 2.0);
                }
            }
        }
        Op::IntersectConic => {
            let (pa, pb) = gen_intersect_pair(rng);
            a = pa;
            b = pb;
            t = *rng.choose(&[1e-5, 1e-5, 1e-3, 1e-9]).unwrap_or(&1e-5);
        }
        Op::Invert => {
            if let Ok(k) = build(&a) {
                let p = k.evaluate_point(t);
                let noise = *rng.choose(&[0.0, 0.0, 1e-9, 1e-4, 0.5]).unwrap_or(&0.0);
                w = [
                    p.x + noise * rng.uniform(-1.0, 1.0),
                    p.y + noise * rng.uniform(-1.0, 1.0),
                    if rng.chance(0.1) { 0.3 } else { 0.0 },
                    0.0,
                    0.0,
                    0.0,
                ];
            }
        }
        Op::Knots => {
            flags = rng.below(4) as i32;
            let lo = *rng.choose(&[0.0, -1.0, -999.0, 3.0, -1.0 + 1e-6, -5.0]).unwrap_or(&0.0);
            let hi = *rng.choose(&[1.0, 3.0, 999.0, 0.0, 1.0 - 1e-6, 5.0]).unwrap_or(&1.0);
            w[0] = lo;
            w[1] = hi;
        }
        Op::Eq => {
            flags = i32::from(rng.chance(0.1));
            if rng.chance(0.6) {
                b = a.clone();
                if rng.chance(0.7) {
                    let eps = 10f64.powi(-(rng.range_i64(9, 15) as i32));
                    match rng.below(4) {
                        0 => b.a += eps,
                        1 => b.b += eps,
                        2 => b.transforms.push([eps, 0.0, 0.0]),
                        _ => b.transforms.push([0.0, eps, -eps]),
                    }
                }
            }
        }
        Op::IsKindOf => {
            n = *rng
                .choose(&[14020, 14020, 14020, 14001, 0, 22008, 14034])
                .unwrap_or(&14020);
        }
        Op::ReadDb => {
            let ty = rng.range_i64(0, 4) as u64;
            let theta = rng.uniform(-4.0, 4.0);
            let vals = [
                rng.uniform(-3.0, 3.0),
                rng.uniform(-3.0, 3.0),
                rng.uniform(-2.0, 2.0),
                rng.uniform(-2.0, 2.0),
                if rng.chance(0.3) { 0.0 } else { theta },
            ];
            binary = rng.chance(0.5);
            if binary {
                data = format!("{ty}");
                for v in vals {
                    data.push(' ');
                    data.push_str(&v.to_string());
                }
            } else {
                let [va, vb, ox, oy, th] = vals;
                data = format!(
                    "{ty} SmConic conic type \n{va:e} SmConic A of conic eqn \n{vb:e} SmConic B \n{ox:e} {oy:e} SmConic origin \n{th:e} SmConic rotation\n"
                );
            }
        }
        _ => {}
    }
    Inputs {
        op: name.to_string(),
        a,
        b,
        t,
        n,
        flags,
        w,
        data,
        binary,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "curve.conic"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_case(rng)).ok()
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
