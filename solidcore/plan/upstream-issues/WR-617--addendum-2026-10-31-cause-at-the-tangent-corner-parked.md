Updates: WR-617

#### WR-617 (update, 2026-10-31). Cause: the offset faces' loops cut the tangent corner the bridge wall covers; parked

- **Where it fails:**
  - The `SM_ERR` comes from `SmMerge::MakeFaceuse`'s vertex-orientation exit (`SmMerge.cpp`, the `pVBRel` test after `MergeEdge`): an other-operand edge's vertex is related to a primary vertex at neither end of the primary edge.
  - The merge is `SmOffsetExecutive::PiecewiseMerge`'s, on the shell's offset B-rep.
  - The disagreeing vertices lie at x ≈ 0 on the inset floor z = 2, at y = 18, 18.5 and 19.5 (port trace).
- **Cause (read from upstream and the port, not traced statement by statement):**
  - The open face y = 20 meets the arc side (r 10) tangentially along x = 0.
  - That tangent edge between a kept face and a shell face takes `EdgeuseOffset`'s shell-tangent-gap branch (`bShellTangentGap`), which bridges it with a wall: the plane x = 0, y 18..20.
  - The floor's and the top's offsets keep the original loop topology. Their rim edge along y = 20 and their arc-offset circle (r 8, reaching only y = 18) share one offset vertex, placed at the circle's end (0, 18, z). So each loop cuts the corner from (40.5, 20, z) to (0, 18, z) and has no edge on the bridge wall. The port's unmerged offset (`merge_results` off, no offset solid) shows this.
  - The merge then makes the inconsistent vertex relation. A correct offset needs a second corner vertex at (0, 20, z) and an edge along the wall to (0, 18, z) in each of the two loops.
- **Ruled out:**
  - A `MateEdges` split. The unmated hole/arc edge is present in the top-open shell too, and that shell closes.
  - The file round trip, the API's copy and the test harness.
- **The offset before the merge against pinned C++:**
  - Equal (every value, max relative error 0) for the top-open shell: `issue1795_plate_api_shell_2_top_offset_solid_unmerged`.
  - For y = 20, equal too (every value, max relative error 0): `issue1795_plate_api_shell_2_y20_unmerged` (no offset solid, the raw offset). The corner cut is upstream behaviour.
- **Fix:** sized L: new edge construction in offset face building (split the corner vertex and add the bridge-wall edge, as an opt-in). An opt-in design exists and is parked pending the reporter's confirmation, on tangency-labs/solidcore#1795, that the face their plate opens is the one tangent to the arc. If they confirm it, it comes back as a scheduled design task. Until then the port keeps upstream's `SM_ERR` (pin `shell_open_at_y20_fails_as_upstream`).
