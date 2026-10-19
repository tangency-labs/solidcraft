//! FFI for `query.face_p1`. Shim: `harness/cpp/shim/query_face_p1.cpp`.
//!
//! [`evaluate_curve`]: `SmApiEvaluateCurve` on a real null curve (kind 0), `SmLine` (1) or
//! `SmHermiteCurve` (2); `data` is `(p1, d1, p2, d2)`, each xyz (lines use p1 and p2).
//!
//! [`eval`]: an [`Op`] on a real `SmBrep` built from unit `topology.edgeuse_p1`'s B-rep
//! block (`path` names the `.smb` file of a file block). The block is validated and
//! built once through that unit's wrapper (its `Export`) before the op runs; the op
//! arguments are validated here (layouts in the shim).

use crate::{call_with_buffer, check, FfiError};

mod sys {
    extern "C" {
        pub fn sc_query_face_p1_curve(
            kind: i32,
            data: *const f64,
            parameter: f64,
            mask: i32,
            out: *mut f64,
            operation_status: *mut i32,
        ) -> i32;
    }
}

/// Call the real API, retaining seeded outputs where optional pointers are null
/// or evaluation fails. `mask` bits 0, 1 and 2 request point, d1 and d2.
///
/// Geometry must be finite; parameter NaN/infinity is deliberately admitted so
/// the API's own natural-domain rejection is compared.
pub fn evaluate_curve(
    kind: i32,
    data: &[f64; 12],
    parameter: f64,
    mask: i32,
) -> Result<(i32, [f64; 9]), FfiError> {
    if !(0..=2).contains(&kind) || !(0..=7).contains(&mask) || data.iter().any(|x| !x.is_finite()) {
        return Err(FfiError::InvalidArgument(
            "curve construction or output mask".into(),
        ));
    }
    let mut values = [0.0; 9];
    let mut status = 0;
    // SAFETY: fixed arrays provide exactly 12 reads and 9 writes; kind and mask
    // are checked and all borrowed buffers remain live throughout the call.
    let result = unsafe {
        sys::sc_query_face_p1_curve(
            kind,
            data.as_ptr(),
            parameter,
            mask,
            values.as_mut_ptr(),
            &raw mut status,
        )
    };
    check(result)?;
    Ok((status, values))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_construction_and_masks_fail_before_cpp() {
        let data = [0.0; 12];
        for kind in [-1, 3, i32::MAX] {
            assert!(matches!(
                evaluate_curve(kind, &data, 0.5, 7),
                Err(FfiError::InvalidArgument(_))
            ));
        }
        for mask in [-1, 8, i32::MAX] {
            assert!(matches!(
                evaluate_curve(1, &data, 0.5, mask),
                Err(FfiError::InvalidArgument(_))
            ));
        }
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut data = data;
            data[0] = bad;
            assert!(matches!(
                evaluate_curve(1, &data, 0.5, 7),
                Err(FfiError::InvalidArgument(_))
            ));
        }
    }
}

/// The B-rep ops of [`eval`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// Each face's protected primary face use `m_pFU` (face-use index, `-1` none).
    FaceFu = 0,
    /// `SmFace` / `SmShell` / `SmRegion::CalculateBoundingBox`.
    Box = 1,
    /// `SmFace::Point3DClassify`.
    Point3d = 2,
    /// `SmFace::EdgeCurveOnClassify`.
    EdgeOn = 3,
}

