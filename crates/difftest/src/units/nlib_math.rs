//! Runner for unit `nlib.math`: NLib control-point arithmetic, quadratic solver,
//! floating-point range checks, real matrices, power-basis surfaces, reallocation.
//!
//! C++: `solidcore_ffi::units::nlib_math` (upstream `source/SMLib/src/NL_Math.cpp`,
//! `NL_SrfPoly.cpp`, `NL_Util.cpp`). Rust: `solidcore::nurbs::math`.
//!
//! | op (`inputs.op`) | Rust                                             | upstream |
//! |------------------|--------------------------------------------------|----------|
//! | `point_ops`      | `CPoint` methods, `combine2_pts`, ...            | `N_Combine2CPts` ... `N_CPtsAreEqual` (see [`POINT_OPS_LAYOUT`]) |
//! | `float_op`       | `float_op_is_bad`                                | `N_FloatOpIsBad` |
//! | `quadratic`      | `solve_quadratic`                                | `N_SolveQuadraticEq` |
//! | `pascal`         | `pascal_triangle`                                | `N_PascalTriRow` |
//! | `check_mem`      | `RealMatrix::{null, from_rows, is_null, ensure}` | `N_InitRealMatrix`, `N_CreateRealMatrix`, `N_RealMatrixIsNULL`, `N_CheckMemRealMatrix` |
//! | `multiply`       | `RealMatrix::multiply_into`                      | `N_RealMatrixMultiply` |
//! | `transpose`      | `RealMatrix::transpose_into` / `transpose_in_place` | `N_RealMatrixTranspose` |
//! | `mul_pts` / `mul_cpts` | `RealMatrix::multiply_points` / `multiply_cpoints` | `N_RealMatrixMultiplyPtArray` / `CPtArray` |
//! | `lu`             | `RealMatrix::lu_decompose` / `lu_decompose_pivot` | `N_RealMatrixLuDecompose` / `Pivot` |
//! | `for_back_pts` / `for_back_cpts` | `RealMatrix::for_back_points` / `for_back_cpoints` | `N_RealMatrixForBack` |
//! | `right_for_back` | `RealMatrix::right_for_back`                     | `N_RealMatrixRightForBack` |
//! | `solve_pivot`    | `lu_decompose_pivot` + `for_back_pivot`          | `N_RealMatrixLuDecomposePivot` + `N_RealMatrixForBackPivot` |
//! | `inverse`        | `RealMatrix::inverse_into`                       | `N_RealMatrixInverse` |
//! | `lstsq`          | `RealMatrix::lst_sq_solve`                       | `N_RealMatrixLstSqSolve` |
//! | `srf_eval`       | `power_basis_srf_eval_pts`, `srf_power_basis_eval_pt` | `N_PowerBasisSrfEvalPts`, `N_SrfPowerBasisEvalPt` |
//! | `srf_derivs`     | `srf_power_basis_eval_derivs`                    | `N_SrfPowerBasisEvalDerivs` |
//! | `realloc_1d` / `realloc_2d` | `realloc_1d` / `realloc_2d`           | `N_Realloc1d*Array` / `N_Realloc2d*Array` |
//!
//! Errors are normalised to `nlib_<NL_*_ERR number>` on both sides. Matrix results
//! are compared on their *defined region* (the entries upstream writes for the
//! result type). Every run has at least [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::math::{
    blend_pt, combine2_pts, combine4_pts, float_op_is_bad, pascal_triangle,
    power_basis_srf_eval_pts, realloc_1d, realloc_2d, solve_quadratic, srf_power_basis_eval_derivs,
    srf_power_basis_eval_pt, translate_sum2_pts, CPoint, FloatOp, MatrixType, QuadraticRoots,
    RealMatrix, MODEL_TOL, NOW, NOZ,
};
use solidcore::{SolidError, Vec3};
use solidcore_ffi::units::nlib_math as ffi;
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// Output record of `point_ops`: `(key, number of doubles)` in shim order.
pub const POINT_OPS_LAYOUT: &[(&str, usize)] = &[
    ("combine2_cpts", 4),
    ("combine2_pts", 3),
    ("translate_sum2_pts", 3),
    ("translate_sum2_cpts", 4),
    ("combine4_pts", 3),
    ("combine4_cpts", 4),
    ("copy_cpt", 4),
    ("copy_pt", 3),
    ("cpt_to_pt_euclid", 3),
    ("cpt_to_pt_and_w", 4),
    ("weight", 4),
    ("vector_blend_cpt", 4),
    ("scale_cpt", 4),
    ("scale_cpt_xyz", 4),
    ("vector_blend_pt", 3),
    ("scale_pt", 3),
    ("transform_cpt", 4),
    ("translate_cpt", 4),
    ("scale_cpt_with_pt_and_vector", 4),
    ("pt_to_cpt", 4),
    ("cpt_get_w", 1),
    ("pt_to_xyz", 3),
    ("pt_from_xyz", 3),
    ("cpt_to_wxwywz", 4),
    ("cpt_from_wxwywz", 4),
    ("cpt_set_x", 4),
    ("cpt_set_y", 4),
    ("cpt_set_z", 4),
    ("cpt_set_w", 4),
    ("sum2_pts", 3),
    ("sum2_cpts", 4),
    ("diff2_pts", 3),
    ("diff2_cpts", 4),
    ("dot2_pts", 1),
    ("dot2_cpts_in4d", 1),
    ("pt_magnitude", 1),
    ("cpt_magnitude", 1),
    ("cpt_rat_deriv", 4),
    ("cpt_to_pt_no_w", 4),
    ("cpt_get_z", 1),
    ("cpt_to_pt", 3),
    ("swap_reals", 2),
    ("swap_cpts", 8),
    ("swap_integers", 2),
    ("cpts_are_equal_ab", 1),
    ("cpts_are_equal_ac", 1),
];

/// `nlib.math` runner.
pub struct Runner;

/// Matrix type in fixtures.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Mt {
    Full,
    LowerLeft,
    UpperRight,
    Banded,
}

impl Mt {
    fn code(self) -> i32 {
        self.rust().code()
    }
    fn rust(self) -> MatrixType {
        match self {
            Mt::Full => MatrixType::Full,
            Mt::LowerLeft => MatrixType::LowerLeft,
            Mt::UpperRight => MatrixType::UpperRight,
            Mt::Banded => MatrixType::Banded,
        }
    }
}

/// A matrix: logical shape `n x m`, type, bandwidth and its storage rows.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Mat {
    n: usize,
    m: usize,
    mtp: Mt,
    bw: usize,
    rows: Vec<Vec<f64>>,
}

impl Mat {
    fn flat(&self) -> Vec<f64> {
        self.rows.iter().flatten().copied().collect()
    }
    fn cols(&self) -> usize {
        self.rows.first().map_or(0, Vec::len)
    }
    fn rust(&self) -> Result<RealMatrix, SolidError> {
        RealMatrix::from_rows(self.n, self.m, self.rows.clone(), self.mtp.rust(), self.bw)
    }
}

/// Item kind of a 1d reallocation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Item1 {
    Flag,
    Real,
    Pt,
    Cpt,
}

/// Item kind of a 2d reallocation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Item2 {
    Real,
    Cpt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)] // parsed once per case; boxing buys nothing
