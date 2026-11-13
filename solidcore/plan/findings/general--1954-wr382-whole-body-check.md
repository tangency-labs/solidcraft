# General: WR-382 whole-body minimum-radius regression (2026-11-02)

For #1954, PR #2383. Rechecked main `fdc55bbb4`, the original audit's
`audit-flag-reachability.md`, `final.json`, and instrumentation patch at
`tdamsma/solidcore@a520213fc094b898a5107417f9e722f539d4bcc3`.
The audit classified the shipped (`reported`) flag `bspline_check_min_radius`
(WR-382) as B-unit: three of five hitting tests failed forced off, but only
direct curve checks supplied the evidence; no SolidCraft case hit it.

The merged #2105 and #2124 records leave WR-612, WR-601, WR-597 and WR-643
without a demonstrated operation-level difference under the shipped preset.
Their documented masks and fixture/dependency gaps remain outside this slice.
This contribution covers another outstanding shipped flag from the original
audit, rather than repeating those unsuccessful geometry probes.

`crates/solidcore/tests/issue_1954_wr382_wire_check.rs` builds a wire through
`new_brep` and `create_wire_edge_from_curve`, then calls the whole-body
`check_brep(Level2)` operation. It does not call the guarded curve checker
directly, hand-build topology records, or replace a geometry with a stand-in.
Both bodies are independently constructed from the same data with the default
kernel, preserving their geometry owner identities. Only
`bspline_check_min_radius` changes between the checks.

The quadratic is `C(t) = (w(t-1/2), (t-1/2)^2, 0)`, on `[0,1]`.
Its speed at the vertex is nonzero (`w`), and its unique minimum radius is
`w^2/2` at `t=1/2`. The narrow input has `w=0.001`, minimum radius `5e-7`;
the healthy control has `w=0.02`, minimum radius `2e-4`.
The observed result is whole-body acceptance versus rejection, accompanied
by exactly one failed report (B-spline test 14), its analytic radius to `1e-9`
relative and its parameter to `1e-9` absolute. A guard hit without that rejection
cannot satisfy the regression. The healthy control prevents blanket rejection.

Pinned C++ has the never-firing radius comparison (WR-382), so its default
acceptance is the parity contract. The opt-in rejection is checked against
the analytic curvature, not claimed as parity with pinned C++. No production
code, tolerances, unit statuses or mismatch exemptions change.

The focused integration test passes. Temporarily changing only the test's
`k.fixes.bspline_check_min_radius = true` to `false`, leaving construction and
all assertions intact, makes it fail at `assert!(!fixed.is_valid())`:
`pointers_status=1000`, no pointer reports, `assert_valid=Some(true)`, no
assertion reports. Restoring `true` passes again. Thus the flag's decision
affects the public check's result, rather than merely being evaluated.

Default-policy comparisons with the shared pinned C++ reference passed:

```text
PASS nurbs.b_spline_curve_p3 cases=2460 passed=2460 failed=0 deferred=0 max_rel_err=0e0 with_values=2460 no_values=0 vacuous=0
PASS topology.assert_valid cases=965 passed=963 failed=0 deferred=0 max_rel_err=0e0 with_values=963 no_values=0 vacuous=0 rejected=2
PASS topology.brep_p5 cases=797 passed=797 failed=0 deferred=0 max_rel_err=0e0 with_values=797 no_values=0 vacuous=0
```

The existing `curvature_radius_tiny_3d_*` / `curvature_radius_regular_3d_*`
native fixtures use the same parabola family under the default policy.
This is not a claim of an added end-to-end C++ fixture. Main `04d51a64e`
was integrated before final focused verification; the only intervening main
change was the independent boolean work-counter regression (#2380).
