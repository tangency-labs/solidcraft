Updates: WR-603

#### WR-603 (update, 2026-11-11). `fillet_trace_from_tangent_point_toward_next` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch B27, which retires the six flags
`fillet_rail_rim_on_face`, `fillet_tangent_point_one_sided_step`, `fillet_torus_center_on_axis`,
`fillet_torus_rail_on_axis`, `fillet_torus_span_from_start_section` and
`fillet_trace_from_tangent_point_toward_next`). `c3_trace_start` (`blend/fillet_solver/p4.rs`) always starts a trace whose first through point is a tangent point toward the next through point.

- **Tests:** `trace_from_a_tangent_first_point_starts_toward_the_next` (`blend/fillet_solver/p4.rs`) runs on `Kernel::default()`; `tests/wr603_trace_from_tangent_point.rs` keeps the peg's added volume.
- **Results:** the goldens rows the six fixes move, with their old and new values:
  `plan/findings/blend.fillet_solver--b27-six-flags-retired-2026-11-11.md`.
