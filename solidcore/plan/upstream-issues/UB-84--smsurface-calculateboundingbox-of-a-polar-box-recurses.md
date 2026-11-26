#### UB-84. `SmSurface::CalculateBoundingBox` of a polar box recurses without bound
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `SmSurface.cpp:5397`, `SmSurface::CalculateBoundingBox` (base body), with `SmSurfaceCache.cpp:1184`, `SmBezierAux2d::BuildPolarBox`.
- **What happens:** For a polar box, the base body asks the top node of the surface's own cache for it (`SmBezierAux2d::GetPolarBox`). Building that box calls the node surface's `CalculateBoundingBox`, which for a curve-bounded or STEP surface is the same base body again. Nothing ends the cycle, so the stack overflows.
- **Trigger:** `CalculateBoundingBox` with `pPolarBox != NULL` on an `SmCurveBoundedSurface` or `SmSTEPSurface` (any surface kind that does not override `CalculateBoundingBox`).
- **How found:** the maintainer's review of `surface.surface_p1` (PR #43), by printing the polar box in the shim; gdb shows about 58k frames cycling through those three functions.
- **Confirmed:** yes (stack overflow in the C++ reference build).
- **Severity:** crash/UB.
- **Rust port:** `Surface::base_calculate_bounding_box` and the surface caches' `calculate_bounding_box` refuse a polar box of those kinds with `InvalidInput` (bounded on wasm). The `surface.surface_p1` FFI never asks the C++ for it.
