#### UB-113. `SmPrimitiveCreation::CreateSweepAlongPlanarPath` crashes on a curve that is not an `SmBSplineCurve`
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmPrimitiveCreation.cpp:5698` `SmPrimitiveCreation::CreateSweepAlongPlanarPath` (static `PositionProfileCurves` uses the result of `rProfileCurves[ii]->Copy(..)` without checking its status; the path curves are C-cast to `SmBSplineCurve*`); reached from `SmApiCreateSweepAlongPlanarPath` (`source/SM_API/src/SmApiBrep.cpp:1420`), which only checks the list sizes and the cap selector.
- **What happens:** A profile curve without `Copy` (base `SmCurve::Copy` returns `SM_ERR` and a `NULL` copy) is dereferenced; a path curve of another kind is used as an `SmBSplineCurve`. Both segfault.
- **Trigger:** `SmApiCreateSweepAlongPlanarPath` with profile `[SmHermiteCurve((0,0,0),(1,0,0),(1,1,0),(1,0,0))]` and path `[SmApiCreateLineSegment((0,0,0),(0,0,10))]` (move `TRUE`, caps `3`); or profile `[SmApiCreateCircle((0,0,0), 5)]` with that Hermite curve as the path.
- **How found:** adversarial review of `sweep.api_brep` (PR #110).
- **Confirmed:** yes (segfault in `CreateSweepAlongPlanarPath` / `SmBSplineCurve` copy constructor under gdb).
- **Severity:** crash.
- **Rust port:** `create_sweep_along_planar_path` is a stub until `primitive.primitive_creation_p2` lands, whose porter must return an error instead. The FFI rejects a Hermite curve in either `PlanarPath` list.
