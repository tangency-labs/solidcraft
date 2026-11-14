Updates: WR-598

#### WR-598 (update, 2026-11-14). `delete_vertex_joins_seam_iso_curve` is removed; the seam iso-curve join is the only path

`SmBrep::DeleteTopologicalVertex` (`topology/brep/p5.rs`, `exact_bspline_of_edge`) always joins an edge whose curve is
a curve on a B-spline-kind surface along a UV line of constant u or v as that surface's iso curve over the edge (the
same curve exactly). Any other curve that is not of an `SmBSplineCurve` kind still leaves the vertex in place (the
routine's silent `SM_ERR`, the crash avoidance kept unconditionally since the record). The flag is gone. Removed: the
field, SolidCraft's preset line, the goldens SolidCraft-case arm, its entries in the kernel rig's `fixes.json` and in
`issue_2260_gn_pipe_refined_range_end.rs`, the flag-off branch, and the tests' flag settings.

- **Defaults:** SolidCraft's preset had the flag on, so the app's results don't change. `Kernel::default()` changes:
  the join succeeds for such seams, where it left the vertex.
- **Goldens:** no row of any unit moves.
- **Tests:** `tests/wr598_delete_vertex_seam_iso_curve.rs` runs on the default kernel; the half that pinned the
  vertex left in place is removed, and the join's assertions are unchanged.
