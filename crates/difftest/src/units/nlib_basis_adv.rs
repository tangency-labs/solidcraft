//! Runner for unit `nlib.basis_adv`: merging knot vectors with multiplicities, knot
//! vector refinement, multiplicity raising and basis derivatives at parameter arrays.
//!
//! C++: `solidcore_ffi::units::nlib_basis_adv` (upstream `source/SMLib/src/NL_BasisAdv.cpp`).
//! Rust: `solidcore::nurbs::basis_adv`.
//!
//! | op (`inputs.op`) | Rust                         | upstream                       |
//! |------------------|------------------------------|--------------------------------|
//! | `compat_mult`    | `compatible_knot_array_mult` | `N_GetCompatibleKnotArrayMult` |
//! | `insert`         | `insert_knots`               | `N_BasisInsertKnots`           |
//! | `inc_mult`       | `increase_knot_mult`         | `N_BasisIncreaseKnotMult`      |
//! | `derivs_array`   | `basis_derivs_array`         | `N_BasisDerivsArray`           |
//!
//! Errors are normalised to `nlib_<NL_*_ERR number>` on both sides. `compat_mult` inputs
//! on which upstream would read past a knot vector's end (upstream-issues UB-13) are
//! [`PAST_END`] on both sides: the C++ side from the ffi's exact walk check (the routine
//! is not called), the Rust side from its out-of-range error, so the two detections are
//! compared. Every run has at least [`MIN_GENERATED`] generated inputs (more with
//! `--seeds`).
//!
//! Known vacuous fixtures: the `inc_mult` fixtures taken from Bezier-form knot vectors
//! (`vNN_..._inc_mult_t*` whose vector has no interior knot, e.g. `DropCurveCurve.smc`)
//! succeed on both sides with an empty knot list, since upstream inserts only copies of
//! interior knots. There is no value to record, so they are left as status-only checks;
//! the generator avoids such inputs.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::basis_adv as ba;
use solidcore::nurbs::knots::{KnotVector, Side};
use solidcore::SolidError;
use solidcore_ffi::units::nlib_basis_adv as ffi;
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1500;

/// Error output of a `compat_mult` case on which upstream reads past `R[mr]` or `S[ms]`.
pub const PAST_END: &str = "read_past_end";

/// Operation of the Rust out-of-range error that [`PAST_END`] stands for.
const COMPAT_OP: &str = "nurbs::basis_adv::compatible_knot_array_mult";

