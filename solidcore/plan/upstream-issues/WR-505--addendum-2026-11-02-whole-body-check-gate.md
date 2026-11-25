Updates: WR-505

#### WR-505 (update, 2026-11-02). Gated at operation level through a revolve and the whole-body check (#1954)

The #1954 flag audit (tdamsma, `flag-reachability-2026-10-31`) classed `internal_pole_row_through_knot` as B-unit: the only test that failed with the flag forced off, `issue_1515_horn_torus_has_no_internal_pole`, calls `has_internal_pole` directly on a canonical torus's NURB. No test showed an operation reaching it.

`wr_505_quarter_horn_torus_passes_the_body_check_only_with_the_fix` in `crates/solidcore/tests/issue_1511_revolve_disc_touching_axis.rs` now does. It takes the fork's #1511 capi case: a circle of radius 5 about (0, 0, 20) in the plane z = 20, its seam vertex on the line along Y through (5, 0, 20), revolved a quarter turn about that line with `create_rotational_sweep`, capped. It then runs `check_brep(Level2)` on the body. Only this flag differs between the two runs, on a default kernel otherwise.

| input | flag off (pinned upstream) | flag on (the fork, smlib#41) |
|---|---|---|
| quarter horn torus (axis 5 from the disc's centre) | 3 faces, volume 616.7936 (closed form pi^2 25 5 / 2 = 616.8503); check fails with one report: B-spline surface test 2, "Bad Internal Pole", on the swept face (`SmSurfOfRevolution`) | the same body; check valid, no failures |
| control: quarter ring torus (axis 8 from the centre) | 3 faces, volume 986.9724 (closed form 986.9604); check valid | the same |

The body is the same either way; only the check's answer changes, as the record says.

Mutation: with both production reads in `has_internal_pole` (`crates/solidcore/src/surface/surface/assert_valid.rs`, the U and V starting rows) forced to upstream's index, the test fails at the flag-on assertion with that same test-2 report. Restored, it passes.

The same answer holds with every `UpstreamFixes` flag on and only this one toggled (probe, not committed), and for a half turn. A full turn whose seam vertex is off the axis keeps the report either way: there the axis point is inside the NURB's V range, a real internal pole.

No library code, tolerance, unit status or known mismatch changes. Per the policy in #2392 the fork is the reference here, so this test is the parity gate for when the flag becomes the default.
