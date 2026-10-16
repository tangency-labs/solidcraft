//! Runner for unit `nlib.srf_tool_p4`: NLib surface knot removal that keeps a list
//! of knots, and making a set of surfaces compatible in both directions.
//!
//! C++: `solidcore_ffi::units::nlib_srf_tool_p4` (upstream
//! `source/SMLib/src/NL_SrfTool.cpp`). Rust: `solidcore::nurbs::srf_tool::p4`.
//!
//! Each case runs one *op* (`inputs.op`). Both sides produce the same flat record of
//! doubles, split into keys by [`to_output`]: `status` (`0` ok, else the `NL_*_ERR`
//! number; `-1` on the Rust side for a non-NLib error, which never matches), then on
//! success the surface(s): `dims = [n, m, p, q, r, s]`, the control points (`net`,
//! x y z w row-major) and the knots (`u`, `v`); `compatible_uv` dumps every surface
//! with the prefix `s<i>_`. Upstream's in-place mode (`inplace`) is compared against
//! the same Rust call.
//!
//! Surface inputs are those of the `nlib.srf_tool_p3` runner (inline, an upstream
//! `.sms` file, or derived from another input). A knot constraint list (`keep_u`,
//! `keep_v`) is absent (`NULL`), an explicit list, or `{"pick": [i, ...], "ends":
//! bool}`: the `i`-th distinct interior knots (modulo their count) of the resolved
//! surface, sorted, with the end knots added when `ends` is set (for upstream
//! fixture surfaces whose knots are not written out). An empty resolved list is
//! passed as absent on both sides. Every run has at least [`MIN_GENERATED`]
//! generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::srf_basic::{Direction, Surface};
use solidcore::nurbs::srf_tool::p1 as tool1;
use solidcore::nurbs::srf_tool::p2 as tool2;
use solidcore::nurbs::srf_tool::p4 as tool;
use solidcore_ffi::units::nlib_srf_tool_p4::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::units::nlib_srf_tool_p2::{
    build, direction, dump, encode, gen_srf, harness, inline, split_srf, status, take, InlineSrf,
};
use crate::units::nlib_srf_tool_p3::{
    flip_rational, gen_dir, gen_refined, gen_tol, new_knots, plain, surface, valid, Src,
};
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2400;

/// `nlib.srf_tool_p4` runner.
pub struct Runner;

/// A knot constraint list.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum Keep {
    /// Distinct interior knots by index, plus the end knots when `ends` is set.
    Pick {
        pick: Vec<usize>,
        #[serde(default)]
        ends: bool,
    },
    /// The list as given.
    List(Vec<f64>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    RemoveConstrained {
        sur: Src,
        #[serde(default)]
        keep_u: Option<Keep>,
        #[serde(default)]
        keep_v: Option<Keep>,
        tol: f64,
        dir: i32,
        #[serde(default)]
        inplace: bool,
    },
    CompatibleUv {
        surs: Vec<Src>,
    },
}

/// The distinct interior knots of `k`.
fn interior(k: &[f64]) -> Vec<f64> {
    let (Some(&a), Some(&b)) = (k.first(), k.last()) else {
        return Vec::new();
    };
    let mut x: Vec<f64> = k.iter().copied().filter(|&t| t != a && t != b).collect();
    x.dedup();
    x
}

/// The list `keep` stands for on knots `k` (`None` when absent or empty).
fn resolve_keep(keep: Option<&Keep>, k: &[f64]) -> Option<Vec<f64>> {
    let list = match keep? {
        Keep::List(l) => l.clone(),
        Keep::Pick { pick, ends } => {
            let x = interior(k);
            let mut l: Vec<f64> = if x.is_empty() {
                Vec::new()
            } else {
                pick.iter()
                    .filter_map(|&i| x.get(i % x.len()).copied())
                    .collect()
            };
            if *ends {
                l.extend(k.first().copied());
                l.extend(k.last().copied());
            }
            l.sort_by(f64::total_cmp);
            l
        }
    };
    (!list.is_empty()).then_some(list)
}

/// The resolved inputs of `RemoveConstrained`: surface and both lists.
type Resolved = (InlineSrf, Option<Vec<f64>>, Option<Vec<f64>>);

