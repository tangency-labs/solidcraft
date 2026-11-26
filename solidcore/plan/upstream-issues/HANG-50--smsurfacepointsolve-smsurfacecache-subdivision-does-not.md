#### HANG-50. `SmSurfacePointSolve` / `SmSurfaceCache` subdivision does not terminate on extreme-weight B-spline surfaces [PSIRT]
- **Severity:** Hang
- **Area:** Surface
- **Location:** `source/SMLib` surface point-solve / surface-cache subdivision path (reached via `surface.surface_point_solve_p3`).
- **What happens:** The C++ reference does not return within the 20 s per-case budget on a crafted B-spline surface whose net contains ~1e308 weights (the non-rational sentinel `-DBL_MAX`) and ~1e-6 control points, with `wrap = true` and a local solver. Related to issue #506 (SurfaceCache subdivision perf, 21 s Rust vs 11 s C++).
- **How found:** generated difftest case (fuzz00316, saved under `fuzz-out/reverify-1024/surface.surface_point_solve_p3/`).
- **Confirmed:** yes (C++ timeout at the 20 s budget).
