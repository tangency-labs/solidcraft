//! Runner for unit `nlib.knots`: knot vectors and B-spline basis functions.
//!
//! C++: `solidcore_ffi::units::nlib_knots` (upstream `source/SMLib/src/NL_BasisBasic.cpp`).
//! Rust: `solidcore::nurbs::knots`.
//!
//! | op (`inputs.op`)     | Rust                                   | upstream                         |
//! |----------------------|----------------------------------------|----------------------------------|
//! | `roundtrip`          | `KnotVector::new` + `set_last_index`   | `N_KnotVectorFromRealArray`, `N_SetKnotIndex`, `N_KnotVectorGetKnots` |
//! | `param_checks`       | `is_param_out_of_bounds`, `is_end_param` | `N_KnotVectorIsParamOutOfBounds`, `N_KnotVectorIsEndParam` |
//! | `is_valid`           | `validate`                             | `N_KnotVectorIsValid`            |
//! | `span_count`         | `span_count`                           | `N_BasisGetSpanCount`            |
//! | `find_span`          | `find_span`                            | `N_BasisFindSpan`                |
//! | `find_span_and_mult` | `find_span_and_mult`                   | `N_BasisFindSpanAndMult`         |
//! | `basis_eval`         | `basis_eval`                           | `N_BasisEval`                    |
//! | `basis_i_eval`       | `basis_i_eval`                         | `N_BasisIEval`                   |
//! | `basis_i_derivs`     | `basis_i_derivs`                       | `N_BasisIDerivs`                 |
//!
//! Errors are normalised to `nlib_<NL_*_ERR number>` on both sides. Every case runs
//! with at least [`MIN_GENERATED`] generated inputs (more with `--seeds`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::knots::{KnotVector, Side};
use solidcore::SolidError;
use solidcore_ffi::units::nlib_knots as ffi;
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1200;

