Updates: WR-530

#### WR-530 (update, 2026-11-16). `local_operation_step_walls` is removed

The step walls of the 2026-10-29 addendum are the default and their flag is gone (tangency-labs/solidcraft#14246, batch offset). In a local operation, a tangent edge whose two faces stay parallel and apart gets the ruled step wall between the two offset edges (`is_step_across_edge`, `create_step_wall`), and the loop gaps at its ends are closed (`close_loop_gaps`, `split_edges_at_step_corners`). Offsets and shells are unchanged: every site runs only for a local operation.

Removed:
- the field `UpstreamFixes::local_operation_step_walls`;
- SolidCraft's preset line;
- its entries in `fixes.json` and in the flag lists of `issue_1945`, `issue_2020`, `issue_2260` and `wr650`.

- **Tests:**
  - These run on the default kernel: `tests/wr_530_local_operation_step_walls.rs`, `wr_566_local_operation_closes_a_wall.rs` and `wr_648_offset_faces_closed_wall_join.rs`. Removed are the two tests that pinned the refusal without the flag.
  - In `topology/brep/local_operation.rs`, the rounded cube's side moved out 0.1 off its round was refused (`LoopGapUncertified`). It now gets its 0.1 step wall and is a manifold solid of volume 8000 - 20 (25 - 25 pi / 4) + 0.1 * 15 * 20, checked to 1e-9 (`a_side_moved_off_its_round_gets_a_step_wall`). The refusal-leaves-the-input case that used it now uses a split block piece turned 0.02 rad and raised 1, which still fails at the loop gap.
  - A local operation given a plane parametrised otherwise than its face's still does not close (`a_coplanar_neighbour_raised_off_its_twin_is_refused_as_not_sound`). The step wall is built from the face's own surface offset, and the UV-matched edges of a fresh plane lie elsewhere.
- **Results:** no goldens row moves (each flag alone against main, 2026-11-16).
