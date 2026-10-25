//! Runner for unit `nlib.crv_tool_p1`: NLib curve tools (knot insertion, refinement
//! and removal, removal error bound, splitting, segment extraction, Bezier
//! decomposition, degree elevation).
//!
//! C++: `solidcore_ffi::units::nlib_crv_tool_p1` (upstream
//! `source/SMLib/src/NL_CrvTool.cpp`). Rust: `solidcore::nurbs::crv_tool::p1`
//! (methods on `solidcore::nurbs::crv_basic::Curve`).
//!
//! Each case runs one routine (`inputs.op`) on one curve. Both sides produce the same
//! flat record of doubles, split into keys by [`to_output`]: `status` (`0` ok, else
//! the `NL_*_ERR` number; `-1` on the Rust side for a non-NLib error, which never
//! matches), then on success the outputs: a curve is `dims = [n, p, m]`, the control
//! points in use (`cpts`, x y z w) and the knots in use (`u`). The C++ side may run
//! the routine in place (`inplace`, output curve == input curve); the Rust side has a
//! single form.
//!
//! A curve input is inline (`cpts` `[x, y, z, w]`, degree `p`, knots `u`) or an
//! upstream `.smc` file (`{"smc": {"$upstream_file": "curve0.smc"}}`, read here as
//! `N_CrvReadFromFilePtr` does). Every run has at least [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::SolidError;
use solidcore_ffi::units::nlib_crv_tool_p1::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::flag;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

#[path = "fixtures/nlib_crv_tool_p1_smc.rs"]
mod smc;

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `nlib.crv_tool_p1` runner.
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

/// A parameter: a value, or `{"knot": i}` for the curve's knot `U[i]` (lets
/// fixtures name knots of `.smc` files exactly; JSON text need not round-trip).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(untagged)]
enum Param {
    /// The parameter value.
    Value(f64),
    /// Knot `U[knot]` of the case's curve.
    Knot { knot: usize },
}

