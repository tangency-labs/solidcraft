#### HANG-34. `SmBlendCurveCrossSectionFSG::CreateSurfaceFromCurves` loops forever when a knot insertion adds no knot
- **Severity:** Hang
- **Area:**
- **Location:** `source/SMLib/src/SmFilletStandardSolver.cpp:4090` (`while (sKnots.GetSize() < 4)`: the mid-span knot is inserted into both rails and the knots are re-read). `SmCircularCrossSectionFSG::CreateSurfaceFromCurves` has the same loop with a bail-out (`:3336`).
- **What happens:** When `InsertOneKnot` fails or the mid-span value equals an existing knot (spans below the knot resolution), the knot count never reaches 4.
- **Trigger:** Rails whose largest knot span is too small to split.
- **How found:** read during porting.
- **Confirmed:** no. The `blend.fillet_standard_solver_p1` shim's dry run rejects such inputs.
- **Severity:** hang.
- **Rust port:** `SolidError::InvalidInput` when an insertion adds no knot.
