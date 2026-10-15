//! Runner for unit `curve.iso_curve`: `SmIsoCurve` (the constant-`u` / constant-`v`
//! curve of a surface) on `SmBSplineSurface` base surfaces: construction with and
//! without clamping, natural interval, knots, equality, type queries, evaluation,
//! continuities, the UV iso line and reading from a database stream.
//!
//! C++: `solidcore_ffi::units::curve_iso_curve` (upstream
//! `source/SMLib/src/SmIsoCurve.cpp`). Rust: `solidcore::curve::iso_curve`, through
//! the [`Curve`] dispatch where the curve interface has the method.
//!
//! A case is the curve (`srf`: an inline or `.sms` surface, `null` for a surface without
//! NURB; `sp` the `SmSurfParamType`; `iso`; `from_left`; `clamp`) and an `op`. Both
//! sides produce one flat record `out`: the status first for status-bearing ops
//! (`0` = `SM_SUCCESS`, `1001` `SM_ERR`, `1007` `SM_ERR_INVALID_INPUT`, `-1` on the
//! Rust side for an error upstream cannot return), then the op's outputs on success.
//!
//! Deferred: evaluation (`SmBSplineSurface::Evaluate` / `EvaluatePoint`, units
//! `nurbs.b_spline_surface_n_lib_p2` / `_p1`), continuities
//! (`SmSurface::CalculateContinuities`, `surface.surface_p6`), the UV iso line of a
//! constant-`u` curve (`SmLine`, `curve.line`) and reading (`SmSurface::ReadFromDB`,
//! `surface.surface_p2`). Every run has at least [`MIN_GENERATED`] generated cases.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p2::ISO_CURVE_TYPE;
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::curve::iso_curve::IsoCurve;
use solidcore::geom::extent1d::Extent1d;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface;
use solidcore::topology::database_io::{DatabaseReader, FileType};
use solidcore::SolidError;
use solidcore_ffi::units::curve_iso_curve::{
    self as ffi, CurveBlock, CurveParams, Nurb, Op, Other, Params,
};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `curve.iso_curve` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

/// An inline surface: control net as stored (`[x, y, z, w]`, rows in u), degrees,
/// knots.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InlineSrf {
    net: Vec<Vec<[f64; 4]>>,
    p: usize,
    q: usize,
    u: Vec<f64>,
    v: Vec<f64>,
}

/// A surface input.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum SrfIn {
    /// An upstream `.sms` file (absolute path after fixture expansion).
    File { sms: String },
    /// Inline data.
    Inline(InlineSrf),
}

/// Curve parameters.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Params0 {
    sp: i32,
    iso: f64,
    #[serde(default)]
    from_left: bool,
    #[serde(default = "yes")]
    clamp: bool,
}

fn yes() -> bool {
    true
}

/// A curve: surface (`None` = no NURB) and parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CurveIn {
    srf: Option<SrfIn>,
    #[serde(flatten)]
    params: Params0,
}

/// The other curve of `eq`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum OtherIn {
    Own(CurveIn),
    Itself,
    SameSurface(Params0),
}

/// The iso data and surface written into a database stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DbIn {
    sp: u64,
    iso: f64,
    from_left: i32,
    srf_type: i64,
    srf: SrfIn,
    #[serde(default = "one")]
    kt: u64,
    #[serde(default = "nine")]
    form: u64,
}

fn one() -> u64 {
    1
}