/// `nlib.basis_adv` runner.
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
    CompatMult {
        r: Vec<f64>,
        s: Vec<f64>,
        p: usize,
    },
    Insert {
        r: Vec<f64>,
        p: usize,
        x: Vec<f64>,
    },
    IncMult {
        u: Vec<f64>,
        p: usize,
        t: usize,
    },
    DerivsArray {
        knots: Vec<f64>,
        degree: usize,
        u: Vec<f64>,
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

/// `nd[k][i][j]` flattened, with its shape.
fn cube(out: Output, nd: &[Vec<Vec<f64>>]) -> Output {
    let rows = nd.len() as i64;
    let cols = nd.first().map_or(0, Vec::len) as i64;
    let cnt = nd.first().and_then(|r| r.first()).map_or(0, Vec::len) as i64;
    let flat: Vec<f64> = nd.iter().flatten().flatten().copied().collect();
    out.ints("nd_shape", vec![rows, cols, cnt])
        .values("nd", flat)
}

fn cpp(i: Inputs) -> Result<Output, HarnessError> {
    let out = match i {
        Inputs::CompatMult { r, s, p } => {
            let p = i32_of(p)?;
            ffi::compat_mult_reads_in_bounds(&r, &s, p).and_then(|in_bounds| {
                if !in_bounds {
                    return Ok(Output::err(PAST_END));
                }
                ffi::compat_mult(&r, &s, p).map(|m| {
                    Output::new()
                        .values("r", m.r)
                        .values("s", m.s)
                        .values("xr", m.xr)
                        .values("xs", m.xs)
                })
            })
        }
        Inputs::Insert { r, p, x } => {
            ffi::insert(&r, i32_of(p)?, &x).map(|s| Output::new().values("s", s))
        }
        Inputs::IncMult { u, p, t } => {
            ffi::inc_mult(&u, i32_of(p)?, i64_of(t)?).map(|x| Output::new().values("x", x))
        }
        Inputs::DerivsArray {
            knots,
            degree,
            u,
            flag,
            der,
        } => ffi::derivs_array(
            &knots,
            i32_of(degree)?,
            &u,
            flag.side().flag(),
            i64_of(der)?,
        )
        .map(|d| cube(Output::new().ints("spans", d.spans), &d.nd)),
    };
    out.or_else(|e| cpp_err(&e))
}

fn rust(i: Inputs) -> Result<Output, HarnessError> {
    let out: Result<Output, SolidError> = match i {
        Inputs::CompatMult { mut r, mut s, p } => {
            match ba::compatible_knot_array_mult(&mut r, &mut s, p) {
                Ok((xr, xs)) => Ok(Output::new()
                    .values("r", r)
                    .values("s", s)
                    .values("xr", xr)
                    .values("xs", xs)),
                Err(SolidError::InvalidInput { operation, reason })
                    if operation == COMPAT_OP && reason.starts_with("index out of range") =>
                {
                    return Ok(Output::err(PAST_END));
                }
                Err(e) => Err(e),
            }
        }
        Inputs::Insert { r, p, x } => {
            ba::insert_knots(&r, p, &x).map(|s| Output::new().values("s", s))
        }
        Inputs::IncMult { u, p, t } => {
            ba::increase_knot_mult(&u, p, t).map(|x| Output::new().values("x", x))
        }
        Inputs::DerivsArray {
            knots,
            degree,
            u,
            flag,
            der,
        } => {
            let knt = KnotVector::from_slice(&knots).map_err(|e| harness(e.to_string()))?;
            match ba::basis_derivs_array(&knt, degree, &u, flag.side(), der) {
                Ok(b) => {
                    let spans = b
                        .spans
                        .iter()
                        .map(|&s| i64_of(s))
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(cube(Output::new().ints("spans", spans), &b.derivs))
                }
                Err(e) => Err(e),
            }
        }
    };
    Ok(out.unwrap_or_else(|e| rust_err(&e)))
}

// ---- generator -------------------------------------------------------------------

fn pick_degree(rng: &mut Rng) -> usize {
    let r = rng.unit();
    if r < 0.06 {
        0
    } else if r < 0.9 {
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

/// Sorted interior knots strictly inside `(a, b)` (at least `margin * (b-a)` from the
/// ends) with multiplicities up to `max_mult`, plus `shared` knots kept with
/// probability 0.6 (and multiplicity up to `max_mult`).
fn gen_interior(
    rng: &mut Rng,
    a: f64,
    b: f64,
    integral: bool,
    max_mult: usize,
    margin: f64,
    shared: &[f64],
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
    for &x in shared {
        if rng.chance(0.6) && x > lo && x < hi {
            v.push(x);
        }
    }
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

fn clamped(p: usize, a: f64, b: f64, inner: Vec<f64>) -> Vec<f64> {
    let mut k = Vec::new();
    k.extend(std::iter::repeat_n(a, p + 1));
    k.extend(inner);
    k.extend(std::iter::repeat_n(b, p + 1));
    k
}

/// A knot vector of degree `p`: mostly clamped, sometimes unclamped (ends spread out).
fn gen_knots(rng: &mut Rng, p: usize, max_mult: usize) -> Vec<f64> {
    let (a, b, integral) = gen_domain(rng);
    let inner = gen_interior(rng, a, b, integral, max_mult, 0.0, &[]);
    if rng.chance(0.8) {
        return clamped(p, a, b, inner);
    }
    let mut lo = a;
    let mut pre: Vec<f64> = (0..p)
        .map(|_| {
            lo -= rng.uniform(0.1, 2.0) * (b - a) / 4.0;
            lo
        })
        .collect();
    pre.reverse();
    let mut k = pre;
    k.push(a);
    k.extend(inner);
    k.push(b);
    let mut hi = b;
    for _ in 0..p {
        if !rng.chance(0.3) {
            hi += rng.uniform(0.1, 2.0) * (b - a) / 4.0;
        }
        k.push(hi);
    }
    k
}

/// Largest `der` for which `p!/(p-k)!`, `k = 1..=min(p,der)`, fits an `i32`.
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

fn gen_derivs_array(rng: &mut Rng) -> Inputs {
    let p = pick_degree(rng);
    let mut knots = gen_knots(rng, p, p.max(1));
    while knots.len() < 2 * p + 2 {
        let last = knots.last().copied().unwrap_or(1.0);
        knots.push(last);
    }
    let m = knots.len() - 1;
    let first = knots.first().copied().unwrap_or(0.0);
    let last = knots.last().copied().unwrap_or(1.0);
    let lo = knots.get(p).copied().unwrap_or(first);
    let hi = knots.get(m - p).copied().unwrap_or(last);
    let flag = if rng.chance(0.5) {
        Flag::Left
    } else {
        Flag::Right
    };
    let cnt = 1 + rng.below(12) as usize;
    let pick = |rng: &mut Rng| -> f64 {
        let r = rng.unit();
        if r < 0.55 {
            rng.uniform(lo, hi)
        } else if r < 0.8 {
            *rng.choose(&knots).unwrap_or(&lo)
        } else if r < 0.9 {
            let k = *rng.choose(&knots).unwrap_or(&lo);
            if rng.chance(0.5) {
                k.next_up()
            } else {
                k.next_down()
            }
        } else {
            *rng.choose(&[lo, hi, first, last]).unwrap_or(&lo)
        }
    };
    let mut u: Vec<f64> = (0..cnt).map(|_| pick(rng)).collect();
    if rng.chance(0.85) {
        u.sort_by(f64::total_cmp);
    }
    // Inner parameters must stop the span walk inside the knots (ffi domain).
    let n = u.len();
    for (j, v) in u.iter_mut().enumerate() {
        let inner = j > 0 && j + 1 < n;
        let past = match flag {
            Flag::Left => *v >= hi,
            Flag::Right => *v > hi,
        };
        if inner && past {
            *v = match flag {
                Flag::Left => hi.next_down(),
                Flag::Right => hi,
            };
        }
    }
    // The first and last parameters may also lie outside the domain.
    if rng.chance(0.1) {
        if let Some(v) = u.first_mut() {
            *v = first - rng.uniform(0.0, 2.0);
        }
    }
    if rng.chance(0.1) {
        if let Some(v) = u.last_mut() {
            *v = last + rng.uniform(0.0, 2.0);
        }
    }
    let der = (rng.below(p as u64 + 3) as usize).min(max_der(p));
    Inputs::DerivsArray {
        knots,
        degree: p,
        u,
        flag,
        der,
    }
}

fn gen_insert(rng: &mut Rng) -> Inputs {
    let p = pick_degree(rng).min(6);
    let r = gen_knots(rng, p, p.max(1));
    let m = r.len() - 1;
    let lo = r.get(p).copied().unwrap_or(0.0);
    let hi = r.get(m.saturating_sub(p)).copied().unwrap_or(1.0);
    let cnt = 1 + rng.below(8) as usize;
    let mut x: Vec<f64> = (0..cnt)
        .map(|_| {
            let c = rng.unit();
            if c < 0.6 {
                rng.uniform(lo, hi)
            } else {
                *rng.choose(&r).unwrap_or(&lo)
            }
        })
        .filter(|&v| v > lo && v < hi)
        .collect();
    if x.is_empty() || rng.chance(0.05) {
        x.push(0.5 * (lo + hi));
    }
    if rng.chance(0.9) {
        x.sort_by(f64::total_cmp);
    }
    if rng.chance(0.08) {
        // Out of range: NL_KNT_ERR.
        if rng.chance(0.5) {
            if let Some(v) = x.first_mut() {
                *v = lo - rng.uniform(0.0, 1.0);
            }
        } else if let Some(v) = x.last_mut() {
            *v = hi;
        }
    }
    Inputs::Insert { r, p, x }
}

fn gen_inc_mult(rng: &mut Rng) -> Inputs {
    // `increase_knot_mult` returns t copies of every interior knot, so a vector with
    // none (len <= 2p + 2) or t = 0 yields an empty result on both sides and compares
    // nothing (a vacuous pass). Retry a bounded number of times to reach a case that
    // produces knots; the last draw is kept so degenerate shapes stay reachable.
    let mut last = None;
    for _ in 0..8 {
        let p = pick_degree(rng).min(8);
        let u = gen_knots(rng, p, (p + 1).max(1));
        let t = rng.below(p as u64 + 2) as usize;
        let produces = t > 0 && u.len() > 2 * p + 2;
        last = Some(Inputs::IncMult { u, p, t });
        if produces {
            break;
        }
    }
    last.unwrap_or(Inputs::IncMult {
        u: vec![0.0],
        p: 0,
        t: 0,
    })
}

fn gen_compat_mult(rng: &mut Rng) -> Inputs {
    let p = 1 + rng.below(6) as usize;
    let (a, b, integral) = gen_domain(rng);
    let shared = gen_interior(rng, a, b, integral, 1, 1e-3, &[]);
    let r = clamped(p, a, b, gen_interior(rng, a, b, integral, p, 1e-3, &shared));
    let mode = rng.unit();
    let (sa, sb) = if mode < 0.5 {
        (a, b)
    } else if mode < 0.6 {
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
    let mapped: Vec<f64> = shared
        .iter()
        .map(|&x| {
            let y = sa + (x - a) / (b - a) * (sb - sa);
            if rng.chance(0.15) {
                y + (sb - sa) * 1e-13 * rng.uniform(-1.0, 1.0)
            } else {
                y
            }
        })
        .collect();
    let s = clamped(
        p,
        sa,
        sb,
        gen_interior(
            rng,
            sa,
            sb,
            integral && (sa, sb) == (a, b),
            p,
            1e-3,
            &mapped,
        ),
    );
    // Degree 0 on vectors clamped for degree p: upstream mostly reads past the ends
    // (UB-13, PAST_END on both sides), sometimes stays in bounds.
    let p = if rng.chance(0.04) { 0 } else { p };
    if rng.chance(0.5) {
        Inputs::CompatMult { r, s, p }
    } else {
        Inputs::CompatMult { r: s, s: r, p }
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let r = rng.unit();
    if r < 0.25 {
        gen_compat_mult(rng)
    } else if r < 0.45 {
        gen_insert(rng)
    } else if r < 0.6 {
        gen_inc_mult(rng)
    } else {
        gen_derivs_array(rng)
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.basis_adv"
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
