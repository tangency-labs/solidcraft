# blend.fillet_executive_p3: sphere-difference mass mismatch resolves with owned UV trimming (2026-10-26)

Updates: plan/findings/blend.fillet_executive_p3--sweep4-sphere-diff-3cylinders-and-brepforfillet2-all-edges.md

Unit: `blend.fillet_executive_p3`. PR: #1215.
Input: unchanged `sweep4-sphere_diff_3cylinders-all-r0.1` in
`fixtures/blend.fillet_executive_p3/sweep_round4.json`: file source
`Fillet/sphere_diff_3cylinders.smb`, every edge, circular G1 round, radius 0.1,
thumbweight 1.

The original finding records area 38235.606154303496 in C++ versus
38235.00487528196 in Rust, with identical sampled topology/geometry. PR #1215
makes the two `fix_uv_trim_curves_notified` trimming sites use the actual held UV
curve's owning-edge zone tolerance, matching `SmCurve::GetEdge` and
`SmTol::GetZoneTol3d(this)`. The B-spline trim uses that tolerance for mapped
endpoint tolerances and degenerate-span removal; standalone/system defaults and
the existing opt-in policies remain unchanged.

On head `fafae1ee868d9244eb41b47efd155cfda35c1dde` (main base
`80b67016b13870bf0584c329daf7f28a6d70b540`), the unchanged case now agrees:
all 1,621 floating values and 694 exact topology/integer values compare with zero
mismatches and `max_rel_err=0`. This was observed both locally on Apple-Clang ARM64
and in the canonical Linux sweep, CI run
https://github.com/tangency-labs/solidcore/actions/runs/40405182303
(report artifact `12692486826`). The sole canonical sweep failure was the stale
known-mismatch entry's fail-closed check: “the case is now pass: remove the entry.”

Remove only this case's entry from
`crates/difftest/known_mismatches/blend.fillet_executive_p3.toml`. Keep the fixture,
all comparisons, and the existing tolerances. The separate bottle mismatch remains
registered and unresolved by this addendum. Main `35239e97c` independently resolves
and removes `brepForFillet2`; the merge retains that resolution rather than restoring
its entry. No claim is made about which of the original three cone-face UV spans
first caused the mass difference; the full unchanged result record now agrees.