fn nine() -> u64 {
    9
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    State,
    Knots {
        #[serde(default)]
        ivl: Option<[f64; 2]>,
    },
    Evaluate {
        t: f64,
        n: usize,
        from_left: bool,
        non_zero: bool,
    },
    EvaluatePoint {
        t: f64,
    },
    Continuities {
        tol: f64,
    },
    Eq {
        other: OtherIn,
    },
    IsKindOf {
        t: i64,
    },
    TypeStrings,
    UvIsoLine,
    ReadDb {
        binary: bool,
        version: u64,
        db: DbIn,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    curve: Option<CurveIn>,
    #[serde(flatten)]
    op: OpIn,
}

/// Reads an upstream `.sms` text file: `n m`, `p q`, rational flag, `(n+1)(m+1)`
/// points (`x y z`, or `wx wy wz w` when rational, v index fastest), `n+p+2` u knots,
/// `m+q+2` v knots.
fn read_sms(path: &str) -> Result<InlineSrf, HarnessError> {
    let text = std::fs::read_to_string(path).map_err(|e| harness(format!("{path}: {e}")))?;
    let mut it = text.split_whitespace();
    let mut num = || -> Result<f64, HarnessError> {
        it.next()
            .ok_or_else(|| harness(format!("{path}: truncated")))?
            .parse::<f64>()
            .map_err(|e| harness(format!("{path}: {e}")))
    };
    let int = |v: f64| -> Result<usize, HarnessError> {
        if v.fract() != 0.0 || v < 0.0 {
            return Err(harness(format!("{path}: bad count {v}")));
        }
        Ok(v as usize)
    };
    let n = int(num()?)?;
    let m = int(num()?)?;
    let p = int(num()?)?;
    let q = int(num()?)?;
    let rat = num()? != 0.0;
    let mut net = Vec::with_capacity(n + 1);
    for _ in 0..=n {
        let mut row = Vec::with_capacity(m + 1);
        for _ in 0..=m {
            let (x, y, z) = (num()?, num()?, num()?);
            let w = if rat { num()? } else { NOW };
            row.push([x, y, z, w]);
        }
        net.push(row);
    }
    let u = (0..n + p + 2)
        .map(|_| num())
        .collect::<Result<Vec<_>, _>>()?;
    let v = (0..m + q + 2)
        .map(|_| num())
        .collect::<Result<Vec<_>, _>>()?;
    Ok(InlineSrf { net, p, q, u, v })
}

fn resolve(s: &SrfIn) -> Result<InlineSrf, HarnessError> {
    match s {
        SrfIn::File { sms } => read_sms(sms),
        SrfIn::Inline(i) => Ok(i.clone()),
    }
}

/// The database bytes of a `read_db` input: the iso header, the surface type and the
/// surface in `SmBSplineSurface::ReadFromDB`'s layout.
fn db_bytes(binary: bool, version: u64, db: &DbIn) -> Result<Vec<u8>, HarnessError> {
    let s = resolve(&db.srf)?;
    let n = s.net.len() as i64 - 1;
    let m = s.net.first().map_or(0, Vec::len) as i64 - 1;
    let rational = s.net.iter().flatten().any(|c| c[3] != NOW);
    let mut vals: Vec<Vec<f64>> = Vec::new();
    for c in s.net.iter().flatten() {
        if rational {
            vals.push(c.to_vec());
        } else {
            vals.push(c[..3].to_vec());
        }
    }
    let rat: i16 = i16::from(rational);
    let mut b = Vec::new();
    if binary {
        b.extend_from_slice(&db.sp.to_ne_bytes());
        b.extend_from_slice(&db.iso.to_ne_bytes());
        b.extend_from_slice(&db.from_left.to_ne_bytes());
        b.extend_from_slice(&db.srf_type.to_ne_bytes());
        b.extend_from_slice(&n.to_ne_bytes());
        b.extend_from_slice(&m.to_ne_bytes());
        b.extend_from_slice(&(s.p as i16).to_ne_bytes());
        b.extend_from_slice(&(s.q as i16).to_ne_bytes());
        b.extend_from_slice(&rat.to_ne_bytes());
        if version > 32 {
            b.extend_from_slice(&db.kt.to_ne_bytes());
            b.extend_from_slice(&db.form.to_ne_bytes());
        }
        for x in vals.iter().flatten().chain(&s.u).chain(&s.v) {
            b.extend_from_slice(&x.to_ne_bytes());
        }
    } else {
        let mut t = String::new();
        let _ = writeln!(t, "{} SmIsoCurve Surf Param Flag ", db.sp);
        let _ = writeln!(t, "{:.17e} SmIsoCurve IsoParameter ", db.iso);
        let _ = writeln!(t, "{} SmIsoCurve From Left evaluation Flag ", db.from_left);
        t.push_str(" IsoCurve->Surface \n");
        let _ = writeln!(t, "Object Type {}", db.srf_type);
        let _ = write!(t, "{} {}\n{} {}\n{}\n", n, m, s.p, s.q, rat);
        if version > 32 {
            let _ = write!(t, "{}\n{}\n", db.kt, db.form);
        }
        for c in &vals {
            let s: Vec<String> = c.iter().map(|x| format!("{x:.17e}")).collect();
            t.push_str(&s.join(" "));
            t.push('\n');
        }
        for x in s.u.iter().chain(&s.v) {
            let _ = writeln!(t, "{x:.17e}");
        }
        b = t.into_bytes();
    }
    Ok(b)
}

// ---- C++ side ---------------------------------------------------------------------

fn nurb_of(s: &SrfIn) -> Result<Nurb, HarnessError> {
    let i = resolve(s)?;
    Ok(Nurb {
        net: i.net,
        p: i.p,
        q: i.q,
        u: i.u,
        v: i.v,
    })
}

fn params_of(p: &Params0) -> CurveParams {
    CurveParams {
        surf_param: p.sp,
        iso: p.iso,
        from_left: p.from_left,
        clamp: p.clamp,
    }
}

fn block_of(c: &CurveIn) -> Result<CurveBlock, HarnessError> {
    Ok(CurveBlock {
        srf: c.srf.as_ref().map(nurb_of).transpose()?,
        params: params_of(&c.params),
    })
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let dummy = CurveIn {
        srf: None,
        params: Params0 {
            sp: 0,
            iso: 0.0,
            from_left: false,
            clamp: false,
        },
    };
    let a = block_of(i.curve.as_ref().unwrap_or(&dummy))?;
    let (op, params) = match &i.op {
        OpIn::State => (Op::State, Params::None),
        OpIn::Knots { ivl } => (Op::Knots, Params::Knots(ivl.map(|[a, b]| (a, b)))),
        OpIn::Evaluate {
            t,
            n,
            from_left,
            non_zero,
        } => (
            Op::Evaluate,
            Params::Evaluate(*t, *n, *from_left, *non_zero),
        ),
        OpIn::EvaluatePoint { t } => (Op::EvaluatePoint, Params::Value(*t)),
        OpIn::Continuities { tol } => (Op::Continuities, Params::Value(*tol)),
        OpIn::Eq { other } => (
            Op::Eq,
            Params::Eq(match other {
                OtherIn::Own(c) => Other::Own(block_of(c)?),
                OtherIn::Itself => Other::Itself,
                OtherIn::SameSurface(p) => Other::SameSurface(params_of(p)),
            }),
        ),
        OpIn::IsKindOf { t } => (Op::IsKindOf, Params::Type(*t)),
        OpIn::TypeStrings => (Op::TypeStrings, Params::None),
        OpIn::UvIsoLine => (Op::UvIsoLine, Params::None),
        OpIn::ReadDb {
            binary,
            version,
            db,
        } => (
            Op::ReadDb,
            Params::Db(db_bytes(*binary, *version, db)?, *binary, *version as i64),
        ),
    };
    match ffi::op(op, &a, &params) {
        Ok(v) => Ok(Output::new().values("out", v)),
        Err(e) => Err(harness(format!("ffi: {e}"))),
    }
}

// ---- Rust side --------------------------------------------------------------------

fn to_surface(s: &SrfIn) -> Result<BSplineSurface, HarnessError> {
    let i = resolve(s)?;
    let net = i
        .net
        .iter()
        .map(|row| {
            row.iter()
                .map(|c| CPoint::new(c[0], c[1], c[2], c[3]))
                .collect()
        })
        .collect();
    let sur = Surface::new(net, i.p, i.q, i.u.clone(), i.v.clone())
        .map_err(|e| harness(format!("surface: {e}")))?;
    Ok(BSplineSurface::from_nurb(sur, false))
}

fn surf_param(code: i32) -> Result<SurfParamType, HarnessError> {
    Ok(match code {
        0 => SurfParamType::U,
        1 => SurfParamType::V,
        2 => SurfParamType::Unknown,
        3 => SurfParamType::Both,
        4 => SurfParamType::Neither,
        5 => SurfParamType::UMin,
        6 => SurfParamType::VMin,
        7 => SurfParamType::UMax,
        8 => SurfParamType::VMax,
        9 => SurfParamType::Out,
        _ => return Err(harness(format!("bad surface parameter type {code}"))),
    })
}

fn make(
    srf: impl Into<solidcore::surface::surface::Surface>,
    p: &Params0,
) -> Result<IsoCurve, HarnessError> {
    IsoCurve::from_surface(srf.into(), surf_param(p.sp)?, p.iso, p.from_left, p.clamp)
        .map_err(|e| harness(e.to_string()))
}

fn build(c: &CurveIn) -> Result<IsoCurve, HarnessError> {
    let srf = match &c.srf {
        Some(s) => to_surface(s)?,
        None => BSplineSurface::new(),
    };
    make(srf, &c.params)
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

fn push3(o: &mut Vec<f64>, x: f64, y: f64, z: f64) {
    o.extend([x, y, z]);
}

fn state(o: &mut Vec<f64>, c: &Curve) -> Result<(), HarnessError> {
    let CurveKind::Iso(iso) = c.kind() else {
        return Err(harness("not an iso curve"));
    };
    let ivl = c.natural_interval();
    o.extend([
        iso.iso_parameter(),
        f64::from(iso.surf_param() as u8),
        flag(iso.from_left()),
        c.type_code() as f64,
        c.dim() as f64,
        ivl.min(),
        ivl.max(),
    ]);
    Ok(())
}

/// The `SmStatus` of a Rust error; `-1` for one upstream cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

enum Fail {
    Solid(SolidError),
    Harness(HarnessError),
}

impl From<SolidError> for Fail {
    fn from(e: SolidError) -> Self {
        Self::Solid(e)
    }
}

impl From<HarnessError> for Fail {
    fn from(e: HarnessError) -> Self {
        Self::Harness(e)
    }
}

fn rust_op(i: &Inputs, o: &mut Vec<f64>) -> Result<(), Fail> {
    if let OpIn::ReadDb {
        binary,
        version,
        db,
    } = &i.op
    {
        let bytes = db_bytes(*binary, *version, db)?;
        let mut r = DatabaseReader::new();
        r.open_for_read(
            bytes,
            if *binary {
                FileType::Binary
            } else {
                FileType::Ascii
            },
        );
        let c = Curve::read_from_db(
            &solidcore::kernel::Kernel::default(),
            ISO_CURVE_TYPE,
            &mut r,
            3,
            *version,
        )?;
        o.push(0.0);
        state(o, &c)?;
        return Ok(());
    }
    let ci = i.curve.as_ref().ok_or_else(|| harness("missing curve"))?;
    let iso = build(ci)?;
    let c = Curve::from(iso.clone());
    match &i.op {
        OpIn::State => state(o, &c)?,
        OpIn::Knots { ivl } => {
            let e = ivl.map(|[a, b]| Extent1d::from_min_max(a, b));
            let (k, m) = c.knots(e.as_ref())?;
            o.push(0.0);
            o.push(k.len() as f64);
            o.extend_from_slice(&k);
            o.push(m.len() as f64);
            o.extend(m.iter().map(|&x| x as f64));
        }
        OpIn::Evaluate {
            t,
            n,
            from_left,
            non_zero,
        } => {
            let v = c.evaluate(*t, *n, *from_left, *non_zero)?;
            o.push(0.0);
            for p in v.iter().take(n + 1) {
                push3(o, p.x, p.y, p.z);
            }
        }
        OpIn::EvaluatePoint { t } => {
            let p = c.evaluate_point(*t)?;
            o.push(0.0);
            push3(o, p.x, p.y, p.z);
        }
        OpIn::Continuities { tol } => {
            let (min, conts) = c.calculate_continuities(*tol)?;
            o.push(0.0);
            o.push(f64::from(min as u8));
            o.push(conts.len() as f64);
            o.extend(conts.iter().map(|&x| f64::from(x as u8)));
        }
        OpIn::Eq { other } => match other {
            OtherIn::Itself => o.push(flag(c.approx_eq(&c)?)),
            OtherIn::Own(b) => {
                let b = Curve::from(build(b)?);
                o.extend([flag(c.approx_eq(&b)?), flag(b.approx_eq(&c)?)]);
            }
            OtherIn::SameSurface(p) => {
                let b = Curve::from(make(iso.surface().clone(), p)?);
                o.extend([flag(c.approx_eq(&b)?), flag(b.approx_eq(&c)?)]);
            }
        },
        OpIn::IsKindOf { t } => o.push(flag(c.is_kind_of(*t))),
        OpIn::TypeStrings => {
            o.push(c.type_code() as f64);
            o.extend(c.type_string().bytes().map(f64::from));
            o.push(-1.0);
            o.extend(c.class_string().bytes().map(f64::from));
        }
        OpIn::UvIsoLine => {
            let line = iso.create_uv_iso_line()?;
            o.push(0.0);
            o.extend([line.type_code() as f64, line.dim() as f64]);
            let ivl = line.natural_interval();
            o.extend([ivl.min(), ivl.max()]);
            for t in [ivl.min(), ivl.max()] {
                let p = line.evaluate_point(t)?;
                o.push(0.0);
                push3(o, p.x, p.y, p.z);
            }
        }
        OpIn::ReadDb { .. } => return Err(harness("unreachable").into()),
    }
    Ok(())
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let mut o = Vec::new();
    match rust_op(i, &mut o) {
        Ok(()) => Ok(Output::new().values("out", o)),
        Err(Fail::Harness(h)) => Err(h),
        Err(Fail::Solid(e)) => {
            if let Some(d) = Output::unported(&e) {
                return Ok(d);
            }
            // a failing op: only its status (C++ writes no outputs after it)
            Ok(Output::new().values("out", vec![status(&e)]))
        }
    }
}

// ---- generated cases --------------------------------------------------------------

/// A random clamped knot vector of degree `p` on `[a, a + len]` with up to `extra`
/// interior knots (multiplicities up to `p`).
fn gen_knots(rng: &mut Rng, p: usize, extra: usize) -> Vec<f64> {
    let (a, len) = if rng.chance(0.3) {
        (rng.below(5) as f64 - 2.0, 1.0 + rng.below(6) as f64)
    } else {
        (rng.uniform(-5.0, 5.0), rng.uniform(0.25, 10.0))
    };
    let b = a + len;
    let mut inner: Vec<f64> = Vec::new();
    let mut remaining = extra;
    while remaining > 0 {
        let v = if rng.chance(0.4) {
            a + len * (1.0 + rng.below(15) as f64) / 16.0
        } else {
            rng.uniform(a, b)
        };
        if v <= a || v >= b || inner.contains(&v) {
            break;
        }
        let mult = if rng.chance(0.25) {
            1 + rng.below(p.min(remaining) as u64) as usize
        } else {
            1
        };
        inner.extend(std::iter::repeat_n(v, mult));
        remaining -= mult;
    }
    inner.sort_by(f64::total_cmp);
    let mut u = vec![a; p + 1];
    u.extend(inner);
    u.extend(std::iter::repeat_n(b, p + 1));
    u
}

/// A random clamped 3D surface, rational or not.
fn gen_srf(rng: &mut Rng) -> InlineSrf {
    let p = 1 + rng.below(4) as usize;
    let q = 1 + rng.below(4) as usize;
    let extra_u = rng.below(5) as usize;
    let u = gen_knots(rng, p, extra_u);
    let extra_v = rng.below(5) as usize;
    let v = gen_knots(rng, q, extra_v);
    let n = u.len() - p - 2;
    let m = v.len() - q - 2;
    let rational = rng.chance(0.3);
    let net = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    let [x, y, z] = if rng.chance(0.5) {
                        [
                            i as f64 + rng.uniform(-0.3, 0.3),
                            j as f64,
                            rng.uniform(-1.0, 1.0),
                        ]
                    } else {
                        rng.point3(-10.0, 10.0)
                    };
                    if rational {
                        let w = rng.uniform(0.3, 3.0);
                        [x * w, y * w, z * w, w]
                    } else {
                        [x, y, z, NOW]
                    }
                })
                .collect()
        })
        .collect();
    InlineSrf { net, p, q, u, v }
}