enum Inputs {
    PointOps {
        a: [f64; 4],
        b: [f64; 4],
        c: [f64; 4],
        d: [f64; 4],
        pa: [f64; 3],
        pb: [f64; 3],
        pc: [f64; 3],
        pd: [f64; 3],
        alpha: f64,
        beta: f64,
        gamma: f64,
        delta: f64,
        w: f64,
        mat: [f64; 16],
        i1: i64,
        i2: i64,
    },
    FloatOp {
        x: f64,
        y: f64,
        flag: i32,
    },
    Quadratic {
        a: f64,
        b: f64,
        c: f64,
    },
    Pascal {
        row: usize,
    },
    CheckMem {
        existing: Option<Mat>,
        n: usize,
        m: usize,
        mtp: Mt,
        bw: usize,
    },
    Multiply {
        a: Mat,
        b: Mat,
        c: Option<Mat>,
    },
    Transpose {
        a: Mat,
        in_place: bool,
        b: Option<Mat>,
    },
    MulPts {
        a: Mat,
        pts: Vec<[f64; 3]>,
    },
    MulCpts {
        a: Mat,
        pts: Vec<[f64; 4]>,
    },
    Lu {
        a: Mat,
        pivot: bool,
    },
    ForBackPts {
        a: Mat,
        decompose: bool,
        rhs: Vec<[f64; 3]>,
    },
    ForBackCpts {
        a: Mat,
        decompose: bool,
        rhs: Vec<[f64; 4]>,
    },
    RightForBack {
        a: Mat,
        decompose: bool,
        y: Vec<f64>,
    },
    SolvePivot {
        a: Mat,
        b: Vec<f64>,
    },
    Inverse {
        a: Mat,
        lud: bool,
        b: Option<Mat>,
    },
    Lstsq {
        a: Mat,
        rhs: Vec<f64>,
    },
    SrfEval {
        net: Vec<Vec<[f64; 4]>>,
        n: usize,
        m: usize,
        u: f64,
        v: f64,
    },
    SrfDerivs {
        net: Vec<Vec<[f64; 4]>>,
        p: usize,
        q: usize,
        u: f64,
        v: f64,
        mfl: bool,
        udr: usize,
        vdr: usize,
    },
    Realloc1d {
        kind: Item1,
        items: Vec<Vec<f64>>,
        n: usize,
        m: usize,
    },
    Realloc2d {
        kind: Item2,
        items: Vec<Vec<Vec<f64>>>,
        n: usize,
        m: usize,
        k: usize,
        l: usize,
    },
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// C++ failure -> normalised output (operation error) or harness error.
fn cpp_err(e: &FfiError) -> Result<Output, HarnessError> {
    match e {
        FfiError::Status(code) if *code > 0 => Ok(Output::err(format!("nlib_{code}"))),
        FfiError::Status(_) => Err(harness(format!("shim failure: {e}"))),
        FfiError::InvalidArgument(m) => Err(harness(format!("ffi rejected: {m}"))),
    }
}

/// Rust failure -> normalised output. Non-NLib errors (`InvalidInput`: an index
/// upstream would take outside its arrays) cannot match any C++ output, so such a
/// case fails, as it should inside the wrapper's domain.
fn rust_err(e: &SolidError) -> Output {
    if let Some(d) = Output::unported(e) {
        return d;
    }
    match e {
        SolidError::Nlib { code, .. } => Output::err(format!("nlib_{}", code.code())),
        other => Output::err(format!("rust: {other}")),
    }
}

/// Builds the FFI view of `m` over `flat` (its flattened storage).
fn view<'a>(m: &Mat, flat: &'a [f64]) -> ffi::Matrix<'a> {
    ffi::Matrix {
        data: flat,
        rows: m.rows.len(),
        cols: m.cols(),
        n: m.n,
        m: m.m,
        mtp: m.mtp.code(),
        bw: m.bw,
    }
}

fn mat_out(prefix: &str, o: ffi::MatrixOut, out: Output) -> Output {
    out.ints(&format!("{prefix}meta"), vec![o.n, o.m, o.mtp, o.bw])
        .values(&format!("{prefix}region"), o.region)
}

fn cpt_arr(p: [f64; 4]) -> CPoint {
    CPoint::new(p[0], p[1], p[2], p[3])
}

