//! FFI wrappers for unit `geom.matrix`.
//!
//! Shim: `harness/cpp/shim/geom_matrix.cpp`.
//! Upstream: `source/SMLib/inc/SmMatrix.h`, `source/SMLib/src/SmMatrix.cpp` (`SmMatrix`).
//!
//! | Rust   | shim                | upstream                           |
//! |--------|---------------------|------------------------------------|
//! | [`op`] | `sc_geom_matrix_op` | one `SmMatrix` method per [`Op`]   |

use crate::{check, FfiError};

mod sys {
    extern "C" {
        pub fn sc_geom_matrix_op(
            op: i32,
            rows: i64,
            cols: i64,
            m: *const f64,
            v: *const f64,
            n_v: i64,
            i: i64,
            j: i64,
            s: f64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
            ints: *mut i64,
            ints_cap: i64,
            ints_len: *mut i64,
            status: *mut i32,
        ) -> i32;
    }
}

/// Largest row / column count the shim accepts.
pub const MAX_DIM: usize = 64;

/// One `SmMatrix` operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Op {
    /// Copy constructor: values = elements, ints = `[rows, cols]`.
    Copy = 0,
    /// `SetSize(i, j)`, then (new size only) `MakeIdentity()` or `Clear()` when not
    /// square: values = elements, ints = `[rows, cols]`.
    Resize = 1,
    /// `AddTo(i, j, s)`; `GetAt`; const `operator[]`; `SetAt(i, j, -s)`; non-const
    /// `operator[]`: values = the three reads, then the elements.
    Access = 2,
    /// `IsSymmetric()`: ints = `[flag]`.
    IsSymmetric = 3,
    /// `Clear()`: values = elements.
    Clear = 4,
    /// `MakeIdentity()`: values = elements.
    Identity = 5,
    /// `LUDecompose(idx)`: values = elements, ints = `idx` (on success / warning).
    Lu = 6,
    /// `LUDecompose` then `SolveWithLUMatrix(idx, v)`: values = `x`, ints =
    /// `[warning]`.
    SolveLu = 7,
    /// `SolveLinearSystem(v)`: values = `x`, ints = `[warning]`.
    Solve = 8,
    /// `EigenSystemJacobiSolver`: values = eigenvalues, eigenvectors (row-major),
    /// matrix after; ints = `[iterations]`.
    Eigen = 9,
    /// `Compute3x3EigenVectors`: values = 3 eigenvalues, 3 vectors (xyz each),
    /// matrix after. `SM_ERR` for a non-symmetric matrix (upstream output undefined).
    Eigen3 = 10,
}

/// Output of [`op`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OpResult {
    /// Upstream `SmStatus` of the operation (`0` = `SM_SUCCESS`).
    pub status: i32,
    /// Float results.
    pub values: Vec<f64>,
    /// Integer results.
    pub ints: Vec<i64>,
}

/// Runs `op` in upstream C++ on the row-major `rows x cols` matrix `m`, right-hand
/// side `v` and operands `i`, `j`, `s`.
pub fn op(
    op: Op,
    rows: usize,
    cols: usize,
    m: &[f64],
    v: &[f64],
    i: i64,
    j: i64,
    s: f64,
) -> Result<OpResult, FfiError> {
    if rows == 0 || cols == 0 || rows > MAX_DIM || cols > MAX_DIM {
        return Err(FfiError::InvalidArgument(format!("size {rows}x{cols}")));
    }
    if m.len() != rows * cols {
        return Err(FfiError::InvalidArgument("element count".into()));
    }
    if v.len() > MAX_DIM {
        return Err(FfiError::InvalidArgument("rhs length".into()));
    }
    let in_dim = |x: i64, n: usize| usize::try_from(x).is_ok_and(|x| x < n);
    let ok = match op {
        Op::Access => in_dim(i, rows) && in_dim(j, cols),
        Op::Resize => (1..=MAX_DIM as i64).contains(&i) && (1..=MAX_DIM as i64).contains(&j),
        Op::SolveLu => rows == cols && v.len() == rows,
        Op::Eigen3 => rows == 3 && cols == 3,
        _ => true,
    };
    if !ok {
        return Err(FfiError::InvalidArgument(format!(
            "operands for {op:?}: {rows}x{cols}, v {}, i {i}, j {j}",
            v.len()
        )));
    }
    // `i`, `j` only size the output for `Resize`, where they were checked above
    // (other ops leave them unchecked, so they must not size an allocation).
    let (out_rows, out_cols) = match op {
        Op::Resize => (
            rows.max(i.unsigned_abs() as usize),
            cols.max(j.unsigned_abs() as usize),
        ),
        _ => (rows, cols),
    };
    let max_out = out_rows * out_cols;
    let mut out = vec![0.0f64; 3 * max_out + rows + 8];
    let mut ints = vec![0i64; rows + 8];
    let (mut out_len, mut ints_len, mut status) = (0i64, 0i64, 0i32);
    // SAFETY: `m` is valid for rows*cols reads and `v` for v.len() reads; `out` and
    // `ints` are valid for their lengths (passed as capacities, which satisfy the
    // shim's minimum); the three scalars are valid for writes. Sizes and operands
    // were checked above as the shim requires; it retains nothing.
    let st = unsafe {
        sys::sc_geom_matrix_op(
            op as i32,
            rows as i64,
            cols as i64,
            m.as_ptr(),
            v.as_ptr(),
            v.len() as i64,
            i,
            j,
            s,
            out.as_mut_ptr(),
            out.len() as i64,
            &raw mut out_len,
            ints.as_mut_ptr(),
            ints.len() as i64,
            &raw mut ints_len,
            &raw mut status,
        )
    };
    check(st)?;
    let take = |n: i64, cap: usize| {
        usize::try_from(n)
            .ok()
            .filter(|&n| n <= cap)
            .ok_or_else(|| FfiError::InvalidArgument(format!("shim returned {n} items")))
    };
    let n_out = take(out_len, out.len())?;
    let n_ints = take(ints_len, ints.len())?;
    out.truncate(n_out);
    ints.truncate(n_ints);
    Ok(OpResult {
        status,
        values: out,
        ints,
    })
}
