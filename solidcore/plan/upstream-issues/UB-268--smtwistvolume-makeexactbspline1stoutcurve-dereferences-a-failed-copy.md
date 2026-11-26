#### UB-268. `SmTwistVolume::MakeExactBSpline1stOutCurve` dereferences the NULL result of a failed `Copy`
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmTwistVolume.cpp`, `SmTwistVolume::MakeExactBSpline1stOutCurve` (def. `:419`): `rInputCurve.Copy(*pContext, (SmCurve *&) rpNewCurve)` (`:538`) ignores the status, and `rpNewCurve->Transform(sRotate)` (`:539`) runs on the `NULL` it set at `:428`.
- **What happens:** NULL pointer dereference whenever the input curve's `Copy` fails. `SmHermiteCurve` has no `Copy`; the base `SmCurve::Copy` (`source/SMLib/inc/SmCurve.h:1277`) returns `SM_ERR` without touching the output (the root cause of UB-48 too).
- **Trigger:** a curve whose `Copy` fails (an `SmHermiteCurve`) in a plane normal to the twist axis, inside the twist domain, mapped by a twist volume: directly, or through `SmVolume::MakeExactBSplineOutCurveFromInSpace` / `FromParamSpace` with a twist head map (`SmCrvInVolume::MakeExactBSplineIfPossible`).
- **How found:** crash in the C++ reference run, reported while porting `curve.crv_in_volume` (#1315, whose shim refuses a Hermite base under a twist head map).
- **Confirmed:** yes. A standalone program linking the reference build (v1.0.1, 3fd5b95, release flags) builds `SmTwistVolume::CreateCanonical(origin 0, axis +Z, mid dir +X, period 10)` and the Hermite curve `P1 (1,0,2) D1 (1,1,0) P2 (2,1,2) D2 (0,1,0)` (plane z = 2). `SmHermiteCurve::Copy` returns `SM_ERR` (1001) with a NULL copy, and `MakeExactBSpline1stOutCurve(SM_VS_IN_SPACE, ...)` then ends in SIGSEGV.
- **Rust port:** not affected (safe Rust). The port of #1315 returns the failed `Copy`'s error.
