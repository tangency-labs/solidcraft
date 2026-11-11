Updates: WR-422

#### WR-422 (update, 2026-11-11). `fillet_torus_center_on_axis` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch B27, which retires the six flags
`fillet_rail_rim_on_face`, `fillet_tangent_point_one_sided_step`, `fillet_torus_center_on_axis`,
`fillet_torus_rail_on_axis`, `fillet_torus_span_from_start_section` and
`fillet_trace_from_tangent_point_toward_next`). `torus_fillet_geom_parts_of` (`blend/fillet_solver/p2.rs`) always turns a section centred on the arc's axis into the half-planes through the edge's ends.

- **Tests:** `tests/issue_417_torus_center_on_axis.rs` runs on `Kernel::default()`; its upstream half (the rim round failing) is removed.
- **Results:** the goldens rows the six fixes move, with their old and new values:
  `plan/findings/blend.fillet_solver--b27-six-flags-retired-2026-11-11.md`.
