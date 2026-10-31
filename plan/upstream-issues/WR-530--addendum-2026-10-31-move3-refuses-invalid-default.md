#### WR-530 addendum (2026-10-31). With step walls, a face moved onto the body's bottom plane is refused (a step gap); the default returns an invalid body
Updates: WR-530

- **Input:** the stepped block of `crates/solidcore/tests/wr_648_offset_faces_closed_wall_join.rs`, a 40 x 30 x 20 block less the part x > 20, z > 10. Its low top (z = 10) is moved by -10 onto the plane of the bottom face, through `OffsetFacesExecutive::offset_faces`.
- **Default (no flags):** Ok, 8 faces, volume 11999.999999999996, manifold. The body is not valid. Level 2 reports 9 failures:
  - SmTrimmingTools Test #0 Edge/Edge Gap: 3;
  - SmTrimmingTools Test #1 UVEdge/UVEdge Gap: 1;
  - SmTrimmingTools Test #2 UVEdge/UVEdge Gap: 2;
  - SmBrep Test #61 Valid: 2;
  - "AssertValid skipped: ValidatePointers failed": 1.
- **With `local_operation_step_walls` and `local_operation_offset_status`:** `Err Upstream { operation: "offset::offset_executive::step_gaps", code: 1043 }`, `SM_ERR_OFFSET_EDGE_VANISHES`: a step gap. The traced `SM_ERR` sites of `step_gaps.rs` were not hit. The status comes from one of the module's two explicit 1043 returns, which this trace does not tell apart:
  - `:280`, a closed edge of a face that did not close to nothing;
  - `:392`, the fork's 34318f4: a vertex moved past the edge's other end.
- **With `local_operation_step_walls` alone:** `SM_ERR` (1001) from `local_operation`'s generic wrap of the extended offset's failure. The stack at the wrap:
  - `topology::brep::local_operation::local_operation::{closure}` (`topology/brep/local_operation.rs:184`);
  - `local_operation` (`local_operation.rs:180`);
  - `offset::offset_executive::p2::local_operation_simple` (`offset/offset_executive/p2.rs:1687`);
  - `OffsetFacesExecutive::move_faces` (`offset/offset_faces.rs:907`);
  - `OffsetFacesExecutive::offset_faces_editing` (`offset_faces.rs:1439`);
  - `OffsetFacesExecutive::offset_faces` (`offset_faces.rs:1410`).
- **Under SolidCraft's whole preset** (tangency-labs/solidcraft#8526's `fixes()`, 188 flags), the same move surfaces as 1001 from `topology::brep::p9`, through another preset flag that changes the path. That flag is not identified.
- **Found by:** a bisection over that preset on this input. `local_operation_step_walls` is the only flag needed for the refusal.
- **Ruling (2026-10-31):** not a regression. The default's body fails `ValidatePointers` and has trim gaps, so refusing it is no worse than the default's invalid result. The flag is unchanged.
- **Open-gap list:** this goes on WR-530's list of open gaps, beside the two in the 2026-10-29 addendum (`WR-530--addendum-2026-10-29-step-walls.md`): `extend_trim_edges`' 1001 on the split block's piece, and the boss wall stopping in `close_loop_gap` with 1043. It is probably the same gap as the boss wall's 1043.
- **Still open:** no valid result for this move, a stepped body with the low top on the bottom plane, is built with or without the flag. The fork's result is not determined: the move goes through the fork-only `SmOffsetFacesExecutive` (WR-615), so pinned SMLib has no counterpart, and the fork was not run.
