# nurbs.b_spline_curve_p3: historical curvature-radius evidence (2026-10-30)

Status: historical evidence; superseded by the default-off policy in WR-382.
The dependency and mismatch descriptions below describe the original unconditional
implementation, not the current tests-only PR. See
`plan/upstream-issues/WR-382--addendum-2026-10-30-analytic-radius-controls.md`.

## Minimum curvature radius validity and solver dependency (#475, PR #677, 2026-10-24)

`BSplineCurve::assert_valid_with` computed the minimum-radius analysis and then
compared an unchanged `BIG_DOUBLE` sentinel with the tolerance. Tests 13/14 could
never fire. Read the analyzed value before the comparison; keep the original
solver, tolerances, operation order and empty-analysis fallback. No extra global
analysis pass is added.

The analytic quadratic `C(t) = (w*(t-1/2), (t-1/2)^2, 0)`, over `[0,1]`, has
minimum radius `w^2/2` at `t=1/2` and nonzero minimum speed `w`. For `w=0.001`,
that radius is exactly `5e-7`, below the computed `2.5000199999200006e-5`
tolerance. The pinned C++ and original Rust return valid with no report; fixed
Rust returns invalid with report 13 (2D) or 14 (3D), value `5e-7`, owner parameter
`(0.5,0)`. Both level-given and base-check paths are covered, including calls
without a report list. The regression fails before the fix. Regular parabolas
(`w=0.02`), straight lines and a C1 straight-span/regular-bend curve remain valid.
Nine native fixtures retain these positive and negative controls.

Five native cases are genuine pinned-C++ correctness differences, recorded
individually in `known_mismatches.toml`: four `curvature_radius_tiny_*` fixtures
and `gen1393`. The latter has an independently evaluated radius
`3.735538104852004e-5` at `10.627113239579597`, below its
`0.0002919990674046823` tolerance (the property evaluator succeeds). The native
reference misses those reports because of its sentinel comparison. No oracle,
refusal guard, fixture or tolerance is weakened.

**Merge dependency: #668 must land before this check is enabled.** Enabling it
also exposes the existing #481 failed-evaluation defect on `curve0_assert_given`
and `curve0_assert_level0`. Without #668, their analysis supplies a false radius
zero at `75.16983285633648`; directly evaluating radius there returns `SM_ERR`,
not a valid zero. Rust adds a spurious report 14 after the already present kink
report 8. These two comparisons are explicitly known **Rust dependency bugs**,
not claimed as physical zero radii or C++ correctness defects. They remain in the
known list while #668 is unmerged, and must be removed after that dependency
lands. This PR stays draft and the unit stays partial.

A separate probe applied only #668's `local_solve1d.rs` change to the same #475
production implementation on main 82fb6910a. With the two now-fixed dependency
entries removed for that probe, all 2204 native cases produce 2199 bit-identical
passes and the five genuine C++ mismatches above; neither curve0 case has an
extra report 14. Their original kink report 8 is preserved. The solver patch is
not included in #677: each PR remains scoped to its own issue. Probe logs and
reports are under `/tmp/solidcore-475-*`; no temporary probe fixture is committed.

The full macOS --green suite has not been re-certified: the pre-existing native
reference defects #448/#449/#450 remain. GitHub CI requires maintainer approval.

Standalone validation on main d7abe913e plus #475: all 2074 kernel tests pass
(two existing integration tests remain ignored), fmt, default/native workspace
Clippy with -D warnings, wasm check and default difftest 46+1 tests pass. Native
`nurbs.b_spline_curve_p3` is partial: 2204 cases, 2197 bit-identical passes, five
C++ mismatches and two explicit #481 dependency mismatches, zero unexpected
failures or deferrals. `curve.line` passes 2103/2103 and `curve.ellipse_p1` passes
2031/2031, both bit-identical. Positive-control perturbation is SENSITIVE: four
regular-parabola cases and the straight-span/bend case, all five detected.

Rebased #677 onto latest fetched main e33985411 without changing its production,
test or fixture bytes. On that head, fmt, all 2076 kernel tests, default/native
workspace Clippy, wasm and default difftest46+1 tests pass. The native numerical
counts above were executed on d7abe913e, and the isolated combined solver probe
on 82fb6910a; they are not mislabeled as full native runs of the newer base.

The final metadata-only rebase to bcc2f80be retains main's rewritten STATUS and
its history archive, the findings status index at the end, and the refreshed
per-unit notes. Production/test/fixture bytes remain identical to the version
validated on e33985411; all unit/known-mismatch TOML parses. No additional Rust
or native execution on the metadata-only base is claimed.
