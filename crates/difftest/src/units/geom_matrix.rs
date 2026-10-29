//! Runner for unit `geom.matrix`: dense matrices, LU solves, Jacobi eigen solver.
//!
//! C++: `solidcore_ffi::units::geom_matrix` (upstream `SmMatrix`,
//! `source/SMLib/inc/SmMatrix.h`, `source/SMLib/src/SmMatrix.cpp`).
//! Rust: `solidcore::geom::matrix::Matrix`.
//!
//! Every case is one operation `op` on the row-major `rows x cols` matrix `m` with
//! right-hand side `v` and operands `i`, `j`, `s`. Matrix entries may be the strings
//! `"NaN"`, `"inf"`, `"-inf"` (JSON has no non-finite numbers). Outputs: `values`
//! (floats), `ints`, `warning` (`SM_ERR_WARNING` from a near-zero pivot); failures
//! are normalised to `status_<SmStatus>` on both sides (`SM_ERR_INVALID_INPUT` =
//! 1007, `SM_ERR` = 1001). Every run uses at least [`MIN_GENERATED`] generated
//! inputs (more with `--seeds`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::matrix::Matrix;
use solidcore::SolidError;
use solidcore_ffi::units::geom_matrix::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::json::num;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 4000;

/// Upstream `SM_ERR_WARNING`.
const SM_ERR_WARNING: i32 = 1023;

/// `geom.matrix` runner.
pub struct Runner;

/// Operation names used in `inputs.op`.
const OPS: [(&str, Op); 11] = [
    ("copy", Op::Copy),
    ("resize", Op::Resize),
    ("access", Op::Access),
    ("is_symmetric", Op::IsSymmetric),
    ("clear", Op::Clear),
    ("identity", Op::Identity),
    ("lu", Op::Lu),
    ("solve_lu", Op::SolveLu),
    ("solve", Op::Solve),
    ("eigen", Op::Eigen),
    ("eigen3", Op::Eigen3),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    op: String,
    rows: usize,
    cols: usize,
    m: Vec<Value>,
    #[serde(default)]
    v: Vec<Value>,
    #[serde(default)]
    i: i64,
    #[serde(default)]
    j: i64,
    #[serde(default)]
    s: f64,
}

fn lookup(name: &str) -> Result<Op, HarnessError> {
    OPS.iter()
        .find(|(n, _)| *n == name)
        .map(|(_, op)| *op)
        .ok_or_else(|| HarnessError::new(format!("unknown op {name}")))
}

/// A JSON number or `"NaN"` / `"inf"` / `"-inf"`.
fn float(v: &Value) -> Result<f64, HarnessError> {
    match v {
        Value::Number(n) => n
            .as_f64()
            .ok_or_else(|| HarnessError::new(format!("bad number {n}"))),
        Value::String(s) if s == "NaN" => Ok(f64::NAN),
        Value::String(s) if s == "inf" => Ok(f64::INFINITY),
        Value::String(s) if s == "-inf" => Ok(f64::NEG_INFINITY),
        other => Err(HarnessError::new(format!("bad float {other}"))),
    }
}

fn floats(v: &[Value]) -> Result<Vec<f64>, HarnessError> {
    v.iter().map(float).collect()
}

fn normalise(e: &SolidError) -> String {
    match e {
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => "status_1007".into(),
        SolidError::Upstream { code, .. } => format!("status_{code}"),
        other => format!("rust_error: {other}"),
    }
}

fn result(values: Vec<f64>, ints: Vec<i64>, warning: Option<bool>) -> Output {
    let o = Output::new().values("values", values).ints("ints", ints);
    match warning {
        Some(w) => o.int("warning", i64::from(w)),
        None => o,
    }
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let op = lookup(&i.op)?;
    let m = floats(&i.m)?;
    let v = floats(&i.v)?;
    let r = match ffi::op(op, i.rows, i.cols, &m, &v, i.i, i.j, i.s) {
        Ok(r) => r,
        Err(e @ (FfiError::InvalidArgument(_) | FfiError::Status(_))) => {
            return Err(HarnessError::new(e.to_string()))
        }
    };
    let warn_ops = matches!(op, Op::Lu | Op::SolveLu | Op::Solve);
    if r.status != 0 && !(op == Op::Lu && r.status == SM_ERR_WARNING) {
        return Ok(Output::err(format!("status_{}", r.status)));
    }
    let (ints, warning) = match op {
        Op::Lu => (r.ints, Some(r.status == SM_ERR_WARNING)),
        Op::SolveLu | Op::Solve => (Vec::new(), r.ints.first().map(|&w| w != 0)),
        _ => (r.ints, None),
    };
    if warn_ops && warning.is_none() {
        return Err(HarnessError::new("shim returned no warning flag"));
    }
    Ok(result(r.values, ints, warning))
}

