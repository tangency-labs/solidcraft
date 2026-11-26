# blend.fillet_solver_p2: WR-592's corner round still fails SmSurfOfRevolution #7 on the rebuilt bar, as the C++ does (2026-10-31)

- **Unit:** `blend.fillet_solver_p2` (`FilletSolver::torus_fillet_geom_parts`,
  `CalcTorusFilletGeom`), with `surface.surf_of_revolution_*` (`RebuildPolarConverter`,
  `CreateIsoParametricCurve`) and `topology.assert_valid` (the check).
- **Context:** PR #1814 ported the fork's tangency-labs/smlib@994f129 as the opt-in
  `Kernel::fixes.fillet_torus_rail_on_axis` (WR-592). The fork's capi test
  `corner_ball_after_a_round_is_exactly_singular` asserts the result passes
  `smlib_brep_check(b, 2)`. A post-merge review found that on the port's test geometry
  (`crates/solidcore/tests/wr592_torus_rail_on_axis.rs`) the result with the flag on
  still fails `SmSurfOfRevolution::Test #7` (Parameterization), and nothing recorded it.
- **Conclusion:** the port matches the pinned C++ bit for bit, with the flag off and
  on. The #7 failure is the pinned SMLib's own roundoff on this input. It is not a
  missing fork fix and not a port divergence. The fork's test passes because its input
  file is a different body.

## Evidence

A throwaway C++ probe linked against the harness's pinned build (`bin/cpp/lib`,
`3fd5b95`, `-ffp-contract=off`) built the test's bar (`SmApiCreateLineSegment` x4 of
the test's `PROFILE`, `SmApiCreateLinearSweep` 10 up, capped). It rounded the edge
nearest (20, 0.0579, 10) with `SmApiFilletEdges(r 2, circular, G1, 1.0)`, then the three
edges nearest the test's points together. It ran the capi check sequence:
`ValidatePointers(level 2)`, then `AssertValid(level 2, SM_WALK)`. A second binary
linked in `SmFilletSolver.cpp` with the fork's 994f129 loop added before the section is
built (the fork's own `sUnitAxis`, `sAxisPnt` and `dCenterTol = GetThisApproxTol3d()`
lines, without its other changes). That loop is "WR-592 alone", as the port's flag is.

The corner's surface of revolution (`swap_uv` true):

| run | STEP u interval | polar circle STEP interval | check (capi sequence) |
|-----|-----------------|----------------------------|-----------------------|
| pinned C++ | `[0, 89.999999979658995]` | `[1.1443758734664252e-09, 89.999999980803494]` | ValidatePointers: `SmTrimmingTools #2` loop gap; AssertValid when run anyway: `SmSurface #5`, `SmSurfOfRevolution #7`, `SmTrimmingTools #2` |
| Rust, flag off | `[0, 89.9999999796589947]` | `[1.14437587346642520e-9, 89.9999999808034943]` | `SmTrimmingTools #2`, AssertValid skipped |
| pinned C++ + 994f129 hunk | `[0, 89.999999979658654]` | `[1.1940644274172826e-10, 89.999999979778153]` | `SmSurfOfRevolution #7` only |
| Rust, flag on | `[0, 89.9999999796586536]` | `[1.19406442741728260e-10, 89.9999999797781527]` | `SmSurfOfRevolution #7` only |

The surface frame is identical too. With the flag on the origin is
(0.74477916181956583, 2.0579000000018697, 7.99999999937629) and the x axis is
(7.2884811100730003e-12, -1, 3.403957914434617e-10) in both. Removed volume
(`SmApiBrepComputeVolume` / `brep_volume`, accuracy 1e-12) is 20.979356723547 (off)
and 20.979356723289 (on) in both. At accuracy 1e-8 it is 20.979356756483 and
20.979356756226 in both.

The same two C++ binaries also read the fork's own input. They used the capi's
`smlib_flat_impl::ReadText` and `BrepFromFlat` from `capi/src/smlib_flat.cpp` at 994f129,
compiled against the pinned headers, then took an `SmApiBrepCopy` and ran the same three
picks and `SmApiFilletEdges`:

