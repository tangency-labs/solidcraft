#### HANG-13. `SmApiCreateLinearSweep` of a full circle over a negative distance never returns
- **Severity:** Hang
- **Area:**
- **Location:** `source/SM_API/src/SmApiBrep.cpp:1196` `SmApiCreateLinearSweep` (and `SmApiCreateLinearSweepWithRepetitions`, `:1685`) -> `source/SMLib/src/SmPrimitiveCreation.cpp:1795` `SmPrimitiveCreation::CreateLinearSweep` -> (one repetition) `:1150` `CreateLinearSweep`, which builds the analytic domain `SmExtent2d(0, 0, 1, dSweepDist)` without ordering it.
- **What happens:** The C++ reference run did not finish within 5 minutes. Which loop spins was not located. The same sweep with a positive distance, or with the direction reversed instead of the distance, finishes at once; arcs short of 360 degrees, lines and line rectangles with a negative distance also finish.
- **Trigger:** `SmApiCreateLinearSweep` of one `SmApiCreateCircle` circle (B-spline or `SmCircle`, e.g. radius `5` at the origin) along `(0, 0, 1)` over distance `-10`, capped or not; `SmApiCreateLinearSweepWithRepetitions` with one repetition likewise.
- **How found:** generated difftest case (`sweep.api_brep`), then adversarial probes.
- **Confirmed:** yes (the C++ reference run did not finish).
- **Severity:** hang.
- **Rust port:** The wrappers are stubs until `primitive.primitive_creation_p3` lands. The FFI rejects a negative distance with a full circle (B-spline, analytic, or an arc spanning 360 degrees); the generator sweeps circles over positive distances.
