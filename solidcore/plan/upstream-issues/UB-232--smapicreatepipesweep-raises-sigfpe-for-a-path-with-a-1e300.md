#### UB-232. `SmApiCreatePipeSweep` raises `SIGFPE` for a path with a `1e300` control point
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SM_API/src/SmApiBrep.cpp:1376` `SmApiCreatePipeSweep` -> `SmPrimitiveCreation::CreatePipeSweep`. The faulting instruction was not located (an integer division or `INT_MIN / -1`).
- **What happens:** The process is killed by signal 8.
- **Trigger:** `SmApiCreatePipeSweep(radius 0.7176, path = SmApiCreateCurve((0,0,0), (3.078,0,6.097), (3.326,0,12.19), (0.516,1e300,18.29)), cap TRUE)`.
- **How found:** crash in the C++ reference run (`difftest --fuzz 2000`, `sweep.api_brep` `fuzz01378`; still crashing in `verify2000/sweep.api_brep/`).
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** The sweep wrappers are stubs until `primitive.primitive_creation_p2` lands; not guarded yet (open).
