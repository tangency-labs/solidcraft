Updates: WR-450

#### WR-450 (update, 2026-11-02). Gated at operation level through the record's own boolean (#1954)

The #1954 flag audit (tdamsma, `flag-reachability-2026-10-31`) classed `analytic_copy_keeps_parameterization` as B-unit: the only test that failed with the flag forced off, `rational_half_cylinder_copy_keeps_parameterization`, calls `copy_and_add_analytics` directly. The record said the full boolean was not run in Rust, for want of a flat-record import.

`wr_450_box_minus_rational_cylinder_is_valid_only_with_the_fix` in `crates/solidcore/tests/wr_450_box_minus_rational_cylinder.rs` now runs it without one. The cylinder (centre (0, 50), r 40, z -60 to 60) is a capped degree-1 `create_skin_primitive` between two circles, each made of two copies of the record's rational quadratic half circle (four equal spans, weights 1, .75, .5, .5, .75, 1). Its two side faces are rational B-spline surfaces in the curves' own parameterization, as the record's faces are. The test then runs `boolean_difference` of the box (-14, 0, -14)-(14, 20, 14) and that cylinder on a default kernel, with only this flag toggled, and `check_brep(Level2)` on the result.

| | flag off (pinned upstream) | flag on (the fork) |
|---|---|---|
| faces, volume | 6, 8492.581163 (closed form 8492.579264) | the same |
| worst edge tolerance (body 1e-5) | 0.78120 | 1e-5 |
| check | `ValidatePointers` fails: `SmBrep` test #59 (edge tolerance 0.78120 against 0.01), test #82 twice (vertex tolerance); `AssertValid` skipped | valid, no failures |

These are #387's numbers (0.78 mm against a body tolerance of 0.022 there). With every `UpstreamFixes` flag on and only this one toggled (probe, not committed), the flag still decides the check: off, `SmBrep` tests #1 and #3 and `SmVertex` test #1 report vertex/face gaps; on, it is valid.

Mutation: with the production read in `copy_and_add_analytics` (`crates/solidcore/src/nurbs/b_spline_surface/p2.rs`) forced to upstream's path, the test fails at the flag-on assertion with the same three reports. Restored, it passes.

No library code, tolerance, unit status or known mismatch changes. Per the policy in #2392 the fork is the reference here, so this test is the parity gate for when the flag becomes the default.
