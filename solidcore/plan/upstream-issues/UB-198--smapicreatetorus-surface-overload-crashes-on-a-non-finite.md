#### UB-198. `SmApiCreateTorus` (surface overload) crashes on a non-finite origin, an infinite radius or overflowing radii
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `source/SM_API/src/SmApiPrimitives.cpp:618` `SmApiCreateTorus` (`SmSurface*&`) -> `source/SMLib/src/SmTorus.cpp:172` `SmTorus::CreateCanonical` (only rejects radii not above `SM_EFF_ZERO`, so NaN and negative radii fail cleanly).
- **What happens:** Segfault while the torus and its NURB are built.
- **Trigger:** (only with both radii above `SM_EFF_ZERO`; otherwise `CreateCanonical` returns `SM_ERR` before it reads the origin, which the #160 review confirmed for NaN/inf origins and `+inf` radii paired with a zero, negative or NaN radius) origin `(NaN, 0, 0)` or `(0, inf, 0)` with radii `3, 1`; major or minor radius `+inf`; radii whose sum overflows (`1e308, 1e308`; `DBL_MAX, 1e293`); minor radius `DBL_MAX` with major `5` (major `DBL_MAX` with minor `1` and `DBL_MAX/2, DBL_MAX/2` are fine).
- **How found:** crash in the C++ reference run (primitive.api_primitives fixtures).
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** `Torus::create_canonical` returns without panicking; the difftest guard (`solidcore_ffi::units::primitive_api_primitives::guard`) keeps these inputs from the C++ side, and only when both radii are above `SM_EFF_ZERO` (the C++ gate), so the cleanly rejected inputs are compared.
