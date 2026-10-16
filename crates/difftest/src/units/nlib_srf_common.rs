//! Runner for unit `nlib.srf_common`: common NURBS surfaces (bilinear patch from corner
//! points, extrusion, cylinder / cone, ruled and revolved surfaces).
//!
//! C++: `solidcore_ffi::units::nlib_srf_common` (upstream
//! `source/SMLib/src/NL_SrfCommon.cpp`). Rust: `solidcore::nurbs::srf_common`.
//!
//! Each case runs one routine (`inputs.op`). Both sides produce the same flat record of
//! doubles, split into keys by [`to_output`]: `status` (`0` ok, else the `NL_*_ERR`
//! number; `-1` on the Rust side for a non-NLib error, which never matches), every
//! input curve after the call (`c<k>_dims = [n, p, m]`, `c<k>_cpts`, `c<k>_u`; the
//! routines change them in place), then on success the surface (`srf_dims =
//! [n, m, p, q, r, s]`, `srf_net` row by row as x y z w, `srf_u`, `srf_v`).
//!
//! A curve input is inline (`cpts` `[x, y, z, w]`, degree `p`, knots `u`) or an
//! upstream `.smc` file (`{"smc": {"$upstream_file": "curve0.smc"}}`, read here as
//! `N_CrvReadFromFilePtr` does). Every run has at least [`MIN_GENERATED`] generated
//! cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::crv_basic::{ConicType, Curve};
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::nurbs::srf_basic::{Direction, Surface};
use solidcore::nurbs::srf_common::{
    corner_pts, cyl_cone, extrude_crv, revolved_srf, ruled_srf, CylCone,
};
use solidcore::SolidError;
use solidcore_ffi::units::nlib_srf_common::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::v3;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1500;

/// `nlib.srf_common` runner.
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    CornerPts {
        p00: [f64; 3],
        p10: [f64; 3],
        p01: [f64; 3],
        p11: [f64; 3],
    },
    ExtrudeCrv {
        crv: CrvIn,
        w: [f64; 3],
        d: f64,
        dir: i32,
    },
    CylCone {
        c: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        rb: f64,
        rt: f64,
        r#as: f64,
        ae: f64,
        h: f64,
        ctp: i32,
        dir: i32,
    },
    RuledSrf {
        a: CrvIn,
        b: CrvIn,
        dir: i32,
    },
    RevolvedSrf {
        crv: CrvIn,
        s: [f64; 3],
        t: [f64; 3],
        al: f64,
        ctp: i32,
    },
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

/// Whitespace-separated numbers of an upstream text file.
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
    let mut num = || it.next().ok_or_else(|| harness(format!("{path}: truncated")));
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
    let u = (0..n + p + 2).map(|_| num()).collect::<Result<Vec<_>, _>>()?;
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

fn dir_of(flag: i32) -> Result<Direction, HarnessError> {
    Direction::from_flag(flag).ok_or_else(|| harness(format!("bad dir {flag}")))
}

fn ctp_of(flag: i32) -> Result<ConicType, HarnessError> {
    ConicType::from_flag(flag).ok_or_else(|| harness(format!("bad ctp {flag}")))
}

fn curves_in(i: &Inputs) -> Vec<&CrvIn> {
    match i {
        Inputs::ExtrudeCrv { crv, .. } | Inputs::RevolvedSrf { crv, .. } => vec![crv],
        Inputs::RuledSrf { a, b, .. } => vec![a, b],
        Inputs::CornerPts { .. } | Inputs::CylCone { .. } => Vec::new(),
    }
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
    if x.fract() != 0.0 || !(0.0..=1e7).contains(&x) {
        return Err(harness(format!("bad count {x} in record")));
    }
    Ok(x as usize)
}

fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut at = 0usize;
    let status = take(flat, &mut at, 1)?.to_vec();
    let ok = status == [0.0];
    let mut out = Output::new().values("status", status);
    for k in 0..curves_in(i).len() {
        let dims = take(flat, &mut at, 3)?;
        let n = count(dims.first().copied().unwrap_or(f64::NAN))?;
        let m = count(dims.get(2).copied().unwrap_or(f64::NAN))?;
        out = out.values(&format!("c{k}_dims"), dims.to_vec());
        out = out.values(&format!("c{k}_cpts"), take(flat, &mut at, 4 * (n + 1))?.to_vec());
        out = out.values(&format!("c{k}_u"), take(flat, &mut at, m + 1)?.to_vec());
    }
    if ok {
        let dims = take(flat, &mut at, 6)?;
        let d = |k: usize| count(dims.get(k).copied().unwrap_or(f64::NAN));
        let (n, m, r, s) = (d(0)?, d(1)?, d(4)?, d(5)?);
        out = out.values("srf_dims", dims.to_vec());
        out = out.values("srf_net", take(flat, &mut at, 4 * (n + 1) * (m + 1))?.to_vec());
        out = out.values("srf_u", take(flat, &mut at, r + 1)?.to_vec());
        out = out.values("srf_v", take(flat, &mut at, s + 1)?.to_vec());
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

fn encode_crv(c: &InlineCrv, o: &mut Vec<f64>) -> Result<(), HarnessError> {
    let n = c
        .cpts
        .len()
        .checked_sub(1)
        .ok_or_else(|| harness("empty curve"))?;
    let m = c.u.len().checked_sub(1).ok_or_else(|| harness("empty knots"))?;
    o.extend([n as f64, c.p as f64, m as f64]);
    o.extend(c.cpts.iter().flatten());
    o.extend_from_slice(&c.u);
    Ok(())
}

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut o = Vec::new();
    let op = match i {
        Inputs::CornerPts { p00, p10, p01, p11 } => {
            for p in [p00, p10, p01, p11] {
                o.extend_from_slice(p);
            }
            Op::CornerPts
        }
        Inputs::ExtrudeCrv { crv, w, d, dir } => {
            encode_crv(&resolve(crv)?, &mut o)?;
            o.extend_from_slice(w);
            o.extend([*d, f64::from(*dir)]);
            Op::ExtrudeCrv
        }
        Inputs::CylCone {
            c,
            x,
            y,
            rb,
            rt,
            r#as,
            ae,
            h,
            ctp,
            dir,
        } => {
            o.extend_from_slice(c);
            o.extend_from_slice(x);
            o.extend_from_slice(y);
            o.extend([*rb, *rt, *r#as, *ae, *h, f64::from(*ctp), f64::from(*dir)]);
            Op::CylCone
        }
        Inputs::RuledSrf { a, b, dir } => {
            encode_crv(&resolve(a)?, &mut o)?;
            encode_crv(&resolve(b)?, &mut o)?;
            o.push(f64::from(*dir));
            Op::RuledSrf
        }
        Inputs::RevolvedSrf { crv, s, t, al, ctp } => {
            encode_crv(&resolve(crv)?, &mut o)?;
            o.extend_from_slice(s);
            o.extend_from_slice(t);
            o.extend([*al, f64::from(*ctp)]);
            Op::RevolvedSrf
        }
    };
    Ok((op, o))
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (op, input) = cpp_input(i)?;
    match ffi::eval(op, &input) {
        Ok(flat) => to_output(i, &flat),
        Err(FfiError::Status(s)) => Err(harness(format!("shim status {s}"))),
        Err(e) => Err(harness(e.to_string())),
    }
}

// ---- Rust side --------------------------------------------------------------------

fn dump_crv(o: &mut Vec<f64>, c: &Curve) {
    o.extend([c.n() as f64, c.degree() as f64, c.m() as f64]);
    for pw in c.cpts() {
        o.extend(pw.to_array());
    }
    o.extend_from_slice(c.knots());
}

fn dump_srf(o: &mut Vec<f64>, s: &Surface) {
    let idx = s.indices();
    o.extend([idx.n, idx.m, idx.p, idx.q, idx.r, idx.s].map(|v| v as f64));
    for row in s.rows() {
        for pw in row {
            o.extend(pw.to_array());
        }
    }
    o.extend_from_slice(s.u_knots());
    o.extend_from_slice(s.v_knots());
}

/// Status of a Rust error: the NLib number, `-1` for anything else.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::Nlib { code, .. } => f64::from(code.code()),
        _ => -1.0,
    }
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let mut crvs = Vec::new();
    for c in curves_in(i) {
        crvs.push(to_curve(&resolve(c)?)?);
    }
    let res: Result<Surface, SolidError> = match (i, crvs.as_mut_slice()) {
        (Inputs::CornerPts { p00, p10, p01, p11 }, _) => {
            corner_pts(v3(*p00), v3(*p10), v3(*p01), v3(*p11))
        }
        (Inputs::ExtrudeCrv { w, d, dir, .. }, [c]) => extrude_crv(c, v3(*w), *d, dir_of(*dir)?),
        (
            Inputs::CylCone {
                c,
                x,
                y,
                rb,
                rt,
                r#as,
                ae,
                h,
                ctp,
                dir,
            },
            _,
        ) => cyl_cone(
            &CylCone {
                center: v3(*c),
                x: v3(*x),
                y: v3(*y),
                rb: *rb,
                rt: *rt,
                start: *r#as,
                end: *ae,
                h: *h,
            },
            ctp_of(*ctp)?,
            dir_of(*dir)?,
        ),
        (Inputs::RuledSrf { dir, .. }, [a, b]) => ruled_srf(a, b, dir_of(*dir)?),
        (Inputs::RevolvedSrf { s, t, al, ctp, .. }, [c]) => {
            revolved_srf(c, v3(*s), v3(*t), *al, ctp_of(*ctp)?)
        }
        _ => return Err(harness("curve count does not match the op")),
    };
    if let Err(e) = &res {
        if let Some(d) = Output::unported(e) {
            return Ok(d);
        }
    }
    let mut o = vec![res.as_ref().map_or_else(status, |_| 0.0)];
    for c in &crvs {
        dump_crv(&mut o, c);
    }
    if let Ok(s) = &res {
        dump_srf(&mut o, s);
    }
    to_output(i, &o)
}

