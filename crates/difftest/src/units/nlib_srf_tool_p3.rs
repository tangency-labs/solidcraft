//! Runner for unit `nlib.srf_tool_p3`: NLib surface removal of all removable knots,
//! joining of two surfaces and making a set of surfaces compatible.
//!
//! C++: `solidcore_ffi::units::nlib_srf_tool_p3` (upstream
//! `source/SMLib/src/NL_SrfTool.cpp`). Rust: `solidcore::nurbs::srf_tool::p3`.
//!
//! Each case runs one *op* (`inputs.op`). Both sides produce the same flat record of
//! doubles, split into keys by [`to_output`]: `status` (`0` ok, else the `NL_*_ERR`
//! number; `-1` on the Rust side for a non-NLib error, which never matches), then on
//! success the surface(s): `dims = [n, m, p, q, r, s]`, the control points (`net`,
//! x y z w row-major) and the knots (`u`, `v`); `make_compatible` dumps every surface
//! with the prefix `s<i>_`. Upstream's in-place modes (`inplace`, join `mode` 1 / 2)
//! are compared against the same Rust call.
//!
//! A surface input is inline, an upstream `.sms` file (`{"sms": {"$upstream_file":
//! "Surface.sms"}}`) or *derived* from another input (`{"base": ..., "elevate_u": 1,
//! "insert_u": [...], "insert_v": [...], "split": [t, dir, half]}`, applied in that
//! order with the green units `nurbs::srf_tool::p1` / `p2`); the derived surface is
//! what both sides get. Inputs must be valid clamped surfaces (the FFI wrapper
//! rejects anything else). Every run has at least [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::math::{NOW, NOZ};
use solidcore::nurbs::srf_basic::{Direction, Surface};
use solidcore::nurbs::srf_tool::p1 as tool1;
use solidcore::nurbs::srf_tool::p2 as tool2;
use solidcore::nurbs::srf_tool::p3 as tool;
use solidcore_ffi::units::nlib_srf_tool_p3::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::units::nlib_srf_tool_p2::{
    build, direction, domain, dump, encode, gen_removable, gen_srf, harness, inline, knots, param,
    resolve, split_srf, status, take, InlineSrf, SrfIn,
};
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2400;

/// `nlib.srf_tool_p3` runner.
pub struct Runner;

/// A surface input: plain, or derived from another one.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum Src {
    /// `base`, then degree elevation, knot insertion and an optional split.
    Derived {
        base: Box<Src>,
        #[serde(default)]
        elevate_u: usize,
        #[serde(default)]
        elevate_v: usize,
        #[serde(default)]
        insert_u: Vec<f64>,
        #[serde(default)]
        insert_v: Vec<f64>,
        /// `[t, dir, half]`: split at `t` in `dir` and keep half `0` or `1`.
        #[serde(default)]
        split: Option<(f64, i32, usize)>,
    },
    /// A file or inline surface.
    Plain(SrfIn),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    RemoveAll {
        sur: Src,
        tol: f64,
        dir: i32,
        #[serde(default)]
        inplace: bool,
    },
    Join {
        sur1: Src,
        sur2: Src,
        dir: i32,
        tol: f64,
        /// `0` new output, `1` into `sur1`, `2` into `sur2`.
        #[serde(default)]
        mode: u8,
    },
    Compatible {
        surs: Vec<Src>,
        dir: i32,
    },
}

fn rs<T>(r: solidcore::Result<T>, what: &str) -> Result<T, HarnessError> {
    r.map_err(|e| harness(format!("deriving input ({what}): {e}")))
}

/// The surface an input stands for.
pub(crate) fn surface(s: &Src) -> Result<InlineSrf, HarnessError> {
    match s {
        Src::Plain(p) => resolve(p),
        Src::Derived {
            base,
            elevate_u,
            elevate_v,
            insert_u,
            insert_v,
            split,
        } => {
            let mut sur = build(&surface(base)?)?;
            if *elevate_u > 0 {
                sur = rs(
                    tool2::elevate_degree(&sur, *elevate_u, Direction::U),
                    "elevate",
                )?;
            }
            if *elevate_v > 0 {
                sur = rs(
                    tool2::elevate_degree(&sur, *elevate_v, Direction::V),
                    "elevate",
                )?;
            }
            if !insert_u.is_empty() {
                sur = rs(tool1::insert_knots(&sur, insert_u, Direction::U), "insert")?;
            }
            if !insert_v.is_empty() {
                sur = rs(tool1::insert_knots(&sur, insert_v, Direction::V), "insert")?;
            }
            if let Some((t, dir, half)) = split {
                let (a, b) = rs(tool1::split(&sur, *t, direction(*dir)?), "split")?;
                sur = if *half == 0 { a } else { b };
            }
            Ok(inline(&sur))
        }
    }
}

