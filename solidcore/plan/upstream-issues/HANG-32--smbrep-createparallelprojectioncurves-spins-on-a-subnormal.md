#### HANG-32. `SmBrep::CreateParallelProjectionCurves` spins on a subnormal approximation tolerance
- **Severity:** Crash / undefined behaviour
- **Area:** Healer target management
- **Location:** `source/SMLib/src/SmBrep.cpp:4928`-`5012` (`SmBrep::CreateParallelProjectionCurves`, which calls `SmSurface::CreateParallelProjectionCurves` at `:4971`), then `SmSurfaceIntersector` / `SmSurface::LocalPointSolve`; the looping routine is not located.
- **What happens:** With `dThisApproxTol3d = 5e-324` (the smallest subnormal double), the projection runs for more than 15 minutes without returning; `1e-100` returns at once.
- **Trigger:** a caller-supplied subnormal approximation tolerance.
- **How found:** the maintainer's adversarial run in the review of #145 (at `5b1b28ae`).
- **Confirmed:** reported from the C++ reference run in that review; not re-run here (a 15-minute hang).
- **Severity:** hang. Not file-reachable (a caller tolerance argument).
- **Rust port:** unchanged. The `topology.brep_p1` difftest FFI refuses approximation tolerances below the smallest normal double (and the runner reports such cases as rejected on both sides).