// ---- generated cases --------------------------------------------------------------

/// A random clamped curve over `[a, a + len]`: degree 1..=5, `n - p` interior knots
/// with multiplicities up to `p`, 2D or 3D, rational or not.
fn gen_crv_on(rng: &mut Rng, a: f64, len: f64, p: Option<usize>, three_d: bool) -> InlineCrv {
    let p = p.unwrap_or_else(|| 1 + rng.below(5) as usize);
    let n = p + rng.below(7) as usize;
    let b = a + len;
    let mut inner: Vec<f64> = Vec::new();
    let mut remaining = n - p;
    while remaining > 0 {
        let v = if rng.chance(0.3) {
            a + len * (1.0 + rng.below(15) as f64) / 16.0
        } else {
            rng.uniform(a, b)
        };
        if v <= a || v >= b || inner.contains(&v) {
            continue;
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

    let rational = rng.chance(0.3);
    let cpts = (0..=n)
        .map(|_| {
            let [x, y, z] = rng.point3(-10.0, 10.0);
            let w = if rational { rng.uniform(0.3, 3.0) } else { 1.0 };
            let z = if three_d { z * w } else { NOZ };
            [x * w, y * w, z, if rational { w } else { NOW }]
        })
        .collect();
    InlineCrv { cpts, p, u }
}

fn gen_range(rng: &mut Rng) -> (f64, f64) {
    if rng.chance(0.4) {
        (0.0, 1.0)
    } else {
        (rng.uniform(-5.0, 5.0), rng.uniform(0.25, 10.0))
    }
}

fn gen_crv(rng: &mut Rng) -> InlineCrv {
    let (a, len) = gen_range(rng);
    let three_d = rng.chance(0.7);
    gen_crv_on(rng, a, len, None, three_d)
}

fn gen_dir(rng: &mut Rng) -> i32 {
    if rng.chance(0.05) {
        3
    } else {
        1 + rng.below(2) as i32
    }
}

fn gen_ctp(rng: &mut Rng) -> i32 {
    rng.choose(&[2, 4, 5]).copied().unwrap_or(2)
}

fn gen_angle(rng: &mut Rng) -> f64 {
    if rng.chance(0.4) {
        rng.choose(&[0.0, 30.0, 45.0, 90.0, 91.0, 120.0, 180.0, 240.0, 270.0, 360.0, -90.0, 720.0])
            .copied()
            .unwrap_or(90.0)
    } else {
        rng.uniform(-400.0, 400.0)
    }
}

/// A unit-ish vector, sometimes exactly an axis, sometimes zero.
fn gen_vec(rng: &mut Rng, zero_chance: f64) -> [f64; 3] {
    if rng.chance(zero_chance) {
        return [0.0, 0.0, 0.0];
    }
    if rng.chance(0.4) {
        let axes = [
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
        ];
        return rng.choose(&axes).copied().unwrap_or([0.0, 0.0, 1.0]);
    }
    rng.point3(-2.0, 2.0)
}

fn gen_case(rng: &mut Rng) -> Inputs {
    match rng.below(10) {
        0 => {
            let mut p = || rng.point3(-10.0, 10.0);
            Inputs::CornerPts {
                p00: p(),
                p10: p(),
                p01: p(),
                p11: p(),
            }
        }
        1 | 2 => Inputs::ExtrudeCrv {
            crv: CrvIn::Inline(gen_crv(rng)),
            w: gen_vec(rng, 0.05),
            d: if rng.chance(0.1) {
                0.0
            } else {
                rng.uniform(-10.0, 10.0)
            },
            dir: gen_dir(rng),
        },
        3 | 4 => {
            let x = gen_vec(rng, 0.03);
            let y = if rng.chance(0.05) {
                x
            } else if rng.chance(0.5) {
                // Orthogonal to x when x is an axis.
                [x[1].abs(), x[2].abs(), x[0].abs()]
            } else {
                gen_vec(rng, 0.03)
            };
            let radius = |rng: &mut Rng| {
                if rng.chance(0.15) {
                    rng.choose(&[0.0, 1e-8, 1e-7, 2e-7]).copied().unwrap_or(0.0)
                } else {
                    rng.uniform(0.1, 10.0)
                }
            };
            Inputs::CylCone {
                c: rng.point3(-10.0, 10.0),
                x,
                y,
                rb: radius(rng),
                rt: radius(rng),
                r#as: gen_angle(rng),
                ae: gen_angle(rng),
                h: rng.uniform(-10.0, 10.0),
                ctp: gen_ctp(rng),
                dir: gen_dir(rng),
            }
        }
        5..=7 => {
            let same = rng.chance(0.6);
            let three_d = rng.chance(0.7);
            let (a, len) = gen_range(rng);
            let ca = gen_crv_on(rng, a, len, None, three_d);
            let cb = if same {
                let b3d = if rng.chance(0.8) { three_d } else { !three_d };
                gen_crv_on(rng, a, len, None, b3d)
            } else {
                gen_crv(rng)
            };
            Inputs::RuledSrf {
                a: CrvIn::Inline(ca),
                b: CrvIn::Inline(cb),
                dir: gen_dir(rng),
            }
        }
        _ => {
            let mut crv = gen_crv(rng);
            let s = if rng.chance(0.4) {
                [0.0, 0.0, 0.0]
            } else {
                rng.point3(-5.0, 5.0)
            };
            let t = gen_vec(rng, 0.03);
            // Put some control points on the axis (or all of them).
            let on_axis = rng.below(4);
            if on_axis > 0 {
                let all = on_axis == 3 && rng.chance(0.5);
                for pt in &mut crv.cpts {
                    if all || rng.chance(0.3) {
                        let k = rng.uniform(-3.0, 3.0);
                        let w = if pt[3] == NOW { 1.0 } else { pt[3] };
                        pt[0] = (s[0] + k * t[0]) * w;
                        pt[1] = (s[1] + k * t[1]) * w;
                        if pt[2] != NOZ {
                            pt[2] = (s[2] + k * t[2]) * w;
                        }
                    }
                }
            }
            let ctp = gen_ctp(rng);
            let mut al = gen_angle(rng);
            if ctp == 4 && quartic_too_wide(al) {
                al = rng.uniform(1.0, 240.0);
            }
            Inputs::RevolvedSrf {
                crv: CrvIn::Inline(crv),
                s,
                t,
                al,
                ctp,
            }
        }
    }
}

/// The quartic sweeps the C++ side rejects (see the ffi wrapper).
fn quartic_too_wide(al: f64) -> bool {
    let tol = 1.0e-6;
    let mut ae = al;
    while ae < tol {
        ae += 360.0;
    }
    while ae > 360.0 - tol {
        ae -= 360.0;
    }
    if ae < tol {
        ae = 360.0;
    }
    ae >= 241.0
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.srf_common"
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