fn pt_arr(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn split_layout(flat: &[f64]) -> Result<Output, HarnessError> {
    let total: usize = POINT_OPS_LAYOUT.iter().map(|(_, n)| n).sum();
    if flat.len() != total {
        return Err(harness(format!(
            "point_ops returned {} values, layout has {total}",
            flat.len()
        )));
    }
    let mut out = Output::new();
    let mut at = 0usize;
    for (key, n) in POINT_OPS_LAYOUT {
        let part = flat
            .get(at..at + n)
            .ok_or_else(|| harness("point_ops layout"))?;
        out = out.values(key, part.to_vec());
        at += n;
    }
    Ok(out)
}

/// True when some matrix of the inputs has storage rows of different lengths. The
/// C++ side gets the flattened rows (a truncated row plus a longer one can still
/// have the right total), the Rust side the rows themselves: not a valid matrix.
fn has_ragged_matrix(v: &Value) -> bool {
    match v {
        Value::Object(o) => {
            let ragged = o.contains_key("mtp")
                && o.get("rows").and_then(Value::as_array).is_some_and(|rows| {
                    let len = |r: &Value| r.as_array().map(Vec::len);
                    rows.iter().any(|r| len(r) != rows.first().and_then(len))
                });
            ragged || o.values().any(has_ragged_matrix)
        }
        Value::Array(a) => a.iter().any(has_ragged_matrix),
        _ => false,
    }
}

#[allow(clippy::too_many_lines)]
fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let value = serde_json::to_value(i).map_err(|e| harness(format!("inputs: {e}")))?;
    if has_ragged_matrix(&value) {
        return Err(harness("matrix storage rows differ in length"));
    }
    let out = match i {
        Inputs::PointOps {
            a,
            b,
            c,
            d,
            pa,
            pb,
            pc,
            pd,
            alpha,
            beta,
            gamma,
            delta,
            w,
            mat,
            i1,
            i2,
        } => {
            let mut input = Vec::with_capacity(ffi::POINT_OPS_INPUTS);
            for p in [a, b, c, d] {
                input.extend_from_slice(p);
            }
            for p in [pa, pb, pc, pd] {
                input.extend_from_slice(p);
            }
            input.extend_from_slice(&[*alpha, *beta, *gamma, *delta, *w]);
            input.extend_from_slice(mat);
            match ffi::point_ops(&input, [*i1, *i2]) {
                Ok(flat) => return split_layout(&flat),
                Err(e) => Err(e),
            }
        }
        Inputs::FloatOp { x, y, flag } => {
            ffi::float_op(*x, *y, *flag).map(|b| Output::new().int("bad", i64::from(b)))
        }
        Inputs::Quadratic { a, b, c } => ffi::quadratic(*a, *b, *c).map(|(n, r)| {
            let roots = match n {
                1 => vec![r[0]],
                2 => vec![r[0], r[1]],
                _ => vec![],
            };
            // n = 1 always has r1 == r2 upstream; record that too.
            let same = i64::from(n != 1 || r[0].to_bits() == r[1].to_bits());
            Output::new()
                .int("n", n)
                .values("roots", roots)
                .int("one_root_r1_eq_r2", same)
        }),
        Inputs::Pascal { row } => ffi::pascal(*row).map(|v| Output::new().ints("tri", v)),
        Inputs::CheckMem {
            existing,
            n,
            m,
            mtp,
            bw,
        } => {
            let flat = existing.as_ref().map(Mat::flat).unwrap_or_default();
            let ex = existing.as_ref().map(|e| view(e, &flat));
            ffi::check_mem(ex.as_ref(), (*n, *m, mtp.code(), *bw))
                .map(|r| Output::new().ints("meta", r.to_vec()))
        }
        Inputs::Multiply { a, b, c } => {
            let (fa, fb) = (a.flat(), b.flat());
            let fc = c.as_ref().map(Mat::flat).unwrap_or_default();
            let cv = c.as_ref().map(|m| view(m, &fc));
            ffi::multiply(&view(a, &fa), &view(b, &fb), cv.as_ref())
                .map(|o| mat_out("", o, Output::new()))
        }
        Inputs::Transpose { a, in_place, b } => {
            let fa = a.flat();
            let fb = b.as_ref().map(Mat::flat).unwrap_or_default();
            let bv = b.as_ref().map(|m| view(m, &fb));
            ffi::transpose(&view(a, &fa), *in_place, bv.as_ref())
                .map(|o| mat_out("", o, Output::new()))
        }
        Inputs::MulPts { a, pts } => {
            let fa = a.flat();
            let flat: Vec<f64> = pts.iter().flatten().copied().collect();
            ffi::mul_pts(&view(a, &fa), &flat, false).map(|v| Output::new().values("q", v))
        }
        Inputs::MulCpts { a, pts } => {
            let fa = a.flat();
            let flat: Vec<f64> = pts.iter().flatten().copied().collect();
            ffi::mul_pts(&view(a, &fa), &flat, true).map(|v| Output::new().values("q", v))
        }
        Inputs::Lu { a, pivot } => {
            let fa = a.flat();
            ffi::lu(&view(a, &fa), *pivot)
                .map(|(ind, r)| Output::new().ints("ind", ind).values("region", r))
        }
        Inputs::ForBackPts { a, decompose, rhs } => {
            let fa = a.flat();
            let flat: Vec<f64> = rhs.iter().flatten().copied().collect();
            ffi::for_back(&view(a, &fa), *decompose, &flat, false)
                .map(|v| Output::new().values("q", v))
        }
        Inputs::ForBackCpts { a, decompose, rhs } => {
            let fa = a.flat();
            let flat: Vec<f64> = rhs.iter().flatten().copied().collect();
            ffi::for_back(&view(a, &fa), *decompose, &flat, true)
                .map(|v| Output::new().values("q", v))
        }
        Inputs::RightForBack { a, decompose, y } => {
            let fa = a.flat();
            ffi::right_for_back(&view(a, &fa), *decompose, y).map(|v| Output::new().values("x", v))
        }
        Inputs::SolvePivot { a, b } => {
            let fa = a.flat();
            ffi::solve_pivot(&view(a, &fa), b).map(|v| Output::new().values("x", v))
        }
        Inputs::Inverse { a, lud, b } => {
            let fa = a.flat();
            let fb = b.as_ref().map(Mat::flat).unwrap_or_default();
            let bv = b.as_ref().map(|m| view(m, &fb));
            ffi::inverse(&view(a, &fa), *lud, bv.as_ref())
                .map(|(o, ar)| mat_out("", o, Output::new()).values("a_region", ar))
        }
        Inputs::Lstsq { a, rhs } => {
            let fa = a.flat();
            ffi::lstsq(&view(a, &fa), rhs).map(|v| Output::new().values("x", v))
        }
        Inputs::SrfEval { net, n, m, u, v } => {
            let (rows, cols, flat) = net_flat(net)?;
            ffi::srf_eval(&flat, rows, cols, *n, *m, *u, *v).map(|r| {
                Output::new()
                    .values("eval_pts", r[..3].to_vec())
                    .values("eval_pt", r[3..].to_vec())
            })
        }
        Inputs::SrfDerivs {
            net,
            p,
            q,
            u,
            v,
            mfl,
            udr,
            vdr,
        } => {
            let (rows, cols, flat) = net_flat(net)?;
            ffi::srf_derivs(&flat, rows, cols, *p, *q, *u, *v, *mfl, *udr, *vdr)
                .map(|r| Output::new().values("sd", r))
        }
        Inputs::Realloc1d { kind, items, n, m } => {
            let (k, width) = match kind {
                Item1::Flag => (0, 1),
                Item1::Real => (1, 1),
                Item1::Pt => (2, 3),
                Item1::Cpt => (3, 4),
            };
            // Ragged items would shift the flattened data (the Rust side reads each
            // item on its own): not a valid array.
            if items.iter().any(|x| x.len() != width) {
                return Err(harness("realloc1d: item width"));
            }
            let flat: Vec<f64> = items.iter().flatten().copied().collect();
            ffi::realloc(k, &flat, 0, 0, [*n, *m, 0, 0]).map(|r| Output::new().values("block", r))
        }
        Inputs::Realloc2d {
            kind,
            items,
            n,
            m,
            k,
            l,
        } => {
            let rows = items.len();
            let cols = items.first().map_or(0, Vec::len);
            let (kd, width) = match kind {
                Item2::Real => (4, 1),
                Item2::Cpt => (5, 4),
            };
            if items
                .iter()
                .any(|r| r.len() != cols || r.iter().any(|x| x.len() != width))
            {
                return Err(harness("realloc2d: ragged rows or items"));
            }
            let flat: Vec<f64> = items.iter().flatten().flatten().copied().collect();
            ffi::realloc(kd, &flat, rows, cols, [*n, *m, *k, *l])
                .map(|r| Output::new().values("block", r))
        }
    };
    out.or_else(|e| cpp_err(&e))
}

fn net_flat(net: &[Vec<[f64; 4]>]) -> Result<(usize, usize, Vec<f64>), HarnessError> {
    let rows = net.len();
    let cols = net.first().map_or(0, Vec::len);
    if net.iter().any(|r| r.len() != cols) {
        return Err(harness("ragged net"));
    }
    Ok((
        rows,
        cols,
        net.iter().flatten().flatten().copied().collect(),
    ))
}

fn net_rust(net: &[Vec<[f64; 4]>]) -> Vec<Vec<CPoint>> {
    net.iter()
        .map(|r| r.iter().map(|p| cpt_arr(*p)).collect())
        .collect()
}

/// Defined region of a Rust matrix (mirrors `emit_region` in the shim).
fn region(mx: &RealMatrix) -> Result<Vec<f64>, SolidError> {
    let (n, m, bw) = (mx.n(), mx.m(), mx.bandwidth());
    let mut out = Vec::new();
    for i in 0..=n {
        match mx.matrix_type() {
            MatrixType::Full => {
                for j in 0..=m {
                    out.push(mx.get(i, j)?);
                }
            }
            MatrixType::LowerLeft => {
                for j in 0..=i.min(m) {
                    out.push(mx.get(i, j)?);
                }
            }
            MatrixType::UpperRight => {
                for j in i..=m {
                    out.push(mx.get(i, j)?);
                }
            }
            MatrixType::Banded => {
                let sbw = bw / 2;
                for c in 0..bw {
                    if (i + c) >= sbw && i + c - sbw <= n {
                        out.push(mx.get(i, c)?);
                    }
                }
            }
        }
    }
    Ok(out)
}

