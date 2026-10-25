//! Runner for unit `nlib.crv_tool_p2`: NLib curve tools (removal of all removable
//! knots, degree reduction, compatibility of curve sets, removal of degenerate
//! segments).
//!
//! C++: `solidcore_ffi::units::nlib_crv_tool_p2` (upstream
//! `source/SMLib/src/NL_CrvTool.cpp`). Rust: `solidcore::nurbs::crv_tool::p2`
//! (methods on `solidcore::nurbs::crv_basic::Curve` and free functions on curve sets).
//!
//! Each case runs one routine (`inputs.op`). Both sides produce the same flat record of
//! doubles, split into keys by [`to_output`]: `status` (`0` ok, else the `NL_*_ERR`
//! number; `-1` on the Rust side for a non-NLib error, which never matches), then the
//! outputs: a curve is `dims = [n, p, m]`, the control points in use (`cpts`, x y z w)
//! and the knots in use (`u`). Single-curve routines report outputs only on success;
//! the curve-set routines work in place and report every curve also after an error.
//! The C++ side may run the single-curve routines in place (`inplace`); the Rust side
//! has a single form.
//!
//! A curve input is inline (`cpts` `[x, y, z, w]`, degree `p`, knots `u`) or an
//! upstream `.smc` file (`{"smc": {"$upstream_file": "curve0.smc"}}`, read here as
//! `N_CrvReadFromFilePtr` does). Every run has at least [`MIN_GENERATED`] generated
//! cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::crv_tool::p2::{
    make_compatible, make_compatible_adj_knots, make_compatible_knot_tol,
};
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::SolidError;
use solidcore_ffi::units::nlib_crv_tool_p2::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::flag;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `nlib.crv_tool_p2` runner.
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
    RemoveKnots {
        crv: CrvIn,
        tol: f64,
        #[serde(default)]
        inplace: bool,
    },
    ReduceDegreeOnce {
        crv: CrvIn,
        tol: f64,
        #[serde(default)]
        inplace: bool,
    },
    ReduceDegree {
        crv: CrvIn,
        tol: f64,
        #[serde(default)]
        inplace: bool,
    },
    RemoveDegenSegs {
        crv: CrvIn,
        tol: f64,
        #[serde(default)]
        inplace: bool,
    },
    MakeCompatible {
        crvs: Vec<CrvIn>,
    },
    MakeCompatibleTol {
        crvs: Vec<CrvIn>,
        knot_tol: f64,
    },
    MakeCompatibleAdj {
        crvs: Vec<CrvIn>,
        tol: f64,
    },
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

#[path = "fixtures/nlib_crv_tool_p2_smc.rs"]
mod smc;

