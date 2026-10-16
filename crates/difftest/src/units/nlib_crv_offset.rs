//! Runner for unit `nlib.crv_offset`: the planar curve offset by point sampling
//! (`N_CrvOffsetPtSampling`).
//!
//! C++: `solidcore_ffi::units::nlib_crv_offset` (upstream
//! `source/SMLib/src/NL_CrvOffset.cpp`). Rust: `solidcore::nurbs::crv_offset`
//! ([`Curve::offset_pt_sampling`]).
//!
//! Inputs: a curve (inline `cpts` `[x, y, z, w]`, degree `p`, knots `u`, or an upstream
//! `.smc` file `{"smc": {"$upstream_file": "crvToOffset.smc"}}`), the plane normal `b`,
//! the distance `d`, the approximation degree `deg`, `tangents` (`tan = NL_YES`), `par`
//! (`uniform`, `chord_length`, `centripetal`, `inherited`) and `tol`.
//!
//! Output keys: `status` (`0` ok, else the `NL_*_ERR` number, `1000` for upstream's
//! bare error flags, `-1` on the Rust side for any other error, which never matches),
//! then on success `dims` (`n p m`), `cpts` (homogeneous `x y z w`) and `knots` of the
//! offset curve.
//!
//! The shim refuses inputs whose sample count exceeds
//! [`ffi::MAX_SAMPLES`](solidcore_ffi::units::nlib_crv_offset::MAX_SAMPLES) (upstream
//! casts it unchecked); the generator screens its cases with the same count computed
//! by the Rust port ([`sample_count`]). Every run has at least [`MIN_GENERATED`]
//! generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::crv_adv::p1::BARE_FLAG_REASON;
use solidcore::nurbs::crv_basic::{ConicType, Curve};
use solidcore::nurbs::crv_common::{circ_arc, Arc};
use solidcore::nurbs::crv_fit::p1::ParamMethod;
use solidcore::nurbs::crv_offset::OffsetParam;
use solidcore::nurbs::geom::Interval;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::{SolidError, Vec3};
use solidcore_ffi::units::nlib_crv_offset as ffi;
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1500;

/// `nlib.crv_offset` runner.
pub struct Runner;

/// An inline curve.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InlineCrv {
    cpts: Vec<[f64; 4]>,
    p: usize,
    u: Vec<f64>,
}

/// A curve input.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum CrvIn {
    /// An upstream `.smc` file (absolute path after fixture expansion).
    File { smc: String },
    /// Inline data.
    Inline(InlineCrv),
}

/// Upstream `par` flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ParIn {
    Uniform,
    ChordLength,
    Centripetal,
    Inherited,
}

