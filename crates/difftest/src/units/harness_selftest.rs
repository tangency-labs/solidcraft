//! Runner for unit `harness.selftest`: proves the difftest pipeline end to end.
//!
//! C++ side: upstream `N_BasisEval` / `N_BasisFindSpan`
//! (`source/SMLib/src/NL_BasisBasic.cpp`) via `solidcore_ffi::units::harness_selftest`.
//! Rust side: a faithful re-implementation kept *here* (the real port of the
//! basis routines belongs to its own unit in `crates/solidcore`).
//!
//! | Rust          | upstream          |
//! |---------------|-------------------|
//! | `find_span`   | `N_BasisFindSpan` |
//! | `basis_eval`  | `N_BasisEval`     |

// Local scratch arrays are sized p+1 / p+2 and indexed by loop variables
// bounded by p; knot reads go through the bounds-checked `at` closure.
#![allow(clippy::indexing_slicing)]

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore_ffi::units::harness_selftest as ffi;
use solidcore_ffi::FfiError;

use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// `harness.selftest` runner.
pub struct Runner;

/// `NL_LEFT` / `NL_RIGHT`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Side {
    Left,
    Right,
}

impl Side {
    fn code(self) -> i32 {
        match self {
            Side::Left => 1,
            Side::Right => 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    /// Trivial shim round-trip `2x + 1`.
    Ping { x: f64 },
    /// `N_BasisEval(knots, degree, u, flag)`.
    BasisEval {
        knots: Vec<f64>,
        degree: i32,
        u: f64,
        flag: Side,
    },
}

/// Normalised error string for an upstream error flag.
const NL_ERROR: &str = "nl_error";

fn ffi_err(e: &FfiError) -> Result<Output, HarnessError> {
    match e {
        FfiError::Status(1) => Ok(Output::err(NL_ERROR)),
        FfiError::Status(_) => Ok(Output::err(e.to_string())),
        FfiError::InvalidArgument(m) => Err(HarnessError::new(format!("ffi rejected: {m}"))),
    }
}

fn check_basis_inputs(knots: &[f64], degree: i32) -> Result<usize, HarnessError> {
    let p = usize::try_from(degree).map_err(|_| HarnessError::new("negative degree"))?;
    if p > ffi::MAX_DEGREE as usize || knots.len() < 2 * p + 2 {
        return Err(HarnessError::new(format!(
            "degree {degree} with {} knots",
            knots.len()
        )));
    }
    Ok(p)
}

// ---- Rust re-implementation (mirrors upstream statement order) --------------

/// `N_BasisFindSpan`. `uk` has length m+1 >= 2p+2 (checked by caller).
fn find_span(uk: &[f64], p: usize, u: f64, flag: Side) -> usize {
    let knot_tol = 1.0e-12;
    let m = uk.len() - 1;
    let at = |i: usize| uk.get(i).copied().unwrap_or(f64::NAN);

    let mut low = p;
    let mut high = m - p;
    let mut mid = (low + high) / 2;
    let mut lastmid = mid;

    if u >= at(m - p) {
        let mut i = m - p - 1;
        while at(i + 1) < at(i) + knot_tol && i > p {
            i -= 1;
        }
        return i;
    }
    if u <= at(p) {
        let mut i = p;
        while at(i + 1) < at(i) + knot_tol && i < m - p - 1 {
            i += 1;
        }
        return i;
    }
    match flag {
        Side::Left => {
            while u < at(mid) || u >= at(mid + 1) {
                if u < at(mid) {
                    high = mid;
                } else {
                    low = mid;
                }
                mid = (low + high) / 2;
                if mid == lastmid {
                    break;
                }
                lastmid = mid;
            }
        }
        Side::Right => {
            while u <= at(mid) || u > at(mid + 1) {
                if u > at(mid) {
                    low = mid;
                } else {
                    high = mid;
                }
                mid = (low + high) / 2;
                if mid == lastmid {
                    break;
                }
                lastmid = mid;
            }
        }
    }
    mid
}

/// `N_BasisEval`: `Ok((span, N[0..=p]))`, `Err(span)` on `NL_NUM_ERR`.
fn basis_eval(uk: &[f64], p: usize, u: f64, flag: Side) -> Result<(usize, Vec<f64>), usize> {
    let zero_tol = 1e-24;
    let m = uk.len() - 1;
    let at = |i: usize| uk.get(i).copied().unwrap_or(f64::NAN);
    let mut n = vec![0.0; p + 1];

    if u == at(p) {
        n[0] = 1.0;
        return Ok((p, n));
    }
    if u == at(m - p) {
        n[p] = 1.0;
        return Ok((m - p - 1, n));
    }

    let i = find_span(uk, p, u, flag);
    let mut left = vec![0.0; p + 2];
    let mut right = vec![0.0; p + 2];
    n[0] = 1.0;
    for k in 1..=p {
        left[k] = u - at(i + 1 - k);
        right[k] = at(i + k) - u;
        let mut saved = 0.0;
        for l in 0..k {
            let delta = right[l + 1] + left[k - l];
            if delta < zero_tol {
                return Err(i);
            }
            let temp = n[l];
            n[l] = saved + temp * (right[l + 1] / delta);
            saved = temp * (left[k - l] / delta);
        }
        n[k] = saved;
    }
    Ok((i, n))
}

// ---- generator ----------------------------------------------------------------

fn gen_basis(rng: &mut Rng) -> Inputs {
    let p = rng.range_i64(0, 5) as usize;
    let n_ctrl = p + 1 + rng.below(10) as usize;
    let n_interior = n_ctrl - p - 1;
    let a = rng.uniform(-10.0, 10.0);
    let b = a + rng.uniform(0.1, 20.0);
    let max_mult = p.max(1);

    let mut interior: Vec<f64> = (0..n_interior).map(|_| rng.uniform(a, b)).collect();
    interior.sort_by(f64::total_cmp);
    let mut mult = 1;
    for i in 1..interior.len() {
        if mult < max_mult && rng.chance(0.2) {
            interior[i] = interior[i - 1];
            mult += 1;
        } else {
            mult = 1;
        }
    }
    let mut knots = vec![a; p + 1];
    knots.extend(interior);
    knots.extend(std::iter::repeat_n(b, p + 1));

    let r = rng.unit();
    let u = if r < 0.5 {
        rng.uniform(a, b)
    } else if r < 0.7 {
        *rng.choose(&knots).unwrap_or(&a)
    } else if r < 0.8 {
        if rng.chance(0.5) {
            a
        } else {
            b
        }
    } else if r < 0.9 {
        if rng.chance(0.5) {
            a - rng.uniform(0.0, 1.0)
        } else {
            b + rng.uniform(0.0, 1.0)
        }
    } else {
        let j = rng.below(knots.len() as u64 - 1) as usize;
        0.5 * (knots[j] + knots[j + 1])
    };
    let flag = if rng.chance(0.5) {
        Side::Left
    } else {
        Side::Right
    };
    Inputs::BasisEval {
        knots,
        degree: p as i32,
        u,
        flag,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "harness.selftest"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_basis(rng)).ok()
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        match case.parse::<Inputs>()? {
            Inputs::Ping { x } => match ffi::ping(x) {
                Ok(y) => Ok(Output::new().scalar("y", y)),
                Err(e) => ffi_err(&e),
            },
            Inputs::BasisEval {
                knots,
                degree,
                u,
                flag,
            } => {
                check_basis_inputs(&knots, degree)?;
                match ffi::basis_eval(&knots, degree, u, flag.code()) {
                    Ok(r) => Ok(Output::new().int("span", r.span).values("basis", r.basis)),
                    Err(e) => ffi_err(&e),
                }
            }
        }
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        match case.parse::<Inputs>()? {
            Inputs::Ping { x } => Ok(Output::new().scalar("y", x * 2.0 + 1.0)),
            Inputs::BasisEval {
                knots,
                degree,
                u,
                flag,
            } => {
                let p = check_basis_inputs(&knots, degree)?;
                match basis_eval(&knots, p, u, flag) {
                    Ok((span, n)) => Ok(Output::new().int("span", span as i64).values("basis", n)),
                    Err(_) => Ok(Output::err(NL_ERROR)),
                }
            }
        }
    }
}