fn idx(x: i64) -> usize {
    usize::try_from(x).unwrap_or(usize::MAX)
}

fn ints_of(v: &[usize]) -> Vec<i64> {
    v.iter().map(|&x| x as i64).collect()
}

fn rust_run(op: Op, i: &Inputs) -> Result<solidcore::Result<Output>, HarnessError> {
    let m = floats(&i.m)?;
    let v = floats(&i.v)?;
    let mut a =
        Matrix::from_row_major(i.rows, i.cols, m).map_err(|e| HarnessError::new(e.to_string()))?;
    let dims = |a: &Matrix| vec![a.num_rows() as i64, a.num_columns() as i64];
    Ok((|| -> solidcore::Result<Output> {
        Ok(match op {
            Op::Copy => {
                let c = a.clone();
                result(c.as_slice().to_vec(), dims(&c), None)
            }
            Op::Resize => {
                let (r, c) = (idx(i.i), idx(i.j));
                let same = r == a.num_rows() && c == a.num_columns();
                a.set_size(r, c)?;
                if !same && a.make_identity().is_err() {
                    a.clear();
                }
                result(a.as_slice().to_vec(), dims(&a), None)
            }
            Op::Access => {
                let (r, c) = (idx(i.i), idx(i.j));
                a.add_to(r, c, i.s)?;
                let mut out = vec![a.at(r, c)?];
                out.push(
                    a.row(r)
                        .and_then(|row| row.get(c).copied())
                        .unwrap_or(f64::NAN),
                );
                a.set_at(r, c, -i.s)?;
                let after = a
                    .row_mut(r)
                    .and_then(|row| row.get_mut(c).map(|x| *x))
                    .unwrap_or(f64::NAN);
                out.push(after);
                out.extend_from_slice(a.as_slice());
                result(out, Vec::new(), None)
            }
            Op::IsSymmetric => result(Vec::new(), vec![i64::from(a.is_symmetric())], None),
            Op::Clear => {
                a.clear();
                result(a.as_slice().to_vec(), Vec::new(), None)
            }
            Op::Identity => {
                a.make_identity()?;
                result(a.as_slice().to_vec(), Vec::new(), None)
            }
            Op::Lu => {
                let lu = a.lu_decompose()?;
                result(
                    a.as_slice().to_vec(),
                    ints_of(&lu.index),
                    Some(lu.near_singular),
                )
            }
            Op::SolveLu => {
                let lu = a.lu_decompose()?;
                let x = a.solve_with_lu(&lu.index, &v)?;
                result(x, Vec::new(), Some(lu.near_singular))
            }
            Op::Solve => {
                let s = a.solve_linear_system(&v)?;
                result(s.x, Vec::new(), Some(s.near_singular))
            }
            Op::Eigen => {
                let e = a.eigen_system_jacobi()?;
                let mut out = e.values;
                out.extend_from_slice(e.vectors.as_slice());
                out.extend_from_slice(a.as_slice());
                result(out, vec![e.rotations as i64], None)
            }
            Op::Eigen3 => {
                let (vals, vecs) = a.compute_3x3_eigen_vectors()?;
                let mut out = vals.to_vec();
                for p in vecs {
                    out.extend_from_slice(&p.to_array());
                }
                out.extend_from_slice(a.as_slice());
                result(out, Vec::new(), None)
            }
        })
    })())
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let op = lookup(&i.op)?;
    match rust_run(op, i)? {
        Ok(o) => Ok(o),
        Err(e) => Ok(Output::unported(&e).unwrap_or_else(|| Output::err(normalise(&e)))),
    }
}