/// `nlib.knots` runner.
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
    fn code(self) -> i32 {
        self.side().flag()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    Roundtrip {
        buf: Vec<f64>,
        m: usize,
        new_m: usize,
    },
    ParamChecks {
        knots: Vec<f64>,
        u: f64,
    },
    IsValid {
        knots: Vec<f64>,
        degree: usize,
    },
    SpanCount {
        knots: Vec<f64>,
        degree: usize,
    },
    FindSpan {
        knots: Vec<f64>,
        degree: usize,
        u: f64,
        flag: Flag,
    },
    FindSpanAndMult {
        knots: Vec<f64>,
        degree: usize,
        u: f64,
        flag: Flag,
    },
    BasisEval {
        knots: Vec<f64>,
        degree: usize,
        u: f64,
        flag: Flag,
    },
    BasisIEval {
        knots: Vec<f64>,
        index: usize,
        degree: usize,
        u: f64,
        flag: Flag,
    },
    BasisIDerivs {
        knots: Vec<f64>,
        index: usize,
        degree: usize,
        u: f64,
        flag: Flag,
        der: usize,
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

/// Rust failure -> normalised output (`Unported` -> deferred case). Non-NLib errors (`InvalidInput`: an index
/// upstream would read out of bounds) cannot match any C++ output, so such a case
/// fails, as it should within the wrapper's domain.
fn rust_err(e: &SolidError) -> Output {
    if let Some(d) = Output::unported(e) {
        return d;
    }
    match e {
        SolidError::Nlib { code, .. } => Output::err(format!("nlib_{}", code.code())),
        other => Output::err(format!("rust: {other}")),
    }
}

fn cpp(i: Inputs) -> Result<Output, HarnessError> {
    let out = match i {
        Inputs::Roundtrip { buf, m, new_m } => ffi::roundtrip(&buf, i64_of(m)?, i64_of(new_m)?)
            .map(|(m, k)| Output::new().int("m", m).values("knots", k)),
        Inputs::ParamChecks { knots, u } => ffi::param_checks(&knots, u).map(|(oob, end)| {
            Output::new()
                .int("out_of_bounds", i64::from(oob))
                .int("end_param", i64::from(end))
        }),
        Inputs::IsValid { knots, degree } => {
            ffi::is_valid(&knots, i32_of(degree)?).map(|()| Output::new().int("valid", 1))
        }
        Inputs::SpanCount { knots, degree } => {
            ffi::span_count(&knots, i32_of(degree)?).map(|n| Output::new().int("count", n))
        }
        Inputs::FindSpan {
            knots,
            degree,
            u,
            flag,
        } => ffi::find_span(&knots, i32_of(degree)?, u, flag.code())
            .map(|s| Output::new().int("span", s)),
        Inputs::FindSpanAndMult {
            knots,
            degree,
            u,
            flag,
        } => ffi::find_span_and_mult(&knots, i32_of(degree)?, u, flag.code())
            .map(|(s, m)| Output::new().int("span", s).int("mult", m)),
        Inputs::BasisEval {
            knots,
            degree,
            u,
            flag,
        } => ffi::basis_eval(&knots, i32_of(degree)?, u, flag.code())
            .map(|(s, n)| Output::new().int("span", s).values("basis", n)),
        Inputs::BasisIEval {
            knots,
            index,
            degree,
            u,
            flag,
        } => ffi::basis_i_eval(&knots, i64_of(index)?, i32_of(degree)?, u, flag.code())
            .map(|v| Output::new().scalar("value", v)),
        Inputs::BasisIDerivs {
            knots,
            index,
            degree,
            u,
            flag,
            der,
        } => ffi::basis_i_derivs(
            &knots,
            i64_of(index)?,
            i32_of(degree)?,
            u,
            flag.code(),
            i64_of(der)?,
        )
        .map(|d| Output::new().values("derivs", d)),
    };
    out.or_else(|e| cpp_err(&e))
}

fn kv(knots: &[f64]) -> Result<KnotVector<'_>, HarnessError> {
    KnotVector::from_slice(knots).map_err(|e| harness(e.to_string()))
}

fn usize_out(v: usize) -> Result<i64, HarnessError> {
    i64_of(v)
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let out: Result<Output, SolidError> = match i {
        Inputs::Roundtrip { buf, m, new_m } => {
            match KnotVector::new(buf, *m).and_then(|mut k| {
                k.set_last_index(*new_m)?;
                Ok(k)
            }) {
                Ok(k) => Ok(Output::new()
                    .int("m", usize_out(k.last_index())?)
                    .values("knots", k.knots().to_vec())),
                Err(e) => Err(e),
            }
        }
        Inputs::ParamChecks { knots, u } => {
            let k = kv(knots)?;
            Ok(Output::new()
                .int("out_of_bounds", i64::from(k.is_param_out_of_bounds(*u)))
                .int("end_param", i64::from(k.is_end_param(*u))))
        }
        Inputs::IsValid { knots, degree } => kv(knots)?
            .validate(*degree)
            .map(|()| Output::new().int("valid", 1)),
        Inputs::SpanCount { knots, degree } => {
            let n = kv(knots)?.span_count(*degree);
            Ok(Output::new().int("count", usize_out(n)?))
        }
        Inputs::FindSpan {
            knots,
            degree,
            u,
            flag,
        } => match kv(knots)?.find_span(*degree, *u, flag.side()) {
            Ok(s) => Ok(Output::new().int("span", usize_out(s)?)),
            Err(e) => Err(e),
        },
        Inputs::FindSpanAndMult {
            knots,
            degree,
            u,
            flag,
        } => match kv(knots)?.find_span_and_mult(*degree, *u, flag.side()) {
            Ok((s, m)) => Ok(Output::new()
                .int("span", usize_out(s)?)
                .int("mult", usize_out(m)?)),
            Err(e) => Err(e),
        },
        Inputs::BasisEval {
            knots,
            degree,
            u,
            flag,
        } => match kv(knots)?.basis_eval(*degree, *u, flag.side()) {
            Ok(b) => Ok(Output::new()
                .int("span", usize_out(b.span)?)
                .values("basis", b.values)),
            Err(e) => Err(e),
        },
        Inputs::BasisIEval {
            knots,
            index,
            degree,
            u,
            flag,
        } => kv(knots)?
            .basis_i_eval(*index, *degree, *u, flag.side())
            .map(|v| Output::new().scalar("value", v)),
        Inputs::BasisIDerivs {
            knots,
            index,
            degree,
            u,
            flag,
            der,
        } => kv(knots)?
            .basis_i_derivs(*index, *degree, *u, flag.side(), *der)
            .map(|d| Output::new().values("derivs", d)),
    };
    Ok(out.unwrap_or_else(|e| rust_err(&e)))
}

// ---- generator -------------------------------------------------------------------

/// Kind of knot vector to generate.
#[derive(Clone, Copy)]
enum Shape {
    /// End multiplicity p+1, interior multiplicity <= p.
    Clamped,
    /// Non-decreasing knots without clamped ends.
    Unclamped,
    /// Clamped with extra end knots (zero-length end spans, upstream's [B405] case).
    OverClamped,
    /// Clamped but with an interior knot of multiplicity p+1 or more.
    OverMultiple,
    /// Arbitrary (possibly decreasing) knots.
    Scrambled,
}

fn pick_shape(rng: &mut Rng) -> Shape {
    let r = rng.unit();
    if r < 0.55 {
        Shape::Clamped
    } else if r < 0.72 {
        Shape::Unclamped
    } else if r < 0.84 {
        Shape::OverClamped
    } else if r < 0.94 {
        Shape::OverMultiple
    } else {
        Shape::Scrambled
    }
}

fn pick_degree(rng: &mut Rng) -> usize {
    let r = rng.unit();
    if r < 0.08 {
        0
    } else if r < 0.85 {
        1 + rng.below(5) as usize
    } else if r < 0.97 {
        6 + rng.below(6) as usize
    } else {
        // NL_MAXDEG and NL_MAXDEG + 1 (the largest upstream N_BasisEval accepts).
        17 + rng.below(2) as usize
    }
}

/// A knot value: integer-valued (exact ties and dyadic arithmetic) or arbitrary.
fn knot_value(rng: &mut Rng, a: f64, b: f64, integral: bool) -> f64 {
    if integral {
        rng.range_i64(a as i64, b as i64) as f64
    } else {
        rng.uniform(a, b)
    }
}

fn gen_knots(rng: &mut Rng, p: usize, shape: Shape) -> Vec<f64> {
    let integral = rng.chance(0.3);
    let (a, b) = if integral {
        let a = rng.range_i64(-5, 5) as f64;
        (a, a + rng.range_i64(1, 12) as f64)
    } else {
        let scale = *rng
            .choose(&[1e-3, 1.0, 1.0, 1.0, 100.0, 1e6])
            .unwrap_or(&1.0);
        let a = rng.uniform(-10.0, 10.0) * scale;
        (a, a + rng.uniform(0.01, 20.0) * scale)
    };
    let n_interior = rng.below(12) as usize;
    let mut interior: Vec<f64> = (0..n_interior)
        .map(|_| knot_value(rng, a, b, integral))
        .filter(|&v| v > a && v < b)
        .collect();
    interior.sort_by(f64::total_cmp);
    // Repeat some interior knots up to multiplicity p (or beyond for OverMultiple).
    let max_mult = match shape {
        Shape::OverMultiple => p + 2,
        _ => p.max(1),
    };
    let mut with_mult = Vec::new();
    for v in interior {
        let mut run = with_mult.iter().rev().take_while(|&&w| w == v).count();
        if run < max_mult {
            with_mult.push(v);
            run += 1;
        }
        while run < max_mult && rng.chance(0.25) {
            with_mult.push(v);
            run += 1;
        }
    }
    if matches!(shape, Shape::OverMultiple) {
        let v = 0.5 * (a + b);
        let at = with_mult.partition_point(|&w| w < v);
        for _ in 0..=p {
            with_mult.insert(at, v);
        }
    }
    let mut knots = Vec::new();
    match shape {
        Shape::Clamped | Shape::OverMultiple => {
            knots.extend(std::iter::repeat_n(a, p + 1));
            knots.extend(with_mult);
            knots.extend(std::iter::repeat_n(b, p + 1));
        }
        Shape::OverClamped => {
            let extra_a = rng.below(p as u64 + 2) as usize;
            let extra_b = rng.below(p as u64 + 2) as usize;
            knots.extend(std::iter::repeat_n(a, p + 1 + extra_a));
            knots.extend(with_mult);
            knots.extend(std::iter::repeat_n(b, p + 1 + extra_b));
        }
        Shape::Unclamped => {
            let lead = rng.below(p as u64 + 1) as usize;
            let mut lo = a;
            let mut pre: Vec<f64> = (0..p + 1 - lead.min(p))
                .map(|_| {
                    lo -= rng.uniform(0.1, 2.0);
                    lo
                })
                .collect();
            pre.reverse();
            knots.extend(pre);
            knots.extend(std::iter::repeat_n(a, lead.max(1)));
            knots.extend(with_mult);
            let mut hi = b;
            knots.push(b);
            for _ in 0..p {
                if rng.chance(0.3) {
                    knots.push(hi);
                } else {
                    hi += rng.uniform(0.1, 2.0);
                    knots.push(hi);
                }
            }
        }
        Shape::Scrambled => {
            knots.extend(std::iter::repeat_n(a, p + 1));
            knots.extend(with_mult);
            knots.extend(std::iter::repeat_n(b, p + 1));
            if knots.len() >= 2 {
                let i = rng.below(knots.len() as u64) as usize;
                let j = rng.below(knots.len() as u64) as usize;
                knots.swap(i, j);
                if let Some(k) = knots.get_mut(i) {
                    *k += rng.uniform(-1.0, 1.0);
                }
            }
        }
    }
    // The span/basis routines need m >= 2p+1.
    while knots.len() < 2 * p + 2 {
        let last = knots.last().copied().unwrap_or(b);
        knots.push(last);
    }
    knots
}

/// A parameter: interior, exactly a knot, the domain or vector ends, outside, a
/// span midpoint, or one ulp away from a knot.
fn gen_param(rng: &mut Rng, knots: &[f64], p: usize) -> f64 {
    let first = knots.first().copied().unwrap_or(0.0);
    let last = knots.last().copied().unwrap_or(1.0);
    let m = knots.len().saturating_sub(1);
    let dom_lo = knots.get(p).copied().unwrap_or(first);
    let dom_hi = knots.get(m.saturating_sub(p)).copied().unwrap_or(last);
    let any_knot = |rng: &mut Rng| *rng.choose(knots).unwrap_or(&first);
    let r = rng.unit();
    if r < 0.35 {
        let (lo, hi) = if dom_lo < dom_hi {
            (dom_lo, dom_hi)
        } else {
            (first, last)
        };
        rng.uniform(lo, hi)
    } else if r < 0.55 {
        any_knot(rng)
    } else if r < 0.70 {
        *rng.choose(&[dom_lo, dom_hi, first, last]).unwrap_or(&first)
    } else if r < 0.80 {
        if rng.chance(0.5) {
            first - rng.uniform(0.0, 2.0)
        } else {
            last + rng.uniform(0.0, 2.0)
        }
    } else if r < 0.90 {
        let j = rng.below(m.max(1) as u64) as usize;
        let a = knots.get(j).copied().unwrap_or(first);
        let b = knots.get(j + 1).copied().unwrap_or(last);
        0.5 * (a + b)
    } else {
        let k = any_knot(rng);
        if rng.chance(0.5) {
            k.next_up()
        } else {
            k.next_down()
        }
    }
}

fn gen_flag(rng: &mut Rng) -> Flag {
    if rng.chance(0.5) {
        Flag::Left
    } else {
        Flag::Right
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let p = pick_degree(rng);
    let shape = pick_shape(rng);
    let knots = gen_knots(rng, p, shape);
    let u = gen_param(rng, &knots, p);
    let flag = gen_flag(rng);
    // n = m - p - 1 >= p >= 0 by construction.
    let n = knots.len().saturating_sub(p + 2);
    let r = rng.unit();
    if r < 0.04 {
        let len = knots.len();
        let m = rng.below(len as u64) as usize;
        let new_m = rng.below(len as u64) as usize;
        Inputs::Roundtrip {
            buf: knots,
            m,
            new_m,
        }
    } else if r < 0.09 {
        Inputs::ParamChecks { knots, u }
    } else if r < 0.17 {
        // Sometimes the "wrong" degree.
        let degree = if rng.chance(0.7) {
            p
        } else {
            rng.below(p as u64 + 3) as usize
        };
        Inputs::IsValid { knots, degree }
    } else if r < 0.21 {
        Inputs::SpanCount { knots, degree: p }
    } else if r < 0.33 {
        Inputs::FindSpan {
            knots,
            degree: p,
            u,
            flag,
        }
    } else if r < 0.43 {
        Inputs::FindSpanAndMult {
            knots,
            degree: p,
            u,
            flag,
        }
    } else if r < 0.65 {
        Inputs::BasisEval {
            knots,
            degree: p,
            u,
            flag,
        }
    } else if r < 0.82 {
        Inputs::BasisIEval {
            index: rng.below(n as u64 + 2) as usize,
            knots,
            degree: p,
            u,
            flag,
        }
    } else {
        Inputs::BasisIDerivs {
            index: rng.below(n as u64 + 2) as usize,
            der: rng.below(p as u64 + 3) as usize,
            knots,
            degree: p,
            u,
            flag,
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.knots"
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
        rust(&case.parse()?)
    }
}
