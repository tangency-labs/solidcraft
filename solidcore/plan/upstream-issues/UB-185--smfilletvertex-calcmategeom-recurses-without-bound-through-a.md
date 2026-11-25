#### UB-185. `SmFilletVertex::CalcMateGeom` recurses without bound through a cycle of mates
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmFilletGeom.cpp:3480-3490` (`SmFilletVertex::CalcMateGeom`: `pMainFV->CalcCornerVertGeom()`), with `CalcCornerVertGeom` (`:1434`) calling `CalcMateGeom` for an `SM_FV_MATE` vertex.
- **What happens:** A mate computes its main vertex (`GetMate(0)`) first, unprocessed, through `CalcCornerVertGeom`; when the main vertex is itself an unprocessed mate whose `GetMate(0)` leads back (a cycle of `SM_FV_MATE` vertices, including a vertex that is its own mate), the two calls recurse until the stack overflows. Nothing marks a vertex as in progress.
- **Trigger:** two unprocessed `SM_FV_MATE` fillet vertices that are each other's `GetMate(0)`.
- **How found:** read during porting (`blend.fillet_geom_p3`, PR #242).
- **Confirmed:** no (the difftest shim refuses mate cycles, `SC_ERR_ARG`).
- **Severity:** crash (stack overflow).
- **Rust port:** the nesting is bounded by `blend::fillet_geom::p3::MAX_MATE_DEPTH` (64; upstream's corners nest it at most once); deeper nesting is `SolidError::InvalidInput`.