mod eval_sys {
    extern "C" {
        pub fn sc_query_face_p1_eval(
            op: i32,
            path: *const std::ffi::c_char,
            block: *const f64,
            n_block: i64,
            args: *const f64,
            n_args: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

fn bad(what: &str) -> FfiError {
    FfiError::InvalidArgument(what.into())
}

/// A selector: a non-negative integer below `2^31` (`-1` too when `null_ok`).
fn selector(x: f64, null_ok: bool) -> Result<(), FfiError> {
    let lo = if null_ok { -1.0 } else { 0.0 };
    if x.fract() == 0.0 && x >= lo && x < 2_147_483_648.0 {
        Ok(())
    } else {
        Err(bad("selector"))
    }
}

fn flag(x: f64) -> Result<(), FfiError> {
    if x == 0.0 || x == 1.0 {
        Ok(())
    } else {
        Err(bad("flag"))
    }
}

fn finite(v: &[f64]) -> Result<(), FfiError> {
    if v.iter().all(|x| x.is_finite()) {
        Ok(())
    } else {
        Err(bad("non-finite argument"))
    }
}

fn tolerance(x: f64) -> Result<(), FfiError> {
    if x.is_finite() && x >= 0.0 {
        Ok(())
    } else {
        Err(bad("tolerance"))
    }
}

fn at(args: &[f64], i: usize) -> Result<f64, FfiError> {
    args.get(i)
        .copied()
        .ok_or_else(|| bad("arguments too short"))
}

fn check_args(op: Op, args: &[f64]) -> Result<(), FfiError> {
    let want = match op {
        Op::FaceFu => 0,
        Op::Box => {
            let target = at(args, 0)?;
            if !(target == 0.0 || target == 1.0 || target == 2.0) {
                return Err(bad("box target"));
            }
            selector(at(args, 1)?, false)?;
            flag(at(args, 2)?)?;
            flag(at(args, 3)?)?;
            4
        }
        Op::Point3d => {
            // face P[3] src_tol do_bnd has_guess gu gv tgt_only has_loops n loop* uv_ok pc_tol
            // Stricter than needed: the C++ also survives non-finite points/guesses and
            // negative tolerances. Every Point3DClassify case defers on the Rust side
            // today; narrow these checks when they start comparing (AGENTS.md §4 5.6).
            selector(at(args, 0)?, false)?;
            finite(args.get(1..4).ok_or_else(|| bad("arguments too short"))?)?;
            tolerance(at(args, 4)?)?;
            flag(at(args, 5)?)?;
            flag(at(args, 6)?)?;
            finite(&[at(args, 7)?, at(args, 8)?])?;
            flag(at(args, 9)?)?;
            flag(at(args, 10)?)?;
            let n = at(args, 11)?;
            if !(n.fract() == 0.0 && (0.0..=64.0).contains(&n)) {
                return Err(bad("loop count"));
            }
            let n = n as usize;
            for i in 0..n {
                selector(at(args, 12 + i)?, false)?;
            }
            flag(at(args, 12 + n)?)?;
            tolerance(at(args, 13 + n)?)?;
            14 + n
        }
        Op::EdgeOn => {
            // face edge dim P[3] Q[3] edge_param i0 i1 orient tol uv_ok
            selector(at(args, 0)?, false)?;
            selector(at(args, 1)?, true)?;
            let dim = at(args, 2)?;
            if !(dim == 2.0 || dim == 3.0) {
                return Err(bad("curve dimension"));
            }
            finite(args.get(3..12).ok_or_else(|| bad("arguments too short"))?)?;
            flag(at(args, 12)?)?;
            tolerance(at(args, 13)?)?;
            flag(at(args, 14)?)?;
            15
        }
    };
    if args.len() == want {
        Ok(())
    } else {
        Err(bad("argument count"))
    }
}

/// Runs `op` with `args` on the B-rep of `block` (see the module docs). An op on a
/// kind without entities outputs `[-2]`.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a block or arguments the wrapper rejects or a
/// B-rep that cannot be built; the shim's status.
pub fn eval(op: Op, path: &str, block: &[f64], args: &[f64]) -> Result<Vec<f64>, FfiError> {
    use super::topology_edgeuse_p1 as graph;
    check_args(op, args)?;
    let export = graph::eval(graph::Op::Export, path, block)?;
    if graph::counts(&export).is_none() {
        return Err(bad("native fixture construction failed"));
    }
    let path = std::ffi::CString::new(path).map_err(|_| bad("path contains NUL"))?;
    let n_block = i64::try_from(block.len()).map_err(|_| bad("block too large"))?;
    let n_args = i64::try_from(args.len()).map_err(|_| bad("arguments too large"))?;
    call_with_buffer(64, |out, len| {
        // SAFETY: the block was validated (and built) by the graph wrapper and the
        // arguments above; all buffers and the C string stay live for the call, and
        // the shim honours `cap`.
        unsafe {
            eval_sys::sc_query_face_p1_eval(
                op as i32,
                path.as_ptr(),
                block.as_ptr(),
                n_block,
                args.as_ptr(),
                n_args,
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })
}

#[cfg(test)]
mod eval_tests {
    use super::*;

    #[test]
    fn malformed_arguments_fail_before_cpp() {
        assert!(check_args(Op::FaceFu, &[]).is_ok());
        assert!(check_args(Op::FaceFu, &[1.0]).is_err());
        assert!(check_args(Op::Box, &[3.0, 0.0, 0.0, 0.0]).is_err());
        assert!(check_args(Op::Box, &[0.0, -1.0, 0.0, 0.0]).is_err());
        assert!(check_args(Op::Box, &[0.0, 0.5, 0.0, 0.0]).is_err());
        assert!(check_args(Op::Box, &[2.0, 7.0, 1.0, 0.0]).is_ok());
        let mut p = vec![
            0.0, 1.0, 2.0, 3.0, 1e-6, 0.0, 1.0, 0.5, 0.5, 1.0, 1.0, 1.0, 4.0, 1.0, 0.0,
        ];
        assert!(check_args(Op::Point3d, &p).is_ok());
        p[2] = f64::NAN;
        assert!(check_args(Op::Point3d, &p).is_err());
        p[2] = 2.0;
        p[11] = 65.0;
        assert!(check_args(Op::Point3d, &p).is_err());
        let mut e = vec![
            0.0, -1.0, 3.0, 0., 0., 0., 1., 1., 1., 0.5, 0.0, 1.0, 0.0, 1e-6, 1.0,
        ];
        assert!(check_args(Op::EdgeOn, &e).is_ok());
        e[2] = 4.0;
        assert!(check_args(Op::EdgeOn, &e).is_err());
        e[2] = 2.0;
        e[1] = -2.0;
        assert!(check_args(Op::EdgeOn, &e).is_err());
    }
}
