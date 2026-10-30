//! Safe native fixtures and `SmBrep::Mirror` for `topology.brep_mirror`.
//!
//! Shim: `harness/cpp/shim/topology_brep_mirror.cpp`. Upstream: `SmBrep.cpp`.
//! The B-rep block is the shared `topology.edgeuse_p1` block (an upstream `.smb` file,
//! a wire, a face or the wire network). Op 0 exports the fixture: `0 graph`. Op 1 runs
//! `Mirror` and returns `record_len record* 0 graph`, the record being the status, then
//! samples of every edge curve, its edge uses' UV trim curves and every face surface
//! (type, natural interval or domain, five or nine evaluated points) and the raw graph
//! of the mirrored B-rep. A failed fixture build returns `-999 status`. Op 3 ([`transform`])
//! runs `SmBrep::Transform(const double[4][4], SmBoolean)` instead, with the same output.

use super::topology_edgeuse_p1::{brep_block, Reader};
use crate::{call_with_buffer, FfiError};
use std::ffi::CString;

mod sys {
    extern "C" {
        pub fn sc_topology_brep_mirror_eval(
            op: i32,
            path: *const std::ffi::c_char,
            input: *const f64,
            n: i64,
            args: *const f64,
            na: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Op 0 exports the fixture graph, op 1 runs `Mirror`, op 2 exports the native face and
/// vertex lists (`nf face* nv vertex*`, indices of the graph).
///
/// `block`: the B-rep block. `args`: editing flag (0 or 1), then the mirror
/// placement's origin, X axis and Y axis (the plane is its XY plane). Every number must
/// be finite, so the placement is whatever `SmAxis2Placement`'s constructor makes of
/// the axes (it defines a result for zero or parallel axes).
pub fn eval(op: u8, path: &str, block: &[f64], args: &[f64; 10]) -> Result<Vec<f64>, FfiError> {
    let bad = |m: &str| FfiError::InvalidArgument(m.into());
    if op > 2 || !args.iter().all(|x| x.is_finite()) || !matches!(args[0], 0.0 | 1.0) {
        return Err(bad("mirror placement or editing flag"));
    }
    run(op, path, block, args)
}

/// Op 3: `SmBrep::Transform(const double[4][4], bScaleTol)` on the fixture, output as
/// op 1 of [`eval`].
///
/// `args`: editing flag, `bScaleTol` (each 0 or 1), then the 4x4 matrix row-major (the
/// 3x3 block is rows 0..3 / columns 0..3, the translation row 3). Every 3x3 element must
/// be zero or of magnitude in `[1e-100, 1e100]`, as [`super::geom_matrix::Op::Svd`]
/// requires (a NaN or infinite norm makes `SVDecompose` read `rW[-1]`), and the
/// translation finite.
pub fn transform(path: &str, block: &[f64], args: &[f64; 18]) -> Result<Vec<f64>, FfiError> {
    let flag = |x: f64| matches!(x, 0.0 | 1.0);
    let block3 = args
        .get(2..14)
        .is_some_and(|m| m.chunks(4).all(|row| row.iter().take(3).all(|&x| super::geom_matrix::svd_element_ok(x))));
    if !args.iter().all(|x| x.is_finite()) || !flag(args[0]) || !flag(args[1]) || !block3 {
        return Err(FfiError::InvalidArgument(
            "transform matrix or editing / scale-tolerance flag".into(),
        ));
    }
    run(3, path, block, args)
}

fn run(op: u8, path: &str, block: &[f64], args: &[f64]) -> Result<Vec<f64>, FfiError> {
    let bad = |m: &str| FfiError::InvalidArgument(m.into());
    let mut rd = Reader {
        input: block,
        at: 0,
    };
    brep_block(&mut rd, path)?;
    rd.done()?;
    let path = CString::new(path).map_err(|_| bad("path"))?;
    call_with_buffer(1 << 20, |out, len| {
        // SAFETY: the validated block and the fixed-length arguments stay live through
        // the call; the output and length pointers come from call_with_buffer.
        unsafe {
            sys::sc_topology_brep_mirror_eval(
                i32::from(op),
                path.as_ptr(),
                block.as_ptr(),
                block.len() as i64,
                args.as_ptr(),
                args.len() as i64,
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })
}
