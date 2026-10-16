//! FFI wrappers for unit `nlib.math`.
//!
//! Shim: `harness/cpp/shim/nlib_math.cpp`.
//! Upstream: `source/SMLib/src/NL_Math.cpp`, `NL_SrfPoly.cpp`, `NL_Util.cpp`.
//!
//! | Rust                 | shim                           | upstream                                   |
//! |----------------------|--------------------------------|--------------------------------------------|
//! | [`point_ops`]        | `sc_nlib_math_point_ops`       | the `N_*Pt*` / `N_*CPt*` arithmetic, `N_Swap*`, `N_CPtsAreEqual` |
//! | [`float_op`]         | `sc_nlib_math_float_op`        | `N_FloatOpIsBad`                           |
//! | [`quadratic`]        | `sc_nlib_math_quadratic`       | `N_SolveQuadraticEq`                       |
//! | [`pascal`]           | `sc_nlib_math_pascal`          | `N_PascalTriRow`                           |
//! | [`check_mem`]        | `sc_nlib_math_check_mem`       | `N_InitRealMatrix`, `N_CreateRealMatrix`, `N_RealMatrixIsNULL`, `N_CheckMemRealMatrix` (`N_SetRealMatrix`, `N_CheckRealMatStorage`, `N_RealMatrixDefine`), `N_GetRealMatrixData`, `N_GetMaxIndexRealMatrix` |
//! | [`multiply`]         | `sc_nlib_math_multiply`        | `N_RealMatrixMultiply`                     |
//! | [`transpose`]        | `sc_nlib_math_transpose`       | `N_RealMatrixTranspose`                    |
//! | [`mul_pts`]          | `sc_nlib_math_mul_pts`         | `N_RealMatrixMultiplyPtArray` / `CPtArray` |
//! | [`lu`]               | `sc_nlib_math_lu`              | `N_RealMatrixLuDecompose` / `Pivot`        |
//! | [`for_back`]         | `sc_nlib_math_for_back`        | `N_RealMatrixForBack`                      |
//! | [`right_for_back`]   | `sc_nlib_math_right_for_back`  | `N_RealMatrixRightForBack`                 |
//! | [`solve_pivot`]      | `sc_nlib_math_solve_pivot`     | `N_RealMatrixLuDecomposePivot` + `N_RealMatrixForBackPivot` |
//! | [`inverse`]          | `sc_nlib_math_inverse`         | `N_RealMatrixInverse`                      |
//! | [`lstsq`]            | `sc_nlib_math_lstsq`           | `N_RealMatrixLstSqSolve`                   |
//! | [`srf_eval`]         | `sc_nlib_math_srf_eval`        | `N_PowerBasisSrfEvalPts`, `N_SrfPowerBasisEvalPt` |
//! | [`srf_derivs`]       | `sc_nlib_math_srf_derivs`      | `N_SrfPowerBasisEvalDerivs`                |
//! | [`realloc`]          | `sc_nlib_math_realloc`         | `N_Realloc1d{Flag,Real,Pt,CPt}Array`, `N_Realloc2d{Real,CPt}Array` |
//!
//! Errors: `FfiError::Status(code)` carries the NLib error number (`NL_ERROR.eno`).
//!
//! Matrices are passed as [`Matrix`] (storage block plus logical shape). Results come
//! back as their *defined region* (the entries upstream writes for the result type,
//! row-major; see `emit_region` in the shim and [`region_len`]).
//!
//! Argument domain (checked here, before any call, so upstream only indexes inside
//! its storage): storage covers the logical shape (`rows >= n+1`, and `cols >= m+1`,
//! or `cols >= bw` for banded); triangular matrices are square; banded matrices are
//! square with an odd bandwidth; vectors and point arrays are long enough; sizes at
//! most [`MAX_DIM`]. See each function for the rest.

use crate::{call_with_buffer, check, FfiError};