| run on `corner-after-round.txt` | polar circle STEP min (deg) | check |
|---------------------------------|-----------------------------|-------|
| pinned C++ | `-1.1292170910250198e-11` | AssertValid: `SmSurface #5` only (the fork's "Before: 1 failed check") |
| pinned C++ + 994f129 hunk | `4.4651122850584335e-11` | sound (the fork's "After: sound") |

There the removed volume (1e-12) is 20.979356724813, which the fork prints as
20.979356725. So pinned SMLib with WR-592 alone reproduces the fork's test on the fork's
input. No other fork commit is needed (candidate (b) of the review is ruled out). The
fork's input is not the port's bar. Its first round has nine faces, its end face is
on a 183.6-degree cylinder, and its residues differ (the corner sits at
x = 0.48929407752807941, not 0.4893 + O(1e-10)).

## Cause

`SmSurfOfRevolution::AssertValid` #7 calls `HasSameParameterization(SM_SP_U, ..,
pPolarCurve)`. That test requires the polar circle's STEP interval to equal the
surface's STEP u interval within `SM_EFF_ZERO * (1 + 90) = 9.1e-11` (degrees here).
`RebuildPolarConverter` takes the circle from `CreateIsoParametricCurve`. Because the
collapsed rail makes the v-min iso degenerate, that iso is taken at 45% of the domain.
`CreateIsoParametricCurve` measures the circle's start as `CCWAngleBetween(x axis,
NURB point at u-min)`. The frame's x axis and the NURB's u-min point come out of different arithmetic. The
frame comes from the generator's 45% point in `CreateCanonical`. The NURB is the
generator turned to angle 0 and swept, and both are then moved and trimmed by the
fillet. They disagree by about 2e-12 rad. Which step contributes most was not traced. That is 1.19e-10 degrees on this bar and
4.5e-11 degrees on the fork's input. The check's tolerance sits at that roundoff level,
so whether #7 passes depends on the input. The fork has the same fragility.

## Also checked

- **Check level:** the fork's `sound()` calls `smlib_brep_check(b, 2, ..)`, which is
  `ValidatePointers` then `AssertValid(SM_WALK)` (capi `smlib_check.cpp` at 994f129,
  the warning SmBrep #21 not counted). `check_brep(.., CheckLevel::Level2)` is the same
  sequence, and the reports agree line for line with the C++ above. Candidate (a) of
  the review, a check that differs, is ruled out.
- **The review's volume point:** the fork's `near(got, want, 1e-8)` is relative
  (`|got - want| <= 1e-8 * |want|`, about 2.1e-7 here). The 3.3e-8 the review measured
  came from the test's volume accuracy of 1e-8. At the fork's 1e-12 the port is
  1.3e-10 from 20 r^2 (1 - pi/4) + r^3 (1 - pi/6) and equals the C++. The test now
  measures at 1e-12 and asserts the fork's relative 1e-8.
- **SolidCraft preset (aside):** with `UpstreamFixes::solidcraft()` the polar circle
  still starts at 1.194e-10 degrees, so #7 would still fail. The check stops earlier,
  though. The *first* round already fails `ValidatePointers` with eight
  `SmBrep::Test #61 Valid: Bad Edge fails pE->ValidateGeometry(FALSE)`. A per-flag
  bisect on the first round names `fillet_refuse_non_manifold_result` (WR-212, a port
  fix in the preset's `reported` list) as the only flag that does this. The pinned C++
  first round is sound. That is a separate follow-up and is not investigated here.

## Test

`crates/solidcore/tests/wr592_torus_rail_on_axis.rs` now asserts the first round's
report is empty. With the flag on, it asserts exactly the `SmSurfOfRevolution #7`
line. With the flag off, it asserts the `SmTrimmingTools #2` line and
"AssertValid skipped: ValidatePointers failed". It also asserts the fork's volume
criterion at accuracy 1e-12.
