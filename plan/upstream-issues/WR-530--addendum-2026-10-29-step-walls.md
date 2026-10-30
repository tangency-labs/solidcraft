#### WR-530 addendum (2026-10-29). The moved face's step is built instead of refused
Updates: WR-530

- **Fork follow-up:** tangency-labs/smlib 5d428ae ("a local operation that moves one of two tangent faces along its normal builds the step"), with 90aabff (a step wall between lines stored with different weights) and 34318f4 (`SM_ERR_OFFSET_EDGE_VANISHES`). None of them is merged upstream.
- **What changes:** WR-530's refusal stays correct where the two faces meet nowhere. Where one of two tangent faces moved along its normal, though, the two stay parallel and apart, and the fork closes that gap with a wall. `SmOffsetGeometryCreation::EdgeuseOffset` (local operation, tangent edge) first asks `IsStepAcrossEdge`: does the gap between the two rebuilt edges run along both faces' normals at all 11 samples, and is it wider than ten times the larger of the edges' tolerances and the approximation tolerance? If so, `CreateStepWall` builds the ruled wall between the two edges. A rail whose weights are all one value is first made non-rational (90aabff). The second rail's direction is judged at its quarter points, because a closed rail's ends coincide. The wall becomes a cap face and a closing face, so the face extensions leave it alone. `CloseLoopGaps` then closes the gaps at the wall's ends, and `SplitEdgesAtStepCorners` splits the edges that run past a wall corner.
- **Rust port:** behind `Kernel::fixes.local_operation_step_walls` (off by default):
  - `offset_geometry_creation.rs`: the step test and the wall (`is_step_across_edge`, `create_step_wall`, `EdgeuseOffset::closing_wall`);
  - `offset_executive/edgeuse.rs` and `p1.rs`: the wall becomes a closing face;
  - `offset_executive/step_gaps.rs`: `close_loop_gaps` and `split_edges_at_step_corners`, called from `do_extended_offset` for a local operation.
- **Tests:** `tests/wr_530_local_operation_step_walls.rs` builds three cases from the fork's description:
  - a 40 x 30 x 10 block with its top split at x = 20, the piece moved up 2: 13200;
  - a 60 x 40 x 20 block with its top split at x = 30, the piece moved up 1 (49200) and down 1 (46800);
  - WR-530's boss wall moved out 1 off its base round: plus pi * 21 * 8.

  Each case checks that WR-530's refusal still applies without the new flag.
- **Open in the port (2026-10-30 probe, flag on):** two of the fork's cases are not built yet; their tests are `#[ignore]`d.
  - The split block's piece x > 30, the fork's own case: it lies under the imprinting box, and the imprint also splits its side neighbours. `extend_trim_edges` still refuses it (1001) for both blocks, up and down. The piece on the other side of the split is built, with exact volumes.
  - The boss wall: it stops in `close_loop_gap` with `SM_ERR_OFFSET_EDGE_VANISHES`.

  Both need a trace against the fork to find where the port parts from it.
- **Not ported:** the fork's offset-faces module (a96ce9f, `smlib_offset_faces` with fillet removal) needs `SmFilletRemoval` and `SmFeatureExecutive`, which are unported, and the fork-only `SmRingFilletRemoval`. It is left for a follow-up (tangency-labs/solidcore#1060).