fn meta_of(mx: &RealMatrix) -> Vec<i64> {
    vec![
        mx.n() as i64,
        mx.m() as i64,
        i64::from(mx.matrix_type().code()),
        mx.bandwidth() as i64,
    ]
}

fn rmat_out(mx: &RealMatrix) -> Result<Output, SolidError> {
    Ok(Output::new()
        .ints("meta", meta_of(mx))
        .values("region", region(mx)?))
}

fn cpts_flat(v: &[CPoint]) -> Vec<f64> {
    v.iter().flat_map(|p| p.to_array()).collect()
}

fn pts_flat(v: &[Vec3]) -> Vec<f64> {
    v.iter().flat_map(|p| p.to_array()).collect()
}

fn opt_rust(m: Option<&Mat>) -> Result<RealMatrix, SolidError> {
    m.map_or_else(|| Ok(RealMatrix::null()), Mat::rust)
}

fn float_op_of(op: i32) -> Result<FloatOp, HarnessError> {
    match op {
        1 => Ok(FloatOp::Addition),
        2 => Ok(FloatOp::Subtraction),
        3 => Ok(FloatOp::Multiplication),
        4 => Ok(FloatOp::Division),
        _ => Err(harness(format!("op {op}"))),
    }
}

#[allow(clippy::too_many_lines)]
fn point_ops_rust(i: &Inputs) -> Result<Output, SolidError> {
    let Inputs::PointOps {
        a,
        b,
        c,
        d,
        pa,
        pb,
        pc,
        pd,
        alpha,
        beta,
        gamma,
        delta,
        w,
        mat,
        i1,
        i2,
    } = i
    else {
        return Ok(Output::err("not point_ops"));
    };
    let (a, b, c, d) = (cpt_arr(*a), cpt_arr(*b), cpt_arr(*c), cpt_arr(*d));
    let (pa, pb, pc, pd) = (pt_arr(*pa), pt_arr(*pb), pt_arr(*pc), pt_arr(*pd));
    let (alpha, beta, gamma, delta, w) = (*alpha, *beta, *gamma, *delta, *w);
    let rows: Vec<Vec<f64>> = mat.chunks(4).map(<[f64]>::to_vec).collect();
    let m4 = RealMatrix::from_rows(3, 3, rows, MatrixType::Full, 3)?;
    let mut o = Output::new();
    let c4 = |p: CPoint| p.to_array().to_vec();
    let p3 = |p: Vec3| p.to_array().to_vec();
    o = o.values("combine2_cpts", c4(CPoint::combine2(alpha, a, beta, b)));
    o = o.values("combine2_pts", p3(combine2_pts(alpha, pa, beta, pb)));
    o = o.values(
        "translate_sum2_pts",
        p3(translate_sum2_pts(pc, alpha, pa, beta, pb)),
    );
    o = o.values(
        "translate_sum2_cpts",
        c4(CPoint::translate_sum2(c, alpha, a, beta, b)),
    );
    o = o.values(
        "combine4_pts",
        p3(combine4_pts(alpha, pa, beta, pb, gamma, pc, delta, pd)),
    );
    o = o.values(
        "combine4_cpts",
        c4(CPoint::combine4(alpha, a, beta, b, gamma, c, delta, d)),
    );
    o = o.values("copy_cpt", c4(a));
    o = o.values("copy_pt", p3(pa));
    o = o.values("cpt_to_pt_euclid", p3(a.to_euclid()));
    let (pe, we) = a.to_euclid_and_w();
    o = o.values("cpt_to_pt_and_w", vec![pe.x, pe.y, pe.z, we]);
    o = o.values("weight", c4(CPoint::weighted(pa, w)));
    let mut bl = b;
    bl.blend(alpha, a);
    o = o.values("vector_blend_cpt", c4(bl));
    o = o.values("scale_cpt", c4(a.scale(alpha)));
    o = o.values("scale_cpt_xyz", c4(a.scale_xyz(alpha)));
    let mut bp = pb;
    blend_pt(&mut bp, alpha, pa);
    o = o.values("vector_blend_pt", p3(bp));
    o = o.values("scale_pt", p3(pa * alpha));
    o = o.values("transform_cpt", c4(a.transform(&m4)?));
    o = o.values("translate_cpt", c4(a.translate(pb)));
    o = o.values("scale_cpt_with_pt_and_vector", c4(a.scale_about(pc, pd)));
    o = o.values("pt_to_cpt", c4(CPoint::from_point(pa)));
    o = o.values("cpt_get_w", vec![a.w]);
    o = o.values("pt_to_xyz", p3(pa));
    o = o.values("pt_from_xyz", p3(Vec3::new(alpha, beta, gamma)));
    o = o.values("cpt_to_wxwywz", a.to_array().to_vec());
    o = o.values(
        "cpt_from_wxwywz",
        c4(CPoint::new(alpha, beta, gamma, delta)),
    );
    let (mut sx, mut sy, mut sz, mut sw) = (a, a, a, a);
    sx.x = alpha;
    sy.y = alpha;
    sz.z = alpha;
    sw.w = alpha;
    o = o.values("cpt_set_x", c4(sx));
    o = o.values("cpt_set_y", c4(sy));
    o = o.values("cpt_set_z", c4(sz));
    o = o.values("cpt_set_w", c4(sw));
    o = o.values("sum2_pts", p3(pa + pb));
    o = o.values("sum2_cpts", c4(CPoint::sum(a, b)));
    o = o.values("diff2_pts", p3(pa - pb));
    o = o.values("diff2_cpts", c4(CPoint::diff(a, b)));
    o = o.values("dot2_pts", vec![pa.dot(pb)]);
    o = o.values("dot2_cpts_in4d", vec![CPoint::dot_4d(a, b)]);
    o = o.values("pt_magnitude", vec![pa.length()]);
    o = o.values("cpt_magnitude", vec![a.magnitude()]);
    o = o.values("cpt_rat_deriv", c4(CPoint::rat_deriv(a, pb, beta)));
    o = o.values("cpt_to_pt_no_w", c4(a.unweighted()));
    o = o.values("cpt_get_z", vec![a.z]);
    o = o.values("cpt_to_pt", p3(a.drop_w()));
    let (mut s1, mut s2) = (alpha, beta);
    core::mem::swap(&mut s1, &mut s2);
    o = o.values("swap_reals", vec![s1, s2]);
    let (mut ca, mut cb) = (a, b);
    core::mem::swap(&mut ca, &mut cb);
    let mut sc = c4(ca);
    sc.extend(c4(cb));
    o = o.values("swap_cpts", sc);
    let (mut j1, mut j2) = (*i1, *i2);
    core::mem::swap(&mut j1, &mut j2);
    o = o.values("swap_integers", vec![j1 as f64, j2 as f64]);
    o = o.values(
        "cpts_are_equal_ab",
        vec![f64::from(u8::from(a.approx_eq(b, MODEL_TOL)))],
    );
    o = o.values(
        "cpts_are_equal_ac",
        vec![f64::from(u8::from(a.approx_eq(c, MODEL_TOL)))],
    );
    Ok(o)
}

