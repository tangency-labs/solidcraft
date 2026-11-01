//! Safe wrapper of the `SM_API` projection wrappers for `topology.api_projection`:
//! `SmApiProjectBrepOntoPlane`, `SmApiCreateSilhouetteCurves` and both
//! `SmApiProjectCurve`s.
//!
//! Shim: `harness/cpp/shim/topology_api_projection.cpp`. Upstream: `SmApiBrep.cpp`,
//! `SmApiCurves.cpp`. Every call runs in a fresh child process (the fillet solver of a
//! filleted box latches a static step, ND-3).
//!
//! # Output
//!
//! The status (`0` = `SM_SUCCESS`), then:
//! - Ops 1-3 (`SmApiProjectBrepOntoPlane`, `SmApiCreateSilhouetteCurves`,
//!   `SmApiProjectCurve(SmBrep*, ..)`): the output array's size, then a curve record per
//!   entry after the `prefill` null entries it started with.
//! - Op 4 (`SmApiProjectCurve(const SmBSplineCurve*, const SmSurface*, ..)`): the UV
//!   curve count and records, then the 3D ones (`-1` for an output not asked for).
//!
//! A curve record is `type lo hi` and five `status x y z` points at `t` in
//! `{0, 1/4, .., 1}` of the natural interval (`SmExtent1d::Evaluate`; `status` alone on
//! a failed evaluation). A failed body, curve or surface build returns `-999 status`.
//!
//! # Guard
//!
//! Every argument finite; the flags 0 or 1; the body `0`, `1` or `2`; `prefill` a whole
//! number in `0..=3`; the curve's point count in `4..=40` for ops 3 and 4. A file body
//! must be an upstream test B-rep file (a path under `upstream/TestFiles/pt_TestFiles`,
//! without `..`, not a part file). A box has sizes in `1e-6..=1e6` and a fillet radius
//! `0` or positive below a quarter of the smallest size. `SmApiCreateSilhouetteCurves`
//! dereferences a null B-rep once its normal unitizes: a null body with a non-zero
//! normal is refused there. Ops 1 and 2 also refuse a normal whose squared length
//! overflows ([`normal_overflows`], UB-169): it unitizes to a zero vector, from which
//! `SmApiProjectBrepOntoPlane` places its plane with the uninitialised outputs of a
//! failed `MakeUnitOrthoVectors` and `SmApiCreateSilhouetteCurves`' `SmHCR` builds its
//! view transform from uninitialised vectors.

