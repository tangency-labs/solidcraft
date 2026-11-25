# surface.cone_p2: recheck the residual cylinder fix against official 1.0.1 (2026-10-27)

Updates: plan/findings/surface.cone_p2--cylinder-nurb-rebuild-issue713--addendum-2026-10-26-upstreamfixes.md

PR: https://github.com/tangency-labs/solidcore/pull/724

The original issue #713 was closed by #1077, which added the opt-in mean-radius
correction. The later maintainer review on #724 confirms a remaining defect:
when the accepted radii have been equalized, their difference is zero and the
subsequent conditional angle update leaves an earlier nonzero semi-angle intact.
This PR completes that existing opt-in; it does not reimplement the closed issue.

The current project pin is official NVIDIA usd-brep 1.0.1,
`3fd5b9548d19aa8c12c29bc9637a96d42a06f8a5`, as declared in
`scripts/upstream-pin.env`. The entire
`SmCone::RebuildSTEPFromNURBParameters` function body in
`source/SMLib/src/SmCone.cpp` is identical to the previous official pin
`c9c5979f04cea8c38213004d4f04df0dcfb40d05` (compared directly in the two checkouts).
In particular, it still uses the strict `SM_EFF_ZERO * (1 + r)` radius equality
and changes the semi-angle only for a nonzero radius difference.

The patch quoted in #713 is from the tangency-labs/smlib fork, identified there
as `f06b48a` on `sync/1.0.1-capi`; it is distinct from the official 1.0.1 release.
The quoted patch adds cylinder radius reconciliation and taper rejection, not
an unconditional angle reset after accepting an edited cylinder. There is no
new official 1.0.1 rebuild implementation to port. Main's other 1.0.1 changes,
including its ellipse-trim code, were retained by merging main `8cb29a1fa`.

The stale-angle reset therefore remains inside the existing
`Kernel::fixes.cylinder_rebuild_mean_radius` accepted-cylinder branch. The default
continues to follow the project's official pin, now 1.0.1, and the taper error
still precedes all analytic-state changes. Four added library regressions plus
main's tests (14 cone-p2 tests in total) pass after the merge. The C++ libraries
and sparse upstream checkout used for revalidation both identify commit `3fd5b95`.

Validation against the 1.0.1 C++ reference:

```text
PASS surface.cone_p2 cases=1596 passed=1596 failed=0 deferred=0 max_rel_err=0e0 with_values=1596 no_values=0 vacuous=0
SENSITIVE surface.cone_p2 perturbed=1596 detected=1596 unperturbed=0 deferred=0
```

All 1,572 exact-base case records from main `8cb29a1fa` are retained unchanged;
24 issue713 fixtures are added. All four runner tests pass, including the
fixture checks with either flag value. Formatting, default/C++ workspace
Clippy, 2,429 kernel unit tests (two existing ignored), executed integrations,
wasm32, and the optimized primitive/cache tests pass.

The exact-base artifact is main CI run `40588316623`, artifact `12772748629`,
validated for provenance, digest and complete unit coverage by the official
baseline script. No filter is used and generated seeds match the baseline.

Final broad validation with that same main snapshot and reference:

```text
difftest: 261 unit(s), 260 passed, 1 partial, 0 failed
difftest: compared 261 reports against the baseline: 0 unit(s) regressed
```

The sweep enumerates 562,762 cases and completes in 474.6 seconds with four
workers. The partial unit is `sweep.topology_sweep`, already partial on the
exact base (677 passes, two existing known mismatches, seven parked C++ defects
and two rejected fixtures). This PR adds no mismatch or parking entries.
The report comparison uses matching default seeds and no case filter; it
performs no native rerun. The 75% value-share and exact-base retention checks
pass. Reports remain uncommitted.
