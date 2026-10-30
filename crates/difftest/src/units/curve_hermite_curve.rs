//! Runner for unit `curve.hermite_curve`: cubic Hermite curves.
//!
//! C++: `solidcore_ffi::units::curve_hermite_curve` (upstream `SmHermiteCurve`,
//! `source/SMLib/inc/SmHermiteCurve.h`, `source/SMLib/src/SmHermiteCurve.cpp`).
//! Rust: `solidcore::curve::hermite_curve`.
//!
//! Every case builds curve `a` (and `b` for `eq`) by a construction `kind` from
//! four vectors `v` (start point, start derivative, end point, end derivative) and
//! a dimension `dim`, then runs one operation `op` with parameter `t`, integer `n`,
//! flag bits `flags`, extra values `w` and (for `read_db`) the database text `data`
//! (`binary`: whitespace separated doubles written little-endian). The result `r`
//! is the operation's values with booleans as `1.0`/`0.0` and statuses as their
//! upstream codes. `is_kind_of` of a type other than `SmHermiteCurve_TYPE` reaches
//! the unported `SmCurve::IsKindOf` and is deferred on the Rust side.
//! Every run uses at least [`MIN_GENERATED`] generated inputs (more with `--seeds`).
//!
//! `write_read` cases (their own seed stream, named `wrNNNN`, [`WRITE_READ_CASES`] per
//! run) write curve `a` with `WriteType` + `WriteToDB` (ASCII when `binary` is false,
//! version `n`), read each side's own bytes back through `ReadType` +
//! `SmCurve::ReadFromDB`, and compare the bytes, the read curve's type, dimension and
//! members, `== a`, `EvaluatePoint(t)` and the bytes of a second write. Equal bytes and
//! each reader's result on them stand for the cross reads (C++ bytes read by Rust and
//! the reverse) without passing C++ output to the Rust side.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::geom::assert_array::{AssertArray, ObjectKey};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::polar_box::{PolarBox, PolarBoxState};
use solidcore::geom::pseudo_box::PseudoBox;
use solidcore::geom::vector2d::Vec2;
use solidcore::geom::vector3d::Vec3;
use solidcore::math::UNDEF_DOUBLE;
use solidcore::topology::brep::validation::curve_assert_valid;
use solidcore::topology::database_io::{DatabaseReader, DatabaseWriter, FileType};
use solidcore::topology::geometry_write::write_curve;
use solidcore::{Kernel, SolidError};
use solidcore_ffi::units::curve_hermite_curve::{self as ffi, Built, Kind, Op, Operands};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::assert_reports::put_assert_reports;
use crate::util::flag;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// Upstream `SM_SUCCESS`.
const SM_SUCCESS: f64 = 1000.0;

/// Upstream `SM_ERR_INVALID_INPUT`.
const SM_ERR_INVALID_INPUT: i32 = 1007;

/// `curve.hermite_curve` runner.
pub struct Runner;

/// Construction names used in `inputs.a.kind` / `inputs.b.kind`.
const KINDS: [(&str, Kind); 4] = [
    ("3d", Kind::ThreeD),
    ("2d", Kind::TwoD),
    ("3d_default", Kind::ThreeDDefault),
    ("2d_default", Kind::TwoDDefault),
];