fn ends(k: &[f64]) -> (f64, f64) {
    (
        k.first().copied().unwrap_or(0.0),
        k.last().copied().unwrap_or(1.0),
    )
}

/// A parameter in or around `knots`' range: interior, an end, an interior knot, or
/// (with `outside`) beyond an end.
fn gen_param(rng: &mut Rng, knots: &[f64], outside: bool) -> f64 {
    let (a, b) = ends(knots);
    match rng.below(if outside { 7 } else { 5 }) {
        0 => a,
        1 => b,
        2 => *rng.choose(knots).unwrap_or(&a),
        5 => a - rng.uniform(1e-12, 3.0),
        6 => b + rng.uniform(1e-12, 3.0),
        _ => rng.uniform(a, b),
    }
}

fn gen_sp(rng: &mut Rng) -> i32 {
    if rng.chance(0.85) {
        rng.below(2) as i32
    } else {
        2 + rng.below(8) as i32
    }
}

fn gen_params(rng: &mut Rng, s: Option<&InlineSrf>) -> Params0 {
    let sp = gen_sp(rng);
    let iso = match s {
        Some(s) => {
            let k = if sp == 0 { &s.u } else { &s.v };
            gen_param(rng, k, true)
        }
        None => rng.uniform(-2.0, 2.0),
    };
    Params0 {
        sp,
        iso,
        from_left: rng.chance(0.5),
        clamp: rng.chance(0.7),
    }
}

