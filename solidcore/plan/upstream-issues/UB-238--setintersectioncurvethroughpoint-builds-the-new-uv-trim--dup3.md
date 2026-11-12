#### UB-238. `SetIntersectionCurveThroughPoint` builds the new UV trim curves from the address of a pointer slot
- **Severity:** Crash / undefined behaviour
- **Area:** Curves
- **Location:** `source/SMLib/src/SmBSplineCurveNLib.cpp:2293` (`new SmBSplineCurve(rUVCurve[ii]->GetDim(), (gw_CURVE *)&sNewUVCrv[ii])`); `sNewUVCrv` is a `SmTArray<NL_CURVE *>` (`:1650`) filled with `N_AllocCrv` results in the knot-inserting cases (`:2018-2025`).
- **What happens:** `&sNewUVCrv[ii]` is the address of the array element holding the `NL_CURVE*`, not the curve, so the copy constructor reads a pointer and the bytes after it as a `gw_CURVE` and dereferences garbage (segfault in the reference build). The 3D curve's own copy two lines up uses `&sNewCrv` correctly.
- **Trigger:** any call with at least one UV trim curve whose parameter falls in a knot-inserting case (1, 4 or 8: a new knot before the start, inside a span away from its ends, or after the end), e.g. a piecewise C0 cubic over `[0,3]` in the plane `z = 0` with one UV curve on a bilinear plane and `dNewKnot = 0.5`.
- **How found:** crash in the C++ reference run (adversarial / edge-case fixture, `nurbs.b_spline_curve_n_lib`).
- **Confirmed:** yes (SIGSEGV running the case through the shim; the knot-moving cases 2, 3, 5, 6, 7 with the same curves run and match the port).
- **Severity:** crash/UB.
- **Rust port:** swaps in the inserted UV curves themselves (the evident intent). The `nurbs.b_spline_curve_n_lib` FFI guard only lets UV curves through when every span and node the parameter could select takes a knot-moving case.
