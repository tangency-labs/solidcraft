#### UB (next free). `SmSurface::...` (surface_p8) segfaults on extreme ~1e308 B-spline weights [PSIRT]
- **Severity:** Crash / undefined behaviour
- **Area:** Surface
- **Location:** `source/SMLib` surface evaluation path reached via `surface.surface_p8` difftest (extreme coordinate ~1e308 with tiny control points).
- **What happens:** The C++ reference is killed by signal 11 (segfault) on a crafted B-spline surface whose net contains ~1e308 weights and ~1e-6 control points. Rust side does not crash.
- **How found:** generated difftest case (fuzz01012, saved under `fuzz-out/reverify-1024/surface.surface_p8/`).
- **Confirmed:** yes (crash in the C++ reference run).