#[allow(clippy::too_many_lines)]
fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let out: Result<Output, SolidError> = match i {
        Inputs::PointOps { .. } => point_ops_rust(i),
        Inputs::FloatOp { x, y, flag } => {
            let op = float_op_of(*flag)?;
            Ok(Output::new().int("bad", i64::from(float_op_is_bad(*x, *y, op))))
        }
        Inputs::Quadratic { a, b, c } => solve_quadratic(*a, *b, *c).map(|r| {
            let (n, roots) = match r {
                QuadraticRoots::None => (0, vec![]),
                QuadraticRoots::One(x) => (1, vec![x]),
                QuadraticRoots::Two(x, y) => (2, vec![x, y]),
            };
            Output::new()
                .int("n", n)
                .values("roots", roots)
                .int("one_root_r1_eq_r2", 1)
        }),
        Inputs::Pascal { row } => pascal_triangle(*row)
            .map(|t| Output::new().ints("tri", t.into_iter().flatten().collect::<Vec<_>>())),
        Inputs::CheckMem {
            existing,
            n,
            m,
            mtp,
            bw,
        } => (|| {
            let mut mx = opt_rust(existing.as_ref())?;
            let was_null = mx.is_null();
            mx.ensure(*n, *m, mtp.rust(), *bw)?;
            let (mn, mm) = mx.max_index();
            let mut meta = meta_of(&mx);
            if Some(&(mn as i64)) != meta.first() || Some(&(mm as i64)) != meta.get(1) {
                return Ok(Output::err("max_index mismatch"));
            }
            meta.push(i64::from(was_null));
            Ok(Output::new().ints("meta", meta))
        })(),
        Inputs::Multiply { a, b, c } => (|| {
            let mut cm = opt_rust(c.as_ref())?;
            a.rust()?.multiply_into(&b.rust()?, &mut cm)?;
            rmat_out(&cm)
        })(),
        Inputs::Transpose { a, in_place, b } => (|| {
            let mut am = a.rust()?;
            if *in_place {
                am.transpose_in_place()?;
                rmat_out(&am)
            } else {
                let mut bm = opt_rust(b.as_ref())?;
                am.transpose_into(&mut bm)?;
                rmat_out(&bm)
            }
        })(),
        Inputs::MulPts { a, pts } => (|| {
            let p: Vec<Vec3> = pts.iter().map(|x| pt_arr(*x)).collect();
            let q = a.rust()?.multiply_points(&p)?;
            Ok(Output::new().values("q", pts_flat(&q)))
        })(),
        Inputs::MulCpts { a, pts } => (|| {
            let p: Vec<CPoint> = pts.iter().map(|x| cpt_arr(*x)).collect();
            let q = a.rust()?.multiply_cpoints(&p)?;
            Ok(Output::new().values("q", cpts_flat(&q)))
        })(),
        Inputs::Lu { a, pivot } => (|| {
            let mut am = a.rust()?;
            let ind: Vec<i64> = if *pivot {
                am.lu_decompose_pivot()?
                    .into_iter()
                    .map(|x| x as i64)
                    .collect()
            } else {
                am.lu_decompose()?;
                vec![]
            };
            Ok(Output::new()
                .ints("ind", ind)
                .values("region", region(&am)?))
        })(),
        Inputs::ForBackPts { a, decompose, rhs } => (|| {
            let mut am = a.rust()?;
            if *decompose {
                am.lu_decompose()?;
            }
            let p: Vec<Vec3> = rhs.iter().map(|x| pt_arr(*x)).collect();
            Ok(Output::new().values("q", cpts_flat(&am.for_back_points(&p)?)))
        })(),
        Inputs::ForBackCpts { a, decompose, rhs } => (|| {
            let mut am = a.rust()?;
            if *decompose {
                am.lu_decompose()?;
            }
            let p: Vec<CPoint> = rhs.iter().map(|x| cpt_arr(*x)).collect();
            Ok(Output::new().values("q", cpts_flat(&am.for_back_cpoints(&p)?)))
        })(),
        Inputs::RightForBack { a, decompose, y } => (|| {
            let mut am = a.rust()?;
            if *decompose {
                am.lu_decompose()?;
            }
            Ok(Output::new().values("x", am.right_for_back(y)?))
        })(),
        Inputs::SolvePivot { a, b } => (|| {
            let mut am = a.rust()?;
            let ind = am.lu_decompose_pivot()?;
            let mut x = b.clone();
            am.for_back_pivot(&ind, &mut x)?;
            Ok(Output::new().values("x", x))
        })(),
        Inputs::Inverse { a, lud, b } => (|| {
            let mut am = a.rust()?;
            let mut bm = opt_rust(b.as_ref())?;
            am.inverse_into(*lud, &mut bm)?;
            Ok(rmat_out(&bm)?.values("a_region", region(&am)?))
        })(),
        Inputs::Lstsq { a, rhs } => a
            .rust()
            .and_then(|am| am.lst_sq_solve(rhs))
            .map(|x| Output::new().values("x", x)),
        Inputs::SrfEval { net, n, m, u, v } => (|| {
            let pw = net_rust(net);
            let s1 = power_basis_srf_eval_pts(&pw, *n, *m, *u, *v)?;
            let s2 = srf_power_basis_eval_pt(&pw, *u, *v)?;
            Ok(Output::new()
                .values("eval_pts", s1.to_array().to_vec())
                .values("eval_pt", s2.to_array().to_vec()))
        })(),
        Inputs::SrfDerivs {
            net,
            p,
            q,
            u,
            v,
            mfl,
            udr,
            vdr,
        } => srf_power_basis_eval_derivs(&net_rust(net), *p, *q, *u, *v, *mfl, *udr, *vdr).map(
            |sd| {
                let flat: Vec<f64> = sd.iter().flatten().flat_map(|p| p.to_array()).collect();
                Output::new().values("sd", flat)
            },
        ),
        Inputs::Realloc1d { kind, items, n, m } => (|| {
            let block: Vec<f64> = match kind {
                Item1::Flag => {
                    let mut a: Vec<i16> = items
                        .iter()
                        .map(|x| x.first().copied().unwrap_or(0.0) as i16)
                        .collect();
                    realloc_1d(&mut a, *n, *m)?;
                    a.iter().take(n.min(m) + 1).map(|x| f64::from(*x)).collect()
                }
                Item1::Real => {
                    let mut a: Vec<f64> = items
                        .iter()
                        .map(|x| x.first().copied().unwrap_or(0.0))
                        .collect();
                    realloc_1d(&mut a, *n, *m)?;
                    a.into_iter().take(n.min(m) + 1).collect()
                }
                Item1::Pt => {
                    let mut a: Vec<Vec3> = items.iter().map(|x| vec3_of(x)).collect();
                    realloc_1d(&mut a, *n, *m)?;
                    pts_flat(a.get(..=*n.min(m)).unwrap_or(&[]))
                }
                Item1::Cpt => {
                    let mut a: Vec<CPoint> = items.iter().map(|x| cpt_of(x)).collect();
                    realloc_1d(&mut a, *n, *m)?;
                    cpts_flat(a.get(..=*n.min(m)).unwrap_or(&[]))
                }
            };
            Ok(Output::new().values("block", block))
        })(),
        Inputs::Realloc2d {
            kind,
            items,
            n,
            m,
            k,
            l,
        } => (|| {
            let (rl, sl) = (k.min(n), l.min(m));
            let block: Vec<f64> = match kind {
                Item2::Real => {
                    let mut a: Vec<Vec<f64>> = items
                        .iter()
                        .map(|r| {
                            r.iter()
                                .map(|x| x.first().copied().unwrap_or(0.0))
                                .collect()
                        })
                        .collect();
                    realloc_2d(&mut a, *n, *m, *k, *l)?;
                    a.get(..=*rl)
                        .unwrap_or(&[])
                        .iter()
                        .flat_map(|r| r.get(..=*sl).unwrap_or(&[]).to_vec())
                        .collect()
                }
                Item2::Cpt => {
                    let mut a: Vec<Vec<CPoint>> = items
                        .iter()
                        .map(|r| r.iter().map(|x| cpt_of(x)).collect())
                        .collect();
                    realloc_2d(&mut a, *n, *m, *k, *l)?;
                    a.get(..=*rl)
                        .unwrap_or(&[])
                        .iter()
                        .flat_map(|r| cpts_flat(r.get(..=*sl).unwrap_or(&[])))
                        .collect()
                }
            };
            Ok(Output::new().values("block", block))
        })(),
    };
    Ok(out.unwrap_or_else(|e| rust_err(&e)))
}

