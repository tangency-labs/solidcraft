Updates: WR-300

#### WR-300 (update, 2026-10-28). From the other end of the edge the held radius makes the round fail (1001), tangency-labs/solidcore#421

The record's trigger has a second symptom, which depends only on which way the variable round's edge runs.

- **Input:** SolidCraft ladder 07-fillets/012 with the box turned 180° about z onto itself before it is rounded. The 60×40×20 box gets r5 on its vertical edge at (60, 40). The top back edge then runs from the corner round's arc (x = 55) to x = 0, and gets the law 5 to 1 along it, which is the same r1 at x = 0 to r5 at x = 55.
- **What happens:** `CalcRailIntEdgeuse`'s retry again finds the arc's end at the held radius 5. Its intersection point is now the first of `CalcFilletGeom`'s trace, not the last. `DoPointIntersection` cannot start from it: it fails at that point, and its two offset-surface points stay 0.51 apart, (60, 34.636, 15) against (60, 35, 14.636). So the first pass traces nothing, the cusp recovery finds no centre line (`SM_ERR_NULL_POINTER`), and `DoFillet` / `SmApiVariableRadiusFillet` fail with `SM_ERR` (1001). With the edge running from x = 0, the same point ends the trace, and the result is the 3.8 mm³ too small body of the record.
- **Confirmed:** yes, on the C++ 1.0.1 build (3fd5b95). Stock `SmApiCreateBox`, `SmApiRotate`, `SmApiFilletEdges` and `SmApiVariableRadiusFillet` give 1001 from x = 55 and success (volume 47751.0915) from x = 0. The Rust port returns the same 1001 and success.
- **Fix:** the record's fix also covers this. With `fillet_retry_radius_as_first` and `fillet_nx1_end_rib_global_merge`, the reversed round builds a manifold solid with 8 faces and 18 edges. Its volume is the forward round's to 4e-8 relative (47754.897 against 47754.895 through SM_API). Test `retry_rail_search_holds_the_radius_from_either_end` in `crates/solidcore/tests/fillet_wr300_320.rs`. Fixes tangency-labs/solidcore#421.
