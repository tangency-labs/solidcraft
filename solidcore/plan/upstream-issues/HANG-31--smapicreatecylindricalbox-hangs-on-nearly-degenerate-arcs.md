#### HANG-31. `SmApiCreateCylindricalBox` hangs on nearly degenerate arcs and full turns
- **Severity:** Hang
- **Area:**
- **Location:** `source/SM_API/src/SmApiPrimitives.cpp:766` `SmApiCreateCylindricalBox` -> `source/SMLib/src/SmPrimitiveCreation.cpp:424` `SmPrimitiveCreation::CreateCylindricalBox`, which builds the four boundary arcs with the three-point `SmCircle(start, mid, end)` constructor.
- **What happens:** Inputs the API's validation accepts never return (over 60 s) when an arc nearly degenerates: a sweep of exactly 360 degrees or within about `1e-12` of it (start and end points coincide), a short or flat inside arc, or an origin far from the arcs. An origin of `1e300` segfaults.
- **Trigger:** angles `0 .. 360`, `-90 .. 270`, `360 .. 720`, `0 .. 360 - 1e-12` (`0 .. 360 - 1e-8` returns); inside radius `1`, angles `0 .. 0.01` (`0 .. 0.012` returns); inside radius `1e-3`, angles `10 .. 10.2`; inside radius `1e-2`, angles `0 .. 0.13`; origin `(1e17, 0, 0)` or non-finite, radii `1, 2`, angles `0 .. 90`.
- **How found:** hang in the C++ reference run (primitive.api_primitives fixtures and probes).
- **Confirmed:** yes.
- **Severity:** hang (crash for the `1e300` origin).
- **Rust port:** not reached yet (deferred); the difftest guard passes on only inputs with an inside-arc chord of at least `1e-4`, a sagitta of at least `1e-8`, a sweep at least `1e-8` degrees short of a full turn and a finite origin within `1e6`.
