#### UB-211. `SmFilletVertex::CalcVertOnVert` dereferences a missing or non-vertex classification object
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmFilletGeom.cpp:2337-2345` (`SmFilletVertex::CalcVertOnVert`), reached from `SmFilletVertex::CalcCornerVertGeom` (`:1434`) for an `SM_FV_ON_VERTEX` vertex.
- **What happens:** `(SmVertex*)GetPointClassObject()` is used without a check: a vertex whose point classification has no object dereferences `NULL` in `pVert->GetPoint()`, and an object of another kind (an edge use or face) is cast blindly and read as a vertex. There is no `NER` as in the sibling calculators.
- **Trigger:** `CalcCornerVertGeom()` on an `SM_FV_ON_VERTEX` fillet vertex whose classification object is `NULL` or not an `SmVertex`.
- **How found:** read during porting (`blend.fillet_geom_p3`, PR #242).
- **Confirmed:** no (the difftest shim refuses such models, `SC_ERR_ARG`).
- **Severity:** crash (NULL dereference) / UB (wrong-type cast).
- **Rust port:** `FilletBrep::calc_vert_on_vert` returns `SM_ERR_NULL_POINTER` without an object and `SolidError::InvalidInput` for an object that is not a vertex.