fn gen_curve(rng: &mut Rng, s: Option<&InlineSrf>) -> CurveIn {
    CurveIn {
        srf: s.cloned().map(SrfIn::Inline),
        params: gen_params(rng, s),
    }
}

/// A slightly different copy of `s` (one control point moved by `eps`).
fn perturb(rng: &mut Rng, s: &InlineSrf, eps: f64) -> InlineSrf {
    let mut t = s.clone();
    let i = rng.below(t.net.len() as u64) as usize;
    if let Some(row) = t.net.get_mut(i) {
        let j = rng.below(row.len() as u64) as usize;
        if let Some(c) = row.get_mut(j) {
            c[0] += eps;
        }
    }
    t
}

fn gen_other(rng: &mut Rng, a: &CurveIn, s: Option<&InlineSrf>) -> OtherIn {
    let mut p = a.params;
    match rng.below(10) {
        0 => return OtherIn::Itself,
        1 => {}
        2 => {
            p.iso += rng
                .choose(&[1e-16, 1e-15, 1e-14, 1e-13, 1e-12, 1e-10])
                .copied()
                .unwrap_or(0.0);
        }
        3 => p.from_left = !p.from_left,
        4 => p.sp = gen_sp(rng),
        5 => p.clamp = !p.clamp,
        _ => {
            // a separate surface: equal, slightly or clearly different
            let srf = match s {
                Some(s) if rng.chance(0.4) => Some(s.clone()),
                Some(s) if rng.chance(0.5) => {
                    let eps = rng
                        .choose(&[1e-16, 1e-13, 1e-10, 1e-3])
                        .copied()
                        .unwrap_or(0.0);
                    Some(perturb(rng, s, eps))
                }
                Some(_) if rng.chance(0.2) => None,
                None if rng.chance(0.5) => None,
                Some(_) | None => Some(gen_srf(rng)),
            };
            if rng.chance(0.3) {
                p = gen_params(rng, srf.as_ref());
            }
            return OtherIn::Own(CurveIn {
                srf: srf.map(SrfIn::Inline),
                params: p,
            });
        }
    }
    OtherIn::SameSurface(p)
}

