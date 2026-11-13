#### Updates: UB-87 (tangency-labs/solidcore#1067)
- **Confirmed (tangency-labs/solidcore#1067):** in gdb, a reference point at height 1.000684 on a cone whose top is at 1 (tolerance 1e-3) leaves `StepV` at `SM_UNDEF_DOUBLE` (1.23456e-61); the iso circle is built there. It is reached from `SmSphere::IntersectWithCone` when a sphere crosses the cone's line just beyond its end.
- **Why:** the reference point passes the height check (`ContainsValue(dParam, d3DTolerance)`), but the point moved onto the cone at that height is beyond the generator's end by up to `d3DTolerance / cos(semi angle)`, farther than the inversion's tolerance, so the generator drop finds no solution and the status that says so is voided.
- **Fix (fork):** the circle's StepV is the clamped height `dParam`; no inversion is needed (tangency-labs/smlib, "SMLib: a cone's iso circle through a point is at the point's height").
- **Rust port:** unchanged. `Cone::create_iso_circle_from_point` returns the failed inversion (`SM_ERR`, 1001) for a point beyond the end within the tolerance, as UB-87 says; the fork's behaviour (a circle at the clamped end height) is not ported, since the difftest target is upstream `c9c5979` and whether the port follows fork fixes is open (#1018). Test `iso_circle_beyond_the_end_within_tolerance_returns_the_failed_inversion` pins the #1067 case (heights 0.9995, 1, 1.0005, 1.000684, 1.0009, 1.002 on a 70 degree cone of height 1).
- **Opt-in:** none; no result the difftest compares changes.

For tangency-labs/solidcore#1067
