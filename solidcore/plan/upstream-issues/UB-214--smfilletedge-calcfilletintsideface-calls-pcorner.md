#### UB-214. `SmFilletEdge::CalcFilletIntSideFace` calls `pCorner->IsExtendedSurfacePatch()` before its `pCorner` test
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmFilletGeom.cpp:6219` (the call) and `:6223` (`(pCorner) ? pCorner->GetExtendedSurface(pFace) : NULL`), `SmFilletEdge::CalcFilletIntSideFace`; the else branch (`:6234`) also calls `pCorner->GetFilletedVertex()` unchecked.
- **What happens:** the conditional that tests `pCorner` four lines after the dereference shows the author expected a null corner, but `pCorner->IsExtendedSurfacePatch()` (`SmFilletCorner.h:206`, an inline read of `m_bExtendedSurfacePatch`) already ran on it: a null `pCorner` reads a member through a null `this`.
- **Trigger:** `CalcFilletIntSideFace` on a fillet edge with no 3D curve whose primary edge use has a fillet geometry, with a `NULL` corner argument (the function's default; `SmFilletVertex::CalcFilletIntSideFace` callers pass the vertex's corner, which is not `NULL` in a real run).
- **How found:** line-by-line comparison while porting `blend.fillet_geom_p5` (PR #264); the shim's guard `p5_side_runs` models it.
- **Confirmed:** no (source inspection; the difftest shim refuses the input).
- **Severity:** UB (null dereference).
- **Rust port:** `FilletBrep::calc_fillet_int_side_face` returns `SM_ERR_NULL_POINTER` for the missing corner (`blend/fillet_geom/p5.rs`, the `UB-214` comment); the shim refuses (`SC_ERR_ARG`) the inputs that reach it.
