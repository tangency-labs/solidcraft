Updates: WR-471

#### WR-471 (update, 2026-11-09). `revolve_arc_centre_near_axis_is_sphere` is removed

A profile arc whose centre is within the caller's tolerance of the revolve axis is revolved as a sphere (the fork's c4209ab): centred on the axis through the arc's two ends, with its poles on the axis, not the arc's own circle, which misses both ends by up to the tolerance and leaves a pole parallel of radius about sqrt(2 tol R). The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::revolve_arc_centre_near_axis_is_sphere`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (the survey found no golden or rig case that reaches a different result with the flag on).
- **Tests:** `sweep::api_brep::tests::issue_1123_arc_centred_within_tolerance_of_the_axis_is_a_sphere_with_its_pole_on_it` (a quarter disc turned a whole turn is a plane and a sphere, volume 2/3 pi 20^3, pole on the axis) and `wr471_*` (a half circle 3e-7 off the axis is a whole sphere).
