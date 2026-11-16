# General: WR-490 addendum: the record's boolean triggers agree by default at v1.0.1 (2026-11-02)

Updates: plan/findings/general--flag-pin-wr490-ellipse-trim-same-since-v1-0-1.md

The record waited on WR-490's boolean triggers: #868's equal rods and the steel-026 tubes. Both were measured at operation level at e0d609258, on x86-64 Linux:

- **Rust:** the default kernel and with `ellipse_trim_seam_lines_span_cone_height` set, volumes from `brep_volume` at 1e-8.
- **C++:** the pinned v1.0.1 (`v1-3fd5b9548d19-726643fb17a52b7d`), through the `boolean.api_brep` runner (`measured` and `checked` calls, volume at the runner's 1e-6). The cases were a scratch fixture and were not committed.

The geometry is #868's:
- Cylinder C: r 44.45 along x (-200..200).
- Rods A and B: r 24.15 at +-45 deg in the XZ plane, axes meeting at (0, 0, 30), each 240 long from 40 before that point.
- Each body is built as `SmApiCreateCylinder`, turned `seam` about its own axis, about Y onto its axis, and moved.
- The tubes are CHS 88.9x5 for the chord and CHS 48.3x4 for the braces, each the outer cylinder minus the inner one.

The closed forms are by slices y = const (each section a union of rectangles, integrated in y):
- rods: 3137849.373374 (#868 gives 3137849.373);
- tubes as they are, `(chord + A) + B`: 779093.246074;
- braces coped to the chord's solid, then `chord + (A' + B')`: 730345.211757.

`crates/solidcore/tests/wr490_equal_rods_and_tubes.rs` asserts the rods and tubes rows below with the flag off and on.

## Rods

Rust, flag off. The flag on gives the same bits, except at seam 60 `(C u B) u A`, which gives 3137849.368603 (4.6e-5, 1.5e-11 relative):

| seam | (C u A) u B | (C u B) u A | C u (A u B) |
|---|---|---|---|
| 0 | 3137849.378981 | 3137849.378936 | 3137849.378387 |
| 35 | 3137849.363913 | 3137849.363931 | 3137849.363448 |
| 60 | 3137849.368605 | 3137849.368649 | 3137849.368667 |
| 90 | 3137849.377541 | 3137849.377371 | 3137849.376859 |

- **Rust:** every result is a manifold solid within 3.2e-9 of the closed form. `AssertValid` passes everywhere. At seam 0 `(C u B) u A` and both orders at seam 60, the check reports an edge/vertex gap of 5.04e-6 or 5.13e-6 against the tolerance 5e-6 (`SmBrep` tests 0 and 2).
- **C++ v1.0.1:** the Rust default's status, topology, volume, face areas and check, within 1e-9, in all 12 cases. The volumes at 1e-6 are 3137846.90 to 3137850.44.
- **WR-490's symptom is gone:** at the old pin, `(C u A) u B` gave `C u A` (2816296.79) at seams 0, 35 and 60.

## Tubes as they are, #868's `(chord + A) + B`

| seam | Rust (flag off = on) | manifold | check (Rust = C++) | C++ at 1e-6 |
|---|---|---|---|---|
| 0 | 779093.262934 | yes | `SmBrep` 12 | 779092.880135 |
| 35 | 779093.242062 | yes | `SmBrep` 12, 2 (x2) | 779093.189404 |
| 60 | 779093.243035 | yes | `SmBrep` 12, 0 (x3), 2 | 779092.491944 |
| 90 | 779093.296233 | yes | `SmBrep` 12, 2 (x2) | 779091.438919 |

- **Volumes:** within 6.4e-8 of the closed form at every seam.
- **WR-490's symptom is gone:** at the old pin the union was refused (-1100), or at seam 90 came out 4592 mm^3 too large.
- **Check:** at every seam, flag off and on, on both engines, it reports `SmBrep` test 12: an inner shell not inside its outer shell. That is a check failure on a body with the right volume, and it is not WR-490's (the flag changes nothing).
- **Steps:** `A u B` of the tubes and `chord u A` are manifold solids with a clean check on both engines.

## Braces coped to the chord's solid, then `chord + (A' + B')`

This is the central probe's variant, not #868's recipe.

| seam | Rust (flag off = on) | manifold | check (Rust = C++) | C++ at 1e-6 |
|---|---|---|---|---|
| 0 | 730345.224778 | yes | `SmBrep` 12 | 730344.841855 |
| 35 | **-4592.382125** | **no** | `SmTrimmingTools` 3, 4 (`ValidatePointers`: loop orientation, zero-area loop) | -4592.382184 |
| 60 | 730345.207586 | yes | `SmBrep` 12 | 730344.455717 |
| 90 | **-4592.381577** | **no** | `SmBrep` 8, 12 (`m_pInfiniteRegion` incorrect) | -4592.381577 |

- **Steps:** the coped braces (102268.84 each) and their union (203185.96) are manifold solids with a clean check at every seam. Only the last union fails.
- **C++ v1.0.1 fails the same way** at seams 35 and 90, so this is an upstream defect of v1.0.1, not a parity gap. It is not WR-490's: the flag changes nothing, and the rods and uncoped tubes are right.
- **Cause:** not traced, so there is no `plan/upstream-issues/` record yet. The test keeps both seams in `coped_tubes_join_known_failing`, which fails once they come out right.
- **The number:** |volume| 4592.38 is #868's "4592 mm^3 too large" at seam 90. That is noted, not explained.
- **Engine values:** in the last union at seams 35 and 60, the engines agree in status, topology, manifold and check. Four face areas (and at 35 the volume) differ by up to 1.2e-7 relative at 35, and two face areas by up to 6.2e-9 at 60. The other 62 runner cases agree within 2.4e-14.
- **Harness:** these two cases have operands 5 deep, above the runner's `MAX_DEPTH` of 4, which was raised for this measurement only. They are in no gate. The difference is not traced either.

## Conclusion

- **The flag is inert** at v1.0.1 on its record's triggers. It gives the default's body everywhere, and changes one rods volume by 1.5e-11 relative.
- **No fork counterpart:** the fork's replay/1.0.1 dropped the change (the 2026-10-31 addendum).
- **Retiring the flag:** by the record's "Waits on", it can be retired. That is the maintainers' call. Retiring it means three things:
  - delete the field and its branch in `ellipse_trim_intervals`;
  - delete the flag-on unit test `seam_crossing_below_a_trimmed_domain_keeps_the_ellipse` (its default twin stays);
  - SolidCraft's preset stops setting the field.

  This addendum changes no code.

**Status:** `crates/solidcore/tests/wr490_equal_rods_and_tubes.rs` (#1954's WR-490 slice).
