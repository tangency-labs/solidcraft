#### WR-10. `smgu_BestFitLineToPoints` takes a fixed eigenvector, not the best-fit direction
- **Severity:** Wrong result
- **Area:** Geometry and tolerances
- **Location:** `SmGeomUtility.cpp:4223`, `smgu_BestFitLineToPoints`. `smgu_BestFitPlaneToPoints` uses the same vector as a plane normal.
- **What happens:** It takes `aEigenVectors[2]` of `C = del*I - M`. The comment calls this the largest eigenvalue's vector, but the least-squares direction minimises `v^T C v`. In practice the result depends on the Jacobi solver and the sort.
- **Trigger:** Axis-aligned collinear points give a direction perpendicular to them (classified `SM_PST_SCATTERED`). Points along `(1,1,1)` give the right direction.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.
