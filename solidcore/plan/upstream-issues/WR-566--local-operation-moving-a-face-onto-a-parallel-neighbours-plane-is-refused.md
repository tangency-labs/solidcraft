#### WR-566. A local operation that moves a step's face onto the other top's plane is refused
- **Severity:** Failure (refused)
- **Area:** Offsets and shells
- **Location:** the fork's `SmOffsetExecutive::CloseLoopGap` and `CloseLoopGaps` (fork-only, opt-in in the port as `local_operation_step_walls`; WR-530 addendum). The pinned SMLib has neither.
- **What happens:** with the step walls opt-in, a local operation that moves a step's low top up onto the high top's plane, or the high top down onto the low top's plane, takes the wall's lower edge onto its upper one. The wall's edges along the sides, and the sides' edges along the wall, close to points. `CloseLoopGap` treats any edge whose vertex moved to its other end like a face turned inside out past it, and returns `SM_ERR_OFFSET_EDGE_VANISHES` (1043).
- **Trigger:** a stepped block, 40 x 30 x 20 with the part x > 20, z > 10 cut away. Either its low top moves up 10 or its high top moves down 10. This is SolidCraft's replace face, rig case shell/cov-replace-face-step-onto-the-higher-face.
- **How found:** reported by SolidCraft. The fork's fix is tangency-labs/smlib ccbf77a ("a step wall the move closes to nothing is removed"; smlib PR 61), which is not merged upstream.
- **Confirmed:** no C++ run was made here (the pinned C++ has no step walls). The fork's capi test `offset_faces_close_a_step_wall` gives the 40 x 30 x 20 box (24000) and the 40 x 30 x 10 box (12000).
- **Rust port:** with `Kernel::fixes.local_operation_closes_walls_to_nothing` (it needs `local_operation_step_walls`, under which `close_loop_gaps` runs):
  - `close_loop_gap` squeezes out an edge whose other vertex lies at the gap's vertex, and notes its offset face and the original face across it;
  - once every gap is closed, `close_loop_gaps` deletes the offset faces that closed to nothing (`IsDegenerate`) and forgets their pairs in the executive's relations;
  - a closed edge on no such face, and across from none, is still 1043.

  `tests/wr_566_local_operation_closes_a_wall.rs` checks both moves, and that without the flag each is 1043.
- **Separate flag:** ccbf77a changes what the step-walls feature accepts (a face closing to nothing is new behaviour, not a correction of the step wall), so it has its own flag.