impl Param {
    fn get(self, c: &InlineCrv) -> Result<f64, HarnessError> {
        match self {
            Param::Value(v) => Ok(v),
            Param::Knot { knot } => c
                .u
                .get(knot)
                .copied()
                .ok_or_else(|| harness(format!("no knot {knot}"))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    InsertKnot {
        crv: CrvIn,
        u: Param,
        r: usize,
        #[serde(default)]
        inplace: bool,
    },
    Split {
        crv: CrvIn,
        u: Param,
    },
    DecomposeBez {
        crv: CrvIn,
    },
    Refine {
        crv: CrvIn,
        x: Vec<Param>,
        #[serde(default)]
        inplace: bool,
    },
    ExtractSeg {
        crv: CrvIn,
        ul: Param,
        ur: Param,
        #[serde(default)]
        inplace: bool,
    },
    RemoveKnot {
        crv: CrvIn,
        u: Param,
        nu: usize,
        tol: f64,
        #[serde(default)]
        inplace: bool,
    },
    MaxErr {
        crv: CrvIn,
        r: usize,
        s: usize,
    },
    Elevate {
        crv: CrvIn,
        t: usize,
        #[serde(default)]
        inplace: bool,
    },
}

impl Inputs {
    fn crv(&self) -> &CrvIn {
        match self {
            Inputs::InsertKnot { crv, .. }
            | Inputs::Split { crv, .. }
            | Inputs::DecomposeBez { crv }
            | Inputs::Refine { crv, .. }
            | Inputs::ExtractSeg { crv, .. }
            | Inputs::RemoveKnot { crv, .. }
            | Inputs::MaxErr { crv, .. }
            | Inputs::Elevate { crv, .. } => crv,
        }
    }
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

/// Reads an upstream `.smc` text file (as `N_CrvReadFromFilePtr`, without its
/// validity check and pruning): `n`, `p`, rational flag, dimension, `n+1` points
/// (`x y [z]`, or `wx wy [wz] w` when rational), `n+p+2` knots.
fn read_smc(path: &str) -> Result<InlineCrv, HarnessError> {
    let text = std::fs::read_to_string(path).map_err(|e| harness(format!("{path}: {e}")))?;
    let c = smc::decode(&text, path)?;
    Ok(InlineCrv {
        cpts: c.cpts,
        p: c.p,
        u: c.u,
    })
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
    if x.fract() != 0.0 || !(0.0..=1e7).contains(&x) {
        return Err(harness(format!("bad count {x} in record")));
    }
    Ok(x as usize)
}

/// Splits a curve dump starting at `at` into `<pfx>dims`, `<pfx>cpts`, `<pfx>u`.
fn split_dump(flat: &[f64], at: &mut usize, pfx: &str, mut out: Output) -> Result<Output, HarnessError> {
    let dims = take(flat, at, 3)?;
    let n = count(dims.first().copied().unwrap_or(f64::NAN))?;
    let m = count(dims.get(2).copied().unwrap_or(f64::NAN))?;
    out = out.values(&format!("{pfx}dims"), dims.to_vec());
    let c = take(flat, at, 4 * (n + 1))?;
    out = out.values(&format!("{pfx}cpts"), c.to_vec());
    let u = take(flat, at, m + 1)?;
    Ok(out.values(&format!("{pfx}u"), u.to_vec()))
}

fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut at = 0usize;
    let status = take(flat, &mut at, 1)?.to_vec();
    let ok = status == [0.0];
    let mut out = Output::new().values("status", status);
    if ok {
        match i {
            Inputs::InsertKnot { .. }
            | Inputs::Refine { .. }
            | Inputs::ExtractSeg { .. }
            | Inputs::Elevate { .. } => {
                out = split_dump(flat, &mut at, "", out)?;
            }
            Inputs::Split { .. } => {
                out = split_dump(flat, &mut at, "l_", out)?;
                out = split_dump(flat, &mut at, "r_", out)?;
            }
            Inputs::DecomposeBez { .. } => {
                let k = take(flat, &mut at, 1)?.to_vec();
                let n = count(k.first().copied().unwrap_or(f64::NAN))?;
                out = out.values("count", k);
                for s in 0..n {
                    out = split_dump(flat, &mut at, &format!("s{s}_"), out)?;
                }
            }
            Inputs::RemoveKnot { .. } => {
                out = out.values("removed", take(flat, &mut at, 1)?.to_vec());
                out = split_dump(flat, &mut at, "", out)?;
            }
            Inputs::MaxErr { .. } => {
                out = out.values("br", take(flat, &mut at, 1)?.to_vec());
            }
        }
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
    let c = resolve(i.crv())?;
    encode_crv(&c, &mut o)?;
    let op = match i {
        Inputs::InsertKnot { u, r, inplace, .. } => {
            o.extend([u.get(&c)?, *r as f64, flag(*inplace)]);
            Op::InsertKnot
        }
        Inputs::Split { u, .. } => {
            o.push(u.get(&c)?);
            Op::Split
        }
        Inputs::DecomposeBez { .. } => Op::DecomposeBez,
        Inputs::Refine { x, inplace, .. } => {
            o.push(x.len() as f64);
            for v in x {
                o.push(v.get(&c)?);
            }
            o.push(flag(*inplace));
            Op::Refine
        }
        Inputs::ExtractSeg { ul, ur, inplace, .. } => {
            o.extend([ul.get(&c)?, ur.get(&c)?, flag(*inplace)]);
            Op::ExtractSeg
        }
        Inputs::RemoveKnot {
            u, nu, tol, inplace, ..
        } => {
            o.extend([u.get(&c)?, *nu as f64, *tol, flag(*inplace)]);
            Op::RemoveKnot
        }
        Inputs::MaxErr { r, s, .. } => {
            o.extend([*r as f64, *s as f64]);
            Op::MaxErr
        }
        Inputs::Elevate { t, inplace, .. } => {
            o.extend([*t as f64, flag(*inplace)]);
            Op::Elevate
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

fn dump(o: &mut Vec<f64>, c: &Curve) {
    o.extend([c.n() as f64, c.degree() as f64, c.m() as f64]);
    for pw in c.cpts() {
        o.extend(pw.to_array());
    }
    o.extend_from_slice(c.knots());
}

/// Status of a Rust error: the NLib number, `-1` for anything else.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::Nlib { code, .. } => f64::from(code.code()),
        _ => -1.0,
    }
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let c = resolve(i.crv())?;
    let cur = to_curve(&c)?;
    let pv = |v: &Param| v.get(&c);
    // Parameters first, so a bad knot index is a harness error on both sides.
    let (u, ul, ur, x) = match i {
        Inputs::InsertKnot { u, .. } | Inputs::Split { u, .. } | Inputs::RemoveKnot { u, .. } => {
            (pv(u)?, 0.0, 0.0, Vec::new())
        }
        Inputs::ExtractSeg { ul, ur, .. } => (0.0, pv(ul)?, pv(ur)?, Vec::new()),
        Inputs::Refine { x, .. } => (0.0, 0.0, 0.0, x.iter().map(pv).collect::<Result<Vec<_>, _>>()?),
        _ => (0.0, 0.0, 0.0, Vec::new()),
    };
    let mut o = vec![0.0];
    let res: Result<(), SolidError> = (|| {
        match i {
            Inputs::InsertKnot { r, .. } => dump(&mut o, &cur.insert_knot(u, *r)?),
            Inputs::Split { .. } => {
                let (l, r) = cur.split(u)?;
                dump(&mut o, &l);
                dump(&mut o, &r);
            }
            Inputs::DecomposeBez { .. } => {
                let segs = cur.decompose_bez()?;
                o.push(segs.len() as f64);
                for s in &segs {
                    dump(&mut o, s);
                }
            }
            Inputs::Refine { .. } => dump(&mut o, &cur.refine(&x)?),
            Inputs::ExtractSeg { .. } => dump(&mut o, &cur.extract_seg(ul, ur)?),
            Inputs::RemoveKnot { nu, tol, .. } => {
                let r = cur.remove_knot(u, *nu, *tol)?;
                o.push(r.removed as f64);
                dump(&mut o, &r.curve);
            }
            Inputs::MaxErr { r, s, .. } => o.push(cur.remove_knot_max_err(*r, *s)?),
            Inputs::Elevate { t, .. } => dump(&mut o, &cur.elevate_degree(*t)?),
        }
        Ok(())
    })();
    match res {
        Ok(()) => to_output(i, &o),
        Err(e) => match Output::unported(&e) {
            Some(d) => Ok(d),
            None => to_output(i, &[status(&e)]),
        },
    }
}

// ---- generated cases --------------------------------------------------------------

/// A random clamped curve: degree 1..=5 (rarely up to 8), `n - p` interior knots
/// with multiplicities up to `p`, 2D or 3D, rational or not.
fn gen_crv(rng: &mut Rng) -> InlineCrv {
    let p = if rng.chance(0.1) {
        6 + rng.below(3) as usize
    } else {
        1 + rng.below(5) as usize
    };
    let n = p + rng.below(9) as usize;
    let (a, len) = if rng.chance(0.3) {
        (rng.below(5) as f64 - 2.0, 1.0 + rng.below(6) as f64)
    } else {
        (rng.uniform(-5.0, 5.0), rng.uniform(0.25, 10.0))
    };
    let b = a + len;
    let integral = rng.chance(0.3);
    // Distinct values strictly inside (a, b), each with multiplicity 1..=p.
    let mut inner: Vec<f64> = Vec::new();
    let mut used: Vec<f64> = Vec::new();
    let mut remaining = n - p;
    while remaining > 0 {
        let mut v = a + 0.5 * len;
        for attempt in 0..32 {
            v = if integral && attempt < 16 {
                a + len * (1.0 + rng.below(15) as f64) / 16.0
            } else {
                rng.uniform(a, b)
            };
            if v > a && v < b && !used.contains(&v) {
                break;
            }
        }
        if v <= a || v >= b || used.contains(&v) {
            break;
        }
        used.push(v);
        let mult = if rng.chance(0.25) {
            1 + rng.below(p.min(remaining) as u64) as usize
        } else {
            1
        };
        inner.extend(std::iter::repeat_n(v, mult));
        remaining -= mult;
    }
    inner.sort_by(f64::total_cmp);
    // Fewer interior knots than asked for (no distinct value found): fewer points.
    let n = p + inner.len();
    let mut u = vec![a; p + 1];
    u.extend(inner);
    u.extend(std::iter::repeat_n(b, p + 1));

    let three_d = rng.chance(0.8);
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

/// Distinct interior knots of `u` (degree `p`).
fn interior(c: &InlineCrv) -> Vec<f64> {
    let p = c.p;
    let mut v: Vec<f64> = c
        .u
        .get(p + 1..c.u.len() - p - 1)
        .unwrap_or(&[])
        .to_vec();
    v.dedup();
    v
}

/// A parameter for `c`: random interior, an interior knot, near a knot, or an end.
fn gen_param(rng: &mut Rng, c: &InlineCrv, ends: f64) -> f64 {
    let a = c.u.first().copied().unwrap_or(0.0);
    let b = c.u.last().copied().unwrap_or(1.0);
    let knots = interior(c);
    let roll = rng.unit();
    if roll < ends {
        if rng.chance(0.5) {
            a
        } else {
            b
        }
    } else if roll < ends + 0.25 && !knots.is_empty() {
        knots.get(rng.below(knots.len() as u64) as usize).copied().unwrap_or(a)
    } else if roll < ends + 0.35 {
        // Within (or just outside) the split snapping tolerance of a knot.
        let k = if knots.is_empty() {
            a
        } else {
            knots.get(rng.below(knots.len() as u64) as usize).copied().unwrap_or(a)
        };
        let d = (b - a) * rng.choose(&[1e-9, 5e-7, 2e-6]).copied().unwrap_or(1e-9);
        let v = if rng.chance(0.5) { k + d } else { k - d };
        v.clamp(a, b)
    } else {
        rng.uniform(a, b)
    }
}

/// `(r, s)`: last index and multiplicity of a random interior knot run of `c`.
fn knot_run(rng: &mut Rng, c: &InlineCrv) -> Option<(usize, usize)> {
    let p = c.p;
    let m = c.u.len() - 1;
    let mut runs = Vec::new();
    let mut r = p + 1;
    while r < m - p {
        let mut e = r;
        while e + 1 < m - p && c.u.get(e + 1) == c.u.get(r) {
            e += 1;
        }
        runs.push((e, e - r + 1));
        r = e + 1;
    }
    rng.choose(&runs).copied()
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let crv = gen_crv(rng);
    let inplace = rng.chance(0.3);
    match rng.below(8) {
        0 => {
            let u = gen_param(rng, &crv, 0.05);
            let r = rng.below(crv.p as u64 + 2) as usize;
            Inputs::InsertKnot {
                crv: CrvIn::Inline(crv),
                u: Param::Value(u),
                r,
                inplace,
            }
        }
        1 => {
            let u = gen_param(rng, &crv, 0.05);
            Inputs::Split {
                crv: CrvIn::Inline(crv),
                u: Param::Value(u),
            }
        }
        2 => Inputs::DecomposeBez {
            crv: CrvIn::Inline(crv),
        },
        3 => {
            let k = 1 + rng.below(6) as usize;
            let mut x: Vec<f64> = (0..k).map(|_| gen_param(rng, &crv, 0.0)).collect();
            if rng.chance(0.05) {
                x.push(crv.u.last().copied().unwrap_or(1.0));
            }
            x.sort_by(f64::total_cmp);
            // Keep the first and last knot interior unless an end was asked for.
            let a = crv.u.first().copied().unwrap_or(0.0);
            if x.first() == Some(&a) && rng.chance(0.9) {
                x.remove(0);
            }
            if x.is_empty() {
                x.push(rng.uniform(a, crv.u.last().copied().unwrap_or(1.0)));
            }
            Inputs::Refine {
                crv: CrvIn::Inline(crv),
                x: x.into_iter().map(Param::Value).collect(),
                inplace,
            }
        }
        4 => {
            let mut ul = gen_param(rng, &crv, 0.2);
            let mut ur = gen_param(rng, &crv, 0.2);
            if ul > ur && rng.chance(0.9) {
                std::mem::swap(&mut ul, &mut ur);
            }
            Inputs::ExtractSeg {
                crv: CrvIn::Inline(crv),
                ul: Param::Value(ul),
                ur: Param::Value(ur),
                inplace,
            }
        }
        5 => {
            let tol = rng
                .choose(&[1e-12, 1e-9, 1e-6, 1e-3, 1.0, 1e3])
                .copied()
                .unwrap_or(1.0);
            let (crv, u, nu) = if rng.chance(0.5) {
                // Make a removable knot by inserting one first.
                let u0 = gen_param(rng, &crv, 0.0);
                let a = crv.u.first().copied().unwrap_or(0.0);
                let b = crv.u.last().copied().unwrap_or(1.0);
                let r = 1 + rng.below(crv.p as u64) as usize;
                let inserted = to_curve(&crv)
                    .ok()
                    .filter(|_| u0 > a && u0 < b)
                    .and_then(|c| c.insert_knot(u0, r).ok());
                if let Some(c) = inserted {
                    let ic = InlineCrv {
                        cpts: c.cpts().iter().map(|pw| pw.to_array()).collect(),
                        p: c.degree(),
                        u: c.knots().to_vec(),
                    };
                    let nu = rng.below(r as u64 + 2) as usize;
                    (ic, u0, nu)
                } else {
                    let nu = rng.below(crv.p as u64 + 1) as usize;
                    (crv, u0, nu)
                }
            } else {
                let u = gen_param(rng, &crv, 0.05);
                let nu = rng.below(crv.p as u64 + 1) as usize;
                (crv, u, nu)
            };
            Inputs::RemoveKnot {
                crv: CrvIn::Inline(crv),
                u: Param::Value(u),
                nu,
                tol,
                inplace,
            }
        }
        6 => match knot_run(rng, &crv) {
            Some((r, s)) => Inputs::MaxErr {
                crv: CrvIn::Inline(crv),
                r,
                s,
            },
            None => Inputs::DecomposeBez {
                crv: CrvIn::Inline(crv),
            },
        },
        _ => {
            let t = if rng.chance(0.05) {
                18 - crv.p.min(17) + rng.below(2) as usize
            } else {
                rng.below(4) as usize
            };
            Inputs::Elevate {
                crv: CrvIn::Inline(crv),
                t,
                inplace,
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.crv_tool_p1"
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
