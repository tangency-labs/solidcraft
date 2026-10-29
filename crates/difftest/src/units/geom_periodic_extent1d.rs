//! Runner for unit `geom.periodic_extent1d`: intervals on a periodic domain.
//!
//! C++: `solidcore_ffi::units::geom_periodic_extent1d` (upstream `SmPeriodicExtent1d`,
//! `source/SMLib/inc/SmPeriodicExtent1d.h`, `source/SMLib/src/SmPeriodicExtent1d.cpp`).
//! Rust: `solidcore::geom::periodic_extent1d::PeriodicExtent1d`.
//!
//! Every case is one operation `op` on extents `a`, `b` (raw members
//! `[min, max, period, full]`), scalars `s`, a boolean `flag` and `has_period`
//! (`SetMinMax`'s optional period). Every case reports `state` (the members of `a`, or
//! of the constructed extent, afterwards); scalar operations add `r` (booleans as
//! `1.0`/`0.0`); `add_interval` / `set_min_max` add the `SmStatus` as integer `status`
//! (`SM_SUCCESS` = 1000, `SM_ERR` = 1001, `SM_ERR_INVALID_INPUT` = 1007).
//! Every run uses at least [`MIN_GENERATED`] generated inputs (more with `--seeds`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::periodic_extent1d::PeriodicExtent1d;
use solidcore::math::{BIG_DOUBLE, EFF_ZERO, PI};
use solidcore::SolidError;
use solidcore_ffi::units::geom_periodic_extent1d::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::flag;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 4000;

/// Upstream `SM_SUCCESS`.
const SM_SUCCESS: i64 = 1000;
/// Upstream `SM_ERR_INVALID_INPUT`.
const SM_ERR_INVALID_INPUT: i64 = 1007;

/// `geom.periodic_extent1d` runner.
pub struct Runner;

/// Operation names used in `inputs.op`.
const OPS: [(&str, Op); 16] = [
    ("new_full", Op::NewFull),
    ("copy", Op::Copy),
    ("get_period", Op::GetPeriod),
    ("set_full_period", Op::SetFullPeriod),
    ("is_empty", Op::IsEmpty),
    ("is_full_period", Op::IsFullPeriod),
    ("crosses_seam", Op::CrossesSeam),
    ("map_value", Op::MapValue),
    ("normalize_value", Op::NormalizeValue),
    ("compliment", Op::Compliment),
    ("add_interval", Op::AddInterval),
    ("contains_value", Op::ContainsValue),
    ("set_min_max", Op::SetMinMax),
    ("is_contained_by", Op::IsContainedBy),
    ("are_disjoint", Op::AreDisjoint),
    ("get_mid", Op::GetMid),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    op: String,
    #[serde(default)]
    a: [f64; 4],
    #[serde(default)]
    b: [f64; 4],
    #[serde(default)]
    s: [f64; 3],
    #[serde(default)]
    flag: bool,
    #[serde(default)]
    has_period: bool,
}

fn lookup(name: &str) -> Result<Op, HarnessError> {
    OPS.iter()
        .find(|(n, _)| *n == name)
        .map(|(_, op)| *op)
        .ok_or_else(|| HarnessError::new(format!("unknown op {name}")))
}

/// Which keys an operation reports besides `state`.
fn has_r(op: Op) -> bool {
    !matches!(
        op,
        Op::NewFull | Op::Copy | Op::SetFullPeriod | Op::Compliment | Op::AddInterval | Op::SetMinMax
    )
}

fn has_status(op: Op) -> bool {
    matches!(op, Op::AddInterval | Op::SetMinMax)
}

fn output(op: Op, state: [f64; 4], r: f64, status: i64) -> Output {
    let mut o = Output::new().values("state", state.to_vec());
    if has_r(op) {
        o = o.scalar("r", r);
    }
    if has_status(op) {
        o = o.int("status", status);
    }
    o
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let op = lookup(&i.op)?;
    let res = ffi::op(op, i.a, i.b, i.s, i.flag, i.has_period)
        .map_err(|e| HarnessError::new(e.to_string()))?;
    Ok(output(op, res.state, res.r, i64::from(res.status)))
}

fn extent(v: [f64; 4]) -> Result<PeriodicExtent1d, HarnessError> {
    if v[3] != 0.0 && v[3] != 1.0 {
        return Err(HarnessError::new(format!("full flag {}", v[3])));
    }
    Ok(PeriodicExtent1d::from_parts(v[0], v[1], v[2], v[3] != 0.0))
}

fn members(e: &PeriodicExtent1d) -> [f64; 4] {
    [e.min(), e.max(), e.period(), flag(e.is_full_period())]
}

fn status_of(r: &solidcore::Result<()>) -> Result<i64, HarnessError> {
    match r {
        Ok(()) => Ok(SM_SUCCESS),
        Err(SolidError::Upstream { code, .. }) => Ok(i64::from(*code)),
        Err(SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. }) => Ok(SM_ERR_INVALID_INPUT),
        Err(e) => Err(HarnessError::new(format!("rust_error: {e}"))),
    }
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let op = lookup(&i.op)?;
    let mut a = extent(i.a)?;
    let b = extent(i.b)?;
    let [s0, s1, s2] = i.s;
    let mut r = 0.0;
    let mut status = 0;
    match op {
        Op::NewFull => a = PeriodicExtent1d::full(s0),
        Op::Copy => {} // `a` is `Copy`: the copy is the extent itself
        Op::GetPeriod => r = a.period(),
        Op::SetFullPeriod => a.set_full_period(i.flag),
        Op::IsEmpty => r = flag(a.is_empty()),
        Op::IsFullPeriod => r = flag(a.is_full_period()),
        Op::CrossesSeam => r = flag(a.crosses_seam()),
        Op::MapValue => r = a.map_value(s0),
        Op::NormalizeValue => r = a.normalize_value(s0),
        Op::Compliment => a.complement(),
        Op::AddInterval => status = status_of(&a.add_interval(s0, s1, i.flag))?,
        Op::ContainsValue => r = flag(a.contains_value(s0)),
        Op::SetMinMax => {
            let p = i.has_period.then_some(s2);
            status = status_of(&a.set_min_max(s0, s1, p, i.flag))?;
        }
        Op::IsContainedBy => r = flag(a.is_contained_by(&b)),
        Op::AreDisjoint => r = flag(a.are_disjoint(&b)),
        Op::GetMid => r = a.mid(),
    }
    Ok(output(op, members(&a), r, status))
}

