Updates: WR-301

#### WR-301 (update, 2026-11-11). `fillet_rail_rim_on_face` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch B27, which retires the six flags
`fillet_rail_rim_on_face`, `fillet_tangent_point_one_sided_step`, `fillet_torus_center_on_axis`,
`fillet_torus_rail_on_axis`, `fillet_torus_span_from_start_section` and
`fillet_trace_from_tangent_point_toward_next`). `TestRollOver` (`blend/fillet_solver/p5.rs`) always joins a rail's neighbouring intervals on its face or the face's boundary before the rollover.

- **Tests:** `tests/fillet_wr300_320.rs` runs on `Kernel::default()`: the holed cube's chamfer and round at 5 build to their closed forms, and at 4 (clear of the rims) too; the pinned refusal is no longer tested. Fixtures `1080-blends-that-fit-beside-a-pocket` and `4055-chamfer-past-a-joined-pin` no longer opt in.
- **Results:** the goldens rows the six fixes move, with their old and new values:
  `plan/findings/blend.fillet_solver--b27-six-flags-retired-2026-11-11.md`.
