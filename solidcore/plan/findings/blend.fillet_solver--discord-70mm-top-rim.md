# blend.fillet_solver: Discord 70 mm top-rim case (2026-11-02)

Units: `blend.fillet_solver`, `step-brep`.
PR: https://github.com/tangency-labs/solidcore/pull/2390.

## Input and selection

The user supplied Claas Kuhnen's `fillet bug.step` from the Discord comparison
dated 30 October 2026 and confirmed the upper model's requested radius as 70 mm.
The lower screenshot's 2 mm request is a different model and is not covered.
The fixture is `crates/step-brep/tests/fixtures/discord-2026-10-30-fillet.step`,
SHA-256 `beb34135184eaed70872c314ba3a76b39c103c730dd79f3aea553c23978f71bb`.
It is AP214 in millimetres, exported by smlibpy/SMLib.

The reader produces one body, 15 faces, 39 edges and 26 vertices without skipped
geometry. The topology-first build reports no face failures. The cylinder radius
is 304.8 mm and its top is z = 458.266019821167 mm. The four convex top outer arcs
are STEP #163, #170, #185 and #198. The regression selects these geometrically.
This is an inferred selection: STEP does not preserve the original command, and
the screenshot does not establish which arcs were selected together.

## Reproduction and existing correction

On solidcore `23ea17c1c21a587e5bd50653e7c73c490f34bbbe`, fresh bodies were used
for each radius and flag profile. The four-arc operation fails with default fixes
at radii 1, 10, 20, 40, 60, 70, 80 and 100 mm. Preparation succeeds; `do_fillet`
returns status 1001 with error code 3 and comment
`Fillet Corner Error: Corner Edge Computation Failure`.
At R70, each of the first three arcs succeeds individually, while the fourth
fails. Thus these trials do **not** demonstrate a radius threshold or establish
the cause of the screenshot's visible distortion.

Delta reduction of the successful investigative flag profile leaves only
`Kernel::fixes.fillet_exact_round_classes` (WR-669). That flag alone makes the
four-arc R70 operation complete. The four resulting round surfaces are surfaces
of revolution. An 11 by 11 parameter grid on each lies within 2.6e-13 mm of the
expected torus:

```text
(sqrt(x*x + y*y) - (304.8 - 70))^2
    + (z - (458.266019821167 - 70))^2 = 70^2
```

The regression checks all 484 points against the radius at 1e-9 mm absolute.
This finite surface check is not a certificate for the trims, junctions or solid.
No new kernel flag or geometry algorithm is introduced. WR-669 classifies from
the supporting faces rather than the old edge-shape-driven try chain; this case
shows that its existing correction changes operation completion. The precise
failing branch inside the old corner construction has not been isolated.

## C++ control

The imported operand was written as native SMLib ASCII and read independently by
a standalone SM_API driver linked against the pinned prebuilt SMLib reference
(`upstream_commit=3fd5b9548d19aa8c12c29bc9637a96d42a06f8a5`, g++ 13.3,
`-ffp-contract=off`, `-fno-fast-math`). At R70 it selects the same four arcs
geometrically and `SmApiFilletEdges` returns 1001. Selecting only the fourth arc
also returns 1001. This confirms the default failure on the **imported operand**;
it does not independently validate STEP import or compare against the later
fork's opt-in implementation. No C++ output or state is supplied to Rust.
The same C++ driver's `ValidatePointers(SM_LEVEL_2)` produces 24 reports and
`AssertValid(SM_LEVEL_2, SM_WALK)` returns false on that operand. Thus failed
validation of the serialized imported operand also occurs in the pinned reference;
this still does not establish whether the original STEP model is defective.

## Unresolved validity

Before the fillet, `check_brep(Level2)` reports 24 failed edge/vertex tolerance
reports (approximately 0.0233 mm versus its 0.01 mm limit) and skips the next
geometry check. Its `failure_count()` is 25 including the synthetic skipped-check
line. The same counts remain after the flag-enabled operation.

Calling `assert_valid` directly as a separate diagnostic also returns false:
besides those tolerance reports it reports two `Face has Coincident Edges` and
eight `Bad Vertex Gap` failures on both operand and result. The regression does
not promote operation success to whole-body validity. Whether these arise from
the original model, its export, or import needs independent investigation.
No tolerance was changed and no healing/retry was used to obtain success.