/// Operation names used in `inputs.op`.
const OPS: [(&str, Op); 11] = [
    ("state", Op::State),
    ("evaluate", Op::Evaluate),
    ("evaluate_point", Op::EvaluatePoint),
    ("bbox", Op::Bbox),
    ("chord", Op::Chord),
    ("knots", Op::Knots),
    ("continuities", Op::Continuities),
    ("eq", Op::Eq),
    ("is_kind_of", Op::IsKindOf),
    ("type_strings", Op::TypeStrings),
    ("read_db", Op::ReadDb),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CurveIn {
    kind: String,
    #[serde(default)]
    v: [[f64; 3]; 4],
    #[serde(default = "default_dim")]
    dim: u32,
}

fn default_dim() -> u32 {
    3
}

impl Default for CurveIn {
    fn default() -> Self {
        Self {
            kind: "3d".into(),
            v: [[0.0; 3]; 4],
            dim: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    op: String,
    #[serde(default)]
    a: CurveIn,
    #[serde(default)]
    b: CurveIn,
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

/// The `write_read` op (not in [`OPS`], so the main stream's op choice is unchanged).
const WRITE_READ: (&str, Op) = ("write_read", Op::WriteRead);

/// The `assert_valid` op (not in [`OPS`], so the main stream's op choice is unchanged).
const ASSERT_VALID: (&str, Op) = ("assert_valid", Op::AssertValid);

/// The op named `name`.
fn op_of(name: &str) -> Result<Op, HarnessError> {
    if name == WRITE_READ.0 {
        return Ok(WRITE_READ.1);
    }
    if name == ASSERT_VALID.0 {
        return Ok(ASSERT_VALID.1);
    }
    lookup(&OPS, name)
}

fn lookup<T: Copy>(table: &[(&str, T)], name: &str) -> Result<T, HarnessError> {
    table
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, x)| *x)
        .ok_or_else(|| HarnessError::new(format!("unknown name {name}")))
}

fn status_code(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => f64::from(SM_ERR_INVALID_INPUT),
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

/// The database bytes of a case: `data` as text, or (`binary`) as whitespace
/// separated doubles written little-endian.
fn db_bytes(i: &Inputs) -> Result<Vec<u8>, HarnessError> {
    if !i.binary {
        return Ok(i.data.clone().into_bytes());
    }
    let mut out = Vec::new();
    for tok in i.data.split_whitespace() {
        let v: f64 = tok
            .parse()
            .map_err(|_| HarnessError::new(format!("bad binary double {tok}")))?;
        out.extend(v.to_le_bytes());
    }
    Ok(out)
}

fn ffi_built(c: &CurveIn) -> Result<Built, HarnessError> {
    Ok(Built {
        kind: lookup(&KINDS, &c.kind)?,
        v: c.v,
        dim: c.dim,
    })
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let op = op_of(&i.op)?;
    let a = ffi_built(&i.a)?;
    let b = ffi_built(&i.b)?;
    let x = Operands {
        t: i.t,
        n: i.n,
        flags: i.flags,
        w: i.w,
        data: db_bytes(i)?,
        file_type: i32::from(i.binary),
    };
    match ffi::op(op, &a, &b, &x) {
        Ok(v) => Ok(Output::new().values("r", v)),
        Err(FfiError::Status(s)) if s > 0 => Ok(Output::err(format!("status_{s}"))),
        Err(e) => Err(HarnessError::new(e.to_string())),
    }
}

/// Build a curve as the shim does.
fn build(c: &CurveIn) -> Result<HermiteCurve, HarnessError> {
    let kind = lookup(&KINDS, &c.kind)?;
    let [v0, v1, v2, v3] = c.v.map(Vec3::from);
    let v2d = |v: [f64; 3]| Vec2::new(v[0], v[1]);
    let [w0, w1, w2, w3] = c.v.map(v2d);
    Ok(match kind {
        Kind::ThreeD => HermiteCurve::new(v0, v1, v2, v3, u64::from(c.dim)),
        Kind::ThreeDDefault => HermiteCurve::new(v0, v1, v2, v3, 3),
        Kind::TwoD => HermiteCurve::from_2d(w0, w1, w2, w3, u64::from(c.dim)),
        Kind::TwoDDefault => HermiteCurve::from_2d(w0, w1, w2, w3, 2),
    })
}

fn put_v(out: &mut Vec<f64>, v: Vec3) {
    out.extend(v.to_array());
}

fn put_members(out: &mut Vec<f64>, h: &HermiteCurve) {
    let (c, d) = h.power_basis();
    for v in [
        h.start_point(),
        h.start_derivative(),
        h.end_point(),
        h.end_derivative(),
        c,
        d,
    ] {
        put_v(out, v);
    }
}

fn str_codes(out: &mut Vec<f64>, s: &str) {
    out.extend(s.bytes().map(f64::from));
}

/// `WriteType` + `WriteToDB` of `c` (as the shim's `write_record`).
fn write_record(c: &Curve, ft: FileType, version: u64) -> solidcore::Result<Vec<u8>> {
    let mut w = DatabaseWriter::open_for_write(ft);
    w.write_type(c.type_code(), Some(c.dim()));
    write_curve(c, &mut w, version)?;
    Ok(w.into_bytes())
}

fn put_bytes(out: &mut Vec<f64>, b: &[u8]) {
    out.push(b.len() as f64);
    out.extend(b.iter().map(|&x| f64::from(x)));
}

/// `write_read` (see the module docs). A failed step is its status, as the shim's.
fn rust_write_read(i: &Inputs, a: HermiteCurve, out: &mut Vec<f64>) -> solidcore::Result<()> {
    let k = Kernel::default();
    let ft = if i.binary {
        FileType::Binary
    } else {
        FileType::Ascii
    };
    let version = u64::try_from(i.n).unwrap_or(0);
    let a = Curve::from(a);
    let bytes = write_record(&a, ft, version)?;
    out.push(SM_SUCCESS);
    put_bytes(out, &bytes);
    let mut db = DatabaseReader::new();
    db.open_for_read(bytes, ft);
    let read = db.read_type(true).and_then(|h| {
        let (Some(t), Some(d)) = (h.type_code, h.dim) else {
            return Err(SolidError::invalid_input("difftest", "type header"));
        };
        Curve::read_from_db(&k, t, &mut db, d, version)
    });
    let c = match read {
        Ok(c) => c,
        Err(e) if e.is_unported() => return Err(e),
        Err(e) => {
            out.push(status_code(&e));
            return Ok(());
        }
    };
    out.push(SM_SUCCESS);
    out.push(c.type_code() as f64);
    out.push(c.dim() as f64);
    let CurveKind::Hermite(h) = c.kind() else {
        return Err(SolidError::invalid_input("difftest", "not a Hermite curve"));
    };
    put_members(out, h);
    out.push(flag(c.approx_eq(&k, &a)?));
    match c.evaluate_point(&k, i.t) {
        Ok(p) => {
            out.push(SM_SUCCESS);
            put_v(out, p);
        }
        Err(e) if e.is_unported() => return Err(e),
        Err(e) => out.push(status_code(&e)),
    }
    match write_record(&c, ft, version) {
        Ok(again) => {
            out.push(SM_SUCCESS);
            put_bytes(out, &again);
        }
        Err(e) if e.is_unported() => return Err(e),
        Err(e) => out.push(status_code(&e)),
    }
    Ok(())
}

fn rust_values(i: &Inputs) -> Result<solidcore::Result<Vec<f64>>, HarnessError> {
    let op = op_of(&i.op)?;
    let a = build(&i.a)?;
    let mut out = Vec::new();
    match op {
        Op::AssertValid => {
            let k = Kernel::default();
            let mut c = Curve::from(a);
            let mut list = AssertArray::new();
            let level = u8::try_from(i.n).map_err(|e| HarnessError::new(e.to_string()))?;
            let with_list = i.flags & 1 != 0;
            let key = ObjectKey(1);
            match curve_assert_valid(&k, &mut c, with_list.then_some(&mut list), level, key) {
                Ok(ok) => put_assert_reports(&mut out, ok, &list, key),
                Err(e) => return Ok(Err(e)),
            }
        }
        Op::State => {
            out.push(a.type_code() as f64);
            out.push(a.dim() as f64);
            for p in a.bezier_points() {
                put_v(&mut out, p);
            }
            let ivl = a.natural_interval();
            out.extend([ivl.min(), ivl.max()]);
            put_members(&mut out, &a);
        }
        Op::Evaluate => {
            let n = usize::try_from(i.n).map_err(|_| HarnessError::new("negative n"))?;
            // `HermiteCurve::evaluate` is infallible and sizes its output by `n`; the
            // FFI rejects orders above its own bound, so a larger one is not a case
            if n > solidcore::curve::MAX_EVAL_DERIVS {
                return Err(HarnessError::new("derivative order above MAX_EVAL_DERIVS"));
            }
            out.push(SM_SUCCESS);
            for v in a.evaluate(i.t, n, i.flags & 1 != 0) {
                put_v(&mut out, v);
            }
        }
        Op::EvaluatePoint => {
            out.push(SM_SUCCESS);
            put_v(&mut out, a.evaluate_point(i.t));
        }
        Op::Bbox => {
            let [w0, w1, w2, w3, w4, w5] = i.w;
            let mut nb = Extent3d::new();
            let mut pb = PseudoBox::new();
            if i.flags & 8 != 0 {
                pb.add_point(Vec3::new(w0, w1, w2));
            }
            let mut xb = if i.flags & 16 != 0 {
                PolarBox::from_vector(Vec3::new(w3, w4, w5))
            } else {
                PolarBox::new()
            };
            let r = a.calculate_bounding_box(
                (i.flags & 1 != 0).then_some(&mut nb),
                (i.flags & 2 != 0).then_some(&mut pb),
                (i.flags & 4 != 0).then_some(&mut xb),
            );
            if let Err(e) = r {
                return Ok(Err(e));
            }
            out.push(SM_SUCCESS);
            if i.flags & 1 != 0 {
                out.extend([nb.u_min(), nb.v_min(), nb.w_min(), nb.u_max(), nb.v_max(), nb.w_max()]);
            }
            if i.flags & 2 != 0 {
                for b in pb.basis() {
                    put_v(&mut out, *b);
                }
                for ivl in pb.intervals() {
                    out.extend([ivl.min(), ivl.max()]);
                }
            }
            if i.flags & 4 != 0 {
                let e = xb.state();
                out.push(f64::from(e.code()));
                if e != PolarBoxState::Uninitialized || i.flags & 16 != 0 {
                    for b in xb.basis() {
                        put_v(&mut out, *b);
                    }
                    let d = xb.domain();
                    out.extend([d.u_min(), d.v_min(), d.u_max(), d.v_max()]);
                }
            }
        }
        Op::Chord => out.push(a.chord_height()),
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
        Op::IsKindOf => match a.is_kind_of(i.n) {
            Ok(b) => out.push(flag(b)),
            Err(e) => return Ok(Err(e)),
        },
        Op::TypeStrings => {
            out.push(a.type_code() as f64);
            str_codes(&mut out, a.type_string());
            out.push(-1.0);
            str_codes(&mut out, a.class_string());
        }
        Op::WriteRead => {
            if let Err(e) = rust_write_read(i, a, &mut out) {
                return Ok(Err(e));
            }
        }
        Op::ReadDb => {
            let ft = if i.binary {
                FileType::Binary
            } else {
                FileType::Ascii
            };
            let mut db = DatabaseReader::new();
            db.open_for_read(db_bytes(i)?, ft);
            match HermiteCurve::read_from_db(&mut db) {
                Ok(h) => {
                    out.push(SM_SUCCESS);
                    out.push(h.type_code() as f64);
                    out.push(h.dim() as f64);
                    put_members(&mut out, &h);
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
            .unwrap_or_else(|| Output::err(format!("status_{}", status_code(&e) as i64)))),
    }
}

// ---- generation ---------------------------------------------------------------------

fn vec3(rng: &mut Rng, scale: f64) -> [f64; 3] {
    let p = rng.point3(-scale, scale);
    if rng.chance(0.15) {
        // a planar curve
        [p[0], p[1], 0.0]
    } else {
        p
    }
}

fn gen_curve(rng: &mut Rng) -> CurveIn {
    let idx = rng.below(KINDS.len() as u64) as usize;
    let (name, _) = KINDS.get(idx).copied().unwrap_or(("3d", Kind::ThreeD));
    let scale = *rng.choose(&[1.0, 10.0, 1000.0, 1e-3]).unwrap_or(&1.0);
    let p1 = vec3(rng, scale);
    let mut d1 = vec3(rng, scale);
    let mut p2 = vec3(rng, scale);
    let mut d2 = vec3(rng, scale);
    // degenerate shapes
    match rng.below(10) {
        0 => d1 = [0.0; 3],
        1 => d2 = [0.0; 3],
        2 => p2 = p1,
        3 => {
            // tiny start derivative (below the non-zero-tangent threshold)
            let e = 1e-14 * rng.uniform(0.0, 1.0);
            d1 = [e, -e, 0.5 * e];
        }
        4 => {
            // a straight line: derivatives along the chord
            let c = [p2[0] - p1[0], p2[1] - p1[1], p2[2] - p1[2]];
            let s1 = rng.uniform(0.0, 3.0);
            let s2 = rng.uniform(0.0, 3.0);
            d1 = [c[0] * s1, c[1] * s1, c[2] * s1];
            d2 = [c[0] * s2, c[1] * s2, c[2] * s2];
        }
        5 => {
            // a cusp: zero first derivative at t = 0.5 needs D1 = -D2 and P1 = P2
            // shifted along it; use the symmetric case P2 - P1 = (D1 + D2) / 4 -> 0
            d2 = [-d1[0], -d1[1], -d1[2]];
            p2 = p1;
        }
        _ => {}
    }
    CurveIn {
        kind: name.to_string(),
        v: [p1, d1, p2, d2],
        dim: *rng.choose(&[2u32, 3, 3, 3]).unwrap_or(&3),
    }
}

fn fmt_line(v: [f64; 3], label: &str) -> String {
    format!("{:e} {:e} {:e} {label}\n", v[0], v[1], v[2])
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let idx = rng.below(OPS.len() as u64) as usize;
    let (name, op) = OPS.get(idx).copied().unwrap_or(("state", Op::State));
    let a = gen_curve(rng);
    let mut b = gen_curve(rng);
    let t = match rng.below(8) {
        0 => 0.0,
        1 => 1.0,
        2 => 0.5,
        3 => rng.uniform(-1.0, 2.0),
        4 => 0.5 + rng.uniform(-1e-12, 1e-12),
        _ => rng.unit(),
    };
    let mut flags = 0;
    let mut n = 0;
    let mut w = [0.0; 6];
    let mut data = String::new();
    let mut binary = false;
    match op {
        Op::Evaluate => {
            n = rng.range_i64(0, 7);
            flags = rng.below(4) as i32;
        }
        Op::Bbox => {
            flags = 1 + rng.below(31) as i32;
            if !matches!(flags & 7, 1..=7) {
                flags |= 2;
            }
            let p = rng.point3(-5.0, 5.0);
            let q = rng.point3(-1.0, 1.0);
            w = [p[0], p[1], p[2], q[0], q[1], q[2]];
        }
        Op::Knots => {
            flags = rng.below(4) as i32;
            let lo = *rng.choose(&[0.0, -1.0, 0.5, 1.0, 1e-17, 1.0 + 1e-15]).unwrap_or(&0.0);
            let hi = *rng.choose(&[1.0, 2.0, 0.5, 0.0, 1.0 - 1e-16]).unwrap_or(&1.0);
            w[0] = lo;
            w[1] = hi;
        }
        Op::Eq => {
            flags = i32::from(rng.chance(0.1));
            if rng.chance(0.6) {
                b = a.clone();
                if rng.chance(0.5) {
                    b.dim = *rng.choose(&[2u32, 3]).unwrap_or(&3);
                }
                if rng.chance(0.7) {
                    let eps = 10f64.powi(-(rng.range_i64(9, 15) as i32));
                    let (vi, ci) = (rng.below(4) as usize, rng.below(3) as usize);
                    if let Some(c) = b.v.get_mut(vi).and_then(|v| v.get_mut(ci)) {
                        *c += eps * rng.uniform(-1.0, 1.0);
                    }
                }
            }
        }
        Op::IsKindOf => {
            n = *rng
                .choose(&[14034, 14034, 14034, 14001, 0, 22008, 14035])
                .unwrap_or(&14034);
        }
        Op::ReadDb => {
            let h = gen_curve(rng);
            let c = build(&h).unwrap_or_default();
            let (pc, pd) = c.power_basis();
            let vals = [
                c.start_point(),
                c.start_derivative(),
                c.end_point(),
                c.end_derivative(),
                pc,
                pd,
            ];
            binary = rng.chance(0.5);
            for v in vals {
                let arr = v.to_array();
                if binary {
                    for x in arr {
                        data.push_str(&x.to_string());
                        data.push(' ');
                    }
                } else {
                    data.push_str(&fmt_line(arr, "SmHermiteCurve"));
                }
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

/// Generated `write_read` cases (their own seed stream, `<unit>:write_read`).
pub const WRITE_READ_CASES: usize = 300;

/// A `write_read` case: a curve of any construction and scale (with huge, tiny,
/// negative-zero and degenerate members), either format, versions 30 and 33.
fn gen_write_read(rng: &mut Rng) -> Inputs {
    let mut a = gen_curve(rng);
    match rng.below(8) {
        0 => {
            // huge and tiny magnitudes (the ASCII fixed format's digit counts)
            let s = *rng.choose(&[1e12, 1e-12, 1e-300, 1e150]).unwrap_or(&1e12);
            for v in &mut a.v {
                *v = v.map(|x| x * s);
            }
        }
        1 => {
            // negative zeros and exact zeros
            if let Some(v) = a.v.get_mut(rng.below(4) as usize) {
                *v = [-0.0, 0.0, -0.0];
            }
        }
        _ => {}
    }
    let t = match rng.below(5) {
        0 => 0.0,
        1 => 1.0,
        2 => rng.uniform(-1.0, 2.0),
        _ => rng.unit(),
    };
    Inputs {
        op: WRITE_READ.0.to_string(),
        a,
        b: CurveIn::default(),
        t,
        n: if rng.chance(0.3) { 33 } else { 30 },
        flags: 0,
        w: [0.0; 6],
        data: String::new(),
        binary: rng.chance(0.5),
    }
}

/// Generated `assert_valid` cases (their own seed stream, `<unit>:assert_valid`).
pub const ASSERT_VALID_CASES: usize = 300;

/// An `assert_valid` case: a curve of any construction (valid, or with undefined
/// members or a dimension other than 2 and 3), any test level, with or without a
/// report list.
fn gen_assert_valid(rng: &mut Rng) -> Inputs {
    let mut a = gen_curve(rng);
    match rng.below(6) {
        0 | 1 => {
            // an undefined member component (SM_UNDEF_DOUBLE)
            let vi = rng.below(4) as usize;
            let ci = rng.below(3) as usize;
            if let Some(c) = a.v.get_mut(vi).and_then(|v| v.get_mut(ci)) {
                *c = UNDEF_DOUBLE;
            }
        }
        2 => {
            // a dimension neither 2 nor 3 (only the ThreeD / TwoD constructions use it)
            a.dim = *rng.choose(&[0u32, 1, 4]).unwrap_or(&4);
        }
        _ => {}
    }
    Inputs {
        op: ASSERT_VALID.0.to_string(),
        b: CurveIn::default(),
        t: 0.0,
        n: *rng.choose(&[0i64, 1, 2, 4]).unwrap_or(&0),
        // a report list only where test 1 stays inside the default label table (the
        // ffi wrapper rejects the rest)
        flags: i32::from(rng.chance(0.8) && ffi_built(&a).is_ok_and(|b| matches!(b.effective_dim(), 2 | 3))),
        a,
        w: [0.0; 6],
        data: String::new(),
        binary: false,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "curve.hermite_curve"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_case(rng)).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let main = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        let mut v = default_cases(self, &main)?;
        // write/read round trips: their own seed stream (the other generated cases are
        // unchanged); named `wrNNNN`
        let wr = CaseContext {
            seeds: ctx.seeds.max(WRITE_READ_CASES),
            ..ctx.clone()
        };
        for mut c in wr.generated_cases("curve.hermite_curve:write_read", |rng| {
            serde_json::to_value(gen_write_read(rng)).ok()
        }) {
            c.name = c.name.replacen("gen", "wr", 1);
            c.unit = self.id().to_string();
            v.push(c);
        }
        // AssertValid with native report comparison: their own seed stream; `avNNNN`
        let av = CaseContext {
            seeds: ctx.seeds.max(ASSERT_VALID_CASES),
            ..ctx.clone()
        };
        for mut c in av.generated_cases("curve.hermite_curve:assert_valid", |rng| {
            serde_json::to_value(gen_assert_valid(rng)).ok()
        }) {
            c.name = c.name.replacen("gen", "av", 1);
            c.unit = self.id().to_string();
            v.push(c);
        }
        Ok(v)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}
