Updates: WR-346

#### WR-346 (update, 2026-11-15). `offset_collapsed_rounds_mated` is removed

An extended inset face whose offset collapses (a convex round thinner than the wall) gets no offset face, and the faces beside it are mated across it in `do_extended_offset`, as at a sharp edge. The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::offset_collapsed_rounds_mated`, SolidCraft's preset line, its entries in `fixes.json` and `issue_2260_gn_pipe_refined_range_end.rs`, and every test setting of it.

- **Results:** three `offset.offset_executive_p2` goldens rows move, all insets past a primitive's own radius, each accounted for from the closed form by the goldens test `offset_offset_executive_p2::tests::wr346_collapsed_insets_follow_the_closed_form`:
  - `extended_sphere_inset_past_radius` (unit sphere, inset 1.5): 1001 becomes 1000 with an empty body (one region, no faces). The inset set of a ball of radius 1 by 1.5 is empty.
  - `member_cylinder_extend_inset_at_radius` and `member_cylinder_extend_inset_past_radius` (r 1, h 4, inset 1 and 1.5): 1001 becomes `SM_ERR_DEGENERATE_SURFACE` (1034), still a refusal; the side wall's offset radius `1 - d` is 0 or negative. The offset built before the refusal is the two caps' offsets, planes at `z = d` and `z = 4 - d`.
- **Tests:** `tests/wr567_revolution_round_offset_collapses.rs`, `tests/issue_1945_fork_1207_step4.rs` and the ignored `issue_397_inset_rounds_thinner_than_wall_collapse` in `offset_executive/p2.rs` now run on the default kernel.
