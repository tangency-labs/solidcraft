Updates: WR-540

#### WR-540 (update, 2026-11-14). `edge_check_orders_tangent_faces` is removed; ordering tangent faces by where they go is the only path

`SmEdge::AssertValid` test 9 (`topology/brep/validation/assert_valid/edge.rs`) always checks whether two faces round
the edge leave it within `SM_ANG_TOL_DEG` of each other and, where they do, orders every face by the direction to its
point 20 edge tolerances into the face, square to the edge. The flag is gone. Removed: the field, SolidCraft's preset
line, its entries in the kernel rig's `fixes.json` and in `issue_2260_gn_pipe_refined_range_end.rs`, the flag-off
branch, and the tests' flag settings.

- **Defaults:** SolidCraft's preset had the flag on, so the app's results don't change. `Kernel::default()` changes:
  a body whose faces are tangent along an edge is no longer refused by test 9.
- **Goldens:** no row of any unit moves.
- **Tests:** `tests/wr_540_tangent_cylinder_edge_order.rs` runs on the default kernel; its expectation is unchanged.
