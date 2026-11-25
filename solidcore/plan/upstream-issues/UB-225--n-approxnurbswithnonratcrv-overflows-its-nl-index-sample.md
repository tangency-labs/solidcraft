#### UB-225. `N_ApproxNurbsWithNonRatCrv` overflows its `NL_INDEX` sample count and writes past its parameter array
- **Severity:** Crash / undefined behaviour
- **Area:** NLib: curves, fitting, surfaces
- **Location:** `source/SMLib/src/NL_CrvApprox.cpp:2698` `N_ApproxNurbsWithNonRatCrv`: `nu[i] = (NL_INDEX)ceil(f1 * gro)` at `:2778`-`2779` (`f1 = (1/tol)^exp`, `gro = max(deg, sqrt(max|C''| / 8))`), `mu += nu[i]` at `:2780`, `N_AllocReal1dArray(mu)` at `:2785`. Reached from `SmBSplineCurveNLib.cpp:524` `SmBSplineCurve::ApproximateRationalCurve` (def. `:497`) and so from `sm_CreateNonRationalCurve` (`SmBSplineSurfaceNLib.cpp:2906`) / `SmBSplineSurface::CreateCornerBlend`.
- **What happens:** For a tiny tolerance or a huge second derivative, `del` is beyond `2^63` (or infinite), so the cast is undefined and `mu` wraps. The parameter array is allocated from the wrapped sum and the sampling loop writes past it. The heap is corrupted; the process dies at once or several calls later.
- **Trigger:** `sm_CreateNonRationalCurve` (approximation of a rational curve) on `TestFiles/pt_TestFiles/curve0.smc` with `tol = 1e-300` (segfault); a rational cubic with one control point coordinate `1e300` (glibc `realloc(): invalid next size` in the same process); `double free or corruption (out)` 50 cases later in the same process.
- **How found:** crash in the C++ reference run (`difftest --fuzz 2000`, `nurbs.b_spline_surface_n_lib_p1` `fuzz00080`, `fuzz00651`, `fuzz00902`; files under `n2000/nurbs.b_spline_surface_n_lib_p1/`).
- **Confirmed:** yes.
- **Severity:** crash/UB (heap buffer overflow).
- **Rust port:** `nurbs::crv_approx` bounds the sample count. The `nurbs.b_spline_surface_n_lib_p1` FFI applies the `nlib.crv_approx` `NonRat` bounds to a rational curve (`APPROX_MIN_TOL = 1e-7 <= tol <= 1`, homogeneous data within `MAX_COORD = 1e6`); the `nlib.crv_approx` FFI already had them.
