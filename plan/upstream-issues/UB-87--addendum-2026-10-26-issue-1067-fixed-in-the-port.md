#### Updates: UB-87 (tangency-labs/solidcore#1067), fixed in the port
- **Status in the port:** fixed. `Cone::create_iso_circle_from_point` (`crates/solidcore/src/surface/cone/p1.rs`) still inverts the point moved onto the cone as upstream does; only when that inversion fails with `SM_ERR` (no generator solution, a point beyond the end by up to the tolerance) it builds the circle at the point's height clamped to the end, as the fork's tangency-labs/smlib "SMLib: a cone's iso circle through a point is at the point's height" does, instead of returning the failure. Every other error still propagates.
- **Why not the fork's change as it is:** the fork replaces the inversion for every point; here a point whose inversion succeeds keeps the exact upstream parameter.
- **Test:** `iso_circle_beyond_the_end_within_tolerance_is_at_the_end` (a 70 degree cone of height 1, tolerance 1e-3, all four `swap_uv` and `inside_out` combinations: heights 0.9995 and 1.0 unchanged; 1.0005, 1.000684 and 1.0009 on the end circle; 1.002 none).
- **Opt-in:** none. Crash, undefined-behaviour and hang fixes are unconditional (#1018). The comparison shim refuses these inputs (UB-87), so no result the difftest compares changes.

Fixes tangency-labs/solidcore#1067