// ---- output layout ----------------------------------------------------------------

fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut out = Output::new();
    let mut at = 0usize;
    let status = take(flat, &mut at, 1)?.to_vec();
    let ok = status.first() == Some(&0.0);
    out = out.values("status", status);
    if ok {
        match i {
            Inputs::RemoveAll { .. } | Inputs::Join { .. } => {
                out = split_srf(flat, &mut at, "", out)?;
            }
            Inputs::Compatible { surs, .. } => {
                for k in 0..surs.len() {
                    out = split_srf(flat, &mut at, &format!("s{k}_"), out)?;
                }
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

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut o = Vec::new();
    let op = match i {
        Inputs::RemoveAll {
            sur,
            tol,
            dir,
            inplace,
        } => {
            encode(&surface(sur)?, &mut o)?;
            o.extend([*tol, f64::from(*dir), if *inplace { 1.0 } else { 0.0 }]);
            Op::RemoveAllKnots
        }
        Inputs::Join {
            sur1,
            sur2,
            dir,
            tol,
            mode,
        } => {
            encode(&surface(sur1)?, &mut o)?;
            encode(&surface(sur2)?, &mut o)?;
            o.extend([f64::from(*dir), *tol, f64::from(*mode)]);
            Op::Join
        }
        Inputs::Compatible { surs, dir } => {
            o.push(surs.len() as f64);
            for s in surs {
                encode(&surface(s)?, &mut o)?;
            }
            o.push(f64::from(*dir));
            Op::MakeCompatible
        }
    };
    Ok((op, o))
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (op, input) = cpp_input(i)?;
    match ffi::eval(op, &input) {
        Ok(flat) => to_output(i, &flat),
        Err(FfiError::InvalidArgument(m)) => Err(harness(format!("ffi rejected: {m}"))),
        Err(e) => Err(harness(format!("shim failure: {e}"))),
    }
}

// ---- Rust side --------------------------------------------------------------------

fn rust_flat(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    match i {
        Inputs::RemoveAll { sur, tol, dir, .. } => {
            let s = build(&surface(sur)?)?;
            let r = tool::remove_all_knots(&s, *tol, direction(*dir)?);
            o.push(status(&r));
            if let Ok(q) = r {
                dump(&mut o, &q);
            }
        }
        Inputs::Join {
            sur1,
            sur2,
            dir,
            tol,
            ..
        } => {
            let a = build(&surface(sur1)?)?;
            let b = build(&surface(sur2)?)?;
            let r = tool::join(&a, &b, direction(*dir)?, *tol);
            o.push(status(&r));
            if let Ok(q) = r {
                dump(&mut o, &q);
            }
        }
        Inputs::Compatible { surs, dir } => {
            let mut v = surs
                .iter()
                .map(|s| build(&surface(s)?))
                .collect::<Result<Vec<Surface>, _>>()?;
            let r = tool::make_compatible(&mut v, direction(*dir)?);
            o.push(status(&r));
            if r.is_ok() {
                for s in &v {
                    dump(&mut o, s);
                }
            }
        }
    }
    Ok(o)
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    to_output(i, &rust_flat(i)?)
}

// ---- generation -------------------------------------------------------------------

pub(crate) fn plain(s: InlineSrf) -> Src {
    Src::Plain(SrfIn::Inline(s))
}

pub(crate) fn gen_dir(rng: &mut Rng) -> i32 {
    match rng.below(5) {
        0 | 1 => 3,
        2 => 1,
        3 => 2,
        _ => 1 + rng.below(3) as i32,
    }
}

pub(crate) fn gen_tol(rng: &mut Rng) -> f64 {
    match rng.below(12) {
        0 => 0.0,
        1 => 1.0,
        2 => 1e3,
        3 => rng.uniform(1e-3, 1e-1),
        4 => 1e-6,
        5 | 6 => 1e-12,
        7 => -1.0,
        _ => 1e-9,
    }
}

/// Random interior parameters of `k` (some on existing knots, some repeated).
pub(crate) fn new_knots(rng: &mut Rng, k: &[f64], deg: usize) -> Vec<f64> {
    let cnt = rng.below(5) as usize;
    let mut x: Vec<f64> = Vec::new();
    for _ in 0..cnt {
        let t = param(rng, k, false);
        let (a, b) = (
            k.first().copied().unwrap_or(0.0),
            k.last().copied().unwrap_or(1.0),
        );
        if t <= a || t >= b {
            continue;
        }
        let reps = 1 + rng.below(deg as u64) as usize;
        for _ in 0..reps {
            x.push(t);
        }
    }
    x.sort_by(f64::total_cmp);
    x
}

/// A surface with many removable knots: random, refined in u and/or v.
pub(crate) fn gen_refined(rng: &mut Rng) -> InlineSrf {
    if rng.chance(0.3) {
        return gen_removable(rng);
    }
    let s = gen_srf(rng);
    let Ok(mut sur) = build(&s) else {
        return s;
    };
    for dir in [Direction::U, Direction::V] {
        if rng.chance(0.25) {
            continue;
        }
        let (k, deg) = if dir == Direction::U {
            (sur.u_knots().to_vec(), sur.p())
        } else {
            (sur.v_knots().to_vec(), sur.q())
        };
        let x = new_knots(rng, &k, deg);
        if x.is_empty() {
            continue;
        }
        if let Ok(next) = tool1::insert_knots(&sur, &x, dir) {
            // Keep interior multiplicities at most the degree (a valid surface).
            let kk = if dir == Direction::U {
                next.u_knots()
            } else {
                next.v_knots()
            };
            if valid(kk, deg) {
                sur = next;
            }
        }
    }
    inline(&sur)
}

pub(crate) fn valid(k: &[f64], p: usize) -> bool {
    crate::units::nlib_srf_tool_p2::knots_valid(k, p)
}

/// `w` -> NOW (Euclidean coordinates) or NOW -> random weights.
pub(crate) fn flip_rational(rng: &mut Rng, s: &mut InlineSrf) {
    let rational = s
        .net
        .first()
        .and_then(|r| r.first())
        .is_some_and(|c| c[3] != NOW);
    for row in &mut s.net {
        for c in row.iter_mut() {
            if rational {
                let w = c[3];
                let z = if c[2] == NOZ { NOZ } else { c[2] / w };
                *c = [c[0] / w, c[1] / w, z, NOW];
            } else {
                let w = rng.uniform(0.5, 2.0);
                let z = if c[2] == NOZ { NOZ } else { c[2] * w };
                *c = [c[0] * w, c[1] * w, z, w];
            }
        }
    }
}

fn gen_join(rng: &mut Rng) -> Inputs {
    let dir = if rng.chance(0.05) {
        3
    } else {
        1 + rng.below(2) as i32
    };
    let mode = rng.below(3) as u8;
    let tol = gen_tol(rng);
    let s = gen_srf(rng);
    let d = if dir == 1 { Direction::U } else { Direction::V };
    let pieces = build(&s).ok().and_then(|sur| {
        let k = if d == Direction::U {
            sur.u_knots().to_vec()
        } else {
            sur.v_knots().to_vec()
        };
        let t = param(rng, &k, false);
        tool1::split(&sur, t, d).ok()
    });
    let (mut a, mut b) = match pieces {
        Some((a, b)) if rng.chance(0.85) => (inline(&a), inline(&b)),
        _ => {
            // Independent surfaces, often with matching sizes across the joint.
            let a = gen_srf(rng);
            let mut b = gen_srf(rng);
            if rng.chance(0.7) {
                let cross = |x: &InlineSrf| {
                    if d == Direction::U {
                        (x.net.first().map_or(0, Vec::len), x.q, x.v.clone())
                    } else {
                        (x.net.len(), x.p, x.u.clone())
                    }
                };
                let (cnt, deg, kn) = cross(&a);
                let net: Vec<Vec<[f64; 4]>> = if d == Direction::U {
                    b.net
                        .iter()
                        .map(|row| {
                            (0..cnt)
                                .map(|j| row.get(j).or(row.last()).copied().unwrap_or([0.0; 4]))
                                .collect()
                        })
                        .collect()
                } else {
                    let rows = b.net.len().max(1);
                    (0..cnt)
                        .map(|i| b.net.get(i % rows).cloned().unwrap_or_default())
                        .collect()
                };
                b.net = net;
                if d == Direction::U {
                    b.q = deg;
                    b.v = kn;
                } else {
                    b.p = deg;
                    b.u = kn;
                }
            }
            (a, b)
        }
    };
    // Different degrees along the joint.
    if rng.chance(0.3) {
        let t = 1 + rng.below(2) as usize;
        let target = if rng.chance(0.5) { &mut a } else { &mut b };
        if let Ok(sur) = build(target) {
            if let Ok(e) = tool2::elevate_degree(&sur, t, d) {
                *target = inline(&e);
            }
        }
    }
    // Mixed rationality.
    if rng.chance(0.25) {
        let target = if rng.chance(0.5) { &mut a } else { &mut b };
        flip_rational(rng, target);
    }
    // Shift the second piece's domain.
    if rng.chance(0.3) {
        let (lo, hi) = domain(rng);
        let k = if d == Direction::U {
            &mut b.u
        } else {
            &mut b.v
        };
        let (a0, b0) = (
            k.first().copied().unwrap_or(0.0),
            k.last().copied().unwrap_or(1.0),
        );
        for x in k.iter_mut() {
            *x = lo + (*x - a0) / (b0 - a0) * (hi - lo);
        }
        let deg = if d == Direction::U { b.p } else { b.q };
        if !valid(if d == Direction::U { &b.u } else { &b.v }, deg) {
            let k = if d == Direction::U {
                &mut b.u
            } else {
                &mut b.v
            };
            let cnt = k.len();
            *k = knots(rng, cnt, deg, 0.0, 1.0);
        }
    }
    Inputs::Join {
        sur1: plain(a),
        sur2: plain(b),
        dir,
        tol,
        mode,
    }
}

fn gen_compatible(rng: &mut Rng) -> Inputs {
    let dir = gen_dir(rng);
    let cnt = 1 + rng.below(4) as usize;
    let base = gen_srf(rng);
    let mut surs = Vec::with_capacity(cnt);
    for _ in 0..cnt {
        let s = if rng.chance(0.4) {
            gen_srf(rng)
        } else {
            // Derived from a shared base: same domain, varied knots and degrees.
            let mut s = base.clone();
            if let Ok(mut sur) = build(&s) {
                if rng.chance(0.4) {
                    let t = 1 + rng.below(2) as usize;
                    let d = if rng.chance(0.5) {
                        Direction::U
                    } else {
                        Direction::V
                    };
                    if let Ok(e) = tool2::elevate_degree(&sur, t, d) {
                        sur = e;
                    }
                }
                for d in [Direction::U, Direction::V] {
                    let (k, deg) = if d == Direction::U {
                        (sur.u_knots().to_vec(), sur.p())
                    } else {
                        (sur.v_knots().to_vec(), sur.q())
                    };
                    let x = new_knots(rng, &k, deg);
                    if !x.is_empty() {
                        if let Ok(next) = tool1::insert_knots(&sur, &x, d) {
                            let kk = if d == Direction::U {
                                next.u_knots()
                            } else {
                                next.v_knots()
                            };
                            if valid(kk, deg) {
                                sur = next;
                            }
                        }
                    }
                }
                s = inline(&sur);
            }
            if rng.chance(0.2) {
                flip_rational(rng, &mut s);
            }
            s
        };
        surs.push(plain(s));
    }
    Inputs::Compatible { surs, dir }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    match rng.below(10) {
        0..=3 => Inputs::RemoveAll {
            sur: plain(gen_refined(rng)),
            tol: gen_tol(rng),
            dir: gen_dir(rng),
            inplace: rng.chance(0.3),
        },
        4..=6 => gen_join(rng),
        _ => gen_compatible(rng),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.srf_tool_p3"
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The generator reaches the outcomes that matter: removals that remove
    /// something and nothing, joins that succeed and fail, compatible sets.
    #[test]
    fn generated_cases_cover_outcomes() {
        let mut stats = [0usize; 6];
        for seed in 0..2400u64 {
            let mut rng = Rng::new(seed);
            let i = gen_case(&mut rng);
            let Ok(flat) = rust_flat(&i) else { continue };
            let ok = flat.first() == Some(&0.0);
            match (&i, ok) {
                (Inputs::RemoveAll { sur, .. }, true) => {
                    let s = surface(sur).unwrap();
                    let removed =
                        flat[1] as usize + 1 < s.net.len() || flat[2] as usize + 1 < s.net[0].len();
                    stats[usize::from(removed)] += 1;
                }
                (Inputs::Join { .. }, true) => stats[2] += 1,
                (Inputs::Join { .. }, false) => stats[3] += 1,
                (Inputs::Compatible { .. }, true) => stats[4] += 1,
                _ => stats[5] += 1,
            }
        }
        eprintln!("{stats:?}");
        assert!(stats[..5].iter().all(|&k| k >= 10), "{stats:?}");
    }

    #[test]
    fn derived_inputs_resolve() {
        let base = InlineSrf {
            net: (0..4)
                .map(|i| {
                    (0..3)
                        .map(|j| [f64::from(i), f64::from(j), 0.0, NOW])
                        .collect()
                })
                .collect(),
            p: 2,
            q: 1,
            u: vec![0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0],
            v: vec![0.0, 0.0, 0.5, 1.0, 1.0],
        };
        let d = Src::Derived {
            base: Box::new(plain(base)),
            elevate_u: 1,
            elevate_v: 0,
            insert_u: vec![0.25],
            insert_v: vec![],
            split: Some((0.5, 1, 1)),
        };
        let s = surface(&d).unwrap();
        assert_eq!(s.p, 3);
        assert_eq!(s.u.first(), Some(&0.5));
    }
}
