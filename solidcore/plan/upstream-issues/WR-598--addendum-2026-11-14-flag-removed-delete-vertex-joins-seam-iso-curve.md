Updates: WR-598

#### WR-598 (update, 2026-11-14). `delete_vertex_joins_seam_iso_curve` is removed; the seam iso-curve join is the only path

`SmBrep::DeleteTopologicalVertex` (`topology/brep/p5.rs`, `exact_bspline_of_edge`) always joins an edge whose curve is
a curve on a B-spline-kind surface along a UV line of exactly constant u or v as that surface's iso curve over the edge
(the same curve exactly). Two cases the flagged path took are now refused, with the routine's silent `SM_ERR`
(CodeRabbit on #16965):

- a UV line whose u (or v) drifts by up to `EFF_ZERO` times the domain's length, which the fork took as constant. Its
  iso curve is not the edge's curve: the gap is up to `|dS/du| du`, which nothing bounded (on `S(u, v) = (u / EFF_ZERO,
  v, u v / EFF_ZERO)` the line from `(0, 0)` to `(EFF_ZERO / 2, 1)` ends at `(0.5, 1, 0.5)`, its `u = 0` iso curve at
  `(0, 1, 0)`);
- a surface that keeps a definition beside its net (a procedural blend's or a screw sweep's), whose iso curve is cut
  from the net, when the net's certified bound exceeds the joined edge's tolerance. The tolerance is not widened.

Unit tests `topology::brep::p5::tests::wr598_only_an_exactly_constant_uv_line_is_an_iso_curve` and
`wr598_iso_curve_on_a_definition_needs_its_bound_within_the_tolerance` pin both and fail without them. Any other curve that is not of an `SmBSplineCurve` kind still leaves the vertex in place (the
routine's silent `SM_ERR`, the crash avoidance kept unconditionally since the record). The flag is gone. Removed: the
field, SolidCraft's preset line, the goldens SolidCraft-case arm, its entries in the kernel rig's `fixes.json` and in
`issue_2260_gn_pipe_refined_range_end.rs`, the flag-off branch, and the tests' flag settings.

- **Defaults:** SolidCraft's preset had the flag on, so the app's results don't change for exactly constant seams on
  surfaces without a definition; the two refusals above are new for it. `Kernel::default()` changes: the join
  succeeds for such seams, where it left the vertex.
- **Goldens:** no row of any unit moves.
- **Tests:** `tests/wr598_delete_vertex_seam_iso_curve.rs` runs on the default kernel; the half that pinned the
  vertex left in place is removed, and the join's assertions are unchanged.
