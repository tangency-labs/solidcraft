Updates: WR-568

#### WR-568 (update, 2026-11-11). `fillet_torus_span_from_start_section` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch B27, which retires the six flags
`fillet_rail_rim_on_face`, `fillet_tangent_point_one_sided_step`, `fillet_torus_center_on_axis`,
`fillet_torus_rail_on_axis`, `fillet_torus_span_from_start_section` and
`fillet_trace_from_tangent_point_toward_next`). `torus_fillet_geom_parts_of` (`blend/fillet_solver/p2.rs`) always takes the span as the turn from the start section to the end one; `IsArc`'s start angle is no longer read.

- **Tests:** `tests/wr_568_round_along_an_arc_span.rs` and `tests/issue_1952_extended_rail_end_unsolved_section.rs` run on `Kernel::default()`: every box of the latter rounds to the closed form; the pinned failures of the -X and -Y boxes and of the groove arc are no longer tested.
- **Results:** the goldens rows the six fixes move, with their old and new values:
  `plan/findings/blend.fillet_solver--b27-six-flags-retired-2026-11-11.md`.
