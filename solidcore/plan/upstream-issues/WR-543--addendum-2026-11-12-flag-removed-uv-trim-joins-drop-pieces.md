Updates: WR-543

#### WR-543 (update, 2026-11-12). `uv_trim_joins_drop_pieces` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch A15a, which retires four topology flags). `create_uv_trim_curve` (`topology/edgeuse/p3.rs`) keeps the longest run of drop pieces that meet end to end, joined on their own edge parameters; the step that stretched a piece within 10% of the edge onto the whole edge interval is gone.

- **Tests:** `fork_e36feb1_every_trim_lifts_onto_its_edge_with_the_fix` runs on `Kernel::default()`; `fork_e36feb1_the_boundary_row_trim_runs_off_its_edge_by_default` is gone.
- **Moved test:** `wr_407_default_join_loses_the_folded_cover` joins two swept walls without WR-407's cover rule (`cover_refuses_folded_continuation`). Its volume was 14596 mm3 short of the closed form with a drop piece stretched onto each trim's edge and is 7744 mm3 too large with the pieces joined, so the test now asserts the excess; with the cover rule the join keeps the closed form to 1 mm3 either way.
- **Results:** the batch survey puts the flag in group A, and the full gate at the retiring change confirms it: no goldens row and no kernel rig case changes.