impl ParIn {
    fn par(self) -> OffsetParam {
        match self {
            ParIn::Uniform => OffsetParam::Fit(ParamMethod::Uniform),
            ParIn::ChordLength => OffsetParam::Fit(ParamMethod::ChordLength),
            ParIn::Centripetal => OffsetParam::Fit(ParamMethod::Centripetal),
            ParIn::Inherited => OffsetParam::Inherited,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    crv: CrvIn,
    b: [f64; 3],
    d: f64,
    deg: usize,
    #[serde(default)]
    tangents: bool,
    par: ParIn,
    tol: f64,
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

fn numbers(path: &str) -> Result<Vec<f64>, HarnessError> {
    let text = std::fs::read_to_string(path).map_err(|e| harness(format!("{path}: {e}")))?;
    text.split_whitespace()
        .map(|t| {
            t.parse::<f64>()
                .map_err(|e| harness(format!("{path}: {e}")))
        })
        .collect()
}

fn int(path: &str, v: f64) -> Result<usize, HarnessError> {
    if v.fract() != 0.0 || v < 0.0 {
        return Err(harness(format!("{path}: bad count {v}")));
    }
    Ok(v as usize)
}

/// Reads an upstream `.smc` text file (as `N_CrvReadFromFilePtr`, without its
/// validity check and pruning): `n`, `p`, rational flag, dimension, `n+1` points
/// (`x y [z]`, or `wx wy [wz] w` when rational), `n+p+2` knots.
fn read_smc(path: &str) -> Result<InlineCrv, HarnessError> {
    let v = numbers(path)?;
    let mut it = v.into_iter();
    let mut num = || {
        it.next()
            .ok_or_else(|| harness(format!("{path}: truncated")))
    };
    let n = int(path, num()?)?;
    let p = int(path, num()?)?;
    let rat = num()? != 0.0;
    let dim = int(path, num()?)?;
    let mut cpts = Vec::with_capacity(n + 1);
    for _ in 0..=n {
        let (x, y) = (num()?, num()?);
        let z = if dim == 3 { num()? } else { NOZ };
        let w = if rat { num()? } else { NOW };
        cpts.push([x, y, z, w]);
    }
    let u = (0..n + p + 2)
        .map(|_| num())
        .collect::<Result<Vec<_>, _>>()?;
    Ok(InlineCrv { cpts, p, u })
}

fn resolve(c: &CrvIn) -> Result<InlineCrv, HarnessError> {
    match c {
        CrvIn::File { smc } => read_smc(smc),
        CrvIn::Inline(c) => Ok(c.clone()),
    }
}

fn to_curve(c: &InlineCrv) -> Result<Curve, HarnessError> {
    let pw = c
        .cpts
        .iter()
        .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
        .collect();
    Curve::new(pw, c.p, c.u.clone()).map_err(|e| harness(format!("curve: {e}")))
}

// ---- records ----------------------------------------------------------------------

fn take<'a>(flat: &'a [f64], at: &mut usize, k: usize) -> Result<&'a [f64], HarnessError> {
    let part = at
        .checked_add(k)
        .and_then(|end| flat.get(*at..end))
        .ok_or_else(|| harness(format!("record too short ({} values)", flat.len())))?;
    *at += k;
    Ok(part)
}

fn count(x: f64) -> Result<usize, HarnessError> {
    if x.fract() != 0.0 || !(0.0..=1e8).contains(&x) {
        return Err(harness(format!("bad count {x} in record")));
    }
    Ok(x as usize)
}

/// Splits a record (status, then on success `n p m`, control points, knots).
fn to_output(flat: &[f64]) -> Result<Output, HarnessError> {
    let mut at = 0usize;
    let status = take(flat, &mut at, 1)?.to_vec();
    let ok = status == [0.0];
    let mut out = Output::new().values("status", status);
    if ok {
        let dims = take(flat, &mut at, 3)?.to_vec();
        let n = count(dims.first().copied().unwrap_or(f64::NAN))?;
        let m = count(dims.last().copied().unwrap_or(f64::NAN))?;
        out = out.values("dims", dims);
        out = out.values("cpts", take(flat, &mut at, 4 * (n + 1))?.to_vec());
        out = out.values("knots", take(flat, &mut at, m + 1)?.to_vec());
    }
    if at != flat.len() {
        return Err(harness(format!(
            "record has {} values, layout used {at}",
            flat.len()
        )));
    }
    Ok(out)
}

// ---- C++ side ---------------------------------------------------------------------

fn encode(i: &Inputs, c: &InlineCrv) -> Result<Vec<f64>, HarnessError> {
    let n = c
        .cpts
        .len()
        .checked_sub(1)
        .ok_or_else(|| harness("empty curve"))?;
    let m =
        c.u.len()
            .checked_sub(1)
            .ok_or_else(|| harness("empty knots"))?;
    let mut o = vec![n as f64, c.p as f64, m as f64];
    o.extend(c.cpts.iter().flatten());
    o.extend_from_slice(&c.u);
    o.extend_from_slice(&i.b);
    o.extend([
        i.d,
        i.deg as f64,
        if i.tangents { 1.0 } else { 0.0 },
        f64::from(i.par.par().flag()),
        i.tol,
    ]);
    Ok(o)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let c = resolve(&i.crv)?;
    match ffi::offset_pt_sampling(&encode(i, &c)?) {
        Ok(flat) => to_output(&flat),
        Err(FfiError::Status(s)) => Err(harness(format!("shim status {s}"))),
        Err(e) => Err(harness(e.to_string())),
    }
}

// ---- Rust side --------------------------------------------------------------------

/// Status of a Rust error: the NLib number, `1000` for upstream's bare error flags,
/// `-1` for anything else.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::Nlib { code, .. } => f64::from(code.code()),
        SolidError::NotConverged { operation, .. } if operation.starts_with("nurbs::bezier") => {
            1000.0
        }
        SolidError::InvalidInput { reason, .. } if *reason == BARE_FLAG_REASON => 1000.0,
        _ => -1.0,
    }
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    to_output(&rust_record(i)?)
}

