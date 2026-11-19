Updates: WR-464

#### WR-464 (update, 2026-11-14). `offset_missing_surface_fails` is removed

`SmOffsetGeometryCreation::FaceuseOffset` asks the surface whether the offset collapses it onto or past its spine before making it (no surface, not a failure); any other offset that makes no surface fails with `SM_ERR` (the fork's 758829c). The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::offset_missing_surface_fails`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (measured on main a7468ec4f: every goldens unit with this and the other group A flags of the batch on, and the rig's table unchanged).
- **Tests:** `offset::offset_executive::face::tests::collapse_thresholds_of_the_analytic_surfaces`.
