#### UB-202. `SmCircularCrossSectionFSG::CreateSurfaceFromCurves` indexes past its cross sections in the second pass of a ruled or revolved surface
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmFilletStandardSolver.cpp:3233` (`SmCircularCrossSectionFSG::CreateSurfaceFromCurves`): the first pass skips to the last knot for sweeps and revolves (`:3435`), so it stores three cross sections; the second pass (`m_bApproximateArcs` with differing unique knot counts) indexes `sCrossSections[lKnotIndex]` with `lKnotIndex` the knot index (`:3504`, `:3613`, `:3636`, `:3756`), which is at least 3 for the last knot.
- **What happens:** `SmTArray::operator[]` reads (and on the replacement / mirror paths writes) past the array's size: an uninitialised or stale pointer is used as a cross section and deleted.
- **Trigger:** A degree-1 straight centre line with a curved rail (the ruled-surface test only rejects rails that are lines in another direction), approximated arcs (the default) whose sections at the first, second and last knots get different unique knot counts (different arc angles, or a point section where the rails meet).
- **How found:** read during porting.
- **Confirmed:** no. The `blend.fillet_standard_solver_p1` shim detects these inputs with a dry run of the first pass and does not run them.
- **Severity:** crash/UB (out-of-bounds read and write).
- **Rust port:** `SolidError::InvalidInput` at the out-of-range index.
