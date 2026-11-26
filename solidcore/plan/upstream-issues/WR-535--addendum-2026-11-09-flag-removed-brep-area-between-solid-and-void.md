Updates: WR-535

#### WR-535 (update, 2026-11-09). `brep_area_between_solid_and_void` is removed

A volume-separating faceuse adds its volume only where it bounds a non void region and its area only where the area behaviour selects it (the fork's tangency-labs/smlib@d2d9586), so a face between two material regions bounds both volumes but is no part of the area. `brep_mass_properties_estimated` with `sheet_faces_area` no longer refuses a body with a partition (it refused it because the boundary would count the partition from both sides). The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::brep_area_between_solid_and_void`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (the survey found no golden or rig case that reaches a different result with the flag on).
- **Tests:** `mass::integrator::properties::tests::a_partition_is_no_part_of_the_area` (two merged boxes: area 800, volume 1500), `a_fin_inside_the_material_is_counted_once` and `merged_cubes_area_counts_solid_void_faces_only`; the assertion that the default refused the partition is removed.
