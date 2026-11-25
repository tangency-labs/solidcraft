Updates: WR-440

#### WR-440 (update, 2026-11-02). Gated at operation level through a revolve and the whole-body check (#1954)

The #1954 flag audit (tdamsma, `flag-reachability-2026-10-31`) classed `check_loop_projected_seam_ends` as B-unit: the only test that failed with the flag forced off, `issue_530_projected_rotational_sweep_loops_cross_periodic_seams`, builds the face with the internal `analytic_rotational_sweep` and calls `check_loop` directly.

`wr_440_revolved_line_passes_the_body_check_only_with_the_fix` in `crates/solidcore/tests/wr_440_revolved_line_seam_check.rs` now reaches it through public operations. It revolves one line in the xz plane a full turn about Z with `create_rotational_sweep` (uncapped), for #530's three profiles, and runs `check_brep(Level2)` on the one-face body. Only this flag differs between the two runs, on a default kernel otherwise.

| profile | flag off (pinned upstream) | flag on (the fork) |
|---|---|---|
| cylinder, (2, 0, 0) to (2, 0, 3) | `SmTrimmingTools` test #1, "UVEdge/UVEdge Gap" in U of 12.566371 (one period) against 1.2566371 | valid, no failures |
| cone frustum, (2, 0, 0) to (1, 0, 3) | the same report, gap 1 against 0.1 | valid |
| reversed cone, (1, 0, 0) to (2, 0, 3) | the same report, gap 1 against 0.1 | valid |

`AssertValid` itself returns true both ways: the face test does not fail on `CheckLoop`'s report, but the check lists it and the body is not valid. The same holds with every `UpstreamFixes` flag on and only this one toggled, with `revolve_analytic_profile_faces` on or off, and capped (probe, not committed).

Mutation: with the production read in `check_loop` (`crates/solidcore/src/topology/trimming_tools.rs`) forced to upstream's raw comparison, the test fails at the flag-on assertion with that same report. Restored, it passes.

No library code, tolerance, unit status or known mismatch changes. Per the policy in #2392 the fork is the reference here, so this test is the parity gate for when the flag becomes the default.
