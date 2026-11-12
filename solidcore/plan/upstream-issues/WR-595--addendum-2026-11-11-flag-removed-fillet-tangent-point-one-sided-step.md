Updates: WR-595

#### WR-595 (update, 2026-11-11). `fillet_tangent_point_one_sided_step` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch B27, which retires the six flags
`fillet_rail_rim_on_face`, `fillet_tangent_point_one_sided_step`, `fillet_torus_center_on_axis`,
`fillet_torus_rail_on_axis`, `fillet_torus_span_from_start_section` and
`fillet_trace_from_tangent_point_toward_next`). `compute_point_values` (`blend/fillet_solver/p4.rs`) always takes a tangent point's tangent from the forward step alone when it found one. WR-603 is removed with it, as the WR-595 addendum of 2026-10-31 asks.

- **Tests:** no test set this flag alone; the goldens rows it moves are listed in the findings file.
- **Results:** the goldens rows the six fixes move, with their old and new values:
  `plan/findings/blend.fillet_solver--b27-six-flags-retired-2026-11-11.md`.
