# topology.trimming_tools: Projected loop endpoints on periodic seams (#530, 2026-10-25)

`topology.trimming_tools` deliberately fixes WR-440 (filed in #709 as WR-264) rather than copying the reference
checker's use of a projected UV representative as an exact sheet choice. The
endpoint provenance comes from `edge_uv_extremes`: an existing trim has end weights,
while a vertex projection has none. Only a projected endpoint is lifted to the
nearest natural period, and only in a direction `Surface::is_closed` certifies. The
10% UV threshold, singularity rules, 3D gap threshold and comparator stay unchanged.
Both exact trim ends keep their original sheet. Surface points are evaluated at the
original parameters: lifting an endpoint outside a B-spline's natural domain must
not clamp away a real physical gap.

The Rust regression builds actual full-turn analytic rotational sweeps of a line
(cylinder, cone, reversed cone). Fully projected loops were already healthy in the
port. Retaining one exact vertical trim at the far seam while its neighbours project
to the near seam makes the old cylinder checker return 1001; this is the failing
before reproduction. All three shapes now pass their mixed-seam configurations.
Six short physical-gap controls still return closure 3 and reports; exact-trim
polygons pass, and deliberately misplaced exact seams still fail. This does not
claim that the three entirely untrimmed Rust sweeps failed before.

Native input setup adds `BCheckLoop.project`, which deletes one selected trim pair
on both sides before calling the unchanged checker. All nine previous fixtures
are preserved. Twenty-four new cases matched C++ before the production change.
Afterwards twenty still match (public torus/revolution/plane plus closed-U and
closed-V degree-one square-tube faces); these include nonperiodic controls. The
four cone.smb selections differ as follows, inspected from raw outputs:

| Case suffix | C++ status / closure | Rust status / closure | Meaning |
|---|---|---|---|
| `cone_eu0` | 0 / 1 | 0 / 0 | seam-only closure classification |
| `cone_eu1` | 1001 / unset | 0 / 1 | false UV rejection removed; singularity retained |
| `cone_eu2` | 1001 / unset | 0 / 0 | false UV rejection removed |
| `cone_eu3` | 0 / 1 | 0 / 0 | seam-only closure classification |

For eu1/eu2 the C++ report is loop list 3, test 1: U gap
`15.30733729460359`, threshold `1.5307337294603591`, exactly one natural period.
There is no failed 3D-gap report. All four are explicit per-case known mismatches
(WR-440 (filed in #709 as WR-264)), so the unit is **partial**; they are two erroneous rejections and two
closure-only differences, not four refused bodies. No case was hidden or deferred.

Validation on `8af64d4928cb2264068cc4828fd7d77628db0102` plus this change: fmt,
default/native workspace Clippy with warnings denied, 2081 kernel unit tests, five
integration passes (one existing partial-sphere/torus case ignored), wasm and 48
runner tests pass. The six-unit native matrix has 16910 cases: 16674 matching, six
explicit known differences (four above, two existing main Rust/harness MoveSeam
limitations), 193 UNPORTED deferrals and 37 UB-137 native guard rejections, with no
other comparison failures. The trimming unit is rerun strictly after recording
its four known cases and partial metadata. Full macOS --green remains limited by
the separate reference issues #448–450; canonical CI is not claimed.


Revalidation for PR #709 on `6766fe5134113aec2e86f27eeb5c651d398c617d` (after #704/#706): all five
implementation/shim/fixture SHA-256 hashes are unchanged. Fresh gates pass with
2082 kernel tests, five integration passes (one existing ignored case), 48 runner
tests, wasm and default/native workspace Clippy. A fresh six-unit native rerun
retains exactly the totals above, with zero unexpected failures; the 20 matching
new seam controls again detect all 20 perturbations.

Opt-in (2026-10-26): following the `UpstreamFixes` precedent (#1077), the lift is
gated by `Kernel::fixes.check_loop_projected_seam_ends`, off by default. With the
default the port reproduces the pinned C++ `CheckLoop`, so the four `cone.smb`
selections above match the reference (two SM_ERR 1001 statuses, two closure-1
classifications) and need no known-mismatch entries; the unit stays green. With the
field on, the behaviour described above holds
(`issue_530_projected_rotational_sweep_loops_cross_periodic_seams`).
