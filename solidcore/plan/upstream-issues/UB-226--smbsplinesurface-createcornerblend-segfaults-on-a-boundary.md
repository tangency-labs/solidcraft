#### UB-226. `SmBSplineSurface::CreateCornerBlend` segfaults on a boundary curve with a `1e300` control point
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmBSplineSurfaceNLib.cpp:3008` `SmBSplineSurface::CreateCornerBlend`. The failing dereference was not located.
- **What happens:** Segfault. The curves are non-rational, so the `sm_CreateNonRationalCurve` approximation (UB-225) is not involved.
- **Trigger:** four non-rational cubic boundary curves (bilinear blend, `tol = 1e-3`, angle `0.05`) with one control point x coordinate set to `1e300`.
- **How found:** crash in the C++ reference run (`difftest --fuzz 2000`, `nurbs.b_spline_surface_n_lib_p2` `fuzz01761`; the C++ ran in process, so the harness reported a Rust-side signal 11).
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** Returns an error. The `nurbs.b_spline_surface_n_lib_p2` FFI rejects Euclidean control point coordinates above `MAX_COORD = 1e6`.
