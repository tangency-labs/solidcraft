#### UB-114. `SmPrimitiveCreation::CreateCurveSweep` crashes on a profile without a B-spline curve
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmPrimitiveCreation.cpp:4882` `SmPrimitiveCreation::CreateCurveSweep`: profile entries that are not `SmBSplineCurve` (including `NULL`) are skipped, then `sNurbs[0]` is read at `:5054` (profile reference point when `bMoveProfileCenterToPath` is false) without checking that `sNurbs` is non-empty; reached from `SmApiCreateCurveSweep` (`source/SM_API/src/SmApiBrep.cpp:1475`), which only checks that the profile list is non-empty.
- **What happens:** Segfault.
- **Trigger:** `SmApiCreateCurveSweep` with profile `[NULL]` or `[SmHermiteCurve(..)]`, path `SmApiCreateCurve((0,0,0),(0,0,3),(0,0,7),(0,0,10))`, no scale curves and `NULL` options (or options with `bMoveProfileCenterToPath = FALSE`). With `bMoveProfileCenterToPath = TRUE` the same profiles return without crashing (all 32 combinations of the other flags and scaling probed).
- **How found:** adversarial review of `sweep.api_brep` (PR #110).
- **Confirmed:** yes (segfault in `CreateCurveSweep` under gdb).
- **Severity:** crash.
- **Rust port:** `create_curve_sweep` is a stub until `primitive.primitive_creation_p3` lands, whose porter must return an error instead. The FFI rejects a curve-sweep profile without a B-spline curve unless the options move the profile center to the path.