/// The Rust record (status, then the curve on success).
fn rust_record(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let c = resolve(&i.crv)?;
    let cur = to_curve(&c)?;
    let [bx, by, bz] = i.b;
    let rec = match cur.offset_pt_sampling(
        Vec3::new(bx, by, bz),
        i.d,
        i.deg,
        i.tangents,
        i.par.par(),
        i.tol,
    ) {
        Ok(q) => {
            let mut o = vec![0.0, q.n() as f64, q.degree() as f64, q.m() as f64];
            for &pw in q.cpts() {
                o.extend(pw.to_array());
            }
            o.extend_from_slice(q.knots());
            o
        }
        Err(e) => vec![status(&e)],
    };
    Ok(rec)
}

// ---- screening --------------------------------------------------------------------

/// The total sample count of `N_CrvOffsetPtSampling` as the shim's guard computes it
/// (`None` when the guard refuses the input).
fn sample_count(i: &Inputs) -> Option<f64> {
    let c = resolve(&i.crv).ok()?;
    let cur = to_curve(&c).ok()?;
    if i.tol == 0.0 {
        return Some(0.0);
    }
    let exp = if i.deg == 1 { 0.5 } else { 0.34 };
    let f1 = (1.0 / i.tol).powf(exp);
    let f2 = 2.0 * i.deg as f64;
    if !f1.is_finite() {
        return None;
    }
    let [bx, by, bz] = i.b;
    let Ok((b, _)) = solidcore::nurbs::geom::normalize(Vec3::new(bx, by, bz)) else {
        return Some(0.0);
    };
    let Ok(bez) = cur.decompose_bez() else {
        return Some(0.0);
    };
    let max = ffi::MAX_SAMPLES as f64;
    let mut mu = 0.0;
    for mut seg in bez {
        seg.reparam_to_interval(Interval::new(0.0, 1.0)).ok()?;
        let Ok(muu) = seg.offset_max_2nd_deriv(i.d, b) else {
            break;
        };
        let s = (0.125 * muu).sqrt();
        let gro = if f2 > s { f2 } else { s };
        let del = (f1 * gro).ceil();
        if !(del >= 0.0 && del <= max) {
            return None;
        }
        mu += del;
        if mu > max {
            return None;
        }
    }
    Some(mu)
}

// ---- generated cases --------------------------------------------------------------

/// A random rotation (orthonormal columns) from three uniform angles.
fn gen_frame(rng: &mut Rng) -> [[f64; 3]; 3] {
    let (a, b, c) = (
        rng.uniform(0.0, std::f64::consts::TAU),
        rng.uniform(0.0, std::f64::consts::TAU),
        rng.uniform(0.0, std::f64::consts::TAU),
    );
    let (sa, ca) = a.sin_cos();
    let (sb, cb) = b.sin_cos();
    let (sc, cc) = c.sin_cos();
    [
        [ca * cb, ca * sb * sc - sa * cc, ca * sb * cc + sa * sc],
        [sa * cb, sa * sb * sc + ca * cc, sa * sb * cc - ca * sc],
        [-sb, cb * sc, cb * cc],
    ]
}

/// A plane: origin, frame (columns 0 and 1 span it, column 2 is the normal), and
/// whether the curve is the 2D / 3D `z = 0` plane.
struct Plane {
    o: [f64; 3],
    f: [[f64; 3]; 3],
    xy: bool,
    two_d: bool,
}

impl Plane {
    fn gen(rng: &mut Rng) -> Plane {
        let xy = rng.chance(0.6);
        if xy {
            let o = rng.point3(-10.0, 10.0);
            Plane {
                o: [o[0], o[1], 0.0],
                f: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                xy,
                two_d: rng.chance(0.5),
            }
        } else {
            Plane {
                o: rng.point3(-10.0, 10.0),
                f: gen_frame(rng),
                xy,
                two_d: false,
            }
        }
    }

