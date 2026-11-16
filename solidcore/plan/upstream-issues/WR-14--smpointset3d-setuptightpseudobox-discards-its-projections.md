#### WR-14. `SmPointSet3d::SetUpTightPseudoBox` discards its projections
- **Severity:** Wrong result
- **Area:** Geometry and tolerances
- **Location:** `SmPointSet.cpp:631`, `SmPointSet3d::SetUpTightPseudoBox`.
- **What happens:** `sBegPt0.ProjectPointToPlane(..);` is `const` and returns the projection, which is discarded. Angles are measured between unprojected points.
- **Trigger:** A slightly non-planar point set: the basis differs in the last bits from a projecting implementation.
- **How found:** generated difftest case (a projecting port differed at ~1e-15).
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Ported as is.