fn vec3_of(x: &[f64]) -> Vec3 {
    let g = |i: usize| x.get(i).copied().unwrap_or(0.0);
    Vec3::new(g(0), g(1), g(2))
}

fn cpt_of(x: &[f64]) -> CPoint {
    let g = |i: usize| x.get(i).copied().unwrap_or(0.0);
    CPoint::new(g(0), g(1), g(2), g(3))
}

// ---- generator -------------------------------------------------------------------

/// A coordinate: mostly uniform, sometimes integral, zero or tiny/huge.
fn coord(rng: &mut Rng) -> f64 {
    let r = rng.unit();
    if r < 0.6 {
        rng.uniform(-10.0, 10.0)
    } else if r < 0.75 {
        rng.range_i64(-5, 5) as f64
    } else if r < 0.82 {
        0.0
    } else if r < 0.9 {
        rng.uniform(-1.0, 1.0) * 1e-9
    } else {
        rng.uniform(-1.0, 1.0) * 1e6
    }
}

/// A weight: positive mostly, sometimes 1, 0 or negative.
fn weight(rng: &mut Rng) -> f64 {
    let r = rng.unit();
    if r < 0.6 {
        rng.uniform(0.1, 5.0)
    } else if r < 0.8 {
        1.0
    } else if r < 0.88 {
        0.0
    } else {
        rng.uniform(-3.0, -0.1)
    }
}

/// A control point with `NOZ` / `NOW` markers sometimes.
fn gen_cpt(rng: &mut Rng) -> [f64; 4] {
    let z = if rng.chance(0.2) { NOZ } else { coord(rng) };
    let w = if rng.chance(0.35) { NOW } else { weight(rng) };
    [coord(rng), coord(rng), z, w]
}

fn gen_pt(rng: &mut Rng) -> [f64; 3] {
    [coord(rng), coord(rng), coord(rng)]
}

fn gen_scalar(rng: &mut Rng) -> f64 {
    let r = rng.unit();
    if r < 0.7 {
        coord(rng)
    } else if r < 0.8 {
        1.0
    } else if r < 0.9 {
        -1.0
    } else if r < 0.95 {
        NOZ
    } else {
        NOW
    }
}

fn gen_point_ops(rng: &mut Rng) -> Inputs {
    let a = gen_cpt(rng);
    let mut b = gen_cpt(rng);
    let mut c = gen_cpt(rng);
    if rng.chance(0.2) {
        b = a;
        b[0] += rng.uniform(-1.0, 1.0) * 1e-7;
    }
    if rng.chance(0.2) {
        c = a;
    }
    let mut mat = [0.0f64; 16];
    for (k, v) in mat.iter_mut().enumerate() {
        *v = if rng.chance(0.3) {
            if k % 5 == 0 {
                1.0
            } else {
                0.0
            }
        } else {
            rng.uniform(-3.0, 3.0)
        };
    }
    if rng.chance(0.5) {
        // Affine: last row (0, 0, 0, 1).
        mat[12] = 0.0;
        mat[13] = 0.0;
        mat[14] = 0.0;
        mat[15] = 1.0;
    }
    Inputs::PointOps {
        a,
        b,
        c,
        d: gen_cpt(rng),
        pa: gen_pt(rng),
        pb: gen_pt(rng),
        pc: gen_pt(rng),
        pd: gen_pt(rng),
        alpha: gen_scalar(rng),
        beta: gen_scalar(rng),
        gamma: gen_scalar(rng),
        delta: gen_scalar(rng),
        w: if rng.chance(0.3) { NOW } else { weight(rng) },
        mat,
        i1: rng.range_i64(-1000, 1000),
        i2: rng.range_i64(-1000, 1000),
    }
}

fn special(rng: &mut Rng) -> f64 {
    let opts = [
        0.0,
        -0.0,
        1.0,
        -1.0,
        f64::MAX,
        -f64::MAX,
        f64::MIN_POSITIVE,
        -f64::MIN_POSITIVE,
        5e-324,
        1e308,
        1e-308,
        0.5,
        2.0,
        1e154,
        1e-154,
    ];
    let r = rng.unit();
    if r < 0.4 {
        *rng.choose(&opts).unwrap_or(&0.0)
    } else if r < 0.7 {
        let e = rng.range_i64(-320, 307) as i32;
        let s = if rng.chance(0.5) { 1.0 } else { -1.0 };
        s * rng.uniform(1.0, 10.0) * 10f64.powi(e)
    } else {
        coord(rng)
    }
}

fn gen_quadratic(rng: &mut Rng) -> Inputs {
    let r = rng.unit();
    let pick = |rng: &mut Rng| {
        let t = rng.unit();
        if t < 0.15 {
            0.0
        } else if t < 0.25 {
            rng.uniform(-1.0, 1.0) * 1e-12
        } else if t < 0.3 {
            special(rng)
        } else {
            coord(rng)
        }
    };
    if r < 0.2 {
        // From roots: a (x - r1)(x - r2), sometimes a double root.
        let a = rng.uniform(-5.0, 5.0);
        let r1 = rng.range_i64(-5, 5) as f64;
        let r2 = if rng.chance(0.4) {
            r1
        } else {
            rng.uniform(-5.0, 5.0)
        };
        Inputs::Quadratic {
            a,
            b: -a * (r1 + r2),
            c: a * r1 * r2,
        }
    } else {
        Inputs::Quadratic {
            a: pick(rng),
            b: pick(rng),
            c: pick(rng),
        }
    }
}

/// Values for a matrix entry.
fn entry(rng: &mut Rng, integral: bool) -> f64 {
    if integral {
        rng.range_i64(-4, 4) as f64
    } else {
        rng.uniform(-5.0, 5.0)
    }
}