fn gen_db(rng: &mut Rng) -> OpIn {
    let s = gen_srf(rng);
    OpIn::ReadDb {
        binary: rng.chance(0.5),
        version: if rng.chance(0.5) { 30 } else { 33 },
        db: DbIn {
            sp: rng.below(3),
            iso: rng.uniform(-5.0, 5.0),
            from_left: rng.below(2) as i32,
            srf_type: if rng.chance(0.9) {
                BSplineSurface::TYPE
            } else {
                15099
            },
            srf: SrfIn::Inline(s),
            kt: rng.below(5),
            form: rng.below(12),
        },
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    if rng.chance(0.04) {
        return Inputs {
            curve: None,
            op: gen_db(rng),
        };
    }
    let s = if rng.chance(0.05) {
        None
    } else {
        Some(gen_srf(rng))
    };
    let curve = gen_curve(rng, s.as_ref());
    let cross = s.as_ref().map(|s| {
        if curve.params.sp == 0 {
            s.v.clone()
        } else {
            s.u.clone()
        }
    });
    let op = match rng.below(20) {
        0..=3 => OpIn::State,
        4..=7 => OpIn::Knots {
            ivl: if rng.chance(0.4) {
                None
            } else {
                let k = cross.clone().unwrap_or_else(|| vec![0.0, 1.0]);
                Some([gen_param(rng, &k, true), gen_param(rng, &k, true)])
            },
        },
        8..=10 => OpIn::Eq {
            other: gen_other(rng, &curve, s.as_ref()),
        },
        11 => OpIn::IsKindOf {
            t: *rng
                .choose(&[ISO_CURVE_TYPE, 14001, 14033, 14010, 15002, 0, 1, 2, 3, 99])
                .unwrap_or(&ISO_CURVE_TYPE),
        },
        12 => OpIn::TypeStrings,
        13 => OpIn::UvIsoLine,
        _ if s.is_none() => OpIn::State,
        14..=16 => {
            let k = cross.unwrap_or_else(|| vec![0.0, 1.0]);
            OpIn::Evaluate {
                t: gen_param(rng, &k, false),
                n: rng.below(4) as usize,
                from_left: rng.chance(0.5),
                non_zero: rng.chance(0.7),
            }
        }
        17 | 18 => {
            let k = cross.unwrap_or_else(|| vec![0.0, 1.0]);
            OpIn::EvaluatePoint {
                t: gen_param(rng, &k, false),
            }
        }
        _ => OpIn::Continuities {
            tol: rng.uniform(0.01, 5.0),
        },
    };
    Inputs {
        curve: Some(curve),
        op,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "curve.iso_curve"
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