// ---- generation ---------------------------------------------------------------

/// An entry: mostly ordinary values, sometimes zero, small integers, tiny, huge.
fn entry(rng: &mut Rng) -> f64 {
    let r = rng.unit();
    if r < 0.60 {
        rng.uniform(-10.0, 10.0)
    } else if r < 0.70 {
        0.0
    } else if r < 0.82 {
        rng.range_i64(-3, 3) as f64
    } else if r < 0.90 {
        rng.uniform(-1.0, 1.0) * 10f64.powi(-(rng.range_i64(6, 14) as i32))
    } else if r < 0.97 {
        rng.uniform(-1.0, 1.0) * 10f64.powi(rng.range_i64(3, 12) as i32)
    } else {
        rng.uniform(-1.0, 1.0)
    }
}

fn dim(rng: &mut Rng) -> usize {
    match rng.below(10) {
        0 => 1,
        1..=7 => 2 + rng.below(4) as usize,
        8 => 6 + rng.below(3) as usize, // around the upstream 6x6 inline buffer
        _ => 9 + rng.below(8) as usize,
    }
}

/// A square `n x n` matrix of one of several shapes (general, symmetric, singular,
/// triangular, permutation, ill-conditioned, scaled, non-finite).
fn square(rng: &mut Rng, n: usize, symmetric: bool) -> Vec<f64> {
    let mut m: Vec<f64> = (0..n * n).map(|_| entry(rng)).collect();
    let set = |m: &mut Vec<f64>, r: usize, c: usize, x: f64| {
        if let Some(p) = m.get_mut(r * n + c) {
            *p = x;
        }
    };
    let get = |m: &Vec<f64>, r: usize, c: usize| m.get(r * n + c).copied().unwrap_or(0.0);
    match rng.below(12) {
        0 => {
            // duplicate or scaled row -> singular
            if n > 1 {
                let (a, b) = (rng.below(n as u64) as usize, rng.below(n as u64) as usize);
                let k = if rng.chance(0.5) {
                    1.0
                } else {
                    rng.uniform(-3.0, 3.0)
                };
                for c in 0..n {
                    let x = get(&m, a, c) * k;
                    set(&mut m, b, c, x);
                }
            }
        }
        1 => {
            // zero or tiny row (the SM_ERR / SM_EFF_ZERO thresholds)
            let r = rng.below(n as u64) as usize;
            let scale = match rng.below(4) {
                0 => 0.0,
                1 => 1e-12,
                2 => 1e-12 * rng.uniform(0.5, 1.5),
                _ => 1e-13,
            };
            for c in 0..n {
                let x = scale * rng.uniform(-1.0, 1.0);
                set(&mut m, r, c, x);
            }
            if scale > 0.0 && rng.chance(0.5) {
                set(&mut m, r, rng.below(n as u64) as usize, scale);
            }
        }
        2 => {
            // triangular
            let upper = rng.chance(0.5);
            for r in 0..n {
                for c in 0..n {
                    if (upper && c < r) || (!upper && c > r) {
                        set(&mut m, r, c, 0.0);
                    }
                }
            }
        }
        3 => {
            // permutation-like: one dominant entry per row (pivot interchanges)
            for x in &mut m {
                *x *= 1e-3;
            }
            let mut perm: Vec<usize> = (0..n).collect();
            for k in (1..n).rev() {
                perm.swap(k, rng.below(k as u64 + 1) as usize);
            }
            for (r, &c) in perm.iter().enumerate() {
                set(&mut m, r, c, rng.uniform(1.0, 5.0));
            }
        }
        4 => {
            // Hilbert matrix: ill-conditioned
            for r in 0..n {
                for c in 0..n {
                    set(&mut m, r, c, 1.0 / ((r + c + 1) as f64));
                }
            }
        }
        5 => {
            // near-zero pivot during elimination: row b = row a + tiny noise
            if n > 1 {
                let (a, b) = (0usize, 1 + rng.below(n as u64 - 1) as usize);
                let eps = 10f64.powi(-(rng.range_i64(10, 16) as i32));
                for c in 0..n {
                    let x = get(&m, a, c) + eps * rng.uniform(-1.0, 1.0);
                    set(&mut m, b, c, x);
                }
            }
        }
        6 => {
            // huge / tiny scales, possibly overflowing
            let k = 10f64.powi(rng.range_i64(-300, 300) as i32);
            for x in &mut m {
                *x *= k;
            }
        }
        7 => {
            // a non-finite entry
            let k = rng.below((n * n) as u64) as usize;
            let x = match rng.below(3) {
                0 => f64::NAN,
                1 => f64::INFINITY,
                _ => f64::NEG_INFINITY,
            };
            if let Some(p) = m.get_mut(k) {
                *p = x;
            }
        }
        8 => {
            // diagonal with repeated values (eigen ties)
            m.fill(0.0);
            for r in 0..n {
                set(&mut m, r, r, rng.range_i64(-2, 2) as f64);
            }
        }
        _ => {}
    }
    if symmetric {
        for r in 0..n {
            for c in r + 1..n {
                let x = get(&m, r, c);
                set(&mut m, c, r, x);
            }
        }
        if rng.chance(0.15) && n > 1 {
            // near-symmetric: perturb one pair around the SM_EFF_ZERO test
            let (r, c) = (0usize, 1 + rng.below(n as u64 - 1) as usize);
            let d = match rng.below(3) {
                0 => 1e-12,
                1 => 1e-12 * rng.uniform(0.5, 2.0),
                _ => 1e-13,
            };
            let x = get(&m, r, c) + d;
            set(&mut m, r, c, x);
        }
        if rng.chance(0.08) {
            // huge entries: overflow inside the rotations
            for x in &mut m {
                *x *= 1e307;
            }
        }
    }
    m
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let pick = rng.below(100);
    let (name, op) = match pick {
        0..=3 => ("copy", Op::Copy),
        4..=6 => ("resize", Op::Resize),
        7..=10 => ("access", Op::Access),
        11..=16 => ("is_symmetric", Op::IsSymmetric),
        17..=18 => ("clear", Op::Clear),
        19..=22 => ("identity", Op::Identity),
        23..=37 => ("lu", Op::Lu),
        38..=50 => ("solve_lu", Op::SolveLu),
        51..=70 => ("solve", Op::Solve),
        71..=87 => ("eigen", Op::Eigen),
        _ => ("eigen3", Op::Eigen3),
    };
    let n = if op == Op::Eigen3 { 3 } else { dim(rng) };
    let (rows, cols) = match op {
        Op::Copy | Op::Resize | Op::Access | Op::Clear | Op::Identity | Op::IsSymmetric
            if rng.chance(0.4) =>
        {
            (n, dim(rng))
        }
        Op::Lu | Op::Solve | Op::Eigen if rng.chance(0.04) => (n, n + 1),
        _ => (n, n),
    };
    let symmetric = matches!(op, Op::Eigen | Op::Eigen3 | Op::IsSymmetric) && rng.chance(0.85);
    let m = if rows == cols {
        square(rng, rows, symmetric)
    } else {
        (0..rows * cols).map(|_| entry(rng)).collect()
    };
    let n_v = match op {
        Op::Solve if rng.chance(0.05) => {
            if rng.chance(0.5) {
                rows + 1
            } else {
                rows - 1
            }
        }
        Op::SolveLu | Op::Solve => rows,
        _ => 0,
    };
    let v: Vec<f64> = (0..n_v)
        .map(|_| match rng.below(8) {
            0 => 0.0,
            _ => entry(rng),
        })
        .collect();
    let (i, j) = match op {
        Op::Resize => {
            if rng.chance(0.2) {
                (rows as i64, cols as i64)
            } else {
                (dim(rng) as i64, dim(rng) as i64)
            }
        }
        Op::Access => (rng.below(rows as u64) as i64, rng.below(cols as u64) as i64),
        _ => (0, 0),
    };
    Inputs {
        op: name.to_string(),
        rows,
        cols,
        m: m.into_iter().map(num).collect(),
        v: v.into_iter().map(num).collect(),
        i,
        j,
        s: entry(rng),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "geom.matrix"
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
