Updates: WR-611

#### WR-611 (update, 2026-11-15). `fillet_trim_inside_by_best_dot` is removed

`TrimOriginalSurfaces` (`crates/solidcore/src/blend/fillet_geom/p6.rs`) now always takes the piece of the original face (or its split faces) 'inside' the fillet's rail as the one whose binormal makes the largest dot with the fillet's, above `SM_EFF_ZERO_SQRT`. Upstream's test, any piece above a fixed 0.2, is gone. The binormal of each candidate edge use is evaluated once (in the choice), not again in the marking loop. Removed: the field, SolidCraft's preset line, its entries in `kernel_cases/fixes.json` and `issue_2260_gn_pipe_refined_range_end.rs`, its entry in the nine fix lists of `fixtures/solidcraft/851-knife-edge-blends.json`, and every test setting of it.

- **Results:** no goldens case changes.
- **Tests:** `wr_611_steep_fillet_trims_its_face` keeps its closed-form volumes (d 0.5 to 5, both sides). Its flag-off test (`wr_611_without_the_opt_in_steep_chamfers_fail`) is removed. `chamfer_knife_edge_equal_distances`, `issue_1945_fork_1207_step4`, `issue_2020_offset_mitred_below_depth` and `wr650_tabbed_plate_mitred_rounds` keep their assertions without setting it.
