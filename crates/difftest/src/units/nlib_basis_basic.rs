//! Runner for unit `nlib.basis_basic`: basis extrema and derivatives, knot
//! reparametrisation and knot merging.
//!
//! C++: `solidcore_ffi::units::nlib_basis_basic` (upstream `source/SMLib/src/NL_BasisBasic.cpp`).
//! Rust: `solidcore::nurbs::basis_basic`.
//!
//! | op (`inputs.op`)    | Rust                                   | upstream                          |
//! |---------------------|----------------------------------------|-----------------------------------|
//! | `global_max`        | `find_global_max`                      | `N_BasisFindGlobalMax`            |
//! | `span_maxima`       | `find_all_span_maxima`                 | `N_BasisFindAllSpanMaxima`        |
//! | `reparam`           | `reparam`                              | `N_BasisReparam`                  |
//! | `compatible`        | `compatible_knot_vectors_to_tol` / `compatible_knot_array` (`tol` absent) | `N_GetCompatibleKnotVectorToTol` / `N_GetCompatibleKnotArray` |
//! | `derivs`            | `basis_derivs`                         | `N_BasisDerivs`                   |
//! | `make_compatible`   | `make_knots_compatible`                | `N_MakeKnotsCompatible`           |
//!
//! Errors are normalised to `nlib_<NL_*_ERR number>` on both sides. Every run has at
//! least [`MIN_GENERATED`] generated inputs (more with `--seeds`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::basis_basic as bb;
use solidcore::nurbs::geom::Interval;
use solidcore::nurbs::knots::{KnotVector, Side};
use solidcore::SolidError;
use solidcore_ffi::units::nlib_basis_basic as ffi;
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1500;

/// `nlib.basis_basic` runner.
pub struct Runner;

/// `NL_LEFT` / `NL_RIGHT`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Flag {
    Left,
    Right,
}