/// A matrix of type `mtp`, `n x m` (banded/triangular: square), storage possibly
/// padded. `dominant` makes the diagonal large (stable LU); `singular` zeroes a row.
fn gen_mat(rng: &mut Rng, mtp: Mt, n: usize, m: usize, bw: usize, dominant: bool) -> Mat {
    let integral = rng.chance(0.3);
    let pad_r = if rng.chance(0.2) {
        rng.below(3) as usize
    } else {
        0
    };
    let pad_c = if rng.chance(0.2) {
        rng.below(3) as usize
    } else {
        0
    };
    let base_cols = if mtp == Mt::Banded {
        if bw == 1 && rng.chance(0.5) {
            2
        } else {
            bw
        }
    } else {
        m + 1
    };
    let rows_n = n + 1 + pad_r;
    let cols_n = base_cols + pad_c;
    let sbw = bw / 2;
    let mut rows = vec![vec![0.0; cols_n]; rows_n];
    for (i, row) in rows.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = entry(rng, integral);
            if dominant {
                let diag = if mtp == Mt::Banded { j == sbw } else { i == j };
                if diag {
                    *v = (rng.uniform(8.0, 20.0) + 4.0 * (n + bw) as f64)
                        * if rng.chance(0.5) { 1.0 } else { -1.0 };
                }
            }
        }
    }
    if !dominant && rng.chance(0.15) && n > 0 {
        let r = rng.below(n as u64 + 1) as usize;
        if let Some(row) = rows.get_mut(r) {
            row.fill(0.0);
        }
    }
    Mat {
        n,
        m,
        mtp,
        bw: if mtp == Mt::Banded {
            bw
        } else if rng.chance(0.1) {
            0
        } else {
            n
        },
        rows,
    }
}

fn odd_bw(rng: &mut Rng, n: usize) -> usize {
    // Odd bandwidths up to 2n+1 (and occasionally wider).
    let max = 2 * n + 1 + if rng.chance(0.1) { 2 } else { 0 };
    let k = rng.below((max / 2 + 1) as u64) as usize;
    2 * k + 1
}

fn gen_size(rng: &mut Rng) -> usize {
    if rng.chance(0.8) {
        rng.below(6) as usize
    } else {
        6 + rng.below(6) as usize
    }
}

fn gen_any_mat(rng: &mut Rng, dominant: bool) -> Mat {
    let n = gen_size(rng);
    let r = rng.unit();
    if r < 0.45 {
        let m = if rng.chance(0.6) { n } else { gen_size(rng) };
        gen_mat(rng, Mt::Full, n, m, 0, dominant)
    } else if r < 0.6 {
        gen_mat(rng, Mt::LowerLeft, n, n, 0, dominant)
    } else if r < 0.75 {
        gen_mat(rng, Mt::UpperRight, n, n, 0, dominant)
    } else {
        let bw = odd_bw(rng, n);
        gen_mat(rng, Mt::Banded, n, n, bw, dominant)
    }
}

fn gen_square(rng: &mut Rng, allow_banded: bool) -> Mat {
    let n = gen_size(rng);
    let dominant = rng.chance(0.75);
    if allow_banded && rng.chance(0.35) {
        let bw = odd_bw(rng, n);
        gen_mat(rng, Mt::Banded, n, n, bw, dominant)
    } else if rng.chance(0.07) {
        // Wrong type or shape: upstream errors.
        if rng.chance(0.5) {
            gen_mat(rng, Mt::LowerLeft, n, n, 0, dominant)
        } else {
            gen_mat(rng, Mt::Full, n, n + 1, 0, dominant)
        }
    } else {
        gen_mat(rng, Mt::Full, n, n, 0, dominant)
    }
}

/// An existing full output matrix of logical shape `n x m` with storage to match
/// (bandwidth 0 makes it "null" upstream, so it is reallocated).
fn gen_out(rng: &mut Rng, n: usize, m: usize) -> Option<Mat> {
    if rng.chance(0.6) {
        return None;
    }
    let (on, om) = if rng.chance(0.75) {
        (n + rng.below(2) as usize, m + rng.below(2) as usize)
    } else {
        // Possibly too small: NL_STO_ERR.
        (n.saturating_sub(1), m.saturating_sub(rng.below(2) as usize))
    };
    let bw = if rng.chance(0.15) {
        0
    } else {
        1 + rng.below(3) as usize
    };
    Some(Mat {
        n: on,
        m: om,
        mtp: Mt::Full,
        bw,
        rows: vec![vec![0.0; om + 1]; on + 1],
    })
}

fn gen_vec(rng: &mut Rng, len: usize) -> Vec<f64> {
    (0..len)
        .map(|_| {
            if rng.chance(0.15) {
                0.0
            } else {
                entry(rng, false)
            }
        })
        .collect()
}

fn gen_net(rng: &mut Rng, rows: usize, cols: usize, rational: bool) -> Vec<Vec<[f64; 4]>> {
    let planar = rng.chance(0.1);
    (0..rows)
        .map(|i| {
            (0..cols)
                .map(|j| {
                    let w = if rational {
                        if i == 0 && j == 0 {
                            rng.uniform(0.5, 2.0)
                        } else {
                            rng.uniform(-1.0, 2.0)
                        }
                    } else {
                        NOW
                    };
                    let z = if planar { NOZ } else { rng.uniform(-3.0, 3.0) };
                    [rng.uniform(-3.0, 3.0), rng.uniform(-3.0, 3.0), z, w]
                })
                .collect()
        })
        .collect()
}

fn gen_param(rng: &mut Rng) -> f64 {
    let r = rng.unit();
    if r < 0.6 {
        rng.unit()
    } else if r < 0.75 {
        *rng.choose(&[0.0, 1.0, 0.5]).unwrap_or(&0.0)
    } else {
        rng.uniform(-1.5, 2.5)
    }
}

fn gen_srf_derivs(rng: &mut Rng) -> Inputs {
    loop {
        let p = rng.below(6) as usize;
        let q = rng.below(6) as usize;
        let mut udr = rng.below(5) as usize;
        let mut vdr = if rng.chance(0.4) {
            udr
        } else {
            rng.below(5) as usize
        };
        if rng.chance(0.03) {
            // NL_INP_ERR.
            udr = 5;
            vdr = rng.below(6) as usize;
        }
        let rational = rng.chance(0.5);
        let mfl = rng.chance(0.5);
        if udr <= 4 && vdr <= 4 {
            if p + 1 + vdr.min(q) * (q + 1) + q > 17 {
                continue;
            }
            if rational && mfl && udr > vdr {
                continue;
            }
        }
        let rows = p + 1 + usize::from(rng.chance(0.2));
        let cols = q + 1 + usize::from(rng.chance(0.2));
        let mut net = gen_net(rng, rows, cols, rational);
        if rational && rng.chance(0.03) {
            for r in &mut net {
                for c in r.iter_mut() {
                    c[3] = 0.0;
                }
            }
        }
        return Inputs::SrfDerivs {
            net,
            p,
            q,
            u: gen_param(rng),
            v: gen_param(rng),
            mfl,
            udr,
            vdr,
        };
    }
}

fn gen_items(rng: &mut Rng, len: usize, width: usize, flags: bool) -> Vec<Vec<f64>> {
    (0..len)
        .map(|_| {
            (0..width)
                .map(|_| {
                    if flags {
                        rng.range_i64(-3, 3) as f64
                    } else {
                        coord(rng)
                    }
                })
                .collect()
        })
        .collect()
}

