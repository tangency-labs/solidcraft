//! Safe wrapper of `SmApiIntersectCurves`, `SmApiIntersectSurfaces`, `SmApiIntersectFaces`,
//! `SmApiIntersectBreps`, `SmApiIntersectCurveSurface`, `SmApiIntersectCurveFace` and
//! `SmApiIntersectCurveBrep` for `intersect.api_intersectors`.
//!
//! Shim: `harness/cpp/shim/intersect_api_intersectors.cpp`. Upstream:
//! `SmApiIntersectors.cpp`. The shim builds the operands itself from an argument stream
//! (`SM_API` curve, surface and primitive constructors, or
//! `SmBSplineSurface::CreateCanonical` over a control net) and runs the call in a fresh
//! child process.
//!
//! The stream starts after the op. A curve is `1 n p*` (`SmApiCreateCurve` of `n` points,
//! `4..=64`), `2 a b` (`SmApiCreateLineSegment`), `3 c r` (`SmApiCreateCircle`, the
//! `SmBSplineCurve` overload) or `4 c r start end` (`SmApiCreateArc`). A surface is
//! `1 nu nv du dv p*` (`CreateCanonical` of an `nu x nv` net, `2..=16` each, of degrees
//! `du < nu`, `dv < nv` in `1..=3`, distinct knots `i / (n - d)` with end multiplicities
//! `d + 1`), `2 o r h` (`SmApiCreateCylinder`, the `SmSurface` overload),
//! `3 curve-points o axis angle` (`SmApiCreateSurfaceRevolution` of an `SmApiCreateCurve`),
//! `4 curve-points curve-points` (`SmApiCreateRuledSurface`), where `curve-points` is
//! `n p*`, `5 p p p p` (`SmApiCreateSurfaceFromCornerPoints`) or `6 type data has_domain
//! [umin vmin umax vmax]`, an analytic surface's `CreateCanonical` then, with `has_domain`
//! 1, `AdjustSTEPUVDomain`: type `1 frame` (`SmPlane`), `2 frame r` (`SmCylinder`),
//! `3 frame rbot rtop h` (`SmCone`), `4 frame r` (`SmSphere`), with `frame` = `o x y`
//! (`SmAxis2Placement::SetCanonical`), `5 curve-points vector` (`SmSurfOfExtrusion`) or
//! `6 curve-points axis-point axis-dir` (`SmSurfOfRevolution`). A body is `1 o l w h`
//! (`SmApiCreateBox`), `2 o r` (`SmApiCreateSphere`),
//! `3 o r h` (`SmApiCreateCylinder`, the `SmBrep` overload) or `4 o l w h co r ch`
//! (`SmApiBooleanDifference` of that box and cylinder). Points are `x y z`.
//!
//! Op 1 (`want_points same curve1 [curve2]`) returns `status n (t1 t2 [x y z])*`; op 2
//! (`surface1 surface2`) returns `status n curve*` (also on a failed status: what the call
//! left in the array), or without calling it `-998 0` for the coincident cylinders whose
//! intersection dereferences the absent tolerance (UB-123) and `-997 0` for the extrusion
//! pairs whose `sm_IntersectGenerators` dereferences the absent curve-type array or an
//! unset cylinder generator (UB-81, UB-93); op 3
//! (`want_points one body1 [body2] face1 face2`, `GetFaces` indices) and op 4
//! (`want_points same body1 [body2]`) return `status n curve* npoints point*` (`npoints`
//! `-1` when not asked for). A curve is `type lo hi` and five `status x y z` samples. On
//! a failed status op 1 returns `status 0`; ops 3 and 4 return what the call left
//! in the arrays (empty for `SmApiIntersectFaces`, which deletes them; the partial
//! results of the earlier face pairs for `SmApiIntersectBreps`). Ops 5 (`curve surface`),
//! 6 (`curve body face`, a `GetFaces` index) and 7 (`curve body`) return
//! `status n curve* npoints point*`, empty on a failed status (the calls remove their own
//! outputs).
//! A failed operand build or a face index out of range returns `-999 status`.

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_intersect_api_intersectors_eval(
            op: i32,
            args: *const f64,
            na: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Runs op `op` (1 curves, 2 surfaces, 3 faces, 4 B-reps, 5 curve/surface, 6 curve/face,
/// 7 curve/B-rep) on the operands the argument stream `args` describes, in a fresh child
/// process.
///
/// Guards: `op` in `1..=7`; every argument finite and at most `1e6` in magnitude; at most
/// 4096 arguments. The shim refuses (`SC_ERR_ARG`) a stream it cannot parse completely.
pub fn eval(op: u8, args: &[f64]) -> Result<Vec<f64>, FfiError> {
    let bad = |m: &str| FfiError::InvalidArgument(m.into());
    if !(1..=7).contains(&op) {
        return Err(bad("op"));
    }
    if args.len() > 4096 || !args.iter().all(|x| x.is_finite() && x.abs() <= 1e6) {
        return Err(bad("argument"));
    }
    let args = args.to_vec();
    crate::isolate::fresh_process(move || {
        call_with_buffer(1 << 20, |out, len| {
            // SAFETY: the arguments stay live through the call; the output and length
            // pointers come from call_with_buffer.
            unsafe {
                sys::sc_intersect_api_intersectors_eval(
                    i32::from(op),
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