    fn point(&self, x: f64, y: f64) -> [f64; 3] {
        let (o, f) = (self.o, self.f);
        [
            o[0] + f[0][0] * x + f[0][1] * y,
            o[1] + f[1][0] * x + f[1][1] * y,
            o[2] + f[2][0] * x + f[2][1] * y,
        ]
    }

    fn normal(&self) -> [f64; 3] {
        [self.f[0][2], self.f[1][2], self.f[2][2]]
    }

    /// Homogeneous control point of the plane point `(x, y)` with weight `w`
    /// (`None` = non-rational).
    fn cpt(&self, x: f64, y: f64, w: Option<f64>) -> [f64; 4] {
        let [px, py, pz] = self.point(x, y);
        let pz = if self.xy { 0.0 } else { pz };
        let ww = w.unwrap_or(1.0);
        let z = if self.two_d { NOZ } else { pz * ww };
        [px * ww, py * ww, z, w.unwrap_or(NOW)]
    }
}

/// Clamped knots of degree `p` with `n - p` interior knots (some with
/// multiplicity up to `p - 1`, keeping the curve G1 for smooth polygons).
fn gen_knots(rng: &mut Rng, n: usize, p: usize) -> Vec<f64> {
    let (a, len) = if rng.chance(0.4) {
        (0.0, 1.0)
    } else {
        (rng.uniform(-5.0, 5.0), rng.uniform(0.25, 10.0))
    };
    let b = a + len;
    let mut inner: Vec<f64> = Vec::new();
    let mut remaining = n - p;
    while remaining > 0 {
        let v = a + len * rng.uniform(0.05, 0.95);
        if inner.contains(&v) {
            continue;
        }
        let mult = if p > 2 && rng.chance(0.15) {
            1 + rng.below((p - 1).min(remaining) as u64) as usize
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

/// A smooth planar free-form curve: control polygon from a turning walk.
fn gen_freeform(rng: &mut Rng, pl: &Plane) -> InlineCrv {
    let p = 1 + rng.below(5) as usize;
    let p = if p == 1 && rng.chance(0.7) { 3 } else { p };
    let n = p + rng.below(7) as usize;
    let rational = rng.chance(0.25);
    let scale = if rng.chance(0.2) {
        rng.uniform(0.01, 0.5)
    } else {
        rng.uniform(1.0, 20.0)
    };
    let (mut x, mut y) = (0.0, 0.0);
    let mut ang = rng.uniform(0.0, std::f64::consts::TAU);
    let turn = rng.uniform(0.1, 1.2);
    let cpts = (0..=n)
        .map(|_| {
            let w = rational.then(|| rng.uniform(0.5, 2.0));
            let c = pl.cpt(x, y, w);
            let step = scale * rng.uniform(0.5, 1.5);
            x += step * ang.cos();
            y += step * ang.sin();
            ang += rng.uniform(-turn, turn);
            c
        })
        .collect();
    InlineCrv {
        cpts,
        p,
        u: gen_knots(rng, n, p),
    }
}

/// A straight curve (collinear control points, monotone along the line).
fn gen_line(rng: &mut Rng, pl: &Plane) -> InlineCrv {
    let p = 1 + rng.below(3) as usize;
    let n = p + rng.below(3) as usize;
    let ang = rng.uniform(0.0, std::f64::consts::TAU);
    let len = rng.uniform(0.5, 30.0);
    let mut ts: Vec<f64> = (0..=n).map(|_| rng.unit()).collect();
    ts.sort_by(f64::total_cmp);
    if let Some(f) = ts.first_mut() {
        *f = 0.0;
    }
    if let Some(l) = ts.last_mut() {
        *l = 1.0;
    }
    let cpts = ts
        .iter()
        .map(|&t| pl.cpt(t * len * ang.cos(), t * len * ang.sin(), None))
        .collect();
    InlineCrv {
        cpts,
        p,
        u: gen_knots(rng, n, p),
    }
}

/// A circular arc or circle built by the port's `N_CreateCircArc`.
fn gen_circle(rng: &mut Rng, pl: &Plane) -> Option<InlineCrv> {
    let full = rng.chance(0.3);
    let end = if full {
        360.0
    } else {
        rng.uniform(10.0, 350.0)
    };
    let ctp = *rng.choose(&[ConicType::Quadratic, ConicType::Quartic, ConicType::Quintic])?;
    let [cx, cy, cz] = pl.point(0.0, 0.0);
    let cz = if pl.xy { 0.0 } else { cz };
    let x = Vec3::new(pl.f[0][0], pl.f[1][0], pl.f[2][0]);
    let y = Vec3::new(pl.f[0][1], pl.f[1][1], pl.f[2][1]);
    let arc = Arc {
        center: Vec3::new(cx, cy, cz),
        x,
        y,
        r: rng.uniform(0.2, 20.0),
        start: 0.0,
        end,
    };
    let mut c = circ_arc(&arc, ctp).ok()?;
    if pl.two_d {
        c.to_2d();
    }
    Some(InlineCrv {
        cpts: c.cpts().iter().map(|q| q.to_array()).collect(),
        p: c.degree(),
        u: c.knots().to_vec(),
    })
}

fn gen_inputs(rng: &mut Rng) -> Option<Inputs> {
    let pl = Plane::gen(rng);
    let kind = rng.below(10);
    let c = match kind {
        0 => gen_line(rng, &pl),
        1 => gen_circle(rng, &pl)?,
        _ => gen_freeform(rng, &pl),
    };
    let size = c
        .cpts
        .iter()
        .map(|q| {
            let w = if q[3] == NOW { 1.0 } else { q[3] };
            (q[0] / w).abs().max((q[1] / w).abs())
        })
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let mut b = pl.normal();
    match rng.below(20) {
        0 => b = [0.0, 0.0, 0.0],
        1 | 2 => b = b.map(|v| -v),
        3 => b = b.map(|v| v * rng.uniform(0.1, 10.0)),
        4 => {
            // Not the plane normal: tilted.
            let t = rng.point3(-0.3, 0.3);
            b = [b[0] + t[0], b[1] + t[1], b[2] + t[2]];
        }
        _ => {}
    }
    let d = match rng.below(10) {
        0 => 0.0,
        1 => rng.uniform(-0.5, 0.5) * size,
        _ => rng.uniform(-0.1, 0.1) * size,
    };
    let deg = match rng.below(12) {
        0 => 1,
        1..=3 => 2,
        4..=8 => 3,
        9 => 4,
        10 => 5,
        _ => 6 + rng.below(3) as usize,
    };
    let par = *rng.choose(&[
        ParIn::Inherited,
        ParIn::Inherited,
        ParIn::Uniform,
        ParIn::ChordLength,
        ParIn::Centripetal,
    ])?;
    let tol = match rng.below(30) {
        0 => 0.0,
        1 => 1.0,
        _ => *rng.choose(&[1e-1, 1e-2, 1e-3, 1e-4, 1e-5])? * size,
    };
    let tol = tol.min(1.0);
    Some(Inputs {
        crv: CrvIn::Inline(c),
        b,
        d,
        deg,
        tangents: rng.chance(0.4),
        par,
        tol,
    })
}

/// A generated case whose sample count the shim accepts.
fn gen_case(rng: &mut Rng) -> Option<Inputs> {
    for _ in 0..64 {
        let Some(mut i) = gen_inputs(rng) else {
            continue;
        };
        for _ in 0..4 {
            if sample_count(&i).is_some() {
                return Some(i);
            }
            i.tol = (i.tol * 10.0).min(1.0);
        }
    }
    None
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.crv_offset"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_case(rng)?).ok()
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

    /// The generator reaches success and the main error statuses, never a Rust-only
    /// error.
    #[test]
    fn generated_cases_cover_statuses() {
        let mut tally: BTreeMap<i64, usize> = BTreeMap::new();
        for s in 0..400 {
            let mut rng = Rng::new(1000 + s);
            let Some(i) = gen_case(&mut rng) else {
                continue;
            };
            let st = rust_record(&i).unwrap()[0];
            *tally.entry(st as i64).or_default() += 1;
        }
        assert!(tally.get(&0).copied().unwrap_or(0) > 200, "{tally:?}");
        eprintln!("{tally:?}");
        assert!(!tally.contains_key(&-1), "{tally:?}");
    }
}
