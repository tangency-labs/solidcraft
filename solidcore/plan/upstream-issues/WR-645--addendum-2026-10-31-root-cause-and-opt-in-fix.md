Updates: WR-645

#### WR-645 (update, 2026-10-31). Root cause in `MakeVariableRadiusFillet`, and an opt-in fix
- **Location:** `source/SMLib/src/SmFilletCorner.cpp:2982`, `SmFilletCorner::MakeVariableRadiusFillet` (def. `:2492`), called from `MakeAnalyticCornerSurface` (`:3625`) for a four-sided corner with one rail-rail interpolation edge: the new variable-radius fillet's rail on the opposite fillet replaces the opposite edge's curve (`pOppositeEdge->SetCurve(pRail1Curve)`), with no check that the rail stays on that fillet's surface (only the rail-rail curve is tested, `TestRailRailInterpolationCurve`).
- **What happens:**
  - At each top corner two convex rim rounds meet the concave corner round: a concave N x N corner of four edges (the corner round's end cross-section, two rim-round cross-sections and a rail-rail interpolation on the top face).
  - `CalcCrossSection` gives the corner round's end edge its exact arc. Traced in the port, all four corners have it after the corner edges are calculated.
  - `MakeVariableRadiusFillet` then turns the corner into a variable-radius fillet between the top face and the corner round. Its rail on the corner round replaces that end edge, and runs past the end of the round's surface (its natural domain) by up to 0.42 at R3, 10 degrees.
  - The B-rep then has the edge 0.844781 off, as the record describes. With vertical walls the rail is the end edge.
  - At the fourth corner the conversion is refused earlier, and that corner is the patch on its exact edges.
- **Rust port:** opt-in `Kernel::fixes.fillet_corner_variable_radius_rail_on_opposite_fillet` (`UpstreamFixes`, default `false`).
  - `FilletCorner::make_variable_radius_fillet_with` (`crates/solidcore/src/blend/fillet_corner/p2.rs`) drops 9 evenly spaced points of the new rail onto the opposite fillet's surface, within its natural domain. When one is more than ten times the corner's tolerance away, or a drop fails, it refuses the conversion as it refuses a crossing rail-rail curve: the new rails are deleted and `SM_ERR` is returned. The corner is then the patch.
  - With it, every corner of the record's trigger is exact (largest edge tolerance 4e-5) and the check passes, at 2, 10 and 20 degrees and at R1 and R3, with the same faces, edges and vertices (23, 52, 32). With vertical walls the body is unchanged.
  - Test: `crates/solidcore/tests/wr_645_drafted_pocket_corner_rounds.rs`. Every difftest runs with the field off.
