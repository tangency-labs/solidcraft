#### WR-59. `SmBendVolume::EvaluateBoundingBoxSimple` pseudo box does not contain the image
- **Severity:** Wrong result
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmBendVolume.cpp:1555` (pseudo box `dXMin`), `:1480` (axis box `dYMax`, unbounded-`u` branch).
- **What happens:** The pseudo box along the mid-angle direction takes its `x` minimum as `sin(thetaEnd) * uMin` (the `cos` variable on the line above is commented out). Once the half angle exceeds `pi/2` the image reaches negative `x` (down to `cos(thetaEnd) * uMax`), but the box starts at a positive value, so the "bounding" pseudo box misses part of the mapped box. Separately, with `u` reaching `SM_INFINITE_PARAMETER` and both end angles at most `0`, the axis box's `y` maximum uses the cosines of the end angles instead of the sines (too large, still containing).
- **Trigger:** Bend with neutral distance `1`, parameter box `u` in `[0.5, 1.5]`, `v` in `[-2, 2]`: pseudo `x` interval starts at `sin(2) * 0.5 = 0.45`, while the corner at `u = 1.5, v = 2` lies at `x = cos(2) * 1.5 = -0.62`.
- **How found:** read during porting.
- **Confirmed:** yes (fixtures `b_box_unit_both_poles`, `b_box_unit_inf_u_down` of `surface.bend_volume` compare the values).
- **Severity:** wrong result.
- **Rust port:** Ported as written.
