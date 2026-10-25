#### UB-231. `SmSurface::CreateNaturalUVTrimCurves` segfaults on a domain `1e-300` wide
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmSurface.cpp:9048` `SmSurface::CreateNaturalUVTrimCurves` (iso curves from `CreateIsoParametricCurve`, degenerate sides replaced by `SmLine::CreateLineSegment`). The failing dereference was not located.
- **What happens:** Segfault.
- **Trigger:** a plane `(o = 0, n = (0,0,1))` with domain `[0, 0] x [1e-300, 1e-9]`, side `0`, remove degenerate curves `TRUE`; `TestFiles/pt_TestFiles/offset_nurb.sms` with domain `[-1e-300, 0] x [0, 10]`, side `0`.
- **How found:** crash in the C++ reference run (`difftest --fuzz 2000`, `surface.surface_p6` `fuzz01069`, `fuzz01289`; still crashing in `verify3/surface.surface_p6/`).
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** Not guarded yet: the `surface.surface_p6` FFI still passes these inputs (open).