#[allow(clippy::too_many_lines)]
fn gen_case(rng: &mut Rng) -> Inputs {
    let r = rng.unit();
    if r < 0.14 {
        gen_point_ops(rng)
    } else if r < 0.2 {
        let op = 1 + rng.below(4) as i32;
        Inputs::FloatOp {
            x: special(rng),
            y: special(rng),
            flag: op,
        }
    } else if r < 0.27 {
        gen_quadratic(rng)
    } else if r < 0.29 {
        Inputs::Pascal {
            row: rng.below(67) as usize,
        }
    } else if r < 0.33 {
        let existing = if rng.chance(0.4) {
            None
        } else {
            let n = gen_size(rng);
            let m = gen_size(rng);
            let bw = if rng.chance(0.2) {
                0
            } else {
                1 + rng.below(4) as usize
            };
            Some(Mat {
                n,
                m,
                mtp: *rng
                    .choose(&[Mt::Full, Mt::LowerLeft, Mt::UpperRight, Mt::Banded])
                    .unwrap_or(&Mt::Full),
                bw,
                rows: vec![vec![0.0; m + 1]; n + 1],
            })
        };
        Inputs::CheckMem {
            existing,
            n: gen_size(rng),
            m: gen_size(rng),
            mtp: *rng
                .choose(&[Mt::Full, Mt::LowerLeft, Mt::UpperRight, Mt::Banded])
                .unwrap_or(&Mt::Full),
            bw: rng.below(6) as usize,
        }
    } else if r < 0.42 {
        let a = gen_any_mat(rng, false);
        // Conformable B mostly: B has a.m + 1 rows.
        let nb = if rng.chance(0.9) { a.m } else { gen_size(rng) };
        let b = match rng.below(4) {
            0 if a.m == nb => gen_mat(rng, Mt::LowerLeft, nb, nb, 0, false),
            1 if a.m == nb => gen_mat(rng, Mt::UpperRight, nb, nb, 0, false),
            2 if rng.chance(0.15) => {
                let bw = odd_bw(rng, nb);
                gen_mat(rng, Mt::Banded, nb, nb, bw, false)
            }
            _ => {
                let mb = if a.mtp == Mt::Full || rng.chance(0.5) {
                    gen_size(rng)
                } else {
                    nb
                };
                gen_mat(rng, Mt::Full, nb, mb, 0, false)
            }
        };
        let c = gen_out(rng, a.n, b.m);
        Inputs::Multiply { a, b, c }
    } else if r < 0.49 {
        let a = gen_any_mat(rng, false);
        let in_place = rng.chance(0.4);
        if in_place && a.mtp == Mt::Banded && a.bw > a.n + 1 {
            let n = a.n;
            let bw = 2 * (n / 2) + 1;
            let a = gen_mat(rng, Mt::Banded, n, n, bw, false);
            return Inputs::Transpose {
                a,
                in_place,
                b: None,
            };
        }
        let b = if in_place || a.mtp == Mt::Banded {
            None
        } else {
            gen_out(rng, a.m, a.n)
        };
        Inputs::Transpose { a, in_place, b }
    } else if r < 0.55 {
        let a = gen_any_mat(rng, false);
        let np = a.n.max(a.m) + 1 + rng.below(2) as usize;
        if rng.chance(0.5) {
            Inputs::MulPts {
                pts: (0..np).map(|_| gen_pt(rng)).collect(),
                a,
            }
        } else {
            Inputs::MulCpts {
                pts: (0..np).map(|_| gen_cpt(rng)).collect(),
                a,
            }
        }
    } else if r < 0.63 {
        let pivot = rng.chance(0.5);
        let banded = !pivot || rng.chance(0.1);
        Inputs::Lu {
            a: gen_square(rng, banded),
            pivot,
        }
    } else if r < 0.7 {
        let a = gen_square(rng, true);
        let decompose = rng.chance(0.8);
        let len = a.n + 1 + rng.below(2) as usize;
        if rng.chance(0.5) {
            Inputs::ForBackPts {
                a,
                decompose,
                rhs: (0..len).map(|_| gen_pt(rng)).collect(),
            }
        } else {
            Inputs::ForBackCpts {
                a,
                decompose,
                rhs: (0..len).map(|_| gen_cpt(rng)).collect(),
            }
        }
    } else if r < 0.75 {
        let a = gen_square(rng, true);
        let len = a.n + 1 + rng.below(2) as usize;
        let y = gen_vec(rng, len);
        Inputs::RightForBack {
            decompose: rng.chance(0.8),
            a,
            y,
        }
    } else if r < 0.8 {
        let a = gen_square(rng, false);
        let b = gen_vec(rng, a.n + 1);
        Inputs::SolvePivot { a, b }
    } else if r < 0.85 {
        let a = gen_square(rng, true);
        let b = gen_out(rng, a.n, a.n);
        Inputs::Inverse {
            lud: rng.chance(0.15),
            a,
            b,
        }
    } else if r < 0.89 {
        let m = rng.below(5) as usize;
        let n = if rng.chance(0.9) {
            m + 1 + rng.below(6) as usize
        } else {
            m
        };
        let a = gen_mat(rng, Mt::Full, n, m, 0, false);
        let rhs = gen_vec(rng, n + 1);
        Inputs::Lstsq { a, rhs }
    } else if r < 0.93 {
        let rows = 1 + rng.below(6) as usize;
        let cols = 1 + rng.below(6) as usize;
        let rational = rng.chance(0.5);
        let net = gen_net(rng, rows, cols, rational);
        Inputs::SrfEval {
            net,
            n: rng.below(rows as u64) as usize,
            m: rng.below(cols as u64) as usize,
            u: gen_param(rng),
            v: gen_param(rng),
        }
    } else if r < 0.97 {
        gen_srf_derivs(rng)
    } else if r < 0.985 {
        let kind = *rng
            .choose(&[Item1::Flag, Item1::Real, Item1::Pt, Item1::Cpt])
            .unwrap_or(&Item1::Real);
        let width = match kind {
            Item1::Flag | Item1::Real => 1,
            Item1::Pt => 3,
            Item1::Cpt => 4,
        };
        let n = rng.below(8) as usize;
        let m = rng.below(8) as usize;
        let len = n.min(m) + 1 + rng.below(3) as usize;
        Inputs::Realloc1d {
            items: gen_items(rng, len, width, kind == Item1::Flag),
            kind,
            n,
            m,
        }
    } else {
        let kind = if rng.chance(0.5) {
            Item2::Real
        } else {
            Item2::Cpt
        };
        let width = if kind == Item2::Real { 1 } else { 4 };
        let (n, m, k, l) = (
            rng.below(5) as usize,
            rng.below(5) as usize,
            rng.below(5) as usize,
            rng.below(5) as usize,
        );
        let rows = k.min(n) + 1 + rng.below(2) as usize;
        let cols = l.min(m) + 1 + rng.below(2) as usize;
        let items = (0..rows)
            .map(|_| gen_items(rng, cols, width, false))
            .collect();
        Inputs::Realloc2d {
            kind,
            items,
            n,
            m,
            k,
            l,
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.math"
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