fn resolve_remove(
    sur: &Src,
    ku: Option<&Keep>,
    kv: Option<&Keep>,
) -> Result<Resolved, HarnessError> {
    let s = surface(sur)?;
    let lu = resolve_keep(ku, &s.u);
    let lv = resolve_keep(kv, &s.v);
    Ok((s, lu, lv))
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
            Inputs::RemoveConstrained { .. } => {
                out = split_srf(flat, &mut at, "", out)?;
            }
            Inputs::CompatibleUv { surs } => {
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

fn push_list(o: &mut Vec<f64>, l: Option<&Vec<f64>>) {
    match l {
        Some(l) => {
            o.push(l.len() as f64);
            o.extend(l);
        }
        None => o.push(0.0),
    }
}

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut o = Vec::new();
    let op = match i {
        Inputs::RemoveConstrained {
            sur,
            keep_u,
            keep_v,
            tol,
            dir,
            inplace,
        } => {
            let (s, lu, lv) = resolve_remove(sur, keep_u.as_ref(), keep_v.as_ref())?;
            encode(&s, &mut o)?;
            push_list(&mut o, lu.as_ref());
            push_list(&mut o, lv.as_ref());
            o.extend([*tol, f64::from(*dir), if *inplace { 1.0 } else { 0.0 }]);
            Op::RemoveKnotsConstraints
        }
        Inputs::CompatibleUv { surs } => {
            o.push(surs.len() as f64);
            for s in surs {
                encode(&surface(s)?, &mut o)?;
            }
            Op::MakeCompatibleUv
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
        Inputs::RemoveConstrained {
            sur,
            keep_u,
            keep_v,
            tol,
            dir,
            ..
        } => {
            let (s, lu, lv) = resolve_remove(sur, keep_u.as_ref(), keep_v.as_ref())?;
            let s = build(&s)?;
            let r = tool::remove_knots_constrained(
                &s,
                lu.as_deref(),
                lv.as_deref(),
                *tol,
                direction(*dir)?,
            );
            o.push(status(&r));
            if let Ok(q) = r {
                dump(&mut o, &q);
            }
        }
        Inputs::CompatibleUv { surs } => {
            let mut v = surs
                .iter()
                .map(|s| build(&surface(s)?))
                .collect::<Result<Vec<Surface>, _>>()?;
            let r = tool::make_compatible_uv(&mut v);
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

/// A random constraint list for knots `k` of degree `deg`.
fn gen_keep(rng: &mut Rng, k: &[f64], deg: usize) -> Option<Keep> {
    if rng.chance(0.3) {
        return None;
    }
    let (Some(&a), Some(&b)) = (k.first(), k.last()) else {
        return None;
    };
    let x = interior(k);
    let all = rng.chance(0.1);
    let mut l: Vec<f64> = Vec::new();
    if rng.chance(0.35) {
        for _ in 0..=rng.below(deg as u64 + 1) {
            l.push(a);
        }
    }
    for &t in &x {
        if all || rng.chance(0.4) {
            l.push(t);
            if rng.chance(0.2) {
                l.push(t);
            }
        }
    }
    if rng.chance(0.1) {
        // A value that is not a knot (stops later matches upstream).
        l.push(rng.uniform(a, b));
    }
    if rng.chance(0.3) {
        l.push(b);
    }
    if rng.chance(0.95) {
        l.sort_by(f64::total_cmp);
    }
    if l.iter().all(|&t| t == a) {
        // Upstream reads past a list holding only the first knot.
        if l.is_empty() && rng.chance(0.5) {
            return None;
        }
        l.push(x.first().copied().unwrap_or(b));
    }
    Some(Keep::List(l))
}

fn gen_remove(rng: &mut Rng) -> Inputs {
    let s = gen_refined(rng);
    let mut s = s;
    if rng.chance(0.15) {
        flip_rational(rng, &mut s);
    }
    let keep_u = gen_keep(rng, &s.u, s.p);
    let keep_v = gen_keep(rng, &s.v, s.q);
    Inputs::RemoveConstrained {
        sur: plain(s),
        keep_u,
        keep_v,
        tol: gen_tol(rng),
        dir: gen_dir(rng),
        inplace: rng.chance(0.3),
    }
}

/// A surface derived from `base`: optional degree elevation and knot insertion.
fn derive(rng: &mut Rng, base: &InlineSrf) -> InlineSrf {
    let Ok(mut sur) = build(base) else {
        return base.clone();
    };
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
        if x.is_empty() {
            continue;
        }
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
    inline(&sur)
}

fn gen_compatible(rng: &mut Rng) -> Inputs {
    let cnt = 1 + rng.below(4) as usize;
    let base = gen_srf(rng);
    let mut surs = Vec::with_capacity(cnt);
    for _ in 0..cnt {
        let mut s = if rng.chance(0.4) {
            gen_srf(rng)
        } else {
            derive(rng, &base)
        };
        if rng.chance(0.2) {
            flip_rational(rng, &mut s);
        }
        surs.push(plain(s));
    }
    Inputs::CompatibleUv { surs }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    if rng.below(10) < 7 {
        gen_remove(rng)
    } else {
        gen_compatible(rng)
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.srf_tool_p4"
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

    /// The generator reaches the outcomes that matter: constrained removals that
    /// keep a knot the unconstrained removal drops, removals of nothing, and
    /// compatible sets.
    #[test]
    fn generated_cases_cover_outcomes() {
        let mut stats = [0usize; 4];
        for seed in 0..2400u64 {
            let mut rng = Rng::new(seed);
            let i = gen_case(&mut rng);
            let Ok(flat) = rust_flat(&i) else { continue };
            let ok = flat.first() == Some(&0.0);
            match (&i, ok) {
                (Inputs::RemoveConstrained { sur, tol, dir, .. }, true) => {
                    let s = build(&surface(sur).unwrap()).unwrap();
                    let all = solidcore::nurbs::srf_tool::p3::remove_all_knots(
                        &s,
                        *tol,
                        direction(*dir).unwrap(),
                    );
                    let kept = matches!(&all, Ok(a) if a.n() < flat[1] as usize || a.m() < flat[2] as usize);
                    stats[usize::from(kept)] += 1;
                }
                (Inputs::CompatibleUv { .. }, true) => stats[2] += 1,
                _ => stats[3] += 1,
            }
        }
        eprintln!("{stats:?}");
        assert!(stats[..3].iter().all(|&k| k >= 10), "{stats:?}");
    }

    #[test]
    fn pick_resolves_on_knots() {
        let k = [0.0, 0.0, 0.2, 0.5, 0.5, 0.9, 1.0, 1.0];
        let keep = Keep::Pick {
            pick: vec![2, 0, 4],
            ends: true,
        };
        assert_eq!(
            resolve_keep(Some(&keep), &k),
            Some(vec![0.0, 0.2, 0.5, 0.9, 1.0])
        );
        assert_eq!(resolve_keep(Some(&Keep::List(vec![])), &k), None);
        assert_eq!(resolve_keep(None, &k), None);
    }
}
