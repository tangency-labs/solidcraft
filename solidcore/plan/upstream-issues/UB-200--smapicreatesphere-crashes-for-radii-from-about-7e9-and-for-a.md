#### UB-200. `SmApiCreateSphere` crashes for radii from about `7e9` and for a non-finite center
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SM_API/src/SmApiPrimitives.cpp:335` `SmApiCreateSphere` -> `source/SMLib/src/SmPrimitiveCreation.cpp:894` `SmPrimitiveCreation::CreateSphere`. The API accepts any finite radius up to `SM_BIG_DOUBLE / 100` (`1e18`).
- **What happens:** Segfault inside `CreateSphere`. A NaN or infinite center coordinate (not validated) also segfaults; `SmApiCreateTorus` (B-rep overload, no validation) segfaults on the same centers.
- **Trigger:** radius `7e9`, `1e10`, `1e15`, `1e18` or `-6.5e9` (`6e9`, `6.5e9` and `-6e9` are fine); center `(NaN, 0, 0)` or `(0, inf, 0)` with radius `1`.
- **How found:** crash in the C++ reference run (primitive.api_primitives fixtures and probes).
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** not reached yet (`SmPrimitiveCreation` is unported, the call is deferred); the difftest guard keeps these inputs from the C++ side.
