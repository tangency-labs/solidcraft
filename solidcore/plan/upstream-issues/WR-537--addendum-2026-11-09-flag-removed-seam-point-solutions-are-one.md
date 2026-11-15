Updates: WR-537

#### WR-537 (update, 2026-11-09). `seam_point_solutions_are_one` is removed

`ComputePointParameters` (`PointClassification`) keeps the nearest of a face or surface solve's solutions when they all evaluate to one point within the source zone tolerance (a point on a closed face's seam), both for the first solve and for the one at 100 times the tolerance (the fork's tangency-labs/smlib@577f7be). The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::seam_point_solutions_are_one`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (the survey found no golden or rig case that reaches a different result with the flag on).
- **Tests:** `wr_537_hair_off_pin_is_empty` (`tests/wr_537_pin_a_hair_off_a_seam.rs`) and `fork_577f7be_hair_off_pin_shares_nothing` (`tests/fork_577f7be_hair_off_pin_seam.rs`) build the pin a hair off the hole and expect an empty intersection; the test that expected `SM_ERR` from the default kernel is removed.