/// Largest highest-index (`n`, `m`) and storage dimension accepted.
pub const MAX_DIM: usize = 64;
/// Number of input doubles of [`point_ops`].
pub const POINT_OPS_INPUTS: usize = 49;
/// `NL_MAXDEG`.
const MAX_DEGREE: usize = 17;
/// `NL_MAXDER`.
const MAX_DER: usize = 4;
/// `NL_NOW`.
const NOW: f64 = -f64::MAX;

mod sys {
    extern "C" {
        pub fn sc_nlib_math_point_ops(
            input: *const f64,
            ints: *const i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_nlib_math_float_op(x: f64, y: f64, op: i32, out: *mut i32) -> i32;
        pub fn sc_nlib_math_quadratic(a: f64, b: f64, c: f64, roots: *mut f64, n: *mut i64) -> i32;
        pub fn sc_nlib_math_pascal(row: i64, out: *mut i64, cap: i64, out_len: *mut i64) -> i32;
        pub fn sc_nlib_math_check_mem(
            data: *const f64,
            meta: *const i64,
            req: *const i64,
            out: *mut i64,
        ) -> i32;
        pub fn sc_nlib_math_multiply(
            a_data: *const f64,
            a_meta: *const i64,
            b_data: *const f64,
            b_meta: *const i64,
            c_data: *const f64,
            c_meta: *const i64,
            out_meta: *mut i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_nlib_math_transpose(
            a_data: *const f64,
            a_meta: *const i64,
            in_place: i32,
            b_data: *const f64,
            b_meta: *const i64,
            out_meta: *mut i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_nlib_math_mul_pts(
            a_data: *const f64,
            a_meta: *const i64,
            pts: *const f64,
            n_pts: i64,
            homogeneous: i32,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_nlib_math_lu(
            a_data: *const f64,
            a_meta: *const i64,
            pivot: i32,
            out_ind: *mut i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_nlib_math_for_back(
            a_data: *const f64,
            a_meta: *const i64,
            decompose: i32,
            rhs: *const f64,
            n_rhs: i64,
            ptp: i32,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_nlib_math_right_for_back(
            a_data: *const f64,
            a_meta: *const i64,
            decompose: i32,
            y: *const f64,
            n_y: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_nlib_math_solve_pivot(
            a_data: *const f64,
            a_meta: *const i64,
            b: *const f64,
            n_b: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_nlib_math_inverse(
            a_data: *const f64,
            a_meta: *const i64,
            lud: i32,
            b_data: *const f64,
            b_meta: *const i64,
            out_meta: *mut i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_nlib_math_lstsq(
            a_data: *const f64,
            a_meta: *const i64,
            rhs: *const f64,
            n_rhs: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_nlib_math_srf_eval(
            net: *const f64,
            nr: i64,
            nc: i64,
            n: i64,
            m: i64,
            u: f64,
            v: f64,
            out: *mut f64,
        ) -> i32;
        pub fn sc_nlib_math_srf_derivs(
            net: *const f64,
            nr: i64,
            nc: i64,
            p: i32,
            q: i32,
            u: f64,
            v: f64,
            mfl: i32,
            udr: i64,
            vdr: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_nlib_math_realloc(
            kind: i32,
            data: *const f64,
            len: i64,
            rows: i64,
            cols: i64,
            dims: *const i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

fn bad(msg: impl Into<String>) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

fn i64_of(v: usize) -> Result<i64, FfiError> {
    i64::try_from(v).map_err(|_| bad(format!("{v} does not fit i64")))
}

fn small(v: usize, what: &str) -> Result<(), FfiError> {
    if v > MAX_DIM {
        return Err(bad(format!("{what} = {v} > {MAX_DIM}")));
    }
    Ok(())
}

/// `NL_MT_FULL`, `NL_MT_LOWERLEFT`, `NL_MT_UPPERRIGHT`, `NL_MT_BANDED`.
pub const MT_FULL: i32 = 0;
/// `NL_MT_LOWERLEFT`.
pub const MT_LOWERLEFT: i32 = 1;
/// `NL_MT_UPPERRIGHT`.
pub const MT_UPPERRIGHT: i32 = 2;
/// `NL_MT_BANDED`.
pub const MT_BANDED: i32 = 3;

/// A matrix argument: row-major storage block `rows x cols` and the logical shape.
#[derive(Debug, Clone, Copy)]
pub struct Matrix<'a> {
    /// Storage, `rows * cols` entries.
    pub data: &'a [f64],
    /// Storage rows.
    pub rows: usize,
    /// Storage columns.
    pub cols: usize,
    /// Highest row index.
    pub n: usize,
    /// Highest column index.
    pub m: usize,
    /// Matrix type (`MT_*`).
    pub mtp: i32,
    /// Bandwidth.
    pub bw: usize,
}

impl Matrix<'_> {
    /// Checks the storage and returns the shim meta `{rows, cols, n, m, mtp, bw}`.
    fn meta(&self) -> Result<[i64; 6], FfiError> {
        small(self.rows, "rows")?;
        small(self.cols, "cols")?;
        small(self.n, "n")?;
        small(self.m, "m")?;
        small(self.bw, "bw")?;
        if self.rows == 0 || self.cols == 0 {
            return Err(bad("empty storage"));
        }
        if self.data.len() != self.rows * self.cols {
            return Err(bad(format!(
                "{} entries for {}x{} storage",
                self.data.len(),
                self.rows,
                self.cols
            )));
        }
        if self.rows < self.n + 1 {
            return Err(bad("storage rows < n + 1"));
        }
        match self.mtp {
            MT_FULL => {
                if self.cols < self.m + 1 {
                    return Err(bad("storage cols < m + 1"));
                }
            }
            MT_LOWERLEFT | MT_UPPERRIGHT => {
                if self.n != self.m || self.cols < self.m + 1 {
                    return Err(bad("triangular matrix must be square within its storage"));
                }
            }
            MT_BANDED => {
                if self.n != self.m || self.bw.is_multiple_of(2) || self.cols < self.bw {
                    return Err(bad("banded matrix must be square with odd bw <= cols"));
                }
            }
            other => return Err(bad(format!("matrix type {other}"))),
        }
        Ok([
            i64_of(self.rows)?,
            i64_of(self.cols)?,
            i64_of(self.n)?,
            i64_of(self.m)?,
            i64::from(self.mtp),
            i64_of(self.bw)?,
        ])
    }

    /// Meta for an *output* matrix that upstream may reuse: only its full storage
    /// extent matters (`rows >= n+1`, `cols >= m+1`).
    fn out_meta(&self) -> Result<[i64; 6], FfiError> {
        small(self.rows, "rows")?;
        small(self.cols, "cols")?;
        small(self.n, "n")?;
        small(self.m, "m")?;
        small(self.bw, "bw")?;
        if self.data.len() != self.rows * self.cols
            || self.rows < self.n + 1
            || self.cols < self.m + 1
        {
            return Err(bad("output storage smaller than its logical shape"));
        }
        if !(MT_FULL..=MT_BANDED).contains(&self.mtp) {
            return Err(bad(format!("matrix type {}", self.mtp)));
        }
        Ok([
            i64_of(self.rows)?,
            i64_of(self.cols)?,
            i64_of(self.n)?,
            i64_of(self.m)?,
            i64::from(self.mtp),
            i64_of(self.bw)?,
        ])
    }
}

/// A result matrix: logical shape, type, bandwidth and its defined region.
#[derive(Debug, Clone, PartialEq)]
pub struct MatrixOut {
    /// Highest row index.
    pub n: i64,
    /// Highest column index.
    pub m: i64,
    /// Matrix type.
    pub mtp: i64,
    /// Bandwidth.
    pub bw: i64,
    /// Defined region, row-major.
    pub region: Vec<f64>,
}

/// Number of entries in the defined region of an `n x m` matrix of type `mtp`
/// (mirrors `emit_region` in the shim).
#[must_use]
pub fn region_len(n: i64, m: i64, mtp: i64, bw: i64) -> usize {
    let mut len = 0usize;
    for i in 0..=n {
        let cnt = match mtp {
            0 => m + 1,
            1 => i.min(m) + 1,
            2 => (m - i + 1).max(0),
            3 => {
                let sbw = bw / 2;
                (0..bw).filter(|c| (0..=n).contains(&(i + c - sbw))).count() as i64
            }
            _ => 0,
        };
        len += usize::try_from(cnt).unwrap_or(0);
    }
    len
}

fn opt_out(m: Option<&Matrix<'_>>) -> Result<Option<[i64; 6]>, FfiError> {
    m.map(Matrix::out_meta).transpose()
}

fn ptr_or_null<T>(o: Option<&[T]>) -> *const T {
    o.map_or(core::ptr::null(), <[T]>::as_ptr)
}

/// All point / control-point arithmetic on one input record (layout in the shim);
/// returns the flat output record.
pub fn point_ops(input: &[f64], ints: [i64; 2]) -> Result<Vec<f64>, FfiError> {
    if input.len() != POINT_OPS_INPUTS {
        return Err(bad(format!("{} inputs", input.len())));
    }
    call_with_buffer(256, |buf, len| {
        // SAFETY: input has POINT_OPS_INPUTS readable doubles, ints two i64; buf is
        // valid for buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_nlib_math_point_ops(
                input.as_ptr(),
                ints.as_ptr(),
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Upstream `N_FloatOpIsBad(x, y, op)` for `op` in `1..=4`.
pub fn float_op(x: f64, y: f64, op: i32) -> Result<bool, FfiError> {
    if !(1..=4).contains(&op) {
        return Err(bad(format!("op {op}")));
    }
    let mut out = 0i32;
    // SAFETY: out is a valid i32.
    let st = unsafe { sys::sc_nlib_math_float_op(x, y, op, &raw mut out) };
    check(st)?;
    Ok(out != 0)
}

/// Upstream `N_SolveQuadraticEq(a, b, c)`: `(n, [r1, r2])` (unset roots are 0).
pub fn quadratic(a: f64, b: f64, c: f64) -> Result<(i64, [f64; 2]), FfiError> {
    let mut roots = [0.0f64; 2];
    let mut n = -1i64;
    // SAFETY: roots has two writable doubles; n is a valid i64.
    let st = unsafe { sys::sc_nlib_math_quadratic(a, b, c, roots.as_mut_ptr(), &raw mut n) };
    check(st)?;
    Ok((n, roots))
}

/// Upstream `N_PascalTriRow(tri, row)`, rows concatenated, for `row <= 66`.
pub fn pascal(row: usize) -> Result<Vec<i64>, FfiError> {
    if row > 66 {
        return Err(bad(format!("row {row} overflows long")));
    }
    let r = i64_of(row)?;
    call_with_buffer(64, |buf, len| {
        // SAFETY: buf valid for buf.len() writes; the shim honours the capacity.
        unsafe { sys::sc_nlib_math_pascal(r, buf.as_mut_ptr(), buf.len() as i64, &raw mut *len) }
    })
}

/// Upstream `N_CheckMemRealMatrix` on a null matrix (`existing = None`) or one over
/// the given storage, with request `(n, m, mtp, bw)`: returns
/// `[n, m, mtp, bw, was_null]` after the call.
pub fn check_mem(
    existing: Option<&Matrix<'_>>,
    req: (usize, usize, i32, usize),
) -> Result<[i64; 5], FfiError> {
    let meta = opt_out(existing)?;
    small(req.0, "n")?;
    small(req.1, "m")?;
    small(req.3, "bw")?;
    if !(MT_FULL..=MT_BANDED).contains(&req.2) {
        return Err(bad(format!("matrix type {}", req.2)));
    }
    let r = [
        i64_of(req.0)?,
        i64_of(req.1)?,
        i64::from(req.2),
        i64_of(req.3)?,
    ];
    let mut out = [0i64; 5];
    // SAFETY: data/meta are null or valid (meta has 6 entries, data rows*cols); upstream
    // only reads the metadata; r has 4 entries, out 5 writable.
    let st = unsafe {
        sys::sc_nlib_math_check_mem(
            ptr_or_null(existing.map(|m| m.data)),
            meta.as_ref().map_or(core::ptr::null(), |m| m.as_ptr()),
            r.as_ptr(),
            out.as_mut_ptr(),
        )
    };
    check(st)?;
    Ok(out)
}

fn call_matrix_out(
    initial: usize,
    mut f: impl FnMut(&mut [i64; 4], &mut [f64], &mut i64) -> i32,
) -> Result<MatrixOut, FfiError> {
    let mut meta = [0i64; 4];
    let region = call_with_buffer(initial, |buf, len| f(&mut meta, buf, len))?;
    Ok(MatrixOut {
        n: meta[0],
        m: meta[1],
        mtp: meta[2],
        bw: meta[3],
        region,
    })
}

/// Upstream `N_RealMatrixMultiply(a, b, c)` with `c` null (`None`) or over the given
/// storage.
pub fn multiply(
    a: &Matrix<'_>,
    b: &Matrix<'_>,
    c: Option<&Matrix<'_>>,
) -> Result<MatrixOut, FfiError> {
    let am = a.meta()?;
    let bm = b.meta()?;
    let cm = opt_out(c)?;
    call_matrix_out(64, |meta, buf, len| {
        // SAFETY: matrices validated (upstream indexes inside each storage: square
        // triangular operands, banded operands rejected by upstream before any access,
        // an existing output passes N_CheckRealMatStorage only if it holds the result);
        // buf valid for buf.len() writes; meta 4 writable i64.
        unsafe {
            sys::sc_nlib_math_multiply(
                a.data.as_ptr(),
                am.as_ptr(),
                b.data.as_ptr(),
                bm.as_ptr(),
                ptr_or_null(c.map(|m| m.data)),
                cm.as_ref().map_or(core::ptr::null(), |m| m.as_ptr()),
                meta.as_mut_ptr(),
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Upstream `N_RealMatrixTranspose`: in place, or into `b` (null when `None`).
///
/// Extra domain: an in-place banded transpose needs `bw <= n + 1` (upstream reuses
/// `(n+1)`-column storage); a banded transpose into an existing `b` needs
/// `b.cols >= bw`.
pub fn transpose(
    a: &Matrix<'_>,
    in_place: bool,
    b: Option<&Matrix<'_>>,
) -> Result<MatrixOut, FfiError> {
    let am = a.meta()?;
    if in_place && b.is_some() {
        return Err(bad("in-place transpose takes no output matrix"));
    }
    if a.mtp == MT_BANDED {
        if in_place && a.bw > a.n + 1 {
            return Err(bad("in-place banded transpose needs bw <= n + 1"));
        }
        if let Some(bb) = b {
            if bb.bw > 0 && bb.cols < a.bw {
                return Err(bad("banded transpose output narrower than bw"));
            }
        }
    }
    let bm = opt_out(b)?;
    call_matrix_out(64, |meta, buf, len| {
        // SAFETY: a validated; b null or validated; buf valid for buf.len() writes.
        unsafe {
            sys::sc_nlib_math_transpose(
                a.data.as_ptr(),
                am.as_ptr(),
                i32::from(in_place),
                ptr_or_null(b.map(|m| m.data)),
                bm.as_ref().map_or(core::ptr::null(), |m| m.as_ptr()),
                meta.as_mut_ptr(),
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Upstream `N_RealMatrixMultiplyPtArray` (`homogeneous = false`, `pts` flattened
/// xyz) or `N_RealMatrixMultiplyCPtArray` (flattened xyzw); returns `n + 1`
/// flattened results. Needs at least `max(n, m) + 1` points.
pub fn mul_pts(a: &Matrix<'_>, pts: &[f64], homogeneous: bool) -> Result<Vec<f64>, FfiError> {
    let am = a.meta()?;
    let stride = if homogeneous { 4 } else { 3 };
    if !pts.len().is_multiple_of(stride) {
        return Err(bad("point array length"));
    }
    let np = pts.len() / stride;
    if np < a.n.max(a.m) + 1 {
        return Err(bad("too few points"));
    }
    let npi = i64_of(np)?;
    call_with_buffer(64, |buf, len| {
        // SAFETY: a validated; pts has np points (upstream reads indices <= max(n, m));
        // buf valid for buf.len() writes.
        unsafe {
            sys::sc_nlib_math_mul_pts(
                a.data.as_ptr(),
                am.as_ptr(),
                pts.as_ptr(),
                npi,
                i32::from(homogeneous),
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Upstream `N_RealMatrixLuDecompose` (`pivot = false`) or
/// `N_RealMatrixLuDecomposePivot`: `(ind, region of the decomposed matrix)` (`ind`
/// is empty without pivoting).
pub fn lu(a: &Matrix<'_>, pivot: bool) -> Result<(Vec<i64>, Vec<f64>), FfiError> {
    let am = a.meta()?;
    let mut ind = vec![-1i64; a.n + 1];
    let region = call_with_buffer(64, |buf, len| {
        // SAFETY: a validated (upstream checks square/type before indexing); ind has
        // n+1 writable entries; buf valid for buf.len() writes.
        unsafe {
            sys::sc_nlib_math_lu(
                a.data.as_ptr(),
                am.as_ptr(),
                i32::from(pivot),
                ind.as_mut_ptr(),
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })?;
    if !pivot {
        ind.clear();
    }
    Ok((ind, region))
}

/// Optional `N_RealMatrixLuDecompose`, then `N_RealMatrixForBack` with Euclidean
/// (`homogeneous = false`, flattened xyz) or homogeneous right-hand sides; returns
/// `n + 1` flattened control points. Needs at least `n + 1` right-hand sides.
pub fn for_back(
    a: &Matrix<'_>,
    decompose: bool,
    rhs: &[f64],
    homogeneous: bool,
) -> Result<Vec<f64>, FfiError> {
    let am = a.meta()?;
    let stride = if homogeneous { 4 } else { 3 };
    if !rhs.len().is_multiple_of(stride) || rhs.len() / stride < a.n + 1 {
        return Err(bad("right-hand side length"));
    }
    let nr = i64_of(rhs.len() / stride)?;
    let ptp = if homogeneous { 2 } else { 1 };
    call_with_buffer(64, |buf, len| {
        // SAFETY: a validated; rhs has >= n+1 points; buf valid for buf.len() writes.
        unsafe {
            sys::sc_nlib_math_for_back(
                a.data.as_ptr(),
                am.as_ptr(),
                i32::from(decompose),
                rhs.as_ptr(),
                nr,
                ptp,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Optional `N_RealMatrixLuDecompose`, then `N_RealMatrixRightForBack(a, y)`; needs
/// at least `n + 1` entries in `y`.
pub fn right_for_back(a: &Matrix<'_>, decompose: bool, y: &[f64]) -> Result<Vec<f64>, FfiError> {
    let am = a.meta()?;
    if y.len() < a.n + 1 {
        return Err(bad("y too short"));
    }
    let ny = i64_of(y.len())?;
    call_with_buffer(64, |buf, len| {
        // SAFETY: a validated; y has >= n+1 entries; buf valid for buf.len() writes.
        unsafe {
            sys::sc_nlib_math_right_for_back(
                a.data.as_ptr(),
                am.as_ptr(),
                i32::from(decompose),
                y.as_ptr(),
                ny,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// `N_RealMatrixLuDecomposePivot` then `N_RealMatrixForBackPivot(a, ind, b)`; returns
/// the solution (all of `b`). Needs at least `n + 1` entries in `b`.
pub fn solve_pivot(a: &Matrix<'_>, b: &[f64]) -> Result<Vec<f64>, FfiError> {
    let am = a.meta()?;
    if b.len() < a.n + 1 {
        return Err(bad("b too short"));
    }
    let nb = i64_of(b.len())?;
    call_with_buffer(64, |buf, len| {
        // SAFETY: a validated; b has >= n+1 entries (ind values are <= n); buf valid.
        unsafe {
            sys::sc_nlib_math_solve_pivot(
                a.data.as_ptr(),
                am.as_ptr(),
                b.as_ptr(),
                nb,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Upstream `N_RealMatrixInverse(a, lud, b)` with `b` null or given: returns the
/// inverse and the region of `a` after the call (LU-decomposed unless `lud`).
pub fn inverse(
    a: &Matrix<'_>,
    lud: bool,
    b: Option<&Matrix<'_>>,
) -> Result<(MatrixOut, Vec<f64>), FfiError> {
    let am = a.meta()?;
    let bm = opt_out(b)?;
    let mut out = call_matrix_out(64, |meta, buf, len| {
        // SAFETY: a validated; b null or validated (reused only when it holds n x n);
        // buf valid for buf.len() writes.
        unsafe {
            sys::sc_nlib_math_inverse(
                a.data.as_ptr(),
                am.as_ptr(),
                i32::from(lud),
                ptr_or_null(b.map(|m| m.data)),
                bm.as_ref().map_or(core::ptr::null(), |m| m.as_ptr()),
                meta.as_mut_ptr(),
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })?;
    let blen = region_len(out.n, out.m, out.mtp, out.bw);
    if blen > out.region.len() {
        return Err(bad("inverse output shorter than its region"));
    }
    let a_region = out.region.split_off(blen);
    Ok((out, a_region))
}

/// Upstream `N_RealMatrixLstSqSolve(a, rhs)` for a full matrix; needs at least
/// `n + 1` right-hand-side entries. Returns `m + 1` unknowns.
pub fn lstsq(a: &Matrix<'_>, rhs: &[f64]) -> Result<Vec<f64>, FfiError> {
    let am = a.meta()?;
    if a.mtp != MT_FULL {
        return Err(bad("lstsq takes a full matrix"));
    }
    if rhs.len() < a.n + 1 {
        return Err(bad("rhs too short"));
    }
    let nr = i64_of(rhs.len())?;
    call_with_buffer(64, |buf, len| {
        // SAFETY: a validated full; rhs has >= n+1 entries; buf valid.
        unsafe {
            sys::sc_nlib_math_lstsq(
                a.data.as_ptr(),
                am.as_ptr(),
                rhs.as_ptr(),
                nr,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

fn net_dims(net: &[f64], rows: usize, cols: usize) -> Result<(i64, i64), FfiError> {
    small(rows, "rows")?;
    small(cols, "cols")?;
    if rows == 0 || cols == 0 || net.len() != rows * cols * 4 {
        return Err(bad("net shape"));
    }
    Ok((i64_of(rows)?, i64_of(cols)?))
}

/// `N_PowerBasisSrfEvalPts(net, n, m, u, v)` and `N_SrfPowerBasisEvalPt` over the
/// whole net (`rows x cols` control points, flattened xyzw): `[S1, S2]`.
pub fn srf_eval(
    net: &[f64],
    rows: usize,
    cols: usize,
    n: usize,
    m: usize,
    u: f64,
    v: f64,
) -> Result<[f64; 6], FfiError> {
    let (nr, nc) = net_dims(net, rows, cols)?;
    if n >= rows || m >= cols {
        return Err(bad("n/m outside the net"));
    }
    let mut out = [0.0f64; 6];
    // SAFETY: net has rows*cols control points; n < rows, m < cols; out has 6 slots.
    let st = unsafe {
        sys::sc_nlib_math_srf_eval(
            net.as_ptr(),
            nr,
            nc,
            i64_of(n)?,
            i64_of(m)?,
            u,
            v,
            out.as_mut_ptr(),
        )
    };
    check(st)?;
    Ok(out)
}

/// `N_SrfPowerBasisEvalDerivs` on a surface over the net (`rows x cols`, flattened
/// xyzw) with degrees `p`, `q`: `SD[0..=udr][0..=vdr]` flattened xyz.
///
/// Extra domain (unless `udr` or `vdr` exceed `NL_MAXDER`, which upstream rejects
/// first): `rows > p`, `cols > q`, and upstream's scratch array `up[NL_MAXDEG+1]`
/// must hold the u-powers and the v-power rows, `p + 1 + min(vdr, q)*(q+1) + q <=
/// NL_MAXDEG`; a rational net with `mfl` needs `udr <= vdr`.
pub fn srf_derivs(
    net: &[f64],
    rows: usize,
    cols: usize,
    p: usize,
    q: usize,
    u: f64,
    v: f64,
    mfl: bool,
    udr: usize,
    vdr: usize,
) -> Result<Vec<f64>, FfiError> {
    let (nr, nc) = net_dims(net, rows, cols)?;
    small(udr, "udr")?;
    small(vdr, "vdr")?;
    small(p, "p")?;
    small(q, "q")?;
    if udr <= MAX_DER && vdr <= MAX_DER {
        if rows <= p || cols <= q {
            return Err(bad("net smaller than the degrees"));
        }
        if p + 1 + vdr.min(q) * (q + 1) + q > MAX_DEGREE {
            return Err(bad("degrees overflow upstream's scratch array"));
        }
        let rational = net.get(3).copied().unwrap_or(NOW) != NOW;
        if rational && mfl && udr > vdr {
            return Err(bad("rational upper-half with udr > vdr reads outside SD"));
        }
    }
    let (pi, qi) = (
        i32::try_from(p).map_err(|_| bad("p"))?,
        i32::try_from(q).map_err(|_| bad("q"))?,
    );
    let (ui, vi) = (i64_of(udr)?, i64_of(vdr)?);
    call_with_buffer(3 * (udr + 1) * (vdr + 1), |buf, len| {
        // SAFETY: net has rows*cols control points and the domain above keeps upstream
        // inside its arrays; buf valid for buf.len() writes.
        unsafe {
            sys::sc_nlib_math_srf_derivs(
                net.as_ptr(),
                nr,
                nc,
                pi,
                qi,
                u,
                v,
                i32::from(mfl),
                ui,
                vi,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// `N_Realloc*`: `kind` 0 flag-1d, 1 real-1d, 2 pt-1d, 3 cpt-1d, 4 real-2d, 5 cpt-2d.
/// `data` is the old array (1d: `len` items; 2d: `rows x cols` items) with 1, 3 or 4
/// doubles per item; `dims = [n, m, k, l]` (1d uses `n`, `m`). Returns the copied
/// block (`0..=min(n,m)`, or `0..=min(k,n)` x `0..=min(l,m)`).
pub fn realloc(
    kind: i32,
    data: &[f64],
    rows: usize,
    cols: usize,
    dims: [usize; 4],
) -> Result<Vec<f64>, FfiError> {
    let item = match kind {
        0 | 1 | 4 => 1,
        2 => 3,
        3 | 5 => 4,
        _ => return Err(bad(format!("kind {kind}"))),
    };
    for d in dims {
        small(d, "dim")?;
    }
    small(rows, "rows")?;
    small(cols, "cols")?;
    let [n, m, k, l] = dims;
    let len = if kind >= 4 {
        if rows == 0 || cols == 0 || data.len() != rows * cols * item {
            return Err(bad("2d array shape"));
        }
        if rows < k.min(n) + 1 || cols < l.min(m) + 1 {
            return Err(bad("2d array smaller than the block to copy"));
        }
        rows * cols
    } else {
        if data.is_empty() || !data.len().is_multiple_of(item) {
            return Err(bad("1d array shape"));
        }
        let len = data.len() / item;
        if len < n.min(m) + 1 {
            return Err(bad("1d array shorter than the block to copy"));
        }
        len
    };
    if kind == 0
        && data
            .iter()
            .any(|f| f.fract() != 0.0 || *f < f64::from(i16::MIN) || *f > f64::from(i16::MAX))
    {
        return Err(bad("flag values must be 16-bit integers"));
    }
    let d = [i64_of(n)?, i64_of(m)?, i64_of(k)?, i64_of(l)?];
    let (li, ri, ci) = (i64_of(len)?, i64_of(rows)?, i64_of(cols)?);
    call_with_buffer(64, |buf, out_len| {
        // SAFETY: data holds `len` items (2d: rows x cols) and the copied block lies
        // inside it; dims has 4 entries; buf valid for buf.len() writes.
        unsafe {
            sys::sc_nlib_math_realloc(
                kind,
                data.as_ptr(),
                li,
                ri,
                ci,
                d.as_ptr(),
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *out_len,
            )
        }
    })
}
