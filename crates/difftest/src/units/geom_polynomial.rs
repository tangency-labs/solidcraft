//! Runner for unit `geom.polynomial`: closed-form real roots of polynomials.
//!
//! C++: `solidcore_ffi::units::geom_polynomial` (upstream `SmPolynomial::Solve*Eqn`,
//! `source/SMLib/src/SmPolynomial.cpp`).
//! Rust: `solidcore::geom::polynomial::{solve_quadratic, solve_cubic, solve_quartic}`.
//!
//! A case is `c` (3, 4 or 5 coefficients, ascending powers, selecting the quadratic,
//! cubic or quartic solver) and the zero tolerance `tol`. Every case reports the
//! root count `n`, the roots `roots` (upstream order) and the `SmStatus` as `status`
//! (`SM_SUCCESS` = 1000). Every run uses at least [`MIN_GENERATED`] generated inputs.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::polynomial::{solve_cubic, solve_quadratic, solve_quartic, Roots};
use solidcore_ffi::units::geom_polynomial as ffi;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 6000;

/// Upstream `SM_SUCCESS` (every solver path returns it).
const SM_SUCCESS: i64 = 1000;

/// `geom.polynomial` runner.
pub struct Runner;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    c: Vec<f64>,
    tol: f64,
}

fn output(n: usize, roots: Vec<f64>, status: i64) -> Output {
    Output::new()
        .int("n", n as i64)
        .values("roots", roots)
        .int("status", status)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let s = ffi::solve(&i.c, i.tol).map_err(|e| HarnessError::new(e.to_string()))?;
    Ok(output(s.roots.len(), s.roots, i64::from(s.status)))
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let roots: Roots = match *i.c.as_slice() {
        [a, b, c] => solve_quadratic(&[a, b, c], i.tol),
        [a, b, c, d] => solve_cubic(&[a, b, c, d], i.tol),
        [a, b, c, d, e] => solve_quartic(&[a, b, c, d, e], i.tol),
        _ => return Err(HarnessError::new(format!("{} coefficients", i.c.len()))),
    };
    Ok(output(roots.len(), roots.to_vec(), SM_SUCCESS))
}

// ---- generation ---------------------------------------------------------------

fn tolerance(rng: &mut Rng, degree: usize) -> f64 {
    match rng.below(12) {
        0..=4 => 1.0e-12,
        5 => 1.0e-8,
        6 => 1.0e-10,
        7 => 1.0e-6,
        8 => 0.0,
        9 => 10f64.powi(rng.range_i64(-14, -3) as i32),
        10 => 1.0e-3,
        // Large tolerances (quartics stay <= 1: above that upstream is UB).
        _ if degree < 4 => rng.uniform(0.0, 2.0),
        _ => rng.uniform(0.0, 1.0),
    }
}

/// A root of interest: integer, simple fraction, or arbitrary.
fn root(rng: &mut Rng) -> f64 {
    match rng.below(6) {
        0 | 1 => f64::from(rng.range_i64(-5, 5) as i32),
        2 => f64::from(rng.range_i64(-12, 12) as i32) / f64::from(rng.range_i64(1, 8) as i32),
        3 => 0.0,
        _ => rng.uniform(-10.0, 10.0),
    }
}

/// Multiplies `poly` (ascending powers) by `(x - r)`.
fn times_linear(poly: &[f64], r: f64) -> Vec<f64> {
    let mut out = vec![0.0; poly.len() + 1];
    for (k, &a) in poly.iter().enumerate() {
        if let Some(o) = out.get_mut(k + 1) {
            *o += a;
        }
        if let Some(o) = out.get_mut(k) {
            *o -= r * a;
        }
    }
    out
}

/// Multiplies `poly` by `(x^2 + b x + c)`.
fn times_quadratic(poly: &[f64], b: f64, c: f64) -> Vec<f64> {
    let mut out = vec![0.0; poly.len() + 2];
    for (k, &a) in poly.iter().enumerate() {
        if let Some(o) = out.get_mut(k + 2) {
            *o += a;
        }
        if let Some(o) = out.get_mut(k + 1) {
            *o += b * a;
        }
        if let Some(o) = out.get_mut(k) {
            *o += c * a;
        }
    }
    out
}

/// Coefficients of a degree-`degree` polynomial built from chosen real roots
/// (with repeats) and irreducible quadratic factors.
fn from_roots(rng: &mut Rng, degree: usize) -> Vec<f64> {
    let mut poly = vec![if rng.chance(0.5) {
        1.0
    } else {
        rng.uniform(-5.0, 5.0)
    }];
    let mut roots: Vec<f64> = Vec::new();
    while poly.len() < degree + 1 {
        let left = degree + 1 - poly.len();
        if left >= 2 && rng.chance(0.25) {
            // (x - m)^2 + s^2 with s maybe tiny (near-double root)
            let m = root(rng);
            let s = if rng.chance(0.3) {
                10f64.powi(rng.range_i64(-9, -3) as i32)
            } else {
                rng.uniform(0.1, 3.0)
            };
            poly = times_quadratic(&poly, -2.0 * m, m * m + s * s);
        } else {
            let r = match roots.last() {
                Some(&prev) if rng.chance(0.4) => {
                    if rng.chance(0.5) {
                        prev
                    } else {
                        prev + rng.uniform(-1e-6, 1e-6)
                    }
                }
                _ => root(rng),
            };
            roots.push(r);
            poly = times_linear(&poly, r);
        }
    }
    poly
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let degree = 2 + rng.below(3) as usize;
    let tol = tolerance(rng, degree);
    let mut c: Vec<f64> = match rng.below(4) {
        0 => (0..=degree).map(|_| rng.uniform(-10.0, 10.0)).collect(),
        1 => (0..=degree)
            .map(|_| f64::from(rng.range_i64(-6, 6) as i32))
            .collect(),
        _ => from_roots(rng, degree),
    };
    // Degenerate tweaks: zero / tiny / near-tolerance coefficients.
    for x in &mut c {
        match rng.below(14) {
            0 => *x = 0.0,
            1 => *x = rng.uniform(-1.0, 1.0) * tol,
            2 => *x = rng.uniform(-3.0, 3.0) * tol.max(1e-15),
            _ => {}
        }
    }
    // Scale.
    if rng.chance(0.2) {
        let s = 10f64.powi(rng.range_i64(-6, 6) as i32) * rng.uniform(0.5, 2.0);
        for x in &mut c {
            *x *= s;
        }
    }
    Inputs { c, tol }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "geom.polynomial"
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
