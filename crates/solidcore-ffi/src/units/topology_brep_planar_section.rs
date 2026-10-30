//! Safe wrappers of `SmBrep::CreatePlanarSectionCurves` and `SmApiIntersectBrepWithPlane`
//! for `topology.brep_planar_section`.
//!
//! Shim: `harness/cpp/shim/topology_brep_planar_section.cpp`. Upstream: `SmBrep.cpp`,
//! `SmApiIntersectors.cpp`. The shim builds the body itself (an upstream `.smb` file read
//! by `ReadFromFile`, or `SmApiCreateBox` optionally filleted by `SmApiCircularFillet`)
//! and runs the section in a fresh child process (the fillet solver latches a static
//! step, ND-3). Op 1 runs `CreatePlanarSectionCurves`:
//! `status n3d curve* nuv curve* nfaces face*` (a count of `-1` for an output not asked
//! for; a curve is `type lo hi` and five `status x y z` samples, `-1` for a null curve;
//! faces are indices into `GetFaces`). Op 2 runs `SmApiIntersectBrepWithPlane`:
//! `status n curve*`. Op 3 runs `SmSurface::CreatePlanarSectionCurves` on every face's
//! surface over the face's UV domain: `nfaces (status n3d curve* nuv curve*)*`. A failed
//! body build returns `-999 status`. Op 4 runs, on every face, the forced-UV
//! continuation's callees (`DropAndTrimCurve`, `LiftCurve`) for each section curve
//! without UV curve: `0 nfaces (status nforced (drop_status ndrops (drop lift_status
//! maxdist lifted)*)*)*`.

use crate::{call_with_buffer, FfiError};
use std::ffi::CString;

mod sys {
    extern "C" {
        pub fn sc_topology_brep_planar_section_eval(
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

/// The number of arguments: source kind, box size (3), fillet radius, plane point (3),
/// plane normal (3), `has_tol`, tol, `has_ang`, ang, `want_3d`, `want_uv`, `want_faces`,
/// `force_uv_classify`, `has_translate`, translation (3), `walk_n`, `walk_i` (the body is first
/// translated by `Transform`; with `walk_n > 1` the plane point is the B-rep box's
/// `Evaluate(t, t, t)`, `t = walk_i / (walk_n - 1)`), strut point (3) for source kind 2
/// (the box with a strut edge in its top face).
pub const NARGS: usize = 28;

/// Runs op 1 (`CreatePlanarSectionCurves`), op 2 (`SmApiIntersectBrepWithPlane`) or op 3
/// (every face surface's `CreatePlanarSectionCurves`) on the
/// body `args` and `path` describe, in a fresh child process.
///
/// Guards: every argument finite; the flags 0 or 1; the source kind 0 (file, `path`
/// non-empty) or 1 (box: sizes in `1e-6..=1e6`, the fillet radius `0` or positive and
/// below a quarter of the smallest size); for kind 2 (the strut box, unfilleted) the strut
/// point strictly inside the top face; a given tolerance not negative; a given angle
/// tolerance in `(0, PI]`; the walk a whole `walk_i < walk_n`, `walk_n` 0 (none) or at
/// least 2. A zero normal is allowed (both functions handle it).
pub fn eval(op: u8, path: &str, args: &[f64; NARGS]) -> Result<Vec<f64>, FfiError> {
    let bad = |m: &str| FfiError::InvalidArgument(m.into());
    if !(1..=4).contains(&op) || !args.iter().all(|x| x.is_finite()) {
        return Err(bad("op or non-finite argument"));
    }
    let flag = |x: f64| x == 0.0 || x == 1.0;
    if ![11, 13, 15, 16, 17, 18, 19].iter().all(|&i| args.get(i).is_some_and(|&v| flag(v))) {
        return Err(bad("flag"));
    }
    if ![0.0, 1.0, 2.0].contains(&args[0]) {
        return Err(bad("source kind"));
    }
    if args[0] == 0.0 {
        if path.is_empty() {
            return Err(bad("file source without path"));
        }
    } else {
        let sized = |x: f64| (1e-6..=1e6).contains(&x);
        if !(sized(args[1]) && sized(args[2]) && sized(args[3])) {
            return Err(bad("box size"));
        }
        let smallest = args[1].min(args[2]).min(args[3]);
        if args[4] < 0.0 || args[4] >= 0.25 * smallest {
            return Err(bad("fillet radius"));
        }
        // the strut's interior point: strictly inside the top face, unfilleted box
        let inside = |x: f64, size: f64| x > 0.01 * size && x < 0.99 * size;
        if args[0] == 2.0
            && (args[4] != 0.0
                || !inside(args[25], args[1])
                || !inside(args[26], args[2])
                || args[27] != args[3])
        {
            return Err(bad("strut point"));
        }
    }
    let (walk_n, walk_i) = (args[23], args[24]);
    let whole = |x: f64| x >= 0.0 && x.fract() == 0.0 && x <= 1e6;
    if !(whole(walk_n) && whole(walk_i)) || (walk_n > 1.0 && walk_i >= walk_n) || walk_n == 1.0
    {
        return Err(bad("plane walk"));
    }
    if args[11] == 1.0 && args[12] < 0.0 {
        return Err(bad("tolerance"));
    }
    if args[13] == 1.0 && !(args[14] > 0.0 && args[14] <= std::f64::consts::PI) {
        return Err(bad("angle tolerance"));
    }
    let path = CString::new(path).map_err(|_| bad("path"))?;
    let args = *args;
    crate::isolate::fresh_process(move || {
        call_with_buffer(1 << 20, |out, len| {
            // SAFETY: the path and the fixed-length arguments stay live through the call;
            // the output and length pointers come from call_with_buffer.
            unsafe {
                sys::sc_topology_brep_planar_section_eval(
                    i32::from(op),
                    path.as_ptr(),
                    args.as_ptr(),
                    NARGS as i64,
                    out.as_mut_ptr(),
                    out.len() as i64,
                    &raw mut *len,
                )
            }
        })
    })
}
