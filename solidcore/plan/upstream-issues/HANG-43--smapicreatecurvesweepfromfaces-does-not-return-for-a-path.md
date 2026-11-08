#### HANG-43. `SmApiCreateCurveSweepFromFaces` does not return for a path with a `1e300` control point
- **Severity:** Hang
- **Area:**
- **Location:** `source/SM_API/src/SmApiBrep.cpp:1530` `SmApiCreateCurveSweepFromFaces` -> `SmPrimitiveCreation::CreateCurveSweep`. The spinning loop was not located; the sweep-surface polyline approximation of HANG-5 is a candidate.
- **What happens:** No result within 20 s.
- **Trigger:** a circular face (radius `29.2` or `21.4`) swept along `SmApiCreateCurve` through four points, one with x `1e300` (options `[F,F,T,F,T]` without scale curve; `[T,F,T,T,F]` with a line scale curve).
- **How found:** generated difftest case (`difftest --fuzz 2000`, `sweep.api_brep` `fuzz01218`, `fuzz01458`; still timing out in `verify2000/sweep.api_brep/`).
- **Confirmed:** yes (timeout).
- **Severity:** hang.
- **Rust port:** `create_curve_sweep` is a stub until `primitive.primitive_creation_p3` lands; not guarded yet (open).