impl Flag {
    fn side(self) -> Side {
        match self {
            Flag::Left => Side::Left,
            Flag::Right => Side::Right,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    GlobalMax {
        knots: Vec<f64>,
        index: usize,
        degree: usize,
        tol: f64,
    },
    SpanMaxima {
        knots: Vec<f64>,
        index: usize,
        degree: usize,
        tol: f64,
    },
    Reparam {
        knots: Vec<f64>,
        degree: usize,
        ul: f64,
        ur: f64,
    },
    Compatible {
        knots: Vec<Vec<f64>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tol: Option<f64>,
    },
    Derivs {
        knots: Vec<f64>,
        degree: usize,
        u: f64,
        flag: Flag,
        der: usize,
    },
    MakeCompatible {
        r: Vec<f64>,
        s: Vec<f64>,
        p: usize,
        q: usize,
    },
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

fn i32_of(v: usize) -> Result<i32, HarnessError> {
    i32::try_from(v).map_err(|_| harness(format!("{v} does not fit i32")))
}

fn i64_of(v: usize) -> Result<i64, HarnessError> {
    i64::try_from(v).map_err(|_| harness(format!("{v} does not fit i64")))
}

/// C++ failure -> normalised output (operation error) or harness error.
fn cpp_err(e: &FfiError) -> Result<Output, HarnessError> {
    match e {
        FfiError::Status(code) if *code > 0 => Ok(Output::err(format!("nlib_{code}"))),
        FfiError::Status(_) => Err(harness(format!("shim failure: {e}"))),
        FfiError::InvalidArgument(m) => Err(harness(format!("ffi rejected: {m}"))),
    }
}

/// Rust failure -> normalised output (`Unported` -> deferred case). `InvalidInput`
/// (out-of-bounds or endless upstream loop) cannot match any C++ output, so such a
/// case fails, as it should within the wrapper's domain.
fn rust_err(e: &SolidError) -> Output {
    if let Some(d) = Output::unported(e) {
        return d;
    }
    match e {
        SolidError::Nlib { code, .. } => Output::err(format!("nlib_{}", code.code())),
        other => Output::err(format!("rust: {other}")),
    }
}

fn nested(key: &str, out: Output, rows: &[Vec<f64>]) -> Output {
    let lens: Vec<i64> = rows.iter().map(|r| r.len() as i64).collect();
    let flat: Vec<f64> = rows.iter().flatten().copied().collect();
    out.ints(&format!("{key}_lens"), lens).values(key, flat)
}

fn cpp(i: Inputs) -> Result<Output, HarnessError> {
    let out = match i {
        Inputs::GlobalMax {
            knots,
            index,
            degree,
            tol,
        } => ffi::global_max(&knots, i64_of(index)?, i32_of(degree)?, tol)
            .map(|(m, u)| Output::new().scalar("max", m).scalar("u", u)),
        Inputs::SpanMaxima {
            knots,
            index,
            degree,
            tol,
        } => ffi::span_maxima(&knots, i64_of(index)?, i32_of(degree)?, tol).map(|(mn, mx, u)| {
            Output::new()
                .values("min", mn)
                .values("max", mx)
                .scalar("u", u)
        }),
        Inputs::Reparam {
            knots,
            degree,
            ul,
            ur,
        } => {
            ffi::reparam(&knots, i32_of(degree)?, ul, ur).map(|k| Output::new().values("knots", k))
        }
        Inputs::Compatible { knots, tol } => ffi::compatible(&knots, tol).map(|c| {
            let out = nested("knots", Output::new(), &c.adjusted);
            nested("new", out, &c.new)
        }),
        Inputs::Derivs {
            knots,
            degree,
            u,
            flag,
            der,
        } => ffi::derivs(&knots, i32_of(degree)?, u, flag.side().flag(), i64_of(der)?)
            .map(|(span, nd)| nested("nd", Output::new().int("span", span), &nd)),
        Inputs::MakeCompatible { r, s, p, q } => {
            ffi::make_compatible(&r, &s, i32_of(p)?, i32_of(q)?).map(|m| {
                Output::new()
                    .values("r", m.r)
                    .values("s", m.s)
                    .values("xr", m.xr)
                    .values("xs", m.xs)
            })
        }
    };
    out.or_else(|e| cpp_err(&e))
}

fn kv(knots: &[f64]) -> Result<KnotVector<'_>, HarnessError> {
    KnotVector::from_slice(knots).map_err(|e| harness(e.to_string()))
}

fn rust(i: Inputs) -> Result<Output, HarnessError> {
    let out: Result<Output, SolidError> = match i {
        Inputs::GlobalMax {
            knots,
            index,
            degree,
            tol,
        } => bb::find_global_max(&kv(&knots)?, index, degree, tol)
            .map(|(m, u)| Output::new().scalar("max", m).scalar("u", u)),
        Inputs::SpanMaxima {
            knots,
            index,
            degree,
            tol,
        } => bb::find_all_span_maxima(&kv(&knots)?, index, degree, tol).map(|s| {
            Output::new()
                .values("min", s.min)
                .values("max", s.max)
                .scalar("u", s.u)
        }),
        Inputs::Reparam {
            mut knots,
            degree,
            ul,
            ur,
        } => bb::reparam(&mut knots, degree, Interval::new(ul, ur))
            .map(|()| Output::new().values("knots", knots)),
        Inputs::Compatible { mut knots, tol } => {
            let res = match tol {
                Some(t) => bb::compatible_knot_vectors_to_tol(&mut knots, t),
                None => bb::compatible_knot_array(&mut knots),
            };
            res.map(|new| {
                let out = nested("knots", Output::new(), &knots);
                nested("new", out, &new)
            })
        }
        Inputs::Derivs {
            knots,
            degree,
            u,
            flag,
            der,
        } => match bb::basis_derivs(&kv(&knots)?, degree, u, flag.side(), der) {
            Ok(b) => Ok(nested(
                "nd",
                Output::new().int("span", i64_of(b.span)?),
                &b.derivs,
            )),
            Err(e) => Err(e),
        },
        Inputs::MakeCompatible { mut r, mut s, p, q } => {
            bb::make_knots_compatible(&mut r, &mut s, p, q).map(|(xr, xs)| {
                Output::new()
                    .values("r", r)
                    .values("s", s)
                    .values("xr", xr)
                    .values("xs", xs)
            })
        }
    };
    Ok(out.unwrap_or_else(|e| rust_err(&e)))
}

// ---- generator -------------------------------------------------------------------

/// Kind of knot vector to generate.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// End multiplicity p+1, interior multiplicity <= p.
    Clamped,
    /// Non-decreasing knots without clamped ends.
    Unclamped,
    /// Clamped but with an interior knot of multiplicity p or more.
    OverMultiple,
    /// Arbitrary (possibly decreasing) knots.
    Scrambled,
}

fn pick_shape(rng: &mut Rng) -> Shape {
    let r = rng.unit();
    if r < 0.6 {
        Shape::Clamped
    } else if r < 0.78 {
        Shape::Unclamped
    } else if r < 0.93 {
        Shape::OverMultiple
    } else {
        Shape::Scrambled
    }
}

fn pick_degree(rng: &mut Rng) -> usize {
    let r = rng.unit();
    if r < 0.06 {
        0
    } else if r < 0.88 {
        1 + rng.below(5) as usize
    } else {
        6 + rng.below(7) as usize
    }
}

/// Domain `[a, b]` and whether knots are integral.
fn gen_domain(rng: &mut Rng) -> (f64, f64, bool) {
    let integral = rng.chance(0.3);
    if integral {
        let a = rng.range_i64(-5, 5) as f64;
        (a, a + rng.range_i64(1, 12) as f64, true)
    } else {
        let scale = *rng
            .choose(&[1e-3, 1.0, 1.0, 1.0, 100.0, 1e4])
            .unwrap_or(&1.0);
        let a = rng.uniform(-10.0, 10.0) * scale;
        (a, a + rng.uniform(0.01, 20.0) * scale, false)
    }
}

/// Sorted interior knots strictly inside `(a, b)`, at least `margin * (b-a)` from the
/// ends, with multiplicities up to `max_mult`.
fn gen_interior(
    rng: &mut Rng,
    a: f64,
    b: f64,
    integral: bool,
    max_mult: usize,
    margin: f64,
) -> Vec<f64> {
    let n = rng.below(10) as usize;
    let lo = a + margin * (b - a);
    let hi = b - margin * (b - a);
    let mut v: Vec<f64> = (0..n)
        .map(|_| {
            if integral {
                rng.range_i64(a as i64, b as i64) as f64
            } else {
                rng.uniform(lo, hi)
            }
        })
        .filter(|&x| x > lo && x < hi)
        .collect();
    v.sort_by(f64::total_cmp);
    let mut out = Vec::new();
    for x in v {
        let mut run = out.iter().rev().take_while(|&&w| w == x).count();
        if run < max_mult {
            out.push(x);
            run += 1;
        }
        while run < max_mult && rng.chance(0.3) {
            out.push(x);
            run += 1;
        }
    }
    out
}

fn gen_knots(rng: &mut Rng, p: usize, shape: Shape) -> Vec<f64> {
    let (a, b, integral) = gen_domain(rng);
    let max_mult = if shape == Shape::OverMultiple {
        p + 1
    } else {
        p.max(1)
    };
    let mut inner = gen_interior(rng, a, b, integral, max_mult, 0.0);
    if shape == Shape::OverMultiple {
        let v = 0.5 * (a + b);
        let at = inner.partition_point(|&w| w < v);
        let extra = p.max(1) + rng.below(2) as usize;
        for _ in 0..extra {
            inner.insert(at, v);
        }
    }
    let mut knots = Vec::new();
    match shape {
        Shape::Clamped | Shape::OverMultiple | Shape::Scrambled => {
            knots.extend(std::iter::repeat_n(a, p + 1));
            knots.extend(inner);
            knots.extend(std::iter::repeat_n(b, p + 1));
            if shape == Shape::Scrambled && knots.len() >= 2 {
                let i = rng.below(knots.len() as u64) as usize;
                let j = rng.below(knots.len() as u64) as usize;
                knots.swap(i, j);
                if let Some(k) = knots.get_mut(i) {
                    *k += rng.uniform(-1.0, 1.0) * (b - a);
                }
            }
        }
        Shape::Unclamped => {
            let mut lo = a;
            let mut pre: Vec<f64> = (0..p)
                .map(|_| {
                    lo -= rng.uniform(0.1, 2.0) * (b - a) / 4.0;
                    lo
                })
                .collect();
            pre.reverse();
            knots.extend(pre);
            knots.push(a);
            knots.extend(inner);
            knots.push(b);
            let mut hi = b;
            for _ in 0..p {
                if rng.chance(0.3) {
                    knots.push(hi);
                } else {
                    hi += rng.uniform(0.1, 2.0) * (b - a) / 4.0;
                    knots.push(hi);
                }
            }
        }
    }
    while knots.len() < 2 * p + 2 {
        let last = knots.last().copied().unwrap_or(b);
        knots.push(last);
    }
    knots
}

fn gen_param(rng: &mut Rng, knots: &[f64], p: usize) -> f64 {
    let first = knots.first().copied().unwrap_or(0.0);
    let last = knots.last().copied().unwrap_or(1.0);
    let m = knots.len().saturating_sub(1);
    let dom_lo = knots.get(p).copied().unwrap_or(first);
    let dom_hi = knots.get(m.saturating_sub(p)).copied().unwrap_or(last);
    let r = rng.unit();
    if r < 0.45 {
        let (lo, hi) = if dom_lo < dom_hi {
            (dom_lo, dom_hi)
        } else {
            (first, last)
        };
        rng.uniform(lo, hi)
    } else if r < 0.65 {
        *rng.choose(knots).unwrap_or(&first)
    } else if r < 0.78 {
        *rng.choose(&[dom_lo, dom_hi, first, last]).unwrap_or(&first)
    } else if r < 0.85 {
        if rng.chance(0.5) {
            first - rng.uniform(0.0, 2.0)
        } else {
            last + rng.uniform(0.0, 2.0)
        }
    } else {
        let k = *rng.choose(knots).unwrap_or(&first);
        if rng.chance(0.5) {
            k.next_up()
        } else {
            k.next_down()
        }
    }
}

/// Largest `der` for which `p!/(p-k)!`, `k = 1..=min(p,der)`, fits an `i32` (the ffi
/// domain of `N_BasisDerivs`).
fn max_der(p: usize) -> usize {
    let mut r = i64::try_from(p).unwrap_or(i64::MAX);
    for k in 1..=p {
        r = r.saturating_mul((p - k) as i64);
        if r > i64::from(i32::MAX) {
            return k - 1;
        }
    }
    usize::MAX
}

/// Mirrors the ffi check that `N_BasisFindGlobalMax`'s bracketing loop advances.
fn global_max_ok(knots: &[f64], i: usize, p: usize) -> bool {
    if p == 1 || knots.len() < p + 2 || i > knots.len() - p - 2 {
        return true;
    }
    let (Some(&ui), Some(&ue)) = (knots.get(i), knots.get(i + p + 1)) else {
        return true;
    };
    let du = (ue - ui) / (p * 10) as f64;
    let mut ur = ui;
    let mut steps = 0;
    while ur < ue {
        let mut next = ur + du;
        if next > ue {
            next = ue;
        }
        if next == ur || steps > 1_000_000 {
            return false;
        }
        ur = next;
        steps += 1;
    }
    true
}

fn gen_tol(rng: &mut Rng) -> f64 {
    *rng.choose(&[1e-14, 1e-12, 1e-10, 1e-9, 1e-8, 1e-6, 1e-4, 1e-2])
        .unwrap_or(&1e-10)
}

/// A clamped knot vector over `[a, b]` with interior knots away from the ends (the
/// `N_MakeKnotsCompatible` domain the generator keeps to).
fn gen_clamped_for_make(
    rng: &mut Rng,
    p: usize,
    a: f64,
    b: f64,
    integral: bool,
    shared: &[f64],
) -> Vec<f64> {
    let mut inner = gen_interior(rng, a, b, integral, p.max(1), 1e-3);
    for &x in shared {
        if rng.chance(0.6) {
            let jitter = if rng.chance(0.2) {
                (b - a) * 1e-13 * rng.uniform(-1.0, 1.0)
            } else {
                0.0
            };
            inner.push(x + jitter);
        }
    }
    inner.sort_by(f64::total_cmp);
    // Keep interior multiplicity <= p.
    let mut out: Vec<f64> = Vec::new();
    for x in inner {
        let run = out.iter().rev().take_while(|&&w| w == x).count();
        if run < p.max(1) {
            out.push(x);
        }
    }
    let mut k = Vec::new();
    k.extend(std::iter::repeat_n(a, p + 1));
    k.extend(out);
    k.extend(std::iter::repeat_n(b, p + 1));
    k
}

fn gen_make(rng: &mut Rng) -> Inputs {
    let mode = rng.unit();
    // Ends that differ by less than NL_PTOL are not rescaled; with degree 0 upstream
    // then reads past U[m] (see plan/findings.md), so that mode uses degrees >= 1.
    let min_deg = u64::from((0.45..0.55).contains(&mode));
    let p = (min_deg + rng.below(6 - min_deg)) as usize;
    let q = (min_deg + rng.below(6 - min_deg)) as usize;
    let (a, b, integral) = gen_domain(rng);
    let shared = gen_interior(rng, a, b, integral, 1, 1e-3);
    let r = gen_clamped_for_make(rng, p, a, b, integral, &shared);
    let (sa, sb) = if mode < 0.45 {
        (a, b)
    } else if mode < 0.55 {
        // Ends within NL_PTOL: no rescale.
        let d = (b - a) * 1e-15;
        (
            a + d * rng.uniform(-1.0, 1.0),
            b + d * rng.uniform(-1.0, 1.0),
        )
    } else {
        let (c, d, _) = gen_domain(rng);
        (c, d)
    };
    let s = if (sa, sb) == (a, b) {
        gen_clamped_for_make(rng, q, a, b, integral, &shared)
    } else {
        // Map the shared knots onto [sa, sb] so some coincide after the rescale.
        let mapped: Vec<f64> = shared
            .iter()
            .map(|&x| sa + (x - a) / (b - a) * (sb - sa))
            .collect();
        gen_clamped_for_make(rng, q, sa, sb, false, &mapped)
    };
    Inputs::MakeCompatible { r, s, p, q }
}

fn gen_compatible(rng: &mut Rng) -> Inputs {
    let nvec = 1 + rng.below(4) as usize;
    let (a, b, integral) = gen_domain(rng);
    let shared = gen_interior(rng, a, b, integral, 1, 0.0);
    let same_range = rng.chance(0.8);
    let mut knots = Vec::new();
    for _ in 0..nvec {
        let p = rng.below(5) as usize;
        let (lo, hi) = if same_range {
            (a, b)
        } else {
            let (c, d, _) = gen_domain(rng);
            (c, d)
        };
        let mut inner = gen_interior(rng, lo, hi, integral && same_range, p.max(1), 0.0);
        if same_range {
            for &x in &shared {
                if rng.chance(0.6) {
                    let jitter = if rng.chance(0.3) {
                        (hi - lo) * rng.uniform(-1e-9, 1e-9)
                    } else {
                        0.0
                    };
                    let mult = 1 + rng.below(p.max(1) as u64) as usize;
                    for _ in 0..mult {
                        inner.push(x + jitter);
                    }
                }
            }
        }
        inner.sort_by(f64::total_cmp);
        let mut k = Vec::new();
        k.extend(std::iter::repeat_n(lo, p + 1));
        k.extend(inner);
        k.extend(std::iter::repeat_n(hi, p + 1));
        if rng.chance(0.05) && k.len() >= 3 {
            // Slightly out-of-order knots (upstream's [B168]).
            let i = 1 + rng.below(k.len() as u64 - 2) as usize;
            if let Some(v) = k.get_mut(i) {
                *v -= (hi - lo) * 1e-12;
            }
        }
        while k.len() + 1 < nvec {
            k.push(hi);
        }
        knots.push(k);
    }
    let tol = if rng.chance(0.25) {
        None
    } else {
        Some(
            *rng.choose(&[1e-12, 1e-9, 1e-8, 1e-7, 1e-4, 1e-2, 0.3])
                .unwrap_or(&1e-8),
        )
    };
    Inputs::Compatible { knots, tol }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let r = rng.unit();
    if r < 0.12 {
        return gen_make(rng);
    }
    if r < 0.27 {
        return gen_compatible(rng);
    }
    let p = pick_degree(rng);
    let shape = pick_shape(rng);
    let knots = gen_knots(rng, p, shape);
    let n = knots.len().saturating_sub(p + 2);
    if r < 0.33 {
        let (lo, hi) = if rng.chance(0.8) {
            let (c, d, _) = gen_domain(rng);
            (c, d)
        } else {
            let a = knots.first().copied().unwrap_or(0.0);
            let b = knots.last().copied().unwrap_or(1.0);
            (a, if rng.chance(0.5) { b } else { b + 1.0 })
        };
        let degree = rng.below(p as u64 + 1) as usize;
        return Inputs::Reparam {
            knots,
            degree,
            ul: lo,
            ur: hi,
        };
    }
    if r < 0.6 {
        let mut index = rng.below(n as u64 + 2) as usize;
        let tries = 8;
        for _ in 0..tries {
            if global_max_ok(&knots, index, p) {
                break;
            }
            index = rng.below(n as u64 + 2) as usize;
        }
        if !global_max_ok(&knots, index, p) {
            index = n + 1; // NL_IND_ERR, no loop.
        }
        let tol = gen_tol(rng);
        return if rng.chance(0.5) {
            Inputs::GlobalMax {
                knots,
                index,
                degree: p,
                tol,
            }
        } else {
            Inputs::SpanMaxima {
                knots,
                index,
                degree: p,
                tol,
            }
        };
    }
    let u = gen_param(rng, &knots, p);
    let flag = if rng.chance(0.5) {
        Flag::Left
    } else {
        Flag::Right
    };
    let der = (rng.below(p as u64 + 3) as usize).min(max_der(p));
    Inputs::Derivs {
        knots,
        degree: p,
        u,
        flag,
        der,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.basis_basic"
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
        cpp(case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(case.parse()?)
    }
}