use std::ffi::CString;
use std::path::{Component, Path};

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_topology_api_projection_eval(
            op: i32,
            path: *const std::ffi::c_char,
            args: *const f64,
            na: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// The number of fixed arguments: `body` (0 null, 1 the file, 2 box), box size (3),
/// fillet radius, `has_translate`, translation (3), point (3), normal or projection
/// vector (3), `prefill`, `want_2d`, `want_3d`, the surface corners `u0v0 u1v0 u0v1
/// u1v1` (12), the curve's point count `n`; the `3 n` point coordinates follow.
pub const NFIXED: usize = 31;

/// The guard's message for a normal [`normal_overflows`] refuses.
pub const NORMAL_OVERFLOWS: &str =
    "UB-169: the normal's squared length overflows, so it unitizes to a zero vector";

fn bad(msg: &str) -> FfiError {
    FfiError::InvalidArgument(msg.to_string())
}

/// Whether `normal`'s squared length (`SmVector3d::LengthSquared`) overflows, so that
/// `Unitize` succeeds with a zero vector (UB-169). Ops 1 and 2 refuse such a normal;
/// the difftest runner's Rust side applies the same test.
#[must_use]
pub fn normal_overflows([x, y, z]: [f64; 3]) -> bool {
    (x * x + y * y + z * z).is_infinite()
}

/// The guard of a file body: an upstream test B-rep file.
fn upstream_brep_file(path: &str) -> Result<(), FfiError> {
    let p = Path::new(path);
    if p.components().any(|c| c == Component::ParentDir) {
        return Err(bad("path with `..`"));
    }
    let canon = p.canonicalize().map_err(|e| bad(&format!("{path}: {e}")))?;
    let under = canon
        .ancestors()
        .any(|a| a.ends_with(Path::new("upstream/TestFiles/pt_TestFiles")));
    if !under {
        return Err(bad("not an upstream test file"));
    }
    let bytes = std::fs::read(&canon).map_err(|e| bad(&format!("{path}: {e}")))?;
    if bytes.starts_with(b"//[Output Summary]") {
        return Err(bad("a part file"));
    }
    Ok(())
}

/// Runs op `op` (see the module docs) on `args` and `path`, in a fresh child process.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for inputs the guard refuses; [`FfiError::Status`] for
/// a shim failure or a crashed child.
pub fn eval(op: u8, path: &str, args: &[f64]) -> Result<Vec<f64>, FfiError> {
    if !(1..=4).contains(&op) || !args.iter().all(|x| x.is_finite()) {
        return Err(bad("op or non-finite argument"));
    }
    let arg = |i: usize| args.get(i).copied().unwrap_or(f64::NAN);
    if args.len() < NFIXED {
        return Err(bad("argument count"));
    }
    let flag = |x: f64| x == 0.0 || x == 1.0;
    let body = arg(0);
    if !(body == 0.0 || body == 1.0 || body == 2.0) {
        return Err(bad("body"));
    }
    if !(flag(arg(5)) && flag(arg(16)) && flag(arg(17))) {
        return Err(bad("flag"));
    }
    let prefill = arg(15);
    if !(0.0..=3.0).contains(&prefill) || prefill.fract() != 0.0 {
        return Err(bad("prefill"));
    }
    let n = arg(30);
    if !(0.0..=40.0).contains(&n) || n.fract() != 0.0 {
        return Err(bad("point count"));
    }
    if args.len() != NFIXED + 3 * n as usize {
        return Err(bad("argument count"));
    }
    if op >= 3 && n < 4.0 {
        return Err(bad("fewer than 4 curve points"));
    }
    if op <= 3 {
        if body == 1.0 {
            upstream_brep_file(path)?;
        }
        if body == 2.0 {
            let (sx, sy, sz, fillet) = (arg(1), arg(2), arg(3), arg(4));
            let sized = |x: f64| (1e-6..=1e6).contains(&x);
            if !(sized(sx) && sized(sy) && sized(sz)) {
                return Err(bad("box size"));
            }
            if fillet < 0.0 || fillet >= 0.25 * sx.min(sy).min(sz) {
                return Err(bad("fillet radius"));
            }
        }
        let zero_normal = arg(12) == 0.0 && arg(13) == 0.0 && arg(14) == 0.0;
        if op == 2 && body == 0.0 && !zero_normal {
            return Err(bad("null B-rep dereferenced (SmApiCreateSilhouetteCurves)"));
        }
        if op <= 2 && normal_overflows([arg(12), arg(13), arg(14)]) {
            return Err(bad(NORMAL_OVERFLOWS));
        }
    }
    let path = CString::new(path).map_err(|_| bad("path has a NUL byte"))?;
    let args = args.to_vec();
    crate::isolate::fresh_process(move || {
        call_with_buffer(1 << 20, |out, len| {
            // SAFETY: the path and the arguments stay live through the call; the output
            // and length pointers come from call_with_buffer.
            unsafe {
                sys::sc_topology_api_projection_eval(
                    i32::from(op),
                    path.as_ptr(),
                    args.as_ptr(),
                    args.len() as i64,
                    out.as_mut_ptr(),
                    out.len() as i64,
                    &raw mut *len,
                )
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The overflow boundary: `x * x` is finite up to about `1.34e154`.
    #[test]
    fn only_an_overflowing_squared_length_is_refused() {
        assert!(!normal_overflows([0.0, 0.0, 0.0]));
        assert!(!normal_overflows([0.0, 0.0, 1.0]));
        assert!(!normal_overflows([1.3e154, 0.0, 0.0]));
        assert!(!normal_overflows([9e153, 9e153, 0.0]));
        assert!(normal_overflows([1.4e154, 0.0, 0.0]));
        assert!(normal_overflows([1e154, 1e154, 1e154]));
        // the nightly fuzz00010 and fuzz00002 normals
        assert!(normal_overflows([1.0, 1e300, 0.1]));
        assert!(normal_overflows([-1e300, 0.0, 1.0]));
    }

    /// Ops 1 and 2 refuse such a normal before any C++ call; ops 3 and 4 do not test it.
    #[test]
    fn projections_along_an_overflowing_normal_are_refused() {
        let mut args = vec![0.0; NFIXED];
        args[0] = 2.0; // a box
        args[1..4].copy_from_slice(&[1.0, 2.0, 3.0]);
        args[12..15].copy_from_slice(&[1.0, 1e300, 0.1]);
        for op in [1, 2] {
            let e = eval(op, "", &args).unwrap_err().to_string();
            assert!(e.contains("UB-169"), "{e}");
        }
    }
}
