Updates: WR-566

#### WR-566 (update, 2026-11-15). `local_operation_closes_walls_to_nothing` is removed; a wall a move closes to nothing always goes

`CloseLoopGap` (`offset/offset_executive/step_gaps.rs`) always squeezes out an edge whose vertex a local operation
moves onto its other end (a local operation only: see below), and `CloseLoopGaps` deletes the faces that closed to nothing; the face move
(`offset/offset_faces.rs`) always removes the edges then left between a moved face and a face that stayed on one
surface (the WR-566 addendum of 2026-10-31). The opt-in described in the original record no longer exists; the
step walls themselves are still behind `local_operation_step_walls`. Removed: the field, SolidCraft's preset line,
its `fixes.json` entry, the flag-off branches (the `SM_ERR_OFFSET_EDGE_VANISHES` refusal of a closed edge on a face
that closed to nothing, and the face move's end without the join), and the flag settings in the tests.

- **Local operations only.** The flag's squeeze ran in shells and offsets too. There an edge closed to a point is
  where the result meets itself along a line. `wr_657_fully_chamfered_box_inset_open_at_a_chamfer_is_refused` (a
  fully chamfered 30 x 20 x 10 box shelled 1 inward, open at a chamfer 2 wide) is such a case. Main refuses it with
  `SM_ERR_OFFSET_EDGE_VANISHES` (1043). With the flag on, it came back as a body of 4 shells that fails Level 2
  (`#18 Region Count`, `#19 Region Connectivity`). The squeeze now runs for `OffsetOperation::LocalOperation` only,
  so that shell keeps main's refusal.
- **Without `local_operation_step_walls`.** The flag's documentation made it a companion of
  `local_operation_step_walls`. The squeeze now runs in every local operation on the default kernel, step
  walls or not: wherever `CloseLoopGap` finds a vertex moved onto its edge's other end.
- **A later failure keeps the refusal by name.** If a gap fails to close (`SM_ERR`, `LoopGapUncertified`)
  after edges have closed to points, `close_loop_gaps` first runs `remove_closed_faces`' check on those edges
  (`closed_faces`). It returns `SM_ERR_OFFSET_EDGE_VANISHES` (1043) for an edge closed on a face that did not
  close to nothing and is not across from one that did, as it would once every gap was closed.
  `topology.brep_local_operation` gen0012 (`revolution_built_sound_or_refused_unchanged` in
  `issue_2978_local_reach_beside_nets.rs`) is such a case. Without this check the squeeze turned its 1043 into
  `LoopGapUncertified` (1001); with it, gen0012 keeps main's 1043.
- **Defaults:** SolidCraft's preset had the flag on. Its local operations don't change, except that a later
  gap failure after a squeeze now reports 1043, as above. A shell or offset that closes an edge to a point is
  refused (1043) instead of returning a body that fails Level 2. `Kernel::default()` changes for local
  operations.
- **Goldens:** no row of any unit moves.
- **Tests:** `wr_566_local_operation_closes_a_wall.rs` and `wr_648_offset_faces_closed_wall_join.rs` keep their
  closed-form cases (24000 and 12000) on the default kernel with `local_operation_step_walls`. Their flag-off tests
  (the 1043 refusal) are removed; the 1043 refusal of an edge moved past its end is still covered by
  `issue_2978_local_reach_beside_nets.rs` and `issue_2012_public_status_codes.rs`.
  `revolution_built_sound_or_refused_unchanged` (gen0012's 1043) and
  `wr_657_fully_chamfered_box_inset_open_at_a_chamfer_is_refused` are unchanged. Each fails if its part of
  this change is removed (the check on a later failure; the local-operation restriction).