Next work: establish input validity independently, recover the actual selected
arcs, and compare trimmed junctions against the Parasolid result. Do not declare
the Discord visual defect fixed from these status and surface samples alone.

## Whole-body investigation and valid reconstruction

A successful return with failed validation is not the acceptance criterion. Direct
projection of the supplied lower blend boundaries onto their supporting B-spline
surfaces finds gaps of approximately 0.0116--0.0117 mm, before any top fillet.
For example, source face 3's edges #125 and #141 have maximum sampled distances
0.01167218789487 and 0.01161402563689 mm. The corresponding boundaries on the other
three lower blends behave similarly; other boundaries on these surfaces are near
2e-13 mm. Five source circular edges also miss their shared vertices by
2.758772e-5 mm. The file declares 1e-7 mm uncertainty. This is not cured merely by
suppressing tolerance reports. Whether the lower surface approximation arose in
the original construction or its SMLib export remains unconfirmed.

The new regression reconstructs the intended solid using existing cylinder,
Boolean-difference and rolling-ball fillet operations. It retains the four actual
cutout centers from the file (including their small offsets), R89.38861960394327,
floor z189.75418806267385, and the main cylinder's R304.8 and height. Its lower
round radius is 241.50994792771928 minus the floor height, inferred from the
source's tangency height. These exact lower blends replace inconsistent exported
lower geometry; they are not a lossless healing of the original STEP.

Three existing opt-in corrections are used:

- `fillet_exact_round_classes` (WR-669).
- `fillet_exact_arc_sections` (WR-708).
- `swap_uv_keeps_analytic_definition`, so a UV swap during subsequent filleting
  preserves the surface's analytic definition.

Every Boolean cut, the lower-round operand, its STEP roundtrip, the R70 result
built from that reimported operand, and its final STEP roundtrip must pass
`check_brep(Level2)` with no failed reports. No healing or tolerance changes are
used. The reconstructed operand has 17 faces (one cutout spans a seam) rather
than the original 15; the final result on merged main `4e30eb2fd` has 20 faces, 55 edges and 37 vertices.
The original fixture and its failing default behavior remain covered separately.

## STEP export corrections

Whole-body roundtrips exposed three orientation losses in `brep_to_model`:

1. `face.faceuse` is an arbitrary side of a face. A Boolean cut may use the other
   side in the material shell. Export now uses the material region's faceuse,
   retaining the original selection for faces without a known material region.
2. The ordered edgeuses already describe an oriented loop, including inner loops.
   Exporting their loopuse orientation as a second reversal reverses holes again.
   The STEP face bound now keeps that directed traversal (`orientation = true`).
3. A kernel cone/cylinder may have swapped UV directions or be inside out. Each
   reverses its normal, while the exported axis/radius describe a canonical STEP
   surface. `ADVANCED_FACE.same_sense` now accounts for both reversals.

These are bridge corrections, not changes to the pinned kernel or its numerical
formulas. Independent smaller regressions roundtrip a blind cylindrical notch,
a through notch, and a central through hole, requiring clean Level2 validation
and unchanged topology counts. No new difftest parity claim is made.

The test recipe is not installed as automatic import repair. The original supplied
STEP still needs its inconsistent lower blends repaired or reconstructed before
this provides a valid complete-body R70 workflow. The separate 2 mm model and an
independent Parasolid comparison remain outside the supplied evidence.


## CI integration follow-up

Run 41459223900 on a0e1a2d ran 4,805 tests: 4,804 passed, including all
Discord/export regressions. The sole failure was the inherited exact-class
comparison `fork_099cf96_exact_class_cases`: its all-current-fixes profile ignored
`SurfaceKind::ProceduralBlend` and expected the fitted tee's historical
0.00147 mm error. This integration failure is also documented on PR #2384.

The grader now recognizes procedural rounds and requires that representation
when the current profile enables `fillet_round_as_definition`. The historical
default and exact-class-only marks are unchanged. The all-current-fixes tee must
instead pass the same volume and Level2 checks and a 1e-9 mm rolling-ball error
bound, with 169 surface/normal samples. The measured worst error is
1.0658141036401503e-14 mm. The wrong-kind negative test also rejects an analytic
round incorrectly labelled procedural. No production kernel code, enabled flags,
reference tolerance or case coverage is removed or relaxed.