/// Reads an upstream `.smc` text file (as `N_CrvReadFromFilePtr`, without its
/// validity check and pruning): `n`, `p`, rational flag, dimension, `n+1` points
/// (`x y [z]`, or `wx wy [wz] w` when rational), `n+p+2` knots.
fn read_smc(path: &str) -> Result<InlineCrv, HarnessError> {
    let text = std::fs::read_to_string(path).map_err(|e| harness(format!("{path}: {e}")))?;
    let smc::SmcCurve { cpts, p, u } = smc::decode(&text, path)?;
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

fn from_curve(c: &Curve) -> InlineCrv {
    InlineCrv {
        cpts: c.cpts().iter().map(|pw| pw.to_array()).collect(),
        p: c.degree(),
        u: c.knots().to_vec(),
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

/// Splits a curve dump starting at `at` into `<pfx>dims`, `<pfx>cpts`, `<pfx>u`.
fn split_dump(
    flat: &[f64],
    at: &mut usize,
    pfx: &str,
    mut out: Output,
) -> Result<Output, HarnessError> {
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
    match i {
        Inputs::RemoveKnots { .. } | Inputs::ReduceDegree { .. } => {
            if ok {
                out = split_dump(flat, &mut at, "", out)?;
            }
        }
        Inputs::ReduceDegreeOnce { .. } => {
            if ok {
                let rfl = take(flat, &mut at, 1)?.to_vec();
                let reduced = rfl == [1.0];
                out = out.values("rfl", rfl);
                out = out.values("mtol", take(flat, &mut at, 1)?.to_vec());
                if reduced {
                    out = split_dump(flat, &mut at, "", out)?;
                }
            }
        }
        Inputs::RemoveDegenSegs { .. } => {
            if ok {
                let nr = take(flat, &mut at, 1)?.to_vec();
                let some = nr.first().is_some_and(|&v| v > 0.0);
                out = out.values("nr", nr);
                if some {
                    out = split_dump(flat, &mut at, "", out)?;
                }
            }
        }
        Inputs::MakeCompatible { crvs }
        | Inputs::MakeCompatibleTol { crvs, .. }
        | Inputs::MakeCompatibleAdj { crvs, .. } => {
            for k in 0..crvs.len() {
                out = split_dump(flat, &mut at, &format!("c{k}_"), out)?;
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
    let single = |crv: &CrvIn, tol: f64, inplace: bool, o: &mut Vec<f64>| {
        encode_crv(&resolve(crv)?, o)?;
        o.extend([tol, flag(inplace)]);
        Ok::<(), HarnessError>(())
    };
    let set = |crvs: &[CrvIn], prm: f64, o: &mut Vec<f64>| {
        o.push(crvs.len() as f64);
        for c in crvs {
            encode_crv(&resolve(c)?, o)?;
        }
        o.push(prm);
        Ok::<(), HarnessError>(())
    };
    let op = match i {
        Inputs::RemoveKnots { crv, tol, inplace } => {
            single(crv, *tol, *inplace, &mut o)?;
            Op::RemoveKnots
        }
        Inputs::ReduceDegreeOnce { crv, tol, inplace } => {
            single(crv, *tol, *inplace, &mut o)?;
            Op::ReduceDegreeOnce
        }
        Inputs::ReduceDegree { crv, tol, inplace } => {
            single(crv, *tol, *inplace, &mut o)?;
            Op::ReduceDegree
        }
        Inputs::RemoveDegenSegs { crv, tol, inplace } => {
            single(crv, *tol, *inplace, &mut o)?;
            Op::RemoveDegenSegs
        }
        Inputs::MakeCompatible { crvs } => {
            set(crvs, 0.0, &mut o)?;
            Op::MakeCompatible
        }
        Inputs::MakeCompatibleTol { crvs, knot_tol } => {
            set(crvs, *knot_tol, &mut o)?;
            Op::MakeCompatibleTol
        }
        Inputs::MakeCompatibleAdj { crvs, tol } => {
            set(crvs, *tol, &mut o)?;
            Op::MakeCompatibleAdj
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
    let crvs = match i {
        Inputs::MakeCompatible { crvs }
        | Inputs::MakeCompatibleTol { crvs, .. }
        | Inputs::MakeCompatibleAdj { crvs, .. } => {
            let mut cs = Vec::with_capacity(crvs.len());
            for c in crvs {
                cs.push(to_curve(&resolve(c)?)?);
            }
            Some(cs)
        }
        _ => None,
    };
    if let Some(mut cs) = crvs {
        let res = match i {
            Inputs::MakeCompatibleTol { knot_tol, .. } => make_compatible_knot_tol(&mut cs, *knot_tol),
            Inputs::MakeCompatibleAdj { tol, .. } => make_compatible_adj_knots(&mut cs, *tol),
            _ => make_compatible(&mut cs),
        };
        if let Err(e) = &res {
            if let Some(d) = Output::unported(e) {
                return Ok(d);
            }
        }
        let mut o = vec![res.as_ref().map_or_else(status, |()| 0.0)];
        for c in &cs {
            dump(&mut o, c);
        }
        return to_output(i, &o);
    }

    let (crv, tol) = match i {
        Inputs::RemoveKnots { crv, tol, .. }
        | Inputs::ReduceDegreeOnce { crv, tol, .. }
        | Inputs::ReduceDegree { crv, tol, .. }
        | Inputs::RemoveDegenSegs { crv, tol, .. } => (crv, *tol),
        _ => return Err(harness("unreachable op")),
    };
    let cur = to_curve(&resolve(crv)?)?;
    let mut o = vec![0.0];
    let res: Result<(), SolidError> = (|| {
        match i {
            Inputs::RemoveKnots { .. } => dump(&mut o, &cur.remove_knots(tol)?),
            Inputs::ReduceDegree { .. } => dump(&mut o, &cur.reduce_degree(tol)?),
            Inputs::ReduceDegreeOnce { .. } => {
                let red = cur.reduce_degree_once(tol)?;
                o.push(flag(red.curve.is_some()));
                o.push(red.max_err);
                if let Some(c) = &red.curve {
                    dump(&mut o, c);
                }
            }
            Inputs::RemoveDegenSegs { .. } => match cur.remove_degen_segs(tol)? {
                Some(r) => {
                    o.push(r.removed as f64);
                    dump(&mut o, &r.curve);
                }
                None => o.push(0.0),
            },
            _ => {}
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

/// A random clamped curve over `[a, a + len]`: degree 1..=5 (rarely up to 8), `n - p`
/// interior knots with multiplicities up to `p`, 2D or 3D, rational or not.
fn gen_crv_on(rng: &mut Rng, a: f64, len: f64, p: Option<usize>) -> InlineCrv {
    let p = p.unwrap_or_else(|| {
        if rng.chance(0.1) {
            6 + rng.below(3) as usize
        } else {
            1 + rng.below(5) as usize
        }
    });
    let n = p + rng.below(9) as usize;
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

fn gen_range(rng: &mut Rng) -> (f64, f64) {
    if rng.chance(0.3) {
        (rng.below(5) as f64 - 2.0, 1.0 + rng.below(6) as f64)
    } else {
        (rng.uniform(-5.0, 5.0), rng.uniform(0.25, 10.0))
    }
}

fn gen_crv(rng: &mut Rng) -> InlineCrv {
    let (a, len) = gen_range(rng);
    gen_crv_on(rng, a, len, None)
}

/// Moves every control point by up to `eps` (relative to its weight).
fn jitter(rng: &mut Rng, c: &mut InlineCrv, eps: f64) {
    for pt in &mut c.cpts {
        let w = if pt[3] == NOW { 1.0 } else { pt[3] };
        pt[0] += eps * w * rng.uniform(-1.0, 1.0);
        pt[1] += eps * w * rng.uniform(-1.0, 1.0);
        if pt[2] != NOZ {
            pt[2] += eps * w * rng.uniform(-1.0, 1.0);
        }
    }
}

fn pick_tol(rng: &mut Rng) -> f64 {
    rng.choose(&[0.0, 1e-12, 1e-9, 1e-6, 1e-4, 1e-3, 1e-2, 0.1, 1.0, 1e3])
        .copied()
        .unwrap_or(1e-6)
}

/// A curve with removable knots: `c` refined with a few random interior knots,
/// optionally jittered.
fn refined(rng: &mut Rng, c: InlineCrv) -> InlineCrv {
    let (Some(&a), Some(&b)) = (c.u.first(), c.u.last()) else {
        return c;
    };
    let k = 1 + rng.below(6) as usize;
    let mut x: Vec<f64> = (0..k)
        .map(|_| {
            if rng.chance(0.3) {
                a + (b - a) * (1.0 + rng.below(7) as f64) / 8.0
            } else {
                rng.uniform(a, b)
            }
        })
        .filter(|&v| v > a && v < b)
        .collect();
    x.sort_by(f64::total_cmp);
    if x.is_empty() {
        return c;
    }
    let Ok(cur) = to_curve(&c) else {
        return c;
    };
    // Keep only knots whose multiplicity stays at most p.
    let Ok(r) = cur.refine(&x) else {
        return c;
    };
    let out = from_curve(&r);
    let p = out.p;
    let inner = out.u.get(p + 1..out.u.len() - p - 1).unwrap_or(&[]);
    let mut run = 1;
    for w in inner.windows(2) {
        run = if w.first() == w.last() { run + 1 } else { 1 };
        if run > p {
            return c;
        }
    }
    out
}

/// A curve of degree `p + t` that is exactly (or, jittered, nearly) of degree `p`.
fn elevated(rng: &mut Rng, c: InlineCrv) -> InlineCrv {
    let t = 1 + rng.below(2) as usize;
    if c.p + t > 17 {
        return c;
    }
    let Ok(cur) = to_curve(&c) else {
        return c;
    };
    match cur.elevate_degree(t) {
        Ok(e) => from_curve(&e),
        Err(_) => c,
    }
}

/// Makes the span at index `s` (counted among the non-zero spans) degenerate by
/// collapsing its `p + 1` control points (optionally with a tiny jitter).
fn collapse_span(rng: &mut Rng, c: &mut InlineCrv, which: usize, eps: f64) {
    let p = c.p;
    let m = c.u.len() - 1;
    let mut spans = Vec::new();
    for i in p..m - p {
        if c.u.get(i) != c.u.get(i + 1) {
            spans.push(i);
        }
    }
    let Some(&i) = spans.get(which.min(spans.len().saturating_sub(1))) else {
        return;
    };
    let Some(&base) = c.cpts.get(i - p) else {
        return;
    };
    for j in i - p..=i {
        if let Some(pt) = c.cpts.get_mut(j) {
            *pt = base;
            let w = if base[3] == NOW { 1.0 } else { base[3] };
            pt[0] += eps * w * rng.uniform(-1.0, 1.0);
            pt[1] += eps * w * rng.uniform(-1.0, 1.0);
        }
    }
}

fn gen_set(rng: &mut Rng) -> Vec<CrvIn> {
    let k = 2 + rng.below(3) as usize;
    let same = rng.chance(0.6);
    let (a, len) = gen_range(rng);
    (0..k)
        .map(|_| {
            let (a, len) = if same && !rng.chance(0.1) {
                (a, len)
            } else if rng.chance(0.2) {
                // Ranges within a small tolerance of each other.
                (a + rng.uniform(-1e-6, 1e-6), len)
            } else {
                gen_range(rng)
            };
            CrvIn::Inline(gen_crv_on(rng, a, len, None))
        })
        .collect()
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let inplace = rng.chance(0.3);
    match rng.below(8) {
        0 | 1 => {
            let mut crv = gen_crv(rng);
            if rng.chance(0.7) {
                crv = refined(rng, crv);
            }
            if rng.chance(0.3) {
                let eps = rng.choose(&[1e-12, 1e-8, 1e-5]).copied().unwrap_or(1e-8);
                jitter(rng, &mut crv, eps);
            }
            Inputs::RemoveKnots {
                crv: CrvIn::Inline(crv),
                tol: pick_tol(rng),
                inplace,
            }
        }
        2 | 3 => {
            let mut crv = gen_crv(rng);
            if rng.chance(0.6) {
                crv = elevated(rng, crv);
            }
            if rng.chance(0.3) {
                let eps = rng.choose(&[1e-12, 1e-8, 1e-5]).copied().unwrap_or(1e-8);
                jitter(rng, &mut crv, eps);
            }
            let tol = pick_tol(rng);
            if rng.chance(0.5) {
                Inputs::ReduceDegreeOnce {
                    crv: CrvIn::Inline(crv),
                    tol,
                    inplace,
                }
            } else {
                Inputs::ReduceDegree {
                    crv: CrvIn::Inline(crv),
                    tol,
                    inplace,
                }
            }
        }
        4 => Inputs::MakeCompatible { crvs: gen_set(rng) },
        5 => Inputs::MakeCompatibleTol {
            crvs: gen_set(rng),
            knot_tol: rng
                .choose(&[0.0, -1.0, 1e-12, 1e-8, 1e-6, 1e-3, 0.01, 0.2])
                .copied()
                .unwrap_or(0.0),
        },
        6 => Inputs::MakeCompatibleAdj {
            crvs: gen_set(rng),
            tol: rng
                .choose(&[1e-9, 1e-6, 1e-3, 0.01, 0.1, 0.5])
                .copied()
                .unwrap_or(1e-6),
        },
        _ => {
            let mut crv = gen_crv(rng);
            if rng.chance(0.7) {
                let spans = 1 + rng.below(3) as usize;
                for _ in 0..spans {
                    let which = rng.below(10) as usize;
                    let eps = rng.choose(&[0.0, 0.0, 1e-9, 1e-6]).copied().unwrap_or(0.0);
                    collapse_span(rng, &mut crv, which, eps);
                }
            }
            Inputs::RemoveDegenSegs {
                crv: CrvIn::Inline(crv),
                tol: rng.choose(&[1e-9, 1e-6, 1e-4, 1e-2]).copied().unwrap_or(1e-6),
                inplace,
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.crv_tool_p2"
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