// ---- generation ---------------------------------------------------------------

fn period(rng: &mut Rng) -> f64 {
    match rng.below(10) {
        0..=2 => 1.0,
        3 | 4 => 2.0 * PI,
        5 => 360.0,
        6 => rng.uniform(1e-3, 1e3),
        7 => 10f64.powi(rng.range_i64(-12, 8) as i32) * rng.uniform(1.0, 9.0),
        8 => 0.0,
        _ => f64::from(rng.range_i64(1, 20) as i32),
    }
}

/// A value of interest in `[0, p]` (sometimes just outside): seam, interior, near
/// the seam to within a few scaled zeros, simple fractions.
fn value(rng: &mut Rng, p: f64) -> f64 {
    let sz = EFF_ZERO * (1.0 + p.abs());
    let jitter = |rng: &mut Rng| rng.uniform(-3.0, 3.0) * sz;
    match rng.below(12) {
        0 => 0.0,
        1 => p,
        2 => jitter(rng).abs(),
        3 => p - jitter(rng).abs(),
        4 => f64::from(rng.range_i64(0, 8) as i32) * p / 8.0,
        5 => rng.uniform(-0.3, 1.3) * p,
        6 => -jitter(rng).abs(),
        7 => p + jitter(rng).abs(),
        _ => rng.uniform(0.0, p),
    }
}

/// Value near `x`: equal, within a few scaled zeros, or unrelated.
fn near(rng: &mut Rng, x: f64, p: f64) -> f64 {
    let sz = EFF_ZERO * (1.0 + p.abs());
    match rng.below(4) {
        0 => x,
        1 => x + rng.uniform(-3.0, 3.0) * sz,
        2 => x + f64::from(rng.range_i64(-2, 2) as i32) * sz,
        _ => value(rng, p),
    }
}

fn gen_extent(rng: &mut Rng, p: f64) -> [f64; 4] {
    match rng.below(14) {
        0 => [BIG_DOUBLE, -BIG_DOUBLE, p, 0.0],
        1 => [0.0, p, p, 1.0],
        2 => [0.0, p, p, 0.0],
        3 => {
            let x = value(rng, p);
            [x, near(rng, x, p), p, 0.0]
        }
        4 => {
            // stale full flag on an ordinary interval
            let (x, y) = (value(rng, p), value(rng, p));
            [x, y, p, 1.0]
        }
        _ => {
            let (x, y) = (rng.uniform(0.0, p), rng.uniform(0.0, p));
            let (x, y) = if rng.chance(0.15) {
                (value(rng, p), value(rng, p))
            } else {
                (x, y)
            };
            [x, y, p, 0.0]
        }
    }
}

/// `b` related to `a`: same, sharing an end, overlapping, complement or independent.
fn partner(rng: &mut Rng, a: [f64; 4]) -> [f64; 4] {
    let p = if rng.chance(0.95) { a[2] } else { period(rng) };
    match rng.below(8) {
        0 => a,
        1 => [a[1], near(rng, a[1], p), p, 0.0],
        2 => [near(rng, a[1], p), value(rng, p), p, 0.0],
        3 => [value(rng, p), near(rng, a[0], p), p, 0.0],
        4 => [a[1], a[0], p, 0.0],
        _ => gen_extent(rng, p),
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let idx_op = rng.below(OPS.len() as u64) as usize;
    let (name, op) = OPS.get(idx_op).copied().unwrap_or(("get_mid", Op::GetMid));
    let p = period(rng);
    let a = gen_extent(rng, p);
    let b = partner(rng, a);
    let s = match op {
        Op::NewFull => [period(rng), 0.0, 0.0],
        Op::MapValue | Op::NormalizeValue => {
            let x = match rng.below(4) {
                0 => rng.uniform(-1.0, 2.0) * p,
                1 => value(rng, p) - p,
                2 => value(rng, p) + p,
                _ => value(rng, p),
            };
            [x, 0.0, 0.0]
        }
        Op::ContainsValue => {
            let x = match rng.below(3) {
                0 => near(rng, a[0], p),
                1 => near(rng, a[1], p),
                _ => value(rng, p),
            };
            [x, 0.0, 0.0]
        }
        Op::AddInterval => {
            let pick = |rng: &mut Rng| match rng.below(4) {
                0 => near(rng, a[0], p),
                1 => near(rng, a[1], p),
                _ => value(rng, p),
            };
            let f = pick(rng);
            let t = if rng.chance(0.1) { f } else { pick(rng) };
            [f, t, 0.0]
        }
        Op::SetMinMax => {
            let q = if rng.chance(0.5) { period(rng) } else { p };
            let x = value(rng, q);
            let y = if rng.chance(0.25) { near(rng, x, q) } else { value(rng, q) };
            [x, y, q]
        }
        _ => [value(rng, p), value(rng, p), 0.0],
    };
    Inputs {
        op: name.to_string(),
        a,
        b,
        s,
        flag: rng.chance(0.5),
        has_period: rng.chance(0.5),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "geom.periodic_extent1d"
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
